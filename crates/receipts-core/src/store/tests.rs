use super::*;
use crate::{
    calls::tests::claim,
    testkit::{self, TempDir},
    tests::sample,
};
use crdt::writer_id;
fn ok(_: &[Entry]) -> Result<(), String> {
    Ok(())
}
fn memory() -> Arc<ReceiptStore> {
    testkit::memory(9)
}

#[test]
fn commit_failure_keeps_live_and_disk_empty_then_retry_commits_original_and_pointer() {
    let a = identity::generate().0;
    let c = sample(&a, Kind::Read, true);
    let store = memory();
    let ledger = store
        .open(c.scope.clone(), writer_id(a.did(), &c.device), ok)
        .unwrap();
    store.fail_next_commit();
    assert!(matches!(
        ledger.write(&c, &a, ok),
        Err(ReceiptError::Store(_))
    ));
    assert!(ledger.vv().is_empty());
    assert!(ledger.entries().unwrap().is_empty());
    assert!(store.db.scan(&ledger.prefix).unwrap().is_empty());
    let result = ledger.write(&c, &a, ok).unwrap();
    assert_eq!(result.entries.len(), 1);
    let entry = &result.entries[0];
    assert_eq!(
        ledger.raw(&entry.id).unwrap(),
        c.sign_claim(&a).unwrap().to_bytes()
    );
    let pointer: Position = cryptography::codec::from_bytes(
        &store
            .db
            .get(&format!("{}index/{}", ledger.prefix, entry.id))
            .unwrap()
            .unwrap(),
        usize::MAX,
    )
    .unwrap();
    assert_eq!(
        pointer,
        Position {
            peer: entry.peer,
            counter: entry.counter
        }
    );
    assert!(ledger.write(&c, &a, ok).unwrap().update.is_none());
    assert_eq!(
        store
            .db
            .scan(&format!("{}u/", ledger.prefix))
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn receiver_failure_and_policy_refusal_do_not_publish_frontier_or_index() {
    let a = identity::generate().0;
    let c = claim(&a, true);
    let store = memory();
    let source = store
        .open(c.scope.clone(), writer_id(a.did(), &c.device), ok)
        .unwrap();
    let committed = source.write(&c, &a, ok).unwrap();
    let update = committed.update.unwrap();
    let target_store = memory();
    let target = target_store.open(c.scope.clone(), 8, ok).unwrap();
    assert!(
        target
            .apply_remote(&update, |_| Err("unverified supporting roster".into()))
            .is_err()
    );
    target_store.fail_next_commit();
    assert!(target.apply_remote(&update, ok).is_err());
    assert!(target.vv().is_empty());
    assert!(target_store.db.scan(&target.prefix).unwrap().is_empty());
    target.apply_remote(&update, ok).unwrap();
    assert_eq!(source.vv(), target.vv());
    assert_eq!(source.entries().unwrap(), target.entries().unwrap());
}
#[test]
fn single_live_owner_enforced_and_failed_open_releases_scope_lease() {
    let a = identity::generate().0;
    let c = sample(&a, Kind::Delivered, false);
    let store = memory();
    let first = store.open(c.scope.clone(), 9, ok).unwrap();
    assert!(store.open(c.scope.clone(), 10, ok).is_err());
    drop(first);
    assert!(
        store
            .open(c.scope.clone(), 9, |_| Err("not admitted".into()))
            .is_err()
    );
    assert!(store.open(c.scope.clone(), 9, ok).is_ok());
}
#[test]
fn real_sealed_reopen_restores_typed_call_observations_and_message_read_holes() {
    let directory = TempDir::new("tinline-receipt-ledger");
    let path = directory.0.join("shared-receipts.redb");
    let key = [9; 32];
    let a = identity::generate().0;
    let c = claim(&a, true);
    let message = sample(&a, Kind::Read, false);
    let peer = writer_id(a.did(), &c.device);
    let sealed = Sealed::new(storage::Store::open(&path).unwrap(), key);
    let store = ReceiptStore::new(sealed.clone());
    let call = store.open(c.scope.clone(), peer, ok).unwrap();
    let messages = store.open(message.scope.clone(), peer, ok).unwrap();
    let call_entry = call.write(&c, &a, ok).unwrap().entries.remove(0);
    messages.write(&message, &a, ok).unwrap();
    drop(call);
    drop(messages);
    drop(store);
    sealed.close();
    drop(sealed);
    let reopened = ReceiptStore::new(Sealed::new(storage::Store::open(&path).unwrap(), key));
    let recovered = reopened.open(c.scope.clone(), peer, ok).unwrap();
    let reads = reopened.open(message.scope.clone(), peer, ok).unwrap();
    assert_eq!(
        recovered.raw(&call_entry.id).unwrap(),
        c.sign_claim(&a).unwrap().to_bytes()
    );
    assert_eq!(recovered.entries().unwrap()[0].receipt.call, c.call);
    assert_eq!(
        reads.entries().unwrap()[0].receipt.messages,
        vec![[1; 32], [3; 32]]
    );
    assert!(recovered.write(&c, &a, ok).unwrap().update.is_none());
}
#[test]
fn already_known_receipt_under_new_native_counter_persists_vv_without_duplicate_index() {
    let a = identity::generate().0;
    let c = sample(&a, Kind::Delivered, false);
    let store = memory();
    let peer = writer_id(a.did(), &c.device);
    let ledger = store.open(c.scope.clone(), peer, ok).unwrap();
    let first = ledger.write(&c, &a, ok).unwrap();
    let original = first.entries[0].clone();
    let raw = loro::LoroDoc::new();
    raw.set_peer_id(peer).unwrap();
    // Loro optimizes equal-value local writes away. An alternate synthetic
    // writer prefix exercises a new native range containing a known receipt,
    // as in the existing group durability fixture. NOT a writer-fork safety claim.
    let mut prefix_claim = c.clone();
    prefix_claim.kind = Kind::Read;
    let prefix_bytes = prefix_claim.sign_claim(&a).unwrap().to_bytes();
    raw.get_map("receipts")
        .insert(
            &receipt_id(&prefix_bytes),
            loro::LoroValue::Binary(prefix_bytes.into()),
        )
        .unwrap();
    raw.commit();
    let before = raw.oplog_vv();
    raw.get_map("receipts")
        .insert(
            &original.id,
            loro::LoroValue::Binary(ledger.raw(&original.id).unwrap().into()),
        )
        .unwrap();
    raw.commit();
    let extra = raw.export(loro::ExportMode::updates(&before)).unwrap();
    let applied = ledger.apply_remote(&extra, ok).unwrap();
    assert!(applied.entries.is_empty());
    assert!(applied.update.is_some());
    assert_eq!(applied.frontier.get(&peer), Some(&2));
    drop(ledger);
    let recovered = store.open(c.scope.clone(), peer, ok).unwrap();
    assert_eq!(recovered.vv().get(&peer), Some(&2));
    assert_eq!(recovered.entries().unwrap(), vec![original]);
    assert_eq!(
        store
            .db
            .scan(&format!("{}index/", recovered.prefix))
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn persisted_frontier_or_sequence_inconsistency_is_not_empty_history() {
    let a = identity::generate().0;
    let c = sample(&a, Kind::Delivered, false);
    let store = memory();
    let ledger = store
        .open(c.scope.clone(), writer_id(a.did(), &c.device), ok)
        .unwrap();
    ledger.write(&c, &a, ok).unwrap();
    let p = ledger.prefix.clone();
    drop(ledger);
    let bad = StoredState {
        seq: 1,
        vv: VersionVector::default().encode(),
    };
    store
        .db
        .put(&format!("{p}state"), &cryptography::codec::to_bytes(&bad).unwrap())
        .unwrap();
    assert!(store.open(c.scope.clone(), 9, ok).is_err());
    store.db.delete(&format!("{p}u/{:020}", 1)).unwrap();
    assert!(store.open(c.scope.clone(), 9, ok).is_err());
}
