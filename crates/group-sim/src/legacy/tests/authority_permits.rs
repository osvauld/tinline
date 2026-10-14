use super::*;
// KIND: invariant
#[test]
fn signed_genesis_and_delegated_permit_authorize_original_relay() {
    let (g, _a, _b, mut c) = signed_fixture();
    let mut h = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    let mut recipient = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    let e = c.post("delegated member");
    assert_eq!(h.receive(e.clone()), Outcome::Accepted);
    recipient.sync_from(&h);
    h.assert_same(&recipient);
    assert_eq!(recipient.accepted[&e.id()].body.author, c.id.did());
    let mut forged = g.clone();
    forged.signature[0] ^= 1;
    assert!(Replica::from_genesis(forged, &g.id()).is_none());
}
// KIND: invariant
#[test]
fn missing_wrong_holder_tampered_and_read_only_permits_cannot_post() {
    let (g, a, _b, mut c) = signed_fixture();
    let original = c.post("attempt");
    let mut inputs = Vec::new();
    let mut e = original.clone();
    e.body.permit = None;
    inputs.push(e);
    let mut e = original.clone();
    e.body.permit.as_mut().unwrap().signature[0] ^= 1;
    inputs.push(e);
    let mut e = original.clone();
    e.body.permit = a.permit.clone();
    inputs.push(e);
    let mut e = original.clone();
    e.body.permit = Some(Grant::issue(
        &a.id,
        c.id.did(),
        &g.id(),
        &[Capability::Read],
        false,
        20,
        None,
    ));
    inputs.push(e);
    let mut e = original.clone();
    e.body.permit = Some(Grant::issue(
        &a.id,
        c.id.did(),
        "other",
        &[Capability::Post],
        false,
        21,
        None,
    ));
    inputs.push(e);
    let mut r = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    for mut e in inputs {
        // Author re-signs: failures must come from authorization, not a stale outer signature.
        e.signature = c.key.sign(&e.bytes()).to_bytes();
        assert_eq!(r.receive(e), Outcome::Invalid);
        assert!(r.accepted.is_empty() && r.pending.is_empty() && r.fanout.is_empty());
    }
}
// KIND: invariant
#[test]
fn cross_author_causal_chain_converges_in_every_delivery_order() {
    let (g, mut a, mut b, mut c) = signed_fixture();
    let first = a.post("A");
    b.observe(&first);
    let second = b.post("B saw A");
    c.observe(&first);
    c.observe(&second);
    let third = c.post("C saw both");
    let es = [first, second, third];
    let mut oracle = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    for e in &es {
        assert_eq!(oracle.receive(e.clone()), Outcome::Accepted);
    }
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut r = Replica::from_genesis(g.clone(), &g.id()).unwrap();
        for i in order {
            r.receive(es[i].clone());
        }
        oracle.assert_same(&r);
        assert_eq!(r.fanout.len(), 3);
    }
    let mut incomplete = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    assert_eq!(incomplete.receive(es[2].clone()), Outcome::Pending);
    assert_eq!(incomplete.receive(es[1].clone()), Outcome::Pending);
    assert!(incomplete.accepted.is_empty() && incomplete.fanout.is_empty());
    assert_eq!(incomplete.receive(es[0].clone()), Outcome::Accepted);
    oracle.assert_same(&incomplete);
}
// KIND: invariant
#[test]
fn independent_posts_and_cross_author_join_survive_host_partition() {
    let (g, mut a, mut b, mut c) = signed_fixture();
    let first = a.post("left");
    let second = b.post("right");
    c.observe(&first);
    c.observe(&second);
    let join = c.post("both");
    let mut h1 = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    let mut h2 = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    assert_eq!(h1.receive(first), Outcome::Accepted);
    assert_eq!(h2.receive(second), Outcome::Accepted);
    assert_eq!(h1.receive(join), Outcome::Pending);
    h1.sync_from(&h2);
    h2.sync_from(&h1);
    h1.assert_same(&h2);
    assert_eq!(h1.accepted.len(), 3);
}
// KIND: invariant
#[test]
fn declared_dependency_cannot_substitute_for_actual_loro_causality() {
    let (g, mut a, mut b, _c) = signed_fixture();
    let first = a.post("needed");
    b.observe(&first);
    let mut next = b.post("dependent");
    next.body.dependencies.clear();
    next.signature = b.key.sign(&next.bytes()).to_bytes();
    let mut r = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    assert_eq!(r.receive(next.clone()), Outcome::Invalid);
    assert!(r.accepted.is_empty() && r.pending.is_empty() && r.fanout.is_empty());
    assert_eq!(r.receive(first), Outcome::Accepted);
    assert_eq!(r.receive(next), Outcome::Invalid);
    assert_eq!(r.accepted.len(), 1);
    assert_eq!(r.fanout.len(), 1);
}

// ---- G-H hardening: single-cause rejections on POPULATED replicas -------------------

// KIND: invariant
#[test]
fn group_only_and_doc_only_mismatches_are_rejected_without_state_change() {
    let mut a = Author::new(1, "g");
    let mut b = Author::new(2, "g");
    // X keeps the "g" document name but signs group "other"; Y keeps group "g" but writes
    // a different document shard (with a consistent peer ID for that shard).
    let mut x = Author::new(3, "g");
    x.group = "other".into();
    let mut y = Author::new(4, "g");
    y.doc_name = "group/g/2026-10-02".into();
    y.doc = LoroDoc::new();
    y.doc
        .set_peer_id(peer(&y.key.verifying_key().to_bytes(), &y.doc_name))
        .unwrap();
    let (mut r, _) = populated_replica(&mut a, &mut b, &[&x, &y]);
    r.assert_rejected_unchanged(x.post("group mismatch only"));
    r.assert_rejected_unchanged(y.post("doc mismatch only"));
    // Positive controls: same keys, matching group and doc.
    assert_eq!(
        r.receive(Author::new(3, "g").post("x ok")),
        Outcome::Accepted
    );
    assert_eq!(
        r.receive(Author::new(4, "g").post("y ok")),
        Outcome::Accepted
    );
}

// KIND: invariant
#[test]
fn size_bounds_and_sequence_zero_are_rejected_without_state_change() {
    let mut a = Author::new(1, "g");
    let mut b = Author::new(2, "g");
    let mut long = Author::new(5, "g");
    let mut deps = Author::new(6, "g");
    let mut zero = Author::new(7, "g");
    let mut big = Author::new(8, "g");
    let mut ok = Author::new(9, "g");
    let (mut r, _) = populated_replica(&mut a, &mut b, &[&long, &deps, &zero, &big, &ok]);
    // Text bound: sole cause (the update itself stays well under 1 MB).
    r.assert_rejected_unchanged(long.post(&"x".repeat(20_001)));
    assert_eq!(r.receive(ok.post(&"x".repeat(20_000))), Outcome::Accepted);
    // Dependency-count bound: sole cause. Without it, the envelope would go Pending.
    let fake = |n: usize| (0..n).map(|i| format!("missing-{i}")).collect();
    let mut e = deps.post("too many deps");
    e.body.dependencies = fake(257);
    resign(&mut e, &deps.key);
    r.assert_rejected_unchanged(e.clone());
    // Sequence zero and oversized update: rejected, but defense in depth only; apply()
    // would also refuse them (sequence/previous rule, Loro decode), so not sole cause.
    let mut e0 = zero.post("zero");
    e0.body.sequence = 0;
    resign(&mut e0, &zero.key);
    r.assert_rejected_unchanged(e0);
    let mut e1 = big.post("big");
    e1.body.update = vec![0; 1_000_001];
    resign(&mut e1, &big.key);
    r.assert_rejected_unchanged(e1);
    // Control for the dependency bound: 256 unknown deps is admissible but not ready.
    e.body.dependencies = fake(256);
    resign(&mut e, &deps.key);
    assert_eq!(r.receive(e), Outcome::Pending);
}
