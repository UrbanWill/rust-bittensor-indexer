//! The rows each job reads from the chain.

use crate::{BlockRef, Timestamp};

/// The state every job records for a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockState {
    /// The block.
    pub at: BlockRef,
    /// The block's timestamp.
    pub ts: Timestamp,
}
