//! W2-M: multi-host group model with host permits and host-initiated reconnect.
//!
//! Status: proposal — not approved. Simulation only; no production API, wire format or UI.
//! Decisions modelled: U4 revised (DID-signed content, random writer IDs), U5 revised (visible
//! block / removal notice), U7 (only hosts hold member group endpoints), U9 (removal by admin
//! flags), section 9 Q5 (multiple hosts; permits in the grant chain; host need not be admin;
//! a host is one specific device; admins can remove a host) and Q6 (after rotating, the host
//! itself reconnects members at their current endpoints; no host overlap window).
//!
//! Provenance: `SimRng`, `Post`/`Writer`/`Board`, `EpNote` are COPIED from `host_only.rs`
//! @dcdf4a4 (trimmed: no `FreezeWriter`, no chat request). `MGrant` is COPIED from
//! `authority.rs` @dcdf4a4 `Grant` and extended with a `device` field (the nominated hosting
//! device) and `Admin`/`Host` capabilities. Coordinator consolidates later. `Genesis` is
//! reused unchanged from `authority.rs`.
//!
//! Model (test-only):
//! - A host is a device holding a host *permit*: a real Ed25519-signed grant whose chain roots
//!   at the group creator and passes through admin grants (`Cap::Admin` + `Cap::Delegate`).
//!   The leaf carries `caps = {Host}`, `holder = host DID` and `device = Some(route slot)`.
//!   A permit with a device can never be a parent. Members accept a host's DID-signed group
//!   note only together with such a permit whose device equals the note's slot.
//! - Device IDs (route slots) are random (seeded RNG), never derived from a key. They are not
//!   cryptographic: any holder of the same mnemonic can claim another sibling's slot.
//! - Members know every host's group endpoint (Q5); each host knows members' group endpoints;
//!   hosts sync with each other (records, ops, directory notes, posts) "as if a VV exchange
//!   had completed" (same idealisation as `scheduler.rs` / `host_only.rs`).
//! - Group ops (admit, flag, remove host) are DID-signed by an admin and verified by every
//!   device; nothing is host fiat. Appointment order for the "admin appointed after the host"
//!   candidate uses grant nonces as a fixture stand-in for an ordered op log (not designed).
//! - Fake `u64` time, deterministic seeds, in-memory only. Open policy is parameterised in
//!   `Cfg`; no option is selected.
use super::*;
use identity::{public_key_from_did, verify};

const POST_DOMAIN: &[u8] = b"tinline/group-sim/multi-host-post/not-production/v1\0";
const NOTE_DOMAIN: &[u8] = b"tinline/group-sim/multi-host-endpoint-note/not-production/v1\0";
const GRANT_DOMAIN: &[u8] = b"tinline/group-sim/multi-host-grant/not-production/v1\0";
const OP_DOMAIN: &[u8] = b"tinline/group-sim/multi-host-op/not-production/v1\0";
const NOTICE_DOMAIN: &[u8] = b"tinline/group-sim/multi-host-block-notice/not-production/v1\0";
const DOC: &str = "group/w2m/2026-10-10";
const MAX_PENDING_POSTS: usize = 16;
const MAX_CHAIN: usize = 8;
// U9: 3 flags from 3 DISTINCT admins (section 8 Q1/Q5 lean; threshold not decided).
const REMOVAL_FLAGS: usize = 3;

// Devices. olga = owner/creator; alice (2 devices) and bob = admins; carol = member;
// hana = member whose desktop H1 is a host and whose phone HP is an ordinary member device;
// hugo (H2) and ivan (H3) = separate host binaries with their own DIDs (not members).
const OLGA: usize = 0;
const A1: usize = 1;
const A2: usize = 2;
const BOB: usize = 3;
const CAROL: usize = 4;
const HP: usize = 5;
const H1: usize = 6;
const H2: usize = 7;
const H3: usize = 8;
const MEMBERS: [usize; 6] = [OLGA, A1, A2, BOB, CAROL, HP];

// ---- helpers -------------------------------------------------------------------------------

fn hex32(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn has(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}
fn ident(seed: u8) -> Identity {
    let mnemonic = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
    identity::recover(&mnemonic.to_string()).unwrap()
}
fn ep(prefix: &str, k: &SigningKey) -> String {
    format!("{prefix}-{}", hex32(&k.verifying_key().to_bytes()))
}
fn signed<T: Serialize>(domain: &[u8], body: &T) -> Vec<u8> {
    [domain, &serde_json::to_vec(body).unwrap()].concat()
}
fn valid(did: &str, bytes: &[u8], sig: &[u8]) -> bool {
    let (Some(key), Ok(sig)) = (public_key_from_did(did), <[u8; 64]>::try_from(sig)) else {
        return false;
    };
    verify(&key, bytes, &sig)
}
fn digest<T: Serialize>(x: &T) -> String {
    blake3::hash(&serde_json::to_vec(x).unwrap())
        .to_hex()
        .to_string()
}
fn dir_key(did: &str, slot: u64) -> String {
    format!("{did}#{slot}")
}
fn pair(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

// Copied from host_only.rs @dcdf4a4. Deterministic SplitMix64: the ONLY source of writer IDs,
// route slots (device IDs) and transport keys. Never seeded by key material.
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
    fn writer(&mut self) -> u64 {
        loop {
            let w = self.next_u64();
            if w != u64::MAX {
                return w;
            }
        }
    }
}

// ---- grants and host permits (copied from authority.rs @dcdf4a4 + device field) ------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum Cap {
    Read,
    Post,
    Delegate,
    Admin,
    Host,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct MGrantBody {
    version: u8,
    group: String,
    issuer: String,
    holder: String,
    device: Option<u64>, // host permits only: the ONE nominated hosting device (route slot)
    caps: BTreeSet<Cap>,
    delegable: bool,
    nonce: u64, // fixture stand-in for appointment order
    parent: Option<Box<MGrant>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct MGrant {
    body: MGrantBody,
    sig: Vec<u8>,
}
impl MGrant {
    #[allow(clippy::too_many_arguments)]
    fn issue(
        issuer: &Identity,
        group: &str,
        holder: &str,
        device: Option<u64>,
        caps: &[Cap],
        delegable: bool,
        nonce: u64,
        parent: Option<MGrant>,
    ) -> Self {
        let body = MGrantBody {
            version: 1,
            group: group.into(),
            issuer: issuer.did().into(),
            holder: holder.into(),
            device,
            caps: caps.iter().copied().collect(),
            delegable,
            nonce,
            parent: parent.map(Box::new),
        };
        let sig = issuer.sign(&signed(GRANT_DOMAIN, &body)).to_vec();
        Self { body, sig }
    }
    fn digest(&self) -> String {
        digest(self)
    }
    // Copied walk from authority.rs `Grant::allows`; added: a device-bound grant (a host
    // permit) can never be a parent.
    fn allows(&self, g: &Genesis, holder: &str, cap: Cap) -> bool {
        let mut links = Vec::new();
        let mut cur = Some(self);
        while let Some(x) = cur {
            if links.len() >= MAX_CHAIN {
                return false;
            }
            links.push(x);
            cur = x.body.parent.as_deref();
        }
        if self.body.holder != holder || !self.body.caps.contains(&cap) {
            return false;
        }
        let group = g.id();
        let mut parent: Option<&MGrant> = None;
        for x in links.into_iter().rev() {
            let b = &x.body;
            if b.version != 1
                || b.group != group
                || public_key_from_did(&b.holder).is_none()
                || !valid(&b.issuer, &signed(GRANT_DOMAIN, b), &x.sig)
            {
                return false;
            }
            match parent {
                None if b.issuer != g.body.creator => return false,
                Some(p)
                    if b.issuer != p.body.holder
                        || !p.body.delegable
                        || p.body.device.is_some()
                        || !p.body.caps.contains(&Cap::Delegate)
                        || !b.caps.is_subset(&p.body.caps) =>
                {
                    return false;
                }
                _ => {}
            }
            parent = Some(x);
        }
        true
    }
}

// ---- DID-signed content (copied from host_only.rs @dcdf4a4, trimmed) -----------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PostBody {
    group: String,
    doc: String,
    author: String,
    writer: u64,
    seq: u64,
    text: String,
    update: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Post {
    body: PostBody,
    sig: Vec<u8>,
}
impl Post {
    fn sign(id: &Identity, body: PostBody) -> Self {
        let sig = id.sign(&signed(POST_DOMAIN, &body)).to_vec();
        Self { body, sig }
    }
    fn verify(&self) -> bool {
        valid(
            &self.body.author,
            &signed(POST_DOMAIN, &self.body),
            &self.sig,
        )
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum Why {
    WrongGroup,
    NotMember,
    Bounds,
    BadSignature,
    WriterOwnedByOtherDid,
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
    group: String,
    roster: BTreeSet<String>,
    doc: LoroDoc,
    accepted: BTreeMap<String, Post>,
    pending: BTreeMap<String, Post>,
    writers: BTreeMap<u64, String>,
    last_seq: BTreeMap<u64, u64>,
    order: Vec<String>,
}
impl Board {
    fn new(group: String, roster: BTreeSet<String>) -> Self {
        Self {
            group,
            roster,
            doc: LoroDoc::new(),
            accepted: BTreeMap::new(),
            pending: BTreeMap::new(),
            writers: BTreeMap::new(),
            last_seq: BTreeMap::new(),
            order: vec![],
        }
    }
    // Stateless checks (group, roster, bounds, DID signature).
    fn precheck(&self, p: &Post) -> Result<(), Why> {
        let b = &p.body;
        if b.group != self.group || b.doc != DOC {
            return Err(Why::WrongGroup);
        }
        if !self.roster.contains(&b.author) {
            return Err(Why::NotMember);
        }
        if b.seq == 0 || b.text.len() > 20_000 || b.update.len() > 1_000_000 {
            return Err(Why::Bounds);
        }
        if !p.verify() {
            return Err(Why::BadSignature);
        }
        Ok(())
    }
    fn receive(&mut self, p: Post) -> Verdict {
        if let Err(why) = self.precheck(&p) {
            return Verdict::Rejected(why);
        }
        let id = p.id();
        if self.accepted.contains_key(&id) {
            return Verdict::Duplicate;
        }
        if self.pending.contains_key(&id) {
            return Verdict::Pending;
        }
        if let Err(why) = self.check(&p) {
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
    fn snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(&(
            self.doc.export(ExportMode::all_updates()).unwrap(),
            format!("{:?}", self.doc.get_deep_value()),
            &self.roster,
            &self.accepted,
            &self.pending,
            &self.writers,
            &self.last_seq,
            &self.order,
        ))
        .unwrap()
    }
    fn texts(&self) -> BTreeSet<String> {
        self.accepted
            .values()
            .map(|p| p.body.text.clone())
            .collect()
    }
}

// ---- DID-signed endpoint notes (copied from host_only.rs @dcdf4a4) -------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Audience {
    Group(String),
    Private,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NoteClaims {
    owner: String,
    audience: Audience,
    slot: u64,
    endpoint: String,
    seq: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct EpNote {
    claims: NoteClaims,
    sig: Vec<u8>,
}
impl EpNote {
    fn sign(id: &Identity, audience: Audience, slot: u64, endpoint: &str, seq: u64) -> Self {
        let claims = NoteClaims {
            owner: id.did().into(),
            audience,
            slot,
            endpoint: endpoint.into(),
            seq,
        };
        let sig = id.sign(&signed(NOTE_DOMAIN, &claims)).to_vec();
        Self { claims, sig }
    }
    fn verify(&self) -> bool {
        valid(
            &self.claims.owner,
            &signed(NOTE_DOMAIN, &self.claims),
            &self.sig,
        )
    }
    fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}
// What a host presents to members and other hosts: its DID-signed note + its permit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct HostRecord {
    note: EpNote,
    permit: MGrant,
}
impl HostRecord {
    fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}

// ---- DID-signed group ops and block notice -------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Op {
    Admit { grant: MGrant },
    Flag { target: String },
    RemoveHost { permit: String }, // permit digest
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct OpBody {
    group: String,
    issuer: String,
    issuer_grant: Option<MGrant>, // None only for the group creator
    nonce: u64,
    op: Op,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SignedOp {
    body: OpBody,
    sig: Vec<u8>,
}
impl SignedOp {
    fn sign(id: &Identity, group: &str, grant: Option<MGrant>, nonce: u64, op: Op) -> Self {
        let body = OpBody {
            group: group.into(),
            issuer: id.did().into(),
            issuer_grant: grant,
            nonce,
            op,
        };
        let sig = id.sign(&signed(OP_DOMAIN, &body)).to_vec();
        Self { body, sig }
    }
    fn verify(&self) -> bool {
        valid(&self.body.issuer, &signed(OP_DOMAIN, &self.body), &self.sig)
    }
    fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
    fn id(&self) -> String {
        digest(self)
    }
}
// U5 revised + section 8b Q1 (decided): one DID-signed "blocked" notice, returned again on
// every later attempt. Wording and timing (8b Q2/Q3) are not modelled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct BlockBody {
    from: String,
    to: String,
    at: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct BlockNotice {
    body: BlockBody,
    sig: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refuse {
    Malformed,
    BadSignature,
    WrongAudience,
    WrongGroup,
    PermitInvalid,
    DeviceMismatch,
    Revoked,
    Stale,
    CapReached,
    NotAdmin,
    NotAppointedAfter,
    NotMember,
    UnknownHost,
    NotAddressed,
    NotHost,
}

// What every device (host or member) believes about the group's control plane.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
struct View {
    hosts: BTreeMap<String, HostRecord>, // "<did>#<slot>" -> accepted host record
    revoked: BTreeSet<String>,           // revoked permit digests
    ops: BTreeMap<String, SignedOp>,
    flags: BTreeMap<String, BTreeSet<String>>, // target -> distinct admin flaggers
    removed: BTreeSet<String>,
    blocked_by: BTreeMap<String, BlockNotice>,
}

// ---- configuration (open policy as parameters) ---------------------------------------------

// G-FORK: cross-host writer-ID claim resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClaimRule {
    /// Each host binds writer -> DID on first accept (host_only.rs rule, per host).
    FirstAtHost,
    /// Hosts hold a new writer's posts until every host has seen every claim for it, then
    /// the lowest claim (blake3 of the signed seq-1 post) wins everywhere.
    LowestAfterAllHostsSaw,
}
// G-TIME: does a member keep its old group endpoint listening until every host acked?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Retention {
    None,
    UntilHostAck,
}
// G-AUTHORITY: which admins may remove a host ("newer admins can remove" — unconfirmed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RemovalRule {
    AnyAdmin,
    AdminAppointedAfterHost,
}
// G-AUTHORITY: maximum hosts a member accepts (user passed; not decided).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostCap {
    Unbounded,
    Max(usize),
}
#[derive(Clone)]
struct Cfg {
    seed: u64,
    hosts: Vec<usize>,
    claim: ClaimRule,
    retention: Retention,
    removal: RemovalRule,
    cap: HostCap,
}
impl Default for Cfg {
    fn default() -> Self {
        Self {
            seed: 0x5EED_0002,
            hosts: vec![H1, H2],
            claim: ClaimRule::FirstAtHost,
            retention: Retention::None,
            removal: RemovalRule::AnyAdmin,
            cap: HostCap::Unbounded,
        }
    }
}

// ---- devices and world ---------------------------------------------------------------------

#[derive(Serialize)]
struct HostState {
    record: HostRecord,
    directory: BTreeMap<String, EpNote>, // member device "<did>#<slot>" -> group note (U7)
    held: BTreeMap<String, Post>,        // LowestAfterAllHostsSaw: unsettled writers' posts
    claims: BTreeMap<u64, BTreeMap<String, String>>, // writer -> claim id -> DID
    known_by: BTreeMap<String, BTreeSet<usize>>, // claim id -> hosts known to hold it
    settled: BTreeMap<u64, String>,      // writer -> winning claim id
    refusals: BTreeMap<String, Why>,     // post id -> why (told to the author device)
}
struct Device {
    id: Identity,
    slot: u64, // random device ID / route slot; owner-replaceable, never key-derived
    w: Writer,
    board: Board,
    view: View,
    group_key: SigningKey,
    my_note: EpNote,
    private_key: SigningKey,
    retained: Vec<String>,
    acks: BTreeMap<String, u64>, // host key -> seq of my note that host reported holding
    outbox: Vec<Post>,
    sent: Vec<Post>,
    refused: Vec<(Post, Why)>,
    op_out: Vec<SignedOp>,
    online: bool,
    host: Option<HostState>,
    private_book: BTreeMap<String, String>,
    blocked: BTreeMap<String, BlockNotice>, // notices I sent (returned on later attempts)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RouteErr {
    SelfOffline,
    PeerOffline,
    EndpointGone,
    Partitioned,
    NoAddress,
    Lost,
    Refused,
    Removed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Path {
    MemberDialedHost,
    HostDialedMember,
}
#[derive(Debug, PartialEq, Eq)]
enum Direct {
    Delivered,
    Blocked(Vec<u8>),
    Unreachable,
}
#[derive(Clone, Debug)]
struct Frame {
    from: usize,
    to: usize,
    to_ep: String,
    kind: &'static str,
    bytes: Vec<u8>,
}
struct World {
    cfg: Cfg,
    rng: SimRng,
    now: u64,
    g: Genesis,
    gid: String,
    d: Vec<Device>,
    grants: BTreeMap<String, MGrant>, // DID -> its admin/member grant (creator has none)
    hosts: Vec<usize>,
    rogue: BTreeSet<usize>, // hosts that ignore U9 removal (limitation fixture)
    live: BTreeMap<String, usize>,
    cut: BTreeSet<(usize, usize)>,
    frames: Vec<Frame>,
}
impl World {
    fn new(cfg: Cfg) -> Self {
        let mut rng = SimRng(cfg.seed);
        // olga alice bob carol hana hugo ivan
        let ids: Vec<Identity> = (30u8..37).map(ident).collect();
        let owners = [0usize, 1, 1, 2, 3, 4, 4, 5, 6];
        let g = Genesis::create(&ids[0], [7; 32]);
        let gid = g.id();
        let admin = [Cap::Read, Cap::Post, Cap::Delegate, Cap::Admin, Cap::Host];
        let mut grants = BTreeMap::new();
        for (k, caps, delegable, nonce) in [
            (1usize, &admin[..], true, 1u64), // alice: admin, appointed first
            (2, &admin[..], true, 20),        // bob: admin, appointed after the hosts
            (3, &[Cap::Read, Cap::Post][..], false, 2),
            (4, &[Cap::Read, Cap::Post][..], false, 3),
        ] {
            let gr = MGrant::issue(
                &ids[0],
                &gid,
                ids[k].did(),
                None,
                caps,
                delegable,
                nonce,
                None,
            );
            grants.insert(ids[k].did().to_string(), gr);
        }
        let roster: BTreeSet<String> = ids[..5].iter().map(|i| i.did().to_string()).collect();
        let mut d = vec![];
        for &k in &owners {
            let id = ids[k].clone();
            let writer = rng.writer();
            let slot = rng.next_u64();
            let group_key = SigningKey::from_bytes(&rng.bytes32());
            let private_key = SigningKey::from_bytes(&rng.bytes32());
            let my_note = EpNote::sign(
                &id,
                Audience::Group(gid.clone()),
                slot,
                &ep("grp", &group_key),
                1,
            );
            d.push(Device {
                w: Writer::new(id.clone(), writer),
                id,
                slot,
                board: Board::new(gid.clone(), roster.clone()),
                view: View::default(),
                group_key,
                my_note,
                private_key,
                retained: vec![],
                acks: BTreeMap::new(),
                outbox: vec![],
                sent: vec![],
                refused: vec![],
                op_out: vec![],
                online: true,
                host: None,
                private_book: BTreeMap::new(),
                blocked: BTreeMap::new(),
            });
        }
        let mut w = World {
            cfg,
            rng,
            now: 0,
            g,
            gid,
            d,
            grants,
            hosts: vec![],
            rogue: BTreeSet::new(),
            live: BTreeMap::new(),
            cut: BTreeSet::new(),
            frames: vec![],
        };
        for i in 0..w.d.len() {
            let (ge, pe) = (w.gep(i), w.pep(i));
            w.live.insert(ge, i);
            w.live.insert(pe, i);
        }
        // Fixture: accepted contacts already hold each other's private endpoint (first device).
        for i in MEMBERS {
            for j in MEMBERS {
                let dj = w.did(j);
                if dj != w.did(i) && !w.d[i].private_book.contains_key(&dj) {
                    let e = w.pep(j);
                    w.d[i].private_book.insert(dj, e);
                }
            }
        }
        for h in w.cfg.hosts.clone() {
            let permit = w.default_permit(h);
            w.make_host(h, permit);
        }
        // Join bootstrap (invite/ticket) is out of scope: every member and host accepts every
        // host record; members hand their group note to every host.
        for h in w.hosts.clone() {
            w.publish_record(h).unwrap();
            for m in MEMBERS {
                let n = w.d[m].my_note.wire();
                let hep = w.gep(h);
                w.send(m, h, hep, "join-note", n.clone());
                w.host_accept_note(h, &n).unwrap();
                let k = w.hk(h);
                w.d[m].acks.insert(k, 1);
            }
        }
        w
    }
    fn did(&self, i: usize) -> String {
        self.d[i].id.did().to_string()
    }
    fn hk(&self, i: usize) -> String {
        dir_key(&self.did(i), self.d[i].slot)
    }
    fn gep(&self, i: usize) -> String {
        ep("grp", &self.d[i].group_key)
    }
    fn pep(&self, i: usize) -> String {
        ep("prv", &self.d[i].private_key)
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
    fn cut(&mut self, a: usize, b: usize) {
        self.cut.insert(pair(a, b));
    }
    fn heal(&mut self, a: usize, b: usize) {
        self.cut.remove(&pair(a, b));
    }
    fn dial(&self, from: usize, to_ep: &str) -> Result<usize, RouteErr> {
        if !self.d[from].online {
            return Err(RouteErr::SelfOffline);
        }
        let Some(&j) = self.live.get(to_ep) else {
            return Err(RouteErr::EndpointGone);
        };
        if !self.d[j].online {
            return Err(RouteErr::PeerOffline);
        }
        if self.cut.contains(&pair(from, j)) {
            return Err(RouteErr::Partitioned);
        }
        Ok(j)
    }
    // Full byte-level device state for rejection tests.
    fn snap(&self, i: usize) -> Vec<u8> {
        let d = &self.d[i];
        let refused: Vec<(&Post, &Why)> = d.refused.iter().map(|(p, w)| (p, w)).collect();
        serde_json::to_vec(&(
            d.board.snapshot(),
            &d.view,
            &d.my_note,
            &d.acks,
            &d.retained,
            &d.private_book,
            &d.outbox,
            refused,
            &d.host,
            &d.blocked,
        ))
        .unwrap()
    }
    fn received(&self, i: usize) -> Vec<u8> {
        let mut hay: Vec<u8> = self
            .frames
            .iter()
            .filter(|f| f.to == i)
            .flat_map(|f| f.bytes.clone())
            .collect();
        hay.extend(self.snap(i));
        hay
    }
    // ---- host appointment ----
    fn default_permit(&self, h: usize) -> MGrant {
        let (issuer, nonce) = match h {
            H1 => (A1, 10),   // alice appoints hana's desktop
            H2 => (OLGA, 11), // owner appoints hugo's host binary
            _ => (A1, 12),
        };
        self.permit(issuer, h, self.d[h].slot, nonce)
    }
    fn permit(&self, issuer: usize, h: usize, device: u64, nonce: u64) -> MGrant {
        let parent = self.grants.get(&self.did(issuer)).cloned();
        MGrant::issue(
            &self.d[issuer].id,
            &self.gid,
            &self.did(h),
            Some(device),
            &[Cap::Host],
            false,
            nonce,
            parent,
        )
    }
    fn make_host(&mut self, h: usize, permit: MGrant) {
        let record = HostRecord {
            note: self.d[h].my_note.clone(),
            permit,
        };
        self.d[h].host = Some(HostState {
            record,
            directory: BTreeMap::new(),
            held: BTreeMap::new(),
            claims: BTreeMap::new(),
            known_by: BTreeMap::new(),
            settled: BTreeMap::new(),
            refusals: BTreeMap::new(),
        });
        if !self.hosts.contains(&h) {
            self.hosts.push(h);
        }
    }
    // Fixture distribution of a new host record to every member and host (in practice it
    // travels via existing hosts; the members verify it either way).
    fn publish_record(&mut self, h: usize) -> Result<(), Refuse> {
        let rec = self.d[h].host.as_ref().unwrap().record.wire();
        let mut targets: Vec<usize> = MEMBERS.to_vec();
        targets.extend(self.hosts.iter().copied());
        for i in targets {
            if i != h {
                let e = self.gep(i);
                self.send(h, i, e, "host-record", rec.clone());
            }
            match self.accept_host_record(i, &rec) {
                Ok(()) | Err(Refuse::Stale) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn check_record(&self, view: &View, r: &HostRecord) -> Result<(), Refuse> {
        if !r.note.verify() {
            return Err(Refuse::BadSignature);
        }
        if r.note.claims.audience != Audience::Group(self.gid.clone()) {
            return Err(Refuse::WrongAudience);
        }
        if !r.permit.allows(&self.g, &r.note.claims.owner, Cap::Host) {
            return Err(Refuse::PermitInvalid);
        }
        if r.permit.body.device != Some(r.note.claims.slot) {
            return Err(Refuse::DeviceMismatch);
        }
        if view.revoked.contains(&r.permit.digest()) {
            return Err(Refuse::Revoked);
        }
        Ok(())
    }
    fn accept_host_record(&mut self, i: usize, bytes: &[u8]) -> Result<(), Refuse> {
        let Ok(r) = serde_json::from_slice::<HostRecord>(bytes) else {
            return Err(Refuse::Malformed);
        };
        self.check_record(&self.d[i].view, &r)?;
        let key = dir_key(&r.note.claims.owner, r.note.claims.slot);
        let view = &self.d[i].view;
        if let Some(cur) = view.hosts.get(&key) {
            if cur.note.claims.seq >= r.note.claims.seq {
                return Err(Refuse::Stale);
            }
        } else if let HostCap::Max(n) = self.cfg.cap {
            if view.hosts.len() >= n {
                return Err(Refuse::CapReached);
            }
        }
        self.d[i].view.hosts.insert(key, r);
        Ok(())
    }
    // `x` admits a link from host `y`: y presents its current record; x must accept it.
    fn admits(&mut self, x: usize, y: usize) -> Result<(), Refuse> {
        let Some(hs) = self.d[y].host.as_ref() else {
            return Err(Refuse::NotHost);
        };
        let r = hs.record.clone();
        match self.accept_host_record(x, &r.wire()) {
            Ok(()) | Err(Refuse::Stale) => {}
            Err(e) => return Err(e),
        }
        if self.d[x].view.hosts.get(&self.hk(y)) != Some(&r) {
            return Err(Refuse::Stale);
        }
        Ok(())
    }
    fn host_accept_note(&mut self, h: usize, bytes: &[u8]) -> Result<(), Refuse> {
        let Ok(n) = serde_json::from_slice::<EpNote>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !n.verify() {
            return Err(Refuse::BadSignature);
        }
        if n.claims.audience != Audience::Group(self.gid.clone()) {
            return Err(Refuse::WrongAudience);
        }
        if !self.d[h].board.roster.contains(&n.claims.owner) {
            return Err(Refuse::NotMember);
        }
        let key = dir_key(&n.claims.owner, n.claims.slot);
        let Some(hs) = self.d[h].host.as_mut() else {
            return Err(Refuse::NotHost);
        };
        if let Some(cur) = hs.directory.get(&key) {
            if cur.claims.seq >= n.claims.seq {
                return Err(Refuse::Stale);
            }
        }
        hs.directory.insert(key, n);
        Ok(())
    }
    // Every device verifies every op itself; all checks run before any mutation.
    fn accept_op(&mut self, i: usize, bytes: &[u8]) -> Result<bool, Refuse> {
        let Ok(op) = serde_json::from_slice::<SignedOp>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !op.verify() {
            return Err(Refuse::BadSignature);
        }
        if op.body.group != self.gid {
            return Err(Refuse::WrongGroup);
        }
        let id = op.id();
        if self.d[i].view.ops.contains_key(&id) {
            return Ok(false);
        }
        let issuer = op.body.issuer.clone();
        let is_admin = issuer == self.g.body.creator
            || op
                .body
                .issuer_grant
                .as_ref()
                .is_some_and(|gr| gr.allows(&self.g, &issuer, Cap::Admin));
        if !is_admin {
            return Err(Refuse::NotAdmin);
        }
        let honest = !self.rogue.contains(&i);
        match op.body.op.clone() {
            Op::Admit { grant } => {
                let holder = grant.body.holder.clone();
                if !grant.allows(&self.g, &holder, Cap::Post) {
                    return Err(Refuse::PermitInvalid);
                }
                self.d[i].board.roster.insert(holder);
            }
            Op::Flag { target } => {
                let dv = &mut self.d[i];
                if !dv.board.roster.contains(&target) && !dv.view.removed.contains(&target) {
                    return Err(Refuse::NotMember);
                }
                let n = {
                    let fl = dv.view.flags.entry(target.clone()).or_default();
                    fl.insert(issuer.clone());
                    fl.len()
                };
                if n >= REMOVAL_FLAGS && dv.view.removed.insert(target.clone()) && honest {
                    dv.board.roster.remove(&target);
                    if let Some(hs) = dv.host.as_mut() {
                        hs.directory.retain(|_, n| n.claims.owner != target);
                    }
                }
            }
            Op::RemoveHost { permit } => {
                let Some((key, rec)) = self.d[i]
                    .view
                    .hosts
                    .iter()
                    .find(|(_, r)| r.permit.digest() == permit)
                    .map(|(k, r)| (k.clone(), r.clone()))
                else {
                    return Err(Refuse::UnknownHost);
                };
                if self.cfg.removal == RemovalRule::AdminAppointedAfterHost {
                    let appointed = op.body.issuer_grant.as_ref().map_or(0, |g| g.body.nonce);
                    if appointed <= rec.permit.body.nonce {
                        return Err(Refuse::NotAppointedAfter);
                    }
                }
                self.d[i].view.hosts.remove(&key);
                self.d[i].view.revoked.insert(permit);
            }
        }
        self.d[i].view.ops.insert(id, op);
        Ok(true)
    }
    fn op(&mut self, i: usize, op: Op) -> SignedOp {
        let grant = self.grants.get(&self.did(i)).cloned();
        let nonce = self.rng.next_u64();
        SignedOp::sign(&self.d[i].id, &self.gid, grant, nonce, op)
    }
    fn submit(&mut self, i: usize, op: SignedOp) {
        let _ = self.accept_op(i, &op.wire());
        self.d[i].op_out.push(op);
    }
    // ---- content ----
    fn post(&mut self, m: usize, text: &str) -> Post {
        let dv = &mut self.d[m];
        dv.w.observe(&dv.board);
        let p = dv.w.post(&self.gid, text);
        dv.outbox.push(p.clone());
        p
    }
    fn host_receive(&mut self, h: usize, p: Post) -> Verdict {
        let id = p.id();
        let v = match self.cfg.claim {
            ClaimRule::FirstAtHost => self.d[h].board.receive(p),
            ClaimRule::LowestAfterAllHostsSaw => {
                let hs = self.d[h].host.as_ref().unwrap();
                match hs.settled.get(&p.body.writer) {
                    Some(win) if hs.claims[&p.body.writer][win] != p.body.author => {
                        Verdict::Rejected(Why::WriterOwnedByOtherDid)
                    }
                    Some(_) => self.d[h].board.receive(p),
                    None => match self.d[h].board.precheck(&p) {
                        Err(why) => Verdict::Rejected(why),
                        Ok(()) => {
                            let hs = self.d[h].host.as_mut().unwrap();
                            if p.body.seq == 1 {
                                hs.claims
                                    .entry(p.body.writer)
                                    .or_default()
                                    .insert(id.clone(), p.body.author.clone());
                                hs.known_by.entry(id.clone()).or_default().insert(h);
                            }
                            hs.held.insert(id.clone(), p);
                            Verdict::Pending
                        }
                    },
                }
            }
        };
        if let Verdict::Rejected(why) = v {
            self.d[h].host.as_mut().unwrap().refusals.insert(id, why);
        }
        v
    }
    fn settle(&mut self, h: usize) {
        if self.cfg.claim != ClaimRule::LowestAfterAllHostsSaw {
            return;
        }
        let all: BTreeSet<usize> = self.hosts.iter().copied().collect();
        let hs = self.d[h].host.as_mut().unwrap();
        for (writer, claims) in hs.claims.clone() {
            if hs.settled.contains_key(&writer) {
                continue;
            }
            let seen_everywhere = claims
                .keys()
                .all(|c| hs.known_by.get(c).is_some_and(|s| s.is_superset(&all)));
            if seen_everywhere {
                let win = claims.keys().next().unwrap().clone(); // lowest claim id
                hs.settled.insert(writer, win);
            }
        }
        let mut held: Vec<Post> = hs.held.values().cloned().collect();
        held.sort_by_key(|p| p.body.seq);
        for p in held {
            let hs = self.d[h].host.as_ref().unwrap();
            let Some(win) = hs.settled.get(&p.body.writer) else {
                continue;
            };
            let owner = hs.claims[&p.body.writer][win].clone();
            let id = p.id();
            self.d[h].host.as_mut().unwrap().held.remove(&id);
            let v = if owner == p.body.author {
                self.d[h].board.receive(p)
            } else {
                Verdict::Rejected(Why::WriterOwnedByOtherDid)
            };
            if let Verdict::Rejected(why) = v {
                self.d[h].host.as_mut().unwrap().refusals.insert(id, why);
            }
        }
    }
    // ---- links ----
    // Member/host link: member dials the host it knows, else the host dials the member's
    // directory endpoint (U1 "side whose address is still live"). Then both admit and sync.
    fn connect(&mut self, m: usize, h: usize) -> Result<Path, RouteErr> {
        if !self.d[m].online {
            return Err(RouteErr::SelfOffline);
        }
        if !self.d[h].online {
            return Err(RouteErr::PeerOffline);
        }
        let hkey = self.hk(h);
        let member_side = self.d[m]
            .view
            .hosts
            .get(&hkey)
            .map(|r| r.note.claims.endpoint.clone())
            .and_then(|e| self.dial(m, &e).ok())
            == Some(h);
        let removed = !self.rogue.contains(&h) && self.d[h].view.removed.contains(&self.did(m));
        if removed {
            if member_side {
                self.removal_notice(m, h);
                return Err(RouteErr::Removed);
            }
            return Err(RouteErr::Lost);
        }
        let path = if member_side {
            Path::MemberDialedHost
        } else {
            let e = self.d[h]
                .host
                .as_ref()
                .and_then(|hs| hs.directory.get(&self.hk(m)))
                .map(|n| n.claims.endpoint.clone());
            match e.map(|e| self.dial(h, &e)) {
                Some(Ok(j)) if j == m => Path::HostDialedMember,
                _ => return Err(RouteErr::Lost),
            }
        };
        if self.admits(m, h).is_err() {
            return Err(RouteErr::Refused);
        }
        self.exchange(m, h);
        Ok(path)
    }
    // Host-initiated only (section 9 Q6): the host dials the member's current endpoint.
    fn connect_from_host(&mut self, h: usize, m: usize) -> Result<Path, RouteErr> {
        let Some(e) = self.d[h]
            .host
            .as_ref()
            .and_then(|hs| hs.directory.get(&self.hk(m)))
            .map(|n| n.claims.endpoint.clone())
        else {
            return Err(RouteErr::NoAddress);
        };
        if self.dial(h, &e)? != m {
            return Err(RouteErr::EndpointGone);
        }
        if self.admits(m, h).is_err() {
            return Err(RouteErr::Refused);
        }
        self.exchange(m, h);
        Ok(Path::HostDialedMember)
    }
    // Honest host answering a removed member: the signed flags are the notice (U5 revised).
    fn removal_notice(&mut self, m: usize, h: usize) {
        let me = self.did(m);
        let ops: Vec<SignedOp> = self.d[h]
            .view
            .ops
            .values()
            .filter(|o| matches!(&o.body.op, Op::Flag { target } if *target == me))
            .cloned()
            .collect();
        let e = self.gep(m);
        for o in ops {
            self.send(h, m, e.clone(), "removal-notice", o.wire());
            let _ = self.accept_op(m, &o.wire());
        }
    }
    fn exchange(&mut self, m: usize, h: usize) {
        let (hep, mep) = (self.gep(h), self.gep(m));
        let (hkey, mkey) = (self.hk(h), self.hk(m));
        // member -> host
        let note = self.d[m].my_note.clone();
        if self.d[m].acks.get(&hkey).copied().unwrap_or(0) < note.claims.seq {
            self.send(m, h, hep.clone(), "ep-note", note.wire());
            let _ = self.host_accept_note(h, &note.wire());
        }
        for o in std::mem::take(&mut self.d[m].op_out) {
            self.send(m, h, hep.clone(), "op", o.wire());
            let _ = self.accept_op(h, &o.wire());
        }
        for p in std::mem::take(&mut self.d[m].outbox) {
            self.send(m, h, hep.clone(), "post", p.wire());
            match self.host_receive(h, p.clone()) {
                Verdict::Rejected(why) => self.d[m].refused.push((p, why)),
                _ => self.d[m].sent.push(p),
            }
        }
        // host -> member
        let recs: Vec<HostRecord> = self.d[h].view.hosts.values().cloned().collect();
        for r in recs {
            let k = dir_key(&r.note.claims.owner, r.note.claims.slot);
            if self.d[m].view.hosts.get(&k) != Some(&r) {
                self.send(h, m, mep.clone(), "host-record", r.wire());
                let _ = self.accept_host_record(m, &r.wire());
            }
        }
        let ops: Vec<SignedOp> = self.d[h]
            .view
            .ops
            .iter()
            .filter(|(id, _)| !self.d[m].view.ops.contains_key(*id))
            .map(|(_, o)| o.clone())
            .collect();
        for o in ops {
            self.send(h, m, mep.clone(), "op", o.wire());
            let _ = self.accept_op(m, &o.wire());
        }
        let missing: Vec<Post> = self.d[h]
            .board
            .order
            .iter()
            .filter(|id| !self.d[m].board.accepted.contains_key(*id))
            .map(|id| self.d[h].board.accepted[id].clone())
            .collect();
        for p in missing {
            self.send(h, m, mep.clone(), "post", p.wire());
            self.d[m].board.receive(p);
        }
        let hs = self.d[h].host.as_ref().unwrap();
        let acked = hs.directory.get(&mkey).map(|n| n.claims.seq);
        let refusals = hs.refusals.clone();
        if let Some(s) = acked {
            self.d[m].acks.insert(hkey, s);
        }
        let sent = std::mem::take(&mut self.d[m].sent);
        for p in sent {
            match refusals.get(&p.id()) {
                Some(why) => self.d[m].refused.push((p, *why)),
                None => self.d[m].sent.push(p),
            }
        }
        self.release_retained(m);
        let dv = &mut self.d[m];
        dv.w.observe(&dv.board);
    }
    fn release_retained(&mut self, m: usize) {
        let seq = self.d[m].my_note.claims.seq;
        let dv = &self.d[m];
        let all_acked = dv
            .view
            .hosts
            .keys()
            .all(|k| dv.acks.get(k).copied().unwrap_or(0) >= seq);
        if all_acked {
            for e in std::mem::take(&mut self.d[m].retained) {
                self.live.remove(&e);
            }
        }
    }
    fn host_sync(&mut self, a: usize, b: usize) -> Result<(), RouteErr> {
        let (ka, kb) = (self.hk(a), self.hk(b));
        let ea = self.d[b]
            .view
            .hosts
            .get(&ka)
            .map(|r| r.note.claims.endpoint.clone());
        let eb = self.d[a]
            .view
            .hosts
            .get(&kb)
            .map(|r| r.note.claims.endpoint.clone());
        let (Some(ea), Some(eb)) = (ea, eb) else {
            return Err(RouteErr::Refused);
        };
        if self.dial(a, &eb) != Ok(b) && self.dial(b, &ea) != Ok(a) {
            return Err(RouteErr::Lost);
        }
        if self.admits(a, b).is_err() || self.admits(b, a).is_err() {
            return Err(RouteErr::Refused);
        }
        for _ in 0..2 {
            self.xfer(a, b);
            self.xfer(b, a);
            self.settle(a);
            self.settle(b);
        }
        Ok(())
    }
    fn xfer(&mut self, from: usize, to: usize) {
        let to_ep = self.gep(to);
        let recs: Vec<HostRecord> = self.d[from].view.hosts.values().cloned().collect();
        for r in recs {
            let k = dir_key(&r.note.claims.owner, r.note.claims.slot);
            if self.d[to].view.hosts.get(&k) != Some(&r) {
                self.send(from, to, to_ep.clone(), "host-record", r.wire());
                let _ = self.accept_host_record(to, &r.wire());
            }
        }
        let ops: Vec<SignedOp> = self.d[from]
            .view
            .ops
            .iter()
            .filter(|(id, _)| !self.d[to].view.ops.contains_key(*id))
            .map(|(_, o)| o.clone())
            .collect();
        for o in ops {
            self.send(from, to, to_ep.clone(), "op", o.wire());
            let _ = self.accept_op(to, &o.wire());
        }
        let notes: Vec<EpNote> = self.d[from]
            .host
            .as_ref()
            .unwrap()
            .directory
            .values()
            .cloned()
            .collect();
        for n in notes {
            let k = dir_key(&n.claims.owner, n.claims.slot);
            let cur = self.d[to]
                .host
                .as_ref()
                .unwrap()
                .directory
                .get(&k)
                .map(|c| c.claims.seq);
            if cur < Some(n.claims.seq) {
                self.send(from, to, to_ep.clone(), "dir-note", n.wire());
                let _ = self.host_accept_note(to, &n.wire());
            }
        }
        let fb = &self.d[from];
        let mut posts: Vec<Post> = fb
            .board
            .order
            .iter()
            .map(|id| fb.board.accepted[id].clone())
            .collect();
        posts.extend(fb.host.as_ref().unwrap().held.values().cloned());
        for p in posts {
            let id = p.id();
            let t = &self.d[to];
            let ths = t.host.as_ref().unwrap();
            if t.board.accepted.contains_key(&id)
                || t.board.pending.contains_key(&id)
                || ths.held.contains_key(&id)
                || ths.refusals.contains_key(&id)
            {
                continue;
            }
            self.send(from, to, to_ep.clone(), "post", p.wire());
            self.host_receive(to, p);
        }
        if self.cfg.claim == ClaimRule::LowestAfterAllHostsSaw {
            let kb = self.d[from].host.as_ref().unwrap().known_by.clone();
            let ths = self.d[to].host.as_mut().unwrap();
            for (c, set) in kb {
                if ths.claims.values().any(|cl| cl.contains_key(&c)) {
                    let e = ths.known_by.entry(c).or_default();
                    e.extend(set);
                    e.insert(to);
                }
            }
        }
    }
    fn round(&mut self) {
        for _ in 0..2 {
            let hs = self.hosts.clone();
            for (x, &a) in hs.iter().enumerate() {
                for &b in &hs[x + 1..] {
                    let _ = self.host_sync(a, b);
                }
            }
            for m in MEMBERS {
                for &h in &hs {
                    let _ = self.connect(m, h);
                }
            }
        }
    }
    // ---- rotation ----
    // Periodic member group-endpoint rotation; the note goes to EVERY host it can reach.
    fn rotate_member(&mut self, m: usize) -> Vec<usize> {
        let old = self.gep(m);
        match self.cfg.retention {
            Retention::None => {
                self.live.remove(&old);
            }
            Retention::UntilHostAck => self.d[m].retained.push(old),
        }
        self.d[m].group_key = SigningKey::from_bytes(&self.rng.bytes32());
        let seq = self.d[m].my_note.claims.seq + 1;
        let new = self.gep(m);
        let note = EpNote::sign(
            &self.d[m].id,
            Audience::Group(self.gid.clone()),
            self.d[m].slot,
            &new,
            seq,
        );
        self.d[m].my_note = note.clone();
        self.live.insert(new, m);
        let mut reached = vec![];
        for h in self.hosts.clone() {
            let Some(e) = self.d[m]
                .view
                .hosts
                .get(&self.hk(h))
                .map(|r| r.note.claims.endpoint.clone())
            else {
                continue;
            };
            if self.dial(m, &e) == Ok(h) {
                self.send(m, h, e, "ep-note", note.wire());
                if self.host_accept_note(h, &note.wire()).is_ok() {
                    let k = self.hk(h);
                    self.d[m].acks.insert(k, seq);
                    reached.push(h);
                }
            }
        }
        self.release_retained(m);
        reached
    }
    // Host rotation: no overlap window; the host itself dials every member at its current
    // directory endpoint and hands over its new record, then syncs peer hosts (Q6).
    fn rotate_host(&mut self, h: usize) -> Vec<usize> {
        let old = self.gep(h);
        self.live.remove(&old);
        self.d[h].group_key = SigningKey::from_bytes(&self.rng.bytes32());
        let seq = self.d[h].my_note.claims.seq + 1;
        let new = self.gep(h);
        let note = EpNote::sign(
            &self.d[h].id,
            Audience::Group(self.gid.clone()),
            self.d[h].slot,
            &new,
            seq,
        );
        self.d[h].my_note = note.clone();
        self.live.insert(new, h);
        self.d[h].host.as_mut().unwrap().record.note = note;
        let rec = self.d[h].host.as_ref().unwrap().record.wire();
        let _ = self.accept_host_record(h, &rec);
        let dir: Vec<EpNote> = self.d[h]
            .host
            .as_ref()
            .unwrap()
            .directory
            .values()
            .cloned()
            .collect();
        let mut reached = vec![];
        for n in dir {
            if let Ok(j) = self.dial(h, &n.claims.endpoint) {
                if self.did(j) == n.claims.owner {
                    self.send(h, j, n.claims.endpoint.clone(), "host-record", rec.clone());
                    if self.admits(j, h).is_ok() {
                        reached.push(j);
                    }
                }
            }
        }
        for other in self.hosts.clone() {
            if other != h {
                let _ = self.host_sync(h, other);
            }
        }
        reached
    }
    fn redraw_writer(&mut self, m: usize) -> u64 {
        let new = self.rng.writer();
        let id = self.d[m].id.clone();
        self.d[m].w = Writer::new(id, new);
        let mut texts: Vec<String> = self.d[m]
            .refused
            .drain(..)
            .map(|(p, _)| p.body.text)
            .collect();
        texts.extend(self.d[m].outbox.drain(..).map(|p| p.body.text));
        for t in texts {
            self.post(m, &t);
        }
        new
    }
    // ---- personal block (private audience, U5 revised) ----
    fn block(&mut self, i: usize, target: &str) -> Vec<u8> {
        let body = BlockBody {
            from: self.did(i),
            to: target.into(),
            at: self.now,
        };
        let sig = self.d[i].id.sign(&signed(NOTICE_DOMAIN, &body)).to_vec();
        let notice = BlockNotice { body, sig };
        let bytes = serde_json::to_vec(&notice).unwrap();
        self.d[i].blocked.insert(target.into(), notice);
        if let Some(e) = self.d[i].private_book.get(target).cloned() {
            if let Ok(j) = self.dial(i, &e) {
                self.send(i, j, e, "block-notice", bytes.clone());
                let _ = self.receive_block_notice(j, &bytes);
            }
        }
        bytes
    }
    fn receive_block_notice(&mut self, j: usize, bytes: &[u8]) -> Result<(), Refuse> {
        let Ok(n) = serde_json::from_slice::<BlockNotice>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !valid(&n.body.from, &signed(NOTICE_DOMAIN, &n.body), &n.sig) {
            return Err(Refuse::BadSignature);
        }
        if n.body.to != self.did(j) {
            return Err(Refuse::NotAddressed);
        }
        self.d[j].view.blocked_by.insert(n.body.from.clone(), n);
        Ok(())
    }
    // Direct private-endpoint attempt; a blocker answers with the same signed notice.
    fn direct(&mut self, i: usize, to: &str) -> Direct {
        let Some(e) = self.d[i].private_book.get(to).cloned() else {
            return Direct::Unreachable;
        };
        let Ok(j) = self.dial(i, &e) else {
            return Direct::Unreachable;
        };
        let me = self.did(i);
        if let Some(n) = self.d[j].blocked.get(&me) {
            let b = serde_json::to_vec(n).unwrap();
            let back = self.pep(i);
            self.send(j, i, back, "block-notice", b.clone());
            return Direct::Blocked(b);
        }
        self.send(i, j, e, "direct", vec![]);
        Direct::Delivered
    }
    fn assert_converged(&self, devs: &[usize]) {
        let first = &self.d[devs[0]].board;
        for &i in devs {
            let b = &self.d[i].board;
            assert!(b.pending.is_empty(), "device {i} has pending posts");
            assert_eq!(
                b.accepted.keys().collect::<Vec<_>>(),
                first.accepted.keys().collect::<Vec<_>>(),
                "device {i} accepted set differs from device {}",
                devs[0]
            );
            assert_eq!(b.doc.get_deep_value(), first.doc.get_deep_value());
        }
    }
}
fn world() -> World {
    World::new(Cfg::default())
}
fn everyone() -> Vec<usize> {
    let mut v = MEMBERS.to_vec();
    v.extend([H1, H2]);
    v
}
fn set(xs: &[&str]) -> BTreeSet<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

// ---- tests ---------------------------------------------------------------------------------

// KIND: invariant
// Q5: a host is appointed by a permit in the grant/delegation chain (creator -> admin ->
// Host leaf bound to one device). Members refuse each single-cause bad record with their
// whole state byte-identical: a member grant presented as permit, a permit issued by a
// non-admin (under her member grant, or self-rooted), a missing permit, a tampered device,
// a permit for another device, a note signed by a non-holder, a permit lacking `Host`.
// Positive control: the same host with a valid admin-issued permit is accepted and relays.
// After an admin removes that permit (signed op), the record is refused as Revoked and the
// host's links are refused, state unchanged.
#[test]
fn host_permit_must_chain_to_an_admin_and_bind_the_device_else_members_refuse() {
    let mut w = world();
    let ivan = w.did(H3);
    let slot = w.d[H3].slot;
    let gid = w.gid.clone();
    let carol_grant = w.grants[&w.did(CAROL)].clone();
    let alice_admin = w.grants[&w.did(A1)].clone();
    let (carol_id, alice_id) = (w.d[CAROL].id.clone(), w.d[A1].id.clone());
    let note = w.d[H3].my_note.clone();
    let good = w.permit(A1, H3, slot, 12);
    let rec = |note: &EpNote, permit: MGrant| {
        HostRecord {
            note: note.clone(),
            permit,
        }
        .wire()
    };
    let mallory = ident(37);
    let mallory_note = EpNote::sign(&mallory, Audience::Group(gid.clone()), slot, &w.gep(H3), 1);
    let mut tampered = good.clone();
    tampered.body.device = Some(slot ^ 1);
    let cases: Vec<(&str, Vec<u8>, Refuse)> = vec![
        (
            "member grant as permit",
            rec(&note, carol_grant.clone()),
            Refuse::PermitInvalid,
        ),
        (
            "non-admin issues under her member grant",
            rec(
                &note,
                MGrant::issue(
                    &carol_id,
                    &gid,
                    &ivan,
                    Some(slot),
                    &[Cap::Host],
                    false,
                    40,
                    Some(carol_grant.clone()),
                ),
            ),
            Refuse::PermitInvalid,
        ),
        (
            "non-admin self-rooted",
            rec(
                &note,
                MGrant::issue(
                    &carol_id,
                    &gid,
                    &ivan,
                    Some(slot),
                    &[Cap::Host],
                    false,
                    41,
                    None,
                ),
            ),
            Refuse::PermitInvalid,
        ),
        (
            "no permit at all",
            serde_json::to_vec(&serde_json::json!({ "note": note })).unwrap(),
            Refuse::Malformed,
        ),
        (
            "tampered device",
            rec(&note, tampered),
            Refuse::PermitInvalid,
        ),
        (
            "permit for another device",
            rec(
                &note,
                MGrant::issue(
                    &alice_id,
                    &gid,
                    &ivan,
                    Some(slot ^ 1),
                    &[Cap::Host],
                    false,
                    42,
                    Some(alice_admin.clone()),
                ),
            ),
            Refuse::DeviceMismatch,
        ),
        (
            "note signed by non-holder",
            rec(&mallory_note, good.clone()),
            Refuse::PermitInvalid,
        ),
        (
            "permit lacks Host",
            rec(
                &note,
                MGrant::issue(
                    &alice_id,
                    &gid,
                    &ivan,
                    Some(slot),
                    &[Cap::Post],
                    false,
                    43,
                    Some(alice_admin.clone()),
                ),
            ),
            Refuse::PermitInvalid,
        ),
    ];
    for i in everyone() {
        for (name, bytes, want) in &cases {
            let before = w.snap(i);
            assert_eq!(w.accept_host_record(i, bytes), Err(*want), "{name} at {i}");
            assert_eq!(w.snap(i), before, "{name} at {i}");
        }
    }
    // H3 cannot open links: it is not a host anywhere.
    assert!(
        MEMBERS
            .iter()
            .all(|&m| !w.d[m].view.hosts.contains_key(&w.hk(H3)))
    );

    // Positive control: valid permit (alice, admin) -> accepted; H3 relays a post.
    w.make_host(H3, good.clone());
    w.publish_record(H3).unwrap();
    for i in everyone() {
        assert!(w.d[i].view.hosts.contains_key(&w.hk(H3)));
    }
    for m in [BOB, CAROL] {
        w.cut(m, H1);
        w.cut(m, H2);
        w.connect(m, H3).unwrap();
    }
    w.post(BOB, "via ivan");
    w.connect(BOB, H3).unwrap();
    w.connect(CAROL, H3).unwrap();
    assert!(w.d[CAROL].board.texts().contains("via ivan"));
    assert!(
        w.frames
            .iter()
            .filter(|f| f.to == CAROL && f.kind == "post")
            .all(|f| f.from == H3)
    );

    // Revoked: bob (admin) removes H3's permit; afterwards the record is refused unchanged.
    let op = w.op(
        BOB,
        Op::RemoveHost {
            permit: good.digest(),
        },
    );
    w.heal(BOB, H1);
    w.submit(BOB, op);
    w.heal(CAROL, H1);
    w.round();
    let good_rec = w.d[H3].host.as_ref().unwrap().record.wire();
    for m in [BOB, CAROL] {
        let k = w.hk(m);
        assert!(w.d[H3].host.as_ref().unwrap().directory.contains_key(&k));
    }
    for m in MEMBERS {
        assert!(!w.d[m].view.hosts.contains_key(&w.hk(H3)));
        let before = w.snap(m);
        assert_eq!(w.accept_host_record(m, &good_rec), Err(Refuse::Revoked));
        // Members whose endpoint H3 learned (it can dial them) refuse it; any others it
        // never had an address for.
        let known = w.d[H3]
            .host
            .as_ref()
            .unwrap()
            .directory
            .contains_key(&w.hk(m));
        let want = if known {
            RouteErr::Refused
        } else {
            RouteErr::Lost
        };
        assert_eq!(w.connect(m, H3), Err(want));
        assert!(w.connect_from_host(H3, m).is_err());
        assert_eq!(w.snap(m), before);
    }
}

// KIND: invariant
// Q5 "hosts are NOT necessarily admins": hugo's host binary (own DID, permit {Host} only)
// relays posts, but every authority act it signs is refused with state byte-identical at
// every member and at the other host: admitting a new member, flagging a member, removing a
// host, appointing another host. A grant it issues lets nobody post. Positive controls: the
// owner's admit of the same newcomer is accepted (NotAdmin is the sole cause), and H2's relay
// reaches members cut off from H1.
#[test]
fn host_without_admin_capability_relays_but_cannot_grant_flag_remove_or_appoint() {
    let mut w = world();
    let hugo = w.d[H2].id.clone();
    let permit = w.d[H2].host.as_ref().unwrap().record.permit.clone();
    let gid = w.gid.clone();
    let mallory = ident(37);
    let h1_permit = w.d[H1].host.as_ref().unwrap().record.permit.digest();
    let hugo_grant = MGrant::issue(
        &hugo,
        &gid,
        mallory.did(),
        None,
        &[Cap::Read, Cap::Post],
        false,
        50,
        Some(permit.clone()),
    );
    assert!(!hugo_grant.allows(&w.g, mallory.did(), Cap::Post));
    let ops = [
        Op::Admit {
            grant: hugo_grant.clone(),
        },
        Op::Flag {
            target: w.did(CAROL),
        },
        Op::RemoveHost { permit: h1_permit },
    ];
    let appoint = HostRecord {
        note: w.d[H3].my_note.clone(),
        permit: MGrant::issue(
            &hugo,
            &gid,
            &w.did(H3),
            Some(w.d[H3].slot),
            &[Cap::Host],
            false,
            51,
            Some(permit.clone()),
        ),
    }
    .wire();
    let mut mw = Writer::new(mallory.clone(), 0xABCD);
    let mallory_post = mw.post(&gid, "mallory via hugo's grant");
    let mut hw = Writer::new(hugo.clone(), 0xABCE);
    let hugo_post = hw.post(&gid, "hugo speaks");
    for i in MEMBERS.iter().copied().chain([H1]) {
        let before = w.snap(i);
        for (k, op) in ops.iter().enumerate() {
            let s = SignedOp::sign(&hugo, &gid, Some(permit.clone()), 60 + k as u64, op.clone());
            assert_eq!(w.accept_op(i, &s.wire()), Err(Refuse::NotAdmin), "op {k}");
        }
        assert_eq!(
            w.accept_host_record(i, &appoint),
            Err(Refuse::PermitInvalid)
        );
        for p in [&mallory_post, &hugo_post] {
            assert_eq!(
                w.d[i].board.receive(p.clone()),
                Verdict::Rejected(Why::NotMember)
            );
        }
        assert_eq!(w.snap(i), before, "device {i}");
    }
    // Relay works.
    for m in [BOB, CAROL] {
        w.cut(m, H1);
    }
    w.post(BOB, "relayed by hugo");
    w.connect(BOB, H2).unwrap();
    w.connect(CAROL, H2).unwrap();
    assert!(w.d[CAROL].board.texts().contains("relayed by hugo"));
    // Positive control: the owner's admit of mallory is accepted.
    let good = MGrant::issue(
        &w.d[OLGA].id,
        &gid,
        mallory.did(),
        None,
        &[Cap::Read, Cap::Post],
        false,
        52,
        None,
    );
    let admit = w.op(OLGA, Op::Admit { grant: good });
    assert_eq!(w.accept_op(CAROL, &admit.wire()), Ok(true));
    assert!(w.d[CAROL].board.roster.contains(mallory.did()));
}

// KIND: invariant
// Q5 "a host is one specific device of some user": the permit names hana's desktop (H1)
// by its random device ID. Her phone (HP, same DID, same mnemonic) honestly presenting the
// same permit with its own device ID is refused (DeviceMismatch); a permit hana issues to
// herself is refused (she is not an admin); state unchanged everywhere. HP remains an
// ordinary member: its posts converge through the real hosts. Device IDs are random, not
// derived from any key.
#[test]
fn host_permit_binds_one_device_and_a_sibling_of_the_same_did_is_not_a_host() {
    let mut w = world();
    assert_eq!(
        w.did(HP),
        w.did(H1),
        "fixture: hana's phone and desktop share a DID"
    );
    assert_ne!(w.d[HP].slot, w.d[H1].slot);
    for i in [HP, H1] {
        let keys = [
            w.d[i].id.signing_public_key(),
            w.d[i].id.device_public_key(),
            w.d[i].group_key.verifying_key().to_bytes(),
        ];
        for k in keys {
            for c in k.chunks(8) {
                assert_ne!(u64::from_le_bytes(c.try_into().unwrap()), w.d[i].slot);
            }
        }
    }
    let h1_permit = w.d[H1].host.as_ref().unwrap().record.permit.clone();
    let hana = w.d[HP].id.clone();
    let gid = w.gid.clone();
    let sibling = HostRecord {
        note: EpNote::sign(
            &hana,
            Audience::Group(gid.clone()),
            w.d[HP].slot,
            &w.gep(HP),
            2,
        ),
        permit: h1_permit,
    }
    .wire();
    let self_appointed = HostRecord {
        note: w.d[HP].my_note.clone(),
        permit: MGrant::issue(
            &hana,
            &gid,
            hana.did(),
            Some(w.d[HP].slot),
            &[Cap::Host],
            false,
            70,
            w.grants.get(hana.did()).cloned(),
        ),
    }
    .wire();
    for i in everyone() {
        let before = w.snap(i);
        assert_eq!(
            w.accept_host_record(i, &sibling),
            Err(Refuse::DeviceMismatch)
        );
        assert_eq!(
            w.accept_host_record(i, &self_appointed),
            Err(Refuse::PermitInvalid)
        );
        assert_eq!(w.snap(i), before);
        assert!(w.d[i].view.hosts.contains_key(&w.hk(H1)));
        assert!(!w.d[i].view.hosts.contains_key(&w.hk(HP)));
    }
    assert!(w.d[HP].host.is_none());
    w.post(HP, "hana from phone");
    w.round();
    w.assert_converged(&everyone());
    assert!(w.d[CAROL].board.texts().contains("hana from phone"));
}

// KIND: limitation
// Shared mnemonic: the device binding is a random ID, not a key. Hana's phone (same phrase)
// can sign a higher-seq host note claiming the DESKTOP's device ID with its own endpoint and
// present H1's genuine permit; members accept it and now dial the phone as "the host".
// Unlinking the phone does not revoke this (no strong revocation under a shared phrase).
#[test]
fn phrase_sharing_sibling_can_claim_the_host_device_id() {
    let mut w = world();
    let h1 = w.d[H1].host.as_ref().unwrap().record.clone();
    let hijack = HostRecord {
        note: EpNote::sign(
            &w.d[HP].id,
            Audience::Group(w.gid.clone()),
            w.d[H1].slot,
            &w.gep(HP),
            h1.note.claims.seq + 1,
        ),
        permit: h1.permit,
    };
    assert_eq!(w.accept_host_record(CAROL, &hijack.wire()), Ok(()));
    let e = w.d[CAROL].view.hosts[&w.hk(H1)]
        .note
        .claims
        .endpoint
        .clone();
    assert_eq!(e, w.gep(HP));
    assert_eq!(w.dial(CAROL, &e), Ok(HP));
}

// KIND: invariant
// Q5 host-to-host sync: alice-1 only reaches H1, carol only reaches H2. Alice's post
// uploaded to H1 is not at carol until H1 and H2 sync; afterwards carol gets it, and every
// post frame carol ever received came from H2. Everyone converges.
#[test]
fn post_via_one_host_reaches_member_served_only_by_another_host() {
    let mut w = world();
    w.cut(A1, H2);
    w.cut(CAROL, H1);
    w.post(A1, "alice via H1");
    assert_eq!(w.connect(A1, H1), Ok(Path::MemberDialedHost));
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Lost));
    w.connect(CAROL, H2).unwrap();
    assert!(!w.d[CAROL].board.texts().contains("alice via H1"));
    w.host_sync(H1, H2).unwrap();
    w.connect(CAROL, H2).unwrap();
    assert!(w.d[CAROL].board.texts().contains("alice via H1"));
    let carol_posts: Vec<&Frame> = w
        .frames
        .iter()
        .filter(|f| f.to == CAROL && f.kind == "post")
        .collect();
    assert!(!carol_posts.is_empty() && carol_posts.iter().all(|f| f.from == H2));
    w.round();
    w.assert_converged(&everyone());
}

// KIND: invariant
// Two hosts partitioned, each with its own members writing; each side converges internally
// but differs from the other. After the host link heals, everyone converges to all posts
// and no device loses a local write (no rollback).
#[test]
fn two_hosts_partitioned_then_healed_converge_without_rollback() {
    let mut w = world();
    let left = [OLGA, A1, BOB];
    let right = [A2, CAROL, HP];
    w.cut(H1, H2);
    for m in left {
        w.cut(m, H2);
    }
    for m in right {
        w.cut(m, H1);
    }
    let writes = [
        (A1, "a1 left"),
        (BOB, "bob left"),
        (A2, "a2 right"),
        (CAROL, "carol right"),
    ];
    for (m, t) in writes {
        w.post(m, t);
    }
    w.round();
    assert_eq!(w.host_sync(H1, H2), Err(RouteErr::Lost));
    w.assert_converged(&[H1, OLGA, A1, BOB]);
    w.assert_converged(&[H2, A2, CAROL, HP]);
    assert_eq!(w.d[H1].board.texts(), set(&["a1 left", "bob left"]));
    assert_eq!(w.d[H2].board.texts(), set(&["a2 right", "carol right"]));
    w.heal(H1, H2);
    w.round();
    w.assert_converged(&everyone());
    let all = set(&["a1 left", "bob left", "a2 right", "carol right"]);
    assert_eq!(w.d[HP].board.texts(), all);
    for (m, t) in writes {
        assert!(w.d[m].board.texts().contains(t));
    }
}

// KIND: invariant
// A member's rotation note goes to EVERY host (one frame each, no member sees it). When one
// host is unreachable, the other host repairs it during host sync; the repaired host then
// reaches the member at the new endpoint. A note altered in relay (endpoint swapped) or a
// stale note is refused by the receiving host with its state byte-identical. Old endpoints
// stop listening.
#[test]
fn member_rotation_note_reaches_every_host_and_hosts_repair_each_other() {
    let mut w = world();
    let old = w.gep(CAROL);
    let first = w.d[CAROL].my_note.clone();
    let reached = w.rotate_member(CAROL);
    assert_eq!(reached, vec![H1, H2]);
    let notes: Vec<&Frame> = w
        .frames
        .iter()
        .filter(|f| f.from == CAROL && f.kind == "ep-note")
        .collect();
    assert_eq!(notes.iter().map(|f| f.to).collect::<Vec<_>>(), vec![H1, H2]);
    assert!(w.live.get(&old).is_none());
    // Second rotation with H2 unreachable: only H1 learns it.
    w.cut(CAROL, H2);
    let reached = w.rotate_member(CAROL);
    assert_eq!(reached, vec![H1]);
    let new = w.gep(CAROL);
    assert_eq!(w.connect_from_host(H2, CAROL), Err(RouteErr::EndpointGone));
    // Tampered relay and stale note are refused at H2 with state unchanged.
    let mut tampered = w.d[CAROL].my_note.clone();
    tampered.claims.endpoint = w.gep(BOB);
    let before = w.snap(H2);
    assert_eq!(
        w.host_accept_note(H2, &tampered.wire()),
        Err(Refuse::BadSignature)
    );
    assert_eq!(w.host_accept_note(H2, &first.wire()), Err(Refuse::Stale));
    assert_eq!(w.snap(H2), before);
    w.host_sync(H1, H2).unwrap();
    let k = w.hk(CAROL);
    assert_eq!(
        w.d[H2].host.as_ref().unwrap().directory[&k].claims.endpoint,
        new
    );
    w.heal(CAROL, H2);
    assert_eq!(w.connect_from_host(H2, CAROL), Ok(Path::HostDialedMember));
    for m in MEMBERS {
        if m != CAROL {
            assert!(!has(&w.received(m), new.as_bytes()));
        }
    }
}

// KIND: invariant
// Q6: after H1 rotates, its old endpoint stops listening at once (no overlap window). H1
// itself dials every online member at that member's CURRENT endpoint (including one that
// rotated earlier) and hands over its new record. A member offline at the time (and cut
// from H2) is recovered later by H1 dialing it, not by the member finding H1. Members refuse
// a forged H1 record and the stale one with state unchanged.
#[test]
fn rotated_host_reconnects_members_at_their_current_endpoints_without_overlap() {
    let mut w = world();
    w.rotate_member(CAROL);
    w.d[BOB].online = false;
    w.cut(BOB, H2);
    let old_h1 = w.gep(H1);
    let stale = w.d[H1].host.as_ref().unwrap().record.clone();
    let n_frames = w.frames.len();
    let reached = w.rotate_host(H1);
    assert!(w.live.get(&old_h1).is_none(), "no overlap window");
    assert_eq!(
        reached.iter().copied().collect::<BTreeSet<_>>(),
        [OLGA, A1, A2, CAROL, HP].into()
    );
    for f in w.frames[n_frames..]
        .iter()
        .filter(|f| f.from == H1 && f.kind == "host-record" && MEMBERS.contains(&f.to))
    {
        assert_eq!(f.to_ep, w.gep(f.to), "host dials member's current endpoint");
    }
    let new_h1 = w.gep(H1);
    for m in [OLGA, A1, A2, CAROL, HP] {
        assert_eq!(w.d[m].view.hosts[&w.hk(H1)].note.claims.endpoint, new_h1);
    }
    // Other host learned it too.
    assert_eq!(w.d[H2].view.hosts[&w.hk(H1)].note.claims.endpoint, new_h1);
    // Bob returns: his only H1 address is dead, H2 is cut.
    w.d[BOB].online = true;
    let e = w.d[BOB].view.hosts[&w.hk(H1)].note.claims.endpoint.clone();
    assert_eq!(w.dial(BOB, &e), Err(RouteErr::EndpointGone));
    assert_eq!(w.connect_from_host(H1, BOB), Ok(Path::HostDialedMember));
    assert_eq!(w.d[BOB].view.hosts[&w.hk(H1)].note.claims.endpoint, new_h1);
    // Forged and stale records.
    let forged = HostRecord {
        note: EpNote::sign(
            &ident(37),
            Audience::Group(w.gid.clone()),
            w.d[H1].slot,
            &w.gep(BOB),
            99,
        ),
        permit: stale.permit.clone(),
    };
    for m in MEMBERS {
        let before = w.snap(m);
        assert_eq!(
            w.accept_host_record(m, &forged.wire()),
            Err(Refuse::PermitInvalid)
        );
        assert_eq!(w.accept_host_record(m, &stale.wire()), Err(Refuse::Stale));
        assert_eq!(w.snap(m), before);
    }
}

// KIND: invariant
// Host H1 and carol both rotated while apart: carol's H1 address is dead and H1's address
// for carol is dead (Lost both ways). With a second host that did not rotate, carol reaches
// H2, uploads her new note, and learns H1's new record from H2; after host sync H1 can dial
// carol itself, and carol can dial H1. No re-share needed while >= 1 host is unchanged.
#[test]
fn host_and_member_both_rotated_while_apart_recover_via_another_host() {
    let mut w = world();
    w.d[CAROL].online = false;
    w.rotate_host(H1);
    w.rotate_member(CAROL);
    w.d[CAROL].online = true;
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Lost));
    assert_eq!(w.connect(CAROL, H2), Ok(Path::MemberDialedHost));
    assert_eq!(
        w.d[CAROL].view.hosts[&w.hk(H1)].note.claims.endpoint,
        w.gep(H1)
    );
    w.host_sync(H1, H2).unwrap();
    assert_eq!(w.connect_from_host(H1, CAROL), Ok(Path::HostDialedMember));
    assert_eq!(w.connect(CAROL, H1), Ok(Path::MemberDialedHost));
    w.post(CAROL, "back");
    w.round();
    w.assert_converged(&everyone());
}

// KIND: candidate-policy G-TIME
// Single host, host and carol both rotated while apart.
// - Retention::None: Lost in both directions; only a manual re-share recovers.
// - Retention::UntilHostAck (coordinator proposal, section 9 Q6): carol keeps her OLD group
//   endpoint listening until the host acknowledges her new note. The host dials the old
//   endpoint, hands over its new record and receives the new note; the ack releases the old
//   endpoint. Cost: the old endpoint stays reachable (to anyone who knows it) until then.
#[test]
fn single_host_both_rotated_retain_old_endpoint_until_ack_vs_no_retention() {
    for retention in [Retention::None, Retention::UntilHostAck] {
        let mut w = World::new(Cfg {
            hosts: vec![H1],
            retention,
            ..Cfg::default()
        });
        w.d[CAROL].online = false;
        let old = w.gep(CAROL);
        w.rotate_member(CAROL);
        w.rotate_host(H1);
        w.d[CAROL].online = true;
        match retention {
            Retention::None => {
                assert!(w.live.get(&old).is_none());
                assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Lost));
                assert_eq!(w.connect_from_host(H1, CAROL), Err(RouteErr::EndpointGone));
            }
            Retention::UntilHostAck => {
                assert_eq!(w.live.get(&old), Some(&CAROL), "old endpoint retained");
                assert_eq!(w.connect(CAROL, H1), Ok(Path::HostDialedMember));
                assert!(w.live.get(&old).is_none(), "released after host ack");
                assert_eq!(w.connect(CAROL, H1), Ok(Path::MemberDialedHost));
                let k = w.hk(CAROL);
                assert_eq!(
                    w.d[H1].host.as_ref().unwrap().directory[&k].claims.endpoint,
                    w.gep(CAROL)
                );
            }
        }
    }
}

// Shared setup for the two writer-collision tests: hosts partitioned, alice-1 only reaches
// `ha`, carol only reaches `hc`; carol is forced onto alice-1's writer ID (a 2^-64 draw or a
// broken RNG; still not key-derived). HP only reaches H2, BOB reaches both.
fn collision_world(claim: ClaimRule, ha: usize, hc: usize) -> (World, u64, Post, Post) {
    let mut w = World::new(Cfg {
        claim,
        ..Cfg::default()
    });
    let shared = w.d[A1].w.writer;
    let carol_id = w.d[CAROL].id.clone();
    w.d[CAROL].w = Writer::new(carol_id, shared);
    w.cut(H1, H2);
    w.cut(A1, hc);
    w.cut(CAROL, ha);
    w.cut(HP, H1);
    let pa = w.post(A1, "alice");
    let pc = w.post(CAROL, "carol");
    w.connect(A1, ha).unwrap();
    w.connect(CAROL, hc).unwrap();
    (w, shared, pa, pc)
}

// KIND: limitation
// First-claim-at-host with two hosts: each host binds the shared writer ID to the DID it saw
// first. After the host link heals, each host refuses the other's post, so the hosts and the
// members they serve diverge permanently (bob saw H1 first; hana's phone only sees H2).
#[test]
fn writer_collision_first_claim_at_each_host_diverges_across_hosts() {
    let (mut w, shared, _, _) = collision_world(ClaimRule::FirstAtHost, H1, H2);
    w.heal(H1, H2);
    w.round();
    w.round();
    let (alice, carol) = (w.did(A1), w.did(CAROL));
    assert_eq!(w.d[H1].board.writers[&shared], alice);
    assert_eq!(w.d[H2].board.writers[&shared], carol);
    assert_eq!(w.d[H1].board.texts(), set(&["alice"]));
    assert_eq!(w.d[H2].board.texts(), set(&["carol"]));
    assert_eq!(w.d[BOB].board.texts(), set(&["alice"]));
    assert_eq!(w.d[HP].board.texts(), set(&["carol"]));
    assert_ne!(
        w.d[BOB].board.doc.get_deep_value(),
        w.d[HP].board.doc.get_deep_value()
    );
}

// KIND: candidate-policy G-FORK
// Deterministic cross-host tie-break: hosts hold a new writer's posts until every host has
// seen every claim for it; then the lowest claim id (blake3 of the signed seq-1 post) wins at
// every host. During the partition nothing under the contested writer is delivered (cost:
// every NEW writer stalls while any host is unreachable). After healing, the same DID wins
// in BOTH placements (order-independent), the loser is told, redraws its writer ID and
// re-posts, and everyone converges with both texts.
// Not a proof: a claim that reaches one host after the other already settled is not
// covered (needs an agreement round; not designed).
#[test]
fn writer_collision_lowest_claim_after_all_hosts_saw_converges() {
    let mut winners = vec![];
    for (ha, hc) in [(H1, H2), (H2, H1)] {
        let (mut w, shared, pa, pc) = collision_world(ClaimRule::LowestAfterAllHostsSaw, ha, hc);
        w.round();
        for i in everyone() {
            assert!(
                w.d[i].board.accepted.is_empty(),
                "stalled while partitioned"
            );
        }
        w.heal(H1, H2);
        w.round();
        let win = if pa.id() < pc.id() { A1 } else { CAROL };
        let lose = if win == A1 { CAROL } else { A1 };
        for h in [H1, H2] {
            assert_eq!(w.d[h].board.writers[&shared], w.did(win));
        }
        w.round();
        assert_eq!(w.d[lose].refused.len(), 1);
        assert_eq!(w.d[lose].refused[0].1, Why::WriterOwnedByOtherDid);
        let new = w.redraw_writer(lose);
        assert_ne!(new, shared);
        w.heal(A1, H1);
        w.heal(A1, H2);
        w.heal(CAROL, H1);
        w.heal(CAROL, H2);
        w.round();
        w.round();
        w.assert_converged(&everyone());
        assert_eq!(w.d[HP].board.texts(), set(&["alice", "carol"]));
        winners.push(w.did(win));
    }
    assert_eq!(winners[0], winners[1], "winner independent of placement");
}

// KIND: invariant
// Q5 "admins can remove a host": bob (admin, appointed after H2) signs RemoveHost for H2's
// permit. Once it propagates, every member refuses H2: links in both directions, a post H2
// pushes, and a fresh H2 record (state unchanged). H1 refuses host sync with H2. Members'
// next rotation notes go only to H1. H2 still holds the endpoints it learned before removal
// until those members rotate (noted, not prevented).
#[test]
fn removed_host_is_refused_by_members_and_hosts_afterwards() {
    let mut w = world();
    w.post(BOB, "before");
    w.round();
    let permit = w.d[H2].host.as_ref().unwrap().record.permit.digest();
    let op = w.op(BOB, Op::RemoveHost { permit });
    w.submit(BOB, op);
    w.cut(BOB, H2); // bob must use H1 to publish the removal
    w.round();
    w.heal(BOB, H2);
    let h2_rec = w.d[H2].host.as_ref().unwrap().record.wire();
    for m in MEMBERS {
        assert!(!w.d[m].view.hosts.contains_key(&w.hk(H2)));
        let before = w.snap(m);
        assert_eq!(w.connect(m, H2), Err(RouteErr::Refused));
        assert_eq!(w.connect_from_host(H2, m), Err(RouteErr::Refused));
        assert_eq!(w.accept_host_record(m, &h2_rec), Err(Refuse::Revoked));
        assert_eq!(w.snap(m), before);
    }
    assert_eq!(w.host_sync(H1, H2), Err(RouteErr::Refused));
    // Stale knowledge at the removed host until carol rotates.
    let carol_old = w.gep(CAROL);
    let k = w.hk(CAROL);
    assert_eq!(
        w.d[H2].host.as_ref().unwrap().directory[&k].claims.endpoint,
        carol_old
    );
    let n = w.frames.len();
    assert_eq!(w.rotate_member(CAROL), vec![H1]);
    assert!(
        w.frames[n..]
            .iter()
            .filter(|f| f.kind == "ep-note")
            .all(|f| f.to == H1)
    );
    assert!(!has(&w.received(H2), w.gep(CAROL).as_bytes()));
    w.post(A1, "after");
    w.round();
    w.assert_converged(&[H1, OLGA, A1, A2, BOB, CAROL, HP]);
    assert!(!w.d[H2].board.texts().contains("after"));
}

// KIND: candidate-policy G-AUTHORITY
// Which admins may remove a host ("newer admins can remove" — unconfirmed). Appointment
// order uses grant nonces as a fixture stand-in for an op log: alice (admin #1) was appointed
// before H1's permit (#10); bob (#20) after.
// - AnyAdmin: alice's removal of H1 is accepted.
// - AdminAppointedAfterHost: alice's removal is refused (state unchanged); bob's is accepted.
#[test]
fn host_removal_any_admin_vs_only_admins_appointed_after_host() {
    for rule in [RemovalRule::AnyAdmin, RemovalRule::AdminAppointedAfterHost] {
        let mut w = World::new(Cfg {
            removal: rule,
            ..Cfg::default()
        });
        let permit = w.d[H1].host.as_ref().unwrap().record.permit.digest();
        let by_alice = w.op(
            A1,
            Op::RemoveHost {
                permit: permit.clone(),
            },
        );
        let by_bob = w.op(
            BOB,
            Op::RemoveHost {
                permit: permit.clone(),
            },
        );
        let before = w.snap(CAROL);
        match rule {
            RemovalRule::AnyAdmin => {
                assert_eq!(w.accept_op(CAROL, &by_alice.wire()), Ok(true));
                assert!(w.d[CAROL].view.revoked.contains(&permit));
            }
            RemovalRule::AdminAppointedAfterHost => {
                assert_eq!(
                    w.accept_op(CAROL, &by_alice.wire()),
                    Err(Refuse::NotAppointedAfter)
                );
                assert_eq!(w.snap(CAROL), before);
                assert_eq!(w.accept_op(CAROL, &by_bob.wire()), Ok(true));
                assert!(w.d[CAROL].view.revoked.contains(&permit));
            }
        }
    }
}

// KIND: limitation
// U9 with two hosts, one rogue (ignores removal). Three distinct admins flag carol; every
// device verifies the flags. The honest host drops carol's directory entry, refuses her link
// and her posts; every honest member removes her. The rogue host keeps her endpoint and keeps
// feeding her new group posts: with no group-content encryption, removal cannot stop a rogue
// host from serving reads. Carol's later posts relayed by the rogue host are rejected by
// every honest replica with state unchanged.
#[test]
fn member_removal_applied_by_honest_hosts_but_rogue_host_still_serves_removed_member() {
    let mut w = world();
    w.rogue.insert(H2);
    let carol = w.did(CAROL);
    for admin in [OLGA, A1, BOB] {
        let op = w.op(
            admin,
            Op::Flag {
                target: carol.clone(),
            },
        );
        w.submit(admin, op);
    }
    w.round();
    for i in [OLGA, A1, A2, BOB, HP, H1] {
        assert!(w.d[i].view.removed.contains(&carol), "device {i}");
        assert!(!w.d[i].board.roster.contains(&carol), "device {i}");
    }
    let h1 = w.d[H1].host.as_ref().unwrap();
    assert!(h1.directory.values().all(|n| n.claims.owner != carol));
    assert!(w.d[H2].board.roster.contains(&carol));
    assert!(
        w.d[H2]
            .host
            .as_ref()
            .unwrap()
            .directory
            .contains_key(&w.hk(CAROL))
    );
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    // Rogue keeps serving reads.
    w.post(BOB, "after removal");
    w.round();
    assert!(w.d[CAROL].board.texts().contains("after removal"));
    // Carol's later post via the rogue host: refused by every honest replica, unchanged.
    let late = w.post(CAROL, "carol after removal");
    for i in [OLGA, A1, A2, BOB, HP, H1] {
        let before = w.d[i].board.snapshot();
        assert_eq!(
            w.d[i].board.receive(late.clone()),
            Verdict::Rejected(Why::NotMember)
        );
        assert_eq!(w.d[i].board.snapshot(), before);
    }
    w.round();
    assert!(w.d[H2].board.texts().contains("carol after removal"));
    for i in [OLGA, A1, A2, BOB, HP, H1] {
        assert!(!w.d[i].board.texts().contains("carol after removal"));
    }
}

// KIND: invariant
// U5 revised (visible) + section 8b Q1 (one signed notice, same reason on every later
// attempt):
// - Removal: forged flags (non-admin flagger, tampered target) change nothing at carol. After
//   three distinct admins flag her, an honest host answers her next connection with the
//   signed flags; carol verifies them and records that she is removed. A second attempt
//   returns the same notice and changes nothing.
// - Personal block: bob blocks carol; carol receives a DID-signed notice on the private path.
//   Forged or misaddressed notices are refused unchanged. Her later direct attempt gets the
//   byte-identical notice. The block changes no group roster, and bob and carol still see
//   each other's group posts (personal block != removal).
#[test]
fn removed_member_and_blocked_contact_receive_verifiable_signed_notices() {
    let mut w = world();
    let carol = w.did(CAROL);
    // --- personal block first (no group change) ---
    let rosters: Vec<_> = everyone()
        .iter()
        .map(|&i| w.d[i].board.roster.clone())
        .collect();
    let mallory = ident(37);
    let forged = {
        let body = BlockBody {
            from: w.did(BOB),
            to: carol.clone(),
            at: 0,
        };
        let sig = mallory.sign(&signed(NOTICE_DOMAIN, &body)).to_vec();
        serde_json::to_vec(&BlockNotice { body, sig }).unwrap()
    };
    let misaddressed = {
        let body = BlockBody {
            from: w.did(BOB),
            to: w.did(A1),
            at: 0,
        };
        let sig = w.d[BOB].id.sign(&signed(NOTICE_DOMAIN, &body)).to_vec();
        serde_json::to_vec(&BlockNotice { body, sig }).unwrap()
    };
    let before = w.snap(CAROL);
    assert_eq!(
        w.receive_block_notice(CAROL, &forged),
        Err(Refuse::BadSignature)
    );
    assert_eq!(
        w.receive_block_notice(CAROL, &misaddressed),
        Err(Refuse::NotAddressed)
    );
    assert_eq!(w.snap(CAROL), before);
    let bob = w.did(BOB);
    let notice = w.block(BOB, &carol);
    assert!(w.d[CAROL].view.blocked_by.contains_key(&bob));
    assert_eq!(w.direct(CAROL, &bob), Direct::Blocked(notice.clone()));
    assert_eq!(w.direct(CAROL, &bob), Direct::Blocked(notice));
    for (k, &i) in everyone().iter().enumerate() {
        assert_eq!(w.d[i].board.roster, rosters[k]);
    }
    w.post(BOB, "bob to group");
    w.post(CAROL, "carol to group");
    w.round();
    assert!(w.d[CAROL].board.texts().contains("bob to group"));
    assert!(w.d[BOB].board.texts().contains("carol to group"));

    // --- removal notice ---
    let gid = w.gid.clone();
    let hana_flag = SignedOp::sign(
        &w.d[HP].id,
        &gid,
        w.grants.get(&w.did(HP)).cloned(),
        80,
        Op::Flag {
            target: carol.clone(),
        },
    );
    let mut tampered = w.op(OLGA, Op::Flag { target: w.did(A2) });
    tampered.body.op = Op::Flag {
        target: carol.clone(),
    };
    let before = w.snap(CAROL);
    assert_eq!(w.accept_op(CAROL, &hana_flag.wire()), Err(Refuse::NotAdmin));
    assert_eq!(
        w.accept_op(CAROL, &tampered.wire()),
        Err(Refuse::BadSignature)
    );
    assert_eq!(w.snap(CAROL), before);
    w.cut(CAROL, H1);
    w.cut(CAROL, H2);
    for admin in [OLGA, A1, BOB] {
        let op = w.op(
            admin,
            Op::Flag {
                target: carol.clone(),
            },
        );
        w.submit(admin, op);
    }
    w.round();
    assert!(!w.d[CAROL].view.removed.contains(&carol), "not yet told");
    w.heal(CAROL, H1);
    let n = w.frames.len();
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    let notices: Vec<&Frame> = w.frames[n..]
        .iter()
        .filter(|f| f.to == CAROL && f.kind == "removal-notice")
        .collect();
    assert_eq!(notices.len(), 3);
    assert!(w.d[CAROL].view.removed.contains(&carol));
    assert_eq!(w.d[CAROL].view.flags[&carol].len(), 3);
    let after = w.snap(CAROL);
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    assert_eq!(w.snap(CAROL), after, "same notice again, no change");
}

// KIND: candidate-policy G-KEYS
// Host binary identity (open: phrase-holding linked device vs own DID + permit).
// - Own DID (hugo, H2): relays, but cannot produce any content a member accepts: posts it
//   signs are NotMember; it holds no member's key.
// - Phrase-holding linked device (hana's desktop H1): its key IS hana's DID key, so the
//   host process can post as hana, sign hana's endpoint notes and sign a block notice as
//   hana — all accepted. A server holding a phrase holds that user's whole identity.
#[test]
fn host_binary_own_did_vs_phrase_holding_linked_device() {
    let mut w = world();
    let gid = w.gid.clone();
    // Own-DID host.
    let mut hw = Writer::new(w.d[H2].id.clone(), 0x4242);
    let p = hw.post(&gid, "hugo as host");
    for m in MEMBERS {
        let before = w.d[m].board.snapshot();
        assert_eq!(
            w.d[m].board.receive(p.clone()),
            Verdict::Rejected(Why::NotMember)
        );
        assert_eq!(w.d[m].board.snapshot(), before);
    }
    assert!(MEMBERS.iter().all(|&m| w.did(m) != w.did(H2)));
    // Phrase-holding host device.
    assert_eq!(w.did(H1), w.did(HP));
    let mut pw = Writer::new(w.d[H1].id.clone(), 0x4343);
    let p = pw.post(&gid, "written by the host process as hana");
    assert_eq!(w.d[CAROL].board.receive(p), Verdict::Accepted);
    let body = BlockBody {
        from: w.did(H1),
        to: w.did(CAROL),
        at: 0,
    };
    let sig = w.d[H1].id.sign(&signed(NOTICE_DOMAIN, &body)).to_vec();
    let b = serde_json::to_vec(&BlockNotice { body, sig }).unwrap();
    assert_eq!(w.receive_block_notice(CAROL, &b), Ok(()));
    let note = EpNote::sign(
        &w.d[H1].id,
        Audience::Private,
        w.d[HP].slot,
        "prv-attacker",
        9,
    );
    assert!(note.verify() && note.claims.owner == w.did(HP));
}

// KIND: candidate-policy G-AUTHORITY
// Max number of hosts (user passed). With Max(2), a third valid permit is refused by members
// (CapReached, state unchanged); Unbounded accepts it. Exposure grows with the host count:
// after carol rotates, exactly the hosts hold her new group endpoint; no member does.
#[test]
fn host_count_cap_candidates_and_endpoint_exposure_scales_with_hosts() {
    for cap in [HostCap::Max(2), HostCap::Unbounded] {
        let mut w = World::new(Cfg {
            cap,
            ..Cfg::default()
        });
        let permit = w.default_permit(H3);
        w.make_host(H3, permit);
        let rec = w.d[H3].host.as_ref().unwrap().record.wire();
        match cap {
            HostCap::Max(_) => {
                for m in MEMBERS {
                    let before = w.snap(m);
                    assert_eq!(w.accept_host_record(m, &rec), Err(Refuse::CapReached));
                    assert_eq!(w.snap(m), before);
                }
            }
            HostCap::Unbounded => {
                w.publish_record(H3).unwrap();
                w.round();
                w.rotate_member(CAROL);
                let e = w.gep(CAROL);
                let holders: BTreeSet<usize> = (0..w.d.len())
                    .filter(|&i| i != CAROL && has(&w.received(i), e.as_bytes()))
                    .collect();
                assert_eq!(holders, [H1, H2, H3].into());
            }
        }
    }
}
