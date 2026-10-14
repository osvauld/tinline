use super::*;

impl World {
    pub(super) fn new(cfg: Cfg) -> Self {
        let mut rng = SimRng(cfg.seed);
        // hana = host, alice (two devices, one mnemonic), bob, carol.
        let ids = [ident(10), ident(11), ident(12), ident(13)];
        let owners = [0usize, 1, 1, 2, 3];
        let roster: BTreeSet<String> = ids.iter().map(|i| i.did().to_string()).collect();
        let req: Vec<[u8; 32]> = ids.iter().map(|_| rng.bytes32()).collect();
        let host_did = ids[0].did().to_string();
        let mut d = vec![];
        for &k in &owners {
            let id = ids[k].clone();
            let writer = rng.writer();
            let slot = rng.next_u64();
            let group_key = SigningKey::from_bytes(&rng.bytes32());
            let private_key = SigningKey::from_bytes(&rng.bytes32());
            let own = id.did().to_string();
            d.push(Device {
                w: Writer::new(id.clone(), writer),
                id,
                slot,
                board: Board::new(roster.clone(), cfg.fork),
                group_key,
                group_seq: 1,
                private_key,
                private_seq: 0,
                req_secret: req[k],
                host_did: host_did.clone(),
                host_note: None,
                req_keys: BTreeMap::new(),
                private_book: BTreeMap::new(),
                unsent_note: None,
                outbox: vec![],
                refused: vec![],
                online: true,
                contacts: roster.iter().filter(|x| **x != own).cloned().collect(),
                blocked: BTreeSet::new(),
                incoming: BTreeMap::new(),
                outgoing: BTreeMap::new(),
                seen: BTreeSet::new(),
            });
        }
        let mut w = World {
            cfg,
            rng,
            now: 0,
            d,
            directory: BTreeMap::new(),
            live: BTreeMap::new(),
            retiring: BTreeMap::new(),
            frames: vec![],
            keys_ever: vec![],
        };
        for i in 0..w.d.len() {
            let (g, p) = (w.d[i].group_ep(), w.d[i].private_ep());
            w.live.insert(g, i);
            w.live.insert(p, i);
            let (gk, pk) = (tkey(&w.d[i].group_key), tkey(&w.d[i].private_key));
            w.keys_ever.push((i, KeyKind::Group, gk));
            w.keys_ever.push((i, KeyKind::Private, pk));
        }
        // Join bootstrap (invite/ticket) is out of scope. Host signs its own note; members
        // hand their group note to the host only; host hands its note to each member.
        let h = &w.d[HOST];
        let hn = EpNote::sign(
            &h.id,
            Audience::Group(GROUP.into()),
            h.slot,
            &h.group_ep(),
            1,
        );
        w.d[HOST].host_note = Some(hn.clone());
        let host_ep = w.d[HOST].group_ep();
        for i in 1..w.d.len() {
            let dv = &w.d[i];
            let n = EpNote::sign(
                &dv.id,
                Audience::Group(GROUP.into()),
                dv.slot,
                &dv.group_ep(),
                1,
            );
            w.send(i, HOST, host_ep.clone(), "join-note", n.wire());
            w.host_accept_note(&n.wire()).unwrap();
            let mep = w.d[i].group_ep();
            w.send(HOST, i, mep, "host-note", hn.wire());
            w.member_accept_host_note(i, &hn.wire()).unwrap();
        }
        // Request-key claims (DID-signed, per DID) relayed by the host to every member.
        let mut claims = vec![];
        for k in [0usize, 1, 3, 4] {
            let dv = &w.d[k];
            let claim = ReqKeyClaim {
                group: GROUP.into(),
                owner: dv.did(),
                x25519: PublicKey::from(&StaticSecret::from(dv.req_secret)).to_bytes(),
            };
            let sig = did_sign(&dv.id, "req-key", &claim);
            claims.push(SignedReqKey { claim, sig });
        }
        for i in 0..w.d.len() {
            for c in &claims {
                if i != HOST {
                    let mep = w.d[i].group_ep();
                    w.send(HOST, i, mep, "req-key", serde_json::to_vec(c).unwrap());
                }
                assert!(did_verify(&c.claim.owner, "req-key", &c.claim, &c.sig));
                w.d[i].req_keys.insert(c.claim.owner.clone(), c.clone());
            }
        }
        w
    }
    pub(super) fn did(&self, i: usize) -> String {
        self.d[i].did()
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
    pub(super) fn listening(&self, ep: &str) -> Option<usize> {
        if let Some(&i) = self.live.get(ep) {
            return Some(i);
        }
        match self.retiring.get(ep) {
            Some(&t) if self.now < t => Some(HOST),
            _ => None,
        }
    }
    pub(super) fn tick(&mut self, n: u64) {
        self.now += n;
    }
    // Host side: accept a member device's DID-signed group note into the directory.
    pub(super) fn host_accept_note(&mut self, bytes: &[u8]) -> Result<(), NoteReject> {
        let Ok(n) = serde_json::from_slice::<EpNote>(bytes) else {
            return Err(NoteReject::Malformed);
        };
        if !n.verify() {
            return Err(NoteReject::BadSignature);
        }
        if n.claims.audience != Audience::Group(GROUP.into()) {
            return Err(NoteReject::WrongAudience);
        }
        if !self.d[HOST].board.roster.contains(&n.claims.owner) {
            return Err(NoteReject::NotMember);
        }
        let key = dir_key(&n.claims.owner, n.claims.slot);
        if let Some(cur) = self.directory.get(&key) {
            if cur.claims.seq >= n.claims.seq {
                return Err(NoteReject::Stale);
            }
        }
        self.directory.insert(key, n);
        Ok(())
    }
    pub(super) fn member_accept_host_note(
        &mut self,
        i: usize,
        bytes: &[u8],
    ) -> Result<(), NoteReject> {
        let Ok(n) = serde_json::from_slice::<EpNote>(bytes) else {
            return Err(NoteReject::Malformed);
        };
        if !n.verify() {
            return Err(NoteReject::BadSignature);
        }
        if n.claims.owner != self.d[i].host_did {
            return Err(NoteReject::WrongOwner);
        }
        if n.claims.audience != Audience::Group(GROUP.into()) {
            return Err(NoteReject::WrongAudience);
        }
        if let Some(cur) = &self.d[i].host_note {
            if cur.claims.seq >= n.claims.seq {
                return Err(NoteReject::Stale);
            }
        }
        self.d[i].host_note = Some(n);
        Ok(())
    }
    pub(super) fn host_snapshot(&self) -> Vec<u8> {
        [
            self.d[HOST].board.snapshot(),
            serde_json::to_vec(&self.directory).unwrap(),
        ]
        .concat()
    }
    // Member dials the host using the only group address it has.
    pub(super) fn reach_host(&self, i: usize) -> Result<String, RouteErr> {
        if !self.d[i].online {
            return Err(RouteErr::SelfOffline);
        }
        let Some(n) = &self.d[i].host_note else {
            return Err(RouteErr::NoAddress);
        };
        let ep = n.claims.endpoint.clone();
        match self.listening(&ep) {
            Some(HOST) if self.d[HOST].online => Ok(ep),
            Some(HOST) => Err(RouteErr::PeerOffline),
            _ => Err(RouteErr::EndpointGone),
        }
    }
    // Host dials a member device via its directory entry.
    pub(super) fn host_reach(&self, i: usize) -> Result<String, RouteErr> {
        if !self.d[HOST].online {
            return Err(RouteErr::SelfOffline);
        }
        let Some(n) = self.directory.get(&dir_key(&self.did(i), self.d[i].slot)) else {
            return Err(RouteErr::NoAddress);
        };
        let ep = n.claims.endpoint.clone();
        match self.live.get(&ep) {
            Some(&j) if j == i && self.d[i].online => Ok(ep),
            Some(&j) if j == i => Err(RouteErr::PeerOffline),
            _ => Err(RouteErr::EndpointGone),
        }
    }
    // Any member's attempt to reach another DID: only addresses it actually holds.
    pub(super) fn dial(&self, i: usize, to: &str) -> Result<String, RouteErr> {
        if !self.d[i].online {
            return Err(RouteErr::SelfOffline);
        }
        let ep = if to == self.d[i].host_did {
            match &self.d[i].host_note {
                Some(n) => n.claims.endpoint.clone(),
                None => return Err(RouteErr::NoAddress),
            }
        } else if let Some(e) = self.d[i].private_book.get(to) {
            e.clone()
        } else {
            return Err(RouteErr::NoAddress);
        };
        match self.listening(&ep) {
            Some(j) if self.did(j) == to && self.d[j].online => Ok(ep),
            Some(j) if self.did(j) == to => Err(RouteErr::PeerOffline),
            _ => Err(RouteErr::EndpointGone),
        }
    }
    // Local write. Members queue for the host; the host applies to its own board directly.
    pub(super) fn post(&mut self, i: usize, text: &str) -> Option<Post> {
        if let QueueBound::Refuse(n) = self.cfg.queue {
            if i != HOST && self.d[i].outbox.len() >= n {
                return None;
            }
        }
        let dv = &mut self.d[i];
        dv.w.observe(&dv.board);
        let p = dv.w.post(GROUP, text);
        if i == HOST {
            let v = self.d[HOST].board.receive(p.clone());
            assert_eq!(v, Verdict::Accepted);
            self.host_push_all();
        } else {
            self.d[i].outbox.push(p.clone());
        }
        Some(p)
    }
    // Host pushes (on an established link) its newer note and every post the device lacks.
    pub(super) fn push_to(&mut self, j: usize, to_ep: String) {
        let hn = self.d[HOST].host_note.clone().unwrap();
        if self.d[j].host_note.as_ref().map(|n| n.claims.seq) < Some(hn.claims.seq) {
            self.send(HOST, j, to_ep.clone(), "host-note", hn.wire());
            self.member_accept_host_note(j, &hn.wire()).unwrap();
        }
        let missing: Vec<Post> = self.d[HOST]
            .board
            .order
            .iter()
            .filter(|id| !self.d[j].board.accepted.contains_key(*id))
            .map(|id| self.d[HOST].board.accepted[id].clone())
            .collect();
        for p in missing {
            self.send(HOST, j, to_ep.clone(), "post", p.wire());
            self.d[j].board.receive(p);
        }
        let dv = &mut self.d[j];
        dv.w.observe(&dv.board);
    }
    pub(super) fn host_push_all(&mut self) {
        for j in 1..self.d.len() {
            if let Ok(ep) = self.host_reach(j) {
                self.push_to(j, ep);
            }
        }
        let h = &mut self.d[HOST];
        h.w.observe(&h.board);
    }
    // Member -> host half of an established link: pending own note, then queued posts.
    pub(super) fn upload(&mut self, i: usize, host_ep: String) -> Vec<Verdict> {
        if let Some(b) = self.d[i].unsent_note.take() {
            self.send(i, HOST, host_ep.clone(), "ep-note", b.clone());
            let _ = self.host_accept_note(&b);
        }
        let mut out = vec![];
        for p in std::mem::take(&mut self.d[i].outbox) {
            self.send(i, HOST, host_ep.clone(), "post", p.wire());
            let v = self.d[HOST].board.receive(p.clone());
            if let Verdict::Rejected(why) = v {
                self.d[i].refused.push((p, why));
            }
            out.push(v);
        }
        out
    }
    // "The side whose address is still live retries" (U1): member dials host, else host
    // dials member. Exchanges queued state, then the host fans out to everyone it can reach.
    pub(super) fn reconnect(&mut self, i: usize) -> Result<Path, RouteErr> {
        if !self.d[i].online {
            return Err(RouteErr::SelfOffline);
        }
        if !self.d[HOST].online {
            return Err(RouteErr::PeerOffline);
        }
        let path = match self.reach_host(i) {
            Ok(hep) => {
                let path = if self.live.get(&hep) == Some(&HOST) {
                    Path::MemberDialedHost
                } else {
                    Path::MemberDialedRetiringHostEndpoint
                };
                self.upload(i, hep);
                let mep = self.d[i].group_ep();
                self.push_to(i, mep);
                path
            }
            Err(_) => match self.host_reach(i) {
                Ok(mep) => {
                    self.push_to(i, mep); // carries the new host note
                    let hep = self.d[HOST].group_ep();
                    self.upload(i, hep);
                    let mep = self.d[i].group_ep();
                    self.push_to(i, mep);
                    Path::HostDialedMember
                }
                Err(_) => return Err(RouteErr::Lost),
            },
        };
        self.host_push_all();
        Ok(path)
    }
    // Periodic member group-endpoint rotation (never triggered by a block). Note goes to the
    // host only; if unreachable it waits for the next link. Old endpoint stops listening.
    pub(super) fn rotate_member_group(&mut self, i: usize) -> Vec<u8> {
        assert_ne!(i, HOST);
        let old = self.d[i].group_ep();
        self.live.remove(&old);
        self.d[i].group_key = SigningKey::from_bytes(&self.rng.bytes32());
        self.d[i].group_seq += 1;
        let dv = &self.d[i];
        let new = dv.group_ep();
        let n = EpNote::sign(
            &dv.id,
            Audience::Group(GROUP.into()),
            dv.slot,
            &new,
            dv.group_seq,
        );
        let k = tkey(&self.d[i].group_key);
        self.keys_ever.push((i, KeyKind::Group, k));
        self.live.insert(new, i);
        let bytes = n.wire();
        match self.reach_host(i) {
            Ok(hep) => {
                self.send(i, HOST, hep, "ep-note", bytes.clone());
                self.host_accept_note(&bytes).unwrap();
            }
            Err(_) => self.d[i].unsent_note = Some(bytes.clone()),
        }
        bytes
    }
    // Host rotation: pushed directly to every member device the host can reach.
    pub(super) fn rotate_host(&mut self) -> Vec<u8> {
        let old = self.d[HOST].group_ep();
        self.live.remove(&old);
        if let HostOverlap::Ticks(t) = self.cfg.overlap {
            self.retiring.insert(old, self.now + t);
        }
        self.d[HOST].group_key = SigningKey::from_bytes(&self.rng.bytes32());
        self.d[HOST].group_seq += 1;
        let h = &self.d[HOST];
        let new = h.group_ep();
        let n = EpNote::sign(
            &h.id,
            Audience::Group(GROUP.into()),
            h.slot,
            &new,
            h.group_seq,
        );
        let k = tkey(&self.d[HOST].group_key);
        self.keys_ever.push((HOST, KeyKind::Group, k));
        self.live.insert(new, HOST);
        self.d[HOST].host_note = Some(n.clone());
        for j in 1..self.d.len() {
            if let Ok(mep) = self.host_reach(j) {
                self.send(HOST, j, mep, "host-note", n.wire());
                self.member_accept_host_note(j, &n.wire()).unwrap();
            }
        }
        n.wire()
    }
    // Private-audience note (initial announce or periodic rotation): sent directly to
    // unblocked contacts' devices, never via the host (U1/U5). Siblings are out of scope.
    pub(super) fn announce_private(&mut self, i: usize) -> Vec<usize> {
        if self.d[i].private_seq > 0 {
            let old = self.d[i].private_ep();
            self.live.remove(&old);
            self.d[i].private_key = SigningKey::from_bytes(&self.rng.bytes32());
            let k = tkey(&self.d[i].private_key);
            self.keys_ever.push((i, KeyKind::Private, k));
            let new = self.d[i].private_ep();
            self.live.insert(new, i);
        }
        self.d[i].private_seq += 1;
        let dv = &self.d[i];
        let n = EpNote::sign(
            &dv.id,
            Audience::Private,
            dv.slot,
            &dv.private_ep(),
            dv.private_seq,
        );
        let recipients: Vec<usize> = (0..self.d.len())
            .filter(|&j| {
                let t = self.did(j);
                t != dv.did() && dv.contacts.contains(&t) && !dv.blocked.contains(&t)
            })
            .collect();
        let owner = self.did(i);
        for &j in &recipients {
            let to_ep = self.d[j].private_ep();
            self.send(i, j, to_ep, "private-note", n.wire());
            assert!(n.verify());
            self.d[j]
                .private_book
                .insert(owner.clone(), n.claims.endpoint.clone());
        }
        recipients
    }
    // Every byte device `i` received on any link plus its validated replica state.
    pub(super) fn received(&self, i: usize) -> Vec<u8> {
        let mut hay: Vec<u8> = self
            .frames
            .iter()
            .filter(|f| f.to == i)
            .flat_map(|f| f.bytes.clone())
            .collect();
        hay.extend(self.d[i].board.snapshot());
        hay.extend(serde_json::to_vec(&self.d[i].host_note).unwrap());
        hay
    }
    // Redraw a writer ID (collision recovery candidate): fresh Loro writer from the RNG,
    // rebuilt from the validated board; refused + queued texts are re-posted under it.
    pub(super) fn redraw_writer(&mut self, i: usize) -> u64 {
        let new = self.rng.writer();
        let id = self.d[i].id.clone();
        self.d[i].w = Writer::new(id, new);
        let mut texts: Vec<String> = self.d[i]
            .refused
            .drain(..)
            .map(|(p, _)| p.body.text)
            .collect();
        texts.extend(self.d[i].outbox.drain(..).map(|p| p.body.text));
        for t in texts {
            self.post(i, &t);
        }
        new
    }
    // ---- chat request via host ----
    pub(super) fn chat_request(&mut self, i: usize, to: &str) -> Option<String> {
        let claim = self.d[i].req_keys.get(to)?.clone();
        let hep = self.reach_host(i).ok()?;
        let request_id = hex32(&self.rng.bytes32()[..8]);
        let inner = Inner::Request {
            group: GROUP.into(),
            from: self.did(i),
            to: to.into(),
            request_id: request_id.clone(),
            sent_at: self.now,
        };
        let signed = SignedInner::new(&self.d[i].id, inner);
        let header = Header {
            group: GROUP.into(),
            kind: ChatKind::Request,
            from: self.did(i),
            to: to.into(),
            ephemeral: [0; 32],
        };
        let eph = self.rng.bytes32();
        let sealed = seal(header, &claim.claim.x25519, eph, &signed);
        self.d[i]
            .outgoing
            .insert(request_id.clone(), (to.into(), signed.digest()));
        let bytes = serde_json::to_vec(&sealed).unwrap();
        self.send(i, HOST, hep, "chat", bytes.clone());
        self.host_route_chat(&bytes);
        Some(request_id)
    }
    // Host sees only the header; delivers to every reachable device of the target DID.
    pub(super) fn host_route_chat(&mut self, bytes: &[u8]) -> Vec<(usize, ChatIn)> {
        let Ok(w) = serde_json::from_slice::<Sealed>(bytes) else {
            return vec![];
        };
        let targets: Vec<usize> = self
            .directory
            .values()
            .filter(|n| n.claims.owner == w.header.to)
            .filter_map(|n| self.live.get(&n.claims.endpoint).copied())
            .collect();
        let mut out = vec![];
        for j in targets {
            if let Ok(mep) = self.host_reach(j) {
                self.send(HOST, j, mep, "chat", bytes.to_vec());
                out.push((j, self.chat_inbox(j, bytes)));
            }
        }
        out
    }
}
