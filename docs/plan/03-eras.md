# Stage 3. Decode every era offline

This stage goes back to roadmap M0 and finishes it. You find every runtime upgrade, record real blocks as fixtures, and show with no network that each value the five jobs read decodes in every era. It also tests the parent-metadata rule of [D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec) at a real upgrade block. It closes roadmap M0. Before you start, stages 1 and 2 must be done.

Release 1 reads only specs 475 to 490, because its windows reach back only to block 9,260,462 at the head measured on 2026-11-12 ([D3](../decisions.md#d3-set-the-milestone-scope-by-what-release-1-reads)). Those windows still hold upgrade blocks, 10 for `epochs` and 1 for `prices` (9,478,217), so the D6 rule matters to release 1 too. The fixtures split this way.

- Release 1 exercises the fixtures at block 9,490,000 (spec 490): `prices_head`, `baskets_head`, `identities_head`, `validators_head`, and the `epochs_post_261` fixture, which sits at or after 9,260,462, and `prices_upgrade_block_9478217`, at the upgrade block of spec 490, which spec 488 executed.
- The fixtures at older blocks exist for release 2. These are spec 233 at 4,920,351, the upgrade block 4,920,350, the 290, 391, and 423 price boundaries, the spec 257 upgrade block, the pre-261 epoch block, the `weights` basket shape of specs 441 to 459, and the two `Delegates` fixtures at 8,636,190 and 8,636,191. All of them decode in `spikes/crates/chain-decode` (26 of 26 cases, `spikes/data/decode-cases.csv`).

The bisection and the fixture recordings make hundreds of historic calls, and each command can resume. On the keyed archive node, bisecting `:code` across the whole history took 3,222 calls and about 1 minute, plus about 11 minutes of version checks (measured on 2026-11-12, [D8](../decisions.md#d8-find-the-spec-table-by-bisection-over-code-and-persist-only-the-specs-a-job-window-touches)). The public archive node runs at about 9 calls a second ([stage 1](01-read-path.md)), so these commands take much longer there.

## Step 13. `feat(cli): list every runtime upgrade by bisection`

- **Goal.** Find the upgrade block of every mainnet spec since dTAO and write them to `fixtures/specs.csv`. `first_block` is the upgrade block, the first block whose state reports the spec, and the spec executes from `first_block + 1` ([D8](../decisions.md#d8-find-the-spec-table-by-bisection-over-code-and-persist-only-the-specs-a-job-window-touches)). The first row is the base row, the spec at the start block: 219 at 4,920,349. Its true upgrade block is earlier and is not needed, and with it `executed_by(4920350)` is 219. This step also seeds the in-memory spec table from the file, which is all M0 needs. Step 24 persists it in M1.
- **Files.** `crates/chain/src/source.rs` (the two new `BlockSource` methods), `crates/chain/src/upgrades.rs`, `crates/chain/src/versions.rs`, the `chain specs` subcommand, `fixtures/specs.csv`.
- **Brief for the agent.** Add two methods to `BlockSource`: `fn code_hash(&self, n: BlockNumber) -> impl Future<Output = Result<[u8; 32], ChainError>> + Send` and `fn spec(&self, n: BlockNumber) -> impl Future<Output = Result<SpecVersion, ChainError>> + Send`. They sit on the trait so that `FixtureSource` has them too and the step 24 code that calls them compiles on `S: BlockSource` alone.

  `SubxtSource` implements `spec` with `state_getRuntimeVersion`. `LegacyRpcMethods` has no method for `state_getStorageHash`, so `code_hash` sends a raw request through the `RpcClient` that `SubxtSource` keeps: `client.request::<Option<H256>>("state_getStorageHash", rpc_params!["0x3a636f6465", hash])`, where the key is `:code` in hex and `hash` comes from one `chain_getBlockHash(n)`. `code_hash` takes a block number, not a `BlockRef`, so a probe costs two requests and not the four that `block_ref` makes before step 24.

  Then write `impl<S: BlockSource> Chain<S> { pub async fn next_upgrade(&self, lo: BlockNumber, hi: BlockNumber) -> Result<Option<(BlockNumber, SpecVersion)>, ChainError> }`, which returns the first upgrade block in `(lo, hi]` with its spec. A range whose two ends have the same `:code` hash holds no upgrade. At each boundary n, confirm with `state_getRuntimeVersion` at n−1 and n. Every request goes through step 6's `Chain::call`.

  `chain specs --from <n>` prints `spec,first_block`, then the base row, the spec of `n`'s state with `first_block` set to `n`, then one row for each upgrade after `n`. It prints and flushes each row as it finds it, so an interrupted run leaves every row found so far in the file. A resumed run with `--from` set to the last row's `first_block` prints that row again as its base row, then the rows after it.

  In `versions.rs`, write this.

  ```rust
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct VersionTable { first_block: BTreeMap<BlockNumber, SpecVersion> }
  #[derive(Debug, thiserror::Error, PartialEq, Eq)]
  pub enum SpecsError { Header, Row(usize), Order(usize) } // each with an #[error]: no header, a bad line, a line not above the one before
  impl VersionTable {
      pub fn seed() -> Result<VersionTable, SpecsError> { todo!() } // parse(include_str! of fixtures/specs.csv)
      pub fn parse(csv: &str) -> Result<VersionTable, SpecsError> { todo!() } // step 24's test parses the file without its last row
      pub fn spec_at(&self, n: BlockNumber) -> Option<SpecAt> { todo!() }
      pub fn rows(&self) -> impl Iterator<Item = (BlockNumber, SpecVersion)> + '_ { todo!() } // step 17 reads it
  }
  ```

  `VersionTable` is the one owner of `spec_at`, and it holds every row ([D8](../decisions.md#d8-find-the-spec-table-by-bisection-over-code-and-persist-only-the-specs-a-job-window-touches)).
- **Who writes what.** The agent writes it. You review it. Review checks:
  - `first_block` comes from bisection over `:code`, never from the first block some reader happened to see (`spikes/crates/chain-specs`, `P5`). Bisecting over `state_getRuntimeVersion` is too slow on the keyed node (`spikes/crates/chain-specs`, `bisect-method`).
  - A test serves a fake source whose state switches to spec 2 at block 999 and asserts `next_upgrade(1, 1050) == Some((999, 2))`, so a reader that first saw spec 2 at block 1,050 still gets `first_block = 999`.
  - A spec that goes down is an error, not an assumption. It is `SpecsError::Order` when seeding, and `ChainError::Unknown` naming both specs in `next_upgrade`.
  - The row after the base row agrees with step 1's measurement.
  - `spec_at(n)` gives `state` as the spec of the last row with `first_block <= n`, and `executed_by` as the spec of the last row with `first_block < n`. It gives `None` at or before the base row's block, where no row gives the executing spec.
  - `chain specs` never calls `VersionTable::seed`, so it runs while `fixtures/specs.csv` holds only its header.
- **Concepts.** None new.
- **Check.** Run these three lines. `VersionTable::seed` embeds `fixtures/specs.csv`, so the first line creates the file with its header, and `bt-chain` compiles from then on. The third line, `cargo run -p bt-indexer -- chain specs --from 4920349 > fixtures/specs.csv`, writes the table.

  ```sh
  mkdir -p fixtures && printf 'spec,first_block\n' > fixtures/specs.csv
  cargo test -p bt-chain sparse_reader_gets_true_first_block
  cargo run -p bt-indexer -- chain specs --from 4920349 > fixtures/specs.csv
  ```

  The test passes. If the third line is interrupted, resume by appending, because `>` empties the file: `cargo run -p bt-indexer -- chain specs --from <last first_block> | sed -u 1,2d >> fixtures/specs.csv`. `sed -u 1,2d` drops the header and the repeated row and writes each row as it arrives, so a resume that is killed keeps the rows it found.

  Then run the standing checks on the full file.
  - `head -3 fixtures/specs.csv` prints `spec,first_block`, `219,4920349`, and `233,4920350`.
  - `diff <(tail -n +3 fixtures/specs.csv) <(tail -n +2 ../rust-bittensor-indexer-rnd/spikes/data/specs.csv | cut -d, -f1,4)` prints only rows for specs after 490. The second file is in the research repository, checked out beside this one.
  - `cargo test -p bt-chain version_table` passes offline, including `version_table_gives_both_specs_at_the_dtao_upgrade`, with `executed_by=219 state=233` at 4,920,350 and `executed_by=233 state=233` at 4,920,351, the values step 1 printed.

  Since dTAO, mainnet has run 106 specs, 233 to 490, the last at 9,478,217. These sample blocks show the file's range. Block 5,500,000 ran spec 261, 6,000,000 ran 292, 7,000,000 ran 348, 8,000,000 ran 393, 8,500,000 ran 423, and 9,000,000 ran 454.

## Step 14. `feat(cli): record a block's state as a fixture`

- **Goal.** Turn any block into an offline fixture, holding the raw storage and runtime API bytes a decoder reads, and the metadata once per spec. The layout follows [Quality](../design/09-quality.md#test-the-decoders-against-recorded-blocks), with `fixtures/state/` and `fixtures/metadata/<spec>.scale` ([D34](../decisions.md#d34-keep-one-fixture-per-test-case-and-record-it-by-running-the-jobs-own-read)).
- **Files.** `crates/chain/src/fixture.rs` (the format, which step 15's `FixtureSource` shares, and `Recorder`), the `fixture record` subcommand, `fixtures/state/spec_233_block_state.json`, `fixtures/metadata/233.scale`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Brief for the agent.** `fixture record --block <n> --case <name> --read <job>` writes one recording per test case to `fixtures/state/<case>.json`, named for the case and not the block, so two cases at the same block never share a file ([D34](../decisions.md#d34-keep-one-fixture-per-test-case-and-record-it-by-running-the-jobs-own-read)). `--read` names a `Chain` read. This step offers `block_state` and `prices`. Step 17 adds `events`, and steps 18 to 21 add `epochs`, `baskets`, `identities`, `validators`, and `delegates`. The recorder wraps the live source in `pub struct Recorder<S: BlockSource>`, which implements `BlockSource`, passes each call on, and keeps what it returns. It then runs the same `Chain` read the test runs, on `Chain<Recorder<SubxtSource>>`, so the fixture holds exactly the requests the test makes. The recorder runs the job read and not a list of items, because a runtime API such as `current_alpha_price` needs a netuid, and only the job read knows the netuids. It writes `{block, hash, spec: {executed_by, state}, storage: [{key, value}], prefixes: [{prefix, pages}], calls: [{name, args, result}]}` as hex with sorted keys, where `value` is null for a key the node answered with no value. Values stay raw SCALE and are never decoded. Metadata calls never go in `calls`. The recorder always fetches both metadatas of the block, `Decodes::Storage` and `Decodes::RuntimeApi`, even when the read needs only one, and writes them to `fixtures/metadata/<spec>.scale` by spec, so an upgrade block records two files. Each is 0.2 to 0.45 MB (201,451 bytes for spec 219, and 444,782 bytes for spec 490 at the metadata version step 4 picks), and only the specs a test uses are committed.
- **Who writes what.** The agent writes it. You review it. Review checks:
  - Recording the same block twice gives a byte-identical file.
  - The format struct derives `Deserialize` and `Serialize` and rejects unknown fields.
  - The metadata file is written only if it is missing.
  - The read the recorder runs is the `Chain` method the test calls, not a copy of it.
- **Concepts.**
  - *serde derive.* The fixture is a typed struct, so a malformed or hand-edited file fails to load with the field name in the error. Trap: `JSON.parse` habits that accept any shape and fail later, deep in a decoder. Source: serde.rs, "Using derive".
- **Check.** `cargo run -p bt-indexer -- fixture record --block 4920351 --case spec_233_block_state --read block_state` writes the two files. A second run leaves `git status --short fixtures` empty.

## Step 15. `test(chain): decode the spec 233 fixture offline`

- **Goal.** The roadmap M0 check in miniature. A test serves block 4,920,351 from `FixtureSource` and decodes its `BlockState` with no network.
- **Files.** `crates/chain/src/fixture.rs` (`FixtureSource`), `crates/chain/tests/decode.rs`, `crates/chain/tests/snapshots/`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  pub struct FixtureSource { blocks: BTreeMap<BlockNumber, Fixture>, metadata: HashMap<SpecVersion, Vec<u8>>, served: Mutex<Vec<(BlockNumber, SpecVersion)>> }
  #[derive(Debug, thiserror::Error)]
  pub enum FixtureError { Io { path: PathBuf, source: std::io::Error }, Json { path: PathBuf, source: serde_json::Error }, Collision(u32) } // each with an #[error]
  impl FixtureSource {
      pub fn load(dir: &Path, cases: &[&str]) -> Result<FixtureSource, FixtureError> { todo!() } // fixtures/state/<case>.json
      pub fn served(&self) -> Vec<(BlockNumber, SpecVersion)> { todo!() } // (block, spec) of each metadata it served
  }
  impl BlockSource for FixtureSource { /* finalized_head, block_ref, storage, runtime_api, code_hash, spec: lookups */ }
  #[tokio::test]
  async fn spec_233_block_state() { todo!() } // insta::assert_debug_snapshot!(chain.block_state(at).await?)
  ```

- **Who writes what.** You write `FixtureSource` and the test. The agent reviews. Review checks:
  - The test goes through `Chain<FixtureSource>`, not through `decode_value`.
  - Each test loads only its own cases, so two cases recorded at one block never collide.
  - A key the fixture lacks is an error, `ChainError::Unknown` built with `subxt::Error::other_str`, and never an absent value.
  - `finalized_head` returns the highest recorded block.
  - `block_ref(n - 1)` for a recorded block n, which step 4's `Decodes::Storage` fetch asks for, returns `BlockRef { number: n - 1, hash: BlockHash([0; 32]), spec: SpecAt { executed_by: e, state: e } }`, where `e` is the fixture's `executed_by`. The zero hash is never read, because `FixtureSource` serves metadata by spec. Without this answer the test fails with `BlockNotFound(BlockNumber(4920350))`.
  - Metadata calls are served by spec from `fixtures/metadata/` and never from `calls`. `Metadata_metadata_versions` answers the version stored in the `.scale` file, and `Metadata_metadata_at_version` answers `metadata[at.spec.state]`. In `calls`, the two fetches at an upgrade block would share one key, since specs 488 and 490 both serve version 16.
  - A `Prefix` read flattens the recorded pages and cuts them again after `start` at the requested page size, so a test can read with a page size other than the recorder's.
  - The snapshot shows `ts` 2025-02-13T21:41:24Z, which `chain read --block 4920351` also prints (the `first_executed_utc` column of `spikes/data/specs.csv`).
- **Concepts.**
  - *Integration tests in `tests/`.* Each file in `tests/` is its own crate that sees only the public API of `bt-chain`, so the test uses `Chain` exactly as `bt-ingest` will. Trap: testing private helpers through a `pub` you added only for the test. Source: The Rust Book, "Test Organization".
  - *insta snapshots.* The first run writes the decoded value to a `.snap` file, and later runs fail on any difference. Trap: accepting a new snapshot without reading it. `cargo insta review` shows each diff. Source: insta.rs, "Quickstart".
- **Check.** `cargo test -p bt-chain --test decode spec_233_block_state` passes, offline.

## Step 16. `test(chain): decode prices offline in every era`

- **Goal.** Show offline that step 7 and step 8 decode prices on both sides of each price boundary, at the dTAO upgrade block 4,920,350, and at 9,478,217, the one upgrade block in release 1's `prices` window. The upgrade blocks are 5,947,548 (spec 290), 7,782,857 (spec 391), and 8,486,593 (spec 423).
- **Files.** `crates/chain/tests/decode.rs`, `crates/chain/tests/snapshots/`, fixtures under `fixtures/state/` and `fixtures/metadata/`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Brief for the agent.** For each boundary, record with `fixture record --read prices` at `u - 1` and `u + 1`: 5,947,547 and 5,947,549, 7,782,856 and 7,782,858, 8,486,592 and 8,486,594. Record also 4,920,350, 4,920,351, 9,478,217, and 9,490,000. Give each recording `--case` set to its test's name, so it lands in `fixtures/state/<test name>.json`. Block 9,478,217 records the metadata of spec 488, which executed it, and spec 490, which its state reports. Report each fixture's size before you commit it.
- **Who writes what.** The agent records the fixtures. You write the tests, one per fixture. Review checks:
  - Each test goes through `Chain::prices` and `FixtureSource`.
  - The test names say the era and the side, such as `prices_before_391` and `prices_from_391`.
  - Each snapshot shows the `PriceSource` the block used.
  - `prices_upgrade_block_4920350` asserts no pools, meaning an empty `points` and no `DecodeError`. At that block `SubnetTAO`, `SubnetAlphaIn`, `SubnetAlphaOut`, and `SubnetVolume` have 0 keys while `NetworksAdded` has 65, so a decoder that defaulted an absent value would return a zero price for every subnet ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec); `spikes/crates/chain-specs`, `P31`).
  - `prices_upgrade_block_9478217` is the one release 1 test where `executed_by` (488) differs from `state` (490). Its storage decodes with spec 488's metadata, and its price call with spec 490's. The values are equal under both metadatas at this block, so the test also asserts, through `FixtureSource::served`, which spec's metadata was served for each read, 488 for storage and 490 for the call. Its snapshot shows `AllSubnetsApi` and a full set of points (`spikes/crates/chain-decode`, `storage-executing-spec`).
  - `upgrade_block_storage_metadata_9478217` checks which metadata the storage decode uses, since `FixtureSource` sees only which metadata it hands out. It decodes 490's `RuntimeMetadataPrefixed` with `subxt::ext::frame_metadata`, drops `SubtensorModule.SubnetTAO` with `retain`, and re-encodes it into a temporary fixture directory beside a copy of the case and of `488.scale`. It then runs `Chain::prices` and asserts 128 points.
- **Concepts.** None new.
- **Check.** Run both commands.
  - `cargo test -p bt-chain --test decode prices_` passes ten tests offline. They are `prices_233`, `prices_upgrade_block_4920350`, `prices_upgrade_block_9478217`, `prices_head`, and the `before` and `from` tests for 290, 391, and 423. Only `prices_upgrade_block_9478217` and `prices_head` cover specs release 1 reads.
  - `cargo test -p bt-chain --test decode upgrade_block_storage_metadata_9478217` passes offline.

  The `served` assertion in `prices_upgrade_block_9478217` catches a decoder that asks `Decodes::RuntimeApi` for the storage metadata. `upgrade_block_storage_metadata_9478217` catches a decoder that fetches both metadatas and then decodes storage with the API one. That decoder reads the trimmed 490 metadata and finds 0 points, while the right one reads `SubnetTAO` with 488's metadata and finds 128.

## Step 17. `feat(chain): prove the parent-metadata rule at a StakeAdded upgrade`

- **Goal.** Implement subxt's `Config` hooks from a version table, and show offline that the events of the spec 257 upgrade block decode with the parent's metadata and fail with their own ([Chain data](../design/02-chain-data.md#give-subxt-the-spec-and-metadata)). The two rules differ only at the upgrade block, so the test uses block 5,228,683. Spec 252 executed it, its state reports 257, and it holds 4 `StakeAdded` with 5 fields each (`spikes/crates/chain-specs`, `D-stakeadded-257`).
- **Files.** `crates/chain/src/config.rs`, `crates/chain/src/events.rs`, `crates/domain/src/account.rs`, the `events` read of `fixture record`, the fixture `fixtures/state/stake_added_5228683.json` of block 5,228,683 with `System.Events`, which both tests load ([D34](../decisions.md#d34-keep-one-fixture-per-test-case-and-record-it-by-running-the-jobs-own-read) names it as the one fixture shared by two cases), the tests in `crates/chain/tests/decode.rs`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  #[derive(Clone, Debug, Default)]
  pub struct Subtensor { versions: Arc<RwLock<BTreeMap<u64, u32>>>, metadata: Arc<RwLock<HashMap<u32, ArcMetadata>>> }
  impl Subtensor { pub fn new(table: &VersionTable) -> Subtensor { todo!() } }  // block number to executing spec, from step 13's table
  impl subxt::Config for Subtensor {
      type AccountId = <SubstrateConfig as Config>::AccountId; type Address = <SubstrateConfig as Config>::Address;
      type Signature = <SubstrateConfig as Config>::Signature; type Hasher = <SubstrateConfig as Config>::Hasher;
      type Header = <SubstrateConfig as Config>::Header; type AssetId = <SubstrateConfig as Config>::AssetId;
      type TransactionExtensions = SubstrateExtrinsicParams<Self>;
      fn spec_and_transaction_version_for_block_number(&self, n: u64) -> Option<(u32, u32)> { todo!() }
      fn metadata_for_spec_version(&self, spec: u32) -> Option<ArcMetadata> { todo!() }
      fn set_metadata_for_spec_version(&self, spec: u32, md: ArcMetadata) { todo!() } }
  // in bt-domain
  #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, DecodeAsType)]
  pub struct Hotkey([u8; 32]); // and Coldkey; both with pub fn bytes(&self) -> &[u8; 32]
  // in bt-chain
  #[derive(DecodeAsType)] struct StakeAddedV233(Coldkey, Hotkey, u64, u64, u16); // StakeAddedV257 adds a u64
  pub struct StakeAdded { pub coldkey: Coldkey, pub hotkey: Hotkey, pub tao: u64, pub alpha: u64, pub netuid: u16, pub fee: Option<u64> }
  // with a From from each era struct
  pub fn stake_added_at(cfg: Subtensor, n: BlockNumber, raw_events: Vec<u8>) -> Result<Vec<StakeAdded>, DecodeError> { todo!() }
  ```

  `Subtensor::new` reads the table through `VersionTable::rows`, since the field is private. `stake_added_at` and `StakeAdded` are public, because `tests/decode.rs` sees only the public API. `Hotkey` and `Coldkey` only decode, so code that builds a storage key from one passes `*hk.bytes()`.
- **Who writes what.** You write it. The agent records the fixture with `fixture record --read events`, which reads the raw `System.Events` through `Chain::storage` and so records the metadata of 252 and 257. Review checks:
  - Events decode only through `OfflineClient`, with the executing spec's metadata registered before `at_block`.
  - Nothing builds an `OnlineClient<Subtensor>`. Its metadata miss at an upgrade block files the state spec's metadata under the executing spec and breaks that spec's other blocks (`spikes/crates/chain-specs`, `online-metadata-miss`).
  - The negative test gets 257's metadata from step 4's `Chain::metadata` with `Decodes::RuntimeApi` and registers it under 252 with `set_metadata_for_spec_version`.
  - The fields are the ones in [Chain data](../design/02-chain-data.md#give-subxt-the-spec-and-metadata), both behind `Arc<RwLock<_>>` because `set_metadata_for_spec_version` takes `&self`.
  - The hook returns the spec that executed `n`, and only events go through it, because one hook cannot serve both specs ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)). Storage keeps decoding through `frame-decode` with the executing spec's metadata, and runtime API calls with the state spec's.
  - `TransactionExtensions` is `SubstrateExtrinsicParams<Self>`, since delegating it to `SubstrateConfig` fails with E0277.
  - Fields decode with `decode_fields_unchecked_as`.
  - The `DecodeAsType` derive needs the `scale-decode` crate with its `derive` feature in `bt-domain` and `bt-chain`.
  - The test asserts both directions.

  The worked reference is `spikes/crates/chain-specs/src/cfg.rs` (`spikes/crates/chain-specs`, `subtensor-config`, `P28`). The negative test asserts that decoding fails, not a message, because the message depends on where a decoder stops. Yours picks `StakeAddedV233` from the hook's spec, 252, so with 257's metadata it fails at the first `StakeAdded` with `expected length 5 but got length 6`. The reference stops later, with `Can't decode field "dispatch_info" in event System.ExtrinsicSuccess`.
- **Concepts.**
  - *A foreign trait with associated types.* `subxt::Config` fixes seven types at compile time and answers two hooks at run time. Six delegate to `SubstrateConfig`, and `TransactionExtensions` names `Self`. Trap: reading a trait as a duck-typed interface that you can satisfy partly. Source: The Rust Book, "Advanced Traits", section "Associated Types".
  - *`[u8; 32]` account newtypes.* `Hotkey` and `Coldkey` both wrap 32 bytes, and stake events put the coldkey first while basket events put the hotkey first. With the newtypes, a `From` that swaps them fails with `E0308`. Trap: a single `AccountId` string type, which lets the swap compile. Source: scale-decode docs, the `DecodeAsType` derive.
  - *`BTreeMap::range`.* The spec that executed block n is the one whose upgrade block is the last before n, which is `range(..n).next_back()`, one ordered lookup. Trap: a sorted array with a hand-written binary search. Source: [std, `BTreeMap::range`](https://doc.rust-lang.org/std/collections/struct.BTreeMap.html#method.range).
- **Check.** `cargo test -p bt-chain parent_metadata && cargo test -p bt-chain own_metadata` passes `parent_metadata_decodes_upgrade_block`, which finds 4 `StakeAdded`, and then `own_metadata_rejects_upgrade_block`.

## Step 18. `feat(chain): read epoch dividends before and after spec 261`

- **Goal.** Detect epoch blocks from `LastMechansimStepBlock`, and read each epoch's nonzero dividends and `stake_at_epoch` ([Chain data](../design/02-chain-data.md#detect-an-epoch-from-chain-state)). `TotalHotkeyAlphaLastEpoch` appears at spec 261, whose upgrade block is 5,328,895.
- **Files.** `crates/domain/src/epoch.rs`, `crates/domain/src/rows.rs` (`Epochs`), `crates/chain/src/jobs/epochs.rs`, the `epochs` read of `fixture record`, `fixtures/state/epochs_pre_261.json` at one epoch block before 5,328,895, and `fixtures/state/epochs_post_261.json` at one at or after 9,260,462.
- **Shape.**

  ```rust
  pub struct EpochDividend { pub netuid: NetUid, pub hotkey: Hotkey, pub dividend: u64, pub stake_at_epoch: Option<u64> }
  pub fn epoch_netuids(at: BlockNumber, last_step: &[(NetUid, u64)]) -> Vec<NetUid> { todo!() }
  pub struct Epochs { pub state: BlockState, pub dividends: Vec<EpochDividend> } // rows.rs, D33
  impl<S: BlockSource> Chain<S> { pub async fn epochs(&self, at: BlockRef) -> Result<Epochs, ChainError> { todo!() } }
  ```

- **Who writes what.** You write the code and the tests. The agent records the two fixtures with `fixture record --read epochs`, after it finds one epoch block on each side. Review checks:
  - The on-chain typo `Mechansim` is kept.
  - The `TotalHotkeyAlphaLastEpoch` key is built from `(*hk.bytes(), netuid)`, because `Hotkey` does not encode.
  - `epoch_netuids` is pure, keeps a netuid whose value equals `at`, and skips root.
  - `stake_at_epoch` is `None` when the executing spec does not declare `TotalHotkeyAlphaLastEpoch`, never 0. [D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec) names it as one of three exceptions to the rule that a job writes no row when the row needs an absent item. Release 1 never reads that era, and what release 2 stores there is open in the [release 2 outline](release-2-outline.md).
  - Zero dividends are dropped.
  - `AlphaDividendsPerSubnet` keys on `Identity` netuid then `Blake2_128Concat` hotkey.
- **Concepts.** None new.
- **Check.** `cargo test -p bt-chain epochs` passes `epochs_pre_261` and `epochs_post_261`. An epoch block holds about 50 dividend rows. `cargo test -p bt-domain epoch_netuids` passes, including `epoch_netuids_keeps_equal_block_and_skips_root`.

## Step 19. `feat(chain): read baskets in both summary shapes`

- **Goal.** Call `get_all_validator_baskets` and read `BasketRate` for each returned hotkey ([Chain data](../design/02-chain-data.md#read-baskets-from-the-runtime-api)). Mainnet went from spec 459 to spec 464 at block 9,088,597, and that upgrade dropped `weights` from `BasketSummary`. One struct without `weights` decodes both shapes, because `DecodeAsType` matches fields by name and skips the extra one (`spikes/crates/chain-decode`, `basket-one-struct`, `era-basket-weights`).
- **Files.** `crates/domain/src/basket.rs`, `crates/domain/src/rows.rs` (`Baskets`), `crates/chain/src/jobs/baskets.rs`, the `baskets` read of `fixture record`, `fixtures/state/baskets_with_weights.json` at 9,088,596 (spec 459) and `fixtures/state/baskets_head.json` at 9,490,000 (spec 490).
- **Shape.**

  ```rust
  #[derive(DecodeAsType)] struct BasketSummary { hotkey: Hotkey, nav_tao: u64, spot_nav_tao: u64, shares: u64, deposited_tao: u64, redeemed_tao: u64, holdings: Vec<BasketHolding> }
  #[derive(DecodeAsType)] struct BasketHolding { netuid: u16, alpha: u64, spot_tao: u64, realizable_tao: u64 }
  pub struct BasketSnapshot { pub hotkey: Hotkey, pub nav_tao: Rao, pub shares: u64, pub rate_bits: Option<i128> }
  pub struct Baskets { pub state: BlockState, pub baskets: Vec<BasketSnapshot> } // rows.rs, D33
  impl<S: BlockSource> Chain<S> { pub async fn baskets(&self, at: BlockRef) -> Result<Baskets, ChainError> { todo!() } }
  ```

- **Who writes what.** You write it. The agent records the two fixtures with `fixture record --read baskets`. One basket call takes 8.7 to 33.5 s (`spikes/crates/chain-decode`, `basket-call-time`), and `chain.rpc_timeout` is 90 s so that it fits ([D23](../decisions.md#d23-cap-the-call-rate-with-a-one-token-bucket-not-only-the-calls-in-flight)). Step 29 sets the client's own timeout to `rpc_timeout` plus 30 s. Until then the client's default of 60 s applies, and the basket call at 9,088,596 can outlast it and fail with `client error: Request timeout`. `fixture record` is idempotent, so run it again. Review checks:
  - The API result decodes with the state spec's metadata, and `BasketRate` with the executing spec's ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)).
  - The `BasketRate` key is built from `*hk.bytes()`.
  - `rate_bits` holds the raw `I96F32` bits as a plain `i128`. A key with no stored value reads the metadata default. `rate_bits` is `None` when the executing spec does not declare `BasketRate`, and the `baskets` job of step 38 then writes no row ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)).
  - No code branches on the spec number.
- **Concepts.**
  - *Fixed-point from raw bits.* The chain stores `I96F32` as a 128-bit integer, so the decoder keeps the bits and only `bt-domain` math converts the `i128` itself, with no fixed-point crate. Trap: converting to `f64` early, the way every JavaScript `Number` is stored, which loses precision above 2^53. Source: the fixed crate docs, `FixedI128::from_bits`.
- **Check.** `cargo test -p bt-chain baskets` passes `baskets_with_weights` (9,088,596, 21 nonempty `weights` skipped) and `baskets_head` (9,490,000, 191 baskets).

## Step 20. `feat(chain): read subnet identities and decode text like the squid`

- **Goal.** Read `SubnetIdentitiesV3`, and turn each byte string into the exact string the squid serves ([Chain data](../design/02-chain-data.md#decode-identity-bytes-the-way-the-squid-does)).
- **Files.** `crates/domain/src/text.rs`, `crates/domain/src/identity.rs`, `crates/domain/src/rows.rs` (`Identities`), `crates/chain/src/jobs/identities.rs`, the `identities` read of `fixture record`, `fixtures/state/identities_head.json` at 9,490,000.
- **Shape.**

  ```rust
  pub fn chain_text(bytes: &[u8]) -> Option<String> { todo!() } // lossy UTF-8, strip NUL, JavaScript trim, empty is None
  fn trim_js(s: &str) -> &str { todo!() }
  pub struct SubnetIdentity { pub netuid: NetUid, pub name: Option<String>, /* seven more */ }
  pub struct Identities { pub state: BlockState, pub identities: Vec<SubnetIdentity> } // rows.rs, D33
  impl<S: BlockSource> Chain<S> { pub async fn identities(&self, at: BlockRef) -> Result<Identities, ChainError> { todo!() } }
  ```

- **Who writes what.** You write it. The agent records the fixture with `fixture record --read identities`. Review checks:
  - Keys decode as `Blake2_128Concat(NetUid)`, not `Identity`.
  - Tests cover U+FEFF, U+0085, NUL, and invalid UTF-8.
  - `subnet_name` maps to `name`, `subnet_url` to `url`, and `subnet_contact` to `contact`.
  - `trim_js` uses the whitespace set of JavaScript's `String.prototype.trim`, written out as a list.
- **Concepts.**
  - *`String::from_utf8_lossy` and `Cow`.* It returns a borrowed `&str` when the bytes are valid and allocates only to repair them. Trap: assuming every call allocates a new string, as `TextDecoder` does. Source: [std, `String::from_utf8_lossy`](https://doc.rust-lang.org/std/string/struct.String.html#method.from_utf8_lossy).
  - *`char` predicates.* Rust's `char::is_whitespace` follows Unicode `White_Space`, which counts U+0085 and not U+FEFF. JavaScript's `trim` does the opposite for both. Trap: assuming `str::trim` matches `trim()`. Source: [std, `char::is_whitespace`](https://doc.rust-lang.org/std/primitive.char.html#method.is_whitespace).
- **Check.** `cargo test -p bt-domain text && cargo test -p bt-chain identities` passes, including `text_trims_like_javascript` and `identities_head`. `identities_head` decodes 121 identities against 128 subnets.

## Step 21. `feat(chain): read validator snapshot inputs with paged storage`

- **Goal.** Read every input of the `validators` job at one block, including the paged `TotalHotkeyAlpha` and `StakingHotkeys`, and measure the snapshot's call count. This is the most expensive read in release 1, so its cost shows here, before stage 5 builds the job on it.
- **Files.** `crates/domain/src/validators.rs`, `crates/chain/src/jobs/validators.rs`, `crates/chain/src/paging.rs`, the `validators` and `delegates` reads of `fixture record`, the trimmed `fixtures/state/validators_head.json` at block 9,490,000, and two fixtures of the `Delegates` item only, at blocks 8,636,190 and 8,636,191, named for their tests, `delegates_u16_at_424.json` and `delegates_at_432_first.json`.
- **Shape.**

  ```rust
  pub struct ValidatorInputs {
      pub delegates: Vec<(Hotkey, u16)>, pub alpha: Vec<(Hotkey, NetUid, u64)>, pub staking: Vec<(Coldkey, Vec<Hotkey>)>,
      pub owners: Vec<(Hotkey, Coldkey)>, pub names: Vec<(Coldkey, Option<String>)>, pub moving_price_bits: Vec<(NetUid, i128)>,
      pub tao_weight: u64, pub dividends: Vec<(NetUid, Hotkey, u64)>,
  }
  impl<S: BlockSource> Chain<S> { pub async fn validator_inputs(&self, at: BlockRef) -> Result<ValidatorInputs, ChainError> { todo!() } }
  async fn read_all_pages<S: BlockSource>(chain: &Chain<S>, at: BlockRef, prefix: Vec<u8>, page: u32) -> Result<Vec<(Vec<u8>, Vec<u8>)>, ChainError> { todo!() }
  ```

  `read_all_pages` takes the `Chain` and not the source, so each page goes through step 6's `Chain::call`.
- **Who writes what.** You write it. The agent records the fixture with `fixture record --read validators` and trims it. A whole snapshot is 56 to 117 MB and is never committed whole ([D34](../decisions.md#d34-keep-one-fixture-per-test-case-and-record-it-by-running-the-jobs-own-read)). The agent keeps a few keys per prefix, enough that the test's small page size still crosses several pages and ends on a short one, and reports the trimmed size. Confirm that the trimmed fixture still passes the test. The agent also records `fixture record --read delegates`, which reads `SubtensorModule.Delegates` only, at 8,636,190 (the upgrade block of spec 432, which spec 424 executed, so its storage decodes as `u16`) and at 8,636,191 (the first block spec 432 executed, `PerU16`). That pair is also a test of [D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec), since the upgrade block decodes with the old metadata. Each is small, since it holds one item. Review checks:
  - The loop ends only on a short page.
  - An error on any page fails the read, and no page is skipped.
  - Keys decode with `subxt::ext::frame_decode::storage::decode_storage_key`.
  - The two `Delegates` tests, `delegates_u16_at_424` and `delegates_at_432_first`, decode through `Chain` and `FixtureSource` into the same `u16` take, because `PerU16` wraps a `u16` and the key bytes do not change (`spikes/crates/chain-decode`, `era-peru16`, `netuid-type`).
- **Concepts.**
  - *Sequential async paging.* Each page's last key is the cursor for the next page, so the pages cannot run in parallel. Trap: a `Promise.all` over pages that you cannot list ahead of time. Source: subxt-rpcs docs, `LegacyRpcMethods::state_get_keys_paged`.
- **Check.** `cargo test -p bt-chain validators` passes `validators_head` offline, and `cargo test -p bt-chain delegates_` passes the two `Delegates` tests. `cargo run -p bt-indexer -- chain validators --block 9490000` prints `delegates=<n> pages=<n> calls=<n> elapsed=<s>`. At that block the snapshot has 8,127 delegates and takes 286 calls (140 key pages, 145 queries), in 451 s at 1 in flight and 131 s at 8 (`spikes/crates/chain-decode`, `validators-cost-head`). Write your numbers into the commit body.

## Step 22. `docs: list every shape change since spec 233`

- **Goal.** `bt-indexer metadata diff` compares the metadata of consecutive specs in `fixtures/specs.csv`, starting from the base row, spec 219, so the first comparison is 219 against 233, and prints each spec where an item we read or decode changed type ([Chain data](../design/02-chain-data.md#find-every-shape-change-before-writing-a-decoder)).
- **Files.** `crates/chain/src/diff.rs`, the `metadata diff` subcommand, `docs/shape-changes.md`, and `/.cache` added to `.gitignore`.
- **Brief for the agent.** For each item in `items.rs` and each event the design lists for release 2, compare the resolved shape between consecutive specs, meaning field names and primitive types, recursively, never type ids. Print `spec | item | change`. Cache each spec's metadata in the git-ignored `.cache/metadata/`, 0.2 to 0.45 MB per spec, so a second run makes no node calls.
- **Who writes what.** The agent writes it. You review it. Review checks:
  - Every change in the design's list of known shape changes appears.
  - A change the list lacks becomes a fixture task for the step that owns that item.
  - The output is sorted, so a second run produces no diff.

  Expect more event rows than the design lists. Event layouts change at 252, 273, 290, 320, 334, 365, 372, 411, and 423, and the diff shows which events change at each.
- **Concepts.** None new.
- **Check.** `cargo run -p bt-indexer -- metadata diff > docs/shape-changes.md` lists `StakeAdded` at 257 and `BasketSummary` at 464.

## Step 23. `docs(decisions): record how historic decoding works`

- **Goal.** Close roadmap M0. Every era in `docs/shape-changes.md` has a passing fixture test, and the decisions log ties each M0 claim to the test that checks it, as [the roadmap](../design/10-roadmap.md#m0-prove-historic-decoding-and-runtime-api-calls) asks.
- **Files.** `docs/decisions.md` (one new entry after the last one), any fixtures that step 22 found missing.
- **Who writes what.** You write the decision entry. D6, D7, and D8 state the rules. Review them against your tests. Leave those entries as they are, and add one new entry after the last one of the [decisions log](../decisions.md). The entry holds four things.
  - The tests behind each claim of D6 to D8. For D6 these are `prices_upgrade_block_4920350`, `prices_upgrade_block_9478217`, `upgrade_block_storage_metadata_9478217`, `parent_metadata_decodes_upgrade_block`, and `own_metadata_rejects_upgrade_block`.
  - Any row of `docs/shape-changes.md` that no entry covers.
  - Any place your results differ from D6 to D8.
  - Which fixtures release 1 exercises.

  The agent records any fixture that step 22 found missing, and reports fixture coverage per era. Review checks:
  - Each claim in D6 to D8 has a passing test named in the new entry.
  - D6 to D8 are unchanged.
  - Every row of `docs/shape-changes.md` maps to a test.
  - The new entry says which fixtures release 1 exercises.
- **Concepts.** None new.
- **Check (roadmap M0).** `unshare -rn cargo test -p bt-chain --offline` runs with no network and passes every `prices_*`, `epochs_*`, `baskets_*`, `identities_*`, `validators_*`, `delegates_*`, `upgrade_block_storage_*`, `version_table`, `parent_metadata_*`, and `own_metadata_*` test. From step 58 the same suite runs with `cargo nextest run -p bt-chain`, as the roadmap writes it.

## Stop here

- **Demo.** Turn the network off and run the decoder suite. It decodes prices in four eras, baskets in both shapes, epochs on both sides of spec 261, identities, and the validator inputs. `parent_metadata_decodes_upgrade_block` passes next to its negative twin.
- **Explain without notes.**
  - Why does the parent-metadata rule matter only at the upgrade block, and how do you find that block?
  - Why must `first_block` come from bisection and not from the first block a job reads?
  - Why do the decoders read raw bytes through `frame-decode` instead of building subxt's `StorageValue`?
  - Why does the decoder at 4,920,350 return no pools instead of a price of zero?
  - How does a new shape change get a struct and a fixture, from `metadata diff` to a passing test?
- **Roadmap.** Closes M0.
