# Stage 2. Build the walking skeleton

This stage runs one job end to end. `prices` reads each new finalized block from the real node, writes its rows and its checkpoint in one transaction, and `bt-indexer status` shows the block. It closes no roadmap check yet. It is the first end-to-end demo, built on the `Chain` and `BlockSource` shape from stage 1. Before you start, stage 1 must be done and Docker must run on your machine.

The skeleton reads one block at a time, on purpose. Stage 4 puts the real runner in place of its loop.

## Step 9. `feat(store): apply the pipeline migrations with bt-indexer migrate`

- **Goal.** A database runs in Docker, `bt-indexer migrate` creates `runtime_version`, `checkpoint`, and `chain_head`, and a second run changes nothing ([Storage](../design/04-storage.md#run-migrations-as-a-separate-step), [D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests)).
- **Files.** `deploy/compose.yaml` (the database only), `crates/store/{Cargo.toml,src/lib.rs,src/error.rs}`, `crates/store/migrations/0001_pipeline_state.sql`, `crates/store/tests/support/mod.rs`, the `migrate` subcommand. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum StoreError {
      #[error("row not found")] NotFound,
      #[error("{value} does not fit column {column}")] Overflow { column: &'static str, value: i128 },
      #[error("database migrations do not match this binary")] Version(#[source] MigrateError),
      #[error("database error")] Db(#[source] sqlx::Error),
  }
  impl From<sqlx::Error> for StoreError { fn from(err: sqlx::Error) -> Self { todo!() } }
  impl StoreError { pub fn disposition(&self) -> Disposition { todo!() } }
  #[derive(Clone, Copy, Debug)]
  pub struct PoolSettings { pub max_connections: u32, pub acquire_timeout: Duration, pub statement_timeout: Duration }
  impl PoolSettings { pub const fn ingest() -> Self { todo!() } } // 10, 5 s, 60 s
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct MigrateReport { pub applied: Vec<String> } // such as "0001_pipeline_state"; empty when up to date
  #[derive(Clone)]
  pub struct Store { pool: PgPool }
  impl Store {
      pub async fn connect(url: &str, settings: PoolSettings) -> Result<Store, StoreError> { todo!() }
      pub async fn migrate(url: &str) -> Result<MigrateReport, StoreError> { todo!() } // one PgConnection, not the pool
      pub async fn check_version(&self) -> Result<(), StoreError> { todo!() }
  }
  ```

- **Who writes what.** The agent writes the compose service with the image pinned as `timescale/timescaledb:2.31.0-pg17` and its digest, the DDL copied from [Storage](../design/04-storage.md#pipeline-state), and the test support module. The compose service sets `POSTGRES_USER`, `POSTGRES_PASSWORD`, and `POSTGRES_DB` to `bt`, publishes `127.0.0.1:55432:5432`, and has a `pg_isready` healthcheck. Port 55432 keeps clear of any local Postgres on 5432. Then run this line in your shell. The `--database-url` flag reads it, and step 32 reads the same name as the config key `database.url` ([D30](../decisions.md#d30-read-the-node-urls-the-database-url-and-the-api-address-from-config)).

  ```sh
  export BT_DATABASE__URL=postgres://bt:bt@127.0.0.1:55432/bt
  ```

  You write `StoreError`, its `From`, and `Store`. Review checks:
  - A failed `check_version` returns `StoreError::Version` with the `MigrateError` that sqlx gave (`VersionMissing`, `VersionMismatch`, or `VersionNotPresent`).
  - `MigrateReport` names each migration as its version in `{:04}_` form and its sqlx description with spaces turned into underscores, because sqlx stores the description as `pipeline state` (`spikes/crates/store`, `migrate-report`).
  - `From` is written by hand, and `RowNotFound` becomes `NotFound`.
  - The outer match over `StoreError` in `disposition` has no wildcard arm. The inner match over `sqlx::Error` needs one, because `sqlx::Error` is `#[non_exhaustive]` and SQLSTATEs are strings.
  - Retry covers the full list of [Error handling](../design/06-error-handling.md#classify-errors-in-one-place): `PoolTimedOut`, `Io`, and SQLSTATE 40001, 40P01, 57014, 57P01, 57P02, and 57P03.
  - `migrate` runs on one connection and exits on error, since a failed `Migrator::run` keeps its advisory lock on a pooled connection.
  - `ingest` and `serve` will call `check_version`, never `migrate`.
  - `PoolSettings` holds the maximum connections, the `acquire_timeout`, and the `statement_timeout`. `connect` sets `statement_timeout` on each connection in `after_connect`, with the ingest defaults of [Ingestion](../design/03-ingestion.md#bound-every-call-with-a-timeout-a-rate-cap-and-a-permit), 10 connections, 5 s, and 60 s.
  - The URL comes from `--database-url` with `env = "BT_DATABASE__URL"` until step 32 loads configuration.
  - `0001` starts with `CREATE EXTENSION IF NOT EXISTS timescaledb;`, because a database made from `template0` has none.
  - The test support starts one container per test binary and gives each test its own database, cloned from a migrated template.
  - The container comes from `testcontainers-modules` 0.15.0 with the `postgres` feature, with the image name and tag set to the compose image.
  - The test container starts with `-c timescaledb.max_background_workers=0`. The TimescaleDB scheduler keeps a session open in the template database, and `CREATE DATABASE ... TEMPLATE` then fails with SQLSTATE 55006.
  - The support module holds one `tokio::sync::Mutex` around the clone, because two clones of one template at once fail the same way.
  - `tests/support/mod.rs` opens with `#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]`, because the `clippy.toml` keys cover only `#[test]` functions, and each test binary uses only part of the module.
  - The container lives in a static, which is never dropped, so it is not removed when the test binary exits. It gets a fixed name per test binary, such as `bt-test-` and the crate name of the test, and the support module removes a leftover container of that name before it starts one. `docker rm -f $(docker ps -aq --filter name=bt-test-)` clears the containers by hand.

  The same DDL applies in `spikes/sql/migrations/0001_pipeline_state.sql` (`spikes/crates/store`, `ddl-applies`, `migrate-lock`, `test-db-per-test`).
- **Concepts.**
  - *A hand-written `From` and `?`.* `?` calls `From::from` on the error, so the `From` impl decides what each `sqlx::Error` means to callers. Trap: `#[from]` on `Db` looks tidy, but it puts `RowNotFound` into `Db`, and the API answers 500 where it means 404. Source: The Rust Book, "Recoverable Errors with `Result`", section "A Shortcut for Propagating Errors: the `?` Operator".
  - *`sqlx::migrate!`.* The macro embeds the SQL files into the binary at compile time, so the deployed binary carries exactly the migrations it was built with. Trap: expecting it to read `migrations/` from disk at run time, as Knex or TypeORM do. Source: [sqlx, `migrate!`](https://docs.rs/sqlx/0.8.6/sqlx/macro.migrate.html).
- **Check.** `docker compose -f deploy/compose.yaml up -d --wait db && cargo run -p bt-indexer -- migrate` prints `applied 0001_pipeline_state`. Keep `--wait`, which holds the command until the healthcheck passes. Without it, `migrate` runs while the container is still starting and fails with `Connection reset by peer`. A second `migrate` prints `database is up to date`. `cargo test -p bt-store` passes.

## Step 10. `feat(ingest): follow the finalized head and show it in status`

- **Goal.** `bt-indexer ingest` runs a head tracker that polls `chain_getFinalizedHead` every 3 seconds, publishes the finalized block on a `watch` channel, and upserts `chain_head`. `bt-indexer status` prints it ([D19](../decisions.md#d19-track-the-head-by-polling)).
- **Files.** `crates/domain/src/head.rs` (`Head`), `crates/ingest/{Cargo.toml,src/lib.rs,src/error.rs,src/head.rs}` (`track_head`), `crates/store/src/progress.rs`, the `ingest` and `status` subcommands. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  // in bt-domain, because bt-store and bt-api name it and may not depend on bt-ingest (D33)
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct Head { pub number: BlockNumber, pub ts: Timestamp }
  #[derive(Debug, thiserror::Error)]
  pub enum RunnerError { #[error(transparent)] Chain(#[from] ChainError), #[error(transparent)] Store(#[from] StoreError) }
  pub async fn track_head<S: BlockSource>(chain: Arc<Chain<S>>, store: Store, tx: watch::Sender<Option<Head>>) -> Result<(), RunnerError> { todo!() }
  impl Store { pub async fn upsert_chain_head(&self, head: &Head) -> Result<(), StoreError> { todo!() } }
  impl Store { pub async fn chain_head(&self) -> Result<Option<Head>, StoreError> { todo!() } }
  ```

- **Who writes what.** You write `track_head`, `RunnerError`, and the `select!` in `main`. The agent writes the two queries, the `status` printer, and a round-trip test for `chain_head`. Review checks:
  - The tracker polls and never subscribes, because a subscription ends on any socket drop.
  - The tracker never publishes a lower block than it published before.
  - It reads the head's time from `Timestamp.Now`.
  - `ctrl_c` stops `ingest` with exit code 0.
  - `status` reads the database, not the node.
  - `Head` is in `bt-domain`, so `bt-store` and `bt-api` can name it ([D33](../decisions.md#d33-put-the-types-that-several-crates-name-in-bt-domain)).
  - `finalized_head` is already a `BlockSource` method from step 5.

  Step 26 adds variants to `RunnerError` and keeps the two `#[from]` arms, so the `?` in this step's code keeps compiling. The probe `spikes/crates/chain-errors/src/bin/head_poll.rs` polls the same way. Over 120 s it reached the same last head as a subscription, 2.1 s later on average, at 2 calls per 3 s (`spikes/crates/chain-errors`, `P24`).
- **Concepts.**
  - *`tokio::sync::watch`.* The channel holds one slot with the newest value, so a slow reader skips stale heads and always sees the latest. Trap: reading it as an event emitter that delivers every value. A runner that counts on seeing each head misses blocks. Source: [tokio, `sync::watch`](https://docs.rs/tokio/1.54.1/tokio/sync/watch/index.html).
  - *`tokio::select!` with `ctrl_c`.* `main` races the tracker against `tokio::signal::ctrl_c()`, and the losing branch's future is dropped. Trap: expecting the dropped branch to run to the end, as a losing promise in `Promise.race` does. Source: tokio tutorial, "Select".
- **Check.** With `cargo run -p bt-indexer -- ingest` running in one terminal, `cargo run -p bt-indexer -- status` in another prints `HEAD <n> finalized <s>s ago`, where `<n>` is near the head that `chain spec` prints and `<s>` is about 30 to 45 seconds, because the finalized head trails the newest block (31 to 40 s over six runs). The number grows when you run it again.

## Step 11. `feat(ingest): index prices live with rows and checkpoint in one transaction`

- **Goal.** The walking skeleton. `prices` reads each new finalized block with step 8's head-era read, then writes the block's `subnet_price` rows and its checkpoint in one transaction. `status` shows the job's block. The job starts at the head in M1, and step 33 adds the history windows in M2 ([D3](../decisions.md#d3-set-the-milestone-scope-by-what-release-1-reads)).
- **Files.** `crates/domain/src/schedule.rs` (`Schedule` and `History`), `crates/ingest/src/{job.rs,live.rs,jobs/prices.rs}`, `crates/store/migrations/0002_subnet_price.sql`, `crates/store/src/{prices.rs,checkpoint.rs}`, `.sqlx/`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Schedule { EveryBlock, Every(NonZeroU32) }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum History { Full, Window(Duration), Latest }
  pub trait Job: Send + Sync + 'static {
      const NAME: &'static str;
      type Rows: Send + Sync + 'static;
      fn schedule(&self) -> Schedule;
      fn history(&self) -> History;
      fn read<S: BlockSource>(&self, chain: &Chain<S>, at: BlockRef) -> impl Future<Output = Result<Self::Rows, ChainError>> + Send;
      fn block_ts(rows: &Self::Rows) -> Timestamp;
      fn write(&self, tx: &mut PgTransaction<'_>, batch: &[Self::Rows]) -> impl Future<Output = Result<(), StoreError>> + Send;
  }
  #[derive(Clone, Copy, Debug)]
  pub struct CheckpointUpdate { pub worker: &'static str, pub last_block: BlockNumber, pub last_block_ts: Timestamp,
      pub target_block: BlockNumber, pub blocks_per_sec: f32 }
  pub async fn commit_checkpoint(tx: &mut PgTransaction<'_>, cp: CheckpointUpdate) -> Result<(), StoreError> { todo!() }
  ```

  `Schedule` and `History` are `Copy`, because step 12's `schedules()` hands out entries and its `progress()` uses one twice. The live loop writes `blocks_per_sec` as 0. Step 33 computes it, and until then `status` shows `RATE` as `0.0/s`.
- **Who writes what.** Before the agent's part, set up `query!`. It checks each statement against a live database at compile time, and three things stop it the first time.
  1. Install the sqlx CLI once.

     ```sh
     cargo install sqlx-cli --version 0.8.6 --locked --no-default-features --features postgres,rustls
     ```

  2. `query!` reads `DATABASE_URL`, not `BT_DATABASE__URL`, so run `export DATABASE_URL=$BT_DATABASE__URL` in the same shell.
  3. A `query!` on a table from a new migration does not compile until that migration is applied, and `bt-indexer migrate` cannot be built while `bt-store` does not compile. So apply each new migration with the CLI, then build.

     ```sh
     cargo sqlx migrate run --source crates/store/migrations
     cargo sqlx prepare --workspace
     ```

  `cargo sqlx prepare` without `--workspace` fails at the root with `failed to get package in current working directory`. Commit `.sqlx/`. A later `bt-indexer migrate` then prints `database is up to date`. Every later step that adds a migration and a `query!` on it follows the same order.

  You write `Job`, `PricesJob`, `commit_checkpoint`, and the sequential live loop. The agent writes the DDL from [Storage](../design/04-storage.md#prices), the column builder, the `UNNEST` `query!`, its round-trip test, and `cargo sqlx prepare --workspace`. Review checks:
  - The trait is the one in `spikes/crates/runner/src/job.rs`, which compiles against `sqlx::PgTransaction` in `src/pg_shape.rs` ([D18](../decisions.md#d18-give-every-job-one-trait-and-build-the-runner-from-a-spawned-read-stage-and-a-batching-writer); `spikes/crates/runner`, `job-trait`, `job-bounds`).
  - `Rows` is `Sync` because the writer task holds `&[Rows]` across `write().await`.
  - `Every` takes a `NonZeroU32`, so step 12's `newest_scheduled` never divides by zero.
  - `write` takes the whole batch, and `block_ts` gives the writer `last_block_ts`.
  - The column builder asserts equal array lengths, because `UNNEST` pads a short array with NULL.
  - A new checkpoint row is inserted with `last_block_ts = to_timestamp(0)`, `target_block = start_block - 1`, and `blocks_per_sec = 0`, using `ON CONFLICT (worker) DO NOTHING` ([D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests); `spikes/crates/store`, `P12`).
  - The update has `WHERE last_block < $new`.
  - No node call happens while a transaction is open.

  The step likely passes 400 changed lines, so it lands as two commits. Land the agent's part first as `feat(store): add the subnet_price table and its batch insert`, then the job and the loop under this step's subject. That first commit holds the DDL and the `query!` together, so it compiles only after `cargo sqlx migrate run` has applied `0002`, as above.
- **Concepts.**
  - *A trait with an associated const and type.* `NAME` and `Rows` are fixed per job at compile time, so one generic loop drives every job with no boxing and no downcasts. Trap: a base class with an overridable `run()`. Source: The Rust Reference, "Associated Items".
  - *sqlx `Transaction`.* Dropping a transaction that was not committed rolls it back, so an early `?` between the rows and the checkpoint leaves neither. Trap: a `try`/`finally` that you must remember to write. Source: [sqlx, `Transaction`](https://docs.rs/sqlx/0.8.6/sqlx/struct.Transaction.html).
  - *`query!` offline data.* The macro checks each SQL statement against a live database at compile time, and `cargo sqlx prepare` saves the result in `.sqlx/` so CI builds with `SQLX_OFFLINE=true`. Trap: committing a query change without rerunning `prepare`, which breaks CI and not your machine. Source: [sqlx, `query!`](https://docs.rs/sqlx/0.8.6/sqlx/macro.query.html), section "Offline Mode".
- **Check.** After `migrate`, run `ingest`. `cargo run -p bt-indexer -- status` prints `prices live <n>` one or two blocks behind `HEAD`. `psql "$BT_DATABASE__URL" -c 'select count(*) from subnet_price'` grows by one row per subnet per block. At 9,490,000, `current_alpha_price_all` returns 129 entries, and dropping root leaves 128.

## Step 12. `feat(domain): compute lag, percent done, and ETA in one function`

- **Goal.** One pure function turns a checkpoint row, the job's schedule, and the head into every progress number, and `status` prints the full table of [Ingestion](../design/03-ingestion.md#the-command-line). Lag and percent done measure to `newest_scheduled(schedule, head)`, not to the raw head ([D20](../decisions.md#d20-measure-lag-and-readiness-against-the-schedule)).
- **Files.** `crates/domain/src/progress.rs`, `crates/domain/src/schedule.rs`, `bin/indexer/src/status.rs`.
- **Shape.**

  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Mode { Backfill, Live } // Display and the checkpoint.mode column use "backfill" and "live"
  pub struct Checkpoint { pub worker: String, pub mode: Mode, pub start_block: BlockNumber, pub last_block: BlockNumber,
      pub last_block_ts: Timestamp, pub target_block: BlockNumber, pub blocks_per_sec: f32, pub updated_at: Timestamp }
  pub struct Progress { pub lag: u32, pub percent_done: f64, pub eta: Option<Duration> }
  pub fn progress(cp: &Checkpoint, schedule: Schedule, head: BlockNumber) -> Progress { todo!() }
  pub fn newest_scheduled(schedule: Schedule, head: BlockNumber) -> BlockNumber { todo!() }
  pub fn schedules() -> Vec<(&'static str, Schedule)> { todo!() } // in bt-domain, worker name to schedule
  impl fmt::Display for Progress { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { todo!() } }
  ```

- **Who writes what.** You write `progress`, `newest_scheduled`, and `schedules`. The agent writes the table printer, its column widths, and `status --watch`, which prints the table again every 2 seconds until Ctrl-C ([Ingestion](../design/03-ingestion.md#the-command-line)). Review checks:
  - Lag is `newest_scheduled(schedule, head) − last_block`, saturating.
  - Percent done is `(last_block − start_block) / (newest_scheduled(schedule, head) − start_block)`, and 100 when the divisor is not positive.
  - Each integer subtraction is `saturating_sub` or `checked_sub`, because step 3's deny lint rejects a bare `-` in `bt-domain`.
  - An `Every(100)` job that committed 9,490,800 has lag 0 until the head reaches 9,490,900.
  - `eta` is `None` in live mode.
  - Percent prints with `{:.1}`.
  - `schedules` is the one table from worker name to `Schedule`, and each job's `schedule()` returns its own entry, so no second list exists to drift.
  - `Checkpoint` holds every column of the `checkpoint` table of [Storage](../design/04-storage.md#pipeline-state), so `/status` (step 31) and the APY anchors (step 46) read it without changing it.
  - `Display for Progress` prints `lag=<n> done=<p>% eta=<s>s`, or `eta=-` with no ETA, for log lines. The status table formats each column itself.
  - `blocks_per_sec` is 0 until step 33, so `RATE` prints `0.0/s` and `eta` stays `None`.

  The tests live in `#[cfg(test)] mod tests` of `crates/domain/src/progress.rs`, under the module path `progress::tests::`, which the check's filter `progress` matches. A reference version is `spikes/crates/runner/src/progress.rs`, with its tests in `tests/progress.rs` (`spikes/crates/runner`, `progress`, `P25`).
- **Concepts.**
  - *`impl Display`.* `status`, the log line, and later the metrics label all format `Progress` through one impl, so `{}` prints the same text everywhere. Trap: a `toString()` habit that formats inline at every call site. Source: [std, `fmt::Display`](https://doc.rust-lang.org/std/fmt/trait.Display.html).
- **Check.** `cargo test -p bt-domain progress` passes, including `every_100_lag_is_zero_until_next_multiple` and `caught_up_scheduled_job_reads_100_only_with_the_newest_scheduled_denominator`. With `ingest` running, `cargo run -p bt-indexer -- status` prints the header `WORKER MODE BLOCK HEAD LAG DONE RATE ETA COMMITTED` and a `prices live` row with a lag of 2 or less. After a restart the sequential loop catches up at about 5 s a block, against a new block about every 12 s, so `LAG` falls slowly. Wait until it stops falling before you read it. `cargo run -p bt-indexer -- status --watch` prints the table again every 2 seconds, and Ctrl-C ends it.

## Stop here

- **Demo.** Start the database, run `migrate`, start `ingest`, and run `status`. The `prices` row follows the head, and `subnet_price` grows each block. Stop `ingest` with Ctrl-C, start it again, and the job continues from its checkpoint.
- **Explain without notes.**
  - Why must the rows and the checkpoint commit in one transaction, and what does a crash between them leave behind?
  - What does dropping an uncommitted sqlx `Transaction` do?
  - Why is the head on a `watch` channel and not on a queue?
  - Why does the `StoreError` conversion from `sqlx::Error` have to be written by hand?
  - Why does lag need the job's schedule?
- **Roadmap.** None yet. This is the first end-to-end demo. Roadmap M1 closes in stage 4, once the real runner, shutdown, and the two crash tests exist.
