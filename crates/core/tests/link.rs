//! Task 34d: linking a device, own-device sync and unlink, with real endpoints.

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, Error, LinkEvents, Node, NodeEvents, NodeStatus};

pub const PASS: &str = "test-passphrase";

#[derive(Debug, Clone)]
pub enum Ev {
    Incoming(CallInfo),
    State(String, CallState),
}

struct Tap(Mutex<mpsc::Sender<Ev>>);
impl NodeEvents for Tap {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, c: CallInfo) {
        let _ = self.0.lock().unwrap().send(Ev::Incoming(c));
    }
    fn on_call_state(&self, id: String, s: CallState) {
        let _ = self.0.lock().unwrap().send(Ev::State(id, s));
    }
    fn on_log(&self, _: String) {}
}

#[derive(Debug, Clone, PartialEq)]
pub enum LEv {
    Code(String, String),
    Done(String, String),
    Failed(String),
    Devices,
    History,
    Unlinked(String),
}

struct LTap(Mutex<mpsc::Sender<LEv>>);
impl LinkEvents for LTap {
    fn on_link_code(&self, code: String, peer_label: String) {
        let _ = self.0.lock().unwrap().send(LEv::Code(code, peer_label));
    }
    fn on_link_done(&self, did: String, name: String) {
        let _ = self.0.lock().unwrap().send(LEv::Done(did, name));
    }
    fn on_link_failed(&self, reason: String) {
        let _ = self.0.lock().unwrap().send(LEv::Failed(reason));
    }
    fn on_devices_changed(&self) {
        let _ = self.0.lock().unwrap().send(LEv::Devices);
    }
    fn on_history_changed(&self) {
        let _ = self.0.lock().unwrap().send(LEv::History);
    }
    fn on_unlinked(&self, did: String) {
        let _ = self.0.lock().unwrap().send(LEv::Unlinked(did));
    }
}

pub struct Peer {
    pub node: Arc<Node>,
    pub rx: mpsc::Receiver<Ev>,
    pub lrx: mpsc::Receiver<LEv>,
    pub did: String,
    pub dir: std::path::PathBuf,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.node.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn blank(name: &str) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-link-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = mpsc::channel();
    let (ltx, lrx) = mpsc::channel();
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Tap(Mutex::new(tx)))).unwrap();
    node.set_link_events(Arc::new(LTap(Mutex::new(ltx))));
    Peer { node, rx, lrx, did: String::new(), dir }
}

pub fn online(n: &Node) {
    n.start().unwrap();
    let t = Instant::now();
    while !n.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn make_peer(name: &str) -> Peer {
    let mut p = blank(name);
    p.node.create_identity(name.into(), PASS.into()).unwrap();
    online(&p.node);
    p.did = p.node.profile().unwrap().did;
    p
}

pub fn wait_for<T>(p: &Peer, secs: u64, what: &str, mut f: impl FnMut(&LEv) -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(secs);
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

pub fn eventually(secs: u64, what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("timed out waiting for {what}");
}

pub fn expect<T>(p: &Peer, secs: u64, what: &str, mut f: impl FnMut(&Ev) -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p.rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(ev) => {
                if let Some(t) = f(&ev) {
                    return t;
                }
            }
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
}

pub fn incoming(p: &Peer, secs: u64) -> String {
    expect(p, secs, "incoming", |e| match e {
        Ev::Incoming(c) => Some(c.call_id.clone()),
        _ => None,
    })
}

pub fn ended(p: &Peer, id: &str, secs: u64) -> String {
    expect(p, secs, "ended", |e| match e {
        Ev::State(i, CallState::Ended { reason }) if i == id => Some(reason.clone()),
        _ => None,
    })
}

pub fn code_of(p: &Peer) -> (String, String) {
    wait_for(p, 30, "link code", |e| match e {
        LEv::Code(c, l) => Some((c.clone(), l.clone())),
        _ => None,
    })
}

/// E (existing, unlocked) links a fresh N. `e_shows`: which side displays the QR. Returns N, already
/// committed (with a passphrase) and started.
pub fn link_fresh(e: &Peer, name: &str, e_shows: bool) -> Peer {
    let n = blank(name);
    if e_shows {
        let qr = e.node.link_show_qr().unwrap();
        n.node.link_new_scan(qr, "Pixel".into()).unwrap();
    } else {
        let qr = n.node.link_new_show_qr("Pixel".into()).unwrap();
        e.node.link_scan(qr).unwrap();
    }
    let (ce, le) = code_of(e);
    let (cn, ln) = code_of(&n);
    assert_eq!(ce, cn, "the same code on both screens");
    assert_eq!(le, "Pixel");
    assert_eq!(ln, e.node.device_label().unwrap_or_default());
    e.node.link_approve(Some(PASS.into())).unwrap();
    let (did, _) = wait_for(&n, 30, "N done", |ev| match ev {
        LEv::Done(d, nm) => Some((d.clone(), nm.clone())),
        _ => None,
    });
    assert_eq!(did, e.did);
    wait_for(e, 30, "E done", |ev| matches!(ev, LEv::Done(..)).then_some(()));
    n.node.set_passphrase(None, "n-pass".into()).unwrap();
    online(&n.node);
    let mut n = n;
    n.did = did;
    n
}

#[test]
fn link_e_shows_n_scans_and_data_arrives() {
    let a = make_peer("h1-a");
    let mut e = make_peer("h1-e");
    e.node.set_device_label("Desktop".into()).unwrap();
    e.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    // A history record: A calls E, E declines.
    let call = a.node.call(e.did.clone()).unwrap();
    let id = incoming(&e, 30);
    assert_eq!(id, call.call_id);
    e.node.decline(id.clone()).unwrap();
    ended(&e, &id, 20);

    let n = link_fresh(&e, "h1-n", true);
    assert_eq!(n.did, e.did);
    assert_ne!(n.node.profile().unwrap().device, e.node.profile().unwrap().device);
    eventually(40, "N has the contact and the call", || {
        n.node.contacts().iter().any(|c| c.did == a.did) && n.node.recent_calls(10).iter().any(|r| r.call_id == id)
    });
    for node in [&e.node, &n.node] {
        let devs = node.linked_devices();
        assert_eq!(devs.len(), 2, "{devs:?}");
        assert!(devs.iter().any(|d| d.label == "Pixel") && devs.iter().all(|d| !d.removed));
        assert!(devs[0].this_device);
    }
    e.did = e.did.clone();
}
