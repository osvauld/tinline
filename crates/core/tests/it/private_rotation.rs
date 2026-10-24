//! Packet F: private (1:1) endpoint rotation (`docs/rotation.md` "Private endpoint").
//!
//! Real nodes over real iroh endpoints (needs network, like `calls.rs`); waits are bounded polls.
//! A rotation is forced with `private_rotate_now` (the periodic schedule itself is unit-tested in
//! `group/rotation/tests.rs`, `node/rotation/tests.rs`). Synthetic identities only.
//!
//! Every test carries one label:
//! - `KIND: acceptance`: decided behaviour (2026-10-16), covered end to end.

use std::time::{Duration, Instant};

use p2pcore::RotationPolicy;

use crate::support;
use support::eventually_200ms as eventually;
use support::*;

const TAG: &str = "pf";
const S: Suite = Suite(TAG);
const BLOCKED: &str = "blocked";


fn text_of(p: &Peer, did: &str, id: &str) -> Option<String> {
    all_msgs(p, did).into_iter().find(|m| m.id == id).map(|m| m.text)
}

/// `from` messages `to` and it arrives.
fn message_arrives(from: &Peer, to: &Peer, text: &str) {
    let id = from.node.send_text(to.did.clone(), text.into(), None).unwrap().id;
    eventually(90, text, || text_of(to, &from.did, &id).is_some());
}

/// `from` calls `to`, it rings, the caller hangs up.
fn call_rings(from: &Peer, to: &Peer) {
    let _ = to.rx.try_iter().count();
    let call = from.node.call(to.did.clone()).unwrap();
    assert_eq!(incoming(to, 60), call.call_id);
    from.node.hangup(call.call_id).unwrap();
    eventually(20, "call over", || from.node.current_call().is_none() && to.node.current_call().is_none());
}

/// Every timing short enough for a test; the interval stays long so nothing rotates by itself.
fn short_windows(p: &Peer, announce_ms: u64, overlap_ms: u64) {
    p.node.group_set_rotation_policy_for_test(Some(RotationPolicy { enabled: true, interval_ms: 30 * 24 * 3600 * 1000, jitter_ms: 0, announce_ms }));
    p.node.set_private_overlap_for_test(Some(overlap_ms));
}

// KIND: acceptance
// A contact follows a rotation: the new key reaches them by the signed device list, messages and
// calls work both ways, `verified` survives (the list links the old key to the new one), the
// account DID and the writer ID stay, and the new QR names the new key.
#[test]
fn a_contact_follows_a_rotation() {
    let (a, b) = S.pair("follow");
    // Rotation is off by default (2026-10-19); this device has it on, without jitter so the
    // restarted schedule is later than the first one (two random jitters need not be ordered).
    a.node.group_set_rotation_policy_for_test(Some(RotationPolicy { jitter_ms: 0, ..RotationPolicy::ON }));
    message_arrives(&b, &a, "before");
    b.node.set_verified(a.did.clone(), true).unwrap();
    let (old, wid, ticket) = (a.key(), a.node.writer_id_for_test().unwrap(), a.node.my_ticket().unwrap());
    let due = a.node.private_endpoint_rotation_due().expect("scheduled");
    a.node.private_rotate_now().unwrap();
    let new = a.key();
    assert_ne!(new, old);
    assert_eq!(a.node.profile().unwrap().did, a.did, "the DID never changes");
    assert_eq!(a.node.writer_id_for_test(), Some(wid), "nor the writer ID");
    assert!(a.node.private_endpoint_rotation_due().unwrap() > due, "the schedule restarts");
    assert_eq!(a.node.status().endpoint_id, a.node.profile().unwrap().device);
    eventually(90, "b dials a's new key", || b.dials(&a) == vec![new.clone()]);
    assert!(b.contact(&a.did).verified, "verified survives a rotated key");
    message_arrives(&a, &b, "after, from a");
    message_arrives(&b, &a, "after, from b");
    call_rings(&b, &a);
    call_rings(&a, &b);
    let fresh = a.node.my_ticket().unwrap();
    assert_ne!(fresh, ticket);
    assert_eq!(proto::ContactTicket::from_text(&fresh).unwrap().device, proto::device_from_text(&new).unwrap());
    // Our own registry hides the rotated key: still one install.
    assert_eq!(a.node.linked_devices().len(), 1);
}

// KIND: acceptance
// A contact that was offline at the rotation and comes back within the overlap reaches us at the
// old address, is told the new one there, and continues on it.
#[test]
fn an_offline_contact_reconnects_through_the_overlap() {
    let (a, mut b) = S.pair("overlap");
    message_arrives(&b, &a, "warm");
    b.node.stop();
    a.node.private_rotate_now().unwrap();
    let new = a.key();
    assert!(a.node.overlap_open_for_test());
    b.restart();
    // b knows only the old key: its next exchange lands on the overlapping old endpoint.
    let id = b.node.send_text(a.did.clone(), "back".into(), None).unwrap().id;
    eventually(120, "b follows to the new key", || b.dials(&a) == vec![new.clone()]);
    eventually(120, "a holds b's message", || text_of(&a, &b.did, &id).is_some());
    call_rings(&b, &a);
}

// KIND: acceptance
// A contact offline for longer than the overlap: the old address is gone, but our next dial to
// them (their key did not change) carries the new one, and they follow.
#[test]
fn an_offline_longer_contact_reconnects_when_we_dial_them() {
    let (a, mut b) = S.pair("longer");
    short_windows(&a, 7 * 24 * 3600 * 1000, 500);
    message_arrives(&b, &a, "warm");
    b.node.stop();
    a.node.private_rotate_now().unwrap();
    let new = a.key();
    eventually(20, "the old address closes", || !a.node.overlap_open_for_test());
    b.restart();
    message_arrives(&a, &b, "we dial them");
    eventually(120, "b follows to the new key", || b.dials(&a) == vec![new.clone()]);
    message_arrives(&b, &a, "and they reach us");
}

// KIND: acceptance
// A contact we block never receives the new address, and the old address refuses it at its hello
// exactly as before the rotation ("blocked"). `b` acts like a modified build that ignores our
// block notice and keeps dialling: only our refusal holds.
#[test]
fn a_blocked_contact_never_learns_the_new_address() {
    let (a, b) = S.pair("blocked");
    message_arrives(&b, &a, "warm");
    let old = a.key();
    b.node.ignore_block_notices_for_test(true);
    a.node.block(b.did.clone()).unwrap();
    a.node.private_rotate_now().unwrap();
    let new = a.key();
    let t0 = Instant::now();
    let call = b.node.call(a.did.clone()).unwrap();
    let (reason, _) = ended_after(&b, &call.call_id, t0, 40);
    assert_eq!(reason, BLOCKED);
    assert!(!saw_incoming(&a));
    std::thread::sleep(Duration::from_secs(8));
    let dials = b.dials(&a);
    assert!(!dials.contains(&new), "the blocked contact learned the new key: {dials:?}");
    assert!(dials.contains(&old));
}

// KIND: acceptance
// A linked install follows: it learns the new key through the account doc and still syncs; the
// rotated key is not listed as an unlinked device anywhere.
#[test]
fn a_linked_device_follows() {
    let e = S.peer("own-e");
    let n = e.link_fresh(TAG, "own-n", LinkAs { new_pass: PASS, ..LinkAs::default() });
    e.node.private_rotate_now().unwrap();
    let new = e.key();
    // The new key may arrive by its own self-sync hello first; the tombstone of the old one
    // follows in the account doc.
    eventually(90, "n knows e's new key and hides the old one", || {
        let l = n.node.linked_devices();
        l.len() == 2 && l.iter().any(|d| d.device == new)
    });
    assert!(n.node.linked_devices().iter().all(|d| !d.removed));
    // A change on n still reaches e.
    n.node.set_device_label("own-n renamed".into()).unwrap();
    eventually(90, "e syncs with n", || e.node.linked_devices().iter().any(|d| d.label == "own-n renamed"));
}

// KIND: acceptance
// Restart mid-rotation: staged but not promoted keeps the old key (the staged one is gone);
// promoted but not completed comes back on the new key and completes (old key tombstoned,
// contacts told).
#[test]
fn a_restart_mid_rotation_keeps_one_key() {
    let (mut a, b) = S.pair("restart");
    message_arrives(&b, &a, "warm");
    let old = a.key();
    a.node.private_rotate_interrupted_for_test(false).unwrap();
    a.restart();
    assert_eq!(a.key(), old, "never promoted: the old key serves on");
    assert!(!a.node.overlap_open_for_test());
    message_arrives(&b, &a, "still the old key");

    a.node.private_rotate_interrupted_for_test(true).unwrap();
    a.restart();
    let new = a.key();
    assert_ne!(new, old, "promoted: the new key serves");
    assert_eq!(a.node.linked_devices().len(), 1);
    eventually(120, "b follows", || b.dials(&a) == vec![new.clone()]);
    message_arrives(&b, &a, "on the new key");
}

// KIND: acceptance
// Lost touch: both sides rotate while apart and neither can reach the other once the overlaps
// closed; each marks the other `lost_touch` when its announce window ends. A fresh ticket swap
// clears it on both sides.
#[test]
fn both_rotating_apart_lose_touch_until_a_ticket_swap() {
    let (mut a, mut b) = S.pair("lost");
    for p in [&a, &b] {
        short_windows(p, 15_000, 500);
    }
    message_arrives(&b, &a, "warm");
    b.node.stop();
    a.node.private_rotate_now().unwrap();
    eventually(20, "a's old address closes", || !a.node.overlap_open_for_test());
    a.node.stop();
    b.restart();
    short_windows(&b, 15_000, 500);
    b.node.private_rotate_now().unwrap();
    eventually(20, "b's old address closes", || !b.node.overlap_open_for_test());
    a.restart();
    short_windows(&a, 15_000, 500);
    eventually(90, "a lost touch with b", || a.contact(&b.did).lost_touch);
    eventually(90, "b lost touch with a", || b.contact(&a.did).lost_touch);
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    assert!(!b.contact(&a.did).lost_touch);
    eventually(30, "a is back in touch", || !a.contact(&b.did).lost_touch);
    message_arrives(&a, &b, "found again");
}
