//! G-C persistence/crash harness for group-chat-tests.md §4 (S01–S07). Test-only.
//!
//! A host replica has an explicit `Durable` record (the "disk") and `Volatile` state (the
//! in-memory `Replica`). A durable write is ONE ideal atomic struct assignment (`Host::write`).
//! A crash drops every volatile field; recovery (`Host::open`) rebuilds the replica only from
//! durable originals and re-verifies every proof. Faults are injected before/after, or as a
//! returned error at, each pipeline step: Validate, ScratchImport, Commit, StoredAck, FanOut.
//!
//! NOT modelled: fsync, power loss, torn/partial writes, redb transactions, vault sealing/AEAD,
//! disk-full, Android process death, real sockets. Process-crash coverage here is not evidence of
//! filesystem or power-loss durability (docs/group-persistence-contract.md).
//!
//! R1b: content here uses the LEGACY device-key-signed `Envelope` fixture (`tests.rs`; superseded
//! by U4 revised + U16 DID-signed posts with an app-supplied writer ID, see `host_only.rs`). The
//! envelope is only a carrier; the properties tested here do not depend on how a post is signed.
use super::*;
use loro::{LoroValue, VersionVector};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Validate,
    ScratchImport,
    // Durability boundary (C2 vocabulary: receiver `Commit`).
    Commit,
    StoredAck,
    FanOut,
}
const STEPS: [Step; 5] = [
    Step::Validate,
    Step::ScratchImport,
    Step::Commit,
    Step::StoredAck,
    Step::FanOut,
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    None,
    CrashBefore(Step),
    CrashAfter(Step),
    // The step returns an error; the process keeps running.
    Fail(Step),
}
fn all_faults() -> Vec<Fault> {
    STEPS
        .iter()
        .flat_map(|&s| [Fault::CrashBefore(s), Fault::CrashAfter(s), Fault::Fail(s)])
        .collect()
}
// AckBeforeCommit is a rejected ordering kept only as a counterexample (C2 §3c analogue).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AckOrder {
    CommitThenAck,
    AckBeforeCommit,
}
// S06 candidates: is the pending-dependency queue durable?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingPolicy {
    Volatile,
    PersistBounded,
}
// S04 candidates: what `open` does when durable state fails re-verification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenPolicy {
    FailClosed,
    Reconstruct,
}

// Stored original: the exact author-signed envelope, never a re-derived diff.
fn encode(e: &Envelope) -> Vec<u8> {
    serde_json::to_vec(&(&e.body, &e.attestation, e.signature.to_vec())).unwrap()
}
fn decode(bytes: &[u8]) -> Option<Envelope> {
    let (body, attestation, signature): (Body, proto::SignedAttestation, Vec<u8>) =
        serde_json::from_slice(bytes).ok()?;
    Some(Envelope {
        body,
        attestation,
        signature: signature.try_into().ok()?,
    })
}

#[derive(Clone, Debug, PartialEq)]
struct Durable {
    // envelope id -> original encoded envelope (proofs).
    originals: BTreeMap<String, Vec<u8>>,
    // "peer:sequence" -> envelope id (author-operation index).
    index: BTreeMap<String, String>,
    // Loro snapshot cache; reconstructible from `originals`, never trusted on its own.
    snapshot: Vec<u8>,
    // Written only under PendingPolicy::PersistBounded.
    pending: BTreeMap<String, Vec<u8>>,
    // S05 eviction manifest: evicted id -> "peer:sequence". Identity is retained, data is not.
    evicted: BTreeMap<String, String>,
    // Oracle counter of durable writes (part of byte-identity checks).
    writes: u64,
}
fn index_of(r: &Replica) -> BTreeMap<String, String> {
    r.accepted.values().map(|e| (e.key(), e.id())).collect()
}
fn encode_all(m: &BTreeMap<String, Envelope>) -> BTreeMap<String, Vec<u8>> {
    m.iter().map(|(id, e)| (id.clone(), encode(e))).collect()
}
fn capture(r: &Replica, policy: PendingPolicy, prev: &Durable) -> Durable {
    let mut evicted = prev.evicted.clone();
    evicted.retain(|id, _| !r.accepted.contains_key(id));
    Durable {
        originals: encode_all(&r.accepted),
        index: index_of(r),
        snapshot: r.doc.export(ExportMode::Snapshot).unwrap(),
        pending: match policy {
            PendingPolicy::Volatile => BTreeMap::new(),
            PendingPolicy::PersistBounded => encode_all(&r.pending),
        },
        evicted,
        writes: prev.writes,
    }
}
// Semantic comparison for "equals the fault-free control" (snapshot bytes are not compared).
#[derive(Debug, PartialEq)]
struct Canonical {
    originals: BTreeMap<String, Vec<u8>>,
    index: BTreeMap<String, String>,
    pending: BTreeSet<String>,
    evicted: BTreeMap<String, String>,
    value: LoroValue,
    vv: VersionVector,
}
fn canonical(d: &Durable) -> Canonical {
    let doc = LoroDoc::new();
    doc.import(&d.snapshot).expect("snapshot decodes");
    Canonical {
        originals: d.originals.clone(),
        index: d.index.clone(),
        pending: d.pending.keys().cloned().collect(),
        evicted: d.evicted.clone(),
        value: doc.get_deep_value(),
        vv: doc.oplog_vv(),
    }
}
// Populated-replica before/after snapshot (G-A §5 helper, local copy).
#[derive(Debug, PartialEq)]
struct View {
    value: LoroValue,
    vv: VersionVector,
    accepted: Vec<String>,
    pending: Vec<String>,
}
fn view(r: &Replica) -> View {
    View {
        value: r.doc.get_deep_value(),
        vv: r.doc.oplog_vv(),
        accepted: r.accepted.keys().cloned().collect(),
        pending: r.pending.keys().cloned().collect(),
    }
}
fn fork(r: &Replica, g: &Genesis) -> Replica {
    let mut s = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    s.doc = r.doc.fork();
    s.accepted = r.accepted.clone();
    s.pending = r.pending.clone();
    s.fanout = r.fanout.clone();
    s
}

#[derive(Default)]
struct Net {
    // StoredAck frames: (envelope id, oracle: was it in Durable when the ack left?).
    acks: Vec<(String, bool)>,
    // Best-effort fan-out pushes towards one subscriber.
    pushes: Vec<Envelope>,
}
impl Net {
    fn acked(&self, id: &str) -> bool {
        self.acks.iter().any(|(a, _)| a == id)
    }
}
#[derive(Debug, PartialEq)]
enum Ingest {
    Stored(BTreeSet<String>),
    Duplicate,
    Pending,
    Limit,
    Invalid,
    Quota,
    Crashed(Fault),
    Failed(Step),
    Down,
}
#[derive(Debug, Default)]
struct OpenReport {
    quarantined: BTreeSet<String>,
    unverifiable: BTreeSet<String>,
    quarantined_pending: BTreeSet<String>,
    index_mismatch: bool,
    snapshot_mismatch: bool,
}
impl OpenReport {
    fn is_clean(&self) -> bool {
        self.quarantined.is_empty()
            && self.unverifiable.is_empty()
            && self.quarantined_pending.is_empty()
            && !self.index_mismatch
            && !self.snapshot_mismatch
    }
}
#[derive(Debug, PartialEq)]
enum Availability {
    Stored,
    Evicted,
    Unknown,
}
struct Volatile {
    replica: Replica,
}
struct Host {
    genesis: Genesis,
    durable: Durable,
    volatile: Option<Volatile>,
    order: AckOrder,
    pending_policy: PendingPolicy,
    quota: Option<usize>,
}
impl Host {
    fn new(g: &Genesis, order: AckOrder, pending_policy: PendingPolicy) -> Self {
        let replica = Replica::from_genesis(g.clone(), &g.id()).unwrap();
        let empty = Durable {
            originals: BTreeMap::new(),
            index: BTreeMap::new(),
            snapshot: vec![],
            pending: BTreeMap::new(),
            evicted: BTreeMap::new(),
            writes: 0,
        };
        let durable = capture(&replica, pending_policy, &empty);
        Self {
            genesis: g.clone(),
            durable,
            volatile: Some(Volatile { replica }),
            order,
            pending_policy,
            quota: None,
        }
    }
    fn replica(&self) -> &Replica {
        &self.volatile.as_ref().expect("host is down").replica
    }
    fn write(&mut self, mut d: Durable) {
        d.writes = self.durable.writes + 1;
        self.durable = d;
    }
    fn crash(&mut self) {
        self.volatile = None;
    }
    fn ingest(&mut self, e: Envelope, fault: Fault, net: &mut Net) -> Ingest {
        let Some(mut vol) = self.volatile.take() else {
            return Ingest::Down;
        };
        // A crash returns without restoring `vol`: every volatile field is dropped.
        macro_rules! before {
            ($s:expr) => {
                if fault == Fault::CrashBefore($s) {
                    return Ingest::Crashed(fault);
                }
                if fault == Fault::Fail($s) {
                    self.volatile = Some(vol);
                    return Ingest::Failed($s);
                }
            };
        }
        macro_rules! after {
            ($s:expr) => {
                if fault == Fault::CrashAfter($s) {
                    return Ingest::Crashed(fault);
                }
            };
        }
        macro_rules! finish {
            ($out:expr) => {{
                self.volatile = Some(vol);
                return $out;
            }};
        }
        let id = e.id();
        before!(Step::Validate);
        // S05: an evicted (id, key) pair keeps identity; a different id for that key is refused.
        let equivocates = self
            .durable
            .evicted
            .iter()
            .any(|(old, key)| *key == e.key() && *old != id);
        if equivocates || !vol.replica.valid_proof(&e) {
            finish!(Ingest::Invalid);
        }
        let duplicate = vol.replica.accepted.contains_key(&id);
        if !duplicate && !vol.replica.dependencies_ready(&e) {
            let out = vol.replica.receive(e);
            if out == Outcome::Pending && self.pending_policy == PendingPolicy::PersistBounded {
                let mut d = self.durable.clone();
                d.pending = encode_all(&vol.replica.pending);
                if d != self.durable {
                    self.write(d);
                }
            }
            after!(Step::Validate);
            finish!(if out == Outcome::Limit {
                Ingest::Limit
            } else {
                Ingest::Pending
            });
        }
        if !duplicate
            && self
                .quota
                .is_some_and(|q| self.durable.originals.len() >= q)
        {
            finish!(Ingest::Quota);
        }
        after!(Step::Validate);
        let mut scratch = None;
        let mut ids = BTreeSet::from([id.clone()]);
        if !duplicate {
            before!(Step::ScratchImport);
            let mut s = fork(&vol.replica, &self.genesis);
            if s.receive(e) != Outcome::Accepted {
                finish!(Ingest::Invalid);
            }
            // Includes pending envelopes promoted by this one.
            ids = s
                .accepted
                .keys()
                .filter(|k| !vol.replica.accepted.contains_key(*k))
                .cloned()
                .collect();
            after!(Step::ScratchImport);
            scratch = Some(s);
        }
        let originals: Vec<Envelope> = ids
            .iter()
            .map(|i| scratch.as_ref().unwrap_or(&vol.replica).accepted[i].clone())
            .collect();
        let steps = match self.order {
            AckOrder::CommitThenAck => [Step::Commit, Step::StoredAck, Step::FanOut],
            AckOrder::AckBeforeCommit => [Step::StoredAck, Step::Commit, Step::FanOut],
        };
        for step in steps {
            // A durable duplicate is only re-acknowledged (idempotent; no write, no push).
            if duplicate && step != Step::StoredAck {
                continue;
            }
            before!(step);
            match step {
                Step::Commit => {
                    let s = scratch.take().unwrap();
                    let d = capture(&s, self.pending_policy, &self.durable);
                    self.write(d);
                    vol.replica = s;
                }
                Step::StoredAck => {
                    for i in &ids {
                        let backed = self.durable.originals.contains_key(i);
                        net.acks.push((i.clone(), backed));
                    }
                }
                Step::FanOut => net.pushes.extend(originals.iter().cloned()),
                _ => unreachable!(),
            }
            after!(step);
        }
        finish!(if duplicate {
            Ingest::Duplicate
        } else {
            Ingest::Stored(ids)
        })
    }
    // Recovery: rebuild ONLY from durable originals with full re-verification.
    fn open(&mut self, policy: OpenPolicy) -> Result<OpenReport, OpenReport> {
        assert!(self.volatile.is_none(), "open is the restart path");
        let mut r = Replica::from_genesis(self.genesis.clone(), &self.genesis.id()).unwrap();
        let mut report = OpenReport::default();
        let mut todo = BTreeMap::new();
        for (id, bytes) in &self.durable.originals {
            match decode(bytes) {
                Some(e) if e.id() == *id => {
                    todo.insert(id.clone(), e);
                }
                _ => {
                    report.quarantined.insert(id.clone());
                }
            }
        }
        loop {
            let ready: Vec<String> = todo
                .iter()
                .filter(|(_, e)| r.dependencies_ready(e))
                .map(|(id, _)| id.clone())
                .collect();
            if ready.is_empty() {
                break;
            }
            for id in ready {
                let e = todo.remove(&id).unwrap();
                if r.receive(e) != Outcome::Accepted {
                    report.quarantined.insert(id);
                }
            }
        }
        // Valid proofs whose stored dependencies did not verify: pending, never accepted.
        for (id, e) in todo {
            report.unverifiable.insert(id.clone());
            if !matches!(r.receive(e), Outcome::Pending) {
                report.quarantined.insert(id);
            }
        }
        report.index_mismatch = index_of(&r) != self.durable.index;
        let snap = LoroDoc::new();
        report.snapshot_mismatch = snap.import(&self.durable.snapshot).is_err()
            || snap.get_deep_value() != r.doc.get_deep_value()
            || snap.oplog_vv() != r.doc.oplog_vv();
        for (id, bytes) in &self.durable.pending {
            match decode(bytes) {
                Some(e) if e.id() == *id && r.valid_proof(&e) && !r.dependencies_ready(&e) => {
                    if r.receive(e) != Outcome::Pending {
                        report.quarantined_pending.insert(id.clone());
                    }
                }
                _ => {
                    report.quarantined_pending.insert(id.clone());
                }
            }
        }
        // Rehydration is not new acceptance: no fan-out, no StoredAck.
        r.fanout.clear();
        if report.is_clean() {
            self.volatile = Some(Volatile { replica: r });
            return Ok(report);
        }
        match policy {
            OpenPolicy::FailClosed => Err(report),
            OpenPolicy::Reconstruct => {
                let d = capture(&r, self.pending_policy, &self.durable);
                self.write(d);
                self.volatile = Some(Volatile { replica: r });
                Ok(report)
            }
        }
    }
    // Serves stored originals for verified (volatile-accepted) ids only.
    fn export_missing(&self, known: &BTreeSet<String>) -> Option<Vec<Vec<u8>>> {
        let vol = self.volatile.as_ref()?;
        Some(
            vol.replica
                .accepted
                .keys()
                .filter(|id| !known.contains(*id))
                .map(|id| self.durable.originals[id].clone())
                .collect(),
        )
    }
    fn availability(&self, id: &str) -> Availability {
        if self.replica().accepted.contains_key(id) {
            Availability::Stored
        } else if self.durable.evicted.contains_key(id) {
            Availability::Evicted
        } else {
            Availability::Unknown
        }
    }
    fn missing_dependencies(&self) -> BTreeSet<String> {
        let r = self.replica();
        r.pending
            .values()
            .flat_map(|e| e.body.previous.iter().chain(e.body.dependencies.iter()))
            .filter(|d| !r.accepted.contains_key(*d) && !r.pending.contains_key(*d))
            .cloned()
            .collect()
    }
    // S05: whole-shard eviction under quota. One durable write; identity manifest retained.
    fn evict_shard(&mut self) {
        let vol = self.volatile.as_mut().expect("host is down");
        let mut fresh = Replica::from_genesis(self.genesis.clone(), &self.genesis.id()).unwrap();
        fresh.pending = vol.replica.pending.clone();
        let mut d = capture(&fresh, self.pending_policy, &self.durable);
        for (id, e) in &vol.replica.accepted {
            d.evicted.insert(id.clone(), e.key());
        }
        vol.replica = fresh;
        self.write(d);
    }
}

struct Sender {
    // Persisted originals awaiting StoredAck; retries resend these exact envelopes.
    unacked: BTreeMap<String, Envelope>,
}
impl Sender {
    fn new(es: &[&Envelope]) -> Self {
        Self {
            unacked: es.iter().map(|e| (e.id(), (*e).clone())).collect(),
        }
    }
    fn absorb(&mut self, net: &Net) {
        for (id, _) in &net.acks {
            self.unacked.remove(id);
        }
    }
    fn retry(&mut self, host: &mut Host, net: &mut Net) -> Vec<Ingest> {
        self.absorb(net);
        let es: Vec<Envelope> = self.unacked.values().cloned().collect();
        let out = es
            .into_iter()
            .map(|e| host.ingest(e, Fault::None, net))
            .collect();
        self.absorb(net);
        out
    }
}
fn deliver(net: &mut Net, sub: &mut Replica) -> Vec<Outcome> {
    net.pushes.drain(..).map(|e| sub.receive(e)).collect()
}
fn reconcile(host: &Host, sub: &mut Replica) -> Vec<Outcome> {
    let known = sub.accepted.keys().cloned().collect();
    host.export_missing(&known)
        .expect("host is open")
        .iter()
        .map(|b| sub.receive(decode(b).unwrap()))
        .collect()
}
fn fresh(g: &Genesis) -> Replica {
    Replica::from_genesis(g.clone(), &g.id()).unwrap()
}

// a1; b1 saw a1; c1 saw both. Host baseline holds a1, b1. c1 is the update under test.
struct Fx {
    g: Genesis,
    a1: Envelope,
    b1: Envelope,
    c1: Envelope,
}
fn fixture() -> Fx {
    let (g, mut a, mut b, mut c) = signed_fixture();
    let a1 = a.post("a1");
    b.observe(&a1);
    let b1 = b.post("b1 saw a1");
    c.observe(&a1);
    c.observe(&b1);
    let c1 = c.post("c1 saw both");
    Fx { g, a1, b1, c1 }
}
fn populated(fx: &Fx, order: AckOrder, policy: PendingPolicy) -> (Host, Net, Replica) {
    let mut host = Host::new(&fx.g, order, policy);
    let mut net = Net::default();
    let mut sub = fresh(&fx.g);
    for e in [&fx.a1, &fx.b1] {
        assert_eq!(
            host.ingest(e.clone(), Fault::None, &mut net),
            Ingest::Stored(BTreeSet::from([e.id()]))
        );
    }
    assert_eq!(
        deliver(&mut net, &mut sub),
        vec![Outcome::Accepted, Outcome::Accepted]
    );
    (host, net, sub)
}
fn control(fx: &Fx) -> Host {
    let (mut host, mut net, _) = populated(fx, AckOrder::CommitThenAck, PendingPolicy::Volatile);
    assert!(matches!(
        host.ingest(fx.c1.clone(), Fault::None, &mut net),
        Ingest::Stored(_)
    ));
    assert_eq!(host.durable.writes, 3);
    host
}
struct Run {
    host: Host,
    net: Net,
    sender: Sender,
    sub: Replica,
    first: Ingest,
    durable_before: Durable,
    view_before: View,
}
fn run(fx: &Fx, fault: Fault, order: AckOrder) -> Run {
    let (mut host, mut net, sub) = populated(fx, order, PendingPolicy::Volatile);
    let sender = Sender::new(&[&fx.c1]);
    let durable_before = host.durable.clone();
    let view_before = view(host.replica());
    let first = host.ingest(fx.c1.clone(), fault, &mut net);
    Run {
        host,
        net,
        sender,
        sub,
        first,
        durable_before,
        view_before,
    }
}
fn expect_injected(r: &Run, fault: Fault) {
    let hit = match fault {
        Fault::Fail(s) => r.first == Ingest::Failed(s),
        _ => r.first == Ingest::Crashed(fault),
    };
    assert!(hit, "{fault:?} not reached: {:?}", r.first);
    assert_eq!(
        r.host.volatile.is_none(),
        !matches!(fault, Fault::Fail(_)),
        "{fault:?}"
    );
}
fn restart_if_down(host: &mut Host) {
    if host.volatile.is_none() {
        let report = host
            .open(OpenPolicy::FailClosed)
            .expect("intact store opens");
        assert!(report.is_clean());
    }
}
fn recover(r: &mut Run) {
    restart_if_down(&mut r.host);
    deliver(&mut r.net, &mut r.sub);
    r.sender.retry(&mut r.host, &mut r.net);
    deliver(&mut r.net, &mut r.sub);
    reconcile(&r.host, &mut r.sub);
}
fn assert_converged(r: &Run, control: &Host, fx: &Fx, fault: Fault) {
    let id = fx.c1.id();
    assert!(
        r.net.acks.iter().all(|(_, b)| *b),
        "{fault:?}: ack before commit"
    );
    for (a, _) in &r.net.acks {
        assert!(r.host.durable.originals.contains_key(a), "{fault:?}");
    }
    assert!(r.net.acked(&id), "{fault:?}: update never acknowledged");
    assert_eq!(
        canonical(&r.host.durable),
        canonical(&control.durable),
        "{fault:?}"
    );
    // Exactly one commit for the update across crash, failure and retries.
    assert_eq!(r.host.durable.writes, control.durable.writes, "{fault:?}");
    assert_eq!(view(r.host.replica()), view(control.replica()), "{fault:?}");
    assert_eq!(view(&r.sub), view(r.host.replica()), "{fault:?}");
    assert_eq!(r.sub.accepted.len(), 3, "{fault:?}");
    assert_eq!(
        r.sub.fanout.len(),
        3,
        "{fault:?}: duplicate subscriber event"
    );
    assert!(r.sender.unacked.is_empty(), "{fault:?}");
    assert_eq!(r.host.durable.originals[&id], encode(&fx.c1), "{fault:?}");
    assert_eq!(r.sub.accepted[&id].signature, fx.c1.signature, "{fault:?}");
}

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
enum Corruption {
    ProofSignature,
    RecordSwapped,
    Undecodable,
    SnapshotBytes,
    StaleSnapshot,
    IndexPointsElsewhere,
}
const CORRUPTIONS: [Corruption; 6] = [
    Corruption::ProofSignature,
    Corruption::RecordSwapped,
    Corruption::Undecodable,
    Corruption::SnapshotBytes,
    Corruption::StaleSnapshot,
    Corruption::IndexPointsElsewhere,
];
// Host holds a1, a2 (previous = a1) and b1 (independent); then crashes; then disk corruption.
struct S04 {
    g: Genesis,
    a1: Envelope,
    a2: Envelope,
    b1: Envelope,
}
fn s04_fixture() -> S04 {
    let (g, mut a, mut b, _c) = signed_fixture();
    let a1 = a.post("a1");
    let a2 = a.post("a2");
    let b1 = b.post("b1");
    S04 { g, a1, a2, b1 }
}
fn s04_host(fx: &S04, kind: Corruption) -> Host {
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
fn proof_kind(kind: Corruption) -> bool {
    matches!(
        kind,
        Corruption::ProofSignature | Corruption::RecordSwapped | Corruption::Undecodable
    )
}
fn assert_flagged(report: &OpenReport, kind: Corruption, fx: &S04) {
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
fn s06_setup(policy: PendingPolicy, extra: usize) -> (Genesis, Envelope, Vec<Envelope>, Host) {
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
fn s06_control(g: &Genesis, a1: &Envelope, later: &[Envelope], host: &Host) -> Canonical {
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
    fn is_stored(&self) -> bool {
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

// KIND: candidate-policy S06
// PersistBounded: pending originals are written durably (still not accepted), re-verified on
// open, and the host itself names the missing dependency to re-request. A corrupted persisted
// pending record is quarantined, never promoted, and is recovered from the sender's original.
#[test]
fn s06_persisted_pending_queue_is_reverified_and_rerequested_after_restart() {
    let (g, a1, later, mut host) = s06_setup(PendingPolicy::PersistBounded, 2);
    let control = s06_control(&g, &a1, &later, &host);
    let mut net = Net::default();
    let mut sender = Sender::new(&later.iter().collect::<Vec<_>>());
    let writes = host.durable.writes;
    for e in &later {
        assert_eq!(
            host.ingest(e.clone(), Fault::None, &mut net),
            Ingest::Pending
        );
    }
    assert_eq!(host.durable.writes, writes + 2);
    assert_eq!(host.durable.pending.len(), 2);
    assert!(host.durable.originals.len() == 1 && net.acks.is_empty());
    host.crash();
    assert!(host.open(OpenPolicy::FailClosed).unwrap().is_clean());
    assert_eq!(host.replica().pending.len(), 2);
    assert_eq!(host.missing_dependencies(), BTreeSet::from([a1.id()]));
    let all: BTreeSet<String> = std::iter::once(&a1).chain(&later).map(|e| e.id()).collect();
    assert_eq!(
        host.ingest(a1.clone(), Fault::None, &mut net),
        Ingest::Stored(all)
    );
    assert!(host.durable.pending.is_empty());
    assert_eq!(canonical(&host.durable), control);
    sender.absorb(&net);
    assert!(sender.unacked.is_empty());

    // Corrupted persisted pending record (signature flip on the last chain element).
    let (g, a1, later, mut host) = s06_setup(PendingPolicy::PersistBounded, 2);
    let mut sender = Sender::new(&later.iter().collect::<Vec<_>>());
    let control = s06_control(&g, &a1, &later, &host);
    let mut net = Net::default();
    for e in &later {
        assert_eq!(
            host.ingest(e.clone(), Fault::None, &mut net),
            Ingest::Pending
        );
    }
    host.crash();
    let target = later[1].id();
    let mut bad = decode(&host.durable.pending[&target]).unwrap();
    bad.signature[0] ^= 1;
    host.durable.pending.insert(target.clone(), encode(&bad));
    let report = host.open(OpenPolicy::Reconstruct).unwrap();
    assert_eq!(report.quarantined_pending, BTreeSet::from([target.clone()]));
    assert_eq!(
        host.ingest(a1.clone(), Fault::None, &mut net),
        Ingest::Stored(BTreeSet::from([a1.id(), later[0].id()]))
    );
    assert!(!host.replica().accepted.contains_key(&target));
    assert_eq!(
        sender.retry(&mut host, &mut net),
        vec![Ingest::Stored(BTreeSet::from([target]))]
    );
    assert_eq!(canonical(&host.durable), control);
}
fn fresh_host(g: &Genesis) -> Host {
    Host::new(g, AckOrder::CommitThenAck, PendingPolicy::Volatile)
}

// a1; b1 saw a1; a2; c1 saw a1+b1; b2. Peer P got an earlier overlapping batch [a1, b1, a2].
struct S07 {
    g: Genesis,
    es: Vec<Envelope>,
    a: Author,
}
fn s07_fixture() -> S07 {
    let (g, mut a, mut b, mut c) = signed_fixture();
    let a1 = a.post("a1");
    b.observe(&a1);
    let b1 = b.post("b1");
    let a2 = a.post("a2");
    c.observe(&a1);
    c.observe(&b1);
    let c1 = c.post("c1");
    let b2 = b.post("b2");
    S07 {
        g,
        es: vec![a1, b1, a2, c1, b2],
        a,
    }
}
fn s07_host_and_peer(fx: &S07) -> (Host, Replica) {
    let mut host = fresh_host(&fx.g);
    let mut net = Net::default();
    for e in &fx.es {
        assert!(host.ingest(e.clone(), Fault::None, &mut net).is_stored());
    }
    // Export happens after a restart: only persisted originals are available.
    host.crash();
    assert!(host.open(OpenPolicy::FailClosed).unwrap().is_clean());
    let mut peer = fresh(&fx.g);
    for e in &fx.es[..3] {
        assert_eq!(peer.receive(e.clone()), Outcome::Accepted);
    }
    (host, peer)
}

// KIND: invariant
// S07: export after overlap sends exactly the missing ops as stored original bytes (author
// signatures intact); resending the whole overlapping set is idempotent.
#[test]
fn s07_export_after_overlapping_batch_sends_only_missing_byte_identical_originals() {
    let fx = s07_fixture();
    let (host, mut peer) = s07_host_and_peer(&fx);
    let known: BTreeSet<String> = peer.accepted.keys().cloned().collect();
    let exported = host.export_missing(&known).unwrap();
    let expected: BTreeSet<Vec<u8>> = fx.es[3..].iter().map(encode).collect();
    assert_eq!(exported.iter().cloned().collect::<BTreeSet<_>>(), expected);
    for bytes in &exported {
        assert!(host.durable.originals.values().any(|o| o == bytes));
        let e = decode(bytes).unwrap();
        let original = fx.es.iter().find(|o| o.id() == e.id()).unwrap();
        assert_eq!(e.signature, original.signature);
        assert_eq!(e.body.author, original.body.author);
    }
    let outcomes: Vec<Outcome> = exported
        .iter()
        .map(|b| peer.receive(decode(b).unwrap()))
        .collect();
    assert_eq!(outcomes, vec![Outcome::Accepted, Outcome::Accepted]);
    assert_eq!(view(&peer), view(host.replica()));
    assert_eq!(peer.fanout.len(), 5);
    let settled = view(&peer);
    for bytes in host.export_missing(&BTreeSet::new()).unwrap() {
        assert_eq!(peer.receive(decode(&bytes).unwrap()), Outcome::Duplicate);
    }
    assert_eq!(view(&peer), settled);
    assert_eq!(peer.fanout.len(), 5);
}

// KIND: invariant
// S07 negative control: a diff synthesized from host state is refused, whether it reuses an
// author's signature (relabelled) or is freshly signed by a member over other authors' ops.
// Each input differs from a valid one in one respect; the populated peer is unchanged.
#[test]
fn s07_synthesized_diff_from_host_state_is_rejected_and_leaves_peer_unchanged() {
    let mut fx = s07_fixture();
    let (host, mut peer) = s07_host_and_peer(&fx);
    let diff = host
        .replica()
        .doc
        .export(ExportMode::updates(&peer.doc.oplog_vv()))
        .unwrap();
    // (i) c1's original with its update replaced by the synthesized diff; c1's signature kept.
    let mut relabelled = fx.es[3].clone();
    relabelled.body.update = diff.clone();
    // (ii) member A signs a well-formed next envelope whose update carries B's and C's ops.
    let a = &mut fx.a;
    let p = peer_of(a);
    let mut signed = Envelope {
        body: Body {
            group: a.group.clone(),
            doc: a.doc_name.clone(),
            author: a.id.did().into(),
            device: a.key.verifying_key().to_bytes(),
            peer: p,
            sequence: a.sequence + 1,
            previous: a.previous.clone(),
            dependencies: a.observed.clone(),
            permit: a.permit.clone(),
            text: "synthesized".into(),
            update: diff,
        },
        attestation: a.attestation.clone(),
        signature: [0; 64],
    };
    signed.signature = a.key.sign(&signed.bytes()).to_bytes();
    assert!(peer.valid_proof(&signed) && peer.dependencies_ready(&signed));
    let before = view(&peer);
    let fanout = peer.fanout.len();
    for e in [relabelled, signed] {
        assert_eq!(peer.receive(e), Outcome::Invalid);
        assert_eq!(view(&peer), before);
        assert_eq!(peer.fanout.len(), fanout);
    }
    // Positive control: the real originals are accepted.
    for e in &fx.es[3..] {
        assert_eq!(peer.receive(e.clone()), Outcome::Accepted);
    }
}
fn peer_of(a: &Author) -> u64 {
    peer(&a.key.verifying_key().to_bytes(), &a.doc_name)
}
