//! The in-memory world that routes engine actions between devices.

pub(super) use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) use identity::Identity;
pub(super) use proto::SignedBlob;
pub(super) use proto::gcall::{self as w, GroupCallMsg};

pub(super) use super::super::client::ClientEnd;
pub(super) use super::super::policy::*;
pub(super) use super::super::select::*;
pub(super) use super::super::*;

pub(super) const OLGA: usize = 0;
pub(super) const A1: usize = 1; // alice, device 1
pub(super) const A2: usize = 2; // alice, device 2 (same DID)
pub(super) const BOB: usize = 3;
pub(super) const CAROL: usize = 4;
pub(super) const H1: usize = 5;
pub(super) const HP: usize = 6; // phone
pub(super) const MALLORY: usize = 7; // not a member
pub(super) const N: usize = 8;
pub(super) const CALL_HOSTS: [usize; 4] = [H1, BOB, OLGA, HP];
pub(super) const T0: u64 = 1_800_000_000_000;

pub(super) fn ident(seed: u8) -> Identity {
    let m = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
    identity::recover(&m.to_string()).unwrap()
}

pub(super) fn gid() -> String {
    "a1".repeat(32)
}

pub(super) fn audio_dgram(seq: u32, level: u8) -> Vec<u8> {
    let mut v = seq.to_be_bytes().to_vec();
    v.push(level);
    v.extend([0xA5u8; 120]);
    v
}

pub(super) fn decode(frame: &[u8]) -> GroupCallMsg {
    w::decode_control(frame).unwrap().unwrap().0
}

pub(super) struct Dev {
    pub(super) id: Identity,
    pub(super) ep: [u8; 32],
    pub(super) href: Option<HostRef>,
    pub(super) board: CallBoard,
    pub(super) cond: Cond,
    pub(super) online: bool,
    pub(super) seq: u32,
}

pub(super) enum Ev {
    ToHost(usize, ConnId, Vec<u8>, bool), // (host, conn, bytes, is_control)
    ToClient(usize, (usize, ConnId), Vec<u8>, bool),
    Close(usize, ConnId, CloseReason),
}

pub(super) struct World {
    pub(super) now: u64,
    pub(super) auth: StaticAuthority,
    pub(super) d: Vec<Dev>,
    pub(super) hosts: BTreeMap<usize, HostEngine>,
    pub(super) clients: BTreeMap<usize, ClientEngine>,
    pub(super) route: BTreeMap<usize, (usize, ConnId)>,
    pub(super) conns: BTreeMap<(usize, ConnId), usize>,
    pub(super) next_conn: u64,
    pub(super) q: VecDeque<Ev>,
    pub(super) cut: BTreeSet<(usize, usize)>,
    pub(super) hold_relay: bool,
    pub(super) held: Vec<SignedBlob>,
    pub(super) taking: BTreeSet<usize>,
    // Last upstream datagram per device; idle joined clients send the gated level-only
    // keepalive every LEVEL_ONLY_MS like a real silence-gated sender.
    pub(super) last_up: BTreeMap<usize, u64>,
    pub(super) ends: Vec<(usize, ClientEnd, u64)>,
    pub(super) joins: Vec<(usize, usize, u32, u64)>, // (member, host dev, gen, at)
    pub(super) host_ended: Vec<(usize, EndReason, Option<HostRef>)>,
    pub(super) notices: Vec<(usize, HostNotice)>,
    pub(super) down: Vec<(usize, Vec<u8>)>,
    pub(super) up_ctl: Vec<(usize, GroupCallMsg)>,
    pub(super) down_ctl: Vec<(usize, GroupCallMsg)>,
}

impl World {
    pub(super) fn new() -> Self {
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

    pub(super) fn did(&self, i: usize) -> String {
        self.d[i].id.did().to_string()
    }

    pub(super) fn href(&self, i: usize) -> HostRef {
        self.d[i].href.clone().unwrap()
    }

    pub(super) fn dev_of(&self, r: &HostRef) -> usize {
        (0..N).find(|&i| self.d[i].href.as_ref() == Some(r)).unwrap()
    }

    pub(super) fn start_call(&mut self, h: usize, starter: usize, backups: Vec<usize>) -> Result<String, Refusal> {
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
    pub(super) fn announce(&mut self, h: usize, blob: SignedBlob) {
        let _ = self.d[h].board.accept_start(&blob, &self.auth);
        if self.hold_relay {
            self.held.push(blob);
        } else {
            self.relay(&blob);
        }
    }

    pub(super) fn relay(&mut self, blob: &SignedBlob) {
        for i in 0..N {
            if self.d[i].online && i != MALLORY {
                let _ = self.d[i].board.accept_start(blob, &self.auth);
            }
        }
        self.settle();
    }

    pub(super) fn flush_relay(&mut self) {
        self.hold_relay = false;
        for b in std::mem::take(&mut self.held) {
            self.relay(&b);
        }
    }

    // Glare and split: a host whose call lost ends it as superseded.
    pub(super) fn settle(&mut self) {
        let hs: Vec<usize> = self.hosts.keys().copied().collect();
        for h in hs {
            let Some(winner) = self.d[h].board.superseded(self.hosts[&h].call_start()) else { continue };
            let acts = self.hosts.get_mut(&h).unwrap().supersede(winner);
            self.host_actions(h, acts);
        }
        self.pump();
    }

    pub(super) fn join(&mut self, m: usize) -> Result<(), Refusal> {
        let start = self.d[m].board.shown().cloned().ok_or(Refusal::Unknown)?;
        self.join_start(m, start)
    }

    pub(super) fn join_start(&mut self, m: usize, start: w::CallStart) -> Result<(), Refusal> {
        let (c, acts) = ClientEngine::new(self.d[m].id.clone(), start, None, self.now, &self.auth)?;
        self.clients.insert(m, c);
        self.client_actions(m, acts);
        self.pump();
        Ok(())
    }

    pub(super) fn client_actions(&mut self, m: usize, acts: Vec<ClientAction>) {
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

    pub(super) fn host_actions(&mut self, h: usize, acts: Vec<HostAction>) {
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

    pub(super) fn pump(&mut self) {
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
    pub(super) fn maybe_take_over(&mut self, h: usize, msg: &GroupCallMsg, _m: usize) {
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

    pub(super) fn step(&mut self) {
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

    pub(super) fn advance(&mut self, ms: u64) {
        for _ in 0..ms / FRAME_MS {
            self.step();
        }
    }

    pub(super) fn send_up(&mut self, m: usize, bytes: Vec<u8>) -> Result<(), Refusal> {
        let (h, conn) = *self.route.get(&m).ok_or(Refusal::Unknown)?;
        self.last_up.insert(m, self.now);
        let e = self.hosts.get_mut(&h).ok_or(Refusal::Unknown)?;
        let acts = e.on_datagram(conn, &bytes, self.now)?;
        self.host_actions(h, acts);
        self.pump();
        Ok(())
    }

    pub(super) fn speak(&mut self, m: usize, level: u8) -> Result<(), Refusal> {
        self.d[m].seq += 1;
        let a = audio_dgram(self.d[m].seq, level);
        self.send_up(m, w::up_audio(&a))
    }

    pub(super) fn roster(&self, h: usize) -> BTreeMap<u8, String> {
        self.hosts[&h].roster()
    }

    pub(super) fn joined(&self, m: usize) -> bool {
        self.clients.get(&m).is_some_and(|c| c.state() == ClientState::Joined)
    }

    pub(super) fn host_of(&self, m: usize) -> (usize, u32) {
        let c = &self.clients[&m];
        (self.dev_of(c.host()), c.host_gen())
    }

    pub(super) fn ended(&self, m: usize, e: ClientEnd) -> bool {
        self.ends.iter().any(|(d, x, _)| *d == m && *x == e)
    }

    // A raw Join for `m` on a fresh connection from `ep`.
    pub(super) fn raw_join(&mut self, h: usize, ep: [u8; 32], join: SignedBlob) -> Result<Vec<HostAction>, Refusal> {
        self.next_conn += 1;
        let conn = ConnId(self.next_conn);
        let e = self.hosts.get_mut(&h).unwrap();
        e.connected(conn, ep);
        e.on_control(conn, GroupCallMsg::Join { join, note: None }, self.now, &self.auth)
    }
}

pub(super) fn base() -> (World, String) {
    let mut s = World::new();
    let cid = s.start_call(H1, OLGA, vec![BOB, OLGA]).unwrap();
    (s, cid)
}
