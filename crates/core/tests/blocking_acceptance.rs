//! R1a: acceptance tests for visible blocking (U5 revised, section 8b of
//! `docs/design/rotation-open-decisions.md` on `endpoint-rotation-blocking`), on real nodes over
//! real iroh endpoints (needs network, like `calls.rs`). These tests define "done" for a feature
//! that is not built yet.
//!
//! Target behaviour, as decided by the user and drawn in the UI design canvas:
//! - Blocking sends ONE DID-signed "blocked" notice; the blocked side verifies it (U10) and shows
//!   "<name> blocked you" with composing/calling disabled. The blocker's chat shows "Blocked" and
//!   an Unblock action (the contact is kept, not forgotten).
//! - Every later attempt by the blocked side (call, chat, history fetch, ticket) is refused with
//!   the same reason, `"blocked"` — not "not accepted", not a timeout — and quickly. Its messages
//!   show "Not delivered: <name> blocked you"; calls end at once with that reason.
//! - The block list syncs across own devices; a sibling applies it the same way.
//! - Blocked contacts are excluded from later device-list / address notes (U8, U1).
//! - Blocking does not trigger an endpoint rotation (U5) and does not change group membership.
//! - Unblock restores messaging; calls only if they were allowed before (U13/U15 tokens).
//!
//! What runs today vs ignored:
//! - Tests that need an API that does not exist call stubs in `mod harness` below
//!   (`unimplemented!("R1: ...")`) and are `#[ignore = "R1: needs ..."]`.
//! - Tests that use only today's API but whose target outcome differs from today's are ignored
//!   with `"R1: today ..."` stating today's outcome; they assert the TARGET, never today's value.
//! - The rest run now and must keep passing.
//!
//! P5 built the block (U19: `remove_contact` became `block`; U20: every notice is also a request
//! to delete our contact entry on their side, history kept read-only). Tests of the blocker's
//! backstop make the blocked node ignore notices (`ignore_block_notices_for_test`), i.e. act
//! like a modified build, so the refusal itself is what is tested.
//!
//! Every test carries `// KIND: acceptance` and a line naming the decision it enforces.

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, DeliveryState, LinkEvents, Message, Node, NodeEvents, NodeStatus};

const PASS: &str = "test-passphrase";
/// A refusal must be "quick": well under any dial timeout (`DIAL_TIMEOUT` is 30 s).
const QUICK: Duration = Duration::from_secs(5);
/// The one refusal reason the blocked side sees on every attempt (U5 revised, 8b Q1).
const BLOCKED: &str = harness::BLOCKED;

// ---------------------------------------------------------------------------------------------
// Test-only harness: the API R1 must provide. Every function is a stub today.
// ---------------------------------------------------------------------------------------------

/// The R1 API for visible blocking (P5) and call tokens (P6), as the tests call it. Stubs remain
/// only for the group API.
#[allow(dead_code)]
mod harness {
    use p2pcore::{Error, Node};
    pub use p2pcore::BlockState;

    /// The single reason string carried by the notice and every later refusal.
    pub const BLOCKED: &str = p2pcore::BLOCKED;

    /// Personal block of `did` (U5 revised, U19, U20): keeps the contact, one DID-signed notice
    /// (which also asks their app to delete its contact entry for us), refusal with `BLOCKED`.
    pub fn block(node: &Node, did: &str) -> Result<(), Error> {
        node.block(did.to_string())
    }

    /// Lifts a personal block and sends the signed unblock notice.
    pub fn unblock(node: &Node, did: &str) -> Result<(), Error> {
        node.unblock(did.to_string())
    }

    pub fn block_state(node: &Node, did: &str) -> BlockState {
        node.block_state(did.to_string())
    }

    /// Notices signed for `did` since its last block (retries count once). Expected: 1.
    pub fn block_notices_sent_for_test(node: &Node, did: &str) -> u32 {
        node.block_notices_sent_for_test(did.to_string())
    }

    /// A block notice to `to` claiming to come from `claimed_did`, signed by `signer`'s DID key
    /// (a forgery when the two differ), as wire JSON.
    pub fn forge_block_notice_for_test(signer: &Node, claimed_did: &str, to: &str) -> String {
        signer.forge_block_notice_for_test(claimed_did.to_string(), to.to_string()).unwrap()
    }

    /// Hands `notice_json` to `node` as if it had arrived; whether it was taken.
    pub fn offer_block_notice_for_test(node: &Node, notice_json: &str) -> bool {
        node.offer_block_notice_for_test(notice_json.to_string())
    }

    /// Why message `msg_id` to `did` was not delivered (`Some("blocked")` while they block us).
    pub fn not_delivered_reason(node: &Node, did: &str, msg_id: &str) -> Option<String> {
        node.not_delivered_reason(did.to_string(), msg_id.to_string())
    }

    /// U13/U15: whether `did` holds our call token (ticket contacts: true by default).
    pub fn calls_allowed(node: &Node, did: &str) -> bool {
        node.calls_allowed(did.to_string())
    }

    /// U13: grant ("allow calls") or revoke the call token of `did`; the message token is kept.
    pub fn set_calls_allowed(node: &Node, did: &str, allowed: bool) -> Result<(), Error> {
        node.allow_calls(did.to_string(), allowed)
    }

    // --- group stubs, only for the "block does not change group membership" test ---

    /// Creates a group hosted by `node`; returns its id.
    pub fn create_group(_node: &Node, _name: &str) -> Result<String, Error> {
        unimplemented!("R1: group chat API")
    }
    /// Host adds contact `did` as a member of `group`.
    pub fn add_group_member(_host: &Node, _group: &str, _did: &str) -> Result<(), Error> {
        unimplemented!("R1: group chat API")
    }
    /// The member DIDs of `group` as `node` sees them.
    pub fn group_members(_node: &Node, _group: &str) -> Vec<String> {
        unimplemented!("R1: group chat API")
    }
    /// Posts `text` to `group`; returns the message id.
    pub fn send_group_text(_node: &Node, _group: &str, _text: &str) -> Result<String, Error> {
        unimplemented!("R1: group chat API")
    }
    /// (author DID, text) of every message `node` holds in `group`.
    pub fn group_texts(_node: &Node, _group: &str) -> Vec<(String, String)> {
        unimplemented!("R1: group chat API")
    }
}

// ---------------------------------------------------------------------------------------------
// Real-node helpers (copied from calls.rs / link.rs; integration tests share no module).
// ---------------------------------------------------------------------------------------------

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

fn online(n: &Node) {
    n.start().unwrap();
    let t = Instant::now();
    while !n.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn blank(name: &str) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-r1a-blk-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = mpsc::channel();
    let (ltx, lrx) = mpsc::channel();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Tap(Mutex::new(tx), logs.clone()))).unwrap();
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
        match p.lrx.recv_timeout(Duration::from_secs(30)).expect("link code") {
            LEv::Code(c) => break c,
            _ => {}
        }
    };
    assert_eq!(code(e), code(&n));
    e.node.link_approve(Some(PASS.into())).unwrap();
    let did = loop {
        match n.lrx.recv_timeout(Duration::from_secs(30)).expect("link done") {
            LEv::Done(d) => break d,
            _ => {}
        }
    };
    n.node.set_passphrase(None, "n-pass".into()).unwrap();
    online(&n.node);
    n.did = did;
    n
}

fn block_today(p: &Peer, did: &str) {
    harness::block(&p.node, did).unwrap();
}

/// `p` acts like a modified build that ignores block notices: only the blocker's refusal holds.
fn ignores_notices(p: &Peer) {
    p.node.ignore_block_notices_for_test(true);
}

fn refused_as_blocked<T: std::fmt::Debug>(r: Result<T, p2pcore::Error>) -> bool {
    matches!(r, Err(p2pcore::Error::Rejected(ref why)) if why == BLOCKED)
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

/// How the call `id` ended on `p`, and how long after `t0`.
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

/// A calls E (E answers) so A learns E's device list, including linked siblings (today the list
/// rides only on call hello/accept).
fn teach_devices_by_call(a: &Peer, e: &Peer) {
    let call = a.node.call(e.did.clone()).unwrap();
    incoming(e, 30);
    e.node.answer(call.call_id.clone()).unwrap();
    eventually(20, "active", || a.node.current_call().is_some());
    std::thread::sleep(Duration::from_millis(500));
    a.node.hangup(call.call_id.clone()).unwrap();
    ended_after(e, &call.call_id, Instant::now(), 20);
}

// ---------------------------------------------------------------------------------------------
// Tests that run today
// ---------------------------------------------------------------------------------------------

// KIND: acceptance
// Decision: U5 revised — blocking does not trigger an endpoint rotation (nor change the DID).
#[test]
fn blocking_does_not_change_the_endpoint_or_the_did() {
    let (a, b) = pair("norot");
    let (dev, did) = (a.node.device_key_for_test().unwrap(), a.did.clone());
    block_today(&a, &b.did);
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(a.node.device_key_for_test().unwrap(), dev, "a block must not rotate the endpoint");
    assert_eq!(a.node.profile().unwrap().did, did);
}

// KIND: acceptance
// Decision: U5 revised / U12 detail — refusal is immediate: a blocked caller never rings the
// blocker and its call ends quickly (the reason itself is checked in
// `blocked_caller_ends_at_once_with_the_blocked_reason`).
#[test]
fn blocked_caller_never_rings_and_ends_quickly() {
    let (a, b) = pair("noring");
    block_today(&a, &b.did);
    let t0 = Instant::now();
    let call = b.node.call(a.did.clone()).unwrap();
    let (reason, took) = ended_after(&b, &call.call_id, t0, 40);
    assert!(took < QUICK, "ended '{reason}' after {took:?}");
    std::thread::sleep(Duration::from_millis(500));
    assert!(!saw_incoming(&a), "a blocked caller must not ring the blocker");
}

// KIND: acceptance
// Decision: U5 revised — the blocked contact's traffic is refused: its new messages never reach
// the blocker's conversation.
#[test]
fn blocked_senders_messages_never_reach_the_blocker() {
    let (a, b) = pair("nomsg");
    let warm = b.node.send_text(a.did.clone(), "before".into(), None).unwrap().id;
    eventually(60, "a holds the first message", || holds(&a, &b.did, &warm));
    block_today(&a, &b.did);
    let m = b.node.send_text(a.did.clone(), "after the block".into(), None).unwrap().id;
    std::thread::sleep(Duration::from_secs(8));
    assert!(!holds(&a, &b.did, &m), "a blocked contact's message reached the blocker");
    assert!(!delivered(&b, &a.did, &m), "the blocked side must not see it as delivered");
}

// KIND: acceptance
// Decision: U5 revised (blocked list syncs across own devices) + U12 detail (refusal is
// immediate): a block made on E holds on its linked sibling N; the blocked caller rings neither.
#[test]
fn block_on_one_device_holds_on_the_linked_sibling() {
    let a = peer("sib-a");
    let e = peer("sib-e");
    e.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let n = link_fresh(&e, "sib-n");
    eventually(40, "N has A", || n.node.contacts().iter().any(|c| c.did == a.did));
    teach_devices_by_call(&a, &e);
    let n_dev = n.node.device_key_for_test().unwrap();
    eventually(20, "A knows N", || a.node.contact_devices_for_test(e.did.clone()).contains(&n_dev));
    let _ = (e.rx.try_iter().count(), n.rx.try_iter().count());

    block_today(&e, &a.did);
    eventually(40, "N holds the block", || n.node.blocked_for_test().contains(&a.did));

    let t0 = Instant::now();
    let call = a.node.call(e.did.clone()).unwrap();
    let (reason, took) = ended_after(&a, &call.call_id, t0, 60);
    assert!(took < QUICK, "ended '{reason}' after {took:?}");
    std::thread::sleep(Duration::from_millis(500));
    assert!(!saw_incoming(&e) && !saw_incoming(&n), "neither own device may ring");
}

// ---------------------------------------------------------------------------------------------
// Target behaviour that differs from today (only today's API; ignored with today's outcome)
// ---------------------------------------------------------------------------------------------

// KIND: acceptance
// Decision: U5 revised + 8b Q1 — every later call attempt ends at once with the reason
// "blocked", both live and in the call record.
#[test]
fn blocked_caller_ends_at_once_with_the_blocked_reason() {
    let (a, b) = pair("callrsn");
    ignores_notices(&b);
    block_today(&a, &b.did);
    for _ in 0..2 {
        let t0 = Instant::now();
        let call = b.node.call(a.did.clone()).unwrap();
        let (reason, took) = ended_after(&b, &call.call_id, t0, 40);
        assert_eq!(reason, BLOCKED);
        assert!(took < QUICK, "{took:?}");
        assert_eq!(b.node.recent_calls(1)[0].reason, BLOCKED);
    }
    assert!(!saw_incoming(&a));
}

// KIND: acceptance
// Decision: U5 revised + U12 detail — blocking during a live call ends it at once on the blocked
// side with the reason "blocked" (not "hangup_remote").
#[test]
fn blocking_mid_call_ends_it_with_the_blocked_reason() {
    let (a, b) = pair("midcall");
    let call = b.node.call(a.did.clone()).unwrap();
    incoming(&a, 30);
    a.node.answer(call.call_id.clone()).unwrap();
    eventually(20, "active", || b.node.current_call().is_some());
    let t0 = Instant::now();
    block_today(&a, &b.did);
    let (reason, took) = ended_after(&b, &call.call_id, t0, 20);
    assert_eq!(reason, BLOCKED);
    assert!(took < QUICK, "{took:?}");
}

// KIND: acceptance
// Decision: U5 revised + 8b Q1 (same reason on every later attempt, incl. tickets) — the
// blocked contact redeeming a fresh ticket of the blocker is refused with "blocked".
#[test]
fn blocked_contact_redeeming_a_fresh_ticket_is_refused_as_blocked() {
    let (a, b) = pair("tktrsn");
    ignores_notices(&b);
    block_today(&a, &b.did);
    let t0 = Instant::now();
    match b.node.add_contact(a.node.my_ticket().unwrap()) {
        Err(p2pcore::Error::Rejected(r)) => assert_eq!(r, BLOCKED),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(t0.elapsed() < QUICK);
}

// KIND: acceptance
// Decision: U8 + U5 revised — a device-list change (a linked device) is pushed to every
// unblocked contact, and never to a blocked one.
#[test]
#[ignore = "R1/P4: today the device list travels only inside call hello/accept (no push)"]
fn blocked_contact_is_excluded_from_device_list_notes() {
    let e = peer("dl-e");
    let b = peer("dl-b");
    let c = peer("dl-c");
    b.node.add_contact(e.node.my_ticket().unwrap()).unwrap();
    c.node.add_contact(e.node.my_ticket().unwrap()).unwrap();
    block_today(&e, &b.did);
    let n = link_fresh(&e, "dl-n");
    let n_dev = n.node.device_key_for_test().unwrap();
    // No call: the change must be pushed.
    eventually(60, "C learns N", || c.node.contact_devices_for_test(e.did.clone()).contains(&n_dev));
    std::thread::sleep(Duration::from_secs(5));
    assert!(!b.node.contact_devices_for_test(e.did.clone()).contains(&n_dev), "a blocked contact learned the new device");
}

// ---------------------------------------------------------------------------------------------
// Target behaviour that needs the R1 API (stubs)
// ---------------------------------------------------------------------------------------------

// KIND: acceptance
// Decision: U5 revised + 8b Q1 — one DID-signed notice; the blocked side verifies it and shows
// "<name> blocked you"; the blocker keeps the contact as "Blocked".
#[test]
fn block_sends_one_signed_notice_the_blocked_side_verifies() {
    let (a, b) = pair("notice");
    harness::block(&a.node, &b.did).unwrap();
    let mine = harness::block_state(&a.node, &b.did);
    assert!(mine.blocked_by_me && mine.blocked_me.is_none());
    assert!(a.node.contacts().iter().any(|c| c.did == b.did), "the blocker keeps the contact (shown as Blocked)");
    eventually(30, "b holds the notice", || harness::block_state(&b.node, &a.did).blocked_me.is_some());
    let notice = harness::block_state(&b.node, &a.did).blocked_me.unwrap();
    assert_eq!((notice.from_did.as_str(), notice.reason.as_str()), (a.did.as_str(), BLOCKED));
    assert!(!harness::block_state(&b.node, &a.did).blocked_by_me, "being blocked is not blocking");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(harness::block_notices_sent_for_test(&a.node, &b.did), 1, "exactly one notice");
}

// KIND: acceptance
// Decision: U10 (every device verifies every signature) applied to the U5-revised notice — a
// notice not signed by the claimed DID is refused and changes nothing.
#[test]
fn a_forged_block_notice_is_refused() {
    let (a, b) = pair("forge");
    let x = peer("forge-x");
    let forged = harness::forge_block_notice_for_test(&x.node, &a.did, &b.did);
    assert!(!harness::offer_block_notice_for_test(&b.node, &forged));
    assert!(harness::block_state(&b.node, &a.did).blocked_me.is_none());
    assert!(b.node.contacts().iter().any(|c| c.did == a.did), "a forgery deletes nothing");
    // Genuine, but addressed to x: b must not take it.
    let misaddressed = harness::forge_block_notice_for_test(&a.node, &a.did, &x.did);
    assert!(!harness::offer_block_notice_for_test(&b.node, &misaddressed));
    assert!(harness::block_state(&b.node, &a.did).blocked_me.is_none());
    let genuine = harness::forge_block_notice_for_test(&a.node, &a.did, &b.did);
    assert!(harness::offer_block_notice_for_test(&b.node, &genuine), "control: a genuine notice is taken");
    // Replayed: refused, state unchanged.
    let held = harness::block_state(&b.node, &a.did).blocked_me;
    assert!(held.is_some());
    assert!(!harness::offer_block_notice_for_test(&b.node, &genuine), "a replay is not taken again");
    assert_eq!(harness::block_state(&b.node, &a.did).blocked_me, held);
}

// KIND: acceptance
// Decision: U5 revised + 8b Q1 — a message from the blocked side is refused quickly and shows
// "Not delivered: <name> blocked you" (reason "blocked"), never stuck pending.
#[test]
fn blocked_senders_message_shows_not_delivered_blocked() {
    let (a, b) = pair("nd");
    // b writes while a is away, so the message exists before any notice; a blocks meanwhile.
    a.node.stop();
    let m = b.node.send_text(a.did.clone(), "hi".into(), None).unwrap().id;
    harness::block(&a.node, &b.did).unwrap();
    let t0 = Instant::now();
    online(&a.node);
    eventually(QUICK.as_secs(), "refused", || harness::not_delivered_reason(&b.node, &a.did, &m).is_some());
    assert_eq!(harness::not_delivered_reason(&b.node, &a.did, &m).as_deref(), Some(BLOCKED));
    assert!(t0.elapsed() < QUICK);
    assert!(!holds(&a, &b.did, &m));
    // With the notice held, a new message is refused at once with the same reason.
    assert!(refused_as_blocked(b.node.send_text(a.did.clone(), "again".into(), None)));
}

// KIND: acceptance
// Decision: U5 revised + 8b Q1/Q2 — messages queued while the blocked side was offline surface
// as not delivered with the same reason once it is back, and by then it holds the notice.
#[test]
fn queued_messages_surface_as_not_delivered_blocked() {
    let (a, b) = pair("queued");
    b.node.stop();
    harness::block(&a.node, &b.did).unwrap();
    let m1 = b.node.send_text(a.did.clone(), "queued 1".into(), None).unwrap().id;
    let m2 = b.node.send_text(a.did.clone(), "queued 2".into(), None).unwrap().id;
    online(&b.node);
    for m in [&m1, &m2] {
        eventually(30, "queued message refused", || harness::not_delivered_reason(&b.node, &a.did, m).is_some());
        assert_eq!(harness::not_delivered_reason(&b.node, &a.did, m).as_deref(), Some(BLOCKED));
        assert!(!holds(&a, &b.did, m));
    }
    assert!(harness::block_state(&b.node, &a.did).blocked_me.is_some());
}

// KIND: acceptance
// Decision: U5 revised + 8b Q1 — every later attempt (calls and messages, repeatedly) gets the
// same "blocked" reason, and the blocker sends no second notice.
#[test]
fn every_later_attempt_gets_the_same_reason_and_no_second_notice() {
    let (a, b) = pair("repeat");
    harness::block(&a.node, &b.did).unwrap();
    for i in 0..3 {
        let t0 = Instant::now();
        let call = b.node.call(a.did.clone()).unwrap();
        assert_eq!(ended_after(&b, &call.call_id, t0, 20).0, BLOCKED, "call attempt {i}");
        // Read-only once the notice is held (it is, after a call that ended "blocked").
        assert!(refused_as_blocked(b.node.send_text(a.did.clone(), format!("try {i}"), None)), "message attempt {i}");
    }
    assert!(!saw_incoming(&a));
    assert_eq!(harness::block_notices_sent_for_test(&a.node, &b.did), 1);
}

// KIND: acceptance
// Decision: U5 revised (block list syncs to own devices) + 8b Q1 — the linked sibling refuses
// the blocked contact with the same "blocked" reason and shows the contact as Blocked.
#[test]
fn linked_sibling_refuses_with_the_blocked_reason() {
    let a = peer("sibr-a");
    let e = peer("sibr-e");
    // a ignores the notice, so its call really reaches the sibling.
    ignores_notices(&a);
    e.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let n = link_fresh(&e, "sibr-n");
    eventually(40, "N has A", || n.node.contacts().iter().any(|c| c.did == a.did));
    teach_devices_by_call(&a, &e);
    harness::block(&e.node, &a.did).unwrap();
    eventually(40, "N shows A as Blocked", || harness::block_state(&n.node, &a.did).blocked_by_me);
    assert_eq!(harness::block_notices_sent_for_test(&n.node, &a.did), 0, "the sibling hands out E's notice; it signs none");
    e.node.stop(); // only the sibling can answer now
    let t0 = Instant::now();
    let call = a.node.call(e.did.clone()).unwrap();
    let (reason, took) = ended_after(&a, &call.call_id, t0, 40);
    assert_eq!(reason, BLOCKED);
    assert!(took < QUICK, "{took:?}");
    assert!(!saw_incoming(&n));
}

/// How many times `p` (the blocker) refused something from a blocked DID, by its own log.
fn refusals(p: &Peer) -> usize {
    p.logs.lock().unwrap().iter().filter(|l| l.contains("refused") && l.contains("blocked")).count()
}

// KIND: acceptance
// Decision: U20 + U23 — the blocked person's app enforces the block: once it holds the notice it
// deletes its contact entry for the blocker (history kept, read-only), and never dials or sends
// again: calls end at once with "blocked", messages and history requests are refused locally,
// and the blocker sees no further attempt at all.
#[test]
fn blocked_app_never_dials_or_sends_after_the_notice() {
    let (a, b) = pair("consent");
    let warm = b.node.send_text(a.did.clone(), "before".into(), None).unwrap().id;
    eventually(60, "a holds the first message", || holds(&a, &b.did, &warm));
    harness::block(&a.node, &b.did).unwrap();
    eventually(30, "b holds the notice", || harness::block_state(&b.node, &a.did).blocked_me.is_some());
    assert!(!b.node.contacts().iter().any(|c| c.did == a.did), "the deletion request removed the contact entry");
    assert!(holds(&b, &a.did, &warm), "the history stays");
    assert!(b.node.chats().unwrap().iter().any(|c| c.peer_did == a.did), "the conversation is still listed");
    std::thread::sleep(Duration::from_secs(2));
    let before = refusals(&a);
    for _ in 0..2 {
        let t0 = Instant::now();
        let call = b.node.call(a.did.clone()).unwrap();
        let (reason, took) = ended_after(&b, &call.call_id, t0, 10);
        assert_eq!(reason, BLOCKED);
        assert!(took < Duration::from_secs(1), "the app refuses locally, without dialling: {took:?}");
        assert!(refused_as_blocked(b.node.send_text(a.did.clone(), "hello?".into(), None)));
        assert!(refused_as_blocked(b.node.fetch_older_history(a.did.clone(), "2026-01-01".into())));
    }
    std::thread::sleep(Duration::from_secs(5));
    assert_eq!(refusals(&a), before, "the blocked app must not reach the blocker at all");
    assert!(!saw_incoming(&a));
    assert_eq!(harness::block_notices_sent_for_test(&a.node, &b.did), 1);
}

// KIND: acceptance
// Decision: U20 layer (2) — a modified build that ignores the notice is still refused by the
// blocker's devices, with "blocked", for calls, messages and history requests alike.
#[test]
fn a_modified_app_that_ignores_the_notice_is_still_refused() {
    let (a, b) = pair("modded");
    ignores_notices(&b);
    harness::block(&a.node, &b.did).unwrap();
    std::thread::sleep(Duration::from_secs(2));
    assert!(harness::block_state(&b.node, &a.did).blocked_me.is_none(), "this build ignores notices");
    let t0 = Instant::now();
    let call = b.node.call(a.did.clone()).unwrap();
    let (reason, took) = ended_after(&b, &call.call_id, t0, 20);
    assert_eq!(reason, BLOCKED);
    assert!(took < QUICK, "{took:?}");
    let m = b.node.send_text(a.did.clone(), "let me in".into(), None).unwrap().id;
    assert!(refused_as_blocked(b.node.fetch_older_history(a.did.clone(), "2026-01-01".into())));
    std::thread::sleep(Duration::from_secs(5));
    assert!(!holds(&a, &b.did, &m));
    assert!(!saw_incoming(&a));
    assert!(refusals(&a) >= 2, "the blocker refused the call and the chat");
}

// KIND: acceptance
// Decision: U5 revised / 8b Q5 — unblock restores messaging in both directions; the blocked
// side's "blocked you" state clears once delivery works again.
#[test]
fn unblock_restores_messaging() {
    let (a, b) = pair("unblk");
    let warm = b.node.send_text(a.did.clone(), "before".into(), None).unwrap().id;
    eventually(60, "a holds the first message", || holds(&a, &b.did, &warm));
    harness::block(&a.node, &b.did).unwrap();
    eventually(30, "b holds the notice", || harness::block_state(&b.node, &a.did).blocked_me.is_some());
    // U20: b's app deleted its contact entry for a; the history stays, read-only.
    assert!(!b.node.contacts().iter().any(|c| c.did == a.did));
    assert!(holds(&b, &a.did, &warm));
    harness::unblock(&a.node, &b.did).unwrap();
    assert!(!harness::block_state(&a.node, &b.did).blocked_by_me);
    // The unblock notice is taken from a non-contact whose block b holds: "blocked you" clears.
    eventually(30, "b's blocked-you state clears", || harness::block_state(&b.node, &a.did).blocked_me.is_none());
    assert!(holds(&b, &a.did, &warm));
    // Messaging resumes once b adds a again (U20: the contact entry is gone).
    assert!(matches!(b.node.send_text(a.did.clone(), "x".into(), None), Err(p2pcore::Error::NotFound)));
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let m = b.node.send_text(a.did.clone(), "after unblock".into(), None).unwrap().id;
    eventually(60, "a holds it", || holds(&a, &b.did, &m));
    eventually(30, "b sees it delivered", || delivered(&b, &a.did, &m));
    let r = a.node.send_text(b.did.clone(), "welcome back".into(), None).unwrap().id;
    eventually(60, "b holds the reply", || holds(&b, &a.did, &r));
    assert!(harness::block_state(&b.node, &a.did).blocked_me.is_none());
}

// KIND: acceptance
// Decision: U13/U15 (separate message and call tokens) + U5 revised + U20 — unblock gives calls
// back only to a contact that was allowed to call before the block; a message-only contact stays
// message-only and is not told "blocked". (P6 changed the steps, not the target: the block's
// deletion request removed their entry for a, and the block took their tokens back, so after
// the unblock they re-add a from its ticket and get fresh tokens.)
#[test]
fn unblock_restores_calls_only_if_previously_allowed() {
    let (a, b) = pair("unblkc-b");
    let c = peer("unblkc-c");
    c.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    assert!(harness::calls_allowed(&a.node, &b.did), "ticket contacts get message+call (U13 default)");
    harness::set_calls_allowed(&a.node, &c.did, false).unwrap();
    for p in [&b, &c] {
        harness::block(&a.node, &p.did).unwrap();
        eventually(40, "their app deleted a", || !p.node.contacts().iter().any(|x| x.did == a.did));
        harness::unblock(&a.node, &p.did).unwrap();
        eventually(40, "the unblock reached them", || harness::block_state(&p.node, &a.did).blocked_me.is_none());
        p.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    }
    assert!(harness::calls_allowed(&a.node, &b.did));
    assert!(!harness::calls_allowed(&a.node, &c.did));
    let call = b.node.call(a.did.clone()).unwrap();
    assert_eq!(incoming(&a, 30), call.call_id, "b could call before, so b can call again");
    b.node.hangup(call.call_id).unwrap();
    std::thread::sleep(Duration::from_secs(1));
    let _ = a.rx.try_iter().count();
    let t0 = Instant::now();
    let call = c.node.call(a.did.clone()).unwrap();
    let (reason, took) = ended_after(&c, &call.call_id, t0, 40);
    assert_ne!(reason, BLOCKED, "c is no longer blocked, only not allowed to call");
    assert_eq!(reason, p2pcore::CALLS_NOT_ALLOWED);
    assert!(took < QUICK, "{took:?}");
    assert!(!saw_incoming(&a));
    let m = c.node.send_text(a.did.clone(), "messages still work".into(), None).unwrap().id;
    eventually(60, "a holds c's message", || holds(&a, &c.did, &m));
}

// KIND: acceptance
// Decision: U5 revised — a personal block does not change group membership (only U9 removal
// does); both stay members and the group keeps carrying both members' posts.
#[test]
#[ignore = "needs the group chat API (none in p2pcore yet); P5 touches nothing group-related"]
fn personal_block_does_not_change_group_membership() {
    let h = peer("grp-h");
    let a = peer("grp-a");
    let b = peer("grp-b");
    for p in [&a, &b] {
        p.node.add_contact(h.node.my_ticket().unwrap()).unwrap();
    }
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let g = harness::create_group(&h.node, "family").unwrap();
    harness::add_group_member(&h.node, &g, &a.did).unwrap();
    harness::add_group_member(&h.node, &g, &b.did).unwrap();
    let mut want = vec![h.did.clone(), a.did.clone(), b.did.clone()];
    want.sort();
    let members = |p: &Peer| {
        let mut m = harness::group_members(&p.node, &g);
        m.sort();
        m
    };
    eventually(60, "everyone sees three members", || [&h, &a, &b].iter().all(|p| members(p) == want));

    harness::block(&a.node, &b.did).unwrap();
    std::thread::sleep(Duration::from_secs(5));
    for p in [&h, &a, &b] {
        assert_eq!(members(p), want, "a personal block changed membership");
    }
    harness::send_group_text(&b.node, &g, "from b after the block").unwrap();
    eventually(60, "the host relays b's post", || {
        harness::group_texts(&h.node, &g).iter().any(|(d, t)| d == &b.did && t == "from b after the block")
    });
    // Whether A's view hides or collapses B's posts is a local UI choice (not decided); the
    // group content itself is unchanged, which is what the host's copy shows.
    harness::send_group_text(&a.node, &g, "from a").unwrap();
    eventually(60, "b still receives a's group posts", || {
        harness::group_texts(&b.node, &g).iter().any(|(d, t)| d == &a.did && t == "from a")
    });
}
