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

`fixtures/state/` holds real chain state recorded from an archive node. Each fixture is the raw SCALE bytes that one job reads at one block, meaning the storage values and runtime API responses, together with the block number and the spec. The metadata for each spec is stored once in `fixtures/metadata/<spec>.scale`.

`bt-indexer fixture record --block <n>` writes a fixture from a live node. We record at least one block per era for each value a job reads, one before and one after each shape change that `bt-indexer metadata diff` reports. For example, the price has eras at specs 290 and 391, and the basket runtime API changed its response at spec 460.

Each decoder test loads a fixture, decodes it, and compares the result with an `insta` snapshot. A change in decoded output then appears as a reviewable diff of the snapshot file.

The fixtures make decoder tests run offline, in milliseconds, and the same way every time. A test that fetched from a public node would fail when the node was slow or pruned.

In release 2, `fixtures/blocks/` adds recorded blocks with their events, for the `events` job.

### Test the store against a real database

Store tests start `timescale/timescaledb` with `testcontainers`, apply the migrations, and run real queries. Each test uses its own schema, so tests run in parallel. A mock database would not catch a wrong index, a TimescaleDB restriction, or a `NUMERIC` conversion error.

### Test the runner end to end without a node

The `chain` crate exposes a `BlockSource` trait. Production uses the `subxt` implementation. Tests use `FixtureSource`, which serves recorded state, can add latency, and can inject failures.

```rust
pub trait BlockSource: Send + Sync + 'static {
	fn finalized_head(&self) -> impl Future<Output = Result<BlockRef, ChainError>> + Send;
	fn block_ref(&self, number: BlockNumber) -> impl Future<Output = Result<BlockRef, ChainError>> + Send;
	fn runtime_api(&self, at: BlockRef, call: RuntimeCall) -> impl Future<Output = Result<Vec<u8>, ChainError>> + Send;
	fn storage(&self, at: BlockRef, query: StorageQuery) -> impl Future<Output = Result<StoragePage, ChainError>> + Send;
}
```

The methods return `impl Future + Send` instead of using `async fn`. A plain `async fn` in a public trait does not promise that its future is `Send`, so a generic job could not pass it to `tokio::spawn`. The [`Job` trait](03-ingestion.md#run-every-job-on-one-runner) uses the same form. Both traits are used generically, never as `dyn`.

These tests run the real runner against `FixtureSource` and a real database.

- **The same blocks twice.** Run a job over a range, then over the same range again. Every table is identical after the second run.
- **A crash in the middle of a batch.** `FixtureSource` fails on the 300th block of a 500-block batch, after every retry. The process stops with an error. A new run then covers the same range. Every table matches a run that never failed.
- **A panic in a read.** `FixtureSource` panics on one block. The runner returns an error, not success, and the checkpoint stays before that block.
- **Shutdown while a stage waits.** Cancel the token while the writer holds rows and the read stage waits on a slow node call. The runner returns within 1 second, and the checkpoint includes the rows the writer held.
- **Backpressure keeps other jobs running.** See below.

### Prove that a stalled writer does not starve other jobs

Two jobs share the global RPC permits. If one job's writer stalls, its reads must give their permits back, or the other job stops too. Each read runs in its own task for this reason. A read future that sits in a paused `buffered` stream, never polled, keeps its permit.

The test runs on paused tokio time, so it is deterministic and takes milliseconds.

1. Configure `chain.rpc_max_in_flight = 8`. Give job A a concurrency of 8 and job B a concurrency of 4. `FixtureSource` answers every call after 50 ms.
2. Run B alone and measure its node calls per second.
3. Run A and B together, with A's writer blocked on a `Notify` that never fires.
4. Assert that B's rate is at least 80% of its rate when alone.
5. Assert that `bt_rpc_in_flight{worker="A"}` reaches 0 while A's writer is blocked.

With A's concurrency equal to the global cap, a version without the spawned reads is expected to let A's unpolled reads hold all 8 permits, which stops B. When you write the test, remove the `tokio::spawn` once and confirm that the test fails on step 4. A test that checks only that each channel stays within its capacity cannot fail, because a bounded channel enforces its capacity itself.

### Test the API through the router

API tests build the real router with a test database and send requests with `tower::ServiceExt::oneshot`. No port is opened. Each test checks the status, the headers, and the body. Response bodies are `insta` JSON snapshots. Error tests check that each failure in the [middleware table](06-error-handling.md#map-every-middleware-failure-to-apierror) returns a problem body with the right status.

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
- **Drift check.** `node tests/compat/drift.mjs <tao-data-api checkout>` extracts the five declarations from the checkout's files and fails if any differs from the copy. A scheduled CI job runs it against tao-data-api's main branch every day. A schema change upstream therefore shows up as a failing job, and the fix updates the copy and `TDA_COMMIT` together.

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
- An APY value `a` matches the squid's `b` when `|a − b| ≤ 10^-4 × (1 + |b|)`. The squid annualizes from its own head timestamp and ours from the `epochs` checkpoint, which can differ by a few blocks. A shift of one minute in a 30-day window changes `ln(1 + APY)` by about 2 × 10^-5 of its value, so 10^-4 leaves room for that and for the unordered float sum. This bound is an estimate. Revisit it if the shadow run shows anchor differences larger than a few blocks.
- Row order must match, except within a run of equal sort values.

**Documented deviations.** It reports these separately, not as differences.

- A `rank` or row order that differs only within a tie, because we break ties by hotkey.
- An APY value of a pair with an epoch row, or a hotkey with a basket snapshot, whose `ts` lies between our 30-day boundary and the squid's. That row is inside one window and outside the other, because of the head anchor.

Release 1 is done when `compat diff` reports no differences, apart from documented deviations, for 24 hours.

### Check the analytics against the chain

- `global_weighted_stake` for a sample of 20 validators, recomputed from storage at a recorded block, matches the `validator` row to the rao.
- In release 2, the slippage port matches `sim_swap` results to the rao, as [analytics](08-analytics.md#slippage-quote) describes.
- In release 2, the folded positions for a sample of 100 coldkeys match `get_stake_info_for_coldkey` at a recorded block.

### Benchmark the hot paths

`criterion` benchmarks cover decoding one `TotalHotkeyAlpha` page, building the `/pools` response for 129 subnets, and writing a 500-block batch of `subnet_price` rows to a test database. The README reports the numbers along with the machine they ran on. CI runs the benchmarks without comparing results, so they keep compiling. Compare results only on the same machine.

## CI checks every pull request

GitHub Actions runs these jobs. A pull request merges only when all of them pass.

| Job | Command |
|---|---|
| Format | `cargo fmt --all --check` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
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
4. Environment variables with the prefix `BT_`, where `__` separates levels. `BT_CHAIN__URLS` sets `chain.urls`.

The result deserializes into one `Config` struct with typed fields, such as `Url`, `Duration`, and `NonZeroU32`. The binary validates the struct at start and exits with a clear message for an invalid value, before any job starts. Jobs receive their part of the struct and never read the environment.

Secrets, such as the database password, come only from environment variables. `Config` wraps them in `secrecy::SecretString`, so they never appear in `Debug` output or logs.

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
| `db` | `timescale/timescaledb:latest-pg17`, pinned to a digest | 5432 |
| `migrate` | `bt-indexer migrate` | none. It runs once, then exits |
| `ingest` | `bt-indexer ingest` | 9100 for metrics |
| `api` | `bt-indexer serve` | 8080 for the API, 9101 for metrics |
| `postgres-exporter` | `prometheuscommunity/postgres-exporter` | 9187 |
| `prometheus` | `prom/prometheus` | 9090 |
| `grafana` | `grafana/grafana` | 3000 |

`ingest` and `api` start only after `migrate` has finished. The node is not part of the stack. `BT_CHAIN__URLS` points at an archive node. Prometheus scrapes `ingest` as job `bt-ingest` and `api` as job `bt-api`, the names the [alerts](07-observability.md#alert-on-symptoms) use.

`docker compose up` with a node URL brings up a working indexer, the API on `localhost:8080`, and dashboards on `localhost:3000`. `docker compose exec ingest bt-indexer status --watch` then shows each job's block, mode, and ETA. The README opens with those two commands.

## Release

- Versions follow SemVer. A `v*` tag builds the image, pushes it to GitHub Container Registry, and publishes a GitHub release.
- `git-cliff` writes `CHANGELOG.md` from Conventional Commits.
- Each release notes the highest runtime spec it has decoder fixtures for. A chain upgrade beyond that spec needs a run of `bt-indexer metadata diff` before the next deploy.
