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
- **Root Reborn** (spec 441) added validator **baskets**. A basket is a fund of alpha holdings that root stakers own shares of. Root claims still exist after spec 441, and `RootClaimed` still fires (`pallets/subtensor/src/staking/claim_root.rs:1368`).

## Date each item by the block's metadata

The spec numbers in this document come from subtensor's git history. A script walks the 213 spec-bump commits on `main` and records the first spec whose commit contains each item. Commit order is close to on-chain order, but mainnet did not run every spec. The reference squid's types, generated from mainnet metadata, show the first mainnet spec where they differ. For example, the swap V3 storage is in the spec 283 commit, but the first mainnet runtime with it is 290. The squid has `src/types/v290.ts` and no `v283.ts`.

The jobs therefore never branch on a spec number alone. Each job asks the metadata cache whether the block's metadata declares the storage item, runtime API, or event it needs. Metadata V15, which spec 233 already exposes, lists runtime APIs too. The spec numbers below say when to expect a change. The metadata decides.

## Read this chain state in release 1

Release 1 decodes no events. It runs five jobs that read storage and runtime APIs at scheduled blocks. [Ingestion](03-ingestion.md#run-every-job-on-one-runner) describes how the runner schedules them.

| Job (`worker` label) | Schedule | History it keeps | Reads | Writes |
|---|---|---|---|---|
| `prices` | every block | `Window(2 days)` by default | the price (see [the eras](#price-comes-from-a-weighted-balancer-pool)), plus `SubnetTAO`, `SubnetTaoProvided` (specs 290 to 422), `SubnetAlphaIn`, `SubnetAlphaInProvided` (specs 290 to 422), `SubnetAlphaOut`, and `SubnetVolume` (`u128`) for every netuid in `NetworksAdded` except 0 | `subnet_price` |
| `validators` | blocks where `n % 100 == 0` | `Latest` | `Delegates`, `NetworksAdded`, `SubnetMovingPrice`, `TaoWeight`, `TotalHotkeyAlpha` (paged), `AlphaDividendsPerSubnet`, `StakingHotkeys` (paged), `Owner`, `IdentitiesV2` | `validator`, `validator_subnet` (the whole set is replaced in one transaction) |
| `epochs` | every block | `Window(31 days)` | `LastMechansimStepBlock` for every netuid. For subnets whose value equals `n`, also `AlphaDividendsPerSubnet` and `TotalHotkeyAlphaLastEpoch` (0 before spec 261). Netuid 0 is skipped | `epoch_dividend` |
| `baskets` | blocks where `n % 500 == 0`, from spec 441 | `Window(31 days)` | `BetaBasketRuntimeApi.get_all_validator_baskets` and `BasketRate` | `basket_snapshot` |
| `identities` | blocks where `n % 100 == 0` | `Latest` | `SubnetIdentitiesV3` (spec 283 and later) | `subnet_identity` (the whole set is replaced) |

[Compatibility numbers](08-analytics.md#compatibility-numbers) defines what the API computes from these rows.

### Storage items and runtime APIs

All storage items below are in `SubtensorModule` unless the table names another pallet. "From spec" is the first spec-bump commit that has the item, with the first mainnet spec in parentheses where the two differ.

| Item | Declared at | Key | Value | From spec |
|---|---|---|---|---|
| `NetworksAdded` | `pallets/subtensor/src/lib.rs:2675` | `NetUid` | `bool` | 233 |
| `SubnetTAO` | `pallets/subtensor/src/lib.rs:2061` | `NetUid` | `TaoBalance` (`u64`) | 233 |
| `SubnetAlphaIn` | `pallets/subtensor/src/lib.rs:2103` | `NetUid` | `AlphaBalance` (`u64`) | 233 |
| `SubnetAlphaOut` | `pallets/subtensor/src/lib.rs:2108` | `NetUid` | `AlphaBalance` (`u64`) | 233 |
| `SubnetTaoProvided`, `SubnetAlphaInProvided` | `pallets/subtensor/src/migrations/migrate_cleanup_swap_v3.rs:11,16` | `NetUid` | `u64` | 283 (290) to 422 |
| `SubnetVolume` | `pallets/subtensor/src/lib.rs:2056` | `NetUid` | `u128` | 233 |
| `SubnetMovingPrice` | `pallets/subtensor/src/lib.rs:2034` | `NetUid` | `I96F32` | 233 |
| `TaoWeight` | `pallets/subtensor/src/lib.rs:1742` | none | `u64` | 233 |
| `Delegates` | `pallets/subtensor/src/lib.rs:1795` | hotkey | `PerU16` (a `u16`; plain `u16` at spec 233) | 233 |
| `TotalHotkeyAlpha` | `pallets/subtensor/src/lib.rs:2191` | (hotkey, `NetUid`) | `AlphaBalance` | 233 |
| `TotalHotkeyAlphaLastEpoch` | `pallets/subtensor/src/lib.rs:2209` | (hotkey, `NetUid`) | `AlphaBalance` | 261 |
| `AlphaDividendsPerSubnet` | `pallets/subtensor/src/lib.rs:1865` | (`NetUid`, hotkey) | `AlphaBalance` | 233 |
| `StakingHotkeys` | `pallets/subtensor/src/lib.rs:2117` | coldkey | `Vec<hotkey>` | 233 |
| `Owner` | `pallets/subtensor/src/lib.rs:1771` | hotkey | coldkey | 233 |
| `IdentitiesV2` | `pallets/subtensor/src/lib.rs:3301` | coldkey | `ChainIdentityOfV2` | 233 |
| `SubnetIdentitiesV3` | `pallets/subtensor/src/lib.rs:3306` | `NetUid` | `SubnetIdentityOfV3` | 283 (290) |
| `LastMechansimStepBlock` | `pallets/subtensor/src/lib.rs:2783` | `NetUid` | `u64` | 233 |
| `BasketRate` | `pallets/subtensor/src/lib.rs:3674` | hotkey | `I96F32` | 441 |
| `SwapRuntimeApi.current_alpha_price` | `pallets/swap/runtime-api/src/lib.rs:29` | `NetUid` argument | `u64` | 283 (290) |
| `SwapRuntimeApi.current_alpha_price_all` | `pallets/swap/runtime-api/src/lib.rs:30` | none | `Vec<SubnetPrice>` | 391 |
| `BetaBasketRuntimeApi.get_all_validator_baskets` | `pallets/subtensor/runtime-api/src/lib.rs:115` | none | `Vec<BasketSummary>` | 441 |

Spec 233 keyed the subnet maps by a plain `u16`. Later specs use the `NetUid` newtype. Both encode the same bytes, so the storage keys do not change.

`SubnetTaoProvided` and `SubnetAlphaInProvided` left the pallet at spec 423. The migration `migrate_cleanup_swap_v3` moved their values into `SubnetTAO` and `SubnetAlphaIn` (`pallets/subtensor/src/migrations/migrate_cleanup_swap_v3.rs:39-48`), so from spec 423 the reserves are the two plain items. Read the provided items only when the block's metadata declares them, and use 0 otherwise.

### Read every netuid in one request

The `prices` job computes the storage keys for every netuid and every item once. It reads all of them for a block with one `state_queryStorageAt` call. It refreshes the netuid set from `NetworksAdded` every 100 blocks, and on any block where the price API returns a netuid it does not know. With the price call and the block hash, that is about 2 to 3 node requests per block. That count is an estimate. [Ingestion](03-ingestion.md#estimate-the-backfill-time) uses it.

`Timestamp.Now` goes in the same `state_queryStorageAt` call, so every job gets the block's time without a second request.

### Price comes from a weighted balancer pool

The pool was a Uniswap-v3 style pool, with `AlphaSqrtPrice` holding the price as `U64F64`. Spec 423 replaced it with a weighted balancer (`pallets/swap/src/pallet/migrations/migrate_swapv3_to_balancer.rs:25`). The current price is `Balancer::calculate_price(x, y)` in `pallets/swap/src/pallet/balancer.rs:220`, from the subnet's alpha and TAO reserves.

The `prices` job uses the runtime API wherever it exists, and storage math only before it. Choose the era by what the block's metadata declares.

1. From spec 391, call `SwapRuntimeApi.current_alpha_price_all` once per block.
2. For specs 290 to 390 (283 to 390 in git), call `SwapRuntimeApi.current_alpha_price(netuid)` once per subnet.
3. Before spec 290, compute `SubnetTAO × 10^9 / SubnetAlphaIn`. The provided items do not exist yet, so their terms are 0.

Both runtime API calls return rao per whole alpha, which is the unit `subnet_price.price` stores. The runtime computes the price in `current_price` (`pallets/swap/src/pallet/impls.rs:22-35`). It returns 10^9, a price of 1, for any subnet whose mechanism is not 1. It returns 0 for a mechanism-1 subnet whose alpha reserve is 0. The `prices` job does not read root (netuid 0), so root never gets a price row.

The squid computed prices for specs 290 to 422 from `AlphaSqrtPrice` with integer math, while the runtime API uses its own fixed-point path. Our historic prices for those specs can therefore differ from the squid's stored values by 1 rao. `/pools` reads only the last 26 hours, so no compat value depends on them. The [decisions log](../decisions.md#d15-read-prices-from-the-runtime-api-not-from-v3-storage-math) records the trade.

In release 2, `prices` keeps full history for candles. Specs 290 to 390 then cost one runtime API call per subnet per block. [Ingestion](03-ingestion.md#estimate-the-backfill-time) estimates that cost.

### Detect an epoch from chain state

A block is an epoch block for a subnet when `LastMechansimStepBlock` for that subnet equals the block number. The name has a typo on chain. Use it as written. Recent runtimes can run an epoch early or defer it (`EpochTriggered` and `EpochDeferred` at `pallets/subtensor/src/macros/events.rs:682,692`), so the schedule must come from chain state, not from a formula on `Tempo`.

At an epoch block, the `epochs` job reads `AlphaDividendsPerSubnet` for that netuid by key prefix and keeps the hotkeys with a nonzero dividend. It reads `TotalHotkeyAlphaLastEpoch(hotkey, netuid)` for each of them as `stake_at_epoch`. Before spec 261 that item does not exist, and `stake_at_epoch` is 0.

### Read baskets from the runtime API

`get_all_validator_baskets` returns one `BasketSummary` per validator with an active basket (`pallets/subtensor/src/rpc_info/basket_info.rs:30-44`). The `baskets` job keeps `hotkey`, `nav_tao`, and `shares` (a `u64`). It reads `BasketRate(hotkey)` for each returned hotkey in the same block.

The summary has two shapes. Mainnet specs 441 to 459 carry a `weights: Vec<(NetUid, u16)>` field between `redeemed_tao` and `holdings`. Mainnet spec 460 removed it. The reference squid dates the removal to 460 from mainnet blocks (`src/basket-runtime-api.ts:17-18,32`), while git puts it in the spec 461 commit (`7c9d45ebd`). Decode each shape with its own struct, chosen by the metadata's return type.

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

Read each item only when the block's metadata declares it.

## Decode each block with its own metadata

Each runtime version has its own metadata. The metadata holds a type registry that says how every event, storage value, and runtime API result is encoded. A decoder built for spec 490 reads spec 233 bytes wrong, or fails.

A block's events are encoded by the runtime that executed the block. That is the runtime in the parent's state. The block that applies a runtime upgrade still runs the old runtime, and its post-state holds the new one. Two rules follow.

- **Decode events with the parent's spec.** Release 2's `events` job fetches metadata at the parent hash.
- **Read state with the block's own spec.** A storage value or runtime API result at block `n` is encoded by the runtime in block `n`'s post-state. Every release 1 job uses this rule.

### Give subxt the spec and metadata

subxt's defaults break the first rule. `OnlineClient` calls `Core_version` and fetches metadata against the block's own hash, unless the `Config` supplies both. Our `Config` implements the two hooks from the `runtime_version` table (`subxt-0.50.3/src/config.rs:113,125`).

```rust
impl Config for Subtensor {
	fn spec_and_transaction_version_for_block_number(&self, n: u64) -> Option<(u32, u32)> {
		self.versions.at_block(n)
	}

	fn metadata_for_spec_version(&self, spec: u32) -> Option<ArcMetadata> {
		self.metadata.get(spec)
	}

	fn set_metadata_for_spec_version(&self, spec: u32, metadata: ArcMetadata) {
		self.metadata.insert(spec, metadata);
	}
}
```

`versions.at_block(n)` answers from the `first_block` of each row in `runtime_version`. It returns the spec of the runtime that executed block `n`, which is the spec in the parent's state. The `chain` crate fills the table as it meets new specs.

1. For a block `n` beyond the known ranges, call `state_getRuntimeVersion` at the parent hash.
2. If the spec is new, fetch its metadata with `state_getMetadata` at the same parent hash. Store the spec, `first_block = n`, and the raw bytes in `runtime_version` in one transaction.
3. Load each spec's metadata from the table into the in-memory cache on first use.

A state read at block `n` uses the spec of block `n + 1`'s parent, which is block `n` itself. The `chain` crate looks that up in the same table.

### Decode into era structs

Decode each event's fields into a struct that derives `scale_decode::DecodeAsType`. `DecodeAsType` matches our struct against the type in that spec's registry. It does not assume one fixed layout. Each struct holds one era's shape, and a `From` conversion turns it into the domain type.

```rust
#[derive(DecodeAsType)]
struct StakeAddedV233(Coldkey, Hotkey, u64, u64, u16);

#[derive(DecodeAsType)]
struct StakeAddedV257(Coldkey, Hotkey, u64, u64, u16, u64);

#[derive(DecodeAsType)]
struct BasketClaimedV441 { hotkey: Hotkey, coldkey: Coldkey, tao: u64 }

fn stake_added(raw: &RawEvent, spec: SpecVersion) -> Result<StakeAdded, DecodeError> {
	match spec.0 {
		..257 => raw.decode_fields::<StakeAddedV233>().map(StakeAdded::from),
		_ => raw.decode_fields::<StakeAddedV257>().map(StakeAdded::from),
	}
}
```

Era structs use `Coldkey` and `Hotkey`, never a bare account type. Tuple stake events put the coldkey first (`pallets/subtensor/src/macros/events.rs:17-24`), while basket events are structs that put the hotkey first (`pallets/subtensor/src/macros/events.rs:514-521`). With bare accounts, a `From` that swaps them compiles. With the newtypes, it fails with `E0308`.

`Coldkey` and `Hotkey` wrap `[u8; 32]` directly and derive `DecodeAsType`. The chain's `AccountId32` is a composite with one `[u8; 32]` field, so the depths match. A probe showed that the derive does not look through an extra layer. `Coldkey(subxt::utils::AccountId32)` against the chain's `AccountId32` fails with `Unexpected(Array)`.

Amounts and netuids decode as plain `u64` and `u16`. `TaoBalance`, `AlphaBalance`, and `NetUid` are newtypes on chain from some spec onward, and a plain integer decodes from them. A derived `NetUid(u16)` does not decode from the plain `u16` that spec 233 uses, so the `From` conversion wraps the integers in our newtypes instead.

The match on `spec` lists every shape change in one place. The reference squid instead generated types for about 60 runtime versions and checked each with `.v233.is()` calls. Our approach needs one struct per real shape change, which is two for `StakeAdded`.

The `runtime_version` table stores the raw metadata bytes for every spec we have seen. A decode bug fix then re-decodes old blocks without fetching metadata from the node again.

### Find every shape change before writing a decoder

Do not rely on release notes to find shape changes. Build a small tool, `bt-indexer metadata diff`. It loads the metadata for every spec from 233 to the current one and prints each spec where an item we read or decode changed type. Run it whenever the chain upgrades, and add a struct for each change it reports. A decoder test then decodes a recorded block from each era. [Quality](09-quality.md#test-the-decoders-against-recorded-blocks) describes those tests.

Known shape changes since spec 233:

- `StakeAdded` and `StakeRemoved` gained the fee field at spec 257.
- `ColdkeySwapped` dropped its `swap_cost` field at spec 385.
- `RootClaimed` gained a `tao` field at spec 441.
- `Delegates` changed from `u16` to `PerU16`, which encodes the same.
- The swap V3 storage appeared at spec 283 (290 on mainnet) and left at spec 423, when the pool changed to a balancer.
- `TotalHotkeyAlphaLastEpoch` appeared at spec 261, and `SubnetIdentitiesV3` at spec 283 (290 on mainnet).
- Basket storage and the basket runtime API appeared at spec 441. Spec 460 removed `weights` from `BasketSummary`.
- `SwapRuntimeApi.current_alpha_price_all` appeared at spec 391.

### Handle an unknown event

The `events` job skips an event it does not index by pallet and variant name before decoding, so the event costs almost nothing. An event we index that fails to decode stops the process. [Error handling](06-error-handling.md#classify-errors-in-one-place) explains why.

## Index these events in release 2

Release 2 adds the `events` job. Indexing starts at block 4,920,350, which runs spec 233. The tables list each event, where subtensor defines it, and the specs where its shape changed. Event lines refer to `pallets/subtensor/src/macros/events.rs`.

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
| `ColdkeySwapped` (line 196) | struct of old coldkey, new coldkey | 233 to 384 add `swap_cost: u64` | Reconciles both coldkeys |
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

Emissions also grow stake with no event. Every coldkey that gains stake by any path is added to `StakingHotkeys` (`pallets/subtensor/src/staking/stake_utils.rs:830-841`). The daily reconcile therefore takes its coldkeys from the keys of `StakingHotkeys`, not from our `position` table.

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

### Other details

- Accounts use the SS58 prefix 42.
- Basket holdings count toward `TotalHotkeyAlpha`. Release 1 serves `TotalHotkeyAlpha` as the squid does and does not subtract them.

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

`Hotkey` and `Coldkey` are separate types, so a function that takes a hotkey rejects a coldkey at compile time.
