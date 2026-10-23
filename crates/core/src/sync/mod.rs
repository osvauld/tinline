//! Several devices of one account: linking (`tinline/link/1`) and own-device sync
//! (`tinline/self/3`). See docs/design/device-linking.md and docs/protocol.md.
//!
//! The runtime is the `sync-runtime` crate (`SyncRt`). This module keeps the UniFFI surface:
//! `LinkEvents`, `LinkedDevice` and the `Node` calls, plus (in `engine`) the own-device tests
//! that run whole nodes.

pub(crate) mod api;
#[cfg(test)]
mod engine;

pub use api::{LinkEvents, LinkedDevice};
