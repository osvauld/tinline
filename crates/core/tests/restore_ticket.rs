//! P2: the contact ticket after a restore from the recovery phrase on an existing install.
//!
//! Restore over an existing account keeps `account.redb` (and with it the cached `State.ticket`)
//! but makes a fresh device key (`node.rs` `set_identity`). A cached ticket names the device key
//! a joiner dials, so it must never outlive that key. See
//! `docs/design/rotation-baseline-report.md` finding O1 and
//! `docs/design/rotation-open-decisions.md` section 5 bug 2.
//!
//! Every test carries one label:
//! - `KIND: invariant`: the correct outcome; must keep holding.
//! - `KIND: limitation`: today's accepted outcome, with the better one named in the comment.
//!
//! Real iroh endpoints (needs network, like `restore.rs`). Waits are bounded polls. Helpers are
//! copied from `rotation_restore_characterization.rs` / `restore.rs` (no shared test module).

use std::sync::Arc;
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, Message, Node, NodeEvents, NodeStatus};

const PASS: &str = "test-passphrase";

struct Tap;
impl NodeEvents for Tap {
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
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Tap)).unwrap();
    node.auto_accept_contacts_for_test(true);
    node
}

fn online(n: &Node) {
    n.start().unwrap();
    let t = Instant::now();
    while !n.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A committed identity (with a passphrase, so it is written at once); started or not.
fn peer(name: &str, started: bool) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-p2-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let node = open(&dir);
    let phrase = node.create_identity(name.into(), PASS.into()).unwrap();
    if started {
        online(&node);
    }
    let did = node.profile().unwrap().did;
    Peer { node, did, phrase, dir }
}

/// Stops the node and opens the same data directory again (locked).
fn reopen(p: &mut Peer) {
    p.node.stop();
    let old = std::mem::replace(&mut p.node, open(&p.dir));
    drop(old);
}

/// The forgotten-passphrase restore (`restore.rs`): reopen, then restore over the existing
/// account with the phrase and the same name. Left NOT started. Returns (old, new) device keys.
fn restore_in_place(p: &mut Peer) -> (String, String) {
    let name = p.node.profile().unwrap().name;
    let old_dev = p.node.device_key_for_test().unwrap();
    reopen(p);
    p.node.restore_identity(p.phrase.clone(), name, "pass two".into()).unwrap();
    assert_eq!(p.node.profile().unwrap().did, p.did, "restore keeps the DID");
    let new_dev = p.node.device_key_for_test().unwrap();
    assert_ne!(new_dev, old_dev, "restore makes a fresh device key");
    (old_dev, new_dev)
}

fn claim(ticket: &str) -> proto::InviteClaim {
    proto::ContactTicket::from_text(ticket).unwrap().verify(now()).unwrap()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
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

fn has(p: &Peer, did: &str, id: &str) -> bool {
    all_msgs(p, did).iter().any(|m| m.id == id)
}

// KIND: invariant
// After a restore that replaced the device key, `my_ticket()` never hands out the cached ticket
// that names the dead key: it mints one naming the current key (checked on the not-yet-started
// node, where no relay is known and the relay check cannot force a re-issue by itself, and again
// once online). A contact redeems the live one and both sides can chat.
#[test]
fn after_restore_my_ticket_names_the_current_device_key_and_redeems() {
    let mut b = peer("new-b", true);
    let c = peer("new-c", true);
    let before = b.node.my_ticket().unwrap();
    let (old_dev, new_dev) = restore_in_place(&mut b);
    assert_eq!(claim(&before).device, old_dev);

    let offline = b.node.my_ticket().unwrap();
    assert_ne!(offline, before, "the pre-restore ticket is not handed out again");
    assert_eq!(claim(&offline).iss, b.did);
    assert_eq!(claim(&offline).device, new_dev, "it names the current device key");

    online(&b.node);
    let live = b.node.my_ticket().unwrap();
    assert_eq!(claim(&live).device, new_dev);
    assert_ne!(live, before);

    let added = c.node.add_contact(live).expect("the fresh ticket redeems");
    assert_eq!(added.did, b.did);
    eventually(30, "B lists C", || b.node.contacts().iter().any(|x| x.did == c.did));

    let m1 = c.node.send_text(b.did.clone(), "hello restored B".into(), None).unwrap().id;
    eventually(60, "B holds C's message", || has(&b, &c.did, &m1));
    let m2 = b.node.send_text(c.did.clone(), "hello C".into(), None).unwrap().id;
    eventually(60, "C holds B's message", || has(&c, &b.did, &m2));
}

// KIND: invariant
// No needless churn. A restore over an existing install always makes a fresh device key
// (`set_identity` calls `new_device_secret()`), so the "same key" case is reopening and
// unlocking the same install: the device key is unchanged and the cached ticket is handed out
// as before. Also, once re-issued after a restore, the new ticket is stable across calls.
#[test]
fn unchanged_device_key_keeps_the_cached_ticket() {
    let mut b = peer("same-b", true);
    let before = b.node.my_ticket().unwrap();
    let dev = b.node.device_key_for_test().unwrap();
    reopen(&mut b);
    b.node.unlock(PASS.into()).unwrap();
    assert_eq!(b.node.device_key_for_test().unwrap(), dev, "unlock keeps the device key");
    assert_eq!(b.node.my_ticket().unwrap(), before, "same key: same ticket");

    restore_in_place(&mut b);
    let first = b.node.my_ticket().unwrap();
    assert_eq!(b.node.my_ticket().unwrap(), first, "re-issued once, then stable");
    reopen(&mut b);
    b.node.unlock("pass two".into()).unwrap();
    assert_eq!(b.node.my_ticket().unwrap(), first, "and it survives a restart");
}

// KIND: limitation
// A ticket shared before the restore names the dead device key; nobody holds that key any more,
// so redeeming it cannot reach B. It fails (bounded: the dial times out, observed ~20 s) and
// creates no contact on either side, and B no longer counts its nonce as current, so even a
// joiner that reached B would be refused ("not the current ticket").
// Better (not done here, needs a policy/discovery decision): a prompt, specific "this ticket is
// no longer valid" instead of a dial timeout indistinguishable from "B is offline".
#[test]
fn a_pre_restore_ticket_fails_without_creating_a_contact() {
    let mut b = peer("old-b", true);
    let c = peer("old-c", true);
    let before = b.node.my_ticket().unwrap();
    restore_in_place(&mut b);
    online(&b.node);
    assert_ne!(b.node.my_ticket().unwrap(), before, "B hands out a different ticket now");

    let t = Instant::now();
    let r = c.node.add_contact(before);
    assert!(r.is_err(), "the pre-restore ticket cannot be redeemed: {r:?}");
    assert!(t.elapsed() < Duration::from_secs(90), "fails within a bound, took {:?}", t.elapsed());
    assert!(c.node.contacts().is_empty(), "C added nobody");
    assert!(b.node.contacts().is_empty(), "B added nobody");
}
