use super::*;

// KIND: invariant
#[test]
fn every_rejection_class_leaves_populated_signed_replica_byte_identical() {
    let (g, mut a, mut b, mut c) = signed_fixture();
    let mut r = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    let a1 = a.post("A");
    b.observe(&a1);
    assert_eq!(r.receive(a1), Outcome::Accepted);
    assert_eq!(r.receive(b.post("B saw A")), Outcome::Accepted);
    let withheld = b.post("withheld");
    assert_eq!(r.receive(b.post("pending on withheld")), Outcome::Pending);
    let original = c.post("attempt");
    let mut inputs = vec![];
    let mut e = original.clone();
    e.body.text = "tampered, not re-signed".into();
    inputs.push(e);
    for permit in [
        None,
        a.permit.clone(),
        Some(Grant::issue(
            &a.id,
            c.id.did(),
            &g.id(),
            &[Capability::Read],
            false,
            20,
            None,
        )),
        Some(Grant::issue(
            &a.id,
            c.id.did(),
            "other",
            &[Capability::Post],
            false,
            21,
            None,
        )),
    ] {
        let mut e = original.clone();
        e.body.permit = permit;
        resign(&mut e, &c.key);
        inputs.push(e);
    }
    let mut e = original.clone();
    c.doc.get_map("secret").insert("x", "injection").unwrap();
    c.doc.commit();
    e.body.update = c
        .doc
        .export(ExportMode::updates(&Default::default()))
        .unwrap();
    resign(&mut e, &c.key);
    inputs.push(e);
    for e in inputs {
        r.assert_rejected_unchanged(e);
    }
    // Positive control, then the pending envelope drains when its dependency arrives.
    assert_eq!(r.receive(original), Outcome::Accepted);
    assert_eq!(r.receive(withheld), Outcome::Accepted);
    assert_eq!((r.accepted.len(), r.pending.len()), (5, 0));
}

// KIND: limitation
#[test]
fn invalid_envelope_promoted_from_pending_is_dropped_without_surfaced_outcome() {
    let mut a = Author::new(1, "g");
    let mut b = Author::new(2, "g");
    let mut c = Author::new(3, "g");
    let a1 = a.post("a1");
    let c1 = c.post("c1");
    b.observe(&a1);
    // b1's Loro ops depend on a1, but it declares only c1: admissible-but-wrong closure.
    let mut b1 = b.post("b1 needs a1");
    b1.body.dependencies = [c1.id()].into();
    resign(&mut b1, &b.key);
    let mut r = Replica::new("g", &[&a, &b, &c]);
    assert_eq!(r.receive(b1.clone()), Outcome::Pending);
    assert_eq!(r.receive(a1), Outcome::Accepted);
    assert!(r.pending.contains_key(&b1.id()));
    // c1 makes b1 "ready"; apply() refuses it during the drain and the result is discarded.
    // The caller sees only c1's Accepted: no Invalid outcome is surfaced for b1.
    assert_eq!(r.receive(c1), Outcome::Accepted);
    assert!(!r.accepted.contains_key(&b1.id()));
    assert!(r.pending.is_empty());
    assert_eq!(r.fanout.len(), 2);
    assert_eq!(r.receive(b1), Outcome::Invalid);
}

// KIND: limitation
#[test]
fn bulk_repair_in_id_order_overflows_pending_bound_but_causal_order_does_not() {
    // Before G-H, Replica::sync_from and Fleet::reconcile sent missing envelopes in
    // envelope-ID (BTreeMap) order. A 60-post chain then hits MAX_PENDING and valid
    // envelopes are refused with Limit; the receiver does not retain them.
    let mut a = Author::new(1, "g");
    let chain: Vec<Envelope> = (0..60).map(|i| a.post(&format!("c{i}"))).collect();
    let mut by_id = chain.clone();
    by_id.sort_by_key(|e| e.id());
    let mut r = Replica::new("g", &[&a]);
    let limits = by_id
        .into_iter()
        .filter(|e| r.receive(e.clone()) == Outcome::Limit)
        .count();
    assert!(
        limits > 0,
        "id-order repair no longer reaches the pending bound"
    );
    assert!(r.accepted.len() < chain.len());
    // Fixed repair order (used by sync_from and Fleet::reconcile): no Pending, no Limit.
    let mut fixed = Replica::new("g", &[&a]);
    for e in causal_order(chain.clone()) {
        assert_eq!(fixed.receive(e), Outcome::Accepted);
    }
    let mut source = Replica::new("g", &[&a]);
    for e in &chain {
        source.receive(e.clone());
    }
    let mut synced = Replica::new("g", &[&a]);
    synced.sync_from(&source);
    source.assert_same(&synced);
    assert_eq!(synced.accepted.len(), 60);
}
