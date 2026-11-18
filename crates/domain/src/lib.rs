//! Types that several crates of the indexer name.

#![deny(clippy::arithmetic_side_effects)]

mod block;
mod rows;
mod time;

pub use block::{BlockHash, BlockNumber, BlockNumberError, BlockRef, SpecAt, SpecVersion};
pub use rows::BlockState;
pub use time::Timestamp;
