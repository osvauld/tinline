use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SignedOp {
    pub(super) body: OpBody,
    pub(super) sig: Vec<u8>,
}
impl SignedOp {
    pub(super) fn sign(
        id: &Identity,
        group: &str,
        grant: Option<MGrant>,
        nonce: u64,
        seen: BTreeSet<String>,
        op: Op,
    ) -> Self {
        let body = OpBody {
            group: group.into(),
            issuer: id.did().into(),
            issuer_grant: grant,
            nonce,
            seen,
            op,
        };
        let sig = id.sign(&signed(OP_DOMAIN, &body)).to_vec();
        Self { body, sig }
    }
    pub(super) fn verify(&self) -> bool {
        valid(&self.body.issuer, &signed(OP_DOMAIN, &self.body), &self.sig)
    }
    pub(super) fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
    pub(super) fn id(&self) -> String {
        digest(self)
    }
    pub(super) fn flag_at(&self) -> u64 {
        match &self.body.op {
            Op::Flag { at, .. } => *at,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refuse {
    Malformed,
    BadSignature,
    WrongGroup,
    PermitInvalid,
    DeviceMismatch,
    Revoked,
    Stale,
    NotAdmin,
    NotMember,
    NotHost,
    NotFlaggable,
    NotAllowed,
    NotFlagger,
    UnknownFlag,
    AfterThreshold,
    IssuerRemoved,
    Removed,
    GrantRevoked,
    BadLift,
    Limit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Acc {
    Applied,
    Duplicate,
    Pending,
}

// ---- configuration (open policy as parameters; defaults are NOT selections) ----------------

/// Section 8 Q1 (G-AUTHORITY): 3 flags from 3 distinct admin DIDs (lean) or any 3 flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FlagCount {
    DistinctAdmins,
    AnyFlags,
}
/// Section 8 Q2 (G-AUTHORITY): group with fewer than 3 admins (owner counted as an admin).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SmallGroup {
    OwnerAlone,
    ThresholdIsAdminCount,
    NoRemoval,
}
/// Section 8 Q3 (G-AUTHORITY): can the owner/creator be flagged?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OwnerRule {
    Protected,
    Flaggable,
}
/// Section 8 Q9 (G-AUTHORITY): can admins be flagged by other admins?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AdminRule {
    Immune,
    Flaggable,
}
/// Section 8 Q4a (G-AUTHORITY): can a flag be withdrawn before the threshold?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Withdrawal {
    Final,
    BeforeThreshold,
}
/// Section 8 Q4b (G-TIME): do flags expire?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Expiry {
    Never,
    /// Counted flags' signed (claimed) `at` must lie within this many ticks of each other.
    SignedWindow(u64),
    /// A flag counts while receiver_clock < arrival + ttl; removal latches once reached.
    ReceiverClock(u64),
}
/// Section 8 Q8 = D01/D02: posts made before/concurrently with the 3rd flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PostRule {
    /// SF: author is a member at the post's own signed op frontier.
    SignedFrontier,
    /// RW: SF, and a removed author's post is kept only if some flag on the author had
    /// already seen it (`seen_posts`). One sub-option of D01 "removal wins"; not designed.
    RemovalWins,
}
/// Section 8 Q7 (G-AUTHORITY): can a removed DID be invited back?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Rejoin {
    Allowed,
    Forbidden,
}
#[derive(Clone)]
pub(super) struct Cfg {
    pub(super) seed: u64,
    pub(super) hosts: Vec<usize>,
    pub(super) admins: Vec<usize>, // devices whose DID gets an admin grant (owner is always admin)
    pub(super) count: FlagCount,
    pub(super) small: SmallGroup,
    pub(super) owner: OwnerRule,
    pub(super) admin: AdminRule,
    pub(super) withdrawal: Withdrawal,
    pub(super) expiry: Expiry,
    pub(super) post: PostRule,
    pub(super) rejoin: Rejoin,
}
impl Default for Cfg {
    fn default() -> Self {
        Self {
            seed: 0x5EED_00F1,
            hosts: vec![H1, H2],
            admins: vec![A1, BOB, DAVE],
            count: FlagCount::DistinctAdmins,
            small: SmallGroup::ThresholdIsAdminCount,
            owner: OwnerRule::Protected,
            admin: AdminRule::Immune,
            withdrawal: Withdrawal::Final,
            expiry: Expiry::Never,
            post: PostRule::SignedFrontier,
            rejoin: Rejoin::Forbidden,
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
    pub(super) admins: BTreeSet<String>, // fixture admin set (counted for the threshold), incl. owner
    pub(super) base: BTreeSet<String>,   // initial members
    pub(super) grants: BTreeMap<String, MGrant>, // DID -> its bootstrap grant (creator has none)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct Derived {
    pub(super) members: BTreeSet<String>, // base + admitted
    pub(super) removed: BTreeSet<String>,
    pub(super) roster: BTreeSet<String>, // members - removed
    pub(super) revoked_grants: BTreeSet<String>,
    pub(super) flags: BTreeMap<String, BTreeSet<String>>, // target -> effective (counted) flag op ids
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
    pub(super) fn threshold_met(&self, fl: &[&SignedOp]) -> bool {
        let issuers: BTreeSet<&str> = fl.iter().map(|f| f.body.issuer.as_str()).collect();
        let count = match self.cfg.count {
            FlagCount::DistinctAdmins => issuers.len(),
            FlagCount::AnyFlags => fl.len(),
        };
        let n = self.admins.len();
        if n >= FLAG_THRESHOLD {
            return count >= FLAG_THRESHOLD;
        }
        match self.cfg.small {
            SmallGroup::OwnerAlone => issuers.contains(self.creator.as_str()),
            SmallGroup::ThresholdIsAdminCount => n > 0 && count >= n,
            SmallGroup::NoRemoval => false,
        }
    }
    pub(super) fn removal_met(&self, fl: &[&SignedOp]) -> bool {
        match self.cfg.expiry {
            Expiry::SignedWindow(win) => fl.iter().any(|start| {
                let t = start.flag_at();
                let sub: Vec<&SignedOp> = fl
                    .iter()
                    .copied()
                    .filter(|f| f.flag_at() >= t && f.flag_at() <= t.saturating_add(win))
                    .collect();
                self.threshold_met(&sub)
            }),
            _ => self.threshold_met(fl),
        }
    }
    // Membership as a pure function of an op set. `live` = (arrival times, receiver clock),
    // used only by the ReceiverClock candidate on the receiver's whole current state.
    pub(super) fn derive(
        &self,
        ops: &BTreeMap<String, SignedOp>,
        ids: &BTreeSet<String>,
        live: Option<(&BTreeMap<String, u64>, u64)>,
    ) -> Derived {
        let mut withdrawn = BTreeSet::new();
        let mut lifted = BTreeSet::new();
        let mut members = self.base.clone();
        let mut readmitted = BTreeSet::new();
        let mut admit_grants: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for id in ids {
            let Some(o) = ops.get(id) else { continue };
            match &o.body.op {
                Op::Withdraw { flag } => {
                    withdrawn.insert(flag.clone());
                }
                Op::Admit { grant, lifts } => {
                    let h = grant.body.holder.clone();
                    members.insert(h.clone());
                    admit_grants
                        .entry(h.clone())
                        .or_default()
                        .push(grant.digest());
                    if !lifts.is_empty() {
                        readmitted.insert(h);
                        lifted.extend(lifts.iter().cloned());
                    }
                }
                _ => {}
            }
        }
        let mut by_target: BTreeMap<String, Vec<&SignedOp>> = BTreeMap::new();
        let mut flags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for id in ids {
            let Some(o) = ops.get(id) else { continue };
            let Op::Flag { target, .. } = &o.body.op else {
                continue;
            };
            if withdrawn.contains(id) || lifted.contains(id) {
                continue;
            }
            if let (Some((arrived, now)), Expiry::ReceiverClock(ttl)) = (live, self.cfg.expiry) {
                if arrived.get(id).is_none_or(|t| t + ttl <= now) {
                    continue;
                }
            }
            by_target.entry(target.clone()).or_default().push(o);
            flags.entry(target.clone()).or_default().insert(id.clone());
        }
        let removed: BTreeSet<String> = by_target
            .iter()
            .filter(|(_, fl)| self.removal_met(fl))
            .map(|(t, _)| t.clone())
            .collect();
        let mut revoked_grants = BTreeSet::new();
        for h in removed.iter().chain(readmitted.iter()) {
            if let Some(g) = self.grants.get(h) {
                revoked_grants.insert(g.digest());
            }
        }
        for h in &removed {
            for d in admit_grants.get(h).into_iter().flatten() {
                revoked_grants.insert(d.clone());
            }
        }
        let roster = members.difference(&removed).cloned().collect();
        Derived {
            members,
            removed,
            roster,
            revoked_grants,
            flags,
        }
    }
    // Checks that need no group state (signature/group checked by the caller).
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
        match &op.body.op {
            Op::Flag { target, .. } => {
                if *target == self.creator && self.cfg.owner == OwnerRule::Protected {
                    return Err(Refuse::NotFlaggable);
                }
                if *target != self.creator
                    && self.admins.contains(target)
                    && self.cfg.admin == AdminRule::Immune
                {
                    return Err(Refuse::NotFlaggable);
                }
            }
            Op::Withdraw { .. } => {
                if self.cfg.withdrawal == Withdrawal::Final {
                    return Err(Refuse::NotAllowed);
                }
            }
            Op::Admit { grant, lifts } => {
                if !grant.allows(&self.g, &grant.body.holder, Cap::Post) {
                    return Err(Refuse::PermitInvalid);
                }
                if !lifts.is_empty() && self.cfg.rejoin == Rejoin::Forbidden {
                    return Err(Refuse::NotAllowed);
                }
                if !lifts.is_subset(&op.body.seen) {
                    return Err(Refuse::BadLift);
                }
            }
            Op::RemoveHost { .. } => {}
        }
        Ok(())
    }
}
