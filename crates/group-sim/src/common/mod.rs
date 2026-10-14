//! Shared kit for the group-sim models.
//!
//! Only items that were byte-for-byte (or semantically) identical copies in several models live
//! here. Items that merely look alike but differ (domain-separation constants, `Post`/`Board`
//! shapes, `World`s, the sealing helpers, the legacy LCG) stay in their own model on purpose:
//! the models are decision records and each pins its own variant.
//!
//! Provenance: `SimRng` and the key/digest helpers were copied unchanged through
//! `host_only -> multi_host -> flags -> removal_rotation` (host_only @dcdf4a4).
mod explore;
mod keys;
mod rng;

pub(crate) use explore::permutations;
pub(crate) use keys::*;
pub(crate) use rng::SimRng;

/// Longest grant chain a verifier follows (identical in authority, multi_host, flags, removal_rotation).
pub(crate) const MAX_CHAIN: usize = 8;

// Actor slots shared by multi_host, flags and removal_rotation (alice has two devices).
pub(crate) const OLGA: usize = 0;
pub(crate) const A1: usize = 1;
pub(crate) const A2: usize = 2;
pub(crate) const BOB: usize = 3;
