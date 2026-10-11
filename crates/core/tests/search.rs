//! S1: local search (`docs/search.md`) on real nodes. The "You" conversation needs no network;
//! the 1:1 and group tests run two nodes over real iroh endpoints like `lifecycle.rs`.
//!
//! Every test carries `// KIND: acceptance (S1)`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, Error, LinkEvents, MessageHit, Node, NodeEvents, NodeStatus, SearchConvKind, SearchNameKind};

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

struct NoLink;
impl LinkEvents for NoLink {
    fn on_link_code(&self, _: String, _: String) {}
    fn on_link_done(&self, _: String, _: String) {}
    fn on_link_failed(&self, _: String) {}
    fn on_devices_changed(&self) {}
    fn on_history_changed(&self) {}
    fn on_unlinked(&self, _: String) {}
}

struct Root(PathBuf);
impl Root {
    fn new(tag: &str) -> Root {
        let p = std::env::temp_dir().join(format!("p2pcore-search-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Root(p)
    }
    fn node(&self) -> Arc<Node> {
        let n = Node::new(self.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
        n.set_link_events(Arc::new(NoLink));
        n
    }
    fn acct(&self, did: &str) -> PathBuf {
        self.0.join("accounts").join(did.strip_prefix("did:key:").unwrap())
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Peer {
    node: Arc<Node>,
    did: String,
    _root: Root,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.node.stop();
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
    let root = Root::new(name);
    let node = root.node();
    node.create_identity(name.into(), PASS.into()).unwrap();
    online(&node);
    let did = node.profile().unwrap().did;
    Peer { node, did, _root: root }
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

fn hits(n: &Node, q: &str) -> Vec<MessageHit> {
    n.search(q.into(), 100, None).map(|r| r.messages).unwrap_or_default()
}

fn texts(n: &Node, q: &str) -> Vec<String> {
    hits(n, q).into_iter().map(|h| h.snippet).collect()
}

fn highlighted(h: &MessageHit) -> Vec<String> {
    let cs: Vec<char> = h.snippet.chars().collect();
    h.highlights.iter().map(|r| cs[r.start as usize..r.end as usize].iter().collect()).collect()
}

fn files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect()
}

// KIND: acceptance (S1) — the "You" conversation: found by word prefix (AND, case/accents
// ignored) with highlights; an edit re-indexes, a delete removes; pages run newest first; names
// find "You"; at rest the index file holds none of the words.
#[test]
fn you_conversation_edit_delete_paging_sealed() {
    let root = Root::new("you");
    let n = root.node();
    n.create_identity("Zoë".into(), PASS.into()).unwrap();
    let me = n.profile().unwrap().did;
    let m1 = n.send_text(me.clone(), "Remember the Café receipt".into(), None).unwrap();
    let m2 = n.send_text(me.clone(), "grocery list: bananas, oats".into(), None).unwrap();

    let h = hits(&n, "cafe REC");
    assert_eq!(h.len(), 1);
    assert_eq!((h[0].kind, h[0].conv.as_str(), h[0].id.as_str(), h[0].conv_name.as_str()), (SearchConvKind::Dm, me.as_str(), m1.id.as_str(), "You"));
    assert_eq!(highlighted(&h[0]), ["Café", "rec"]);
    assert!(hits(&n, "cafe bananas").is_empty(), "every word must match");
    assert!(hits(&n, "afe").is_empty(), "word prefixes, not substrings");
    // The day opens the hit.
    let page = n.chat_day(me.clone(), Some(h[0].day.clone())).unwrap();
    assert!(page.messages.iter().any(|m| m.id == m1.id));

    n.edit_message(me.clone(), m2.id.clone(), "grocery list: apples".into()).unwrap();
    assert!(hits(&n, "banana").is_empty(), "the old text is gone");
    assert_eq!(hits(&n, "apple")[0].id, m2.id);
    n.delete_message(me.clone(), m2.id.clone()).unwrap();
    assert!(hits(&n, "grocery").is_empty(), "a deleted message is not found");

    let mut ids = Vec::new();
    for i in 0..7 {
        ids.push(n.send_text(me.clone(), format!("standup note {i}"), None).unwrap().id);
        std::thread::sleep(Duration::from_millis(3));
    }
    let mut seen = Vec::new();
    let mut cursor = None;
    loop {
        let r = n.search("standup".into(), 3, cursor).unwrap();
        assert!(r.messages.len() <= 3);
        seen.extend(r.messages.iter().map(|m| m.id.clone()));
        match r.next {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    ids.reverse();
    assert_eq!(seen, ids, "newest first, each once");

    let names = n.search("you".into(), 10, None).unwrap().names;
    assert!(names.iter().any(|h| h.kind == SearchNameKind::Me && h.id == me));

    n.lock();
    eventually(10, "locked", || n.search("standup".into(), 5, None).is_err());
    let raw = std::fs::read(root.acct(&me).join("search.redb")).unwrap();
    for w in ["Remember", "receipt", "standup", "Caf", "apples"] {
        assert!(!raw.windows(w.len()).any(|x| x == w.as_bytes()), "{w} at rest");
    }
}

// KIND: acceptance (S1) — the index is derived data: deleted while locked, it is rebuilt in the
// background after the next unlock.
#[test]
fn deleted_index_is_rebuilt() {
    let root = Root::new("rebuild");
    let n = root.node();
    n.create_identity("rita".into(), PASS.into()).unwrap();
    let me = n.profile().unwrap().did;
    n.send_text(me.clone(), "the lighthouse keeper".into(), None).unwrap();
    assert_eq!(hits(&n, "lighth").len(), 1);
    n.lock();
    eventually(10, "locked", || n.search("keeper".into(), 5, None).is_err());
    let path = root.acct(&me).join("search.redb");
    std::fs::remove_file(&path).unwrap();
    assert!(!files(&root.acct(&me)).contains(&path));
    n.unlock(PASS.into()).unwrap();
    eventually(30, "rebuilt", || hits(&n, "keeper").len() == 1);
    assert!(path.exists());
}

// KIND: acceptance (S1) — one index per account: an account switch switches it; results never
// mix accounts.
#[test]
fn account_switch_shows_only_that_account() {
    let root = Root::new("switch");
    let n = root.node();
    n.create_identity("ann".into(), PASS.into()).unwrap();
    let a = n.profile().unwrap().did;
    n.send_text(a.clone(), "alpha orchard".into(), None).unwrap();
    n.begin_new_account().unwrap();
    n.create_identity("ben".into(), "other pass".into()).unwrap();
    let b = n.profile().unwrap().did;
    n.send_text(b.clone(), "bravo orchard".into(), None).unwrap();
    assert!(hits(&n, "alpha").is_empty());
    assert_eq!(texts(&n, "orchard"), ["bravo orchard"]);
    n.switch_account(a.clone()).unwrap();
    assert!(n.search("orchard".into(), 5, None).is_err(), "locked after a switch");
    n.unlock(PASS.into()).unwrap();
    assert_eq!(texts(&n, "orchard"), ["alpha orchard"]);
    assert!(hits(&n, "bravo").is_empty());
}

// KIND: acceptance (S1) — 1:1: what A sends reaches B's index (live push); A's edit and delete
// follow; B finds A by name.
#[test]
fn messages_from_a_contact() {
    let a = peer("searcha");
    let b = peer("searchb");
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    eventually(30, "contacts", || a.node.contacts().iter().any(|c| c.did == b.did) && b.node.contacts().iter().any(|c| c.did == a.did));
    let m = a.node.send_text(b.did.clone(), "meet at the harbour gate".into(), None).unwrap();
    eventually(WAIT, "B finds it", || hits(&b.node, "harb gat").iter().any(|h| h.id == m.id));
    let h = hits(&b.node, "harbour").remove(0);
    assert_eq!((h.conv.as_str(), h.author.as_str(), h.author_name.as_str(), h.conv_name.as_str()), (a.did.as_str(), a.did.as_str(), "searcha", "searcha"));
    a.node.edit_message(b.did.clone(), m.id.clone(), "meet at the station".into()).unwrap();
    eventually(WAIT, "B sees the edit", || hits(&b.node, "harbour").is_empty() && hits(&b.node, "station").len() == 1);
    a.node.delete_message(b.did.clone(), m.id.clone()).unwrap();
    eventually(WAIT, "B sees the delete", || hits(&b.node, "station").is_empty());
    let names = b.node.search("SEARCHA".into(), 5, None).unwrap().names;
    assert!(names.iter().any(|n| n.kind == SearchNameKind::Contact && n.id == a.did));
}

// KIND: acceptance (S1) — group posts are found by members (and the group by its name); system
// lines are not indexed.
#[test]
fn group_posts_found_by_members() {
    let o = peer("searcho");
    let m = peer("searchm");
    let g = o.node.group_create("Hiking Club".into()).unwrap();
    let link = o.node.group_invite(g.clone()).unwrap();
    assert_eq!(m.node.group_join(link).unwrap(), g);
    o.node.group_post(g.clone(), "summit at dawn, bring headlamps".into(), None).unwrap();
    eventually(WAIT, "the member finds the post", || hits(&m.node, "summ head").iter().any(|h| h.kind == SearchConvKind::Group && h.conv == g));
    let h = hits(&m.node, "dawn").remove(0);
    assert_eq!((h.day.as_str(), h.author.as_str(), h.conv_name.as_str()), ("-", o.did.as_str(), "Hiking Club"));
    assert_eq!(hits(&o.node, "headlamps").len(), 1, "the author finds its own post");
    // Locating the post: the page ending at it (docs/search.md "Apps").
    let page = m.node.group_messages(g.clone(), Some(format!("{}:{}~", h.at, h.id)), 20).unwrap();
    assert_eq!(page.messages.last().map(|x| x.id.clone()), Some(h.id.clone()));
    assert!(hits(&m.node, "joined").is_empty() && hits(&m.node, "searchm").is_empty(), "no system lines");
    let names = m.node.search("hik".into(), 5, None).unwrap().names;
    assert!(names.iter().any(|n| n.kind == SearchNameKind::Group && n.id == g && n.name == "Hiking Club"));
}

// ---- tests added for the gaps: recovery, closed days, devices, groups, names, errors, budget ----

const DAY_MS: i64 = 86_400_000;

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

/// Locks and unlocks: the index is closed and opened again, and reconcile runs in the background.
fn relock(n: &Node) {
    n.lock();
    eventually(10, "locked", || n.search("x".into(), 1, None).is_err());
    n.unlock(PASS.into()).unwrap();
}

/// Locks, deletes the account's `search.redb`, unlocks (a rebuild).
fn rebuild(p: &Peer) {
    p.node.lock();
    eventually(10, "locked", || p.node.search("x".into(), 1, None).is_err());
    std::fs::remove_file(p._root.acct(&p.did).join("search.redb")).unwrap();
    p.node.unlock(PASS.into()).unwrap();
}

fn ids(n: &Node, q: &str) -> Vec<String> {
    hits(n, q).into_iter().map(|h| h.id).collect()
}

// KIND: acceptance (S1) — a crash between the chat commit and the index write: the message is
// in the chat store but not found; the next open reconciles and finds it. A later live change is
// indexed, and does not hide the missed one.
#[test]
fn crash_between_commit_and_index_write_is_healed_by_reconcile() {
    let p = peer("crash");
    let me = p.did.clone();
    p.node.send_text(me.clone(), "first quokka".into(), None).unwrap();
    p.node.search_skip_next_writes_for_test(1);
    let lost = p.node.send_text(me.clone(), "second axolotl".into(), None).unwrap();
    assert!(hits(&p.node, "axolotl").is_empty(), "the index write was skipped");
    assert!(p.node.chat_day(me.clone(), None).unwrap().messages.iter().any(|m| m.id == lost.id), "but the chat holds it");
    let live = p.node.send_text(me.clone(), "third pangolin".into(), None).unwrap();
    assert_eq!(ids(&p.node, "pangolin"), [live.id.as_str()], "later changes are indexed live");
    assert!(hits(&p.node, "axolotl").is_empty(), "the shard watermark did not move past the gap");
    relock(&p.node);
    eventually(30, "reconcile finds the missed message", || ids(&p.node, "axolotl") == [lost.id.as_str()]);
    assert_eq!(hits(&p.node, "quokka").len(), 1);
    assert_eq!(hits(&p.node, "pangolin").len(), 1);
}

// KIND: acceptance (S1) — a day compacted into a snapshot blob is read from the blob and
// indexed again after the index file is deleted. (The day is one B pulled from A: a day with
// unacknowledged ops of ours is never compacted.)
#[test]
fn closed_day_is_reindexed_from_its_snapshot() {
    let a = peer("closeda");
    let b = peer("closedb");
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    eventually(30, "contacts", || a.node.contacts().iter().any(|c| c.did == b.did) && b.node.contacts().iter().any(|c| c.did == a.did));
    let old = a.node.write_as_sibling_for_test(b.did.clone(), "ancient manuscript".into(), now_ms() - 20 * DAY_MS).unwrap();
    let fresh = a.node.send_text(b.did.clone(), "fresh manuscript".into(), None).unwrap();
    eventually(WAIT, "B has the fresh one", || ids(&b.node, "fresh") == [fresh.id.as_str()]);
    eventually(WAIT, "B pulls the old day", || {
        let _ = b.node.fetch_older_history(a.did.clone(), "9999-12-31".into());
        ids(&b.node, "ancient") == [old.as_str()]
    });
    assert_eq!(b.node.compact_now_for_test().unwrap(), 1, "the old day is closed");
    rebuild(&b);
    eventually(30, "both days indexed again", || hits(&b.node, "manuscript").len() == 2);
    let h = hits(&b.node, "ancient").remove(0);
    assert_eq!((h.id.as_str(), h.conv_name.as_str()), (old.as_str(), "closeda"));
    assert!(b.node.chat_day(a.did.clone(), Some(h.day)).unwrap().messages.iter().any(|m| m.id == old), "the hit opens its (closed) day");
}

struct Tap(Mutex<mpsc::Sender<(bool, String)>>);
impl LinkEvents for Tap {
    fn on_link_code(&self, code: String, _: String) {
        let _ = self.0.lock().unwrap().send((false, code));
    }
    fn on_link_done(&self, did: String, _: String) {
        let _ = self.0.lock().unwrap().send((true, did));
    }
    fn on_link_failed(&self, _: String) {}
    fn on_devices_changed(&self) {}
    fn on_history_changed(&self) {}
    fn on_unlinked(&self, _: String) {}
}

/// A node with a tap on its link events.
fn tapped(name: &str) -> (Root, Arc<Node>, mpsc::Receiver<(bool, String)>) {
    let root = Root::new(name);
    let (tx, rx) = mpsc::channel();
    let node = Node::new(root.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    node.set_link_events(Arc::new(Tap(Mutex::new(tx))));
    (root, node, rx)
}

/// Links a fresh install of `e`'s account; returns it started.
fn linked(e: &Peer, e_rx: &mpsc::Receiver<(bool, String)>, name: &str) -> Peer {
    let (root, node, rx) = tapped(name);
    node.link_new_scan(e.node.link_show_qr().unwrap(), "Desk".into()).unwrap();
    let next = |rx: &mpsc::Receiver<(bool, String)>, done: bool| loop {
        let (d, v) = rx.recv_timeout(Duration::from_secs(30)).expect("link event");
        if d == done {
            break v;
        }
    };
    assert_eq!(next(e_rx, false), next(&rx, false));
    e.node.link_approve(Some(PASS.into())).unwrap();
    let did = next(&rx, true);
    node.set_passphrase(None, "n-pass".into()).unwrap();
    online(&node);
    Peer { node, did, _root: root }
}

// KIND: acceptance (S1) — our other device's messages are searchable here: the "You" chat and a
// 1:1 chat, both written on the old device after linking and arriving by own-device sync.
#[test]
fn messages_from_our_other_device_are_searchable() {
    let (root, node, a_rx) = tapped("devsa");
    node.create_identity("devsa".into(), PASS.into()).unwrap();
    online(&node);
    let did = node.profile().unwrap().did;
    let a = Peer { node, did, _root: root };
    let o = peer("devso");
    a.node.add_contact(o.node.my_ticket().unwrap()).unwrap();
    eventually(30, "contacts", || o.node.contacts().iter().any(|c| c.did == a.did));
    let a2 = linked(&a, &a_rx, "devsa2");
    eventually(WAIT, "a2 knows o", || a2.node.contacts().iter().any(|c| c.did == o.did));
    let note = a.node.send_text(a.did.clone(), "note to self: marmalade".into(), None).unwrap();
    let dm = a.node.send_text(o.did.clone(), "to o: persimmon".into(), None).unwrap();
    eventually(WAIT, "a2 finds the You note", || ids(&a2.node, "marmal") == [note.id.as_str()]);
    eventually(WAIT, "a2 finds the 1:1 message", || ids(&a2.node, "persim") == [dm.id.as_str()]);
    let h = hits(&a2.node, "persim").remove(0);
    assert_eq!((h.conv.as_str(), h.author.as_str(), h.author_name.as_str()), (o.did.as_str(), a.did.as_str(), "devsa"));
    assert_eq!(hits(&a2.node, "marmal")[0].conv_name, "You");
}

// KIND: acceptance (S1) — a day pulled with `fetch_older_history` (a vouched snapshot) is indexed
// when imported.
#[test]
fn a_day_pulled_by_history_import_is_searchable() {
    let a = peer("hista");
    let b = peer("histb");
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    eventually(30, "contacts", || a.node.contacts().iter().any(|c| c.did == b.did) && b.node.contacts().iter().any(|c| c.did == a.did));
    let old = a.node.write_as_sibling_for_test(b.did.clone(), "last month: chestnut harvest".into(), now_ms() - 30 * DAY_MS).unwrap();
    assert!(hits(&b.node, "chestnut").is_empty());
    eventually(WAIT, "the day arrives", || b.node.fetch_older_history(a.did.clone(), "9999-12-31".into()).unwrap_or(0) > 0 || !hits(&b.node, "chestnut").is_empty());
    eventually(WAIT, "B finds it", || ids(&b.node, "chestnut") == [old.as_str()]);
    let h = hits(&b.node, "chestnut").remove(0);
    assert!(b.node.chat_day(a.did.clone(), Some(h.day)).unwrap().messages.iter().any(|m| m.id == old));
}

// KIND: acceptance (S1) — group posts: a stale or hidden row is fixed by reconcile (old words
// gone, the real text found; a post no longer shown removed), and a deleted index is rebuilt
// from the group store. (The product has no way to edit or delete a post; the hook writes the
// stale rows an old version or a hidden post would leave.)
#[test]
fn group_reconcile_fixes_stale_posts_and_rebuilds() {
    let o = peer("grpo");
    let g = o.node.group_create("Chess Club".into()).unwrap();
    let p1 = o.node.group_post(g.clone(), "rook endgames on friday".into(), None).unwrap();
    let p2 = o.node.group_post(g.clone(), "bring your clocks".into(), None).unwrap();
    assert_eq!(ids(&o.node, "rook"), [p1.id.as_str()]);
    o.node.search_plant_group_post_for_test(g.clone(), p1.id.clone(), "obsolete zebra".into()).unwrap();
    o.node.search_plant_group_post_for_test(g.clone(), "ghost".into(), "phantom walrus".into()).unwrap();
    assert_eq!(ids(&o.node, "zebra"), [p1.id.as_str()]);
    assert_eq!(hits(&o.node, "walrus").len(), 1);
    relock(&o.node);
    eventually(30, "reconcile restores the text", || ids(&o.node, "rook") == [p1.id.as_str()]);
    assert!(hits(&o.node, "zebra").is_empty(), "the old text is gone");
    assert!(hits(&o.node, "walrus").is_empty(), "a post that is not shown is removed");
    assert_eq!(ids(&o.node, "clocks"), [p2.id.as_str()]);
    rebuild(&o);
    eventually(30, "rebuilt from the group", || ids(&o.node, "clocks") == [p2.id.as_str()] && ids(&o.node, "rook") == [p1.id.as_str()]);
    let h = hits(&o.node, "clocks").remove(0);
    assert_eq!((h.kind, h.conv.as_str(), h.conv_name.as_str(), h.day.as_str()), (SearchConvKind::Group, g.as_str(), "Chess Club", "-"));
}

// KIND: acceptance (S1) — names: a contact, a group and "You" (by our profile name) are found by
// name on the first page only; later pages carry messages alone.
#[test]
fn names_on_the_first_page_only() {
    let a = peer("Orchard Alice");
    let b = peer("orchard-b");
    a.node.add_contact(b.node.my_ticket().unwrap()).unwrap();
    let g = a.node.group_create("Orchard Club".into()).unwrap();
    for i in 0..3 {
        a.node.send_text(a.did.clone(), format!("orchard note {i}"), None).unwrap();
        std::thread::sleep(Duration::from_millis(3));
    }
    let first = a.node.search("orchard".into(), 1, None).unwrap();
    assert_eq!(first.names.len(), 3, "{:?}", first.names);
    assert!(first.names.iter().any(|n| n.kind == SearchNameKind::Me && n.id == a.did && n.name == "You"));
    assert!(first.names.iter().any(|n| n.kind == SearchNameKind::Contact && n.id == b.did));
    assert!(first.names.iter().any(|n| n.kind == SearchNameKind::Group && n.id == g && n.name == "Orchard Club"));
    assert_eq!(first.messages.len(), 1);
    let mut cursor = first.next.clone();
    let mut total = 1;
    while let Some(c) = cursor {
        let r = a.node.search("orchard".into(), 1, Some(c)).unwrap();
        assert!(r.names.is_empty(), "names only on the first page");
        total += r.messages.len();
        cursor = r.next;
    }
    assert_eq!(total, 3);
}

// KIND: acceptance (S1) — a locked node answers `Locked`, and the kept history of someone who
// blocked us (their request removed our contact entry) is still searchable, by name too.
#[test]
fn locked_search_and_kept_history_after_a_block() {
    let a = peer("blka");
    let b = peer("blkb");
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    eventually(30, "contacts", || a.node.contacts().iter().any(|c| c.did == b.did) && b.node.contacts().iter().any(|c| c.did == a.did));
    let m = a.node.send_text(b.did.clone(), "the beacon is lit".into(), None).unwrap();
    eventually(WAIT, "B finds it", || ids(&b.node, "beacon") == [m.id.as_str()]);
    a.node.block(b.did.clone()).unwrap();
    eventually(30, "b lost the contact entry", || !b.node.contacts().iter().any(|c| c.did == a.did));
    assert_eq!(ids(&b.node, "beacon"), [m.id.as_str()], "kept history is searchable");
    assert_eq!(hits(&b.node, "beacon")[0].conv_name, "blka");
    assert!(b.node.search("blka".into(), 5, None).unwrap().names.iter().any(|n| n.id == a.did), "and found by name");
    assert_eq!(ids(&a.node, "beacon"), [m.id.as_str()], "the blocker keeps it too");
    b.node.lock();
    eventually(10, "locked", || b.node.search("beacon".into(), 5, None).is_err());
    assert!(matches!(b.node.search("beacon".into(), 5, None), Err(Error::Locked)));
}

// KIND: acceptance (S1) — the per-call posting budget: when a query hits it, `next` is set and
// the following pages continue where it stopped (every hit once, newest first), also when pages
// are limited by `limit` as well.
#[test]
fn scan_budget_pages_continue_without_gaps() {
    let p = peer("budget");
    let me = p.did.clone();
    let mut want = Vec::new();
    for i in 0..12 {
        want.push(p.node.send_text(me.clone(), format!("budget item {i}"), None).unwrap().id);
        std::thread::sleep(Duration::from_millis(3));
    }
    want.reverse();
    for (limit, budget) in [(100, 5), (3, 5), (100, 1), (4, 100)] {
        let mut seen = Vec::new();
        let mut cursor = None;
        let mut pages = 0;
        loop {
            let r = p.node.search_with_budget_for_test("budget".into(), limit, cursor, budget).unwrap();
            pages += 1;
            if pages == 1 && budget < 12 {
                assert!(r.next.is_some(), "the budget cut the first page");
                assert!(r.messages.len() <= budget as usize);
            }
            seen.extend(r.messages.iter().map(|m| m.id.clone()));
            cursor = r.next;
            if cursor.is_none() {
                break;
            }
            assert!(pages < 50, "paging must end");
        }
        assert_eq!(seen, want, "limit {limit}, budget {budget}");
    }
}
