//! Actual group importer/store regression tests, not the Python routing model.
//! Per-store append failures occur before the sealed-store transaction commits.
use super::*;

mod bounded_exports;
mod content_adapter;
mod group_receipts_sim;
mod history_inventory;
mod loro_ranges;
mod metadata_obligations;
mod receipt_persistence;
mod receipt_reopen;
mod storage_attestations;

pub(super) fn receipt_cores_for_test(
    account: identity::Identity,
    source_epoch: u64,
    target_epoch: u64,
) -> (Arc<GroupCore>, Arc<GroupCore>, String, Vec<u8>) {
    let (f, host) = storage_attestations::host_fixture_for(Fixture::for_author(account));
    let update = f.post_update();
    host.apply(&f.gid, &day(), &update, |_, _, _| Ok(()))
        .unwrap();
    let bytes = host
        .issue_storage_attestation(&f.gid, &day())
        .unwrap()
        .to_bytes();
    let mut source = forwarding_proofs::sibling(&f, [4; 32]);
    let mut target = forwarding_proofs::sibling(&f, [5; 32]);
    source.epoch = source_epoch;
    target.epoch = target_epoch;
    source.own_content_apply(&f.gid, &day(), &update).unwrap();
    source
        .accept_storage_attestation(&bytes, &f.gid, &day())
        .unwrap();
    (Arc::new(source), Arc::new(target), f.gid, bytes)
}

pub(super) fn own_content_core_for_test(
    account: identity::Identity,
    epoch: u64,
    populated: bool,
) -> (Arc<GroupCore>, String, String) {
    let f = Fixture::for_author(account.clone());
    let mut core = if populated {
        forwarding_proofs::sibling(&f, [4; 32])
    } else {
        receiver()
    };
    core.epoch = epoch;
    core.account = account.clone();
    core.did = account.did().into();
    core.dev = if populated { [4; 32] } else { [5; 32] };
    core.wid = writer_id(account.did(), &core.dev);
    let at = now_ms() as i64;
    let day = crate::chat::doc::day_of(at);
    if populated {
        let doc = GroupDoc::new_in(
            &f.gid,
            DocId::Day(day.clone()),
            writer_id(account.did(), &f.dev),
        );
        let record = Record {
            group: f.gid.clone(),
            author: account.did().into(),
            dev: f.dev,
            at,
            seen: f.meta.seen(),
            body: Body::Post {
                text: "original phone bytes".into(),
                reply_to: None,
            },
        };
        let (_, update) = doc.write(&account, &record).unwrap();
        core.own_content_apply(&f.gid, &DocId::Day(day.clone()), &update)
            .unwrap();
    }
    (Arc::new(core), f.gid, day)
}
mod forwarding_proofs;
mod reader;

fn receiver() -> GroupCore {
    let account = identity::generate().0;
    let did = account.did().to_string();
    let dev = [2; 32];
    let store = GroupStore::in_memory("synthetic durability fixture");
    let receipts = member_receipts::controller(&store, &did, dev);
    GroupCore {
        epoch: 0,
        store,
        account,
        did: did.clone(),
        dev,
        wid: writer_id(&did, &dev),
        groups: Default::default(),
        ep: Default::default(),
        ep_id: Default::default(),
        sessions: Default::default(),
        dialing: Default::default(),
        backoff: Default::default(),
        refusals: Default::default(),
        my_addr: Default::default(),
        announced: Default::default(),
        next: AtomicU64::new(1),
        kick: Notify::new(),
        closed: AtomicBool::new(false),
        loops: AtomicBool::new(false),
        moving: Default::default(),
        compacted: Default::default(),
        codes: Default::default(),
        receipts,
    }
}

struct Fixture {
    core: GroupCore,
    author: identity::Identity,
    dev: [u8; 32],
    gid: String,
    meta: GroupDoc,
}

impl Fixture {
    fn new() -> Self {
        Self::for_author(identity::generate().0)
    }

    fn for_author(author: identity::Identity) -> Self {
        let dev = [1; 32];
        let nonce = [3; 16];
        let gid = group_id(author.did(), &nonce);
        let meta = GroupDoc::new(&gid, writer_id(author.did(), &dev));
        let record = Record {
            group: gid.clone(),
            author: author.did().into(),
            dev,
            at: 0,
            seen: meta.seen(),
            body: Body::Genesis {
                name: "synthetic group".into(),
                nonce,
                owner_name: "author".into(),
            },
        };
        let (_, update) = meta.write(&author, &record).unwrap();
        let core = receiver();
        core.groups.lock().insert(
            gid.clone(),
            Live::empty(&gid, core.wid, GroupMeta::default()),
        );
        assert!(
            core.apply(&gid, &DocId::Meta, &update, |_, _, _| Ok(()))
                .unwrap()
                .is_some()
        );
        Self {
            core,
            author,
            dev,
            gid,
            meta,
        }
    }

    fn meta_update(&self) -> Vec<u8> {
        let record = Record {
            group: self.gid.clone(),
            author: self.author.did().into(),
            dev: self.dev,
            at: 1,
            seen: self.meta.seen(),
            body: Body::EncKey { key: [7; 32] },
        };
        self.meta.write(&self.author, &record).unwrap().1
    }

    fn post_update(&self) -> Vec<u8> {
        let doc = GroupDoc::new_in(&self.gid, day(), writer_id(self.author.did(), &self.dev));
        let record = Record {
            group: self.gid.clone(),
            author: self.author.did().into(),
            dev: self.dev,
            at: 1000,
            seen: self.meta.seen(),
            body: Body::Post {
                text: "must persist".into(),
                reply_to: None,
            },
        };
        doc.write(&self.author, &record).unwrap().1
    }

    fn durable_meta_vv(&self) -> VersionVector {
        let (snap, updates) = self.core.store.meta_rows(&self.gid).unwrap();
        GroupDoc::load(&self.gid, self.core.wid, snap.as_deref(), &updates)
            .unwrap()
            .vv()
    }
}

fn day() -> DocId {
    DocId::Day("1970-01-01".into())
}

#[test]
fn duplicate_meta_record_with_new_operation_range_is_persisted_before_ack() {
    let f = Fixture::new();
    let entry = f.meta.entries().unwrap().pop().unwrap();
    let original = f.meta.raw_value(&entry.id).unwrap();
    let before = f.meta.vv();
    // Local same-value writes are optimized away. Use an alternate writer
    // prefix to exercise an accepted new op range containing a known record.
    // This is NOT a claim that writer forks are safe or a new fork policy.
    let branch = GroupDoc::new(&f.gid, f.meta.peer());
    let prefix = Record {
        group: f.gid.clone(),
        author: f.author.did().into(),
        dev: f.dev,
        at: 1,
        seen: Vec::new(),
        body: Body::EncKey { key: [9; 32] },
    }
    .sign(&f.author)
    .to_bytes();
    branch.raw_insert(&record_id(&prefix), prefix);
    branch.raw_insert(&entry.id, original);
    let update = branch.export_since(&before).unwrap();
    assert_ne!(
        branch.vv(),
        before,
        "fixture must introduce a new oplog operation"
    );
    f.core
        .apply(&f.gid, &DocId::Meta, &update, |_, _, _| Ok(()))
        .unwrap();
    assert_eq!(f.core.doc_vv(&f.gid, &DocId::Meta), branch.vv());
    assert_eq!(
        f.durable_meta_vv(),
        branch.vv(),
        "acknowledged VV must survive reload even when record ID was already known"
    );
    assert_eq!(f.core.groups.lock()[&f.gid].entries.len(), 1);
    let own_receiver = forwarding_proofs::sibling(&f, [4; 32]);
    own_receiver
        .own_content_apply(&f.gid, &DocId::Meta, &update)
        .unwrap();
    let marker = own_receiver.groups.lock()[&f.gid]
        .meta
        .own_owed_meta
        .clone();
    assert_eq!(decode_vv(&marker).unwrap(), branch.vv());
    let saved = own_receiver
        .store
        .groups()
        .unwrap()
        .into_iter()
        .find(|(g, _)| g == &f.gid)
        .unwrap()
        .1;
    assert_eq!(
        saved.own_owed_meta, marker,
        "VV-only own imports must persist their upstream obligation too"
    );
}

#[test]
fn meta_checkpoint_is_independent_and_keeps_original_writer() {
    let f = Fixture::new();
    let checkpoint = f.meta.checkpoint();
    let before = checkpoint.vv();
    let _ = f.meta_update();
    assert_ne!(f.meta.vv(), before);
    assert_eq!(checkpoint.vv(), before);
    assert_eq!(checkpoint.peer(), f.meta.peer());
    assert_eq!(checkpoint.entries().unwrap().len(), 1);
}

#[test]
fn failed_meta_append_restores_pending_post_visibility_and_unread_count() {
    let f = Fixture::new();
    let meta_update = f.meta_update();
    // The post names a meta frontier the receiver has not yet received.
    let post = f.post_update();
    f.core
        .apply(&f.gid, &day(), &post, |_, _, _| Ok(()))
        .unwrap();
    let previous_meta = crate::group::store::json(&f.core.groups.lock()[&f.gid].meta);
    assert!(
        f.core.groups.lock()[&f.gid]
            .shards
            .peek("1970-01-01")
            .unwrap()
            .has_pending()
    );
    f.core.store.fail_next_append_for_test();
    assert!(
        f.core
            .apply(&f.gid, &DocId::Meta, &meta_update, |_, _, _| Ok(()))
            .is_err()
    );
    {
        let mut groups = f.core.groups.lock();
        let live = groups.get_mut(&f.gid).unwrap();
        assert_eq!(crate::group::store::json(&live.meta), previous_meta);
        assert_eq!(live.meta.unread, 0);
        assert!(live.meta.last.is_none());
        let shard = live
            .shards
            .get(&f.core.store, &f.gid, f.core.wid, "1970-01-01", &live.state)
            .unwrap();
        assert!(shard.has_pending());
    }
    assert!(
        f.core
            .apply(&f.gid, &DocId::Meta, &meta_update, |_, _, _| Ok(()))
            .unwrap()
            .is_some()
    );
    let groups = f.core.groups.lock();
    let live = &groups[&f.gid];
    assert_eq!(live.meta.unread, 1);
    assert!(live.meta.last.is_some());
    assert!(!live.shards.peek("1970-01-01").unwrap().has_pending());
}

#[test]
fn meta_append_failure_is_not_acknowledgeable_and_memory_is_rolled_back() {
    let f = Fixture::new();
    let before = f.core.doc_vv(&f.gid, &DocId::Meta);
    let before_local = crate::group::store::json(&f.core.groups.lock()[&f.gid].meta);
    let update = f.meta_update();
    f.core.store.fail_next_append_for_test();
    let result = f
        .core
        .apply(&f.gid, &DocId::Meta, &update, |_, _, _| Ok(()));
    assert!(
        result.is_err(),
        "failed persistence must not enter group_read's success/ack branch"
    );
    assert_eq!(f.core.doc_vv(&f.gid, &DocId::Meta), before);
    assert_eq!(f.durable_meta_vv(), before);
    assert_eq!(
        crate::group::store::json(&f.core.groups.lock()[&f.gid].meta),
        before_local
    );
    assert_eq!(f.core.groups.lock()[&f.gid].entries.len(), 1);
}

#[test]
fn day_append_failure_is_not_acknowledgeable_or_visible() {
    let f = Fixture::new();
    let before_local = crate::group::store::json(&f.core.groups.lock()[&f.gid].meta);
    let update = f.post_update();
    f.core.store.fail_next_append_for_test();
    let result = f.core.apply(&f.gid, &day(), &update, |_, _, _| Ok(()));
    assert!(
        result.is_err(),
        "failed day persistence must not enter the success/ack branch"
    );
    assert_eq!(f.core.doc_vv(&f.gid, &day()), VersionVector::default());
    assert_eq!(
        crate::group::store::json(&f.core.groups.lock()[&f.gid].meta),
        before_local
    );
    assert!(
        f.core
            .store
            .shard_meta(&f.gid, "1970-01-01")
            .unwrap()
            .is_none()
    );
    assert!(f.core.store.days(&f.gid).unwrap().is_empty());
    assert!(
        f.core.groups.lock()[&f.gid]
            .shards
            .peek("1970-01-01")
            .is_none()
    );
}

#[test]
fn retry_meta_after_failed_append_persists_original_once() {
    let f = Fixture::new();
    let update = f.meta_update();
    f.core.store.fail_next_append_for_test();
    let _ = f
        .core
        .apply(&f.gid, &DocId::Meta, &update, |_, _, _| Ok(()));
    assert!(
        f.core
            .apply(&f.gid, &DocId::Meta, &update, |_, _, _| Ok(()))
            .unwrap()
            .is_some(),
        "retry must not see unpersisted ops as already accepted"
    );
    assert_eq!(f.durable_meta_vv(), f.meta.vv());
    assert_eq!(f.core.groups.lock()[&f.gid].entries.len(), 2);
    assert!(
        f.core
            .apply(&f.gid, &DocId::Meta, &update, |_, _, _| Ok(()))
            .unwrap()
            .is_none()
    );
    assert_eq!(f.core.store.meta_rows(&f.gid).unwrap().1.len(), 2);
}

#[test]
fn retry_day_after_failed_append_persists_original_once() {
    let f = Fixture::new();
    let update = f.post_update();
    f.core.store.fail_next_append_for_test();
    let _ = f.core.apply(&f.gid, &day(), &update, |_, _, _| Ok(()));
    assert!(
        f.core
            .apply(&f.gid, &day(), &update, |_, _, _| Ok(()))
            .unwrap()
            .is_some(),
        "day retry must not mistake transient import for persisted acceptance"
    );
    let (snap, updates) = f.core.store.shard_rows(&f.gid, "1970-01-01").unwrap();
    let reloaded = GroupDoc::load_in(&f.gid, day(), f.core.wid, snap.as_deref(), &updates).unwrap();
    assert_eq!(reloaded.entries().unwrap().len(), 1);
    assert_eq!(reloaded.vv(), f.core.doc_vv(&f.gid, &day()));
    assert_eq!(updates.len(), 1);
    assert!(
        f.core
            .apply(&f.gid, &day(), &update, |_, _, _| Ok(()))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        f.core
            .store
            .shard_rows(&f.gid, "1970-01-01")
            .unwrap()
            .1
            .len(),
        1
    );
}

#[test]
fn failed_local_meta_write_restores_writer_and_retries_at_same_counter() {
    let f = Fixture::new();
    let before = f.core.doc_vv(&f.gid, &DocId::Meta);
    let before_local = crate::group::store::json(&f.core.groups.lock()[&f.gid].meta);
    let body = Body::EncKey { key: [8; 32] };
    f.core.store.fail_next_append_for_test();
    assert!(f.core.write_at(&f.gid, body.clone(), 42).is_err());
    assert_eq!(f.core.doc_vv(&f.gid, &DocId::Meta), before);
    assert_eq!(
        crate::group::store::json(&f.core.groups.lock()[&f.gid].meta),
        before_local
    );
    assert_eq!(f.core.groups.lock()[&f.gid].doc.peer(), f.core.wid);
    let (entry, _) = f.core.write_at(&f.gid, body, 42).unwrap();
    assert_eq!(entry.counter, 0);
    assert_eq!(entry.peer, f.core.wid);
    assert_eq!(f.durable_meta_vv(), f.core.doc_vv(&f.gid, &DocId::Meta));
    assert_eq!(f.core.groups.lock()[&f.gid].entries.len(), 2);
}

#[test]
fn failed_local_day_write_does_not_leave_a_phantom_outbox_or_counter() {
    let f = Fixture::new();
    let before_local = crate::group::store::json(&f.core.groups.lock()[&f.gid].meta);
    let body = Body::Post {
        text: "local durability".into(),
        reply_to: None,
    };
    f.core.store.fail_next_append_for_test();
    assert!(f.core.write_at(&f.gid, body.clone(), 1000).is_err());
    assert_eq!(f.core.doc_vv(&f.gid, &day()), VersionVector::default());
    assert_eq!(
        crate::group::store::json(&f.core.groups.lock()[&f.gid].meta),
        before_local
    );
    assert!(f.core.store.days(&f.gid).unwrap().is_empty());
    let (entry, _) = f.core.write_at(&f.gid, body, 1000).unwrap();
    assert_eq!(entry.counter, 0);
    assert_eq!(entry.peer, f.core.wid);
    let (snap, updates) = f.core.store.shard_rows(&f.gid, "1970-01-01").unwrap();
    let reloaded = GroupDoc::load_in(&f.gid, day(), f.core.wid, snap.as_deref(), &updates).unwrap();
    assert_eq!(reloaded.entries().unwrap().len(), 1);
    assert_eq!(reloaded.vv(), f.core.doc_vv(&f.gid, &day()));
}
