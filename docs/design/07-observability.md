# Observability

This document lists the traces, logs, metrics, dashboards, and alerts. With them, an operator can answer four questions from Grafana alone. Which block is each job on, and is it backfilling or live? Which stage limits throughput? Is the API answering? Are the numbers it serves still correct?

## Set up tracing once

The `telemetry` crate builds the `tracing` subscriber. The binary calls it once, at the start of `main`.

- **Format.** JSON lines in production (`telemetry.log_format = "json"`). Readable, colored lines in development (`"pretty"`).
- **Filter.** `RUST_LOG` sets it, with the default `info,bt_=debug,sqlx=warn`. Every workspace crate's name starts with `bt_` (`bt_chain`, `bt_ingest`, `bt_store`, `bt_api`, and the others), so their targets do too. `EnvFilter` matches a directive's target as a prefix of the event's target (`tracing-subscriber-0.3.23/src/filter/env/directive.rs:246`), so `bt_=debug` covers every crate in the workspace and no dependency. A test binary with this filter printed `debug` lines from targets `bt_chain::rpc` and `bt_store` and dropped a `debug` line from `hyper` and an `info` line from `sqlx::query`. `sqlx` logs every query at `info`, which is too much for production.
- **Export.** With `telemetry.otlp_endpoint` set, a `tracing-opentelemetry` layer sends spans to an OpenTelemetry collector. Without it, spans appear only in the logs.

Library crates use `tracing` macros and `#[tracing::instrument]`. They never install a subscriber.

## Log every commit

Every commit logs one `info` line, so `docker compose logs ingest` shows each job's progress with no dashboard.

```
worker=epochs mode=backfill from=9487621 to=9488120 head=9612345 lag=124225 rate=310/s eta=6m41s
```

A job logs once when it switches mode, as `worker=prices mode=live block=9612343`. It logs the switch back to backfill the same way.

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

The `metrics` crate records the values, and `metrics-exporter-prometheus` serves them on `telemetry.metrics_addr`. Every name that the indexer exports has the prefix `bt_`. The exception is the database dashboard's metrics, which come from `postgres_exporter` and are named `pg_*`. Every label has a fixed, small set of values. No label holds an account, a hash, or a raw path.

The `worker` label holds the job name, which is `prices`, `validators`, `epochs`, `baskets`, or `identities`. Release 2 adds `events` and `positions`.

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

`bt_lag_blocks` counts from the newest block the schedule selects, not from the raw head. A `validators` job that has committed block 9,612,300 at head 9,612,345 has a lag of 0, because its next block is 9,612,400. Without that rule, a 100-block job would show a lag between 0 and 99 in steady state, and `IndexerLagging` would fire every cycle.

`bt_blocks_processed_total` counts chain blocks, whether the schedule selects them or not, so its rate has the same unit as the lag. ETA is lag divided by that rate.

At start, each job sets `bt_last_commit_timestamp_seconds` from its checkpoint row's `updated_at`, not from the start time. A restart therefore does not reset the age, and a process that crashes and restarts in a loop still fires `IndexerStalled`.

### Runner

| Metric | Type | Labels |
|---|---|---|
| `bt_pipeline_stage_busy_seconds` | histogram | `worker`, `stage` (`read` or `write`) |
| `bt_pipeline_send_wait_seconds` | histogram | `worker`, `stage` (`read`) |
| `bt_batch_rows` | histogram | `table` |
| `bt_batch_commit_seconds` | histogram | `worker` |

Each stage's time splits into three parts. The read stage is busy while it waits for the next ordered read to finish, waits on send while the channel to the writer is full, and is idle while it waits for a new head. The writer is busy while it builds and commits a batch, and idle while it waits on its channel. `rate(…_sum[5m])` of each histogram is therefore the fraction of time in that state, between 0 and 1. Only the read stage sends, so `bt_pipeline_send_wait_seconds` has only `stage="read"`.

### Node and database

| Metric | Type | Labels |
|---|---|---|
| `bt_rpc_requests_total` | counter | `worker`, `method`, `outcome` (`ok`, `retry`, `timeout`, or `error`) |
| `bt_rpc_request_seconds` | histogram | `worker`, `method` |
| `bt_rpc_in_flight` | gauge | `worker` |
| `bt_rpc_permit_wait_seconds` | histogram | `worker` |
| `bt_rpc_reconnects_total` | counter | `url_index` |
| `bt_db_pool_connections` | gauge | `pool` (`ingest` or `api`), `state` (`idle` or `used`) |
| `bt_db_query_seconds` | histogram | `query`. A fixed name per repository function |

`bt_rpc_in_flight` and `bt_rpc_permit_wait_seconds` carry `worker`, so the dashboard shows which job holds the global RPC permits. A timed-out call is recorded with `outcome="timeout"` and its full duration, so a hung node shows up in the latency histogram.

### API

| Metric | Type | Labels |
|---|---|---|
| `bt_http_requests_total` | counter | `route`, `method`, `status` |
| `bt_http_request_seconds` | histogram | `route`, `method` |
| `bt_api_ready` | gauge | 1 when `/readyz` would answer 200, 0 otherwise. The API recomputes it every 10 s |

`route` is the matched route template, such as `/subnets/{netuid}/validators`, or `unmatched` when no route matched. Shed requests count as `status="503"` on their route.

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

1. **Indexer.** It opens with the Progress row. Below that are blocks per second per job, the runner panels, node request rate and latency per job, and RPC permits in flight per job.
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

The panel turns red when the job's `IndexerStalled` or `IndexerLagging` alert fires.

## Find the slow stage

The runner for each job has three stages. They are the reads, the writer, and the node behind the reads. The limit is the stage that is busy close to 100% of the time while the stage after it does not block it. Start at the writer and walk backwards. Each rule names one cause.

1. If `rate(bt_pipeline_stage_busy_seconds_sum{stage="write"}[5m])` is close to 1, the writer is the limit. The database is the cause. Raise the batch size or make the inserts faster.
2. Otherwise, if `rate(bt_pipeline_stage_busy_seconds_sum{stage="read"}[5m])` is close to 1 and `rate(bt_pipeline_send_wait_seconds_sum[5m])` is close to 0, the reads are the limit. Find the cause inside the reads.
   1. If `bt_rpc_permit_wait_seconds` is high for the job, the global RPC cap, `chain.rpc_max_in_flight`, is the limit. Raise it if the node can take more.
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
| `ApiErrorRate` | `sum(rate(bt_http_requests_total{status=~"5.."}[5m])) / sum(rate(bt_http_requests_total[5m])) > 0.01` | 5m | page |
| `BlockGap` | `bt_gap_blocks > 0` | 0m | ticket |
| `ApiLatency` | `histogram_quantile(0.99, sum by (le, route) (rate(bt_http_request_seconds_bucket[5m]))) > 1` | 10m | ticket |
| `PositionDriftHigh` (release 2) | `1 - sum(increase(bt_position_drift_alpha_bucket{le="1"}[1d])) / sum(increase(bt_position_drift_alpha_count[1d])) > 0.01` | 1h | ticket |

- **`IndexerDown`** covers a process that has exited. Its metrics are no longer scraped, so no lag or staleness rule can fire for it.
- **`IndexerStalled`** fires per worker when the last commit is older than five schedule intervals at 12 s per block, and never sooner than 120 s. That is 120 s for `prices` and `epochs`, 6,000 s for `validators` and `identities`, and 30,000 s for `baskets`. It covers every worker, including `positions` in release 2. It also covers a crash loop, because the commit age survives a restart, and a decode error, because that error stops the process before it commits.
- **`IndexerLagging`** fires only for a job in live mode. A job in backfill is far behind by design.
- **`CompatReadyFalse`** fires when `/readyz` has been false for 5 minutes, or when the API process is down. [API](05-api.md#get-status) defines readiness.
- **`ApiErrorRate`** counts 503 responses from load shedding. Sustained shedding means the API needs more capacity, so it pages.
- **`PositionDriftHigh`** fires when more than 1% of the settles in the last day corrected a position by more than 1 alpha. The bucket `le="1"` is 1 whole alpha, because the metric divides the stored raw drift by 10^9. It checks correctness, not uptime. Ordinary growth from emissions shows up as drift too, so set the threshold after measuring a week of normal drift.
