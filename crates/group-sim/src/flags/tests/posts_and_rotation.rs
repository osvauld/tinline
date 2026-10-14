use super::*;

// KIND: candidate-policy D01
// Section 8 Q8 asks about posts around the 3rd flag (= D01/D02). This test applies the
// SIGNED FRONTIER rule. Over all 720 orders of q1..q3 and the three flags, there is one
// outcome: q1, q2 and q3 are all kept, never retroactively.
// - q3 is backdated: its signed fields are shaped exactly like honest q2's, so it is kept.
//   That is the D02 limitation of this rule.
// - q4 cites the removal and is refused, state unchanged.
// - World part: in the U7 topology a removed member cannot upload at all, because honest
//   hosts refuse her link. Her honest offline post therefore never reaches the group, even
//   though any replica handed that post accepts it under this rule.
#[test]
fn posts_around_third_flag_signed_frontier_keeps_concurrent_and_backdated_posts() {
    let (w, t) = d01(PostRule::SignedFrontier);
    let carol = w.did(CAROL);
    let mut x = explore(&w.ctx, &t.events());
    assert_eq!(x.orders, 720);
    assert_eq!(x.finals.len(), 1);
    assert!(!x.retro_post);
    assert!(x.last.der.removed.contains(&carol));
    assert_eq!(
        x.last.board.texts(),
        set(&["q1 seen by alice", "q2 honest offline", "q3 backdated"])
    );
    assert_eq!(t.q[1].body.ops_seen, t.q[2].body.ops_seen);
    let before = x.last.snap();
    assert_eq!(
        x.last.accept_post(&w.ctx, t.q[3].clone()),
        Verdict::Rejected(Why::NotMember)
    );
    assert_eq!(x.last.snap(), before);

    let mut w = world();
    w.cut(CAROL, H1);
    w.cut(CAROL, H2);
    let offline = w.post(CAROL, "written offline before the flags");
    for a in [A1, BOB, DAVE] {
        w.flag_and_submit(a, CAROL);
    }
    w.round();
    w.heal(CAROL, H1);
    w.heal(CAROL, H2);
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    w.round();
    assert_eq!(w.d[CAROL].outbox.len(), 1, "still queued, never uploaded");
    for i in everyone(&w) {
        if i != CAROL {
            let texts = w.d[i].rep.board.texts();
            assert!(!texts.contains("written offline before the flags"), "{i}");
        }
    }
    assert_eq!(w.accept_post(H1, offline), Verdict::Accepted);
}

// KIND: candidate-policy D01
// Section 8 Q8 (= D01/D02), under the REMOVAL WINS rule. Here a removed author's post is
// kept only if some flag had seen it. Over all 720 orders there is one outcome: only q1 is
// kept, and q2 and q3 are dropped. The backdated q3 is refused, but so is honest q2.
// Some orders show q2 and then drop it (retroactive). Replaying one such order shows it:
// q1 and q2 are shown, the flags arrive, q2 disappears, and the board is rebuilt on real
// Loro. q3 is then refused (RemovedConcurrent) and q4 (NotMember), state unchanged.
#[test]
fn posts_around_third_flag_removal_wins_drops_unseen_posts_retroactively() {
    let (w, t) = d01(PostRule::RemovalWins);
    let carol = w.did(CAROL);
    let x = explore(&w.ctx, &t.events());
    assert_eq!(x.orders, 720);
    assert_eq!(x.finals.len(), 1);
    assert!(x.retro_post);
    assert!(x.last.der.removed.contains(&carol));
    assert_eq!(x.last.board.texts(), set(&["q1 seen by alice"]));

    let mut r = Replica::new(&w.ctx, false);
    assert_eq!(r.accept_post(&w.ctx, t.q[0].clone()), Verdict::Accepted);
    assert_eq!(r.accept_post(&w.ctx, t.q[1].clone()), Verdict::Accepted);
    assert_eq!(r.board.accepted.len(), 2);
    for f in &t.f {
        r.accept_op(&w.ctx, &f.wire(), 0).unwrap();
    }
    assert_eq!(r.board.texts(), set(&["q1 seen by alice"]));
    let before = r.snap();
    assert_eq!(
        r.accept_post(&w.ctx, t.q[2].clone()),
        Verdict::Rejected(Why::RemovedConcurrent)
    );
    assert_eq!(
        r.accept_post(&w.ctx, t.q[3].clone()),
        Verdict::Rejected(Why::NotMember)
    );
    assert_eq!(r.snap(), before);
}

// KIND: candidate-policy G-AUTHORITY
// Section 8 Q7: can a removed DID be invited back?
// - Allowed: alice signs an Admit with a NEW grant that lifts the three flags; it must cite
//   them in its frontier. Over all 120 orders there is one outcome, even though the
//   readmission may arrive before the flags it lifts (it then waits pending). Carol is a
//   member again, and her post citing the readmission is kept. Her OLD grant stays revoked
//   and the new one is live. Re-presenting the old grant is refused (GrantRevoked).
// - Forbidden: the same readmission is refused (NotAllowed), state unchanged. Carol stays
//   removed. Her post citing the readmission stays pending forever, because its frontier
//   never becomes known.
// Under both rules, a plain Admit (no lifts) of a removed DID is refused (Removed), state
// unchanged.
#[test]
fn rejoin_after_removal_allowed_vs_forbidden() {
    for rejoin in [Rejoin::Allowed, Rejoin::Forbidden] {
        let w = with(|c| c.rejoin = rejoin);
        let carol = w.did(CAROL);
        let gid = w.ctx.gid.clone();
        let fa = w.flag_with(A1, 1, &[], CAROL, 0, &[]);
        let fb = w.flag_with(BOB, 2, &[], CAROL, 0, &[]);
        let fd = w.flag_with(DAVE, 3, &[], CAROL, 0, &[]);
        let lifts = ids(&[&fa, &fb, &fd]);
        let alice_grant = w.ctx.grants[&w.did(A1)].clone();
        let new_grant = MGrant::issue(
            &w.d[A1].id,
            &gid,
            &carol,
            None,
            &[Cap::Read, Cap::Post],
            false,
            77,
            Some(alice_grant),
        );
        let readmit = w.op_with(
            A1,
            5,
            lifts.clone(),
            Op::Admit {
                grant: new_grant.clone(),
                lifts: lifts.clone(),
            },
        );
        let plain = w.op_with(
            OLGA,
            6,
            lifts.clone(),
            Op::Admit {
                grant: new_grant.clone(),
                lifts: BTreeSet::new(),
            },
        );
        let old = w.op_with(
            A1,
            7,
            lifts.clone(),
            Op::Admit {
                grant: w.ctx.grants[&carol].clone(),
                lifts: lifts.clone(),
            },
        );
        let mut cw = Writer::new(w.d[CAROL].id.clone(), 0xC0C1);
        let back = cw.post(&gid, "carol is back", ids(&[&readmit]));

        let mut r = Replica::new(&w.ctx, false);
        for f in [&fa, &fb, &fd] {
            r.accept_op(&w.ctx, &f.wire(), 0).unwrap();
        }
        assert!(r.der.removed.contains(&carol));
        let before = r.snap();
        assert_eq!(r.accept_op(&w.ctx, &plain.wire(), 0), Err(Refuse::Removed));
        assert_eq!(r.snap(), before);
        let evs = [
            Ev::Op(fa.clone()),
            Ev::Op(fb.clone()),
            Ev::Op(fd.clone()),
            Ev::Op(readmit.clone()),
            Ev::Post(back.clone()),
        ];
        match rejoin {
            Rejoin::Allowed => {
                assert_eq!(
                    r.accept_op(&w.ctx, &old.wire(), 0),
                    Err(Refuse::GrantRevoked)
                );
                assert_eq!(r.snap(), before);
                let x = explore(&w.ctx, &evs);
                assert_eq!(x.orders, 120);
                assert_eq!(x.finals.len(), 1);
                let der = &x.last.der;
                assert!(!der.removed.contains(&carol) && der.roster.contains(&carol));
                assert!(der.revoked_grants.contains(&w.ctx.grants[&carol].digest()));
                assert!(!der.revoked_grants.contains(&new_grant.digest()));
                assert_eq!(x.last.board.texts(), set(&["carol is back"]));
            }
            Rejoin::Forbidden => {
                assert_eq!(
                    r.accept_op(&w.ctx, &readmit.wire(), 0),
                    Err(Refuse::NotAllowed)
                );
                assert_eq!(r.snap(), before);
                let x = explore(&w.ctx, &evs);
                assert_eq!(x.finals.len(), 1);
                assert!(x.last.der.removed.contains(&carol));
                assert!(x.last.store.is_empty());
                assert!(x.last.post_pending.contains_key(&back.id()));
            }
        }
    }
}

// KIND: candidate-policy G-AUTHORITY
// Section 8 Q9: can admins flag other admins?
// - Immune: flags on alice by olga, bob and dave are refused (NotFlaggable), state unchanged.
// - Flaggable: alice first flags erin. Then olga, bob and dave remove alice, and both of her
//   devices lose membership and admin power. Her later flag, whose frontier includes her
//   removal, is refused (IssuerRemoved), state unchanged, and so is her post. Her earlier
//   flag on erin still counts: with bob's and dave's flags, it removes erin. A flag alice
//   signs with a stale frontier (one that does not cite her removal) is still applied. The
//   D02 backdating limitation applies to ops too.
#[test]
fn admins_flagging_admins_immune_vs_flaggable() {
    for rule in [AdminRule::Immune, AdminRule::Flaggable] {
        let mut w = with(|c| c.admin = rule);
        let alice = w.did(A1);
        let erin = w.did(ERIN);
        if rule == AdminRule::Immune {
            for a in [OLGA, BOB, DAVE] {
                let f = w.flag(a, &alice);
                for i in everyone(&w) {
                    let before = w.snap(i);
                    assert_eq!(w.accept_op(i, &f.wire()), Err(Refuse::NotFlaggable));
                    assert_eq!(w.snap(i), before);
                }
            }
            continue;
        }
        let earlier = w.flag_and_submit(A1, ERIN);
        w.round();
        for a in [OLGA, BOB, DAVE] {
            w.flag_and_submit(a, A1);
        }
        w.round();
        for i in everyone(&w) {
            let der = &w.d[i].rep.der;
            assert!(der.removed.contains(&alice) && !der.roster.contains(&alice));
        }
        let late = w.flag(A2, &erin);
        let p = w.post(A2, "alice after removal");
        for i in everyone(&w) {
            if i == A2 {
                continue;
            }
            let before = w.snap(i);
            assert_eq!(w.accept_op(i, &late.wire()), Err(Refuse::IssuerRemoved));
            assert_eq!(
                w.accept_post(i, p.clone()),
                Verdict::Rejected(Why::NotMember)
            );
            assert_eq!(w.snap(i), before, "device {i}");
        }
        let stale = w.op_with(
            A1,
            99,
            ids(&[&earlier]),
            Op::Flag {
                target: erin.clone(),
                at: 0,
                seen_posts: BTreeSet::new(),
            },
        );
        assert_eq!(w.accept_op(H1, &stale.wire()), Ok(Acc::Applied));
        w.flag_and_submit(BOB, ERIN);
        w.flag_and_submit(DAVE, ERIN);
        w.round();
        for i in everyone(&w) {
            if i != A1 && i != A2 {
                assert!(w.d[i].rep.der.removed.contains(&erin), "device {i}");
            }
        }
    }
}

// KIND: invariant
// Section 10 Q8 (decided): group-level rotation on host revocation. There are three hosts.
// 1. The owner signs RemoveHost for H3 and hands it to the legitimate host H1.
// 2. H1 rotates, and itself reconnects every member, delivering the signed revocation.
// 3. H2 gets the revocation by host sync.
// 4. Each member and remaining host rotates its group endpoint exactly once, and sends the
//    new note only to permitted hosts.
// Afterwards every address H3 ever knew is dead, and H3 receives no frame at all. It holds
// none of the new endpoints and none of the new content. Its record, its links and its host
// sync are all refused, with state unchanged.
#[test]
fn host_revocation_rotates_the_group_and_revoked_host_reaches_nobody() {
    let mut w = with(|c| c.hosts = vec![H1, H2, H3]);
    w.post(ERIN, "before revocation");
    w.round();
    let mut h3_knows: BTreeSet<String> = w.d[H3]
        .rep
        .dir
        .as_ref()
        .unwrap()
        .values()
        .map(|n| n.claims.endpoint.clone())
        .collect();
    let h3_key = w.hk(H3);
    h3_knows.extend(
        w.d[H3]
            .rep
            .hosts
            .iter()
            .filter(|(k, _)| **k != h3_key)
            .map(|(_, r)| r.note.claims.endpoint.clone()),
    );
    assert_eq!(h3_knows.len(), MEMBERS.len() + 2);
    assert!(h3_knows.iter().all(|e| w.dial(H3, e).is_ok()));
    let permit = w.permit_digest(H3);
    let n = w.frames.len();
    w.revoke_host(OLGA, H1, H3);
    w.round();
    for i in everyone(&w) {
        if i == H3 {
            continue;
        }
        let rep = &w.d[i].rep;
        assert!(rep.revoked_permits.contains(&permit), "device {i}");
        assert!(!rep.hosts.contains_key(&h3_key), "device {i}");
        assert_eq!(w.d[i].my_note.claims.seq, 2, "device {i} rotated once");
    }
    for e in &h3_knows {
        assert_eq!(w.dial(H3, e), Err(RouteErr::EndpointGone));
    }
    assert!(w.frames[n..].iter().all(|f| f.to != H3));
    assert!(
        w.frames[n..]
            .iter()
            .filter(|f| f.kind == "ep-note")
            .all(|f| f.to == H1 || f.to == H2)
    );
    let hay = w.received(H3);
    for i in 0..w.d.len() {
        if i != H3 {
            assert!(
                !has(&hay, w.gep(i).as_bytes()),
                "H3 holds new endpoint of {i}"
            );
        }
    }
    let rec = w.d[H3].host.as_ref().unwrap().wire();
    for m in MEMBERS {
        let before = w.snap(m);
        assert_eq!(w.accept_host_record(m, &rec), Err(Refuse::Revoked));
        assert_eq!(w.connect(m, H3), Err(RouteErr::Lost));
        assert_eq!(w.connect_from_host(H3, m), Err(RouteErr::EndpointGone));
        assert_eq!(w.snap(m), before);
    }
    assert!(w.host_sync(H1, H3).is_err());
    assert!(w.host_sync(H2, H3).is_err());
    w.post(BOB, "after revocation");
    w.round();
    w.assert_converged(&[OLGA, A1, A2, BOB, DAVE, CAROL, ERIN, H1, H2]);
    assert!(w.d[CAROL].rep.board.texts().contains("after revocation"));
    assert!(!w.d[H3].rep.board.texts().contains("after revocation"));
    assert!(w.d[H3].rep.board.texts().contains("before revocation"));
}

// KIND: limitation
// Two limits of section 10 Q8:
// (1) Content the revoked host already held stays with it.
// (2) A member that has not yet been reached still trusts H3. Erin is cut from H1 and H2:
//     H3 can still dial her unchanged endpoint, and her new post goes to H3 only. That post
//     reaches no other device, and it is not re-sent after she learns of the revocation, so
//     it is stranded at the revoked host. Once H1 reaches her, she rotates and H3 loses her
//     address.
#[test]
fn host_revocation_held_content_and_unreached_member_keeps_using_revoked_host() {
    let mut w = with(|c| c.hosts = vec![H1, H2, H3]);
    w.post(BOB, "held before revocation");
    w.round();
    w.cut(ERIN, H1);
    w.cut(ERIN, H2);
    w.revoke_host(OLGA, H1, H3);
    w.round();
    assert!(w.d[H3].rep.board.texts().contains("held before revocation"));
    let erin_old = w.gep(ERIN);
    assert_eq!(w.d[ERIN].my_note.claims.seq, 1);
    assert!(w.d[ERIN].rep.hosts.contains_key(&w.hk(H3)));
    assert_eq!(w.dial(H3, &erin_old), Ok(ERIN));
    w.post(ERIN, "erin to the revoked host");
    assert_eq!(w.connect(ERIN, H3), Ok(Path::MemberDialedHost));
    assert!(
        w.d[H3]
            .rep
            .board
            .texts()
            .contains("erin to the revoked host")
    );
    w.heal(ERIN, H1);
    assert_eq!(w.connect(ERIN, H1), Ok(Path::HostDialedMember));
    w.round();
    assert_eq!(w.d[ERIN].my_note.claims.seq, 2);
    assert!(!w.d[ERIN].rep.hosts.contains_key(&w.hk(H3)));
    assert_eq!(w.dial(H3, &erin_old), Err(RouteErr::EndpointGone));
    assert!(!has(&w.received(H3), w.gep(ERIN).as_bytes()));
    for i in [OLGA, A1, A2, BOB, DAVE, CAROL, H1, H2] {
        let texts = w.d[i].rep.board.texts();
        assert!(!texts.contains("erin to the revoked host"), "device {i}");
    }
}

// KIND: limitation
// Section 10 Q8 does not cover an UNDETECTED rogue host. After H3's revocation and the group
// rotation, suppose H2 is also hostile but nobody has revoked it. It still holds every
// member's new group endpoint and every new post. Rotation shuts out only the hosts that were
// revoked.
#[test]
fn undetected_rogue_host_still_learns_every_new_endpoint_after_group_rotation() {
    let mut w = with(|c| c.hosts = vec![H1, H2, H3]);
    w.revoke_host(OLGA, H1, H3);
    w.round();
    w.post(CAROL, "after rotation");
    w.round();
    let hay = w.received(H2);
    for m in MEMBERS {
        assert_eq!(w.d[m].my_note.claims.seq, 2);
        assert!(
            has(&hay, w.gep(m).as_bytes()),
            "H2 holds new endpoint of {m}"
        );
    }
    assert!(w.d[H2].rep.board.texts().contains("after rotation"));
}
