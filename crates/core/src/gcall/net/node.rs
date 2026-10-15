//! Node hooks: shutdown, incoming connections and the `Inner` / `Node` entry points.

use super::*;

// ---- node hooks -------------------------------------------------------------------------

impl Inner {
    /// `Node::stop` / `Node::lock` / an account switch, before the endpoints close: leave the call and, when hosting, end it with
    /// `End{host_leaving, next_host}`; waits (bounded) until those frames are out.
    pub(crate) async fn gcall_shutdown(&self) {
        let svc = &self.gcall;
        if let Some(s) = svc.live() {
            let mut conns = vec![];
            {
                let st = s.st.lock();
                if let Some(h) = &st.host {
                    conns.extend(h.conns.values().map(|p| p.conn.clone()));
                }
                if let Link::Remote { peer: Some(p), .. } = &st.link {
                    conns.push(p.conn.clone());
                }
            }
            s.with(|s, st, q| {
                if let Some(c) = st.client.as_mut() {
                    q.push_back(Work::C(c.leave(s.now())));
                }
            });
            s.with(|s, st, q| {
                if let Some(h) = st.host.as_mut() {
                    let next = h.eng.next_backup(s.auth());
                    q.push_back(Work::H(h.eng.end(EndReason::HostLeaving, next)));
                }
            });
            let deadline = tokio::time::Instant::now() + FLUSH_TIMEOUT + Duration::from_millis(200);
            for c in conns {
                let _ = tokio::time::timeout_at(deadline, c.closed()).await;
            }
        }
        *svc.cur.lock() = None;
        *svc.known.lock() = Calls::default();
    }

    /// A `tinline/gcall/1` connection (called from the accept loop in `node.rs`). The first
    /// control frame must be a Join for the call this device hosts, or one it may take over.
    pub(crate) async fn handle_gcall_incoming(self: Arc<Self>, accepting: Accepting) -> Result<(), Error> {
        let conn = tokio::time::timeout(DIAL_TIMEOUT, accepting).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?;
        let first = async {
            let (send, mut recv) = conn.accept_bi().await.map_err(Error::net)?;
            let mut buf = vec![];
            let msg = read_frame(&mut recv, &mut buf).await.ok_or_else(|| Error::Protocol("no frame".into()))?;
            Ok::<_, Error>((send, recv, buf, msg))
        };
        let (send, recv, buf, msg) = match tokio::time::timeout(DIAL_TIMEOUT, first).await {
            Ok(Ok(x)) => x,
            Ok(Err(e)) => {
                conn.close(CLOSE_REFUSED.into(), b"");
                return Err(e);
            }
            Err(_) => {
                conn.close(CLOSE_REFUSED.into(), b"");
                return Err(Error::Timeout);
            }
        };
        let join = match &msg {
            GroupCallMsg::Join { join, .. } => join.open().ok(),
            _ => None,
        };
        let session = match join {
            Some(j) => self.gcall_session_for(&j),
            None => None,
        };
        let Some(s) = session else {
            conn.close(CLOSE_REFUSED.into(), b"");
            return Err(Error::Rejected("no such call here".into()));
        };
        s.serve(conn, send, recv, buf, msg).await;
        Ok(())
    }

    /// The session that serves `j`: the call hosted here, or a takeover (§5) when this device
    /// is a backup of the previous generation.
    fn gcall_session_for(self: &Arc<Self>, j: &w::Join) -> Option<Arc<Session>> {
        let svc = &self.gcall;
        if let Some(s) = svc.live() {
            let known = svc.known_start(&j.call_id);
            let ok = s.with(|s, st, _| {
                if let Some(h) = &st.host {
                    return h.eng.call_start().call_id == j.call_id;
                }
                let prev = st.client.as_ref().map(|c| c.call_start().clone()).filter(|p| p.call_id == j.call_id).or(known);
                prev.is_some_and(|p| p.host_gen + 1 == j.host_gen && s.take_over(st, &p).is_ok())
            });
            return ok.then_some(s);
        }
        // Listen-only takeover by a backup that was not in the call.
        let prev = svc.known_start(&j.call_id).filter(|p| p.host_gen + 1 == j.host_gen)?;
        let cfg = svc.cfg.lock().clone()?;
        let me = self.me().ok()?;
        let s = Session::new(&j.call_id, me.id.clone(), cfg, Arc::downgrade(self), self.handle.clone(), svc.known.clone());
        {
            let mut cur = svc.cur.lock();
            if cur.as_ref().is_some_and(|c| !c.finished()) {
                return None;
            }
            *cur = Some(s.clone());
        }
        if !s.with(|s, st, _| s.take_over(st, &prev).is_ok()) {
            return None;
        }
        s.spawn_ticker();
        Some(s)
    }
}

/// The group-call service on `Inner` (the group layer, `group::calls`, drives it from core
/// threads); `Node` wraps it below.
impl Inner {
    /// Sets the directory, event sink, own host ref and conditions. Applies to sessions
    /// started afterwards.
    pub(crate) fn gcall_configure(&self, cfg: GcallConfig) {
        // Explicit conditions count as reported (`Service::cond_known`).
        *self.gcall.cond.lock() = Some(cfg.cond);
        *self.gcall.cfg.lock() = Some(Arc::new(cfg));
    }

    pub(crate) fn gcall_cfg(&self) -> Result<Arc<GcallConfig>, Error> {
        self.gcall.cfg.lock().clone().ok_or_else(|| Error::Protocol("group calls not configured".into()))
    }

    /// A new session, unless a group call or a 1:1 call is going on.
    fn gcall_new_session(self: &Arc<Self>, call_id: &str) -> Result<Arc<Session>, Error> {
        let cfg = self.gcall_cfg()?;
        let me = self.me()?;
        if self.group_endpoint().is_none() {
            return Err(Error::NotStarted);
        }
        let s = Session::new(call_id, me.id.clone(), cfg, Arc::downgrade(self), self.handle.clone(), self.gcall.known.clone());
        // Under the 1:1 slot's lock, so that no 1:1 call begins meanwhile (it checks `in_call`).
        self.with_no_call(|| {
            let mut cur = self.gcall.cur.lock();
            if cur.as_ref().is_some_and(|c| !c.finished()) {
                return Err(Error::Busy);
            }
            *cur = Some(s.clone());
            Ok(())
        })?;
        Ok(s)
    }

    /// New local hosting conditions (§2.4: battery, charging, thermal, metered), from the
    /// platform. A call hosted here degrades as the policy says (K = 1 and no new Joins, or
    /// `End{host_leaving, next_host}`); later takeovers start with them.
    pub(crate) fn gcall_set_cond(self: &Arc<Self>, cond: Cond) {
        *self.gcall.cond.lock() = Some(cond);
        if let Some(c) = self.gcall.cfg.lock().as_mut() {
            let mut n = (**c).clone();
            n.cond = cond;
            *c = Arc::new(n);
        }
        if let Some(s) = self.gcall.live() {
            s.with(|s, st, q| {
                st.cond = cond;
                if let Some(h) = st.host.as_mut() {
                    q.push_back(Work::H(h.eng.set_cond(cond, s.auth())));
                }
            });
        }
    }

    /// A CallStart relayed through the chat hosts: remembered so that this device can take
    /// over as a backup, or follow a `Superseded`. Verified (signature, permit, starter).
    /// When it beats the call hosted here (glare, split), that call ends `superseded`.
    pub(crate) fn gcall_observe_start(self: &Arc<Self>, start: &SignedStart) -> Result<(), Error> {
        let cfg = self.gcall_cfg()?;
        let s = start.open().map_err(|e| Error::Protocol(e.to_string()))?;
        if !cfg.dir.host_permit_ok(&s.gid, &s.host_ref)
            || !cfg.dir.is_member(&s.gid, &s.starter_did)
        {
            return Err(Error::Rejected("call host not permitted".into()));
        }
        let follow = {
            let mut k = self.gcall.known.lock();
            let b = k.boards.entry(s.gid.clone()).or_insert_with(|| CallBoard::new(&s.gid, cfg.host_ref_for(&s.gid)));
            let _ = b.accept_start(start, &*cfg.dir);
            let follow = k.follow.as_ref().is_some_and(|(by, gid, t)| *by == s.host_ref && *gid == s.gid && t.elapsed() < FOLLOW_WINDOW);
            if follow {
                k.follow = None;
            }
            if k.starts.get(&s.call_id).is_none_or(|have| have.host_gen < s.host_gen) {
                k.starts.insert(s.call_id.clone(), s);
            }
            follow
        };
        if let Some(cur) = self.gcall.live() {
            cur.sweep();
        }
        if follow && let Err(e) = self.gcall_join(start, None) {
            tracing::debug!("gcall: following the glare winner failed: {e}");
        }
        Ok(())
    }

    /// A relayed `End{ended}`: the call leaves the group's board (it no longer wins a glare).
    pub(crate) fn gcall_observe_end(&self, end: &SignedEnd) -> Result<(), Error> {
        let cfg = self.gcall_cfg()?;
        let e = end.open().map_err(|e| Error::Protocol(e.to_string()))?;
        let mut k = self.gcall.known.lock();
        let b = k.boards.entry(e.gid.clone()).or_insert_with(|| CallBoard::new(&e.gid, cfg.host_ref_for(&e.gid)));
        b.accept_end(end, &*cfg.dir).map_err(|r| Error::Rejected(format!("{r:?}")))
    }

    /// Host a call. `cfg.host_ref` must be this device's configured host ref. With
    /// `own_user_in` this device's user joins at once through `ConnId::LOCAL`. Returns the
    /// signed CallStart to announce (also delivered as `HostingStarted`).
    pub(crate) fn gcall_host(self: &Arc<Self>, cfg: HostConfig) -> Result<SignedStart, Error> {
        let gc = self.gcall_cfg()?;
        if gc.host_ref_for(&cfg.gid).as_ref() != Some(&cfg.host_ref) {
            return Err(Error::Protocol("host_ref is not this device's".into()));
        }
        let own = cfg.own_user_in;
        let s = self.gcall_new_session(&cfg.call_id)?;
        let res = s.with(|s, st, q| {
            let now = s.now();
            let eng = HostEngine::new(s.id.clone(), cfg, now).map_err(|e| Error::Rejected(format!("{e:?}")))?;
            let blob = eng.start_blob().clone();
            let start = eng.call_start().clone();
            st.events.push(GcallEvent::HostingStarted { call_id: start.call_id.clone(), host_gen: start.host_gen, start: blob.clone() });
            st.host = Some(HostSide { eng, conns: BTreeMap::new(), taking: false });
            if own {
                let (c, acts) = ClientEngine::new(s.id.clone(), start, s.cfg.note_now(), now, &*s.cfg.dir).map_err(|e| Error::Rejected(format!("{e:?}")))?;
                st.client = Some(c);
                q.push_back(Work::C(acts));
            }
            Ok(blob)
        });
        match res {
            Ok(_) => s.spawn_ticker(),
            Err(_) => *self.gcall.cur.lock() = None,
        }
        res
    }

    /// Join the call announced by `start`. `host_addr` is where its host can be reached (else
    /// the directory is asked). Progress arrives as `GcallEvent`s.
    pub(crate) fn gcall_join(
        self: &Arc<Self>,
        start: &SignedStart,
        host_addr: Option<EndpointAddr>,
    ) -> Result<(), Error> {
        let gc = self.gcall_cfg()?;
        let s = start.open().map_err(|e| Error::Protocol(e.to_string()))?;
        self.gcall_observe_start(start)?;
        // Joining the call this device already hosts listen-only: add the user to it.
        if let Some(cur) = self.gcall.live() {
            self.not_in_call()?;
            return cur.with(|cur, st, q| {
                let hosts_it = st.host.as_ref().is_some_and(|h| h.eng.call_start().call_id == s.call_id);
                if !hosts_it || st.client.as_ref().is_some_and(|c| !is_ended(c)) {
                    return Err(Error::Busy);
                }
                let start = st.host.as_ref().expect("checked").eng.call_start().clone();
                let (c, acts) = ClientEngine::new(cur.id.clone(), start, gc.note_now(), cur.now(), &*gc.dir).map_err(|e| Error::Rejected(format!("{e:?}")))?;
                st.client = Some(c);
                q.push_back(Work::C(acts));
                Ok(())
            });
        }
        let sess = self.gcall_new_session(&s.call_id)?;
        let host = s.host_ref.clone();
        let res = sess.with(|sess, st, q| {
            if let Some(a) = host_addr {
                st.addrs.insert(host, a);
            }
            let (c, acts) = ClientEngine::new(sess.id.clone(), s, gc.note_now(), sess.now(), &*gc.dir).map_err(|e| Error::Rejected(format!("{e:?}")))?;
            st.client = Some(c);
            q.push_back(Work::C(acts));
            Ok(())
        });
        match res {
            Ok(()) => sess.spawn_ticker(),
            Err(_) => *self.gcall.cur.lock() = None,
        }
        res
    }

    /// Leave the call (signed Leave). A desktop host keeps hosting listen-only; a phone host
    /// ends the call with `host_leaving` (§2.4, done by the engine).
    pub(crate) fn gcall_leave(self: &Arc<Self>) {
        if let Some(s) = self.gcall.live() {
            s.with(|s, st, q| {
                if let Some(c) = st.client.as_mut() {
                    q.push_back(Work::C(c.leave(s.now())));
                }
            });
        }
    }

    /// As host: end the call for everyone (`End{ended}`).
    pub(crate) fn gcall_end(self: &Arc<Self>) {
        if let Some(s) = self.gcall.live() {
            s.with(|_, st, q| {
                if let Some(h) = st.host.as_mut() {
                    q.push_back(Work::H(h.eng.end(EndReason::Ended, None)));
                }
            });
        }
    }

    /// Mute this device's microphone: silence is sent instead, so the sender gate closes and
    /// only level-only datagrams go out.
    pub(crate) fn gcall_set_mic_muted(&self, muted: bool) {
        if let Some(s) = self.gcall.live() {
            s.st.lock().mic_muted = muted;
        }
    }

    /// Local mute of one participant (§3.4, §6); the host is not told.
    pub(crate) fn gcall_mute_participant(&self, did: &str, muted: bool) {
        if let Some(s) = self.gcall.live()
            && let Some(c) = s.st.lock().client.as_mut()
        {
            c.set_muted(did, muted);
        }
    }

    /// Microphone PCM, 48 kHz mono, any length (what `Node::push_mic` does when no 1:1 call
    /// is active).
    pub(crate) fn gcall_push_mic(&self, pcm: &[i16]) {
        self.gcall.push_mic(pcm);
    }

    /// The next 20 ms of the group-call mix.
    pub(crate) fn gcall_pull_speaker(&self) -> Vec<i16> {
        self.gcall.pull_speaker()
    }

    pub(crate) fn gcall_status(&self) -> Option<GcallStatus> {
        self.gcall.cur.lock().clone().map(|s| s.status())
    }

    /// This node's group endpoint on loopback (direct, no relay). It must be up.
    pub(crate) fn gcall_loopback_addr(&self) -> Result<EndpointAddr, Error> {
        let ep = self.group_endpoint().ok_or(Error::NotStarted)?;
        let mut a = EndpointAddr::new(ep.id());
        for s in ep.bound_sockets() {
            let ip: std::net::IpAddr = if s.is_ipv4() { std::net::Ipv4Addr::LOCALHOST.into() } else { std::net::Ipv6Addr::LOCALHOST.into() };
            a = a.with_ip_addr(std::net::SocketAddr::new(ip, s.port()));
        }
        Ok(a)
    }
}

/// Group calls on a [`crate::Node`]. **Unstable** plain Rust; the app API is the group layer's
/// UniFFI calls (`group::calls`). Calls run on the group endpoint (U2/U7): the calls that start
/// a session bring it up first.
impl crate::Node {
    /// Sets the directory, event sink, own host ref and conditions. Applies to sessions
    /// started afterwards. Group chat configures itself (`GroupCallDir`) unless this was called.
    pub fn gcall_configure(&self, cfg: GcallConfig) {
        self.inner.gcall_configure(cfg)
    }

    /// The group endpoint, bound (otherwise it is bound only once the account has a group).
    fn gcall_ensure_endpoint(&self) -> Result<(), Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            inner.group_ensure(&core).await.map(|_| ())
        })?
    }

    /// New local hosting conditions (§2.4: battery, charging, thermal, metered), from the
    /// platform. A call hosted here degrades as the policy says (K = 1 and no new Joins, or
    /// `End{host_leaving, next_host}`); later takeovers and the advertised `CallHostStatus`
    /// use them.
    pub fn gcall_set_cond(&self, cond: Cond) {
        self.inner.gcall_set_cond(cond)
    }

    /// A CallStart relayed through the chat hosts (`Inner::gcall_observe_start`).
    pub fn gcall_observe_start(&self, start: &SignedStart) -> Result<(), Error> {
        self.inner.gcall_observe_start(start)
    }

    /// A relayed `End{ended}`: the call leaves the group's board.
    pub fn gcall_observe_end(&self, end: &SignedEnd) -> Result<(), Error> {
        self.inner.gcall_observe_end(end)
    }

    /// Host a call (`Inner::gcall_host`).
    pub fn gcall_host(&self, cfg: HostConfig) -> Result<SignedStart, Error> {
        self.gcall_ensure_endpoint()?;
        self.inner.gcall_host(cfg)
    }

    /// Join the call announced by `start` (`Inner::gcall_join`).
    pub fn gcall_join(
        &self,
        start: &SignedStart,
        host_addr: Option<EndpointAddr>,
    ) -> Result<(), Error> {
        self.gcall_ensure_endpoint()?;
        self.inner.gcall_join(start, host_addr)
    }

    pub fn gcall_leave(&self) {
        self.inner.gcall_leave()
    }

    pub fn gcall_end(&self) {
        self.inner.gcall_end()
    }

    pub fn gcall_set_mic_muted(&self, muted: bool) {
        self.inner.gcall_set_mic_muted(muted)
    }

    pub fn gcall_mute_participant(&self, did: &str, muted: bool) {
        self.inner.gcall_mute_participant(did, muted)
    }

    pub fn gcall_push_mic(&self, pcm: &[i16]) {
        self.inner.gcall_push_mic(pcm)
    }

    pub fn gcall_pull_speaker(&self) -> Vec<i16> {
        self.inner.gcall_pull_speaker()
    }

    pub fn gcall_status(&self) -> Option<GcallStatus> {
        self.inner.gcall_status()
    }

    /// This node's group endpoint on loopback (direct, no relay), bound if needed: for tests
    /// and same-machine hosts.
    pub fn gcall_loopback_addr(&self) -> Result<EndpointAddr, Error> {
        self.gcall_ensure_endpoint()?;
        self.inner.gcall_loopback_addr()
    }
}
