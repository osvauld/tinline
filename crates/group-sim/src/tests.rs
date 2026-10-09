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
    fn sync_from(&mut self, other: &Self) {
        for e in other.accepted.values() {
            self.receive(e.clone());
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
}

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
#[test]
fn all_small_delivery_permutations_converge_with_duplicates() {
    let mut a = Author::new(1, "g");
    let mut b = Author::new(2, "g");
    let es = [a.post("a1"), a.post("a2"), b.post("b1")];
    let mut oracle = Replica::new("g", &[&a, &b]);
    for e in &es {
        oracle.receive(e.clone());
    }
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
            r.receive(es[i].clone());
            r.receive(es[i].clone());
        }
        oracle.assert_same(&r);
        assert_eq!(r.fanout.len(), 3);
    }
}
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
    assert_eq!(other.receive(good), Outcome::Invalid);
}
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
}
#[test]
fn seeded_reordered_loss_and_retry_schedules_converge() {
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
            oracle.receive(e.clone());
        }
        let mut rng = seed;
        for _ in 0..80 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let i = ((rng >> 32) as usize) % es.len();
            if rng % 3 != 0 {
                r.receive(es[i].clone());
            }
        }
        r.sync_from(&oracle);
        oracle.assert_same(&r);
    }
}
#[test]
fn pending_dependencies_are_bounded() {
    let mut a = Author::new(1, "g");
    a.post("withheld");
    let mut r = Replica::new("g", &[&a]);
    for _ in 0..MAX_PENDING {
        assert_eq!(r.receive(a.post("pending")), Outcome::Pending);
    }
    assert_eq!(r.receive(a.post("overflow")), Outcome::Limit);
    assert_eq!(r.pending.len(), MAX_PENDING);
    assert!(r.accepted.is_empty());
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
