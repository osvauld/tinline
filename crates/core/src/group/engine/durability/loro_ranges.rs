//! Actual Loro 1.16 range exports through existing group validation/persistence.
//! Test-only range helper. No production batching algorithm or wire negotiation.
use super::*;
use loro::{ExportMode, IdSpan, LoroDoc};

pub(super) fn source(f: &Fixture, dev: [u8; 32]) -> GroupDoc {
    GroupDoc::new_in(&f.gid, day(), writer_id(f.author.did(), &dev))
}
pub(super) fn post(f: &Fixture, doc: &GroupDoc, dev: [u8; 32], text: &str) -> (Entry, Vec<u8>) {
    doc.write(
        &f.author,
        &Record {
            v: 1,
            group: f.gid.clone(),
            author: f.author.did().into(),
            dev,
            at: 1000,
            seen: f.meta.seen(),
            body: Body::Post {
                text: text.into(),
                reply_to: None,
            },
        },
    )
    .unwrap()
}
fn range(doc: &GroupDoc, peer: u64, start: i32, end: i32) -> Vec<u8> {
    // Snapshot contains native operation history. Rebuilding the export handle
    // never inserts/re-signs records or changes their original writer IDs.
    let loro = LoroDoc::new();
    loro.import(&doc.snapshot()).unwrap();
    loro.export(ExportMode::updates_in_range(vec![IdSpan::new(
        peer, start, end,
    )]))
    .unwrap()
}
pub(super) fn persisted(core: &GroupCore, gid: &str) -> GroupDoc {
    let (snapshot, updates) = core.store.shard_rows(gid, "1970-01-01").unwrap();
    GroupDoc::load_in(gid, day(), core.wid, snapshot.as_deref(), &updates).unwrap()
}
pub(super) fn assert_originals(a: &GroupDoc, b: &GroupDoc) {
    assert_eq!(a.vv(), b.vv());
    let held = b.entries().unwrap();
    for e in a.entries().unwrap() {
        assert_eq!(a.raw_value(&e.id), b.raw_value(&e.id));
        let found = held.iter().find(|v| v.id == e.id).unwrap();
        assert_eq!(
            (found.peer, found.counter, &found.rec.author, found.rec.dev),
            (e.peer, e.counter, &e.rec.author, e.rec.dev)
        );
    }
}

#[test]
fn native_range_chunks_cross_two_own_replicas_with_exact_proofs_and_writers() {
    let f = Fixture::new();
    let doc = source(&f, f.dev);
    for i in 0..24 {
        post(&f, &doc, f.dev, &format!("original operation {i}"));
    }
    let laptop = forwarding_proofs::sibling(&f, [4; 32]);
    let phone = forwarding_proofs::sibling(&f, [5; 32]);
    for start in (0..24).step_by(3) {
        let up = range(&doc, doc.peer(), start, start + 3);
        assert!(up.len() < crate::group::own::contents::MAX_BATCH);
        assert!(
            laptop
                .own_content_apply(&f.gid, &day(), &up)
                .unwrap()
                .is_some()
        );
        // Forward from persisted rows, using the receiver's current frontier.
        let before = phone.doc_vv(&f.gid, &day());
        let (forward, advertised) = laptop
            .own_content_export(&f.gid, &day(), &before)
            .unwrap()
            .unwrap();
        phone.own_content_apply(&f.gid, &day(), &forward).unwrap();
        assert_eq!(
            decode_vv(&advertised).unwrap(),
            persisted(&phone, &f.gid).vv()
        );
    }
    assert_originals(&doc, &persisted(&laptop, &f.gid));
    assert_originals(&doc, &persisted(&phone, &f.gid));
    assert!(persisted(&phone, &f.gid).vv().get(&phone.wid).is_none());
    assert!(
        phone
            .own_content_export(&f.gid, &day(), &doc.vv())
            .unwrap()
            .is_none()
    );
}

#[test]
fn later_range_without_predecessor_is_not_persisted_and_retries_after_prefix() {
    let f = Fixture::new();
    let doc = source(&f, f.dev);
    for i in 0..4 {
        post(&f, &doc, f.dev, &format!("record {i}"));
    }
    let core = forwarding_proofs::sibling(&f, [4; 32]);
    let late = range(&doc, doc.peer(), 2, 4);
    assert!(matches!(
        core.own_content_apply(&f.gid, &day(), &late),
        Err(Error::Protocol(_))
    ));
    assert_eq!(persisted(&core, &f.gid).vv(), VersionVector::default());
    assert!(
        core.store
            .shard_meta(&f.gid, "1970-01-01")
            .unwrap()
            .is_none()
    );
    let prefix = range(&doc, doc.peer(), 0, 2);
    core.own_content_apply(&f.gid, &day(), &prefix).unwrap();
    core.own_content_apply(&f.gid, &day(), &late).unwrap();
    assert_originals(&doc, &persisted(&core, &f.gid));
    assert!(
        core.own_content_apply(&f.gid, &day(), &late)
            .unwrap()
            .is_none()
    );
}

#[test]
fn cross_writer_causal_ranges_require_both_original_writer_prefixes() {
    let f = Fixture::new();
    let a = source(&f, f.dev);
    let (_, first) = post(&f, &a, f.dev, "phone before laptop");
    let other_dev = [6; 32];
    let b = source(&f, other_dev);
    b.import_for_test(&first); // Valid original ops, merge only for source fixture.
    let (_, second) = post(&f, &b, other_dev, "laptop after phone");
    a.import_for_test(&second);
    post(&f, &a, f.dev, "phone after laptop");
    let core = forwarding_proofs::sibling(&f, [4; 32]);
    let a1 = range(&a, a.peer(), 0, 1);
    let b1 = range(&a, b.peer(), 0, 1);
    let a2 = range(&a, a.peer(), 1, 2);
    assert!(core.own_content_apply(&f.gid, &day(), &b1).is_err());
    assert!(core.own_content_apply(&f.gid, &day(), &a2).is_err());
    assert_eq!(persisted(&core, &f.gid).vv(), VersionVector::default());
    core.own_content_apply(&f.gid, &day(), &a1).unwrap();
    assert!(core.own_content_apply(&f.gid, &day(), &a2).is_err());
    assert_eq!(persisted(&core, &f.gid).vv().get(&a.peer()), Some(&1));
    core.own_content_apply(&f.gid, &day(), &b1).unwrap();
    core.own_content_apply(&f.gid, &day(), &a2).unwrap();
    assert_originals(&a, &persisted(&core, &f.gid));
}

#[test]
fn failed_range_append_rolls_back_frontier_and_resends_exact_original() {
    let f = Fixture::new();
    let doc = source(&f, f.dev);
    for i in 0..6 {
        post(&f, &doc, f.dev, &format!("fault range {i}"));
    }
    let core = forwarding_proofs::sibling(&f, [4; 32]);
    core.own_content_apply(&f.gid, &day(), &range(&doc, doc.peer(), 0, 3))
        .unwrap();
    let durable_before = persisted(&core, &f.gid).vv();
    let last = range(&doc, doc.peer(), 3, 6);
    core.store.fail_next_append_for_test();
    assert!(matches!(
        core.own_content_apply(&f.gid, &day(), &last),
        Err(Error::Io(_))
    ));
    assert_eq!(core.doc_vv(&f.gid, &day()), durable_before);
    assert_eq!(persisted(&core, &f.gid).vv(), durable_before);
    // A forwarded export cannot leak the failed import's record bytes/frontier.
    assert!(
        core.own_content_export(&f.gid, &day(), &durable_before)
            .unwrap()
            .is_none()
    );
    core.own_content_apply(&f.gid, &day(), &last).unwrap();
    assert_originals(&doc, &persisted(&core, &f.gid));
}

#[test]
fn native_range_is_not_a_byte_slice_and_truncated_halves_fail_without_frontier() {
    let f = Fixture::new();
    let doc = source(&f, f.dev);
    post(&f, &doc, f.dev, &"indivisible signed payload ".repeat(100));
    let up = range(&doc, doc.peer(), 0, 1);
    let core = forwarding_proofs::sibling(&f, [4; 32]);
    for broken in [&up[..up.len() / 2], &up[up.len() / 2..]] {
        assert!(core.own_content_apply(&f.gid, &day(), broken).is_err());
        assert_eq!(persisted(&core, &f.gid).vv(), VersionVector::default());
    }
    core.own_content_apply(&f.gid, &day(), &up).unwrap();
    assert_originals(&doc, &persisted(&core, &f.gid));
}

#[test]
fn valid_native_range_cannot_be_imported_into_another_group() {
    let f = Fixture::new();
    let doc = source(&f, f.dev);
    post(&f, &doc, f.dev, "bound to this group");
    let up = range(&doc, doc.peer(), 0, 1);
    let other = Fixture::new();
    let core = forwarding_proofs::sibling(&other, [4; 32]);
    assert!(core.own_content_apply(&other.gid, &day(), &up).is_err());
    assert_eq!(persisted(&core, &other.gid).vv(), VersionVector::default());
}
