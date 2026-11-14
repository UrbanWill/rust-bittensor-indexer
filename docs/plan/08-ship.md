# Stage 8. Ship release 1

This stage packages the indexer so that someone else can run it: the Docker image, the full compose stack, every CI job, the alerts and the runbook, the README, and the `v0.1.0` tag. It closes roadmap M5, a clean machine that reaches `/readyz` 200 from `docker compose up`. Before you start, stage 7 must be done, with a clean 24-hour shadow run.

## Step 56. `build(docker): build a small image with cargo-chef`

- **Goal.** One image with one entry point, `bt-indexer`, where a code change rebuilds only our crates ([build a small image](../design/09-quality.md#build-a-small-image)).
- **Files.** `Dockerfile`, `.dockerignore`.
- **Brief for the agent.** Three stages. `cargo-chef` computes the recipe, the builder compiles the dependencies from it and then the workspace with `SQLX_OFFLINE=true`, and the runtime stage copies the binary into `gcr.io/distroless/cc-debian12` as a non-root user. The subcommand picks the role.
- **Who writes what.** The agent writes it. Review checks:
  - `.dockerignore` keeps `target/`, `.cache/`, and `fixtures/` out of the build context, except `fixtures/specs.csv` (`!fixtures/specs.csv`), because `bt-chain` embeds it with `include_str!` and does not compile without it.
  - The builder stage copies `fixtures/specs.csv` before the workspace build.
  - The runtime stage has no shell and no compiler.
  - A change to one `.rs` file rebuilds from the workspace layer, not from the dependency layer.
- **Concepts.** None new.
- **Check.** `docker build -t bt-indexer . && docker run --rm bt-indexer --help` prints the subcommands, `migrate`, `ingest`, `serve`, and `status` among them.

## Step 57. `feat(deploy): run the full stack with docker compose`

- **Goal.** `deploy/compose.yaml` grows from the database of step 9 to every service of [Docker Compose](../design/09-quality.md#run-everything-with-docker-compose).
- **Files.** `deploy/compose.yaml`, `deploy/db/init/`, `deploy/prometheus/prometheus.yml`.
- **Brief for the agent.** Add `migrate`, `ingest`, `api`, and `postgres-exporter` next to the `prometheus` and `grafana` services of step 41, with the db image pinned as `timescale/timescaledb:2.31.0-pg17@sha256:b56a3f51991df1f79d0c1a0841cad5c5fa8058fdd6a3ee6886394a6d4c174538` ([D16](../decisions.md#d16-set-the-stores-rules-for-migrations-writes-and-tests)). `ingest` and `api` start only after `migrate` has finished. The API connects as a role with `SELECT` rights only ([the API reads and never writes](../design/05-api.md#the-api-reads-and-never-writes)). Each service reads the one key `database.url` ([D30](../decisions.md#d30-read-the-node-urls-the-database-url-and-the-api-address-from-config)): `migrate` and `ingest` get the writer role's URL in `BT_DATABASE__URL`, and `api` gets the read-only role's URL in the same variable. `api` listens on `api.listen_addr`, `0.0.0.0:8080`. A script in `deploy/db/init/` creates that role before any migration runs, with `GRANT CONNECT`, `GRANT USAGE ON SCHEMA public`, and `ALTER DEFAULT PRIVILEGES FOR ROLE <migrate role> IN SCHEMA public GRANT SELECT ON TABLES TO <api role>`. The default covers only tables that the named role creates, so it must name the role that runs `migrate` (`spikes/crates/store` and `spikes/crates/api`, `api-role`). Prometheus scrapes `ingest:9100` as job `bt-ingest` and `api:9101` as job `bt-api`.
- **Who writes what.** The agent writes it. You review the ports and the start order. Review checks:
  - `migrate` uses `service_completed_successfully`.
  - The default privileges name the migrate role, so the API role also reads tables and chunks that later migrations create.
  - The node is not a service, and `BT_CHAIN__URLS` comes from the environment.
  - No secret sits in the file.
- **Concepts.** None new.
- **Check.** `docker compose -f deploy/compose.yaml up -d && docker compose -f deploy/compose.yaml exec ingest bt-indexer status` lists five jobs. A `SELECT` as the API role on a table that a migration created works. An `INSERT` as the API role fails with SQLSTATE 42501, `permission denied for table <name>`.

## Step 58. `ci: add sqlx, deny, machete, docs, coverage, OpenAPI, and image jobs`

- **Goal.** Every job of [CI](../design/09-quality.md#ci-checks-every-pull-request) runs on each push and each pull request, and the daily drift job runs against tao-data-api.
- **Files.** `.github/workflows/ci.yml`, `.github/workflows/drift.yml`, `deny.toml`, `tests/compat/drift.mjs`.
- **Brief for the agent.** Add `cargo sqlx prepare --workspace --check`, `cargo deny check`, `cargo machete`, the OpenAPI diff, `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS="-D warnings"`, `cargo llvm-cov nextest`, `cargo bench --workspace --no-run`, which keeps the benchmarks compiling without comparing results ([Benchmark the hot paths](../design/09-quality.md#benchmark-the-hot-paths)), and the image build. Switch the test job to `cargo nextest run --workspace` with Docker and Node, and put the database tests in a nextest test group with `max-threads = 1`. `cargo nextest` runs each test in its own process, and step 9 gives each test binary one database container with a fixed name that a start removes if it is left over, so without the group one test can remove a container another is using. Write `drift.mjs`, which extracts the five schema declarations from a tao-data-api checkout and fails when one differs from the copy of step 44, and schedule it once a day.
- **Who writes what.** The agent writes it and fixes what each new job reports. You review the permissions and secrets. Review checks:
  - The drift job never runs on pull requests from forks.
  - Coverage reports without a threshold.
  - Each fix the new jobs force is its own commit.
- **Concepts.** None new.
- **Check.** `git push origin master && gh run watch` shows every job passing on the push to `master`. `gh run list --branch master --limit 1` shows the same run. `node tests/compat/drift.mjs <tao-data-api checkout>` exits 0.

## Step 59. `feat(deploy): add alert rules and a runbook section per alert`

- **Goal.** The release 1 alerts of [alert on symptoms](../design/07-observability.md#alert-on-symptoms), a runbook section for each, and the API and Database dashboards next to the Indexer dashboard of step 41.
- **Files.** `deploy/prometheus/alerts.yaml`, `docs/runbook.md`, `deploy/grafana/dashboards/{api.json,database.json,indexer.json}`, `scripts/check-runbook.sh`.
- **Brief for the agent.** Copy the eight release 1 rules, `IndexerDown` through `ApiLatency`, with their `for` and severity. Leave out `PositionDriftHigh`, which is release 2. `check-runbook.sh` fails when an alert has no heading in the runbook. Build the dashboards of [provision the dashboards](../design/07-observability.md#provision-the-dashboards-from-files), with the custom `postgres_exporter` query on `hypertable_detailed_size`. Export any metric of [Runner](../design/07-observability.md#runner) or [Node and database](../design/07-observability.md#node-and-database) that a panel needs and steps 29 and 30 did not add.
- **Who writes what.** The agent writes the rules, the script, and the dashboards. You write the runbook text, because an on-call engineer will ask about it. Review checks:
  - Each expression matches the design doc.
  - `ApiLatency` reads `bt_http_request_seconds_bucket`, which metrics-exporter-prometheus 0.18.3 emits only with the buckets that step 43 sets, `[0.005, 0.025, 0.1, 0.5, 1.0, 2.5, 10.0]` (`spikes/crates/api`, `metrics-buckets`).
  - Every Progress panel turns red on its job's `IndexerStalled` or `IndexerLagging`.
  - Panels label by route template and worker only.

  The dashboard JSON passes 400 lines on its own, so the natural second commit is `feat(deploy): add the API and database dashboards`.
- **Concepts.** None new.
- **Check.** `docker run --rm -v "$PWD/deploy/prometheus:/p" --entrypoint promtool prom/prometheus check rules /p/alerts.yaml` prints `SUCCESS: 8 rules found`. `scripts/check-runbook.sh` exits 0. `curl -s localhost:9101/metrics | grep -c '^bt_http_request_seconds_bucket'` prints a number above 0.

## Step 60. `docs(readme): open with docker compose up and the measured numbers`

- **Goal.** Close M5 with the README of [the roadmap](../design/10-roadmap.md#m5-ship-release-1): the two commands, the `status` output, a dashboard screenshot, the architecture diagram, the measured numbers from steps 42 and 52, and the known limits.
- **Files.** `README.md`, `docs/images/`.
- **Shape.** No code. The README opens with `docker compose -f deploy/compose.yaml up -d` and `docker compose -f deploy/compose.yaml exec ingest bt-indexer status --watch`, as [Run everything with Docker Compose](../design/09-quality.md#run-everything-with-docker-compose) asks.
- **Who writes what.** You write it. The agent follows it on a clean machine and reports every step that did not work. Review checks:
  - Every number names its machine.
  - The known limits include release 2's missing events and positions.
  - No command depends on a file outside the repository.
- **Concepts.** None new.
- **Check (M5).** On a clean machine, `docker compose up` with a node URL starts every job, `bt-indexer status` shows their progress, and `curl -si localhost:8080/readyz | head -1` prints `HTTP/1.1 200 OK` once every window is covered.

## Step 61. `chore(release): tag v0.1.0 with a generated changelog`

- **Goal.** A `v0.1.0` tag that builds and pushes the image and publishes a GitHub release ([release](../design/09-quality.md#release)).
- **Files.** `cliff.toml`, `CHANGELOG.md`, `.github/workflows/release.yml`.
- **Brief for the agent.** Configure `git-cliff` to group Conventional Commits by type and scope. The release workflow runs on `v*` tags, builds the image, pushes it to GitHub Container Registry, and publishes a release whose notes name the highest runtime spec with decoder fixtures.
- **Who writes what.** The agent writes the config and the workflow. You read `CHANGELOG.md`, which tests every commit subject in this plan, and push the tag. Review checks:
  - No stray entry in the changelog, and the only unscoped entries are the plan's `ci:`, `docs:`, and `build:` subjects.
  - The release names the highest fixture spec from `fixtures/metadata/`.
- **Concepts.** None new.
- **Check.** `git cliff --latest` lists the release's commits grouped by type, and `gh release view v0.1.0` shows the release with its image.

## Stop here

- **Demo.** A clean machine goes from `git clone` to `/readyz` 200 with `docker compose up`, and Grafana shows the Progress row and the API dashboard.
- **Explain without notes.**
  - Follow one block from the finalized head to a value in a compat response. Name each crate it passes through.
  - What does the process do on a decode error, a node outage, a database outage, and an API overload?
  - Why is `migrate` a separate service that runs once, and why does the API connect with a read-only role?
  - Which alerts page and which open a ticket, and why does `IndexerStalled` survive a crash loop?
- **Roadmap.** Closes M5, and with it release 1.
