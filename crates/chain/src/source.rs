//! Where `Chain` gets raw block data: a live node, or recorded bytes in tests.

use bt_domain::{BlockHash, BlockNumber, BlockRef, SpecAt, SpecVersion};
use std::collections::HashMap;
use std::future::Future;
use subxt::config::substrate::H256;
use subxt::config::{Header, RpcConfigFor, SubstrateConfig};
use subxt::rpcs::{LegacyRpcMethods, RpcClient};

use crate::{ChainError, unknown};

/// Serves raw block data. `Chain` decodes what it returns.
pub trait BlockSource: Send + Sync + 'static {
    /// Returns the finalized head with both of its specs.
    fn finalized_head(&self) -> impl Future<Output = Result<BlockRef, ChainError>> + Send;

    /// Returns block `n` with both of its specs.
    fn block_ref(
        &self,
        n: BlockNumber,
    ) -> impl Future<Output = Result<BlockRef, ChainError>> + Send;

    /// Reads raw storage at `at`.
    fn storage(
        &self,
        at: BlockRef,
        q: StorageQuery,
    ) -> impl Future<Output = Result<StoragePage, ChainError>> + Send;

    /// Calls a runtime API at `at` and returns the raw SCALE result.
    fn runtime_api(
        &self,
        at: BlockRef,
        call: RuntimeCall,
    ) -> impl Future<Output = Result<Vec<u8>, ChainError>> + Send;
}

/// The storage keys to read.
#[derive(Clone, Debug)]
pub enum StorageQuery {
    /// These exact keys.
    Keys(Vec<Vec<u8>>),
    /// One page of the keys under `prefix`, after `start`.
    Prefix {
        /// The key prefix.
        prefix: Vec<u8>,
        /// The last key of the previous page, or `None` for the first page.
        start: Option<Vec<u8>>,
        /// The page size.
        page: u32,
    },
}

/// Storage keys with their raw values.
#[derive(Clone, Debug, Default)]
pub struct StoragePage {
    /// Each key with its value. `None` means the node holds no value for the key.
    pub entries: Vec<(Vec<u8>, Option<Vec<u8>>)>,
}

/// A runtime API call, as subxt's `{trait}_{method}` name and SCALE-encoded arguments.
#[derive(Clone, Debug)]
pub struct RuntimeCall {
    /// The call name, such as `Metadata_metadata_at_version`.
    pub name: String,
    /// The SCALE-encoded arguments.
    pub args: Vec<u8>,
}

/// A `BlockSource` backed by one archive node.
pub struct SubxtSource {
    #[allow(dead_code)] // step 13 sends a raw request through it
    client: RpcClient,
    rpc: LegacyRpcMethods<RpcConfigFor<SubstrateConfig>>,
}

impl SubxtSource {
    /// Connects to a node over a secure WebSocket.
    pub async fn connect(url: &str) -> Result<Self, ChainError> {
        let client = RpcClient::from_url(url).await.map_err(unknown)?;
        Ok(Self {
            rpc: LegacyRpcMethods::new(client.clone()),
            client,
        })
    }
    /// Reads the block at `hash` with both of its specs.
    async fn block_at_hash(&self, hash: H256) -> Result<BlockRef, ChainError> {
        let header = self
            .rpc
            .chain_get_header(Some(hash))
            .await
            .map_err(unknown)?
            .ok_or_else(|| unknown(subxt::Error::other_str("header not found")))?;
        let number = BlockNumber::try_from(header.number())
            .map_err(|_| unknown(subxt::Error::other_str("block number does not fit in u32")))?;

        let state = self
            .rpc
            .state_get_runtime_version(Some(hash))
            .await
            .map_err(unknown)?
            .spec_version;

        let executed_by = self
            .rpc
            .state_get_runtime_version(Some(header.parent_hash))
            .await
            .map_err(unknown)?
            .spec_version;

        Ok(BlockRef {
            number,
            hash: BlockHash(hash.0),
            spec: SpecAt {
                executed_by: SpecVersion::new(executed_by),
                state: SpecVersion::new(state),
            },
        })
    }
}

impl BlockSource for SubxtSource {
    async fn finalized_head(&self) -> Result<BlockRef, ChainError> {
        let hash = self.rpc.chain_get_finalized_head().await.map_err(unknown)?;

        self.block_at_hash(hash).await
    }

    async fn block_ref(&self, n: BlockNumber) -> Result<BlockRef, ChainError> {
        let hash = self
            .rpc
            .chain_get_block_hash(Some(u64::from(n).into()))
            .await
            .map_err(unknown)?
            .ok_or_else(|| unknown(subxt::Error::other_str("block not found")))?;

        self.block_at_hash(hash).await
    }

    async fn storage(&self, at: BlockRef, q: StorageQuery) -> Result<StoragePage, ChainError> {
        let hash = H256(at.hash.0);
        let keys = match q {
            StorageQuery::Keys(keys) => keys,
            StorageQuery::Prefix {
                prefix,
                start,
                page,
            } => self
                .rpc
                .state_get_keys_paged(&prefix, page, start.as_deref(), Some(hash))
                .await
                .map_err(unknown)?,
        };
        if keys.is_empty() {
            return Ok(StoragePage::default());
        }

        let sets = self
            .rpc
            .state_query_storage_at(keys.iter().map(Vec::as_slice), Some(hash))
            .await
            .map_err(unknown)?;

        let mut values: HashMap<Vec<u8>, Option<Vec<u8>>> = sets
            .into_iter()
            .flat_map(|set| set.changes)
            .map(|(k, v)| (k.0, v.map(|v| v.0)))
            .collect();

        let entries = keys
            .into_iter()
            .map(|k| {
                let v = values.remove(&k).flatten();
                (k, v)
            })
            .collect();
        Ok(StoragePage { entries })
    }

    async fn runtime_api(&self, at: BlockRef, call: RuntimeCall) -> Result<Vec<u8>, ChainError> {
        self.rpc
            .state_call(&call.name, Some(&call.args), Some(H256(at.hash.0)))
            .await
            .map_err(unknown)
    }
}
