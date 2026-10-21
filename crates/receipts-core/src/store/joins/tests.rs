use super::*;
use crate::participation::{joined_views, tests::fixture};
use storage::{Sealed, Store};

struct Authority(bool);
impl CallAuthority for Authority {
    fn is_member(&self, _: &str, _: &str) -> bool {
        self.0
    }
    fn host_permit_ok(&self, _: &str, _: &w::HostRef) -> bool {
        self.0
    }
    fn endpoint_ok(&self, _: &str, _: &str, _: Option<&w::SignedNote>, _: &[u8; 32]) -> bool {
        self.0
    }
}
fn flipped<T: proto::Signable>(s: &proto::Signed<T>) -> proto::Signed<T> {
    let mut sig = *s.sig();
    sig[0] ^= 1;
    proto::Signed::from_parts(s.body().to_vec(), sig)
}
fn originals(p: &JoinProofs) -> [Evidence; 3] {
    [
        Evidence::from_blob(&p.start).unwrap(),
        Evidence::from_blob(&p.expected_join).unwrap(),
        Evidence::from_blob(&p.roster).unwrap(),
    ]
}
fn setup(
    a: &identity::Identity,
    p: &JoinProofs,
) -> (Arc<ReceiptStore>, Arc<JoinController>, Ledger) {
    let store = crate::testkit::memory(8);
    let controller = store.join_controller();
    let ledger = open(&store, a, p);
    (store, controller, ledger)
}
fn open(store: &Arc<ReceiptStore>, a: &identity::Identity, p: &JoinProofs) -> Ledger {
    store
        .open(
            Scope {
                conversation: Conversation::Group(p.start.open().unwrap().gid),
                day: "1970-01-01".into(),
            },
            writer_id(a.did(), &[1; 32]),
            |_| Ok(()),
        )
        .unwrap()
}
fn capture(c: &JoinController, a: &identity::Identity, p: &JoinProofs, at: u64) -> String {
    match c
        .capture(
            a,
            [1; 32],
            7,
            at,
            vec![(9, 1)],
            p,
            &Authority(true),
            |context, claim| {
                assert_eq!(context.source_epoch, 7);
                assert_eq!(context.issuer, a.did());
                assert_eq!(context.device, claim.device);
                Ok(())
            },
        )
        .unwrap()
    {
        CapturedJoin::Pending(id) => id,
        CapturedJoin::AlreadyCommitted(_) => panic!("expected pending"),
    }
}
fn commit(c: &JoinController, id: &str, l: &Ledger, a: &identity::Identity) -> JoinCommit {
    c.try_commit(id, l, a, &Authority(true), |context, _| {
        if context.source_epoch != 7 || context.issuer != a.did() || context.device != [1; 32] {
            return Err("stale source".into());
        }
        Ok(())
    })
    .unwrap()
}

#[test]
fn concurrent_callbacks_share_one_frozen_intent() {
    let (a, p) = fixture();
    let (store, c, l) = setup(&a, &p);
    let ids = std::thread::scope(|scope| {
        let mut jobs = Vec::new();
        for at in 2..6 {
            let (a, p, c) = (&a, &p, &c);
            jobs.push(scope.spawn(move || capture(c, a, p, at)));
        }
        jobs.into_iter()
            .map(|j| j.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(ids.iter().all(|id| id == &ids[0]));
    assert_eq!(
        store
            .db
            .keys_after(PENDING, None, MAX_PENDING_JOINS + 1)
            .unwrap()
            .len(),
        1
    );
    let result = commit(&c, &ids[0], &l, &a);
    assert!(result.newly_committed);
    assert!((2..6).contains(&result.view.observed_join_at_ms));
    assert_eq!(l.entries().unwrap().len(), 1);
}

#[test]
fn factory_shares_controller_and_keeps_one_ledger_per_scope() {
    let (a, p) = fixture();
    let (store, controller, _) = setup(&a, &p);
    assert!(Arc::ptr_eq(&controller, &store.join_controller()));
    // The placeholder binding above drops its ledger, so open one explicitly.
    let _ledger = open(&store, &a, &p);
    assert!(
        store
            .open(
                Scope {
                    conversation: Conversation::Group(p.start.open().unwrap().gid),
                    day: "1970-01-01".into()
                },
                writer_id(a.did(), &[1; 32]),
                |_| Ok(())
            )
            .is_err()
    );
}

#[test]
fn real_client_join_capture_freezes_first_observation_and_commits_before_view() {
    // fixture drives real ClientEngine::on_control to its Joined action; still
    // not a live authenticated Node callback or UI test.
    let (a, p) = fixture();
    let (store, c, l) = setup(&a, &p);
    let id = capture(&c, &a, &p, 2);
    let key = format!("{PENDING}{id}");
    let frozen = store.db.get(&key).unwrap().unwrap();
    assert!(l.entries().unwrap().is_empty());
    assert!(joined_views(&l).unwrap().is_empty());
    assert_eq!(capture(&c, &a, &p, 86_400_002), id);
    assert_eq!(store.db.get(&key).unwrap().unwrap(), frozen);
    let intent: Intent = decode(&frozen, MAX_JOIN_INTENT_BYTES).unwrap();
    let result = commit(&c, &id, &l, &a);
    assert!(result.newly_committed);
    assert_eq!(result.view.observed_join_at_ms, 2);
    assert_eq!(l.raw(&result.view.receipt_id).unwrap(), intent.signed);
    assert!(store.db.get(&key).unwrap().is_none());
    assert!(store.db.get(&format!("{DONE}{id}")).unwrap().is_some());
    assert_eq!(joined_views(&l).unwrap(), vec![result.view.clone()]);
    let vv = l.vv();
    assert!(!commit(&c, &id, &l, &a).newly_committed);
    assert_eq!(l.vv(), vv);
    assert_eq!(
        c.capture(
            &a,
            [1; 32],
            7,
            86_400_002,
            vec![(9, 2)],
            &p,
            &Authority(true),
            |_, _| Ok(())
        )
        .unwrap(),
        CapturedJoin::AlreadyCommitted(id)
    );
    assert_eq!(joined_views(&l).unwrap(), vec![result.view]);
}

#[test]
fn forged_protocol_originals_cannot_enter_outbox() {
    let (a, p) = fixture();
    let (_, c, l) = setup(&a, &p);
    for index in 0..3 {
        let mut forged = JoinProofs {
            start: p.start.clone(),
            expected_join: p.expected_join.clone(),
            roster: p.roster.clone(),
        };
        match index {
            0 => forged.start = flipped(&forged.start),
            1 => forged.expected_join = flipped(&forged.expected_join),
            _ => forged.roster = flipped(&forged.roster),
        }
        assert!(
            c.capture(
                &a,
                [1; 32],
                7,
                2,
                vec![(9, 1)],
                &forged,
                &Authority(true),
                |_, _| Ok(())
            )
            .is_err()
        );
    }
    assert!(c.next_pending().unwrap().is_none());
    assert!(l.entries().unwrap().is_empty());
}

#[test]
fn failed_capture_is_not_reported_durable_and_can_retry() {
    let (a, p) = fixture();
    let (store, c, l) = setup(&a, &p);
    store.fail_next_commit();
    assert!(
        c.capture(
            &a,
            [1; 32],
            7,
            2,
            vec![(9, 1)],
            &p,
            &Authority(true),
            |_, _| Ok(())
        )
        .is_err()
    );
    assert!(c.next_pending().unwrap().is_none());
    assert!(l.entries().unwrap().is_empty());
    let id = capture(&c, &a, &p, 2);
    assert_eq!(c.next_pending().unwrap(), Some(id));
}

#[test]
fn failed_completion_leaves_intent_and_no_evidence_frontier_or_done_marker() {
    let (a, p) = fixture();
    let (store, c, l) = setup(&a, &p);
    let id = capture(&c, &a, &p, 2);
    let before = store.db.get(&format!("{PENDING}{id}")).unwrap();
    let vv = l.vv();
    store.fail_next_commit();
    assert!(
        c.try_commit(&id, &l, &a, &Authority(true), |_, _| Ok(()))
            .is_err()
    );
    assert_eq!(store.db.get(&format!("{PENDING}{id}")).unwrap(), before);
    assert!(store.db.get(&format!("{DONE}{id}")).unwrap().is_none());
    assert!(l.entries().unwrap().is_empty());
    assert_eq!(l.vv(), vv);
    for evidence in originals(&p) {
        assert!(l.evidence(&evidence.id).unwrap().is_none());
    }
    assert!(commit(&c, &id, &l, &a).newly_committed);
    assert_eq!(l.entries().unwrap().len(), 1);
}

#[test]
fn admission_runs_for_capture_pending_retry_and_completed_replay() {
    let (a, p) = fixture();
    let (_, c, l) = setup(&a, &p);
    assert!(
        c.capture(
            &a,
            [1; 32],
            7,
            2,
            vec![(9, 1)],
            &p,
            &Authority(true),
            |_, _| Err("not actual Joined".into())
        )
        .is_err()
    );
    assert!(c.next_pending().unwrap().is_none());
    let id = capture(&c, &a, &p, 2);
    assert!(
        c.try_commit(&id, &l, &a, &Authority(true), |_, _| Err(
            "device/source changed".into()
        ))
        .is_err()
    );
    assert!(l.entries().unwrap().is_empty());
    commit(&c, &id, &l, &a);
    assert!(
        c.try_commit(&id, &l, &a, &Authority(true), |_, _| Err("stopped".into()))
            .is_err()
    );
    assert!(
        c.capture(
            &a,
            [1; 32],
            7,
            3,
            vec![(9, 1)],
            &p,
            &Authority(true),
            |_, _| Err("stopped".into())
        )
        .is_err()
    );
    assert_eq!(joined_views(&l).unwrap().len(), 1);
}

#[test]
fn old_epoch_is_not_rebound_and_authority_changes_defer_without_erasure() {
    let (a, p) = fixture();
    let (store, c, l) = setup(&a, &p);
    let id = capture(&c, &a, &p, 2);
    assert!(
        c.capture(
            &a,
            [1; 32],
            8,
            3,
            vec![(9, 1)],
            &p,
            &Authority(true),
            |_, _| Ok(())
        )
        .is_err()
    );
    assert!(
        c.try_commit(&id, &l, &a, &Authority(false), |_, _| Ok(()))
            .is_err()
    );
    assert!(
        c.try_commit(&id, &l, &a, &Authority(true), |ctx, _| {
            if ctx.source_epoch == 8 {
                Ok(())
            } else {
                Err("stale epoch".into())
            }
        })
        .is_err()
    );
    assert!(store.db.get(&format!("{PENDING}{id}")).unwrap().is_some());
    commit(&c, &id, &l, &a);
    assert!(
        c.try_commit(&id, &l, &a, &Authority(false), |_, _| Ok(()))
            .is_err()
    );
    assert_eq!(joined_views(&l).unwrap().len(), 1);
}

#[test]
fn wrong_account_backing_scope_or_local_device_does_not_complete_intent() {
    let (a, p) = fixture();
    let (store, c, l) = setup(&a, &p);
    let id = capture(&c, &a, &p, 2);
    let other = identity::generate().0;
    assert!(
        c.try_commit(&id, &l, &other, &Authority(true), |_, _| Ok(()))
            .is_err()
    );
    let (_, _, alien) = setup(&a, &p);
    assert!(
        c.try_commit(&id, &alien, &a, &Authority(true), |_, _| Ok(()))
            .is_err()
    );
    let wrong = store
        .open(
            Scope {
                conversation: Conversation::Group("b".repeat(64)),
                day: "1970-01-01".into(),
            },
            writer_id(a.did(), &[1; 32]),
            |_| Ok(()),
        )
        .unwrap();
    assert!(
        c.try_commit(&id, &wrong, &a, &Authority(true), |_, _| Ok(()))
            .is_err()
    );
    assert!(
        c.try_commit(&id, &l, &a, &Authority(true), |ctx, _| {
            if ctx.device == [2; 32] {
                Ok(())
            } else {
                Err("wrong device".into())
            }
        })
        .is_err()
    );
    assert!(l.entries().unwrap().is_empty());
    assert!(store.db.get(&format!("{PENDING}{id}")).unwrap().is_some());
}

#[test]
fn queue_is_bounded_rotates_failed_intents_and_never_evicts() {
    let (a, p) = fixture();
    let (store, c, _) = setup(&a, &p);
    let mut ids = std::collections::BTreeSet::new();
    ids.insert(capture(&c, &a, &p, 2));
    for _ in 1..MAX_PENDING_JOINS {
        let (a, p) = fixture();
        ids.insert(capture(&c, &a, &p, 2));
    }
    assert_eq!(ids.len(), MAX_PENDING_JOINS);
    let (extra_account, extra) = fixture();
    assert!(
        c.capture(
            &extra_account,
            [1; 32],
            7,
            2,
            vec![(9, 1)],
            &extra,
            &Authority(true),
            |_, _| Ok(())
        )
        .is_err()
    );
    assert_eq!(
        store
            .db
            .keys_after(PENDING, None, MAX_PENDING_JOINS + 1)
            .unwrap()
            .len(),
        MAX_PENDING_JOINS
    );
    let observed: std::collections::BTreeSet<_> = (0..MAX_PENDING_JOINS)
        .map(|_| c.next_pending().unwrap().unwrap())
        .collect();
    assert_eq!(ids, observed);
    assert!(ids.contains(&c.next_pending().unwrap().unwrap()));
    assert!(ids.contains(&capture(&c, &a, &p, 3))); // duplicates still work at capacity
}

#[test]
fn clean_reopen_keeps_exact_pending_and_completed_body_without_auto_epoch_upgrade() {
    let (a, p) = fixture();
    let root = std::env::temp_dir().join(format!(
        "receipt-join-controller-{}-{}",
        std::process::id(),
        w::new_call_id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("joined.redb");
    let db = Sealed::new(Store::open(&path).unwrap(), [8; 32]);
    let store = ReceiptStore::new(db.clone());
    let c = store.join_controller();
    let id = capture(&c, &a, &p, 2);
    let frozen = db.get(&format!("{PENDING}{id}")).unwrap().unwrap();
    drop(c);
    drop(store);
    db.close();
    drop(db);
    let db = Sealed::new(Store::open(&path).unwrap(), [8; 32]);
    let store = ReceiptStore::new(db.clone());
    let c = store.join_controller();
    let l = open(&store, &a, &p);
    assert_eq!(c.next_pending().unwrap(), Some(id.clone()));
    assert_eq!(db.get(&format!("{PENDING}{id}")).unwrap().unwrap(), frozen);
    assert!(
        c.try_commit(&id, &l, &a, &Authority(true), |ctx, _| {
            if ctx.source_epoch == 8 {
                Ok(())
            } else {
                Err("explicit recovery required".into())
            }
        })
        .is_err()
    );
    let view = commit(&c, &id, &l, &a).view;
    let original = l.raw(&view.receipt_id).unwrap();
    drop(l);
    drop(c);
    drop(store);
    db.close();
    drop(db);
    let db = Sealed::new(Store::open(&path).unwrap(), [8; 32]);
    let store = ReceiptStore::new(db.clone());
    let c = store.join_controller();
    let l = open(&store, &a, &p);
    assert!(c.next_pending().unwrap().is_none());
    let replay = commit(&c, &id, &l, &a);
    assert!(!replay.newly_committed);
    assert_eq!(replay.view, view);
    assert_eq!(l.raw(&view.receipt_id).unwrap(), original);
    drop(l);
    drop(c);
    drop(store);
    db.close();
    drop(db);
    // This is a clean reopen, not a process-kill/power-loss experiment.
}

#[test]
fn malformed_pending_or_completed_evidence_cannot_publish_or_implicitly_repair() {
    let (a, p) = fixture();
    let (store, c, l) = setup(&a, &p);
    let id = capture(&c, &a, &p, 2);
    let key = format!("{PENDING}{id}");
    let bytes = store.db.get(&key).unwrap().unwrap();
    let mut bad: Intent = decode(&bytes, MAX_JOIN_INTENT_BYTES).unwrap();
    bad.originals[0][0] ^= 1;
    store
        .db
        .put(&key, &encode(&bad, MAX_JOIN_INTENT_BYTES).unwrap())
        .unwrap();
    assert!(
        c.try_commit(&id, &l, &a, &Authority(true), |_, _| Ok(()))
            .is_err()
    );
    assert!(l.entries().unwrap().is_empty());
    let mut trailing = bytes.clone();
    trailing.push(0);
    store.db.put(&key, &trailing).unwrap();
    assert!(
        c.try_commit(&id, &l, &a, &Authority(true), |_, _| Ok(()))
            .is_err()
    );
    store
        .db
        .put(&key, &vec![0; MAX_JOIN_INTENT_BYTES + 1])
        .unwrap();
    assert!(
        c.try_commit(&id, &l, &a, &Authority(true), |_, _| Ok(()))
            .is_err()
    );
    store.db.put(&key, &bytes).unwrap();
    commit(&c, &id, &l, &a);
    let e = Evidence::from_blob(&p.start).unwrap();
    let evidence_key = format!("{}evidence/{}", l.prefix, hex::encode(e.id));
    store
        .db
        .put(&evidence_key, b"corrupt synthetic original")
        .unwrap();
    assert!(
        c.try_commit(&id, &l, &a, &Authority(true), |_, _| Ok(()))
            .is_err()
    );
    assert!(joined_views(&l).is_err());
    assert_eq!(
        store.db.get(&evidence_key).unwrap().unwrap(),
        b"corrupt synthetic original"
    );
}

#[test]
fn existing_receipt_completion_sidecars_are_atomic_without_advancing_native_frontier() {
    let (a, p) = fixture();
    let (store, c, l) = setup(&a, &p);
    let id = capture(&c, &a, &p, 2);
    let claim = prepare_join(&a, [1; 32], 2, vec![(9, 1)], &p, &Authority(true))
        .unwrap()
        .0;
    let originals = originals(&p);
    l.write_with_evidence(&claim, &a, &originals, |_| Ok(()))
        .unwrap();
    let vv = l.vv();
    store.fail_next_commit();
    assert!(
        c.try_commit(&id, &l, &a, &Authority(true), |_, _| Ok(()))
            .is_err()
    );
    assert!(store.db.get(&format!("{PENDING}{id}")).unwrap().is_some());
    assert!(!commit(&c, &id, &l, &a).newly_committed);
    assert_eq!(l.vv(), vv);
    assert_eq!(l.entries().unwrap().len(), 1);
}

#[test]
fn low_level_dangling_receipt_cannot_be_healed_by_pending_completion() {
    let (a, p) = fixture();
    let (store, c, l) = setup(&a, &p);
    let id = capture(&c, &a, &p, 2);
    let claim = prepare_join(&a, [1; 32], 2, vec![(9, 1)], &p, &Authority(true))
        .unwrap()
        .0;
    l.write(&claim, &a, |_| Ok(())).unwrap(); // synthetic legacy/dangling path
    let vv = l.vv();
    assert!(
        c.try_commit(&id, &l, &a, &Authority(true), |_, _| Ok(()))
            .is_err()
    );
    assert_eq!(l.vv(), vv);
    assert!(store.db.get(&format!("{PENDING}{id}")).unwrap().is_some());
    assert!(store.db.get(&format!("{DONE}{id}")).unwrap().is_none());
    assert!(
        l.evidence(&Evidence::from_blob(&p.start).unwrap().id)
            .unwrap()
            .is_none()
    );
    assert!(joined_views(&l).is_err());
}

#[test]
fn newer_roster_is_same_transition_but_new_expected_join_is_a_separate_observation() {
    let (a, template) = fixture();
    let host = identity::generate().0;
    let mut s = template.start.open().unwrap();
    s.host_ref.holder_did = host.did().into();
    s.starter_did = host.did().into();
    let start = proto::Signed::sign(&s, &host).unwrap();
    let make = |issued, epoch| {
        let (j, blob) = w::sign_join(&a, &s.gid, &s.call_id, s.host_gen, issued).unwrap();
        JoinProofs {
            start: start.clone(),
            expected_join: blob.clone(),
            roster: proto::Signed::sign(
                &w::Roster {
                    gid: s.gid.clone(),
                    call_id: s.call_id.clone(),
                    host_gen: s.host_gen,
                    host_did: host.did().into(),
                    epoch,
                    entries: vec![w::RosterEntry::new(0, &j, &blob)],
                },
                &host,
            )
            .unwrap(),
        }
    };
    let p = make(1, 1);
    let (store, c, l) = setup(&a, &p);
    let first = capture(&c, &a, &p, 2);
    let frozen = store.db.get(&format!("{PENDING}{first}")).unwrap();
    let mut roster = p.roster.open().unwrap();
    roster.epoch = 2;
    let updated = JoinProofs {
        start: start.clone(),
        expected_join: p.expected_join.clone(),
        roster: proto::Signed::sign(&roster, &host).unwrap(),
    };
    assert_eq!(capture(&c, &a, &updated, 3), first);
    assert_eq!(store.db.get(&format!("{PENDING}{first}")).unwrap(), frozen);
    let rejoin = make(4, 3);
    let second = capture(&c, &a, &rejoin, 5);
    assert_ne!(first, second);
    assert_eq!(commit(&c, &first, &l, &a).view.observed_join_at_ms, 2);
    assert_eq!(commit(&c, &second, &l, &a).view.observed_join_at_ms, 5);
    let views = joined_views(&l).unwrap();
    assert_eq!(views.len(), 2);
    assert!(views.iter().all(|v| v.call_id == s.call_id));
}
