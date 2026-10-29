//! Code wake tokens: one `msg` token per contact ticket, the wake it means, its throttle and its
//! revocation (`docs/push.md` "Code wake tokens").

use super::tests::{KEY, NOW, hint_text, never, state, target, woken_state};
use super::*;
use wake_relay_core::api::IssuedToken;

const NONCE: &str = "bm9uY2Ux";
const TAG: u8 = 40;

fn answer(req: &RegisterRequest) -> RegisterResponse {
    RegisterResponse {
        tokens: req
            .tags
            .iter()
            .map(|t| IssuedToken {
                tag: t.tag,
                token: format!("code-{}", t.tag[0]),
            })
            .collect(),
    }
}

/// `s` with a code token (tag [`TAG`]) for the ticket [`NONCE`], living a week.
fn with_code(mut s: WakeState) -> (WakeState, String) {
    let (req, pending) = s
        .plan_code(NONCE, NOW + 7 * DAY_MS, NOW, [TAG; 16])
        .expect("a target is set");
    assert_eq!(req.tags.len(), 1);
    assert_eq!(req.tags[0].kinds, Kinds::MSG, "never call");
    assert_eq!(
        req.tags[0].exp_ms,
        NOW + 7 * DAY_MS,
        "expires with the ticket"
    );
    assert_eq!(req.hint_key, KEY);
    let token = s.apply_code(pending, answer(&req)).expect("our tag");
    (s, token)
}

#[test]
fn code_token_is_planned_only_with_a_target_and_a_sane_expiry() {
    assert!(
        WakeState::new(KEY)
            .plan_code(NONCE, NOW + DAY_MS, NOW, [1; 16])
            .is_none()
    );
    let s = state();
    assert!(
        s.plan_code(NONCE, NOW, NOW, [1; 16]).is_none(),
        "already expired"
    );
    assert!(
        s.plan_code(NONCE, NOW + MAX_TOKEN_LIFE_MS + 1, NOW, [1; 16])
            .is_none()
    );
    let (s, token) = with_code(state());
    assert_eq!(s.code_for(NONCE).unwrap().token, token);
    // An answer for a target that changed meanwhile is dropped.
    let mut s2 = state();
    let (req, pending) = s2.plan_code(NONCE, NOW + DAY_MS, NOW, [2; 16]).unwrap();
    s2.set_target(target(9));
    assert_eq!(s2.apply_code(pending, answer(&req)), None);
    assert!(s2.codes.is_empty());
}

#[test]
fn code_tag_hint_is_a_code_wake() {
    let (mut s, _) = with_code(woken_state());
    let h = hint_text(&KEY, TAG, Kind::Msg, NOW);
    assert_eq!(s.on_wake(&h, NOW, never), WakeAction::CodeWake);
    // Contacts' tags still mean their contact.
    let h = hint_text(&KEY, 1, Kind::Msg, NOW);
    assert_eq!(
        s.on_wake(&h, NOW, never),
        WakeAction::DialBack {
            did: "did:test:a".into()
        }
    );
    // A code token never rings.
    let call = hint_text(&KEY, TAG, Kind::Call, NOW);
    assert_eq!(s.on_wake(&call, NOW, never), WakeAction::IgnoreCall);
    // Nobody to block: the sender is a stranger.
    let h = hint_text(&KEY, TAG, Kind::Msg, NOW + 1);
    assert_eq!(s.on_wake(&h, NOW + 1, |_| true), WakeAction::CodeWake);
}

#[test]
fn code_wakes_are_throttled_per_tag() {
    let (mut s, _) = with_code(state());
    let at = |t: u64| hint_text(&KEY, TAG, Kind::Msg, t);
    for i in 0..CODE_WAKE_CAP as u64 {
        assert_eq!(
            s.on_wake(&at(NOW + i), NOW + i, never),
            WakeAction::CodeWake,
            "wake {i}"
        );
    }
    let over = NOW + CODE_WAKE_CAP as u64;
    assert_eq!(
        s.on_wake(&at(over), over, never),
        WakeAction::Ignore,
        "over the cap"
    );
    // The window slides: once the first wake is older than the window, one more is acted on.
    let later = NOW + CODE_WAKE_WINDOW_MS;
    assert_eq!(s.on_wake(&at(later), later, never), WakeAction::CodeWake);
    assert_eq!(s.on_wake(&at(later), later, never), WakeAction::Ignore);
    // The throttle survives the sealed state (JSON).
    let back: WakeState = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back, s);
}

#[test]
fn expired_code_tag_is_ignored_and_forgotten() {
    let (mut s, token) = with_code(state());
    let exp = NOW + 7 * DAY_MS;
    let h = hint_text(&KEY, TAG, Kind::Msg, exp);
    assert_eq!(s.on_wake(&h, exp, never), WakeAction::Ignore);
    // Expired codes are dropped without a revoke (the relay refuses them by itself).
    assert!(s.retire_codes(exp, |_| true));
    assert!(s.codes.is_empty());
    assert!(!s.revoke_outbox.contains(&token));
    assert_eq!(s.on_wake(&h, exp, never), WakeAction::Ignore);
}

#[test]
fn retired_codes_are_revoked() {
    // A ticket that stopped adding contacts (reset, block): its token goes to the revoke outbox.
    let (mut s, token) = with_code(state());
    assert!(
        !s.retire_codes(NOW, |n| n == NONCE),
        "still live: nothing changes"
    );
    assert!(s.retire_codes(NOW, |_| false));
    assert_eq!(s.revoke_outbox, vec![token.clone()]);
    let h = hint_text(&KEY, TAG, Kind::Msg, NOW);
    assert_eq!(
        s.on_wake(&h, NOW, never),
        WakeAction::Ignore,
        "a revoked tag means nothing"
    );
    // Push turned off takes code tokens back too; a new target only forgets them.
    let (mut s, token) = with_code(state());
    s.clear_target();
    assert!(s.codes.is_empty() && s.revoke_outbox.contains(&token));
    let (mut s, token) = with_code(state());
    s.set_target(target(2));
    assert!(s.codes.is_empty() && !s.revoke_outbox.contains(&token));
    // Reissuing for the same nonce replaces and revokes the old one.
    let (mut s, first) = with_code(state());
    let (req, pending) = s
        .plan_code(NONCE, NOW + DAY_MS, NOW, [TAG + 1; 16])
        .unwrap();
    let second = s.apply_code(pending, answer(&req)).unwrap();
    assert_ne!(first, second);
    assert_eq!(s.codes.len(), 1);
    assert_eq!(s.revoke_outbox, vec![first]);
}

#[test]
fn token_bytes_round_trip_only_canonical_base64url() {
    let raw: Vec<u8> = (0..250u8).collect();
    let text = token_text(&raw);
    assert_eq!(token_bytes(&text), Some(raw));
    assert_eq!(token_bytes("not base64!"), None);
    // Non-canonical trailing bits are refused, so a ticket always carries the exact token.
    assert_eq!(token_bytes("AB"), None);
}

#[test]
fn old_state_without_codes_still_loads() {
    let mut v = serde_json::to_value(state()).unwrap();
    v.as_object_mut().unwrap().remove("codes");
    let back: WakeState = serde_json::from_value(v).unwrap();
    assert!(back.codes.is_empty());
}
