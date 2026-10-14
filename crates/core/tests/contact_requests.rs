//! Adding by code as a request, between real nodes (needs network, like tickets.rs).
//! `docs/protocol.md` "Contact requests"; design: SIGNUP-HANDOFF.md § 6f core asks 0, 0b and 1.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use p2pcore::{
    AddError, AddOutcome, CallInfo, CallState, CodePeek, Contact, ContactRequest, ContactRequestEvents, Node, NodeEvents, NodeStatus,
};

const PASS: &str = "test-passphrase";

struct Quiet;
impl NodeEvents for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}

#[derive(Default)]
struct Requests {
    asked: Mutex<Vec<ContactRequest>>,
    accepted: Mutex<Vec<Contact>>,
}
impl ContactRequestEvents for Requests {
    fn on_contact_request(&self, request: ContactRequest) {
        self.asked.lock().push(request);
    }
    fn on_request_accepted(&self, contact: Contact) {
        self.accepted.lock().push(contact);
    }
}

struct Peer {
    node: Arc<Node>,
    did: String,
    dir: PathBuf,
    ev: Arc<Requests>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A synthetic account, started and online; contact hellos are held as requests (the default).
fn peer(tag: &str) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-creq-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    let ev = Arc::new(Requests::default());
    node.set_contact_request_events(ev.clone());
    node.create_identity(tag.into(), PASS.into()).unwrap();
    node.start().unwrap();
    let t = Instant::now();
    while !node.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    let did = node.profile().unwrap().did;
    Peer { node, did, dir, ev }
}

fn eventually(secs: u64, what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(secs), "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn has(p: &Peer, did: &str) -> bool {
    p.node.contacts().iter().any(|c| c.did == did)
}

fn requested(r: Result<AddOutcome, AddError>) -> String {
    match r {
        Ok(AddOutcome::Requested { did, .. }) => did,
        other => panic!("expected a request, got {other:?}"),
    }
}

/// 0b: a hello is held as a request; nobody is anybody's contact until Accept, and "Not now"
/// (no answer) sends nothing. Accept: both have each other, with call tokens both ways.
#[test]
fn a_hello_is_a_request_until_accepted() {
    let (tara, maya) = (peer("held-tara"), peer("held-maya"));
    let code = tara.node.my_code().unwrap();
    assert!(code.expires_at > now() + 6 * 24 * 3600, "a week of life");
    assert_eq!(requested(maya.node.add_contact_by_code(format!("Add me: {}", code.code))), tara.did);

    assert!(tara.node.contacts().is_empty(), "held, not stored as a contact");
    assert!(maya.node.contacts().is_empty());
    let asked = tara.node.contact_requests();
    assert_eq!(asked.len(), 1);
    assert_eq!((asked[0].did.as_str(), asked[0].name.as_str()), (maya.did.as_str(), "held-maya"));
    assert_eq!(asked[0].expires_at, code.expires_at, "the request lasts as long as the code it used");
    eventually(5, "the request event", || tara.ev.asked.lock().len() == 1);
    let sent = maya.node.sent_contact_requests();
    assert_eq!((sent.len(), sent[0].did.as_str(), sent[0].name.as_str()), (1, tara.did.as_str(), "held-tara"));

    // Not now: nothing is sent, the request just waits on both sides.
    std::thread::sleep(Duration::from_secs(3));
    assert!(maya.node.contacts().is_empty() && maya.ev.accepted.lock().is_empty());
    assert_eq!(maya.node.sent_contact_requests().len(), 1);
    assert_eq!(tara.node.contact_requests().len(), 1);

    let c = tara.node.accept_contact_request(maya.did.clone()).unwrap();
    assert_eq!(c.did, maya.did);
    assert!(has(&tara, &maya.did));
    assert!(tara.node.contact_requests().is_empty());
    eventually(40, "Maya takes the welcome", || has(&maya, &tara.did));
    eventually(5, "the accepted event", || maya.ev.accepted.lock().len() == 1);
    assert_eq!(maya.ev.accepted.lock()[0].name, "held-tara", "the code's signed name");
    assert!(maya.node.sent_contact_requests().is_empty());
    // Both may call (ticket contacts, `policy_for`).
    assert!(tara.node.calls_allowed(maya.did.clone()) && tara.node.can_call(maya.did.clone()));
    assert!(maya.node.calls_allowed(tara.did.clone()) && maya.node.can_call(tara.did.clone()));
    // Accepting again: there is no request any more.
    assert!(matches!(tara.node.accept_contact_request(maya.did.clone()), Err(p2pcore::Error::NotFound)));
}

/// 0: one code, two joiners, both succeed; the same joiner may use it again until it expires; a repeated
/// hello while waiting stays one request (one event).
#[test]
fn a_code_adds_several_people_once_each() {
    let (tara, maya, ravi) = (peer("two-tara"), peer("two-maya"), peer("two-ravi"));
    let code = tara.node.my_code().unwrap().code;
    requested(maya.node.add_contact_by_code(code.clone()));
    requested(maya.node.add_contact_by_code(code.clone()));
    requested(ravi.node.add_contact_by_code(code.clone()));
    assert_eq!(tara.node.contact_requests().len(), 2, "one request per joiner");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(tara.ev.asked.lock().len(), 2, "a repeated hello raises no second event");
    assert_eq!(tara.node.my_code().unwrap().code, code, "the code is not used up");

    tara.node.accept_contact_request(maya.did.clone()).unwrap();
    tara.node.accept_contact_request(ravi.did.clone()).unwrap();
    eventually(40, "both have Tara", || has(&maya, &tara.did) && has(&ravi, &tara.did));
    assert_eq!(tara.node.contacts().len(), 2);

    // The same joiner and code again (her other device, say): good until the code expires, and as
    // she is a contact already it adds no one new.
    assert!(maya.node.add_contact_by_code(code).is_ok());
    assert_eq!(tara.node.contacts().len(), 2);
}

/// 0: a reset code is refused at once; a rename keeps the old code good until it expires.
#[test]
fn reset_codes_are_refused_and_renamed_ones_live_on() {
    let (tara, maya, ravi) = (peer("reset-tara"), peer("reset-maya"), peer("reset-ravi"));
    let old = tara.node.my_code().unwrap().code;
    tara.node.set_name("Tara I".into()).unwrap();
    let renamed = tara.node.my_code().unwrap().code;
    assert_ne!(old, renamed, "a rename re-signs the code");
    requested(maya.node.add_contact_by_code(old.clone()));
    tara.node.reset_ticket().unwrap();
    assert!(matches!(ravi.node.add_contact_by_code(renamed), Err(AddError::Refused)));
    assert!(matches!(ravi.node.add_contact_by_code(old), Err(AddError::Refused)));
    // A request that arrived before the reset can still be answered.
    assert_eq!(tara.node.contact_requests().len(), 1);
    tara.node.accept_contact_request(maya.did.clone()).unwrap();
    eventually(40, "Maya has Tara", || has(&maya, &tara.did));
}

/// Blocked joiner: refused as before, with the block's reason (typed, no string matching).
#[test]
fn a_blocked_joiner_is_refused() {
    let (tara, maya) = (peer("block-tara"), peer("block-maya"));
    requested(maya.node.add_contact_by_code(tara.node.my_code().unwrap().code));
    tara.node.accept_contact_request(maya.did.clone()).unwrap();
    eventually(40, "Maya has Tara", || has(&maya, &tara.did));
    tara.node.block(maya.did.clone()).unwrap();
    let fresh = tara.node.my_code().unwrap().code;
    assert!(matches!(maya.node.add_contact_by_code(fresh), Err(AddError::Blocked)));
    assert!(tara.node.contact_requests().is_empty(), "never held");
}

/// The Welcome waits for a joiner who went offline, and lands when they are back.
#[test]
fn an_accept_reaches_a_joiner_who_was_offline() {
    let (tara, maya) = (peer("late-tara"), peer("late-maya"));
    requested(maya.node.add_contact_by_code(tara.node.my_code().unwrap().code));
    maya.node.stop();
    tara.node.accept_contact_request(maya.did.clone()).unwrap();
    std::thread::sleep(Duration::from_secs(3));
    maya.node.start().unwrap();
    eventually(90, "Maya takes the retried welcome", || has(&maya, &tara.did));
}

/// 1: `peek_code` says why a code can't be used, without dialling.
#[test]
fn peek_says_why() {
    let tara = peer("peek-tara");
    assert_eq!(tara.node.peek_code("hello there".into()), CodePeek::NotACode);
    assert_eq!(tara.node.peek_code(tara.node.my_ticket().unwrap()), CodePeek::Own);
    let (other, _) = identity::generate();
    let dev = proto::device_public(&proto::new_device_secret());
    let gone = proto::issue_contact_ticket(&other, dev, "Old Code", None, now() - 7200, 3600).to_text();
    match tara.node.peek_code(format!("here: {gone}")) {
        CodePeek::Expired { name, did, .. } => assert_eq!((name.as_str(), did.as_str()), ("Old Code", other.did())),
        x => panic!("expected expired, got {x:?}"),
    }
    assert!(matches!(tara.node.add_contact_by_code(gone), Err(AddError::Expired)));
    let live = proto::issue_contact_ticket(&other, dev, "Live Code", None, now(), 3600).to_text();
    match tara.node.peek_code(live) {
        CodePeek::Found { card } => assert_eq!((card.name.as_str(), card.known), ("Live Code", false)),
        x => panic!("expected found, got {x:?}"),
    }
    assert!(matches!(tara.node.add_contact_by_code(tara.node.my_ticket().unwrap()), Err(AddError::OwnCode)));
    assert!(matches!(tara.node.add_contact_by_code("nothing".into()), Err(AddError::NotACode)));
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}
