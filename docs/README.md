# Design documentation

These documents describe the Bittensor indexer before any code exists. The indexer replaces the reference squid for tao-data-api, and then adds its own analytics. The documents record what the indexer stores, how it reads the chain, what it serves, and why each design choice was made. Read them in order the first time.

| Document | What it answers |
|---|---|
| [Overview](design/01-overview.md) | What we build in releases 1 and 2, what we leave out, the tech stack, and the workspace layout |
| [Chain data](design/02-chain-data.md) | Which storage, runtime APIs, and events we read, and how we decode them across runtime versions |
| [Ingestion](design/03-ingestion.md) | How every job runs on one runner, in backfill and live mode, and where to see its progress |
| [Storage](design/04-storage.md) | Which database holds which data, and the table schemas |
| [API](design/05-api.md) | The five compat routes field by field, `GET /status`, readiness, and the middleware |
| [Error handling](design/06-error-handling.md) | The error types in each crate, and which errors retry, skip, or stop the process |
| [Observability](design/07-observability.md) | Traces, metrics, dashboards, alerts, and health checks |
| [Analytics](design/08-analytics.md) | The definition and formula of every derived number we serve |
| [Quality](design/09-quality.md) | Tests, CI, configuration, and deployment |
| [Roadmap](design/10-roadmap.md) | Milestones and the check that closes each one |
| [Decisions](decisions.md) | One entry per architecture decision, with the alternatives we rejected |

## Sources

Three local repositories informed these documents.

- **tao-data-api** at `/home/urban/Work/talisman/tao-data-api` is the contract. It is a Cloudflare Worker that reads the indexer through `INDEXER_DATA_API_1` and `INDEXER_DATA_API_2` (`src/common/indexer.ts`). The indexer is compatible when tao-data-api's five indexer calls get the same response shapes and values from it as from the squid. [API](design/05-api.md#compatibility-routes) gives those calls field by field. Citations into this repository start with `tao-data-api/`.
- **The reference squid** at `/home/urban/Work/talisman/bittensor-squid` is a TypeScript Subsquid indexer for the same chain. It is a data reference only. We take its meaning of each value that tao-data-api receives. We do not copy its code, its GraphQL API, its other routes, or its cadences and workarounds, unless a value tao-data-api receives depends on them. Citations into this repository start with `bittensor-squid/`.
- **The subtensor node** at `../subtensor`, at commit `d967d1358` (spec version 490). It defines every storage item, runtime API, and event we read. Paths in these docs that start with `pallets/`, `primitives/`, `common/`, or `runtime/` point into that repository.
