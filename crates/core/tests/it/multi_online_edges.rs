//! Several accounts on one device, the edges (`docs/multi-online.md`): tests 9 onward, after the
//! eight of `multi_online.rs`. Same device model and helpers (`support/multi.rs`).

use p2pcore::{Error, WakeOutcome};

use crate::support;
use support::multi::{BG_IDLE_MS, LINK_IDLE_MS, active, device, device3, incoming_with_account};
use support::*;

const TAG: &str = "multie";
const S: Suite = Suite(TAG);

/// 9. Declining, and separately hanging up, a call that rings background `b` leaves `a` open,
/// `b` in the background, and frees the call floor each time (the next call rings again).
#[test]
fn declining_or_hanging_up_a_background_ring_does_not_switch() {
    let c = S.peer("e9-c");
    let d = device(TAG, "e9", &[&c], &[], true);
    d.p.node.on_wake(d.b_call_hint.clone().unwrap()).unwrap();
    for how in ["decline", "hangup", "decline again"] {
        let call = c.node.call(d.b.clone()).unwrap();
        let (id, account) = incoming_with_account(&d.p, 40);
        assert_eq!((id.clone(), account), (call.call_id.clone(), Some(d.b.clone())), "{how}: rings b");
        if how == "hangup" {
            d.p.node.hangup(id.clone()).unwrap();
        } else {
            d.p.node.decline(id.clone()).unwrap();
        }
        expect(&c, 30, "caller sees the end", ended(&call.call_id));
        assert_eq!(d.open(), d.a, "{how}: a stays open");
        assert_eq!(d.background(), vec![d.b.clone()], "{how}: b stays in the background");
        assert!(d.p.node.current_call().is_none(), "{how}: nothing on a");
    }
}

/// 10. Declining a wake ring (before the call arrives) of background `b` does not switch either,
/// and the caller's call that follows is declined without ringing.
#[test]
fn declining_a_background_wake_ring_does_not_switch() {
    let c = S.peer("e10-c");
    let d = device(TAG, "e10", &[&c], &[], true);
    match d.p.node.on_wake(d.b_call_hint.clone().unwrap()).unwrap() {
        WakeOutcome::Ring { account, .. } => assert_eq!(account.map(|a| a.did), Some(d.b.clone())),
        other => panic!("expected a ring, got {other:?}"),
    }
    d.p.node.decline_wake_ring(c.did.clone()).unwrap();
    assert_eq!(d.open(), d.a, "a stays open");
    assert_eq!(d.background(), vec![d.b.clone()]);
    let call = c.node.call(d.b.clone()).unwrap();
    expect(&c, 30, "the call is declined", ended(&call.call_id));
    assert!(!saw_incoming(&d.p), "nothing rang");
    assert_eq!(d.open(), d.a);
}

/// 11. A call wake for background `b` while `a` (open) is in a call: `EndCall`, no ring, the
/// caller is told busy, `a`'s call goes on and nothing reaches `a`'s screens. (`b` is loaded
/// for the wake, as its endpoint must be up to answer `Busy`; it surfaces nothing and idles out.)
#[test]
fn call_wake_while_the_floor_is_held_ends_quietly() {
    let (c, c2) = (S.peer("e11-c"), S.peer("e11-c2"));
    let d = device(TAG, "e11", &[&c], &[&c2], true);
    let first = c2.node.call(d.a.clone()).unwrap();
    let (id, account) = incoming_with_account(&d.p, 40);
    assert_eq!((id.clone(), account), (first.call_id.clone(), None));
    d.p.node.answer(id.clone()).unwrap();
    active(&c2, &id);

    let outcome = d.p.node.on_wake(d.b_call_hint.clone().unwrap()).unwrap();
    assert_eq!(outcome, WakeOutcome::EndCall, "no ring while the call holds the floor");
    assert_eq!(d.acc.messages_for(&d.b).len(), 0);
    let second = c.node.call(d.b.clone()).unwrap();
    assert_eq!(expect(&c, 60, "b is busy", ended(&second.call_id)), "busy");
    assert!(!saw_incoming(&d.p), "no second incoming call on the device");
    assert_eq!(d.open(), d.a);
    assert_eq!(d.p.node.current_call().map(|i| i.call_id), Some(id.clone()), "a's call goes on");
    assert!(d.p.node.waiting_call().is_none());
    assert_eq!(d.screen.shown(&c.did), 0);
    d.p.node.hangup(id).unwrap();
}

/// 12. `create_identity` / `restore_identity` of an account that runs in the background (the
/// open one, or one woken) is `AccountExists`, and nothing running is touched. (`create` makes a
/// fresh phrase, so it cannot meet an existing account; `restore` is the way in.)
#[test]
fn restoring_a_background_account_is_account_exists() {
    let c = S.peer("e12-c");
    let d = device(TAG, "e12", &[&c], &[], true);
    let a_phrase = d.p.node.recovery_phrase(PASS.into()).unwrap();
    support::multi::message_while_asleep(&d, &c);
    assert_eq!(d.background(), vec![d.b.clone()]);
    // A blank screen: a goes to the background too (unlocked, on disk).
    d.p.node.begin_new_account().unwrap();
    assert_eq!(d.background().len(), 2);
    for (phrase, did) in [(d.b_phrase.clone(), &d.b), (a_phrase, &d.a)] {
        let r = d.p.node.restore_identity(phrase, "again".into(), PASS.into());
        assert!(matches!(r, Err(Error::AccountExists(ref x)) if x == did), "restore of a running account: {r:?}");
    }
    assert_eq!(d.background().len(), 2, "both still run");
    assert!(!d.p.node.has_identity(), "still the blank screen");
}

/// 13. Three accounts: `b` and `e` are woken at once; both deliver, the open `a`'s screens see
/// nothing, both count unread, and both idle out.
#[test]
fn two_background_accounts_woken_at_once_both_deliver_and_idle_out() {
    if !support::with_env(&[("P2P_IDLE_CLOSE_MS", LINK_IDLE_MS)]) {
        return;
    }
    let (c, c2) = (S.peer("e13-c"), S.peer("e13-c2"));
    let (d, e) = device3(TAG, "e13", &[&c], &[&c2], &[]);
    d.p.node.set_background_idle_ms_for_test(BG_IDLE_MS);
    let m1 = c.node.send_text(d.b.clone(), "to b".into(), None).unwrap();
    let m2 = c2.node.send_text(e.did.clone(), "to e".into(), None).unwrap();
    let w1 = d.p.node.on_wake(d.b_msg_hint.clone().unwrap()).unwrap();
    let w2 = d.p.node.on_wake(e.msg_hint.clone().unwrap()).unwrap();
    assert_eq!(w1, WakeOutcome::DialedBack { did: c.did.clone() });
    assert_eq!(w2, WakeOutcome::DialedBack { did: c2.did.clone() });
    let mut running = d.background();
    running.sort();
    let mut both = vec![d.b.clone(), e.did.clone()];
    both.sort();
    assert_eq!(running, both, "b and e run, a is open");
    eventually(60, "background message for b", || !d.acc.messages_for(&d.b).is_empty());
    eventually(60, "background message for e", || !d.acc.messages_for(&e.did).is_empty());
    assert_eq!(d.acc.messages_for(&d.b)[0].0, c.did);
    assert_eq!(d.acc.messages_for(&e.did)[0].0, c2.did);
    eventually(30, "delivered to b", || delivered(&c, &d.b, &m1.id));
    eventually(30, "delivered to e", || delivered(&c2, &e.did, &m2.id));
    eventually(30, "unread counted", || d.unread(&d.b) == 1 && d.unread(&e.did) == 1);
    assert_eq!((d.screen.shown(&c.did), d.screen.shown(&c2.did)), (0, 0), "the open account's screens hear nothing");
    assert_eq!(d.open(), d.a);
    assert_eq!(d.unread(&d.a), 0);
    eventually(60, "both idle out", || d.background().is_empty());
    assert_eq!((d.unread(&d.b), d.unread(&e.did)), (1, 1), "unread kept after closing");
}

/// 14. A lock forgets the hint keys: the next wake for `b` asks the platform for its key again.
#[test]
fn hint_keys_are_asked_for_again_after_a_lock() {
    let c = S.peer("e14-c");
    let d = device(TAG, "e14", &[&c], &[], true);
    support::multi::message_while_asleep(&d, &c);
    let asked = d.acc.asked.load(std::sync::atomic::Ordering::SeqCst);
    assert!(asked >= 1);
    d.p.node.lock();
    assert_eq!(d.p.node.hint_keys_for_test(), 0);
    d.p.node.unlock_with_key(d.a_key.clone()).unwrap();
    let outcome = d.p.node.on_wake(d.b_msg_hint.clone().unwrap()).unwrap();
    assert!(matches!(outcome, WakeOutcome::DialedBack { .. }), "{outcome:?}");
    assert!(d.acc.asked.load(std::sync::atomic::Ordering::SeqCst) > asked, "key asked for again");
    assert_eq!(d.p.node.hint_keys_for_test(), 1);
}

/// 15. Switching the open account during a 1:1 call is refused (`InCall`, as documented): to a
/// sleeping account, to a background one, and to a blank screen; the call and the open
/// account are untouched.
#[test]
fn switching_the_open_account_during_a_call_is_refused() {
    let (c, c2) = (S.peer("e15-c"), S.peer("e15-c2"));
    let d = device(TAG, "e15", &[&c], &[&c2], true);
    let call = c2.node.call(d.a.clone()).unwrap();
    let (id, _) = incoming_with_account(&d.p, 40);
    assert_eq!(id, call.call_id);
    d.p.node.answer(id.clone()).unwrap();
    active(&c2, &id);
    // b asleep.
    assert!(matches!(d.p.node.switch_account(d.b.clone()), Err(Error::InCall)));
    // b woken (running in the background).
    support::multi::message_while_asleep(&d, &c);
    assert_eq!(d.background(), vec![d.b.clone()]);
    assert!(matches!(d.p.node.switch_account(d.b.clone()), Err(Error::InCall)));
    assert!(matches!(d.p.node.begin_new_account(), Err(Error::InCall)));
    assert_eq!(d.open(), d.a);
    assert_eq!(d.background(), vec![d.b.clone()], "b not promoted");
    assert_eq!(d.p.node.current_call().map(|i| i.call_id), Some(id.clone()), "the call goes on");
    d.p.node.hangup(id.clone()).unwrap();
    expect(&c2, 30, "call ended", ended(&call.call_id));
    // Free again: the switch now works.
    d.p.node.switch_account(d.b.clone()).unwrap();
    assert_eq!(d.open(), d.b);
}
