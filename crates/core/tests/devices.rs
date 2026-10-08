//! A second device of the same person (same phrase, another install) meeting a contact.
//! Needs network, like calls.rs.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, Node, NodeEvents, NodeStatus};

const PASS: &str = "test-passphrase";

#[derive(Default)]
struct Count {
    contacts_changed: AtomicU32,
    incoming: AtomicU32,
}
impl NodeEvents for Count {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {
        self.contacts_changed.fetch_add(1, Ordering::SeqCst);
    }
    fn on_incoming_call(&self, _: CallInfo) {
        self.incoming.fetch_add(1, Ordering::SeqCst);
    }
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}

struct Peer {
    node: Arc<Node>,
    events: Arc<Count>,
    did: String,
    dir: PathBuf,
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `phrase`: another install of an existing person; each gets its own data root and device key.
fn peer(tag: &str, phrase: Option<&str>) -> (Peer, String) {
    let dir = std::env::temp_dir().join(format!("p2pcore-devices-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let events = Arc::new(Count::default());
    let node = Node::new(dir.to_string_lossy().into(), events.clone()).unwrap();
    let phrase = match phrase {
        Some(p) => {
            node.restore_identity(p.into(), tag.into(), PASS.into()).unwrap();
            p.to_string()
        }
        None => node.create_identity(tag.into(), PASS.into()).unwrap(),
    };
    node.start().unwrap();
    let t = Instant::now();
    while !node.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    let did = node.profile().unwrap().did;
    (Peer { node, events, did, dir }, phrase)
}

fn wait(what: &str, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(20), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// S4: a device we have not met before, arriving through an authenticated call, resets
/// `verified` exactly as when it adds us, and the UI is told.
#[test]
fn unfamiliar_device_in_a_call_resets_verified_and_notifies() {
    let (a, _) = peer("s4-a", None);
    let (b1, phrase) = peer("s4-b1", None);
    b1.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    assert_eq!(a.node.contacts()[0].did, b1.did);
    let d1 = a.node.contacts()[0].device.clone();

    // Four more installs of B redeem tickets of A: A keeps four devices per contact, so b1's
    // falls out of its list. b1 still holds A's grant and can call.
    let mut others = Vec::new();
    for i in 2..=5 {
        let (bi, _) = peer(&format!("s4-b{i}"), Some(&phrase));
        assert_eq!(bi.did, b1.did);
        bi.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
        others.push(bi);
    }
    assert_ne!(a.node.contacts()[0].device, d1);
    // Adding a new device through a ticket already resets it (the existing rule).
    assert!(!a.node.contacts()[0].verified);

    a.node.set_verified(b1.did.clone(), true).unwrap();
    assert!(a.node.contacts()[0].verified);
    let changed = a.events.contacts_changed.load(Ordering::SeqCst);

    b1.node.call(a.did.clone()).unwrap();
    wait("the call to reach A", || a.events.incoming.load(Ordering::SeqCst) > 0);
    wait("verification reset", || !a.node.contacts()[0].verified);
    assert_eq!(a.node.contacts()[0].device, d1, "the calling device is now the first to dial");
    assert!(a.events.contacts_changed.load(Ordering::SeqCst) > changed, "the UI was told");

    // And it stuck: a restart of A still shows it unverified.
    a.node.lock();
    a.node.unlock(PASS.into()).unwrap();
    assert!(!a.node.contacts()[0].verified);
    drop(others);
}
