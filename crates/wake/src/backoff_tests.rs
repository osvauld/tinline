//! The `msg` wake schedule of a target that does not answer (`msg_wake_gap_ms`,
//! `msg_wake_due`, `WakeState::wake_for` / `reached`; `docs/notifications-plan.md` N2).

use super::tests::{NOW, state};
use super::*;

const DEV: [u8; 32] = [9; 32];
const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;
const WEEK: u64 = 7 * DAY_MS;

/// Wake times over `span` from `start` when something tries every `step` ms and every try that
/// is due goes out (the count only grows: the target never answers).
fn schedule(start: u64, span: u64, step: u64) -> Vec<u64> {
    let (mut last, mut count, mut out) = (None, 0u32, Vec::new());
    let mut now = start;
    while now < start + span {
        if msg_wake_due(last, count, now) {
            last = Some(now);
            count += 1;
            out.push(now - start);
        }
        now += step;
    }
    out
}

/// A state holding one long-lived `msg` token of `did:test:b`'s device.
fn holding() -> WakeState {
    let mut s = state();
    assert!(s.hold(
        "did:test:b",
        DEV,
        "tok".into(),
        NOW + 29 * DAY_MS,
        Kinds::ALL,
        NOW
    ));
    s
}

#[test]
fn backoff_gaps_grow_to_an_hour() {
    assert_eq!(
        msg_wake_gap_ms(0),
        MSG_WAKE_EVERY_MS,
        "reached, then a wake: the floor"
    );
    assert_eq!(msg_wake_gap_ms(1), 30_000);
    assert_eq!(msg_wake_gap_ms(2), 90_000);
    assert_eq!(msg_wake_gap_ms(3), 8 * MIN);
    assert_eq!(msg_wake_gap_ms(4), 20 * MIN);
    assert_eq!(msg_wake_gap_ms(5), HOUR);
    assert_eq!(msg_wake_gap_ms(u32::MAX), HOUR);
}

#[test]
fn backoff_wakes_at_0_30s_2m_10m_30m_then_hourly() {
    let at = schedule(NOW, 4 * HOUR, 1_000);
    assert_eq!(
        at,
        vec![
            0,
            30_000,
            2 * MIN,
            10 * MIN,
            30 * MIN,
            90 * MIN,
            150 * MIN,
            210 * MIN
        ]
    );
}

#[test]
fn backoff_due_respects_the_last_wake() {
    assert!(msg_wake_due(None, 0, NOW), "never woken");
    assert!(msg_wake_due(None, 7, NOW));
    assert!(
        !msg_wake_due(Some(NOW), 0, NOW + 29_999),
        "the 30 s floor holds after a reset"
    );
    assert!(msg_wake_due(Some(NOW), 0, NOW + 30_000));
    assert!(!msg_wake_due(Some(NOW), 5, NOW + HOUR - 1));
    assert!(msg_wake_due(Some(NOW), 5, NOW + HOUR));
    assert!(
        msg_wake_due(Some(u64::MAX - 1), 5, u64::MAX),
        "saturates at the end of time, no overflow"
    );
}

#[test]
fn backoff_a_week_with_the_other_side_off_is_about_170_wakes() {
    // Tried every second for a week (far more often than any dial loop).
    let at = schedule(NOW, WEEK, 1_000);
    assert!(at.len() <= 175, "{} wakes in a week", at.len());
    assert!(
        at.len() >= 165,
        "still hourly: {} wakes in a week",
        at.len()
    );
    let day = at
        .iter()
        .filter(|t| **t >= DAY_MS && **t < 2 * DAY_MS)
        .count();
    assert_eq!(day, 24, "about 24 a day once settled");
}

#[test]
fn backoff_wake_for_follows_the_schedule_for_a_week() {
    let mut s = holding();
    let mut sent = 0;
    let mut now = NOW;
    while now < NOW + WEEK {
        if s.wake_for("did:test:b", &DEV, Kind::Msg, now).is_some() {
            sent += 1;
        }
        now += 5_000;
    }
    assert!(sent <= 175, "{sent} wakes in a week");
    assert_eq!(s.held[0].held.msg_wakes, sent);
}

#[test]
fn backoff_reaching_the_contact_starts_it_over() {
    let mut s = holding();
    let mut now = NOW;
    for _ in 0..6 {
        while s.wake_for("did:test:b", &DEV, Kind::Msg, now).is_none() {
            now += 1_000;
        }
    }
    assert_eq!(s.held[0].held.msg_wakes, 6);
    assert!(
        s.wake_for("did:test:b", &DEV, Kind::Msg, now + 30 * MIN)
            .is_none(),
        "hourly by now"
    );

    assert!(s.reached("did:test:b"));
    assert!(!s.reached("did:test:b"), "nothing left to reset");
    assert!(
        s.wake_for("did:test:b", &DEV, Kind::Msg, now + 29_000)
            .is_none(),
        "the 30 s floor"
    );
    assert!(
        s.wake_for("did:test:b", &DEV, Kind::Msg, now + 30_000)
            .is_some(),
        "prompt again"
    );
    assert!(
        s.wake_for("did:test:b", &DEV, Kind::Msg, now + 60_000)
            .is_some(),
        "30 s later"
    );
    assert_eq!(s.held[0].held.msg_wakes, 2);
}

#[test]
fn backoff_reached_touches_only_that_contact() {
    let mut s = holding();
    assert!(s.hold(
        "did:test:c",
        DEV,
        "tok-c".into(),
        NOW + DAY_MS,
        Kinds::ALL,
        NOW
    ));
    assert!(s.wake_for("did:test:b", &DEV, Kind::Msg, NOW).is_some());
    assert!(s.wake_for("did:test:c", &DEV, Kind::Msg, NOW).is_some());
    assert!(s.reached("did:test:b"));
    let wakes = |did: &str| s.held.iter().find(|e| e.did == did).unwrap().held.msg_wakes;
    assert_eq!((wakes("did:test:b"), wakes("did:test:c")), (0, 1));
}

#[test]
fn backoff_call_wakes_are_not_part_of_it() {
    let mut s = holding();
    for i in 0..10 {
        assert!(
            s.wake_for("did:test:b", &DEV, Kind::Call, NOW + i * CALL_WAKE_EVERY_MS)
                .is_some()
        );
    }
    assert_eq!(s.held[0].held.msg_wakes, 0);
}

#[test]
fn backoff_a_new_token_keeps_the_schedule() {
    let mut s = holding();
    assert!(s.wake_for("did:test:b", &DEV, Kind::Msg, NOW).is_some());
    assert!(s.hold(
        "did:test:b",
        DEV,
        "renewed".into(),
        NOW + 20 * DAY_MS,
        Kinds::ALL,
        NOW + 1
    ));
    assert_eq!(s.held[0].held.msg_wakes, 1);
}

#[test]
fn backoff_old_state_without_the_count_loads() {
    let mut v = serde_json::to_value(holding()).unwrap();
    v["held"][0]["held"]
        .as_object_mut()
        .unwrap()
        .remove("msg_wakes");
    let s: WakeState = serde_json::from_value(v).unwrap();
    assert_eq!(s.held[0].held.msg_wakes, 0);
}

#[test]
fn backoff_reached_device_touches_only_that_device() {
    let mut s = holding();
    let other = [8u8; 32];
    assert!(s.hold(
        "did:test:b",
        other,
        "tok-2".into(),
        NOW + DAY_MS,
        Kinds::MSG,
        NOW
    ));
    assert!(s.wake_for("did:test:b", &DEV, Kind::Msg, NOW).is_some());
    assert!(s.wake_for("did:test:b", &other, Kind::Msg, NOW).is_some());
    assert!(s.reached_device("did:test:b", &DEV));
    assert!(
        !s.reached_device("did:test:b", &DEV),
        "nothing left to reset"
    );
    let wakes = |d: &[u8; 32]| {
        s.held
            .iter()
            .find(|e| e.device == *d)
            .unwrap()
            .held
            .msg_wakes
    };
    assert_eq!((wakes(&DEV), wakes(&other)), (0, 1));
}

#[test]
fn backoff_drop_held_forgets_one_device_only() {
    let mut s = holding();
    let other = [8u8; 32];
    assert!(s.hold(
        "did:test:b",
        other,
        "tok-2".into(),
        NOW + DAY_MS,
        Kinds::MSG,
        NOW
    ));
    assert!(s.drop_held("did:test:b", &DEV));
    assert!(!s.drop_held("did:test:b", &DEV));
    assert_eq!(s.held.len(), 1);
    assert_eq!(s.held[0].device, other);
}
