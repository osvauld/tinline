//! W2-F: group removal by admin flags (U9) and group-level rotation on host revocation
//! (`docs/design/rotation-open-decisions.md` section 10 Q8, decided).
//!
//! Status: proposal — not approved. Simulation only; no production API, wire format or UI.
//! Decisions modelled: U4 revised (DID-signed content, random writer IDs), U5 revised (the
//! removed member is told with a verifiable signed notice), U7 + section 9 Q5 (multiple hosts;
//! only hosts hold member group endpoints), U9 (admins flag; at 3 flags the member is removed
//! and its grant revoked; flags/removal are DID-signed group ops every device verifies), and
//! section 10 Q8 (a removed host triggers a group-level rotation of every member's and every
//! remaining host's group endpoint; notes go only to permitted hosts).
//!
//! The U9 details in section 8 are NOT decided. Each is a `Cfg` parameter (G-AUTHORITY,
//! G-TIME) and the post rule around the 3rd flag reuses the D01/D02 candidates (signed
//! frontier vs removal wins) from `policy_candidates.rs`. `Cfg::default()` values are fixture
//! defaults chosen so other tests have *some* value; they are not selections.
//!
//! Provenance: `SimRng`, `MGrant` (+ `Cap`), `Post`/`Writer`/`Board`, `EpNote`, `HostRecord`,
//! `SignedOp` and the `World` link/host-sync/rotation machinery are COPIED from
//! `multi_host.rs` @0924939 (itself from `host_only.rs`/`authority.rs`) and changed as follows:
//! - `Board` has no roster: membership is decided by the policy layer (`Replica`), which keeps
//!   a store of policy-accepted posts and rebuilds the real Loro board when a rule drops posts.
//! - Posts carry `ops_seen` (the author's signed op frontier, as in the D01/D02 `Header`).
//! - Ops carry `seen` (the issuer's op frontier); every op is authorized at its own frontier,
//!   and ops whose frontier is missing are held pending. Membership is DERIVED from the op set.
//! - `Op` gains `Withdraw`, `Admit { lifts }` (rejoin) and flag `at`/`seen_posts`.
//! - A host that accepts a `RemoveHost` rotates and itself pushes (full exchange) to members.
//! - No rogue fixture, writer-claim rule, retention, host cap, chat request or block notice.
//!
//! Fake `u64` time, deterministic seeds, in-memory only.
use crate::common::{
    A1, A2, BOB, MAX_CHAIN, OLGA, SimRng, digest, dir_key, ep, has, ident, pair, permutations, set,
    signed, valid,
};
use crate::legacy::*;
use identity::public_key_from_did;

mod fixtures;
mod ops_rules;
mod replica;
mod world;
mod world_driver;

use fixtures::{
    Board, CAROL, Cap, DAVE, ERIN, EpNote, FLAG_THRESHOLD, H1, H2, H3, HostRecord, MAX_PENDING,
    MEMBERS, MGrant, OP_DOMAIN, Op, OpBody, Post, Verdict, Why, Writer,
};
use ops_rules::{
    Acc, AdminRule, Cfg, Ctx, Derived, Expiry, FlagCount, OwnerRule, PostRule, Refuse, Rejoin,
    SignedOp, SmallGroup, Withdrawal, closure,
};
use replica::{Device, Frame, Path, Replica, RouteErr, World};
use world_driver::{Ev, everyone, explore, ids, with, world};

mod tests;
