use super::*;
#[test]
fn permit_binds_owner_hash_consent_and_scope_and_rejects_other_domains() {
    let (a, _) = identity::generate();
    let (b, _) = identity::generate();
    let hash = "a".repeat(64);
    let blob = sign_picture_deletion(&a, &hash).unwrap();
    assert_eq!(
        verify_picture_deletion(&blob, a.did())
            .unwrap()
            .picture_hash,
        hash
    );
    assert!(verify_picture_deletion(&blob, b.did()).is_err());
    let mut p = verify_picture_deletion(&blob, a.did()).unwrap();
    p.picture_hash = "b".repeat(64);
    let tampered = SignedBlob {
        payload: crate::enc(serde_json::to_vec(&p).unwrap()),
        ..blob.clone()
    };
    assert!(verify_picture_deletion(&tampered, a.did()).is_err());
    p.consent = false;
    assert!(verify_picture_deletion(&sign_blob(&a, DOMAIN, &p), a.did()).is_err());
    p.consent = true;
    p.scope = "delete-chat-history".into();
    assert!(verify_picture_deletion(&sign_blob(&a, DOMAIN, &p), a.did()).is_err());
    assert!(
        verify_picture_deletion(&crate::sign_block_notice(&a, b.did(), true, 1), a.did()).is_err()
    );
    assert!(sign_picture_deletion(&a, "not-a-hash").is_err());
}
