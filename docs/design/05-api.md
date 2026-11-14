# API

`bt-indexer serve` runs an Axum 0.8 HTTP server that reads from the database. In release 1 it serves the five routes that tao-data-api calls, plus the operational endpoints. Release 2 adds the native API under `/api/v1`. This document covers the routes, their frozen response types, readiness, the middleware stack, and shutdown. [Error handling](06-error-handling.md) covers the error types behind the error responses.

Compat claims cite two repositories by name. `bittensor-squid/...` is a checkout of the reference squid, and `tao-data-api/...` is a checkout of the consumer at commit `6a46bbf`.

## The API reads and never writes

The server connects with a PostgreSQL role that has `SELECT` rights only. A bug in a handler cannot change indexed data. The role is granted with `ALTER DEFAULT PRIVILEGES FOR ROLE <migrate role>`, so it can read every table and chunk that later migrations create, and an `INSERT` or `UPDATE` fails with 42501 ([D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests)). [Quality](09-quality.md#run-everything-with-docker-compose) gives the statements.

The server keeps its own `PgPool`, sized by `api.db_max_connections` (default 20). It reads `database.url`, the same key as `ingest`, and the deployment sets it to the read-only role's URL. It binds to `api.listen_addr`, default `0.0.0.0:8080` ([D30](../decisions.md#d30-read-the-node-urls-the-database-url-and-the-api-address-from-config)). `/metrics` is served on a separate port (`telemetry.metrics_addr`) that is not exposed to the public. `serve` overrides its default of `0.0.0.0:9100` to `0.0.0.0:9101`, because `ingest` exports on 9100 ([D31](../decisions.md#d31-export-ingest-metrics-on-9100-and-serve-metrics-on-9101)).

- Each connection sets `statement_timeout = '5s'`, so one slow query cannot hold a connection for long. `after_connect` with `SET statement_timeout = '5s'` and `PgConnectOptions::options([("statement_timeout", "5s")])` both set it on every pooled connection, and the second needs no extra round trip (`spikes/crates/store`, `statement-timeout-opt`). A query cut by the timeout fails with SQLSTATE 57014 after 5.0 s.
- `acquire_timeout` is 2 seconds. That is shorter than the 10 second request timeout, so a request that cannot get a connection fails with 503 instead of waiting out its whole budget. [Shed load instead of queueing](#shed-load-instead-of-queueing) explains why.

The server does not depend on the `chain` crate. It keeps serving while the node is down, and `/status` and `/readyz` then report the lag.

### Write each read so it uses an index

These forms used the indexes in `EXPLAIN` at synthetic release 1 sizes (4,644,000 price rows, 3,072,000 epoch rows, 28,000 basket rows, 5,000 validators and 40,000 validator-subnet rows), measured on 2026-11-12 (`spikes/crates/store`, `read-pools-dayago`, `read-root-apr`).

- **The `/pools` day-ago row.** Take each netuid at the tip, then `CROSS JOIN LATERAL (SELECT ... FROM subnet_price p WHERE p.netuid = c.netuid AND p.ts > tip − 26 h AND p.ts <= tip − 24 h ORDER BY p.ts DESC LIMIT 1)`. It reads one row per netuid through the primary key in 1.3 to 2.1 ms. `DISTINCT ON (netuid)` read 77,400 rows and sorted them, 31 to 44 ms.
- **The tip.** The newest `subnet_price` row comes from the `ts DESC` index that `create_hypertable` adds, in 0.22 ms.
- **`/validators`** uses the `(global_weighted_stake DESC, hotkey)` index, 0.72 ms. **`/subnets/{netuid}/validators`** uses `(netuid, alpha_stake DESC, hotkey)`, 1.2 ms. **`/validators/{hotkey}/subnets`** uses the `validator_subnet` primary key, 0.04 ms.
- **Stake and take in one statement.** The subnet and hotkey routes read `alpha_stake` from `validator_subnet` and `take` from `validator`. Read both in one statement with a join. Two statements can straddle a `validators` commit and pair a new stake with an old take.
- **Epoch rows for one subnet** use `epoch_dividend (netuid, ts DESC)` and return about 18,000 rows in 19 to 46 ms.
- **Epoch rows for one hotkey.** No index leads with `hotkey`. Write `WHERE netuid = ANY($netuids) AND hotkey = $1`, with the netuids from the hotkey's `validator_subnet` rows. That uses the primary key, 1.4 ms. `WHERE hotkey = $1` alone scans every chunk, 56 to 82 ms, and 166 to 170 ms after PostgreSQL switched the prepared statement to a generic plan on its sixth run.
- **The measurable test** (at least 10 distinct epoch times in 30 days) runs per netuid as `SELECT count(*) FROM (SELECT DISTINCT ts FROM epoch_dividend WHERE netuid = $n AND ts > H − 30 days ORDER BY ts DESC LIMIT 10)`, about 0.5 to 1 ms. `count(DISTINCT ts)` took 89 to 166 ms.
- **Root APR rows** use the `basket_snapshot` primary key `(hotkey, ts)`, 0.31 ms.

## Serve the compat routes at the host root

tao-data-api builds each indexer URL with `new URL(path, url)` (`tao-data-api/src/common/indexer.ts:107`). Every `path` it passes is absolute, such as `"/pools"` (`tao-data-api/src/endpoints/pools.ts:73`). An absolute path replaces the whole path of the base URL. A prefix on `INDEXER_DATA_API_1_URL`, such as `https://indexer.example/compat`, is therefore dropped, and the request goes to `https://indexer.example/pools`. The routes must sit at the host root, with the exact paths tao-data-api sends.

The native API of release 2 lives under `/api/v1`. The compat route `/v1/subnet-identities` occupies `/v1`, so a native `/v1` prefix would mix two contracts under one path.

## Compatibility routes

These five routes are the whole contract with tao-data-api. Their paths, response shapes, and values match the reference squid. Nothing else the squid serves is part of the contract. [Analytics](08-analytics.md#compatibility-numbers) defines how each value is computed.

| Route | tao-data-api caller | Squid handler |
|---|---|---|
| `GET /pools` | `tao-data-api/src/endpoints/pools.ts:73` | `bittensor-squid/api/src/routes/pools.ts:136-141` |
| `GET /validators` | `tao-data-api/src/endpoints/validators.ts:68` | `bittensor-squid/api/src/routes/validators.ts:297-310` |
| `GET /subnets/{netuid}/validators` | `tao-data-api/src/endpoints/subnetValidators.ts:93` | `bittensor-squid/api/src/routes/validators.ts:362-378` |
| `GET /validators/{hotkey}/subnets` | `tao-data-api/src/endpoints/validatorSubnets.ts:91` | `bittensor-squid/api/src/routes/validators.ts:430-446` |
| `GET /v1/subnet-identities` | `tao-data-api/src/endpoints/subnetIdentities.ts:97` | `bittensor-squid/api/src/routes/subnet-identities.ts:25-43` |

The squid also serves `/v1/` twins of the first four routes. tao-data-api never calls them, so we do not serve them.

### Rules for every compat route

- **The JSON type of each field is part of the contract.** tao-data-api parses every response with a Zod schema and throws when it fails (`tao-data-api/src/common/indexer.ts:51-55`). A number where the schema wants a string therefore becomes a 502 at tao-data-api, not a small formatting difference.
- **Extra fields are harmless.** Each schema is a plain `z.object`, which strips unknown keys. The squid sends fields that tao-data-api drops, such as `nominator_stake` and `basket`, and our DTOs leave them out. We do send `updatedAtBlock`, which tao-data-api strips, because the squid's golden test pins it.
- **Error bodies are never read.** tao-data-api throws on any non-2xx status (`tao-data-api/src/common/indexer.ts:47-49`). When the primary indexer fails, it cancels the body and tries the other mirror (`tao-data-api/src/common/indexer.ts:142-160`). We still send every error as `application/problem+json`, the same format as every other route.
- **Accounts are SS58 strings with prefix 42.**
- **No query parameters.** None of the five squid routes declares a query schema, and tao-data-api sends none. We ignore any that arrive.
- **Ties break by a second key.** The squid's orderings have no tiebreaker, so equal rows come back in an arbitrary order. We break ties by `hotkey`, or by `netuid` where the hotkey is fixed. Any order is valid for the squid, so the result stays compatible. Decision [D28](../decisions.md#d28-anchor-apy-on-the-indexed-head-and-break-rank-ties-by-hotkey) records the deviation.

### `GET /pools`

The body is a bare JSON array with one object per subnet (`bittensor-squid/api/src/schemas/pools.ts:3`).

| Field | JSON type | Unit and format | Null |
|---|---|---|---|
| `netuid` | number | The subnet id. tao-data-api requires a number (`tao-data-api/src/endpoints/pools.ts:22`) | Never |
| `total_tao` | string | Rao, an integer. It is the pool's TAO reserve (`bittensor-squid/api/src/routes/pools.ts:74`) | Never |
| `total_alpha` | string | Raw alpha (10^9 per alpha), an integer. It is the alpha reserve plus `alpha_out` (`pools.ts:75`) | Never |
| `price` | string | TAO per alpha as a decimal. At most 9 fraction digits, trailing zeros stripped, no `.` when the value is whole, and `"0"` for zero (`pools.ts:6-15`, `pools.ts:121`) | Never |
| `price_change_1_day` | string | Percent, so 5 % is `"5"`, not `"0.05"`. Truncated toward zero to at most 16 fraction digits, with trailing zeros stripped. A negative value has a `-` prefix. `"0"` when there is no price a day before, or that price is 0 (`pools.ts:87-108`) | Never |
| `market_cap` | string | Rao, not TAO, as a decimal with at most 9 fraction digits and trailing zeros stripped (`pools.ts:26-35`, `pools.ts:123`) | Never |
| `tao_volume_24_hr` | string | Rao, an integer. `"0"` when there is no row a day before, or the volume counter went down (`pools.ts:110-115`, `pools.ts:124`) | Never |

tao-data-api accepts a number or a string for every field except `netuid` (`tao-data-api/src/endpoints/pools.ts:19-29`). It passes the values on unchanged, so its clients see our strings. We send strings formatted exactly as above.

Rows and windows:

- **The tip** is the newest `subnet_price` row by `ts`. It gives the tip block and the tip time (`bittensor-squid/api/src/routes/pools.ts:50-55`).
- **One row per subnet** comes from the rows at exactly the tip block. A subnet with no row at the tip block is left out (`pools.ts:56-63`). Root (netuid 0) never has a row, so it never appears.
- **The day-ago row** for a subnet is its newest row with `tip time − 26 h < ts ≤ tip time − 24 h` (`pools.ts:64-71`). The window is anchored on the tip time, not on the wall clock.
- **Order** is `netuid` ascending (`pools.ts:82`).
- A subnet younger than about 24 hours, or one with a gap in the 24 to 26 hour window, reports `"0"` for both the price change and the volume.

tao-data-api's OpenAPI example for this route (`tao-data-api/src/endpoints/pools.ts:48-58`) shows TAO-like amounts and a fractional price change. It is wrong on units. Do not use it as a fixture.

### `GET /validators`

The body is a bare JSON array. The Zod schema is at `tao-data-api/src/endpoints/validators.ts:18-25`, and the squid's TypeBox schema at `bittensor-squid/api/src/schemas/validators.ts:4-23`.

| Field | JSON type | Unit and format | Null |
|---|---|---|---|
| `hotkey` | string | SS58, prefix 42 | Never |
| `name` | string or null | The identity name of the hotkey's owner, with NUL characters removed and whitespace trimmed (`bittensor-squid/api/src/routes/validators.ts:279`) | When the hotkey has no owner, the owner has no identity, or the name is empty |
| `global_weighted_stake` | **string** | Rao of TAO-equivalent stake, an integer (`validators.ts:265`). This is the squid's display figure, not the chain's stake weight. A number here fails the Zod schema | Never |
| `global_nominators` | number | An integer count of coldkeys (`validators.ts:282`) | Never |
| `active_subnets` | number | An integer count of subnets (`validators.ts:283`) | Never |
| `rank` | number | An integer from 1 to N (`validators.ts:270`, `validators.ts:285`) | Never |

Rows and order:

- **Filter.** Only validators with `global_weighted_stake > 0` (`validators.ts:272`).
- **Order.** `global_weighted_stake` descending (`validators.ts:273`), then `hotkey` ascending.
- **Rank.** The row's position in that order, after the filter. The squid uses `ROW_NUMBER()` with no tiebreaker (`validators.ts:270`), so its rank for tied rows is arbitrary. Ours is deterministic.
- We do not send the squid's `global_weighted_stake_nominators` and `active_subnets_nominators`. tao-data-api strips them.

### `GET /subnets/{netuid}/validators`

The body is a bare JSON array. The Zod schema is at `tao-data-api/src/endpoints/subnetValidators.ts:29-34`.

| Field | JSON type | Unit and format | Null |
|---|---|---|---|
| `hotkey` | string | SS58, prefix 42 | Never |
| `stake` | **string** | Raw alpha, an integer, from `TotalHotkeyAlpha(hotkey, netuid)` (`bittensor-squid/api/src/routes/validators.ts:329`). At netuid 0 it is rao of root TAO. A number here fails the Zod schema | Never |
| `thirty_day_apy` | number or null | A fraction, so 0.59 means 59 %, net of the validator's take. At netuid 0 it is the root basket APR | When there is not enough data |
| `thirty_day_apy_gross` | number or null | A fraction, before the take | When there is not enough data, when the take is 100 %, and always at netuid 0 (`validators.ts:203-207`) |

We always send `thirty_day_apy_gross`, even when it is null. The Zod schema also accepts a missing key, but the squid always sends it.

Input, rows, and order:

- **`netuid`** must be an integer from 0 to 65,535, as in the squid (`bittensor-squid/api/src/schemas/validators.ts:72-74`). Any other value gets 400. tao-data-api checks the same range before it calls (`tao-data-api/src/endpoints/subnetValidators.ts:46-57`).
- **An unknown netuid** returns `200 []`.
- **Filter.** Only rows with stake above 0 on that subnet (`validators.ts:337-339`).
- **Order.** Stake descending (`validators.ts:337-339`), then `hotkey` ascending.
- An APY of `0` means the subnet is measurable and the validator earned nothing. `null` means there is not enough data. [Analytics](08-analytics.md#compatibility-numbers) gives the 10-epoch rule and the root APR rules.
- We do not send the squid's `nominator_stake` and `basket`.

tao-data-api turns `stake` into a JavaScript number for its own clients (`tao-data-api/src/endpoints/subnetValidators.ts:99-106`). That loses precision above 2^53 raw alpha. The loss happens in tao-data-api, and the indexer still sends the exact string.

### `GET /validators/{hotkey}/subnets`

The body is a bare JSON array. The Zod schema is at `tao-data-api/src/endpoints/validatorSubnets.ts:32-37`.

| Field | JSON type | Unit and format | Null |
|---|---|---|---|
| `netuid` | number | The subnet id | Never |
| `stake` | **string** | Raw alpha, an integer. At netuid 0 it is rao of root TAO | Never |
| `thirty_day_apy` | number or null | As in `GET /subnets/{netuid}/validators` | As there |
| `thirty_day_apy_gross` | number or null | As in `GET /subnets/{netuid}/validators` | As there, so always null at netuid 0 |

Input, rows, and order:

- **`hotkey`** is any string in the squid, with no validation (`bittensor-squid/api/src/schemas/validators.ts:98-100`). The squid compares it as text, so a value that is not a stored address returns `200 []`. Our handler decodes the path as an SS58 prefix-42 account. A value that does not decode returns `200 []`, not 400. tao-data-api checks only a regex before it calls (`tao-data-api/src/endpoints/validatorSubnets.ts:50-53`). A string can pass that regex and still fail the SS58 checksum, and a 400 would become a 502 at tao-data-api.
- **An unknown hotkey** returns `200 []`.
- **Filter.** Only rows with stake above 0 (`bittensor-squid/api/src/routes/validators.ts:405-407`).
- **Order.** Stake descending (`validators.ts:405-407`), then `netuid` ascending. The order compares raw alpha across subnets with different prices, so it is not an order by value. We copy it.
- If the hotkey has root stake, the netuid 0 row appears, with the basket APR and a null gross APY.

### `GET /v1/subnet-identities`

The body is an object, `{"subnets": [...]}`, not a bare array (`bittensor-squid/api/src/schemas/subnet-identities.ts:8-29`). The keys are camelCase. tao-data-api renames them to snake_case for its own clients (`tao-data-api/src/endpoints/subnetIdentities.ts:102-108`). The Zod schema is at `tao-data-api/src/endpoints/subnetIdentities.ts:33-47`.

| Field | JSON type | Unit and format | Null |
|---|---|---|---|
| `netuid` | number | An integer subnet id | Never |
| `name`, `description`, `url`, `githubRepo`, `discord`, `contact`, `logoUrl`, `additional` | string or null | The identity text, decoded as UTF-8 with invalid bytes replaced by U+FFFD, NUL characters removed, and whitespace trimmed (`bittensor-squid/src/bytes.ts:2-16`) | When the field is empty after trimming. The key is always present, because the Zod schema fails on a missing key |
| `updatedAtBlock` | number | The block at which the identity last changed (`bittensor-squid/src/subnet-identity.ts:117-130`). tao-data-api strips it, and the squid's golden test pins it | Never |

Rows and order:

- **One object per subnet** that has a `SubnetIdentitiesV3` entry. A subnet whose fields are all empty is still listed, with every field null. A subnet with no entry, or one that was deregistered, is left out (`bittensor-squid/src/subnet-identity.ts:108-115`, `subnet-identity.ts:131-137`).
- **netuid 0** appears if root has an identity entry.
- **Order** is `netuid` ascending (`bittensor-squid/api/src/routes/subnet-identities.ts:19`).

## Freeze the compat DTOs

The compat routes live in `crates/api/src/compat/`, one module per route. Each response type is a plain struct with `Serialize`, written by hand to match the table above. The native API never reuses these types, and these types never reuse native ones. A change to a compat DTO is a breaking change to the contract, so it needs a matching change in tao-data-api first.

```rust
#[derive(Serialize, ToSchema)]
pub struct SubnetValidatorDto {
    pub hotkey: Hotkey,                      // serializes as the SS58 prefix-42 string
    pub stake: AmountString,                 // integer as a JSON string
    #[schema(required = true)]
    pub thirty_day_apy: Option<f64>,         // null, never omitted
    #[schema(required = true)]
    pub thirty_day_apy_gross: Option<f64>,
}

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(pools::list))
        .routes(routes!(validators::list))
        .routes(routes!(subnet_validators::list))
        .routes(routes!(validator_subnets::list))
        .routes(routes!(subnet_identities::list))
        .layer(axum::middleware::map_response(cache_control_on_success)) // 2xx only
}

/// Sets `Cache-Control: public, max-age=12` when `res.status().is_success()`.
async fn cache_control_on_success(res: Response) -> Response;

pub fn router(state: AppState) -> Router {
    let cfg = &state.config;
    let (compat, compat_doc) = compat::routes().split_for_parts();
    let (ops, ops_doc) = ops::routes().split_for_parts(); // /status, /healthz, /readyz
    compat
        .layer(middleware::shed(cfg))            // compat routes only
        .merge(ops)
        .merge(docs(compat_doc, ops_doc))        // /docs and /openapi.json
        .fallback(|| async { ApiError::NotFound { resource: "route" } })
        .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
        .layer(middleware::common())             // every route
        .with_state(state)
}

#[derive(Clone)]
pub struct AppState {
    db: PgPool,
    config: Arc<ApiConfig>,
    basket_floor: Option<BlockNumber>,
}
```

`OpenApiRouter`, `routes!` and `split_for_parts` come from `utoipa-axum` 0.3.0. [Middleware stack](#middleware-stack) gives the layers inside `shed` and `common`. The two-part router, with shed compat routes and unshed ops routes, compiles and serves with axum 0.8.9 (`spikes/crates/api`, `healthz-shed`).

- `AmountString` wraps a `u128`, with `From<u64>`, and serializes it as a string, so no compat amount can reach JSON as a number ([D27](../decisions.md#d27-hold-amounts-in-u128-and-write-the-format-helpers-lint-clean)). `global_weighted_stake` is a `u128`, and the `u64` stakes share the same type. A stored `NUMERIC(39,0)` of 18,446,744,073,709,551,620 serves only through it. A plain `u128` field serializes as a JSON number and fails the Zod schema (`spikes/crates/api`, `P27`).
- `Option<f64>` serializes `None` as `null`. No field carries `skip_serializing_if`, so every key is always present, as the squid sends it.
- serde and JavaScript both print floats in the shortest form that reads back to the same value. The APY sums run in an order that SQL does not fix, so values can differ from the squid in the last digit. Tests compare floats with a tolerance.

Three test layers pin the contract. Golden tests port the squid's route tests, a Node script runs tao-data-api's own Zod schemas against our responses, and `bt-indexer compat diff` compares live responses with the squid. [Quality](09-quality.md) describes them.

## Parse input into domain types at the boundary

Handlers receive domain types, not strings. Custom extractors parse and validate the path before the handler runs.

- `NetUid` accepts 0 to 65,535. Any other value fails with 400, as in the squid. It is stored as `INTEGER`, so no value in that range can overflow a column.
- On the compat route `GET /validators/{hotkey}/subnets`, a value that does not decode as an SS58 prefix-42 account returns `200 []`, as described above. On every native route, a malformed address fails with 400.
- `ApiPath<T>` and `ApiQuery<T>` wrap Axum's `Path` and `Query` and turn their rejections into `ApiError`. Axum's own rejections return plain text, so without the wrappers a bad path would not return the same error format as every other error. Both implement `FromRequestParts<S>` with `type Rejection = ApiError`, for `T: DeserializeOwned + Send` and `S: Send + Sync`.

## GET /status

`GET /status` is for operators and for scripts. It is not a compat route. It reads the `checkpoint` and `chain_head` tables on every request and returns the same numbers as `bt-indexer status`. [Ingestion](03-ingestion.md#see-where-every-job-is) describes the tables and the other places progress shows up.

```json
{
    "chain": {
        "finalized_block": 9491345,
        "finalized_time": "2026-11-12T16:10:12Z"
    },
    "ready": false,
    "not_ready": ["epochs: backfill, 41345 blocks behind"],
    "workers": [
        {
            "worker": "prices",
            "mode": "live",
            "start_block": 9469262,
            "last_block": 9491343,
            "last_block_time": "2026-11-12T16:09:48Z",
            "target_block": 9491345,
            "lag_blocks": 2,
            "percent_done": 100.0,
            "blocks_per_sec": 0.1,
            "eta_seconds": null,
            "last_commit_time": "2026-11-12T16:10:08Z",
            "last_commit_age_seconds": 4
        },
        {
            "worker": "epochs",
            "mode": "backfill",
            "start_block": 9260462,
            "last_block": 9450000,
            "last_block_time": "2026-11-06T22:21:36Z",
            "target_block": 9491345,
            "lag_blocks": 41345,
            "percent_done": 82.1,
            "blocks_per_sec": 21.9,
            "eta_seconds": 1888,
            "last_commit_time": "2026-11-12T16:10:11Z",
            "last_commit_age_seconds": 1
        }
    ]
}
```

- **`lag_blocks`** is the distance from `last_block` to the newest block the job's schedule selects at or below the finalized head. For an every-block job that is the head itself. For `validators`, which runs at multiples of 100, a job that has committed block 9,491,300 has a lag of 0 until the head reaches 9,491,400. Without this rule a healthy 100-block job would always look up to 99 blocks behind.
- **`percent_done`** is `(last_block − start_block) / (newest_scheduled(schedule, head) − start_block) × 100`, and 100 when that divisor is not positive. Against the raw head, a caught-up `validators` job would print 0 % and a caught-up `baskets` job 99.85 % ([D20](../decisions.md#d20-measure-lag-and-readiness-against-the-schedule)).
- **`eta_seconds`** is `lag_blocks / blocks_per_sec` in backfill mode, and `null` in live mode.
- **`ready`** and **`not_ready`** give the result of the readiness check below, with one reason per failing job.

One function in `bt-domain`, `progress(checkpoint, schedule, head)`, computes these fields from a checkpoint row, the job's `Schedule`, and the head row. The checkpoint row does not hold the schedule, so `bt-domain` also holds one table from worker name to `Schedule`, and every caller reads it. `bt-indexer status`, `GET /status`, `/readyz`, and the progress log line all call the same function, so they agree.

## Report readiness per job window

- **`/healthz`** returns 200 while the process runs, also while the compat routes shed load. Docker or Kubernetes uses it to restart a stuck process. The shed layers wrap only the compat routes, so a busy process is not restarted ([middleware stack](#middleware-stack)).
- **`/readyz`** returns 200 only when the database answers `SELECT 1` within one second and every compat job has covered the history its routes read. Otherwise it returns 503 with a problem body that lists the failing jobs. A load balancer uses it to stop sending traffic, and the `CompatReadyFalse` alert pages on it.

A job is behind when `last_block < newest_scheduled(schedule, head − api.max_lag_blocks)`, with `max_lag_blocks` at default 50 ([D20](../decisions.md#d20-measure-lag-and-readiness-against-the-schedule)). A job is ready when it is not behind and its history reaches back far enough. The test reads the lag against the schedule, not the raw head. For `validators` and `identities` (every 100 blocks) and `baskets` (every 500), the lag at the raw head jumps to 100 or 500 when the head reaches the next scheduled block. A limit on that lag would fail `/readyz` from that block until the snapshot commits, every cycle. On synthetic heads, a healthy job stays ready at 9,612,400 and 9,612,500, and a real lag at 9,612,451 still fails (`spikes/crates/api`, `ready-schedule`).

| Job | History its routes read | Ready when |
|---|---|---|
| `prices` | 26 hours before the tip, for `/pools` | `start_block` is at least 7,800 blocks before the head (26 hours at 12 seconds per block) |
| `epochs` | 30 days of epochs, and the earliest epoch inside them, for the APYs | `start_block` is at least 223,200 blocks before the head (31 days) |
| `baskets` | 30 days of snapshots, for the root APR | `start_block` is at least 223,200 blocks before the head, or `start_block` is the first scheduled block at or after the spec 441 floor |
| `validators` | The current snapshot | The job has committed one snapshot |
| `identities` | The current snapshot | The job has committed one snapshot |

The spec 441 clause never binds in release 1. The floor is 8,765,684, the block after spec 441's upgrade block, and `basket_floor(&VersionTable)` returns it. `serve` computes it once at start and passes it to the handlers in `AppState`, because `bt-api` does not depend on `bt-chain` ([ingestion](03-ingestion.md#start-each-job-from-its-history-window)). The first scheduled block at or after it is 8,766,000. The 31-day window started much later, at about 9,260,462 at the head measured on 2026-11-12 ([D3](../decisions.md#d3-set-the-milestone-scope-by-what-release-1-reads)). It stays for release 2.

The block counts assume 12 seconds per block, which matches the measured 12.0 s, so they are approximate. Each windowed job starts one day earlier than its window ([ingestion](03-ingestion.md#start-each-job-from-its-history-window)), so a drift in block time does not leave a gap at the old end.

A backfill runs from `start_block` toward the head. The last 26 hours or 31 days are complete only when the job reaches the head, so the lag test is what holds `/readyz` at 503 during a backfill.

## Middleware stack

The stack has two parts ([D25](../decisions.md#d25-shed-load-on-the-compat-routes-and-do-not-rate-limit-or-cache-in-process)). The common part wraps every route, the ops routes `/healthz`, `/readyz` and `/status` included, and is applied with `Router::layer` after `.fallback(...)` and `.method_not_allowed_fallback(...)`. The shed part wraps the compat router only, inside the common part. A compat request therefore passes through every layer of the common part and then every layer of the shed part, in the order of the two tables below. Under overload the compat routes shed while the ops routes keep answering, so Docker or Kubernetes does not restart a process that is only busy. With a limit of 4 and 8 requests in flight, `/pools` answered 503, `/healthz` 200, `/status` 200, `/nope` a 404 problem body, and a slow compat request 504 (`spikes/crates/api`, `healthz-shed`).

The common part, from the outside in:

| Layer | Crate | Setting |
|---|---|---|
| `SetRequestIdLayer::x_request_id(MakeRequestUuid)` | `tower-http` | Sets `x-request-id` to a UUID if the client sent none, and keeps the client's value otherwise |
| `TraceLayer::new_for_http()` | `tower-http` | One span per request. See [observability](07-observability.md) |
| HTTP metrics, `axum::middleware::from_fn(http_metrics)` | our middleware | Records the request count and latency by `MatchedPath`, and `route="unmatched"` when no route matched |
| `PropagateRequestIdLayer::x_request_id()` | `tower-http` | Copies `x-request-id` to the response |
| `CorsLayer::permissive()` | `tower-http` | Any origin. It sits outside every layer that can fail, so a 503 or a 504 also carries the CORS headers. `permissive()` allows every method, not only `GET` |
| `CatchPanicLayer::custom(panic_response)` | `tower-http` | Turns a handler panic into `ApiError::Internal`, a 500 problem response. A panic does not leak a concurrency permit |
| `CompressionLayer::new()` | `tower-http` | gzip and br |

The shed part, applied with `.layer(...)` to the compat router, from the outside in:

| Layer | Crate | Setting |
|---|---|---|
| `HandleErrorLayer::new(middleware_error)` | `axum` | Maps an overload error to `ApiError::Overloaded` (503 with `Retry-After: 1`) and an elapsed timeout to `ApiError::Timeout` (504) |
| `tower::timeout::TimeoutLayer::new(d)` | `tower` | 10 seconds. The `tower` layer returns an error that `HandleErrorLayer` maps. tower-http's `TimeoutLayer::new` is deprecated since 0.6.7 and answers 408 with no body and no content type, so the stack uses the `tower` layer |
| `.load_shed()` | `tower` | Rejects a request at once when the concurrency limit is full |
| `GlobalConcurrencyLimitLayer::new(n)` | `tower` | `api.max_concurrent_requests`, default 64, shared by every compat route |

The middleware functions have these signatures: `async fn middleware_error(err: BoxError) -> ApiError`, `fn panic_response(_: Box<dyn Any + Send + 'static>) -> Response`, and `async fn http_metrics(req: axum::extract::Request, next: axum::middleware::Next) -> Response`.

Use `GlobalConcurrencyLimitLayer`, not the plain `ConcurrencyLimitLayer`. The plain layer creates a new semaphore each time it wraps a service (`tower-0.5.3/src/limit/concurrency/layer.rs:24-25`), so under `Router::layer` each route gets its own semaphore. It still sheds within one route, but the total concurrency becomes the limit times the number of routes. With the plain layer and a limit of 4, 50 concurrent requests on one route gave 4 × 200 and 46 × 503, and other routes stayed at 200 (`spikes/crates/api`, `per-route-limit`).

Metrics and traces use the matched route template, such as `/subnets/{netuid}/validators`, and never the raw path. Raw paths would create one time series per subnet and per hotkey.

### Shed load instead of queueing

Without a limit, every request that arrives waits for a database connection, and the waits add up. A burst then holds every request until the request timeout, and most of them fail late. With shedding and a 2 second acquire timeout, at most `api.max_concurrent_requests` requests wait, and the others get 503 at once or within the acquire timeout.

The stack therefore fails fast at three points:

- `GlobalConcurrencyLimitLayer` lets at most 64 compat requests run at once, and `LoadShedLayer` answers the next one with 503 instead of queueing it. With N+1 concurrent slow requests, exactly one got 503, at once, and none hung.
- The pool's `acquire_timeout` of 2 seconds is shorter than the request timeout, so a request that does get in but finds the pool empty also fails with 503 and `Retry-After: 1`. A database server that is down gives the same answer, because sqlx retries the connect until `acquire_timeout`, and the answer comes after 2,002 to 2,004 ms.
- `statement_timeout` ends any query after 5 seconds. Its SQLSTATE 57014 maps to `ApiError::Unavailable`, a 503 with `Retry-After: 1` ([D25](../decisions.md#d25-shed-load-on-the-compat-routes-and-do-not-rate-limit-or-cache-in-process)), so a slow query reads as an unavailable database (`spikes/crates/api`, `stmt-timeout`).

A fast 503 is the right answer for tao-data-api. It treats any non-ok response as a failure, tries the other mirror, and serves a stale cached value while one is young enough (`tao-data-api/src/common/cache.ts:144-161`). A request that waits 10 seconds before it fails holds a tao-data-api worker for those 10 seconds and gives the same result.

We do not rate-limit per client IP. tao-data-api is one client behind one address, and it has its own per-client limiter (`tao-data-api/src/index.ts:27`). A per-IP limit at the indexer would throttle tao-data-api itself. If the indexer is ever exposed to the public, rate-limit at the reverse proxy. Decision [D25](../decisions.md#d25-shed-load-on-the-compat-routes-and-do-not-rate-limit-or-cache-in-process) records this.

## Do not cache in process

Release 1 has no in-process response cache. tao-data-api already caches `/pools` for 5 minutes (`tao-data-api/src/endpoints/pools.ts:17`) and the other routes for 15 minutes (`tao-data-api/src/common/constants.ts:1`). Concurrent requests for the same key wait on one refresh instead of each calling the indexer (`tao-data-api/src/common/cache.ts:49-53`). The indexer therefore sees a few requests per route every few minutes, and each query reads a small, indexed window.

Successful (2xx) compat responses carry `Cache-Control: public, max-age=12`, one block time ([D26](../decisions.md#d26-set-cache-control-on-successful-compat-responses-only)). A `map_response` layer on the compat router sets it only when the status is a success, as in the code above. A 400 or 503 never carries it, so a client never caches an error (`spikes/crates/api`, `cache-control-2xx`). The ops routes do not carry it either. Add an in-process cache only if the load test in the [roadmap](10-roadmap.md#m3-serve-the-compat-routes) shows that one is needed. With canned data, the stack added a p99 of 1.88 ms at 200 requests a second (measured on 2026-11-12, AMD Ryzen 5 5600X, release build).

## Describe the API with OpenAPI

Every handler carries a `#[utoipa::path]` attribute, and every response type derives `ToSchema`. `utoipa-axum` builds the router and the OpenAPI document from the same functions. Swagger UI serves the document at `/docs` through `SwaggerUi::new("/docs").url("/openapi.json", doc)`, and the raw document is at `/openapi.json`. The document is OpenAPI 3.1.0.

The versions are `utoipa` 6.0.0, `utoipa-axum` 0.3.0, and `utoipa-swagger-ui` 10.0.2 with feature `axum`, which compile together with axum 0.8.9 (`spikes/crates/api`, `openapi`). The emitted document lists the five compat paths and `/healthz`, `/readyz` and `/status`.

utoipa does not enforce the contract on its own. Three rules close the gaps it leaves (`spikes/crates/api`, `openapi-nullable`).

- **Register every route through `routes!`.** `OpenApiRouter::route` adds a route with no documentation (`utoipa-axum-0.3.0/src/router.rs:251`). A route added that way answers 200 and is absent from the document.
- **Mark every `Option` field `#[schema(required = true)]`.** Without it, utoipa documents the key as nullable but leaves it out of `required`, while the contract always sends the key. `Option<f64>` renders `{"type":["number","null"],"format":"double"}`.
- **Give every `responses(...)` entry a `description = "..."`.** OpenAPI 3.1 requires it, and utoipa emits none by default.

`AmountString` and `Hotkey` get their schemas from a hand-written `impl utoipa::PartialSchema` plus an empty `impl utoipa::ToSchema {}`. They render `{"type":"string","pattern":"^[0-9]+$"}` and `{"type":"string"}`. A `u16` path parameter renders `minimum: 0` with no maximum.

`openapi.json` is committed at the repository root. A CI step generates it again and fails if the result differs. A change to a compat DTO therefore shows up in review as a diff of the specification.

## Add the native API in release 2

Release 2 adds the native API under `/api/v1`. It serves the data of the event pipeline, positions, and the release 2 analytics. It has its own DTOs, and it keeps the conventions below.

| Method and path | Returns |
|---|---|
| `GET /api/v1/subnets` | Every subnet with its current price, reserves, market cap, emission share, and identity |
| `GET /api/v1/subnets/{netuid}` | One subnet with the same fields |
| `GET /api/v1/subnets/{netuid}/candles?resolution=&from=&to=` | OHLCV candles. Resolutions are `1m`, `5m`, `15m`, `1h`, `4h`, `1d`, and `1w` |
| `GET /api/v1/subnets/{netuid}/prices?from=&to=` | Raw price points, at most 5,000 per request, which is about 16.7 hours |
| `GET /api/v1/subnets/{netuid}/metrics?from=&to=&resolution=` | Snapshot series of reserves, emissions, moving price, and TAO flow |
| `GET /api/v1/subnets/{netuid}/flow?window=` | Net TAO staked into the subnet over a window |
| `GET /api/v1/subnets/{netuid}/holders` | Positions on the subnet, largest first, with a cursor |
| `GET /api/v1/subnets/{netuid}/holder-counts?from=&to=` | Daily holder counts |
| `GET /api/v1/subnets/{netuid}/validators` | Validators on the subnet with stake and APY |
| `GET /api/v1/validators` | Every validator with stake, take, nominators, and APY |
| `GET /api/v1/validators/{hotkey}` | One validator |
| `GET /api/v1/validators/{hotkey}/subnets` | The validator's stake and APY per subnet |
| `GET /api/v1/validators/{hotkey}/basket` | The Root Reborn basket's rate, shares, and NAV, and the root APR |
| `GET /api/v1/accounts/{coldkey}/positions` | Current positions with value in TAO and the block of the last settle |
| `GET /api/v1/accounts/{coldkey}/staking-actions` | Staking actions, newest first, with a cursor |
| `GET /api/v1/accounts/{coldkey}/transfers` | TAO transfers, newest first, with a cursor |
| `GET /api/v1/quote/{netuid}?tao_in=` or `?alpha_in=` | A slippage quote. See [analytics](08-analytics.md) |

The native API serves no balances and no extrinsic rows. The indexer does not store them.

### Follow the native response conventions

- **Amounts are strings.** Rao and alpha amounts are `u64` values, and JavaScript numbers lose precision above 2^53. The response sends `"1500000000"`, never `1500000000`. Each amount field name ends in `_rao` or `_alpha`, so the unit is visible.
- **Prices are rao per whole alpha**, as strings. `price_rao: "23400000"` means 0.0234 TAO per alpha.
- **Ratios and APYs are JSON numbers.** They are derived values and do not need exact precision.
- **Every response says how fresh it is.** The body carries `as_of_block` and `as_of_time`.
- **Times are RFC 3339 strings in UTC.**
- **Accounts are SS58 strings** with prefix 42. A malformed address fails with 400 before the handler runs.

A list response has this shape.

```json
{
    "data": [],
    "next_cursor": "eyJ0cyI6IjIwMjYtMTEtMTBUMTI6MDA6MDBaIiwiYiI6OTQ3NTcwMCwiZSI6M30",
    "as_of_block": 9491345,
    "as_of_time": "2026-11-12T16:10:12Z"
}
```

Lists use keyset pagination. The cursor is an opaque base64 string that encodes the sort key of the last row, such as `(ts, block_number, event_index)` for staking actions. An `OFFSET` query reads and discards every row before the page, so page 1,000 is slow. A cursor query starts at the index position, and new rows arriving between two requests do not shift the next page.

Time-series endpoints take `from`, `to`, and `resolution`, and return at most 5,000 points. A request for one-minute candles over a year asks for 525,600 points, so it fails with 400 and a `detail` that names the largest range the resolution allows.

## Version the native API, and never the compat routes

The compat paths are frozen. tao-data-api chooses them, so we cannot add a version to them, and a change to their shapes or values breaks the contract. A change needs a change in tao-data-api first.

The native API is versioned by path. Adding a field or an endpoint does not change the version. Removing or renaming a field, or changing a unit, needs `/api/v2`, and `/api/v1` keeps serving until its users have moved.

## Shut down without dropping requests

The server runs with `axum::serve(listener, app).with_graceful_shutdown(token.clone().cancelled_owned())`. On `SIGTERM`, it stops accepting connections and finishes the requests in flight. The 10 second `TimeoutLayer` bounds each compat request, so shutdown waits at most about 10 seconds. After a cancel at 200 ms, a new connection at 300 ms was refused, an idle keep-alive connection closed, an in-flight 1,500 ms request answered 200, and `serve` returned `Ok(())` at 1,504 ms. With a 2 s timeout and a hung handler, the client got 504 and `serve` returned at 2,002 ms (`spikes/crates/api`, `shutdown`). The server uses the same `CancellationToken` as the ingest jobs, so when `ingest` and `serve` run in one process, one signal stops both.
