use super::*;

pub(super) const POST_DOMAIN: &[u8] = b"tinline/group-sim/host-only-post/not-production/v1\0";
pub(super) const NOTE_DOMAIN: &[u8] =
    b"tinline/group-sim/host-only-endpoint-note/not-production/v1\0";
pub(super) const CHAT_DOMAIN: &str = "tinline/group-sim/host-only-chat/not-production/v1";
pub(super) const KDF_CONTEXT: &str = "tinline/group-sim/host-only-chat/not-production/v1 aead-key";
pub(super) const GROUP: &str = "w2h-group";
pub(super) const DOC: &str = "group/w2h-group/2026-10-10";
pub(super) const MAX_PENDING_POSTS: usize = 16;
pub(super) const PAD: usize = 1024;

pub(super) const HOST: usize = 0;
pub(super) const A1: usize = 1; // alice, device 1
pub(super) const A2: usize = 2; // alice, device 2 (same mnemonic / DID)
pub(super) const BOB: usize = 3;
pub(super) const CAROL: usize = 4;
// Encodings of a 32-byte key that could appear in anything a receiver holds.
pub(super) fn leaks_key(hay: &[u8], key: &[u8; 32]) -> bool {
    [
        key.to_vec(),
        hex32(key).into_bytes(),
        serde_json::to_vec(key).unwrap(),
    ]
    .iter()
    .any(|n| has(hay, n))
}
pub(super) fn tkey(k: &SigningKey) -> [u8; 32] {
    k.verifying_key().to_bytes()
}
pub(super) fn ep(prefix: &str, k: &SigningKey) -> String {
    format!("{prefix}-{}", hex32(&tkey(k)))
}

// ---- DID-signed content --------------------------------------------------------------------

// Every field a group member ever receives about a post. No device/transport key, no
// attestation, no endpoint (U4 revised).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PostBody {
    pub(super) group: String,
    pub(super) doc: String,
    pub(super) author: String, // DID
    pub(super) writer: u64,    // random per-device writer ID == Loro peer ID
    pub(super) seq: u64,       // per-writer sequence
    pub(super) text: String,
    pub(super) update: Vec<u8>, // Loro update containing only `writer`'s ops
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Post {
    pub(super) body: PostBody,
    pub(super) sig: Vec<u8>, // DID-key Ed25519 signature
}
impl Post {
    pub(super) fn signed_bytes(b: &PostBody) -> Vec<u8> {
        [POST_DOMAIN, &serde_json::to_vec(b).unwrap()].concat()
    }
    pub(super) fn sign(id: &Identity, body: PostBody) -> Self {
        let sig = id.sign(&Self::signed_bytes(&body)).to_vec();
        Self { body, sig }
    }
    pub(super) fn verify(&self) -> bool {
        let Some(key) = public_key_from_did(&self.body.author) else {
            return false;
        };
        let Ok(sig) = <[u8; 64]>::try_from(self.sig.as_slice()) else {
            return false;
        };
        verify(&key, &Self::signed_bytes(&self.body), &sig)
    }
    pub(super) fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
    pub(super) fn id(&self) -> String {
        blake3::hash(&self.wire()).to_hex().to_string()
    }
    pub(super) fn key(&self) -> String {
        format!("{}:{}", self.body.writer, self.body.seq)
    }
    pub(super) fn value(&self) -> String {
        serde_json::to_string(&(&self.body.author, &self.body.text)).unwrap()
    }
}

// Device-side content producer. `writer` is drawn from `SimRng`, never from a key.
pub(super) struct Writer {
    pub(super) id: Identity,
    pub(super) writer: u64,
    pub(super) doc: LoroDoc,
    pub(super) seq: u64,
}
impl Writer {
    pub(super) fn new(id: Identity, writer: u64) -> Self {
        let doc = LoroDoc::new();
        doc.set_peer_id(writer).unwrap();
        Self {
            id,
            writer,
            doc,
            seq: 0,
        }
    }
    // Import everything the device's validated board holds (idempotent).
    pub(super) fn observe(&mut self, board: &Board) {
        for id in &board.order {
            let _ = self.doc.import(&board.accepted[id].body.update);
        }
    }
    pub(super) fn post(&mut self, group: &str, text: &str) -> Post {
        let before = self.doc.oplog_vv();
        self.seq += 1;
        let value = serde_json::to_string(&(self.id.did(), text)).unwrap();
        self.doc
            .get_map("messages")
            .insert(&format!("{}:{}", self.writer, self.seq), value)
            .unwrap();
        self.doc.commit();
        let body = PostBody {
            group: group.into(),
            doc: DOC.into(),
            author: self.id.did().into(),
            writer: self.writer,
            seq: self.seq,
            text: text.into(),
            update: self.doc.export(ExportMode::updates(&before)).unwrap(),
        };
        Post::sign(&self.id, body)
    }
}

// ---- validating board (host and every member run the same checks) -------------------------

// G-FORK candidates for two posts with the same (writer, seq) from the SAME DID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ForkPolicy {
    /// Keep the first post accepted; refuse the later one (host order decides).
    RejectLater,
    /// Refuse the later one AND freeze the writer ID: no further posts under it.
    FreezeWriter,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Why {
    WrongGroup,
    NotMember,
    Bounds,
    BadSignature,
    WriterOwnedByOtherDid,
    WriterFrozen,
    SameDidFork,
    LoroMismatch,
    Limit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    Accepted,
    Duplicate,
    Pending,
    Rejected(Why),
}
pub(super) struct Board {
    pub(super) roster: BTreeSet<String>,
    pub(super) policy: ForkPolicy,
    pub(super) doc: LoroDoc,
    pub(super) accepted: BTreeMap<String, Post>,
    pub(super) pending: BTreeMap<String, Post>,
    pub(super) writers: BTreeMap<u64, String>, // writer ID -> DID that first used it here
    pub(super) last_seq: BTreeMap<u64, u64>,
    pub(super) frozen: BTreeSet<u64>,
    pub(super) order: Vec<String>,
}
impl Board {
    pub(super) fn new(roster: BTreeSet<String>, policy: ForkPolicy) -> Self {
        Self {
            roster,
            policy,
            doc: LoroDoc::new(),
            accepted: BTreeMap::new(),
            pending: BTreeMap::new(),
            writers: BTreeMap::new(),
            last_seq: BTreeMap::new(),
            frozen: BTreeSet::new(),
            order: vec![],
        }
    }
    pub(super) fn receive(&mut self, p: Post) -> Verdict {
        let b = &p.body;
        if b.group != GROUP || b.doc != DOC {
            return Verdict::Rejected(Why::WrongGroup);
        }
        if !self.roster.contains(&b.author) {
            return Verdict::Rejected(Why::NotMember);
        }
        if b.seq == 0 || b.text.len() > 20_000 || b.update.len() > 1_000_000 {
            return Verdict::Rejected(Why::Bounds);
        }
        if !p.verify() {
            return Verdict::Rejected(Why::BadSignature);
        }
        let id = p.id();
        if self.accepted.contains_key(&id) {
            return Verdict::Duplicate;
        }
        if self.pending.contains_key(&id) {
            return Verdict::Pending;
        }
        if let Err(why) = self.check(&p) {
            if why == Why::SameDidFork && self.policy == ForkPolicy::FreezeWriter {
                self.frozen.insert(p.body.writer);
            }
            return Verdict::Rejected(why);
        }
        let last = self.last_seq.get(&p.body.writer).copied().unwrap_or(0);
        if p.body.seq > last + 1 {
            return self.park(id, p);
        }
        match self.apply(p.clone()) {
            Ok(true) => {
                self.drain();
                Verdict::Accepted
            }
            Ok(false) => self.park(id, p),
            Err(why) => Verdict::Rejected(why),
        }
    }
    pub(super) fn park(&mut self, id: String, p: Post) -> Verdict {
        if self.pending.len() >= MAX_PENDING_POSTS {
            return Verdict::Rejected(Why::Limit);
        }
        self.pending.insert(id, p);
        Verdict::Pending
    }
    pub(super) fn check(&self, p: &Post) -> Result<(), Why> {
        let b = &p.body;
        if let Some(owner) = self.writers.get(&b.writer) {
            if *owner != b.author {
                return Err(Why::WriterOwnedByOtherDid);
            }
        }
        if self.frozen.contains(&b.writer) {
            return Err(Why::WriterFrozen);
        }
        if self
            .accepted
            .values()
            .any(|a| a.body.writer == b.writer && a.body.seq == b.seq)
        {
            return Err(Why::SameDidFork);
        }
        let Ok(meta) = LoroDoc::decode_import_blob_meta(&b.update, true) else {
            return Err(Why::LoroMismatch);
        };
        if meta.partial_end_vv.keys().any(|k| *k != b.writer) {
            return Err(Why::LoroMismatch);
        }
        Ok(())
    }
    // Ok(false): Loro reports missing causal deps (other writers' ops) -> caller parks it.
    pub(super) fn apply(&mut self, p: Post) -> Result<bool, Why> {
        let scratch = self.doc.fork();
        let Ok(status) = scratch.import(&p.body.update) else {
            return Err(Why::LoroMismatch);
        };
        if status.pending.is_some() {
            return Ok(false);
        }
        let expected = LoroDoc::new();
        let map = expected.get_map("messages");
        for a in self.accepted.values().chain(std::iter::once(&p)) {
            map.insert(&a.key(), a.value()).unwrap();
        }
        if scratch.get_deep_value() != expected.get_deep_value() {
            return Err(Why::LoroMismatch);
        }
        let id = p.id();
        self.doc = scratch;
        self.writers.insert(p.body.writer, p.body.author.clone());
        self.last_seq.insert(p.body.writer, p.body.seq);
        self.accepted.insert(id.clone(), p);
        self.order.push(id);
        Ok(true)
    }
    pub(super) fn drain(&mut self) {
        loop {
            let mut progressed = false;
            for id in self.pending.keys().cloned().collect::<Vec<_>>() {
                let p = self.pending[&id].clone();
                let last = self.last_seq.get(&p.body.writer).copied().unwrap_or(0);
                if p.body.seq > last + 1 {
                    continue;
                }
                if self.check(&p).is_err() {
                    self.pending.remove(&id);
                    continue;
                }
                match self.apply(p) {
                    Ok(true) => {
                        self.pending.remove(&id);
                        progressed = true;
                    }
                    Ok(false) => {}
                    Err(_) => {
                        self.pending.remove(&id);
                    }
                }
            }
            if !progressed {
                break;
            }
        }
    }
    pub(super) fn content_snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(&(
            self.doc.export(ExportMode::all_updates()).unwrap(),
            format!("{:?}", self.doc.get_deep_value()),
            format!("{:?}", self.doc.oplog_vv()),
            &self.accepted,
            &self.pending,
            &self.writers,
            &self.last_seq,
            &self.order,
        ))
        .unwrap()
    }
    // Full byte-level state for rejection tests.
    pub(super) fn snapshot(&self) -> Vec<u8> {
        [
            self.content_snapshot(),
            serde_json::to_vec(&self.frozen).unwrap(),
        ]
        .concat()
    }
    pub(super) fn texts(&self) -> BTreeSet<String> {
        self.accepted
            .values()
            .map(|p| p.body.text.clone())
            .collect()
    }
}

// ---- DID-signed endpoint notes (sim stand-in for permits; no succession verifier) ---------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Audience {
    Group(String),
    Private,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct NoteClaims {
    pub(super) owner: String,
    pub(super) audience: Audience,
    pub(super) slot: u64, // random per-device route slot (host routing key); not a key, not an address
    pub(super) endpoint: String,
    pub(super) seq: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct EpNote {
    pub(super) claims: NoteClaims,
    pub(super) sig: Vec<u8>,
}
impl EpNote {
    pub(super) fn bytes(c: &NoteClaims) -> Vec<u8> {
        [NOTE_DOMAIN, &serde_json::to_vec(c).unwrap()].concat()
    }
    pub(super) fn sign(
        id: &Identity,
        audience: Audience,
        slot: u64,
        endpoint: &str,
        seq: u64,
    ) -> Self {
        let claims = NoteClaims {
            owner: id.did().into(),
            audience,
            slot,
            endpoint: endpoint.into(),
            seq,
        };
        let sig = id.sign(&Self::bytes(&claims)).to_vec();
        Self { claims, sig }
    }
    pub(super) fn verify(&self) -> bool {
        let Some(key) = public_key_from_did(&self.claims.owner) else {
            return false;
        };
        let Ok(sig) = <[u8; 64]>::try_from(self.sig.as_slice()) else {
            return false;
        };
        verify(&key, &Self::bytes(&self.claims), &sig)
    }
    pub(super) fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NoteReject {
    Malformed,
    BadSignature,
    WrongAudience,
    WrongOwner,
    NotMember,
    Stale,
}

// ---- chat request via host (U3) ------------------------------------------------------------
// Sealing below is COPIED from chat_request.rs @e0a5ce3 (W2-Q) and trimmed; coordinator
// consolidates later. SIMULATION construction, not an approved production scheme (HPKE vs
// Noise is W2-Q question 10). Request keys here are one static X25519 key per DID (W2-Q's
// request-key rotation is not re-modelled).

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ReqKeyClaim {
    pub(super) group: String,
    pub(super) owner: String,
    pub(super) x25519: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SignedReqKey {
    pub(super) claim: ReqKeyClaim,
    pub(super) sig: Vec<u8>,
}
pub(super) fn chat_bytes(purpose: &str, body: &impl Serialize) -> Vec<u8> {
    [
        CHAT_DOMAIN.as_bytes(),
        b"\0",
        purpose.as_bytes(),
        b"\0",
        &serde_json::to_vec(body).unwrap(),
    ]
    .concat()
}
pub(super) fn did_sign(id: &Identity, purpose: &str, body: &impl Serialize) -> Vec<u8> {
    id.sign(&chat_bytes(purpose, body)).to_vec()
}
pub(super) fn did_verify(did: &str, purpose: &str, body: &impl Serialize, sig: &[u8]) -> bool {
    let (Some(public), Ok(sig)) = (public_key_from_did(did), <[u8; 64]>::try_from(sig)) else {
        return false;
    };
    verify(&public, &chat_bytes(purpose, body), &sig)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum ChatKind {
    Request,
    Answer,
}
// Everything the host can read (AEAD associated data).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Header {
    pub(super) group: String,
    pub(super) kind: ChatKind,
    pub(super) from: String,
    pub(super) to: String,
    pub(super) ephemeral: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Sealed {
    pub(super) header: Header,
    pub(super) ciphertext: Vec<u8>,
}
// Request carries NO endpoint; only the acceptance carries the target's private endpoint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Inner {
    Request {
        group: String,
        from: String,
        to: String,
        request_id: String,
        sent_at: u64,
    },
    Accept {
        group: String,
        from: String,
        to: String,
        request_id: String,
        request_digest: String,
        endpoint: String,
    },
}
impl Inner {
    pub(super) fn parties(&self) -> (&str, &str, &str) {
        match self {
            Inner::Request {
                group, from, to, ..
            }
            | Inner::Accept {
                group, from, to, ..
            } => (group, from, to),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SignedInner {
    pub(super) inner: Inner,
    pub(super) sig: Vec<u8>,
}
impl SignedInner {
    pub(super) fn new(id: &Identity, inner: Inner) -> Self {
        let sig = did_sign(id, "chat-inner", &inner);
        Self { inner, sig }
    }
    pub(super) fn verify(&self) -> bool {
        did_verify(self.inner.parties().1, "chat-inner", &self.inner, &self.sig)
    }
    pub(super) fn digest(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).unwrap())
            .to_hex()
            .to_string()
    }
    pub(super) fn padded(&self) -> Vec<u8> {
        let bytes = serde_json::to_vec(self).unwrap();
        assert!(bytes.len() + 4 <= PAD, "fixture payload exceeds pad bucket");
        let mut out = (bytes.len() as u32).to_le_bytes().to_vec();
        out.extend(bytes);
        out.resize(PAD, 0);
        out
    }
}
pub(super) fn unpad(b: &[u8]) -> Option<SignedInner> {
    if b.len() != PAD {
        return None;
    }
    let len = u32::from_le_bytes(b[..4].try_into().unwrap()) as usize;
    serde_json::from_slice(b.get(4..4 + len)?).ok()
}
pub(super) fn aead_key(shared: &[u8; 32], ephemeral: &[u8; 32], recipient: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(KDF_CONTEXT, &[&shared[..], ephemeral, recipient].concat())
}
