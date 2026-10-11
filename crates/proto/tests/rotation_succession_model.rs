//! Test-only strict-history candidate. Not an exported protocol or frozen wire format.
//! Real DID signatures; fake transport addresses. No networking, expiry or disk I/O.
use identity::Identity;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"test-only/rotation-succession/v1\0";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Record {
    slot: u64,
    audience: String,
    generation: u64,
    previous: Option<[u8; 32]>,
    endpoint: [u8; 32],
}

#[derive(Clone, Debug)]
struct Signed {
    record: Record,
    signature: [u8; 64],
}

fn payload(record: &Record) -> Vec<u8> {
    [DOMAIN, &serde_json::to_vec(record).unwrap()].concat()
}

impl Signed {
    fn new(owner: &Identity, record: Record) -> Self {
        let signature = owner.sign(&payload(&record));
        Self { record, signature }
    }
    fn hash(&self) -> [u8; 32] {
        Sha256::digest([payload(&self.record), self.signature.to_vec()].concat()).into()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Receiver {
    owner_key: [u8; 32],
    slot: u64,
    audience: String,
    // In production these fields must be persisted atomically with the address.
    head: Option<(u64, [u8; 32], [u8; 32])>,
}

impl Receiver {
    fn new(owner: &Identity, slot: u64) -> Self {
        Self { owner_key: owner.signing_public_key(), slot, audience: "private".into(), head: None }
    }
    fn accept(&mut self, signed: &Signed) -> Result<(), &'static str> {
        let r = &signed.record;
        if !identity::verify(&self.owner_key, &payload(r), &signed.signature) {
            return Err("signature");
        }
        if r.slot != self.slot || r.audience != self.audience {
            return Err("scope");
        }
        let hash = signed.hash();
        match self.head {
            None if r.generation != 0 || r.previous.is_some() => return Err("gap"),
            Some((generation, _, _)) if r.generation < generation => return Err("stale"),
            Some((generation, old_hash, _)) if r.generation == generation => {
                return if old_hash == hash { Ok(()) } else { Err("fork") };
            }
            Some((generation, old_hash, _)) => {
                if generation.checked_add(1) != Some(r.generation) { return Err("gap"); }
                if r.previous != Some(old_hash) { return Err("predecessor"); }
            }
            None => {}
        }
        self.head = Some((r.generation, hash, r.endpoint));
        Ok(())
    }
}

// Public deterministic seeds below are fixtures ONLY, not a proposed key derivation.
fn chain(owner: &Identity, slot: u64, count: u64) -> Vec<Signed> {
    let mut records: Vec<Signed> = Vec::new();
    for generation in 0..count {
        records.push(Signed::new(owner, Record {
            slot, audience: "private".into(), generation,
            previous: records.last().map(Signed::hash),
            endpoint: proto::device_public(&Sha256::digest(generation.to_be_bytes()).into()),
        }));
    }
    records
}

#[test]
fn ordered_history_and_duplicate_are_idempotent() {
    let (id, _) = identity::generate();
    let mut receiver = Receiver::new(&id, 1);
    for record in chain(&id, 1, 4) {
        receiver.accept(&record).unwrap();
        let before = receiver.clone();
        receiver.accept(&record).unwrap();
        assert_eq!(receiver, before);
    }
}

#[test]
fn missing_history_requires_catchup_and_survives_serialized_restart() {
    let (id, _) = identity::generate();
    let records = chain(&id, 1, 8);
    let mut receiver = Receiver::new(&id, 1);
    receiver.accept(&records[0]).unwrap();
    let before = receiver.clone();
    assert_eq!(receiver.accept(&records[7]), Err("gap"));
    assert_eq!(receiver, before);
    receiver = serde_json::from_slice(&serde_json::to_vec(&receiver).unwrap()).unwrap();
    for record in &records[1..] { receiver.accept(record).unwrap(); }
    let before = receiver.clone();
    assert_eq!(receiver.accept(&records[0]), Err("stale"));
    assert_eq!(receiver, before);
    assert_eq!(receiver.head.unwrap().0, 7);
}

#[test]
fn forged_tampered_cross_slot_and_cross_audience_updates_do_not_mutate_state() {
    let (id, _) = identity::generate();
    let (stranger, _) = identity::generate();
    let good = chain(&id, 1, 1).remove(0);
    let receiver = Receiver::new(&id, 1);
    let mut tampered = good.clone();
    tampered.record.endpoint[0] ^= 1;
    let mut wrong_audience = good.record.clone();
    wrong_audience.audience = "group".into();
    for (bad, reason) in [
        (tampered, "signature"),
        (Signed::new(&stranger, good.record.clone()), "signature"),
        (chain(&id, 2, 1).remove(0), "scope"),
        (Signed::new(&id, wrong_audience), "scope"),
    ] {
        let mut state = receiver.clone();
        assert_eq!(state.accept(&bad), Err(reason));
        assert_eq!(state, receiver);
    }
}

#[test]
fn owner_signed_fork_and_wrong_predecessor_are_detected() {
    let (id, _) = identity::generate();
    let records = chain(&id, 1, 3);
    let mut receiver = Receiver::new(&id, 1);
    receiver.accept(&records[0]).unwrap();
    receiver.accept(&records[1]).unwrap();
    let before = receiver.clone();
    let mut fork = records[1].record.clone();
    fork.endpoint = [99; 32];
    assert_eq!(receiver.accept(&Signed::new(&id, fork)), Err("fork"));
    let mut spliced = records[2].record.clone();
    spliced.previous = Some(records[0].hash());
    assert_eq!(receiver.accept(&Signed::new(&id, spliced)), Err("predecessor"));
    assert_eq!(receiver, before);
}

#[test]
fn every_three_record_delivery_order_catches_up_without_rollback() {
    let (id, _) = identity::generate();
    let records = chain(&id, 1, 3);
    for order in [[0,1,2], [0,2,1], [1,0,2], [1,2,0], [2,0,1], [2,1,0]] {
        let mut receiver = Receiver::new(&id, 1);
        for index in order {
            let before = receiver.clone();
            if receiver.accept(&records[index]).is_err() { assert_eq!(receiver, before); }
        }
        // Explicit replay/catch-up; the receiver does not silently accept gaps.
        for record in &records { let _ = receiver.accept(record); }
        assert_eq!(receiver.head.unwrap().0, 2);
    }
}

// Network composition lives in a separate test-only module but uses this exact verifier.
#[path = "support/rotation_network.rs"]
mod network;
