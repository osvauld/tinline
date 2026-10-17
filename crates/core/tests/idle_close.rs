//! Sessions are not held open (decision 2026-10-17): a chat or own-device session with no
//! traffic for the idle time closes cleanly on both sides, unless the conversation is watched,
//! something is pending, or a 1:1 call is live; whoever has something to send redials.
//!
//! Real iroh endpoints (needs network, like `link.rs`). The idle time is shortened for the whole
//! binary with `P2P_IDLE_CLOSE_MS` (the production value is 60 s, `wire::IDLE_CLOSE`).
//! Core only: not an Android or desktop UI validation.

use std::time::Duration;

mod support;
use support::*;

const TAG: &str = "idle";
/// The idle time in this binary.
const IDLE_MS: u64 = 2_000;

fn short_idle() {
    // Every test sets the same values before any node exists, so the races are benign.
    unsafe {
        std::env::set_var("P2P_IDLE_CLOSE_MS", IDLE_MS.to_string());
        std::env::set_var("P2P_SELF_SYNC_SECS", "1");
    }
}

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
    p.logged(&format!("chat session with {did} ended: idle"))
}

/// A pair whose first sessions (contact exchange, profiles, receipts) have settled and closed.
fn settled_pair(tag: &str) -> (Peer, Peer) {
    short_idle();
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
// and nothing watched; the peer sees a normal close and neither side redials because of it)
#[test]
fn a_message_dials_a_session_that_closes_when_idle_and_nobody_redials() {
    let (a, b) = settled_pair("msg");
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
    let lost =
        |p: &Peer, did: &str| p.logged(&format!("chat session with {did} ended: connection lost"));
    assert!(!lost(&a, &b.did) && !lost(&b, &a.did), "closed cleanly");
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

// KIND: acceptance (user 2026-10-17: an open conversation keeps its session; closing it lets
// the idle close happen; presence is known only while the session is up)
#[test]
fn watching_keeps_the_session_open_and_unwatching_lets_it_close() {
    let (a, b) = settled_pair("watch");
    a.node.watch_presence(b.did.clone());
    eventually(60, "A sees B online", || {
        a.node.contact_online(b.did.clone())
    });
    // Well past the idle time: B keeps it too (A's keep datagram), so it never drops.
    assert!(
        !within(4 * IDLE_MS / 1000, || sessions(&a, &b.did) == 0
            || sessions(&b, &a.did) == 0),
        "a watched conversation keeps its session on both sides"
    );
    assert!(a.node.contact_online(b.did.clone()));
    a.node.unwatch_presence(b.did.clone());
    eventually(30, "closed once unwatched", || both_closed(&a, &b));
    eventually(10, "presence unknown again", || {
        !a.node.contact_online(b.did.clone())
    });
    let n = opened(&a, &b.did);
    assert!(
        !within(4 * IDLE_MS / 1000, || sessions(&a, &b.did) > 0),
        "no redial after unwatch"
    );
    assert_eq!(opened(&a, &b.did), n);
}

// KIND: acceptance (user 2026-10-17: delivery is sender-driven; a pending outbox redials until
// the message is delivered, then the session idles out)
#[test]
fn a_pending_outbox_redials_until_delivered() {
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
    short_idle();
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
