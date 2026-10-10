use super::doc::*;
use loro::{ExportMode, LoroDoc, LoroMap};

const DAY: &str = "2026-10-07";
const NOON: i64 = 1_791_374_400_000; // 2026-10-07 12:00:00 UTC

fn sec(n: u8) -> [u8; 32] {
    [n; 32]
}

fn dev(n: u8) -> [u8; 32] {
    proto::device_public(&sec(n))
}

/// A pre-P8 writer: device-key signature only (what an un-upgraded app writes). The authors of
/// these tests ("A", "B") are not DIDs, so only the legacy path can judge them.
fn legacy(n: u8) -> Writer {
    Writer::legacy(sec(n))
}

fn msg(id: &str, author: &str, at: i64, text: &str) -> MsgRec {
    MsgRec { id: id.into(), author: author.into(), at, text: text.into(), edited_at: None, deleted: false, reply_to: None, file: None, sig: Vec::new() }
}

fn ctx(signer: &str, device: u8, live: bool) -> Ctx {
    Ctx { signer_did: signer.into(), signer_peer: peer_id(&dev(device), &doc_name("p", DAY)), now_ms: NOON, live, history: None, keys: vec![("A".into(), dev(1)), ("B".into(), dev(2))], hide_unverified: None, known_writers: vec![] }
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
    e.insert("sig", "00".repeat(64)).unwrap();
}

#[test]
fn days_and_pairs() {
    assert_eq!(day_of(NOON), DAY);
    assert_eq!(day_of(0), "1970-01-01");
    assert_eq!(day_start(DAY), Some(NOON - 12 * HOUR_MS));
    assert_eq!(day_start("2026-02-30"), None);
    assert_eq!(day_start("nope"), None);
    assert_eq!(pair_id("did:a", "did:b"), pair_id("did:b", "did:a"));
    assert_ne!(pair_id("did:a", "did:b"), pair_id("did:a", "did:c"));
    assert_ne!(peer_id(&dev(1), "dm/p/x"), peer_id(&dev(2), "dm/p/x"));
    assert_ne!(peer_id(&dev(1), "dm/p/x"), peer_id(&dev(1), "dm/p/y"));
}

#[test]
fn honest_batches_apply_once_and_converge() {
    let (mut a, mut b) = (shard(1), shard(2));
    let u = a.add_message(&msg("m1", "A", NOON, "hello"), &legacy(1)).unwrap();
    let r = b.apply_remote(&u, &ctx("A", 1, true)).unwrap();
    assert_eq!(r.added, vec!["m1".to_string()]);
    // Replay is a no-op.
    assert!(b.apply_remote(&u, &ctx("A", 1, true)).unwrap().is_empty());
    // Concurrent writes on both sides merge to the same state, ordered by (at, id).
    let ua = a.add_message(&msg("m2", "A", NOON + 5, "from a"), &legacy(1)).unwrap();
    let ub = b.add_message(&msg("m3", "B", NOON + 5, "from b"), &legacy(2)).unwrap();
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
    let (a, mut b) = (shard(1), shard(2));
    let u = a.add_message(&msg("m1", "A", NOON, "hello"), &legacy(1)).unwrap();
    b.apply_remote(&u, &ctx("A", 1, true)).unwrap();
    let e = a.edit("m1", "hello!", NOON + 1000, &legacy(1)).unwrap();
    let r = b.apply_remote(&e, &ctx("A", 1, true)).unwrap();
    assert_eq!(r.changed, vec!["m1".to_string()]);
    assert_eq!(b.messages().unwrap()["m1"].text, "hello!");
    let d = a.delete("m1", NOON + 2000, &legacy(1)).unwrap();
    b.apply_remote(&d, &ctx("A", 1, true)).unwrap();
    let m = &b.messages().unwrap()["m1"];
    assert!(m.deleted && m.text.is_empty());
    // B tries to edit A's message: dropped.
    let evil = b.edit("m1", "mine now", NOON + 3000, &legacy(2)).unwrap();
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
    let u = b.add_message(&msg("m1", "B", NOON, "hi"), &legacy(2)).unwrap();
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
    let u = a.add_message(&msg("m6", "A", NOON, "mine"), &legacy(1)).unwrap();
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
    a.add_message(&msg("m1", "A", NOON, "x"), &legacy(1)).unwrap();
    let c = a.my_counter();
    let back = Shard::load("p", DAY, &dev(1), Some(&a.snapshot()), &[]).unwrap();
    assert_eq!(back.peer(), a.peer());
    back.add_message(&msg("m2", "A", NOON + 1, "y"), &legacy(1)).unwrap();
    assert!(back.my_counter() > c);
    assert_eq!(back.messages().unwrap().len(), 2);
}

fn history_ctx() -> Ctx {
    let mut c = ctx("B", 2, false);
    c.history = Some("A".into());
    c
}

fn set_field(map: &LoroMap, id: &str, k: &str, v: &str) {
    if let Some(loro::ValueOrContainer::Container(loro::Container::Map(e))) = map.get(id) {
        e.insert(k, v).unwrap();
    }
}

#[test]
fn forged_messages_in_vouched_snapshots_are_rejected() {
    let a = shard(1);
    a.add_message(&msg("m1", "A", NOON, "real"), &legacy(1)).unwrap();
    // B (the malicious peer) forges a message "from A" signed with its own key.
    let evil = shard(2);
    evil.add_message(&msg("f1", "A", NOON, "A says pay B"), &legacy(2)).unwrap();
    let mut fresh = shard(1);
    assert_eq!(fresh.apply_remote(&evil.snapshot(), &history_ctx()), Err(Reject::BadSignature));
    assert!(fresh.messages().unwrap().is_empty());
    // Same with a junk signature.
    let u = forge(&shard(9), 77, |m| put(m, "f2", "A", NOON, "x"));
    assert_eq!(fresh.apply_remote(&u, &history_ctx()), Err(Reject::BadSignature));
    // An honest snapshot of A's own message is accepted op-agnostically.
    let ok = fresh.apply_remote(&a.snapshot(), &history_ctx()).unwrap();
    assert_eq!(ok.added, vec!["m1".to_string()]);
}

#[test]
fn altered_text_and_rollbacks_are_rejected() {
    let a = shard(1);
    a.add_message(&msg("m1", "A", NOON, "real"), &legacy(1)).unwrap();
    let old_snap = a.snapshot();
    a.edit("m1", "v2", NOON + 10, &legacy(1)).unwrap();
    let mut victim = shard(1);
    victim.apply_remote(&a.snapshot(), &history_ctx()).unwrap();
    // B keeps A's signature but swaps the text.
    let u = forge(&victim, 77, |m| set_field(m, "m1", "text", "pay B"));
    assert_eq!(victim.apply_remote(&u, &history_ctx()), Err(Reject::BadSignature));
    assert_eq!(victim.messages().unwrap()["m1"].text, "v2");
    // Replaying A's older, validly signed version must not roll the edit back.
    let _ = victim.apply_remote(&old_snap, &history_ctx());
    assert_eq!(victim.messages().unwrap()["m1"].text, "v2");
}

#[test]
fn signed_messages_do_not_replay_across_days_or_pairs() {
    let a = shard(1);
    a.add_message(&msg("m1", "A", NOON, "hello"), &legacy(1)).unwrap();
    let snap = a.snapshot();
    let mut c = history_ctx();
    c.now_ms = NOON + DAY_MS;
    // Verbatim replay into another day, and into another pair.
    let mut wrong_day = Shard::new("p", "2026-10-08", &dev(2));
    assert!(wrong_day.apply_remote(&snap, &c).is_err());
    let mut wrong_pair = Shard::new("q", DAY, &dev(2));
    assert_eq!(wrong_pair.apply_remote(&snap, &history_ctx()), Err(Reject::BadSignature));
    // Same signature on a copy whose time was moved into the other day's range.
    let sig = hex::encode(&a.messages().unwrap()["m1"].sig);
    let u = forge(&shard(9), 77, |m| {
        put(m, "m1", "A", NOON + DAY_MS, "hello");
        set_field(m, "m1", "sig", &sig);
    });
    assert_eq!(wrong_day.apply_remote(&u, &c), Err(Reject::BadSignature));
}

/// A history snapshot from B (the contact) as `chat_apply_history` judges it: per message.
fn per_message_ctx() -> Ctx {
    let mut c = history_ctx();
    c.hide_unverified = Some("B".into());
    c
}

#[test]
fn one_unverifiable_message_of_the_contact_does_not_block_the_rest_of_a_snapshot() {
    // B wrote b1 with a device key we never learned (5); A answered (a1), then B wrote b2 with
    // its known key (2). Each change builds on the previous ones.
    let b_old = Shard::new("p", DAY, &dev(5));
    b_old.add_message(&msg("b1", "B", NOON, "old key"), &legacy(5)).unwrap();
    let a = Shard::load("p", DAY, &dev(1), Some(&b_old.snapshot()), &[]).unwrap();
    a.add_message(&msg("a1", "A", NOON + 1, "reply"), &legacy(1)).unwrap();
    let b = Shard::load("p", DAY, &dev(2), Some(&a.snapshot()), &[]).unwrap();
    b.add_message(&msg("b2", "B", NOON + 2, "new key"), &legacy(2)).unwrap();
    let snap = b.snapshot();
    // All-or-nothing: the one bad message rejects everything.
    let mut strict = shard(3);
    assert_eq!(strict.apply_remote(&snap, &history_ctx()), Err(Reject::BadSignature));
    assert!(strict.messages().unwrap().is_empty());
    // Per message: a1 and b2 are accepted, b1's ops are kept but it stays hidden.
    let mut recv = shard(3);
    let r = recv.apply_remote(&snap, &per_message_ctx()).unwrap();
    assert_eq!(r.added, vec!["a1".to_string(), "b2".to_string()]);
    assert_eq!(r.hidden, vec!["b1".to_string()]);
    assert_eq!(recv.messages().unwrap().keys().collect::<Vec<_>>(), vec!["a1", "b2"]);
    assert!(recv.hidden.contains("b1"));
    // A replay changes nothing and reveals nothing.
    assert!(recv.apply_remote(&snap, &per_message_ctx()).unwrap().is_empty());
    assert_eq!(recv.messages().unwrap().len(), 2);
    // Reloaded with its hidden set (as `ShardMeta::hidden` persists it): still hidden.
    let mut back = Shard::load("p", DAY, &dev(3), Some(&recv.snapshot()), &[]).unwrap();
    back.hidden = recv.hidden.clone();
    assert_eq!(back.messages().unwrap().len(), 2);
    // A later change to b1 that still does not verify keeps it hidden, not rejected.
    let u = b_old.edit("b1", "old key, edited", NOON + 5, &legacy(5)).unwrap();
    let r = back.apply_remote(&u, &per_message_ctx()).unwrap();
    assert!(r.added.is_empty() && r.changed.is_empty());
    assert_eq!(r.hidden, vec!["b1".to_string()]);
    // Once b1 carries a signature by a key we know, it shows, reported as added.
    let u = b.edit("b1", "signed again", NOON + 10, &legacy(2)).unwrap();
    let r = back.apply_remote(&u, &per_message_ctx()).unwrap();
    assert_eq!(r.added, vec!["b1".to_string()]);
    assert!(back.hidden.is_empty());
    assert_eq!(back.messages().unwrap()["b1"].text, "signed again");
}

#[test]
fn per_message_judging_never_hides_our_messages_or_a_forged_change() {
    // A message "by A" (the receiver) that does not verify still rejects the whole snapshot.
    let evil = shard(2);
    evil.add_message(&msg("f1", "A", NOON, "A says pay B"), &legacy(2)).unwrap();
    let mut fresh = shard(1);
    assert_eq!(fresh.apply_remote(&evil.snapshot(), &per_message_ctx()), Err(Reject::BadSignature));
    assert!(fresh.messages().unwrap().is_empty() && fresh.hidden.is_empty());
    // A forged change to a message of B's that we accepted still rejects: a verified message is
    // never replaced or hidden.
    let b = shard(2);
    b.add_message(&msg("b1", "B", NOON, "real"), &legacy(2)).unwrap();
    let mut victim = shard(1);
    victim.apply_remote(&b.snapshot(), &per_message_ctx()).unwrap();
    let u = forge(&victim, 77, |m| set_field(m, "b1", "text", "forged"));
    assert_eq!(victim.apply_remote(&u, &per_message_ctx()), Err(Reject::BadSignature));
    assert_eq!(victim.messages().unwrap()["b1"].text, "real");
    assert!(victim.hidden.is_empty());
    // Live batches are never judged per message, whatever the context says.
    let mut live = shard(1);
    let u = forge(&shard(9), peer_id(&dev(2), &doc_name("p", DAY)), |m| put(m, "b9", "B", NOON, "x"));
    let mut c = ctx("B", 2, true);
    c.hide_unverified = Some("B".into());
    assert_eq!(live.apply_remote(&u, &c), Err(Reject::BadSignature));
    assert!(live.hidden.is_empty());
}

// ---- P8: messages signed by the author's DID key -------------------------------------------

/// Accounts with real `did:key` DIDs (0 and 1 talk; 2 is a stranger).
fn acct(n: usize) -> &'static identity::Identity {
    static IDS: std::sync::OnceLock<Vec<identity::Identity>> = std::sync::OnceLock::new();
    &IDS.get_or_init(|| (0..3).map(|_| identity::generate().0).collect())[n]
}

fn did(n: usize) -> String {
    acct(n).did().to_string()
}

/// Account `n` writing on device `device`: device signature plus DID signature.
fn writer(device: u8, n: usize) -> Writer {
    Writer::new(sec(device), acct(n).clone())
}

/// A live batch from account `signer` on `device`, judged knowing NO device key at all.
fn dctx(signer: usize, device: u8, live: bool) -> Ctx {
    Ctx { signer_did: did(signer), signer_peer: peer_id(&dev(device), &doc_name("p", DAY)), now_ms: NOON, live, history: None, keys: vec![], hide_unverified: None, known_writers: vec![] }
}

/// A vouched snapshot from account `contact`, received by account `me`, judged per message.
fn dhist(contact: usize, me: usize) -> Ctx {
    let mut c = dctx(contact, 9, false);
    c.history = Some(did(me));
    c.hide_unverified = Some(did(contact));
    c
}

fn name() -> String {
    doc_name("p", DAY)
}

/// Like `forge`, but the change carries `note` as its commit message.
fn forge_noted(from: &Shard, peer: u64, note: &str, f: impl FnOnce(&LoroMap)) -> Vec<u8> {
    let doc = LoroDoc::new();
    doc.import(&from.snapshot()).unwrap();
    doc.set_peer_id(peer).unwrap();
    let before = doc.oplog_vv();
    f(&doc.get_map("messages"));
    doc.commit_with(loro::CommitOptions::new().commit_msg(note));
    doc.export(ExportMode::updates(&before)).unwrap()
}

/// Writes `rec` as a raw record with the device signature `sig`.
fn put_signed(map: &LoroMap, rec: &MsgRec, sig: &[u8]) {
    put(map, &rec.id, &rec.author, rec.at, &rec.text);
    set_field(map, &rec.id, "sig", &hex::encode(sig));
}

#[test]
fn did_signed_messages_verify_without_any_device_key_of_the_author() {
    // Account 0 writes on device 5, which the receiver never learns.
    let a = shard(5);
    let mut b = shard(2);
    let u = a.add_message(&msg("m1", &did(0), NOON, "hi"), &writer(5, 0)).unwrap();
    assert_eq!(b.apply_remote(&u, &dctx(0, 5, true)).unwrap().added, vec!["m1".to_string()]);
    let e = a.edit("m1", "hi!", NOON + 1000, &writer(5, 0)).unwrap();
    assert_eq!(b.apply_remote(&e, &dctx(0, 5, true)).unwrap().changed, vec!["m1".to_string()]);
    let d = a.delete("m1", NOON + 2000, &writer(5, 0)).unwrap();
    b.apply_remote(&d, &dctx(0, 5, true)).unwrap();
    assert!(b.messages().unwrap()["m1"].deleted);
    // A restore: same account, new device key (6). Its messages verify the same way, and so do
    // the ones written before the restore, on a receiver that knows neither device.
    let a2 = Shard::load("p", DAY, &dev(6), Some(&a.snapshot()), &[]).unwrap();
    a2.add_message(&msg("m2", &did(0), NOON + 5, "after restore"), &writer(6, 0)).unwrap();
    let mut fresh = shard(3);
    let r = fresh.apply_remote(&a2.snapshot(), &dhist(0, 1)).unwrap();
    assert_eq!(r.added, vec!["m1".to_string(), "m2".to_string()]);
    assert!(r.hidden.is_empty() && fresh.hidden.is_empty());
    // The same snapshot relayed by one of the receiver's own devices (own-device sync, as
    // `chat_own_apply` judges it: signer = us, `history` = the contact).
    let mut own = shard(4);
    let mut c = dctx(1, 9, false);
    c.history = Some(did(0));
    c.hide_unverified = Some(did(0));
    let r = own.apply_remote(&a2.snapshot(), &c).unwrap();
    assert_eq!(r.added.len(), 2);
}

#[test]
fn dual_signed_records_keep_the_legacy_shape_and_the_device_signature() {
    let a = shard(1);
    a.add_message(&msg("m1", &did(0), NOON, "hi"), &writer(1, 0)).unwrap();
    a.edit("m1", "hi!", NOON + 10, &writer(1, 0)).unwrap();
    // An un-upgraded app rejects a record with any field it does not know (`Malformed`): the
    // record holds legacy fields only.
    let doc = LoroDoc::new();
    doc.import(&a.snapshot()).unwrap();
    let loro::LoroValue::Map(top) = doc.get_deep_value() else { panic!() };
    assert_eq!(top.keys().collect::<Vec<_>>(), vec!["messages"]);
    let loro::LoroValue::Map(msgs) = &top["messages"] else { panic!() };
    let loro::LoroValue::Map(m1) = &msgs["m1"] else { panic!() };
    for k in m1.keys() {
        assert!(["id", "author", "at", "text", "edited_at", "deleted", "reply_to", "file", "sig"].contains(&k.as_str()), "{k}");
    }
    // The device signature an un-upgraded receiver checks is still there and verifies.
    let rec = &a.messages().unwrap()["m1"];
    assert!(verify_msg(&dev(1), &name(), rec));
    // The DID signature rides in the change's commit message, which survives a reload and a
    // re-export, so a receiver that knows no device key accepts the reloaded doc.
    let back = Shard::load("p", DAY, &dev(1), Some(&a.snapshot()), &[]).unwrap();
    let all = back.export_all_since(&loro::VersionVector::default()).unwrap();
    let mut fresh = shard(3);
    assert_eq!(fresh.apply_remote(&all, &dhist(0, 1)).unwrap().added, vec!["m1".to_string()]);
    assert_eq!(fresh.messages().unwrap()["m1"].text, "hi!");
}

#[test]
fn a_did_signature_that_does_not_verify_is_never_rescued_by_the_device_signature() {
    let pa = peer_id(&dev(1), &name());
    let rec = msg("m1", &did(0), NOON, "x");
    let devsig = sign_msg(&sec(1), &name(), &rec);
    let mut known = dctx(0, 1, true);
    known.keys = vec![(did(0), dev(1))];
    // Control: no DID signature at all, a known device key: the legacy path accepts it.
    let u = forge(&shard(9), pa, |m| put_signed(m, &rec, &devsig));
    assert!(shard(2).apply_remote(&u, &known).is_ok());
    // A commit message that is not a DID signature note changes nothing.
    let u = forge_noted(&shard(9), pa, "hello", |m| put_signed(m, &rec, &devsig));
    assert!(shard(2).apply_remote(&u, &known).is_ok());
    // A DID signature by another account, by the wrong domain, or junk: rejected although the
    // device signature verifies.
    let wrong = [
        sign_msg_did(acct(2), &name(), &rec),
        acct(0).sign(&msg_sig_input(&name(), &rec)).to_vec(),
        sign_msg_did(acct(0), &doc_name("q", DAY), &rec),
        vec![7; 64],
        vec![1, 2, 3],
    ];
    for bad in &wrong {
        let note = did_sig_note(&rec.id, &devsig, bad);
        let u = forge_noted(&shard(9), pa, &note, |m| put_signed(m, &rec, &devsig));
        let mut b = shard(2);
        assert_eq!(b.apply_remote(&u, &known), Err(Reject::BadSignature));
        assert!(b.messages().unwrap().is_empty());
        // In a vouched snapshot from the author it stays hidden, exactly like a bad device
        // signature; one claiming to be ours rejects the whole snapshot.
        let mut h = dhist(0, 1);
        h.keys = known.keys.clone();
        let mut b = shard(2);
        assert_eq!(b.apply_remote(&u, &h).unwrap().hidden, vec!["m1".to_string()]);
        assert!(b.messages().unwrap().is_empty());
        let mut ours = dhist(1, 0);
        ours.keys = known.keys.clone();
        assert_eq!(shard(2).apply_remote(&u, &ours), Err(Reject::BadSignature));
    }
    // A valid DID signature next to a bad one for the same version: still rejected.
    let good = sign_msg_did(acct(0), &name(), &rec);
    let u = {
        let doc = LoroDoc::new();
        doc.set_peer_id(pa).unwrap();
        put_signed(&doc.get_map("messages"), &rec, &devsig);
        doc.commit_with(loro::CommitOptions::new().commit_msg(&did_sig_note(&rec.id, &devsig, &good)));
        // A second change that leaves the record as it is (an absent `deleted` reads as false).
        if let Some(loro::ValueOrContainer::Container(loro::Container::Map(e))) = doc.get_map("messages").get("m1") {
            e.insert("deleted", false).unwrap();
        }
        doc.commit_with(loro::CommitOptions::new().commit_msg(&did_sig_note(&rec.id, &devsig, &wrong[0])));
        doc.export(ExportMode::all_updates()).unwrap()
    };
    assert_eq!(shard(2).apply_remote(&u, &known), Err(Reject::BadSignature));
}

#[test]
fn did_signatures_are_domain_separated_and_bound_to_doc_and_content() {
    let rec = msg("m1", &did(0), NOON, "x");
    let s = sign_msg_did(acct(0), &name(), &rec);
    assert!(verify_msg_did(&name(), &rec, &s));
    assert!(!verify_msg_did(&doc_name("q", DAY), &rec, &s));
    assert!(!verify_msg_did(&name(), &msg("m1", &did(0), NOON, "y"), &s));
    assert!(!verify_msg_did(&name(), &msg("m1", &did(1), NOON, "x"), &s));
    // The DID key signing the device-domain bytes is not a DID signature, and vice versa.
    assert!(!verify_msg_did(&name(), &rec, &acct(0).sign(&msg_sig_input(&name(), &rec))));
    let mut as_device = rec.clone();
    as_device.sig = s.clone();
    assert!(!verify_msg(&acct(0).signing_public_key(), &name(), &as_device));
    // An author that is not a `did:key` never verifies.
    assert!(!verify_msg_did(&name(), &msg("m1", "A", NOON, "x"), &s));
}

#[test]
fn an_un_upgraded_device_of_the_author_falls_back_to_the_device_signature_per_version() {
    // Account 0 writes m1 (dual-signed) on device 1; its un-upgraded device 3 edits it with a
    // device signature only. That version carries no DID signature, so it is judged by the
    // legacy rule: accepted iff device 3 is a known key of account 0.
    let a = shard(1);
    let u = a.add_message(&msg("m1", &did(0), NOON, "v1"), &writer(1, 0)).unwrap();
    let old = Shard::load("p", DAY, &dev(3), Some(&a.snapshot()), &[]).unwrap();
    let e = old.edit("m1", "v2", NOON + 10, &Writer::legacy(sec(3))).unwrap();
    let mut b = shard(2);
    b.apply_remote(&u, &dctx(0, 1, true)).unwrap();
    assert_eq!(b.apply_remote(&e, &dctx(0, 3, true)), Err(Reject::BadSignature));
    let mut c = dctx(0, 3, true);
    c.keys = vec![(did(0), dev(3))];
    assert_eq!(b.apply_remote(&e, &c).unwrap().changed, vec!["m1".to_string()]);
    assert_eq!(b.messages().unwrap()["m1"].text, "v2");
}

// ---- P9: writer ID = hash(DID || device value) as the Loro peer id -------------------------

/// A device value (random in the app; fixed here).
fn val(n: u8) -> [u8; 32] {
    [100 + n; 32]
}

/// Account `n` on device `device` with device value `v`, writing under its writer ID.
fn p9(device: u8, n: usize, v: u8) -> Writer {
    Writer::p9(sec(device), acct(n).clone(), val(v), true)
}

fn wid(n: usize, v: u8) -> u64 {
    writer_id(&did(n), &val(v))
}

/// A shard of account `n`'s device `device` that knows its own writer ID (as the app sets it).
fn wshard(device: u8, n: usize, v: u8) -> Shard {
    let mut s = shard(device);
    s.writer = Some(wid(n, v));
    s
}

/// The peer ids an update holds ops of.
fn peers_of(u: &[u8]) -> Vec<u64> {
    LoroDoc::decode_import_blob_meta(u, true).unwrap().partial_end_vv.keys().copied().collect()
}

/// A P9 note for `rec` written by account `n` with device secret `device` and value `v`.
fn p9_note(rec: &MsgRec, device: u8, n: usize, v: u8) -> (Vec<u8>, String) {
    let devsig = sign_msg(&sec(device), &name(), rec);
    let note = did_sig_note_dv(&rec.id, &devsig, &sign_msg_did(acct(n), &name(), rec), Some(&val(v)));
    (devsig, note)
}

#[test]
fn writer_ids_are_domain_separated_and_derived_from_no_device_key() {
    // Deterministic, and it changes with the DID and with the value.
    assert_eq!(wid(0, 1), writer_id(&did(0), &val(1)));
    assert_ne!(wid(0, 1), wid(1, 1));
    assert_ne!(wid(0, 1), wid(0, 2));
    // The DID is length-prefixed: moving bytes between DID and value gives another ID.
    let mut v = [0u8; 32];
    v[0] = b'b';
    let mut w = [0u8; 32];
    w[..31].copy_from_slice(&v[1..]);
    assert_ne!(writer_id("a", &v), writer_id("ab", &w));
    // Its own domain: not the legacy peer id of any device, and not the plain hash.
    let h = blake3::hash(&[did(0).as_bytes(), &val(1)].concat());
    assert_ne!(wid(0, 1), u64::from_le_bytes(h.as_bytes()[..8].try_into().unwrap()));
    // The writer writes under it, whatever device key signs the record.
    assert_eq!(p9(5, 0, 1).writer_id(), Some(wid(0, 1)));
    assert_eq!(p9(6, 0, 1).writer_id(), Some(wid(0, 1)));
    assert_eq!(writer(5, 0).writer_id(), None);
    // Fresh values are random.
    assert_ne!(new_device_value(), new_device_value());
}

#[test]
fn p9_notes_stay_readable_by_p8_apps_and_under_the_note_limit() {
    // What a P8 app parses a note into (no `deny_unknown_fields`).
    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct P8Note {
        t: String,
        id: String,
        sig: String,
        did_sig: String,
    }
    let rec = msg(&"x".repeat(64), &did(0), NOON, "longest id");
    let (devsig, note) = p9_note(&rec, 1, 0, 1);
    assert!(note.len() < MAX_NOTE, "{}", note.len());
    let old: P8Note = serde_json::from_str(&note).unwrap();
    assert_eq!(old.sig, hex::encode(&devsig));
    assert!(verify_msg_did(&name(), &rec, &hex::decode(&old.did_sig).unwrap()));
    // A P8 note has no `dv`; P9 still reads it.
    assert!(!did_sig_note(&rec.id, &devsig, &[0; 64]).contains("dv"));
}

#[test]
fn p9_writes_use_the_writer_id_and_verify_with_no_device_key() {
    let a = wshard(5, 0, 1);
    let u = a.add_message(&msg("m1", &did(0), NOON, "hi"), &p9(5, 0, 1)).unwrap();
    assert_eq!(a.peer(), wid(0, 1));
    assert_eq!(peers_of(&u), vec![wid(0, 1)]);
    // The device key's legacy peer id appears nowhere in the doc.
    assert!(a.vv().get(&peer_id(&dev(5), &name())).is_none());
    // A receiver that knows no device key of the author: live (signer_peer is the session's
    // legacy peer id, which the batch does not use) and as history.
    let mut b = shard(2);
    let r = b.apply_remote(&u, &dctx(0, 5, true)).unwrap();
    assert_eq!(r.added, vec!["m1".to_string()]);
    assert_eq!(r.writers, vec![wid(0, 1)]);
    assert!(r.dv_seen);
    let e = a.edit("m1", "hi!", NOON + 10, &p9(5, 0, 1)).unwrap();
    assert_eq!(b.apply_remote(&e, &dctx(0, 5, true)).unwrap().changed, vec!["m1".to_string()]);
    let mut h = shard(3);
    assert_eq!(h.apply_remote(&a.snapshot(), &dhist(0, 1)).unwrap().added, vec!["m1".to_string()]);
    assert_eq!(h.messages().unwrap()["m1"].text, "hi!");
    // Every replica keeps the sender's number unchanged.
    assert_eq!(b.vv(), a.vv());
    assert_eq!(h.vv(), a.vv());
}

/// Sets `deleted` to false on `id`: an op that leaves the record as it reads (absent = false).
fn touch(map: &LoroMap, id: &str) {
    if let Some(loro::ValueOrContainer::Container(loro::Container::Map(e))) = map.get(id) {
        e.insert("deleted", false).unwrap();
    }
}

#[test]
fn forged_writer_ids_are_refused_without_any_state_change() {
    // Account 0 (device 1, value 1) wrote m1 under its writer ID; account 1's device 2 holds it.
    let a = wshard(1, 0, 1);
    a.add_message(&msg("m1", &did(0), NOON, "real"), &p9(1, 0, 1)).unwrap();
    let mut b = shard(2);
    b.apply_remote(&a.snapshot(), &dctx(0, 1, true)).unwrap();
    let (snap, vv) = (b.snapshot(), b.vv());
    let unchanged = |s: &Shard| s.vv() == vv && s.snapshot() == snap && s.messages().unwrap().len() == 1;
    let hist = dhist(0, 1);

    // 1. A post whose peer id is not `writer_id(author, dv)` for the value its note carries.
    let rec = msg("m2", &did(0), NOON + 1, "x");
    let (devsig, note) = p9_note(&rec, 1, 0, 1);
    let u = forge_noted(&a, wid(0, 2), &note, |m| put_signed(m, &rec, &devsig));
    assert_eq!(b.apply_remote(&u, &dctx(0, 1, true)), Err(Reject::ForeignPeer));
    assert!(unchanged(&b));
    // 2. Ops under a known writer ID without any note.
    let u = forge(&a, wid(0, 1), |m| touch(m, "m1"));
    assert_eq!(b.apply_remote(&u, &dctx(0, 1, true)), Err(Reject::ForeignPeer));
    assert_eq!(b.apply_remote(&u, &hist), Err(Reject::ForeignPeer));
    assert!(unchanged(&b));
    // 3. A genuine note replayed on a change that writes no new version.
    let real = a.messages().unwrap()["m1"].clone();
    let note = did_sig_note_dv("m1", &real.sig, &sign_msg_did(acct(0), &name(), &real), Some(&val(1)));
    let u = forge_noted(&a, wid(0, 1), &note, |m| touch(m, "m1"));
    assert_eq!(b.apply_remote(&u, &dctx(0, 1, true)), Err(Reject::ForeignPeer));
    assert_eq!(b.apply_remote(&u, &hist), Err(Reject::ForeignPeer));
    assert!(unchanged(&b));
    // Control: the genuine next post of account 0 is accepted.
    let ok = a.add_message(&msg("m5", &did(0), NOON + 5, "next"), &p9(1, 0, 1)).unwrap();
    assert_eq!(b.apply_remote(&ok, &dctx(0, 1, true)).unwrap().added, vec!["m5".to_string()]);
}

#[test]
fn cross_did_copying_of_a_writer_id_is_refused() {
    // Account 0's devices 1 (value 1) and 3 (value 3); device 3 holds m1 that device 1 wrote.
    let a = wshard(1, 0, 1);
    a.add_message(&msg("m1", &did(0), NOON, "real"), &p9(1, 0, 1)).unwrap();
    let mut d3 = wshard(3, 0, 3);
    let mut own = dctx(0, 9, false);
    own.history = Some(did(1));
    d3.apply_remote(&a.snapshot(), &own).unwrap();
    let (snap, vv) = (d3.snapshot(), d3.vv());
    // The contact (account 1) posts its own, validly DID-signed message under account 0's
    // writer ID: with account 0's value copied (the hash then names account 0, not the
    // author), or with its own value (the hash is not that ID).
    let rec = msg("b1", &did(1), NOON + 1, "under 0's id");
    let devsig = sign_msg(&sec(2), &name(), &rec);
    let did_sig = sign_msg_did(acct(1), &name(), &rec);
    for v in [val(1), val(2)] {
        let note = did_sig_note_dv(&rec.id, &devsig, &did_sig, Some(&v));
        let u = forge_noted(&a, wid(0, 1), &note, |m| put_signed(m, &rec, &devsig));
        // Live from the contact's session, and in the contact's vouched snapshot.
        assert_eq!(d3.apply_remote(&u, &dctx(1, 2, true)), Err(Reject::ForeignPeer));
        assert_eq!(d3.apply_remote(&u, &dhist(1, 0)), Err(Reject::ForeignPeer));
        assert!(d3.vv() == vv && d3.snapshot() == snap);
    }
    // On a doc that has not seen that writer ID yet, the IDs remembered from earlier docs
    // (`known_writers`) refuse it; without that memory the legacy rule applies (open question).
    let note = did_sig_note_dv(&rec.id, &devsig, &did_sig, Some(&val(2)));
    let u = forge_noted(&shard(9), wid(0, 1), &note, |m| put_signed(m, &rec, &devsig));
    let mut fresh = wshard(4, 0, 4);
    let mut c = dhist(1, 0);
    c.known_writers = vec![wid(0, 1)];
    assert_eq!(fresh.apply_remote(&u, &c), Err(Reject::ForeignPeer));
    assert!(fresh.vv().is_empty());
    // Its own message under its own writer ID is fine.
    let b = wshard(2, 1, 2);
    let u = b.add_message(&rec, &p9(2, 1, 2)).unwrap();
    assert_eq!(d3.apply_remote(&u, &dctx(1, 2, true)).unwrap().added, vec!["b1".to_string()]);
}

#[test]
fn a_note_whose_value_does_not_match_its_peer_falls_to_the_legacy_rule() {
    // A correctly DID-signed P9 note under a peer id that is not its writer ID: what a P9 app
    // writing under its legacy peer id produces. Live it must be the session's legacy peer id;
    // in history any peer id that is not a known writer ID (the P8 rule).
    let rec = msg("m1", &did(0), NOON, "x");
    let (devsig, note) = p9_note(&rec, 1, 0, 1);
    let u = forge_noted(&shard(9), 77, &note, |m| put_signed(m, &rec, &devsig));
    assert_eq!(shard(2).apply_remote(&u, &dctx(0, 1, true)), Err(Reject::ForeignPeer));
    assert_eq!(shard(2).apply_remote(&u, &dhist(0, 1)).unwrap().added, vec!["m1".to_string()]);
    let mut c = dhist(0, 1);
    c.known_writers = vec![77];
    assert_eq!(shard(2).apply_remote(&u, &c), Err(Reject::ForeignPeer));
    // A bad DID signature there hides the contact's message, as before P9.
    let bad = did_sig_note_dv(&rec.id, &devsig, &sign_msg_did(acct(2), &name(), &rec), Some(&val(1)));
    let u = forge_noted(&shard(9), 77, &bad, |m| put_signed(m, &rec, &devsig));
    let mut h = shard(2);
    assert_eq!(h.apply_remote(&u, &dhist(0, 1)).unwrap().hidden, vec!["m1".to_string()]);
    assert!(h.messages().unwrap().is_empty());
}

#[test]
fn two_own_devices_with_different_values_converge() {
    // Account 0 on devices 1 (value 1) and 3 (value 3): two writer IDs, one DID.
    assert_ne!(wid(0, 1), wid(0, 3));
    let d1 = wshard(1, 0, 1);
    let mut d3 = wshard(3, 0, 3);
    let mut contact = shard(2);
    let u1 = d1.add_message(&msg("a1", &did(0), NOON, "from 1"), &p9(1, 0, 1)).unwrap();
    let u3 = d3.add_message(&msg("a3", &did(0), NOON + 1, "from 3"), &p9(3, 0, 3)).unwrap();
    // The contact gets each device's batch live from that device's session.
    contact.apply_remote(&u1, &dctx(0, 1, true)).unwrap();
    contact.apply_remote(&u3, &dctx(0, 3, true)).unwrap();
    // Own-device sync (`chat_own_apply`: signer = us, history = the contact) both ways.
    let mut own = dctx(0, 9, false);
    own.history = Some(did(1));
    own.hide_unverified = Some(did(1));
    let mut d1 = d1;
    d3.apply_remote(&d1.export_all_since(&d3.vv()).unwrap(), &own).unwrap();
    d1.apply_remote(&d3.export_all_since(&d1.vv()).unwrap(), &own).unwrap();
    // Device 3 edits the message device 1 created, under its own writer ID.
    let e = d3.edit("a1", "from 1, edited on 3", NOON + 10, &p9(3, 0, 3)).unwrap();
    assert_eq!(peers_of(&e), vec![wid(0, 3)]);
    contact.apply_remote(&e, &dctx(0, 3, true)).unwrap();
    d1.apply_remote(&e, &own).unwrap();
    for s in [&d1, &d3, &contact] {
        assert_eq!(s.vv(), d3.vv());
        assert_eq!(s.messages().unwrap(), d3.messages().unwrap());
    }
    assert_eq!(contact.messages().unwrap()["a1"].text, "from 1, edited on 3");
    // A device sends contacts only its own writer's ops.
    assert!(d1.export_own_since(&contact.vv()).is_none());
}

#[test]
fn a_device_switching_from_its_legacy_peer_to_its_writer_id_keeps_working() {
    // Device 1 of account 0 wrote m1 and m2 as a P8 app (legacy peer id); m2 never reached
    // the contact. Then it upgrades: the app sets its writer ID and writes under it.
    let legacy_peer = peer_id(&dev(1), &name());
    let mut a = shard(1);
    let u1 = a.add_message(&msg("m1", &did(0), NOON, "old"), &writer(1, 0)).unwrap();
    a.add_message(&msg("m2", &did(0), NOON + 1, "old, unsent"), &writer(1, 0)).unwrap();
    let mut b = shard(2);
    b.apply_remote(&u1, &dctx(0, 1, true)).unwrap();
    a = Shard::load("p", DAY, &dev(1), Some(&a.snapshot()), &[]).unwrap();
    a.writer = Some(wid(0, 1));
    assert_eq!(a.own_peers(), vec![legacy_peer, wid(0, 1)]);
    // An edit of a message created under the old peer id, and a new message.
    a.edit("m1", "old, edited", NOON + 10, &p9(1, 0, 1)).unwrap();
    a.add_message(&msg("m3", &did(0), NOON + 11, "new"), &p9(1, 0, 1)).unwrap();
    assert_eq!(a.peer(), wid(0, 1));
    // The catch-up holds the unsent legacy op and the new ones: one batch, both own peers,
    // accepted live from the same session (legacy peer = the session's; writer ID proven).
    assert!(a.owes(&b.vv()));
    let up = a.export_own_since(&b.vv()).unwrap();
    let mut ps = peers_of(&up);
    ps.sort();
    let mut want = vec![legacy_peer, wid(0, 1)];
    want.sort();
    assert_eq!(ps, want);
    let r = b.apply_remote(&up, &dctx(0, 1, true)).unwrap();
    assert_eq!(r.added, vec!["m2".to_string(), "m3".to_string()]);
    assert_eq!(r.changed, vec!["m1".to_string()]);
    assert_eq!(b.vv(), a.vv());
    assert!(!a.owes(&b.vv()));
    // A snapshot of the mixed doc: reloads (counters continue under the writer ID), and is
    // accepted as history by a receiver that knows no device key.
    let c = a.my_counter();
    let back = Shard::load("p", DAY, &dev(1), Some(&a.snapshot()), &[]).unwrap();
    back.add_message(&msg("m4", &did(0), NOON + 12, "after reload"), &p9(1, 0, 1)).unwrap();
    assert!(back.vv().get(&wid(0, 1)).unwrap() > &c);
    let mut fresh = shard(3);
    assert_eq!(fresh.apply_remote(&back.snapshot(), &dhist(0, 1)).unwrap().added.len(), 4);
    assert_eq!(fresh.messages().unwrap(), back.messages().unwrap());
    // Own-device sync of the mixed doc to another device of account 0, which then edits a
    // message written under the legacy peer id with its own writer ID.
    let mut d3 = wshard(3, 0, 3);
    let mut own = dctx(0, 9, false);
    own.history = Some(did(1));
    d3.apply_remote(&back.snapshot(), &own).unwrap();
    let e = d3.edit("m2", "edited on 3", NOON + 20, &p9(3, 0, 3)).unwrap();
    b.apply_remote(&back.export_own_since(&b.vv()).unwrap(), &dctx(0, 1, true)).unwrap();
    assert_eq!(b.apply_remote(&e, &dctx(0, 3, true)).unwrap().changed, vec!["m2".to_string()]);
}

#[test]
fn compat_mode_writes_under_the_legacy_peer_and_still_advertises_the_value() {
    // For a contact with a device that predates P9, which requires every op of a live batch to
    // be under the session's legacy peer id, the app writes under the legacy peer id. The note
    // carries the value, so an upgraded receiver learns that the writer judges writer IDs.
    let legacy_peer = peer_id(&dev(1), &name());
    let a = shard(1);
    let compat = Writer::p9(sec(1), acct(0).clone(), val(1), false);
    let u = a.add_message(&msg("m1", &did(0), NOON, "hi"), &compat).unwrap();
    // What a pre-P9 receiver checks (`partial_end_vv` keys all equal its signer peer).
    assert_eq!(peers_of(&u), vec![legacy_peer]);
    let r = shard(2).apply_remote(&u, &dctx(0, 1, true)).unwrap();
    assert!(r.dv_seen && r.writers.is_empty());
    // Under the writer ID the same pre-P9 check fails: why compat mode exists.
    let w = wshard(1, 0, 1);
    let u = w.add_message(&msg("m1", &did(0), NOON, "hi"), &p9(1, 0, 1)).unwrap();
    assert!(peers_of(&u).iter().all(|p| *p != legacy_peer));
    // P8 notes (no value) and pre-P8 records keep the legacy rule.
    let r = shard(2).apply_remote(&shard_with_p8(), &dctx(0, 1, true)).unwrap();
    assert!(!r.dv_seen);
}

fn shard_with_p8() -> Vec<u8> {
    let a = shard(1);
    a.add_message(&msg("m1", &did(0), NOON, "p8"), &writer(1, 0)).unwrap()
}
