//! Errors from decoding chain data.

use bt_domain::{BlockNumber, SpecVersion};

/// A value or event that did not decode with the metadata picked for it.
#[derive(Debug, thiserror::Error)]
#[error("decode {item} at block {block} (spec {spec})")]
pub struct DecodeError {
    /// What was being decoded.
    pub kind: DecodeKind,
    /// The block the bytes came from.
    pub block: BlockNumber,
    /// The spec whose metadata the decode used.
    pub spec: SpecVersion,
    /// The storage entry or runtime API method.
    pub item: &'static str,
    /// The underlying decode failure.
    #[source]
    pub reason: Box<dyn std::error::Error + Send + Sync>,
}

/// The kind of data a `DecodeError` was decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeKind {
    /// A storage value or runtime API result.
    State,
    /// An event. Unused until release 2.
    Event,
}
