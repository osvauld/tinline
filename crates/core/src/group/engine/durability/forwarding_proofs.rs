//! Real sealed group export/import primitives through synthetic linked replicas.
//! No self-sync transport integration or new permission/hosting policy is implied.
use super::*;

pub(super) fn sibling(f: &Fixture, dev: [u8; 32]) -> GroupCore {
    let mut core = receiver();
    core.account = f.author.clone();
    core.did = f.author.did().into();
    core.dev = dev;
    core.wid = writer_id(&core.did, &dev);
    core.groups.lock().insert(
        f.gid.clone(),
        Live::empty(&f.gid, core.wid, GroupMeta::default()),
    );
    let bootstrap = f.meta.export_since(&VersionVector::default()).unwrap();
    core.apply(&f.gid, &DocId::Meta, &bootstrap, |_, _, _| Ok(()))
        .unwrap();
    core
}

#[test]
fn persisted_phone_post_forwarded_by_laptop_keeps_exact_author_proof_and_writer() {
    let f = Fixture::new();
    let laptop = sibling(&f, [4; 32]);
    let mobile = sibling(&f, [5; 32]);
    let original = f.post_update();
    laptop
        .apply(&f.gid, &day(), &original, |_, _, _| Ok(()))
        .unwrap();
    // Same durable-export helper the actual history path uses. Export is not a
    // replacement snapshot authored/signed by the forwarding laptop.
    let forwarded = super::super::history::export_day_for_durability_test(
        &laptop,
        &f.gid,
        "1970-01-01",
        &VersionVector::default(),
    )
    .unwrap();
    mobile
        .apply(&f.gid, &day(), &forwarded, |_, _, _| Ok(()))
        .unwrap();
    let (snapshot, updates) = mobile.store.shard_rows(&f.gid, "1970-01-01").unwrap();
    let reloaded =
        GroupDoc::load_in(&f.gid, day(), mobile.wid, snapshot.as_deref(), &updates).unwrap();
    let entries = reloaded.entries().unwrap();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.rec.author, f.author.did());
    assert_eq!(entry.rec.dev, f.dev);
    assert_eq!(entry.peer, writer_id(f.author.did(), &f.dev));
    assert_ne!(entry.peer, laptop.wid);
    assert_ne!(entry.peer, mobile.wid);
    let original_doc = GroupDoc::new_in(&f.gid, day(), writer_id(f.author.did(), &f.dev));
    original_doc.import_for_test(&original);
    assert_eq!(
        reloaded.raw_value(&entry.id),
        original_doc.raw_value(&entry.id)
    );
    // Reconciliation tolerates duplicate forwarding without duplicate records.
    assert!(
        mobile
            .apply(&f.gid, &day(), &forwarded, |_, _, _| Ok(()))
            .unwrap()
            .is_none()
    );
    assert!(laptop.groups.lock()[&f.gid].state.hosts.is_empty());
    assert!(mobile.groups.lock()[&f.gid].state.hosts.is_empty());
}

#[test]
fn own_replica_forwarding_cannot_strip_or_substitute_author_signature() {
    let f = Fixture::new();
    let mobile = sibling(&f, [5; 32]);
    let original = f.post_update();
    let original_doc = GroupDoc::new_in(&f.gid, day(), writer_id(f.author.did(), &f.dev));
    original_doc.import_for_test(&original);
    let entry = original_doc.entries().unwrap().pop().unwrap();
    let raw = original_doc.raw_value(&entry.id).unwrap();
    let mut signed: SignedRecord = bincode::deserialize(&raw).unwrap();
    let mut record: Record = bincode::deserialize(&signed.body).unwrap();
    record.body = Body::Post {
        text: "substituted by forwarder".into(),
        reply_to: None,
    };
    signed.body = bincode::serialize(&record).unwrap();
    let forged = signed.to_bytes();
    let forgery = GroupDoc::new_in(&f.gid, day(), entry.peer);
    let update = forgery.raw_insert(&record_id(&forged), forged);
    let result = mobile.apply(&f.gid, &day(), &update, |_, _, _| Ok(()));
    assert!(matches!(
        result,
        Err(Reject::Record(RecordError::BadSignature))
    ));
    assert_eq!(mobile.doc_vv(&f.gid, &day()), VersionVector::default());
    assert!(mobile.store.days(&f.gid).unwrap().is_empty());
}

#[test]
fn valid_forwarded_record_cannot_be_replayed_into_second_group() {
    let first = Fixture::new();
    let second = Fixture::new();
    let mobile = sibling(&second, [5; 32]);
    let update = first.post_update();
    let result = mobile.apply(&second.gid, &day(), &update, |_, _, _| Ok(()));
    assert!(matches!(
        result,
        Err(Reject::Record(RecordError::WrongGroup))
    ));
    assert_eq!(mobile.doc_vv(&second.gid, &day()), VersionVector::default());
    assert!(mobile.store.days(&second.gid).unwrap().is_empty());
}
