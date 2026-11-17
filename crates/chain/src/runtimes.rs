//! Fetches and caches runtime metadata per spec.

use crate::{Chain, ChainError, unknown};
use bt_domain::{BlockHash, BlockRef, SpecVersion};
use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};
use subxt::config::substrate::H256;
use subxt::ext::codec::{Decode, Encode};
use subxt::{ArcMetadata, Metadata};

/// Metadata fetched so far, one entry per spec. Clones share the same cache.
#[derive(Clone, Default)]
pub struct Runtimes {
    /// Each spec's metadata, fetched once.
    metadata: Arc<RwLock<HashMap<SpecVersion, ArcMetadata>>>,
}

impl Runtimes {
    fn get(&self, spec: SpecVersion) -> Option<ArcMetadata> {
        let cache = self.metadata.read().unwrap_or_else(PoisonError::into_inner);
        cache.get(&spec).cloned()
    }

    fn insert(&self, spec: SpecVersion, md: ArcMetadata) {
        let mut cache = self
            .metadata
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        cache.insert(spec, md);
    }
}

/// The kind of read the metadata is for, which picks the spec (D6).
pub enum Decodes {
    /// Storage and events. Uses the spec that executed the block, fetched at block n−1.
    Storage,
    /// Runtime API calls. Uses the spec in the block's own state, fetched at block n.
    RuntimeApi,
}

impl Chain {
    /// Returns the metadata that a read of kind `of` at `at` decodes with, and the spec it belongs to.
    pub async fn metadata(
        &self,
        at: BlockRef,
        of: Decodes,
    ) -> Result<(SpecVersion, ArcMetadata), ChainError> {
        let spec = match of {
            Decodes::Storage => at.spec.executed_by,
            Decodes::RuntimeApi => at.spec.state,
        };

        if let Some(md) = self.runtimes.get(spec) {
            return Ok((spec, md));
        }

        let fetch_at = match of {
            Decodes::Storage => {
                let parent = at
                    .number
                    .prev()
                    .ok_or_else(|| unknown(subxt::Error::other_str("block 0 has no parent")))?;
                self.spec_at(Some(parent)).await?.hash
            }
            Decodes::RuntimeApi => at.hash,
        };

        let md = self.fetch_metadata(fetch_at).await?;
        self.runtimes.insert(spec, md.clone());
        Ok((spec, md))
    }

    async fn fetch_metadata(&self, hash: BlockHash) -> Result<ArcMetadata, ChainError> {
        let at = Some(H256(hash.0));

        let bytes = self
            .rpc
            .state_call("Metadata_metadata_versions", None, at)
            .await
            .map_err(unknown)?;
        let versions = Vec::<u32>::decode(&mut bytes.as_slice()).map_err(unknown)?;
        let version = versions
            .into_iter()
            .filter(|&v| v != u32::MAX)
            .max()
            .ok_or_else(|| unknown(subxt::Error::other_str("no stable metadata version")))?;

        let bytes = self
            .rpc
            .state_call("Metadata_metadata_at_version", Some(&version.encode()), at)
            .await
            .map_err(unknown)?;
        let opaque = Option::<Vec<u8>>::decode(&mut bytes.as_slice())
            .map_err(unknown)?
            .ok_or_else(|| unknown(subxt::Error::other_str("metadata version not served")))?;
        let md = Metadata::decode(&mut opaque.as_slice()).map_err(unknown)?;
        Ok(md.arc())
    }
}
