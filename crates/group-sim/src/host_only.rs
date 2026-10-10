//! W2-H: host-only group model with DID-signed content (user decisions U4 revised, U7).
//!
//! Status: proposal — not approved. Simulation only; no production API, wire format or UI.
//! Parallel model: it does NOT use the `Envelope`/`Replica`/`Author` fixtures in `tests.rs`,
//! which encode the superseded device-key-signed, attestation-carrying, mesh-sync design.
//!
//! Model (test-only):
//! - Content (`Post`) is signed by the member's DID key (`Identity::sign`, real Ed25519) and
//!   carries a random per-device *writer ID* used as the Loro peer ID and per-device sequence
//!   space. Writer IDs come from a seeded SplitMix64 stream (`SimRng`), never from any key.
//!   No device key, transport key, attestation or endpoint is a field of any post.
//! - Topology (U7): only the host holds members' group endpoints (`World::directory`, keyed
//!   by DID + a random per-device route slot). Members hold only the host's DID-signed group
//!   note and their private address book (private notes / accepted chat requests). There is
//!   no field in which a member could store another member's group endpoint.
//! - All group traffic goes member -> host -> members. Host push selects missing posts "as if
//!   a VV exchange had completed" (same idealisation as `scheduler.rs`); every receiver still
//!   verifies every post's DID signature itself.
//! - Endpoints are fresh random fixture keys (`grp-<hex>`, `prv-<hex>`); rotation draws a new
//!   one. Nothing is derived from a previous endpoint (no future-endpoint scheme).
//! - Fake `u64` time, deterministic seeds, in-memory only.
//! - Open policy (writer-ID collisions G-FORK, offline queue bound G-STORAGE, host endpoint
//!   overlap G-TIME) is modelled as parameters; no option is selected.
use super::*;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use identity::{public_key_from_did, verify};
use x25519_dalek::{PublicKey, StaticSecret};

const POST_DOMAIN: &[u8] = b"tinline/group-sim/host-only-post/not-production/v1\0";
const NOTE_DOMAIN: &[u8] = b"tinline/group-sim/host-only-endpoint-note/not-production/v1\0";
const CHAT_DOMAIN: &str = "tinline/group-sim/host-only-chat/not-production/v1";
const KDF_CONTEXT: &str = "tinline/group-sim/host-only-chat/not-production/v1 aead-key";
const GROUP: &str = "w2h-group";
const DOC: &str = "group/w2h-group/2026-10-10";
const MAX_PENDING_POSTS: usize = 16;
const PAD: usize = 1024;

const HOST: usize = 0;
const A1: usize = 1; // alice, device 1
const A2: usize = 2; // alice, device 2 (same mnemonic / DID)
const BOB: usize = 3;
const CAROL: usize = 4;

// ---- helpers -------------------------------------------------------------------------------

fn hex32(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn has(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}
// Encodings of a 32-byte key that could appear in anything a receiver holds.
fn leaks_key(hay: &[u8], key: &[u8; 32]) -> bool {
    [
        key.to_vec(),
        hex32(key).into_bytes(),
        serde_json::to_vec(key).unwrap(),
    ]
    .iter()
    .any(|n| has(hay, n))
}
fn ident(seed: u8) -> Identity {
    let mnemonic = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
    identity::recover(&mnemonic.to_string()).unwrap()
}
fn tkey(k: &SigningKey) -> [u8; 32] {
    k.verifying_key().to_bytes()
}
fn ep(prefix: &str, k: &SigningKey) -> String {
    format!("{prefix}-{}", hex32(&tkey(k)))
}

// Deterministic SplitMix64. The ONLY source of writer IDs, route slots and transport keys.
// Seeded by a fixture constant; never by key material. Not production randomness.
struct SimRng(u64);
impl SimRng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn bytes32(&mut self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for c in out.chunks_mut(8) {
            c.copy_from_slice(&self.next_u64().to_le_bytes());
        }
        out
    }
    // Loro reserves u64::MAX; redraw in that (fixture-irrelevant) case.
    fn writer(&mut self) -> u64 {
        loop {
            let w = self.next_u64();
            if w != u64::MAX {
                return w;
            }
        }
    }
}

// ---- DID-signed content --------------------------------------------------------------------

// Every field a group member ever receives about a post. No device/transport key, no
// attestation, no endpoint (U4 revised).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PostBody {
    group: String,
    doc: String,
    author: String, // DID
    writer: u64,    // random per-device writer ID == Loro peer ID
    seq: u64,       // per-writer sequence
    text: String,
    update: Vec<u8>, // Loro update containing only `writer`'s ops
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Post {
    body: PostBody,
    sig: Vec<u8>, // DID-key Ed25519 signature
}
impl Post {
    fn signed_bytes(b: &PostBody) -> Vec<u8> {
        [POST_DOMAIN, &serde_json::to_vec(b).unwrap()].concat()
    }
    fn sign(id: &Identity, body: PostBody) -> Self {
        let sig = id.sign(&Self::signed_bytes(&body)).to_vec();
        Self { body, sig }
    }
    fn verify(&self) -> bool {
        let Some(key) = public_key_from_did(&self.body.author) else {
            return false;
        };
        let Ok(sig) = <[u8; 64]>::try_from(self.sig.as_slice()) else {
            return false;
        };
        verify(&key, &Self::signed_bytes(&self.body), &sig)
    }
    fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
    fn id(&self) -> String {
        blake3::hash(&self.wire()).to_hex().to_string()
    }
    fn key(&self) -> String {
        format!("{}:{}", self.body.writer, self.body.seq)
    }
    fn value(&self) -> String {
        serde_json::to_string(&(&self.body.author, &self.body.text)).unwrap()
    }
}

// Device-side content producer. `writer` is drawn from `SimRng`, never from a key.
struct Writer {
    id: Identity,
    writer: u64,
    doc: LoroDoc,
    seq: u64,
}
impl Writer {
    fn new(id: Identity, writer: u64) -> Self {
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
    fn observe(&mut self, board: &Board) {
        for id in &board.order {
            let _ = self.doc.import(&board.accepted[id].body.update);
        }
    }
    fn post(&mut self, group: &str, text: &str) -> Post {
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
enum ForkPolicy {
    /// Keep the first post accepted; refuse the later one (host order decides).
    RejectLater,
    /// Refuse the later one AND freeze the writer ID: no further posts under it.
    FreezeWriter,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
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
enum Verdict {
    Accepted,
    Duplicate,
    Pending,
    Rejected(Why),
}
struct Board {
    roster: BTreeSet<String>,
    policy: ForkPolicy,
    doc: LoroDoc,
    accepted: BTreeMap<String, Post>,
    pending: BTreeMap<String, Post>,
    writers: BTreeMap<u64, String>, // writer ID -> DID that first used it here
    last_seq: BTreeMap<u64, u64>,
    frozen: BTreeSet<u64>,
    order: Vec<String>,
}
impl Board {
    fn new(roster: BTreeSet<String>, policy: ForkPolicy) -> Self {
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
    fn receive(&mut self, p: Post) -> Verdict {
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
    fn park(&mut self, id: String, p: Post) -> Verdict {
        if self.pending.len() >= MAX_PENDING_POSTS {
            return Verdict::Rejected(Why::Limit);
        }
        self.pending.insert(id, p);
        Verdict::Pending
    }
    fn check(&self, p: &Post) -> Result<(), Why> {
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
    fn apply(&mut self, p: Post) -> Result<bool, Why> {
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
    fn drain(&mut self) {
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
    fn content_snapshot(&self) -> Vec<u8> {
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
    fn snapshot(&self) -> Vec<u8> {
        [
            self.content_snapshot(),
            serde_json::to_vec(&self.frozen).unwrap(),
        ]
        .concat()
    }
    fn texts(&self) -> BTreeSet<String> {
        self.accepted
            .values()
            .map(|p| p.body.text.clone())
            .collect()
    }
}

// ---- DID-signed endpoint notes (sim stand-in for permits; no succession verifier) ---------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Audience {
    Group(String),
    Private,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NoteClaims {
    owner: String,
    audience: Audience,
    slot: u64, // random per-device route slot (host routing key); not a key, not an address
    endpoint: String,
    seq: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct EpNote {
    claims: NoteClaims,
    sig: Vec<u8>,
}
impl EpNote {
    fn bytes(c: &NoteClaims) -> Vec<u8> {
        [NOTE_DOMAIN, &serde_json::to_vec(c).unwrap()].concat()
    }
    fn sign(id: &Identity, audience: Audience, slot: u64, endpoint: &str, seq: u64) -> Self {
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
    fn verify(&self) -> bool {
        let Some(key) = public_key_from_did(&self.claims.owner) else {
            return false;
        };
        let Ok(sig) = <[u8; 64]>::try_from(self.sig.as_slice()) else {
            return false;
        };
        verify(&key, &Self::bytes(&self.claims), &sig)
    }
    fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoteReject {
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
struct ReqKeyClaim {
    group: String,
    owner: String,
    x25519: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SignedReqKey {
    claim: ReqKeyClaim,
    sig: Vec<u8>,
}
fn chat_bytes(purpose: &str, body: &impl Serialize) -> Vec<u8> {
    [
        CHAT_DOMAIN.as_bytes(),
        b"\0",
        purpose.as_bytes(),
        b"\0",
        &serde_json::to_vec(body).unwrap(),
    ]
    .concat()
}
fn did_sign(id: &Identity, purpose: &str, body: &impl Serialize) -> Vec<u8> {
    id.sign(&chat_bytes(purpose, body)).to_vec()
}
fn did_verify(did: &str, purpose: &str, body: &impl Serialize, sig: &[u8]) -> bool {
    let (Some(public), Ok(sig)) = (public_key_from_did(did), <[u8; 64]>::try_from(sig)) else {
        return false;
    };
    verify(&public, &chat_bytes(purpose, body), &sig)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum ChatKind {
    Request,
    Answer,
}
// Everything the host can read (AEAD associated data).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Header {
    group: String,
    kind: ChatKind,
    from: String,
    to: String,
    ephemeral: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Sealed {
    header: Header,
    ciphertext: Vec<u8>,
}
// Request carries NO endpoint; only the acceptance carries the target's private endpoint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Inner {
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
    fn parties(&self) -> (&str, &str, &str) {
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
struct SignedInner {
    inner: Inner,
    sig: Vec<u8>,
}
impl SignedInner {
    fn new(id: &Identity, inner: Inner) -> Self {
        let sig = did_sign(id, "chat-inner", &inner);
        Self { inner, sig }
    }
    fn verify(&self) -> bool {
        did_verify(self.inner.parties().1, "chat-inner", &self.inner, &self.sig)
    }
    fn digest(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).unwrap())
            .to_hex()
            .to_string()
    }
    fn padded(&self) -> Vec<u8> {
        let bytes = serde_json::to_vec(self).unwrap();
        assert!(bytes.len() + 4 <= PAD, "fixture payload exceeds pad bucket");
        let mut out = (bytes.len() as u32).to_le_bytes().to_vec();
        out.extend(bytes);
        out.resize(PAD, 0);
        out
    }
}
fn unpad(b: &[u8]) -> Option<SignedInner> {
    if b.len() != PAD {
        return None;
    }
    let len = u32::from_le_bytes(b[..4].try_into().unwrap()) as usize;
    serde_json::from_slice(b.get(4..4 + len)?).ok()
}
fn aead_key(shared: &[u8; 32], ephemeral: &[u8; 32], recipient: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(KDF_CONTEXT, &[&shared[..], ephemeral, recipient].concat())
}
fn aad(h: &Header) -> Vec<u8> {
    [
        CHAT_DOMAIN.as_bytes(),
        b"\0aad\0",
        &serde_json::to_vec(h).unwrap(),
    ]
    .concat()
}
fn seal(mut header: Header, recipient: &[u8; 32], eph_seed: [u8; 32], p: &SignedInner) -> Sealed {
    let eph = StaticSecret::from(eph_seed);
    let eph_pub = PublicKey::from(&eph).to_bytes();
    header.ephemeral = eph_pub;
    let shared = eph.diffie_hellman(&PublicKey::from(*recipient));
    assert!(shared.was_contributory());
    let key = aead_key(shared.as_bytes(), &eph_pub, recipient);
    let ciphertext = Aes256Gcm::new_from_slice(&key)
        .unwrap()
        .encrypt(
            Nonce::from_slice(&[0u8; 12]),
            Payload {
                msg: &p.padded(),
                aad: &aad(&header),
            },
        )
        .unwrap();
    Sealed { header, ciphertext }
}
fn open(secret: &StaticSecret, w: &Sealed) -> Option<SignedInner> {
    let recipient = PublicKey::from(secret).to_bytes();
    let shared = secret.diffie_hellman(&PublicKey::from(w.header.ephemeral));
    if !shared.was_contributory() {
        return None;
    }
    let key = aead_key(shared.as_bytes(), &w.header.ephemeral, &recipient);
    let plain = Aes256Gcm::new_from_slice(&key)
        .unwrap()
        .decrypt(
            Nonce::from_slice(&[0u8; 12]),
            Payload {
                msg: &w.ciphertext,
                aad: &aad(&w.header),
            },
        )
        .ok()?;
    unpad(&plain)
}
// Direct (private-endpoint) first message after acceptance; DID-signed, host not involved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Hello {
    from: String,
    to: String,
    endpoint: String,
    text: String,
}
#[derive(Debug, PartialEq, Eq)]
enum ChatIn {
    Request { from: String, request_id: String },
    Accepted { from: String },
    Dropped,
}

// ---- devices and world ---------------------------------------------------------------------

struct Device {
    id: Identity,
    w: Writer,
    slot: u64,
    board: Board,
    group_key: SigningKey,
    group_seq: u64,
    private_key: SigningKey,
    private_seq: u64,
    req_secret: [u8; 32], // per-DID request key (same on all of a DID's devices)
    host_did: String,
    // A member's ENTIRE group address book: the host's DID-signed group note. Nothing else.
    host_note: Option<EpNote>,
    req_keys: BTreeMap<String, SignedReqKey>,
    private_book: BTreeMap<String, String>, // DID -> private endpoint (private audience only)
    unsent_note: Option<Vec<u8>>,
    outbox: Vec<Post>,
    refused: Vec<(Post, Why)>,
    online: bool,
    contacts: BTreeSet<String>,
    blocked: BTreeSet<String>, // personal block list: excludes from private notes (U5 rev.)
    incoming: BTreeMap<String, (String, String)>, // request_id -> (from, digest)
    outgoing: BTreeMap<String, (String, String)>, // request_id -> (to, digest)
    seen: BTreeSet<String>,
}
impl Device {
    fn did(&self) -> String {
        self.id.did().into()
    }
    fn group_ep(&self) -> String {
        ep("grp", &self.group_key)
    }
    fn private_ep(&self) -> String {
        ep("prv", &self.private_key)
    }
    fn chat_snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(&(
            &self.incoming,
            &self.outgoing,
            &self.seen,
            &self.private_book,
        ))
        .unwrap()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueueBound {
    Unbounded,
    Refuse(usize),
}
// G-TIME candidate: how long the host keeps its old group endpoint listening after rotating.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostOverlap {
    Immediate,
    Ticks(u64),
}
#[derive(Clone, Copy)]
struct Cfg {
    seed: u64,
    fork: ForkPolicy,
    queue: QueueBound,
    overlap: HostOverlap,
}
impl Default for Cfg {
    fn default() -> Self {
        Self {
            seed: 0x5EED_0001,
            fork: ForkPolicy::RejectLater,
            queue: QueueBound::Unbounded,
            overlap: HostOverlap::Immediate,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RouteErr {
    SelfOffline,
    PeerOffline,
    NoAddress,
    EndpointGone,
    Lost,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Path {
    MemberDialedHost,
    MemberDialedRetiringHostEndpoint,
    HostDialedMember,
}
#[derive(Clone, Debug)]
struct Frame {
    from: usize,
    to: usize,
    to_ep: String,
    kind: &'static str,
    bytes: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyKind {
    Group,
    Private,
}
struct World {
    cfg: Cfg,
    rng: SimRng,
    now: u64,
    d: Vec<Device>,
    // Host-only knowledge (U7): "<did>#<slot>" -> member device's DID-signed group note.
    directory: BTreeMap<String, EpNote>,
    live: BTreeMap<String, usize>, // listening endpoint -> device index
    retiring: BTreeMap<String, u64>, // old host endpoint -> retire_at (overlap candidate)
    frames: Vec<Frame>,            // every frame on any link
    keys_ever: Vec<(usize, KeyKind, [u8; 32])>,
}
fn dir_key(did: &str, slot: u64) -> String {
    format!("{did}#{slot}")
}
impl World {
    fn new(cfg: Cfg) -> Self {
        let mut rng = SimRng(cfg.seed);
        // hana = host, alice (two devices, one mnemonic), bob, carol.
        let ids = [ident(10), ident(11), ident(12), ident(13)];
        let owners = [0usize, 1, 1, 2, 3];
        let roster: BTreeSet<String> = ids.iter().map(|i| i.did().to_string()).collect();
        let req: Vec<[u8; 32]> = ids.iter().map(|_| rng.bytes32()).collect();
        let host_did = ids[0].did().to_string();
        let mut d = vec![];
        for &k in &owners {
            let id = ids[k].clone();
            let writer = rng.writer();
            let slot = rng.next_u64();
            let group_key = SigningKey::from_bytes(&rng.bytes32());
            let private_key = SigningKey::from_bytes(&rng.bytes32());
            let own = id.did().to_string();
            d.push(Device {
                w: Writer::new(id.clone(), writer),
                id,
                slot,
                board: Board::new(roster.clone(), cfg.fork),
                group_key,
                group_seq: 1,
                private_key,
                private_seq: 0,
                req_secret: req[k],
                host_did: host_did.clone(),
                host_note: None,
                req_keys: BTreeMap::new(),
                private_book: BTreeMap::new(),
                unsent_note: None,
                outbox: vec![],
                refused: vec![],
                online: true,
                contacts: roster.iter().filter(|x| **x != own).cloned().collect(),
                blocked: BTreeSet::new(),
                incoming: BTreeMap::new(),
                outgoing: BTreeMap::new(),
                seen: BTreeSet::new(),
            });
        }
        let mut w = World {
            cfg,
            rng,
            now: 0,
            d,
            directory: BTreeMap::new(),
            live: BTreeMap::new(),
            retiring: BTreeMap::new(),
            frames: vec![],
            keys_ever: vec![],
        };
        for i in 0..w.d.len() {
            let (g, p) = (w.d[i].group_ep(), w.d[i].private_ep());
            w.live.insert(g, i);
            w.live.insert(p, i);
            let (gk, pk) = (tkey(&w.d[i].group_key), tkey(&w.d[i].private_key));
            w.keys_ever.push((i, KeyKind::Group, gk));
            w.keys_ever.push((i, KeyKind::Private, pk));
        }
        // Join bootstrap (invite/ticket) is out of scope. Host signs its own note; members
        // hand their group note to the host only; host hands its note to each member.
        let h = &w.d[HOST];
        let hn = EpNote::sign(
            &h.id,
            Audience::Group(GROUP.into()),
            h.slot,
            &h.group_ep(),
            1,
        );
        w.d[HOST].host_note = Some(hn.clone());
        let host_ep = w.d[HOST].group_ep();
        for i in 1..w.d.len() {
            let dv = &w.d[i];
            let n = EpNote::sign(
                &dv.id,
                Audience::Group(GROUP.into()),
                dv.slot,
                &dv.group_ep(),
                1,
            );
            w.send(i, HOST, host_ep.clone(), "join-note", n.wire());
            w.host_accept_note(&n.wire()).unwrap();
            let mep = w.d[i].group_ep();
            w.send(HOST, i, mep, "host-note", hn.wire());
            w.member_accept_host_note(i, &hn.wire()).unwrap();
        }
        // Request-key claims (DID-signed, per DID) relayed by the host to every member.
        let mut claims = vec![];
        for k in [0usize, 1, 3, 4] {
            let dv = &w.d[k];
            let claim = ReqKeyClaim {
                group: GROUP.into(),
                owner: dv.did(),
                x25519: PublicKey::from(&StaticSecret::from(dv.req_secret)).to_bytes(),
            };
            let sig = did_sign(&dv.id, "req-key", &claim);
            claims.push(SignedReqKey { claim, sig });
        }
        for i in 0..w.d.len() {
            for c in &claims {
                if i != HOST {
                    let mep = w.d[i].group_ep();
                    w.send(HOST, i, mep, "req-key", serde_json::to_vec(c).unwrap());
                }
                assert!(did_verify(&c.claim.owner, "req-key", &c.claim, &c.sig));
                w.d[i].req_keys.insert(c.claim.owner.clone(), c.clone());
            }
        }
        w
    }
    fn did(&self, i: usize) -> String {
        self.d[i].did()
    }
    fn send(&mut self, from: usize, to: usize, to_ep: String, kind: &'static str, b: Vec<u8>) {
        self.frames.push(Frame {
            from,
            to,
            to_ep,
            kind,
            bytes: b,
        });
    }
    fn listening(&self, ep: &str) -> Option<usize> {
        if let Some(&i) = self.live.get(ep) {
            return Some(i);
        }
        match self.retiring.get(ep) {
            Some(&t) if self.now < t => Some(HOST),
            _ => None,
        }
    }
    fn tick(&mut self, n: u64) {
        self.now += n;
    }
    // Host side: accept a member device's DID-signed group note into the directory.
    fn host_accept_note(&mut self, bytes: &[u8]) -> Result<(), NoteReject> {
        let Ok(n) = serde_json::from_slice::<EpNote>(bytes) else {
            return Err(NoteReject::Malformed);
        };
        if !n.verify() {
            return Err(NoteReject::BadSignature);
        }
        if n.claims.audience != Audience::Group(GROUP.into()) {
            return Err(NoteReject::WrongAudience);
        }
        if !self.d[HOST].board.roster.contains(&n.claims.owner) {
            return Err(NoteReject::NotMember);
        }
        let key = dir_key(&n.claims.owner, n.claims.slot);
        if let Some(cur) = self.directory.get(&key) {
            if cur.claims.seq >= n.claims.seq {
                return Err(NoteReject::Stale);
            }
        }
        self.directory.insert(key, n);
        Ok(())
    }
    fn member_accept_host_note(&mut self, i: usize, bytes: &[u8]) -> Result<(), NoteReject> {
        let Ok(n) = serde_json::from_slice::<EpNote>(bytes) else {
            return Err(NoteReject::Malformed);
        };
        if !n.verify() {
            return Err(NoteReject::BadSignature);
        }
        if n.claims.owner != self.d[i].host_did {
            return Err(NoteReject::WrongOwner);
        }
        if n.claims.audience != Audience::Group(GROUP.into()) {
            return Err(NoteReject::WrongAudience);
        }
        if let Some(cur) = &self.d[i].host_note {
            if cur.claims.seq >= n.claims.seq {
                return Err(NoteReject::Stale);
            }
        }
        self.d[i].host_note = Some(n);
        Ok(())
    }
    fn host_snapshot(&self) -> Vec<u8> {
        [
            self.d[HOST].board.snapshot(),
            serde_json::to_vec(&self.directory).unwrap(),
        ]
        .concat()
    }
    // Member dials the host using the only group address it has.
    fn reach_host(&self, i: usize) -> Result<String, RouteErr> {
        if !self.d[i].online {
            return Err(RouteErr::SelfOffline);
        }
        let Some(n) = &self.d[i].host_note else {
            return Err(RouteErr::NoAddress);
        };
        let ep = n.claims.endpoint.clone();
        match self.listening(&ep) {
            Some(HOST) if self.d[HOST].online => Ok(ep),
            Some(HOST) => Err(RouteErr::PeerOffline),
            _ => Err(RouteErr::EndpointGone),
        }
    }
    // Host dials a member device via its directory entry.
    fn host_reach(&self, i: usize) -> Result<String, RouteErr> {
        if !self.d[HOST].online {
            return Err(RouteErr::SelfOffline);
        }
        let Some(n) = self.directory.get(&dir_key(&self.did(i), self.d[i].slot)) else {
            return Err(RouteErr::NoAddress);
        };
        let ep = n.claims.endpoint.clone();
        match self.live.get(&ep) {
            Some(&j) if j == i && self.d[i].online => Ok(ep),
            Some(&j) if j == i => Err(RouteErr::PeerOffline),
            _ => Err(RouteErr::EndpointGone),
        }
    }
    // Any member's attempt to reach another DID: only addresses it actually holds.
    fn dial(&self, i: usize, to: &str) -> Result<String, RouteErr> {
        if !self.d[i].online {
            return Err(RouteErr::SelfOffline);
        }
        let ep = if to == self.d[i].host_did {
            match &self.d[i].host_note {
                Some(n) => n.claims.endpoint.clone(),
                None => return Err(RouteErr::NoAddress),
            }
        } else if let Some(e) = self.d[i].private_book.get(to) {
            e.clone()
        } else {
            return Err(RouteErr::NoAddress);
        };
        match self.listening(&ep) {
            Some(j) if self.did(j) == to && self.d[j].online => Ok(ep),
            Some(j) if self.did(j) == to => Err(RouteErr::PeerOffline),
            _ => Err(RouteErr::EndpointGone),
        }
    }
    // Local write. Members queue for the host; the host applies to its own board directly.
    fn post(&mut self, i: usize, text: &str) -> Option<Post> {
        if let QueueBound::Refuse(n) = self.cfg.queue {
            if i != HOST && self.d[i].outbox.len() >= n {
                return None;
            }
        }
        let dv = &mut self.d[i];
        dv.w.observe(&dv.board);
        let p = dv.w.post(GROUP, text);
        if i == HOST {
            let v = self.d[HOST].board.receive(p.clone());
            assert_eq!(v, Verdict::Accepted);
            self.host_push_all();
        } else {
            self.d[i].outbox.push(p.clone());
        }
        Some(p)
    }
    // Host pushes (on an established link) its newer note and every post the device lacks.
    fn push_to(&mut self, j: usize, to_ep: String) {
        let hn = self.d[HOST].host_note.clone().unwrap();
        if self.d[j].host_note.as_ref().map(|n| n.claims.seq) < Some(hn.claims.seq) {
            self.send(HOST, j, to_ep.clone(), "host-note", hn.wire());
            self.member_accept_host_note(j, &hn.wire()).unwrap();
        }
        let missing: Vec<Post> = self.d[HOST]
            .board
            .order
            .iter()
            .filter(|id| !self.d[j].board.accepted.contains_key(*id))
            .map(|id| self.d[HOST].board.accepted[id].clone())
            .collect();
        for p in missing {
            self.send(HOST, j, to_ep.clone(), "post", p.wire());
            self.d[j].board.receive(p);
        }
        let dv = &mut self.d[j];
        dv.w.observe(&dv.board);
    }
    fn host_push_all(&mut self) {
        for j in 1..self.d.len() {
            if let Ok(ep) = self.host_reach(j) {
                self.push_to(j, ep);
            }
        }
        let h = &mut self.d[HOST];
        h.w.observe(&h.board);
    }
    // Member -> host half of an established link: pending own note, then queued posts.
    fn upload(&mut self, i: usize, host_ep: String) -> Vec<Verdict> {
        if let Some(b) = self.d[i].unsent_note.take() {
            self.send(i, HOST, host_ep.clone(), "ep-note", b.clone());
            let _ = self.host_accept_note(&b);
        }
        let mut out = vec![];
        for p in std::mem::take(&mut self.d[i].outbox) {
            self.send(i, HOST, host_ep.clone(), "post", p.wire());
            let v = self.d[HOST].board.receive(p.clone());
            if let Verdict::Rejected(why) = v {
                self.d[i].refused.push((p, why));
            }
            out.push(v);
        }
        out
    }
    // "The side whose address is still live retries" (U1): member dials host, else host
    // dials member. Exchanges queued state, then the host fans out to everyone it can reach.
    fn reconnect(&mut self, i: usize) -> Result<Path, RouteErr> {
        if !self.d[i].online {
            return Err(RouteErr::SelfOffline);
        }
        if !self.d[HOST].online {
            return Err(RouteErr::PeerOffline);
        }
        let path = match self.reach_host(i) {
            Ok(hep) => {
                let path = if self.live.get(&hep) == Some(&HOST) {
                    Path::MemberDialedHost
                } else {
                    Path::MemberDialedRetiringHostEndpoint
                };
                self.upload(i, hep);
                let mep = self.d[i].group_ep();
                self.push_to(i, mep);
                path
            }
            Err(_) => match self.host_reach(i) {
                Ok(mep) => {
                    self.push_to(i, mep); // carries the new host note
                    let hep = self.d[HOST].group_ep();
                    self.upload(i, hep);
                    let mep = self.d[i].group_ep();
                    self.push_to(i, mep);
                    Path::HostDialedMember
                }
                Err(_) => return Err(RouteErr::Lost),
            },
        };
        self.host_push_all();
        Ok(path)
    }
    // Periodic member group-endpoint rotation (never triggered by a block). Note goes to the
    // host only; if unreachable it waits for the next link. Old endpoint stops listening.
    fn rotate_member_group(&mut self, i: usize) -> Vec<u8> {
        assert_ne!(i, HOST);
        let old = self.d[i].group_ep();
        self.live.remove(&old);
        self.d[i].group_key = SigningKey::from_bytes(&self.rng.bytes32());
        self.d[i].group_seq += 1;
        let dv = &self.d[i];
        let new = dv.group_ep();
        let n = EpNote::sign(
            &dv.id,
            Audience::Group(GROUP.into()),
            dv.slot,
            &new,
            dv.group_seq,
        );
        let k = tkey(&self.d[i].group_key);
        self.keys_ever.push((i, KeyKind::Group, k));
        self.live.insert(new, i);
        let bytes = n.wire();
        match self.reach_host(i) {
            Ok(hep) => {
                self.send(i, HOST, hep, "ep-note", bytes.clone());
                self.host_accept_note(&bytes).unwrap();
            }
            Err(_) => self.d[i].unsent_note = Some(bytes.clone()),
        }
        bytes
    }
    // Host rotation: pushed directly to every member device the host can reach.
    fn rotate_host(&mut self) -> Vec<u8> {
        let old = self.d[HOST].group_ep();
        self.live.remove(&old);
        if let HostOverlap::Ticks(t) = self.cfg.overlap {
            self.retiring.insert(old, self.now + t);
        }
        self.d[HOST].group_key = SigningKey::from_bytes(&self.rng.bytes32());
        self.d[HOST].group_seq += 1;
        let h = &self.d[HOST];
        let new = h.group_ep();
        let n = EpNote::sign(
            &h.id,
            Audience::Group(GROUP.into()),
            h.slot,
            &new,
            h.group_seq,
        );
        let k = tkey(&self.d[HOST].group_key);
        self.keys_ever.push((HOST, KeyKind::Group, k));
        self.live.insert(new, HOST);
        self.d[HOST].host_note = Some(n.clone());
        for j in 1..self.d.len() {
            if let Ok(mep) = self.host_reach(j) {
                self.send(HOST, j, mep, "host-note", n.wire());
                self.member_accept_host_note(j, &n.wire()).unwrap();
            }
        }
        n.wire()
    }
    // Private-audience note (initial announce or periodic rotation): sent directly to
    // unblocked contacts' devices, never via the host (U1/U5). Siblings are out of scope.
    fn announce_private(&mut self, i: usize) -> Vec<usize> {
        if self.d[i].private_seq > 0 {
            let old = self.d[i].private_ep();
            self.live.remove(&old);
            self.d[i].private_key = SigningKey::from_bytes(&self.rng.bytes32());
            let k = tkey(&self.d[i].private_key);
            self.keys_ever.push((i, KeyKind::Private, k));
            let new = self.d[i].private_ep();
            self.live.insert(new, i);
        }
        self.d[i].private_seq += 1;
        let dv = &self.d[i];
        let n = EpNote::sign(
            &dv.id,
            Audience::Private,
            dv.slot,
            &dv.private_ep(),
            dv.private_seq,
        );
        let recipients: Vec<usize> = (0..self.d.len())
            .filter(|&j| {
                let t = self.did(j);
                t != dv.did() && dv.contacts.contains(&t) && !dv.blocked.contains(&t)
            })
            .collect();
        let owner = self.did(i);
        for &j in &recipients {
            let to_ep = self.d[j].private_ep();
            self.send(i, j, to_ep, "private-note", n.wire());
            assert!(n.verify());
            self.d[j]
                .private_book
                .insert(owner.clone(), n.claims.endpoint.clone());
        }
        recipients
    }
    // Every byte device `i` received on any link plus its validated replica state.
    fn received(&self, i: usize) -> Vec<u8> {
        let mut hay: Vec<u8> = self
            .frames
            .iter()
            .filter(|f| f.to == i)
            .flat_map(|f| f.bytes.clone())
            .collect();
        hay.extend(self.d[i].board.snapshot());
        hay.extend(serde_json::to_vec(&self.d[i].host_note).unwrap());
        hay
    }
    // Redraw a writer ID (collision recovery candidate): fresh Loro writer from the RNG,
    // rebuilt from the validated board; refused + queued texts are re-posted under it.
    fn redraw_writer(&mut self, i: usize) -> u64 {
        let new = self.rng.writer();
        let id = self.d[i].id.clone();
        self.d[i].w = Writer::new(id, new);
        let mut texts: Vec<String> = self.d[i]
            .refused
            .drain(..)
            .map(|(p, _)| p.body.text)
            .collect();
        texts.extend(self.d[i].outbox.drain(..).map(|p| p.body.text));
        for t in texts {
            self.post(i, &t);
        }
        new
    }
    // ---- chat request via host ----
    fn chat_request(&mut self, i: usize, to: &str) -> Option<String> {
        let claim = self.d[i].req_keys.get(to)?.clone();
        let hep = self.reach_host(i).ok()?;
        let request_id = hex32(&self.rng.bytes32()[..8]);
        let inner = Inner::Request {
            group: GROUP.into(),
            from: self.did(i),
            to: to.into(),
            request_id: request_id.clone(),
            sent_at: self.now,
        };
        let signed = SignedInner::new(&self.d[i].id, inner);
        let header = Header {
            group: GROUP.into(),
            kind: ChatKind::Request,
            from: self.did(i),
            to: to.into(),
            ephemeral: [0; 32],
        };
        let eph = self.rng.bytes32();
        let sealed = seal(header, &claim.claim.x25519, eph, &signed);
        self.d[i]
            .outgoing
            .insert(request_id.clone(), (to.into(), signed.digest()));
        let bytes = serde_json::to_vec(&sealed).unwrap();
        self.send(i, HOST, hep, "chat", bytes.clone());
        self.host_route_chat(&bytes);
        Some(request_id)
    }
    // Host sees only the header; delivers to every reachable device of the target DID.
    fn host_route_chat(&mut self, bytes: &[u8]) -> Vec<(usize, ChatIn)> {
        let Ok(w) = serde_json::from_slice::<Sealed>(bytes) else {
            return vec![];
        };
        let targets: Vec<usize> = self
            .directory
            .values()
            .filter(|n| n.claims.owner == w.header.to)
            .filter_map(|n| self.live.get(&n.claims.endpoint).copied())
            .collect();
        let mut out = vec![];
        for j in targets {
            if let Ok(mep) = self.host_reach(j) {
                self.send(HOST, j, mep, "chat", bytes.to_vec());
                out.push((j, self.chat_inbox(j, bytes)));
            }
        }
        out
    }
    fn chat_inbox(&mut self, j: usize, bytes: &[u8]) -> ChatIn {
        let Ok(w) = serde_json::from_slice::<Sealed>(bytes) else {
            return ChatIn::Dropped;
        };
        let me = self.did(j);
        let Some(s) = open(&StaticSecret::from(self.d[j].req_secret), &w) else {
            return ChatIn::Dropped;
        };
        let (group, from, to) = s.inner.parties();
        if !s.verify() || group != GROUP || to != me || from != w.header.from {
            return ChatIn::Dropped;
        }
        let digest = s.digest();
        // Blocking is NOT modelled on the chat path. U5 revised (visible blocking) leaves the
        // notice shape open (decisions section 8b); W2-Q owns that path.
        if self.d[j].seen.contains(&digest) {
            return ChatIn::Dropped;
        }
        match &s.inner {
            Inner::Request {
                from, request_id, ..
            } => {
                if self.d[j].incoming.contains_key(request_id) {
                    return ChatIn::Dropped;
                }
                let dv = &mut self.d[j];
                dv.seen.insert(digest.clone());
                dv.incoming
                    .insert(request_id.clone(), (from.clone(), digest));
                ChatIn::Request {
                    from: from.clone(),
                    request_id: request_id.clone(),
                }
            }
            Inner::Accept {
                from,
                request_id,
                request_digest,
                endpoint,
                ..
            } => {
                let ok = self.d[j].outgoing.get(request_id)
                    == Some(&(from.clone(), request_digest.clone()));
                if !ok {
                    return ChatIn::Dropped;
                }
                let dv = &mut self.d[j];
                dv.seen.insert(digest);
                dv.private_book.insert(from.clone(), endpoint.clone());
                ChatIn::Accepted { from: from.clone() }
            }
        }
    }
    fn chat_accept(&mut self, j: usize, request_id: &str) -> Vec<(usize, ChatIn)> {
        let (from, digest) = self.d[j].incoming[request_id].clone();
        let claim = self.d[j].req_keys[&from].clone();
        let hep = self.reach_host(j).unwrap();
        let inner = Inner::Accept {
            group: GROUP.into(),
            from: self.did(j),
            to: from.clone(),
            request_id: request_id.into(),
            request_digest: digest,
            endpoint: self.d[j].private_ep(),
        };
        let signed = SignedInner::new(&self.d[j].id, inner);
        let header = Header {
            group: GROUP.into(),
            kind: ChatKind::Answer,
            from: self.did(j),
            to: from,
            ephemeral: [0; 32],
        };
        let eph = self.rng.bytes32();
        let bytes = serde_json::to_vec(&seal(header, &claim.claim.x25519, eph, &signed)).unwrap();
        self.send(j, HOST, hep, "chat", bytes.clone());
        self.host_route_chat(&bytes)
    }
    // Direct private-endpoint link after acceptance; the host is not on this path.
    fn direct(&mut self, i: usize, to: &str, text: &str) -> Result<(), RouteErr> {
        let to_ep = self.dial(i, to)?;
        let j = self.listening(&to_ep).unwrap();
        let hello = Hello {
            from: self.did(i),
            to: to.into(),
            endpoint: self.d[i].private_ep(),
            text: text.into(),
        };
        let sig = did_sign(&self.d[i].id, "hello", &hello);
        self.send(
            i,
            j,
            to_ep,
            "direct",
            serde_json::to_vec(&(&hello, &sig)).unwrap(),
        );
        assert!(did_verify(&hello.from, "hello", &hello, &sig));
        self.d[j].private_book.insert(hello.from, hello.endpoint);
        Ok(())
    }
    fn assert_converged(&self, devices: &[usize]) {
        let h = &self.d[HOST].board;
        for &i in devices {
            let b = &self.d[i].board;
            assert!(b.pending.is_empty(), "device {i} has pending posts");
            assert_eq!(
                b.accepted.keys().collect::<Vec<_>>(),
                h.accepted.keys().collect::<Vec<_>>(),
                "device {i} accepted set differs from host"
            );
            assert_eq!(b.doc.get_deep_value(), h.doc.get_deep_value());
        }
    }
}
fn world() -> World {
    World::new(Cfg::default())
}
const ALL: [usize; 5] = [HOST, A1, A2, BOB, CAROL];

// ---- tests ---------------------------------------------------------------------------------

// KIND: invariant
// U4 revised: a post is (DID, random writer ID, per-writer seq, text, Loro update) signed by
// the DID key; nothing else. Two devices of the same DID write concurrently (neither has seen
// the other), each under its own random writer ID == Loro peer ID; host relay converges
// every replica. The post verifies against the DID's public key alone (no attestation).
// Writer IDs come from the seeded RNG: same identities, different seed -> different writers.
#[test]
fn did_signed_posts_with_random_writer_ids_two_own_devices_converge() {
    let mut w = world();
    let alice = w.did(A1);
    assert_eq!(alice, w.did(A2), "fixture: one mnemonic, two devices");
    let (wa1, wa2) = (w.d[A1].w.writer, w.d[A2].w.writer);
    assert_ne!(wa1, wa2);
    assert_eq!(w.d[A1].w.doc.peer_id(), wa1);
    assert_eq!(w.d[A2].w.doc.peer_id(), wa2);
    // Not derived from any key the device holds (sanity check on the construction).
    for i in ALL {
        let dv = &w.d[i];
        let keys = [
            dv.id.signing_public_key(),
            dv.id.device_public_key(),
            dv.id.encryption_public_key(),
            tkey(&dv.group_key),
            tkey(&dv.private_key),
        ];
        for k in keys {
            for c in k.chunks(8) {
                assert_ne!(u64::from_le_bytes(c.try_into().unwrap()), dv.w.writer);
                assert_ne!(u64::from_be_bytes(c.try_into().unwrap()), dv.w.writer);
            }
        }
    }
    let other = World::new(Cfg {
        seed: 0x5EED_0002,
        ..Cfg::default()
    });
    assert_eq!(other.did(A1), alice);
    assert_ne!(other.d[A1].w.writer, wa1);

    let p1 = w.post(A1, "alice on phone").unwrap();
    let p2 = w.post(A2, "alice on laptop").unwrap();
    let pb = w.post(BOB, "bob").unwrap();
    assert_eq!((p1.body.seq, p2.body.seq), (1, 1));
    // Exactly these fields; DID key verifies; no attestation or device field exists.
    let v: serde_json::Value = serde_json::from_slice(&p1.wire()).unwrap();
    let fields: BTreeSet<&str> = v["body"]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    assert_eq!(
        fields,
        ["author", "doc", "group", "seq", "text", "update", "writer"].into()
    );
    assert_eq!(v.as_object().unwrap().len(), 2, "body + sig only");
    let alice_key = public_key_from_did(&alice).unwrap();
    let sig: [u8; 64] = p1.sig.clone().try_into().unwrap();
    assert!(verify(&alice_key, &Post::signed_bytes(&p1.body), &sig));
    assert_eq!(
        LoroDoc::decode_import_blob_meta(&p1.body.update, true)
            .unwrap()
            .partial_end_vv
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![wa1]
    );
    for i in [A2, A1, BOB, CAROL] {
        assert!(w.reconnect(i).is_ok());
    }
    w.assert_converged(&ALL);
    let texts = w.d[CAROL].board.texts();
    for p in [&p1, &p2, &pb] {
        assert!(texts.contains(&p.body.text));
    }
    // Both devices' posts are attributed to the same DID under distinct writer IDs.
    let alice_writers: BTreeSet<u64> = w.d[HOST]
        .board
        .accepted
        .values()
        .filter(|p| p.body.author == alice)
        .map(|p| p.body.writer)
        .collect();
    assert_eq!(alice_writers, [wa1, wa2].into());
    // A follow-up from each device continues its own per-writer sequence after observing.
    let q1 = w.post(A1, "phone again").unwrap();
    let q2 = w.post(A2, "laptop again").unwrap();
    assert_eq!((q1.body.seq, q2.body.seq), (2, 2));
    for i in [A1, A2] {
        w.reconnect(i).unwrap();
    }
    w.assert_converged(&ALL);
    assert_eq!(w.d[HOST].board.accepted.len(), 5);
}

// KIND: invariant
// U7: scan every byte each member device received (all frames + its replica state + its
// host note). No device key (identity device key = today's EndpointId), and no group or
// private transport key / endpoint of any other device — including its own sibling device
// and every rotated-out key — appears. Positive controls: each member holds the host's
// CURRENT group endpoint; the host's received bytes DO contain members' group endpoints, so
// the scanner finds them when present.
#[test]
fn members_receive_no_device_key_transport_key_or_endpoint_of_any_other_member() {
    let mut w = world();
    for (i, t) in [(A1, "a1"), (A2, "a2"), (BOB, "b"), (CAROL, "c")] {
        w.post(i, t);
    }
    w.post(HOST, "host speaks");
    for i in [A1, A2, BOB, CAROL] {
        w.reconnect(i).unwrap();
    }
    w.rotate_member_group(BOB);
    w.rotate_member_group(A2);
    w.rotate_host();
    w.post(CAROL, "after rotations");
    for i in [A1, A2, BOB, CAROL] {
        w.reconnect(i).unwrap();
    }
    w.assert_converged(&ALL);
    let device_keys: Vec<[u8; 32]> = ALL.iter().map(|&i| w.d[i].id.device_public_key()).collect();
    for m in [A1, A2, BOB, CAROL] {
        let hay = w.received(m);
        for k in &device_keys {
            assert!(!leaks_key(&hay, k), "device key reached member {m}");
        }
        for (owner, kind, k) in &w.keys_ever {
            if *owner == m || (*owner == HOST && *kind == KeyKind::Group) {
                continue;
            }
            assert!(
                !leaks_key(&hay, k),
                "member {m} received {kind:?} key of device {owner}"
            );
        }
        // Positive control: the host's current group endpoint is known.
        assert!(has(&hay, w.d[HOST].group_ep().as_bytes()));
        assert!(w.d[m].private_book.is_empty());
    }
    // Scanner sensitivity: the host's received bytes contain every member's group endpoint.
    let host_hay = w.received(HOST);
    for m in [A1, A2, BOB, CAROL] {
        assert!(leaks_key(&host_hay, &tkey(&w.d[m].group_key)));
    }
}

// KIND: invariant
// W2-G `author_key_equal_to_private_transport_key_leaks_private_endpoint_to_blocked_member`
// (block_in_group.rs:753) re-run under this model: Bob blocks Carol, announces and twice
// rotates his private endpoint to unblocked contacts only, posts in the group, Carol posts
// and is fully served. Carol learns none of Bob's private endpoints, nor his group endpoint,
// nor his device key, from anything she received: there is no key field in a post to carry
// one. Positive controls: Alice (unblocked) holds Bob's latest private endpoint; Carol holds
// and verifies Bob's posts; her replica equals the host's.
#[test]
fn blocked_member_learns_no_private_endpoint_from_did_signed_group_content() {
    let mut w = world();
    let (bob, carol) = (w.did(BOB), w.did(CAROL));
    w.d[BOB].blocked.insert(carol.clone());
    for _ in 0..3 {
        let to = w.announce_private(BOB);
        assert!(!to.contains(&CAROL));
        assert_eq!(to, vec![HOST, A1, A2]);
    }
    w.post(BOB, "hi from bob");
    w.post(CAROL, "hi from carol");
    for i in [BOB, CAROL, A1, A2] {
        w.reconnect(i).unwrap();
    }
    w.post(BOB, "bob again");
    w.reconnect(BOB).unwrap();
    w.assert_converged(&ALL);
    let hay = w.received(CAROL);
    let bob_private: Vec<&[u8; 32]> = w
        .keys_ever
        .iter()
        .filter(|(o, k, _)| *o == BOB && *k == KeyKind::Private)
        .map(|(_, _, k)| k)
        .collect();
    assert_eq!(bob_private.len(), 3);
    for k in &bob_private {
        assert!(
            !leaks_key(&hay, k),
            "Bob's private endpoint leaked to Carol"
        );
    }
    assert!(!leaks_key(&hay, &tkey(&w.d[BOB].group_key)));
    assert!(!leaks_key(&hay, &w.d[BOB].id.device_public_key()));
    assert!(!w.d[CAROL].private_book.contains_key(&bob));
    assert_eq!(w.dial(CAROL, &bob), Err(RouteErr::NoAddress));
    // Positive controls.
    assert_eq!(w.d[A1].private_book[&bob], w.d[BOB].private_ep());
    assert_eq!(w.dial(A1, &bob), Ok(w.d[BOB].private_ep()));
    let bob_posts: Vec<&Post> = w.d[CAROL]
        .board
        .accepted
        .values()
        .filter(|p| p.body.author == bob)
        .collect();
    assert_eq!(bob_posts.len(), 2);
    assert!(bob_posts.iter().all(|p| p.verify()));
}

// KIND: invariant
// U7 topology by construction: a member's address book is {host note} ∪ private book; no
// field can hold another member's group endpoint. With no private relationship, every
// member-to-member dial is `NoAddress` in both directions, while each member reaches the
// host and the host reaches each member device. Every group frame has the host at one end.
#[test]
fn member_to_member_delivery_has_no_address_by_construction() {
    let mut w = world();
    for i in [A1, BOB, CAROL] {
        w.post(i, "x");
        w.reconnect(i).unwrap();
    }
    for a in [A1, A2, BOB, CAROL] {
        for b in [A1, A2, BOB, CAROL] {
            if w.did(a) != w.did(b) {
                assert_eq!(w.dial(a, &w.did(b)), Err(RouteErr::NoAddress));
            }
        }
        assert_eq!(w.dial(a, &w.did(HOST)), Ok(w.d[HOST].group_ep()));
        assert!(w.reach_host(a).is_ok());
        assert_eq!(w.host_reach(a), Ok(w.d[a].group_ep()));
    }
    assert!(!w.frames.is_empty());
    for f in &w.frames {
        assert!(
            f.from == HOST || f.to == HOST,
            "{} frame bypassed host",
            f.kind
        );
    }
    // The transport itself would deliver (positive control): it is the address that is
    // missing, not connectivity.
    assert_eq!(w.listening(&w.d[CAROL].group_ep()), Some(CAROL));
}

// KIND: invariant
// Member group rotation: Bob's DID-signed note goes to the host only (one frame); the host
// routes later posts to the new endpoint; the old endpoint is retired (no listener). Forged,
// replayed/stale, private-audience, wrong-group and non-member notes leave the host state
// byte-identical. No member ever receives either of Bob's group endpoints.
#[test]
fn member_rotation_note_goes_to_host_only_and_host_routes_to_new_endpoint() {
    let mut w = world();
    let old_ep = w.d[BOB].group_ep();
    let before_frames = w.frames.len();
    let note = w.rotate_member_group(BOB);
    let new_ep = w.d[BOB].group_ep();
    let sent: Vec<&Frame> = w.frames[before_frames..].iter().collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].from, sent[0].to, sent[0].kind),
        (BOB, HOST, "ep-note")
    );
    assert_eq!(sent[0].bytes, note);
    let key = dir_key(&w.did(BOB), w.d[BOB].slot);
    assert_eq!(w.directory[&key].claims.endpoint, new_ep);
    assert_eq!(w.directory[&key].claims.seq, 2);
    assert_eq!(w.listening(&old_ep), None);
    assert_eq!(w.host_reach(BOB), Ok(new_ep.clone()));
    // Host delivers Alice's post to Bob's new endpoint.
    w.post(A1, "for everyone");
    w.reconnect(A1).unwrap();
    let to_bob: Vec<&Frame> = w
        .frames
        .iter()
        .filter(|f| f.to == BOB && f.kind == "post")
        .collect();
    assert_eq!(to_bob.last().unwrap().to_ep, new_ep);
    assert!(w.d[BOB].board.texts().contains("for everyone"));
    // Rejections: host state unchanged.
    let bob_id = w.d[BOB].id.clone();
    let carol_id = w.d[CAROL].id.clone();
    let outsider = ident(99);
    let slot = w.d[BOB].slot;
    let mut forged = EpNote::sign(
        &carol_id,
        Audience::Group(GROUP.into()),
        slot,
        "grp-evil",
        9,
    );
    forged.claims.owner = w.did(BOB);
    let mut tampered: EpNote = serde_json::from_slice(&note).unwrap();
    tampered.claims.endpoint = "grp-evil".into();
    let cases: Vec<(Vec<u8>, NoteReject)> = vec![
        (note.clone(), NoteReject::Stale),
        (
            EpNote::sign(&bob_id, Audience::Group(GROUP.into()), slot, &old_ep, 1).wire(),
            NoteReject::Stale,
        ),
        (forged.wire(), NoteReject::BadSignature),
        (tampered.wire(), NoteReject::BadSignature),
        (
            EpNote::sign(&bob_id, Audience::Private, slot, "prv-x", 9).wire(),
            NoteReject::WrongAudience,
        ),
        (
            EpNote::sign(&bob_id, Audience::Group("other".into()), slot, "grp-x", 9).wire(),
            NoteReject::WrongAudience,
        ),
        (
            EpNote::sign(&outsider, Audience::Group(GROUP.into()), 1, "grp-x", 9).wire(),
            NoteReject::NotMember,
        ),
        (b"not json".to_vec(), NoteReject::Malformed),
    ];
    for (bytes, why) in cases {
        let before = w.host_snapshot();
        assert_eq!(w.host_accept_note(&bytes), Err(why));
        assert_eq!(w.host_snapshot(), before);
    }
    for m in [A1, A2, CAROL] {
        let hay = w.received(m);
        assert!(!has(&hay, old_ep.as_bytes()) && !has(&hay, new_ep.as_bytes()));
    }
}

// KIND: invariant
// Host rotation: online members get the DID-signed host note pushed directly (host knows
// their group endpoints). Carol was offline; the old host endpoint retired immediately, so
// her dial fails (EndpointGone), but her own group endpoint is still live and the host's
// retry reaches her (HostDialedMember): she gets the new note, uploads her queued post and
// converges. A forged/stale host note is refused by members with state unchanged.
#[test]
fn host_rotation_reaches_online_members_directly_and_offline_member_by_host_retry() {
    let mut w = world();
    w.d[CAROL].online = false;
    w.post(CAROL, "written offline");
    let before = w.frames.len();
    let note = w.rotate_host();
    let new_host = w.d[HOST].group_ep();
    let pushed: BTreeSet<usize> = w.frames[before..]
        .iter()
        .filter(|f| f.kind == "host-note")
        .map(|f| f.to)
        .collect();
    assert_eq!(pushed, [A1, A2, BOB].into());
    for i in [A1, A2, BOB] {
        assert_eq!(w.reach_host(i), Ok(new_host.clone()));
    }
    w.d[CAROL].online = true;
    assert_eq!(w.reach_host(CAROL), Err(RouteErr::EndpointGone));
    // Forged/stale host notes are refused by Carol, unchanged.
    let snap = serde_json::to_vec(&w.d[CAROL].host_note).unwrap();
    let mut forged: EpNote = serde_json::from_slice(&note).unwrap();
    forged.claims.endpoint = "grp-evil".into();
    let bob_id = w.d[BOB].id.clone();
    let by_bob = EpNote::sign(&bob_id, Audience::Group(GROUP.into()), 0, "grp-evil", 9);
    let old = EpNote::sign(
        &w.d[HOST].id.clone(),
        Audience::Group(GROUP.into()),
        w.d[HOST].slot,
        "grp-old",
        1,
    );
    for (b, why) in [
        (forged.wire(), NoteReject::BadSignature),
        (by_bob.wire(), NoteReject::WrongOwner),
        (old.wire(), NoteReject::Stale),
    ] {
        assert_eq!(w.member_accept_host_note(CAROL, &b), Err(why));
        assert_eq!(serde_json::to_vec(&w.d[CAROL].host_note).unwrap(), snap);
    }
    assert_eq!(w.reconnect(CAROL), Ok(Path::HostDialedMember));
    assert_eq!(w.reach_host(CAROL), Ok(new_host));
    assert_eq!(w.d[CAROL].host_note.as_ref().unwrap().wire(), note);
    assert!(w.d[HOST].board.texts().contains("written offline"));
    w.assert_converged(&ALL);
}

// KIND: limitation
// Host and Carol both rotate their group endpoints while apart (Carol offline, immediate
// retirement). Carol's note cannot reach the host; the host's push cannot reach Carol; Carol
// has no other member's address to ask (U7: no member forwarding). Both directions fail and
// she is cut off from the group: "lost contact — re-share" (decision Q22). Content written
// meanwhile never reaches her.
#[test]
fn host_and_member_both_rotated_while_apart_lose_each_other() {
    let mut w = world();
    w.d[CAROL].online = false;
    w.rotate_member_group(CAROL);
    assert!(w.d[CAROL].unsent_note.is_some());
    w.rotate_host();
    w.post(BOB, "carol never sees this");
    w.reconnect(BOB).unwrap();
    w.d[CAROL].online = true;
    assert_eq!(w.reach_host(CAROL), Err(RouteErr::EndpointGone));
    assert_eq!(w.host_reach(CAROL), Err(RouteErr::EndpointGone));
    for m in [A1, BOB] {
        assert_eq!(w.dial(CAROL, &w.did(m)), Err(RouteErr::NoAddress));
    }
    assert_eq!(w.reconnect(CAROL), Err(RouteErr::Lost));
    assert!(!w.d[CAROL].board.texts().contains("carol never sees this"));
}

// KIND: candidate-policy G-TIME
// Same both-rotated scenario with the host keeping its old endpoint listening for a window
// (overlap candidate) vs retiring it at once. Within the window Carol dials the retiring
// endpoint, receives the new note, delivers her own pending rotation note and converges;
// after the window she is lost exactly as with immediate retirement. The window value, and
// whether any overlap is acceptable, is open (Section 2 timing).
#[test]
fn host_overlap_window_candidate_recovers_member_only_within_window() {
    for (overlap, wait, expect) in [
        (HostOverlap::Immediate, 1, Err(RouteErr::Lost)),
        (
            HostOverlap::Ticks(5),
            3,
            Ok(Path::MemberDialedRetiringHostEndpoint),
        ),
        (HostOverlap::Ticks(5), 7, Err(RouteErr::Lost)),
    ] {
        let mut w = World::new(Cfg {
            overlap,
            ..Cfg::default()
        });
        w.d[CAROL].online = false;
        w.rotate_member_group(CAROL);
        w.rotate_host();
        w.post(BOB, "meanwhile");
        w.reconnect(BOB).unwrap();
        w.tick(wait);
        w.d[CAROL].online = true;
        assert_eq!(w.reconnect(CAROL), expect, "{overlap:?} after {wait}");
        if expect.is_ok() {
            assert_eq!(w.host_reach(CAROL), Ok(w.d[CAROL].group_ep()));
            assert_eq!(w.reach_host(CAROL), Ok(w.d[HOST].group_ep()));
            w.assert_converged(&ALL);
        }
    }
}

// KIND: limitation
// Host offline: the group stalls. Members keep writing locally (queued), cannot deliver to
// each other (no address) or to the host, and see nothing new. When the host returns, every
// queue flushes and all replicas converge. U7 accepts this; host succession/backup host is
// open (D01–D06).
#[test]
fn host_offline_stalls_group_writes_queue_and_converge_on_return() {
    let mut w = world();
    w.post(BOB, "before outage");
    w.reconnect(BOB).unwrap();
    let base: Vec<Vec<u8>> = ALL.iter().map(|&i| w.d[i].board.snapshot()).collect();
    w.d[HOST].online = false;
    w.post(A1, "a during outage");
    w.post(BOB, "b during outage");
    w.post(CAROL, "c during outage");
    for i in [A1, A2, BOB, CAROL] {
        assert_eq!(w.reconnect(i), Err(RouteErr::PeerOffline));
        assert_eq!(w.dial(i, &w.did(HOST)), Err(RouteErr::PeerOffline));
        for j in [A1, BOB, CAROL] {
            if w.did(i) != w.did(j) {
                assert_eq!(w.dial(i, &w.did(j)), Err(RouteErr::NoAddress));
            }
        }
    }
    assert_eq!(w.d[A1].outbox.len(), 1);
    for (k, &i) in ALL.iter().enumerate() {
        assert_eq!(
            w.d[i].board.snapshot(),
            base[k],
            "board {i} changed while host offline"
        );
    }
    w.d[HOST].online = true;
    for i in [A1, A2, BOB, CAROL] {
        w.reconnect(i).unwrap();
    }
    w.assert_converged(&ALL);
    assert_eq!(w.d[HOST].board.accepted.len(), 4);
}

// KIND: candidate-policy G-STORAGE
// Local queue while the host is offline: Unbounded keeps every write; Refuse(2) refuses the
// third write locally with the device's writer doc, outbox and board byte-identical (no
// silent drop of an already-queued post). Both converge on host return. Bound value, and
// whether refuse vs drop-oldest vs unbounded is right, is open.
#[test]
fn offline_queue_bound_candidates_unbounded_vs_refuse_new() {
    for (queue, expect) in [(QueueBound::Unbounded, 3), (QueueBound::Refuse(2), 2)] {
        let mut w = World::new(Cfg {
            queue,
            ..Cfg::default()
        });
        w.d[HOST].online = false;
        assert!(w.post(A1, "q1").is_some());
        assert!(w.post(A1, "q2").is_some());
        let before = (
            w.d[A1].w.doc.export(ExportMode::all_updates()).unwrap(),
            w.d[A1].w.seq,
            serde_json::to_vec(&w.d[A1].outbox).unwrap(),
            w.d[A1].board.snapshot(),
        );
        let third = w.post(A1, "q3");
        if queue == QueueBound::Unbounded {
            assert!(third.is_some());
        } else {
            assert!(third.is_none());
            let after = (
                w.d[A1].w.doc.export(ExportMode::all_updates()).unwrap(),
                w.d[A1].w.seq,
                serde_json::to_vec(&w.d[A1].outbox).unwrap(),
                w.d[A1].board.snapshot(),
            );
            assert_eq!(after, before);
        }
        w.d[HOST].online = true;
        w.reconnect(A1).unwrap();
        for i in [A2, BOB, CAROL] {
            w.reconnect(i).unwrap();
        }
        w.assert_converged(&ALL);
        assert_eq!(w.d[HOST].board.accepted.len(), expect);
    }
}

// Populated host board: alice-1 seq1, bob seq1 (after observing alice), alice-1 seq3 pending
// (seq2 withheld). Returns (world, withheld seq2).
fn populated() -> (World, Post) {
    let mut w = world();
    w.post(A1, "a1");
    w.reconnect(A1).unwrap();
    w.post(BOB, "b1");
    w.reconnect(BOB).unwrap();
    let a2 = w.post(A1, "a2").unwrap();
    let a3 = w.post(A1, "a3").unwrap();
    w.d[A1].outbox.clear();
    assert_eq!(w.d[HOST].board.receive(a3), Verdict::Pending);
    assert_eq!(w.d[HOST].board.accepted.len(), 2);
    (w, a2)
}
fn resign(id: &Identity, mut p: Post) -> Post {
    p.sig = id.sign(&Post::signed_bytes(&p.body)).to_vec();
    p
}

// KIND: invariant
// Rejections on a populated host board (accepted history + one pending post), each compared
// on the full serialized board before/after, each a single cause (re-signed where needed):
// forged DID signature (Bob signs a body claiming Alice), tampered text, wrong group, wrong
// doc, non-member DID, seq 0, Loro update whose ops belong to another writer ID, text not
// matching the Loro op. Replays return Duplicate / Pending without change. A member's own
// board refuses a host-tampered relay. Positive control: the withheld post then drains.
#[test]
fn forged_tampered_wrong_group_and_replayed_posts_leave_state_unchanged() {
    let (mut w, a2) = populated();
    let alice = w.d[A1].id.clone();
    let bob = w.d[BOB].id.clone();
    let outsider = ident(99);
    let mut forged = a2.clone();
    forged.body.text = "forged".into();
    let forged = resign(&bob, forged);
    let mut tampered = a2.clone();
    tampered.body.text = "tampered".into();
    let mut wrong_group = a2.clone();
    wrong_group.body.group = "other".into();
    let mut wrong_doc = a2.clone();
    wrong_doc.body.doc = "group/other/2026-10-10".into();
    let mut not_member = a2.clone();
    not_member.body.author = outsider.did().into();
    let mut seq0 = a2.clone();
    seq0.body.seq = 0;
    let mut other_writer = a2.clone();
    other_writer.body.writer = w.d[A2].w.writer;
    let mut text_mismatch = a2.clone();
    text_mismatch.body.text = "says something else".into();
    let cases = vec![
        (forged, Why::BadSignature),
        (tampered, Why::BadSignature),
        (resign(&alice, wrong_group), Why::WrongGroup),
        (resign(&alice, wrong_doc), Why::WrongGroup),
        (resign(&outsider, not_member), Why::NotMember),
        (resign(&alice, seq0), Why::Bounds),
        (resign(&alice, other_writer), Why::LoroMismatch),
        (resign(&alice, text_mismatch), Why::LoroMismatch),
    ];
    let board = &mut w.d[HOST].board;
    for (p, why) in cases {
        let before = board.snapshot();
        assert_eq!(board.receive(p), Verdict::Rejected(why));
        assert_eq!(board.snapshot(), before, "{why:?} mutated the board");
    }
    for id in board.order.clone() {
        let before = board.snapshot();
        assert_eq!(
            board.receive(board.accepted[&id].clone()),
            Verdict::Duplicate
        );
        assert_eq!(board.snapshot(), before);
    }
    let pend = board.pending.values().next().unwrap().clone();
    let before = board.snapshot();
    assert_eq!(board.receive(pend), Verdict::Pending);
    assert_eq!(board.snapshot(), before);
    // A host that alters a relayed post is caught by the member's own verification.
    let mut relayed = w.d[HOST].board.accepted.values().next().unwrap().clone();
    relayed.body.text = "host rewrote this".into();
    let carol_board = &mut w.d[CAROL].board;
    let before = carol_board.snapshot();
    assert_eq!(
        carol_board.receive(relayed),
        Verdict::Rejected(Why::BadSignature)
    );
    assert_eq!(carol_board.snapshot(), before);
    // Positive control.
    assert_eq!(w.d[HOST].board.receive(a2), Verdict::Accepted);
    assert!(w.d[HOST].board.pending.is_empty());
    assert_eq!(w.d[HOST].board.accepted.len(), 4);
}

// KIND: limitation
// Why a writer-ID collision rule is mandatory: two Loro docs that use the same peer ID for
// different content (as two DIDs, or two cloned devices, with a colliding writer ID would)
// import each other's updates without error but silently keep only their own op for the
// shared (peer, counter) — the replicas diverge permanently. Loro alone does not detect it.
#[test]
fn loro_same_peer_id_from_two_writers_silently_diverges_without_a_collision_rule() {
    let mut rng = SimRng(7);
    let shared = rng.writer();
    let (x, y) = (LoroDoc::new(), LoroDoc::new());
    x.set_peer_id(shared).unwrap();
    y.set_peer_id(shared).unwrap();
    x.get_map("messages").insert("k", "from alice").unwrap();
    x.commit();
    y.get_map("messages").insert("k2", "from carol").unwrap();
    y.commit();
    let ux = x.export(ExportMode::all_updates()).unwrap();
    let uy = y.export(ExportMode::all_updates()).unwrap();
    let _ = x.import(&uy);
    let _ = y.import(&ux);
    assert_ne!(x.get_deep_value(), y.get_deep_value(), "replicas diverged");
    let both = LoroDoc::new();
    both.get_map("messages").insert("k", "from alice").unwrap();
    both.get_map("messages").insert("k2", "from carol").unwrap();
    assert_ne!(x.get_deep_value(), both.get_deep_value());
    assert_ne!(y.get_deep_value(), both.get_deep_value());
}

// KIND: candidate-policy G-FORK
// Cross-DID writer-ID collision (forced: Carol reuses Alice-1's writer ID — models a 2^-64
// draw or a broken RNG; still not key-derived). Candidate "first claim wins at the host":
// the host binds writer -> DID on first accept; the later DID's post is refused with the
// host board unchanged. Because all traffic is serialised by the single host, every member
// converges to the host's choice in BOTH arrival orders. The loser redraws a writer ID,
// rebuilds from its validated board and re-posts; all converge with both texts.
#[test]
fn cross_did_writer_collision_first_claim_at_host_wins_and_loser_redraws() {
    for alice_first in [true, false] {
        let mut w = world();
        let shared = w.d[A1].w.writer;
        let carol_id = w.d[CAROL].id.clone();
        w.d[CAROL].w = Writer::new(carol_id, shared);
        w.post(A1, "alice");
        w.post(CAROL, "carol");
        let (first, second) = if alice_first {
            (A1, CAROL)
        } else {
            (CAROL, A1)
        };
        w.reconnect(first).unwrap();
        let hb = w.d[HOST].board.snapshot();
        let hep = w.reach_host(second).unwrap();
        let v = w.upload(second, hep);
        assert_eq!(v, vec![Verdict::Rejected(Why::WriterOwnedByOtherDid)]);
        assert_eq!(w.d[HOST].board.snapshot(), hb);
        for i in [A1, A2, BOB, CAROL] {
            w.reconnect(i).unwrap();
        }
        w.assert_converged(&ALL);
        assert_eq!(w.d[HOST].board.writers[&shared], w.did(first));
        assert_eq!(w.d[HOST].board.accepted.len(), 1);
        let new = w.redraw_writer(second);
        assert_ne!(new, shared);
        w.reconnect(second).unwrap();
        w.assert_converged(&ALL);
        assert_eq!(
            w.d[BOB].board.texts(),
            ["alice".to_string(), "carol".to_string()].into()
        );
    }
}

// KIND: candidate-policy G-FORK
// Same-DID writer-ID collision (forced: Alice-2 reuses Alice-1's writer ID, as a cloned or
// restored device state would). Both post seq 1 with different text.
// - RejectLater: host keeps the first, refuses the second with the host board byte-identical;
//   Alice-1 continues under its writer; Alice-2 redraws and re-posts.
// - FreezeWriter: host refuses the second AND freezes the writer (content state unchanged,
//   only the frozen set grows); Alice-1's next post is refused too, so BOTH devices must
//   redraw. Any phrase holder can therefore freeze a sibling's writer (cf. W2-R Q19).
// Members never receive the conflicting post under either candidate; all converge.
#[test]
fn same_did_writer_collision_reject_later_vs_freeze_writer() {
    for fork in [ForkPolicy::RejectLater, ForkPolicy::FreezeWriter] {
        let mut w = World::new(Cfg {
            fork,
            ..Cfg::default()
        });
        let shared = w.d[A1].w.writer;
        let alice = w.d[A2].id.clone();
        w.d[A2].w = Writer::new(alice, shared);
        w.post(A1, "phone");
        w.post(A2, "cloned laptop");
        w.reconnect(A1).unwrap();
        let (full, content) = (
            w.d[HOST].board.snapshot(),
            w.d[HOST].board.content_snapshot(),
        );
        let hep = w.reach_host(A2).unwrap();
        assert_eq!(w.upload(A2, hep), vec![Verdict::Rejected(Why::SameDidFork)]);
        assert_eq!(w.d[HOST].board.content_snapshot(), content);
        match fork {
            ForkPolicy::RejectLater => assert_eq!(w.d[HOST].board.snapshot(), full),
            ForkPolicy::FreezeWriter => {
                assert_eq!(w.d[HOST].board.frozen, [shared].into())
            }
        }
        for i in [A1, A2, BOB, CAROL] {
            w.reconnect(i).unwrap();
        }
        assert!(!w.d[BOB].board.texts().contains("cloned laptop"));
        w.post(A1, "phone again");
        let hep = w.reach_host(A1).unwrap();
        let v = w.upload(A1, hep);
        match fork {
            ForkPolicy::RejectLater => assert_eq!(v, vec![Verdict::Accepted]),
            ForkPolicy::FreezeWriter => {
                assert_eq!(v, vec![Verdict::Rejected(Why::WriterFrozen)]);
                w.redraw_writer(A1);
            }
        }
        w.redraw_writer(A2);
        for i in [A1, A2, BOB, CAROL] {
            w.reconnect(i).unwrap();
        }
        w.assert_converged(&ALL);
        assert_eq!(
            w.d[CAROL].board.texts(),
            ["phone", "cloned laptop", "phone again"]
                .map(String::from)
                .into()
        );
    }
}

// KIND: invariant
// U3 under U7 + U4 revised: Alice-1 requests a 1:1 chat with Carol via the host. The sealed
// request carries no endpoint and is DID-signed inside the ciphertext; the host routes it by
// header only. A tampered or replayed request is dropped with Carol's state unchanged. Carol
// accepts; the acceptance (DID-signed, sealed to Alice) carries Carol's private endpoint;
// Alice-1 dials Carol directly with no host frame, and Carol learns Alice's private endpoint
// from the DID-signed direct hello. The host and Bob never see either private endpoint.
// Group posts keep working alongside. (Alice-2 drops the acceptance: it has no outgoing
// record — linked-device sync of chat state is out of scope.)
#[test]
fn chat_request_via_host_with_did_signed_content_then_direct() {
    let mut w = world();
    let (alice, carol) = (w.did(A1), w.did(CAROL));
    w.post(A1, "group still works");
    w.reconnect(A1).unwrap();
    let rid = w.chat_request(A1, &carol).unwrap();
    let to_carol: Vec<Frame> = w
        .frames
        .iter()
        .filter(|f| f.kind == "chat" && f.to == CAROL)
        .cloned()
        .collect();
    assert_eq!(to_carol.len(), 1);
    assert_eq!(
        w.d[CAROL].incoming[&rid].0, alice,
        "carol holds the request from alice"
    );
    // The request plaintext has no endpoint of Alice.
    let req: Sealed = serde_json::from_slice(&to_carol[0].bytes).unwrap();
    let plain = open(&StaticSecret::from(w.d[CAROL].req_secret), &req).unwrap();
    assert!(plain.verify());
    assert!(matches!(plain.inner, Inner::Request { .. }));
    let plain_bytes = serde_json::to_vec(&plain).unwrap();
    assert!(!leaks_key(&plain_bytes, &tkey(&w.d[A1].private_key)));
    assert!(!leaks_key(&plain_bytes, &tkey(&w.d[A1].group_key)));
    // Tampered and replayed requests: dropped, Carol unchanged.
    let snap = w.d[CAROL].chat_snapshot();
    let mut t = req.clone();
    t.ciphertext[3] ^= 1;
    let mut rerouted = req.clone();
    rerouted.header.from = w.did(BOB);
    for bytes in [
        serde_json::to_vec(&t).unwrap(),
        serde_json::to_vec(&rerouted).unwrap(),
        to_carol[0].bytes.clone(),
    ] {
        assert_eq!(w.chat_inbox(CAROL, &bytes), ChatIn::Dropped);
        assert_eq!(w.d[CAROL].chat_snapshot(), snap);
    }
    let mut answered = w.chat_accept(CAROL, &rid);
    answered.sort_by_key(|(i, _)| *i);
    assert_eq!(
        answered,
        vec![
            (
                A1,
                ChatIn::Accepted {
                    from: carol.clone()
                }
            ),
            (A2, ChatIn::Dropped)
        ]
    );
    assert_eq!(w.d[A1].private_book[&carol], w.d[CAROL].private_ep());
    let n = w.frames.len();
    w.direct(A1, &carol, "hi carol, directly").unwrap();
    assert!(w.frames[n..].iter().all(|f| f.from != HOST && f.to != HOST));
    assert_eq!(w.d[CAROL].private_book[&alice], w.d[A1].private_ep());
    assert_eq!(w.dial(CAROL, &alice), Ok(w.d[A1].private_ep()));
    // Host and bystander never see a private endpoint.
    for who in [HOST, BOB] {
        let mut hay = w.received(who);
        if who == HOST {
            hay.extend(w.host_snapshot());
        }
        for i in [A1, CAROL] {
            assert!(
                !leaks_key(&hay, &tkey(&w.d[i].private_key)),
                "{who} saw {i}"
            );
        }
    }
    w.assert_converged(&[HOST, A1]);
}

// KIND: limitation
// U7 consequences the host-only model does NOT hide: (1) the host holds every member device's
// group endpoint and route slot, so it can count each DID's devices and see when each device
// rotates; (2) it learns who requested a chat with whom and when (header metadata); (3)
// members learn each DID's device count and per-device activity from the random writer IDs
// in content, even though they never learn device keys or endpoints.
#[test]
fn host_holds_all_member_endpoints_and_metadata_members_see_writer_ids() {
    let mut w = world();
    let (alice, carol) = (w.did(A1), w.did(CAROL));
    let alice_entries = w
        .directory
        .values()
        .filter(|n| n.claims.owner == alice)
        .count();
    assert_eq!(alice_entries, 2);
    for i in [A1, A2, BOB, CAROL] {
        assert_eq!(
            w.directory[&dir_key(&w.did(i), w.d[i].slot)]
                .claims
                .endpoint,
            w.d[i].group_ep()
        );
    }
    w.chat_request(BOB, &carol).unwrap();
    let meta: Vec<Header> = w
        .frames
        .iter()
        .filter(|f| f.to == HOST && f.kind == "chat")
        .map(|f| serde_json::from_slice::<Sealed>(&f.bytes).unwrap().header)
        .collect();
    assert_eq!(
        (meta[0].from.as_str(), meta[0].to.as_str()),
        (w.did(BOB).as_str(), carol.as_str())
    );
    w.post(A1, "one");
    w.post(A2, "two");
    for i in [A1, A2, CAROL] {
        w.reconnect(i).unwrap();
    }
    let writers_seen_by_carol: BTreeSet<u64> = w.d[CAROL]
        .board
        .accepted
        .values()
        .filter(|p| p.body.author == alice)
        .map(|p| p.body.writer)
        .collect();
    assert_eq!(
        writers_seen_by_carol.len(),
        2,
        "carol can count alice's devices"
    );
}
