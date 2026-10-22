//! Group chat (G1): DID-signed records in a Loro doc, carried over a second iroh endpoint (the
//! group endpoint, U2) between members and hosts only (U7). See `docs/group-core.md`.
//!
//! The model is the `group-core` crate and the runtime (engine, sessions, invitations, codes,
//! requests, presence, images, member receipts, calls) is the `group-runtime` crate; both are
//! re-exported here so `crate::group::...` paths keep working. This module keeps the UniFFI
//! surface: the records declared as remote types, the app's event callbacks and the `Node` calls.

pub use group_runtime::{attestation, doc, engine, images, mail, own, record, state, store};

pub mod api;
pub mod calllog;
pub mod calls;
pub mod code;
pub mod invitations;
pub(crate) mod member_receipts;
pub mod requests;
pub mod rotation;

#[cfg(test)]
mod own_auth_tests;
