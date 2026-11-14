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
    todo!();
}
