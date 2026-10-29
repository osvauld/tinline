use super::*;

#[test]
fn concurrent_old_picture_consent_does_not_prevent_profile_convergence() {
    let (owner, _) = identity::generate();
    let a = ProfileDoc::new(&[1; 32]);
    let b = ProfileDoc::new(&[2; 32]);
    let hash = "a".repeat(64);
    for i in 0..8 {
        a.set("clock", json!(i)).unwrap();
    }
    a.set(
        "picture",
        json!({"hash":hash,"key":"b".repeat(64),"content_hash":"c".repeat(64),"size":80}),
    )
    .unwrap();
    b.set(
        "picture",
        json!({"hash":"d".repeat(64),"key":"e".repeat(64),"content_hash":"f".repeat(64),"size":80}),
    )
    .unwrap();
    b.request_deletion(
        &hash,
        &proto::profile::sign_picture_deletion(&owner, &hash, 1_000).unwrap(),
    )
    .unwrap();
    validate(&a, owner.did()).unwrap();
    validate(&b, owner.did()).unwrap();
    let a_before = a.snapshot();
    let b_before = b.snapshot();
    a.import(&b_before).unwrap();
    b.import(&a_before).unwrap();
    assert_eq!(a.fields().unwrap(), b.fields().unwrap());
    assert_eq!(picture(&a.fields().unwrap()).unwrap().unwrap().hash, hash);
    validate(&a, owner.did())
        .expect("current picture protection must defer consent, not reject the merged profile");
    validate(&b, owner.did()).unwrap();
    let db = storage::Sealed::new(storage::Store::open_in_memory().unwrap(), [7; 32]);
    let marker = picture_marker(owner.did(), &hash);
    db.put(&marker, b"ready").unwrap();
    db.apply(&metadata_ops(&db, owner.did(), &a).unwrap())
        .unwrap();
    assert_eq!(
        db.get(&marker).unwrap().unwrap(),
        b"ready",
        "current picture remains protected"
    );
    a.remove("picture").unwrap();
    db.apply(&metadata_ops(&db, owner.did(), &a).unwrap())
        .unwrap();
    assert!(
        db.get(&marker).unwrap().is_none(),
        "consent applies once the hash is no longer selected"
    );
}

#[test]
fn deletion_validation_binds_owner_and_defers_current_picture_cleanup() {
    let (a, _) = identity::generate();
    let (b, _) = identity::generate();
    let doc = ProfileDoc::new(&[1; 32]);
    let hash = "a".repeat(64);
    doc.set(
        "picture",
        json!({"hash":hash,"key":"b".repeat(64),"content_hash":"c".repeat(64),"size":80}),
    )
    .unwrap();
    let before = doc.snapshot();
    let staged = ProfileDoc::load(&[1; 32], Some(&before)).unwrap();
    staged
        .request_deletion(
            &hash,
            &proto::profile::sign_picture_deletion(&a, &hash, 1_000).unwrap(),
        )
        .unwrap();
    validate(&staged, a.did()).unwrap();
    let db = storage::Sealed::new(storage::Store::open_in_memory().unwrap(), [7; 32]);
    let marker = picture_marker(a.did(), &hash);
    db.put(&marker, b"ready").unwrap();
    db.apply(&metadata_ops(&db, a.did(), &staged).unwrap())
        .unwrap();
    assert_eq!(
        db.get(&marker).unwrap().unwrap(),
        b"ready",
        "cannot delete a current picture"
    );
    assert_eq!(doc.snapshot(), before);
    staged.remove("picture").unwrap();
    assert!(
        validate(&staged, b.did()).is_err(),
        "another owner cannot reuse this permit"
    );
    assert!(validate(&staged, a.did()).is_ok());
}
#[test]
fn deletion_only_removes_the_signing_owners_namespace_even_for_shared_hashes() {
    let (a, _) = identity::generate();
    let (b, _) = identity::generate();
    let db = storage::Sealed::new(storage::Store::open_in_memory().unwrap(), [7; 32]);
    let hash = "a".repeat(64);
    let a_key = picture_marker(a.did(), &hash);
    let b_key = picture_marker(b.did(), &hash);
    db.put(&a_key, b"ready").unwrap();
    db.put(&b_key, b"ready").unwrap();
    let doc = ProfileDoc::new(&[1; 32]);
    doc.request_deletion(
        &hash,
        &proto::profile::sign_picture_deletion(&a, &hash, 1_000).unwrap(),
    )
    .unwrap();
    validate(&doc, a.did()).unwrap();
    db.apply(&metadata_ops(&db, a.did(), &doc).unwrap())
        .unwrap();
    assert!(db.get(&a_key).unwrap().is_none());
    assert_eq!(db.get(&b_key).unwrap().unwrap(), b"ready");
}
#[test]
fn contact_cache_preserves_unknown_profile_data_without_a_second_signature_layer() {
    let (owner, _) = identity::generate();
    let cache = ProfileCache::new(&[1; 32]);
    let doc = ProfileDoc::new(&[2; 32]);
    doc.set("example:data", json!({"nested":[1,true,"x"]}))
        .unwrap();
    cache.set(owner.did(), &doc).unwrap();
    let reloaded = ProfileCache::load(&[3; 32], Some(&cache.snapshot())).unwrap();
    assert_eq!(
        reloaded
            .profile(owner.did(), &[3; 32])
            .unwrap()
            .unwrap()
            .fields()
            .unwrap(),
        doc.fields().unwrap()
    );
}
#[test]
fn recovery_keeps_the_hub_tags_of_my_and_my_contacts_cached_pictures() {
    // Regression (2026-10-29): markers and hub tags hash the owner under different domains, so
    // recovery matched nothing and the GC took every cached picture on each start.
    let (me, _) = identity::generate();
    let (contact, _) = identity::generate();
    let (stranger, _) = identity::generate();
    let hash = "a".repeat(64);
    let markers = [&me, &contact, &stranger].map(|id| picture_marker(id.did(), &hash));
    let tags = marker_tags(markers, [me.did().to_string(), contact.did().to_string()]);
    let parsed: iroh_blobs::Hash = hash.parse().unwrap();
    let want = [me.did(), contact.did()]
        .map(|did| transfer::BlobHub::owned_tag(did, &parsed))
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(tags, want);
}
