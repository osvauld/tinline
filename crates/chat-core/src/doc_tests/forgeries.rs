//! Forged batches, device-key records and span proofs.

use super::*;

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
