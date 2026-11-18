//! Decodes raw storage values with a metadata the caller picks.

use bt_domain::BlockRef;
use subxt::Metadata;
use subxt::ext::scale_decode::IntoVisitor;

use crate::DecodeError;

/// Decodes the value of `pallet.entry` at `at`.
///
/// Returns `None` when `md` does not declare the entry. A declared entry with
/// no stored value (`bytes` is `None`) decodes the default bytes from `md`.
pub(crate) fn decode_value<T: IntoVisitor>(
    md: &Metadata,
    at: BlockRef,
    pallet: &'static str,
    entry: &'static str,
    bytes: Option<&[u8]>,
) -> Result<Option<T>, DecodeError> {
    todo!()
}
