//! Clean sealed-redb reopen, not power loss/process-kill/fsync validation.
use super::*;
use super::{metadata_obligations::disk_core, reader::TestDir, storage_attestations::host_fixture};
use crate::group::attestation::persistence::digest;

#[test]
fn original_receipts_projections_and_only_covered_debt_survive_reopen_and_retry() {
    let directory = TestDir::new();
    let path = directory.0.join("receipt-restart.redb");
    let (f, host) = host_fixture();
    let core = disk_core(&path, &f.author);
    core.groups.lock().insert(
        f.gid.clone(),
        Live::empty(&f.gid, core.wid, GroupMeta::default()),
    );
    core.own_content_apply(
        &f.gid,
        &DocId::Meta,
        &f.meta.export_since(&VersionVector::default()).unwrap(),
    )
    .unwrap();
    let update = f.post_update();
    core.own_content_apply(&f.gid, &day(), &update).unwrap();
    host.apply(&f.gid, &day(), &update, |_, _, _| Ok(()))
        .unwrap();
    let bytes = host
        .issue_storage_attestation(&f.gid, &day())
        .unwrap()
        .to_bytes();
    let meta_bytes = host
        .issue_storage_attestation(&f.gid, &DocId::Meta)
        .unwrap()
        .to_bytes();
    core.store.fail_next_meta_write_for_test();
    assert!(
        core.accept_storage_attestation(&bytes, &f.gid, &day())
            .is_err()
    );
    core.shut();
    drop(core);

    let recovered = disk_core(&path, &f.author);
    assert!(
        recovered
            .store
            .storage_attestation(&f.gid, &digest(&bytes))
            .unwrap()
            .is_none()
    );
    assert!(
        recovered
            .store
            .acks_day(&f.gid, "1970-01-01")
            .unwrap()
            .is_empty()
    );
    assert!(
        recovered.groups.lock()[&f.gid]
            .meta
            .owed_days
            .contains("1970-01-01")
    );
    assert!(
        recovered
            .accept_storage_attestation(&bytes, &f.gid, &day())
            .unwrap()
    );
    // A day confirmation cannot silently clear metadata obligations.
    assert!(
        !recovered.groups.lock()[&f.gid]
            .meta
            .own_owed_meta
            .is_empty()
    );
    assert!(
        recovered
            .accept_storage_attestation(&meta_bytes, &f.gid, &DocId::Meta)
            .unwrap()
    );
    recovered.shut();
    drop(recovered);

    let final_core = disk_core(&path, &f.author);
    assert_eq!(
        final_core
            .store
            .storage_attestation(&f.gid, &digest(&bytes))
            .unwrap()
            .unwrap()
            .bytes,
        bytes
    );
    assert_eq!(
        final_core
            .store
            .storage_attestation(&f.gid, &digest(&meta_bytes))
            .unwrap()
            .unwrap()
            .bytes,
        meta_bytes
    );
    assert!(
        !final_core.groups.lock()[&f.gid]
            .meta
            .owed_days
            .contains("1970-01-01")
    );
    assert!(
        final_core.groups.lock()[&f.gid]
            .meta
            .own_owed_meta
            .is_empty()
    );
    assert_eq!(
        final_core
            .store
            .acks_day(&f.gid, "1970-01-01")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(final_core.store.acks(&f.gid).unwrap().len(), 1);
    let first = final_core.storage_attestation_page(&f.gid, None).unwrap();
    let second = final_core
        .storage_attestation_page(&f.gid, Some(&digest(&first[0].1)))
        .unwrap();
    assert_eq!((first.len(), second.len()), (1, 1));
    assert!(
        final_core
            .storage_attestation_page(&f.gid, Some(&digest(&second[0].1)))
            .unwrap()
            .is_empty()
    );
    assert!(
        !final_core
            .accept_storage_attestation(&bytes, &f.gid, &day())
            .unwrap()
    );
    assert!(
        !final_core
            .accept_storage_attestation(&meta_bytes, &f.gid, &DocId::Meta)
            .unwrap()
    );
    final_core.shut();
}
