//! Several accounts reachable on one device (`docs/multi-online.md`, tests 1-8 of its "Tests").
//!
//! A device holds two accounts: `a` (open) and `b` (asleep, or running in the background). Wake
//! pushes are simulated with the hint the relay would send (`wake_hint_for_test`); there is no
//! relay. Real iroh endpoints (needs network, like `calls.rs`). Idle times are shortened with
//! `set_background_idle_ms_for_test` and, where a chat session must close first, with
//! `P2P_IDLE_CLOSE_MS` in an isolated child (`with_env`).

use std::sync::atomic::Ordering;

use p2pcore::{BackgroundMode, LockState, WakeOutcome};

use crate::support;
use support::multi::{BG_IDLE_MS, LINK_IDLE_MS, active, device, incoming_with_account, message_while_asleep};
use support::*;

const TAG: &str = "multi";
const S: Suite = Suite(TAG);

/// 1. A message wake for sleeping `b`: `b` loads, dials back, the message lands in `b`'s store
/// and `on_background_message(b)` fires; `a`'s screens see nothing; `b` closes once idle.
#[test]
fn wake_for_a_sleeping_account_delivers_in_the_background() {
    if !support::with_env(&[("P2P_IDLE_CLOSE_MS", LINK_IDLE_MS)]) {
        return;
    }
    let c = S.peer("w1-c");
    let d = device(TAG, "w1", &[&c], &[], true);
    d.p.node.set_background_idle_ms_for_test(BG_IDLE_MS);
    assert!(d.background().is_empty(), "b asleep before the wake");
    let id = message_while_asleep(&d, &c);
    assert_eq!(d.background(), vec![d.b.clone()], "b runs in the background");
    eventually(60, "background message for b", || !d.acc.messages_for(&d.b).is_empty());
    assert_eq!(d.acc.messages_for(&d.b)[0], (c.did.clone(), "w1-c".to_string()));
    eventually(30, "delivered to b", || delivered(&c, &d.b, &id));
    assert_eq!(d.screen.shown(&c.did), 0, "the open account's screens hear nothing");
    assert_eq!(d.open(), d.a, "a stays open");
    eventually(60, "b closes once quiet", || d.background().is_empty());
    assert!(d.p.node.account_endpoint_for_test(d.b.clone()).is_none(), "b's slot is gone");
}

/// 2. A call wake for `b` rings naming `b`; answering switches to `b` (a goes to the
/// background) and the call connects.
#[test]
fn call_wake_rings_the_account_and_answering_switches_to_it() {
    let c = S.peer("w2-c");
    let d = device(TAG, "w2", &[&c], &[], true);
    match d.p.node.on_wake(d.b_call_hint.clone().unwrap()).unwrap() {
        WakeOutcome::Ring { did, account, .. } => {
            assert_eq!(did, c.did);
            assert_eq!(account.map(|a| a.did), Some(d.b.clone()), "the ring names b");
        }
        other => panic!("expected a ring, got {other:?}"),
    }
    let call = c.node.call(d.b.clone()).unwrap();
    let (id, account) = incoming_with_account(&d.p, 40);
    assert_eq!(id, call.call_id);
    assert_eq!(account, Some(d.b.clone()), "the incoming call names b");
    d.p.node.answer(id.clone()).unwrap();
    assert_eq!(d.open(), d.b, "answering opened b");
    assert_eq!(d.background(), vec![d.a.clone()], "a went to the background");
    active(&c, &id);
    assert_eq!(d.p.node.current_call().map(|i| i.call_id), Some(id.clone()));
    d.p.node.hangup(id).unwrap();
}

/// 3. One 1:1 call across the accounts: `b` (open) in a call, a call to `a` (background) is
/// busy; and the reverse.
#[test]
fn a_call_on_one_account_makes_the_other_busy() {
    let (c, c2) = (S.peer("w3-c"), S.peer("w3-c2"));
    let d = device(TAG, "w3", &[&c], &[&c2], true);
    // b opens; a stays up in the background.
    d.switch(&d.b);
    online(&d.p.node);
    assert_eq!(d.background(), vec![d.a.clone()]);

    let first = c.node.call(d.b.clone()).unwrap();
    let (id, account) = incoming_with_account(&d.p, 40);
    assert_eq!((id.clone(), account), (first.call_id.clone(), None));
    d.p.node.answer(id.clone()).unwrap();
    active(&c, &id);
    let second = c2.node.call(d.a.clone()).unwrap();
    assert_eq!(expect(&c2, 60, "a is busy", ended(&second.call_id)), "busy");
    d.p.node.hangup(id).unwrap();
    expect(&d.p, 10, "first call ended", ended(&first.call_id));

    // Back to a; b stays up in the background.
    d.switch(&d.a);
    assert_eq!(d.background(), vec![d.b.clone()]);
    let third = c2.node.call(d.a.clone()).unwrap();
    let (id, _) = incoming_with_account(&d.p, 40);
    assert_eq!(id, third.call_id);
    d.p.node.answer(id.clone()).unwrap();
    active(&c2, &id);
    let fourth = c.node.call(d.b.clone()).unwrap();
    assert_eq!(expect(&c, 60, "b is busy", ended(&fourth.call_id)), "busy");
    assert!(d.p.node.call(c2.did.clone()).is_err(), "no second outgoing call either");
    d.p.node.hangup(id).unwrap();
}

/// 4. A wake for a passphrase-only account (no remembered key): `Ignore`, nothing loaded.
#[test]
fn wake_for_an_account_without_a_key_is_ignored() {
    let c = S.peer("w4-c");
    let d = device(TAG, "w4", &[&c], &[], false);
    let outcome = d.p.node.on_wake(d.b_msg_hint.clone().unwrap()).unwrap();
    assert_eq!(outcome, WakeOutcome::Ignore);
    assert!(d.background().is_empty(), "nothing loaded");
    assert!(d.p.node.account_endpoint_for_test(d.b.clone()).is_none());
    assert_eq!(d.p.node.hint_keys_for_test(), 0);
    assert_eq!(d.open(), d.a);
}

/// 5. Switching to a background account restarts nothing; the account left idles out.
#[test]
fn switching_to_a_background_account_keeps_its_endpoint() {
    if !support::with_env(&[("P2P_IDLE_CLOSE_MS", LINK_IDLE_MS)]) {
        return;
    }
    let d = device(TAG, "w5", &[], &[], true);
    d.switch(&d.b);
    online(&d.p.node);
    // a went to the background with its endpoint up.
    let before = d.p.node.account_endpoint_for_test(d.a.clone()).expect("a runs");
    assert!(before.0, "a's endpoint is up");
    d.p.node.switch_account(d.a.clone()).unwrap();
    assert_eq!(d.p.node.lock_state(), LockState::Unlocked, "a opened unlocked");
    assert_eq!(d.p.node.account_endpoint_for_test(d.a.clone()), Some(before), "no endpoint restart");
    assert!(d.p.node.status().started);
    assert_eq!(d.background(), vec![d.b.clone()]);
    d.p.node.set_background_idle_ms_for_test(BG_IDLE_MS);
    eventually(60, "b idles out", || d.background().is_empty());
    assert!(d.p.node.account_endpoint_for_test(d.b.clone()).is_none(), "b closed");
}

/// 6. `lock` with a background account closes both and forgets the hint keys.
#[test]
fn lock_closes_every_account_and_forgets_hint_keys() {
    let c = S.peer("w6-c");
    let d = device(TAG, "w6", &[&c], &[], true);
    message_while_asleep(&d, &c);
    assert_eq!(d.background(), vec![d.b.clone()]);
    assert_eq!(d.p.node.hint_keys_for_test(), 1, "b's hint key is remembered");
    d.p.node.lock();
    assert!(d.background().is_empty(), "b closed");
    assert!(d.p.node.account_endpoint_for_test(d.b.clone()).is_none());
    assert_eq!(d.p.node.hint_keys_for_test(), 0, "hint keys forgotten");
    assert_eq!(d.p.node.lock_state(), LockState::Locked);
    assert!(!d.p.node.status().started);
}

/// 7. Unread: a message in background `b` counts 1, kept after `b` closes; opening `b` clears
/// the switcher's count and the conversation's own read cursor shows it unread.
#[test]
fn unread_of_a_background_account_survives_it_closing() {
    if !support::with_env(&[("P2P_IDLE_CLOSE_MS", LINK_IDLE_MS)]) {
        return;
    }
    let c = S.peer("w7-c");
    let d = device(TAG, "w7", &[&c], &[], true);
    d.p.node.set_background_idle_ms_for_test(BG_IDLE_MS);
    let changed = d.acc.changed.load(Ordering::SeqCst);
    let id = message_while_asleep(&d, &c);
    eventually(60, "b counts one unread", || d.unread(&d.b) == 1);
    assert!(d.acc.changed.load(Ordering::SeqCst) > changed, "the switcher was told");
    assert_eq!(d.unread(&d.a), 0, "the open account shows none");
    eventually(60, "b closes", || d.background().is_empty());
    assert_eq!(d.unread(&d.b), 1, "kept after b closed");
    d.switch(&d.b);
    assert_eq!(d.unread(&d.b), 0, "the open account shows none");
    assert!(holds(&d.p, &c.did, &id), "the message is in b's store");
    let chat = d.p.node.chats().unwrap().into_iter().find(|ch| ch.peer_did == c.did).expect("chat with c");
    assert_eq!(chat.unread, 1, "still unread in the conversation");
    // a went to the background on the switch and has nothing unread.
    assert_eq!(d.unread(&d.a), 0);
}

/// 8. `KeepAll`: both accounts up after `start`, neither idles out, a message to the background
/// one arrives without a wake. `OpenOnly`: a wake for another account is ignored.
#[test]
fn keep_all_runs_every_account_and_open_only_none() {
    let c = S.peer("w8-c");
    let d = device(TAG, "w8", &[&c], &[], true);
    d.p.node.set_background_idle_ms_for_test(BG_IDLE_MS);
    d.p.node.set_background_mode(BackgroundMode::KeepAll);
    d.p.node.start().unwrap();
    assert_eq!(d.background(), vec![d.b.clone()], "b opened at start");
    eventually(20, "b's endpoint up", || d.p.node.account_endpoint_for_test(d.b.clone()).is_some_and(|e| e.0));
    std::thread::sleep(std::time::Duration::from_millis(BG_IDLE_MS * 3));
    assert_eq!(d.background(), vec![d.b.clone()], "no idle-out under KeepAll");
    let m = c.node.send_text(d.b.clone(), "no wake needed".into(), None).unwrap();
    eventually(60, "delivered to b without a wake", || delivered(&c, &d.b, &m.id));
    eventually(10, "background message for b", || !d.acc.messages_for(&d.b).is_empty());
    assert_eq!(d.screen.shown(&c.did), 0);

    d.p.node.set_background_mode(BackgroundMode::OpenOnly);
    assert!(d.background().is_empty(), "OpenOnly closes the background accounts");
    let asked = d.acc.asked.load(Ordering::SeqCst);
    assert_eq!(d.p.node.on_wake(d.b_msg_hint.clone().unwrap()).unwrap(), WakeOutcome::Ignore);
    assert!(d.background().is_empty(), "nothing loaded");
    assert_eq!(d.acc.asked.load(Ordering::SeqCst), asked, "no key asked for");
}
