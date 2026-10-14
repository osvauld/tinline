//! Real-code checks for the DID-signed device list (`proto::sign_device_list`,
//! `proto::verify_device_list`, `proto::newer`, `proto::list_seq_in_bounds`).
//!
//! The push-to-contacts outbox / block-notice / fork-resolution model that used to live here
//! moved to the `design-models` branch on 2026-10-14; see `docs/archive/models.md`.
use ed25519_dalek::SigningKey;
use proto::{SignedBlob, newer, sign_device_list, verify_device_list};

#[path = "support/fixtures.rs"]
mod fixtures;
use fixtures::*;

const BOB: u8 = 2;
const MALLORY: u8 = 9;

fn pk(k: &SigningKey) -> [u8; 32] {
    k.verifying_key().to_bytes()
}

fn dev(slot: u8, generation: u64) -> SigningKey {
    fixture_key(slot, generation)
}

fn rewrite(blob: &SignedBlob, edit: impl FnOnce(&mut serde_json::Value)) -> SignedBlob {
    let mut v: serde_json::Value = serde_json::from_slice(&base64_decode(&blob.payload)).unwrap();
    edit(&mut v);
    SignedBlob {
        payload: base64_encode(&serde_json::to_vec(&v).unwrap()),
        signature: blob.signature.clone(),
    }
}

// Forged (Mallory signs a payload claiming Bob's DID), tampered (seq bumped under Bob's real
// signature) and wrong-DID (Mallory's honest list offered as Bob's) lists are all refused.
#[test]
fn forged_tampered_and_wrong_did_lists_are_refused() {
    let bob = owner(BOB);
    let mallory = owner(MALLORY);
    let good = sign_device_list(&bob, &[(pk(&dev(2, 0)), None)], 20);
    assert_eq!(verify_device_list(&good, bob.did()).unwrap().seq, 20);

    let theirs = sign_device_list(&mallory, &[(pk(&dev(9, 0)), None)], 999);
    let forged = rewrite(&theirs, |v| v["did"] = serde_json::Value::String(did(BOB)));
    assert_eq!(verify_device_list(&forged, bob.did()), Err(proto::Error::BadSignature));

    let tampered = rewrite(&good, |v| v["seq"] = serde_json::json!(5000));
    assert_eq!(verify_device_list(&tampered, bob.did()), Err(proto::Error::BadSignature));

    assert_eq!(verify_device_list(&theirs, bob.did()), Err(proto::Error::WrongIssuer));
}

// `newer` compares seq first; at equal seq the larger signature wins, so every receiver
// converges on the same list regardless of delivery order.
#[test]
fn newer_prefers_higher_seq_then_larger_signature() {
    let bob = owner(BOB);
    let l1 = sign_device_list(&bob, &[(pk(&dev(2, 0)), None), (pk(&dev(3, 0)), None)], 30);
    let l2 = sign_device_list(&bob, &[(pk(&dev(2, 0)), None), (pk(&dev(4, 0)), None)], 30);
    assert_ne!(newer(&l1, &l2), newer(&l2, &l1));
    let higher = sign_device_list(&bob, &[(pk(&dev(2, 0)), None)], 31);
    assert!(newer(&higher, &l1) && newer(&higher, &l2));
    assert!(!newer(&l1, &higher) && !newer(&l2, &higher));
    assert!(!newer(&l1, &l1));
}

// A list signed at `seq = u64::MAX` (or more than a day ahead of the receiver's clock) would pin
// contacts forever; the seq bound refuses it.
#[test]
fn seq_bound_refuses_lists_too_far_ahead() {
    let now = 1_000_000;
    assert!(proto::list_seq_in_bounds(now, now));
    assert!(proto::list_seq_in_bounds(now + proto::MAX_LIST_SEQ_AHEAD_MS, now));
    assert!(!proto::list_seq_in_bounds(now + proto::MAX_LIST_SEQ_AHEAD_MS + 1, now));
    assert!(!proto::list_seq_in_bounds(u64::MAX, now));
}
