//! A long run of one 1:1 conversation on real nodes (`support::Peer`, loopback iroh): N simulated
//! days (`CHAT_LONGRUN_DAYS`, default 30, at least 20) of traffic both ways between A and B, each
//! with a second device (A2, B2). Every device has days off (a weekly day, a week or ten-day gap)
//! and keeps writing while away; messages of closed days are edited and deleted, B's from both of
//! its devices; one day crosses UTC midnight; on the last day B's clock is three hours ahead; both
//! mark read now and then; every device compacts every day. After every reconnect every online
//! device must agree, and when both sides are up everything must be delivered.
//!
//! Multi-device cases: while A2 is away it edits and deletes three of A's messages that A edits
//! too (later edit wins, delete beats edit; all four devices must pick the same winner); while
//! B is away A reaches B2 directly; near the end, with B's side away, A2 writes two messages that
//! reach only A and is then lost for good (stopped, unlinked by A): A must deliver them when B is
//! back. At the end A restarts (closed days come back from their blobs), a fresh device A3 links
//! and must receive the whole conversation. `LONGRUN ...` lines carry the measurements (run with
//! `--nocapture`).
//!
//! The simulated day is `chat_clock_offset_for_test` on every node: messages get that `at`; the
//! hello days and the compaction limit use it too. The live window uses the real clock, so
//! messages of past days travel as catch-up `Sync` frames; only the last day exercises `Push`.
//!
//! The second test measures what a fresh device receives from a sibling that holds more
//! contact-days than one own-device hello lists (`MAX_OWN_DOCS`); it reports, it does not judge.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use p2pcore::{DeliveryState, Message};

use crate::support;
use support::all_msgs_oldest_first as all_msgs;
use support::eventually_200ms as eventually;
use support::now_ms_i64 as now_ms;
use support::*;

const S: Suite = Suite("longrun");

const DAY: i64 = 86_400_000;
const HOUR: i64 = 3_600_000;

/// Simulated days in the run (`CHAT_LONGRUN_DAYS`; 30 by default, 180 for a long one, at least 20).
fn days() -> i64 {
    std::env::var("CHAT_LONGRUN_DAYS").ok().and_then(|v| v.parse().ok()).filter(|n| *n >= 20).unwrap_or(30)
}

/// Start of the UTC day containing `ms`.
fn floor(ms: i64) -> i64 {
    ms.div_euclid(DAY) * DAY
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: i64) -> i64 {
        (self.next() % n as u64) as i64
    }
}

/// Which install wrote a message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dev {
    A,
    B,
    A2,
    B2,
}

/// What was written, enough to pick edit and delete targets later and to check the end state.
struct Sent {
    id: String,
    dev: Dev,
    day: i64,
    edited: bool,
    deleted: bool,
    /// Part of the two-device conflict: the closed-day edits leave it alone.
    pinned: bool,
}

impl Sent {
    fn new(m: Message, dev: Dev, day: i64) -> Sent {
        Sent { id: m.id, dev, day, edited: false, deleted: false, pinned: false }
    }
}

fn find(p: &Peer, did: &str, id: &str) -> Option<Message> {
    all_msgs(p, did).into_iter().find(|m| m.id == id)
}

/// `(id, text, deleted)` of every message, sorted by id: what two devices must agree on.
fn state(p: &Peer, did: &str) -> Vec<(String, String, bool)> {
    let mut v: Vec<_> = all_msgs(p, did).into_iter().map(|m| (m.id, m.text, m.deleted)).collect();
    v.sort();
    v
}

/// Every message this device wrote to `did` has two ticks.
fn mine_delivered(p: &Peer, did: &str) -> bool {
    all_msgs(p, did).iter().filter(|m| m.outgoing).all(|m| m.delivery == DeliveryState::Delivered)
}

/// `p` writes to `to` at `base + plus` on its simulated clock, then goes back to `base`.
fn send(p: &Peer, to: &str, text: &str, base: i64, plus: i64) -> Message {
    p.node.chat_clock_offset_for_test(base + plus);
    let m = p.node.send_text(to.to_string(), text.into(), None).unwrap();
    p.node.chat_clock_offset_for_test(base);
    m
}

/// `p` edits `id` at `base + plus` on its simulated clock (which decides the winner of two edits).
fn edit_at(p: &Peer, to: &str, id: &str, text: &str, base: i64, plus: i64) {
    p.node.chat_clock_offset_for_test(base + plus);
    p.node.edit_message(to.to_string(), id.to_string(), text.into()).unwrap();
    p.node.chat_clock_offset_for_test(base);
}

/// `p` deletes `id` at `base + plus` on its simulated clock.
fn delete_at(p: &Peer, to: &str, id: &str, base: i64, plus: i64) {
    p.node.chat_clock_offset_for_test(base + plus);
    p.node.delete_message(to.to_string(), id.to_string()).unwrap();
    p.node.chat_clock_offset_for_test(base);
}

/// `(files, bytes)` under `p`, recursively.
fn dir_stats(p: &Path) -> (u64, u64) {
    let mut files = 0;
    let mut bytes = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                let (f, b) = dir_stats(&e.path());
                files += f;
                bytes += b;
            } else {
                files += 1;
                bytes += md.len();
            }
        }
    }
    (files, bytes)
}

/// The account directory of `p` (`<root>/accounts/<id>/`).
fn account_dir(p: &Peer) -> Option<PathBuf> {
    std::fs::read_dir(p.dir.join("accounts")).ok()?.flatten().map(|e| e.path()).find(|e| e.is_dir())
}

/// `(chat.redb bytes, blob files, blob bytes)` of `p`.
fn sizes(p: &Peer) -> (u64, u64, u64) {
    let Some(dir) = account_dir(p) else { return (0, 0, 0) };
    let redb = std::fs::metadata(dir.join("chat.redb")).map(|m| m.len()).unwrap_or(0);
    let (files, bytes) = dir_stats(&dir.join("blobs"));
    (redb, files, bytes)
}

/// How many day shards `p` holds of its conversation with `did`.
fn days_held(p: &Peer, did: &str) -> usize {
    p.node.chat_doc_peers_for_test(did.to_string()).0.into_iter().map(|(d, _, _)| d).collect::<BTreeSet<_>>().len()
}

/// `(id, text, deleted)` of the messages `p` holds that its own account wrote.
fn own_state(p: &Peer, did: &str) -> Vec<(String, String, bool)> {
    all_msgs(p, did).into_iter().filter(|m| m.outgoing).map(|m| (m.id, m.text, m.deleted)).collect()
}

/// Waits up to 240 s until the devices `ups` are consistent on day `d` (and, with `delivered`,
/// show two ticks on all of their own messages). Devices of one person (`side`) hold the same
/// messages. Across the two people, each device holds what the other side's online devices wrote
/// in the hello window (days `d-6..`), as they hold it. That is all the protocol owes it there: a
/// device sends the contact only its own account's ops (`docs/chat.md` §8), and on a session only
/// the last `HELLO_DAYS` plus days still unacknowledged by any of our devices; the rest comes from
/// our own devices. With `full`, all must be equal. On a timeout, prints what differs, by ledger
/// entry, and panics.
#[allow(clippy::too_many_arguments)]
fn agree(what: &str, d: i64, peers: &[&Peer], names: &[&str], other: &[String], side: &[bool], ups: &[usize], full: bool, delivered: bool, ledger: &[Sent]) {
    let recent = |id: &str| ledger.iter().find(|s| s.id == id).is_some_and(|s| s.day >= d - 6);
    let ok = || {
        let st: Vec<Vec<(String, String, bool)>> = (0..peers.len()).map(|i| if ups.contains(&i) { state(peers[i], &other[i]) } else { Vec::new() }).collect();
        let consistent = ups.iter().all(|&i| {
            ups.iter().all(|&j| {
                if i == j {
                    true
                } else if full || side[i] == side[j] {
                    st[i] == st[j]
                } else {
                    own_state(peers[j], &other[j]).iter().filter(|x| recent(&x.0)).all(|x| st[i].contains(x))
                }
            })
        });
        consistent && (!delivered || ups.iter().all(|&i| mine_delivered(peers[i], &other[i])))
    };
    let who = |id: &str| ledger.iter().find(|s| s.id == id).map(|s| format!("{:?} day {}", s.dev, s.day)).unwrap_or_else(|| "warm-up".into());
    let dump = || {
        let union: BTreeSet<String> = ups.iter().flat_map(|&i| all_msgs(peers[i], &other[i]).into_iter().map(|m| m.id)).collect();
        for &i in ups {
            let msgs = all_msgs(peers[i], &other[i]);
            let held: BTreeSet<String> = msgs.iter().map(|m| m.id.clone()).collect();
            let missing: Vec<String> = union.difference(&held).map(|id| who(id)).collect();
            let undelivered: Vec<String> = msgs.iter().filter(|m| m.outgoing && m.delivery != DeliveryState::Delivered).map(|m| format!("{} ({:?})", who(&m.id), m.delivery)).collect();
            println!("DUMP {what} {}: holds {}, lacks {:?}, own not delivered {:?}", names[i], msgs.len(), missing, undelivered);
        }
        // What the check still waits for, across the two people (the hello window).
        for &i in ups {
            let si = state(peers[i], &other[i]);
            for &j in ups {
                if side[i] == side[j] {
                    continue;
                }
                let unmet: Vec<String> = own_state(peers[j], &other[j]).into_iter().filter(|x| recent(&x.0) && !si.contains(x)).map(|x| format!("{}: {:?} deleted {}", who(&x.0), x.1, x.2)).collect();
                if !unmet.is_empty() {
                    println!("DUMP {what} {} still lacks from {}: {:?}", names[i], names[j], unmet);
                }
            }
        }
    };
    let t = Instant::now();
    let mut slow_dumped = false;
    while t.elapsed() < Duration::from_secs(240) {
        if ok() {
            return;
        }
        if !slow_dumped && t.elapsed() > Duration::from_secs(30) {
            slow_dumped = true;
            println!("SLOW {what}: not consistent after 30 s");
            dump();
            for &i in ups {
                let logs = peers[i].logs.lock().unwrap();
                let tail: Vec<&String> = logs.iter().rev().take(30).collect();
                for l in tail.into_iter().rev() {
                    println!("SLOWLOG {}: {l}", names[i]);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    dump();
    for &i in ups {
        let logs = peers[i].logs.lock().unwrap();
        let tail: Vec<&String> = logs.iter().rev().take(40).collect();
        for l in tail.into_iter().rev() {
            println!("LOG {}: {l}", names[i]);
        }
    }
    let s0 = state(peers[ups[0]], &other[ups[0]]);
    for &i in &ups[1..] {
        let si = state(peers[i], &other[i]);
        let diff: Vec<String> = si.iter().filter(|x| !s0.contains(x) && s0.iter().any(|y| y.0 == x.0)).map(|x| format!("{}: {:?} deleted {}", who(&x.0), x.1, x.2)).collect();
        if !diff.is_empty() {
            println!("DUMP {} differs from {} on {:?}", names[i], names[ups[0]], diff);
        }
    }
    panic!("timed out waiting for {what}");
}

fn pct(v: &mut [Duration], p: f64) -> Duration {
    if v.is_empty() {
        return Duration::ZERO;
    }
    v.sort();
    v[((v.len() as f64 * p).ceil() as usize).clamp(1, v.len()) - 1]
}

fn ms(d: Duration) -> u128 {
    d.as_millis()
}

// ---- 1. the long run ---------------------------------------------------------------------------

#[test]
fn a_conversation_over_many_days_with_absences_edits_and_compaction_converges() {
    let n = days();
    let t_run = Instant::now();

    // A and B, contacts, one warm-up delivered; then A2 and B2 link as second devices.
    let a = S.peer("a");
    let b = S.peer("b");
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    eventually(30, "A has B as a contact", || a.node.contacts().iter().any(|c| c.did == b.did));
    let hi = a.node.send_text(b.did.clone(), "warm-up".into(), None).unwrap().id;
    eventually(60, "B holds the warm-up", || has(&b, &a.did, &hi));
    eventually(60, "warm-up delivered", || ticks(&a, &b.did, &hi) == DeliveryState::Delivered);
    let t_link = Instant::now();
    let a2 = S.link_fresh(&a, "a2");
    eventually(60, "A2 knows B", || a2.node.contacts().iter().any(|c| c.did == b.did));
    eventually(120, "A2 holds the warm-up through A", || has(&a2, &b.did, &hi));
    println!("LONGRUN link: A2 linked and holds the conversation after {} ms", ms(t_link.elapsed()));

    let t_link = Instant::now();
    let b2 = S.link_fresh(&b, "b2");
    let (a2_key, b2_key) = (a2.key(), b2.key());
    eventually(60, "A learns B2 from B's device list", || a.dials(&b).contains(&b2_key));
    eventually(60, "B2 knows A", || b2.node.contacts().iter().any(|c| c.did == a.did));
    eventually(120, "B2 holds the warm-up through B", || has(&b2, &a.did, &hi));
    println!("LONGRUN link: B2 linked, A dials it, holds the conversation after {} ms", ms(t_link.elapsed()));

    let peers = [&a, &b, &a2, &b2];
    let names = ["A", "B", "A2", "B2"];
    // Whose conversation each device shows, and which side it is on.
    let other = [b.did.clone(), a.did.clone(), b.did.clone(), a.did.clone()];
    let side_a = [true, false, true, false];
    let mut up = [true; 4];
    // A2 is lost for good on `lost_day` (B's side is away that day).
    let mut gone = [false; 4];
    let lost_day = n - 4;
    // Handover days: B2 is away the day before, B writes then and goes away the day B2 is back
    // (day 3 with A2 writing that day, and near the end with A2 gone).
    let handovers = [3, n - 2];
    let mut lost_ids: Vec<String> = Vec::new();
    let mut lost_reported = false;
    // The two-device conflict: (message, winning text or None for deleted).
    let mut conflicts: Vec<(String, Option<String>)> = Vec::new();
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    let mut ledger: Vec<Sent> = Vec::new();
    // (day, who came up, time from `start` to agreement)
    let mut reconnects: Vec<(i64, &str, Duration)> = Vec::new();
    let mut compactions: Vec<Duration> = Vec::new();
    let mut closed_a = 0u32;

    let check_conflicts = |p: &Peer, did: &str, conflicts: &[(String, Option<String>)]| {
        for (id, want) in conflicts {
            let m = find(p, did, id).expect("conflict message held");
            match want {
                Some(t) => assert!(!m.deleted && m.text == *t, "{id}: {t:?} wins, not {:?} (deleted {})", m.text, m.deleted),
                None => assert!(m.deleted, "{id}: the delete wins over a later edit, not {:?}", m.text),
            }
        }
    };

    for d in 0..n {
        let base = -(n - 1 - d) * DAY;
        // Who is up today. A misses every 11th day; B a weekly day and days 10..=19 (then A talks to
        // B2 alone); B2 days 20..=26 and every 9th day; A2 days 5..=14 (and is gone after
        // `lost_day`); on `lost_day` B's side is away. A and B's side are never both away.
        let handover = handovers.contains(&d);
        let before_handover = handovers.contains(&(d + 1));
        let b_want = !(d % 7 == 3 || (10..=19).contains(&d)) && d != lost_day && !handover;
        let b2_want = (!((20..=26).contains(&d) || d % 9 == 7) && d != lost_day && !before_handover) || handover;
        let a_want = !(d % 11 == 5) || !(b_want || b2_want) || before_handover;
        let a2_want = !(5..=14).contains(&d) && !gone[2];
        let want = [a_want, b_want, a2_want, b2_want];
        let mut came_up: Vec<(usize, Instant)> = Vec::new();
        for i in 0..4 {
            if gone[i] {
                continue;
            }
            if up[i] && !want[i] {
                peers[i].node.stop();
                up[i] = false;
            } else if !up[i] && want[i] {
                let t = Instant::now();
                online(&peers[i].node);
                up[i] = true;
                came_up.push((i, t));
            }
        }
        for p in peers {
            p.node.chat_clock_offset_for_test(base);
        }

        // Writes: a few each way (also while away), A2 and B2 now and then, a midnight pair every
        // 9th day.
        let na = 1 + rng.below(4);
        let nb = 1 + rng.below(4);
        for k in 0..na {
            let plus = 8 * HOUR + rng.below(14 * HOUR);
            let m = send(&a, &b.did, &format!("a d{d} #{k}"), base, plus);
            ledger.push(Sent::new(m, Dev::A, d));
        }
        // On the last day B's clock is three hours ahead, for the whole day: its pushes are live
        // (the day is today) and carry that error, and so does its judgement of A's pushes; the
        // receiver's live window is 24 h. Earlier days travel as catch-up `Sync` whatever the
        // clock, so a skew there would prove nothing. (`send` resets the clock, so B's writes on
        // this day keep the skew by hand; `at` stays under 24 h ahead of the real clock.)
        let skew = if d == n - 1 { 3 * HOUR } else { 0 };
        if skew != 0 {
            b.node.chat_clock_skew_for_test(base + skew);
        }
        for k in 0..nb {
            let m = if skew != 0 {
                let plus = 8 * HOUR + rng.below(12 * HOUR);
                b.node.chat_clock_skew_for_test(base + skew + plus);
                let m = b.node.send_text(a.did.clone(), format!("b d{d} #{k} (+3h clock)"), None).unwrap();
                b.node.chat_clock_skew_for_test(base + skew);
                m
            } else {
                let plus = 8 * HOUR + rng.below(14 * HOUR);
                send(&b, &a.did, &format!("b d{d} #{k}"), base, plus)
            };
            ledger.push(Sent::new(m, Dev::B, d));
        }
        if (d % 6 == 2 || handover) && !gone[2] {
            let m = send(&a2, &b.did, &format!("a2 d{d}"), base, 20 * HOUR);
            ledger.push(Sent::new(m, Dev::A2, d));
        }
        if d % 5 == 1 {
            let m = send(&b2, &a.did, &format!("b2 d{d}"), base, 19 * HOUR);
            ledger.push(Sent::new(m, Dev::B2, d));
        }
        if d % 9 == 4 && d < n - 1 {
            let m = send(&a, &b.did, &format!("a d{d} 23:59:30"), base, DAY - 30_000);
            ledger.push(Sent::new(m, Dev::A, d));
            let m = send(&a, &b.did, &format!("a d{d} 00:00:30 next day"), base, DAY + 30_000);
            ledger.push(Sent::new(m, Dev::A, d + 1));
        }
        // A2 writes two messages while B's side is away; they reach A only, then A2 is lost.
        if d == lost_day {
            for k in 0..2 {
                let m = send(&a2, &b.did, &format!("a2 d{d} lost #{k}"), base, (10 + k) * HOUR);
                lost_ids.push(m.id.clone());
                ledger.push(Sent::new(m, Dev::A2, d));
            }
        }
        // The two-device conflict: A2 is away since day 5 and still holds A's messages of days
        // 1..=4. Both devices change the same three messages on day 6, at different hours.
        if d == 6 {
            let picks: Vec<usize> = ledger.iter().enumerate().filter(|(_, s)| s.dev == Dev::A && (1..=4).contains(&s.day)).map(|(i, _)| i).take(3).collect();
            assert_eq!(picks.len(), 3, "three of A's messages of days 1..=4");
            let ids: Vec<String> = picks.iter().map(|&i| ledger[i].id.clone()).collect();
            // 1. A at 09:00, A2 (away) at 20:00: A2's later edit wins.
            edit_at(&a, &b.did, &ids[0], "c1 by A at 09", base, 9 * HOUR);
            edit_at(&a2, &b.did, &ids[0], "c1 by A2 at 20 while away", base, 20 * HOUR);
            // 2. A2 (away) at 08:00, A at 21:00: A's later edit wins.
            edit_at(&a2, &b.did, &ids[1], "c2 by A2 at 08 while away", base, 8 * HOUR);
            edit_at(&a, &b.did, &ids[1], "c2 by A at 21", base, 21 * HOUR);
            // 3. A2 (away) deletes at 07:00, A edits at 22:00: the delete wins.
            delete_at(&a2, &b.did, &ids[2], base, 7 * HOUR);
            edit_at(&a, &b.did, &ids[2], "c3 by A at 22", base, 22 * HOUR);
            conflicts = vec![
                (ids[0].clone(), Some("c1 by A2 at 20 while away".into())),
                (ids[1].clone(), Some("c2 by A at 21".into())),
                (ids[2].clone(), None),
            ];
            for (k, &i) in picks.iter().enumerate() {
                ledger[i].pinned = true;
                ledger[i].edited = k < 2;
                ledger[i].deleted = k == 2;
            }
        }
        // Edits and deletes of messages in closed days (yesterday's compaction closed days up to
        // d-9): A edits one of day d-9 and deletes one of day d-12; B edits one of its own of
        // d-10; every third day B2 edits one of B's of d-11 that it holds.
        if d >= 12 {
            if let Some(s) = ledger.iter_mut().find(|s| s.dev == Dev::A && s.day == d - 9 && !s.edited && !s.deleted && !s.pinned) {
                a.node.edit_message(b.did.clone(), s.id.clone(), format!("a d{} edited on d{d}", s.day)).unwrap();
                s.edited = true;
            }
            if let Some(s) = ledger.iter_mut().find(|s| s.dev == Dev::A && s.day == d - 12 && !s.deleted && !s.pinned) {
                a.node.delete_message(b.did.clone(), s.id.clone()).unwrap();
                s.deleted = true;
            }
            if let Some(s) = ledger.iter_mut().find(|s| s.dev == Dev::B && s.day == d - 10 && !s.edited && !s.deleted) {
                b.node.edit_message(a.did.clone(), s.id.clone(), format!("b d{} edited on d{d}", s.day)).unwrap();
                s.edited = true;
            }
            if d % 3 == 0
                && let Some(s) = ledger.iter_mut().find(|s| s.dev == Dev::B && s.day == d - 11 && !s.edited && !s.deleted && find(&b2, &a.did, &s.id).is_some())
            {
                b2.node.edit_message(a.did.clone(), s.id.clone(), format!("b d{} edited on d{d} by B2", s.day)).unwrap();
                s.edited = true;
            }
        }
        if d % 5 == 0 {
            let _ = b.node.mark_read(a.did.clone());
            let _ = a.node.mark_read(b.did.clone());
        }

        // Every online device agrees; when both sides are up, everything held is delivered.
        let ups: Vec<usize> = (0..4).filter(|&i| up[i]).collect();
        let both = ups.iter().any(|&i| side_a[i]) && ups.iter().any(|&i| !side_a[i]);
        let what = if both { "online devices are consistent and everything is delivered" } else { "one side's online devices agree" };
        agree(&format!("day {d}: {what}"), d, &peers, &names, &other, &side_a, &ups, false, both, &ledger);
        if handover {
            let held: BTreeSet<String> = all_msgs(&b2, &a.did).into_iter().map(|m| m.id).collect();
            let yesterday: Vec<&Sent> = ledger.iter().filter(|s| s.dev == Dev::B && s.day == d - 1).collect();
            let lacks = yesterday.iter().filter(|s| !held.contains(&s.id)).count();
            assert!(yesterday.iter().all(|s| has(&a, &b.did, &s.id)), "A holds B's messages of yesterday");
            let a_old = ledger.iter().filter(|s| matches!(s.dev, Dev::A | Dev::A2) && s.day < d - 6 && !held.contains(&s.id)).count();
            let b2_st = state(&b2, &a.did);
            let stale = state(&a, &b.did).iter().filter(|x| !b2_st.contains(x) && held.contains(&x.0)).count();
            println!(
                "LONGRUN day {d}: B2 back as B leaves: lacks {lacks} of B's {} messages of yesterday that A holds (A sends only its own), \
                 {a_old} of A's older than the hello window, and shows {stale} messages without a later edit or delete (all come from B)",
                yesterday.len()
            );
        }
        for (i, t) in came_up {
            reconnects.push((d, names[i], t.elapsed()));
            println!("LONGRUN day {d}: {} back, agreed after {} ms", names[i], ms(t.elapsed()));
            if i == 2 && !conflicts.is_empty() {
                for j in ups.iter().copied() {
                    check_conflicts(peers[j], &other[j], &conflicts);
                }
                println!("LONGRUN day {d}: A2 back, all online devices pick the same winners of the three conflicts");
            }
        }
        if d == lost_day {
            assert!(lost_ids.iter().all(|id| has(&a, &b.did, id)), "A holds A2's last messages");
            a2.node.stop();
            up[2] = false;
            gone[2] = true;
            a.node.unlink_device(a2_key.clone(), Some(PASS.into())).unwrap();
            println!("LONGRUN day {d}: A2 lost for good with {} messages only A holds; A unlinked it", lost_ids.len());
        }
        if d > lost_day && both && !lost_reported {
            lost_reported = true;
            println!("LONGRUN day {d}: A2's last messages reached B's side through A, two ticks");
        }

        // Compaction on every device that is up.
        for i in 0..4 {
            if up[i] {
                let t = Instant::now();
                let closed = peers[i].node.compact_now_for_test().unwrap();
                compactions.push(t.elapsed());
                if i == 0 {
                    closed_a = closed;
                }
            }
        }
        if skew != 0 {
            b.node.chat_clock_offset_for_test(base);
        }
        if d % 10 == 9 || d == n - 1 {
            let (redb, bf, bb) = sizes(&a);
            println!("LONGRUN day {d}: {} messages, A closed {closed_a} days, chat.redb {redb} B, blobs {bf} files {bb} B, {} s so far", ledger.len(), t_run.elapsed().as_secs());
        }
    }

    // Everyone still linked up, final agreement.
    for i in 0..4 {
        if !up[i] && !gone[i] {
            online(&peers[i].node);
            up[i] = true;
        }
    }
    let live: Vec<usize> = (0..4).filter(|&i| !gone[i]).collect();
    agree("final: every device agrees and everything is delivered", n, &peers, &names, &other, &side_a, &live, true, true, &ledger);
    let want = state(&a, &b.did);
    assert_eq!(want.len(), ledger.len() + 1, "every message written (and the warm-up), once");
    for s in &ledger {
        let m = find(&b, &a.did, &s.id).or_else(|| find(&a, &b.did, &s.id)).expect("held");
        assert_eq!(m.deleted, s.deleted, "{}: deleted", s.id);
        if s.edited && !s.deleted && !s.pinned {
            assert!(m.text.contains("edited on"), "{}: shows the edit, not {:?}", s.id, m.text);
        }
    }
    for &i in &live {
        check_conflicts(peers[i], &other[i], &conflicts);
    }
    eventually(60, "B drops A2 from A's devices", || !b.dials(&a).contains(&a2_key) && !b2.dials(&a).contains(&a2_key));
    for p in peers {
        assert!(!p.refused_chat(), "refused: {:?}", p.refusals());
    }
    let closed_b = b.node.compact_now_for_test().unwrap();
    println!("LONGRUN end: {} messages over {n} days; A closed {closed_a} days, B {closed_b}", ledger.len());

    // A restarts: the closed days come back from their blobs.
    let mut a = a;
    let t = Instant::now();
    a.reopen_unlocked(PASS);
    online(&a.node);
    a.node.chat_clock_offset_for_test(0);
    assert_eq!(state(&a, &b.did), want, "A after a restart");
    println!("LONGRUN restart: A reopened and read every day in {} ms", ms(t.elapsed()));

    // A fresh device links and receives the whole conversation.
    let t = Instant::now();
    let a3 = S.link_fresh(&a, "a3");
    eventually(300, "A3 receives the whole conversation", || state(&a3, &b.did) == want);
    let (da, d3) = (days_held(&a, &b.did), days_held(&a3, &b.did));
    println!("LONGRUN a3: whole conversation ({} days) after {} ms", d3, ms(t.elapsed()));
    assert_eq!(d3, da, "A3 holds every day A holds");
    // A3 marks every day holding our not-yet-acknowledged messages as owed to B (P10b) until the
    // acks reach it (relayed from A, or from B's own hello); compaction skips owed days, so wait
    // until A3 shows two ticks on everything before comparing what it closes.
    let t = Instant::now();
    eventually(300, "A3 shows every message of ours delivered", || mine_delivered(&a3, &b.did));
    println!("LONGRUN a3: two ticks on every message after {} ms", ms(t.elapsed()));
    let closed_3 = a3.node.compact_now_for_test().unwrap();
    assert_eq!(closed_3, closed_a, "A3 closes the same days");
    assert!(!a3.refused_chat(), "A3 refused: {:?}", a3.refusals());

    let mut rc: Vec<Duration> = reconnects.iter().map(|r| r.2).collect();
    println!(
        "LONGRUN reconnects: {} (p50 {} ms, p95 {} ms, max {} ms); compaction passes: {} (p50 {} ms, p95 {} ms); run {} s",
        rc.len(),
        ms(pct(&mut rc, 0.5)),
        ms(pct(&mut rc, 0.95)),
        ms(rc.last().copied().unwrap_or_default()),
        compactions.len(),
        ms(pct(&mut compactions, 0.5)),
        ms(pct(&mut compactions, 0.95)),
        t_run.elapsed().as_secs()
    );
}

// ---- 2. a large history and a fresh device ------------------------------------------------------

// Measurement. A sibling offers a new device at most `MAX_OWN_DOCS` (4096, less a few markers)
// shards per hello, newest first (`chat_own_docs`). An account with 60 contacts and 80 days each
// (4800 contact-days, written in process) links a fresh device: how many contact-days arrive, and
// is the newest day of every contact among them? Nothing is compacted here: the hello lists a
// day's meta whether it is closed or not.
#[test]
fn a_fresh_device_of_a_large_history_receives_the_newest_days() {
    let a = S.peer("cap-a");
    let (contacts, days_each) = (60usize, 80i64);
    let mut dids = Vec::new();
    for i in 0..contacts {
        let phrase = a.node.synth_phrase_for_test();
        let did = a.node.synth_peer_for_test(phrase).unwrap();
        a.node.synth_contact_for_test(did.clone(), format!("Contact {i}")).unwrap();
        dids.push(did);
    }
    let today = floor(now_ms());
    let t = Instant::now();
    for d in 0..days_each {
        let at = today - (days_each - d) * DAY + 12 * HOUR;
        for (i, did) in dids.iter().enumerate() {
            let author = if (d as usize + i) % 2 == 0 { a.did.clone() } else { did.clone() };
            a.node.synth_dm_for_test(did.clone(), author, format!("c{i} d{d}"), at, None).unwrap();
        }
    }
    let total = contacts as i64 * days_each;
    println!("LONGRUN cap: {total} contact-days written in {} ms", ms(t.elapsed()));

    let t = Instant::now();
    let a3 = S.link_fresh(&a, "cap-a3");
    let count = || dids.iter().map(|d| days_held(&a3, d)).sum::<usize>();
    // Own-device sync runs until the link idles: wait for the count to settle.
    let (mut last, mut stable) = (0usize, Instant::now());
    let end = Instant::now() + Duration::from_secs(300);
    while Instant::now() < end {
        let c = count();
        if c != last {
            last = c;
            stable = Instant::now();
        } else if c > 0 && stable.elapsed() > Duration::from_secs(30) {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    let first = last;
    println!("LONGRUN cap: first session: {first} of {total} contact-days on the new device after {} ms", ms(t.elapsed()));
    // A second session (as a network change would start): does more arrive?
    a.node.network_changed();
    a3.node.network_changed();
    std::thread::sleep(Duration::from_secs(20));
    let second = count();
    println!("LONGRUN cap: after a second session: {second} of {total} contact-days");

    let newest = (0..days_each).map(|d| today - (days_each - d) * DAY + 12 * HOUR).max().unwrap();
    let with_newest = dids
        .iter()
        .filter(|did| all_msgs(&a3, did).iter().any(|m| m.at as i64 == newest))
        .count();
    println!("LONGRUN cap: {with_newest} of {contacts} contacts have their newest day on the new device");
    assert!(second > 0, "the new device received something");
    assert!(!a3.refused_chat(), "A3 refused: {:?}", a3.refusals());
    assert!(!a.refused_chat(), "A refused: {:?}", a.refusals());
}
