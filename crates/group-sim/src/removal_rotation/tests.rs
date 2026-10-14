use super::*;

// ---- tests ---------------------------------------------------------------------------------

// KIND: invariant
// U12 detail (decided): refusal is immediate. In the same tick in which the hosts derive the
// removal, and long before any rotation, every honest host refuses carol's NEW post (it cites
// the removal in its signed frontier) and her call request. Members refuse the post too. Every
// refusal leaves the refusing device's state byte-identical, and the callee never hears of
// the call. Control: before the removal carol's call to erin is relayed; after it bob's is.
#[test]
fn removal_refuses_new_posts_and_calls_immediately_at_every_host() {
    let mut w = world();
    w.advance(T0);
    let carol = w.did(CAROL);
    assert_eq!(w.call(CAROL, H1, ERIN), Ok(()), "control: member may call");
    remove(&mut w, CAROL);
    assert_eq!(w.now, T0, "no time has passed");
    for h in [H1, H2] {
        assert!(w.d[h].rep.der.removed.contains(&carol), "host {h}");
        assert_eq!(w.d[h].sched.as_ref().unwrap().learned[&carol], T0);
        assert_eq!(note_seq(&w, h), 1, "not rotated yet");
    }
    assert!(w.d[CAROL].rep.der.removed.contains(&carol), "told");

    let p = w.post(CAROL, "carol after removal");
    w.d[CAROL].outbox.clear();
    for i in with_hosts(remaining(&[CAROL])) {
        let before = w.snap(i);
        assert_eq!(
            w.accept_post(i, p.clone()),
            Verdict::Rejected(Why::NotMember),
            "device {i}"
        );
        assert_eq!(w.snap(i), before, "device {i}");
    }
    let q = w.post(CAROL, "carol after removal, via link");
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    assert_eq!(w.d[CAROL].refused, vec![(q, Why::NotMember)]);

    for h in [H1, H2] {
        let (bh, be) = (w.snap(h), w.snap(ERIN));
        let n = w.frames.len();
        assert_eq!(w.call(CAROL, h, ERIN), Err(Refuse::Removed), "host {h}");
        assert_eq!(w.snap(h), bh, "host {h}");
        assert_eq!(w.snap(ERIN), be);
        assert!(w.frames[n..].iter().all(|f| f.to != ERIN), "no ring");
    }
    assert_eq!(w.call(BOB, H1, ERIN), Ok(()), "control: bob still may call");
    for i in with_hosts(remaining(&[CAROL])) {
        assert!(!w.texts(i).iter().any(|t| t.starts_with("carol after")));
    }
}

// KIND: invariant
// U12 (decided): after the removal every host signs a "rotating soon" announcement and sends
// it to the REMAINING members only (both hosts, all six, none to carol). Each member verifies
// it and records rotate_at = removal + 2 days. One tick before the deadline nothing has
// rotated and carol can still dial the hosts. At the deadline each host rotates exactly once,
// dials every remaining member itself and hands over its new record; members send nothing
// and do not rotate. Afterwards carol reaches nobody: every endpoint she ever learned is
// dead, no frame reaches her, and the hosts hold no address for her. The group keeps working.
#[test]
fn hosts_announce_to_remaining_members_only_then_rotate_after_two_days_and_reconnect_them() {
    let mut w = world();
    w.advance(T0);
    let carol = w.did(CAROL);
    let rest = remaining(&[CAROL]);
    let n0 = w.frames.len();
    remove(&mut w, CAROL);
    for h in [H1, H2] {
        let to: BTreeSet<usize> = w.frames[n0..]
            .iter()
            .filter(|f| f.kind == "rot-announce" && f.from == h)
            .map(|f| f.to)
            .collect();
        assert_eq!(
            to,
            rest.iter().copied().collect::<BTreeSet<usize>>(),
            "host {h}"
        );
    }
    assert!(
        !w.frames
            .iter()
            .any(|f| f.kind == "rot-announce" && f.to == CAROL)
    );
    assert!(!has(&w.received(CAROL), b"rotate_at"));
    assert!(w.d[CAROL].rep.announced.is_empty());
    for &m in &rest {
        assert_eq!(w.d[m].rep.announced.len(), 2, "member {m}");
        for a in w.d[m].rep.announced.values() {
            assert_eq!(a.body.rotate_at, T0 + ROTATION_DELAY);
            assert_eq!(a.body.removed, BTreeSet::from([carol.clone()]));
        }
    }

    w.advance(T0 + ROTATION_DELAY - 1);
    let old: Vec<String> = [H1, H2].iter().map(|&h| w.gep(h)).collect();
    for (k, h) in [H1, H2].into_iter().enumerate() {
        assert_eq!(note_seq(&w, h), 1);
        assert_eq!(w.dial(CAROL, &old[k]), Ok(h), "still live during the delay");
    }

    let n1 = w.frames.len();
    w.advance(T0 + ROTATION_DELAY);
    for (k, h) in [H1, H2].into_iter().enumerate() {
        assert_eq!(note_seq(&w, h), 2, "host {h} rotated once");
        assert_eq!(w.d[h].sched.as_ref().unwrap().done.len(), 1);
        assert_eq!(w.dial(BOB, &old[k]), Err(RouteErr::EndpointGone));
        for &m in &rest {
            assert_eq!(
                w.d[m].rep.hosts.get(&w.hk(h)),
                w.d[h].host.as_ref(),
                "member {m} reconnected to host {h} by the host"
            );
        }
    }
    for &m in &rest {
        assert_eq!(note_seq(&w, m), 1, "member {m} did not rotate");
    }
    assert!(
        w.frames[n1..].iter().all(|f| w.hosts.contains(&f.from)),
        "members did nothing"
    );
    assert!(w.frames[n1..].iter().all(|f| f.to != CAROL));

    w.assert_reaches_nobody(CAROL);
    w.post(BOB, "after rotation");
    w.round();
    w.assert_converged(&with_hosts(rest));
    assert!(w.texts(H2).contains("after rotation"));
    assert!(!w.texts(CAROL).contains("after rotation"));
}

// KIND: invariant
// U11 + U12 (decided): carol wrote a post while cut off, before the flags (its signed frontier
// predates the removal). One day into the delay she dials H1: she is told, and her queued
// post is uploaded and kept, and reaches every remaining member. A post citing the removal is
// refused. After the hosts rotate, a backdated post (pre-removal frontier) cannot be
// delivered at all: every host address she knows is dead, it stays in her outbox, and no
// other device ever holds it.
#[test]
fn pre_removal_posts_accepted_until_hosts_rotate_then_backdated_post_undeliverable() {
    let mut w = world();
    w.advance(T0);
    let carol = w.did(CAROL);
    let rest = with_hosts(remaining(&[CAROL]));
    w.cut(CAROL, H1);
    w.cut(CAROL, H2);
    w.post(CAROL, "offline before removal");
    remove(&mut w, CAROL);
    assert!(!w.d[CAROL].rep.der.removed.contains(&carol), "not told yet");

    w.advance(T0 + TICKS_PER_DAY);
    w.heal(CAROL, H1);
    w.heal(CAROL, H2);
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    assert!(w.d[CAROL].rep.der.removed.contains(&carol), "told");
    assert!(w.d[CAROL].outbox.is_empty() && w.d[CAROL].refused.is_empty());
    let q = w.post(CAROL, "cites the removal");
    assert_eq!(w.connect(CAROL, H2), Err(RouteErr::Removed));
    assert_eq!(w.d[CAROL].refused, vec![(q, Why::NotMember)]);
    w.round();
    for &i in &rest {
        assert!(w.texts(i).contains("offline before removal"), "device {i}");
        assert!(!w.texts(i).contains("cites the removal"), "device {i}");
    }

    w.advance(T0 + ROTATION_DELAY);
    let b = w.post_citing(CAROL, "backdated after rotation", BTreeSet::new());
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Lost));
    assert_eq!(w.connect(CAROL, H2), Err(RouteErr::Lost));
    w.round();
    assert_eq!(w.d[CAROL].outbox, vec![b.clone()], "never uploaded");
    for &i in &rest {
        assert!(!w.d[i].rep.store.contains_key(&b.id()), "device {i}");
    }
    w.assert_reaches_nobody(CAROL);
}

// KIND: candidate-policy G-HISTORY
// U11 open: bound the removed member's pre-removal uploads? Carol wrote 3 honest posts while
// cut off before the flags. During the delay she is told, then signs 2 backdated posts with
// the same (pre-removal) frontier. Honest and backdated posts have the identical signed shape.
// - RotationOnly: all 5 are kept everywhere; only the rotation ends the window.
// - Count(2): each host keeps the first 2 (both honest) and refuses the rest with UploadCap,
//   state unchanged; H2 receives the same 2 by host sync and also refuses the 3rd. The cap
//   cannot tell honest backlog from backdating, so honest post 3 is lost too. The cap is
//   enforced by hosts only: a member handed post 3 directly accepts it (signed frontier).
#[test]
fn count_cap_on_pre_removal_uploads_vs_rotation_only() {
    for cap in [UploadCap::RotationOnly, UploadCap::Count(2)] {
        let mut w = with(|c| c.cap = cap);
        w.advance(T0);
        w.cut(CAROL, H1);
        w.cut(CAROL, H2);
        let honest: Vec<Post> = (1..=3)
            .map(|k| w.post(CAROL, &format!("honest {k}")))
            .collect();
        remove(&mut w, CAROL);
        w.advance(T0 + TICKS_PER_DAY);
        w.heal(CAROL, H1);
        assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
        let back: Vec<Post> = (1..=2)
            .map(|k| w.post_citing(CAROL, &format!("backdated {k}"), BTreeSet::new()))
            .collect();
        assert_eq!(honest[0].body.ops_seen, back[0].body.ops_seen, "same shape");
        assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
        w.round();
        let carol_texts = |w: &World, i: usize| {
            w.texts(i)
                .into_iter()
                .filter(|t| t.starts_with("honest") || t.starts_with("backdated"))
                .count()
        };
        match cap {
            UploadCap::RotationOnly => {
                assert!(w.d[CAROL].refused.is_empty());
                for i in with_hosts(remaining(&[CAROL])) {
                    assert_eq!(carol_texts(&w, i), 5, "device {i}");
                }
            }
            UploadCap::Count(n) => {
                assert_eq!(n, 2);
                let why: Vec<Why> = w.d[CAROL].refused.iter().map(|(_, y)| *y).collect();
                assert_eq!(why, vec![Why::UploadCap; 3]);
                for i in with_hosts(remaining(&[CAROL])) {
                    assert_eq!(
                        w.texts(i),
                        w.texts(H1),
                        "device {i} holds what the hosts kept"
                    );
                    assert!(w.texts(i).contains("honest 2"));
                    assert!(!w.texts(i).contains("honest 3"), "honest backlog lost");
                }
                for h in [H1, H2] {
                    let before = w.snap(h);
                    assert_eq!(
                        w.accept_post(h, honest[2].clone()),
                        Verdict::Rejected(Why::UploadCap),
                        "host {h}"
                    );
                    assert_eq!(w.snap(h), before, "host {h}");
                }
                assert_eq!(w.accept_post(ERIN, honest[2].clone()), Verdict::Accepted);
            }
        }
    }
}

// KIND: candidate-policy G-TIME
// U12 open: several removals inside one delay window. Carol is removed at T0; erin (who got
// carol's announcement as a member) is cut off, writes an offline post, and is removed at
// T0+1d. At T0+2d+1h erin and carol try to upload:
// - KeepFirstDeadline: one rotation at T0+2d covering both; erin's window was only 1 day.
// - ExtendToLast: one rotation at T0+3d covering both; both keep a window (carol's grows).
// - PerRemoval: rotations at T0+2d and T0+3d. The FIRST already cuts erin off (removed
//   members are not reconnected), so the second rotation adds cost but no window.
// In all three, both removed members reach nobody at the end.
#[test]
fn several_removals_in_one_window_keep_first_vs_extend_vs_one_per_removal() {
    for b in [
        Batching::KeepFirstDeadline,
        Batching::ExtendToLast,
        Batching::PerRemoval,
    ] {
        let mut w = with(|c| c.batching = b);
        w.advance(T0);
        let (carol, erin) = (w.did(CAROL), w.did(ERIN));
        remove(&mut w, CAROL);
        for a in w.d[ERIN].rep.announced.values() {
            assert_eq!(a.body.rotate_at, T0 + ROTATION_DELAY, "erin was a member");
        }
        w.advance(T0 + TICKS_PER_DAY);
        w.cut(ERIN, H1);
        w.cut(ERIN, H2);
        let e1 = w.post(ERIN, "erin offline");
        remove(&mut w, ERIN);
        w.advance(T0 + ROTATION_DELAY);
        w.advance(T0 + ROTATION_DELAY + TICKS_PER_HOUR);
        w.heal(ERIN, H1);
        w.heal(ERIN, H2);
        let erin_try = w.connect(ERIN, H1);
        let carol_try = w.connect(CAROL, H1);
        w.advance(T0 + ROTATION_DELAY + TICKS_PER_DAY);
        let done = &w.d[H1].sched.as_ref().unwrap().done;
        let both = BTreeSet::from([carol.clone(), erin.clone()]);
        match b {
            Batching::KeepFirstDeadline => {
                assert_eq!(*done, vec![(T0 + ROTATION_DELAY, both)]);
                assert_eq!(erin_try, Err(RouteErr::Lost));
                assert_eq!(carol_try, Err(RouteErr::Lost));
                assert!(!w.d[H1].rep.store.contains_key(&e1.id()));
            }
            Batching::ExtendToLast => {
                assert_eq!(*done, vec![(T0 + ROTATION_DELAY + TICKS_PER_DAY, both)]);
                assert_eq!(erin_try, Err(RouteErr::Removed));
                assert_eq!(carol_try, Err(RouteErr::Removed), "carol's window grew");
                assert!(w.d[H1].rep.store.contains_key(&e1.id()));
            }
            Batching::PerRemoval => {
                assert_eq!(
                    *done,
                    vec![
                        (T0 + ROTATION_DELAY, BTreeSet::from([carol.clone()])),
                        (
                            T0 + ROTATION_DELAY + TICKS_PER_DAY,
                            BTreeSet::from([erin.clone()])
                        ),
                    ]
                );
                assert_eq!(erin_try, Err(RouteErr::Lost), "cut by the first rotation");
                assert_eq!(carol_try, Err(RouteErr::Lost));
                assert!(!w.d[H1].rep.store.contains_key(&e1.id()));
            }
        }
        let rotations = w.d[H1].sched.as_ref().unwrap().done.len() as u64;
        assert_eq!(note_seq(&w, H1), 1 + rotations);
        w.assert_reaches_nobody(CAROL);
        w.assert_reaches_nobody(ERIN);
    }
}

// KIND: invariant
// U12 (decided: the host reconnects members itself). Erin is offline from before the removal
// until after the rotation, so she never gets the announcement. Back online, the host address
// she holds is dead and she cannot reach a host herself. Each host dials her directory
// endpoint (unchanged: members do not rotate), hands over its new record and syncs; she
// learns the removal, receives the posts she missed, and converges with the group. The
// expired announcement is not delivered.
#[test]
fn member_offline_for_whole_delay_is_reconnected_by_the_host_afterwards() {
    let mut w = world();
    w.advance(T0);
    let carol = w.did(CAROL);
    w.d[ERIN].online = false;
    remove(&mut w, CAROL);
    w.post(BOB, "during the delay");
    w.round();
    assert!(w.d[ERIN].rep.announced.is_empty());
    w.advance(T0 + ROTATION_DELAY);
    w.d[ERIN].online = true;
    w.advance(T0 + ROTATION_DELAY + TICKS_PER_HOUR);
    for h in [H1, H2] {
        let e = w.host_ep(ERIN, h).unwrap();
        assert_eq!(w.dial(ERIN, &e), Err(RouteErr::EndpointGone), "host {h}");
    }
    for h in [H1, H2] {
        assert_eq!(w.connect_from_host(h, ERIN), Ok(Path::HostDialedMember));
        assert_eq!(w.d[ERIN].rep.hosts.get(&w.hk(h)), w.d[h].host.as_ref());
    }
    assert!(w.d[ERIN].rep.der.removed.contains(&carol));
    assert!(w.texts(ERIN).contains("during the delay"));
    assert!(w.d[ERIN].rep.announced.is_empty(), "expired, not delivered");
    assert_eq!(note_seq(&w, ERIN), 1);
    w.post(ERIN, "erin is back");
    w.round();
    w.assert_converged(&with_hosts(remaining(&[CAROL])));
}

// KIND: invariant
// U10 + U12: every member verifies every announcement. Twelve bad announcements are refused
// at every remaining member with state byte-identical: malformed; rotate_at changed after
// signing; signed by judy but naming H1; judy with H1's permit; judy with a self-issued
// permit; an admin's member grant used as a host permit; judy with a genuine owner permit but
// no host record any member accepted; wrong group; slot not matching the permit; naming a DID
// that is not removed; citing an unknown op; and a replay of the accepted announcement.
// A genuine newer announcement is accepted, and the older one replayed after it is Stale.
// Carol never receives an announcement; handed one, she refuses it (Removed), unchanged.
#[test]
fn forged_replayed_or_misdirected_announcements_refused_unchanged_never_sent_to_removed_member() {
    let mut w = world();
    w.advance(T0);
    remove(&mut w, CAROL);
    let rest = remaining(&[CAROL]);
    let g = w.d[H1].ann.clone().unwrap();
    for &m in &rest {
        assert_eq!(w.d[m].rep.announced.get(&w.hk(H1)), Some(&g), "member {m}");
    }
    let h1_permit = g.permit.clone();
    let judy = w.d[H3].id.clone();
    let mut tampered = g.clone();
    tampered.body.rotate_at += TICKS_PER_DAY;
    let mut as_judy = g.body.clone();
    as_judy.host = w.did(H3);
    as_judy.slot = w.d[H3].slot;
    let self_permit = MGrant::issue(
        &judy,
        &w.ctx.gid,
        &w.did(H3),
        Some(w.d[H3].slot),
        &[Cap::Host],
        false,
        1,
        None,
    );
    let mut as_alice = g.body.clone();
    as_alice.host = w.did(A1);
    as_alice.slot = w.d[A1].slot;
    let alice_grant = w.ctx.grants[&w.did(A1)].clone();
    let owner_permit = w.permit(H3, 99);
    let h1 = w.d[H1].id.clone();
    let resign = |f: &dyn Fn(&mut AnnBody)| {
        let mut b = g.body.clone();
        b.seq = 2;
        f(&mut b);
        Announce::sign(&h1, b, h1_permit.clone()).wire()
    };
    let judy_did = w.did(H3);
    let cases: Vec<(&str, Vec<u8>, Refuse)> = vec![
        (
            "malformed",
            b"not an announcement".to_vec(),
            Refuse::Malformed,
        ),
        ("tampered", tampered.wire(), Refuse::BadSignature),
        (
            "judy as H1",
            Announce::sign(&judy, g.body.clone(), h1_permit.clone()).wire(),
            Refuse::BadSignature,
        ),
        (
            "judy, H1 permit",
            Announce::sign(&judy, as_judy.clone(), h1_permit.clone()).wire(),
            Refuse::PermitInvalid,
        ),
        (
            "judy, self permit",
            Announce::sign(&judy, as_judy.clone(), self_permit).wire(),
            Refuse::PermitInvalid,
        ),
        (
            "admin grant",
            Announce::sign(&w.d[A1].id, as_alice, alice_grant).wire(),
            Refuse::PermitInvalid,
        ),
        (
            "unknown host",
            Announce::sign(&judy, as_judy, owner_permit).wire(),
            Refuse::NotHost,
        ),
        (
            "wrong group",
            resign(&|b| b.group = "other".into()),
            Refuse::WrongGroup,
        ),
        (
            "slot",
            resign(&|b| b.slot = b.slot.wrapping_add(1)),
            Refuse::DeviceMismatch,
        ),
        (
            "not removed",
            resign(&|b| b.removed = BTreeSet::from([judy_did.clone()])),
            Refuse::NotRemoved,
        ),
        (
            "unknown op",
            resign(&|b| b.cites = BTreeSet::from(["00".repeat(32)])),
            Refuse::Unknown,
        ),
        ("replay", g.wire(), Refuse::Stale),
    ];
    for (name, bytes, want) in &cases {
        for &m in &rest {
            let before = w.snap(m);
            assert_eq!(w.accept_announce(m, bytes), Err(*want), "{name} at {m}");
            assert_eq!(w.snap(m), before, "{name} at {m}");
        }
    }
    let newer = resign(&|_| {});
    assert_eq!(w.accept_announce(BOB, &newer), Ok(()));
    let before = w.snap(BOB);
    assert_eq!(w.accept_announce(BOB, &g.wire()), Err(Refuse::Stale));
    assert_eq!(w.snap(BOB), before);

    let before = w.snap(CAROL);
    assert_eq!(w.accept_announce(CAROL, &g.wire()), Err(Refuse::Removed));
    assert_eq!(w.snap(CAROL), before);
    w.advance(T0 + ROTATION_DELAY);
    assert!(
        !w.frames
            .iter()
            .any(|f| f.kind == "rot-announce" && f.to == CAROL)
    );
}

// KIND: limitation
// U12 accepted cost: during the 2-day delay carol still holds LIVE host addresses. She can
// dial both hosts at will; each dial gets only the removal notice (no records, posts or
// announcements) and leaves host state unchanged. Under U11 she can also sign NEW posts with
// a pre-removal frontier (D02 backdating); hosts cannot tell them from honest offline posts,
// so they are kept and relayed to every member. Only the rotation ends this.
#[test]
fn during_delay_removed_member_still_reaches_live_hosts_and_backdated_posts_are_kept() {
    let mut w = world();
    w.advance(T0);
    remove(&mut w, CAROL);
    w.advance(T0 + TICKS_PER_DAY);
    for h in [H1, H2] {
        let e = w.host_ep(CAROL, h).unwrap();
        assert_eq!(w.dial(CAROL, &e), Ok(h), "live address");
        let before = w.snap(h);
        let n = w.frames.len();
        for _ in 0..3 {
            assert_eq!(w.connect(CAROL, h), Err(RouteErr::Removed));
        }
        assert_eq!(w.snap(h), before, "host {h} unchanged");
        assert!(
            w.frames[n..]
                .iter()
                .filter(|f| f.to == CAROL)
                .all(|f| f.kind == "removal-notice")
        );
    }
    let b = w.post_citing(CAROL, "backdated during delay", BTreeSet::new());
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    assert!(w.d[CAROL].refused.is_empty());
    w.round();
    for i in with_hosts(remaining(&[CAROL])) {
        assert!(w.d[i].rep.store.contains_key(&b.id()), "device {i}");
    }
    w.advance(T0 + ROTATION_DELAY);
    w.assert_reaches_nobody(CAROL);
}

// KIND: candidate-policy G-DISCOVERY
// U12 open: does a member removal also rotate members' own group endpoints? Under U7 carol
// never learned any member's group endpoint (only hosts hold them), so after the host
// rotation she reaches nobody under both options. MembersToo only adds cost: every
// remaining member rotates once and sends 2 notes (one per host); HostsOnly sends none.
#[test]
fn member_endpoints_hosts_only_vs_members_also_rotate() {
    for mr in [MemberRotation::HostsOnly, MemberRotation::MembersToo] {
        let mut w = with(|c| c.members = mr);
        w.advance(T0);
        let rest = remaining(&[CAROL]);
        let n0 = w.frames.len();
        remove(&mut w, CAROL);
        w.advance(T0 + ROTATION_DELAY);
        let member_notes = w.frames[n0..]
            .iter()
            .filter(|f| f.kind == "ep-note" && MEMBERS.contains(&f.from))
            .count();
        for &m in &rest {
            let want = if mr == MemberRotation::MembersToo {
                2
            } else {
                1
            };
            assert_eq!(note_seq(&w, m), want, "member {m}");
        }
        match mr {
            MemberRotation::HostsOnly => assert_eq!(member_notes, 0),
            MemberRotation::MembersToo => assert_eq!(member_notes, rest.len() * 2),
        }
        let hay = w.received(CAROL);
        for &m in &rest {
            assert!(!has(&hay, w.gep(m).as_bytes()), "member {m} current");
        }
        for f in &w.frames {
            if let Ok(n) = serde_json::from_slice::<EpNote>(&f.bytes) {
                let member = MEMBERS.iter().any(|&m| w.did(m) == n.claims.owner);
                if member && n.claims.owner != w.did(CAROL) {
                    assert!(!has(&hay, n.claims.endpoint.as_bytes()), "any member note");
                }
            }
        }
        w.assert_reaches_nobody(CAROL);
        w.post(BOB, "still works");
        w.round();
        w.assert_converged(&with_hosts(rest));
    }
}

// KIND: limitation
// Each host runs the 2-day delay from when IT derives the removal. H2 is offline at the
// removal and learns it by host sync one day later, so it rotates one day after H1. Between
// the two rotations carol can no longer reach H1 but still reaches H2, where her backdated
// post is kept; host sync then carries it to H1 and every member. The group's effective
// upload cut-off is the LAST host's rotation, not the first.
#[test]
fn host_that_learns_removal_later_rotates_later_so_cut_off_is_the_last_host() {
    let mut w = world();
    w.advance(T0);
    let carol = w.did(CAROL);
    w.d[H2].online = false;
    remove(&mut w, CAROL);
    assert!(!w.d[H2].rep.der.removed.contains(&carol));
    w.advance(T0 + TICKS_PER_DAY);
    w.d[H2].online = true;
    assert_eq!(w.host_sync(H1, H2), Ok(()));
    assert_eq!(
        w.d[H2].sched.as_ref().unwrap().learned[&carol],
        T0 + TICKS_PER_DAY
    );
    w.advance(T0 + ROTATION_DELAY + TICKS_PER_HOUR);
    assert_eq!((note_seq(&w, H1), note_seq(&w, H2)), (2, 1));
    let b = w.post_citing(CAROL, "backdated between rotations", BTreeSet::new());
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Lost));
    assert_eq!(w.connect(CAROL, H2), Err(RouteErr::Removed));
    w.round();
    for i in with_hosts(remaining(&[CAROL])) {
        assert!(w.d[i].rep.store.contains_key(&b.id()), "device {i}");
    }
    w.advance(T0 + TICKS_PER_DAY + ROTATION_DELAY);
    assert_eq!(note_seq(&w, H2), 2);
    w.assert_reaches_nobody(CAROL);
}
