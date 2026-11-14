# Quality

This document covers how we test the indexer, how we prove it is compatible with tao-data-api, what CI checks, how configuration loads, and how the indexer is built and deployed.

## Test what a user of each crate can observe

Each test asserts behavior that a caller of the crate sees, such as a decoded value, a row in the database, or a response body. A test that breaks when only the internals change gets rewritten.

### Test the domain rules directly

The `domain` crate is pure, so its tests need no database and no node.

- Unit tests for each conversion, such as fixed-point bits to a ratio and `TaoWeight` to a fraction.
- Unit tests for the [compatibility number](08-analytics.md#compatibility-numbers) helpers, with expected strings taken from the squid's code, not from a run of ours. The cases include `"0"` for zero, a whole price with no decimal point, a 9-digit fraction with trailing zeros removed, a price change that truncates toward zero, the 16-digit cap, and a negative change.
- `proptest` properties for arithmetic. An overflow returns an error and never wraps. Adding alpha from two subnets fails.
- In release 2, `proptest` properties for the slippage quote, listed in [analytics](08-analytics.md#slippage-quote).

### Test the decoders against recorded blocks

`fixtures/state/` holds real chain state recorded from an archive node. Each fixture is one recording per test case, named for the case, not the block, as `fixtures/state/<case>.json`, for example `prices_head.json` or `prices_4920351.json`. It holds the raw SCALE bytes that one job reads at one block, meaning the storage values and runtime API responses, together with the block number and both spec numbers of `SpecAt { executed_by, state }`. The metadata for each spec is stored once in `fixtures/metadata/<spec>.scale`. Two tests share `stake_added_5228683.json`, and every other case has its own file ([D34](../decisions.md#d34-keep-one-fixture-per-test-case-and-record-it-by-running-the-jobs-own-read)).

`bt-indexer fixture record --block <n> --case <name> --read <job>` writes a fixture from a live node. The reads are `block_state`, `prices`, and `events`, then `epochs`, `baskets`, `identities`, `validators`, and `delegates` as the jobs arrive. The recorder wraps the live source in `Recorder<S: BlockSource>` and runs the same `Chain` read that the test runs, so the fixture holds exactly the requests the test makes, with the arguments each runtime API needs. The recorder always fetches both metadatas of the block, for storage and for runtime APIs, so an upgrade block gets both spec files, even for a read such as `System.Events` that asks only for the storage metadata.

`FixtureSource` answers what `Chain` asks of a live node.

- `Chain::metadata` calls `block_ref(n − 1)` on a cache miss. For a recorded block n, `FixtureSource` answers `BlockRef { number: n − 1, hash: [0; 32], spec: SpecAt { executed_by: e, state: e } }`, where `e` is the fixture's `executed_by`.
- Metadata calls never go in the fixture's `calls`. At an upgrade block the two fetches would collide on one key, because both specs serve V16 with the same argument.
- For paged reads it flattens the recorded pages and cuts them again at the requested size after `start`, so a read returns the same keys at any page size.

We record at least one block per era for each value a job reads, one before and one after each shape change that `bt-indexer metadata diff` reports. For example, the price has eras at specs 290 (block 5,947,548) and 391 (block 7,782,857), and the basket runtime API dropped `weights` at mainnet spec 464 (block 9,088,597).

Each decoder test loads a fixture, decodes it, and compares the result with an `insta` snapshot. A change in decoded output then appears as a reviewable diff of the snapshot file. Storage bytes decode through `frame-decode` with a `subxt::Metadata` built from the fixture's metadata file, the same path the online decoder takes, because subxt's own storage value type has a `pub(crate)` constructor. Storage and events of block n decode with the metadata of the spec that executed n, and runtime API responses at n with the metadata of n's own state spec ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)).

`spikes/data/decode-cases.csv` lists 35 recorded cases. This path decodes all of them offline, across every era of every item, and the 8 upgrade blocks among them decode with the executing spec's metadata into the shapes that rule predicts (`spikes/crates/chain-decode`, `decode-table`). For release 1's window, the executing and state specs' metadatas give equal values for every key of every item at all 10 upgrade blocks of specs 477 to 490 (`spikes/crates/chain-decode`, `storage-executing-spec`).

**Which fixtures release 1 exercises.** Release 1 reads only specs 475 to 490, because its longest window reaches back only to about block 9,260,462 at the head of 9,490,862 on 2026-11-12 (`spikes/crates/chain-specs`, `P29`, [D3](../decisions.md#d3-set-the-milestone-scope-by-what-release-1-reads)). The fixtures for the older eras (before 261, before 290, 290, 391, 423, and the 441 to 459 basket shape with `weights`) serve release 2, so they stay in the repository.

**What is committed.** A metadata file is about 0.2 to 0.45 MB per spec, 201,451 bytes for spec 219 and 444,782 bytes for the largest, the V16 `490.scale`. Commit metadata only for the specs a test uses. A whole validator snapshot is 56 to 117 MB of state per block, so it is never committed whole. The `validators` decoder test uses a trimmed subset of that state.

The fixtures make decoder tests run offline, in milliseconds, and the same way every time. A test that fetched from a public node would fail when the node was slow or pruned.

In release 2, `fixtures/blocks/` adds recorded blocks with their events, for the `events` job.

### Test the store against a real database

Store tests start the pinned `timescale/timescaledb:2.31.0-pg17` image with `testcontainers`, one container per test binary, and run real queries. The harness migrates one template database once. Each test then gets its own database, cloned from that template, so tests run in parallel ([D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests)). A mock database would not catch a wrong index, a TimescaleDB restriction, or a `NUMERIC` conversion error.

| Isolation | Time per test (measured on 2026-11-12) |
|---|---|
| Container per test | 2.84 s, and 3.78 s for four at once |
| Database per test, created and migrated | 103 to 184 ms |
| Database per test, cloned from a migrated template | 28 ms |

The clone keeps the hypertables, the retention jobs and the migration history, and a later `migrate` on it applies nothing (`spikes/crates/store`, `test-db-per-test`). The container starts with `-c timescaledb.max_background_workers=0`, so a scheduled retention run cannot drop back-dated test rows. A test that needs a policy to run calls `CALL run_job(id)` (`spikes/crates/store`, `test-bg-jobs`). The flag also keeps the TimescaleDB scheduler from holding a session in the template, which would make the clone fail with SQLSTATE 55006.

The support module in `tests/support/mod.rs` follows these rules.

- It holds one mutex around the clone, because two clones of one template at once fail with the same SQLSTATE 55006.
- Each test binary names its container and removes a leftover one before it starts, because a container held in a static is not removed when the test binary exits.
- The file opens with `#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]`, because the clippy settings for tests cover only `#[test]` functions and each test binary uses only part of the module.
- Tests never reuse a pool that ran migrations, because a failed run keeps its advisory lock on that pool's connection.

### Test the runner end to end without a node

The `chain` crate exposes a `BlockSource` trait. Production uses the `subxt` implementation. Tests use `FixtureSource`, which serves recorded state, can add latency, and can inject failures.

```rust
pub trait BlockSource: Send + Sync + 'static {
    fn finalized_head(&self) -> impl Future<Output = Result<BlockRef, ChainError>> + Send;
    fn block_ref(&self, number: BlockNumber) -> impl Future<Output = Result<BlockRef, ChainError>> + Send;
    fn runtime_api(&self, at: BlockRef, call: RuntimeCall) -> impl Future<Output = Result<Vec<u8>, ChainError>> + Send;
    fn storage(&self, at: BlockRef, query: StorageQuery) -> impl Future<Output = Result<StoragePage, ChainError>> + Send;
    fn code_hash(&self, n: BlockNumber) -> impl Future<Output = Result<[u8; 32], ChainError>> + Send;
    fn spec(&self, number: BlockNumber) -> impl Future<Output = Result<SpecVersion, ChainError>> + Send;
}

pub struct StoragePage {
    pub entries: Vec<(Vec<u8>, Option<Vec<u8>>)>,
}
```

`code_hash` and `spec` are the two calls of the spec bisection, `state_getStorageHash(":code")` and `state_getRuntimeVersion`. They are trait methods so that `FixtureSource` can serve the bisection too. subxt's `LegacyRpcMethods` has no `state_getStorageHash`, so `SubxtSource` keeps its `RpcClient` and sends the request by name. `code_hash` takes a block number, not a `BlockRef`, so a probe costs two requests.

The methods return `impl Future + Send` instead of using `async fn`. A plain `async fn` in a public trait does not promise that its future is `Send`, so a generic job could not pass it to `tokio::spawn`. The [`Job` trait](03-ingestion.md#run-every-job-on-one-runner) uses the same form. Both traits are used generically, never as `dyn`.

These tests run the real runner against `FixtureSource` and a real database.

- **The same blocks twice.** Run a job over a range, then over the same range again. Every table is identical after the second run.
- **A crash in the middle of a batch.** `FixtureSource` fails on the 300th block of a 500-block batch, after every retry. The process stops with an error. A new run then covers the same range. Every table matches a run that never failed.
- **A panic in a read.** `FixtureSource` panics on one block. The runner returns an error, not success, and the checkpoint stays before that block.
- **Shutdown while a stage waits.** Cancel the token while the writer holds rows and the read stage waits on a slow node call. The runner returns within 1 second, and the checkpoint includes the rows the writer held.
- **Backpressure keeps other jobs running.** See below.

### Prove that a stalled writer does not starve other jobs

Two jobs share the global RPC permits. If one job's writer stalls, its reads must give their permits back, or the other job stops too. Each read runs in its own task for this reason. A read future that sits in a paused `buffered` stream, never polled, keeps its permit.

The test runs on real time. A's writer calls `store.begin()` on the real test database before its `write` reaches the blocked gate, and paused tokio time auto-advances during real I/O. On paused time, a 5 s timeout around a 200 ms socket reply fires after 21 µs of wall time (`spikes/crates/runner`, `paused-time`). With 50 ms calls and a 10 s window, the test takes seconds.

1. Configure `chain.rpc_max_in_flight = 8`. Give job A a concurrency of 8 and a channel capacity of 8, and job B a concurrency of 4. `FixtureSource` answers every call after 50 ms.
2. Run B alone and measure its node calls per second.
3. Run A and B together, with A's writer blocked on a `Notify` that never fires.
4. Over a 10 s window that starts once A's read stage has stopped sending, assert that B's rate is at least 80% of its rate when alone.
5. Assert that `bt_rpc_in_flight{worker="A"}` reaches 0 while A's writer is blocked.
6. Assert that A holds no permit: once B is idle, `permits.available_permits()` equals the total.

Set the test's `chain.rpc_max_per_second` above B's rate alone. Under a cap at or below it, the rate check passes with broken reads. On the keyed node at a cap of 40, with 8 permits, a stalled job with unspawned reads holds 3 permits, and the other job still runs at 9.79 blocks a second against 10.51 alone. Only the free permit count, 5 of 8, shows the loss (`spikes/crates/runner-live`, `stalled-neighbour`). The rate assertion in step 4 is therefore the primary check above the cap, and step 6 catches what the rate hides under one.

B runs at 80.0 calls a second alone and at 100% of that with spawned reads (`spikes/crates/runner`, `read-starvation`, `two-jobs`). The window starts once A's read stage has stopped sending, and A's channel holds 8, because while A is still filling its channel B's measured rate sits close to the threshold.

When you write the test, break it twice and confirm that it fails each time.

- **Remove the `tokio::spawn` once.** A's unpolled reads then hold 6 of the 8 permits, 2 in calls and 4 granted to semaphore waiters that no task polls, and B falls to 50% of its rate alone. The test fails on steps 4, 5, and 6.
- **Hold each permit across the send to the writer once.** A then holds all 8 permits, B falls to 0%, and A's `bt_rpc_in_flight` reads 0. Step 5 passes, so steps 4 and 6 catch this version.

A test that checks only that each channel stays within its capacity cannot fail, because a bounded channel enforces its capacity itself.

### Test the API through the router

API tests build the real router with a test database and send requests with `tower::ServiceExt::oneshot`. No port is opened. An overload test holds every compat permit and checks that a compat route answers 503 while `/healthz`, `/readyz` and `/status` still answer ([D25](../decisions.md#d25-shed-load-on-the-compat-routes-and-do-not-rate-limit-or-cache-in-process)). Each test checks the status, the headers, and the body. Response bodies are `insta` JSON snapshots. Error tests check that each failure in the [middleware table](06-error-handling.md#map-every-middleware-failure-to-apierror) returns a problem body with the right status.

## Prove compatibility with tao-data-api

Release 1 is compatible when tao-data-api can point `INDEXER_DATA_API_1` or `INDEXER_DATA_API_2` at the indexer and get the same five calls answered with the same shapes and values. Four checks prove it, from the cheapest to the most complete.

### Port the squid's golden tests

The squid has router tests that seed a real database and assert exact responses. We port their seeds and assertions to Rust router tests in `crates/api/tests/compat/`. The squid tests call the `/v1/` aliases. Our ports call the unversioned paths that tao-data-api uses, because we serve only those.

| Squid test | Seeds | Assertions |
|---|---|---|
| `api/src/routes/validators.test.ts` | `:80-136` | A hotkey with 10 or more epochs has net and gross above 0 (`:176-180`). An idle hotkey on a measurable subnet has 0 and 0 (`:182-184`). A hotkey at 100% take has 0 and null (`:186-188`). A hotkey with fewer than 10 epochs has null and null (`:190-192`). The root APR equals its closed form to 10 decimal places, with gross null (`:203-215`). The root APR is null for a share-price step over 2 (`:217-222`), and for zero shares or zero NAV mid-window (`:224-229`). An idle hotkey is 0 on a measurable subnet and null on a young one (`:231-237`) |
| `api/src/routes/subnet-identities.test.ts` | `:8-19` | The whole `/v1/subnet-identities` body, including the `subnets` envelope, camelCase keys, nulls for unset fields, and netuid order (`:25-61`) |

The squid seeds rows relative to `NOW()`. Our ports set the `epochs` and `baskets` checkpoints' `last_block_ts` to the same instant, because our APY windows anchor there.

No squid test covers `/pools` or `/validators`. New fixtures cover them:

- `/pools` formatting, from the helper cases above, through the router.
- The tip-block rule. A netuid with no row at the tip block is left out, and the day-ago row comes from the 24 to 26 hour window only.
- `price_change_1_day` and `tao_volume_24_hr` are `"0"` with no day-ago row, and the volume is `"0"` when the counter went down.
- `/validators` drops rows with `global_weighted_stake` 0, and `rank` breaks ties by hotkey.
- `global_weighted_stake` and `stake` are JSON strings.

### Validate our responses with tao-data-api's own schemas

tao-data-api parses every indexer response with a Zod schema, and a response that fails it becomes a 502 for its clients. A test runs those schemas against our responses.

tao-data-api does not export the schemas. They are module-level constants in `src/endpoints/pools.ts:21-29`, `validators.ts:18-25`, `subnetValidators.ts:29-34`, `validatorSubnets.ts:32-37`, and `subnetIdentities.ts:33-47`. So `tests/compat/tda-schemas.mjs` holds a verbatim copy of those five declarations, and `tests/compat/TDA_COMMIT` names the tao-data-api commit they were copied from (`6a46bbf` at the time of writing). `tests/compat/package.json` pins `zod` to the version in tao-data-api's lock file, 3.25.76.

- **Schema test.** A Rust test in `crates/api/tests/compat/` sends each compatibility request through the router and pipes the body to `node tests/compat/check.mjs <route>`. The script parses it with the matching schema and exits nonzero on failure. The test fails, not skips, when `node` is missing, so CI cannot pass it silently.
- **Drift check.** `node tests/compat/drift.mjs <tao-data-api checkout>` extracts the five declarations from the checkout's files and fails if any differs from the copy. A scheduled CI job runs it against tao-data-api's main branch every day. A schema change upstream therefore shows up as a failing job, and the response is to update the copy and `TDA_COMMIT` together.

### Diff live responses against the squid

`bt-indexer compat diff --squid <url>` fetches the five routes from the squid and from the indexer and compares them. It reads our database directly as well as our API, so it can align the two sides to the same block. It prints every difference. It runs every 10 minutes during the shadow run.

**What it fetches.** `/pools`, `/validators`, `/v1/subnet-identities`, `/subnets/{netuid}/validators` for every netuid in our `/pools` response plus 0, and `/validators/{hotkey}/subnets` for the top 50 hotkeys by rank and 50 random ones.

**How it aligns the two sides.** Both indexers move every block, so two responses fetched at the same moment can describe different blocks.

- For `/pools`, it finds the block in our last 100 `subnet_price` blocks whose `(netuid, total_tao, total_alpha)` set equals the squid's response, and compares against our response computed at that tip. No match is a difference.
- For the validator and identity routes, it fetches only while our `validators` checkpoint is at a snapshot block `b` and the head is between `b + 20` and `b + 80`. Both sides then serve snapshot `b` while the squid is less than 20 blocks behind.

**How it compares.**

- It matches rows by `netuid`, by `hotkey`, or by both. A missing or extra row is a difference.
- It compares only the keys that tao-data-api's schemas keep. Zod strips the rest, such as `nominator_stake`, `basket`, and `updatedAtBlock`, so their values never reach a client.
- Strings and integers must match exactly. Null against a number is a difference.
- APY fields compare through `apy_matches`, never `==`. serde_json writes `Some(0.0)` as `0.0` where the squid's JavaScript writes `0`, and `serde_json::Value` equality treats those two as different (`spikes/crates/api`, `value-zero`).
- An APY value `a` matches the squid's `b` when `|a − b| ≤ 10^-4 × (1 + |b|)`. The squid annualizes from its own head timestamp and ours from the `epochs` checkpoint, which can differ by a few blocks. A shift of one minute in a 30-day window changes `ln(1 + APY)` by about 2 × 10^-5 of its value, so 10^-4 leaves room for that and for the unordered float sum. This bound is an estimate. Revisit it if the shadow run shows anchor differences larger than a few blocks.
- Row order must match, except within a run of equal sort values.

**Documented deviations.** It reports these separately, not as differences.

- A `rank` or row order that differs only within a tie, because we break ties by hotkey.
- An APY value of a pair with an epoch row, or a hotkey with a basket snapshot, whose `ts` lies between our 30-day boundary and the squid's. That row is inside one window and outside the other, because of the head anchor.
- A squid 500 on a route whose rows include a 100 % take with 10 or more earning epochs, or a compounded APY that overflows. The squid's SQL raises an error there, and ours serves null for that value ([D29](../decisions.md#d29-return-a-null-apy-where-the-squids-sql-errors)).

Release 1 is done when `compat diff` reports no differences, apart from documented deviations, for 24 hours.

### Check the analytics against the chain

- `global_weighted_stake` for a sample of 20 validators, recomputed from storage at a recorded block, matches the `validator` row to the rao.
- In release 2, the slippage port matches `sim_swap` results to the rao, as [analytics](08-analytics.md#slippage-quote) describes.
- In release 2, the folded positions for a sample of 100 coldkeys match `get_stake_info_for_coldkeys` at a recorded block.

### Benchmark the hot paths

`criterion` benchmarks cover decoding one `TotalHotkeyAlpha` page, building the `/pools` response for 128 subnets, and writing a 500-block batch of `subnet_price` rows to a test database. The README reports the numbers along with the machine they ran on. CI runs the benchmarks without comparing results, so they keep compiling. Compare results only on the same machine.

## CI checks every pull request

GitHub Actions runs these jobs. A pull request merges only when all of them pass.

| Job | Command |
|---|---|
| Format | `cargo fmt --all --check` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings`. Each `ChainError` variant that carries a `subxt::Error` boxes it, because it is 240 bytes by value and would otherwise trip `clippy::result_large_err` ([D22](../decisions.md#d22-classify-chain-errors-by-what-the-node-sends-and-bound-the-clients-waits-with-our-own-timeouts)) |
| Test | `cargo nextest run --workspace`, with Docker available for `testcontainers` and Node.js for the schema test |
| SQL metadata | `cargo sqlx prepare --workspace --check` |
| Dependencies | `cargo deny check`, for licenses, security advisories, and duplicate versions |
| Unused dependencies | `cargo machete` |
| OpenAPI | Generate `openapi.json` and fail if it differs from the committed file |
| Docs | `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS="-D warnings"` |
| Coverage | `cargo llvm-cov nextest`, reported on the pull request. CI does not enforce a threshold |
| Image | Build the Docker image |

A separate scheduled job runs the tao-data-api drift check once a day. It needs read access to the tao-data-api repository, so it does not run on pull requests from forks.

`SQLX_OFFLINE=true` makes the build use the committed `.sqlx/` query data, so only the test job needs a database.

The workspace sets lints once in `Cargo.toml` under `[workspace.lints]`. Each crate has `[lints] workspace = true`. [Error handling](06-error-handling.md#do-not-panic-in-library-code) lists the Clippy lints. The workspace also sets `unsafe_code = "forbid"` and `missing_docs = "warn"` for public items.

## Configuration

`figment` reads configuration in this order. A later source overrides an earlier one.

1. Defaults compiled into the binary.
2. `config/default.toml`.
3. The file in `BT_CONFIG`, if set.
4. Environment variables with the prefix `BT_`, where `__` separates levels. `BT_CHAIN__URLS` sets `chain.urls`, and `BT_DATABASE__URL` sets `database.url`.

`chain.urls` accepts one bare URL string or a list, and a string is split on commas, so `BT_CHAIN__URLS=wss://...` loads. A test loads the bare form. Config tests use `figment::Jail`, never `std::env::set_var`, which is `unsafe` in edition 2024 and so forbidden by `unsafe_code = "forbid"`. `load()` boxes the figment error, which would otherwise trip `clippy::result_large_err` ([D30](../decisions.md#d30-read-the-node-urls-the-database-url-and-the-api-address-from-config)).

The result deserializes into one `Config` struct with typed fields, such as `Url`, `Duration`, and `NonZeroU32`. The binary validates the struct at start and exits with a clear message for an invalid value, before any job starts. Jobs receive their part of the struct and never read the environment.

Secrets come only from environment variables. The database URL holds the password and each node URL holds the key, so `chain.urls` and `database.url` are both secrets. `Config` wraps them in `secrecy::SecretString`, so they never appear in `Debug` output or logs. A `chain.urls` held as `Url` would print `apikey=` in its `Debug` output.

## Build and deploy

### Build a small image

The `Dockerfile` has three stages.

1. `cargo-chef` computes the dependency recipe.
2. The builder compiles the dependencies from the recipe, then compiles the workspace. A code change then rebuilds only our crates.
3. The runtime stage copies the binary into `gcr.io/distroless/cc-debian12` and runs as a non-root user.

The image has one entry point, `bt-indexer`. The subcommand selects the role.

### Run everything with Docker Compose

`deploy/compose.yaml` starts the full stack for a demo or for local development.

| Service | Image | Port |
|---|---|---|
| `db` | `timescale/timescaledb:2.31.0-pg17`, pinned as `timescale/timescaledb@sha256:b56a3f51991df1f79d0c1a0841cad5c5fa8058fdd6a3ee6886394a6d4c174538` | 5432 inside, published at `127.0.0.1:55432` |
| `migrate` | `bt-indexer migrate` | none. It runs once, then exits |
| `ingest` | `bt-indexer ingest` | 9100 for metrics |
| `api` | `bt-indexer serve` | 8080 for the API, 9101 for metrics |
| `postgres-exporter` | `prometheuscommunity/postgres-exporter` | 9187 |
| `prometheus` | `prom/prometheus` | 9090 |
| `grafana` | `grafana/grafana` | 3000 |

`ingest` and `api` start only after `migrate` has finished.

The `db` service sets `POSTGRES_USER`, `POSTGRES_PASSWORD`, and `POSTGRES_DB` to `bt`, and has a `pg_isready` healthcheck. It publishes `127.0.0.1:55432:5432`, because port 5432 on a development machine is often taken. From the host, `export BT_DATABASE__URL=postgres://bt:bt@127.0.0.1:55432/bt`. Start it with `docker compose -f deploy/compose.yaml up -d --wait db`, so that `migrate` runs only once the healthcheck passes. `sqlx::query!` reads `DATABASE_URL`, not `BT_DATABASE__URL`, so a build that checks queries online also needs `export DATABASE_URL=$BT_DATABASE__URL`.

The database has two roles. `migrate` and `ingest` connect as the owner role, which creates every table. `api` connects as a read-only role. Each process gets its role's URL in `database.url` ([D30](../decisions.md#d30-read-the-node-urls-the-database-url-and-the-api-address-from-config)). A script in `deploy/db/init/` sets up the read-only role before the first migration, with `ALTER DEFAULT PRIVILEGES FOR ROLE <migrate role>`, because default privileges cover only the tables that the named role creates ([D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests)).

```sql
GRANT CONNECT ON DATABASE <database> TO <api role>;
GRANT USAGE ON SCHEMA public TO <api role>;
ALTER DEFAULT PRIVILEGES FOR ROLE <migrate role> IN SCHEMA public GRANT SELECT ON TABLES TO <api role>;
```

With this grant the API role reads every table and every hypertable chunk that later migrations create, and an `INSERT` or `UPDATE` fails with 42501 (`spikes/crates/store`, `api-role`). The node is not part of the stack. `BT_CHAIN__URLS` points at an archive node. Prometheus scrapes `ingest` as job `bt-ingest` and `api` as job `bt-api`, the names the [alerts](07-observability.md#alert-on-symptoms) use.

`docker compose up` with a node URL brings up a working indexer, the API on `localhost:8080`, and dashboards on `localhost:3000`. `docker compose exec ingest bt-indexer status --watch` then shows each job's block, mode, and ETA. The README opens with those two commands.

## Release

- Versions follow SemVer. A `v*` tag builds the image, pushes it to GitHub Container Registry, and publishes a GitHub release.
- `git-cliff` writes `CHANGELOG.md` from Conventional Commits.
- Each release notes the highest runtime spec it has decoder fixtures for. A chain upgrade beyond that spec needs a run of `bt-indexer metadata diff` before the next deploy.
