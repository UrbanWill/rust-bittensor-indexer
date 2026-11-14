# Stage 4. The real runner

This stage replaces the sequential loop of step 11 with the runner that [Ingestion](../design/03-ingestion.md#run-every-job-on-one-runner) describes. Reads run concurrently and in order, one writer commits batches, one token stops every task, and every node call shares one rate cap and one set of permits. It closes roadmap M1. Before you start, stage 3 must be done, because the runner tests serve recorded blocks from `FixtureSource` (step 15). The runner shapes below compile and pass their tests against a fake source in `spikes/crates/runner`, so copy these signatures ([D18](../decisions.md#d18-give-every-job-one-trait-and-build-the-runner-from-a-spawned-read-stage-and-a-batching-writer)).

Configuration arrives in step 32. Until then the config types this stage needs, `RpcLimits` (step 29), `TelemetryConfig` (step 30), and `ApiConfig` (step 31), take their values from `Default` impls that hold the defaults of [Ingestion](../design/03-ingestion.md#configuration), and `main` builds each with `Default::default()`. The node and database URLs still come from the flags and variables of steps 1 and 9. Step 32's loader then uses those same `Default` impls as its compiled defaults, so no default is written twice.

## Step 24. `feat(ingest): persist runtime versions and refill the cache on start`

- **Goal.** Every spec a job needs lands in `runtime_version` with its true `first_block` and its metadata, the first time a job needs it, and a restart loads the table instead of asking the node again. In M0 the table lives in memory, and M1 persists it ([D8](../decisions.md#d8-find-the-spec-table-by-bisection-over-code-and-persist-only-the-specs-a-job-window-touches)).
- **Files.** `crates/domain/src/runtime.rs`, `crates/chain/src/runtimes.rs`, `crates/chain/src/versions.rs`, `crates/chain/src/upgrades.rs`, `crates/chain/tests/specs.rs`, `crates/store/src/runtime_version.rs`, `crates/ingest/src/head.rs`, `bin/indexer/src/main.rs`.
- **Shape.**

  ```rust
  // crates/domain/src/runtime.rs, because bt-store names it and bt-store never depends on bt-chain (D33)
  #[must_use]
  pub struct NewRuntime { pub spec: SpecVersion, pub first_block: BlockNumber, pub metadata: Vec<u8> }
  impl Runtimes { pub fn load(rows: Vec<NewRuntime>) -> Result<Runtimes, ChainError> { todo!() } }  // the metadata cache
  impl SubxtSource { pub fn with_specs(self, table: Arc<RwLock<VersionTable>>) -> SubxtSource { todo!() } }  // block_ref reads specs here
  impl VersionTable { pub fn add(&mut self, spec: SpecVersion, first_block: BlockNumber) -> Result<(), SpecsError> { todo!() } }
  impl<S: BlockSource> Chain<S> {
      pub fn with_runtimes(self, runtimes: Runtimes) -> Chain<S> { todo!() }  // hands the loaded cache to the chain
      pub async fn learn_specs(&self, table: &VersionTable, range: RangeInclusive<BlockNumber>) -> Result<Vec<NewRuntime>, ChainError> { todo!() }
  }
  impl Store {
      pub async fn insert_runtime(&self, rt: &NewRuntime) -> Result<(), StoreError> { todo!() }
      pub async fn runtimes(&self) -> Result<Vec<NewRuntime>, StoreError> { todo!() }  // every stored row, for Runtimes::load
  }
  ```

- **Who writes what.** You write `NewRuntime`, `load`, `VersionTable::add`, `learn_specs`, and the sparse-reader tests. The agent writes `insert_runtime` and `runtimes` with their round-trip test, and `FixtureSource::with_specs`. Review checks:
  - Step 13's `VersionTable` answers `spec_at` and holds every row of `fixtures/specs.csv`, the base row 219 included ([D8](../decisions.md#d8-find-the-spec-table-by-bisection-over-code-and-persist-only-the-specs-a-job-window-touches)). `Runtimes` holds the metadata. Steps 17 and 38 take `&VersionTable`.
  - `first_block` is the upgrade block, the first block whose state reports the spec. `spec_at(n)` gives `state` as the spec with the greatest `first_block <= n`, and `executed_by` as the state spec of `n - 1` ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)).
  - `runtime_version` gets a row only for a spec that a job window touches, the first time a job needs it, and its metadata is fetched once. That is 11 specs and about 5 MB in release 1 ([Storage](../design/04-storage.md#release-1)). Later starts load the stored rows into `Runtimes` and fetch nothing.
  - `learn_specs` returns one `NewRuntime` for each spec in the range whose metadata `Runtimes` does not hold yet, a spec past the table's last row included. `ingest` stores each one with `insert_runtime` and calls `VersionTable::add` for a spec the table lacks. A spec that goes down, or a `first_block` at or below the last row's, is an error.
  - `learn_specs` starts from the boundaries in the `VersionTable`. It bisects over `state_getStorageHash(":code")` only where a hash differs from that table, then confirms each boundary with `state_getRuntimeVersion` at `n - 1` and `n`. It never takes the first block a job read. Metadata comes from `Metadata_metadata_at_version` ([D7](../decisions.md#d7-fetch-metadata-with-metadata_metadata_at_version)), and one fetch takes 0.9 to 8.8 s per spec (`spikes/crates/chain-errors`, `metadata-fetch-time`).
  - A range with no new spec costs one `:code` hash call per range end. `ingest` makes these calls at start over every job's range, and the head tracker makes them on each new head. All 106 `:code` changes since dTAO were spec changes (`spikes/crates/chain-specs`, `P30`).
  - `ingest` runs `learn_specs` over every job's range before it spawns any task. It then shares the table as `Arc<std::sync::RwLock<VersionTable>>` between the head tracker and `SubxtSource`. The head tracker is the only writer, and it adds a new spec before it publishes the head. No guard is held across an `.await`. Step 28's `spawn_head` takes the shared table.
  - `SubxtSource::block_ref` makes one request, `chain_getBlockHash`, in place of step 1's four, and takes both specs from the table with `spec_at`. The per-block costs in [Ingestion](../design/03-ingestion.md) count one request for the block, 3 a block for `prices` (`spikes/crates/chain-decode`, `prices-cost`). Four requests a block would about double the 757,672 calls of the estimate for `prices` and `epochs`.
  - The chain returns `NewRuntime`, and `ingest` stores it, so `bt-chain` needs no dependency on `bt-store`. The insert uses `ON CONFLICT (spec_version) DO NOTHING`, on the primary key of [`runtime_version`](../design/04-storage.md#pipeline-state). A bare `ON CONFLICT DO NOTHING` would also swallow a second spec at an existing `first_block` (`spikes/crates/store`, `runtime-insert`).
- **Concepts.**
  - *`#[must_use]` effect values.* The chain crate cannot write the table, so it returns the write as a value, and the compiler warns when a caller drops it. Trap: a callback that someone forgets to wire, which fails in silence. Source: [The Rust Reference, "Diagnostic attributes", the `must_use` attribute](https://doc.rust-lang.org/reference/attributes/diagnostics.html#the-must_use-attribute).
- **Check.** `cargo test -p bt-chain sparse_reader` passes three tests:
  - `sparse_reader_stores_the_upgrade_block`. A reader of blocks 1,000 and 1,500 meets an upgrade at block 1,234 and stores `first_block` 1,234.
  - `sparse_reader_finds_490_missing_from_the_table`. It builds the table with `VersionTable::parse` from the real file with its 490 row removed, so only the bisection can find the spec. It reads every 100th block from 9,478,000 and stores spec 490 at 9,478,217, not 9,478,300. It then calls `VersionTable::add` and asserts that `spec_at(9,478,218)` gives `executed_by` 490, which checks that a new spec reaches the table (`spikes/crates/chain-specs`, `P5`).
  - Step 13's `sparse_reader_gets_true_first_block`, which the filter also runs.

  Time the first start of `ingest` and write the time in the commit body. After it, `psql "$BT_DATABASE__URL" -c 'select count(*), max(fetched_at) from runtime_version'` prints a count of 1, spec 490, because `prices` starts at the head in M1, plus any spec `learn_specs` found past the last row. The count grows as later steps give the jobs their windows: the specs the `prices` window touches after step 33 (488 and 490 at the measured head), and 11 after step 37 adds the 31-day `epochs` window. Restart `ingest`. The count and `max(fetched_at)` do not change, so the restart fetched no metadata.

## Step 25. `test(ingest): index the same blocks twice and get identical tables`

- **Goal.** Prove that the writes are idempotent. Run `prices` over a fixture range, rewind the checkpoint, run it again, and compare the tables. This is the first of the two M1 tests.
- **Files.** `crates/ingest/src/runner.rs`, `crates/ingest/tests/support/mod.rs`, `crates/ingest/tests/idempotent.rs`, three consecutive fixture blocks in the head era under `fixtures/state/`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  pub async fn run_range<J: Job, S: BlockSource>(job: J, chain: Chain<S>, store: &Store,
      range: RangeInclusive<BlockNumber>) -> Result<(), RunnerError> { todo!() }
  async fn test_db() -> Store { todo!() }                                            // test helper
  async fn dump_tables(store: &Store) -> BTreeMap<String, Vec<String>> { todo!() }  // test helper
  async fn rewind(store: &Store, worker: &str, to: BlockNumber) { todo!() }         // test helper
  ```

- **Who writes what.** The agent writes the test harness and records the fixtures with `fixture record --read prices`. You write `run_range` on top of step 11's loop, and the test. Review checks:
  - One `timescale/timescaledb:2.31.0-pg17` container per test binary, started with `-c timescaledb.max_background_workers=0`, as in step 9. The harness migrates one template database once, on one connection that no test reuses, because a failed migration keeps its advisory lock on its connection (`spikes/crates/store`, `migrate-lock`). `test_db` clones the template per test with `CREATE DATABASE .. TEMPLATE`, inside one mutex, because two clones of one template at once fail with 55006 ([D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests)).
  - The container lives in a static, and a static is not dropped when the test binary exits. So the container has a fixed name for this binary, and the harness removes a leftover one before it starts. `tests/support/mod.rs` opens with `#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]`, as step 9's does.
  - The dump orders rows by each table's natural key, and it leaves out `checkpoint.updated_at`, `checkpoint.blocks_per_sec`, and `runtime_version.fetched_at`, the only columns that a correct rerun changes.
  - Remove `ON CONFLICT DO NOTHING` once, watch the rerun fail with SQLSTATE 23505, and put it back. Say so in the commit body.
- **Concepts.**
  - *A database per test from a template.* Cloning a migrated template takes 28 ms, against 103 to 184 ms to create and migrate, and 2.84 s for a container per test (`spikes/crates/store`, `test-db-per-test`). Trap: one shared test database, where parallel tests see each other's rows and fail at random. Source: [PostgreSQL docs, "Template Databases"](https://www.postgresql.org/docs/current/manage-ag-templatedbs.html).
- **Check.** `cargo test -p bt-ingest same_blocks_twice` passes `same_blocks_twice_give_identical_tables`.

## Step 26. `feat(ingest): read blocks concurrently in order and hand rows to one writer`

- **Goal.** Replace the sequential loop of step 11 with the two stages of [Ingestion](../design/03-ingestion.md#read-in-order-write-in-one-place). Spawned reads run in `buffered(concurrency)`, a bounded channel carries the rows, and one writer commits them, one block per transaction for now. Delete the old loop.
- **Files.** `crates/ingest/src/{runner.rs,read.rs,writer.rs,error.rs,job.rs}`, `crates/chain/src/fixture.rs`, `crates/ingest/tests/read.rs`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  pub enum ToWriter<R> { Rows(BlockNumber, R), End }
  #[derive(Debug, thiserror::Error)]
  pub enum RunnerError {  // keeps step 10's #[from] arms, so `?` in steps 10 and 11 still compiles
      #[error(transparent)] Chain(#[from] ChainError),
      #[error(transparent)] Store(#[from] StoreError),
      #[error("the read stage closed its channel before End")] UpstreamClosed,
      #[error("the read of block {block} panicked")] ReadPanicked { block: BlockNumber, #[source] source: JoinError },
  }
  pub struct ReadPlan { pub next: BlockNumber, pub until: Option<BlockNumber>, pub concurrency: NonZeroUsize }
  pub fn spawn_read<J: Job, S: BlockSource>(job: Arc<J>, chain: Arc<Chain<S>>, n: BlockNumber)
      -> AbortOnDropHandle<Result<J::Rows, ChainError>> { todo!() }
  pub async fn read_stage<J: Job, S: BlockSource>(job: Arc<J>, chain: Arc<Chain<S>>, head: watch::Receiver<Option<Head>>,
      plan: ReadPlan, tx: mpsc::Sender<ToWriter<J::Rows>>, token: CancellationToken) -> Result<(), RunnerError> { todo!() }
  pub async fn write_stage<J: Job>(job: Arc<J>, store: Store,
      rx: mpsc::Receiver<ToWriter<J::Rows>>) -> Result<(), RunnerError> { todo!() }
  ```

  Later steps add parameters to `write_stage`: the batch settings in step 27, the token in step 28, the head and the retry policy in step 29, and the per-mode settings in step 33. Steps 29 and 33 show its signature at that point.
- **Who writes what.** You write all of it. The agent adds the test support that this step and steps 28, 29, and 36 use: `FixtureSource::with_latency`, `panic_at(block)`, synthetic blocks, and the `calls()` and `in_flight()` counters. The agent also adds the tokio-util feature `rt` for `AbortOnDropHandle`. If the diff passes 400 lines, commit `RunnerError` and `ToWriter` first and the stages second. Review checks:
  - Each item of `stream::iter(next..=target).filter(schedule)` is a `spawn_read` inside `buffered`, and a `JoinError` becomes `ReadPanicked` at its block. The writer sets `checkpoint.last_block` from the number in `Rows`, and its time from `J::block_ts` ([D18](../decisions.md#d18-give-every-job-one-trait-and-build-the-runner-from-a-spawned-read-stage-and-a-batching-writer)).
  - `End` goes out only at `until`, and a channel that closes before `End` is `UpstreamClosed`. `run_range` now runs the two stages with `until` set. The token is created here and cancelled from step 28 on.
- **Concepts.**
  - *`buffered` over spawned tasks.* Results come back in block order. Trap: `Promise.all(blocks.map(read))` starts everything at once and keeps running. A Rust stream starts nothing until polled, and a `buffered` stream that nobody polls freezes its futures halfway through their calls. The spawn lets them finish. Source: [futures docs, `StreamExt::buffered`](https://docs.rs/futures/latest/futures/stream/trait.StreamExt.html#method.buffered).
  - *`'static` tasks with `Arc` and `async move`.* `tokio::spawn(job.read(&chain, at))` fails with E0521, because the future borrows `&self` and `&Chain` and a task may outlive both. Move the `Arc` clones into an `async move` block that resolves the `BlockRef` with `chain.block_ref(n)` and then calls `read`. Trap: expecting a closure to keep `this` alive, as JavaScript does. Source: [tokio tutorial, "Spawning", the `'static` bound section](https://tokio.rs/tokio/tutorial/spawning).
  - *Abort on drop.* A dropped `JoinHandle` detaches its task, which keeps running (`spikes/crates/runner`, `detach`). `AbortOnDropHandle` aborts it. Trap: assuming a dropped handle cancels, as an aborted `fetch` does. Source: [tokio-util docs, `task::AbortOnDropHandle`](https://docs.rs/tokio-util/latest/tokio_util/task/struct.AbortOnDropHandle.html).
  - *Bounded `mpsc`.* `send().await` waits on a full channel, which is the backpressure. Trap: an array as a queue, which never pushes back. Source: [tokio docs, `sync::mpsc`](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html).
- **Check.** `cargo test -p bt-ingest read_` passes `read_keeps_block_order` (200 blocks at concurrency 8 with a fixture latency of 20 to 117 ms, in order and with at most 8 reads in flight) and `read_panic_is_an_error_at_its_block` (`ReadPanicked`, and the checkpoint stays before the block). `same_blocks_twice` still passes.

## Step 27. `feat(ingest): commit batches by block count or wait time`

- **Goal.** The writer flushes when the batch holds `batch_blocks` blocks or when `batch_max_wait` passes, whichever comes first, and commits each batch in one transaction, as [Ingestion](../design/03-ingestion.md#write-each-batch-in-one-transaction) describes.
- **Files.** `crates/ingest/src/writer.rs`, `crates/ingest/tests/batch.rs`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  pub struct BatchSettings { pub blocks: NonZeroUsize, pub max_wait: Duration }
  struct Batch<R> { first: Option<BlockNumber>, last: Option<BlockNumber>, rows: Vec<R>, deadline: Option<Instant> }  // tokio::time::Instant
  impl<R> Batch<R> {
      fn push(&mut self, n: BlockNumber, rows: R, s: &BatchSettings) -> bool { todo!() }        // true when full
      fn take(&mut self) -> Option<(BlockNumber, BlockNumber, Vec<R>)> { todo!() }              // (first, last, rows), None when empty
  }
  enum Event<R> { Deadline, Rows(BlockNumber, R), End, Closed }                          // Cancelled arrives in step 28
  async fn next_event<R>(rx: &mut mpsc::Receiver<ToWriter<R>>, deadline: Option<Instant>) -> Event<R> { todo!() }
  async fn commit_batch<J: Job>(job: &J, store: &Store, last: BlockNumber, batch: &[J::Rows]) -> Result<(), RunnerError> { todo!() }
  ```

- **Who writes what.** You write all of it. Review checks:
  - `next_event` holds the only `select!`. It is `biased;` with the deadline before `recv`, so a busy channel cannot hold off a commit (`spikes/crates/runner`, `writer-bias`).
  - The deadline branch is `sleep_until(deadline.unwrap_or_else(Instant::now))` behind `if deadline.is_some()`, because `select!` evaluates the expression of a disabled branch too, and `deadline.unwrap()` would panic there.
  - The commit is never a branch's future in `select!`. It runs after `next_event` returns, where no branch can drop it, because a commit inside a losing branch can return `Ok` with 0 rows written. `take` returning `None` means no transaction begins.
  - `Job::write` gets the whole batch as a slice, which is why `Rows` is `Sync` ([D18](../decisions.md#d18-give-every-job-one-trait-and-build-the-runner-from-a-spawned-read-stage-and-a-batching-writer)). The checkpoint takes `last` and `J::block_ts(batch.last())`. `first` is the batch's first block, which step 30's log line prints as `from=`.
  - `next_event` and `commit_batch` take over the receive and the commit inside step 26's writer. Step 28 adds the cancel to `Event` and `next_event`, and step 29 adds the retry to `commit_batch`.
- **Concepts.**
  - *`select!` cancel safety.* `recv()` may lose a race without harm. A half-run commit may not. Trap: reading `select!` as `Promise.race`. The losing branch is dropped mid-await, so only cancel-safe futures belong in it. Source: [tokio docs, `select!`, "Cancellation safety"](https://docs.rs/tokio/latest/tokio/macro.select.html#cancellation-safety).
  - *Paused time.* `#[tokio::test(start_paused = true)]` makes a 5-second deadline take no wall time. Trap: reading it as Jest fake timers. tokio auto-advances the clock whenever every task is idle, and a task waiting on a real socket counts as idle, so a timeout around real I/O fires at once (`spikes/crates/runner`, `paused-time`). So the deadline tests drive `next_event` and `Batch` alone. Source: [tokio docs, `time::pause`](https://docs.rs/tokio/latest/tokio/time/fn.pause.html).
- **Check.** `cargo test -p bt-ingest batch_` passes `batch_flushes_at_count`, `batch_flushes_at_deadline` (commits at 5.00 s paused), `batch_deadline_beats_a_busy_channel`, `batch_never_commits_empty`, and `batch_commits_rows_and_checkpoint_together`. Only the last touches the database, on real time.

## Step 28. `feat(ingest): stop every task on one token and exit with the right code`

- **Goal.** The shutdown path of [Ingestion](../design/03-ingestion.md#stop-cleanly-and-fail-loudly). One `CancellationToken` reaches every task, one `JoinSet` owns them, the writer commits what it holds, and the exit code is 0 on a signal and 1 on any failure.
- **Files.** `crates/ingest/src/{supervisor.rs,read.rs,writer.rs,head.rs,error.rs}`, `bin/indexer/src/main.rs`, `crates/ingest/tests/shutdown.rs`.
- **Shape.**

  ```rust
  pub struct RunnerDeps<S: BlockSource> { pub chain: Arc<Chain<S>>, pub store: Store, pub head: watch::Receiver<Option<Head>> }
  pub struct Supervisor { token: CancellationToken, tasks: JoinSet<Result<(), RunnerError>> }
  impl Supervisor {
      pub fn new(token: CancellationToken) -> Supervisor { todo!() }
      pub fn spawn_head<S: BlockSource>(&mut self, chain: Arc<Chain<S>>, store: Store, table: Arc<RwLock<VersionTable>>,
          tx: watch::Sender<Option<Head>>) { todo!() }  // the head tracker is the only writer of the shared VersionTable (step 24)
      pub fn spawn_job<J: Job, S: BlockSource>(&mut self, job: J, deps: RunnerDeps<S>, plan: ReadPlan, batch: BatchSettings,
          channel_capacity: NonZeroUsize) { todo!() }
      pub async fn run(self, shutdown_timeout: Duration) -> Result<(), RunnerError> { todo!() }  // Ok only on a signal
  }
  enum Event<R> { Cancelled, Deadline, Rows(BlockNumber, R), End, Closed }  // step 27's Event plus Cancelled
  async fn next_event<R>(rx: &mut mpsc::Receiver<ToWriter<R>>, deadline: Option<Instant>, token: &CancellationToken) -> Event<R> { todo!() }
  async fn shutdown_signal() { todo!() }  // SIGTERM or SIGINT, in bin/indexer
  // added to RunnerError: #[error("a task panicked")] TaskPanicked(#[source] JoinError)
  ```

- **Who writes what.** You write all of it. Review checks:
  - Both stages of each job and the head tracker live in the one `JoinSet`, so no stage runs under a bare `tokio::spawn`.
  - The head tracker polls `chain_getFinalizedHead` every 3 s ([D19](../decisions.md#d19-track-the-head-by-polling)).
  - The first `Err` or `JoinError` cancels the token. A `JoinError` from `join_next` becomes `RunnerError::TaskPanicked`. `run` then waits up to `shutdown_timeout` and calls `abort_all`. A signal shutdown that had to abort a task logs a warning with how many tasks it aborted and still returns `Ok`, so the exit code is 0 ([D18](../decisions.md#d18-give-every-job-one-trait-and-build-the-runner-from-a-spawned-read-stage-and-a-batching-writer)).
  - `next_event` is `biased;` in the order cancel, deadline, `recv`. Every other `.await` on a channel, the head `watch`, or a node call sits in `select!` with `token.cancelled()` or in `run_until_cancelled`. On an already cancelled token `run_until_cancelled` never polls the future. A dropped read stage aborts its spawned reads through `AbortOnDropHandle`, so none keeps running.
  - With a stalled writer, the rows a job holds stay at `batch_blocks + channel_capacity + concurrency` blocks: the batch, the full channel, and the finished reads (`spikes/crates/runner`, `backpressure`).
- **Concepts.**
  - *`CancellationToken`.* One token, cloned into every task, wakes every waiter at once. Trap: an `AbortController` that only some calls check. A loop that checks the token only between items stays blocked in `recv()`. Source: [tokio-util docs, `sync::CancellationToken`](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html).
  - *`JoinSet` and the two error layers.* `join_next` yields `Option<Result<Result<(), RunnerError>, JoinError>>`, the inner layer for the task's own error and the outer for a panic. Trap: Node crashes on an unhandled rejection for you. A tokio task that fails says nothing unless something joins it. Source: [tokio docs, `task::JoinSet`](https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html).
  - *`ExitCode`.* `main` maps the result to the code, so no library calls `process::exit` and skips the final commit. Trap: `process.exit(1)` deep in a handler. Source: [std docs, `process::ExitCode`](https://doc.rust-lang.org/std/process/struct.ExitCode.html).
- **Check.** `cargo test -p bt-ingest shutdown_` passes on real time: `shutdown_while_waiting` (returns within 1 second, held rows committed, and `in_flight()` at 0), `shutdown_failed_job_cancels_the_rest`, and `shutdown_stuck_task_is_aborted_at_timeout`. Run `ingest`, note the `prices` block in `status`, send `kill -TERM` to `ingest`, and `echo $?` prints `0`. Then `cargo run -q -p bt-indexer -- status` shows the `prices` checkpoint at or past the block you noted, so the writer committed what it held before it exited.

## Step 29. `feat(chain): cap the RPC rate, share permits, and retry with backoff`

- **Goal.** One token bucket caps node calls a second, one global `Semaphore` caps the calls in flight, and one `retry_with_backoff` retries every `Retry` error with full jitter, for node calls and for the writer's batch. All of it sits inside step 6's `Chain::call`, with no change to its signature ([D21](../decisions.md#d21-call-the-node-in-two-layers), [D23](../decisions.md#d23-cap-the-call-rate-with-a-one-token-bucket-not-only-the-calls-in-flight)). The second M1 test passes.
- **Files.** `crates/domain/src/disposition.rs`, `crates/chain/src/{retry.rs,rate.rs,source.rs,lib.rs,fixture.rs}`, `crates/chain/Cargo.toml`, `crates/ingest/src/{writer.rs,supervisor.rs}`, `crates/ingest/tests/crash.rs`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  pub trait HasDisposition { fn disposition(&self) -> Disposition; }  // for ChainError and StoreError, replaces the inherent disposition() of steps 6 and 9
  #[derive(Clone, Debug)]
  pub struct RetryPolicy { pub first: Duration, pub max: Duration, pub attempts: NonZeroU32 }
  pub struct TokenBucket { per_second: NonZeroU32, state: std::sync::Mutex<(f64, tokio::time::Instant)> }  // holds at most one token
  impl TokenBucket {
      pub fn new(per_second: NonZeroU32) -> TokenBucket { todo!() }
      pub async fn take(&self) { todo!() }
  }
  #[derive(Clone)]
  pub struct RpcLimits { pub rate: Arc<TokenBucket>, pub permits: Arc<Semaphore>, pub timeout: Duration, pub retry: RetryPolicy }
  pub async fn retry_with_backoff<T, E, F, Fut>(policy: &RetryPolicy, token: &CancellationToken, op: F) -> Result<T, E>
  where E: HasDisposition, F: FnMut() -> Fut, Fut: Future<Output = Result<T, E>> { todo!() }
  fn backoff(policy: &RetryPolicy, attempt: u32, rng: &mut impl Rng) -> Duration { todo!() }
  // crates/chain/src/lib.rs. Chain's rpc_timeout field becomes limits: RpcLimits, and it gains the process's CancellationToken.
  impl<S: BlockSource> Chain<S> {
      pub fn new(source: S, limits: RpcLimits, token: CancellationToken) -> Chain<S> { todo!() }
      async fn call<T, Fut>(&self, method: &'static str, block: BlockNumber, request: impl FnMut() -> Fut) -> Result<T, ChainError>
      where Fut: Future<Output = Result<T, ChainError>> { todo!() }  // step 6's signature
  }
  // crates/chain/src/source.rs. The error names the URL's index, never the URL.
  #[derive(Debug, thiserror::Error)]
  pub enum ConnectError {
      #[error("no answer from chain.urls[{index}] within {after:?}; a plain connect said: {plain}")]
      TimedOut { index: usize, after: Duration, plain: String },
      #[error("cannot connect to chain.urls[{index}]: {reason}")] Failed { index: usize, reason: String },
  }
  pub async fn connect(urls: &[String], index: usize, rpc_timeout: Duration) -> Result<RpcClient, ConnectError> { todo!() }
  impl SubxtSource { pub fn new(client: RpcClient) -> SubxtSource { todo!() } }  // with connect, replaces step 5's Chain::connect
  enum Committed { Done, Stopped }  // Stopped: a Retry error after the token was cancelled
  async fn commit_batch<J: Job>(job: &J, store: &Store, last: BlockNumber, batch: &[J::Rows],
      retry: &RetryPolicy, token: &CancellationToken) -> Result<Committed, RunnerError> { todo!() }  // step 27's, with the retry
  pub async fn write_stage<J: Job>(job: Arc<J>, store: Store, rx: mpsc::Receiver<ToWriter<J::Rows>>, head: watch::Receiver<Option<Head>>,
      batch: BatchSettings, retry: RetryPolicy, token: CancellationToken) -> Result<(), RunnerError> { todo!() }
  ```

- **Who writes what.** You write the retry, the backoff, the bucket, the permit, and the body of `Chain::call`. The agent writes `connect` and `FixtureSource::fail_at(block)`. The agent also adds `rand` to `crates/chain/Cargo.toml` and names subxt's non-default `reconnecting-rpc-client` feature in the manifest. In this step `write_stage` gains `head` and `retry` and takes the signature above. `spawn_job` gains a `retry: RetryPolicy` argument and passes it, with a clone of `deps.head`, to `write_stage`. The writer stages are the ones of `spikes/crates/runner` ([D18](../decisions.md#d18-give-every-job-one-trait-and-build-the-runner-from-a-spawned-read-stage-and-a-batching-writer)), and these limits ran under them on the real node (`spikes/crates/runner-live`, `wiring`).

  Review checks for `connect`:
  - `connect` builds the client with `ReconnectingRpcClient::builder().request_timeout(rpc_timeout + Duration::from_secs(30))` in place of step 1's `RpcClient::from_url`. Set it, because the client's own `request_timeout` defaults to 60 s, below the 90s of `chain.rpc_timeout`. With 30 s of headroom our `tokio::time::timeout` always fires first. A client timeout that fires anyway arrives as `OtherRpcClientError(Client(RpcError(RequestTimeout)))`, and step 6's `RequestTimeout` arm maps it to `Timeout` ([D22](../decisions.md#d22-classify-chain-errors-by-what-the-node-sends-and-bound-the-clients-waits-with-our-own-timeouts), `spikes/crates/chain-errors`, `client-request-timeout`, `reconnect-defaults`).
  - `connect` wraps the build in `tokio::time::timeout(rpc_timeout, builder.build(url))`, because the reconnecting client retries inside `build` forever (`subxt-rpcs-0.50.3/src/client/reconnecting_rpc_client/mod.rs:331-334`), and a wrong URL would hang the start with no error ([D22](../decisions.md#d22-classify-chain-errors-by-what-the-node-sends-and-bound-the-clients-waits-with-our-own-timeouts), `spikes/crates/chain-errors`, `connect-timeout`). `url` comes from `urls.get(index)`, and a missing index is `ConnectError::Failed`, because `urls[index]` trips clippy's `indexing_slicing`. With a dead first URL and a 3 s timeout, the start ends after 3 s with `no answer from chain.urls[0] within 3s` and no URL in the text (`spikes/crates/runner-live`, `dead-first-url`).
  - On that timeout, `connect` makes one plain `RpcClient::from_url` attempt and puts its error, with the key and host removed, in `plain`, because the plain client reports a rejected handshake at once while the reconnecting build stays pending. On the real gateway, a wrong key gives `Rejected { status_code: 403 }` and a wrong path gives 404 ([D22](../decisions.md#d22-classify-chain-errors-by-what-the-node-sends-and-bound-the-clients-waits-with-our-own-timeouts), `spikes/crates/chain-errors`, `gateway-handshake-rejected`). The plain attempt gets its own short timeout, because a listener that never answers would hang it too.
  - A URL that lost its `?apikey=` part still connects. The gateway then allows 10 calls a second and answers most calls with `-32029`, so every job retries `RateLimited` until `chain.rpc_max_retries` stops the process (`spikes/crates/chain-errors`, `gateway-keyless`).

  Review checks for the rate cap and the permits:
  - Each attempt takes one permit from `chain.rpc_max_in_flight` (default 16) first and one token from `chain.rpc_max_per_second` (default 120) last, then runs the call inside the 90s of `chain.rpc_timeout` ([D23](../decisions.md#d23-cap-the-call-rate-with-a-one-token-bucket-not-only-the-calls-in-flight)). The permit comes first, so that a call waiting for a permit holds no token and the calls started in any window stay at the cap (`spikes/crates/runner-live`, `token-permit-order`). Neither is held across the backoff sleep, and an aborted read gives its permit back. Only the sleep selects on the token, so an attempt is never dropped halfway.
  - `rpc_timeout` covers the call only. The wait for a permit and a token is outside it and has no limit, and step 30's `bt_rpc_permit_wait_seconds` shows it. So a cap far below demand makes calls wait and does not turn them into timeouts (`spikes/crates/runner-live`, `timeout-scope`).
  - The bucket holds at most one token, so calls are at least `1 / rpc_max_per_second` apart. On the node, any one second then holds at most the cap plus one call (`spikes/crates/runner-live`, `cap-window`). A bucket that holds a second of tokens and starts full lets a burst past the cap (`spikes/crates/runner-live`, `bucket-burst`), and the check below asserts that such a bucket fails the window test. A caller that finds no token takes the next free slot under the lock and sleeps until it, so callers are served in order. The lock is a `std::sync::Mutex`, never held across an `.await`. The state uses `tokio::time::Instant`, so the paused-time tests move it.
  - The bucket counts `BlockSource` calls, not node requests. `finalized_head` is one call and two requests (`spikes/crates/runner-live`, `head-two-requests`).

  Review checks for the retry:
  - Create `rand::rng()` inside the `backoff` call and never hold it across the sleep. `ThreadRng` is not `Send`, and holding it across the sleep breaks every spawned read with "future cannot be sent between threads safely" (`spikes/crates/runner-live`, `rng-send`).
  - `RateLimited` and `Disconnected` are `Retry`. `RateLimited` covers `-32029` and `-32004`, as in step 6. The node's codes 4001 to 4003 for a bad read stay `Unknown` and stop the process, as in step 6 ([D22](../decisions.md#d22-classify-chain-errors-by-what-the-node-sends-and-bound-the-clients-waits-with-our-own-timeouts)). The store's Retry list is step 9's full list: `PoolTimedOut`, `Io`, and SQLSTATE 40001, 40P01, 57014, 57P01, 57P02, and 57P03 ([D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests)).
  - The reconnecting client retries one URL forever and never fails over. A call made during an outage waits until `rpc_timeout`, ends as `Timeout`, and retries, so a run through an outage finishes with no gap (`spikes/crates/runner-live`, `outage`). The wait starts at 200 ms and doubles to 30 s, as in [Retry node requests](../design/03-ingestion.md#retry-node-requests). `retry_with_backoff` is the only code that loops on errors.
  - The writer retries the whole batch in a new transaction, since `Job::write` borrows the batch. On a cancel during the backoff, `retry_with_backoff` still returns the last error. `commit_batch` turns a `Retry` error after a cancelled token into `Ok(Committed::Stopped)`, and any other error stays an error. So the writer gives up the batch and stops cleanly, and the process exits 0 (`spikes/crates/runner`, `cancel-in-commit-backoff`). That is safe because writes are idempotent.
- **Concepts.**
  - *Permits do not bound the rate.* In a run with 64 permits at 90 to 140 ms a call, the permits ask for 450 to 700 calls a second against the key's 200 (`spikes/crates/chain-errors`, `sustain-http-historic`). The bucket bounds calls a second, the permits bound memory and sockets. Trap: `p-limit` as a rate limiter. Source: [tokio docs, `sync::Semaphore`](https://docs.rs/tokio/latest/tokio/sync/struct.Semaphore.html).
  - *Closures that return futures.* A future runs once, so a retry takes a closure that builds a new future for each attempt. That is why step 6 gave `Chain::call` a closure. Trap: passing `call(at)` where `|| call(at)` belongs. Source: [The Rust Book, "Closures"](https://doc.rust-lang.org/book/ch13-01-closures.html).
- **Check.** Four commands pass.
  - `cargo test -p bt-chain retry_` passes on paused time with no database: `retry_stops_after_its_attempts` (the attempt count and the last error), `retry_one_token_bucket_keeps_every_window_at_the_cap` (no 1 s window holds more than 120 calls, and a bucket that holds a second of tokens fails it), `retry_permits_cap_the_calls_in_flight`, `retry_wait_for_a_token_is_outside_rpc_timeout`, and `retry_hung_call_times_out_and_returns_its_permit` (`Timeout` after its attempts, and every permit comes back).
  - `cargo test -p bt-chain connect_times_out` passes `connect_times_out_names_the_url_index`. Against a local listener that accepts TCP and never answers the WebSocket handshake, `connect` returns after its short test timeout with an error that names `chain.urls[0]` and holds no URL.
  - `cargo test -p bt-ingest cancel_during_commit_backoff` passes on real time. Every commit fails with a `Retry` error, the token is cancelled during the backoff, and `Supervisor::run` returns `Ok`, so the exit code is 0.
  - `cargo test -p bt-ingest crash_mid_batch` passes on real time with a 1 ms first wait. The run fails at block 300 of 500 after every retry, a new run finishes, and the tables match a clean run.

## Step 30. `feat(telemetry): log every commit and export progress metrics`

- **Goal.** The progress log line, the spans of [Observability](../design/07-observability.md#name-spans-after-the-work), the optional OpenTelemetry export, the progress gauges, and the node call metrics on `/metrics`.
- **Files.** `crates/telemetry/{Cargo.toml,src/lib.rs}`, calls in `crates/ingest/src/writer.rs` and `crates/chain/src/{lib.rs,rate.rs}`, `bin/indexer/src/main.rs`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Brief for the agent.** `bt_telemetry::init(&TelemetryConfig) -> Result<Guard, TelemetryError>` installs the `pretty` or `json` subscriber, a `tracing-opentelemetry` layer when `telemetry.otlp_endpoint` is set ([Observability](../design/07-observability.md#set-up-tracing-once)), and the Prometheus exporter on `telemetry.metrics_addr`. Write `pub struct TelemetryConfig { pub log_format: LogFormat, pub otlp_endpoint: Option<String>, pub metrics_addr: SocketAddr }` with `pub enum LogFormat { Pretty, Json }` and a `Default` that holds the defaults of [Ingestion](../design/03-ingestion.md#configuration). `Guard` holds the OpenTelemetry provider and flushes it on drop. `TelemetryError` is a `thiserror` enum with one variant each for the subscriber, the exporter, and the OTLP setup. The `metrics_addr` default is `0.0.0.0:9100`, which `ingest` uses, and `serve` overrides the default to `0.0.0.0:9101`, so both run on one host.

  `main` builds one `Chain` per job with `chain.for_worker(J::NAME)` and one for the head tracker with `chain.for_worker("head")`, so `Chain::call` can fill the `worker` label. The values share the source, the `Runtimes`, and the `RpcLimits` behind `Arc`. Add `pub fn for_worker(&self, worker: &'static str) -> Chain<S>`, and hold the source as `Arc<S>` for it (`spikes/crates/runner-live`, `two-jobs`). `Chain::call` counts each `Disconnected` result in `bt_rpc_disconnects_total` ([Error handling](../design/06-error-handling.md#classify-subxterror-when-it-enters-the-crate)), and `bt_rpc_permit_wait_seconds{worker}` times the whole wait in front of a call, for the permit and the rate token together ([Observability](../design/07-observability.md#node-and-database)).

  At each commit the writer logs `worker= mode= from= to= head= lag= rate= eta=` and sets the gauges of [Progress](../design/07-observability.md#progress), with `lag` and `eta` from step 12's `progress()`. `from=` and `to=` are the batch's `first` and `last` from step 27. `head=` is the newest head in the writer's `watch` from step 29. Until step 33, `mode` is `live`, `rate` is 0, `eta` is empty, `target_block` is the head the writer reads from its `watch` at the commit, and `CheckpointUpdate.blocks_per_sec` is 0. The `batch` and `write_batch` spans are `info`, and `read` and `rpc` are `debug`.
- **Who writes what.** The agent writes it. You review it. Review checks:
  - No library crate installs a subscriber.
  - No label holds a block number, a hash, or an account.
  - `bt_last_commit_timestamp_seconds` starts from the checkpoint's `updated_at`, not from the start time.
  - Lag counts from `newest_scheduled(schedule, head)`, so an `Every(100)` job reads 0 between its blocks ([D20](../decisions.md#d20-measure-lag-and-readiness-against-the-schedule)).
  - A test with `chain.rpc_max_per_second` at 1 makes two calls and sees `bt_rpc_permit_wait_seconds` record a wait near 1 s for the second, so the histogram counts the wait for a rate token and not only for a permit (`spikes/crates/runner-live`, `wait-metric`).
  - The exporter serves its own port. `metrics-exporter-prometheus` 0.18.3 with default features off renders in process only, so the manifest names the feature that adds its HTTP listener.
- **Concepts.**
  - *`#[instrument]` on async functions.* `#[instrument(skip_all, fields(block = %at.number))]` attaches the span to the future, so every log line inside carries the block. Trap: holding `span.enter()` across an `.await`, which tags other tasks' logs with your span. Source: [tracing docs, `attr.instrument`](https://docs.rs/tracing/latest/tracing/attr.instrument.html).
- **Check.** `curl -s localhost:9100/metrics | grep 'bt_lag_blocks{worker="prices"}'` shows a value from 0 to 2, and `ingest` logs one `worker=prices` line per commit. `curl -s localhost:9100/metrics | grep -c '^bt_rpc_permit_wait_seconds'` prints a number above 0.

## Step 31. `feat(api): serve GET /status from the checkpoint tables`

- **Goal.** `bt-indexer serve` answers `GET /status` with the JSON of [API](../design/05-api.md#get-status), computed by the same `progress()`, and `GET /healthz` with 200 while the process runs ([readiness](../design/05-api.md#report-readiness-per-job-window)). Both live in the ops router, which steps 43 and 50 build on. M1 includes the `api` crate with this one data route ([D3](../decisions.md#d3-set-the-milestone-scope-by-what-release-1-reads)). The step then closes M1 with an hour of live indexing.
- **Files.** `crates/api/{Cargo.toml,src/lib.rs,src/state.rs,src/error.rs,src/ops/mod.rs,src/ops/status.rs,src/ops/health.rs}`, `crates/api/tests/status.rs`, subcommand `serve`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  pub struct ApiConfig { pub listen_addr: SocketAddr, pub db_max_connections: u32, pub max_concurrent_requests: usize, pub max_lag_blocks: u32 }
  impl Default for ApiConfig { fn default() -> Self { todo!() } }  // listen_addr 0.0.0.0:8080, then 20, 64, 50
  #[derive(Clone)]
  pub struct AppState { db: PgPool, config: Arc<ApiConfig> }
  impl AppState { pub fn new(db: PgPool, config: Arc<ApiConfig>) -> AppState { todo!() } }  // serve in bin/indexer builds it
  pub fn router(state: AppState) -> Router { todo!() }
  pub fn routes() -> Router<AppState> { todo!() }  // in ops: /status and /healthz, /readyz joins in step 50
  async fn status(State(s): State<AppState>) -> Result<Json<StatusDto>, ApiError> { todo!() }
  async fn healthz() -> StatusCode { todo!() }
  #[derive(Debug, thiserror::Error)]
  pub enum ApiError {  // grows in step 43
      #[error("database unavailable")] Unavailable(#[source] StoreError),
      #[error("internal error")] Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
  }
  impl IntoResponse for ApiError { fn into_response(self) -> Response { todo!() } }  // minimal: 503 or 500 with the message; step 43 replaces it
  ```

- **Who writes what.** You write `AppState`, `router`, `ops::routes()`, and both handlers. The agent writes `StatusDto` and an insta test through `oneshot`. `StatusDto` holds `chain` with `finalized_block` and `finalized_time`, and `workers`, one entry per worker with the twelve keys of the JSON in [API](../design/05-api.md#get-status), in that order. Review checks:
  - Write the minimal `impl IntoResponse for ApiError` with the handlers, because without it registering `status` fails with E0277, `Handler<_, _>` is not satisfied. It answers 503 for `Unavailable` and 500 for `Internal`, and step 43 replaces it with problem+json.
  - `serve` binds `api.listen_addr`, default `0.0.0.0:8080` ([D30](../decisions.md#d30-read-the-node-urls-the-database-url-and-the-api-address-from-config)). It connects with the same database URL setting as `ingest`, `BT_DATABASE__URL`, which step 32 makes the key `database.url`. The deployment of step 57 gives `serve` the read-only API role's URL.
  - `bt-api` has no dependency on `bt-chain`.
  - `healthz` touches neither the database nor the node, so it answers 200 while the process runs, whatever the jobs do.
  - Each worker's schedule comes from the one worker-to-`Schedule` table in `bt-domain`, which `bt-indexer status` also uses ([D20](../decisions.md#d20-measure-lag-and-readiness-against-the-schedule)). Percent done divides by `newest_scheduled(schedule, head) - start_block` and is 100 when that is not positive.
  - `eta_seconds` is `null`, not absent, in live mode. `ready` and `not_ready` arrive in step 50.
  - `serve` calls `bt_telemetry::init` with `telemetry.metrics_addr` overridden to `0.0.0.0:9101`, so its `/metrics` listener does not collide with `ingest` on 9100 ([Quality](../design/09-quality.md#run-everything-with-docker-compose)).
- **Concepts.**
  - *axum extractors and `State`.* A handler's arguments declare what it reads, and the compiler checks them. Trap: Express's `(req, res)`, where `req` is a bag of everything. A missing bound gives an unreadable trait error, so add `#[axum::debug_handler]` while you write. Source: [axum docs, `extract`](https://docs.rs/axum/latest/axum/extract/index.html).
- **Check (M1).** With `ingest` and `serve` running, `curl -s localhost:8080/status | jq -c '.workers[0] | {worker, mode}'` prints `{"worker":"prices","mode":"live"}`, and `curl -s -o /dev/null -w '%{http_code}' localhost:8080/healthz` prints `200`. `curl -s -o /dev/null -w '%{http_code}' localhost:9101/metrics` prints `200` while `ingest` serves 9100. `cargo test -p bt-api status` passes `status_lists_every_worker`. Then run the M1 check. Leave `ingest` running for one hour and sample `status` once a minute with `for i in $(seq 60); do cargo run -q -p bt-indexer -- status | grep '^prices'; sleep 60; done`. All 60 lines show `prices live` with `LAG` below 5. The node can slow `prices` to about 2 blocks a second for minutes at a time (`spikes/crates/runner-live`, `node-slow-windows`), which is still far above the one block every 12 s that live needs. Write the hour's start and end blocks into the commit body.

## Stop here

- **Demo.** Run `ingest`, `serve`, and `status --watch` together. Send `kill -TERM` to `ingest`, restart it, and show that `prices` continues from its checkpoint with no gap. Show `curl localhost:8080/status`, `curl localhost:9100/metrics` from `ingest`, and `curl localhost:9101/metrics` from `serve`.
- **Explain without notes.**
  - Why does `tokio::spawn(job.read(&chain, at))` not compile, and what change makes it compile?
  - Why is each read a spawned task held in an `AbortOnDropHandle`, and what goes wrong with the permits without each?
  - Why must the commit never be the future of a `select!` branch, and why does the deadline come before `recv`?
  - How does cancel wake a writer that is blocked in `recv()`, and what does it commit before it exits?
  - Why does a channel that closes before `End` count as a failure and not as the end of the range?
  - Why do permits alone not keep the node under its rate limit?
- **Roadmap.** Closes M1. Step 31's hour-long run shows `prices` live with lag below 5 for 1 hour, and `same_blocks_twice` (step 25) and `crash_mid_batch` (step 29) pass.
