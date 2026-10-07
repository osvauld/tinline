use super::doc::*;
use loro::{ExportMode, LoroDoc, LoroMap};

const DAY: &str = "2026-10-07";
const NOON: i64 = 1_791_374_400_000; // 2026-10-07 12:00:00 UTC

fn dev(n: u8) -> [u8; 32] {
    [n; 32]
}

fn msg(id: &str, author: &str, at: i64, text: &str) -> MsgRec {
    MsgRec { id: id.into(), author: author.into(), at, text: text.into(), edited_at: None, deleted: false, reply_to: None, file: None }
}

fn ctx(signer: &str, device: u8, live: bool) -> Ctx {
    Ctx { signer_did: signer.into(), signer_peer: peer_id(&dev(device), &doc_name("p", DAY)), now_ms: NOON, live, history: None }
}

fn shard(device: u8) -> Shard {
    Shard::new("p", DAY, &dev(device))
}

/// A raw doc seeded with `from`'s content, writing as `peer`; returns the update it produces.
fn forge(from: &Shard, peer: u64, f: impl FnOnce(&LoroMap)) -> Vec<u8> {
    let doc = LoroDoc::new();
    doc.import(&from.snapshot()).unwrap();
    doc.set_peer_id(peer).unwrap();
    let before = doc.oplog_vv();
    f(&doc.get_map("messages"));
    doc.commit();
    doc.export(ExportMode::updates(&before)).unwrap()
}

fn put(map: &LoroMap, id: &str, author: &str, at: i64, text: &str) {
    let e = map.insert_container(id, LoroMap::new()).unwrap();
    e.insert("id", id).unwrap();
    e.insert("author", author).unwrap();
    e.insert("at", at).unwrap();
    e.insert("text", text).unwrap();
}

#[test]
fn days_and_pairs() {
    assert_eq!(day_of(NOON), DAY);
    assert_eq!(day_of(0), "1970-01-01");
    assert_eq!(day_start(DAY), Some(NOON - 12 * HOUR_MS));
    assert_eq!(day_start("2026-02-30"), None);
    assert_eq!(day_start("nope"), None);
    assert_eq!(prev_day("2026-03-01").unwrap(), "2026-02-28");
    assert_eq!(pair_id("did:a", "did:b"), pair_id("did:b", "did:a"));
    assert_ne!(pair_id("did:a", "did:b"), pair_id("did:a", "did:c"));
    assert_ne!(peer_id(&dev(1), "dm/p/x"), peer_id(&dev(2), "dm/p/x"));
    assert_ne!(peer_id(&dev(1), "dm/p/x"), peer_id(&dev(1), "dm/p/y"));
}

#[test]
fn honest_batches_apply_once_and_converge() {
    let (mut a, mut b) = (shard(1), shard(2));
    let u = a.add_message(&msg("m1", "A", NOON, "hello")).unwrap();
    let r = b.apply_remote(&u, &ctx("A", 1, true)).unwrap();
    assert_eq!(r.added, vec!["m1".to_string()]);
    // Replay is a no-op.
    assert!(b.apply_remote(&u, &ctx("A", 1, true)).unwrap().is_empty());
    // Concurrent writes on both sides merge to the same state, ordered by (at, id).
    let ua = a.add_message(&msg("m2", "A", NOON + 5, "from a")).unwrap();
    let ub = b.add_message(&msg("m3", "B", NOON + 5, "from b")).unwrap();
    a.apply_remote(&ub, &ctx("B", 2, true)).unwrap();
    b.apply_remote(&ua, &ctx("A", 1, true)).unwrap();
    let order = |s: &Shard| {
        let mut v: Vec<_> = s.messages().unwrap().into_values().collect();
        v.sort_by(|x, y| x.order().cmp(&y.order()));
        v.into_iter().map(|m| m.id).collect::<Vec<_>>()
    };
    assert_eq!(order(&a), vec!["m1", "m2", "m3"]);
    assert_eq!(order(&a), order(&b));
    // Catch-up via vv: only own ops beyond what the other holds.
    let none = loro::VersionVector::default();
    assert!(a.export_own_since(&b.vv()).is_none());
    let full = a.export_own_since(&none).unwrap();
    let mut fresh = shard(2);
    fresh.apply_remote(&full, &ctx("A", 1, false)).unwrap();
    assert_eq!(fresh.messages().unwrap().len(), 2);
}

#[test]
fn edits_and_deletes_by_the_author_only() {
    let (mut a, mut b) = (shard(1), shard(2));
    let u = a.add_message(&msg("m1", "A", NOON, "hello")).unwrap();
    b.apply_remote(&u, &ctx("A", 1, true)).unwrap();
    let e = a.edit("m1", "hello!", NOON + 1000).unwrap();
    let r = b.apply_remote(&e, &ctx("A", 1, true)).unwrap();
    assert_eq!(r.changed, vec!["m1".to_string()]);
    assert_eq!(b.messages().unwrap()["m1"].text, "hello!");
    let d = a.delete("m1", NOON + 2000).unwrap();
    b.apply_remote(&d, &ctx("A", 1, true)).unwrap();
    let m = &b.messages().unwrap()["m1"];
    assert!(m.deleted && m.text.is_empty());
    // B tries to edit A's message: dropped.
    let evil = b.edit("m1", "mine now", NOON + 3000).unwrap();
    let mut a2 = shard(1);
    a2.apply_remote(&u, &ctx("A", 1, false)).unwrap();
    assert!(matches!(a2.apply_remote(&evil, &ctx("B", 2, true)), Err(Reject::NotYours(_)) | Err(Reject::Pending)));
    assert_eq!(a2.messages().unwrap()["m1"].text, "hello");
}

#[test]
fn forged_batches_are_rejected_and_not_stored() {
    let mut a = shard(1);
    let before = a.snapshot();
    let b = shard(2);
    let pb = peer_id(&dev(2), &doc_name("p", DAY));
    let pa = peer_id(&dev(1), &doc_name("p", DAY));
    let seed = shard(9);

    // 1. Ops carry B's peer id but arrive signed by device 3 (a third device).
    let u = b.add_message(&msg("m1", "B", NOON, "hi")).unwrap();
    assert_eq!(a.apply_remote(&u, &ctx("B", 3, true)), Err(Reject::ForeignPeer));
    // 2. Right peer id, but `author` names someone else.
    let u = forge(&seed, pb, |m| put(m, "m2", "A", NOON, "I am A"));
    assert_eq!(a.apply_remote(&u, &ctx("B", 2, true)), Err(Reject::WrongAuthor));
    // 3. `at` outside the shard's day, and (live) far from now.
    let u = forge(&seed, pb, |m| put(m, "m3", "B", NOON - 3 * DAY_MS, "old"));
    assert_eq!(a.apply_remote(&u, &ctx("B", 2, true)), Err(Reject::OutOfRange));
    let mut far = ctx("B", 2, true);
    far.now_ms = NOON + 2 * DAY_MS;
    let u = forge(&seed, pb, |m| put(m, "m4", "B", NOON, "stale"));
    assert_eq!(a.apply_remote(&u, &far), Err(Reject::OutOfRange));
    // 4. A foreign root container.
    let u = forge(&seed, pb, |_| {});
    let _ = u;
    let doc = LoroDoc::new();
    doc.set_peer_id(pb).unwrap();
    doc.get_map("other").insert("x", 1).unwrap();
    doc.commit();
    let u = doc.export(ExportMode::all_updates()).unwrap();
    assert_eq!(a.apply_remote(&u, &ctx("B", 2, true)), Err(Reject::Structure));
    // 5. Unknown field in a message.
    let u = forge(&seed, pb, |m| {
        put(m, "m5", "B", NOON, "x");
        if let Some(loro::ValueOrContainer::Container(loro::Container::Map(e))) = m.get("m5") {
            e.insert("admin", true).unwrap();
        }
    });
    assert!(matches!(a.apply_remote(&u, &ctx("B", 2, true)), Err(Reject::Malformed(_))));
    // 6. Garbage.
    assert_eq!(a.apply_remote(b"not loro", &ctx("B", 2, true)), Err(Reject::Undecodable));
    // Nothing was stored by any of it.
    assert_eq!(a.snapshot().len(), before.len());
    assert!(a.messages().unwrap().is_empty());

    // 7. Immutable fields: A rewrites the `at` and `author` of its own message.
    let mut b2 = shard(2);
    let u = a.add_message(&msg("m6", "A", NOON, "mine")).unwrap();
    b2.apply_remote(&u, &ctx("A", 1, true)).unwrap();
    let u = forge(&a, pa, |m| {
        if let Some(loro::ValueOrContainer::Container(loro::Container::Map(e))) = m.get("m6") {
            e.insert("at", NOON + 10).unwrap();
        }
    });
    assert_eq!(b2.apply_remote(&u, &ctx("A", 1, true)), Err(Reject::Immutable("m6".into())));
    let u = forge(&a, pa, |m| {
        if let Some(loro::ValueOrContainer::Container(loro::Container::Map(e))) = m.get("m6") {
            e.insert("author", "B").unwrap();
        }
    });
    assert_eq!(b2.apply_remote(&u, &ctx("A", 1, true)), Err(Reject::Immutable("m6".into())));
    assert_eq!(b2.messages().unwrap()["m6"].at, NOON);
}

#[test]
fn snapshot_round_trip_keeps_the_peer_id() {
    let a = shard(1);
    a.add_message(&msg("m1", "A", NOON, "x")).unwrap();
    let c = a.my_counter();
    let back = Shard::load("p", DAY, &dev(1), Some(&a.snapshot()), &[]).unwrap();
    assert_eq!(back.peer, a.peer);
    back.add_message(&msg("m2", "A", NOON + 1, "y")).unwrap();
    assert!(back.my_counter() > c);
    assert_eq!(back.messages().unwrap().len(), 2);
}
