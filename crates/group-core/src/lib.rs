//! The group chat model (G1-G3, `docs/group-core.md`) with no IO runtime, transport or UniFFI:
//! DID-signed records, the per-group Loro docs and their acceptance rules, the state derived
//! from them, day shards, the sealed group vault and its rows, sealed member mail, the endpoint
//! rotation policy, invite code links, stored invitation/request/call-log rows and host storage
//! attestations. p2pcore re-exports these modules under `p2pcore::group` and owns the engine,
//! wire sessions, calls and app API.

pub mod attestation;
pub mod calllog;
pub mod code;
pub mod doc;
mod error;
pub mod files;
pub mod invitations;
pub mod mail;
pub mod record;
pub mod requests;
pub mod rotation;
pub mod shard;
pub mod state;
pub mod store;

pub use error::Error;

/// A group id: 64 lowercase hex characters (checked wherever one arrives from a peer or a link).
pub fn valid_gid(gid: &str) -> bool {
    gid.len() == 64 && gid.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The first 8 characters of an id, for logs; never panics on odd input. The one shortener for
/// every identifier a log line carries (group, call, device and endpoint ids, DIDs). A
/// `did:key:z6Mk...` DID is cut after that constant prefix, so its 8 characters tell DIDs apart.
pub fn short(id: &str) -> &str {
    let id = id.strip_prefix("did:key:z6Mk").unwrap_or(id);
    id.get(..8).unwrap_or(id)
}

#[cfg(test)]
mod tests;
