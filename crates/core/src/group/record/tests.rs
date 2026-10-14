use super::*;

fn acct() -> identity::Identity {
    identity::generate().0
}

fn rec(a: &identity::Identity, group: &str, body: Body) -> Record {
    Record { v: 1, group: group.into(), author: a.did().into(), dev: [7; 32], at: 1000, seen: vec![(1, 2)], body }
}

// KIND: unit (G1 stage 1)
#[test]
fn records_verify_and_tampering_is_refused() {
    let a = acct();
    let r = rec(&a, "g", Body::Post { text: "hi".into(), reply_to: None });
    let bytes = r.sign(&a).to_bytes();
    assert_eq!(SignedRecord::open(&bytes, "g").unwrap(), r);
    assert_eq!(SignedRecord::open(&bytes, "other"), Err(RecordError::WrongGroup));
    // Any flipped byte: undecodable or a bad signature, never accepted.
    for i in 0..bytes.len() {
        let mut t = bytes.clone();
        t[i] ^= 0x41;
        assert!(SignedRecord::open(&t, "g").is_err(), "byte {i}");
    }
    // Trailing junk: a second id for the same record is refused.
    let mut t = bytes.clone();
    t.push(0);
    assert!(SignedRecord::open(&t, "g").is_err());
    // Claiming another author: the signature is the signer's, not the claimed DID's.
    let b = acct();
    let mut forged = rec(&a, "g", Body::Post { text: "as b".into(), reply_to: None });
    forged.author = b.did().into();
    let body = bincode::serialize(&forged).unwrap();
    let sig = a.sign(&sig_input(RECORD_DOMAIN, &body)).to_vec();
    let bytes = SignedRecord { body, sig }.to_bytes();
    assert_eq!(SignedRecord::open(&bytes, "g"), Err(RecordError::BadSignature));
}

// KIND: unit (G1 stage 1)
#[test]
fn malformed_records_are_refused() {
    let a = acct();
    let cases = [
        Body::Post { text: String::new(), reply_to: None },
        Body::Post { text: "x".repeat(MAX_TEXT + 1), reply_to: None },
        Body::Genesis { name: " ".into(), nonce: [0; 16], owner_name: "o".into() },
        Body::CallHostPermit { host: "d".repeat(201), host_dev: [0; 32], active: true },
    ];
    for b in cases {
        assert_eq!(SignedRecord::open(&rec(&a, "g", b).sign(&a).to_bytes(), "g"), Err(RecordError::Malformed));
    }
    let mut r = rec(&a, "g", Body::Leave);
    r.seen = vec![(5, 1), (3, 1)];
    assert_eq!(SignedRecord::open(&r.sign(&a).to_bytes(), "g"), Err(RecordError::Malformed));
}

// KIND: unit (G1 stage 1)
#[test]
fn invites_host_addrs_and_auth_verify() {
    let a = acct();
    let b = acct();
    let inv = Invite { group: "g".into(), issuer: a.did().into(), expires: 10, nonce: [1; 16] }.sign(&a);
    assert_eq!(inv.open().unwrap().issuer, a.did());
    let mut forged = Invite { group: "g".into(), issuer: b.did().into(), expires: 10, nonce: [1; 16] }.sign(&a);
    assert!(forged.open().is_none());
    forged.sig[0] ^= 1;
    assert!(forged.open().is_none());

    let ha = HostAddr { group: "g".into(), did: a.did().into(), dev: [1; 32], endpoint: [2; 32], relay: None, at: 5 }.sign(&a);
    assert!(ha.open().is_some());
    let mut t = ha.clone();
    t.body[3] ^= 1;
    assert!(t.open().is_none());

    let auth = EndpointAuth::new(&a, [1; 32], [9; 32]);
    assert!(auth.verify(&[9; 32]));
    assert!(!auth.verify(&[8; 32]), "made for another endpoint");
    let mut t = auth.clone();
    t.did = b.did().into();
    assert!(!t.verify(&[9; 32]));

    let tok = InviteToken { group: "g".into(), name: "n".into(), owner: a.did().into(), invite: Some(inv), hosts: vec![ha] };
    assert_eq!(bincode::deserialize::<InviteToken>(&bincode::serialize(&tok).unwrap()).unwrap(), tok);
}

#[test]
fn group_ids_bind_owner_and_nonce() {
    assert_ne!(group_id("did:a", &[0; 16]), group_id("did:b", &[0; 16]));
    assert_ne!(group_id("did:a", &[0; 16]), group_id("did:a", &[1; 16]));
}
