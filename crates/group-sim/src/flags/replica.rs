use super::*;

// ---- per-device policy + content state -----------------------------------------------------

pub(super) struct Replica {
    pub(super) board: Board,
    pub(super) hosts: BTreeMap<String, HostRecord>, // "<did>#<slot>" -> accepted host record
    pub(super) revoked_permits: BTreeSet<String>,
    pub(super) ops: BTreeMap<String, SignedOp>,
    pub(super) op_pending: BTreeMap<String, SignedOp>,
    pub(super) arrived: BTreeMap<String, u64>,
    pub(super) latched: BTreeSet<String>, // ReceiverClock only
    pub(super) der: Derived,
    pub(super) store: BTreeMap<String, Post>, // policy-accepted posts (board = materialisation)
    pub(super) post_pending: BTreeMap<String, Post>,
    pub(super) dir: Option<BTreeMap<String, EpNote>>, // hosts only (U7): member group notes
    pub(super) rotate_due: bool,
}
impl Replica {
    pub(super) fn new(ctx: &Ctx, host: bool) -> Self {
        Self {
            board: Board::new(ctx.gid.clone()),
            hosts: BTreeMap::new(),
            revoked_permits: BTreeSet::new(),
            ops: BTreeMap::new(),
            op_pending: BTreeMap::new(),
            arrived: BTreeMap::new(),
            latched: BTreeSet::new(),
            der: ctx.derive(&BTreeMap::new(), &BTreeSet::new(), None),
            store: BTreeMap::new(),
            post_pending: BTreeMap::new(),
            dir: host.then(BTreeMap::new),
            rotate_due: false,
        }
    }
    pub(super) fn snap(&self) -> Vec<u8> {
        serde_json::to_vec(&(
            self.board.snapshot(),
            &self.hosts,
            &self.revoked_permits,
            &self.ops,
            &self.op_pending,
            &self.arrived,
            &self.latched,
            &self.der,
            &self.store,
            &self.post_pending,
            &self.dir,
            self.rotate_due,
        ))
        .unwrap()
    }
    // Delivery-order-independent outcome (no arrival times, no board insertion order).
    pub(super) fn outcome(&self) -> Vec<u8> {
        serde_json::to_vec(&(
            &self.der,
            self.ops.keys().collect::<Vec<_>>(),
            self.store.keys().collect::<Vec<_>>(),
            self.board.accepted.keys().collect::<Vec<_>>(),
            self.op_pending.keys().collect::<Vec<_>>(),
            self.post_pending.keys().collect::<Vec<_>>(),
            &self.revoked_permits,
        ))
        .unwrap()
    }
    pub(super) fn accept_op(&mut self, ctx: &Ctx, bytes: &[u8], now: u64) -> Result<Acc, Refuse> {
        let Ok(op) = serde_json::from_slice::<SignedOp>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !op.verify() {
            return Err(Refuse::BadSignature);
        }
        if op.body.group != ctx.gid {
            return Err(Refuse::WrongGroup);
        }
        let id = op.id();
        if self.ops.contains_key(&id) {
            return Ok(Acc::Duplicate);
        }
        if self.op_pending.contains_key(&id) {
            return Ok(Acc::Pending);
        }
        ctx.static_check(&op)?;
        if !op.body.seen.iter().all(|s| self.ops.contains_key(s)) {
            if self.op_pending.len() >= MAX_PENDING {
                return Err(Refuse::Limit);
            }
            self.op_pending.insert(id, op);
            return Ok(Acc::Pending);
        }
        self.frontier_check(ctx, &op)?;
        self.apply_op(id, op, now);
        self.drain_ops(ctx, now);
        self.recompute(ctx, now);
        Ok(Acc::Applied)
    }
    // Every op is authorized at its OWN signed frontier, so the verdict does not depend on
    // delivery order (ops with stale frontiers are the D02-style limitation).
    pub(super) fn frontier_check(&self, ctx: &Ctx, op: &SignedOp) -> Result<(), Refuse> {
        let fr = closure(&self.ops, &op.body.seen);
        let d = ctx.derive(&self.ops, &fr, None);
        if d.removed.contains(&op.body.issuer) {
            return Err(Refuse::IssuerRemoved);
        }
        match &op.body.op {
            Op::Flag { target, .. } => {
                if !d.roster.contains(target) {
                    return Err(Refuse::NotMember);
                }
            }
            Op::Withdraw { flag } => {
                if !fr.contains(flag) {
                    return Err(Refuse::UnknownFlag);
                }
                let f = &self.ops[flag];
                let Op::Flag { target, .. } = &f.body.op else {
                    return Err(Refuse::UnknownFlag);
                };
                if f.body.issuer != op.body.issuer {
                    return Err(Refuse::NotFlagger);
                }
                if d.removed.contains(target) {
                    return Err(Refuse::AfterThreshold);
                }
            }
            Op::Admit { grant, lifts } => {
                if d.revoked_grants.contains(&grant.digest()) {
                    return Err(Refuse::GrantRevoked);
                }
                let h = &grant.body.holder;
                if lifts.is_empty() {
                    if d.removed.contains(h) {
                        return Err(Refuse::Removed);
                    }
                } else if !d.removed.contains(h)
                    || !lifts.iter().all(|l| {
                        matches!(self.ops.get(l).map(|o| &o.body.op),
                            Some(Op::Flag { target, .. }) if target == h)
                    })
                {
                    return Err(Refuse::BadLift);
                }
            }
            Op::RemoveHost { .. } => {}
        }
        Ok(())
    }
    pub(super) fn apply_op(&mut self, id: String, op: SignedOp, now: u64) {
        if let Op::RemoveHost { permit } = &op.body.op {
            if self.revoked_permits.insert(permit.clone()) {
                self.hosts.retain(|_, r| r.permit.digest() != *permit);
                self.rotate_due = true; // section 10 Q8: group-level rotation
            }
        }
        self.arrived.insert(id.clone(), now);
        self.ops.insert(id, op);
    }
    pub(super) fn drain_ops(&mut self, ctx: &Ctx, now: u64) {
        loop {
            let ready = self
                .op_pending
                .iter()
                .find(|(_, o)| o.body.seen.iter().all(|s| self.ops.contains_key(s)))
                .map(|(k, _)| k.clone());
            let Some(id) = ready else { break };
            let op = self.op_pending.remove(&id).unwrap();
            if self.frontier_check(ctx, &op).is_ok() {
                self.apply_op(id, op, now);
            }
        }
    }
    pub(super) fn tick(&mut self, ctx: &Ctx, now: u64) {
        self.recompute(ctx, now);
    }
    pub(super) fn recompute(&mut self, ctx: &Ctx, now: u64) {
        let all: BTreeSet<String> = self.ops.keys().cloned().collect();
        let receiver_clock = matches!(ctx.cfg.expiry, Expiry::ReceiverClock(_));
        let live = receiver_clock.then_some((&self.arrived, now));
        let mut d = ctx.derive(&self.ops, &all, live);
        if receiver_clock {
            let lifted: BTreeSet<String> = self
                .ops
                .values()
                .filter_map(|o| match &o.body.op {
                    Op::Admit { grant, lifts } if !lifts.is_empty() => {
                        Some(grant.body.holder.clone())
                    }
                    _ => None,
                })
                .collect();
            self.latched.extend(d.removed.iter().cloned());
            self.latched.retain(|t| !lifted.contains(t));
            d.removed = self.latched.clone();
            d.roster = d.members.difference(&d.removed).cloned().collect();
        }
        self.der = d;
        let removed = self.der.removed.clone();
        if let Some(dir) = self.dir.as_mut() {
            dir.retain(|_, n| !removed.contains(&n.claims.owner));
        }
        let mut dropped = false;
        for id in self.store.keys().cloned().collect::<Vec<_>>() {
            let p = self.store[&id].clone();
            if self.post_ok(ctx, &p).is_err() {
                self.store.remove(&id);
                dropped = true;
            }
        }
        if dropped {
            self.rebuild_board(ctx);
        }
        self.drain_posts(ctx);
    }
    pub(super) fn post_ok(&self, ctx: &Ctx, p: &Post) -> Result<(), Why> {
        let fr = closure(&self.ops, &p.body.ops_seen);
        let d = ctx.derive(&self.ops, &fr, None);
        if !d.roster.contains(&p.body.author) {
            return Err(Why::NotMember);
        }
        if ctx.cfg.post == PostRule::RemovalWins && self.der.removed.contains(&p.body.author) {
            let pid = p.id();
            let seen_by_a_flag = self.ops.values().any(|o| {
                matches!(&o.body.op, Op::Flag { target, seen_posts, .. }
                    if *target == p.body.author && seen_posts.contains(&pid))
            });
            if !seen_by_a_flag {
                return Err(Why::RemovedConcurrent);
            }
        }
        Ok(())
    }
    pub(super) fn accept_post(&mut self, ctx: &Ctx, p: Post) -> Verdict {
        if let Err(why) = self.board.precheck(&p) {
            return Verdict::Rejected(why);
        }
        let id = p.id();
        if self.store.contains_key(&id) {
            return Verdict::Duplicate;
        }
        if self.post_pending.contains_key(&id) {
            return Verdict::Pending;
        }
        if !self.der.members.contains(&p.body.author) {
            return Verdict::Rejected(Why::NotMember);
        }
        if !p.body.ops_seen.iter().all(|s| self.ops.contains_key(s)) {
            if self.post_pending.len() >= MAX_PENDING {
                return Verdict::Rejected(Why::Limit);
            }
            self.post_pending.insert(id, p);
            return Verdict::Pending;
        }
        self.admit_post(ctx, id, p)
    }
    pub(super) fn admit_post(&mut self, ctx: &Ctx, id: String, p: Post) -> Verdict {
        if let Err(why) = self.post_ok(ctx, &p) {
            return Verdict::Rejected(why);
        }
        let v = self.board.receive(p.clone());
        if matches!(v, Verdict::Accepted | Verdict::Pending) {
            self.store.insert(id, p);
        }
        v
    }
    pub(super) fn drain_posts(&mut self, ctx: &Ctx) {
        loop {
            let ready = self
                .post_pending
                .iter()
                .find(|(_, p)| p.body.ops_seen.iter().all(|s| self.ops.contains_key(s)))
                .map(|(k, _)| k.clone());
            let Some(id) = ready else { break };
            let p = self.post_pending.remove(&id).unwrap();
            let _ = self.admit_post(ctx, id, p);
        }
    }
    // Real Loro board rebuilt from the surviving policy-accepted posts. A dropped post leaves
    // later posts on the same writer chain stranded in `board.pending` (D01 Q4).
    pub(super) fn rebuild_board(&mut self, ctx: &Ctx) {
        let mut posts: Vec<Post> = self.store.values().cloned().collect();
        posts.sort_by_key(|p| (p.body.writer, p.body.seq));
        self.board = Board::new(ctx.gid.clone());
        for p in posts {
            self.board.receive(p);
        }
    }
    pub(super) fn check_record(&self, ctx: &Ctx, r: &HostRecord) -> Result<(), Refuse> {
        if !r.note.verify() {
            return Err(Refuse::BadSignature);
        }
        if r.note.claims.group != ctx.gid {
            return Err(Refuse::WrongGroup);
        }
        if !r.permit.allows(&ctx.g, &r.note.claims.owner, Cap::Host) {
            return Err(Refuse::PermitInvalid);
        }
        if r.permit.body.device != Some(r.note.claims.slot) {
            return Err(Refuse::DeviceMismatch);
        }
        if self.revoked_permits.contains(&r.permit.digest()) {
            return Err(Refuse::Revoked);
        }
        Ok(())
    }
    pub(super) fn accept_host_record(&mut self, ctx: &Ctx, bytes: &[u8]) -> Result<(), Refuse> {
        let Ok(r) = serde_json::from_slice::<HostRecord>(bytes) else {
            return Err(Refuse::Malformed);
        };
        self.check_record(ctx, &r)?;
        let key = dir_key(&r.note.claims.owner, r.note.claims.slot);
        if let Some(cur) = self.hosts.get(&key) {
            if cur.note.claims.seq >= r.note.claims.seq {
                return Err(Refuse::Stale);
            }
        }
        self.hosts.insert(key, r);
        Ok(())
    }
    pub(super) fn accept_note(&mut self, ctx: &Ctx, bytes: &[u8]) -> Result<(), Refuse> {
        let Ok(n) = serde_json::from_slice::<EpNote>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !n.verify() {
            return Err(Refuse::BadSignature);
        }
        if n.claims.group != ctx.gid {
            return Err(Refuse::WrongGroup);
        }
        if !self.der.roster.contains(&n.claims.owner) {
            return Err(Refuse::NotMember);
        }
        let key = dir_key(&n.claims.owner, n.claims.slot);
        let Some(dir) = self.dir.as_mut() else {
            return Err(Refuse::NotHost);
        };
        if let Some(cur) = dir.get(&key) {
            if cur.claims.seq >= n.claims.seq {
                return Err(Refuse::Stale);
            }
        }
        dir.insert(key, n);
        Ok(())
    }
}

// ---- devices and world (copied from multi_host.rs @0924939, trimmed) -----------------------

pub(super) struct Device {
    pub(super) id: Identity,
    pub(super) slot: u64, // random device ID / route slot; owner-replaceable, never key-derived
    pub(super) w: Writer,
    pub(super) rep: Replica,
    pub(super) group_key: SigningKey,
    pub(super) my_note: EpNote,
    pub(super) host: Option<HostRecord>, // own record if this device is a host
    pub(super) acks: BTreeMap<String, u64>,
    pub(super) op_out: Vec<SignedOp>,
    pub(super) outbox: Vec<Post>,
    pub(super) refused: Vec<(Post, Why)>,
    pub(super) online: bool,
}
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
