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
    // The hosts stop syncing each other (members relay too, so b talks to o only); a talks to h
    // only.
    o.node.group_dial_only_for_test(Some(o.did.clone()));
    h.node.group_dial_only_for_test(Some(h.did.clone()));
    b.node.group_dial_only_for_test(Some(o.did.clone()));
    a.node.group_dial_only_for_test(Some(h.did.clone()));
    // The other side of each closed session notices over the network.
    std::thread::sleep(Duration::from_secs(4));
    a.node.group_post(g.clone(), "only h has this".into(), None).unwrap();
    eventually(WAIT, "h acknowledged it", || delivered(&a, &g, "only h has this"));
    std::thread::sleep(Duration::from_secs(1));
    assert!(!has_post(&o, &g, &a.did, "only h has this"), "o must not have it yet");
    h.node.stop();
    restart(&mut a, true);
    let tok = o.node.group_invite(g.clone()).unwrap();
    assert_eq!(a.node.group_join(tok).unwrap(), g);
    a.node.group_dial_only_for_test(Some(o.did.clone()));
    a.node.group_post(g.clone(), "written after the loss".into(), None).unwrap();
    eventually(WAIT, "o has the new post", || has_post(&o, &g, &a.did, "written after the loss"));
    // h comes back; the hosts sync again.
    online(&h.node);
    h.node.group_dial_only_for_test(None);
    o.node.group_dial_only_for_test(None);
    a.node.group_dial_only_for_test(None);
    b.node.group_dial_only_for_test(None);
    for p in [&o, &h, &b, &a] {
        eventually(WAIT, "both posts everywhere", || {
            has_post(p, &g, &a.did, "only h has this") && has_post(p, &g, &a.did, "written after the loss")
        });
    }
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

