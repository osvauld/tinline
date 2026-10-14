//! P10: a conversation that goes through block -> unblock -> re-add (U19/U20/U24), a restart, a
//! lost chat store or a failed write never writes two different ops under the same
//! (writer ID, counter). Loro treats a known (peer, counter) as already held, so such an op would
//! be dropped silently on the contact's side: the message would never show up there.
//!
//! Real nodes over real iroh endpoints (needs network, like `blocking_acceptance.rs`); waits are
//! bounded polls. Restarts drop the node and open the same directory again with no pause, so they
//! also exercise `chat_open` waiting for a `chat.redb` the old node still holds.
//!
//! Every test carries `// KIND: acceptance` and the decision it enforces.

use std::sync::Arc;
use std::time::Duration;

use p2pcore::Node;

mod support;
use support::eventually_200ms as eventually;
use support::*;

const TAG: &str = "p10";

fn open(dir: &std::path::Path) -> Arc<Node> {
    Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap()
}

/// a and b, each a contact of the other (b scanned a's ticket).
fn pair(tag: &str) -> (Peer, Peer) {
    Peer::pair(TAG, tag)
}

/// Drops the node and opens the same data directory again at once (a process restart). No
/// pause: the chat store must open even while the old node's tasks still hold `chat.redb`.
fn restart(p: &mut Peer) {
    p.reopen();
    p.node.unlock(PASS.into()).unwrap();
    online(&p.node);
    eventually(10, "the chat store is open after the restart", || p.node.writer_id_for_test().is_some());
}

fn text_of(p: &Peer, did: &str, id: &str) -> Option<String> {
    all_msgs(p, did).into_iter().find(|m| m.id == id).map(|m| m.text)
}

fn send(p: &Peer, to: &Peer, text: &str) -> String {
    p.node.send_text(to.did.clone(), text.into(), None).unwrap().id
}

/// Both sides hold every one of `ids` with the same text, and the sender's copy says delivered.
fn both_hold_all(a: &Peer, b: &Peer, from_a: &[&String], from_b: &[&String]) {
    for id in from_a {
        eventually(90, &format!("b holds a's {id}"), || holds(b, &a.did, id));
        eventually(60, &format!("a sees {id} delivered"), || delivered(a, &b.did, id));
    }
    for id in from_b {
        eventually(90, &format!("a holds b's {id}"), || holds(a, &b.did, id));
        eventually(60, &format!("b sees {id} delivered"), || delivered(b, &a.did, id));
    }
    for id in from_a.iter().chain(from_b) {
        assert_eq!(text_of(a, &b.did, id), text_of(b, &a.did, id), "{id} reads the same on both sides");
    }
}

/// Once everything is delivered, both replicas hold the same op count per (day, writer ID).
fn same_ops(a: &Peer, b: &Peer) {
    let (mut oa, mut ob) = (a.node.chat_doc_peers_for_test(b.did.clone()).0, b.node.chat_doc_peers_for_test(a.did.clone()).0);
    oa.sort();
    ob.sort();
    assert_eq!(oa, ob, "both replicas hold the same ops per (day, writer ID)");
}

/// B takes A's block (its app deletes its contact entry for A, U20), A unblocks, B re-adds A.
fn block_unblock_readd(a: &Peer, b: &Peer) {
    a.node.block(b.did.clone()).unwrap();
    eventually(30, "b holds the notice", || b.node.block_state(a.did.clone()).blocked_me.is_some());
    assert!(!b.node.contacts().iter().any(|c| c.did == a.did), "the deletion request removed the contact entry");
    a.node.unblock(b.did.clone()).unwrap();
    eventually(30, "b's blocked-you state clears", || b.node.block_state(a.did.clone()).blocked_me.is_none());
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
}

// KIND: acceptance
// Decision: U19/U20/U24 + P10 — block, unblock and re-add keep the conversation (history on both
// sides) and new messages from both sides, written the same day as the old ones, reach the other
// side: no (writer ID, counter) is reused.
#[test]
fn after_block_unblock_readd_both_sides_get_every_new_message() {
    let (a, b) = pair("readd");
    let a1 = send(&a, &b, "a1 before the block");
    let b1 = send(&b, &a, "b1 before the block");
    both_hold_all(&a, &b, &[&a1], &[&b1]);
    let (wa, wb) = (a.node.writer_id_for_test().unwrap(), b.node.writer_id_for_test().unwrap());

    block_unblock_readd(&a, &b);
    assert!(holds(&a, &b.did, &b1) && holds(&b, &a.did, &a1), "history kept on both sides");

    let a2 = send(&a, &b, "a2 after the re-add");
    let b2 = send(&b, &a, "b2 after the re-add");
    let a3 = send(&a, &b, "a3");
    let b3 = send(&b, &a, "b3");
    both_hold_all(&a, &b, &[&a1, &a2, &a3], &[&b1, &b2, &b3]);
    assert_eq!(all_msgs(&a, &b.did).len(), 6);
    assert_eq!(all_msgs(&b, &a.did).len(), 6);
    assert_eq!((a.node.writer_id_for_test(), b.node.writer_id_for_test()), (Some(wa), Some(wb)), "same installs, same writer IDs");
    same_ops(&a, &b);
}

// KIND: acceptance
// Decision: U24 (messages written before a block are delivered after an unblock + re-add) + P10
// — with restarts of both processes at every step in between.
#[test]
fn messages_written_before_the_block_arrive_after_the_readd_across_restarts() {
    let (mut a, mut b) = pair("prebl");
    let w1 = send(&a, &b, "warm");
    both_hold_all(&a, &b, &[&w1], &[]);

    // a writes while b is away; then b writes while a is away. Neither reaches the other.
    b.node.stop();
    let a_pre = send(&a, &b, "a: written before the block, b was away");
    a.node.stop();
    online(&b.node);
    let b_pre = send(&b, &a, "b: written before the block, a was away");
    std::thread::sleep(Duration::from_secs(2));
    assert!(!holds(&a, &b.did, &b_pre) && !holds(&b, &a.did, &a_pre));

    // a blocks b while still away, restarts, comes back: b's message is refused, b takes the notice.
    a.node.block(b.did.clone()).unwrap();
    restart(&mut a);
    eventually(60, "b holds the notice", || b.node.block_state(a.did.clone()).blocked_me.is_some());
    assert!(!holds(&a, &b.did, &b_pre), "refused while blocked");
    restart(&mut b);
    assert!(b.node.block_state(a.did.clone()).blocked_me.is_some(), "the notice survives the restart");

    a.node.unblock(b.did.clone()).unwrap();
    restart(&mut a);
    eventually(60, "b's blocked-you state clears", || b.node.block_state(a.did.clone()).blocked_me.is_none());
    restart(&mut b);
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    restart(&mut b);

    both_hold_all(&a, &b, &[&w1, &a_pre], &[&b_pre]);
    // And both write the same day afterwards.
    let a2 = send(&a, &b, "a2 after everything");
    let b2 = send(&b, &a, "b2 after everything");
    both_hold_all(&a, &b, &[&w1, &a_pre, &a2], &[&b_pre, &b2]);
    same_ops(&a, &b);
}

/// The account directory of `p` (where `chat.redb` lives).
fn chat_store_path(p: &Peer) -> std::path::PathBuf {
    fn find(d: &std::path::Path) -> Option<std::path::PathBuf> {
        for e in std::fs::read_dir(d).ok()?.flatten() {
            let path = e.path();
            if path.file_name().is_some_and(|n| n == "chat.redb") {
                return Some(path);
            }
            if path.is_dir()
                && let Some(f) = find(&path)
            {
                return Some(f);
            }
        }
        None
    }
    find(&p.dir).expect("chat.redb")
}

// KIND: acceptance
// Decision: P10 — a chat store that is gone (deleted, or lost while the profile stayed) is a
// fresh start: the install writes under a new writer ID, so its first new message is not taken
// for one the contact already holds.
#[test]
fn a_lost_chat_store_does_not_reuse_writer_counters() {
    let (mut a, b) = pair("lost");
    let m1 = send(&a, &b, "m1");
    let m2 = send(&a, &b, "m2");
    both_hold_all(&a, &b, &[&m1, &m2], &[]);
    let before = a.node.writer_id_for_test().unwrap();

    let path = chat_store_path(&a);
    a.node.stop();
    let old = std::mem::replace(&mut a.node, open(&a.dir));
    drop(old);
    std::thread::sleep(Duration::from_secs(2));
    std::fs::remove_file(&path).unwrap();
    a.node.unlock(PASS.into()).unwrap();
    online(&a.node);
    eventually(10, "a's chat store is open", || a.node.writer_id_for_test().is_some());
    assert_ne!(a.node.writer_id_for_test(), Some(before), "a fresh chat store writes under a fresh writer ID");

    let m3 = send(&a, &b, "m3 after the store was lost");
    eventually(90, "b holds m3", || holds(&b, &a.did, &m3));
    assert_eq!(text_of(&b, &a.did, &m3).as_deref(), Some("m3 after the store was lost"));
    let r = send(&b, &a, "reply");
    eventually(90, "a holds the reply", || holds(&a, &b.did, &r));
}

// KIND: acceptance
// Decision: P10 — a local write that could not be stored leaves nothing behind: the contact never
// gets it, and the next writes (also after a restart) do not reuse its counters.
#[test]
fn a_failed_chat_write_is_not_sent_and_its_counters_are_not_reused() {
    let (mut a, b) = pair("failw");
    let m0 = send(&a, &b, "m0");
    both_hold_all(&a, &b, &[&m0], &[]);

    a.node.fail_next_chat_writes_for_test(1);
    assert!(a.node.send_text(b.did.clone(), "m1: never stored".into(), None).is_err());
    let m2 = send(&a, &b, "m2");
    both_hold_all(&a, &b, &[&m0, &m2], &[]);
    std::thread::sleep(Duration::from_secs(2));
    assert!(!all_msgs(&b, &a.did).iter().any(|m| m.text.starts_with("m1")), "the failed write never left a");
    assert!(!all_msgs(&a, &b.did).iter().any(|m| m.text.starts_with("m1")), "nor shows on a");

    restart(&mut a);
    let m3 = send(&a, &b, "m3 after a restart");
    both_hold_all(&a, &b, &[&m0, &m2, &m3], &[]);
    same_ops(&a, &b);
}
