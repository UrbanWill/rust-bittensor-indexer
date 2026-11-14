# Chain data

This document lists the chain state that release 1 reads and the events that release 2 decodes. It explains how one decoder handles every runtime version since dTAO, and where the chain's data model differs from what its names suggest.

Every `pallets/...:line` citation refers to subtensor at the spec 490 commit (`d967d1358`), unless the sentence names another commit. Line numbers drift with each runtime release, so find an item by name.

## Know the chain's model before reading its data

Subtensor is a Substrate chain with about 12-second blocks. These parts of its model matter to the indexer.

- **TAO** is the native token. One TAO is 10^9 **rao**. Every on-chain balance is a `u64` count of rao.
- **Subnets** have a `NetUid` (`u16`, `common/src/lib.rs:77`). Subnet 0 is **root**.
- Every subnet except root has its own token, **alpha**, also counted in units of 10^-9. Alpha amounts from two subnets are different assets. Adding them is a bug.
- A **coldkey** owns funds. A **hotkey** is the key that a validator or miner runs with. Stake belongs to a (coldkey, hotkey, subnet) triple.
- Staking TAO into a subnet is a swap. The TAO goes into the subnet's pool and the staker receives alpha. Unstaking is the reverse swap. The [price section](#price-comes-from-a-weighted-balancer-pool) describes the pool.
- Each subnet runs an **epoch** every `Tempo` blocks. The epoch computes incentives and dividends. Since spec 475, a subnet runs either Yuma consensus or Null consensus (`EpochConsensus` in `pallets/subtensor/src/lib.rs:627`). Null consensus uses the weights of the validator with the largest stake.
- **Root Reborn** (spec 441) added validator **baskets**. A basket is a fund of alpha holdings that root stakers own shares of. Root claims run alongside baskets, and `RootClaimed` fires on both sides of spec 441 (`pallets/subtensor/src/staking/claim_root.rs:1368`).

## Date each item by the block's metadata

The spec numbers in this document come from subtensor's git history. Each is the first of the 213 spec-bump commits on `main` that contains the item. Commit order is close to on-chain order, but mainnet did not run every spec. Where the first mainnet spec differs, it follows in parentheses. For example, the swap V3 storage is in the spec 283 commit, but the first mainnet runtime with it is 290. The squid has `src/types/v290.ts` and no `v283.ts`.

`spikes/data/specs.csv` holds the 106 specs that mainnet ran from dTAO to 2026-11-12, 233 to 490, with the upgrade block and date of each (`spikes/crates/chain-specs`, `spec-list`). Mainnet never ran 275, 283, 384, 390, 428, 460 or 461. The upgrade block of a spec is the first block whose state reports it, the `first_state_block` column of that file. The previous spec executed that block, and the new spec executes from the next one.

The jobs therefore never branch on a spec number alone. Each job asks the metadata cache whether the right metadata declares the storage item, runtime API, or event it needs. For storage and events at block `n`, that is the metadata of the spec that executed `n`. For a runtime API at `n`, it is the metadata of `n`'s own state spec. [Decode each block](#decode-each-block-with-the-metadata-of-the-runtime-that-ran-it) gives the rule. Metadata V15, which spec 233 already serves through `Metadata_metadata_at_version`, lists runtime APIs too. The spec numbers below say when to expect a change. The metadata decides.

## Read this chain state in release 1

Release 1 decodes no events. It runs five jobs that read storage and runtime APIs at scheduled blocks. [Ingestion](03-ingestion.md#run-every-job-on-one-runner) describes how the runner schedules them.

Release 1's windows reach back only to block 9,260,462, where the 31-day window starts at the head of 9,490,862 measured on 2026-11-12 (`spikes/crates/chain-specs`, `P29`). Release 1 therefore reads specs 475 to 490 and nothing older ([D3](../decisions.md#d3-set-the-milestone-scope-by-what-release-1-reads)). The `prices` window of 2 days starts at spec 488. The older eras below matter for release 2, and the fixtures of milestone M0 ([roadmap](10-roadmap.md)) test them for it.

| Job (`worker` label) | Schedule | History it keeps | Reads | Writes |
|---|---|---|---|---|
| `prices` | every block | `Window(2 days)` by default | the price (see [the eras](#price-comes-from-a-weighted-balancer-pool)), plus `SubnetTAO`, `SubnetTaoProvided` (specs 290 to 422), `SubnetAlphaIn`, `SubnetAlphaInProvided` (specs 290 to 422), `SubnetAlphaOut`, and `SubnetVolume` (`u128`) for every netuid in `NetworksAdded` except 0 | `subnet_price` |
| `validators` | blocks where `n % 100 == 0` | `Latest` | `Delegates`, `NetworksAdded`, `SubnetMovingPrice`, `TaoWeight`, `TotalHotkeyAlpha` (paged), `AlphaDividendsPerSubnet`, `StakingHotkeys` (paged), `Owner`, `IdentitiesV2` | `validator`, `validator_subnet` (the whole set is replaced in one transaction) |
| `epochs` | every block | `Window(31 days)` | `LastMechansimStepBlock` for every netuid. For subnets whose value equals `n`, also `AlphaDividendsPerSubnet` and `TotalHotkeyAlphaLastEpoch` (not declared before spec 261). Netuid 0 is skipped | `epoch_dividend` |
| `baskets` | blocks where `n % 500 == 0`, from spec 441 | `Window(31 days)` | `BetaBasketRuntimeApi.get_all_validator_baskets` and `BasketRate` | `basket_snapshot` |
| `identities` | blocks where `n % 100 == 0` | `Latest` | `SubnetIdentitiesV3` (declared from spec 283, 290 on mainnet, but see the [note on V3](#storage-items-and-runtime-apis)) | `subnet_identity` (changed rows are written and vanished rows deleted) |

[Compatibility numbers](08-analytics.md#compatibility-numbers) defines what the API computes from these rows.

### Storage items and runtime APIs

All storage items below are in `SubtensorModule` unless the table names another pallet. "From spec" is the first spec-bump commit that has the item, with the first mainnet spec in parentheses where the two differ. "Key" names the hasher. "Decodes as" is the Rust type that decoded the item through `DecodeAsType` in every mainnet era, 35 of 35 cases (`spikes/crates/chain-decode`, `decode-table`; `spikes/data/decode-cases.csv`). At its 8 upgrade blocks the table decodes with the executing spec's metadata, as the rule [below](#decode-each-block-with-the-metadata-of-the-runtime-that-ran-it) sets.

| Item | Declared at | Key | Value | Decodes as | From spec |
|---|---|---|---|---|---|
| `NetworksAdded` | `pallets/subtensor/src/lib.rs:2675` | `Identity(NetUid)` | `bool` | `bool` | 233 |
| `SubnetTAO` | `pallets/subtensor/src/lib.rs:2061` | `Identity(NetUid)` | `TaoBalance` (`u64`) | `u64` | 233 |
| `SubnetAlphaIn` | `pallets/subtensor/src/lib.rs:2103` | `Identity(NetUid)` | `AlphaBalance` (`u64`) | `u64` | 233 |
| `SubnetAlphaOut` | `pallets/subtensor/src/lib.rs:2108` | `Identity(NetUid)` | `AlphaBalance` (`u64`) | `u64` | 233 |
| `SubnetTaoProvided`, `SubnetAlphaInProvided` | `pallets/subtensor/src/migrations/migrate_cleanup_swap_v3.rs:11,16` | `Identity(NetUid)` | `u64` | `u64` | 283 (290) to 422 |
| `SubnetVolume` | `pallets/subtensor/src/lib.rs:2056` | `Identity(NetUid)` | `u128` | `u128` | 233 |
| `SubnetMovingPrice` | `pallets/subtensor/src/lib.rs:2034` | `Identity(NetUid)` | `I96F32` | `i128` (the bits) | 233 |
| `TaoWeight` | `pallets/subtensor/src/lib.rs:1742` | none | `u64`, default 971,718,665,099,567,868 | `u64` | 233 |
| `Delegates` | `pallets/subtensor/src/lib.rs:1795` | `Blake2_128Concat(hotkey)` | `u16` through spec 424, `PerU16` from spec 432 (block 8,636,190). Default 11,796 | `u16` | 233 |
| `TotalHotkeyAlpha` | `pallets/subtensor/src/lib.rs:2191` | (`Blake2_128Concat(hotkey)`, `Identity(NetUid)`) | `AlphaBalance` | `u64` | 233 |
| `TotalHotkeyAlphaLastEpoch` | `pallets/subtensor/src/lib.rs:2209` | (`Blake2_128Concat(hotkey)`, `Identity(NetUid)`) | `AlphaBalance` | `u64` | 261 |
| `AlphaDividendsPerSubnet` | `pallets/subtensor/src/lib.rs:1865` | (`Identity(NetUid)`, `Blake2_128Concat(hotkey)`) | `AlphaBalance` | `u64` | 233 |
| `StakingHotkeys` | `pallets/subtensor/src/lib.rs:2117` | `Blake2_128Concat(coldkey)` | `Vec<hotkey>` | `Vec<Hotkey>` | 233 |
| `Owner` | `pallets/subtensor/src/lib.rs:1771` | `Blake2_128Concat(hotkey)` | coldkey | `Coldkey` | 233 |
| `IdentitiesV2` | `pallets/subtensor/src/lib.rs:3301` | `Blake2_128Concat(coldkey)` | `ChainIdentityOfV2` | `ChainIdentityV2`, 7 `Vec<u8>` fields | 233 |
| `SubnetIdentitiesV3` | `pallets/subtensor/src/lib.rs:3306` | `Blake2_128Concat(NetUid)` | `SubnetIdentityOfV3` | `SubnetIdentityV3`, 8 `Vec<u8>` fields | 283 (290) |
| `LastMechansimStepBlock` | `pallets/subtensor/src/lib.rs:2783` | `Identity(NetUid)` | `u64` | `u64` | 233 |
| `BasketRate` | `pallets/subtensor/src/lib.rs:3674` | `Blake2_128Concat(hotkey)` | `I96F32` (`FixedI128 { bits: i128 }` in the registry) | `i128` (the bits) | 441 |
| `SwapRuntimeApi.current_alpha_price` | `pallets/swap/runtime-api/src/lib.rs:29` | `NetUid` argument | `u64` | `u64` | 283 (290) |
| `SwapRuntimeApi.current_alpha_price_all` | `pallets/swap/runtime-api/src/lib.rs:30` | none | `Vec<SubnetPrice>` | `Vec<{ netuid: u16, price: u64 }>` | 391 |
| `BetaBasketRuntimeApi.get_all_validator_baskets` | `pallets/subtensor/runtime-api/src/lib.rs:115` | none | `Vec<BasketSummary>` | see [baskets](#read-baskets-from-the-runtime-api) | 441 |

`Hotkey` and `Coldkey` are `struct Hotkey(pub [u8; 32])` and `struct Coldkey(pub [u8; 32])`. `NetUid` decodes as `u16`.

Specs through 424 key the subnet maps by a plain `u16`. From spec 432 (block 8,636,190) the map keys and the runtime API arguments use the `NetUid` newtype. Both encode the same bytes, so the storage keys do not change.

An item that the metadata in use does not declare is absent, and no decoder substitutes a default for it ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)). A row that requires an absent item is not written. A row for which the item is optional is written without it. Four items are optional. Without `SubnetTaoProvided` and `SubnetAlphaInProvided`, the reserve sum has no provided term. Without `SubnetEmaTaoFlow`, `ema_tao_flow_bits` is `NULL`. [Epochs](#detect-an-epoch-from-chain-state) covers `TotalHotkeyAlphaLastEpoch` before spec 261.

A declared item with no stored value for a key reads as the default in its metadata. subxt's own `fetch` does this (`spikes/crates/chain-errors`, `missing-storage`). The product decodes raw bytes through `frame-decode`, so our code applies the default. Two items here have nonzero defaults, `TaoWeight` and `Delegates`, so take each default from the metadata and never assume 0.

- For `Delegates`, `state_getStorage` answers null, and the metadata's default bytes decode to 11,796 (`spikes/crates/chain-decode`, `default-from-metadata`).
- `TaoWeight` is first stored at 4,920,351, and 20 samples from there to the head hold no null. At 4,920,350 its read answers null, and spec 219, which executed that block, does not declare it, so the block reads it as absent. The raw null path with spec 233's metadata decodes the default in the table (`spikes/crates/chain-decode`, `taoweight-null`).

`SubnetTaoProvided` and `SubnetAlphaInProvided` left the pallet at spec 423 (block 8,486,593). The migration `migrate_cleanup_swap_v3` moved their values into `SubnetTAO` and `SubnetAlphaIn` (`pallets/subtensor/src/migrations/migrate_cleanup_swap_v3.rs:39-48`), so from spec 423 the reserves are the two plain items. Their keys outlive them. After 423 the chain still holds 129 keys of each, all zero (`spikes/crates/chain-specs`, `P31-423`). A reader that iterates by prefix sees these stale keys, so read the provided items only by key, and only while the executing spec's metadata declares them.

`SubnetIdentitiesV3` is declared from spec 290 (block 5,947,548) but holds 0 rows at 5,947,549, while `SubnetIdentitiesV2` holds 112. By spec 352, V3 holds 121 rows and V2 holds 2. Release 1 reads V3 at specs 475 to 490, where it holds the data (121 identities against 128 subnets at 9,490,000). A release 2 identity read at an older block must not pick V3 because it is declared.

### Read every netuid in one request

The `prices` job computes the storage keys for every netuid and every item once. It reads all of them for a block with one `state_queryStorageAt` call. `Timestamp.Now` goes in the same call, so every job gets the block's time without a second request. The job refreshes the netuid set from `NetworksAdded` every 100 blocks, and on any block where the price API returns a netuid it does not know. With the price call and the block hash, that is 3 node requests and 54 KB per block at spec 490, and 2 requests at spec 234 (measured on 2026-11-12, `spikes/crates/chain-decode`, `out/cost.txt`). [Ingestion](03-ingestion.md#estimate-the-backfill-time) builds its estimate on that cost.

### Price comes from a weighted balancer pool

The pool was a Uniswap-v3 style pool, with `AlphaSqrtPrice` holding the price as `U64F64` (key `Twox64Concat(u16)`). Spec 423 (block 8,486,593) replaced it with a weighted balancer (`pallets/swap/src/pallet/migrations/migrate_swapv3_to_balancer.rs:25`). The current price is `Balancer::calculate_price(x, y)` in `pallets/swap/src/pallet/balancer.rs:220`, from the subnet's alpha and TAO reserves.

The `prices` job uses the runtime API wherever it exists, and storage math only before it. A runtime API call at block `n` runs the code of `n`'s own state spec, so choose the era by what that spec's metadata declares.

1. From spec 391 (block 7,782,857), call `SwapRuntimeApi.current_alpha_price_all` once per block.
2. From spec 283 (290 on mainnet, block 5,947,548) to spec 390 (385 on mainnet), call `SwapRuntimeApi.current_alpha_price(netuid)` once per subnet.
3. Before spec 283 (290 on mainnet), compute `SubnetTAO × 10^9 / SubnetAlphaIn` from storage, decoded with the executing spec's metadata. The provided items do not exist yet, so the reserves have no provided term.

At an upgrade block the new code runs over storage that the migration has not touched yet (`spikes/crates/chain-specs`, `P31`, `P31-290`, `P31-423`). At 5,947,548 and 8,486,593 the price API gives prices continuous with the next block. Storage math is different. At block 4,920,350, the dTAO items hold 0 keys while `NetworksAdded` holds 65, and spec 219, which executed that block, does not declare them. Read as zeros, they would give a zero price for every subnet. They are absent ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)), so `prices` writes no row for a subnet whose `SubnetTAO` or `SubnetAlphaIn` is not declared. Release 1 reads only one upgrade block in its `prices` window, 9,478,217 (spec 490), where the API path applies.

Both runtime API calls return rao per whole alpha, which is the unit `subnet_price.price` stores. The runtime computes the price in `current_price` (`pallets/swap/src/pallet/impls.rs:22-35`). It returns 10^9, a price of 1, for any subnet whose mechanism is not 1. It returns 0 for a mechanism-1 subnet whose alpha reserve is 0. `current_alpha_price_all` also returns root (netuid 0) at 10^9, one of its 129 entries at the measured head. The `prices` job drops that entry, so root never gets a price row.

The squid computed prices for specs 290 to 422 from `AlphaSqrtPrice` with integer math, while the runtime API uses its own fixed-point path. Our historic prices for those specs can therefore differ from the squid's stored values by 1 rao. `/pools` reads only the last 26 hours, so no compat value depends on them. Decision [D10](../decisions.md#d10-read-prices-from-the-runtime-api-not-from-v3-storage-math) records the trade.

In release 2, `prices` keeps full history for candles. Specs 290 to 390 then cost one runtime API call per subnet per block, 128 calls at the measured block, and 131 calls per block with the 3 fixed calls. Over blocks 5,947,548 to 7,782,856 the 128 price calls alone come to (7,782,856 − 5,947,548 + 1) × 128 = 234,919,552 calls, at least about 23 days at the 120 calls a second cap ([D23](../decisions.md#d23-cap-the-call-rate-with-a-one-token-bucket-not-only-the-calls-in-flight)). [Ingestion](03-ingestion.md#estimate-the-backfill-time) uses that cost.

### Detect an epoch from chain state

A block is an epoch block for a subnet when `LastMechansimStepBlock` for that subnet equals the block number. The name has a typo on chain. Use it as written. Recent runtimes can run an epoch early or defer it (`EpochTriggered` and `EpochDeferred` at `pallets/subtensor/src/macros/events.rs:682,692`), so the schedule must come from chain state, not from a formula on `Tempo`.

At an epoch block, the `epochs` job reads `AlphaDividendsPerSubnet` for that netuid by key prefix and keeps the hotkeys with a nonzero dividend. It reads `TotalHotkeyAlphaLastEpoch(hotkey, netuid)` for each of them as `stake_at_epoch`. Before spec 261 (block 5,328,895) that item is not declared, and the decoder returns `None` for it. Release 1 never reads that era, so `stake_at_epoch` is `NOT NULL`, and release 2 chooses what to store for it.

Measured on 2026-11-12, an epoch block holds about 50 dividend rows, and the job costs 3.0 calls and 23 KB per block at 9,490,000.

### Read baskets from the runtime API

`get_all_validator_baskets` returns one `BasketSummary` per validator with an active basket (`pallets/subtensor/src/rpc_info/basket_info.rs:30-44`). The `baskets` job keeps `hotkey`, `nav_tao`, and `shares` (a `u64`). It reads `BasketRate(hotkey)` for each returned hotkey in the same block.

The summary has two shapes. Specs 441 to 459 carry a `weights: Vec<(NetUid, u16)>` field between `redeemed_tao` and `holdings`. The spec 461 commit (`7c9d45ebd`) removed it. Mainnet ran 459 and then 464 (block 9,088,597), so 464 is the first mainnet spec without the field.

One struct without `weights` decodes both shapes, because `DecodeAsType` matches fields by name and skips a field our struct does not have. It decoded specs 441 to 490, including 21 nonempty `weights` at block 9,088,596. At 448 and 459 it gave every field equal to the struct with `weights`, for 179 and 187 baskets (`spikes/crates/chain-decode`, `basket-one-struct`).

```rust
#[derive(DecodeAsType)]
struct BasketSummary { hotkey: Hotkey, nav_tao: u64, spot_nav_tao: u64, shares: u64, deposited_tao: u64, redeemed_tao: u64, holdings: Vec<BasketHolding> }

#[derive(DecodeAsType)]
struct BasketHolding { netuid: u16, alpha: u64, spot_tao: u64, realizable_tao: u64 }
```

Release 1 reads only the shape without `weights`. One `get_all_validator_baskets` call took 8.7 to 33.5 s near the head on 2026-11-12, depending on node load, and a snapshot of about 190 baskets cost 3 calls and about 444 KB (`spikes/crates/chain-decode`, `basket-call-time`, `baskets-cost`). So `chain.rpc_timeout` is 90 s ([D23](../decisions.md#d23-cap-the-call-rate-with-a-one-token-bucket-not-only-the-calls-in-flight)).

The squid takes baskets every 500 blocks. Keep that cadence, because the root APR formula and its 10-snapshot rule were tuned on it.

### Decode identity bytes the way the squid does

`IdentitiesV2(coldkey).name` and every `SubnetIdentitiesV3` field are byte strings. Decode each as UTF-8, replacing invalid sequences with U+FFFD, remove all NUL characters, and trim. An empty result is `NULL`. The squid does the same in `src/bytes.ts:2-16`, and the compat routes return its strings.

`SubnetIdentityOfV3` names three fields differently from the API. `subnet_name` becomes `name`, `subnet_url` becomes `url`, and `subnet_contact` becomes `contact`.

## Read subnet snapshots in release 2

Release 2 adds one more state job, `subnets`, for the native analytics. It runs at blocks where `n % 100 == 0` with `Full` history, and writes `subnet_snapshot`. The squid sampled at the same blocks, and its 24-hour emission figure is 100 times the sampled sum, so keep the cadence.

It calls `SubtensorRuntimeApi.get_all_dynamic_info` (`pallets/subtensor/runtime-api/src/lib.rs:58`). It then reads the items that the API lacks, for every netuid in `NetworksAdded` except 0.

- `SubnetMovingPrice` (`pallets/subtensor/src/lib.rs:2034`) and `Tempo` (`pallets/subtensor/src/lib.rs:2612`).
- `SubnetEmaTaoFlow` (`pallets/subtensor/src/lib.rs:2387`, from spec 334).
- `SubnetTaoInEmission`, `SubnetAlphaInEmission`, `SubnetAlphaOutEmission`, `SubnetExcessTao`, and `SubnetEmissionEnabled` (`pallets/subtensor/src/lib.rs:2066-2093`).
- `SubnetVolume` (`pallets/subtensor/src/lib.rs:2056`).
- From the swap pallet, `FeeRate` (`pallets/swap/src/pallet/mod.rs:97`) and, from spec 423, `SwapBalancer` (`pallets/swap/src/pallet/mod.rs:110`).

Read each storage item only when the executing spec's metadata declares it, and call the runtime API only when the block's state spec declares it.

## Decode each block with the metadata of the runtime that ran it

Each runtime version has its own metadata. The metadata holds a type registry that says how every event, storage value, and runtime API result is encoded. A decoder built for spec 490 reads spec 233 bytes wrong, or fails.

The upgrade block `u` of a spec still runs the old runtime, and its post-state reports the new one. Each block therefore has two specs, and `SpecAt { executed_by, state }` keeps both. `BlockRef` carries it. `executed_by` is the state spec of the parent, and `state` is the spec the block's own state reports. They differ only at an upgrade block. Two rules follow ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)).

- **Decode storage and events with the executing spec.** The storage and events of block `n` decode with the metadata of `executed_by`. At `u` the storage is still in the old layout. `System.LastRuntimeUpgrade` still names the old spec, and items the new spec adds have 0 keys until `u + 1`. Key counts show this at 4,920,350, 5,947,548 and 8,486,593 (`spikes/crates/chain-specs`, `P31`, `P31-290`, `P31-423`). The parent's metadata decodes the events of all 106 upgrade blocks with no bytes left over, and the block's own metadata fails at 13 of them (`spikes/crates/chain-specs`, `D-scan`).
- **Call runtime APIs with the state spec.** A runtime API call at `n` runs the node's code for `n`'s state, which is the new code at `u`. Encode its arguments and decode its result with the metadata of `state`.

Decision [D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec) lists the checks behind both rules. They cover every item release 1 reads at the 10 upgrade blocks of its window, sampled maps at 7 older upgrade blocks, and the 35 cases of the decode table.

### Fetch metadata through the runtime API

Fetch metadata with `state_call("Metadata_metadata_at_version", SCALE(v: u32), hash)`. The result decodes as `Option<Vec<u8>>` holding `RuntimeMetadataPrefixed`. `v` is the highest value that `Metadata_metadata_versions` returns at the same hash, after dropping `4294967295`. That is 16 for specs 326 to 490. subxt chooses the same way. V16 decodes the same cases as V15 with identical output, and gives the same shapes for the 48 recorded specs from 326 to 490 (`spikes/crates/chain-decode`, `metadata-v16`).

For the executing spec, call at the parent hash. For the state spec, call at the block's own hash ([D7](../decisions.md#d7-fetch-metadata-with-metadata_metadata_at_version)). `BlockRef` holds no parent hash, so on a cache miss `Chain::metadata` first calls `source.block_ref(n − 1)` ([D34](../decisions.md#d34-keep-one-fixture-per-test-case-and-record-it-by-running-the-jobs-own-read)).

One fetch took 0.9 to 8.8 s per spec for 400 to 445 KB, measured for specs 475, 482 and 490 (`spikes/crates/chain-errors`, `metadata-fetch-time`). A metadata file is 0.2 to 0.45 MB.

Do not use `state_getMetadata`. It returns V14, which lists no runtime APIs, so the metadata could not say whether an API exists (`spikes/crates/chain-specs`, `P4`). `Metadata_metadata_at_version(15)` lists 19 runtime APIs at spec 233 and 26 at spec 490. The versions on offer by era:

| Mainnet specs | `Metadata_metadata_versions` |
|---|---|
| 219 to 277 | `[14, 15]` |
| 290 to 323 | `[14, 15, 4294967295]` |
| 326 to 490 | `[14, 15, 16]` |

### Find each spec's upgrade block by bisection

The `runtime_version` table maps each spec to `first_block`, its upgrade block. The spec executes from `first_block + 1`. So `executed_by(n)` is the spec of the last row with `first_block < n`, and `state(n)` is the spec of the last row with `first_block <= n` ([D8](../decisions.md#d8-find-the-spec-table-by-bisection-over-code-and-persist-only-the-specs-a-job-window-touches)).

The table starts with a base row: spec 219 at 4,920,349, the spec in force at the start block. It makes `executed_by(4920350)` answer 219. With no row below 4,920,350, `OfflineClient::at_block(4920350)` fails with `SpecVersionNotFound` (`spikes/crates/chain-errors`, `metadata-missing`). 219's true upgrade block is earlier and is not needed.

The table is found by bisection, not learned when a reader first meets a spec. A reader that skips blocks meets a spec late. One that reads every 100th block first meets 490 at 9,478,300, but 490's upgrade block is 9,478,217 (`spikes/crates/chain-specs`, `P5`).

1. Bisect over `state_getStorageHash(":code")`. A change of the hash between two blocks means an upgrade between them.
2. Confirm each boundary with `state_getRuntimeVersion` at `n − 1` and `n`.
3. At each range end and at each new head, one `:code` hash check rules out a new spec. Upgrades can come 2 blocks apart (4,920,350 and 4,920,352), so nothing coarser does (`spikes/crates/chain-specs`, `P30`).

The whole bisection from dTAO to 2026-11-12 took 3,222 calls and about 1 minute on the keyed archive node, plus about 11 minutes of version checks. It found 106 specs, 233 to 490. On a warm node one `:code` hash took 0.28 to 0.35 s and one runtime version 0.28 to 1.46 s. A bisection over `state_getRuntimeVersion` at 6 concurrent calls found no boundary in 23 minutes (`spikes/crates/chain-specs`, `bisect-method`).

`bt-indexer chain specs --from 4920349` writes the table to `fixtures/specs.csv`, the product's one committed spec file. Its rows are the 219 base row and the rows of `spikes/data/specs.csv`. The product seeds the table from `fixtures/specs.csv` and bisects only past its last row.

`chain` may not depend on `store`, so a read that finds a new spec returns a `#[must_use] NewRuntime`, and `ingest` stores it (`spikes/crates/chain-specs`, `P6`, `P7`). The table lives in memory in M0 and is persisted in `runtime_version` from M1. The in-memory table holds every row of `fixtures/specs.csv`. `runtime_version` gets a row, with its metadata, only for each spec that a job window touches, the first time a job needs that spec ([D8](../decisions.md#d8-find-the-spec-table-by-bisection-over-code-and-persist-only-the-specs-a-job-window-touches)). Each spec's metadata loads from the table into the in-memory cache on first use.

### Give subxt the spec and metadata

Storage values and runtime API results decode from raw bytes through `frame-decode`, with a `subxt::Metadata` that we pick by the rules above. subxt's `StorageValue::new` is `pub(crate)`, so subxt cannot decode storage bytes we fetched ourselves (`spikes/crates/chain-decode`, `P19`). One subxt `Config` hook cannot serve both rules either. subxt asks it for one spec per block number and uses that spec's metadata for both events and storage. At 5,228,683 the hook answers 252 while the state reports 257 (`spikes/crates/chain-specs`, `P1`).

The hook therefore serves events only, in release 2, and answers `executed_by`. It implements all 7 associated types. `TransactionExtensions` is `SubstrateExtrinsicParams<Self>`, and the others delegate to `SubstrateConfig`. Its maps sit behind `Arc<RwLock<_>>` because `set_metadata_for_spec_version` takes `&self`.

Events decode only through `OfflineClient`, with the executing spec's metadata registered through `set_metadata_for_spec_version` before `at_block`. An `OnlineClient` fetches missing metadata itself, at the block's own hash, and files it under the hook's spec (`subxt-0.50.3/src/client/online_client.rs:397-403,604-617`). At an upgrade block that stores the new spec's metadata under the old spec's number, and the old spec's ordinary blocks then fail to decode (`spikes/crates/chain-specs`, `online-metadata-miss`).

```rust
#[derive(Clone, Debug, Default)]
pub struct Subtensor {
    versions: Arc<RwLock<BTreeMap<u64, u32>>>,
    metadata: Arc<RwLock<HashMap<u32, ArcMetadata>>>,
}

impl Config for Subtensor {
    type AccountId = <SubstrateConfig as Config>::AccountId;
    type Address = <SubstrateConfig as Config>::Address;
    type Signature = <SubstrateConfig as Config>::Signature;
    type Hasher = <SubstrateConfig as Config>::Hasher;
    type Header = <SubstrateConfig as Config>::Header;
    type TransactionExtensions = SubstrateExtrinsicParams<Self>;
    type AssetId = <SubstrateConfig as Config>::AssetId;

    fn spec_and_transaction_version_for_block_number(&self, n: u64) -> Option<(u32, u32)> {
        // Keyed by first_block. The spec that executed n is the last one upgraded before n.
        self.versions.read().unwrap().range(..n).next_back().map(|(_, s)| (*s, 1))
    }

    fn metadata_for_spec_version(&self, spec: u32) -> Option<ArcMetadata> {
        self.metadata.read().unwrap().get(&spec).cloned()
    }

    fn set_metadata_for_spec_version(&self, spec: u32, metadata: ArcMetadata) {
        self.metadata.write().unwrap().insert(spec, metadata);
    }
}
```

At upgrade block 5,228,683, executed by 252 with state 257, this hook decodes the block's 4 `StakeAdded` events with 252's metadata through `OfflineClient`. With 257's metadata the same bytes fail (`spikes/crates/chain-specs`, `subtensor-config`).

### Decode into era structs

Decode each event's fields into a struct that derives `scale_decode::DecodeAsType`. `DecodeAsType` matches our struct against the type in that spec's registry. It does not assume one fixed layout. Each event struct holds one era's shape, and a `From` conversion turns it into the domain type. The event decoders call subxt 0.50.3's `decode_fields_unchecked_as::<T>()` ([D22](../decisions.md#d22-classify-chain-errors-by-what-the-node-sends-and-bound-the-clients-waits-with-our-own-timeouts)). The match is on the executing spec, never on the block's state spec.

```rust
#[derive(DecodeAsType)]
struct StakeAddedV233(Coldkey, Hotkey, u64, u64, u16);

#[derive(DecodeAsType)]
struct StakeAddedV257(Coldkey, Hotkey, u64, u64, u16, u64);

#[derive(DecodeAsType)]
struct BasketClaimedV441 { hotkey: Hotkey, coldkey: Coldkey, tao: u64 }

fn stake_added(raw: &RawEvent, executed_by: SpecVersion) -> Result<StakeAdded, DecodeError> {
    match executed_by.0 {
        ..257 => raw.decode_fields_unchecked_as::<StakeAddedV233>().map(StakeAdded::from),
        _ => raw.decode_fields_unchecked_as::<StakeAddedV257>().map(StakeAdded::from),
    }
}
```

Era structs use `Coldkey` and `Hotkey`, never a bare account type. Tuple stake events put the coldkey first (`pallets/subtensor/src/macros/events.rs:17-24`), while basket events are structs that put the hotkey first (`pallets/subtensor/src/macros/events.rs:514-521`). With bare accounts, a `From` that swaps them compiles. With the newtypes, the compiler rejects it.

`Coldkey` and `Hotkey` wrap `[u8; 32]` directly and derive `DecodeAsType`. The chain's `AccountId32` is a composite with one `[u8; 32]` field, so the depths match. Keep that one layer.

Amounts and netuids decode as plain `u64` and `u16`. `TaoBalance`, `AlphaBalance`, and `NetUid` are newtypes in the type registry from spec 432 (block 8,636,190), with the same values, and a plain integer decodes from them in every case of the decode table. The `From` conversion wraps the integers in our newtypes, so the decode structs need only the plain integers in every era.

The match on `spec` lists every shape change in one place. The reference squid instead generated types for about 60 runtime versions and checked each with `.v233.is()` calls. Our approach needs one struct per real shape change, which is two for `StakeAdded`.

The `runtime_version` table stores the raw metadata bytes for every spec a job has needed. A decode bug fix then re-decodes old blocks without fetching metadata from the node again.

### Find every shape change before writing a decoder

Do not rely on release notes to find shape changes. Build a small tool, `bt-indexer metadata diff`. It loads the metadata for every spec from 219, which executed block 4,920,350, to the current one and prints each spec where an item we read or decode changed type. Run it whenever the chain upgrades, and add a struct for each change it reports. A decoder test then decodes a recorded block from each era. [Quality](09-quality.md#test-the-decoders-against-recorded-blocks) describes those tests.

Known shape changes since spec 233. Each block number is the mainnet upgrade block from `spikes/data/specs.csv`.

- `StakeAdded` and `StakeRemoved` gained the fee field at spec 257 (block 5,228,683).
- `TotalHotkeyAlphaLastEpoch` appeared at spec 261 (block 5,328,895).
- The swap V3 storage, `SwapRuntimeApi.current_alpha_price`, the two provided items, and `SubnetIdentitiesV3` appeared at spec 283 (290 on mainnet, block 5,947,548).
- `SubnetIdentities`, `SubnetIdentitiesV2`, and the coldkey `Identities` left at spec 361 (block 7,063,679).
- `ColdkeySwapped` dropped its `swap_cost` field at spec 385 (block 7,782,670).
- `SwapRuntimeApi.current_alpha_price_all` appeared at spec 391 (block 7,782,857).
- The swap V3 storage and the provided items left at spec 423 (block 8,486,593), when the pool changed to a balancer.
- `Delegates` changed from `u16` to `PerU16` at spec 432 (block 8,636,190), which encodes the same. Map keys, runtime API arguments, and event amounts and netuids became newtypes in the registry at the same spec, with the same bytes.
- Basket storage and the basket runtime API appeared at spec 441 (block 8,765,683). `RootClaimed` gained a `tao` field at the same spec.
- `BasketSummary` lost `weights` at spec 461 (464 on mainnet, block 9,088,597).

At 13 upgrade blocks the parent's metadata and the block's own metadata disagree on the events. They are the upgrade blocks of specs 233, 252, 257, 273, 290, 320, 334, 365, 372, 385, 411, 423, and 441 (`spikes/crates/chain-specs`, `D-scan`). `bt-indexer metadata diff` names the events that changed at each, before release 2 writes a decoder for them.

### Handle an unknown event

The `events` job skips an event it does not index by pallet and variant name before decoding, so the event costs almost nothing. An event we index that fails to decode stops the process. [Error handling](06-error-handling.md#classify-errors-in-one-place) explains why.

## Index these events in release 2

Release 2 adds the `events` job. Indexing starts at block 4,920,350. That is the upgrade block to spec 233, and spec 219 executed it, so its events decode with 219's metadata (`spikes/crates/chain-specs`, `P2`). The tables list each event, where subtensor defines it, and the specs where its shape changed. Event lines refer to `pallets/subtensor/src/macros/events.rs`.

### SubtensorModule (pallet index 7)

| Event | Current fields | Shape history from spec 233 | Effect in the indexer |
|---|---|---|---|
| `StakeAdded` (line 17) | coldkey, hotkey, `TaoBalance`, `AlphaBalance`, `NetUid`, fee `u64` | 233 has five fields. 257 adds the fee | A staking action. Adds alpha to a position |
| `StakeRemoved` (line 26) | the same as `StakeAdded` | the same as `StakeAdded` | A staking action. Removes alpha from a position |
| `StakeMoved` (line 35) | coldkey, origin hotkey, origin netuid, destination hotkey, destination netuid, `TaoBalance` | none | A staking action that links two legs |
| `StakeTransferred` (line 297) | origin coldkey, destination coldkey, hotkey, origin netuid, destination netuid, `TaoBalance` | none | A staking action that links two legs |
| `StakeSwapped` (line 310) | coldkey, hotkey, origin netuid, destination netuid, `TaoBalance` | none | A staking action that links two legs |
| `AlphaRecycled` (line 334) | coldkey, hotkey, `AlphaBalance`, `NetUid` | from 257 | Removes alpha from a position |
| `AlphaBurned` (line 340) | coldkey, hotkey, `AlphaBalance`, `NetUid` | from 257 | Removes alpha from a position |
| `HotkeySwapped` (line 171) | struct of coldkey, old hotkey, new hotkey | none | Reconciles every coldkey with a position on either hotkey |
| `HotkeySwappedOnSubnet` (line 373) | struct of coldkey, old hotkey, new hotkey, netuid | from 275 (276 on mainnet) | The same, on that netuid only |
| `ColdkeySwapped` (line 196) | struct of old coldkey, new coldkey | 233 to 384 (377 on mainnet) add `swap_cost: u64` | Reconciles both coldkeys |
| `RootClaimed` (line 472) | struct of coldkey, `tao` | 334 to 440 have only `coldkey`. 441 adds `tao` | Reconciles the coldkey |
| `NetworkAdded` (line 13) | `NetUid`, mechanism `u16` | none | Opens a `subnet` row |
| `NetworkRemoved` (line 15) | `NetUid` | none | Marks the subnet removed, and reconciles its holders |
| `NetworkDissolveCleanupCompleted` (line 586) | struct of `NetUid` | from 428 (432 on mainnet) | Reconciles the subnet's holders again |
| `BasketDeposited` (line 483) | hotkey, tao, shares | from 441 | A basket flow |
| `BasketStakedIn` (line 497) | hotkey, coldkey, tao, value, shares | from 441 | A basket flow |
| `BasketClaimed` (line 514) | hotkey, coldkey, tao | from 441 | A basket flow. Adds the TAO to the coldkey's root position |
| `BasketHoldingConverted` (line 525) | hotkey, netuid, tao | from 441 | A basket flow |

The sixth `StakeAdded` and `StakeRemoved` field is the swap fee. The emit site at `pallets/subtensor/src/staking/stake_utils.rs:1418` passes `swap_result.fee_paid`. The reference squid calls the field `nonce`. Use the subtensor name.

### Balances

`Balances.Transfer` (from, to, amount) creates a `transfer` row. The indexer reads no other balance event and stores no balances.

### Swap (specs 283 to 422)

The swap pallet declared three liquidity events from spec 283 (290 on mainnet) to spec 422. Its extrinsics emitted them, and moved stake with `T::BalanceOps::decrease_stake` and `increase_stake`, until spec 411 (`pallets/swap/src/pallet/mod.rs:367,375,479,486` at commit `e9bbb6134`). From spec 411 the extrinsics return `UserLiquidityDisabled` or do nothing. From spec 423 the pallet declares only `FeeRateSet` (`pallets/swap/src/pallet/mod.rs:137-140`).

| Event | Fields | Effect in the indexer |
|---|---|---|
| `LiquidityAdded` | coldkey, hotkey, netuid, position id, liquidity `u64`, tao, alpha, tick low, tick high | Reconciles the coldkey |
| `LiquidityRemoved` | the same, plus `fee_tao` and `fee_alpha` before the ticks | Reconciles the coldkey |
| `LiquidityModified` | coldkey, hotkey, netuid, position id, liquidity `i64`, tao `i64`, alpha `i64`, `fee_tao`, `fee_alpha`, tick low, tick high | Reconciles the coldkey |

Field lists are from commit `e367ae647` (spec 422), lines 193 to 266 of `pallets/swap/src/pallet/mod.rs`. The indexer reads only the coldkey and the netuid, so a reorder of the other fields does not affect it.

### Events for later releases

The reference squid does not index these events. They support analytics that the [roadmap](10-roadmap.md) lists after release 2.

- `NeuronRegistered` (line 49) and `BulkNeuronsRegistered` (line 51) for registration and churn.
- `WeightsSet` (line 44), `WeightsCommitted` (line 263), `WeightsRevealed` (line 270), and `TimelockedWeightsCommitted` (line 422) for how often validators set weights.
- `EpochTriggered`, `EpochDeferred`, and `EpochSkipped` (lines 682 to 702) for epoch timing.
- `EpochConsensusSet` (line 896) for the switch between Yuma and Null consensus.
- `SubnetOwnerChanged` (line 710) and `DissolveNetworkScheduled` (line 238) for the subnet lifecycle.
- `LimitOrders.OrderExecuted` from `pallets/limit-orders/src/lib.rs:417`.

## Watch for these traps in the data

These traps cost the reference squid several bug fixes. Each one has a rule in `domain` and a test.

### Alpha storage holds shares, not alpha

`AlphaV2` (`pallets/subtensor/src/lib.rs:2234`) maps (hotkey, coldkey, netuid) to a `SafeFloat { mantissa: u128, exponent: i64 }`. The value is the coldkey's share of the hotkey's pool, not an amount of alpha. To get alpha, multiply the share by `TotalHotkeyAlpha` and divide by `TotalHotkeySharesV2`. Read positions through `StakeInfoRuntimeApi.get_stake_info_for_coldkeys` (`pallets/subtensor/runtime-api/src/lib.rs:82`) instead. It returns alpha.

That API skips every position whose alpha is 0 (`pallets/subtensor/src/rpc_info/stake_info.rs:69-70`). A position that moved away therefore vanishes from the result instead of reading 0. A settle must zero every position of the coldkey that the result does not list.

### Moves, transfers, and swaps emit two legs

`StakeMoved`, `StakeTransferred`, and `StakeSwapped` carry a TAO amount but no alpha amount. The chain emits a `StakeRemoved` for the origin and a `StakeAdded` for the destination before each of them. Positions change from the two legs. The summary event only links them. Counting both the legs and the summary counts the move twice.

The fee on a leg depends on the subnets.

- A move within one subnet passes a fee of 0 on both legs (`pallets/subtensor/src/staking/stake_utils.rs:1612,1620`). It does not trade on the pool, so it adds nothing to pool volume.
- A move across subnets trades on both pools and pays one fee. The origin leg drops its fee only when the origin is root, and the destination leg drops its fee otherwise (`pallets/subtensor/src/staking/move_stake.rs:509-510`).
- If the TAO from the origin leg is below the minimum stake, the chain does not restake it, so only the `StakeRemoved` leg exists (`pallets/subtensor/src/staking/move_stake.rs:528-549`).

### The swap pallet emits only FeeRateSet from spec 423

From spec 423, `pallets/swap` declares only `FeeRateSet`. Before that, the liquidity events also moved stake, as the [swap event table](#swap-specs-283-to-422) shows. Swap amounts appear in `StakeAdded` and `StakeRemoved`. They do not give pool volume, because many swaps emit no stake event. The [volume trap](#volume-comes-from-subnetvolume) explains.

### A hotkey swap carries no amounts, and moves every nominator

`HotkeySwapped` names the owner's coldkey and the two hotkeys, but not the stake that moved. The swap moves every (coldkey, netuid) position on the old hotkey (`pallets/subtensor/src/swap/swap_hotkey.rs:121`), not only the owner's. The position folder therefore reconciles every coldkey with a position on the old or the new hotkey. For `HotkeySwappedOnSubnet`, it limits that to the event's netuid.

### Coldkey swaps and root claims move stake with no stake event

`swap_coldkey.rs` and `claim_root.rs` emit no `StakeAdded` or `StakeRemoved`. A coldkey swap moves every position to the new coldkey. A root claim adds stake with `increase_stake_for_hotkey_and_coldkey_on_subnet` (`pallets/subtensor/src/staking/claim_root.rs:344`) and emits only `RootClaimed`. The folder reconciles the coldkeys these events name. The new coldkey of a swap has no position row yet, and the reconcile creates it.

Emissions also grow stake with no event. Every coldkey that gains stake by any path is added to `StakingHotkeys` (`pallets/subtensor/src/staking/stake_utils.rs:830-841`). The daily reconcile therefore takes its coldkeys from the keys of `StakingHotkeys`. That set includes coldkeys that hold only emissions, which have no `position` row.

### Removing a subnet takes two steps

At spec 490, `NetworkRemoved` marks the subnet `PendingDissolution` (`pallets/subtensor/src/subnets/dissolution.rs:194,205`). The stake stays until a later block clears the subnet and emits `NetworkDissolveCleanupCompleted` (`pallets/subtensor/src/subnets/dissolution.rs:1108`). Earlier runtimes cleared the stake in the same call as `NetworkRemoved`.

The folder reconciles the subnet's holders at both events. Whatever the chain returns at that block is correct for either era. A new subnet takes the lowest netuid that is neither live nor waiting for cleanup (`pallets/subtensor/src/subnets/subnet.rs:152-165`), so a netuid is reused after cleanup. `subnet` is therefore keyed by `(netuid, registered_block)`.

### Volume comes from SubnetVolume

`SubnetVolume` is a lifetime counter of TAO swapped through a subnet's pool. Both swap functions add to it (`pallets/subtensor/src/staking/stake_utils.rs:1034,1087`). Many callers swap without a stake event, such as emission, basket, and root-claim code. Volume over a window is therefore the difference of two `SubnetVolume` readings, not a sum of stake events. The `prices` job stores the counter with each price as `lifetime_volume`.

### Fixed-point values use different scales

| Item | Type | Convert with |
|---|---|---|
| `SubnetMovingPrice`, `BasketRate` | `I96F32` | `I96F32::from_bits(raw)` |
| `SubnetEmaTaoFlow` (`pallets/subtensor/src/lib.rs:2387`) | `Option<(u64, I64F64)>`, the block of the last update and the EMA | `I64F64::from_bits(raw)` on the second field, truncated to rao |
| `AlphaSqrtPrice` (removed at spec 423) | `U64F64` | `U64F64::from_bits(raw)` |
| `TaoWeight` | `u64` | divide by `u64::MAX` |
| `Delegates` (take) | `u16` | divide by 65,535 |

Decode each of these as its raw integer and convert it with the `fixed` crate. Never pass them through `f64` in the write path. `f64` loses precision above 2^53, and the `I96F32` values pass that limit.

### Basket holdings count toward TotalHotkeyAlpha

`TotalHotkeyAlpha` includes the alpha that baskets hold. Release 1 serves it as the squid does and does not subtract the holdings.

## Port a few domain types from subtensor

The `domain` crate copies these types. Each copied file names its source and keeps the Apache-2.0 notice.

| Our type | Source in subtensor | Why we copy it |
|---|---|---|
| `Rao`, `Alpha` | `TaoBalance`, `AlphaBalance` in `common/src/currency.rs:51-95` | Newtypes, so TAO and alpha cannot be mixed |
| `NetUid` | `common/src/lib.rs:77` | Newtype for the subnet id |
| `SafeFloat` | `primitives/share-pool` | Converts shares to alpha |
| `Balancer` price | `pallets/swap/src/pallet/balancer.rs` | The slippage quote in [analytics](08-analytics.md) |

Our `Alpha` carries its subnet, `Alpha { netuid, amount }`, because alpha from two subnets is two assets. Adding two `Alpha` values checks that the subnets match.

```rust
pub struct Rao(u64);
pub struct Alpha { netuid: NetUid, amount: u64 }
pub struct NetUid(u16);
pub struct SpecVersion(pub u32);
pub struct BlockNumber(u32);
#[derive(DecodeAsType)]
pub struct Hotkey([u8; 32]);
#[derive(DecodeAsType)]
pub struct Coldkey([u8; 32]);
```

`Hotkey` and `Coldkey` are separate types, so a function that takes a hotkey rejects a coldkey at compile time. Both print with the SS58 prefix 42.
