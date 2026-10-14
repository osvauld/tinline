use super::*;

// KIND: candidate-policy G-TIME
// Single host, host and carol both rotated while apart.
// - Retention::None: Lost in both directions; only a manual re-share recovers.
// - Retention::UntilHostAck (coordinator proposal, section 9 Q6): carol keeps her OLD group
//   endpoint listening until the host acknowledges her new note. The host dials the old
//   endpoint, hands over its new record and receives the new note; the ack releases the old
//   endpoint. Cost: the old endpoint stays reachable (to anyone who knows it) until then.
#[test]
fn single_host_both_rotated_retain_old_endpoint_until_ack_vs_no_retention() {
    for retention in [Retention::None, Retention::UntilHostAck] {
        let mut w = World::new(Cfg {
            hosts: vec![H1],
            retention,
            ..Cfg::default()
        });
        w.d[CAROL].online = false;
        let old = w.gep(CAROL);
        w.rotate_member(CAROL);
        w.rotate_host(H1);
        w.d[CAROL].online = true;
        match retention {
            Retention::None => {
                assert!(w.live.get(&old).is_none());
                assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Lost));
                assert_eq!(w.connect_from_host(H1, CAROL), Err(RouteErr::EndpointGone));
            }
            Retention::UntilHostAck => {
                assert_eq!(w.live.get(&old), Some(&CAROL), "old endpoint retained");
                assert_eq!(w.connect(CAROL, H1), Ok(Path::HostDialedMember));
                assert!(w.live.get(&old).is_none(), "released after host ack");
                assert_eq!(w.connect(CAROL, H1), Ok(Path::MemberDialedHost));
                let k = w.hk(CAROL);
                assert_eq!(
                    w.d[H1].host.as_ref().unwrap().directory[&k].claims.endpoint,
                    w.gep(CAROL)
                );
            }
        }
    }
}

// Shared setup for the two writer-collision tests: hosts partitioned, alice-1 only reaches
// `ha`, carol only reaches `hc`; carol is forced onto alice-1's writer ID (a 2^-64 draw or a
// broken RNG; still not key-derived). HP only reaches H2, BOB reaches both.
pub(super) fn collision_world(claim: ClaimRule, ha: usize, hc: usize) -> (World, u64, Post, Post) {
    let mut w = World::new(Cfg {
        claim,
        ..Cfg::default()
    });
    let shared = w.d[A1].w.writer;
    let carol_id = w.d[CAROL].id.clone();
    w.d[CAROL].w = Writer::new(carol_id, shared);
    w.cut(H1, H2);
    w.cut(A1, hc);
    w.cut(CAROL, ha);
    w.cut(HP, H1);
    let pa = w.post(A1, "alice");
    let pc = w.post(CAROL, "carol");
    w.connect(A1, ha).unwrap();
    w.connect(CAROL, hc).unwrap();
    (w, shared, pa, pc)
}

// KIND: limitation
// First-claim-at-host with two hosts: each host binds the shared writer ID to the DID it saw
// first. After the host link heals, each host refuses the other's post, so the hosts and the
// members they serve diverge permanently (bob saw H1 first; hana's phone only sees H2).
#[test]
fn writer_collision_first_claim_at_each_host_diverges_across_hosts() {
    let (mut w, shared, _, _) = collision_world(ClaimRule::FirstAtHost, H1, H2);
    w.heal(H1, H2);
    w.round();
    w.round();
    let (alice, carol) = (w.did(A1), w.did(CAROL));
    assert_eq!(w.d[H1].board.writers[&shared], alice);
    assert_eq!(w.d[H2].board.writers[&shared], carol);
    assert_eq!(w.d[H1].board.texts(), set(&["alice"]));
    assert_eq!(w.d[H2].board.texts(), set(&["carol"]));
    assert_eq!(w.d[BOB].board.texts(), set(&["alice"]));
    assert_eq!(w.d[HP].board.texts(), set(&["carol"]));
    assert_ne!(
        w.d[BOB].board.doc.get_deep_value(),
        w.d[HP].board.doc.get_deep_value()
    );
}

// KIND: candidate-policy G-FORK
// Deterministic cross-host tie-break: hosts hold a new writer's posts until every host has
// seen every claim for it; then the lowest claim id (blake3 of the signed seq-1 post) wins at
// every host. During the partition nothing under the contested writer is delivered (cost:
// every NEW writer stalls while any host is unreachable). After healing, the same DID wins
// in BOTH placements (order-independent), the loser is told, redraws its writer ID and
// re-posts, and everyone converges with both texts.
// Not a proof: a claim that reaches one host after the other already settled is not
// covered (needs an agreement round; not designed).
#[test]
fn writer_collision_lowest_claim_after_all_hosts_saw_converges() {
    let mut winners = vec![];
    for (ha, hc) in [(H1, H2), (H2, H1)] {
        let (mut w, shared, pa, pc) = collision_world(ClaimRule::LowestAfterAllHostsSaw, ha, hc);
        w.round();
        for i in everyone() {
            assert!(
                w.d[i].board.accepted.is_empty(),
                "stalled while partitioned"
            );
        }
        w.heal(H1, H2);
        w.round();
        let win = if pa.id() < pc.id() { A1 } else { CAROL };
        let lose = if win == A1 { CAROL } else { A1 };
        for h in [H1, H2] {
            assert_eq!(w.d[h].board.writers[&shared], w.did(win));
        }
        w.round();
        assert_eq!(w.d[lose].refused.len(), 1);
        assert_eq!(w.d[lose].refused[0].1, Why::WriterOwnedByOtherDid);
        let new = w.redraw_writer(lose);
        assert_ne!(new, shared);
        w.heal(A1, H1);
        w.heal(A1, H2);
        w.heal(CAROL, H1);
        w.heal(CAROL, H2);
        w.round();
        w.round();
        w.assert_converged(&everyone());
        assert_eq!(w.d[HP].board.texts(), set(&["alice", "carol"]));
        winners.push(w.did(win));
    }
    assert_eq!(winners[0], winners[1], "winner independent of placement");
}

// KIND: invariant
// Q5 "admins can remove a host": bob (admin, appointed after H2) signs RemoveHost for H2's
// permit. Once it propagates, every member refuses H2: links in both directions, a post H2
// pushes, and a fresh H2 record (state unchanged). H1 refuses host sync with H2. Members'
// next rotation notes go only to H1. H2 still holds the endpoints it learned before removal
// until those members rotate (noted, not prevented).
#[test]
fn removed_host_is_refused_by_members_and_hosts_afterwards() {
    let mut w = world();
    w.post(BOB, "before");
    w.round();
    let permit = w.d[H2].host.as_ref().unwrap().record.permit.digest();
    let op = w.op(BOB, Op::RemoveHost { permit });
    w.submit(BOB, op);
    w.cut(BOB, H2); // bob must use H1 to publish the removal
    w.round();
    w.heal(BOB, H2);
    let h2_rec = w.d[H2].host.as_ref().unwrap().record.wire();
    for m in MEMBERS {
        assert!(!w.d[m].view.hosts.contains_key(&w.hk(H2)));
        let before = w.snap(m);
        assert_eq!(w.connect(m, H2), Err(RouteErr::Refused));
        assert_eq!(w.connect_from_host(H2, m), Err(RouteErr::Refused));
        assert_eq!(w.accept_host_record(m, &h2_rec), Err(Refuse::Revoked));
        assert_eq!(w.snap(m), before);
    }
    assert_eq!(w.host_sync(H1, H2), Err(RouteErr::Refused));
    // Stale knowledge at the removed host until carol rotates.
    let carol_old = w.gep(CAROL);
    let k = w.hk(CAROL);
    assert_eq!(
        w.d[H2].host.as_ref().unwrap().directory[&k].claims.endpoint,
        carol_old
    );
    let n = w.frames.len();
    assert_eq!(w.rotate_member(CAROL), vec![H1]);
    assert!(
        w.frames[n..]
            .iter()
            .filter(|f| f.kind == "ep-note")
            .all(|f| f.to == H1)
    );
    assert!(!has(&w.received(H2), w.gep(CAROL).as_bytes()));
    w.post(A1, "after");
    w.round();
    w.assert_converged(&[H1, OLGA, A1, A2, BOB, CAROL, HP]);
    assert!(!w.d[H2].board.texts().contains("after"));
}

// KIND: candidate-policy G-AUTHORITY
// Which admins may remove a host ("newer admins can remove" — unconfirmed). Appointment
// order uses grant nonces as a fixture stand-in for an op log: alice (admin #1) was appointed
// before H1's permit (#10); bob (#20) after.
// - AnyAdmin: alice's removal of H1 is accepted.
// - AdminAppointedAfterHost: alice's removal is refused (state unchanged); bob's is accepted.
#[test]
fn host_removal_any_admin_vs_only_admins_appointed_after_host() {
    for rule in [RemovalRule::AnyAdmin, RemovalRule::AdminAppointedAfterHost] {
        let mut w = World::new(Cfg {
            removal: rule,
            ..Cfg::default()
        });
        let permit = w.d[H1].host.as_ref().unwrap().record.permit.digest();
        let by_alice = w.op(
            A1,
            Op::RemoveHost {
                permit: permit.clone(),
            },
        );
        let by_bob = w.op(
            BOB,
            Op::RemoveHost {
                permit: permit.clone(),
            },
        );
        let before = w.snap(CAROL);
        match rule {
            RemovalRule::AnyAdmin => {
                assert_eq!(w.accept_op(CAROL, &by_alice.wire()), Ok(true));
                assert!(w.d[CAROL].view.revoked.contains(&permit));
            }
            RemovalRule::AdminAppointedAfterHost => {
                assert_eq!(
                    w.accept_op(CAROL, &by_alice.wire()),
                    Err(Refuse::NotAppointedAfter)
                );
                assert_eq!(w.snap(CAROL), before);
                assert_eq!(w.accept_op(CAROL, &by_bob.wire()), Ok(true));
                assert!(w.d[CAROL].view.revoked.contains(&permit));
            }
        }
    }
}

// KIND: limitation
// U9 with two hosts, one rogue (ignores removal). Three distinct admins flag carol; every
// device verifies the flags. The honest host drops carol's directory entry, refuses her link
// and her posts; every honest member removes her. The rogue host keeps her endpoint and keeps
// feeding her new group posts: with no group-content encryption, removal cannot stop a rogue
// host from serving reads. Carol's later posts relayed by the rogue host are rejected by
// every honest replica with state unchanged.
#[test]
fn member_removal_applied_by_honest_hosts_but_rogue_host_still_serves_removed_member() {
    let mut w = world();
    w.rogue.insert(H2);
    let carol = w.did(CAROL);
    for admin in [OLGA, A1, BOB] {
        let op = w.op(
            admin,
            Op::Flag {
                target: carol.clone(),
            },
        );
        w.submit(admin, op);
    }
    w.round();
    for i in [OLGA, A1, A2, BOB, HP, H1] {
        assert!(w.d[i].view.removed.contains(&carol), "device {i}");
        assert!(!w.d[i].board.roster.contains(&carol), "device {i}");
    }
    let h1 = w.d[H1].host.as_ref().unwrap();
    assert!(h1.directory.values().all(|n| n.claims.owner != carol));
    assert!(w.d[H2].board.roster.contains(&carol));
    assert!(
        w.d[H2]
            .host
            .as_ref()
            .unwrap()
            .directory
            .contains_key(&w.hk(CAROL))
    );
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    // Rogue keeps serving reads.
    w.post(BOB, "after removal");
    w.round();
    assert!(w.d[CAROL].board.texts().contains("after removal"));
    // Carol's later post via the rogue host: refused by every honest replica, unchanged.
    let late = w.post(CAROL, "carol after removal");
    for i in [OLGA, A1, A2, BOB, HP, H1] {
        let before = w.d[i].board.snapshot();
        assert_eq!(
            w.d[i].board.receive(late.clone()),
            Verdict::Rejected(Why::NotMember)
        );
        assert_eq!(w.d[i].board.snapshot(), before);
    }
    w.round();
    assert!(w.d[H2].board.texts().contains("carol after removal"));
    for i in [OLGA, A1, A2, BOB, HP, H1] {
        assert!(!w.d[i].board.texts().contains("carol after removal"));
    }
}

// KIND: invariant
// U5 revised (visible) + section 8b Q1 (one signed notice, same reason on every later
// attempt):
// - Removal: forged flags (non-admin flagger, tampered target) change nothing at carol. After
//   three distinct admins flag her, an honest host answers her next connection with the
//   signed flags; carol verifies them and records that she is removed. A second attempt
//   returns the same notice and changes nothing.
// - Personal block: bob blocks carol; carol receives a DID-signed notice on the private path.
//   Forged or misaddressed notices are refused unchanged. Her later direct attempt gets the
//   byte-identical notice. The block changes no group roster, and bob and carol still see
//   each other's group posts (personal block != removal).
#[test]
fn removed_member_and_blocked_contact_receive_verifiable_signed_notices() {
    let mut w = world();
    let carol = w.did(CAROL);
    // --- personal block first (no group change) ---
    let rosters: Vec<_> = everyone()
        .iter()
        .map(|&i| w.d[i].board.roster.clone())
        .collect();
    let mallory = ident(37);
    let forged = {
        let body = BlockBody {
            from: w.did(BOB),
            to: carol.clone(),
            at: 0,
        };
        let sig = mallory.sign(&signed(NOTICE_DOMAIN, &body)).to_vec();
        serde_json::to_vec(&BlockNotice { body, sig }).unwrap()
    };
    let misaddressed = {
        let body = BlockBody {
            from: w.did(BOB),
            to: w.did(A1),
            at: 0,
        };
        let sig = w.d[BOB].id.sign(&signed(NOTICE_DOMAIN, &body)).to_vec();
        serde_json::to_vec(&BlockNotice { body, sig }).unwrap()
    };
    let before = w.snap(CAROL);
    assert_eq!(
        w.receive_block_notice(CAROL, &forged),
        Err(Refuse::BadSignature)
    );
    assert_eq!(
        w.receive_block_notice(CAROL, &misaddressed),
        Err(Refuse::NotAddressed)
    );
    assert_eq!(w.snap(CAROL), before);
    let bob = w.did(BOB);
    let notice = w.block(BOB, &carol);
    assert!(w.d[CAROL].view.blocked_by.contains_key(&bob));
    assert_eq!(w.direct(CAROL, &bob), Direct::Blocked(notice.clone()));
    assert_eq!(w.direct(CAROL, &bob), Direct::Blocked(notice));
    for (k, &i) in everyone().iter().enumerate() {
        assert_eq!(w.d[i].board.roster, rosters[k]);
    }
    w.post(BOB, "bob to group");
    w.post(CAROL, "carol to group");
    w.round();
    assert!(w.d[CAROL].board.texts().contains("bob to group"));
    assert!(w.d[BOB].board.texts().contains("carol to group"));

    // --- removal notice ---
    let gid = w.gid.clone();
    let hana_flag = SignedOp::sign(
        &w.d[HP].id,
        &gid,
        w.grants.get(&w.did(HP)).cloned(),
        80,
        Op::Flag {
            target: carol.clone(),
        },
    );
    let mut tampered = w.op(OLGA, Op::Flag { target: w.did(A2) });
    tampered.body.op = Op::Flag {
        target: carol.clone(),
    };
    let before = w.snap(CAROL);
    assert_eq!(w.accept_op(CAROL, &hana_flag.wire()), Err(Refuse::NotAdmin));
    assert_eq!(
        w.accept_op(CAROL, &tampered.wire()),
        Err(Refuse::BadSignature)
    );
    assert_eq!(w.snap(CAROL), before);
    w.cut(CAROL, H1);
    w.cut(CAROL, H2);
    for admin in [OLGA, A1, BOB] {
        let op = w.op(
            admin,
            Op::Flag {
                target: carol.clone(),
            },
        );
        w.submit(admin, op);
    }
    w.round();
    assert!(!w.d[CAROL].view.removed.contains(&carol), "not yet told");
    w.heal(CAROL, H1);
    let n = w.frames.len();
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    let notices: Vec<&Frame> = w.frames[n..]
        .iter()
        .filter(|f| f.to == CAROL && f.kind == "removal-notice")
        .collect();
    assert_eq!(notices.len(), 3);
    assert!(w.d[CAROL].view.removed.contains(&carol));
    assert_eq!(w.d[CAROL].view.flags[&carol].len(), 3);
    let after = w.snap(CAROL);
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    assert_eq!(w.snap(CAROL), after, "same notice again, no change");
}

// KIND: candidate-policy G-KEYS
// Host binary identity (open: phrase-holding linked device vs own DID + permit).
// - Own DID (hugo, H2): relays, but cannot produce any content a member accepts: posts it
//   signs are NotMember; it holds no member's key.
// - Phrase-holding linked device (hana's desktop H1): its key IS hana's DID key, so the
//   host process can post as hana, sign hana's endpoint notes and sign a block notice as
//   hana — all accepted. A server holding a phrase holds that user's whole identity.
#[test]
fn host_binary_own_did_vs_phrase_holding_linked_device() {
    let mut w = world();
    let gid = w.gid.clone();
    // Own-DID host.
    let mut hw = Writer::new(w.d[H2].id.clone(), 0x4242);
    let p = hw.post(&gid, "hugo as host");
    for m in MEMBERS {
        let before = w.d[m].board.snapshot();
        assert_eq!(
            w.d[m].board.receive(p.clone()),
            Verdict::Rejected(Why::NotMember)
        );
        assert_eq!(w.d[m].board.snapshot(), before);
    }
    assert!(MEMBERS.iter().all(|&m| w.did(m) != w.did(H2)));
    // Phrase-holding host device.
    assert_eq!(w.did(H1), w.did(HP));
    let mut pw = Writer::new(w.d[H1].id.clone(), 0x4343);
    let p = pw.post(&gid, "written by the host process as hana");
    assert_eq!(w.d[CAROL].board.receive(p), Verdict::Accepted);
    let body = BlockBody {
        from: w.did(H1),
        to: w.did(CAROL),
        at: 0,
    };
    let sig = w.d[H1].id.sign(&signed(NOTICE_DOMAIN, &body)).to_vec();
    let b = serde_json::to_vec(&BlockNotice { body, sig }).unwrap();
    assert_eq!(w.receive_block_notice(CAROL, &b), Ok(()));
    let note = EpNote::sign(
        &w.d[H1].id,
        Audience::Private,
        w.d[HP].slot,
        "prv-attacker",
        9,
    );
    assert!(note.verify() && note.claims.owner == w.did(HP));
}

// KIND: candidate-policy G-AUTHORITY
// Max number of hosts (user passed). With Max(2), a third valid permit is refused by members
// (CapReached, state unchanged); Unbounded accepts it. Exposure grows with the host count:
// after carol rotates, exactly the hosts hold her new group endpoint; no member does.
#[test]
fn host_count_cap_candidates_and_endpoint_exposure_scales_with_hosts() {
    for cap in [HostCap::Max(2), HostCap::Unbounded] {
        let mut w = World::new(Cfg {
            cap,
            ..Cfg::default()
        });
        let permit = w.default_permit(H3);
        w.make_host(H3, permit);
        let rec = w.d[H3].host.as_ref().unwrap().record.wire();
        match cap {
            HostCap::Max(_) => {
                for m in MEMBERS {
                    let before = w.snap(m);
                    assert_eq!(w.accept_host_record(m, &rec), Err(Refuse::CapReached));
                    assert_eq!(w.snap(m), before);
                }
            }
            HostCap::Unbounded => {
                w.publish_record(H3).unwrap();
                w.round();
                w.rotate_member(CAROL);
                let e = w.gep(CAROL);
                let holders: BTreeSet<usize> = (0..w.d.len())
                    .filter(|&i| i != CAROL && has(&w.received(i), e.as_bytes()))
                    .collect();
                assert_eq!(holders, [H1, H2, H3].into());
            }
        }
    }
}
