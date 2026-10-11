//! P10b: our own messages written under a writer ID that is no longer this install's (a restore
//! over an existing data directory keeps `chat.redb` but makes a new device ID, hence a new writer
//! ID; or another install of the account wrote them) still reach the contact. A device sends a
//! contact every op it lacks that any of our writer IDs wrote (each proven by our DID signature,
//! P9c), not only the ops of its current writer ID.
//!
//! Real iroh endpoints (needs network, like `restore.rs`, `link.rs`). Helpers are copied from
//! `rotation_restore_characterization.rs` (the tests share no module).

#![allow(dead_code)]

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, DeliveryState, LinkEvents, Message, Node, NodeEvents, NodeStatus};

const PASS: &str = "test-passphrase";

#[derive(Debug, Clone)]
enum Ev {
    Incoming(CallInfo),
    #[allow(dead_code)]
    State(String, CallState),
}

struct Tap(Mutex<mpsc::Sender<Ev>>, Arc<Mutex<Vec<String>>>);
impl NodeEvents for Tap {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, c: CallInfo) {
        let _ = self.0.lock().unwrap().send(Ev::Incoming(c));
    }
    fn on_call_state(&self, id: String, s: CallState) {
        let _ = self.0.lock().unwrap().send(Ev::State(id, s));
    }
    fn on_log(&self, line: String) {
        self.1.lock().unwrap().push(line);
    }
}

#[derive(Debug, Clone, PartialEq)]
enum LEv {
    Code(String),
    Done(String),
    Other,
}

struct LTap(Mutex<mpsc::Sender<LEv>>);
impl LinkEvents for LTap {
    fn on_link_code(&self, code: String, _: String) {
        let _ = self.0.lock().unwrap().send(LEv::Code(code));
    }
    fn on_link_done(&self, did: String, _: String) {
        let _ = self.0.lock().unwrap().send(LEv::Done(did));
    }
    fn on_link_failed(&self, _: String) {
        let _ = self.0.lock().unwrap().send(LEv::Other);
    }
    fn on_devices_changed(&self) {
        let _ = self.0.lock().unwrap().send(LEv::Other);
    }
    fn on_history_changed(&self) {
        let _ = self.0.lock().unwrap().send(LEv::Other);
    }
    fn on_unlinked(&self, _: String) {
        let _ = self.0.lock().unwrap().send(LEv::Other);
    }
}

struct Peer {
    node: Arc<Node>,
    rx: mpsc::Receiver<Ev>,
    lrx: mpsc::Receiver<LEv>,
    logs: Arc<Mutex<Vec<String>>>,
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

impl Peer {
    fn logged(&self, needle: &str) -> bool {
        self.logs.lock().unwrap().iter().any(|l| l.contains(needle))
    }

    /// Whether this node refused any chat batch, history snapshot or own-device batch.
    fn refused_chat(&self) -> bool {
        ["rejected chat batch", "rejected history", "rejected own-device batch"].iter().any(|n| self.logged(n))
    }
}

type Opened = (Arc<Node>, mpsc::Receiver<Ev>, mpsc::Receiver<LEv>, Arc<Mutex<Vec<String>>>);

fn open(dir: &std::path::Path) -> Opened {
    let (tx, rx) = mpsc::channel();
    let (ltx, lrx) = mpsc::channel();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Tap(Mutex::new(tx), logs.clone()))).unwrap();
    node.set_link_events(Arc::new(LTap(Mutex::new(ltx))));
    (node, rx, lrx, logs)
}

fn blank(name: &str) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-p10b-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (node, rx, lrx, logs) = open(&dir);
    Peer { node, rx, lrx, logs, did: String::new(), phrase: String::new(), dir }
}

fn online(n: &Node) {
    n.start().unwrap();
    let t = Instant::now();
    while !n.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A committed identity; started (`online`) or not.
fn peer(name: &str, started: bool) -> Peer {
    let mut p = blank(name);
    p.phrase = p.node.create_identity(name.into(), PASS.into()).unwrap();
    if started {
        online(&p.node);
    }
    p.did = p.node.profile().unwrap().did;
    p
}

/// The forgotten-passphrase restore of `restore.rs` (`contacts_history_and_chats_survive_…`):
/// stop, drop the node, open the same data directory (locked) and restore over it with the
/// phrase and the same name. Left NOT started. Asserts the DID is kept and the device key is not.
fn restore_in_place(p: &mut Peer) -> (String, String) {
    let name = p.node.profile().unwrap().name;
    let old_dev = p.node.device_key_for_test().unwrap();
    p.node.stop();
    let (node, rx, lrx, logs) = open(&p.dir);
    let old = std::mem::replace(&mut p.node, node);
    drop(old);
    p.rx = rx;
    p.lrx = lrx;
    p.logs = logs;
    p.node.restore_identity(p.phrase.clone(), name, "pass two".into()).unwrap();
    assert_eq!(p.node.profile().unwrap().did, p.did, "restore keeps the DID");
    let new_dev = p.node.device_key_for_test().unwrap();
    assert_ne!(new_dev, old_dev, "restore makes a fresh device key (restore.rs:104)");
    (old_dev, new_dev)
}

fn wait_link(p: &Peer, what: &str, mut f: impl FnMut(&LEv) -> Option<String>) -> String {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        match p.lrx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(ev) => {
                if let Some(t) = f(&ev) {
                    return t;
                }
            }
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
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

/// `e` (started) links a fresh install of its account (copied from `link.rs` `link_fresh`).
fn link_fresh(e: &Peer, name: &str) -> Peer {
    let n = blank(name);
    let qr = e.node.link_show_qr().unwrap();
    n.node.link_new_scan(qr, "Pixel".into()).unwrap();
    let code = |ev: &LEv| match ev {
        LEv::Code(c) => Some(c.clone()),
        _ => None,
    };
    assert_eq!(wait_link(e, "E code", code), wait_link(&n, "N code", code));
    e.node.link_approve(Some(PASS.into())).unwrap();
    let did = wait_link(&n, "N done", |ev| match ev {
        LEv::Done(d) => Some(d.clone()),
        _ => None,
    });
    wait_link(e, "E done", |ev| matches!(ev, LEv::Done(..)).then(String::new));
    n.node.set_passphrase(None, "n-pass".into()).unwrap();
    online(&n.node);
    let mut n = n;
    n.did = did;
    n
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

fn ticks(p: &Peer, did: &str, id: &str) -> DeliveryState {
    all_msgs(p, did).into_iter().find(|m| m.id == id).expect("message held").delivery
}


fn creator(p: &Peer, did: &str, id: &str) -> u64 {
    p.node.chat_doc_peers_for_test(did.to_string()).1.into_iter().find(|(m, _)| m == id).map(|(_, w)| w).expect("message held")
}

fn count(p: &Peer, did: &str, id: &str) -> usize {
    all_msgs(p, did).iter().filter(|m| m.id == id).count()
}

/// Stop, drop the node, open the same directory and unlock it (a process restart). Left stopped.
fn restart(p: &mut Peer, pass: &str) {
    p.node.stop();
    let (node, rx, lrx, logs) = open(&p.dir);
    drop(std::mem::replace(&mut p.node, node));
    p.rx = rx;
    p.lrx = lrx;
    p.logs = logs;
    p.node.unlock(pass.into()).unwrap();
}

/// B (contact of A) sends `m1` (delivered), then `m2` while A is offline. Returns `(a, b, m2,
/// B's writer ID at the time)`, both left as they are: A stopped, B started.
fn pending_m2(tag: &str) -> (Peer, Peer, String, u64) {
    let a = peer(&format!("{tag}-a"), true);
    let b = peer(&format!("{tag}-b"), true);
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
    restore_in_place(&mut b);
    online(&b.node);
    m2_arrives(&a, &b, &m2, old_wid);
}

// (2) The same with a process restart of B after the restore, before A returns: the pending day
// and B's old writer ID are known from the store, not from memory.
#[test]
fn a_message_pending_at_a_restore_is_delivered_after_a_restart() {
    let (a, mut b, m2, old_wid) = pending_m2("p10b-rr");
    restore_in_place(&mut b);
    online(&b.node);
    std::thread::sleep(Duration::from_millis(500));
    restart(&mut b, "pass two");
    online(&b.node);
    m2_arrives(&a, &b, &m2, old_wid);
}

// (3) Another install of the same account forwards a message the first one wrote while the
// contact was offline: B1 writes `m2`, B2 receives it by own-device sync, B1 is stopped, unlinked
// and wiped, then A comes online and gets `m2` from B2 (proven by B's DID signature).
#[test]
fn a_sibling_device_forwards_an_undelivered_message_of_an_unlinked_device() {
    let a = peer("p10b-s-a", true);
    let b1 = peer("p10b-s-b1", true);
    b1.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let b2 = link_fresh(&b1, "p10b-s-b2");
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
    let a = peer("p10b-l-a", true);
    let b = peer("p10b-l-b", true);
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
