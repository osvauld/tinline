use super::*;

const T0: u64 = 1_800_000_000;
const TTL: u64 = 600;
const GTTL: u64 = 86_400;

struct Peer {
    id: Identity,
    secret: [u8; 32],
    dev: [u8; 32],
}

fn peer() -> Peer {
    let (id, _) = identity::generate();
    let secret = new_device_secret();
    Peer {
        id,
        dev: device_public(&secret),
        secret,
    }
}

fn none() -> HashSet<String> {
    HashSet::new()
}

fn ticket(a: &Peer) -> ContactTicket {
    issue_contact_ticket(&a.id, a.dev, "alice", Some("https://relay".into()), T0, TTL)
}

/// Full add-contact: returns (alice's view of bob, bob's view of alice).
fn add_contact(a: &Peer, b: &Peer) -> (NewContact, NewContact) {
    let t = ticket(a);
    let (hello, pending) = contact_hello(&b.id, b.dev, &t, "bob", T0 + 1, GTTL).unwrap();
    let (welcome, a_view) =
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, T0 + 2, &none(), GTTL).unwrap();
    let b_view = accept_contact_welcome(&b.id, &pending, &welcome, a.dev, T0 + 3).unwrap();
    (a_view, b_view)
}

fn hello_parts(m: &Msg) -> (SignedAttestation, String, SignedBlob, String, SignedGrant) {
    match m.clone() {
        Msg::ContactHello {
        relay: None,
            attestation,
            name,
            invite,
            binding,
            grant_for_you,
        } => (attestation, name, invite, binding, grant_for_you),
        _ => panic!(),
    }
}

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
    // Bob calls Alice with the grant Alice gave him.
    let msg = call_hello(
        &b.id,
        attest(&b.id, b.dev, T0 + 10),
        b_view.grant_from_them.clone(),
        "c1".into(),
    );
    let known = |d: &str| d == a_view.did;
    let c = accept_call_hello(&a.id, &msg, b.dev, T0 + 10, &none(), known).unwrap();
    assert_eq!((c.did.as_str(), c.device), (b.id.did(), b.dev));

    // Bob's second phone, same DID, own attestation: allowed.
    let dev2 = device_public(&new_device_secret());
    let msg2 = call_hello(
        &b.id,
        attest(&b.id, dev2, T0 + 11),
        b_view.grant_from_them,
        "c2".into(),
    );
    let c2 = accept_call_hello(&a.id, &msg2, dev2, T0 + 11, &none(), known).unwrap();
    assert_eq!(c2.device, dev2);
}

/// A v1 ticket as the previous release issued it (JSON invite).
fn ticket_v1(a: &Peer) -> ContactTicket {
    let claim = InviteClaim {
        v: 1,
        iss: a.id.did().to_string(),
        nonce: random16(),
        iat: T0,
        exp: T0 + TTL,
        device: enc(a.dev),
        name: "alice".into(),
        relay: Some("https://relay".into()),
    };
    ContactTicket {
        v: 1,
        did: claim.iss.clone(),
        name: claim.name.clone(),
        device: claim.device.clone(),
        relay: claim.relay.clone(),
        invite: sign_blob(&a.id, INVITE_DOMAIN, &claim),
    }
}

#[test]
fn ticket_text_roundtrip_and_prefix() {
    let a = peer();
    let t = ticket(&a);
    let text = t.to_text();
    assert!(text.starts_with("OSVC2:"));
    let back = ContactTicket::from_text(&text).unwrap();
    assert_eq!(back, t);
    let claim = back.verify(T0 + 1).unwrap();
    assert_eq!(claim.iss, a.id.did());
    assert_eq!(claim.exp, T0 + TTL);
    // Case-insensitive body and prefix, surrounding whitespace tolerated.
    let lower = format!("  \n{}\t", text.to_lowercase());
    assert_eq!(ContactTicket::from_text(&lower).unwrap(), t);
    assert_eq!(
        ContactTicket::from_text(&text.replace("OSVC2:", "osvc3:")),
        Err(Error::UnknownVersion)
    );
    // v1 text still accepted, and keeps working end to end.
    let (b, v1) = (peer(), ticket_v1(&a));
    let v1_text = v1.to_text();
    assert!(v1_text.starts_with("osvc1."));
    let v1_back = ContactTicket::from_text(&v1_text).unwrap();
    assert_eq!(v1_back, v1);
    v1_back.verify(T0 + 1).unwrap();
    let (hello, pending) = contact_hello(&b.id, b.dev, &v1_back, "bob", T0 + 1, GTTL).unwrap();
    let (welcome, _) =
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, T0 + 2, &none(), GTTL).unwrap();
    accept_contact_welcome(&b.id, &pending, &welcome, a.dev, T0 + 3).unwrap();
}

#[test]
fn v2_typical_length() {
    let a = peer();
    let relay = Some("https://euc1-1.relay.n0.iroh.link./".to_string());
    let t = issue_contact_ticket(&a.id, a.dev, "Alexandria Montgome", relay, T0, 7 * 86_400);
    assert_eq!(t.name.len(), 19);
    let text = t.to_text();
    println!("v2 typical ticket length: {} ({text})", text.len());
    assert!(text.len() <= 260, "{}", text.len());
    println!("v1 ticket length: {}", ticket_v1(&a).to_text().len());
    assert_eq!(ContactTicket::from_text(&text).unwrap(), t);
}

#[test]
fn v2_relay_codes_and_name_cap() {
    let a = peer();
    for relay in [
        None,
        Some("https://use1-1.relay.n0.iroh.link./".to_string()),
        Some("https://aps1-1.relay.n0.iroh.link./".to_string()),
        Some("https://my.relay.example/".to_string()),
    ] {
        let t = issue_contact_ticket(&a.id, a.dev, "é".repeat(40).as_str(), relay.clone(), T0, TTL);
        assert!(t.name.len() <= 31 && t.name.chars().all(|c| c == 'é'));
        let back = ContactTicket::from_text(&t.to_text()).unwrap();
        assert_eq!(back.relay, relay);
        back.verify(T0).unwrap();
    }
}

#[test]
fn v2_any_flipped_bit_fails() {
    let a = peer();
    let t = ticket(&a);
    let text = t.to_text();
    let body = b32_decode(&text["OSVC2:".len()..]).unwrap();
    for i in 0..body.len() {
        let mut bad = body.clone();
        bad[i] ^= 1;
        let text = format!("OSVC2:{}", b32_encode(&bad));
        let ok = ContactTicket::from_text(&text).and_then(|t| t.verify(T0));
        assert!(ok.is_err(), "byte {i} tamper accepted");
    }
    // Truncation and trailing garbage.
    assert!(ContactTicket::from_text(&text[..text.len() - 4]).is_err());
    assert!(ContactTicket::from_text(&format!("{text}AA")).is_err());
}

#[test]
fn v2_visible_fields_must_match_and_blob_tamper() {
    let (a, m) = (peer(), peer());
    let t = ticket(&a);
    let mut x = t.clone();
    x.name = "mallory".into();
    assert_eq!(x.verify(T0), Err(Error::TicketMismatch));
    let mut x = t.clone();
    x.invite = ticket(&m).invite;
    assert_eq!(x.verify(T0), Err(Error::TicketMismatch));
    // Signature from another key over the same payload.
    let mut x = t;
    x.invite.signature = ticket(&m).invite.signature;
    assert_eq!(x.verify(T0), Err(Error::BadSignature));
}

#[test]
fn tampered_visible_ticket_fields() {
    let a = peer();
    let m = peer();
    let t = ticket(&a);
    let mut x = t.clone();
    x.name = "mallory".into();
    assert_eq!(x.verify(T0), Err(Error::TicketMismatch));
    let mut x = t.clone();
    x.device = enc(m.dev);
    assert_eq!(x.verify(T0), Err(Error::TicketMismatch));
    let mut x = t.clone();
    x.relay = Some("https://evil".into());
    assert_eq!(x.verify(T0), Err(Error::TicketMismatch));
    let mut x = t.clone();
    x.did = m.id.did().into();
    assert_eq!(x.verify(T0), Err(Error::TicketMismatch));
    // Swapping in someone else's invite under our visible fields.
    let mut x = t;
    x.invite = ticket(&m).invite;
    assert_eq!(x.verify(T0), Err(Error::TicketMismatch));
}

#[test]
fn tampered_invite_payload_fails_signature() {
    let a = peer();
    let mut t = ticket_v1(&a);
    let mut claim: InviteClaim = serde_json::from_slice(&dec(&t.invite.payload).unwrap()).unwrap();
    claim.exp += 1_000_000;
    t.invite.payload = enc(serde_json::to_vec(&claim).unwrap());
    assert_eq!(t.verify(T0), Err(Error::BadSignature));
}

#[test]
fn expired_ticket() {
    let (a, b) = (peer(), peer());
    let t = ticket(&a);
    assert_eq!(t.verify(T0 + TTL), Err(Error::Expired));
    assert_eq!(
        contact_hello(&b.id, b.dev, &t, "bob", T0 + TTL + 1, GTTL).err(),
        Some(Error::Expired)
    );
    // Owner side re-checks too: joiner can't pre-date its clock.
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", T0, GTTL).unwrap();
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, T0 + TTL + 1, &none(), GTTL).err(),
        Some(Error::Expired)
    );
}

#[test]
fn replayed_nonce() {
    let (a, b) = (peer(), peer());
    let t = ticket(&a);
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", T0, GTTL).unwrap();
    let (_, nc) = accept_contact_hello(&a.id, a.dev, &hello, b.dev, T0, &none(), GTTL).unwrap();
    let spent: HashSet<_> = [nc.redeemed_nonce].into();
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, T0, &spent, GTTL).err(),
        Some(Error::Replayed)
    );
}

#[test]
fn invite_issued_by_someone_else() {
    let (a, b, m) = (peer(), peer(), peer());
    // Mallory's ticket redeemed at Alice.
    let t = ticket(&m);
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", T0, GTTL).unwrap();
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, T0, &none(), GTTL).err(),
        Some(Error::WrongIssuer)
    );
}

#[test]
fn invite_device_must_be_ours() {
    let (a, b) = (peer(), peer());
    let t = ticket(&a);
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", T0, GTTL).unwrap();
    let other_dev = device_public(&new_device_secret());
    assert_eq!(
        accept_contact_hello(&a.id, other_dev, &hello, b.dev, T0, &none(), GTTL).err(),
        Some(Error::DeviceMismatch)
    );
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
    };
    assert_eq!(
        accept_contact_welcome(&b.id, &pending, &bad, a.dev, T0).err(),
        Some(Error::WrongHolder)
    );
    let bad = Msg::ContactWelcome {
        attestation,
        name,
        grant_for_you: issue_grant(&m.id, b.id.did(), T0, GTTL),
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
        accept_call_hello(&a.id, &msg, b.dev, T0 + 2 + GTTL, &none(), known).err(),
        Some(Error::Expired)
    );
    let gid = verify_grant(&b_view.grant_from_them, a.id.did(), b.id.did(), T0, &none())
        .unwrap()
        .id;
    let revoked: HashSet<_> = [gid.clone()].into();
    assert_eq!(
        accept_call_hello(&a.id, &msg, b.dev, T0 + 5, &revoked, known).err(),
        Some(Error::Revoked)
    );
    let ok = accept_call_hello(&a.id, &msg, b.dev, T0 + 5, &none(), known).unwrap();
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

#[test]
fn frame_roundtrip_all_variants() {
    let a = peer();
    let g = issue_grant(&a.id, a.id.did(), T0, GTTL);
    let msgs = [
        Msg::Ringing,
        Msg::Busy,
        Msg::Hangup,
        Msg::Reject {
            reason: "no".into(),
        },
        Msg::Decline {
            reason: "later".into(),
        },
        Msg::Accept {
            renewed_grant: None,
        },
        Msg::Accept {
            renewed_grant: Some(g),
        },
    ];
    for m in msgs {
        let f = encode_frame(&m);
        let (back, n) = decode_frame(&f).unwrap().unwrap();
        assert_eq!((back, n), (m, f.len()));
    }
}

#[test]
fn frame_split_across_reads_and_back_to_back() {
    let m1 = Msg::Reject { reason: "x".into() };
    let m2 = Msg::Hangup;
    let mut wire = encode_frame(&m1);
    let first_len = wire.len();
    wire.extend(encode_frame(&m2));
    // Every prefix shorter than the first frame needs more bytes.
    for cut in 0..first_len {
        assert_eq!(decode_frame(&wire[..cut]).unwrap(), None, "cut {cut}");
    }
    let (got1, used) = decode_frame(&wire).unwrap().unwrap();
    assert_eq!((got1, used), (m1, first_len));
    let (got2, _) = decode_frame(&wire[used..]).unwrap().unwrap();
    assert_eq!(got2, m2);
}

#[test]
fn oversized_and_garbage_frames() {
    // Rejected from the header alone, before the body arrives.
    let head = ((MAX_FRAME + 1) as u32).to_be_bytes();
    assert_eq!(decode_frame(&head), Err(Error::FrameTooLarge));
    assert_eq!(
        decode_frame(&u32::MAX.to_be_bytes()),
        Err(Error::FrameTooLarge)
    );
    let mut f = 5u32.to_be_bytes().to_vec();
    f.extend(b"nojso");
    assert_eq!(decode_frame(&f), Err(Error::Decode));
}
