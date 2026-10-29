//! A long run of one 1:1 conversation on real nodes (`support::Peer`, loopback iroh): N simulated
//! days (`CHAT_LONGRUN_DAYS`, default 30) of traffic both ways between A and B, with A's second
//! device A2 along. Each side has days off (a weekly day, and a ten-day gap for B and for A2) and
//! keeps writing while away; messages of closed days are edited and deleted; one day crosses UTC
//! midnight, one day B's clock is three hours wrong; both mark read now and then; every device
//! compacts every day. After every reconnect, every online device must agree. At the end A
//! restarts (closed days come back from their blobs), a fresh device A3 links and must receive
//! the whole conversation. `LONGRUN ...` lines carry the measurements (run with `--nocapture`).
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

/// Simulated days in the run (`CHAT_LONGRUN_DAYS`; 30 by default, 180 for a long one).
fn days() -> i64 {
    std::env::var("CHAT_LONGRUN_DAYS").ok().and_then(|v| v.parse().ok()).filter(|n| *n >= 14).unwrap_or(30)
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
}

/// What was written, enough to pick edit and delete targets later and to check the end state.
struct Sent {
    id: String,
    dev: Dev,
    day: i64,
    edited: bool,
    deleted: bool,
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

    // A and B, contacts, one warm-up delivered; then A2 links as A's second device.
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

    let peers = [&a, &b, &a2];
    let names = ["A", "B", "A2"];
    let mut up = [true, true, true];
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    let mut ledger: Vec<Sent> = Vec::new();
    // (day, who came up, time from `start` to agreement)
    let mut reconnects: Vec<(i64, &str, Duration)> = Vec::new();
    let mut compactions: Vec<Duration> = Vec::new();
    let mut closed_a = 0u32;

    for d in 0..n {
        let base = -(n - 1 - d) * DAY;
        // Who is up today: A misses every 11th day, B a weekly day and days 10..=19, A2 days 5..=14
        // (back while B is still away: it catches up through A alone). A and B are never both down.
        let b_want = !(d % 7 == 3 || (10..=19).contains(&d));
        let a_want = !(d % 11 == 5) || !b_want;
        let a2_want = !(5..=14).contains(&d);
        let want = [a_want, b_want, a2_want];
        let mut came_up: Vec<(usize, Instant)> = Vec::new();
        for i in 0..3 {
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

        // Writes: a few each way (also while away), A2 now and then, a midnight pair every 9th
        // day, and one day B's clock is three hours ahead.
        let na = 1 + rng.below(4);
        let nb = 1 + rng.below(4);
        for k in 0..na {
            let plus = 8 * HOUR + rng.below(14 * HOUR);
            let m = send(&a, &b.did, &format!("a d{d} #{k}"), base, plus);
            ledger.push(Sent { id: m.id, dev: Dev::A, day: d, edited: false, deleted: false });
        }
        for k in 0..nb {
            let plus = 8 * HOUR + rng.below(14 * HOUR);
            let m = send(&b, &a.did, &format!("b d{d} #{k}"), base, plus);
            ledger.push(Sent { id: m.id, dev: Dev::B, day: d, edited: false, deleted: false });
        }
        if d % 6 == 2 {
            let m = send(&a2, &b.did, &format!("a2 d{d}"), base, 20 * HOUR);
            ledger.push(Sent { id: m.id, dev: Dev::A2, day: d, edited: false, deleted: false });
        }
        if d % 9 == 4 && d < n - 1 {
            let m = send(&a, &b.did, &format!("a d{d} 23:59:30"), base, DAY - 30_000);
            ledger.push(Sent { id: m.id, dev: Dev::A, day: d, edited: false, deleted: false });
            let m = send(&a, &b.did, &format!("a d{d} 00:00:30 next day"), base, DAY + 30_000);
            ledger.push(Sent { id: m.id, dev: Dev::A, day: d + 1, edited: false, deleted: false });
        }
        if d == n / 2 {
            b.node.chat_clock_skew_for_test(base + 3 * HOUR);
            let m = b.node.send_text(a.did.clone(), format!("b d{d} skewed +3h"), None).unwrap();
            b.node.chat_clock_offset_for_test(base);
            ledger.push(Sent { id: m.id, dev: Dev::B, day: d, edited: false, deleted: false });
        }

        // Edits and deletes of messages in closed days (yesterday's compaction closed days up to
        // d-9): A edits one of day d-9 and deletes one of day d-12; B edits one of its own of d-10.
        if d >= 12 {
            if let Some(s) = ledger.iter_mut().find(|s| s.dev == Dev::A && s.day == d - 9 && !s.edited && !s.deleted) {
                a.node.edit_message(b.did.clone(), s.id.clone(), format!("a d{} edited on d{d}", s.day)).unwrap();
                s.edited = true;
            }
            if let Some(s) = ledger.iter_mut().find(|s| s.dev == Dev::A && s.day == d - 12 && !s.deleted) {
                a.node.delete_message(b.did.clone(), s.id.clone()).unwrap();
                s.deleted = true;
            }
            if let Some(s) = ledger.iter_mut().find(|s| s.dev == Dev::B && s.day == d - 10 && !s.edited && !s.deleted) {
                b.node.edit_message(a.did.clone(), s.id.clone(), format!("b d{} edited on d{d}", s.day)).unwrap();
                s.edited = true;
            }
        }
        if d % 5 == 0 {
            let _ = b.node.mark_read(a.did.clone());
            let _ = a.node.mark_read(b.did.clone());
        }

        // Everyone who is up agrees.
        if up[0] && up[1] {
            eventually(240, &format!("day {d}: A and B agree and everything is delivered"), || {
                state(&a, &b.did) == state(&b, &a.did) && mine_delivered(&a, &b.did) && mine_delivered(&b, &a.did)
            });
        }
        if up[0] && up[2] {
            eventually(240, &format!("day {d}: A2 agrees with A"), || state(&a2, &b.did) == state(&a, &b.did));
        }
        for (i, t) in came_up {
            reconnects.push((d, names[i], t.elapsed()));
            println!("LONGRUN day {d}: {} back, agreed after {} ms", names[i], ms(t.elapsed()));
        }

        // Compaction on every device that is up.
        for i in 0..3 {
            if up[i] {
                let t = Instant::now();
                let closed = peers[i].node.compact_now_for_test().unwrap();
                compactions.push(t.elapsed());
                if i == 0 {
                    closed_a = closed;
                }
            }
        }
        if d % 10 == 9 || d == n - 1 {
            let (redb, bf, bb) = sizes(&a);
            println!("LONGRUN day {d}: {} messages, A closed {closed_a} days, chat.redb {redb} B, blobs {bf} files {bb} B, {} s so far", ledger.len(), t_run.elapsed().as_secs());
        }
    }

    // Everyone up, final agreement.
    for i in 0..3 {
        if !up[i] {
            online(&peers[i].node);
            up[i] = true;
        }
    }
    eventually(240, "final: A and B agree and everything is delivered", || {
        state(&a, &b.did) == state(&b, &a.did) && mine_delivered(&a, &b.did) && mine_delivered(&b, &a.did)
    });
    eventually(240, "final: A2 agrees", || state(&a2, &b.did) == state(&a, &b.did));
    let want = state(&a, &b.did);
    assert_eq!(want.len(), ledger.len() + 1, "every message written (and the warm-up), once");
    for s in &ledger {
        let m = find(&b, &a.did, &s.id).or_else(|| find(&a, &b.did, &s.id)).expect("held");
        assert_eq!(m.deleted, s.deleted, "{}: deleted", s.id);
        if s.edited && !s.deleted {
            assert!(m.text.contains("edited on"), "{}: shows the edit, not {:?}", s.id, m.text);
        }
    }
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
