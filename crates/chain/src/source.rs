//! Where `Chain` gets raw block data: a live node, or recorded bytes in tests.

use bt_domain::{BlockNumber, BlockRef};
use std::future::Future;
use subxt::config::{RpcConfigFor, SubstrateConfig};
use subxt::rpcs::{LegacyRpcMethods, RpcClient};

use crate::ChainError;

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
    client: RpcClient,
    rpc: LegacyRpcMethods<RpcConfigFor<SubstrateConfig>>,
}

impl SubxtSource {
    /// Connects to a node over a secure WebSocket.
    pub async fn connect(url: &str) -> Result<Self, ChainError> {
        todo!()
    }
}

impl BlockSource for SubxtSource {
    async fn finalized_head(&self) -> Result<BlockRef, ChainError> {
        todo!()
    }

    async fn block_ref(&self, n: BlockNumber) -> Result<BlockRef, ChainError> {
        todo!()
    }

    async fn storage(&self, at: BlockRef, q: StorageQuery) -> Result<StoragePage, ChainError> {
        todo!()
    }

    async fn runtime_api(&self, at: BlockRef, call: RuntimeCall) -> Result<Vec<u8>, ChainError> {
        todo!()
    }
}
