use super::*;
pub(super) fn sample(a: &identity::Identity, kind: Kind, group: bool) -> Receipt {
    Receipt {
        version: 1,
        call: None,
        scope: Scope {
            conversation: if group {
                Conversation::Group("a".repeat(64))
            } else {
                Conversation::Direct("a".repeat(64))
            },
            day: "1970-01-01".into(),
        },
        kind,
        recipient: a.did().into(),
        device: [1; 32],
        messages: vec![[1; 32], [3; 32]],
        metadata: if group { vec![(7, 1)] } else { vec![] },
    }
}
#[test]
fn both_domains_and_receipt_kinds_roundtrip_without_read_hole_expansion() {
    let a = identity::generate().0;
    for group in [false, true] {
        for kind in [Kind::Delivered, Kind::Read] {
            let c = sample(&a, kind, group);
            let bytes = c.sign_claim(&a).unwrap().to_bytes().unwrap();
            let opened = SignedReceipt::open(&bytes, &c.scope).unwrap();
            assert_eq!(opened, c);
            assert!(!opened.messages.contains(&[2; 32]));
        }
    }
}
#[test]
fn valid_shaped_mutations_of_every_claim_field_break_the_signature() {
    let a = identity::generate().0;
    let c = sample(&a, Kind::Delivered, true);
    let s = c.sign_claim(&a).unwrap();
    for field in 0..7 {
        let mut changed = c.clone();
        match field {
            0 => changed.kind = Kind::Read,
            1 => changed.device[0] ^= 1,
            2 => changed.recipient = identity::generate().0.did().into(),
            3 => changed.messages.push([4; 32]),
            4 => changed.metadata[0].1 = 2,
            5 => changed.scope.day = "1970-01-02".into(),
            _ => changed.scope.conversation = Conversation::Group("b".repeat(64)),
        }
        let forged = SignedReceipt {
            body: bincode::serialize(&changed).unwrap(),
            sig: s.sig.clone(),
        }
        .to_bytes()
        .unwrap();
        assert_eq!(
            SignedReceipt::open(&forged, &changed.scope),
            Err(ReceiptError::Signature)
        );
    }
    assert_eq!(
        c.sign_claim(&identity::generate().0),
        Err(ReceiptError::Signature)
    );
    let mut wrong = c.scope.clone();
    wrong.day = "1970-01-02".into();
    assert_eq!(
        SignedReceipt::open(&s.to_bytes().unwrap(), &wrong),
        Err(ReceiptError::Scope)
    );
}
#[test]
fn canonical_hash_sets_context_and_shape_are_required_before_signing() {
    let a = identity::generate().0;
    let base = sample(&a, Kind::Read, true);
    for field in 0..10 {
        let mut c = base.clone();
        match field {
            0 => c.version = 2,
            1 => c.messages.clear(),
            2 => c.messages.reverse(),
            3 => c.messages.push([3; 32]),
            4 => c.messages = vec![[1; 32]; MAX_MESSAGES + 1],
            5 => c.metadata.clear(),
            6 => c.metadata[0].1 = 0,
            7 => c.metadata.push((7, 2)),
            8 => c.scope.day = "1970-02-30".into(),
            _ => c.scope.conversation = Conversation::Group("A".repeat(64)),
        }
        assert_eq!(c.sign_claim(&a), Err(ReceiptError::Bounds));
    }
    let mut dm = sample(&a, Kind::Delivered, false);
    dm.metadata = vec![(7, 1)];
    assert_eq!(dm.sign_claim(&a), Err(ReceiptError::Bounds));
}
#[test]
fn byte_bounds_trailing_data_and_adversarial_lengths_fail_closed() {
    let a = identity::generate().0;
    let c = sample(&a, Kind::Read, false);
    let bytes = c.sign_claim(&a).unwrap().to_bytes().unwrap();
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(SignedReceipt::open(&extra, &c.scope).is_err());
    assert!(SignedReceipt::open(&bytes[..bytes.len() / 2], &c.scope).is_err());
    assert!(SignedReceipt::open(&u64::MAX.to_le_bytes(), &c.scope).is_err());
    assert_eq!(
        SignedReceipt::open(&vec![0; MAX_SIGNED + 1], &c.scope),
        Err(ReceiptError::Bounds)
    );
}
#[test]
fn host_record_and_raw_signature_domains_cannot_be_interpreted_as_recipient_proof() {
    let a = identity::generate().0;
    let c = sample(&a, Kind::Read, false);
    let body = bincode::serialize(&c).unwrap();
    for input in [body.clone(), {
        let mut v = b"tinline-group-storage-attestation-v1\0".to_vec();
        put_bytes_pub(&mut v, &body);
        v
    }] {
        let bytes = SignedReceipt {
            body: body.clone(),
            sig: a.sign(&input).to_vec(),
        }
        .to_bytes()
        .unwrap();
        assert_eq!(
            SignedReceipt::open(&bytes, &c.scope),
            Err(ReceiptError::Signature)
        );
    }
}
#[test]
fn maximum_exact_hash_batch_and_metadata_context_fit_codec_budget() {
    let a = identity::generate().0;
    let mut c = sample(&a, Kind::Delivered, true);
    c.messages = (0..MAX_MESSAGES as u64)
        .map(|n| {
            let mut h = [0; 32];
            h[..8].copy_from_slice(&n.to_be_bytes());
            h
        })
        .collect();
    c.metadata = (0..MAX_CONTEXT_WRITERS as u64).map(|p| (p, 1)).collect();
    let bytes = c.sign_claim(&a).unwrap().to_bytes().unwrap();
    assert!(bytes.len() < MAX_SIGNED);
    assert_eq!(SignedReceipt::open(&bytes, &c.scope).unwrap(), c);
    c.metadata.push((MAX_CONTEXT_WRITERS as u64, 1));
    assert_eq!(c.sign_claim(&a), Err(ReceiptError::Bounds));
}
#[test]
fn content_hash_changes_with_original_version_bytes_and_is_not_receipt_hash() {
    assert_ne!(
        message_digest(b"original signed v1"),
        message_digest(b"original signed v2")
    );
    assert_ne!(
        hex::encode(message_digest(b"signed bytes")),
        receipt_id(b"signed bytes")
    );
}
