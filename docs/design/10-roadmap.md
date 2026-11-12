# Roadmap

Each milestone ends with a check that someone else can run. A milestone is done when its check passes on `master`, not when its code is written. Release 1 comes first, because it replaces the squid for tao-data-api. Within each release, the riskiest unknown comes first.

| Milestone | Work | Check |
|---|---|---|
| M0 | Prove historic decoding and runtime API calls, including the parent-metadata rule | `cargo nextest run -p chain` decodes the recorded fixtures for every era, offline |
| M1 | The runner, the checkpoint, `status`, and the `prices` job in live mode | `bt-indexer status` shows the job live, with lag below 5 for 1 hour |
| M2 | `validators`, `epochs`, `baskets`, and `identities` with backfill windows | `status` shows each job's backfill progress and each job reaching live |
| M3 | The compat router | The golden tests and the Zod schema tests pass |
| M4 | Shadow run | `compat diff` is clean for 24 hours. tao-data-api runs against the indexer in staging, with `INDEXER_DATA_API_2` pointed at us |
| M5 | Ship release 1 | The Docker image, compose, CI, dashboards, alerts, and runbook exist, and a clean machine reaches `/readyz` 200 |
| R2 | Events, positions, then native analytics | Each release 2 milestone below keeps its own check |

## Release 1

### M0. Prove historic decoding and runtime API calls

The design depends on `subxt` 0.50 reading old blocks with the metadata of their own runtime. No other work starts until a prototype shows that it does.

- Create the workspace with the `domain` and `chain` crates.
- Implement subxt's `Config::spec_and_transaction_version_for_block_number` and `metadata_for_spec_version` from `runtime_version`, and fetch metadata at the parent hash.
- Read every storage item the five jobs need, at one block in each era.
- Call `SwapRuntimeApi.current_alpha_price_all`, `SwapRuntimeApi.current_alpha_price`, and `BetaBasketRuntimeApi.get_all_validator_baskets` at one block in each era where they exist.
- Decode `StakeAdded` from the first block of a new spec with the parent's metadata, to prove the parent-metadata rule that release 2 needs.
- Build `bt-indexer metadata diff` and `bt-indexer fixture record`. Run `metadata diff` over every spec from 233 to the current one, and commit its output as `docs/shape-changes.md`.

**Check.** `cargo nextest run -p chain` decodes the recorded fixtures for every era, offline. That covers the storage values, the runtime API results, and the upgrade-block events.

If `subxt` cannot do this, the fallback is decoding with `scale-decode` and `frame-decode` directly against the metadata bytes. Record the outcome in the [decisions log](../decisions.md).

### M1. Run the `prices` job live

- The `store` crate, the migrations for `runtime_version`, `checkpoint`, `chain_head`, and `subnet_price`, and `bt-indexer migrate`.
- The head tracker and the runner, with the writer, the bounded channel, the RPC permits, the timeouts, and graceful shutdown. [Ingestion](03-ingestion.md#run-every-job-on-one-runner) describes them.
- The `prices` job in live mode.
- `bt-indexer status` and `GET /status`.
- The `telemetry` crate with the progress metrics and the progress log line.

**Check.** `bt-indexer status` shows `prices` live, with lag below 5 for 1 hour. The test that indexes the same blocks twice and the test that crashes in the middle of a batch both pass.

### M2. Backfill the other four jobs

- The `validators`, `epochs`, `baskets`, and `identities` jobs.
- `History` windows, the start block stored in the checkpoint, and the retention policies.
- Backfill settings and the switch to live mode per job.
- The indexer dashboard in `deploy/grafana/`, with the "Progress" row.

**Check.** `bt-indexer status` shows each job's backfill progress, with its block, percent done, rate, and ETA, and then shows each job reaching live. The README records the measured backfill time per job and the machine.

### M3. Serve the compat routes

- The `api` crate with the five routes in `crates/api/src/compat/`, the operational endpoints, the middleware stack, and OpenAPI.
- The golden tests ported from the squid, the new `/pools` and `/validators` fixtures, and the Zod schema test in `tests/compat/`. [Quality](09-quality.md) describes them.

**Check.** The golden tests and the Zod schema tests pass. A load test with `oha` against the five routes at 200 requests per second keeps p99 below 100 ms on the README's machine, and an overload run answers with 503, not with queued requests. [API](05-api.md#do-not-cache-in-process) adds a cache only if this test fails.

### M4. Shadow the squid

- `bt-indexer compat diff --squid <url>`.
- A staging deploy of tao-data-api with `INDEXER_DATA_API_2` pointed at the indexer.

**Check.** `compat diff` shows no differences for 24 hours, apart from the deviations in [decision D13](../decisions.md#d13-anchor-apy-on-the-indexed-head-and-break-rank-ties-by-hotkey). tao-data-api runs against the indexer in staging with no Zod failures and no 502s.

### M5. Ship release 1

- The Docker image and `deploy/compose.yaml`.
- CI with every job in [quality](09-quality.md).
- The dashboards, the alert rules, and a runbook section for each alert.
- A README that opens with `docker compose up` and shows the `bt-indexer status` output, a dashboard screenshot, the architecture diagram, the measured numbers, and the known limits.

**Check.** On a clean machine, `docker compose up` with a node URL starts every job. `bt-indexer status` shows their progress, and `/readyz` returns 200 once every window is covered.

## Release 2

Release 2 adds events, positions, and the native analytics. Each milestone keeps the check it had before release 1 was split out, minus parallel backfill.

### R2.1. Index events

- The `events` job on the runner, decoding each block's events with the parent block's metadata.
- The fact tables `staking_action`, `transfer`, and `basket_flow`.
- Every decoder struct that `docs/shape-changes.md` lists, each with a fixture test.
- `prices` switched to `Full` history.

**Check.** The test that indexes the same blocks twice passes. A backfill of `events` from block 4,920,350 finishes with zero decode errors and zero gaps, and the README records the measured time, the blocks per second, and the machine. Run against a node, `events` follows the finalized head with lag below 5 for one hour.

### R2.2. Positions and reconciliation

- The position folder, `reconcile_request`, `position_settle`, and the reconciler, with the rules in [decision D10](../decisions.md#d10-read-state-only-in-release-1-and-move-events-and-positions-to-release-2).
- `subnet`, `subnet_snapshot`, and daily holder counts.

**Check.** Folded positions for 100 sampled coldkeys match `get_stake_info_for_coldkey` at the head, within the drift since the last settle.

### R2.3. Native analytics and the native API

- The candle aggregates.
- The ported `Balancer` and its differential test against `sim_swap`.
- The native API under `/api/v1`, described in [API](05-api.md#add-the-native-api-in-release-2).

**Check.** The differential test passes for every recorded case. Candles for one subnet over one day match a candle computed by hand from `subnet_price`. A load test with `oha` against `/api/v1/subnets` at 200 requests per second keeps p99 below 100 ms. The endpoint list in [API](05-api.md#add-the-native-api-in-release-2) matches `openapi.json`.

## After release 2

Ordered by value to users of the data:

1. Registration cost and churn.
2. Yuma and Null consensus comparison.
3. Cost basis and PnL per position.
4. Weight-setting cadence and epoch timing.
5. A server-sent events stream of new staking actions per subnet.
6. Indexing unfinalized blocks, with rollback. [Decision D3](../decisions.md#d3-index-finalized-blocks-only) describes what that needs.
