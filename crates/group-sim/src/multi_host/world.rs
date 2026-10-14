use super::*;

impl World {
    pub(super) fn new(cfg: Cfg) -> Self {
        let mut rng = SimRng(cfg.seed);
        // olga alice bob carol hana hugo ivan
        let ids: Vec<Identity> = (30u8..37).map(ident).collect();
        let owners = [0usize, 1, 1, 2, 3, 4, 4, 5, 6];
        let g = Genesis::create(&ids[0], [7; 32]);
        let gid = g.id();
        let admin = [Cap::Read, Cap::Post, Cap::Delegate, Cap::Admin, Cap::Host];
        let mut grants = BTreeMap::new();
        for (k, caps, delegable, nonce) in [
            (1usize, &admin[..], true, 1u64), // alice: admin, appointed first
            (2, &admin[..], true, 20),        // bob: admin, appointed after the hosts
            (3, &[Cap::Read, Cap::Post][..], false, 2),
            (4, &[Cap::Read, Cap::Post][..], false, 3),
        ] {
            let gr = MGrant::issue(
                &ids[0],
                &gid,
                ids[k].did(),
                None,
                caps,
                delegable,
                nonce,
                None,
            );
            grants.insert(ids[k].did().to_string(), gr);
        }
        let roster: BTreeSet<String> = ids[..5].iter().map(|i| i.did().to_string()).collect();
        let mut d = vec![];
        for &k in &owners {
            let id = ids[k].clone();
            let writer = rng.writer();
            let slot = rng.next_u64();
            let group_key = SigningKey::from_bytes(&rng.bytes32());
            let private_key = SigningKey::from_bytes(&rng.bytes32());
            let my_note = EpNote::sign(
                &id,
                Audience::Group(gid.clone()),
                slot,
                &ep("grp", &group_key),
                1,
            );
            d.push(Device {
                w: Writer::new(id.clone(), writer),
                id,
                slot,
                board: Board::new(gid.clone(), roster.clone()),
                view: View::default(),
                group_key,
                my_note,
                private_key,
                retained: vec![],
                acks: BTreeMap::new(),
                outbox: vec![],
                sent: vec![],
                refused: vec![],
                op_out: vec![],
                online: true,
                host: None,
                private_book: BTreeMap::new(),
                blocked: BTreeMap::new(),
            });
        }
        let mut w = World {
            cfg,
            rng,
            now: 0,
            g,
            gid,
            d,
            grants,
            hosts: vec![],
            rogue: BTreeSet::new(),
            live: BTreeMap::new(),
            cut: BTreeSet::new(),
            frames: vec![],
        };
        for i in 0..w.d.len() {
            let (ge, pe) = (w.gep(i), w.pep(i));
            w.live.insert(ge, i);
            w.live.insert(pe, i);
        }
        // Fixture: accepted contacts already hold each other's private endpoint (first device).
        for i in MEMBERS {
            for j in MEMBERS {
                let dj = w.did(j);
                if dj != w.did(i) && !w.d[i].private_book.contains_key(&dj) {
                    let e = w.pep(j);
                    w.d[i].private_book.insert(dj, e);
                }
            }
        }
        for h in w.cfg.hosts.clone() {
            let permit = w.default_permit(h);
            w.make_host(h, permit);
        }
        // Join bootstrap (invite/ticket) is out of scope: every member and host accepts every
        // host record; members hand their group note to every host.
        for h in w.hosts.clone() {
            w.publish_record(h).unwrap();
            for m in MEMBERS {
                let n = w.d[m].my_note.wire();
                let hep = w.gep(h);
                w.send(m, h, hep, "join-note", n.clone());
                w.host_accept_note(h, &n).unwrap();
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
    pub(super) fn pep(&self, i: usize) -> String {
        ep("prv", &self.d[i].private_key)
    }
    pub(super) fn send(
        &mut self,
        from: usize,
        to: usize,
        to_ep: String,
        kind: &'static str,
        b: Vec<u8>,
    ) {
        self.frames.push(Frame {
            from,
            to,
            to_ep,
            kind,
            bytes: b,
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
            d.board.snapshot(),
            &d.view,
            &d.my_note,
            &d.acks,
            &d.retained,
            &d.private_book,
            &d.outbox,
            refused,
            &d.host,
            &d.blocked,
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
    // ---- host appointment ----
    pub(super) fn default_permit(&self, h: usize) -> MGrant {
        let (issuer, nonce) = match h {
            H1 => (A1, 10),   // alice appoints hana's desktop
            H2 => (OLGA, 11), // owner appoints hugo's host binary
            _ => (A1, 12),
        };
        self.permit(issuer, h, self.d[h].slot, nonce)
    }
    pub(super) fn permit(&self, issuer: usize, h: usize, device: u64, nonce: u64) -> MGrant {
        let parent = self.grants.get(&self.did(issuer)).cloned();
        MGrant::issue(
            &self.d[issuer].id,
            &self.gid,
            &self.did(h),
            Some(device),
            &[Cap::Host],
            false,
            nonce,
            parent,
        )
    }
    pub(super) fn make_host(&mut self, h: usize, permit: MGrant) {
        let record = HostRecord {
            note: self.d[h].my_note.clone(),
            permit,
        };
        self.d[h].host = Some(HostState {
            record,
            directory: BTreeMap::new(),
            held: BTreeMap::new(),
            claims: BTreeMap::new(),
            known_by: BTreeMap::new(),
            settled: BTreeMap::new(),
            refusals: BTreeMap::new(),
        });
        if !self.hosts.contains(&h) {
            self.hosts.push(h);
        }
    }
    // Fixture distribution of a new host record to every member and host (in practice it
    // travels via existing hosts; the members verify it either way).
    pub(super) fn publish_record(&mut self, h: usize) -> Result<(), Refuse> {
        let rec = self.d[h].host.as_ref().unwrap().record.wire();
        let mut targets: Vec<usize> = MEMBERS.to_vec();
        targets.extend(self.hosts.iter().copied());
        for i in targets {
            if i != h {
                let e = self.gep(i);
                self.send(h, i, e, "host-record", rec.clone());
            }
            match self.accept_host_record(i, &rec) {
                Ok(()) | Err(Refuse::Stale) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    pub(super) fn check_record(&self, view: &View, r: &HostRecord) -> Result<(), Refuse> {
        if !r.note.verify() {
            return Err(Refuse::BadSignature);
        }
        if r.note.claims.audience != Audience::Group(self.gid.clone()) {
            return Err(Refuse::WrongAudience);
        }
        if !r.permit.allows(&self.g, &r.note.claims.owner, Cap::Host) {
            return Err(Refuse::PermitInvalid);
        }
        if r.permit.body.device != Some(r.note.claims.slot) {
            return Err(Refuse::DeviceMismatch);
        }
        if view.revoked.contains(&r.permit.digest()) {
            return Err(Refuse::Revoked);
        }
        Ok(())
    }
    pub(super) fn accept_host_record(&mut self, i: usize, bytes: &[u8]) -> Result<(), Refuse> {
        let Ok(r) = serde_json::from_slice::<HostRecord>(bytes) else {
            return Err(Refuse::Malformed);
        };
        self.check_record(&self.d[i].view, &r)?;
        let key = dir_key(&r.note.claims.owner, r.note.claims.slot);
        let view = &self.d[i].view;
        if let Some(cur) = view.hosts.get(&key) {
            if cur.note.claims.seq >= r.note.claims.seq {
                return Err(Refuse::Stale);
            }
        } else if let HostCap::Max(n) = self.cfg.cap {
            if view.hosts.len() >= n {
                return Err(Refuse::CapReached);
            }
        }
        self.d[i].view.hosts.insert(key, r);
        Ok(())
    }
    // `x` admits a link from host `y`: y presents its current record; x must accept it.
    pub(super) fn admits(&mut self, x: usize, y: usize) -> Result<(), Refuse> {
        let Some(hs) = self.d[y].host.as_ref() else {
            return Err(Refuse::NotHost);
        };
        let r = hs.record.clone();
        match self.accept_host_record(x, &r.wire()) {
            Ok(()) | Err(Refuse::Stale) => {}
            Err(e) => return Err(e),
        }
        if self.d[x].view.hosts.get(&self.hk(y)) != Some(&r) {
            return Err(Refuse::Stale);
        }
        Ok(())
    }
    pub(super) fn host_accept_note(&mut self, h: usize, bytes: &[u8]) -> Result<(), Refuse> {
        let Ok(n) = serde_json::from_slice::<EpNote>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !n.verify() {
            return Err(Refuse::BadSignature);
        }
        if n.claims.audience != Audience::Group(self.gid.clone()) {
            return Err(Refuse::WrongAudience);
        }
        if !self.d[h].board.roster.contains(&n.claims.owner) {
            return Err(Refuse::NotMember);
        }
        let key = dir_key(&n.claims.owner, n.claims.slot);
        let Some(hs) = self.d[h].host.as_mut() else {
            return Err(Refuse::NotHost);
        };
        if let Some(cur) = hs.directory.get(&key) {
            if cur.claims.seq >= n.claims.seq {
                return Err(Refuse::Stale);
            }
        }
        hs.directory.insert(key, n);
        Ok(())
    }
    // Every device verifies every op itself; all checks run before any mutation.
    pub(super) fn accept_op(&mut self, i: usize, bytes: &[u8]) -> Result<bool, Refuse> {
        let Ok(op) = serde_json::from_slice::<SignedOp>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !op.verify() {
            return Err(Refuse::BadSignature);
        }
        if op.body.group != self.gid {
            return Err(Refuse::WrongGroup);
        }
        let id = op.id();
        if self.d[i].view.ops.contains_key(&id) {
            return Ok(false);
        }
        let issuer = op.body.issuer.clone();
        let is_admin = issuer == self.g.body.creator
            || op
                .body
                .issuer_grant
                .as_ref()
                .is_some_and(|gr| gr.allows(&self.g, &issuer, Cap::Admin));
        if !is_admin {
            return Err(Refuse::NotAdmin);
        }
        let honest = !self.rogue.contains(&i);
        match op.body.op.clone() {
            Op::Admit { grant } => {
                let holder = grant.body.holder.clone();
                if !grant.allows(&self.g, &holder, Cap::Post) {
                    return Err(Refuse::PermitInvalid);
                }
                self.d[i].board.roster.insert(holder);
            }
            Op::Flag { target } => {
                let dv = &mut self.d[i];
                if !dv.board.roster.contains(&target) && !dv.view.removed.contains(&target) {
                    return Err(Refuse::NotMember);
                }
                let n = {
                    let fl = dv.view.flags.entry(target.clone()).or_default();
                    fl.insert(issuer.clone());
                    fl.len()
                };
                if n >= REMOVAL_FLAGS && dv.view.removed.insert(target.clone()) && honest {
                    dv.board.roster.remove(&target);
                    if let Some(hs) = dv.host.as_mut() {
                        hs.directory.retain(|_, n| n.claims.owner != target);
                    }
                }
            }
            Op::RemoveHost { permit } => {
                let Some((key, rec)) = self.d[i]
                    .view
                    .hosts
                    .iter()
                    .find(|(_, r)| r.permit.digest() == permit)
                    .map(|(k, r)| (k.clone(), r.clone()))
                else {
                    return Err(Refuse::UnknownHost);
                };
                if self.cfg.removal == RemovalRule::AdminAppointedAfterHost {
                    let appointed = op.body.issuer_grant.as_ref().map_or(0, |g| g.body.nonce);
                    if appointed <= rec.permit.body.nonce {
                        return Err(Refuse::NotAppointedAfter);
                    }
                }
                self.d[i].view.hosts.remove(&key);
                self.d[i].view.revoked.insert(permit);
            }
        }
        self.d[i].view.ops.insert(id, op);
        Ok(true)
    }
    pub(super) fn op(&mut self, i: usize, op: Op) -> SignedOp {
        let grant = self.grants.get(&self.did(i)).cloned();
        let nonce = self.rng.next_u64();
        SignedOp::sign(&self.d[i].id, &self.gid, grant, nonce, op)
    }
    pub(super) fn submit(&mut self, i: usize, op: SignedOp) {
        let _ = self.accept_op(i, &op.wire());
        self.d[i].op_out.push(op);
    }
    // ---- content ----
    pub(super) fn post(&mut self, m: usize, text: &str) -> Post {
        let dv = &mut self.d[m];
        dv.w.observe(&dv.board);
        let p = dv.w.post(&self.gid, text);
        dv.outbox.push(p.clone());
        p
    }
    pub(super) fn host_receive(&mut self, h: usize, p: Post) -> Verdict {
        let id = p.id();
        let v = match self.cfg.claim {
            ClaimRule::FirstAtHost => self.d[h].board.receive(p),
            ClaimRule::LowestAfterAllHostsSaw => {
                let hs = self.d[h].host.as_ref().unwrap();
                match hs.settled.get(&p.body.writer) {
                    Some(win) if hs.claims[&p.body.writer][win] != p.body.author => {
                        Verdict::Rejected(Why::WriterOwnedByOtherDid)
                    }
                    Some(_) => self.d[h].board.receive(p),
                    None => match self.d[h].board.precheck(&p) {
                        Err(why) => Verdict::Rejected(why),
                        Ok(()) => {
                            let hs = self.d[h].host.as_mut().unwrap();
                            if p.body.seq == 1 {
                                hs.claims
                                    .entry(p.body.writer)
                                    .or_default()
                                    .insert(id.clone(), p.body.author.clone());
                                hs.known_by.entry(id.clone()).or_default().insert(h);
                            }
                            hs.held.insert(id.clone(), p);
                            Verdict::Pending
                        }
                    },
                }
            }
        };
        if let Verdict::Rejected(why) = v {
            self.d[h].host.as_mut().unwrap().refusals.insert(id, why);
        }
        v
    }
    pub(super) fn settle(&mut self, h: usize) {
        if self.cfg.claim != ClaimRule::LowestAfterAllHostsSaw {
            return;
        }
        let all: BTreeSet<usize> = self.hosts.iter().copied().collect();
        let hs = self.d[h].host.as_mut().unwrap();
        for (writer, claims) in hs.claims.clone() {
            if hs.settled.contains_key(&writer) {
                continue;
            }
            let seen_everywhere = claims
                .keys()
                .all(|c| hs.known_by.get(c).is_some_and(|s| s.is_superset(&all)));
            if seen_everywhere {
                let win = claims.keys().next().unwrap().clone(); // lowest claim id
                hs.settled.insert(writer, win);
            }
        }
        let mut held: Vec<Post> = hs.held.values().cloned().collect();
        held.sort_by_key(|p| p.body.seq);
        for p in held {
            let hs = self.d[h].host.as_ref().unwrap();
            let Some(win) = hs.settled.get(&p.body.writer) else {
                continue;
            };
            let owner = hs.claims[&p.body.writer][win].clone();
            let id = p.id();
            self.d[h].host.as_mut().unwrap().held.remove(&id);
            let v = if owner == p.body.author {
                self.d[h].board.receive(p)
            } else {
                Verdict::Rejected(Why::WriterOwnedByOtherDid)
            };
            if let Verdict::Rejected(why) = v {
                self.d[h].host.as_mut().unwrap().refusals.insert(id, why);
            }
        }
    }
}
