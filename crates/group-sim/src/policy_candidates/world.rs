use super::*;

pub(super) struct World {
    pub(super) genesis: Genesis,
    pub(super) gid: String,
    pub(super) a: Author,
    pub(super) b: Author,
    pub(super) c: Author,
    pub(super) d: Author,
}
impl World {
    pub(super) fn new() -> Self {
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
    pub(super) fn dids(&self) -> BTreeSet<String> {
        [&self.a, &self.b, &self.c, &self.d]
            .iter()
            .map(|x| x.id.did().to_string())
            .collect()
    }
    pub(super) fn op(
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
pub(super) fn add(m: &Author, admin: bool, expires_at: Option<u64>) -> Change {
    Change::Add {
        member: did(m),
        admin,
        expires_at,
    }
}
pub(super) fn remove(m: &Author) -> Change {
    Change::Remove { member: did(m) }
}
pub(super) fn epoch(
    number: u64,
    members: &[&Author],
    admins: &[&Author],
    cut: &[&Envelope],
) -> Change {
    Change::Epoch {
        number,
        members: members.iter().map(|m| did(m)).collect(),
        admins: admins.iter().map(|m| did(m)).collect(),
        cut: cut.iter().map(|e| e.id()).collect(),
    }
}
pub(super) fn m(op: &MetaOp) -> Ev {
    Ev::Meta(op.clone())
}
pub(super) fn p(e: &Envelope) -> Ev {
    Ev::Post(e.clone())
}
// Deliver `events` to a fresh node in EVERY order; collect distinct final outcomes.
pub(super) fn explore(
    w: &World,
    rule: PostRule,
    mem: Membership,
    clock: u64,
    events: &[Ev],
) -> Report {
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
pub(super) fn side(w: &World, rule: PostRule, mem: Membership, clock: u64, evs: &[&Ev]) -> Node {
    let mut n = Node::new(w, rule, mem, clock);
    for e in evs {
        n.deliver(e);
    }
    n
}
pub(super) fn heal(n: &mut Node, all: &[Ev]) {
    for e in all {
        assert_ne!(n.deliver(e), Admit::Invalid);
    }
}

// ---- D01: revocation vs truly concurrent post -------------------------------------------

pub(super) struct D01 {
    pub(super) add_b: MetaOp,
    pub(super) rm_b: MetaOp,
    pub(super) q: Envelope,
    pub(super) p: Envelope,
    pub(super) r: Envelope,
}
impl D01 {
    // A adds B; B posts q (A sees it); A removes B (seen q); concurrently B, offline, posts p
    // at the pre-removal frontier; after learning of removal B posts r at {rm_b}.
    pub(super) fn new(w: &mut World) -> Self {
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
    pub(super) fn events(&self) -> Vec<Ev> {
        vec![
            m(&self.add_b),
            p(&self.q),
            m(&self.rm_b),
            p(&self.p),
            p(&self.r),
        ]
    }
    // Index sets: A's side holds the removal; B's side holds the concurrent post.
    pub(super) fn sides(&self) -> ([Ev; 3], [Ev; 3]) {
        (
            [m(&self.add_b), p(&self.q), m(&self.rm_b)],
            [m(&self.add_b), p(&self.q), p(&self.p)],
        )
    }
}
