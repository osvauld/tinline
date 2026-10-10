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
//! Today's closest thing to a block is `Node::remove_contact` (forget + block the DID). Tests
//! about refusal use [`block_today`], which calls it; when R1 lands, switch it to
//! `harness::block` (which keeps the contact). Whether "remove" and "block" stay separate actions
//! is an open question (see the report for packet R1a).
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

/// Proposed R1 API for visible blocking. Each stub documents the behaviour the tests expect.
/// When the feature lands, replace each body with the real `Node` call (or delete the stub and
/// call the method directly) and remove the matching `#[ignore]`s.
#[allow(dead_code)]
mod harness {
    use p2pcore::{Error, Node};

    /// The single reason string carried by the notice and every later refusal.
    pub const BLOCKED: &str = "blocked";

    /// A verified "blocked" notice as the blocked side holds it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct BlockNotice {
        /// The DID that blocked us; the notice is signed by this DID's key (not a device key).
        pub from_did: String,
        /// Unix seconds, from the signed body.
        pub signed_at: u64,
        /// Always `"blocked"`.
        pub reason: String,
    }

    /// What the UI shows for a contact, block-wise.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct BlockState {
        /// We blocked them: chat shows "Blocked" + Unblock; the contact is still listed.
        pub blocked_by_me: bool,
        /// They blocked us: "<name> blocked you", composing and calling disabled. Only set from a
        /// notice (or a refusal carrying the reason) that verified under their DID.
        pub blocked_me: Option<BlockNotice>,
    }

    /// Personal block of `did` (U5 revised). Keeps the contact (shown as "Blocked"), adds the DID
    /// to the block list (synced to own devices), revokes its message and call tokens (U13),
    /// sends ONE DID-signed notice to the contact (retried until delivered), and from now on
    /// refuses its calls, chat sync, history fetches and ticket redemptions with `BLOCKED`.
    /// Does NOT rotate any endpoint (U5) and does NOT change group membership.
    pub fn block(_node: &Node, _did: &str) -> Result<(), Error> {
        unimplemented!("R1: visible block (keep contact, one signed notice)")
    }

    /// Lifts a personal block: accept messages again; calls only if the contact held a call
    /// token before the block (U13). Sends the current device list / private endpoints (U8).
    pub fn unblock(_node: &Node, _did: &str) -> Result<(), Error> {
        unimplemented!("R1: unblock")
    }

    /// Block state of `did` as the UI reads it.
    pub fn block_state(_node: &Node, _did: &str) -> BlockState {
        unimplemented!("R1: block state for the UI")
    }

    /// For tests: how many "blocked" notices this node has sent to `did` since its last block
    /// (retries of the same undelivered notice count once). Expected: exactly 1 per block.
    pub fn block_notices_sent_for_test(_node: &Node, _did: &str) -> u32 {
        unimplemented!("R1: notice counter")
    }

    /// For tests: a "blocked" notice claiming to come from `claimed_did` but signed by `signer`'s
    /// DID key (a forgery when the two differ), as wire JSON.
    pub fn forge_block_notice_for_test(_signer: &Node, _claimed_did: &str) -> String {
        unimplemented!("R1: notice forging for tests")
    }

    /// For tests: hands `notice_json` to `node` as if it arrived from `from_did`; whether it was
    /// accepted (signature verified under `from_did`'s DID key, U10).
    pub fn offer_block_notice_for_test(_node: &Node, _from_did: &str, _notice_json: &str) -> bool {
        unimplemented!("R1: notice intake for tests")
    }

    /// Why message `msg_id` to `did` was not delivered, if it was finally refused (UI: "Not
    /// delivered: <name> blocked you" for `Some("blocked")`). `None` while pending or delivered.
    pub fn not_delivered_reason(_node: &Node, _did: &str, _msg_id: &str) -> Option<String> {
        unimplemented!("R1: per-message refusal reason")
    }

    /// U13/U15: whether `did` holds our call token (ticket contacts: true by default).
    pub fn calls_allowed(_node: &Node, _did: &str) -> bool {
        unimplemented!("R1/P6: separate call token")
    }

    /// U13: grant ("allow calls") or revoke the call token of `did`; the message token is kept.
    pub fn set_calls_allowed(_node: &Node, _did: &str, _allowed: bool) -> Result<(), Error> {
        unimplemented!("R1/P6: separate call token")
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
    #[allow(dead_code)]
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

/// Today's block: `remove_contact` (forgets the contact and blocks the DID). Switch to
/// `harness::block` when R1 lands.
fn block_today(p: &Peer, did: &str) {
    p.node.remove_contact(did.to_string()).unwrap();
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
#[ignore = "R1: today the blocked caller ends 'unreachable' (Reject 'not accepted')"]
fn blocked_caller_ends_at_once_with_the_blocked_reason() {
    let (a, b) = pair("callrsn");
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
#[ignore = "R1: today the blocked side's call ends 'hangup_remote'"]
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
#[ignore = "R1: today the ticket refusal reads 'not accepted'"]
fn blocked_contact_redeeming_a_fresh_ticket_is_refused_as_blocked() {
    let (a, b) = pair("tktrsn");
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
#[ignore = "R1: needs visible block + notice"]
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
#[ignore = "R1: needs visible block + notice"]
fn a_forged_block_notice_is_refused() {
    let (a, b) = pair("forge");
    let x = peer("forge-x");
    let forged = harness::forge_block_notice_for_test(&x.node, &a.did);
    assert!(!harness::offer_block_notice_for_test(&b.node, &a.did, &forged));
    assert!(harness::block_state(&b.node, &a.did).blocked_me.is_none());
    let genuine = harness::forge_block_notice_for_test(&a.node, &a.did);
    assert!(harness::offer_block_notice_for_test(&b.node, &a.did, &genuine), "control: a genuine notice is taken");
}

// KIND: acceptance
// Decision: U5 revised + 8b Q1 — a message from the blocked side is refused quickly and shows
// "Not delivered: <name> blocked you" (reason "blocked"), never stuck pending.
#[test]
#[ignore = "R1: needs per-message refusal reason (today the message stays Pending)"]
fn blocked_senders_message_shows_not_delivered_blocked() {
    let (a, b) = pair("nd");
    harness::block(&a.node, &b.did).unwrap();
    let t0 = Instant::now();
    let m = b.node.send_text(a.did.clone(), "hi".into(), None).unwrap().id;
    eventually(QUICK.as_secs(), "refused", || harness::not_delivered_reason(&b.node, &a.did, &m).is_some());
    assert_eq!(harness::not_delivered_reason(&b.node, &a.did, &m).as_deref(), Some(BLOCKED));
    assert!(t0.elapsed() < QUICK);
    assert!(!holds(&a, &b.did, &m));
}

// KIND: acceptance
// Decision: U5 revised + 8b Q1/Q2 — messages queued while the blocked side was offline surface
// as not delivered with the same reason once it is back, and by then it holds the notice.
#[test]
#[ignore = "R1: needs visible block + per-message refusal reason"]
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
#[ignore = "R1: needs visible block + notice counter"]
fn every_later_attempt_gets_the_same_reason_and_no_second_notice() {
    let (a, b) = pair("repeat");
    harness::block(&a.node, &b.did).unwrap();
    for i in 0..3 {
        let t0 = Instant::now();
        let call = b.node.call(a.did.clone()).unwrap();
        assert_eq!(ended_after(&b, &call.call_id, t0, 20).0, BLOCKED, "call attempt {i}");
        let m = b.node.send_text(a.did.clone(), format!("try {i}"), None).unwrap().id;
        eventually(QUICK.as_secs(), "refused", || harness::not_delivered_reason(&b.node, &a.did, &m).is_some());
        assert_eq!(harness::not_delivered_reason(&b.node, &a.did, &m).as_deref(), Some(BLOCKED));
    }
    assert!(!saw_incoming(&a));
    assert_eq!(harness::block_notices_sent_for_test(&a.node, &b.did), 1);
}

// KIND: acceptance
// Decision: U5 revised (block list syncs to own devices) + 8b Q1 — the linked sibling refuses
// the blocked contact with the same "blocked" reason and shows the contact as Blocked.
#[test]
#[ignore = "R1: needs visible block (sibling refusal reason)"]
fn linked_sibling_refuses_with_the_blocked_reason() {
    let a = peer("sibr-a");
    let e = peer("sibr-e");
    e.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let n = link_fresh(&e, "sibr-n");
    eventually(40, "N has A", || n.node.contacts().iter().any(|c| c.did == a.did));
    teach_devices_by_call(&a, &e);
    harness::block(&e.node, &a.did).unwrap();
    eventually(40, "N shows A as Blocked", || harness::block_state(&n.node, &a.did).blocked_by_me);
    e.node.stop(); // only the sibling can answer now
    let t0 = Instant::now();
    let call = a.node.call(e.did.clone()).unwrap();
    let (reason, took) = ended_after(&a, &call.call_id, t0, 40);
    assert_eq!(reason, BLOCKED);
    assert!(took < QUICK, "{took:?}");
    assert!(!saw_incoming(&n));
}

// KIND: acceptance
// Decision: U5 revised / 8b Q5 — unblock restores messaging in both directions; the blocked
// side's "blocked you" state clears once delivery works again.
#[test]
#[ignore = "R1: needs visible block + unblock"]
fn unblock_restores_messaging() {
    let (a, b) = pair("unblk");
    harness::block(&a.node, &b.did).unwrap();
    eventually(30, "b holds the notice", || harness::block_state(&b.node, &a.did).blocked_me.is_some());
    harness::unblock(&a.node, &b.did).unwrap();
    assert!(!harness::block_state(&a.node, &b.did).blocked_by_me);
    let m = b.node.send_text(a.did.clone(), "after unblock".into(), None).unwrap().id;
    eventually(60, "a holds it", || holds(&a, &b.did, &m));
    eventually(30, "b sees it delivered", || delivered(&b, &a.did, &m));
    let r = a.node.send_text(b.did.clone(), "welcome back".into(), None).unwrap().id;
    eventually(60, "b holds the reply", || holds(&b, &a.did, &r));
    assert!(harness::block_state(&b.node, &a.did).blocked_me.is_none());
}

// KIND: acceptance
// Decision: U13/U15 (separate message and call tokens) + U5 revised — unblock gives calls back
// only to a contact that was allowed to call before the block; a message-only contact stays
// message-only and is not told "blocked".
#[test]
#[ignore = "R1: needs visible block + unblock + separate call token (P6)"]
fn unblock_restores_calls_only_if_previously_allowed() {
    let (a, b) = pair("unblkc-b");
    let c = peer("unblkc-c");
    c.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    assert!(harness::calls_allowed(&a.node, &b.did), "ticket contacts get message+call (U13 default)");
    harness::set_calls_allowed(&a.node, &c.did, false).unwrap();
    for d in [&b.did, &c.did] {
        harness::block(&a.node, d).unwrap();
        harness::unblock(&a.node, d).unwrap();
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
    assert!(took < QUICK, "{took:?}");
    assert!(!saw_incoming(&a));
    let m = c.node.send_text(a.did.clone(), "messages still work".into(), None).unwrap().id;
    eventually(60, "a holds c's message", || holds(&a, &c.did, &m));
}

// KIND: acceptance
// Decision: U5 revised — a personal block does not change group membership (only U9 removal
// does); both stay members and the group keeps carrying both members' posts.
#[test]
#[ignore = "R1: needs group chat API (no group API in p2pcore yet)"]
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
