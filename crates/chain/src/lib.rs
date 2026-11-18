//! Reads Bittensor chain data from an archive node.

use bt_domain::{BlockRef, BlockState, SpecVersion, Timestamp};

mod decode;
mod error;
mod items;
mod runtimes;
mod source;
pub use error::{DecodeError, DecodeKind};
pub use items::{Item, declares};
pub use runtimes::{Decodes, Runtimes};
pub use source::{BlockSource, RuntimeCall, StoragePage, StorageQuery, SubxtSource};

/// Reads and decodes chain data from a `BlockSource`.
pub struct Chain<S: BlockSource> {
    source: S,
    runtimes: Runtimes,
}

/// Errors from the node.
#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    /// A value did not decode with the metadata picked for it.
    #[error(transparent)]
    Decode(#[from] DecodeError),
    /// Any node error. Step 6 splits it into classified variants.
    #[error("node error")]
    Unknown(#[source] Box<subxt::Error>),
}

impl Chain<SubxtSource> {
    /// Connects to a node over a secure WebSocket.
    pub async fn connect(url: &str) -> Result<Self, ChainError> {
        todo!()
    }
}

impl<S: BlockSource> Chain<S> {
    /// Reads through `source`, with an empty metadata cache.
    pub fn new(source: S) -> Self {
        todo!()
    }

    /// Returns the source.
    pub fn source(&self) -> &S {
        todo!()
    }

    /// Reads `Timestamp.Now` at `at`, with the spec whose metadata decoded it.
    pub async fn timestamp(&self, at: BlockRef) -> Result<(Timestamp, SpecVersion), ChainError> {
        todo!()
    }

    /// Reads the state every job records for `at`.
    pub async fn block_state(&self, at: BlockRef) -> Result<BlockState, ChainError> {
        todo!()
    }
}

/// Wraps any node error as `ChainError::Unknown` until step 6 classifies it.
fn unknown(e: impl Into<subxt::Error>) -> ChainError {
    ChainError::Unknown(Box::new(e.into()))
}
