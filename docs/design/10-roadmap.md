# Roadmap

Each milestone ends with a check that someone else can run. A milestone is done when its check passes on `master`, not when its code is written. Release 1 comes first, because it replaces the squid for tao-data-api. Within each release, the riskiest unknown comes first.

| Milestone | Work | Check |
|---|---|---|
| M0 | Prove historic decoding and runtime API calls, including the parent-metadata rule | `cargo test -p bt-chain --offline` decodes the recorded fixtures for every era with no network, and `fixtures/specs.csv` matches `spikes/data/specs.csv` after the base row |
| M1 | The store, the runner, the checkpoint, `status`, `GET /status`, and the `prices` job live from the head | `bt-indexer status` shows the job live, with lag below 5 for 1 hour, and the same-blocks-twice and crash tests pass |
| M2 | History windows, then `validators`, `epochs`, `baskets`, and `identities` with backfill | `status` shows each job's backfill progress and each job reaching live, and the README records the backfill time per job |
| M3 | The compat router | The golden tests and the Zod schema tests pass, a load test at 200 requests per second keeps p99 below 100 ms, and an overload run answers with 503 |
| M4 | Shadow run | `compat diff` is clean for 24 hours, apart from documented deviations. tao-data-api runs against the indexer in staging, with `INDEXER_DATA_API_2` pointed at us, with no Zod failures and no 502s |
| M5 | Ship release 1 | On a clean machine, `docker compose up` with a node URL starts every job, `bt-indexer status` shows their progress, and `/readyz` returns 200 once every window is covered |
| R2 | Events, positions, then native analytics | Each release 2 milestone below keeps its own check |

## Release 1

### M0. Prove historic decoding and runtime API calls

The design depends on reading old blocks with the right metadata. Storage and events of a block decode with the metadata of the spec that executed it, and runtime API results with the metadata of the block's own state ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)). With this rule the probes decode 35 of 35 recorded cases across every sampled era, including 8 upgrade blocks decoded with the executing spec's metadata (`spikes/crates/chain-decode`, `decode-table`). A spec that lived between two sampled points could still hide a shape change, and `metadata diff` rules that out. No item that release 1 reads changes shape across its 11 specs, 475 to 490 (`spikes/crates/chain-decode`, `shapes-r1-specs`). M0 builds the rule into the product.

- Create the workspace with the `bt-domain` and `bt-chain` packages ([D32](../decisions.md#d32-name-the-crates-bt-name)).
- Build `bt-indexer chain specs`, which finds each spec's upgrade block by bisection over `state_getStorageHash(":code")`, and run it with `--from 4920349` to write `fixtures/specs.csv`, the product's one committed spec file. Its first row is the base row, spec 219 at 4,920,349, so `executed_by(4920350)` is 219. Its other rows must equal `spikes/data/specs.csv` ([D8](../decisions.md#d8-find-the-spec-table-by-bisection-over-code-and-persist-only-the-specs-a-job-window-touches)). `first_block` is each spec's upgrade block.
- Seed the spec table in memory from `fixtures/specs.csv`, and find specs past its last row by bisection. The table is persisted in M1.
- Fetch metadata with `Metadata_metadata_at_version`, at the parent hash for the executing spec and at the block's own hash for the state spec ([D7](../decisions.md#d7-fetch-metadata-with-metadata_metadata_at_version)).
- Decode storage values and runtime API results from raw bytes through `frame-decode`. Implement subxt's `Config` hooks from the in-memory table for events only.
- Read every storage item the five jobs need, at one block in each era.
- Call `SwapRuntimeApi.current_alpha_price_all`, `SwapRuntimeApi.current_alpha_price`, and `BetaBasketRuntimeApi.get_all_validator_baskets` at one block in each era where they exist.
- Decode `StakeAdded` from upgrade block 5,228,683, which spec 252 executed and whose state reports 257. It decodes with 252's metadata and fails with 257's. This is the parent-metadata rule that release 2 needs.
- Build `bt-indexer metadata diff` and `bt-indexer fixture record`. Run `metadata diff` over every spec from 219, which executed block 4,920,350, to the current one, and commit its output as `docs/shape-changes.md`.

Release 1 reads only specs 475 to 490, so its jobs exercise only the fixtures of those specs. M0 keeps the older eras, before 261, before 290, 290, 391, 423, and the 441 to 459 basket shape with `weights`, because release 2 needs them ([D3](../decisions.md#d3-set-the-milestone-scope-by-what-release-1-reads)).

**Check.** `cargo test -p bt-chain --offline`, run with no network (`unshare -rn`), decodes the recorded fixtures for every era. That covers the storage values, the runtime API results, and the upgrade-block events. `fixtures/specs.csv` matches `spikes/data/specs.csv` row for row after the base row.

### M1. Run the `prices` job live

- The `bt-store` package, the migrations for `runtime_version`, `checkpoint`, `chain_head`, and `subnet_price`, and `bt-indexer migrate`.
- The spec table persisted in `runtime_version`, one row with its metadata for each spec a job window touches ([D8](../decisions.md#d8-find-the-spec-table-by-bisection-over-code-and-persist-only-the-specs-a-job-window-touches)). M0's in-memory table becomes its cache, and `bt-ingest` stores each `NewRuntime` that `bt-chain` returns.
- The head tracker and the runner, with the writer, the bounded channel, the RPC permits, the timeouts, and graceful shutdown. [Ingestion](03-ingestion.md#run-every-job-on-one-runner) describes them.
- The `prices` job in live mode, started at the head. It has no history window yet.
- `bt-indexer status`.
- The `bt-api` package with `GET /status` only. M3 adds the compat routes.
- The `bt-telemetry` package with the progress metrics and the progress log line.

**Check.** `bt-indexer status` shows `prices` live, with lag below 5 for 1 hour. The test that indexes the same blocks twice and the test that crashes in the middle of a batch both pass. The readiness rule still reports `prices` not ready, because its history starts at the head. That is expected until M2 adds the window (`spikes/crates/api`, `P23`).

### M2. Backfill the other four jobs

- `History` windows and the start block stored in the checkpoint. The retention policies ship with each hypertable's migration ([D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests)). `prices` gains its 2-day window here.
- The `validators`, `epochs`, `baskets`, and `identities` jobs.
- The first M2 run starts from an empty database. `start_block` never moves once a checkpoint row exists, so `prices` would otherwise keep the head start from M1.
- Backfill settings and the switch to live mode per job.
- The indexer dashboard in `deploy/grafana/`, with the "Progress" row.

**Check.** `bt-indexer status` shows each job's backfill progress, with its block, percent done, rate, and ETA, and then shows each job reaching live. The README records the measured backfill time per job and the machine.

### M3. Serve the compat routes

- The five routes in `crates/api/src/compat/`, the other operational endpoints, the middleware stack, and OpenAPI, added to the `bt-api` package that M1 created.
- The golden tests ported from the squid, the new `/pools` and `/validators` fixtures, and the Zod schema test in `tests/compat/`. [Quality](09-quality.md) describes them.

**Check.** The golden tests and the Zod schema tests pass. A load test with `oha` against the five routes at 200 requests per second keeps p99 below 100 ms on the README's machine, and an overload run answers with 503, not with queued requests. [API](05-api.md#do-not-cache-in-process) adds a cache only if this test fails.

### M4. Shadow the squid

- `bt-indexer compat diff --squid <url>`.
- A staging deploy of tao-data-api with `INDEXER_DATA_API_2` pointed at the indexer.

**Check.** `compat diff` shows no differences for 24 hours, apart from the deviations in [D28](../decisions.md#d28-anchor-apy-on-the-indexed-head-and-break-rank-ties-by-hotkey). tao-data-api runs against the indexer in staging with no Zod failures and no 502s.

### M5. Ship release 1

- The Docker image and `deploy/compose.yaml`.
- CI with every job in [quality](09-quality.md).
- The dashboards, the alert rules, and a runbook section for each alert.
- A README that opens with `docker compose up` and shows the `bt-indexer status` output, a dashboard screenshot, the architecture diagram, the measured numbers, and the known limits.

**Check.** On a clean machine, `docker compose up` with a node URL starts every job. `bt-indexer status` shows their progress, and `/readyz` returns 200 once every window is covered.

## Release 2

Release 2 adds events, positions, and the native analytics. Each milestone ends with its own check.

### R2.1. Index events

- The `events` job on the runner, decoding each block's events with the parent block's metadata.
- The fact tables `staking_action`, `transfer`, and `basket_flow`.
- Every decoder struct that `docs/shape-changes.md` lists, each with a fixture test.
- `prices` switched to `Full` history.

**Check.** The test that indexes the same blocks twice passes. A backfill of `events` from block 4,920,350, whose events decode with spec 219's metadata, finishes with zero decode errors and zero gaps, and the README records the measured time, the blocks per second, and the machine. Run against a node, `events` follows the finalized head with lag below 5 for one hour.

### R2.2. Positions and reconciliation

- The position folder, `reconcile_request`, `position_settle`, and the reconciler, with the rules in [D13](../decisions.md#d13-correct-positions-from-chain-state-on-a-schedule).
- `subnet`, `subnet_snapshot`, and daily holder counts.

**Check.** Folded positions for 100 sampled coldkeys match `get_stake_info_for_coldkeys` at the head, within the drift since the last settle.

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
6. Indexing unfinalized blocks, with rollback. [Decision D4](../decisions.md#d4-index-finalized-blocks-only) describes what that needs.
