# Stage 6. Serve the compat routes

This stage serves the five routes that tao-data-api calls, with the error format, the readiness check, the OpenAPI document, and a load test around them. It closes roadmap M3, where the golden tests and the Zod schema tests pass and the load test holds p99 below 100 ms. Before you start, stage 5 must be done, with all five jobs live. You also need the `bt-api` crate and `GET /status` from step 31, the `progress()` function, `newest_scheduled`, and the schedule table from step 12, Node.js on the machine for the schema test, and a tao-data-api checkout to copy the schemas from. The worked references below are in the probe `spikes/crates/api`, built with axum 0.8.9, tower 0.5.3, tower-http 0.6.8, and tokio-util 0.7.20.

## Step 43. `feat(api): answer every failure as problem+json behind the middleware stack`

- **Goal.** Grow the `ApiError` that step 31 uses, and replace its minimal `IntoResponse`, into the full enum of [error handling](../design/06-error-handling.md#the-api-returns-problem-details), and put the routes behind the two-part layer stack of [D25](../decisions.md#d25-shed-load-on-the-compat-routes-and-do-not-rate-limit-or-cache-in-process), so that every failure answers `application/problem+json` and the ops routes stay up under overload.
- **Files.** `crates/api/src/{error.rs,middleware.rs,lib.rs}`, `crates/api/tests/errors.rs`, the `serve` subcommand. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum ApiError {
      #[error("{0}")] BadRequest(String),
      #[error("{resource} not found")] NotFound { resource: &'static str },
      #[error("method not allowed")] MethodNotAllowed,
      #[error("server overloaded")] Overloaded,
      #[error("request timed out")] Timeout,
      #[error("database unavailable")] Unavailable(#[source] StoreError),
      #[error("internal error")] Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
  }
  impl IntoResponse for ApiError { fn into_response(self) -> Response { todo!() } }
  impl From<StoreError> for ApiError { fn from(err: StoreError) -> Self { todo!() } }
  pub fn problem(status: StatusCode, detail: &str) -> Response { todo!() }
  async fn middleware_error(err: BoxError) -> ApiError { todo!() }
  fn panic_response(_: Box<dyn Any + Send + 'static>) -> Response { todo!() }
  async fn http_metrics(req: axum::extract::Request, next: axum::middleware::Next) -> Response { todo!() }
  const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
  pub fn shed(cfg: &ApiConfig) -> impl Layer<Route, Service: Service<Request, Response = Response, Error = Infallible, Future: Send + 'static> + Clone + Send + Sync + 'static> + Clone + Send + Sync + 'static { todo!() }  // middleware::shed, compat routes only
  pub fn common() -> impl Layer<Route, Service: Service<Request, Response = Response, Error = Infallible, Future: Send + 'static> + Clone + Send + Sync + 'static> + Clone + Send + Sync + 'static { todo!() }  // middleware::common, every route
  ```

- **Who writes what.** You write `ApiError`, both impls, `problem`, `middleware_error`, and `panic_response`. The agent writes `middleware::shed`, `middleware::common`, `http_metrics`, and one test per failure through `oneshot`.

  The real compat routes arrive in step 44, so the tests put a test-only compat router under `middleware::shed`, with `/test/slow`, which sleeps past `REQUEST_TIMEOUT` and holds a concurrency slot, `/test/panic`, which panics, and `/test/db`, which runs `SELECT pg_sleep(1)` after `SET LOCAL statement_timeout = '10ms'` on the test database and returns the `StoreError`.

  `shed(cfg)` is a `ServiceBuilder` of `HandleErrorLayer::new(middleware_error)`, `tower::timeout::TimeoutLayer::new(REQUEST_TIMEOUT)`, `.load_shed()`, and `GlobalConcurrencyLimitLayer::new(cfg.max_concurrent_requests)`. `router` applies it to the compat router with `.layer(middleware::shed(cfg))`, then `.merge(ops::routes())` from step 31, then `.fallback(...)` and `.method_not_allowed_fallback(...)`, then `.layer(middleware::common())` over everything, as in [API](../design/05-api.md#freeze-the-compat-dtos).

  `common()` starts with `.map_response(IntoResponse::into_response)` so its return type can name `Response`, then, outermost first, `SetRequestIdLayer::x_request_id(MakeRequestUuid)`, `TraceLayer::new_for_http()`, `axum::middleware::from_fn(http_metrics)`, `PropagateRequestIdLayer::x_request_id()`, `CorsLayer::permissive()`, `CatchPanicLayer::custom(panic_response)`, and `CompressionLayer::new()`. The worked reference is `router_ops_exempt` in `spikes/crates/api/src/app.rs` (`spikes/crates/api`, `healthz-shed`). Stub `shed()` and `common()` with the body `tower::layer::util::Identity::new()`, because a `todo!()` body has type `!`, which is not a `Layer<Route>`.

  Review checks:
  - The request timeout is the constant `REQUEST_TIMEOUT`, 10 s.
  - The timeout is `tower::timeout::TimeoutLayer`, not the tower-http one, which is deprecated and answers 408 with no body.
  - The limit is `GlobalConcurrencyLimitLayer`, not `concurrency_limit(n)`, which limits each route apart.
  - `From<StoreError>` maps `PoolTimedOut`, a lost connection, and SQLSTATE 57014 to `Unavailable`, 503 with `Retry-After: 1` ([D25](../decisions.md#d25-shed-load-on-the-compat-routes-and-do-not-rate-limit-or-cache-in-process)).
  - 500 and 503 log the full error chain and send only the generic `detail`.
  - The metrics `route` label is the `MatchedPath` template or `unmatched`.
  - The exporter sets buckets for `bt_http_request_seconds` with `set_buckets_for_metric(Matcher::Full(..), &[0.005, 0.025, 0.1, 0.5, 1.0, 2.5, 10.0])`, or it renders a summary with no `_bucket` series.
  - `serve` runs `with_graceful_shutdown(token.clone().cancelled_owned())` on the token of step 28.
- **Concepts.**
  - *`IntoResponse` for an error enum.* Handlers return `Result<T, ApiError>`, so `?` turns a store error into a problem body in one place. Trap: `thiserror` needs an `#[error(...)]` on every variant. Without them the derive fails with E0277, `ApiError doesn't implement std::fmt::Display` (`spikes/crates/api`, `api-error-derive`). Source: [axum, `response`](https://docs.rs/axum/0.8.9/axum/response/index.html).
  - *Layer order in `ServiceBuilder`.* The first layer added is the outermost, so CORS and the request ID wrap the layers that can fail. A fallible layer needs `HandleErrorLayer` outside it before axum accepts it. Trap: one shed stack over every route. Under overload it sheds `/healthz`, `/readyz`, and `/status` too, and an orchestrator then restarts a busy process. Source: [axum, "Applying fallible middleware"](https://docs.rs/axum/0.8.9/axum/error_handling/index.html#applying-fallible-middleware).
  - *`Error::is` on a boxed error.* `middleware_error` learns which layer failed by testing the concrete type inside `BoxError`, such as `tower::load_shed::error::Overloaded`. Trap: matching on the message text, as `err.message.includes(...)` would. Source: [std, `error::Error`](https://doc.rust-lang.org/std/error/trait.Error.html).
- **Check.** `cargo test -p bt-api --test errors` passes one test each for 404, 405, 500, 503, and 504, with these bodies. 404 is `{"detail":"route not found","status":404,"title":"Not Found","type":"about:blank"}`. 405 has `detail` `method not allowed` and `allow: GET,HEAD`. 500 is `internal error`, 504 is `request timed out`, and the shed 503 is `server overloaded` with `retry-after: 1`. Each response has `content-type: application/problem+json`, an `access-control-allow-origin` header, and an `x-request-id` header. `/test/db` answers 503 `database unavailable` with `retry-after: 1`, so SQLSTATE 57014 does not answer 500. With the limit held by `/test/slow` requests, `/healthz` and `/status` answer 200. A test installs a capturing `tracing` subscriber and finds the full error chain, down to the SQLSTATE 57014 source, in the log line of the 503, and the panic message in the log line of the 500, while neither body holds them. The 400 test arrives with the first extractor in step 46.

## Step 44. `feat(api): serve GET /v1/subnet-identities and check it with tao-data-api's schema`

- **Goal.** The thinnest compat route from table to JSON, and the Zod schema test that every later route reuses.
- **Files.** `crates/api/src/compat/{mod.rs,subnet_identities.rs}`, `crates/store/src/queries/identities.rs`, `tests/compat/{tda-schemas.mjs,check.mjs,package.json,TDA_COMMIT}`, `crates/api/tests/compat/{main.rs,identities.rs}`.
- **Shape.**

  ```rust
  #[derive(Serialize)]
  #[serde(rename_all = "camelCase")]
  pub struct SubnetIdentityDto {
      pub netuid: NetUid,          // a JSON number
      pub name: Option<String>,    // and description, url, github_repo, discord, contact, logo_url, additional
      pub updated_at_block: BlockNumber,
  }
  #[derive(Serialize)]
  pub struct SubnetIdentitiesBody { pub subnets: Vec<SubnetIdentityDto> }
  pub async fn list(State(s): State<AppState>) -> Result<Json<SubnetIdentitiesBody>, ApiError> { todo!() }
  pub fn routes() -> Router<AppState> { todo!() }
  ```

- **Who writes what.** The agent writes the DTO, the store query, the Node harness (the five schema declarations copied verbatim, `zod` pinned to 3.25.76, `TDA_COMMIT` set to the commit copied from), the Rust test that pipes each body to `node tests/compat/check.mjs <route>`, and the port of `subnet-identities.test.ts:8-61`. The worked reference is `spikes/compat/` (`spikes/crates/api`, `zod-cases`, `zod-http`), which checked against tao-data-api commit 6a46bbfca9c8c0cc38ae37993d9941b37332a49a. You write `list` and `compat::routes()`, and compare the schema copy with tao-data-api line by line. Review checks:
  - The schema test fails, not skips, when `node` is missing.
  - No field has `skip_serializing_if`.
  - `githubRepo` and `logoUrl` are spelled as the schema spells them.
  - `updatedAtBlock` comes from `updated_block`, which step 39 moves only on a changed row.
  - `compat::routes()` adds a `map_response` layer that sets `cache-control: public, max-age=12` only when the status is 2xx, so a 400, a 503, or a 504 never carries it ([D26](../decisions.md#d26-set-cache-control-on-successful-compat-responses-only), `spikes/crates/api`, `cache-control-2xx`).
- **Concepts.**
  - *serde container attributes.* `rename_all` pins the JSON names to the contract while the Rust fields stay snake case. Trap: `JSON.stringify` drops an `undefined` key, but serde writes `None` as `null`, and the Zod schema fails on a missing key. Source: [serde, "Container attributes"](https://serde.rs/container-attrs.html).
  - *Router tests with `oneshot`.* The test sends a request through the real router and middleware with no port open. Trap: starting a server per test, as supertest does. Source: [tower, `ServiceExt`](https://docs.rs/tower/0.5.3/tower/util/trait.ServiceExt.html).
- **Check.** `cargo test -p bt-api --test compat identities` passes the golden test `identities_matches_the_golden_body` and the schema test `identities_passes_the_zod_schema`. Add `skip_serializing_if = "Option::is_none"` to `name` and run it again. The schema test fails. Revert. `curl -si localhost:8080/v1/subnet-identities | grep -i cache-control` prints `cache-control: public, max-age=12`. A test through `oneshot` with the pool closed gets a 503 from the route with no `cache-control` header.

## Step 45. `feat(domain): compute epoch APY and root APR`

- **Goal.** The two formulas of [epoch APY](../design/08-analytics.md#epoch-apy) and [root APR](../design/08-analytics.md#root-apr) as pure functions, with every null rule.
- **Files.** `crates/domain/src/apy.rs`.
- **Shape.**

  ```rust
  pub struct EpochYield { pub dividend: u64, pub stake_at_epoch: u64 }
  pub struct EpochWindow<'a> { pub yields: &'a [EpochYield], pub take: f64, pub measurable: bool, pub window_secs: u64 }
  pub struct Apy { pub net: Option<f64>, pub gross: Option<f64> }
  pub fn epoch_apy(w: &EpochWindow<'_>) -> Apy { todo!() }
  pub fn window_secs(anchor: Timestamp, first: Timestamp) -> u64 { todo!() }
  pub struct BasketPoint { pub ts: Timestamp, pub nav_tao: u64, pub shares: u64, pub rate_bits: i128 }
  pub fn root_apr(points: &[BasketPoint]) -> Option<f64> { todo!() }
  ```

- **Who writes what.** You write all of it. The agent reviews. The worked reference is `spikes/crates/api/src/apy.rs` (`spikes/crates/api`, `apy-formula`), which matched the squid's own SQL on PostgreSQL 17 to a relative difference of 1.0e-12 in 27 of 30 cases. Review checks:
  - One test per row of the case table and per null rule in [Epoch APY](../design/08-analytics.md#epoch-apy).
  - A 100 % take with 10 or more earning epochs gives `gross = None` and a computed `net`, and a compounded value that overflows gives `gross = None` and `net = None`, where the squid's SQL raises `division by zero` or `value out of range: overflow` and fails the whole route ([D29](../decisions.md#d29-return-a-null-apy-where-the-squids-sql-errors)).
  - `rate_delta` is exact in `i128` before the conversion to `f64`.
  - `Y` is 31,557,600.
  - Expected values come from the squid's tests ([Port the squid's golden tests](../design/09-quality.md#port-the-squids-golden-tests)) and compare with a tolerance.
  - No result is `NaN` or infinite.
- **Concepts.**
  - *`Option<f64>` as null.* `None` means "not enough data" and `Some(0.0)` means "earned nothing", and the case table decides which. Trap: serde_json writes a `NaN` or an infinity as `null` with no error (`serde_json-1.0.154/src/ser.rs:169-174`), so a division by zero reaches tao-data-api as a valid-looking null. Return `None` on purpose and assert `is_finite` in tests. Source: [std, `f64`](https://doc.rust-lang.org/std/primitive.f64.html).
- **Check.** `cargo test -p bt-domain apy` passes `apy_matches_each_case_of_the_table`, `apy_null_rules`, and `apy_root_apr_closed_form`, with the closed-form root APR equal to 10 decimal places. The falling-price root case gives 1.275165099.

## Step 46. `feat(api): serve both stake routes with APYs`

- **Goal.** `GET /subnets/{netuid}/validators` and `GET /validators/{hotkey}/subnets`, with stake as a string and the APYs anchored on the `epochs` and `baskets` checkpoints.
- **Files.** `crates/api/src/compat/{subnet_validators.rs,validator_subnets.rs}`, `crates/api/src/{extract.rs,amount.rs}`, `crates/domain/src/account.rs`, `crates/store/src/queries/stake.rs`, `crates/api/tests/compat/stake.rs`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  pub struct ApiPath<T>(pub T);
  impl<T: DeserializeOwned + Send, S: Send + Sync> FromRequestParts<S> for ApiPath<T> {
      type Rejection = ApiError;
      async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> { todo!() }
  }
  pub struct CompatHotkey(pub Option<Hotkey>);  // a string that is not SS58 prefix 42 gives None, then 200 []
  pub struct AmountString(pub u128);            // serializes as a JSON string
  impl From<u64> for AmountString { fn from(v: u64) -> Self { todo!() } }
  impl Serialize for AmountString { fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { todo!() } }
  impl Hotkey { pub fn from_ss58(s: &str) -> Option<Hotkey> { todo!() } }  // plus Serialize as SS58 prefix 42
  pub async fn list(ApiPath(netuid): ApiPath<NetUid>, State(s): State<AppState>) -> Result<Json<Vec<SubnetValidatorDto>>, ApiError> { todo!() }
  ```

- **Who writes what.** You write `ApiPath`, `CompatHotkey`, `AmountString`, the SS58 parse and `Serialize`, and the two handlers that join stake rows with step 45. The agent writes the queries, the DTOs (`SubnetValidatorDto` as in [Freeze the compat DTOs](../design/05-api.md#freeze-the-compat-dtos), and one for `/validators/{hotkey}/subnets` from the field table of [its route](../design/05-api.md#get-validatorshotkeysubnets)), the golden ports of `validators.test.ts` with checkpoints seeded as [Port the squid's golden tests](../design/09-quality.md#port-the-squids-golden-tests) says, and the schema checks. Review checks:
  - `netuid=70000` gives 400 with `detail` ``Invalid URL: Cannot parse `70000` to a `u16` ``.
  - An unknown netuid gives `200 []`.
  - A hotkey that fails SS58 gives `200 []`.
  - Ties break by hotkey or by netuid.
  - Netuid 0 serves the root APR with gross null.
  - The anchors are the checkpoints' `last_block_ts`, never `now()` ([D28](../decisions.md#d28-anchor-apy-on-the-indexed-head-and-break-rank-ties-by-hotkey)).
  - `AmountString` holds a `u128` so the `u64` stakes and step 49's `u128` share it ([D27](../decisions.md#d27-hold-amounts-in-u128-and-write-the-format-helpers-lint-clean)).
  - Epoch rows for a hotkey are read with `netuid = ANY($netuids) AND hotkey = $1` (1.4 ms on the primary key, where `hotkey = $1` alone seq scans in 56 to 170 ms).
  - A test prepares that query and runs it seven times on one connection with `EXPLAIN (ANALYZE)`, and the plan from the sixth run on still uses the primary key, since a prepared plan can change after its fifth run (`spikes/crates/store`, `read-epoch-hotkey`).
  - "Measurable" is tested per netuid with `SELECT DISTINCT ts ... LIMIT 10` (about 0.5 ms), not `count(DISTINCT ts)` (89 to 166 ms).
  - `/validators/{hotkey}/subnets` reads stake and take in one statement, so the pair cannot straddle a `validators` commit.

  This step likely passes 400 lines. The natural second commit is `feat(api): serve GET /validators/{hotkey}/subnets`.
- **Concepts.**
  - *Wrapping an extractor.* `ApiPath` runs axum's `Path` and turns its plain-text rejection into `ApiError`, so a bad path keeps the problem format. Trap: validating `req.params` inside the handler, as in Express. Source: [axum, "Wrapping extractors"](https://docs.rs/axum/0.8.9/axum/extract/index.html#wrapping-extractors).
  - *Custom `Serialize`.* The JSON type is the contract, so an amount writes itself as a string. Trap: a derived integer becomes a JSON number, which fails `z.string()` and loses precision above 2^53 in JavaScript (`spikes/crates/api`, `P27`). Also, `subxt-utils-accountid32` parses any SS58 prefix from 0 to 16,383, a first byte up to 127 (`subxt-utils-accountid32-0.50.3/src/lib.rs:89-93`), so `from_ss58` re-encodes with prefix 42 and compares (`spikes/crates/api`, `ss58-prefix`). Source: [serde, "Implementing Serialize"](https://serde.rs/impl-serialize.html).
- **Check.** `cargo test -p bt-api --test compat stake` passes the golden tests `stake_subnet_validators_matches_the_golden_body` and `stake_validator_subnets_matches_the_golden_body`, and both schema checks. `cargo test -p bt-api --test errors` now also passes the 400 test.

## Step 47. `feat(domain): format pool numbers the way the squid does`

- **Goal.** The `/pools` string rules of [analytics](../design/08-analytics.md#pools), with integer math only, clean under the deny lint of `bt-domain` ([D27](../decisions.md#d27-hold-amounts-in-u128-and-write-the-format-helpers-lint-clean)).
- **Files.** `crates/domain/src/format.rs`.
- **Shape.**

  ```rust
  pub fn fmt_scaled<const SCALE: u32>(value: u128) -> String { todo!() }
  pub fn rao_to_tao(rao: u64) -> String { todo!() }
  pub fn market_cap(price: u64, total_alpha: u64) -> String { todo!() }
  pub fn price_change_1_day(cur: u64, old: Option<u64>) -> String { todo!() }
  pub fn tao_volume_24_hr(cur: u128, old: Option<u128>) -> u128 { todo!() }
  ```

- **Who writes what.** You write the tests first, with each expected string taken from the squid's `pools.ts`, then the bodies. The agent reviews. The worked reference is `spikes/crates/api/src/format_lint.rs` (`spikes/crates/api`, `P33-lint-clean`), which matched the squid's `pools.ts` on 27,026 rows. Review checks:
  - Every case in [Test the domain rules directly](../design/09-quality.md#test-the-domain-rules-directly) has a test.
  - `price_change_1_day(1, Some(3))` is `"-66.6666666666666666"`.
  - A proptest over the full `u64` range shows no panic.
  - `cargo clippy` passes with no `#[expect]` or `#[allow]` in the file.
- **Concepts.**
  - *Exact decimals from integers.* Quotient and remainder give the digits, so no float touches a served amount. Trap: `Number.prototype.toFixed` rounds, and `Math.floor` rounds a negative down. Work on the magnitude with `abs_diff` and add the `-` only when `cur < old` and the scaled value is not 0. Source: The Rust Reference, "Arithmetic and Logical Binary Operators".
  - *Arithmetic the lint accepts.* `bt-domain` denies `clippy::arithmetic_side_effects` ([Do not panic in library code](../design/06-error-handling.md#do-not-panic-in-library-code)). In the [`/pools`](../design/08-analytics.md#pools) helpers it flags the `/` and the `%` in `fmt_scaled`, the `u128` product in `market_cap`, the `i128` expression in `price_change_1_day`, and `cur - old` in `tao_volume_24_hr` (`spikes/crates/api`, `P33`). Divide by a `NonZeroU128`, multiply with `u128::strict_mul`, and subtract with `checked_sub`. Trap: a runtime `scale` lets `fmt_scaled(1, 39)` panic on `10u128.pow(39)`. A `const SCALE: u32` with `const { NonZeroU128::new(10u128.pow(SCALE)).unwrap() }` turns that into a build error. Source: [clippy, `arithmetic_side_effects`](https://rust-lang.github.io/rust-clippy/master/index.html#arithmetic_side_effects).
- **Check.** `cargo test -p bt-domain format` passes `format_matches_the_squid_cases` and the property test `format_never_panics_on_any_u64`, and `cargo clippy -p bt-domain -- -D warnings` is clean.

## Step 48. `feat(api): serve GET /pools`

- **Goal.** `/pools` per [analytics](../design/08-analytics.md#pools). The tip block, one row per subnet at that block, the day-ago row in the 24 to 26 hour window, and netuid order.
- **Files.** `crates/api/src/compat/pools.rs`, `crates/store/src/queries/pools.rs`, `crates/api/tests/compat/pools.rs`.
- **Shape.**

  ```rust
  pub struct PoolRow { pub netuid: NetUid, pub price: u64, pub tao_reserve: u64, pub alpha_reserve: u64, pub alpha_out: u64, pub lifetime_volume: u128 }
  pub struct PoolsAtTip { pub tip_block: BlockNumber, pub tip_ts: Timestamp, pub rows: Vec<(PoolRow, Option<PoolRow>)> }
  pub async fn pools_at_tip(db: &PgPool) -> Result<Option<PoolsAtTip>, StoreError> { todo!() }
  #[derive(Serialize)]
  pub struct PoolDto { pub netuid: NetUid, pub total_tao: String, pub total_alpha: String, pub price: String,
      pub price_change_1_day: String, pub market_cap: String, pub tao_volume_24_hr: String }  // the keys of the check below, in that order
  pub fn pool_dto(cur: &PoolRow, day_ago: Option<&PoolRow>) -> PoolDto { todo!() }
  pub async fn list(State(s): State<AppState>) -> Result<Json<Vec<PoolDto>>, ApiError> { todo!() }
  ```

- **Who writes what.** The agent writes the query, `PoolDto`, and the new `/pools` fixtures that [Port the squid's golden tests](../design/09-quality.md#port-the-squids-golden-tests) lists, with the schema check. You write `pool_dto` from the step 47 helpers and `list`, and read the query's window against the doc. Review checks:
  - The window anchors on the tip's `ts`, never on `now()`.
  - The day-ago row comes from a `LATERAL (... ORDER BY ts DESC LIMIT 1)` per netuid, 1.3 to 2.1 ms against 31 to 44 ms for `DISTINCT ON` (`spikes/crates/store`).
  - A netuid with no row at the tip block is left out.
  - `total_alpha` sums `alpha_reserve + alpha_out` in `u64`.
  - An empty table gives `200 []`.
- **Concepts.** None new.
- **Check.** `cargo test -p bt-api --test compat pools` passes `pools_matches_the_golden_body` and its schema check. `curl -s localhost:8080/pools | jq -c '.[0]'` shows every field except `netuid` as a string, in this form (`spikes/crates/api`, `pools-curl`): `{"netuid":1,"total_tao":"45000000000000","total_alpha":"4900000000000000","price":"0.0234","price_change_1_day":"5.8823529411764705","market_cap":"114660000000000","tao_volume_24_hr":"654321000000000000"}`.

## Step 49. `feat(api): serve GET /validators`

- **Goal.** `/validators` with the `global_weighted_stake > 0` filter, then `rank` by stake with ties broken by hotkey.
- **Files.** `crates/api/src/compat/validators.rs`, `crates/store/src/queries/validators.rs`, `crates/api/tests/compat/validators.rs`.
- **Shape.**

  ```rust
  #[derive(Serialize)]
  pub struct ValidatorDto {
      pub hotkey: Hotkey, pub name: Option<String>, pub global_weighted_stake: AmountString,
      pub global_nominators: u32, pub active_subnets: u32, pub rank: u32,
  }
  pub fn rank(rows: Vec<ValidatorRow>) -> Vec<ValidatorDto> { todo!() }
  pub async fn list(State(s): State<AppState>) -> Result<Json<Vec<ValidatorDto>>, ApiError> { todo!() }
  ```

- **Who writes what.** You write `rank` and its tie test. The agent writes the query, the DTO, the new `/validators` fixtures, and the schema check. Review checks:
  - `global_weighted_stake` reads the `NUMERIC(39,0)` column as `BigDecimal`, checks `is_integer()`, then `to_u128()`, and serves through `AmountString` ([D27](../decisions.md#d27-hold-amounts-in-u128-and-write-the-format-helpers-lint-clean)).
  - A stored 18446744073709551620 serves as that string (`spikes/crates/api`, `db-u128`).
  - The filter runs before ranking.
  - Ties compare the 32-byte account IDs.
- **Concepts.**
  - *`Ordering::then_with` for a tie-break.* `sort_by` takes a comparator that returns `Ordering`, so "stake descending, then hotkey ascending" is one chained expression, `b.stake.cmp(&a.stake).then_with(|| a.hotkey.cmp(&b.hotkey))`. Trap: calling `.reverse()` on the whole chain, which turns the hotkey tie-break descending too. Source: [std, `Ordering::then_with`](https://doc.rust-lang.org/std/cmp/enum.Ordering.html#method.then_with).
- **Check.** `cargo test -p bt-api --test compat validators` passes `validators_matches_the_golden_body`, `validators_rank_breaks_ties_by_hotkey`, and the schema check. All five schema checks now run in `cargo test -p bt-api --test compat`.

## Step 50. `feat(api): report readiness per job window on /readyz`

- **Goal.** `/readyz` and the `ready` and `not_ready` fields of `/status`, per [readiness](../design/05-api.md#report-readiness-per-job-window), and the `bt_api_ready` gauge.
- **Files.** `crates/domain/src/readiness.rs`, `crates/api/src/{state.rs,ops/ready.rs,ops/status.rs}`, the `serve` subcommand.
- **Shape.**

  ```rust
  pub enum HistoryNeed { Blocks(u32), OneCommit, BlocksOrFromBlock { blocks: u32, from: BlockNumber } }
  pub fn history_need(worker: &str, basket_floor: Option<BlockNumber>) -> Option<HistoryNeed> { todo!() }
  pub struct NotReady { pub worker: String, pub reason: String }
  pub fn readiness(jobs: &[Checkpoint], head: BlockNumber, max_lag: u32, basket_floor: Option<BlockNumber>) -> Result<(), Vec<NotReady>> { todo!() }
  // AppState and AppState::new gain basket_floor: Option<BlockNumber>, which serve fills at start
  async fn readyz(State(s): State<AppState>) -> Response { todo!() }
  async fn refresh_ready_gauge(state: AppState, token: CancellationToken) { todo!() }
  ```

- **Who writes what.** You write all of it. The agent reviews and adds the API tests. The worked reference is `behind_schedule` in `spikes/crates/api/src/status.rs` (`spikes/crates/api`, `ready-schedule`). Review checks:
  - A job is behind when `last_block < newest_scheduled(schedule, head - max_lag)`, with `newest_scheduled` and the schedule table of step 12 ([D20](../decisions.md#d20-measure-lag-and-readiness-against-the-schedule)).
  - With `max_lag` 50, a healthy `validators` or `identities` job (every 100) that committed 9,490,800 stays ready at head 9,490,900, and `baskets` (every 500) that committed 9,490,500 stays ready at 9,491,000, while the same `validators` job at head 9,490,951 is behind and fails.
  - The `SELECT 1` probe has a 1 s limit.
  - The 503 body goes through `problem()` from step 43.
  - `basket_floor` is step 38's `basket_floor(&VersionTable)`, 8,765,684, because release 1's `runtime_version` holds no row for spec 441 (step 24).
  - `serve` in `bin/indexer` computes it once at start from `VersionTable::seed()` and puts it in `AppState`, so `bt-api` never depends on `bt-chain`.
  - `baskets` is ready when it covers its window or its `start_block` is the first scheduled block at or after that floor, and in release 1 the window always starts later, so the window decides ([API](../design/05-api.md#report-readiness-per-job-window)).
  - `/status` and `/readyz` call the same function.
- **Concepts.**
  - *A background task on `tokio::time::interval`.* The gauge refreshes every 10 s in a loop that also selects on the shutdown token. Trap: `setInterval` fires whether or not the last run finished. An awaited `tick()` cannot overlap itself, but after a slow run the default `MissedTickBehavior::Burst` fires the missed ticks at once. Set `MissedTickBehavior::Delay`. Source: [tokio, `time::interval`](https://docs.rs/tokio/1.54.1/tokio/time/fn.interval.html).
- **Check.** During a backfill, `curl -si localhost:8080/readyz | head -1` prints `HTTP/1.1 503 Service Unavailable` (`spikes/crates/api`, `readyz-line`). Once every window is covered it prints `HTTP/1.1 200 OK`.

## Step 51. `feat(api): generate the OpenAPI document from the handlers`

- **Goal.** One OpenAPI document built from the handlers, served at `/openapi.json` and `/docs`, and committed so that a DTO change shows up in review.
- **Files.** `crates/api/src/openapi.rs`, every handler module, `openapi.json`, the `openapi` subcommand. Dependencies: see [the table](README.md#dependencies-by-step).
- **Brief for the agent.** Add `utoipa` 6.0.0, `utoipa-axum` 0.3.0, and `utoipa-swagger-ui` 10.0.2 with feature `axum` ([OpenAPI](../design/05-api.md#describe-the-api-with-openapi)). `compat::routes()` and step 31's `ops::routes()` become `utoipa_axum::router::OpenApiRouter<AppState>`, built with `.routes(routes!(handler))` and split with `split_for_parts()`, as in [API](../design/05-api.md#freeze-the-compat-dtos). Serve the UI with `SwaggerUi::new("/docs").url("/openapi.json", doc)`. Give `AmountString` the schema `{"type":"string","pattern":"^[0-9]+$"}` and `Hotkey` `{"type":"string"}`, each through a hand-written `impl utoipa::PartialSchema` plus `impl utoipa::ToSchema {}`. Mark every `Option` field `#[schema(required = true)]`, because utoipa otherwise leaves the always-sent key out of `required`. Give each `responses(...)` entry a `description = "..."`, which OpenAPI 3.1 requires. `bt-indexer openapi` prints the document, and a test fails when it differs from the committed `openapi.json`. Pin `@redocly/cli` in `tests/compat/package.json`, and add a test that runs its `lint` on the document. The worked reference is `spikes/crates/api/src/app.rs` and `bin/openapi_dump.rs` (`spikes/crates/api`, `openapi`, `openapi-nullable`).
- **Who writes what.** The agent writes it. Review checks:
  - No compat golden snapshot changes.
  - `AmountString` shows as `type: string`.
  - `/metrics` is not in the document.
  - No handler module calls `OpenApiRouter::route`, which adds a route the document omits (`spikes/crates/api`, `openapi-total`), so a test compares the router's paths with the document's.
- **Concepts.** None new.
- **Check.** `cargo test -p bt-api openapi_is_current` passes. `curl -s localhost:8080/openapi.json | jq -r '.openapi, (.paths | keys | join(" "))'` prints `3.1.0` and `/healthz /pools /readyz /status /subnets/{netuid}/validators /v1/subnet-identities /validators /validators/{hotkey}/subnets`. `curl -so /dev/null -w '%{http_code}' localhost:8080/docs/` prints `200`. `npm --prefix tests/compat exec -- redocly lint "$PWD/openapi.json"` reports no error, and reports one when a `description` is removed from a `responses(...)` entry.

## Step 52. `perf(api): load test the compat routes and shed overload`

- **Goal.** Close M3 with measured numbers, and prove that overload answers 503 at once instead of queueing.
- **Files.** `scripts/load.sh`, `crates/api/src/db.rs`, `README.md` (the numbers). Dependencies: see [the table](README.md#dependencies-by-step).
- **Brief for the agent.** Give the API pool `statement_timeout = '5s'` with `PgConnectOptions::options([("statement_timeout", "5s")])` or in `after_connect`, and an `acquire_timeout` of 2 s, as [the API reads and never writes](../design/05-api.md#the-api-reads-and-never-writes) asks. `scripts/load.sh` uses `oha` from `PATH` and otherwise installs it with `cargo install oha --locked`. It runs `oha` at 200 requests per second for 60 s against each of the five routes, then an overload run with `api.max_concurrent_requests` set low. It prints p50, p95, and p99 per route and the status counts of the overload run. The timing must be open loop, counted from each request's scheduled start, so that queueing shows. The open-loop reference is `spikes/crates/api/src/bin/load.rs` (`spikes/crates/api`, `load-stack`).
- **Who writes what.** The agent writes the script and the pool settings. You run it on the README machine and write the numbers and the machine into the README. Review checks:
  - The overload run shows 503 with `Retry-After: 1`, not 408 or growing latency.
  - A query held past 5 s answers 503 with `Retry-After: 1`, not 500 ([D25](../decisions.md#d25-shed-load-on-the-compat-routes-and-do-not-rate-limit-or-cache-in-process)).
  - The numbers name the machine.

  If p99 passes 100 ms, run `EXPLAIN ANALYZE` on the slow query before anyone proposes a cache. The criterion benchmarks of [Benchmark the hot paths](../design/09-quality.md#benchmark-the-hot-paths) are the natural second commit, `perf(bench): add criterion benchmarks for the hot paths`.
- **Concepts.** None new.
- **Check (M3).** `scripts/load.sh` reports p99 below 100 ms on every route, and the overload run shows 503 responses while `/healthz` answers 200. `cargo test -p bt-api --test compat` passes the golden and schema tests. For scale, the stack alone, with canned data and no database, has a p99 of 1.88 ms at 200 requests per second on an AMD Ryzen 5 5600X, measured on 2026-11-12. The real queries add to that.

## Stop here

- **Demo.** `curl` the five routes. Change one DTO field to a number and watch the schema test fail. Run `scripts/load.sh` and show the 503s of the overload run, while `/healthz` still answers 200.
- **Explain without notes.**
  - Why are the compat DTOs written by hand, and why does a change to one need a change in tao-data-api first?
  - Why does the stack use the `tower` timeout and `GlobalConcurrencyLimitLayer`, and what goes wrong with their look-alikes?
  - Why do the shed layers wrap only the compat router?
  - What is the difference between a null APY, a zero APY, and a `NaN`, and which one can reach a client by accident?
  - Why does a bad netuid get 400, while a bad hotkey gets `200 []`?
  - Why do the APYs anchor on the checkpoint time and not on the wall clock?
- **Roadmap.** Closes M3.
