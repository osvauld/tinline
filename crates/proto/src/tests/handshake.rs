//! Handshakes: add-contact, call hello, own-device hello, attestation and binding checks.

use super::*;

#[test]
fn device_key_derivation() {
    let s = new_device_secret();
    let p = device_public(&s);
    assert_eq!(device_from_text(&device_to_text(&p)).unwrap(), p);
    assert_ne!(new_device_secret(), s);
}

#[test]
fn add_contact_happy_path() {
    let (a, b) = (peer(), peer());
    let (a_view, b_view) = add_contact(&a, &b);
    assert_eq!(a_view.did, b.id.did());
    assert_eq!(a_view.name, "bob");
    assert_eq!(a_view.device, b.dev);
    assert_eq!(b_view.did, a.id.did());
    assert_eq!(b_view.name, "alice");
    // Each holds a grant issued by the other.
    verify_grant(
        &a_view.grant_from_them,
        b.id.did(),
        a.id.did(),
        T0 + 5,
        &none(),
    )
    .unwrap();
    verify_grant(
        &b_view.grant_from_them,
        a.id.did(),
        b.id.did(),
        T0 + 5,
        &none(),
    )
    .unwrap();
    let _ = (&a.secret, &b.secret);
}

#[test]
fn call_happy_path_and_multi_device() {
    let (a, b) = (peer(), peer());
    let (a_view, b_view) = add_contact(&a, &b);
    // Bob calls Alice with the call token Alice gave him (P6: the message token does not ring).
    let call_token = b_view
        .call_grant_from_them
        .clone()
        .expect("ticket contacts get a call token");
    let msg = call_hello(
        &b.id,
        attest(&b.id, b.dev, T0 + 10),
        call_token.clone(),
        "c1".into(),
    );
    let known = |d: &str| d == a_view.did;
    let c = accept_call_hello(&a.id, &msg, b.dev, T0 + 10, &none(), known).unwrap();
    assert_eq!((c.did.as_str(), c.device), (b.id.did(), b.dev));

    // Bob's second phone, same DID, own attestation: allowed.
    let dev2 = device_public(&new_device_secret());
    let msg2 = call_hello(&b.id, attest(&b.id, dev2, T0 + 11), call_token, "c2".into());
    let c2 = accept_call_hello(&a.id, &msg2, dev2, T0 + 11, &none(), known).unwrap();
    assert_eq!(c2.device, dev2);
}

#[test]
fn attestation_device_not_remote_device() {
    let (a, b, m) = (peer(), peer(), peer());
    let t = ticket(&a);
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", T0, GTTL).unwrap();
    // A relay/MITM forwards Bob's hello over its own connection.
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &hello, m.dev, T0, &none(), GTTL).err(),
        Some(Error::DeviceMismatch)
    );
}

#[test]
fn welcome_device_mismatches() {
    let (a, b, m) = (peer(), peer(), peer());
    let t = ticket(&a);
    let (hello, pending) = contact_hello(&b.id, b.dev, &t, "bob", T0, GTTL).unwrap();
    let (welcome, _) =
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, T0, &none(), GTTL).unwrap();
    assert_eq!(
        accept_contact_welcome(&b.id, &pending, &welcome, m.dev, T0).err(),
        Some(Error::DeviceMismatch)
    );
    // Mallory answers with her own attestation + grant instead of Alice's.
    let (_, m_welcome, ..) = {
        let t2 = ticket(&m);
        let (h2, _) = contact_hello(&b.id, b.dev, &t2, "bob", T0, GTTL).unwrap();
        let (w, nc) = accept_contact_hello(&m.id, m.dev, &h2, b.dev, T0, &none(), GTTL).unwrap();
        (0, w, nc)
    };
    assert_eq!(
        accept_contact_welcome(&b.id, &pending, &m_welcome, m.dev, T0).err(),
        Some(Error::WrongIssuer)
    );
}

#[test]
fn welcome_grant_wrong_holder_or_issuer() {
    let (a, b, m) = (peer(), peer(), peer());
    let t = ticket(&a);
    let (hello, pending) = contact_hello(&b.id, b.dev, &t, "bob", T0, GTTL).unwrap();
    let (welcome, _) =
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, T0, &none(), GTTL).unwrap();
    let Msg::ContactWelcome {
        attestation, name, ..
    } = welcome
    else {
        panic!()
    };
    let bad = Msg::ContactWelcome {
        attestation: attestation.clone(),
        name: name.clone(),
        grant_for_you: issue_grant(&a.id, m.id.did(), T0, GTTL),
        call_grant_for_you: None,
    };
    assert_eq!(
        accept_contact_welcome(&b.id, &pending, &bad, a.dev, T0).err(),
        Some(Error::WrongHolder)
    );
    let bad = Msg::ContactWelcome {
        attestation,
        name,
        grant_for_you: issue_grant(&m.id, b.id.did(), T0, GTTL),
        call_grant_for_you: None,
    };
    assert_eq!(
        accept_contact_welcome(&b.id, &pending, &bad, a.dev, T0).err(),
        Some(Error::WrongIssuer)
    );
}

#[test]
fn binding_for_different_device_or_invite() {
    let (a, b) = (peer(), peer());
    let t = ticket(&a);
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", T0, GTTL).unwrap();
    let (att, name, invite, _, grant) = hello_parts(&hello);

    // Binding made for some other joiner device.
    let claim = open_invite(&invite).unwrap();
    let other = device_public(&new_device_secret());
    let sig =
        b.id.sign(&binding_message(&claim.nonce, &other, &a.dev).unwrap());
    let bad = Msg::ContactHello {
        relay: None,
        attestation: att.clone(),
        name: name.clone(),
        invite: invite.clone(),
        binding: enc(sig),
        grant_for_you: grant.clone(),
        call_grant_for_you: None,
    };
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &bad, b.dev, T0, &none(), GTTL).err(),
        Some(Error::BadSignature)
    );

    // Binding for a different invite's nonce.
    let t2 = ticket(&a);
    let (h2, _) = contact_hello(&b.id, b.dev, &t2, "bob", T0, GTTL).unwrap();
    let (_, _, _, binding2, _) = hello_parts(&h2);
    let bad = Msg::ContactHello {
        relay: None,
        attestation: att,
        name,
        invite,
        binding: binding2,
        grant_for_you: grant,
        call_grant_for_you: None,
    };
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &bad, b.dev, T0, &none(), GTTL).err(),
        Some(Error::BadSignature)
    );
}

#[test]
fn binding_signed_by_wrong_key() {
    let (a, b, m) = (peer(), peer(), peer());
    let t = ticket(&a);
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", T0, GTTL).unwrap();
    let (att, name, invite, _, grant) = hello_parts(&hello);
    let claim = open_invite(&invite).unwrap();
    let sig =
        m.id.sign(&binding_message(&claim.nonce, &b.dev, &a.dev).unwrap());
    let bad = Msg::ContactHello {
        relay: None,
        attestation: att,
        name,
        invite,
        binding: enc(sig),
        grant_for_you: grant,
        call_grant_for_you: None,
    };
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &bad, b.dev, T0, &none(), GTTL).err(),
        Some(Error::BadSignature)
    );
}

#[test]
fn hello_grant_wrong_holder_and_expired() {
    let (a, b, m) = (peer(), peer(), peer());
    let t = ticket(&a);
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", T0, GTTL).unwrap();
    let (att, name, invite, binding, _) = hello_parts(&hello);
    let mk = |g: SignedGrant| Msg::ContactHello {
        relay: None,
        attestation: att.clone(),
        name: name.clone(),
        invite: invite.clone(),
        binding: binding.clone(),
        grant_for_you: g,
        call_grant_for_you: None,
    };
    let wrong_holder = mk(issue_grant(&b.id, m.id.did(), T0, GTTL));
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &wrong_holder, b.dev, T0, &none(), GTTL).err(),
        Some(Error::WrongHolder)
    );
    let wrong_issuer = mk(issue_grant(&m.id, a.id.did(), T0, GTTL));
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &wrong_issuer, b.dev, T0, &none(), GTTL).err(),
        Some(Error::WrongIssuer)
    );
    let expired = mk(issue_grant(&b.id, a.id.did(), T0 - 100, 10));
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &expired, b.dev, T0, &none(), GTTL).err(),
        Some(Error::Expired)
    );
}

#[test]
fn grant_expired_and_revoked_on_call() {
    let (a, b) = (peer(), peer());
    let (a_view, b_view) = add_contact(&a, &b);
    let known = |d: &str| d == a_view.did;
    let msg = call_hello(
        &b.id,
        attest(&b.id, b.dev, T0),
        b_view.grant_from_them.clone(),
        "c".into(),
    );

    assert_eq!(
        accept_hello(&a.id, &msg, b.dev, T0 + 2 + GTTL, &none(), known).err(),
        Some(Error::Expired)
    );
    let gid = verify_grant(&b_view.grant_from_them, a.id.did(), b.id.did(), T0, &none())
        .unwrap()
        .id;
    let revoked: HashSet<_> = [gid.clone()].into();
    assert_eq!(
        accept_hello(&a.id, &msg, b.dev, T0 + 5, &revoked, known).err(),
        Some(Error::Revoked)
    );
    let ok = accept_hello(&a.id, &msg, b.dev, T0 + 5, &none(), known).unwrap();
    assert_eq!(ok.grant_id, gid);
}

#[test]
fn non_contact_caller() {
    let (a, b) = (peer(), peer());
    let (_, b_view) = add_contact(&a, &b);
    let msg = call_hello(
        &b.id,
        attest(&b.id, b.dev, T0),
        b_view.grant_from_them,
        "c".into(),
    );
    assert_eq!(
        accept_call_hello(&a.id, &msg, b.dev, T0 + 5, &none(), |_| false).err(),
        Some(Error::NotContact)
    );
}

#[test]
fn stolen_grant_with_other_did_attestation() {
    let (a, b, m) = (peer(), peer(), peer());
    let (_, b_view) = add_contact(&a, &b);
    // Mallory is a contact too, and presents Bob's grant with her own attestation + device.
    let msg = call_hello(
        &m.id,
        attest(&m.id, m.dev, T0),
        b_view.grant_from_them.clone(),
        "c".into(),
    );
    assert_eq!(
        accept_call_hello(&a.id, &msg, m.dev, T0 + 5, &none(), |_| true).err(),
        Some(Error::WrongHolder)
    );
    // Or presents Bob's attestation from his phone's connection she doesn't hold.
    let msg = call_hello(
        &m.id,
        attest(&b.id, b.dev, T0),
        b_view.grant_from_them,
        "c".into(),
    );
    assert_eq!(
        accept_call_hello(&a.id, &msg, m.dev, T0 + 5, &none(), |_| true).err(),
        Some(Error::DeviceMismatch)
    );
}

#[test]
fn grant_issued_by_third_party_rejected_on_call() {
    let (a, b, m) = (peer(), peer(), peer());
    let g = issue_grant(&m.id, b.id.did(), T0, GTTL);
    let msg = call_hello(&b.id, attest(&b.id, b.dev, T0), g, "c".into());
    assert_eq!(
        accept_call_hello(&a.id, &msg, b.dev, T0, &none(), |_| true).err(),
        Some(Error::WrongIssuer)
    );
}

#[test]
fn attestation_forgery_and_domain_separation() {
    let (a, b) = (peer(), peer());
    // Attestation claiming Alice's DID but signed by Bob.
    let claim = Attestation {
        v: 1,
        did: a.id.did().into(),
        device: enc(b.dev),
        iat: T0,
    };
    let forged = sign_blob(&b.id, ATTEST_DOMAIN, &claim);
    assert_eq!(verify_attestation(&forged), Err(Error::BadSignature));
    // A grant blob can't be replayed as an attestation (domains differ).
    let g = issue_grant(&a.id, b.id.did(), T0, GTTL);
    assert!(verify_attestation(&g).is_err());
    let at = attest(&a.id, a.dev, T0);
    assert!(verify_grant(&at, a.id.did(), b.id.did(), T0, &none()).is_err());
}

#[test]
fn cannot_add_self() {
    let a = peer();
    let t = ticket(&a);
    assert_eq!(
        contact_hello(&a.id, a.dev, &t, "me", T0, GTTL).err(),
        Some(Error::SelfContact)
    );
}

#[test]
fn wrong_message_kinds() {
    let (a, b) = (peer(), peer());
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &Msg::Hangup, b.dev, T0, &none(), GTTL).err(),
        Some(Error::UnexpectedMessage)
    );
    assert_eq!(
        accept_call_hello(&a.id, &Msg::Ringing, b.dev, T0, &none(), |_| true).err(),
        Some(Error::UnexpectedMessage)
    );
}

// ---- own-device sync ---------------------------------------------------------------

#[test]
fn self_hello_checks() {
    let (a, other) = (peer(), peer());
    let d2 = dev();
    let none = HashSet::new();
    let att = attest(&a.id, d2, T0);
    assert!(accept_self_hello(&att, a.id.did(), d2, &none).is_ok());
    assert_eq!(
        accept_self_hello(&attest(&other.id, d2, T0), a.id.did(), d2, &none),
        Err(Error::WrongIssuer)
    );
    assert_eq!(
        accept_self_hello(&att, a.id.did(), dev(), &none),
        Err(Error::DeviceMismatch)
    );
    assert_eq!(
        accept_self_hello(&att, a.id.did(), d2, &HashSet::from([d2])),
        Err(Error::Tombstoned)
    );
    // Unrelated tombstones don't matter.
    assert!(accept_self_hello(&att, a.id.did(), d2, &HashSet::from([dev()])).is_ok());
}

#[test]
fn call_grant_and_self_sync_do_not_cross() {
    let (a, b) = (peer(), peer());
    let none = HashSet::new();
    // A call grant is not an attestation, so it cannot open own-device sync.
    let g = issue_grant(&a.id, b.id.did(), T0, GTTL);
    assert!(accept_self_hello(&g, a.id.did(), b.dev, &none).is_err());
    // A self attestation alone (no grant from us) does not get a call accepted: here A's
    // second device calls A's first with a grant from a stranger, and also as a non-contact.
    let d2 = dev();
    let hello = call_hello(
        &a.id,
        attest(&a.id, d2, T0),
        issue_grant(&b.id, a.id.did(), T0, GTTL),
        "c".into(),
    );
    assert_eq!(
        accept_call_hello(&a.id, &hello, d2, T0 + 1, &self::none(), |_| true),
        Err(Error::WrongIssuer)
    );
}
