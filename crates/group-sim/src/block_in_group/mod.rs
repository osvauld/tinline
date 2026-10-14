//! W2-G: a personal DID block inside a group (user decisions U5, U6). Status: proposal — not
//! approved. Simulation only; no production API, wire format or UI.
//!
//! Model (test-only):
//! - A personal block is a LOCAL set of DIDs owned by one member (`BTreeSet<String>`). It is
//!   never signed, never sent, and is not a group op. Shared group state is the existing
//!   `Replica` (signed genesis + delegated `Grant`s, real Loro, real Ed25519) — unchanged code.
//! - "Host" = a `Replica` that members upload to and are served from (as in `tests.rs`). A
//!   `HostRelay` parameter encodes the OPEN question whether a host that blocked someone may
//!   withhold relay (G-BLOCK); `Neutral` is the only option that leaves group state untouched.
//! - Local display of a blocked member's group messages is a pure function of (replica, block
//!   list, `Display`); the three `Display` options are candidates, none is selected.
//! - Chat requests from a blocked member (U3) are W2-Q's scope and are not modelled here.
//!
//! R1b (U4 revised, U5 revised, U7): the tests in this file use the LEGACY device-key-signed
//! `Envelope` fixture (`tests.rs`) only as a carrier for properties that do not depend on it
//! (a personal block never touches shared group state, grants or relay; display and
//! host-relay candidates). The `run` trace also lets D sync directly with C (step 4), which
//! U7 no longer allows for group traffic; it is kept as the path the host-relay candidates
//! measure against. Removed by R1b, with their current-design coverage:
//! - `blocker_host_candidate_neutral_relay_converges_and_is_silent` (asserted silence; the
//!   neutral-relay fingerprint is the `host_is_blocker = true` case of
//!   `personal_block_leaves_every_replica_grant_and_relayed_byte_unchanged`, and the visible
//!   notice is in `multi_host.rs` `removed_member_and_blocked_contact_receive_verifiable_signed_notices`);
//! - `blocked_member_never_learns_blockers_private_endpoint_through_group` and
//!   `author_key_equal_to_private_transport_key_leaks_private_endpoint_to_blocked_member`
//!   (device-key content, members holding each other's group endpoints): see `host_only.rs`
//!   `blocked_member_learns_no_private_endpoint_from_did_signed_group_content` and
//!   `members_receive_no_device_key_transport_key_or_endpoint_of_any_other_member`.
use super::*;

mod fixtures;

use fixtures::{World, world, replica, did, catch_up, ids, HostRelay, run, Display, Line, render, texts};

mod tests;
