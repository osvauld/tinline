//! G-B: candidate causal-policy oracles for gates D01–D05. Status: proposal — not approved.
//!
//! NOTHING here selects a policy. Each `candidate-policy D0x` test encodes ONE option of an
//! open gate; a `limitation` test passes by demonstrating what an option cannot achieve.
//! See `docs/group-policy-options.md` for the comparison table and the open questions.
//!
//! Model (test-only, not a wire format):
//! - Membership ops (`MetaOp`) are real DID-signed records (Ed25519 via `identity`) forming a
//!   causal DAG: `parents` = the meta frontier the issuer had seen, `seen_posts` = the content
//!   the issuer had accepted, `at` = the issuer's CLAIMED clock (only timestamp candidates read it).
//! - Posts are the existing real device-signed `Envelope`s (real Loro updates). The existing
//!   `Body` has no `meta_frontier` field, so a provisional policy `Header` (frontier, claimed
//!   time, epoch) is carried as JSON in the signed `text`. Production would carry it in
//!   `GroupUpdate.meta_frontier` (docs/group-chat.md §5); either way it is signer-chosen.
//! - `Node` admits ops/posts (signature + reference checks; pending while references are
//!   missing) and evaluates a `PostRule` + `Membership` rule over what it holds. The content
//!   layer is then materialized by feeding policy-accepted envelopes, in causal order, into the
//!   existing `Replica` (unchanged code), so Loro/proof checks and pending dependencies are real.
//! - Fixture simplifications: the genesis creator is always admin and cannot be removed;
//!   unauthorized ops are retained but ineffective (their effect can depend on later knowledge
//!   under some candidates); no pending bound on the policy layer.
//!
//! R1b: content here uses the LEGACY device-key-signed `Envelope` fixture (`tests.rs`; superseded
//! by U4 revised + U16 DID-signed posts with an app-supplied writer ID, see `host_only.rs`). The
//! envelope is only a carrier; the properties tested here do not depend on how a post is signed.
use crate::common::permutations;
use crate::legacy::*;
use identity::{public_key_from_did, verify};
use std::cell::RefCell;

mod nodes;
mod ops;
mod world;

use nodes::{Node, Report};
use ops::{
    Admit, Change, Ev, Header, Members, Membership, MetaOp, PostRule, REMOVE_WINS, Resolve,
    Verdict, authorized, header, post, second_device_post, verify_cached,
};
use world::{D01, World, add, epoch, explore, heal, m, p, remove, side};

mod tests;
