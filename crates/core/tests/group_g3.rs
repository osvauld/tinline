//! G3: inviting contacts directly, and short invite codes, on real nodes over real iroh endpoints
//! (needs network, like `group_core.rs`, whose helpers are copied here). Protocol and choices:
//! `docs/group-core.md` (G3 sections).
//!
//! Every test carries `// KIND: acceptance (G3 stage N)` and what it enforces.
#![allow(dead_code)]

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{
    CallInfo, CallState, GroupInvitation, GroupInvitationEvents, GroupInvitationState, GroupItemKind, GroupMessage, LinkEvents, Node, NodeEvents,
    NodeStatus,
};

const PASS: &str = "test-passphrase";
const WAIT: u64 = 90;

struct Quiet;
impl NodeEvents for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
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

struct ITap(Mutex<Vec<GroupInvitation>>);
impl GroupInvitationEvents for ITap {
    fn on_group_invitation(&self, invitation: GroupInvitation) {
        self.0.lock().unwrap().push(invitation);
    }
}

struct Peer {
    node: Arc<Node>,
    lrx: mpsc::Receiver<LEv>,
    inv: Arc<ITap>,
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
    let dir = std::env::temp_dir().join(format!("p2pcore-g3-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (ltx, lrx) = mpsc::channel();
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    node.set_link_events(Arc::new(LTap(Mutex::new(ltx))));
    let inv = Arc::new(ITap(Mutex::new(Vec::new())));
    node.set_group_invitation_events(inv.clone());
    Peer { node, lrx, inv, did: String::new(), dir }
}

fn peer(name: &str) -> Peer {
    let mut p = blank(name);
    p.node.create_identity(name.into(), PASS.into()).unwrap();
    online(&p.node);
    p.did = p.node.profile().unwrap().did;
    p
}

/// `e` links a fresh install (same DID); returns it committed and online.
fn link_fresh(e: &Peer, name: &str) -> Peer {
    let mut n = blank(name);
    let qr = e.node.link_show_qr().unwrap();
    n.node.link_new_scan(qr, "Desk".into()).unwrap();
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

/// `group_join`, unwrapped. GL: the product rides out transient network failures itself (the
/// fetch retries within its deadline, the join re-dials every 2 s while it waits), so the test-side
/// retry GF added is gone: any error here is a real failure.
fn join(p: &Peer, link: String) -> String {
    p.node.group_join(link).unwrap_or_else(|e| panic!("join: {e:?}"))
}

fn eventually(secs: u64, what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    panic!("timed out waiting for {what}");
}

fn members(p: &Peer, g: &str) -> Vec<String> {
    let mut m: Vec<String> = p.node.group_detail(g.into()).unwrap().members.into_iter().map(|m| m.did).collect();
    m.sort();
    m
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

fn contacts(a: &Peer, b: &Peer) {
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    eventually(30, "both are contacts", || a.node.contacts().iter().any(|c| c.did == b.did) && b.node.contacts().iter().any(|c| c.did == a.did));
}

fn incoming(p: &Peer, g: &str) -> Option<GroupInvitation> {
    p.node.group_invitations_incoming().unwrap().into_iter().find(|i| i.group_id == g)
}

fn outgoing_state(p: &Peer, g: &str, did: &str) -> Option<GroupInvitationState> {
    p.node.group_invitations_outgoing(g.into()).unwrap().into_iter().find(|i| i.invitee_did == did).map(|i| i.state)
}

fn posts(p: &Peer, g: &str) -> Vec<GroupMessage> {
    p.node.group_messages(g.into(), None, 200).unwrap().messages.into_iter().filter(|m| m.kind == GroupItemKind::Post).collect()
}

// ---- stage 1: inviting contacts directly ----------------------------------------------------------

// KIND: acceptance (G3 stage 1) — an admin invites two contacts; the invitation travels over the
// 1:1 private channel; each sees "o invited you to G" with Join / Decline; one joins (same
// verification as a link), one declines; the inviter sees Joined / Declined per invitee.
#[test]
fn contacts_invited_directly_join_or_decline() {
    let o = peer("dj-o");
    let a = peer("dj-a");
    let b = peer("dj-b");
    contacts(&o, &a);
    contacts(&o, &b);
    let g = o.node.group_create("Book club".into()).unwrap();
    let sent = o.node.group_invite_contacts(g.clone(), vec![a.did.clone(), b.did.clone()]).unwrap();
    assert_eq!(sent.len(), 2);
    assert!(sent.iter().all(|i| i.outgoing && i.state == GroupInvitationState::Pending && i.group_name == "Book club"));

    for p in [&a, &b] {
        eventually(WAIT, "the invitation arrives", || incoming(p, &g).is_some());
        let i = incoming(p, &g).unwrap();
        assert_eq!(i.inviter_did, o.did);
        assert_eq!(i.inviter_name, "dj-o");
        assert_eq!(i.group_name, "Book club");
        assert_eq!(i.state, GroupInvitationState::Pending);
        assert!(!i.outgoing);
        assert!(i.expires > 0);
        assert!(p.inv.0.lock().unwrap().iter().any(|e| e.group_id == g), "an event was raised");
    }
    let ia = incoming(&a, &g).unwrap();
    let joined = a.node.group_invitation_accept(ia.id.clone()).unwrap();
    assert_eq!(joined.state, GroupInvitationState::Joined);
    assert_eq!(incoming(&a, &g).unwrap().state, GroupInvitationState::Joined);
    eventually(WAIT, "a is a member everywhere", || members(&o, &g) == sorted(vec![o.did.clone(), a.did.clone()]) && members(&a, &g) == members(&o, &g));

    let ib = incoming(&b, &g).unwrap();
    assert_eq!(b.node.group_invitation_decline(ib.id.clone()).unwrap().state, GroupInvitationState::Declined);
    assert!(b.node.group_invitation_accept(ib.id).is_err(), "already answered");
    assert!(b.node.groups().unwrap().is_empty());

    eventually(WAIT, "o sees Joined and Declined", || {
        outgoing_state(&o, &g, &a.did) == Some(GroupInvitationState::Joined) && outgoing_state(&o, &g, &b.did) == Some(GroupInvitationState::Declined)
    });
    // Existing members are not invited again.
    assert!(o.node.group_invite_contacts(g.clone(), vec![a.did.clone()]).is_err());
}

// KIND: acceptance (G3 stage 1) — an offline contact gets the invitation when it is next online
// (outbox with retry, like the device-list push).
#[test]
fn an_offline_contact_gets_the_invitation_when_back() {
    let o = peer("off-o");
    let a = peer("off-a");
    contacts(&o, &a);
    let g = o.node.group_create("Later".into()).unwrap();
    a.node.stop();
    std::thread::sleep(Duration::from_secs(2));
    o.node.group_invite_contacts(g.clone(), vec![a.did.clone()]).unwrap();
    std::thread::sleep(Duration::from_secs(5));
    assert_eq!(outgoing_state(&o, &g, &a.did), Some(GroupInvitationState::Pending));
    online(&a.node);
    eventually(WAIT, "the invitation arrives after a comes back", || incoming(&a, &g).is_some());
    let id = incoming(&a, &g).unwrap().id;
    a.node.group_invitation_accept(id).unwrap();
    eventually(WAIT, "o sees Joined", || outgoing_state(&o, &g, &a.did) == Some(GroupInvitationState::Joined));
}

// KIND: acceptance (G3 stage 1, P5) — blocked contacts, either direction, cannot be invited; a
// non-contact cannot be invited either; non-admins cannot invite.
#[test]
fn blocked_contacts_and_strangers_cannot_be_invited() {
    let o = peer("bl-o");
    let a = peer("bl-a");
    let b = peer("bl-b");
    let x = peer("bl-x");
    contacts(&o, &a);
    contacts(&o, &b);
    let g = o.node.group_create("Quiet".into()).unwrap();
    o.node.block(a.did.clone()).unwrap();
    assert!(o.node.group_invite_contacts(g.clone(), vec![a.did.clone()]).is_err(), "we block them");
    b.node.block(o.did.clone()).unwrap();
    eventually(WAIT, "o learns b blocks it", || o.node.block_state(b.did.clone()).blocked_me.is_some());
    assert!(o.node.group_invite_contacts(g.clone(), vec![b.did.clone()]).is_err(), "they block us");
    assert!(o.node.group_invite_contacts(g.clone(), vec![x.did.clone()]).is_err(), "not a contact");
    std::thread::sleep(Duration::from_secs(3));
    assert!(a.node.group_invitations_incoming().unwrap().is_empty());
    assert!(b.node.group_invitations_incoming().unwrap().is_empty());
}

// KIND: acceptance (G3 stage 1) — the invitee's other device shows the invitation too, and an
// answer on one device shows on the other (own-device sync over self/3).
#[test]
fn invitations_sync_to_own_devices() {
    let o = peer("sy-o");
    let a = peer("sy-a");
    contacts(&o, &a);
    let a2 = link_fresh(&a, "sy-a2");
    let g = o.node.group_create("Both".into()).unwrap();
    o.node.group_invite_contacts(g.clone(), vec![a.did.clone()]).unwrap();
    eventually(WAIT, "both devices list it", || incoming(&a, &g).is_some() && incoming(&a2, &g).is_some());
    let id = incoming(&a2, &g).unwrap().id;
    a2.node.group_invitation_decline(id).unwrap();
    eventually(WAIT, "the other device shows Declined", || incoming(&a, &g).is_some_and(|i| i.state == GroupInvitationState::Declined));
    eventually(WAIT, "o sees Declined", || outgoing_state(&o, &g, &a.did) == Some(GroupInvitationState::Declined));
}

// ---- stage 2: short invite codes --------------------------------------------------------------------

// KIND: acceptance (G3 stage 2) — the invite link is short (<= 90 chars, `tinline://g/...`), peek
// fetches the details from the host, and joining works with the link and with the bare code.
#[test]
fn short_invite_links_peek_and_join() {
    let o = peer("sc-o");
    let a = peer("sc-a");
    let b = peer("sc-b");
    let g = o.node.group_create("Short".into()).unwrap();
    let link = o.node.group_invite(g.clone()).unwrap();
    eprintln!("invite link ({} chars): {link}", link.len());
    assert!(link.starts_with("tinline://g/"), "{link}");
    assert!(link.len() <= 90, "{} chars: {link}", link.len());
    let peek = a.node.group_peek_invite(link.clone()).expect("peek fetches from the host");
    assert_eq!(peek.group_id, g);
    assert_eq!(peek.name, "Short");
    assert_eq!(peek.owner_did, o.did);
    assert_eq!(peek.issuer_did.as_deref(), Some(o.did.as_str()));
    assert!(peek.expires.is_some_and(|e| e > 0));
    assert_eq!(join(&a, link.clone()), g);
    let bare = link.strip_prefix("tinline://g/").unwrap().to_string();
    assert_eq!(join(&b, bare), g);
    eventually(WAIT, "both joined", || members(&o, &g).len() == 3);
    // The old long format is gone.
    assert!(a.node.group_peek_invite("tlgroup1:AAAA".into()).is_none());
}

// KIND: acceptance (G3 stage 2) — an admin that is not a host registers its code at a host; the
// link names that host and works; the device token is short too.
#[test]
fn a_non_host_admins_link_registers_at_a_host() {
    let o = peer("nh-o");
    let a = peer("nh-a");
    let c = peer("nh-c");
    let g = o.node.group_create("Admins".into()).unwrap();
    join(&a, o.node.group_invite(g.clone()).unwrap());
    eventually(WAIT, "a is a member", || members(&o, &g).contains(&a.did));
    o.node.group_set_admin(g.clone(), a.did.clone(), true).unwrap();
    eventually(WAIT, "a knows it is admin", || a.node.group_detail(g.clone()).unwrap().members.iter().any(|m| m.did == a.did && m.is_admin));
    let link = a.node.group_invite(g.clone()).unwrap();
    assert!(link.len() <= 90, "{link}");
    assert_eq!(c.node.group_peek_invite(link.clone()).unwrap().issuer_did.as_deref(), Some(a.did.as_str()));
    assert_eq!(join(&c, link), g);
    let tok = a.node.group_device_token(g.clone()).unwrap();
    assert!(tok.starts_with("tinline://g/") && tok.len() <= 90, "{tok}");
    let peek = c.node.group_peek_invite(tok).unwrap();
    assert!(peek.issuer_did.is_none());
}

// KIND: acceptance (G3 stage 2) — hosts refuse unknown and revoked codes with a clear reason, and
// close a connection that guesses too many codes.
#[test]
fn hosts_refuse_unknown_and_revoked_codes_and_limit_guessing() {
    let o = peer("rv-o");
    let a = peer("rv-a");
    let g = o.node.group_create("Revoked".into()).unwrap();
    let link = o.node.group_invite(g.clone()).unwrap();
    o.node.group_revoke_invite(link.clone()).unwrap();
    match a.node.group_join(link.clone()) {
        Err(p2pcore::Error::Rejected(r)) => assert!(r.contains("unknown or expired"), "{r}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(a.node.group_peek_invite(link.clone()).is_none());
    assert!(a.node.groups().unwrap().is_empty());
    // Guessing: one connection, many wrong codes: refused, then closed.
    let answers = a.node.group_guess_codes_for_test(link, 10);
    eprintln!("{answers:?}");
    assert!(answers.len() < 10, "the host closed the connection: {answers:?}");
    assert!(answers.iter().all(|r| r.contains("unknown or expired")), "{answers:?}");
    // A fresh link still works.
    assert_eq!(join(&a, o.node.group_invite(g.clone()).unwrap()), g);
    o.node.group_post(g.clone(), "welcome".into(), None).unwrap();
    eventually(WAIT, "a sees it", || posts(&a, &g).iter().any(|m| m.text == "welcome"));
}
