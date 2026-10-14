//! P6 acceptance: separate `message` and `call` tokens (decisions U13, U15; section 14 of
//! `docs/design/rotation-open-decisions.md`), on real nodes over real iroh endpoints (needs
//! network, like `calls.rs`).
//!
//! - Ticket contacts get message + call; group chat-request contacts message only (U13). There
//!   is no group code yet, so the group origin is driven through `add_contact_as_for_test`.
//! - A message-only contact chats; its call ends with "calls not allowed" and never rings
//!   (PROVISIONAL (coordinator): the reason is shown to every authenticated contact).
//! - "Allow calls" sends a call token; "disallow calls" takes every call token back (per-DID
//!   cut-off) and leaves chat alone.
//! - A block takes back both; an unblock does not revive them (U20: the other side re-adds us
//!   and the re-add carries fresh tokens).
//!
//! Every test carries `// KIND: acceptance` and the decision it enforces.

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CALLS_NOT_ALLOWED, CallInfo, CallState, ContactOrigin, DeliveryState, LinkEvents, Message, Node, NodeEvents, NodeStatus};

const PASS: &str = "test-passphrase";
/// A refusal must be quick: well under any dial timeout.
const QUICK: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
enum Ev {
    Incoming(CallInfo),
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

#[derive(Debug, Clone)]
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
    fn on_devices_changed(&self) {}
    fn on_history_changed(&self) {}
    fn on_unlinked(&self, _: String) {}
}

struct Peer {
    node: Arc<Node>,
    rx: mpsc::Receiver<Ev>,
    lrx: mpsc::Receiver<LEv>,
    logs: Arc<Mutex<Vec<String>>>,
    did: String,
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

fn online(n: &Node) {
    n.start().unwrap();
    let t = Instant::now();
    while !n.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn blank(name: &str) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-p6-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = mpsc::channel();
    let (ltx, lrx) = mpsc::channel();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Tap(Mutex::new(tx), logs.clone()))).unwrap();
    node.auto_accept_contacts_for_test(true);
    node.set_link_events(Arc::new(LTap(Mutex::new(ltx))));
    Peer { node, rx, lrx, logs, did: String::new(), dir }
}

fn peer(name: &str) -> Peer {
    let mut p = blank(name);
    p.node.create_identity(name.into(), PASS.into()).unwrap();
    online(&p.node);
    p.did = p.node.profile().unwrap().did;
    p
}

/// a and b, each a contact of the other (b scanned a's ticket).
fn pair(tag: &str) -> (Peer, Peer) {
    let a = peer(&format!("{tag}-a"));
    let b = peer(&format!("{tag}-b"));
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    (a, b)
}

/// `e` (existing) links a fresh install; returns it committed and online.
fn link_fresh(e: &Peer, name: &str) -> Peer {
    let mut n = blank(name);
    let qr = e.node.link_show_qr().unwrap();
    n.node.link_new_scan(qr, "Pixel".into()).unwrap();
    let code = |p: &Peer| loop {
        if let LEv::Code(c) = p.lrx.recv_timeout(Duration::from_secs(30)).expect("link code") {
            break c;
        }
    };
    assert_eq!(code(e), code(&n));
    e.node.link_approve(Some(PASS.into())).unwrap();
    let did = loop {
        if let LEv::Done(d) = n.lrx.recv_timeout(Duration::from_secs(30)).expect("link done") {
            break d;
        }
    };
    n.node.set_passphrase(None, "n-pass".into()).unwrap();
    online(&n.node);
    n.did = did;
    n
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

fn ended_after(p: &Peer, id: &str, t0: Instant, secs: u64) -> (String, Duration) {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p.rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(Ev::State(i, CallState::Ended { reason })) if i == id => return (reason, t0.elapsed()),
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for call {id} to end"),
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

fn saw_incoming(p: &Peer) -> bool {
    p.rx.try_iter().any(|e| matches!(e, Ev::Incoming(_)))
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

fn holds(p: &Peer, did: &str, id: &str) -> bool {
    all_msgs(p, did).iter().any(|m| m.id == id)
}

fn delivered(p: &Peer, did: &str, id: &str) -> bool {
    all_msgs(p, did).iter().any(|m| m.id == id && m.delivery == DeliveryState::Delivered)
}

/// `from` calls `to`; `to` rings; `from` hangs up and both settle.
fn rings(from: &Peer, to: &Peer) {
    let call = from.node.call(to.did.clone()).unwrap();
    assert_eq!(incoming(to, 30), call.call_id, "the call rings");
    from.node.hangup(call.call_id.clone()).unwrap();
    ended_after(from, &call.call_id, Instant::now(), 30);
    std::thread::sleep(Duration::from_millis(500));
    let _ = (from.rx.try_iter().count(), to.rx.try_iter().count());
}

/// `from` calls `to` and is refused with "calls not allowed", quickly, and `to` never rings.
fn refused_calls_not_allowed(from: &Peer, to: &Peer) {
    let t0 = Instant::now();
    let call = from.node.call(to.did.clone()).unwrap();
    let (reason, took) = ended_after(from, &call.call_id, t0, 40);
    assert_eq!(reason, CALLS_NOT_ALLOWED);
    assert!(took < QUICK, "took {took:?}");
    std::thread::sleep(Duration::from_millis(500));
    assert!(!saw_incoming(to), "a message-only contact must not ring us");
}

fn message_arrives(from: &Peer, to: &Peer, text: &str) {
    let m = from.node.send_text(to.did.clone(), text.into(), None).unwrap().id;
    eventually(60, "message delivered", || holds(to, &from.did, &m) && delivered(from, &to.did, &m));
}

// KIND: acceptance
// Decision: U13 default — a ticket contact holds message + call both ways: chat and calls work
// in both directions, and each side reports calls allowed / can call.
#[test]
fn ticket_contacts_can_chat_and_call_both_ways() {
    let (a, b) = pair("tkt");
    for (x, y) in [(&a, &b), (&b, &a)] {
        assert!(x.node.calls_allowed(y.did.clone()), "ticket contacts may call");
        assert!(x.node.can_call(y.did.clone()), "ticket contacts hold a call token");
        let (m, c) = x.node.held_token_ids_for_test(y.did.clone());
        assert!(m.is_some() && c.is_some() && m != c, "two separate tokens");
    }
    message_arrives(&b, &a, "hi a");
    message_arrives(&a, &b, "hi b");
    rings(&b, &a);
    rings(&a, &b);
}

// KIND: acceptance
// Decision: U13 default (group chat-request contacts are message-only) + section 14 Q6
// PROVISIONAL (show "calls not allowed") — the message-only contact chats, its call ends at once
// with "calls not allowed" and never rings; it is not told "blocked".
#[test]
fn a_message_only_contact_chats_but_its_call_is_refused() {
    let a = peer("mo-a");
    let b = peer("mo-b");
    // b came to a through a group chat request: b gives a message only.
    b.node.add_contact_as_for_test(a.node.my_ticket().unwrap(), ContactOrigin::GroupRequest).unwrap();
    assert!(!b.node.calls_allowed(a.did.clone()));
    assert!(!a.node.can_call(b.did.clone()), "a holds no call token from b");
    message_arrives(&a, &b, "message-only works");
    message_arrives(&b, &a, "and back");
    refused_calls_not_allowed(&a, &b);
    assert!(b.logged("incoming call refused: calls not allowed"));
    // b may still call a (a gave b the ticket default).
    rings(&b, &a);
}

// KIND: acceptance
// Decision: U13 ("allow calls" sends the call token) — the upgrade lets calls through.
#[test]
fn allow_calls_upgrades_a_message_only_contact() {
    let a = peer("up-a");
    let b = peer("up-b");
    b.node.add_contact_as_for_test(a.node.my_ticket().unwrap(), ContactOrigin::GroupRequest).unwrap();
    refused_calls_not_allowed(&a, &b);
    b.node.allow_calls(a.did.clone(), true).unwrap();
    assert!(b.node.calls_allowed(a.did.clone()));
    eventually(40, "a receives b's call token", || a.node.can_call(b.did.clone()));
    rings(&a, &b);
    message_arrives(&a, &b, "chat unchanged");
}

// KIND: acceptance
// Decision: U13 + U15 (separate tokens: a downgrade takes the call token only) — "disallow
// calls" stops their calls at once; chat goes on; our own calls to them are unaffected.
#[test]
fn disallow_calls_stops_calls_but_not_chat() {
    let (a, b) = pair("down");
    rings(&a, &b);
    b.node.allow_calls(a.did.clone(), false).unwrap();
    assert!(!b.node.calls_allowed(a.did.clone()));
    refused_calls_not_allowed(&a, &b);
    assert!(!a.node.can_call(b.did.clone()), "the refused caller drops its call token");
    message_arrives(&a, &b, "still chatting");
    message_arrives(&b, &a, "yes");
    rings(&b, &a);
    // And back up again.
    b.node.allow_calls(a.did.clone(), true).unwrap();
    eventually(40, "a gets a fresh call token", || a.node.can_call(b.did.clone()));
    rings(&a, &b);
}

// KIND: acceptance
// Decision: U13 (blocking revokes both tokens) + U20 (unblock does not revive them: the other
// side re-adds and gets fresh tokens) — b acts like a modified app that ignores the notice, so it
// keeps a's old tokens; after the unblock they open nothing; the re-add issues fresh ones.
#[test]
fn block_revokes_both_tokens_and_unblock_does_not_revive_them() {
    let (a, b) = pair("rev");
    b.node.ignore_block_notices_for_test(true);
    let old = b.node.held_token_ids_for_test(a.did.clone());
    a.node.block(b.did.clone()).unwrap();
    a.node.unblock(b.did.clone()).unwrap();
    assert_eq!(b.node.held_token_ids_for_test(a.did.clone()), old, "the modified app kept the old tokens");

    // Old call token: refused (generic: it was taken back), never rings.
    let t0 = Instant::now();
    let call = b.node.call(a.did.clone()).unwrap();
    let (reason, took) = ended_after(&b, &call.call_id, t0, 40);
    assert_ne!(reason, CALLS_NOT_ALLOWED, "a revoked token is not a permission question");
    assert!(took < Duration::from_secs(35), "{reason} after {took:?}");
    std::thread::sleep(Duration::from_millis(500));
    assert!(!saw_incoming(&a), "a revoked call token must not ring");
    // Old message token: chat refused.
    let m = b.node.send_text(a.did.clone(), "with an old token".into(), None).unwrap().id;
    std::thread::sleep(Duration::from_secs(8));
    assert!(!holds(&a, &b.did, &m), "a revoked message token must not open chat");
    assert!(a.logged("no valid message token"));

    // Re-add: fresh tokens, and everything works again.
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let fresh = b.node.held_token_ids_for_test(a.did.clone());
    assert!(fresh.0.is_some() && fresh.1.is_some());
    assert_ne!(fresh.0, old.0, "a fresh message token");
    assert_ne!(fresh.1, old.1, "a fresh call token");
    rings(&b, &a);
    message_arrives(&b, &a, "after the re-add");
}

// KIND: acceptance
// Decision: U20 + U13 — with the shipped app: block, the blocked app deletes our entry; unblock;
// they re-add and get fresh message + call tokens; a contact that was message-only before the
// block stays message-only after its re-add (and is not told "blocked").
#[test]
fn unblock_and_readd_issues_fresh_tokens_and_keeps_call_permission() {
    let (a, b) = pair("readd");
    let c = peer("readd-c");
    c.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    a.node.allow_calls(c.did.clone(), false).unwrap();
    let old_b = b.node.held_token_ids_for_test(a.did.clone());
    for p in [&b, &c] {
        a.node.block(p.did.clone()).unwrap();
        eventually(40, "their app deleted our entry", || !p.node.contacts().iter().any(|x| x.did == a.did));
        a.node.unblock(p.did.clone()).unwrap();
        eventually(40, "the unblock reached them", || p.node.block_state(a.did.clone()).blocked_me.is_none());
        p.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    }
    let new_b = b.node.held_token_ids_for_test(a.did.clone());
    assert!(new_b.1.is_some());
    assert_ne!(new_b.0, old_b.0);
    assert_ne!(new_b.1, old_b.1);
    assert!(a.node.calls_allowed(b.did.clone()));
    assert!(!a.node.calls_allowed(c.did.clone()), "message-only stays message-only");
    rings(&b, &a);
    refused_calls_not_allowed(&c, &a);
    message_arrives(&c, &a, "messages still work");
    message_arrives(&b, &a, "and from b");
}

// KIND: acceptance
// Decision: U13 + section 14 Q4 (best effort) — "disallow calls" made on one device (N) holds on
// its linked sibling (E) once the account doc has synced: A's call to E (the only device A
// knows) is refused with "calls not allowed" and rings neither device.
#[test]
fn a_downgrade_holds_on_the_linked_sibling() {
    let a = peer("sib-a");
    let e = peer("sib-e");
    e.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let n = link_fresh(&e, "sib-n");
    eventually(40, "N has A", || n.node.contacts().iter().any(|c| c.did == a.did));
    assert!(n.node.calls_allowed(a.did.clone()));
    n.node.allow_calls(a.did.clone(), false).unwrap();
    eventually(40, "E holds N's downgrade", || !e.node.calls_allowed(a.did.clone()));
    let _ = (e.rx.try_iter().count(), n.rx.try_iter().count());
    let t0 = Instant::now();
    let call = a.node.call(e.did.clone()).unwrap();
    let (reason, _) = ended_after(&a, &call.call_id, t0, 60);
    assert_eq!(reason, CALLS_NOT_ALLOWED);
    std::thread::sleep(Duration::from_millis(500));
    assert!(!saw_incoming(&e) && !saw_incoming(&n), "neither own device may ring");
}
