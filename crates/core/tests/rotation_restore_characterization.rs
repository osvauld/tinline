//! K1: characterization of what a restore-over-existing (forgotten passphrase) does today, given
//! that it replaces this install's device key (`node.rs` `set_identity`: a fresh
//! `new_device_secret()`, the old registry entry marked `removed`) while keeping `account.redb`
//! and `chat.redb`. See `docs/design/rotation-baseline-report.md` §4 (O1–O6) and
//! `docs/design/rotation-author-key-trace.md` §2–§7.
//!
//! Every test carries one label:
//! - `KIND: limitation`: asserts today's (wrong) outcome; the comment names the expected-correct
//!   one. When production code is fixed, the assertion flips and the test must be rewritten.
//! - `KIND: invariant`: holds today and should keep holding.
//!
//! Real iroh endpoints (needs network, like `restore.rs`, `fanout.rs`, `link.rs`). Waits are
//! bounded polls. "Never arrives" claims are bounded too (stated per test) and, where the code
//! leaves a trace, backed by the log line that explains why.
//! Helpers are copied from `link.rs` / `restore.rs` (they are not in a shared module). Bugs 1 and
//! 3 of decisions section 5 are fixed on `dev` (P1); their tests are invariants now.
//! P8: messages are signed by the author's DID key as well, so a message written before a
//! restore verifies on any device; only records written without it (pre-P8, simulated with
//! `legacy_message_signing_for_test`) still depend on the device key that signed them.

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
    let dir = std::env::temp_dir().join(format!("p2pcore-rotk1-{name}-{}", std::process::id()));
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

fn incoming(p: &Peer, secs: u64) -> String {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p.rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(Ev::Incoming(c)) => return c.call_id,
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for an incoming call"),
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

/// Polls `f` for up to `secs`: whether it ever held. Used for bounded "does not happen" claims.
fn within(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    f()
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

fn verified(p: &Peer, did: &str) -> bool {
    p.node.contacts().into_iter().find(|c| c.did == did).expect("contact").verified
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// `caller` calls `callee`, the callee answers, the caller hangs up. The answer carries the
/// callee's signed `DeviceList` to the caller (as in `link.rs`), so afterwards the caller holds
/// it and dials the callee by that list only (`note_device` no longer adds unknown devices).
fn call_answered(caller: &Peer, callee: &Peer) {
    let call = caller.node.call(callee.did.clone()).unwrap();
    assert_eq!(incoming(callee, 30), call.call_id);
    callee.node.answer(call.call_id.clone()).unwrap();
    eventually(20, "call active", || caller.node.current_call().is_some());
    std::thread::sleep(Duration::from_millis(500));
    caller.node.hangup(call.call_id).unwrap();
    eventually(20, "call over", || caller.node.current_call().is_none() && callee.node.current_call().is_none());
}

/// The chat fixture: B added A by A's ticket. `a_holds_list`: A also holds B's signed device
/// list (A called B once). B sends `m1`, which A holds and B shows Delivered. With `pending`,
/// A goes offline and B sends `m2`, which stays Pending. Then B restores in place (new device
/// key) and is left stopped; A is left stopped iff `pending`.
struct Chat {
    a: Peer,
    b: Peer,
    m1: String,
    m2: Option<String>,
    old_dev: String,
    new_dev: String,
}

fn chat_fixture(tag: &str, a_holds_list: bool, pending: bool, a_verifies_b: bool) -> Chat {
    let a = peer(&format!("{tag}-a"), true);
    let mut b = peer(&format!("{tag}-b"), true);
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    if a_holds_list {
        call_answered(&a, &b);
    }
    if a_verifies_b {
        a.node.set_verified(b.did.clone(), true).unwrap();
    }
    let m1 = b.node.send_text(a.did.clone(), "m1: delivered before restore".into(), None).unwrap().id;
    eventually(60, "A holds m1", || has(&a, &b.did, &m1));
    eventually(60, "B shows m1 delivered", || ticks(&b, &a.did, &m1) == DeliveryState::Delivered);
    let m2 = pending.then(|| {
        a.node.stop();
        let m2 = b.node.send_text(a.did.clone(), "m2: pending at restore".into(), None).unwrap().id;
        std::thread::sleep(Duration::from_secs(1));
        assert_eq!(ticks(&b, &a.did, &m2), DeliveryState::Pending, "A is offline");
        m2
    });
    let (old_dev, new_dev) = restore_in_place(&mut b);
    Chat { a, b, m1, m2, old_dev, new_dev }
}

// ---- 1. O1: the cached ticket -----------------------------------------------------------------

// KIND: limitation
// O1 confirmed. `ticket()` (node.rs, the reuse check) compares issuer, name, expiry and relay but
// not `claim.device`, and restore keeps `State.ticket`. So after a restore the app keeps showing
// the pre-restore ticket, which names the dead key; a joiner who scans it dials a key nobody
// holds and fails. Asserted on the not-yet-started node, where no relay is known and the
// relay check cannot force a re-issue. (Observed once started too, with an unchanged relay; not
// asserted because the relay is network-dependent.)
// Expected-correct: a ticket whose `device` is not our current device key is never handed out;
// `my_ticket()` after a restore mints a fresh one naming the new key, and that one redeems.
#[test]
fn o1_restore_keeps_handing_out_the_cached_ticket_for_the_dead_key() {
    let mut b = peer("o1-b", true);
    let c = peer("o1-c", true);
    let before = b.node.my_ticket().unwrap();
    let (old_dev, new_dev) = restore_in_place(&mut b);

    let after = b.node.my_ticket().unwrap();
    assert_eq!(after, before, "the pre-restore ticket is handed out again");
    let claim = proto::ContactTicket::from_text(&after).unwrap().verify(now()).unwrap();
    assert_eq!(claim.iss, b.did);
    assert_eq!(claim.device, old_dev, "it names the old device key");
    assert_ne!(claim.device, new_dev);

    online(&b.node);
    // Redeeming it dials the old key: no endpoint answers (observed `Net("timed out")` ~20 s).
    let r = c.node.add_contact(after);
    assert!(r.is_err(), "the cached ticket cannot be redeemed: {r:?}");
    assert!(c.node.contacts().is_empty() && b.node.contacts().is_empty());
}

// ---- 2. O3: a message sent but not yet delivered before the restore ---------------------------

// KIND: invariant
// O3 refuted for this configuration. A holds no signed device list for B, so the restored device
// is added to A's devices on its first chat session (`note_device`). B's first new message builds
// on the stranded `m2` ops, A sees `Pending`, pulls the day as a vouched history snapshot from B
// (engine.rs `need_history` -> `chat_pull_day`), and verifies `m2` against B's old key, which it
// still holds. So `m2` is delivered after all, alongside `m3`.
// Note: the route depends on B writing something new; the live path never re-sends `m2` itself
// (`export_own_since` exports only the new peer).
#[test]
fn o3_pending_message_reaches_a_contact_that_holds_no_device_list() {
    let f = chat_fixture("o3inv", false, true, false);
    let (a, b, m2) = (&f.a, &f.b, f.m2.clone().unwrap());
    online(&a.node);
    online(&b.node);
    let m3 = b.node.send_text(a.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A holds m3", || has(a, &b.did, &m3));
    eventually(30, "A holds m2", || has(a, &b.did, &m2));
    let all = all_msgs(a, &b.did);
    for id in [&f.m1, &m2, &m3] {
        assert_eq!(all.iter().filter(|m| &m.id == id).count(), 1, "exactly once: {id}");
    }
}

// KIND: invariant
// Bug 1 of decisions section 5 (was `o3_pending_message_stalls_all_chat_...`, a limitation). A
// holds B's signed device list (one earlier call), which names only B's OLD key; the restored key
// is accepted for the session but not added to A's devices (P4 territory, still asserted below).
// B's new `m3` builds on the stranded `m2`, A sees `Pending` and pulls the day as a vouched
// snapshot from B over that live session. `m2` is signed by B's old key (in A's list) and `m3`
// by B's new key: the key of the very session the snapshot arrives on, attested by B's DID and
// already trusted for B's live batches. Before the fix the snapshot was judged with A's stored
// list only and rejected as a whole ("rejected history: signature does not verify"), and every
// later message stalled the same way.
// Expected (now asserted): `m2` and `m3` reach A once each, B shows `m3` delivered, and a later
// `m4` reaches A too.
#[test]
fn o3_pending_message_and_new_messages_reach_a_contact_holding_our_device_list() {
    let f = chat_fixture("o3fix", true, true, false);
    let (a, b, m2) = (&f.a, &f.b, f.m2.clone().unwrap());
    online(&a.node);
    online(&b.node);
    let m3 = b.node.send_text(a.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A holds m3", || has(a, &b.did, &m3));
    eventually(30, "A holds m2", || has(a, &b.did, &m2));
    eventually(30, "B shows m3 delivered", || ticks(b, &a.did, &m3) == DeliveryState::Delivered);
    let m4 = b.node.send_text(a.did.clone(), "m4: later".into(), None).unwrap().id;
    eventually(60, "A holds m4", || has(a, &b.did, &m4));
    assert!(!a.logged("rejected history"), "no snapshot of B's was rejected");
    let all = all_msgs(a, &b.did);
    for id in [&f.m1, &m2, &m3, &m4] {
        assert_eq!(all.iter().filter(|m| &m.id == id).count(), 1, "exactly once: {id}");
    }
    assert_eq!(a.node.contact_devices_for_test(b.did.clone()), vec![f.old_dev.clone()], "A's list for B is unchanged (P4)");
}

// KIND: invariant
// Control for the test above: with nothing pending at the restore, the same contact (holding B's
// old device list) accepts B's new messages over the live session, because they only build on
// ops A already holds and the live session verifies them against the session's key.
#[test]
fn o3_without_a_pending_message_new_messages_reach_a_contact_holding_our_device_list() {
    let f = chat_fixture("o3ctl", true, false, false);
    let (a, b) = (&f.a, &f.b);
    online(&b.node);
    let m3 = b.node.send_text(a.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A holds m3", || has(a, &b.did, &m3));
    eventually(30, "B shows m3 delivered", || ticks(b, &a.did, &m3) == DeliveryState::Delivered);
    assert_eq!(all_msgs(a, &b.did).len(), 2, "m1 and m3, once each");
}

// ---- 3. O4: delivery ticks of pre-restore messages --------------------------------------------

// KIND: limitation
// O4 confirmed. `mc` stores only a counter; `chat_delivery` pairs it with the CURRENT device's
// peer. Right after the restore (no network involved) the already-delivered `m1` drops back to
// one tick. Later, once A holds `m2` (via the history pull of the invariant test above) and has
// acked `m3`, B still shows `m2` Pending: its old-peer counter is compared with the new peer's
// ack. (Observed: one more message from B pushes the new peer's counter past it and `m2` then
// shows Delivered: correct only by counter coincidence, and by the same mechanism a message A
// does not hold could show two ticks.)
// Expected-correct: a pre-restore message keeps the ticks of the peer that wrote it: `m1` stays
// Delivered, `m2` becomes Delivered exactly when A acks the old peer's ops.
#[test]
fn o4_ticks_of_pre_restore_messages_follow_the_new_device_counter() {
    let f = chat_fixture("o4", false, true, false);
    let (a, b, m1, m2) = (&f.a, &f.b, &f.m1, f.m2.clone().unwrap());
    assert_eq!(ticks(b, &a.did, m1), DeliveryState::Pending, "m1 was Delivered before the restore");
    assert_eq!(ticks(b, &a.did, &m2), DeliveryState::Pending);

    online(&a.node);
    online(&b.node);
    let m3 = b.node.send_text(a.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A holds m2 and m3", || has(a, &b.did, &m3) && has(a, &b.did, &m2));
    eventually(30, "B shows m3 delivered", || ticks(b, &a.did, &m3) == DeliveryState::Delivered);
    std::thread::sleep(Duration::from_secs(3));
    assert!(has(a, &b.did, &m2));
    assert_eq!(ticks(b, &a.did, &m2), DeliveryState::Pending, "A holds m2, B still shows one tick");
}

// ---- 4. O2: the contact's other device and our pre-restore history ----------------------------

/// A has two installs (A1, A2 linked). B's `m1` reaches only A1 (A2 offline), and A1 answers
/// with `r1`, which B holds (so `r1`'s ops build on `m1`'s, in A1's doc and in B's). B restores;
/// a call from the restored B rings A2, whose answer path takes B's new signed device list: A2
/// knows only B's NEW key and holds neither `m1` nor `r1`. Left: B and A2 started, A1 stopped.
/// With `legacy_m1`, B writes `m1` as an un-upgraded (pre-P8) app does: device signature only.
struct Sib {
    a1: Peer,
    a2: Peer,
    b: Peer,
    m1: String,
    r1: String,
}

fn sibling_fixture(tag: &str, legacy_m1: bool) -> Sib {
    let a1 = peer(&format!("{tag}-a1"), true);
    let mut b = peer(&format!("{tag}-b"), true);
    b.node.add_contact(a1.node.my_ticket().unwrap()).unwrap();
    let a2 = link_fresh(&a1, &format!("{tag}-a2"));
    eventually(40, "A2 has B", || a2.node.contacts().iter().any(|c| c.did == b.did));
    // B learns A2 from A's device list on A1's answer.
    call_answered(&b, &a1);
    let a2_dev = a2.node.device_key_for_test().unwrap();
    eventually(20, "B knows A2", || b.node.contact_devices_for_test(a1.did.clone()).contains(&a2_dev));

    a2.node.stop();
    b.node.legacy_message_signing_for_test(legacy_m1);
    let m1 = b.node.send_text(a1.did.clone(), "m1: only A1 holds it".into(), None).unwrap().id;
    eventually(60, "A1 holds m1", || has(&a1, &b.did, &m1));
    let r1 = a1.node.send_text(b.did.clone(), "r1: A1 answers m1".into(), None).unwrap().id;
    eventually(60, "B holds r1", || has(&b, &a1.did, &r1));
    a1.node.stop();

    let (_old_dev, new_dev) = restore_in_place(&mut b);
    online(&b.node);
    online(&a2.node);
    call_answered(&b, &a2);
    eventually(20, "A2 knows only B's new key", || a2.node.contact_devices_for_test(b.did.clone()) == vec![new_dev.clone()]);
    assert!(!has(&a2, &b.did, &m1) && !has(&a2, &b.did, &r1), "precondition: A2 holds neither m1 nor r1");
    Sib { a1, a2, b, m1, r1 }
}

// KIND: invariant
// Bug 3 of decisions section 5, the snapshot route. The restored B writes `m3` to A; A2 gets it
// live, but it builds on `m1`/`r1`, so A2 pulls B's vouched snapshot of the day. It holds `m1`
// (B's OLD key, which A2 never knew), `r1` (A1's key: one of A's own devices) and `m3` (B's new
// key). Before the fix the one unverifiable `m1` made A2 reject the whole snapshot, so nothing
// from B reached A2 that day. Expected (now asserted): history is judged per message: `r1` and
// `m3` reach A2 (what happens to `m1` is the separate limitation test below).
#[test]
fn o2_contacts_other_device_gets_our_new_messages_although_it_never_knew_our_old_key() {
    let f = sibling_fixture("o2snap", false);
    let (a2, b) = (&f.a2, &f.b);
    let m3 = b.node.send_text(f.a1.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A2 holds m3", || has(a2, &b.did, &m3));
    eventually(30, "A2 holds r1", || has(a2, &b.did, &f.r1));
    eventually(30, "B shows m3 delivered", || ticks(b, &f.a1.did, &m3) == DeliveryState::Delivered);
    assert!(!a2.logged("rejected history"), "the snapshot was not rejected as a whole");
}

// KIND: invariant
// Bug 3 of decisions section 5, the relay route. With B stopped, A1 comes back and relays the day
// to A2 (own-device sync): one batch with `m1` (B's old key, unknown to A2) and A1's own `r1`.
// Before the fix own-device sync rejected that batch as a whole, so A2 never got `r1` either.
// Expected (now asserted): `r1` reaches A2 from A1; the account doc syncs as before; A1 keeps
// showing `m1` (stored messages are never re-verified) and learns B's new list from A2.
#[test]
fn o2_contacts_other_device_accepts_the_verifiable_messages_its_sibling_relays() {
    let f = sibling_fixture("o2rel", false);
    let (a1, a2, b) = (&f.a1, &f.a2, &f.b);
    b.node.stop();
    online(&a1.node);
    a1.node.rename_contact(b.did.clone(), Some("Bee".into())).unwrap();
    a2.node.sync_now_for_test();
    eventually(60, "the account doc syncs A1 -> A2", || {
        a2.node.contacts().iter().any(|c| c.did == b.did && c.alias.as_deref() == Some("Bee"))
    });
    eventually(60, "A2 holds r1, relayed by A1 with m1", || has(a2, &b.did, &f.r1));
    assert!(has(a1, &b.did, &f.m1), "A1 keeps showing m1");
    let new_dev = a2.node.contact_devices_for_test(b.did.clone());
    eventually(20, "A1 learns B's new list from A2", || a1.node.contact_devices_for_test(b.did.clone()) == new_dev);
}

// KIND: invariant
// P8 (was the P1 limitation below for every message). `m1` was written by B before its restore
// and signed by B's OLD device key, which A2 never knew, and by B's DID key, which does not
// change on restore. A2 verifies `m1` against B's DID, so it shows `m1` by both routes: B's
// vouched snapshot (pulled because `m3` builds on `m1`), and A1 relaying the day.
#[test]
fn o2_a_pre_restore_message_reaches_a_device_that_never_knew_the_old_key() {
    let f = sibling_fixture("o2did", false);
    let (a1, a2, b) = (&f.a1, &f.a2, &f.b);
    // Route 1: B's vouched snapshot.
    let m3 = b.node.send_text(a1.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A2 holds m3", || has(a2, &b.did, &m3));
    eventually(30, "A2 shows m1 from B's snapshot", || has(a2, &b.did, &f.m1));
    assert!(!a2.logged("signature does not verify"), "nothing was hidden or rejected");
    // Route 2: A1 relays the day (a second copy of the same ops changes nothing).
    b.node.stop();
    online(&a1.node);
    a1.node.rename_contact(b.did.clone(), Some("Bee".into())).unwrap();
    a2.node.sync_now_for_test();
    eventually(60, "the account doc syncs A1 -> A2", || {
        a2.node.contacts().iter().any(|c| c.did == b.did && c.alias.as_deref() == Some("Bee"))
    });
    assert!(has(a2, &b.did, &f.m1) && has(a2, &b.did, &f.r1) && has(a2, &b.did, &m3));
    assert!(has(a1, &b.did, &f.m1), "A1 keeps showing m1");
    assert!(!a2.logged("signature does not verify"));
}

// KIND: limitation
// POLICY-GATED (production-phase1-plan "Open details", P1; decisions section 12 Q1). Only for
// records without a DID signature (written by a pre-P8 app; simulated here). `m1` was signed by
// B's OLD device key only, and A2 never knew that key: neither the snapshot from B nor the relay
// from A1 lets A2 verify it, so A2 keeps its ops (later messages build on them) but never shows
// it; A1 still does. Bounded: `m1` does not appear on A2 within 20 s after both routes delivered
// their verifiable messages.
// Expected-correct: undecided. Options: accept that such `m1` is lost on A2; B re-signs its own
// unacknowledged messages with its DID key after a restore (a valid re-signed copy would replace
// the hidden one); or receivers keep superseded keys of the same DID for verifying content only.
#[test]
fn o2_a_pre_p8_message_signed_only_by_the_old_key_stays_hidden_from_a_device_that_never_knew_that_key() {
    let f = sibling_fixture("o2lim", true);
    let (a1, a2, b) = (&f.a1, &f.a2, &f.b);
    // Route 1: B's vouched snapshot (pulled because `m3` builds on `m1`).
    let m3 = b.node.send_text(a1.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A2 holds m3", || has(a2, &b.did, &m3));
    eventually(20, "A2 logged the hidden message", || a2.logged(&f.m1));
    // Route 2: A1 relays the day.
    b.node.stop();
    online(&a1.node);
    a1.node.rename_contact(b.did.clone(), Some("Bee".into())).unwrap();
    a2.node.sync_now_for_test();
    eventually(60, "the account doc syncs A1 -> A2", || {
        a2.node.contacts().iter().any(|c| c.did == b.did && c.alias.as_deref() == Some("Bee"))
    });
    assert!(!within(20, || has(a2, &b.did, &f.m1)), "A2 never shows m1");
    assert!(has(a1, &b.did, &f.m1), "A1 keeps showing m1");
    assert!(has(a2, &b.did, &f.r1) && has(a2, &b.did, &m3));
}

// ---- 5. O6: the contact's `verified` flag -----------------------------------------------------

// KIND: limitation
// O6 confirmed. A marked B verified. B restores (same DID, same safety number) and its first chat
// session presents the new key; `note_device` clears `verified` for any device it has not seen.
// A now also dials both keys: the dead one stays in A's list (no signed list retires it).
// (Observed as well when A holds B's device list: the flag is cleared although the new key is
// not even added.)
// Expected-correct: POLICY-GATED (verified-DID policy; handoff "enforce verified-DID policy").
// One candidate: a DID-attested key change on the same DID keeps `verified`, since the safety
// number is derived from DIDs only. This test only records that today the flag is lost.
#[test]
fn o6_contacts_verified_flag_is_cleared_by_our_restore() {
    let f = chat_fixture("o6", false, false, true);
    let (a, b) = (&f.a, &f.b);
    assert!(verified(a, &b.did), "verified before the restore");
    online(&b.node);
    let m3 = b.node.send_text(a.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A holds m3", || has(a, &b.did, &m3));
    assert!(!verified(a, &b.did), "the restore cleared verified");
    let devs = a.node.contact_devices_for_test(b.did.clone());
    assert!(devs.contains(&f.new_dev) && devs.contains(&f.old_dev), "A dials both keys: {devs:?}");
}

// KIND: invariant
// What `verified` attests (the safety number) does not depend on the device key: a restore
// changes neither side's safety number. No network needed.
#[test]
fn o6_safety_numbers_do_not_change_on_restore() {
    let a = peer("o6sn-a", false);
    let mut b = peer("o6sn-b", false);
    let on_a = a.node.safety_number(b.did.clone()).unwrap();
    let on_b = b.node.safety_number(a.did.clone()).unwrap();
    assert_eq!(on_a, on_b);
    restore_in_place(&mut b);
    assert_eq!(b.node.safety_number(a.did.clone()).unwrap(), on_b);
    assert_eq!(a.node.safety_number(b.did.clone()).unwrap(), on_a);
}
