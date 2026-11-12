# API

`bt-indexer serve` runs an Axum 0.8 HTTP server that reads from the database. In release 1 it serves the five routes that tao-data-api calls, plus the operational endpoints. Release 2 adds the native API under `/api/v1`. This document covers the routes, their frozen response types, readiness, the middleware stack, and shutdown. [Error handling](06-error-handling.md) covers the error types behind the error responses.

Compat claims cite two repositories by name. `bittensor-squid/...` is the reference squid at `/home/urban/Work/talisman/bittensor-squid`, and `tao-data-api/...` is the consumer at `/home/urban/Work/talisman/tao-data-api`.

## The API reads and never writes

The server connects with a PostgreSQL role that has `SELECT` rights only. A bug in a handler cannot change indexed data.

The server keeps its own `PgPool`, sized by `api.db_max_connections` (default 20).

- Each connection sets `statement_timeout = '5s'` in `after_connect`, so one slow query cannot hold a connection for long.
- `acquire_timeout` is 2 seconds. That is shorter than the 10 second request timeout, so a request that cannot get a connection fails with 503 instead of waiting out its whole budget. [Shed load instead of queueing](#shed-load-instead-of-queueing) explains why.

The server does not depend on the `chain` crate. It keeps serving while the node is down, and `/status` and `/readyz` then report the lag.

## Serve the compat routes at the host root

tao-data-api builds each indexer URL with `new URL(path, url)` (`tao-data-api/src/common/indexer.ts:107`). Every `path` it passes is absolute, such as `"/pools"` (`tao-data-api/src/endpoints/pools.ts:73`). An absolute path replaces the whole path of the base URL. A prefix on `INDEXER_DATA_API_1_URL`, such as `https://indexer.example/compat`, is therefore dropped, and the request goes to `https://indexer.example/pools`. The routes must sit at the host root, with the exact paths tao-data-api sends.

The native API of release 2 moves to `/api/v1`. The compat route `/v1/subnet-identities` already occupies `/v1`, so a native `/v1` prefix would mix two contracts under one path.

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
- **Extra fields are harmless but we send none.** Each schema is a plain `z.object`, which strips unknown keys. The squid sends fields that tao-data-api drops, such as `nominator_stake` and `basket`. Our DTOs leave them out.
- **Error bodies are never read.** tao-data-api throws on any non-2xx status (`tao-data-api/src/common/indexer.ts:47-49`). When the primary indexer fails, it cancels the body and tries the other mirror (`tao-data-api/src/common/indexer.ts:142-160`). We still send every error as `application/problem+json`, the same format as every other route.
- **Accounts are SS58 strings with prefix 42.**
- **No query parameters.** None of the five squid routes declares a query schema, and tao-data-api sends none. We ignore any that arrive.
- **Ties break by a second key.** The squid's orderings have no tiebreaker, so equal rows come back in an arbitrary order. We break ties by `hotkey`, or by `netuid` where the hotkey is fixed. Any order is valid for the squid, so the result stays compatible. [Decision D13](../decisions.md#d13-anchor-apy-on-the-indexed-head-and-break-rank-ties-by-hotkey) records the deviation.

### `GET /pools`

The body is a bare JSON array with one object per subnet (`bittensor-squid/api/src/schemas/pools.ts:3`).

| Field | JSON type | Unit and format | Null |
|---|---|---|---|
| `netuid` | number | The subnet id. tao-data-api requires a number (`tao-data-api/src/endpoints/pools.ts:22`) | Never |
| `total_tao` | string | Rao, an integer. It is the pool's TAO reserve (`bittensor-squid/api/src/routes/pools.ts:74`) | Never |
| `total_alpha` | string | Raw alpha (10^9 per alpha), an integer. It is the alpha reserve plus `alpha_out` (`pools.ts:75`) | Never |
| `price` | string | TAO per alpha as a decimal. At most 9 fraction digits, trailing zeros stripped, no `.` when the value is whole, and `"0"` for zero (`pools.ts:6-15`, `pools.ts:121`) | Never |
| `price_change_1_day` | string | Percent, so 5 % is `"5"`, not `"0.05"`. Truncated toward zero, with at most 16 fraction digits after trailing zeros are stripped. A negative value has a `-` prefix. `"0"` when there is no price a day before, or that price is 0 (`pools.ts:87-108`) | Never |
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
	pub stake: AmountString,                 // u64 as a JSON string
	pub thirty_day_apy: Option<f64>,         // null, never omitted
	pub thirty_day_apy_gross: Option<f64>,
}

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/pools", get(pools::list))
		.route("/validators", get(validators::list))
		.route("/subnets/{netuid}/validators", get(subnet_validators::list))
		.route("/validators/{hotkey}/subnets", get(validator_subnets::list))
		.route("/v1/subnet-identities", get(subnet_identities::list))
}

pub fn router(state: AppState) -> Router {
	Router::new()
		.merge(compat::routes())
		.merge(ops::routes())                    // /status, /healthz, /readyz, /docs, /openapi.json
		.layer(middleware::stack(&state.config))
		.with_state(state)
}

#[derive(Clone)]
pub struct AppState {
	db: PgPool,
	config: Arc<ApiConfig>,
}
```

- `AmountString` wraps a `u64` and serializes it as a string, so no compat amount can reach JSON as a number.
- `Option<f64>` serializes `None` as `null`. No field carries `skip_serializing_if`, because the Zod schemas fail on a missing key.
- serde and JavaScript both print floats in the shortest form that reads back to the same value. The APY sums run in an order that SQL does not fix, so values can differ from the squid in the last digit. Tests compare floats with a tolerance.
- `NetUid` accepts 0 to 65,535 and is stored as `INTEGER`, so no value in that range can overflow a column.

Three test layers pin the contract. Golden tests port the squid's route tests, a Node script runs tao-data-api's own Zod schemas against our responses, and `bt-indexer compat diff` compares live responses with the squid. [Quality](09-quality.md) describes them.

## Parse input into domain types at the boundary

Handlers receive domain types, not strings. Custom extractors parse and validate the path before the handler runs.

- `NetUid` accepts 0 to 65,535. Any other value fails with 400, as in the squid.
- On the compat route `GET /validators/{hotkey}/subnets`, a value that does not decode as an SS58 prefix-42 account returns `200 []`, as described above. On every native route, a malformed address fails with 400.
- `ApiPath` and `ApiQuery` wrap Axum's `Path` and `Query` and turn their rejections into `ApiError`. Axum's own rejections return plain text, so without the wrappers a bad path would not return the same error format as every other error.

## GET /status

`GET /status` is for operators and for scripts. It is not a compat route. It reads the `checkpoint` and `chain_head` tables on every request and returns the same numbers as `bt-indexer status`. [Ingestion](03-ingestion.md#see-where-every-job-is) describes the tables and the other places progress shows up.

```json
{
	"chain": {
		"finalized_block": 9612345,
		"finalized_time": "2026-11-11T09:30:12Z"
	},
	"ready": false,
	"not_ready": ["epochs: backfill, 124225 blocks behind"],
	"workers": [
		{
			"worker": "prices",
			"mode": "live",
			"start_block": 9590745,
			"last_block": 9612343,
			"last_block_time": "2026-11-11T09:29:48Z",
			"target_block": 9612345,
			"lag_blocks": 2,
			"percent_done": 100.0,
			"blocks_per_sec": 0.1,
			"eta_seconds": null,
			"last_commit_time": "2026-11-11T09:30:08Z",
			"last_commit_age_seconds": 4
		},
		{
			"worker": "epochs",
			"mode": "backfill",
			"start_block": 9381945,
			"last_block": 9488120,
			"last_block_time": "2026-10-25T03:25:12Z",
			"target_block": 9612345,
			"lag_blocks": 124225,
			"percent_done": 46.1,
			"blocks_per_sec": 310.0,
			"eta_seconds": 401,
			"last_commit_time": "2026-11-11T09:30:11Z",
			"last_commit_age_seconds": 1
		}
	]
}
```

- **`lag_blocks`** is the distance from `last_block` to the newest block the job's schedule selects at or below the finalized head. For an every-block job that is the head itself. For `validators`, which runs at multiples of 100, a job that has committed block 9,612,300 has a lag of 0 until the head reaches 9,612,400. Without this rule a healthy 100-block job would always look up to 99 blocks behind.
- **`percent_done`** is `(last_block − start_block) / (head − start_block) × 100`.
- **`eta_seconds`** is `lag_blocks / blocks_per_sec` in backfill mode, and `null` in live mode.
- **`ready`** and **`not_ready`** give the result of the readiness check below, with one reason per failing job.

One function in `domain` computes these fields from a checkpoint row and the head row. `bt-indexer status`, `GET /status`, and the progress log line all call it, so they cannot disagree.

## Report readiness per job window

- **`/healthz`** returns 200 while the process runs. Docker or Kubernetes uses it to restart a stuck process.
- **`/readyz`** returns 200 only when the database answers `SELECT 1` within one second and every compat job has covered the history its routes read. Otherwise it returns 503 with a problem body that lists the failing jobs. A load balancer uses it to stop sending traffic, and the `CompatReadyFalse` alert pages on it.

A job is ready when its lag, as `/status` defines it, is at most `api.max_lag_blocks` (default 50), and its history reaches back far enough.

| Job | History its routes read | Ready when |
|---|---|---|
| `prices` | 26 hours before the tip, for `/pools` | `start_block` is at least 7,800 blocks before the head (26 hours at 12 seconds per block) |
| `epochs` | 30 days of epochs, and the earliest epoch inside them, for the APYs | `start_block` is at least 223,200 blocks before the head (31 days) |
| `baskets` | 30 days of snapshots, for the root APR | `start_block` is at least 223,200 blocks before the head, or the job started at the first spec 441 block |
| `validators` | The current snapshot | The job has committed one snapshot |
| `identities` | The current snapshot | The job has committed one snapshot |

The block counts assume 12 seconds per block, so they are approximate. Each windowed job starts one day earlier than its window ([ingestion](03-ingestion.md#run-every-job-on-one-runner)), so a drift in block time does not leave a gap at the old end.

A backfill runs from `start_block` toward the head. The last 26 hours or 31 days are complete only when the job reaches the head, so the lag test is what holds `/readyz` at 503 during a backfill.

## Middleware stack

`tower::ServiceBuilder` applies these layers from the outside in.

| Layer | Crate | Setting |
|---|---|---|
| `SetRequestIdLayer` | `tower-http` | Sets `x-request-id` to a UUID if the client sent none |
| `TraceLayer` | `tower-http` | One span per request. See [observability](07-observability.md) |
| HTTP metrics | our middleware | Records the request count and latency by `MatchedPath`, and `route="unmatched"` when no route matched |
| `PropagateRequestIdLayer` | `tower-http` | Copies `x-request-id` to the response |
| `CorsLayer` | `tower-http` | `GET` from any origin. It sits outside every layer that can fail, so a 503 or a 504 also carries the CORS headers |
| `CatchPanicLayer::custom` | `tower-http` | Turns a handler panic into `ApiError::Internal`, a 500 problem response |
| `HandleErrorLayer` | `axum` | Maps an overload error to `ApiError::Overloaded` (503 with `Retry-After: 1`) and an elapsed timeout to `ApiError::Timeout` (504) |
| `TimeoutLayer` | `tower` | 10 seconds. The `tower` layer returns an error that `HandleErrorLayer` maps. The `tower-http` layer of the same name would answer 408 itself |
| `LoadShedLayer` | `tower` | Rejects a request at once when the concurrency limit is full |
| `GlobalConcurrencyLimitLayer` | `tower` | `api.max_concurrent_requests`, default 64, shared by every route. The plain `ConcurrencyLimitLayer` creates a new semaphore each time it wraps a service (`tower-0.5.3/src/limit/concurrency/layer.rs:24-25`), so under `Router::layer` each route gets its own limit and nothing is shed |
| `CompressionLayer` | `tower-http` | gzip and br |

Metrics and traces use the matched route template, such as `/subnets/{netuid}/validators`, and never the raw path. Raw paths would create one time series per subnet and per hotkey.

The server runs with plain `axum::serve`. No layer needs the client's socket address.

### Shed load instead of queueing

Without a limit, every request that arrives waits for a database connection, and the waits add up. In a measured probe with 1,000 requests against a handler that holds a connection for 0.5 seconds, 939 requests waited on the pool at once, and 600 of them ended in a 408 after 10 to 12 seconds. With shedding and a 2 second acquire timeout, at most 64 waited, and the other requests got 503 within 2 seconds.

The stack therefore fails fast at three points:

- `GlobalConcurrencyLimitLayer` lets at most 64 requests run at once, and `LoadShedLayer` answers the next one with 503 instead of queueing it.
- The pool's `acquire_timeout` of 2 seconds is shorter than the request timeout, so a request that does get in but finds the pool empty also fails with 503.
- `statement_timeout` ends any query after 5 seconds.

A fast 503 is the right answer for tao-data-api. It treats any non-ok response as a failure, tries the other mirror, and serves a stale cached value while one is young enough (`tao-data-api/src/common/cache.ts:144-161`). A request that waits 10 seconds before it fails holds a tao-data-api worker for those 10 seconds and gives the same result.

We do not rate-limit per client IP. tao-data-api is one client behind one address, and it has its own per-client limiter (`tao-data-api/src/index.ts:27`). A per-IP limit at the indexer would throttle tao-data-api itself. If the indexer is ever exposed to the public, rate-limit at the reverse proxy. [Decision D14](../decisions.md#d14-shed-load-and-do-not-rate-limit-or-cache-in-process) records this.

## Do not cache in process

Release 1 has no in-process response cache. tao-data-api already caches `/pools` for 5 minutes (`tao-data-api/src/endpoints/pools.ts:17`) and the other routes for 15 minutes (`tao-data-api/src/common/constants.ts:1`). Concurrent requests for the same key wait on one refresh instead of each calling the indexer (`tao-data-api/src/common/cache.ts:49-53`). The indexer therefore sees a few requests per route every few minutes, and each query reads a small, indexed window.

Every response carries `Cache-Control: public, max-age=12`, one block time. Add an in-process cache only if the load test in the [roadmap](10-roadmap.md#m3-serve-the-compat-routes) shows that one is needed.

## Describe the API with OpenAPI

Every handler carries a `#[utoipa::path]` attribute, and every response type derives `ToSchema`. `utoipa-axum` builds the router and the OpenAPI document from the same functions, so a route cannot exist without its documentation. Swagger UI serves the document at `/docs`, and the raw document is at `/openapi.json`.

`openapi.json` is committed at the repository root. A CI step generates it again and fails if the result differs. A change to a compat DTO therefore shows up in review as a diff of the specification.

`/metrics` is served on a separate port (`telemetry.metrics_addr`, default `0.0.0.0:9100`) that is not exposed to the public.

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
	"next_cursor": "eyJ0cyI6IjIwMjYtMTEtMTBUMTI6MDA6MDBaIiwiYiI6OTUyMDAwMCwiZSI6M30",
	"as_of_block": 9612345,
	"as_of_time": "2026-11-11T09:30:12Z"
}
```

Lists use keyset pagination. The cursor is an opaque base64 string that encodes the sort key of the last row, such as `(ts, block_number, event_index)` for staking actions. An `OFFSET` query reads and discards every row before the page, so page 1,000 is slow. A cursor query starts at the index position, and new rows arriving between two requests do not shift the next page.

Time-series endpoints take `from`, `to`, and `resolution`, and return at most 5,000 points. A request for one-minute candles over a year asks for 525,600 points, so it fails with 400 and a `detail` that names the largest range the resolution allows.


## Version the native API, and never the compat routes

The compat paths are frozen. tao-data-api chooses them, so we cannot add a version to them, and a change to their shapes or values breaks the contract. A change needs a change in tao-data-api first.

The native API is versioned by path. Adding a field or an endpoint does not change the version. Removing or renaming a field, or changing a unit, needs `/api/v2`, and `/api/v1` keeps serving until its users have moved.

## Shut down without dropping requests

The server runs with `axum::serve(listener, app).with_graceful_shutdown(token.cancelled_owned())`. On `SIGTERM`, it stops accepting connections and finishes the requests in flight. The 10 second `TimeoutLayer` bounds each of them, so shutdown waits at most about 10 seconds. The server uses the same `CancellationToken` as the ingest jobs, so when `ingest` and `serve` run in one process, one signal stops both.
