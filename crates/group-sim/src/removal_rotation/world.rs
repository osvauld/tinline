use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RouteErr {
    SelfOffline,
    PeerOffline,
    EndpointGone,
    Partitioned,
    NoAddress,
    Lost,
    Refused,
    Removed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Path {
    MemberDialedHost,
    HostDialedMember,
}
#[derive(Clone, Debug)]
pub(super) struct Frame {
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) kind: &'static str,
    pub(super) bytes: Vec<u8>,
}
pub(super) struct World {
    pub(super) ctx: Ctx,
    pub(super) rng: SimRng,
    pub(super) now: u64,
    pub(super) d: Vec<Device>,
    pub(super) hosts: Vec<usize>,
    pub(super) live: BTreeMap<String, usize>,
    pub(super) cut: BTreeSet<(usize, usize)>,
    pub(super) frames: Vec<Frame>,
}
impl World {
    pub(super) fn new(cfg: Cfg) -> Self {
        let mut rng = SimRng(cfg.seed);
        // olga alice bob dave carol erin hugo ivan judy
        let ids: Vec<Identity> = (80u8..89).map(ident).collect();
        let owners = [0usize, 1, 1, 2, 3, 4, 5, 6, 7, 8];
        let g = Genesis::create(&ids[0], [11; 32]);
        let gid = g.id();
        let admin_ids: BTreeSet<usize> = [1usize, 2, 3].into_iter().collect(); // alice bob dave
        let mut grants = BTreeMap::new();
        for k in 1..=5usize {
            let (caps, delegable): (&[Cap], bool) = if admin_ids.contains(&k) {
                (&[Cap::Read, Cap::Post, Cap::Delegate, Cap::Admin], true)
            } else {
                (&[Cap::Read, Cap::Post], false)
            };
            let gr = MGrant::issue(
                &ids[0],
                &gid,
                ids[k].did(),
                None,
                caps,
                delegable,
                k as u64,
                None,
            );
            grants.insert(ids[k].did().to_string(), gr);
        }
        let creator = ids[0].did().to_string();
        let mut admins: BTreeSet<String> = admin_ids
            .iter()
            .map(|&k| ids[k].did().to_string())
            .collect();
        admins.insert(creator.clone());
        let ctx = Ctx {
            cfg: cfg.clone(),
            g,
            gid: gid.clone(),
            creator,
            admins,
            base: ids[..6].iter().map(|i| i.did().to_string()).collect(),
            grants,
        };
        let mut d = vec![];
        for (dv, &k) in owners.iter().enumerate() {
            let id = ids[k].clone();
            let writer = rng.writer();
            let slot = rng.next_u64();
            let group_key = SigningKey::from_bytes(&rng.bytes32());
            let my_note = EpNote::sign(&id, &gid, slot, &ep("grp", &group_key), 1);
            let is_host = cfg.hosts.contains(&dv);
            d.push(Device {
                w: Writer::new(id.clone(), writer),
                id,
                slot,
                rep: Replica::new(&ctx, is_host),
                group_key,
                my_note,
                host: None,
                acks: BTreeMap::new(),
                op_out: vec![],
                outbox: vec![],
                refused: vec![],
                online: true,
                sched: is_host.then(Sched::default),
                ann: None,
            });
        }
        let mut w = World {
            ctx,
            rng,
            now: 0,
            d,
            hosts: vec![],
            live: BTreeMap::new(),
            cut: BTreeSet::new(),
            frames: vec![],
        };
        for i in 0..w.d.len() {
            let e = w.gep(i);
            w.live.insert(e, i);
        }
        for h in cfg.hosts.clone() {
            let permit = w.permit(h, 10 + h as u64);
            w.d[h].host = Some(HostRecord {
                note: w.d[h].my_note.clone(),
                permit,
            });
            w.hosts.push(h);
        }
        // Join bootstrap (invite/ticket) is a fixture: every member and host accepts every
        // host record; members hand their group note to every host.
        for h in w.hosts.clone() {
            let rec = w.d[h].host.as_ref().unwrap().wire();
            let mut targets = MEMBERS.to_vec();
            targets.extend(w.hosts.iter().copied());
            for i in targets {
                w.d[i].rep.accept_host_record(&w.ctx, &rec).unwrap();
            }
            for m in MEMBERS {
                let n = w.d[m].my_note.wire();
                w.d[h].rep.accept_note(&w.ctx, &n).unwrap();
                let k = w.hk(h);
                w.d[m].acks.insert(k, 1);
            }
        }
        w
    }
    pub(super) fn did(&self, i: usize) -> String {
        self.d[i].id.did().to_string()
    }
    pub(super) fn hk(&self, i: usize) -> String {
        dir_key(&self.did(i), self.d[i].slot)
    }
    pub(super) fn gep(&self, i: usize) -> String {
        ep("grp", &self.d[i].group_key)
    }
    pub(super) fn send(&mut self, from: usize, to: usize, kind: &'static str, bytes: Vec<u8>) {
        self.frames.push(Frame {
            from,
            to,
            kind,
            bytes,
        });
    }
    pub(super) fn cut(&mut self, a: usize, b: usize) {
        self.cut.insert(pair(a, b));
    }
    pub(super) fn heal(&mut self, a: usize, b: usize) {
        self.cut.remove(&pair(a, b));
    }
    pub(super) fn dial(&self, from: usize, to_ep: &str) -> Result<usize, RouteErr> {
        if !self.d[from].online {
            return Err(RouteErr::SelfOffline);
        }
        let Some(&j) = self.live.get(to_ep) else {
            return Err(RouteErr::EndpointGone);
        };
        if !self.d[j].online {
            return Err(RouteErr::PeerOffline);
        }
        if self.cut.contains(&pair(from, j)) {
            return Err(RouteErr::Partitioned);
        }
        Ok(j)
    }
    // Full byte-level device state for rejection tests.
    pub(super) fn snap(&self, i: usize) -> Vec<u8> {
        let d = &self.d[i];
        let refused: Vec<(&Post, &Why)> = d.refused.iter().map(|(p, w)| (p, w)).collect();
        serde_json::to_vec(&(
            d.rep.snap(),
            &d.my_note,
            &d.acks,
            &d.op_out,
            &d.outbox,
            refused,
            &d.host,
            &d.sched,
            &d.ann,
        ))
        .unwrap()
    }
    pub(super) fn received(&self, i: usize) -> Vec<u8> {
        let mut hay: Vec<u8> = self
            .frames
            .iter()
            .filter(|f| f.to == i)
            .flat_map(|f| f.bytes.clone())
            .collect();
        hay.extend(self.snap(i));
        hay
    }
    pub(super) fn permit(&self, h: usize, nonce: u64) -> MGrant {
        MGrant::issue(
            &self.d[OLGA].id,
            &self.ctx.gid,
            &self.did(h),
            Some(self.d[h].slot),
            &[Cap::Host],
            false,
            nonce,
            None,
        )
    }
    pub(super) fn host_ep(&self, i: usize, h: usize) -> Option<String> {
        self.d[i]
            .rep
            .hosts
            .get(&self.hk(h))
            .map(|r| r.note.claims.endpoint.clone())
    }
    pub(super) fn accept_host_record(&mut self, i: usize, bytes: &[u8]) -> Result<(), Refuse> {
        self.d[i].rep.accept_host_record(&self.ctx, bytes)
    }
    pub(super) fn host_accept_note(&mut self, h: usize, bytes: &[u8]) -> Result<(), Refuse> {
        self.d[h].rep.accept_note(&self.ctx, bytes)
    }
    pub(super) fn accept_op(&mut self, i: usize, bytes: &[u8]) -> Result<Acc, Refuse> {
        self.d[i].rep.accept_op(&self.ctx, bytes)
    }
    pub(super) fn accept_post(&mut self, i: usize, p: Post) -> Verdict {
        self.d[i].rep.accept_post(&self.ctx, p)
    }
    pub(super) fn accept_announce(&mut self, i: usize, bytes: &[u8]) -> Result<(), Refuse> {
        let me = self.did(i);
        self.d[i].rep.accept_announce(&self.ctx, bytes, &me)
    }
    pub(super) fn admits(&mut self, x: usize, y: usize) -> Result<(), Refuse> {
        let Some(r) = self.d[y].host.clone() else {
            return Err(Refuse::NotHost);
        };
        match self.accept_host_record(x, &r.wire()) {
            Ok(()) | Err(Refuse::Stale) => {}
            Err(e) => return Err(e),
        }
        if self.d[x].rep.hosts.get(&self.hk(y)) != Some(&r) {
            return Err(Refuse::Stale);
        }
        Ok(())
    }
    // ---- ops ----
    pub(super) fn flag_and_submit(&mut self, i: usize, target: usize) -> SignedOp {
        let nonce = self.rng.next_u64();
        let seen = self.d[i].rep.ops.keys().cloned().collect();
        let grant = self.ctx.grants.get(&self.did(i)).cloned();
        let op = SignedOp::sign(
            &self.d[i].id,
            &self.ctx.gid,
            grant,
            nonce,
            seen,
            Op::Flag {
                target: self.did(target),
            },
        );
        assert_eq!(self.accept_op(i, &op.wire()), Ok(Acc::Applied), "flag {i}");
        self.d[i].op_out.push(op.clone());
        self.after();
        op
    }
    // ---- content ----
    pub(super) fn post(&mut self, m: usize, text: &str) -> Post {
        let seen = self.d[m].rep.ops.keys().cloned().collect();
        self.post_citing(m, text, seen)
    }
    // A post citing an explicit frontier. With a pre-removal frontier this is the D02
    // backdating shape: identical to an honest offline post.
    pub(super) fn post_citing(&mut self, m: usize, text: &str, frontier: BTreeSet<String>) -> Post {
        let dv = &mut self.d[m];
        dv.w.observe(&dv.rep.board);
        let p = dv.w.post(&self.ctx.gid, text, frontier);
        dv.outbox.push(p.clone());
        p
    }
    // ---- calls (NEW fixture): caller dials the host it knows; host relays to the callee ----
    pub(super) fn call(&mut self, caller: usize, h: usize, callee: usize) -> Result<(), Refuse> {
        let nonce = self.rng.next_u64();
        let req = CallReq::sign(
            &self.d[caller].id,
            CallBody {
                group: self.ctx.gid.clone(),
                caller: self.did(caller),
                callee: self.did(callee),
                nonce,
            },
        );
        let Some(e) = self.host_ep(caller, h) else {
            return Err(Refuse::NotHost);
        };
        if self.dial(caller, &e) != Ok(h) {
            return Err(Refuse::Unreachable);
        }
        self.send(caller, h, "call", req.wire());
        self.d[h].rep.accept_call(&self.ctx, &req.wire(), None)?;
        let ce = self.d[h]
            .rep
            .dir
            .as_ref()
            .and_then(|dir| dir.get(&self.hk(callee)))
            .map(|n| n.claims.endpoint.clone());
        match ce.map(|e| self.dial(h, &e)) {
            Some(Ok(j)) if j == callee => {}
            _ => return Err(Refuse::Unreachable),
        }
        self.send(h, callee, "call", req.wire());
        let me = self.did(callee);
        self.d[callee]
            .rep
            .accept_call(&self.ctx, &req.wire(), Some(&me))
    }
    // ---- links ----
    pub(super) fn connect(&mut self, m: usize, h: usize) -> Result<Path, RouteErr> {
        let r = self.connect_inner(m, h);
        self.after();
        r
    }
    // Copied from flags.rs @82d221f `connect_inner`; changed: the removed member's dial gets
    // the signed notice AND (U11, section 11 Q8 = yes) her queued posts are uploaded and
    // judged one by one (signed frontier + cap candidate). Nothing else flows to her.
    pub(super) fn connect_inner(&mut self, m: usize, h: usize) -> Result<Path, RouteErr> {
        if !self.d[m].online {
            return Err(RouteErr::SelfOffline);
        }
        if !self.d[h].online {
            return Err(RouteErr::PeerOffline);
        }
        let member_side = self.host_ep(m, h).and_then(|e| self.dial(m, &e).ok()) == Some(h);
        if self.d[h].host.is_some() && self.d[h].rep.der.removed.contains(&self.did(m)) {
            if member_side {
                self.removal_notice(m, h);
                for p in std::mem::take(&mut self.d[m].outbox) {
                    self.send(m, h, "post", p.wire());
                    if let Verdict::Rejected(why) = self.accept_post(h, p.clone()) {
                        self.d[m].refused.push((p, why));
                    }
                }
                return Err(RouteErr::Removed);
            }
            return Err(RouteErr::Lost);
        }
        let path = if member_side {
            Path::MemberDialedHost
        } else {
            let e = self.d[h]
                .rep
                .dir
                .as_ref()
                .and_then(|dir| dir.get(&self.hk(m)))
                .map(|n| n.claims.endpoint.clone());
            match e.map(|e| self.dial(h, &e)) {
                Some(Ok(j)) if j == m => Path::HostDialedMember,
                _ => return Err(RouteErr::Lost),
            }
        };
        if self.admits(m, h).is_err() {
            return Err(RouteErr::Refused);
        }
        self.exchange(m, h);
        Ok(path)
    }
    pub(super) fn connect_from_host(&mut self, h: usize, m: usize) -> Result<Path, RouteErr> {
        let Some(e) = self.d[h]
            .rep
            .dir
            .as_ref()
            .and_then(|dir| dir.get(&self.hk(m)))
            .map(|n| n.claims.endpoint.clone())
        else {
            return Err(RouteErr::NoAddress);
        };
        if self.dial(h, &e)? != m {
            return Err(RouteErr::EndpointGone);
        }
        if self.admits(m, h).is_err() {
            return Err(RouteErr::Refused);
        }
        self.exchange(m, h);
        self.after();
        Ok(Path::HostDialedMember)
    }
    // Copied from flags.rs @82d221f: the notice is the signed flags + their causal frontier.
    pub(super) fn removal_notice(&mut self, m: usize, h: usize) {
        let me = self.did(m);
        let fl = self.d[h]
            .rep
            .der
            .flags
            .get(&me)
            .cloned()
            .unwrap_or_default();
        for id in closure(&self.d[h].rep.ops, &fl) {
            let o = self.d[h].rep.ops[&id].clone();
            self.send(h, m, "removal-notice", o.wire());
            let _ = self.accept_op(m, &o.wire());
        }
    }
    // Copied from flags.rs @82d221f; added: a host hands its pending announcement to the member.
    pub(super) fn exchange(&mut self, m: usize, h: usize) {
        let (hkey, mkey) = (self.hk(h), self.hk(m));
        let note = self.d[m].my_note.clone();
        if self.d[m].acks.get(&hkey).copied().unwrap_or(0) < note.claims.seq {
            self.send(m, h, "ep-note", note.wire());
            let _ = self.host_accept_note(h, &note.wire());
        }
        for o in std::mem::take(&mut self.d[m].op_out) {
            self.send(m, h, "op", o.wire());
            let _ = self.accept_op(h, &o.wire());
        }
        for p in std::mem::take(&mut self.d[m].outbox) {
            self.send(m, h, "post", p.wire());
            if let Verdict::Rejected(why) = self.accept_post(h, p.clone()) {
                self.d[m].refused.push((p, why));
            }
        }
        let recs: Vec<HostRecord> = self.d[h].rep.hosts.values().cloned().collect();
        for r in recs {
            let k = dir_key(&r.note.claims.owner, r.note.claims.slot);
            if self.d[m].rep.hosts.get(&k) != Some(&r) {
                self.send(h, m, "host-record", r.wire());
                let _ = self.accept_host_record(m, &r.wire());
            }
        }
        let ops: Vec<SignedOp> = self.d[h]
            .rep
            .ops
            .iter()
            .filter(|(id, _)| !self.d[m].rep.ops.contains_key(*id))
            .map(|(_, o)| o.clone())
            .collect();
        for o in ops {
            self.send(h, m, "op", o.wire());
            let _ = self.accept_op(m, &o.wire());
        }
        let missing: Vec<Post> = self.d[h]
            .rep
            .store
            .iter()
            .filter(|(id, _)| !self.d[m].rep.store.contains_key(*id))
            .map(|(_, p)| p.clone())
            .collect();
        for p in missing {
            self.send(h, m, "post", p.wire());
            self.accept_post(m, p);
        }
        if let Some(a) = self.d[h].ann.clone() {
            let have = self.d[m].rep.announced.get(&hkey).map(|c| c.body.seq);
            if a.body.rotate_at > self.now && have < Some(a.body.seq) {
                self.send(h, m, "rot-announce", a.wire());
                let _ = self.accept_announce(m, &a.wire());
            }
        }
        let acked = self.d[h]
            .rep
            .dir
            .as_ref()
            .and_then(|dir| dir.get(&mkey))
            .map(|n| n.claims.seq);
        if let Some(s) = acked {
            self.d[m].acks.insert(hkey, s);
        }
        let dv = &mut self.d[m];
        dv.w.observe(&dv.rep.board);
    }
    pub(super) fn host_sync(&mut self, a: usize, b: usize) -> Result<(), RouteErr> {
        let r = self.host_sync_inner(a, b);
        self.after();
        r
    }
    pub(super) fn host_sync_inner(&mut self, a: usize, b: usize) -> Result<(), RouteErr> {
        let (ka, kb) = (self.hk(a), self.hk(b));
        let ea = self.d[b]
            .rep
            .hosts
            .get(&ka)
            .map(|r| r.note.claims.endpoint.clone());
        let eb = self.d[a]
            .rep
            .hosts
            .get(&kb)
            .map(|r| r.note.claims.endpoint.clone());
        let (Some(ea), Some(eb)) = (ea, eb) else {
            return Err(RouteErr::Refused);
        };
        if self.dial(a, &eb) != Ok(b) && self.dial(b, &ea) != Ok(a) {
            return Err(RouteErr::Lost);
        }
        if self.admits(a, b).is_err() || self.admits(b, a).is_err() {
            return Err(RouteErr::Refused);
        }
        for _ in 0..2 {
            self.xfer(a, b);
            self.xfer(b, a);
        }
        Ok(())
    }
}
