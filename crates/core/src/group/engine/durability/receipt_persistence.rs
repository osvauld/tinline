//! Receipt transactions use actual sealed stores; failures precede commit.
use super::storage_attestations::{host_fixture, import_meta, update_role};
use super::*;
use crate::group::attestation::{AttestationError as AE, persistence::digest};

#[test]
fn receipt_original_index_and_day_debt_commit_atomically_and_retry() {
    let (f, host) = host_fixture();
    let phone = forwarding_proofs::sibling(&f, [4; 32]);
    let update = f.post_update();
    host.apply(&f.gid, &day(), &update, |_, _, _| Ok(()))
        .unwrap();
    phone.own_content_apply(&f.gid, &day(), &update).unwrap();
    let bytes = host
        .issue_storage_attestation(&f.gid, &day())
        .unwrap()
        .to_bytes();
    for metadata_failure in [false, true] {
        if metadata_failure {
            phone.store.fail_next_meta_write_for_test();
        } else {
            phone.store.fail_next_append_for_test();
        }
        assert!(matches!(
            phone.accept_storage_attestation(&bytes, &f.gid, &day()),
            Err(AE::Store(_))
        ));
        assert!(
            phone
                .store
                .storage_attestation(&f.gid, &digest(&bytes))
                .unwrap()
                .is_none()
        );
        assert!(
            phone
                .store
                .acks_day(&f.gid, "1970-01-01")
                .unwrap()
                .is_empty()
        );
        assert!(
            phone.groups.lock()[&f.gid]
                .meta
                .owed_days
                .contains("1970-01-01")
        );
        assert!(
            phone
                .store
                .groups()
                .unwrap()
                .into_iter()
                .find(|(g, _)| g == &f.gid)
                .unwrap()
                .1
                .owed_days
                .contains("1970-01-01")
        );
    }
    assert!(
        phone
            .accept_storage_attestation(&bytes, &f.gid, &day())
            .unwrap()
    );
    assert_eq!(
        phone
            .store
            .storage_attestation(&f.gid, &digest(&bytes))
            .unwrap()
            .unwrap()
            .bytes,
        bytes
    );
    assert!(
        !phone.groups.lock()[&f.gid]
            .meta
            .owed_days
            .contains("1970-01-01")
    );
    assert_eq!(phone.store.acks_day(&f.gid, "1970-01-01").unwrap().len(), 1);
    assert!(
        !phone
            .accept_storage_attestation(&bytes, &f.gid, &day())
            .unwrap()
    );
}

#[test]
fn partial_proof_never_clears_newer_day_debt() {
    let (f, host) = host_fixture();
    let phone = forwarding_proofs::sibling(&f, [4; 32]);
    let first = f.post_update();
    host.apply(&f.gid, &day(), &first, |_, _, _| Ok(()))
        .unwrap();
    let bytes = host
        .issue_storage_attestation(&f.gid, &day())
        .unwrap()
        .to_bytes();
    let source = GroupDoc::load_in(
        &f.gid,
        day(),
        writer_id(f.author.did(), &f.dev),
        None,
        &[first.clone()],
    )
    .unwrap();
    let rec = Record {
        group: f.gid.clone(),
        author: f.author.did().into(),
        dev: f.dev,
        at: 1001,
        seen: f.meta.seen(),
        body: Body::Post {
            text: "newer debt".into(),
            reply_to: None,
        },
    };
    let (_, second) = source.write(&f.author, &rec).unwrap();
    phone.own_content_apply(&f.gid, &day(), &first).unwrap();
    phone.own_content_apply(&f.gid, &day(), &second).unwrap();
    assert!(
        phone
            .accept_storage_attestation(&bytes, &f.gid, &day())
            .unwrap()
    );
    assert!(
        phone.groups.lock()[&f.gid]
            .meta
            .owed_days
            .contains("1970-01-01")
    );
    host.apply(&f.gid, &day(), &second, |_, _, _| Ok(()))
        .unwrap();
    let full = host
        .issue_storage_attestation(&f.gid, &day())
        .unwrap()
        .to_bytes();
    phone
        .accept_storage_attestation(&full, &f.gid, &day())
        .unwrap();
    assert!(
        !phone.groups.lock()[&f.gid]
            .meta
            .owed_days
            .contains("1970-01-01")
    );
    // Original historical evidence is retained, not replaced by an unsigned union.
    assert!(
        phone
            .store
            .storage_attestation(&f.gid, &digest(&bytes))
            .unwrap()
            .is_some()
    );
}

#[test]
fn first_acceptance_checks_revocation_but_known_original_remains_historical() {
    let (mut f, host) = host_fixture();
    let phone = forwarding_proofs::sibling(&f, [4; 32]);
    let other = forwarding_proofs::sibling(&f, [5; 32]);
    let update = f.post_update();
    host.apply(&f.gid, &day(), &update, |_, _, _| Ok(()))
        .unwrap();
    let bytes = host
        .issue_storage_attestation(&f.gid, &day())
        .unwrap()
        .to_bytes();
    phone
        .accept_storage_attestation(&bytes, &f.gid, &day())
        .unwrap();
    update_role(&mut f, &host, false);
    import_meta(&f, &phone);
    import_meta(&f, &other);
    assert_eq!(
        other.accept_storage_attestation(&bytes, &f.gid, &day()),
        Err(AE::Authority)
    );
    assert!(
        !phone
            .accept_storage_attestation(&bytes, &f.gid, &day())
            .unwrap()
    );
    phone.own_content_apply(&f.gid, &day(), &update).unwrap();
    assert!(
        phone
            .accept_storage_attestation(&bytes, &f.gid, &day())
            .unwrap()
    );
    assert!(
        !phone.groups.lock()[&f.gid]
            .meta
            .owed_days
            .contains("1970-01-01")
    );
}

#[test]
fn metadata_debt_clears_only_after_original_receipt_transaction_and_legacy_acks_are_not_forwarded()
{
    let (f, host) = host_fixture();
    let phone = forwarding_proofs::sibling(&f, [4; 32]);
    let update = f.meta_update();
    host.apply(&f.gid, &DocId::Meta, &update, |_, _, _| Ok(()))
        .unwrap();
    phone
        .own_content_apply(&f.gid, &DocId::Meta, &update)
        .unwrap();
    assert!(!phone.groups.lock()[&f.gid].meta.own_owed_meta.is_empty());
    phone
        .store
        .put_ack(
            &f.gid,
            &host.did,
            &host.dev,
            &host.doc_vv(&f.gid, &DocId::Meta).encode(),
        )
        .unwrap();
    assert!(
        phone
            .storage_attestation_page(&f.gid, None)
            .unwrap()
            .is_empty()
    );
    let bytes = host
        .issue_storage_attestation(&f.gid, &DocId::Meta)
        .unwrap()
        .to_bytes();
    phone.store.fail_next_append_for_test();
    assert!(
        phone
            .accept_storage_attestation(&bytes, &f.gid, &DocId::Meta)
            .is_err()
    );
    assert!(!phone.groups.lock()[&f.gid].meta.own_owed_meta.is_empty());
    phone
        .accept_storage_attestation(&bytes, &f.gid, &DocId::Meta)
        .unwrap();
    assert!(phone.groups.lock()[&f.gid].meta.own_owed_meta.is_empty());
    assert_eq!(
        phone.storage_attestation_page(&f.gid, None).unwrap()[0].1,
        bytes
    );
    assert!(
        phone
            .storage_attestation_page(&f.gid, Some(&digest(&bytes)))
            .unwrap()
            .is_empty()
    );
}
