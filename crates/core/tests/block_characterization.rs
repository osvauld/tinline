//! W2-B: characterisation of what blocking does TODAY in the shipped core, measured against the
//! user's decision U5 (`docs/design/rotation-open-decisions.md` §0): a block is silent, the
//! blocked party's traffic is dropped, and what it observes is the same as "offline".
//!
//! Today, blocking is `Node::remove_contact` (`crates/core/src/node.rs:984`): the contact is
//! forgotten, its DID goes into `State.blocked` (`crates/core/src/store.rs:171`) and the current
//! ticket is dropped. Nothing is sent to the blocked party. Incoming traffic from it is then
//! turned away by:
//! - calls: `incoming_call` answers `Msg::Reject { reason: "not accepted" }`
//!   (`node.rs:2168`–`2177`); the caller maps that to `LegEvent::Failed("rejected: …")`
//!   (`node.rs:347`), logs `device N of <name>: rejected: not accepted` through
//!   `NodeEvents::on_log` (`node.rs:2632`) and ends the call `"unreachable"` (`node.rs:2573`);
//! - chat: `verify_auth` fails (`chat/engine.rs:336`) and the listener closes the QUIC connection
//!   with application reason `"not accepted"` (`chat/engine.rs:466`);
//! - tickets: `accept_hello` refuses (`node.rs:2034`), answered with the same `Reject` as any
//!   other refused hello.
//!
//! Every test carries one label:
//! - `KIND: limitation`: asserts today's outcome where it differs from U5; the comment names the
//!   expected-correct outcome. When production is fixed the assertion flips and the test must be
//!   rewritten.
//! - `KIND: invariant`: holds today and must keep holding under U5.
//!
//! Real iroh endpoints (needs network, like `calls.rs`, `restore.rs`). Waits are bounded polls.
//! No production code is changed; helpers are copied from `calls.rs` /
//! `rotation_restore_characterization.rs` (they are not in a shared module).

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, DeliveryState, Error, Message, Node, NodeEvents, NodeStatus};

const PASS: &str = "test-passphrase";

// ---- helpers copied from rotation_restore_characterization.rs / calls.rs ----

#[derive(Debug, Clone)]
enum Ev {
    #[allow(dead_code)]
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

fn peer(name: &str) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-w2b-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = mpsc::channel();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let node = Node::new(
        dir.to_string_lossy().into(),
        Arc::new(Tap(Mutex::new(tx), logs.clone())),
    )
    .unwrap();
    node.create_identity(name.into(), PASS.into()).unwrap();
    online(&node);
    let did = node.profile().unwrap().did;
    Peer {
        node,
        rx,
        logs,
        did,
        dir,
    }
}

/// a and b, each a contact of the other (b scanned a's ticket).
fn pair(tag: &str) -> (Peer, Peer) {
    let a = peer(&format!("{tag}-a"));
    let b = peer(&format!("{tag}-b"));
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    (a, b)
}

/// How the call `id` ended on `p`, and when (bounded by `secs`).
fn ended_at(p: &Peer, id: &str, secs: u64) -> (String, Instant) {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p
            .rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
        {
            Ok(Ev::State(i, CallState::Ended { reason })) if i == id => {
                return (reason, Instant::now());
            }
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for call {id} to end"),
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
        page = pg
            .older_day
            .and_then(|d| p.node.chat_day(did.to_string(), Some(d)).ok());
    }
    out
}

fn delivery(p: &Peer, did: &str, id: &str) -> DeliveryState {
    all_msgs(p, did)
        .into_iter()
        .find(|m| m.id == id)
        .expect("message held")
        .delivery
}

fn contacts_dump(p: &Peer) -> String {
    format!("{:?}", p.node.contacts())
}

/// What the dialling side can tell about a refused-or-absent callee. U5 wants the blocked and
/// the offline value to be equal.
#[derive(Debug, PartialEq)]
struct CallObservation {
    reason: String,
    /// Ended within 15 s. A refused dial ends in well under a second; an offline callee ended
    /// after ~20 s in our runs (the leg logged "timed out"; `DIAL_TIMEOUT` is 30 s, `node.rs:34`).
    fast: bool,
    /// The caller's `on_log` (shown in the app's log view) carries the explicit refusal.
    reject_logged: bool,
}

// ---- tests ----

// KIND: invariant
// The block is not announced: the blocked side's contact list is unchanged, while the blocker
// holds the DID in `blocked`. Blocking does not rotate the blocker's device key (its endpoint
// id); U5: blocking never triggers a rotation. (It does drop the blocker's current ticket,
// `node.rs:991`, which is a ticket reset, not an endpoint change.)
#[test]
fn block_is_not_announced_and_does_not_change_the_endpoint() {
    let (a, b) = pair("quiet");
    let ticket_before = a.node.my_ticket().unwrap();
    let a_dev = a.node.device_key_for_test().unwrap();
    let b_view = contacts_dump(&b);
    assert!(b_view.contains(&a.did));

    a.node.remove_contact(b.did.clone()).unwrap();

    assert!(a.node.blocked_for_test().contains(&b.did));
    assert!(a.node.contacts().is_empty());
    assert_eq!(
        a.node.device_key_for_test().unwrap(),
        a_dev,
        "a block must not rotate the endpoint"
    );
    // Bounded: nothing reaches b within 5 s (nothing is sent at all, node.rs:984-1010).
    std::thread::sleep(Duration::from_secs(5));
    assert_eq!(contacts_dump(&b), b_view, "b must not learn of the block");
    assert!(b.node.blocked_for_test().is_empty());
    // Observed side effect: the blocker's ticket was reset.
    assert_ne!(a.node.my_ticket().unwrap(), ticket_before);
    assert_eq!(a.node.device_key_for_test().unwrap(), a_dev);
}

// KIND: limitation
// TODAY: a blocked caller and a caller of an offline contact both end with "unreachable" and the
// same call record reason, but the blocked caller is refused in about a second with an explicit
// `Reject` (it reaches the blocker's endpoint, so it learns the blocker is online) and its log
// says "rejected: not accepted"; the offline caller ends only after the dial timeout and logs no
// rejection. EXPECTED-CORRECT (U5): the two `CallObservation`s are equal (traffic dropped, no
// Reject frame, the blocked caller times out like an offline one).
#[test]
fn blocked_caller_is_refused_explicitly_unlike_an_offline_callee() {
    let (a, b) = pair("callblk");
    let (c, d) = pair("calloff");
    a.node.remove_contact(b.did.clone()).unwrap();
    c.node.stop();

    // Both calls run concurrently.
    let t0 = Instant::now();
    let off_call = d.node.call(c.did.clone()).unwrap();
    let blk_call = b.node.call(a.did.clone()).unwrap();
    let (blk_reason, blk_at) = ended_at(&b, &blk_call.call_id, 40);
    let (off_reason, off_at) = ended_at(&d, &off_call.call_id, 70);
    let blk_secs = blk_at.duration_since(t0).as_secs_f32();
    let off_secs = off_at.duration_since(t0).as_secs_f32();
    eprintln!(
        "blocked call ended '{blk_reason}' after {blk_secs:.1}s; offline after {off_secs:.1}s"
    );

    let blocked = CallObservation {
        reason: blk_reason,
        fast: blk_secs < 15.0,
        reject_logged: b.logged("rejected: not accepted"),
    };
    let offline = CallObservation {
        reason: off_reason,
        fast: off_secs < 15.0,
        reject_logged: d.logged("rejected: not accepted"),
    };
    // What matches U5 already: the end reason and the record.
    assert_eq!(blocked.reason, "unreachable");
    assert_eq!(offline.reason, "unreachable");
    assert_eq!(
        b.node.recent_calls(10)[0].reason,
        d.node.recent_calls(10)[0].reason
    );
    // The blocker's UI hears nothing; it logs the refusal locally.
    assert!(!saw_incoming(&a), "a blocked call must not ring");
    assert!(a.logged("rejected call"));
    // The limitation: the observations differ in timing and in the explicit refusal.
    assert_eq!(
        blocked,
        CallObservation {
            reason: "unreachable".into(),
            fast: true,
            reject_logged: true
        }
    );
    assert_eq!(
        offline,
        CallObservation {
            reason: "unreachable".into(),
            fast: false,
            reject_logged: false
        }
    );
    assert_ne!(blocked, offline);
}

// KIND: invariant
// At the chat API level the blocked sender already sees what U5 asks for: its message stays
// `Pending`, exactly like a message to an offline contact, and presence never shows the blocker
// online. The blocker holds nothing from it (its conversation with the blocked DID was purged,
// `node.rs:995`). Bounded claim: 12 s. (The network-level difference is the next test.)
#[test]
fn blocked_sender_message_stays_pending_like_an_offline_contact() {
    let (a, b) = pair("chatblk");
    let (c, d) = pair("chatoff");
    a.node.remove_contact(b.did.clone()).unwrap();
    c.node.stop();

    let blk = b
        .node
        .send_text(a.did.clone(), "hello?".into(), None)
        .unwrap();
    let off = d
        .node
        .send_text(c.did.clone(), "hello?".into(), None)
        .unwrap();
    std::thread::sleep(Duration::from_secs(12));

    assert_eq!(delivery(&b, &a.did, &blk.id), DeliveryState::Pending);
    assert_eq!(delivery(&d, &c.did, &off.id), DeliveryState::Pending);
    assert_eq!(
        b.node.contact_online(a.did.clone()),
        d.node.contact_online(c.did.clone())
    );
    assert!(!b.node.contact_online(a.did.clone()));
    assert!(
        !all_msgs(&a, &b.did).iter().any(|m| m.id == blk.id),
        "the blocker must not hold it"
    );
}

// KIND: limitation
// TODAY: the chat path also distinguishes "blocked" from "offline" for anyone who looks below the
// `DeliveryState`: the blocked sender's dial completes a QUIC handshake with the blocker (so the
// blocker is online) and is then closed; `fetch_older_history` (a public API that dials,
// `chat/engine.rs:1779`) fails within seconds, whereas against an offline contact it fails only
// after the dial timeout. EXPECTED-CORRECT (U5): both fail the same way after the same time
// (no accepted connection for a blocked DID's device, or an indistinguishable timeout).
#[test]
fn blocked_history_fetch_fails_fast_unlike_an_offline_contact() {
    let (a, b) = pair("histblk");
    let (c, d) = pair("histoff");
    a.node.remove_contact(b.did.clone()).unwrap();
    c.node.stop();

    let (bn, ad) = (b.node.clone(), a.did.clone());
    let (dn, cd) = (d.node.clone(), c.did.clone());
    let t0 = Instant::now();
    let off = std::thread::spawn(move || {
        let r = dn.fetch_older_history(cd, "2026-01-01".into());
        (r, Instant::now())
    });
    let blk = std::thread::spawn(move || {
        let r = bn.fetch_older_history(ad, "2026-01-01".into());
        (r, Instant::now())
    });
    let (blk_r, blk_at) = blk.join().unwrap();
    let (off_r, off_at) = off.join().unwrap();
    let blk_secs = blk_at.duration_since(t0).as_secs_f32();
    let off_secs = off_at.duration_since(t0).as_secs_f32();
    eprintln!(
        "blocked fetch: {blk_r:?} after {blk_secs:.1}s; offline fetch: {off_r:?} after {off_secs:.1}s"
    );

    let blk_e = blk_r.expect_err("a blocked peer gets no history");
    let off_e = off_r.expect_err("an offline peer gives no history");
    assert!(blk_secs < 15.0, "blocked: refused at once ({blk_secs:.1}s)");
    assert!(
        off_secs >= 15.0,
        "offline: only after the dial timeout ({off_secs:.1}s)"
    );
    // Observed: blocked -> `Net("connection lost")` (the blocker closed an accepted connection);
    // offline -> `Timeout`. Different error kinds as well as different timing.
    assert!(
        matches!(blk_e, Error::Net(_)),
        "blocked: an explicit close, not a timeout: {blk_e:?}"
    );
    assert!(matches!(off_e, Error::Timeout), "offline: {off_e:?}");
}

// KIND: invariant
// Redeeming a fresh ticket of the blocker: refused with the same `Rejected("not accepted")` as a
// spent ticket, so the word "blocked" never reaches the blocked party (`node.rs:2111`). (It still
// shows the blocker is online, as any answered dial does; whether a blocked DID's contact request
// should instead look like no answer is the open W2-Q/G-BLOCK question.) Only scanning the
// blocked party's ticket lifts the block (`node.rs:1983`, `tickets.rs`).
#[test]
fn blocked_ticket_redemption_gets_the_same_refusal_as_a_spent_ticket() {
    let (a, b) = pair("tktblk");
    let c = peer("tktblk-c");
    a.node.remove_contact(b.did.clone()).unwrap();

    let fresh = a.node.my_ticket().unwrap();
    let blocked_err = b.node.add_contact(fresh.clone()).expect_err("blocked");
    // `fresh` was not spent by the refused redemption: c may use it, after which it is spent.
    c.node.add_contact(fresh.clone()).unwrap();
    let spent_err = b
        .node
        .add_contact(a.node.my_ticket().unwrap())
        .map(|_| ())
        .expect_err("still blocked");
    let other_spent = peer("tktblk-e");
    let reused_err = other_spent.node.add_contact(fresh).expect_err("spent");

    let text = |e: &Error| match e {
        Error::Rejected(r) => r.clone(),
        other => panic!("expected Rejected, got {other:?}"),
    };
    assert_eq!(text(&blocked_err), "not accepted");
    assert_eq!(text(&blocked_err), text(&reused_err));
    assert_eq!(text(&spent_err), text(&reused_err));
    assert!(!a.node.contacts().iter().any(|x| x.did == b.did));
    assert!(a.node.blocked_for_test().contains(&b.did));
}
