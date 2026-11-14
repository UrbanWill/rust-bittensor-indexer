# Stage 5. The other four jobs

This stage gives every job its history window and its backfill and live settings, then adds `validators`, `epochs`, `baskets`, and `identities` on the runner of stage 4. It closes roadmap M2, which needs a measured backfill from an empty database. Before you start, M1 must pass.

Release 1's backfill at the measured head is about 757,672 node calls and about 6.8 GB: `prices` 64,800, `epochs` 691,200, `baskets` 1,383, `validators` 286, `identities` 3. From each job's rate at 8 in flight, measured alone on the keyed node on 2026-11-12, the time lies between 1 h 30 min and 5 h 35 min. The low end is the cap of 120 calls a second, which alone sets a floor of about 1 h 30 min to 1 h 45 min, for 2.53 to 3.0 `epochs` calls a block ([D23](../decisions.md#d23-cap-the-call-rate-with-a-one-token-bucket-not-only-the-calls-in-flight)). The range assumes the node's fast rate. The node sometimes serves `prices` at about 2 blocks a second for minutes at a time, with no error, against its usual 11.29 to 12.97 blocks a second (`spikes/crates/runner-live`, `node-slow-windows`). Such slow windows lengthen the backfill beyond the range. Step 42 runs the first full backfill and measures the combined time of all five jobs on one runner.

## Step 32. `feat(config): load configuration from defaults, files, and environment`

- **Goal.** One typed `Config` replaces the CLI flags and the environment reads, as [Quality](../design/09-quality.md#configuration) describes.
- **Files.** `bin/indexer/src/config.rs`, `config/default.toml`, `bin/indexer/src/main.rs`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Brief for the agent.** figment layers the compiled defaults, `config/default.toml`, the file in `BT_CONFIG`, and `BT_` variables with `__` between levels. Fields are typed (`Duration` through humantime, `NonZeroU32`). The database URL and each entry of `chain.urls` are `SecretString` values, parsed as a `Url` once to validate, because the node URL carries the key. A `Url` prints `apikey=` in its `Debug` output, so a `Config` with `chain.urls` typed as `Url` would leak the key. Validation runs before any task starts, and each crate receives only its own section ([D30](../decisions.md#d30-read-the-node-urls-the-database-url-and-the-api-address-from-config)).

  `chain.urls` accepts one bare URL string or a list, through a custom deserializer, and a string is split on commas. Without it `BT_CHAIN__URLS=wss://...` fails to load with `invalid type: found string ..., expected a sequence for key "CHAIN.URLS"`, and every start after this step fails. `connect` takes the URLs as strings, so `main` passes each one with `expose_secret()`. `database.url`, set by `BT_DATABASE__URL`, is a key. `migrate`, `ingest`, and `serve` all read it, and the deployment gives `serve` the read-only API role's URL and `migrate` and `ingest` the writer role's URL. `load() -> Result<Config, Box<figment::Error>>` boxes the error, because `figment::Error` is 208 bytes and trips `clippy::result_large_err`. The test module carries `#[allow(clippy::result_large_err)]`, because the closure `Jail::expect_with` takes returns `figment::Error` by value. Tests set variables with `figment::Jail` and never call `std::env::set_var`, which is `unsafe` in edition 2024 and so forbidden by `unsafe_code = "forbid"`.
- **Who writes what.** The agent writes it. You review it. Review checks:
  - Every key in [Ingestion](../design/03-ingestion.md#configuration) exists with its default. These come from the rate measurements of [D23](../decisions.md#d23-cap-the-call-rate-with-a-one-token-bucket-not-only-the-calls-in-flight): `chain.rpc_max_per_second = 120`, `chain.rpc_max_in_flight = 16`, `chain.rpc_timeout = "90s"`, and a backfill `concurrency` of 8 for each job.
  - The `database.*`, `api.*`, and `telemetry.*` keys exist too: `database.url`, `api.listen_addr = "0.0.0.0:8080"`, `api.db_max_connections = 20`, `api.max_concurrent_requests = 64`, `api.max_lag_blocks = 50`, `telemetry.log_format`, `telemetry.otlp_endpoint`, and `telemetry.metrics_addr = "0.0.0.0:9100"`, whose default `serve` overrides to `0.0.0.0:9101` as in step 31. A `telemetry.metrics_addr` set in a file or the environment wins over both. The API request timeout is the 10 s constant of step 43.
  - The compiled defaults are the `Default` impls of `RpcLimits`, `TelemetryConfig`, and `ApiConfig` from steps 29 to 31, so each default is written once.
  - No crate below the binary reads the environment.
  - `Debug` output of `Config` shows no secret.
- **Concepts.**
  - *Layered config into one typed struct.* A bad value fails at start with the key's name, not deep in a job an hour later. Trap: `process.env` reads scattered through the code. Source: [figment docs](https://docs.rs/figment/latest/figment/).
  - *`SecretString`.* The wrapper's `Debug` prints a placeholder, so a logged `Config` leaks nothing. Source: [secrecy docs](https://docs.rs/secrecy/latest/secrecy/).
- **Check.** `env -u BT_CHAIN__URLS cargo run -q -p bt-indexer -- ingest; echo $?` prints figment's error, which reads missing field `urls` for key "CHAIN", then `1`. `cargo test -p bt-indexer config_` passes `config_bare_url_loads` (`BT_CHAIN__URLS=wss://example.invalid:443` gives one URL), `config_url_list_loads` (two URLs split on a comma), and `config_debug_shows_no_secret`, which sets a node URL with `?apikey=` in it and asserts the key is absent from the `Debug` output.

## Step 33. `feat(ingest): start jobs at their history window and switch between backfill and live`

- **Goal.** A fresh `prices` starts at the head minus 2 days and 1 day of margin, backfills with backfill settings, switches to live, and logs the switch once. M1 started it at the head, and M2 adds the window ([D3](../decisions.md#d3-set-the-milestone-scope-by-what-release-1-reads)). Drop the M1 database first, because `start_block` never moves.
- **Files.** `crates/domain/src/plan.rs`, `crates/ingest/src/{runner.rs,writer.rs}`, `crates/store/src/checkpoint.rs`.
- **Shape.**

  ```rust
  pub fn start_block(h: History, s: Schedule, head: BlockNumber, spec_floor: Option<BlockNumber>) -> BlockNumber { todo!() }
  pub fn next_range(cp: &Checkpoint, h: History, s: Schedule, head: BlockNumber) -> Option<RangeInclusive<BlockNumber>> { todo!() }
  impl Mode { pub fn next(self, schedule: Schedule, last_block: BlockNumber, head: BlockNumber, live_threshold: u32) -> Mode { todo!() } }
  pub struct Ewma { per_sec: f32, window: Duration }
  impl Ewma { pub fn record(&mut self, blocks: u32, elapsed: Duration) -> f32 { todo!() } }
  pub struct ModeSettings { pub concurrency: NonZeroUsize, pub batch: BatchSettings }
  pub struct JobModes { pub backfill: ModeSettings, pub live: ModeSettings, pub live_threshold: u32 }
  pub async fn write_stage<J: Job>(job: Arc<J>, store: Store, rx: mpsc::Receiver<ToWriter<J::Rows>>, head: watch::Receiver<Option<Head>>,
      modes: JobModes, retry: RetryPolicy, token: CancellationToken) -> Result<(), RunnerError> { todo!() }  // modes in place of batch: BatchSettings; spawn_job takes modes: JobModes in place of batch too
  ```

- **Who writes what.** You write all of it. Review checks:
  - `start_block` is written once, on the first run, and never moves on a restart. It never goes below block 4,920,350.
  - A new checkpoint row holds `last_block_ts = to_timestamp(0)`, `target_block = start_block - 1`, and `blocks_per_sec = 0`, inserted with `ON CONFLICT (worker) DO NOTHING` ([D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests)).
  - `Latest` jumps to the newest scheduled block. `Mode::next` returns `Backfill` while `last_block < newest_scheduled(schedule, head − live_threshold)`, with a saturating subtraction, and `Live` otherwise, as [Ingestion](../design/03-ingestion.md#switch-each-job-between-backfill-and-live-settings) says ([D20](../decisions.md#d20-measure-lag-and-readiness-against-the-schedule)). An `Every(100)` job whose lag to the head jumps to 100 at each new scheduled block stays `Live`, so `validators`, `identities`, and `baskets` do not switch settings on every cycle. Tests reproduce the `epochs` and `baskets` numbers of [The command line](../design/03-ingestion.md#the-command-line).
  - The read stage picks `concurrency` when it builds each range, and the writer picks `BatchSettings` from `JobModes` for each batch, so a mode switch takes effect at the next range.
  - The writer now fills step 30's log line and `CheckpointUpdate` with computed values: `mode` from `Mode::next`, `rate` and `blocks_per_sec` from `Ewma::record`, and `eta` from `progress()`. `target_block` is still the head the writer reads from its `watch` at the commit.
  - `Mode` maps to the strings of the `CHECK` on `checkpoint.mode`, `'backfill'` and `'live'`, in one `as_str` and one `FromStr`.
  - If the diff passes 400 lines, commit the start rule first and the mode switch second.
- **Concepts.**
  - *Policy as enums and pure functions.* `start_block`, `next_range`, and `Mode::next` are pure, so each rule is a three-line test with no runner. Trap: flags like `isLive` set from three places in the loop. Source: [The Rust Book, "Enums and Pattern Matching"](https://doc.rust-lang.org/book/ch06-00-enums.html).
- **Check.** From an empty database, `cargo run -q -p bt-indexer -- status --watch` shows `prices backfill` with a rising percent, then `prices live`. The 21,600 blocks take about 28 to 32 minutes at 11.29 to 12.97 blocks a second, the rate of `prices` alone at 8 in flight on the keyed node. The log holds one `mode=live` line. `psql "$BT_DATABASE__URL" -c "select start_block from checkpoint where worker = 'prices'"` prints the first run's head minus 21,600, for example 9,469,262 at the head 9,490,862 measured on 2026-11-12.

## Step 34. `feat(domain): compute global weighted stake with checked u128 math`

- **Goal.** The squid's `global_weighted_stake`, `global_nominators`, and `active_subnets` from `ValidatorInputs` (step 21), as [Analytics](../design/08-analytics.md#validators) defines them.
- **Files.** `crates/domain/src/validators.rs`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  pub struct ValidatorRow { pub hotkey: Hotkey, pub owner: Option<Coldkey>, pub name: Option<String>, pub take: u16,
      pub global_weighted_stake: u128, pub global_nominators: u32, pub active_subnets: u32 }
  pub struct ValidatorSubnetRow { pub hotkey: Hotkey, pub netuid: NetUid, pub alpha_stake: u64 }  // one validator_subnet row; the job adds snapshot_block
  pub struct ValidatorSnapshot { pub rows: Vec<ValidatorRow>, pub subnets: Vec<ValidatorSubnetRow> }
  pub enum MathError { Overflow { what: &'static str } }
  pub fn validator_snapshot(i: &ValidatorInputs) -> Result<ValidatorSnapshot, MathError> { todo!() }
  fn weighted_term(alpha: u64, netuid: NetUid, moving_price_bits: Option<i128>, tao_weight: u64) -> Result<u128, MathError> { todo!() }
  ```

- **Who writes what.** You write all of it. Review checks:
  - Every `+` and `*` is `checked_*`, and each term is floored on its own. The `#![deny(clippy::arithmetic_side_effects)]` that step 3 put at the top of `crates/domain/src/lib.rs` enforces it. A crate with `[lints] workspace = true` cannot add the lint in its `Cargo.toml`.
  - Netuid 0 divides by 2^64 - 1, and a netuid with no `SubnetMovingPrice` adds 0. `moving_price_bits` is the `i128` of `FixedI128 { bits }` and converts with `u128::try_from`, never `as`.
  - `tao_weight` is the `u64` of `TaoWeight`. Each coldkey counts once in `global_nominators`. Expected values come from the squid code that the analytics doc cites.
- **Concepts.**
  - *Checked arithmetic under a deny lint.* The lint rejects a bare `+` in `bt-domain`, so every overflow becomes an `Err` that the caller must handle. Trap: JavaScript `Number` loses precision past 2^53 in silence. Source: [std docs, `u128::checked_mul`](https://doc.rust-lang.org/std/primitive.u128.html#method.checked_mul).
  - *Property tests.* proptest generates inputs and shrinks a failure to the smallest case. Trap: `BigInt` never overflows, so the squid never tested for it. Here "returns `Err` and never wraps" is the property. Source: [the proptest book](https://proptest-rs.github.io/proptest/).
- **Check.** `cargo test -p bt-domain weighted_stake` passes `weighted_stake_matches_a_hand_computed_case` and the property test `weighted_stake_never_wraps`.

## Step 35. `feat(ingest): index the validator snapshot`

- **Goal.** `validators` runs at blocks where `n % 100 == 0`, with `Latest` history, and replaces `validator` and `validator_subnet` in one transaction.
- **Files.** `crates/ingest/src/jobs/validators.rs`, `crates/domain/src/rows.rs` (`Validators`), `crates/store/migrations/0003_validators.sql`, `crates/store/src/validators.rs`.
- **Shape.**

  ```rust
  pub struct ValidatorsJob;  // NAME = "validators", Schedule::Every(100), History::Latest
  pub struct Validators { pub state: BlockState, pub snapshot: ValidatorSnapshot }  // crates/domain/src/rows.rs, because bt-store names it (D33)
  // `ValidatorsJob::block_ts` returns `rows.state.ts`.
  pub async fn replace_validators(tx: &mut PgTransaction<'_>, s: &ValidatorSnapshot) -> Result<(), StoreError> { todo!() }
  ```

- **Who writes what.** You write the job. The agent writes the DDL from [Storage](../design/04-storage.md#validators), the `UNNEST` writers, and their round-trip tests. Review checks:
  - The delete and the insert share the checkpoint's transaction. `write` replaces the set with the batch's last snapshot only.
  - A row exists only for a key of `Delegates` ([Analytics](../design/08-analytics.md#validators)).
  - A declared `TaoWeight` with no stored key reads its metadata default, 971,718,665,099,567,868, not 0. A test serves `TaoWeight` with no stored value through `FixtureSource` and asserts that value. The test builds the case itself, because no real block has a declared `TaoWeight` with no stored value. `TaoWeight` is first stored at 4,920,351, and spec 219, which executed 4,920,350, does not declare it (`spikes/crates/chain-decode`, `taoweight-null`). An item the executing spec does not declare is absent, never defaulted ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)).
  - [Check the analytics against the chain](../design/09-quality.md#check-the-analytics-against-the-chain): the agent adds `--stake` to step 21's `chain validators`, which prints `hotkey global_weighted_stake` for the 20 largest validators at a block, recomputed from storage through `validator_snapshot`. At the block of the committed snapshot, each of the 20 matches its `validator` row to the rao. Put the block and the 20 hotkeys in the commit body.
  - `u128` goes through `BigDecimal::from` into `NUMERIC(39,0)`, and a failed conversion is `StoreError::Overflow`. The `UNNEST` builder asserts equal array lengths ([D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests)). The nullable columns `owner_coldkey` and `name` need the `as _` override in `query!`, as in [Storage](../design/04-storage.md), or the macro fails with E0308, `expected &[Vec<u8>], found &[Option<Vec<u8>>]`.
- **Concepts.** None new.
- **Check.** `cargo run -q -p bt-indexer -- status` shows `validators live` at a multiple of 100 with lag 0. The log shows about 286 calls and the snapshot's time at the live setting of 4 in flight, which you record. At 8 in flight a snapshot takes 131 s. `validator` holds about 2,738 rows, the count at block 9,490,000 (`spikes/crates/chain-decode`, `decode-table`). Put the logged call count and time in the commit body.

## Step 36. `test(ingest): prove a stalled writer does not starve another job`

- **Goal.** The backpressure test of [Quality](../design/09-quality.md#prove-that-a-stalled-writer-does-not-starve-other-jobs), with two small test jobs that share the global permits. It runs on real time, because `commit_batch` calls `store.begin()` on the real store before A's `write` reaches the gate, and paused time auto-advances during real I/O.
- **Files.** `crates/ingest/tests/backpressure.rs`.
- **Shape.**

  ```rust
  struct JobA { gate: Arc<Notify>, entered: Arc<Notify> }  // write signals `entered`, then waits on `gate` forever
  struct JobB;                                             // one storage read per block
  #[tokio::test]
  async fn stalled_writer_does_not_starve_other_job() { todo!() }
  ```

- **Who writes what.** You write the test. The agent writes its setup from the synthetic blocks and the `calls()` and `in_flight()` counters that step 26 added to `FixtureSource`. Each job gets its own `FixtureSource`, and both `Chain`s share one `RpcLimits`, so one `Arc<Semaphore>` and one bucket. Review checks:
  - Start the measured window once A's read stage stops sending, or run A with a `channel_capacity` of 8. A window that starts while A still fills a channel of 256 measures B during that fill, and the rate check then passes with almost no margin (`spikes/crates/runner`, `two-jobs`).
  - The rate assertion is the primary check: B's call rate against its rate alone, at least 80%. Set the test's `rpc_max_per_second` above B's rate alone, or the cap hides lost permits and the rate check passes with broken reads. In a run on the node with 8 permits, unspawned reads hold 3 of them while the other job still runs at 9.79 blocks a second, against 10.51 alone (`spikes/crates/runner-live`, `stalled-neighbour`).
  - The test also asserts that A holds no permit: `permits.available_permits()` equals the total once B is idle. The free permit count shows a lost permit where the rate does not. A's `in_flight()` reaching 0 is a secondary check only, because a permit held across the send leaves `in_flight()` at 0 while B starves.
  - Break it twice and say so in the commit body. Remove the `tokio::spawn` from step 26 once, and B falls to about 50%. Hold the permit across the send once, and B falls to 0%. Both fail the rate assertion and the permit assertion.
- **Concepts.**
  - *`Notify` as a test gate.* A `Notify` that never fires holds a task at one exact `.await`, so the test controls where the stall happens. Trap: a `setTimeout` long enough to look stuck, which makes the test slow and flaky. Source: [tokio docs, `sync::Notify`](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html).
- **Check.** `cargo test -p bt-ingest stalled_writer` passes, and fails with the spawn removed and with the permit held across the send. In the reference test, B alone runs at 80.0 calls a second, and B beside a stalled A at 99% to 100% of that (`spikes/crates/runner`, `two-jobs`).

## Step 37. `feat(ingest): index epoch dividends over a 31-day window`

- **Goal.** `epochs` runs on every block with `Window(31 days)` and writes `epoch_dividend`. Its backfill starts one day before the window, 32 days and 230,400 blocks back, the same margin `prices` has.
- **Files.** `crates/ingest/src/jobs/epochs.rs`, `crates/domain/src/rows.rs` (`Epochs`), `crates/store/migrations/0004_epoch_dividend.sql`, `crates/store/src/epochs.rs`.
- **Shape.**

  ```rust
  pub struct EpochsJob;  // NAME = "epochs", Schedule::EveryBlock, History::Window(31 days)
  pub async fn insert_epoch_dividends(tx: &mut PgTransaction<'_>, batch: &[Epochs]) -> Result<(), StoreError> { todo!() }  // Epochs and BlockState live in bt-domain (D33)
  ```

- **Who writes what.** You write the job. The agent writes the DDL from [Storage](../design/04-storage.md#epoch-dividends), the writer, and its round-trip test. Review checks:
  - A block with no epoch writes no rows and still advances the checkpoint. A zero dividend never reaches the table.
  - The job reads `LastMechansimStepBlock` (`u64`, the chain's spelling), `AlphaDividendsPerSubnet` keyed by (`Identity` netuid, `Blake2_128Concat` hotkey), and `TotalHotkeyAlphaLastEpoch` keyed by (`Blake2_128Concat` hotkey, `Identity` netuid). The two maps take their keys in opposite order.
  - The round-trip test gives `dividend` and `stake_at_epoch` different values, so a swap fails it.
  - `stake_at_epoch` is `NOT NULL`. Release 1's window starts after spec 261, so the decoder cannot return `None` for it there. A `None` is an error that names the block, and the job never writes NULL or 0 in its place. The DDL rejects a NULL with 23502. What release 2 stores before spec 261 is open in the [release 2 outline](release-2-outline.md).
- **Concepts.** None new.
- **Check.** `cargo run -q -p bt-indexer -- status --watch` shows `epochs backfill` with a percent, a rate, and an ETA, then `epochs live`. The 230,400 blocks at 3.0 calls a block take between 2 h 55 min and 4 h 17 min at 14.96 to 21.95 blocks a second at 8 in flight, the longest backfill in release 1. `psql "$BT_DATABASE__URL" -c "select block_number, netuid, count(*) from epoch_dividend group by 1, 2 order by 1 desc limit 5"` shows about 50 rows per subnet epoch (34.6 to 55.5 on average across eras, `spikes/crates/chain-decode`, `epochs-cost`).

## Step 38. `feat(ingest): index basket snapshots from spec 441`

- **Goal.** `baskets` runs every 500 blocks with `Window(31 days)`. It starts at the later of its window start and the first block spec 441 executes, `first_block + 1` of the first spec that declares `BetaBasketRuntimeApi`. Spec 441's upgrade block is 8,765,683, so the floor is 8,765,684. At the upgrade block itself the job reads `BasketRate` with the executing spec, 440, which does not declare it ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)). In release 1 the window start is always later, so the floor never binds. Release 2 needs it.
- **Files.** `crates/ingest/src/jobs/baskets.rs`, `crates/store/migrations/0005_basket_snapshot.sql`, `crates/store/src/baskets.rs`.
- **Shape.**

  ```rust
  pub struct BasketsJob;  // NAME = "baskets", Schedule::Every(500), History::Window(31 days)
  pub fn basket_floor(table: &VersionTable) -> Option<BlockNumber> { todo!() }  // the spec_floor of step 33, and step 50's floor
  ```

- **Who writes what.** You write the job and `basket_floor`. The agent writes the DDL from [Storage](../design/04-storage.md#baskets), the writer, and its round-trip test. Review checks:
  - `basket_floor` returns `first_block + 1` of spec 441 from step 13's `VersionTable`, 8,765,684. It reads the `VersionTable` because `runtime_version` holds only the specs the windows touch (step 24). The `+ 1` is there because `BasketRate` is read with the executing spec, 440, at the upgrade block, and 440 does not declare it ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec), [D8](../decisions.md#d8-find-the-spec-table-by-bisection-over-code-and-persist-only-the-specs-a-job-window-touches)).
  - The job calls `BetaBasketRuntimeApi_get_all_validator_baskets` through `state_call`, with the metadata of the block's own state spec ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)), and decodes one `BasketSummary` struct without `weights`. `DecodeAsType` skips `weights` by name for specs 441 to 459. It was dropped at spec 464 (block 9,088,597).
  - `rate_bits` (the `i128` of `BasketRate`, keyed by `Blake2_128Concat(AccountId32)`) and `shares` reach `NUMERIC(39,0)` through `BigDecimal`, never through `f64`. A basket with no stored `BasketRate` key reads the metadata default, which the round-trip test asserts is 0. `rate_bits` is `None` only when the executing spec does not declare `BasketRate`, and then the job writes no row for that basket, as step 19 and [Storage](../design/04-storage.md#baskets) say ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)). In the release 1 window `BasketRate` is always declared. The job never writes NULL or 0 for `rate_bits`, and the DDL rejects a NULL with 23502.
- **Concepts.** None new.
- **Check.** `cargo run -q -p bt-indexer -- status` shows `baskets` starting at the first multiple of 500 at or above its window start, 9,260,500 at the head measured on 2026-11-12, and then reaching `live`. Each basket call takes 8.7 to 33.5 s, so the job relies on `chain.rpc_timeout = "90s"` ([D23](../decisions.md#d23-cap-the-call-rate-with-a-one-token-bucket-not-only-the-calls-in-flight), `spikes/crates/chain-decode`, `basket-call-time`). From the mean basket call, measured at up to 4 in flight, the 461 snapshots take about 46 min. Basket calls at the backfill `concurrency` of 8 are not yet measured, so write the longest basket call and the count of `Timeout` errors from the run's log into the commit body. `psql "$BT_DATABASE__URL" -c 'select block_number, count(*) from basket_snapshot group by 1 order by 1 desc limit 1'` shows the newest snapshot's basket count, 191 at the head on 2026-11-12 (`spikes/crates/chain-decode`, `squid-baskets`).

## Step 39. `feat(ingest): index subnet identities as a latest-only set`

- **Goal.** `identities` runs every 100 blocks with `Latest` history. It writes only rows that changed and deletes rows for subnets that no longer have an identity, as [Storage](../design/04-storage.md#subnet-identities) says ([D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests)).
- **Files.** `crates/ingest/src/jobs/identities.rs`, `crates/store/migrations/0006_subnet_identity.sql`, `crates/store/src/identities.rs`.
- **Shape.**

  ```rust
  pub struct IdentitiesJob;  // NAME = "identities", Schedule::Every(100), History::Latest
  pub struct IdentityChanges { pub upserts: Vec<SubnetIdentity>, pub deletes: Vec<NetUid> }
  pub fn identity_changes(old: &[SubnetIdentity], new: &[SubnetIdentity]) -> IdentityChanges { todo!() }
  ```

- **Who writes what.** You write the job and `identity_changes`. The agent writes the DDL and the writer. Review checks:
  - The job reads `SubnetIdentitiesV3`, keyed by `Blake2_128Concat(u16)`, not `Identity`, into the 8 `Vec<u8>` fields of `SubnetIdentityV3`.
  - The writer reads the current rows inside the same transaction that writes the changes.
  - Every `subnet_identity` text column is nullable, so each needs the `as _` override in the `UNNEST` `query!`, as in step 35.
  - `updated_block` moves only on a changed row. A second run at a later block with the same identities writes 0 upserts and 0 deletes and leaves `updated_block` where it was (`spikes/crates/store`, `P26`).
- **Concepts.** None new.
- **Check.** `cargo run -q -p bt-indexer -- status` lists all five jobs, and `psql "$BT_DATABASE__URL" -c 'select count(*) from subnet_identity'` prints the subnets with an identity, 121 of the 128 in `NetworksAdded` on 2026-11-12.

## Step 40. `feat(ingest): verify retention policies and add an hourly gap check`

- **Goal.** Prove that windowed tables drop old chunks, and add an hourly task that looks for gaps in `prices`, as [Find gaps](../design/03-ingestion.md#find-gaps) describes. Migrations 0002, 0004 and 0005 create the policies with their hypertables ([D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests)), and this step tests them.
- **Files.** `crates/store/src/prices.rs`, `crates/store/tests/retention.rs`, `crates/ingest/src/gap.rs`, `crates/ingest/tests/gap.rs`.
- **Brief for the agent.** Write a retention test that back-dates rows, pauses each policy job with `alter_job(id, scheduled => false)`, runs it with `CALL run_job(id)`, and checks which rows remain. Add a gap task to the supervisor's `JoinSet` that runs the `prices` gap query once an hour, sets `bt_gap_blocks{worker}`, and logs an error on a gap. The query is the one in [Find gaps with a hashed DISTINCT](../design/04-storage.md#find-gaps-with-a-hashed-distinct): `SELECT (max(cp.last_block) - min(d.b) + 1 - count(*))::bigint FROM (SELECT DISTINCT block_number AS b FROM subnet_price) d, checkpoint cp WHERE cp.worker = 'prices' AND d.b <= cp.last_block`.
- **Who writes what.** The agent writes it. You review it. Review checks:
  - Each policy is the window plus 2 days. A 4-day policy keeps a 4.50-day-old row, and a 33-day policy keeps a 34.00-day-old row, because rows outlive the window by up to one chunk (`spikes/crates/store`, `retention`). A later change uses `remove_retention_policy` then `add_retention_policy`, because a second `add_retention_policy` fails with 42710.
  - The gap query uses the hashed `SELECT DISTINCT` subquery. `count(DISTINCT block_number)` sorts 80 MB to disk and takes 1.6 to 2.4 s at 4.6 million rows, against about 0.12 s (`spikes/crates/store`, `read-gap`).
  - The gap task waits on the token, so it stops at once on cancel.
- **Concepts.** None new.
- **Check.** `cargo test -p bt-store retention` passes `retention_drops_rows_past_each_policy`, and `cargo test -p bt-ingest gap_` passes `gap_check_sees_a_deleted_block`, which deletes one block's rows and sees the gauge at 1. `psql "$BT_DATABASE__URL" -c "select hypertable_name from timescaledb_information.jobs where proc_name = 'policy_retention'"` lists `basket_snapshot`, `epoch_dividend`, and `subnet_price`. A head-era block holds at most 128 `subnet_price` rows, because the job drops the root entry (netuid 0 at 1,000,000,000) of the 129 that `current_alpha_price_all` returns.

## Step 41. `feat(deploy): add the Grafana progress dashboard`

- **Goal.** Prometheus scrapes the indexer, and Grafana shows the Progress row of [Observability](../design/07-observability.md#the-progress-row-shows-every-job-at-a-glance).
- **Files.** `deploy/compose.yaml`, `deploy/prometheus/prometheus.yml`, `deploy/grafana/`.
- **Brief for the agent.** Add the `prometheus` and `grafana` services to compose, the scrape configuration, and file provisioning for the data source and the indexer dashboard. The Progress row has one stat panel per job, built from the expressions in the observability doc.
- **Who writes what.** The agent writes it. You review it. Review checks:
  - Panels read only the `bt_` progress metrics, and job names match the `worker` label.
  - The dashboard JSON holds no hard-coded data source id.
- **Concepts.** None new.
- **Check.** `docker compose -f deploy/compose.yaml up -d prometheus grafana`, then `localhost:3000` shows five job panels.

## Step 42. `docs(readme): record measured backfill time per job`

- **Goal.** Run the first full backfill of release 1 and close M2 with its numbers. Drop the database, run `ingest` until every job is live, and write each job's backfill time, blocks per second, and calls into `README.md`, with the machine and the node. This run also measures the combined time of five jobs that share 16 permits and the 120 calls a second cap, and historic `state_call` latency under the indexer's own load, where one call has taken from 0.3 s to 19 s.
- **Files.** `README.md`.
- **Who writes what.** You run the backfill and write the table. The agent checks that every number in the table comes from this run's log or `status` output. Review checks:
  - Each job has a time, a rate, and a call count. Expect totals near the calls and inside the time range in this stage's introduction, or a longer time if the node has slow windows. A total far outside that range needs an explanation before it goes in the table.
  - The table also records each hypertable's size from `hypertable_detailed_size`. These real sizes take the place of the synthetic sizes measured in `spikes/crates/store`.
  - The numbers replace the estimates in [Estimate the backfill time](../design/03-ingestion.md#estimate-the-backfill-time), and the README links to them.
- **Concepts.** None new.
- **Check.** From an empty database, `cargo run -q -p bt-indexer -- status --watch` shows each job's block, percent, rate, and ETA, then each job live. The README table holds the numbers, and one more line: the time from start to the first committed batch, which covers fetching and storing the metadata of all 11 specs.

## Stop here

- **Demo.** Drop the database and start `ingest`. `status --watch` and the Grafana Progress row show five jobs fill and reach `live`.
- **Explain without notes.**
  - Why does `start_block` never move, and what does a `Latest` job do after a long stop?
  - Why does lag count from the newest scheduled block and not from the head?
  - Why does the stalled-writer test fail when the read spawn is removed, and why does `in_flight()` alone miss a permit held across the send?
  - Why does the `u128` math use `checked_*` under a deny lint?
  - Why does the stalled-writer test run on real time?
- **Roadmap.** Closes M2. `bt-indexer status` shows each job's backfill progress and then each job live, and the README records the measured backfill time per job and the machine.
