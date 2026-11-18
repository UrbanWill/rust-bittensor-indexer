//! Block timestamps.

use chrono::{DateTime, Utc};
use std::fmt;

/// The time a block was produced, from `Timestamp.Now`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub DateTime<Utc>);

impl fmt::Display for Timestamp {
    /// Prints RFC 3339 to the second, such as `2025-02-13T21:40:36Z`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}
