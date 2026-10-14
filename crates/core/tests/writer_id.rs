//! U16, P9c: the Loro peer id a device writes chat docs with is its writer ID,
//! `writer_id(DID, device ID)`, with a random device ID per account per install, and nothing else.
//! Real nodes over real iroh endpoints (needs network, like `restore.rs`); waits are bounded polls.
//!
//! Every test carries one label:
//! - `KIND: invariant`: holds and should keep holding.

use std::sync::Arc;
use p2pcore::{DeliveryState, Node};

mod support;
use support::eventually_200ms as eventually;
use support::*;

const TAG: &str = "p9";

fn open(dir: &std::path::Path) -> Arc<Node> {
    Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap()
}

fn peer(name: &str) -> Peer {
    Peer::new(TAG, name)
}

/// Drops the node and opens the same data directory again (a process restart).
fn restart(p: &mut Peer) {
    p.reopen();
    // No pause: `chat_open` waits for a `chat.redb` the old node's tasks still hold (P10).
    p.node.unlock(PASS.into()).unwrap();
    online(&p.node);
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

// KIND: invariant (P9c: strict, rewritten)
// Every op of a conversation is under a writer ID, from the very first message on: each side
// writes only under its own, every replica keeps the sender's number, and neither a
// device-derived peer id nor device key bytes appear as a Loro peer id. Ticks follow the writer
// ID. The writer ID survives a restart.
#[test]
fn every_op_is_written_under_a_writer_id() {
    let mut a = peer("wid-a");
    let b = peer("wid-b");
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let (wa, wb) = (a.node.writer_id_for_test().unwrap(), b.node.writer_id_for_test().unwrap());
    assert_ne!(wa, wb);
    let (dev_a, dev_b) = (a.node.device_key_for_test().unwrap(), b.node.device_key_for_test().unwrap());

    let m1 = a.node.send_text(b.did.clone(), "m1: the first message".into(), None).unwrap().id;
    eventually(60, "B holds m1", || text_of(&b, &a.did, &m1).is_some());
    let r1 = b.node.send_text(a.did.clone(), "r1".into(), None).unwrap().id;
    eventually(60, "A holds r1", || text_of(&a, &b.did, &r1).is_some());
    let m2 = a.node.send_text(b.did.clone(), "m2".into(), None).unwrap().id;
    a.node.edit_message(b.did.clone(), m1.clone(), "m1: edited".into()).unwrap();
    eventually(60, "B holds m2 and the edit", || text_of(&b, &a.did, &m2).is_some() && text_of(&b, &a.did, &m1).as_deref() == Some("m1: edited"));
    eventually(60, "A shows m2 delivered", || all_msgs(&a, &b.did).iter().any(|m| m.id == m2 && m.delivery == DeliveryState::Delivered));
    eventually(60, "A shows m1 delivered", || all_msgs(&a, &b.did).iter().any(|m| m.id == m1 && m.delivery == DeliveryState::Delivered));

    for (p, did) in [(&a, &b.did), (&b, &a.did)] {
        // Same creators on both replicas: the sender's number, unchanged.
        assert_eq!(creator(p, did, &m1), wa);
        assert_eq!(creator(p, did, &r1), wb);
        assert_eq!(creator(p, did, &m2), wa);
        // No other peer id at all.
        let peers: Vec<u64> = ops(p, did).iter().map(|(_, q, _)| *q).collect();
        assert!(peers.iter().all(|q| [wa, wb].contains(q)), "{peers:?}");
        // No device key bytes as a peer id.
        for dev in [&dev_a, &dev_b] {
            let k = proto::device_from_text(dev).unwrap();
            let raw = u64::from_le_bytes(k[..8].try_into().unwrap());
            let raw_be = u64::from_be_bytes(k[..8].try_into().unwrap());
            assert!(!peers.contains(&raw) && !peers.contains(&raw_be));
        }
    }

    // A restart keeps the device ID, hence the writer ID; new content stays under it.
    restart(&mut a);
    eventually(20, "A's chat store is open", || a.node.writer_id_for_test().is_some());
    assert_eq!(a.node.writer_id_for_test(), Some(wa));
    let m3 = a.node.send_text(b.did.clone(), "m3: after restart".into(), None).unwrap().id;
    eventually(60, "B holds m3", || text_of(&b, &a.did, &m3).is_some());
    assert_eq!(creator(&b, &a.did, &m3), wa);
}

// KIND: invariant
// A restore (forgotten passphrase, over the existing account directory) is a new install of the
// device: new device key and new device ID, hence a new writer ID. Chat keeps working under it,
// and the contact accepts it without knowing anything new about this device (P9c: every version
// is DID-signed, the writer ID is proven by the version's device ID).
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
