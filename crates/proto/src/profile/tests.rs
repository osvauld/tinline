use super::*;
#[test]
fn permit_binds_owner_hash_and_scope_and_rejects_other_domains() {
    let (a, _) = identity::generate();
    let (b, _) = identity::generate();
    let hash = "a".repeat(64);
    let blob = sign_picture_deletion(&a, &hash, 1_000).unwrap();
    assert_eq!(verify_picture_deletion(&blob, a.did()).unwrap().claim.picture_hash, hash);
    assert!(verify_picture_deletion(&blob, b.did()).is_err());
    let mut p = verify_picture_deletion(&blob, a.did()).unwrap();
    p.claim.scope = "delete-chat-history".into();
    assert!(Signed::sign(&p, &a).is_err());
    assert!(sign_picture_deletion(&a, "not-a-hash", 1_000).is_err());
    // Only a permit for anyone counts.
    let mut p = verify_picture_deletion(&blob, a.did()).unwrap();
    p.sub = Subject::Did(b.did().to_string());
    let for_b = Signed::sign(&p, &a).unwrap();
    assert_eq!(verify_picture_deletion(&for_b, a.did()).err(), Some(Error::WrongHolder));
    let p = verify_picture_deletion(&blob, a.did()).unwrap();
    crate::signed::conformance::check(&p, &a, &b);
}
