//! G-B: candidate causal-policy oracles for gates D01–D05. Status: proposal — not approved.
//!
//! NOTHING here selects a policy. Each `candidate-policy D0x` test encodes ONE option of an
//! open gate; a `limitation` test passes by demonstrating what an option cannot achieve.
//! See `docs/group-policy-options.md` for the comparison table and the open questions.
//!
//! Model (test-only, not a wire format):
//! - Membership ops (`MetaOp`) are real DID-signed records (Ed25519 via `identity`) forming a
//!   causal DAG: `parents` = the meta frontier the issuer had seen, `seen_posts` = the content
//!   the issuer had accepted, `at` = the issuer's CLAIMED clock (only timestamp candidates read it).
//! - Posts are the existing real device-signed `Envelope`s (real Loro updates). The existing
//!   `Body` has no `meta_frontier` field, so a provisional policy `Header` (frontier, claimed
//!   time, epoch) is carried as JSON in the signed `text`. Production would carry it in
//!   `GroupUpdate.meta_frontier` (docs/group-chat.md §5); either way it is signer-chosen.
//! - `Node` admits ops/posts (signature + reference checks; pending while references are
//!   missing) and evaluates a `PostRule` + `Membership` rule over what it holds. The content
//!   layer is then materialized by feeding policy-accepted envelopes, in causal order, into the
//!   existing `Replica` (unchanged code), so Loro/proof checks and pending dependencies are real.
//! - Fixture simplifications: the genesis creator is always admin and cannot be removed;
//!   unauthorized ops are retained but ineffective (their effect can depend on later knowledge
//!   under some candidates); no pending bound on the policy layer.
use super::*;
use identity::{public_key_from_did, verify};
use std::cell::RefCell;

const META_DOMAIN: &[u8] = b"tinline/group-sim/meta/not-production/v1\0";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum Change {
    Add {
        member: String,
        admin: bool,
        expires_at: Option<u64>,
    },
    Remove {
        member: String,
    },
    // Epoch-cut candidate only: complete member/admin lists for the new epoch plus the IDs of
    // previous-epoch posts the issuer closes into history (a production cut would be a VV).
    Epoch {
        number: u64,
        members: BTreeSet<String>,
        admins: BTreeSet<String>,
        cut: BTreeSet<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct MetaBody {
    group: String,
    issuer: String,
    parents: BTreeSet<String>,
    seen_posts: BTreeSet<String>,
    at: u64,
    change: Change,
}
#[derive(Clone, Debug, PartialEq)]
struct MetaOp {
    body: MetaBody,
    signature: Vec<u8>,
}
impl MetaOp {
    fn bytes(body: &MetaBody) -> Vec<u8> {
        [META_DOMAIN, &serde_json::to_vec(body).unwrap()].concat()
    }
    fn issue(
        issuer: &Identity,
        group: &str,
        parents: &[&MetaOp],
        seen: &[&Envelope],
        at: u64,
        change: Change,
    ) -> Self {
        let body = MetaBody {
            group: group.into(),
            issuer: issuer.did().into(),
            parents: parents.iter().map(|p| p.id()).collect(),
            seen_posts: seen.iter().map(|e| e.id()).collect(),
            at,
            change,
        };
        let signature = issuer.sign(&Self::bytes(&body)).to_vec();
        Self { body, signature }
    }
    fn id(&self) -> String {
        blake3::hash(&Self::bytes(&self.body)).to_hex().to_string()
    }
    fn verify(&self, group: &str) -> bool {
        let Some(key) = public_key_from_did(&self.body.issuer) else {
            return false;
        };
        let Ok(sig) = <&[u8; 64]>::try_from(self.signature.as_slice()) else {
            return false;
        };
        self.body.group == group && verify(&key, &Self::bytes(&self.body), sig)
    }
    fn target(&self) -> Option<&str> {
        match &self.body.change {
            Change::Add { member, .. } | Change::Remove { member } => Some(member),
            Change::Epoch { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Header {
    frontier: BTreeSet<String>,
    at: u64,
    epoch: Option<String>,
    msg: String,
}
fn header_text(frontier: &[&MetaOp], at: u64, epoch: Option<&MetaOp>, msg: &str) -> String {
    serde_json::to_string(&Header {
        frontier: frontier.iter().map(|m| m.id()).collect(),
        at,
        epoch: epoch.map(|e| e.id()),
        msg: msg.into(),
    })
    .unwrap()
}
fn header(e: &Envelope) -> Option<Header> {
    serde_json::from_str(&e.body.text).ok()
}
fn post(
    a: &mut Author,
    frontier: &[&MetaOp],
    at: u64,
    epoch: Option<&MetaOp>,
    msg: &str,
) -> Envelope {
    a.post(&header_text(frontier, at, epoch, msg))
}
// A fresh device chain of the same DID (reuses G-H's `second_device_envelope`).
fn second_device_post(
    owner: &Author,
    seed: u8,
    frontier: &[&MetaOp],
    at: u64,
    epoch: Option<&MetaOp>,
    msg: &str,
) -> Envelope {
    let key = SigningKey::from_bytes(&[seed; 32]);
    let writer = peer(&key.verifying_key().to_bytes(), &owner.doc_name);
    second_device_envelope(owner, &key, writer, &header_text(frontier, at, epoch, msg))
}

#[derive(Clone, Debug, PartialEq, Default)]
struct Members(BTreeMap<String, (bool, Option<u64>)>);
impl Members {
    fn genesis(creator: &str) -> Self {
        Self([(creator.to_string(), (true, None))].into())
    }
    fn has(&self, did: &str) -> bool {
        self.0.contains_key(did)
    }
    fn is_admin(&self, did: &str) -> bool {
        self.0.get(did).is_some_and(|m| m.0)
    }
    fn expiry(&self, did: &str) -> Option<Option<u64>> {
        self.0.get(did).map(|m| m.1)
    }
    fn apply(&mut self, op: &MetaOp, creator: &str) {
        match &op.body.change {
            Change::Add {
                member,
                admin,
                expires_at,
            } => {
                self.0.insert(member.clone(), (*admin, *expires_at));
            }
            Change::Remove { member } if member != creator => {
                self.0.remove(member);
            }
            _ => {}
        }
    }
}
fn authorized(op: &MetaOp, s: &Members, creator: &str) -> bool {
    match &op.body.change {
        Change::Add { .. } => s.is_admin(&op.body.issuer),
        Change::Remove { member } => s.is_admin(&op.body.issuer) && member != creator,
        Change::Epoch { .. } => false,
    }
}

/// How concurrent membership ops on the same member resolve (D03).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Resolve {
    /// Maximal concurrent Add vs Remove on a member: removed. Each op is authorized at its
    /// OWN signed frontier.
    RemoveWins,
    /// Maximal concurrent Add vs Remove: present (OR-set style). Authorized at own frontier.
    AddWins,
    /// Deterministic fold: topological order, ready ops taken junior-first (issuer power
    /// ascending: member 0, admin 1, creator 2; ties by op ID) so seniors act last; each op is
    /// authorized against the FOLDED state at that point, not its own frontier.
    Priority,
}
#[derive(Clone, Copy, Debug)]
struct Membership {
    resolve: Resolve,
    /// Drop Adds whose issuer is not an admin in the evaluated state (fixpoint).
    cascade: bool,
}
const REMOVE_WINS: Membership = Membership {
    resolve: Resolve::RemoveWins,
    cascade: false,
};

/// How a post's validity is decided (D01, D02, D04, D05).
#[derive(Clone, Copy, Debug, PartialEq)]
enum PostRule {
    /// Author is a member at the post's own signed meta frontier.
    SignedFrontier,
    /// Decided ONCE, when the post's references are present, against the receiver's whole
    /// current membership view; never revisited.
    ArrivalView,
    /// SignedFrontier AND no authorized removal of the author that is concurrent with the post
    /// (removal not in the post's frontier, post not in the remover's seen content).
    RemovalWinsConcurrent,
    /// Membership linearized by issuer-claimed `at`; post valid iff member at its claimed `at`.
    AuthorTimestamp,
    /// Decided ONCE at arrival: member at frontier AND receiver's local clock < grant expiry.
    LocalClockExpiry,
    /// Member at frontier AND author-claimed `at` < grant expiry.
    TimestampExpiry,
    /// Post names an epoch on the winning epoch chain whose member list includes the author,
    /// and either that epoch is the tip or the next winning epoch's cut names the post.
    EpochCut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Accepted,
    Rejected,
    Pending,
}
#[derive(Debug, PartialEq)]
enum Admit {
    Stored,
    Pending,
    Duplicate,
    Invalid,
}
#[derive(Clone)]
enum Ev {
    Meta(MetaOp),
    Post(Envelope),
}

// Speed-up for the all-orders loops: remember only POSITIVE signature checks, keyed by the
// exact signed bytes + signature + expected group.
fn verify_cached(op: &MetaOp, group: &str) -> bool {
    thread_local! {
        static VALID: RefCell<BTreeSet<Vec<u8>>> = const { RefCell::new(BTreeSet::new()) };
    }
    let key = [
        MetaOp::bytes(&op.body),
        op.signature.clone(),
        group.as_bytes().to_vec(),
    ]
    .concat();
    if VALID.with(|v| v.borrow().contains(&key)) {
        return true;
    }
    let ok = op.verify(group);
    if ok {
        VALID.with(|v| v.borrow_mut().insert(key));
    }
    ok
}

struct Node {
    group: String,
    creator: String,
    dids: BTreeSet<String>,
    rule: PostRule,
    membership: Membership,
    clock: u64,
    meta: BTreeMap<String, MetaOp>,
    meta_pending: BTreeMap<String, MetaOp>,
    posts: BTreeMap<String, Envelope>,
    posts_pending: BTreeMap<String, Envelope>,
    // ArrivalView / LocalClockExpiry decisions, fixed once made.
    arrival: BTreeMap<String, bool>,
    // Authorization at an op's own frontier never changes once the op is admitted.
    auth_cache: RefCell<BTreeMap<String, bool>>,
    // A frontier's closure is fixed once admitted, so its evaluated state never changes.
    state_cache: RefCell<BTreeMap<BTreeSet<String>, Members>>,
}
impl Node {
    fn new(w: &World, rule: PostRule, membership: Membership, clock: u64) -> Self {
        assert!(w.genesis.verify(&w.gid));
        Self {
            group: w.gid.clone(),
            creator: w.genesis.body.creator.clone(),
            dids: w.dids(),
            rule,
            membership,
            clock,
            meta: BTreeMap::new(),
            meta_pending: BTreeMap::new(),
            posts: BTreeMap::new(),
            posts_pending: BTreeMap::new(),
            arrival: BTreeMap::new(),
            auth_cache: RefCell::new(BTreeMap::new()),
            state_cache: RefCell::new(BTreeMap::new()),
        }
    }
    // Speed-up for the all-orders loops: remember only POSITIVE results, keyed by the exact
    // signed bytes + signature + attestation (the envelope ID alone excludes the signature).
    fn valid_proof_cached(&self, e: &Envelope) -> bool {
        thread_local! {
            static VALID: RefCell<BTreeSet<Vec<u8>>> = const { RefCell::new(BTreeSet::new()) };
        }
        let key = [
            e.bytes(),
            e.signature.to_vec(),
            serde_json::to_vec(&e.attestation).unwrap(),
            self.group.as_bytes().to_vec(),
        ]
        .concat();
        if VALID.with(|v| v.borrow().contains(&key)) {
            return true;
        }
        let ok = self.content_replica().valid_proof(e);
        if ok {
            VALID.with(|v| v.borrow_mut().insert(key));
        }
        ok
    }
    fn content_replica(&self) -> Replica {
        let mut r = Replica::new(&self.group, &[]);
        r.authorization = Box::new(FixtureAllowlist(self.dids.clone()));
        r
    }
    fn deliver(&mut self, ev: &Ev) -> Admit {
        let id = match ev {
            Ev::Meta(op) => {
                if !verify_cached(op, &self.group) {
                    return Admit::Invalid;
                }
                let id = op.id();
                if self.meta.contains_key(&id) {
                    return Admit::Duplicate;
                }
                if self.meta_pending.contains_key(&id) {
                    return Admit::Pending;
                }
                self.meta_pending.insert(id.clone(), op.clone());
                id
            }
            Ev::Post(e) => {
                if header(e).is_none() || e.body.group != self.group || !self.valid_proof_cached(e)
                {
                    return Admit::Invalid;
                }
                let id = e.id();
                if self.posts.contains_key(&id) {
                    return Admit::Duplicate;
                }
                if self.posts_pending.contains_key(&id) {
                    return Admit::Pending;
                }
                self.posts_pending.insert(id.clone(), e.clone());
                id
            }
        };
        self.drain();
        if self.meta.contains_key(&id) || self.posts.contains_key(&id) {
            Admit::Stored
        } else {
            Admit::Pending
        }
    }
    fn refs_ready(&self, e: &Envelope) -> bool {
        let h = header(e).unwrap();
        h.frontier.iter().all(|f| self.meta.contains_key(f))
            && h.epoch.iter().all(|x| self.meta.contains_key(x))
    }
    fn drain(&mut self) {
        loop {
            let ready = self
                .meta_pending
                .iter()
                .find(|(_, op)| op.body.parents.iter().all(|p| self.meta.contains_key(p)))
                .map(|(id, _)| id.clone());
            if let Some(id) = ready {
                let op = self.meta_pending.remove(&id).unwrap();
                self.meta.insert(id, op);
                continue;
            }
            let ready = self
                .posts_pending
                .iter()
                .find(|(_, e)| self.refs_ready(e))
                .map(|(id, _)| id.clone());
            let Some(id) = ready else {
                break;
            };
            let e = self.posts_pending.remove(&id).unwrap();
            if let Some(decision) = self.arrival_decision(&e) {
                self.arrival.insert(id.clone(), decision);
            }
            self.posts.insert(id, e);
        }
    }
    fn arrival_decision(&self, e: &Envelope) -> Option<bool> {
        let author = &e.body.author;
        match self.rule {
            PostRule::ArrivalView => Some(self.current().has(author)),
            PostRule::LocalClockExpiry => {
                let h = header(e).unwrap();
                Some(
                    self.state(&h.frontier)
                        .expiry(author)
                        .is_some_and(|x| x.is_none_or(|x| self.clock < x)),
                )
            }
            _ => None,
        }
    }

    // ---- membership evaluation -------------------------------------------------------
    fn closure(&self, frontier: &BTreeSet<String>) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut todo: Vec<String> = frontier.iter().cloned().collect();
        while let Some(id) = todo.pop() {
            if out.insert(id.clone()) {
                todo.extend(self.meta[&id].body.parents.iter().cloned());
            }
        }
        out
    }
    fn auth_own(&self, id: &str) -> bool {
        let cached = self.auth_cache.borrow().get(id).copied();
        if let Some(v) = cached {
            return v;
        }
        let op = &self.meta[id];
        let v = authorized(op, &self.state(&op.body.parents), &self.creator);
        self.auth_cache.borrow_mut().insert(id.into(), v);
        v
    }
    fn set_fold(&self, ids: &BTreeSet<String>, excluded: &BTreeSet<String>) -> Members {
        let eff: Vec<&String> = ids
            .iter()
            .filter(|id| {
                !excluded.contains(*id) && self.meta[*id].target().is_some() && self.auth_own(id)
            })
            .collect();
        let mut s = Members::genesis(&self.creator);
        let targets: BTreeSet<&str> = eff
            .iter()
            .filter_map(|id| self.meta[*id].target())
            .collect();
        for m in targets {
            if m == self.creator {
                continue;
            }
            let ops: Vec<(&String, BTreeSet<String>)> = eff
                .iter()
                .filter(|id| self.meta[**id].target() == Some(m))
                .map(|id| (*id, self.closure(&self.meta[*id].body.parents)))
                .collect();
            let maximal: Vec<&MetaOp> = ops
                .iter()
                .filter(|(o, _)| !ops.iter().any(|(_, past)| past.contains(*o)))
                .map(|(o, _)| &self.meta[*o])
                .collect();
            let adds: Vec<(bool, Option<u64>)> = maximal
                .iter()
                .filter_map(|op| match &op.body.change {
                    Change::Add {
                        admin, expires_at, ..
                    } => Some((*admin, *expires_at)),
                    _ => None,
                })
                .collect();
            let removed = maximal.len() > adds.len();
            let present = match self.membership.resolve {
                Resolve::RemoveWins => !removed && !adds.is_empty(),
                Resolve::AddWins => !adds.is_empty(),
                Resolve::Priority => unreachable!(),
            };
            if present {
                let admin = adds.iter().any(|a| a.0);
                let expiry = if adds.iter().any(|a| a.1.is_none()) {
                    None
                } else {
                    adds.iter().filter_map(|a| a.1).max()
                };
                s.0.insert(m.to_string(), (admin, expiry));
            }
        }
        s
    }
    fn power(&self, s: &Members, did: &str) -> u8 {
        if did == self.creator {
            2
        } else {
            s.is_admin(did) as u8
        }
    }
    fn priority_fold(&self, ids: &BTreeSet<String>, excluded: &BTreeSet<String>) -> Members {
        let mut s = Members::genesis(&self.creator);
        let mut done = BTreeSet::new();
        loop {
            let next = ids
                .iter()
                .filter(|id| {
                    !done.contains(*id)
                        && self.meta[*id].body.parents.iter().all(|p| done.contains(p))
                })
                .min_by_key(|id| (self.power(&s, &self.meta[*id].body.issuer), (*id).clone()))
                .cloned();
            let Some(id) = next else {
                break;
            };
            done.insert(id.clone());
            let op = &self.meta[&id];
            if !excluded.contains(&id) && authorized(op, &s, &self.creator) {
                s.apply(op, &self.creator);
            }
        }
        s
    }
    fn state(&self, frontier: &BTreeSet<String>) -> Members {
        let cached = self.state_cache.borrow().get(frontier).cloned();
        if let Some(s) = cached {
            return s;
        }
        let s = self.state_uncached(frontier);
        self.state_cache
            .borrow_mut()
            .insert(frontier.clone(), s.clone());
        s
    }
    fn state_uncached(&self, frontier: &BTreeSet<String>) -> Members {
        let ids = self.closure(frontier);
        let mut excluded = BTreeSet::new();
        loop {
            let s = match self.membership.resolve {
                Resolve::Priority => self.priority_fold(&ids, &excluded),
                _ => self.set_fold(&ids, &excluded),
            };
            if !self.membership.cascade {
                return s;
            }
            let orphans: Vec<String> = ids
                .iter()
                .filter(|id| {
                    let op = &self.meta[*id];
                    !excluded.contains(*id)
                        && matches!(op.body.change, Change::Add { .. })
                        && !s.is_admin(&op.body.issuer)
                })
                .cloned()
                .collect();
            if orphans.is_empty() {
                return s;
            }
            excluded.extend(orphans);
        }
    }
    fn current(&self) -> Members {
        self.state(&self.meta.keys().cloned().collect())
    }
    fn state_by_time(&self, t: u64) -> Members {
        let mut ops: Vec<(u64, &String)> = self
            .meta
            .iter()
            .filter(|(_, op)| op.body.at <= t)
            .map(|(id, op)| (op.body.at, id))
            .collect();
        ops.sort();
        let mut s = Members::genesis(&self.creator);
        for (_, id) in ops {
            let op = &self.meta[id];
            if authorized(op, &s, &self.creator) {
                s.apply(op, &self.creator);
            }
        }
        s
    }
    // Content causal past of the given posts (via signed previous/dependencies).
    fn post_past(&self, seen: &BTreeSet<String>) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut todo: Vec<String> = seen.iter().cloned().collect();
        while let Some(id) = todo.pop() {
            if !out.insert(id.clone()) {
                continue;
            }
            if let Some(e) = self.posts.get(&id).or(self.posts_pending.get(&id)) {
                todo.extend(
                    e.body
                        .previous
                        .iter()
                        .chain(e.body.dependencies.iter())
                        .cloned(),
                );
            }
        }
        out
    }
    fn concurrent_removal(&self, e: &Envelope, h: &Header) -> bool {
        let past = self.closure(&h.frontier);
        let id = e.id();
        self.meta.iter().any(|(rid, r)| {
            matches!(&r.body.change, Change::Remove { member } if *member == e.body.author)
                && !past.contains(rid)
                && self.auth_own(rid)
                && !self.post_past(&r.body.seen_posts).contains(&id)
        })
    }
    fn epoch_parent(&self, op: &MetaOp) -> Option<String> {
        let Change::Epoch { number, .. } = &op.body.change else {
            return None;
        };
        let parents: Vec<&String> = op
            .body
            .parents
            .iter()
            .filter(|p| matches!(&self.meta[*p].body.change, Change::Epoch { number: n, .. } if *n + 1 == *number))
            .collect();
        (parents.len() == 1).then(|| parents[0].clone())
    }
    fn epoch_valid(&self, id: &str) -> bool {
        let op = &self.meta[id];
        let Change::Epoch { number, .. } = &op.body.change else {
            return false;
        };
        if *number == 1 {
            return op.body.issuer == self.creator;
        }
        self.epoch_parent(op).is_some_and(|p| {
            matches!(&self.meta[&p].body.change, Change::Epoch { admins, .. } if admins.contains(&op.body.issuer))
                && self.epoch_valid(&p)
        })
    }
    // Winning chain: from epoch 1, repeatedly the valid child with the LOWEST op ID.
    fn epoch_chain(&self) -> Vec<String> {
        let mut chain: Vec<String> = vec![];
        loop {
            let next = self
                .meta
                .iter()
                .filter(|(id, op)| {
                    matches!(&op.body.change, Change::Epoch { number, .. }
                    if match chain.last() {
                        None => *number == 1,
                        Some(p) => self.epoch_parent(op).as_ref() == Some(p),
                    }) && self.epoch_valid(id)
                })
                .map(|(id, _)| id.clone())
                .min();
            match next {
                Some(id) => chain.push(id),
                None => return chain,
            }
        }
    }
    fn epoch_ok(&self, e: &Envelope, h: &Header) -> bool {
        let Some(eid) = &h.epoch else {
            return false;
        };
        let chain = self.epoch_chain();
        let Some(i) = chain.iter().position(|x| x == eid) else {
            return false;
        };
        let Change::Epoch { members, .. } = &self.meta[eid].body.change else {
            return false;
        };
        members.contains(&e.body.author)
            && (i + 1 == chain.len()
                || matches!(&self.meta[&chain[i + 1]].body.change,
                    Change::Epoch { cut, .. } if cut.contains(&e.id())))
    }
    fn accepted_by_rule(&self, e: &Envelope) -> bool {
        let h = header(e).unwrap();
        let author = &e.body.author;
        match self.rule {
            PostRule::SignedFrontier => self.state(&h.frontier).has(author),
            PostRule::ArrivalView | PostRule::LocalClockExpiry => self.arrival[&e.id()],
            PostRule::RemovalWinsConcurrent => {
                self.state(&h.frontier).has(author) && !self.concurrent_removal(e, &h)
            }
            PostRule::AuthorTimestamp => self.state_by_time(h.at).has(author),
            PostRule::TimestampExpiry => self
                .state(&h.frontier)
                .expiry(author)
                .is_some_and(|x| x.is_none_or(|x| h.at < x)),
            PostRule::EpochCut => self.epoch_ok(e, &h),
        }
    }
    fn verdicts(&self) -> BTreeMap<String, Verdict> {
        let mut v: BTreeMap<String, Verdict> = self
            .posts
            .iter()
            .map(|(id, e)| {
                let ok = self.accepted_by_rule(e);
                (
                    id.clone(),
                    if ok {
                        Verdict::Accepted
                    } else {
                        Verdict::Rejected
                    },
                )
            })
            .collect();
        v.extend(
            self.posts_pending
                .keys()
                .map(|id| (id.clone(), Verdict::Pending)),
        );
        v
    }
    // Feed policy-accepted envelopes, in causal order, into the existing content Replica.
    fn materialize(&self) -> Replica {
        let v = self.verdicts();
        let mut r = self.content_replica();
        let accepted: Vec<Envelope> = self
            .posts
            .iter()
            .filter(|(id, _)| v[*id] == Verdict::Accepted)
            .map(|(_, e)| e.clone())
            .collect();
        for e in causal_order(accepted) {
            let out = r.receive(e);
            assert!(
                matches!(out, Outcome::Accepted | Outcome::Pending),
                "{out:?}"
            );
        }
        r
    }
    fn finalize(&self) -> Final {
        let verdicts = self.verdicts();
        let r = self.materialize();
        // Invariant (I3/I4 across layers): content never holds a policy-rejected post, and a
        // policy-accepted post is held pending only for a missing (withheld) dependency.
        for (id, e) in &r.accepted {
            assert_eq!(verdicts[id], Verdict::Accepted);
            assert!(r.dependencies_ready(e));
        }
        for (id, e) in &r.pending {
            assert_eq!(verdicts[id], Verdict::Accepted);
            assert!(!r.dependencies_ready(e));
        }
        Final {
            verdicts,
            members: self.current(),
            accepted: r.accepted.keys().cloned().collect(),
            pending: r.pending.keys().cloned().collect(),
            value: format!("{:?}", r.doc.get_deep_value()),
        }
    }
    fn verdict(&self, e: &Envelope) -> Option<Verdict> {
        self.verdicts().get(&e.id()).copied()
    }
    fn snapshot(&self) -> Vec<u8> {
        let metas = |m: &BTreeMap<String, MetaOp>| {
            m.iter()
                .map(|(id, op)| (id.clone(), MetaOp::bytes(&op.body), op.signature.clone()))
                .collect::<Vec<_>>()
        };
        let posts = |m: &BTreeMap<String, Envelope>| {
            m.iter()
                .map(|(id, e)| (id.clone(), e.bytes(), e.signature.to_vec()))
                .collect::<Vec<_>>()
        };
        serde_json::to_vec(&(
            metas(&self.meta),
            metas(&self.meta_pending),
            posts(&self.posts),
            posts(&self.posts_pending),
            &self.arrival,
            self.clock,
        ))
        .unwrap()
    }
}

#[derive(Debug, PartialEq)]
struct Final {
    verdicts: BTreeMap<String, Verdict>,
    members: Members,
    accepted: Vec<String>,
    pending: Vec<String>,
    value: String,
}
impl Final {
    fn of(&self, e: &Envelope) -> Verdict {
        self.verdicts[&e.id()]
    }
}
struct Report {
    finals: Vec<Final>,
    orders: usize,
    // Some order showed a post Accepted at an intermediate step but not in the final state.
    retroactive: bool,
}
impl Report {
    fn single(&self) -> &Final {
        assert_eq!(
            self.finals.len(),
            1,
            "order-dependent outcome: {:?}",
            self.finals.iter().map(|f| &f.verdicts).collect::<Vec<_>>()
        );
        &self.finals[0]
    }
}

struct World {
    genesis: Genesis,
    gid: String,
    a: Author,
    b: Author,
    c: Author,
    d: Author,
}
impl World {
    fn new() -> Self {
        let creator = Author::new(1, "bootstrap");
        let genesis = Genesis::create(&creator.id, [77; 32]);
        let gid = genesis.id();
        Self {
            a: Author::new(1, &gid),
            b: Author::new(2, &gid),
            c: Author::new(3, &gid),
            d: Author::new(4, &gid),
            genesis,
            gid,
        }
    }
    fn dids(&self) -> BTreeSet<String> {
        [&self.a, &self.b, &self.c, &self.d]
            .iter()
            .map(|x| x.id.did().to_string())
            .collect()
    }
    fn op(
        &self,
        issuer: &Author,
        parents: &[&MetaOp],
        seen: &[&Envelope],
        at: u64,
        change: Change,
    ) -> MetaOp {
        MetaOp::issue(&issuer.id, &self.gid, parents, seen, at, change)
    }
}
fn did(a: &Author) -> String {
    a.id.did().to_string()
}
fn add(m: &Author, admin: bool, expires_at: Option<u64>) -> Change {
    Change::Add {
        member: did(m),
        admin,
        expires_at,
    }
}
fn remove(m: &Author) -> Change {
    Change::Remove { member: did(m) }
}
fn epoch(number: u64, members: &[&Author], admins: &[&Author], cut: &[&Envelope]) -> Change {
    Change::Epoch {
        number,
        members: members.iter().map(|m| did(m)).collect(),
        admins: admins.iter().map(|m| did(m)).collect(),
        cut: cut.iter().map(|e| e.id()).collect(),
    }
}
fn m(op: &MetaOp) -> Ev {
    Ev::Meta(op.clone())
}
fn p(e: &Envelope) -> Ev {
    Ev::Post(e.clone())
}

fn permutations(n: usize) -> Vec<Vec<usize>> {
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
// Deliver `events` to a fresh node in EVERY order; collect distinct final outcomes.
fn explore(w: &World, rule: PostRule, mem: Membership, clock: u64, events: &[Ev]) -> Report {
    let mut seen: Vec<(BTreeMap<String, Verdict>, Members, Vec<usize>)> = vec![];
    let mut retroactive = false;
    let orders = permutations(events.len());
    for order in &orders {
        let mut n = Node::new(w, rule, mem, clock);
        let mut ever = BTreeSet::new();
        for &i in order {
            let out = n.deliver(&events[i]);
            assert!(
                matches!(out, Admit::Stored | Admit::Pending),
                "{order:?}: event {i} gave {out:?}"
            );
            ever.extend(
                n.verdicts()
                    .into_iter()
                    .filter(|(_, v)| *v == Verdict::Accepted)
                    .map(|(id, _)| id),
            );
        }
        assert!(n.meta_pending.is_empty() && n.posts_pending.is_empty());
        let v = n.verdicts();
        retroactive |= ever.iter().any(|id| v[id] != Verdict::Accepted);
        let members = n.current();
        if !seen.iter().any(|(sv, sm, _)| *sv == v && *sm == members) {
            seen.push((v, members, order.clone()));
        }
    }
    let finals = seen
        .into_iter()
        .map(|(_, _, order)| {
            let mut n = Node::new(w, rule, mem, clock);
            for i in order {
                n.deliver(&events[i]);
            }
            n.finalize()
        })
        .collect();
    Report {
        finals,
        orders: orders.len(),
        retroactive,
    }
}
fn side(w: &World, rule: PostRule, mem: Membership, clock: u64, evs: &[&Ev]) -> Node {
    let mut n = Node::new(w, rule, mem, clock);
    for e in evs {
        n.deliver(e);
    }
    n
}
fn heal(n: &mut Node, all: &[Ev]) {
    for e in all {
        assert_ne!(n.deliver(e), Admit::Invalid);
    }
}

// ---- D01: revocation vs truly concurrent post -------------------------------------------

struct D01 {
    add_b: MetaOp,
    rm_b: MetaOp,
    q: Envelope,
    p: Envelope,
    r: Envelope,
}
impl D01 {
    // A adds B; B posts q (A sees it); A removes B (seen q); concurrently B, offline, posts p
    // at the pre-removal frontier; after learning of removal B posts r at {rm_b}.
    fn new(w: &mut World) -> Self {
        let add_b = w.op(&w.a, &[], &[], 1, add(&w.b, false, None));
        let q = post(&mut w.b, &[&add_b], 2, None, "q seen by A");
        let rm_b = w.op(&w.a, &[&add_b], &[&q], 5, remove(&w.b));
        let p = post(&mut w.b, &[&add_b], 4, None, "p concurrent with removal");
        let r = post(&mut w.b, &[&rm_b], 6, None, "r after learning removal");
        Self {
            add_b,
            rm_b,
            q,
            p,
            r,
        }
    }
    fn events(&self) -> Vec<Ev> {
        vec![
            m(&self.add_b),
            p(&self.q),
            m(&self.rm_b),
            p(&self.p),
            p(&self.r),
        ]
    }
    // Index sets: A's side holds the removal; B's side holds the concurrent post.
    fn sides(&self) -> ([Ev; 3], [Ev; 3]) {
        (
            [m(&self.add_b), p(&self.q), m(&self.rm_b)],
            [m(&self.add_b), p(&self.q), p(&self.p)],
        )
    }
}

// KIND: candidate-policy D01
#[test]
fn d01_signed_frontier_keeps_concurrent_post_in_every_order() {
    let mut w = World::new();
    let t = D01::new(&mut w);
    let rule = PostRule::SignedFrontier;
    let rep = explore(&w, rule, REMOVE_WINS, 0, &t.events());
    assert_eq!(rep.orders, 120);
    let f = rep.single();
    assert_eq!(
        (f.of(&t.q), f.of(&t.p), f.of(&t.r)),
        (Verdict::Accepted, Verdict::Accepted, Verdict::Rejected)
    );
    assert_eq!((f.accepted.len(), f.pending.len()), (2, 0));
    assert!(!f.members.has(&did(&w.b)));
    // Monotone: once a post's frontier is present its verdict never changes.
    assert!(!rep.retroactive);
    // Partition: B's side accepts p before heal; A's side has not seen it; both keep it.
    let (left, right) = t.sides();
    let mut l = side(&w, rule, REMOVE_WINS, 0, &left.iter().collect::<Vec<_>>());
    let mut r = side(&w, rule, REMOVE_WINS, 0, &right.iter().collect::<Vec<_>>());
    assert_eq!(
        (l.verdict(&t.p), r.verdict(&t.p)),
        (None, Some(Verdict::Accepted))
    );
    heal(&mut l, &t.events());
    heal(&mut r, &t.events());
    assert_eq!(l.finalize(), *f);
    assert_eq!(r.finalize(), *f);
    // Negative control: without add_b every post stays pending; nothing is accepted.
    let evs = t.events();
    let n = side(
        &w,
        rule,
        REMOVE_WINS,
        0,
        &evs[1..].iter().collect::<Vec<_>>(),
    );
    assert_eq!(n.verdicts().len(), 3);
    assert!(n.verdicts().values().all(|v| *v == Verdict::Pending));
    assert!(n.materialize().accepted.is_empty());
}

// KIND: candidate-policy D01
#[test]
fn d01_arrival_view_outcome_depends_on_delivery_order_and_never_reconverges() {
    let mut w = World::new();
    let t = D01::new(&mut w);
    let rule = PostRule::ArrivalView;
    let rep = explore(&w, rule, REMOVE_WINS, 0, &t.events());
    // Not order-independent: p (and even q) flip with arrival order. Violates I5.
    assert!(rep.finals.len() >= 2);
    assert!(rep.finals.iter().any(|f| f.of(&t.p) == Verdict::Accepted));
    assert!(rep.finals.iter().any(|f| f.of(&t.p) == Verdict::Rejected));
    assert!(rep.finals.iter().any(|f| f.of(&t.q) == Verdict::Rejected));
    assert!(rep.finals.iter().all(|f| f.of(&t.r) == Verdict::Rejected));
    // Partition: decisions are final, so the two sides disagree permanently after heal.
    let (left, right) = t.sides();
    let mut l = side(&w, rule, REMOVE_WINS, 0, &left.iter().collect::<Vec<_>>());
    let mut r = side(&w, rule, REMOVE_WINS, 0, &right.iter().collect::<Vec<_>>());
    heal(&mut l, &t.events());
    heal(&mut r, &t.events());
    assert_eq!(l.verdict(&t.p), Some(Verdict::Rejected));
    assert_eq!(r.verdict(&t.p), Some(Verdict::Accepted));
    assert_ne!(l.finalize().value, r.finalize().value);
}

// KIND: candidate-policy D01
#[test]
fn d01_removal_wins_drops_concurrent_post_in_every_order_retroactively() {
    let mut w = World::new();
    let t = D01::new(&mut w);
    let rule = PostRule::RemovalWinsConcurrent;
    let rep = explore(&w, rule, REMOVE_WINS, 0, &t.events());
    let f = rep.single();
    // q survives only because the remover signed it into `seen_posts`.
    assert_eq!(
        (f.of(&t.q), f.of(&t.p), f.of(&t.r)),
        (Verdict::Accepted, Verdict::Rejected, Verdict::Rejected)
    );
    assert_eq!(f.accepted.len(), 1);
    // Non-monotone: some orders show p accepted before rm_b arrives.
    assert!(rep.retroactive);
    // Partition: B's side shows p until heal, then drops it.
    let (left, right) = t.sides();
    let l = side(&w, rule, REMOVE_WINS, 0, &left.iter().collect::<Vec<_>>());
    let mut r = side(&w, rule, REMOVE_WINS, 0, &right.iter().collect::<Vec<_>>());
    assert_eq!(r.verdict(&t.p), Some(Verdict::Accepted));
    assert_eq!(l.verdict(&t.p), None);
    heal(&mut r, &t.events());
    assert_eq!(r.verdict(&t.p), Some(Verdict::Rejected));
    assert_eq!(r.finalize(), *f);
}

// ---- D02: stale frontier / backdating by a removed member ----------------------------

struct D02 {
    add_b: MetaOp,
    rm_b: MetaOp,
    q: Envelope,
    s: Envelope,
    r: Envelope,
}
impl D02 {
    // B is removed at claimed time 10 (A saw q). B has RECEIVED the removal, then signs s
    // against the stale frontier {add_b} with a backdated claimed time 3, and r honestly.
    fn new(w: &mut World) -> Self {
        let add_b = w.op(&w.a, &[], &[], 1, add(&w.b, false, None));
        let q = post(&mut w.b, &[&add_b], 2, None, "q");
        let rm_b = w.op(&w.a, &[&add_b], &[&q], 10, remove(&w.b));
        let s = post(&mut w.b, &[&add_b], 3, None, "s backdated after removal");
        let r = post(&mut w.b, &[&rm_b], 11, None, "r honest frontier");
        Self {
            add_b,
            rm_b,
            q,
            s,
            r,
        }
    }
    fn events(&self) -> Vec<Ev> {
        vec![
            m(&self.add_b),
            p(&self.q),
            m(&self.rm_b),
            p(&self.s),
            p(&self.r),
        ]
    }
}

// KIND: invariant
#[test]
fn d02_stale_frontier_post_is_byte_identical_to_honest_delayed_post() {
    // Two worlds, same seeds. Honest B never learns of its removal before writing; malicious
    // B holds rm_b in its own node first. Nothing B signs encodes what B knew.
    let mut honest = World::new();
    let mut malicious = World::new();
    let h = D01::new(&mut honest);
    let add_b = malicious.op(&malicious.a, &[], &[], 1, add(&malicious.b, false, None));
    let q = post(&mut malicious.b, &[&add_b], 2, None, "q seen by A");
    let rm_b = malicious.op(&malicious.a, &[&add_b], &[&q], 5, remove(&malicious.b));
    let mut b_view = Node::new(&malicious, PostRule::SignedFrontier, REMOVE_WINS, 0);
    for e in [m(&add_b), p(&q), m(&rm_b)] {
        assert_eq!(b_view.deliver(&e), Admit::Stored);
    }
    assert!(!b_view.current().has(&did(&malicious.b)));
    let stale = post(
        &mut malicious.b,
        &[&add_b],
        4,
        None,
        "p concurrent with removal",
    );
    assert_eq!(rm_b.id(), h.rm_b.id());
    assert_eq!(stale.bytes(), h.p.bytes());
    assert_eq!(stale.signature, h.p.signature);
    assert_eq!(stale.id(), h.p.id());
}

// KIND: limitation
#[test]
fn d02_signed_frontier_accepts_backdated_post_from_removed_member() {
    let mut w = World::new();
    let t = D02::new(&mut w);
    let rep = explore(&w, PostRule::SignedFrontier, REMOVE_WINS, 0, &t.events());
    let f = rep.single();
    // The removed member only needs to cite a pre-removal frontier: accepted in all 120 orders.
    assert_eq!(f.of(&t.s), Verdict::Accepted);
    assert_eq!(f.of(&t.r), Verdict::Rejected);
    assert!(f.accepted.contains(&t.s.id()));
    assert!(!f.members.has(&did(&w.b)));
}

// KIND: candidate-policy D02
#[test]
fn d02_removal_wins_rejects_backdated_post_but_only_remover_seen_posts_survive() {
    let mut w = World::new();
    let t = D02::new(&mut w);
    let rule = PostRule::RemovalWinsConcurrent;
    let rep = explore(&w, rule, REMOVE_WINS, 0, &t.events());
    let f = rep.single();
    assert_eq!(
        (f.of(&t.q), f.of(&t.s), f.of(&t.r)),
        (Verdict::Accepted, Verdict::Rejected, Verdict::Rejected)
    );
    // Same outcome as the honest delayed p in D01 (byte-identical case): no distinction.
    // What the removed member still achieves: provisional visibility on receivers that
    // have not yet received the removal (partition), withdrawn after heal.
    let mut cut_off = side(
        &w,
        rule,
        REMOVE_WINS,
        0,
        &[&m(&t.add_b), &p(&t.q), &p(&t.s)],
    );
    assert_eq!(cut_off.verdict(&t.s), Some(Verdict::Accepted));
    heal(&mut cut_off, &t.events());
    assert_eq!(cut_off.finalize(), *f);
    assert!(rep.retroactive);
}

// KIND: limitation
#[test]
fn d02_author_timestamp_accepts_backdated_claimed_time() {
    let mut w = World::new();
    let t = D02::new(&mut w);
    let rep = explore(&w, PostRule::AuthorTimestamp, REMOVE_WINS, 0, &t.events());
    let f = rep.single();
    // s claims at=3 < removal at=10: accepted in every order; only r (at=11) is rejected.
    assert_eq!(
        (f.of(&t.q), f.of(&t.s), f.of(&t.r)),
        (Verdict::Accepted, Verdict::Accepted, Verdict::Rejected)
    );
    // The removal's own claimed time is equally unverifiable: the same trace with the removal
    // claimed at 2 would also cut q. Timestamps order nothing a signer cannot choose.
}

// KIND: candidate-policy D02
#[test]
fn d02_epoch_cut_rejects_stale_epoch_post_not_named_in_cut() {
    let mut w = World::new();
    let e1 = w.op(&w.a, &[], &[], 1, epoch(1, &[&w.a, &w.b], &[&w.a], &[]));
    let q = post(&mut w.b, &[&e1], 2, Some(&e1), "q in epoch 1");
    let e2 = w.op(&w.a, &[&e1], &[&q], 10, epoch(2, &[&w.a], &[&w.a], &[&q]));
    let s = post(
        &mut w.b,
        &[&e1],
        3,
        Some(&e1),
        "s stale epoch 1 after removal",
    );
    let events = vec![m(&e1), p(&q), m(&e2), p(&s)];
    let rule = PostRule::EpochCut;
    let rep = explore(&w, rule, REMOVE_WINS, 0, &events);
    assert_eq!(rep.orders, 24);
    let f = rep.single();
    assert_eq!((f.of(&q), f.of(&s)), (Verdict::Accepted, Verdict::Rejected));
    assert!(rep.retroactive);
    // Partition: a receiver without e2 shows s until it learns e2.
    let mut stale = side(&w, rule, REMOVE_WINS, 0, &[&m(&e1), &p(&q), &p(&s)]);
    assert_eq!(stale.verdict(&s), Some(Verdict::Accepted));
    heal(&mut stale, &events);
    assert_eq!(stale.finalize(), *f);
}

// ---- D03: concurrent regrant/removal, competing admins, parent revocation -------------

struct D03Regrant {
    events: Vec<Ev>,
    stale: Envelope,
    merged: Envelope,
}
// A adds B (member) and C (admin). Concurrently: A removes B; C re-adds B. B posts `stale`
// citing only the regrant, and `merged` citing both.
fn d03_regrant(w: &mut World) -> D03Regrant {
    let add_b = w.op(&w.a, &[], &[], 1, add(&w.b, false, None));
    let add_c = w.op(&w.a, &[], &[], 1, add(&w.c, true, None));
    let rm_b = w.op(&w.a, &[&add_b, &add_c], &[], 2, remove(&w.b));
    let regrant = w.op(&w.c, &[&add_b, &add_c], &[], 2, add(&w.b, false, None));
    let stale = post(&mut w.b, &[&regrant], 3, None, "cites regrant only");
    let merged = post(&mut w.b, &[&rm_b, &regrant], 4, None, "cites both");
    D03Regrant {
        events: vec![
            m(&add_b),
            m(&add_c),
            m(&rm_b),
            m(&regrant),
            p(&stale),
            p(&merged),
        ],
        stale,
        merged,
    }
}
struct D03Rivals {
    events: Vec<Ev>,
    rm_c_by_b: MetaOp,
    rm_b_by_c: MetaOp,
}
// B and C are both admins; concurrently B removes C and C removes B.
fn d03_rivals(w: &World) -> D03Rivals {
    let add_b = w.op(&w.a, &[], &[], 1, add(&w.b, true, None));
    let add_c = w.op(&w.a, &[], &[], 1, add(&w.c, true, None));
    let rm_c_by_b = w.op(&w.b, &[&add_b, &add_c], &[], 2, remove(&w.c));
    let rm_b_by_c = w.op(&w.c, &[&add_b, &add_c], &[], 2, remove(&w.b));
    D03Rivals {
        events: vec![m(&add_b), m(&add_c), m(&rm_c_by_b), m(&rm_b_by_c)],
        rm_c_by_b,
        rm_b_by_c,
    }
}
fn d03_resolution(resolve: Resolve) -> (World, D03Regrant, Report, Report, D03Rivals) {
    let mut w = World::new();
    let t = d03_regrant(&mut w);
    let mem = Membership {
        resolve,
        cascade: false,
    };
    let regrant = explore(&w, PostRule::SignedFrontier, mem, 0, &t.events);
    assert_eq!(regrant.orders, 720);
    let rivals = d03_rivals(&w);
    let rival_rep = explore(&w, PostRule::SignedFrontier, mem, 0, &rivals.events);
    // Partition: each rival sees only its own removal and believes the other is gone.
    let left = side(
        &w,
        PostRule::SignedFrontier,
        mem,
        0,
        &rivals.events[..3].iter().collect::<Vec<_>>(),
    );
    let right_evs = [&rivals.events[0], &rivals.events[1], &rivals.events[3]];
    let right = side(&w, PostRule::SignedFrontier, mem, 0, &right_evs);
    assert!(left.current().is_admin(&did(&w.b)) && !left.current().has(&did(&w.c)));
    assert!(right.current().is_admin(&did(&w.c)) && !right.current().has(&did(&w.b)));
    // Stale frontier is unaffected by the resolution rule: `stale` never cites rm_b.
    assert_eq!(regrant.single().of(&t.stale), Verdict::Accepted);
    (w, t, regrant, rival_rep, rivals)
}

// KIND: candidate-policy D03
#[test]
fn d03_remove_wins_resolution() {
    let (w, t, regrant, rivals, _) = d03_resolution(Resolve::RemoveWins);
    let f = regrant.single();
    assert!(!f.members.has(&did(&w.b)));
    assert_eq!(f.of(&t.merged), Verdict::Rejected);
    // Mutual removal: both rival admins are removed, in every order.
    let r = rivals.single();
    assert!(!r.members.has(&did(&w.b)) && !r.members.has(&did(&w.c)));
}

// KIND: candidate-policy D03
#[test]
fn d03_add_wins_resolution() {
    let (w, t, regrant, rivals, _) = d03_resolution(Resolve::AddWins);
    let f = regrant.single();
    assert!(f.members.has(&did(&w.b)));
    assert_eq!(f.of(&t.merged), Verdict::Accepted);
    // Add-wins does not help mutual removal (no concurrent add): both are removed.
    let r = rivals.single();
    assert!(!r.members.has(&did(&w.b)) && !r.members.has(&did(&w.c)));
}

// KIND: candidate-policy D03
#[test]
fn d03_priority_fold_resolution() {
    let (w, t, regrant, rivals, ops) = d03_resolution(Resolve::Priority);
    let f = regrant.single();
    // Creator's removal is applied after the junior admin's regrant: B is out.
    assert!(!f.members.has(&did(&w.b)));
    assert_eq!(f.of(&t.merged), Verdict::Rejected);
    // Equal-power rivals: the lower op ID applies first and disables the other issuer.
    // Deterministic in every order, but the survivor is an arbitrary hash comparison.
    let r = rivals.single();
    let b_survives = ops.rm_c_by_b.id() < ops.rm_b_by_c.id();
    assert_eq!(r.members.is_admin(&did(&w.b)), b_survives);
    assert_eq!(r.members.is_admin(&did(&w.c)), !b_survives);
}

struct D03Parent {
    events: Vec<Ev>,
    stale: Envelope,
    after: Envelope,
}
// A makes B admin; B adds C; later A removes B (having seen B's add of C). Concurrently with
// that removal B adds D. C posts `stale` at {add_c} and `after` at {rm_b}.
fn d03_parent(w: &mut World) -> D03Parent {
    let add_b = w.op(&w.a, &[], &[], 1, add(&w.b, true, None));
    let add_c = w.op(&w.b, &[&add_b], &[], 2, add(&w.c, false, None));
    let rm_b = w.op(&w.a, &[&add_b, &add_c], &[], 3, remove(&w.b));
    let add_d = w.op(&w.b, &[&add_b], &[], 3, add(&w.d, false, None));
    let stale = post(&mut w.c, &[&add_c], 4, None, "C before parent revocation");
    let after = post(&mut w.c, &[&rm_b], 5, None, "C after parent revocation");
    D03Parent {
        events: vec![
            m(&add_b),
            m(&add_c),
            m(&rm_b),
            m(&add_d),
            p(&stale),
            p(&after),
        ],
        stale,
        after,
    }
}

// KIND: candidate-policy D03
#[test]
fn d03_no_cascade_keeps_members_added_by_later_removed_admin() {
    let mut w = World::new();
    let t = d03_parent(&mut w);
    let rep = explore(&w, PostRule::SignedFrontier, REMOVE_WINS, 0, &t.events);
    let f = rep.single();
    // C (added before revocation) and D (added concurrently with it) both remain.
    assert!(f.members.has(&did(&w.c)) && f.members.has(&did(&w.d)));
    assert!(!f.members.has(&did(&w.b)));
    assert_eq!(
        (f.of(&t.stale), f.of(&t.after)),
        (Verdict::Accepted, Verdict::Accepted)
    );
}

// KIND: candidate-policy D03
#[test]
fn d03_cascade_drops_members_added_by_removed_admin_but_not_stale_frontier_posts() {
    let mut w = World::new();
    let t = d03_parent(&mut w);
    let mem = Membership {
        resolve: Resolve::RemoveWins,
        cascade: true,
    };
    let rep = explore(&w, PostRule::SignedFrontier, mem, 0, &t.events);
    let f = rep.single();
    assert!(!f.members.has(&did(&w.c)) && !f.members.has(&did(&w.d)));
    assert_eq!(f.of(&t.after), Verdict::Rejected);
    // A cascade-removed member citing the pre-revocation frontier is still accepted.
    assert_eq!(f.of(&t.stale), Verdict::Accepted);
    // Same under the priority fold with cascade.
    let pri = Membership {
        resolve: Resolve::Priority,
        cascade: true,
    };
    let g = explore(&w, PostRule::SignedFrontier, pri, 0, &t.events);
    let g = g.single();
    assert!(!g.members.has(&did(&w.c)) && !g.members.has(&did(&w.d)));
    assert_eq!(
        (g.of(&t.stale), g.of(&t.after)),
        (Verdict::Accepted, Verdict::Rejected)
    );
}

// ---- D04: grant expiry vs delayed delivery and receiver clocks ------------------------

struct D04 {
    add_b: MetaOp,
    p1: Envelope,
    p2: Envelope,
    p3: Envelope,
}
// B's grant expires at 10. p1 claims 5; p2 honestly claims 15; p3 (B's second device, own
// chain) was written at 15 but CLAIMS 5.
fn d04(w: &mut World) -> D04 {
    let add_b = w.op(&w.a, &[], &[], 1, add(&w.b, false, Some(10)));
    let p1 = post(&mut w.b, &[&add_b], 5, None, "p1");
    let p2 = post(&mut w.b, &[&add_b], 15, None, "p2 after expiry");
    let p3 = second_device_post(&w.b, 42, &[&add_b], 5, None, "p3 backdated");
    D04 { add_b, p1, p2, p3 }
}
impl D04 {
    fn events(&self) -> Vec<Ev> {
        vec![m(&self.add_b), p(&self.p1), p(&self.p2), p(&self.p3)]
    }
}

// KIND: candidate-policy D04
#[test]
fn d04_receiver_clock_expiry_makes_history_depend_on_receiver_and_arrival_time() {
    let mut w = World::new();
    let t = d04(&mut w);
    let rule = PostRule::LocalClockExpiry;
    let early = explore(&w, rule, REMOVE_WINS, 8, &t.events());
    let late = explore(&w, rule, REMOVE_WINS, 12, &t.events());
    let (e, l) = (early.single(), late.single());
    assert!(
        [&t.p1, &t.p2, &t.p3]
            .iter()
            .all(|x| e.of(x) == Verdict::Accepted)
    );
    assert!(
        [&t.p1, &t.p2, &t.p3]
            .iter()
            .all(|x| l.of(x) == Verdict::Rejected)
    );
    assert_ne!(e, l);
    // Same receiver, clock advancing between deliveries: a third distinct history.
    let mut n = Node::new(&w, rule, REMOVE_WINS, 8);
    n.deliver(&m(&t.add_b));
    n.deliver(&p(&t.p1));
    n.clock = 12;
    n.deliver(&p(&t.p2));
    n.deliver(&p(&t.p3));
    let mid = n.finalize();
    assert_eq!(
        (mid.of(&t.p1), mid.of(&t.p2), mid.of(&t.p3)),
        (Verdict::Accepted, Verdict::Rejected, Verdict::Rejected)
    );
    assert_ne!(mid, *e);
    assert_ne!(mid, *l);
}

fn current_access(n: &Node, who: &str) -> bool {
    n.current()
        .expiry(who)
        .is_some_and(|x| x.is_none_or(|x| n.clock < x))
}

// KIND: candidate-policy D04
#[test]
fn d04_author_timestamp_expiry_is_deterministic_and_separate_from_current_access() {
    let mut w = World::new();
    let t = d04(&mut w);
    let rule = PostRule::TimestampExpiry;
    let early = explore(&w, rule, REMOVE_WINS, 8, &t.events());
    let late = explore(&w, rule, REMOVE_WINS, 12, &t.events());
    let f = early.single();
    assert_eq!(f, late.single());
    assert_eq!(
        (f.of(&t.p1), f.of(&t.p2)),
        (Verdict::Accepted, Verdict::Rejected)
    );
    // Current access (sessions, future distribution) may use the local clock without
    // touching historical validity: receivers differ here, histories do not.
    let evs = t.events();
    let n8 = side(&w, rule, REMOVE_WINS, 8, &evs.iter().collect::<Vec<_>>());
    let n12 = side(&w, rule, REMOVE_WINS, 12, &evs.iter().collect::<Vec<_>>());
    assert!(current_access(&n8, &did(&w.b)));
    assert!(!current_access(&n12, &did(&w.b)));
    assert_eq!(n8.finalize(), n12.finalize());
}

// KIND: limitation
#[test]
fn d04_author_timestamp_expiry_accepts_backdated_post_after_expiry() {
    let mut w = World::new();
    let t = d04(&mut w);
    for clock in [8, 12, 1_000] {
        let rep = explore(
            &w,
            PostRule::TimestampExpiry,
            REMOVE_WINS,
            clock,
            &t.events(),
        );
        let f = rep.single();
        // p3 was written after expiry but claims at=5: accepted by every receiver forever.
        assert_eq!(f.of(&t.p3), Verdict::Accepted);
        assert!(f.accepted.contains(&t.p3.id()));
    }
}

// KIND: candidate-policy D04
#[test]
fn d04_explicit_expiry_op_is_clock_free_but_unenforced_until_issued_and_cited() {
    let mut w = World::new();
    let t = d04(&mut w);
    // Expiry is enforced only by an admin-issued removal; the grant's expires_at is advisory.
    let rm_b = w.op(&w.a, &[&t.add_b], &[&t.p1], 10, remove(&w.b));
    let p4 = post(&mut w.b, &[&rm_b], 16, None, "p4 cites the expiry op");
    let events = vec![m(&t.add_b), p(&t.p1), p(&t.p2), m(&rm_b), p(&p4)];
    let rule = PostRule::SignedFrontier;
    let early = explore(&w, rule, REMOVE_WINS, 8, &events);
    let late = explore(&w, rule, REMOVE_WINS, 12, &events);
    let f = early.single();
    assert_eq!(f, late.single());
    // p2 (written at 15, frontier without the expiry op) stays valid; p4 does not.
    assert_eq!(
        (f.of(&t.p1), f.of(&t.p2), f.of(&p4)),
        (Verdict::Accepted, Verdict::Accepted, Verdict::Rejected)
    );
    assert!(!early.retroactive);
}

// ---- D05: offline across removal / epoch changes, later rejoin ------------------------

struct D05 {
    events: Vec<Ev>,
    o: Envelope,
    n1: Envelope,
    n2: Envelope,
}
// B goes offline after add_b and writes o. Meanwhile A removes B (not having seen o) and
// later re-adds B. B returns: n1 on the same device chain (previous = o), n2 on a new device.
fn d05(w: &mut World) -> D05 {
    let add_b = w.op(&w.a, &[], &[], 1, add(&w.b, false, None));
    let o = post(&mut w.b, &[&add_b], 3, None, "o offline");
    let rm_b = w.op(&w.a, &[&add_b], &[], 5, remove(&w.b));
    let readd = w.op(&w.a, &[&rm_b], &[], 9, add(&w.b, false, None));
    let n1 = post(&mut w.b, &[&readd], 20, None, "n1 same device after rejoin");
    let n2 = second_device_post(&w.b, 43, &[&readd], 20, None, "n2 new device after rejoin");
    D05 {
        events: vec![m(&add_b), p(&o), m(&rm_b), m(&readd), p(&n1), p(&n2)],
        o,
        n1,
        n2,
    }
}

// KIND: candidate-policy D05
#[test]
fn d05_signed_frontier_offline_posts_survive_removal_and_rejoin() {
    let mut w = World::new();
    let t = d05(&mut w);
    let rep = explore(&w, PostRule::SignedFrontier, REMOVE_WINS, 0, &t.events);
    let f = rep.single();
    assert!(
        [&t.o, &t.n1, &t.n2]
            .iter()
            .all(|x| f.of(x) == Verdict::Accepted)
    );
    assert_eq!((f.accepted.len(), f.pending.len()), (3, 0));
    assert!(f.members.has(&did(&w.b)));
    assert!(!rep.retroactive);
}

// KIND: candidate-policy D05
#[test]
fn d05_removal_wins_drops_offline_post_and_strands_same_device_chain() {
    let mut w = World::new();
    let t = d05(&mut w);
    let rep = explore(
        &w,
        PostRule::RemovalWinsConcurrent,
        REMOVE_WINS,
        0,
        &t.events,
    );
    let f = rep.single();
    assert_eq!(f.of(&t.o), Verdict::Rejected);
    // n1 is valid by policy but its signed predecessor o is rejected: it stays pending in
    // the content layer forever (withheld dependency), never implicitly accepted.
    assert_eq!(f.of(&t.n1), Verdict::Accepted);
    assert_eq!(f.pending, vec![t.n1.id()]);
    // Rejoining on a fresh device chain works.
    assert_eq!(f.of(&t.n2), Verdict::Accepted);
    assert_eq!(f.accepted, vec![t.n2.id()]);
    assert!(rep.retroactive);
}

// KIND: invariant
#[test]
fn d05_rejected_predecessor_never_becomes_implicitly_accepted() {
    let mut w = World::new();
    let t = d05(&mut w);
    let mut n = side(
        &w,
        PostRule::RemovalWinsConcurrent,
        REMOVE_WINS,
        0,
        &t.events.iter().collect::<Vec<_>>(),
    );
    // Redelivery and a second complete sync do not release n1.
    for _ in 0..2 {
        for e in &t.events {
            assert_eq!(n.deliver(e), Admit::Duplicate);
        }
        let mut r = n.materialize();
        let mut again = n.materialize();
        again.sync_from(&r);
        r.sync_from(&again);
        assert!(r.pending.contains_key(&t.n1.id()));
        assert!(!r.accepted.contains_key(&t.n1.id()) && !r.accepted.contains_key(&t.o.id()));
    }
    // Negative control: under a rule that accepts o, n1 is accepted.
    let ok = side(
        &w,
        PostRule::SignedFrontier,
        REMOVE_WINS,
        0,
        &t.events.iter().collect::<Vec<_>>(),
    );
    assert!(ok.materialize().accepted.contains_key(&t.n1.id()));
}

// KIND: candidate-policy D05
#[test]
fn d05_epoch_cut_drops_uncut_offline_post_and_rejoin_needs_new_epoch() {
    for o_seen in [false, true] {
        let mut w = World::new();
        let e1 = w.op(&w.a, &[], &[], 1, epoch(1, &[&w.a, &w.b], &[&w.a], &[]));
        let o = post(&mut w.b, &[&e1], 3, Some(&e1), "o offline in epoch 1");
        let cut: Vec<&Envelope> = if o_seen { vec![&o] } else { vec![] };
        let e2 = w.op(&w.a, &[&e1], &cut, 5, epoch(2, &[&w.a], &[&w.a], &cut));
        let e3 = w.op(&w.a, &[&e2], &[], 9, epoch(3, &[&w.a, &w.b], &[&w.a], &[]));
        let n2 = second_device_post(&w.b, 43, &[&e3], 20, Some(&e3), "n2 in epoch 3");
        let events = vec![m(&e1), p(&o), m(&e2), m(&e3), p(&n2)];
        let rep = explore(&w, PostRule::EpochCut, REMOVE_WINS, 0, &events);
        let f = rep.single();
        // o survives only if the epoch issuer had it and named it in the cut.
        let expected = if o_seen {
            Verdict::Accepted
        } else {
            Verdict::Rejected
        };
        assert_eq!(f.of(&o), expected);
        assert_eq!(f.of(&n2), Verdict::Accepted);
        assert_eq!(rep.retroactive, !o_seen);
    }
}

// KIND: candidate-policy D05
#[test]
fn d05_competing_epochs_after_partition_tie_break_drops_losing_branch() {
    let mut w = World::new();
    let e1 = w.op(
        &w.a,
        &[],
        &[],
        1,
        epoch(1, &[&w.a, &w.b, &w.c], &[&w.a, &w.c], &[]),
    );
    let all = [&w.a, &w.b, &w.c];
    let e2a = w.op(&w.a, &[&e1], &[], 5, epoch(2, &all, &[&w.a, &w.c], &[]));
    let e2c = w.op(&w.c, &[&e1], &[], 5, epoch(2, &all, &[&w.a, &w.c], &[]));
    let pb = post(&mut w.b, &[&e2a], 6, Some(&e2a), "B on A's epoch 2");
    let pc = post(&mut w.c, &[&e2c], 6, Some(&e2c), "C on C's epoch 2");
    let events = vec![m(&e1), m(&e2a), m(&e2c), p(&pb), p(&pc)];
    let rule = PostRule::EpochCut;
    let rep = explore(&w, rule, REMOVE_WINS, 0, &events);
    let f = rep.single();
    let a_wins = e2a.id() < e2c.id();
    let (win, lose) = if a_wins { (&pb, &pc) } else { (&pc, &pb) };
    assert_eq!(
        (f.of(win), f.of(lose)),
        (Verdict::Accepted, Verdict::Rejected)
    );
    // Partition: each side accepts its own post; after heal the losing side's post is dropped.
    let mut left = side(&w, rule, REMOVE_WINS, 0, &[&m(&e1), &m(&e2a), &p(&pb)]);
    let mut right = side(&w, rule, REMOVE_WINS, 0, &[&m(&e1), &m(&e2c), &p(&pc)]);
    assert_eq!(left.verdict(&pb), Some(Verdict::Accepted));
    assert_eq!(right.verdict(&pc), Some(Verdict::Accepted));
    heal(&mut left, &events);
    heal(&mut right, &events);
    assert_eq!(left.finalize(), *f);
    assert_eq!(right.finalize(), *f);
}

// ---- shared: policy-layer admission ---------------------------------------------------

// KIND: invariant
#[test]
fn forged_or_misscoped_meta_ops_and_headerless_posts_leave_policy_node_unchanged() {
    let mut w = World::new();
    let t = D01::new(&mut w);
    let mut n = Node::new(&w, PostRule::SignedFrontier, REMOVE_WINS, 0);
    assert_eq!(n.deliver(&m(&t.add_b)), Admit::Stored);
    assert_eq!(n.deliver(&p(&t.q)), Admit::Stored);
    assert_eq!(n.deliver(&p(&t.r)), Admit::Pending); // waits for rm_b
    let mut forged = t.rm_b.clone();
    forged.signature[0] ^= 1;
    let mut altered = t.rm_b.clone();
    altered.body.change = remove(&w.c);
    let other_group = MetaOp::issue(&w.a.id, "other", &[&t.add_b], &[], 5, remove(&w.b));
    let mut bogus_issuer = t.rm_b.clone();
    bogus_issuer.body.issuer = "did:key:not-a-key".into();
    bogus_issuer.signature = w.a.id.sign(&MetaOp::bytes(&bogus_issuer.body)).to_vec();
    let headerless = w.c.post("no policy header");
    for ev in [
        m(&forged),
        m(&altered),
        m(&other_group),
        m(&bogus_issuer),
        p(&headerless),
    ] {
        let before = n.snapshot();
        assert_eq!(n.deliver(&ev), Admit::Invalid);
        assert_eq!(n.snapshot(), before, "rejection mutated policy node");
    }
    // An authentic op by a non-admin is retained (its effect may depend on later knowledge
    // under some candidates) but never changes membership here.
    let members = n.current();
    let self_add = w.op(&w.d, &[&t.add_b], &[], 3, add(&w.d, true, None));
    assert_eq!(n.deliver(&m(&self_add)), Admit::Stored);
    assert_eq!(n.current(), members);
    // Positive control: the authentic removal is stored and releases r.
    assert_eq!(n.deliver(&m(&t.rm_b)), Admit::Stored);
    assert_eq!(n.verdict(&t.r), Some(Verdict::Rejected));
    assert_eq!(n.deliver(&m(&t.rm_b)), Admit::Duplicate);
}
