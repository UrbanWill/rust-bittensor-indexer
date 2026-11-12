# Storage

The indexer uses one PostgreSQL 17 database with the TimescaleDB extension. This document explains which kind of table holds which kind of data, gives the schema for both releases, and estimates the size.

## Match the table kind to the data

The data falls into four kinds. Each kind gets the storage that fits how the API reads it.

| Data | Example | How the API reads it | Storage |
|---|---|---|---|
| Current state | `validator`, `subnet_identity`, `position` | One row by key, or the top N by a column | Plain table with B-tree indexes |
| Measurements over time | `subnet_price`, `epoch_dividend`, `basket_snapshot` | A time range for one subnet or hotkey | Hypertable on `ts`, with a retention policy for windowed jobs |
| Facts over time (release 2) | `staking_action`, `transfer`, `basket_flow` | Rows for one account, newest first, a page at a time | Hypertable on `ts`, with indexes on the account columns |
| Aggregates (release 2) | OHLC candles, daily holder counts | A time range at one resolution | Continuous aggregate, or a table that a job fills |

A hypertable is a TimescaleDB table split into chunks by time. A query for one week reads only the chunks for that week. A compressed chunk stores each column separately and takes about a tenth of the space, according to TimescaleDB's documentation. Segmenting by `netuid` keeps each subnet's rows together, so a query for one subnet decompresses only that subnet's rows.

A continuous aggregate is a materialized view that TimescaleDB refreshes on a schedule, and only for the chunks that changed. Candles for one subnet over a year then read 365 daily rows, not 2.6 million price points.

### Why one database

TimescaleDB is still PostgreSQL. `sqlx` checks every query against it at compile time, one backup covers all the data, and one transaction can write a fact and the checkpoint that covers it. A column store such as ClickHouse would scan faster, but it adds a second system to run, back up, and keep consistent with PostgreSQL. At the size estimated below, PostgreSQL with compression is enough. The [decisions log](../decisions.md#d4-one-postgresql-database-with-timescaledb) records the trade.

Release 1 keeps no response cache and runs no Redis. The [decisions log](../decisions.md#d14-shed-load-and-do-not-rate-limit-or-cache-in-process) explains why.

## Store values in exact types

| Value | Column type | Rule |
|---|---|---|
| Rao and alpha amounts that fit `u64` | `BIGINT` with `CHECK (x >= 0)` | `u64` on chain. Total TAO supply is 2.1 × 10^16 rao, below the `i64` limit of 9.2 × 10^18. The store converts with `i64::try_from` and returns `StoreError::Overflow` on failure. It never uses `as` |
| Signed amounts, such as drift and flow | `BIGINT` | |
| `u128` values and fixed-point raw bits (`I96F32`, `I64F64`) | `NUMERIC(39,0)` | The raw integer. The API converts it. sqlx needs its `bigdecimal` feature for this type |
| Prices | `BIGINT` | Rao per whole alpha |
| Accounts | `BYTEA` with `CHECK (length(x) = 32)` | The raw public key. The API renders SS58 with prefix 42 |
| Hashes | `BYTEA` with `CHECK (length(x) = 32)` | |
| Block numbers | `INTEGER` | `u32` on chain, so `i32` lasts until block 2.1 × 10^9 |
| Netuid | `INTEGER` with `CHECK (netuid BETWEEN 0 AND 65535)` | `u16` on chain. `INTEGER` holds every `u16`, so no netuid overflows |
| Raw `u16` values, such as a take | `INTEGER` with `CHECK (x BETWEEN 0 AND 65535)` | |
| Time | `TIMESTAMPTZ` | From `Timestamp.Now` in the block |

Accounts are stored as bytes, not SS58 strings. A 32-byte key takes less space and indexes faster than a 48-character string. One account also has only one byte form, while one key can render as several SS58 strings under different prefixes.

No column holds a floating-point amount. The API computes ratios and APYs in `float8` at query time and never writes them back.

### Write batches with UNNEST

Writers insert a batch with `INSERT ... SELECT * FROM UNNEST($1::bytea[], $2::integer[], ...)`. `query!` infers each array parameter as non-null, so a nullable column needs an `as _` override, which turns off its compile-time check. Two columns of the same type, such as `coldkey` and `hotkey`, can also be swapped without a compile error. Two rules close both gaps.

- One function per table builds every column array of a batch from the row structs, such as `fn columns(rows: &[StakingActionRow]) -> StakingActionCols`. No other code builds the arrays.
- Each batch table has a round-trip test. It writes rows with a distinct value in every column, nulls included, reads them back, and compares them.

## Release 1 schema

The migrations in `crates/store/migrations/` are the source of truth. This section gives every release 1 table in full.

### Pipeline state

```sql
CREATE TABLE runtime_version (
	spec_version  INTEGER PRIMARY KEY,
	first_block   INTEGER NOT NULL UNIQUE,
	metadata      BYTEA NOT NULL,
	fetched_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE checkpoint (
	worker          TEXT PRIMARY KEY,
	mode            TEXT NOT NULL CHECK (mode IN ('backfill', 'live')),
	start_block     INTEGER NOT NULL,
	last_block      INTEGER NOT NULL,
	last_block_ts   TIMESTAMPTZ NOT NULL,
	target_block    INTEGER NOT NULL,
	blocks_per_sec  REAL NOT NULL,
	updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
	CHECK (start_block <= last_block + 1)
);

CREATE TABLE chain_head (
	id               INTEGER PRIMARY KEY CHECK (id = 1),
	finalized_block  INTEGER NOT NULL,
	finalized_ts     TIMESTAMPTZ NOT NULL,
	updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

`runtime_version.first_block` is the first block that the spec executed. [Chain data](02-chain-data.md#give-subxt-the-spec-and-metadata) explains how the decoder uses it.

A job creates its `checkpoint` row on its first run with `last_block = start_block − 1`, so `start_block` is fixed from then on. [Ingestion](03-ingestion.md#see-where-every-job-is) describes every column.

### Prices

```sql
CREATE TABLE subnet_price (
	ts               TIMESTAMPTZ NOT NULL,
	block_number     INTEGER NOT NULL,
	netuid           INTEGER NOT NULL CHECK (netuid BETWEEN 1 AND 65535),
	price            BIGINT NOT NULL CHECK (price >= 0),
	tao_reserve      BIGINT NOT NULL CHECK (tao_reserve >= 0),
	alpha_reserve    BIGINT NOT NULL CHECK (alpha_reserve >= 0),
	alpha_out        BIGINT NOT NULL CHECK (alpha_out >= 0),
	lifetime_volume  NUMERIC(39,0) NOT NULL CHECK (lifetime_volume >= 0),
	PRIMARY KEY (netuid, ts)
);
SELECT create_hypertable('subnet_price', 'ts', chunk_time_interval => INTERVAL '1 day');
ALTER TABLE subnet_price SET (
	timescaledb.compress,
	timescaledb.compress_segmentby = 'netuid',
	timescaledb.compress_orderby = 'ts DESC'
);
SELECT add_compression_policy('subnet_price', INTERVAL '2 days');
SELECT add_retention_policy('subnet_price', INTERVAL '4 days');
```

`price` is rao per whole alpha. `tao_reserve` is `SubnetTAO` plus `SubnetTaoProvided`, and `alpha_reserve` is `SubnetAlphaIn` plus `SubnetAlphaInProvided`. The provided terms are 0 when the block's metadata lacks them. `lifetime_volume` is the `u128` `SubnetVolume` counter. Root has no rows, so `netuid` starts at 1.

TimescaleDB requires the time column in every unique index, so `ts` is part of the primary key. One block has one timestamp, so `(netuid, ts)` is unique. `create_hypertable` also creates an index on `ts DESC`, which finds the newest row for `/pools`.

`/pools` reads the last 26 hours, which are never compressed. Release 2 sets `prices` to `Full` history and removes the retention policy.

### Validators

```sql
CREATE TABLE validator (
	hotkey                 BYTEA PRIMARY KEY CHECK (length(hotkey) = 32),
	owner_coldkey          BYTEA CHECK (length(owner_coldkey) = 32),
	name                   TEXT,
	take                   INTEGER NOT NULL CHECK (take BETWEEN 0 AND 65535),
	global_weighted_stake  NUMERIC(39,0) NOT NULL CHECK (global_weighted_stake >= 0),
	global_nominators      INTEGER NOT NULL CHECK (global_nominators >= 0),
	active_subnets         INTEGER NOT NULL CHECK (active_subnets >= 0),
	snapshot_block         INTEGER NOT NULL
);
CREATE INDEX ON validator (global_weighted_stake DESC, hotkey);

CREATE TABLE validator_subnet (
	hotkey          BYTEA NOT NULL CHECK (length(hotkey) = 32),
	netuid          INTEGER NOT NULL CHECK (netuid BETWEEN 0 AND 65535),
	alpha_stake     BIGINT NOT NULL CHECK (alpha_stake >= 0),
	snapshot_block  INTEGER NOT NULL,
	PRIMARY KEY (hotkey, netuid)
);
CREATE INDEX ON validator_subnet (netuid, alpha_stake DESC, hotkey);
```

`take` is the raw `Delegates` value. `global_weighted_stake` is the squid's display figure in rao, not the chain's stake weight. [Compatibility numbers](08-analytics.md#compatibility-numbers) defines it. The `validators` job replaces both tables in one transaction, so a reader never sees half of a snapshot.

### Epoch dividends

```sql
CREATE TABLE epoch_dividend (
	ts              TIMESTAMPTZ NOT NULL,
	block_number    INTEGER NOT NULL,
	netuid          INTEGER NOT NULL CHECK (netuid BETWEEN 1 AND 65535),
	hotkey          BYTEA NOT NULL CHECK (length(hotkey) = 32),
	dividend        BIGINT NOT NULL CHECK (dividend > 0),
	stake_at_epoch  BIGINT NOT NULL CHECK (stake_at_epoch >= 0),
	PRIMARY KEY (netuid, hotkey, ts)
);
SELECT create_hypertable('epoch_dividend', 'ts', chunk_time_interval => INTERVAL '7 days');
CREATE INDEX ON epoch_dividend (netuid, ts DESC);
SELECT add_retention_policy('epoch_dividend', INTERVAL '33 days');
```

The job writes only nonzero dividends and skips root. The `(netuid, ts)` index serves the per-subnet count of epochs and the earliest epoch in the window.

### Baskets

```sql
CREATE TABLE basket_snapshot (
	ts            TIMESTAMPTZ NOT NULL,
	block_number  INTEGER NOT NULL,
	hotkey        BYTEA NOT NULL CHECK (length(hotkey) = 32),
	rate_bits     NUMERIC(39,0) NOT NULL,
	shares        NUMERIC(39,0) NOT NULL CHECK (shares >= 0),
	nav_tao       BIGINT NOT NULL CHECK (nav_tao >= 0),
	PRIMARY KEY (hotkey, ts)
);
SELECT create_hypertable('basket_snapshot', 'ts', chunk_time_interval => INTERVAL '7 days');
SELECT add_retention_policy('basket_snapshot', INTERVAL '33 days');
```

`rate_bits` is the raw `I96F32` of `BasketRate`, or 0 when the item is missing. `shares` is a `u64` on chain.

### Subnet identities

```sql
CREATE TABLE subnet_identity (
	netuid         INTEGER PRIMARY KEY CHECK (netuid BETWEEN 0 AND 65535),
	name           TEXT,
	description    TEXT,
	url            TEXT,
	github_repo    TEXT,
	discord        TEXT,
	contact        TEXT,
	logo_url       TEXT,
	additional     TEXT,
	updated_block  INTEGER NOT NULL
);
```

Each text column is `NULL` when the decoded string is empty. A subnet whose identity exists with every field empty still has a row. `updated_block` is the block where the row last changed. The `identities` job writes only rows that changed, and deletes rows for subnets that no longer have an identity.

### Drop old chunks with retention policies

Each windowed table drops chunks older than its window plus 2 days. The margin keeps a full window while the job runs a little behind, and a chunk is dropped whole, so rows can outlive the policy by up to one chunk.

| Table | Job history | Retention |
|---|---|---|
| `subnet_price` | `Window(2 days)` | 4 days. None in release 2, where the job keeps `Full` history |
| `epoch_dividend` | `Window(31 days)` | 33 days |
| `basket_snapshot` | `Window(31 days)` | 33 days |
| `validator`, `validator_subnet`, `subnet_identity` | `Latest` | none. The job replaces the rows |

If `jobs.prices.history` changes, a migration changes the policy to match.

## Release 2 schema

Release 2 adds the `events` and `subnets` jobs, the position folder, and the reconciler. [Ingestion](03-ingestion.md#correct-positions-from-chain-state) says which worker writes which table.

### Facts

```sql
CREATE TABLE block (
	number        INTEGER PRIMARY KEY,
	hash          BYTEA NOT NULL UNIQUE CHECK (length(hash) = 32),
	parent_hash   BYTEA NOT NULL CHECK (length(parent_hash) = 32),
	ts            TIMESTAMPTZ NOT NULL,
	spec_version  INTEGER NOT NULL REFERENCES runtime_version
);

CREATE TYPE staking_kind AS ENUM (
	'stake_added', 'stake_removed', 'stake_moved', 'stake_transferred',
	'stake_swapped', 'alpha_recycled', 'alpha_burned'
);

CREATE TABLE staking_action (
	ts               TIMESTAMPTZ NOT NULL,
	block_number     INTEGER NOT NULL,
	event_index      INTEGER NOT NULL,
	extrinsic_index  INTEGER,
	kind             staking_kind NOT NULL,
	coldkey          BYTEA NOT NULL CHECK (length(coldkey) = 32),
	hotkey           BYTEA NOT NULL CHECK (length(hotkey) = 32),
	netuid           INTEGER NOT NULL CHECK (netuid BETWEEN 0 AND 65535),
	tao              BIGINT CHECK (tao >= 0),
	alpha            BIGINT CHECK (alpha >= 0),
	fee              BIGINT CHECK (fee >= 0),
	dest_coldkey     BYTEA CHECK (length(dest_coldkey) = 32),
	dest_hotkey      BYTEA CHECK (length(dest_hotkey) = 32),
	dest_netuid      INTEGER CHECK (dest_netuid BETWEEN 0 AND 65535),
	PRIMARY KEY (ts, block_number, event_index)
);
SELECT create_hypertable('staking_action', 'ts', chunk_time_interval => INTERVAL '7 days');
CREATE INDEX ON staking_action (coldkey, ts DESC);
CREATE INDEX ON staking_action (hotkey, ts DESC);
CREATE INDEX ON staking_action (netuid, ts DESC);

CREATE TABLE transfer (
	ts            TIMESTAMPTZ NOT NULL,
	block_number  INTEGER NOT NULL,
	event_index   INTEGER NOT NULL,
	from_account  BYTEA NOT NULL CHECK (length(from_account) = 32),
	to_account    BYTEA NOT NULL CHECK (length(to_account) = 32),
	amount        BIGINT NOT NULL CHECK (amount >= 0),
	PRIMARY KEY (ts, block_number, event_index)
);
SELECT create_hypertable('transfer', 'ts', chunk_time_interval => INTERVAL '7 days');
CREATE INDEX ON transfer (from_account, ts DESC);
CREATE INDEX ON transfer (to_account, ts DESC);

CREATE TYPE basket_flow_kind AS ENUM (
	'deposited', 'staked_in', 'claimed', 'holding_converted'
);

CREATE TABLE basket_flow (
	ts            TIMESTAMPTZ NOT NULL,
	block_number  INTEGER NOT NULL,
	event_index   INTEGER NOT NULL,
	kind          basket_flow_kind NOT NULL,
	hotkey        BYTEA NOT NULL CHECK (length(hotkey) = 32),
	coldkey       BYTEA CHECK (length(coldkey) = 32),
	netuid        INTEGER CHECK (netuid BETWEEN 0 AND 65535),
	tao           BIGINT NOT NULL CHECK (tao >= 0),
	value_tao     BIGINT CHECK (value_tao >= 0),
	shares        NUMERIC(39,0) CHECK (shares >= 0),
	PRIMARY KEY (ts, block_number, event_index)
);
SELECT create_hypertable('basket_flow', 'ts', chunk_time_interval => INTERVAL '7 days');
CREATE INDEX ON basket_flow (hotkey, ts DESC);
CREATE INDEX ON basket_flow (coldkey, ts DESC) WHERE coldkey IS NOT NULL;

CREATE TYPE position_event_kind AS ENUM (
	'hotkey_swapped', 'hotkey_swapped_on_subnet', 'coldkey_swapped',
	'root_claimed', 'liquidity_changed',
	'network_added', 'network_removed', 'network_cleanup_completed'
);

CREATE TABLE position_event (
	block_number  INTEGER NOT NULL,
	event_index   INTEGER NOT NULL,
	kind          position_event_kind NOT NULL,
	netuid        INTEGER CHECK (netuid BETWEEN 0 AND 65535),
	coldkey       BYTEA CHECK (length(coldkey) = 32),
	new_coldkey   BYTEA CHECK (length(new_coldkey) = 32),
	hotkey        BYTEA CHECK (length(hotkey) = 32),
	new_hotkey    BYTEA CHECK (length(new_hotkey) = 32),
	mechanism     INTEGER CHECK (mechanism BETWEEN 0 AND 65535),
	PRIMARY KEY (block_number, event_index)
);
```

TimescaleDB requires the time column in every unique index, so `ts` leads the primary key of each hypertable. `(block_number, event_index)` is unique on its own, because one block has one timestamp.

`staking_action.tao` and `alpha` are `NULL` where the event does not carry them. The linking events `stake_moved`, `stake_transferred`, and `stake_swapped` carry TAO only. `transfer` holds `Balances.Transfer`.

`position_event` holds the events that change positions or subnets without carrying an amount. The folder reads it in block order. It is a plain table, because the folder reads it by block and nothing else reads it by time.

### Reconcile

```sql
CREATE TABLE reconcile_request (
	block_number  INTEGER NOT NULL,
	coldkey       BYTEA NOT NULL CHECK (length(coldkey) = 32),
	reason        TEXT NOT NULL,
	fulfilled_at  TIMESTAMPTZ,
	PRIMARY KEY (block_number, coldkey)
);
CREATE INDEX ON reconcile_request (block_number) WHERE fulfilled_at IS NULL;

CREATE TABLE position_settle (
	block_number  INTEGER NOT NULL,
	coldkey       BYTEA NOT NULL CHECK (length(coldkey) = 32),
	hotkey        BYTEA NOT NULL CHECK (length(hotkey) = 32),
	netuid        INTEGER NOT NULL CHECK (netuid BETWEEN 0 AND 65535),
	alpha         BIGINT NOT NULL CHECK (alpha >= 0),
	drift         BIGINT,
	PRIMARY KEY (block_number, coldkey, hotkey, netuid)
);
```

The folder inserts `reconcile_request` rows. The reconciler sets `fulfilled_at`. The reconciler inserts `position_settle` rows, and the folder sets `drift` when it applies the settle. `drift` is the settled value minus the folded value, in alpha units of 10^-9.

### Current state

```sql
CREATE TABLE subnet (
	netuid            INTEGER NOT NULL CHECK (netuid BETWEEN 0 AND 65535),
	registered_block  INTEGER NOT NULL,
	removed_block     INTEGER,
	cleanup_block     INTEGER,
	mechanism         INTEGER NOT NULL CHECK (mechanism BETWEEN 0 AND 65535),
	PRIMARY KEY (netuid, registered_block),
	CHECK (removed_block IS NULL OR removed_block >= registered_block)
);
CREATE UNIQUE INDEX ON subnet (netuid) WHERE removed_block IS NULL;

CREATE TABLE position (
	coldkey            BYTEA NOT NULL CHECK (length(coldkey) = 32),
	hotkey             BYTEA NOT NULL CHECK (length(hotkey) = 32),
	netuid             INTEGER NOT NULL CHECK (netuid BETWEEN 0 AND 65535),
	alpha              BIGINT NOT NULL CHECK (alpha >= 0),
	shortfall          BIGINT NOT NULL DEFAULT 0 CHECK (shortfall >= 0),
	last_action_block  INTEGER NOT NULL,
	last_settle_block  INTEGER,
	PRIMARY KEY (coldkey, hotkey, netuid)
);
CREATE INDEX ON position (netuid, alpha DESC) WHERE alpha > 0;
CREATE INDEX ON position (hotkey, netuid);

CREATE TABLE subnet_holder_count (
	netuid   INTEGER NOT NULL CHECK (netuid BETWEEN 0 AND 65535),
	day      DATE NOT NULL,
	holders  INTEGER NOT NULL CHECK (holders >= 0),
	PRIMARY KEY (netuid, day)
);
```

A netuid can be reused after its cleanup, so `subnet` is keyed by `(netuid, registered_block)`. The partial unique index allows one open row per netuid. Genesis subnets that exist before block 4,920,350 get a row with `registered_block` 4,920,350, read from `NetworksAdded` and `SubnetMechanism` when the folder starts.

A position whose alpha reaches 0 stays as a row with `alpha = 0`. The partial index on `alpha > 0` keeps holder queries fast. `shortfall` is the alpha the folder could not subtract since the last settle. [Ingestion](03-ingestion.md#subtract-with-checked_sub) explains it.

The folder writes one `subnet_holder_count` row per subnet at the first block of each UTC day. A holder is a coldkey whose summed alpha on the subnet is above 0.

### Subnet snapshots

```sql
CREATE TABLE subnet_snapshot (
	ts                  TIMESTAMPTZ NOT NULL,
	block_number        INTEGER NOT NULL,
	netuid              INTEGER NOT NULL CHECK (netuid BETWEEN 1 AND 65535),
	tao_in              BIGINT NOT NULL CHECK (tao_in >= 0),
	alpha_in            BIGINT NOT NULL CHECK (alpha_in >= 0),
	alpha_out           BIGINT NOT NULL CHECK (alpha_out >= 0),
	moving_price_bits   NUMERIC(39,0) NOT NULL,
	ema_tao_flow_bits   NUMERIC(39,0),
	tao_in_emission     BIGINT NOT NULL CHECK (tao_in_emission >= 0),
	alpha_in_emission   BIGINT NOT NULL CHECK (alpha_in_emission >= 0),
	alpha_out_emission  BIGINT NOT NULL CHECK (alpha_out_emission >= 0),
	excess_tao          BIGINT CHECK (excess_tao >= 0),
	emission_enabled    BOOLEAN,
	lifetime_volume     NUMERIC(39,0) NOT NULL CHECK (lifetime_volume >= 0),
	tempo               INTEGER NOT NULL CHECK (tempo BETWEEN 0 AND 65535),
	fee_rate            INTEGER CHECK (fee_rate BETWEEN 0 AND 65535),
	PRIMARY KEY (netuid, ts)
);
SELECT create_hypertable('subnet_snapshot', 'ts', chunk_time_interval => INTERVAL '30 days');
ALTER TABLE subnet_snapshot SET (
	timescaledb.compress,
	timescaledb.compress_segmentby = 'netuid',
	timescaledb.compress_orderby = 'ts DESC'
);
SELECT add_compression_policy('subnet_snapshot', INTERVAL '60 days');
```

`ema_tao_flow_bits` is the raw `I64F64` from the second field of `SubnetEmaTaoFlow`, and `NULL` before spec 334 or when the item is absent. Columns for items that a spec lacks are `NULL`.

### Aggregates

Each continuous aggregate is created `WITH NO DATA`, in its own migration file that starts with `-- no-transaction`. sqlx runs such a file as one simple query, and `CREATE MATERIALIZED VIEW ... WITH DATA` cannot run inside the implicit transaction of a multi-statement query. With `WITH NO DATA`, the file can also hold the refresh policy and the `materialized_only` setting.

```sql
-- no-transaction
CREATE MATERIALIZED VIEW IF NOT EXISTS candle_1m
WITH (timescaledb.continuous, timescaledb.materialized_only = false) AS
SELECT
	netuid,
	time_bucket('1 minute', ts)       AS bucket,
	first(price, ts)                  AS open,
	max(price)                        AS high,
	min(price)                        AS low,
	last(price, ts)                   AS close,
	first(lifetime_volume, ts)        AS volume_open,
	last(lifetime_volume, ts)         AS volume_close,
	count(*)                          AS points
FROM subnet_price
GROUP BY netuid, bucket
WITH NO DATA;

SELECT add_continuous_aggregate_policy('candle_1m',
	start_offset => INTERVAL '1 hour',
	end_offset => INTERVAL '1 minute',
	schedule_interval => INTERVAL '1 minute',
	if_not_exists => true);
```

`materialized_only = false` is not the default, so each view sets it. A query then also sees the newest bucket that has not been refreshed yet.

A migration run outside a transaction is not atomic with sqlx's insert into `_sqlx_migrations`. A crash between the two reruns the file, so each statement uses `IF NOT EXISTS` or `if_not_exists => true`.

`candle_5m`, `candle_15m`, and `candle_1h` aggregate `candle_1m`. `candle_4h`, `candle_1d`, and `candle_1w` aggregate `candle_1h`. Each level reads the level below it, not the raw prices, and takes `first(volume_open)` and `last(volume_close)` of its inputs.

A candle's volume is `volume_close` minus the previous candle's `volume_close`, from the `SubnetVolume` counter. A sum of stake events would miss every swap that emits no stake event. [Chain data](02-chain-data.md#volume-comes-from-subnetvolume) explains why. The API computes the difference at read time and reports 0 when the counter went down.

## Run migrations as a separate step

`bt-indexer migrate` applies the migrations in `crates/store/migrations/` with `sqlx::migrate!`. Neither `ingest` nor `serve` applies migrations at start. Each checks that the database is at the version it was built for and exits with an error if it is not. Two processes that both migrate at start can race. A deploy that migrates as its own step can also stop before the new code runs.

Migrations move forward only. To undo one, write a new one.

## Size the database

These numbers are estimates, not measurements. Replace them with `hypertable_detailed_size` output after the first backfill. Each row size includes about 40 bytes of primary key index.

### Release 1

- **`subnet_price`** has one row per subnet per block. Two days are about 129 × 14,400 = 1.9 million rows. Retention keeps up to 4 days plus one 1-day chunk, about 4.6 million rows. At about 130 bytes per row, that is about 600 MB before compression, and less once chunks older than 2 days compress.
- **`epoch_dividend`** has one row per validator with a nonzero dividend per subnet epoch. About 128 subnets at a tempo of 360 run about 2,550 epochs per day. At about 30 paid validators per epoch, a guess, that is about 77,000 rows per day, and about 3 million rows over 33 days plus one chunk. At about 120 bytes per row, that is about 360 MB.
- **`basket_snapshot`** has one row per validator with a basket every 500 blocks, about 14 per day. At 50 validators, a guess, 40 days hold about 29,000 rows.
- **`validator`**, **`validator_subnet`**, and **`subnet_identity`** hold the current set only, a few thousand rows.
- **`runtime_version`** holds about 1 MB of metadata per spec. Release 1 meets only the few specs in its windows.

Release 1 needs about 1 GB. Plan for 10 GB of disk.

### Release 2

- **`subnet_price`** keeps full history. About 4.69 million blocks since the dTAO start, times an average of about 100 subnets over that period, gives about 470 million rows. The average subnet count is a guess, since the count grew from 64 to about 129. At about 130 bytes per row, that is about 61 GB uncompressed. At the 10 times compression that TimescaleDB documents, it is about 6 GB.
- **`subnet_snapshot`** has about 100 rows every 100 blocks, about 4.7 million rows and under 1 GB.
- **`staking_action`**, **`transfer`**, and **`basket_flow`** depend on how many events the chain emitted, which we have not measured. Count the events in a sample of 10,000 blocks during the first backfill, then extrapolate.
- **`block`** has 4.69 million rows of about 120 bytes, about 560 MB.
- **`position`** and **`position_settle`** depend on the coldkey count. The daily reconcile writes one settle per position per day.
- **`runtime_version`** holds fewer than 300 specs, under 300 MB.

Plan for 100 GB of disk for release 2, and check the estimate after the first backfill.
