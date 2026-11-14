# Stage 1. Prove the read path against the real node

This stage reads real chain data at old blocks, one value per step, and prints it with a `bt-indexer chain ...` command. By its end you have the true shape of `Chain` and `BlockSource`. It closes no roadmap check yet. It shows the decoding rule of [D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec) at a real upgrade block before any database or runner code depends on it. Storage and events of block n decode with the metadata of the spec that executed n, and a runtime API call at n uses the metadata of n's own state spec. Release 1 reads only specs 475 to 490 ([D3](../decisions.md#d3-set-the-milestone-scope-by-what-release-1-reads)), but the rule shows only at an upgrade block, so the checks use the dTAO upgrade at 4,920,350.

Before you start, set `BT_CHAIN__URLS` in your shell to an archive node's `wss://` URL. If the URL carries a key after a `?`, put it in single quotes, because the shell treats a `?` as a pattern.

```sh
export BT_CHAIN__URLS='wss://<archive node>:443'
```

The keyed archive node allows 200 calls a second per key and answers JSON-RPC `-32029` above that. The same gateway with no key accepts the connection and allows 10 calls a second, so a URL that lost its key starts fine and then answers mostly `-32029` (`spikes/crates/chain-errors`, `gateway-keyless`). The public archive node runs at about 9 calls a second at 4 in flight, measured on 2026-11-12 (`spikes/crates/chain-errors`, `P32`). Every check in this stage makes only a few calls, so wait a minute and run it again if you see `-32029`.

## Step 1. `build: create the workspace and print a block's runtime specs`

- **Goal.** Replace the `cargo init` package with a two-crate workspace, and print which spec executed a block and which spec its state holds. The line it prints for block 4,920,350 shows the rule the whole design rests on. Spec 219 executed that block, and its state already reports spec 233. So its storage and events decode with 219's metadata, and a runtime API call at it runs 233's code ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)).
- **Files.** Delete `src/main.rs`. Write the root `Cargo.toml`, `rust-toolchain.toml`, `clippy.toml`, `crates/chain/Cargo.toml`, `crates/chain/src/lib.rs`, `bin/indexer/Cargo.toml`, and `bin/indexer/src/main.rs`.
- **Who writes what.** You write all of it. Then ask the agent to review. Review checks:
  - No `unwrap`, `expect`, or `panic!` in `bt-chain`.
  - `anyhow` appears only in `bt-indexer`.
  - `main` holds no RPC logic.
  - Every public item has a `///` line.
- **Concepts.**
  - *Cargo workspace.* Two crates now, and seven by release 1, share one `Cargo.lock`, one dependency list, and one lint set. Trap: npm-style hoisting does not exist here. Each crate still names every dependency it uses, with `workspace = true`. Source: [The Cargo Book, "Workspaces"](https://doc.rust-lang.org/cargo/reference/workspaces.html).
  - *`#[tokio::main]`.* The macro builds a runtime and runs `main` on it. Trap: there is no ambient event loop. A future does nothing until something awaits it or spawns it. Source: [tokio, `attr.main`](https://docs.rs/tokio/1.54.1/tokio/attr.main.html).
  - *thiserror in libraries, anyhow in the binary.* A caller of `bt-chain` matches on `ChainError` variants to decide what to do, so the library returns a typed enum. Only `main` collapses errors into `anyhow::Error` to print them. Trap: one catch-all error type everywhere, the way `throw new Error(...)` works. Source: the thiserror README, section "Comparison to anyhow".
- **Check.** `cargo run -p bt-indexer -- chain spec --block 4920350` prints `block=4920350 hash=0x9352d23c... executed_by=219 state=233`.

### Check the starting state

Run these from the repository root.

```sh
git status --short
rustc --version
```

Expect `rustc 1.99.1` or later. The repository starts as a `cargo init` package. It tracks `Cargo.toml`, `src/main.rs`, and a `.gitignore` that holds `/target`, and `git status` shows only `A  Cargo.lock`. Keep the staged `Cargo.lock`. A workspace with a binary commits its lock file, and the first build rewrites it.

This step's commit holds only the workspace. If `git status` shows other changes, such as the docs, commit them first in a commit of their own. Name their paths, so the staged `Cargo.lock` stays out of that commit.

```sh
git add docs
git commit -m "docs: add the design and the implementation plan" -- docs
git status --short
```

Create the branch for this step.

```sh
git switch -c step-01
```

### Write the files

1. Delete the `cargo init` package.

   ```sh
   git rm src/main.rs
   ```

2. Write `rust-toolchain.toml`. It pins the compiler this plan was built with, so CI and your machine agree.

   ```toml
   [toolchain]
   channel = "1.99.1"
   components = ["clippy", "rustfmt"]
   ```

3. Write `clippy.toml`. The workspace denies `unwrap` and its relatives in library code, and these keys allow them in tests.

   ```toml
   allow-unwrap-in-tests = true
   allow-expect-in-tests = true
   allow-panic-in-tests = true
   allow-indexing-slicing-in-tests = true
   ```

4. Replace the root `Cargo.toml`. `subxt` is pinned exactly, because the design relies on the behavior of 0.50.3. The other versions are the ones this plan was built with. The tokio features cover the product code of every later step: `time` for step 6's timeout, and `signal` and `sync` for step 10. Step 27 adds `test-util` as a dev feature, for paused time. Later steps add crates to this list. [Dependencies by step](README.md#dependencies-by-step) gives each one with its version, its features, and the step that first needs it.

   ```toml
   [workspace]
   resolver = "3"
   members = ["bin/indexer", "crates/chain"]

   [workspace.package]
   version = "0.1.0"
   edition = "2024"
   publish = false

   [workspace.dependencies]
   bt-chain = { path = "crates/chain" }
   anyhow = "1.0.104"
   clap = { version = "4.6.7", features = ["derive", "env"] }
   subxt = "=0.50.3"
   thiserror = "2.0.21"
   tokio = { version = "1.54.1", features = ["macros", "rt-multi-thread", "signal", "sync", "time"] }

   [workspace.lints.rust]
   unsafe_code = "forbid"
   missing_docs = "warn"

   [workspace.lints.clippy]
   unwrap_used = "deny"
   expect_used = "deny"
   indexing_slicing = "deny"
   panic = "deny"
   ```

5. Write `crates/chain/Cargo.toml`.

   ```toml
   [package]
   name = "bt-chain"
   version.workspace = true
   edition.workspace = true
   publish.workspace = true

   [dependencies]
   subxt.workspace = true
   thiserror.workspace = true

   [lints]
   workspace = true
   ```

6. Write `bin/indexer/Cargo.toml`.

   ```toml
   [package]
   name = "bt-indexer"
   version.workspace = true
   edition.workspace = true
   publish.workspace = true

   [dependencies]
   anyhow.workspace = true
   bt-chain.workspace = true
   clap.workspace = true
   tokio.workspace = true

   [lints]
   workspace = true
   ```

7. Write `crates/chain/src/lib.rs` with this shape. `missing_docs` is a warning, and `cargo clippy -- -D warnings` turns it into an error, so every public item and every public field needs its `///` line.

   ```rust
   //! Reads Bittensor chain data from an archive node.

   use subxt::config::substrate::H256;
   use subxt::config::{Header, RpcConfigFor, SubstrateConfig};
   use subxt::rpcs::{LegacyRpcMethods, RpcClient};

   /// A connection to one archive node.
   pub struct Chain {
       rpc: LegacyRpcMethods<RpcConfigFor<SubstrateConfig>>,
   }

   /// The two runtime specs that matter for one block.
   #[derive(Debug)]
   pub struct SpecAt {
       /// The block number.
       pub number: u64,
       /// The block hash.
       pub hash: H256,
       /// The spec that executed the block. It is the spec in the parent's state.
       pub executed_by: u32,
       /// The spec the block's own state reports. Runtime API calls at the block run it.
       pub state: u32,
   }

   /// Errors from the node.
   #[derive(Debug, thiserror::Error)]
   pub enum ChainError {
       /// Any node error. Step 6 splits it into classified variants.
       #[error("node error")]
       Unknown(#[source] Box<subxt::Error>),
   }

   impl Chain {
       /// Connects to a node over a secure WebSocket.
       pub async fn connect(url: &str) -> Result<Self, ChainError> {
           todo!()
       }

       /// Reads the specs at `block`, or at the finalized head when `block` is `None`.
       pub async fn spec_at(&self, block: Option<u64>) -> Result<SpecAt, ChainError> {
           todo!()
       }
   }

   /// Wraps any node error as `ChainError::Unknown` until step 6 classifies it.
   fn unknown(e: impl Into<subxt::Error>) -> ChainError {
       todo!()
   }
   ```

8. Write `bin/indexer/src/main.rs` with this shape. The `///` lines on the clap types become the `--help` text.

   ```rust
   //! The `bt-indexer` binary.

   use bt_chain::Chain;
   use clap::{Parser, Subcommand};

   /// Indexes the Bittensor chain.
   #[derive(Parser)]
   struct Cli {
       #[command(subcommand)]
       cmd: Cmd,
   }

   #[derive(Subcommand)]
   enum Cmd {
       /// Inspect the chain.
       Chain {
           #[command(subcommand)]
           cmd: ChainCmd,
       },
   }

   #[derive(Subcommand)]
   enum ChainCmd {
       /// Print the spec that executed a block and the spec of its state.
       Spec {
           /// The archive node's WebSocket URL.
           #[arg(long, env = "BT_CHAIN__URLS")]
           url: String,
           /// The block number. Omit it to use the finalized head.
           #[arg(long)]
           block: Option<u64>,
       },
   }

   #[tokio::main]
   async fn main() -> anyhow::Result<()> {
       todo!()
   }
   ```

9. Run `cargo check`. The first run downloads and compiles subxt and its dependencies, which takes about 30 seconds with a warm registry. Expect warnings about unused variables and imports, and no errors.

### Fill the bodies

Fill them in this order. Each bullet names the exact call.

- `unknown`. Build `ChainError::Unknown(Box::new(e.into()))`.
- `connect`. Call `RpcClient::from_url(url).await`, map its error with `.map_err(unknown)?`, and wrap the client in `LegacyRpcMethods::new(client)`.
- `spec_at`, the hash. With `Some(n)`, call `self.rpc.chain_get_block_hash(Some(n.into())).await`. It returns an `Option`, and `None` means the node has no such block, so turn `None` into `unknown(subxt::Error::other_str("block not found"))`. With `None`, call `self.rpc.chain_get_finalized_head().await`.
- `spec_at`, the header. Call `self.rpc.chain_get_header(Some(hash)).await`, and treat `None` the same way, with the message `"header not found"`. The `Header` import gives you `header.number()`.
- `spec_at`, the specs. `state` is `self.rpc.state_get_runtime_version(Some(hash))`, read at the block's own hash. `executed_by` is the same call at `Some(header.parent_hash)`. Take `.spec_version` from each result.
- `main`. Call `Cli::parse()`, match `Cmd::Chain { cmd: ChainCmd::Spec { url, block } }`, connect, bind `let s = chain.spec_at(block).await?;`, and print with this line.

  ```rust
  println!("block={} hash={:?} executed_by={} state={}", s.number, s.hash, s.executed_by, s.state);
  ```

Every RPC call in `spec_at` ends in `.await.map_err(unknown)?`. If you are stuck for more than 20 minutes on one body, paste it to the agent and ask for a review of what you have, not for the body.

### Run the checks

Run these four in order.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p bt-indexer -- chain spec --block 4920350
```

Expect no output from `cargo fmt`, no lint warnings from `cargo clippy`, and `test result: ok. 0 passed` from each test target. `cargo clippy` still prints one note, `the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1`. It comes from a dependency, and the exit code is 0. The last command prints this.

```
block=4920350 hash=0x9352d23c47b3f6dc7de623942a9ae0ba14bebb4bb45313a54a8c41c6da16861f executed_by=219 state=233
```

Two more runs show the rule from both sides.

- `cargo run -p bt-indexer -- chain spec --block 4920351` prints `executed_by=233 state=233`. The block after the upgrade runs the new code.
- `cargo run -p bt-indexer -- chain spec` prints `block=<head> hash=0x<64 hex digits> executed_by=490 state=490`, while the head runs spec 490, as it did on 2026-11-12. The block number grows by about one every 12 seconds, and both specs change at the next upgrade.

### If the output differs

| You see | Do this |
|---|---|
| `error[E0599]: no method named 'number' found for struct 'SubstrateHeader<Hash>'` | Add `Header` to the `subxt::config` import. `number()` is a trait method. |
| `` error[E0277]: `?` couldn't convert the error to `ChainError` `` | Add `.map_err(unknown)` before the `?`. The RPC calls return `subxt_rpcs::Error`, and `unknown` converts it through `subxt::Error`. |
| `error: missing documentation for a struct field` | Add a `///` line above that field. |
| `error: the following required arguments were not provided: --url <URL>` | Run the `export BT_CHAIN__URLS=...` line in this shell. |
| `Error: node error` with `Too Many Requests` | The node limits calls (`-32029`). If it comes on the first few calls, the URL has probably lost its key (see the top of this stage), so check that the `export` line quotes it. Otherwise wait a minute and run the command again. |
| `Error: node error` with `status code: 403` | The key in the URL is wrong. The gateway rejects the connection in about 0.3 s (`spikes/crates/chain-errors`, `gateway-handshake-rejected`). |
| `Error: node error` with `status code: 404` | The path in the URL is wrong. An unrouted path such as `/zzz` gives 404 (`spikes/crates/chain-errors`, `gateway-handshake-rejected`). |
| `Error: node error` with `Connection refused` or a TLS error | Check that the URL starts with `wss://` and ends with `:443`. |
| `Error: node error` with `Other error: block not found` | The block number is above the finalized head. Use a lower one. |
| `executed_by=233 state=233` for block 4,920,350 | `executed_by` reads at the block's own hash. Read it at `header.parent_hash`. |
| `cargo fmt --all --check` prints a diff | Run `cargo fmt --all`, then run the four checks again. |
| `rustup` downloads a toolchain on the first build | Expected when your default toolchain is not 1.99.1. Let it finish. |

### Commit

Stage everything and check what goes into the commit.

```sh
git add -A
git status --short
```

Expect `D  src/main.rs`, `M  Cargo.toml`, `A  Cargo.lock`, `A  clippy.toml`, `A  rust-toolchain.toml`, and the four new files under `bin/` and `crates/`. Nothing under `target/` may appear. If a file is missing or extra, fix it and run `git add -A` again.

```sh
git commit -m "build: create the workspace and print a block's runtime specs" \
  -m "Block 4,920,350 applied the dTAO upgrade. Spec 219 executed it, so its storage and events decode with 219's metadata, while its state already reports spec 233 (D6)."
git switch master
git merge --ff-only step-01
git branch -d step-01
```

You are done when `git log --oneline -1` shows the new subject and the four checks pass on `master`. Step 2 can wait for the next sitting.

## Step 2. `ci: check format, lints, and tests on every push`

- **Goal.** Every later commit runs through the same three commands the plan uses.
- **Files.** `.github/workflows/ci.yml`.
- **Brief for the agent.** Write one job that installs the toolchain from `rust-toolchain.toml`, caches `~/.cargo` and `target`, and runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`. No test may need a node URL. Set `SQLX_OFFLINE=true` now, so step 11 does not have to change CI.
- **Who writes what.** The agent writes the workflow. You review it. Review checks:
  - The toolchain comes from the file, not from `stable`.
  - No step reads `BT_CHAIN__URLS`.
  - The cache key includes `Cargo.lock`.
- **Concepts.** None new.
- **Check.** `git push origin master && gh run watch` shows the three steps green.

## Step 3. `feat(domain): parse block numbers and specs into newtypes at the CLI`

- **Goal.** Start `bt-domain` with the first newtypes, and reject a block number that does not fit `u32` before it reaches the chain code. `BlockRef` carries both specs of the block, so every later read can pick the right metadata ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)).
- **Files.** `crates/domain/{Cargo.toml,src/lib.rs,src/block.rs}`. Edit `crates/chain/src/lib.rs` and `bin/indexer/src/main.rs`. Add `crates/domain` to the workspace members and `bt-domain` to `[workspace.dependencies]`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
  pub struct BlockNumber(u32);
  impl BlockNumber {
      pub const fn new(n: u32) -> Self { todo!() }
      pub const fn get(self) -> u32 { todo!() }
      pub fn prev(self) -> Option<BlockNumber> { todo!() } // checked_sub(1)
      pub fn next(self) -> Option<BlockNumber> { todo!() } // checked_add(1)
  }
  #[derive(Debug, thiserror::Error, PartialEq, Eq)]
  pub enum BlockNumberError {
      #[error("block number is not a number")] NotANumber,
      #[error("block number does not fit in u32")] TooLarge,
  }
  impl FromStr for BlockNumber { type Err = BlockNumberError; fn from_str(s: &str) -> Result<Self, Self::Err> { todo!() } }
  impl TryFrom<u64> for BlockNumber { type Error = BlockNumberError; fn try_from(n: u64) -> Result<Self, Self::Error> { todo!() } }
  impl From<BlockNumber> for u64 { fn from(n: BlockNumber) -> u64 { todo!() } }
  impl fmt::Display for BlockNumber { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { todo!() } }
  #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
  pub struct SpecVersion(pub u32); // with new, get, and Display
  #[derive(Clone, Copy, PartialEq, Eq, Hash)]
  pub struct BlockHash(pub [u8; 32]);
  impl fmt::Debug for BlockHash { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { todo!() } } // 0x and 64 hex digits
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct SpecAt { pub executed_by: SpecVersion, pub state: SpecVersion }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct BlockRef { pub number: BlockNumber, pub hash: BlockHash, pub spec: SpecAt }
  ```

  `BlockRef`, `SpecAt`, and `BlockHash` are `Copy`, because every later signature takes `at: BlockRef` by value and uses it again. `BlockHash` prints as hex, so step 1's output line stays the same. `Display` on `BlockNumber` is what error messages such as step 6's `at block 4920350` use. Step 7 adds `NetUid` and `Rao` with the same `new` and `get`.
- **Who writes what.** You write it. Review checks:
  - No `as` casts between `u64` and `u32`.
  - The field of `BlockNumber` is private. Code outside `bt-domain` uses `new`, `get`, and `u64::from`.
  - In code, `n - 1` and `first_block + 1` are `prev()` and `next()`, which return `None` at the ends of the range.
  - `--block` has type `Option<BlockNumber>` in clap.
  - `SpecAt` moves out of `bt-chain`, and `spec_at` returns a `BlockRef`.
  - `bt-domain` has no dependency on `bt-chain`.
  - `crates/domain/src/lib.rs` opens with `#![deny(clippy::arithmetic_side_effects)]`, which step 34 and step 47 rely on. Put the lint in `lib.rs`, because a crate with `[lints] workspace = true` cannot add a lint of its own in its `Cargo.toml`, and cargo then fails with `cannot override workspace.lints in lints`.
- **Concepts.**
  - *Newtype.* Block numbers, specs, and later netuids are all small integers that must never mix. Trap: a TypeScript type alias is structural and brands nothing, so the habit of `type BlockNumber = number` gives no protection. Source: The Rust Book, "Advanced Types", section "Using the Newtype Pattern for Type Safety and Abstraction".
  - *Parse at the boundary.* clap calls `FromStr` on the argument, so a bad value never enters the program, and the code behind it never checks again. Trap: validating the same number again deep inside the library. Source: [clap, `value_parser!`](https://docs.rs/clap/4.6.7/clap/macro.value_parser.html).
- **Check.** `cargo run -p bt-indexer -- chain spec --block 5000000000` prints `error: invalid value '5000000000' for '--block <BLOCK>': block number does not fit in u32`. `cargo test -p bt-domain` passes.

## Step 4. `feat(chain): fetch a block's metadata and report what it declares`

- **Goal.** Get the metadata each kind of read at a block needs, and answer "does this metadata declare item X". Storage and events use the executing spec's metadata, and runtime API calls use the state spec's ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)). The metadata, never the spec number, picks each era ([Chain data](../design/02-chain-data.md#date-each-item-by-the-blocks-metadata)).
- **Files.** `crates/chain/src/runtimes.rs`, `crates/chain/src/items.rs`, the `chain metadata` subcommand.
- **Shape.**

  ```rust
  #[derive(Clone, Default)]
  pub struct Runtimes { metadata: Arc<RwLock<HashMap<SpecVersion, ArcMetadata>>> }
  pub enum Decodes { Storage, RuntimeApi }
  impl Chain {
      pub async fn metadata(&self, at: BlockRef, of: Decodes) -> Result<(SpecVersion, ArcMetadata), ChainError> { todo!() }
  }
  pub enum Item { Storage(&'static str, &'static str), Api(&'static str, &'static str) }
  pub fn declares(md: &Metadata, item: Item) -> bool { todo!() }
  ```

- **Who writes what.** You write it. Review checks:
  - The fetch is `state_call("Metadata_metadata_at_version", SCALE(v: u32), hash)`, decoded as `Option<Vec<u8>>` that holds `RuntimeMetadataPrefixed`. It is never `state_getMetadata`, which returns V14 with no runtime API list ([D7](../decisions.md#d7-fetch-metadata-with-metadata_metadata_at_version)).
  - `v` is the highest value of `Metadata_metadata_versions` after dropping `4294967295`, which is 15 at spec 233 and 16 at spec 490.
  - `Decodes::Storage` takes `at.spec.executed_by` and fetches at block n−1. `Decodes::RuntimeApi` takes `at.spec.state` and fetches at n.
  - A `BlockRef` holds no parent hash, so on a cache miss for `Decodes::Storage`, `Chain::metadata` first calls `block_ref(n - 1)` on the source, which costs four more calls.
  - The returned `SpecVersion` is the spec whose metadata it is, and step 5 prints it.
  - No lock guard lives across an `.await`.
  - The cache fetches each spec once. One fetch takes 0.9 to 8.8 s per spec (`spikes/crates/chain-errors`, `metadata-fetch-time`).

  The probe `spikes/crates/chain-specs/src/meta.rs` makes the same calls (`spikes/crates/chain-specs`, `meta-calls`, `P4`). The shape writes `impl Chain`, because `Chain` is not generic yet. Step 5 turns it into `impl<S: BlockSource> Chain<S>`.
- **Concepts.**
  - *`Arc<RwLock<_>>`.* Every job's `Chain` shares one metadata cache, and the subxt hooks of step 17 take `&self`, so the metadata cache needs shared ownership and interior mutability. Trap: holding a `std::sync` guard across `.await`, which makes the future non-`Send` and fails to compile at `tokio::spawn`. Source: tokio tutorial, "Shared state".
- **Check.** Run both commands.
  - `cargo run -p bt-indexer -- chain metadata --block 4920350` prints `spec=233 SubtensorModule.SubnetTAO=yes SwapRuntimeApi.current_alpha_price_all=no`. Spec 233, the state spec, gives the metadata a runtime API call at that block uses.
  - `cargo run -p bt-indexer -- chain metadata`, at the head, prints `spec=490 SubtensorModule.SubnetTAO=yes SwapRuntimeApi.current_alpha_price_all=yes`, while the head runs spec 490, as it did on 2026-11-12.

  The head line catches a fetch that returns metadata with no runtime API list, such as version 14. That metadata prints `=no` at the head, where `=yes` is expected.

## Step 5. `feat(chain): read one storage value through a BlockSource`

- **Goal.** Read `Timestamp.Now` at a block through the `BlockSource` trait of [Quality](../design/09-quality.md#test-the-runner-end-to-end-without-a-node), and decode the raw bytes with the executing spec's metadata ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)). `Chain` becomes generic over its source, so tests can serve recorded bytes later ([D18](../decisions.md#d18-give-every-job-one-trait-and-build-the-runner-from-a-spawned-read-stage-and-a-batching-writer)).
- **Files.** `crates/chain/src/source.rs` (the trait and `SubxtSource`), `crates/chain/src/decode.rs`, `crates/chain/src/error.rs` (`DecodeError` and `DecodeKind`), `crates/domain/src/time.rs` (`Timestamp`), `crates/domain/src/rows.rs` (`BlockState`), the `chain read` subcommand. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  pub trait BlockSource: Send + Sync + 'static {
      fn finalized_head(&self) -> impl Future<Output = Result<BlockRef, ChainError>> + Send;
      fn block_ref(&self, n: BlockNumber) -> impl Future<Output = Result<BlockRef, ChainError>> + Send;
      fn storage(&self, at: BlockRef, q: StorageQuery) -> impl Future<Output = Result<StoragePage, ChainError>> + Send;
      fn runtime_api(&self, at: BlockRef, call: RuntimeCall) -> impl Future<Output = Result<Vec<u8>, ChainError>> + Send;
  }
  #[derive(Clone, Debug)]
  pub enum StorageQuery { Keys(Vec<Vec<u8>>), Prefix { prefix: Vec<u8>, start: Option<Vec<u8>>, page: u32 } }
  #[derive(Clone, Debug, Default)]
  pub struct StoragePage { pub entries: Vec<(Vec<u8>, Option<Vec<u8>>)> } // None: the node holds no value
  #[derive(Clone, Debug)]
  pub struct RuntimeCall { pub name: String, pub args: Vec<u8> }
  pub struct SubxtSource { client: RpcClient, rpc: LegacyRpcMethods<RpcConfigFor<SubstrateConfig>> }
  pub struct Chain<S: BlockSource> { source: S, runtimes: Runtimes }
  impl Chain<SubxtSource> { pub async fn connect(url: &str) -> Result<Self, ChainError> { todo!() } }
  impl<S: BlockSource> Chain<S> {
      pub fn new(source: S) -> Self { todo!() }
      pub fn source(&self) -> &S { todo!() }
      pub async fn timestamp(&self, at: BlockRef) -> Result<(Timestamp, SpecVersion), ChainError> { todo!() }
      pub async fn block_state(&self, at: BlockRef) -> Result<BlockState, ChainError> { todo!() }
  }
  fn decode_value<T: IntoVisitor>(md: &Metadata, at: BlockRef, pallet: &'static str, entry: &'static str, bytes: Option<&[u8]>) -> Result<Option<T>, DecodeError> { todo!() }
  // in bt-domain
  #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
  pub struct Timestamp(pub chrono::DateTime<Utc>); // Display prints RFC 3339 to the second, such as 2025-02-13T21:40:36Z
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct BlockState { pub at: BlockRef, pub ts: Timestamp }
  #[derive(Debug, thiserror::Error)]
  #[error("decode {item} at block {block} (spec {spec})")]
  pub struct DecodeError {
      pub kind: DecodeKind, pub block: BlockNumber, pub spec: SpecVersion, pub item: &'static str,
      #[source] pub reason: Box<dyn std::error::Error + Send + Sync>,
  }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum DecodeKind { State, Event }
  // added to ChainError: Decode(#[from] DecodeError)
  ```

- **Who writes what.** You write it. Review checks:
  - `block_ref` fills both specs, as step 1 did, and `finalized_head` does the same for the head, so a command run without `--block` still reads the head.
  - `block_state` builds on `timestamp`, which returns the time with the spec whose metadata decoded it. `chain read` prints that spec as `decoded_with`, never a spec read by a second call.
  - Step 1's `connect` and `spec_at` move into `SubxtSource` and `Chain<SubxtSource>`. The `impl Chain` blocks of steps 1 and 4 become `impl<S: BlockSource> Chain<S>`.
  - `SubxtSource` keeps the `RpcClient` next to `LegacyRpcMethods`, because step 13 sends a raw request through it.
  - A `Prefix` query returns one page of keys with their values, in two calls, `state_getKeysPaged` and then `state_queryStorageAt`.
  - Step 4's metadata calls go through `runtime_api`, so a test source can serve them.
  - Keys come from `subxt::ext::frame_decode::storage::encode_storage_key`. `frame-decode` is reached through `subxt::ext` and is not a direct dependency.
  - `BlockState` and `Timestamp` live in `bt-domain`, because `bt-store` names them later and never depends on `bt-chain` ([D33](../decisions.md#d33-put-the-types-that-several-crates-name-in-bt-domain)).
  - Values decode through `frame-decode` with a `subxt::Metadata` the caller picks, because subxt's `StorageValue::new` is `pub(crate)` and calling it fails with E0624 (`spikes/crates/chain-decode`, `P19`).
  - An item the executing spec does not declare is absent and gives `None`, never zero. A declared item with no stored key decodes the default bytes from the metadata. Our code applies that default, because frame-decode reads only the bytes it is given ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)).
  - `DecodeError` has the fields of [Error handling](../design/06-error-handling.md#each-library-crate-owns-one-error-enum). `spec` is the spec whose metadata the decode used, so `decode_value` fills it with `at.spec.executed_by`, and a runtime API result (step 8) fills it with `at.spec.state`. `item` is the entry name.
  - `DecodeKind::Event` stays unused until release 2.
- **Concepts.**
  - *RPITIT with `+ Send`.* `impl Future + Send` in a trait lets a generic caller pass the future to `tokio::spawn`. Trap: a plain `async fn` in a public trait does not promise `Send`, and the error appears far away, at the spawn. Source: Rust Blog, "Announcing `async fn` and return-position `impl Trait` in traits" (2023-12-21).
  - *Static dispatch.* `Chain<S>` is generic and never `dyn BlockSource`, so each source gets its own compiled copy and no allocation per call. Trap: reaching for `Box<dyn Trait>` the way you would pass an interface in TypeScript. Source: The Rust Book, "Traits: Defining Shared Behavior", section "Traits as Parameters".
- **Check.** `cargo run -p bt-indexer -- chain read --block 4920350` prints `executed_by=219 state=233 decoded_with=219 ts=2025-02-13T21:40:36Z` (`spikes/crates/chain-specs`, `P2`, and the `first_state_block_utc` column of `spikes/data/specs.csv`). `decoded_with` is the `SpecVersion` that `Chain::metadata` returned for the decode. It catches a decoder that used `Decodes::RuntimeApi` metadata, which prints `decoded_with=233`. The `ts` is the same under spec 233's metadata at this block, so it does not catch that decoder.

## Step 6. `feat(chain): classify node errors and time-bound every call`

- **Goal.** Every node call goes through two layers ([D21](../decisions.md#d21-call-the-node-in-two-layers)). In `SubxtSource`, `classified` turns each raw request's `subxt::Error` into a `ChainError` variant ([Error handling](../design/06-error-handling.md#classify-subxterror-when-it-enters-the-crate), [D22](../decisions.md#d22-classify-chain-errors-by-what-the-node-sends-and-bound-the-clients-waits-with-our-own-timeouts)). On `Chain<S>`, `call` wraps each `BlockSource` method in the timeout, so a slow call ends the same way on every source. Step 1's `unknown` helper goes away.
- **Files.** `crates/chain/src/error.rs`, `crates/chain/src/source.rs` (`classified` and `classify`), `crates/chain/src/lib.rs` (`Chain::call`), `crates/domain/src/disposition.rs`, and the `--rpc-timeout-ms` flag of `chain read` in `bin/indexer/src/main.rs`. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum ChainError {
      #[error("node connection lost, client is reconnecting")] Disconnected(#[source] Box<subxt::Error>),
      #[error("rpc request limit reached")] RateLimited(#[source] Box<subxt::Error>),
      #[error("{method} at block {block} timed out after {after:?}")] Timeout { method: &'static str, block: BlockNumber, after: Duration },
      #[error("block {0} not found on node")] BlockNotFound(BlockNumber),
      #[error("no metadata for spec {0}")] MetadataMissing(SpecVersion),
      #[error(transparent)] Decode(#[from] DecodeError),
      #[error("unclassified node error")] Unknown(#[source] Box<subxt::Error>),
  }
  pub enum Disposition { Retry, StopJob, StopProcess }
  impl ChainError { pub fn disposition(&self) -> Disposition { todo!() } }
  // crates/chain/src/source.rs, used only by SubxtSource
  async fn classified<T>(method: &'static str, block: BlockNumber, spec: Option<SpecVersion>,
      request: impl Future<Output = Result<T, subxt::Error>>) -> Result<T, ChainError> { todo!() }
  fn classify(err: subxt::Error, method: &'static str, block: BlockNumber, spec: Option<SpecVersion>, elapsed: Duration) -> ChainError { todo!() }
  // crates/chain/src/lib.rs. Chain gains an rpc_timeout: Duration field.
  impl<S: BlockSource> Chain<S> {
      async fn call<T, Fut>(&self, method: &'static str, block: BlockNumber, request: impl FnMut() -> Fut) -> Result<T, ChainError>
      where Fut: Future<Output = Result<T, ChainError>> { todo!() }
  }
  ```

- **Who writes what.** You write it. Review checks:
  - `classify` and `classified` take a block number and an optional spec, because `chain_getBlockHash`, `chain_getHeader`, `state_getRuntimeVersion`, and `chain_getFinalizedHead` run before any hash or spec is known. `spec` is `None` for the calls inside `block_ref` and `finalized_head`, and `finalized_head` passes block 0.
  - `Chain::call` applies the timeout only, in this step. Step 29 adds the permit, the rate token, the retry, and the counters inside the same function, with no signature change. It takes a closure because each attempt needs a new future.
  - `Chain` gets `block_ref`, `storage`, `runtime_api`, and `finalized_head` methods that wrap the source's method in `call`. Every read in `bt-chain` uses them, step 4's metadata fetch included, and never calls `self.source` directly. `block_ref` passes the method name of its first request, `chain_getBlockHash`, and `finalized_head` passes block 0.
  - The timeout sits in `Chain::call` and not in `SubxtSource`, because step 29's hung-call test runs on a fixture source and expects `Timeout`. The generic `Chain<S>` never holds a `subxt::Error`, so classification sits in `SubxtSource`. Both layers run under the runner (`spikes/crates/runner-live`, `wiring`).
  - `disposition` has no wildcard arm. `classify` holds the only wildcard.
  - `DisconnectedWillReconnect` becomes `Disconnected` wherever subxt nests it.
  - A JSON-RPC user error with code `-32029` (the keyed node) or `-32004` becomes `RateLimited`. No documentation names `-32004`, and neither node has returned it, but mapping it costs nothing, since `is_rpc_limit_reached` matches only chainHead `limitReached`.
  - The node answers an unknown block with 4003 on `state_getStorage`, `state_call`, and `state_getKeysPaged`, and with 4003 for a failing runtime API. It answers 4001 for an unknown block on `state_queryStorageAt`, and 4002 for a `state_getKeysPaged` count above 1000 (`spikes/crates/chain-errors`, `node-error-codes`). All three stay `Unknown` and stop the process.
  - The client's own `RequestTimeout` becomes `Timeout` with `after: elapsed`, not `Unknown`, so a slow call is retried and does not stop the process (`spikes/crates/chain-errors`, `client-request-timeout`). The plain client of this step nests it as `Client(Box<RequestTimeout>)`. `OtherRpcClientError(Client(RpcError(RequestTimeout)))` is the reconnecting client's shape, which arrives in step 29. The test builds the plain shape.
  - A codec error becomes `Decode` with `DecodeKind::State` and the `spec` it was given, since storage decodes outside subxt and only RPC and runtime API results reach `classify`. A codec error with `spec: None` becomes `Unknown`, because `DecodeError` needs a spec.
  - `classify` produces `BlockNotFound` and `MetadataMissing` from subxt's at-block errors, and `block_ref` returns `BlockNotFound` for a `None` hash.
  - Raw legacy calls need `.map_err(subxt::Error::from)`. Every subxt variant is boxed, because `subxt::Error` is 240 bytes.
  - The `reconnecting-rpc-client` feature is not a default and stays off until step 29 names it.
  - `Disposition` lives in `bt-domain`, so `StoreError` can share it in step 9.

  A reference `classify` is in `spikes/crates/chain-errors/src/chain_error.rs` (`spikes/crates/chain-errors`, `ratelimit-classify`, `unknown-block`).
- **Concepts.**
  - *Exhaustive `match`.* A new `ChainError` variant breaks the build at `disposition` until someone decides whether it retries. Trap: a `default:` arm that silently retries the new case. Source: The Rust Book, "The match Control Flow Construct".
  - *Boxing a large error.* `subxt::Error` is big enough that `Result<_, subxt::Error>` trips `clippy::result_large_err`, which `-D warnings` makes fatal (`spikes/crates/chain-errors`, `P16`). A `Box` keeps every `Result` small. Source: the clippy lint list, `result_large_err`.
  - *`tokio::time::timeout`.* It wraps any future and returns `Err(Elapsed)` when the time runs out, dropping the inner future. Trap: expecting the request to stop on the server, as an `AbortController` suggests. The node may still do the work. Source: [tokio, `time::timeout`](https://docs.rs/tokio/1.54.1/tokio/time/fn.timeout.html).
- **Check.** `cargo test -p bt-chain classify` passes `classify_reconnect_is_disconnected`, `classify_rate_limit_is_rate_limited` (both codes), `classify_codec_is_decode`, and `classify_client_request_timeout_is_timeout`. `cargo run -p bt-indexer -- chain read --block 4920350 --rpc-timeout-ms 1` prints `Error: chain_getBlockHash at block 4920350 timed out after 1ms`, because the block hash is the first call the command makes (`spikes/crates/chain-errors`, `classify-tests`).

## Step 7. `feat(chain): read prices from storage for the pre-290 era`

- **Goal.** Read every netuid's reserves, `SubnetAlphaOut`, and `SubnetVolume` in one `state_queryStorageAt`, and compute the price the way the chain did before the swap API existed ([Chain data](../design/02-chain-data.md#price-comes-from-a-weighted-balancer-pool)). The check reads block 4,920,351, the first block spec 233 executed. At the upgrade block 4,920,350 the dTAO items have 0 keys, because storage there is still pre-migration ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec); `spikes/crates/chain-specs`, `P31`). Step 16 tests that block.
- **Files.** `crates/domain/src/{netuid.rs,amount.rs,price.rs,rows.rs}`, `crates/chain/src/jobs/prices.rs`, the `chain prices` subcommand. Dependencies: see [the table](README.md#dependencies-by-step).
- **Shape.**

  ```rust
  // in bt-domain
  #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
  pub struct NetUid(u16); // new(u16), get(self) -> u16
  #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
  pub struct Rao(u64);    // new(u64), get(self) -> u64
  pub struct PricePoint { pub netuid: NetUid, pub price: Rao, pub tao_reserve: Rao, pub alpha_reserve: u64, pub alpha_out: u64, pub lifetime_volume: u128 }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum PriceSource { StorageMath, PerSubnetApi, AllSubnetsApi }
  pub fn storage_price(tao: Rao, alpha_in: u64) -> Option<Rao> { todo!() }
  pub struct Prices { pub state: BlockState, pub source: PriceSource, pub points: Vec<PricePoint> } // rows.rs
  // in bt-chain
  pub fn price_source(api_md: &Metadata) -> PriceSource { todo!() }
  impl<S: BlockSource> Chain<S> { pub async fn prices(&self, at: BlockRef) -> Result<Prices, ChainError> { todo!() } }
  ```

- **Who writes what.** You write it. Review checks:
  - `Prices` is the `prices` job's rows struct and lives in `crates/domain/src/rows.rs`, with `PriceSource` beside `PricePoint` in `bt-domain`, because `bt-store` names them ([D33](../decisions.md#d33-put-the-types-that-several-crates-name-in-bt-domain)).
  - The storage items decode with `Decodes::Storage`.
  - Netuids come from a `NetworksAdded` prefix read. Each key gives its netuid through `decode_storage_key` and then `decode_storage_key_values::<(u16,)>`. Netuid 0 is skipped.
  - When the executing spec does not declare the reserve items, the block gives no `PricePoint`, never a zero price ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)).
  - `storage_price` computes `SubnetTAO × 10^9 / SubnetAlphaIn` with `checked_mul` and `checked_div` in `u128`. A stored zero alpha reserve gives a price of 0, as the runtime does.
  - `Timestamp.Now` is one of the keys in the same `state_queryStorageAt` call.
  - `PriceSource` in this step only ever returns `StorageMath` at a pre-290 block. The other arms are `todo!()` until step 8.
- **Concepts.**
  - *Enums chosen from data.* `PriceSource` names the three price eras, and `price_source` picks one from what the metadata declares, so every `match` must handle each era. Trap: `if (spec >= 391) ... else if (spec >= 290)` chains spread across files. Source: The Rust Book, "Defining an Enum".
- **Check.** `cargo run -p bt-indexer -- chain prices --block 4920351` prints `spec=233 source=StorageMath subnets=64` and then one `netuid=<id> price=<rao> tao_reserve=<rao> alpha_reserve=<n>` line per subnet. The 64 subnets are the 65 `NetworksAdded` keys less root. `cargo test -p bt-domain price` passes, including `storage_price_zero_alpha_is_zero`.

## Step 8. `feat(chain): read prices from the runtime API from spec 290`

- **Goal.** Cover the other two price eras. Call `current_alpha_price(netuid)` once per subnet for specs 290 to 390, call `current_alpha_price_all` once per block from spec 391, and add `SubnetTaoProvided` and `SubnetAlphaInProvided` to the reserves when the executing spec declares them (specs 290 to 422). The API eras start at the upgrade blocks 5,947,548 (spec 290) and 7,782,857 (spec 391), and the provided items leave at 8,486,593 (spec 423).
- **Files.** `crates/chain/src/runtime_api.rs`, `crates/chain/src/jobs/prices.rs`.
- **Shape.**

  ```rust
  fn encode_call(md: &Metadata, at: BlockRef, api: &'static str, method: &'static str, args: impl IntoEncodableValues) -> Result<RuntimeCall, DecodeError> { todo!() }
  fn decode_call<T: IntoVisitor>(md: &Metadata, at: BlockRef, api: &'static str, method: &'static str, bytes: &[u8]) -> Result<T, DecodeError> { todo!() }
  async fn per_subnet_prices<S: BlockSource>(chain: &Chain<S>, at: BlockRef, md: &Metadata, netuids: &[NetUid]) -> Result<Vec<(NetUid, Rao)>, ChainError> { todo!() }
  async fn all_subnet_prices<S: BlockSource>(chain: &Chain<S>, at: BlockRef, md: &Metadata) -> Result<Vec<(NetUid, Rao)>, ChainError> { todo!() }
  ```

- **Who writes what.** You write it. Review checks:
  - `price_source` returns all three arms, read from the `Decodes::RuntimeApi` metadata, because the node runs the new code at an upgrade block ([D6](../decisions.md#d6-decode-storage-and-events-with-the-executing-spec-and-runtime-apis-with-the-blocks-own-spec)).
  - The call names follow subxt's `{trait}_{method}` form, such as `SwapRuntimeApi_current_alpha_price_all`.
  - A `DecodeError` from `encode_call` or `decode_call` carries `spec: at.spec.state`, the spec whose metadata the call used.
  - The provided items decode with `Decodes::Storage` and are read only when declared.
  - `current_alpha_price_all` returns root (netuid 0) at 1,000,000,000, and the read drops it.
  - One failed subnet call fails the whole block.
- **Concepts.**
  - *Collecting into `Result`.* `collect::<Result<Vec<_>, _>>()` turns per-subnet results into one result that stops at the first failure. Trap: `Promise.allSettled` habits that log a failed subnet and keep going, which leaves a block with missing prices and a checkpoint past it. Source: std docs, `impl FromIterator<Result<A, E>> for Result<V, E>`.
- **Check.** Run `cargo run -p bt-indexer -- chain prices --block <n>` at four blocks. At 4920351 it prints `spec=233 source=StorageMath`. At 6000000, which ran spec 292, it prints `spec=292 source=PerSubnetApi`. At 7782857, the upgrade block of spec 391, it prints `spec=391 source=AllSubnetsApi`. Without `--block`, at the head, it prints `spec=490 source=AllSubnetsApi`, while the head runs spec 490, as it did on 2026-11-12. The 7782857 line catches code that picks the era from the executing spec's metadata, which prints `PerSubnetApi` there and passes at the other three blocks.

## Stop here

- **Demo.** `bt-indexer chain spec --block 4920350` shows spec 219 against spec 233. `bt-indexer chain prices` prints prices at a spec 233 block, a spec 292 block, and the head, from three different sources.
- **Explain without notes.**
  - At an upgrade block, why do storage and events decode with the executing spec's metadata, while a runtime API call uses the block's own state spec?
  - Why does the metadata, and not the spec number, pick the era?
  - Why is `state_getMetadata` not enough to know whether a runtime API exists?
  - Why do the `BlockSource` methods return `impl Future + Send` instead of using `async fn`?
  - Why does `disposition` have no wildcard arm while `classify` has one?
- **Roadmap.** None yet. The read path that roadmap M0 depends on runs against the live node. Stage 3 closes M0 with offline fixtures.
