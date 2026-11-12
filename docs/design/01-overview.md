# Overview

The indexer reads the Bittensor chain (subtensor) and stores chain state in PostgreSQL. Release 1 replaces the reference squid for its one consumer, tao-data-api. It serves the five routes that tao-data-api calls, with the same response shapes and the same values. Release 2 adds the event pipeline, positions, and the native analytics. Grafana dashboards and `bt-indexer status` show the progress and health of every job.

## Goals

- **Compatible with tao-data-api.** tao-data-api can point `INDEXER_DATA_API_1` or `INDEXER_DATA_API_2` at the indexer and get its five calls answered with the same paths, shapes, and values as the squid gives. [API](05-api.md#compatibility-routes) gives the contract field by field. Nothing else about the squid is a contract.
- **Correct numbers.** Every value we serve matches what the chain says at the same block. Where the squid's meaning of a value differs from the chain's, we copy the squid's meaning and name it, as with `global_weighted_stake`.
- **Progress you can see.** An operator can see in one command which block every backfill is on, and which block live mode is on. The same numbers appear in `GET /status`, in the metrics, on the dashboard, and in the logs.
- **Restart at any point without damage.** A crash, a deploy, or a lost node connection leaves the database in a state the next run continues from. Every job commits its rows and its checkpoint in one transaction.
- **Every runtime version a job reads.** A job reads a block with the metadata of that block's runtime. In release 1 the windows reach back 31 days. In release 2 the `prices` and `events` jobs reach back to spec 233.

## Non-goals

- **GraphQL.** The squid serves a GraphQL API. We do not implement it, in either release. tao-data-api, the consumer we replace the squid for, calls only REST routes.
- **The rest of the squid's REST API.** Only the five routes that tao-data-api calls are a contract. The squid's other routes, and the `/v1/` twins of four compat routes, are not served.
- **Copying the squid's code.** The squid is a data reference. We take its meaning of each value. We copy its cadences or workarounds only where a value that tao-data-api receives depends on them.
- **A block explorer.** We store no extrinsics, no balances, and no accounts.
- **Blocks before dTAO.** Nothing starts before block 4,920,350 (spec 233), where dTAO started.
- **Writing to the chain.** The indexer reads only. It holds no keys.
- **Unfinalized blocks.** We index finalized blocks only. [Decision D3](../decisions.md#d3-index-finalized-blocks-only) explains why.

## Scope of release 1

Release 1 serves exactly the five compat routes at the host root, plus the operational endpoints. It decodes no events. Five jobs read chain state and write it.

| Job | Schedule | History it keeps | Writes | Feeds |
|---|---|---|---|---|
| `prices` | every block | 2 days | `subnet_price` | `GET /pools` |
| `validators` | blocks where `n % 100 == 0` | latest only | `validator`, `validator_subnet` | `GET /validators` and both stake routes |
| `epochs` | every block | 31 days | `epoch_dividend` | the APYs in both stake routes |
| `baskets` | blocks where `n % 500 == 0`, from spec 441 | 31 days | `basket_snapshot` | the root APR in both stake routes |
| `identities` | blocks where `n % 100 == 0` | latest only | `subnet_identity` | `GET /v1/subnet-identities` |

[Ingestion](03-ingestion.md#run-every-job-on-one-runner) lists what each job reads, and [storage](04-storage.md) gives each table. Release 1 leaves out everything no compat route reads. That covers transfers, staking actions, positions, holder counts, candles, validator history, and parallel backfill.

## Scope of release 2

Release 2 is the portfolio showcase.

- An `events` job on the same runner. It decodes the events of each block with the metadata of the parent block.
- The position folder and the reconciler, which build current positions from staking events and correct them from chain state.
- Staking actions, transfers, basket flows, subnet snapshots, holder counts, and OHLCV candles.
- The native API under `/api/v1`, described in [API](05-api.md).
- `prices` keeps full history from spec 233, so candles cover all of dTAO.

The [roadmap](10-roadmap.md) orders the work.

## Tech stack

| Concern | Choice | Notes |
|---|---|---|
| Language | Rust, stable, edition 2024 | `rust-toolchain.toml` pins the version |
| Async runtime | `tokio` | Multi-threaded runtime |
| Chain client | `subxt` 0.50 | Reads blocks, storage, and runtime APIs, and in release 2 events. Decodes against the metadata of each block's runtime. See [chain data](02-chain-data.md) |
| SCALE decoding | `scale-decode` with `#[derive(DecodeAsType)]` | Decodes storage values and event fields into our own structs |
| Fixed-point numbers | `fixed` | `U64F64`, `I96F32`, and `I64F64` from their raw bits |
| Database | PostgreSQL 17 with the TimescaleDB extension | One database. See [storage](04-storage.md) |
| Database access | `sqlx` with compile-time checked queries and the `bigdecimal` feature | `.sqlx/` offline data is committed, so CI builds without a database. `bigdecimal` maps the `NUMERIC(39,0)` columns |
| HTTP server | `axum` 0.8 on `tower` and `tower-http` | See [API](05-api.md) |
| OpenAPI | `utoipa`, `utoipa-axum`, and `utoipa-swagger-ui` | The specification is generated from the handler types |
| Errors | `thiserror` in library crates, `anyhow` in the binary only | See [error handling](06-error-handling.md) |
| Traces and logs | `tracing`, `tracing-subscriber`, and optionally `tracing-opentelemetry` | JSON logs in production, readable logs in development |
| Metrics | `metrics` and `metrics-exporter-prometheus` | Prometheus scrapes `/metrics` |
| Dashboards | Grafana | Dashboards are JSON files in `deploy/grafana/` |
| Command line | `clap` with derive | One binary with subcommands |
| Configuration | `figment` | Defaults, then a TOML file, then environment variables |
| Shutdown | `tokio-util` `CancellationToken` and `tokio::task::JoinSet` | One token stops every task |
| Tests | `cargo-nextest`, `proptest`, `insta`, `testcontainers`, `criterion`, and a small Node script | The Node script runs tao-data-api's Zod schemas against our responses. See [quality](09-quality.md) |

Release 1 has no rate limiter and no response cache, so it does not depend on `tower_governor` or `moka`. [API](05-api.md#shed-load-instead-of-queueing) explains why. Add either crate only when a measurement shows the need.

Pin exact versions in `Cargo.toml` when the workspace is created. `axum`, `tower-http`, and `utoipa-axum` release in step, so upgrade them together.

### Why we do not depend on subtensor crates

The subtensor repository has the real types, but its crates are a poor dependency for an indexer.

- Even `subtensor-runtime-common` depends on `frame-support`, `sp-runtime`, and `sp-io`.
- Every crate pins a fork of the Polkadot SDK (`RaoFoundation/polkadot-sdk` at a fixed revision) with `[patch]` overrides. The node's lock file has about 1,650 packages.
- The crates describe one runtime version, the current one. They cannot decode a block from spec 233.

We copy the few types we need instead. [Chain data](02-chain-data.md) lists them. The subtensor code is Apache-2.0. Each copied file keeps the license notice and names the source path.

We also rejected `bittensor-rs` from crates.io. It generates bindings from the latest metadata only, so it cannot decode old blocks either. `sdk/bittensor-core` in the subtensor repository decodes dynamically into JSON-like values shaped for the Python SDK. It is not published, and its default features pull in OpenSSL and timelock cryptography.

## Workspace layout

```
crates/
	domain/       Newtypes, domain types, and pure math: prices, APY, root APR, progress. No I/O, no async.
	chain/        subxt client, metadata cache, and decoders from SCALE to domain types.
	ingest/       The runner, the head tracker, and one module per job. Release 2 adds the position folder and the reconciler.
	store/        sqlx repositories, batch writers, migrations, and every query the API runs.
	api/          Axum router. compat/ holds the frozen routes, ops/ the operational endpoints, and v1/ the release 2 native API.
	telemetry/    Tracing subscriber and metrics exporter setup.
bin/
	indexer/      The bt-indexer binary.
tests/
	compat/       The Node script and the pinned copy of tao-data-api's Zod schemas.
deploy/
	compose.yaml  PostgreSQL with TimescaleDB, Prometheus, Grafana, and the indexer.
	grafana/      Provisioned data sources and dashboards.
	prometheus/   Scrape configuration and alert rules.
fixtures/
	blocks/       Recorded blocks, storage values, runtime API results, and metadata, one set per runtime era.
docs/
```

There is no `analytics` crate. Its queries live in `store`, so the compile-time checks of `sqlx` cover them, and its math lives in `domain`, so a test can call it without a database.

The dependency direction is fixed. `domain` depends on nothing in the workspace. `chain` and `store` depend on `domain`. `ingest` depends on `chain` and `store`. `api` depends on `store` and `domain`, never on `chain`, so the API keeps working when the node is down.

### Subcommands

| Subcommand | What it does |
|---|---|
| `bt-indexer ingest` | Runs the head tracker and every job on the runner |
| `bt-indexer serve` | Runs the API |
| `bt-indexer migrate` | Applies the database migrations |
| `bt-indexer status` | Prints one line per job from the `checkpoint` and `chain_head` tables. Works while the API is down. `--watch` refreshes every 2 seconds |
| `bt-indexer metadata diff` | Compares the metadata of every spec and lists the storage items, runtime APIs, and events whose shape changed |
| `bt-indexer fixture record` | Records blocks, storage values, and runtime API results from a node into `fixtures/` for offline tests |
| `bt-indexer compat diff --squid <url>` | Fetches the five compat routes from the squid and from the indexer at the same time and prints every field that differs |

You can run `ingest` and `serve` as one process for a demo, or as two processes in production so each restarts on its own.

## Architecture

```mermaid
flowchart LR
	node[(Subtensor<br/>archive node)]

	subgraph ingest[bt-indexer ingest]
		direction TB
		heads[Head tracker] --> runner
		subgraph runner[Runner]
			direction TB
			prices[prices]
			validators[validators]
			epochs[epochs]
			baskets[baskets]
			identities[identities]
			events[events<br/>release 2]
		end
		events -.-> folder[Position folder<br/>release 2]
		recon[Reconciler<br/>release 2]
	end

	subgraph pg[PostgreSQL + TimescaleDB]
		progress[(checkpoint,<br/>chain_head)]
		state[(Current state,<br/>hypertables)]
		r2[(Facts, positions,<br/>candles<br/>release 2)]
	end

	api[bt-indexer serve<br/>Axum]
	status[bt-indexer status]
	tda[tao-data-api]
	prom[Prometheus] --> graf[Grafana]

	node -- finalized heads --> heads
	node -- storage, runtime APIs --> runner
	node -. stake info .-> recon
	runner --> state
	runner --> progress
	heads --> progress
	events -.-> r2
	folder -.-> r2
	recon -.-> r2
	pg --> api
	progress --> status
	api -- compat routes --> tda
	ingest -. /metrics .-> prom
	api -. /metrics .-> prom
```

Dotted lines are release 2.

The workers write to the database. Each owns a separate set of tables, so no two workers write the same row.

- **The head tracker** follows the finalized head and writes it to `chain_head`. Every job plans its blocks from it.
- **The runner** runs every job the same way. It selects the blocks the job's schedule wants, reads them with bounded concurrency, and commits the rows and the job's checkpoint in one transaction. Each job has its own checkpoint, its own mode (`backfill` or `live`), and its own history window. A slow basket read therefore never delays price points. Release 1 runs `prices`, `validators`, `epochs`, `baskets`, and `identities`. Release 2 adds `events`, which writes staking actions, transfers, and basket flows as facts.
- **The position folder** (release 2) applies staking actions and settles in block order to build current positions. It is the only writer of the position tables.
- **The reconciler** (release 2) reads the true stake of chosen coldkeys from the chain and writes it as settles. [Ingestion](03-ingestion.md#correct-positions-from-chain-state) explains why positions built only from events drift.

[Ingestion](03-ingestion.md#run-every-job-on-one-runner) describes the runner, its backpressure, and its shutdown.

The `serve` subcommand reads from the database only. tao-data-api calls the compat routes, and operators read `GET /status`.

### See progress

`bt-indexer status` answers "which block is each backfill on, and which block is live on" in one command. It prints each job's mode, block, the head, the lag, the percent done, the rate, and the ETA. [Ingestion](03-ingestion.md#see-where-every-job-is) shows its output and the other places the same numbers appear.

## Runtime requirements

- **An archive node.** The jobs read storage and call runtime APIs at old blocks, 31 days back in release 1 and back to spec 233 in release 2. A pruned node keeps only recent state, so a backfill fails on it. Public archive endpoints rate-limit requests. Run your own node for the release 2 backfill.
- **PostgreSQL 17 with TimescaleDB 2.x.** The `deploy/compose.yaml` file starts both.
- **Disk.** [Storage](04-storage.md) estimates the size.
