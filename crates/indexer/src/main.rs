//! The `bt-indexer` binary.

use bt_chain::{Chain, Decodes, Item, declares};
use bt_domain::BlockNumber;
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Chain {
            cmd: ChainCmd::Spec { url, block },
        } => {
            let chain = Chain::connect(&url).await?;
            let s = chain.spec_at(block).await?;
            println!(
                "block={} hash={:?} executed_by={} state={}",
                s.number, s.hash, s.spec.executed_by, s.spec.state
            );
        }
        Cmd::Chain {
            cmd: ChainCmd::Metadata { url, block },
        } => {
            let chain = Chain::connect(&url).await?;
            let at = chain.spec_at(block).await?;
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
    }
    Ok(())
}
