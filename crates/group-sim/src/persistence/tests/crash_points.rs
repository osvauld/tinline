use super::*;

// KIND: invariant
// I7 + C2 §4 I3/I6 analogue: every crash/failure point, then restart + retry from the sender's
// persisted original + reconciliation, ends equal to a fault-free control with one commit.
#[test]
fn every_crash_or_failure_point_converges_to_fault_free_control_with_one_commit() {
    let fx = fixture();
    let control = control(&fx);
    let faults = all_faults();
    assert_eq!(faults.len(), 15);
    for fault in faults {
        let mut r = run(&fx, fault, AckOrder::CommitThenAck);
        expect_injected(&r, fault);
        recover(&mut r);
        assert_converged(&r, &control, &fx, fault);
    }
}

// KIND: invariant
// S01: crash or failure before the durability boundary leaves Durable byte-identical, sends no
// StoredAck/push, and recovery exposes exactly the populated pre-update replica.
#[test]
fn s01_crash_or_failure_before_commit_sends_no_ack_and_recovers_no_partial_state() {
    let fx = fixture();
    let control = control(&fx);
    let id = fx.c1.id();
    for fault in [
        Fault::CrashBefore(Step::Validate),
        Fault::CrashAfter(Step::Validate),
        Fault::CrashBefore(Step::ScratchImport),
        Fault::CrashAfter(Step::ScratchImport),
        Fault::CrashBefore(Step::Commit),
        Fault::Fail(Step::Validate),
        Fault::Fail(Step::ScratchImport),
        Fault::Fail(Step::Commit),
    ] {
        let mut r = run(&fx, fault, AckOrder::CommitThenAck);
        expect_injected(&r, fault);
        assert_eq!(
            r.host.durable, r.durable_before,
            "{fault:?}: durable changed"
        );
        assert_eq!(r.view_before.accepted.len(), 2, "populated baseline");
        assert!(!r.net.acked(&id) && r.net.pushes.is_empty(), "{fault:?}");
        restart_if_down(&mut r.host);
        assert_eq!(
            view(r.host.replica()),
            r.view_before,
            "{fault:?}: partial state"
        );
        assert_eq!(r.host.durable, r.durable_before, "{fault:?}: open wrote");
        assert_eq!(r.host.availability(&id), Availability::Unknown);
        // Sender still holds the original because no ack arrived; the retry commits it.
        r.sender.absorb(&r.net);
        assert!(r.sender.unacked.contains_key(&id));
        recover(&mut r);
        assert_converged(&r, &control, &fx, fault);
    }
}

// KIND: invariant
// S02: once committed, a crash/failure before StoredAck or fan-out keeps update/proof/index;
// the sender's retry is a durable-noop re-ack, and reconciliation (not the lost push) repairs
// the subscriber without a duplicate event.
#[test]
fn s02_commit_then_crash_or_failure_before_ack_or_fanout_retains_and_retries_idempotently() {
    let fx = fixture();
    let control = control(&fx);
    let id = fx.c1.id();
    for fault in [
        Fault::CrashAfter(Step::Commit),
        Fault::CrashBefore(Step::StoredAck),
        Fault::Fail(Step::StoredAck),
        Fault::CrashAfter(Step::StoredAck),
        Fault::CrashBefore(Step::FanOut),
        Fault::Fail(Step::FanOut),
    ] {
        let mut r = run(&fx, fault, AckOrder::CommitThenAck);
        expect_injected(&r, fault);
        assert_eq!(
            canonical(&r.host.durable),
            canonical(&control.durable),
            "{fault:?}"
        );
        assert_eq!(r.host.durable.writes, r.durable_before.writes + 1);
        assert_eq!(r.host.durable.index[&fx.c1.key()], id);
        assert!(r.net.pushes.is_empty(), "{fault:?}: push escaped");
        restart_if_down(&mut r.host);
        let after_open = r.host.durable.clone();
        assert!(r.host.replica().accepted.contains_key(&id), "{fault:?}");
        r.sender.absorb(&r.net);
        if r.sender.unacked.contains_key(&id) {
            assert_eq!(
                r.sender.retry(&mut r.host, &mut r.net),
                vec![Ingest::Duplicate],
                "{fault:?}"
            );
            assert_eq!(r.host.durable, after_open, "{fault:?}: retry rewrote");
            assert!(r.net.acked(&id) && r.net.pushes.is_empty());
        }
        assert!(!r.sub.accepted.contains_key(&id), "{fault:?}");
        assert_eq!(reconcile(&r.host, &mut r.sub), vec![Outcome::Accepted]);
        // Redundant repair and a late duplicate push change nothing.
        assert!(reconcile(&r.host, &mut r.sub).is_empty());
        assert_eq!(r.sub.receive(fx.c1.clone()), Outcome::Duplicate);
        assert_converged(&r, &control, &fx, fault);
    }
}

// KIND: invariant
// S03: a StoredAck followed by an immediate forced restart: the acknowledged original, index
// entry and recovered replica state survive; the sender's reliance on the ack is truthful.
#[test]
fn s03_stored_ack_then_immediate_forced_restart_keeps_acknowledged_original() {
    let fx = fixture();
    let control = control(&fx);
    let id = fx.c1.id();
    for fault in [
        Fault::CrashAfter(Step::StoredAck),
        Fault::CrashBefore(Step::FanOut),
        Fault::CrashAfter(Step::FanOut),
    ] {
        let mut r = run(&fx, fault, AckOrder::CommitThenAck);
        expect_injected(&r, fault);
        assert_eq!(r.net.acks.last(), Some(&(id.clone(), true)), "{fault:?}");
        r.sender.absorb(&r.net);
        assert!(
            r.sender.unacked.is_empty(),
            "sender dropped its copy on ack"
        );
        r.host.open(OpenPolicy::FailClosed).expect("opens");
        assert_eq!(r.host.durable.originals[&id], encode(&fx.c1));
        assert_eq!(r.host.durable.index[&fx.c1.key()], id);
        assert_eq!(r.host.availability(&id), Availability::Stored);
        recover(&mut r);
        assert_converged(&r, &control, &fx, fault);
    }
}

// KIND: limitation
// Counterexample ordering (C2 §3c AckBeforeCommit): StoredAck then crash before Commit. The
// sender drops its copy on the ack; the host restarts without the update; nothing resends it.
#[test]
fn s03_ack_before_commit_ordering_acknowledges_an_update_that_restart_loses() {
    let fx = fixture();
    let control = control(&fx);
    let id = fx.c1.id();
    // Positive control: the same ordering without a crash converges, so loss is crash-caused.
    // (Even then the ack left before the commit: the oracle records it as unbacked.)
    let mut ok = run(&fx, Fault::None, AckOrder::AckBeforeCommit);
    recover(&mut ok);
    assert_eq!(canonical(&ok.host.durable), canonical(&control.durable));
    assert!(ok.sub.accepted.contains_key(&id) && ok.sender.unacked.is_empty());
    assert_eq!(ok.net.acks.last(), Some(&(id.clone(), false)));

    let fault = Fault::CrashAfter(Step::StoredAck);
    let mut r = run(&fx, fault, AckOrder::AckBeforeCommit);
    expect_injected(&r, fault);
    assert_eq!(r.net.acks.last(), Some(&(id.clone(), false)));
    assert_eq!(r.host.durable, r.durable_before);
    recover(&mut r);
    assert!(r.sender.unacked.is_empty());
    assert_eq!(r.host.availability(&id), Availability::Unknown);
    assert!(!r.sub.accepted.contains_key(&id));
    assert_ne!(canonical(&r.host.durable), canonical(&control.durable));
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Corruption {
    ProofSignature,
    RecordSwapped,
    Undecodable,
    SnapshotBytes,
    StaleSnapshot,
    IndexPointsElsewhere,
}
pub(super) const CORRUPTIONS: [Corruption; 6] = [
    Corruption::ProofSignature,
    Corruption::RecordSwapped,
    Corruption::Undecodable,
    Corruption::SnapshotBytes,
    Corruption::StaleSnapshot,
    Corruption::IndexPointsElsewhere,
];
// Host holds a1, a2 (previous = a1) and b1 (independent); then crashes; then disk corruption.
pub(super) struct S04 {
    pub(super) g: Genesis,
    pub(super) a1: Envelope,
    pub(super) a2: Envelope,
    pub(super) b1: Envelope,
}
pub(super) fn s04_fixture() -> S04 {
    let (g, mut a, mut b, _c) = signed_fixture();
    let a1 = a.post("a1");
    let a2 = a.post("a2");
    let b1 = b.post("b1");
    S04 { g, a1, a2, b1 }
}
pub(super) fn s04_host(fx: &S04, kind: Corruption) -> Host {
    let mut host = Host::new(&fx.g, AckOrder::CommitThenAck, PendingPolicy::Volatile);
    let mut net = Net::default();
    assert!(matches!(
        host.ingest(fx.a1.clone(), Fault::None, &mut net),
        Ingest::Stored(_)
    ));
    let stale = host.durable.snapshot.clone();
    for e in [&fx.a2, &fx.b1] {
        assert!(matches!(
            host.ingest(e.clone(), Fault::None, &mut net),
            Ingest::Stored(_)
        ));
    }
    host.crash();
    let d = &mut host.durable;
    match kind {
        Corruption::ProofSignature => {
            let mut e = decode(&d.originals[&fx.a1.id()]).unwrap();
            e.signature[0] ^= 1;
            d.originals.insert(fx.a1.id(), encode(&e));
        }
        Corruption::RecordSwapped => {
            let other = d.originals[&fx.b1.id()].clone();
            d.originals.insert(fx.a1.id(), other);
        }
        Corruption::Undecodable => {
            d.originals.insert(fx.a1.id(), b"not an envelope".to_vec());
        }
        Corruption::SnapshotBytes => d.snapshot = vec![0xff; 24],
        Corruption::StaleSnapshot => d.snapshot = stale,
        Corruption::IndexPointsElsewhere => {
            d.index.insert(fx.a2.key(), fx.a1.id());
        }
    }
    host
}
pub(super) fn proof_kind(kind: Corruption) -> bool {
    matches!(
        kind,
        Corruption::ProofSignature | Corruption::RecordSwapped | Corruption::Undecodable
    )
}
pub(super) fn assert_flagged(report: &OpenReport, kind: Corruption, fx: &S04) {
    match kind {
        _ if proof_kind(kind) => {
            assert_eq!(report.quarantined, BTreeSet::from([fx.a1.id()]), "{kind:?}");
            assert_eq!(
                report.unverifiable,
                BTreeSet::from([fx.a2.id()]),
                "{kind:?}"
            );
        }
        Corruption::SnapshotBytes | Corruption::StaleSnapshot => {
            assert!(
                report.snapshot_mismatch && report.quarantined.is_empty(),
                "{kind:?}"
            )
        }
        _ => assert!(
            report.index_mismatch && report.quarantined.is_empty(),
            "{kind:?}"
        ),
    }
}

// KIND: invariant
// S04 under either open policy: anything the host serves after opening a corrupted store is a
// verified original byte-identical to what the author signed; corrupted bytes are never served.
#[test]
fn s04_no_open_policy_serves_corrupted_proof_snapshot_or_index_as_valid() {
    let fx = s04_fixture();
    let originals = [&fx.a1, &fx.a2, &fx.b1].map(encode);
    // Positive control: the same store without corruption reopens clean.
    let mut clean = fresh_host(&fx.g);
    let mut net = Net::default();
    for e in [&fx.a1, &fx.a2, &fx.b1] {
        assert!(clean.ingest(e.clone(), Fault::None, &mut net).is_stored());
    }
    clean.crash();
    assert!(clean.open(OpenPolicy::FailClosed).unwrap().is_clean());
    for policy in [OpenPolicy::FailClosed, OpenPolicy::Reconstruct] {
        for kind in CORRUPTIONS {
            let mut host = s04_host(&fx, kind);
            let opened = host.open(policy);
            let report = opened.as_ref().unwrap_or_else(|r| r);
            assert_flagged(report, kind, &fx);
            let Some(served) = host.export_missing(&BTreeSet::new()) else {
                assert!(opened.is_err() && policy == OpenPolicy::FailClosed);
                continue;
            };
            assert!(!served.is_empty(), "{policy:?} {kind:?}");
            let mut independent = fresh(&fx.g);
            for bytes in &served {
                assert!(
                    originals.contains(bytes),
                    "{policy:?} {kind:?}: non-original"
                );
                // Export order is by id, so a dependent may arrive first (Pending).
                let outcome = independent.receive(decode(bytes).unwrap());
                assert!(matches!(outcome, Outcome::Accepted | Outcome::Pending));
            }
            assert_eq!(
                independent.accepted.len(),
                served.len(),
                "{policy:?} {kind:?}"
            );
            assert!(independent.pending.is_empty());
        }
    }
}

// KIND: candidate-policy S04
// FailClosed: any re-verification mismatch keeps the replica closed: nothing served, nothing
// ingested or acknowledged, and the corrupted store is left untouched as evidence.
#[test]
fn s04_fail_closed_policy_refuses_to_open_and_leaves_corrupted_store_untouched() {
    let fx = s04_fixture();
    let (_, _, _, mut c) = signed_fixture();
    let unrelated = c.post("arrives while closed");
    for kind in CORRUPTIONS {
        let mut host = s04_host(&fx, kind);
        let corrupted = host.durable.clone();
        let report = host
            .open(OpenPolicy::FailClosed)
            .expect_err("must not open");
        assert_flagged(&report, kind, &fx);
        assert!(host.volatile.is_none());
        assert!(host.export_missing(&BTreeSet::new()).is_none());
        let mut net = Net::default();
        assert_eq!(
            host.ingest(unrelated.clone(), Fault::None, &mut net),
            Ingest::Down
        );
        assert!(net.acks.is_empty() && net.pushes.is_empty());
        assert_eq!(host.durable, corrupted, "{kind:?}");
    }
}

// KIND: candidate-policy S04
// Reconstruct: quarantine records that fail re-verification, demote dependents to pending,
// rebuild index/snapshot from verified proofs in ONE write, and re-request what is missing.
#[test]
fn s04_reconstruct_policy_quarantines_and_rebuilds_from_verified_proofs() {
    let fx = s04_fixture();
    let mut control = Host::new(&fx.g, AckOrder::CommitThenAck, PendingPolicy::Volatile);
    let mut net = Net::default();
    for e in [&fx.a1, &fx.a2, &fx.b1] {
        control.ingest(e.clone(), Fault::None, &mut net);
    }
    for kind in CORRUPTIONS {
        let mut host = s04_host(&fx, kind);
        let writes = host.durable.writes;
        let report = host.open(OpenPolicy::Reconstruct).expect("reconstructs");
        assert_flagged(&report, kind, &fx);
        assert_eq!(host.durable.writes, writes + 1, "{kind:?}");
        let mut net = Net::default();
        if proof_kind(kind) {
            let r = host.replica();
            assert_eq!(r.accepted.keys().collect::<Vec<_>>(), vec![&fx.b1.id()]);
            assert_eq!(r.pending.keys().collect::<Vec<_>>(), vec![&fx.a2.id()]);
            assert_eq!(host.missing_dependencies(), BTreeSet::from([fx.a1.id()]));
            assert_eq!(
                host.durable.originals.keys().collect::<Vec<_>>(),
                vec![&fx.b1.id()]
            );
            // Re-requested original arrives; the demoted dependent is promoted, both acked.
            assert_eq!(
                host.ingest(fx.a1.clone(), Fault::None, &mut net),
                Ingest::Stored(BTreeSet::from([fx.a1.id(), fx.a2.id()]))
            );
            assert!(net.acks.iter().all(|(_, b)| *b) && net.acks.len() == 2);
        }
        assert_eq!(
            canonical(&host.durable),
            canonical(&control.durable),
            "{kind:?}"
        );
        // The rebuilt store reopens clean under the strict policy.
        host.crash();
        assert!(
            host.open(OpenPolicy::FailClosed).unwrap().is_clean(),
            "{kind:?}"
        );
    }
}

// KIND: invariant
// S05: quota refusal is distinct (no ack, durable unchanged); whole-shard eviction reports
// Evicted (not Stored, not Unknown) across restart; genesis authority and per-key identity are
// retained; dependents of evicted ops wait as pending and name what to re-request.
#[test]
fn s05_quota_and_shard_eviction_report_truthful_availability_and_keep_identity() {
    let (g, mut a, mut b, mut c) = signed_fixture();
    let a1 = a.post("a1");
    let a2 = a.post("a2");
    let b1 = b.post("b1");
    let c1 = c.post("c1");
    let mut twin = Author::new(1, "unconfigured");
    twin.configure(&g.id(), a.permit.clone().unwrap());
    let a1_equivocation = twin.post("same key, different op");
    assert_eq!(a1_equivocation.key(), a1.key());
    assert_ne!(a1_equivocation.id(), a1.id());

    let mut host = Host::new(&g, AckOrder::CommitThenAck, PendingPolicy::Volatile);
    host.quota = Some(2);
    let mut net = Net::default();
    for e in [&a1, &b1] {
        assert!(matches!(
            host.ingest(e.clone(), Fault::None, &mut net),
            Ingest::Stored(_)
        ));
    }
    let before = host.durable.clone();
    assert_eq!(
        host.ingest(c1.clone(), Fault::None, &mut net),
        Ingest::Quota
    );
    assert_eq!(host.durable, before);
    assert!(!net.acked(&c1.id()));
    assert_eq!(host.availability(&c1.id()), Availability::Unknown);

    host.evict_shard();
    assert_eq!(host.durable.writes, before.writes + 1);
    host.crash();
    assert!(host.open(OpenPolicy::FailClosed).unwrap().is_clean());
    for id in [a1.id(), b1.id()] {
        assert_eq!(host.availability(&id), Availability::Evicted);
        assert!(!host.durable.originals.contains_key(&id));
    }
    assert_eq!(host.export_missing(&BTreeSet::new()), Some(vec![]));
    // Authority (signed genesis + grants) survives eviction: an independent post is accepted.
    assert_eq!(
        host.ingest(c1.clone(), Fault::None, &mut net),
        Ingest::Stored(BTreeSet::from([c1.id()]))
    );
    // A dependent of an evicted op is neither accepted nor rejected; it names its dependency.
    assert_eq!(
        host.ingest(a2.clone(), Fault::None, &mut net),
        Ingest::Pending
    );
    assert_eq!(host.missing_dependencies(), BTreeSet::from([a1.id()]));
    // Single cause: the equivocation is otherwise valid (a fresh replica accepts it) ...
    assert_eq!(
        fresh(&g).receive(a1_equivocation.clone()),
        Outcome::Accepted
    );
    // ... but the retained (id, key) manifest refuses it with no state change.
    let (d, v) = (host.durable.clone(), view(host.replica()));
    assert_eq!(
        host.ingest(a1_equivocation, Fault::None, &mut net),
        Ingest::Invalid
    );
    assert_eq!((&host.durable, view(host.replica())), (&d, v));
    // Refill from an original (operator raised quota) restores availability truthfully.
    host.quota = None;
    assert_eq!(
        host.ingest(a1.clone(), Fault::None, &mut net),
        Ingest::Stored(BTreeSet::from([a1.id(), a2.id()]))
    );
    assert_eq!(host.availability(&a1.id()), Availability::Stored);
    assert_eq!(host.availability(&b1.id()), Availability::Evicted);
    assert!(!host.durable.evicted.contains_key(&a1.id()));
}

// KIND: limitation
// S05: a StoredAck is not a future-retention guarantee. After the sender drops its copy on ack
// and the host evicts, no replica in this model can serve the original again.
#[test]
fn s05_earlier_stored_ack_does_not_guarantee_retention_after_eviction() {
    let fx = fixture();
    let (mut host, net, _sub) = populated(&fx, AckOrder::CommitThenAck, PendingPolicy::Volatile);
    let mut sender = Sender::new(&[&fx.a1]);
    sender.absorb(&net);
    assert!(net.acks.contains(&(fx.a1.id(), true)));
    assert!(sender.unacked.is_empty());
    host.evict_shard();
    assert_eq!(host.availability(&fx.a1.id()), Availability::Evicted);
    let mut late = fresh(&fx.g);
    assert!(reconcile(&host, &mut late).is_empty());
    assert!(late.accepted.is_empty());
}

// a1 withheld; a2..a(n) depend on it. Host baseline holds b1.
pub(super) fn s06_setup(
    policy: PendingPolicy,
    extra: usize,
) -> (Genesis, Envelope, Vec<Envelope>, Host) {
    let (g, mut a, mut b, _c) = signed_fixture();
    let a1 = a.post("withheld");
    let later: Vec<Envelope> = (0..extra).map(|i| a.post(&format!("later {i}"))).collect();
    let b1 = b.post("b1");
    let mut host = Host::new(&g, AckOrder::CommitThenAck, policy);
    let mut net = Net::default();
    assert!(matches!(
        host.ingest(b1, Fault::None, &mut net),
        Ingest::Stored(_)
    ));
    (g, a1, later, host)
}
pub(super) fn s06_control(
    g: &Genesis,
    a1: &Envelope,
    later: &[Envelope],
    host: &Host,
) -> Canonical {
    let mut c = Host::new(g, AckOrder::CommitThenAck, PendingPolicy::Volatile);
    let mut net = Net::default();
    for e in host.replica().accepted.values() {
        c.ingest(e.clone(), Fault::None, &mut net);
    }
    for e in std::iter::once(a1).chain(later) {
        c.ingest(e.clone(), Fault::None, &mut net);
    }
    assert_eq!(c.replica().accepted.len(), 2 + later.len());
    canonical(&c.durable)
}

// KIND: invariant
// S06 under both pending policies: restart never promotes a pending envelope without its
// dependency; the queue stays bounded (overflow refused, never persisted); the accepted set
// after restart equals the pre-crash accepted set; supplying the dependency converges.
#[test]
fn s06_restart_with_pending_queue_never_accepts_implicitly_and_stays_bounded() {
    for policy in [PendingPolicy::Volatile, PendingPolicy::PersistBounded] {
        let (g, a1, later, mut host) = s06_setup(policy, MAX_PENDING + 1);
        let control = s06_control(&g, &a1, &later, &host);
        let mut net = Net::default();
        let mut sender = Sender::new(&later.iter().collect::<Vec<_>>());
        let outcomes: Vec<Ingest> = later
            .iter()
            .map(|e| host.ingest(e.clone(), Fault::None, &mut net))
            .collect();
        assert!(
            outcomes[..MAX_PENDING]
                .iter()
                .all(|o| *o == Ingest::Pending)
        );
        assert_eq!(outcomes[MAX_PENDING], Ingest::Limit);
        assert!(net.acks.is_empty());
        assert!(host.durable.pending.len() <= MAX_PENDING);
        let accepted_before = view(host.replica()).accepted;
        host.crash();
        assert!(host.open(OpenPolicy::FailClosed).unwrap().is_clean());
        assert_eq!(view(host.replica()).accepted, accepted_before, "{policy:?}");
        assert!(host.replica().pending.len() <= MAX_PENDING);
        for e in &later {
            assert!(!host.replica().accepted.contains_key(&e.id()), "{policy:?}");
        }
        assert!(host.ingest(a1.clone(), Fault::None, &mut net).is_stored());
        sender.retry(&mut host, &mut net);
        assert!(sender.unacked.is_empty(), "{policy:?}");
        let mut got = canonical(&host.durable);
        got.pending.clear();
        assert_eq!(got, control, "{policy:?}");
    }
}
impl Ingest {
    pub(super) fn is_stored(&self) -> bool {
        matches!(self, Ingest::Stored(_))
    }
}

// KIND: candidate-policy S06
// Volatile pending: the queue is lost on restart with no durable write; the host has nothing to
// re-request and recovery depends on the sender retrying its unacknowledged originals.
#[test]
fn s06_volatile_pending_queue_is_lost_on_restart_and_recovered_by_sender_retry() {
    let (g, a1, later, mut host) = s06_setup(PendingPolicy::Volatile, 2);
    let control = s06_control(&g, &a1, &later, &host);
    let mut net = Net::default();
    let mut sender = Sender::new(&later.iter().collect::<Vec<_>>());
    let durable = host.durable.clone();
    for e in &later {
        assert_eq!(
            host.ingest(e.clone(), Fault::None, &mut net),
            Ingest::Pending
        );
    }
    assert_eq!(host.durable, durable);
    host.crash();
    assert!(host.open(OpenPolicy::FailClosed).unwrap().is_clean());
    assert!(host.replica().pending.is_empty());
    assert!(host.missing_dependencies().is_empty());
    assert_eq!(
        sender.retry(&mut host, &mut net),
        vec![Ingest::Pending, Ingest::Pending]
    );
    assert_eq!(host.missing_dependencies(), BTreeSet::from([a1.id()]));
    let all: BTreeSet<String> = std::iter::once(&a1).chain(&later).map(|e| e.id()).collect();
    assert_eq!(host.ingest(a1, Fault::None, &mut net), Ingest::Stored(all));
    assert_eq!(canonical(&host.durable), control);
}
