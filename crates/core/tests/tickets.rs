//! Ticket and contact rules between real nodes (needs network, like calls.rs).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, Error, Node, NodeEvents, NodeStatus};

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
    dir: PathBuf,
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn peer(tag: &str, started: bool) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-tickets-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    node.create_identity(tag.into(), PASS.into()).unwrap();
    if started {
        node.start().unwrap();
        let t = Instant::now();
        while !node.status().online && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let did = node.profile().unwrap().did;
    Peer { node, did, dir }
}

#[test]
fn reset_and_redeemed_tickets_are_dead() {
    let (a, b, c) = (peer("reset-a", true), peer("reset-b", true), peer("reset-c", true));
    let t1 = a.node.my_ticket().unwrap();
    assert_eq!(a.node.my_ticket().unwrap(), t1, "stable until used");
    a.node.reset_ticket().unwrap();
    let t2 = a.node.my_ticket().unwrap();
    assert_ne!(t1, t2);
    // The reset ticket is unspent but no longer current.
    assert!(matches!(b.node.add_contact(t1.clone()), Err(Error::Rejected(_))));
    assert!(a.node.contacts().is_empty());
    b.node.add_contact(t2.clone()).unwrap();
    // Redeemed: spent, and a fresh one is minted.
    assert!(matches!(c.node.add_contact(t2), Err(Error::Rejected(_))));
    assert_ne!(a.node.my_ticket().unwrap(), t1);
    assert_eq!(a.node.contacts().len(), 1);
}

#[test]
fn removed_contact_cannot_come_back_through_our_ticket() {
    let (a, b) = (peer("block-a", true), peer("block-b", true));
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    a.node.remove_contact(b.did.clone()).unwrap();
    assert!(a.node.contacts().is_empty());
    let fresh = a.node.my_ticket().unwrap();
    assert!(matches!(b.node.add_contact(fresh), Err(Error::Rejected(_))));
    assert!(a.node.contacts().is_empty(), "still blocked");
    // Scanning their ticket ourselves is the way back.
    a.node.add_contact(b.node.my_ticket().unwrap()).unwrap();
    assert_eq!(a.node.contacts().len(), 1);
}

#[test]
fn ticket_with_unusable_relay_is_refused() {
    let a = peer("relay-a", false);
    let (other, _) = identity::generate();
    let dev = proto::device_public(&proto::new_device_secret());
    for relay in ["http://10.0.0.1/", "ftp://relay.example/", "https://not a url"] {
        let t = proto::issue_contact_ticket(&other, dev, "evil", Some(relay.into()), now(), 600);
        match a.node.add_contact(t.to_text()) {
            Err(Error::Protocol(m)) => assert!(m.contains("relay"), "{relay}: {m}"),
            other => panic!("{relay}: expected a relay refusal, got {other:?}"),
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}
