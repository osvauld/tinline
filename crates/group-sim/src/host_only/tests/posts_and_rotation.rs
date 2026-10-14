use super::*;

// ---- tests ---------------------------------------------------------------------------------

// KIND: invariant
// U4 revised: a post is (DID, random writer ID, per-writer seq, text, Loro update) signed by
// the DID key; nothing else. Two devices of the same DID write concurrently (neither has seen
// the other), each under its own random writer ID == Loro peer ID; host relay converges
// every replica. The post verifies against the DID's public key alone (no attestation).
// Writer IDs come from the seeded RNG: same identities, different seed -> different writers.
#[test]
fn did_signed_posts_with_random_writer_ids_two_own_devices_converge() {
    let mut w = world();
    let alice = w.did(A1);
    assert_eq!(alice, w.did(A2), "fixture: one mnemonic, two devices");
    let (wa1, wa2) = (w.d[A1].w.writer, w.d[A2].w.writer);
    assert_ne!(wa1, wa2);
    assert_eq!(w.d[A1].w.doc.peer_id(), wa1);
    assert_eq!(w.d[A2].w.doc.peer_id(), wa2);
    // Not derived from any key the device holds (sanity check on the construction).
    for i in ALL {
        let dv = &w.d[i];
        let keys = [
            dv.id.signing_public_key(),
            dv.id.device_public_key(),
            dv.id.encryption_public_key(),
            tkey(&dv.group_key),
            tkey(&dv.private_key),
        ];
        for k in keys {
            for c in k.chunks(8) {
                assert_ne!(u64::from_le_bytes(c.try_into().unwrap()), dv.w.writer);
                assert_ne!(u64::from_be_bytes(c.try_into().unwrap()), dv.w.writer);
            }
        }
    }
    let other = World::new(Cfg {
        seed: 0x5EED_0002,
        ..Cfg::default()
    });
    assert_eq!(other.did(A1), alice);
    assert_ne!(other.d[A1].w.writer, wa1);

    let p1 = w.post(A1, "alice on phone").unwrap();
    let p2 = w.post(A2, "alice on laptop").unwrap();
    let pb = w.post(BOB, "bob").unwrap();
    assert_eq!((p1.body.seq, p2.body.seq), (1, 1));
    // Exactly these fields; DID key verifies; no attestation or device field exists.
    let v: serde_json::Value = serde_json::from_slice(&p1.wire()).unwrap();
    let fields: BTreeSet<&str> = v["body"]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    assert_eq!(
        fields,
        ["author", "doc", "group", "seq", "text", "update", "writer"].into()
    );
    assert_eq!(v.as_object().unwrap().len(), 2, "body + sig only");
    let alice_key = public_key_from_did(&alice).unwrap();
    let sig: [u8; 64] = p1.sig.clone().try_into().unwrap();
    assert!(verify(&alice_key, &Post::signed_bytes(&p1.body), &sig));
    assert_eq!(
        LoroDoc::decode_import_blob_meta(&p1.body.update, true)
            .unwrap()
            .partial_end_vv
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![wa1]
    );
    for i in [A2, A1, BOB, CAROL] {
        assert!(w.reconnect(i).is_ok());
    }
    w.assert_converged(&ALL);
    let texts = w.d[CAROL].board.texts();
    for p in [&p1, &p2, &pb] {
        assert!(texts.contains(&p.body.text));
    }
    // Both devices' posts are attributed to the same DID under distinct writer IDs.
    let alice_writers: BTreeSet<u64> = w.d[HOST]
        .board
        .accepted
        .values()
        .filter(|p| p.body.author == alice)
        .map(|p| p.body.writer)
        .collect();
    assert_eq!(alice_writers, [wa1, wa2].into());
    // A follow-up from each device continues its own per-writer sequence after observing.
    let q1 = w.post(A1, "phone again").unwrap();
    let q2 = w.post(A2, "laptop again").unwrap();
    assert_eq!((q1.body.seq, q2.body.seq), (2, 2));
    for i in [A1, A2] {
        w.reconnect(i).unwrap();
    }
    w.assert_converged(&ALL);
    assert_eq!(w.d[HOST].board.accepted.len(), 5);
}

// KIND: invariant
// U7: scan every byte each member device received (all frames + its replica state + its
// host note). No device key (identity device key = today's EndpointId), and no group or
// private transport key / endpoint of any other device — including its own sibling device
// and every rotated-out key — appears. Positive controls: each member holds the host's
// CURRENT group endpoint; the host's received bytes DO contain members' group endpoints, so
// the scanner finds them when present.
#[test]
fn members_receive_no_device_key_transport_key_or_endpoint_of_any_other_member() {
    let mut w = world();
    for (i, t) in [(A1, "a1"), (A2, "a2"), (BOB, "b"), (CAROL, "c")] {
        w.post(i, t);
    }
    w.post(HOST, "host speaks");
    for i in [A1, A2, BOB, CAROL] {
        w.reconnect(i).unwrap();
    }
    w.rotate_member_group(BOB);
    w.rotate_member_group(A2);
    w.rotate_host();
    w.post(CAROL, "after rotations");
    for i in [A1, A2, BOB, CAROL] {
        w.reconnect(i).unwrap();
    }
    w.assert_converged(&ALL);
    let device_keys: Vec<[u8; 32]> = ALL.iter().map(|&i| w.d[i].id.device_public_key()).collect();
    for m in [A1, A2, BOB, CAROL] {
        let hay = w.received(m);
        for k in &device_keys {
            assert!(!leaks_key(&hay, k), "device key reached member {m}");
        }
        for (owner, kind, k) in &w.keys_ever {
            if *owner == m || (*owner == HOST && *kind == KeyKind::Group) {
                continue;
            }
            assert!(
                !leaks_key(&hay, k),
                "member {m} received {kind:?} key of device {owner}"
            );
        }
        // Positive control: the host's current group endpoint is known.
        assert!(has(&hay, w.d[HOST].group_ep().as_bytes()));
        assert!(w.d[m].private_book.is_empty());
    }
    // Scanner sensitivity: the host's received bytes contain every member's group endpoint.
    let host_hay = w.received(HOST);
    for m in [A1, A2, BOB, CAROL] {
        assert!(leaks_key(&host_hay, &tkey(&w.d[m].group_key)));
    }
}

// KIND: invariant
// W2-G `author_key_equal_to_private_transport_key_leaks_private_endpoint_to_blocked_member`
// (block_in_group.rs:753) re-run under this model: Bob blocks Carol, announces and twice
// rotates his private endpoint to unblocked contacts only, posts in the group, Carol posts
// and is fully served. Carol learns none of Bob's private endpoints, nor his group endpoint,
// nor his device key, from anything she received: there is no key field in a post to carry
// one. Positive controls: Alice (unblocked) holds Bob's latest private endpoint; Carol holds
// and verifies Bob's posts; her replica equals the host's.
#[test]
fn blocked_member_learns_no_private_endpoint_from_did_signed_group_content() {
    let mut w = world();
    let (bob, carol) = (w.did(BOB), w.did(CAROL));
    w.d[BOB].blocked.insert(carol.clone());
    for _ in 0..3 {
        let to = w.announce_private(BOB);
        assert!(!to.contains(&CAROL));
        assert_eq!(to, vec![HOST, A1, A2]);
    }
    w.post(BOB, "hi from bob");
    w.post(CAROL, "hi from carol");
    for i in [BOB, CAROL, A1, A2] {
        w.reconnect(i).unwrap();
    }
    w.post(BOB, "bob again");
    w.reconnect(BOB).unwrap();
    w.assert_converged(&ALL);
    let hay = w.received(CAROL);
    let bob_private: Vec<&[u8; 32]> = w
        .keys_ever
        .iter()
        .filter(|(o, k, _)| *o == BOB && *k == KeyKind::Private)
        .map(|(_, _, k)| k)
        .collect();
    assert_eq!(bob_private.len(), 3);
    for k in &bob_private {
        assert!(
            !leaks_key(&hay, k),
            "Bob's private endpoint leaked to Carol"
        );
    }
    assert!(!leaks_key(&hay, &tkey(&w.d[BOB].group_key)));
    assert!(!leaks_key(&hay, &w.d[BOB].id.device_public_key()));
    assert!(!w.d[CAROL].private_book.contains_key(&bob));
    assert_eq!(w.dial(CAROL, &bob), Err(RouteErr::NoAddress));
    // Positive controls.
    assert_eq!(w.d[A1].private_book[&bob], w.d[BOB].private_ep());
    assert_eq!(w.dial(A1, &bob), Ok(w.d[BOB].private_ep()));
    let bob_posts: Vec<&Post> = w.d[CAROL]
        .board
        .accepted
        .values()
        .filter(|p| p.body.author == bob)
        .collect();
    assert_eq!(bob_posts.len(), 2);
    assert!(bob_posts.iter().all(|p| p.verify()));
}

// KIND: invariant
// U7 topology by construction: a member's address book is {host note} ∪ private book; no
// field can hold another member's group endpoint. With no private relationship, every
// member-to-member dial is `NoAddress` in both directions, while each member reaches the
// host and the host reaches each member device. Every group frame has the host at one end.
#[test]
fn member_to_member_delivery_has_no_address_by_construction() {
    let mut w = world();
    for i in [A1, BOB, CAROL] {
        w.post(i, "x");
        w.reconnect(i).unwrap();
    }
    for a in [A1, A2, BOB, CAROL] {
        for b in [A1, A2, BOB, CAROL] {
            if w.did(a) != w.did(b) {
                assert_eq!(w.dial(a, &w.did(b)), Err(RouteErr::NoAddress));
            }
        }
        assert_eq!(w.dial(a, &w.did(HOST)), Ok(w.d[HOST].group_ep()));
        assert!(w.reach_host(a).is_ok());
        assert_eq!(w.host_reach(a), Ok(w.d[a].group_ep()));
    }
    assert!(!w.frames.is_empty());
    for f in &w.frames {
        assert!(
            f.from == HOST || f.to == HOST,
            "{} frame bypassed host",
            f.kind
        );
    }
    // The transport itself would deliver (positive control): it is the address that is
    // missing, not connectivity.
    assert_eq!(w.listening(&w.d[CAROL].group_ep()), Some(CAROL));
}

// KIND: invariant
// Member group rotation: Bob's DID-signed note goes to the host only (one frame); the host
// routes later posts to the new endpoint; the old endpoint is retired (no listener). Forged,
// replayed/stale, private-audience, wrong-group and non-member notes leave the host state
// byte-identical. No member ever receives either of Bob's group endpoints.
#[test]
fn member_rotation_note_goes_to_host_only_and_host_routes_to_new_endpoint() {
    let mut w = world();
    let old_ep = w.d[BOB].group_ep();
    let before_frames = w.frames.len();
    let note = w.rotate_member_group(BOB);
    let new_ep = w.d[BOB].group_ep();
    let sent: Vec<&Frame> = w.frames[before_frames..].iter().collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].from, sent[0].to, sent[0].kind),
        (BOB, HOST, "ep-note")
    );
    assert_eq!(sent[0].bytes, note);
    let key = dir_key(&w.did(BOB), w.d[BOB].slot);
    assert_eq!(w.directory[&key].claims.endpoint, new_ep);
    assert_eq!(w.directory[&key].claims.seq, 2);
    assert_eq!(w.listening(&old_ep), None);
    assert_eq!(w.host_reach(BOB), Ok(new_ep.clone()));
    // Host delivers Alice's post to Bob's new endpoint.
    w.post(A1, "for everyone");
    w.reconnect(A1).unwrap();
    let to_bob: Vec<&Frame> = w
        .frames
        .iter()
        .filter(|f| f.to == BOB && f.kind == "post")
        .collect();
    assert_eq!(to_bob.last().unwrap().to_ep, new_ep);
    assert!(w.d[BOB].board.texts().contains("for everyone"));
    // Rejections: host state unchanged.
    let bob_id = w.d[BOB].id.clone();
    let carol_id = w.d[CAROL].id.clone();
    let outsider = ident(99);
    let slot = w.d[BOB].slot;
    let mut forged = EpNote::sign(
        &carol_id,
        Audience::Group(GROUP.into()),
        slot,
        "grp-evil",
        9,
    );
    forged.claims.owner = w.did(BOB);
    let mut tampered: EpNote = serde_json::from_slice(&note).unwrap();
    tampered.claims.endpoint = "grp-evil".into();
    let cases: Vec<(Vec<u8>, NoteReject)> = vec![
        (note.clone(), NoteReject::Stale),
        (
            EpNote::sign(&bob_id, Audience::Group(GROUP.into()), slot, &old_ep, 1).wire(),
            NoteReject::Stale,
        ),
        (forged.wire(), NoteReject::BadSignature),
        (tampered.wire(), NoteReject::BadSignature),
        (
            EpNote::sign(&bob_id, Audience::Private, slot, "prv-x", 9).wire(),
            NoteReject::WrongAudience,
        ),
        (
            EpNote::sign(&bob_id, Audience::Group("other".into()), slot, "grp-x", 9).wire(),
            NoteReject::WrongAudience,
        ),
        (
            EpNote::sign(&outsider, Audience::Group(GROUP.into()), 1, "grp-x", 9).wire(),
            NoteReject::NotMember,
        ),
        (b"not json".to_vec(), NoteReject::Malformed),
    ];
    for (bytes, why) in cases {
        let before = w.host_snapshot();
        assert_eq!(w.host_accept_note(&bytes), Err(why));
        assert_eq!(w.host_snapshot(), before);
    }
    for m in [A1, A2, CAROL] {
        let hay = w.received(m);
        assert!(!has(&hay, old_ep.as_bytes()) && !has(&hay, new_ep.as_bytes()));
    }
}

// KIND: invariant
// Host rotation: online members get the DID-signed host note pushed directly (host knows
// their group endpoints). Carol was offline; the old host endpoint retired immediately, so
// her dial fails (EndpointGone), but her own group endpoint is still live and the host's
// retry reaches her (HostDialedMember): she gets the new note, uploads her queued post and
// converges. A forged/stale host note is refused by members with state unchanged.
#[test]
fn host_rotation_reaches_online_members_directly_and_offline_member_by_host_retry() {
    let mut w = world();
    w.d[CAROL].online = false;
    w.post(CAROL, "written offline");
    let before = w.frames.len();
    let note = w.rotate_host();
    let new_host = w.d[HOST].group_ep();
    let pushed: BTreeSet<usize> = w.frames[before..]
        .iter()
        .filter(|f| f.kind == "host-note")
        .map(|f| f.to)
        .collect();
    assert_eq!(pushed, [A1, A2, BOB].into());
    for i in [A1, A2, BOB] {
        assert_eq!(w.reach_host(i), Ok(new_host.clone()));
    }
    w.d[CAROL].online = true;
    assert_eq!(w.reach_host(CAROL), Err(RouteErr::EndpointGone));
    // Forged/stale host notes are refused by Carol, unchanged.
    let snap = serde_json::to_vec(&w.d[CAROL].host_note).unwrap();
    let mut forged: EpNote = serde_json::from_slice(&note).unwrap();
    forged.claims.endpoint = "grp-evil".into();
    let bob_id = w.d[BOB].id.clone();
    let by_bob = EpNote::sign(&bob_id, Audience::Group(GROUP.into()), 0, "grp-evil", 9);
    let old = EpNote::sign(
        &w.d[HOST].id.clone(),
        Audience::Group(GROUP.into()),
        w.d[HOST].slot,
        "grp-old",
        1,
    );
    for (b, why) in [
        (forged.wire(), NoteReject::BadSignature),
        (by_bob.wire(), NoteReject::WrongOwner),
        (old.wire(), NoteReject::Stale),
    ] {
        assert_eq!(w.member_accept_host_note(CAROL, &b), Err(why));
        assert_eq!(serde_json::to_vec(&w.d[CAROL].host_note).unwrap(), snap);
    }
    assert_eq!(w.reconnect(CAROL), Ok(Path::HostDialedMember));
    assert_eq!(w.reach_host(CAROL), Ok(new_host));
    assert_eq!(w.d[CAROL].host_note.as_ref().unwrap().wire(), note);
    assert!(w.d[HOST].board.texts().contains("written offline"));
    w.assert_converged(&ALL);
}

// KIND: limitation
// Host and Carol both rotate their group endpoints while apart (Carol offline, immediate
// retirement). Carol's note cannot reach the host; the host's push cannot reach Carol; Carol
// has no other member's address to ask (U7: no member forwarding). Both directions fail and
// she is cut off from the group: "lost contact — re-share" (decision Q22). Content written
// meanwhile never reaches her.
#[test]
fn host_and_member_both_rotated_while_apart_lose_each_other() {
    let mut w = world();
    w.d[CAROL].online = false;
    w.rotate_member_group(CAROL);
    assert!(w.d[CAROL].unsent_note.is_some());
    w.rotate_host();
    w.post(BOB, "carol never sees this");
    w.reconnect(BOB).unwrap();
    w.d[CAROL].online = true;
    assert_eq!(w.reach_host(CAROL), Err(RouteErr::EndpointGone));
    assert_eq!(w.host_reach(CAROL), Err(RouteErr::EndpointGone));
    for m in [A1, BOB] {
        assert_eq!(w.dial(CAROL, &w.did(m)), Err(RouteErr::NoAddress));
    }
    assert_eq!(w.reconnect(CAROL), Err(RouteErr::Lost));
    assert!(!w.d[CAROL].board.texts().contains("carol never sees this"));
}

// KIND: candidate-policy G-TIME
// Same both-rotated scenario with the host keeping its old endpoint listening for a window
// (overlap candidate) vs retiring it at once. Within the window Carol dials the retiring
// endpoint, receives the new note, delivers her own pending rotation note and converges;
// after the window she is lost exactly as with immediate retirement. The window value, and
// whether any overlap is acceptable, is open (Section 2 timing).
#[test]
fn host_overlap_window_candidate_recovers_member_only_within_window() {
    for (overlap, wait, expect) in [
        (HostOverlap::Immediate, 1, Err(RouteErr::Lost)),
        (
            HostOverlap::Ticks(5),
            3,
            Ok(Path::MemberDialedRetiringHostEndpoint),
        ),
        (HostOverlap::Ticks(5), 7, Err(RouteErr::Lost)),
    ] {
        let mut w = World::new(Cfg {
            overlap,
            ..Cfg::default()
        });
        w.d[CAROL].online = false;
        w.rotate_member_group(CAROL);
        w.rotate_host();
        w.post(BOB, "meanwhile");
        w.reconnect(BOB).unwrap();
        w.tick(wait);
        w.d[CAROL].online = true;
        assert_eq!(w.reconnect(CAROL), expect, "{overlap:?} after {wait}");
        if expect.is_ok() {
            assert_eq!(w.host_reach(CAROL), Ok(w.d[CAROL].group_ep()));
            assert_eq!(w.reach_host(CAROL), Ok(w.d[HOST].group_ep()));
            w.assert_converged(&ALL);
        }
    }
}

// KIND: limitation
// Host offline: the group stalls. Members keep writing locally (queued), cannot deliver to
// each other (no address) or to the host, and see nothing new. When the host returns, every
// queue flushes and all replicas converge. U7 accepts this; host succession/backup host is
// open (D01–D06).
#[test]
fn host_offline_stalls_group_writes_queue_and_converge_on_return() {
    let mut w = world();
    w.post(BOB, "before outage");
    w.reconnect(BOB).unwrap();
    let base: Vec<Vec<u8>> = ALL.iter().map(|&i| w.d[i].board.snapshot()).collect();
    w.d[HOST].online = false;
    w.post(A1, "a during outage");
    w.post(BOB, "b during outage");
    w.post(CAROL, "c during outage");
    for i in [A1, A2, BOB, CAROL] {
        assert_eq!(w.reconnect(i), Err(RouteErr::PeerOffline));
        assert_eq!(w.dial(i, &w.did(HOST)), Err(RouteErr::PeerOffline));
        for j in [A1, BOB, CAROL] {
            if w.did(i) != w.did(j) {
                assert_eq!(w.dial(i, &w.did(j)), Err(RouteErr::NoAddress));
            }
        }
    }
    assert_eq!(w.d[A1].outbox.len(), 1);
    for (k, &i) in ALL.iter().enumerate() {
        assert_eq!(
            w.d[i].board.snapshot(),
            base[k],
            "board {i} changed while host offline"
        );
    }
    w.d[HOST].online = true;
    for i in [A1, A2, BOB, CAROL] {
        w.reconnect(i).unwrap();
    }
    w.assert_converged(&ALL);
    assert_eq!(w.d[HOST].board.accepted.len(), 4);
}

// KIND: candidate-policy G-STORAGE
// Local queue while the host is offline: Unbounded keeps every write; Refuse(2) refuses the
// third write locally with the device's writer doc, outbox and board byte-identical (no
// silent drop of an already-queued post). Both converge on host return. Bound value, and
// whether refuse vs drop-oldest vs unbounded is right, is open.
#[test]
fn offline_queue_bound_candidates_unbounded_vs_refuse_new() {
    for (queue, expect) in [(QueueBound::Unbounded, 3), (QueueBound::Refuse(2), 2)] {
        let mut w = World::new(Cfg {
            queue,
            ..Cfg::default()
        });
        w.d[HOST].online = false;
        assert!(w.post(A1, "q1").is_some());
        assert!(w.post(A1, "q2").is_some());
        let before = (
            w.d[A1].w.doc.export(ExportMode::all_updates()).unwrap(),
            w.d[A1].w.seq,
            serde_json::to_vec(&w.d[A1].outbox).unwrap(),
            w.d[A1].board.snapshot(),
        );
        let third = w.post(A1, "q3");
        if queue == QueueBound::Unbounded {
            assert!(third.is_some());
        } else {
            assert!(third.is_none());
            let after = (
                w.d[A1].w.doc.export(ExportMode::all_updates()).unwrap(),
                w.d[A1].w.seq,
                serde_json::to_vec(&w.d[A1].outbox).unwrap(),
                w.d[A1].board.snapshot(),
            );
            assert_eq!(after, before);
        }
        w.d[HOST].online = true;
        w.reconnect(A1).unwrap();
        for i in [A2, BOB, CAROL] {
            w.reconnect(i).unwrap();
        }
        w.assert_converged(&ALL);
        assert_eq!(w.d[HOST].board.accepted.len(), expect);
    }
}

// Populated host board: alice-1 seq1, bob seq1 (after observing alice), alice-1 seq3 pending
// (seq2 withheld). Returns (world, withheld seq2).
pub(super) fn populated() -> (World, Post) {
    let mut w = world();
    w.post(A1, "a1");
    w.reconnect(A1).unwrap();
    w.post(BOB, "b1");
    w.reconnect(BOB).unwrap();
    let a2 = w.post(A1, "a2").unwrap();
    let a3 = w.post(A1, "a3").unwrap();
    w.d[A1].outbox.clear();
    assert_eq!(w.d[HOST].board.receive(a3), Verdict::Pending);
    assert_eq!(w.d[HOST].board.accepted.len(), 2);
    (w, a2)
}
pub(super) fn resign(id: &Identity, mut p: Post) -> Post {
    p.sig = id.sign(&Post::signed_bytes(&p.body)).to_vec();
    p
}

// KIND: invariant
// Rejections on a populated host board (accepted history + one pending post), each compared
// on the full serialized board before/after, each a single cause (re-signed where needed):
// forged DID signature (Bob signs a body claiming Alice), tampered text, wrong group, wrong
// doc, non-member DID, seq 0, Loro update whose ops belong to another writer ID, text not
// matching the Loro op. Replays return Duplicate / Pending without change. A member's own
// board refuses a host-tampered relay. Positive control: the withheld post then drains.
#[test]
fn forged_tampered_wrong_group_and_replayed_posts_leave_state_unchanged() {
    let (mut w, a2) = populated();
    let alice = w.d[A1].id.clone();
    let bob = w.d[BOB].id.clone();
    let outsider = ident(99);
    let mut forged = a2.clone();
    forged.body.text = "forged".into();
    let forged = resign(&bob, forged);
    let mut tampered = a2.clone();
    tampered.body.text = "tampered".into();
    let mut wrong_group = a2.clone();
    wrong_group.body.group = "other".into();
    let mut wrong_doc = a2.clone();
    wrong_doc.body.doc = "group/other/2026-10-10".into();
    let mut not_member = a2.clone();
    not_member.body.author = outsider.did().into();
    let mut seq0 = a2.clone();
    seq0.body.seq = 0;
    let mut other_writer = a2.clone();
    other_writer.body.writer = w.d[A2].w.writer;
    let mut text_mismatch = a2.clone();
    text_mismatch.body.text = "says something else".into();
    let cases = vec![
        (forged, Why::BadSignature),
        (tampered, Why::BadSignature),
        (resign(&alice, wrong_group), Why::WrongGroup),
        (resign(&alice, wrong_doc), Why::WrongGroup),
        (resign(&outsider, not_member), Why::NotMember),
        (resign(&alice, seq0), Why::Bounds),
        (resign(&alice, other_writer), Why::LoroMismatch),
        (resign(&alice, text_mismatch), Why::LoroMismatch),
    ];
    let board = &mut w.d[HOST].board;
    for (p, why) in cases {
        let before = board.snapshot();
        assert_eq!(board.receive(p), Verdict::Rejected(why));
        assert_eq!(board.snapshot(), before, "{why:?} mutated the board");
    }
    for id in board.order.clone() {
        let before = board.snapshot();
        assert_eq!(
            board.receive(board.accepted[&id].clone()),
            Verdict::Duplicate
        );
        assert_eq!(board.snapshot(), before);
    }
    let pend = board.pending.values().next().unwrap().clone();
    let before = board.snapshot();
    assert_eq!(board.receive(pend), Verdict::Pending);
    assert_eq!(board.snapshot(), before);
    // A host that alters a relayed post is caught by the member's own verification.
    let mut relayed = w.d[HOST].board.accepted.values().next().unwrap().clone();
    relayed.body.text = "host rewrote this".into();
    let carol_board = &mut w.d[CAROL].board;
    let before = carol_board.snapshot();
    assert_eq!(
        carol_board.receive(relayed),
        Verdict::Rejected(Why::BadSignature)
    );
    assert_eq!(carol_board.snapshot(), before);
    // Positive control.
    assert_eq!(w.d[HOST].board.receive(a2), Verdict::Accepted);
    assert!(w.d[HOST].board.pending.is_empty());
    assert_eq!(w.d[HOST].board.accepted.len(), 4);
}
