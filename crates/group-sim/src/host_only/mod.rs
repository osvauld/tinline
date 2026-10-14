//! W2-H: host-only group model with DID-signed content (user decisions U4 revised, U7).
//!
//! Status: proposal — not approved. Simulation only; no production API, wire format or UI.
//! Parallel model: it does NOT use the `Envelope`/`Replica`/`Author` fixtures in `tests.rs`,
//! which encode the superseded device-key-signed, attestation-carrying, mesh-sync design.
//!
//! Model (test-only):
//! - Content (`Post`) is signed by the member's DID key (`Identity::sign`, real Ed25519) and
//!   carries a random per-device *writer ID* used as the Loro peer ID and per-device sequence
//!   space. Writer IDs come from a seeded SplitMix64 stream (`SimRng`), never from any key.
//!   No device key, transport key, attestation or endpoint is a field of any post.
//! - Topology (U7): only the host holds members' group endpoints (`World::directory`, keyed
//!   by DID + a random per-device route slot). Members hold only the host's DID-signed group
//!   note and their private address book (private notes / accepted chat requests). There is
//!   no field in which a member could store another member's group endpoint.
//! - All group traffic goes member -> host -> members. Host push selects missing posts "as if
//!   a VV exchange had completed" (same idealisation as `scheduler.rs`); every receiver still
//!   verifies every post's DID signature itself.
//! - Endpoints are fresh random fixture keys (`grp-<hex>`, `prv-<hex>`); rotation draws a new
//!   one. Nothing is derived from a previous endpoint (no future-endpoint scheme).
//! - Fake `u64` time, deterministic seeds, in-memory only.
//! - Open policy (writer-ID collisions G-FORK, offline queue bound G-STORAGE, host endpoint
//!   overlap G-TIME) is modelled as parameters; no option is selected.
use crate::common::{SimRng, dir_key, has, hex32, ident};
use crate::legacy::*;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use identity::{public_key_from_did, verify};
use x25519_dalek::{PublicKey, StaticSecret};

mod chat_and_net;
mod fixtures;
mod world;
mod world_driver;

use chat_and_net::{
    Cfg, ChatIn, Device, Frame, Hello, HostOverlap, KeyKind, Path, QueueBound, RouteErr, World,
    open, seal,
};
use fixtures::{
    A1, A2, Audience, BOB, Board, CAROL, CHAT_DOMAIN, ChatKind, EpNote, ForkPolicy, GROUP, HOST,
    Header, Inner, NoteReject, Post, ReqKeyClaim, Sealed, SignedInner, SignedReqKey, Verdict, Why,
    Writer, aead_key, did_sign, did_verify, ep, leaks_key, tkey, unpad,
};
use world_driver::{ALL, world};

mod tests;
