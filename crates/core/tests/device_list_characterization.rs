//! P4 update: U8 push (`crates/core/src/devlist.rs`) and the seq bound are implemented; the
//! limitations they fix are flipped to invariants and say so. `push` paths of section 2 replaced
//! the "until a call" assertions.
//!
//! W2-D: characterisation of how a contact learns our signed `DeviceList` TODAY, measured against
//! the user's decision U8 (`docs/design/rotation-open-decisions.md` §0): whenever the list changes
//! (device added, unlinked, restored, rotated) it is pushed to every unblocked contact.
//!
//! Today:
//! - `my_device_list` (`crates/core/src/node.rs:2316`) re-signs lazily, only when it is asked for
//!   the list and the registry changed; `seq = now_ms().max(prev + 1)` (`node.rs:2371`).
//! - It is asked for in exactly two places: the caller's `CallHello.devices` (`node.rs:2510`) and
//!   the callee's `Accept.devices` (`node.rs:2754`). The receiving side merges it with
//!   `merge_device_list` (`node.rs:3122`, via `accept_device_list` `node.rs:2296`, called at
//!   `node.rs:2218` and `:2606`): verify for the contact's DID, keep only if `proto::newer`.
//! - Our own installs pass contacts' lists among themselves in the account doc
//!   (`crates/core/src/sync/accdoc.rs:360`), but nothing sends OUR list to a contact outside a call.
//!   Chat sessions never carry it.
//!
//! Every test carries one label:
//! - `KIND: limitation`: asserts today's outcome where it differs from U8; the comment names the
//!   expected-correct outcome. When production is fixed the assertion flips and the test must be
//!   rewritten.
//! - `KIND: invariant`: holds today and must keep holding under U8.
//!
//! Real iroh endpoints (needs network, like `calls.rs`, `restore.rs`). Waits are bounded polls;
//! "never arrives" claims are bounded and stated per test. No production code is changed; helpers
//! are copied from `rotation_restore_characterization.rs` / `block_characterization.rs` /
//! `devices.rs` (they are not in a shared module).

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, DeliveryState, Message, Node, NodeEvents, NodeStatus};

const PASS: &str = "test-passphrase";

// ---- helpers copied from rotation_restore_characterization.rs / block_characterization.rs ----

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

struct Peer {
    node: Arc<Node>,
    rx: mpsc::Receiver<Ev>,
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
    fn devices_of(&self, other: &Peer) -> Vec<String> {
        self.node.contact_devices_for_test(other.did.clone())
    }
    fn dump(&self) -> String {
        format!("{:?}", self.node.contacts())
    }
}

type Opened = (Arc<Node>, mpsc::Receiver<Ev>, Arc<Mutex<Vec<String>>>);

fn open(dir: &std::path::Path) -> Opened {
    let (tx, rx) = mpsc::channel();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let node = Node::new(
        dir.to_string_lossy().into(),
        Arc::new(Tap(Mutex::new(tx), logs.clone())),
    )
    .unwrap();
    node.auto_accept_contacts_for_test(true);
    (node, rx, logs)
}

fn blank(name: &str) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-w2d-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (node, rx, logs) = open(&dir);
    Peer {
        node,
        rx,
        logs,
        did: String::new(),
        phrase: String::new(),
        dir,
    }
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

/// a and b, each a contact of the other (b scanned a's ticket).
fn pair(tag: &str) -> (Peer, Peer) {
    let a = peer(&format!("{tag}-a"), true);
    let b = peer(&format!("{tag}-b"), true);
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    assert!(a.node.contacts().iter().any(|c| c.did == b.did), "a has b");
    (a, b)
}

/// Registers another (never started) install of `p`'s account with `p`, as in `devices.rs`
/// (`add_own_device_for_test`: real linking is not needed for the list). Returns its device key.
fn register_sibling(p: &Peer, name: &str) -> String {
    let s = blank(name);
    s.node
        .restore_identity(p.phrase.clone(), name.into(), PASS.into())
        .unwrap();
    let key = s.node.device_key_for_test().unwrap();
    p.node
        .add_own_device_for_test(s.node.own_attestation_for_test().unwrap())
        .unwrap();
    key
}

fn own_list(p: &Peer) -> proto::DeviceList {
    let blob: proto::SignedBlob =
        serde_json::from_str(&p.node.own_device_list_for_test().unwrap()).unwrap();
    proto::verify_device_list(&blob, &p.did).unwrap()
}

fn list_devices(l: &proto::DeviceList) -> Vec<String> {
    l.devices.iter().map(|e| e.device.clone()).collect()
}

/// The forgotten-passphrase restore (copied from `rotation_restore_characterization.rs`): stop,
/// reopen the same directory, restore over it. Left NOT started. Returns (old, new) device keys.
fn restore_in_place(p: &mut Peer) -> (String, String) {
    let name = p.node.profile().unwrap().name;
    let old_dev = p.node.device_key_for_test().unwrap();
    p.node.stop();
    let (node, rx, logs) = open(&p.dir);
    let old = std::mem::replace(&mut p.node, node);
    drop(old);
    p.rx = rx;
    p.logs = logs;
    p.node
        .restore_identity(p.phrase.clone(), name, "pass two".into())
        .unwrap();
    assert_eq!(
        p.node.profile().unwrap().did,
        p.did,
        "restore keeps the DID"
    );
    let new_dev = p.node.device_key_for_test().unwrap();
    assert_ne!(new_dev, old_dev, "restore makes a fresh device key");
    (old_dev, new_dev)
}

fn incoming(p: &Peer, secs: u64) -> String {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p
            .rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
        {
            Ok(Ev::Incoming(c)) => return c.call_id,
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for an incoming call"),
        }
    }
}

fn ended(p: &Peer, id: &str, secs: u64) -> String {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p
            .rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
        {
            Ok(Ev::State(i, CallState::Ended { reason })) if i == id => return reason,
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for call {id} to end"),
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

fn all_msgs(p: &Peer, did: &str) -> Vec<Message> {
    let mut out = Vec::new();
    let mut page = p.node.chat_day(did.to_string(), None).ok();
    while let Some(pg) = page {
        out.extend(pg.messages.clone());
        page = pg
            .older_day
            .and_then(|d| p.node.chat_day(did.to_string(), Some(d)).ok());
    }
    out
}

fn has(p: &Peer, did: &str, id: &str) -> bool {
    all_msgs(p, did).iter().any(|m| m.id == id)
}

fn ticks(p: &Peer, did: &str, id: &str) -> DeliveryState {
    all_msgs(p, did)
        .into_iter()
        .find(|m| m.id == id)
        .expect("message held")
        .delivery
}

/// `caller` calls `callee`, the callee answers, the caller hangs up.
fn call_answered(caller: &Peer, callee: &Peer) {
    let call = caller.node.call(callee.did.clone()).unwrap();
    assert_eq!(incoming(callee, 30), call.call_id);
    callee.node.answer(call.call_id.clone()).unwrap();
    eventually(20, "call active", || caller.node.current_call().is_some());
    std::thread::sleep(Duration::from_millis(500));
    caller.node.hangup(call.call_id).unwrap();
    eventually(20, "call over", || {
        caller.node.current_call().is_none() && callee.node.current_call().is_none()
    });
}

/// `caller` calls `callee`; returns once the callee rings (its hello has been processed), then the
/// caller hangs up.
fn call_until_ringing(caller: &Peer, callee: &Peer) {
    let call = caller.node.call(callee.did.clone()).unwrap();
    assert_eq!(incoming(callee, 30), call.call_id);
    caller.node.hangup(call.call_id).unwrap();
    eventually(20, "call over", || {
        caller.node.current_call().is_none() && callee.node.current_call().is_none()
    });
}

/// Sends `text` from `from` to `to` and waits until `to` holds it. Chat is the traffic
/// that would carry a push if anything did.
fn chat(from: &Peer, to: &Peer, text: &str) -> String {
    let id = from
        .node
        .send_text(to.did.clone(), text.into(), None)
        .unwrap()
        .id;
    eventually(60, "message arrives", || has(to, &from.did, &id));
    id
}

// ---- 1. the two carriers that exist ----------------------------------------------------------

// KIND: invariant
// The list does travel in a call, both ways: the caller's `CallHello.devices` gives the callee the
// caller's list (`node.rs:2510` -> `:2218`) and the callee's `Accept.devices` gives the caller the
// callee's list (`node.rs:2754` -> `:2606`). Each side registered a second install beforehand; after
// one answered call each side dials the other's second install too.
#[test]
fn an_answered_call_carries_both_device_lists() {
    let (a, b) = pair("both");
    let a2 = register_sibling(&a, "both-a2");
    let b2 = register_sibling(&b, "both-b2");
    // P4: the push may already have delivered both lists; the call must carry (or at least not
    // undo) them.
    call_answered(&a, &b);
    eventually(10, "A learns B2 from the Accept", || {
        a.devices_of(&b).contains(&b2)
    });
    eventually(10, "B learns A2 from the CallHello", || {
        b.devices_of(&a).contains(&a2)
    });
}

// ---- 2. a change reaches no contact until a call ----------------------------------------------

// KIND: invariant (was a limitation; fixed by P4, U8)
// A holds B's list (one answered call). B adds an install: B's own list is re-signed with a higher
// seq naming it and pushed (`Msg::Devices`); A learns the new install without a call or a chat.
#[test]
fn adding_a_device_reaches_the_contact_without_a_call() {
    let (a, b) = pair("add");
    call_answered(&a, &b);
    let seq0 = own_list(&b).seq;
    let b2 = register_sibling(&b, "add-b2");
    let mine = own_list(&b);
    assert!(
        mine.seq > seq0 && list_devices(&mine).contains(&b2),
        "B re-signed a list naming B2"
    );
    eventually(30, "A learns B2 from the push", || a.devices_of(&b).contains(&b2));
}

// KIND: invariant (was a limitation; fixed by P4, U8)
// A dials B and B2 (learned in a call). B unlinks B2 (never started): the shorter list is pushed
// and A stops dialling B2 without a call.
#[test]
fn unlinking_a_device_reaches_the_contact_without_a_call() {
    let (a, b) = pair("unlink");
    let b2 = register_sibling(&b, "unlink-b2");
    call_answered(&a, &b);
    eventually(10, "A dials B2", || a.devices_of(&b).contains(&b2));
    b.node.unlink_device(b2.clone(), Some(PASS.into())).unwrap();
    assert!(
        !list_devices(&own_list(&b)).contains(&b2),
        "B's own list dropped B2"
    );
    eventually(30, "A drops B2", || !a.devices_of(&b).contains(&b2));
    assert_eq!(a.devices_of(&b), vec![b.node.device_key_for_test().unwrap()]);
}

// ---- 3. restore: the K1 stall and what a push would and would not fix ---------------------------

// KIND: invariant (was a limitation; resolved by DID-signed chat, P8/P9/U18, plus P4's push)
// K1 §5 bug 1 fixture: A holds B's list (one call), B's `m2` is pending (A offline) when B
// restores. A takes B's re-signed list (here offered directly; B's own push sends the same list
// when it starts) and dials only the new key. Before P8 the snapshot holding the old-key-signed
// `m2` was rejected ("signature does not verify") and nothing arrived. Chat content is now signed
// by the DID key, so the restore no longer strands it: both `m2` and `m3` reach A.
#[test]
fn restore_push_and_did_signed_chat_clear_the_pending_message_stall() {
    let a = peer("rst-a", true);
    let mut b = peer("rst-b", true);
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    call_answered(&a, &b);
    let m1 = chat(&b, &a, "m1: delivered before restore");
    eventually(60, "B shows m1 delivered", || {
        ticks(&b, &a.did, &m1) == DeliveryState::Delivered
    });
    a.node.stop();
    let m2 = b
        .node
        .send_text(a.did.clone(), "m2: pending at restore".into(), None)
        .unwrap()
        .id;
    std::thread::sleep(Duration::from_secs(1));
    let (old_dev, new_dev) = restore_in_place(&mut b);
    let pushed = b.node.own_device_list_for_test().unwrap();
    let l: proto::SignedBlob = serde_json::from_str(&pushed).unwrap();
    assert_eq!(
        list_devices(&proto::verify_device_list(&l, &b.did).unwrap()),
        vec![new_dev.clone()]
    );

    online(&a.node);
    assert_eq!(
        a.devices_of(&b),
        vec![old_dev],
        "today: A still holds the pre-restore list"
    );
    assert!(
        a.node.offer_device_list_for_test(b.did.clone(), pushed),
        "the simulated push is taken"
    );
    assert_eq!(a.devices_of(&b), vec![new_dev.clone()]);

    online(&b.node);
    let m3 = b
        .node
        .send_text(a.did.clone(), "m3: after restore".into(), None)
        .unwrap()
        .id;
    eventually(90, "m2 and m3 reach A", || has(&a, &b.did, &m3) && has(&a, &b.did, &m2));
    assert!(!a.logged("rejected history: signature does not verify"));
}

// ---- 4. blocking and rejection ---------------------------------------------------------------

// KIND: invariant
// A blocked contact learns nothing new: B blocks A (P5: visible notice, which also asks A's app to
// delete its entry for B, U20), then adds an install. B never pushes the list to A (P4 excludes
// blocked contacts), and a call from A, if A still has B, ends "blocked" without a list. Bounded:
// 15 s after the block.
#[test]
fn a_blocked_caller_receives_no_device_list() {
    let (a, b) = pair("blk");
    call_answered(&a, &b);
    b.node.block(a.did.clone()).unwrap();
    let b2 = register_sibling(&b, "blk-b2");
    assert!(list_devices(&own_list(&b)).contains(&b2));
    if let Ok(call) = a.node.call(b.did.clone()) {
        let reason = ended(&a, &call.call_id, 40);
        assert_eq!(reason, p2pcore::BLOCKED);
    }
    assert!(!within(15, || a.devices_of(&b).contains(&b2)), "A never learns B2");
    assert!(!b.logged(&format!("device list delivered to {}", a.did)));
}

// KIND: invariant
// `merge_device_list` (`node.rs:3122`) refuses a stale (lower seq), replayed (same blob), forged
// (another key, B's DID), tampered and wrong-DID list, leaving A's contacts unchanged.
#[test]
fn merge_refuses_stale_replayed_forged_tampered_and_wrong_did_lists() {
    let (a, b) = pair("rej");
    let bid = identity::recover(&b.phrase).unwrap();
    let mal = identity::recover(
        &identity::Mnemonic::from_entropy(&[9; 32])
            .unwrap()
            .to_string(),
    )
    .unwrap();
    let x = proto::device_to_text(&[7u8; 32]);
    let k = |t: &str| proto::device_from_text(t).unwrap();
    let mine = b.node.device_key_for_test().unwrap();
    // P4: seqs are relative to now (a seq more than a day ahead is refused by the seq bound).
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let good = proto::sign_device_list(&bid, &[(k(&mine), None), (k(&x), None)], t + 1000);
    assert!(
        a.node
            .offer_device_list_for_test(b.did.clone(), serde_json::to_string(&good).unwrap())
    );

    let stale = proto::sign_device_list(&bid, &[(k(&mine), None)], t);
    let forged = {
        let m = proto::sign_device_list(&mal, &[(k(&x), None)], t + 5000);
        let mut v: serde_json::Value = serde_json::from_slice(&decode(&m.payload)).unwrap();
        v["did"] = b.did.clone().into();
        proto::SignedBlob {
            payload: encode(&serde_json::to_vec(&v).unwrap()),
            signature: m.signature,
        }
    };
    let tampered = {
        let mut v: serde_json::Value = serde_json::from_slice(&decode(&good.payload)).unwrap();
        v["seq"] = serde_json::json!(t + 9000);
        proto::SignedBlob {
            payload: encode(&serde_json::to_vec(&v).unwrap()),
            signature: good.signature.clone(),
        }
    };
    let wrong_did = proto::sign_device_list(&mal, &[(k(&x), None)], t + 5000);
    for (what, blob) in [
        ("stale", stale),
        ("replayed", good.clone()),
        ("forged", forged),
        ("tampered", tampered),
        ("wrong did", wrong_did),
    ] {
        let before = a.dump();
        assert!(
            !a.node
                .offer_device_list_for_test(b.did.clone(), serde_json::to_string(&blob).unwrap()),
            "{what}"
        );
        assert_eq!(a.dump(), before, "{what}: unchanged");
    }
    assert_eq!(a.devices_of(&b), vec![mine, x]);
}

// ---- 5. shared mnemonic ----------------------------------------------------------------------

// KIND: invariant (was a limitation; fixed by P4's seq bound, PROVISIONAL (coordinator))
// Any phrase holder can sign a list at `seq = u64::MAX`; A refuses it (seq more than a day ahead
// of A's clock), keeps dialling B's real device, and B's honest list still reaches A.
#[test]
fn a_max_seq_list_from_any_phrase_holder_is_refused() {
    let (a, b) = pair("pin");
    let holder = identity::recover(&b.phrase).unwrap();
    let hostile = proto::device_to_text(&[8u8; 32]);
    let pinned = proto::sign_device_list(
        &holder,
        &[(proto::device_from_text(&hostile).unwrap(), None)],
        u64::MAX,
    );
    assert!(
        !a.node
            .offer_device_list_for_test(b.did.clone(), serde_json::to_string(&pinned).unwrap())
    );
    let b2 = register_sibling(&b, "pin-b2");
    eventually(30, "B's honest list reaches A", || a.devices_of(&b).contains(&b2));
    assert!(!a.devices_of(&b).contains(&hostile));
}

// URL-safe no-pad base64 (the encoding of `SignedBlob`), so the test can tamper with payloads.
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
fn encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |a, b| (a << 8) | *b as u32) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}
fn decode(text: &str) -> Vec<u8> {
    let vals: Vec<u32> = text
        .bytes()
        .map(|c| B64.iter().position(|x| *x == c).unwrap() as u32)
        .collect();
    let mut out = Vec::new();
    for chunk in vals.chunks(4) {
        let n = chunk.iter().fold(0u32, |a, b| (a << 6) | b) << (6 * (4 - chunk.len()));
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}
