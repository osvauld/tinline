use super::*;
use crate::chat::doc::{MsgRec, Shard, Writer, doc_name, pair_id};
use receipts_core::{
    Conversation, Kind, MAX_MESSAGES, Receipt, ReceiptError, Scope,
    store::{ReceiptStore, messages::*},
    testkit::{self, TempDir},
};
use std::sync::Arc;
use storage::{Sealed, Store};

const LIMITS: crdt::batching::BatchLimits = crdt::batching::BatchLimits {
    bytes: 1 << 20,
    operations: 64,
    writers: 64,
};
const DAY: &str = "2026-10-15";
const AT: i64 = 1_792_022_400_000 + 3_600_000; // 2026-10-15T01:00Z

/// A ledger factory over the sealed store in `dir` (reopened by each call).
fn open(dir: &TempDir) -> Arc<ReceiptStore> {
    ReceiptStore::new(Sealed::new(Store::open(dir.0.join("r.redb")).unwrap(), [4; 32]))
}
fn memory() -> Arc<ReceiptStore> {
    testkit::memory(5)
}

struct Pair {
    alice: identity::Identity,
    bob: identity::Identity,
    pair: String,
    scope: Scope,
}
fn pair() -> Pair {
    let alice = identity::generate().0;
    let bob = identity::generate().0;
    let pair = pair_id(alice.did(), bob.did());
    let scope = Scope {
        conversation: Conversation::Direct(pair.clone()),
        day: DAY.into(),
    };
    Pair {
        alice,
        bob,
        pair,
        scope,
    }
}

/// Real DID-signed versions written by `author` into the shared day doc.
fn versions(p: &Pair, author: &identity::Identity, n: usize) -> (Shard, Vec<MsgRec>) {
    let dev = [7u8; 32];
    let w = Writer::new(author.clone(), dev);
    let shard = Shard::new(&p.pair, DAY, w.writer_id());
    for i in 0..n {
        let rec = MsgRec {
            id: format!("{i:032x}"),
            author: author.did().into(),
            at: AT + i as i64,
            text: format!("hello {i}"),
            edited_at: None,
            deleted: false,
            reply_to: None,
            file: None,
            dev,
            did_sig: vec![],
        };
        shard.add_message(&rec, &w).unwrap();
    }
    let recs = shard.messages().unwrap().into_values().collect();
    (shard, recs)
}
fn digests(p: &Pair, recs: &[MsgRec]) -> Vec<[u8; 32]> {
    recs.iter()
        .map(|r| dm_version(&doc_name(&p.pair, DAY), r))
        .collect()
}

#[test]
fn dm_version_binds_exact_signed_version_and_doc() {
    let p = pair();
    let (shard, recs) = versions(&p, &p.alice, 1);
    let doc = doc_name(&p.pair, DAY);
    let v1 = dm_version(&doc, &recs[0]);
    assert_eq!(
        v1,
        dm_version(&doc, &shard.messages().unwrap()[&recs[0].id])
    );
    assert_ne!(v1, dm_version(&doc_name(&p.pair, "2026-10-16"), &recs[0]));
    // An edit is a new DID-signed version: a receipt for v1 must not cover it.
    shard
        .edit(
            &recs[0].id,
            "edited",
            AT + 10,
            &Writer::new(p.alice.clone(), [7; 32]),
        )
        .unwrap();
    let v2 = dm_version(&doc, &shard.messages().unwrap()[&recs[0].id]);
    assert_ne!(v1, v2);
    let mut other_dev = recs[0].clone();
    other_dev.dev[0] ^= 1;
    assert_ne!(v1, dm_version(&doc, &other_dev));
}

#[test]
fn intent_commits_exact_versions_atomically_and_survives_reopen() {
    let p = pair();
    let (_, recs) = versions(&p, &p.alice, 3);
    let ds = digests(&p, &recs);
    let dir = TempDir::new("tinline-message-receipts");
    let issued = {
        let store = open(&dir);
        let bob = MessageReceipts::new(store.clone(), p.bob.did(), [2; 32]);
        // Only A and C were read: B must stay a hole.
        let ops = bob
            .intent_ops(Kind::Read, &p.scope, [ds[0], ds[2]])
            .unwrap();
        store.db().apply(&ops).unwrap();
        assert_eq!(bob.pending().unwrap(), 1);
        let issued = bob.pump(&p.bob).unwrap();
        assert_eq!(issued.len(), 1);
        assert_eq!(bob.pending().unwrap(), 0);
        assert!(bob.pump(&p.bob).unwrap().is_empty());
        issued
    };
    let store = open(&dir);
    let bob = MessageReceipts::new(store, p.bob.did(), [2; 32]);
    let is_bob = |d: &str| d == p.bob.did();
    for (i, d) in ds.iter().enumerate() {
        let e = bob.evidence(&p.scope, d, is_bob).unwrap();
        assert!(e.delivered.is_empty());
        assert_eq!(e.read.len(), usize::from(i != 1), "version {i}");
    }
    let e = bob.evidence(&p.scope, &ds[0], is_bob).unwrap();
    assert_eq!(e.read[0].id, issued[0].entry.id);
    assert_eq!(e.read[0].receipt.device, [2; 32]);
    assert_eq!(e.read[0].receipt.messages, {
        let mut v = vec![ds[0], ds[2]];
        v.sort();
        v
    });
}

#[test]
fn failed_commit_keeps_intent_and_nothing_projects_until_retry() {
    let p = pair();
    let (_, recs) = versions(&p, &p.alice, 1);
    let ds = digests(&p, &recs);
    let store = memory();
    let bob = MessageReceipts::new(store.clone(), p.bob.did(), [2; 32]);
    store
        .db()
        .apply(
            &bob.intent_ops(Kind::Delivered, &p.scope, ds.clone())
                .unwrap(),
        )
        .unwrap();
    store.fail_next_commit();
    assert!(bob.pump(&p.bob).is_err());
    assert_eq!(bob.pending().unwrap(), 1);
    let none = bob.evidence(&p.scope, &ds[0], |_| true).unwrap();
    assert!(none.delivered.is_empty() && none.read.is_empty());
    assert_eq!(bob.pump(&p.bob).unwrap().len(), 1);
    assert_eq!(bob.pending().unwrap(), 0);
    assert_eq!(
        bob.evidence(&p.scope, &ds[0], |_| true)
            .unwrap()
            .delivered
            .len(),
        1
    );
    // Duplicate obligation (e.g. replayed batch): no second statement.
    store
        .db()
        .apply(
            &bob.intent_ops(Kind::Delivered, &p.scope, ds.clone())
                .unwrap(),
        )
        .unwrap();
    assert!(bob.pump(&p.bob).unwrap().is_empty());
    assert_eq!(bob.pending().unwrap(), 0);
    assert_eq!(store_entries(&bob, &p.scope), 1);
}
fn store_entries(c: &MessageReceipts, scope: &Scope) -> usize {
    c.ledger(scope).unwrap().entries().unwrap().len()
}

#[test]
fn large_obligation_splits_into_bounded_statements_and_completes_once() {
    let p = pair();
    let store = memory();
    let bob = MessageReceipts::new(store.clone(), p.bob.did(), [2; 32]);
    let ds: Vec<[u8; 32]> = (0..(MAX_MESSAGES + 44) as u32)
        .map(|i| *blake3::hash(&i.to_be_bytes()).as_bytes())
        .collect();
    store
        .db()
        .apply(
            &bob.intent_ops(Kind::Delivered, &p.scope, ds.clone())
                .unwrap(),
        )
        .unwrap();
    let issued = bob.pump(&p.bob).unwrap();
    assert_eq!(issued.len(), 2);
    assert!(
        issued
            .iter()
            .all(|i| i.entry.receipt.messages.len() <= MAX_MESSAGES)
    );
    assert_eq!(bob.pending().unwrap(), 0);
    for d in &ds {
        assert_eq!(
            bob.evidence(&p.scope, d, |_| true).unwrap().delivered.len(),
            1
        );
    }
}

#[test]
fn another_own_device_receipt_counts_for_the_account_and_is_not_reissued() {
    let p = pair();
    let (_, recs) = versions(&p, &p.alice, 2);
    let ds = digests(&p, &recs);
    // Bob's phone issues delivered for both.
    let phone_store = memory();
    let phone = MessageReceipts::new(phone_store.clone(), p.bob.did(), [2; 32]);
    phone_store
        .db()
        .apply(
            &phone
                .intent_ops(Kind::Delivered, &p.scope, ds.clone())
                .unwrap(),
        )
        .unwrap();
    phone.pump(&p.bob).unwrap();
    let update = phone
        .ledger(&p.scope)
        .unwrap()
        .export_batch(&Default::default(), LIMITS)
        .unwrap()
        .unwrap()
        .update;
    // Bob's laptop holds that statement (own-device relay is a separate,
    // unwired policy; the low-level ledger import stands in for it here).
    let laptop_store = memory();
    let laptop = MessageReceipts::new(laptop_store.clone(), p.bob.did(), [3; 32]);
    laptop
        .ledger(&p.scope)
        .unwrap()
        .apply_remote(&update, |_| Ok(()))
        .unwrap();
    laptop_store
        .db()
        .apply(
            &laptop
                .intent_ops(Kind::Delivered, &p.scope, ds.clone())
                .unwrap(),
        )
        .unwrap();
    assert!(
        laptop.pump(&p.bob).unwrap().is_empty(),
        "already covered by the account"
    );
    assert_eq!(laptop.pending().unwrap(), 0);
    // A read by the laptop is a NEW kind: delivered coverage does not imply it.
    laptop_store
        .db()
        .apply(&laptop.intent_ops(Kind::Read, &p.scope, [ds[0]]).unwrap())
        .unwrap();
    assert_eq!(laptop.pump(&p.bob).unwrap().len(), 1);

    // Alice imports from Bob's laptop session: one device suffices.
    let alice = MessageReceipts::new(memory(), p.alice.did(), [9; 32]);
    let all = laptop
        .export(&p.scope, &Default::default(), LIMITS)
        .unwrap()
        .unwrap();
    let fresh = alice
        .import_direct(&p.scope, p.bob.did(), &all.update)
        .unwrap();
    assert_eq!(fresh.len(), 2);
    let from_bob = |d: &str| d == p.bob.did();
    let e0 = alice.evidence(&p.scope, &ds[0], from_bob).unwrap();
    assert_eq!((e0.delivered.len(), e0.read.len()), (1, 1));
    assert_eq!(e0.delivered[0].receipt.device, [2; 32]);
    assert_eq!(e0.read[0].receipt.device, [3; 32]);
    let e1 = alice.evidence(&p.scope, &ds[1], from_bob).unwrap();
    assert_eq!((e1.delivered.len(), e1.read.len()), (1, 0));
    // Replay: idempotent, nothing new, projection unchanged.
    assert!(
        alice
            .import_direct(&p.scope, p.bob.did(), &all.update)
            .unwrap()
            .is_empty()
    );
    assert_eq!(alice.evidence(&p.scope, &ds[0], from_bob).unwrap(), e0);
}

#[test]
fn import_refuses_foreign_issuers_wrong_sessions_and_call_claims() {
    let p = pair();
    let (_, recs) = versions(&p, &p.alice, 1);
    let ds = digests(&p, &recs);
    let mallory = identity::generate().0;
    // Mallory signs a delivered claim in Alice/Bob's scope under her own key.
    let forged_store = memory();
    let forged = MessageReceipts::new(forged_store.clone(), mallory.did(), [6; 32]);
    let ledger = forged.ledger(&p.scope).unwrap();
    ledger
        .write(
            &Receipt {
                scope: p.scope.clone(),
                kind: Kind::Delivered,
                recipient: mallory.did().into(),
                device: [6; 32],
                messages: ds.clone(),
                metadata: vec![],
                call: None,
            },
            &mallory,
            |_| Ok(()),
        )
        .unwrap();
    let limits = LIMITS;
    let update = ledger
        .export_batch(&Default::default(), limits)
        .unwrap()
        .unwrap()
        .update;
    let alice = MessageReceipts::new(memory(), p.alice.did(), [9; 32]);
    // Presented on Bob's session: issuer is not a party.
    assert!(matches!(
        alice.import_direct(&p.scope, p.bob.did(), &update),
        Err(ReceiptError::Policy(_))
    ));
    // Mallory's own session cannot name the Alice/Bob conversation at all.
    assert_eq!(
        alice
            .import_direct(&p.scope, mallory.did(), &update)
            .unwrap_err(),
        ReceiptError::Scope
    );
    // We are never our own remote session.
    assert_eq!(
        alice
            .import_direct(&p.scope, p.alice.did(), &update)
            .unwrap_err(),
        ReceiptError::Scope
    );
    assert!(
        alice
            .ledger(&p.scope)
            .unwrap()
            .entries()
            .unwrap()
            .is_empty()
    );
    assert!(alice.ledger(&p.scope).unwrap().vv().is_empty());
    assert!(alice.intent_ops(Kind::Call, &p.scope, ds).is_err());
}

#[test]
fn issuing_marks_the_outbox_atomically_until_the_counterparty_acknowledges() {
    let p = pair();
    let (_, recs) = versions(&p, &p.alice, 1);
    let ds = digests(&p, &recs);
    let store = memory();
    let bob = MessageReceipts::new(store.clone(), p.bob.did(), [2; 32]);
    let conv = Conversation::Direct(p.pair.clone());
    store
        .db()
        .apply(
            &bob.intent_ops(Kind::Delivered, &p.scope, ds.clone())
                .unwrap(),
        )
        .unwrap();
    // A failed commit leaves neither a statement nor an outbox mark.
    store.fail_next_commit();
    assert!(bob.pump(&p.bob).is_err());
    assert!(bob.outbox_days(&conv, 8).unwrap().is_empty());
    bob.pump(&p.bob).unwrap();
    assert_eq!(bob.outbox_days(&conv, 8).unwrap(), vec![DAY.to_string()]);
    // Holding nothing / only someone else's ops does not complete it.
    assert!(!bob.acknowledged(&p.scope, &Default::default()).unwrap());
    let mut other = loro::VersionVector::default();
    other.insert(42, 5);
    assert!(!bob.acknowledged(&p.scope, &other).unwrap());
    assert_eq!(bob.outbox_days(&conv, 8).unwrap().len(), 1);
    let theirs = bob.vv(&p.scope).unwrap();
    assert!(bob.acknowledged(&p.scope, &theirs).unwrap());
    assert!(bob.outbox_days(&conv, 8).unwrap().is_empty());
    // Another conversation's outbox is separate.
    assert!(
        bob.outbox_days(&Conversation::Direct("b".repeat(64)), 8)
            .unwrap()
            .is_empty()
    );
}
