//! Contact tickets carrying a code wake token (`docs/push.md` "Code wake tokens").

use super::*;

/// About what the relay seals for an FCM token (~250 bytes); the content is opaque to proto.
fn fcm_sized_token() -> Vec<u8> {
    (0..250u32).map(|i| (i * 7 + 3) as u8).collect()
}

fn ticket_with(a: &Peer, wake: Option<Vec<u8>>) -> ContactTicket {
    let (nonce, exp_min) = ticket_nonce_and_exp(T0, TTL);
    issue_contact_ticket_with(
        &a.id,
        a.dev,
        "alice",
        Some("https://euc1-1.relay.n0.iroh.link./".into()),
        nonce,
        exp_min,
        wake,
    )
}

#[test]
fn ticket_round_trips_with_and_without_a_wake_token() {
    let a = peer();
    for wake in [None, Some(fcm_sized_token())] {
        let t = ticket_with(&a, wake.clone());
        let back = ContactTicket::from_text(&t.to_text()).unwrap();
        assert_eq!(back, t);
        let claim = back.verify(T0 + 1).unwrap();
        assert_eq!(claim.wake, wake);
        assert_eq!(claim.exp, T0 + TTL);
        // The add flow is unchanged by it.
        let b = peer();
        let (hello, pending) = contact_hello(&b.id, b.dev, &back, "bob", M0 + 1, GTTL).unwrap();
        let (welcome, _) =
            accept_contact_hello(&a.id, a.dev, &hello, b.dev, M0 + 2, &Unrecorded, GTTL).unwrap();
        accept_contact_welcome(&b.id, &pending, &welcome, a.dev, M0 + 3).unwrap();
    }
    // The plain issuer makes a ticket without one.
    assert_eq!(ticket(&a).verify(T0).unwrap().wake, None);
}

#[test]
fn ticket_wake_token_size_bound() {
    let a = peer();
    let plain = ticket_with(&a, None).to_text();
    let with = ticket_with(&a, Some(fcm_sized_token())).to_text();
    // A 250-byte token adds 250 bytes plus a 2-byte length: 8/5 of that in base32.
    let added = with.len() - plain.len();
    assert!((400..=405).contains(&added), "{added}");
    assert!(
        plain.len() <= 265 && with.len() <= 670,
        "{} {}",
        plain.len(),
        with.len()
    );
    // The largest token that fits still parses; a larger or empty one is left out, not signed.
    let max = ticket_with(&a, Some(vec![1; MAX_TICKET_WAKE]));
    assert_eq!(
        ContactTicket::from_text(&max.to_text())
            .unwrap()
            .verify(T0)
            .unwrap()
            .wake
            .map(|w| w.len()),
        Some(MAX_TICKET_WAKE)
    );
    assert_eq!(
        ticket_with(&a, Some(vec![1; MAX_TICKET_WAKE + 1]))
            .verify(T0)
            .unwrap()
            .wake,
        None
    );
    assert_eq!(
        ticket_with(&a, Some(Vec::new())).verify(T0).unwrap().wake,
        None
    );
    // A body naming an oversized token is refused when opened.
    let mut body = max.invite.peek().unwrap();
    body.wake = Some(vec![1; MAX_TICKET_WAKE + 1]);
    let raw = cryptography::codec::to_bytes(&body).unwrap();
    let sig =
        a.id.sign(&cryptography::sign::input(Ticket::DOMAIN, &[&raw]));
    assert!(Signed::<Ticket>::from_parts(raw, sig).open().is_err());
}

#[test]
fn ticket_signature_covers_the_wake_token() {
    let (a, m) = (peer(), peer());
    let t = ticket_with(&a, Some(fcm_sized_token()));
    // Another token swapped in under the same signature.
    let mut body = t.invite.peek().unwrap();
    body.wake = Some(vec![9; 250]);
    let mut x = t.clone();
    x.invite = Signed::from_parts(
        cryptography::codec::to_bytes(&body).unwrap(),
        *t.invite.sig(),
    );
    assert_eq!(x.verify(T0), Err(Error::BadSignature));
    // Token removed.
    body.wake = None;
    x.invite = Signed::from_parts(
        cryptography::codec::to_bytes(&body).unwrap(),
        *t.invite.sig(),
    );
    assert_eq!(x.verify(T0), Err(Error::BadSignature));
    // Token added to a ticket that had none.
    let plain = ticket_with(&a, None);
    let mut body = plain.invite.peek().unwrap();
    body.wake = Some(fcm_sized_token());
    x.invite = Signed::from_parts(
        cryptography::codec::to_bytes(&body).unwrap(),
        *plain.invite.sig(),
    );
    assert_eq!(x.verify(T0), Err(Error::BadSignature));
    // Mallory's token in a ticket of Alice's: only Alice can sign one.
    let theirs = ticket_with(&m, Some(vec![5; 250]));
    let mut body = t.invite.peek().unwrap();
    body.wake = theirs.invite.peek().unwrap().wake;
    x.invite = Signed::from_parts(
        cryptography::codec::to_bytes(&body).unwrap(),
        *theirs.invite.sig(),
    );
    assert!(x.verify(T0).is_err());
    // Any flipped bit of the token is caught.
    let text = t.to_text();
    let bytes = b32_decode(&text["OSVC2:".len()..]).unwrap();
    let start = bytes.len() - 64 - 250;
    for i in start..bytes.len() - 64 {
        let mut bad = bytes.clone();
        bad[i] ^= 0x10;
        let ok = ContactTicket::from_text(&format!("OSVC2:{}", b32_encode(&bad)))
            .and_then(|t| t.verify(T0));
        assert!(ok.is_err(), "byte {i}");
    }
}

#[test]
fn wake_handover_in_hello_and_welcome_round_trips() {
    let (a, b) = (peer(), peer());
    let (hello, _) = contact_hello(&b.id, b.dev, &ticket(&a), "bob", M0, GTTL).unwrap();
    let h = WakeHandover {
        token: "tok".into(),
        exp_ms: M0 + 1,
        kinds: 3,
    };
    let Msg::ContactHello {
        attestation,
        name,
        invite,
        binding,
        grant_for_you,
        call_grant_for_you,
        relay,
        ..
    } = hello
    else {
        panic!()
    };
    let with = Msg::ContactHello {
        attestation,
        name,
        invite,
        binding,
        grant_for_you,
        call_grant_for_you,
        relay,
        wake: Some(h.clone()),
    };
    let json = serde_json::to_string(&with).unwrap();
    assert_eq!(serde_json::from_str::<Msg>(&json).unwrap(), with);
    // Still accepted: the handover is not part of the checked hello.
    accept_contact_hello(&a.id, a.dev, &with, b.dev, M0, &Unrecorded, GTTL).unwrap();
    // An oversized token is refused at parse time.
    let long = json.replace(
        "\"tok\"",
        &format!("\"{}\"", "x".repeat(MAX_WAKE_TOKEN_CHARS + 1)),
    );
    assert!(serde_json::from_str::<Msg>(&long).is_err());
}
