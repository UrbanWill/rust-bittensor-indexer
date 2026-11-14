# Error handling

This document describes the error types in each crate, how the indexer decides whether to retry an error or stop, and what the API returns to a client.

## Each library crate owns one error enum

Library crates define their errors with `thiserror`. Only the binary uses `anyhow`, to add context at startup and to report the error that stops the process. No library function returns `anyhow::Error`. The caller of a library function can then match on the variant and decide what to do.

```rust
// crates/chain
#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("node connection lost, client is reconnecting")]
    Disconnected(#[source] Box<subxt::Error>),
    #[error("rpc request limit reached")]
    RateLimited(#[source] Box<subxt::Error>),
    #[error("{method} at block {block} timed out after {after:?}")]
    Timeout { method: &'static str, block: BlockNumber, after: Duration },
    #[error("block {0} not found on node")]
    BlockNotFound(BlockNumber),
    #[error("no metadata for spec {0}")]
    MetadataMissing(SpecVersion),
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error("unclassified node error")]
    Unknown(#[source] Box<subxt::Error>),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

`Disconnected`, `RateLimited`, and `Unknown` carry the `subxt::Error` they came from, and each boxes it, because it is 240 bytes by value. Unboxed, it trips `clippy::result_large_err` on every function that returns `Result<_, ChainError>`, and CI runs Clippy with `-D warnings` ([D22](../decisions.md#d22-classify-chain-errors-by-what-the-node-sends-and-bound-the-clients-waits-with-our-own-timeouts), `spikes/crates/chain-errors`).

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

`StoreError::disposition` returns `Retry` for these errors ([D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests)).

- A pool timeout, `sqlx::Error::PoolTimedOut`.
- A lost connection, `sqlx::Error::Io`. A connection reset by a database restart arrives this way, and the pool recovers in 262 ms with no new pool (`spikes/crates/store`, `outage`).
- The PostgreSQL codes `57P01` (admin shutdown), `57P02` (crash shutdown), and `57P03` (cannot connect now), which all mean the server went away. A dropped connection arrives first as `57P01`, then as `Io(UnexpectedEof)`.
- The PostgreSQL codes `40001` (serialization failure), `40P01` (deadlock), and `57014` (statement timeout).

It returns `StopProcess` for everything else, including constraint violations and `Overflow`. A constraint violation means the code wrote data it should not have, and a retry writes the same data again. `40001`, `40P01`, `57014`, `57P01`, `Io`, `PoolTimedOut`, and a constraint violation have each been produced against PostgreSQL (`spikes/crates/store`, `sqlstate-map`, `outage`).

The `match` on our own enums has no wildcard arm. Adding a variant fails the build until someone decides how the runner handles it. The inner match on `sqlx::Error` needs one, because that enum is `#[non_exhaustive]` (`sqlx-core-0.8.6/src/error.rs:31`) and SQLSTATEs are strings.

### Release 1 stops the process for `StopJob` too

Every release 1 job feeds a compatibility route. If one job stopped while the others ran, the process would look healthy, `up` would stay 1, and the stopped job's route would keep serving data that grows older with every block. So in release 1 the runner handles `StopJob` exactly like `StopProcess`. The process exits, the restart retries from the checkpoint, and `IndexerStalled` fires if the job still cannot pass the block. [Observability](07-observability.md#alert-on-symptoms) defines that alert.

Release 2 adds jobs that no compatibility route reads. For those, `StopJob` stops only the job.

### Classify `subxt::Error` when it enters the crate

`subxt::Error` is `#[non_exhaustive]` (`subxt-0.50.3/src/error.rs:31`), so a `match` on it needs a wildcard. That wildcard lives in exactly one function. Every node call in `crates/chain` goes through two functions ([D21](../decisions.md#d21-call-the-node-in-two-layers)). `SubxtSource` wraps each raw request in `classified`, which turns `subxt::Error` into `ChainError`. `Chain::call` wraps each `BlockSource` method in the permit, the rate token, the timeout, and the retry, and counts each `Disconnected` result in `bt_rpc_disconnects_total`, which [observability](07-observability.md#node-and-database) defines. It takes a closure, because each attempt needs a new future.

```rust
// crates/chain/src/source.rs. Only SubxtSource calls it.
async fn classified<T>(
    method: &'static str,
    block: BlockNumber,
    spec: Option<SpecVersion>,
    request: impl Future<Output = Result<T, subxt::Error>>,
) -> Result<T, ChainError> {
    let started = Instant::now();
    request.await.map_err(|err| classify(err, method, block, spec, started.elapsed()))
}

impl<S: BlockSource> Chain<S> {
    async fn call<T, Fut>(&self, method: &'static str, block: BlockNumber, mut request: impl FnMut() -> Fut) -> Result<T, ChainError>
    where
        Fut: Future<Output = Result<T, ChainError>>,
    {
        retry_with_backoff(&self.limits.retry, &self.token, || {
            let attempt = request();
            async move {
                let _permit = self.limits.permits.acquire().await;
                self.limits.rate.take().await;
                let result = match tokio::time::timeout(self.limits.timeout, attempt).await {
                    Err(_elapsed) => Err(ChainError::Timeout { method, block, after: self.limits.timeout }),
                    Ok(result) => result,
                };
                if let Err(ChainError::Disconnected(_)) = &result {
                    metrics::counter!("bt_rpc_disconnects_total").increment(1);
                }
                result
            }
        })
        .await
    }
}

fn classify(err: subxt::Error, method: &'static str, block: BlockNumber, spec: Option<SpecVersion>, elapsed: Duration) -> ChainError {
    // subxt decodes only runtime API results here, so `spec` is the block's state spec. Storage
    // decodes through frame-decode with `executed_by` and builds its own `DecodeError`.
    // `spec` is None inside `block_ref` and `finalized_head`, which decode no runtime value.
    let decode = |err: subxt::Error| match spec {
        Some(spec) => ChainError::Decode(DecodeError { kind: DecodeKind::State, block, spec, item: method, reason: Box::new(err) }),
        None => ChainError::Unknown(Box::new(err)),
    };
    match rpc_error(&err) {
        Some(RpcsError::DisconnectedWillReconnect(_)) => return ChainError::Disconnected(Box::new(err)),
        Some(e) if is_request_timeout(e) => return ChainError::Timeout { method, block, after: elapsed },
        Some(RpcsError::User(u)) if RATE_LIMIT_CODES.contains(&u.code) => return ChainError::RateLimited(Box::new(err)),
        Some(RpcsError::Decode(_) | RpcsError::Deserialization(_)) => return decode(err),
        _ => {}
    }
    if err.is_disconnected_will_reconnect() {
        return ChainError::Disconnected(Box::new(err));
    }
    match err {
        subxt::Error::OnlineClientAtBlockError(OnlineClientAtBlockError::BlockNotFound { .. }) => {
            ChainError::BlockNotFound(block)
        }
        subxt::Error::OfflineClientAtBlockError(OfflineClientAtBlockError::MetadataNotFound { spec_version }) => {
            ChainError::MetadataMissing(SpecVersion(spec_version))
        }
        subxt::Error::OtherCodecError(_)
        | subxt::Error::StorageValueError(_)
        | subxt::Error::ModuleErrorDecodeError(_)
        | subxt::Error::RuntimeApiError(RuntimeApiError::CouldNotDecodeResponse(_))
        | subxt::Error::StorageError(StorageError::NoValueFound) => decode(err),
        subxt::Error::EventsError(ref e) if !matches!(e, EventsError::CannotFetchEventBytes(_)) => decode(err),
        other => ChainError::Unknown(Box::new(other)),
    }
}

/// JSON-RPC codes treated as "slow down".
const RATE_LIMIT_CODES: [i32; 2] = [-32029, -32004];

/// The client's own request timeout. The plain client boxes jsonrpsee's `RequestTimeout` bare inside
/// `subxt::rpcs::Error::Client`. The reconnecting client boxes it inside its own `Error::RpcError`.
fn is_request_timeout(err: &RpcsError) -> bool { /* as `is_request_timeout` in `spikes/crates/chain-errors/src/classify.rs` */ }
```

The arms follow `spikes/crates/chain-errors/src/chain_error.rs`, which tests them against the errors that a dropped socket, a rate limit, a bad block, and a hung call produce on subxt 0.50.3 ([D22](../decisions.md#d22-classify-chain-errors-by-what-the-node-sends-and-bound-the-clients-waits-with-our-own-timeouts)). `classify` takes the block number and the spec, not a `BlockRef`, because the first calls of a read have no `BlockRef` yet. `spec` is `None` for the calls inside `block_ref` and `finalized_head`, and `finalized_head` passes block 0. `DecodeError.spec` is the spec of the metadata used for the decode. That is `executed_by` for storage and events, and `state` for a runtime API result. `MetadataMissing` takes the spec from the error itself.

- `rpc_error` finds the `subxt::rpcs::Error` wherever subxt nested it. A raw legacy call puts it in `OtherRpcClientError`, and subxt's own calls put it inside a `BackendError` in `StorageError::CannotFetchValue`, `RuntimeApiError::CannotCallApi`, and the other fetch wrappers. The wrappers hold the `BackendError` without `#[source]`, so `Error::source()` cannot reach it, and `rpc_error` names each wrapper (`rpc_error` in `spikes/crates/chain-errors/src/classify.rs`). Raw legacy calls need `.map_err(subxt::Error::from)` before they reach `classified`.
- `DisconnectedWillReconnect` is `Disconnected`. A raw call in flight when the socket drops gets it, and subxt does not replay that call. subxt replays its own backend calls internally, in a loop with no backoff, so they return `Ok` after the reconnect. A call made while the socket is down queues and waits, with no error, until `chain.rpc_timeout`.
- The client's own `RequestTimeout` is `Timeout`, with the time the call ran. The outer `chain.rpc_timeout` fires first, so this arm is a backstop. Without it, a hung call that reaches the client's timeout surfaces as `OtherRpcClientError(Client(RpcError(RequestTimeout)))`, falls through to `Unknown`, and stops the process (`spikes/crates/chain-errors`, `client-request-timeout`).
- A JSON-RPC user error whose code is in `RATE_LIMIT_CODES`, `-32029` or `-32004`, is `RateLimited`. The node sends `-32029` (`spikes/crates/chain-errors`, `node-error-codes`). The arm matches these codes, because subxt's `is_rpc_limit_reached` (`subxt-0.50.3/src/error.rs:137`) matches only chainHead's `limitReached` response. subxt does not retry a `-32029`. It returns the error after one request.
- `BlockNotFound` and `MetadataMissing` come from the subxt errors that mean them. `MetadataNotFound` comes only from `OfflineClient::at_block`, which returns it for a spec with no metadata registered (`spikes/crates/chain-errors`, `metadata-missing`). `OnlineClient` fetches missing metadata from the node instead.
- Codec and storage-decode errors are `Decode`. A retry would decode the same bytes again.
- Anything else is `Unknown` and stops the process. That includes the node's state error codes, each of which means the code asked for something wrong, so a retry would fail the same way (`spikes/crates/chain-errors`, `node-error-codes`). 4003 answers an unknown block on `state_getStorage`, `state_call` and `state_getKeysPaged`, and a failing runtime API. 4001 answers an unknown block on `state_queryStorageAt`, the validators job's batch read. 4002 answers a `state_getKeysPaged` count above 1000. It also includes the plain client's `RestartNeeded`, after which every call fails the same way. A new subxt variant then shows up as a stop with its message in the log, never as a silent retry loop.
- Event decoding (release 2) uses `decode_fields_unchecked_as::<T>()` and builds its `DecodeError` with `DecodeKind::Event` at the decoder, not here.

One function, `retry_with_backoff(policy, token, op)`, retries any operation whose error says `Retry`. Only its backoff sleep selects on the token. An attempt never does, so a commit is never dropped halfway. [Ingestion](03-ingestion.md#retry-node-requests) gives the backoff settings. After `chain.rpc_max_retries` attempts, the last error becomes `StopProcess`. No other code in the workspace loops on errors.

`BlockNotFound` retries because a node that is still syncing returns it for a block that the node will soon have. A raw `chain_getBlockHash` past the head answers `Ok(None)`, and the call that looks up the hash turns that into `BlockNotFound` itself.

**Connecting.** The chain crate builds its client with subxt's reconnecting RPC client. That client sits behind the `reconnecting-rpc-client` feature, which is not a default, so the manifest names it. It retries one URL forever and never fails over to another URL.

- It also retries forever inside `build` (`subxt-rpcs-0.50.3/src/client/reconnecting_rpc_client/mod.rs:331-334`), so a wrong URL would hang at startup with no error. `connect` therefore wraps the build in `tokio::time::timeout(chain.rpc_timeout, builder.build(url))` ([D22](../decisions.md#d22-classify-chain-errors-by-what-the-node-sends-and-bound-the-clients-waits-with-our-own-timeouts)).
- On that timeout, `connect` makes one plain `RpcClient::from_url` attempt and puts that attempt's error in the message. A wrong key reports `Rejected { status_code: 403 }` and a wrong path 404, both in about 0.3 s, and neither error text holds the key or the host (`spikes/crates/chain-errors`, `gateway-handshake-rejected`).
- The message names the URL's index in `BT_CHAIN__URLS`, never the URL, because the URL can hold a key (`spikes/crates/chain-errors`, `connect-timeout`).
- A URL with no key connects. The gateway accepts it and allows 10 calls a second, so it fails later as `RateLimited` retries, not at startup (`spikes/crates/chain-errors`, `gateway-keyless`).

**Timeouts.** `chain.rpc_timeout` (default 90 s, [D23](../decisions.md#d23-cap-the-call-rate-with-a-one-token-bucket-not-only-the-calls-in-flight)) bounds every node call. It covers the call only, not the wait for a permit and a rate token before it (`spikes/crates/runner-live`, `timeout-scope`). A hung call therefore releases its RPC permit, and the call is recorded in `bt_rpc_request_seconds` with `outcome="timeout"`. An error answer from the node takes 25 to 60 s, and one basket call took up to 33.5 s.

The client has its own timeout too, 60 s by default, which is below `chain.rpc_timeout`. `connect` builds the client as `ReconnectingRpcClient::builder().request_timeout(chain.rpc_timeout + 30 s)`, so the outer timeout always fires first. When a node lists `archive_v1_*` methods, as the keyed node does, `runtime_apis().call_raw` tries `archive_v1_call` and then `state_call`, so it fails only after two client timeouts, 120 s at the client's default. The outer `tokio::time::timeout` bounds that too (`spikes/crates/chain-errors`, `client-request-timeout`).

## What each error does

| Error | Where | Disposition | Action |
|---|---|---|---|
| `Disconnected`, `RateLimited`, `Timeout`, or `BlockNotFound` | Any job read | `Retry` | Retry with backoff. After `chain.rpc_max_retries` attempts, stop the process |
| Runtime API not present at a spec | Any job read | none | Not an error. The job picks its era from the spec before it calls, as [chain data](02-chain-data.md) describes |
| Decode error on a storage value or runtime API response | Any job read | `StopJob` | In release 1, stop the process. In release 2, stop only the job when no compatibility route reads it |
| Decode error on an indexed event | `events` job (release 2) | `StopProcess` | Stop the process |
| `Unknown` node error or `MetadataMissing` | Any job read | `StopProcess` | Stop the process |
| Lost connection (`Io`, `57P01` to `57P03`), pool timeout, serialization failure, deadlock, or statement timeout | Any writer | `Retry` | Roll back the transaction. Retry the whole batch |
| Constraint violation or `Overflow` | Any writer | `StopProcess` | Stop the process |
| A channel closes before its end-of-range message | Runner | `StopProcess` | Stop the process. An upstream task died |
| A task panics | Runner | `StopProcess` | Stop the process |
| Open reconcile request at the folder's next block | Position folder (release 2) | none | Wait. Not an error |
| `SIGTERM` or `SIGINT` | Supervisor | none | Commit what each writer holds, then exit with code 0 |
| A task still running at `shutdown_timeout` after a signal | Supervisor | none | Abort it, log a warning that says how many tasks it aborted, and still exit with code 0. The abort rolls back its transaction |
| A signal while a writer waits in a commit backoff | Writer | none | Give up the batch and exit with code 0. In `commit_batch`, a `Retry` error after a cancelled token becomes `Ok(Committed::Stopped)`. Any other error stays an error ([D18](../decisions.md#d18-give-every-job-one-trait-and-build-the-runner-from-a-spawned-read-stage-and-a-batching-writer), `spikes/crates/runner`, `cancel-in-commit-backoff`). The next run writes the same blocks again |

"Stop the process" always runs the same steps. The supervisor cancels the root `CancellationToken`. Every writer commits the rows it holds, because those rows belong to blocks before the failure and the write is idempotent. Then the process exits with code 1.

### A decode failure stops the job

No job skips a value that it fails to decode. A skipped `TotalHotkeyAlpha` page makes `global_weighted_stake` wrong, and nothing would show it until someone compared it with the chain. A stop shows up within minutes. `IndexerStalled` fires, the log names the block and the item, and the checkpoint stays before the bad block.

To recover, record the failing block as a fixture with `bt-indexer fixture record --block <n> --case <name> --read <job>` ([D34](../decisions.md#d34-keep-one-fixture-per-test-case-and-record-it-by-running-the-jobs-own-read)), add the decoder for the new shape with a test on that fixture, and deploy. The job restarts from its checkpoint and decodes the block.

In release 2 the `events` job filters events by name before decoding. A new event in a runtime upgrade therefore never stops it. A new shape of an event that we index does.

### Every failure ends the process

Each job runs its read stage and its writer as tasks in one `JoinSet`, and the supervisor holds one `JoinSet` of jobs and the head tracker. Every task returns `Result<(), RunnerError>`. The supervisor takes the first `Err` from `join_next`, or the first `JoinError`, cancels the token, and exits with code 1.

Three rules keep a failure from looking like success.

- **No bare `tokio::spawn` for a stage.** A stage spawned outside the `JoinSet` can panic, and the writer then reads the closed channel as the end of the range. A backfill would exit 0 with blocks missing.
- **The writer finishes only on an explicit end message.** The read stage sends `ToWriter::End` after the last block of its range. A channel that closes before `End` is `RunnerError::UpstreamClosed`, which stops the process.
- **A panicking read is an error at its block.** Each read is a `tokio::spawn` of an `async move` block over `Arc` clones of the job and the chain, held in a `tokio_util::task::AbortOnDropHandle`, inside the ordered `buffered` stream ([Ingestion](03-ingestion.md#read-in-order-write-in-one-place)). The stream awaits the handle, so a panic arrives as `RunnerError::ReadPanicked { block, source }` at that block's position, and the read stage returns it as its error. A plain `JoinHandle` would detach the read when the stage stops, and the read would keep its permit.

Every `.await` on a channel, the head watch, or a node call goes through `select!` with `token.cancelled()`, or through `token.run_until_cancelled`. A stage that waits on an empty channel therefore stops at once on cancel, instead of when a shutdown timeout aborts it and drops the batch it holds. A commit runs outside any `select!`, because a commit future dropped by a losing branch loses its batch ([D18](../decisions.md#d18-give-every-job-one-trait-and-build-the-runner-from-a-spawned-read-stage-and-a-batching-writer)).

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

The `domain` crate also denies `arithmetic_side_effects`, with `#![deny(clippy::arithmetic_side_effects)]` at the top of `crates/domain/src/lib.rs`. The attribute sits in the source, because a crate with `[lints] workspace = true` cannot add its own lint in its `Cargo.toml`.

Amount arithmetic uses `checked_add`, `checked_sub`, and `checked_mul`, which return an error on overflow instead of wrapping. A wrapped `u64` in a stake total is a wrong number that looks right. The format helpers in `bt-domain`, such as `fmt_scaled<const SCALE: u32>(value: u128)`, pass the lint with bodies that follow `spikes/crates/api/src/format_lint.rs` and use operations such as `u128::strict_mul` ([D27](../decisions.md#d27-hold-amounts-in-u128-and-write-the-format-helpers-lint-clean)).

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
| `BadRequest` | 400 | The message, such as ``"Invalid URL: Cannot parse `70000` to a `u16`"`` |
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
    "detail": "Invalid URL: Cannot parse `70000` to a `u16`"
}
```

That detail is what `ApiPath<u16>` sends for `/subnets/70000/validators` (`spikes/crates/api`). A friendlier message would need its own `Deserialize` for `NetUid`.

The compatibility routes never return 404 for an unknown netuid or hotkey. They return `200 []`, as the squid does.

`From<StoreError> for ApiError` maps `NotFound` to `NotFound`. It maps a lost connection, a pool timeout, and a query cut by `statement_timeout` (SQLSTATE `57014`) to `Unavailable`, which answers 503 with `Retry-After: 1` ([D25](../decisions.md#d25-shed-load-on-the-compat-routes-and-do-not-rate-limit-or-cache-in-process)). Everything else maps to `Internal`. A slow query therefore reads as an unavailable database, not as a 500 `internal error`. The API pool's `acquire_timeout` of 2 s is shorter than the 10 s request timeout, so a pool that cannot serve answers 503 quickly instead of 504 late. [API](05-api.md#shed-load-instead-of-queueing) explains the overload design.

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

let shed = ServiceBuilder::new()
    .layer(HandleErrorLayer::new(middleware_error))
    .layer(tower::timeout::TimeoutLayer::new(Duration::from_secs(10)))
    .load_shed()
    .layer(GlobalConcurrencyLimitLayer::new(cfg.api.max_concurrent_requests));

let app = compat
    .layer(shed)
    .merge(ops)
    .fallback(|| async { ApiError::NotFound { resource: "route" } })
    .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
    .layer(
        ServiceBuilder::new()
            .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
            .layer(TraceLayer::new_for_http())
            .layer(axum::middleware::from_fn(http_metrics))
            .layer(PropagateRequestIdLayer::x_request_id())
            .layer(CorsLayer::permissive())
            .layer(CatchPanicLayer::custom(panic_response))
            .layer(CompressionLayer::new()),
    );
```

`compat` holds the compatibility routes, and `ops` holds `/healthz`, `/readyz`, and `/status`. The timeout and the shed layers wrap the compat router only, and the other layers wrap everything ([D25](../decisions.md#d25-shed-load-on-the-compat-routes-and-do-not-rate-limit-or-cache-in-process)). Under overload the compat routes shed while `/healthz` keeps answering, so Docker or Kubernetes does not restart a process that is only busy. With a limit of 4 and 8 requests in flight, `/pools` answered 503 while `/healthz` and `/status` answered 200. [API](05-api.md#shed-load-instead-of-queueing) gives the routes.

| Failure | Source | Response |
|---|---|---|
| No route matches | `Router::fallback` | 404 |
| The route has no handler for the method | `Router::method_not_allowed_fallback` | 405 |
| A handler panics | `CatchPanicLayer::custom` | 500 |
| A compat request takes longer than 10 s | `tower::timeout::TimeoutLayer` through `HandleErrorLayer` | 504 |
| `api.max_concurrent_requests` compat requests are already in flight | `load_shed` through `HandleErrorLayer` | 503, with `Retry-After: 1` |
| An extractor rejects the input | `ApiPath` and `ApiQuery` | 400 |

The ops routes have no timeout and are never shed.

Two layers are chosen over look-alikes that behave differently.

- **Use `tower::timeout::TimeoutLayer`, not the one in `tower_http`.** tower-http 0.6.8 deprecates its `TimeoutLayer::new` (since 0.6.7). That layer returns its own 408 response with no body and no content type (`tower-http-0.6.8/src/timeout/service.rs:30`), which `HandleErrorLayer` never sees. The `tower` layer returns an `Elapsed` error, which maps to 504.
- **Use `GlobalConcurrencyLimitLayer`, not `ConcurrencyLimitLayer` or `concurrency_limit(n)`.** `ConcurrencyLimitLayer::layer` creates a new semaphore on every call (`tower-0.5.3/src/limit/concurrency/layer.rs:24-25`), and `Router::layer` calls it once per route. The plain layer still sheds within one route, but each route gets its own semaphore, so the total in flight is the limit times the number of routes. With a limit of 4, 50 concurrent requests on one route gave 4 × 200 and 46 × 503, while the other routes still answered 200. `GlobalConcurrencyLimitLayer` shares one semaphore across every route it wraps.

Against axum 0.8.9, tower 0.5.3, and tower-http 0.6.8, every row of the table above answers with `content-type: application/problem+json`, `access-control-allow-origin`, and an `x-request-id` header. `CorsLayer` and `PropagateRequestIdLayer` sit outside `CatchPanicLayer`, so they add their headers to error responses too.

The problem body has no request ID field, because `IntoResponse` cannot see the request. The `x-request-id` response header carries it on every response. A user reporting a problem sends that ID, and the operator finds the full error in the logs with it.
