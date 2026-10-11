//! GA-P4: group calls (G6) on four real nodes over loopback, no relay. Membership and permits
//! come from one shared `StaticDirectory`; every member's microphone is a synthetic tone fed
//! through `Node::push_mic`, and its speaker is read with `Node::pull_speaker`, at 50 Hz as
//! the platform audio threads do.
//!
//! One test, in phases: everyone joins (the host's own user through ConnId::LOCAL); each member
//! hears the other speakers' tones and not its own; with four speakers and K = 3 the host
//! forwards at most K + 1 and settles on K; a member leaves and everyone re-rosters; the host
//! dies and the members redial the backup, which takes over at host_gen 2 and carries audio.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::gcall::net::{GcallConfig, GcallEvent, GcallEvents, StaticDirectory};
use p2pcore::gcall::{ClientEnd, ClientState, Cond, HostConfig, HostRef};
use p2pcore::{CallInfo, CallState, Node, NodeEvents, NodeStatus};

const PASS: &str = "test-passphrase";
const FREQ: [f32; 4] = [440.0, 620.0, 830.0, 1070.0];
const AMP: f32 = 8_000.0;
const K: usize = 3;

struct Quiet;

impl NodeEvents for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}

struct Tap(Mutex<mpsc::Sender<GcallEvent>>);

impl GcallEvents for Tap {
    fn on_gcall_event(&self, ev: GcallEvent) {
        let _ = self.0.lock().unwrap().send(ev);
    }
}

struct Dir(std::path::PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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
    _dir: Dir,
}

fn member(i: usize, dir: &Arc<StaticDirectory>) -> M {
    let path = std::env::temp_dir().join(format!("p2pcore-test-gcall-{i}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    let node = Node::new(path.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    node.create_identity(format!("m{i}"), PASS.into()).unwrap();
    node.start().unwrap();
    let did = node.profile().unwrap().did;
    let addr = node.gcall_loopback_addr().unwrap();
    let href = HostRef { holder_did: did.clone(), device_slot: i as u64, permit_digest: format!("permit-{i}") };
    {
        let mut a = dir.auth.lock();
        a.members.insert(did.clone());
        a.endpoints.entry(did.clone()).or_default().insert(*addr.id.as_bytes());
        if i < 2 {
            a.permits.insert(href.permit_digest.clone());
        }
    }
    dir.hosts.lock().insert(href.clone(), addr);
    let (tx, rx) = mpsc::channel();
    node.gcall_configure(GcallConfig {
        dir: dir.clone(),
        events: Arc::new(Tap(Mutex::new(tx))),
        host_ref: (i < 2).then(|| href.clone()),
        cond: Cond::desktop(),
        note: None,
    });
    M { node, rx, evs: vec![], did, href, tone: audio::Tone::new(FREQ[i], AMP), speaking: false, rec: vec![], _dir: Dir(path) }
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

#[test]
fn group_call_on_real_nodes() {
    let t0 = Instant::now();
    let dir = Arc::new(StaticDirectory::default());
    let mut m: Vec<M> = (0..4).map(|i| member(i, &dir)).collect();
    let gid = "a1".repeat(32);

    // ---- host (with its own user in) and joins -------------------------------------------
    let start = m[0]
        .node
        .gcall_host(HostConfig {
            gid: gid.clone(),
            call_id: proto::gcall::new_call_id(),
            host_gen: 1,
            host_ref: m[0].href.clone(),
            starter_did: m[0].did.clone(),
            backups: vec![m[1].href.clone()],
            meta_frontier: "frontier".into(),
            cond: Cond::desktop(),
            own_user_in: true,
        })
        .unwrap();
    let host_addr = dir.hosts.lock()[&m[0].href].clone();
    for x in &m[1..] {
        x.node.gcall_join(&start, Some(host_addr.clone())).unwrap();
    }
    let all_in = |ms: &[&mut M]| ms.iter().all(|x| client_gen(x).is_some_and(|(s, g, _, n)| s == ClientState::Joined && g == 1 && n == 4));
    assert!(drive(&mut m.iter_mut().collect::<Vec<_>>(), 10_000, false, all_in), "everyone joins");
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
    drive(&mut m.iter_mut().collect::<Vec<_>>(), 2_500, true, |_| false);
    assert_hears(&m.iter_mut().collect::<Vec<_>>(), &[0, 1, 2, 3], &[0, 1, 2]);

    // ---- four speakers, K = 3: at most K + 1 forwarded, settling on K ---------------------
    for x in m.iter_mut() {
        x.speaking = true;
        x.rec.clear();
    }
    let mut most = 0;
    let host = m[0].node.clone();
    drive(&mut m.iter_mut().collect::<Vec<_>>(), 1_500, false, |_| {
        most = most.max(host.gcall_status().unwrap().hosting.unwrap().forwarded.len());
        false
    });
    let heard = |x: &M| x.node.gcall_status().unwrap().client.unwrap().heard;
    let before: Vec<BTreeMap<u8, u64>> = m.iter().map(heard).collect();
    drive(&mut m.iter_mut().collect::<Vec<_>>(), 600, false, |_| false);
    let fwd = host.gcall_status().unwrap().hosting.unwrap().forwarded;
    assert!(most <= K + 1, "forwarded {most} > K + 1");
    assert_eq!(fwd.len(), K, "settles on K speakers");
    for (x, b) in m.iter().zip(&before) {
        let now = heard(x);
        let mine = x.node.gcall_status().unwrap().client.unwrap().my_slot;
        let live: Vec<u8> = now.iter().filter(|(s, n)| **n > b.get(s).copied().unwrap_or(0)).map(|(s, _)| *s).collect();
        let expect: Vec<u8> = fwd.iter().copied().filter(|s| Some(*s) != mine).collect();
        assert_eq!(live, expect, "a member receives exactly the forwarded set minus itself");
        assert!(live.len() <= K);
    }

    // ---- a member leaves: everyone re-rosters ----------------------------------------------
    m[3].node.gcall_leave();
    let left = |ms: &[&mut M]| ms[..3].iter().all(|x| client_gen(x).is_some_and(|(_, _, _, n)| n == 3));
    assert!(drive(&mut m.iter_mut().collect::<Vec<_>>(), 5_000, false, left), "re-roster after leave");
    assert!(m[3].evs.iter().any(|e| matches!(e, GcallEvent::Ended { why: ClientEnd::Left, .. })));
    for x in &m[..3] {
        assert!(x.evs.iter().any(|e| matches!(e, GcallEvent::RosterChanged { roster, .. } if roster.len() == 3)));
    }
    assert_eq!(host.gcall_status().unwrap().hosting.unwrap().roster.len(), 3);
    drop(host);
    let gone = m.remove(3);
    drop(gone);

    // ---- the host dies: members redial the backup, which takes over at gen 2 -------------
    let dead = m.remove(0);
    let t_kill = Instant::now();
    drop(dead);
    let took_over = |ms: &[&mut M]| {
        let backup = ms[0].node.gcall_status().and_then(|s| s.hosting).is_some_and(|h| h.host_gen == 2 && h.roster.len() == 2);
        backup && ms.iter().all(|x| client_gen(x).is_some_and(|(s, g, h, n)| s == ClientState::Joined && g == 2 && h == ms[0].href && n == 2))
    };
    assert!(drive(&mut m.iter_mut().collect::<Vec<_>>(), 15_000, false, took_over), "backup takes over at host_gen 2");
    eprintln!("takeover after {:?}", t_kill.elapsed());
    assert!(m[0].evs.iter().any(|e| matches!(e, GcallEvent::HostingStarted { host_gen: 2, .. })));
    assert!(m[1].evs.iter().any(|e| matches!(e, GcallEvent::Reconnecting { .. })));
    assert!(m[1].evs.iter().any(|e| matches!(e, GcallEvent::Joined { host_gen: 2, .. })));
    for x in m.iter_mut() {
        x.rec.clear();
    }
    drive(&mut m.iter_mut().collect::<Vec<_>>(), 2_000, true, |_| false);
    assert_hears(&m.iter_mut().collect::<Vec<_>>(), &[1, 2], &[1, 2]);
    eprintln!("total {:?}", t0.elapsed());
}
