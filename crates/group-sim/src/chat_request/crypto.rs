use super::*;

pub(super) const SIM: &str = "tinline/group-sim/chat-request/not-production/v1";
pub(super) const KDF_CONTEXT: &str = "tinline/group-sim/chat-request/not-production/v1 aead-key";
pub(super) const RNG_CONTEXT: &str = "tinline/group-sim/chat-request/not-production/v1 sim-rng";
pub(super) const GROUP: &str = "w2q-group";
// Requester-side deadline after which an unanswered request is reported as `NoAnswer`.
// Fixture value; the real value is a product/timing question (G-TIME).
pub(super) const ANSWER_TIMEOUT: u64 = 10;
// Fixed plaintext bucket so the host cannot tell accept from decline by size.
pub(super) const PAD: usize = 1024;

// ---------------------------------------------------------------- helpers

pub(super) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
pub(super) fn sim_random(seed: u8, label: &str, counter: u64) -> [u8; 32] {
    let mut input = vec![seed];
    input.extend_from_slice(label.as_bytes());
    input.push(0);
    input.extend_from_slice(&counter.to_le_bytes());
    blake3::derive_key(RNG_CONTEXT, &input)
}
pub(super) fn signing_bytes(purpose: &str, body: &impl Serialize) -> Vec<u8> {
    [
        SIM.as_bytes(),
        b"\0",
        purpose.as_bytes(),
        b"\0",
        &serde_json::to_vec(body).unwrap(),
    ]
    .concat()
}
pub(super) fn did_sign(id: &Identity, purpose: &str, body: &impl Serialize) -> Vec<u8> {
    id.sign(&signing_bytes(purpose, body)).to_vec()
}
pub(super) fn did_verify(did: &str, purpose: &str, body: &impl Serialize, sig: &[u8]) -> bool {
    let (Some(public), Ok(sig)) = (
        identity::public_key_from_did(did),
        <[u8; 64]>::try_from(sig),
    ) else {
        return false;
    };
    identity::verify(&public, &signing_bytes(purpose, body), &sig)
}
pub(super) fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

// ---------------------------------------------------------------- wire types

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct RequestKeyClaim {
    pub(super) group: String,
    pub(super) owner: String,
    pub(super) seq: u64,
    pub(super) x25519: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SignedRequestKey {
    pub(super) claim: RequestKeyClaim,
    pub(super) sig: Vec<u8>,
}
impl SignedRequestKey {
    pub(super) fn verify(&self) -> bool {
        did_verify(&self.claim.owner, "request-key", &self.claim, &self.sig)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum WireKind {
    Request,
    Answer,
}
// Everything the host can read. Authenticated as AEAD associated data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Header {
    pub(super) group: String,
    pub(super) kind: WireKind,
    pub(super) from: String,
    pub(super) to: String,
    pub(super) key_seq: u64,
    pub(super) ephemeral: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Wire {
    pub(super) header: Header,
    pub(super) ciphertext: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Inner {
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
    pub(super) fn from(&self) -> &str {
        match self {
            Inner::Request { from, .. }
            | Inner::Accept { from, .. }
            | Inner::Decline { from, .. }
            | Inner::Blocked { from, .. } => from,
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
        did_verify(self.inner.from(), "chat-inner", &self.inner, &self.sig)
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
pub(super) fn aad(h: &Header) -> Vec<u8> {
    [SIM.as_bytes(), b"\0aad\0", &serde_json::to_vec(h).unwrap()].concat()
}
// SIMULATION construction (see module docs). Header.ephemeral is overwritten.
pub(super) fn seal(
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
pub(super) fn open(secret: &StaticSecret, wire: &Wire) -> Option<SignedInner> {
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
pub(super) enum Relay {
    Sealed(Wire),
    // Candidate G-RETIREMENT reply: the target's current public key note, unencrypted.
    KeyHint {
        from: String,
        to: String,
        note: SignedRequestKey,
    },
}
impl Relay {
    pub(super) fn route(&self) -> (&str, &str) {
        match self {
            Relay::Sealed(w) => (&w.header.from, &w.header.to),
            Relay::KeyHint { from, to, .. } => (from, to),
        }
    }
    pub(super) fn wire(&self) -> &Wire {
        match self {
            Relay::Sealed(w) => w,
            Relay::KeyHint { .. } => panic!("not a sealed relay"),
        }
    }
}

// ---------------------------------------------------------------- policies (open gates)

// G-BLOCK candidate: does a user "decline" send a signed refusal, or stay silent?
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) enum DeclinePolicy {
    Explicit,
    Silent,
}
// G-BLOCK candidate, section 8b Q4 (open): what a blocker returns to a chat request from a
// DID it blocked. U5 revised decides that blocking is visible; it does not decide this path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) enum BlockReply {
    // No reply over the host: at this layer the requester sees `NoAnswer`, as for offline.
    Drop,
    // A sealed, DID-signed "blocked" answer of the same kind and size as accept/decline.
    SealedNotice,
}
// G-RETIREMENT candidate: what a target does with a request sealed to an unusable key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) enum RetiredKeyPolicy {
    Drop,
    HintCurrentKey,
}
// G-REQUEST-RATE (new, proposed gate): values are fixtures, not decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct RateLimit {
    pub(super) max: usize,
    pub(super) window: u64,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Policy {
    pub(super) decline: DeclinePolicy,
    pub(super) block: BlockReply,
    pub(super) retired_key: RetiredKeyPolicy,
    // Old request key stays usable for decrypt until rotate_time + grace (exclusive).
    pub(super) grace: u64,
    pub(super) target_rate: Option<RateLimit>,
}
pub(super) fn policy() -> Policy {
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
pub(super) enum Answer {
    Accepted(String),
    Refused,
    Blocked,
    KeyStale,
}
// What the requester can observe about one of its requests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Observation {
    Pending,
    Accepted(String),
    Refused,
    Blocked,
    KeyStale,
    NoAnswer,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DropReason {
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
pub(super) enum Delivery {
    NewRequest(String),
    Answered(String),
    KeyHint(String),
    Dropped(DropReason),
}
#[derive(Clone, Serialize)]
pub(super) struct KeySlot {
    pub(super) secret: Option<[u8; 32]>,
    pub(super) public: [u8; 32],
    pub(super) retire_at: Option<u64>,
}
#[derive(Clone, Serialize)]
pub(super) struct Outgoing {
    pub(super) to: String,
    pub(super) sent_at: u64,
    pub(super) key_seq: u64,
    pub(super) digest: String,
    pub(super) answer: Option<Answer>,
}
#[derive(Clone, Serialize)]
pub(super) struct Incoming {
    pub(super) from: String,
    pub(super) endpoint: String,
    pub(super) digest: String,
    pub(super) received_at: u64,
}
#[derive(Clone, Serialize)]
pub(super) struct MemberState {
    pub(super) rng: u64,
    pub(super) private_endpoint: String,
    pub(super) group_endpoint: String,
    pub(super) current_seq: u64,
    pub(super) keys: BTreeMap<u64, KeySlot>,
    pub(super) known_keys: BTreeMap<String, SignedRequestKey>,
    pub(super) outgoing: BTreeMap<String, Outgoing>,
    pub(super) incoming: BTreeMap<String, Incoming>,
    pub(super) seen: BTreeSet<String>,
    pub(super) contacts: BTreeMap<String, String>,
    pub(super) blocked: BTreeSet<String>,
    pub(super) request_times: BTreeMap<String, Vec<u64>>,
    pub(super) direct_inbox: Vec<(String, String)>,
    pub(super) policy: Policy,
}
#[derive(Clone, Serialize)]
pub(super) struct DirectBody {
    pub(super) to_endpoint: String,
    pub(super) from: String,
    pub(super) text: String,
}
#[derive(Clone, Serialize)]
pub(super) struct DirectMsg {
    pub(super) body: DirectBody,
    pub(super) sig: Vec<u8>,
}
