use super::*;

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
