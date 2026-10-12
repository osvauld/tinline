//! Group day shards (`docs/group-shards.md`): paging across days, the chat list without loading
//! days, compaction and restart, history backfill to a new member and to a wiped host, posts
//! that arrive before the records they name, and several hosts. Real nodes over real iroh
//! endpoints (needs network, like `group_core.rs`, whose helpers are copied here).
//!
//! Every test carries `// KIND: acceptance (group shards)`.
#![allow(dead_code)]

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, GroupItemKind, GroupMessage, LinkEvents, Node, NodeEvents, NodeStatus};

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



const DAY_MS: i64 = 24 * 3_600_000;

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn texts(p: &Peer, g: &str) -> Vec<String> {
    all(p, g).into_iter().filter(|m| m.kind == GroupItemKind::Post).map(|m| m.text).collect()
}

/// `n` days of history, `per` posts a day, oldest first: day `i` is `n - i` days ago.
fn write_history(o: &Peer, g: &str, n: i64, per: i64) -> Vec<String> {
    let mut out = Vec::new();
    for d in (1..=n).rev() {
        for k in 0..per {
            let t = format!("day-{d}-post-{k}");
            // Mid-day, so the post sits in its day whatever the hour of the test run.
            let at = now_ms() / DAY_MS * DAY_MS - d * DAY_MS + 12 * 3_600_000 + k * 1000;
            o.node.group_post_at_for_test(g.into(), at, t.clone()).unwrap();
            out.push(t);
        }
    }
    out
}

// KIND: acceptance (group shards) — `group_messages` pages backwards across day docs: oldest
// first within a page, no gaps or repeats, the cursor of the page's first message ends the next
// page, `older` is `None` at the very first message; only a bounded number of day docs is in
// memory; the cursor `{at}:{id}~` of a message still ends the page at that message.
#[test]
fn paging_crosses_days_and_the_cache_is_bounded() {
    let o = peer("pg-o");
    let g = o.node.group_create("Paging".into()).unwrap();
    let mut want = write_history(&o, &g, 20, 3);
    o.node.group_post(g.clone(), "today".into(), None).unwrap();
    want.push("today".into());
    assert_eq!(texts(&o, &g), want, "all pages together are the whole conversation, in order");
    // Pages of 7 across 20 day boundaries.
    let mut seen = Vec::new();
    let mut before = None;
    let mut pages = 0;
    loop {
        let page = o.node.group_messages(g.clone(), before, 7).unwrap();
        assert!(page.messages.len() <= 7);
        let mut posts: Vec<String> = page.messages.iter().filter(|m| m.kind == GroupItemKind::Post).map(|m| m.text.clone()).collect();
        posts.extend(seen.drain(..));
        seen = posts;
        pages += 1;
        match page.older {
            Some(c) => before = Some(c),
            None => break,
        }
    }
    assert!(pages >= 9);
    assert_eq!(seen, want);
    assert!(o.node.group_cached_days_for_test(g.clone()).unwrap() <= 12, "day docs in memory are bounded");
    // A hit's cursor puts the page's end at that message, from a day far back.
    let target = all(&o, &g).into_iter().find(|m| m.text == "day-15-post-1").unwrap();
    let page = o.node.group_messages(g.clone(), Some(format!("{}:{}~", target.at, target.id)), 4).unwrap();
    assert_eq!(page.messages.last().map(|m| m.id.clone()), Some(target.id));
    assert!(page.older.is_some());
    // Days are real: 21 of them, one per posting day.
    assert_eq!(o.node.group_days_for_test(g.clone()).unwrap().len(), 21);
}

// KIND: acceptance (group shards) — the chat-list row (last message, unread) comes from the
// group's local state: after a restart, `groups()` answers without loading any day doc; reading
// resets unread; a new message moves the row.
#[test]
fn the_chat_list_row_and_unread_do_not_load_days() {
    let o = peer("cl-o");
    let mut a = peer("cl-a");
    let g = group_with(&o, &[&a]);
    for i in 0..3 {
        o.node.group_post(g.clone(), format!("news {i}"), None).unwrap();
    }
    eventually(WAIT, "a has them", || has_post(&a, &g, &o.did, "news 2"));
    let row = |p: &Peer| p.node.groups().unwrap().into_iter().find(|s| s.id == g).unwrap();
    assert_eq!(row(&a).unread, 3);
    assert_eq!(row(&a).last.as_ref().map(|m| m.text.clone()), Some("news 2".into()));
    restart(&mut a, false);
    assert_eq!(a.node.group_cached_days_for_test(g.clone()).unwrap(), 0);
    let r = row(&a);
    assert_eq!((r.unread, r.last.map(|m| m.text)), (3, Some("news 2".into())));
    assert_eq!(a.node.group_cached_days_for_test(g.clone()).unwrap(), 0, "the list loaded no day doc");
    a.node.group_mark_read(g.clone()).unwrap();
    assert_eq!(row(&a).unread, 0);
    o.node.group_post(g.clone(), "news 3".into(), None).unwrap();
    eventually(WAIT, "the row moves", || {
        let r = row(&a);
        r.unread == 1 && r.last.is_some_and(|m| m.text == "news 3")
    });
}

// KIND: acceptance (group shards) — days older than the closing age are compacted into one
// snapshot row each and nothing is lost: after a restart (closed days only reload on demand)
// every message is still there, in order, and a late post into a closed day opens it again.
#[test]
fn closed_days_survive_a_restart() {
    let mut o = peer("cd-o");
    let g = o.node.group_create("Closed".into()).unwrap();
    let want = write_history(&o, &g, 14, 2);
    // (The hourly compaction may have closed the first days while they were being written.)
    o.node.group_compact_now_for_test().unwrap();
    let closed = o.node.group_closed_days_for_test(g.clone()).unwrap();
    assert_eq!(closed, 6, "the days 9 to 14 days ago are closed");
    assert_eq!(o.node.group_compact_now_for_test().unwrap(), 0, "nothing left to close");
    restart(&mut o, false);
    assert_eq!(o.node.group_cached_days_for_test(g.clone()).unwrap(), 0);
    assert_eq!(texts(&o, &g), want);
    assert_eq!(o.node.group_closed_days_for_test(g.clone()).unwrap(), closed, "still closed after the restart");
    // A late post into a closed day: the day opens again, then closes at the next compaction.
    let at = now_ms() / DAY_MS * DAY_MS - 12 * DAY_MS + 15 * 3_600_000;
    o.node.group_post_at_for_test(g.clone(), at, "late".into()).unwrap();
    assert_eq!(o.node.group_closed_days_for_test(g.clone()).unwrap(), closed - 1);
    // (The restart's own compaction may get there first.)
    o.node.group_compact_now_for_test().unwrap();
    assert_eq!(o.node.group_closed_days_for_test(g.clone()).unwrap(), closed);
    assert!(texts(&o, &g).contains(&"late".to_string()));
    assert_eq!(texts(&o, &g).len(), want.len() + 1);
}

// KIND: acceptance (group shards, decision 3) — every member holds the full history: a new
// member is filled with all days in the background by the inventory sync with its host.
#[test]
fn a_new_member_backfills_the_whole_history() {
    let o = peer("bf-o");
    let g = o.node.group_create("History".into()).unwrap();
    let want = write_history(&o, &g, 30, 2);
    let a = peer("bf-a");
    a.node.group_backfill_pause_ms_for_test(5);
    let tok = o.node.group_invite(g.clone()).unwrap();
    assert_eq!(join(&a, tok), g);
    eventually(WAIT, "a holds every day", || texts(&a, &g) == want);
    assert_eq!(a.node.group_days_for_test(g.clone()).unwrap(), o.node.group_days_for_test(g.clone()).unwrap());
    // And it serves them on: a second member that can only reach a's view of the group is not
    // needed; the host's copy is intact.
    assert_eq!(texts(&o, &g), want);
}

// KIND: acceptance (group shards, decision 3) — hosts and members exchange the days the other
// lacks: a host that lost its history (a wiped or new host) is refilled by a member in session.
#[test]
fn a_wiped_host_is_refilled_from_a_member() {
    let mut o = peer("wh-o");
    let a = peer("wh-a");
    let g = group_with(&o, &[&a]);
    let want = write_history(&o, &g, 12, 2);
    o.node.group_post(g.clone(), "today".into(), None).unwrap();
    a.node.group_post(g.clone(), "from a".into(), None).unwrap();
    eventually(WAIT, "a holds it all", || texts(&a, &g).len() == want.len() + 2);
    let held = a.node.group_days_for_test(g.clone()).unwrap();
    // The host forgets every day, then restarts: its session with a comes back, and it asks a.
    assert_eq!(o.node.group_drop_days_for_test(g.clone()).unwrap() as usize, held.len());
    assert!(texts(&o, &g).is_empty());
    restart(&mut o, false);
    o.node.group_backfill_pause_ms_for_test(5);
    eventually(WAIT, "the host has its history back", || texts(&o, &g).len() == want.len() + 2);
    assert_eq!(o.node.group_days_for_test(g.clone()).unwrap(), held);
    let mut t = texts(&o, &g);
    t.sort();
    let mut w = texts(&a, &g);
    w.sort();
    assert_eq!(t, w);
}

// KIND: acceptance (group shards) — a post that arrives before the meta records it names
// (here: its author's join) is held pending and hidden, not refused; it shows (with unread and
// the chat-list row) as soon as the meta doc advances.
#[test]
fn a_post_before_its_meta_is_pending_then_shown() {
    let o = peer("pm-o");
    let phrase = o.node.synth_phrase_for_test();
    let did = o.node.synth_peer_for_test(phrase).unwrap();
    let mut sg = o.node.synth_group_new_for_test("Pending".into()).unwrap();
    let g = sg.gid();
    let at = now_ms();
    let m = o.node.synth_group_add_for_test(&mut sg, did.clone(), "early".into(), at).unwrap();
    o.node.synth_group_post_for_test(&mut sg, m, at + 10, "early bird".into()).unwrap();
    // The day doc first: the post names the join, which we do not hold yet.
    o.node.synth_group_flush_part_for_test(&mut sg, false, true).unwrap();
    assert!(!texts(&o, &g).contains(&"early bird".to_string()), "pending posts are hidden");
    let row = o.node.groups().unwrap().into_iter().find(|s| s.id == g).unwrap();
    assert_eq!(row.unread, 0);
    assert!(row.last.is_none_or(|m| m.text != "early bird"));
    // Then the meta doc: the same post shows.
    o.node.synth_group_flush_part_for_test(&mut sg, true, false).unwrap();
    assert!(texts(&o, &g).contains(&"early bird".to_string()));
    let row = o.node.groups().unwrap().into_iter().find(|s| s.id == g).unwrap();
    assert_eq!((row.unread, row.last.map(|x| x.text)), (1, Some("early bird".into())));
    // And a stranger's post (no join at all) stays hidden whatever the order.
    let phrase = o.node.synth_phrase_for_test();
    let stranger = o.node.synth_peer_for_test(phrase).unwrap();
    let mut r = o.node.synth_remote_open_for_test(g.clone(), stranger).unwrap();
    o.node.synth_remote_post_for_test(&mut r, "spam".into(), now_ms() + 20).unwrap();
    assert!(!texts(&o, &g).contains(&"spam".to_string()));
}

// KIND: acceptance (group shards) — members on different hosts converge per day doc: a recent
// and an older post written through different hosts reach everyone, and history the other host
// lacks arrives by backfill.
#[test]
fn hosts_converge_per_day_doc_and_share_history() {
    let o = peer("hc-o");
    let h = peer("hc-h");
    let a = peer("hc-a");
    let b = peer("hc-b");
    let g = group_with(&o, &[&h, &a, &b]);
    o.node.group_appoint_host(g.clone(), h.did.clone(), h.node.group_device_id().unwrap()).unwrap();
    eventually(WAIT, "h sees the permit", || all(&h, &g).iter().any(|x| x.kind == GroupItemKind::HostAdded && x.subject_did == h.did));
    h.node.group_accept_host(g.clone(), true).unwrap();
    for p in [&o, &h, &a, &b] {
        eventually(WAIT, "two hosts everywhere", || p.node.group_detail(g.clone()).unwrap().hosts.len() == 2);
        p.node.group_backfill_pause_ms_for_test(5);
    }
    eventually(WAIT, "members learn h's group endpoint", || {
        let he = h.node.group_endpoint_id().unwrap();
        [&a, &b].iter().all(|p| p.node.group_known_endpoints_for_test(g.clone()).contains(&he))
    });
    a.node.group_dial_only_for_test(Some(o.did.clone()));
    b.node.group_dial_only_for_test(Some(h.did.clone()));
    a.node.group_post(g.clone(), "a via o today".into(), None).unwrap();
    b.node.group_post(g.clone(), "b via h today".into(), None).unwrap();
    let three = now_ms() - 3 * DAY_MS;
    let twenty = now_ms() - 20 * DAY_MS;
    a.node.group_post_at_for_test(g.clone(), three, "a via o, 3 days ago".into()).unwrap();
    b.node.group_post_at_for_test(g.clone(), twenty, "b via h, 20 days ago".into()).unwrap();
    for (who, p) in [("o", &o), ("h", &h), ("a", &a), ("b", &b)] {
        for t in ["a via o today", "b via h today", "a via o, 3 days ago", "b via h, 20 days ago"] {
            eventually(WAIT, &format!("{who} has {t}"), || texts(p, &g).iter().any(|x| x == t));
        }
    }
    let days = o.node.group_days_for_test(g.clone()).unwrap();
    for p in [&h, &a, &b] {
        eventually(WAIT, "the same days everywhere", || p.node.group_days_for_test(g.clone()).unwrap() == days);
    }
}

// ---- liveness ---------------------------------------------------------------------------------

fn utc_day(ms: i64) -> String {
    let z = ms.div_euclid(DAY_MS) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

/// The newest page holds a post with this text.
fn newest_has(p: &Peer, g: &str, text: &str) -> bool {
    p.node.group_messages(g.into(), None, 8).map(|pg| pg.messages.iter().any(|m| m.text == text)).unwrap_or(false)
}

fn two_hosts(tag: &str, extra: usize) -> (Peer, Peer, Vec<Peer>, String) {
    let o = peer(&format!("{tag}-o"));
    let h = peer(&format!("{tag}-h"));
    let ms: Vec<Peer> = (0..extra).map(|i| peer(&format!("{tag}-m{i}"))).collect();
    let mut refs: Vec<&Peer> = vec![&h];
    refs.extend(ms.iter());
    let g = group_with(&o, &refs);
    o.node.group_appoint_host(g.clone(), h.did.clone(), h.node.group_device_id().unwrap()).unwrap();
    eventually(WAIT, "h sees the permit", || all(&h, &g).iter().any(|x| x.kind == GroupItemKind::HostAdded && x.subject_did == h.did));
    h.node.group_accept_host(g.clone(), true).unwrap();
    for p in std::iter::once(&o).chain(std::iter::once(&h)).chain(ms.iter()) {
        eventually(WAIT, "two hosts everywhere", || p.node.group_detail(g.clone()).unwrap().hosts.len() == 2);
    }
    let he = h.node.group_endpoint_id().unwrap();
    for m in &ms {
        eventually(WAIT, "members learn h", || m.node.group_known_endpoints_for_test(g.clone()).contains(&he));
    }
    (o, h, ms, g)
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() as f64 * q).ceil() as usize).clamp(1, v.len()) - 1]
}

// KIND: acceptance (group liveness a) — a post written by one member shows on another member
// through the host within a second (loopback), p95 over 50 posts.
#[test]
fn a_post_shows_on_another_member_within_a_second() {
    let o = peer("lv-o");
    let a = peer("lv-a");
    let b = peer("lv-b");
    let g = group_with(&o, &[&a, &b]);
    let mut ms = Vec::new();
    for i in 0..50 {
        let text = format!("latency {i}");
        let t = Instant::now();
        a.node.group_post(g.clone(), text.clone(), None).unwrap();
        while !newest_has(&b, &g, &text) {
            assert!(t.elapsed() < Duration::from_secs(20), "post {i} never arrived");
            std::thread::sleep(Duration::from_millis(2));
        }
        ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let (p50, p95) = (pct(&mut ms.clone(), 0.5), pct(&mut ms, 0.95));
    println!("LIVENESS post->shown over 50 posts: p50 {p50:.0} ms, p95 {p95:.0} ms");
    assert!(p95 < 1000.0, "p95 {p95} ms");
}

// KIND: acceptance (group liveness b) — a burst of 200 posts from one member arrives complete
// and in order on two members connected to different hosts.
#[test]
fn a_burst_of_200_arrives_complete_and_in_order_through_two_hosts() {
    let (o, h, ms, g) = two_hosts("bu", 3);
    let (a, b, c) = (&ms[0], &ms[1], &ms[2]);
    a.node.group_dial_only_for_test(Some(o.did.clone()));
    b.node.group_dial_only_for_test(Some(o.did.clone()));
    c.node.group_dial_only_for_test(Some(h.did.clone()));
    let want: Vec<String> = (0..200).map(|i| format!("burst {i:03}")).collect();
    for t in &want {
        a.node.group_post(g.clone(), t.clone(), None).unwrap();
    }
    for (who, p) in [("b", b), ("c", c), ("o", &o), ("h", &h)] {
        eventually(WAIT, &format!("{who} has the burst"), || texts(p, &g).iter().filter(|t| t.starts_with("burst")).count() == 200);
        let got: Vec<String> = texts(p, &g).into_iter().filter(|t| t.starts_with("burst")).collect();
        let at: Vec<u64> = all(p, &g).iter().filter(|m| m.text.starts_with("burst")).map(|m| m.at).collect();
        assert!(at.windows(2).all(|w| w[0] <= w[1]), "{who}: in time order");
        let mut sorted = got.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, want, "{who}: complete, no duplicates");
    }
    // Ties of the millisecond clock are ordered by id, the same on every device.
    assert_eq!(texts(b, &g), texts(c, &g));
}

// KIND: acceptance (group liveness c) — three members posting at the same time converge.
#[test]
fn three_members_posting_concurrently_converge() {
    let (o, h, ms, g) = two_hosts("cc", 3);
    for (i, m) in ms.iter().enumerate() {
        m.node.group_dial_only_for_test(Some(if i % 2 == 0 { o.did.clone() } else { h.did.clone() }));
    }
    let handles: Vec<_> = ms
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let (n, g) = (m.node.clone(), g.clone());
            std::thread::spawn(move || {
                for k in 0..30 {
                    n.group_post(g.clone(), format!("m{i} says {k}"), None).unwrap();
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
        })
        .collect();
    for t in handles {
        t.join().unwrap();
    }
    let mut want: Vec<String> = (0..3).flat_map(|i| (0..30).map(move |k| format!("m{i} says {k}"))).collect();
    want.sort();
    for p in ms.iter().chain([&o, &h]) {
        eventually(WAIT, "everyone has all 90", || texts(p, &g).len() == 90);
        let mut got = texts(p, &g);
        let order = got.clone();
        got.sort();
        assert_eq!(got, want);
        assert_eq!(order, texts(&ms[0], &g), "same order everywhere");
    }
}

// KIND: acceptance (group liveness d) — a host restarts in the middle of a burst: every post
// still arrives, none twice.
#[test]
fn a_host_restart_mid_burst_loses_and_duplicates_nothing() {
    let mut o = peer("rs-o");
    let a = peer("rs-a");
    let b = peer("rs-b");
    let g = group_with(&o, &[&a, &b]);
    let (n, gg) = (a.node.clone(), g.clone());
    let writer = std::thread::spawn(move || {
        for k in 0..100 {
            n.group_post(gg.clone(), format!("mid {k:03}"), None).unwrap();
            std::thread::sleep(Duration::from_millis(30));
        }
    });
    std::thread::sleep(Duration::from_millis(900));
    restart(&mut o, false);
    writer.join().unwrap();
    for (who, p) in [("o", &o), ("b", &b), ("a", &a)] {
        eventually(WAIT, &format!("{who} has all 100"), || texts(p, &g).iter().filter(|t| t.starts_with("mid")).count() == 100);
        let mut t: Vec<String> = texts(p, &g).into_iter().filter(|t| t.starts_with("mid")).collect();
        t.sort();
        t.dedup();
        assert_eq!(t.len(), 100, "{who}: no duplicates");
    }
}

// KIND: acceptance (group liveness e) — posts either side of a UTC midnight land in the right
// day docs and all arrive.
#[test]
fn posts_across_utc_midnight_land_in_the_right_days() {
    let o = peer("mn-o");
    let a = peer("mn-a");
    let g = group_with(&o, &[&a]);
    let midnight = now_ms() / DAY_MS * DAY_MS - 3 * DAY_MS;
    let times = [midnight - 2000, midnight - 1, midnight, midnight + 1, midnight + 2000];
    for (i, at) in times.iter().enumerate() {
        let p = if i % 2 == 0 { &o } else { &a };
        p.node.group_post_at_for_test(g.clone(), *at, format!("edge {i}")).unwrap();
    }
    let (before, after) = (utc_day(midnight - 1), utc_day(midnight));
    assert_ne!(before, after);
    for p in [&o, &a] {
        eventually(WAIT, "all five edge posts", || texts(p, &g).iter().filter(|t| t.starts_with("edge")).count() == 5);
        let days = p.node.group_days_for_test(g.clone()).unwrap();
        assert!(days.contains(&before) && days.contains(&after), "{days:?}");
        let order: Vec<String> = texts(p, &g);
        assert_eq!(order, (0..5).map(|i| format!("edge {i}")).collect::<Vec<_>>());
    }
}

// KIND: acceptance (group liveness f) — a member that was offline for 10 days catches up every
// day through the inventory sync (the days older than a week are not pushed live).
#[test]
fn a_member_offline_for_ten_days_catches_up_every_day() {
    let o = peer("of-o");
    let a = peer("of-a");
    let g = group_with(&o, &[&a]);
    a.node.group_backfill_pause_ms_for_test(5);
    o.node.group_backfill_pause_ms_for_test(5);
    a.node.stop();
    std::thread::sleep(Duration::from_secs(1));
    let want = write_history(&o, &g, 10, 3);
    o.node.group_post(g.clone(), "today".into(), None).unwrap();
    online(&a.node);
    eventually(WAIT, "a has every day", || texts(&a, &g).len() == want.len() + 1);
    assert_eq!(a.node.group_days_for_test(g.clone()).unwrap(), o.node.group_days_for_test(g.clone()).unwrap());
    assert_eq!(texts(&a, &g), texts(&o, &g));
}
