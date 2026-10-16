//! GL: stop / start, lock / unlock, an in-process restart and an account switch, repeated, on
//! real nodes that have groups, chats and things still owed (a pending group invitation and an
//! undelivered chat message to an offline contact, so their outbox tasks are running).
//!
//! What must hold every time: the stores (`group.redb`, `chat.redb`, `account.redb`) reopen at
//! once — no open ever waits for a file a previous session still holds ("Database already
//! open") — and after `stop` / `lock` / a switch no background task opens a store again. The
//! counters come from `Node::store_stats_for_test` (opens, opens that had to wait).
//!
//! Every test carries `// KIND: acceptance (GL)`.
#![allow(dead_code)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use p2pcore::{GroupInvitationState, GroupItemKind, Node};

mod support;
use support::*;

const ROUNDS: usize = 6;

struct Peer {
    node: Arc<Node>,
    did: String,
    dir: TempDir,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.node.stop();
    }
}

fn new_node(dir: &std::path::Path) -> Arc<Node> {
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    node.auto_accept_contacts_for_test(true);
    node.set_link_events(Arc::new(NoLink));
    node
}

fn peer(name: &str) -> Peer {
    let dir = TempDir::new("gl", name);
    let node = new_node(&dir);
    node.create_identity(name.into(), PASS.into()).unwrap();
    online(&node);
    let did = node.profile().unwrap().did;
    Peer { node, did, dir }
}

fn contacts(a: &Peer, b: &Peer) {
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    eventually(30, "both are contacts", || a.node.contacts().iter().any(|c| c.did == b.did) && b.node.contacts().iter().any(|c| c.did == a.did));
}

fn has_post(n: &Node, g: &str, did: &str, text: &str) -> bool {
    n.group_messages(g.into(), None, 500)
        .map(|p| p.messages.iter().any(|m| m.kind == GroupItemKind::Post && m.author_did == did && m.text == text))
        .unwrap_or(false)
}

fn has_chat(n: &Node, peer: &str, text: &str) -> bool {
    n.chat_day(peer.into(), None).map(|d| d.messages.iter().any(|m| m.text == text)).unwrap_or(false)
}

/// `o` hosts a group `a` is in, has chatted with `a`, and owes the offline `c` an invitation
/// and a chat message (both outboxes are retrying). Returns the group id.
fn busy(o: &Peer, a: &Peer, c: &Peer) -> String {
    contacts(o, a);
    contacts(o, c);
    let g = o.node.group_create("Lifecycle".into()).unwrap();
    let link = o.node.group_invite(g.clone()).unwrap();
    assert_eq!(a.node.group_join(link).unwrap(), g);
    o.node.send_text(a.did.clone(), "hello a".into(), None).unwrap();
    eventually(WAIT, "a has o's chat message", || has_chat(&a.node, &o.did, "hello a"));
    c.node.stop();
    std::thread::sleep(Duration::from_secs(1));
    o.node.group_invite_contacts(g.clone(), vec![c.did.clone()]).unwrap();
    o.node.send_text(c.did.clone(), "for c, later".into(), None).unwrap();
    g
}

/// No background task opens a store while the node is stopped / locked: the open count stays
/// where it was for `secs`.
fn no_reopen(n: &Node, secs: u64, what: &str) {
    let (opens, _) = n.store_stats_for_test();
    std::thread::sleep(Duration::from_secs(secs));
    let (now, _) = n.store_stats_for_test();
    assert_eq!(now, opens, "{what}: a store was opened again while stopped");
}

fn no_waits(n: &Node, what: &str) {
    let (_, waits) = n.store_stats_for_test();
    assert_eq!(waits, 0, "{what}: a store open had to wait for a file still held");
}

/// Everything still works after the cycles: the group, the chat, and both outboxes.
fn still_works(o: &Peer, a: &Peer, c: &Peer, g: &str, tag: &str) {
    let mine = format!("o after {tag}");
    o.node.group_post(g.into(), mine.clone(), None).unwrap();
    eventually(WAIT, "a gets o's post", || has_post(&a.node, g, &o.did, &mine));
    let theirs = format!("a after {tag}");
    a.node.group_post(g.into(), theirs.clone(), None).unwrap();
    eventually(WAIT, "o gets a's post", || has_post(&o.node, g, &a.did, &theirs));
    let chat = format!("chat after {tag}");
    o.node.send_text(a.did.clone(), chat.clone(), None).unwrap();
    eventually(WAIT, "a gets o's chat", || has_chat(&a.node, &o.did, &chat));
    online(&c.node);
    eventually(WAIT, "c gets the owed invitation", || {
        c.node.group_invitations_incoming().unwrap().iter().any(|i| i.group_id == g && i.state == GroupInvitationState::Pending)
    });
    eventually(WAIT, "c gets the owed chat message", || has_chat(&c.node, &o.did, "for c, later"));
}

// KIND: acceptance (GL) — stop, then start at once, repeatedly, on a node with groups, chats and
// running outboxes: the group store reopens without waiting, nothing reopens it while stopped.
#[test]
fn stop_start_cycles_release_and_reopen_the_stores_at_once() {
    let o = peer("ss-o");
    let a = peer("ss-a");
    let c = peer("ss-c");
    let g = busy(&o, &a, &c);
    for i in 0..ROUNDS {
        o.node.stop();
        no_reopen(&o.node, if i == 0 { 3 } else { 1 }, "after stop");
        online(&o.node);
        assert!(o.node.groups().unwrap().iter().any(|s| s.id == g), "round {i}: the group is back");
        assert!(has_chat(&o.node, &a.did, "hello a"), "round {i}: the chat is there");
        no_waits(&o.node, &format!("stop/start round {i}"));
    }
    // Stop immediately followed by start, no pause at all.
    for i in 0..ROUNDS {
        o.node.stop();
        online(&o.node);
        assert!(o.node.groups().unwrap().iter().any(|s| s.id == g), "fast round {i}: the group is back");
        no_waits(&o.node, &format!("fast stop/start round {i}"));
    }
    eventually(WAIT, "the group endpoint is back up", || o.node.group_endpoint_id().is_some());
    still_works(&o, &a, &c, &g, "stop/start");
    no_waits(&o.node, "stop/start, end");
}

// KIND: acceptance (GL) — lock, then unlock and start, repeatedly: every store reopens without
// waiting; nothing opens one while locked.
#[test]
fn lock_unlock_cycles_release_and_reopen_the_stores_at_once() {
    let o = peer("lu-o");
    let a = peer("lu-a");
    let c = peer("lu-c");
    let g = busy(&o, &a, &c);
    for i in 0..ROUNDS {
        o.node.lock();
        no_reopen(&o.node, 1, "after lock");
        o.node.unlock(PASS.into()).unwrap();
        online(&o.node);
        assert!(o.node.groups().unwrap().iter().any(|s| s.id == g), "round {i}: the group is back");
        assert!(has_chat(&o.node, &a.did, "hello a"), "round {i}: the chat is back");
        no_waits(&o.node, &format!("lock/unlock round {i}"));
    }
    eventually(WAIT, "the group endpoint is back up", || o.node.group_endpoint_id().is_some());
    still_works(&o, &a, &c, &g, "lock/unlock");
    no_waits(&o.node, "lock/unlock, end");
}

// KIND: acceptance (GL) — an in-process restart (the app drops its node and makes a new one on the
// same directory), repeatedly: the new node opens every store at once.
#[test]
fn in_process_restarts_open_every_store_at_once() {
    let mut o = peer("rs-o");
    let a = peer("rs-a");
    let c = peer("rs-c");
    let g = busy(&o, &a, &c);
    for i in 0..ROUNDS {
        if i % 2 == 0 {
            o.node.stop();
        }
        // Dropped without waiting (odd rounds without even a stop): the drop releases the files.
        let old = std::mem::replace(&mut o.node, Arc::clone(&a.node));
        drop(old);
        let node = new_node(&o.dir);
        o.node = node;
        o.node.unlock(PASS.into()).unwrap();
        online(&o.node);
        assert!(o.node.groups().unwrap().iter().any(|s| s.id == g), "round {i}: the group is back");
        assert!(has_chat(&o.node, &a.did, "hello a"), "round {i}: the chat is back");
        no_waits(&o.node, &format!("restart round {i}"));
    }
    eventually(WAIT, "the group endpoint is back up", || o.node.group_endpoint_id().is_some());
    still_works(&o, &a, &c, &g, "restarts");
    no_waits(&o.node, "restarts, end");
}

// KIND: acceptance (GL) — switching between two accounts of one install, repeatedly: each
// account's stores reopen at once, nothing of the account switched away from is reopened, and
// each sees only its own groups.
#[test]
fn account_switch_cycles_release_and_reopen_the_stores_at_once() {
    let o = peer("sw-o");
    let a = peer("sw-a");
    let c = peer("sw-c");
    let g = busy(&o, &a, &c);
    let first = o.did.clone();
    o.node.begin_new_account().unwrap();
    o.node.create_identity("sw-o2".into(), PASS.into()).unwrap();
    online(&o.node);
    let second = o.node.profile().unwrap().did;
    let g2 = o.node.group_create("Second account".into()).unwrap();
    for i in 0..ROUNDS {
        o.node.switch_account(first.clone()).unwrap();
        no_reopen(&o.node, 1, "after switching away from the second account");
        o.node.unlock(PASS.into()).unwrap();
        online(&o.node);
        let ids: Vec<String> = o.node.groups().unwrap().into_iter().map(|s| s.id).collect();
        assert!(ids.contains(&g) && !ids.contains(&g2), "round {i}: the first account's groups only");
        assert!(has_chat(&o.node, &a.did, "hello a"), "round {i}: the first account's chat");
        no_waits(&o.node, &format!("switch round {i} (first)"));

        o.node.switch_account(second.clone()).unwrap();
        no_reopen(&o.node, 1, "after switching away from the first account");
        o.node.unlock(PASS.into()).unwrap();
        online(&o.node);
        let ids: Vec<String> = o.node.groups().unwrap().into_iter().map(|s| s.id).collect();
        assert!(ids.contains(&g2) && !ids.contains(&g), "round {i}: the second account's groups only");
        no_waits(&o.node, &format!("switch round {i} (second)"));
    }
    o.node.switch_account(first).unwrap();
    o.node.unlock(PASS.into()).unwrap();
    online(&o.node);
    eventually(WAIT, "the group endpoint is back up", || o.node.group_endpoint_id().is_some());
    still_works(&o, &a, &c, &g, "switches");
    no_waits(&o.node, "switches, end");
}

// ---- transient network failures in joins ------------------------------------------------------

// KIND: acceptance (GL) — fetching an invite (peek, join) rides out lost connections inside its
// own deadline; a refusal is not retried.
#[test]
fn peek_and_join_ride_out_lost_connections() {
    let o = peer("tr-o");
    let a = peer("tr-a");
    let g = o.node.group_create("Flaky".into()).unwrap();
    let link = o.node.group_invite(g.clone()).unwrap();

    a.node.group_fail_next_fetches_for_test(2);
    let peek = a.node.group_peek_invite(link.clone()).expect("peek after two lost connections");
    assert_eq!(peek.group_id, g);

    a.node.group_fail_next_fetches_for_test(2);
    a.node.group_fail_next_dials_for_test(5);
    let t = Instant::now();
    assert_eq!(a.node.group_join(link.clone()).expect("join after lost connections"), g);
    assert!(t.elapsed() < Duration::from_secs(60), "within the join deadline");

    // A refusal (a revoked link: the host no longer knows its code) comes back at once, not
    // retried.
    let c = peer("tr-c");
    let revoked = o.node.group_invite(g.clone()).unwrap();
    o.node.group_revoke_invite(revoked.clone()).unwrap();
    let t = Instant::now();
    match c.node.group_join(revoked) {
        Err(p2pcore::Error::Rejected(_)) => {}
        other => panic!("a revoked link: {other:?}"),
    }
    assert!(t.elapsed() < Duration::from_secs(15), "not retried: {:?}", t.elapsed());
}

// KIND: acceptance (GL) — accepting a direct invitation rides out failed dials to the host
// (the join keeps dialling at a short interval while the user waits, not at the slow backoff).
#[test]
fn accepting_an_invitation_rides_out_failed_dials() {
    let o = peer("ta-o");
    let b = peer("ta-b");
    contacts(&o, &b);
    let g = o.node.group_create("Flaky too".into()).unwrap();
    o.node.group_invite_contacts(g.clone(), vec![b.did.clone()]).unwrap();
    let inv = || b.node.group_invitations_incoming().unwrap().into_iter().find(|i| i.group_id == g);
    eventually(WAIT, "the invitation arrives", || inv().is_some());
    b.node.group_fail_next_dials_for_test(5);
    let t = Instant::now();
    let r = b.node.group_invitation_accept(inv().unwrap().id).expect("accept after failed dials");
    assert_eq!(r.state, GroupInvitationState::Joined);
    assert!(t.elapsed() < Duration::from_secs(45), "joined well inside the deadline: {:?}", t.elapsed());
}

// KIND: acceptance (D11; was `rotation_acceptance.rs`) — rotation is periodic and never implicit:
// a restart keeps the private endpoint (the device key), the DID and the writer ID, and the
// contact keeps reaching us. Private-endpoint rotation is the next packet (`docs/rotation.md`).
#[test]
fn endpoint_is_stable_without_a_due_rotation() {
    let (mut a, b) = support::Peer::pair("r1a-rot", "stable");
    let (dev, wid) = (a.node.device_key_for_test().unwrap(), a.node.writer_id_for_test());
    assert_eq!(b.node.contact_devices_for_test(a.did.clone()), vec![dev.clone()]);
    a.reopen();
    // As in writer_id.rs: `chat_open` does not retry while the old tasks still hold chat.redb.
    std::thread::sleep(Duration::from_secs(2));
    a.node.unlock(a.pass.clone()).unwrap();
    online(&a.node);
    assert_eq!(a.node.device_key_for_test().unwrap(), dev);
    assert_eq!(a.node.profile().unwrap().did, a.did);
    eventually_200ms(20, "chat store open", || a.node.writer_id_for_test().is_some());
    assert_eq!(a.node.writer_id_for_test(), wid);
    let m = b.node.send_text(a.did.clone(), "after restart".into(), None).unwrap().id;
    eventually_200ms(60, "it arrives", || holds(&a, &b.did, &m));
    eventually_200ms(30, "it is delivered", || delivered(&b, &a.did, &m));
}
