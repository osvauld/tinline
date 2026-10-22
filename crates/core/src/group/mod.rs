//! Group chat (G1): DID-signed records in a Loro doc, carried over a second iroh endpoint (the
//! group endpoint, U2) between members and hosts only (U7). See `docs/group-core.md`.
//!
//! The model (records, docs, derived state, day shards, the group vault, sealed mail, rotation
//! policy, code links, stored invitation/request rows) is the `group-core` crate, re-exported
//! here so `crate::group::record::...` paths keep working. This module keeps what needs the
//! node: the engine, the wire sessions, calls and the UniFFI surface.

pub use group_core::{doc, mail, record, shard, state};

mod access;
pub mod api;
pub(crate) mod attestation;
pub mod calldir;
pub mod calllog;
pub mod calls;
pub mod code;
pub mod engine;
pub mod images;
pub mod invitations;
pub(crate) mod member_receipts;
pub mod own;
mod relay;
pub mod requests;
pub mod rotation;
pub mod store;
pub mod wire;

pub(crate) use group_core::{short, valid_gid};
