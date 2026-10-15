//! Writer ID squatting and cross-DID copying.

use super::*;

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
