use super::*;

impl World {
    // ---- links ----
    // Member/host link: member dials the host it knows, else the host dials the member's
    // directory endpoint (U1 "side whose address is still live"). Then both admit and sync.
    pub(super) fn connect(&mut self, m: usize, h: usize) -> Result<Path, RouteErr> {
        if !self.d[m].online {
            return Err(RouteErr::SelfOffline);
        }
        if !self.d[h].online {
            return Err(RouteErr::PeerOffline);
        }
        let hkey = self.hk(h);
        let member_side = self.d[m]
            .view
            .hosts
            .get(&hkey)
            .map(|r| r.note.claims.endpoint.clone())
            .and_then(|e| self.dial(m, &e).ok())
            == Some(h);
        let removed = !self.rogue.contains(&h) && self.d[h].view.removed.contains(&self.did(m));
        if removed {
            if member_side {
                self.removal_notice(m, h);
                return Err(RouteErr::Removed);
            }
            return Err(RouteErr::Lost);
        }
        let path = if member_side {
            Path::MemberDialedHost
        } else {
            let e = self.d[h]
                .host
                .as_ref()
                .and_then(|hs| hs.directory.get(&self.hk(m)))
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
    // Host-initiated only (section 9 Q6): the host dials the member's current endpoint.
    pub(super) fn connect_from_host(&mut self, h: usize, m: usize) -> Result<Path, RouteErr> {
        let Some(e) = self.d[h]
            .host
            .as_ref()
            .and_then(|hs| hs.directory.get(&self.hk(m)))
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
        Ok(Path::HostDialedMember)
    }
    // Honest host answering a removed member: the signed flags are the notice (U5 revised).
    pub(super) fn removal_notice(&mut self, m: usize, h: usize) {
        let me = self.did(m);
        let ops: Vec<SignedOp> = self.d[h]
            .view
            .ops
            .values()
            .filter(|o| matches!(&o.body.op, Op::Flag { target } if *target == me))
            .cloned()
            .collect();
        let e = self.gep(m);
        for o in ops {
            self.send(h, m, e.clone(), "removal-notice", o.wire());
            let _ = self.accept_op(m, &o.wire());
        }
    }
    pub(super) fn exchange(&mut self, m: usize, h: usize) {
        let (hep, mep) = (self.gep(h), self.gep(m));
        let (hkey, mkey) = (self.hk(h), self.hk(m));
        // member -> host
        let note = self.d[m].my_note.clone();
        if self.d[m].acks.get(&hkey).copied().unwrap_or(0) < note.claims.seq {
            self.send(m, h, hep.clone(), "ep-note", note.wire());
            let _ = self.host_accept_note(h, &note.wire());
        }
        for o in std::mem::take(&mut self.d[m].op_out) {
            self.send(m, h, hep.clone(), "op", o.wire());
            let _ = self.accept_op(h, &o.wire());
        }
        for p in std::mem::take(&mut self.d[m].outbox) {
            self.send(m, h, hep.clone(), "post", p.wire());
            match self.host_receive(h, p.clone()) {
                Verdict::Rejected(why) => self.d[m].refused.push((p, why)),
                _ => self.d[m].sent.push(p),
            }
        }
        // host -> member
        let recs: Vec<HostRecord> = self.d[h].view.hosts.values().cloned().collect();
        for r in recs {
            let k = dir_key(&r.note.claims.owner, r.note.claims.slot);
            if self.d[m].view.hosts.get(&k) != Some(&r) {
                self.send(h, m, mep.clone(), "host-record", r.wire());
                let _ = self.accept_host_record(m, &r.wire());
            }
        }
        let ops: Vec<SignedOp> = self.d[h]
            .view
            .ops
            .iter()
            .filter(|(id, _)| !self.d[m].view.ops.contains_key(*id))
            .map(|(_, o)| o.clone())
            .collect();
        for o in ops {
            self.send(h, m, mep.clone(), "op", o.wire());
            let _ = self.accept_op(m, &o.wire());
        }
        let missing: Vec<Post> = self.d[h]
            .board
            .order
            .iter()
            .filter(|id| !self.d[m].board.accepted.contains_key(*id))
            .map(|id| self.d[h].board.accepted[id].clone())
            .collect();
        for p in missing {
            self.send(h, m, mep.clone(), "post", p.wire());
            self.d[m].board.receive(p);
        }
        let hs = self.d[h].host.as_ref().unwrap();
        let acked = hs.directory.get(&mkey).map(|n| n.claims.seq);
        let refusals = hs.refusals.clone();
        if let Some(s) = acked {
            self.d[m].acks.insert(hkey, s);
        }
        let sent = std::mem::take(&mut self.d[m].sent);
        for p in sent {
            match refusals.get(&p.id()) {
                Some(why) => self.d[m].refused.push((p, *why)),
                None => self.d[m].sent.push(p),
            }
        }
        self.release_retained(m);
        let dv = &mut self.d[m];
        dv.w.observe(&dv.board);
    }
    pub(super) fn release_retained(&mut self, m: usize) {
        let seq = self.d[m].my_note.claims.seq;
        let dv = &self.d[m];
        let all_acked = dv
            .view
            .hosts
            .keys()
            .all(|k| dv.acks.get(k).copied().unwrap_or(0) >= seq);
        if all_acked {
            for e in std::mem::take(&mut self.d[m].retained) {
                self.live.remove(&e);
            }
        }
    }
    pub(super) fn host_sync(&mut self, a: usize, b: usize) -> Result<(), RouteErr> {
        let (ka, kb) = (self.hk(a), self.hk(b));
        let ea = self.d[b]
            .view
            .hosts
            .get(&ka)
            .map(|r| r.note.claims.endpoint.clone());
        let eb = self.d[a]
            .view
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
            self.settle(a);
            self.settle(b);
        }
        Ok(())
    }
    pub(super) fn xfer(&mut self, from: usize, to: usize) {
        let to_ep = self.gep(to);
        let recs: Vec<HostRecord> = self.d[from].view.hosts.values().cloned().collect();
        for r in recs {
            let k = dir_key(&r.note.claims.owner, r.note.claims.slot);
            if self.d[to].view.hosts.get(&k) != Some(&r) {
                self.send(from, to, to_ep.clone(), "host-record", r.wire());
                let _ = self.accept_host_record(to, &r.wire());
            }
        }
        let ops: Vec<SignedOp> = self.d[from]
            .view
            .ops
            .iter()
            .filter(|(id, _)| !self.d[to].view.ops.contains_key(*id))
            .map(|(_, o)| o.clone())
            .collect();
        for o in ops {
            self.send(from, to, to_ep.clone(), "op", o.wire());
            let _ = self.accept_op(to, &o.wire());
        }
        let notes: Vec<EpNote> = self.d[from]
            .host
            .as_ref()
            .unwrap()
            .directory
            .values()
            .cloned()
            .collect();
        for n in notes {
            let k = dir_key(&n.claims.owner, n.claims.slot);
            let cur = self.d[to]
                .host
                .as_ref()
                .unwrap()
                .directory
                .get(&k)
                .map(|c| c.claims.seq);
            if cur < Some(n.claims.seq) {
                self.send(from, to, to_ep.clone(), "dir-note", n.wire());
                let _ = self.host_accept_note(to, &n.wire());
            }
        }
        let fb = &self.d[from];
        let mut posts: Vec<Post> = fb
            .board
            .order
            .iter()
            .map(|id| fb.board.accepted[id].clone())
            .collect();
        posts.extend(fb.host.as_ref().unwrap().held.values().cloned());
        for p in posts {
            let id = p.id();
            let t = &self.d[to];
            let ths = t.host.as_ref().unwrap();
            if t.board.accepted.contains_key(&id)
                || t.board.pending.contains_key(&id)
                || ths.held.contains_key(&id)
                || ths.refusals.contains_key(&id)
            {
                continue;
            }
            self.send(from, to, to_ep.clone(), "post", p.wire());
            self.host_receive(to, p);
        }
        if self.cfg.claim == ClaimRule::LowestAfterAllHostsSaw {
            let kb = self.d[from].host.as_ref().unwrap().known_by.clone();
            let ths = self.d[to].host.as_mut().unwrap();
            for (c, set) in kb {
                if ths.claims.values().any(|cl| cl.contains_key(&c)) {
                    let e = ths.known_by.entry(c).or_default();
                    e.extend(set);
                    e.insert(to);
                }
            }
        }
    }
    pub(super) fn round(&mut self) {
        for _ in 0..2 {
            let hs = self.hosts.clone();
            for (x, &a) in hs.iter().enumerate() {
                for &b in &hs[x + 1..] {
                    let _ = self.host_sync(a, b);
                }
            }
            for m in MEMBERS {
                for &h in &hs {
                    let _ = self.connect(m, h);
                }
            }
        }
    }
    // ---- rotation ----
    // Periodic member group-endpoint rotation; the note goes to EVERY host it can reach.
    pub(super) fn rotate_member(&mut self, m: usize) -> Vec<usize> {
        let old = self.gep(m);
        match self.cfg.retention {
            Retention::None => {
                self.live.remove(&old);
            }
            Retention::UntilHostAck => self.d[m].retained.push(old),
        }
        self.d[m].group_key = SigningKey::from_bytes(&self.rng.bytes32());
        let seq = self.d[m].my_note.claims.seq + 1;
        let new = self.gep(m);
        let note = EpNote::sign(
            &self.d[m].id,
            Audience::Group(self.gid.clone()),
            self.d[m].slot,
            &new,
            seq,
        );
        self.d[m].my_note = note.clone();
        self.live.insert(new, m);
        let mut reached = vec![];
        for h in self.hosts.clone() {
            let Some(e) = self.d[m]
                .view
                .hosts
                .get(&self.hk(h))
                .map(|r| r.note.claims.endpoint.clone())
            else {
                continue;
            };
            if self.dial(m, &e) == Ok(h) {
                self.send(m, h, e, "ep-note", note.wire());
                if self.host_accept_note(h, &note.wire()).is_ok() {
                    let k = self.hk(h);
                    self.d[m].acks.insert(k, seq);
                    reached.push(h);
                }
            }
        }
        self.release_retained(m);
        reached
    }
    // Host rotation: no overlap window; the host itself dials every member at its current
    // directory endpoint and hands over its new record, then syncs peer hosts (Q6).
    pub(super) fn rotate_host(&mut self, h: usize) -> Vec<usize> {
        let old = self.gep(h);
        self.live.remove(&old);
        self.d[h].group_key = SigningKey::from_bytes(&self.rng.bytes32());
        let seq = self.d[h].my_note.claims.seq + 1;
        let new = self.gep(h);
        let note = EpNote::sign(
            &self.d[h].id,
            Audience::Group(self.gid.clone()),
            self.d[h].slot,
            &new,
            seq,
        );
        self.d[h].my_note = note.clone();
        self.live.insert(new, h);
        self.d[h].host.as_mut().unwrap().record.note = note;
        let rec = self.d[h].host.as_ref().unwrap().record.wire();
        let _ = self.accept_host_record(h, &rec);
        let dir: Vec<EpNote> = self.d[h]
            .host
            .as_ref()
            .unwrap()
            .directory
            .values()
            .cloned()
            .collect();
        let mut reached = vec![];
        for n in dir {
            if let Ok(j) = self.dial(h, &n.claims.endpoint) {
                if self.did(j) == n.claims.owner {
                    self.send(h, j, n.claims.endpoint.clone(), "host-record", rec.clone());
                    if self.admits(j, h).is_ok() {
                        reached.push(j);
                    }
                }
            }
        }
        for other in self.hosts.clone() {
            if other != h {
                let _ = self.host_sync(h, other);
            }
        }
        reached
    }
    pub(super) fn redraw_writer(&mut self, m: usize) -> u64 {
        let new = self.rng.writer();
        let id = self.d[m].id.clone();
        self.d[m].w = Writer::new(id, new);
        let mut texts: Vec<String> = self.d[m]
            .refused
            .drain(..)
            .map(|(p, _)| p.body.text)
            .collect();
        texts.extend(self.d[m].outbox.drain(..).map(|p| p.body.text));
        for t in texts {
            self.post(m, &t);
        }
        new
    }
    // ---- personal block (private audience, U5 revised) ----
    pub(super) fn block(&mut self, i: usize, target: &str) -> Vec<u8> {
        let body = BlockBody {
            from: self.did(i),
            to: target.into(),
            at: self.now,
        };
        let sig = self.d[i].id.sign(&signed(NOTICE_DOMAIN, &body)).to_vec();
        let notice = BlockNotice { body, sig };
        let bytes = serde_json::to_vec(&notice).unwrap();
        self.d[i].blocked.insert(target.into(), notice);
        if let Some(e) = self.d[i].private_book.get(target).cloned() {
            if let Ok(j) = self.dial(i, &e) {
                self.send(i, j, e, "block-notice", bytes.clone());
                let _ = self.receive_block_notice(j, &bytes);
            }
        }
        bytes
    }
    pub(super) fn receive_block_notice(&mut self, j: usize, bytes: &[u8]) -> Result<(), Refuse> {
        let Ok(n) = serde_json::from_slice::<BlockNotice>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !valid(&n.body.from, &signed(NOTICE_DOMAIN, &n.body), &n.sig) {
            return Err(Refuse::BadSignature);
        }
        if n.body.to != self.did(j) {
            return Err(Refuse::NotAddressed);
        }
        self.d[j].view.blocked_by.insert(n.body.from.clone(), n);
        Ok(())
    }
    // Direct private-endpoint attempt; a blocker answers with the same signed notice.
    pub(super) fn direct(&mut self, i: usize, to: &str) -> Direct {
        let Some(e) = self.d[i].private_book.get(to).cloned() else {
            return Direct::Unreachable;
        };
        let Ok(j) = self.dial(i, &e) else {
            return Direct::Unreachable;
        };
        let me = self.did(i);
        if let Some(n) = self.d[j].blocked.get(&me) {
            let b = serde_json::to_vec(n).unwrap();
            let back = self.pep(i);
            self.send(j, i, back, "block-notice", b.clone());
            return Direct::Blocked(b);
        }
        self.send(i, j, e, "direct", vec![]);
        Direct::Delivered
    }
    pub(super) fn assert_converged(&self, devs: &[usize]) {
        let first = &self.d[devs[0]].board;
        for &i in devs {
            let b = &self.d[i].board;
            assert!(b.pending.is_empty(), "device {i} has pending posts");
            assert_eq!(
                b.accepted.keys().collect::<Vec<_>>(),
                first.accepted.keys().collect::<Vec<_>>(),
                "device {i} accepted set differs from device {}",
                devs[0]
            );
            assert_eq!(b.doc.get_deep_value(), first.doc.get_deep_value());
        }
    }
}
pub(super) fn world() -> World {
    World::new(Cfg::default())
}
pub(super) fn everyone() -> Vec<usize> {
    let mut v = MEMBERS.to_vec();
    v.extend([H1, H2]);
    v
}
pub(super) fn set(xs: &[&str]) -> BTreeSet<String> {
    xs.iter().map(|s| s.to_string()).collect()
}
