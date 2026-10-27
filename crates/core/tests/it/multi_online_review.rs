//! Several accounts on one device, the review fixes (`docs/multi-online.md` "Fixed after
//! review"): tests 16 onward, after `multi_online.rs` and `multi_online_edges.rs`. Same device
//! model and helpers (`support/multi.rs`).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use p2pcore::{BackgroundMode, Error, LockState, WakeOutcome};

use crate::support;
use support::multi::{
    Accounts, BG_IDLE_MS, LINK_IDLE_MS, befriend, device, device3, incoming_with_account, message_while_asleep, never_active, returns_within,
};
use support::*;

const TAG: &str = "multir";
const S: Suite = Suite(TAG);

/// 16. Locked means every account stays shut (decided 2026-10-27, option a): after `lock` a
/// wake for `b` is `Locked` as on one account (no key asked, nothing loaded), and a `KeepAll`
/// start opens nothing; once `a` is unlocked, `b` runs again.
#[test]
fn lock_keeps_every_account_shut_until_unlocked() {
    let c = S.peer("r16-c");
    let d = device(TAG, "r16", &[&c], &[], true);
    message_while_asleep(&d, &c);
    assert_eq!(d.background(), vec![d.b.clone()]);
    d.p.node.lock();
    assert!(d.background().is_empty());
    let asked = d.acc.asked.load(Ordering::SeqCst);
    let r = d.p.node.on_wake(d.b_msg_hint.clone().unwrap());
    assert!(matches!(r, Err(Error::Locked)), "the locked path, as on one account: {r:?}");
    d.p.node.set_background_mode(BackgroundMode::KeepAll);
    let _ = d.p.node.start();
    assert!(d.background().is_empty(), "nothing opened while locked");
    assert!(d.p.node.account_endpoint_for_test(d.b.clone()).is_none());
    assert_eq!(d.acc.asked.load(Ordering::SeqCst), asked, "no key asked for while locked");
    assert_eq!(d.p.node.hint_keys_for_test(), 0);

    d.p.node.unlock_with_key(d.a_key.clone()).unwrap();
    d.p.node.start().unwrap();
    assert_eq!(d.background(), vec![d.b.clone()], "unlocked: KeepAll opens b again");
}

/// 17. Answering a wake ring for `b` switches to `b`; switching back to `a` before the call
/// arrives takes the early answer back: `b`'s call then rings, naming `b`, and does not connect
/// where nobody hears it.
#[test]
fn an_early_answer_does_not_follow_the_user_to_another_account() {
    let c = S.peer("r17-c");
    let d = device(TAG, "r17", &[&c], &[], true);
    assert!(matches!(d.p.node.on_wake(d.b_call_hint.clone().unwrap()).unwrap(), WakeOutcome::Ring { .. }));
    d.p.node.answer_wake_ring(c.did.clone()).unwrap();
    assert_eq!(d.open(), d.b, "answering switched to b");
    d.p.node.switch_account(d.a.clone()).unwrap();
    assert_eq!(d.open(), d.a);
    let call = c.node.call(d.b.clone()).unwrap();
    let (id, account) = incoming_with_account(&d.p, 40);
    assert_eq!((id.clone(), account), (call.call_id.clone(), Some(d.b.clone())), "it rings b");
    never_active(&c, &call.call_id, 4);
    assert!(d.p.node.current_call().is_none(), "nothing on a");
    d.p.node.decline(id).unwrap();
    assert_eq!(expect(&c, 30, "declined", ended(&call.call_id)), "declined");
}

/// 18. One ring across the accounts: while `b` rings for a wake, a call wake for `e` is
/// `EndCall`, and `e`'s call is busy (it never rings on the device).
#[test]
fn a_call_wake_while_another_account_rings_ends_quietly() {
    let (c, c2) = (S.peer("r18-c"), S.peer("r18-c2"));
    let (d, e) = device3(TAG, "r18", &[&c], &[&c2], &[]);
    match d.p.node.on_wake(d.b_call_hint.clone().unwrap()).unwrap() {
        WakeOutcome::Ring { account, .. } => assert_eq!(account.map(|a| a.did), Some(d.b.clone())),
        other => panic!("expected b to ring, got {other:?}"),
    }
    assert_eq!(d.p.node.on_wake(e.call_hint.clone().unwrap()).unwrap(), WakeOutcome::EndCall, "a second ring");
    let call = c2.node.call(e.did.clone()).unwrap();
    assert_eq!(expect(&c2, 60, "e is busy", ended(&call.call_id)), "busy");
    assert!(!saw_incoming(&d.p), "nothing rang for e");
    assert_eq!(d.open(), d.a);
}

/// 19. A wake for `b` while `b`'s slot is still closing (idled out) waits for the close, then
/// loads `b` once and delivers: no second slot over the same stores.
#[test]
fn a_wake_while_the_account_closes_waits_for_the_close() {
    if !support::with_env(&[("P2P_IDLE_CLOSE_MS", LINK_IDLE_MS)]) {
        return;
    }
    let c = S.peer("r19-c");
    let d = device(TAG, "r19", &[&c], &[], true);
    d.p.node.set_background_idle_ms_for_test(BG_IDLE_MS);
    let first = message_while_asleep(&d, &c);
    eventually(30, "delivered to b", || delivered(&c, &d.b, &first));
    d.p.node.set_close_delay_for_test(2_000);
    eventually(60, "b idles out (its close now takes 2 s)", || d.background().is_empty());
    let second = message_while_asleep(&d, &c);
    assert_eq!(d.background(), vec![d.b.clone()], "b runs again, once");
    d.p.node.set_close_delay_for_test(0);
    eventually(60, "the second message reaches b", || delivered(&c, &d.b, &second));
    eventually(60, "background message for it", || d.acc.messages_for(&d.b).len() >= 2);
}

/// 20. A platform that calls back into the node from inside a callback (locks from
/// `account_key`, locks from `on_accounts_changed` during a switch) does not deadlock it.
#[test]
fn a_platform_calling_back_from_a_callback_does_not_deadlock() {
    let c = S.peer("r20-c");
    let d = device(TAG, "r20", &[&c], &[], true);
    let node = Arc::downgrade(&d.p.node);
    *d.acc.on_key.lock().unwrap() = Some(Box::new(move || {
        if let Some(n) = node.upgrade() {
            n.lock();
        }
    }));
    let (n, hint) = (d.p.node.clone(), d.b_msg_hint.clone().unwrap());
    let outcome = returns_within(60, "on_wake", move || n.on_wake(hint));
    assert_eq!(outcome.unwrap(), WakeOutcome::Ignore, "locked meanwhile: b is not opened");
    assert!(d.background().is_empty());
    assert_eq!(d.p.node.lock_state(), LockState::Locked);

    d.p.node.unlock_with_key(d.a_key.clone()).unwrap();
    let node = Arc::downgrade(&d.p.node);
    *d.acc.on_changed.lock().unwrap() = Some(Box::new(move || {
        if let Some(n) = node.upgrade() {
            n.lock();
        }
    }));
    let (n, b) = (d.p.node.clone(), d.b.clone());
    returns_within(60, "switch_account", move || n.switch_account(b)).unwrap();
    assert_eq!(d.open(), d.b);
    assert_eq!(d.p.node.lock_state(), LockState::Locked);
    assert!(d.background().is_empty(), "the lock closed a, which the switch had left running");
}

/// 21. Removing an account that ran in the background forgets its hint key too.
#[test]
fn removing_a_background_account_forgets_its_hint_key() {
    let c = S.peer("r21-c");
    let d = device(TAG, "r21", &[&c], &[], true);
    message_while_asleep(&d, &c);
    assert_eq!(d.p.node.hint_keys_for_test(), 1);
    d.p.node.remove_account(d.b.clone()).unwrap();
    assert!(d.background().is_empty());
    assert_eq!(d.p.node.hint_keys_for_test(), 0, "b's hint key went with it");
}

/// 22. Under `KeepAll`, a message that `b` already read on its other device raises no notice
/// when it is synced in as `b` comes up in the background; a new message does.
#[test]
fn keep_all_does_not_notify_what_another_device_already_read() {
    let c = S.peer("r22-c");
    let p = Peer::blank(TAG, "r22-dev");
    p.node.create_identity("Bea".into(), PASS.into()).unwrap();
    let b = p.node.profile().unwrap().did;
    let b_key = p.node.unlock_key().unwrap();
    online(&p.node);
    befriend(&c, &p, &b);
    let b2 = S.link_fresh(&p, "r22-b2");
    // `b` goes to sleep (still `OpenOnly`); `a` opens.
    p.node.begin_new_account().unwrap();
    p.node.create_identity("Ada".into(), PASS.into()).unwrap();
    online(&p.node);
    let acc = Arc::new(Accounts::default());
    acc.remember(&b, b_key);
    p.node.set_account_events(acc.clone());

    let old = c.node.send_text(b.clone(), "read elsewhere".into(), None).unwrap();
    eventually(60, "b2 has it", || holds(&b2, &c.did, &old.id));
    b2.node.mark_read(c.did.clone()).unwrap();
    c.node.stop();

    p.node.set_background_mode(BackgroundMode::KeepAll);
    p.node.start().unwrap();
    assert_eq!(p.node.background_accounts_for_test(), vec![b.clone()]);
    // b syncs with b2 now: the old message arrives already read.
    std::thread::sleep(std::time::Duration::from_secs(8));
    online(&c.node);
    let new = c.node.send_text(b.clone(), "new one".into(), None).unwrap();
    eventually(60, "a notice for the new message", || acc.messages_for(&b).len() == 1);
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert_eq!(acc.messages_for(&b), vec![(c.did.clone(), "r22-c".to_string())], "one notice: the new message only");

    p.node.switch_account(b.clone()).unwrap();
    assert!(holds(&p, &c.did, &old.id), "the read message did reach b here");
    assert!(holds(&p, &c.did, &new.id));
    let chat = p.node.chats().unwrap().into_iter().find(|ch| ch.peer_did == c.did).expect("chat with c");
    assert_eq!(chat.unread, 1, "only the new message is unread");
}
