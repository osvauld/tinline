use super::*;

const P: RotationPolicy = RotationPolicy::ON;

// KIND: unit (rotation, D11) — members never rotate; a host rotates an interval plus its jitter
// after the key was made.
#[test]
fn due_is_made_plus_interval_plus_jitter_and_only_when_hosting() {
    assert_eq!(rotation_due(1_000, &P, 5, 2_000, false), None);
    assert_eq!(
        rotation_due(1_000, &P, 5, 2_000, true),
        Some(1_000 + P.interval_ms + 5)
    );
    // The stored jitter never exceeds the policy's bound (a lowered setting caps it).
    let tight = RotationPolicy { jitter_ms: 3, ..P };
    assert_eq!(
        rotation_due(1_000, &tight, 5, 2_000, true),
        Some(1_000 + P.interval_ms + 3)
    );
}

// KIND: unit (rotation, 2026-10-19) — off by default: nothing is ever due, hosting or not, however
// overdue the key is.
#[test]
fn the_default_policy_never_rotates() {
    let off = RotationPolicy::DEFAULT;
    assert!(!off.enabled);
    assert!(off.invalid().is_none());
    for now in [2_000, 1_000 + 50 * off.interval_ms] {
        assert_eq!(rotation_due(1_000, &off, 0, now, true), None);
        assert_eq!(rotation_due(1_000, &off, 0, now, false), None);
    }
}

// KIND: unit (rotation) — overdue on resume: due now (once); the promote then sets `made = now`,
// so the next deadline counts a full interval from the rotation, not from the missed one.
#[test]
fn an_overdue_device_rotates_once_and_counts_from_then() {
    let made = 1_000;
    let now = made + 5 * P.interval_ms;
    let due = rotation_due(made, &P, 0, now, true).unwrap();
    assert!(due <= now, "overdue");
    assert_eq!(
        rotation_due(now, &P, 0, now, true),
        Some(now + P.interval_ms)
    );
}

// KIND: unit (rotation) — a key made more than an interval "in the future" (the clock went back)
// is due now instead of never.
#[test]
fn a_key_from_the_future_is_due_now() {
    let now = 10 * P.interval_ms;
    assert_eq!(
        rotation_due(now + 2 * P.interval_ms, &P, 0, now, true),
        Some(now)
    );
}

// KIND: property (rotation, G-08c) — for any valid policy and any key made up to now, the
// deadline is never earlier than `made + MIN_INTERVAL_MS`, and it depends on nothing but its
// inputs: no record time can move it (there is no such input), and asking again later (or after
// re-saving the same setting) gives the same deadline.
#[test]
fn due_is_never_earlier_than_made_plus_the_floor() {
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..10_000 {
        let interval = RotationPolicy::MIN_INTERVAL_MS + next() % (60 * DAY_MS);
        let policy = RotationPolicy {
            enabled: true,
            interval_ms: interval,
            jitter_ms: next() % (interval + 1),
            announce_ms: 1 + next() % (30 * DAY_MS),
        };
        assert!(policy.invalid().is_none());
        let made = next() % (1 << 45);
        let now = made + next() % (90 * DAY_MS);
        let jitter = next() % (2 * DAY_MS);
        let due = rotation_due(made, &policy, jitter, now, true).unwrap();
        assert!(due >= made + RotationPolicy::MIN_INTERVAL_MS);
        assert_eq!(
            Some(due),
            rotation_due(made, &policy, jitter, now + next() % DAY_MS, true)
        );
    }
}

// KIND: unit (rotation) — the setting cannot go below the floor or carry nonsense.
#[test]
fn invalid_policies_are_refused() {
    assert!(P.invalid().is_none());
    let short = RotationPolicy {
        interval_ms: RotationPolicy::MIN_INTERVAL_MS - 1,
        jitter_ms: 0,
        ..P
    };
    assert!(short.invalid().is_some());
    assert!(
        RotationPolicy {
            jitter_ms: P.interval_ms + 1,
            ..P
        }
        .invalid()
        .is_some()
    );
    assert!(
        RotationPolicy {
            announce_ms: 0,
            ..P
        }
        .invalid()
        .is_some()
    );
}
