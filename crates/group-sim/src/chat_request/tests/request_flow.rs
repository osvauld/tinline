use super::*;

// ---------------------------------------------------------------- tests

// KIND: invariant
#[test]
fn request_is_e2e_and_host_sees_only_routing_metadata() {
    let mut w = world(policy());
    let (a, b) = (w.alice.did(), w.bob.did());
    let rid = send(&mut w.host, &mut w.alice, &b, 1);
    assert_eq!(
        pump(&mut w.host, &mut w.bob, 2),
        vec![Delivery::NewRequest(rid.clone())]
    );
    w.host.submit(w.bob.accept(&rid).unwrap(), 3).unwrap();
    assert_eq!(
        pump(&mut w.host, &mut w.alice, 4),
        vec![Delivery::Answered(rid.clone())]
    );
    // Leave one relay sitting in a mailbox too, so mailbox scanning is exercised.
    send(&mut w.host, &mut w.carol, &b, 5);

    for ep in [
        &w.alice.state.private_endpoint,
        &w.bob.state.private_endpoint,
        &w.carol.state.private_endpoint,
    ] {
        assert!(!w.host.leaks(ep), "private endpoint visible to host: {ep}");
    }
    assert!(
        !w.host.leaks(&rid),
        "request id is inside the ciphertext only"
    );
    // Positive controls: the scanner sees group endpoints (U2: host knows them) and would
    // see a private endpoint in an unencrypted relay of the same shape.
    assert!(w.host.leaks(&w.alice.state.group_endpoint));
    let mut leaky = Host::default();
    let mut plain = w.host.state.relayed[0].wire().clone();
    plain.ciphertext = SignedInner::new(
        &w.alice.id,
        Inner::Request {
            group: GROUP.into(),
            from: a.clone(),
            to: b.clone(),
            request_id: rid.clone(),
            key_seq: 1,
            endpoint: w.alice.state.private_endpoint.clone(),
            sent_at: 1,
        },
    )
    .padded();
    leaky.state.relayed.push(Relay::Sealed(plain));
    assert!(leaky.leaks(&w.alice.state.private_endpoint));

    // What the host can read is exactly the routing header; ciphertexts are fixed size.
    assert_eq!(w.host.state.relayed.len(), 3);
    for r in &w.host.state.relayed {
        let v = serde_json::to_value(r.wire()).unwrap();
        let fields: BTreeSet<_> = v["header"].as_object().unwrap().keys().cloned().collect();
        let expect: BTreeSet<String> = ["group", "kind", "from", "to", "key_seq", "ephemeral"]
            .map(String::from)
            .into();
        assert_eq!(fields, expect);
        assert_eq!(r.wire().ciphertext.len(), PAD + 16);
    }
    let h = &w.host.state.relayed[0].wire().header;
    assert_eq!(
        (h.from.as_str(), h.to.as_str(), h.kind),
        (a.as_str(), b.as_str(), WireKind::Request)
    );
}

// KIND: invariant
#[test]
fn accept_binds_private_endpoints_to_dids_then_direct_chat_skips_host() {
    let mut w = world(policy());
    let (a, b, c) = (w.alice.did(), w.bob.did(), w.carol.did());
    let rid = send(&mut w.host, &mut w.alice, &b, 1);
    pump(&mut w.host, &mut w.bob, 2);
    w.host.submit(w.bob.accept(&rid).unwrap(), 3).unwrap();
    pump(&mut w.host, &mut w.alice, 4);

    assert_eq!(
        w.alice.observe(&rid, 4),
        Observation::Accepted(w.bob.state.private_endpoint.clone())
    );
    assert_eq!(
        w.alice.state.contacts.get(&b),
        Some(&w.bob.state.private_endpoint)
    );
    assert_eq!(
        w.bob.state.contacts.get(&a),
        Some(&w.alice.state.private_endpoint)
    );
    assert!(w.carol.state.contacts.is_empty());
    assert!(w.bob.state.incoming.is_empty());

    // Direct chat afterwards: host state byte-identical throughout.
    let host_before = w.host.snapshot();
    let hi = w.alice.direct_send(&b, "hi bob").unwrap();
    assert!(route_direct(&hi, &mut [&mut w.bob, &mut w.carol]));
    let back = w.bob.direct_send(&a, "hi alice").unwrap();
    assert!(route_direct(&back, &mut [&mut w.alice, &mut w.carol]));
    assert_eq!(
        w.bob.state.direct_inbox,
        vec![(a.clone(), "hi bob".to_string())]
    );
    assert_eq!(
        w.alice.state.direct_inbox,
        vec![(b.clone(), "hi alice".to_string())]
    );
    assert_eq!(host_before, w.host.snapshot());

    // Endpoint is bound to the DID: Carol has no route, and a message to Bob's endpoint
    // claiming Alice's DID but signed by Carol is refused with Bob unchanged.
    assert!(w.carol.direct_send(&b, "x").is_none());
    let mut forged = hi.clone();
    forged.body.text = "forged".into();
    forged.sig = did_sign(&w.carol.id, "direct", &forged.body);
    let bob_before = w.bob.snapshot();
    assert!(!route_direct(&forged, &mut [&mut w.bob]));
    assert_eq!(bob_before, w.bob.snapshot());
    let mut from_carol = forged.clone();
    from_carol.body.from = c;
    from_carol.sig = did_sign(&w.carol.id, "direct", &from_carol.body);
    assert!(
        !route_direct(&from_carol, &mut [&mut w.bob]),
        "not a contact"
    );
    assert_eq!(bob_before, w.bob.snapshot());
}

// KIND: invariant
#[test]
fn host_tampering_is_detected_and_target_state_unchanged() {
    let mut w = world(policy());
    let (b, c) = (w.bob.did(), w.carol.did());
    // Populate Bob: one pending request from Carol already held.
    let carol_rid = send(&mut w.host, &mut w.carol, &b, 1);
    assert_eq!(
        pump(&mut w.host, &mut w.bob, 1),
        vec![Delivery::NewRequest(carol_rid)]
    );
    let (rid_b, to_bob) = w.alice.request(&b, 2).unwrap();
    let (_, to_carol) = w.alice.request(&c, 2).unwrap();
    let (wb, wc) = (to_bob.wire().clone(), to_carol.wire().clone());

    // Bit flips: ciphertext body, tag, and the header's ephemeral key.
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(flip(wb.clone(), 7)),
        3,
        DropReason::Undecryptable,
    );
    let last = wb.ciphertext.len() - 1;
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(flip(wb.clone(), last)),
        3,
        DropReason::Undecryptable,
    );
    let mut eph = wb.clone();
    eph.header.ephemeral[0] ^= 1;
    assert_rejected(&mut w.bob, Relay::Sealed(eph), 3, DropReason::Undecryptable);
    // Swapped ciphertext between targets (Bob's header, Carol's ciphertext).
    let mut swapped = wb.clone();
    swapped.ciphertext = wc.ciphertext.clone();
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(swapped),
        3,
        DropReason::Undecryptable,
    );
    // Re-addressed: Carol's request with its header rewritten to Bob.
    let mut readdressed = wc.clone();
    readdressed.header.to = b.clone();
    readdressed.header.key_seq = w.bob.state.current_seq;
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(readdressed),
        3,
        DropReason::Undecryptable,
    );
    // Spoofed sender in routing metadata (AAD-bound).
    let mut spoofed = wb.clone();
    spoofed.header.from = c.clone();
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(spoofed),
        3,
        DropReason::Undecryptable,
    );
    // Whole wire mis-delivered to Carol.
    assert_rejected(
        &mut w.carol,
        Relay::Sealed(wb.clone()),
        3,
        DropReason::Misaddressed,
    );
    // Header kind flipped Request -> Answer.
    let mut kind = wb.clone();
    kind.header.kind = WireKind::Answer;
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(kind),
        3,
        DropReason::Undecryptable,
    );

    // Positive control on the same populated replica.
    assert_eq!(w.bob.receive(to_bob, 3).0, Delivery::NewRequest(rid_b));
}

// KIND: invariant
#[test]
fn insider_reencrypted_request_is_rejected_by_signed_inner_binding() {
    let mut w = world(policy());
    let (a, b, c) = (w.alice.did(), w.bob.did(), w.carol.did());
    let (_, to_carol) = w.alice.request(&c, 1).unwrap();
    // Carol is a legitimate recipient, so she can decrypt Alice's signed request...
    let stolen = w.carol.insider_open(to_carol.wire(), 1).unwrap();
    assert!(stolen.verify());
    // ...and reseal it, unchanged, to Bob with a consistent header claiming Alice.
    let bob_key = w.carol.state.known_keys[&b].clone();
    let header = Header {
        group: GROUP.into(),
        kind: WireKind::Request,
        from: a.clone(),
        to: b.clone(),
        key_seq: bob_key.claim.seq,
        ephemeral: [0; 32],
    };
    let eph = w.carol.next_random("ephemeral");
    let forwarded = seal(header.clone(), &bob_key.claim.x25519, eph, &stolen);
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(forwarded),
        2,
        DropReason::Misaddressed,
    );
    // Editing the signed `to` breaks Alice's signature.
    let mut edited = stolen.clone();
    if let Inner::Request { to, key_seq, .. } = &mut edited.inner {
        *to = b.clone();
        *key_seq = bob_key.claim.seq;
    }
    let eph = w.carol.next_random("ephemeral");
    let resigned = seal(header, &bob_key.claim.x25519, eph, &edited);
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(resigned),
        2,
        DropReason::BadSignature,
    );
    assert!(w.bob.state.incoming.is_empty() && w.bob.state.contacts.is_empty());
}

// KIND: invariant
#[test]
fn replayed_request_and_acceptance_are_rejected() {
    let mut w = world(policy());
    let b = w.bob.did();
    let (rid, req) = w.alice.request(&b, 1).unwrap();
    assert_eq!(
        w.bob.receive(req.clone(), 2).0,
        Delivery::NewRequest(rid.clone())
    );
    assert_rejected(&mut w.bob, req.clone(), 3, DropReason::Replay);
    let acc = w.bob.accept(&rid).unwrap();
    // Replay after the request was answered (incoming entry gone) is still a replay.
    assert_rejected(&mut w.bob, req, 4, DropReason::Replay);
    assert_eq!(
        w.alice.receive(acc.clone(), 5).0,
        Delivery::Answered(rid.clone())
    );
    assert_rejected(&mut w.alice, acc, 6, DropReason::Replay);
    assert_eq!(
        w.alice.observe(&rid, 6),
        Observation::Accepted(w.bob.state.private_endpoint.clone())
    );
}

// KIND: invariant
#[test]
fn acceptance_cannot_be_redirected_to_a_third_party() {
    let mut w = world(policy());
    let (a, b, c) = (w.alice.did(), w.bob.did(), w.carol.did());
    let alice_rid = send(&mut w.host, &mut w.alice, &b, 1);
    // Carol has her own outstanding request to Bob, so she has something to confuse.
    let carol_rid = send(&mut w.host, &mut w.carol, &b, 1);
    pump(&mut w.host, &mut w.bob, 2);
    let acc = w.bob.accept(&alice_rid).unwrap();
    let wa = acc.wire().clone();

    // Host delivers the whole acceptance to Carol.
    assert_rejected(&mut w.carol, acc.clone(), 3, DropReason::Misaddressed);
    // Host re-addresses the header to Carol.
    let mut readdressed = wa.clone();
    readdressed.header.to = c.clone();
    readdressed.header.key_seq = w.carol.state.current_seq;
    assert_rejected(
        &mut w.carol,
        Relay::Sealed(readdressed),
        3,
        DropReason::Undecryptable,
    );
    // Insider Alice decrypts Bob's acceptance and reseals it to Carol, even rewriting the
    // request ID to Carol's: Bob's signature covers `to` and the request digest.
    let inner = w.alice.insider_open(&wa, 3).unwrap();
    let ckey = w.alice.state.known_keys[&c].clone();
    let mut header = wa.header.clone();
    header.to = c.clone();
    header.key_seq = ckey.claim.seq;
    let eph = w.alice.next_random("ephemeral");
    let resealed = seal(header.clone(), &ckey.claim.x25519, eph, &inner);
    assert_rejected(
        &mut w.carol,
        Relay::Sealed(resealed),
        3,
        DropReason::Misaddressed,
    );
    let mut rewritten = inner.clone();
    if let Inner::Accept { to, request_id, .. } = &mut rewritten.inner {
        *to = c.clone();
        *request_id = carol_rid.clone();
    }
    let eph = w.alice.next_random("ephemeral");
    let resealed = seal(header, &ckey.claim.x25519, eph, &rewritten);
    assert_rejected(
        &mut w.carol,
        Relay::Sealed(resealed),
        3,
        DropReason::BadSignature,
    );
    // Carol forges an acceptance "from Bob" to Alice carrying her own endpoint.
    let digest = w.alice.state.outgoing[&alice_rid].digest.clone();
    let fake = SignedInner::new(
        &w.carol.id,
        Inner::Accept {
            group: GROUP.into(),
            from: b.clone(),
            to: a.clone(),
            request_id: alice_rid.clone(),
            request_digest: digest,
            endpoint: w.carol.state.private_endpoint.clone(),
        },
    );
    let akey = w.carol.state.known_keys[&a].clone();
    let eph = w.carol.next_random("ephemeral");
    let fake = seal(
        Header {
            group: GROUP.into(),
            kind: WireKind::Answer,
            from: b.clone(),
            to: a.clone(),
            key_seq: akey.claim.seq,
            ephemeral: [0; 32],
        },
        &akey.claim.x25519,
        eph,
        &fake,
    );
    assert_rejected(
        &mut w.alice,
        Relay::Sealed(fake),
        3,
        DropReason::BadSignature,
    );

    // Positive controls: genuine acceptance reaches Alice; Carol's request still pending.
    assert_eq!(w.alice.receive(acc, 4).0, Delivery::Answered(alice_rid));
    assert_eq!(
        w.alice.state.contacts.get(&b),
        Some(&w.bob.state.private_endpoint)
    );
    assert_eq!(w.carol.observe(&carol_rid, 4), Observation::Pending);
    assert!(w.carol.state.contacts.is_empty());
}

// KIND: candidate-policy G-BLOCK
// Kept under U5 revised: the property is that a host which drops a request cannot forge the
// target's refusal (decline is the open explicit-vs-silent candidate); it asserts nothing
// about whether a block is silent.
#[test]
fn host_drop_yields_no_answer_never_refused() {
    let explicit = Policy {
        decline: DeclinePolicy::Explicit,
        ..policy()
    };
    let mut w = world(explicit);
    let (a, b) = (w.alice.did(), w.bob.did());
    let rid = send(&mut w.host, &mut w.alice, &b, 1);
    // Malicious host discards the request.
    assert_eq!(w.host.fetch(&b).len(), 1);
    assert_eq!(
        w.alice.observe(&rid, 1 + ANSWER_TIMEOUT - 1),
        Observation::Pending
    );
    assert_eq!(
        w.alice.observe(&rid, 1 + ANSWER_TIMEOUT),
        Observation::NoAnswer
    );
    // Even knowing the request ID and digest, the host cannot forge Bob's refusal.
    let host_id = Author::new(40, GROUP).id;
    let forged = SignedInner::new(
        &host_id,
        Inner::Decline {
            group: GROUP.into(),
            from: b.clone(),
            to: a.clone(),
            request_id: rid.clone(),
            request_digest: w.alice.state.outgoing[&rid].digest.clone(),
        },
    );
    let akey = w.host.state.directory[&a].clone();
    let forged = seal(
        Header {
            group: GROUP.into(),
            kind: WireKind::Answer,
            from: b.clone(),
            to: a.clone(),
            key_seq: akey.claim.seq,
            ephemeral: [0; 32],
        },
        &akey.claim.x25519,
        sim_random(40, "ephemeral", 1),
        &forged,
    );
    assert_rejected(
        &mut w.alice,
        Relay::Sealed(forged),
        12,
        DropReason::BadSignature,
    );
    assert_eq!(w.alice.observe(&rid, 12), Observation::NoAnswer);

    // Positive control: a genuine explicit decline is the only way to see Refused.
    let rid2 = send(&mut w.host, &mut w.alice, &b, 20);
    pump(&mut w.host, &mut w.bob, 21);
    w.host.submit(w.bob.decline(&rid2).unwrap(), 21).unwrap();
    pump(&mut w.host, &mut w.alice, 22);
    assert_eq!(w.alice.observe(&rid2, 22), Observation::Refused);
}

#[derive(Clone, Copy, Debug)]
pub(super) enum TargetCase {
    Offline,
    OnlineUnanswered,
    // Only with `BlockReply::Drop`: asserts that no reply is emitted.
    Blocked,
    // Only with `DeclinePolicy::Silent`.
    SilentDecline,
}
// Runs one fresh, identically seeded world; returns (observation, requester snapshot,
// host submit result) as seen by Alice.
pub(super) fn requester_view(p: Policy, case: TargetCase) -> (Observation, Vec<u8>, bool) {
    let mut w = world(p);
    let (a, b) = (w.alice.did(), w.bob.did());
    if let TargetCase::Blocked = case {
        assert_eq!(p.block, BlockReply::Drop);
        w.bob.block(&a);
    }
    let (rid, relay) = w.alice.request(&b, 1).unwrap();
    let submitted = w.host.submit(relay, 1).is_ok();
    match case {
        TargetCase::Offline => {}
        TargetCase::OnlineUnanswered => {
            assert_eq!(
                pump(&mut w.host, &mut w.bob, 2),
                vec![Delivery::NewRequest(rid.clone())]
            );
        }
        TargetCase::Blocked => {
            let before = w.bob.snapshot();
            let deliveries = pump(&mut w.host, &mut w.bob, 2);
            assert_eq!(deliveries, vec![Delivery::Dropped(DropReason::Blocked)]);
            assert_eq!(
                before,
                w.bob.snapshot(),
                "blocked drop must not change target state"
            );
            assert!(
                w.host.state.mailboxes.values().all(|m| m.is_empty()),
                "no reply emitted"
            );
        }
        TargetCase::SilentDecline => {
            assert_eq!(p.decline, DeclinePolicy::Silent);
            pump(&mut w.host, &mut w.bob, 2);
            assert!(w.bob.decline(&rid).is_none());
        }
    }
    // Alice polls the host; nothing is waiting for her in any case.
    assert!(pump(&mut w.host, &mut w.alice, 1 + ANSWER_TIMEOUT).is_empty());
    (
        w.alice.observe(&rid, 1 + ANSWER_TIMEOUT),
        w.alice.snapshot(),
        submitted,
    )
}

// KIND: candidate-policy G-BLOCK
// Section 8b Q4 (open): a chat request from a DID the target blocked. Replaces the W2-Q tests
// that asserted the rejected silent option (U5 revised). Two candidates:
// - `SealedNotice`: Bob answers each request through the host with a sealed, DID-signed
//   "blocked" answer bound to that request. Alice observes `Blocked` on every attempt (same
//   reason), which she can tell apart from an offline target, an unanswered request and an
//   explicit decline. Bob's state is unchanged by each refused request. The host sees an
//   Answer of the same kind and ciphertext size as an accept, so it cannot tell a block
//   from an accept. A host-forged "blocked" answer fails the DID signature and a replayed
//   one is refused, Alice's state unchanged in both.
// - `Drop`: no reply; at this layer Alice sees `NoAnswer`, the same as an offline target.
//   The block is then visible only through the private-path notice (proto
//   `rotation_visible_block.rs`, `multi_host.rs`), if Alice is a private contact at all.
#[test]
fn candidate_blocked_requester_gets_sealed_signed_answer_vs_drop() {
    let p = Policy {
        decline: DeclinePolicy::Explicit,
        block: BlockReply::SealedNotice,
        ..policy()
    };
    let mut w = world(p);
    let (a, b, c) = (w.alice.did(), w.bob.did(), w.carol.did());
    w.bob.block(&a);
    // Reference size: a genuine accept from Bob to Carol.
    let rc = send(&mut w.host, &mut w.carol, &b, 1);
    pump(&mut w.host, &mut w.bob, 1);
    let accept = w.bob.accept(&rc).unwrap();
    let mut first_reply = None;
    for t in [2, 20] {
        let rid = send(&mut w.host, &mut w.alice, &b, t);
        let before = w.bob.snapshot();
        assert_eq!(
            pump(&mut w.host, &mut w.bob, t + 1),
            vec![Delivery::Dropped(DropReason::Blocked)]
        );
        assert_eq!(w.bob.snapshot(), before, "refusal changed target state");
        let reply = w.host.state.mailboxes[&a].last().unwrap().clone();
        assert_eq!(reply.route(), (b.as_str(), a.as_str()));
        assert_eq!(reply.wire().header.kind, accept.wire().header.kind);
        assert_eq!(
            reply.wire().ciphertext.len(),
            accept.wire().ciphertext.len()
        );
        assert_eq!(
            pump(&mut w.host, &mut w.alice, t + 2),
            vec![Delivery::Answered(rid.clone())]
        );
        assert_eq!(w.alice.observe(&rid, t + 2), Observation::Blocked);
        assert!(!w.alice.state.contacts.contains_key(&b));
        first_reply.get_or_insert(reply);
    }
    // Replay of the first sealed answer: already answered, refused unchanged.
    assert_rejected(&mut w.alice, first_reply.unwrap(), 30, DropReason::Replay);
    // A host-forged "blocked" answer for a fresh request fails Bob's DID signature.
    let rid = send(&mut w.host, &mut w.alice, &b, 40);
    let host_id = Author::new(40, GROUP).id;
    let forged = SignedInner::new(
        &host_id,
        Inner::Blocked {
            group: GROUP.into(),
            from: b.clone(),
            to: a.clone(),
            request_id: rid.clone(),
            request_digest: w.alice.state.outgoing[&rid].digest.clone(),
        },
    );
    let akey = w.host.state.directory[&a].clone();
    let forged = seal(
        Header {
            group: GROUP.into(),
            kind: WireKind::Answer,
            from: b.clone(),
            to: a.clone(),
            key_seq: akey.claim.seq,
            ephemeral: [0; 32],
        },
        &akey.claim.x25519,
        sim_random(40, "ephemeral", 2),
        &forged,
    );
    assert_rejected(
        &mut w.alice,
        Relay::Sealed(forged),
        41,
        DropReason::BadSignature,
    );
    assert_eq!(w.alice.observe(&rid, 41), Observation::Pending);
    // Carol (not blocked) is unaffected.
    w.host.submit(accept, 3).unwrap();
    pump(&mut w.host, &mut w.carol, 3);
    assert!(matches!(w.carol.observe(&rc, 3), Observation::Accepted(_)));
    assert!(w.bob.state.contacts.contains_key(&c));

    // Distinguishable under SealedNotice; under Drop, blocked == offline at this layer.
    for case in [TargetCase::Offline, TargetCase::OnlineUnanswered] {
        assert_eq!(requester_view(p, case).0, Observation::NoAnswer);
    }
    let drop = Policy {
        block: BlockReply::Drop,
        ..p
    };
    let blocked = requester_view(drop, TargetCase::Blocked);
    assert_eq!(blocked.0, Observation::NoAnswer);
    assert_eq!(blocked, requester_view(drop, TargetCase::Offline));
    // The silent-decline candidate (open, separate from blocking) also reads as NoAnswer, so
    // under SealedNotice it stays distinguishable from a block.
    let silent = Policy {
        decline: DeclinePolicy::Silent,
        ..p
    };
    assert_eq!(
        requester_view(silent, TargetCase::SilentDecline).0,
        Observation::NoAnswer
    );
}
