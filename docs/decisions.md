# Decisions

One entry per architecture decision of the indexer. Each entry gives the decision, the reason, and the alternatives considered. The entries are grouped by topic: scope, decoding, data model, storage, the runner, node access, the API, configuration and workspace, and fixtures.

## D1. Define compatibility by tao-data-api's five calls

**Decision.** The indexer is compatible when tao-data-api can point `INDEXER_DATA_API_1` or `INDEXER_DATA_API_2` at it and get its five calls answered with the same paths, response shapes, and values as the squid gives. The five routes are `GET /pools`, `GET /validators`, `GET /subnets/{netuid}/validators`, `GET /validators/{hotkey}/subnets`, and `GET /v1/subnet-identities`, at the host root. Nothing else about the squid is a contract. The squid is a data reference for the meaning of each value.

**Reason.** tao-data-api is the only consumer we replace the squid for. It reads the indexer only through `tao-data-api/src/common/indexer.ts`. A contract defined by the consumer can be tested, by running its Zod schemas and comparing live responses. "Match the squid" cannot, because the squid has many routes and behaviors that tao-data-api does not read.

**Alternatives considered.**
- *Serve the squid's GraphQL API.* tao-data-api never calls it. It would add a second query language and a second schema to keep in step.
- *Serve every squid REST route.* Only the five routes have a consumer. The others would freeze shapes that nothing depends on.
- *A new native contract and a migration of tao-data-api.* It moves the work to another repository and delays the replacement.

## D2. Read state only in release 1, and index events and positions in release 2

**Decision.** Release 1 runs five jobs that read chain state, namely `prices`, `validators`, `epochs`, `baskets`, and `identities`. It decodes no events. The `events` job, the position folder (D12), the reconciler (D13), and the tables they write belong to release 2.

**Reason.** No compat route reads an event, a fact, or a position. The event pipeline and the positions also hold the hardest correctness problems, such as metadata at runtime upgrades, stake changes with no event, and folds that go negative. Release 1 can ship once the state reads are right, and release 2 can solve those problems without blocking the replacement.

**Alternatives considered.**
- *Ship both pipelines in release 1.* It delays the replacement for data that tao-data-api never reads.

## D3. Set the milestone scope by what release 1 reads

**Decision.** The milestones M0 to M3 are those of the [roadmap](design/10-roadmap.md). M1 includes the `api` crate with `GET /status` only. M1 starts `prices` at the head, and M2 adds the history windows (D14). A checkpoint's `start_block` never moves, so the M1 database is dropped before M2. Release 1's windows reach back only to block 9,260,462 at the measured head, so release 1 reads specs 475 to 490 and nothing older. M0 keeps the older eras (pre-290, 290, 391, 423, pre-261, and the `weights` basket shape), because the decode table already covers them and release 2 needs them. Release 1's tests exercise the fixtures of specs 475 to 490, and the older fixtures exist for release 2.

**Reason.** M1 and M2 need a way to show progress, and an ops router serves `/status` with no compat route (`spikes/crates/api`, `P22`). `prices` started at the head passes M1's lag check and fails only readiness (`spikes/crates/api`, `P23`). At head 9,490,862, measured on 2026-11-12, the 31-day window starts at 9,260,462 (spec 475) and the `prices` window at 9,469,262 (spec 488) (`spikes/crates/chain-specs`, `P29`). Spec 441 begins at 8,765,683, before the window, so the `baskets` floor never binds in release 1 (`spikes/crates/chain-decode`, `P29-r1`). The decode table passes 35 of 35 cases across every era of every item, and it decodes each of its 8 upgrade blocks by D6 (`spikes/crates/chain-decode`, `decode-table`).

**Alternatives considered.**
- *Drop the pre-475 eras from M0.* Release 2 needs them, and the decode table covers them at no extra cost.
- *Serve `GET /status` from M3, with the compat routes.* M1 and M2 then have no way to show progress.

## D4. Index finalized blocks only

**Decision.** Every worker reads only up to the finalized head.

**Reason.** A finalized block cannot be reverted, so the indexer needs no rollback logic. The cost is about two blocks, roughly 24 seconds, of extra lag. Nothing we serve needs fresher data.

**Alternatives considered.**
- *Index the best head and roll back on a reorg.* It needs the parent hash check on every block, an undo record for every fold, and tests for reorgs of every depth. It stays on the roadmap after the first release.

## D5. Decode with subxt and our own structs, not subtensor crates

**Decision.** Decode events with `subxt` 0.50, and storage and runtime API results with `frame-decode`, into structs we define with the `#[derive(DecodeAsType)]` of `scale-decode`. Use one struct per shape change, decoded with the metadata that D6 selects. Copy `TaoBalance` and `AlphaBalance` (as `Rao` and `Alpha`), `NetUid`, `SafeFloat`, and `Balancer` from subtensor into `domain`.

**Reason.** Subtensor's crates describe only the current runtime, so they cannot decode old blocks. They also pin a fork of the Polkadot SDK and bring about 1,650 packages with them. Our structs depend only on the encoding, which changes rarely and visibly.

**Alternatives considered.**
- *Depend on `node-subtensor-runtime` or `pallet-subtensor`.* One version only, a fork pin, and a lockstep upgrade with every chain release.
- *Generate types for every spec with `subxt` codegen.* This is what the reference squid does with about 60 versions. It produces a large amount of generated code where most versions are identical.
- *`bittensor-rs`.* It generates from the latest metadata only.
- *`sdk/bittensor-core`.* Its output is untyped values shaped for the Python SDK. It is not published, and its default features pull in OpenSSL and timelock cryptography.

## D6. Decode storage and events with the executing spec, and runtime APIs with the block's own spec

**Decision.** Storage and events of block n decode with the metadata of the spec that executed n, which is the state spec of n's parent. Runtime API calls at n use the metadata of n's own state spec, because the node runs the new code. `SpecAt { executed_by, state }` keeps both numbers, and `BlockRef` carries it. Storage decodes raw bytes through `frame-decode` with a `subxt::Metadata` we pick (`spikes/crates/chain-decode`, `P19`, `P19-offline-decode`).

An item that the metadata in use does not declare is absent, and no decoder substitutes a default for it. A row that requires an absent item is not written. A row for which the item is optional is written without it. Four items are optional:
- `SubnetTaoProvided` and `SubnetAlphaInProvided`. Without them the reserve sum has no provided term.
- `SubnetEmaTaoFlow`. Without it `ema_tao_flow_bits` is `NULL`.
- `TotalHotkeyAlphaLastEpoch`, which is absent before spec 261. The decoder returns `None`. Release 1 never reads that era, so `stake_at_epoch` is `NOT NULL`, and release 2 chooses what to store for it.

A declared item with no stored key reads as its metadata default. The product decodes raw bytes, so our code applies that default.

**Reason.** At an upgrade block u the storage is still pre-migration. `System.LastRuntimeUpgrade` still names the old spec at u, and the items that the new spec adds have 0 keys until u+1. Key counts show this at 4,920,350 and 5,947,548 (`spikes/crates/chain-specs`, `P31`, `P31-290`) and at 8,486,593 (`spikes/crates/chain-specs`, `P31-423`). At 4,920,350 a decoder that defaults an absent value returns a price of 0 for every subnet, with no error (`spikes/crates/chain-decode`, `p31-4920350`). The parent's metadata decodes the events of all 106 upgrade blocks with 0 bytes left, and the block's own metadata fails at 13 of them (`spikes/crates/chain-specs`, `D-scan`). One subxt `Config` hook takes one spec per block number and uses it for events and storage alike, so it cannot serve both specs (`spikes/crates/chain-specs`, `P1`).

A metadata default is not always zero. With no stored key, `Delegates` decodes from its default bytes to 11,796, and `TaoWeight` to 971,718,665,099,567,868 (`spikes/crates/chain-decode`, `default-from-metadata`, `taoweight-null`).

The decode evidence:
- The decode table holds 35 cases and passes 35 of 35. 27 decode by this rule. At 19 of those the two rules pick the same spec, and 8 sit on upgrade blocks where they differ. The other 8 decode the same 8 upgrade blocks by state spec, for comparison (`spikes/crates/chain-decode`, `decode-table`).
- In release 1's window, at all 10 upgrade blocks of specs 477 to 490, every storage item release 1 reads is declared under both the executing spec's and the state spec's metadata. Items of other eras, such as the provided reserves, are declared under neither. Every key of every map decodes to equal values under both. The maps were read whole, for example 333,510 `Owner` keys (`spikes/crates/chain-decode`, `storage-executing-spec`).
- At 7 older upgrade blocks (4,920,350, 5,947,548, 7,063,679, 7,782,857, 8,486,593, 8,765,683, and 9,088,597), on maps sampled at 2,000 keys, no value differs either. There the two rules differ in what they can see. An item that only the state spec declares holds 0 keys, while an item that only the executing spec declares still holds data, such as 2 `SubnetIdentitiesV2` keys at 7,063,679 and 129 `SubnetTaoProvided` keys at 8,486,593. A reader that used the state spec would miss those.
- No item release 1 reads changes shape across specs 475 to 490, checked on all 11 mainnet specs in that range (`spikes/crates/chain-decode`, `shapes-r1-specs`).

**Alternatives considered.**
- *The block's own spec for state and events.* One rule for every read, but storage at an upgrade block still has the old layout, and events fail to decode at 13 of 106 upgrade blocks.
- *One subxt `Config` hook for every read.* It answers with one spec per block number, and a block needs two.
- *Default an absent item to zero.* The upgrade block then gives silent zero prices.

## D7. Fetch metadata with `Metadata_metadata_at_version`

**Decision.** Fetch metadata with `state_call("Metadata_metadata_at_version", SCALE(v: u32), hash)` and decode the answer as `Option<Vec<u8>>` holding `RuntimeMetadataPrefixed`. `v` is the highest value from `Metadata_metadata_versions` after dropping `4294967295`. Metadata for the events of block n is the same call at n's parent hash (D6). The versions by era are `[14, 15]` for specs 219 to 277, `[14, 15, 4294967295]` for specs 290 to 323, and `[14, 15, 16]` for specs 326 to 490 (`spikes/crates/chain-specs`, `meta-calls`, `meta-eras`).

**Reason.** `state_getMetadata` returns V14, which lists no runtime APIs, so a job cannot ask the metadata whether an API exists. `Metadata_metadata_at_version(15)` lists 19 runtime APIs at spec 233 and 26 at spec 490 (`spikes/crates/chain-specs`, `P4`). subxt picks the version in the same way. V16, fetched for the 48 recorded specs from 326 to 490, decodes the 35 cases of the decode table with output identical to V15 and gives the same shapes (`spikes/crates/chain-decode`, `metadata-v16`).

**Alternatives considered.**
- *`state_getMetadata`.* One plain call, but it returns V14 with no runtime API list.

## D8. Find the spec table by bisection over `:code`, and persist only the specs a job window touches

**Decision.** `runtime_version.first_block` is the upgrade block, the first block whose state reports the spec (`first_state_block` in `spikes/data/specs.csv`). The spec executes from `first_block + 1`. The table is found by bisection over `state_getStorageHash(":code")`, with `state_getRuntimeVersion` at n−1 and n to confirm each boundary. One `:code` hash check per range end and per new head rules out a new spec. `chain` returns a `#[must_use] NewRuntime`, which `bt-domain` defines (D33), and `ingest` stores it.

The product's committed spec file is `fixtures/specs.csv`. `bt-indexer chain specs --from 4920349` writes it by bisection. Its first row is a base row: spec 219 at block 4,920,349, the spec in force at the start block, so `executed_by(4920350)` is 219. The other rows are the 106 mainnet specs, 233 to 490, from dTAO to 2026-11-12, and they equal the rows of `spikes/data/specs.csv`. They span about 4.57 million blocks, from 4,920,350 to the head of 2026-11-12 (D3).

The in-memory table is seeded from `fixtures/specs.csv` and holds every row of the file, from M0. From M1, `runtime_version` gets a row, with its metadata, only for a spec that a job window touches, the first time a job needs that spec. The file has 107 rows.

The table gives these boundaries (`spikes/crates/chain-specs`, `item-eras`):
- Baskets drop `weights` at mainnet spec 464 (block 9,088,597). Mainnet ran 459 and then 464 (`spikes/crates/chain-decode`, `era-basket-weights`).
- `Delegates` is `u16` through spec 424 and `PerU16` from spec 432 (block 8,636,190). The `NetUid` newtype also starts at 432 (`spikes/crates/chain-decode`, `era-peru16`, `netuid-type`).

**Reason.** A spec learned when a reader first sees it is wrong for a sparse or late reader. A reader of every 100th block first meets spec 490 at 9,478,300, but 490 first executed at 9,478,218. `epochs` would store 9,260,462 for spec 475, but 475 first executed at 9,233,782 (`spikes/crates/chain-specs`, `P5`). Upgrades come as close as 2 blocks apart, so only a check per range end rules out a new spec (`spikes/crates/chain-specs`, `P30`). `chain` may not depend on `store`, so it cannot fill the table itself (`spikes/crates/chain-specs`, `P6`, `P7`).

A table with no row below 4,920,350 leaves `executed_by(4920350)` undefined, and `OfflineClient::at_block(4920350)` then fails with `SpecVersionNotFound` (`spikes/crates/chain-errors`, `metadata-missing`). The base row defines it. 219's true upgrade block is earlier and is not needed. The in-memory table holds every row, so `executed_by(n)` is defined for every block.

On the keyed archive node the whole `:code` bisection took 3,222 calls and about 1 minute, plus about 11 minutes of version checks (`spikes/crates/chain-specs`, `spec-list`). On a warm node one `:code` hash took 0.28 to 0.35 s and one runtime version 0.28 to 1.46 s (`spikes/crates/chain-specs`, `bisect-method`).

Release 1 reads only specs 475 to 490, which are 11 specs and about 5 MB of metadata ([storage](design/04-storage.md), D3). The metadata of the other specs would cost node calls and disk for specs that no release 1 job decodes.

**Alternatives considered.**
- *Store `first_block = n` when a spec is first seen.* No table to build, but it records a late block for any reader that skips blocks.
- *Bisect over `state_getRuntimeVersion`.* Each call costs more than a `:code` hash, and at 6 concurrent calls it found no boundary in 23 minutes.
- *Have `chain` write `runtime_version`.* `chain` may not depend on `store`.
- *Persist every row of `fixtures/specs.csv` at first start.* It fetches about 96 metadata blobs that release 1 never uses.

## D9. Prefer runtime APIs over raw storage

**Decision.** Read state through runtime APIs where they exist at the block's spec, and fall back to storage for older specs.

**Reason.** The runtime keeps its APIs stable while it renames and migrates storage. The reference squid moved its price reads to `SwapRuntimeApi` after `AlphaSqrtPrice` was removed. `AlphaV2` holds shares, not alpha, and `get_stake_info_for_coldkeys` returns alpha directly.

**Alternatives considered.**
- *Storage only.* Every storage migration becomes an indexer change.

## D10. Read prices from the runtime API, not from V3 storage math

**Decision.** The `prices` job reads `SwapRuntimeApi.current_alpha_price_all` from spec 391 and `SwapRuntimeApi.current_alpha_price(netuid)` per subnet for specs 290 to 390 (`pallets/swap/runtime-api/src/lib.rs:29-30`). Spec 290 is the first mainnet runtime with that API, which git dates to the spec 283 commit. Before spec 290 it computes `SubnetTAO × 10^9 / SubnetAlphaIn`, because the provided items do not exist yet. The job picks the era from what the block's metadata declares, not from the spec number. The runtime API returns 10^9, a price of 1, for a subnet whose mechanism is not 1 (`pallets/swap/src/pallet/impls.rs:22-36`, `runtime/src/lib.rs:2874-2878`). At an upgrade block the price items can be absent, and `prices` then writes no row (D6).

In release 2, `prices` keeps full history (D14). The per-subnet era of specs 290 to 390 then costs one runtime API call per subnet per block. That is 234,919,552 price calls, about 23 days at 120 calls a second (D23), which is a floor. The figure counts the 128 price calls per block. With the 3 fixed calls a block costs 131 (`spikes/crates/chain-decode`, `backfill-r2-price`). The whole release 2 backfill takes about 2.5 days without that era and at least about 25 days with it ([ingestion](design/03-ingestion.md#estimate-the-backfill-time)). Whether candles for those specs use the per-subnet API or the squid's `AlphaSqrtPrice` math is decided before that backfill. Release 1 is not affected.

**Reason.** This follows D9. The runtime API is the chain's own price, so we do not reimplement the swap math of each era. The squid computed its V3-era prices from `AlphaSqrtPrice` storage (`bittensor-squid/src/main.ts:1408-1418`), so our historic prices can differ from its stored values by 1 rao. `/pools` reads only the last 26 hours, which are all in the runtime API era, so no compat value changes.

**Alternatives considered.**
- *Copy the squid's `AlphaSqrtPrice` math.* It matches old squid rows to the rao, but it reimplements chain math that the runtime already exposes, and no compat value depends on it.

## D11. Stop on a decode error

**Decision.** A decode failure on an indexed event stops the event pipeline. The process exits, and it fails the same way after a restart until a decoder that handles the block is deployed.

**Reason.** A skipped staking event makes positions wrong, and nothing shows it. A stop raises an alert within minutes, and the checkpoint stays before the bad block.

**Alternatives considered.**
- *Skip and record the failure.* Wrong data looks the same as correct data.

## D12. Separate facts from folds

**Decision.** Workers that write facts do not depend on block order. One worker, the position folder, computes positions in strict block order. One worker inserts the rows of each table, and no two workers write the same column. This is release 2 work (D2).

**Reason.** Blocks can be fetched concurrently without breaking positions, because only the folder depends on order. No two workers write the same column, so there are no locks or conflicts between them. The reconciler sends corrections through `position_settle`, so the folder stays the only writer of `position` (D13).

**Alternatives considered.**
- *Update positions in the event pipeline's transaction.* Simple, but every fact write is then tied to the fold's strict order, and the reconciler becomes a second writer of `position`.

## D13. Correct positions from chain state on a schedule

**Decision.** In release 2, the reconciler reads true positions from `StakeInfoRuntimeApi` and writes them as settles. The position folder asks for each reconcile and applies each settle.

- The folder writes the reconcile requests of a block itself, in its own transaction, and waits until the reconciler has fulfilled them before it applies the block.
- An event that moves stake without an amount reconciles the coldkeys it touches. A hotkey swap reconciles every coldkey with a position on the old or the new hotkey. For `HotkeySwappedOnSubnet` the scope is that netuid.
- Every 7,200 blocks, about once a day, the folder requests every coldkey that is a key of `StakingHotkeys`.
- A settle zeroes the positions of a reconciled coldkey that are absent from the runtime API response.
- The folder subtracts with `checked_sub`. On underflow it stores 0 and adds the missing amount to `position.shortfall`. The next settle resets `shortfall`, and its `drift` includes the missing amount.

**Reason.** Events do not cover every change to stake. Hotkey swaps carry no amounts, emissions add stake on every epoch, and migrations rewrite stake. Every coldkey that gains stake by any path is a key of `StakingHotkeys`, so the daily set includes the coldkeys that hold only emissions and have no `position` row. The runtime API skips a position whose alpha is 0, so a settle zeroes what the response does not list. The reference squid patched the drift five times before it settled every holder from chain state. [Chain data](design/02-chain-data.md#watch-for-these-traps-in-the-data) cites each case in subtensor.

**Alternatives considered.**
- *Read every position from chain state at every block.* About 45,000 hotkey entries per read makes it too slow for backfill.
- *Trust events alone.* Positions drift, and nothing shows it.

## D14. Give each job a history window

**Decision.** Each job declares its history. `Window(d)` starts `d` days plus one day of margin before the head. `Latest` reads only the newest scheduled block at or below the finalized head. `Full` starts at block 4,920,350. The start block is stored in the checkpoint row on the first run, so a restart does not move it. Windowed tables drop chunks older than the window plus 2 days. In release 1, `prices` keeps 2 days, `epochs` and `baskets` keep 31 days, and `validators` and `identities` keep the latest snapshot. Release 2 sets `prices` to `Full` for candles. D3 places the windows in the milestones.

**Reason.** The compat routes read at most the last 31 days. A full backfill from dTAO would read millions of blocks whose rows no route reads. A window cuts the backfill of a fresh install to 230,400 blocks for the longest window, and it keeps the database small.

**Alternatives considered.**
- *Backfill everything from dTAO.* Most of the reads produce rows that nothing serves.

## D15. One PostgreSQL database with TimescaleDB

**Decision.** Store everything in PostgreSQL 17 with TimescaleDB. Time series go in compressed hypertables. Candles go in continuous aggregates, which arrive in release 2.

**Reason.** One system to run and back up. One transaction can cover a fact and the state it changes. `sqlx` checks every query at compile time. The estimated size fits PostgreSQL. The largest table of release 2, `subnet_price` with full history, is about 60 GB uncompressed at the measured head and about 6 GB compressed, so the deployment provisions 100 GB.

**Alternatives considered.**
- *ClickHouse for time series.* Faster scans, but a second database to run and to keep consistent with the first.
- *Redis as a response cache.* Responses change once per block, and tao-data-api already caches each route (D25).

## D16. Set the store's rules for migrations, writes, and tests

**Decision.** The image is pinned as `timescale/timescaledb:2.31.0-pg17`, digest `sha256:b56a3f51991df1f79d0c1a0841cad5c5fa8058fdd6a3ee6886394a6d4c174538`.

Migrations:
- `0001` starts with `CREATE EXTENSION IF NOT EXISTS timescaledb;`.
- The migration that creates each hypertable also creates its retention and compression policies. A later change uses `remove_retention_policy` and then `add_retention_policy`.
- Every release 1 and release 2 migration runs in a transaction, without `-- no-transaction` (`spikes/crates/store`, `no-tx-r1`, `no-tx-cagg`).
- `bt-indexer migrate` uses one connection and exits on error.
- The read-only API role is granted with `ALTER DEFAULT PRIVILEGES FOR ROLE <migrate role>`.

Writes:
- A new checkpoint row is inserted with `last_block_ts = to_timestamp(0)`, `target_block = start_block − 1`, and `blocks_per_sec = 0`, using `ON CONFLICT (worker) DO NOTHING`.
- `identities` writes changed rows only and deletes vanished ones.
- `sqlx::Error::Io`, `sqlx::Error::PoolTimedOut`, and SQLSTATE 40001, 40P01, 57014, 57P01, 57P02, and 57P03 are classed as `Retry`.
- The `UNNEST` column builder asserts equal array lengths.
- Columns map as `TIMESTAMPTZ` to `DateTime<Utc>`, `INTEGER` to `i32`, `BIGINT` to `i64` through `try_from`, `NUMERIC(39,0)` to `BigDecimal`, `BYTEA` to `Vec<u8>`, and `REAL` to `f32`.
- Queries are `sqlx::query!` with a committed `.sqlx/` cache (`spikes/crates/store`, `sqlx-offline`).

Tests:
- Each test gets its own database, cloned from a migrated template database, on one container per test binary.

**Reason.** Each rule answers a behaviour of TimescaleDB 2.31.0 on PostgreSQL 17.
- A database made from `template0` has no extension (`spikes/crates/store`, `ddl-applies`).
- A hypertable holds one retention policy. A second `add_retention_policy` fails with 42710, so a change removes the policy first (`spikes/crates/store`, `retention-policy-dup`, `policy-change`).
- A failed `Migrator::run` keeps its advisory lock on the pooled connection, and a second run on another connection waits indefinitely (`spikes/crates/store`, `migrate-lock`).
- Default privileges apply only to tables that the named role creates (`spikes/crates/store`, `api-role`).
- A checkpoint row without the three values fails with 23502 (`spikes/crates/store`, `P12`).
- Replacing the whole identity set moves `updated_block` on every row (`spikes/crates/store`, `P26`).
- A lost connection arrives first as 57P01 and then as `Io(UnexpectedEof)` (`spikes/crates/store`, `sqlstate-map`, `outage`). 57P02 and 57P03 are in the list by class, because they also mean the server went away. `PoolTimedOut`, 40001, 40P01, and 57014 are transient, and a retry of the whole batch writes the same rows.
- `UNNEST` accepts arrays of unequal length instead of rejecting them, and a NULL reaches the column (`spikes/crates/store`, `unnest-ragged`).
- A container per test takes 2.84 s, a database created and migrated per test takes 103 to 184 ms, and a clone of a migrated template takes 28 ms (`spikes/crates/store`, `test-db-per-test`).

**Alternatives considered.**
- *Replace the identity set on each read.* One statement, but `updated_block` loses its meaning.
- *A container or a schema per test.* A container is 100 times slower than a template clone, and a schema per test needs a `search_path` in every connection.

## D17. Run every job on one runner, with no parallel backfill

**Decision.** One runner runs every job, from `prices` in release 1 to `events` in release 2. A job turns a block into rows. The runner reads blocks with bounded concurrency in block order, and one writer commits the rows and the job's checkpoint in one transaction. No job splits its range into parallel parts. D18 gives the runner's shapes, and D23 the concurrency defaults.

**Reason.** The event pipeline and the snapshot pipeline do the same thing, so one runner gives one place for backpressure, shutdown, failure handling, and progress. Parallel parts put no more calls in flight than one pipeline of the same total concurrency, under the same permits and rate cap. They therefore add code and no speed. Without them, every checkpoint is a contiguous frontier.

**Alternatives considered.**
- *Separate event and snapshot pipelines.* Two implementations of the same loop, with two sets of bugs.
- *Split a backfill into parallel block ranges.* No gain in throughput, and a checkpoint that can move past blocks no part has written.

## D18. Give every job one trait, and build the runner from a spawned read stage and a batching writer

**Decision.** The runner uses these shapes:

```rust
pub trait Job: Send + Sync + 'static {
    const NAME: &'static str;
    type Rows: Send + Sync + 'static;
    fn schedule(&self) -> Schedule;
    fn history(&self) -> History;
    fn read<S: BlockSource>(&self, chain: &Chain<S>, at: BlockRef) -> impl Future<Output = Result<Self::Rows, ChainError>> + Send;
    fn block_ts(rows: &Self::Rows) -> Timestamp;
    fn write(&self, tx: &mut PgTransaction<'_>, batch: &[Self::Rows]) -> impl Future<Output = Result<(), StoreError>> + Send;
}
pub enum Schedule { EveryBlock, Every(NonZeroU32) }
pub enum ToWriter<R> { Rows(BlockNumber, R), End }
pub fn spawn_read<J: Job, S: BlockSource>(job: Arc<J>, chain: Arc<Chain<S>>, n: BlockNumber)
    -> AbortOnDropHandle<Result<J::Rows, ChainError>>;
pub async fn retry_with_backoff<T, E, F, Fut>(policy: &RetryPolicy, token: &CancellationToken, op: F) -> Result<T, E>
where E: HasDisposition, F: FnMut() -> Fut, Fut: Future<Output = Result<T, E>>;
```

- `Chain<S: BlockSource>` is generic over its source, and so is `Job::read`.
- Each read is `tokio::spawn(async move { .. })` over `Arc<J>` and `Arc<Chain<S>>`, held in `tokio_util::task::AbortOnDropHandle`.
- The read side sends `ToWriter::End` before it drops its sender. A writer that sees the channel close without `End` returns `RunnerError::UpstreamClosed`.
- The writer's `next_event` is `select!` with `biased;`, then cancel, then deadline, then `recv`. The commit runs outside the `select!`.
- Rows alive peak at `batch_blocks + channel_capacity + concurrency`.
- In `retry_with_backoff`, only the backoff sleep selects on the token. On cancel it returns the last error.
- A signal shutdown that has to abort a task at `shutdown_timeout` logs a warning that says how many tasks it aborted, and exits 0.
- A cancel during a commit backoff gives up the batch and exits 0. In `commit_batch`, a `Retry` error after a cancelled token becomes `Ok(Committed::Stopped)`, so the writer stops cleanly. The guard is `Err(e) if matches!(e.disposition(), Disposition::Retry) && token.is_cancelled()`. Any other error stays an error, so a fatal error such as SQLSTATE 23505 during shutdown exits 1.

**Reason.** Each shape answers a constraint of the runner.
- Tests drive reads through `BlockSource`, so `Chain` is generic over the source. `Chain<S>` runs with a fake source (`spikes/crates/runner`, `P8`).
- `tokio::spawn` needs a `'static` future, so each read owns its job and its chain through `async move` over `Arc`s (`spikes/crates/runner`, `P9`).
- The batching writer holds `&[Rows]` across `write().await` in a spawned task, so `Rows` must be `Sync` (`spikes/crates/runner`, `job-trait`, `job-bounds`).
- A plain `JoinHandle` detaches its task on drop, so a stopped read stage would leave reads running that hold their permits (`spikes/crates/runner`, `detach`).
- A commit inside a `select!` branch is lost with its batch when another branch wins (`spikes/crates/runner`, `writer-select`). With `recv` before the deadline, a busy channel never lets the deadline win, and the first commit holds 100,000 blocks against 1 (`spikes/crates/runner`, `writer-bias`).
- `Every(0)` would divide by zero in `newest_scheduled`, hence `NonZeroU32`.
- In a test with a `batch_blocks` of 500, a `channel_capacity` of 256 and a `concurrency` of 32, a stalled writer holds rows alive at 788, which is 500 + 256 + 32 (`spikes/crates/runner`, `backpressure`).
- The shutdown exits are safe, because writes are idempotent and an abort rolls back its transaction (`spikes/crates/runner`, `shutdown`). After a cancel in a commit backoff the next run writes the same blocks again, and the process exits 0 through the writer and the supervisor (`spikes/crates/runner`, `cancel-in-commit-backoff`).

**Alternatives considered.**
- *A concrete `Chain` in `Job::read`.* A shorter signature, but a test source cannot drive it.
- *A `JoinHandle` per read.* It keeps permits after the read stage stops.
- *A commit inside a `select!` branch.* It loses the batch when another branch wins.
- *Exit 1 when shutdown aborts a task, or on a cancel during a commit backoff.* No data is at risk, and a warning says how many tasks were aborted.
- *`Ok(Committed::Stopped)` for any error once the token is cancelled.* A fatal error during shutdown would then exit 0.

## D19. Track the head by polling

**Decision.** The head tracker polls `chain_getFinalizedHead` every 3 s. It does not subscribe to finalized heads.

**Reason.** `BlockSource` has no subscription, so a tracker that subscribes cannot run on a test source. A subscription also ends on any socket drop. The reconnecting client yields one `DisconnectedWillReconnect` item and then ends the stream, and the plain client just ends it (`spikes/crates/chain-errors`, `reconnect-subscription`). Measured over 120 s, polling reached the same last head as a subscription, 2.1 s later on average and 3.5 s at most, and never went backwards. Each poll is one call of two node requests (`spikes/crates/chain-errors`, `P24`).

**Alternatives considered.**
- *Subscribe to finalized heads.* About 2 s fresher, but `BlockSource` cannot express it, and it would need its own resubscribe loop.

## D20. Measure lag and readiness against the schedule

**Decision.** `newest_scheduled(schedule, head)` is the reference point. Percent done is `(last_block − start_block) / (newest_scheduled(schedule, head) − start_block)`, and 100 when the divisor is not positive. A job is behind when `last_block < newest_scheduled(schedule, head − max_lag_blocks)`. `bt-domain` holds one table from worker name to `Schedule`.

**Reason.** The checkpoint row does not hold the schedule, and the lag of an `Every(k)` job cannot be computed without it (`spikes/crates/runner`, `P25`). Measured against the raw head, readiness fails the moment the head reaches the next scheduled block for the 100- and 500-block jobs, and stays failed until that snapshot commits. On synthetic heads, the rule above gives no false failure at 9,612,400 or 9,612,500, and real lag at 9,612,451 still fails (`spikes/crates/api`, `ready-schedule`). Percent done against the raw head prints 0.0 % for a caught-up `validators` job (`spikes/crates/runner`, `progress`).

**Alternatives considered.**
- *Lag against the raw head.* No schedule table, but `/readyz` fails at every scheduled block for the 100- and 500-block jobs.

## D21. Call the node in two layers

**Decision.** A node call passes through two functions.

```rust
// crates/chain/src/source.rs. Used only by SubxtSource.
async fn classified<T>(method: &'static str, block: BlockNumber, spec: Option<SpecVersion>,
    request: impl Future<Output = Result<T, subxt::Error>>) -> Result<T, ChainError>;
fn classify(err: subxt::Error, method: &'static str, block: BlockNumber, spec: Option<SpecVersion>,
    elapsed: Duration) -> ChainError;

// On Chain<S>.
async fn call<T, Fut>(&self, method: &'static str, block: BlockNumber, request: impl FnMut() -> Fut)
    -> Result<T, ChainError>
where Fut: Future<Output = Result<T, ChainError>>;
```

- `SubxtSource` wraps each raw request in `classified`, which turns `subxt::Error` into `ChainError` by the arms of D22. No other code sees `subxt::Error`.
- `spec` is `None` for the calls inside `block_ref` and `finalized_head`, which run before any spec is known. `finalized_head` passes block 0.
- `Chain::call` wraps each `BlockSource` method. It applies `rpc_timeout`, the permit, the rate token (D23), the retry, and the counters, `bt_rpc_disconnects_total` included.
- `Chain::for_worker(&self, worker: &'static str) -> Chain<S>` returns a `Chain` that labels its metrics with `worker`. `main` builds one per job with `J::NAME`, and one for the head tracker with `"head"`. They share the source, the metadata cache, and the limits behind `Arc`.

**Reason.** Each layer holds what only it can hold.
- `Chain<S: BlockSource>` is generic (D18), and every `BlockSource` method returns `ChainError`, so `Chain<S>` never holds a `subxt::Error`. Only `SubxtSource` does, so classification sits there.
- A hung call must time out on every source, `FixtureSource` included, so the timeout sits in the generic layer.
- A future runs once, so `call` takes a closure and makes a new future for each attempt.
- `classify` takes a block number and an optional spec, because a `BlockRef` does not exist yet for the call that makes it. `chain_getBlockHash` has a number and no hash, and `finalized_head` has neither.
- `chain.block_ref(n)` and `job.read(&chain, at)` carry no worker, so each `Chain` carries the `worker` label of `bt_rpc_requests_total`, `bt_rpc_in_flight` and `bt_rpc_permit_wait_seconds` itself.

The runner stages run with this shape on the keyed node, with a `SubxtSource` on the reconnecting client (`spikes/crates/runner-live`, `wiring`).

**Alternatives considered.**
- *One function that classifies, times out, and retries.* A generic `Chain<S>` never sees a `subxt::Error`, and a future passed in cannot be retried.
- *The timeout inside `SubxtSource`.* A hung call on `FixtureSource` would never time out.
- *A worker argument on every `Chain` method.* Every job's `read` would have to carry it.

## D22. Classify chain errors by what the node sends, and bound the client's waits with our own timeouts

**Decision.** `Disconnected`, `RateLimited`, and `Unknown` carry the `subxt::Error` they came from, boxed. `classify` (D21) has these arms:
- `DisconnectedWillReconnect` is `Disconnected`, wherever subxt nests it.
- The client's own `RequestTimeout`, in the reconnecting client's shape and the plain client's, is `ChainError::Timeout` with `after: elapsed`.
- JSON-RPC user error code `-32029` or `-32004` is `RateLimited`. Both are in `RATE_LIMIT_CODES`, and `-32029` is the one the keyed node sends.
- `BlockNotFound` and `MetadataMissing` come from the subxt errors that mean them.
- The node's state error codes stay `Unknown` and stop the process: 4003 for an unknown block on `state_getStorage`, `state_call` and `state_getKeysPaged`, and for a failing runtime API; 4001 for an unknown block on `state_queryStorageAt`; 4002 for a `state_getKeysPaged` count above 1000 (`spikes/crates/chain-errors`, `node-error-codes`).
- A decode error carries the spec of the metadata used for the decode. `DecodeError.spec` is `executed_by` for storage and events, and `state` for a runtime API result. `DecodeKind` derives `Debug, Clone, Copy, PartialEq, Eq`. Events decode with subxt 0.50.3's `decode_fields_unchecked_as::<T>()` (`spikes/crates/chain-errors`, `P18`).

The client is subxt's reconnecting client, behind the `reconnecting-rpc-client` feature, which the manifest names. It retries one URL forever and has no failover, so the indexer uses the first URL of `chain.urls` (D30). Our own timeouts bound every wait on it:
- Each call runs inside our `tokio::time::timeout` of `chain.rpc_timeout` (D21). A call made during an outage waits until then.
- `connect` builds the client with `ReconnectingRpcClient::builder().request_timeout(chain.rpc_timeout + 30 s)`, so our timeout fires first, and the `RequestTimeout` arm is a second line of defence.
- `connect` wraps the build in `tokio::time::timeout(chain.rpc_timeout, builder.build(url))`. On timeout it makes one plain `RpcClient::from_url` attempt and puts that attempt's error in the message. The message names the URL's index in `BT_CHAIN__URLS`, never the URL.

**Reason.** `Unknown` stops the process, so each error that a healthy run can meet has an arm of its own.
- A raw call in flight at a dropped socket fails with `DisconnectedWillReconnect` (`spikes/crates/chain-errors`, `P14`).
- The keyed node answers a rate limit with `-32029` (`spikes/crates/chain-errors`, `ratelimit-classify`). subxt's `is_rpc_limit_reached` matches only chainHead's `limitReached`, and subxt sends 1 request and gives up on `-32029` (`spikes/crates/chain-errors`, `subxt-no-retry-on-limit`).
- Both clients have their own request timeout of 60 s, below `rpc_timeout`. A hung call fails at 60.00 s as `OtherRpcClientError(Client(RpcError(RequestTimeout)))`. With `request_timeout` above `rpc_timeout` the outer timeout fires first, and the arm turns the client's timeout into `Timeout` (`spikes/crates/chain-errors`, `client-request-timeout`). When a node lists `archive_v1_*` methods, as the keyed node does, `runtime_apis().call_raw` tries `archive_v1_call` and then `state_call`, so it fails only after two client timeouts, 120 s at the default. Our outer timeout bounds it.
- An unknown hash, a pruned block, and a failing runtime API all answer code 4003 on the single-block reads, so no separate variant can tell them apart (`spikes/crates/chain-errors`, `unknown-block`).
- `MetadataNotFound` comes only from `OfflineClient::at_block`, and a real call with no metadata registered returns it (`spikes/crates/chain-errors`, `P15`, `metadata-missing`).
- `size_of::<subxt::Error>()` is 240 bytes, which trips `clippy::result_large_err` by value (`spikes/crates/chain-errors`, `P16`).
- A block number alone cannot fill `DecodeError.spec`, so `classify` takes the spec.

The client's waits:
- The reconnecting client retries forever inside `build` (`subxt-rpcs-0.50.3/src/client/reconnecting_rpc_client/mod.rs:331-334`), so without a timeout a wrong URL hangs at startup with no error (`spikes/crates/chain-errors`, `connect-timeout`). The URL can hold a key, so the error names only its index.
- After startup, a call on the reconnecting client whose handshake is rejected sees no error and waits until the outer timeout (`spikes/crates/chain-errors`, `reconnecting-handshake-rejected`). The plain client returns an error at once (`spikes/crates/chain-errors`, `ws-handshake-rejected`).
- On the real gateway a wrong key gets `403 Forbidden` on the handshake. The reconnecting `build` stays pending, while the plain client returns `Rejected { status_code: 403 }` in about 0.3 s, and a wrong path gives 404 the same way. Neither error text holds the key or the host (`spikes/crates/chain-errors`, `gateway-handshake-rejected`). Without the plain attempt, a wrong key waits the full `rpc_timeout` and reports only a timeout.
- A URL with no key is not rejected. It connects and runs at 10 calls a second, so it fails later with `-32029`, not at startup (`spikes/crates/chain-errors`, `gateway-keyless`).
- The feature is opt-in (`spikes/crates/chain-errors`, `P17`). The client has no failover (`spikes/crates/chain-errors`, `url-failover`), and it holds a call through an outage (`spikes/crates/chain-errors`, `reconnect-outage`).

**Alternatives considered.**
- *subxt's `is_rpc_limit_reached` for rate limits.* It never matches a JSON-RPC rate limit.
- *The client's default `request_timeout` of 60 s.* It fires before `rpc_timeout`, so the client, not our timeout, ends a slow call.
- *Move to the next URL after repeated failures.* subxt has no mechanism for it.
- *Call `build` with no timeout.* A wrong URL hangs the process with no error.
- *Report only the connect timeout.* A wrong key or path then waits 90 s and does not say which.

## D23. Cap the call rate with a one-token bucket, not only the calls in flight

**Decision.**
- `chain.rpc_max_per_second`, default `120`, sets a token bucket that every node call passes. The bucket holds at most one token, so calls start at least `1 / rpc_max_per_second` apart. A caller that finds no token takes the next free slot under the lock and sleeps until it, so callers are served in order.
- `chain.rpc_max_in_flight` defaults to `16`, and a job's backfill `concurrency` to `8`. A call takes its permit first and its rate token last.
- `chain.rpc_timeout` defaults to `90s` and covers the call only. The wait for a permit and a token is outside it and has no limit. `bt_rpc_permit_wait_seconds` shows that wait. D22 sets the client's own timeout above it.
- The bucket counts `BlockSource` calls. `finalized_head` is one call and two requests.
- The bucket uses `tokio::time::Instant`, which advances on paused time in tests.
- `rand::rng()` is created inside the `backoff` call and never held across an await.

**Reason.** Measured on the keyed node on 2026-11-12, the limit is 200 calls a second per key (`x-ratelimit-limit-sec: 200`). A limited call answers JSON-RPC `-32029`, inside HTTP 429 over HTTP (`spikes/crates/chain-errors`, `ratelimit-shape-http`), and as a per-call error over WebSocket with the socket left open (`spikes/crates/chain-errors`, `ratelimit-shape-ws`). 133 calls a second ran clean for 60 s (`spikes/crates/chain-errors`, `sustain-http-historic`). Permits alone do not bound the rate. A permit frees as soon as its call returns, and the p50 is 90 to 140 ms. The cap of 120 is 60 % of the limit, because the key is shared.

One `get_all_validator_baskets` call takes 8.7 to 33.5 s, depending on node load (`spikes/crates/chain-decode`, `basket-call-time`), and historic `state_call` latency varies from 0.3 s to 19 s for one call (`spikes/crates/chain-errors`, `statecall-variance`), hence 90 s.

The public archive node drew no limit error in three runs, the last of 47 calls mixing `state_getKeysPaged` and `state_call` (`spikes/crates/chain-errors`, `P32`). Release 1 runs on the keyed node.

The cap sets the floor of the release 1 backfill. About 757,672 calls take about 1 h 30 min to 1 h 45 min at 120 calls a second (649,384 calls at 2.53 `epochs` calls per block, 757,672 at 3.0). The `epochs` term of 3.0 calls per block, measured near the head, is an upper end, because a block at 9,260,000 took 2.53 (`spikes/crates/chain-decode`, `backfill-r1`). [Ingestion](design/03-ingestion.md#estimate-the-backfill-time) gives the whole estimate.

The bucket, measured on 2026-11-13:
- With one token, a 1 s window at a cap of 120 holds 120 calls on paused time (`spikes/crates/runner-live`, `bucket-burst`).
- On the keyed node, the most attempts started in any 1 s window was the cap plus one: 41 at 40, 11 at 10, 3 at 2, and 2 at 1 (`spikes/crates/runner-live`, `cap-window`).
- With the permit first, a caller that queues for a permit holds no token. At a cap of 10 with 2 permits, 12 readers and two slow calls, 10 calls started in one window (`spikes/crates/runner-live`, `token-permit-order`).
- At a cap of 1 with 8 reads, each call waits 9 s for its token under a 5 s `rpc_timeout`. With the timeout around the call only, all 85 attempts succeed and the run exits 0 (`spikes/crates/runner-live`, `timeout-scope`).
- One `finalized_head` attempt takes one token and makes two node requests, counted through a proxy (`spikes/crates/runner-live`, `head-two-requests`).
- `ThreadRng` is not `Send`, so a generator held across the backoff sleep would keep a read from being spawned. `backoff(policy, attempt, &mut rand::rng())` creates and drops it in one statement (`spikes/crates/runner-live`, `rng-send`).

**Alternatives considered.**
- *Bound the load with permits alone.* The rate then grows as latency falls, and the node answers `-32029`.
- *A shorter `rpc_timeout`, such as 30 s.* The basket call alone takes up to 34 s.
- *A bucket that holds a second of tokens.* It absorbs bursts, but it lets twice the cap into one window.
- *The token before the permit.* Calls that wait for a permit keep their tokens, and a window holds twice the cap.
- *A timeout that also covers the wait.* It stops the process when the cap is far below demand.

## D24. Axum for the API

**Decision.** Serve the API with Axum 0.8 and `tower-http` middleware.

**Reason.** Axum runs on the same Tokio and `tower` stack as the rest of the workspace. `tower-http` provides tracing, request IDs, compression, and CORS as layers, and tower provides the request timeout (D25). `utoipa-axum` builds the router and the OpenAPI document from the same functions.

**Alternatives considered.**
- *Actix Web.* It works well, but it has its own runtime model and middleware system, separate from the `tower` stack the rest of the workspace uses.

## D25. Shed load on the compat routes, and do not rate-limit or cache in process

**Decision.** The API limits concurrent requests on the compat routes and answers the overflow with 503 at once (`spikes/crates/api`, `n-plus-1`). The API pool's `acquire_timeout` is 2 seconds, shorter than the 10 second request timeout. Release 1 has no per-IP rate limit and no in-process response cache.

`HandleErrorLayer`, tower's `TimeoutLayer`, `load_shed`, and `GlobalConcurrencyLimitLayer` wrap the compat router only. Request ID, trace, metrics, propagate, CORS, catch panic, and compression wrap everything. `/healthz`, `/readyz`, and `/status` stay up under overload. A query cut by `statement_timeout` (SQLSTATE 57014) maps to `Unavailable`, 503 with `Retry-After: 1`. [API](design/05-api.md#middleware-stack) gives the layer order (`spikes/crates/api`, `stack-order`).

**Reason.** Without a limit, requests queue on the pool. 939 requests waited at once, and 600 timed out after 10 to 12 seconds. With shedding, at most 64 waited and the rest got 503 within 2 seconds.

The health checks sit outside the shed stack, because an orchestrator could otherwise restart a process that is only busy. At a limit of 4 and 8 held requests, `/pools` answers 503 while `/healthz` and `/status` answer 200.

A query cut by `statement_timeout` is load, not a bug, so it answers 503 and stays out of the error rate (`spikes/crates/api`, `stmt-timeout`).

tao-data-api is one client behind one address, and it has its own per-client limiter (`tao-data-api/src/index.ts:27`). A per-IP limit at the indexer would throttle that one client. tao-data-api also caches each route for 5 to 15 minutes and lets concurrent requests wait on one refresh, so an indexer cache would save almost nothing.

**Alternatives considered.**
- *`tower_governor` per client IP.* It throttles the one real client, and behind a proxy every client shares one IP.
- *A `moka` cache keyed by block, with ETags.* It adds invalidation rules for a load that tao-data-api's own cache already absorbs.
- *One shed stack over every route.* Less wiring, but an overloaded process then answers `/healthz` and `/status` with 503 (`spikes/crates/api`, `healthz-shed`, `global-limit`).
- *A limit per route with the plain `ConcurrencyLimitLayer`.* It sheds within a route and limits each route separately, so the routes do not share one limit (`spikes/crates/api`, `per-route-limit`).
- *tower-http's `TimeoutLayer`.* Deprecated since 0.6.7, and it answers 408 with no body (`spikes/crates/api`, `tower-http-timeout`).

## D26. Set `Cache-Control` on successful compat responses only

**Decision.** `Cache-Control: public, max-age=12` goes on successful (2xx) compat responses only. A 400 or a 503 never carries it, and the ops routes never do. A `map_response` layer on the compat router sets the header when the status is a success.

**Reason.** A 400, or the 503 that a handler answers for a pool timeout, must not be cacheable, or a client could cache an error for one block time. The `map_response` layer sets the header on 2xx responses and on nothing else (`spikes/crates/api`, `cache-control-2xx`).

**Alternatives considered.**
- *`SetResponseHeaderLayer::overriding` on the compat router.* One line, but it marks errors as cacheable.

## D27. Hold amounts in `u128`, and write the format helpers lint-clean

**Decision.** `AmountString` wraps a `u128` with `From<u64>`. The format helpers in `bt-domain` take their scale as a const generic, for example `fmt_scaled<const SCALE: u32>(value: u128)`, and pass `clippy::arithmetic_side_effects` as they stand.

**Reason.** `global_weighted_stake` is a `NUMERIC(39,0)` `u128`. A stored value of 18446744073709551620 serves only through `AmountString(u128)`, and a `u128` written as a JSON number fails tao-data-api's Zod schema (`spikes/crates/api`, `P27`, `db-u128`). The helpers pass the lint and match the squid on 27,026 rows (`spikes/crates/api`, `P33-lint-clean`). A runtime `scale` panics at 39, and a `const SCALE` turns that into a compile error (`spikes/crates/api`, `fmt-overflow`).

**Alternatives considered.**
- *`AmountString` over `u64`.* It cannot hold `global_weighted_stake`.
- *`#[expect(clippy::arithmetic_side_effects)]` on helpers that take a runtime `scale`.* It passes the lint (`spikes/crates/api`, `P33-lint-expect`), but it keeps the panic at a `scale` of 39.

## D28. Anchor APY on the indexed head and break rank ties by hotkey

**Decision.** These are deliberate deviations from the squid.

- The APY routes anchor both the 30-day row filter and the annualizing window at the chain time of the newest block the job has committed, never at `now()`. Validator APY uses the `epochs` checkpoint and root APR uses the `baskets` checkpoint ([analytics](design/08-analytics.md#compatibility-numbers)).
- `rank` in `GET /validators`, and the order of rows with equal stake, break ties by `hotkey`. `GET /validators/{hotkey}/subnets` breaks ties by `netuid`.

**Reason.** The squid filters epoch rows by wall-clock `NOW()` but annualizes over a window that ends at its indexed head (`bittensor-squid/api/src/routes/validators.ts:113` against `validators.ts:73-75`). When it lags, it counts fewer epochs over the same window. When the indexer is caught up, our anchor matches the squid within about 24 seconds of window. When it lags, ours stays consistent with itself. The squid's `ROW_NUMBER()` has no tiebreaker (`bittensor-squid/api/src/routes/validators.ts:270`), so its order for tied rows is arbitrary, and any fixed order is one it could return.

**Alternatives considered.**
- *Copy `NOW()`.* A lagging indexer would report a lower APY for no reason in the data.
- *No tiebreaker.* Rank would change between two identical requests, and golden tests could not pin it.

## D29. Return a null APY where the squid's SQL errors

**Decision.** At a 100 % take with 10 or more earning epochs, the gross APY of that row is `null` and the net APY is computed. When the compounded value overflows, both are `null`. `compat diff` treats a squid 500 on those rows as expected. APY fields compare through `apy_matches`, never `==`. This is a deliberate deviation from the squid, like those in D28.

**Reason.** In both situations the squid's SQL raises an error, `division by zero` or `value out of range: overflow`, and the error fails its whole route. Where the squid returns a value, in 27 of 30 cases, our formula equals the squid's own SQL on PostgreSQL 17, with a largest relative difference of 1.0e-12 (`spikes/crates/api`, `apy-formula`). serde_json writes `0.0` where the squid writes `0`, and `serde_json::Value` treats the two as different (`spikes/crates/api`, `value-zero`, `serde-floats`).

**Alternatives considered.**
- *Copy the squid's error.* One row would fail the whole route.
- *Compare APY values with `==`.* `0.0` and `0` differ.

## D30. Read the node URLs, the database URL, and the API address from config

**Decision.**
- `chain.urls` accepts one bare URL string or a list. A string is split on commas. A test loads the bare form.
- `database.url`, set as `BT_DATABASE__URL`, is a config key. `serve` reads the same key. The deployment gives `serve` the URL of the read-only API role, and `ingest` and `migrate` the URL of the writer role.
- `api.listen_addr`, default `0.0.0.0:8080`, is where `serve` binds.
- Tests do not call `std::env::set_var`. They use `figment::Jail`.
- `load()` boxes the figment error.

**Reason.**
- With `Env::prefixed("BT_").split("__")` and `urls: Vec<Url>`, `BT_CHAIN__URLS=wss://example.invalid:443` fails with `invalid type: found string "wss://example.invalid:443", expected a sequence for key "CHAIN.URLS"`, and only `BT_CHAIN__URLS=[wss://example.invalid:443]` loads. The key accepts both forms, so a bare URL loads too.
- By the `__` rule, `BT_DATABASE__URL` is the key `database.url`. The API connects as a read-only role (D16), so `serve` gets a URL of its own from its environment.
- `serve` needs a bind address, and the compose table publishes 8080.
- `std::env::set_var` is `unsafe` in edition 2024, and the workspace sets `unsafe_code = "forbid"`.
- The figment error trips `clippy::result_large_err` by value.

**Alternatives considered.**
- *A list-only `chain.urls`.* A bare `export BT_CHAIN__URLS=wss://...` then fails at start.
- *A separate key for the API's database URL.* Each process already gets its own environment, so one key serves both roles.

## D31. Export ingest metrics on 9100 and serve metrics on 9101

**Decision.** `telemetry.metrics_addr` defaults to `0.0.0.0:9100`. `bt-indexer ingest` uses the default. `bt-indexer serve` overrides the default to `0.0.0.0:9101`.

**Reason.** Distinct ports let both processes run on one host without a collision. The service table in [quality](design/09-quality.md) gives `ingest` 9100 and `serve` 9101 for metrics.

**Alternatives considered.**
- *One port for both processes.* One default to remember, but two processes on one host would collide.

## D32. Name the crates `bt-<name>`

**Decision.** Directories are `crates/{domain,chain,ingest,store,api,telemetry}` and `bin/indexer`. Packages are `bt-domain`, `bt-chain`, `bt-store`, `bt-ingest`, `bt-api`, `bt-telemetry`, and `bt-indexer`. Every command uses `-p bt-<name>`.

**Reason.** Every crate name must start with `bt_`. Cargo selects a package by its name, so a package named `bt-chain` does not match `-p chain` (`spikes/crates/chain-decode`, `P20`).

**Alternatives considered.**
- *A package named `chain` with `[lib] name = "bt_chain"`.* It works (`spikes/crates/chain-decode`, `P20-lib-name`), but the package and the library then have different names.

## D33. Put the types that several crates name in `bt-domain`

**Decision.**
- `Head` lives in `crates/domain/src/head.rs`.
- `NewRuntime` lives in `crates/domain/src/runtime.rs` and keeps `#[must_use]`.
- `BlockState` and each job's rows struct (`Prices`, `Epochs`, `Baskets`, `Identities`, `Validators`) live in `crates/domain/src/rows.rs`, because both `bt-chain` and `bt-store` name them.
- `Timestamp(pub chrono::DateTime<Utc>)` lives in `crates/domain/src/time.rs`.
- `bt-store` never depends on `bt-chain`.

**Reason.** Any other home for these types breaks the layering of the [overview](design/01-overview.md#workspace-layout).
- With `Head` in `bt-ingest` and `Store::upsert_chain_head(&Head)` in `bt-store`, the two packages depend on each other. `bt-api` also reads `chain_head`, and it may not use `bt-ingest`.
- With `NewRuntime` or `Epochs` in `bt-chain` and named by `bt-store`, `bt-api` depends on `bt-chain` through `bt-store`, which the overview forbids.
- With these types in `bt-domain`, the workspace is acyclic and layered as the overview says.

**Alternatives considered.**
- *Let `bt-store` depend on `bt-chain`.* `bt-api` then depends on the chain crate.
- *Give each store function its own tuple shape, such as `&[(BlockRef, Vec<EpochDividend>)]`.* Every job's write would need its own conversion, and `Head` and `NewRuntime` would still need a shared home.

## D34. Keep one fixture per test case, and record it by running the job's own read

**Decision.**
- Fixtures follow the layout in [quality](design/09-quality.md), `fixtures/state/<case>.json` and `fixtures/metadata/<spec>.scale`.
- A state fixture is one recording per test case, named for the case, not the block, for example `prices_head.json` or `prices_4920351.json`. The block number is inside the file. Tests that replay the same requests share a case, as two tests share `stake_added_5228683.json`.
- Metadata fixtures are about 0.2 to 0.45 MB per spec, 201,451 bytes for spec 219 and 444,782 bytes for the largest, the V16 `490.scale`. They are committed only for the specs a test uses.
- A whole validator snapshot fixture is 56 to 117 MB and is never committed whole. The test uses a trimmed subset.
- `bt-indexer fixture record --block <n> --case <name> --read <job>` records a fixture. The reads are `block_state`, `prices`, `events`, `epochs`, `baskets`, `identities`, `validators`, and `delegates`.
- The recorder wraps the live source in `Recorder<S: BlockSource>` and runs the same `Chain` read that the test runs, so the fixture holds exactly the requests the test makes.
- It always fetches both metadatas of the block, for `Decodes::Storage` and for `Decodes::RuntimeApi`.
- `Chain::metadata` calls `source.block_ref(n − 1)` on a cache miss, because the storage metadata comes from the parent hash and `BlockRef` holds no parent hash.
- `FixtureSource` answers `block_ref(n − 1)` for a recorded block n with `BlockRef { number: n − 1, hash: [0; 32], spec: SpecAt { executed_by: e, state: e } }`, where `e` is the fixture's `executed_by`. Metadata calls never go in the fixture's `calls`.
- For paged reads, `FixtureSource` flattens the recorded pages and cuts them again at the requested size after `start`.

**Reason.**
- Recording every case grows `fixtures/` to 365 MB, almost all of it validator snapshots (`spikes/crates/chain-decode`, `fixtures-size`).
- The per-subnet price API needs one call per netuid, and only the job read knows the netuids. For map items, a prefix scan also records a different set of requests from the one `Chain::prices` sends. Recorded through the job read, all ten price fixtures replay offline.
- Reading `System.Events` asks only for `Decodes::Storage`. A recording with that fetch alone holds `252.scale` and lacks `257.scale`, which the negative test needs. The two files are 224,549 and 227,933 bytes.
- `Chain::metadata` asks for the parent block, so `FixtureSource` must answer for it.
- In `calls`, the two metadata fetches at an upgrade block would share one key, because specs 488 and 490 both serve V16 with the same argument.
- Cut again at the requested size, 129 `NetworksAdded` keys come back as 129 at page sizes 1, 10, 128, 129, and 1,000.

**Alternatives considered.**
- *One file per block.* Two cases at the same block would share a file.
- *Commit every recorded fixture.* 365 MB in the repository.
- *Record one storage item or runtime API at a time.* It cannot give a runtime API its arguments, and it records different requests from the ones the test makes.
- *Fetch only the metadata a read asks for.* An upgrade block then lacks the second spec's file.
