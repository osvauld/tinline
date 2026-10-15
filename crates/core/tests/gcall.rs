//! GA-P4/GA-P5: group calls (G6) on real nodes over loopback, no relay. Membership and permits
//! come from one shared `StaticDirectory` per test; every member's microphone is a synthetic
//! tone fed through `Node::push_mic`, and its speaker is read with `Node::pull_speaker`, at
//! 50 Hz as the platform audio threads do.
//!
//! - `group_call_on_real_nodes`: everyone joins (the host's own user through ConnId::LOCAL);
//!   each member hears the other speakers' tones and not its own; with four speakers the host
//!   forwards all four (K = 12, every speaker); a member leaves and everyone re-rosters;
//!   the host `stop`s (End{host_leaving, next_host}) and the members redial backup 1 at once;
//!   backup 1 then crashes without any close and backup 2 notices by missed heartbeats.
//! - `phone_host_degrades`: `gcall_set_cond` on a phone host lowers K to 1 and refuses Joins,
//!   then ends the call with `next_host`, and the members go on at the backup.
//! - `glare_two_hosts`: two calls started at once for one group settle on the lower
//!   BLAKE3(call_id); the loser ends `superseded` and everyone ends up in the winner's call.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::gcall::net::{GcallConfig, GcallEvent, GcallEvents, StaticDirectory};
use p2pcore::gcall::{ClientEnd, ClientState, Cond, EndReason, HEARTBEAT_MISSES, HEARTBEAT_MS, HostConfig, HostNotice, HostRef};
use p2pcore::Node;

mod support;
use support::*;

const FREQ: [f32; 4] = [440.0, 620.0, 830.0, 1070.0];
const AMP: f32 = 8_000.0;
/// Every host forwards every speaker (Q2, 2026-10-14).
const K: usize = 12;

struct Tap(Mutex<mpsc::Sender<GcallEvent>>);

impl GcallEvents for Tap {
    fn on_gcall_event(&self, ev: GcallEvent) {
        let _ = self.0.lock().unwrap().send(ev);
    }
}

struct M {
    node: Arc<Node>,
    rx: mpsc::Receiver<GcallEvent>,
    evs: Vec<GcallEvent>,
    did: String,
    href: HostRef,
    tone: audio::Tone,
    speaking: bool,
    rec: Vec<i16>,
    _dir: TempDir,
}

/// Member `i` of a test's group; it holds a `host_call` permit (and is configured as a host)
/// when `host`.
fn member(tag: &str, i: usize, dir: &Arc<StaticDirectory>, host: bool, cond: Cond) -> M {
    let path = TempDir::new("test-gcall", &format!("{tag}-{i}"));
    let node = Node::new(path.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    node.create_identity(format!("m{i}"), PASS.into()).unwrap();
    node.start().unwrap();
    let did = node.profile().unwrap().did;
    let addr = node.gcall_loopback_addr().unwrap();
    let href = HostRef { holder_did: did.clone(), device_slot: i as u64, permit_digest: format!("permit-{tag}-{i}") };
    {
        let mut a = dir.auth.lock();
        a.members.insert(did.clone());
        a.endpoints.entry(did.clone()).or_default().insert(*addr.id.as_bytes());
        if host {
            a.permits.insert(href.permit_digest.clone());
        }
    }
    dir.hosts.lock().insert(href.clone(), addr);
    let (tx, rx) = mpsc::channel();
    node.gcall_configure(GcallConfig { dir: dir.clone(), events: Arc::new(Tap(Mutex::new(tx))), host_ref: host.then(|| href.clone()), cond, note: None });
    M { node, rx, evs: vec![], did, href, tone: audio::Tone::new(FREQ[i], AMP), speaking: false, rec: vec![], _dir: path }
}

fn host_cfg(m: &M, gid: &str, backups: Vec<HostRef>, cond: Cond) -> HostConfig {
    HostConfig {
        gid: gid.into(),
        call_id: proto::gcall::new_call_id(),
        host_gen: 1,
        host_ref: m.href.clone(),
        starter_did: m.did.clone(),
        backups,
        meta_frontier: "frontier".into(),
        cond,
        own_user_in: true,
    }
}

/// Runs the members' audio for `ms` at 20 ms per frame (recording speaker output if `rec`),
/// stopping early once `until` holds.
fn drive(ms: &mut [&mut M], dur: u64, rec: bool, mut until: impl FnMut(&[&mut M]) -> bool) -> bool {
    let start = Instant::now();
    let mut next = start;
    while start.elapsed() < Duration::from_millis(dur) {
        for m in ms.iter_mut() {
            let mut f = [0i16; audio::FRAME];
            if m.speaking {
                m.tone.next_frame(&mut f);
            }
            m.node.push_mic(f.to_vec());
            let out = m.node.pull_speaker();
            if rec {
                m.rec.extend(out);
            }
            m.evs.extend(m.rx.try_iter());
        }
        if until(ms) {
            return true;
        }
        next += Duration::from_millis(20);
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    false
}

fn all(m: &mut [M]) -> Vec<&mut M> {
    m.iter_mut().collect()
}

/// Amplitude of the `freq` component (Goertzel).
fn amplitude_at(x: &[i16], freq: f32) -> f32 {
    let w = std::f64::consts::TAU * freq as f64 / audio::RATE as f64;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s0 = v as f64 + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let p = s1 * s1 + s2 * s2 - c * s1 * s2;
    (2.0 * p.sqrt() / x.len() as f64) as f32
}

fn last_second(m: &M) -> &[i16] {
    &m.rec[m.rec.len().saturating_sub(audio::RATE as usize)..]
}

fn client_gen(m: &M) -> Option<(ClientState, u32, HostRef, usize)> {
    let c = m.node.gcall_status()?.client?;
    Some((c.state, c.host_gen, c.host, c.roster.len()))
}

/// Joined at `host_gen` with `host`, in a Roster of `n`.
fn joined(m: &M, host_gen: u32, host: &HostRef, n: usize) -> bool {
    client_gen(m).is_some_and(|(s, g, h, r)| s == ClientState::Joined && g == host_gen && h == *host && r == n)
}

fn hosting(m: &M, host_gen: u32, n: usize) -> bool {
    m.node.gcall_status().and_then(|s| s.hosting).is_some_and(|h| h.host_gen == host_gen && h.roster.len() == n)
}

/// `who` hears exactly the tones of `speakers` other than its own.
fn assert_hears(ms: &[&mut M], idx: &[usize], speakers: &[usize]) {
    for (m, &i) in ms.iter().zip(idx) {
        let x = last_second(m);
        for &s in speakers {
            let a = amplitude_at(x, FREQ[s]);
            if s == i {
                assert!(a < 300.0, "member {i} hears its own tone ({a:.0})");
            } else {
                assert!(a > 1_000.0, "member {i} does not hear member {s} ({a:.0})");
            }
        }
    }
}

/// Stops `m`'s process abruptly: no QUIC close, no End. `m` must hold the only `Node` handle.
fn crash(m: M) {
    let M { node, .. } = m;
    Arc::try_unwrap(node).ok().expect("sole node handle").crash_for_test();
}

#[test]
fn group_call_on_real_nodes() {
    let t0 = Instant::now();
    let dir = Arc::new(StaticDirectory::default());
    let mut m: Vec<M> = (0..4).map(|i| member("main", i, &dir, i < 3, Cond::desktop())).collect();
    let gid = "a1".repeat(32);

    // ---- host (with its own user in) and joins -------------------------------------------
    let start = m[0].node.gcall_host(host_cfg(&m[0], &gid, vec![m[1].href.clone(), m[2].href.clone()], Cond::desktop())).unwrap();
    let host_addr = dir.hosts.lock()[&m[0].href].clone();
    for x in &m[1..] {
        x.node.gcall_join(&start, Some(host_addr.clone())).unwrap();
    }
    let h0 = m[0].href.clone();
    // The session updates its state under its lock and delivers the events just after it, so a
    // status can show a change whose event is not in the channel yet (a preempted session
    // thread: load, a single core). Every wait below therefore also waits for its events.
    let joined_ev = |x: &M, g: u32, h: &HostRef| x.evs.iter().any(|e| matches!(e, GcallEvent::Joined { host_gen, host, .. } if *host_gen == g && host == h));
    assert!(drive(&mut all(&mut m), 10_000, false, |ms| ms.iter().all(|x| joined(x, 1, &h0, 4) && joined_ev(x, 1, &h0))), "everyone joins");
    for x in &m {
        assert!(x.evs.iter().any(|e| matches!(e, GcallEvent::Joined { host_gen: 1, host, .. } if *host == m[0].href)), "Joined event");
    }
    assert!(m[0].evs.iter().any(|e| matches!(e, GcallEvent::HostingStarted { host_gen: 1, .. })));
    let hs = m[0].node.gcall_status().unwrap().hosting.unwrap();
    assert_eq!((hs.k, hs.roster.len()), (K, 4));
    eprintln!("joined after {:?}", t0.elapsed());

    // ---- three speakers: each member hears the others ------------------------------------
    for (i, x) in m.iter_mut().enumerate() {
        x.speaking = i < 3;
        x.rec.clear();
    }
    drive(&mut all(&mut m), 2_500, true, |_| false);
    assert_hears(&all(&mut m), &[0, 1, 2, 3], &[0, 1, 2]);

    // ---- four speakers: every one forwarded (K = 12), each hears the other three ------------
    for x in m.iter_mut() {
        x.speaking = true;
        x.rec.clear();
    }
    let mut most = 0;
    let host = m[0].node.clone();
    drive(&mut all(&mut m), 1_500, false, |_| {
        most = most.max(host.gcall_status().unwrap().hosting.unwrap().forwarded.len());
        false
    });
    let heard = |x: &M| x.node.gcall_status().unwrap().client.unwrap().heard;
    let before: Vec<BTreeMap<u8, u64>> = m.iter().map(heard).collect();
    drive(&mut all(&mut m), 600, false, |_| false);
    let fwd = host.gcall_status().unwrap().hosting.unwrap().forwarded;
    assert!(most <= 4, "forwarded {most} > 4 speakers");
    assert_eq!(fwd.len(), 4, "every speaker forwarded");
    for (x, b) in m.iter().zip(&before) {
        let now = heard(x);
        let mine = x.node.gcall_status().unwrap().client.unwrap().my_slot;
        let live: Vec<u8> = now.iter().filter(|(s, n)| **n > b.get(s).copied().unwrap_or(0)).map(|(s, _)| *s).collect();
        let expect: Vec<u8> = fwd.iter().copied().filter(|s| Some(*s) != mine).collect();
        assert_eq!(live, expect, "a member receives exactly the forwarded set minus itself");
        assert_eq!(live.len(), 3);
    }

    // ---- a member leaves: everyone re-rosters ----------------------------------------------
    m[3].node.gcall_leave();
    let rostered = |x: &M| x.evs.iter().any(|e| matches!(e, GcallEvent::RosterChanged { roster, .. } if roster.len() == 3));
    let left = |ms: &[&mut M]| ms[..3].iter().all(|x| client_gen(x).is_some_and(|(_, _, _, n)| n == 3) && rostered(x));
    assert!(drive(&mut all(&mut m), 5_000, false, left), "re-roster after leave");
    assert!(m[3].evs.iter().any(|e| matches!(e, GcallEvent::Ended { why: ClientEnd::Left, .. })));
    for x in &m[..3] {
        assert!(x.evs.iter().any(|e| matches!(e, GcallEvent::RosterChanged { roster, .. } if roster.len() == 3)));
    }
    assert_eq!(host.gcall_status().unwrap().hosting.unwrap().roster.len(), 3);
    drop(host);
    drop(m.remove(3));

    // ---- the host stops: End{host_leaving, next_host: backup 1}, redial without waiting ----
    let t_stop = Instant::now();
    m[0].node.stop();
    let mut stopped = m.remove(0);
    stopped.evs.extend(stopped.rx.try_iter());
    assert!(
        stopped.evs.iter().any(|e| matches!(e, GcallEvent::HostingEnded { reason: EndReason::HostLeaving, next_host: Some(n), .. } if *n == m[0].href)),
        "stop ends hosting with next_host"
    );
    assert!(stopped.evs.iter().any(|e| matches!(e, GcallEvent::Ended { why: ClientEnd::Left, .. })), "stop leaves");
    assert!(stopped.node.gcall_status().is_none());
    let h1 = m[0].href.clone();
    let moved = |ms: &[&mut M]| hosting(ms[0], 2, 2) && ms.iter().all(|x| joined(x, 2, &h1, 2));
    assert!(drive(&mut all(&mut m), 5_000, false, moved), "backup 1 takes over at host_gen 2");
    let took = t_stop.elapsed();
    eprintln!("planned handover after {took:?}");
    assert!(took < Duration::from_millis(HEARTBEAT_MISSES * HEARTBEAT_MS), "redial on End, not on missed heartbeats ({took:?})");
    drop(stopped);
    for x in m.iter_mut() {
        x.rec.clear();
    }
    drive(&mut all(&mut m), 1_500, true, |_| false);
    assert_hears(&all(&mut m), &[1, 2], &[1, 2]);

    // ---- backup 1 crashes (no close at all): backup 2 notices by 3 missed heartbeats -------
    let b2 = &mut m[1];
    b2.evs.extend(b2.rx.try_iter());
    b2.evs.clear(); // its Reconnecting from the planned handover
    // Still on backup 1: a host loss before the crash (a starved host under load) would make
    // the takeover below prove nothing.
    assert!(joined(&m[1], 2, &h1, 2), "backup 2 still joined to backup 1 before the crash");
    let t_crash = Instant::now();
    crash(m.remove(0));
    let mut detected = None;
    let h2 = m[0].href.clone();
    let took_over = drive(&mut all(&mut m), 8_000, false, |ms| {
        if detected.is_none() && ms[0].evs.iter().any(|e| matches!(e, GcallEvent::Reconnecting { .. })) {
            detected = Some(t_crash.elapsed());
        }
        // Not done before the Reconnecting event is in (see the joins above).
        detected.is_some() && hosting(ms[0], 3, 1) && joined(ms[0], 3, &h2, 1)
    });
    let detected = detected.expect("host loss detected");
    eprintln!("crash detected after {detected:?}");
    // The last heartbeat came at most one period before the crash.
    let bound = HEARTBEAT_MISSES * HEARTBEAT_MS;
    assert!(detected >= Duration::from_millis(bound - HEARTBEAT_MS - 100), "detected by a close, not by heartbeats ({detected:?})");
    // `detected` is when this thread saw the event (20 ms polling plus scheduling, which under
    // load reaches hundreds of ms), so allow a second past the bound. A host lost any other way
    // than by heartbeats shows up only at the 20 s QUIC idle timeout, far beyond either bound.
    assert!(detected <= Duration::from_millis(bound + 1_000), "detection beyond the §5 bound ({detected:?})");
    assert!(took_over, "backup 2 takes over at host_gen 3");
    eprintln!("total {:?}", t0.elapsed());
}

#[test]
fn phone_host_degrades() {
    let dir = Arc::new(StaticDirectory::default());
    let phone = Cond::phone(80, false);
    let mut m: Vec<M> = (0..4).map(|i| member("phone", i, &dir, i < 2, if i == 0 { phone } else { Cond::desktop() })).collect();
    let gid = "c3".repeat(32);
    let start = m[0].node.gcall_host(host_cfg(&m[0], &gid, vec![m[1].href.clone()], phone)).unwrap();
    for x in &m[1..3] {
        x.node.gcall_join(&start, None).unwrap();
    }
    let h0 = m[0].href.clone();
    assert!(drive(&mut all(&mut m), 10_000, false, |ms| ms[..3].iter().all(|x| joined(x, 1, &h0, 3))), "joins");
    assert_eq!(m[0].node.gcall_status().unwrap().hosting.unwrap().k, K, "a phone host forwards every speaker");

    // Battery under 15 %: limited, K = 1, new Joins refused.
    m[0].node.gcall_set_cond(Cond::phone(12, false));
    let fresh: Vec<GcallEvent> = m[0].rx.try_iter().collect();
    m[0].evs.extend(fresh);
    assert!(m[0].evs.iter().any(|e| matches!(e, GcallEvent::Notice(HostNotice::Limited))));
    assert_eq!(m[0].node.gcall_status().unwrap().hosting.unwrap().k, 1, "limited K");
    m[3].node.gcall_join(&start, None).unwrap();
    let refused = |ms: &[&mut M]| ms[3].evs.iter().any(|e| matches!(e, GcallEvent::Ended { why: ClientEnd::HostLost, .. }));
    assert!(drive(&mut all(&mut m), 6_000, false, refused), "the late Join is refused");
    assert_eq!(m[0].node.gcall_status().unwrap().hosting.unwrap().roster.len(), 3);

    // Battery under 8 %: End{host_leaving, next_host: backup}; everyone goes on there.
    m[0].node.gcall_set_cond(Cond::phone(5, false));
    let h1 = m[1].href.clone();
    let moved = |ms: &[&mut M]| hosting(ms[1], 2, 3) && ms[..3].iter().all(|x| joined(x, 2, &h1, 3));
    assert!(drive(&mut all(&mut m), 5_000, false, moved), "members move to the backup");
    assert!(m[0].evs.iter().any(|e| matches!(e, GcallEvent::HostingEnded { reason: EndReason::HostLeaving, next_host: Some(n), .. } if *n == h1)));
}

#[test]
fn glare_two_hosts() {
    let dir = Arc::new(StaticDirectory::default());
    let mut m: Vec<M> = (0..4).map(|i| member("glare", i, &dir, i < 2, Cond::desktop())).collect();
    let gid = "d4".repeat(32);
    // Two members start a call at once; one more member joins each.
    let a = m[0].node.gcall_host(host_cfg(&m[0], &gid, vec![], Cond::desktop())).unwrap();
    let b = m[1].node.gcall_host(host_cfg(&m[1], &gid, vec![], Cond::desktop())).unwrap();
    m[2].node.gcall_join(&a, None).unwrap();
    m[3].node.gcall_join(&b, None).unwrap();
    let (h0, h1) = (m[0].href.clone(), m[1].href.clone());
    let two = |ms: &[&mut M]| joined(ms[0], 1, &h0, 2) && joined(ms[2], 1, &h0, 2) && joined(ms[1], 1, &h1, 2) && joined(ms[3], 1, &h1, 2);
    assert!(drive(&mut all(&mut m), 10_000, false, two), "two calls");

    // Both CallStarts reach everyone through the chat relay, in no particular order.
    for x in &m {
        for s in [&a, &b] {
            x.node.gcall_observe_start(s).unwrap();
        }
    }
    let id = |s: &proto::SignedBlob| proto::gcall::open_start(s).unwrap().call_id;
    let (ia, ib) = (id(&a), id(&b));
    let a_wins = blake3::hash(ia.as_bytes()).as_bytes() < blake3::hash(ib.as_bytes()).as_bytes();
    let (win, lose, wid) = if a_wins { (0, 1, ia) } else { (1, 0, ib) };
    let wh = m[win].href.clone();
    let settled = |ms: &[&mut M]| {
        hosting(ms[win], 1, 4) && ms.iter().all(|x| joined(x, 1, &wh, 4) && x.node.gcall_status().is_some_and(|s| s.call_id == wid))
    };
    assert!(drive(&mut all(&mut m), 10_000, false, settled), "everyone in the winner's call");
    assert!(m[lose].node.gcall_status().unwrap().hosting.is_none());
    assert!(
        m[lose].evs.iter().any(|e| matches!(e, GcallEvent::HostingEnded { reason: EndReason::Superseded, next_host: Some(n), .. } if *n == wh)),
        "the loser ends superseded"
    );
    let follower = if lose == 0 { 2 } else { 3 };
    for i in [lose, follower] {
        assert!(m[i].evs.iter().any(|e| matches!(e, GcallEvent::Superseded { by, .. } if *by == wh)), "member {i} told to follow");
    }
}
