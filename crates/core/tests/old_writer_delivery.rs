//! P10b: our own messages written under a writer ID that is no longer this install's (a restore
//! over an existing data directory keeps `chat.redb` but makes a new device ID, hence a new writer
//! ID; or another install of the account wrote them) still reach the contact. A device sends a
//! contact every op it lacks that any of our writer IDs wrote (each proven by our DID signature,
//! P9c), not only the ops of its current writer ID.
//!
//! Real iroh endpoints (needs network, like `restore.rs`, `link.rs`). The shared harness is in
//! `support/`.

#![allow(dead_code)]

use std::time::Duration;

use p2pcore::DeliveryState;

mod support;
use support::eventually_200ms as eventually;
use support::*;

const S: Suite = Suite("p10b");

impl Peer {
    /// Whether this node refused any chat batch, history snapshot or own-device batch.
    fn refused_chat(&self) -> bool {
        ["rejected chat batch", "rejected history", "rejected own-device batch"].iter().any(|n| self.logged(n))
    }
}

fn creator(p: &Peer, did: &str, id: &str) -> u64 {
    p.node.chat_doc_peers_for_test(did.to_string()).1.into_iter().find(|(m, _)| m == id).map(|(_, w)| w).expect("message held")
}

fn count(p: &Peer, did: &str, id: &str) -> usize {
    all_msgs(p, did).iter().filter(|m| m.id == id).count()
}

/// B (contact of A) sends `m1` (delivered), then `m2` while A is offline. Returns `(a, b, m2,
/// B's writer ID at the time)`, both left as they are: A stopped, B started.
fn pending_m2(tag: &str) -> (Peer, Peer, String, u64) {
    let a = S.noisy(&format!("{tag}-a"), true);
    let b = S.noisy(&format!("{tag}-b"), true);
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let m1 = b.node.send_text(a.did.clone(), "m1".into(), None).unwrap().id;
    eventually(60, "A holds m1", || has(&a, &b.did, &m1));
    eventually(60, "B shows m1 delivered", || ticks(&b, &a.did, &m1) == DeliveryState::Delivered);
    a.node.stop();
    let m2 = b.node.send_text(a.did.clone(), "m2: written while A is offline".into(), None).unwrap().id;
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(ticks(&b, &a.did, &m2), DeliveryState::Pending, "A is offline");
    let wid = b.node.writer_id_for_test().unwrap();
    (a, b, m2, wid)
}

/// After B's writer ID changed, with B writing nothing new, A comes online and gets `m2` (under
/// B's old writer ID) exactly once, and B shows it delivered.
fn m2_arrives(a: &Peer, b: &Peer, m2: &str, old_wid: u64) {
    assert_ne!(b.node.writer_id_for_test().unwrap(), old_wid, "B writes under a new writer ID now");
    online(&a.node);
    eventually(90, "A holds m2", || has(a, &b.did, m2));
    assert_eq!(count(a, &b.did, m2), 1, "exactly once");
    assert_eq!(creator(a, &b.did, m2), old_wid, "written under B's old writer ID");
    eventually(60, "B shows m2 delivered", || ticks(b, &a.did, m2) == DeliveryState::Delivered);
    assert!(!a.refused_chat() && !b.refused_chat(), "nothing refused");
}

// (1) A message written while the contact is offline, then a restore over the same directory
// (keeps `chat.redb`, new device ID and writer ID), then the contact comes online: it receives
// the pre-restore message without B writing anything new.
#[test]
fn a_message_pending_at_a_restore_is_delivered_without_a_new_write() {
    let (a, mut b, m2, old_wid) = pending_m2("p10b-r");
    b.restore_in_place();
    online(&b.node);
    m2_arrives(&a, &b, &m2, old_wid);
}

// (2) The same with a process restart of B after the restore, before A returns: the pending day
// and B's old writer ID are known from the store, not from memory.
#[test]
fn a_message_pending_at_a_restore_is_delivered_after_a_restart() {
    let (a, mut b, m2, old_wid) = pending_m2("p10b-rr");
    b.restore_in_place();
    online(&b.node);
    std::thread::sleep(Duration::from_millis(500));
    b.reopen_unlocked("pass two");
    online(&b.node);
    m2_arrives(&a, &b, &m2, old_wid);
}

// (3) Another install of the same account forwards a message the first one wrote while the
// contact was offline: B1 writes `m2`, B2 receives it by own-device sync, B1 is stopped, unlinked
// and wiped, then A comes online and gets `m2` from B2 (proven by B's DID signature).
#[test]
fn a_sibling_device_forwards_an_undelivered_message_of_an_unlinked_device() {
    let a = S.noisy("p10b-s-a", true);
    let b1 = S.noisy("p10b-s-b1", true);
    b1.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let b2 = S.link_both_done(&b1, "p10b-s-b2");
    eventually(40, "B2 has A", || b2.node.contacts().iter().any(|c| c.did == a.did));
    let m1 = b1.node.send_text(a.did.clone(), "m1".into(), None).unwrap().id;
    eventually(60, "A holds m1", || has(&a, &b1.did, &m1));
    eventually(60, "B2 holds m1", || has(&b2, &a.did, &m1));
    a.node.stop();
    let m2 = b1.node.send_text(a.did.clone(), "m2: written on B1 while A is offline".into(), None).unwrap().id;
    let b1_wid = b1.node.writer_id_for_test().unwrap();
    assert_ne!(b2.node.writer_id_for_test().unwrap(), b1_wid, "two installs, two writer IDs");
    eventually(60, "B2 holds m2", || has(&b2, &a.did, &m2));
    let b1_dev = b1.node.device_key_for_test().unwrap();
    drop(b1); // stopped, directory wiped
    b2.node.unlink_device(b1_dev, Some("n-pass".into())).unwrap();
    online(&a.node);
    eventually(120, "A holds m2", || has(&a, &b2.did, &m2));
    assert_eq!(count(&a, &b2.did, &m2), 1, "exactly once");
    assert_eq!(creator(&a, &b2.did, &m2), b1_wid, "written under B1's writer ID");
    eventually(60, "B2 shows m2 delivered", || ticks(&b2, &a.did, &m2) == DeliveryState::Delivered);
    assert!(!a.refused_chat() && !b2.refused_chat(), "nothing refused");
}

// A live push never carries an op the contact refuses for its 24 h live window. B holds an
// undelivered message of ours sent 30 h ago under another writer ID (as relayed by a sibling;
// nothing pushes it), then edits it live while its session with A is up. The batch holds the
// old message's creation, so it goes out as a catch-up `Sync`, not a live `Push`: A refuses
// nothing, the session stays up, and A gets the message with the edit.
#[test]
fn a_live_edit_does_not_push_an_old_undelivered_op_of_another_writer() {
    let a = S.noisy("p10b-l-a", true);
    let b = S.noisy("p10b-l-b", true);
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let m1 = b.node.send_text(a.did.clone(), "m1".into(), None).unwrap().id;
    eventually(60, "A holds m1", || has(&a, &b.did, &m1));
    eventually(60, "B shows m1 delivered", || ticks(&b, &a.did, &m1) == DeliveryState::Delivered);
    std::thread::sleep(Duration::from_secs(1));
    let ended = |p: &Peer| p.logs.lock().unwrap().iter().filter(|l| l.contains("chat session with") && l.contains("ended")).count();
    let (a_ended, b_ended) = (ended(&a), ended(&b));
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let at = now_ms - 30 * 3600 * 1000;
    let old = b.node.write_as_sibling_for_test(a.did.clone(), "old: from a sibling".into(), at).unwrap();
    assert_ne!(creator(&b, &a.did, &old), b.node.writer_id_for_test().unwrap(), "another writer ID");
    b.node.edit_message(a.did.clone(), old.clone(), "old: edited live".into()).unwrap();
    eventually(60, "A holds the edited message", || all_msgs(&a, &b.did).iter().any(|m| m.id == old && m.text == "old: edited live"));
    eventually(60, "B shows it delivered", || ticks(&b, &a.did, &old) == DeliveryState::Delivered);
    assert!(!a.refused_chat(), "A refused nothing");
    assert_eq!((ended(&a), ended(&b)), (a_ended, b_ended), "the session never restarted");
}
