//! When this device's group endpoint rotates. The policy values and the schedule live in
//! `group_core::rotation`, re-exported here; this module declares the policy to UniFFI. The
//! rebind itself is `engine/rotation.rs`.

pub use group_core::rotation::*;

/// Whether and how often this device's endpoints rotate (stored in `group.redb`; it drives the
/// private endpoint too, `node/rotation.rs`).
#[uniffi::remote(Record)]
pub struct RotationPolicy {
    /// Off by default (decided 2026-10-19 for the alpha: the apps have no rotation settings or
    /// lost-touch screen yet, so a contact who misses the announce window would drop silently).
    /// While off nothing is scheduled; `private_rotate_now` still works.
    pub enabled: bool,
    /// Time between two rotations, ms (counted from the last one).
    pub interval_ms: u64,
    /// Upper bound of the random extra delay drawn once per key, ms.
    pub jitter_ms: u64,
    /// How long a host that rotated keeps telling offline members where it moved, ms.
    pub announce_ms: u64,
}
