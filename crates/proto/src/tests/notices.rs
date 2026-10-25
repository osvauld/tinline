//! Block notices and the separate message / call tokens.

use super::*;

// ---- block notices -------------------------------------------------------------------

#[test]
fn block_notice_signs_and_verifies_for_its_addressee() {
    let (a, b) = (peer(), peer());
    let n = sign_block_notice(&a.id, b.id.did(), true, 1_000);
    let claim = verify_block_notice(&n, b.id.did()).unwrap();
    assert_eq!(
        (
            claim.from.as_str(),
            claim.to.as_str(),
            claim.blocked,
            claim.at
        ),
        (a.id.did(), b.id.did(), true, 1_000)
    );
    let u = sign_block_notice(&a.id, b.id.did(), false, 1_001);
    assert!(!verify_block_notice(&u, b.id.did()).unwrap().blocked);
    // It travels as a call-protocol frame.
    let m = Msg::Notice { notice: n.clone() };
    let (back, _) = decode_frame(&encode_frame(&m)).unwrap().unwrap();
    assert_eq!(back, m);
}

#[test]
fn forged_tampered_or_misaddressed_block_notices_are_refused() {
    let (a, b, x) = (peer(), peer(), peer());
    // Claims to be from A, signed by X.
    let forged = forge_block_notice_for_test(&x.id, a.id.did(), b.id.did(), true, 1_000);
    assert_eq!(
        verify_block_notice(&forged, b.id.did()),
        Err(Error::BadSignature)
    );
    // Control: the same helper signed by A itself is genuine.
    let genuine = forge_block_notice_for_test(&a.id, a.id.did(), b.id.did(), true, 1_000);
    assert!(verify_block_notice(&genuine, b.id.did()).is_ok());
    // Addressed to someone else: B must not take it (nor X take B's).
    assert_eq!(
        verify_block_notice(&genuine, x.id.did()),
        Err(Error::WrongHolder)
    );
    // A payload edit (block -> unblock, or a later `at`) breaks the signature.
    let mut claim = genuine.peek().unwrap();
    claim.at += 1;
    let tampered = Signed::from_parts(
        cryptography::codec::to_bytes(&claim).unwrap(),
        *genuine.sig(),
    );
    assert_eq!(
        verify_block_notice(&tampered, b.id.did()),
        Err(Error::BadSignature)
    );
    // Another signed object of A's (a grant for B) is not a notice: own signing domain.
    let grant = issue_grant(&a.id, b.id.did(), M0, GTTL);
    assert!(verify_block_notice(&retype(&grant), b.id.did()).is_err());
    // A notice to oneself is meaningless.
    // The shape check refuses it at sign time and, in a raw envelope, at open time.
    let selfie = BlockNotice {
        from: a.id.did().into(),
        to: a.id.did().into(),
        blocked: true,
        at: 5,
    };
    assert_eq!(Signed::sign(&selfie, &a.id).err(), Some(Error::SelfContact));
    let body = cryptography::codec::to_bytes(&selfie).unwrap();
    let sig =
        a.id.sign(&cryptography::sign::input(BlockNotice::DOMAIN, &[&body]));
    let raw = Signed::<BlockNotice>::from_parts(body, sig);
    assert!(verify_block_notice(&raw, a.id.did()).is_err());
}

// ---- P6: separate message and call tokens (U13, U15) ---------------------------------------

#[test]
fn ticket_contacts_exchange_message_and_call_tokens() {
    let (a, b) = (peer(), peer());
    let (a_view, b_view) = add_contact(&a, &b);
    for (view, iss, holder) in [(&a_view, &b, &a), (&b_view, &a, &b)] {
        let m = verify_grant(
            &view.grant_from_them,
            iss.id.did(),
            holder.id.did(),
            M0 + 5)
        .unwrap();
        assert_eq!(m.claim, Cap::Message);
        let c = verify_grant(
            view.call_grant_from_them.as_ref().unwrap(),
            iss.id.did(),
            holder.id.did(),
            M0 + 5)
        .unwrap();
        assert_eq!(c.claim, Cap::Call);
        assert_ne!(m.nonce, c.nonce, "two separate tokens");
    }
}

#[test]
fn message_only_policy_gives_no_call_token_either_way() {
    let (a, b) = (peer(), peer());
    let t = ticket(&a);
    let (hello, pending) = contact_hello_with(
        &b.id,
        b.dev,
        &t,
        "bob",
        M0 + 1,
        GTTL,
        TokenPolicy::MESSAGE_ONLY,
    )
    .unwrap();
    let (welcome, a_view) =
        accept_contact_hello_with(&a.id, a.dev, &hello, b.dev, M0 + 2, &Unrecorded, GTTL, |did| {
            assert_eq!(did, b.id.did());
            TokenPolicy::MESSAGE_ONLY
        })
        .unwrap();
    let b_view = accept_contact_welcome(&b.id, &pending, &welcome, a.dev, M0 + 3).unwrap();
    assert!(a_view.call_grant_from_them.is_none() && b_view.call_grant_from_them.is_none());
    // Bob's message token does not ring Alice, but `accept_hello` still says who he is (so the
    // node can answer "calls not allowed" instead of a generic refusal).
    let hello = call_hello(
        attest(&b.id, b.dev, T0 + 10),
        b_view.grant_from_them.clone(),
        "c1".into(),
    );
    let known = |d: &str| d == b.id.did();
    assert_eq!(
        accept_call_hello(&a.id, &hello, b.dev, M0 + 10, known).err(),
        Some(Error::BadCapability)
    );
    let who = accept_hello(&a.id, &hello, b.dev, M0 + 10, known).unwrap();
    assert_eq!(who.did, b.id.did());
    assert!(who.grant.allows(Cap::Message) && !who.grant.allows(Cap::Call));
}

#[test]
fn policy_issues_above_the_cut_off() {
    let (a, b) = (peer(), peer());
    let t = ticket(&a);
    let cut = T0 + 50;
    let (hello, _) = contact_hello_with(
        &b.id,
        b.dev,
        &t,
        "bob",
        M0 + 1,
        GTTL,
        TokenPolicy {
            call: true,
            not_before_ms: cut,
        },
    )
    .unwrap();
    let Msg::ContactHello {
        grant_for_you,
        call_grant_for_you,
        ..
    } = hello
    else {
        panic!()
    };
    for g in [grant_for_you, call_grant_for_you.unwrap()] {
        let g = verify_grant(&g, b.id.did(), a.id.did(), M0 + 1).unwrap();
        assert!(g.iat_ms > cut);
        assert_eq!(g.exp_ms - g.iat_ms, GTTL);
    }
}

#[test]
fn a_call_token_in_the_message_slot_is_refused() {
    let (a, b) = (peer(), peer());
    let t = ticket(&a);
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", M0 + 1, GTTL).unwrap();
    let Msg::ContactHello {
        attestation,
        name,
        invite,
        binding,
        ..
    } = hello
    else {
        panic!()
    };
    let swapped = Msg::ContactHello {
        relay: None,
        attestation,
        name,
        invite,
        binding,
        grant_for_you: issue_grant(&b.id, a.id.did(), M0, GTTL),
        call_grant_for_you: None,
    };
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &swapped, b.dev, M0 + 2, &Unrecorded, GTTL).err(),
        Some(Error::BadCapability)
    );
}

#[test]
fn tokens_message_round_trips() {
    let a = peer();
    let g = issue_token(&a.id, "did:key:x", Cap::Message, M0, GTTL);
    let m = Msg::Tokens {
        grants: vec![g.clone()],
    };
    let (back, _) = decode_frame(&encode_frame(&m)).unwrap().unwrap();
    assert_eq!(back, m);
    assert_eq!(g.peek().unwrap().claim, Cap::Message);
}

fn wake_tokens(token: String) -> Msg {
    Msg::WakeTokens {
        device: [7; 32],
        token,
        exp_ms: M0 + GTTL,
        kinds: 3,
    }
}

#[test]
fn wake_tokens_messages_round_trip() {
    for m in [wake_tokens("sealed.token-_".into()), Msg::WakeTokensTaken] {
        let (back, _) = decode_frame(&encode_frame(&m)).unwrap().unwrap();
        assert_eq!(back, m);
    }
}

#[test]
fn wake_token_bounded() {
    let max = wake_tokens("a".repeat(MAX_WAKE_TOKEN_CHARS));
    assert_eq!(decode_frame(&encode_frame(&max)).unwrap().unwrap().0, max);
    let big = wake_tokens("a".repeat(MAX_WAKE_TOKEN_CHARS + 1));
    assert_eq!(decode_frame(&encode_frame(&big)).err(), Some(Error::Decode));
}
