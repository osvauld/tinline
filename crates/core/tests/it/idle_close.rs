//! Sessions are not held open, and links are consent-based (decision 2026-10-17): a chat or
//! own-device session with no traffic for the idle time closes cleanly, unless this side has its
//! own reason (the user typed or wrote to that contact within the idle time, something is
//! pending, or a 1:1 call is live); whoever has something to send redials. Typing dials ahead;
//! opening a conversation does not. Presence is sent only while active with that contact.
//!
//! Real iroh endpoints (needs network, like `link.rs`). The idle time is shortened for the whole
//! isolated child with `P2P_IDLE_CLOSE_MS` (the production value is 60 s, `wire::IDLE_CLOSE`).
//! Core only: not an Android or desktop UI validation.

use std::time::Duration;

use crate::support;
use support::*;

const TAG: &str = "idle";
/// The idle time in each isolated test process.
const IDLE_MS: u64 = 2_000;

fn sessions(p: &Peer, did: &str) -> u32 {
    p.node.chat_sessions_for_test(did.to_string())
}

fn both_closed(a: &Peer, b: &Peer) -> bool {
    sessions(a, &b.did) == 0 && sessions(b, &a.did) == 0
}

/// How many chat sessions with `did` this peer has had (dialled or accepted).
fn opened(p: &Peer, did: &str) -> usize {
    let needle = format!("chat session with {did} (");
    p.logs
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.contains(&needle))
        .count()
}

fn idle_ended(p: &Peer, did: &str) -> bool {
    idle_ends(p, did) > 0
}

/// How many chat sessions with `did` this peer closed for idleness.
fn idle_ends(p: &Peer, did: &str) -> usize {
    let needle = format!("chat session with {did} ended: idle");
    p.logs.lock().unwrap().iter().filter(|l| l.contains(&needle)).count()
}

/// A pair whose first sessions (contact exchange, profiles, receipts) have settled and closed.
fn settled_pair(tag: &str) -> (Peer, Peer) {
    let (a, b) = Peer::pair(TAG, tag);
    eventually(60, "contacts both ways", || {
        a.node.contacts().iter().any(|c| c.did == b.did)
            && b.node.contacts().iter().any(|c| c.did == a.did)
    });
    // The exchange that follows adding a contact (profiles, receipts) dials too: wait until
    // both sides have been without a session for a while.
    eventually(90, "first sessions closed", || {
        !within(3, || !both_closed(&a, &b))
    });
    (a, b)
}

// KIND: acceptance (user 2026-10-17: a session closes after the idle time with nothing pending
// and no recent activity; the peer sees a normal close and neither side redials because of it)
#[test]
fn a_message_dials_a_session_that_closes_when_idle_and_nobody_redials() {
    if !support::with_env(&[
        ("P2P_IDLE_CLOSE_MS", &IDLE_MS.to_string()),
        ("P2P_SELF_SYNC_SECS", "1"),
    ]) {
        return;
    }
    let (a, b) = settled_pair("msg");
    // Counted from here: the sessions that adding the contact opens may end any way.
    let lost = |p: &Peer, did: &str| {
        let needle = format!("chat session with {did} ended: connection lost");
        p.logs.lock().unwrap().iter().filter(|l| l.contains(&needle)).count()
    };
    let lost_before = (lost(&a, &b.did), lost(&b, &a.did));
    let id = a
        .node
        .send_text(b.did.clone(), "hello idle".into(), None)
        .unwrap()
        .id;
    eventually(60, "B holds the message", || holds(&b, &a.did, &id));
    eventually(60, "A sees it delivered", || delivered(&a, &b.did, &id));
    eventually(30, "both sides closed", || both_closed(&a, &b));
    assert!(
        idle_ended(&a, &b.did) || idle_ended(&b, &a.did),
        "one side closed for idleness"
    );
    // The other side saw a clean close, not a lost connection.
    assert_eq!((lost(&a, &b.did), lost(&b, &a.did)), lost_before, "closed cleanly");
    let (na, nb) = (opened(&a, &b.did), opened(&b, &a.did));
    // Past the 30 s pending tick: nothing pending, so nobody dials.
    assert!(
        !within(35, || sessions(&a, &b.did) > 0 || sessions(&b, &a.did) > 0),
        "no session comes back with nothing to send"
    );
    assert_eq!(
        (opened(&a, &b.did), opened(&b, &a.did)),
        (na, nb),
        "nobody redialled"
    );
}

// KIND: acceptance (user 2026-10-17: typing is the wake point: it dials the contact ahead of
// the send; with no further activity the session closes after the idle time, nobody redials)
#[test]
fn typing_dials_ahead_and_the_session_closes_after_idle() {
    if !support::with_env(&[
        ("P2P_IDLE_CLOSE_MS", &IDLE_MS.to_string()),
        ("P2P_SELF_SYNC_SECS", "1"),
    ]) {
        return;
    }
    let (a, b) = settled_pair("typing");
    let n = opened(&a, &b.did);
    let ends = idle_ends(&a, &b.did) + idle_ends(&b, &a.did);
    a.node.chat_typing(b.did.clone());
    eventually(30, "typing dials B", || sessions(&a, &b.did) > 0);
    assert_eq!(opened(&a, &b.did), n + 1, "one session");
    // Many keystrokes: throttled, no further dial while the session is up.
    for _ in 0..50 {
        a.node.chat_typing(b.did.clone());
    }
    eventually(30, "closed after idle", || both_closed(&a, &b));
    assert!(idle_ends(&a, &b.did) + idle_ends(&b, &a.did) > ends, "closed for idleness");
    let n = opened(&a, &b.did);
    assert!(
        !within(4 * IDLE_MS / 1000, || sessions(&a, &b.did) > 0 || sessions(&b, &a.did) > 0),
        "no redial once idle"
    );
    assert_eq!(opened(&a, &b.did), n);
}

// KIND: acceptance (user 2026-10-17: a contact can never hold my link open: B keeps typing to
// A (local activity on B only, no messages), A is idle, so A's side closes after the idle time
// each time; B's typing dials again, which is B's own action)
#[test]
fn a_contact_cannot_keep_my_session_open() {
    if !support::with_env(&[
        ("P2P_IDLE_CLOSE_MS", &IDLE_MS.to_string()),
        ("P2P_SELF_SYNC_SECS", "1"),
    ]) {
        return;
    }
    let (a, b) = settled_pair("peer-keep");
    let ends = idle_ends(&a, &b.did);
    let stop = std::time::Instant::now() + Duration::from_millis(5 * IDLE_MS);
    let mut up = false;
    while std::time::Instant::now() < stop {
        b.node.chat_typing(a.did.clone());
        up |= sessions(&a, &b.did) > 0;
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(up, "B's typing dialled A");
    assert!(idle_ends(&a, &b.did) > ends, "A closed its idle side although B stayed active");
    // B stops: everything closes.
    eventually(30, "both closed", || both_closed(&a, &b));
}

// KIND: acceptance (user 2026-10-17: presence needs consent: while A is idle, B does not see A
// online even over the session B dialled to deliver; once A types, B does. B is active (it
// wrote), so A sees B)
#[test]
fn presence_shows_only_while_active_with_the_contact() {
    if !support::with_env(&[
        ("P2P_IDLE_CLOSE_MS", &IDLE_MS.to_string()),
        ("P2P_SELF_SYNC_SECS", "1"),
    ]) {
        return;
    }
    let (a, b) = settled_pair("presence");
    let id = b
        .node
        .send_text(a.did.clone(), "are you there".into(), None)
        .unwrap()
        .id;
    let mut session = false;
    let mut b_sees_a = false;
    let mut a_sees_b = false;
    let stop = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < stop && !(holds(&a, &b.did, &id) && delivered(&b, &a.did, &id) && a_sees_b) {
        session |= sessions(&a, &b.did) > 0;
        b_sees_a |= b.node.contact_online(a.did.clone());
        a_sees_b |= a.node.contact_online(b.did.clone());
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(holds(&a, &b.did, &id) && delivered(&b, &a.did, &id), "delivered");
    assert!(session, "a session carried the delivery");
    assert!(a_sees_b, "A sees B, who is active with A");
    // Until the delivery session closes.
    let stop = std::time::Instant::now() + Duration::from_secs(30);
    while (sessions(&a, &b.did) > 0 || sessions(&b, &a.did) > 0) && std::time::Instant::now() < stop {
        b_sees_a |= b.node.contact_online(a.did.clone());
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!b_sees_a, "idle A sent no presence");
    // A types: B sees A online.
    eventually(30, "B sees A after A types", || {
        a.node.chat_typing(b.did.clone());
        b.node.contact_online(a.did.clone())
    });
}

// KIND: acceptance (user 2026-10-17, final: sending a read receipt is activity: B reading A's
// message makes B online for A; B's automatic delivered receipt alone does not)
#[test]
fn a_read_receipt_is_activity_but_a_delivered_receipt_is_not() {
    if !support::with_env(&[
        ("P2P_IDLE_CLOSE_MS", &IDLE_MS.to_string()),
        ("P2P_SELF_SYNC_SECS", "1"),
    ]) {
        return;
    }
    let (a, b) = settled_pair("read-activity");
    let id = a.node.send_text(b.did.clone(), "read me".into(), None).unwrap().id;
    eventually(60, "B holds the message", || holds(&b, &a.did, &id));
    eventually(60, "A sees it delivered", || delivered(&a, &b.did, &id));
    // B's delivered receipt went out; B stayed passive, so A never saw B online. Wait out the
    // sessions (A was active, so they close after the idle time).
    let mut a_sees_b = false;
    let stop = std::time::Instant::now() + Duration::from_secs(30);
    while !both_closed(&a, &b) && std::time::Instant::now() < stop {
        a_sees_b |= a.node.contact_online(b.did.clone());
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(both_closed(&a, &b), "sessions closed");
    assert!(!a_sees_b, "a delivered receipt is not activity: A never saw B online");
    // B reads: that is activity, so B dials if needed and A sees B online.
    b.node.mark_read(a.did.clone()).unwrap();
    eventually(30, "A sees B after B read", || a.node.contact_online(b.did.clone()));
}

// KIND: acceptance (user 2026-10-17, final: a live 1:1 call with the contact is activity: each
// side is online for the other during the call without anyone typing or writing)
#[test]
fn a_live_call_counts_as_activity_for_presence() {
    if !support::with_env(&[
        ("P2P_IDLE_CLOSE_MS", &IDLE_MS.to_string()),
        ("P2P_SELF_SYNC_SECS", "1"),
    ]) {
        return;
    }
    let (a, b) = settled_pair("call-activity");
    assert!(!a.node.contact_online(b.did.clone()) && !b.node.contact_online(a.did.clone()));
    let call = b.node.call(a.did.clone()).unwrap();
    let inc = incoming(&a, 40);
    a.node.answer(inc).unwrap();
    eventually(60, "both see each other online during the call", || {
        a.node.contact_online(b.did.clone()) && b.node.contact_online(a.did.clone())
    });
    b.node.hangup(call.call_id).unwrap();
}

// KIND: acceptance (user 2026-10-17: delivery is sender-driven; a pending outbox redials until
// the message is delivered, then the session idles out)
#[test]
fn a_pending_outbox_redials_until_delivered() {
    if !support::with_env(&[
        ("P2P_IDLE_CLOSE_MS", &IDLE_MS.to_string()),
        ("P2P_SELF_SYNC_SECS", "1"),
    ]) {
        return;
    }
    let (a, b) = settled_pair("outbox");
    b.node.stop();
    let id = a
        .node
        .send_text(b.did.clone(), "while you were away".into(), None)
        .unwrap()
        .id;
    std::thread::sleep(Duration::from_secs(3));
    assert!(!delivered(&a, &b.did, &id));
    online(&b.node);
    // B starts with nothing pending for A and does not dial; A's retry brings it.
    eventually(120, "B gets it from A's retries", || holds(&b, &a.did, &id));
    eventually(60, "A sees it delivered", || delivered(&a, &b.did, &id));
    eventually(30, "then the session idles out", || both_closed(&a, &b));
}

// KIND: acceptance (user 2026-10-17: no dial-everyone at startup)
#[test]
fn startup_does_not_dial_contacts_with_nothing_pending() {
    if !support::with_env(&[
        ("P2P_IDLE_CLOSE_MS", &IDLE_MS.to_string()),
        ("P2P_SELF_SYNC_SECS", "1"),
    ]) {
        return;
    }
    let (a, b) = settled_pair("start");
    let nb = opened(&b, &a.did);
    a.node.stop();
    online(&a.node);
    assert!(
        !within(8, || sessions(&a, &b.did) > 0 || opened(&b, &a.did) > nb),
        "a restart dials no contact it owes nothing"
    );
    // Sending still works: the sender dials.
    let id = a
        .node
        .send_text(b.did.clone(), "after restart".into(), None)
        .unwrap()
        .id;
    eventually(60, "B holds it", || holds(&b, &a.did, &id));
}

// KIND: acceptance (user 2026-10-17: own devices follow the same rule: dial when there is
// something to sync, close after the idle time)
#[test]
fn an_own_device_change_syncs_then_the_link_closes_after_idle() {
    if !support::with_env(&[
        ("P2P_IDLE_CLOSE_MS", &IDLE_MS.to_string()),
        ("P2P_SELF_SYNC_SECS", "1"),
    ]) {
        return;
    }
    let a1 = Peer::new(TAG, "own-a1");
    let a2 = a1.link_fresh(
        TAG,
        "own-a2",
        LinkAs {
            wait_existing_done: true,
            ..LinkAs::default()
        },
    );
    eventually(60, "two devices on both", || {
        [&a1, &a2].iter().all(|p| {
            p.node
                .linked_devices()
                .iter()
                .filter(|d| !d.removed)
                .count()
                == 2
        })
    });
    eventually(90, "first own-device sessions closed", || {
        !within(3, || {
            a1.node.own_sessions_for_test() > 0 || a2.node.own_sessions_for_test() > 0
        })
    });
    let me = a1.did.clone();
    // A note to self ("You"): a local change only own-device sync carries.
    let id = a1
        .node
        .send_text(me.clone(), "note to self".into(), None)
        .unwrap()
        .id;
    eventually(60, "the other device has it", || holds(&a2, &me, &id));
    eventually(30, "the own-device link closes after idle", || {
        a1.node.own_sessions_for_test() == 0 && a2.node.own_sessions_for_test() == 0
    });
    assert!(
        a1.logged("closed: idle") || a2.logged("closed: idle"),
        "closed for idleness"
    );
    assert!(
        !within(6, || a1.node.own_sessions_for_test() > 0
            || a2.node.own_sessions_for_test() > 0),
        "in sync: nobody redials"
    );
}
