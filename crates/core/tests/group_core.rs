//! G1: group chat in core, first vertical slice, on real nodes over real iroh endpoints (needs
//! network, like `calls.rs`). Decisions: `rotation-open-decisions.md` §0 on
//! `endpoint-rotation-blocking` (U2 two endpoints, U7 only hosts know member endpoints, U9/U15
//! 3-flag removal, U10 every device verifies, U11 signed frontier, U14 trusted hosts, U16 writer
//! IDs). Protocol: `docs/group-core.md`.
//!
//! Every test carries `// KIND: acceptance (G1 stage N)` and the decision it enforces.

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
    let dir = std::env::temp_dir().join(format!("p2pcore-g1-{name}-{}", std::process::id()));
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
        assert_eq!(join(&m, tok), g);
    }
    let want = sorted(std::iter::once(o.did.clone()).chain(ms.iter().map(|m| m.did.clone())).collect());
    for p in std::iter::once(o).chain(ms.iter().copied()) {
        eventually(WAIT, "everyone sees every member", || members(p, &g) == want);
    }
    g
}

// ---- stage 2: group endpoint, host role, create / invite / join / post ---------------------------

// KIND: acceptance (G1 stage 2) — create, invite, join through the host, post both ways.
// Decisions: U7 (all traffic via the host), U10 (every device verifies), creator's node is the
// first host (PROVISIONAL default).
#[test]
fn create_invite_join_and_post_both_ways() {
    let o = peer("cj-o");
    let a = peer("cj-a");
    let g = group_with(&o, &[&a]);

    let d = o.node.group_detail(g.clone()).unwrap();
    assert_eq!(d.name, "Weekend hike");
    assert_eq!(d.my_role, GroupRole::Owner);
    assert!(d.am_host, "the creator's node is the first host");
    assert_eq!(d.hosts.len(), 1);
    let da = a.node.group_detail(g.clone()).unwrap();
    assert_eq!(da.my_role, GroupRole::Member);
    assert!(!da.am_host);
    assert_eq!(da.owner_did, o.did);
    assert_eq!(da.hosts[0].did, o.did);
    assert_eq!(a.node.groups().unwrap().iter().filter(|s| s.id == g).count(), 1);

    let m = a.node.group_post(g.clone(), "from a".into(), None).unwrap();
    assert!(m.outgoing);
    eventually(WAIT, "o sees a's post", || has_post(&o, &g, &a.did, "from a"));
    eventually(WAIT, "a's post is delivered to a host", || {
        all(&a, &g).iter().any(|x| x.id == m.id && x.delivery == GroupDelivery::Delivered)
    });
    o.node.group_post(g.clone(), "from o".into(), Some(m.id.clone())).unwrap();
    eventually(WAIT, "a sees o's post", || has_post(&a, &g, &o.did, "from o"));
    let reply = all(&a, &g).into_iter().find(|x| x.text == "from o").unwrap();
    assert_eq!(reply.reply_to.as_deref(), Some(m.id.as_str()));
    // A join shows as a system line.
    assert!(all(&o, &g).iter().any(|x| x.kind == GroupItemKind::Joined && x.subject_did == a.did));
}

// KIND: acceptance (G1 stage 2) — U2 + U7: a second endpoint for groups; only hosts learn
// members' group endpoints; members know only hosts' group endpoints; the private endpoint never
// appears in group traffic.
#[test]
fn member_endpoints_stay_with_hosts() {
    let o = peer("ep-o");
    let a = peer("ep-a");
    let b = peer("ep-b");
    let g = group_with(&o, &[&a, &b]);
    a.node.group_post(g.clone(), "hi".into(), None).unwrap();
    eventually(WAIT, "b sees a", || has_post(&b, &g, &a.did, "hi"));

    let ge = |p: &Peer| p.node.group_endpoint_id().expect("group endpoint up");
    for p in [&o, &a, &b] {
        assert_ne!(ge(p), p.node.status().endpoint_id, "group endpoint is not the private one");
    }
    let dir = o.node.group_directory_for_test(g.clone());
    assert!(dir.contains(&ge(&a)) && dir.contains(&ge(&b)), "the host knows members' group endpoints: {dir:?}");
    for (me, other) in [(&a, &b), (&b, &a)] {
        let known = me.node.group_known_endpoints_for_test(g.clone());
        assert_eq!(known, vec![ge(&o)], "a member knows only the host's group endpoint");
        assert!(!known.contains(&ge(other)) && !known.contains(&other.node.status().endpoint_id));
    }
    for p in [&o, &a, &b] {
        let known = p.node.group_known_endpoints_for_test(g.clone());
        for q in [&o, &a, &b] {
            assert!(!known.contains(&q.node.status().endpoint_id), "no private endpoint in group data");
        }
    }
}

// KIND: acceptance (G1 stage 2) — an offline member catches up through the host.
#[test]
fn offline_member_catches_up_via_host() {
    let o = peer("off-o");
    let a = peer("off-a");
    let b = peer("off-b");
    let g = group_with(&o, &[&a, &b]);
    b.node.stop();
    std::thread::sleep(Duration::from_secs(1));
    for i in 0..3 {
        a.node.group_post(g.clone(), format!("while b was away {i}"), None).unwrap();
    }
    eventually(WAIT, "the host holds them", || (0..3).all(|i| has_post(&o, &g, &a.did, &format!("while b was away {i}"))));
    online(&b.node);
    eventually(WAIT, "b catches up", || (0..3).all(|i| has_post(&b, &g, &a.did, &format!("while b was away {i}"))));
    // And b's own offline write reaches the others once it is back.
    b.node.stop();
    std::thread::sleep(Duration::from_secs(1));
    let m = b.node.group_post(g.clone(), "written offline".into(), None).unwrap();
    assert_eq!(m.delivery, GroupDelivery::Waiting);
    online(&b.node);
    eventually(WAIT, "a sees b's offline write", || has_post(&a, &g, &b.did, "written offline"));
}

// KIND: acceptance (G1 stage 2) — multiple hosts (§9 Q5): two members that each reach a
// different host converge; the hosts sync each other. Hosting is opt-in per device.
#[test]
fn members_on_different_hosts_converge() {
    let o = peer("mh-o");
    let h = peer("mh-h");
    let a = peer("mh-a");
    let b = peer("mh-b");
    let g = group_with(&o, &[&h, &a, &b]);
    // Owner appoints h's device; it is a host only once h accepts.
    o.node.group_appoint_host(g.clone(), h.did.clone(), h.node.group_device_id().unwrap()).unwrap();
    eventually(WAIT, "h sees the permit", || all(&h, &g).iter().any(|x| x.kind == GroupItemKind::HostAdded && x.subject_did == h.did));
    assert!(!h.node.group_detail(g.clone()).unwrap().am_host, "not before h accepts");
    assert_eq!(h.node.group_detail(g.clone()).unwrap().hosts.len(), 1);
    h.node.group_accept_host(g.clone(), true).unwrap();
    for p in [&o, &h, &a, &b] {
        eventually(WAIT, "two hosts everywhere", || p.node.group_detail(g.clone()).unwrap().hosts.len() == 2);
    }
    assert!(h.node.group_detail(g.clone()).unwrap().am_host);
    eventually(WAIT, "members learn h's group endpoint", || {
        let he = h.node.group_endpoint_id().unwrap();
        [&a, &b].iter().all(|p| p.node.group_known_endpoints_for_test(g.clone()).contains(&he))
    });

    a.node.group_dial_only_for_test(Some(o.did.clone()));
    b.node.group_dial_only_for_test(Some(h.did.clone()));
    a.node.group_post(g.clone(), "a via o".into(), None).unwrap();
    b.node.group_post(g.clone(), "b via h".into(), None).unwrap();
    eventually(WAIT, "b sees a's post via the other host", || has_post(&b, &g, &a.did, "a via o"));
    eventually(WAIT, "a sees b's post via the other host", || has_post(&a, &g, &b.did, "b via h"));
}

// KIND: acceptance (G1 stage 2) — a non-member cannot join without an admin's invite.
#[test]
fn a_stranger_without_an_invite_is_refused() {
    let o = peer("st-o");
    let a = peer("st-a");
    let x = peer("st-x");
    let g = group_with(&o, &[&a]);
    // A member's device token admits only that member's own DID.
    let tok = a.node.group_device_token(g.clone()).unwrap();
    assert!(x.node.group_join(tok).is_err());
    // A non-admin cannot issue an invite.
    assert!(a.node.group_invite(g.clone()).is_err());
    assert_eq!(members(&o, &g), sorted(vec![o.did.clone(), a.did.clone()]));
    assert!(x.node.groups().unwrap().is_empty());
}

// ---- stage 3: a member's second device -------------------------------------------------------------

// KIND: acceptance (G1 stage 3) — a linked device (same DID, own device ID / writer ID, own
// group endpoint) joins, sees history and sends; every device verifies (U10, U16).
#[test]
fn a_second_device_of_a_member_joins_sees_and_sends() {
    let o = peer("md-o");
    let a = peer("md-a");
    let g = group_with(&o, &[&a]);
    a.node.group_post(g.clone(), "before the second device".into(), None).unwrap();
    eventually(WAIT, "o has it", || has_post(&o, &g, &a.did, "before the second device"));

    let a2 = link_fresh(&a, "md-a2");
    assert_eq!(a2.did, a.did);
    assert_ne!(a2.node.group_device_id().unwrap(), a.node.group_device_id().unwrap());
    let tok = a.node.group_device_token(g.clone()).unwrap();
    assert_eq!(join(&a2, tok), g);
    eventually(WAIT, "a2 sees history", || has_post(&a2, &g, &a.did, "before the second device"));
    assert_eq!(a2.node.group_detail(g.clone()).unwrap().my_role, GroupRole::Member);

    a2.node.group_post(g.clone(), "from the second device".into(), None).unwrap();
    eventually(WAIT, "o sees it under a's DID", || has_post(&o, &g, &a.did, "from the second device"));
    eventually(WAIT, "a's first device sees it", || has_post(&a, &g, &a.did, "from the second device"));
    o.node.group_post(g.clone(), "to both devices".into(), None).unwrap();
    eventually(WAIT, "a2 receives", || has_post(&a2, &g, &o.did, "to both devices"));
    // Both devices of a are listed as a's writer devices; a2's own message is outgoing on a2.
    let det = o.node.group_detail(g.clone()).unwrap();
    let ma = det.members.iter().find(|m| m.did == a.did).unwrap();
    assert_eq!(ma.device_ids.len(), 2);
    assert!(all(&a, &g).iter().any(|m| m.text == "from the second device" && m.outgoing));
}

// ---- stage 4: admins, 3-flag removal, host refusal, leave -------------------------------------------

/// o owner+host, a admin, m and b members.
fn admin_group(tag: &str) -> (Peer, Peer, Peer, Peer, String) {
    let o = peer(&format!("{tag}-o"));
    let a = peer(&format!("{tag}-a"));
    let m = peer(&format!("{tag}-m"));
    let b = peer(&format!("{tag}-b"));
    let g = group_with(&o, &[&a, &m, &b]);
    o.node.group_set_admin(g.clone(), a.did.clone(), true).unwrap();
    for p in [&o, &a, &m, &b] {
        eventually(WAIT, "a is admin everywhere", || {
            p.node.group_detail(g.clone()).unwrap().members.iter().any(|x| x.did == a.did && x.is_admin)
        });
    }
    assert_eq!(a.node.group_detail(g.clone()).unwrap().my_role, GroupRole::Admin);
    (o, a, m, b, g)
}

// KIND: acceptance (G1 stage 4) — U9/U15: any 3 admin flags remove the member, on every device;
// the removed member's app shows the removal with the signed flags and disables posting; the
// group shows one neutral line. Non-admins cannot flag.
#[test]
fn three_admin_flags_remove_a_member_everywhere() {
    let (o, a, m, b, g) = admin_group("fl");
    assert!(b.node.group_flag(g.clone(), m.did.clone()).is_err(), "a non-admin cannot flag");
    a.node.group_flag(g.clone(), m.did.clone()).unwrap();
    a.node.group_flag(g.clone(), m.did.clone()).unwrap();
    eventually(WAIT, "two flags counted", || {
        o.node.group_detail(g.clone()).unwrap().members.iter().any(|x| x.did == m.did && x.flags == 2)
    });
    assert!(members(&b, &g).contains(&m.did));
    o.node.group_flag(g.clone(), m.did.clone()).unwrap();
    for p in [&o, &a, &b] {
        eventually(WAIT, "m removed for everyone", || !members(p, &g).contains(&m.did));
        eventually(WAIT, "one removal line", || all(p, &g).iter().any(|x| x.kind == GroupItemKind::Removed && x.subject_did == m.did));
    }
    eventually(WAIT, "m's app shows removed", || m.node.group_detail(g.clone()).unwrap().my_role == GroupRole::Removed);
    let d = m.node.group_detail(g.clone()).unwrap();
    let r = d.removal.expect("removal details");
    assert_eq!(r.flags.len(), 3);
    assert_eq!(sorted(r.flags.iter().map(|f| f.by_did.clone()).collect()), sorted(vec![a.did.clone(), a.did.clone(), o.did.clone()]));
    assert!(m.node.group_post(g.clone(), "still here?".into(), None).is_err(), "composer disabled");
    let s = m.node.groups().unwrap().into_iter().find(|s| s.id == g).unwrap();
    assert_eq!(s.my_role, GroupRole::Removed);
}

// KIND: acceptance (G1 stage 4) — U12 detail: refusal is immediate at the host even against a
// modified app that ignores its removal; a post written after seeing the removal never shows.
#[test]
fn the_host_refuses_a_removed_member_that_ignores_its_removal() {
    let (o, a, m, b, g) = admin_group("rf");
    m.node.group_ignore_removal_for_test(true);
    for _ in 0..3 {
        a.node.group_flag(g.clone(), m.did.clone()).unwrap();
    }
    eventually(WAIT, "o applied the removal", || !members(&o, &g).contains(&m.did));
    eventually(WAIT, "m's replica has the flags", || {
        m.node.group_detail(g.clone()).unwrap().members.iter().all(|x| x.did != m.did)
    });
    m.node.group_post(g.clone(), "sneaking in".into(), None).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    for p in [&o, &a, &b] {
        assert!(!has_post(p, &g, &m.did, "sneaking in"), "refused everywhere");
    }
}

// KIND: acceptance (G1 stage 4) — U11 option A: a post the member wrote offline before the
// removal (its signed frontier predates the flags) is still accepted by the host and shown,
// marked as written before the removal.
#[test]
fn an_offline_post_written_before_the_removal_is_delivered() {
    let (o, a, m, b, g) = admin_group("bf");
    m.node.stop();
    std::thread::sleep(Duration::from_secs(1));
    m.node.group_post(g.clone(), "running late, start without me".into(), None).unwrap();
    for _ in 0..3 {
        a.node.group_flag(g.clone(), m.did.clone()).unwrap();
    }
    eventually(WAIT, "o applied the removal", || !members(&o, &g).contains(&m.did));
    online(&m.node);
    eventually(WAIT, "m learns it was removed", || m.node.group_detail(g.clone()).unwrap().my_role == GroupRole::Removed);
    for p in [&o, &b] {
        eventually(WAIT, "the pre-removal post arrives", || has_post(p, &g, &m.did, "running late, start without me"));
        let x = all(p, &g).into_iter().find(|x| x.text == "running late, start without me").unwrap();
        assert!(x.before_removal);
    }
    let _ = a;
}

// KIND: acceptance (G1 stage 4) — leaving: the member is gone for everyone and cannot post.
#[test]
fn a_member_can_leave() {
    let o = peer("lv-o");
    let a = peer("lv-a");
    let b = peer("lv-b");
    let g = group_with(&o, &[&a, &b]);
    a.node.group_leave(g.clone()).unwrap();
    for p in [&o, &b] {
        eventually(WAIT, "a left", || !members(p, &g).contains(&a.did));
    }
    assert_eq!(a.node.group_detail(g.clone()).unwrap().my_role, GroupRole::Left);
    assert!(a.node.group_post(g.clone(), "x".into(), None).is_err());
    assert!(all(&o, &g).iter().any(|x| x.kind == GroupItemKind::Left && x.subject_did == a.did));
}

// KIND: acceptance (G1 stage 4) — an admin can revoke a host; the revoked device stops hosting.
#[test]
fn an_admin_can_revoke_a_host() {
    let o = peer("rh-o");
    let h = peer("rh-h");
    let a = peer("rh-a");
    let g = group_with(&o, &[&h, &a]);
    o.node.group_appoint_host(g.clone(), h.did.clone(), h.node.group_device_id().unwrap()).unwrap();
    eventually(WAIT, "h sees the permit", || all(&h, &g).iter().any(|x| x.kind == GroupItemKind::HostAdded && x.subject_did == h.did));
    h.node.group_accept_host(g.clone(), true).unwrap();
    eventually(WAIT, "a sees two hosts", || a.node.group_detail(g.clone()).unwrap().hosts.len() == 2);
    o.node.group_revoke_host(g.clone(), h.did.clone(), h.node.group_device_id().unwrap()).unwrap();
    for p in [&o, &h, &a] {
        eventually(WAIT, "one host again", || p.node.group_detail(g.clone()).unwrap().hosts.len() == 1);
    }
    assert!(!h.node.group_detail(g.clone()).unwrap().am_host);
    assert!(all(&a, &g).iter().any(|x| x.kind == GroupItemKind::HostRemoved && x.subject_did == h.did));
}

// ---- persistence ------------------------------------------------------------------------------------

// KIND: acceptance (G1) — groups live in the sealed group store: after a process restart the
// host still hosts (same group endpoint) and members reconnect and keep posting.
#[test]
fn groups_survive_a_host_restart() {
    let mut o = peer("rs-o");
    let a = peer("rs-a");
    let g = group_with(&o, &[&a]);
    a.node.group_post(g.clone(), "before the restart".into(), None).unwrap();
    eventually(WAIT, "o has it", || has_post(&o, &g, &a.did, "before the restart"));
    let ge = o.node.group_endpoint_id().unwrap();
    o.node.stop();
    std::thread::sleep(Duration::from_secs(2));
    let node = Node::new(o.dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    let old = std::mem::replace(&mut o.node, node);
    drop(old);
    o.node.unlock(PASS.into()).unwrap();
    online(&o.node);
    assert!(has_post(&o, &g, &a.did, "before the restart"), "history reloaded");
    let d = o.node.group_detail(g.clone()).unwrap();
    assert!(d.am_host && d.my_role == GroupRole::Owner);
    eventually(WAIT, "group endpoint back up", || o.node.group_endpoint_id().as_deref() == Some(ge.as_str()));
    a.node.group_post(g.clone(), "after the restart".into(), None).unwrap();
    eventually(WAIT, "a reconnects to the restarted host", || has_post(&o, &g, &a.did, "after the restart"));
    o.node.group_post(g.clone(), "host is back".into(), None).unwrap();
    eventually(WAIT, "a receives", || has_post(&a, &g, &o.did, "host is back"));
}
