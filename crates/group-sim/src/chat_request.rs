//! W2-Q: 1:1 chat request routed through the group host (U3). Blocking is visible (U5
//! revised); what a blocker returns to a chat request is open (section 8b Q4) and modelled as
//! the `BlockReply` candidates. R1b removed the tests that asserted the rejected silent option.
//!
//! Status: proposal — not approved. Simulation only; no production API, transport or UI.
//!
//! Model: every member publishes a DID-signed X25519 *request key* in the group (the host
//! keeps a directory of the latest note per DID). A requester seals a chat request to the
//! target's request key and hands it to the host, which only sees routing metadata
//! (`Header`). If the target accepts, it seals an answer back to the requester's request
//! key. The private endpoints travel only inside the sealed, DID-signed payloads; after
//! acceptance the two chat directly and the host is not involved.
//!
//! The "host" here is the mailbox role of the group's relay replica (the `Replica` used as a
//! host in `tests.rs`/`reachability.rs`); it is modelled as a separate struct because chat
//! requests are not group envelopes and must never enter the shared Loro document.
//!
//! !!! SIMULATION CONSTRUCTION, NOT AN APPROVED PRODUCTION SCHEME !!!
//! Sealing = ephemeral X25519 -> blake3::derive_key (domain-separated context, input =
//! shared || ephemeral_pub || recipient_pub) -> AES-256-GCM with the serialized routing header
//! as AAD and a fixed zero nonce (safe only because every seal uses a fresh ephemeral key, so
//! the derived key is single-use). The sender's DID signature is *inside* the ciphertext, and
//! the signed payload names group, sender, recipient and key sequence, so a re-encrypting
//! insider cannot re-address it. Plaintexts are padded to a fixed bucket so accept/decline
//! sizes match. Production must pick a reviewed construction (e.g. HPKE RFC 9180 base or
//! auth mode, or a Noise pattern) — OPEN QUESTION, not decided here.
//!
//! Ephemeral keys, request IDs and request-key secrets come from a deterministic blake3
//! "RNG" keyed by fixture seeds. Never production randomness.
use super::*;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use x25519_dalek::{PublicKey, StaticSecret};

const SIM: &str = "tinline/group-sim/chat-request/not-production/v1";
const KDF_CONTEXT: &str = "tinline/group-sim/chat-request/not-production/v1 aead-key";
const RNG_CONTEXT: &str = "tinline/group-sim/chat-request/not-production/v1 sim-rng";
const GROUP: &str = "w2q-group";
// Requester-side deadline after which an unanswered request is reported as `NoAnswer`.
// Fixture value; the real value is a product/timing question (G-TIME).
const ANSWER_TIMEOUT: u64 = 10;
// Fixed plaintext bucket so the host cannot tell accept from decline by size.
const PAD: usize = 1024;

// ---------------------------------------------------------------- helpers

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn sim_random(seed: u8, label: &str, counter: u64) -> [u8; 32] {
    let mut input = vec![seed];
    input.extend_from_slice(label.as_bytes());
    input.push(0);
    input.extend_from_slice(&counter.to_le_bytes());
    blake3::derive_key(RNG_CONTEXT, &input)
}
fn signing_bytes(purpose: &str, body: &impl Serialize) -> Vec<u8> {
    [
        SIM.as_bytes(),
        b"\0",
        purpose.as_bytes(),
        b"\0",
        &serde_json::to_vec(body).unwrap(),
    ]
    .concat()
}
fn did_sign(id: &Identity, purpose: &str, body: &impl Serialize) -> Vec<u8> {
    id.sign(&signing_bytes(purpose, body)).to_vec()
}
fn did_verify(did: &str, purpose: &str, body: &impl Serialize, sig: &[u8]) -> bool {
    let (Some(public), Ok(sig)) = (
        identity::public_key_from_did(did),
        <[u8; 64]>::try_from(sig),
    ) else {
        return false;
    };
    identity::verify(&public, &signing_bytes(purpose, body), &sig)
}
fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

// ---------------------------------------------------------------- wire types

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RequestKeyClaim {
    group: String,
    owner: String,
    seq: u64,
    x25519: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SignedRequestKey {
    claim: RequestKeyClaim,
    sig: Vec<u8>,
}
impl SignedRequestKey {
    fn verify(&self) -> bool {
        did_verify(&self.claim.owner, "request-key", &self.claim, &self.sig)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum WireKind {
    Request,
    Answer,
}
// Everything the host can read. Authenticated as AEAD associated data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Header {
    group: String,
    kind: WireKind,
    from: String,
    to: String,
    key_seq: u64,
    ephemeral: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Wire {
    header: Header,
    ciphertext: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Inner {
    Request {
        group: String,
        from: String,
        to: String,
        request_id: String,
        key_seq: u64,
        endpoint: String,
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
    Decline {
        group: String,
        from: String,
        to: String,
        request_id: String,
        request_digest: String,
    },
    // Candidate 8b Q4 (`BlockReply::SealedNotice`): "you are blocked", bound to the request.
    Blocked {
        group: String,
        from: String,
        to: String,
        request_id: String,
        request_digest: String,
    },
}
impl Inner {
    fn from(&self) -> &str {
        match self {
            Inner::Request { from, .. }
            | Inner::Accept { from, .. }
            | Inner::Decline { from, .. }
            | Inner::Blocked { from, .. } => from,
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
        did_verify(self.inner.from(), "chat-inner", &self.inner, &self.sig)
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
    [SIM.as_bytes(), b"\0aad\0", &serde_json::to_vec(h).unwrap()].concat()
}
// SIMULATION construction (see module docs). Header.ephemeral is overwritten.
fn seal(
    mut header: Header,
    recipient: &[u8; 32],
    eph_seed: [u8; 32],
    payload: &SignedInner,
) -> Wire {
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
                msg: &payload.padded(),
                aad: &aad(&header),
            },
        )
        .unwrap();
    Wire { header, ciphertext }
}
fn open(secret: &StaticSecret, wire: &Wire) -> Option<SignedInner> {
    let recipient = PublicKey::from(secret).to_bytes();
    let shared = secret.diffie_hellman(&PublicKey::from(wire.header.ephemeral));
    if !shared.was_contributory() {
        return None;
    }
    let key = aead_key(shared.as_bytes(), &wire.header.ephemeral, &recipient);
    let plain = Aes256Gcm::new_from_slice(&key)
        .unwrap()
        .decrypt(
            Nonce::from_slice(&[0u8; 12]),
            Payload {
                msg: &wire.ciphertext,
                aad: &aad(&wire.header),
            },
        )
        .ok()?;
    unpad(&plain)
}

// What the host stores and forwards.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Relay {
    Sealed(Wire),
    // Candidate G-RETIREMENT reply: the target's current public key note, unencrypted.
    KeyHint {
        from: String,
        to: String,
        note: SignedRequestKey,
    },
}
impl Relay {
    fn route(&self) -> (&str, &str) {
        match self {
            Relay::Sealed(w) => (&w.header.from, &w.header.to),
            Relay::KeyHint { from, to, .. } => (from, to),
        }
    }
    fn wire(&self) -> &Wire {
        match self {
            Relay::Sealed(w) => w,
            Relay::KeyHint { .. } => panic!("not a sealed relay"),
        }
    }
}

// ---------------------------------------------------------------- policies (open gates)

// G-BLOCK candidate: does a user "decline" send a signed refusal, or stay silent?
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum DeclinePolicy {
    Explicit,
    Silent,
}
// G-BLOCK candidate, section 8b Q4 (open): what a blocker returns to a chat request from a
// DID it blocked. U5 revised decides that blocking is visible; it does not decide this path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum BlockReply {
    // No reply over the host: at this layer the requester sees `NoAnswer`, as for offline.
    Drop,
    // A sealed, DID-signed "blocked" answer of the same kind and size as accept/decline.
    SealedNotice,
}
// G-RETIREMENT candidate: what a target does with a request sealed to an unusable key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum RetiredKeyPolicy {
    Drop,
    HintCurrentKey,
}
// G-REQUEST-RATE (new, proposed gate): values are fixtures, not decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct RateLimit {
    max: usize,
    window: u64,
}
#[derive(Clone, Copy, Debug, Serialize)]
struct Policy {
    decline: DeclinePolicy,
    block: BlockReply,
    retired_key: RetiredKeyPolicy,
    // Old request key stays usable for decrypt until rotate_time + grace (exclusive).
    grace: u64,
    target_rate: Option<RateLimit>,
}
fn policy() -> Policy {
    Policy {
        decline: DeclinePolicy::Silent,
        block: BlockReply::Drop,
        retired_key: RetiredKeyPolicy::Drop,
        grace: 5,
        target_rate: None,
    }
}

// ---------------------------------------------------------------- member

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
enum Answer {
    Accepted(String),
    Refused,
    Blocked,
    KeyStale,
}
// What the requester can observe about one of its requests.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Observation {
    Pending,
    Accepted(String),
    Refused,
    Blocked,
    KeyStale,
    NoAnswer,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropReason {
    Misaddressed,
    StaleKey,
    Undecryptable,
    BadSignature,
    BadKeyNote,
    Replay,
    Blocked,
    RateLimited,
    UnknownRequest,
}
// Local-only receive result (never sent back to the peer).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Delivery {
    NewRequest(String),
    Answered(String),
    KeyHint(String),
    Dropped(DropReason),
}
#[derive(Clone, Serialize)]
struct KeySlot {
    secret: Option<[u8; 32]>,
    public: [u8; 32],
    retire_at: Option<u64>,
}
#[derive(Clone, Serialize)]
struct Outgoing {
    to: String,
    sent_at: u64,
    key_seq: u64,
    digest: String,
    answer: Option<Answer>,
}
#[derive(Clone, Serialize)]
struct Incoming {
    from: String,
    endpoint: String,
    digest: String,
    received_at: u64,
}
#[derive(Clone, Serialize)]
struct MemberState {
    rng: u64,
    private_endpoint: String,
    group_endpoint: String,
    current_seq: u64,
    keys: BTreeMap<u64, KeySlot>,
    known_keys: BTreeMap<String, SignedRequestKey>,
    outgoing: BTreeMap<String, Outgoing>,
    incoming: BTreeMap<String, Incoming>,
    seen: BTreeSet<String>,
    contacts: BTreeMap<String, String>,
    blocked: BTreeSet<String>,
    request_times: BTreeMap<String, Vec<u64>>,
    direct_inbox: Vec<(String, String)>,
    policy: Policy,
}
#[derive(Clone, Serialize)]
struct DirectBody {
    to_endpoint: String,
    from: String,
    text: String,
}
#[derive(Clone, Serialize)]
struct DirectMsg {
    body: DirectBody,
    sig: Vec<u8>,
}
struct Member {
    id: Identity,
    seed: u8,
    state: MemberState,
}
impl Member {
    fn new(seed: u8, name: &str, policy: Policy) -> Self {
        // Reuse the shared identity fixture (real DID + Ed25519).
        let Author { id, .. } = Author::new(seed, GROUP);
        let tag = |label: &str| hex(&sim_random(seed, label, 0)[..6]);
        let mut m = Member {
            id,
            seed,
            state: MemberState {
                rng: 0,
                private_endpoint: format!("private-ep-{name}-{}", tag("private-endpoint")),
                group_endpoint: format!("group-ep-{name}-{}", tag("group-endpoint")),
                current_seq: 0,
                keys: BTreeMap::new(),
                known_keys: BTreeMap::new(),
                outgoing: BTreeMap::new(),
                incoming: BTreeMap::new(),
                seen: BTreeSet::new(),
                contacts: BTreeMap::new(),
                blocked: BTreeSet::new(),
                request_times: BTreeMap::new(),
                direct_inbox: vec![],
                policy,
            },
        };
        m.add_key();
        m
    }
    fn did(&self) -> String {
        self.id.did().to_string()
    }
    fn snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(&self.state).unwrap()
    }
    fn next_random(&mut self, label: &str) -> [u8; 32] {
        self.state.rng += 1;
        sim_random(self.seed, label, self.state.rng)
    }
    fn add_key(&mut self) {
        let secret = self.next_random("request-key");
        let public = PublicKey::from(&StaticSecret::from(secret)).to_bytes();
        self.state.current_seq += 1;
        self.state.keys.insert(
            self.state.current_seq,
            KeySlot {
                secret: Some(secret),
                public,
                retire_at: None,
            },
        );
    }
    fn key_note(&self) -> SignedRequestKey {
        let claim = RequestKeyClaim {
            group: GROUP.into(),
            owner: self.did(),
            seq: self.state.current_seq,
            x25519: self.state.keys[&self.state.current_seq].public,
        };
        let sig = did_sign(&self.id, "request-key", &claim);
        SignedRequestKey { claim, sig }
    }
    fn rotate_request_key(&mut self, now: u64) -> SignedRequestKey {
        let grace = self.state.policy.grace;
        for slot in self.state.keys.values_mut() {
            slot.retire_at.get_or_insert(now + grace);
        }
        self.add_key();
        self.key_note()
    }
    // Erases secrets whose retirement time has passed.
    fn tick(&mut self, now: u64) {
        for slot in self.state.keys.values_mut() {
            if slot.retire_at.is_some_and(|r| now >= r) {
                slot.secret = None;
            }
        }
    }
    // Never returns a retired key, even if `tick` has not erased it yet.
    fn usable_secret(&self, seq: u64, now: u64) -> Option<StaticSecret> {
        let slot = self.state.keys.get(&seq)?;
        if slot.retire_at.is_some_and(|r| now >= r) {
            return None;
        }
        slot.secret.map(StaticSecret::from)
    }
    fn learn_key(&mut self, note: SignedRequestKey) -> bool {
        if !note.verify() || note.claim.group != GROUP || note.claim.owner == self.did() {
            return false;
        }
        if let Some(k) = self.state.known_keys.get(&note.claim.owner) {
            if k.claim.seq >= note.claim.seq {
                return false;
            }
        }
        self.state.known_keys.insert(note.claim.owner.clone(), note);
        true
    }
    fn sync_keys(&mut self, host: &Host) {
        for note in host.state.directory.values() {
            self.learn_key(note.clone());
        }
    }
    fn block(&mut self, did: &str) {
        self.state.blocked.insert(did.into());
    }
    fn request(&mut self, to: &str, now: u64) -> Option<(String, Relay)> {
        let note = self.state.known_keys.get(to)?.clone();
        let request_id = hex(&self.next_random("request-id")[..16]);
        let signed = SignedInner::new(
            &self.id,
            Inner::Request {
                group: GROUP.into(),
                from: self.did(),
                to: to.into(),
                request_id: request_id.clone(),
                key_seq: note.claim.seq,
                endpoint: self.state.private_endpoint.clone(),
                sent_at: now,
            },
        );
        let header = Header {
            group: GROUP.into(),
            kind: WireKind::Request,
            from: self.did(),
            to: to.into(),
            key_seq: note.claim.seq,
            ephemeral: [0; 32],
        };
        let eph = self.next_random("ephemeral");
        let wire = seal(header, &note.claim.x25519, eph, &signed);
        self.state.outgoing.insert(
            request_id.clone(),
            Outgoing {
                to: to.into(),
                sent_at: now,
                key_seq: note.claim.seq,
                digest: signed.digest(),
                answer: None,
            },
        );
        Some((request_id, Relay::Sealed(wire)))
    }
    fn answer(&mut self, request_id: &str, accept: bool) -> Option<Relay> {
        let inc = self.state.incoming.remove(request_id)?;
        if !accept && self.state.policy.decline == DeclinePolicy::Silent {
            return None;
        }
        let note = self.state.known_keys.get(&inc.from)?.clone();
        let inner = if accept {
            self.state
                .contacts
                .insert(inc.from.clone(), inc.endpoint.clone());
            Inner::Accept {
                group: GROUP.into(),
                from: self.did(),
                to: inc.from.clone(),
                request_id: request_id.into(),
                request_digest: inc.digest,
                endpoint: self.state.private_endpoint.clone(),
            }
        } else {
            Inner::Decline {
                group: GROUP.into(),
                from: self.did(),
                to: inc.from.clone(),
                request_id: request_id.into(),
                request_digest: inc.digest,
            }
        };
        let signed = SignedInner::new(&self.id, inner);
        let header = Header {
            group: GROUP.into(),
            kind: WireKind::Answer,
            from: self.did(),
            to: inc.from,
            key_seq: note.claim.seq,
            ephemeral: [0; 32],
        };
        let eph = self.next_random("ephemeral");
        Some(Relay::Sealed(seal(
            header,
            &note.claim.x25519,
            eph,
            &signed,
        )))
    }
    fn accept(&mut self, request_id: &str) -> Option<Relay> {
        self.answer(request_id, true)
    }
    fn decline(&mut self, request_id: &str) -> Option<Relay> {
        self.answer(request_id, false)
    }
    fn observe(&self, request_id: &str, now: u64) -> Observation {
        let o = &self.state.outgoing[request_id];
        match &o.answer {
            Some(Answer::Accepted(ep)) => Observation::Accepted(ep.clone()),
            Some(Answer::Refused) => Observation::Refused,
            Some(Answer::Blocked) => Observation::Blocked,
            Some(Answer::KeyStale) => Observation::KeyStale,
            None if now >= o.sent_at + ANSWER_TIMEOUT => Observation::NoAnswer,
            None => Observation::Pending,
        }
    }
    // All drops return before any state mutation.
    fn receive(&mut self, relay: Relay, now: u64) -> (Delivery, Vec<Relay>) {
        let dropped = |r| (Delivery::Dropped(r), vec![]);
        let me = self.did();
        let wire = match relay {
            Relay::KeyHint { from, to, note } => {
                if to != me || note.claim.owner != from {
                    return dropped(DropReason::Misaddressed);
                }
                let stale: Vec<String> = self
                    .state
                    .outgoing
                    .iter()
                    .filter(|(_, o)| {
                        o.to == from && o.answer.is_none() && o.key_seq < note.claim.seq
                    })
                    .map(|(k, _)| k.clone())
                    .collect();
                if stale.is_empty() {
                    return dropped(DropReason::UnknownRequest);
                }
                if !self.learn_key(note) {
                    return dropped(DropReason::BadKeyNote);
                }
                for rid in stale {
                    self.state.outgoing.get_mut(&rid).unwrap().answer = Some(Answer::KeyStale);
                }
                return (Delivery::KeyHint(from), vec![]);
            }
            Relay::Sealed(w) => w,
        };
        let h = &wire.header;
        if h.group != GROUP || h.to != me {
            return dropped(DropReason::Misaddressed);
        }
        let Some(secret) = self.usable_secret(h.key_seq, now) else {
            let mut replies = vec![];
            if h.kind == WireKind::Request
                && self.state.policy.retired_key == RetiredKeyPolicy::HintCurrentKey
                && !self.state.blocked.contains(&h.from)
            {
                replies.push(Relay::KeyHint {
                    from: me,
                    to: h.from.clone(),
                    note: self.key_note(),
                });
            }
            return (Delivery::Dropped(DropReason::StaleKey), replies);
        };
        let Some(signed) = open(&secret, &wire) else {
            return dropped(DropReason::Undecryptable);
        };
        if !signed.verify() {
            return dropped(DropReason::BadSignature);
        }
        let digest = signed.digest();
        match signed.inner {
            Inner::Request {
                group,
                from,
                to,
                request_id,
                key_seq,
                endpoint,
                ..
            } => {
                if h.kind != WireKind::Request
                    || group != GROUP
                    || to != me
                    || from != h.from
                    || key_seq != h.key_seq
                {
                    return dropped(DropReason::Misaddressed);
                }
                if self.state.seen.contains(&request_id) {
                    return dropped(DropReason::Replay);
                }
                // Blocked requester: no state change. Whether a reply goes back is the open
                // 8b Q4 candidate (`BlockReply`).
                if self.state.blocked.contains(&from) {
                    let replies = match self.state.policy.block {
                        BlockReply::Drop => vec![],
                        BlockReply::SealedNotice => self
                            .blocked_answer(&from, &request_id, &digest)
                            .into_iter()
                            .collect(),
                    };
                    return (Delivery::Dropped(DropReason::Blocked), replies);
                }
                if let Some(limit) = self.state.policy.target_rate {
                    let recent = self.state.request_times.get(&from).map_or(0, |t| {
                        t.iter().filter(|&&at| at + limit.window > now).count()
                    });
                    if recent >= limit.max {
                        return dropped(DropReason::RateLimited);
                    }
                }
                self.state.seen.insert(request_id.clone());
                self.state
                    .request_times
                    .entry(from.clone())
                    .or_default()
                    .push(now);
                self.state.incoming.insert(
                    request_id.clone(),
                    Incoming {
                        from,
                        endpoint,
                        digest,
                        received_at: now,
                    },
                );
                (Delivery::NewRequest(request_id), vec![])
            }
            Inner::Accept {
                ref group,
                ref from,
                ref to,
                ref request_id,
                ref request_digest,
                ..
            }
            | Inner::Decline {
                ref group,
                ref from,
                ref to,
                ref request_id,
                ref request_digest,
            }
            | Inner::Blocked {
                ref group,
                ref from,
                ref to,
                ref request_id,
                ref request_digest,
            } => {
                if h.kind != WireKind::Answer || group != GROUP || *to != me || *from != h.from {
                    return dropped(DropReason::Misaddressed);
                }
                let Some(o) = self.state.outgoing.get(request_id) else {
                    return dropped(DropReason::UnknownRequest);
                };
                if o.to != *from || o.digest != *request_digest {
                    return dropped(DropReason::Misaddressed);
                }
                if o.answer.is_some() {
                    return dropped(DropReason::Replay);
                }
                let rid = request_id.clone();
                let answer = match &signed.inner {
                    Inner::Accept { endpoint, from, .. } => {
                        self.state.contacts.insert(from.clone(), endpoint.clone());
                        Answer::Accepted(endpoint.clone())
                    }
                    Inner::Blocked { .. } => Answer::Blocked,
                    _ => Answer::Refused,
                };
                self.state.outgoing.get_mut(&rid).unwrap().answer = Some(answer);
                (Delivery::Answered(rid), vec![])
            }
        }
    }
    // Candidate 8b Q4: the sealed "blocked" answer. Read-only: the ephemeral key is derived
    // from the request ID instead of the member RNG, so refusing leaves the state unchanged.
    fn blocked_answer(&self, to: &str, request_id: &str, digest: &str) -> Option<Relay> {
        let note = self.state.known_keys.get(to)?;
        let signed = SignedInner::new(
            &self.id,
            Inner::Blocked {
                group: GROUP.into(),
                from: self.did(),
                to: to.into(),
                request_id: request_id.into(),
                request_digest: digest.into(),
            },
        );
        let header = Header {
            group: GROUP.into(),
            kind: WireKind::Answer,
            from: self.did(),
            to: to.into(),
            key_seq: note.claim.seq,
            ephemeral: [0; 32],
        };
        let counter = u64::from_le_bytes(
            blake3::hash(request_id.as_bytes()).as_bytes()[..8]
                .try_into()
                .unwrap(),
        );
        let eph = sim_random(self.seed, "blocked-ephemeral", counter);
        Some(Relay::Sealed(seal(
            header,
            &note.claim.x25519,
            eph,
            &signed,
        )))
    }
    // Test-only insider view: decrypt a wire with this member's key (a recipient can always
    // do this; used to model a malicious recipient re-encrypting what it received).
    fn insider_open(&self, wire: &Wire, now: u64) -> Option<SignedInner> {
        open(&self.usable_secret(wire.header.key_seq, now)?, wire)
    }
    fn direct_send(&self, to: &str, text: &str) -> Option<DirectMsg> {
        let body = DirectBody {
            to_endpoint: self.state.contacts.get(to)?.clone(),
            from: self.did(),
            text: text.into(),
        };
        let sig = did_sign(&self.id, "direct", &body);
        Some(DirectMsg { body, sig })
    }
    fn direct_receive(&mut self, m: &DirectMsg) -> bool {
        let b = &m.body;
        if b.to_endpoint != self.state.private_endpoint
            || !self.state.contacts.contains_key(&b.from)
            || self.state.blocked.contains(&b.from)
            || !did_verify(&b.from, "direct", b, &m.sig)
        {
            return false;
        }
        self.state
            .direct_inbox
            .push((b.from.clone(), b.text.clone()));
        true
    }
}
// Direct network: delivers by private endpoint only; the host is not a parameter.
fn route_direct(msg: &DirectMsg, members: &mut [&mut Member]) -> bool {
    members
        .iter_mut()
        .find(|m| m.state.private_endpoint == msg.body.to_endpoint)
        .is_some_and(|m| m.direct_receive(msg))
}

// ---------------------------------------------------------------- host

#[derive(Debug, PartialEq, Eq)]
enum HostRefusal {
    NotMember,
    RateLimited,
}
#[derive(Clone, Default, Serialize)]
struct HostState {
    // U2: the host legitimately knows group endpoints (positive control for leak scans).
    group_endpoints: BTreeMap<String, String>,
    directory: BTreeMap<String, SignedRequestKey>,
    mailboxes: BTreeMap<String, Vec<Relay>>,
    // key = "from\nto", accepted request times.
    rate: BTreeMap<String, Vec<u64>>,
    relayed: Vec<Relay>,
}
#[derive(Default)]
struct Host {
    state: HostState,
    rate: Option<RateLimit>,
}
impl Host {
    fn snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(&self.state).unwrap()
    }
    fn join(&mut self, m: &Member) {
        self.state
            .group_endpoints
            .insert(m.did(), m.state.group_endpoint.clone());
        assert!(self.publish(m.key_note()));
    }
    fn publish(&mut self, note: SignedRequestKey) -> bool {
        if !note.verify()
            || note.claim.group != GROUP
            || !self.state.group_endpoints.contains_key(&note.claim.owner)
            || self
                .state
                .directory
                .get(&note.claim.owner)
                .is_some_and(|k| k.claim.seq >= note.claim.seq)
        {
            return false;
        }
        self.state.directory.insert(note.claim.owner.clone(), note);
        true
    }
    fn submit(&mut self, relay: Relay, now: u64) -> Result<(), HostRefusal> {
        let (from, to) = relay.route();
        let (from, to) = (from.to_string(), to.to_string());
        if !self.state.group_endpoints.contains_key(&from)
            || !self.state.group_endpoints.contains_key(&to)
        {
            return Err(HostRefusal::NotMember);
        }
        let is_request = matches!(&relay, Relay::Sealed(w) if w.header.kind == WireKind::Request);
        let key = format!("{from}\n{to}");
        if let (true, Some(limit)) = (is_request, self.rate) {
            let recent = self.state.rate.get(&key).map_or(0, |t| {
                t.iter().filter(|&&at| at + limit.window > now).count()
            });
            if recent >= limit.max {
                return Err(HostRefusal::RateLimited);
            }
        }
        if is_request {
            self.state.rate.entry(key).or_default().push(now);
        }
        self.state.relayed.push(relay.clone());
        self.state.mailboxes.entry(to).or_default().push(relay);
        Ok(())
    }
    fn fetch(&mut self, did: &str) -> Vec<Relay> {
        self.state.mailboxes.remove(did).unwrap_or_default()
    }
    // Leak scan: serialized state AND the raw bytes of every stored/relayed ciphertext.
    fn leaks(&self, needle: &str) -> bool {
        let n = needle.as_bytes();
        contains(&self.snapshot(), n)
            || self
                .state
                .relayed
                .iter()
                .chain(self.state.mailboxes.values().flatten())
                .any(|r| match r {
                    Relay::Sealed(w) => contains(&w.ciphertext, n),
                    Relay::KeyHint { .. } => false,
                })
    }
}

// ---------------------------------------------------------------- fixtures

struct World {
    host: Host,
    alice: Member,
    bob: Member,
    carol: Member,
}
fn world_with(alice: Policy, bob: Policy, carol: Policy) -> World {
    let mut w = World {
        host: Host::default(),
        alice: Member::new(31, "alice", alice),
        bob: Member::new(32, "bob", bob),
        carol: Member::new(33, "carol", carol),
    };
    for m in [&w.alice, &w.bob, &w.carol] {
        w.host.join(m);
    }
    for m in [&mut w.alice, &mut w.bob, &mut w.carol] {
        m.sync_keys(&w.host);
    }
    w
}
fn world(p: Policy) -> World {
    world_with(p, p, p)
}
fn pump(host: &mut Host, m: &mut Member, now: u64) -> Vec<Delivery> {
    let mut out = vec![];
    for relay in host.fetch(&m.did()) {
        let (d, replies) = m.receive(relay, now);
        for r in replies {
            host.submit(r, now).unwrap();
        }
        out.push(d);
    }
    out
}
fn send(host: &mut Host, from: &mut Member, to: &str, now: u64) -> String {
    let (rid, relay) = from.request(to, now).unwrap();
    host.submit(relay, now).unwrap();
    rid
}
// Delivers `relay` directly and asserts it is dropped for `reason` with state unchanged.
fn assert_rejected(m: &mut Member, relay: Relay, now: u64, reason: DropReason) {
    let before = m.snapshot();
    let (d, replies) = m.receive(relay, now);
    assert_eq!(d, Delivery::Dropped(reason));
    assert!(replies.is_empty());
    assert_eq!(before, m.snapshot(), "state changed on {reason:?}");
}
fn flip(mut w: Wire, i: usize) -> Wire {
    w.ciphertext[i] ^= 1;
    w
}

// ---------------------------------------------------------------- tests

// KIND: invariant
#[test]
fn request_is_e2e_and_host_sees_only_routing_metadata() {
    let mut w = world(policy());
    let (a, b) = (w.alice.did(), w.bob.did());
    let rid = send(&mut w.host, &mut w.alice, &b, 1);
    assert_eq!(
        pump(&mut w.host, &mut w.bob, 2),
        vec![Delivery::NewRequest(rid.clone())]
    );
    w.host.submit(w.bob.accept(&rid).unwrap(), 3).unwrap();
    assert_eq!(
        pump(&mut w.host, &mut w.alice, 4),
        vec![Delivery::Answered(rid.clone())]
    );
    // Leave one relay sitting in a mailbox too, so mailbox scanning is exercised.
    send(&mut w.host, &mut w.carol, &b, 5);

    for ep in [
        &w.alice.state.private_endpoint,
        &w.bob.state.private_endpoint,
        &w.carol.state.private_endpoint,
    ] {
        assert!(!w.host.leaks(ep), "private endpoint visible to host: {ep}");
    }
    assert!(
        !w.host.leaks(&rid),
        "request id is inside the ciphertext only"
    );
    // Positive controls: the scanner sees group endpoints (U2: host knows them) and would
    // see a private endpoint in an unencrypted relay of the same shape.
    assert!(w.host.leaks(&w.alice.state.group_endpoint));
    let mut leaky = Host::default();
    let mut plain = w.host.state.relayed[0].wire().clone();
    plain.ciphertext = SignedInner::new(
        &w.alice.id,
        Inner::Request {
            group: GROUP.into(),
            from: a.clone(),
            to: b.clone(),
            request_id: rid.clone(),
            key_seq: 1,
            endpoint: w.alice.state.private_endpoint.clone(),
            sent_at: 1,
        },
    )
    .padded();
    leaky.state.relayed.push(Relay::Sealed(plain));
    assert!(leaky.leaks(&w.alice.state.private_endpoint));

    // What the host can read is exactly the routing header; ciphertexts are fixed size.
    assert_eq!(w.host.state.relayed.len(), 3);
    for r in &w.host.state.relayed {
        let v = serde_json::to_value(r.wire()).unwrap();
        let fields: BTreeSet<_> = v["header"].as_object().unwrap().keys().cloned().collect();
        let expect: BTreeSet<String> = ["group", "kind", "from", "to", "key_seq", "ephemeral"]
            .map(String::from)
            .into();
        assert_eq!(fields, expect);
        assert_eq!(r.wire().ciphertext.len(), PAD + 16);
    }
    let h = &w.host.state.relayed[0].wire().header;
    assert_eq!(
        (h.from.as_str(), h.to.as_str(), h.kind),
        (a.as_str(), b.as_str(), WireKind::Request)
    );
}

// KIND: invariant
#[test]
fn accept_binds_private_endpoints_to_dids_then_direct_chat_skips_host() {
    let mut w = world(policy());
    let (a, b, c) = (w.alice.did(), w.bob.did(), w.carol.did());
    let rid = send(&mut w.host, &mut w.alice, &b, 1);
    pump(&mut w.host, &mut w.bob, 2);
    w.host.submit(w.bob.accept(&rid).unwrap(), 3).unwrap();
    pump(&mut w.host, &mut w.alice, 4);

    assert_eq!(
        w.alice.observe(&rid, 4),
        Observation::Accepted(w.bob.state.private_endpoint.clone())
    );
    assert_eq!(
        w.alice.state.contacts.get(&b),
        Some(&w.bob.state.private_endpoint)
    );
    assert_eq!(
        w.bob.state.contacts.get(&a),
        Some(&w.alice.state.private_endpoint)
    );
    assert!(w.carol.state.contacts.is_empty());
    assert!(w.bob.state.incoming.is_empty());

    // Direct chat afterwards: host state byte-identical throughout.
    let host_before = w.host.snapshot();
    let hi = w.alice.direct_send(&b, "hi bob").unwrap();
    assert!(route_direct(&hi, &mut [&mut w.bob, &mut w.carol]));
    let back = w.bob.direct_send(&a, "hi alice").unwrap();
    assert!(route_direct(&back, &mut [&mut w.alice, &mut w.carol]));
    assert_eq!(
        w.bob.state.direct_inbox,
        vec![(a.clone(), "hi bob".to_string())]
    );
    assert_eq!(
        w.alice.state.direct_inbox,
        vec![(b.clone(), "hi alice".to_string())]
    );
    assert_eq!(host_before, w.host.snapshot());

    // Endpoint is bound to the DID: Carol has no route, and a message to Bob's endpoint
    // claiming Alice's DID but signed by Carol is refused with Bob unchanged.
    assert!(w.carol.direct_send(&b, "x").is_none());
    let mut forged = hi.clone();
    forged.body.text = "forged".into();
    forged.sig = did_sign(&w.carol.id, "direct", &forged.body);
    let bob_before = w.bob.snapshot();
    assert!(!route_direct(&forged, &mut [&mut w.bob]));
    assert_eq!(bob_before, w.bob.snapshot());
    let mut from_carol = forged.clone();
    from_carol.body.from = c;
    from_carol.sig = did_sign(&w.carol.id, "direct", &from_carol.body);
    assert!(
        !route_direct(&from_carol, &mut [&mut w.bob]),
        "not a contact"
    );
    assert_eq!(bob_before, w.bob.snapshot());
}

// KIND: invariant
#[test]
fn host_tampering_is_detected_and_target_state_unchanged() {
    let mut w = world(policy());
    let (b, c) = (w.bob.did(), w.carol.did());
    // Populate Bob: one pending request from Carol already held.
    let carol_rid = send(&mut w.host, &mut w.carol, &b, 1);
    assert_eq!(
        pump(&mut w.host, &mut w.bob, 1),
        vec![Delivery::NewRequest(carol_rid)]
    );
    let (rid_b, to_bob) = w.alice.request(&b, 2).unwrap();
    let (_, to_carol) = w.alice.request(&c, 2).unwrap();
    let (wb, wc) = (to_bob.wire().clone(), to_carol.wire().clone());

    // Bit flips: ciphertext body, tag, and the header's ephemeral key.
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(flip(wb.clone(), 7)),
        3,
        DropReason::Undecryptable,
    );
    let last = wb.ciphertext.len() - 1;
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(flip(wb.clone(), last)),
        3,
        DropReason::Undecryptable,
    );
    let mut eph = wb.clone();
    eph.header.ephemeral[0] ^= 1;
    assert_rejected(&mut w.bob, Relay::Sealed(eph), 3, DropReason::Undecryptable);
    // Swapped ciphertext between targets (Bob's header, Carol's ciphertext).
    let mut swapped = wb.clone();
    swapped.ciphertext = wc.ciphertext.clone();
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(swapped),
        3,
        DropReason::Undecryptable,
    );
    // Re-addressed: Carol's request with its header rewritten to Bob.
    let mut readdressed = wc.clone();
    readdressed.header.to = b.clone();
    readdressed.header.key_seq = w.bob.state.current_seq;
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(readdressed),
        3,
        DropReason::Undecryptable,
    );
    // Spoofed sender in routing metadata (AAD-bound).
    let mut spoofed = wb.clone();
    spoofed.header.from = c.clone();
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(spoofed),
        3,
        DropReason::Undecryptable,
    );
    // Whole wire mis-delivered to Carol.
    assert_rejected(
        &mut w.carol,
        Relay::Sealed(wb.clone()),
        3,
        DropReason::Misaddressed,
    );
    // Header kind flipped Request -> Answer.
    let mut kind = wb.clone();
    kind.header.kind = WireKind::Answer;
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(kind),
        3,
        DropReason::Undecryptable,
    );

    // Positive control on the same populated replica.
    assert_eq!(w.bob.receive(to_bob, 3).0, Delivery::NewRequest(rid_b));
}

// KIND: invariant
#[test]
fn insider_reencrypted_request_is_rejected_by_signed_inner_binding() {
    let mut w = world(policy());
    let (a, b, c) = (w.alice.did(), w.bob.did(), w.carol.did());
    let (_, to_carol) = w.alice.request(&c, 1).unwrap();
    // Carol is a legitimate recipient, so she can decrypt Alice's signed request...
    let stolen = w.carol.insider_open(to_carol.wire(), 1).unwrap();
    assert!(stolen.verify());
    // ...and reseal it, unchanged, to Bob with a consistent header claiming Alice.
    let bob_key = w.carol.state.known_keys[&b].clone();
    let header = Header {
        group: GROUP.into(),
        kind: WireKind::Request,
        from: a.clone(),
        to: b.clone(),
        key_seq: bob_key.claim.seq,
        ephemeral: [0; 32],
    };
    let eph = w.carol.next_random("ephemeral");
    let forwarded = seal(header.clone(), &bob_key.claim.x25519, eph, &stolen);
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(forwarded),
        2,
        DropReason::Misaddressed,
    );
    // Editing the signed `to` breaks Alice's signature.
    let mut edited = stolen.clone();
    if let Inner::Request { to, key_seq, .. } = &mut edited.inner {
        *to = b.clone();
        *key_seq = bob_key.claim.seq;
    }
    let eph = w.carol.next_random("ephemeral");
    let resigned = seal(header, &bob_key.claim.x25519, eph, &edited);
    assert_rejected(
        &mut w.bob,
        Relay::Sealed(resigned),
        2,
        DropReason::BadSignature,
    );
    assert!(w.bob.state.incoming.is_empty() && w.bob.state.contacts.is_empty());
}

// KIND: invariant
#[test]
fn replayed_request_and_acceptance_are_rejected() {
    let mut w = world(policy());
    let b = w.bob.did();
    let (rid, req) = w.alice.request(&b, 1).unwrap();
    assert_eq!(
        w.bob.receive(req.clone(), 2).0,
        Delivery::NewRequest(rid.clone())
    );
    assert_rejected(&mut w.bob, req.clone(), 3, DropReason::Replay);
    let acc = w.bob.accept(&rid).unwrap();
    // Replay after the request was answered (incoming entry gone) is still a replay.
    assert_rejected(&mut w.bob, req, 4, DropReason::Replay);
    assert_eq!(
        w.alice.receive(acc.clone(), 5).0,
        Delivery::Answered(rid.clone())
    );
    assert_rejected(&mut w.alice, acc, 6, DropReason::Replay);
    assert_eq!(
        w.alice.observe(&rid, 6),
        Observation::Accepted(w.bob.state.private_endpoint.clone())
    );
}

// KIND: invariant
#[test]
fn acceptance_cannot_be_redirected_to_a_third_party() {
    let mut w = world(policy());
    let (a, b, c) = (w.alice.did(), w.bob.did(), w.carol.did());
    let alice_rid = send(&mut w.host, &mut w.alice, &b, 1);
    // Carol has her own outstanding request to Bob, so she has something to confuse.
    let carol_rid = send(&mut w.host, &mut w.carol, &b, 1);
    pump(&mut w.host, &mut w.bob, 2);
    let acc = w.bob.accept(&alice_rid).unwrap();
    let wa = acc.wire().clone();

    // Host delivers the whole acceptance to Carol.
    assert_rejected(&mut w.carol, acc.clone(), 3, DropReason::Misaddressed);
    // Host re-addresses the header to Carol.
    let mut readdressed = wa.clone();
    readdressed.header.to = c.clone();
    readdressed.header.key_seq = w.carol.state.current_seq;
    assert_rejected(
        &mut w.carol,
        Relay::Sealed(readdressed),
        3,
        DropReason::Undecryptable,
    );
    // Insider Alice decrypts Bob's acceptance and reseals it to Carol, even rewriting the
    // request ID to Carol's: Bob's signature covers `to` and the request digest.
    let inner = w.alice.insider_open(&wa, 3).unwrap();
    let ckey = w.alice.state.known_keys[&c].clone();
    let mut header = wa.header.clone();
    header.to = c.clone();
    header.key_seq = ckey.claim.seq;
    let eph = w.alice.next_random("ephemeral");
    let resealed = seal(header.clone(), &ckey.claim.x25519, eph, &inner);
    assert_rejected(
        &mut w.carol,
        Relay::Sealed(resealed),
        3,
        DropReason::Misaddressed,
    );
    let mut rewritten = inner.clone();
    if let Inner::Accept { to, request_id, .. } = &mut rewritten.inner {
        *to = c.clone();
        *request_id = carol_rid.clone();
    }
    let eph = w.alice.next_random("ephemeral");
    let resealed = seal(header, &ckey.claim.x25519, eph, &rewritten);
    assert_rejected(
        &mut w.carol,
        Relay::Sealed(resealed),
        3,
        DropReason::BadSignature,
    );
    // Carol forges an acceptance "from Bob" to Alice carrying her own endpoint.
    let digest = w.alice.state.outgoing[&alice_rid].digest.clone();
    let fake = SignedInner::new(
        &w.carol.id,
        Inner::Accept {
            group: GROUP.into(),
            from: b.clone(),
            to: a.clone(),
            request_id: alice_rid.clone(),
            request_digest: digest,
            endpoint: w.carol.state.private_endpoint.clone(),
        },
    );
    let akey = w.carol.state.known_keys[&a].clone();
    let eph = w.carol.next_random("ephemeral");
    let fake = seal(
        Header {
            group: GROUP.into(),
            kind: WireKind::Answer,
            from: b.clone(),
            to: a.clone(),
            key_seq: akey.claim.seq,
            ephemeral: [0; 32],
        },
        &akey.claim.x25519,
        eph,
        &fake,
    );
    assert_rejected(
        &mut w.alice,
        Relay::Sealed(fake),
        3,
        DropReason::BadSignature,
    );

    // Positive controls: genuine acceptance reaches Alice; Carol's request still pending.
    assert_eq!(w.alice.receive(acc, 4).0, Delivery::Answered(alice_rid));
    assert_eq!(
        w.alice.state.contacts.get(&b),
        Some(&w.bob.state.private_endpoint)
    );
    assert_eq!(w.carol.observe(&carol_rid, 4), Observation::Pending);
    assert!(w.carol.state.contacts.is_empty());
}

// KIND: candidate-policy G-BLOCK
// Kept under U5 revised: the property is that a host which drops a request cannot forge the
// target's refusal (decline is the open explicit-vs-silent candidate); it asserts nothing
// about whether a block is silent.
#[test]
fn host_drop_yields_no_answer_never_refused() {
    let explicit = Policy {
        decline: DeclinePolicy::Explicit,
        ..policy()
    };
    let mut w = world(explicit);
    let (a, b) = (w.alice.did(), w.bob.did());
    let rid = send(&mut w.host, &mut w.alice, &b, 1);
    // Malicious host discards the request.
    assert_eq!(w.host.fetch(&b).len(), 1);
    assert_eq!(
        w.alice.observe(&rid, 1 + ANSWER_TIMEOUT - 1),
        Observation::Pending
    );
    assert_eq!(
        w.alice.observe(&rid, 1 + ANSWER_TIMEOUT),
        Observation::NoAnswer
    );
    // Even knowing the request ID and digest, the host cannot forge Bob's refusal.
    let host_id = Author::new(40, GROUP).id;
    let forged = SignedInner::new(
        &host_id,
        Inner::Decline {
            group: GROUP.into(),
            from: b.clone(),
            to: a.clone(),
            request_id: rid.clone(),
            request_digest: w.alice.state.outgoing[&rid].digest.clone(),
        },
    );
    let akey = w.host.state.directory[&a].clone();
    let forged = seal(
        Header {
            group: GROUP.into(),
            kind: WireKind::Answer,
            from: b.clone(),
            to: a.clone(),
            key_seq: akey.claim.seq,
            ephemeral: [0; 32],
        },
        &akey.claim.x25519,
        sim_random(40, "ephemeral", 1),
        &forged,
    );
    assert_rejected(
        &mut w.alice,
        Relay::Sealed(forged),
        12,
        DropReason::BadSignature,
    );
    assert_eq!(w.alice.observe(&rid, 12), Observation::NoAnswer);

    // Positive control: a genuine explicit decline is the only way to see Refused.
    let rid2 = send(&mut w.host, &mut w.alice, &b, 20);
    pump(&mut w.host, &mut w.bob, 21);
    w.host.submit(w.bob.decline(&rid2).unwrap(), 21).unwrap();
    pump(&mut w.host, &mut w.alice, 22);
    assert_eq!(w.alice.observe(&rid2, 22), Observation::Refused);
}

#[derive(Clone, Copy, Debug)]
enum TargetCase {
    Offline,
    OnlineUnanswered,
    // Only with `BlockReply::Drop`: asserts that no reply is emitted.
    Blocked,
    // Only with `DeclinePolicy::Silent`.
    SilentDecline,
}
// Runs one fresh, identically seeded world; returns (observation, requester snapshot,
// host submit result) as seen by Alice.
fn requester_view(p: Policy, case: TargetCase) -> (Observation, Vec<u8>, bool) {
    let mut w = world(p);
    let (a, b) = (w.alice.did(), w.bob.did());
    if let TargetCase::Blocked = case {
        assert_eq!(p.block, BlockReply::Drop);
        w.bob.block(&a);
    }
    let (rid, relay) = w.alice.request(&b, 1).unwrap();
    let submitted = w.host.submit(relay, 1).is_ok();
    match case {
        TargetCase::Offline => {}
        TargetCase::OnlineUnanswered => {
            assert_eq!(
                pump(&mut w.host, &mut w.bob, 2),
                vec![Delivery::NewRequest(rid.clone())]
            );
        }
        TargetCase::Blocked => {
            let before = w.bob.snapshot();
            let deliveries = pump(&mut w.host, &mut w.bob, 2);
            assert_eq!(deliveries, vec![Delivery::Dropped(DropReason::Blocked)]);
            assert_eq!(
                before,
                w.bob.snapshot(),
                "blocked drop must not change target state"
            );
            assert!(
                w.host.state.mailboxes.values().all(|m| m.is_empty()),
                "no reply emitted"
            );
        }
        TargetCase::SilentDecline => {
            assert_eq!(p.decline, DeclinePolicy::Silent);
            pump(&mut w.host, &mut w.bob, 2);
            assert!(w.bob.decline(&rid).is_none());
        }
    }
    // Alice polls the host; nothing is waiting for her in any case.
    assert!(pump(&mut w.host, &mut w.alice, 1 + ANSWER_TIMEOUT).is_empty());
    (
        w.alice.observe(&rid, 1 + ANSWER_TIMEOUT),
        w.alice.snapshot(),
        submitted,
    )
}

// KIND: candidate-policy G-BLOCK
// Section 8b Q4 (open): a chat request from a DID the target blocked. Replaces the W2-Q tests
// that asserted the rejected silent option (U5 revised). Two candidates:
// - `SealedNotice`: Bob answers each request through the host with a sealed, DID-signed
//   "blocked" answer bound to that request. Alice observes `Blocked` on every attempt (same
//   reason), which she can tell apart from an offline target, an unanswered request and an
//   explicit decline. Bob's state is unchanged by each refused request. The host sees an
//   Answer of the same kind and ciphertext size as an accept, so it cannot tell a block
//   from an accept. A host-forged "blocked" answer fails the DID signature and a replayed
//   one is refused, Alice's state unchanged in both.
// - `Drop`: no reply; at this layer Alice sees `NoAnswer`, the same as an offline target.
//   The block is then visible only through the private-path notice (proto
//   `rotation_visible_block.rs`, `multi_host.rs`), if Alice is a private contact at all.
#[test]
fn candidate_blocked_requester_gets_sealed_signed_answer_vs_drop() {
    let p = Policy {
        decline: DeclinePolicy::Explicit,
        block: BlockReply::SealedNotice,
        ..policy()
    };
    let mut w = world(p);
    let (a, b, c) = (w.alice.did(), w.bob.did(), w.carol.did());
    w.bob.block(&a);
    // Reference size: a genuine accept from Bob to Carol.
    let rc = send(&mut w.host, &mut w.carol, &b, 1);
    pump(&mut w.host, &mut w.bob, 1);
    let accept = w.bob.accept(&rc).unwrap();
    let mut first_reply = None;
    for t in [2, 20] {
        let rid = send(&mut w.host, &mut w.alice, &b, t);
        let before = w.bob.snapshot();
        assert_eq!(
            pump(&mut w.host, &mut w.bob, t + 1),
            vec![Delivery::Dropped(DropReason::Blocked)]
        );
        assert_eq!(w.bob.snapshot(), before, "refusal changed target state");
        let reply = w.host.state.mailboxes[&a].last().unwrap().clone();
        assert_eq!(reply.route(), (b.as_str(), a.as_str()));
        assert_eq!(reply.wire().header.kind, accept.wire().header.kind);
        assert_eq!(
            reply.wire().ciphertext.len(),
            accept.wire().ciphertext.len()
        );
        assert_eq!(
            pump(&mut w.host, &mut w.alice, t + 2),
            vec![Delivery::Answered(rid.clone())]
        );
        assert_eq!(w.alice.observe(&rid, t + 2), Observation::Blocked);
        assert!(!w.alice.state.contacts.contains_key(&b));
        first_reply.get_or_insert(reply);
    }
    // Replay of the first sealed answer: already answered, refused unchanged.
    assert_rejected(&mut w.alice, first_reply.unwrap(), 30, DropReason::Replay);
    // A host-forged "blocked" answer for a fresh request fails Bob's DID signature.
    let rid = send(&mut w.host, &mut w.alice, &b, 40);
    let host_id = Author::new(40, GROUP).id;
    let forged = SignedInner::new(
        &host_id,
        Inner::Blocked {
            group: GROUP.into(),
            from: b.clone(),
            to: a.clone(),
            request_id: rid.clone(),
            request_digest: w.alice.state.outgoing[&rid].digest.clone(),
        },
    );
    let akey = w.host.state.directory[&a].clone();
    let forged = seal(
        Header {
            group: GROUP.into(),
            kind: WireKind::Answer,
            from: b.clone(),
            to: a.clone(),
            key_seq: akey.claim.seq,
            ephemeral: [0; 32],
        },
        &akey.claim.x25519,
        sim_random(40, "ephemeral", 2),
        &forged,
    );
    assert_rejected(
        &mut w.alice,
        Relay::Sealed(forged),
        41,
        DropReason::BadSignature,
    );
    assert_eq!(w.alice.observe(&rid, 41), Observation::Pending);
    // Carol (not blocked) is unaffected.
    w.host.submit(accept, 3).unwrap();
    pump(&mut w.host, &mut w.carol, 3);
    assert!(matches!(w.carol.observe(&rc, 3), Observation::Accepted(_)));
    assert!(w.bob.state.contacts.contains_key(&c));

    // Distinguishable under SealedNotice; under Drop, blocked == offline at this layer.
    for case in [TargetCase::Offline, TargetCase::OnlineUnanswered] {
        assert_eq!(requester_view(p, case).0, Observation::NoAnswer);
    }
    let drop = Policy {
        block: BlockReply::Drop,
        ..p
    };
    let blocked = requester_view(drop, TargetCase::Blocked);
    assert_eq!(blocked.0, Observation::NoAnswer);
    assert_eq!(blocked, requester_view(drop, TargetCase::Offline));
    // The silent-decline candidate (open, separate from blocking) also reads as NoAnswer, so
    // under SealedNotice it stays distinguishable from a block.
    let silent = Policy {
        decline: DeclinePolicy::Silent,
        ..p
    };
    assert_eq!(
        requester_view(silent, TargetCase::SilentDecline).0,
        Observation::NoAnswer
    );
}

// KIND: candidate-policy G-BLOCK
#[test]
fn candidate_explicit_decline_reports_refused_and_hides_kind_from_host() {
    let mut w = world(Policy {
        decline: DeclinePolicy::Explicit,
        ..policy()
    });
    let b = w.bob.did();
    let r_alice = send(&mut w.host, &mut w.alice, &b, 1);
    let r_carol = send(&mut w.host, &mut w.carol, &b, 1);
    pump(&mut w.host, &mut w.bob, 2);
    let declined = w.bob.decline(&r_alice).unwrap();
    let accepted = w.bob.accept(&r_carol).unwrap();
    // Host sees the same kind and size for accept and decline.
    assert_eq!(declined.wire().header.kind, accepted.wire().header.kind);
    assert_eq!(
        declined.wire().ciphertext.len(),
        accepted.wire().ciphertext.len()
    );
    w.host.submit(declined, 3).unwrap();
    pump(&mut w.host, &mut w.alice, 4);
    assert_eq!(w.alice.observe(&r_alice, 4), Observation::Refused);
    assert!(w.alice.state.contacts.is_empty());
    assert!(!w.bob.state.contacts.contains_key(&w.alice.did()));
    // Under this candidate a decline (Refused) is distinguishable from a block (NoAnswer
    // under the `BlockReply::Drop` candidate, `Blocked` under `SealedNotice`).
    let (blocked, ..) = requester_view(
        Policy {
            decline: DeclinePolicy::Explicit,
            ..policy()
        },
        TargetCase::Blocked,
    );
    assert_ne!(blocked, Observation::Refused);
}

// KIND: candidate-policy G-REQUEST-RATE
#[test]
fn candidate_host_enforced_rate_limit_per_requester_target_pair() {
    let mut w = world(policy());
    w.host.rate = Some(RateLimit { max: 2, window: 10 });
    let b = w.bob.did();
    send(&mut w.host, &mut w.alice, &b, 1);
    send(&mut w.host, &mut w.alice, &b, 2);
    let (_, third) = w.alice.request(&b, 3).unwrap();
    let before = w.host.snapshot();
    assert_eq!(
        w.host.submit(third.clone(), 3),
        Err(HostRefusal::RateLimited)
    );
    assert_eq!(
        before,
        w.host.snapshot(),
        "over-limit drop must not change host state"
    );
    // Per pair: Carol -> Bob and Alice -> Carol are unaffected.
    send(&mut w.host, &mut w.carol, &b, 3);
    let c = w.carol.did();
    send(&mut w.host, &mut w.alice, &c, 3);
    // Window expiry re-admits Alice -> Bob.
    assert_eq!(w.host.submit(third, 11), Ok(()));
}

// KIND: candidate-policy G-REQUEST-RATE
#[test]
fn candidate_target_enforced_rate_limit_drops_silently() {
    let mut w = world(Policy {
        target_rate: Some(RateLimit { max: 2, window: 10 }),
        ..policy()
    });
    let b = w.bob.did();
    let r1 = send(&mut w.host, &mut w.alice, &b, 1);
    let r2 = send(&mut w.host, &mut w.alice, &b, 1);
    let r3 = send(&mut w.host, &mut w.alice, &b, 1);
    let rc = send(&mut w.host, &mut w.carol, &b, 1);
    let mut inbox = w.host.fetch(&b).into_iter();
    assert_eq!(
        w.bob.receive(inbox.next().unwrap(), 2).0,
        Delivery::NewRequest(r1)
    );
    assert_eq!(
        w.bob.receive(inbox.next().unwrap(), 2).0,
        Delivery::NewRequest(r2)
    );
    let third = inbox.next().unwrap();
    assert_rejected(&mut w.bob, third.clone(), 2, DropReason::RateLimited);
    assert_eq!(
        w.bob.receive(inbox.next().unwrap(), 2).0,
        Delivery::NewRequest(rc)
    );
    // Requester sees the same as any unanswered request.
    assert_eq!(
        w.alice.observe(&r3, 1 + ANSWER_TIMEOUT),
        Observation::NoAnswer
    );
    // Not marked seen, so the same request is accepted once the window passes.
    assert_eq!(w.bob.receive(third, 12).0, Delivery::NewRequest(r3));
}

// KIND: invariant
#[test]
fn retired_request_key_never_decrypts_after_retirement() {
    for retired_key in [RetiredKeyPolicy::Drop, RetiredKeyPolicy::HintCurrentKey] {
        let mut w = world(Policy {
            retired_key,
            grace: 5,
            ..policy()
        });
        let b = w.bob.did();
        let note = w.bob.rotate_request_key(10); // seq 1 retires at 15
        assert!(w.host.publish(note));
        // Alice has not synced: she still seals to seq 1.
        let (r_in_grace, in_grace) = w.alice.request(&b, 12).unwrap();
        let (_, at_boundary) = w.alice.request(&b, 12).unwrap();
        let (_, late) = w.alice.request(&b, 12).unwrap();
        assert_eq!(in_grace.wire().header.key_seq, 1);
        assert_eq!(
            w.bob.receive(in_grace, 14).0,
            Delivery::NewRequest(r_in_grace)
        );
        for (relay, now) in [(at_boundary, 15), (late.clone(), 30)] {
            let before = w.bob.snapshot();
            let (d, _) = w.bob.receive(relay, now);
            assert_eq!(
                d,
                Delivery::Dropped(DropReason::StaleKey),
                "{retired_key:?} t={now}"
            );
            assert_eq!(before, w.bob.snapshot());
        }
        // tick erases the secret; the public half is kept for audit only.
        w.bob.tick(15);
        assert!(w.bob.state.keys[&1].secret.is_none());
        assert!(w.bob.state.keys[&2].secret.is_some());
        assert!(w.bob.usable_secret(1, 15).is_none());
        assert!(w.bob.insider_open(late.wire(), 30).is_none());
    }
}

// KIND: candidate-policy G-RETIREMENT
#[test]
fn candidate_stale_key_request_is_dropped_silently() {
    let mut w = world(policy());
    let b = w.bob.did();
    assert!(w.host.publish(w.bob.rotate_request_key(10)));
    w.bob.tick(15);
    let rid = send(&mut w.host, &mut w.alice, &b, 20);
    let before = w.bob.snapshot();
    assert_eq!(
        pump(&mut w.host, &mut w.bob, 21),
        vec![Delivery::Dropped(DropReason::StaleKey)]
    );
    assert_eq!(before, w.bob.snapshot());
    assert!(
        w.host.state.mailboxes.values().all(|m| m.is_empty()),
        "no reply"
    );
    assert_eq!(
        w.alice.observe(&rid, 20 + ANSWER_TIMEOUT),
        Observation::NoAnswer
    );
    // Recovery is only by Alice re-syncing the directory and asking again.
    w.alice.sync_keys(&w.host);
    let rid2 = send(&mut w.host, &mut w.alice, &b, 31);
    assert_eq!(
        pump(&mut w.host, &mut w.bob, 32),
        vec![Delivery::NewRequest(rid2)]
    );
}

// KIND: candidate-policy G-RETIREMENT
#[test]
fn candidate_stale_key_request_is_answered_with_current_key_hint() {
    let mut w = world(Policy {
        retired_key: RetiredKeyPolicy::HintCurrentKey,
        ..policy()
    });
    let (a, b, c) = (w.alice.did(), w.bob.did(), w.carol.did());
    let old_note = w.bob.key_note();
    assert!(w.host.publish(w.bob.rotate_request_key(10)));
    w.bob.tick(15);
    // Carol is blocked: she gets no hint (same as Drop / offline).
    w.bob.block(&c);
    let rc = send(&mut w.host, &mut w.carol, &b, 20);
    let ra = send(&mut w.host, &mut w.alice, &b, 20);
    let before = w.bob.snapshot();
    let deliveries = pump(&mut w.host, &mut w.bob, 21);
    assert_eq!(deliveries, vec![Delivery::Dropped(DropReason::StaleKey); 2]);
    assert_eq!(before, w.bob.snapshot());
    assert!(w.host.fetch(&c).is_empty());
    assert_eq!(
        w.carol.observe(&rc, 20 + ANSWER_TIMEOUT),
        Observation::NoAnswer
    );
    // A replayed OLD hint does not roll Alice back.
    let alice_before = w.alice.snapshot();
    let replayed = Relay::KeyHint {
        from: b.clone(),
        to: a.clone(),
        note: old_note,
    };
    assert_eq!(
        w.alice.receive(replayed, 21).0,
        Delivery::Dropped(DropReason::UnknownRequest)
    );
    assert_eq!(alice_before, w.alice.snapshot());
    // Alice gets the genuine hint, learns seq 2 and retries.
    assert_eq!(
        pump(&mut w.host, &mut w.alice, 22),
        vec![Delivery::KeyHint(b.clone())]
    );
    assert_eq!(w.alice.observe(&ra, 22), Observation::KeyStale);
    assert_eq!(w.alice.state.known_keys[&b].claim.seq, 2);
    let ra2 = send(&mut w.host, &mut w.alice, &b, 23);
    assert_eq!(
        pump(&mut w.host, &mut w.bob, 24),
        vec![Delivery::NewRequest(ra2)]
    );
}

// KIND: invariant
#[test]
fn request_key_notes_need_did_signature_and_increasing_seq() {
    let mut w = world(policy());
    let b = w.bob.did();
    let seq1 = w.bob.key_note();
    let seq2 = w.bob.rotate_request_key(10);
    let mut bad = vec![];
    // Signed by Carol over Bob's claim (host/insider key substitution).
    let mut forged = seq2.clone();
    forged.claim.x25519 = w.carol.state.keys[&1].public;
    forged.sig = did_sign(&w.carol.id, "request-key", &forged.claim);
    bad.push(forged);
    // Bob's signature but substituted key bytes.
    let mut swapped = seq2.clone();
    swapped.claim.x25519 = [9; 32];
    bad.push(swapped);
    // Wrong group.
    let mut other = seq2.clone();
    other.claim.group = "other-group".into();
    other.sig = did_sign(&w.bob.id, "request-key", &other.claim);
    bad.push(other);
    let host_before = w.host.snapshot();
    let alice_before = w.alice.snapshot();
    for note in bad {
        assert!(!w.alice.learn_key(note.clone()));
        assert!(!w.host.publish(note));
    }
    assert_eq!(alice_before, w.alice.snapshot());
    assert_eq!(host_before, w.host.snapshot());
    // Positive control, then rollback to seq 1 refused by both.
    assert!(w.alice.learn_key(seq2.clone()) && w.host.publish(seq2));
    let (alice_before, host_before) = (w.alice.snapshot(), w.host.snapshot());
    assert!(!w.alice.learn_key(seq1.clone()));
    assert!(!w.host.publish(seq1));
    assert_eq!(alice_before, w.alice.snapshot());
    assert_eq!(host_before, w.host.snapshot());
    assert_eq!(w.alice.state.known_keys[&b].claim.seq, 2);
}

// KIND: limitation
#[test]
fn host_learns_who_requested_whom_and_answer_timing() {
    // E2E hides content and endpoints, NOT the social graph. The host sees sender,
    // target and time of every request, and that an answer flowed back. Under the silent
    // decline candidate any answer at all means "accepted".
    let mut w = world(policy());
    let (a, b) = (w.alice.did(), w.bob.did());
    let rid = send(&mut w.host, &mut w.alice, &b, 1);
    pump(&mut w.host, &mut w.bob, 2);
    w.host.submit(w.bob.accept(&rid).unwrap(), 3).unwrap();
    let graph: Vec<(String, String, WireKind)> = w
        .host
        .state
        .relayed
        .iter()
        .map(|r| {
            let h = &r.wire().header;
            (h.from.clone(), h.to.clone(), h.kind)
        })
        .collect();
    assert_eq!(
        graph,
        vec![
            (a.clone(), b.clone(), WireKind::Request),
            (b, a.clone(), WireKind::Answer)
        ]
    );
    assert_eq!(
        w.host
            .state
            .rate
            .values()
            .flatten()
            .copied()
            .collect::<Vec<_>>(),
        vec![1]
    );
}

// KIND: limitation
#[test]
fn target_learns_requester_private_endpoint_before_deciding() {
    // In this 2-message construction the request carries the requester's private
    // endpoint, so a target that later declines (or leaves it pending) has already seen
    // it. A 3-message variant (endpoint only after accept) is an open question.
    let mut w = world(policy());
    let b = w.bob.did();
    let rid = send(&mut w.host, &mut w.alice, &b, 1);
    pump(&mut w.host, &mut w.bob, 2);
    assert_eq!(
        w.bob.state.incoming[&rid].endpoint,
        w.alice.state.private_endpoint
    );
    assert!(w.bob.decline(&rid).is_none());
    assert!(!w.bob.state.contacts.contains_key(&w.alice.did()));
}
