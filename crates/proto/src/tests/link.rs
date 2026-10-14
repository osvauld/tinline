//! Device linking: QR, proofs, confirm codes, sealed account grant.

use super::*;

// ---- link QR -----------------------------------------------------------------------

#[test]
fn link_qr_roundtrip_case_and_expiry() {
    for relay in [
        None,
        Some(KNOWN_RELAYS[2].to_string()),
        Some("https://my.relay/x".to_string()),
    ] {
        let q = new_link_qr(dev(), relay, T0, LINK_TTL_SECS);
        let t = q.to_text();
        assert!(
            t.starts_with("OSVL1:") && t.chars().all(|c| c.is_ascii_alphanumeric() || c == ':')
        );
        assert_eq!(LinkQr::from_text(&t).unwrap(), q);
        assert_eq!(
            LinkQr::from_text(&format!("  {}\n", t.to_lowercase())).unwrap(),
            q
        );
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
    assert!(
        accept_link_hello(&SECRET, &e, &n, LinkRole::Displayer, &e, &LinkMsg::Unlinked).is_err()
    );
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
    assert_eq!(
        open_link_grant(&SECRET, &e.dev, &n, &sealed).unwrap(),
        b"secret words"
    );
    assert!(open_link_grant(&[8; 16], &e.dev, &n, &sealed).is_err());
    assert!(open_link_grant(&SECRET, &n, &e.dev, &sealed).is_err());
    let mut raw = dec(&sealed).unwrap();
    *raw.last_mut().unwrap() ^= 1;
    assert!(open_link_grant(&SECRET, &e.dev, &n, &enc(raw)).is_err());
    assert!(open_link_grant(&SECRET, &e.dev, &n, "AA").is_err());
    assert_ne!(
        sealed,
        seal_link_grant(&SECRET, &e.dev, &n, b"secret words")
    );

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
        .map(|_| RegistryEntry {
            attestation: attest(&e.id, dev(), T0),
            label: "x".repeat(64),
            removed: false,
        })
        .collect();
    let f = encode_link_frame(&seal_link_grant_body(&SECRET, &e.dev, &dev(), &b));
    assert!(f.len() - 4 < MAX_FRAME, "{}", f.len());
}

#[test]
fn link_done_checks() {
    let (e, n, other) = (peer(), peer(), peer());
    let done = |att| LinkMsg::LinkDone {
        attestation: att,
        label: format!("Ph\u{202E}one{}", "x".repeat(100)),
    };
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
    assert_eq!(
        accept_link_done(&done(forged), e.id.did(), n_dev),
        Err(Error::BadSignature)
    );
    assert_eq!(
        accept_link_done(&LinkMsg::Unlinked, e.id.did(), n_dev),
        Err(Error::UnexpectedMessage)
    );
}
