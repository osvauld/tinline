//! When this device's group endpoint rotates: the policy values and the schedule
//! (`docs/rotation.md`). The rebind itself is `engine/rotation.rs`; the key and its state are
//! the `ep/*` rows (`store/endpoint.rs`).

const DAY_MS: u64 = 24 * 3600 * 1000;

/// How often the group endpoint rotates, per device (stored in `group.redb`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, uniffi::Record,
)]
pub struct RotationPolicy {
    /// Time between two rotations, ms (counted from the last one).
    pub interval_ms: u64,
    /// Upper bound of the random extra delay drawn once per key, ms.
    pub jitter_ms: u64,
    /// How long a host that rotated keeps telling offline members where it moved, ms.
    pub announce_ms: u64,
}

impl RotationPolicy {
    /// Decided defaults (2026-10-16): 7 days, up to 1 day of jitter, announce for a whole
    /// interval; the old endpoint closes right after the promote (no overlap).
    pub const DEFAULT: Self = Self {
        interval_ms: 7 * DAY_MS,
        jitter_ms: DAY_MS,
        announce_ms: 7 * DAY_MS,
    };
    /// The shortest interval a setting may ask for (decided 2026-10-16), so a
    /// mistaken setting cannot make a device rotate (and drop every session) every few minutes.
    pub const MIN_INTERVAL_MS: u64 = DAY_MS;

    /// Why this policy may not be stored, if it may not.
    pub fn invalid(&self) -> Option<&'static str> {
        if self.interval_ms < Self::MIN_INTERVAL_MS {
            Some("the rotation interval is too short")
        } else if self.jitter_ms > self.interval_ms {
            Some("the rotation jitter is longer than the interval")
        } else if self.announce_ms == 0 {
            Some("the announce window is empty")
        } else {
            None
        }
    }
}

impl Default for RotationPolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A random jitter in `0..=max` ms, drawn once per key (persisted as `ep/jitter`).
pub(crate) fn draw_jitter(max: u64) -> u64 {
    if max == 0 {
        return 0;
    }
    let mut b = [0u8; 8];
    cryptography::rng::fill(&mut b);
    u64::from_le_bytes(b) % max.saturating_add(1)
}

/// When the endpoint made at `made` (unix ms, local clock) is due to rotate, `None` while this
/// device hosts no chat and no calls (members never rotate). Counted from the last rotation
/// only: no record time (`at`) and no saving of the setting moves it, so it cannot be postponed
/// forever; a device that was off past the deadline rotates once on resume and counts a full
/// interval from then. A `made` more than an interval in the future (the clock went back) is
/// due now.
pub(crate) fn rotation_due(
    made: u64,
    policy: &RotationPolicy,
    jitter: u64,
    now: u64,
    hosting: bool,
) -> Option<u64> {
    if !hosting {
        return None;
    }
    let interval = policy.interval_ms.max(1);
    if made > now.saturating_add(interval) {
        return Some(now);
    }
    Some(
        made.saturating_add(interval)
            .saturating_add(jitter.min(policy.jitter_ms)),
    )
}

#[cfg(test)]
mod tests;
