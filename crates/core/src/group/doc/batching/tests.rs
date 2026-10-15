use super::*;
use crate::chat::doc::writer_id;
use crate::group::{
    doc::{DocId, Reject},
    record::{Body, Record},
};

fn docs(n: usize, id: DocId) -> (identity::Identity, Vec<GroupDoc>) {
    let account = identity::generate().0;
    let docs = (1..=n)
        .map(|i| {
            GroupDoc::new_in(
                "synthetic bounded export",
                id.clone(),
                writer_id(account.did(), &[i as u8; 32]),
            )
        })
        .collect();
    (account, docs)
}
fn day() -> DocId {
    DocId::Day("1970-01-01".into())
}
fn write(account: &identity::Identity, doc: &GroupDoc, device: u8, text: &str) -> Vec<u8> {
    let body = if doc.id == DocId::Meta {
        Body::EncKey { key: [device; 32] }
    } else {
        Body::Post {
            text: text.into(),
            reply_to: None,
        }
    };
    doc.write(
        account,
        &Record {
            v: 1,
            group: doc.group.clone(),
            author: account.did().into(),
            dev: [device; 32],
            at: 1000,
            seen: vec![],
            body,
        },
    )
    .unwrap()
    .1
}
fn limits(bytes: usize, operations: usize) -> BatchLimits {
    BatchLimits {
        bytes,
        operations,
        writers: 32,
    }
}
fn receiver(source: &GroupDoc) -> GroupDoc {
    GroupDoc::new_in(&source.group, source.id.clone(), 987)
}
fn exact(source: &GroupDoc, target: &GroupDoc) {
    assert_eq!(source.vv(), target.vv());
    let held = target.entries().unwrap();
    for entry in source.entries().unwrap() {
        assert_eq!(source.raw_value(&entry.id), target.raw_value(&entry.id));
        let actual = held.iter().find(|e| e.id == entry.id).unwrap();
        assert_eq!(actual, &entry);
    }
}
fn drain(source: &GroupDoc, target: &mut GroupDoc, cap: BatchLimits) -> usize {
    let mut batches = 0;
    loop {
        let Some(batch) = source.export_batch(&target.vv(), cap).unwrap() else {
            break;
        };
        assert!(batch.update.len() <= cap.bytes);
        assert!(batch.operations > 0 && batch.operations <= cap.operations);
        let before = target.vv();
        target.apply_remote(&batch.update, |_| Ok(())).unwrap();
        assert_ne!(target.vv(), before);
        assert_eq!(target.vv(), batch.frontier);
        assert_eq!(batch.more, target.vv() != source.vv());
        assert!(
            target
                .apply_remote(&batch.update, |_| Ok(()))
                .unwrap()
                .is_empty()
        );
        batches += 1;
        assert!(batches <= 1000, "planner did not progress");
    }
    exact(source, target);
    batches
}

#[test]
fn empty_and_fully_covered_documents_have_no_batch() {
    let (a, source) = docs(1, day());
    assert!(
        source[0]
            .export_batch(&VersionVector::default(), limits(4096, 8))
            .unwrap()
            .is_none()
    );
    write(&a, &source[0], 1, "one");
    assert!(
        source[0]
            .export_batch(&source[0].vv(), limits(4096, 8))
            .unwrap()
            .is_none()
    );
}

#[test]
fn operation_budget_sends_strict_prefix_not_whole_source_frontier() {
    let (a, source) = docs(1, day());
    for i in 0..19 {
        write(&a, &source[0], 1, &format!("post {i}"));
    }
    let mut target = receiver(&source[0]);
    let first = source[0]
        .export_batch(&target.vv(), limits(4096, 3))
        .unwrap()
        .unwrap();
    assert_eq!(first.operations, 3);
    assert_eq!(first.frontier.get(&source[0].peer()), Some(&3));
    assert!(first.more);
    assert_eq!(drain(&source[0], &mut target, limits(4096, 3)), 7);
}

#[test]
fn byte_budget_breaks_large_archive_into_whole_native_batches() {
    let (a, source) = docs(1, day());
    for i in 0..80 {
        let text: String = (0..300)
            .map(|j| char::from(b'a' + ((j * 17 + i * 13 + j * j) % 26) as u8))
            .collect();
        write(&a, &source[0], 1, &format!("{i}: {text}"));
    }
    let mut target = receiver(&source[0]);
    let one = source[0]
        .export_batch(&target.vv(), limits(4096, 1))
        .unwrap()
        .unwrap();
    let cap = limits(one.update.len() + 32, 64);
    assert!(source[0].export_since(&target.vv()).unwrap().len() > cap.bytes);
    assert!(drain(&source[0], &mut target, cap) > 1);
}

#[test]
fn cross_writer_prerequisites_are_selected_before_dependent_operations() {
    let (a, source) = docs(2, day());
    let a1 = write(&a, &source[0], 1, "A1");
    source[1].import_for_test(&a1);
    let b1 = write(&a, &source[1], 2, "B1 depends on A1");
    source[0].import_for_test(&b1);
    write(&a, &source[0], 1, "A2 depends on B1");
    let mut target = receiver(&source[0]);
    assert_eq!(drain(&source[0], &mut target, limits(4096, 1)), 3);
}

#[test]
fn concurrent_writers_and_seeded_dependency_schedules_converge() {
    for seed in [1_u64, 7, 91] {
        let (a, writers) = docs(3, day());
        let source = receiver(&writers[0]);
        let mut state = seed;
        let mut last: Option<Vec<u8>> = None;
        for i in 0..30 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let index = (state as usize) % writers.len();
            if state & 8 != 0 {
                if let Some(up) = &last {
                    writers[index].import_for_test(up);
                }
            }
            let up = write(
                &a,
                &writers[index],
                index as u8 + 1,
                &format!("seed {seed} post {i}"),
            );
            source.import_for_test(&up);
            last = Some(up);
        }
        let mut target = receiver(&source);
        drain(&source, &mut target, limits(4096, 4));
    }
}

#[test]
fn interrupted_receiver_resumes_from_native_snapshot_and_actual_frontier() {
    let (a, source) = docs(1, day());
    for i in 0..15 {
        write(&a, &source[0], 1, &format!("retained {i}"));
    }
    let mut target = receiver(&source[0]);
    let first = source[0]
        .export_batch(&target.vv(), limits(4096, 4))
        .unwrap()
        .unwrap();
    target.apply_remote(&first.update, |_| Ok(())).unwrap();
    let lost = source[0]
        .export_batch(&target.vv(), limits(4096, 4))
        .unwrap()
        .unwrap();
    let snapshot = target.snapshot();
    let mut reopened =
        GroupDoc::load_in(&source[0].group, day(), 987, Some(&snapshot), &[]).unwrap();
    let retry = source[0]
        .export_batch(&reopened.vv(), limits(4096, 4))
        .unwrap()
        .unwrap();
    assert_eq!(retry.frontier, lost.frontier);
    // Do not assume equal encoded bytes across calls; originals/IDs must match.
    reopened.apply_remote(&retry.update, |_| Ok(())).unwrap();
    drain(&source[0], &mut reopened, limits(4096, 4));
}

#[test]
fn indivisible_operation_over_budget_defers_without_advancing_source() {
    let (a, source) = docs(1, day());
    write(&a, &source[0], 1, "small but indivisible");
    let before = source[0].vv();
    let batch = source[0]
        .export_batch(&VersionVector::default(), limits(4096, 1))
        .unwrap()
        .unwrap();
    let required = batch.update.len();
    assert_eq!(
        source[0]
            .export_batch(&VersionVector::default(), limits(required - 1, 1))
            .unwrap_err(),
        BatchError::Oversized {
            required,
            limit: required - 1
        }
    );
    assert_eq!(source[0].vv(), before);
    let mut target = receiver(&source[0]);
    drain(&source[0], &mut target, limits(required, 1));
}

#[test]
fn noncausal_or_negative_receiver_frontier_is_rejected() {
    let (a, source) = docs(2, day());
    let a1 = write(&a, &source[0], 1, "A1");
    source[1].import_for_test(&a1);
    let b1 = write(&a, &source[1], 2, "B1");
    source[0].import_for_test(&b1);
    let mut impossible = VersionVector::default();
    impossible.insert(source[1].peer(), 1); // B1 without A1.
    assert_eq!(
        source[0]
            .export_batch(&impossible, limits(4096, 4))
            .unwrap_err(),
        BatchError::Frontier
    );
    impossible.insert(source[1].peer(), -1);
    assert_eq!(
        source[0]
            .export_batch(&impossible, limits(4096, 4))
            .unwrap_err(),
        BatchError::Frontier
    );
}

#[test]
fn receiver_extra_history_is_not_a_source_ack_or_source_frontier() {
    let (a, source) = docs(2, day());
    write(&a, &source[0], 1, "source-only");
    let independent = write(&a, &source[1], 2, "receiver-only");
    let mut target = receiver(&source[0]);
    target.apply_remote(&independent, |_| Ok(())).unwrap();
    let batch = source[0]
        .export_batch(&target.vv(), limits(4096, 4))
        .unwrap()
        .unwrap();
    assert!(batch.frontier.get(&source[1].peer()).is_none());
    assert!(!batch.more);
    target.apply_remote(&batch.update, |_| Ok(())).unwrap();
    assert_eq!(target.vv().get(&source[0].peer()), Some(&1));
    assert_eq!(target.vv().get(&source[1].peer()), Some(&1));
}

#[test]
fn invalid_limits_and_writer_budget_fail_closed() {
    let (a, source) = docs(2, day());
    let a1 = write(&a, &source[0], 1, "A1");
    source[1].import_for_test(&a1);
    let b1 = write(&a, &source[1], 2, "B1");
    source[0].import_for_test(&b1);
    for cap in [
        limits(0, 1),
        limits(4096, 0),
        limits(MAX_EXPORT_BYTES + 1, 1),
        limits(4096, MAX_EXPORT_OPS + 1),
    ] {
        assert_eq!(
            source[0]
                .export_batch(&VersionVector::default(), cap)
                .unwrap_err(),
            BatchError::Bounds
        );
    }
    let cap = BatchLimits {
        bytes: 4096,
        operations: 4,
        writers: 1,
    };
    assert_eq!(
        source[0]
            .export_batch(&VersionVector::default(), cap)
            .unwrap_err(),
        BatchError::Writers
    );
}

#[test]
fn tainted_source_never_exports_unaccepted_operations() {
    let (a, mut source) = docs(2, day());
    let up = write(&a, &source[0], 1, "valid original but refused policy");
    assert!(matches!(
        source[1].apply_remote_inplace(&up, |_| Err("refused".into())),
        Err(Reject::Policy(_))
    ));
    assert_eq!(
        source[1]
            .export_batch(&VersionVector::default(), limits(4096, 8))
            .unwrap_err(),
        BatchError::Tainted
    );
}

#[test]
fn grouped_loro_changes_can_be_split_at_whole_map_operation_boundaries() {
    let (a, source) = docs(1, day());
    let map = source[0].doc.get_map(super::super::ROOT);
    for i in 0..9 {
        let record = Record {
            v: 1,
            group: source[0].group.clone(),
            author: a.did().into(),
            dev: [1; 32],
            at: 1000,
            seen: vec![],
            body: Body::Post {
                text: format!("one transaction, operation {i}"),
                reply_to: None,
            },
        };
        let bytes = record.sign(&a).to_bytes();
        let id = crate::group::record::record_id(&bytes);
        map.insert(&id, loro::LoroValue::Binary(bytes.into()))
            .unwrap();
    }
    source[0].doc.commit();
    let change = source[0]
        .doc
        .get_change(ID::new(source[0].peer(), 0))
        .unwrap();
    assert_eq!(change.len, 9);
    let mut target = receiver(&source[0]);
    assert_eq!(drain(&source[0], &mut target, limits(4096, 2)), 5);
}

#[test]
fn metadata_maps_use_the_same_native_batching_without_text_rewriting() {
    let (a, source) = docs(2, DocId::Meta);
    let up = write(&a, &source[0], 1, "");
    source[1].import_for_test(&up);
    let up = write(&a, &source[1], 2, "");
    source[0].import_for_test(&up);
    let mut target = receiver(&source[0]);
    assert_eq!(drain(&source[0], &mut target, limits(4096, 1)), 2);
}
