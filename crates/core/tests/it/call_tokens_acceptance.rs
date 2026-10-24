//! P6 acceptance: separate `message` and `call` tokens (decisions U13, U15; section 14 of
//! `docs/design/rotation-open-decisions.md`), on real nodes over real iroh endpoints (needs
//! network, like `calls.rs`).
//!
//! - Ticket contacts get message + call; group chat-request contacts message only (U13). There
//!   is no group code yet, so the group origin is driven through `add_contact_as_for_test`.
//! - A message-only contact chats; its call ends with "calls not allowed" and never rings
//!   (PROVISIONAL (coordinator): the reason is shown to every authenticated contact).
//! - "Allow calls" sends a call token; "disallow calls" takes every call token back (per-DID
//!   cut-off) and leaves chat alone.
//! - A block takes back both; an unblock does not revive them (U20: the other side re-adds us
//!   and the re-add carries fresh tokens).
//!
//! Every test carries `// KIND: acceptance` and the decision it enforces.

use std::time::{Duration, Instant};

use p2pcore::{CALLS_NOT_ALLOWED, ContactOrigin};

use crate::support;
use support::eventually_200ms as eventually;
use support::*;

const S: Suite = Suite("p6");
/// A refusal must be quick: well under any dial timeout.
const QUICK: Duration = Duration::from_secs(5);

/// `from` calls `to`; `to` rings; `from` hangs up and both settle.
fn rings(from: &Peer, to: &Peer) {
    let call = from.node.call(to.did.clone()).unwrap();
    assert_eq!(incoming(to, 30), call.call_id, "the call rings");
    from.node.hangup(call.call_id.clone()).unwrap();
    ended_after(from, &call.call_id, Instant::now(), 30);
    std::thread::sleep(Duration::from_millis(500));
    let _ = (from.rx.try_iter().count(), to.rx.try_iter().count());
}

/// `from` calls `to` and is refused with "calls not allowed", quickly, and `to` never rings.
fn refused_calls_not_allowed(from: &Peer, to: &Peer) {
    let t0 = Instant::now();
    let call = from.node.call(to.did.clone()).unwrap();
    let (reason, took) = ended_after(from, &call.call_id, t0, 40);
    assert_eq!(reason, CALLS_NOT_ALLOWED);
    assert!(took < QUICK, "took {took:?}");
    std::thread::sleep(Duration::from_millis(500));
    assert!(!saw_incoming(to), "a message-only contact must not ring us");
}

fn message_arrives(from: &Peer, to: &Peer, text: &str) {
    let m = from.node.send_text(to.did.clone(), text.into(), None).unwrap().id;
    eventually(60, "message delivered", || holds(to, &from.did, &m) && delivered(from, &to.did, &m));
}

// KIND: acceptance
// Decision: U13 default — a ticket contact holds message + call both ways: chat and calls work
// in both directions, and each side reports calls allowed / can call.
#[test]
fn ticket_contacts_can_chat_and_call_both_ways() {
    let (a, b) = S.pair("tkt");
    for (x, y) in [(&a, &b), (&b, &a)] {
        assert!(x.node.calls_allowed(y.did.clone()), "ticket contacts may call");
        assert!(x.node.can_call(y.did.clone()), "ticket contacts hold a call token");
        let (m, c) = x.node.held_token_ids_for_test(y.did.clone());
        assert!(m.is_some() && c.is_some() && m != c, "two separate tokens");
    }
    message_arrives(&b, &a, "hi a");
    message_arrives(&a, &b, "hi b");
    rings(&b, &a);
    rings(&a, &b);
}

// KIND: acceptance
// Decision: U13 default (group chat-request contacts are message-only) + section 14 Q6
// PROVISIONAL (show "calls not allowed") — the message-only contact chats, its call ends at once
// with "calls not allowed" and never rings; it is not told "blocked".
#[test]
fn a_message_only_contact_chats_but_its_call_is_refused() {
    let a = S.peer("mo-a");
    let b = S.peer("mo-b");
    // b came to a through a group chat request: b gives a message only.
    b.node.add_contact_as_for_test(a.node.my_ticket().unwrap(), ContactOrigin::GroupRequest).unwrap();
    assert!(!b.node.calls_allowed(a.did.clone()));
    assert!(!a.node.can_call(b.did.clone()), "a holds no call token from b");
    message_arrives(&a, &b, "message-only works");
    message_arrives(&b, &a, "and back");
    refused_calls_not_allowed(&a, &b);
    assert!(b.logged("incoming call refused: calls not allowed"));
    // b may still call a (a gave b the ticket default).
    rings(&b, &a);
}

// KIND: acceptance
// Decision: U13 ("allow calls" sends the call token) — the upgrade lets calls through.
#[test]
fn allow_calls_upgrades_a_message_only_contact() {
    let a = S.peer("up-a");
    let b = S.peer("up-b");
    b.node.add_contact_as_for_test(a.node.my_ticket().unwrap(), ContactOrigin::GroupRequest).unwrap();
    refused_calls_not_allowed(&a, &b);
    b.node.allow_calls(a.did.clone(), true).unwrap();
    assert!(b.node.calls_allowed(a.did.clone()));
    eventually(40, "a receives b's call token", || a.node.can_call(b.did.clone()));
    rings(&a, &b);
    message_arrives(&a, &b, "chat unchanged");
}

// KIND: acceptance
// Decision: U13 + U15 (separate tokens: a downgrade takes the call token only) — "disallow
// calls" stops their calls at once; chat goes on; our own calls to them are unaffected.
#[test]
fn disallow_calls_stops_calls_but_not_chat() {
    let (a, b) = S.pair("down");
    rings(&a, &b);
    b.node.allow_calls(a.did.clone(), false).unwrap();
    assert!(!b.node.calls_allowed(a.did.clone()));
    refused_calls_not_allowed(&a, &b);
    assert!(!a.node.can_call(b.did.clone()), "the refused caller drops its call token");
    message_arrives(&a, &b, "still chatting");
    message_arrives(&b, &a, "yes");
    rings(&b, &a);
    // And back up again.
    b.node.allow_calls(a.did.clone(), true).unwrap();
    eventually(40, "a gets a fresh call token", || a.node.can_call(b.did.clone()));
    rings(&a, &b);
}

// KIND: acceptance
// Decision: U13 (blocking revokes both tokens) + U20 (unblock does not revive them: the other
// side re-adds and gets fresh tokens) — b acts like a modified app that ignores the notice, so it
// keeps a's old tokens; after the unblock they open nothing; the re-add issues fresh ones.
#[test]
fn block_revokes_both_tokens_and_unblock_does_not_revive_them() {
    let (a, b) = S.pair("rev");
    b.node.ignore_block_notices_for_test(true);
    let old = b.node.held_token_ids_for_test(a.did.clone());
    a.node.block(b.did.clone()).unwrap();
    a.node.unblock(b.did.clone()).unwrap();
    assert_eq!(b.node.held_token_ids_for_test(a.did.clone()), old, "the modified app kept the old tokens");

    // Old call token: refused (generic: it was taken back), never rings.
    let t0 = Instant::now();
    let call = b.node.call(a.did.clone()).unwrap();
    let (reason, took) = ended_after(&b, &call.call_id, t0, 40);
    assert_ne!(reason, CALLS_NOT_ALLOWED, "a revoked token is not a permission question");
    assert!(took < Duration::from_secs(35), "{reason} after {took:?}");
    std::thread::sleep(Duration::from_millis(500));
    assert!(!saw_incoming(&a), "a revoked call token must not ring");
    // Old message token: chat refused.
    let m = b.node.send_text(a.did.clone(), "with an old token".into(), None).unwrap().id;
    std::thread::sleep(Duration::from_secs(8));
    assert!(!holds(&a, &b.did, &m), "a revoked message token must not open chat");
    assert!(a.logged("no valid message token"));

    // Re-add: fresh tokens, and everything works again.
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let fresh = b.node.held_token_ids_for_test(a.did.clone());
    assert!(fresh.0.is_some() && fresh.1.is_some());
    assert_ne!(fresh.0, old.0, "a fresh message token");
    assert_ne!(fresh.1, old.1, "a fresh call token");
    rings(&b, &a);
    message_arrives(&b, &a, "after the re-add");
}

// KIND: acceptance
// Decision: U20 + U13 — with the shipped app: block, the blocked app deletes our entry; unblock;
// they re-add and get fresh message + call tokens; a contact that was message-only before the
// block stays message-only after its re-add (and is not told "blocked").
#[test]
fn unblock_and_readd_issues_fresh_tokens_and_keeps_call_permission() {
    let (a, b) = S.pair("readd");
    let c = S.peer("readd-c");
    c.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    a.node.allow_calls(c.did.clone(), false).unwrap();
    let old_b = b.node.held_token_ids_for_test(a.did.clone());
    for p in [&b, &c] {
        a.node.block(p.did.clone()).unwrap();
        eventually(40, "their app deleted our entry", || !p.node.contacts().iter().any(|x| x.did == a.did));
        a.node.unblock(p.did.clone()).unwrap();
        eventually(40, "the unblock reached them", || p.node.block_state(a.did.clone()).blocked_me.is_none());
        p.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    }
    let new_b = b.node.held_token_ids_for_test(a.did.clone());
    assert!(new_b.1.is_some());
    assert_ne!(new_b.0, old_b.0);
    assert_ne!(new_b.1, old_b.1);
    assert!(a.node.calls_allowed(b.did.clone()));
    assert!(!a.node.calls_allowed(c.did.clone()), "message-only stays message-only");
    rings(&b, &a);
    refused_calls_not_allowed(&c, &a);
    message_arrives(&c, &a, "messages still work");
    message_arrives(&b, &a, "and from b");
}

// KIND: acceptance
// Decision: U13 + section 14 Q4 (best effort) — "disallow calls" made on one device (N) holds on
// its linked sibling (E) once the account doc has synced: A's call to E (the only device A
// knows) is refused with "calls not allowed" and rings neither device.
#[test]
fn a_downgrade_holds_on_the_linked_sibling() {
    let a = S.peer("sib-a");
    let e = S.peer("sib-e");
    e.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let n = S.link_fresh(&e, "sib-n");
    eventually(40, "N has A", || n.node.contacts().iter().any(|c| c.did == a.did));
    assert!(n.node.calls_allowed(a.did.clone()));
    n.node.allow_calls(a.did.clone(), false).unwrap();
    eventually(40, "E holds N's downgrade", || !e.node.calls_allowed(a.did.clone()));
    let _ = (e.rx.try_iter().count(), n.rx.try_iter().count());
    let t0 = Instant::now();
    let call = a.node.call(e.did.clone()).unwrap();
    let (reason, _) = ended_after(&a, &call.call_id, t0, 60);
    assert_eq!(reason, CALLS_NOT_ALLOWED);
    std::thread::sleep(Duration::from_millis(500));
    assert!(!saw_incoming(&e) && !saw_incoming(&n), "neither own device may ring");
}
