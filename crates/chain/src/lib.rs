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
        todo!();
    }

    /// Reads the specs at `block`, or at the finalized head when `block` is `None`.
    pub async fn spec_at(&self, block: Option<u64>) -> Result<SpecAt, ChainError> {
        todo!();
    }
}

/// Wraps any node error as `ChainError::Unknown` until step 6 classifies it.
fn unknown(e: impl Into<subxt::Error>) -> ChainError {
    todo!();
}
