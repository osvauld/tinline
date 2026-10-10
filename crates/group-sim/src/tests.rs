use super::authority::{Capability, Genesis, Grant, PostAuthorization, SignedGroupAuthority};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use identity::Identity;
use loro::{ExportMode, LoroDoc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const DOMAIN: &[u8] = b"tinline/group-simulation/not-production/v1\0";
const MAX_PENDING: usize = 16;

#[path = "reachability.rs"]
mod reachability;
#[path = "persistence.rs"]
mod persistence;
#[path = "policy_candidates.rs"]
mod policy_candidates;
#[path = "block_in_group.rs"]
mod block_in_group;
#[path = "host_relay.rs"]
mod host_relay;
#[path = "chat_request.rs"]
mod chat_request;
#[path = "host_only.rs"]
mod host_only;
#[path = "multi_host.rs"]
mod multi_host;
#[path = "flags.rs"]
mod flags;

#[derive(Clone, Serialize, Deserialize)]
struct Body {
    group: String,
    doc: String,
    author: String,
    device: [u8; 32],
    peer: u64,
    sequence: u64,
    previous: Option<String>,
    dependencies: BTreeSet<String>,
    permit: Option<Grant>,
    text: String,
    update: Vec<u8>,
}
#[derive(Clone)]
struct Envelope {
    body: Body,
    attestation: proto::SignedAttestation,
    signature: [u8; 64],
}
impl Envelope {
    fn bytes(&self) -> Vec<u8> {
        [DOMAIN, &serde_json::to_vec(&self.body).unwrap()].concat()
    }
    fn id(&self) -> String {
        blake3::hash(&self.bytes()).to_hex().to_string()
    }
    fn key(&self) -> String {
        format!("{}:{}", self.body.peer, self.body.sequence)
    }
    fn value(&self) -> String {
        serde_json::to_string(&(&self.body.author, &self.body.text)).unwrap()
    }
}
// Provisional author adapter: peer IDs bind signing key + document, never a routing hint.
fn peer(key: &[u8; 32], doc: &str) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(DOMAIN);
    h.update(key);
    h.update(doc.as_bytes());
    u64::from_le_bytes(h.finalize().as_bytes()[..8].try_into().unwrap())
}
struct Author {
    id: Identity,
    key: SigningKey,
    attestation: proto::SignedAttestation,
    group: String,
    doc_name: String,
    doc: LoroDoc,
    sequence: u64,
    previous: Option<String>,
    observed: BTreeSet<String>,
    permit: Option<Grant>,
}
impl Author {
    fn new(seed: u8, group: &str) -> Self {
        let mnemonic = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
        let id = identity::recover(&mnemonic.to_string()).unwrap();
        let key = SigningKey::from_bytes(&[seed; 32]);
        let doc_name = format!("group/{group}/2026-10-01");
        let doc = LoroDoc::new();
        doc.set_peer_id(peer(&key.verifying_key().to_bytes(), &doc_name))
            .unwrap();
        let attestation = proto::attest(&id, key.verifying_key().to_bytes(), 0);
        Self {
            id,
            key,
            attestation,
            group: group.into(),
            doc_name,
            doc,
            sequence: 0,
            previous: None,
            observed: BTreeSet::new(),
            permit: None,
        }
    }
    fn configure(&mut self, group: &str, permit: Grant) {
        assert_eq!(self.sequence, 0);
        self.group = group.into();
        self.doc_name = format!("group/{group}/2026-10-01");
        self.doc = LoroDoc::new();
        self.doc
            .set_peer_id(peer(&self.key.verifying_key().to_bytes(), &self.doc_name))
            .unwrap();
        self.permit = Some(permit);
    }
    // Fixture-only: caller supplies an already validated original envelope.
    fn observe(&mut self, e: &Envelope) {
        let status = self.doc.import(&e.body.update).unwrap();
        assert!(status.pending.is_none());
        self.observed.insert(e.id());
        self.observed.extend(e.body.dependencies.iter().cloned());
        if let Some(p) = &e.body.previous {
            self.observed.insert(p.clone());
        }
    }
    fn post(&mut self, text: &str) -> Envelope {
        let before = self.doc.oplog_vv();
        self.sequence += 1;
        let p = peer(&self.key.verifying_key().to_bytes(), &self.doc_name);
        let value = serde_json::to_string(&(self.id.did(), text)).unwrap();
        self.doc
            .get_map("messages")
            .insert(&format!("{p}:{}", self.sequence), value)
            .unwrap();
        self.doc.commit();
        let mut e = Envelope {
            body: Body {
                group: self.group.clone(),
                doc: self.doc_name.clone(),
                author: self.id.did().into(),
                device: self.key.verifying_key().to_bytes(),
                peer: p,
                sequence: self.sequence,
                previous: self.previous.clone(),
                dependencies: self.observed.clone(),
                permit: self.permit.clone(),
                text: text.into(),
                update: self.doc.export(ExportMode::updates(&before)).unwrap(),
            },
            attestation: self.attestation.clone(),
            signature: [0; 64],
        };
        e.signature = self.key.sign(&e.bytes()).to_bytes();
        self.previous = Some(e.id());
        e
    }
}
#[derive(Debug, PartialEq)]
enum Outcome {
    Accepted,
    Duplicate,
    Pending,
    Invalid,
    Limit,
}
struct FixtureAllowlist(BTreeSet<String>);
impl PostAuthorization for FixtureAllowlist {
    fn allows_post(&self, author: &str, _permit: Option<&Grant>) -> bool {
        self.0.contains(author)
    }
}
struct Replica {
    group: String,
    doc_name: String,
    authorization: Box<dyn PostAuthorization>,
    doc: LoroDoc,
    accepted: BTreeMap<String, Envelope>,
    pending: BTreeMap<String, Envelope>,
    fanout: Vec<String>,
}
impl Replica {
    fn new(group: &str, authors: &[&Author]) -> Self {
        Self {
            group: group.into(),
            doc_name: format!("group/{group}/2026-10-01"),
            authorization: Box::new(FixtureAllowlist(
                authors.iter().map(|a| a.id.did().to_string()).collect(),
            )),
            doc: LoroDoc::new(),
            accepted: BTreeMap::new(),
            pending: BTreeMap::new(),
            fanout: vec![],
        }
    }
    fn from_genesis(g: Genesis, expected_group: &str) -> Option<Self> {
        let authority = SignedGroupAuthority::verified(g, expected_group)?;
        let mut r = Self::new(expected_group, &[]);
        r.authorization = Box::new(authority);
        Some(r)
    }
    fn authorized(&self, e: &Envelope) -> bool {
        self.authorization
            .allows_post(&e.body.author, e.body.permit.as_ref())
    }
    fn dependencies_ready(&self, e: &Envelope) -> bool {
        e.body
            .previous
            .iter()
            .chain(e.body.dependencies.iter())
            .all(|p| self.accepted.contains_key(p))
    }
    fn valid_proof(&self, e: &Envelope) -> bool {
        let b = &e.body;
        if b.group != self.group
            || b.doc != self.doc_name
            || !self.authorized(e)
            || b.dependencies.len() > 256
            || b.sequence == 0
            || b.text.len() > 20_000
            || b.update.len() > 1_000_000
            || b.peer != peer(&b.device, &b.doc)
        {
            return false;
        }
        let Ok(a) = proto::verify_attestation(&e.attestation) else {
            return false;
        };
        if a.did != b.author || a.device_key().ok() != Some(b.device) {
            return false;
        }
        let Ok(k) = VerifyingKey::from_bytes(&b.device) else {
            return false;
        };
        k.verify_strict(&e.bytes(), &Signature::from_bytes(&e.signature))
            .is_ok()
    }
    fn receive(&mut self, e: Envelope) -> Outcome {
        if !self.valid_proof(&e) {
            return Outcome::Invalid;
        }
        let id = e.id();
        if self.accepted.contains_key(&id) {
            return Outcome::Duplicate;
        }
        if !self.dependencies_ready(&e) {
            if self.pending.contains_key(&id) {
                return Outcome::Pending;
            }
            if self.pending.len() >= MAX_PENDING {
                return Outcome::Limit;
            }
            self.pending.insert(id, e);
            return Outcome::Pending;
        }
        let result = self.apply(e);
        if result == Outcome::Accepted {
            loop {
                let ready = self
                    .pending
                    .iter()
                    .find(|(_, e)| self.dependencies_ready(e))
                    .map(|(id, _)| id.clone());
                let Some(id) = ready else {
                    break;
                };
                let next = self.pending.remove(&id).unwrap();
                self.apply(next);
            }
        }
        result
    }
    fn apply(&mut self, e: Envelope) -> Outcome {
        // Signed predecessor/dependency IDs must be available before scratch importing.
        match &e.body.previous {
            None if e.body.sequence != 1 => return Outcome::Invalid,
            Some(prev) => {
                let Some(p) = self.accepted.get(prev) else {
                    return Outcome::Invalid;
                };
                if p.body.device != e.body.device || p.body.sequence + 1 != e.body.sequence {
                    return Outcome::Invalid;
                }
            }
            _ => {}
        }
        if self.accepted.values().any(|a| a.key() == e.key()) {
            return Outcome::Invalid;
        }
        let Ok(meta) = LoroDoc::decode_import_blob_meta(&e.body.update, true) else {
            return Outcome::Invalid;
        };
        if meta.partial_end_vv.keys().any(|p| *p != e.body.peer) {
            return Outcome::Invalid;
        }
        // Validate actual Loro causality against the declared closure, not incidental
        // receiver knowledge. An omitted prerequisite must fail in every delivery order.
        let mut closure = BTreeSet::new();
        let mut todo: Vec<String> = e
            .body
            .previous
            .iter()
            .chain(e.body.dependencies.iter())
            .cloned()
            .collect();
        while let Some(id) = todo.pop() {
            if !closure.insert(id.clone()) {
                continue;
            }
            let Some(dep) = self.accepted.get(&id) else {
                return Outcome::Invalid;
            };
            todo.extend(
                dep.body
                    .previous
                    .iter()
                    .chain(dep.body.dependencies.iter())
                    .cloned(),
            );
        }
        let causal = LoroDoc::new();
        for id in &closure {
            if causal.import(&self.accepted[id].body.update).is_err() {
                return Outcome::Invalid;
            }
        }
        let Ok(status) = causal.import(&e.body.update) else {
            return Outcome::Invalid;
        };
        if status.pending.is_some() {
            return Outcome::Invalid;
        }
        let expected_causal = LoroDoc::new();
        let map = expected_causal.get_map("messages");
        for dep in closure
            .iter()
            .map(|id| &self.accepted[id])
            .chain(std::iter::once(&e))
        {
            map.insert(&dep.key(), dep.value()).unwrap();
        }
        if causal.get_deep_value() != expected_causal.get_deep_value() {
            return Outcome::Invalid;
        }

        let scratch = self.doc.fork();
        let Ok(status) = scratch.import(&e.body.update) else {
            return Outcome::Invalid;
        };
        if status.pending.is_some() {
            return Outcome::Invalid;
        }
        let expected = LoroDoc::new();
        let map = expected.get_map("messages");
        for a in self.accepted.values().chain(std::iter::once(&e)) {
            map.insert(&a.key(), a.value()).unwrap();
        }
        if scratch.get_deep_value() != expected.get_deep_value() {
            return Outcome::Invalid;
        }
        let id = e.id();
        self.doc = scratch;
        self.accepted.insert(id.clone(), e);
        self.fanout.push(id);
        Outcome::Accepted
    }
    // Complete-set repair in causal order (as a VV/frontier exchange would send). G-H fix:
    // ID order overflowed MAX_PENDING for >16-deep chains and silently left gaps.
    fn sync_from(&mut self, other: &Self) {
        for e in causal_order(other.accepted.values().cloned().collect()) {
            self.receive(e);
        }
    }
    fn assert_same(&self, other: &Self) {
        assert_eq!(self.doc.get_deep_value(), other.doc.get_deep_value());
        assert_eq!(self.doc.oplog_vv(), other.doc.oplog_vv());
        assert_eq!(
            self.accepted.keys().collect::<Vec<_>>(),
            other.accepted.keys().collect::<Vec<_>>()
        );
        assert!(self.pending.is_empty() && other.pending.is_empty());
    }
    // Byte-level state used by rejection tests: Loro export bytes, deep value, VV, and the
    // signed bytes/signature/attestation of every accepted and pending envelope, plus fanout.
    fn snapshot(&self) -> Snapshot {
        let envelope = |(id, e): (&String, &Envelope)| {
            (
                id.clone(),
                e.bytes(),
                e.signature.to_vec(),
                serde_json::to_vec(&e.attestation).unwrap(),
            )
        };
        Snapshot {
            export: self.doc.export(ExportMode::all_updates()).unwrap(),
            value: format!("{:?}", self.doc.get_deep_value()),
            vv: format!("{:?}", self.doc.oplog_vv()),
            accepted: self.accepted.iter().map(envelope).collect(),
            pending: self.pending.iter().map(envelope).collect(),
            fanout: self.fanout.clone(),
        }
    }
    // Rejection must be an Invalid outcome AND leave the replica byte-for-byte unchanged.
    fn assert_rejected_unchanged(&mut self, e: Envelope) {
        let before = self.snapshot();
        assert_eq!(self.receive(e), Outcome::Invalid);
        assert_eq!(self.snapshot(), before, "rejection mutated replica state");
    }
}
#[derive(Debug, PartialEq)]
struct Snapshot {
    export: Vec<u8>,
    value: String,
    vv: String,
    accepted: Vec<(String, Vec<u8>, Vec<u8>, Vec<u8>)>,
    pending: Vec<(String, Vec<u8>, Vec<u8>, Vec<u8>)>,
    fanout: Vec<String>,
}
// Populated allowlist replica: accepted cross-author history (a1, b1 saw a1) plus one
// pending envelope (a3, waiting for withheld a2). Returns the withheld a2.
fn populated_replica(a: &mut Author, b: &mut Author, extra: &[&Author]) -> (Replica, Envelope) {
    let mut members = vec![&*a, &*b];
    members.extend_from_slice(extra);
    let mut r = Replica::new("g", &members);
    let a1 = a.post("base a1");
    b.observe(&a1);
    let b1 = b.post("base b1 saw a1");
    let a2 = a.post("base a2 withheld");
    let a3 = a.post("base a3 pending");
    assert_eq!(r.receive(a1), Outcome::Accepted);
    assert_eq!(r.receive(b1), Outcome::Accepted);
    assert_eq!(r.receive(a3), Outcome::Pending);
    assert_eq!(
        (r.accepted.len(), r.pending.len(), r.fanout.len()),
        (2, 1, 2)
    );
    (r, a2)
}
fn resign(e: &mut Envelope, key: &SigningKey) {
    e.signature = key.sign(&e.bytes()).to_bytes();
}
// Topological order over the given envelopes (deps inside the set come first; ties by ID).
fn causal_order(mut rest: Vec<Envelope>) -> Vec<Envelope> {
    rest.sort_by_key(|e| e.id());
    let inside: BTreeSet<String> = rest.iter().map(|e| e.id()).collect();
    let mut done = BTreeSet::new();
    let mut out = vec![];
    while !rest.is_empty() {
        let i = rest
            .iter()
            .position(|e| {
                e.body
                    .previous
                    .iter()
                    .chain(e.body.dependencies.iter())
                    .all(|d| !inside.contains(d) || done.contains(d))
            })
            .expect("dependency cycle");
        let e = rest.remove(i);
        done.insert(e.id());
        out.push(e);
    }
    out
}

// KIND: invariant
#[test]
fn original_author_survives_host_relay_and_offline_delivery() {
    let mut a = Author::new(1, "g");
    let b = Author::new(2, "g");
    let mut h = Replica::new("g", &[&a, &b]);
    let mut r = Replica::new("g", &[&a, &b]);
    let e = a.post("hello");
    assert_eq!(h.receive(e.clone()), Outcome::Accepted);
    r.sync_from(&h);
    h.assert_same(&r);
    assert_eq!(r.accepted[&e.id()].body.author, a.id.did());
    assert_eq!(r.accepted[&e.id()].signature, e.signature);
}
// KIND: invariant
#[test]
fn all_small_delivery_permutations_converge_with_duplicates() {
    let mut a = Author::new(1, "g");
    let mut b = Author::new(2, "g");
    let es = [a.post("a1"), a.post("a2"), b.post("b1")];
    let mut oracle = Replica::new("g", &[&a, &b]);
    for e in &es {
        assert_eq!(oracle.receive(e.clone()), Outcome::Accepted);
    }
    let mut duplicates = 0;
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut r = Replica::new("g", &[&a, &b]);
        for i in order {
            let first = r.receive(es[i].clone());
            let second = r.receive(es[i].clone());
            // A redelivery is Duplicate once accepted, or stays Pending (not re-queued).
            match first {
                Outcome::Accepted => assert_eq!(second, Outcome::Duplicate),
                Outcome::Pending => assert_eq!(second, Outcome::Pending),
                other => panic!("valid envelope gave {other:?}"),
            }
            duplicates += (second == Outcome::Duplicate) as usize;
        }
        oracle.assert_same(&r);
        assert_eq!(r.fanout.len(), 3);
    }
    assert!(duplicates > 0, "duplicate path never exercised");
}
// KIND: invariant
#[test]
fn missing_dependency_is_pending_not_converged() {
    let mut a = Author::new(1, "g");
    let first = a.post("first");
    let second = a.post("second");
    let mut r = Replica::new("g", &[&a]);
    assert_eq!(r.receive(second), Outcome::Pending);
    assert!(r.accepted.is_empty() && r.fanout.is_empty());
    assert_eq!(r.receive(first), Outcome::Accepted);
    assert_eq!(r.accepted.len(), 2);
    assert!(r.pending.is_empty());
}
// KIND: invariant
#[test]
fn forged_tampered_cross_group_and_outsider_inputs_leave_no_state() {
    let mut a = Author::new(1, "g");
    let mut x = Author::new(3, "g");
    let good = a.post("good");
    let mut tampered = good.clone();
    tampered.body.text = "forged".into();
    let mut wrong_did = good.clone();
    wrong_did.body.author = x.id.did().into();
    let outsider = x.post("outside");
    let mut r = Replica::new("g", &[&a]);
    for e in [tampered, wrong_did, outsider] {
        assert_eq!(r.receive(e), Outcome::Invalid);
    }
    assert!(r.accepted.is_empty() && r.pending.is_empty() && r.fanout.is_empty());
    let mut other = Replica::new("other", &[&a]);
    assert_eq!(other.receive(good.clone()), Outcome::Invalid);
    assert!(other.accepted.is_empty() && other.pending.is_empty() && other.fanout.is_empty());
    // Positive control: the same envelope is valid for its own group.
    assert_eq!(r.receive(good), Outcome::Accepted);
}
// KIND: invariant
#[test]
fn malicious_author_cannot_hide_extra_loro_mutations_in_signed_update() {
    let mut a = Author::new(1, "g");
    let mut e = a.post("ok");
    a.doc
        .get_map("secret")
        .insert("unexpected", "injection")
        .unwrap();
    a.doc.commit();
    e.body.update = a
        .doc
        .export(ExportMode::updates(&Default::default()))
        .unwrap();
    e.signature = a.key.sign(&e.bytes()).to_bytes();
    let mut r = Replica::new("g", &[&a]);
    assert_eq!(r.receive(e), Outcome::Invalid);
    assert!(r.accepted.is_empty() && r.fanout.is_empty());
}
// KIND: invariant
#[test]
fn two_hosts_partition_then_reconcile_without_rolling_back_local_writes() {
    let mut a = Author::new(1, "g");
    let mut b = Author::new(2, "g");
    let mut h1 = Replica::new("g", &[&a, &b]);
    let mut h2 = Replica::new("g", &[&a, &b]);
    h1.receive(a.post("left"));
    h2.receive(b.post("right"));
    h1.sync_from(&h2);
    h2.sync_from(&h1);
    h1.assert_same(&h2);
    let stale = Replica::new("g", &[&a, &b]);
    h1.sync_from(&stale);
    assert_eq!(h1.accepted.len(), 2);
    // Non-vacuous stale peer: holds a strict subset (only "left"). Syncing from it must not
    // roll h1 back; syncing to it catches it up.
    let mut stale = Replica::new("g", &[&a, &b]);
    let left = h1
        .accepted
        .values()
        .find(|e| e.body.text == "left")
        .unwrap()
        .clone();
    assert_eq!(stale.receive(left.clone()), Outcome::Accepted);
    let before = h1.snapshot();
    assert_eq!(h1.receive(left), Outcome::Duplicate);
    h1.sync_from(&stale);
    assert_eq!(h1.snapshot(), before);
    stale.sync_from(&h1);
    h1.assert_same(&stale);
    assert_eq!(stale.accepted.len(), 2);
}
// KIND: invariant
#[test]
fn seeded_reordered_loss_and_retry_schedules_converge() {
    let mut duplicates = 0;
    for seed in 1u64..=20 {
        let mut a = Author::new(1, "g");
        let mut b = Author::new(2, "g");
        let mut es = vec![];
        for i in 0..5 {
            es.push(a.post(&format!("a{i}")));
            es.push(b.post(&format!("b{i}")));
        }
        let mut oracle = Replica::new("g", &[&a, &b]);
        let mut r = Replica::new("g", &[&a, &b]);
        for e in &es {
            assert_eq!(oracle.receive(e.clone()), Outcome::Accepted);
        }
        let mut rng = seed;
        for _ in 0..80 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let i = ((rng >> 32) as usize) % es.len();
            if rng % 3 != 0 {
                let outcome = r.receive(es[i].clone());
                assert!(
                    matches!(
                        outcome,
                        Outcome::Accepted | Outcome::Duplicate | Outcome::Pending
                    ),
                    "seed={seed} valid envelope gave {outcome:?}"
                );
                duplicates += (outcome == Outcome::Duplicate) as usize;
            }
        }
        r.sync_from(&oracle);
        oracle.assert_same(&r);
        // Non-vacuity: every envelope accepted exactly once on both sides.
        assert_eq!((oracle.accepted.len(), r.accepted.len()), (10, 10));
        assert_eq!(r.fanout.len(), 10);

        // Negative control: withhold one envelope (one that has a later same-author post)
        // from BOTH the lossy schedule and the repair source. Repair must not converge.
        let withheld = (seed as usize) % 8;
        let source = oracle_without(&oracle, &es[withheld]);
        let mut partial = Replica::new("g", &[&a, &b]);
        let mut rng = seed;
        for _ in 0..80 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let i = ((rng >> 32) as usize) % es.len();
            if rng % 3 != 0 && i != withheld {
                partial.receive(es[i].clone());
            }
        }
        partial.sync_from(&source);
        // Same-author successors of the withheld post: (4 - withheld/2) of them.
        let blocked = 4 - withheld / 2;
        assert_eq!(partial.accepted.len(), 10 - 1 - blocked, "seed={seed}");
        assert_eq!(partial.pending.len(), blocked, "seed={seed}");
        assert!(!partial.accepted.contains_key(&es[withheld].id()));
        assert_ne!(partial.doc.get_deep_value(), oracle.doc.get_deep_value());
        // Releasing the withheld envelope drains pending and converges.
        assert_eq!(partial.receive(es[withheld].clone()), Outcome::Accepted);
        oracle.assert_same(&partial);
    }
    assert!(duplicates > 0, "no Duplicate outcome observed in any seed");
}
// Repair source that omits one envelope: rebuilds from the oracle without it.
fn oracle_without(oracle: &Replica, omit: &Envelope) -> Replica {
    let mut r = Replica::new(&oracle.group, &[]);
    r.authorization = Box::new(FixtureAllowlist(
        oracle
            .accepted
            .values()
            .map(|e| e.body.author.clone())
            .collect(),
    ));
    for e in causal_order(oracle.accepted.values().cloned().collect()) {
        if e.id() != omit.id() {
            r.receive(e);
        }
    }
    r
}
// KIND: invariant
#[test]
fn pending_dependencies_are_bounded() {
    let mut a = Author::new(1, "g");
    let withheld = a.post("withheld");
    let mut r = Replica::new("g", &[&a]);
    for _ in 0..MAX_PENDING {
        assert_eq!(r.receive(a.post("pending")), Outcome::Pending);
    }
    let overflow = a.post("overflow");
    let before = r.snapshot();
    assert_eq!(r.receive(overflow.clone()), Outcome::Limit);
    assert_eq!(r.pending.len(), MAX_PENDING);
    assert!(r.accepted.is_empty());
    // The refused envelope is not retained anywhere: state is byte-identical.
    assert!(!r.pending.contains_key(&overflow.id()));
    assert_eq!(r.snapshot(), before);
    // Releasing the dependency drains all 16 pending; the refused one stays absent
    // until it is resent, i.e. the sender (not the receiver) owns the retry.
    assert_eq!(r.receive(withheld), Outcome::Accepted);
    assert_eq!(r.accepted.len(), MAX_PENDING + 1);
    assert!(r.pending.is_empty());
    assert!(!r.accepted.contains_key(&overflow.id()));
    assert_eq!(r.receive(overflow), Outcome::Accepted);
    assert_eq!(r.accepted.len(), MAX_PENDING + 2);
}

fn signed_fixture() -> (Genesis, Author, Author, Author) {
    let mut a = Author::new(1, "unconfigured");
    let mut b = Author::new(2, "unconfigured");
    let mut c = Author::new(3, "unconfigured");
    let g = Genesis::create(&a.id, [42; 32]);
    let admin = Grant::issue(
        &a.id,
        b.id.did(),
        &g.id(),
        &[Capability::Read, Capability::Post, Capability::Delegate],
        true,
        1,
        None,
    );
    let member = Grant::issue(
        &b.id,
        c.id.did(),
        &g.id(),
        &[Capability::Read, Capability::Post],
        false,
        2,
        Some(admin.clone()),
    );
    let owner = Grant::issue(
        &a.id,
        a.id.did(),
        &g.id(),
        &[Capability::Read, Capability::Post, Capability::Delegate],
        true,
        3,
        None,
    );
    a.configure(&g.id(), owner);
    b.configure(&g.id(), admin);
    c.configure(&g.id(), member);
    (g, a, b, c)
}
// KIND: invariant
#[test]
fn signed_genesis_and_delegated_permit_authorize_original_relay() {
    let (g, _a, _b, mut c) = signed_fixture();
    let mut h = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    let mut recipient = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    let e = c.post("delegated member");
    assert_eq!(h.receive(e.clone()), Outcome::Accepted);
    recipient.sync_from(&h);
    h.assert_same(&recipient);
    assert_eq!(recipient.accepted[&e.id()].body.author, c.id.did());
    let mut forged = g.clone();
    forged.signature[0] ^= 1;
    assert!(Replica::from_genesis(forged, &g.id()).is_none());
}
// KIND: invariant
#[test]
fn missing_wrong_holder_tampered_and_read_only_permits_cannot_post() {
    let (g, a, _b, mut c) = signed_fixture();
    let original = c.post("attempt");
    let mut inputs = Vec::new();
    let mut e = original.clone();
    e.body.permit = None;
    inputs.push(e);
    let mut e = original.clone();
    e.body.permit.as_mut().unwrap().signature[0] ^= 1;
    inputs.push(e);
    let mut e = original.clone();
    e.body.permit = a.permit.clone();
    inputs.push(e);
    let mut e = original.clone();
    e.body.permit = Some(Grant::issue(
        &a.id,
        c.id.did(),
        &g.id(),
        &[Capability::Read],
        false,
        20,
        None,
    ));
    inputs.push(e);
    let mut e = original.clone();
    e.body.permit = Some(Grant::issue(
        &a.id,
        c.id.did(),
        "other",
        &[Capability::Post],
        false,
        21,
        None,
    ));
    inputs.push(e);
    let mut r = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    for mut e in inputs {
        // Author re-signs: failures must come from authorization, not a stale outer signature.
        e.signature = c.key.sign(&e.bytes()).to_bytes();
        assert_eq!(r.receive(e), Outcome::Invalid);
        assert!(r.accepted.is_empty() && r.pending.is_empty() && r.fanout.is_empty());
    }
}
// KIND: invariant
#[test]
fn cross_author_causal_chain_converges_in_every_delivery_order() {
    let (g, mut a, mut b, mut c) = signed_fixture();
    let first = a.post("A");
    b.observe(&first);
    let second = b.post("B saw A");
    c.observe(&first);
    c.observe(&second);
    let third = c.post("C saw both");
    let es = [first, second, third];
    let mut oracle = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    for e in &es {
        assert_eq!(oracle.receive(e.clone()), Outcome::Accepted);
    }
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut r = Replica::from_genesis(g.clone(), &g.id()).unwrap();
        for i in order {
            r.receive(es[i].clone());
        }
        oracle.assert_same(&r);
        assert_eq!(r.fanout.len(), 3);
    }
    let mut incomplete = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    assert_eq!(incomplete.receive(es[2].clone()), Outcome::Pending);
    assert_eq!(incomplete.receive(es[1].clone()), Outcome::Pending);
    assert!(incomplete.accepted.is_empty() && incomplete.fanout.is_empty());
    assert_eq!(incomplete.receive(es[0].clone()), Outcome::Accepted);
    oracle.assert_same(&incomplete);
}
// KIND: invariant
#[test]
fn independent_posts_and_cross_author_join_survive_host_partition() {
    let (g, mut a, mut b, mut c) = signed_fixture();
    let first = a.post("left");
    let second = b.post("right");
    c.observe(&first);
    c.observe(&second);
    let join = c.post("both");
    let mut h1 = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    let mut h2 = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    assert_eq!(h1.receive(first), Outcome::Accepted);
    assert_eq!(h2.receive(second), Outcome::Accepted);
    assert_eq!(h1.receive(join), Outcome::Pending);
    h1.sync_from(&h2);
    h2.sync_from(&h1);
    h1.assert_same(&h2);
    assert_eq!(h1.accepted.len(), 3);
}
// KIND: invariant
#[test]
fn declared_dependency_cannot_substitute_for_actual_loro_causality() {
    let (g, mut a, mut b, _c) = signed_fixture();
    let first = a.post("needed");
    b.observe(&first);
    let mut next = b.post("dependent");
    next.body.dependencies.clear();
    next.signature = b.key.sign(&next.bytes()).to_bytes();
    let mut r = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    assert_eq!(r.receive(next.clone()), Outcome::Invalid);
    assert!(r.accepted.is_empty() && r.pending.is_empty() && r.fanout.is_empty());
    assert_eq!(r.receive(first), Outcome::Accepted);
    assert_eq!(r.receive(next), Outcome::Invalid);
    assert_eq!(r.accepted.len(), 1);
    assert_eq!(r.fanout.len(), 1);
}

// ---- G-H hardening: single-cause rejections on POPULATED replicas -------------------

// KIND: invariant
#[test]
fn attestation_must_bind_author_did_and_device_key_each_as_sole_cause() {
    let mut a = Author::new(1, "g");
    let mut b = Author::new(2, "g");
    let mut x = Author::new(3, "g");
    let (mut r, _) = populated_replica(&mut a, &mut b, &[&x]);
    // X is allowlisted, signs with its own key, and its peer/doc/group all match, so the
    // attestation check is the only failing check in each case below.
    let e = x.post("x1");
    let mut did_mismatch = e.clone();
    did_mismatch.attestation = proto::attest(&a.id, x.key.verifying_key().to_bytes(), 0);
    r.assert_rejected_unchanged(did_mismatch);
    let mut key_mismatch = e.clone();
    key_mismatch.attestation = proto::attest(&x.id, a.key.verifying_key().to_bytes(), 0);
    r.assert_rejected_unchanged(key_mismatch);
    let mut forged = e.clone();
    forged.attestation.signature = a.attestation.signature.clone();
    r.assert_rejected_unchanged(forged);
    // Positive control: the untouched envelope is accepted on the same replica.
    assert_eq!(r.receive(e), Outcome::Accepted);
    assert_eq!(r.accepted.len(), 3);
}

// KIND: invariant
#[test]
fn group_only_and_doc_only_mismatches_are_rejected_without_state_change() {
    let mut a = Author::new(1, "g");
    let mut b = Author::new(2, "g");
    // X keeps the "g" document name but signs group "other"; Y keeps group "g" but writes
    // a different document shard (with a consistent peer ID for that shard).
    let mut x = Author::new(3, "g");
    x.group = "other".into();
    let mut y = Author::new(4, "g");
    y.doc_name = "group/g/2026-10-02".into();
    y.doc = LoroDoc::new();
    y.doc
        .set_peer_id(peer(&y.key.verifying_key().to_bytes(), &y.doc_name))
        .unwrap();
    let (mut r, _) = populated_replica(&mut a, &mut b, &[&x, &y]);
    r.assert_rejected_unchanged(x.post("group mismatch only"));
    r.assert_rejected_unchanged(y.post("doc mismatch only"));
    // Positive controls: same keys, matching group and doc.
    assert_eq!(
        r.receive(Author::new(3, "g").post("x ok")),
        Outcome::Accepted
    );
    assert_eq!(
        r.receive(Author::new(4, "g").post("y ok")),
        Outcome::Accepted
    );
}

// KIND: invariant
#[test]
fn size_bounds_and_sequence_zero_are_rejected_without_state_change() {
    let mut a = Author::new(1, "g");
    let mut b = Author::new(2, "g");
    let mut long = Author::new(5, "g");
    let mut deps = Author::new(6, "g");
    let mut zero = Author::new(7, "g");
    let mut big = Author::new(8, "g");
    let mut ok = Author::new(9, "g");
    let (mut r, _) = populated_replica(&mut a, &mut b, &[&long, &deps, &zero, &big, &ok]);
    // Text bound: sole cause (the update itself stays well under 1 MB).
    r.assert_rejected_unchanged(long.post(&"x".repeat(20_001)));
    assert_eq!(r.receive(ok.post(&"x".repeat(20_000))), Outcome::Accepted);
    // Dependency-count bound: sole cause. Without it, the envelope would go Pending.
    let fake = |n: usize| (0..n).map(|i| format!("missing-{i}")).collect();
    let mut e = deps.post("too many deps");
    e.body.dependencies = fake(257);
    resign(&mut e, &deps.key);
    r.assert_rejected_unchanged(e.clone());
    // Sequence zero and oversized update: rejected, but defense in depth only; apply()
    // would also refuse them (sequence/previous rule, Loro decode), so not sole cause.
    let mut e0 = zero.post("zero");
    e0.body.sequence = 0;
    resign(&mut e0, &zero.key);
    r.assert_rejected_unchanged(e0);
    let mut e1 = big.post("big");
    e1.body.update = vec![0; 1_000_001];
    resign(&mut e1, &big.key);
    r.assert_rejected_unchanged(e1);
    // Control for the dependency bound: 256 unknown deps is admissible but not ready.
    e.body.dependencies = fake(256);
    resign(&mut e, &deps.key);
    assert_eq!(r.receive(e), Outcome::Pending);
}

// Hand-built envelope from DID `owner` using an additional device key, writing under
// Loro peer `writer` (normally peer(device, doc)). First multi-device-per-DID fixture.
fn second_device_envelope(
    owner: &Author,
    device: &SigningKey,
    writer: u64,
    text: &str,
) -> Envelope {
    let doc = LoroDoc::new();
    doc.set_peer_id(writer).unwrap();
    let value = serde_json::to_string(&(owner.id.did(), text)).unwrap();
    doc.get_map("messages")
        .insert(&format!("{writer}:1"), value)
        .unwrap();
    doc.commit();
    let mut e = Envelope {
        body: Body {
            group: owner.group.clone(),
            doc: owner.doc_name.clone(),
            author: owner.id.did().into(),
            device: device.verifying_key().to_bytes(),
            peer: writer,
            sequence: 1,
            previous: None,
            dependencies: BTreeSet::new(),
            permit: None,
            text: text.into(),
            update: doc.export(ExportMode::all_updates()).unwrap(),
        },
        attestation: proto::attest(&owner.id, device.verifying_key().to_bytes(), 0),
        signature: [0; 64],
    };
    resign(&mut e, device);
    e
}

// KIND: invariant
#[test]
fn second_device_cannot_write_under_another_devices_loro_peer_id() {
    let mut a = Author::new(1, "g");
    let mut b = Author::new(2, "g");
    let x = Author::new(3, "g");
    let (mut r, _) = populated_replica(&mut a, &mut b, &[&x]);
    // A's second device (validly attested by A's DID) claims X's writer peer. The update's
    // ops really are from that peer, so only the peer = H(device, doc) binding catches it.
    let a2 = SigningKey::from_bytes(&[9; 32]);
    let victim = peer(&x.key.verifying_key().to_bytes(), &a.doc_name);
    r.assert_rejected_unchanged(second_device_envelope(&a, &a2, victim, "impersonate"));
    // Positive control: the same second device under its own bound peer is accepted.
    let own = peer(&a2.verifying_key().to_bytes(), &a.doc_name);
    let e = second_device_envelope(&a, &a2, own, "second device");
    assert_eq!(r.receive(e), Outcome::Accepted);
    assert_eq!(r.accepted.len(), 3);
}

// KIND: invariant
#[test]
fn every_rejection_class_leaves_populated_signed_replica_byte_identical() {
    let (g, mut a, mut b, mut c) = signed_fixture();
    let mut r = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    let a1 = a.post("A");
    b.observe(&a1);
    assert_eq!(r.receive(a1), Outcome::Accepted);
    assert_eq!(r.receive(b.post("B saw A")), Outcome::Accepted);
    let withheld = b.post("withheld");
    assert_eq!(r.receive(b.post("pending on withheld")), Outcome::Pending);
    let original = c.post("attempt");
    let mut inputs = vec![];
    let mut e = original.clone();
    e.body.text = "tampered, not re-signed".into();
    inputs.push(e);
    for permit in [
        None,
        a.permit.clone(),
        Some(Grant::issue(
            &a.id,
            c.id.did(),
            &g.id(),
            &[Capability::Read],
            false,
            20,
            None,
        )),
        Some(Grant::issue(
            &a.id,
            c.id.did(),
            "other",
            &[Capability::Post],
            false,
            21,
            None,
        )),
    ] {
        let mut e = original.clone();
        e.body.permit = permit;
        resign(&mut e, &c.key);
        inputs.push(e);
    }
    let mut e = original.clone();
    c.doc.get_map("secret").insert("x", "injection").unwrap();
    c.doc.commit();
    e.body.update = c
        .doc
        .export(ExportMode::updates(&Default::default()))
        .unwrap();
    resign(&mut e, &c.key);
    inputs.push(e);
    for e in inputs {
        r.assert_rejected_unchanged(e);
    }
    // Positive control, then the pending envelope drains when its dependency arrives.
    assert_eq!(r.receive(original), Outcome::Accepted);
    assert_eq!(r.receive(withheld), Outcome::Accepted);
    assert_eq!((r.accepted.len(), r.pending.len()), (5, 0));
}

// KIND: limitation
#[test]
fn invalid_envelope_promoted_from_pending_is_dropped_without_surfaced_outcome() {
    let mut a = Author::new(1, "g");
    let mut b = Author::new(2, "g");
    let mut c = Author::new(3, "g");
    let a1 = a.post("a1");
    let c1 = c.post("c1");
    b.observe(&a1);
    // b1's Loro ops depend on a1, but it declares only c1: admissible-but-wrong closure.
    let mut b1 = b.post("b1 needs a1");
    b1.body.dependencies = [c1.id()].into();
    resign(&mut b1, &b.key);
    let mut r = Replica::new("g", &[&a, &b, &c]);
    assert_eq!(r.receive(b1.clone()), Outcome::Pending);
    assert_eq!(r.receive(a1), Outcome::Accepted);
    assert!(r.pending.contains_key(&b1.id()));
    // c1 makes b1 "ready"; apply() refuses it during the drain and the result is discarded.
    // The caller sees only c1's Accepted: no Invalid outcome is surfaced for b1.
    assert_eq!(r.receive(c1), Outcome::Accepted);
    assert!(!r.accepted.contains_key(&b1.id()));
    assert!(r.pending.is_empty());
    assert_eq!(r.fanout.len(), 2);
    assert_eq!(r.receive(b1), Outcome::Invalid);
}

// KIND: limitation
#[test]
fn bulk_repair_in_id_order_overflows_pending_bound_but_causal_order_does_not() {
    // Before G-H, Replica::sync_from and Fleet::reconcile sent missing envelopes in
    // envelope-ID (BTreeMap) order. A 60-post chain then hits MAX_PENDING and valid
    // envelopes are refused with Limit; the receiver does not retain them.
    let mut a = Author::new(1, "g");
    let chain: Vec<Envelope> = (0..60).map(|i| a.post(&format!("c{i}"))).collect();
    let mut by_id = chain.clone();
    by_id.sort_by_key(|e| e.id());
    let mut r = Replica::new("g", &[&a]);
    let limits = by_id
        .into_iter()
        .filter(|e| r.receive(e.clone()) == Outcome::Limit)
        .count();
    assert!(
        limits > 0,
        "id-order repair no longer reaches the pending bound"
    );
    assert!(r.accepted.len() < chain.len());
    // Fixed repair order (used by sync_from and Fleet::reconcile): no Pending, no Limit.
    let mut fixed = Replica::new("g", &[&a]);
    for e in causal_order(chain.clone()) {
        assert_eq!(fixed.receive(e), Outcome::Accepted);
    }
    let mut source = Replica::new("g", &[&a]);
    for e in &chain {
        source.receive(e.clone());
    }
    let mut synced = Replica::new("g", &[&a]);
    synced.sync_from(&source);
    source.assert_same(&synced);
    assert_eq!(synced.accepted.len(), 60);
}
