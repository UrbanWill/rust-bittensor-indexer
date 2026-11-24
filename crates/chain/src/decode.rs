//! Decodes raw storage values with a metadata the caller picks.

use bt_domain::BlockRef;
use subxt::Metadata;
use subxt::ext::frame_decode::storage::{
    StorageTypeInfo, decode_default_storage_value_with_info, decode_storage_value_with_info,
};
use subxt::ext::scale_decode::IntoVisitor;

use crate::{DecodeError, DecodeKind, Item, declares};

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
    if !declares(md, Item::Storage(pallet, entry)) {
        return Ok(None);
    }

    let fail = |reason: Box<dyn std::error::Error + Send + Sync>| DecodeError {
        kind: DecodeKind::State,
        block: at.number,
        spec: at.spec.executed_by,
        item: entry,
        reason,
    };

    let info = md
        .storage_info(pallet, entry)
        .map_err(|e| fail(Box::new(e.into_owned())))?;

    match bytes {
        Some(mut bytes) => {
            decode_storage_value_with_info(&mut bytes, &info, md.types(), T::into_visitor())
                .map(Some)
                .map_err(|e| fail(Box::new(e)))
        }
        None => decode_default_storage_value_with_info(&info, md.types(), T::into_visitor())
            .map_err(|e| fail(Box::new(e))),
    }
}
