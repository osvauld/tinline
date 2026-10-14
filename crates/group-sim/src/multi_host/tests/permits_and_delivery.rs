use super::*;

// ---- tests ---------------------------------------------------------------------------------

// KIND: invariant
// Q5: a host is appointed by a permit in the grant/delegation chain (creator -> admin ->
// Host leaf bound to one device). Members refuse each single-cause bad record with their
// whole state byte-identical: a member grant presented as permit, a permit issued by a
// non-admin (under her member grant, or self-rooted), a missing permit, a tampered device,
// a permit for another device, a note signed by a non-holder, a permit lacking `Host`.
// Positive control: the same host with a valid admin-issued permit is accepted and relays.
// After an admin removes that permit (signed op), the record is refused as Revoked and the
// host's links are refused, state unchanged.
#[test]
fn host_permit_must_chain_to_an_admin_and_bind_the_device_else_members_refuse() {
    let mut w = world();
    let ivan = w.did(H3);
    let slot = w.d[H3].slot;
    let gid = w.gid.clone();
    let carol_grant = w.grants[&w.did(CAROL)].clone();
    let alice_admin = w.grants[&w.did(A1)].clone();
    let (carol_id, alice_id) = (w.d[CAROL].id.clone(), w.d[A1].id.clone());
    let note = w.d[H3].my_note.clone();
    let good = w.permit(A1, H3, slot, 12);
    let rec = |note: &EpNote, permit: MGrant| {
        HostRecord {
            note: note.clone(),
            permit,
        }
        .wire()
    };
    let mallory = ident(37);
    let mallory_note = EpNote::sign(&mallory, Audience::Group(gid.clone()), slot, &w.gep(H3), 1);
    let mut tampered = good.clone();
    tampered.body.device = Some(slot ^ 1);
    let cases: Vec<(&str, Vec<u8>, Refuse)> = vec![
        (
            "member grant as permit",
            rec(&note, carol_grant.clone()),
            Refuse::PermitInvalid,
        ),
        (
            "non-admin issues under her member grant",
            rec(
                &note,
                MGrant::issue(
                    &carol_id,
                    &gid,
                    &ivan,
                    Some(slot),
                    &[Cap::Host],
                    false,
                    40,
                    Some(carol_grant.clone()),
                ),
            ),
            Refuse::PermitInvalid,
        ),
        (
            "non-admin self-rooted",
            rec(
                &note,
                MGrant::issue(
                    &carol_id,
                    &gid,
                    &ivan,
                    Some(slot),
                    &[Cap::Host],
                    false,
                    41,
                    None,
                ),
            ),
            Refuse::PermitInvalid,
        ),
        (
            "no permit at all",
            serde_json::to_vec(&serde_json::json!({ "note": note })).unwrap(),
            Refuse::Malformed,
        ),
        (
            "tampered device",
            rec(&note, tampered),
            Refuse::PermitInvalid,
        ),
        (
            "permit for another device",
            rec(
                &note,
                MGrant::issue(
                    &alice_id,
                    &gid,
                    &ivan,
                    Some(slot ^ 1),
                    &[Cap::Host],
                    false,
                    42,
                    Some(alice_admin.clone()),
                ),
            ),
            Refuse::DeviceMismatch,
        ),
        (
            "note signed by non-holder",
            rec(&mallory_note, good.clone()),
            Refuse::PermitInvalid,
        ),
        (
            "permit lacks Host",
            rec(
                &note,
                MGrant::issue(
                    &alice_id,
                    &gid,
                    &ivan,
                    Some(slot),
                    &[Cap::Post],
                    false,
                    43,
                    Some(alice_admin.clone()),
                ),
            ),
            Refuse::PermitInvalid,
        ),
    ];
    for i in everyone() {
        for (name, bytes, want) in &cases {
            let before = w.snap(i);
            assert_eq!(w.accept_host_record(i, bytes), Err(*want), "{name} at {i}");
            assert_eq!(w.snap(i), before, "{name} at {i}");
        }
    }
    // H3 cannot open links: it is not a host anywhere.
    assert!(
        MEMBERS
            .iter()
            .all(|&m| !w.d[m].view.hosts.contains_key(&w.hk(H3)))
    );

    // Positive control: valid permit (alice, admin) -> accepted; H3 relays a post.
    w.make_host(H3, good.clone());
    w.publish_record(H3).unwrap();
    for i in everyone() {
        assert!(w.d[i].view.hosts.contains_key(&w.hk(H3)));
    }
    for m in [BOB, CAROL] {
        w.cut(m, H1);
        w.cut(m, H2);
        w.connect(m, H3).unwrap();
    }
    w.post(BOB, "via ivan");
    w.connect(BOB, H3).unwrap();
    w.connect(CAROL, H3).unwrap();
    assert!(w.d[CAROL].board.texts().contains("via ivan"));
    assert!(
        w.frames
            .iter()
            .filter(|f| f.to == CAROL && f.kind == "post")
            .all(|f| f.from == H3)
    );

    // Revoked: bob (admin) removes H3's permit; afterwards the record is refused unchanged.
    let op = w.op(
        BOB,
        Op::RemoveHost {
            permit: good.digest(),
        },
    );
    w.heal(BOB, H1);
    w.submit(BOB, op);
    w.heal(CAROL, H1);
    w.round();
    let good_rec = w.d[H3].host.as_ref().unwrap().record.wire();
    for m in [BOB, CAROL] {
        let k = w.hk(m);
        assert!(w.d[H3].host.as_ref().unwrap().directory.contains_key(&k));
    }
    for m in MEMBERS {
        assert!(!w.d[m].view.hosts.contains_key(&w.hk(H3)));
        let before = w.snap(m);
        assert_eq!(w.accept_host_record(m, &good_rec), Err(Refuse::Revoked));
        // Members whose endpoint H3 learned (it can dial them) refuse it; any others it
        // never had an address for.
        let known = w.d[H3]
            .host
            .as_ref()
            .unwrap()
            .directory
            .contains_key(&w.hk(m));
        let want = if known {
            RouteErr::Refused
        } else {
            RouteErr::Lost
        };
        assert_eq!(w.connect(m, H3), Err(want));
        assert!(w.connect_from_host(H3, m).is_err());
        assert_eq!(w.snap(m), before);
    }
}

// KIND: invariant
// Q5 "hosts are NOT necessarily admins": hugo's host binary (own DID, permit {Host} only)
// relays posts, but every authority act it signs is refused with state byte-identical at
// every member and at the other host: admitting a new member, flagging a member, removing a
// host, appointing another host. A grant it issues lets nobody post. Positive controls: the
// owner's admit of the same newcomer is accepted (NotAdmin is the sole cause), and H2's relay
// reaches members cut off from H1.
#[test]
fn host_without_admin_capability_relays_but_cannot_grant_flag_remove_or_appoint() {
    let mut w = world();
    let hugo = w.d[H2].id.clone();
    let permit = w.d[H2].host.as_ref().unwrap().record.permit.clone();
    let gid = w.gid.clone();
    let mallory = ident(37);
    let h1_permit = w.d[H1].host.as_ref().unwrap().record.permit.digest();
    let hugo_grant = MGrant::issue(
        &hugo,
        &gid,
        mallory.did(),
        None,
        &[Cap::Read, Cap::Post],
        false,
        50,
        Some(permit.clone()),
    );
    assert!(!hugo_grant.allows(&w.g, mallory.did(), Cap::Post));
    let ops = [
        Op::Admit {
            grant: hugo_grant.clone(),
        },
        Op::Flag {
            target: w.did(CAROL),
        },
        Op::RemoveHost { permit: h1_permit },
    ];
    let appoint = HostRecord {
        note: w.d[H3].my_note.clone(),
        permit: MGrant::issue(
            &hugo,
            &gid,
            &w.did(H3),
            Some(w.d[H3].slot),
            &[Cap::Host],
            false,
            51,
            Some(permit.clone()),
        ),
    }
    .wire();
    let mut mw = Writer::new(mallory.clone(), 0xABCD);
    let mallory_post = mw.post(&gid, "mallory via hugo's grant");
    let mut hw = Writer::new(hugo.clone(), 0xABCE);
    let hugo_post = hw.post(&gid, "hugo speaks");
    for i in MEMBERS.iter().copied().chain([H1]) {
        let before = w.snap(i);
        for (k, op) in ops.iter().enumerate() {
            let s = SignedOp::sign(&hugo, &gid, Some(permit.clone()), 60 + k as u64, op.clone());
            assert_eq!(w.accept_op(i, &s.wire()), Err(Refuse::NotAdmin), "op {k}");
        }
        assert_eq!(
            w.accept_host_record(i, &appoint),
            Err(Refuse::PermitInvalid)
        );
        for p in [&mallory_post, &hugo_post] {
            assert_eq!(
                w.d[i].board.receive(p.clone()),
                Verdict::Rejected(Why::NotMember)
            );
        }
        assert_eq!(w.snap(i), before, "device {i}");
    }
    // Relay works.
    for m in [BOB, CAROL] {
        w.cut(m, H1);
    }
    w.post(BOB, "relayed by hugo");
    w.connect(BOB, H2).unwrap();
    w.connect(CAROL, H2).unwrap();
    assert!(w.d[CAROL].board.texts().contains("relayed by hugo"));
    // Positive control: the owner's admit of mallory is accepted.
    let good = MGrant::issue(
        &w.d[OLGA].id,
        &gid,
        mallory.did(),
        None,
        &[Cap::Read, Cap::Post],
        false,
        52,
        None,
    );
    let admit = w.op(OLGA, Op::Admit { grant: good });
    assert_eq!(w.accept_op(CAROL, &admit.wire()), Ok(true));
    assert!(w.d[CAROL].board.roster.contains(mallory.did()));
}

// KIND: invariant
// Q5 "a host is one specific device of some user": the permit names hana's desktop (H1)
// by its random device ID. Her phone (HP, same DID, same mnemonic) honestly presenting the
// same permit with its own device ID is refused (DeviceMismatch); a permit hana issues to
// herself is refused (she is not an admin); state unchanged everywhere. HP remains an
// ordinary member: its posts converge through the real hosts. Device IDs are random, not
// derived from any key.
#[test]
fn host_permit_binds_one_device_and_a_sibling_of_the_same_did_is_not_a_host() {
    let mut w = world();
    assert_eq!(
        w.did(HP),
        w.did(H1),
        "fixture: hana's phone and desktop share a DID"
    );
    assert_ne!(w.d[HP].slot, w.d[H1].slot);
    for i in [HP, H1] {
        let keys = [
            w.d[i].id.signing_public_key(),
            w.d[i].id.device_public_key(),
            w.d[i].group_key.verifying_key().to_bytes(),
        ];
        for k in keys {
            for c in k.chunks(8) {
                assert_ne!(u64::from_le_bytes(c.try_into().unwrap()), w.d[i].slot);
            }
        }
    }
    let h1_permit = w.d[H1].host.as_ref().unwrap().record.permit.clone();
    let hana = w.d[HP].id.clone();
    let gid = w.gid.clone();
    let sibling = HostRecord {
        note: EpNote::sign(
            &hana,
            Audience::Group(gid.clone()),
            w.d[HP].slot,
            &w.gep(HP),
            2,
        ),
        permit: h1_permit,
    }
    .wire();
    let self_appointed = HostRecord {
        note: w.d[HP].my_note.clone(),
        permit: MGrant::issue(
            &hana,
            &gid,
            hana.did(),
            Some(w.d[HP].slot),
            &[Cap::Host],
            false,
            70,
            w.grants.get(hana.did()).cloned(),
        ),
    }
    .wire();
    for i in everyone() {
        let before = w.snap(i);
        assert_eq!(
            w.accept_host_record(i, &sibling),
            Err(Refuse::DeviceMismatch)
        );
        assert_eq!(
            w.accept_host_record(i, &self_appointed),
            Err(Refuse::PermitInvalid)
        );
        assert_eq!(w.snap(i), before);
        assert!(w.d[i].view.hosts.contains_key(&w.hk(H1)));
        assert!(!w.d[i].view.hosts.contains_key(&w.hk(HP)));
    }
    assert!(w.d[HP].host.is_none());
    w.post(HP, "hana from phone");
    w.round();
    w.assert_converged(&everyone());
    assert!(w.d[CAROL].board.texts().contains("hana from phone"));
}

// KIND: limitation
// Shared mnemonic: the device binding is a random ID, not a key. Hana's phone (same phrase)
// can sign a higher-seq host note claiming the DESKTOP's device ID with its own endpoint and
// present H1's genuine permit; members accept it and now dial the phone as "the host".
// Unlinking the phone does not revoke this (no strong revocation under a shared phrase).
#[test]
fn phrase_sharing_sibling_can_claim_the_host_device_id() {
    let mut w = world();
    let h1 = w.d[H1].host.as_ref().unwrap().record.clone();
    let hijack = HostRecord {
        note: EpNote::sign(
            &w.d[HP].id,
            Audience::Group(w.gid.clone()),
            w.d[H1].slot,
            &w.gep(HP),
            h1.note.claims.seq + 1,
        ),
        permit: h1.permit,
    };
    assert_eq!(w.accept_host_record(CAROL, &hijack.wire()), Ok(()));
    let e = w.d[CAROL].view.hosts[&w.hk(H1)]
        .note
        .claims
        .endpoint
        .clone();
    assert_eq!(e, w.gep(HP));
    assert_eq!(w.dial(CAROL, &e), Ok(HP));
}

// KIND: invariant
// Q5 host-to-host sync: alice-1 only reaches H1, carol only reaches H2. Alice's post
// uploaded to H1 is not at carol until H1 and H2 sync; afterwards carol gets it, and every
// post frame carol ever received came from H2. Everyone converges.
#[test]
fn post_via_one_host_reaches_member_served_only_by_another_host() {
    let mut w = world();
    w.cut(A1, H2);
    w.cut(CAROL, H1);
    w.post(A1, "alice via H1");
    assert_eq!(w.connect(A1, H1), Ok(Path::MemberDialedHost));
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Lost));
    w.connect(CAROL, H2).unwrap();
    assert!(!w.d[CAROL].board.texts().contains("alice via H1"));
    w.host_sync(H1, H2).unwrap();
    w.connect(CAROL, H2).unwrap();
    assert!(w.d[CAROL].board.texts().contains("alice via H1"));
    let carol_posts: Vec<&Frame> = w
        .frames
        .iter()
        .filter(|f| f.to == CAROL && f.kind == "post")
        .collect();
    assert!(!carol_posts.is_empty() && carol_posts.iter().all(|f| f.from == H2));
    w.round();
    w.assert_converged(&everyone());
}

// KIND: invariant
// Two hosts partitioned, each with its own members writing; each side converges internally
// but differs from the other. After the host link heals, everyone converges to all posts
// and no device loses a local write (no rollback).
#[test]
fn two_hosts_partitioned_then_healed_converge_without_rollback() {
    let mut w = world();
    let left = [OLGA, A1, BOB];
    let right = [A2, CAROL, HP];
    w.cut(H1, H2);
    for m in left {
        w.cut(m, H2);
    }
    for m in right {
        w.cut(m, H1);
    }
    let writes = [
        (A1, "a1 left"),
        (BOB, "bob left"),
        (A2, "a2 right"),
        (CAROL, "carol right"),
    ];
    for (m, t) in writes {
        w.post(m, t);
    }
    w.round();
    assert_eq!(w.host_sync(H1, H2), Err(RouteErr::Lost));
    w.assert_converged(&[H1, OLGA, A1, BOB]);
    w.assert_converged(&[H2, A2, CAROL, HP]);
    assert_eq!(w.d[H1].board.texts(), set(&["a1 left", "bob left"]));
    assert_eq!(w.d[H2].board.texts(), set(&["a2 right", "carol right"]));
    w.heal(H1, H2);
    w.round();
    w.assert_converged(&everyone());
    let all = set(&["a1 left", "bob left", "a2 right", "carol right"]);
    assert_eq!(w.d[HP].board.texts(), all);
    for (m, t) in writes {
        assert!(w.d[m].board.texts().contains(t));
    }
}

// KIND: invariant
// A member's rotation note goes to EVERY host (one frame each, no member sees it). When one
// host is unreachable, the other host repairs it during host sync; the repaired host then
// reaches the member at the new endpoint. A note altered in relay (endpoint swapped) or a
// stale note is refused by the receiving host with its state byte-identical. Old endpoints
// stop listening.
#[test]
fn member_rotation_note_reaches_every_host_and_hosts_repair_each_other() {
    let mut w = world();
    let old = w.gep(CAROL);
    let first = w.d[CAROL].my_note.clone();
    let reached = w.rotate_member(CAROL);
    assert_eq!(reached, vec![H1, H2]);
    let notes: Vec<&Frame> = w
        .frames
        .iter()
        .filter(|f| f.from == CAROL && f.kind == "ep-note")
        .collect();
    assert_eq!(notes.iter().map(|f| f.to).collect::<Vec<_>>(), vec![H1, H2]);
    assert!(w.live.get(&old).is_none());
    // Second rotation with H2 unreachable: only H1 learns it.
    w.cut(CAROL, H2);
    let reached = w.rotate_member(CAROL);
    assert_eq!(reached, vec![H1]);
    let new = w.gep(CAROL);
    assert_eq!(w.connect_from_host(H2, CAROL), Err(RouteErr::EndpointGone));
    // Tampered relay and stale note are refused at H2 with state unchanged.
    let mut tampered = w.d[CAROL].my_note.clone();
    tampered.claims.endpoint = w.gep(BOB);
    let before = w.snap(H2);
    assert_eq!(
        w.host_accept_note(H2, &tampered.wire()),
        Err(Refuse::BadSignature)
    );
    assert_eq!(w.host_accept_note(H2, &first.wire()), Err(Refuse::Stale));
    assert_eq!(w.snap(H2), before);
    w.host_sync(H1, H2).unwrap();
    let k = w.hk(CAROL);
    assert_eq!(
        w.d[H2].host.as_ref().unwrap().directory[&k].claims.endpoint,
        new
    );
    w.heal(CAROL, H2);
    assert_eq!(w.connect_from_host(H2, CAROL), Ok(Path::HostDialedMember));
    for m in MEMBERS {
        if m != CAROL {
            assert!(!has(&w.received(m), new.as_bytes()));
        }
    }
}

// KIND: invariant
// Q6: after H1 rotates, its old endpoint stops listening at once (no overlap window). H1
// itself dials every online member at that member's CURRENT endpoint (including one that
// rotated earlier) and hands over its new record. A member offline at the time (and cut
// from H2) is recovered later by H1 dialing it, not by the member finding H1. Members refuse
// a forged H1 record and the stale one with state unchanged.
#[test]
fn rotated_host_reconnects_members_at_their_current_endpoints_without_overlap() {
    let mut w = world();
    w.rotate_member(CAROL);
    w.d[BOB].online = false;
    w.cut(BOB, H2);
    let old_h1 = w.gep(H1);
    let stale = w.d[H1].host.as_ref().unwrap().record.clone();
    let n_frames = w.frames.len();
    let reached = w.rotate_host(H1);
    assert!(w.live.get(&old_h1).is_none(), "no overlap window");
    assert_eq!(
        reached.iter().copied().collect::<BTreeSet<_>>(),
        [OLGA, A1, A2, CAROL, HP].into()
    );
    for f in w.frames[n_frames..]
        .iter()
        .filter(|f| f.from == H1 && f.kind == "host-record" && MEMBERS.contains(&f.to))
    {
        assert_eq!(f.to_ep, w.gep(f.to), "host dials member's current endpoint");
    }
    let new_h1 = w.gep(H1);
    for m in [OLGA, A1, A2, CAROL, HP] {
        assert_eq!(w.d[m].view.hosts[&w.hk(H1)].note.claims.endpoint, new_h1);
    }
    // Other host learned it too.
    assert_eq!(w.d[H2].view.hosts[&w.hk(H1)].note.claims.endpoint, new_h1);
    // Bob returns: his only H1 address is dead, H2 is cut.
    w.d[BOB].online = true;
    let e = w.d[BOB].view.hosts[&w.hk(H1)].note.claims.endpoint.clone();
    assert_eq!(w.dial(BOB, &e), Err(RouteErr::EndpointGone));
    assert_eq!(w.connect_from_host(H1, BOB), Ok(Path::HostDialedMember));
    assert_eq!(w.d[BOB].view.hosts[&w.hk(H1)].note.claims.endpoint, new_h1);
    // Forged and stale records.
    let forged = HostRecord {
        note: EpNote::sign(
            &ident(37),
            Audience::Group(w.gid.clone()),
            w.d[H1].slot,
            &w.gep(BOB),
            99,
        ),
        permit: stale.permit.clone(),
    };
    for m in MEMBERS {
        let before = w.snap(m);
        assert_eq!(
            w.accept_host_record(m, &forged.wire()),
            Err(Refuse::PermitInvalid)
        );
        assert_eq!(w.accept_host_record(m, &stale.wire()), Err(Refuse::Stale));
        assert_eq!(w.snap(m), before);
    }
}

// KIND: invariant
// Host H1 and carol both rotated while apart: carol's H1 address is dead and H1's address
// for carol is dead (Lost both ways). With a second host that did not rotate, carol reaches
// H2, uploads her new note, and learns H1's new record from H2; after host sync H1 can dial
// carol itself, and carol can dial H1. No re-share needed while >= 1 host is unchanged.
#[test]
fn host_and_member_both_rotated_while_apart_recover_via_another_host() {
    let mut w = world();
    w.d[CAROL].online = false;
    w.rotate_host(H1);
    w.rotate_member(CAROL);
    w.d[CAROL].online = true;
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Lost));
    assert_eq!(w.connect(CAROL, H2), Ok(Path::MemberDialedHost));
    assert_eq!(
        w.d[CAROL].view.hosts[&w.hk(H1)].note.claims.endpoint,
        w.gep(H1)
    );
    w.host_sync(H1, H2).unwrap();
    assert_eq!(w.connect_from_host(H1, CAROL), Ok(Path::HostDialedMember));
    assert_eq!(w.connect(CAROL, H1), Ok(Path::MemberDialedHost));
    w.post(CAROL, "back");
    w.round();
    w.assert_converged(&everyone());
}
