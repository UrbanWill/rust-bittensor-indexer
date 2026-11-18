//! Reads Bittensor chain data from an archive node.

use bt_domain::{BlockRef, BlockState, SpecVersion, Timestamp};
use subxt::ext::frame_decode::storage::encode_storage_key;

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
        Ok(Self::new(SubxtSource::connect(url).await?))
    }
}

impl<S: BlockSource> Chain<S> {
    /// Reads through `source`, with an empty metadata cache.
    pub fn new(source: S) -> Self {
        Self {
            source,
            runtimes: Runtimes::default(),
        }
    }

    /// Returns the source.
    pub fn source(&self) -> &S {
        &self.source
    }

    /// Reads `Timestamp.Now` at `at`, with the spec whose metadata decoded it.
    pub async fn timestamp(&self, at: BlockRef) -> Result<(Timestamp, SpecVersion), ChainError> {
        let (spec, md) = self.metadata(at, Decodes::Storage).await?;
        let fail = |reason: Box<dyn std::error::Error + Send + Sync>| DecodeError {
            kind: DecodeKind::State,
            block: at.number,
            spec,
            item: "Now",
            reason,
        };

        let key = encode_storage_key("Timestamp", "Now", (), &*md, md.types())
            .map_err(|e| fail(Box::new(e)))?;
        let page = self
            .source
            .storage(at, StorageQuery::Keys(vec![key]))
            .await?;

        let bytes = page.entries.first().and_then(|(_, v)| v.as_deref());

        let ms: u64 = decode::decode_value(&md, at, "Timestamp", "Now", bytes)?
            .ok_or_else(|| fail("Timestamp.Now is not declared".into()))?;
        let ms = i64::try_from(ms).map_err(|e| fail(Box::new(e)))?;
        let ts = chrono::DateTime::from_timestamp_millis(ms)
            .ok_or_else(|| fail("Timestamp out of range".into()))?;
        Ok((Timestamp(ts), spec))
    }

    /// Reads the state every job records for `at`.
    pub async fn block_state(&self, at: BlockRef) -> Result<BlockState, ChainError> {
        let (ts, _) = self.timestamp(at).await?;
        Ok(BlockState { at, ts })
    }
}

/// Wraps any node error as `ChainError::Unknown` until step 6 classifies it.
fn unknown(e: impl Into<subxt::Error>) -> ChainError {
    ChainError::Unknown(Box::new(e.into()))
}
