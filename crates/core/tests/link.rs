//! Task 34d: linking a device, own-device sync and unlink, with real endpoints.

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, LinkEvents, Node, NodeEvents, NodeStatus};

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
    let e = make_peer("h1-e");
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
}

fn dev(p: &Peer) -> String {
    p.node.device_key_for_test().unwrap()
}

fn connect(a: &Peer, b: &Peer) {
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
}

#[test]
fn link_n_shows_e_scans_and_contacts_learn_the_new_device() {
    let a = make_peer("h2-a");
    let e = make_peer("h2-e");
    connect(&a, &e);
    let n = link_fresh(&e, "h2-n", false);
    eventually(40, "N has the contact", || n.node.contacts().iter().any(|c| c.did == a.did));
    // A learns N from the list that rides on the answer of a call to E.
    let call = a.node.call(e.did.clone()).unwrap();
    incoming(&e, 30);
    e.node.answer(call.call_id.clone()).unwrap();
    eventually(20, "active", || a.node.current_call().is_some());
    std::thread::sleep(Duration::from_millis(500));
    a.node.hangup(call.call_id.clone()).unwrap();
    ended(&e, &call.call_id, 20);
    eventually(20, "A knows both devices", || a.node.contact_devices_for_test(e.did.clone()).contains(&dev(&n)));
    // Next call rings both.
    let call2 = a.node.call(e.did.clone()).unwrap();
    assert_eq!(incoming(&e, 30), call2.call_id);
    assert_eq!(incoming(&n, 30), call2.call_id);
    a.node.hangup(call2.call_id.clone()).unwrap();
    // The history converges to one record per call, the better one winning.
    eventually(30, "histories agree", || {
        let r = |p: &Peer| p.node.recent_calls(50).into_iter().filter(|r| r.call_id == call.call_id).map(|r| r.reason).collect::<Vec<_>>();
        let (re, rn) = (r(&e), r(&n));
        re.len() == 1 && re == rn
    });
}

#[test]
fn wrong_passphrase_on_approve_can_retry_and_cancel_ends_the_link() {
    let e = make_peer("h3-e");
    let n = blank("h3-n");
    let qr = e.node.link_show_qr().unwrap();
    // A second link cannot start meanwhile.
    assert!(e.node.link_show_qr().is_err());
    n.node.link_new_scan(qr, "Pixel".into()).unwrap();
    code_of(&e);
    code_of(&n);
    assert!(matches!(e.node.link_approve(Some("nope".into())), Err(p2pcore::Error::WrongPassphrase)));
    assert!(matches!(e.node.link_approve(None), Err(p2pcore::Error::WrongPassphrase)));
    e.node.link_cancel();
    assert_eq!(wait_for(&e, 20, "E failed", |ev| match ev { LEv::Failed(r) => Some(r.clone()), _ => None }), "cancelled");
    assert_eq!(wait_for(&n, 20, "N failed", |ev| match ev { LEv::Failed(r) => Some(r.clone()), _ => None }), "rejected");
    assert!(!n.node.has_identity());
    // The slot is free again.
    e.node.link_show_qr().unwrap();
    e.node.link_cancel();
}

#[test]
fn wrong_secret_burns_the_qr_and_a_second_scanner_gets_nothing() {
    let e = make_peer("h4-e");
    let qr_text = e.node.link_show_qr().unwrap();
    let mut qr = proto::LinkQr::from_text(&qr_text).unwrap();
    qr.secret[0] ^= 1;
    let bad = blank("h4-n1");
    bad.node.link_new_scan(qr.to_text(), "X".into()).unwrap();
    let r = wait_for(&bad, 30, "bad scanner failed", |ev| match ev { LEv::Failed(r) => Some(r.clone()), _ => None });
    assert!(r == "rejected" || r == "bad_proof", "{r}");
    assert_eq!(wait_for(&e, 30, "E failed", |ev| match ev { LEv::Failed(r) => Some(r.clone()), _ => None }), "bad_proof");
    // The QR is spent: even the right secret no longer works.
    let good = blank("h4-n2");
    good.node.link_new_scan(qr_text, "Y".into()).unwrap();
    wait_for(&good, 30, "good scanner failed", |ev| match ev { LEv::Failed(r) => Some(r.clone()), _ => None });
    assert!(!good.node.has_identity() && !bad.node.has_identity());
    assert_eq!(e.node.linked_devices().len(), 1);
}

#[test]
fn only_one_of_two_racing_scanners_gets_through() {
    let e = make_peer("h5-e");
    let qr = e.node.link_show_qr().unwrap();
    let n1 = blank("h5-n1");
    let n2 = blank("h5-n2");
    n1.node.link_new_scan(qr.clone(), "One".into()).unwrap();
    n2.node.link_new_scan(qr, "Two".into()).unwrap();
    let got = |p: &Peer| {
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            match p.lrx.recv_timeout(end.saturating_duration_since(Instant::now())) {
                Ok(LEv::Code(..)) => return true,
                Ok(LEv::Failed(_)) => return false,
                Ok(_) => {}
                Err(_) => panic!("neither code nor failure"),
            }
        }
    };
    let (r1, r2) = (got(&n1), got(&n2));
    assert!(r1 ^ r2, "exactly one scanner is let in ({r1}, {r2})");
    e.node.link_cancel();
}

#[test]
fn locked_e_cannot_approve() {
    let e = make_peer("h6-e");
    let n = blank("h6-n");
    let qr = n.node.link_new_show_qr("Pixel".into()).unwrap();
    e.node.link_scan(qr).unwrap();
    code_of(&e);
    code_of(&n);
    e.node.lock();
    assert!(matches!(e.node.link_approve(Some(PASS.into())), Err(p2pcore::Error::Locked)));
    // Locking ended the link on E's side; N is told.
    wait_for(&n, 30, "N failed", |ev| matches!(ev, LEv::Failed(_)).then_some(()));
    assert!(!n.node.has_identity());
}
