//! Several devices of one account: linking (`tinline/link/1`) and own-device sync
//! (`tinline/self/3`). See docs/design/device-linking.md and docs/protocol.md.

// The own-device docs and their merge rules are `account_model::sync`.
pub(crate) use account_model::sync::{accdoc, callsdoc, profilecache, profiledoc};
/// The own-device doc wrapper lives in the `crdt` crate.
pub(crate) mod doc {
    pub(crate) use crdt::doc::Doc;
}
pub(crate) mod api;
pub(crate) mod engine;
pub(crate) mod frames;
pub(crate) mod link;

pub use api::{LinkEvents, LinkedDevice};
