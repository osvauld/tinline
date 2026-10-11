//! GC-S cases from `crates/group-sim/src/group_call.rs`, run against the real engines, plus
//! G6-D2. A small in-memory world routes engine actions between devices.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use identity::Identity;
use proto::SignedBlob;
use proto::gcall::{self as w, GroupCallMsg};

use super::client::ClientEnd;
use super::policy::*;
use super::select::*;
use super::*;

const OLGA: usize = 0;
const A1: usize = 1; // alice, device 1
const A2: usize = 2; // alice, device 2 (same DID)
const BOB: usize = 3;
const CAROL: usize = 4;
const H1: usize = 5;
const HP: usize = 6; // phone
const MALLORY: usize = 7; // not a member
const N: usize = 8;
const CALL_HOSTS: [usize; 4] = [H1, BOB, OLGA, HP];
const T0: u64 = 1_800_000_000_000;

fn ident(seed: u8) -> Identity {
    let m = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
    identity::recover(&m.to_string()).unwrap()
}

fn gid() -> String {
    "a1".repeat(32)
}

fn audio_dgram(seq: u32, level: u8) -> Vec<u8> {
    let mut v = seq.to_be_bytes().to_vec();
    v.push(level);
    v.extend([0xA5u8; 120]);
    v
}

fn decode(frame: &[u8]) -> GroupCallMsg {
    w::decode_control(frame).unwrap().unwrap().0
}

struct Dev {
    id: Identity,
    ep: [u8; 32],
    href: Option<HostRef>,
    board: CallBoard,
    cond: Cond,
    online: bool,
    seq: u32,
}

enum Ev {
    ToHost(usize, ConnId, Vec<u8>, bool), // (host, conn, bytes, is_control)
    ToClient(usize, (usize, ConnId), Vec<u8>, bool),
    Close(usize, ConnId, CloseReason),
}

struct World {
    now: u64,
    auth: StaticAuthority,
    d: Vec<Dev>,
    hosts: BTreeMap<usize, HostEngine>,
    clients: BTreeMap<usize, ClientEngine>,
    route: BTreeMap<usize, (usize, ConnId)>,
    conns: BTreeMap<(usize, ConnId), usize>,
    next_conn: u64,
    q: VecDeque<Ev>,
    cut: BTreeSet<(usize, usize)>,
    hold_relay: bool,
    held: Vec<SignedBlob>,
    taking: BTreeSet<usize>,
    // Last upstream datagram per device; idle joined clients send the gated level-only
    // keepalive every LEVEL_ONLY_MS like a real silence-gated sender.
    last_up: BTreeMap<usize, u64>,
    ends: Vec<(usize, ClientEnd, u64)>,
    joins: Vec<(usize, usize, u32, u64)>, // (member, host dev, gen, at)
    host_ended: Vec<(usize, EndReason, Option<HostRef>)>,
    notices: Vec<(usize, HostNotice)>,
    down: Vec<(usize, Vec<u8>)>,
    up_ctl: Vec<(usize, GroupCallMsg)>,
    down_ctl: Vec<(usize, GroupCallMsg)>,
}

impl World {
    fn new() -> Self {
        let mut auth = StaticAuthority::default();
        let ids: Vec<Identity> = (0..N).map(|i| if i == A2 { ident(10 + A1 as u8) } else { ident(10 + i as u8) }).collect();
        let mut d = vec![];
        for (i, id) in ids.into_iter().enumerate() {
            let ep = [i as u8 + 1; 32];
            auth.endpoints.entry(id.did().into()).or_default().insert(ep);
            if i != MALLORY {
                auth.members.insert(id.did().into());
            }
            let href = CALL_HOSTS.contains(&i).then(|| HostRef { holder_did: id.did().into(), device_slot: i as u64, permit_digest: format!("permit-{i}") });
            if let Some(h) = &href {
                auth.permits.insert(h.permit_digest.clone());
            }
            let cond = if i == HP { Cond::phone(80, false) } else { Cond::desktop() };
            d.push(Dev { board: CallBoard::new(&gid(), href.clone()), id, ep, href, cond, online: true, seq: 0 });
        }
        World {
            now: T0,
            auth,
            d,
            hosts: BTreeMap::new(),
            clients: BTreeMap::new(),
            route: BTreeMap::new(),
            conns: BTreeMap::new(),
            next_conn: 100,
            q: VecDeque::new(),
            cut: BTreeSet::new(),
            hold_relay: false,
            held: vec![],
            taking: BTreeSet::new(),
            last_up: BTreeMap::new(),
            ends: vec![],
            joins: vec![],
            host_ended: vec![],
            notices: vec![],
            down: vec![],
            up_ctl: vec![],
            down_ctl: vec![],
        }
    }

    fn did(&self, i: usize) -> String {
        self.d[i].id.did().to_string()
    }

    fn href(&self, i: usize) -> HostRef {
        self.d[i].href.clone().unwrap()
    }

    fn dev_of(&self, r: &HostRef) -> usize {
        (0..N).find(|&i| self.d[i].href.as_ref() == Some(r)).unwrap()
    }

    fn start_call(&mut self, h: usize, starter: usize, backups: Vec<usize>) -> Result<String, Refusal> {
        let cfg = HostConfig {
            gid: gid(),
            call_id: w::new_call_id(),
            host_gen: 1,
            host_ref: self.href(h),
            starter_did: self.did(starter),
            backups: backups.iter().map(|b| self.href(*b)).collect(),
            meta_frontier: "frontier".into(),
            cond: self.d[h].cond,
            own_user_in: self.did(starter) == self.did(h),
        };
        let e = HostEngine::new(self.d[h].id.clone(), cfg, self.now)?;
        let cid = e.call_start().call_id.clone();
        let blob = e.start_blob().clone();
        self.hosts.insert(h, e);
        self.announce(h, blob);
        Ok(cid)
    }

    // The host's own board first; everyone else through the chat relay unless held.
    fn announce(&mut self, h: usize, blob: SignedBlob) {
        let _ = self.d[h].board.accept_start(&blob, &self.auth);
        if self.hold_relay {
            self.held.push(blob);
        } else {
            self.relay(&blob);
        }
    }

    fn relay(&mut self, blob: &SignedBlob) {
        for i in 0..N {
            if self.d[i].online && i != MALLORY {
                let _ = self.d[i].board.accept_start(blob, &self.auth);
            }
        }
        self.settle();
    }

    fn flush_relay(&mut self) {
        self.hold_relay = false;
        for b in std::mem::take(&mut self.held) {
            self.relay(&b);
        }
    }

    // Glare and split: a host whose call lost ends it as superseded.
    fn settle(&mut self) {
        let hs: Vec<usize> = self.hosts.keys().copied().collect();
        for h in hs {
            let Some(winner) = self.d[h].board.superseded(self.hosts[&h].call_start()) else { continue };
            let acts = self.hosts.get_mut(&h).unwrap().supersede(winner);
            self.host_actions(h, acts);
        }
        self.pump();
    }

    fn join(&mut self, m: usize) -> Result<(), Refusal> {
        let start = self.d[m].board.shown().cloned().ok_or(Refusal::Unknown)?;
        self.join_start(m, start)
    }

    fn join_start(&mut self, m: usize, start: w::CallStart) -> Result<(), Refusal> {
        let (c, acts) = ClientEngine::new(self.d[m].id.clone(), start, None, self.now, &self.auth)?;
        self.clients.insert(m, c);
        self.client_actions(m, acts);
        self.pump();
        Ok(())
    }

    fn client_actions(&mut self, m: usize, acts: Vec<ClientAction>) {
        for a in acts {
            match a {
                ClientAction::Dial { host } => {
                    let h = self.dev_of(&host);
                    let conn = if h == m {
                        ConnId::LOCAL
                    } else {
                        self.next_conn += 1;
                        ConnId(self.next_conn)
                    };
                    if let Some(old) = self.route.insert(m, (h, conn)) {
                        self.conns.remove(&old);
                    }
                    self.conns.insert((h, conn), m);
                    self.clients.get_mut(&m).unwrap().set_local(h == m);
                    if !self.d[h].online || self.cut.contains(&(m, h)) {
                        let acts = self.clients.get_mut(&m).unwrap().dial_failed(self.now, &self.auth);
                        self.client_actions(m, acts);
                        return;
                    }
                    if let Some(e) = self.hosts.get_mut(&h) {
                        e.connected(conn, self.d[m].ep);
                    }
                }
                ClientAction::Control(f) => {
                    self.up_ctl.push((m, decode(&f)));
                    if let Some(&(h, conn)) = self.route.get(&m) {
                        self.q.push_back(Ev::ToHost(h, conn, f, true));
                    }
                }
                ClientAction::Datagram(b) => {
                    self.last_up.insert(m, self.now);
                    if let Some(&(h, conn)) = self.route.get(&m) {
                        self.q.push_back(Ev::ToHost(h, conn, b, false));
                    }
                }
                ClientAction::Joined { .. } => {
                    let c = &self.clients[&m];
                    let (h, g) = (self.dev_of(c.host()), c.host_gen());
                    self.joins.push((m, h, g, self.now));
                }
                ClientAction::RosterChanged => {}
                ClientAction::Superseded { by } => {
                    let start = self.d[m].board.hosted_by(&by).cloned();
                    self.ends.push((m, ClientEnd::Superseded, self.now));
                    if let Some(s) = start {
                        let (c, acts) = ClientEngine::new(self.d[m].id.clone(), s, None, self.now, &self.auth).unwrap();
                        self.clients.insert(m, c);
                        self.client_actions(m, acts);
                        return;
                    }
                }
                ClientAction::Ended(e) => {
                    if e != ClientEnd::Superseded {
                        self.ends.push((m, e, self.now));
                    }
                }
            }
        }
    }

    fn host_actions(&mut self, h: usize, acts: Vec<HostAction>) {
        for a in acts {
            match a {
                HostAction::Control { conn, frame } => self.q.push_back(Ev::ToClient(h, (h, conn), frame, true)),
                HostAction::Datagram { conn, bytes } => self.q.push_back(Ev::ToClient(h, (h, conn), bytes, false)),
                HostAction::Close { conn, reason } => self.q.push_back(Ev::Close(h, conn, reason)),
                HostAction::Notice(n) => self.notices.push((h, n)),
                HostAction::Ended { reason, next_host, end } => {
                    self.host_ended.push((h, reason, next_host));
                    self.hosts.remove(&h);
                    if let (EndReason::Ended, Some(e)) = (reason, end) {
                        for i in 0..N {
                            let _ = self.d[i].board.accept_end(&e, &self.auth);
                        }
                    }
                }
            }
        }
    }

    fn pump(&mut self) {
        while let Some(ev) = self.q.pop_front() {
            match ev {
                Ev::ToHost(h, conn, bytes, ctl) => {
                    let Some(&m) = self.conns.get(&(h, conn)) else { continue };
                    if !self.d[h].online {
                        continue;
                    }
                    if ctl {
                        let msg = decode(&bytes);
                        if !self.hosts.contains_key(&h) {
                            self.maybe_take_over(h, &msg, m);
                        }
                        let Some(e) = self.hosts.get_mut(&h) else { continue };
                        e.connected(conn, self.d[m].ep);
                        match e.on_control(conn, msg, self.now, &self.auth) {
                            Ok(acts) => {
                                self.taking.remove(&h);
                                self.host_actions(h, acts);
                            }
                            Err(_) if self.taking.remove(&h) => {
                                self.hosts.remove(&h); // a takeover whose first Join was refused
                            }
                            Err(_) => {}
                        }
                    } else if let Some(e) = self.hosts.get_mut(&h)
                        && let Ok(acts) = e.on_datagram(conn, &bytes, self.now)
                    {
                        self.host_actions(h, acts);
                    }
                }
                Ev::ToClient(h, key, bytes, ctl) => {
                    let Some(&m) = self.conns.get(&key) else { continue };
                    if self.route.get(&m) != Some(&key) || !self.d[h].online {
                        continue;
                    }
                    let Some(c) = self.clients.get_mut(&m) else { continue };
                    if ctl {
                        let msg = decode(&bytes);
                        self.down_ctl.push((m, msg.clone()));
                        if let Ok(acts) = c.on_control(msg, self.now, &self.auth) {
                            self.client_actions(m, acts);
                        }
                    } else {
                        c.on_datagram(&bytes, self.now);
                        self.down.push((m, bytes));
                    }
                }
                Ev::Close(h, conn, reason) => {
                    let Some(m) = self.conns.remove(&(h, conn)) else { continue };
                    if self.route.get(&m) != Some(&(h, conn)) {
                        continue;
                    }
                    self.route.remove(&m);
                    let c = self.clients.get_mut(&m).unwrap();
                    let acts = c.on_closed(reason == CloseReason::AnsweredElsewhere, self.now, &self.auth);
                    self.client_actions(m, acts);
                }
            }
        }
    }

    // §5: a backup that gets the first valid Join{gen + 1} signs CallStart{gen + 1}.
    fn maybe_take_over(&mut self, h: usize, msg: &GroupCallMsg, _m: usize) {
        let GroupCallMsg::Join { join, .. } = msg else { return };
        let Ok(j) = w::open_join(join) else { return };
        if self.d[h].board.may_take(&j.call_id) != Some(j.host_gen) {
            return;
        }
        let prev = self.d[h].board.start(&j.call_id).unwrap().clone();
        if let Ok(e) = HostEngine::takeover(self.d[h].id.clone(), &prev, self.href(h), self.d[h].cond, true, self.now) {
            let blob = e.start_blob().clone();
            self.hosts.insert(h, e);
            self.taking.insert(h);
            self.announce(h, blob);
        }
    }

    fn step(&mut self) {
        self.now += FRAME_MS;
        let hs: Vec<usize> = self.hosts.keys().copied().collect();
        for h in hs {
            if !self.d[h].online {
                continue;
            }
            let Some(e) = self.hosts.get_mut(&h) else { continue };
            let acts = e.tick(self.now, &self.auth);
            self.host_actions(h, acts);
        }
        let ms: Vec<usize> = self.clients.keys().copied().collect();
        for m in ms {
            if !self.d[m].online {
                continue;
            }
            let acts = self.clients.get_mut(&m).unwrap().tick(self.now, &self.auth);
            self.client_actions(m, acts);
            let idle = self.last_up.get(&m).is_none_or(|t| self.now >= t + LEVEL_ONLY_MS);
            if self.joined(m) && idle {
                self.last_up.insert(m, self.now);
                if let Some(&(h, conn)) = self.route.get(&m) {
                    self.q.push_back(Ev::ToHost(h, conn, w::up_level(127).to_vec(), false));
                }
            }
        }
        self.pump();
    }

    fn advance(&mut self, ms: u64) {
        for _ in 0..ms / FRAME_MS {
            self.step();
        }
    }

    fn send_up(&mut self, m: usize, bytes: Vec<u8>) -> Result<(), Refusal> {
        let (h, conn) = *self.route.get(&m).ok_or(Refusal::Unknown)?;
        self.last_up.insert(m, self.now);
        let e = self.hosts.get_mut(&h).ok_or(Refusal::Unknown)?;
        let acts = e.on_datagram(conn, &bytes, self.now)?;
        self.host_actions(h, acts);
        self.pump();
        Ok(())
    }

    fn speak(&mut self, m: usize, level: u8) -> Result<(), Refusal> {
        self.d[m].seq += 1;
        let a = audio_dgram(self.d[m].seq, level);
        self.send_up(m, w::up_audio(&a))
    }

    fn roster(&self, h: usize) -> BTreeMap<u8, String> {
        self.hosts[&h].roster()
    }

    fn joined(&self, m: usize) -> bool {
        self.clients.get(&m).is_some_and(|c| c.state() == ClientState::Joined)
    }

    fn host_of(&self, m: usize) -> (usize, u32) {
        let c = &self.clients[&m];
        (self.dev_of(c.host()), c.host_gen())
    }

    fn ended(&self, m: usize, e: ClientEnd) -> bool {
        self.ends.iter().any(|(d, x, _)| *d == m && *x == e)
    }

    // A raw Join for `m` on a fresh connection from `ep`.
    fn raw_join(&mut self, h: usize, ep: [u8; 32], join: SignedBlob) -> Result<Vec<HostAction>, Refusal> {
        self.next_conn += 1;
        let conn = ConnId(self.next_conn);
        let e = self.hosts.get_mut(&h).unwrap();
        e.connected(conn, ep);
        e.on_control(conn, GroupCallMsg::Join { join, note: None }, self.now, &self.auth)
    }
}

fn base() -> (World, String) {
    let mut s = World::new();
    let cid = s.start_call(H1, OLGA, vec![BOB, OLGA]).unwrap();
    (s, cid)
}

// GC-S01. H refuses each bad Join on a populated call; H's state is unchanged and the Roster
// too. Positive control: a valid Join is accepted.
#[test]
fn gcall_s01_bad_joins_are_refused_and_leave_host_state_unchanged() {
    let (mut s, cid) = base();
    for m in [A1, BOB] {
        s.join(m).unwrap();
    }
    s.advance(3_000);
    let carol = s.d[CAROL].id.clone();
    let (cep, mep) = (s.d[CAROL].ep, s.d[MALLORY].ep);
    let j = |id: &Identity, gid: &str, cid: &str, g: u32, t: u64| w::sign_join(id, gid, cid, g, t).1;
    let now = s.now;
    let mut tampered = j(&carol, &gid(), &cid, 1, now);
    {
        use base64::Engine;
        let e = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let mut v: serde_json::Value = serde_json::from_slice(&e.decode(&tampered.payload).unwrap()).unwrap();
        v["issued_ms"] = (now + 1).into();
        tampered.payload = e.encode(serde_json::to_vec(&v).unwrap());
    }
    let a1_join = s.up_ctl.iter().find_map(|(m, msg)| match msg {
        GroupCallMsg::Join { join, .. } if *m == A1 => Some(join.clone()),
        _ => None,
    });
    let bob_join = j(&s.d[BOB].id.clone(), &gid(), &cid, 1, now);
    s.auth.endpoints.entry(s.did(MALLORY)).or_default().insert(mep);
    let cases: Vec<(&str, [u8; 32], SignedBlob, Refusal)> = vec![
        ("non-member", mep, j(&s.d[MALLORY].id.clone(), &gid(), &cid, 1, now), Refusal::NotMember),
        ("tampered body", cep, tampered, Refusal::BadSignature),
        ("wrong gid", cep, j(&carol, &"e".repeat(64), &cid, 1, now), Refusal::WrongGroup),
        ("wrong call_id", cep, j(&carol, &gid(), &w::new_call_id(), 1, now), Refusal::WrongCall),
        ("future host_gen", cep, j(&carol, &gid(), &cid, 2, now), Refusal::StaleGen),
        ("replayed accepted Join", s.d[A1].ep, a1_join.unwrap(), Refusal::Replay),
        ("bob's Join from carol's connection", cep, bob_join, Refusal::EndpointMismatch),
    ];
    let roster = s.roster(H1);
    for (case, ep, blob, want) in cases {
        let before = s.hosts[&H1].snapshot();
        assert_eq!(s.raw_join(H1, ep, blob).unwrap_err(), want, "{case}");
        assert_eq!(s.hosts[&H1].snapshot(), before, "{case}");
        assert_eq!(s.roster(H1), roster, "{case}");
    }
    // Oversize control never reaches the engine.
    let mut huge = ((w::MAX_CONTROL + 1) as u32).to_be_bytes().to_vec();
    huge.extend([b' '; 16]);
    assert_eq!(w::decode_control(&huge).map_err(Refusal::from), Err(Refusal::Oversize));
    // Positive control, then a removed member (U12: refusal is immediate, and H ejects her).
    s.join(CAROL).unwrap();
    assert_eq!(s.roster(H1).len(), 3);
    s.auth.removed.insert(s.did(CAROL));
    s.step();
    assert!(s.ended(CAROL, ClientEnd::Removed));
    assert_eq!(s.roster(H1).len(), 2);
    s.advance(3_000);
    let before = s.hosts[&H1].snapshot();
    assert_eq!(s.raw_join(H1, cep, j(&carol, &gid(), &cid, 1, s.now)).unwrap_err(), Refusal::NotMember);
    assert_eq!(s.hosts[&H1].snapshot(), before);
}

// GC-S03. After a redial to gen 2, gen-1 control from the old host (End, CallStart replayed
// or freshly signed, Roster) is ignored by every board and client, state unchanged.
#[test]
fn gcall_s03_stale_end_callstart_and_roster_after_redial_are_ignored() {
    let (mut s, cid) = base();
    for m in [A1, CAROL] {
        s.join(m).unwrap();
    }
    let gen1 = s.hosts[&H1].start_blob().clone();
    s.d[H1].online = false;
    s.advance(4_000);
    assert_eq!(s.hosts[&BOB].call_start().host_gen, 2);
    let h1 = s.d[H1].id.clone();
    let end = w::sign_end(&h1, w::End { v: 1, gid: gid(), call_id: cid.clone(), host_gen: 1, host_did: s.did(H1), reason: EndReason::Ended, next_host: None });
    let mut fresh = s.d[H1].board.start(&cid).cloned().unwrap();
    fresh.host_gen = 1;
    fresh.host_ref = s.href(H1);
    let fresh = w::sign_start(&h1, fresh);
    let roster = w::sign_roster(&h1, w::Roster { v: 1, gid: gid(), call_id: cid.clone(), host_gen: 1, host_did: s.did(H1), epoch: 99, entries: vec![] });
    for i in [OLGA, A1, BOB, CAROL, HP] {
        let before = s.d[i].board.clone();
        assert_eq!(s.d[i].board.accept_end(&end, &s.auth), Err(Refusal::StaleGen), "end at {i}");
        assert_eq!(s.d[i].board.accept_start(&gen1, &s.auth), Err(Refusal::StaleGen), "replayed start at {i}");
        assert_eq!(s.d[i].board.accept_start(&fresh, &s.auth), Err(Refusal::StaleGen), "fresh gen-1 at {i}");
        assert_eq!(s.d[i].board, before);
    }
    for m in [A1, CAROL] {
        let c = s.clients.get_mut(&m).unwrap();
        let before = (c.state(), c.host_gen(), c.epoch(), c.roster().clone());
        assert_eq!(c.on_control(GroupCallMsg::End { end: end.clone() }, s.now, &s.auth), Err(Refusal::StaleGen));
        assert_eq!(c.on_control(GroupCallMsg::Roster { roster: roster.clone() }, s.now, &s.auth), Err(Refusal::StaleGen));
        assert_eq!((c.state(), c.host_gen(), c.epoch(), c.roster().clone()), before);
        assert_eq!(s.host_of(m), (BOB, 2));
    }
}

// GC-S05. Two members start a call at once on different hosts; the relay is held so each
// call gets participants. After the relay the lower BLAKE3(call_id) wins everywhere, the
// loser ends `superseded`, and its participants (the loser host's own user included) move.
#[test]
fn gcall_s05_concurrent_starts_converge_and_the_loser_ends_superseded() {
    let mut s = World::new();
    s.hold_relay = true;
    let c1 = s.start_call(H1, OLGA, vec![OLGA]).unwrap();
    let c2 = s.start_call(BOB, BOB, vec![OLGA]).unwrap();
    // Starters get their host's CallStart directly (the CallHostRequest answer).
    let (b1, b2) = (s.held[0].clone(), s.held[1].clone());
    for m in [OLGA, A1] {
        s.d[m].board.accept_start(&b1, &s.auth).unwrap();
        s.join(m).unwrap();
    }
    s.d[CAROL].board.accept_start(&b2, &s.auth).unwrap();
    s.join(CAROL).unwrap();
    s.join(BOB).unwrap(); // BOB's own user, through ConnId::LOCAL
    assert_eq!((s.roster(H1).len(), s.roster(BOB).len()), (2, 2));
    s.flush_relay();
    let (winner, win, lose) = if board::call_hash(&c1) < board::call_hash(&c2) { (c1, H1, BOB) } else { (c2, BOB, H1) };
    assert!(!s.hosts.contains_key(&lose), "loser stopped hosting");
    assert!(s.host_ended.contains(&(lose, EndReason::Superseded, Some(s.href(win)))));
    for m in [OLGA, A1, BOB, CAROL, HP, H1] {
        assert_eq!(s.d[m].board.shown().unwrap().call_id, winner, "shown at {m}");
    }
    for m in [OLGA, A1, BOB, CAROL] {
        assert!(s.joined(m), "{m} joined");
        assert_eq!(s.clients[&m].call_start().call_id, winner);
        assert_eq!(s.host_of(m).0, win);
    }
    assert_eq!(s.roster(win).len(), 4);
    assert_eq!(s.ends.iter().filter(|(_, e, _)| *e == ClientEnd::Superseded).count(), 2);
}

// GC-S06. Forwarded counts equal the §4 formulas (every member sending all the time), no
// self-echo, at most K per member; hysteresis prevents flapping; and the replacement timing
// with and without G6-D2.
#[test]
fn gcall_s06_forwarded_counts_match_section_4_and_hysteresis_prevents_flapping() {
    let copies = |set: &BTreeSet<u8>, sent: &BTreeSet<u8>, rx: &BTreeSet<u8>| -> Vec<(u8, u8)> {
        set.intersection(sent).flat_map(|s| rx.iter().filter(move |r| *r != s).map(move |r| (*s, *r))).collect()
    };
    for (n, k, up) in [(3usize, 2usize, 3usize), (3, 3, 4), (5, 2, 7), (5, 3, 10), (8, 2, 13), (8, 3, 19), (12, 2, 21), (12, 3, 31)] {
        let slots: BTreeSet<u8> = (0..n as u8).collect();
        let levels: BTreeMap<u8, u8> = slots.iter().map(|&s| (s, 10 + 2 * s)).collect();
        let mut f = Forwarder::new(k);
        for _ in 0..AVG_FRAMES * 2 {
            f.frame(&levels);
        }
        assert_eq!(f.selected().len(), k.min(n));
        assert!(f.selected().contains(&0));
        let cp = copies(f.selected(), &slots, &slots);
        assert!(cp.iter().all(|(a, b)| a != b), "no self-echo");
        assert!(slots.iter().all(|r| cp.iter().filter(|(_, t)| t == r).count() <= k));
        let h_up = cp.iter().filter(|(_, t)| *t != 0).count();
        let formula = if n > k { (n - 2) * k + 1 } else { (n - 1) * (n - 1) };
        assert_eq!((h_up, h_up), (formula, up), "N={n} K={k}");
        assert_eq!(cp.len(), if n > k { n * k - k } else { n * (n - 1) }, "dedicated host N={n} K={k}");
    }
    // Hysteresis: three close, jittery speakers and K = 2 never flap; without it they do.
    let run = |hyst: bool| {
        let mut f = Forwarder::new(2);
        let mut rng = 6u64;
        let (mut changes, mut last) = (0, BTreeSet::new());
        for t in 0..500 {
            let lv: BTreeMap<u8, u8> = [(1u8, 30u8), (2, 31), (3, 32)]
                .iter()
                .map(|&(s, b)| {
                    rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    (s, b - 6 + ((rng >> 33) % 13) as u8)
                })
                .collect();
            if hyst {
                f.frame(&lv);
            } else {
                // No hysteresis: S is simply the K lowest averages this frame.
                let mut g = Forwarder::new(2);
                g.frame(&lv);
                f = g;
            }
            if t > AVG_FRAMES && *f.selected() != last {
                changes += 1;
            }
            last = f.selected().clone();
        }
        changes
    };
    assert_eq!(run(true), 0, "no flapping with hysteresis");
    assert!(run(false) >= 10, "flapping without hysteresis");
    // Replacement. S = {1 at 30, 2 at 32}; slot 3 present at 60, then bursts at level 5.
    // Without G6-D2: swap on burst frame 19 for a 30-frame burst (forwarded from the next
    // datagram), never for 10 frames. With it: forwarded at once (provisional), in S after
    // HYST_FRAMES for both.
    for (d2, burst, fwd_at, sel_at) in [(false, 10, None, None), (false, 30, Some(20), Some(19)), (true, 10, Some(1), Some(10)), (true, 30, Some(1), Some(10))] {
        let mut f = Forwarder::new(2);
        f.set_provisional(d2);
        let steady = BTreeMap::from([(1u8, 30u8), (2, 32), (3, 60)]);
        for _ in 0..AVG_FRAMES * 2 {
            f.frame(&steady);
        }
        let (mut fa, mut sa) = (None, None);
        for t in 1..=burst + AVG_FRAMES {
            let l3 = if t <= burst { 5 } else { 60 };
            f.offer(3, l3);
            if fa.is_none() && f.is_forwarded(3) {
                fa = Some(t);
            }
            f.frame(&BTreeMap::from([(1u8, 30u8), (2, 32), (3, l3)]));
            if sa.is_none() && f.selected().contains(&3) {
                sa = Some(t);
            }
            assert!(f.forwarded().len() <= 3, "at most K + 1");
        }
        assert_eq!((fa, sa), (fwd_at, sel_at), "d2={d2} burst {burst}");
    }
}

fn tone_frame(t: &mut audio::Tone) -> [i16; audio::FRAME] {
    let mut f = [0i16; audio::FRAME];
    t.next_frame(&mut f);
    f
}

// GC-S06b (G6-D1). Real engines and the real sender gate: H sends `Active` to every
// participant whenever the forwarded set changes, gens 1, 2, 3, … in order, and every client
// tracks H. Level-only datagrams reach H and are never forwarded. A gated sender stops Opus
// and its seq keeps counting. A stale Active is ignored.
#[test]
fn gcall_s06b_active_on_every_change_and_level_only_never_forwarded() {
    let (mut s, _) = base();
    let ps = [OLGA, A1, CAROL];
    for m in ps {
        s.join(m).unwrap();
    }
    let mut tones: Vec<audio::Tone> = (0..3).map(|i| audio::Tone::new(300.0 + 100.0 * i as f32, 8_000.0)).collect();
    let silence = [0i16; audio::FRAME];
    let (d0, c0) = (s.down.len(), s.down_ctl.len());
    let mut up_carol = vec![];
    for t in 0..350u32 {
        let script = [(OLGA, !(100..300).contains(&t)), (A1, (100..200).contains(&t)), (CAROL, t >= 300)];
        for (k, (m, on)) in script.into_iter().enumerate() {
            let pcm = if on { tone_frame(&mut tones[k]) } else { silence };
            let acts = s.clients.get_mut(&m).unwrap().on_mic(&pcm);
            if m == CAROL {
                up_carol.extend(acts.iter().filter_map(|a| match a {
                    ClientAction::Datagram(b) => Some(b.clone()),
                    _ => None,
                }));
            }
            s.client_actions(m, acts);
        }
        s.step();
        let (g, set) = s.hosts[&H1].active();
        for m in ps {
            assert_eq!(s.clients[&m].active(), (g, set), "client {m} at frame {t}");
        }
    }
    let (changes, _) = s.hosts[&H1].active();
    assert!(changes >= 4, "forwarded set changed {changes} times");
    for m in ps {
        let gens: Vec<u32> = s.down_ctl[c0..]
            .iter()
            .filter(|(d, _)| *d == m)
            .filter_map(|(_, msg)| match msg {
                GroupCallMsg::Active { r#gen, .. } => Some(*r#gen),
                _ => None,
            })
            .collect();
        assert_eq!(gens, (1..=changes).collect::<Vec<_>>(), "one Active per change at {m}");
    }
    assert!(s.hosts[&H1].level_only_rx() > 0);
    assert!(s.down[d0..].iter().all(|(_, b)| w::parse_down(b).is_ok()), "only full audio is forwarded");
    // CAROL: 15 Opus frames until the gate closes, level-only every 500 ms, Opus again at 300.
    let opus: Vec<u32> = up_carol.iter().filter(|b| b[0] == w::UP_AUDIO).map(|b| u32::from_be_bytes(b[1..5].try_into().unwrap())).collect();
    let gate = (300 / FRAME_MS) as u32;
    assert_eq!(opus[..gate as usize], (0..gate).collect::<Vec<_>>()[..]);
    assert_eq!(opus[gate as usize], 300, "seq kept counting while gated");
    let levels = up_carol.iter().filter(|b| b[0] == w::UP_LEVEL).count() as u32;
    assert_eq!(levels, (300 - gate) / (LEVEL_ONLY_MS / FRAME_MS) as u32);
    let c = s.clients.get_mut(&OLGA).unwrap();
    assert_eq!(c.on_control(GroupCallMsg::Active { r#gen: changes, slots: vec![0, 1, 2] }, s.now, &s.auth), Err(Refusal::Stale));
}

// GC-S08. Host lost after 3 missed heartbeats. (a) backups[0] up: redial it at once with
// gen + 1; its CallStart lists the old backups minus itself (G9). (b) backups[0] down: reach
// backups[1] after ANSWER_TIMEOUT_MS. (c) both down: "host lost". (d) split: some members reach
// b0, others b1; after the relay b0 wins and b1's members move.
#[test]
fn gcall_s08_host_drop_redials_backup0_else_backup1_and_split_resolves_to_backup0() {
    let setup = || {
        let (mut s, cid) = base();
        for m in [A1, CAROL, HP] {
            s.join(m).unwrap();
        }
        s.advance(2_000);
        (s, cid)
    };
    let last_hb = |s: &World| s.now - (s.now - T0) % HEARTBEAT_MS;
    // (a)
    let (mut s, cid) = setup();
    let lost = last_hb(&s) + HEARTBEAT_MISSES * HEARTBEAT_MS;
    s.d[H1].online = false;
    s.advance(10_000);
    for m in [A1, CAROL, HP] {
        assert_eq!(s.host_of(m), (BOB, 2), "{m}");
        assert_eq!(s.clients[&m].call_start().call_id, cid);
        let at = s.joins.iter().rev().find(|j| j.0 == m).unwrap().3;
        assert!(at >= lost && at <= lost + FRAME_MS, "{m} redialed at {at}, lost at {lost}");
    }
    assert_eq!(s.roster(BOB).len(), 3);
    assert_eq!(s.hosts[&BOB].call_start().backups, vec![s.href(OLGA)], "G9");
    // (b)
    let (mut s, _) = setup();
    let lost = last_hb(&s) + HEARTBEAT_MISSES * HEARTBEAT_MS;
    s.d[H1].online = false;
    s.d[BOB].online = false; // unreachable: skipped at once
    s.advance(12_000);
    for m in [A1, CAROL, HP] {
        assert_eq!(s.host_of(m), (OLGA, 2));
        let at = s.joins.iter().rev().find(|j| j.0 == m).unwrap().3;
        assert!(at <= lost + FRAME_MS, "an unreachable backup is skipped at once: {at} vs {lost}");
    }
    // (b') BOB is reachable but silent: the redial waits ANSWER_TIMEOUT_MS.
    let (mut s, _) = setup();
    let lost = last_hb(&s) + HEARTBEAT_MISSES * HEARTBEAT_MS;
    s.d[H1].online = false;
    s.d[BOB].board = CallBoard::new(&gid(), None); // cannot take over, never answers
    s.advance(12_000);
    for m in [A1, CAROL, HP] {
        assert_eq!(s.host_of(m), (OLGA, 2));
        let at = s.joins.iter().rev().find(|j| j.0 == m).unwrap().3;
        assert!(at >= lost + ANSWER_TIMEOUT_MS && at <= lost + ANSWER_TIMEOUT_MS + FRAME_MS, "{at}");
    }
    // (c)
    let (mut s, _) = setup();
    for h in [H1, BOB, OLGA] {
        s.d[h].online = false;
    }
    s.advance(12_000);
    for m in [A1, CAROL, HP] {
        assert!(s.ended(m, ClientEnd::HostLost), "{m}");
        assert!(!s.joined(m));
    }
    // (d) split
    let (mut s, cid) = setup();
    s.hold_relay = true;
    s.cut.insert((HP, BOB));
    s.d[H1].online = false;
    s.advance(4_000);
    assert_eq!(s.hosts[&BOB].call_start().host_gen, 2);
    assert_eq!(s.hosts[&OLGA].call_start().host_gen, 2, "b1 started gen 2 too");
    assert_eq!(s.host_of(HP), (OLGA, 2));
    s.cut.clear();
    s.flush_relay();
    assert!(!s.hosts.contains_key(&OLGA), "b1 ended superseded");
    for m in [A1, CAROL, HP] {
        assert_eq!(s.host_of(m), (BOB, 2), "{m}");
        assert!(s.joined(m));
        assert_eq!(s.clients[&m].call_start().call_id, cid);
    }
    assert_eq!(s.roster(BOB).len(), 3);
}

// GC-S13. Floods are bounded and other members are unaffected.
#[test]
fn gcall_s13_floods_are_bounded_and_other_members_are_unaffected() {
    let (mut s, cid) = base();
    for m in [OLGA, A1, CAROL] {
        s.join(m).unwrap();
    }
    let a1_conn = s.route[&A1].1;
    let olga = s.did(OLGA);
    let olga_slot = s.hosts[&H1].slot_of(&olga).unwrap();
    let mut big = vec![w::UP_AUDIO];
    big.extend(vec![0u8; w::MAX_UP_DATAGRAM]);
    let (mut accepted, secs) = (0u64, 5u64);
    let d0 = s.down.len();
    for _ in 0..secs * 50 {
        s.speak(OLGA, 20).unwrap();
        s.speak(A1, 25).unwrap();
        for _ in 0..4 {
            accepted += u64::from(s.speak(CAROL, 30).is_ok());
        }
        let e = s.hosts.get_mut(&H1).unwrap();
        assert_eq!(e.on_datagram(a1_conn, &big, s.now), Err(Refusal::Oversize));
        s.step();
    }
    assert!(accepted <= secs * MAX_UP_PPS + UP_BURST, "flood bounded: {accepted}");
    assert!(accepted >= secs * MAX_UP_PPS, "rate honoured: {accepted}");
    assert_eq!(s.hosts[&H1].dropped(&olga), 0, "olga never dropped");
    let olga_to_a1 = s.down[d0..].iter().filter(|(m, b)| *m == A1 && b[1] == olga_slot).count() as u64;
    assert_eq!(olga_to_a1, secs * 50, "every olga frame forwarded");
    // Level-only flood: same bucket, never forwarded; bad lengths dropped.
    let (rx0, d1) = (s.hosts[&H1].level_only_rx(), s.down.len());
    for _ in 0..50 {
        for _ in 0..4 {
            let _ = s.send_up(CAROL, w::up_level(10).to_vec());
        }
        s.step();
    }
    let rx = s.hosts[&H1].level_only_rx() - rx0;
    assert!(rx <= MAX_UP_PPS + UP_BURST, "level-only flood bounded: {rx}");
    assert_eq!(s.down.len(), d1, "level-only datagrams are never forwarded");
    for bad in [vec![w::UP_LEVEL], vec![w::UP_LEVEL, 1, 2]] {
        assert_eq!(s.send_up(CAROL, bad), Err(Refusal::Malformed));
    }
    // Join spam: a newer Join after the gap is fine; more within JOIN_MIN_GAP_MS are refused
    // with H's state unchanged.
    let carol = s.d[CAROL].id.clone();
    let cep = s.d[CAROL].ep;
    let first = w::sign_join(&carol, &gid(), &cid, 1, s.now + 1).1;
    assert!(s.raw_join(H1, cep, first).is_ok());
    let roster = s.roster(H1);
    for i in 0..5 {
        let r = w::sign_join(&carol, &gid(), &cid, 1, s.now + 2 + i).1;
        let before = s.hosts[&H1].snapshot();
        assert_eq!(s.raw_join(H1, cep, r), Err(Refusal::RateLimited));
        assert_eq!(s.hosts[&H1].snapshot(), before);
    }
    assert_eq!(s.roster(H1), roster);
    // More than MAX_CONTROL_PER_S frames on one connection are refused.
    let e = s.hosts.get_mut(&H1).unwrap();
    let junk = GroupCallMsg::Heartbeat { epoch: 0 };
    let refused: Vec<Refusal> = (0..MAX_CONTROL_PER_S + 2).map(|_| e.on_control(a1_conn, junk.clone(), s.now, &s.auth).unwrap_err()).collect();
    assert_eq!(refused.iter().filter(|r| **r == Refusal::RateLimited).count(), 2);
}

// GC-S16. Phone degrade: the §2.4 table, then while hosting.
#[test]
fn gcall_s16_phone_degrade_reduces_k_refuses_joins_and_hands_over() {
    use PhoneAction::*;
    let p = |battery: u8, plugged: bool, thermal: Thermal, metered: bool| Cond { thermal, metered, ..Cond::phone(battery, plugged) };
    for (c, want) in [
        (p(30, false, Thermal::None, false), Advertise),
        (p(29, false, Thermal::None, false), StopAdvertising),
        (p(14, false, Thermal::None, false), Limited),
        (p(8, false, Thermal::None, false), Limited),
        (p(7, false, Thermal::None, false), End),
        (p(5, true, Thermal::None, false), Advertise),
        (p(90, true, Thermal::Moderate, false), StopAdvertising),
        (p(90, true, Thermal::Severe, false), Limited),
        (p(90, true, Thermal::Critical, false), End),
        (p(90, true, Thermal::None, true), StopAdvertising),
        (Cond { metered_override: true, ..p(90, true, Thermal::None, true) }, Advertise),
    ] {
        assert_eq!(phone_policy(&c), want, "{c:?}");
    }
    let mut s = World::new();
    assert_eq!(s.start_call(HP, OLGA, vec![]), Err(Refusal::Refused), "a phone never hosts listen-only");
    let cid = s.start_call(HP, HP, vec![H1, BOB]).unwrap();
    assert_eq!(s.hosts[&HP].k(), K_PHONE);
    for m in [HP, OLGA, A1, BOB] {
        s.join(m).unwrap();
    }
    // Limited at 14 %: K = 1, no provisional slot, new Joins refused.
    let c14 = Cond::phone(14, false);
    let acts = s.hosts.get_mut(&HP).unwrap().set_cond(c14, &s.auth);
    s.host_actions(HP, acts);
    s.pump();
    assert!(s.notices.contains(&(HP, HostNotice::Limited)));
    let before = s.hosts[&HP].snapshot();
    let r = w::sign_join(&s.d[CAROL].id.clone(), &gid(), &cid, 1, s.now).1;
    let cep = s.d[CAROL].ep;
    assert_eq!(s.raw_join(HP, cep, r), Err(Refusal::Limited));
    assert_eq!(s.hosts[&HP].snapshot(), before);
    for _ in 0..30 {
        for (m, l) in [(HP, 20), (OLGA, 22), (A1, 24)] {
            s.speak(m, l).unwrap();
        }
        s.step();
        assert!(s.hosts[&HP].forwarder().forwarded().len() <= 1);
    }
    // End at 7 %: End{host_leaving, next_host: backups[0]}; members redial H1 at once.
    let t = s.now;
    let acts = s.hosts.get_mut(&HP).unwrap().set_cond(Cond::phone(7, false), &s.auth);
    s.host_actions(HP, acts);
    s.pump();
    assert!(s.host_ended.contains(&(HP, EndReason::HostLeaving, Some(s.href(H1)))));
    for m in [OLGA, A1, BOB] {
        assert_eq!(s.host_of(m), (H1, 2), "{m}");
        assert!(s.joins.contains(&(m, H1, 2, t)), "no heartbeat wait for {m}");
    }
    // Wi-Fi to metered mid-call: everyone stays, Joins beyond the metered cap are refused, and
    // the phone's own user leaving ends the call with host_leaving.
    let mut s = World::new();
    s.d[HP].cond = Cond::phone(80, true);
    let cid = s.start_call(HP, HP, vec![H1]).unwrap();
    for m in [HP, OLGA, A1, BOB, CAROL] {
        s.join(m).unwrap();
    }
    let metered = Cond { metered: true, ..Cond::phone(80, true) };
    let acts = s.hosts.get_mut(&HP).unwrap().set_cond(metered, &s.auth);
    assert_eq!(acts, vec![HostAction::Notice(HostNotice::MeteredData)]);
    assert_eq!(s.roster(HP).len(), 5, "continues above the metered cap");
    let acts = s.clients.get_mut(&A1).unwrap().leave(s.now);
    s.client_actions(A1, acts);
    s.pump();
    s.advance(JOIN_MIN_GAP_MS);
    let r = w::sign_join(&s.d[A1].id.clone(), &gid(), &cid, 1, s.now).1;
    let a1ep = s.d[A1].ep;
    let before = s.hosts[&HP].snapshot();
    assert_eq!(s.raw_join(HP, a1ep, r), Err(Refusal::Full));
    assert_eq!(s.hosts[&HP].snapshot(), before);
    assert_eq!(s.roster(HP).len(), CAP_PHONE_METERED);
    let acts = s.clients.get_mut(&HP).unwrap().leave(s.now);
    s.client_actions(HP, acts);
    s.pump();
    assert!(!s.hosts.contains_key(&HP));
    for m in [OLGA, BOB, CAROL] {
        assert_eq!(s.host_of(m), (H1, 2), "{m}");
    }
}

// GC-S17. One DID on two linked devices: the newer Join replaces the older (one slot per
// DID); the older device is closed "answered elsewhere"; replays and older Joins are refused.
#[test]
fn gcall_s17_same_did_on_two_devices_newer_join_replaces_older() {
    let (mut s, cid) = base();
    s.join(OLGA).unwrap();
    s.join(A1).unwrap();
    let alice = s.did(A1);
    let a1_join = s.up_ctl.iter().rev().find_map(|(m, msg)| match msg {
        GroupCallMsg::Join { join, .. } if *m == A1 => Some(join.clone()),
        _ => None,
    });
    let a1_conn = s.route[&A1].1;
    let slot = s.hosts[&H1].slot_of(&alice).unwrap();
    s.advance(JOIN_MIN_GAP_MS);
    s.join(A2).unwrap();
    let r = s.roster(H1);
    assert_eq!((r.len(), r.values().filter(|d| **d == alice).count()), (2, 1), "one slot per DID");
    assert_eq!(s.hosts[&H1].slot_of(&alice), Some(slot));
    assert_eq!(s.hosts[&H1].conn_of(&alice), Some(s.route[&A2].1));
    assert!(s.ended(A1, ClientEnd::AnsweredElsewhere));
    assert!(s.joined(A2));
    let e = s.hosts.get_mut(&H1).unwrap();
    assert_eq!(e.on_datagram(a1_conn, &w::up_audio(&audio_dgram(1, 10)), s.now), Err(Refusal::NotParticipant));
    s.advance(JOIN_MIN_GAP_MS);
    let ep1 = s.d[A1].ep;
    let older = w::sign_join(&s.d[A1].id.clone(), &gid(), &cid, 1, T0 - 1).1;
    for (case, blob, want) in [("replay", a1_join.unwrap(), Refusal::Replay), ("older issued_ms", older, Refusal::Stale)] {
        let before = s.hosts[&H1].snapshot();
        assert_eq!(s.raw_join(H1, ep1, blob), Err(want), "{case}");
        assert_eq!(s.hosts[&H1].snapshot(), before, "{case}");
    }
    s.join(A1).unwrap();
    assert!(s.joined(A1));
    assert!(s.ended(A2, ClientEnd::AnsweredElsewhere));
    assert_eq!(s.roster(H1).len(), 2);
}

// G6-D2. S is full and a new sender crosses the threshold: its first datagram is forwarded at
// once in a provisional slot announced in `Active` (before the audio). At most one provisional
// slot. After HYST_FRAMES it is promoted (louder by HYST_DB) or dropped (and not offered again
// until it goes quiet).
#[test]
fn gcall_d2_new_speaker_forwarded_at_once_in_one_provisional_slot() {
    for loud in [true, false] {
        let (mut s, _) = base();
        for m in [OLGA, A1, BOB, CAROL, HP] {
            s.join(m).unwrap();
        }
        let steady = [(OLGA, 20u8), (A1, 24), (BOB, 28)];
        for _ in 0..AVG_FRAMES * 2 {
            for (m, l) in steady {
                s.speak(m, l).unwrap();
            }
            s.step();
        }
        let e = &s.hosts[&H1];
        let sel: BTreeSet<u8> = steady.iter().map(|(m, _)| e.slot_of(&s.did(*m)).unwrap()).collect();
        assert_eq!(e.forwarder().forwarded(), sel, "S full, nothing provisional");
        let carol = e.slot_of(&s.did(CAROL)).unwrap();
        let hp = e.slot_of(&s.did(HP)).unwrap();
        // CAROL's first speech datagram: Active first, then the audio, in the same call.
        let (d0, c0) = (s.down.len(), s.down_ctl.len());
        let level = if loud { 5 } else { 40 };
        s.speak(CAROL, level).unwrap();
        s.speak(HP, 40).unwrap(); // a second newcomer in the same frame waits
        let fwd: Vec<&(usize, Vec<u8>)> = s.down[d0..].iter().filter(|(_, b)| b[1] == carol).collect();
        assert_eq!(fwd.len(), 4, "first words go to the four others at once");
        assert!(!s.down[d0..].iter().any(|(_, b)| b[1] == hp), "at most one provisional slot");
        let actives: Vec<(usize, u32, Vec<u8>)> = s.down_ctl[c0..]
            .iter()
            .filter_map(|(m, msg)| match msg {
                GroupCallMsg::Active { r#gen, slots } => Some((*m, *r#gen, slots.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(actives.len(), 5);
        assert!(actives.iter().all(|(_, _, sl)| sl.contains(&carol) && sl.len() == 4));
        for m in [OLGA, A1, BOB, HP] {
            assert!(s.clients[&m].active().1.contains(&carol), "{m} activated the slot before mixing it");
        }
        // 200 ms later hysteresis decides.
        let mut frames_fwd = 0;
        for f in 1..=HYST_FRAMES + 5 {
            s.step();
            for (m, l) in steady {
                s.speak(m, l).unwrap();
            }
            let d = s.down.len();
            s.speak(CAROL, level).unwrap();
            frames_fwd += usize::from(s.down[d..].iter().any(|(_, b)| b[1] == carol));
            let fw = s.hosts[&H1].forwarder().forwarded();
            assert!(fw.len() <= K_DESKTOP + 1, "frame {f}: {fw:?}");
        }
        let fw = s.hosts[&H1].forwarder();
        if loud {
            assert!(fw.selected().contains(&carol), "promoted after 200 ms");
            assert_eq!(fw.selected().len(), K_DESKTOP);
            assert_eq!(frames_fwd, HYST_FRAMES as usize + 5, "never cut");
        } else {
            assert!(!fw.is_forwarded(carol), "dropped after 200 ms");
            assert_eq!(fw.selected(), &sel);
            assert!(frames_fwd >= HYST_FRAMES as usize - 1 && frames_fwd <= HYST_FRAMES as usize, "{frames_fwd}");
            assert!(s.clients[&OLGA].active().1.iter().all(|x| *x != carol));
        }
    }
}

// G10 and permit checks at the client: a Roster listing a non-member, or control from a host
// whose permit was revoked, is refused; a revoked host makes members redial the next backup.
#[test]
fn gcall_client_refuses_rogue_roster_and_redials_when_the_host_permit_is_revoked() {
    let (mut s, cid) = base();
    for m in [A1, CAROL] {
        s.join(m).unwrap();
    }
    let h1 = s.d[H1].id.clone();
    let (j, b) = w::sign_join(&s.d[MALLORY].id.clone(), &gid(), &cid, 1, s.now);
    let rogue = w::sign_roster(
        &h1,
        w::Roster { v: 1, gid: gid(), call_id: cid.clone(), host_gen: 1, host_did: s.did(H1), epoch: 999, entries: vec![w::RosterEntry::new(9, &j, &b)] },
    );
    let c = s.clients.get_mut(&A1).unwrap();
    assert_eq!(c.on_control(GroupCallMsg::Roster { roster: rogue }, s.now, &s.auth), Err(Refusal::NotMember));
    s.auth.permits.remove(&s.href(H1).permit_digest);
    s.step();
    for m in [A1, CAROL] {
        assert_eq!(s.host_of(m), (BOB, 2), "{m}");
        assert!(s.joined(m));
    }
}
