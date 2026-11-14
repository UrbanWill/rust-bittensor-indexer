# Analytics

This document defines every derived number the API serves. Each definition gives the formula, the tables it reads, and the cases where the number is undefined. A number with no definition here does not ship.

Release 1 serves only the numbers in [compatibility numbers](#compatibility-numbers). They match what the reference squid computes, because tao-data-api forwards them to its clients. Release 2 adds the [native analytics](#release-2-analytics).

All amounts are integers in rao or raw alpha units (10^9 per whole alpha) until the last step. Ratios are computed in `f64` (or `float8` in SQL) at query time and are never stored as amounts.

Citations use two roots. `S/` is the reference squid, `bittensor-squid/`. `T/` is `tao-data-api/src/`. After a file's first citation in a section, later citations give its file name only.

## Compatibility numbers

These numbers feed the five routes in [API](05-api.md#compatibility-routes). Each one is defined from the squid's code, because that code produces what tao-data-api receives today. Where we deviate on purpose, the definition says so, and the decisions log records it.

### `/pools`

`/pools` reads `subnet_price`, which the `prices` job writes every block for every netuid in `NetworksAdded` except 0. Root therefore never appears.

**The tip block.** The response describes one block, the tip.

- The tip is the newest `subnet_price` row by `ts`. Its `block_number` and `ts` anchor everything else (`S/api/src/routes/pools.ts:50-55`).
- The current row for each netuid is its row at exactly the tip block. A netuid with no row at the tip block is left out of the response (`pools.ts:56-63`). The `prices` job writes a block's rows in one transaction, so the tip block always has a row for every subnet that existed at that block.
- The day-ago row for each netuid is its newest row with `tip.ts − 26 h < ts ≤ tip.ts − 24 h` (`pools.ts:64-71`). The window anchors on the tip's timestamp, not on `now()`.
- Rows are ordered by `netuid` ascending (`pools.ts:82`).

**The fields.** Every field except `netuid` is a JSON string. tao-data-api's schema accepts a number or a string (`T/endpoints/pools.ts:21-29`) and forwards the value unchanged, so the exact string is the contract.

| Field | Unit | Definition |
|---|---|---|
| `netuid` | | JSON integer |
| `total_tao` | rao, integer | `tao_reserve`, which is `SubnetTAO + SubnetTaoProvided` (`pools.ts:74`) |
| `total_alpha` | raw alpha, integer | `alpha_reserve + alpha_out` (`pools.ts:75`). Sum the two in Rust as `u64`. Each is below 2^63, so the sum fits |
| `price` | TAO per alpha, decimal | `fmt_scaled::<9>(price)` (`pools.ts:6-15,121`) |
| `market_cap` | rao, decimal | `fmt_scaled::<9>(price × total_alpha)`, with the product in `u128` (`pools.ts:26-35,123`) |
| `price_change_1_day` | percent, decimal | See below (`pools.ts:87-108`) |
| `tao_volume_24_hr` | rao, integer | `lifetime_volume(current) − lifetime_volume(day-ago)`. It is `"0"` when there is no day-ago row or when the counter went down (`pools.ts:110-115`) |

`price` is the stored integer in rao per whole alpha. `market_cap` is in rao, not TAO, and keeps up to 9 fraction digits. Both inputs come from `BIGINT` columns, so each is below 2^63, and the product fits `u128`.

**Decimal format.** `fmt_scaled::<S>(v)` prints the integer `v` divided by 10^S.

- The integer part is `v / 10^S`.
- The fraction is `v mod 10^S`, padded to `S` digits, with trailing zeros removed.
- With no fraction left, there is no decimal point. Zero is `"0"`.

**Price change.** Let `cur` be the current price and `old` the day-ago price, both integers in rao per alpha.

- If there is no day-ago row, or `old` is 0, the value is `"0"`.
- Otherwise `scaled = trunc((cur − old) × 10^18 / old)`, where `trunc` rounds toward zero, as JavaScript `BigInt` division does.
- The squid multiplies `scaled` by 100 and prints it with 18 fraction digits. That is the same string as `fmt_scaled::<16>(|scaled|)`, so the value has at most 16 fraction digits.
- A negative `scaled` gets a `-` prefix. A change that truncates to 0 is `"0"`, with no sign.
- For example, `cur = 1` and `old = 3` give `"-66.6666666666666666"`.

A subnet younger than about 24 hours, or one with no row in the 24 to 26 hour window, therefore reports `price_change_1_day = "0"` and `tao_volume_24_hr = "0"`, not null. No `/pools` field is ever null.

**Rust helpers.** These functions use integer math only. `bt-domain` denies `clippy::arithmetic_side_effects` with `#![deny(clippy::arithmetic_side_effects)]` at the top of `crates/domain/src/lib.rs` ([error handling](06-error-handling.md)), so their bodies use only operations that lint accepts ([D27](../decisions.md#d27-hold-amounts-in-u128-and-write-the-format-helpers-lint-clean)). The signatures are these.

```rust
/// Prints `value / 10^SCALE` with at most `SCALE` fraction digits, trailing zeros removed.
pub fn fmt_scaled<const SCALE: u32>(value: u128) -> String;
pub fn rao_to_tao(rao: u64) -> String;
pub fn market_cap(price: u64, total_alpha: u64) -> String;
pub fn price_change_1_day(cur: u64, old: Option<u64>) -> String;
pub fn tao_volume_24_hr(cur: u128, old: Option<u128>) -> u128;
```

The bodies are in `spikes/crates/api/src/format_lint.rs`. The lint rejects plain `/` and `%` in `fmt_scaled`, the `u128` product in `market_cap`, an `i128` expression in `price_change_1_day`, and `cur - old` in `tao_volume_24_hr` (`spikes/crates/api`, `P33`). Each body uses an operation the lint accepts instead.

- `fmt_scaled` divides by `const { NonZeroU128::new(10u128.pow(SCALE)).unwrap() }`. The divisor is computed at compile time, so a `SCALE` above 38 fails the build.
- `market_cap` multiplies with `u128::strict_mul`. Two `u64` values always fit a `u128` product.
- `price_change_1_day` filters `old` through `NonZeroU64`, computes `u128::from(cur.abs_diff(old)).strict_mul(10^18) / NonZeroU128::from(old)`, and adds the `-` only when `cur < old` and the result is not 0. `|cur − old| × 10^18` stays below 2^128.
- `tao_volume_24_hr` is `old.and_then(|old| cur.checked_sub(old)).unwrap_or(0)`.

These bodies produce the same strings as the squid's `pools.ts` code on 27,026 rows (`spikes/crates/api`, `P33-lint-clean`).

The division truncates toward zero, as `BigInt` does, because it runs on the absolute difference and the sign is added afterwards. The ×100 step disappears because printing `scaled × 100` with 18 digits equals printing `scaled` with 16.

### `/validators`

`/validators` reads `validator`, which the `validators` job replaces as a whole set at every block where `n % 100 == 0` (`S/src/main.ts:1508`).

**Which hotkeys have a row.** A hotkey has a row when it is a key of `Delegates` and has a nonzero `TotalHotkeyAlpha` on at least one netuid (`main.ts:1522-1531,1591-1595,1835-1838`). The response then keeps only rows with `global_weighted_stake > 0` (`S/api/src/routes/validators.ts:272`).

| Field | JSON | Definition |
|---|---|---|
| `hotkey` | string | SS58 with prefix 42 |
| `name` | string or null | See [text fields](#text-fields) |
| `global_weighted_stake` | string | Integer, rao. See below. It must be a JSON string, because tao-data-api's schema is `z.string()` (`T/endpoints/validators.ts:21`) |
| `global_nominators` | number | See below |
| `active_subnets` | number | See below |
| `rank` | number | See below |

**`global_weighted_stake`.** This is the squid's display figure, a TAO-equivalent sum of the hotkey's stake. It is not the chain's stake weight, which [release 2](#chain-stake-weight) defines. For every netuid `n` where `alpha = TotalHotkeyAlpha(hotkey, n)` is nonzero, add one term, floored on its own (`main.ts:1600-1609,1621`).

- For `n ≠ 0`, the term is `alpha × SubnetMovingPrice(n).bits / 2^32`. `SubnetMovingPrice` is `I96F32`, an EMA of the price, not the spot price (`main.ts:1538-1542,1559-1563`). A netuid missing from `NetworksAdded` has no moving price and adds 0, but its stake still gives the hotkey a row.
- For `n = 0`, the term is `alpha × TaoWeight / (2^64 − 1)` (`main.ts:1543-1546,1603-1605`). `U64_MAX` is `(1n << 64n) - 1n` (`main.ts:1459`).

Compute the sum in `u128` with `checked_mul` and `checked_add`. An overflow is a `StopProcess` error, as [error handling](06-error-handling.md#classify-errors-in-one-place) describes.

**`global_nominators`.** The number of coldkeys whose `StakingHotkeys` list contains the hotkey, counting each coldkey once (`main.ts:1740-1769`). The owner's coldkey counts when its list contains the hotkey. Stake is not checked.

**`active_subnets`.** The number of netuids in `NetworksAdded`, root included, where `AlphaDividendsPerSubnet(netuid, hotkey)` is nonzero at the snapshot block (`main.ts:1700-1725,1857`). That storage holds the dividends of each subnet's last epoch.

**`rank`.** The 1-based position after the filter, ordered by `global_weighted_stake` descending, then by hotkey ascending. The response rows use the same order. The squid uses `ROW_NUMBER() OVER (ORDER BY global_weighted_stake DESC)` with no tiebreaker (`validators.ts:270,273`), so its order of equal stakes is arbitrary. The hotkey tiebreaker is a deliberate deviation that makes `rank` deterministic. It compares the 32-byte account IDs.

### `/subnets/{netuid}/validators` and `/validators/{hotkey}/subnets`

Both routes read `validator_subnet`, which the same `validators` job writes. A row exists for every pair of a hotkey with a `validator` row and a netuid with nonzero `TotalHotkeyAlpha` (`main.ts:1624-1634`).

- `/subnets/{netuid}/validators` returns that netuid's rows with `alpha_stake > 0`, ordered by `alpha_stake` descending, then by hotkey (`validators.ts:337-339`).
- `/validators/{hotkey}/subnets` returns that hotkey's rows with `alpha_stake > 0`, ordered by `alpha_stake` descending, then by netuid (`validators.ts:405-407`). The order compares raw alpha across subnets with different prices, so it is not an order by value.

| Field | JSON | Definition |
|---|---|---|
| `hotkey` or `netuid` | string or number | The other half of the key |
| `stake` | string | `TotalHotkeyAlpha(hotkey, netuid)`, raw, as an integer string (`validators.ts:329`). At netuid 0 it is root TAO in rao. It must be a string (`T/endpoints/subnetValidators.ts:31`) |
| `thirty_day_apy` | number or null | For `netuid ≠ 0`, the [epoch APY](#epoch-apy), net. For netuid 0, the [root APR](#root-apr) (`validators.ts:203-205`) |
| `thirty_day_apy_gross` | number or null | For `netuid ≠ 0`, the epoch APY, gross. For netuid 0, always null (`validators.ts:206-207`) |

Both APY values are fractions. 0.59 means 59%.

### Epoch APY

Dividends compound. Each epoch's dividend raises the validator's stake, and the next epoch pays on the larger amount. The APY therefore compounds the per-epoch yields and annualizes them (`S/api/src/routes/validators.ts:44-116`, explained at `:13-43`).

The inputs come from `epoch_dividend`. The `epochs` job writes one row for each `(netuid, hotkey)` with nonzero `AlphaDividendsPerSubnet` at each block where `LastMechansimStepBlock(netuid)` equals the block number. `stake_at_epoch` is `TotalHotkeyAlphaLastEpoch`. The squid stores 0 before spec 261 (`S/src/main.ts:2157-2212`). Our decoder returns `None` there ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)), and release 1 never reads that era. What a full-history job stores there is an open question in the [release 2 outline](../plan/release-2-outline.md). Netuid 0 has no rows.

**The anchor.** Let `H` be the `last_block_ts` of the `epochs` checkpoint, the chain time of the newest block the job has committed. Every window below uses `H`. The squid filters rows by wall-clock `NOW()` and annualizes from `MAX(block.timestamp)` (`validators.ts:57,73,113`). When the indexer is caught up, the two anchors differ by about 24 seconds, the two blocks between the best and the finalized head (`spikes/crates/chain-errors`, `finality-gap`). When it lags, our rows and our window still describe the same 30 days. This is a deliberate deviation.

For a hotkey `h` on netuid `n ≠ 0`:

- `take = validator.take / 65535`, or 0 when the hotkey has no `validator` row (`validators.ts:50-52`).
- `E` is the set of rows for `(h, n)` with `ts > H − 30 days` and `dividend > 0` (`validators.ts:110-114`).
- `n` is **measurable** when its rows from all hotkeys with `ts > H − 30 days` have at least 10 distinct `ts` values (`validators.ts:53-59`).
- For each row in `E`, `y_i = dividend / stake_at_epoch` in `f64`, or `y_i = 0` when `stake_at_epoch` is 0 (`validators.ts:66-69`).
- `W = H − max(H − 30 days, first)` in seconds, where `first` is the earliest `ts` of any row for netuid `n`, in any window (`validators.ts:71-82`). A new subnet is then not diluted by days before it existed. If `W` is 0, both values are null.
- `Y = 31,557,600`, the seconds in 365.25 days.

| Case | `thirty_day_apy` (net) | `thirty_day_apy_gross` |
|---|---|---|
| `take ≥ 1` | as below | null |
| `\|E\| ≥ 10` | `exp(Σ ln(1 + y_i))^(Y / W) − 1` | `exp(Σ ln(1 + y_i / (1 − take)))^(Y / W) − 1` |
| `\|E\| = 0` and `n` is measurable | 0 | 0 |
| any other case | null | null |

(`validators.ts:63-85` for net, `:86-109` for gross.)

A value that is not a finite `f64`, such as a compounded value that overflows, is null ([D29](../decisions.md#d29-return-a-null-apy-where-the-squids-sql-errors)). Against the squid's own SQL on PostgreSQL 17, the formula above matched in 27 of 30 cases with a largest relative difference of 1.0 × 10^-12 (`spikes/crates/api`, `apy-formula`).

The other 3 cases are where the squid's SQL raises an error and fails its whole route: `division by zero` for a 100 % take with 10 or more earning epochs, and `value out of range: overflow` when the compounded value overflows. Ours answers the route, with gross null at a 100 % take as the table says, and with net and gross null on overflow. `compat diff` treats a squid 500 on those rows as expected.

- The take row wins for gross. A hotkey at 100% take that earned nothing on a measurable subnet has net 0 and gross null.
- A hotkey with 1 to 9 rows in `E` gets null on every subnet, measurable or not.
- An epoch where the validator earned nothing has no row, so it counts as a yield of 0 inside `W`.
- `AlphaDividendsPerSubnet` is after the validator's take, so net is what a nominator receives. Gross applies `1 / (1 − take)` per epoch, because the take is charged at each epoch before the dividend compounds. `gross × (1 − take)` therefore does not equal net.
- The sum of logs avoids the precision loss of multiplying hundreds of small factors. The order of the sum is unspecified, so two correct implementations can differ in the last bits.

The APY measures yield in alpha. It leaves out changes in the alpha price.

`epoch_dividend` keeps 31 days, and its retention drops chunks older than 33 days. That is enough for `first`, because `max(H − 30 days, first)` ignores any `first` older than 30 days.

### Root APR

A root position is a claim on the validator's basket. `BasketRate` is a running total of basket shares per rao of root stake, stored as raw `I96F32` bits. Netuid 0 serves this simple APR as `thirty_day_apy`, with no compounding, because root rewards stay in the basket and do not earn the root rate (`validators.ts:118-153`).

The inputs are the `basket_snapshot` rows for the hotkey with `ts > H_b − 30 days`, ordered by `ts`. `H_b` is the `last_block_ts` of the `baskets` checkpoint. The squid uses wall-clock `NOW()` here (`validators.ts:179`), so this is the same deliberate deviation as the epoch APY. The `baskets` job writes a row every 500 blocks for each fund that the runtime API returns, with `rate_bits = BasketRate(hotkey).bits`, or 0 when the storage entry is missing (`S/src/main.ts:2048-2050,2075-2078`).

For each row `i`, ordered by `ts`:

- `price_i = nav_tao_i / shares_i` in `f64`, or null when `shares_i` is 0 (`validators.ts:174`).
- For `i ≥ 2`, `rate_delta_i = (rate_bits_i − rate_bits_{i−1}) / 2^32`, computed exactly in `numeric` or `i128`, then converted to `f64` (`validators.ts:172-173`). The first row has no delta.
- For `i ≥ 2`, `step_i = max(price_i / price_{i−1}, price_{i−1} / price_i)` (`validators.ts:168-170`).

Over the window:

- `n` is the number of rows.
- `priced` is the number of rows with `price_i > 0`. A row with zero shares or zero NAV is not priced.
- `window_yield = Σ over i ≥ 2 of rate_delta_i × price_i`.
- `step = max step_i`, or 1 when there is only one row.
- `span` is `max(ts) − min(ts)` in seconds.

`root_apr = window_yield × 31,557,600 / span` (`validators.ts:161`). It is null unless all four conditions hold (`validators.ts:157-162`):

1. `n ≥ 10`.
2. `priced = n`. A fund that held no shares, or had zero NAV, at any snapshot in the window gets null.
3. `step ≤ 2`. A near-total redemption that leaves dust NAV over a few shares makes the share price jump, and the interval accrued at the old price would be valued at the new one.
4. `span > 0`.

A hotkey with no fund has no rows, so `n = 0` and the value is null. Each interval's new shares are valued at the share price of that interval. Valuing all of them at the end price would count later share price changes as yield. Slippage and fees are already inside `nav_tao`.

### Text fields

The validator `name` and the eight `/v1/subnet-identities` text fields decode chain bytes the same way (`S/src/bytes.ts:2-16`).

1. Decode the bytes as UTF-8, replacing each invalid sequence with U+FFFD.
2. Remove every NUL character.
3. Trim whitespace from both ends.
4. Map an empty result to null.

`name` comes from `IdentitiesV2(Owner(hotkey)).name`. It is null when the hotkey has no owner or the owner has no identity (`S/src/main.ts:1771-1830,1848`).

The squid trims with JavaScript's `String.prototype.trim`, whose whitespace set differs from Rust's `str::trim`. JavaScript trims U+FEFF and keeps U+0085. Rust does the opposite. `String::from_utf8_lossy` followed by this function must match Node's `Buffer.toString('utf8')` and `trim`, including on truncated, overlong, and surrogate sequences. The identity decoder's tests compare the two on such inputs.

```rust
fn trim_js(s: &str) -> &str {
    s.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}
```

## Release 2 analytics

These numbers need the `events` job, positions, or the candle aggregates, which release 2 adds.

### Candles

`candle_*` views give the open, high, low, and close of the price in each bucket. `open` is the first price point in the bucket and `close` is the last. A bucket with no price point has no candle. The API does not fill gaps.

Volume comes from the chain's own counter, `SubnetVolume`, which the `prices` job stores as `lifetime_volume` on every row. Each candle view stores `last(lifetime_volume, ts)` per bucket. The API computes the volume of a bucket at query time as that value minus the previous bucket's value, with `LAG` over the buckets, and uses 0 when the counter went down. For the first bucket of a range, or after a gap, it uses `last − first` within the bucket.

Volume does not come from stake events. Both swap functions add to `SubnetVolume` (`pallets/subtensor/src/staking/stake_utils.rs:1034,1087`), and many swaps emit no stake event. The squid uses the same counter.

### Market cap

The native API uses the [compatibility formula](#pools), `price × (alpha_reserve + alpha_out) / 10^9` in rao, with the product in `u128`, or `numeric` in SQL. A `BIGINT` product overflows, for example `(500 × 10^9) × (20 × 10^6)`. The API also returns `alpha_out` alone, for clients that count only the alpha that stakers hold.

### Emission share

`emission_share(n) = tao_in_emission(n) / Σ tao_in_emission` over all subnets, from the same `subnet_snapshot` block. The value is between 0 and 1. It is undefined when the sum is 0.

### Net TAO flow

Net flow is the TAO that stakers moved into a subnet over a window.

```
net_flow(n, window) = Σ tao of StakeAdded on n − Σ tao of StakeRemoved on n
```

The sum covers every `staking_action` in the window, including the two legs of a move.

- A move from subnet A to subnet B is a real outflow from A and a real inflow to B. A cross-subnet move pays one swap fee, on the destination leg unless the origin is root (`pallets/subtensor/src/staking/move_stake.rs:509-510`).
- A move between two hotkeys on the same subnet adds and removes the same amount with a fee of 0 (`stake_utils.rs:1620`), so it cancels out. It never touches the pool.

The chain uses its own flow figure, `SubnetEmaTaoFlow`, to allocate emissions. It is stored as `(u64, I64F64)`, and the flow is the second field. The API returns both, as `net_flow` over the requested window and `ema_tao_flow` from the latest snapshot. If the 30-day `net_flow` and the chain's EMA disagree in sign for a long time, the indexer may have missed stake events. The analytics test suite compares the two on recorded data.

### Holders and concentration

- **Holders** of subnet `n` are the coldkeys whose positions on `n`, summed across hotkeys, hold more than 0 alpha. `subnet_holder_count` stores the count at the first block of each UTC day.
- **Top-10 share** is the alpha held by the 10 largest coldkeys divided by `alpha_out`.
- **HHI** is `Σ (alpha_c / alpha_out)²` over all coldkeys `c`. It is 1 when one coldkey holds everything, and close to 0 when stake is spread evenly.

Both concentration figures group positions by coldkey first, so one coldkey staked through several hotkeys counts once.

### Position value

`value_rao = alpha × price / 10^9` at the latest price, with the product in `u128`, or `numeric` in SQL. This is the mark value. It ignores slippage, which matters for a large position. The API also returns `exit_value_rao`, computed with the [slippage quote](#slippage-quote) for unstaking the whole position.

Root positions are TAO, so their value is their amount.

Every position carries `last_settle_block`. [Ingestion](03-ingestion.md#correct-positions-from-chain-state) explains why positions grow between settles from emissions. The API states that the value is exact at the last settle block and adds the actions since then. It does not claim the value is exact at the head.

### Cost basis (later release)

`cost_basis_rao` is the TAO staked in minus the TAO unstaked out, over the coldkey's actions on the position. `pnl_rao = value_rao − cost_basis_rao`. It needs actions from the first stake, so it is undefined for positions opened before the indexed range.

### Chain stake weight

The chain weighs a hotkey's stake on one subnet as `alpha + tao × tao_weight`, where `alpha` and `tao` are the hotkey's inherited alpha and inherited root TAO on that subnet, and `tao_weight` is `TaoWeight` as a fraction (`stake_utils.rs:263-287`). There is no price term. The native API serves this per subnet as `stake_weight`.

This is a different number from [`global_weighted_stake`](#validators), which sums stake across subnets at the moving price for display. One has a price term and the other does not, so keep the names apart.

### Slippage quote

`GET /api/v1/quote/{netuid}?tao_in=` returns what a swap would pay out now. The pool is a weighted balancer. The swap first takes the fee off the input (`pallets/swap/src/pallet/swap_step.rs:55-56`). It then pays out, for an input `dx` after the fee into a reserve `x`, with output reserve `y`:

```
dy = y × (1 − (x / (x + dx)) ^ (w_in / w_out))
```

The weights come from the `SwapBalancer` storage item, as a `Perquintill` quote weight. The fee comes from `FeeRate`. The `domain` crate ports `Balancer` from `pallets/swap/src/pallet/balancer.rs`, including its rounding.

The response contains:

- `amount_out` in rao or alpha.
- `fee` in the input unit.
- `spot_price` and `effective_price`, both in rao per alpha.
- `price_impact`, which is `|effective_price − spot_price| / spot_price`.
- `as_of_block`, the block of the reserves.
- `weights_block`, the block of the weights and the fee.

The quote reads the reserves from the latest `subnet_price` row. It reads the weights and the fee from the latest `subnet_snapshot`, which is at most 100 blocks old. `weights_block` makes that age visible.

The port has to match the chain to the last rao. A differential test checks it.

1. Record `SwapRuntimeApi.sim_swap_tao_for_alpha` and `sim_swap_alpha_for_tao` results for every subnet at 20 blocks across the balancer era, with several input sizes each. Store them as fixtures.
2. Run the ported function on the same reserves, weights, and inputs.
3. Assert equal output for every case.

A property test also checks invariants on random reserves.

- `amount_out` is less than the output reserve.
- A larger input never pays out less.
- `price_impact` is between 0 and 1.

### Later analytics

These need the events that [chain data](02-chain-data.md) lists for later releases.

- **Registration cost and churn.** Burn cost per registration over time, UID turnover per subnet, and the median time a UID lasts.
- **Weight-setting cadence.** Time between `WeightsSet` or `WeightsRevealed` events per validator and subnet.
- **Yuma and Null consensus.** Dividend spread, validator count, and churn for subnets in each mode, starting from the `EpochConsensusSet` event.
- **Epoch timing.** Deferred and skipped epochs per subnet, from `EpochDeferred` and `EpochSkipped`.
