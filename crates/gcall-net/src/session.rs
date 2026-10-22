//! One call session: both engines behind one lock, and the actions they emit.

use super::*;

pub(super) struct Peer {
    pub(super) conn: Connection,
    pub(super) tx: mpsc::UnboundedSender<Out>,
}

impl Peer {
    pub(super) fn close(self, code: u32) {
        if self.tx.send(Out::Close(code)).is_err() {
            self.conn.close(code.into(), b"");
        }
    }
}

pub(super) enum Out {
    Frame(Vec<u8>),
    Close(u32),
}

pub(super) struct HostSide {
    pub(super) eng: HostEngine,
    pub(super) conns: BTreeMap<ConnId, Peer>,
    /// A takeover engine waiting for its first valid Join; dropped if that Join is refused.
    pub(super) taking: bool,
}

pub(super) enum Link {
    None,
    Local,
    Remote { seq: u64, peer: Option<Peer>, queued: Vec<Vec<u8>> },
}

pub(super) struct St {
    pub(super) host: Option<HostSide>,
    pub(super) client: Option<ClientEngine>,
    pub(super) link: Link,
    /// Host addresses given with `gcall_join`, before the directory.
    pub(super) addrs: BTreeMap<HostRef, EndpointAddr>,
    pub(super) mic: Vec<i16>,
    pub(super) mic_muted: bool,
    pub(super) last_up: u64,
    pub(super) events: Vec<GcallEvent>,
    pub(super) next_conn: u64,
    pub(super) next_seq: u64,
    /// The winner named by a `Superseded`, joined when the client ends.
    pub(super) follow: Option<HostRef>,
    /// Current local conditions (§2.4); a takeover starts with these.
    pub(super) cond: Cond,
}

pub(super) enum Work {
    H(Vec<HostAction>),
    C(Vec<ClientAction>),
}

pub(super) struct Session {
    pub(super) call_id: String,
    pub(super) id: Identity,
    pub(super) cfg: Arc<GcallConfig>,
    /// Calls run on the group endpoint (U2/U7), looked up at each dial: it may rotate (`docs/rotation.md`).
    pub(super) inner: std::sync::Weak<dyn GcallHost>,
    pub(super) handle: tokio::runtime::Handle,
    /// Session clock: Unix ms at creation plus monotonic elapsed time.
    pub(super) t0: Instant,
    pub(super) t0_unix: u64,
    pub(super) known: Known,
    pub(super) st: Mutex<St>,
}

pub(super) fn is_ended(c: &ClientEngine) -> bool {
    matches!(c.state(), ClientState::Ended(_))
}

impl Session {
    pub(super) fn new(call_id: &str, id: Identity, cfg: Arc<GcallConfig>, inner: std::sync::Weak<dyn GcallHost>, handle: tokio::runtime::Handle, known: Known) -> Arc<Self> {
        let cond = cfg.cond;
        Arc::new(Session {
            call_id: call_id.into(),
            id,
            cfg,
            inner,
            handle,
            t0: Instant::now(),
            t0_unix: now_ms(),
            known,
            st: Mutex::new(St {
                host: None,
                client: None,
                link: Link::None,
                addrs: BTreeMap::new(),
                mic: vec![],
                mic_muted: false,
                last_up: 0,
                events: vec![],
                next_conn: 1,
                next_seq: 1,
                follow: None,
                cond,
            }),
        })
    }

    pub(super) fn now(&self) -> u64 {
        self.t0_unix + self.t0.elapsed().as_millis() as u64
    }

    pub(super) fn auth(&self) -> &dyn CallAuthority {
        &*self.cfg.dir
    }

    pub(super) fn finished(&self) -> bool {
        let st = self.st.lock();
        st.host.is_none() && st.client.as_ref().is_none_or(is_ended)
    }

    /// Runs `f` under the lock, executes every engine action it queued (and their
    /// consequences), then delivers the events outside the lock.
    pub(super) fn with<R>(self: &Arc<Self>, f: impl FnOnce(&Arc<Self>, &mut St, &mut VecDeque<Work>) -> R) -> R {
        let (r, events) = {
            let mut st = self.st.lock();
            let mut q = VecDeque::new();
            let r = f(self, &mut st, &mut q);
            while let Some(work) = q.pop_front() {
                match work {
                    Work::H(acts) => {
                        for a in acts {
                            self.host_act(&mut st, a, &mut q);
                        }
                    }
                    Work::C(acts) => {
                        for a in acts {
                            self.client_act(&mut st, a, &mut q);
                        }
                    }
                }
            }
            (r, std::mem::take(&mut st.events))
        };
        for e in events {
            self.cfg.events.on_gcall_event(e);
        }
        r
    }

    pub(super) fn spawn_ticker(self: &Arc<Self>) {
        let s = self.clone();
        self.handle.spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_millis(FRAME_MS));
            iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                iv.tick().await;
                s.with(|s, st, q| s.tick(st, q));
                if s.finished() {
                    s.with(|s, st, _| s.drop_link(st));
                    break;
                }
            }
        });
    }

    pub(super) fn tick(self: &Arc<Self>, st: &mut St, q: &mut VecDeque<Work>) {
        let now = self.now();
        if let Some(h) = st.host.as_mut() {
            q.push_back(Work::H(h.eng.tick(now, self.auth())));
        }
        if let Some(c) = st.client.as_mut() {
            q.push_back(Work::C(c.tick(now, self.auth())));
            // An app that is not pushing audio would be dropped after MEMBER_DROP_MS: keep
            // the gated sender's level-only datagram going.
            let joined = c.state() == ClientState::Joined;
            if joined && matches!(st.link, Link::Remote { .. }) && now >= st.last_up + LEVEL_ONLY_MS {
                q.push_back(Work::C(vec![ClientAction::Datagram(w::up_level(SILENT_LEVEL).to_vec())]));
            }
        }
    }

    pub(super) fn push_mic(self: &Arc<Self>, pcm: &[i16]) {
        self.with(|_, st, q| {
            if st.client.as_ref().is_none_or(is_ended) {
                st.mic.clear();
                return;
            }
            st.mic.extend_from_slice(pcm);
            let mut frame = [0i16; audio::FRAME];
            while st.mic.len() >= audio::FRAME {
                if st.mic_muted {
                    frame = [0; audio::FRAME];
                } else {
                    frame.copy_from_slice(&st.mic[..audio::FRAME]);
                }
                st.mic.drain(..audio::FRAME);
                let acts = st.client.as_mut().expect("checked above").on_mic(&frame);
                q.push_back(Work::C(acts));
            }
        });
    }

    pub(super) fn pull_speaker(&self) -> Vec<i16> {
        let mut out = [0i16; audio::FRAME];
        let now = self.now();
        if let Some(c) = self.st.lock().client.as_mut() {
            c.mix(&mut out, now);
        }
        out.to_vec()
    }

    pub(super) fn status(&self) -> GcallStatus {
        let st = self.st.lock();
        let call_id = match (&st.client, &st.host) {
            (Some(c), _) => c.call_start().call_id.clone(),
            (None, Some(h)) => h.eng.call_start().call_id.clone(),
            _ => self.call_id.clone(),
        };
        GcallStatus {
            call_id,
            client: st.client.as_ref().map(|c| ClientStatus {
                state: c.state(),
                host: c.host().clone(),
                host_gen: c.host_gen(),
                my_slot: c.my_slot(),
                roster: c.roster().clone(),
                active: c.active().1.clone(),
                heard: c.roster().keys().map(|s| (*s, c.heard(*s))).collect(),
            }),
            hosting: st.host.as_ref().map(|h| HostingStatus {
                host_gen: h.eng.call_start().host_gen,
                k: h.eng.k(),
                epoch: h.eng.epoch(),
                roster: h.eng.roster(),
                forwarded: h.eng.active().1.clone(),
            }),
        }
    }

    /// Closes the client's current link (a remote one with a goodbye after its queued frames).
    pub(super) fn drop_link(&self, st: &mut St) {
        if let Link::Remote { peer: Some(p), .. } = std::mem::replace(&mut st.link, Link::None) {
            p.close(CLOSE_BYE);
        }
    }

    /// §5: become `prev`'s next-generation host, if this device is one of its backups.
    pub(super) fn take_over(&self, st: &mut St, prev: &CallStart) -> Result<(), Refusal> {
        if st.host.is_some() {
            return Err(Refusal::Refused);
        }
        let me = self.cfg.host_ref_for(&prev.gid).ok_or(Refusal::Refused)?;
        let own = st.client.as_ref().is_some_and(|c| !is_ended(c) && c.call_start().call_id == prev.call_id);
        let eng = HostEngine::takeover(self.id.clone(), prev, me, st.cond, own, self.now())?;
        let s = eng.call_start();
        st.events.push(GcallEvent::HostingStarted { call_id: s.call_id.clone(), host_gen: s.host_gen, start: eng.start_blob().clone() });
        tracing::info!("gcall: taking over {} at gen {}", s.call_id, s.host_gen);
        st.host = Some(HostSide { eng, conns: BTreeMap::new(), taking: true });
        Ok(())
    }

    /// §3.1 glare / §5 split: if the call hosted here lost to a call on the board, end it with
    /// `End{superseded, next_host: winner}`.
    pub(super) fn sweep(self: &Arc<Self>) {
        self.with(|s, st, q| {
            let Some(h) = st.host.as_mut() else { return };
            let mine = h.eng.call_start().clone();
            let winner = {
                let mut k = s.known.lock();
                let b = k.boards.entry(mine.gid.clone()).or_insert_with(|| CallBoard::new(&mine.gid, s.cfg.host_ref_for(&mine.gid)));
                let _ = b.accept_start(h.eng.start_blob(), s.auth());
                b.superseded(&mine)
            };
            if let Some(w) = winner {
                tracing::info!("gcall: {} gen {} superseded", mine.call_id, mine.host_gen);
                q.push_back(Work::H(h.eng.supersede(w)));
            }
        });
    }

    pub(super) fn host_act(self: &Arc<Self>, st: &mut St, a: HostAction, q: &mut VecDeque<Work>) {
        let now = self.now();
        let local_up = matches!(st.link, Link::Local);
        match a {
            HostAction::Control { conn: ConnId::LOCAL, frame } => {
                let (Some(c), true) = (st.client.as_mut(), local_up) else { return };
                let Ok(Some((msg, _))) = w::decode_control(&frame) else { return };
                if let Ok(acts) = c.on_control(msg, now, &*self.cfg.dir) {
                    q.push_back(Work::C(acts));
                }
            }
            HostAction::Control { conn, frame } => {
                if let Some(p) = st.host.as_ref().and_then(|h| h.conns.get(&conn)) {
                    let _ = p.tx.send(Out::Frame(frame));
                }
            }
            HostAction::Datagram { conn: ConnId::LOCAL, bytes } => {
                if let (Some(c), true) = (st.client.as_mut(), local_up) {
                    c.on_datagram(&bytes, now);
                }
            }
            HostAction::Datagram { conn, bytes } => {
                if let Some(p) = st.host.as_ref().and_then(|h| h.conns.get(&conn)) {
                    let _ = p.conn.send_datagram(Bytes::from(bytes));
                }
            }
            HostAction::Close { conn: ConnId::LOCAL, reason } => {
                if local_up {
                    st.link = Link::None;
                    if let Some(c) = st.client.as_mut() {
                        q.push_back(Work::C(c.on_closed(reason == CloseReason::AnsweredElsewhere, now, &*self.cfg.dir)));
                    }
                }
            }
            HostAction::Close { conn, reason } => {
                if let Some(p) = st.host.as_mut().and_then(|h| h.conns.remove(&conn)) {
                    p.close(close_code(reason));
                }
            }
            HostAction::Notice(n) => st.events.push(GcallEvent::Notice(n)),
            HostAction::Ended { reason, next_host, end } => {
                if let Some(h) = st.host.take() {
                    for (_, p) in h.conns {
                        p.close(CLOSE_ENDED);
                    }
                    let s = h.eng.call_start();
                    tracing::info!("gcall: stopped hosting {} gen {}: {reason:?}", s.call_id, s.host_gen);
                    if let Some(e) = end.as_ref().filter(|_| reason == EndReason::Ended)
                        && let Some(b) = self.known.lock().boards.get_mut(&s.gid)
                    {
                        let _ = b.accept_end(e, self.auth());
                    }
                    st.events.push(GcallEvent::HostingEnded { call_id: s.call_id.clone(), reason, next_host, end });
                }
            }
        }
    }

    pub(super) fn client_act(self: &Arc<Self>, st: &mut St, a: ClientAction, q: &mut VecDeque<Work>) {
        if st.client.is_none() {
            return;
        }
        match a {
            ClientAction::Dial { host } => self.act_dial(st, host, q),
            ClientAction::Control(frame) => self.act_control(st, frame, q),
            ClientAction::Datagram(bytes) => self.act_datagram(st, bytes, q),
            ClientAction::Joined { slot } => {
                let c = st.client.as_ref().expect("present");
                let s = c.call_start();
                st.events.push(GcallEvent::Joined { call_id: s.call_id.clone(), host_gen: c.host_gen(), slot, host: c.host().clone() });
            }
            ClientAction::RosterChanged => {
                let c = st.client.as_ref().expect("present");
                let s = c.call_start();
                st.events.push(GcallEvent::RosterChanged { call_id: s.call_id.clone(), host_gen: c.host_gen(), roster: c.roster().clone() });
            }
            ClientAction::Superseded { by } => {
                let c = st.client.as_ref().expect("present");
                st.events.push(GcallEvent::Superseded { call_id: c.call_start().call_id.clone(), by: by.clone() });
                st.follow = Some(by);
            }
            ClientAction::Ended(why) => self.act_ended(st, why, q),
        }
    }

    /// Dial a host: our own host engine through `ConnId::LOCAL` (taking over first as a backup),
    /// or a remote link.
    fn act_dial(self: &Arc<Self>, st: &mut St, host: HostRef, q: &mut VecDeque<Work>) {
        let now = self.now();
        let auth = &*self.cfg.dir;
        let c = st.client.as_mut().expect("present");
        let (hgen, prev) = (c.host_gen(), c.call_start().clone());
        if hgen > prev.host_gen {
            st.events.push(GcallEvent::Reconnecting { call_id: prev.call_id.clone() });
        }
        self.drop_link(st);
        if self.cfg.host_ref_for(&prev.gid).as_ref() == Some(&host) {
            // Our own host engine, through ConnId::LOCAL; as a backup, take over first.
            let have = st.host.as_ref().is_some_and(|h| {
                let s = h.eng.call_start();
                s.call_id == prev.call_id && s.host_gen == hgen
            });
            let ok = have || (hgen == prev.host_gen + 1 && self.take_over(st, &prev).is_ok());
            let c = st.client.as_mut().expect("present");
            c.set_local(ok);
            if ok {
                st.link = Link::Local;
            } else {
                q.push_back(Work::C(c.dial_failed(now, auth)));
            }
            return;
        }
        let addr = st.addrs.get(&host).cloned().or_else(|| self.cfg.dir.host_addr(&host));
        let c = st.client.as_mut().expect("present");
        c.set_local(false);
        match addr {
            None => q.push_back(Work::C(c.dial_failed(now, auth))),
            Some(addr) => {
                let seq = st.next_seq;
                st.next_seq += 1;
                st.link = Link::Remote { seq, peer: None, queued: vec![] };
                let s = self.clone();
                self.handle.spawn(async move { s.dial(seq, addr).await });
            }
        }
    }

    /// A control frame from our client: to our own host engine, the remote peer, or queued
    /// until the link is up.
    fn act_control(self: &Arc<Self>, st: &mut St, frame: Vec<u8>, q: &mut VecDeque<Work>) {
        let now = self.now();
        let auth = &*self.cfg.dir;
        match &mut st.link {
            Link::Local => {
                let Ok(Some((msg, _))) = w::decode_control(&frame) else { return };
                let Some(h) = st.host.as_mut() else { return };
                match h.eng.on_control(ConnId::LOCAL, msg, now, auth) {
                    Ok(acts) => {
                        h.taking = false;
                        q.push_back(Work::H(acts));
                    }
                    Err(e) => {
                        tracing::debug!("gcall: own Join/Leave refused: {e:?}");
                        if h.taking {
                            st.host = None;
                        }
                    }
                }
            }
            Link::Remote { peer: Some(p), .. } => {
                let _ = p.tx.send(Out::Frame(frame));
            }
            Link::Remote { peer: None, queued, .. } => queued.push(frame),
            Link::None => {}
        }
    }

    /// An audio datagram from our client.
    fn act_datagram(self: &Arc<Self>, st: &mut St, bytes: Vec<u8>, q: &mut VecDeque<Work>) {
        let now = self.now();
        st.last_up = now;
        match &st.link {
            Link::Local => {
                if let Some(h) = st.host.as_mut()
                    && let Ok(acts) = h.eng.on_datagram(ConnId::LOCAL, &bytes, now)
                {
                    q.push_back(Work::H(acts));
                }
            }
            Link::Remote { peer: Some(p), .. } => {
                let _ = p.conn.send_datagram(Bytes::from(bytes));
            }
            _ => {}
        }
    }

    /// The client ended; after a glare or split, join the winner if its CallStart is known.
    fn act_ended(self: &Arc<Self>, st: &mut St, why: ClientEnd, q: &mut VecDeque<Work>) {
        let now = self.now();
        let auth = &*self.cfg.dir;
        let c = st.client.as_ref().expect("present");
        let (call_id, gid) = (c.call_start().call_id.clone(), c.call_start().gid.clone());
        st.events.push(GcallEvent::Ended { call_id, why });
        self.drop_link(st);
        st.mic.clear();
        // §3.1 glare / §5 split: join the winner if its CallStart is known.
        let by = st.follow.take().filter(|_| why == ClientEnd::Superseded);
        let next = by.and_then(|by| {
            let mut k = self.known.lock();
            let found = k.starts.values().find(|k| k.host_ref == by && k.gid == gid).cloned();
            if found.is_none() {
                // Its CallStart is still on its way through the relay.
                k.follow = Some((by, gid.clone(), Instant::now()));
            }
            found
        });
        if let Some(start) = next
            && let Ok((c, acts)) = ClientEngine::new(self.id.clone(), start, self.cfg.note_now(), now, auth)
        {
            st.client = Some(c);
            q.push_back(Work::C(acts));
        }
    }
}

/// The wall clock in unix ms (0 if it reads before 1970).
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}
