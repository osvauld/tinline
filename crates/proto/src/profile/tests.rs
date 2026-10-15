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
    p.consent = false;
    assert!(Signed::sign(&p, &a).is_err());
    p.consent = true;
    p.scope = "delete-chat-history".into();
    assert!(Signed::sign(&p, &a).is_err());
    assert!(sign_picture_deletion(&a, "not-a-hash").is_err());
    let p = verify_picture_deletion(&blob, a.did()).unwrap();
    crate::signed::conformance::check(&p, &a, &b);
}
