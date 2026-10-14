use super::*;

// KIND: invariant
#[test]
fn historical_expired_bindings_are_retained_but_never_used_as_fallback_routes() {
    let (g, a, _b, _c) = signed_fixture();
    let n = nomination(&a, &g.id());
    let old = evidence(&n, 10, "old", 10);
    let new = successor(&old, 11, "new", 20);
    let mut directory = HostDirectory::default();
    assert!(directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![old, new]
        })
    ));
    assert_eq!(directory.active(10).unwrap().device_id, 11);
    assert!(directory.active(20).is_none());
    assert_eq!(directory.history.len(), 2);
    // Even if an old endpoint is still listening, expiration cannot reactivate it.
    let mut network = Network::default();
    network.live.insert("old".into(), 10);
    let mut author = Author::new(4, "g");
    assert_eq!(
        network.connect(&directory, 20, author.post("no stale fallback")),
        Err(RouteFailure::NoActiveBinding)
    );
    assert!(network.queue.is_empty());
    // Positive controls (G-H): before t=20 the current binding routes once its endpoint
    // listens; while it does not, the reason is EndpointGone, never a fallback to `old`
    // (which is still listening, and still within its own retire_at at t=5).
    assert_eq!(
        network.connect(&directory, 5, author.post("old listening, new not")),
        Err(RouteFailure::EndpointGone)
    );
    network.live.insert("new".into(), 11);
    assert_eq!(
        network.connect(&directory, 15, author.post("routes to new")),
        Ok(())
    );
    assert_eq!(network.queue.len(), 1);
    assert_eq!(network.queue[0].0, "new");
}
// KIND: invariant
#[test]
fn every_group_proof_predecessor_is_checked_before_accepting_or_exporting_history() {
    let (g, a, _b, _c) = signed_fixture();
    let n = nomination(&a, &g.id());
    let root = evidence(&n, 10, "group-old", 10);
    let good = vec![root.clone(), successor(&root, 11, "group-new", 20)];
    let mut directory = HostDirectory::default();
    assert!(directory.install(&n, &MockRotation(VerifiedSuccession { path: good.clone() })));
    for index in 0..good.len() {
        let mut bad = good.clone();
        bad[index].audience = Audience::Private;
        bad[index].endpoint = "SECRET-private-endpoint".into();
        assert!(!directory.install(&n, &MockRotation(VerifiedSuccession { path: bad })));
        assert_eq!(directory.history.len(), 2);
        assert!(
            directory
                .history
                .iter()
                .all(|r| !r.endpoint.contains("SECRET"))
        );
        assert_eq!(directory.current.as_ref().unwrap().endpoint, "group-new");
    }
    let before = format!("{:?}{:?}", directory.history, directory.current);
    let mut wrong_group = good.clone();
    wrong_group[0].audience = Audience::Group("another-group".into());
    assert!(!directory.install(&n, &MockRotation(VerifiedSuccession { path: wrong_group })));
    // G-H: wrong group at the leaf and a mid-path owner mismatch are refused too, and no
    // refusal changes the installed directory.
    let mut wrong_group_leaf = good.clone();
    wrong_group_leaf[1].audience = Audience::Group("another-group".into());
    assert!(!directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: wrong_group_leaf
        })
    ));
    let mut other_owner = good.clone();
    other_owner[1].owner = "did:key:someone-else".into();
    assert!(!directory.install(&n, &MockRotation(VerifiedSuccession { path: other_owner })));
    assert_eq!(
        format!("{:?}{:?}", directory.history, directory.current),
        before
    );
}
// KIND: invariant
#[test]
fn replaceable_device_ids_route_messages_without_new_group_delegation() {
    let (g, mut a, b, _c) = signed_fixture();
    let n = nomination(&b, &g.id());
    let mut directory = HostDirectory::default();
    let mut network = Network::default();
    let old = evidence(&n, 10, "host-old", 10);
    let new = successor(&old, 11, "host-new", 30);
    assert!(directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![old.clone()]
        })
    ));
    network.live.insert("host-old".into(), 10);
    let mut host = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    let mut recipient = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    let first = a.post("queued before replacement");
    assert_eq!(network.connect(&directory, 5, first.clone()), Ok(()));
    // Endpoint retired before queued delivery: do not pretend it migrated automatically.
    network.live.remove("host-old");
    network.live.insert("host-new".into(), 11);
    assert_eq!(network.deliver(&mut host), 0);
    assert_eq!(
        network.connect(&directory, 11, first.clone()),
        Err(RouteFailure::NoActiveBinding)
    );
    assert!(directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![old, new]
        })
    ));
    assert_eq!(network.connect(&directory, 11, first.clone()), Ok(()));
    assert_eq!(network.deliver(&mut host), 1);
    let second = a.post("after replacement");
    assert_eq!(network.connect(&directory, 12, second), Ok(()));
    assert_eq!(network.deliver(&mut host), 1);
    recipient.sync_from(&host);
    recipient.assert_same(&host);
    assert_eq!(host.accepted[&first.id()].signature, first.signature);
    // No permit-chain extension for routing succession: same original message/authority.
    assert_eq!(host.accepted.len(), 2);
}
// KIND: invariant
#[test]
fn sibling_or_other_owner_path_does_not_inherit_device_nomination() {
    let (g, a, b, _c) = signed_fixture();
    let n = nomination(&a, &g.id());
    let mut directory = HostDirectory::default();
    let sibling = evidence(&n, 99, "sibling", 30);
    assert!(!directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![sibling]
        })
    ));
    let mut impostor = evidence(&n, 10, "other-owner", 30);
    impostor.owner = b.id.did().into();
    assert!(!directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![impostor]
        })
    ));
    assert!(directory.history.is_empty() && directory.current.is_none());
    // G-H: a path that starts at the nominated device but continues with a sibling (a
    // record not naming the previous device) is refused as a whole. Before G-H the mock
    // install checked only the first element and accepted this path.
    let root = evidence(&n, 10, "nominated", 30);
    let unlinked_sibling = evidence(&n, 99, "sibling", 40);
    assert!(!directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![root.clone(), unlinked_sibling]
        })
    ));
    let mut spliced = successor(&root, 99, "sibling", 40);
    spliced.predecessor = Some(98);
    assert!(!directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![root.clone(), spliced]
        })
    ));
    let mut rooted_mid_path = root.clone();
    rooted_mid_path.predecessor = Some(7);
    assert!(!directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![rooted_mid_path]
        })
    ));
    assert!(directory.history.is_empty() && directory.current.is_none());
    // Positive control: a linked successor is accepted.
    assert!(directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![root.clone(), successor(&root, 99, "successor", 40)]
        })
    ));
    assert_eq!(directory.current.as_ref().unwrap().device_id, 99);
}
// KIND: limitation
#[test]
fn offline_mutual_retirement_does_not_gain_magical_discovery() {
    let (g, mut a, b, _c) = signed_fixture();
    let n = nomination(&b, &g.id());
    let mut directory = HostDirectory::default();
    assert!(directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![evidence(&n, 10, "known-old", 10)]
        })
    ));
    let mut network = Network::default();
    network.live.insert("unknown-new".into(), 11);
    assert!(
        !network
            .connect(&directory, 11, a.post("pending until rediscovery"))
            .is_ok()
    );
    assert!(network.queue.is_empty());
    // G-H: tie the failure to retirement, not to a missing endpoint. Even with the known
    // endpoint still listening, at t=11 the reason is NoActiveBinding; at t=9 it routes.
    network.live.insert("known-old".into(), 10);
    assert_eq!(
        network.connect(&directory, 11, a.post("retired")),
        Err(RouteFailure::NoActiveBinding)
    );
    assert!(network.queue.is_empty());
    assert_eq!(
        network.connect(&directory, 9, a.post("before retirement")),
        Ok(())
    );
    // The new endpoint is live, yet nothing in the directory names it: no discovery.
    assert!(
        directory
            .history
            .iter()
            .all(|r| r.endpoint != "unknown-new")
    );
}

// KIND: invariant
#[test]
fn adapter_without_verified_path_installs_nothing_and_keeps_previous_binding() {
    let (g, a, _b, _c) = signed_fixture();
    let n = nomination(&a, &g.id());
    let mut directory = HostDirectory::default();
    assert!(!directory.install(&n, &NoVerifiedPath));
    assert!(directory.history.is_empty() && directory.current.is_none());
    let mut network = Network::default();
    let mut author = Author::new(4, "g");
    assert_eq!(
        network.connect(&directory, 0, author.post("nothing installed")),
        Err(RouteFailure::NoActiveBinding)
    );
    let root = evidence(&n, 10, "nominated", 30);
    assert!(directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![root.clone()]
        })
    ));
    assert!(!directory.install(&n, &NoVerifiedPath));
    assert_eq!(directory.current.as_ref().unwrap().endpoint, "nominated");
    assert_eq!(directory.history.len(), 1);
}
