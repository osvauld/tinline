//! The batch rules of `doc.rs` (P9c, U18: strict, no transition). Every message version carries
//! its author's DID signature and the writing install's device ID as record fields, bound to the
//! span of ops that wrote it; every op of every batch must lie in such a proven span under the
//! writer ID `writer_id(author, device ID)`. Device keys play no part.
//!
//! Every test carries one label: `KIND: invariant` (holds and must keep holding) or
//! `KIND: limitation` (an accepted or policy-gated gap, stated in its comment).

use super::doc::*;
use loro::{ExportMode, LoroDoc, LoroMap};

const DAY: &str = "2026-10-07";
const NOON: i64 = 1_791_374_400_000; // 2026-10-07 12:00:00 UTC

/// Accounts with real `did:key` DIDs (0 and 1 talk; 2 is a stranger).
fn acct(n: usize) -> &'static identity::Identity {
    static IDS: std::sync::OnceLock<Vec<identity::Identity>> = std::sync::OnceLock::new();
    &IDS.get_or_init(|| (0..3).map(|_| identity::generate().0).collect())[n]
}

fn did(n: usize) -> String {
    acct(n).did().to_string()
}

/// A device ID (random per install in the app; fixed here).
fn val(v: u8) -> [u8; 32] {
    [100 + v; 32]
}

fn wid(n: usize, v: u8) -> u64 {
    writer_id(&did(n), &val(v))
}

/// Account `n` on the install with device ID `v`.
fn w(n: usize, v: u8) -> Writer {
    Writer::new(acct(n).clone(), val(v))
}

/// The shard of account `n` on install `v` (it writes under its writer ID).
fn shard(n: usize, v: u8) -> Shard {
    Shard::new("p", DAY, wid(n, v))
}

/// A receiver that only reads in these tests.
fn recv() -> Shard {
    Shard::new("p", DAY, 1)
}

fn name() -> String {
    doc_name("p", DAY)
}

fn msg(id: &str, author: &str, at: i64, text: &str) -> MsgRec {
    MsgRec { id: id.into(), author: author.into(), at, text: text.into(), edited_at: None, deleted: false, reply_to: None, file: None, dev: [0; 32], did_sig: Vec::new() }
}

/// A live push from account `signer`.
fn live(signer: usize) -> Ctx {
    Ctx { signer_did: did(signer), now_ms: NOON, live: true, history: None }
}

/// A catch-up `Sync` from account `signer` (no 24 h window).
fn sync(signer: usize) -> Ctx {
    Ctx { live: false, ..live(signer) }
}

/// A vouched history snapshot from the contact `contact`, received by `me`.
fn hist(contact: usize, me: usize) -> Ctx {
    Ctx { signer_did: did(contact), now_ms: NOON, live: false, history: Some(did(me)) }
}

/// Own-device sync of account `me` about the conversation with `contact` (`chat_own_apply`).
fn own(me: usize, contact: usize) -> Ctx {
    Ctx { signer_did: did(me), now_ms: NOON, live: false, history: Some(did(contact)) }
}

/// A raw doc seeded with `from`'s content, writing as `peer`.
fn seeded(from: &Shard, peer: u64) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.import(&from.snapshot()).unwrap();
    doc.set_peer_id(peer).unwrap();
    doc
}

/// Raw ops under `peer` on a doc seeded with `from`; returns the update they make.
fn forge(from: &Shard, peer: u64, f: impl FnOnce(&LoroMap)) -> Vec<u8> {
    let doc = seeded(from, peer);
    let before = doc.oplog_vv();
    f(&doc.get_map("messages"));
    doc.commit();
    doc.export(ExportMode::updates(&before)).unwrap()
}

fn entry(map: &LoroMap, id: &str) -> Option<LoroMap> {
    match map.get(id) {
        Some(loro::ValueOrContainer::Container(loro::Container::Map(m))) => Some(m),
        _ => None,
    }
}

fn set(map: &LoroMap, id: &str, k: &str, v: impl Into<loro::LoroValue>) {
    entry(map, id).unwrap().insert(k, v).unwrap();
}

/// Writes `rec` under `peer` (a doc seeded with `from`) as one span the way a writer does: the
/// immutable fields if `from` does not hold it, then the version under `peer`'s version key with
/// `did_sig = sign(rec, where the span really is)`.
fn span(from: &Shard, peer: u64, rec: &MsgRec, sign: impl FnOnce(&MsgRec, &WrittenAt) -> Vec<u8>) -> Vec<u8> {
    let doc = seeded(from, peer);
    let before = doc.oplog_vv();
    let map = doc.get_map("messages");
    let e = match entry(&map, &rec.id) {
        Some(e) => e,
        None => {
            let e = map.insert_container(&rec.id, LoroMap::new()).unwrap();
            e.insert("id", rec.id.as_str()).unwrap();
            e.insert("author", rec.author.as_str()).unwrap();
            e.insert("at", rec.at).unwrap();
            e
        }
    };
    let at = WrittenAt { peer, counter: before.get(&peer).copied().unwrap_or(0), len: doc.get_pending_txn_len() as u32 + 1 };
    e.insert(&version_key(peer), version_value(rec, &sign(rec, &at))).unwrap();
    assert_eq!(doc.get_pending_txn_len() as u32, at.len);
    doc.commit();
    doc.export(ExportMode::updates(&before)).unwrap()
}

/// Signed by account `n`'s DID key, as a writer signs.
fn by(n: usize) -> impl FnOnce(&MsgRec, &WrittenAt) -> Vec<u8> {
    move |r, at| sign_msg(acct(n), &name(), r, at)
}

/// `rec` written by account `n` on install `v`.
fn rec_of(id: &str, n: usize, v: u8, at: i64, text: &str) -> MsgRec {
    MsgRec { dev: val(v), ..msg(id, &did(n), at, text) }
}

/// The peer ids an update holds ops of.
fn peers_of(u: &[u8]) -> Vec<u64> {
    LoroDoc::decode_import_blob_meta(u, true).unwrap().partial_end_vv.keys().copied().collect()
}

// ---- basics ------------------------------------------------------------------------------------

// KIND: invariant
#[test]
fn days_and_pairs() {
    assert_eq!(day_of(NOON), DAY);
    assert_eq!(day_of(0), "1970-01-01");
    assert_eq!(day_start(DAY), Some(NOON - 12 * HOUR_MS));
    assert_eq!(day_start("2026-02-30"), None);
    assert_eq!(day_start("nope"), None);
    assert_eq!(pair_id("did:a", "did:b"), pair_id("did:b", "did:a"));
    assert_ne!(pair_id("did:a", "did:b"), pair_id("did:a", "did:c"));
}

// KIND: invariant
#[test]
fn writer_ids_are_domain_separated_and_derived_from_no_device_key() {
    // Deterministic, and it changes with the DID and with the device ID.
    assert_eq!(wid(0, 1), writer_id(&did(0), &val(1)));
    assert_ne!(wid(0, 1), wid(1, 1));
    assert_ne!(wid(0, 1), wid(0, 2));
    // The DID is length-prefixed: moving bytes between DID and device ID gives another ID.
    let mut v = [0u8; 32];
    v[0] = b'b';
    let mut x = [0u8; 32];
    x[..31].copy_from_slice(&v[1..]);
    assert_ne!(writer_id("a", &v), writer_id("ab", &x));
    // Its own domain, not the plain hash.
    let h = blake3::hash(&[did(0).as_bytes(), &val(1)].concat());
    assert_ne!(wid(0, 1), u64::from_le_bytes(h.as_bytes()[..8].try_into().unwrap()));
    // A writer writes under it.
    assert_eq!(w(0, 1).writer_id(), wid(0, 1));
    // Fresh device IDs are random.
    assert_ne!(new_device_value(), new_device_value());
}

// KIND: invariant
#[test]
fn did_signatures_are_domain_separated_and_bound_to_doc_content_device_and_span() {
    let rec = rec_of("m1", 0, 1, NOON, "x");
    let at = WrittenAt { peer: wid(0, 1), counter: 3, len: 4 };
    let s = sign_msg(acct(0), &name(), &rec, &at);
    let signed = MsgRec { did_sig: s.clone(), ..rec.clone() };
    assert!(verify_msg(&name(), &signed, &at));
    assert!(!verify_msg(&doc_name("q", DAY), &signed, &at));
    assert!(!verify_msg(&name(), &MsgRec { text: "y".into(), ..signed.clone() }, &at));
    assert!(!verify_msg(&name(), &MsgRec { author: did(1), ..signed.clone() }, &at));
    // Bound to the device ID and to the span that writes the version.
    assert!(!verify_msg(&name(), &MsgRec { dev: val(2), ..signed.clone() }, &at));
    for other in [WrittenAt { peer: at.peer ^ 1, ..at }, WrittenAt { counter: 4, ..at }, WrittenAt { len: 5, ..at }] {
        assert!(!verify_msg(&name(), &signed, &other));
    }
    // Another account's key, a device key over the same bytes, or the P9b domain: no.
    assert!(!verify_msg(&name(), &MsgRec { did_sig: sign_msg(acct(1), &name(), &rec, &at), ..rec.clone() }, &at));
    let device = proto::new_device_secret();
    let by_device = cryptography::signature::sign(&device, &msg_sig_input(&name(), &rec, &at)).to_vec();
    assert!(!verify_msg(&name(), &MsgRec { did_sig: by_device, ..rec.clone() }, &at));
    let mut p9b = b"tinline-chat-msg-did-v2".to_vec();
    let input = msg_sig_input(&name(), &rec, &at);
    p9b.extend_from_slice(&input[b"tinline-chat-msg-v3".len()..]);
    assert!(!verify_msg(&name(), &MsgRec { did_sig: acct(0).sign(&p9b).to_vec(), ..rec.clone() }, &at));
    // An author that is not a `did:key` never verifies.
    assert!(!verify_msg(&name(), &MsgRec { author: "A".into(), ..signed }, &at));
}

// KIND: invariant (P9c)
// The DID signature and the device ID are record fields (of the version value the writer files
// under its own version key, `v:<writer ID>`); the record has no device signature and no change
// carries a commit message. The writer ID is the only
// peer id in the doc.
#[test]
fn records_carry_the_did_signature_and_device_id_as_fields() {
    let a = shard(0, 1);
    let u = a.add_message(&msg("m1", &did(0), NOON, "hi"), &w(0, 1)).unwrap();
    a.edit("m1", "hi!", NOON + 10, &w(0, 1)).unwrap();
    assert_eq!(a.peer(), wid(0, 1));
    assert_eq!(peers_of(&u), vec![wid(0, 1)]);
    let doc = LoroDoc::new();
    doc.import(&a.snapshot()).unwrap();
    let loro::LoroValue::Map(top) = doc.get_deep_value() else { panic!() };
    let loro::LoroValue::Map(msgs) = &top["messages"] else { panic!() };
    let loro::LoroValue::Map(m1) = &msgs["m1"] else { panic!() };
    let mut keys: Vec<&str> = m1.keys().map(|k| k.as_str()).collect();
    keys.sort();
    let vk = version_key(wid(0, 1));
    assert_eq!(keys, vec!["at", "author", "id", vk.as_str()]);
    let loro::LoroValue::Map(v) = &m1[&vk] else { panic!() };
    let mut keys: Vec<&str> = v.keys().map(|k| k.as_str()).collect();
    keys.sort();
    assert_eq!(keys, vec!["deleted", "dev", "did_sig", "edited_at", "text"]);
    let rec = &a.messages().unwrap()["m1"];
    assert_eq!(rec.dev, val(1));
    let json = a.export_all_since(&loro::VersionVector::default()).unwrap();
    let mut b = recv();
    b.apply_remote(&json, &hist(0, 1)).unwrap();
    let changes = {
        let d = LoroDoc::new();
        d.import(&json).unwrap();
        d.export_json_updates_without_peer_compression(&Default::default(), &d.oplog_vv()).changes
    };
    assert!(changes.iter().all(|c| c.msg.is_none()), "no commit messages");
    assert_eq!(b.messages().unwrap()["m1"].text, "hi!");
}

// ---- honest traffic ----------------------------------------------------------------------------

// KIND: invariant
#[test]
fn honest_batches_apply_once_and_converge() {
    let (mut a, mut b) = (shard(0, 1), shard(1, 2));
    let u = a.add_message(&msg("m1", &did(0), NOON, "hello"), &w(0, 1)).unwrap();
    let r = b.apply_remote(&u, &live(0)).unwrap();
    assert_eq!(r.added, vec!["m1".to_string()]);
    // Replay is a no-op.
    assert_eq!(b.apply_remote(&u, &live(0)).unwrap(), Applied::default());
    // Concurrent writes on both sides merge to the same state, ordered by (at, id).
    let ua = a.add_message(&msg("m2", &did(0), NOON + 5, "from a"), &w(0, 1)).unwrap();
    let ub = b.add_message(&msg("m3", &did(1), NOON + 5, "from b"), &w(1, 2)).unwrap();
    a.apply_remote(&ub, &live(1)).unwrap();
    b.apply_remote(&ua, &live(0)).unwrap();
    let order = |s: &Shard| {
        let mut v: Vec<_> = s.messages().unwrap().into_values().collect();
        v.sort_by(|x, y| x.order().cmp(&y.order()));
        v.into_iter().map(|m| m.id).collect::<Vec<_>>()
    };
    assert_eq!(order(&a), vec!["m1", "m2", "m3"]);
    assert_eq!(order(&a), order(&b));
    // Catch-up via vv: only own ops beyond what the other holds.
    assert!(a.export_own_since(&b.vv(), &did(0)).is_none());
    let full = a.export_own_since(&loro::VersionVector::default(), &did(0)).unwrap();
    assert_eq!(peers_of(&full), vec![wid(0, 1)]);
    let mut fresh = shard(1, 3);
    fresh.apply_remote(&full, &sync(0)).unwrap();
    assert_eq!(fresh.messages().unwrap().len(), 2);
}

// KIND: invariant
#[test]
fn edits_and_deletes_by_the_author_only() {
    let (a, mut b) = (shard(0, 1), shard(1, 2));
    let u = a.add_message(&msg("m1", &did(0), NOON, "hello"), &w(0, 1)).unwrap();
    b.apply_remote(&u, &live(0)).unwrap();
    let e = a.edit("m1", "hello!", NOON + 1000, &w(0, 1)).unwrap();
    assert_eq!(b.apply_remote(&e, &live(0)).unwrap().changed, vec!["m1".to_string()]);
    assert_eq!(b.messages().unwrap()["m1"].text, "hello!");
    // B edits A's message (signing with its own key): refused.
    let evil = b.edit("m1", "mine now", NOON + 1500, &w(1, 2)).unwrap();
    a.delete("m1", NOON + 2000, &w(0, 1)).unwrap();
    let mut b2 = shard(1, 3);
    b2.apply_remote(&a.snapshot(), &sync(0)).unwrap();
    let m = &b2.messages().unwrap()["m1"];
    assert!(m.deleted && m.text.is_empty());
    let mut a2 = shard(0, 1);
    a2.apply_remote(&u, &sync(0)).unwrap();
    assert_eq!(a2.apply_remote(&evil, &live(1)), Err(Reject::Pending));
    a2.apply_remote(&e, &sync(0)).unwrap();
    assert_eq!(a2.apply_remote(&evil, &live(1)), Err(Reject::NotYours("m1".into())));
    assert_eq!(a2.messages().unwrap()["m1"].text, "hello!");
}

// KIND: invariant
#[test]
fn snapshot_round_trip_keeps_the_writer_id() {
    let a = shard(0, 1);
    a.add_message(&msg("m1", &did(0), NOON, "x"), &w(0, 1)).unwrap();
    let c = a.my_counter();
    let back = Shard::load("p", DAY, wid(0, 1), Some(&a.snapshot()), &[]).unwrap();
    assert_eq!(back.peer(), wid(0, 1));
    back.add_message(&msg("m2", &did(0), NOON + 1, "y"), &w(0, 1)).unwrap();
    assert!(back.my_counter() > c);
    assert_eq!(back.messages().unwrap().len(), 2);
}

// KIND: invariant
// Loro merges adjacent changes of one peer (locally, and on import when nothing tells them
// apart). The proof is per span of ops, not per change, so merging changes nothing: a third
// device accepts a copy re-exported by a device whose Loro merged them.
#[test]
fn merged_loro_changes_do_not_matter() {
    let a = shard(0, 1);
    let mut ups = Vec::new();
    for i in 0..4 {
        ups.push(a.add_message(&msg(&format!("m{i}"), &did(0), NOON + i, "quick"), &w(0, 1)).unwrap());
    }
    a.edit("m1", "edited", NOON + 9, &w(0, 1)).unwrap();
    let mut b = recv();
    for u in &ups {
        b.apply_remote(u, &live(0)).unwrap();
    }
    b.apply_remote(&a.export_own_since(&b.vv(), &did(0)).unwrap(), &live(0)).unwrap();
    let mut c = recv();
    assert_eq!(c.apply_remote(&b.export_all_since(&Default::default()).unwrap(), &hist(0, 1)).unwrap().added.len(), 4);
    let mut d = recv();
    assert_eq!(d.apply_remote(&b.snapshot(), &hist(0, 1)).unwrap().added.len(), 4);
    assert_eq!(d.messages().unwrap(), a.messages().unwrap());
}

// KIND: invariant
// Two own devices (two device IDs, so two writer IDs) edit the same message concurrently: every
// replica ends on the same version, and that version is one of the two signed ones (the edits
// write the same keys in the same order, so one of them wins every key), which a third device
// accepts as history.
#[test]
fn concurrent_edits_on_two_own_devices_converge_on_one_signed_version() {
    let d1 = shard(0, 1);
    let u = d1.add_message(&msg("m1", &did(0), NOON, "v0"), &w(0, 1)).unwrap();
    let mut d3 = shard(0, 3);
    d3.apply_remote(&u, &own(0, 1)).unwrap();
    let mut d1 = d1;
    // (Same edit time: see the limitation test below for different ones.)
    let e1 = d1.edit("m1", "from 1", NOON + 10, &w(0, 1)).unwrap();
    let e3 = d3.edit("m1", "from 3", NOON + 10, &w(0, 3)).unwrap();
    let mut contact = shard(1, 2);
    contact.apply_remote(&u, &live(0)).unwrap();
    contact.apply_remote(&e3, &live(0)).unwrap();
    contact.apply_remote(&e1, &live(0)).unwrap();
    d1.apply_remote(&e3, &own(0, 1)).unwrap();
    d3.apply_remote(&e1, &own(0, 1)).unwrap();
    let v = d1.messages().unwrap();
    assert_eq!(d3.messages().unwrap(), v);
    assert_eq!(contact.messages().unwrap(), v);
    assert!(["from 1", "from 3"].contains(&v["m1"].text.as_str()));
    let mut third = recv();
    assert_eq!(third.apply_remote(&contact.snapshot(), &hist(0, 1)).unwrap().added, vec!["m1".to_string()]);
}

// KIND: invariant (P10; replaces the flaky limitation test
// `concurrent_edits_whose_loro_winner_is_older_are_refused`)
// Two own devices edit the same message concurrently with different edit times. Every replica,
// in either arrival order, accepts both edits and shows the newer one: competing versions are
// ordered by (deleted, edited_at, writer ID), never by Loro's internal winner (Lamport, then
// peer id), which made the old rollback rule refuse the batch on one replica only. Many device-ID
// pairs, so both Loro peer orders occur in every run.
#[test]
fn concurrent_edits_converge_on_the_newest_version_in_every_arrival_order() {
    for v in 0..16u8 {
        let (v1, v3) = (10 + 2 * v, 11 + 2 * v);
        let mut d1 = shard(0, v1);
        let u = d1.add_message(&msg("m1", &did(0), NOON, "v0"), &w(0, v1)).unwrap();
        let mut d3 = shard(0, v3);
        d3.apply_remote(&u, &own(0, 1)).unwrap();
        let e1 = d1.edit("m1", "older", NOON + 10, &w(0, v1)).unwrap();
        let e3 = d3.edit("m1", "newer", NOON + 11, &w(0, v3)).unwrap();
        for (first, second) in [(&e1, &e3), (&e3, &e1)] {
            let mut c = shard(1, 2);
            c.apply_remote(&u, &live(0)).unwrap();
            c.apply_remote(first, &live(0)).unwrap();
            assert_eq!(c.apply_remote(second, &live(0)).map(|_| ()), Ok(()), "pair {v}");
            assert_eq!(c.messages().unwrap()["m1"].text, "newer", "pair {v}");
            assert_eq!(c.messages().unwrap()["m1"].edited_at, Some(NOON + 11));
        }
        d1.apply_remote(&e3, &own(0, 1)).unwrap();
        d3.apply_remote(&e1, &own(0, 1)).unwrap();
        assert_eq!(d1.messages().unwrap(), d3.messages().unwrap());
        assert_eq!(d1.messages().unwrap()["m1"].text, "newer");
    }
}

// KIND: invariant (P10)
// The order of competing versions: a delete beats any concurrent edit, whatever its time; equal
// edit times go to the higher writer ID. Every replica agrees, in either arrival order, and a
// device's next edit supersedes whatever it shows.
#[test]
fn a_delete_beats_a_concurrent_edit_and_ties_go_to_the_higher_writer_id() {
    let base = |v1: u8, v3: u8| {
        let d1 = shard(0, v1);
        let u = d1.add_message(&msg("m1", &did(0), NOON, "v0"), &w(0, v1)).unwrap();
        let mut d3 = shard(0, v3);
        d3.apply_remote(&u, &own(0, 1)).unwrap();
        (d1, d3, u)
    };
    let both_orders = |u: &[u8], x: &[u8], y: &[u8]| {
        let mut out = Vec::new();
        for (first, second) in [(x, y), (y, x)] {
            let mut c = shard(1, 2);
            c.apply_remote(u, &live(0)).unwrap();
            c.apply_remote(first, &live(0)).unwrap();
            c.apply_remote(second, &live(0)).unwrap();
            out.push(c.messages().unwrap()["m1"].clone());
        }
        assert_eq!(out[0], out[1]);
        out.remove(0)
    };
    // A delete at NOON+10 against an edit at NOON+20: deleted.
    let (d1, d3, u) = base(1, 3);
    let del = d1.delete("m1", NOON + 10, &w(0, 1)).unwrap();
    let ed = d3.edit("m1", "later edit", NOON + 20, &w(0, 3)).unwrap();
    let m = both_orders(&u, &del, &ed);
    assert!(m.deleted && m.text.is_empty());
    // Equal edit times: the higher writer ID wins.
    let (d1, mut d3, u) = base(1, 3);
    let e1 = d1.edit("m1", "from 1", NOON + 10, &w(0, 1)).unwrap();
    let e3 = d3.edit("m1", "from 3", NOON + 10, &w(0, 3)).unwrap();
    let m = both_orders(&u, &e1, &e3);
    assert_eq!(m.text, if wid(0, 1) > wid(0, 3) { "from 1" } else { "from 3" });
    // The losing device's next edit, even with an older clock, wins everywhere.
    d3.apply_remote(&e1, &own(0, 1)).unwrap();
    let again = d3.edit("m1", "from 3 again", NOON + 5, &w(0, 3)).unwrap();
    let mut c = shard(1, 2);
    for b in [&u, &e1, &e3, &again] {
        c.apply_remote(b, &live(0)).unwrap();
    }
    assert_eq!(c.messages().unwrap()["m1"].text, "from 3 again");
    assert_eq!(d3.messages().unwrap(), c.messages().unwrap());
}

// KIND: invariant (P10)
// A writer's own versions never go back: an older edit time or an undone delete under its own
// version key is refused, whether the receiver held the newer version before or gets both in
// one batch (the verdict comes from the ops alone). No writer may write another's version key.
#[test]
fn a_writer_cannot_roll_back_its_own_version_or_write_another_writers_key() {
    let a = shard(0, 1);
    let u = a.add_message(&msg("m1", &did(0), NOON, "v0"), &w(0, 1)).unwrap();
    let e = a.edit("m1", "v1", NOON + 10, &w(0, 1)).unwrap();
    let back = span(&a, wid(0, 1), &MsgRec { text: "back".into(), edited_at: Some(NOON + 5), ..rec_of("m1", 0, 1, NOON, "") }, by(0));
    let mut c = shard(1, 2);
    c.apply_remote(&u, &live(0)).unwrap();
    c.apply_remote(&e, &live(0)).unwrap();
    assert_eq!(c.apply_remote(&back, &live(0)), Err(Reject::Immutable("m1".into())));
    let all = doc_with(&[&u, &e, &back], 99, |_| {});
    assert_eq!(shard(1, 2).apply_remote(&all, &live(0)), Err(Reject::Immutable("m1".into())));
    // An undone delete, even with a later edit time.
    let del = a.delete("m1", NOON + 20, &w(0, 1)).unwrap();
    let undo = span(&a, wid(0, 1), &MsgRec { text: "alive".into(), edited_at: Some(NOON + 30), ..rec_of("m1", 0, 1, NOON, "") }, by(0));
    c.apply_remote(&del, &live(0)).unwrap();
    assert_eq!(c.apply_remote(&undo, &live(0)), Err(Reject::Immutable("m1".into())));
    assert!(c.messages().unwrap()["m1"].deleted);
    assert_eq!(a.edit("m1", "again", NOON + 40, &w(0, 1)), Err(Reject::Immutable("m1".into())), "a writer does not edit a deleted message");
    // A span of writer 1 that files a version under writer 3's key.
    let other = {
        let d = seeded(&a, wid(0, 1));
        let before = d.oplog_vv();
        let r = MsgRec { edited_at: Some(NOON + 50), ..rec_of("m1", 0, 3, NOON, "as 3") };
        let at = WrittenAt { peer: wid(0, 1), counter: before.get(&wid(0, 1)).copied().unwrap_or(0), len: 1 };
        set(&d.get_map("messages"), "m1", &version_key(wid(0, 3)), version_value(&r, &sign_msg(acct(0), &name(), &r, &at)));
        d.commit();
        d.export(ExportMode::updates(&before)).unwrap()
    };
    assert_eq!(c.apply_remote(&other, &live(0)), Err(Reject::Unsigned));
}

// KIND: limitation (Loro; why P10 never rebuilds a day under the same writer ID)
// A day rebuilt from nothing under the same writer ID writes at (writer ID, counter) pairs the
// contact already holds; Loro takes them for ops it has and drops them, so the new message is
// silently lost there. Under a fresh writer ID it arrives.
#[test]
fn a_day_rebuilt_under_the_same_writer_id_is_silently_lost() {
    let a = shard(0, 1);
    let u1 = a.add_message(&msg("m1", &did(0), NOON, "first"), &w(0, 1)).unwrap();
    let mut b = shard(1, 2);
    b.apply_remote(&u1, &live(0)).unwrap();
    let rebuilt = shard(0, 1);
    let u2 = rebuilt.add_message(&msg("m2", &did(0), NOON + 1, "second"), &w(0, 1)).unwrap();
    assert_eq!(b.apply_remote(&u2, &live(0)), Ok(Applied::default()));
    assert!(!b.messages().unwrap().contains_key("m2"));
    let fresh = shard(0, 4);
    let u3 = fresh.add_message(&msg("m3", &did(0), NOON + 2, "third"), &w(0, 4)).unwrap();
    assert_eq!(b.apply_remote(&u3, &live(0)).unwrap().added, vec!["m3".to_string()]);
}

// KIND: invariant
// Two own devices write under their own writer IDs; a device edits a message the other created.
// (Which ops a device sends a contact: `a_device_sends_a_contact_every_op_of_ours_it_holds`.)
#[test]
fn two_own_devices_with_different_device_ids_converge() {
    assert_ne!(wid(0, 1), wid(0, 3));
    let mut d1 = shard(0, 1);
    let mut d3 = shard(0, 3);
    let mut contact = shard(1, 2);
    let u1 = d1.add_message(&msg("a1", &did(0), NOON, "from 1"), &w(0, 1)).unwrap();
    let u3 = d3.add_message(&msg("a3", &did(0), NOON + 1, "from 3"), &w(0, 3)).unwrap();
    contact.apply_remote(&u1, &live(0)).unwrap();
    contact.apply_remote(&u3, &live(0)).unwrap();
    d3.apply_remote(&d1.export_all_since(&d3.vv()).unwrap(), &own(0, 1)).unwrap();
    d1.apply_remote(&d3.export_all_since(&d1.vv()).unwrap(), &own(0, 1)).unwrap();
    let e = d3.edit("a1", "from 1, edited on 3", NOON + 10, &w(0, 3)).unwrap();
    assert_eq!(peers_of(&e), vec![wid(0, 3)]);
    contact.apply_remote(&e, &live(0)).unwrap();
    d1.apply_remote(&e, &own(0, 1)).unwrap();
    for s in [&d1, &d3, &contact] {
        assert_eq!(s.vv(), d3.vv());
        assert_eq!(s.messages().unwrap(), d3.messages().unwrap());
    }
    assert_eq!(contact.messages().unwrap()["a1"].text, "from 1, edited on 3");
    assert!(d1.export_own_since(&contact.vv(), &did(0)).is_none());
}

// KIND: invariant (P9c; replaces the P8/P9 restore tests)
// A restore is a new install: new device ID, new writer ID. Every message, written before or
// after it, is DID-signed, so all of them verify on a receiver that knows nothing about either
// install, live, as history and in own-device sync.
#[test]
fn messages_from_before_and_after_a_restore_verify_on_any_device() {
    let a = shard(0, 1);
    let u = a.add_message(&msg("m1", &did(0), NOON, "before"), &w(0, 1)).unwrap();
    let mut b = recv();
    b.apply_remote(&u, &live(0)).unwrap();
    let a2 = Shard::load("p", DAY, wid(0, 2), Some(&a.snapshot()), &[]).unwrap();
    a2.edit("m1", "before, edited after", NOON + 5, &w(0, 2)).unwrap();
    let u2 = a2.add_message(&msg("m2", &did(0), NOON + 6, "after"), &w(0, 2)).unwrap();
    assert_eq!(peers_of(&u2), vec![wid(0, 2)]);
    for ctx in [hist(0, 1), own(1, 0)] {
        let mut fresh = recv();
        let r = fresh.apply_remote(&a2.snapshot(), &ctx).unwrap();
        assert_eq!(r.added, vec!["m1".to_string(), "m2".to_string()]);
        assert_eq!(fresh.messages().unwrap(), a2.messages().unwrap());
    }
    b.apply_remote(&a2.export_own_since(&b.vv(), &did(0)).unwrap(), &live(0)).unwrap();
    assert_eq!(b.messages().unwrap(), a2.messages().unwrap());
}

// KIND: invariant (P10b)
// A device sends a contact every op of ours the contact lacks, whichever of our writer IDs wrote
// it (an earlier install of this device, before a restore; another of our devices), and never
// the contact's own ops. The contact takes them live: each is proven by our DID signature.
#[test]
fn a_device_sends_a_contact_every_op_of_ours_it_holds() {
    let mut contact = shard(1, 2);
    let r = contact.add_message(&msg("r1", &did(1), NOON, "from the contact"), &w(1, 2)).unwrap();
    // Before a restore: m1 delivered, m2 not.
    let mut old = shard(0, 1);
    old.apply_remote(&r, &live(1)).unwrap();
    contact.apply_remote(&old.add_message(&msg("m1", &did(0), NOON + 1, "delivered"), &w(0, 1)).unwrap(), &live(0)).unwrap();
    old.add_message(&msg("m2", &did(0), NOON + 2, "not delivered"), &w(0, 1)).unwrap();
    // After it: a new writer ID, nothing written yet. The undelivered m2 is still owed.
    let new = Shard::load("p", DAY, wid(0, 3), Some(&old.snapshot()), &[]).unwrap();
    assert_eq!(new.own_writers(&did(0)), [wid(0, 1), wid(0, 3)].into_iter().collect());
    let u = new.export_own_since(&contact.vv(), &did(0)).expect("m2 is owed");
    assert_eq!(peers_of(&u), vec![wid(0, 1)]);
    assert_eq!(contact.apply_remote(&u, &live(0)).unwrap().added, vec!["m2".to_string()]);
    assert!(new.export_own_since(&contact.vv(), &did(0)).is_none());
    // A sibling device holding our ops by own-device sync forwards them too; never the
    // contact's (a fresh contact lacking r1 gets only ours).
    let mut sib = shard(0, 4);
    sib.apply_remote(&old.snapshot(), &own(0, 1)).unwrap();
    let mut fresh = recv();
    let u = sib.export_own_since(&fresh.vv(), &did(0)).unwrap();
    assert_eq!(peers_of(&u), vec![wid(0, 1)]);
    assert_eq!(fresh.apply_remote(&u, &sync(0)), Err(Reject::Pending), "ours build on the contact's r1, which it must hold");
    let mut holds_r1 = recv();
    holds_r1.apply_remote(&r, &live(1)).unwrap();
    let u = sib.export_own_since(&holds_r1.vv(), &did(0)).unwrap();
    assert_eq!(peers_of(&u), vec![wid(0, 1)]);
    assert_eq!(holds_r1.apply_remote(&u, &sync(0)).unwrap().added, vec!["m1".to_string(), "m2".to_string()]);
}

// ---- forgeries ---------------------------------------------------------------------------------

// KIND: invariant
#[test]
fn forged_batches_are_rejected_and_not_stored() {
    let mut a = shard(0, 1);
    let before = a.snapshot();
    let seed = recv();
    let pb = wid(1, 2);
    // 1. B's session relays a genuine message of a third account's: not its own.
    let c = shard(2, 5);
    let u = c.add_message(&msg("c1", &did(2), NOON, "hi"), &w(2, 5)).unwrap();
    assert_eq!(a.apply_remote(&u, &live(1)), Err(Reject::WrongAuthor));
    // 2. B's own writer ID and signature, but `author` names A.
    let u = span(&seed, pb, &MsgRec { dev: val(2), ..msg("m2", &did(0), NOON, "I am A") }, by(1));
    assert_eq!(a.apply_remote(&u, &live(1)), Err(Reject::WrongAuthor));
    // 3. `at` outside the shard's day, and (live) far from now.
    let u = span(&seed, pb, &rec_of("m3", 1, 2, NOON - 3 * DAY_MS, "old"), by(1));
    assert_eq!(a.apply_remote(&u, &live(1)), Err(Reject::OutOfRange));
    let mut far = live(1);
    far.now_ms = NOON + 2 * DAY_MS;
    let u = span(&seed, pb, &rec_of("m4", 1, 2, NOON, "stale"), by(1));
    assert_eq!(a.apply_remote(&u, &far), Err(Reject::OutOfRange));
    // 4. A foreign root container.
    let doc = LoroDoc::new();
    doc.set_peer_id(pb).unwrap();
    doc.get_map("other").insert("x", 1).unwrap();
    doc.commit();
    let u = doc.export(ExportMode::all_updates()).unwrap();
    assert_eq!(a.apply_remote(&u, &live(1)), Err(Reject::Structure));
    // 5. An unknown field in a message.
    let u = doc_with(&[&span(&seed, pb, &rec_of("m5", 1, 2, NOON, "x"), by(1))], pb, |m| set(m, "m5", "admin", true));
    assert!(matches!(a.apply_remote(&u, &live(1)), Err(Reject::Malformed(_))));
    // 6. Garbage.
    assert_eq!(a.apply_remote(b"not loro", &live(1)), Err(Reject::Undecodable));
    // Nothing was stored by any of it.
    assert_eq!(a.snapshot().len(), before.len());
    assert!(a.messages().unwrap().is_empty());

    // 7. Immutable fields: A rewrites the `at` of its own message, validly signed.
    let mut b2 = shard(1, 2);
    let u = a.add_message(&msg("m6", &did(0), NOON, "mine"), &w(0, 1)).unwrap();
    b2.apply_remote(&u, &live(0)).unwrap();
    let moved = {
        let d = seeded(&a, wid(0, 1));
        let before = d.oplog_vv();
        let mut r = a.messages().unwrap()["m6"].clone();
        r.at = NOON + 10;
        let map = d.get_map("messages");
        set(&map, "m6", "at", r.at);
        let at = WrittenAt { peer: wid(0, 1), counter: before.get(&wid(0, 1)).copied().unwrap_or(0), len: d.get_pending_txn_len() as u32 + 1 };
        set(&map, "m6", &version_key(wid(0, 1)), version_value(&r, &sign_msg(acct(0), &name(), &r, &at)));
        d.commit();
        d.export(ExportMode::updates(&before)).unwrap()
    };
    assert_eq!(b2.apply_remote(&moved, &live(0)), Err(Reject::Immutable("m6".into())));
    assert_eq!(b2.messages().unwrap()["m6"].at, NOON);
}

/// A doc holding `updates`, plus raw ops `f` under `peer`; returns all of it (self-contained).
fn doc_with(updates: &[&[u8]], peer: u64, f: impl FnOnce(&LoroMap)) -> Vec<u8> {
    let doc = LoroDoc::new();
    for u in updates {
        doc.import(u).unwrap();
    }
    doc.set_peer_id(peer).unwrap();
    f(&doc.get_map("messages"));
    doc.commit();
    doc.export(ExportMode::all_updates()).unwrap()
}

// KIND: invariant (P9c, test-first)
// Device keys no longer sign chat content. A record shaped as before P9c (a device-key `sig`
// field, no `dev`/`did_sig`) is malformed; a record whose `did_sig` was made with a device key
// (over exactly the bytes a DID key signs) does not verify. Refused live, in a contact's
// history and in own-device sync: there is no acceptance path based on device keys.
#[test]
fn a_record_signed_only_by_a_device_key_is_refused() {
    let device = proto::new_device_secret();
    // Pre-P9c shape.
    let old = forge(&recv(), wid(1, 2), |m| {
        let e = m.insert_container("b1", LoroMap::new()).unwrap();
        e.insert("id", "b1").unwrap();
        e.insert("author", did(1).as_str()).unwrap();
        e.insert("at", NOON).unwrap();
        e.insert("text", "device-signed").unwrap();
        e.insert("sig", "00".repeat(64)).unwrap();
    });
    // Current shape, signed by a device key.
    let rec = rec_of("b2", 1, 2, NOON, "device-signed");
    let dev_signed = span(&recv(), wid(1, 2), &rec, |r, at| cryptography::signature::sign(&device, &msg_sig_input(&name(), r, at)).to_vec());
    for ctx in [live(1), sync(1), hist(1, 0), own(0, 1)] {
        let mut a = shard(0, 1);
        assert!(matches!(a.apply_remote(&old, &ctx), Err(Reject::Malformed(_))), "{ctx:?}");
        assert_eq!(a.apply_remote(&dev_signed, &ctx), Err(Reject::BadSignature), "{ctx:?}");
        assert!(a.vv().is_empty() && a.messages().unwrap().is_empty());
    }
    // Control: the same record signed by the DID key is accepted.
    let ok = span(&recv(), wid(1, 2), &rec, by(1));
    assert_eq!(shard(0, 1).apply_remote(&ok, &live(1)).unwrap().added, vec!["b2".to_string()]);
}

// KIND: invariant (P9c)
// Every op must lie in a span that ends with the version's `did_sig` and touches one message:
// trailing unsigned ops, a span over two messages, and a span whose signature is not its last
// op are refused, with no state change.
#[test]
fn every_op_must_belong_to_a_did_signed_span() {
    let a = shard(0, 1);
    a.add_message(&msg("m1", &did(0), NOON, "real"), &w(0, 1)).unwrap();
    let mut b = recv();
    b.apply_remote(&a.snapshot(), &live(0)).unwrap();
    let (snap, vv) = (b.snapshot(), b.vv());
    let unchanged = |s: &Shard| s.vv() == vv && s.snapshot() == snap;
    // 1. A genuine span followed by ops nobody signed (they leave the record as it reads).
    a.edit("m1", "v2", NOON + 1, &w(0, 1)).unwrap();
    let trailing = {
        let all = doc_with(&[&a.snapshot()], wid(0, 1), |m| touch(m, "m1"));
        let d = LoroDoc::new();
        d.import(&all).unwrap();
        d.export(ExportMode::updates(&vv)).unwrap()
    };
    for ctx in [live(0), hist(0, 1), own(1, 0)] {
        assert_eq!(b.apply_remote(&trailing, &ctx), Err(Reject::Unsigned), "{ctx:?}");
        assert!(unchanged(&b));
    }
    // 2. One span writing two messages, signed for the second one.
    let two = {
        let d = seeded(&recv(), wid(0, 1));
        let map = d.get_map("messages");
        for id in ["x1", "x2"] {
            let e = map.insert_container(id, LoroMap::new()).unwrap();
            e.insert("id", id).unwrap();
            e.insert("author", did(0).as_str()).unwrap();
            e.insert("at", NOON).unwrap();
        }
        let at = WrittenAt { peer: wid(0, 1), counter: 0, len: d.get_pending_txn_len() as u32 + 2 };
        for id in ["x1", "x2"] {
            let r = rec_of(id, 0, 1, NOON, "t");
            entry(&map, id).unwrap().insert(&version_key(wid(0, 1)), version_value(&r, &sign_msg(acct(0), &name(), &r, &at))).unwrap();
        }
        d.commit();
        d.export(ExportMode::all_updates()).unwrap()
    };
    assert_eq!(recv().apply_remote(&two, &live(0)), Err(Reject::Unsigned));
    // 3. The version first, an immutable field after it (the span does not end with `v`).
    let late = {
        let d = seeded(&recv(), wid(0, 1));
        let map = d.get_map("messages");
        let r = rec_of("y1", 0, 1, NOON, "t");
        let e = map.insert_container("y1", LoroMap::new()).unwrap();
        e.insert("id", "y1").unwrap();
        e.insert("author", did(0).as_str()).unwrap();
        e.insert("at", NOON).unwrap();
        let at = WrittenAt { peer: wid(0, 1), counter: 0, len: d.get_pending_txn_len() as u32 + 2 };
        e.insert(&version_key(wid(0, 1)), version_value(&r, &sign_msg(acct(0), &name(), &r, &at))).unwrap();
        e.insert("reply_to", "y0").unwrap();
        d.commit();
        d.export(ExportMode::all_updates()).unwrap()
    };
    assert_eq!(recv().apply_remote(&late, &live(0)), Err(Reject::Unsigned));
    // Control: the same span written the right way round.
    assert!(recv().apply_remote(&span(&recv(), wid(0, 1), &rec_of("y1", 0, 1, NOON, "t"), by(0)), &live(0)).is_ok());
}

// KIND: invariant
#[test]
fn forged_messages_in_vouched_snapshots_are_rejected() {
    // B (account 1, the malicious peer) forges a message "from A" (account 0, the receiver).
    // Under A's writer ID, signed with B's key: the signature fails.
    let u = span(&recv(), wid(0, 1), &rec_of("f1", 0, 1, NOON, "A says pay B"), by(1));
    assert_eq!(shard(0, 1).apply_remote(&u, &hist(1, 0)), Err(Reject::BadSignature));
    // Under B's own writer ID: not A's writer ID.
    let u = span(&recv(), wid(1, 2), &rec_of("f1", 0, 2, NOON, "A says pay B"), by(1));
    assert_eq!(shard(0, 1).apply_remote(&u, &hist(1, 0)), Err(Reject::ForeignPeer));
    // A forged message of B's own under a writer ID of another DID, or under a random peer id.
    for peer in [wid(0, 1), 77] {
        let u = span(&recv(), peer, &rec_of("f2", 1, 2, NOON, "x"), by(1));
        assert_eq!(shard(0, 1).apply_remote(&u, &hist(1, 0)), Err(Reject::ForeignPeer));
    }
    // An honest snapshot is accepted.
    let a = shard(0, 1);
    a.add_message(&msg("m1", &did(0), NOON, "real"), &w(0, 1)).unwrap();
    let b = Shard::load("p", DAY, wid(1, 2), Some(&a.snapshot()), &[]).unwrap();
    b.add_message(&msg("b1", &did(1), NOON + 1, "reply"), &w(1, 2)).unwrap();
    let mut fresh = shard(0, 3);
    assert_eq!(fresh.apply_remote(&b.snapshot(), &hist(1, 0)).unwrap().added, vec!["b1".to_string(), "m1".to_string()]);
}

// KIND: invariant
#[test]
fn altered_text_and_rollbacks_are_rejected() {
    let a = shard(0, 1);
    a.add_message(&msg("m1", &did(0), NOON, "real"), &w(0, 1)).unwrap();
    let old_snap = a.snapshot();
    a.edit("m1", "v2", NOON + 10, &w(0, 1)).unwrap();
    let mut victim = shard(1, 2);
    victim.apply_remote(&a.snapshot(), &hist(0, 1)).unwrap();
    // B keeps A's signature but swaps the text: under A's writer ID (the signature is for
    // another span and other text) and under its own (not A's writer ID).
    let real = victim.messages().unwrap()["m1"].clone();
    for (peer, want) in [(wid(0, 1), Reject::BadSignature), (wid(1, 2), Reject::ForeignPeer)] {
        let u = span(&victim, peer, &MsgRec { text: "pay B".into(), ..real.clone() }, |r, _| r.did_sig.clone());
        assert_eq!(victim.apply_remote(&u, &hist(0, 1)), Err(want));
    }
    assert_eq!(victim.messages().unwrap()["m1"].text, "v2");
    // Replaying A's older, validly signed version does not roll the edit back.
    let _ = victim.apply_remote(&old_snap, &hist(0, 1));
    assert_eq!(victim.messages().unwrap()["m1"].text, "v2");
}

// KIND: invariant
#[test]
fn signed_messages_do_not_replay_across_days_or_pairs() {
    let a = shard(0, 1);
    a.add_message(&msg("m1", &did(0), NOON, "hello"), &w(0, 1)).unwrap();
    let snap = a.snapshot();
    let mut c = hist(0, 1);
    c.now_ms = NOON + DAY_MS;
    // Verbatim replay into another day, and into another pair.
    let mut wrong_day = Shard::new("p", "2026-10-08", 1);
    assert!(wrong_day.apply_remote(&snap, &c).is_err());
    let mut wrong_pair = Shard::new("q", DAY, 1);
    assert_eq!(wrong_pair.apply_remote(&snap, &hist(0, 1)), Err(Reject::BadSignature));
    // The same signature on a copy whose time was moved into the other day's range.
    let r = a.messages().unwrap()["m1"].clone();
    let u = span(&recv(), wid(0, 1), &MsgRec { at: NOON + DAY_MS, ..r.clone() }, |_, _| r.did_sig.clone());
    assert_eq!(wrong_day.apply_remote(&u, &c), Err(Reject::BadSignature));
}

// ---- writer IDs ----------------------------------------------------------------------------------

/// Ops on the record `id` that leave it as it reads (a field set and removed again).
fn touch(map: &LoroMap, id: &str) {
    let e = entry(map, id).unwrap();
    e.insert("x", 1).unwrap();
    e.delete("x").unwrap();
}

// KIND: invariant
#[test]
fn forged_writer_ids_are_refused_without_any_state_change() {
    let a = shard(0, 1);
    a.add_message(&msg("m1", &did(0), NOON, "real"), &w(0, 1)).unwrap();
    let mut b = shard(1, 2);
    b.apply_remote(&a.snapshot(), &live(0)).unwrap();
    let (snap, vv) = (b.snapshot(), b.vv());
    let unchanged = |s: &Shard| s.vv() == vv && s.snapshot() == snap && s.messages().unwrap().len() == 1;
    // 1. A post whose peer id is not `writer_id(author, dev)` for the device ID it carries.
    let u = span(&a, wid(0, 2), &rec_of("m2", 0, 1, NOON + 1, "x"), by(0));
    assert_eq!(b.apply_remote(&u, &live(0)), Err(Reject::ForeignPeer));
    assert!(unchanged(&b));
    // 2. Ops under the writer ID that no signature covers.
    let u = forge(&a, wid(0, 1), |m| touch(m, "m1"));
    for ctx in [live(0), hist(0, 1)] {
        assert_eq!(b.apply_remote(&u, &ctx), Err(Reject::Unsigned));
        assert!(unchanged(&b));
    }
    // 3. The genuine signature replayed on another span (it re-writes the same version).
    let real = a.messages().unwrap()["m1"].clone();
    let u = span(&a, wid(0, 1), &MsgRec { edited_at: Some(0), ..real.clone() }, |_, _| real.did_sig.clone());
    for ctx in [live(0), hist(0, 1)] {
        assert_eq!(b.apply_remote(&u, &ctx), Err(Reject::BadSignature));
        assert!(unchanged(&b));
    }
    // Control: the genuine next post of account 0 is accepted.
    let ok = a.add_message(&msg("m5", &did(0), NOON + 5, "next"), &w(0, 1)).unwrap();
    assert_eq!(b.apply_remote(&ok, &live(0)).unwrap().added, vec!["m5".to_string()]);
}

// KIND: invariant
// The contact (account 1) posts its own, validly DID-signed message under a writer ID of
// account 0: with its own device ID or with account 0's copied, the hash does not give that ID.
// No memory of earlier batches is needed for that (no `known_writers` any more).
#[test]
fn cross_did_copying_of_a_writer_id_is_refused() {
    let a = shard(0, 1);
    a.add_message(&msg("m1", &did(0), NOON, "real"), &w(0, 1)).unwrap();
    let mut d3 = shard(0, 3);
    d3.apply_remote(&a.snapshot(), &own(0, 1)).unwrap();
    let (snap, vv) = (d3.snapshot(), d3.vv());
    for v in [1, 2] {
        let u = span(&a, wid(0, 1), &rec_of("b1", 1, v, NOON + 1, "under 0's id"), by(1));
        for ctx in [live(1), hist(1, 0)] {
            assert_eq!(d3.apply_remote(&u, &ctx), Err(Reject::ForeignPeer));
            assert!(d3.vv() == vv && d3.snapshot() == snap);
        }
        // Also on a doc that has never seen that writer ID.
        let u = span(&recv(), wid(0, 1), &rec_of("b1", 1, v, NOON + 1, "under 0's id"), by(1));
        let mut fresh = shard(0, 4);
        assert_eq!(fresh.apply_remote(&u, &hist(1, 0)), Err(Reject::ForeignPeer));
        assert!(fresh.vv().is_empty());
    }
    // Its own message under its own writer ID is fine.
    let b = shard(1, 2);
    let u = b.add_message(&msg("b1", &did(1), NOON + 1, "mine"), &w(1, 2)).unwrap();
    assert_eq!(d3.apply_remote(&u, &live(1)).unwrap().added, vec!["b1".to_string()]);
}

// KIND: invariant (P9c; was the P9b limitation
// `the_contact_can_still_squat_an_unseen_sibling_writer_id_with_its_own_messages`, now closed)
// Account 0 has devices 1 ("us") and 3 (a sibling we have not synced with). The contact
// (account 1) tries to occupy the sibling's writer ID with its own, validly DID-signed messages
// (its device ID, or the sibling's copied), and with junk. Every attempt is refused in every
// context, although this device has never seen that writer ID; the sibling's real post then
// arrives and shows, and the contact's honest snapshot holding it changes nothing.
#[test]
fn the_contact_cannot_squat_an_unseen_sibling_writer_id() {
    let sibling = wid(0, 3);
    let mut us = shard(0, 1);
    let (snap, vv) = (us.snapshot(), us.vv());
    let unchanged = |s: &Shard| s.vv() == vv && s.snapshot() == snap && s.messages().unwrap().is_empty();
    // (update, verdict in a contact's history / own-device sync, verdict live from the contact)
    let mut attempts = Vec::new();
    for v in [2, 3] {
        let u = span(&recv(), sibling, &rec_of("b1", 1, v, NOON, "squatting"), by(1));
        attempts.push((u, Reject::ForeignPeer, Reject::ForeignPeer));
    }
    // Junk: a record "by us" under the sibling's ID that we never signed; ops nobody signed.
    let u = span(&recv(), sibling, &rec_of("a9", 0, 3, NOON, "forged"), by(1));
    attempts.push((u, Reject::BadSignature, Reject::WrongAuthor));
    let u = doc_with(&[&span(&recv(), wid(1, 2), &rec_of("b2", 1, 2, NOON, "x"), by(1))], sibling, |m| touch(m, "b2"));
    attempts.push((u, Reject::Unsigned, Reject::Unsigned));
    for (u, in_history, in_live) in &attempts {
        for ctx in [hist(1, 0), own(0, 1), live(1), sync(1)] {
            let want = if ctx.history.is_some() { in_history } else { in_live };
            assert_eq!(us.apply_remote(u, &ctx), Err(want.clone()), "{ctx:?}");
            assert!(unchanged(&us));
        }
    }
    // A copy of the sibling's genuine record on another span under its ID.
    let s3 = shard(0, 3);
    let real = s3.add_message(&msg("a3", &did(0), NOON + 1, "from 3"), &w(0, 3)).unwrap();
    let r3 = s3.messages().unwrap()["a3"].clone();
    let copied = span(&recv(), sibling, &MsgRec { at: NOON + 2, ..r3.clone() }, |_, _| r3.did_sig.clone());
    assert_eq!(us.apply_remote(&copied, &hist(1, 0)), Err(Reject::BadSignature));
    assert!(unchanged(&us));
    // The sibling's real post arrives (own-device sync) and shows; the contact's honest
    // snapshot holding it changes nothing.
    assert_eq!(us.apply_remote(&real, &own(0, 1)).unwrap().added, vec!["a3".to_string()]);
    let mut c = shard(1, 2);
    c.apply_remote(&real, &live(0)).unwrap();
    assert_eq!(us.apply_remote(&c.snapshot(), &hist(1, 0)).unwrap(), Applied::default());
    assert_eq!(us.vv(), c.vv());
}
