//! G2: group chat core, second slice, on real nodes over real iroh endpoints (needs network,
//! like `group_core.rs`, whose helpers are copied here). Decisions: `rotation-open-decisions.md`
//! §0 on `endpoint-rotation-blocking` (U3 chat requests through the host, U12 host rotation after
//! a removal, U16 writer IDs) and P10 (writer counters never reused). Protocol: `docs/group-core.md`.
//!
//! Every test carries `// KIND: acceptance (G2 stage N)` and the decision it enforces.
#![allow(dead_code)]

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, GroupDelivery, GroupItemKind, GroupMessage, GroupRole, LinkEvents, Node, NodeEvents, NodeStatus};

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

struct Peer {
    node: Arc<Node>,
    lrx: mpsc::Receiver<LEv>,
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
    let dir = std::env::temp_dir().join(format!("p2pcore-g2-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (ltx, lrx) = mpsc::channel();
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    node.set_link_events(Arc::new(LTap(Mutex::new(ltx))));
    Peer { node, lrx, did: String::new(), dir }
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

fn all(p: &Peer, g: &str) -> Vec<GroupMessage> {
    let mut out = Vec::new();
    let mut before = None;
    loop {
        let page = p.node.group_messages(g.into(), before, 50).unwrap();
        let mut ms = page.messages;
        ms.reverse();
        out.extend(ms);
        match page.older {
            Some(o) => before = Some(o),
            None => break,
        }
    }
    out.reverse();
    out
}

fn posts(p: &Peer, g: &str) -> Vec<(String, String)> {
    all(p, g).into_iter().filter(|m| m.kind == GroupItemKind::Post).map(|m| (m.author_did, m.text)).collect()
}

fn has_post(p: &Peer, g: &str, did: &str, text: &str) -> bool {
    posts(p, g).iter().any(|(d, t)| d == did && t == text)
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

/// Owner `o` (first host) creates a group and each of `ms` joins with an invite from `o`.
fn group_with(o: &Peer, ms: &[&Peer]) -> String {
    let g = o.node.group_create("Weekend hike".into()).unwrap();
    for m in ms {
        let tok = o.node.group_invite(g.clone()).unwrap();
        assert_eq!(m.node.group_join(tok).unwrap(), g);
    }
    let want = sorted(std::iter::once(o.did.clone()).chain(ms.iter().map(|m| m.did.clone())).collect());
    for p in std::iter::once(o).chain(ms.iter().copied()) {
        eventually(WAIT, "everyone sees every member", || members(p, &g) == want);
    }
    g
}


/// Every file named `name` under `dir`.
fn find_files(dir: &std::path::Path, name: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(find_files(&p, name));
        } else if p.file_name().is_some_and(|n| n == name) {
            out.push(p);
        }
    }
    out
}

/// Restarts `p`'s process (a new `Node` over the same directory); with `lose_groups`, the group
/// store (`group.redb`) is gone when it comes back (lost, deleted, or a backup without it).
fn restart(p: &mut Peer, lose_groups: bool) {
    p.node.stop();
    std::thread::sleep(Duration::from_secs(2));
    let node = Node::new(p.dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    let (ltx, lrx) = mpsc::channel();
    node.set_link_events(Arc::new(LTap(Mutex::new(ltx))));
    p.lrx = lrx;
    let old = std::mem::replace(&mut p.node, node);
    drop(old);
    if lose_groups {
        let files = find_files(&p.dir, "group.redb");
        assert!(!files.is_empty(), "there was a group store");
        for f in files {
            std::fs::remove_file(f).unwrap();
        }
    }
    p.node.unlock(PASS.into()).unwrap();
    online(&p.node);
}

fn delivered(p: &Peer, g: &str, text: &str) -> bool {
    all(p, g).iter().any(|x| x.text == text && x.delivery == GroupDelivery::Delivered)
}

// ---- stage 1: writer counters after a lost group store (P10) ---------------------------------------

// KIND: acceptance (G2 stage 1) — P10 for groups: a post written offline that never reached a
// host is lost with the store; after the device rejoins it writes as a fresh writer (new group
// device ID), so no (writer ID, counter) is reused and its new post arrives everywhere.
#[test]
fn a_lost_group_store_gives_a_fresh_writer_and_new_posts_arrive() {
    let o = peer("ls-o");
    let mut a = peer("ls-a");
    let b = peer("ls-b");
    let g = group_with(&o, &[&a, &b]);
    a.node.group_post(g.clone(), "before".into(), None).unwrap();
    eventually(WAIT, "before is delivered", || delivered(&a, &g, "before"));
    let dev_before = a.node.group_device_id().unwrap();
    a.node.stop();
    std::thread::sleep(Duration::from_secs(1));
    let m = a.node.group_post(g.clone(), "never left this device".into(), None).unwrap();
    assert_eq!(m.delivery, GroupDelivery::Waiting);
    restart(&mut a, true);
    assert!(a.node.groups().unwrap().is_empty(), "the group went with the store");
    let dev_after = a.node.group_device_id().unwrap();
    assert_ne!(dev_after, dev_before, "a fresh group store writes under a fresh device ID");
    // Rejoin (a is still a member: any admin's invite, or a device token, lets it back in).
    let tok = o.node.group_invite(g.clone()).unwrap();
    assert_eq!(a.node.group_join(tok).unwrap(), g);
    eventually(WAIT, "a sees history again", || has_post(&a, &g, &a.did, "before"));
    a.node.group_post(g.clone(), "after the loss".into(), None).unwrap();
    for p in [&o, &b] {
        eventually(WAIT, "the new post arrives", || has_post(p, &g, &a.did, "after the loss"));
        assert!(has_post(p, &g, &a.did, "before"));
    }
    eventually(WAIT, "delivered", || delivered(&a, &g, "after the loss"));
    let det = o.node.group_detail(g.clone()).unwrap();
    let ma = det.members.iter().find(|x| x.did == a.did).unwrap();
    assert!(ma.device_ids.contains(&dev_before) && ma.device_ids.contains(&dev_after));
}

// KIND: acceptance (G2 stage 1) — P10 for groups, the harmful case: the post reached only a
// second host that is offline when the device loses its store and rejoins through the first
// host. Reusing the writer ID would put the new post at the same (writer, counter) as the one
// held by the offline host, and one of the two would be dropped silently when the hosts meet.
#[test]
fn a_lost_group_store_never_reuses_a_counter_held_by_an_offline_host() {
    let o = peer("lh-o");
    let h = peer("lh-h");
    let mut a = peer("lh-a");
    let b = peer("lh-b");
    let g = group_with(&o, &[&h, &a, &b]);
    o.node.group_appoint_host(g.clone(), h.did.clone(), h.node.group_device_id().unwrap()).unwrap();
    eventually(WAIT, "h sees the permit", || all(&h, &g).iter().any(|x| x.kind == GroupItemKind::HostAdded && x.subject_did == h.did));
    h.node.group_accept_host(g.clone(), true).unwrap();
    for p in [&o, &h, &a, &b] {
        eventually(WAIT, "two hosts everywhere", || p.node.group_detail(g.clone()).unwrap().hosts.len() == 2);
    }
    eventually(WAIT, "a knows h's group endpoint", || {
        let he = h.node.group_endpoint_id().unwrap();
        a.node.group_known_endpoints_for_test(g.clone()).contains(&he)
    });
    // Only h is up (b is down too: members relay); a's post reaches h alone.
    o.node.stop();
    b.node.stop();
    std::thread::sleep(Duration::from_secs(2));
    a.node.group_post(g.clone(), "only h has this".into(), None).unwrap();
    eventually(WAIT, "h acknowledged it", || delivered(&a, &g, "only h has this"));
    assert!(has_post(&h, &g, &a.did, "only h has this"));
    // h goes away holding it; o comes back; a loses its group store and rejoins through o.
    h.node.stop();
    online(&o.node);
    restart(&mut a, true);
    let tok = o.node.group_invite(g.clone()).unwrap();
    assert_eq!(a.node.group_join(tok).unwrap(), g);
    assert!(!has_post(&o, &g, &a.did, "only h has this"), "o never got it");
    a.node.group_post(g.clone(), "written after the loss".into(), None).unwrap();
    eventually(WAIT, "o has the new post", || has_post(&o, &g, &a.did, "written after the loss"));
    // h and b come back; the hosts sync again.
    online(&h.node);
    online(&b.node);
    for p in [&o, &h, &b, &a] {
        eventually(WAIT, "both posts everywhere", || {
            has_post(p, &g, &a.did, "only h has this") && has_post(p, &g, &a.did, "written after the loss")
        });
    }
}

// KIND: regression (GF, flake root cause) — a new host's signed address must not be lost when it
// arrives before the records that make it a host. Here h accepts while the first host o is away,
// so h's address first reaches o in the Hello of h's next session, before o has h's acceptance;
// o used to drop it and nobody ever re-sent it, so members never learned h's endpoint (the
// `a knows h's group endpoint` timeout above, under load, when h's session to o was reconnecting).
#[test]
fn a_host_that_accepts_while_the_first_host_is_away_still_reaches_members() {
    let o = peer("ha-o");
    let h = peer("ha-h");
    let a = peer("ha-a");
    let g = group_with(&o, &[&h, &a]);
    o.node.group_appoint_host(g.clone(), h.did.clone(), h.node.group_device_id().unwrap()).unwrap();
    eventually(WAIT, "h is asked to host", || h.node.group_detail(g.clone()).unwrap().host_requested_here);
    o.node.stop();
    h.node.group_accept_host(g.clone(), true).unwrap();
    assert!(h.node.group_detail(g.clone()).unwrap().am_host);
    online(&o.node);
    for p in [&o, &a] {
        eventually(WAIT, "two hosts everywhere", || p.node.group_detail(g.clone()).unwrap().hosts.len() == 2);
    }
    let he = h.node.group_endpoint_id().unwrap();
    eventually(WAIT, "o knows h's group endpoint", || o.node.group_known_endpoints_for_test(g.clone()).contains(&he));
    eventually(WAIT, "a knows h's group endpoint", || a.node.group_known_endpoints_for_test(g.clone()).contains(&he));
}

// ---- stage 2: own-device sync of the group list ---------------------------------------------------

fn lists(p: &Peer, g: &str) -> bool {
    p.node.groups().unwrap().iter().any(|s| s.id == g)
}

// KIND: acceptance (G2 stage 2) — a linked device learns the account's groups through own-device
// sync (no `group_device_token`): groups joined before and after linking, and a group created on
// the new device reaches the old one. Contents come from the hosts (U7), verified (U10).
#[test]
fn a_linked_device_learns_the_accounts_groups_without_a_token() {
    let o = peer("os-o");
    let a = peer("os-a");
    let g = group_with(&o, &[&a]);
    a.node.group_post(g.clone(), "before linking".into(), None).unwrap();
    eventually(WAIT, "o has it", || has_post(&o, &g, &a.did, "before linking"));

    let a2 = link_fresh(&a, "os-a2");
    eventually(WAIT, "a2 lists the group", || lists(&a2, &g));
    eventually(WAIT, "a2 sees history", || has_post(&a2, &g, &a.did, "before linking"));
    assert_eq!(a2.node.group_detail(g.clone()).unwrap().my_role, GroupRole::Member);
    a2.node.group_post(g.clone(), "from a2".into(), None).unwrap();
    eventually(WAIT, "o sees a2's post", || has_post(&o, &g, &a.did, "from a2"));

    // A group a joins after linking.
    let g2 = o.node.group_create("Second".into()).unwrap();
    a.node.group_join(o.node.group_invite(g2.clone()).unwrap()).unwrap();
    eventually(WAIT, "a2 lists the second group", || lists(&a2, &g2));
    eventually(WAIT, "a2 is a member there", || a2.node.group_detail(g2.clone()).map(|d| d.my_role == GroupRole::Member).unwrap_or(false));

    // A group created on a2 (a2 is its host) reaches a.
    let g3 = a2.node.group_create("Made on a2".into()).unwrap();
    eventually(WAIT, "a lists a2's group", || lists(&a, &g3));
    eventually(WAIT, "a is its owner too", || a.node.group_detail(g3.clone()).map(|d| d.my_role == GroupRole::Owner).unwrap_or(false));
    a.node.group_post(g3.clone(), "hello from a".into(), None).unwrap();
    eventually(WAIT, "a2 (the host) has it", || has_post(&a2, &g3, &a.did, "hello from a"));
    // The device token still works.
    assert!(a.node.group_device_token(g.clone()).is_ok());
}


// ---- stage 3: 1:1 chat requests through the host (U3) ---------------------------------------------

fn incoming(p: &Peer) -> Vec<p2pcore::GroupChatRequest> {
    p.node.group_chat_requests_incoming().unwrap()
}

fn outgoing(p: &Peer) -> Vec<p2pcore::GroupChatRequest> {
    p.node.group_chat_requests_outgoing().unwrap()
}

fn is_contact(p: &Peer, did: &str) -> bool {
    p.node.contacts().iter().any(|c| c.did == did)
}

/// Asks `did` for a chat once the request can be sealed to them: a member's encryption key is a
/// separate record their app writes after it sees itself as a member, so it can trail the
/// membership the test waited for (the API refuses with this transient reason until then).
fn request_chat(p: &Peer, g: &str, did: &str) -> p2pcore::GroupChatRequest {
    let mut out = None;
    eventually(WAIT, "their key is published in the group", || match p.node.group_chat_request(g.into(), did.into()) {
        Ok(r) => {
            out = Some(r);
            true
        }
        Err(p2pcore::Error::Rejected(why)) if why.contains("not published a key") => false,
        Err(e) => panic!("chat request: {e:?}"),
    });
    out.unwrap()
}

fn chat_has(p: &Peer, peer_did: &str, text: &str) -> bool {
    p.node.chat_day(peer_did.into(), None).map(|d| d.messages.iter().any(|m| m.text == text)).unwrap_or(false)
}

// KIND: acceptance (G2 stage 3) — U3 + P6: a member asks another for a 1:1 chat through the
// host; the host relays mail it cannot read and never sees the recipient's private endpoint; on
// accept both become message-only contacts (no calls) and chat directly.
#[test]
fn a_chat_request_accepted_makes_message_only_contacts_that_chat_directly() {
    let o = peer("cr-o");
    let a = peer("cr-a");
    let b = peer("cr-b");
    let g = group_with(&o, &[&a, &b]);
    let r = request_chat(&a, &g, &b.did);
    assert!(r.outgoing && r.state == p2pcore::ChatRequestState::Pending && r.peer_did == b.did);
    // Asking again returns the pending request.
    assert_eq!(a.node.group_chat_request(g.clone(), b.did.clone()).unwrap().id, r.id);
    eventually(WAIT, "b sees the request", || incoming(&b).iter().any(|x| x.id == r.id && x.peer_did == a.did));
    let x = incoming(&b).into_iter().find(|x| x.id == r.id).unwrap();
    assert_eq!(x.peer_name, "cr-a");
    assert_eq!(x.group_id, g);
    assert!(!is_contact(&a, &b.did) && !is_contact(&b, &a.did), "nothing before accepting");
    b.node.group_chat_accept(r.id.clone()).unwrap();
    eventually(WAIT, "a's request is accepted and the contact added", || {
        outgoing(&a).iter().any(|x| x.id == r.id && x.state == p2pcore::ChatRequestState::Accepted && x.contact_added)
    });
    eventually(WAIT, "contacts both ways", || is_contact(&a, &b.did) && is_contact(&b, &a.did));
    assert!(!a.node.calls_allowed(b.did.clone()) && !b.node.calls_allowed(a.did.clone()), "P6: message only");
    assert!(!a.node.can_call(b.did.clone()) && !b.node.can_call(a.did.clone()));
    a.node.send_text(b.did.clone(), "hi, directly".into(), None).unwrap();
    eventually(WAIT, "b gets the direct message", || chat_has(&b, &a.did, "hi, directly"));
    b.node.send_text(a.did.clone(), "hi back".into(), None).unwrap();
    eventually(WAIT, "a gets the reply", || chat_has(&a, &b.did, "hi back"));
    // The host relayed sealed mail and never held b's (or a's) private endpoint.
    assert!(o.node.group_relayed_mail_count_for_test(g.clone()) >= 2);
    for p in [&a, &b] {
        assert!(!o.node.group_relayed_mail_contains_for_test(g.clone(), p.node.status().endpoint_id));
    }
    assert!(!is_contact(&o, &a.did) && !is_contact(&o, &b.did));
}

// KIND: acceptance (G2 stage 3) — a declined request shows "declined" to the requester
// (PROVISIONAL), and nobody becomes a contact. A request from a DID the recipient blocks is
// refused with the blocked reason and never shown to the recipient (P5), even from a modified
// app that skips its own checks.
#[test]
fn chat_requests_can_be_declined_and_blocked_senders_are_refused() {
    let o = peer("cd-o");
    let a = peer("cd-a");
    let b = peer("cd-b");
    let c = peer("cd-c");
    let g = group_with(&o, &[&a, &b, &c]);
    let r = request_chat(&a, &g, &b.did);
    eventually(WAIT, "b sees it", || incoming(&b).iter().any(|x| x.id == r.id));
    b.node.group_chat_decline(r.id.clone()).unwrap();
    eventually(WAIT, "a sees declined", || outgoing(&a).iter().any(|x| x.id == r.id && x.state == p2pcore::ChatRequestState::Declined));
    assert!(!is_contact(&a, &b.did) && !is_contact(&b, &a.did));
    assert!(b.node.group_chat_accept(r.id.clone()).is_err(), "already answered");

    // b knows c from a ticket and blocks c; c's app is modified (ignores the notice, skips checks).
    c.node.ignore_block_notices_for_test(true);
    c.node.add_contact(b.node.my_ticket().unwrap()).unwrap();
    eventually(WAIT, "b has c", || is_contact(&b, &c.did));
    b.node.block(c.did.clone()).unwrap();
    c.node.group_skip_request_checks_for_test(true);
    let rc = request_chat(&c, &g, &b.did);
    eventually(WAIT, "c sees blocked", || outgoing(&c).iter().any(|x| x.id == rc.id && x.state == p2pcore::ChatRequestState::Blocked));
    assert!(!incoming(&b).iter().any(|x| x.peer_did == c.did), "never shown to b");
    // The shipped app refuses locally: we do not ask someone we block.
    assert!(b.node.group_chat_request(g.clone(), c.did.clone()).is_err());
}

// ---- stage 4: host endpoint rotation after a removal / host revocation (U12) ---------------------

// KIND: acceptance (G2 stage 4) — U12: after a removal, the host rotates its group endpoint once
// the delay (2 days; shortened here) has passed; remaining members, including one offline at the
// time, are reconnected to the new endpoint by the host; the removed member never learns it.
#[test]
fn the_host_rotates_after_a_removal_and_only_remaining_members_follow() {
    let o = peer("ro-o");
    let a = peer("ro-a");
    let m = peer("ro-m");
    let b = peer("ro-b");
    let c = peer("ro-c");
    let g = group_with(&o, &[&a, &m, &b, &c]);
    o.node.group_set_admin(g.clone(), a.did.clone(), true).unwrap();
    eventually(WAIT, "a is admin", || a.node.group_detail(g.clone()).unwrap().my_role == GroupRole::Admin);
    o.node.group_set_rotation_delay_for_test(25);
    let old = o.node.group_endpoint_id().unwrap();
    // c is offline across the rotation.
    c.node.stop();
    for _ in 0..3 {
        a.node.group_flag(g.clone(), m.did.clone()).unwrap();
    }
    eventually(WAIT, "m removed at the host", || !members(&o, &g).contains(&m.did));
    // Not immediate.
    assert_eq!(o.node.group_endpoint_id().unwrap(), old, "the rotation waits for the delay");
    eventually(WAIT, "m knows it was removed", || m.node.group_detail(g.clone()).unwrap().my_role == GroupRole::Removed);
    eventually(WAIT + 25, "the host rotated", || o.node.group_endpoint_id().is_some_and(|e| e != old));
    let new = o.node.group_endpoint_id().unwrap();
    for p in [&a, &b] {
        eventually(WAIT, "remaining members learn the new endpoint", || p.node.group_known_endpoints_for_test(g.clone()) == vec![new.clone()]);
    }
    b.node.group_post(g.clone(), "after the rotation".into(), None).unwrap();
    eventually(WAIT, "a gets it through the new endpoint", || has_post(&a, &g, &b.did, "after the rotation"));
    online(&c.node);
    eventually(WAIT, "the offline member is reconnected", || has_post(&c, &g, &b.did, "after the rotation"));
    assert_eq!(c.node.group_known_endpoints_for_test(g.clone()), vec![new.clone()]);
    std::thread::sleep(Duration::from_secs(3));
    assert!(!m.node.group_known_endpoints_for_test(g.clone()).contains(&new), "the removed member never learns it");
    // One rotation per removal: it stays put.
    std::thread::sleep(Duration::from_secs(10));
    assert_eq!(o.node.group_endpoint_id().unwrap(), new);
}

// KIND: acceptance (G2 stage 4) — §10 Q8: revoking a host makes the remaining hosts rotate the
// same way; members follow.
#[test]
fn a_host_revocation_rotates_the_remaining_hosts() {
    let o = peer("rv-o");
    let h = peer("rv-h");
    let a = peer("rv-a");
    let g = group_with(&o, &[&h, &a]);
    assert!(!h.node.group_detail(g.clone()).unwrap().host_requested_here);
    o.node.group_appoint_host(g.clone(), h.did.clone(), h.node.group_device_id().unwrap()).unwrap();
    // G-UI: the appointed device is asked (and only it).
    eventually(WAIT, "h is asked to host", || h.node.group_detail(g.clone()).unwrap().host_requested_here);
    assert!(!a.node.group_detail(g.clone()).unwrap().host_requested_here);
    let req = o.node.group_detail(g.clone()).unwrap().host_requests;
    assert!(req.len() == 1 && req[0].did == h.did && req[0].appointed_by.as_deref() == Some(o.did.as_str()));
    h.node.group_accept_host(g.clone(), true).unwrap();
    eventually(WAIT, "a sees two hosts", || a.node.group_detail(g.clone()).unwrap().hosts.len() == 2);
    let d = h.node.group_detail(g.clone()).unwrap();
    assert!(!d.host_requested_here && d.host_requests.is_empty() && d.am_host);
    o.node.group_set_rotation_delay_for_test(5);
    let old = o.node.group_endpoint_id().unwrap();
    o.node.group_revoke_host(g.clone(), h.did.clone(), h.node.group_device_id().unwrap()).unwrap();
    eventually(WAIT + 5, "o rotated", || o.node.group_endpoint_id().is_some_and(|e| e != old));
    let new = o.node.group_endpoint_id().unwrap();
    eventually(WAIT, "a follows", || {
        let k = a.node.group_known_endpoints_for_test(g.clone());
        k.contains(&new) && !k.contains(&old)
    });
    a.node.group_post(g.clone(), "after the revocation".into(), None).unwrap();
    eventually(WAIT, "o gets it", || has_post(&o, &g, &a.did, "after the revocation"));
}
