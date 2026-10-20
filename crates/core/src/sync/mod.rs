//! Several devices of one account: linking (`tinline/link/1`) and own-device sync
//! (`tinline/self/3`). See docs/design/device-linking.md and docs/protocol.md.

pub(crate) mod accdoc;
#[cfg(test)]
mod accdoc_tests;
pub(crate) mod callsdoc;
pub(crate) mod profiledoc;
pub(crate) mod profilecache;
#[cfg(test)]
mod profiledoc_tests;
/// The own-device doc wrapper lives in the `crdt` crate; `io` maps any error into ours.
pub(crate) mod doc {
    pub(crate) use crdt::doc::{Doc, entries, field, text};

    pub(crate) fn io<E: std::fmt::Display>(e: E) -> crate::Error {
        crate::Error::Io(e.to_string())
    }
}
pub(crate) mod api;
pub(crate) mod engine;
pub(crate) mod frames;
pub(crate) mod link;

pub use api::{LinkEvents, LinkedDevice};
