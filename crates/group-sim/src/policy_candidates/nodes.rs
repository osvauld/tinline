use super::*;

pub(super) struct Node {
    pub(super) group: String,
    pub(super) creator: String,
    pub(super) dids: BTreeSet<String>,
    pub(super) rule: PostRule,
    pub(super) membership: Membership,
    pub(super) clock: u64,
    pub(super) meta: BTreeMap<String, MetaOp>,
    pub(super) meta_pending: BTreeMap<String, MetaOp>,
    pub(super) posts: BTreeMap<String, Envelope>,
    pub(super) posts_pending: BTreeMap<String, Envelope>,
    // ArrivalView / LocalClockExpiry decisions, fixed once made.
    pub(super) arrival: BTreeMap<String, bool>,
    // Authorization at an op's own frontier never changes once the op is admitted.
    pub(super) auth_cache: RefCell<BTreeMap<String, bool>>,
    // A frontier's closure is fixed once admitted, so its evaluated state never changes.
    pub(super) state_cache: RefCell<BTreeMap<BTreeSet<String>, Members>>,
}
impl Node {
    pub(super) fn new(w: &World, rule: PostRule, membership: Membership, clock: u64) -> Self {
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
    pub(super) fn valid_proof_cached(&self, e: &Envelope) -> bool {
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
    pub(super) fn content_replica(&self) -> Replica {
        let mut r = Replica::new(&self.group, &[]);
        r.authorization = Box::new(FixtureAllowlist(self.dids.clone()));
        r
    }
    pub(super) fn deliver(&mut self, ev: &Ev) -> Admit {
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
    pub(super) fn refs_ready(&self, e: &Envelope) -> bool {
        let h = header(e).unwrap();
        h.frontier.iter().all(|f| self.meta.contains_key(f))
            && h.epoch.iter().all(|x| self.meta.contains_key(x))
    }
    pub(super) fn drain(&mut self) {
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
    pub(super) fn arrival_decision(&self, e: &Envelope) -> Option<bool> {
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
    pub(super) fn closure(&self, frontier: &BTreeSet<String>) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut todo: Vec<String> = frontier.iter().cloned().collect();
        while let Some(id) = todo.pop() {
            if out.insert(id.clone()) {
                todo.extend(self.meta[&id].body.parents.iter().cloned());
            }
        }
        out
    }
    pub(super) fn auth_own(&self, id: &str) -> bool {
        let cached = self.auth_cache.borrow().get(id).copied();
        if let Some(v) = cached {
            return v;
        }
        let op = &self.meta[id];
        let v = authorized(op, &self.state(&op.body.parents), &self.creator);
        self.auth_cache.borrow_mut().insert(id.into(), v);
        v
    }
    pub(super) fn set_fold(&self, ids: &BTreeSet<String>, excluded: &BTreeSet<String>) -> Members {
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
    pub(super) fn power(&self, s: &Members, did: &str) -> u8 {
        if did == self.creator {
            2
        } else {
            s.is_admin(did) as u8
        }
    }
    pub(super) fn priority_fold(
        &self,
        ids: &BTreeSet<String>,
        excluded: &BTreeSet<String>,
    ) -> Members {
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
    pub(super) fn state(&self, frontier: &BTreeSet<String>) -> Members {
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
    pub(super) fn state_uncached(&self, frontier: &BTreeSet<String>) -> Members {
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
    pub(super) fn current(&self) -> Members {
        self.state(&self.meta.keys().cloned().collect())
    }
    pub(super) fn state_by_time(&self, t: u64) -> Members {
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
    pub(super) fn post_past(&self, seen: &BTreeSet<String>) -> BTreeSet<String> {
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
    pub(super) fn concurrent_removal(&self, e: &Envelope, h: &Header) -> bool {
        let past = self.closure(&h.frontier);
        let id = e.id();
        self.meta.iter().any(|(rid, r)| {
            matches!(&r.body.change, Change::Remove { member } if *member == e.body.author)
                && !past.contains(rid)
                && self.auth_own(rid)
                && !self.post_past(&r.body.seen_posts).contains(&id)
        })
    }
    pub(super) fn epoch_parent(&self, op: &MetaOp) -> Option<String> {
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
    pub(super) fn epoch_valid(&self, id: &str) -> bool {
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
    pub(super) fn epoch_chain(&self) -> Vec<String> {
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
    pub(super) fn epoch_ok(&self, e: &Envelope, h: &Header) -> bool {
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
    pub(super) fn accepted_by_rule(&self, e: &Envelope) -> bool {
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
    pub(super) fn verdicts(&self) -> BTreeMap<String, Verdict> {
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
    pub(super) fn materialize(&self) -> Replica {
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
    pub(super) fn finalize(&self) -> Final {
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
    pub(super) fn verdict(&self, e: &Envelope) -> Option<Verdict> {
        self.verdicts().get(&e.id()).copied()
    }
    pub(super) fn snapshot(&self) -> Vec<u8> {
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
pub(super) struct Final {
    pub(super) verdicts: BTreeMap<String, Verdict>,
    pub(super) members: Members,
    pub(super) accepted: Vec<String>,
    pub(super) pending: Vec<String>,
    pub(super) value: String,
}
impl Final {
    pub(super) fn of(&self, e: &Envelope) -> Verdict {
        self.verdicts[&e.id()]
    }
}
pub(super) struct Report {
    pub(super) finals: Vec<Final>,
    pub(super) orders: usize,
    // Some order showed a post Accepted at an intermediate step but not in the final state.
    pub(super) retroactive: bool,
}
impl Report {
    pub(super) fn single(&self) -> &Final {
        assert_eq!(
            self.finals.len(),
            1,
            "order-dependent outcome: {:?}",
            self.finals.iter().map(|f| &f.verdicts).collect::<Vec<_>>()
        );
        &self.finals[0]
    }
}
