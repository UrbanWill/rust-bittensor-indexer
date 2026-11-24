//! The `bt-indexer` binary.

use bt_chain::{BlockSource, Chain, Decodes, Item, declares};
use bt_domain::{BlockNumber, BlockRef};
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
        block: Option<BlockNumber>,
    },
    /// Print the runtime API metadata's spec and whether it declares two items.
    Metadata {
        /// The archive node's WebSocket URL.
        #[arg(long, env = "BT_CHAIN__URLS")]
        url: String,
        /// The block number. Omit it to use the finalized head.
        #[arg(long)]
        block: Option<BlockNumber>,
    },
    /// Print a block's specs and its timestamp, with the spec whose metadata decoded it.
    Read {
        /// The archive node's WebSocket URL.
        #[arg(long, env = "BT_CHAIN__URLS")]
        url: String,
        /// The block number. Omit it to use the finalized head.
        #[arg(long)]
        block: Option<BlockNumber>,
    },
}

/// Reads block `block`, or the finalized head when `block` is `None`.
async fn block_at<S: BlockSource>(
    chain: &Chain<S>,
    block: Option<BlockNumber>,
) -> anyhow::Result<BlockRef> {
    let at = match block {
        Some(n) => chain.source().block_ref(n).await?,
        None => chain.source().finalized_head().await?,
    };

    Ok(at)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Chain {
            cmd: ChainCmd::Spec { url, block },
        } => {
            let chain = Chain::connect(&url).await?;
            let s = block_at(&chain, block).await?;
            println!(
                "block={} hash={:?} executed_by={} state={}",
                s.number, s.hash, s.spec.executed_by, s.spec.state
            );
        }
        Cmd::Chain {
            cmd: ChainCmd::Metadata { url, block },
        } => {
            let chain = Chain::connect(&url).await?;
            let at = block_at(&chain, block).await?;
            let (spec, md) = chain.metadata(at, Decodes::RuntimeApi).await?;
            let yn = |b: bool| if b { "yes" } else { "no" };
            println!(
                "spec={spec} SubtensorModule.SubnetTAO={} SwapRuntimeApi.current_alpha_price_all={}",
                yn(declares(&md, Item::Storage("SubtensorModule", "SubnetTAO"))),
                yn(declares(
                    &md,
                    Item::Api("SwapRuntimeApi", "current_alpha_price_all")
                )),
            );
        }
        Cmd::Chain {
            cmd: ChainCmd::Read { url, block },
        } => {
            let chain = Chain::connect(&url).await?;
            let at = block_at(&chain, block).await?;
            let (ts, spec) = chain.timestamp(at).await?;
            println!(
                "executed_by={} state={} decoded_with={spec} ts={ts}",
                at.spec.executed_by, at.spec.state
            )
        }
    }
    Ok(())
}
