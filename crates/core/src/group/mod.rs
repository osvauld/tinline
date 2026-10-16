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
pub mod rotation;
pub mod shard;
pub mod state;
pub mod store;
pub mod wire;

/// A group id: 64 lowercase hex characters (checked wherever one arrives from a peer or a link).
pub(crate) fn valid_gid(gid: &str) -> bool {
    gid.len() == 64 && gid.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The first 8 characters of an id, for logs; never panics on odd input.
pub(crate) fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}
