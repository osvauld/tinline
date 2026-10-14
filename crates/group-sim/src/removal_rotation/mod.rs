//! W2-U: announced, delayed host rotation after a member removal (U11, U12).
//!
//! Status: proposal — not approved. Simulation only; no production API, wire format or UI.
//! Decisions modelled (`docs/design/rotation-open-decisions.md` section 0):
//! - U4 revised: content is signed by the DID key; Loro peer IDs are random writer IDs.
//! - U5 revised: the removed member is told (signed flags returned on her dial).
//! - U7: only hosts hold members' group endpoints; all group traffic goes via hosts.
//! - U9 / U10: 3 flags from distinct admins remove a member; every device verifies every
//!   message, op, host record, announcement and call request itself.
//! - U11: signed frontier (D01/D02 option A): a post is kept if its signed op frontier
//!   predates the removal. Hosts accept a removed member's upload of such posts.
//! - U12 + detail: after a removal every host signs a "rotating soon" announcement, sends it
//!   to REMAINING members only, and rotates its group endpoint `ROTATION_DELAY` (2 days)
//!   later, reconnecting members itself. Refusal of the removed DID's NEW posts and calls is
//!   immediate.
//!
//! NOT decided, modelled as `Cfg` parameters (defaults are fixture values, not selections):
//! - `UploadCap` (G-HISTORY): count cap on pre-removal uploads vs the rotation as the only
//!   cut-off.
//! - `Batching` (G-TIME): several removals inside one delay window.
//! - `MemberRotation` (G-DISCOVERY): do members also rotate their own group endpoints?
//!
//! Provenance: helpers, `SimRng`, `MGrant`/`Cap`, `Post`/`Writer`/`Board`, `EpNote`,
//! `HostRecord`, `SignedOp`, closure/frontier checks and the `World` link/host-sync/rotation
//! machinery are COPIED from `flags.rs` @82d221f (itself from `multi_host.rs` @0924939) and
//! trimmed: only `Op::Flag` (distinct-admin threshold, owner and admins not flaggable), only
//! the signed-frontier post rule, no host revocation, no withdrawal/expiry/rejoin. Added: the
//! removal-rotation schedule (`Sched`), the signed `Announce`, DID-signed `CallReq` relayed
//! by a host, the removed member's pre-removal upload on her dial, and the cap candidate.
//!
//! Fake `u64` time (1 tick = 1 simulated second), deterministic seeds, in-memory only.
use super::*;
use identity::{public_key_from_did, verify};

mod fixtures;
mod replica;
mod world;
mod world_driver;

use fixtures::{ANNOUNCE_DOMAIN, CALL_DOMAIN, MAX_PENDING, FLAG_THRESHOLD, TICKS_PER_DAY, TICKS_PER_HOUR, ROTATION_DELAY, T0, OLGA, A1, BOB, DAVE, CAROL, ERIN, H1, H2, H3, MEMBERS, has, ident, ep, signed, valid, digest, dir_key, pair, SimRng, Cap, MGrant, Post, Writer, Why, Verdict, Board, EpNote, HostRecord, Op, SignedOp};
use replica::{AnnBody, Announce, CallBody, CallReq, Refuse, Acc, UploadCap, Batching, MemberRotation, Cfg, Ctx, closure, Replica, Sched, Device};
use world::{RouteErr, Path, World};
use world_driver::{world, with, remaining, with_hosts, remove, note_seq};

mod tests;
