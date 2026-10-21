use super::*;

fn acct() -> identity::Identity {
    identity::generate().0
}

/// An envelope over `x` signed by `key` whatever `x` names as its signer.
fn forge<T: Signable>(x: &T, key: &identity::Identity) -> Signed<T> {
    let body = cryptography::codec::to_bytes(x).unwrap();
    let sig = key.sign(&cryptography::sign::input(T::DOMAIN, &[&body]));
    Signed::from_parts(body, sig)
}

fn rec(a: &identity::Identity, group: &str, body: Body) -> Record {
    Record {
        group: group.into(),
        author: a.did().into(),
        dev: [7; 32],
        at: 1000,
        seen: vec![(1, 2)],
        body,
    }
}

// KIND: unit (G1 stage 1)
#[test]
fn records_verify_and_tampering_is_refused() {
    let a = acct();
    let r = rec(&a, "g", Body::Post { text: "hi".into(), reply_to: None });
    let bytes = r.sign(&a).to_bytes();
    assert_eq!(open_record(&bytes, "g").unwrap(), r);
    assert_eq!(open_record(&bytes, "other"), Err(RecordError::WrongGroup));
    // Any flipped byte: undecodable or a bad signature, never accepted.
    for i in 0..bytes.len() {
        let mut t = bytes.clone();
        t[i] ^= 0x41;
        assert!(open_record(&t, "g").is_err(), "byte {i}");
    }
    // Trailing junk: a second id for the same record is refused.
    let mut t = bytes.clone();
    t.push(0);
    assert!(open_record(&t, "g").is_err());
    // Claiming another author: the signature is the signer's, not the claimed DID's.
    let b = acct();
    let mut forged = rec(&a, "g", Body::Post { text: "as b".into(), reply_to: None });
    forged.author = b.did().into();
    let bytes = forge(&forged, &a).to_bytes();
    assert_eq!(open_record(&bytes, "g"), Err(RecordError::BadSignature));
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
        assert_eq!(
            open_record(&forge(&rec(&a, "g", b), &a).to_bytes(), "g"),
            Err(RecordError::Malformed)
        );
    }
    let mut r = rec(&a, "g", Body::Leave);
    r.seen = vec![(5, 1), (3, 1)];
    assert_eq!(
        open_record(&forge(&r, &a).to_bytes(), "g"),
        Err(RecordError::Malformed)
    );
}

// KIND: unit (G1 stage 1)
#[test]
fn invites_host_addrs_and_auth_verify() {
    let a = acct();
    let b = acct();
    let inv = issue_invite(&a, "g", 1, 10);
    assert_eq!(inv.open().unwrap().iss, a.did());
    let theirs = Invite {
        iss: b.did().into(),
        sub: proto::Subject::Anyone,
        iat_ms: 1,
        exp_ms: 10,
        nonce: [1; 16],
        claim: GroupJoin { group: "g".into() },
    };
    assert!(
        Signed::sign(&theirs, &a).is_err(),
        "cannot sign as another issuer"
    );
    assert!(
        forge(&theirs, &a).open().is_err(),
        "claimed issuer did not sign"
    );
    assert!(inv.open_from(b.did()).is_err());
    let mut raw = inv.to_bytes();
    *raw.last_mut().unwrap() ^= 1;
    assert!(
        Signed::<Invite>::from_bytes(&raw)
            .and_then(|s| s.open())
            .is_err()
    );

    let ha = Signed::sign(
        &HostAddr {
            group: "g".into(),
            did: a.did().into(),
            dev: [1; 32],
            endpoint: [2; 32],
            relay: None,
            at: 5,
        },
        &a,
    )
    .unwrap();
    assert!(ha.open().is_ok());
    let mut raw = ha.to_bytes();
    raw[3] ^= 1;
    assert!(
        Signed::<HostAddr>::from_bytes(&raw)
            .and_then(|s| s.open())
            .is_err()
    );

    let auth = endpoint_auth(&a, [1; 32], [9; 32]);
    assert!(verify_endpoint(&auth, &[9; 32]).is_some());
    assert!(
        verify_endpoint(&auth, &[8; 32]).is_none(),
        "made for another endpoint"
    );
    let note = auth.open().unwrap();
    let other = proto::gcall::GroupNote {
        did: b.did().into(),
        ..note
    };
    assert!(
        verify_endpoint(&forge(&other, &a), &[9; 32]).is_none(),
        "claimed DID did not sign"
    );

    let tok = InviteToken { group: "g".into(), name: "n".into(), owner: a.did().into(), invite: Some(inv), hosts: vec![ha], open: false };
    assert_eq!(cryptography::codec::from_bytes::<InviteToken>(&cryptography::codec::to_bytes(&tok).unwrap(), usize::MAX).unwrap(), tok);
}

#[test]
fn group_ids_bind_owner_and_nonce() {
    assert_ne!(group_id("did:a", &[0; 16]), group_id("did:b", &[0; 16]));
    assert_ne!(group_id("did:a", &[0; 16]), group_id("did:a", &[1; 16]));
}

// The shared envelope property table for every statement type this crate signs.
#[test]
fn core_signables_pass_the_envelope_conformance_table() {
    use crate::mail::Mail;
    use crate::{attestation::StorageAttestation, doc::DocId};
    use receipts_core::{Conversation, Kind, Receipt, Scope};
    use proto::signed::conformance::check;
    let (a, stranger) = (acct(), acct());
    check(
        &rec(
            &a,
            "g",
            Body::Post {
                text: "hi".into(),
                reply_to: None,
            },
        ),
        &a,
        &stranger,
    );
    check(
        &Invite {
            iss: a.did().into(),
            sub: proto::Subject::Anyone,
            iat_ms: 1,
            exp_ms: 10,
            nonce: [1; 16],
            claim: GroupJoin { group: "g".into() },
        },
        &a,
        &stranger,
    );
    check(
        &HostAddr {
            group: "g".into(),
            did: a.did().into(),
            dev: [1; 32],
            endpoint: [2; 32],
            relay: None,
            at: 5,
        },
        &a,
        &stranger,
    );
    check(
        &Mail {
            group: "g".into(),
            from: a.did().into(),
            to: stranger.did().into(),
            id: [4; 16],
            at: 9,
            sealed: vec![1, 2, 3],
        },
        &a,
        &stranger,
    );
    check(
        &Receipt {
            call: None,
            scope: Scope {
                conversation: Conversation::Direct("a".repeat(64)),
                day: "1970-01-01".into(),
            },
            kind: Kind::Delivered,
            recipient: a.did().into(),
            device: [1; 32],
            messages: vec![[1; 32]],
            metadata: vec![],
        },
        &a,
        &stranger,
    );
    check(
        &StorageAttestation {
            group: "a".repeat(64),
            doc: DocId::Day("2026-10-14".into()),
            host: a.did().into(),
            host_dev: [7; 32],
            stored: vec![(1, 3)],
            metadata: vec![(2, 4)],
        },
        &a,
        &stranger,
    );
}
