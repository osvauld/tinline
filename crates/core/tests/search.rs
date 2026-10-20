//! S1: local search (`docs/search.md`) on real nodes. The "You" conversation needs no network;
//! the 1:1 and group tests run two nodes over real iroh endpoints like `lifecycle.rs`.
//!
//! Every test carries `// KIND: acceptance (S1)`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use p2pcore::{Error, LinkEvents, MessageHit, Node, SearchConvKind, SearchNameKind};

mod support;
use support::*;
use support::now_ms_i64 as now_ms;

struct Root(TempDir);
impl Root {
    fn new(tag: &str) -> Root {
        Root(TempDir::created("search", tag))
    }
    fn node(&self) -> Arc<Node> {
        let n = Node::new(self.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
        n.auto_accept_contacts_for_test(true);
        n.set_link_events(Arc::new(NoLink));
        n
    }
    fn acct(&self, did: &str) -> PathBuf {
        self.0.join("accounts").join(did.strip_prefix("did:key:").unwrap())
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

fn peer(name: &str) -> Peer {
    let root = Root::new(name);
    let node = root.node();
    node.create_identity(name.into(), PASS.into()).unwrap();
    online(&node);
    let did = node.profile().unwrap().did;
    Peer { node, did, _root: root }
}

/// The UTC day (`YYYY-MM-DD`) of a unix-millisecond time.
fn utc_day(ms: u64) -> String {
    let z = (ms / 86_400_000) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
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
    // A group hit carries the UTC day of its day doc (the post's time).
    assert_eq!((h.day.as_str(), h.author.as_str(), h.conv_name.as_str()), (utc_day(h.at).as_str(), o.did.as_str(), "Hiking Club"));
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
    node.auto_accept_contacts_for_test(true);
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
    // The planted rows live in the hot tier (memory), so a reconcile now (not a reopen, which
    // would drop them) has to fix them.
    o.node.search_reconcile_now_for_test().unwrap();
    eventually(30, "reconcile restores the text", || ids(&o.node, "rook") == [p1.id.as_str()]);
    assert!(hits(&o.node, "zebra").is_empty(), "the old text is gone");
    assert!(hits(&o.node, "walrus").is_empty(), "a post that is not shown is removed");
    assert_eq!(ids(&o.node, "clocks"), [p2.id.as_str()]);
    rebuild(&o);
    eventually(30, "rebuilt from the group", || ids(&o.node, "clocks") == [p2.id.as_str()] && ids(&o.node, "rook") == [p1.id.as_str()]);
    let h = hits(&o.node, "clocks").remove(0);
    assert_eq!((h.kind, h.conv.as_str(), h.conv_name.as_str(), h.day.as_str()), (SearchConvKind::Group, g.as_str(), "Chess Club", utc_day(h.at).as_str()));
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

// ---- index v2: hot tier + segments ----------------------------------------------------------

/// Sends `text` to ourselves stamped `days` days in the past.
fn send_old(n: &Node, me: &str, text: &str, days: i64) -> String {
    n.chat_clock_offset_for_test(-days * DAY_MS);
    let id = n.send_text(me.to_string(), text.to_string(), None).unwrap().id;
    n.chat_clock_offset_for_test(0);
    id
}

// KIND: acceptance (v2) — a day that is no longer hot is written into a segment (memory empties)
// and still found, newest first with the hot ones; an edit and a delete of the old (segmented)
// message are seen at once, and a lost memory tier heals on reopen.
#[test]
fn cold_day_goes_to_a_segment_and_edits_still_show() {
    let p = peer("v2cold");
    let me = p.did.clone();
    let old = send_old(&p.node, &me, "ancient manuscript", 20);
    let older = send_old(&p.node, &me, "older manuscript", 40);
    let fresh = p.node.send_text(me.clone(), "fresh manuscript".into(), None).unwrap().id;
    assert_eq!(ids(&p.node, "manuscript"), [fresh.as_str(), old.as_str(), older.as_str()]);
    let (hot, segs, seg_docs) = p.node.search_maintain_for_test().unwrap();
    assert!(segs >= 1 && seg_docs == 2, "two cold docs in segments: {segs} {seg_docs}");
    assert_eq!(hot, 1, "the fresh one stays in memory");
    assert_eq!(ids(&p.node, "manuscript"), [fresh.as_str(), old.as_str(), older.as_str()]);
    assert_eq!(hits(&p.node, "ancient")[0].snippet, "ancient manuscript");

    // Edit the segmented message: its day comes back into memory, the answer changes at once.
    p.node.edit_message(me.clone(), old.clone(), "ancient parchment".into()).unwrap();
    assert!(hits(&p.node, "ancient manuscript").is_empty());
    assert_eq!(ids(&p.node, "parchment"), [old.as_str()]);
    assert_eq!(ids(&p.node, "manuscript"), [fresh.as_str(), older.as_str()]);
    // A crash before the month was rewritten: memory is lost, the file still has the old text;
    // the reopen sees the stale watermark of that day and reconciles it.
    relock(&p.node);
    eventually(30, "the edit is back", || ids(&p.node, "parchment") == [old.as_str()] && hits(&p.node, "ancient manuscript").is_empty());
    assert_eq!(ids(&p.node, "manuscript"), [fresh.as_str(), older.as_str()]);
    // Delete the other old one; rewrite; the answers stay.
    p.node.delete_message(me.clone(), older.clone()).unwrap();
    assert_eq!(ids(&p.node, "manuscript"), [fresh.as_str()]);
    let (hot, _, seg_docs) = p.node.search_maintain_for_test().unwrap();
    assert_eq!((hot, seg_docs), (1, 1), "the deleted one is gone from the rewritten month");
    assert_eq!(ids(&p.node, "manuscript"), [fresh.as_str()]);
    assert_eq!(ids(&p.node, "parchment"), [old.as_str()]);
}

// KIND: acceptance (v2) — a late group post into a day that is already in a segment is found at
// once (and with the day's other posts), and the segment is rewritten on request.
#[test]
fn late_group_post_into_a_cold_day() {
    let o = peer("v2late");
    let g = o.node.group_create("Gardeners".into()).unwrap();
    let at = now_ms() - 30 * DAY_MS;
    let early = o.node.group_post_at_for_test(g.clone(), at, "early daffodil bulbs".into()).unwrap();
    o.node.search_maintain_for_test().unwrap();
    let (_, segs, seg_docs) = o.node.search_maintain_for_test().unwrap();
    assert!(segs >= 1 && seg_docs >= 1);
    assert_eq!(ids(&o.node, "daffodil"), [early.id.as_str()]);
    let late = o.node.group_post_at_for_test(g.clone(), at + 60_000, "late tulip bulbs".into()).unwrap();
    assert_eq!(ids(&o.node, "bulbs"), [late.id.as_str(), early.id.as_str()]);
    assert_eq!(ids(&o.node, "tulip"), [late.id.as_str()]);
    let (hot, _, seg_docs) = o.node.search_maintain_for_test().unwrap();
    assert_eq!((hot, seg_docs), (0, 2));
    assert_eq!(ids(&o.node, "bulbs"), [late.id.as_str(), early.id.as_str()]);
    // After a reopen the same, from the file alone.
    relock(&o.node);
    eventually(30, "reopened", || ids(&o.node, "bulbs") == [late.id.as_str(), early.id.as_str()]);
}

// KIND: acceptance (v2) — paging newest first across the hot tier and several segments, with the
// tiers changing between pages: every doc once, none skipped; a small SEG_MAX_DOCS splits the
// old days over several segments.
#[test]
fn paging_across_tiers_with_a_tier_change_between_pages() {
    let p = peer("v2page");
    let me = p.did.clone();
    p.node.search_set_caps_for_test(1000, 4).unwrap();
    let mut want = Vec::new();
    for d in (9..=24).rev() {
        for k in 0..2 {
            want.push(send_old(&p.node, &me, &format!("standup report {d}-{k}"), d));
        }
    }
    for i in 0..3 {
        want.push(p.node.send_text(me.clone(), format!("standup report now {i}"), None).unwrap().id);
        std::thread::sleep(Duration::from_millis(3));
    }
    // Oldest sent first; the result is newest first by time.
    let mut by_time: Vec<(u64, String)> = hits(&p.node, "standup").into_iter().map(|h| (h.at, h.id)).collect();
    by_time.sort();
    by_time.reverse();
    let want_ids: Vec<String> = by_time.into_iter().map(|x| x.1).collect();
    assert_eq!(want_ids.len(), want.len());
    assert_eq!(ids(&p.node, "standup"), want_ids, "all in memory: newest first");
    let mut seen = Vec::new();
    let mut cursor = None;
    let mut step = 0;
    loop {
        let r = p.node.search("standup".into(), 5, cursor).unwrap();
        seen.extend(r.messages.iter().map(|m| m.id.clone()));
        // The tiers change between pages: first the cold days go into segments, then an old
        // message is edited (its day comes back into memory), then the segments are rewritten.
        match step {
            0 => {
                let (_, segs, _) = p.node.search_maintain_for_test().unwrap();
                assert!(segs >= 3, "SEG_MAX_DOCS 4 splits the month over several segments: {segs}");
            }
            1 => {
                p.node.edit_message(me.clone(), want[0].clone(), "standup report edited".into()).unwrap();
            }
            _ => {
                p.node.search_maintain_for_test().unwrap();
            }
        }
        step += 1;
        match r.next {
            Some(c) => cursor = Some(c),
            None => break,
        }
        assert!(step < 40);
    }
    assert_eq!(seen, want_ids, "each once, newest first, whatever the tier");
}

// KIND: acceptance (v2) — a file in the v1 layout is emptied and rebuilt in the background; a
// locked node answers Locked and keeps no memory tier.
#[test]
fn v1_file_is_rebuilt_and_lock_drops_memory() {
    let root = Root::new("v2v1");
    let n = root.node();
    n.create_identity("vera".into(), PASS.into()).unwrap();
    let me = n.profile().unwrap().did;
    n.send_text(me.clone(), "the lighthouse keeper".into(), None).unwrap();
    send_old(&n, &me, "old harbour light", 30);
    assert_eq!(hits(&n, "keeper").len(), 1);
    n.lock();
    eventually(10, "locked", || n.search("keeper".into(), 5, None).is_err());
    assert!(matches!(n.search_maintain_for_test(), Err(Error::Locked)), "no index, no memory tier while locked");
    // Replace the file with one in the v1 layout (a version row of 1 and per-message keys).
    let path = root.acct(&me).join("search.redb");
    std::fs::remove_file(&path).unwrap();
    let v1 = storage::Store::open(&path).unwrap();
    v1.put("v", b"not-a-sealed-v2-value").unwrap();
    v1.put("d/0123456789abcdef", b"old row").unwrap();
    v1.put("t/00112233445566778899aabbccddeeff/ffffffffffffffff/0123456789abcdef", b"").unwrap();
    v1.close();
    drop(v1);
    n.unlock(PASS.into()).unwrap();
    eventually(30, "rebuilt", || hits(&n, "keeper").len() == 1 && hits(&n, "harbour").len() == 1);
    n.lock();
    eventually(10, "locked", || n.search("keeper".into(), 5, None).is_err());
    let after = storage::Store::open(&path).unwrap();
    assert!(after.get("d/0123456789abcdef").unwrap().is_none(), "the v1 rows are gone");
    assert!(after.get("m").unwrap().is_some(), "and the rebuild wrote a manifest");
    after.close();
}

// ---- find in chat ---------------------------------------------------------------------------

fn find(n: &Node, kind: SearchConvKind, conv: &str, q: &str) -> Vec<MessageHit> {
    n.search_in(kind, conv.into(), q.into(), 100, None).unwrap().messages
}

fn find_ids(n: &Node, kind: SearchConvKind, conv: &str, q: &str) -> Vec<String> {
    find(n, kind, conv, q).into_iter().map(|h| h.id).collect()
}

// KIND: acceptance (find in chat) — a DM and a group: each answers with its own hits only,
// newest first, `names` empty; paging by cursor runs across the hot tier and segments (a tier
// change between pages) without repeats or gaps.
#[test]
fn find_in_chat_dm_and_group_only_their_hits_paged() {
    let p = peer("findpage");
    let me = p.did.clone();
    let g = p.node.group_create("Harbour Crew".into()).unwrap();
    p.node.search_set_caps_for_test(1000, 3).unwrap();
    let now = now_ms();
    let (mut mine, mut theirs) = (Vec::new(), Vec::new());
    for d in [60i64, 45, 30, 20, 12, 9] {
        mine.push(send_old(&p.node, &me, &format!("lantern note {d}"), d));
        theirs.push(p.node.group_post_at_for_test(g.clone(), now - d * DAY_MS + 5_000, format!("lantern post {d}")).unwrap().id);
    }
    for i in 0..3 {
        mine.push(p.node.send_text(me.clone(), format!("lantern note fresh {i}"), None).unwrap().id);
        std::thread::sleep(Duration::from_millis(3));
        theirs.push(p.node.group_post_at_for_test(g.clone(), now_ms() + i, format!("lantern post fresh {i}")).unwrap().id);
    }
    // Two tiers: the old days into segments (three docs at most each), the fresh ones in memory.
    p.node.search_maintain_for_test().unwrap();
    mine.reverse();
    theirs.reverse();
    let r = p.node.search_in(SearchConvKind::Dm, me.clone(), "lant".into(), 100, None).unwrap();
    assert!(r.names.is_empty() && r.next.is_none());
    assert_eq!(r.messages.iter().map(|h| h.id.clone()).collect::<Vec<_>>(), mine, "DM: its hits only, newest first");
    assert!(r.messages.iter().all(|h| h.kind == SearchConvKind::Dm && h.conv == me));
    let r = p.node.search_in(SearchConvKind::Group, g.clone(), "lant".into(), 100, None).unwrap();
    assert_eq!(r.messages.iter().map(|h| h.id.clone()).collect::<Vec<_>>(), theirs, "group: its hits only");
    assert!(r.messages.iter().all(|h| h.kind == SearchConvKind::Group && h.conv == g && h.conv_name == "Harbour Crew"));
    assert_eq!(highlighted(&r.messages[0]), ["lant"]);
    assert_eq!(hits(&p.node, "lantern").len(), mine.len() + theirs.len(), "the global search sees both");
    // Pages of 4 over both tiers, with a tier change in the middle (an old message is edited).
    for (kind, conv, want) in [(SearchConvKind::Dm, me.clone(), mine.clone()), (SearchConvKind::Group, g.clone(), theirs.clone())] {
        let (mut seen, mut cursor, mut pages) = (Vec::new(), None, 0);
        loop {
            let r = p.node.search_in(kind, conv.clone(), "lantern".into(), 4, cursor).unwrap();
            seen.extend(r.messages.iter().map(|h| h.id.clone()));
            pages += 1;
            if pages == 2 && kind == SearchConvKind::Dm {
                p.node.edit_message(me.clone(), want[want.len() - 1].clone(), "lantern note edited".into()).unwrap();
            }
            cursor = r.next;
            if cursor.is_none() {
                break;
            }
            assert!(pages < 20);
        }
        assert_eq!(seen, want, "{kind:?}: each once, newest first");
        assert!(pages >= 3);
    }
}

// KIND: acceptance (find in chat) — a segment without the conversation is not opened; the doc
// table drops a busy chat's postings (counted) so that a quiet chat amid a common prefix finds
// all its hits in one call; the drop budget ends a page with a cursor that continues without
// gaps.
#[test]
fn find_in_chat_skips_segments_and_budgets_are_split() {
    let p = peer("findbudget");
    let me = p.did.clone();
    let busy = p.node.group_create("Busy".into()).unwrap();
    let quiet = p.node.group_create("Quiet".into()).unwrap();
    let base = now_ms();
    // Two months: the busy group alone in the older one; the quiet chat amid it in the newer.
    for i in 0..40 {
        p.node.group_post_at_for_test(busy.clone(), base - 90 * DAY_MS + i * 1000, format!("common prefix chatter old {i}")).unwrap();
    }
    let mut want = Vec::new();
    for i in 0..400 {
        p.node.group_post_at_for_test(busy.clone(), base - 20 * DAY_MS + i * 1000, format!("common prefix chatter {i}")).unwrap();
        if i % 100 == 50 {
            want.push(p.node.group_post_at_for_test(quiet.clone(), base - 20 * DAY_MS + i * 1000 + 500, format!("common quiet words {i}")).unwrap().id);
        }
    }
    let mine = send_old(&p.node, &me, "common private words", 20);
    want.reverse();
    let (_, segs, _) = p.node.search_maintain_for_test().unwrap();
    assert!(segs >= 2, "{segs}");
    // One call, all of the quiet chat's hits, though 400+ postings of the busy chat share the prefix.
    let (r, opened, dropped) = p.node.search_in_with_budget_for_test(SearchConvKind::Group, quiet.clone(), "common".into(), 100, None, 20_000, 200_000).unwrap();
    assert_eq!(r.messages.iter().map(|h| h.id.clone()).collect::<Vec<_>>(), want);
    assert!(r.next.is_none());
    assert_eq!(opened, 1, "the older month's segment has no day of this group");
    assert!(dropped >= 400, "the busy chat's postings were dropped by the doc table: {dropped}");
    // Cheap enough that a small own budget is not touched by the others' postings.
    let (r, _, _) = p.node.search_in_with_budget_for_test(SearchConvKind::Group, quiet.clone(), "common".into(), 100, None, 5, 200_000).unwrap();
    assert_eq!(r.messages.len(), 4);
    // A drop budget of 100 cuts the walk; pages continue to the same result.
    let (mut seen, mut cursor, mut pages) = (Vec::new(), None, 0);
    loop {
        let (r, _, _) = p.node.search_in_with_budget_for_test(SearchConvKind::Group, quiet.clone(), "common".into(), 100, cursor, 20_000, 100).unwrap();
        pages += 1;
        seen.extend(r.messages.iter().map(|h| h.id.clone()));
        cursor = r.next;
        if cursor.is_none() {
            break;
        }
        assert!(pages < 30);
    }
    assert_eq!(seen, want);
    assert!(pages >= 2, "the drop budget cut the first page: {pages}");
    assert_eq!(find_ids(&p.node, SearchConvKind::Dm, &me, "common"), [mine]);
}

// KIND: acceptance (find in chat) — two nodes: a contact's DM is found by the other side only
// in that conversation (our own "You" hit with the same word is not mixed in); a conversation
// the chat list does not show, a group not held and an empty query return nothing; Locked while
// locked.
#[test]
fn find_in_chat_contact_hidden_and_locked() {
    let a = peer("findhida");
    let b = peer("findhidb");
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    eventually(30, "contacts", || a.node.contacts().iter().any(|c| c.did == b.did) && b.node.contacts().iter().any(|c| c.did == a.did));
    let m = a.node.send_text(b.did.clone(), "the beacon is lit".into(), None).unwrap();
    let mine = b.node.send_text(b.did.clone(), "beacon notes for later".into(), None).unwrap();
    eventually(WAIT, "B finds it", || hits(&b.node, "beacon").len() == 2);
    assert_eq!(find_ids(&b.node, SearchConvKind::Dm, &a.did, "beacon"), [m.id.as_str()]);
    assert_eq!(find_ids(&b.node, SearchConvKind::Dm, &b.did, "beacon"), [mine.id.as_str()]);
    // Not shown in B's chat list (a stranger), a group B does not hold, and the wrong kind.
    let stranger = peer("findstranger");
    stranger.node.send_text(stranger.did.clone(), "beacon of the stranger".into(), None).unwrap();
    assert!(find(&b.node, SearchConvKind::Dm, &stranger.did, "beacon").is_empty());
    let g = a.node.group_create("Private".into()).unwrap();
    a.node.group_post(g.clone(), "beacon in a private group".into(), None).unwrap();
    assert!(find(&b.node, SearchConvKind::Group, &g, "beacon").is_empty());
    assert_eq!(find(&a.node, SearchConvKind::Group, &g, "beacon").len(), 1);
    assert!(find(&b.node, SearchConvKind::Group, &a.did, "beacon").is_empty(), "a DID is no group");
    assert!(find(&b.node, SearchConvKind::Dm, &a.did, "").is_empty());
    assert!(matches!(b.node.search_in(SearchConvKind::Dm, a.did.clone(), "beacon".into(), 5, Some("zz".into())), Err(Error::Protocol(_))));
    b.node.lock();
    eventually(10, "locked", || b.node.search("beacon".into(), 5, None).is_err());
    assert!(matches!(b.node.search_in(SearchConvKind::Dm, a.did.clone(), "beacon".into(), 5, None), Err(Error::Locked)));
}
