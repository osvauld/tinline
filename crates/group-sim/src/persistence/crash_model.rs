use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Validate,
    ScratchImport,
    // Durability boundary (C2 vocabulary: receiver `Commit`).
    Commit,
    StoredAck,
    FanOut,
}
pub(super) const STEPS: [Step; 5] = [
    Step::Validate,
    Step::ScratchImport,
    Step::Commit,
    Step::StoredAck,
    Step::FanOut,
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fault {
    None,
    CrashBefore(Step),
    CrashAfter(Step),
    // The step returns an error; the process keeps running.
    Fail(Step),
}
pub(super) fn all_faults() -> Vec<Fault> {
    STEPS
        .iter()
        .flat_map(|&s| [Fault::CrashBefore(s), Fault::CrashAfter(s), Fault::Fail(s)])
        .collect()
}
// AckBeforeCommit is a rejected ordering kept only as a counterexample (C2 §3c analogue).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AckOrder {
    CommitThenAck,
    AckBeforeCommit,
}
// S06 candidates: is the pending-dependency queue durable?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PendingPolicy {
    Volatile,
    PersistBounded,
}
// S04 candidates: what `open` does when durable state fails re-verification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OpenPolicy {
    FailClosed,
    Reconstruct,
}

// Stored original: the exact author-signed envelope, never a re-derived diff.
pub(super) fn encode(e: &Envelope) -> Vec<u8> {
    serde_json::to_vec(&(&e.body, &e.attestation, e.signature.to_vec())).unwrap()
}
pub(super) fn decode(bytes: &[u8]) -> Option<Envelope> {
    let (body, attestation, signature): (Body, proto::SignedAttestation, Vec<u8>) =
        serde_json::from_slice(bytes).ok()?;
    Some(Envelope {
        body,
        attestation,
        signature: signature.try_into().ok()?,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Durable {
    // envelope id -> original encoded envelope (proofs).
    pub(super) originals: BTreeMap<String, Vec<u8>>,
    // "peer:sequence" -> envelope id (author-operation index).
    pub(super) index: BTreeMap<String, String>,
    // Loro snapshot cache; reconstructible from `originals`, never trusted on its own.
    pub(super) snapshot: Vec<u8>,
    // Written only under PendingPolicy::PersistBounded.
    pub(super) pending: BTreeMap<String, Vec<u8>>,
    // S05 eviction manifest: evicted id -> "peer:sequence". Identity is retained, data is not.
    pub(super) evicted: BTreeMap<String, String>,
    // Oracle counter of durable writes (part of byte-identity checks).
    pub(super) writes: u64,
}
pub(super) fn index_of(r: &Replica) -> BTreeMap<String, String> {
    r.accepted.values().map(|e| (e.key(), e.id())).collect()
}
pub(super) fn encode_all(m: &BTreeMap<String, Envelope>) -> BTreeMap<String, Vec<u8>> {
    m.iter().map(|(id, e)| (id.clone(), encode(e))).collect()
}
pub(super) fn capture(r: &Replica, policy: PendingPolicy, prev: &Durable) -> Durable {
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
pub(super) struct Canonical {
    pub(super) originals: BTreeMap<String, Vec<u8>>,
    pub(super) index: BTreeMap<String, String>,
    pub(super) pending: BTreeSet<String>,
    pub(super) evicted: BTreeMap<String, String>,
    pub(super) value: LoroValue,
    pub(super) vv: VersionVector,
}
pub(super) fn canonical(d: &Durable) -> Canonical {
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
pub(super) struct View {
    pub(super) value: LoroValue,
    pub(super) vv: VersionVector,
    pub(super) accepted: Vec<String>,
    pub(super) pending: Vec<String>,
}
pub(super) fn view(r: &Replica) -> View {
    View {
        value: r.doc.get_deep_value(),
        vv: r.doc.oplog_vv(),
        accepted: r.accepted.keys().cloned().collect(),
        pending: r.pending.keys().cloned().collect(),
    }
}
pub(super) fn fork(r: &Replica, g: &Genesis) -> Replica {
    let mut s = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    s.doc = r.doc.fork();
    s.accepted = r.accepted.clone();
    s.pending = r.pending.clone();
    s.fanout = r.fanout.clone();
    s
}

#[derive(Default)]
pub(super) struct Net {
    // StoredAck frames: (envelope id, oracle: was it in Durable when the ack left?).
    pub(super) acks: Vec<(String, bool)>,
    // Best-effort fan-out pushes towards one subscriber.
    pub(super) pushes: Vec<Envelope>,
}
impl Net {
    pub(super) fn acked(&self, id: &str) -> bool {
        self.acks.iter().any(|(a, _)| a == id)
    }
}
#[derive(Debug, PartialEq)]
pub(super) enum Ingest {
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
pub(super) struct OpenReport {
    pub(super) quarantined: BTreeSet<String>,
    pub(super) unverifiable: BTreeSet<String>,
    pub(super) quarantined_pending: BTreeSet<String>,
    pub(super) index_mismatch: bool,
    pub(super) snapshot_mismatch: bool,
}
impl OpenReport {
    pub(super) fn is_clean(&self) -> bool {
        self.quarantined.is_empty()
            && self.unverifiable.is_empty()
            && self.quarantined_pending.is_empty()
            && !self.index_mismatch
            && !self.snapshot_mismatch
    }
}
#[derive(Debug, PartialEq)]
pub(super) enum Availability {
    Stored,
    Evicted,
    Unknown,
}
pub(super) struct Volatile {
    pub(super) replica: Replica,
}
pub(super) struct Host {
    pub(super) genesis: Genesis,
    pub(super) durable: Durable,
    pub(super) volatile: Option<Volatile>,
    pub(super) order: AckOrder,
    pub(super) pending_policy: PendingPolicy,
    pub(super) quota: Option<usize>,
}
impl Host {
    pub(super) fn new(g: &Genesis, order: AckOrder, pending_policy: PendingPolicy) -> Self {
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
    pub(super) fn replica(&self) -> &Replica {
        &self.volatile.as_ref().expect("host is down").replica
    }
    pub(super) fn write(&mut self, mut d: Durable) {
        d.writes = self.durable.writes + 1;
        self.durable = d;
    }
    pub(super) fn crash(&mut self) {
        self.volatile = None;
    }
    pub(super) fn ingest(&mut self, e: Envelope, fault: Fault, net: &mut Net) -> Ingest {
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
    pub(super) fn open(&mut self, policy: OpenPolicy) -> Result<OpenReport, OpenReport> {
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
    pub(super) fn export_missing(&self, known: &BTreeSet<String>) -> Option<Vec<Vec<u8>>> {
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
    pub(super) fn availability(&self, id: &str) -> Availability {
        if self.replica().accepted.contains_key(id) {
            Availability::Stored
        } else if self.durable.evicted.contains_key(id) {
            Availability::Evicted
        } else {
            Availability::Unknown
        }
    }
    pub(super) fn missing_dependencies(&self) -> BTreeSet<String> {
        let r = self.replica();
        r.pending
            .values()
            .flat_map(|e| e.body.previous.iter().chain(e.body.dependencies.iter()))
            .filter(|d| !r.accepted.contains_key(*d) && !r.pending.contains_key(*d))
            .cloned()
            .collect()
    }
    // S05: whole-shard eviction under quota. One durable write; identity manifest retained.
    pub(super) fn evict_shard(&mut self) {
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

pub(super) struct Sender {
    // Persisted originals awaiting StoredAck; retries resend these exact envelopes.
    pub(super) unacked: BTreeMap<String, Envelope>,
}
impl Sender {
    pub(super) fn new(es: &[&Envelope]) -> Self {
        Self {
            unacked: es.iter().map(|e| (e.id(), (*e).clone())).collect(),
        }
    }
    pub(super) fn absorb(&mut self, net: &Net) {
        for (id, _) in &net.acks {
            self.unacked.remove(id);
        }
    }
    pub(super) fn retry(&mut self, host: &mut Host, net: &mut Net) -> Vec<Ingest> {
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
pub(super) fn deliver(net: &mut Net, sub: &mut Replica) -> Vec<Outcome> {
    net.pushes.drain(..).map(|e| sub.receive(e)).collect()
}
pub(super) fn reconcile(host: &Host, sub: &mut Replica) -> Vec<Outcome> {
    let known = sub.accepted.keys().cloned().collect();
    host.export_missing(&known)
        .expect("host is open")
        .iter()
        .map(|b| sub.receive(decode(b).unwrap()))
        .collect()
}
pub(super) fn fresh(g: &Genesis) -> Replica {
    Replica::from_genesis(g.clone(), &g.id()).unwrap()
}

// a1; b1 saw a1; c1 saw both. Host baseline holds a1, b1. c1 is the update under test.
pub(super) struct Fx {
    pub(super) g: Genesis,
    pub(super) a1: Envelope,
    pub(super) b1: Envelope,
    pub(super) c1: Envelope,
}
pub(super) fn fixture() -> Fx {
    let (g, mut a, mut b, mut c) = signed_fixture();
    let a1 = a.post("a1");
    b.observe(&a1);
    let b1 = b.post("b1 saw a1");
    c.observe(&a1);
    c.observe(&b1);
    let c1 = c.post("c1 saw both");
    Fx { g, a1, b1, c1 }
}
pub(super) fn populated(fx: &Fx, order: AckOrder, policy: PendingPolicy) -> (Host, Net, Replica) {
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
pub(super) fn control(fx: &Fx) -> Host {
    let (mut host, mut net, _) = populated(fx, AckOrder::CommitThenAck, PendingPolicy::Volatile);
    assert!(matches!(
        host.ingest(fx.c1.clone(), Fault::None, &mut net),
        Ingest::Stored(_)
    ));
    assert_eq!(host.durable.writes, 3);
    host
}
