//! Contents-adapter primitives; transport assertions live in sync/engine/group_contents/wire_tests.rs.
use super::*;
use crate::own::contents::{MAX_BATCH, doc_name, parse_doc};

#[test]
fn own_group_document_names_are_scoped_and_canonical() {
    let f = Fixture::new();
    for doc in [DocId::Meta, day()] {
        assert_eq!(
            parse_doc(&doc_name(&f.gid, &doc)),
            Some((f.gid.clone(), doc))
        );
    }
    for suffix in [
        "../meta",
        "2026-02-30",
        "1970-1-1",
        "meta/extra",
        "",
        "/meta",
    ] {
        assert!(parse_doc(&format!("group/{}/{suffix}", f.gid)).is_none());
    }
    assert!(parse_doc("group/not-a-group/meta").is_none());
    assert!(parse_doc(&format!("dm/{}/meta", f.gid)).is_none());
    assert!(parse_doc(&format!("group/{}/meta", "A".repeat(64))).is_none());
}

#[test]
fn own_group_export_never_serves_failed_unpersisted_import() {
    let f = Fixture::new();
    let before = f.durable_meta_vv();
    let update = f.meta_update();
    f.core.store.fail_next_append_for_test();
    assert!(matches!(
        f.core.own_content_apply(&f.gid, &DocId::Meta, &update),
        Err(Error::Io(_))
    ));
    let (export, vv) = f
        .core
        .own_content_export(&f.gid, &DocId::Meta, &VersionVector::default())
        .unwrap()
        .unwrap();
    assert_eq!(VersionVector::decode(&vv).unwrap(), before);
    let mut checked = GroupDoc::new(&f.gid, f.core.wid);
    checked.apply_remote(&export, |_| Ok(())).unwrap();
    assert_eq!(checked.entries().unwrap().len(), 1);
    assert_eq!(checked.vv(), before);
}

#[test]
fn own_group_import_bound_and_unknown_scope_fail_before_mutation() {
    let f = Fixture::new();
    let before = f.durable_meta_vv();
    assert!(matches!(
        f.core
            .own_content_apply(&f.gid, &DocId::Meta, &vec![0; MAX_BATCH + 1]),
        Err(Error::Protocol(_))
    ));
    let other = Fixture::new();
    let update = other.meta_update();
    assert!(matches!(
        f.core.own_content_apply(&other.gid, &DocId::Meta, &update),
        Err(Error::Protocol(_))
    ));
    assert_eq!(f.core.store.groups().unwrap().len(), 1);
    assert_eq!(f.durable_meta_vv(), before);
    assert!(
        f.core
            .own_content_export(&other.gid, &DocId::Meta, &VersionVector::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn own_group_import_export_reconciles_without_reauthoring_or_duplicate_receipt() {
    let f = Fixture::new();
    let update = f.post_update();
    assert!(
        f.core
            .own_content_apply(&f.gid, &day(), &update)
            .unwrap()
            .is_some()
    );
    let (export, vv) = f
        .core
        .own_content_export(&f.gid, &day(), &VersionVector::default())
        .unwrap()
        .unwrap();
    let mut verifier = GroupDoc::new_in(&f.gid, day(), f.core.wid);
    verifier.apply_remote(&export, |_| Ok(())).unwrap();
    let entries = verifier.entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].rec.author, f.author.did());
    assert_eq!(entries[0].rec.dev, f.dev);
    assert_eq!(verifier.vv(), VersionVector::decode(&vv).unwrap());
    assert!(
        f.core
            .own_content_export(&f.gid, &day(), &verifier.vv())
            .unwrap()
            .is_none()
    );
    assert!(
        f.core
            .own_content_apply(&f.gid, &day(), &export)
            .unwrap()
            .is_none()
    );
}

#[test]
fn own_group_inventory_obeys_caller_budget_without_claiming_archived_history() {
    let mut f = Fixture::new();
    // Inventory eligibility is account-level membership. This fixture uses the
    // creator DID; production GroupCore obtains that DID from the active account.
    f.core.did = f.author.did().into();
    let update = f.post_update();
    f.core.own_content_apply(&f.gid, &day(), &update).unwrap();
    assert!(f.core.own_content_docs(0).is_empty());
    let docs = f.core.own_content_docs(1);
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].0, doc_name(&f.gid, &DocId::Meta));
    // The 1970 fixture day remains exportable but is not offered as a recent day.
    assert_eq!(f.core.own_content_docs(100).len(), 1);
    assert!(
        f.core
            .own_content_export(&f.gid, &day(), &VersionVector::default())
            .unwrap()
            .is_some()
    );
}
