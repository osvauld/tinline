//! Contact tickets: the `OSVC2:` text form, tampering, expiry, replay.

use super::*;

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
    // The whole flow works from the text form.
    let b = peer();
    let (hello, pending) = contact_hello(&b.id, b.dev, &back, "bob", M0 + 1, GTTL).unwrap();
    let (welcome, _) =
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, M0 + 2, &Unrecorded, GTTL).unwrap();
    accept_contact_welcome(&b.id, &pending, &welcome, a.dev, M0 + 3).unwrap();
    // The retired JSON form is no longer a ticket.
    assert_eq!(
        ContactTicket::from_text("osvc1.abcd"),
        Err(Error::UnknownVersion)
    );
}

#[test]
fn v2_typical_length() {
    let a = peer();
    let relay = Some("https://euc1-1.relay.n0.iroh.link./".to_string());
    let t = issue_contact_ticket(&a.id, a.dev, "Alexandria Montgome", relay, T0, 7 * 86_400);
    assert_eq!(t.name.len(), 19);
    let text = t.to_text();
    assert!(text.len() <= 265, "{}", text.len());
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
        let t = issue_contact_ticket(
            &a.id,
            a.dev,
            "é".repeat(40).as_str(),
            relay.clone(),
            T0,
            TTL,
        );
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
    // Signature from another key over the same body.
    let mut x = t.clone();
    x.invite = Signed::from_parts(t.invite.body().to_vec(), *ticket(&m).invite.sig());
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
    x.device = m.dev;
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
fn tampered_invite_body_fails_signature() {
    let a = peer();
    let mut t = ticket(&a);
    let mut body = t.invite.peek().unwrap();
    body.exp_min += 1_000_000;
    t.invite = Signed::from_parts(
        cryptography::codec::to_bytes(&body).unwrap(),
        *t.invite.sig(),
    );
    assert_eq!(t.verify(T0), Err(Error::BadSignature));
}

/// A body whose signer is not who the ticket claims cannot be opened under the carried key.
#[test]
fn ticket_signed_by_another_key_is_refused() {
    let (a, m) = (peer(), peer());
    let mut body = ticket(&a).invite.peek().unwrap();
    body.pk = m.id.signing_public_key();
    let raw = cryptography::codec::to_bytes(&body).unwrap();
    let sig =
        a.id.sign(&cryptography::sign::input(Ticket::DOMAIN, &[&raw]));
    assert_eq!(
        Signed::<Ticket>::from_parts(raw, sig).open().err(),
        Some(Error::BadSignature)
    );
}

#[test]
fn expired_ticket() {
    let (a, b) = (peer(), peer());
    let t = ticket(&a);
    assert_eq!(t.verify(T0 + TTL), Err(Error::Expired));
    assert_eq!(
        contact_hello(&b.id, b.dev, &t, "bob", M0 + (TTL + 1) * 1000, GTTL).err(),
        Some(Error::Expired)
    );
    // Owner side re-checks too: joiner can't pre-date its clock.
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", M0, GTTL).unwrap();
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, M0 + (TTL + 1) * 1000, &Unrecorded, GTTL).err(),
        Some(Error::Expired)
    );
}

/// The issuer's store decides who may still redeem a ticket: here (joiner, nonce) is spent for
/// one joiner, and another joiner still redeems it.
#[test]
fn redeemed_per_joiner() {
    let (a, b, c) = (peer(), peer(), peer());
    let t = ticket(&a);
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", M0, GTTL).unwrap();
    let (_, nb) = accept_contact_hello(&a.id, a.dev, &hello, b.dev, M0, &Unrecorded, GTTL).unwrap();
    let spent = Spent([redeemed_key(&nb.did, &nb.redeemed_nonce)].into());
    assert_eq!(accept_contact_hello(&a.id, a.dev, &hello, b.dev, M0, &spent, GTTL).err(), Some(Error::Replayed));
    let (again, _) = contact_hello(&b.id, b.dev, &t, "bob", M0 + 1, GTTL).unwrap();
    assert_eq!(accept_contact_hello(&a.id, a.dev, &again, b.dev, M0 + 1, &spent, GTTL).err(), Some(Error::Replayed));
    let (other, _) = contact_hello(&c.id, c.dev, &t, "carol", M0, GTTL).unwrap();
    assert!(accept_contact_hello(&a.id, a.dev, &other, c.dev, M0, &spent, GTTL).is_ok());
}

/// The joiner's half survives a round trip through storage.
#[test]
fn pending_contact_serialises() {
    let (a, b) = (peer(), peer());
    let (_, pending) = contact_hello(&b.id, b.dev, &ticket(&a), "bob", M0, GTTL).unwrap();
    let back: PendingContact = serde_json::from_str(&serde_json::to_string(&pending).unwrap()).unwrap();
    assert_eq!(back, pending);
    assert_eq!(hello_invite(&contact_hello(&b.id, b.dev, &ticket(&a), "bob", M0, GTTL).unwrap().0).unwrap().iss, a.id.did());
}

#[test]
fn invite_issued_by_someone_else() {
    let (a, b, m) = (peer(), peer(), peer());
    // Mallory's ticket redeemed at Alice.
    let t = ticket(&m);
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", M0, GTTL).unwrap();
    assert_eq!(
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, M0, &Unrecorded, GTTL).err(),
        Some(Error::WrongIssuer)
    );
}

#[test]
fn invite_device_must_be_ours() {
    let (a, b) = (peer(), peer());
    let t = ticket(&a);
    let (hello, _) = contact_hello(&b.id, b.dev, &t, "bob", M0, GTTL).unwrap();
    let other_dev = cryptography::signature::public_key(&new_device_secret());
    assert_eq!(
        accept_contact_hello(&a.id, other_dev, &hello, b.dev, M0, &Unrecorded, GTTL).err(),
        Some(Error::DeviceMismatch)
    );
}
