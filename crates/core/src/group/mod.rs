//! Group chat (G1): DID-signed records in a Loro doc, carried over a second iroh endpoint (the
//! group endpoint, U2) between members and hosts only (U7). See `docs/group-core.md`.

pub mod api;
pub mod calldir;
pub mod calls;
pub mod doc;
pub mod code;
pub mod engine;
pub mod invitations;
pub mod mail;
pub mod own;
pub mod record;
pub mod requests;
pub mod state;
pub mod store;
pub mod wire;
