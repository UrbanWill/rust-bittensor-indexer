//! Types that several crates of the indexer name.

#![deny(clippy::arithmetic_side_effects)]

mod block;

pub use block::{BlockHash, BlockNumber, BlockNumberError, BlockRef, SpecAt, SpecVersion};
