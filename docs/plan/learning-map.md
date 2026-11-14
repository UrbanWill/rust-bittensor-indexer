# Learning map

Every Rust and tokio concept in the [plan](README.md), in the order you meet it. Each row names the step that first uses the concept and one primary source. The step itself says why the project uses the concept there, and which mistake a JavaScript engineer tends to make with it.

Read the source when you reach the step, not before. A concept that a step does not list is one the plan assumes you know.

This file is generated from the stage files by `gen-tables.py`. Edit the step, not this file.

## Stage 1. Prove the read path against the real node

| Concept | First used in | Primary source |
|---|---|---|
| Cargo workspace | [Step 1](01-read-path.md) | [The Cargo Book, "Workspaces"](https://doc.rust-lang.org/cargo/reference/workspaces.html) |
| `#[tokio::main]` | [Step 1](01-read-path.md) | [tokio, `attr.main`](https://docs.rs/tokio/1.54.1/tokio/attr.main.html) |
| thiserror in libraries, anyhow in the binary | [Step 1](01-read-path.md) | the thiserror README, section "Comparison to anyhow" |
| Newtype | [Step 3](01-read-path.md) | The Rust Book, "Advanced Types", section "Using the Newtype Pattern for Type Safety and Abstraction" |
| Parse at the boundary | [Step 3](01-read-path.md) | [clap, `value_parser!`](https://docs.rs/clap/4.6.7/clap/macro.value_parser.html) |
| `Arc<RwLock<_>>` | [Step 4](01-read-path.md) | tokio tutorial, "Shared state" |
| RPITIT with `+ Send` | [Step 5](01-read-path.md) | Rust Blog, "Announcing `async fn` and return-position `impl Trait` in traits" (2023-12-21) |
| Static dispatch | [Step 5](01-read-path.md) | The Rust Book, "Traits: Defining Shared Behavior", section "Traits as Parameters" |
| Exhaustive `match` | [Step 6](01-read-path.md) | The Rust Book, "The match Control Flow Construct" |
| Boxing a large error | [Step 6](01-read-path.md) | the clippy lint list, `result_large_err` |
| `tokio::time::timeout` | [Step 6](01-read-path.md) | [tokio, `time::timeout`](https://docs.rs/tokio/1.54.1/tokio/time/fn.timeout.html) |
| Enums chosen from data | [Step 7](01-read-path.md) | The Rust Book, "Defining an Enum" |
| Collecting into `Result` | [Step 8](01-read-path.md) | std docs, `impl FromIterator<Result<A, E>> for Result<V, E>` |

## Stage 2. Build the walking skeleton

| Concept | First used in | Primary source |
|---|---|---|
| A hand-written `From` and `?` | [Step 9](02-skeleton.md) | The Rust Book, "Recoverable Errors with `Result`", section "A Shortcut for Propagating Errors: the `?` Operator" |
| `sqlx::migrate!` | [Step 9](02-skeleton.md) | [sqlx, `migrate!`](https://docs.rs/sqlx/0.8.6/sqlx/macro.migrate.html) |
| `tokio::sync::watch` | [Step 10](02-skeleton.md) | [tokio, `sync::watch`](https://docs.rs/tokio/1.54.1/tokio/sync/watch/index.html) |
| `tokio::select!` with `ctrl_c` | [Step 10](02-skeleton.md) | tokio tutorial, "Select" |
| A trait with an associated const and type | [Step 11](02-skeleton.md) | The Rust Reference, "Associated Items" |
| sqlx `Transaction` | [Step 11](02-skeleton.md) | [sqlx, `Transaction`](https://docs.rs/sqlx/0.8.6/sqlx/struct.Transaction.html) |
| `query!` offline data | [Step 11](02-skeleton.md) | [sqlx, `query!`](https://docs.rs/sqlx/0.8.6/sqlx/macro.query.html), section "Offline Mode" |
| `impl Display` | [Step 12](02-skeleton.md) | [std, `fmt::Display`](https://doc.rust-lang.org/std/fmt/trait.Display.html) |

## Stage 3. Decode every era offline

| Concept | First used in | Primary source |
|---|---|---|
| serde derive | [Step 14](03-eras.md) | serde.rs, "Using derive" |
| Integration tests in `tests/` | [Step 15](03-eras.md) | The Rust Book, "Test Organization" |
| insta snapshots | [Step 15](03-eras.md) | insta.rs, "Quickstart" |
| A foreign trait with associated types | [Step 17](03-eras.md) | The Rust Book, "Advanced Traits", section "Associated Types" |
| `[u8; 32]` account newtypes | [Step 17](03-eras.md) | scale-decode docs, the `DecodeAsType` derive |
| `BTreeMap::range` | [Step 17](03-eras.md) | [std, `BTreeMap::range`](https://doc.rust-lang.org/std/collections/struct.BTreeMap.html#method.range) |
| Fixed-point from raw bits | [Step 19](03-eras.md) | the fixed crate docs, `FixedI128::from_bits` |
| `String::from_utf8_lossy` and `Cow` | [Step 20](03-eras.md) | [std, `String::from_utf8_lossy`](https://doc.rust-lang.org/std/string/struct.String.html#method.from_utf8_lossy) |
| `char` predicates | [Step 20](03-eras.md) | [std, `char::is_whitespace`](https://doc.rust-lang.org/std/primitive.char.html#method.is_whitespace) |
| Sequential async paging | [Step 21](03-eras.md) | subxt-rpcs docs, `LegacyRpcMethods::state_get_keys_paged` |

## Stage 4. The real runner

| Concept | First used in | Primary source |
|---|---|---|
| `#[must_use]` effect values | [Step 24](04-runner.md) | [The Rust Reference, "Diagnostic attributes", the `must_use` attribute](https://doc.rust-lang.org/reference/attributes/diagnostics.html#the-must_use-attribute) |
| A database per test from a template | [Step 25](04-runner.md) | [PostgreSQL docs, "Template Databases"](https://www.postgresql.org/docs/current/manage-ag-templatedbs.html) |
| `buffered` over spawned tasks | [Step 26](04-runner.md) | [futures docs, `StreamExt::buffered`](https://docs.rs/futures/latest/futures/stream/trait.StreamExt.html#method.buffered) |
| `'static` tasks with `Arc` and `async move` | [Step 26](04-runner.md) | [tokio tutorial, "Spawning", the `'static` bound section](https://tokio.rs/tokio/tutorial/spawning) |
| Abort on drop | [Step 26](04-runner.md) | [tokio-util docs, `task::AbortOnDropHandle`](https://docs.rs/tokio-util/latest/tokio_util/task/struct.AbortOnDropHandle.html) |
| Bounded `mpsc` | [Step 26](04-runner.md) | [tokio docs, `sync::mpsc`](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html) |
| `select!` cancel safety | [Step 27](04-runner.md) | [tokio docs, `select!`, "Cancellation safety"](https://docs.rs/tokio/latest/tokio/macro.select.html#cancellation-safety) |
| Paused time | [Step 27](04-runner.md) | [tokio docs, `time::pause`](https://docs.rs/tokio/latest/tokio/time/fn.pause.html) |
| `CancellationToken` | [Step 28](04-runner.md) | [tokio-util docs, `sync::CancellationToken`](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html) |
| `JoinSet` and the two error layers | [Step 28](04-runner.md) | [tokio docs, `task::JoinSet`](https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html) |
| `ExitCode` | [Step 28](04-runner.md) | [std docs, `process::ExitCode`](https://doc.rust-lang.org/std/process/struct.ExitCode.html) |
| Permits do not bound the rate | [Step 29](04-runner.md) | [tokio docs, `sync::Semaphore`](https://docs.rs/tokio/latest/tokio/sync/struct.Semaphore.html) |
| Closures that return futures | [Step 29](04-runner.md) | [The Rust Book, "Closures"](https://doc.rust-lang.org/book/ch13-01-closures.html) |
| `#[instrument]` on async functions | [Step 30](04-runner.md) | [tracing docs, `attr.instrument`](https://docs.rs/tracing/latest/tracing/attr.instrument.html) |
| axum extractors and `State` | [Step 31](04-runner.md) | [axum docs, `extract`](https://docs.rs/axum/latest/axum/extract/index.html) |

## Stage 5. The other four jobs

| Concept | First used in | Primary source |
|---|---|---|
| Layered config into one typed struct | [Step 32](05-jobs.md) | [figment docs](https://docs.rs/figment/latest/figment/) |
| `SecretString` | [Step 32](05-jobs.md) | [secrecy docs](https://docs.rs/secrecy/latest/secrecy/) |
| Policy as enums and pure functions | [Step 33](05-jobs.md) | [The Rust Book, "Enums and Pattern Matching"](https://doc.rust-lang.org/book/ch06-00-enums.html) |
| Checked arithmetic under a deny lint | [Step 34](05-jobs.md) | [std docs, `u128::checked_mul`](https://doc.rust-lang.org/std/primitive.u128.html#method.checked_mul) |
| Property tests | [Step 34](05-jobs.md) | [the proptest book](https://proptest-rs.github.io/proptest/) |
| `Notify` as a test gate | [Step 36](05-jobs.md) | [tokio docs, `sync::Notify`](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html) |

## Stage 6. Serve the compat routes

| Concept | First used in | Primary source |
|---|---|---|
| `IntoResponse` for an error enum | [Step 43](06-compat-api.md) | [axum, `response`](https://docs.rs/axum/0.8.9/axum/response/index.html) |
| Layer order in `ServiceBuilder` | [Step 43](06-compat-api.md) | [axum, "Applying fallible middleware"](https://docs.rs/axum/0.8.9/axum/error_handling/index.html#applying-fallible-middleware) |
| `Error::is` on a boxed error | [Step 43](06-compat-api.md) | [std, `error::Error`](https://doc.rust-lang.org/std/error/trait.Error.html) |
| serde container attributes | [Step 44](06-compat-api.md) | [serde, "Container attributes"](https://serde.rs/container-attrs.html) |
| Router tests with `oneshot` | [Step 44](06-compat-api.md) | [tower, `ServiceExt`](https://docs.rs/tower/0.5.3/tower/util/trait.ServiceExt.html) |
| `Option<f64>` as null | [Step 45](06-compat-api.md) | [std, `f64`](https://doc.rust-lang.org/std/primitive.f64.html) |
| Wrapping an extractor | [Step 46](06-compat-api.md) | [axum, "Wrapping extractors"](https://docs.rs/axum/0.8.9/axum/extract/index.html#wrapping-extractors) |
| Custom `Serialize` | [Step 46](06-compat-api.md) | [serde, "Implementing Serialize"](https://serde.rs/impl-serialize.html) |
| Exact decimals from integers | [Step 47](06-compat-api.md) | The Rust Reference, "Arithmetic and Logical Binary Operators" |
| Arithmetic the lint accepts | [Step 47](06-compat-api.md) | [clippy, `arithmetic_side_effects`](https://rust-lang.github.io/rust-clippy/master/index.html#arithmetic_side_effects) |
| `Ordering::then_with` for a tie-break | [Step 49](06-compat-api.md) | [std, `Ordering::then_with`](https://doc.rust-lang.org/std/cmp/enum.Ordering.html#method.then_with) |
| A background task on `tokio::time::interval` | [Step 50](06-compat-api.md) | [tokio, `time::interval`](https://docs.rs/tokio/1.54.1/tokio/time/fn.interval.html) |

## Stage 7. Shadow the squid

| Concept | First used in | Primary source |
|---|---|---|
| `tokio::join!` | [Step 53](07-shadow-run.md) | [tokio, `join!`](https://docs.rs/tokio/1.54.1/tokio/macro.join.html) |
| `serde_json::Value` as the compared type | [Step 53](07-shadow-run.md) | [serde_json, `Value`](https://docs.rs/serde_json/1.0.154/serde_json/enum.Value.html) |
