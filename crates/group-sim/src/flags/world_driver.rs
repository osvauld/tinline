use super::*;

impl World {
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
    // Section 10 Q8: a device that newly accepts a host removal rotates its group endpoint
    // once. Members send the new note only to hosts still permitted in their view; remaining
    // hosts rotate and reconnect members themselves. The removed host never rotates here.
    pub(super) fn run_rotations(&mut self) {
        while let Some(i) = (0..self.d.len()).find(|&i| self.d[i].rep.rotate_due) {
            self.d[i].rep.rotate_due = false;
            let revoked_self = self.d[i]
                .host
                .as_ref()
                .is_some_and(|r| self.d[i].rep.revoked_permits.contains(&r.permit.digest()));
            if revoked_self || !self.d[i].online {
                continue;
            }
            if self.d[i].host.is_some() {
                self.rotate_host(i);
            } else {
                self.rotate_member(i);
            }
        }
    }
    // Copied from multi_host.rs @0924939 (no retention): note to every permitted host.
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
            let Some(e) = self.d[m]
                .rep
                .hosts
                .get(&self.hk(h))
                .map(|r| r.note.claims.endpoint.clone())
            else {
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
    // Copied from multi_host.rs @0924939 `rotate_host`; changed: after handing over the new
    // record the host runs a full exchange, so the revocation op reaches each member too.
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
    // Revocation wave used by the host-revocation tests: the owner signs `RemoveHost` and
    // hands it to a legitimate host, which rotates and pushes it to every member it reaches.
    pub(super) fn revoke_host(&mut self, issuer: usize, via: usize, gone: usize) -> SignedOp {
        let permit = self.permit_digest(gone);
        let op = self.op(issuer, Op::RemoveHost { permit });
        assert_eq!(self.submit(issuer, op.clone()), Ok(Acc::Applied));
        self.connect(issuer, via).unwrap();
        op
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
}
pub(super) fn world() -> World {
    World::new(Cfg::default())
}
pub(super) fn with(f: impl FnOnce(&mut Cfg)) -> World {
    let mut cfg = Cfg::default();
    f(&mut cfg);
    World::new(cfg)
}
pub(super) fn everyone(w: &World) -> Vec<usize> {
    let mut v = MEMBERS.to_vec();
    v.extend(w.hosts.iter().copied());
    v
}

// ---- delivery-order exploration (pattern from policy_candidates.rs `explore`) --------------

#[derive(Clone)]
pub(super) enum Ev {
    Op(SignedOp),
    Post(Post),
}
pub(super) fn deliver(rep: &mut Replica, ctx: &Ctx, ev: &Ev, now: u64) {
    match ev {
        Ev::Op(o) => {
            let _ = rep.accept_op(ctx, &o.wire(), now);
        }
        Ev::Post(p) => {
            let _ = rep.accept_post(ctx, p.clone());
        }
    }
}
pub(super) fn permutations(n: usize) -> Vec<Vec<usize>> {
    fn go(prefix: &mut Vec<usize>, rest: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if rest.is_empty() {
            out.push(prefix.clone());
            return;
        }
        for i in 0..rest.len() {
            let x = rest.remove(i);
            prefix.push(x);
            go(prefix, rest, out);
            prefix.pop();
            rest.insert(i, x);
        }
    }
    let mut out = vec![];
    go(&mut vec![], &mut (0..n).collect(), &mut out);
    out
}
pub(super) struct Explored {
    pub(super) orders: usize,
    pub(super) finals: BTreeSet<Vec<u8>>,
    pub(super) retro_post: bool, // some order showed a post accepted, then dropped it
    pub(super) retro_removal: bool, // some order showed a DID removed, then not removed
    pub(super) last: Replica,
}
pub(super) fn explore(ctx: &Ctx, events: &[Ev]) -> Explored {
    let (mut finals, mut retro_post, mut retro_removal) = (BTreeSet::new(), false, false);
    let mut last = None;
    let orders = permutations(events.len());
    for order in &orders {
        let mut r = Replica::new(ctx, false);
        let (mut shown, mut removed) = (BTreeSet::new(), BTreeSet::new());
        for &k in order {
            deliver(&mut r, ctx, &events[k], 0);
            shown.extend(r.board.accepted.keys().cloned());
            removed.extend(r.der.removed.iter().cloned());
        }
        retro_post |= shown.iter().any(|p| !r.board.accepted.contains_key(p));
        retro_removal |= removed.iter().any(|d| !r.der.removed.contains(d));
        finals.insert(r.outcome());
        last = Some(r);
    }
    Explored {
        orders: orders.len(),
        finals,
        retro_post,
        retro_removal,
        last: last.unwrap(),
    }
}
pub(super) fn set(xs: &[&str]) -> BTreeSet<String> {
    xs.iter().map(|s| s.to_string()).collect()
}
pub(super) fn ids(ops: &[&SignedOp]) -> BTreeSet<String> {
    ops.iter().map(|o| o.id()).collect()
}
