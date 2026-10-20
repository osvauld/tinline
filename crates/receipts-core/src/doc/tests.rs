use super::*;
use crate::tests::sample;
fn ok(_: &[Entry]) -> Result<(), String> {
    Ok(())
}
fn new(c: &Receipt, peer: u64) -> ReceiptDoc {
    ReceiptDoc::new(c.scope.clone(), peer).unwrap()
}
fn limits(operations: usize) -> BatchLimits {
    BatchLimits {
        bytes: 64 * 1024,
        operations,
        writers: 16,
    }
}

#[test]
fn original_delivery_and_read_receipts_cross_two_native_hops_without_reauthoring() {
    let a = identity::generate().0;
    let c = sample(&a, Kind::Delivered, true);
    let mut source = new(&c, writer_id(a.did(), &c.device));
    let mut laptop = new(&c, 77);
    let mut phone = new(&c, 88);
    let (delivered, first) = source.write(&c, &a, ok).unwrap().unwrap();
    let mut read = c.clone();
    read.kind = Kind::Read;
    let (read_entry, _) = source.write(&read, &a, ok).unwrap().unwrap();
    let mut loops = 0;
    while let Some(batch) = source.export_batch(&laptop.vv(), limits(1)).unwrap() {
        assert_eq!(batch.operations, 1);
        laptop.apply_remote(&batch.update, ok).unwrap();
        loops += 1;
    }
    assert_eq!(loops, 2);
    while let Some(batch) = laptop.export_batch(&phone.vv(), limits(1)).unwrap() {
        phone.apply_remote(&batch.update, ok).unwrap();
    }
    assert_eq!(phone.entries().unwrap(), source.entries().unwrap());
    assert_eq!(phone.raw(&delivered.id), source.raw(&delivered.id));
    assert_eq!(phone.raw(&read_entry.id), source.raw(&read_entry.id));
    assert!(phone.apply_remote(&first, ok).unwrap().is_empty());
    assert!(source.write(&c, &a, ok).unwrap().is_none());
}
#[test]
fn policy_refusal_and_checkpoint_restore_leave_document_and_writer_unchanged() {
    let a = identity::generate().0;
    let c = sample(&a, Kind::Read, false);
    let peer = writer_id(a.did(), &c.device);
    let mut source = new(&c, peer);
    let before = source.vv();
    assert!(matches!(
        source.write(&c, &a, |_| Err("not read".into())),
        Err(ReceiptError::Policy(_))
    ));
    assert_eq!(source.vv(), before);
    let (entry, update) = source.write(&c, &a, ok).unwrap().unwrap();
    let mut target = new(&c, 9);
    let saved = target.checkpoint().unwrap();
    assert!(
        target
            .apply_remote(&update, |_| Err("no admission".into()))
            .is_err()
    );
    assert_eq!(target.vv(), before);
    target.apply_remote(&update, ok).unwrap();
    target = saved;
    assert_eq!(target.vv(), before);
    target.apply_remote(&update, ok).unwrap();
    assert_eq!(target.entries().unwrap(), vec![entry]);
    assert_eq!(target.doc.peer_id(), 9);
}
#[test]
fn malformed_wrong_scope_foreign_writer_and_noninsert_operations_rollback() {
    let a = identity::generate().0;
    let c = sample(&a, Kind::Delivered, true);
    let bytes = c.sign_claim(&a).unwrap().to_bytes();
    let key = receipt_id(&bytes);
    for scenario in 0..6 {
        let raw = LoroDoc::new();
        raw.set_peer_id(if scenario == 0 {
            99
        } else {
            writer_id(a.did(), &c.device)
        })
        .unwrap();
        match scenario {
            1 => {
                raw.get_map(ROOT)
                    .insert("wrong key", LoroValue::Binary(bytes.clone().into()))
                    .unwrap();
            }
            2 => {
                raw.get_map("other")
                    .insert(&key, LoroValue::Binary(bytes.clone().into()))
                    .unwrap();
            }
            3 => {
                raw.get_map(ROOT).insert(&key, "not signed bytes").unwrap();
            }
            4 => {
                raw.get_map(ROOT)
                    .insert(&key, LoroValue::Binary(bytes.clone().into()))
                    .unwrap();
                raw.commit();
                raw.get_map(ROOT).delete(&key).unwrap();
            }
            _ => {
                raw.get_map(ROOT)
                    .insert(&key, LoroValue::Binary(bytes.clone().into()))
                    .unwrap();
            }
        }
        raw.commit();
        let update = raw.export(ExportMode::Snapshot).unwrap();
        let mut scope = c.scope.clone();
        if scenario == 5 {
            scope.day = "1970-01-02".into();
        }
        let mut target = ReceiptDoc::new(scope, 9).unwrap();
        assert!(target.apply_remote(&update, ok).is_err());
        assert!(target.vv().is_empty());
        assert!(target.entries().unwrap().is_empty());
    }
}
#[test]
fn missing_predecessor_fails_without_mutation_then_succeeds_after_prefix() {
    let a = identity::generate().0;
    let c = sample(&a, Kind::Delivered, false);
    let mut source = new(&c, writer_id(a.did(), &c.device));
    let (_, first) = source.write(&c, &a, ok).unwrap().unwrap();
    let prefix = source.vv();
    let mut read = c.clone();
    read.kind = Kind::Read;
    source.write(&read, &a, ok).unwrap();
    let later = source.export_batch(&prefix, limits(1)).unwrap().unwrap();
    let mut target = new(&c, 9);
    assert_eq!(
        target.apply_remote(&later.update, ok),
        Err(ReceiptError::Pending)
    );
    assert!(target.vv().is_empty());
    target.apply_remote(&first, ok).unwrap();
    target.apply_remote(&later.update, ok).unwrap();
    assert_eq!(target.entries().unwrap(), source.entries().unwrap());
}
#[test]
fn full_native_snapshot_and_remainder_preserve_signatures_and_resume_frontier() {
    let a = identity::generate().0;
    let c = sample(&a, Kind::Read, true);
    let mut source = new(&c, writer_id(a.did(), &c.device));
    source.write(&c, &a, ok).unwrap();
    let snap = source.snapshot().unwrap();
    let mut next = c.clone();
    next.messages.push([4; 32]);
    source.write(&next, &a, ok).unwrap();
    let mut recovered = ReceiptDoc::load(c.scope.clone(), 9, Some(&snap), &[], ok).unwrap();
    let batch = source
        .export_batch(&recovered.vv(), limits(1))
        .unwrap()
        .unwrap();
    assert_eq!(batch.operations, 1);
    recovered.apply_remote(&batch.update, ok).unwrap();
    assert_eq!(recovered.entries().unwrap(), source.entries().unwrap());
    assert!(
        ReceiptDoc::load(c.scope.clone(), 9, Some(&snap), &[], |_| Err(
            "not admitted".into()
        ))
        .is_err()
    );
}
#[test]
fn concurrent_accounts_converge_and_indivisible_receipt_over_budget_defers() {
    let a = identity::generate().0;
    let b = identity::generate().0;
    let ca = sample(&a, Kind::Delivered, true);
    let cb = sample(&b, Kind::Read, true);
    let mut da = new(&ca, writer_id(a.did(), &ca.device));
    let mut db = new(&cb, writer_id(b.did(), &cb.device));
    let (_, ua) = da.write(&ca, &a, ok).unwrap().unwrap();
    let (_, ub) = db.write(&cb, &b, ok).unwrap().unwrap();
    da.apply_remote(&ub, ok).unwrap();
    db.apply_remote(&ua, ok).unwrap();
    assert_eq!(da.entries().unwrap(), db.entries().unwrap());
    assert!(matches!(
        da.export_batch(
            &VersionVector::default(),
            BatchLimits {
                bytes: 1,
                ..limits(1)
            }
        ),
        Err(BatchError::Oversized { .. })
    ));
}
