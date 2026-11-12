# Decisions

One entry per architecture decision. Each entry gives the decision, the reason, and the alternatives we rejected. To change a decision, add a new entry that replaces the old one, and mark the old one as replaced. Do not edit the old entry.

## D1. Decode with subxt and our own structs, not subtensor crates

**Decision.** Decode events and storage with `subxt` 0.50 and `scale-decode`, into structs we define with `#[derive(DecodeAsType)]`. Use one struct per shape change, with the metadata of the block's own spec. Copy `TaoBalance`, `AlphaBalance`, `NetUid`, `SafeFloat`, and `Balancer` from subtensor into `domain`.

**Reason.** Subtensor's crates describe only the current runtime, so they cannot decode old blocks. They also pin a fork of the Polkadot SDK and bring about 1,650 packages with them. Our structs depend only on the encoding, which changes rarely and visibly.

**Rejected.**
- *Depend on `node-subtensor-runtime` or `pallet-subtensor`.* One version only, a fork pin, and a lockstep upgrade with every chain release.
- *Generate types for every spec with `subxt` codegen.* This is what the reference squid does with about 60 versions. It produces a large amount of generated code where most versions are identical.
- *`bittensor-rs`.* It generates from the latest metadata only.
- *`sdk/bittensor-core`.* Its output is untyped values shaped for the Python SDK. It is not published, and its default features pull in OpenSSL and timelock cryptography.

**Amended by D10.** Release 2 decodes events with the metadata of the parent block's spec, and reads state with the block's own spec.

## D2. Separate facts from folds

**Decision.** Workers that write facts run in any order and in parallel. One worker, the position folder, computes positions in strict block order. Each table has one writer.

**Reason.** Backfill can then fetch with high concurrency without breaking positions. No two workers write the same row, so there are no locks or conflicts between them. The reconciler sends corrections through `position_settle`, so the folder stays the only writer of `position`.

**Rejected.**
- *Update positions in the event pipeline's transaction.* Simple, but parallel backfill becomes impossible, and the reconciler becomes a second writer of `position`.

**Amended by D11.** Facts stay independent of order, but no worker runs block ranges in parallel.

## D3. Index finalized blocks only

**Decision.** Every worker reads only up to the finalized head.

**Reason.** A finalized block cannot be reverted, so the indexer needs no rollback logic. The cost is about two blocks, roughly 24 seconds, of extra lag. Nothing we serve needs fresher data.

**Rejected.**
- *Index the best head and roll back on a reorg.* It needs the parent hash check on every block, an undo record for every fold, and tests for reorgs of every depth. It stays on the roadmap after the first release.

## D4. One PostgreSQL database with TimescaleDB

**Decision.** Store everything in PostgreSQL 17 with TimescaleDB. Time series go in compressed hypertables, and candles in continuous aggregates.

**Reason.** One system to run and back up. One transaction can cover a fact and the state it changes. `sqlx` checks every query at compile time. The estimated size, about 50 GB in the first year, fits PostgreSQL.

**Rejected.**
- *ClickHouse for time series.* Faster scans, but a second database to run and to keep consistent with the first.
- *Redis for caching.* Responses change once per block, and an in-process `moka` cache keyed by block covers that.

**Amended by D14.** Release 1 has no in-process cache, `moka` included. Continuous aggregates for candles arrive in release 2.

## D5. Prefer runtime APIs over raw storage

**Decision.** Read state through runtime APIs where they exist at the block's spec, and fall back to storage for older specs.

**Reason.** The runtime keeps its APIs stable while it renames and migrates storage. The reference squid moved its price reads to `SwapRuntimeApi` after `AlphaSqrtPrice` was removed. `AlphaV2` holds shares, not alpha, and `get_stake_info_for_coldkey` returns alpha directly.

**Rejected.**
- *Storage only.* Every storage migration becomes an indexer change.

## D6. Correct positions from chain state on a schedule

**Decision.** The reconciler reads true positions from `StakeInfoRuntimeApi` after each hotkey swap and once a day for every coldkey with a position. It writes them as settles.

**Reason.** Events do not cover every change to stake. Hotkey swaps carry no amounts, emissions add stake on every epoch, and migrations rewrite stake. The reference squid patched the drift five times before it settled every holder from chain state.

**Rejected.**
- *Read every position from chain state at every block.* About 45,000 hotkey entries per read makes it too slow for backfill.
- *Trust events alone.* Positions drift, and nothing shows it.

**Amended by D10.** The reconciler and the position folder move to release 2, with the corrected reconcile rules that D10 lists.

## D7. Stop on a decode error

**Decision.** A decode failure on an indexed event stops the event pipeline. The process exits, and it fails the same way after a restart until a decoder fix is deployed.

**Reason.** A skipped staking event makes positions wrong, and nothing shows it. A stop raises an alert within minutes, and the checkpoint stays before the bad block.

**Rejected.**
- *Skip and record the failure.* Wrong data looks the same as correct data.

## D8. Axum for the API

**Decision.** Serve the API with Axum 0.8 and `tower-http` middleware.

**Reason.** Axum runs on the same Tokio and `tower` stack as the rest of the workspace. `tower-http` provides tracing, request IDs, timeouts, compression, and CORS as layers. `utoipa-axum` builds the router and the OpenAPI document from the same functions.

**Rejected.**
- *Actix Web.* It works well, but it has its own runtime model and middleware system, separate from the `tower` stack the rest of the workspace uses.

## D9. Define compatibility by tao-data-api's five calls

**Decision.** The indexer is compatible when tao-data-api can point `INDEXER_DATA_API_1` or `INDEXER_DATA_API_2` at it and get its five calls answered with the same paths, response shapes, and values as the squid gives. The five routes are `GET /pools`, `GET /validators`, `GET /subnets/{netuid}/validators`, `GET /validators/{hotkey}/subnets`, and `GET /v1/subnet-identities`, at the host root. Nothing else about the squid is a contract. The squid is a data reference for the meaning of each value.

**Reason.** tao-data-api is the only consumer we replace the squid for. It reads the indexer only through `tao-data-api/src/common/indexer.ts`. A contract defined by the consumer can be tested, by running its Zod schemas and comparing live responses. "Match the squid" cannot, because the squid has many routes and behaviors that tao-data-api does not read.

**Rejected.**
- *Serve the squid's GraphQL API.* tao-data-api never calls it. It would add a second query language and a second schema to keep in step.
- *Serve every squid REST route.* Only the five routes have a consumer. The others would freeze shapes that nothing depends on.
- *A new native contract and a migration of tao-data-api.* It moves the work to another repository and delays the replacement.

## D10. Read state only in release 1, and move events and positions to release 2

**Decision.** Release 1 runs five jobs that read chain state, namely `prices`, `validators`, `epochs`, `baskets`, and `identities`. It decodes no events. The `events` job, the position folder, the reconciler, and the tables they write move to release 2. Release 2 keeps the earlier design with these corrections:

- `events` decodes each block's events with the metadata of the parent block's spec, and reads state with the block's own spec.
- A hotkey swap reconciles every coldkey with a position on the old or the new hotkey. For `HotkeySwappedOnSubnet` the scope is that netuid. A settle zeroes the positions of a reconciled coldkey that are absent from the runtime API response.
- The position folder writes the periodic reconcile requests itself when it reaches a boundary block, in its own transaction, and then waits for them.
- The folder subtracts with `checked_sub`. On underflow it stores 0 and records the shortfall as drift.

**Reason.** No compat route reads an event, a fact, or a position. The event pipeline and the positions also hold the hardest correctness problems, such as metadata at runtime upgrades, stake changes with no event, and folds that go negative. Moving them out lets release 1 ship once the state reads are right, and lets release 2 fix those problems without blocking the replacement.

**Rejected.**
- *Ship both pipelines in release 1.* It delays the replacement for data that tao-data-api never reads.

## D11. Run every job on one runner, with no parallel backfill

**Decision.** One runner runs every job, from `prices` in release 1 to `events` in release 2. A job turns a block into rows. The runner reads blocks with bounded concurrency in block order, and one writer commits the rows and the job's checkpoint in one transaction. No job splits its range into parallel parts. This amends D2. Facts stay independent of order, but nothing runs ranges in parallel.

**Reason.** The event pipeline and the snapshot pipeline do the same thing, so one runner gives one place for backpressure, shutdown, failure handling, and progress. In a measured probe, one pipeline at concurrency 64 matched two parallel parts at concurrency 32 each. Parallel parts therefore add code and no speed. Without them, every checkpoint is a contiguous frontier.

**Rejected.**
- *Separate event and snapshot pipelines.* Two implementations of the same loop, with two sets of bugs.
- *Parallel backfill ranges (`--workers`, `backfill_range`).* No measured gain, and a checkpoint that can move past blocks no part has written.

## D12. Give each job a history window

**Decision.** Each job declares its history. `Window(d)` starts `d` days plus one day of margin before the head. `Latest` reads only the newest scheduled block at or below the finalized head. `Full` starts at block 4,920,350. The start block is stored in the checkpoint row on the first run, so a restart does not move it. Windowed tables drop chunks older than the window plus 2 days. In release 1, `prices` keeps 2 days, `epochs` and `baskets` keep 31 days, and `validators` and `identities` keep the latest snapshot. Release 2 sets `prices` to `Full` for candles.

**Reason.** The compat routes read at most the last 31 days. A full backfill from dTAO would read millions of blocks whose rows no route reads. A window cuts the backfill of a fresh install to 230,400 blocks for the longest window, and it keeps the database small.

**Rejected.**
- *Backfill everything from dTAO.* Most of the reads produce rows that nothing serves.

## D13. Anchor APY on the indexed head and break rank ties by hotkey

**Decision.** These are deliberate deviations from the squid.

- The APY routes anchor both the 30-day row filter and the annualizing window at the chain time of the newest block the job has committed, never at `now()`. Validator APY uses the `epochs` checkpoint and root APR uses the `baskets` checkpoint ([analytics](design/08-analytics.md#compatibility-numbers)).
- `rank` in `GET /validators`, and the order of rows with equal stake, break ties by `hotkey`. `GET /validators/{hotkey}/subnets` breaks ties by `netuid`.

**Reason.** The squid filters epoch rows by wall-clock `NOW()` but annualizes over a window that ends at its indexed head (`bittensor-squid/api/src/routes/validators.ts:113` against `validators.ts:73-75`). When it lags, it counts fewer epochs over the same window. When the indexer is caught up, our anchor matches the squid within about 24 seconds of window. When it lags, ours stays consistent with itself. The squid's `ROW_NUMBER()` has no tiebreaker (`bittensor-squid/api/src/routes/validators.ts:270`), so its order for tied rows is arbitrary, and any fixed order is one it could return.

**Rejected.**
- *Copy `NOW()`.* A lagging indexer would report a lower APY for no reason in the data.
- *No tiebreaker.* Rank would change between two identical requests, and golden tests could not pin it.

## D14. Shed load, and do not rate-limit or cache in process

**Decision.** Release 1 has no per-IP rate limit and no in-process response cache. The API limits concurrent requests and answers the overflow with 503 at once. The API pool's `acquire_timeout` is 2 seconds, shorter than the 10 second request timeout. This amends D4's note on `moka`.

**Reason.** tao-data-api is one client behind one address, and it has its own per-client limiter (`tao-data-api/src/index.ts:27`). A per-IP limit at the indexer would throttle that one client. tao-data-api also caches each route for 5 to 15 minutes and lets concurrent requests wait on one refresh, so an indexer cache would save almost nothing. Without a limit, requests queue on the pool. In a measured probe, 939 requests waited at once and 600 ended in a 408 after 10 to 12 seconds. With shedding, at most 64 waited and the rest got 503 within 2 seconds.

**Rejected.**
- *`tower_governor` per client IP.* It throttles the one real client, and behind a proxy every client shares one IP.
- *A `moka` cache keyed by block, with ETags.* It adds invalidation rules for a load that tao-data-api's own cache already absorbs.

## D15. Read prices from the runtime API, not from V3 storage math

**Decision.** The `prices` job reads `SwapRuntimeApi.current_alpha_price_all` from spec 391 and `SwapRuntimeApi.current_alpha_price(netuid)` per subnet for specs 290 to 390 (`pallets/swap/runtime-api/src/lib.rs:29-30`). Spec 290 is the first mainnet runtime with that API, which git dates to the spec 283 commit. Before spec 290 it computes `SubnetTAO × 10^9 / SubnetAlphaIn`, because the provided items do not exist yet. The job picks the era from what the block's metadata declares, not from the spec number. The runtime API returns 10^9, a price of 1, for a subnet whose mechanism is not 1 (`pallets/swap/src/pallet/impls.rs:22-36`, `runtime/src/lib.rs:2874-2878`).

**Reason.** This follows D5. The runtime API is the chain's own price, so we do not reimplement the swap math of each era. The squid computed its V3-era prices from `AlphaSqrtPrice` storage (`bittensor-squid/src/main.ts:1408-1418`), so our historic prices can differ from its stored values by 1 rao. `/pools` reads only the last 26 hours, which are all in the runtime API era, so no compat value changes.

**Open for release 2.** With `prices` on full history, specs 290 to 390 cost one runtime API call per subnet per block, about 180 million calls by estimate. That moves the release 2 backfill from about 25 hours to about 8 days ([ingestion](design/03-ingestion.md#estimate-the-backfill-time)). Decide before that backfill whether candles for those specs use the per-subnet API or the squid's `AlphaSqrtPrice` math. Release 1 is not affected.

**Rejected.**
- *Copy the squid's `AlphaSqrtPrice` math.* It matches old squid rows to the rao, but it reimplements chain math that the runtime already exposes, and no compat value depends on it.
