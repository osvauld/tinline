use super::*;

impl World {
    pub(super) fn xfer(&mut self, from: usize, to: usize) {
        let recs: Vec<HostRecord> = self.d[from].rep.hosts.values().cloned().collect();
        for r in recs {
            let k = dir_key(&r.note.claims.owner, r.note.claims.slot);
            if self.d[to].rep.hosts.get(&k) != Some(&r) {
                self.send(from, to, "host-record", r.wire());
                let _ = self.accept_host_record(to, &r.wire());
            }
        }
        let ops: Vec<SignedOp> = self.d[from]
            .rep
            .ops
            .iter()
            .filter(|(id, _)| !self.d[to].rep.ops.contains_key(*id))
            .map(|(_, o)| o.clone())
            .collect();
        for o in ops {
            self.send(from, to, "op", o.wire());
            let _ = self.accept_op(to, &o.wire());
        }
        let notes: Vec<EpNote> = self.d[from]
            .rep
            .dir
            .as_ref()
            .map(|dir| dir.values().cloned().collect())
            .unwrap_or_default();
        for n in notes {
            let k = dir_key(&n.claims.owner, n.claims.slot);
            let cur = self.d[to]
                .rep
                .dir
                .as_ref()
                .and_then(|dir| dir.get(&k))
                .map(|c| c.claims.seq);
            if cur < Some(n.claims.seq) {
                self.send(from, to, "dir-note", n.wire());
                let _ = self.host_accept_note(to, &n.wire());
            }
        }
        let posts: Vec<Post> = self.d[from]
            .rep
            .store
            .iter()
            .filter(|(id, _)| {
                !self.d[to].rep.store.contains_key(*id)
                    && !self.d[to].rep.post_pending.contains_key(*id)
            })
            .map(|(_, p)| p.clone())
            .collect();
        for p in posts {
            self.send(from, to, "post", p.wire());
            self.accept_post(to, p);
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
    // ---- NEW: removal -> announcement -> delayed rotation (U12) ----
    // Runs after every link/op: each host that has derived a removal it has not scheduled
    // yet schedules a rotation (per `Batching`), signs an announcement and pushes it to every
    // remaining member it can reach (its directory no longer holds removed members).
    pub(super) fn after(&mut self) {
        loop {
            let mut progressed = false;
            for h in self.hosts.clone() {
                let fresh: BTreeSet<String> = {
                    let s = self.d[h].sched.as_ref().unwrap();
                    self.d[h]
                        .rep
                        .der
                        .removed
                        .iter()
                        .filter(|x| !s.learned.contains_key(*x))
                        .cloned()
                        .collect()
                };
                if !fresh.is_empty() {
                    progressed = true;
                    self.schedule(h, fresh);
                }
            }
            if !progressed {
                break;
            }
        }
    }
    pub(super) fn schedule(&mut self, h: usize, fresh: BTreeSet<String>) {
        let now = self.now;
        let due = now + ROTATION_DELAY;
        let batching = self.ctx.cfg.batching;
        let s = self.d[h].sched.as_mut().unwrap();
        for x in &fresh {
            s.learned.insert(x.clone(), now);
        }
        let rotate_at = match batching {
            Batching::PerRemoval => {
                s.pending.entry(due).or_default().extend(fresh.clone());
                due
            }
            Batching::KeepFirstDeadline => {
                let at = s.pending.keys().next().copied().unwrap_or(due);
                s.pending.entry(at).or_default().extend(fresh.clone());
                at
            }
            Batching::ExtendToLast => {
                let mut all = fresh.clone();
                for (_, v) in std::mem::take(&mut s.pending) {
                    all.extend(v);
                }
                s.pending.insert(due, all);
                due
            }
        };
        s.seq += 1;
        let seq = s.seq;
        let mut cites = BTreeSet::new();
        for x in &fresh {
            let fl = self.d[h].rep.der.flags.get(x).cloned().unwrap_or_default();
            cites.extend(closure(&self.d[h].rep.ops, &fl));
        }
        let body = AnnBody {
            group: self.ctx.gid.clone(),
            host: self.did(h),
            slot: self.d[h].slot,
            seq,
            removed: fresh,
            cites,
            rotate_at,
        };
        let permit = self.d[h].host.as_ref().unwrap().permit.clone();
        self.d[h].ann = Some(Announce::sign(&self.d[h].id, body, permit));
        // The host reaches members itself (section 9 Q6); unreachable members get it on
        // their next link while it is still pending.
        let dir: Vec<EpNote> = self.d[h]
            .rep
            .dir
            .as_ref()
            .map(|dir| dir.values().cloned().collect())
            .unwrap_or_default();
        for n in dir {
            if let Ok(j) = self.dial(h, &n.claims.endpoint) {
                if self.did(j) == n.claims.owner && self.admits(j, h).is_ok() {
                    self.exchange(j, h);
                }
            }
        }
    }
    // Moves the fake clock and performs every rotation that falls due (hosts first, then
    // members under MembersToo). A host offline at its deadline rotates when next advanced.
    pub(super) fn advance(&mut self, to: u64) {
        assert!(to >= self.now);
        self.now = to;
        loop {
            let due = self
                .hosts
                .iter()
                .filter(|&&h| self.d[h].online)
                .filter_map(|&h| {
                    let (&at, _) = self.d[h].sched.as_ref()?.pending.iter().next()?;
                    (at <= to).then_some((at, h))
                })
                .min();
            let Some((at, h)) = due else { break };
            let s = self.d[h].sched.as_mut().unwrap();
            let set = s.pending.remove(&at).unwrap();
            s.done.push((to, set));
            self.rotate_host(h);
            self.after();
        }
        for m in MEMBERS {
            let me = self.did(m);
            let r = &mut self.d[m].rep;
            if r.member_due.is_some_and(|t| t <= to) && !r.der.removed.contains(&me) {
                r.member_due = None;
                if self.d[m].online {
                    self.rotate_member(m);
                }
            }
        }
        self.after();
    }
    // Copied from flags.rs @82d221f `rotate_member` (no retention).
    pub(super) fn rotate_member(&mut self, m: usize) -> Vec<usize> {
        let old = self.gep(m);
        self.live.remove(&old);
        self.d[m].group_key = SigningKey::from_bytes(&self.rng.bytes32());
        let seq = self.d[m].my_note.claims.seq + 1;
        let new = self.gep(m);
        let note = EpNote::sign(&self.d[m].id, &self.ctx.gid, self.d[m].slot, &new, seq);
        self.d[m].my_note = note.clone();
        self.live.insert(new, m);
        let mut reached = vec![];
        for h in self.hosts.clone() {
            let Some(e) = self.host_ep(m, h) else {
                continue;
            };
            if self.dial(m, &e) == Ok(h) {
                self.send(m, h, "ep-note", note.wire());
                if self.host_accept_note(h, &note.wire()).is_ok() {
                    let k = self.hk(h);
                    self.d[m].acks.insert(k, seq);
                    reached.push(h);
                }
            }
        }
        reached
    }
    // Copied from flags.rs @82d221f `rotate_host`: new key, old endpoint dead, the host dials
    // every member still in its directory (removed members were dropped), hands over its new
    // record and runs a full exchange; then syncs with the other hosts.
    pub(super) fn rotate_host(&mut self, h: usize) -> Vec<usize> {
        let old = self.gep(h);
        self.live.remove(&old);
        self.d[h].group_key = SigningKey::from_bytes(&self.rng.bytes32());
        let seq = self.d[h].my_note.claims.seq + 1;
        let new = self.gep(h);
        let note = EpNote::sign(&self.d[h].id, &self.ctx.gid, self.d[h].slot, &new, seq);
        self.d[h].my_note = note.clone();
        self.live.insert(new, h);
        self.d[h].host.as_mut().unwrap().note = note;
        let rec = self.d[h].host.as_ref().unwrap().wire();
        let _ = self.accept_host_record(h, &rec);
        let dir: Vec<EpNote> = self.d[h]
            .rep
            .dir
            .as_ref()
            .map(|dir| dir.values().cloned().collect())
            .unwrap_or_default();
        let mut reached = vec![];
        for n in dir {
            if let Ok(j) = self.dial(h, &n.claims.endpoint) {
                if self.did(j) == n.claims.owner {
                    self.send(h, j, "host-record", rec.clone());
                    if self.admits(j, h).is_ok() {
                        self.exchange(j, h);
                        reached.push(j);
                    }
                }
            }
        }
        for other in self.hosts.clone() {
            if other != h {
                let _ = self.host_sync_inner(h, other);
            }
        }
        reached
    }
    pub(super) fn assert_converged(&self, devs: &[usize]) {
        let first = &self.d[devs[0]].rep.board;
        for &i in devs {
            let b = &self.d[i].rep.board;
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
    // Every endpoint `i` has ever been told about (frames + state), other than its own.
    pub(super) fn known_endpoints(&self, i: usize) -> Vec<String> {
        let hay = self.received(i);
        let own = self.gep(i);
        let mut all: Vec<String> = self.live.keys().cloned().collect();
        for dv in &self.d {
            if let Some(r) = &dv.host {
                all.push(r.note.claims.endpoint.clone());
            }
        }
        for f in &self.frames {
            if let Ok(r) = serde_json::from_slice::<HostRecord>(&f.bytes) {
                all.push(r.note.claims.endpoint);
            }
            if let Ok(n) = serde_json::from_slice::<EpNote>(&f.bytes) {
                all.push(n.claims.endpoint);
            }
        }
        all.sort();
        all.dedup();
        all.into_iter()
            .filter(|e| *e != own && has(&hay, e.as_bytes()))
            .collect()
    }
    // U12 end state for a removed device: every endpoint it ever learned is dead, and it
    // cannot link to any host in either direction.
    pub(super) fn assert_reaches_nobody(&mut self, i: usize) {
        for e in self.known_endpoints(i) {
            assert_eq!(self.dial(i, &e), Err(RouteErr::EndpointGone), "{e}");
        }
        for h in self.hosts.clone() {
            assert_eq!(self.connect(i, h), Err(RouteErr::Lost), "host {h}");
            assert_eq!(self.connect_from_host(h, i), Err(RouteErr::NoAddress));
        }
    }
    pub(super) fn texts(&self, i: usize) -> BTreeSet<String> {
        self.d[i].rep.board.texts()
    }
}
pub(super) fn world() -> World {
    World::new(Cfg::default())
}
pub(super) fn with(f: impl FnOnce(&mut Cfg)) -> World {
    let mut cfg = Cfg::default();
    f(&mut cfg);
    World::new(cfg)
}
pub(super) fn remaining(gone: &[usize]) -> Vec<usize> {
    MEMBERS.into_iter().filter(|m| !gone.contains(m)).collect()
}
pub(super) fn with_hosts(devs: Vec<usize>) -> Vec<usize> {
    let mut v = devs;
    v.extend([H1, H2]);
    v
}
// U9: three distinct admins flag `target`; one round delivers the flags to both hosts.
pub(super) fn remove(w: &mut World, target: usize) -> Vec<SignedOp> {
    let fs = [A1, BOB, DAVE]
        .into_iter()
        .map(|a| w.flag_and_submit(a, target))
        .collect();
    w.round();
    fs
}
pub(super) fn note_seq(w: &World, i: usize) -> u64 {
    w.d[i].my_note.claims.seq
}
