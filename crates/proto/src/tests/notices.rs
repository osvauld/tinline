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
    let mut tampered = genuine.clone();
    let mut claim: BlockNotice = serde_json::from_slice(&dec(&tampered.payload).unwrap()).unwrap();
    claim.at += 1;
    tampered.payload = enc(serde_json::to_vec(&claim).unwrap());
    assert_eq!(
        verify_block_notice(&tampered, b.id.did()),
        Err(Error::BadSignature)
    );
    // Another signed object of A's (a grant for B) is not a notice: own signing domain.
    let grant = issue_grant(&a.id, b.id.did(), T0, GTTL);
    assert!(verify_block_notice(&grant, b.id.did()).is_err());
    // A notice to oneself is meaningless.
    let selfie = sign_block_notice(&a.id, a.id.did(), true, 5);
    assert!(verify_block_notice(&selfie, a.id.did()).is_err());
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
            T0 + 5,
            &none(),
        )
        .unwrap();
        assert_eq!((m.cap.as_str(), m.v), (CAP_MESSAGE, 2));
        let c = verify_grant(
            view.call_grant_from_them.as_ref().unwrap(),
            iss.id.did(),
            holder.id.did(),
            T0 + 5,
            &none(),
        )
        .unwrap();
        assert_eq!(c.cap, CAP_CALL);
        assert_ne!(m.id, c.id, "two separate tokens");
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
        T0 + 1,
        GTTL,
        TokenPolicy::MESSAGE_ONLY,
    )
    .unwrap();
    let (welcome, a_view) =
        accept_contact_hello_with(&a.id, a.dev, &hello, b.dev, T0 + 2, &none(), GTTL, |did| {
            assert_eq!(did, b.id.did());
            TokenPolicy::MESSAGE_ONLY
        })
        .unwrap();
    let b_view = accept_contact_welcome(&b.id, &pending, &welcome, a.dev, T0 + 3).unwrap();
    assert!(a_view.call_grant_from_them.is_none() && b_view.call_grant_from_them.is_none());
    // Bob's message token does not ring Alice, but `accept_hello` still says who he is (so the
    // node can answer "calls not allowed" instead of a generic refusal).
    let hello = call_hello(
        &b.id,
        attest(&b.id, b.dev, T0 + 10),
        b_view.grant_from_them.clone(),
        "c1".into(),
    );
    let known = |d: &str| d == b.id.did();
    assert_eq!(
        accept_call_hello(&a.id, &hello, b.dev, T0 + 10, &none(), known).err(),
        Some(Error::BadCapability)
    );
    let who = accept_hello(&a.id, &hello, b.dev, T0 + 10, &none(), known).unwrap();
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
        T0 + 1,
        GTTL,
        TokenPolicy {
            call: true,
            not_before: cut,
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
        let g = verify_grant(&g, b.id.did(), a.id.did(), T0 + 1, &none()).unwrap();
        assert!(g.iat > cut);
        assert_eq!(g.exp - g.iat, GTTL);
    }
}

#[test]
fn a_call_token_in_the_message_slot_is_refused() {
    let (a, b) = (peer(), peer());
    let t = ticket(&a);
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", T0 + 1, GTTL).unwrap();
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
        grant_for_you: issue_grant(&b.id, a.id.did(), T0, GTTL),
        call_grant_for_you: None,
    };
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &swapped, b.dev, T0 + 2, &none(), GTTL).err(),
        Some(Error::BadCapability)
    );
}

#[test]
fn tokens_message_round_trips() {
    let a = peer();
    let g = issue_token(&a.id, "did:key:x", Cap::Message, T0, GTTL);
    let m = Msg::Tokens {
        grants: vec![g.clone()],
    };
    let (back, _) = decode_frame(&encode_frame(&m)).unwrap().unwrap();
    assert_eq!(back, m);
    assert_eq!(peek_grant(&g).unwrap().cap, CAP_MESSAGE);
}
