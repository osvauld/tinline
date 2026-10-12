//! 1:1 chat day shards and live delivery, on real nodes: the UTC day boundary, the live and
//! day windows, a contact offline for more than the 7 days of a hello, closed (compacted) days,
//! the shard cache (12 days), and how fast and how completely messages arrive.
//!
//! Real iroh endpoints (needs network, like `old_writer_delivery.rs`; the helpers are copied
//! from there, the tests share no module). Time is injected per node with
//! `chat_clock_offset_for_test` (messages get that `at`; the live window, hello days and
//! compaction use it), and `write_as_sibling_for_test` / `write_in_day_for_test` for ops filed
//! under another writer ID or day.

#![allow(dead_code)]

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, ChatEvents, Chat, DeliveryState, LinkEvents, Message, Node, NodeEvents, NodeStatus};

const PASS: &str = "test-passphrase";

#[derive(Debug, Clone)]
enum Ev {
    Incoming(CallInfo),
    #[allow(dead_code)]
    State(String, CallState),
}

struct Tap(Mutex<mpsc::Sender<Ev>>, Arc<Mutex<Vec<String>>>);
impl NodeEvents for Tap {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, c: CallInfo) {
        let _ = self.0.lock().unwrap().send(Ev::Incoming(c));
    }
    fn on_call_state(&self, id: String, s: CallState) {
        let _ = self.0.lock().unwrap().send(Ev::State(id, s));
    }
    fn on_log(&self, line: String) {
        self.1.lock().unwrap().push(line);
    }
}

#[derive(Debug, Clone, PartialEq)]
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
    fn on_devices_changed(&self) {
        let _ = self.0.lock().unwrap().send(LEv::Other);
    }
    fn on_history_changed(&self) {
        let _ = self.0.lock().unwrap().send(LEv::Other);
    }
    fn on_unlinked(&self, _: String) {
        let _ = self.0.lock().unwrap().send(LEv::Other);
    }
}

struct Peer {
    node: Arc<Node>,
    rx: mpsc::Receiver<Ev>,
    lrx: mpsc::Receiver<LEv>,
    logs: Arc<Mutex<Vec<String>>>,
    did: String,
    phrase: String,
    dir: std::path::PathBuf,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.node.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Peer {
    fn logged(&self, needle: &str) -> bool {
        self.logs.lock().unwrap().iter().any(|l| l.contains(needle))
    }

    /// The log lines about refused batches.
    fn refusals(&self) -> Vec<String> {
        self.logs.lock().unwrap().iter().filter(|l| l.contains("rejected ")).cloned().collect()
    }

    /// Whether this node refused any chat batch, history snapshot or own-device batch.
    fn refused_chat(&self) -> bool {
        ["rejected chat batch", "rejected history", "rejected own-device batch"].iter().any(|n| self.logged(n))
    }
}

type Opened = (Arc<Node>, mpsc::Receiver<Ev>, mpsc::Receiver<LEv>, Arc<Mutex<Vec<String>>>);

fn open(dir: &std::path::Path) -> Opened {
    let (tx, rx) = mpsc::channel();
    let (ltx, lrx) = mpsc::channel();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Tap(Mutex::new(tx), logs.clone()))).unwrap();
    node.set_link_events(Arc::new(LTap(Mutex::new(ltx))));
    (node, rx, lrx, logs)
}

fn blank(name: &str) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-shards-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (node, rx, lrx, logs) = open(&dir);
    Peer { node, rx, lrx, logs, did: String::new(), phrase: String::new(), dir }
}

fn online(n: &Node) {
    n.start().unwrap();
    let t = Instant::now();
    while !n.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A committed identity; started (`online`) or not.
fn peer(name: &str, started: bool) -> Peer {
    let mut p = blank(name);
    p.phrase = p.node.create_identity(name.into(), PASS.into()).unwrap();
    if started {
        online(&p.node);
    }
    p.did = p.node.profile().unwrap().did;
    p
}

fn eventually(secs: u64, what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("timed out waiting for {what}");
}


const DAY: i64 = 86_400_000;
const HOUR: i64 = 3_600_000;

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

/// Start of the UTC day containing `ms`.
fn floor(ms: i64) -> i64 {
    ms.div_euclid(DAY) * DAY
}

/// `YYYY-MM-DD` (UTC) of `ms` (days-from-civil inverse, H. Hinnant).
fn day_str(ms: i64) -> String {
    let z = ms.div_euclid(DAY) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Noon UTC, `n` days before today.
fn noon_ago(n: i64) -> i64 {
    floor(now_ms()) - n * DAY + 12 * HOUR
}

/// Every message of the conversation, oldest first (the order the UI shows: day, then `(at, id)`).
fn all_msgs(p: &Peer, did: &str) -> Vec<Message> {
    let mut pages = Vec::new();
    let mut page = p.node.chat_day(did.to_string(), None).ok();
    while let Some(pg) = page {
        pages.push(pg.messages.clone());
        page = pg.older_day.and_then(|d| p.node.chat_day(did.to_string(), Some(d)).ok());
    }
    pages.into_iter().rev().flatten().collect()
}

fn find(p: &Peer, did: &str, id: &str) -> Option<Message> {
    all_msgs(p, did).into_iter().find(|m| m.id == id)
}

fn has(p: &Peer, did: &str, id: &str) -> bool {
    find(p, did, id).is_some()
}

fn ticks(p: &Peer, did: &str, id: &str) -> DeliveryState {
    find(p, did, id).expect("message held").delivery
}

/// `(id, text, deleted)` of every message, sorted by id: the state two devices must agree on.
fn state(p: &Peer, did: &str) -> Vec<(String, String, bool)> {
    let mut v: Vec<_> = all_msgs(p, did).into_iter().map(|m| (m.id, m.text, m.deleted)).collect();
    v.sort();
    v
}

fn restart(p: &mut Peer, pass: &str) {
    p.node.stop();
    let (node, rx, lrx, logs) = open(&p.dir);
    drop(std::mem::replace(&mut p.node, node));
    p.rx = rx;
    p.lrx = lrx;
    p.logs = logs;
    p.node.unlock(pass.into()).unwrap();
}

/// Records when events arrived.
#[derive(Default)]
struct Stamps {
    added: Mutex<std::collections::HashMap<String, Instant>>,
    delivered: Mutex<std::collections::HashMap<String, Instant>>,
}

impl ChatEvents for Stamps {
    fn on_message_added(&self, m: Message) {
        self.added.lock().unwrap().entry(m.id).or_insert_with(Instant::now);
    }
    fn on_message_changed(&self, _: Message) {}
    fn on_chat_changed(&self, _: Chat) {}
    fn on_delivery_changed(&self, _: String, id: String, d: DeliveryState) {
        if d == DeliveryState::Delivered {
            self.delivered.lock().unwrap().entry(id).or_insert_with(Instant::now);
        }
    }
    fn on_transfer_progress(&self, _: String, _: String, _: u64, _: u64, _: bool) {}
    fn on_presence_changed(&self, _: String, _: bool) {}
}

fn tap(p: &Peer) -> Arc<Stamps> {
    let t = Arc::new(Stamps::default());
    p.node.set_chat_events(t.clone());
    t
}

/// Two started peers that are contacts, with a chat session up (one warm-up message delivered).
fn pair(tag: &str) -> (Peer, Peer) {
    let a = peer(&format!("{tag}-a"), true);
    let b = peer(&format!("{tag}-b"), true);
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    eventually(30, "A has B as a contact", || a.node.contacts().iter().any(|c| c.did == b.did));
    let hi = a.node.send_text(b.did.clone(), "warm-up".into(), None).unwrap().id;
    eventually(60, "B holds the warm-up", || has(&b, &a.did, &hi));
    eventually(60, "warm-up delivered", || ticks(&a, &b.did, &hi) == DeliveryState::Delivered);
    (a, b)
}

/// `p` writes one message to `to` with its chat clock `days_ago` days (and `plus` ms) back.
fn send_at(p: &Peer, to: &str, text: &str, days_ago: i64, plus: i64) -> Message {
    p.node.chat_clock_offset_for_test(-days_ago * DAY + plus);
    let m = p.node.send_text(to.to_string(), text.into(), None).unwrap();
    p.node.chat_clock_offset_for_test(0);
    m
}

fn pct(v: &mut [Duration], p: f64) -> Duration {
    v.sort();
    v[((v.len() as f64 * p).ceil() as usize).clamp(1, v.len()) - 1]
}

// ---- 1. day boundaries and windows -------------------------------------------------------------

// A message at 23:59:30 UTC and one at 00:00:30 UTC (live, the clock of the sender set there) land
// in the shards of two days on both sides, and both arrive.
#[test]
fn messages_around_utc_midnight_land_in_two_day_shards_and_both_arrive() {
    let (a, b) = pair("bd");
    let midnight = floor(now_ms());
    a.node.chat_clock_offset_for_test(midnight - 30_000 - now_ms());
    let m1 = a.node.send_text(b.did.clone(), "before midnight".into(), None).unwrap();
    a.node.chat_clock_offset_for_test(midnight + 30_000 - now_ms());
    let m2 = a.node.send_text(b.did.clone(), "after midnight".into(), None).unwrap();
    a.node.chat_clock_offset_for_test(0);
    assert!((m1.at as i64 - (midnight - 30_000)).abs() < 5_000 && (m2.at as i64 - (midnight + 30_000)).abs() < 5_000);
    for p in [&a, &b] {
        let other = if std::ptr::eq(p, &a) { &b.did } else { &a.did };
        eventually(60, "both messages held", || has(p, other, &m1.id) && has(p, other, &m2.id));
        let prev: Vec<String> = p.node.chat_day(other.clone(), Some(day_str(midnight - 1))).unwrap().messages.into_iter().map(|m| m.id).collect();
        let next: Vec<String> = p.node.chat_day(other.clone(), Some(day_str(midnight))).unwrap().messages.into_iter().map(|m| m.id).collect();
        assert_eq!(prev, vec![m1.id.clone()], "23:59:30 is in the day before");
        assert!(next.contains(&m2.id) && !next.contains(&m1.id), "00:00:30 is in the new day");
    }
    eventually(60, "both delivered", || ticks(&a, &b.did, &m1.id) == DeliveryState::Delivered && ticks(&a, &b.did, &m2.id) == DeliveryState::Delivered);
    assert!(!a.refused_chat() && !b.refused_chat(), "refused: {:?}", [&a, &b].map(|p| p.refusals()));
}

/// A writes a message of `at` ms into the shard of the day 3 days ago (so `at` can be outside
/// it); returns whether B ends up holding it. A refusal is awaited in B's log.
fn post_dated(tag: &str, at_from_day_start: i64, accepted: bool) {
    let (a, b) = pair(tag);
    // Written while B is away (a sibling's relayed op is not pushed): the hello of B's return
    // sends it as a catch-up sync.
    b.node.stop();
    let day_start = floor(now_ms()) - 3 * DAY;
    let id = a.node.write_in_day_for_test(b.did.clone(), "post-dated".into(), day_start + at_from_day_start, day_str(day_start)).unwrap();
    online(&b.node);
    if accepted {
        eventually(60, "B holds it", || has(&b, &a.did, &id));
        eventually(60, "delivered", || ticks(&a, &b.did, &id) == DeliveryState::Delivered);
        assert!(!b.refused_chat());
    } else {
        eventually(60, "B refuses the batch", || b.refused_chat());
        std::thread::sleep(Duration::from_secs(1));
        assert!(!has(&b, &a.did, &id), "a refused message is not stored");
        assert_eq!(ticks(&a, &b.did, &id), DeliveryState::Pending);
    }
}

// The shard's day +- 1 h is the window of `at`: 30 min past the end and 30 min before the start
// are accepted, 90 min past or before are refused.
#[test]
fn a_message_30_minutes_past_its_days_end_is_accepted() {
    post_dated("pd1", DAY + 30 * 60_000, true);
}

#[test]
fn a_message_30_minutes_before_its_days_start_is_accepted() {
    post_dated("pd2", -30 * 60_000, true);
}

#[test]
fn a_message_90_minutes_past_its_days_end_is_refused() {
    post_dated("pd3", DAY + 90 * 60_000, false);
}

#[test]
fn a_message_90_minutes_before_its_days_start_is_refused() {
    post_dated("pd4", -90 * 60_000, false);
}

// A live push is judged against the receiver's clock: a sender 23 h ahead is accepted, one 30 h
// ahead is refused (the refused push is then delivered by the catch-up sync of the reconnect,
// which has no live window, once the sender's clock is right again).
#[test]
fn a_live_push_from_a_clock_23_hours_ahead_is_accepted() {
    let (a, b) = pair("sk1");
    a.node.chat_clock_skew_for_test(23 * HOUR);
    let m = a.node.send_text(b.did.clone(), "from the future".into(), None).unwrap();
    a.node.chat_clock_skew_for_test(0);
    eventually(60, "B holds it", || has(&b, &a.did, &m.id));
    eventually(60, "delivered", || ticks(&a, &b.did, &m.id) == DeliveryState::Delivered);
    assert!(!b.refused_chat());
}

#[test]
fn a_live_push_from_a_clock_30_hours_ahead_is_refused() {
    let (a, b) = pair("sk2");
    a.node.chat_clock_skew_for_test(30 * HOUR);
    let m = a.node.send_text(b.did.clone(), "from the far future".into(), None).unwrap();
    eventually(60, "B refuses the live push", || b.refused_chat());
    // Refused as a live push: not held from it. Back on a right clock, the reconnect catches up.
    a.node.chat_clock_skew_for_test(0);
    eventually(120, "B holds it after the catch-up sync", || has(&b, &a.did, &m.id));
    eventually(60, "delivered", || ticks(&a, &b.did, &m.id) == DeliveryState::Delivered);
}

// ---- 2. a contact offline for more than the 7 days of a hello ------------------------------------

// B is offline while A writes on 10 distinct days (the oldest 10 days back); on reconnect B has
// all 10 without anyone scrolling: days with unacknowledged ops go into the hello whatever their
// age.
#[test]
fn a_contact_offline_for_ten_days_gets_every_day_on_reconnect() {
    let (a, b) = pair("off1");
    b.node.stop();
    let ids: Vec<String> = (1..=10).rev().map(|d| send_at(&a, &b.did, &format!("day -{d}"), d, 0).id).collect();
    online(&b.node);
    for id in &ids {
        eventually(120, "B holds the day", || has(&b, &a.did, id));
    }
    let n = all_msgs(&b, &a.did).iter().filter(|m| ids.contains(&m.id)).count();
    assert_eq!(n, 10, "all 10 days, not the last 7");
    for id in &ids {
        eventually(60, "delivered", || ticks(&a, &b.did, id) == DeliveryState::Delivered);
    }
    assert_eq!(b.node.fetch_older_history(a.did.clone(), day_str(now_ms())).unwrap(), 0, "nothing left for history to add");
    assert!(!a.refused_chat() && !b.refused_chat(), "refused: {:?}", [&a, &b].map(|p| p.refusals()));
}

// The reverse: A is away while B writes on 10 days (some written under another writer ID).
#[test]
fn a_contact_that_was_away_for_ten_days_gets_what_the_other_wrote() {
    let (a, b) = pair("off2");
    a.node.stop();
    let mut ids: Vec<String> = (1..=5).rev().map(|d| send_at(&b, &a.did, &format!("day -{d}"), d, 0).id).collect();
    for d in (6..=10).rev() {
        ids.push(b.node.write_as_sibling_for_test(a.did.clone(), format!("sibling day -{d}"), noon_ago(d)).unwrap());
    }
    online(&a.node);
    for id in &ids {
        eventually(120, "A holds the day", || has(&a, &b.did, id));
    }
    for id in &ids {
        eventually(60, "delivered", || ticks(&b, &a.did, id) == DeliveryState::Delivered);
    }
    assert_eq!(all_msgs(&a, &b.did).iter().filter(|m| ids.contains(&m.id)).count(), 10);
    assert!(!a.refused_chat() && !b.refused_chat(), "refused: {:?}", [&a, &b].map(|p| p.refusals()));
}

// ---- 3. closed days ---------------------------------------------------------------------------

// After both sides compacted the old days: an edit and a delete of a message in a closed day
// reach the peer and survive a restart of both; a message arriving late for a closed day is
// accepted and visible; compacting again keeps everything.
#[test]
fn edits_deletes_and_late_messages_in_closed_days_converge_and_survive_restarts() {
    let (mut a, mut b) = pair("cl");
    let a1 = send_at(&a, &b.did, "a1", 10, 0).id;
    let a2 = send_at(&a, &b.did, "a2", 11, 0).id;
    let b1 = send_at(&b, &a.did, "b1", 10, 60_000).id;
    for id in [&a1, &a2] {
        eventually(60, "B holds", || has(&b, &a.did, id));
        eventually(60, "delivered", || ticks(&a, &b.did, id) == DeliveryState::Delivered);
    }
    eventually(60, "A holds b1", || has(&a, &b.did, &b1));
    eventually(60, "b1 delivered", || ticks(&b, &a.did, &b1) == DeliveryState::Delivered);
    assert!(a.node.compact_now_for_test().unwrap() >= 2, "A closed both days");
    assert!(b.node.compact_now_for_test().unwrap() >= 2, "B closed both days");

    a.node.edit_message(b.did.clone(), a1.clone(), "a1 edited".into()).unwrap();
    a.node.delete_message(b.did.clone(), a2.clone()).unwrap();
    b.node.edit_message(a.did.clone(), b1.clone(), "b1 edited".into()).unwrap();
    let texts = |p: &Peer, did: &str| -> Vec<(String, bool)> {
        [&a1, &a2, &b1].iter().map(|id| find(p, did, id).map(|m| (m.text, m.deleted)).unwrap_or_default()).collect()
    };
    let want = vec![("a1 edited".to_string(), false), (String::new(), true), ("b1 edited".to_string(), false)];
    eventually(60, "B shows A's edit and delete", || texts(&b, &a.did)[..2] == want[..2]);
    eventually(60, "A shows B's edit", || texts(&a, &b.did) == want);
    eventually(60, "B shows its own edit", || texts(&b, &a.did) == want);
    let (sa, sb) = (state(&a, &b.did), state(&b, &a.did));
    assert_eq!(sa, sb);

    // Restart both: the closed days and their deltas come back from the store.
    restart(&mut a, PASS);
    restart(&mut b, PASS);
    assert_eq!(texts(&a, &b.did), want, "A after its restart");
    assert_eq!(texts(&b, &a.did), want, "B after its restart");
    online(&a.node);
    online(&b.node);

    // A message for a closed day arrives late (written under another writer ID, as a sibling's).
    let late = a.node.write_as_sibling_for_test(b.did.clone(), "late".into(), noon_ago(10) + HOUR).unwrap();
    eventually(90, "B holds the late message", || has(&b, &a.did, &late));
    eventually(60, "late delivered", || ticks(&a, &b.did, &late) == DeliveryState::Delivered);
    assert_eq!(find(&b, &a.did, &late).unwrap().text, "late");
    let (sa, sb) = (state(&a, &b.did), state(&b, &a.did));
    assert_eq!(sa, sb);
    assert_eq!(sa.len(), 5, "warm-up, a1, a2, b1, late");

    // Compact again and restart: nothing is lost.
    assert!(a.node.compact_now_for_test().unwrap() >= 2);
    assert!(b.node.compact_now_for_test().unwrap() >= 2);
    assert_eq!(state(&a, &b.did), sa);
    assert_eq!(state(&b, &a.did), sb);
    restart(&mut a, PASS);
    restart(&mut b, PASS);
    assert_eq!(state(&a, &b.did), sa, "A after recompaction and restart");
    assert_eq!(state(&b, &a.did), sb, "B after recompaction and restart");
    assert!(!a.refused_chat() && !b.refused_chat(), "refused: {:?}", [&a, &b].map(|p| p.refusals()));
}

// ---- 4. the shard cache ---------------------------------------------------------------------

// 15 days are touched on both sides (more than the 12 the cache keeps), then read and written
// again, closed and open, in turn: every read and write is still right and reaches the peer.
#[test]
fn reads_and_writes_beyond_the_shard_cache_stay_correct() {
    let (a, b) = pair("ca");
    let mut ids: Vec<String> = (1..=15).rev().map(|d| send_at(&a, &b.did, &format!("a day -{d}"), d, 0).id).collect();
    for id in &ids {
        eventually(120, "B holds", || has(&b, &a.did, id));
    }
    assert!(all_msgs(&a, &b.did).len() >= 16 && all_msgs(&b, &a.did).len() >= 16);
    assert!(a.node.compact_now_for_test().unwrap() >= 7, "days of 8+ days ago are closed");
    assert!(b.node.compact_now_for_test().unwrap() >= 7);
    // B answers on every day (writes on evicted and closed days), A edits what it wrote.
    for d in (1..=15).rev() {
        ids.push(send_at(&b, &a.did, &format!("b day -{d}"), d, 60_000).id);
    }
    for (i, id) in ids[..15].iter().enumerate().rev() {
        a.node.edit_message(b.did.clone(), id.clone(), format!("a edited {i}")).unwrap();
    }
    for id in &ids[15..] {
        eventually(120, "A holds B's answer", || has(&a, &b.did, id));
    }
    for (i, id) in ids[..15].iter().enumerate() {
        eventually(120, "B shows the edit", || find(&b, &a.did, id).is_some_and(|m| m.text == format!("a edited {i}")));
    }
    let (sa, sb) = (state(&a, &b.did), state(&b, &a.did));
    assert_eq!(sa, sb);
    assert_eq!(sa.len(), 31, "warm-up + 15 + 15");
    // Read every day again, oldest first then newest first: the same.
    for _ in 0..2 {
        assert_eq!(state(&a, &b.did), sa);
        assert_eq!(state(&b, &a.did), sb);
    }
    for id in &ids {
        let p = if ids[..15].contains(id) { (&a, &b) } else { (&b, &a) };
        eventually(60, "delivered", || ticks(p.0, &p.1.did, id) == DeliveryState::Delivered);
    }
    assert!(!a.refused_chat() && !b.refused_chat(), "refused: {:?}", [&a, &b].map(|p| p.refusals()));
}

// ---- 5. liveness ------------------------------------------------------------------------------

// Loopback latency of one message at a time: send -> `on_message_added` on the peer, and send ->
// the Delivered tick on the sender. 50 messages after a warm-up.
#[test]
fn single_message_latency_on_loopback_is_well_under_a_second() {
    let (a, b) = pair("lat");
    let (ta, tb) = (tap(&a), tap(&b));
    let (mut recv, mut tick) = (Vec::new(), Vec::new());
    for i in 0..50 {
        let t0 = Instant::now();
        let id = a.node.send_text(b.did.clone(), format!("m{i}"), None).unwrap().id;
        let end = Instant::now() + Duration::from_secs(10);
        let (mut r, mut d) = (None, None);
        while (r.is_none() || d.is_none()) && Instant::now() < end {
            r = r.or_else(|| tb.added.lock().unwrap().get(&id).copied());
            d = d.or_else(|| ta.delivered.lock().unwrap().get(&id).copied());
            std::thread::sleep(Duration::from_micros(500));
        }
        recv.push(r.expect("arrived").saturating_duration_since(t0));
        tick.push(d.expect("ticked").saturating_duration_since(t0));
    }
    let (r50, r95, d50, d95) = (pct(&mut recv, 0.5), pct(&mut recv, 0.95), pct(&mut tick, 0.5), pct(&mut tick, 0.95));
    println!("send -> on_message_added on the peer: p50 {r50:?} p95 {r95:?} max {:?}", recv.last().unwrap());
    println!("send -> Delivered tick:               p50 {d50:?} p95 {d95:?} max {:?}", tick.last().unwrap());
    assert!(r95 < Duration::from_secs(1), "receive p95 {r95:?}");
    assert!(d95 < Duration::from_secs(1), "tick p95 {d95:?}");
}

/// Waits until `p` holds exactly `ids` (each once), in `(at, id)` order across days.
fn assert_complete(p: &Peer, other: &str, ids: &[String], what: &str) {
    eventually(120, what, || {
        let held = all_msgs(p, other);
        ids.iter().all(|id| held.iter().any(|m| &m.id == id))
    });
    let held = all_msgs(p, other);
    let mut seen = std::collections::HashSet::new();
    assert!(held.iter().all(|m| seen.insert(m.id.clone())), "{what}: no duplicates");
    assert_eq!(held.len(), ids.len() + 1, "{what}: exactly the sent messages (and the warm-up)");
    let keys: Vec<(u64, &str)> = held.iter().map(|m| (m.at, m.id.as_str())).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "{what}: (at, id) order");
}

fn all_delivered(p: &Peer, other: &str, ids: &[String]) {
    eventually(120, "all delivered", || {
        let held = all_msgs(p, other);
        ids.iter().all(|id| held.iter().any(|m| &m.id == id && m.delivery == DeliveryState::Delivered))
    });
}

// A burst of 200 sent back to back arrives complete and in order, and every one is ticked.
#[test]
fn a_burst_of_200_arrives_complete_in_order_and_delivered() {
    let (a, b) = pair("burst");
    let t0 = Instant::now();
    let ids: Vec<String> = (0..200).map(|i| a.node.send_text(b.did.clone(), format!("burst {i}"), None).unwrap().id).collect();
    let sent = t0.elapsed();
    assert_complete(&b, &a.did, &ids, "B");
    let arrived = t0.elapsed();
    all_delivered(&a, &b.did, &ids);
    println!("burst of 200: sent in {sent:?}, all held by B after {arrived:?}, all ticked after {:?}", t0.elapsed());
    assert_complete(&a, &b.did, &ids, "A");
    assert!(!a.refused_chat() && !b.refused_chat(), "refused: {:?}", [&a, &b].map(|p| p.refusals()));
}

// Both sides send 100 at the same time: both end with the same 200 (plus the warm-up).
#[test]
fn both_sides_sending_100_at_once_converge() {
    let (a, b) = pair("both");
    let send = |p: &Peer, to: String, tag: &'static str| {
        let n = p.node.clone();
        std::thread::spawn(move || (0..100).map(|i| n.send_text(to.clone(), format!("{tag} {i}"), None).unwrap().id).collect::<Vec<_>>())
    };
    let (ha, hb) = (send(&a, b.did.clone(), "from a"), send(&b, a.did.clone(), "from b"));
    let mut ids = ha.join().unwrap();
    ids.extend(hb.join().unwrap());
    assert_complete(&a, &b.did, &ids, "A");
    assert_complete(&b, &a.did, &ids, "B");
    assert_eq!(state(&a, &b.did), state(&b, &a.did));
    let (mine, theirs): (Vec<String>, Vec<String>) = (ids[..100].to_vec(), ids[100..].to_vec());
    all_delivered(&a, &b.did, &mine);
    all_delivered(&b, &a.did, &theirs);
    assert!(!a.refused_chat() && !b.refused_chat(), "refused: {:?}", [&a, &b].map(|p| p.refusals()));
}

// B's process restarts in the middle of a burst: everything still arrives, once each.
#[test]
fn a_peer_restarting_mid_burst_still_gets_everything_once() {
    let (a, mut b) = pair("drop");
    let tb = tap(&b);
    let n = a.node.clone();
    let to = b.did.clone();
    let sender = std::thread::spawn(move || (0..200).map(|i| n.send_text(to.clone(), format!("burst {i}"), None).unwrap().id).collect::<Vec<_>>());
    eventually(60, "B has part of the burst", || tb.added.lock().unwrap().len() >= 30);
    restart(&mut b, PASS);
    tap(&b);
    online(&b.node);
    let ids = sender.join().unwrap();
    assert_complete(&b, &a.did, &ids, "B");
    all_delivered(&a, &b.did, &ids);
    assert_complete(&a, &b.did, &ids, "A");
}
