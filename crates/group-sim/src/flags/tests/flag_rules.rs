use super::*;

// ---- tests ---------------------------------------------------------------------------------

// KIND: invariant
// U9 (decided): admins flag a member; at 3 flags the member is removed and its grant revoked.
// Here the 3 flags come from 3 distinct admins, so this holds under either `FlagCount`
// candidate. Flags are DID-signed ops that EVERY device verifies and applies itself. After 2
// flags no roster changes anywhere. After the 3rd, every device derives the same state:
// members, both hosts, and carol herself once told. The hosts drop her group endpoint. Her
// revoked grant cannot be re-presented in an Admit: GrantRevoked, state byte-identical.
#[test]
fn three_flags_from_distinct_admins_remove_member_and_revoke_grant_everywhere() {
    let mut w = world();
    let carol = w.did(CAROL);
    w.post(CAROL, "carol before");
    w.round();
    w.flag_and_submit(A1, CAROL);
    w.flag_and_submit(BOB, CAROL);
    w.round();
    for i in everyone(&w) {
        let der = &w.d[i].rep.der;
        assert!(der.roster.contains(&carol), "device {i}");
        assert!(der.removed.is_empty(), "device {i}");
        assert_eq!(der.flags[&carol].len(), 2, "device {i}");
    }
    w.flag_and_submit(DAVE, CAROL);
    w.round();
    let grant = w.ctx.grants[&carol].clone();
    let first = w.d[OLGA].rep.der.clone();
    for i in everyone(&w) {
        assert_eq!(w.d[i].rep.der, first, "device {i}");
    }
    assert!(first.removed.contains(&carol) && !first.roster.contains(&carol));
    assert!(first.revoked_grants.contains(&grant.digest()));
    assert_eq!(first.flags[&carol].len(), 3);
    for h in [H1, H2] {
        let dir = w.d[h].rep.dir.as_ref().unwrap();
        assert!(dir.values().all(|n| n.claims.owner != carol), "host {h}");
    }
    let readmit = w.op(
        OLGA,
        Op::Admit {
            grant,
            lifts: BTreeSet::new(),
        },
    );
    for i in everyone(&w) {
        let before = w.snap(i);
        assert_eq!(
            w.accept_op(i, &readmit.wire()),
            Err(Refuse::GrantRevoked),
            "device {i}"
        );
        assert_eq!(w.snap(i), before, "device {i}");
    }
    w.assert_converged(&[OLGA, A1, A2, BOB, DAVE, ERIN, H1, H2]);
}

// KIND: candidate-policy G-AUTHORITY
// Section 8 Q1: does "3 flags" mean 3 distinct admin DIDs (the lean) or any 3 flags? Alice
// flags carol three times from two devices: A1 twice and A2 once, all under one DID.
// - DistinctAdmins: this counts as 1 admin, so carol stays. Bob and dave then complete the
//   removal.
// - AnyFlags: one admin removes carol alone, just by repeating the flag.
#[test]
fn same_admin_flagging_three_times_distinct_admins_vs_any_flags() {
    for count in [FlagCount::DistinctAdmins, FlagCount::AnyFlags] {
        let mut w = with(|c| c.count = count);
        let carol = w.did(CAROL);
        w.flag_and_submit(A1, CAROL);
        w.flag_and_submit(A1, CAROL);
        w.flag_and_submit(A2, CAROL);
        w.round();
        for i in everyone(&w) {
            let der = &w.d[i].rep.der;
            assert_eq!(der.flags[&carol].len(), 3, "{count:?} device {i}");
            assert_eq!(
                der.removed.contains(&carol),
                count == FlagCount::AnyFlags,
                "{count:?} device {i}"
            );
        }
        if count == FlagCount::DistinctAdmins {
            w.flag_and_submit(BOB, CAROL);
            w.flag_and_submit(DAVE, CAROL);
            w.round();
            for i in everyone(&w) {
                assert!(w.d[i].rep.der.removed.contains(&carol), "device {i}");
            }
        }
    }
}

// KIND: invariant
// Flags commute. Removal is a pure function of the verified op set, and each op is
// authorized at its own signed frontier.
// (a) Three admin flags and two posts, delivered to a fresh replica in all 120 orders, give
//     ONE final outcome. A re-delivered flag is a Duplicate, and the state is unchanged.
// (b) Two partitioned sides: H1 with alice's flag (and olga, carol, erin), and H2 with bob's
//     and dave's flags. Neither side removes carol, since one side has 1 flag and the other
//     has 2. After H1-H2 heal, host sync carries the ops. Every device then derives the same
//     state, and carol is removed.
#[test]
fn flags_commute_across_delivery_orders_and_two_partitioned_hosts() {
    let w = world();
    let carol = w.did(CAROL);
    let fa = w.flag_with(A1, 1, &[], CAROL, 0, &[]);
    let fb = w.flag_with(BOB, 2, &[], CAROL, 0, &[]);
    let fd = w.flag_with(DAVE, 3, &[], CAROL, 0, &[]);
    let mut cw = Writer::new(w.d[CAROL].id.clone(), 0xC0C0);
    let q = cw.post(&w.ctx.gid, "carol", BTreeSet::new());
    let mut ew = Writer::new(w.d[ERIN].id.clone(), 0xE0E0);
    let p = ew.post(&w.ctx.gid, "erin", BTreeSet::new());
    let evs = [
        Ev::Op(fa.clone()),
        Ev::Op(fb),
        Ev::Op(fd),
        Ev::Post(q),
        Ev::Post(p),
    ];
    let mut x = explore(&w.ctx, &evs);
    assert_eq!(x.orders, 120);
    assert_eq!(x.finals.len(), 1);
    assert!(!x.retro_post && !x.retro_removal);
    assert!(x.last.der.removed.contains(&carol));
    assert_eq!(x.last.board.texts(), set(&["carol", "erin"]));
    let before = x.last.snap();
    assert_eq!(x.last.accept_op(&w.ctx, &fa.wire(), 0), Ok(Acc::Duplicate));
    assert_eq!(x.last.snap(), before);

    let mut w = world();
    w.cut(H1, H2);
    for m in [OLGA, A1, A2, CAROL, ERIN] {
        w.cut(m, H2);
    }
    for m in [BOB, DAVE] {
        w.cut(m, H1);
    }
    w.flag_and_submit(A1, CAROL);
    w.flag_and_submit(BOB, CAROL);
    w.flag_and_submit(DAVE, CAROL);
    w.round();
    assert_eq!(w.d[H1].rep.der.flags[&carol].len(), 1);
    assert_eq!(w.d[H2].rep.der.flags[&carol].len(), 2);
    for i in everyone(&w) {
        assert!(w.d[i].rep.der.removed.is_empty(), "device {i}");
    }
    w.heal(H1, H2);
    w.round();
    let first = w.d[OLGA].rep.der.clone();
    assert!(first.removed.contains(&carol));
    for i in everyone(&w) {
        assert_eq!(w.d[i].rep.der, first, "device {i}");
    }
}

// KIND: invariant
// Flags are neither host fiat nor forgeable. Every device refuses each of the following, and
// its full state stays byte-identical:
// - malformed bytes;
// - a flag signed by another key that names alice as issuer (forged);
// - alice's real flag with its target swapped after signing;
// - a flag by member erin under her member grant;
// - a flag by erin under a self-issued "admin" grant;
// - a flag by host binary hugo (H1), presenting its {Host} permit as the issuer grant;
// - a flag by hugo with no grant;
// - a flag for another group;
// - a flag on a DID that is not a member.
// Positive control: alice's genuine flag is applied everywhere.
#[test]
fn forged_non_admin_host_only_wrong_group_and_non_member_flags_are_refused_unchanged() {
    let mut w = world();
    let carol = w.did(CAROL);
    let gid = w.ctx.gid.clone();
    let alice = w.d[A1].id.clone();
    let alice_grant = w.ctx.grants[&w.did(A1)].clone();
    let erin = w.d[ERIN].id.clone();
    let erin_grant = w.ctx.grants[&w.did(ERIN)].clone();
    let hugo = w.d[H1].id.clone();
    let host_permit = w.d[H1].host.as_ref().unwrap().permit.clone();
    let mallory = ident(77);
    let flag = |target: &str| Op::Flag {
        target: target.into(),
        at: 0,
        seen_posts: BTreeSet::new(),
    };
    let none = BTreeSet::new;
    let forged = {
        let body = OpBody {
            group: gid.clone(),
            issuer: alice.did().into(),
            issuer_grant: Some(alice_grant.clone()),
            nonce: 1,
            seen: none(),
            op: flag(&carol),
        };
        let sig = mallory.sign(&signed(OP_DOMAIN, &body)).to_vec();
        SignedOp { body, sig }
    };
    let mut tampered = SignedOp::sign(
        &alice,
        &gid,
        Some(alice_grant.clone()),
        2,
        none(),
        flag(&w.did(ERIN)),
    );
    tampered.body.op = flag(&carol);
    let self_admin = MGrant::issue(
        &erin,
        &gid,
        erin.did(),
        None,
        &[Cap::Read, Cap::Post, Cap::Delegate, Cap::Admin],
        true,
        4,
        None,
    );
    let cases: Vec<(&str, Vec<u8>, Refuse)> = vec![
        ("malformed", b"{not an op".to_vec(), Refuse::Malformed),
        ("forged issuer", forged.wire(), Refuse::BadSignature),
        ("target swapped", tampered.wire(), Refuse::BadSignature),
        (
            "member erin",
            SignedOp::sign(&erin, &gid, Some(erin_grant), 3, none(), flag(&carol)).wire(),
            Refuse::NotAdmin,
        ),
        (
            "self-issued admin grant",
            SignedOp::sign(&erin, &gid, Some(self_admin), 5, none(), flag(&carol)).wire(),
            Refuse::NotAdmin,
        ),
        (
            "host permit as issuer grant",
            SignedOp::sign(&hugo, &gid, Some(host_permit), 6, none(), flag(&carol)).wire(),
            Refuse::NotAdmin,
        ),
        (
            "host without grant",
            SignedOp::sign(&hugo, &gid, None, 7, none(), flag(&carol)).wire(),
            Refuse::NotAdmin,
        ),
        (
            "other group",
            SignedOp::sign(
                &alice,
                "other-group",
                Some(alice_grant.clone()),
                8,
                none(),
                flag(&carol),
            )
            .wire(),
            Refuse::WrongGroup,
        ),
        (
            "non-member target",
            SignedOp::sign(
                &alice,
                &gid,
                Some(alice_grant.clone()),
                9,
                none(),
                flag(mallory.did()),
            )
            .wire(),
            Refuse::NotMember,
        ),
    ];
    for i in everyone(&w) {
        for (name, bytes, want) in &cases {
            let before = w.snap(i);
            assert_eq!(w.accept_op(i, bytes), Err(*want), "{name} at {i}");
            assert_eq!(w.snap(i), before, "{name} at {i}");
        }
    }
    let good = SignedOp::sign(&alice, &gid, Some(alice_grant), 10, none(), flag(&carol));
    for i in everyone(&w) {
        assert_eq!(w.accept_op(i, &good.wire()), Ok(Acc::Applied), "device {i}");
        assert_eq!(w.d[i].rep.der.flags[&carol].len(), 1);
    }
}

// KIND: candidate-policy G-AUTHORITY
// Section 8 Q2: what happens in a group with fewer than 3 admins? Here only olga (the owner)
// and alice are admins. Counting the owner as an admin is itself an open question. Bob is a
// plain member, so his flag is refused (NotAdmin, state unchanged).
// - OwnerAlone: the owner's single flag removes; alice's flag alone does not.
// - ThresholdIsAdminCount (= 2): removal needs both olga and alice.
// - NoRemoval: carol is never removed, even with every admin's flag.
#[test]
fn fewer_than_three_admins_owner_alone_vs_admin_count_vs_no_removal() {
    for small in [
        SmallGroup::OwnerAlone,
        SmallGroup::ThresholdIsAdminCount,
        SmallGroup::NoRemoval,
    ] {
        let mk = || {
            with(|c| {
                c.admins = vec![A1];
                c.small = small;
            })
        };
        let mut w = mk();
        let carol = w.did(CAROL);
        assert_eq!(w.ctx.admins.len(), 2);
        let by_bob = w.flag(BOB, &carol);
        for i in everyone(&w) {
            let before = w.snap(i);
            assert_eq!(w.accept_op(i, &by_bob.wire()), Err(Refuse::NotAdmin));
            assert_eq!(w.snap(i), before);
        }
        // Owner's flag alone.
        w.flag_and_submit(OLGA, CAROL);
        w.round();
        for i in everyone(&w) {
            let removed = w.d[i].rep.der.removed.contains(&carol);
            assert_eq!(removed, small == SmallGroup::OwnerAlone, "{small:?} at {i}");
        }
        // Alice first, then the owner.
        let mut w = mk();
        w.flag_and_submit(A1, CAROL);
        w.round();
        for i in everyone(&w) {
            assert!(w.d[i].rep.der.removed.is_empty(), "{small:?} at {i}");
        }
        w.flag_and_submit(OLGA, CAROL);
        w.round();
        for i in everyone(&w) {
            let der = &w.d[i].rep.der;
            assert_eq!(der.flags[&carol].len(), 2);
            assert_eq!(
                der.removed.contains(&carol),
                small != SmallGroup::NoRemoval,
                "{small:?} at {i}"
            );
            assert_eq!(der.roster.contains(&carol), small == SmallGroup::NoRemoval);
        }
    }
}

// KIND: candidate-policy G-AUTHORITY
// Section 8 Q3: can the owner/creator be flagged?
// - Protected (the lean): each admin's flag on olga is refused (NotFlaggable), state unchanged.
// - Flaggable: three admins remove olga. Her later ops are refused (IssuerRemoved, state
//   unchanged), and so are her posts. But the grants and host permits she issued still
//   verify, because chains root at the genesis creator key, not at a "current owner". So
//   alice's flag on erin is applied, and the hosts olga appointed still relay. Whether
//   removing the owner should cascade, or transfer ownership, is undecided.
#[test]
fn owner_protected_vs_flaggable() {
    for owner in [OwnerRule::Protected, OwnerRule::Flaggable] {
        let mut w = with(|c| c.owner = owner);
        let olga = w.did(OLGA);
        let erin = w.did(ERIN);
        if owner == OwnerRule::Protected {
            for a in [A1, BOB, DAVE] {
                let f = w.flag(a, &olga);
                for i in everyone(&w) {
                    let before = w.snap(i);
                    assert_eq!(w.accept_op(i, &f.wire()), Err(Refuse::NotFlaggable));
                    assert_eq!(w.snap(i), before);
                }
            }
            continue;
        }
        for a in [A1, BOB, DAVE] {
            w.flag_and_submit(a, OLGA);
        }
        w.round();
        for i in everyone(&w) {
            assert!(w.d[i].rep.der.removed.contains(&olga), "device {i}");
        }
        let late = w.flag(OLGA, &erin);
        let p = w.post(OLGA, "owner after removal");
        for i in everyone(&w) {
            let before = w.snap(i);
            assert_eq!(w.accept_op(i, &late.wire()), Err(Refuse::IssuerRemoved));
            assert_eq!(
                w.accept_post(i, p.clone()),
                Verdict::Rejected(Why::NotMember)
            );
            assert_eq!(w.snap(i), before, "device {i}");
        }
        w.flag_and_submit(A1, ERIN);
        w.post(BOB, "hosts appointed by the removed owner still relay");
        w.round();
        for i in everyone(&w) {
            if i != OLGA {
                assert_eq!(w.d[i].rep.der.flags[&erin].len(), 1, "device {i}");
            }
        }
        assert!(
            w.d[CAROL]
                .rep
                .board
                .texts()
                .contains("hosts appointed by the removed owner still relay")
        );
    }
}

// KIND: candidate-policy G-AUTHORITY
// Section 8 Q4a: can an admin withdraw a flag before the threshold?
// - Final: a withdrawal op is refused (NotAllowed), state unchanged.
// - BeforeThreshold: alice flags, bob flags, alice withdraws, dave flags. That leaves 2
//   effective flags, so carol stays. Two withdrawals are refused, state unchanged:
//   withdrawing someone else's flag (NotFlagger), and withdrawing once the removal is in the
//   withdrawer's frontier (AfterThreshold).
// - "Before" can only be judged at the withdrawal's own signed frontier. So a withdrawal
//   CONCURRENT with the 3rd flag cancels a removal that replicas receiving the 3rd flag first
//   already showed. Over all 24 orders there is one final outcome (not removed), but some
//   orders show a retroactive un-removal.
#[test]
fn flag_withdrawal_final_vs_before_threshold() {
    let mut w = world();
    let fa = w.flag_and_submit(A1, CAROL);
    w.round();
    let wd = w.op(A1, Op::Withdraw { flag: fa.id() });
    for i in everyone(&w) {
        let before = w.snap(i);
        assert_eq!(w.accept_op(i, &wd.wire()), Err(Refuse::NotAllowed));
        assert_eq!(w.snap(i), before);
    }

    let mut w = with(|c| c.withdrawal = Withdrawal::BeforeThreshold);
    let carol = w.did(CAROL);
    let fa = w.flag_and_submit(A1, CAROL);
    let fb = w.flag_and_submit(BOB, CAROL);
    w.round();
    let wd = w.op(A1, Op::Withdraw { flag: fa.id() });
    assert_eq!(w.submit(A1, wd), Ok(Acc::Applied));
    w.round();
    let not_mine = w.op(DAVE, Op::Withdraw { flag: fb.id() });
    for i in everyone(&w) {
        let before = w.snap(i);
        assert_eq!(w.accept_op(i, &not_mine.wire()), Err(Refuse::NotFlagger));
        assert_eq!(w.snap(i), before);
    }
    w.flag_and_submit(DAVE, CAROL);
    w.round();
    for i in everyone(&w) {
        let der = &w.d[i].rep.der;
        assert!(!der.removed.contains(&carol), "device {i}");
        assert_eq!(der.flags[&carol].len(), 2, "device {i}");
    }
    w.flag_and_submit(A1, CAROL); // alice flags again
    w.round();
    for i in everyone(&w) {
        assert!(w.d[i].rep.der.removed.contains(&carol), "device {i}");
    }
    let too_late = w.op(BOB, Op::Withdraw { flag: fb.id() });
    for i in everyone(&w) {
        let before = w.snap(i);
        assert_eq!(
            w.accept_op(i, &too_late.wire()),
            Err(Refuse::AfterThreshold)
        );
        assert_eq!(w.snap(i), before);
    }

    let w = with(|c| c.withdrawal = Withdrawal::BeforeThreshold);
    let fa = w.flag_with(A1, 1, &[], CAROL, 0, &[]);
    let fb = w.flag_with(BOB, 2, &[], CAROL, 0, &[]);
    let fd = w.flag_with(DAVE, 3, &[], CAROL, 0, &[]);
    let wd = w.op_with(A1, 4, ids(&[&fa, &fb]), Op::Withdraw { flag: fa.id() });
    let mut first_flags_then_withdraw = Replica::new(&w.ctx, false);
    for o in [&fa, &fb, &fd] {
        first_flags_then_withdraw
            .accept_op(&w.ctx, &o.wire(), 0)
            .unwrap();
    }
    assert!(first_flags_then_withdraw.der.removed.contains(&carol));
    first_flags_then_withdraw
        .accept_op(&w.ctx, &wd.wire(), 0)
        .unwrap();
    assert!(!first_flags_then_withdraw.der.removed.contains(&carol));
    let x = explore(&w.ctx, &[Ev::Op(fa), Ev::Op(fb), Ev::Op(fd), Ev::Op(wd)]);
    assert_eq!(x.orders, 24);
    assert_eq!(x.finals.len(), 1);
    assert!(!x.last.der.removed.contains(&carol));
    assert!(x.retro_removal);
}

// KIND: candidate-policy G-TIME
// Section 8 Q4b: do flags expire? The flags carry claimed times 0, 50 and 300.
// - Never: carol is removed.
// - SignedWindow(100): the three flags never fall within 100 ticks of each other, so carol
//   stays. But `at` is chosen by the signer: dave re-signs his flag claiming at=10, and it
//   counts.
// - ReceiverClock(100): replica X gets alice's flag at t=0 and the other two at t=200.
//   Replica Y gets all three at t=200. With the same op set but different arrival times, Y
//   removes carol and X never does, because alice's flag expired at X before the 3rd
//   arrived. The divergence is permanent.
#[test]
fn flag_expiry_never_vs_signed_window_vs_receiver_clock() {
    for expiry in [
        Expiry::Never,
        Expiry::SignedWindow(100),
        Expiry::ReceiverClock(100),
    ] {
        let w = with(|c| c.expiry = expiry);
        let carol = w.did(CAROL);
        let fa = w.flag_with(A1, 1, &[], CAROL, 0, &[]);
        let fb = w.flag_with(BOB, 2, &[], CAROL, 50, &[]);
        let fd = w.flag_with(DAVE, 3, &[], CAROL, 300, &[]);
        let mut y = Replica::new(&w.ctx, false);
        for f in [&fa, &fb, &fd] {
            y.accept_op(&w.ctx, &f.wire(), 200).unwrap();
        }
        match expiry {
            Expiry::Never => assert!(y.der.removed.contains(&carol)),
            Expiry::SignedWindow(_) => {
                assert!(!y.der.removed.contains(&carol));
                let backdated = w.flag_with(DAVE, 4, &[], CAROL, 10, &[]);
                y.accept_op(&w.ctx, &backdated.wire(), 300).unwrap();
                assert!(y.der.removed.contains(&carol));
            }
            Expiry::ReceiverClock(_) => {
                assert!(y.der.removed.contains(&carol));
                let mut x = Replica::new(&w.ctx, false);
                x.accept_op(&w.ctx, &fa.wire(), 0).unwrap();
                x.accept_op(&w.ctx, &fb.wire(), 200).unwrap();
                x.accept_op(&w.ctx, &fd.wire(), 200).unwrap();
                assert_eq!(
                    x.ops.keys().collect::<Vec<_>>(),
                    y.ops.keys().collect::<Vec<_>>()
                );
                assert!(!x.der.removed.contains(&carol));
                x.tick(&w.ctx, 1_000);
                y.tick(&w.ctx, 1_000);
                assert!(!x.der.removed.contains(&carol));
                assert!(y.der.removed.contains(&carol), "latched");
            }
        }
    }
}

// KIND: invariant
// After a removal (U9 + U7 + U5 revised), every honest host has dropped carol's group
// endpoint. Each one refuses, with state unchanged at every host and member: her link, her
// posts citing her current frontier, and her new rotation note. A host can no longer dial her
// (NoAddress). She is told explicitly: on her next dial, an honest host returns the three
// signed flags, and she verifies them herself. A repeat attempt gets the same notice, and her
// state does not change.
#[test]
fn removed_member_refused_by_every_honest_host_endpoint_dropped_and_told_with_signed_flags() {
    let mut w = world();
    let carol = w.did(CAROL);
    w.post(CAROL, "carol before");
    w.round();
    w.cut(CAROL, H1);
    w.cut(CAROL, H2);
    for a in [A1, BOB, DAVE] {
        w.flag_and_submit(a, CAROL);
    }
    w.round();
    let carol_ep = w.gep(CAROL);
    for h in [H1, H2] {
        assert!(w.d[h].rep.der.removed.contains(&carol));
        let dir = w.d[h].rep.dir.as_ref().unwrap();
        assert!(dir.values().all(|n| n.claims.owner != carol));
        assert!(!has(&w.snap(h), carol_ep.as_bytes()), "host {h}");
    }
    assert!(!w.d[CAROL].rep.der.removed.contains(&carol), "not told yet");
    w.heal(CAROL, H1);
    let n = w.frames.len();
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    let notices: Vec<&Frame> = w.frames[n..]
        .iter()
        .filter(|f| f.to == CAROL && f.kind == "removal-notice")
        .collect();
    assert_eq!(notices.len(), 3);
    assert!(notices.iter().all(|f| f.from == H1));
    assert!(w.d[CAROL].rep.der.removed.contains(&carol));
    assert_eq!(w.d[CAROL].rep.der.flags[&carol].len(), 3);
    let after = w.snap(CAROL);
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    assert_eq!(w.snap(CAROL), after, "same notice again, no change");

    let p = w.post(CAROL, "carol after notice");
    let note = EpNote::sign(
        &w.d[CAROL].id,
        &w.ctx.gid,
        w.d[CAROL].slot,
        "grp-carol-rotated",
        2,
    );
    for i in everyone(&w) {
        if i == CAROL {
            continue;
        }
        let before = w.snap(i);
        assert_eq!(
            w.accept_post(i, p.clone()),
            Verdict::Rejected(Why::NotMember),
            "device {i}"
        );
        if w.d[i].host.is_some() {
            assert_eq!(w.host_accept_note(i, &note.wire()), Err(Refuse::NotMember));
            assert_eq!(w.connect_from_host(i, CAROL), Err(RouteErr::NoAddress));
        }
        assert_eq!(w.snap(i), before, "device {i}");
    }
}

// Shared D01/D02 trace for the two post-rule candidates. Carol's writer: q1 (seen by alice's
// flag), q2 (honest, concurrent: frontier before any flag), then the three flags, then q3
// (written after removal but citing the old, empty frontier = backdated) and q4 (cites the
// removal).
pub(super) struct D01 {
    pub(super) q: [Post; 4],
    pub(super) f: [SignedOp; 3],
}
pub(super) fn d01(post: PostRule) -> (World, D01) {
    let w = with(|c| c.post = post);
    let gid = w.ctx.gid.clone();
    let mut cw = Writer::new(w.d[CAROL].id.clone(), 0xC0C0);
    let q1 = cw.post(&gid, "q1 seen by alice", BTreeSet::new());
    let q2 = cw.post(&gid, "q2 honest offline", BTreeSet::new());
    let fa = w.flag_with(A1, 1, &[], CAROL, 0, &[&q1]);
    let fb = w.flag_with(BOB, 2, &[], CAROL, 0, &[]);
    let fd = w.flag_with(DAVE, 3, &[], CAROL, 0, &[]);
    let q3 = cw.post(&gid, "q3 backdated", BTreeSet::new());
    let q4 = cw.post(&gid, "q4 cites removal", ids(&[&fa, &fb, &fd]));
    (
        w,
        D01 {
            q: [q1, q2, q3, q4],
            f: [fa, fb, fd],
        },
    )
}
impl D01 {
    pub(super) fn events(&self) -> Vec<Ev> {
        let mut v: Vec<Ev> = self.q[..3].iter().cloned().map(Ev::Post).collect();
        v.extend(self.f.iter().cloned().map(Ev::Op));
        v
    }
}
