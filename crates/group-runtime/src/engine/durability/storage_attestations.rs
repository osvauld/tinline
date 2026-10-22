//! Real native docs, signed hosting records and sealed persistence, not wire tests.
use super::*;
#[cfg(test)]
use crate::attestation::{
    AttestationError as AE, SignedStorageAttestation, open_attestation,
};
use proto::Signed;

pub(super) fn host_fixture() -> (Fixture, GroupCore) {
    host_fixture_for(Fixture::new())
}
pub(super) fn host_fixture_for(mut f: Fixture) -> (Fixture, GroupCore) {
    let account = identity::generate().0;
    let dev = [7; 32];
    let permit = Record {
        group: f.gid.clone(),
        author: f.author.did().into(),
        dev: f.dev,
        at: 1,
        seen: f.meta.seen(),
        body: Body::HostPermit {
            host: account.did().into(),
            host_dev: dev,
            active: true,
        },
    };
    f.meta.write(&f.author, &permit).unwrap();
    let host_doc = GroupDoc::load(
        &f.gid,
        writer_id(account.did(), &dev),
        Some(&f.meta.snapshot()),
        &[],
    )
    .unwrap();
    let invite = Signed::sign(
        &Invite {
            iss: f.author.did().into(),
            sub: proto::Subject::Anyone,
            iat_ms: 1,
            exp_ms: 1000,
            nonce: [8; 16],
            claim: GroupJoin { group: f.gid.clone() },
        },
        &f.author,
    )
    .unwrap();
    let join = Record {
        group: f.gid.clone(),
        author: account.did().into(),
        dev,
        at: 2,
        seen: host_doc.seen(),
        body: Body::Join {
            invite,
            name: "synthetic independent host".into(),
        },
    };
    host_doc.write(&account, &join).unwrap();
    let accept = Record {
        body: Body::HostAccept { active: true },
        seen: host_doc.seen(),
        at: 3,
        ..join
    };
    host_doc.write(&account, &accept).unwrap();
    f.meta
        .apply_remote(&host_doc.export_since(&f.meta.vv()).unwrap(), |_| Ok(()))
        .unwrap();
    let mut core = receiver();
    core.account = account.clone();
    core.did = account.did().into();
    core.dev = dev;
    core.wid = writer_id(&core.did, &dev);
    core.groups.lock().insert(
        f.gid.clone(),
        Live::empty(&f.gid, core.wid, GroupMeta::default()),
    );
    core.apply(
        &f.gid,
        &DocId::Meta,
        &f.meta.export_since(&VersionVector::default()).unwrap(),
        |_, _, _| Ok(()),
    )
    .unwrap();
    assert!(
        core.groups.lock()[&f.gid]
            .state
            .is_host(&core.did, &core.dev)
    );
    (f, core)
}
pub(super) fn update_role(f: &mut Fixture, core: &GroupCore, active: bool) {
    let rec = Record {
        group: f.gid.clone(),
        author: f.author.did().into(),
        dev: f.dev,
        at: 100,
        seen: f.meta.seen(),
        body: Body::HostPermit {
            host: core.did.clone(),
            host_dev: core.dev,
            active,
        },
    };
    f.meta.write(&f.author, &rec).unwrap();
}
pub(super) fn import_meta(f: &Fixture, core: &GroupCore) {
    let update = f
        .meta
        .export_since(&core.doc_vv(&f.gid, &DocId::Meta))
        .unwrap();
    core.apply(&f.gid, &DocId::Meta, &update, |_, _, _| Ok(()))
        .unwrap();
}

#[test]
fn independent_host_attests_persisted_originals_and_two_own_replicas_verify_unchanged_proof() {
    let (f, host) = host_fixture();
    let phone = forwarding_proofs::sibling(&f, [4; 32]);
    let laptop = forwarding_proofs::sibling(&f, [5; 32]);
    let update = f.post_update();
    host.apply(&f.gid, &day(), &update, |_, _, _| Ok(()))
        .unwrap();
    let proof = host
        .issue_storage_attestation(&f.gid, &day())
        .unwrap()
        .to_bytes();
    let via_laptop = proof.clone(); // relay opaque original bytes, not a laptop re-signing
    let checked = laptop
        .verify_storage_attestation(&via_laptop, &f.gid, &day())
        .unwrap();
    assert_eq!(
        phone
            .verify_storage_attestation(&proof, &f.gid, &day())
            .unwrap(),
        checked
    );
    assert_eq!(checked.host, host.did);
    assert_ne!(checked.host, phone.did);
    let (snap, updates) = host.store.shard_rows(&f.gid, "1970-01-01").unwrap();
    let persisted = GroupDoc::load_in(&f.gid, day(), host.wid, snap.as_deref(), &updates).unwrap();
    assert_eq!(checked.stored, persisted.seen());
    assert_eq!(persisted.entries().unwrap()[0].rec.author, f.author.did());
    assert_eq!(
        host.issue_storage_attestation(&f.gid, &day())
            .unwrap()
            .to_bytes(),
        proof
    );
    // Verification is deliberately not integration: no Ack index/debt mutation.
    assert!(
        phone
            .store
            .acks_day(&f.gid, "1970-01-01")
            .unwrap()
            .is_empty()
    );
    assert!(
        laptop
            .store
            .acks_day(&f.gid, "1970-01-01")
            .unwrap()
            .is_empty()
    );
    let meta = host
        .issue_storage_attestation(&f.gid, &DocId::Meta)
        .unwrap()
        .to_bytes();
    assert!(
        phone
            .verify_storage_attestation(&meta, &f.gid, &DocId::Meta)
            .is_ok()
    );
    assert_eq!(
        phone.verify_storage_attestation(&proof, &f.gid, &DocId::Meta),
        Err(AE::Scope)
    );
}

#[test]
fn failed_append_never_attests_unpersisted_operation_or_a_planned_frontier() {
    let (f, host) = host_fixture();
    let first = f.post_update();
    host.store.fail_next_append_for_test();
    assert!(matches!(
        host.apply(&f.gid, &day(), &first, |_, _, _| Ok(())),
        Err(Reject::Persistence(_))
    ));
    assert_eq!(
        host.issue_storage_attestation(&f.gid, &day()),
        Err(AE::Missing)
    );
    host.apply(&f.gid, &day(), &first, |_, _, _| Ok(()))
        .unwrap();
    let before = host
        .issue_storage_attestation(&f.gid, &day())
        .unwrap()
        .to_bytes();
    let next = GroupDoc::load_in(
        &f.gid,
        day(),
        writer_id(f.author.did(), &f.dev),
        None,
        &[first.clone()],
    )
    .unwrap();
    let record = Record {
        group: f.gid.clone(),
        author: f.author.did().into(),
        dev: f.dev,
        at: 1001,
        seen: f.meta.seen(),
        body: Body::Post {
            text: "second original".into(),
            reply_to: None,
            image: None,
        },
    };
    let (_, after) = next.write(&f.author, &record).unwrap();
    host.store.fail_next_append_for_test();
    assert!(matches!(
        host.apply(&f.gid, &day(), &after, |_, _, _| Ok(())),
        Err(Reject::Persistence(_))
    ));
    assert_eq!(
        host.issue_storage_attestation(&f.gid, &day())
            .unwrap()
            .to_bytes(),
        before
    );
    host.apply(&f.gid, &day(), &after, |_, _, _| Ok(()))
        .unwrap();
    let advanced = host
        .issue_storage_attestation(&f.gid, &day())
        .unwrap()
        .to_bytes();
    assert_ne!(advanced, before);
}

#[test]
fn no_hosting_authority_is_created_by_signature_membership_or_mutable_cache() {
    let (f, host) = host_fixture();
    let laptop = forwarding_proofs::sibling(&f, [4; 32]);
    // Group owner/member with signing key, but no HostPermit + HostAccept for this device.
    assert_eq!(
        laptop.issue_storage_attestation(&f.gid, &DocId::Meta),
        Err(AE::Authority)
    );
    // A forged in-memory derived cache must not grant authority absent persisted records.
    laptop.groups.lock().get_mut(&f.gid).unwrap().state.hosts =
        host.groups.lock()[&f.gid].state.hosts.clone();
    laptop.groups.lock().get_mut(&f.gid).unwrap().state.hosts[0].did = laptop.did.clone();
    laptop.groups.lock().get_mut(&f.gid).unwrap().state.hosts[0].dev = laptop.dev;
    assert_eq!(
        laptop.issue_storage_attestation(&f.gid, &DocId::Meta),
        Err(AE::Authority)
    );
    let mut changed_host = host;
    changed_host.dev = [9; 32];
    assert_eq!(
        changed_host.issue_storage_attestation(&f.gid, &DocId::Meta),
        Err(AE::Authority)
    );
}

#[test]
fn missing_metadata_defers_and_current_revocation_overrides_signed_old_context() {
    let (mut f, host) = host_fixture();
    host.apply(&f.gid, &day(), &f.post_update(), |_, _, _| Ok(()))
        .unwrap();
    let proof = host
        .issue_storage_attestation(&f.gid, &day())
        .unwrap()
        .to_bytes();
    let phone = forwarding_proofs::sibling(&f, [4; 32]);
    assert!(
        phone
            .verify_storage_attestation(&proof, &f.gid, &day())
            .is_ok()
    );
    let blank = receiver();
    assert_eq!(
        blank.verify_storage_attestation(&proof, &f.gid, &day()),
        Err(AE::PendingMetadata)
    );
    blank.groups.lock().insert(
        f.gid.clone(),
        Live::empty(&f.gid, blank.wid, GroupMeta::default()),
    );
    assert_eq!(
        blank.verify_storage_attestation(&proof, &f.gid, &day()),
        Err(AE::PendingMetadata)
    );
    update_role(&mut f, &host, false);
    import_meta(&f, &phone);
    import_meta(&f, &host);
    // Cryptographic claim remains genuine; new acceptance/issuance lacks authority.
    assert!(open_attestation(&proof, &f.gid, &day()).is_ok());
    assert_eq!(
        phone.verify_storage_attestation(&proof, &f.gid, &day()),
        Err(AE::Authority)
    );
    assert_eq!(
        host.issue_storage_attestation(&f.gid, &day()),
        Err(AE::Authority)
    );
}

#[test]
fn member_signature_is_valid_crypto_but_not_host_authority() {
    let (f, host) = host_fixture();
    let phone = forwarding_proofs::sibling(&f, [4; 32]);
    let original = host
        .issue_storage_attestation(&f.gid, &DocId::Meta)
        .unwrap()
        .to_bytes();
    let mut claim = open_attestation(&original, &f.gid, &DocId::Meta).unwrap();
    claim.host = phone.did.clone();
    claim.host_dev = phone.dev;
    let body = cryptography::codec::to_bytes(&claim).unwrap();
    let input = cryptography::sign::input(cryptography::domains::STORAGE_ATTESTATION, &[&body]);
    let bytes = SignedStorageAttestation::from_parts(body, phone.account.sign(&input)).to_bytes();
    assert!(open_attestation(&bytes, &f.gid, &DocId::Meta).is_ok());
    assert_eq!(
        phone.verify_storage_attestation(&bytes, &f.gid, &DocId::Meta),
        Err(AE::Authority)
    );
}

#[test]
fn removed_host_cannot_mint_or_have_new_statement_accepted() {
    let (f, host) = host_fixture();
    let phone = forwarding_proofs::sibling(&f, [4; 32]);
    let bytes = host
        .issue_storage_attestation(&f.gid, &DocId::Meta)
        .unwrap()
        .to_bytes();
    for at in 10..13 {
        let flag = Record {
            group: f.gid.clone(),
            author: f.author.did().into(),
            dev: f.dev,
            at,
            seen: f.meta.seen(),
            body: Body::Flag {
                member: host.did.clone(),
            },
        };
        f.meta.write(&f.author, &flag).unwrap();
    }
    import_meta(&f, &phone);
    import_meta(&f, &host);
    assert!(host.groups.lock()[&f.gid].state.is_removed(&host.did));
    assert_eq!(
        host.issue_storage_attestation(&f.gid, &DocId::Meta),
        Err(AE::Authority)
    );
    assert_eq!(
        phone.verify_storage_attestation(&bytes, &f.gid, &DocId::Meta),
        Err(AE::Authority)
    );
}

#[test]
fn missing_host_context_requires_catchup_not_optimistic_acceptance() {
    let (f, host) = host_fixture();
    let phone = forwarding_proofs::sibling(&f, [4; 32]);
    // Replace only the receiver's durable metadata with a genuine genesis-only
    // replica of this SAME group, not metadata from a different synthetic DID.
    let owner_genesis = f
        .meta
        .entries()
        .unwrap()
        .into_iter()
        .find(|e| matches!(e.rec.body, Body::Genesis { .. }))
        .unwrap();
    let update = f
        .meta
        .export_until(
            &[(owner_genesis.peer, owner_genesis.counter)],
            &VersionVector::default(),
        )
        .unwrap();
    let old = receiver();
    old.groups.lock().insert(
        f.gid.clone(),
        Live::empty(&f.gid, old.wid, GroupMeta::default()),
    );
    old.apply(&f.gid, &DocId::Meta, &update, |_, _, _| Ok(()))
        .unwrap();
    let bytes = host
        .issue_storage_attestation(&f.gid, &DocId::Meta)
        .unwrap()
        .to_bytes();
    assert_eq!(
        old.verify_storage_attestation(&bytes, &f.gid, &DocId::Meta),
        Err(AE::PendingMetadata)
    );
    assert!(
        phone
            .verify_storage_attestation(&bytes, &f.gid, &DocId::Meta)
            .is_ok()
    );
}

#[test]
fn failed_metadata_append_keeps_authority_at_actual_persisted_state() {
    let (mut f, host) = host_fixture();
    update_role(&mut f, &host, false);
    import_meta(&f, &host);
    assert_eq!(
        host.issue_storage_attestation(&f.gid, &DocId::Meta),
        Err(AE::Authority)
    );
    let regrant = Record {
        group: f.gid.clone(),
        author: f.author.did().into(),
        dev: f.dev,
        at: 101,
        seen: f.meta.seen(),
        body: Body::HostPermit {
            host: host.did.clone(),
            host_dev: host.dev,
            active: true,
        },
    };
    f.meta.write(&f.author, &regrant).unwrap();
    let update = f
        .meta
        .export_since(&host.doc_vv(&f.gid, &DocId::Meta))
        .unwrap();
    host.store.fail_next_append_for_test();
    assert!(matches!(
        host.apply(&f.gid, &DocId::Meta, &update, |_, _, _| Ok(())),
        Err(Reject::Persistence(_))
    ));
    assert_eq!(
        host.issue_storage_attestation(&f.gid, &DocId::Meta),
        Err(AE::Authority)
    );
}
