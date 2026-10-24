//! Honest traffic: apply, converge, ordering of competing versions, restores.

use super::*;

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

// KIND: invariant (P10b)
// `creates_outside` tells the sender when a batch would create a message a live push may not
// carry (older than the receiver's live window); only creations the receiver lacks count.
#[test]
fn creates_outside_names_old_creations_the_receiver_lacks() {
    let a = shard(0, 1);
    a.add_message(&msg("old", &did(0), NOON - 3 * HOUR_MS, "old"), &w(0, 1)).unwrap();
    let mut b = recv();
    let now = NOON + DAY_MS - 2 * HOUR_MS;
    assert!(a.creates_outside(&b.vv(), &did(0), now, DAY_MS - HOUR_MS));
    assert!(!a.creates_outside(&b.vv(), &did(0), NOON, DAY_MS - HOUR_MS));
    b.apply_remote(&a.export_own_since(&b.vv(), &did(0)).unwrap(), &sync(0)).unwrap();
    a.edit("old", "edited", now, &w(0, 1)).unwrap();
    assert!(!a.creates_outside(&b.vv(), &did(0), now, DAY_MS - HOUR_MS), "an edit of a held message is fine");
}
