# Observability

This document lists the traces, logs, metrics, dashboards, and alerts. With them, an operator can answer four questions from Grafana alone. Which block is each job on, and is it backfilling or live? Which stage limits throughput? Is the API answering? Are the numbers it serves still correct?

## Set up tracing once

The `bt-telemetry` crate builds the `tracing` subscriber. The binary calls it once, at the start of `main`.

- **Format.** JSON lines in production (`telemetry.log_format = "json"`). Readable, colored lines in development (`"pretty"`).
- **Filter.** `RUST_LOG` sets it, with the default `info,bt_=debug,sqlx=warn`. Every workspace package is named `bt-<name>` (`bt-chain`, `bt-ingest`, `bt-store`, `bt-api`, and the others, in directories `crates/<name>`), so its library crate is `bt_<name>` and its log targets start with `bt_` ([D32](../decisions.md#d32-name-the-crates-bt-name)). `EnvFilter` matches a directive's target as a prefix of the event's target (`tracing-subscriber-0.3.23/src/filter/env/directive.rs:246`), so `bt_=debug` covers every crate in the workspace and no dependency. `sqlx=warn` keeps `sqlx` quiet, because it logs every query at `info`, which is too much for production.
- **Export.** With `telemetry.otlp_endpoint` set, a `tracing-opentelemetry` layer sends spans to an OpenTelemetry collector. Without it, spans appear only in the logs.

Library crates use `tracing` macros and `#[tracing::instrument]`. They never install a subscriber.

## Log every commit

Every commit logs one `info` line, so `docker compose logs ingest` shows each job's progress with no dashboard.

```
worker=epochs mode=backfill from=9449501 to=9450000 head=9491345 lag=41345 rate=21.9/s eta=31m28s
```

A job logs once when it switches mode, as `worker=prices mode=live block=9491343`. It logs the switch back to backfill the same way.

## Name spans after the work

The span tree follows the runner, so a slow batch shows where it spent its time.

```
batch{worker="epochs" from=9487621 to=9488120}
├── read{block=9487621 spec=490}
│   └── rpc{method="state_queryStorageAt"}
└── write_batch{rows=1830}
    ├── insert{table="epoch_dividend" rows=1830}
    └── checkpoint{worker="epochs" block=9488120}
```

Every span that handles a block records `block` and `spec` as fields. A log line inside the span then carries both, with no extra code. `#[instrument(skip_all, fields(block = %at.number))]` keeps large arguments out of the span.

At backfill rates, one span per block is too many. Create `read` and `rpc` spans at `debug` level, and `batch` and `write_batch` at `info`. The default filter records the `debug` spans too, which helps in development. Production sets `RUST_LOG=info,sqlx=warn`, which records one span per batch.

### Trace a request

The API's `TraceLayer` creates one span per request.

```rust
TraceLayer::new_for_http().make_span_with(|req: &Request<_>| {
    let route = req.extensions().get::<MatchedPath>().map_or("unmatched", MatchedPath::as_str);
    let request_id = req.headers().get("x-request-id").and_then(|v| v.to_str().ok());
    tracing::info_span!("http", method = %req.method(), route, request_id, status = tracing::field::Empty)
})
```

`on_response` records `status` and the latency. Query spans from `crates/store` nest inside the request span, so a slow request shows which query took the time.

## Export these metrics

The `metrics` crate records the values, and `metrics-exporter-prometheus` serves them on `telemetry.metrics_addr`. It defaults to `0.0.0.0:9100`, where `ingest` exports, and `serve` overrides the default to `0.0.0.0:9101`, so both processes can run on one host ([D31](../decisions.md#d31-export-ingest-metrics-on-9100-and-serve-metrics-on-9101)). Every name that the indexer exports has the prefix `bt_`. The database dashboard also reads `pg_*` metrics, which `postgres_exporter` exports. Every label has a fixed, small set of values. No label holds an account, a hash, or a raw path.

The `worker` label names who made the call or the commit. That is a job, `prices`, `validators`, `epochs`, `baskets`, or `identities`, with `events`, `subnets`, and `positions` in release 2, or `head` for the head tracker's node calls.

The node metrics take `worker` from the `Chain` value that makes the call, because `chain.block_ref(n)` and `job.read(&chain, at)` take no worker argument. `main` builds one `Chain` per job with `chain.for_worker(J::NAME)`, and one for the head tracker with `chain.for_worker("head")`. They share the source, the metadata cache, and the limits behind `Arc` ([D21](../decisions.md#d21-call-the-node-in-two-layers), `spikes/crates/runner-live`, `wiring`).

### Progress

These metrics carry the same numbers as the `checkpoint` and `chain_head` tables. [Ingestion](03-ingestion.md#see-where-every-job-is) describes the tables, `bt-indexer status`, and `GET /status`.

| Metric | Type | Labels and meaning |
|---|---|---|
| `bt_chain_finalized_block` | gauge | The finalized head |
| `bt_checkpoint_block` | gauge | `worker`. The checkpoint's `last_block` |
| `bt_start_block` | gauge | `worker`. The checkpoint's `start_block`, where the job's history starts |
| `bt_lag_blocks` | gauge | `worker`. The newest block that the job's schedule selects at or below the finalized head, minus `last_block` |
| `bt_worker_mode` | gauge | `worker`, `mode` (`backfill` or `live`). 1 for the current mode, 0 for the other |
| `bt_last_commit_timestamp_seconds` | gauge | `worker`. Wall-clock time of the last commit |
| `bt_blocks_processed_total` | counter | `worker`. Increases by the checkpoint's advance at each commit |
| `bt_job_interval_blocks` | gauge | `worker`. The schedule interval in blocks, which is 1 for `prices` and `epochs`, 100 for `validators` and `identities`, and 500 for `baskets` |
| `bt_job_concurrency` | gauge | `worker`. The job's current `concurrency`, which changes with the mode |
| `bt_runtime_spec_version` | gauge | The spec of the finalized head |
| `bt_gap_blocks` | gauge | `worker`. Missing blocks the hourly gap check found below the checkpoint. It should always be 0 |

`bt_lag_blocks` counts from the newest block the schedule selects, not from the raw head. A `validators` job that has committed block 9,491,300 at head 9,491,345 has a lag of 0, because its next block is 9,491,400. Without that rule, a 100-block job would show a lag between 0 and 99 in steady state, and `IndexerLagging` would fire every cycle.

`bt_blocks_processed_total` counts chain blocks, whether the schedule selects them or not, so its rate has the same unit as the lag. ETA is lag divided by that rate.

At start, each job sets `bt_last_commit_timestamp_seconds` from its checkpoint row's `updated_at`, not from the start time. A restart therefore does not reset the age, and a process that crashes and restarts in a loop still fires `IndexerStalled`.

### Runner

| Metric | Type | Labels |
|---|---|---|
| `bt_pipeline_stage_busy_seconds` | histogram | `worker`, `stage` (`read` or `write`) |
| `bt_pipeline_send_wait_seconds` | histogram | `worker`, `stage` (`read`) |
| `bt_batch_rows` | histogram | `table` |
| `bt_batch_commit_seconds` | histogram | `worker` |

Each stage's time splits into three parts. The read stage is busy while it waits for the next ordered read to finish, waits on send while the channel to the writer is full, and is idle while it waits for a new head. The writer is busy while it builds and commits a batch, and idle while it waits on its channel. `rate(…_sum[5m])` of each histogram is therefore the fraction of time in that state, between 0 and 1.

### Node and database

| Metric | Type | Labels |
|---|---|---|
| `bt_rpc_requests_total` | counter | `worker`, `method`, `outcome` (`ok`, `retry`, `timeout`, or `error`) |
| `bt_rpc_request_seconds` | histogram | `worker`, `method` |
| `bt_rpc_in_flight` | gauge | `worker` |
| `bt_rpc_permit_wait_seconds` | histogram | `worker` |
| `bt_rpc_disconnects_total` | counter | none. `Chain::call` adds one for each `ChainError::Disconnected` result, one per call in flight when the socket dropped. A reconnect with no call in flight is not counted, because subxt exposes no reconnect counter or callback (`spikes/crates/chain-errors`, `disconnect-counter`) |
| `bt_db_pool_connections` | gauge | `pool` (`ingest` or `api`), `state` (`idle` or `used`) |
| `bt_db_query_seconds` | histogram | `query`. A fixed name per repository function |

`bt_rpc_in_flight` and `bt_rpc_permit_wait_seconds` carry `worker`, so the dashboard shows which job is making calls and which one waits. `bt_rpc_permit_wait_seconds` covers the whole wait in front of a call, first for a `chain.rpc_max_in_flight` permit (default 16) and then for a token from the `chain.rpc_max_per_second` bucket (default 120), as [D23](../decisions.md#d23-cap-the-call-rate-with-a-one-token-bucket-not-only-the-calls-in-flight) sets them. In a test capped at 1 call a second, the second call's wait read 1 s (`spikes/crates/runner-live`, `wait-metric`). That wait is outside `chain.rpc_timeout`, which covers the call only. A timed-out call is recorded with `outcome="timeout"` and its full duration, so a hung node shows up in the latency histogram.

`bt_rpc_in_flight` counts calls in progress, not permits held. A permit that the semaphore has granted to a waiter that no task polls does not show in it, and neither does a permit held across the send to the writer. In a test with 8 permits, a job whose reads were not spawned showed 2 calls in flight while it held 6 permits, and a job that held its permits across the send showed 0 while it held all 8 (`spikes/crates/runner`, `read-starvation`, `two-jobs`). The runner spawns every read so that no permit is lost this way.

The starvation test in [quality](09-quality.md#prove-that-a-stalled-writer-does-not-starve-other-jobs) therefore checks the other job's rate and the free permit count as well as this gauge. Under a rate cap the rate alone can hide lost permits, which only the free permit count shows (`spikes/crates/runner-live`, `stalled-neighbour`).

### API

| Metric | Type | Labels |
|---|---|---|
| `bt_http_requests_total` | counter | `route`, `method`, `status` |
| `bt_http_request_seconds` | histogram | `route`, `method` |
| `bt_api_ready` | gauge | 1 when `/readyz` would answer 200, 0 otherwise. The API recomputes it every 10 s |

`route` is the matched route template, such as `/subnets/{netuid}/validators`, or `unmatched` when no route matched. Shed requests count as `status="503"` on their route. The exporter renders these names and labels as listed (`spikes/crates/api`, `metrics-names`).

The exporter sets explicit buckets for `bt_http_request_seconds`, because `ApiLatency` reads its `_bucket` series. Without set buckets, `metrics-exporter-prometheus` 0.18.3 renders a histogram as a Prometheus summary, with quantile series only (`spikes/crates/api`, `metrics-buckets`).

```rust
PrometheusBuilder::new()
    .set_buckets_for_metric(Matcher::Full("bt_http_request_seconds".into()), &[0.005, 0.025, 0.1, 0.5, 1.0, 2.5, 10.0])?
```

The runner histograms are read through `_sum`, which a summary also has. A rule that reads another histogram by `_bucket`, such as `bt_position_drift_alpha` in release 2, needs its own `set_buckets_for_metric` call with a bucket at the bound that rule uses.

### Release 2

| Metric | Type | Labels |
|---|---|---|
| `bt_events_decoded_total` | counter | `pallet`, `variant` |
| `bt_reconcile_open_requests` | gauge | |
| `bt_position_drift_alpha` | histogram | The absolute drift per settle, in whole alpha. The folder stores drift in raw units of 10^-9 alpha and divides by 10^9 for this metric |
| `bt_positions_settled_total` | counter | `outcome` (`match` or `corrected`) |

## Provision the dashboards from files

Grafana loads its data source and its dashboards from `deploy/grafana/provisioning/` at start. The dashboard JSON lives in the repository, so `docker compose up` shows the dashboards with no clicks, and a change to a panel is a reviewed diff.

There are three dashboards.

1. **Indexer.** It opens with the Progress row. Below that are blocks per second per job, the runner panels, node request rate and latency per job, the total node request rate against `chain.rpc_max_per_second`, and RPC calls in flight per job.
2. **API.** Requests per second and p50, p95, and p99 latency per route, error rate by status, 503 responses from load shedding, and `bt_api_ready`.
3. **Database.** Pool use, query latency per query, hypertable size, and compression ratio. The last two come from `postgres_exporter` with a custom query on `hypertable_detailed_size`.

### The Progress row shows every job at a glance

The Progress row has one stat panel per job. Each panel shows five values, from these expressions.

| Value | Expression |
|---|---|
| Block | `bt_checkpoint_block{worker="$w"}` |
| Mode | `bt_worker_mode{worker="$w", mode="live"}`, mapped to `live` for 1 and `backfill` for 0 |
| Lag | `bt_lag_blocks{worker="$w"}` |
| Percent done | `100 * (bt_checkpoint_block - bt_start_block) / clamp_min(bt_checkpoint_block + bt_lag_blocks - bt_start_block, 1)` for `worker="$w"` |
| ETA | `bt_lag_blocks / rate(bt_blocks_processed_total[5m])` for `worker="$w"` |

`bt-indexer status` is the reference for percent done. It follows [D20](../decisions.md#d20-measure-lag-and-readiness-against-the-schedule), which gives 100 when the divisor is not positive, as it is for a job that has committed exactly its first snapshot with no lag. The panel's expression clamps that divisor to 1 and shows 0 % in that case.

The panel turns red when the job's `IndexerStalled` or `IndexerLagging` alert fires.

## Find the slow stage

The runner for each job has three stages. They are the reads, the writer, and the node behind the reads. The limit is the stage that is busy close to 100% of the time while the stage after it does not block it. Start at the writer and walk backwards. Each rule names one cause.

1. If `rate(bt_pipeline_stage_busy_seconds_sum{stage="write"}[5m])` is close to 1, the writer is the limit. The database is the cause. Raise the batch size or make the inserts faster.
2. Otherwise, if `rate(bt_pipeline_stage_busy_seconds_sum{stage="read"}[5m])` is close to 1 and `rate(bt_pipeline_send_wait_seconds_sum[5m])` is close to 0, the reads are the limit. Find the cause inside the reads.
   1. If `bt_rpc_permit_wait_seconds` is high for the job, a global RPC cap is the limit. When `sum(rate(bt_rpc_requests_total[1m]))` sits at `chain.rpc_max_per_second`, the rate cap is the limit. Otherwise `chain.rpc_max_in_flight` is. Raise either only if the node can take more. The keyed archive node allowed 200 calls a second per key, and 133 calls a second ran clean for 60 s, measured on 2026-11-12 ([D23](../decisions.md#d23-cap-the-call-rate-with-a-one-token-bucket-not-only-the-calls-in-flight)).
   2. If permit wait is 0 and `bt_rpc_in_flight{worker}` equals `bt_job_concurrency{worker}`, the job's own concurrency is the limit. Raise `concurrency` for that job.
   3. If permit wait is 0 and `bt_rpc_in_flight{worker}` stays below `bt_job_concurrency{worker}`, the node's latency is the limit. Check `bt_rpc_request_seconds` for the slow method.
3. Otherwise, both stages are mostly idle. The job is live and waits for the chain. Nothing needs tuning.

Rule 2 never fires when rule 1 does. The read stage waits on send only when the channel to the writer is full, and the channel fills only while the writer is busy.

Change one setting at a time, and compare blocks per second over at least 10 minutes before and after.

## Alert on symptoms

Prometheus loads the rules from `deploy/prometheus/alerts.yaml`. The scrape configuration names the ingest process `bt-ingest` and the API process `bt-api`. Each alert ships with a section in `docs/runbook.md` that says what to check first. An alert without a runbook section does not merge.

| Alert | Expression | For | Severity |
|---|---|---|---|
| `IndexerDown` | `up{job="bt-ingest"} == 0` | 2m | page |
| `IndexerStalled` | `time() - bt_last_commit_timestamp_seconds > clamp_min(5 * 12 * bt_job_interval_blocks, 120)` | 1m | page |
| `IndexerLagging` | `bt_lag_blocks > 50 and on(worker) bt_worker_mode{mode="live"} == 1` | 5m | page |
| `CompatReadyFalse` | `bt_api_ready == 0 or up{job="bt-api"} == 0` | 5m | page |
| `ApiErrorRate` | `sum(rate(bt_http_requests_total{status=~"5..", status!="503"}[5m])) / sum(rate(bt_http_requests_total[5m])) > 0.01` | 5m | page |
| `ApiOverloaded` | `sum(rate(bt_http_requests_total{status="503", route!="/readyz"}[5m])) / sum(rate(bt_http_requests_total[5m])) > 0.01` | 10m | ticket |
| `BlockGap` | `bt_gap_blocks > 0` | 0m | ticket |
| `ApiLatency` | `histogram_quantile(0.99, sum by (le, route) (rate(bt_http_request_seconds_bucket[5m]))) > 1` | 10m | ticket |
| `PositionDriftHigh` (release 2) | `1 - sum(increase(bt_position_drift_alpha_bucket{le="1"}[1d])) / sum(increase(bt_position_drift_alpha_count[1d])) > 0.01` | 1h | ticket |

- **`IndexerDown`** covers a process that has exited. Prometheus cannot scrape its metrics, so no lag or staleness rule can fire for it.
- **`IndexerStalled`** fires per worker when the last commit is older than five schedule intervals at 12 s per block, and never sooner than 120 s. That is 120 s for `prices` and `epochs`, 6,000 s for `validators` and `identities`, and 30,000 s for `baskets`. It covers every worker, including `positions` in release 2. It also covers a crash loop, because the commit age survives a restart, and a decode error, because that error stops the process before it commits.
- **`IndexerLagging`** fires only for a job in live mode. A job in backfill is far behind by design. A 100-block job's lag jumps to 100 at each scheduled block, so the alert stays quiet only if that snapshot commits within 5 minutes. A `validators` snapshot took 131 s at 8 in flight and 451 s at 1.
- **`CompatReadyFalse`** fires when `/readyz` has been false for 5 minutes, or when the API process is down. [API](05-api.md#get-status) defines readiness.
- **`ApiErrorRate`** counts every 5xx except 503. A 503 is the API protecting itself, by shedding load or by reporting the database unavailable, so it is not a fault in the code.
- **`ApiOverloaded`** counts the 503 responses of every route except `/readyz`, whose 503 means not ready and belongs to `CompatReadyFalse`. Sustained shedding means the API needs more capacity, which is work for the next day and not a page. A database that is down pages through `CompatReadyFalse`.
- **`PositionDriftHigh`** fires when more than 1% of the settles in the last day corrected a position by more than 1 alpha. The bucket `le="1"` is 1 whole alpha, because the metric divides the stored raw drift by 10^9. It checks correctness, not uptime. The 1% threshold is a starting value. Ordinary growth from emissions shows up as drift too, so set the threshold from a week of measured normal drift.
