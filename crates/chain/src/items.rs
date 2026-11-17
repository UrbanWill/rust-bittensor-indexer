//! Answers whether a metadata declares a storage entry or a runtime API method.

use subxt::Metadata;

/// A storage entry or runtime API method that a metadata may declare.
#[derive(Clone, Copy, Debug)]
pub enum Item {
    /// A storage entry, as pallet name and entry name.
    Storage(&'static str, &'static str),
    /// A runtime API method, as trait name and method name.
    Api(&'static str, &'static str),
}

/// Reports whether `md` declares `item`.
pub fn declares(md: &Metadata, item: Item) -> bool {
    match item {
        Item::Storage(pallet, entry) => md
            .pallet_by_name(pallet)
            .and_then(|p| p.storage())
            .and_then(|s| s.entry_by_name(entry))
            .is_some(),
        Item::Api(api, method) => md
            .runtime_api_trait_by_name(api)
            .and_then(|t| t.method_by_name(method))
            .is_some(),
    }
}
