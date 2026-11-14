# Stage 7. Shadow the squid

This stage builds `bt-indexer compat diff`, which compares our five routes with the reference squid at the same block, and then runs it for 24 hours while a staging tao-data-api reads from us. It closes roadmap M4. Before you start, stage 6 must be done. You also need the URL of a running squid, written `$SQUID_URL` below, and a staging deploy of tao-data-api whose `INDEXER_DATA_API_2` you can point at the indexer.

## Step 53. `feat(cli): diff /pools against the squid at an aligned block`

- **Goal.** `bt-indexer compat diff --squid <url> --route pools` finds our block whose `(netuid, total_tao, total_alpha)` set equals the squid's response, then prints every field that differs at that block ([diff live responses](../design/09-quality.md#diff-live-responses-against-the-squid)).
- **Files.** `bin/indexer/src/compat_diff/{mod.rs,compare.rs,pools.rs}`, `crates/store/src/queries/pools.rs` (a query at a given block). Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  pub enum Diff {
      Missing { route: &'static str, key: String },
      Extra { route: &'static str, key: String },
      Field { route: &'static str, key: String, field: &'static str, ours: Value, theirs: Value },
  }
  pub fn compare_rows(route: &'static str, ours: &[Value], theirs: &[Value], key: &[&str], keep: &[&str], apy: &[&str]) -> Vec<Diff> { todo!() }
  pub fn apy_matches(ours: f64, theirs: f64) -> bool { todo!() }   // |a - b| <= 1e-4 * (1 + |b|)
  async fn align_pools(store: &Store, squid: &[Value]) -> Result<Option<BlockNumber>, StoreError> { todo!() }
  async fn pools_at(store: &Store, block: BlockNumber) -> Result<Vec<PoolDto>, StoreError> { todo!() }
  ```

- **Who writes what.** You write `compare_rows`, `apy_matches`, and `align_pools`. The agent writes the HTTP fetch, the subcommand, the text output, and a `--json` flag that prints one JSON line per run. `pools_at` reuses `pool_dto` from step 48. Review checks:
  - `align_pools` searches only our last 100 `subnet_price` blocks.
  - No match is a difference, not a skip.
  - Only the keys that tao-data-api's schemas keep are compared.
  - Strings and integers match exactly, and a number against a string is a difference.
  - Every key in `apy` compares through `apy_matches`, never `==`, because serde_json writes `Some(0.0)` as `0.0` where the squid writes `0`, and `serde_json::Value` treats the pair as unequal ([D29](../decisions.md#d29-return-a-null-apy-where-the-squids-sql-errors), `spikes/crates/api`, `value-zero`).
- **Concepts.**
  - *`tokio::join!`.* Both sides are fetched at once, so the two responses describe blocks as close together as possible. Trap: `Promise.all` habits. `join!` waits for every future and returns every result, while `try_join!` returns at the first error and drops the other future. Source: [tokio, `join!`](https://docs.rs/tokio/1.54.1/tokio/macro.join.html).
  - *`serde_json::Value` as the compared type.* The diff compares raw JSON, so a field that the squid sends as a number and we send as a string shows up. Trap: parsing both sides into our own DTO, which hides exactly the type differences that make tao-data-api fail. Source: [serde_json, `Value`](https://docs.rs/serde_json/1.0.154/serde_json/enum.Value.html).
- **Check.** `cargo test -p bt-indexer compare_rows` passes, including `compare_rows_float_zero_equals_integer_zero`, where ours is `0.0` and theirs is `0` with no difference. `cargo run -p bt-indexer -- compat diff --squid $SQUID_URL --route pools` prints `pools: aligned at block <n>, 0 differences`.

## Step 54. `feat(cli): diff the validator and identity routes against the squid`

- **Goal.** The other four routes, fetched only while both sides serve the same validator snapshot, with the deviations of [D28](../decisions.md#d28-anchor-apy-on-the-indexed-head-and-break-rank-ties-by-hotkey) and [D29](../decisions.md#d29-return-a-null-apy-where-the-squids-sql-errors) reported apart from the differences. The step also answers two open questions: the request headers tao-data-api sends under Cloudflare Workers, and whether a compat route needs `ValidatorPermit` or `Keys`.
- **Files.** `bin/indexer/src/compat_diff/{validators.rs,identities.rs,stake.rs,deviation.rs}`.
- **Shape.**

  ```rust
  pub enum Deviation {
      TieOrder { route: &'static str, key: String },
      ApyWindowEdge { route: &'static str, key: String },
      SquidApyError { route: &'static str, key: String },
  }
  pub fn snapshot_window(checkpoint: BlockNumber, head: BlockNumber) -> Option<BlockNumber> { todo!() }
  pub fn split_deviations(diffs: Vec<Diff>, ctx: &DeviationContext) -> (Vec<Diff>, Vec<Deviation>) { todo!() }
  fn sample_hotkeys(ranked: &[Hotkey], random: usize, seed: u64) -> Vec<Hotkey> { todo!() }
  ```

- **Who writes what.** You write `snapshot_window`, `split_deviations`, and `DeviationContext`, which holds what the three deviation checks need: both 30-day APY boundaries, the `ts` of each compared row, the keys of our rows that D29 names, and the squid's 500 answers by route and key. The agent writes the fetch loops, the sampling, and the output. Review checks:
  - `snapshot_window` returns `Some(b)` only while the `validators` checkpoint is at a snapshot block `b` and the head is between `b + 20` and `b + 80`.
  - The route sample matches [Diff live responses against the squid](../design/09-quality.md#diff-live-responses-against-the-squid), every netuid in our `/pools` plus 0 and the top 50 hotkeys by rank plus 50 random ones.
  - Null against a number is a difference.
  - Row order must match except within a run of equal sort values.
  - An APY counts as a window-edge deviation only when its row's `ts` lies between our 30-day boundary and the squid's.
  - A squid 500 counts as `SquidApyError` only when our answer to the same request holds a row that D29 names, a 100 % take with 10 or more earning epochs, where our gross APY is null, or a compounded value that overflows, where both our APYs are null. Any other squid 500 is a difference.

  This step likely passes 400 lines. The natural second commit is `feat(cli): diff the per-subnet and per-hotkey routes against the squid`.
- **Answer the two open questions.**
  - *Headers under Workers.* Under Node, tao-data-api's requests carry `accept: */*`, `accept-encoding: gzip, deflate`, and `user-agent: node`, and its code adds only `signal` (`spikes/compat/consumer.mjs`; `spikes/crates/api`, `consumer-headers`). Point the staging deploy at us, log the headers of one request it sends, and record them in `docs/shadow-run.md`. Confirm that the `accept-encoding` it sends gets a body it decodes, with no Zod failure in the staging log.
  - *`ValidatorPermit` and `Keys`.* The squid's validator snapshot also reads them (`main.ts:1565-1689`) for `validator_subnet.has_validator_permit` and nominator-only figures. Search the squid's five route queries for those columns. If one reads them, the stake-route diff shows it as Extra or Missing rows for hotkeys without a permit. Record the answer as a decisions entry, and open a fix before step 55 if a route needs them.
- **Concepts.** None new.
- **Check.** `cargo run -p bt-indexer -- compat diff --squid $SQUID_URL` prints one line per route with `0 differences` and a count of documented deviations, then a total. The Workers headers and the `ValidatorPermit` answer are written down.

## Step 55. `docs: record the 24-hour shadow run`

- **Goal.** Close M4. Run `compat diff` every 10 minutes for 24 hours while the staging tao-data-api reads from us through `INDEXER_DATA_API_2`, and commit what happened as `docs/shadow-run.md`.
- **Files.** `scripts/shadow-run.sh`, `docs/shadow-run.md`.
- **Brief for the agent.** `scripts/shadow-run.sh` runs `bt-indexer compat diff --squid $SQUID_URL --json` every 10 minutes and appends each line to `runs.jsonl`. It never stops on a difference, so one bad run does not end the record.
- **Who writes what.** The agent writes the script. You run it, read every difference, and write `docs/shadow-run.md`: the setup, the request headers and the `ValidatorPermit` answer from step 54, the run count, each documented deviation that appeared (a squid 500 on a D29 row among them), the Zod failure and 502 counts from the staging logs, and every fix the run forced. Review checks:
  - Each fix lands in its own `fix(...)` commit, and the 24 hours restart after it.
  - The document names the indexer commit and the squid version.
- **Concepts.** None new.
- **Check (M4).** `jq -s 'length, (map(.differences) | add)' runs.jsonl` prints `144` and `0`. The staging logs show no Zod failure and no 502.

## Stop here

- **Demo.** One live `compat diff` run with `0 differences`, and `docs/shadow-run.md` with the 24-hour record.
- **Explain without notes.**
  - Why can two responses fetched at the same moment describe different blocks, and how does the diff align `/pools` and the validator routes?
  - Why is the APY tolerance `1e-4 × (1 + |b|)`, and what would make it too loose?
  - Which deviations are expected, and which decision records each one?
  - Why does a squid 500 on a stake route not count as a difference, and when does it?
  - What does tao-data-api do when our response fails its schema, or when we answer 503?
- **Roadmap.** Closes M4.
