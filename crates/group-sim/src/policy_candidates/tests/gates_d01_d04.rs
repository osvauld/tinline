use super::*;

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

pub(super) struct D02 {
    pub(super) add_b: MetaOp,
    pub(super) rm_b: MetaOp,
    pub(super) q: Envelope,
    pub(super) s: Envelope,
    pub(super) r: Envelope,
}
impl D02 {
    // B is removed at claimed time 10 (A saw q). B has RECEIVED the removal, then signs s
    // against the stale frontier {add_b} with a backdated claimed time 3, and r honestly.
    pub(super) fn new(w: &mut World) -> Self {
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
    pub(super) fn events(&self) -> Vec<Ev> {
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

pub(super) struct D03Regrant {
    pub(super) events: Vec<Ev>,
    pub(super) stale: Envelope,
    pub(super) merged: Envelope,
}
// A adds B (member) and C (admin). Concurrently: A removes B; C re-adds B. B posts `stale`
// citing only the regrant, and `merged` citing both.
pub(super) fn d03_regrant(w: &mut World) -> D03Regrant {
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
pub(super) struct D03Rivals {
    pub(super) events: Vec<Ev>,
    pub(super) rm_c_by_b: MetaOp,
    pub(super) rm_b_by_c: MetaOp,
}
// B and C are both admins; concurrently B removes C and C removes B.
pub(super) fn d03_rivals(w: &World) -> D03Rivals {
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
pub(super) fn d03_resolution(resolve: Resolve) -> (World, D03Regrant, Report, Report, D03Rivals) {
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

pub(super) struct D03Parent {
    pub(super) events: Vec<Ev>,
    pub(super) stale: Envelope,
    pub(super) after: Envelope,
}
// A makes B admin; B adds C; later A removes B (having seen B's add of C). Concurrently with
// that removal B adds D. C posts `stale` at {add_c} and `after` at {rm_b}.
pub(super) fn d03_parent(w: &mut World) -> D03Parent {
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

pub(super) struct D04 {
    pub(super) add_b: MetaOp,
    pub(super) p1: Envelope,
    pub(super) p2: Envelope,
    pub(super) p3: Envelope,
}
// B's grant expires at 10. p1 claims 5; p2 honestly claims 15; p3 (B's second device, own
// chain) was written at 15 but CLAIMS 5.
pub(super) fn d04(w: &mut World) -> D04 {
    let add_b = w.op(&w.a, &[], &[], 1, add(&w.b, false, Some(10)));
    let p1 = post(&mut w.b, &[&add_b], 5, None, "p1");
    let p2 = post(&mut w.b, &[&add_b], 15, None, "p2 after expiry");
    let p3 = second_device_post(&w.b, 42, &[&add_b], 5, None, "p3 backdated");
    D04 { add_b, p1, p2, p3 }
}
impl D04 {
    pub(super) fn events(&self) -> Vec<Ev> {
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

pub(super) fn current_access(n: &Node, who: &str) -> bool {
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

pub(super) struct D05 {
    pub(super) events: Vec<Ev>,
    pub(super) o: Envelope,
    pub(super) n1: Envelope,
    pub(super) n2: Envelope,
}
// B goes offline after add_b and writes o. Meanwhile A removes B (not having seen o) and
// later re-adds B. B returns: n1 on the same device chain (previous = o), n2 on a new device.
pub(super) fn d05(w: &mut World) -> D05 {
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
