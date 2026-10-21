//! Borrowed metadata obligations, not routing/pairing or receipt-trust tests.
//! Host Ack callbacks below assume the reader already admitted the host.
use super::reader::{Quiet, TestDir};
use super::*;
use crate::Node;

fn node(directory: &TestDir) -> Arc<Node> {
    Node::new(directory.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap()
}
fn owed(core: &GroupCore, gid: &str) -> Vec<u8> {
    core.groups.lock()[gid].meta.own_owed_meta.clone()
}
fn durable_owed(core: &GroupCore, gid: &str) -> Vec<u8> {
    core.store
        .groups()
        .unwrap()
        .into_iter()
        .find(|(g, _)| g == gid)
        .unwrap()
        .1
        .own_owed_meta
}
fn ack(node: &Node, core: &GroupCore, gid: &str, vv: &VersionVector) {
    node.inner.group_ack_for_durability_test(
        core,
        gid,
        "did:synthetic:host",
        &[9; 32],
        &DocId::Meta,
        &vv.encode(),
    );
}

#[test]
fn phone_metadata_import_owes_upstream_without_a_laptop_writer() {
    let directory = TestDir::new();
    let node = node(&directory);
    let f = Fixture::new();
    let core = forwarding_proofs::sibling(&f, [4; 32]);
    assert!(!node.inner.group_owes(&core, &f.gid));
    let update = f.meta_update();
    core.own_content_apply(&f.gid, &DocId::Meta, &update)
        .unwrap();
    assert!(core.doc_vv(&f.gid, &DocId::Meta).get(&core.wid).is_none());
    assert!(
        node.inner.group_owes(&core, &f.gid),
        "borrowed phone metadata must survive an offline upstream"
    );
    assert_eq!(owed(&core, &f.gid), durable_owed(&core, &f.gid));
    assert_eq!(
        decode_vv(&owed(&core, &f.gid)).unwrap(),
        core.doc_vv(&f.gid, &DocId::Meta)
    );
}

#[test]
fn failed_phone_metadata_append_restores_obligation_and_retries_once() {
    let f = Fixture::new();
    let core = forwarding_proofs::sibling(&f, [4; 32]);
    let before = core.doc_vv(&f.gid, &DocId::Meta);
    let update = f.meta_update();
    core.store.fail_next_append_for_test();
    assert!(matches!(
        core.own_content_apply(&f.gid, &DocId::Meta, &update),
        Err(Error::Io(_))
    ));
    assert!(owed(&core, &f.gid).is_empty());
    assert!(durable_owed(&core, &f.gid).is_empty());
    assert_eq!(core.doc_vv(&f.gid, &DocId::Meta), before);
    assert!(
        core.own_content_apply(&f.gid, &DocId::Meta, &update)
            .unwrap()
            .is_some()
    );
    let obligation = owed(&core, &f.gid);
    assert!(!obligation.is_empty());
    assert!(
        core.own_content_apply(&f.gid, &DocId::Meta, &update)
            .unwrap()
            .is_none()
    );
    assert_eq!(owed(&core, &f.gid), obligation);
    assert_eq!(durable_owed(&core, &f.gid), obligation);
}

#[test]
fn stale_metadata_ack_cannot_clear_a_newer_phone_frontier() {
    let directory = TestDir::new();
    let node = node(&directory);
    let f = Fixture::new();
    let core = forwarding_proofs::sibling(&f, [4; 32]);
    let first = f.meta_update();
    core.own_content_apply(&f.gid, &DocId::Meta, &first)
        .unwrap();
    let old = core.doc_vv(&f.gid, &DocId::Meta);
    let second = f.meta_update();
    core.own_content_apply(&f.gid, &DocId::Meta, &second)
        .unwrap();
    let newest = core.doc_vv(&f.gid, &DocId::Meta);
    assert_ne!(old, newest);
    ack(&node, &core, &f.gid, &VersionVector::default());
    ack(&node, &core, &f.gid, &old);
    assert!(node.inner.group_owes(&core, &f.gid));
    assert_eq!(decode_vv(&durable_owed(&core, &f.gid)).unwrap(), newest);
    ack(&node, &core, &f.gid, &newest);
    assert!(owed(&core, &f.gid).is_empty());
    assert!(durable_owed(&core, &f.gid).is_empty());
    assert!(!node.inner.group_owes(&core, &f.gid));
}

#[test]
fn failed_metadata_ack_clear_keeps_memory_and_disk_obligation() {
    let directory = TestDir::new();
    let node = node(&directory);
    let f = Fixture::new();
    let core = forwarding_proofs::sibling(&f, [4; 32]);
    core.own_content_apply(&f.gid, &DocId::Meta, &f.meta_update())
        .unwrap();
    let held = core.doc_vv(&f.gid, &DocId::Meta);
    let obligation = owed(&core, &f.gid);
    core.store.fail_next_meta_write_for_test();
    ack(&node, &core, &f.gid, &held);
    assert_eq!(owed(&core, &f.gid), obligation);
    assert_eq!(durable_owed(&core, &f.gid), obligation);
    assert!(node.inner.group_owes(&core, &f.gid));
    ack(&node, &core, &f.gid, &held);
    assert!(!node.inner.group_owes(&core, &f.gid));
}

#[test]
fn normal_host_metadata_import_does_not_create_own_link_obligation() {
    let f = Fixture::new();
    let core = forwarding_proofs::sibling(&f, [4; 32]);
    core.apply(&f.gid, &DocId::Meta, &f.meta_update(), |_, _, _| Ok(()))
        .unwrap();
    assert!(owed(&core, &f.gid).is_empty());
    assert!(durable_owed(&core, &f.gid).is_empty());
}

#[test]
fn legacy_group_metadata_without_obligation_retains_existing_user_state() {
    let meta = GroupMeta {
        read_upto: 42,
        unread: 7,
        ..Default::default()
    };
    let mut value = serde_json::to_value(&meta).unwrap();
    value.as_object_mut().unwrap().remove("own_owed_meta");
    let legacy: GroupMeta = serde_json::from_value(value).unwrap();
    assert!(legacy.own_owed_meta.is_empty());
    assert_eq!((legacy.read_upto, legacy.unread), (42, 7));
}

pub(super) fn disk_core(path: &std::path::Path, account: &identity::Identity) -> GroupCore {
    let mut core = receiver();
    core.store = crate::group::store::open(path.to_path_buf(), "synthetic restart fixture").unwrap();
    core.account = account.clone();
    core.did = account.did().into();
    core.dev = [4; 32];
    core.wid = writer_id(&core.did, &core.dev);
    for (gid, meta) in core.store.groups().unwrap() {
        let (snapshot, updates) = core.store.meta_rows(&gid).unwrap();
        let doc = GroupDoc::load(&gid, core.wid, snapshot.as_deref(), &updates).unwrap();
        let entries = doc.entries_trusted().unwrap();
        let state = derive_meta(&gid, &entries, &doc.vv());
        core.groups.lock().insert(
            gid,
            Live {
                doc,
                entries,
                state,
                meta,
                shards: Shards::default(),
            },
        );
    }
    core
}

#[test]
fn phone_metadata_obligation_survives_reopen_before_ack_and_duplicate_retry() {
    let directory = TestDir::new();
    let node = node(&directory);
    let path = directory.0.join("owned-group-restart.redb");
    let f = Fixture::new();
    let core = disk_core(&path, &f.author);
    core.groups.lock().insert(
        f.gid.clone(),
        Live::empty(&f.gid, core.wid, GroupMeta::default()),
    );
    let genesis = f.meta.export_since(&VersionVector::default()).unwrap();
    core.own_content_apply(&f.gid, &DocId::Meta, &genesis)
        .unwrap();
    ack(&node, &core, &f.gid, &core.doc_vv(&f.gid, &DocId::Meta));
    let update = f.meta_update();
    core.own_content_apply(&f.gid, &DocId::Meta, &update)
        .unwrap();
    let accepted = core.doc_vv(&f.gid, &DocId::Meta);
    // Stored before a reply/receipt. Release and reopen the actual sealed redb.
    core.shut();
    drop(core);
    let recovered = disk_core(&path, &f.author);
    assert!(node.inner.group_owes(&recovered, &f.gid));
    assert_eq!(recovered.doc_vv(&f.gid, &DocId::Meta), accepted);
    assert!(
        recovered
            .own_content_apply(&f.gid, &DocId::Meta, &update)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        decode_vv(&durable_owed(&recovered, &f.gid)).unwrap(),
        accepted
    );
    ack(&node, &recovered, &f.gid, &accepted);
    recovered.shut();
    drop(recovered);
    let final_core = disk_core(&path, &f.author);
    assert!(owed(&final_core, &f.gid).is_empty());
    assert!(!node.inner.group_owes(&final_core, &f.gid));
    final_core.shut();
}
