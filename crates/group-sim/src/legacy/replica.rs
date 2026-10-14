use super::*;

#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    Accepted,
    Duplicate,
    Pending,
    Invalid,
    Limit,
}
pub(crate) struct FixtureAllowlist(pub(crate) BTreeSet<String>);
impl PostAuthorization for FixtureAllowlist {
    fn allows_post(&self, author: &str, _permit: Option<&Grant>) -> bool {
        self.0.contains(author)
    }
}
pub(crate) struct Replica {
    pub(crate) group: String,
    pub(crate) doc_name: String,
    pub(crate) authorization: Box<dyn PostAuthorization>,
    pub(crate) doc: LoroDoc,
    pub(crate) accepted: BTreeMap<String, Envelope>,
    pub(crate) pending: BTreeMap<String, Envelope>,
    pub(crate) fanout: Vec<String>,
}
impl Replica {
    pub(crate) fn new(group: &str, authors: &[&Author]) -> Self {
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
    pub(crate) fn from_genesis(g: Genesis, expected_group: &str) -> Option<Self> {
        let authority = SignedGroupAuthority::verified(g, expected_group)?;
        let mut r = Self::new(expected_group, &[]);
        r.authorization = Box::new(authority);
        Some(r)
    }
    pub(crate) fn authorized(&self, e: &Envelope) -> bool {
        self.authorization
            .allows_post(&e.body.author, e.body.permit.as_ref())
    }
    pub(crate) fn dependencies_ready(&self, e: &Envelope) -> bool {
        e.body
            .previous
            .iter()
            .chain(e.body.dependencies.iter())
            .all(|p| self.accepted.contains_key(p))
    }
    pub(crate) fn valid_proof(&self, e: &Envelope) -> bool {
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
    pub(crate) fn receive(&mut self, e: Envelope) -> Outcome {
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
    pub(crate) fn apply(&mut self, e: Envelope) -> Outcome {
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
    pub(crate) fn sync_from(&mut self, other: &Self) {
        for e in causal_order(other.accepted.values().cloned().collect()) {
            self.receive(e);
        }
    }
    pub(crate) fn assert_same(&self, other: &Self) {
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
    pub(crate) fn snapshot(&self) -> Snapshot {
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
    pub(crate) fn assert_rejected_unchanged(&mut self, e: Envelope) {
        let before = self.snapshot();
        assert_eq!(self.receive(e), Outcome::Invalid);
        assert_eq!(self.snapshot(), before, "rejection mutated replica state");
    }
}
#[derive(Debug, PartialEq)]
pub(crate) struct Snapshot {
    pub(crate) export: Vec<u8>,
    pub(crate) value: String,
    pub(crate) vv: String,
    pub(crate) accepted: Vec<(String, Vec<u8>, Vec<u8>, Vec<u8>)>,
    pub(crate) pending: Vec<(String, Vec<u8>, Vec<u8>, Vec<u8>)>,
    pub(crate) fanout: Vec<String>,
}
// Populated allowlist replica: accepted cross-author history (a1, b1 saw a1) plus one
// pending envelope (a3, waiting for withheld a2). Returns the withheld a2.
pub(crate) fn populated_replica(
    a: &mut Author,
    b: &mut Author,
    extra: &[&Author],
) -> (Replica, Envelope) {
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
