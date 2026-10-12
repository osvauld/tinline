//! Several devices of one account: linking (`tinline/link/1`) and own-device sync
//! (`tinline/self/3`). See docs/design/device-linking.md and docs/protocol.md.

pub(crate) mod accdoc;
pub(crate) mod callsdoc;
pub(crate) mod profiledoc;
pub(crate) mod profilecache;
#[cfg(test)]
mod profiledoc_tests;
pub(crate) mod doc;
pub(crate) mod api;
pub(crate) mod engine;
pub(crate) mod frames;
pub(crate) mod link;

pub use api::{LinkEvents, LinkedDevice};
