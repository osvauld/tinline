//! W2-R: rotation notes relayed by the group host (user decisions U1, U2).
//!
//! Simulation only. A `RotationNote` is a stand-in for a succession permit: the owner DID signs
//! (owner, audience, endpoint, seq) with its real Ed25519 DID key. It is NOT the succession
//! verifier (G-D), carries no device path, and no future endpoint is derived from anything:
//! every endpoint is a fresh fixture string. Time is not modelled (no expiry/grace, G-TIME).
//! Membership is a fixed roster fixture; group authorization stays in `authority.rs`.
//! Endpoint strings: group endpoints start with `grp-`, private endpoints with `PRIVATE-`, so
//! audience leaks can be found by scanning bytes.
//!
//! R1b (U7: only the host knows members' group endpoints): a member's `GroupView` accepts only
//! its own and the host's notes (`Reject::NotHost` otherwise); the host relays its own note
//! to members but never a member's note; a returning member retries its own notes to the host
//! and pulls the host's; recovery after a host rotation is the host's retry (`host_push`), not
//! member forwarding. Removed by R1b, with their current-design coverage:
//! - `member_rotation_is_relayed_to_online_members_and_offline_member_catches_up_from_host`,
//!   `two_members_rotated_while_apart_recover_each_other_via_host`: `host_only.rs`
//!   `member_rotation_note_goes_to_host_only_and_host_routes_to_new_endpoint`, `multi_host.rs`
//!   `member_rotation_note_reaches_every_host_and_hosts_repair_each_other` and
//!   `host_and_member_both_rotated_while_apart_recover_via_another_host`;
//! - `host_rotation_reaches_offline_member_through_verified_member_forwarding`: `host_only.rs`
//!   `host_rotation_reaches_online_members_directly_and_offline_member_by_host_retry` and
//!   `host_overlap_window_candidate_recovers_member_only_within_window`, `multi_host.rs`
//!   `rotated_host_reconnects_members_at_their_current_endpoints_without_overlap`;
//! - `host_cannot_substitute_endpoint_in_relayed_note` (the host no longer relays member
//!   notes to members): `multi_host.rs` `member_rotation_note_reaches_every_host_and_hosts_repair_each_other`
//!   (note altered in host-to-host relay refused) and the forged-host-note cases of
//!   `host_only.rs` `host_rotation_reaches_online_members_directly_and_offline_member_by_host_retry`;
//! - `host_and_all_forwarders_unreachable_means_no_recovery_without_reshare`: `host_only.rs`
//!   `host_and_member_both_rotated_while_apart_lose_each_other`.
use crate::legacy::*;
use identity::{public_key_from_did, verify};

mod model;

use model::{
    A, C, CatchUp, D, ForkPolicy, Frame, GapPolicy, HOST, NoteAudience, Policy, Reject,
    RotationNote, RouteErr, World, all_policies, contains, world,
};

mod tests;
