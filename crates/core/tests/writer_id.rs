//! P9 (U16): the Loro peer id a device writes chat docs with is its writer ID,
//! `writer_id(DID, device value)`, with a random device value per account per install.
//! Real nodes over real iroh endpoints (needs network, like `restore.rs`); waits are bounded polls.
//!
//! Every test carries one label:
//! - `KIND: invariant`: holds after P9 and should keep holding.
//! - `KIND: limitation`: an accepted gap, stated in the comment.

use std::sync::Arc;
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, DeliveryState, Message, Node, NodeEvents, NodeStatus};

const PASS: &str = "test-passphrase";

struct Quiet;
impl NodeEvents for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}

struct Peer {
    node: Arc<Node>,
    did: String,
    phrase: String,
    dir: std::path::PathBuf,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.node.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn open(dir: &std::path::Path) -> Arc<Node> {
    Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap()
}

fn online(n: &Node) {
    n.start().unwrap();
    let t = Instant::now();
    while !n.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn peer(name: &str) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-p9-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let node = open(&dir);
    let phrase = node.create_identity(name.into(), PASS.into()).unwrap();
    online(&node);
    let did = node.profile().unwrap().did;
    Peer { node, did, phrase, dir }
}

/// Drops the node and opens the same data directory again (a process restart).
fn restart(p: &mut Peer) {
    p.node.stop();
    let old = std::mem::replace(&mut p.node, open(&p.dir));
    drop(old);
    // The old node's tasks may hold `chat.redb` for a moment, and `chat_open` does not retry
    // (unlike `account.redb`): give them time to wind down, as a real process exit would.
    std::thread::sleep(Duration::from_secs(2));
    p.node.unlock(PASS.into()).unwrap();
    online(&p.node);
}

fn eventually(secs: u64, what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("timed out waiting for {what}");
}

fn all_msgs(p: &Peer, did: &str) -> Vec<Message> {
    let mut out = Vec::new();
    let mut page = p.node.chat_day(did.to_string(), None).ok();
    while let Some(pg) = page {
        out.extend(pg.messages.clone());
        page = pg.older_day.and_then(|d| p.node.chat_day(did.to_string(), Some(d)).ok());
    }
    out
}

fn text_of(p: &Peer, did: &str, id: &str) -> Option<String> {
    all_msgs(p, did).into_iter().find(|m| m.id == id).map(|m| m.text)
}

fn creator(p: &Peer, did: &str, id: &str) -> u64 {
    p.node.chat_doc_peers_for_test(did.to_string()).1.into_iter().find(|(m, _)| m == id).expect("message held").1
}

/// Ops per (day, peer id) in `p`'s copy of the conversation with `did`.
fn ops(p: &Peer, did: &str) -> Vec<(String, u64, i32)> {
    p.node.chat_doc_peers_for_test(did.to_string()).0
}

// KIND: invariant
// Once both sides have seen the other write notes with a device value, every newly written op
// (new messages, and edits of a message created under a legacy peer id) is under a writer ID:
// no device-derived peer id and no device key bytes appear as Loro peer ids in new content, on
// either replica, and every replica keeps the sender's number. The first message, written
// before A knew that B judges writer IDs, is under A's legacy peer id (compatibility with
// un-upgraded contacts). Ticks follow the writer ID. The writer ID survives a restart.
#[test]
fn new_content_is_written_under_writer_ids_not_device_derived_peer_ids() {
    let mut a = peer("wid-a");
    let b = peer("wid-b");
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let (wa, wb) = (a.node.writer_id_for_test().unwrap(), b.node.writer_id_for_test().unwrap());
    assert_ne!(wa, wb);
    let (dev_a, dev_b) = (a.node.device_key_for_test().unwrap(), b.node.device_key_for_test().unwrap());

    let m1 = a.node.send_text(b.did.clone(), "m1: before A knows B".into(), None).unwrap().id;
    eventually(60, "B holds m1", || text_of(&b, &a.did, &m1).is_some());
    let after_m1 = ops(&a, &b.did);
    let r1 = b.node.send_text(a.did.clone(), "r1".into(), None).unwrap().id;
    eventually(60, "A holds r1", || text_of(&a, &b.did, &r1).is_some());
    let m2 = a.node.send_text(b.did.clone(), "m2".into(), None).unwrap().id;
    a.node.edit_message(b.did.clone(), m1.clone(), "m1: edited under the writer ID".into()).unwrap();
    eventually(60, "B holds m2 and the edit", || {
        text_of(&b, &a.did, &m2).is_some() && text_of(&b, &a.did, &m1).as_deref() == Some("m1: edited under the writer ID")
    });
    let r2 = b.node.send_text(a.did.clone(), "r2".into(), None).unwrap().id;
    eventually(60, "A holds r2", || text_of(&a, &b.did, &r2).is_some());
    eventually(60, "A shows m2 delivered", || all_msgs(&a, &b.did).iter().any(|m| m.id == m2 && m.delivery == DeliveryState::Delivered));

    let day = ops(&a, &b.did)[0].0.clone();
    let la = p2pcore::Node::legacy_peer_for_test(dev_a.clone(), a.did.clone(), b.did.clone(), day.clone()).unwrap();
    let lb = p2pcore::Node::legacy_peer_for_test(dev_b.clone(), a.did.clone(), b.did.clone(), day.clone()).unwrap();
    for (p, did) in [(&a, &b.did), (&b, &a.did)] {
        // Same creators on both replicas: the sender's number, unchanged.
        assert_eq!(creator(p, did, &m1), la, "m1 was written before A knew that B judges writer IDs");
        assert_eq!(creator(p, did, &r1), wb);
        assert_eq!(creator(p, did, &m2), wa);
        assert_eq!(creator(p, did, &r2), wb);
        let held = ops(p, did);
        // Only the legacy op(s) of m1 are under a device-derived peer id; B never wrote under one.
        let peers: Vec<u64> = held.iter().map(|(_, q, _)| *q).collect();
        assert!(peers.iter().all(|q| [la, wa, wb].contains(q)), "{peers:?}");
        assert!(!peers.contains(&lb));
        // Nothing was written under it after m1 (the edit of m1 is under the writer ID).
        let at = |v: &[(String, u64, i32)]| v.iter().find(|(_, q, _)| *q == la).map(|(_, _, c)| *c);
        assert_eq!(at(&held), at(&after_m1));
        // No device key bytes as a peer id.
        for dev in [&dev_a, &dev_b] {
            let k = proto::device_from_text(dev).unwrap();
            let raw = u64::from_le_bytes(k[..8].try_into().unwrap());
            let raw_be = u64::from_be_bytes(k[..8].try_into().unwrap());
            assert!(!peers.contains(&raw) && !peers.contains(&raw_be));
        }
    }

    // A restart keeps the device value, hence the writer ID; new content stays under it.
    restart(&mut a);
    eventually(20, "A's chat store is open", || a.node.writer_id_for_test().is_some());
    assert_eq!(a.node.writer_id_for_test(), Some(wa));
    let m3 = a.node.send_text(b.did.clone(), "m3: after restart".into(), None).unwrap().id;
    eventually(60, "B holds m3", || text_of(&b, &a.did, &m3).is_some());
    assert_eq!(creator(&b, &a.did, &m3), wa);
}

// KIND: invariant
// A restore (forgotten passphrase, over the existing account directory) is a new install of the
// device: new device key and new device value, hence a new writer ID. Chat keeps working under
// it, and the contact accepts it without knowing anything new about this device.
#[test]
fn a_restore_makes_a_new_writer_id() {
    let a = peer("wid-ra");
    let mut b = peer("wid-rb");
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let m1 = b.node.send_text(a.did.clone(), "m1".into(), None).unwrap().id;
    eventually(60, "A holds m1", || text_of(&a, &b.did, &m1).is_some());
    let r1 = a.node.send_text(b.did.clone(), "r1".into(), None).unwrap().id;
    eventually(60, "B holds r1", || text_of(&b, &a.did, &r1).is_some());
    let before = b.node.writer_id_for_test().unwrap();
    let name = b.node.profile().unwrap().name;
    b.node.stop();
    let node = open(&b.dir);
    drop(std::mem::replace(&mut b.node, node));
    b.node.restore_identity(b.phrase.clone(), name, "pass two".into()).unwrap();
    online(&b.node);
    let after = b.node.writer_id_for_test().unwrap();
    assert_ne!(after, before);
    let m2 = b.node.send_text(a.did.clone(), "m2: after restore".into(), None).unwrap().id;
    eventually(90, "A holds m2", || text_of(&a, &b.did, &m2).is_some());
    assert_eq!(creator(&a, &b.did, &m2), after);
}

// KIND: invariant (P9b)
// The writer ID is tied to the install's device ID, not to the device key: replacing the device
// key on disk without a new install (what endpoint rotation will do) keeps the writer ID, and
// new content stays under it on the contact's side. A restore still makes a new one
// (`a_restore_makes_a_new_writer_id`).
#[test]
fn a_new_device_key_without_a_new_install_keeps_the_writer_id() {
    let mut a = peer("wid-ka");
    let b = peer("wid-kb");
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let m1 = a.node.send_text(b.did.clone(), "m1".into(), None).unwrap().id;
    eventually(60, "B holds m1", || text_of(&b, &a.did, &m1).is_some());
    let r1 = b.node.send_text(a.did.clone(), "r1".into(), None).unwrap().id;
    eventually(60, "A holds r1", || text_of(&a, &b.did, &r1).is_some());
    let (wa, key) = (a.node.writer_id_for_test().unwrap(), a.node.device_key_for_test().unwrap());
    a.node.rotate_device_key_on_disk_for_test().unwrap();
    restart(&mut a);
    eventually(20, "A's chat store is open", || a.node.writer_id_for_test().is_some());
    assert_ne!(a.node.device_key_for_test().unwrap(), key, "the device key changed");
    assert_eq!(a.node.writer_id_for_test(), Some(wa), "the writer ID did not");
    let m2 = a.node.send_text(b.did.clone(), "m2: new device key".into(), None).unwrap().id;
    eventually(90, "B holds m2", || text_of(&b, &a.did, &m2).is_some());
    assert_eq!(creator(&b, &a.did, &m2), wa);
}
