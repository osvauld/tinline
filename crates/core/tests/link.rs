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

fn same<T: PartialEq + std::fmt::Debug>(secs: u64, what: &str, f: impl Fn() -> (T, T)) {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        let (a, b) = f();
        if a == b {
            return;
        }
        if Instant::now() > end {
            panic!("{what}: {a:?} != {b:?}");
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn alias(p: &Peer, did: &str) -> Option<Option<String>> {
    p.node.contacts().into_iter().find(|c| c.did == did).map(|c| c.alias)
}

fn verified(p: &Peer, did: &str) -> Option<bool> {
    p.node.contacts().into_iter().find(|c| c.did == did).map(|c| c.verified)
}

#[test]
fn concurrent_contact_edits_converge_and_removal_wins() {
    let a = make_peer("s1-a");
    let e = make_peer("s1-e");
    connect(&a, &e);
    let n = link_fresh(&e, "s1-n", true);
    eventually(40, "N has A", || n.node.contacts().iter().any(|c| c.did == a.did));

    // Edits of different fields on both devices at once: both survive.
    e.node.rename_contact(a.did.clone(), Some("Al".into())).unwrap();
    n.node.set_verified(a.did.clone(), true).unwrap();
    same(30, "alias", || (alias(&e, &a.did), alias(&n, &a.did)));
    same(30, "verified", || (verified(&e, &a.did), verified(&n, &a.did)));
    assert_eq!(alias(&n, &a.did), Some(Some("Al".into())));
    assert_eq!(verified(&e, &a.did), Some(true));

    // N offline: it edits the contact while E removes it. The removal stands.
    n.node.stop();
    n.node.rename_contact(a.did.clone(), Some("Stale".into())).unwrap();
    e.node.remove_contact(a.did.clone()).unwrap();
    n.node.start().unwrap();
    n.node.sync_now_for_test();
    eventually(40, "N drops A", || !n.node.contacts().iter().any(|c| c.did == a.did));
    assert!(e.node.blocked_for_test().contains(&a.did));
    eventually(20, "N blocks A", || n.node.blocked_for_test().contains(&a.did));

    // A later ticket scan brings the contact back on both devices.
    std::thread::sleep(Duration::from_millis(1200));
    e.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    eventually(40, "N has A again", || n.node.contacts().iter().any(|c| c.did == a.did));
    assert!(!n.node.blocked_for_test().contains(&a.did) && !e.node.blocked_for_test().contains(&a.did));
}

#[test]
fn redeemed_nonces_union_and_strangers_cannot_open_self_sync() {
    let a = make_peer("s2-a");
    let x = make_peer("s2-x");
    let e = make_peer("s2-e");
    connect(&a, &e);
    let n = link_fresh(&e, "s2-n", true);
    // X redeems E's ticket: the nonce must reach N, which then refuses the same ticket.
    x.node.add_contact(e.node.my_ticket().unwrap()).unwrap();
    let nonces = e.node.redeemed_for_test();
    assert!(!nonces.is_empty());
    eventually(40, "N has the nonce", || n.node.redeemed_for_test() == nonces);
    // A contact (other DID) is not let into tinline/self/1; our own device is.
    assert!(!a.node.self_probe_for_test(dev(&e)));
    assert!(!x.node.self_probe_for_test(dev(&n)));
    assert!(n.node.self_probe_for_test(dev(&e)));
}

#[test]
fn unlink_an_online_device() {
    let a = make_peer("u1-a");
    let e = make_peer("u1-e");
    connect(&a, &e);
    let n = link_fresh(&e, "u1-n", true);
    let nd = dev(&n);
    // A learns N, then N is unlinked.
    let call = a.node.call(e.did.clone()).unwrap();
    incoming(&e, 30);
    e.node.answer(call.call_id.clone()).unwrap();
    eventually(20, "active", || a.node.current_call().is_some());
    std::thread::sleep(Duration::from_millis(500));
    a.node.hangup(call.call_id.clone()).unwrap();
    ended(&e, &call.call_id, 20);
    eventually(20, "A knows N", || a.node.contact_devices_for_test(e.did.clone()).contains(&nd));
    assert!(matches!(e.node.unlink_device(nd.clone(), Some("wrong".into())), Err(p2pcore::Error::WrongPassphrase)));
    e.node.unlink_device(nd.clone(), Some(PASS.into())).unwrap();
    wait_for(&n, 30, "N unlinked", |ev| matches!(ev, LEv::Unlinked(_)).then_some(()));
    assert!(!n.node.has_identity() && n.node.accounts().is_empty());
    let devs = e.node.linked_devices();
    assert!(devs.iter().any(|d| d.device == nd && d.removed));
    // The next call to E teaches A the shorter list.
    let call = a.node.call(e.did.clone()).unwrap();
    incoming(&e, 30);
    e.node.answer(call.call_id.clone()).unwrap();
    eventually(20, "active", || a.node.current_call().is_some());
    std::thread::sleep(Duration::from_millis(500));
    a.node.hangup(call.call_id.clone()).unwrap();
    eventually(20, "A forgets N", || !a.node.contact_devices_for_test(e.did.clone()).contains(&nd));
}

#[test]
fn an_offline_device_learns_it_was_unlinked_when_it_returns() {
    let e = make_peer("u2-e");
    let n = link_fresh(&e, "u2-n", false);
    let nd = dev(&n);
    n.node.stop();
    std::thread::sleep(Duration::from_millis(500));
    e.node.unlink_device(nd.clone(), None).unwrap_err(); // the passphrase is required
    e.node.unlink_device(nd.clone(), Some(PASS.into())).unwrap();
    // N comes back: its first dial to E is answered with "Unlinked", and it drops the account.
    n.node.start().unwrap();
    n.node.sync_now_for_test();
    wait_for(&n, 60, "N unlinked", |ev| matches!(ev, LEv::Unlinked(_)).then_some(()));
    assert!(!n.node.has_identity());
    // And E would not sync with it: a probe as N is refused (N is gone, so check E's view).
    assert!(e.node.linked_devices().iter().any(|d| d.device == nd && d.removed));
}

#[test]
fn unlinking_this_device_removes_the_account_here_and_tombstones_it_there() {
    let e = make_peer("u3-e");
    let n = link_fresh(&e, "u3-n", true);
    let nd = dev(&n);
    n.node.unlink_device(nd.clone(), Some("n-pass".into())).unwrap();
    wait_for(&n, 20, "N unlinked", |ev| matches!(ev, LEv::Unlinked(_)).then_some(()));
    assert!(n.node.accounts().is_empty());
    eventually(30, "E tombstones N", || e.node.linked_devices().iter().any(|d| d.device == nd && d.removed));
}

#[test]
fn three_devices_converge_after_offline_edits() {
    let a = make_peer("t1-a");
    let b = make_peer("t1-b");
    let e = make_peer("t1-e");
    connect(&a, &e);
    connect(&b, &e);
    let n1 = link_fresh(&e, "t1-n1", true);
    let n2 = link_fresh(&e, "t1-n2", false);
    for p in [&n1, &n2] {
        eventually(60, "contacts everywhere", || p.node.contacts().len() == 2);
    }
    eventually(60, "three devices everywhere", || [&e, &n1, &n2].iter().all(|p| p.node.linked_devices().iter().filter(|d| !d.removed).count() == 3));
    n2.node.stop();
    e.node.rename_contact(a.did.clone(), Some("Alpha".into())).unwrap();
    n1.node.set_verified(b.did.clone(), true).unwrap();
    n1.node.rename_contact(b.did.clone(), Some("Beta".into())).unwrap();
    n2.node.start().unwrap();
    n2.node.sync_now_for_test();
    for p in [&e, &n1, &n2] {
        let (a_did, b_did) = (a.did.clone(), b.did.clone());
        eventually(60, "converged", || {
            alias(p, &a_did) == Some(Some("Alpha".into()))
                && alias(p, &b_did) == Some(Some("Beta".into()))
                && verified(p, &b_did) == Some(true)
        });
    }
}

// ---- task 34g: chats and "You" items between own devices ---------------------------------------

use p2pcore::{DeliveryState, Message, TransferState};

fn all_msgs(p: &Peer, did: &str) -> Vec<Message> {
    let mut out = Vec::new();
    let mut page = p.node.chat_day(did.to_string(), None).ok();
    while let Some(pg) = page {
        out.extend(pg.messages.clone());
        page = pg.older_day.and_then(|d| p.node.chat_day(did.to_string(), Some(d)).ok());
    }
    out
}

fn find_msg(p: &Peer, did: &str, text: &str) -> Option<Message> {
    all_msgs(p, did).into_iter().find(|m| m.text == text)
}

fn count_msgs(p: &Peer, did: &str, text: &str) -> usize {
    all_msgs(p, did).iter().filter(|m| m.text == text).count()
}

fn unread(p: &Peer, did: &str) -> u32 {
    p.node.chats().unwrap().into_iter().find(|c| c.peer_did == did).map_or(0, |c| c.unread)
}

/// A with a contact B who has two devices (b1 original, b2 linked).
fn trio(tag: &str) -> (Peer, Peer, Peer) {
    let a = make_peer(&format!("{tag}-a"));
    let b1 = make_peer(&format!("{tag}-b1"));
    connect(&a, &b1);
    let b2 = link_fresh(&b1, &format!("{tag}-b2"), true);
    eventually(40, "b2 has A", || b2.node.contacts().iter().any(|c| c.did == a.did));
    eventually(40, "two devices everywhere", || [&b1, &b2].iter().all(|p| p.node.linked_devices().iter().filter(|d| !d.removed).count() == 2));
    (a, b1, b2)
}

#[test]
fn chat_reaches_both_devices_and_replies_show_as_outgoing() {
    let (a, b1, b2) = trio("c1");
    a.node.send_text(b1.did.clone(), "hello B".into(), None).unwrap();
    for p in [&b1, &b2] {
        eventually(40, "B device has hello", || find_msg(p, &a.did, "hello B").is_some());
        assert!(!find_msg(p, &a.did, "hello B").unwrap().outgoing);
    }
    b1.node.send_text(a.did.clone(), "reply from b1".into(), None).unwrap();
    eventually(40, "b2 shows the reply as outgoing", || find_msg(&b2, &a.did, "reply from b1").is_some_and(|m| m.outgoing && m.author_did == b1.did));
    eventually(40, "A has the reply", || find_msg(&a, &b1.did, "reply from b1").is_some());
    // Both of B's devices talk to A at once: nothing is lost or doubled anywhere.
    b1.node.send_text(a.did.clone(), "x1".into(), None).unwrap();
    b2.node.send_text(a.did.clone(), "x2".into(), None).unwrap();
    a.node.send_text(b1.did.clone(), "x3".into(), None).unwrap();
    for p in [&a, &b1, &b2] {
        let other = if p.did == a.did { b1.did.clone() } else { a.did.clone() };
        for t in ["x1", "x2", "x3", "hello B", "reply from b1"] {
            eventually(60, "everyone has everything", || count_msgs(p, &other, t) >= 1);
        }
    }
    std::thread::sleep(Duration::from_secs(2));
    for p in [&a, &b1, &b2] {
        let other = if p.did == a.did { b1.did.clone() } else { a.did.clone() };
        for t in ["x1", "x2", "x3", "hello B", "reply from b1"] {
            assert_eq!(count_msgs(p, &other, t), 1, "{t} on {}", p.did);
        }
        assert_eq!(all_msgs(p, &other).len(), 5, "same history everywhere");
    }
    // The sending device shows the real ticks; the other one shows what arrived as delivered.
    assert_eq!(find_msg(&b2, &a.did, "x2").unwrap().delivery, DeliveryState::Delivered);
}

#[test]
fn a_device_that_was_offline_catches_up_from_the_other_while_the_contact_is_away() {
    let (a, b1, b2) = trio("c2");
    b2.node.stop();
    a.node.send_text(b1.did.clone(), "while b2 slept".into(), None).unwrap();
    eventually(40, "b1 has it", || find_msg(&b1, &a.did, "while b2 slept").is_some());
    b1.node.send_text(a.did.clone(), "b1 reply".into(), None).unwrap();
    a.node.stop();
    online(&b2.node);
    b2.node.sync_now_for_test();
    eventually(60, "b2 caught up from b1 alone", || {
        find_msg(&b2, &a.did, "while b2 slept").is_some_and(|m| !m.outgoing) && find_msg(&b2, &a.did, "b1 reply").is_some_and(|m| m.outgoing)
    });
}

#[test]
fn reading_on_one_device_clears_unread_on_the_other() {
    let (a, b1, b2) = trio("c3");
    a.node.send_text(b1.did.clone(), "one".into(), None).unwrap();
    a.node.send_text(b1.did.clone(), "two".into(), None).unwrap();
    eventually(40, "both unread 2", || unread(&b1, &a.did) == 2 && unread(&b2, &a.did) == 2);
    b1.node.mark_read(a.did.clone()).unwrap();
    assert_eq!(unread(&b1, &a.did), 0);
    eventually(40, "b2 cleared", || unread(&b2, &a.did) == 0);
    // A newer message counts again on both.
    std::thread::sleep(Duration::from_millis(50));
    a.node.send_text(b1.did.clone(), "three".into(), None).unwrap();
    eventually(40, "both unread 1", || unread(&b1, &a.did) == 1 && unread(&b2, &a.did) == 1);
    // And the other direction: read on b2 clears b1.
    b2.node.mark_read(a.did.clone()).unwrap();
    eventually(40, "b1 cleared", || unread(&b1, &a.did) == 0);
}

#[test]
fn a_file_from_the_contact_reaches_the_other_device_through_ours_while_the_contact_is_offline() {
    let (a, b1, b2) = trio("c4");
    let dir = std::env::temp_dir().join(format!("p2pcore-c4-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("pic.bin");
    let bytes: Vec<u8> = (0..300_000u32).map(|i| (i.wrapping_mul(31) % 251) as u8).collect();
    std::fs::write(&src, &bytes).unwrap();
    b2.node.stop();
    let m = a.node.send_file(b1.did.clone(), src.to_string_lossy().into(), "application/octet-stream".into(), None).unwrap();
    eventually(60, "b1 holds the file", || {
        all_msgs(&b1, &a.did).iter().any(|x| x.id == m.id && x.attachment.as_ref().is_some_and(|t| t.state == TransferState::Ready))
    });
    a.node.stop();
    online(&b2.node);
    b2.node.sync_now_for_test();
    eventually(90, "b2 holds the file", || {
        all_msgs(&b2, &a.did).iter().any(|x| x.id == m.id && x.attachment.as_ref().is_some_and(|t| t.state == TransferState::Ready))
    });
    let out = dir.join("out.bin");
    b2.node.save_attachment(a.did.clone(), m.id.clone(), out.to_string_lossy().into()).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), bytes);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn you_items_sync_between_own_devices_and_never_reach_contacts() {
    let (a, b1, b2) = trio("c5");
    let sent = b1.node.send_text(b1.did.clone(), "remember the milk".into(), None).unwrap();
    assert!(sent.outgoing);
    assert_eq!(sent.delivery, DeliveryState::Delivered);
    eventually(40, "b2 has the You item", || find_msg(&b2, &b2.did, "remember the milk").is_some_and(|m| m.outgoing));
    for p in [&b1, &b2] {
        let you = p.node.chats().unwrap().into_iter().find(|c| c.peer_did == p.did).expect("a You entry");
        assert_eq!(you.peer_name, "You");
        assert_eq!(you.preview, "remember the milk");
        assert_eq!(you.unread, 0);
    }
    // An edit travels too.
    b2.node.edit_message(b2.did.clone(), sent.id.clone(), "remember the oat milk".into()).unwrap();
    eventually(40, "b1 sees the edit", || find_msg(&b1, &b1.did, "remember the oat milk").is_some());
    // Give a session with A every chance to leak it, then look.
    b1.node.send_text(a.did.clone(), "to A".into(), None).unwrap();
    eventually(40, "A has the normal message", || find_msg(&a, &b1.did, "to A").is_some());
    std::thread::sleep(Duration::from_secs(2));
    assert!(!a.node.has_you_docs_for_test() || all_msgs(&a, &a.did).is_empty());
    assert!(all_msgs(&a, &b1.did).iter().all(|m| !m.text.contains("milk")));
    assert!(a.node.chats().unwrap().iter().all(|c| !c.preview.contains("milk")));
    assert!(!a.node.has_chat_docs_for_test(&b1.did, &b1.did));
    // A new device linked later gets the saved items as well.
    let b3 = link_fresh(&b1, "c5-b3", false);
    eventually(60, "b3 has the You item", || find_msg(&b3, &b3.did, "remember the oat milk").is_some());
}

#[test]
fn removing_a_contact_on_one_device_removes_the_conversation_on_the_other() {
    let (a, b1, b2) = trio("c6");
    a.node.send_text(b1.did.clone(), "keep?".into(), None).unwrap();
    eventually(40, "b2 has it", || find_msg(&b2, &a.did, "keep?").is_some());
    b1.node.remove_contact(a.did.clone()).unwrap();
    eventually(40, "b2 dropped A", || !b2.node.contacts().iter().any(|c| c.did == a.did));
    eventually(20, "b2 dropped the conversation", || {
        b2.node.chat_day(a.did.clone(), None).is_err() && b2.node.chats().unwrap().iter().all(|c| c.peer_did != a.did)
    });
    assert!(!b2.node.has_chat_docs_for_test(&b2.did, &a.did));
    // The sync does not bring it back.
    b1.node.sync_now_for_test();
    std::thread::sleep(Duration::from_secs(3));
    assert!(!b2.node.has_chat_docs_for_test(&b2.did, &a.did) && !b1.node.has_chat_docs_for_test(&b1.did, &a.did));
}

#[test]
fn a_forged_batch_relayed_by_an_own_device_is_rejected() {
    let (a, _b1, b2) = trio("c7");
    // Genuine control first, so the rejections below are about the forgery and nothing else.
    b2.node.forged_own_batch_for_test(a.did.clone(), 3).unwrap();
    assert!(find_msg(&b2, &a.did, "forged 3").is_some_and(|m| m.outgoing));
    for kind in 0..3u8 {
        assert!(b2.node.forged_own_batch_for_test(a.did.clone(), kind).is_err(), "kind {kind} must be rejected");
        assert!(find_msg(&b2, &a.did, &format!("forged {kind}")).is_none());
    }
}

#[test]
fn a_message_held_only_by_our_other_device_is_not_delivered() {
    let (a, b1, b2) = trio("c8");
    a.node.stop();
    b1.node.send_text(a.did.clone(), "contact is away".into(), None).unwrap();
    eventually(40, "b2 has it", || find_msg(&b2, &a.did, "contact is away").is_some());
    std::thread::sleep(Duration::from_secs(2));
    let on_b1 = find_msg(&b1, &a.did, "contact is away").unwrap().delivery;
    let on_b2 = find_msg(&b2, &a.did, "contact is away").unwrap().delivery;
    assert_eq!(on_b1, DeliveryState::Pending, "sender");
    assert_eq!(on_b2, DeliveryState::Pending, "other own device");
    // The contact comes back: both of our devices show two ticks.
    online(&a.node);
    b1.node.sync_now_for_test();
    b2.node.sync_now_for_test();
    eventually(60, "A has it", || find_msg(&a, &b1.did, "contact is away").is_some());
    for p in [&b1, &b2] {
        eventually(60, "two ticks on both devices", || find_msg(p, &a.did, "contact is away").is_some_and(|m| m.delivery == DeliveryState::Delivered));
        let c = p.node.chats().unwrap().into_iter().find(|c| c.peer_did == a.did).unwrap();
        assert_eq!(c.last_delivery, DeliveryState::Delivered);
    }
}
