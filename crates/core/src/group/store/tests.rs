use super::super::doc::{DocId, GroupDoc};
use super::super::record::*;
use super::*;
use loro::VersionVector;

// KIND: unit (G1 stage 1) — the meta doc is stored sealed and reloads intact.
#[test]
fn the_doc_is_sealed_at_rest_and_reloads() {
    let a = identity::generate().0;
    let s = GroupStore::in_memory("alpha beta");
    let dev = [1; 32];
    let peer = crate::chat::doc::writer_id(a.did(), &dev);
    let doc = GroupDoc::new("g", peer);
    let mut meta = GroupMeta::default();
    for i in 0..70 {
        let name = format!("secret text {i}");
        let rec = Record {
            group: "g".into(),
            author: a.did().into(),
            dev,
            at: i,
            seen: doc.seen(),
            body: Body::Genesis {
                name,
                nonce: [0; 16],
                owner_name: String::new(),
            },
        };
        let (_, up) = doc.write(&a, &rec).unwrap();
        s.append_meta("g", &mut meta, &up, || doc.snapshot()).unwrap();
    }
    for row in s.raw_rows() {
        assert!(!row.windows(11).any(|w| w == b"secret text"), "plaintext at rest");
    }
    let (snap, ups) = s.meta_rows("g").unwrap();
    assert!(snap.is_some(), "compacted once");
    let back = GroupDoc::load("g", peer, snap.as_deref(), &ups).unwrap();
    assert_eq!(back.entries().unwrap().len(), 70);
    assert_eq!(s.groups().unwrap().len(), 1);
    // Another phrase cannot read it.
    let other = GroupStore {
        db: Sealed::new(s.db.raw().clone(), key_from_phrase("gamma")),
        fail_next_append: Default::default(),
        fail_next_meta_write: Default::default(),
    };
    assert!(other.meta_rows("g").is_err());
}

// KIND: unit (group shards) — day docs are stored sealed per day, listed in the day index,
// compacted to one snapshot row and reload intact; forgetting the group drops them.
#[test]
fn day_docs_are_sealed_indexed_and_compact() {
    let a = identity::generate().0;
    let s = GroupStore::in_memory("alpha beta");
    let dev = [1; 32];
    let peer = crate::chat::doc::writer_id(a.did(), &dev);
    let day = || DocId::Day("1970-01-01".into());
    let doc = GroupDoc::new_in("g", day(), peer);
    let mut sm = ShardMeta::default();
    for i in 0..70 {
        let text = format!("secret text {i}");
        let rec = Record {
            group: "g".into(),
            author: a.did().into(),
            dev,
            at: i,
            seen: vec![],
            body: Body::Post {
                text,
                reply_to: None,
            },
        };
        let (_, up) = doc.write(&a, &rec).unwrap();
        s.append_shard("g", "1970-01-01", &mut sm, doc.vv().encode(), &up, || doc.snapshot(), None).unwrap();
    }
    assert_eq!(s.days("g").unwrap(), vec!["1970-01-01".to_string()]);
    assert!(s.days("h").unwrap().is_empty());
    for row in s.raw_rows() {
        assert!(!row.windows(11).any(|w| w == b"secret text"), "plaintext at rest");
    }
    let stored = s.shard_meta("g", "1970-01-01").unwrap().unwrap();
    assert!(!stored.closed && VersionVector::decode(&stored.vv).unwrap() == doc.vv());
    let (snap, ups) = s.shard_rows("g", "1970-01-01").unwrap();
    assert!(snap.is_some() && !ups.is_empty(), "compacted once, then updates again");
    // Closing: one snapshot row, no updates.
    s.compact_shard("g", "1970-01-01", &mut sm, &doc.snapshot()).unwrap();
    let (snap, ups) = s.shard_rows("g", "1970-01-01").unwrap();
    assert!(ups.is_empty() && s.shard_meta("g", "1970-01-01").unwrap().unwrap().closed);
    let back = GroupDoc::load_in("g", day(), peer, snap.as_deref(), &ups).unwrap();
    assert_eq!(back.entries_trusted().unwrap().len(), 70);
    // A later update opens it again.
    let rec = Record {
        group: "g".into(),
        author: a.did().into(),
        dev,
        at: 99,
        seen: vec![],
        body: Body::Post {
            text: "late".into(),
            reply_to: None,
        },
    };
    let (_, up) = doc.write(&a, &rec).unwrap();
    s.append_shard("g", "1970-01-01", &mut sm, doc.vv().encode(), &up, || doc.snapshot(), None).unwrap();
    assert!(!s.shard_meta("g", "1970-01-01").unwrap().unwrap().closed);
    s.forget("g").unwrap();
    assert!(s.days("g").unwrap().is_empty() && s.shard_meta("g", "1970-01-01").unwrap().is_none());
}
