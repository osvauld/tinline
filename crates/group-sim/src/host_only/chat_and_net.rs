use super::*;

pub(super) fn aad(h: &Header) -> Vec<u8> {
    [
        CHAT_DOMAIN.as_bytes(),
        b"\0aad\0",
        &serde_json::to_vec(h).unwrap(),
    ]
    .concat()
}
pub(super) fn seal(mut header: Header, recipient: &[u8; 32], eph_seed: [u8; 32], p: &SignedInner) -> Sealed {
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
pub(super) fn open(secret: &StaticSecret, w: &Sealed) -> Option<SignedInner> {
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
pub(super) struct Hello {
    pub(super) from: String,
    pub(super) to: String,
    pub(super) endpoint: String,
    pub(super) text: String,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ChatIn {
    Request { from: String, request_id: String },
    Accepted { from: String },
    Dropped,
}

// ---- devices and world ---------------------------------------------------------------------

pub(super) struct Device {
    pub(super) id: Identity,
    pub(super) w: Writer,
    pub(super) slot: u64,
    pub(super) board: Board,
    pub(super) group_key: SigningKey,
    pub(super) group_seq: u64,
    pub(super) private_key: SigningKey,
    pub(super) private_seq: u64,
    pub(super) req_secret: [u8; 32], // per-DID request key (same on all of a DID's devices)
    pub(super) host_did: String,
    // A member's ENTIRE group address book: the host's DID-signed group note. Nothing else.
    pub(super) host_note: Option<EpNote>,
    pub(super) req_keys: BTreeMap<String, SignedReqKey>,
    pub(super) private_book: BTreeMap<String, String>, // DID -> private endpoint (private audience only)
    pub(super) unsent_note: Option<Vec<u8>>,
    pub(super) outbox: Vec<Post>,
    pub(super) refused: Vec<(Post, Why)>,
    pub(super) online: bool,
    pub(super) contacts: BTreeSet<String>,
    pub(super) blocked: BTreeSet<String>, // personal block list: excludes from private notes (U5 rev.)
    pub(super) incoming: BTreeMap<String, (String, String)>, // request_id -> (from, digest)
    pub(super) outgoing: BTreeMap<String, (String, String)>, // request_id -> (to, digest)
    pub(super) seen: BTreeSet<String>,
}
impl Device {
    pub(super) fn did(&self) -> String {
        self.id.did().into()
    }
    pub(super) fn group_ep(&self) -> String {
        ep("grp", &self.group_key)
    }
    pub(super) fn private_ep(&self) -> String {
        ep("prv", &self.private_key)
    }
    pub(super) fn chat_snapshot(&self) -> Vec<u8> {
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
pub(super) enum QueueBound {
    Unbounded,
    Refuse(usize),
}
// G-TIME candidate: how long the host keeps its old group endpoint listening after rotating.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HostOverlap {
    Immediate,
    Ticks(u64),
}
#[derive(Clone, Copy)]
pub(super) struct Cfg {
    pub(super) seed: u64,
    pub(super) fork: ForkPolicy,
    pub(super) queue: QueueBound,
    pub(super) overlap: HostOverlap,
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
pub(super) enum RouteErr {
    SelfOffline,
    PeerOffline,
    NoAddress,
    EndpointGone,
    Lost,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Path {
    MemberDialedHost,
    MemberDialedRetiringHostEndpoint,
    HostDialedMember,
}
#[derive(Clone, Debug)]
pub(super) struct Frame {
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) to_ep: String,
    pub(super) kind: &'static str,
    pub(super) bytes: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeyKind {
    Group,
    Private,
}
pub(super) struct World {
    pub(super) cfg: Cfg,
    pub(super) rng: SimRng,
    pub(super) now: u64,
    pub(super) d: Vec<Device>,
    // Host-only knowledge (U7): "<did>#<slot>" -> member device's DID-signed group note.
    pub(super) directory: BTreeMap<String, EpNote>,
    pub(super) live: BTreeMap<String, usize>, // listening endpoint -> device index
    pub(super) retiring: BTreeMap<String, u64>, // old host endpoint -> retire_at (overlap candidate)
    pub(super) frames: Vec<Frame>,            // every frame on any link
    pub(super) keys_ever: Vec<(usize, KeyKind, [u8; 32])>,
}
pub(super) fn dir_key(did: &str, slot: u64) -> String {
    format!("{did}#{slot}")
}
