use super::*;

// KIND: invariant
#[test]
fn private_endpoints_never_appear_in_host_state_or_any_group_relayed_bytes() {
    for policy in all_policies() {
        let mut w = world(policy);
        // A full mix of group and private rotations, an offline catch-up and a host rotation.
        w.nodes[D].online = false;
        for i in [A, C, HOST] {
            w.rotate_private(i);
            let n = w.rotate_group(i);
            w.announce(i, &n).unwrap();
            w.rotate_private(i);
        }
        w.nodes[D].online = true;
        // U7: the host's retry reaches dave at his still-live endpoint (no member forwarding).
        assert_eq!(w.host_push(D), Ok(true));
        w.catch_up(D).unwrap();
        w.rotate_private(D);
        let n = w.rotate_group(D);
        w.announce(D, &n).unwrap();
        assert!(!w.wire.is_empty() && !w.nodes[HOST].relayed.is_empty());
        // Positive control: the private endpoints do exist and the scan can see them.
        assert_eq!(w.private_endpoints_ever.len(), 4 + 7);
        for ep in &w.private_endpoints_ever {
            assert!(w.private_wire.iter().any(|f| contains(&f.bytes, ep)) || ep.ends_with("-1"));
        }
        let hay = w.group_bytes_scan();
        for bytes in &hay {
            assert!(!contains(bytes, "PRIVATE-"), "{policy:?}");
            for ep in &w.private_endpoints_ever {
                assert!(!contains(bytes, ep));
            }
        }
        // A buggy/hostile member pushes its private note at the host: refused, and host
        // state plus everything the host relays stays free of it. (The inbound frame itself
        // carries the leak; the host cannot un-send a sender's mistake.)
        let leak = RotationNote::sign(
            &w.nodes[C].author.id,
            NoteAudience::Private,
            &w.nodes[C].private_endpoint.clone(),
            w.nodes[C].private_seq,
        )
        .wire();
        let before = w.host_snapshot();
        assert_eq!(w.inject(C, HOST, &leak), Err(Reject::PrivateAudience));
        assert_eq!(w.host_snapshot(), before);
        assert!(!contains(&w.host_snapshot(), "PRIVATE-"));
        assert!(
            w.nodes[HOST]
                .relayed
                .iter()
                .all(|r| !contains(r, "PRIVATE-"))
        );
    }
}

// KIND: invariant
// R1b (U7): the host accepts members' group notes; a member accepts only the host's. Every
// malformed, forged, bit-flipped, truncated, stale, replayed, wrong-owner, non-member,
// private-audience or foreign note leaves the host's state (for alice's notes, sent by a
// member) and a member's view (for the host's notes, sent by a faulty relay) byte-identical.
// A genuine note of another member is refused by a member (`NotHost`), view unchanged.
#[test]
fn forged_stale_replayed_wrong_owner_private_and_foreign_notes_leave_host_and_member_unchanged() {
    for policy in all_policies() {
        let mut w = world(policy);
        let n2 = w.rotate_group(A);
        assert_eq!(w.announce(A, &n2), Ok(vec![]));
        let h2 = w.rotate_group(HOST);
        assert_eq!(w.announce(HOST, &h2), Ok(vec![A, C, D]));
        // (owner of the notes, receiver, sender, current note)
        for (owner, to, from, current) in [(A, HOST, D, n2.clone()), (HOST, C, HOST, h2)] {
            let id = w.nodes[owner].author.id.clone();
            let name = w.nodes[owner].name;
            let group = NoteAudience::Group(w.group.clone());
            let n1 = RotationNote::sign(&id, group.clone(), &format!("grp-{name}-1"), 1).wire();
            let mut forged: RotationNote = serde_json::from_slice(&current).unwrap();
            forged.claims.seq = 3;
            forged.claims.endpoint = format!("grp-{name}-hijack");
            let mut flipped: RotationNote = serde_json::from_slice(&current).unwrap();
            flipped.signature[7] ^= 1;
            let mut short_sig: RotationNote = serde_json::from_slice(&current).unwrap();
            short_sig.signature.truncate(63);
            let stale = RotationNote::sign(&id, group.clone(), "grp-rollback", 1).wire();
            let mut wrong_owner =
                RotationNote::sign(&w.nodes[D].author.id, group.clone(), "grp-x", 3);
            wrong_owner.claims.owner = w.did(owner);
            let outsider = Author::new(9, &w.group);
            let not_member = RotationNote::sign(&outsider.id, group.clone(), "grp-out-1", 1).wire();
            let private = RotationNote::sign(&id, NoteAudience::Private, "PRIVATE-x-9", 3).wire();
            let foreign =
                RotationNote::sign(&id, NoteAudience::Group("other".into()), "grp-o", 3).wire();
            let cases: Vec<(&str, Vec<u8>, Reject)> = vec![
                ("forged", forged.wire(), Reject::BadSignature),
                ("bit-flip", flipped.wire(), Reject::BadSignature),
                ("short-sig", short_sig.wire(), Reject::BadSignature),
                ("stale", stale, Reject::Stale),
                ("replay-current", current.clone(), Reject::Replay),
                ("replay-old", n1, Reject::Replay),
                ("wrong-owner", wrong_owner.wire(), Reject::BadSignature),
                ("not-member", not_member, Reject::NotMember),
                ("private", private, Reject::PrivateAudience),
                ("wrong-group", foreign, Reject::WrongGroup),
                ("malformed", b"{\"claims\":1}".to_vec(), Reject::Malformed),
            ];
            for (case, bytes, want) in cases {
                let snap = |w: &World| {
                    if to == HOST {
                        w.host_snapshot()
                    } else {
                        w.view_snapshot(to)
                    }
                };
                let before = snap(&w);
                assert_eq!(
                    w.inject(from, to, &bytes),
                    Err(want),
                    "{name} {case} {policy:?}"
                );
                assert_eq!(snap(&w), before, "{name} {case} mutated node {to}");
            }
        }
        let before = w.view_snapshot(C);
        assert_eq!(w.inject(HOST, C, &n2), Err(Reject::NotHost));
        assert_eq!(w.view_snapshot(C), before);
        // Positive controls on the same populated state.
        let n3 = w.rotate_group(A);
        assert_eq!(w.announce(A, &n3), Ok(vec![]));
        assert_eq!(w.known_endpoint(HOST, A), Some("grp-alice-3".into()));
        assert_eq!(w.known_endpoint(C, A), None);
        let h3 = w.rotate_group(HOST);
        assert_eq!(w.announce(HOST, &h3), Ok(vec![A, C, D]));
        assert_eq!(w.known_endpoint(C, HOST), Some("grp-host-3".into()));
    }
}

// KIND: invariant
// R1b (U7): Alice personally blocks Carol. The host still holds both their new group
// endpoints and reaches both; Carol still receives the host's rotation; no member holds
// another member's group endpoint in either run; every group view and the host relay log are
// byte-identical with and without the block. Only the private audience differs: Carol is
// left out of Alice's private note.
#[test]
fn personal_block_does_not_stop_group_notes_or_change_group_state() {
    for policy in all_policies() {
        let run = |block: bool| {
            let mut w = world(policy);
            if block {
                // Alice personally blocks carol (private audience only, U5 revised).
                let c = w.did(C);
                w.nodes[A].blocked.insert(c);
            }
            let private_recipients = w.rotate_private(A);
            for i in [A, C] {
                let n = w.rotate_group(i);
                assert_eq!(w.announce(i, &n), Ok(vec![]));
            }
            let n = w.rotate_group(HOST);
            let delivered = w.announce(HOST, &n).unwrap();
            let views: Vec<Vec<u8>> = (0..4).map(|i| w.view_snapshot(i)).collect();
            (w.host_snapshot(), views, delivered, private_recipients, w)
        };
        let (host_plain, views_plain, del_plain, priv_plain, _) = run(false);
        let (host_block, views_block, del_block, priv_block, w) = run(true);
        assert_eq!(del_block, vec![A, C, D]);
        assert_eq!(w.known_endpoint(C, HOST), Some("grp-host-2".into()));
        assert_eq!(w.known_endpoint(HOST, A), Some("grp-alice-2".into()));
        assert_eq!(w.known_endpoint(HOST, C), Some("grp-carol-2".into()));
        for (x, y) in [(C, HOST), (HOST, C), (A, HOST), (HOST, A)] {
            assert_eq!(w.reach(x, y), Ok(()));
        }
        assert_eq!(w.known_endpoint(C, A), None);
        assert_eq!(w.known_endpoint(A, C), None);
        assert_eq!(w.reach(C, A), Err(RouteErr::NoBinding));
        // Group state (roster, directories, host relay log) is byte-identical with the block.
        assert_eq!(del_plain, del_block);
        assert_eq!(host_plain, host_block);
        assert_eq!(views_plain, views_block);
        // Only the private audience differs: carol is excluded from alice's private note.
        assert!(priv_plain.contains(&w.did(C)));
        assert!(!priv_block.contains(&w.did(C)));
        assert_eq!(priv_block.len() + 1, priv_plain.len());
    }
}

// KIND: candidate-policy G-HISTORY
// R1b (U7): only the host holds members' group notes, so the history question is what a
// member's retry sends the host after the host missed several of its rotations. Alice rotates
// three times (seq 2..4) while the host is offline; when it returns, her retry sends her latest
// note only, or her full history (seq 1..4). A strict (contiguous) host cannot use a
// latest-only retry and keeps routing alice to her retired endpoint. Members' views are
// untouched in every case.
#[test]
fn catch_up_latest_only_vs_full_history_under_gap_policies() {
    let mut results = vec![];
    for policy in all_policies()
        .into_iter()
        .filter(|p| p.fork == ForkPolicy::KeepFirst)
    {
        let mut w = world(policy);
        w.nodes[HOST].online = false;
        for _ in 0..3 {
            let n = w.rotate_group(A);
            assert_eq!(w.announce(A, &n), Err(RouteErr::EndpointGone));
        }
        w.nodes[HOST].online = true;
        let host_before = w.view_snapshot(HOST);
        let members_before: Vec<Vec<u8>> = [C, D].iter().map(|&i| w.view_snapshot(i)).collect();
        let wire_before = w.wire.len();
        let (accepted, _) = w.catch_up(A).unwrap();
        let sent: Vec<&Frame> = w.wire[wire_before..]
            .iter()
            .filter(|f| f.from == w.did(A) && f.to == w.did(HOST))
            .collect();
        let bytes: usize = sent.iter().map(|f| f.bytes.len()).sum();
        let routed = w.reach(HOST, A);
        match (policy.catch_up, policy.gap) {
            (CatchUp::LatestOnly, GapPolicy::RequireContiguous) => {
                assert_eq!((accepted, sent.len()), (0, 1));
                assert_eq!(w.view_snapshot(HOST), host_before);
                assert_eq!(routed, Err(RouteErr::EndpointGone));
            }
            (CatchUp::LatestOnly, GapPolicy::AcceptGap) => {
                assert_eq!((accepted, sent.len()), (1, 1));
                assert_eq!(routed, Ok(()));
            }
            (CatchUp::FullHistory, _) => {
                // seq 1 is a replay at the host; seq 2..4 are new.
                assert_eq!((accepted, sent.len()), (3, 4));
                assert_eq!(routed, Ok(()));
            }
        }
        let members_after: Vec<Vec<u8>> = [C, D].iter().map(|&i| w.view_snapshot(i)).collect();
        assert_eq!(members_after, members_before);
        results.push((policy.catch_up, policy.gap, sent.len(), bytes));
    }
    // Full history costs more bytes on every retry and grows with rotation count.
    let latest = results
        .iter()
        .find(|r| r.0 == CatchUp::LatestOnly)
        .unwrap()
        .3;
    let full = results
        .iter()
        .find(|r| r.0 == CatchUp::FullHistory)
        .unwrap()
        .3;
    assert!(full > latest);
}

// KIND: candidate-policy G-FORK
// R1b: the two views are the group's two hosts (multi-host is decided; under U7 a member
// never holds alice's note), receiving the honest and the forked seq-2 note in opposite order.
#[test]
fn same_seq_fork_keep_first_diverges_by_arrival_order_freeze_converges_but_blocks_routing() {
    for fork in [ForkPolicy::KeepFirst, ForkPolicy::Freeze] {
        let policy = Policy {
            gap: GapPolicy::AcceptGap,
            fork,
            catch_up: CatchUp::FullHistory,
        };
        let w = world(policy);
        // A linked device / mnemonic clone of alice signs a different seq-2 note.
        let clone = Author::new(1, &w.group);
        assert_eq!(clone.id.did(), w.did(A));
        let g = NoteAudience::Group(w.group.clone());
        let honest = RotationNote::sign(&w.nodes[A].author.id, g.clone(), "grp-alice-2", 2).wire();
        let forked = RotationNote::sign(&clone.id, g.clone(), "grp-alice-clone-2", 2).wire();
        let mut host = w.nodes[HOST].view.clone();
        let mut host2 = w.nodes[HOST].view.clone();
        assert!(host.accept(policy, &honest).is_ok());
        assert_eq!(host.accept(policy, &forked), Err(Reject::ForkSameSeq));
        assert!(host2.accept(policy, &forked).is_ok());
        assert_eq!(host2.accept(policy, &honest), Err(Reject::ForkSameSeq));
        let a = w.did(A);
        match fork {
            ForkPolicy::KeepFirst => {
                // Arrival order picks the winner: the two hosts route alice differently.
                assert_eq!(host.route(&a).unwrap().claims.endpoint, "grp-alice-2");
                assert_eq!(
                    host2.route(&a).unwrap().claims.endpoint,
                    "grp-alice-clone-2"
                );
            }
            ForkPolicy::Freeze => {
                // Order-independent, but alice's group path is frozen everywhere: any
                // mnemonic holder can DoS it. Later honest notes are refused too.
                assert!(host.route(&a).is_none() && host2.route(&a).is_none());
                assert_eq!(host.frozen, host2.frozen);
                let n3 = RotationNote::sign(&w.nodes[A].author.id, g, "grp-alice-3", 3).wire();
                let before = serde_json::to_vec(&host).unwrap();
                assert_eq!(host.accept(policy, &n3), Err(Reject::Frozen));
                assert_eq!(serde_json::to_vec(&host).unwrap(), before);
            }
        }
    }
}

// KIND: limitation
// R1b (U7): a mnemonic clone of alice signs a higher-seq group note; the host cannot tell it
// from alice's and now routes alice's group traffic to the clone. Members never held alice's
// group endpoint, so the effect is confined to what hosts route. Alice's next honest note
// then looks like a fork and is refused; nothing here can tell which device is the owner's.
#[test]
fn mnemonic_clone_with_higher_seq_replaces_honest_group_endpoint_at_the_host() {
    let mut w = world(all_policies()[0]);
    let clone = Author::new(1, &w.group);
    let g = NoteAudience::Group(w.group.clone());
    let n2 = w.rotate_group(A);
    w.announce(A, &n2).unwrap();
    // The clone holds the same DID key: its seq-3 note is indistinguishable from the owner's.
    let hostile = RotationNote::sign(&clone.id, g, "grp-clone-3", 3).wire();
    w.live.insert("grp-clone-3".into(), w.did(A));
    assert_eq!(w.inject(D, HOST, &hostile).map(|n| n.claims.seq), Ok(3));
    assert_eq!(w.known_endpoint(HOST, A), Some("grp-clone-3".into()));
    assert_eq!(w.reach(HOST, A), Ok(()));
    for i in [C, D] {
        assert_eq!(w.known_endpoint(i, A), None);
    }
    // Honest alice still believes she is at seq 2; her next rotation (seq 3) is a fork.
    let honest3 = w.rotate_group(A);
    let before = w.host_snapshot();
    assert_eq!(w.inject(A, HOST, &honest3), Err(Reject::ForkSameSeq));
    assert_eq!(w.host_snapshot(), before);
}
