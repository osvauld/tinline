//! S1: local search (`docs/search.md`) on real nodes. The "You" conversation needs no network;
//! the 1:1 and group tests run two nodes over real iroh endpoints like `lifecycle.rs`.
//!
//! Every test carries `// KIND: acceptance (S1)`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, LinkEvents, MessageHit, Node, NodeEvents, NodeStatus, SearchConvKind, SearchNameKind};

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
