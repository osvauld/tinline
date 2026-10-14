use super::*;

// ---- NEW: host "rotating soon" announcement (U12) -----------------------------------------

/// Signed by the host's DID, carried with its owner-issued {Host} permit. `cites` = the flag
/// ops (and their causal frontier) that establish every DID in `removed`, so each member can
/// check the removal itself (U10). `rotate_at` is the host's own clock: members cannot
/// verify it, they only learn when to expect a new host record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct AnnBody {
    pub(super) group: String,
    pub(super) host: String,
    pub(super) slot: u64,
    pub(super) seq: u64,
    pub(super) removed: BTreeSet<String>,
    pub(super) cites: BTreeSet<String>,
    pub(super) rotate_at: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Announce {
    pub(super) body: AnnBody,
    pub(super) permit: MGrant,
    pub(super) sig: Vec<u8>,
}
impl Announce {
    pub(super) fn sign(id: &Identity, body: AnnBody, permit: MGrant) -> Self {
        let sig = id.sign(&signed(ANNOUNCE_DOMAIN, &body)).to_vec();
        Self { body, permit, sig }
    }
    pub(super) fn verify(&self) -> bool {
        valid(
            &self.body.host,
            &signed(ANNOUNCE_DOMAIN, &self.body),
            &self.sig,
        )
    }
    pub(super) fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}

// ---- NEW: DID-signed call request relayed by a host (fixture for "no new calls") ----------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct CallBody {
    pub(super) group: String,
    pub(super) caller: String,
    pub(super) callee: String,
    pub(super) nonce: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct CallReq {
    pub(super) body: CallBody,
    pub(super) sig: Vec<u8>,
}
impl CallReq {
    pub(super) fn sign(id: &Identity, body: CallBody) -> Self {
        let sig = id.sign(&signed(CALL_DOMAIN, &body)).to_vec();
        Self { body, sig }
    }
    pub(super) fn verify(&self) -> bool {
        valid(
            &self.body.caller,
            &signed(CALL_DOMAIN, &self.body),
            &self.sig,
        )
    }
    pub(super) fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refuse {
    Malformed,
    BadSignature,
    WrongGroup,
    PermitInvalid,
    DeviceMismatch,
    Stale,
    NotAdmin,
    NotMember,
    NotHost,
    NotFlaggable,
    IssuerRemoved,
    Removed,
    NotRemoved,
    Unknown,
    Unreachable,
    Limit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Acc {
    Applied,
    Duplicate,
    Pending,
}

// ---- configuration (open policy as parameters; defaults are NOT selections) ----------------

/// U11 open / G-HISTORY: bound on a removed member's pre-removal uploads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UploadCap {
    /// Only the host rotation ends the upload window (coordinator note under U12).
    RotationOnly,
    /// Each host accepts at most this many posts by a removed author after the removal.
    Count(usize),
}
/// U12 open / G-TIME: several removals inside one delay window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Batching {
    /// A later removal joins the pending rotation; its deadline is NOT moved.
    KeepFirstDeadline,
    /// A later removal joins the pending rotation and moves it to (last removal + delay).
    ExtendToLast,
    /// Every removal schedules its own rotation at (its removal + delay).
    PerRemoval,
}
/// U12 open / G-DISCOVERY: does a member removal also rotate members' own group endpoints?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MemberRotation {
    HostsOnly,
    /// Each member that accepted an announcement rotates once at its `rotate_at`.
    MembersToo,
}
#[derive(Clone)]
pub(super) struct Cfg {
    pub(super) seed: u64,
    pub(super) hosts: Vec<usize>,
    pub(super) cap: UploadCap,
    pub(super) batching: Batching,
    pub(super) members: MemberRotation,
}
impl Default for Cfg {
    fn default() -> Self {
        Self {
            seed: 0x5EED_02F7,
            hosts: vec![H1, H2],
            cap: UploadCap::RotationOnly,
            batching: Batching::KeepFirstDeadline,
            members: MemberRotation::HostsOnly,
        }
    }
}

// Immutable group context every device shares (genesis + fixture bootstrap).
#[derive(Clone)]
pub(super) struct Ctx {
    pub(super) cfg: Cfg,
    pub(super) g: Genesis,
    pub(super) gid: String,
    pub(super) creator: String,
    pub(super) admins: BTreeSet<String>,
    pub(super) base: BTreeSet<String>,
    pub(super) grants: BTreeMap<String, MGrant>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct Derived {
    pub(super) removed: BTreeSet<String>,
    pub(super) roster: BTreeSet<String>,
    pub(super) flags: BTreeMap<String, BTreeSet<String>>, // target -> flag op ids
}

pub(super) fn closure(
    ops: &BTreeMap<String, SignedOp>,
    frontier: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack: Vec<String> = frontier.iter().cloned().collect();
    while let Some(id) = stack.pop() {
        if let Some(o) = ops.get(&id) {
            if out.insert(id) {
                stack.extend(o.body.seen.iter().cloned());
            }
        }
    }
    out
}

impl Ctx {
    // Membership as a pure function of an op set (trimmed from flags.rs: distinct admins).
    pub(super) fn derive(
        &self,
        ops: &BTreeMap<String, SignedOp>,
        ids: &BTreeSet<String>,
    ) -> Derived {
        let mut issuers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut flags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for id in ids {
            let Some(o) = ops.get(id) else { continue };
            let Op::Flag { target } = &o.body.op;
            issuers
                .entry(target.clone())
                .or_default()
                .insert(o.body.issuer.clone());
            flags.entry(target.clone()).or_default().insert(id.clone());
        }
        let removed: BTreeSet<String> = issuers
            .into_iter()
            .filter(|(_, s)| s.len() >= FLAG_THRESHOLD)
            .map(|(t, _)| t)
            .collect();
        let roster = self.base.difference(&removed).cloned().collect();
        Derived {
            removed,
            roster,
            flags,
        }
    }
    pub(super) fn static_check(&self, op: &SignedOp) -> Result<(), Refuse> {
        let issuer = &op.body.issuer;
        let is_admin = *issuer == self.creator
            || op
                .body
                .issuer_grant
                .as_ref()
                .is_some_and(|gr| gr.allows(&self.g, issuer, Cap::Admin));
        if !is_admin {
            return Err(Refuse::NotAdmin);
        }
        let Op::Flag { target } = &op.body.op;
        if self.admins.contains(target) {
            return Err(Refuse::NotFlaggable);
        }
        Ok(())
    }
}

// ---- per-device policy + content state -----------------------------------------------------

pub(super) struct Replica {
    pub(super) board: Board,
    pub(super) hosts: BTreeMap<String, HostRecord>, // "<did>#<slot>" -> accepted host record
    pub(super) ops: BTreeMap<String, SignedOp>,
    pub(super) op_pending: BTreeMap<String, SignedOp>,
    pub(super) der: Derived,
    pub(super) store: BTreeMap<String, Post>,
    pub(super) post_pending: BTreeMap<String, Post>,
    pub(super) dir: Option<BTreeMap<String, EpNote>>, // hosts only (U7): member group notes
    pub(super) late: BTreeMap<String, BTreeSet<String>>, // hosts: removed author -> posts taken since
    pub(super) announced: BTreeMap<String, Announce>, // members: host key -> latest verified announcement
    pub(super) member_due: Option<u64>,               // members, MembersToo only
    pub(super) calls: Vec<String>, // relayed (host) or received (member) call ids
}
impl Replica {
    pub(super) fn new(ctx: &Ctx, host: bool) -> Self {
        Self {
            board: Board::new(ctx.gid.clone()),
            hosts: BTreeMap::new(),
            ops: BTreeMap::new(),
            op_pending: BTreeMap::new(),
            der: ctx.derive(&BTreeMap::new(), &BTreeSet::new()),
            store: BTreeMap::new(),
            post_pending: BTreeMap::new(),
            dir: host.then(BTreeMap::new),
            late: BTreeMap::new(),
            announced: BTreeMap::new(),
            member_due: None,
            calls: vec![],
        }
    }
    pub(super) fn snap(&self) -> Vec<u8> {
        serde_json::to_vec(&(
            self.board.snapshot(),
            &self.hosts,
            &self.ops,
            &self.op_pending,
            &self.der,
            &self.store,
            &self.post_pending,
            &self.dir,
            &self.late,
            &self.announced,
            self.member_due,
            &self.calls,
        ))
        .unwrap()
    }
    pub(super) fn accept_op(&mut self, ctx: &Ctx, bytes: &[u8]) -> Result<Acc, Refuse> {
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
        self.ops.insert(id, op);
        self.drain_ops(ctx);
        self.recompute(ctx);
        Ok(Acc::Applied)
    }
    pub(super) fn frontier_check(&self, ctx: &Ctx, op: &SignedOp) -> Result<(), Refuse> {
        let d = ctx.derive(&self.ops, &closure(&self.ops, &op.body.seen));
        if d.removed.contains(&op.body.issuer) {
            return Err(Refuse::IssuerRemoved);
        }
        let Op::Flag { target } = &op.body.op;
        if !d.roster.contains(target) {
            return Err(Refuse::NotMember);
        }
        Ok(())
    }
    pub(super) fn drain_ops(&mut self, ctx: &Ctx) {
        loop {
            let ready = self
                .op_pending
                .iter()
                .find(|(_, o)| o.body.seen.iter().all(|s| self.ops.contains_key(s)))
                .map(|(k, _)| k.clone());
            let Some(id) = ready else { break };
            let op = self.op_pending.remove(&id).unwrap();
            if self.frontier_check(ctx, &op).is_ok() {
                self.ops.insert(id, op);
            }
        }
    }
    pub(super) fn recompute(&mut self, ctx: &Ctx) {
        let all: BTreeSet<String> = self.ops.keys().cloned().collect();
        self.der = ctx.derive(&self.ops, &all);
        let removed = self.der.removed.clone();
        if let Some(dir) = self.dir.as_mut() {
            dir.retain(|_, n| !removed.contains(&n.claims.owner));
        }
        self.drain_posts(ctx);
    }
    // U11 (decided): signed frontier — the author is a member at the post's own frontier.
    pub(super) fn post_ok(&self, ctx: &Ctx, p: &Post) -> Result<(), Why> {
        let d = ctx.derive(&self.ops, &closure(&self.ops, &p.body.ops_seen));
        if !d.roster.contains(&p.body.author) {
            return Err(Why::NotMember);
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
        if !ctx.base.contains(&p.body.author) {
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
        // Cap candidate: enforced by HOSTS only (members verify the signed frontier alone).
        let author = p.body.author.clone();
        let late = self.dir.is_some() && self.der.removed.contains(&author);
        if let (true, UploadCap::Count(n)) = (late, ctx.cfg.cap) {
            if self.late.get(&author).map_or(0, |s| s.len()) >= n {
                return Verdict::Rejected(Why::UploadCap);
            }
        }
        let v = self.board.receive(p.clone());
        if matches!(v, Verdict::Accepted | Verdict::Pending) {
            self.store.insert(id.clone(), p);
            if late {
                self.late.entry(author).or_default().insert(id);
            }
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
    // NEW (U10, U12): every member verifies an announcement itself before keeping it.
    pub(super) fn accept_announce(
        &mut self,
        ctx: &Ctx,
        bytes: &[u8],
        me: &str,
    ) -> Result<(), Refuse> {
        let Ok(a) = serde_json::from_slice::<Announce>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !a.verify() {
            return Err(Refuse::BadSignature);
        }
        if a.body.group != ctx.gid {
            return Err(Refuse::WrongGroup);
        }
        if !a.permit.allows(&ctx.g, &a.body.host, Cap::Host) {
            return Err(Refuse::PermitInvalid);
        }
        if a.permit.body.device != Some(a.body.slot) {
            return Err(Refuse::DeviceMismatch);
        }
        let key = dir_key(&a.body.host, a.body.slot);
        if self.hosts.get(&key).map(|r| &r.permit) != Some(&a.permit) {
            return Err(Refuse::NotHost);
        }
        if a.body.removed.contains(me) {
            return Err(Refuse::Removed);
        }
        if !a.body.cites.iter().all(|c| self.ops.contains_key(c)) {
            return Err(Refuse::Unknown);
        }
        let d = ctx.derive(&self.ops, &closure(&self.ops, &a.body.cites));
        if a.body.removed.is_empty() || !a.body.removed.is_subset(&d.removed) {
            return Err(Refuse::NotRemoved);
        }
        if let Some(cur) = self.announced.get(&key) {
            if cur.body.seq >= a.body.seq {
                return Err(Refuse::Stale);
            }
        }
        if ctx.cfg.members == MemberRotation::MembersToo {
            self.member_due = Some(self.member_due.unwrap_or(0).max(a.body.rotate_at));
        }
        self.announced.insert(key, a);
        Ok(())
    }
    // NEW: call request check. A host (`me = None`) relays only roster-to-roster calls; the
    // callee (`me = Some`) verifies again (U10). Refusal of a removed caller is immediate.
    pub(super) fn accept_call(
        &mut self,
        ctx: &Ctx,
        bytes: &[u8],
        me: Option<&str>,
    ) -> Result<(), Refuse> {
        let Ok(c) = serde_json::from_slice::<CallReq>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !c.verify() {
            return Err(Refuse::BadSignature);
        }
        if c.body.group != ctx.gid {
            return Err(Refuse::WrongGroup);
        }
        if self.der.removed.contains(&c.body.caller) {
            return Err(Refuse::Removed);
        }
        if !self.der.roster.contains(&c.body.caller)
            || !self.der.roster.contains(&c.body.callee)
            || me.is_some_and(|me| me != c.body.callee)
        {
            return Err(Refuse::NotMember);
        }
        let id = digest(&c);
        if self.calls.contains(&id) {
            return Err(Refuse::Stale);
        }
        self.calls.push(id);
        Ok(())
    }
}

// ---- devices and world (copied from flags.rs @82d221f, trimmed; schedule is new) -----------

/// NEW (U12): a host's removal-rotation schedule, driven by the host's own clock.
#[derive(Clone, Debug, Default, Serialize)]
pub(super) struct Sched {
    pub(super) learned: BTreeMap<String, u64>, // removed DID -> tick this host first derived the removal
    pub(super) pending: BTreeMap<u64, BTreeSet<String>>, // rotate_at -> DIDs the rotation cuts off
    pub(super) done: Vec<(u64, BTreeSet<String>)>, // performed rotations: (tick, DIDs)
    pub(super) seq: u64,                       // announcement sequence
}
pub(super) struct Device {
    pub(super) id: Identity,
    pub(super) slot: u64, // random device ID / route slot; owner-replaceable, never key-derived
    pub(super) w: Writer,
    pub(super) rep: Replica,
    pub(super) group_key: SigningKey,
    pub(super) my_note: EpNote,
    pub(super) host: Option<HostRecord>,
    pub(super) acks: BTreeMap<String, u64>,
    pub(super) op_out: Vec<SignedOp>,
    pub(super) outbox: Vec<Post>,
    pub(super) refused: Vec<(Post, Why)>,
    pub(super) online: bool,
    pub(super) sched: Option<Sched>,  // hosts only
    pub(super) ann: Option<Announce>, // hosts only: latest announcement
}
