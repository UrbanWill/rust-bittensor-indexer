# Error handling

This document describes the error types in each crate, how the indexer decides whether to retry an error or stop, and what the API returns to a client.

## Each library crate owns one error enum

Library crates define their errors with `thiserror`. Only the binary uses `anyhow`, to add context at startup and to report the error that stops the process. No library function returns `anyhow::Error`. The caller of a library function can then match on the variant and decide what to do.

```rust
// crates/chain
#[derive(Debug, thiserror::Error)]
pub enum ChainError {
	#[error("node connection lost, client is reconnecting")]
	Disconnected(#[source] subxt::Error),
	#[error("rpc request limit reached")]
	RateLimited(#[source] subxt::Error),
	#[error("{method} at block {block} timed out after {after:?}")]
	Timeout { method: &'static str, block: BlockNumber, after: Duration },
	#[error("block {0} not found on node")]
	BlockNotFound(BlockNumber),
	#[error("no metadata for spec {0}")]
	MetadataMissing(SpecVersion),
	#[error(transparent)]
	Decode(#[from] DecodeError),
	#[error("unclassified node error")]
	Unknown(#[source] subxt::Error),
}

#[derive(Debug, thiserror::Error)]
#[error("decode {item} at block {block} (spec {spec})")]
pub struct DecodeError {
	pub kind: DecodeKind,
	pub block: BlockNumber,
	pub spec: SpecVersion,
	pub item: &'static str,
	#[source]
	pub reason: Box<dyn std::error::Error + Send + Sync>,
}

pub enum DecodeKind {
	/// A storage value or a runtime API response.
	State,
	/// An event that the `events` job indexes (release 2).
	Event,
}

// crates/store
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
	#[error("not found")]
	NotFound,
	#[error("value {value} does not fit column {column}")]
	Overflow { column: &'static str, value: i128 },
	#[error(transparent)]
	Db(sqlx::Error),
}
```

An error carries the context that a person needs to reproduce it. `DecodeError` names the block, the spec, and the storage item, runtime API, or event, so the log line alone tells you which fixture to record.

`Overflow` holds an `i128`, so it can report a `u128` such as `SubnetVolume` or the raw bits of a fixed-point value. The `NUMERIC(39,0)` columns need sqlx's `bigdecimal` feature, because sqlx has no `i128` mapping.

`StoreError` implements `From<sqlx::Error>` by hand instead of with `#[from]`. A blanket conversion would put `RowNotFound` into `Db`, and the API would answer 500 where it means 404.

```rust
impl From<sqlx::Error> for StoreError {
	fn from(err: sqlx::Error) -> Self {
		match err {
			sqlx::Error::RowNotFound => Self::NotFound,
			other => Self::Db(other),
		}
	}
}
```

## Classify errors in one place

Each error enum implements one method that says what the runner does with it.

```rust
pub enum Disposition {
	/// Retry the operation with backoff.
	Retry,
	/// Stop this job. Other jobs continue.
	StopJob,
	/// Stop every job and exit with code 1.
	StopProcess,
}

impl ChainError {
	pub fn disposition(&self) -> Disposition {
		match self {
			Self::Disconnected(_) | Self::RateLimited(_) | Self::Timeout { .. } | Self::BlockNotFound(_) => {
				Disposition::Retry
			}
			Self::Decode(DecodeError { kind: DecodeKind::State, .. }) => Disposition::StopJob,
			Self::Decode(DecodeError { kind: DecodeKind::Event, .. }) => Disposition::StopProcess,
			Self::MetadataMissing(_) | Self::Unknown(_) => Disposition::StopProcess,
		}
	}
}
```

`StoreError::disposition` returns `Retry` for a lost connection, a pool timeout, and the PostgreSQL codes `40001` (serialization failure), `40P01` (deadlock), and `57014` (statement timeout). It returns `StopProcess` for everything else, including constraint violations and `Overflow`. A constraint violation means the code wrote data it should not have, and a retry writes the same data again.

The `match` on our own enums has no wildcard arm. Adding a variant fails the build until someone decides how the runner handles it.

### Release 1 stops the process for `StopJob` too

Every release 1 job feeds a compatibility route. If one job stopped while the others ran, the process would look healthy, `up` would stay 1, and the stopped job's route would keep serving data that grows older with every block. So in release 1 the runner handles `StopJob` exactly like `StopProcess`. The process exits, the restart retries from the checkpoint, and `IndexerStalled` fires if the job still cannot pass the block. [Observability](07-observability.md#alert-on-symptoms) defines that alert.

Release 2 adds jobs that no compatibility route reads. For those, `StopJob` stops only the job.

### Classify `subxt::Error` when it enters the crate

`subxt::Error` is `#[non_exhaustive]` (`subxt-0.50.3/src/error.rs:31`), so a `match` on it needs a wildcard. That wildcard lives in exactly one function. Every node call in `crates/chain` goes through `Chain::call`, which applies the timeout and the classification.

```rust
impl Chain {
	async fn call<T>(
		&self,
		method: &'static str,
		at: BlockRef,
		request: impl Future<Output = Result<T, subxt::Error>>,
	) -> Result<T, ChainError> {
		match tokio::time::timeout(self.rpc_timeout, request).await {
			Err(_elapsed) => Err(ChainError::Timeout { method, block: at.number, after: self.rpc_timeout }),
			Ok(result) => result.map_err(|err| classify(err, method, at)),
		}
	}
}

fn classify(err: subxt::Error, method: &'static str, at: BlockRef) -> ChainError {
	if err.is_disconnected_will_reconnect() {
		return ChainError::Disconnected(err);
	}
	if err.is_rpc_limit_reached() {
		return ChainError::RateLimited(err);
	}
	match err {
		subxt::Error::OtherCodecError(_)
		| subxt::Error::StorageValueError(_)
		| subxt::Error::EventsError(_)
		| subxt::Error::ModuleErrorDecodeError(_) => ChainError::Decode(DecodeError {
			kind: DecodeKind::State,
			block: at.number,
			spec: at.spec,
			item: method,
			reason: Box::new(err),
		}),
		other => ChainError::Unknown(other),
	}
}
```

- `is_disconnected_will_reconnect` and `is_rpc_limit_reached` (`subxt-0.50.3/src/error.rs:127`, `:137`) are the only cases that subxt marks as safe to retry. The chain crate builds its client with subxt's reconnecting RPC client, so a dropped WebSocket surfaces as `DisconnectedWillReconnect`.
- Codec and storage-decode errors are `Decode`. A retry would decode the same bytes again.
- Anything else is `Unknown` and stops the process. A new subxt variant then shows up as a stop with its message in the log, never as a silent retry loop.
- Event decoding (release 2) builds its `DecodeError` with `DecodeKind::Event` at the decoder, not here.

`chain.rpc_timeout` (default 30 s) bounds every node call. A hung call therefore releases its RPC permit, and the call is recorded in `bt_rpc_request_seconds` with `outcome="timeout"`.

One function, `retry_with_backoff`, retries any operation whose error says `Retry`. [Ingestion](03-ingestion.md#run-every-job-on-one-runner) gives the backoff settings. After `chain.rpc_max_retries` attempts, the last error becomes `StopProcess`. No other code in the workspace loops on errors.

`BlockNotFound` retries because a node that is still syncing returns it for a block that the node will soon have.

## What each error does

| Error | Where | Disposition | Action |
|---|---|---|---|
| `Disconnected`, `RateLimited`, `Timeout`, or `BlockNotFound` | Any job read | `Retry` | Retry with backoff. After `chain.rpc_max_retries` attempts, stop the process |
| Runtime API not present at a spec | Any job read | none | Not an error. The job picks its era from the spec before it calls, as [chain data](02-chain-data.md) describes |
| Decode error on a storage value or runtime API response | Any job read | `StopJob` | In release 1, stop the process. In release 2, stop only the job when no compatibility route reads it |
| Decode error on an indexed event | `events` job (release 2) | `StopProcess` | Stop the process |
| `Unknown` node error or `MetadataMissing` | Any job read | `StopProcess` | Stop the process |
| Lost connection, pool timeout, serialization failure, deadlock, or statement timeout | Any writer | `Retry` | Roll back the transaction. Retry the whole batch |
| Constraint violation or `Overflow` | Any writer | `StopProcess` | Stop the process |
| A channel closes before its end-of-range message | Runner | `StopProcess` | Stop the process. An upstream task died |
| A task panics | Runner | `StopProcess` | Stop the process |
| Open reconcile request at the folder's next block | Position folder (release 2) | none | Wait. Not an error |
| `SIGTERM` or `SIGINT` | Supervisor | none | Commit what each writer holds, then exit with code 0 |

"Stop the process" always runs the same steps. The supervisor cancels the root `CancellationToken`. Every writer commits the rows it holds, because those rows belong to blocks before the failure and the write is idempotent. Then the process exits with code 1.

### A decode failure stops the job

No job skips a value that it fails to decode. A skipped `TotalHotkeyAlpha` page makes `global_weighted_stake` wrong, and nothing would show it until someone compared it with the chain. A stop shows up within minutes. `IndexerStalled` fires, the log names the block and the item, and the checkpoint stays before the bad block.

To fix it, record the failing block as a fixture with `bt-indexer fixture record --block <n>`, add the decoder for the new shape with a test on that fixture, and deploy. The job restarts from its checkpoint and decodes the block.

In release 2 the `events` job filters events by name before decoding. A new event in a runtime upgrade therefore never stops it. A new shape of an event that we index does.

### Every failure ends the process

Each job runs its read stage and its writer as tasks in one `JoinSet`, and the supervisor holds one `JoinSet` of jobs and the head tracker. Every task returns `Result<(), RunnerError>`. The supervisor takes the first `Err` from `join_next`, or the first `JoinError`, cancels the token, and exits with code 1.

Three rules keep a failure from looking like success.

- **No bare `tokio::spawn` for a stage.** A stage spawned outside the `JoinSet` can panic, and the writer then reads the closed channel as the end of the range. A backfill would exit 0 with blocks missing.
- **The writer finishes only on an explicit end message.** The read stage sends `ToWriter::End` after the last block of its range. A channel that closes before `End` is `RunnerError::UpstreamClosed`, which stops the process.
- **A panicking read is an error at its block.** Each read runs as `tokio::spawn(read(block))` inside the ordered `buffered` stream. The stream awaits the `JoinHandle`, so a panic arrives as a `JoinError` at that block's position, and the read stage returns it as its error.

Every `.await` on a channel, the head watch, or a node call goes through `select!` with `token.cancelled()`, or through `token.run_until_cancelled`. A stage that waits on an empty channel therefore stops at once on cancel, instead of when a shutdown timeout aborts it and drops the batch it holds.

The supervisor does not restart a stopped job inside the process. Docker or Kubernetes restarts the process, and each job continues from its checkpoint. A process restart and a crash then follow the same recovery path, and that path runs on every deploy, so it is tested all the time. An in-process restart would be a second recovery path that runs only when something has gone wrong.

## Do not panic in library code

The workspace enables these Clippy lints in every library crate, as subtensor does:

```toml
[workspace.lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
indexing_slicing = "deny"
panic = "deny"
```

The `domain` crate also denies `arithmetic_side_effects`. Amount arithmetic uses `checked_add`, `checked_sub`, and `checked_mul`, which return an error on overflow instead of wrapping. A wrapped `u64` in a stake total is a wrong number that looks right.

Tests and the binary's `main` may use `expect` where a failure means the environment is broken, such as a missing configuration file.

The release profile keeps `panic = "unwind"`. A handler panic in the API then becomes a 500 problem response, and a panic in an ingest task becomes a `JoinError` that stops the process.

## The API returns problem details

Handlers return `Result<T, ApiError>`. `ApiError` implements `IntoResponse` and writes an RFC 9457 problem body with `Content-Type: application/problem+json`. tao-data-api never reads an error body ([API](05-api.md#compatibility-routes)), but every route keeps the same error format.

```rust
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
	#[error("{0}")]
	BadRequest(String),
	#[error("{resource} not found")]
	NotFound { resource: &'static str },
	#[error("method not allowed")]
	MethodNotAllowed,
	#[error("server overloaded")]
	Overloaded,
	#[error("request timed out")]
	Timeout,
	#[error("database unavailable")]
	Unavailable(#[source] StoreError),
	#[error("internal error")]
	Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}
```

| Variant | Status | `detail` sent to the client |
|---|---|---|
| `BadRequest` | 400 | The message, such as `"netuid must be between 0 and 65535"` |
| `NotFound` | 404 | The message |
| `MethodNotAllowed` | 405 | `"method not allowed"` |
| `Overloaded` | 503, with `Retry-After: 1` | `"server overloaded"` |
| `Timeout` | 504 | `"request timed out"` |
| `Unavailable` | 503, with `Retry-After: 1` | `"database unavailable"` |
| `Internal` | 500 | `"internal error"` |

```json
{
	"type": "about:blank",
	"title": "Bad Request",
	"status": 400,
	"detail": "netuid must be between 0 and 65535"
}
```

The compatibility routes never return 404 for an unknown netuid or hotkey. They return `200 []`, as the squid does.

`From<StoreError> for ApiError` maps `NotFound` to `NotFound`, a lost connection or a pool timeout to `Unavailable`, and everything else to `Internal`. The API pool's `acquire_timeout` of 2 s is shorter than the 10 s request timeout, so a pool that cannot serve answers 503 quickly instead of 504 late. [API](05-api.md#shed-load-instead-of-queueing) explains the overload design.

`Internal` holds a boxed error, not `anyhow::Error`, so the `api` crate does not depend on `anyhow`. For 500 and 503 responses, `into_response` logs the full error chain at `error` level, inside the request span. The client sees a generic message. SQL text and internal messages never reach the client.

### Map every middleware failure to `ApiError`

Some failures never reach a handler. Each one has a mapping, so every response is problem+json.

```rust
async fn middleware_error(err: BoxError) -> ApiError {
	if err.is::<tower::load_shed::error::Overloaded>() {
		ApiError::Overloaded
	} else if err.is::<tower::timeout::error::Elapsed>() {
		ApiError::Timeout
	} else {
		ApiError::Internal(err)
	}
}

fn panic_response(_payload: Box<dyn Any + Send + 'static>) -> Response {
	ApiError::Internal("handler panicked".into()).into_response()
}

let app = router
	.fallback(|| async { ApiError::NotFound { resource: "route" } })
	.method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
	.layer(
		ServiceBuilder::new()
			.layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
			.layer(trace_layer)
			.layer(metrics_layer)
			.layer(PropagateRequestIdLayer::x_request_id())
			.layer(CorsLayer::permissive())
			.layer(CatchPanicLayer::custom(panic_response))
			.layer(HandleErrorLayer::new(middleware_error))
			.layer(tower::timeout::TimeoutLayer::new(Duration::from_secs(10)))
			.load_shed()
			.layer(GlobalConcurrencyLimitLayer::new(cfg.api.max_concurrent_requests))
			.layer(CompressionLayer::new()),
	);
```

| Failure | Source | Response |
|---|---|---|
| No route matches | `Router::fallback` | 404 |
| The route has no handler for the method | `Router::method_not_allowed_fallback` | 405 |
| A handler panics | `CatchPanicLayer::custom` | 500 |
| The request takes longer than 10 s | `tower::timeout::TimeoutLayer` through `HandleErrorLayer` | 504 |
| `api.max_concurrent_requests` requests are already in flight | `load_shed` through `HandleErrorLayer` | 503, with `Retry-After: 1` |
| An extractor rejects the input | `ApiPath` and `ApiQuery` | 400 |

Two choices in this stack are easy to get wrong.

- **Use `tower::timeout::TimeoutLayer`, not the one in `tower_http`.** The `tower_http` layer returns its own empty 408 response (`tower-http-0.6.8/src/timeout/service.rs:30`), which `HandleErrorLayer` never sees. The `tower` layer returns an `Elapsed` error, which maps to 504.
- **Use `GlobalConcurrencyLimitLayer`, not `concurrency_limit(n)`.** `ConcurrencyLimitLayer::layer` creates a new semaphore on every call (`tower-0.5.3/src/limit/concurrency/layer.rs:24-25`), and `Router::layer` calls it once per route. A probe with a limit of 1 served a second concurrent request to the same route with no shedding. With `GlobalConcurrencyLimitLayer`, which shares one semaphore, the second request got 503.

The same probe checked every row of the table above against axum 0.8.9, tower 0.5.3, and tower-http 0.6.8. Each response had `content-type: application/problem+json`, `access-control-allow-origin`, and an `x-request-id` header. `CorsLayer` and `PropagateRequestIdLayer` sit outside `CatchPanicLayer`, so they add their headers to error responses too.

The problem body has no request ID field, because `IntoResponse` cannot see the request. The `x-request-id` response header carries it on every response. A user reporting a problem sends that ID, and the operator finds the full error in the logs with it.
