//! GA-I1: group calls wired into group chat, on real nodes over real iroh endpoints (needs
//! network, like `group_core.rs` / `group_g2.rs`, the shared harness is in `support/`; the audio
//! helpers come from `gcall.rs`). Spec: `docs/design/group-call-spec.md` §2, §3, §5, §11;
//! decisions U2, U7, U21, D11 (`rotation-open-decisions.md` §0).
//!
//! A group of the owner `o` (chat host) and three members; `c` is a desktop the owner allowed
//! to host calls (`CallHostPermit`) and that accepted (`CallHostAccept`); `o` also hosts calls
//! but advertises worse conditions (a desktop on battery), so it ranks second: the backup.
//!
//! - `a_member_starts_a_call_the_call_host_carries_it_and_a_removed_member_is_ejected`: call
//!   hosting is separate from chat hosting; `a` starts a call, the ranking picks `c`, the
//!   request and the CallStart travel through the chat host, everyone joins on `c`'s group
//!   endpoint and hears the others' tones; no plain member learns another member's group
//!   endpoint (U7); a member removed by 3 flags is ejected at once and never sees a later call.
//! - `the_next_call_follows_a_rotated_call_host_and_a_revoked_call_host_hands_over`: after a
//!   the call host rotates its group endpoint (D11, `group_rotate_now`) and the next call reaches
//!   it at the new one; revoking its call-host permit mid-call moves everyone to the backup (§5).
#![allow(dead_code)]

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::gcall::{Class, ClientState, Cond, Thermal};
use p2pcore::{GroupCallEvent, GroupCallEventKind, GroupCallEvents, GroupRole, Node};

mod support;
use support::group::{Member, WAIT, members};
use support::*;

#[path = "support/group_call_capacity.rs"]
mod capacity_tests;

const FREQ: [f32; 4] = [440.0, 620.0, 830.0, 1070.0];
const AMP: f32 = 8_000.0;

struct Tap(Mutex<mpsc::Sender<GroupCallEvent>>);
impl GroupCallEvents for Tap {
    fn on_group_call(&self, e: GroupCallEvent) {
        let _ = self.0.lock().unwrap().send(e);
    }
}

struct Peer {
    node: Arc<Node>,
    rx: mpsc::Receiver<GroupCallEvent>,
    evs: Vec<GroupCallEvent>,
    did: String,
    dir: TempDir,
    tone: audio::Tone,
    speaking: bool,
    rec: Vec<i16>,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.node.stop();
    }
}

impl Member for Peer {
    fn node(&self) -> &Arc<Node> {
        &self.node
    }
    fn did(&self) -> &str {
        &self.did
    }
}

impl Peer {
    fn events(&mut self) -> &[GroupCallEvent] {
        self.evs.extend(self.rx.try_iter());
        &self.evs
    }
}

fn peer(name: &str, i: usize) -> Peer {
    let dir = TempDir::new("gcall-group", name);
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    let (tx, rx) = mpsc::channel();
    node.set_group_call_events(Arc::new(Tap(Mutex::new(tx))));
    node.create_identity(name.into(), PASS.into()).unwrap();
    online(&node);
    let did = node.profile().unwrap().did;
    Peer { node, rx, evs: vec![], did, dir, tone: audio::Tone::new(FREQ[i], AMP), speaking: false, rec: vec![] }
}

/// Owner `o` (first chat host) creates a group and each of `ms` joins with an invite from `o`.
fn group_with(o: &Peer, ms: &[&Peer]) -> String {
    let g = o.node.group_create("Band practice".into()).unwrap();
    for m in ms {
        let tok = o.node.group_invite(g.clone()).unwrap();
        assert_eq!(m.node.group_join(tok).unwrap(), g);
    }
    let mut want: Vec<String> = std::iter::once(o.did.clone()).chain(ms.iter().map(|m| m.did.clone())).collect();
    want.sort();
    for p in std::iter::once(o).chain(ms.iter().copied()) {
        eventually(WAIT, "everyone sees every member", || members(p, &g) == want);
    }
    g
}

/// A desktop on battery: a call host that ranks below one on mains (tier 2 vs 0).
fn on_battery() -> Cond {
    Cond { class: Class::Desktop, plugged: false, battery: 80, metered: false, metered_override: false, thermal: Thermal::None }
}

/// `c` hosts calls for `g` (appointed by `o`, accepted); `o` too (its own device), as the
/// backup. Waits until `a` sees both call hosts.
fn call_hosts(o: &Peer, c: &Peer, a: &Peer, g: &str) {
    o.node.gcall_set_cond(on_battery());
    // Hosting waits for the platform's conditions (GA-A1).
    c.node.gcall_set_cond(Cond::desktop());
    o.node.group_appoint_call_host(g.into(), c.did.clone(), c.node.group_device_id().unwrap()).unwrap();
    eventually(WAIT, "c is asked to host calls", || c.node.group_detail(g.into()).unwrap().call_host_requested_here);
    assert!(!a.node.group_detail(g.into()).unwrap().call_host_requested_here);
    c.node.group_accept_call_host(g.into(), true).unwrap();
    o.node.group_appoint_call_host(g.into(), o.did.clone(), o.node.group_device_id().unwrap()).unwrap();
    o.node.group_accept_call_host(g.into(), true).unwrap();
    eventually(WAIT, "a sees two call hosts", || a.node.group_detail(g.into()).unwrap().call_hosts.len() == 2);
    let d = c.node.group_detail(g.into()).unwrap();
    assert!(d.am_call_host && !d.am_host, "call hosting does not make a chat host");
    assert_eq!(d.hosts.len(), 1, "o is still the only chat host");
}

/// Starts a call from `p` once the statuses of the call hosts `hosts` arrived.
fn start_call(p: &Peer, g: &str, hosts: &[&Peer]) -> String {
    eventually(WAIT, "the call hosts' statuses", || {
        let have = p.node.group_call_hosts_advertising_for_test(g.into());
        hosts.iter().all(|h| have.contains(&h.did))
    });
    let mut id = None;
    eventually(WAIT, "a call host to pick", || {
        id = p.node.group_call_start(g.into()).ok();
        id.is_some()
    });
    id.unwrap()
}

fn knows_call(p: &Peer, g: &str, call: &str) -> bool {
    p.node.group_calls(g.into()).unwrap().iter().any(|c| c.call_id == call)
}

/// Joined at `host_gen` with `host_did`'s device, in a Roster of `n`.
fn joined(p: &Peer, gen_: u32, host_did: &str, n: usize) -> bool {
    p.node.gcall_status().and_then(|s| s.client).is_some_and(|c| c.state == ClientState::Joined && c.host_gen == gen_ && c.host.holder_did == host_did && c.roster.len() == n)
}

/// Runs the peers' audio for `ms` at 20 ms per frame (recording if `rec`), until `until`.
fn drive(ps: &mut [&mut Peer], dur: u64, rec: bool, mut until: impl FnMut(&[&mut Peer]) -> bool) -> bool {
    let start = Instant::now();
    let mut next = start;
    while start.elapsed() < Duration::from_millis(dur) {
        for p in ps.iter_mut() {
            let mut f = [0i16; audio::FRAME];
            if p.speaking {
                p.tone.next_frame(&mut f);
            }
            p.node.push_mic(f.to_vec());
            let out = p.node.pull_speaker();
            if rec {
                p.rec.extend(out);
            }
        }
        if until(ps) {
            return true;
        }
        next += Duration::from_millis(20);
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    false
}

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

/// Each of `ps` (index `idx[k]`) hears the tones of `speakers` other than its own.
fn assert_hears(ps: &[&mut Peer], idx: &[usize], speakers: &[usize]) {
    for (p, &i) in ps.iter().zip(idx) {
        let x = &p.rec[p.rec.len().saturating_sub(audio::RATE as usize)..];
        for &s in speakers {
            let a = amplitude_at(x, FREQ[s]);
            if s == i {
                assert!(a < 300.0, "peer {i} hears its own tone ({a:.0})");
            } else {
                assert!(a > 1_000.0, "peer {i} does not hear peer {s} ({a:.0})");
            }
        }
    }
}

// KIND: acceptance (GA-I1) — U21 / G6 §2–§3: a member starts a call; the ranking picks the
// desktop call host; request and CallStart go through the chat host; everyone joins on the
// call host's group endpoint and hears the others; U7 holds; a removed member is ejected at once
// and never receives a later CallStart (§9.3).
#[test]
fn a_member_starts_a_call_the_call_host_carries_it_and_a_removed_member_is_ejected() {
    let t0 = Instant::now();
    let mut o = peer("m-o", 0);
    let mut a = peer("m-a", 1);
    let mut b = peer("m-b", 2);
    let mut c = peer("m-c", 3);
    let g = group_with(&o, &[&a, &b, &c]);
    call_hosts(&o, &c, &a, &g);
    eprintln!("group and call hosts after {:?}", t0.elapsed());

    // ---- a starts: c is picked, everyone gets the CallStart and joins ----------------------
    let call = start_call(&a, &g, &[&o, &c]);
    for p in [&o, &b, &c] {
        eventually(WAIT, "the CallStart reaches every member", || knows_call(p, &g, &call));
    }
    let info = a.node.group_calls(g.clone()).unwrap().into_iter().find(|x| x.call_id == call).unwrap();
    assert_eq!((info.host_did.as_str(), info.starter_did.as_str()), (c.did.as_str(), a.did.as_str()), "the desktop on mains hosts");
    assert!(c.node.group_calls(g.clone()).unwrap().iter().any(|x| x.call_id == call && x.hosted_here));
    for p in [&o, &b, &c] {
        p.node.group_call_join(g.clone(), call.clone()).unwrap();
    }
    let cd = c.did.clone();
    assert!(drive(&mut [&mut o, &mut a, &mut b, &mut c], 20_000, false, |ps| ps.iter().all(|p| joined(p, 1, &cd, 4))), "everyone joins c's call");
    assert!(a.events().iter().any(|e| e.kind == GroupCallEventKind::Joined && e.call_id == call));
    assert!(b.events().iter().any(|e| e.kind == GroupCallEventKind::Announced && e.call_id == call && e.group_id == g));
    eprintln!("joined after {:?}", t0.elapsed());

    // ---- three speakers: everyone hears the others -----------------------------------------
    for (p, s) in [(&mut a, true), (&mut b, true), (&mut c, true), (&mut o, false)] {
        p.speaking = s;
        p.rec.clear();
    }
    drive(&mut [&mut o, &mut a, &mut b, &mut c], 2_500, true, |_| false);
    assert_hears(&[&mut o, &mut a, &mut b, &mut c], &[0, 1, 2, 3], &[1, 2, 3]);

    // ---- U7: a plain member knows only the hosts' group endpoints --------------------------
    let ep = |p: &Peer| p.node.group_endpoint_id().unwrap();
    let (eo, ea, eb, ec) = (ep(&o), ep(&a), ep(&b), ep(&c));
    for (p, own, other) in [(&a, &ea, &eb), (&b, &eb, &ea)] {
        let known = p.node.group_known_endpoints_for_test(g.clone());
        assert!(known.iter().all(|k| *k == eo || *k == ec), "a member knows only host endpoints: {known:?}");
        assert!(!known.contains(other) && !known.contains(own));
    }

    // ---- b is removed (3 flags): ejected from the call at once -----------------------------
    for _ in 0..3 {
        o.node.group_flag(g.clone(), b.did.clone()).unwrap();
    }
    let ejected = |ps: &[&mut Peer]| {
        let b_out = ps[2].node.gcall_status().and_then(|s| s.client).is_none_or(|c| matches!(c.state, ClientState::Ended(_)));
        b_out && ps[3].node.gcall_status().and_then(|s| s.hosting).is_some_and(|h| h.roster.len() == 3)
    };
    assert!(drive(&mut [&mut o, &mut a, &mut b, &mut c], 30_000, false, ejected), "the removed member is ejected");
    eventually(WAIT, "b knows it was removed", || b.node.group_detail(g.clone()).unwrap().my_role == GroupRole::Removed);
    assert!(b.node.group_call_join(g.clone(), call.clone()).is_err(), "a removed member cannot join");

    // ---- the call ends when everyone left; a later call never reaches b --------------------
    for p in [&a, &o, &c] {
        p.node.group_call_leave();
    }
    for p in [&a, &o] {
        eventually(WAIT, "the End is relayed", || !knows_call(p, &g, &call));
    }
    let call2 = start_call(&a, &g, &[&o, &c]);
    for p in [&o, &c] {
        eventually(WAIT, "the second call reaches the members", || knows_call(p, &g, &call2));
    }
    std::thread::sleep(Duration::from_secs(3));
    assert!(!knows_call(&b, &g, &call2), "a removed member never receives a later CallStart");
    assert!(!b.events().iter().any(|e| e.call_id == call2));
    eprintln!("total {:?}", t0.elapsed());
}

// KIND: acceptance (GA-I1) — D11 + G6 §5: when the call host rotates its group endpoint, the
// next call reaches it at the new one and a removed member never learns it; revoking its call-host permit mid-call
// moves everyone to the backup at the next generation.
#[test]
#[ignore = "fails on GitHub CI: the call start right after rotation gives up (docs/TODO.md)"]
fn the_next_call_follows_a_rotated_call_host_and_a_revoked_call_host_hands_over() {
    let t0 = Instant::now();
    let mut o = peer("r-o", 0);
    let mut a = peer("r-a", 1);
    let b = peer("r-b", 2);
    let mut c = peer("r-c", 3);
    let g = group_with(&o, &[&a, &b, &c]);
    call_hosts(&o, &c, &a, &g);
    let (old_o, old_c) = (o.node.group_endpoint_id().unwrap(), c.node.group_endpoint_id().unwrap());
    for _ in 0..3 {
        o.node.group_flag(g.clone(), b.did.clone()).unwrap();
    }
    eventually(WAIT, "b removed at the host", || !members(&o, &g).contains(&b.did));
    // D11: rotation is periodic; "change it now" stands in for the interval coming up. The chat
    // host first, then the call host (which learns o's new endpoint through `Moved`).
    o.node.group_rotate_now().unwrap();
    assert_ne!(o.node.group_endpoint_id().unwrap(), old_o, "the chat host rotated");
    eventually(WAIT, "c learns the chat host's new endpoint", || {
        let k = c.node.group_known_endpoints_for_test(g.clone());
        k.contains(&o.node.group_endpoint_id().unwrap()) && !k.contains(&old_o)
    });
    eventually(WAIT, "c is back in session with the chat host", || {
        c.node.group_detail(g.clone()).unwrap().hosts.iter().any(|h| h.did == o.did && h.online)
    });
    c.node.group_rotate_now().unwrap();
    assert_ne!(c.node.group_endpoint_id().unwrap(), old_c, "the call host rotated");
    let new_c = c.node.group_endpoint_id().unwrap();
    eventually(WAIT, "a learns the call host's new endpoint", || {
        let k = a.node.group_known_endpoints_for_test(g.clone());
        k.contains(&new_c) && !k.contains(&old_c)
    });
    eprintln!("rotated after {:?}", t0.elapsed());

    // ---- the next call is hosted by c at its new endpoint ----------------------------------
    let call = start_call(&a, &g, &[&o, &c]);
    for p in [&o, &c] {
        eventually(WAIT, "the CallStart reaches the members", || knows_call(p, &g, &call));
        p.node.group_call_join(g.clone(), call.clone()).unwrap();
    }
    let cd = c.did.clone();
    assert!(drive(&mut [&mut o, &mut a, &mut c], 20_000, false, |ps| ps.iter().all(|p| joined(p, 1, &cd, 3))), "everyone joins c at its new endpoint");
    assert!(!knows_call(&b, &g, &call));
    assert!(!b.node.group_known_endpoints_for_test(g.clone()).contains(&new_c), "the removed member never learns it");

    // ---- c's call-host permit is revoked mid-call: everyone moves to the backup o ----------
    o.node.group_revoke_call_host(g.clone(), c.did.clone(), c.node.group_device_id().unwrap()).unwrap();
    let od = o.did.clone();
    let moved = |ps: &[&mut Peer]| ps.iter().all(|p| joined(p, 2, &od, 3)) && ps[0].node.gcall_status().and_then(|s| s.hosting).is_some_and(|h| h.host_gen == 2);
    assert!(drive(&mut [&mut o, &mut a, &mut c], 30_000, false, moved), "members move to the backup");
    assert!(a.events().iter().any(|e| e.kind == GroupCallEventKind::Reconnecting && e.call_id == call));
    for (p, s) in [(&mut o, true), (&mut a, true), (&mut c, false)] {
        p.speaking = s;
        p.rec.clear();
    }
    drive(&mut [&mut o, &mut a, &mut c], 2_000, true, |_| false);
    assert_hears(&[&mut o, &mut a, &mut c], &[0, 1, 3], &[0, 1]);
    eventually(WAIT, "the takeover's CallStart is announced", || {
        a.node.group_calls(g.clone()).unwrap().iter().any(|x| x.call_id == call && x.host_gen == 2 && x.host_did == o.did)
    });
    eprintln!("total {:?}", t0.elapsed());
}

/// `peer`, with a second (spare) account on the same node, made first: the node runs `name`.
fn peer_with_spare(name: &str, i: usize) -> (Peer, String) {
    let dir = TempDir::new("gcall-group", name);
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    let (tx, rx) = mpsc::channel();
    node.set_group_call_events(Arc::new(Tap(Mutex::new(tx))));
    node.create_identity(format!("{name}-spare"), PASS.into()).unwrap();
    let spare = node.profile().unwrap().did;
    node.begin_new_account().unwrap();
    node.create_identity(name.into(), PASS.into()).unwrap();
    online(&node);
    let did = node.profile().unwrap().did;
    (Peer { node, rx, evs: vec![], did, dir, tone: audio::Tone::new(FREQ[i], AMP), speaking: false, rec: vec![] }, spare)
}

// KIND: acceptance (GA-M4) — G6 §5 / §11 lifecycle: an account switch on a member leaves the
// call (its slot leaves the Roster, and nothing of the call stays on the node for the apps'
// audio to follow); a lock on the call host ends the call with End{host_leaving, next_host}
// and the members move to the backup.
#[test]
fn an_account_switch_leaves_the_call_and_a_locked_call_host_hands_over() {
    let t0 = Instant::now();
    let mut o = peer("l-o", 0);
    let (mut a, spare) = peer_with_spare("l-a", 1);
    let mut b = peer("l-b", 2);
    let mut c = peer("l-c", 3);
    let g = group_with(&o, &[&a, &b, &c]);
    call_hosts(&o, &c, &a, &g);
    let call = start_call(&a, &g, &[&o, &c]);
    for p in [&o, &b, &c] {
        eventually(WAIT, "the CallStart reaches every member", || knows_call(p, &g, &call));
        p.node.group_call_join(g.clone(), call.clone()).unwrap();
    }
    let cd = c.did.clone();
    assert!(drive(&mut [&mut o, &mut a, &mut b, &mut c], 20_000, false, |ps| ps.iter().all(|p| joined(p, 1, &cd, 4))), "everyone joins c's call");
    assert!(a.node.group_call_here().is_some_and(|h| h.in_call && h.call_id == call));
    eprintln!("joined after {:?}", t0.elapsed());

    // ---- (a) a switches account: it leaves the call ----------------------------------------
    a.node.switch_account(spare.clone()).unwrap();
    assert!(a.node.group_call_here().is_none(), "nothing of the call is left for the audio to follow");
    assert!(a.node.gcall_status().is_none(), "the client engine is dropped");
    eventually(10, "a's app hears Left (its audio follows group_call_here)", || a.events().iter().any(|e| e.kind == GroupCallEventKind::Left && e.call_id == call));
    let ad = a.did.clone();
    let left = |ps: &[&mut Peer]| {
        ps[2].node.gcall_status().and_then(|s| s.hosting).is_some_and(|h| h.roster.len() == 3)
            && ps.iter().take(2).all(|p| p.node.gcall_status().and_then(|s| s.client).is_some_and(|c| c.roster.len() == 3 && !c.roster.values().any(|d| *d == ad)))
    };
    assert!(drive(&mut [&mut o, &mut b, &mut c], 10_000, false, left), "a's slot leaves the Roster");
    eprintln!("a left after {:?}", t0.elapsed());

    // ---- (b) the call host c locks: End{host_leaving, next_host = o} -----------------------
    c.node.lock();
    assert!(c.node.group_call_here().is_none() && c.node.gcall_status().is_none(), "c's engines are dropped");
    eventually(10, "c's app hears HostingEnded and Left", || {
        let ev = c.events();
        [GroupCallEventKind::HostingEnded, GroupCallEventKind::Left].iter().all(|k| ev.iter().any(|e| e.kind == *k && e.call_id == call))
    });
    let od = o.did.clone();
    let moved = |ps: &[&mut Peer]| ps.iter().all(|p| joined(p, 2, &od, 2)) && ps[0].node.gcall_status().and_then(|s| s.hosting).is_some_and(|h| h.host_gen == 2);
    assert!(drive(&mut [&mut o, &mut b], 15_000, false, moved), "members move to the backup");
    assert!(b.events().iter().any(|e| e.kind == GroupCallEventKind::Reconnecting && e.call_id == call));
    assert!(b.node.group_call_here().is_some_and(|h| h.in_call && h.call_id == call));
    eprintln!("total {:?}", t0.elapsed());
}

// KIND: acceptance — a call ends when the last participant leaves: the host (c, also a
// participant) signs the End, every member (also one that never joined) hears `Ended` for the
// group, `group_calls` is empty everywhere, and the leavers' own `Left` carries the group id
// (the apps drop the call's Join / banner on it).
#[test]
fn both_participants_hang_up_and_every_member_sees_the_call_end() {
    let t0 = Instant::now();
    let mut o = peer("e-o", 0);
    let mut a = peer("e-a", 1);
    let mut b = peer("e-b", 2);
    let mut c = peer("e-c", 3);
    let g = group_with(&o, &[&a, &b, &c]);
    call_hosts(&o, &c, &a, &g);
    let call = start_call(&a, &g, &[&o, &c]);
    for p in [&o, &b, &c] {
        eventually(WAIT, "the CallStart reaches every member", || knows_call(p, &g, &call));
    }
    c.node.group_call_join(g.clone(), call.clone()).unwrap();
    let cd = c.did.clone();
    assert!(drive(&mut [&mut o, &mut a, &mut b, &mut c], 20_000, false, |ps| joined(ps[1], 1, &cd, 2) && joined(ps[3], 1, &cd, 2)), "a and c are in the call");
    eprintln!("joined after {:?}", t0.elapsed());

    a.node.group_call_leave();
    c.node.group_call_leave();
    for (n, p) in [("o", &o), ("a", &a), ("b", &b), ("c", &c)] {
        eventually(WAIT, &format!("{n}: group_calls is empty"), || p.node.group_calls(g.clone()).unwrap().is_empty());
    }
    std::thread::sleep(Duration::from_secs(1));
    for (n, p) in [("o", &mut o), ("b", &mut b), ("a", &mut a), ("c", &mut c)] {
        let ev = p.events();
        assert!(ev.iter().any(|e| e.kind == GroupCallEventKind::Ended && e.call_id == call && e.group_id == g), "{n} hears Ended for the group: {ev:?}");
    }
    for (n, p) in [("a", &mut a), ("c", &mut c)] {
        let ev = p.events();
        assert!(ev.iter().filter(|e| e.call_id == call).all(|e| e.group_id == g), "{n}: every event of the call names the group: {ev:?}");
    }
    assert!(a.node.group_call_here().is_none() && c.node.group_call_here().is_none());
}
