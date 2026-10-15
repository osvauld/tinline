use super::*;

// KIND: unit (G2 stage 3) — U3: the host relays an envelope it can check but not read; the
// body cannot be moved into another sender's envelope.
#[test]
fn mail_is_sealed_to_the_recipient_and_bound_to_its_envelope() {
    let (a, b, host) = (identity::generate().0, identity::generate().0, identity::generate().0);
    let letter = Letter::Accept { req: [1; 16], ticket: "secret-ticket-text".into() };
    let m = seal(&b, "g", a.did(), &a.encryption_public_key(), letter.clone(), 5).unwrap();
    let bytes = m.to_bytes();
    assert!(!bytes.windows(13).any(|w| w == b"secret-ticket"), "plaintext visible to the host");
    let env = SignedMail::from_bytes(&bytes).unwrap().open().unwrap();
    assert_eq!((env.from.as_str(), env.to.as_str()), (b.did(), a.did()));
    assert_eq!(open_for(&a, "g", &m).unwrap().1, letter);
    assert!(open_for(&host, "g", &m).is_none(), "the host cannot open it");
    assert!(open_for(&a, "other", &m).is_none());
    // The host re-wraps the sealed body in its own envelope: refused by the recipient.
    let mut forged = env.clone();
    forged.from = host.did().into();
    let body = cryptography::codec::to_bytes(&forged).unwrap();
    let fm = SignedMail { sig: host.sign(&sig_input(&body)).to_vec(), body };
    assert!(fm.open().is_some());
    assert!(open_for(&a, "g", &fm).is_none());
    // Tampered signature.
    let mut t = m.clone();
    t.sig[0] ^= 1;
    assert!(t.open().is_none());
}
