//! A long run of one group on real nodes (`support::Peer`, loopback iroh): N days
//! (`GROUP_LONGRUN_DAYS`, default 30) of posts from the owner/host O, a second host H, member A
//! on two devices (A, A2) and member B on two devices (B, B2). Each has days away and keeps
//! posting while away (B2 is away for a week right after B's own gap, so B's posts of that gap
//! reach B2 only through the hosts); O
//! restarts once and rotates its group endpoint once; the group compacts on every device every
//! day. After every reconnect every online device must show the same posts. At the end O
//! restarts again (closed days come back from the store), a new member C joins and must backfill
//! the whole history, and a fresh device A3 of A joins with a device token and must hold it too.
//! `LONGRUN ...` lines carry the measurements (run with `--nocapture`).
//!
//! Days are backdated with `group_post_at_for_test` (there is no group clock hook): day `d` of
//! `N` is `N-1-d` days ago. Compaction judges age by the real clock, so a day older than 8 days
//! closes at the end of its own simulated day; a post that arrives for it later (written while
//! away) reopens it, and the next pass closes it again, as a late arrival does in production.
//! Group posts have no edits or deletes yet, so none are simulated.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{GroupItemKind, Node, RotationPolicy};

use crate::support;
use support::group::*;
use support::now_ms_i64 as now_ms;
use support::*;

const S: Suite = Suite("glong");

const DAY: i64 = 86_400_000;
const HOUR: i64 = 3_600_000;

fn days() -> i64 {
    std::env::var("GROUP_LONGRUN_DAYS").ok().and_then(|v| v.parse().ok()).filter(|n| *n >= 14).unwrap_or(30)
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

/// Index into the device arrays: O, H, A, A2, B, B2.
const O: usize = 0;
const H: usize = 1;
const A: usize = 2;
const A2: usize = 3;
const B: usize = 4;
const B2: usize = 5;
const NAMES: [&str; 6] = ["O", "H", "A", "A2", "B", "B2"];

/// One post: who wrote it and whether every online device has agreed on it since.
struct Sent {
    text: String,
    dev: usize,
    synced: bool,
}

/// The post texts of `g` as `p` shows them, oldest first.
fn texts(p: &Peer, g: &str) -> Vec<String> {
    all(p, g).into_iter().filter(|m| m.kind == GroupItemKind::Post).map(|m| m.text).collect()
}

fn sorted_texts(p: &Peer, g: &str) -> Vec<String> {
    let mut v = texts(p, g);
    v.sort();
    v
}

/// Restarts `p`'s process (a new `Node` over the same directory), unlocked and online.
fn restart(p: &mut Peer) {
    p.node.stop();
    std::thread::sleep(Duration::from_secs(2));
    let node = Node::new(p.dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    let (ltx, lrx) = mpsc::channel();
    node.set_link_events(Arc::new(LTap::new(ltx)));
    p.lrx = lrx;
    let old = std::mem::replace(&mut p.node, node);
    drop(old);
    p.node.unlock(PASS.into()).unwrap();
    online(&p.node);
}

fn account_dir(p: &Peer) -> Option<PathBuf> {
    std::fs::read_dir(p.dir.join("accounts")).ok()?.flatten().map(|e| e.path()).find(|e| e.is_dir())
}

fn group_redb_bytes(p: &Peer) -> u64 {
    account_dir(p).and_then(|d| std::fs::metadata(d.join("group.redb")).ok()).map(|m| m.len()).unwrap_or(0)
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

/// Appoints `h` as a second host of `g` and waits until everyone in `all_peers` sees two hosts and
/// every member knows h's group endpoint.
fn appoint_host(o: &Peer, h: &Peer, g: &str, all_peers: &[&Peer]) {
    o.node.group_appoint_host(g.to_string(), h.did.clone(), h.node.group_device_id().unwrap()).unwrap();
    eventually(WAIT, "h sees the permit", || all(h, g).iter().any(|x| x.kind == GroupItemKind::HostAdded && x.subject_did == h.did));
    h.node.group_accept_host(g.to_string(), true).unwrap();
    for p in all_peers {
        eventually(WAIT, "two hosts everywhere", || p.node.group_detail(g.to_string()).unwrap().hosts.len() == 2);
    }
    let he = h.node.group_endpoint_id().unwrap();
    for p in all_peers {
        if p.did != o.did && p.did != h.did {
            eventually(WAIT, "members learn h", || p.node.group_known_endpoints_for_test(g.to_string()).contains(&he));
        }
    }
}

// ---- the long run -------------------------------------------------------------------------------

#[test]
#[ignore = "long simulation, not for CI: run by hand (loadtable step chat_longrun / group_longrun, --include-ignored)"]
fn a_group_over_many_days_with_absences_a_host_restart_and_rotation_converges() {
    let n = days();
    let t_run = Instant::now();

    // O creates the group; H, A and B join; A2 and B2 link as second devices and join with a
    // device token; H becomes the second host.
    let mut o = S.peer("o");
    let h = S.peer("h");
    let a = S.peer("a");
    let b = S.peer("b");
    let g = group_with(&o, &[&h, &a, &b]);
    let t_link = Instant::now();
    let a2 = S.link_desk(&a, "a2");
    let tok = a.node.group_device_token(g.clone()).unwrap();
    assert_eq!(join(&a2, tok), g);
    eventually(WAIT, "A2 sees the members", || members(&a2, &g) == members(&a, &g));
    println!("LONGRUN link: A2 linked and joined after {} ms", ms(t_link.elapsed()));
    let t_link = Instant::now();
    let b2 = S.link_desk(&b, "b2");
    let tok = b.node.group_device_token(g.clone()).unwrap();
    assert_eq!(join(&b2, tok), g);
    eventually(WAIT, "B2 sees the members", || members(&b2, &g) == members(&b, &g));
    println!("LONGRUN link: B2 linked and joined after {} ms", ms(t_link.elapsed()));
    appoint_host(&o, &h, &g, &[&o, &h, &a, &a2, &b, &b2]);
    for p in [&o, &h, &a, &a2, &b, &b2] {
        p.node.group_backfill_pause_ms_for_test(5);
    }

    let today = now_ms() / DAY * DAY;
    let mut up = [true; 6];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut ledger: Vec<Sent> = Vec::new();
    let mut reconnects: Vec<(i64, &str, Duration)> = Vec::new();
    let mut compactions: Vec<Duration> = Vec::new();
    let mut rotated = false;
    let mut restarted = false;
    let old_endpoint = o.node.group_endpoint_id().unwrap();

    for d in 0..n {
        let day_start = today - (n - 1 - d) * DAY;
        // O restarts once, mid-run, before the day's traffic.
        if !restarted && d == n / 2 && up[O] {
            let t = Instant::now();
            restart(&mut o);
            restarted = true;
            println!("LONGRUN day {d}: O restarted in {} ms", ms(t.elapsed()));
        }
        let peers = [&o, &h, &a, &a2, &b, &b2];

        // Who is up today. A host is always up: O stays when H is away and the other way round.
        // B2 is away days 17..=23 (right after B's gap) and every 8th day.
        let h_want = d % 13 != 8;
        let o_want = !(d % 17 == 9) || !h_want;
        let b2_want = !((17..=23).contains(&d) || d % 8 == 6);
        let want = [o_want, h_want, d % 11 != 5, !(12..=18).contains(&d), !(d % 7 == 3 || (10..=16).contains(&d)), b2_want];
        let mut came_up: Vec<(usize, Instant)> = Vec::new();
        for i in 0..6 {
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

        // The host rotates its group endpoint once (a shortened interval), then back to the
        // normal days-long one.
        if !rotated && d >= n / 3 && up[O] {
            let t = Instant::now();
            o.node.group_set_rotation_policy_for_test(Some(RotationPolicy { enabled: true, interval_ms: 15_000, jitter_ms: 0, announce_ms: 600_000 }));
            eventually(WAIT + 15, "O rotated", || o.node.group_endpoint_id().is_some_and(|e| e != old_endpoint));
            o.node.group_set_rotation_policy_for_test(Some(RotationPolicy::ON));
            let new = o.node.group_endpoint_id().unwrap();
            for i in [A, A2, B, B2] {
                if up[i] {
                    eventually(WAIT, &format!("{} follows the rotation", NAMES[i]), || peers[i].node.group_known_endpoints_for_test(g.clone()).contains(&new));
                }
            }
            rotated = true;
            println!("LONGRUN day {d}: O rotated its group endpoint, online members followed, {} ms", ms(t.elapsed()));
        }

        // Posts, backdated into this day, also from devices that are away.
        let post = |i: usize, k: i64, plus: i64, ledger: &mut Vec<Sent>| {
            let text = format!("{} d{d} #{k}", NAMES[i]);
            peers[i].node.group_post_at_for_test(g.clone(), day_start + plus, text.clone()).unwrap();
            ledger.push(Sent { text, dev: i, synced: false });
        };
        for k in 0..1 + rng.below(2) {
            post(O, k, 8 * HOUR + rng.below(14 * HOUR), &mut ledger);
        }
        if d % 4 == 1 {
            post(H, 0, 9 * HOUR + rng.below(12 * HOUR), &mut ledger);
        }
        for k in 0..1 + rng.below(3) {
            post(A, k, 8 * HOUR + rng.below(14 * HOUR), &mut ledger);
        }
        if d % 6 == 2 {
            post(A2, 0, 20 * HOUR, &mut ledger);
        }
        for k in 0..1 + rng.below(3) {
            post(B, k, 8 * HOUR + rng.below(14 * HOUR), &mut ledger);
        }
        if d % 5 == 1 {
            post(B2, 0, 19 * HOUR, &mut ledger);
        }
        if d % 9 == 4 && d < n - 1 {
            post(A, 90, DAY - 30_000, &mut ledger);
            post(A, 91, DAY + 30_000, &mut ledger);
        }

        // Every device that is up shows the same posts: everything written by devices that are
        // up now, plus everything agreed on before.
        let ref_i = if up[O] { O } else { H };
        let mut expected: Vec<String> = ledger.iter().filter(|s| up[s.dev] || s.synced).map(|s| s.text.clone()).collect();
        expected.sort();
        eventually(WAIT + 30, &format!("day {d}: every online device shows the same posts"), || {
            let want = texts(peers[ref_i], &g);
            let mut set = want.clone();
            set.sort();
            set == expected && (0..6).filter(|i| up[*i]).all(|i| texts(peers[i], &g) == want)
        });
        for s in ledger.iter_mut() {
            if up[s.dev] {
                s.synced = true;
            }
        }
        for (i, t) in came_up {
            reconnects.push((d, NAMES[i], t.elapsed()));
            println!("LONGRUN day {d}: {} back, agreed after {} ms", NAMES[i], ms(t.elapsed()));
        }

        // Compaction on every device that is up.
        for i in 0..6 {
            if up[i] {
                let t = Instant::now();
                peers[i].node.group_compact_now_for_test().unwrap();
                compactions.push(t.elapsed());
            }
        }
        if d % 10 == 9 || d == n - 1 {
            let closed = if up[O] { o.node.group_closed_days_for_test(g.clone()).unwrap() } else { 0 };
            println!("LONGRUN day {d}: {} posts, O closed {closed} days, O group.redb {} B, {} s so far", ledger.len(), group_redb_bytes(&o), t_run.elapsed().as_secs());
        }
    }

    // Everyone up, final agreement.
    {
        let peers = [&o, &h, &a, &a2, &b, &b2];
        for i in 0..6 {
            if !up[i] {
                online(&peers[i].node);
                up[i] = true;
            }
        }
        let mut expected: Vec<String> = ledger.iter().map(|s| s.text.clone()).collect();
        expected.sort();
        eventually(WAIT + 30, "final: every device shows every post", || {
            let want = texts(&o, &g);
            let mut set = want.clone();
            set.sort();
            set == expected && peers.iter().all(|p| texts(p, &g) == want)
        });
        let days_o = o.node.group_days_for_test(g.clone()).unwrap();
        for p in peers {
            eventually(WAIT, "the same days everywhere", || p.node.group_days_for_test(g.clone()).unwrap() == days_o);
            p.node.group_compact_now_for_test().unwrap();
        }
        let closed_o = o.node.group_closed_days_for_test(g.clone()).unwrap();
        for p in peers {
            assert_eq!(p.node.group_closed_days_for_test(g.clone()).unwrap(), closed_o, "closed days agree");
        }
        println!("LONGRUN end: {} posts over {n} days in {} day docs, {closed_o} closed", ledger.len(), days_o.len());
    }
    let want = texts(&o, &g);

    // O restarts: closed days reload on demand, nothing is lost.
    let t = Instant::now();
    restart(&mut o);
    assert_eq!(texts(&o, &g), want, "O after a restart");
    println!("LONGRUN restart: O reopened and read every day in {} ms", ms(t.elapsed()));

    // A new member joins and backfills the whole history.
    let c = S.peer("c");
    c.node.group_backfill_pause_ms_for_test(5);
    let t = Instant::now();
    let tok = o.node.group_invite(g.clone()).unwrap();
    assert_eq!(join(&c, tok), g);
    eventually(WAIT + 60, "C backfills every day", || texts(&c, &g) == want);
    println!("LONGRUN c: new member holds the whole history ({} days) after {} ms", c.node.group_days_for_test(g.clone()).unwrap().len(), ms(t.elapsed()));

    // A fresh device of A joins with a device token and holds the whole history.
    let t = Instant::now();
    let a3 = S.link_desk(&a, "a3");
    a3.node.group_backfill_pause_ms_for_test(5);
    let tok = a.node.group_device_token(g.clone()).unwrap();
    assert_eq!(join(&a3, tok), g);
    eventually(WAIT + 60, "A3 holds the whole history", || texts(&a3, &g) == want);
    println!("LONGRUN a3: fresh device holds the whole history after {} ms", ms(t.elapsed()));
    assert_eq!(a3.node.group_days_for_test(g.clone()).unwrap(), o.node.group_days_for_test(g.clone()).unwrap());

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
