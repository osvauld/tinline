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
            devices: None,
        },
        Msg::Accept {
            renewed_grant: Some(g),
            devices: None,
        },
        Msg::Cancel {
            reason: CancelReason::AnsweredElsewhere,
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

#[test]
fn sanitize_name_strips_and_caps() {
    assert_eq!(sanitize_name("bo\u{202E}b\u{0}\n\u{2066}x\u{2069}"), "bobx");
    assert_eq!(sanitize_name("  Zoë  "), "Zoë");
    let long = "é".repeat(200);
    assert_eq!(sanitize_name(&long).chars().count(), MAX_NAME_CHARS);
}

#[test]
fn peer_names_are_sanitized_on_receipt() {
    let (a, b) = (peer(), peer());
    let t = ticket(&a);
    let evil = format!("bob\u{202E}\u{7}{}", "x".repeat(100));
    let (hello, pending) = contact_hello(&b.id, b.dev, &t, &evil, T0 + 1, GTTL).unwrap();
    let (_, a_view) = accept_contact_hello(&a.id, a.dev, &hello, b.dev, T0 + 2, &none(), GTTL).unwrap();
    assert_eq!(a_view.name.chars().count(), MAX_NAME_CHARS);
    assert!(a_view.name.starts_with("bobxxx"));
    assert!(!a_view.name.contains('\u{202E}') && !a_view.name.contains('\u{7}'));
    // A ticket whose signed name carries control characters decodes clean.
    let t = issue_contact_ticket(&a.id, a.dev, "al\u{202E}ice\n", None, T0, TTL);
    assert_eq!(t.name, "alice");
    let back = ContactTicket::from_text(&t.to_text()).unwrap();
    assert_eq!(back.verify(T0 + 1).unwrap().name, "alice");
    assert_eq!(pending.name, "alice");
}

// ---- multi-device ------------------------------------------------------------------

fn dev() -> [u8; 32] {
    device_public(&new_device_secret())
}

fn list_of(a: &Peer, n: usize, seq: u64) -> SignedBlob {
    let devs: Vec<_> = (0..n).map(|_| (dev(), None)).collect();
    sign_device_list(&a.id, &devs, seq)
}

#[test]
fn device_list_roundtrip_and_bad_relay_dropped() {
    let a = peer();
    let (d1, d2) = (dev(), dev());
    let blob = sign_device_list(
        &a.id,
        &[(d1, Some("https://relay.example/".into())), (d2, Some("http://bad".into()))],
        42,
    );
    let l = verify_device_list(&blob, a.id.did()).unwrap();
    assert_eq!((l.seq, l.did.as_str()), (42, a.id.did()));
    assert_eq!(l.devices[0].device_key().unwrap(), d1);
    assert_eq!(l.devices[0].relay.as_deref(), Some("https://relay.example/"));
    assert_eq!(l.devices[1].relay, None);
}

#[test]
fn device_list_forged_wrong_did_and_limits() {
    let (a, b) = (peer(), peer());
    let blob = list_of(&a, 2, 1);
    assert_eq!(verify_device_list(&blob, b.id.did()), Err(Error::WrongIssuer));
    // Signature by someone else over a payload claiming A.
    let forged = SignedBlob {
        signature: list_of(&b, 2, 1).signature,
        ..blob.clone()
    };
    assert_eq!(verify_device_list(&forged, a.id.did()), Err(Error::BadSignature));
    // Another domain's blob (an attestation) is not a device list.
    assert!(verify_device_list(&attest(&a.id, a.dev, T0), a.id.did()).is_err());
    assert!(verify_device_list(&list_of(&a, MAX_LIST_DEVICES, 1), a.id.did()).is_ok());
    assert_eq!(
        verify_device_list(&list_of(&a, MAX_LIST_DEVICES + 1, 1), a.id.did()),
        Err(Error::TooManyDevices)
    );
    let d = dev();
    let dup = sign_device_list(&a.id, &[(d, None), (d, None)], 1);
    assert_eq!(verify_device_list(&dup, a.id.did()), Err(Error::Decode));
    let empty = sign_device_list(&a.id, &[], 1);
    assert!(verify_device_list(&empty, a.id.did()).is_err());
}

#[test]
fn device_list_ordering() {
    let a = peer();
    let (old, new) = (list_of(&a, 1, 10), list_of(&a, 2, 11));
    assert!(newer(&new, &old) && !newer(&old, &new));
    assert!(!newer(&old, &old));
    // Tie: exactly one side wins, and it is the larger signature.
    let (x, y) = (list_of(&a, 1, 20), list_of(&a, 2, 20));
    assert_ne!(newer(&x, &y), newer(&y, &x));
    let (xs, ys) = (dec(&x.signature).unwrap(), dec(&y.signature).unwrap());
    assert_eq!(newer(&x, &y), xs > ys);
    let junk = SignedBlob { payload: "!".into(), signature: "!".into() };
    assert!(newer(&old, &junk) && !newer(&junk, &old));
}

#[test]
fn old_format_messages_still_parse() {
    let a = peer();
    let g = issue_grant(&a.id, a.id.did(), T0, GTTL);
    let att = attest(&a.id, a.dev, T0);
    let hello = serde_json::json!({"t":"call_hello","call_id":"c","attestation":att,"grant":g});
    match serde_json::from_value::<Msg>(hello).unwrap() {
        Msg::CallHello { devices, relay, .. } => assert!(devices.is_none() && relay.is_none()),
        _ => panic!(),
    }
    let acc = serde_json::json!({"t":"accept","renewed_grant":null});
    assert_eq!(
        serde_json::from_value::<Msg>(acc).unwrap(),
        Msg::Accept { renewed_grant: None, devices: None }
    );
    // And without the new fields set, we emit exactly the old wire shape.
    let m = call_hello(&a.id, att, g, "c".into());
    assert!(!serde_json::to_string(&m).unwrap().contains("devices"));
}

#[test]
fn cancel_tokens() {
    for (r, t) in [
        (CancelReason::AnsweredElsewhere, "answered_elsewhere"),
        (CancelReason::DeclinedElsewhere, "declined_elsewhere"),
        (CancelReason::CallerHangup, "caller_hangup"),
    ] {
        let j = serde_json::to_value(Msg::Cancel { reason: r }).unwrap();
        assert_eq!(j, serde_json::json!({"t":"cancel","reason":t}));
    }
    assert!(serde_json::from_str::<Msg>(r#"{"t":"cancel","reason":"nope"}"#).is_err());
}

#[test]
fn call_hello_with_max_device_list_fits_frame() {
    let a = peer();
    let long = format!("https://{}.example/", "r".repeat(150));
    let devs: Vec<_> = (0..MAX_LIST_DEVICES).map(|_| (dev(), Some(long.clone()))).collect();
    let list = sign_device_list(&a.id, &devs, u64::MAX);
    let mut m = call_hello(
        &a.id,
        attest(&a.id, a.dev, T0),
        issue_grant(&a.id, a.id.did(), T0, GTTL),
        "x".repeat(64),
    );
    if let Msg::CallHello { devices, relay, .. } = &mut m {
        *devices = Some(list);
        *relay = Some(long);
    }
    let f = encode_frame(&m);
    assert!(f.len() - 4 < MAX_FRAME, "{}", f.len());
    assert_eq!(decode_frame(&f).unwrap().unwrap().0, m);
}

// ---- link QR -----------------------------------------------------------------------

#[test]
fn link_qr_roundtrip_case_and_expiry() {
    for relay in [None, Some(KNOWN_RELAYS[2].to_string()), Some("https://my.relay/x".to_string())] {
        let q = new_link_qr(dev(), relay, T0, LINK_TTL_SECS);
        let t = q.to_text();
        assert!(t.starts_with("OSVL1:") && t.chars().all(|c| c.is_ascii_alphanumeric() || c == ':'));
        assert_eq!(LinkQr::from_text(&t).unwrap(), q);
        assert_eq!(LinkQr::from_text(&format!("  {}\n", t.to_lowercase())).unwrap(), q);
        assert!(!q.is_expired(T0 + 299));
        assert!(q.is_expired(T0 + 360));
        assert!(q.exp >= T0 + LINK_TTL_SECS);
    }
    let q = new_link_qr(dev(), None, T0, 300);
    assert_ne!(q.secret, new_link_qr(q.device, None, T0, 300).secret);
    assert_eq!(LinkQr::from_text("OSVC2:AAAA"), Err(Error::UnknownVersion));
    assert!(LinkQr::from_text(&format!("{}A", q.to_text())).is_err());
    assert!(LinkQr::from_text("OSVL1:AAAA").is_err());
    assert!(LinkQr::from_text("").is_err());
}

// ---- link handshake ----------------------------------------------------------------

const SECRET: [u8; 16] = [7; 16];

#[test]
fn link_proofs() {
    let (e, n) = (dev(), dev());
    let p = link_proof(&SECRET, &e, &n, LinkRole::Scanner, &n);
    assert!(verify_link_proof(&SECRET, &e, &n, LinkRole::Scanner, &n, &p).is_ok());
    assert!(verify_link_proof(&[8; 16], &e, &n, LinkRole::Scanner, &n, &p).is_err());
    assert!(verify_link_proof(&SECRET, &e, &n, LinkRole::Displayer, &n, &p).is_err());
    assert!(verify_link_proof(&SECRET, &e, &n, LinkRole::Scanner, &e, &p).is_err());
    assert!(verify_link_proof(&SECRET, &n, &e, LinkRole::Scanner, &n, &p).is_err());
    assert!(verify_link_proof(&SECRET, &e, &n, LinkRole::Scanner, &n, &p[..31]).is_err());
    // Message form.
    let hello = link_hello(&SECRET, &e, &n, LinkRole::Displayer, &e, "Tablet");
    assert!(accept_link_hello(&SECRET, &e, &n, LinkRole::Displayer, &e, &hello).is_ok());
    assert!(accept_link_hello(&SECRET, &e, &n, LinkRole::Scanner, &e, &hello).is_err());
    assert!(accept_link_hello(&SECRET, &e, &n, LinkRole::Displayer, &n, &hello).is_err());
    assert!(accept_link_hello(&SECRET, &e, &n, LinkRole::Displayer, &e, &LinkMsg::Unlinked).is_err());
}

#[test]
fn confirm_codes() {
    let (e, n) = (dev(), dev());
    let c = confirm_code(&SECRET, &e, &n);
    assert_eq!(c, confirm_code(&SECRET, &e, &n));
    let b = c.as_bytes();
    assert!(c.len() == 7 && b[3] == b' ' && c.replace(' ', "").bytes().all(|x| x.is_ascii_digit()));
    // Over many secrets/devices codes vary and are always well formed.
    let mut seen = HashSet::new();
    for i in 0..50u8 {
        let c = confirm_code(&[i; 16], &e, &n);
        assert_eq!(c.len(), 7);
        seen.insert(c);
    }
    assert!(seen.len() > 40);
    assert_ne!(confirm_code(&[8; 16], &e, &n), c);
    assert_ne!(confirm_code(&SECRET, &e, &dev()), c);
}

fn body(e: &Peer) -> LinkGrantBody {
    LinkGrantBody {
        mnemonic: "word ".repeat(24),
        account_name: "Me".into(),
        registry: vec![RegistryEntry {
            attestation: attest(&e.id, e.dev, T0),
            label: "Pixel".into(),
            removed: false,
        }],
    }
}

#[test]
fn link_grant_seal_open() {
    let (e, n) = (peer(), dev());
    let sealed = seal_link_grant(&SECRET, &e.dev, &n, b"secret words");
    assert_eq!(open_link_grant(&SECRET, &e.dev, &n, &sealed).unwrap(), b"secret words");
    assert!(open_link_grant(&[8; 16], &e.dev, &n, &sealed).is_err());
    assert!(open_link_grant(&SECRET, &n, &e.dev, &sealed).is_err());
    let mut raw = dec(&sealed).unwrap();
    *raw.last_mut().unwrap() ^= 1;
    assert!(open_link_grant(&SECRET, &e.dev, &n, &enc(raw)).is_err());
    assert!(open_link_grant(&SECRET, &e.dev, &n, "AA").is_err());
    assert_ne!(sealed, seal_link_grant(&SECRET, &e.dev, &n, b"secret words"));

    let b = body(&e);
    let msg = seal_link_grant_body(&SECRET, &e.dev, &n, &b);
    let f = encode_link_frame(&msg);
    let (back, used) = decode_link_frame(&f).unwrap().unwrap();
    assert_eq!(used, f.len());
    assert_eq!(open_link_grant_body(&SECRET, &e.dev, &n, &back).unwrap(), b);
    assert!(open_link_grant_body(&[8; 16], &e.dev, &n, &back).is_err());
    let json = serde_json::to_string(&back).unwrap();
    assert!(!json.contains("word") && json.contains("\"t\":\"link_grant\""));
}

#[test]
fn link_grant_with_full_registry_fits_frame() {
    let e = peer();
    let mut b = body(&e);
    b.registry = (0..MAX_LIST_DEVICES * 2)
        .map(|_| RegistryEntry { attestation: attest(&e.id, dev(), T0), label: "x".repeat(64), removed: false })
        .collect();
    let f = encode_link_frame(&seal_link_grant_body(&SECRET, &e.dev, &dev(), &b));
    assert!(f.len() - 4 < MAX_FRAME, "{}", f.len());
}

#[test]
fn link_done_checks() {
    let (e, n, other) = (peer(), peer(), peer());
    let done = |att| LinkMsg::LinkDone { attestation: att, label: format!("Ph\u{202E}one{}", "x".repeat(100)) };
    // N holds E's identity after import: same DID, its own device key.
    let n_dev = dev();
    let ok = accept_link_done(&done(attest(&e.id, n_dev, T0)), e.id.did(), n_dev).unwrap();
    assert_eq!(ok.attestation.did, e.id.did());
    assert_eq!(ok.label.chars().count(), MAX_NAME_CHARS);
    assert!(!ok.label.contains('\u{202E}'));
    assert_eq!(
        accept_link_done(&done(attest(&other.id, n_dev, T0)), e.id.did(), n_dev),
        Err(Error::WrongIssuer)
    );
    assert_eq!(
        accept_link_done(&done(attest(&e.id, n_dev, T0)), e.id.did(), n.dev),
        Err(Error::DeviceMismatch)
    );
    let mut forged = attest(&e.id, n_dev, T0);
    forged.signature = attest(&other.id, n_dev, T0).signature;
    assert_eq!(accept_link_done(&done(forged), e.id.did(), n_dev), Err(Error::BadSignature));
    assert_eq!(
        accept_link_done(&LinkMsg::Unlinked, e.id.did(), n_dev),
        Err(Error::UnexpectedMessage)
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
    assert_eq!(accept_self_hello(&att, a.id.did(), dev(), &none), Err(Error::DeviceMismatch));
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
    let hello = call_hello(&a.id, attest(&a.id, d2, T0), issue_grant(&b.id, a.id.did(), T0, GTTL), "c".into());
    assert_eq!(
        accept_call_hello(&a.id, &hello, d2, T0 + 1, &self::none(), |_| true),
        Err(Error::WrongIssuer)
    );
}
