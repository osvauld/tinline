use super::*;

// KIND: invariant
#[test]
fn original_author_survives_host_relay_and_offline_delivery() {
    let mut a = Author::new(1, "g");
    let b = Author::new(2, "g");
    let mut h = Replica::new("g", &[&a, &b]);
    let mut r = Replica::new("g", &[&a, &b]);
    let e = a.post("hello");
    assert_eq!(h.receive(e.clone()), Outcome::Accepted);
    r.sync_from(&h);
    h.assert_same(&r);
    assert_eq!(r.accepted[&e.id()].body.author, a.id.did());
    assert_eq!(r.accepted[&e.id()].signature, e.signature);
}
// KIND: invariant
#[test]
fn all_small_delivery_permutations_converge_with_duplicates() {
    let mut a = Author::new(1, "g");
    let mut b = Author::new(2, "g");
    let es = [a.post("a1"), a.post("a2"), b.post("b1")];
    let mut oracle = Replica::new("g", &[&a, &b]);
    for e in &es {
        assert_eq!(oracle.receive(e.clone()), Outcome::Accepted);
    }
    let mut duplicates = 0;
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut r = Replica::new("g", &[&a, &b]);
        for i in order {
            let first = r.receive(es[i].clone());
            let second = r.receive(es[i].clone());
            // A redelivery is Duplicate once accepted, or stays Pending (not re-queued).
            match first {
                Outcome::Accepted => assert_eq!(second, Outcome::Duplicate),
                Outcome::Pending => assert_eq!(second, Outcome::Pending),
                other => panic!("valid envelope gave {other:?}"),
            }
            duplicates += (second == Outcome::Duplicate) as usize;
        }
        oracle.assert_same(&r);
        assert_eq!(r.fanout.len(), 3);
    }
    assert!(duplicates > 0, "duplicate path never exercised");
}
// KIND: invariant
#[test]
fn missing_dependency_is_pending_not_converged() {
    let mut a = Author::new(1, "g");
    let first = a.post("first");
    let second = a.post("second");
    let mut r = Replica::new("g", &[&a]);
    assert_eq!(r.receive(second), Outcome::Pending);
    assert!(r.accepted.is_empty() && r.fanout.is_empty());
    assert_eq!(r.receive(first), Outcome::Accepted);
    assert_eq!(r.accepted.len(), 2);
    assert!(r.pending.is_empty());
}
// KIND: invariant
#[test]
fn forged_tampered_cross_group_and_outsider_inputs_leave_no_state() {
    let mut a = Author::new(1, "g");
    let mut x = Author::new(3, "g");
    let good = a.post("good");
    let mut tampered = good.clone();
    tampered.body.text = "forged".into();
    let mut wrong_did = good.clone();
    wrong_did.body.author = x.id.did().into();
    let outsider = x.post("outside");
    let mut r = Replica::new("g", &[&a]);
    for e in [tampered, wrong_did, outsider] {
        assert_eq!(r.receive(e), Outcome::Invalid);
    }
    assert!(r.accepted.is_empty() && r.pending.is_empty() && r.fanout.is_empty());
    let mut other = Replica::new("other", &[&a]);
    assert_eq!(other.receive(good.clone()), Outcome::Invalid);
    assert!(other.accepted.is_empty() && other.pending.is_empty() && other.fanout.is_empty());
    // Positive control: the same envelope is valid for its own group.
    assert_eq!(r.receive(good), Outcome::Accepted);
}
// KIND: invariant
#[test]
fn malicious_author_cannot_hide_extra_loro_mutations_in_signed_update() {
    let mut a = Author::new(1, "g");
    let mut e = a.post("ok");
    a.doc
        .get_map("secret")
        .insert("unexpected", "injection")
        .unwrap();
    a.doc.commit();
    e.body.update = a
        .doc
        .export(ExportMode::updates(&Default::default()))
        .unwrap();
    e.signature = a.key.sign(&e.bytes()).to_bytes();
    let mut r = Replica::new("g", &[&a]);
    assert_eq!(r.receive(e), Outcome::Invalid);
    assert!(r.accepted.is_empty() && r.fanout.is_empty());
}
// KIND: invariant
#[test]
fn two_hosts_partition_then_reconcile_without_rolling_back_local_writes() {
    let mut a = Author::new(1, "g");
    let mut b = Author::new(2, "g");
    let mut h1 = Replica::new("g", &[&a, &b]);
    let mut h2 = Replica::new("g", &[&a, &b]);
    h1.receive(a.post("left"));
    h2.receive(b.post("right"));
    h1.sync_from(&h2);
    h2.sync_from(&h1);
    h1.assert_same(&h2);
    let stale = Replica::new("g", &[&a, &b]);
    h1.sync_from(&stale);
    assert_eq!(h1.accepted.len(), 2);
    // Non-vacuous stale peer: holds a strict subset (only "left"). Syncing from it must not
    // roll h1 back; syncing to it catches it up.
    let mut stale = Replica::new("g", &[&a, &b]);
    let left = h1
        .accepted
        .values()
        .find(|e| e.body.text == "left")
        .unwrap()
        .clone();
    assert_eq!(stale.receive(left.clone()), Outcome::Accepted);
    let before = h1.snapshot();
    assert_eq!(h1.receive(left), Outcome::Duplicate);
    h1.sync_from(&stale);
    assert_eq!(h1.snapshot(), before);
    stale.sync_from(&h1);
    h1.assert_same(&stale);
    assert_eq!(stale.accepted.len(), 2);
}
// KIND: invariant
#[test]
fn seeded_reordered_loss_and_retry_schedules_converge() {
    let mut duplicates = 0;
    for seed in 1u64..=20 {
        let mut a = Author::new(1, "g");
        let mut b = Author::new(2, "g");
        let mut es = vec![];
        for i in 0..5 {
            es.push(a.post(&format!("a{i}")));
            es.push(b.post(&format!("b{i}")));
        }
        let mut oracle = Replica::new("g", &[&a, &b]);
        let mut r = Replica::new("g", &[&a, &b]);
        for e in &es {
            assert_eq!(oracle.receive(e.clone()), Outcome::Accepted);
        }
        let mut rng = seed;
        for _ in 0..80 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let i = ((rng >> 32) as usize) % es.len();
            if rng % 3 != 0 {
                let outcome = r.receive(es[i].clone());
                assert!(
                    matches!(
                        outcome,
                        Outcome::Accepted | Outcome::Duplicate | Outcome::Pending
                    ),
                    "seed={seed} valid envelope gave {outcome:?}"
                );
                duplicates += (outcome == Outcome::Duplicate) as usize;
            }
        }
        r.sync_from(&oracle);
        oracle.assert_same(&r);
        // Non-vacuity: every envelope accepted exactly once on both sides.
        assert_eq!((oracle.accepted.len(), r.accepted.len()), (10, 10));
        assert_eq!(r.fanout.len(), 10);

        // Negative control: withhold one envelope (one that has a later same-author post)
        // from BOTH the lossy schedule and the repair source. Repair must not converge.
        let withheld = (seed as usize) % 8;
        let source = oracle_without(&oracle, &es[withheld]);
        let mut partial = Replica::new("g", &[&a, &b]);
        let mut rng = seed;
        for _ in 0..80 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let i = ((rng >> 32) as usize) % es.len();
            if rng % 3 != 0 && i != withheld {
                partial.receive(es[i].clone());
            }
        }
        partial.sync_from(&source);
        // Same-author successors of the withheld post: (4 - withheld/2) of them.
        let blocked = 4 - withheld / 2;
        assert_eq!(partial.accepted.len(), 10 - 1 - blocked, "seed={seed}");
        assert_eq!(partial.pending.len(), blocked, "seed={seed}");
        assert!(!partial.accepted.contains_key(&es[withheld].id()));
        assert_ne!(partial.doc.get_deep_value(), oracle.doc.get_deep_value());
        // Releasing the withheld envelope drains pending and converges.
        assert_eq!(partial.receive(es[withheld].clone()), Outcome::Accepted);
        oracle.assert_same(&partial);
    }
    assert!(duplicates > 0, "no Duplicate outcome observed in any seed");
}
// KIND: invariant
#[test]
fn pending_dependencies_are_bounded() {
    let mut a = Author::new(1, "g");
    let withheld = a.post("withheld");
    let mut r = Replica::new("g", &[&a]);
    for _ in 0..MAX_PENDING {
        assert_eq!(r.receive(a.post("pending")), Outcome::Pending);
    }
    let overflow = a.post("overflow");
    let before = r.snapshot();
    assert_eq!(r.receive(overflow.clone()), Outcome::Limit);
    assert_eq!(r.pending.len(), MAX_PENDING);
    assert!(r.accepted.is_empty());
    // The refused envelope is not retained anywhere: state is byte-identical.
    assert!(!r.pending.contains_key(&overflow.id()));
    assert_eq!(r.snapshot(), before);
    // Releasing the dependency drains all 16 pending; the refused one stays absent
    // until it is resent, i.e. the sender (not the receiver) owns the retry.
    assert_eq!(r.receive(withheld), Outcome::Accepted);
    assert_eq!(r.accepted.len(), MAX_PENDING + 1);
    assert!(r.pending.is_empty());
    assert!(!r.accepted.contains_key(&overflow.id()));
    assert_eq!(r.receive(overflow), Outcome::Accepted);
    assert_eq!(r.accepted.len(), MAX_PENDING + 2);
}
