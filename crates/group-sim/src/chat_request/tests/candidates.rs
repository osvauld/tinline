use super::*;

// KIND: candidate-policy G-BLOCK
#[test]
fn candidate_explicit_decline_reports_refused_and_hides_kind_from_host() {
    let mut w = world(Policy {
        decline: DeclinePolicy::Explicit,
        ..policy()
    });
    let b = w.bob.did();
    let r_alice = send(&mut w.host, &mut w.alice, &b, 1);
    let r_carol = send(&mut w.host, &mut w.carol, &b, 1);
    pump(&mut w.host, &mut w.bob, 2);
    let declined = w.bob.decline(&r_alice).unwrap();
    let accepted = w.bob.accept(&r_carol).unwrap();
    // Host sees the same kind and size for accept and decline.
    assert_eq!(declined.wire().header.kind, accepted.wire().header.kind);
    assert_eq!(
        declined.wire().ciphertext.len(),
        accepted.wire().ciphertext.len()
    );
    w.host.submit(declined, 3).unwrap();
    pump(&mut w.host, &mut w.alice, 4);
    assert_eq!(w.alice.observe(&r_alice, 4), Observation::Refused);
    assert!(w.alice.state.contacts.is_empty());
    assert!(!w.bob.state.contacts.contains_key(&w.alice.did()));
    // Under this candidate a decline (Refused) is distinguishable from a block (NoAnswer
    // under the `BlockReply::Drop` candidate, `Blocked` under `SealedNotice`).
    let (blocked, ..) = requester_view(
        Policy {
            decline: DeclinePolicy::Explicit,
            ..policy()
        },
        TargetCase::Blocked,
    );
    assert_ne!(blocked, Observation::Refused);
}

// KIND: candidate-policy G-REQUEST-RATE
#[test]
fn candidate_host_enforced_rate_limit_per_requester_target_pair() {
    let mut w = world(policy());
    w.host.rate = Some(RateLimit { max: 2, window: 10 });
    let b = w.bob.did();
    send(&mut w.host, &mut w.alice, &b, 1);
    send(&mut w.host, &mut w.alice, &b, 2);
    let (_, third) = w.alice.request(&b, 3).unwrap();
    let before = w.host.snapshot();
    assert_eq!(
        w.host.submit(third.clone(), 3),
        Err(HostRefusal::RateLimited)
    );
    assert_eq!(
        before,
        w.host.snapshot(),
        "over-limit drop must not change host state"
    );
    // Per pair: Carol -> Bob and Alice -> Carol are unaffected.
    send(&mut w.host, &mut w.carol, &b, 3);
    let c = w.carol.did();
    send(&mut w.host, &mut w.alice, &c, 3);
    // Window expiry re-admits Alice -> Bob.
    assert_eq!(w.host.submit(third, 11), Ok(()));
}

// KIND: candidate-policy G-REQUEST-RATE
#[test]
fn candidate_target_enforced_rate_limit_drops_silently() {
    let mut w = world(Policy {
        target_rate: Some(RateLimit { max: 2, window: 10 }),
        ..policy()
    });
    let b = w.bob.did();
    let r1 = send(&mut w.host, &mut w.alice, &b, 1);
    let r2 = send(&mut w.host, &mut w.alice, &b, 1);
    let r3 = send(&mut w.host, &mut w.alice, &b, 1);
    let rc = send(&mut w.host, &mut w.carol, &b, 1);
    let mut inbox = w.host.fetch(&b).into_iter();
    assert_eq!(
        w.bob.receive(inbox.next().unwrap(), 2).0,
        Delivery::NewRequest(r1)
    );
    assert_eq!(
        w.bob.receive(inbox.next().unwrap(), 2).0,
        Delivery::NewRequest(r2)
    );
    let third = inbox.next().unwrap();
    assert_rejected(&mut w.bob, third.clone(), 2, DropReason::RateLimited);
    assert_eq!(
        w.bob.receive(inbox.next().unwrap(), 2).0,
        Delivery::NewRequest(rc)
    );
    // Requester sees the same as any unanswered request.
    assert_eq!(
        w.alice.observe(&r3, 1 + ANSWER_TIMEOUT),
        Observation::NoAnswer
    );
    // Not marked seen, so the same request is accepted once the window passes.
    assert_eq!(w.bob.receive(third, 12).0, Delivery::NewRequest(r3));
}

// KIND: invariant
#[test]
fn retired_request_key_never_decrypts_after_retirement() {
    for retired_key in [RetiredKeyPolicy::Drop, RetiredKeyPolicy::HintCurrentKey] {
        let mut w = world(Policy {
            retired_key,
            grace: 5,
            ..policy()
        });
        let b = w.bob.did();
        let note = w.bob.rotate_request_key(10); // seq 1 retires at 15
        assert!(w.host.publish(note));
        // Alice has not synced: she still seals to seq 1.
        let (r_in_grace, in_grace) = w.alice.request(&b, 12).unwrap();
        let (_, at_boundary) = w.alice.request(&b, 12).unwrap();
        let (_, late) = w.alice.request(&b, 12).unwrap();
        assert_eq!(in_grace.wire().header.key_seq, 1);
        assert_eq!(
            w.bob.receive(in_grace, 14).0,
            Delivery::NewRequest(r_in_grace)
        );
        for (relay, now) in [(at_boundary, 15), (late.clone(), 30)] {
            let before = w.bob.snapshot();
            let (d, _) = w.bob.receive(relay, now);
            assert_eq!(
                d,
                Delivery::Dropped(DropReason::StaleKey),
                "{retired_key:?} t={now}"
            );
            assert_eq!(before, w.bob.snapshot());
        }
        // tick erases the secret; the public half is kept for audit only.
        w.bob.tick(15);
        assert!(w.bob.state.keys[&1].secret.is_none());
        assert!(w.bob.state.keys[&2].secret.is_some());
        assert!(w.bob.usable_secret(1, 15).is_none());
        assert!(w.bob.insider_open(late.wire(), 30).is_none());
    }
}

// KIND: candidate-policy G-RETIREMENT
#[test]
fn candidate_stale_key_request_is_dropped_silently() {
    let mut w = world(policy());
    let b = w.bob.did();
    assert!(w.host.publish(w.bob.rotate_request_key(10)));
    w.bob.tick(15);
    let rid = send(&mut w.host, &mut w.alice, &b, 20);
    let before = w.bob.snapshot();
    assert_eq!(
        pump(&mut w.host, &mut w.bob, 21),
        vec![Delivery::Dropped(DropReason::StaleKey)]
    );
    assert_eq!(before, w.bob.snapshot());
    assert!(
        w.host.state.mailboxes.values().all(|m| m.is_empty()),
        "no reply"
    );
    assert_eq!(
        w.alice.observe(&rid, 20 + ANSWER_TIMEOUT),
        Observation::NoAnswer
    );
    // Recovery is only by Alice re-syncing the directory and asking again.
    w.alice.sync_keys(&w.host);
    let rid2 = send(&mut w.host, &mut w.alice, &b, 31);
    assert_eq!(
        pump(&mut w.host, &mut w.bob, 32),
        vec![Delivery::NewRequest(rid2)]
    );
}

// KIND: candidate-policy G-RETIREMENT
#[test]
fn candidate_stale_key_request_is_answered_with_current_key_hint() {
    let mut w = world(Policy {
        retired_key: RetiredKeyPolicy::HintCurrentKey,
        ..policy()
    });
    let (a, b, c) = (w.alice.did(), w.bob.did(), w.carol.did());
    let old_note = w.bob.key_note();
    assert!(w.host.publish(w.bob.rotate_request_key(10)));
    w.bob.tick(15);
    // Carol is blocked: she gets no hint (same as Drop / offline).
    w.bob.block(&c);
    let rc = send(&mut w.host, &mut w.carol, &b, 20);
    let ra = send(&mut w.host, &mut w.alice, &b, 20);
    let before = w.bob.snapshot();
    let deliveries = pump(&mut w.host, &mut w.bob, 21);
    assert_eq!(deliveries, vec![Delivery::Dropped(DropReason::StaleKey); 2]);
    assert_eq!(before, w.bob.snapshot());
    assert!(w.host.fetch(&c).is_empty());
    assert_eq!(
        w.carol.observe(&rc, 20 + ANSWER_TIMEOUT),
        Observation::NoAnswer
    );
    // A replayed OLD hint does not roll Alice back.
    let alice_before = w.alice.snapshot();
    let replayed = Relay::KeyHint {
        from: b.clone(),
        to: a.clone(),
        note: old_note,
    };
    assert_eq!(
        w.alice.receive(replayed, 21).0,
        Delivery::Dropped(DropReason::UnknownRequest)
    );
    assert_eq!(alice_before, w.alice.snapshot());
    // Alice gets the genuine hint, learns seq 2 and retries.
    assert_eq!(
        pump(&mut w.host, &mut w.alice, 22),
        vec![Delivery::KeyHint(b.clone())]
    );
    assert_eq!(w.alice.observe(&ra, 22), Observation::KeyStale);
    assert_eq!(w.alice.state.known_keys[&b].claim.seq, 2);
    let ra2 = send(&mut w.host, &mut w.alice, &b, 23);
    assert_eq!(
        pump(&mut w.host, &mut w.bob, 24),
        vec![Delivery::NewRequest(ra2)]
    );
}

// KIND: invariant
#[test]
fn request_key_notes_need_did_signature_and_increasing_seq() {
    let mut w = world(policy());
    let b = w.bob.did();
    let seq1 = w.bob.key_note();
    let seq2 = w.bob.rotate_request_key(10);
    let mut bad = vec![];
    // Signed by Carol over Bob's claim (host/insider key substitution).
    let mut forged = seq2.clone();
    forged.claim.x25519 = w.carol.state.keys[&1].public;
    forged.sig = did_sign(&w.carol.id, "request-key", &forged.claim);
    bad.push(forged);
    // Bob's signature but substituted key bytes.
    let mut swapped = seq2.clone();
    swapped.claim.x25519 = [9; 32];
    bad.push(swapped);
    // Wrong group.
    let mut other = seq2.clone();
    other.claim.group = "other-group".into();
    other.sig = did_sign(&w.bob.id, "request-key", &other.claim);
    bad.push(other);
    let host_before = w.host.snapshot();
    let alice_before = w.alice.snapshot();
    for note in bad {
        assert!(!w.alice.learn_key(note.clone()));
        assert!(!w.host.publish(note));
    }
    assert_eq!(alice_before, w.alice.snapshot());
    assert_eq!(host_before, w.host.snapshot());
    // Positive control, then rollback to seq 1 refused by both.
    assert!(w.alice.learn_key(seq2.clone()) && w.host.publish(seq2));
    let (alice_before, host_before) = (w.alice.snapshot(), w.host.snapshot());
    assert!(!w.alice.learn_key(seq1.clone()));
    assert!(!w.host.publish(seq1));
    assert_eq!(alice_before, w.alice.snapshot());
    assert_eq!(host_before, w.host.snapshot());
    assert_eq!(w.alice.state.known_keys[&b].claim.seq, 2);
}

// KIND: limitation
#[test]
fn host_learns_who_requested_whom_and_answer_timing() {
    // E2E hides content and endpoints, NOT the social graph. The host sees sender,
    // target and time of every request, and that an answer flowed back. Under the silent
    // decline candidate any answer at all means "accepted".
    let mut w = world(policy());
    let (a, b) = (w.alice.did(), w.bob.did());
    let rid = send(&mut w.host, &mut w.alice, &b, 1);
    pump(&mut w.host, &mut w.bob, 2);
    w.host.submit(w.bob.accept(&rid).unwrap(), 3).unwrap();
    let graph: Vec<(String, String, WireKind)> = w
        .host
        .state
        .relayed
        .iter()
        .map(|r| {
            let h = &r.wire().header;
            (h.from.clone(), h.to.clone(), h.kind)
        })
        .collect();
    assert_eq!(
        graph,
        vec![
            (a.clone(), b.clone(), WireKind::Request),
            (b, a.clone(), WireKind::Answer)
        ]
    );
    assert_eq!(
        w.host
            .state
            .rate
            .values()
            .flatten()
            .copied()
            .collect::<Vec<_>>(),
        vec![1]
    );
}

// KIND: limitation
#[test]
fn target_learns_requester_private_endpoint_before_deciding() {
    // In this 2-message construction the request carries the requester's private
    // endpoint, so a target that later declines (or leaves it pending) has already seen
    // it. A 3-message variant (endpoint only after accept) is an open question.
    let mut w = world(policy());
    let b = w.bob.did();
    let rid = send(&mut w.host, &mut w.alice, &b, 1);
    pump(&mut w.host, &mut w.bob, 2);
    assert_eq!(
        w.bob.state.incoming[&rid].endpoint,
        w.alice.state.private_endpoint
    );
    assert!(w.bob.decline(&rid).is_none());
    assert!(!w.bob.state.contacts.contains_key(&w.alice.did()));
}
