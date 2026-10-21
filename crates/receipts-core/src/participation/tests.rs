use super::*;
use crdt::writer_id;
use gcall_core::{ClientAction, ClientEngine, ClientState};
use storage::{Sealed, Store};
struct Authority {
    allowed: bool,
}
impl CallAuthority for Authority {
    fn is_member(&self, _: &str, _: &str) -> bool {
        self.allowed
    }
    fn host_permit_ok(&self, _: &str, _: &w::HostRef) -> bool {
        self.allowed
    }
    fn endpoint_ok(&self, _: &str, _: &str, _: Option<&w::SignedNote>, _: &[u8; 32]) -> bool {
        self.allowed
    }
}
fn verify_import(claims: &[store::ImportClaim], evidence: &[Evidence]) -> Result<(), String> {
    for claim in claims {
        verify_joined_entry(&claim.entry, |id| {
            Ok(evidence.iter().find(|e| e.id == *id).cloned())
        })
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn sidecars(p: &JoinProofs) -> Vec<Evidence> {
    vec![
        Evidence::from_blob(&p.start).unwrap(),
        Evidence::from_blob(&p.expected_join).unwrap(),
        Evidence::from_blob(&p.roster).unwrap(),
    ]
}
fn replica(p: &JoinProofs, peer: u64) -> (std::sync::Arc<store::ReceiptStore>, Ledger) {
    let db = crate::testkit::memory(9);
    let l = db
        .open(
            Scope {
                conversation: Conversation::Group(p.start.open().unwrap().gid),
                day: "1970-01-01".into(),
            },
            peer,
            |_| Ok(()),
        )
        .unwrap();
    (db, l)
}
pub(crate) fn fixture() -> (identity::Identity, JoinProofs) {
    let host = identity::generate().0;
    let local = identity::generate().0;
    let auth = Authority { allowed: true };
    let start = w::CallStart {
        gid: "a".repeat(64),
        call_id: w::new_call_id(),
        host_gen: 1,
        host_ref: w::HostRef {
            holder_did: host.did().into(),
            device_slot: 1,
            permit_digest: "d".repeat(64),
        },
        starter_did: host.did().into(),
        started_ms: 0,
        k: 3,
        max_participants: 12,
        backups: vec![],
        meta_frontier: "m".repeat(64),
    };
    let signed_start = Signed::sign(&start, &host).unwrap();
    let (mut client, actions) =
        ClientEngine::new(local.clone(), start.clone(), None, 1, &auth).unwrap();
    let join = actions
        .into_iter()
        .find_map(|a| {
            if let ClientAction::Control(bytes) = a {
                if let (w::GroupCallMsg::Join { join, .. }, _) =
                    w::decode_control(&bytes).unwrap().unwrap()
                {
                    return Some(join);
                }
                None
            } else {
                None
            }
        })
        .unwrap();
    let j = join.open().unwrap();
    let roster = Signed::sign(
        &w::Roster {
            gid: start.gid.clone(),
            call_id: start.call_id.clone(),
            host_gen: 1,
            host_did: host.did().into(),
            epoch: 1,
            entries: vec![w::RosterEntry::new(0, &j, &join)],
        },
        &host,
    )
    .unwrap();
    let observed = client
        .on_control(
            w::GroupCallMsg::Roster {
                roster: roster.clone(),
            },
            2,
            &auth,
        )
        .unwrap();
    assert!(
        observed
            .iter()
            .any(|a| matches!(a, ClientAction::Joined { .. }))
    );
    assert_eq!(client.state(), ClientState::Joined);
    (
        local,
        JoinProofs {
            start: signed_start,
            expected_join: join,
            roster,
        },
    )
}
fn ledger(a: &identity::Identity, p: &JoinProofs) -> (std::sync::Arc<store::ReceiptStore>, Ledger) {
    let s = p.start.open().unwrap();
    let db = crate::testkit::memory(9);
    let l = db
        .open(
            Scope {
                conversation: Conversation::Group(s.gid),
                day: "1970-01-01".into(),
            },
            writer_id(a.did(), &[1; 32]),
            |_| Ok(()),
        )
        .unwrap();
    (db, l)
}
#[test]
fn actual_client_join_transition_to_durable_originals_and_receipt_view() {
    let (a, p) = fixture();
    let (_, l) = ledger(&a, &p);
    let c = persist_join(
        &l,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(c.entries.len(), 1);
    let views = joined_views(&l).unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].issuer, a.did());
    assert_eq!(views[0].device, [1; 32]);
    assert_eq!(views[0].call_id, p.start.open().unwrap().call_id);
    for evidence in sidecars(&p) {
        assert_eq!(
            l.evidence(&evidence.id).unwrap().unwrap().bytes,
            evidence.bytes
        );
    }
    assert!(
        persist_join(
            &l,
            &a,
            [1; 32],
            2,
            vec![(7, 1)],
            &p,
            &Authority { allowed: true },
            |_| Ok(())
        )
        .unwrap()
        .update
        .is_none()
    );
    assert_eq!(joined_views(&l).unwrap().len(), 1);
}
#[test]
fn failure_persists_neither_original_evidence_nor_receipt_or_projection() {
    let (a, p) = fixture();
    let (s, l) = ledger(&a, &p);
    s.fail_next_commit();
    assert!(
        persist_join(
            &l,
            &a,
            [1; 32],
            2,
            vec![(7, 1)],
            &p,
            &Authority { allowed: true },
            |_| Ok(())
        )
        .is_err()
    );
    assert!(l.vv().is_empty());
    assert!(joined_views(&l).unwrap().is_empty());
    for evidence in sidecars(&p) {
        assert!(l.evidence(&evidence.id).unwrap().is_none());
    }
    persist_join(
        &l,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(joined_views(&l).unwrap().len(), 1);
}
#[test]
fn crypto_valid_roster_for_another_expected_join_is_not_this_device_confirmation() {
    let (a, mut p) = fixture();
    let (_, l) = ledger(&a, &p);
    let s = p.start.open().unwrap();
    // Another genuinely signed Join of the SAME account. The roster still
    // verifies, but it must not stand for this device's expected outbound Join.
    p.expected_join = w::sign_join(&a, &s.gid, &s.call_id, s.host_gen, 3)
        .unwrap()
        .1;
    assert!(
        persist_join(
            &l,
            &a,
            [1; 32],
            4,
            vec![(7, 1)],
            &p,
            &Authority { allowed: true },
            |_| Ok(())
        )
        .is_err()
    );
    assert!(l.vv().is_empty());
}
#[test]
fn revoked_roles_local_device_gate_and_forged_originals_refuse_before_commit() {
    let (a, mut p) = fixture();
    let (_, l) = ledger(&a, &p);
    assert!(
        persist_join(
            &l,
            &a,
            [1; 32],
            2,
            vec![(7, 1)],
            &p,
            &Authority { allowed: false },
            |_| Ok(())
        )
        .is_err()
    );
    assert!(
        persist_join(
            &l,
            &a,
            [1; 32],
            2,
            vec![(7, 1)],
            &p,
            &Authority { allowed: true },
            |_| Err("local device epoch revoked".into())
        )
        .is_err()
    );
    p.roster = w::SignedRoster::from_parts(p.roster.body().to_vec(), *p.expected_join.sig());
    assert!(
        persist_join(
            &l,
            &a,
            [1; 32],
            2,
            vec![(7, 1)],
            &p,
            &Authority { allowed: true },
            |_| Ok(())
        )
        .is_err()
    );
    assert!(l.vv().is_empty());
}
#[test]
fn dangling_evidence_cannot_produce_a_proof_backed_joined_view() {
    let (a, _) = fixture();
    let claim = super::super::calls::tests::claim(&a, true);
    let s = crate::testkit::memory(9);
    let l = s
        .open(
            claim.scope.clone(),
            writer_id(a.did(), &claim.device),
            |_| Ok(()),
        )
        .unwrap();
    l.write(&claim, &a, |_| Ok(())).unwrap();
    assert!(joined_views(&l).is_err());
}

#[test]
fn strict_original_receipt_and_proofs_cross_two_storage_hops_without_resigning() {
    let (a, p) = fixture();
    let (_, source) = ledger(&a, &p);
    let update = persist_join(
        &source,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| Ok(()),
    )
    .unwrap()
    .update
    .unwrap();
    let proofs = sidecars(&p);
    let (_, laptop) = replica(&p, writer_id(a.did(), &[2; 32]));
    let (_, phone) = replica(&p, writer_id(a.did(), &[3; 32]));
    let received = laptop
        .apply_remote_with_evidence(&update, &proofs, |claims, proofs| {
            assert_eq!(claims.len(), 1);
            assert!(!claims[0].already_stored);
            verify_import(claims, proofs)
        })
        .unwrap();
    let entry = &received.entries[0];
    assert_eq!(source.raw(&entry.id), laptop.raw(&entry.id));
    let forwarded: Vec<_> = proofs
        .iter()
        .map(|e| laptop.evidence(&e.id).unwrap().unwrap())
        .collect();
    phone
        .apply_remote_with_evidence(&received.update.unwrap(), &forwarded, verify_import)
        .unwrap();
    assert_eq!(source.vv(), phone.vv());
    assert_eq!(
        joined_views(&source).unwrap(),
        joined_views(&phone).unwrap()
    );
    for proof in proofs {
        assert_eq!(
            proof.bytes,
            phone.evidence(&proof.id).unwrap().unwrap().bytes
        );
    }
    let before = phone.vv();
    phone
        .apply_remote_with_evidence(&update, &forwarded, |claims, evidence| {
            assert_eq!(claims.len(), 1);
            assert!(claims[0].already_stored);
            verify_import(claims, evidence)
        })
        .unwrap();
    assert_eq!(before, phone.vv());
}
#[test]
fn strict_import_missing_or_duplicate_or_unrelated_proofs_has_no_durable_effect() {
    let (a, p) = fixture();
    let (_, source) = ledger(&a, &p);
    let update = persist_join(
        &source,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| Ok(()),
    )
    .unwrap()
    .update
    .unwrap();
    let proofs = sidecars(&p);
    let (_, target) = ledger(&a, &p);
    assert!(
        target
            .apply_remote_with_evidence(&update, &proofs[..2], verify_import)
            .is_err()
    );
    let mut duplicate = proofs.clone();
    duplicate.push(proofs[0].clone());
    assert!(
        target
            .apply_remote_with_evidence(&update, &duplicate, verify_import)
            .is_err()
    );
    let (_, other) = fixture();
    let mut unrelated = proofs.clone();
    unrelated.push(Evidence::from_blob(&other.roster).unwrap());
    assert!(
        target
            .apply_remote_with_evidence(&update, &unrelated, verify_import)
            .is_err()
    );
    assert!(target.vv().is_empty());
    assert!(target.entries().unwrap().is_empty());
    for e in &proofs {
        assert!(target.evidence(&e.id).unwrap().is_none());
    }
    target
        .apply_remote_with_evidence(&update, &proofs, verify_import)
        .unwrap();
    assert_eq!(joined_views(&target).unwrap().len(), 1);
}
#[test]
fn strict_import_refusal_commit_failure_and_duplicate_peer_gate_never_publish_progress() {
    let (a, p) = fixture();
    let (_, source) = ledger(&a, &p);
    let update = persist_join(
        &source,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| Ok(()),
    )
    .unwrap()
    .update
    .unwrap();
    let proofs = sidecars(&p);
    let (factory, target) = ledger(&a, &p);
    assert!(
        target
            .apply_remote_with_evidence(&update, &proofs, |_, _| Err("removed own peer".into()))
            .is_err()
    );
    factory.fail_next_commit();
    assert!(
        target
            .apply_remote_with_evidence(&update, &proofs, verify_import)
            .is_err()
    );
    assert!(target.vv().is_empty());
    for e in &proofs {
        assert!(target.evidence(&e.id).unwrap().is_none());
    }
    target
        .apply_remote_with_evidence(&update, &proofs, verify_import)
        .unwrap();
    let before = target.vv();
    let called = std::cell::Cell::new(false);
    assert!(
        target
            .apply_remote_with_evidence(&update, &[], |claims, evidence| {
                assert!(claims.is_empty());
                assert!(evidence.is_empty());
                called.set(true);
                Err("peer revoked on replay".into())
            })
            .is_err()
    );
    assert!(called.get());
    assert_eq!(before, target.vv());
    assert_eq!(joined_views(&target).unwrap().len(), 1);
}
#[test]
fn signed_receipt_with_hash_correct_but_forged_protocol_evidence_is_refused() {
    let (a, mut p) = fixture();
    let (_, source) = ledger(&a, &p);
    let first = persist_join(
        &source,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| Ok(()),
    )
    .unwrap();
    let mut claim = first.entries[0].receipt.clone();
    p.roster = w::SignedRoster::from_parts(p.roster.body().to_vec(), *p.expected_join.sig());
    let proofs = sidecars(&p);
    claim.call.as_mut().unwrap().evidence = proofs.iter().map(|e| e.id).collect();
    claim.call.as_mut().unwrap().evidence.sort_unstable();
    let (_, malicious) = ledger(&a, &p);
    let update = malicious
        .write(&claim, &a, |_| Ok(()))
        .unwrap()
        .update
        .unwrap();
    let (_, target) = replica(&p, 8);
    assert!(
        target
            .apply_remote_with_evidence(&update, &proofs, verify_import)
            .is_err()
    );
    assert!(target.vv().is_empty());
    for e in &proofs {
        assert!(target.evidence(&e.id).unwrap().is_none());
    }
}
#[test]
fn strict_vv_only_import_checks_known_originals_and_commits_frontier_atomically() {
    let (a, p) = fixture();
    let (_, source) = ledger(&a, &p);
    let first = persist_join(
        &source,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| Ok(()),
    )
    .unwrap();
    let entry = first.entries[0].clone();
    let update = first.update.unwrap();
    let proofs = sidecars(&p);
    let (factory, target) = replica(&p, 8);
    target
        .apply_remote_with_evidence(&update, &proofs, verify_import)
        .unwrap();
    // Synthetic alternate prefix, as in the existing VV-only ledger fixture.
    // This exercises known-ID/new-range persistence, NOT writer-fork safety.
    let raw = loro::LoroDoc::new();
    raw.set_peer_id(entry.peer).unwrap();
    let mut prefix = entry.receipt.clone();
    prefix.call.as_mut().unwrap().event = CallEventKind::ObservedStart;
    let bytes = prefix.sign_claim(&a).unwrap().to_bytes();
    raw.get_map("receipts")
        .insert(&receipt_id(&bytes), loro::LoroValue::Binary(bytes.into()))
        .unwrap();
    raw.commit();
    let before = raw.oplog_vv();
    raw.get_map("receipts")
        .insert(
            &entry.id,
            loro::LoroValue::Binary(source.raw(&entry.id).unwrap().into()),
        )
        .unwrap();
    raw.commit();
    let extra = raw.export(loro::ExportMode::updates(&before)).unwrap();
    let vv = target.vv();
    factory.fail_next_commit();
    assert!(
        target
            .apply_remote_with_evidence(&extra, &[], verify_import)
            .is_err()
    );
    assert_eq!(vv, target.vv());
    let result = target
        .apply_remote_with_evidence(&extra, &[], |claims, evidence| {
            assert_eq!(claims.len(), 1);
            assert!(claims[0].already_stored);
            verify_import(claims, evidence)
        })
        .unwrap();
    assert!(result.entries.is_empty());
    assert!(result.update.is_some());
    assert_eq!(result.frontier.get(&entry.peer), Some(&2));
    assert_eq!(target.entries().unwrap(), vec![entry]);
}
#[test]
fn new_native_progress_cannot_implicitly_heal_an_older_dangling_claim() {
    let (a, p) = fixture();
    let (_, source) = ledger(&a, &p);
    let first = persist_join(
        &source,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| Ok(()),
    )
    .unwrap();
    let mut another_report = first.entries[0].receipt.clone();
    another_report.call.as_mut().unwrap().observed_at_ms = 3;
    // An additional synthetic signed observation sharing originals. This is
    // not a second real local Joined transition or a retry-identity policy.
    let next = source
        .write(&another_report, &a, |_| Ok(()))
        .unwrap()
        .update
        .unwrap();
    let (_, target) = replica(&p, 8);
    target
        .apply_remote(&first.update.unwrap(), |_| Ok(()))
        .unwrap();
    let before = target.vv();
    let proofs = sidecars(&p);
    assert!(
        target
            .apply_remote_with_evidence(&next, &proofs, verify_import)
            .is_err()
    );
    assert_eq!(before, target.vv());
    assert_eq!(target.entries().unwrap().len(), 1);
    assert!(joined_views(&target).is_err());
    for e in &proofs {
        assert!(target.evidence(&e.id).unwrap().is_none());
    }
}
#[test]
fn proof_aware_export_shrinks_native_prefix_until_all_originals_fit_then_converges() {
    use crdt::batching::BatchLimits;
    let (a, p) = fixture();
    let (_, source) = ledger(&a, &p);
    persist_join(
        &source,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| Ok(()),
    )
    .unwrap();
    for _ in 0..5 {
        let (other, proofs) = fixture();
        let (_, writer) = ledger(&other, &proofs);
        let update = persist_join(
            &writer,
            &other,
            [1; 32],
            2,
            vec![(7, 1)],
            &proofs,
            &Authority { allowed: true },
            |_| Ok(()),
        )
        .unwrap()
        .update
        .unwrap();
        source
            .apply_remote_with_evidence(&update, &sidecars(&proofs), verify_import)
            .unwrap();
    }
    let (_, target) = replica(&p, 8);
    let mut pages = 0;
    while let Some(batch) = source
        .export_with_evidence(
            &target.vv(),
            BatchLimits {
                bytes: 256 * 1024,
                operations: 64,
                writers: 4096,
            },
        )
        .unwrap()
    {
        assert!(batch.evidence.len() <= store::MAX_IMPORT_EVIDENCE);
        if pages == 0 {
            assert!(batch.native.more);
            assert!(batch.native.operations < 6);
        }
        let covered = batch.native.frontier.clone();
        target
            .apply_remote_with_evidence(&batch.native.update, &batch.evidence, verify_import)
            .unwrap();
        assert_eq!(covered, target.vv());
        pages += 1;
        assert!(pages <= 6);
    }
    assert!(pages > 1);
    assert_eq!(source.vv(), target.vv());
    assert_eq!(
        joined_views(&source).unwrap(),
        joined_views(&target).unwrap()
    );
    assert_eq!(target.entries().unwrap().len(), 6);
}
#[test]
fn proof_aware_export_refuses_missing_or_corrupt_original_without_source_progress() {
    use crdt::batching::BatchLimits;
    for corrupt in [false, true] {
        let (a, p) = fixture();
        let backing = Sealed::new(Store::open_in_memory().unwrap(), [9; 32]);
        let factory = store::ReceiptStore::new(backing.clone());
        let source = factory
            .open(
                Scope {
                    conversation: Conversation::Group(p.start.open().unwrap().gid),
                    day: "1970-01-01".into(),
                },
                writer_id(a.did(), &[1; 32]),
                |_| Ok(()),
            )
            .unwrap();
        persist_join(
            &source,
            &a,
            [1; 32],
            2,
            vec![(7, 1)],
            &p,
            &Authority { allowed: true },
            |_| Ok(()),
        )
        .unwrap();
        let before = source.vv();
        let suffix = format!(
            "evidence/{}",
            hex::encode(Evidence::from_blob(&p.roster).unwrap().id)
        );
        let path = backing
            .scan("receipts/")
            .unwrap()
            .into_iter()
            .find(|(k, _)| k.ends_with(&suffix))
            .unwrap()
            .0;
        if corrupt {
            backing.put(&path, b"bad").unwrap();
        } else {
            backing.delete(&path).unwrap();
        }
        assert!(matches!(
            source.export_with_evidence(
                &loro::VersionVector::default(),
                BatchLimits {
                    bytes: 256 * 1024,
                    operations: 64,
                    writers: 4096
                }
            ),
            Err(store::bundles::EvidenceExportError::Receipt(_))
        ));
        assert_eq!(before, source.vv());
    }
}
#[test]
fn proof_aware_export_defers_indivisible_native_oversize_without_returning_a_frontier() {
    use crdt::batching::{BatchError, BatchLimits};
    let (a, p) = fixture();
    let (_, source) = ledger(&a, &p);
    assert!(
        source
            .export_with_evidence(
                &loro::VersionVector::default(),
                BatchLimits {
                    bytes: 1,
                    operations: 1,
                    writers: 4096
                }
            )
            .unwrap()
            .is_none()
    );
    persist_join(
        &source,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| Ok(()),
    )
    .unwrap();
    let before = source.vv();
    assert!(matches!(
        source.export_with_evidence(
            &loro::VersionVector::default(),
            BatchLimits {
                bytes: 1,
                operations: 1,
                writers: 4096
            }
        ),
        Err(store::bundles::EvidenceExportError::Native(
            BatchError::Oversized { .. }
        ))
    ));
    assert_eq!(before, source.vv());
}
#[test]
fn strict_import_rejects_known_dangling_claim_without_implicit_upgrade() {
    let (a, p) = fixture();
    let (_, source) = ledger(&a, &p);
    let update = persist_join(
        &source,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| Ok(()),
    )
    .unwrap()
    .update
    .unwrap();
    let proofs = sidecars(&p);
    let (_, target) = ledger(&a, &p);
    target.apply_remote(&update, |_| Ok(())).unwrap();
    let before = target.vv();
    assert!(
        target
            .apply_remote_with_evidence(&update, &proofs, verify_import)
            .is_err()
    );
    assert_eq!(before, target.vv());
    assert!(joined_views(&target).is_err());
    for e in &proofs {
        assert!(target.evidence(&e.id).unwrap().is_none());
    }
}
#[test]
fn identical_replay_still_checks_current_local_device_gate() {
    let (a, p) = fixture();
    let (_, l) = ledger(&a, &p);
    persist_join(
        &l,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| Ok(()),
    )
    .unwrap();
    let before = l.vv();
    let checked = std::cell::Cell::new(false);
    let result = persist_join(
        &l,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| {
            checked.set(true);
            Err("local device no longer admitted".into())
        },
    );
    assert!(result.is_err());
    assert!(checked.get());
    assert_eq!(before, l.vv());
    // Historical accepted observation remains; refusal does not erase it.
    assert_eq!(joined_views(&l).unwrap().len(), 1);
}

#[test]
fn clean_sealed_redb_reopen_restores_exact_originals_and_joined_projection() {
    let directory = crate::testkit::TempDir::new("tinline-join-evidence");
    let path = directory.0.join("receipts.redb");
    let (a, p) = fixture();
    let scope = Scope {
        conversation: Conversation::Group(p.start.open().unwrap().gid),
        day: "1970-01-01".into(),
    };
    let peer = writer_id(a.did(), &[1; 32]);
    let backing = Sealed::new(Store::open(&path).unwrap(), [9; 32]);
    let factory = store::ReceiptStore::new(backing.clone());
    let l = factory.open(scope.clone(), peer, |_| Ok(())).unwrap();
    let committed = persist_join(
        &l,
        &a,
        [1; 32],
        2,
        vec![(7, 1)],
        &p,
        &Authority { allowed: true },
        |_| Ok(()),
    )
    .unwrap();
    let raw = l.raw(&committed.entries[0].id).unwrap();
    let views = joined_views(&l).unwrap();
    let vv = l.vv();
    drop(l);
    drop(factory);
    backing.close();
    drop(backing);
    let factory = store::ReceiptStore::new(Sealed::new(Store::open(&path).unwrap(), [9; 32]));
    let l = factory.open(scope, peer, |_| Ok(())).unwrap();
    assert_eq!(vv, l.vv());
    assert_eq!(views, joined_views(&l).unwrap());
    assert_eq!(raw, l.raw(&committed.entries[0].id).unwrap());
    for e in sidecars(&p) {
        assert_eq!(e.bytes, l.evidence(&e.id).unwrap().unwrap().bytes);
    }
    assert!(
        persist_join(
            &l,
            &a,
            [1; 32],
            2,
            vec![(7, 1)],
            &p,
            &Authority { allowed: true },
            |_| Ok(())
        )
        .unwrap()
        .update
        .is_none()
    );
    drop(l);
    drop(factory);
}

#[test]
fn missing_or_corrupt_originals_fail_closed_after_reopen_without_silent_repair() {
    for corrupt in [false, true] {
        let (a, p) = fixture();
        let backing = Sealed::new(Store::open_in_memory().unwrap(), [9; 32]);
        let factory = store::ReceiptStore::new(backing.clone());
        let scope = Scope {
            conversation: Conversation::Group(p.start.open().unwrap().gid),
            day: "1970-01-01".into(),
        };
        let peer = writer_id(a.did(), &[1; 32]);
        let l = factory.open(scope.clone(), peer, |_| Ok(())).unwrap();
        persist_join(
            &l,
            &a,
            [1; 32],
            2,
            vec![(7, 1)],
            &p,
            &Authority { allowed: true },
            |_| Ok(()),
        )
        .unwrap();
        let before = l.vv();
        drop(l);
        let id = Evidence::from_blob(&p.roster).unwrap().id;
        let suffix = format!("evidence/{}", hex::encode(id));
        let path = backing
            .scan("receipts/")
            .unwrap()
            .into_iter()
            .find(|(key, _)| key.ends_with(&suffix))
            .unwrap()
            .0;
        if corrupt {
            backing.put(&path, b"corrupt original").unwrap();
        } else {
            backing.delete(&path).unwrap();
        }
        let l = factory.open(scope, peer, |_| Ok(())).unwrap();
        // Low-level native reconstruction remains available, but neither its
        // existence nor a duplicate issuance may promote dangling originals.
        assert!(joined_views(&l).is_err());
        assert!(
            persist_join(
                &l,
                &a,
                [1; 32],
                2,
                vec![(7, 1)],
                &p,
                &Authority { allowed: true },
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(before, l.vv());
        assert_eq!(
            backing.get(&path).unwrap(),
            if corrupt {
                Some(b"corrupt original".to_vec())
            } else {
                None
            }
        );
    }
}
