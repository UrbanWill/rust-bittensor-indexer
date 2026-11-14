# Release 2 outline

Release 2 adds the event pipeline, positions, and the native analytics on top of the release 1 runner. This page orders the work of [the roadmap's release 2](../design/10-roadmap.md#release-2) and says what release 1 already gives each milestone. It has no step breakdown. Plan the steps when release 1 has shipped.

## Answer three questions before the first full-history job

**Prices for specs 290 to 390.** R2.1 moves `prices` to `Full` history. `current_alpha_price_all` starts at spec 391, so for specs 290 to 390 the runtime API answers one subnet per call. That is (7,782,856 − 5,947,548 + 1) × 128 = 234,919,552 price calls, about 235 million, or about 23 days at the 120 calls a second of `chain.rpc_max_per_second` ([D23](../decisions.md#d23-cap-the-call-rate-with-a-one-token-bucket-not-only-the-calls-in-flight)). Each block also makes 3 fixed calls, so a block costs 131. The 23 days are a floor. The era's first and last blocks took 375 s and 266 s for their 131 calls at 2 in flight (`spikes/crates/chain-decode`, `per-subnet-price-cost`), so measure the era's rate before the backfill. [Decision D10](../decisions.md#d10-read-prices-from-the-runtime-api-not-from-v3-storage-math) leaves one question open. Do the candles for those specs use the per-subnet runtime API, or the squid's `Swap.AlphaSqrtPrice` math, whose item exists from block 5,947,548 (spec 290) to block 8,486,592 (spec 422)? Record the answer as a new decisions entry before the backfill starts.

**Values translated in place.** [D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec) decodes the storage and events of block n with the metadata of the spec that executed n, and runtime API calls at n with the metadata of n's own state spec. No release 1 item changes its encoding in place across an upgrade, so release 1 does not need to tell the two apart. Release 2 reads every spec from 219 on. Before its first full-history job, check each item it reads at each upgrade block that changes the item's type, and record whether any value was rewritten in place by the migration. The worked reference is the `chain-specs` probe (`spikes/crates/chain-specs`, `P31`, `D-scan`).

**`stake_at_epoch` before spec 261.** `TotalHotkeyAlphaLastEpoch` first appears at spec 261 (block 5,328,895). Before it the decoder returns `None` ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)). Release 1 never reads that era, and its `epoch_dividend.stake_at_epoch` column is `NOT NULL`. A full-history `epochs` job reads it. Decide what the row stores there, a nullable column or no row, and record it as a new decisions entry.

## R2.1. Index events

1. The `events` job on the runner, decoding each block's events with the metadata of the spec that executed it, which is the state spec of its parent ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)). Decode through `OfflineClient` with that metadata registered first, as step 17 does. A cold `OnlineClient` files the upgrade block's metadata under the executing spec and then fails at that spec's own blocks (`spikes/crates/chain-specs`, `online-metadata-miss`).
2. A decoder struct and a fixture test for every event shape in `docs/shape-changes.md`.
3. The fact tables `staking_action`, `transfer`, and `basket_flow`.
4. `prices` switched to `Full` history, once the decisions entry on prices for specs 290 to 390 exists.
5. The check. Index the same blocks twice, then backfill `events` from block 4,920,350 with zero decode errors and zero gaps, then follow the head with lag below 5 for one hour.

Release 1 provides the runner and its tests (steps 24 to 31), history windows (step 33), the parent-metadata proof (step 17), `docs/shape-changes.md` (step 22), `fixture record` and `FixtureSource` (steps 14 and 15), the spec table seeded from `fixtures/specs.csv`, whose base row is spec 219 (step 13, [D8](../decisions.md#d8-find-the-spec-table-by-bisection-over-code-and-persist-only-the-specs-a-job-window-touches)), and the decisions entry on historic decoding (step 23).

## R2.2. Positions and reconciliation

1. The position folder, the one worker that runs in strict block order, with `checked_sub` and drift on underflow.
2. `reconcile_request`, `position_settle`, and the reconciler, with the rules of [D13](../decisions.md#d13-correct-positions-from-chain-state-on-a-schedule).
3. `subnet`, `subnet_snapshot`, and daily holder counts.
4. The check. Folded positions for 100 sampled coldkeys match `get_stake_info_for_coldkeys` at the head.

Release 1 provides runtime API calls through `BlockSource` (step 8), paged storage reads (step 21), checked `u128` math under the deny lint (step 34), and one writer per table.

## R2.3. Native analytics and the native API

1. The candle aggregates over `subnet_price`.
2. The ported `Balancer` and its differential test against `sim_swap`.
3. The native API under `/api/v1`, with its own DTOs.
4. The check. The differential test, candles against a hand-computed day, p99 below 100 ms on `/api/v1/subnets`, and the endpoint list matching `openapi.json`.

Release 1 provides the problem+json errors and the middleware stack (step 43), the OpenAPI document and its CI check (steps 51 and 58), and the load test script (step 52).

## Chain facts that matter only in release 2

Release 1 reads specs 475 to 490 only ([D3](../decisions.md#d3-set-the-milestone-scope-by-what-release-1-reads)). These facts, measured on 2026-11-12, bite once a job reads older blocks.

- **Block 4,920,350 is an upgrade block executed by spec 219.** Its state reports spec 233, but its events need spec 219's metadata, so the `events` backfill needs a spec 219 fixture. At the 106 upgrade blocks from 4,920,350 on, the parent's metadata decoded every block's events, and the block's own metadata failed at 13 (`spikes/crates/chain-specs`, `D-scan`).
- **Storage at an upgrade block is pre-migration.** At 4,920,350, 5,947,548, and 8,486,593, `System.LastRuntimeUpgrade` still names the old spec, and items the new spec adds have 0 keys until the next block (`spikes/crates/chain-specs`, `P31`, `P31-290`, `P31-423`). An item the executing spec does not declare is absent, not zero. What a runtime API that the new spec adds returns at the upgrade block itself is not yet known.
- **Event fixtures sit on both sides of the upgrade block.** The upgrade block `first_block` still emits events in the old shape. The new shape can first appear at `first_block + 1`.
- **`SubnetIdentitiesV3` is declared but empty from spec 290 to about spec 352.** At 5,947,549 V3 holds 0 rows and V2 holds 112. By spec 352, V3 holds 121 and V2 holds 2. A historic identity read must not pick V3 because it is declared (`spikes/crates/chain-decode`, `identities-declared-empty`).
- **Stale Provided keys after spec 423.** `SubnetTaoProvided` and `SubnetAlphaInProvided` leave the metadata at spec 423 (block 8,486,593) but keep 129 keys, all zero, in storage. A reader that iterates by prefix sees them (`spikes/crates/chain-specs`, `P31-423`).
- **Baskets drop `weights` at spec 464** (block 9,088,597). Mainnet ran 459 and then 464. One struct without `weights` decodes both shapes, because `DecodeAsType` skips the field (`spikes/crates/chain-decode`, `era-basket-weights`, `basket-one-struct`). A `Full` `baskets` history also needs step 38's `basket_floor(&VersionTable)`, `first_block + 1` of spec 441, because at the upgrade block the executing spec, 440, does not declare `BasketRate`.
- **A full backfill crosses every upgrade.** The spec table rules out a new spec with one `:code` hash check per range end and per new head ([D8](../decisions.md#d8-find-the-spec-table-by-bisection-over-code-and-persist-only-the-specs-a-job-window-touches)), so crossing 106 boundaries costs no extra discovery.
