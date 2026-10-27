use super::*;
use wake_relay_core::api::IssuedToken;
use wake_relay_core::hint::{Hint, STALE_CALL_MS, seal};

pub(super) const NOW: u64 = 1_000_000_000;
pub(super) const KEY: [u8; 32] = [7; 32];
const DEV: [u8; 32] = [9; 32];

pub(super) fn target(n: u8) -> Target {
    Target::Fcm {
        token: format!("synthetic-fcm-token-{n:04}"),
    }
}

pub(super) fn state() -> WakeState {
    let mut s = WakeState::new(KEY);
    s.set_target(target(1));
    s
}

/// Deterministic tags: 1, 2, 3, ...
fn counter() -> impl FnMut() -> [u8; 16] {
    let mut n = 0u8;
    move || {
        n += 1;
        [n; 16]
    }
}

fn contacts(names: &[&str], kinds: Kinds) -> Vec<(String, Kinds)> {
    names
        .iter()
        .map(|n| (format!("did:test:{n}"), kinds))
        .collect()
}

/// Pretends to be the relay: one token per requested tag.
fn answer(req: &RegisterRequest) -> RegisterResponse {
    RegisterResponse {
        tokens: req
            .tags
            .iter()
            .map(|t| IssuedToken {
                tag: t.tag,
                token: format!("wt-{}", t.tag[0]),
            })
            .collect(),
    }
}

/// Plans and applies one register round.
fn register(
    s: &mut WakeState,
    cs: &[(String, Kinds)],
    now: u64,
    tags: impl FnMut() -> [u8; 16],
) -> Vec<(String, Issued)> {
    let (req, pending) = s
        .plan_register(cs, now, tags)
        .expect("something to register");
    let resp = answer(&req);
    s.apply_register(pending, resp)
}

pub(super) fn hint_text(key: &[u8; 32], tag: u8, kind: Kind, sent_at_ms: u64) -> String {
    let hint = Hint {
        tag: [tag; 16],
        kind,
        sent_at_ms,
    };
    seal(key, &hint, [3; 12]).unwrap()
}

pub(super) fn never(_: &str) -> bool {
    false
}

// ---- registration ----

#[test]
fn plans_nothing_without_a_target() {
    let s = WakeState::new(KEY);
    assert!(
        s.plan_register(&contacts(&["a"], Kinds::ALL), NOW, counter())
            .is_none()
    );
}

#[test]
fn plans_nothing_without_contacts() {
    assert!(state().plan_register(&[], NOW, counter()).is_none());
}

#[test]
fn new_contacts_get_a_token_with_a_30_day_expiry() {
    let mut s = state();
    let cs = contacts(&["a", "b"], Kinds::ALL);
    let (req, _) = s.plan_register(&cs, NOW, counter()).unwrap();
    assert_eq!(req.tags.len(), 2);
    assert_eq!(req.hint_key, KEY);
    assert_eq!(req.target, target(1));
    assert!(req.tags.iter().all(|t| t.exp_ms == NOW + TOKEN_LIFE_MS));
    let out = register(&mut s, &cs, NOW, counter());
    assert_eq!(out.len(), 2);
    assert_eq!(s.issued_for("did:test:a").unwrap().token, "wt-1");
    assert_eq!(s.issued_for("did:test:b").unwrap().token, "wt-2");
    // Everything fresh now: nothing to do.
    assert!(s.plan_register(&cs, NOW + DAY_MS, counter()).is_none());
}

#[test]
fn only_the_contact_that_needs_it_is_planned() {
    let mut s = state();
    register(&mut s, &contacts(&["a"], Kinds::ALL), NOW, counter());
    let both = contacts(&["a", "b"], Kinds::ALL);
    let (req, _) = s.plan_register(&both, NOW, || [50; 16]).unwrap();
    assert_eq!(req.tags.len(), 1);
}

#[test]
fn renews_within_seven_days_of_expiry() {
    let mut s = state();
    let cs = contacts(&["a"], Kinds::ALL);
    register(&mut s, &cs, NOW, counter());
    let exp = NOW + TOKEN_LIFE_MS;
    assert!(
        s.plan_register(&cs, exp - RENEW_BEFORE_MS - 1, counter())
            .is_none()
    );
    let (req, _) = s
        .plan_register(&cs, exp - RENEW_BEFORE_MS, counter())
        .unwrap();
    assert_eq!(req.tags.len(), 1);
    // A renewal with the same kinds revokes nothing: the old token just expires.
    s.revoke_outbox.clear();
    register(&mut s, &cs, exp - RENEW_BEFORE_MS, || [8; 16]);
    assert!(s.revoke_outbox.is_empty());
    assert_eq!(s.issued_for("did:test:a").unwrap().tag, [8; 16]);
}

#[test]
fn turning_calls_off_reissues_and_revokes_the_old_token() {
    let mut s = state();
    register(&mut s, &contacts(&["a"], Kinds::ALL), NOW, counter());
    let msg_only = contacts(&["a"], Kinds::MSG);
    let (req, pending) = s.plan_register(&msg_only, NOW, || [5; 16]).unwrap();
    assert_eq!(req.tags[0].kinds, Kinds::MSG);
    // Nothing is revoked until the new token is in hand.
    assert!(s.revoke_outbox.is_empty());
    let resp = answer(&req);
    s.apply_register(pending, resp);
    assert_eq!(s.revoke_outbox, vec!["wt-1".to_owned()]);
    assert_eq!(s.issued_for("did:test:a").unwrap().kinds, Kinds::MSG);
}

#[test]
fn register_requests_are_capped() {
    let s = state();
    let names: Vec<String> = (0..MAX_TAGS_PER_REGISTER + 5)
        .map(|i| i.to_string())
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let (req, pending) = s
        .plan_register(&contacts(&refs, Kinds::MSG), NOW, || [1; 16])
        .unwrap();
    assert_eq!(req.tags.len(), MAX_TAGS_PER_REGISTER);
    drop(pending);
}

#[test]
fn the_rest_follows_in_the_next_round() {
    let mut s = state();
    let names: Vec<String> = (0..MAX_TAGS_PER_REGISTER + 5)
        .map(|i| i.to_string())
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let cs = contacts(&refs, Kinds::MSG);
    let mut n = 0u32;
    let mut tags = || {
        n += 1;
        let mut t = [0u8; 16];
        t[..4].copy_from_slice(&n.to_be_bytes());
        t
    };
    let (req, pending) = s.plan_register(&cs, NOW, &mut tags).unwrap();
    let resp = RegisterResponse {
        tokens: req
            .tags
            .iter()
            .map(|t| IssuedToken {
                tag: t.tag,
                token: "x".into(),
            })
            .collect(),
    };
    s.apply_register(pending, resp);
    let (req2, _) = s.plan_register(&cs, NOW, &mut tags).unwrap();
    assert_eq!(req2.tags.len(), 5);
}

#[test]
fn apply_ignores_unknown_tags() {
    let mut s = state();
    let cs = contacts(&["a"], Kinds::ALL);
    let (_, pending) = s.plan_register(&cs, NOW, counter()).unwrap();
    let resp = RegisterResponse {
        tokens: vec![IssuedToken {
            tag: [99; 16],
            token: "stray".into(),
        }],
    };
    assert!(s.apply_register(pending, resp).is_empty());
    assert!(s.issued.is_empty());
}

#[test]
fn apply_after_the_target_changed_is_dropped() {
    let mut s = state();
    let cs = contacts(&["a"], Kinds::ALL);
    let (req, pending) = s.plan_register(&cs, NOW, counter()).unwrap();
    s.set_target(target(2));
    assert!(s.apply_register(pending, answer(&req)).is_empty());
    assert!(s.issued.is_empty());
}

#[test]
fn a_changed_push_token_clears_issued_but_an_unchanged_one_does_not() {
    let mut s = state();
    register(&mut s, &contacts(&["a"], Kinds::ALL), NOW, counter());
    s.set_target(target(1));
    assert_eq!(s.issued.len(), 1);
    s.set_target(target(2));
    assert!(s.issued.is_empty());
    // Old tokens die with the old push token; nothing to tell the relay.
    assert!(s.revoke_outbox.is_empty());
    assert_eq!(s.target, Some(target(2)));
}

#[test]
fn going_always_on_revokes_everything_issued() {
    let mut s = state();
    register(&mut s, &contacts(&["a", "b"], Kinds::ALL), NOW, counter());
    s.clear_target();
    assert!(s.target.is_none());
    assert!(s.issued.is_empty());
    assert_eq!(s.revoke_outbox.len(), 2);
}

// ---- revoking ----

#[test]
fn block_revokes_our_token_and_drops_theirs() {
    let mut s = state();
    register(&mut s, &contacts(&["a", "b"], Kinds::ALL), NOW, counter());
    s.hold(
        "did:test:a",
        DEV,
        "theirs".into(),
        NOW + DAY_MS,
        Kinds::ALL,
        NOW,
    );
    s.hold(
        "did:test:b",
        DEV,
        "other".into(),
        NOW + DAY_MS,
        Kinds::ALL,
        NOW,
    );
    s.block("did:test:a");
    assert!(s.issued_for("did:test:a").is_none());
    assert!(s.issued_for("did:test:b").is_some());
    assert_eq!(s.take_revokes(), vec!["wt-1".to_owned()]);
    assert_eq!(s.held.len(), 1);
    assert_eq!(s.held[0].did, "did:test:b");
}

#[test]
fn revokes_stay_until_the_relay_confirms() {
    let mut s = state();
    register(&mut s, &contacts(&["a"], Kinds::ALL), NOW, counter());
    s.forget_contact("did:test:a");
    assert_eq!(s.take_revokes().len(), 1);
    assert_eq!(s.take_revokes().len(), 1);
    s.revoked_ok("wt-1");
    assert!(s.take_revokes().is_empty());
}

#[test]
fn a_re_added_contact_gets_a_new_tag() {
    let mut s = state();
    let cs = contacts(&["a"], Kinds::ALL);
    register(&mut s, &cs, NOW, counter());
    s.block("did:test:a");
    register(&mut s, &cs, NOW, || [42; 16]);
    assert_eq!(s.issued_for("did:test:a").unwrap().tag, [42; 16]);
}

// ---- tokens we hold ----

#[test]
fn hold_rejects_expired_too_long_and_oversized() {
    let mut s = state();
    assert!(!s.hold("d", DEV, "t".into(), NOW, Kinds::ALL, NOW));
    assert!(!s.hold("d", DEV, "t".into(), NOW - 1, Kinds::ALL, NOW));
    assert!(!s.hold(
        "d",
        DEV,
        "t".into(),
        NOW + MAX_TOKEN_LIFE_MS + 1,
        Kinds::ALL,
        NOW
    ));
    assert!(!s.hold(
        "d",
        DEV,
        "x".repeat(MAX_TOKEN_CHARS + 1),
        NOW + DAY_MS,
        Kinds::ALL,
        NOW
    ));
    assert!(!s.hold("d", DEV, String::new(), NOW + DAY_MS, Kinds::ALL, NOW));
    assert!(s.held.is_empty());
    assert!(s.hold(
        "d",
        DEV,
        "t".into(),
        NOW + MAX_TOKEN_LIFE_MS,
        Kinds::ALL,
        NOW
    ));
    assert_eq!(s.held.len(), 1);
}

#[test]
fn hold_keeps_one_token_per_device_and_keeps_the_throttle() {
    let mut s = state();
    s.hold("d", DEV, "old".into(), NOW + DAY_MS, Kinds::ALL, NOW);
    assert!(s.wake_for("d", &DEV, Kind::Msg, NOW).is_some());
    s.hold(
        "d",
        DEV,
        "new".into(),
        NOW + 2 * DAY_MS,
        Kinds::ALL,
        NOW + 1,
    );
    s.hold(
        "d",
        [1; 32],
        "second device".into(),
        NOW + DAY_MS,
        Kinds::ALL,
        NOW,
    );
    assert_eq!(s.held.len(), 2);
    assert!(s.wake_for("d", &DEV, Kind::Msg, NOW + 1).is_none());
    assert_eq!(
        s.wake_for("d", &DEV, Kind::Msg, NOW + MSG_WAKE_EVERY_MS)
            .as_deref(),
        Some("new")
    );
}

// ---- waking a contact ----

#[test]
fn wake_for_needs_a_live_token_that_allows_the_kind() {
    let mut s = state();
    assert!(s.wake_for("d", &DEV, Kind::Msg, NOW).is_none());
    s.hold("d", DEV, "t".into(), NOW + 1000, Kinds::MSG, NOW);
    assert!(s.wake_for("d", &DEV, Kind::Call, NOW).is_none());
    assert!(s.wake_for("d", &DEV, Kind::Msg, NOW + 1000).is_none());
    assert_eq!(
        s.wake_for("d", &DEV, Kind::Msg, NOW + 999).as_deref(),
        Some("t")
    );
}

#[test]
fn msg_wakes_are_throttled_to_one_per_30_s_and_calls_to_one_per_10_s() {
    let mut s = state();
    s.hold("d", DEV, "t".into(), NOW + DAY_MS, Kinds::ALL, NOW);
    assert!(s.wake_for("d", &DEV, Kind::Msg, NOW).is_some());
    assert!(s.wake_for("d", &DEV, Kind::Msg, NOW + 29_999).is_none());
    assert!(s.wake_for("d", &DEV, Kind::Msg, NOW + 30_000).is_some());

    assert!(s.wake_for("d", &DEV, Kind::Call, NOW).is_some());
    assert!(s.wake_for("d", &DEV, Kind::Call, NOW + 9_999).is_none());
    assert!(s.wake_for("d", &DEV, Kind::Call, NOW + 10_000).is_some());
}

#[test]
fn throttles_are_independent_per_kind_and_per_device() {
    let mut s = state();
    s.hold("d", DEV, "t1".into(), NOW + DAY_MS, Kinds::ALL, NOW);
    s.hold("d", [2; 32], "t2".into(), NOW + DAY_MS, Kinds::ALL, NOW);
    assert!(s.wake_for("d", &DEV, Kind::Msg, NOW).is_some());
    assert!(s.wake_for("d", &DEV, Kind::Call, NOW).is_some());
    assert_eq!(
        s.wake_for("d", &[2; 32], Kind::Msg, NOW).as_deref(),
        Some("t2")
    );
}

#[test]
fn a_refused_wake_does_not_start_the_throttle() {
    let mut s = state();
    s.hold("d", DEV, "t".into(), NOW + DAY_MS, Kinds::MSG, NOW);
    assert!(s.wake_for("d", &DEV, Kind::Call, NOW).is_none());
    assert!(s.held[0].held.last_call_wake_ms.is_none());
}

#[test]
fn relay_410_drops_the_token() {
    let mut s = state();
    s.hold("d", DEV, "t".into(), NOW + DAY_MS, Kinds::ALL, NOW);
    s.hold("d", [2; 32], "u".into(), NOW + DAY_MS, Kinds::ALL, NOW);
    s.wake_result("d", &DEV, Kind::Msg, 410);
    assert_eq!(s.held.len(), 1);
    assert_eq!(s.held[0].device, [2; 32]);
}

#[test]
fn relay_403_removes_that_kind() {
    let mut s = state();
    s.hold("d", DEV, "t".into(), NOW + DAY_MS, Kinds::ALL, NOW);
    s.wake_result("d", &DEV, Kind::Call, 403);
    assert_eq!(s.held[0].held.kinds, Kinds::MSG);
    assert!(s.wake_for("d", &DEV, Kind::Call, NOW).is_none());
    // The last kind going leaves nothing to keep.
    s.wake_result("d", &DEV, Kind::Msg, 403);
    assert!(s.held.is_empty());
}

#[test]
fn relay_202_429_and_5xx_change_nothing() {
    let mut s = state();
    s.hold("d", DEV, "t".into(), NOW + DAY_MS, Kinds::ALL, NOW);
    let before = s.clone();
    for status in [202, 429, 500, 503] {
        s.wake_result("d", &DEV, Kind::Msg, status);
    }
    assert_eq!(s, before);
}

// ---- being woken ----

/// A state that issued tags 1 (msg+call) and 2 (msg only) to two contacts.
pub(super) fn woken_state() -> WakeState {
    let mut s = state();
    let cs = vec![
        ("did:test:a".to_owned(), Kinds::ALL),
        ("did:test:b".to_owned(), Kinds::MSG),
    ];
    register(&mut s, &cs, NOW, counter());
    s
}

fn is_a(did: &str) -> bool {
    did == "did:test:a"
}

#[test]
fn msg_wake_dials_back() {
    let mut s = woken_state();
    let h = hint_text(&KEY, 1, Kind::Msg, NOW);
    assert_eq!(
        s.on_wake(&h, NOW, never),
        WakeAction::DialBack {
            did: "did:test:a".into()
        }
    );
}

#[test]
fn call_wake_rings_and_never_dials() {
    let mut s = woken_state();
    let h = hint_text(&KEY, 1, Kind::Call, NOW);
    assert_eq!(
        s.on_wake(&h, NOW + 5_000, never),
        WakeAction::Ring {
            did: "did:test:a".into()
        }
    );
}

#[test]
fn blocked_contacts_are_ignored() {
    let mut s = woken_state();
    let msg = hint_text(&KEY, 1, Kind::Msg, NOW);
    let call = hint_text(&KEY, 1, Kind::Call, NOW);
    assert_eq!(s.on_wake(&msg, NOW, is_a), WakeAction::Ignore);
    assert_eq!(s.on_wake(&call, NOW, is_a), WakeAction::IgnoreCall);
}

#[test]
fn a_stale_call_is_not_rung() {
    let mut s = woken_state();
    let h = hint_text(&KEY, 1, Kind::Call, NOW);
    assert_eq!(
        s.on_wake(&h, NOW + STALE_CALL_MS, never),
        WakeAction::Ring {
            did: "did:test:a".into()
        }
    );
    assert_eq!(
        s.on_wake(&h, NOW + STALE_CALL_MS + 1, never),
        WakeAction::IgnoreCall
    );
}

#[test]
fn a_stale_msg_wake_still_dials_back() {
    let mut s = woken_state();
    let h = hint_text(&KEY, 1, Kind::Msg, NOW);
    assert!(matches!(
        s.on_wake(&h, NOW + 10 * STALE_CALL_MS, never),
        WakeAction::DialBack { .. }
    ));
}

#[test]
fn a_call_wake_on_a_msg_only_token_is_refused() {
    let mut s = woken_state();
    let h = hint_text(&KEY, 2, Kind::Call, NOW);
    assert_eq!(s.on_wake(&h, NOW, never), WakeAction::IgnoreCall);
}

#[test]
fn a_revoked_tag_is_ignored() {
    let mut s = woken_state();
    let msg = hint_text(&KEY, 1, Kind::Msg, NOW);
    let call = hint_text(&KEY, 1, Kind::Call, NOW);
    s.block("did:test:a");
    assert_eq!(s.on_wake(&msg, NOW, never), WakeAction::Ignore);
    assert_eq!(s.on_wake(&call, NOW, never), WakeAction::IgnoreCall);
}

#[test]
fn an_unknown_tag_is_ignored() {
    let mut s = woken_state();
    let h = hint_text(&KEY, 77, Kind::Msg, NOW);
    assert_eq!(s.on_wake(&h, NOW, never), WakeAction::Ignore);
}

#[test]
fn garbage_and_wrong_key_are_ignored() {
    let mut s = woken_state();
    assert_eq!(s.on_wake("", NOW, never), WakeAction::Ignore);
    assert_eq!(s.on_wake("not a hint!", NOW, never), WakeAction::Ignore);
    assert_eq!(s.on_wake(&"A".repeat(1000), NOW, never), WakeAction::Ignore);
    let wrong = hint_text(&[1; 32], 1, Kind::Msg, NOW);
    assert_eq!(s.on_wake(&wrong, NOW, never), WakeAction::Ignore);
    let wrong_call = hint_text(&[1; 32], 1, Kind::Call, NOW);
    assert_eq!(s.on_wake(&wrong_call, NOW, never), WakeAction::Ignore);
}

// ---- misc ----

#[test]
fn state_round_trips_through_json() {
    let mut s = woken_state();
    s.hold("did:test:c", DEV, "t".into(), NOW + DAY_MS, Kinds::ALL, NOW);
    s.wake_for("did:test:c", &DEV, Kind::Msg, NOW);
    s.forget_contact("did:test:b");
    let json = serde_json::to_string(&s).unwrap();
    let back: WakeState = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn user_agent_names_the_app() {
    assert_eq!(user_agent("0.2.0"), "Tinline/0.2.0");
}

// ---- which account a push is for (several accounts on one device) ----

#[test]
fn a_hint_opens_only_under_its_own_accounts_key() {
    let s = woken_state();
    let mine = hint_text(&KEY, 1, Kind::Msg, NOW);
    let theirs = hint_text(&[1; 32], 1, Kind::Msg, NOW);
    assert!(s.opens(&mine));
    assert!(hint_opens(&KEY, &mine));
    assert!(!s.opens(&theirs), "another account's hint does not open");
    assert!(!hint_opens(&KEY, &theirs));
    assert!(hint_opens(&[1; 32], &theirs) && !hint_opens(&[1; 32], &mine));
}

#[test]
fn garbage_and_oversized_hints_open_nothing() {
    let s = woken_state();
    for text in ["", "not a hint!", "AAAA", &"A".repeat(1000)] {
        assert!(!s.opens(text), "{text:?}");
        assert!(!hint_opens(&KEY, text), "{text:?}");
    }
}

#[test]
fn opening_says_whose_push_it_is_whatever_it_would_mean() {
    // An unissued tag, a blocked contact and a stale call are all Ignore/IgnoreCall for this
    // account, yet the push is still its own: another account must not be tried for them.
    let mut s = woken_state();
    let unknown = hint_text(&KEY, 99, Kind::Msg, NOW);
    assert_eq!(s.on_wake(&unknown, NOW, never), WakeAction::Ignore);
    assert!(s.opens(&unknown));
    let call = hint_text(&KEY, 1, Kind::Call, NOW);
    assert_eq!(s.on_wake(&call, NOW, is_a), WakeAction::IgnoreCall);
    assert!(s.opens(&call));
    let stale = hint_text(&KEY, 1, Kind::Call, NOW);
    assert_eq!(s.on_wake(&stale, NOW + STALE_CALL_MS + 1, never), WakeAction::IgnoreCall);
    assert!(s.opens(&stale));
}

#[test]
fn a_state_with_another_hint_key_no_longer_opens_the_old_hints() {
    // What a lock does to routing: the key is gone with the state, a fresh state has its own.
    let old = hint_text(&KEY, 1, Kind::Msg, NOW);
    assert!(WakeState::new(KEY).opens(&old));
    assert!(!WakeState::new([2; 32]).opens(&old));
}
