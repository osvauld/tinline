//! Persisted adapter tests for the native bounded planner; no new wire behavior.
use super::loro_ranges::{assert_originals, persisted, post, source};
use super::*;
use crate::group::doc::batching::BatchLimits;

fn cap() -> BatchLimits {
    BatchLimits {
        bytes: 4096,
        operations: 3,
        writers: 32,
    }
}
fn drain(from: &GroupCore, to: &GroupCore, gid: &str) -> usize {
    let mut count = 0;
    while let Some(batch) = from
        .own_content_export_batch(gid, &day(), &to.doc_vv(gid, &day()), cap())
        .unwrap()
    {
        assert!(batch.update.len() <= cap().bytes);
        assert!(batch.operations <= cap().operations);
        to.own_content_apply(gid, &day(), &batch.update).unwrap();
        assert_eq!(persisted(to, gid).vv(), batch.frontier);
        count += 1;
        assert!(count < 100);
    }
    count
}

#[test]
fn bounded_native_export_from_persisted_rows_crosses_both_own_hops() {
    let f = Fixture::new();
    let a = source(&f, f.dev);
    let bdev = [6; 32];
    let b = source(&f, bdev);
    let mut last: Option<Vec<u8>> = None;
    for i in 0..18 {
        let (doc, dev) = if i % 2 == 0 { (&a, f.dev) } else { (&b, bdev) };
        if let Some(up) = &last {
            doc.import_for_test(up);
        }
        last = Some(post(&f, doc, dev, &format!("native two-hop range {i}")).1);
    }
    a.import_for_test(last.as_ref().unwrap());
    f.core
        .own_content_apply(
            &f.gid,
            &day(),
            &a.export_since(&VersionVector::default()).unwrap(),
        )
        .unwrap();
    let laptop = forwarding_proofs::sibling(&f, [4; 32]);
    let phone = forwarding_proofs::sibling(&f, [5; 32]);
    assert_eq!(drain(&f.core, &laptop, &f.gid), 6);
    assert_eq!(drain(&laptop, &phone, &f.gid), 6);
    assert_originals(&a, &persisted(&phone, &f.gid));
    assert!(persisted(&laptop, &f.gid).vv().get(&laptop.wid).is_none());
    assert!(persisted(&phone, &f.gid).vv().get(&phone.wid).is_none());
}

#[test]
fn failed_append_does_not_advance_bounded_export_and_retry_repairs_both_hops() {
    let f = Fixture::new();
    let doc = source(&f, f.dev);
    for i in 0..6 {
        post(&f, &doc, f.dev, &format!("durable range {i}"));
    }
    f.core
        .own_content_apply(
            &f.gid,
            &day(),
            &doc.export_since(&VersionVector::default()).unwrap(),
        )
        .unwrap();
    let laptop = forwarding_proofs::sibling(&f, [4; 32]);
    let phone = forwarding_proofs::sibling(&f, [5; 32]);
    let first = f
        .core
        .own_content_export_batch(&f.gid, &day(), &VersionVector::default(), cap())
        .unwrap()
        .unwrap();
    laptop
        .own_content_apply(&f.gid, &day(), &first.update)
        .unwrap();
    drain(&laptop, &phone, &f.gid);
    let before = laptop.doc_vv(&f.gid, &day());
    let later = f
        .core
        .own_content_export_batch(&f.gid, &day(), &before, cap())
        .unwrap()
        .unwrap();
    laptop.store.fail_next_append_for_test();
    assert!(matches!(
        laptop.own_content_apply(&f.gid, &day(), &later.update),
        Err(Error::Io(_))
    ));
    assert_eq!(persisted(&laptop, &f.gid).vv(), before);
    assert!(
        laptop
            .own_content_export_batch(&f.gid, &day(), &phone.doc_vv(&f.gid, &day()), cap())
            .unwrap()
            .is_none()
    );
    laptop
        .own_content_apply(&f.gid, &day(), &later.update)
        .unwrap();
    drain(&laptop, &phone, &f.gid);
    assert_originals(&doc, &persisted(&phone, &f.gid));
}

#[test]
fn bounded_export_preserves_document_scope_and_unknown_group_guards() {
    let f = Fixture::new();
    let core = forwarding_proofs::sibling(&f, [4; 32]);
    assert!(
        core.own_content_export_batch(
            &f.gid,
            &DocId::Day("not-a-day".into()),
            &VersionVector::default(),
            cap()
        )
        .is_err()
    );
    assert!(
        core.own_content_export_batch(&"0".repeat(64), &day(), &VersionVector::default(), cap())
            .unwrap()
            .is_none()
    );
    let doc = source(&f, f.dev);
    post(&f, &doc, f.dev, "stored original");
    core.own_content_apply(
        &f.gid,
        &day(),
        &doc.export_since(&VersionVector::default()).unwrap(),
    )
    .unwrap();
    let tiny = BatchLimits { bytes: 1, ..cap() };
    assert!(
        core.own_content_export_batch(&f.gid, &day(), &VersionVector::default(), tiny)
            .is_err()
    );
    assert_originals(&doc, &persisted(&core, &f.gid));
}
