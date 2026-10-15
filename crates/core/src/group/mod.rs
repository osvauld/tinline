//! Group chat (G1): DID-signed records in a Loro doc, carried over a second iroh endpoint (the
//! group endpoint, U2) between members and hosts only (U7). See `docs/group-core.md`.

mod access;
pub mod api;
// Prototype crypto/persisted-state adapter; transport integration follows separately.
#[allow(dead_code)]
pub(crate) mod attestation;
pub mod calldir;
pub mod calllog;
pub mod calls;
pub mod doc;
pub mod code;
pub mod engine;
pub mod invitations;
pub mod mail;
pub mod own;
pub mod record;
mod relay;
pub mod requests;
pub mod shard;
pub mod state;
pub mod store;
pub mod wire;
