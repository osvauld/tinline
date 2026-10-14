//! W2-M: multi-host group model with host permits and host-initiated reconnect.
//!
//! Status: proposal — not approved. Simulation only; no production API, wire format or UI.
//! Decisions modelled: U4 revised (DID-signed content, random writer IDs), U5 revised (visible
//! block / removal notice), U7 (only hosts hold member group endpoints), U9 (removal by admin
//! flags), section 9 Q5 (multiple hosts; permits in the grant chain; host need not be admin;
//! a host is one specific device; admins can remove a host) and Q6 (after rotating, the host
//! itself reconnects members at their current endpoints; no host overlap window).
//!
//! Provenance: `SimRng`, `Post`/`Writer`/`Board`, `EpNote` are COPIED from `host_only.rs`
//! @dcdf4a4 (trimmed: no `FreezeWriter`, no chat request). `MGrant` is COPIED from
//! `authority.rs` @dcdf4a4 `Grant` and extended with a `device` field (the nominated hosting
//! device) and `Admin`/`Host` capabilities. Coordinator consolidates later. `Genesis` is
//! reused unchanged from `authority.rs`.
//!
//! Model (test-only):
//! - A host is a device holding a host *permit*: a real Ed25519-signed grant whose chain roots
//!   at the group creator and passes through admin grants (`Cap::Admin` + `Cap::Delegate`).
//!   The leaf carries `caps = {Host}`, `holder = host DID` and `device = Some(route slot)`.
//!   A permit with a device can never be a parent. Members accept a host's DID-signed group
//!   note only together with such a permit whose device equals the note's slot.
//! - Device IDs (route slots) are random (seeded RNG), never derived from a key. They are not
//!   cryptographic: any holder of the same mnemonic can claim another sibling's slot.
//! - Members know every host's group endpoint (Q5); each host knows members' group endpoints;
//!   hosts sync with each other (records, ops, directory notes, posts) "as if a VV exchange
//!   had completed" (same idealisation as `scheduler.rs` / `host_only.rs`).
//! - Group ops (admit, flag, remove host) are DID-signed by an admin and verified by every
//!   device; nothing is host fiat. Appointment order for the "admin appointed after the host"
//!   candidate uses grant nonces as a fixture stand-in for an ordered op log (not designed).
//! - Fake `u64` time, deterministic seeds, in-memory only. Open policy is parameterised in
//!   `Cfg`; no option is selected.
use crate::common::{
    A1, A2, BOB, MAX_CHAIN, OLGA, SimRng, digest, dir_key, ep, has, hex32, ident, pair, set,
    signed, valid,
};
use crate::legacy::*;
use identity::public_key_from_did;

// G6 group-call simulation; a child module so it reuses this file's private fixtures.
mod group_call;

mod fixtures;
mod ops_cfg;
mod world;
mod world_driver;

use fixtures::{
    Audience, Board, CAROL, Cap, EpNote, H1, H2, H3, HP, HostRecord, MEMBERS, MGrant,
    NOTICE_DOMAIN, OP_DOMAIN, Op, OpBody, Post, REMOVAL_FLAGS, Verdict, Why, Writer,
};
use ops_cfg::{
    BlockBody, BlockNotice, Cfg, ClaimRule, Device, Direct, Frame, HostCap, HostState, Path,
    Refuse, RemovalRule, Retention, RouteErr, SignedOp, View, World,
};
use world_driver::{everyone, world};

mod tests;
