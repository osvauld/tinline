//! Pure cryptographic codec tests. Runtime persisted-role tests live with the
//! actual engine/store fixtures in engine/durability/storage_attestations.rs.
use super::*;

fn claim(account: &identity::Identity) -> StorageAttestation {
    StorageAttestation {
        version: 1,
        group: "a".repeat(64),
        doc: DocId::Day("2026-10-14".into()),
        host: account.did().into(),
        host_dev: [7; 32],
        stored: vec![(1, 3), (9, 2)],
        metadata: vec![(2, 4)],
    }
}
fn unchecked_bytes(claim: &StorageAttestation, account: &identity::Identity) -> Vec<u8> {
    let body = cryptography::codec::to_bytes(claim).unwrap();
    SignedStorageAttestation {
        sig: account.sign(&sig_input(&body)).to_vec(),
        body,
    }
    .to_bytes()
    .unwrap()
}

#[test]
fn canonical_receipt_roundtrip_and_all_signed_fields_are_bound() {
    let account = identity::generate().0;
    let c = claim(&account);
    let proof = c.sign(&account).unwrap();
    let bytes = proof.to_bytes().unwrap();
    assert_eq!(
        SignedStorageAttestation::open(&bytes, &c.group, &c.doc),
        Ok(c.clone())
    );
    let other = identity::generate().0;
    let mut variants = Vec::new();
    let mut changed = c.clone();
    changed.group = "b".repeat(64);
    variants.push(changed);
    let mut changed = c.clone();
    changed.doc = DocId::Day("2026-10-15".into());
    variants.push(changed);
    let mut changed = c.clone();
    changed.host = other.did().into();
    variants.push(changed);
    let mut changed = c.clone();
    changed.host_dev[0] ^= 1;
    variants.push(changed);
    let mut changed = c.clone();
    changed.stored[0].1 += 1;
    variants.push(changed);
    let mut changed = c.clone();
    changed.metadata[0].1 += 1;
    variants.push(changed);
    for changed in variants {
        let tampered = SignedStorageAttestation {
            body: cryptography::codec::to_bytes(&changed).unwrap(),
            sig: proof.sig.clone(),
        }
        .to_bytes()
        .unwrap();
        assert_eq!(
            SignedStorageAttestation::open(&tampered, &changed.group, &changed.doc),
            Err(AttestationError::Signature)
        );
    }
    assert_eq!(
        SignedStorageAttestation::open(&bytes, &"b".repeat(64), &c.doc),
        Err(AttestationError::Scope)
    );
    assert_eq!(
        SignedStorageAttestation::open(&bytes, &c.group, &DocId::Meta),
        Err(AttestationError::Scope)
    );
}

#[test]
fn wrong_key_raw_signature_and_record_domain_are_not_storage_receipts() {
    let account = identity::generate().0;
    let other = identity::generate().0;
    let c = claim(&account);
    let body = cryptography::codec::to_bytes(&c).unwrap();
    assert_eq!(c.sign(&other), Err(AttestationError::Signature));
    let record_input = cryptography::sign::input(cryptography::domains::GROUP_RECORD, &[&body]);
    for signature in [
        account.sign(&body),
        other.sign(&sig_input(&body)),
        account.sign(&record_input),
    ] {
        let bytes = SignedStorageAttestation {
            body: body.clone(),
            sig: signature.to_vec(),
        }
        .to_bytes()
        .unwrap();
        assert_eq!(
            SignedStorageAttestation::open(&bytes, &c.group, &c.doc),
            Err(AttestationError::Signature)
        );
    }
}

#[test]
fn unsupported_versions_bad_scope_and_frontier_aliases_are_refused() {
    let account = identity::generate().0;
    let c = claim(&account);
    let mut variants = Vec::new();
    let mut bad = c.clone();
    bad.version = 2;
    variants.push(bad);
    let mut bad = c.clone();
    bad.group = "A".repeat(64);
    variants.push(bad);
    let mut bad = c.clone();
    bad.doc = DocId::Day("2026-02-30".into());
    variants.push(bad);
    let mut bad = c.clone();
    bad.stored.clear();
    variants.push(bad);
    let mut bad = c.clone();
    bad.metadata.clear();
    variants.push(bad);
    for counter in [0, -1] {
        let mut bad = c.clone();
        bad.stored[0].1 = counter;
        variants.push(bad);
    }
    let mut bad = c.clone();
    bad.stored.reverse();
    variants.push(bad);
    let mut bad = c.clone();
    bad.stored.push((9, 7));
    variants.push(bad);
    let mut bad = c.clone();
    bad.doc = DocId::Meta;
    variants.push(bad);
    let mut bad = c.clone();
    bad.stored = (0..4097).map(|p| (p, 1)).collect();
    variants.push(bad);
    for bad in variants {
        let bytes = unchecked_bytes(&bad, &account);
        assert_eq!(
            SignedStorageAttestation::open(&bytes, &bad.group, &bad.doc),
            Err(AttestationError::Bounds)
        );
    }
    let mut meta = c.clone();
    meta.doc = DocId::Meta;
    meta.stored = meta.metadata.clone();
    assert!(meta.sign(&account).is_ok());
}

#[test]
fn trailing_bytes_truncation_oversize_and_adversarial_lengths_are_refused() {
    let account = identity::generate().0;
    let c = claim(&account);
    let mut proof = c.sign(&account).unwrap();
    let bytes = proof.to_bytes().unwrap();
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert_eq!(
        SignedStorageAttestation::open(&trailing, &c.group, &c.doc),
        Err(AttestationError::Encoding)
    );
    for end in [0, 1, bytes.len() / 2, bytes.len() - 1] {
        assert!(SignedStorageAttestation::open(&bytes[..end], &c.group, &c.doc).is_err());
    }
    proof.body.push(0);
    proof.sig = account.sign(&sig_input(&proof.body)).to_vec();
    assert_eq!(
        SignedStorageAttestation::open(&proof.to_bytes().unwrap(), &c.group, &c.doc),
        Err(AttestationError::Encoding)
    );
    assert_eq!(
        SignedStorageAttestation::open(&vec![0; MAX_SIGNED + 1], &c.group, &c.doc),
        Err(AttestationError::Bounds)
    );
    assert!(SignedStorageAttestation::open(&u64::MAX.to_le_bytes(), &c.group, &c.doc).is_err());
    proof.sig.pop();
    assert_eq!(proof.to_bytes(), Err(AttestationError::Bounds));
}

#[test]
fn maximum_writer_frontiers_fit_without_silently_truncating() {
    let account = identity::generate().0;
    let mut c = claim(&account);
    c.stored = (0..MAX_WRITERS as u64).map(|p| (p, i32::MAX)).collect();
    c.metadata = c.stored.clone();
    let bytes = c.sign(&account).unwrap().to_bytes().unwrap();
    assert!(bytes.len() < MAX_SIGNED);
    assert_eq!(
        SignedStorageAttestation::open(&bytes, &c.group, &c.doc),
        Ok(c)
    );
}
