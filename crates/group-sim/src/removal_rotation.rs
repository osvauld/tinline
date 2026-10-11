//! W2-U: announced, delayed host rotation after a member removal (U11, U12).
//!
//! Status: proposal — not approved. Simulation only; no production API, wire format or UI.
//! Decisions modelled (`docs/design/rotation-open-decisions.md` section 0):
//! - U4 revised: content is signed by the DID key; Loro peer IDs are random writer IDs.
//! - U5 revised: the removed member is told (signed flags returned on her dial).
//! - U7: only hosts hold members' group endpoints; all group traffic goes via hosts.
//! - U9 / U10: 3 flags from distinct admins remove a member; every device verifies every
//!   message, op, host record, announcement and call request itself.
//! - U11: signed frontier (D01/D02 option A): a post is kept if its signed op frontier
//!   predates the removal. Hosts accept a removed member's upload of such posts.
//! - U12 + detail: after a removal every host signs a "rotating soon" announcement, sends it
//!   to REMAINING members only, and rotates its group endpoint `ROTATION_DELAY` (2 days)
//!   later, reconnecting members itself. Refusal of the removed DID's NEW posts and calls is
//!   immediate.
//!
//! NOT decided, modelled as `Cfg` parameters (defaults are fixture values, not selections):
//! - `UploadCap` (G-HISTORY): count cap on pre-removal uploads vs the rotation as the only
//!   cut-off.
//! - `Batching` (G-TIME): several removals inside one delay window.
//! - `MemberRotation` (G-DISCOVERY): do members also rotate their own group endpoints?
//!
//! Provenance: helpers, `SimRng`, `MGrant`/`Cap`, `Post`/`Writer`/`Board`, `EpNote`,
//! `HostRecord`, `SignedOp`, closure/frontier checks and the `World` link/host-sync/rotation
//! machinery are COPIED from `flags.rs` @82d221f (itself from `multi_host.rs` @0924939) and
//! trimmed: only `Op::Flag` (distinct-admin threshold, owner and admins not flaggable), only
//! the signed-frontier post rule, no host revocation, no withdrawal/expiry/rejoin. Added: the
//! removal-rotation schedule (`Sched`), the signed `Announce`, DID-signed `CallReq` relayed
//! by a host, the removed member's pre-removal upload on her dial, and the cap candidate.
//!
//! Fake `u64` time (1 tick = 1 simulated second), deterministic seeds, in-memory only.
use super::*;
use identity::{public_key_from_did, verify};

const POST_DOMAIN: &[u8] = b"tinline/group-sim/removal-rotation-post/not-production/v1\0";
const NOTE_DOMAIN: &[u8] = b"tinline/group-sim/removal-rotation-endpoint-note/not-production/v1\0";
const GRANT_DOMAIN: &[u8] = b"tinline/group-sim/removal-rotation-grant/not-production/v1\0";
const OP_DOMAIN: &[u8] = b"tinline/group-sim/removal-rotation-op/not-production/v1\0";
const ANNOUNCE_DOMAIN: &[u8] = b"tinline/group-sim/removal-rotation-announce/not-production/v1\0";
const CALL_DOMAIN: &[u8] = b"tinline/group-sim/removal-rotation-call/not-production/v1\0";
const DOC: &str = "group/w2u/2026-10-10";
const MAX_PENDING: usize = 16;
const MAX_CHAIN: usize = 8;
const FLAG_THRESHOLD: usize = 3; // U9 / U10 (decided)
const TICKS_PER_DAY: u64 = 86_400; // 1 tick = 1 simulated second
const TICKS_PER_HOUR: u64 = 3_600;
const ROTATION_DELAY: u64 = 2 * TICKS_PER_DAY; // U12 detail (decided): 2 days
const T0: u64 = 1_000; // fixture start time

// Devices. olga = owner/creator; alice (2 devices), bob, dave = admins; carol, erin = members;
// hugo (H1), ivan (H2) = host binaries with their own DIDs and an owner-issued {Host} permit;
// judy (H3) = a DID with no accepted host record (forgery fixture).
const OLGA: usize = 0;
const A1: usize = 1;
const A2: usize = 2;
const BOB: usize = 3;
const DAVE: usize = 4;
const CAROL: usize = 5;
const ERIN: usize = 6;
const H1: usize = 7;
const H2: usize = 8;
const H3: usize = 9;
const MEMBERS: [usize; 7] = [OLGA, A1, A2, BOB, DAVE, CAROL, ERIN];

// ---- helpers (copied from flags.rs @82d221f) -----------------------------------------------

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

// Copied from flags.rs @82d221f. Deterministic SplitMix64: the ONLY source of writer IDs,
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

// ---- grants and host permits (copied from flags.rs @82d221f) -------------------------------

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
    device: Option<u64>,
    caps: BTreeSet<Cap>,
    delegable: bool,
    nonce: u64,
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

// ---- DID-signed content (copied from flags.rs @82d221f) ------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PostBody {
    group: String,
    doc: String,
    author: String,
    writer: u64,
    seq: u64,
    ops_seen: BTreeSet<String>, // author's signed op frontier (U11 "signed frontier")
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
    fn post(&mut self, group: &str, text: &str, ops_seen: BTreeSet<String>) -> Post {
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
            ops_seen,
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
    UploadCap, // new: count-cap candidate on a removed author's uploads
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
// Copied from flags.rs @82d221f `Board` (no roster; real Loro import + content check).
struct Board {
    group: String,
    doc: LoroDoc,
    accepted: BTreeMap<String, Post>,
    pending: BTreeMap<String, Post>,
    writers: BTreeMap<u64, String>,
    last_seq: BTreeMap<u64, u64>,
    order: Vec<String>,
}
impl Board {
    fn new(group: String) -> Self {
        Self {
            group,
            doc: LoroDoc::new(),
            accepted: BTreeMap::new(),
            pending: BTreeMap::new(),
            writers: BTreeMap::new(),
            last_seq: BTreeMap::new(),
            order: vec![],
        }
    }
    fn precheck(&self, p: &Post) -> Result<(), Why> {
        let b = &p.body;
        if b.group != self.group || b.doc != DOC {
            return Err(Why::WrongGroup);
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
        if self.pending.len() >= MAX_PENDING {
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

// ---- DID-signed endpoint notes and host records (copied from flags.rs @82d221f) ------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NoteClaims {
    owner: String,
    group: String,
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
    fn sign(id: &Identity, group: &str, slot: u64, endpoint: &str, seq: u64) -> Self {
        let claims = NoteClaims {
            owner: id.did().into(),
            group: group.into(),
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

// ---- DID-signed group ops (trimmed from flags.rs @82d221f: flags only) ---------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Op {
    Flag { target: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct OpBody {
    group: String,
    issuer: String,
    issuer_grant: Option<MGrant>, // None only for the group creator
    nonce: u64,
    seen: BTreeSet<String>, // issuer's op frontier: the op is authorized at this frontier
    op: Op,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SignedOp {
    body: OpBody,
    sig: Vec<u8>,
}
impl SignedOp {
    fn sign(
        id: &Identity,
        group: &str,
        grant: Option<MGrant>,
        nonce: u64,
        seen: BTreeSet<String>,
        op: Op,
    ) -> Self {
        let body = OpBody {
            group: group.into(),
            issuer: id.did().into(),
            issuer_grant: grant,
            nonce,
            seen,
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

// ---- NEW: host "rotating soon" announcement (U12) -----------------------------------------

/// Signed by the host's DID, carried with its owner-issued {Host} permit. `cites` = the flag
/// ops (and their causal frontier) that establish every DID in `removed`, so each member can
/// check the removal itself (U10). `rotate_at` is the host's own clock: members cannot
/// verify it, they only learn when to expect a new host record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct AnnBody {
    group: String,
    host: String,
    slot: u64,
    seq: u64,
    removed: BTreeSet<String>,
    cites: BTreeSet<String>,
    rotate_at: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Announce {
    body: AnnBody,
    permit: MGrant,
    sig: Vec<u8>,
}
impl Announce {
    fn sign(id: &Identity, body: AnnBody, permit: MGrant) -> Self {
        let sig = id.sign(&signed(ANNOUNCE_DOMAIN, &body)).to_vec();
        Self { body, permit, sig }
    }
    fn verify(&self) -> bool {
        valid(
            &self.body.host,
            &signed(ANNOUNCE_DOMAIN, &self.body),
            &self.sig,
        )
    }
    fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}

// ---- NEW: DID-signed call request relayed by a host (fixture for "no new calls") ----------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CallBody {
    group: String,
    caller: String,
    callee: String,
    nonce: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CallReq {
    body: CallBody,
    sig: Vec<u8>,
}
impl CallReq {
    fn sign(id: &Identity, body: CallBody) -> Self {
        let sig = id.sign(&signed(CALL_DOMAIN, &body)).to_vec();
        Self { body, sig }
    }
    fn verify(&self) -> bool {
        valid(
            &self.body.caller,
            &signed(CALL_DOMAIN, &self.body),
            &self.sig,
        )
    }
    fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refuse {
    Malformed,
    BadSignature,
    WrongGroup,
    PermitInvalid,
    DeviceMismatch,
    Stale,
    NotAdmin,
    NotMember,
    NotHost,
    NotFlaggable,
    IssuerRemoved,
    Removed,
    NotRemoved,
    Unknown,
    Unreachable,
    Limit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Acc {
    Applied,
    Duplicate,
    Pending,
}

// ---- configuration (open policy as parameters; defaults are NOT selections) ----------------

/// U11 open / G-HISTORY: bound on a removed member's pre-removal uploads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UploadCap {
    /// Only the host rotation ends the upload window (coordinator note under U12).
    RotationOnly,
    /// Each host accepts at most this many posts by a removed author after the removal.
    Count(usize),
}
/// U12 open / G-TIME: several removals inside one delay window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Batching {
    /// A later removal joins the pending rotation; its deadline is NOT moved.
    KeepFirstDeadline,
    /// A later removal joins the pending rotation and moves it to (last removal + delay).
    ExtendToLast,
    /// Every removal schedules its own rotation at (its removal + delay).
    PerRemoval,
}
/// U12 open / G-DISCOVERY: does a member removal also rotate members' own group endpoints?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemberRotation {
    HostsOnly,
    /// Each member that accepted an announcement rotates once at its `rotate_at`.
    MembersToo,
}
#[derive(Clone)]
struct Cfg {
    seed: u64,
    hosts: Vec<usize>,
    cap: UploadCap,
    batching: Batching,
    members: MemberRotation,
}
impl Default for Cfg {
    fn default() -> Self {
        Self {
            seed: 0x5EED_02F7,
            hosts: vec![H1, H2],
            cap: UploadCap::RotationOnly,
            batching: Batching::KeepFirstDeadline,
            members: MemberRotation::HostsOnly,
        }
    }
}

// Immutable group context every device shares (genesis + fixture bootstrap).
#[derive(Clone)]
struct Ctx {
    cfg: Cfg,
    g: Genesis,
    gid: String,
    creator: String,
    admins: BTreeSet<String>,
    base: BTreeSet<String>,
    grants: BTreeMap<String, MGrant>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
struct Derived {
    removed: BTreeSet<String>,
    roster: BTreeSet<String>,
    flags: BTreeMap<String, BTreeSet<String>>, // target -> flag op ids
}

fn closure(ops: &BTreeMap<String, SignedOp>, frontier: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack: Vec<String> = frontier.iter().cloned().collect();
    while let Some(id) = stack.pop() {
        if let Some(o) = ops.get(&id) {
            if out.insert(id) {
                stack.extend(o.body.seen.iter().cloned());
            }
        }
    }
    out
}

impl Ctx {
    // Membership as a pure function of an op set (trimmed from flags.rs: distinct admins).
    fn derive(&self, ops: &BTreeMap<String, SignedOp>, ids: &BTreeSet<String>) -> Derived {
        let mut issuers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut flags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for id in ids {
            let Some(o) = ops.get(id) else { continue };
            let Op::Flag { target } = &o.body.op;
            issuers
                .entry(target.clone())
                .or_default()
                .insert(o.body.issuer.clone());
            flags.entry(target.clone()).or_default().insert(id.clone());
        }
        let removed: BTreeSet<String> = issuers
            .into_iter()
            .filter(|(_, s)| s.len() >= FLAG_THRESHOLD)
            .map(|(t, _)| t)
            .collect();
        let roster = self.base.difference(&removed).cloned().collect();
        Derived {
            removed,
            roster,
            flags,
        }
    }
    fn static_check(&self, op: &SignedOp) -> Result<(), Refuse> {
        let issuer = &op.body.issuer;
        let is_admin = *issuer == self.creator
            || op
                .body
                .issuer_grant
                .as_ref()
                .is_some_and(|gr| gr.allows(&self.g, issuer, Cap::Admin));
        if !is_admin {
            return Err(Refuse::NotAdmin);
        }
        let Op::Flag { target } = &op.body.op;
        if self.admins.contains(target) {
            return Err(Refuse::NotFlaggable);
        }
        Ok(())
    }
}

// ---- per-device policy + content state -----------------------------------------------------

struct Replica {
    board: Board,
    hosts: BTreeMap<String, HostRecord>, // "<did>#<slot>" -> accepted host record
    ops: BTreeMap<String, SignedOp>,
    op_pending: BTreeMap<String, SignedOp>,
    der: Derived,
    store: BTreeMap<String, Post>,
    post_pending: BTreeMap<String, Post>,
    dir: Option<BTreeMap<String, EpNote>>, // hosts only (U7): member group notes
    late: BTreeMap<String, BTreeSet<String>>, // hosts: removed author -> posts taken since
    announced: BTreeMap<String, Announce>, // members: host key -> latest verified announcement
    member_due: Option<u64>,               // members, MembersToo only
    calls: Vec<String>,                    // relayed (host) or received (member) call ids
}
impl Replica {
    fn new(ctx: &Ctx, host: bool) -> Self {
        Self {
            board: Board::new(ctx.gid.clone()),
            hosts: BTreeMap::new(),
            ops: BTreeMap::new(),
            op_pending: BTreeMap::new(),
            der: ctx.derive(&BTreeMap::new(), &BTreeSet::new()),
            store: BTreeMap::new(),
            post_pending: BTreeMap::new(),
            dir: host.then(BTreeMap::new),
            late: BTreeMap::new(),
            announced: BTreeMap::new(),
            member_due: None,
            calls: vec![],
        }
    }
    fn snap(&self) -> Vec<u8> {
        serde_json::to_vec(&(
            self.board.snapshot(),
            &self.hosts,
            &self.ops,
            &self.op_pending,
            &self.der,
            &self.store,
            &self.post_pending,
            &self.dir,
            &self.late,
            &self.announced,
            self.member_due,
            &self.calls,
        ))
        .unwrap()
    }
    fn accept_op(&mut self, ctx: &Ctx, bytes: &[u8]) -> Result<Acc, Refuse> {
        let Ok(op) = serde_json::from_slice::<SignedOp>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !op.verify() {
            return Err(Refuse::BadSignature);
        }
        if op.body.group != ctx.gid {
            return Err(Refuse::WrongGroup);
        }
        let id = op.id();
        if self.ops.contains_key(&id) {
            return Ok(Acc::Duplicate);
        }
        if self.op_pending.contains_key(&id) {
            return Ok(Acc::Pending);
        }
        ctx.static_check(&op)?;
        if !op.body.seen.iter().all(|s| self.ops.contains_key(s)) {
            if self.op_pending.len() >= MAX_PENDING {
                return Err(Refuse::Limit);
            }
            self.op_pending.insert(id, op);
            return Ok(Acc::Pending);
        }
        self.frontier_check(ctx, &op)?;
        self.ops.insert(id, op);
        self.drain_ops(ctx);
        self.recompute(ctx);
        Ok(Acc::Applied)
    }
    fn frontier_check(&self, ctx: &Ctx, op: &SignedOp) -> Result<(), Refuse> {
        let d = ctx.derive(&self.ops, &closure(&self.ops, &op.body.seen));
        if d.removed.contains(&op.body.issuer) {
            return Err(Refuse::IssuerRemoved);
        }
        let Op::Flag { target } = &op.body.op;
        if !d.roster.contains(target) {
            return Err(Refuse::NotMember);
        }
        Ok(())
    }
    fn drain_ops(&mut self, ctx: &Ctx) {
        loop {
            let ready = self
                .op_pending
                .iter()
                .find(|(_, o)| o.body.seen.iter().all(|s| self.ops.contains_key(s)))
                .map(|(k, _)| k.clone());
            let Some(id) = ready else { break };
            let op = self.op_pending.remove(&id).unwrap();
            if self.frontier_check(ctx, &op).is_ok() {
                self.ops.insert(id, op);
            }
        }
    }
    fn recompute(&mut self, ctx: &Ctx) {
        let all: BTreeSet<String> = self.ops.keys().cloned().collect();
        self.der = ctx.derive(&self.ops, &all);
        let removed = self.der.removed.clone();
        if let Some(dir) = self.dir.as_mut() {
            dir.retain(|_, n| !removed.contains(&n.claims.owner));
        }
        self.drain_posts(ctx);
    }
    // U11 (decided): signed frontier — the author is a member at the post's own frontier.
    fn post_ok(&self, ctx: &Ctx, p: &Post) -> Result<(), Why> {
        let d = ctx.derive(&self.ops, &closure(&self.ops, &p.body.ops_seen));
        if !d.roster.contains(&p.body.author) {
            return Err(Why::NotMember);
        }
        Ok(())
    }
    fn accept_post(&mut self, ctx: &Ctx, p: Post) -> Verdict {
        if let Err(why) = self.board.precheck(&p) {
            return Verdict::Rejected(why);
        }
        let id = p.id();
        if self.store.contains_key(&id) {
            return Verdict::Duplicate;
        }
        if self.post_pending.contains_key(&id) {
            return Verdict::Pending;
        }
        if !ctx.base.contains(&p.body.author) {
            return Verdict::Rejected(Why::NotMember);
        }
        if !p.body.ops_seen.iter().all(|s| self.ops.contains_key(s)) {
            if self.post_pending.len() >= MAX_PENDING {
                return Verdict::Rejected(Why::Limit);
            }
            self.post_pending.insert(id, p);
            return Verdict::Pending;
        }
        self.admit_post(ctx, id, p)
    }
    fn admit_post(&mut self, ctx: &Ctx, id: String, p: Post) -> Verdict {
        if let Err(why) = self.post_ok(ctx, &p) {
            return Verdict::Rejected(why);
        }
        // Cap candidate: enforced by HOSTS only (members verify the signed frontier alone).
        let author = p.body.author.clone();
        let late = self.dir.is_some() && self.der.removed.contains(&author);
        if let (true, UploadCap::Count(n)) = (late, ctx.cfg.cap) {
            if self.late.get(&author).map_or(0, |s| s.len()) >= n {
                return Verdict::Rejected(Why::UploadCap);
            }
        }
        let v = self.board.receive(p.clone());
        if matches!(v, Verdict::Accepted | Verdict::Pending) {
            self.store.insert(id.clone(), p);
            if late {
                self.late.entry(author).or_default().insert(id);
            }
        }
        v
    }
    fn drain_posts(&mut self, ctx: &Ctx) {
        loop {
            let ready = self
                .post_pending
                .iter()
                .find(|(_, p)| p.body.ops_seen.iter().all(|s| self.ops.contains_key(s)))
                .map(|(k, _)| k.clone());
            let Some(id) = ready else { break };
            let p = self.post_pending.remove(&id).unwrap();
            let _ = self.admit_post(ctx, id, p);
        }
    }
    fn check_record(&self, ctx: &Ctx, r: &HostRecord) -> Result<(), Refuse> {
        if !r.note.verify() {
            return Err(Refuse::BadSignature);
        }
        if r.note.claims.group != ctx.gid {
            return Err(Refuse::WrongGroup);
        }
        if !r.permit.allows(&ctx.g, &r.note.claims.owner, Cap::Host) {
            return Err(Refuse::PermitInvalid);
        }
        if r.permit.body.device != Some(r.note.claims.slot) {
            return Err(Refuse::DeviceMismatch);
        }
        Ok(())
    }
    fn accept_host_record(&mut self, ctx: &Ctx, bytes: &[u8]) -> Result<(), Refuse> {
        let Ok(r) = serde_json::from_slice::<HostRecord>(bytes) else {
            return Err(Refuse::Malformed);
        };
        self.check_record(ctx, &r)?;
        let key = dir_key(&r.note.claims.owner, r.note.claims.slot);
        if let Some(cur) = self.hosts.get(&key) {
            if cur.note.claims.seq >= r.note.claims.seq {
                return Err(Refuse::Stale);
            }
        }
        self.hosts.insert(key, r);
        Ok(())
    }
    fn accept_note(&mut self, ctx: &Ctx, bytes: &[u8]) -> Result<(), Refuse> {
        let Ok(n) = serde_json::from_slice::<EpNote>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !n.verify() {
            return Err(Refuse::BadSignature);
        }
        if n.claims.group != ctx.gid {
            return Err(Refuse::WrongGroup);
        }
        if !self.der.roster.contains(&n.claims.owner) {
            return Err(Refuse::NotMember);
        }
        let key = dir_key(&n.claims.owner, n.claims.slot);
        let Some(dir) = self.dir.as_mut() else {
            return Err(Refuse::NotHost);
        };
        if let Some(cur) = dir.get(&key) {
            if cur.claims.seq >= n.claims.seq {
                return Err(Refuse::Stale);
            }
        }
        dir.insert(key, n);
        Ok(())
    }
    // NEW (U10, U12): every member verifies an announcement itself before keeping it.
    fn accept_announce(&mut self, ctx: &Ctx, bytes: &[u8], me: &str) -> Result<(), Refuse> {
        let Ok(a) = serde_json::from_slice::<Announce>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !a.verify() {
            return Err(Refuse::BadSignature);
        }
        if a.body.group != ctx.gid {
            return Err(Refuse::WrongGroup);
        }
        if !a.permit.allows(&ctx.g, &a.body.host, Cap::Host) {
            return Err(Refuse::PermitInvalid);
        }
        if a.permit.body.device != Some(a.body.slot) {
            return Err(Refuse::DeviceMismatch);
        }
        let key = dir_key(&a.body.host, a.body.slot);
        if self.hosts.get(&key).map(|r| &r.permit) != Some(&a.permit) {
            return Err(Refuse::NotHost);
        }
        if a.body.removed.contains(me) {
            return Err(Refuse::Removed);
        }
        if !a.body.cites.iter().all(|c| self.ops.contains_key(c)) {
            return Err(Refuse::Unknown);
        }
        let d = ctx.derive(&self.ops, &closure(&self.ops, &a.body.cites));
        if a.body.removed.is_empty() || !a.body.removed.is_subset(&d.removed) {
            return Err(Refuse::NotRemoved);
        }
        if let Some(cur) = self.announced.get(&key) {
            if cur.body.seq >= a.body.seq {
                return Err(Refuse::Stale);
            }
        }
        if ctx.cfg.members == MemberRotation::MembersToo {
            self.member_due = Some(self.member_due.unwrap_or(0).max(a.body.rotate_at));
        }
        self.announced.insert(key, a);
        Ok(())
    }
    // NEW: call request check. A host (`me = None`) relays only roster-to-roster calls; the
    // callee (`me = Some`) verifies again (U10). Refusal of a removed caller is immediate.
    fn accept_call(&mut self, ctx: &Ctx, bytes: &[u8], me: Option<&str>) -> Result<(), Refuse> {
        let Ok(c) = serde_json::from_slice::<CallReq>(bytes) else {
            return Err(Refuse::Malformed);
        };
        if !c.verify() {
            return Err(Refuse::BadSignature);
        }
        if c.body.group != ctx.gid {
            return Err(Refuse::WrongGroup);
        }
        if self.der.removed.contains(&c.body.caller) {
            return Err(Refuse::Removed);
        }
        if !self.der.roster.contains(&c.body.caller)
            || !self.der.roster.contains(&c.body.callee)
            || me.is_some_and(|me| me != c.body.callee)
        {
            return Err(Refuse::NotMember);
        }
        let id = digest(&c);
        if self.calls.contains(&id) {
            return Err(Refuse::Stale);
        }
        self.calls.push(id);
        Ok(())
    }
}

// ---- devices and world (copied from flags.rs @82d221f, trimmed; schedule is new) -----------

/// NEW (U12): a host's removal-rotation schedule, driven by the host's own clock.
#[derive(Clone, Debug, Default, Serialize)]
struct Sched {
    learned: BTreeMap<String, u64>, // removed DID -> tick this host first derived the removal
    pending: BTreeMap<u64, BTreeSet<String>>, // rotate_at -> DIDs the rotation cuts off
    done: Vec<(u64, BTreeSet<String>)>, // performed rotations: (tick, DIDs)
    seq: u64,                       // announcement sequence
}
struct Device {
    id: Identity,
    slot: u64, // random device ID / route slot; owner-replaceable, never key-derived
    w: Writer,
    rep: Replica,
    group_key: SigningKey,
    my_note: EpNote,
    host: Option<HostRecord>,
    acks: BTreeMap<String, u64>,
    op_out: Vec<SignedOp>,
    outbox: Vec<Post>,
    refused: Vec<(Post, Why)>,
    online: bool,
    sched: Option<Sched>,  // hosts only
    ann: Option<Announce>, // hosts only: latest announcement
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
#[derive(Clone, Debug)]
struct Frame {
    from: usize,
    to: usize,
    kind: &'static str,
    bytes: Vec<u8>,
}
struct World {
    ctx: Ctx,
    rng: SimRng,
    now: u64,
    d: Vec<Device>,
    hosts: Vec<usize>,
    live: BTreeMap<String, usize>,
    cut: BTreeSet<(usize, usize)>,
    frames: Vec<Frame>,
}
impl World {
    fn new(cfg: Cfg) -> Self {
        let mut rng = SimRng(cfg.seed);
        // olga alice bob dave carol erin hugo ivan judy
        let ids: Vec<Identity> = (80u8..89).map(ident).collect();
        let owners = [0usize, 1, 1, 2, 3, 4, 5, 6, 7, 8];
        let g = Genesis::create(&ids[0], [11; 32]);
        let gid = g.id();
        let admin_ids: BTreeSet<usize> = [1usize, 2, 3].into_iter().collect(); // alice bob dave
        let mut grants = BTreeMap::new();
        for k in 1..=5usize {
            let (caps, delegable): (&[Cap], bool) = if admin_ids.contains(&k) {
                (&[Cap::Read, Cap::Post, Cap::Delegate, Cap::Admin], true)
            } else {
                (&[Cap::Read, Cap::Post], false)
            };
            let gr = MGrant::issue(
                &ids[0],
                &gid,
                ids[k].did(),
                None,
                caps,
                delegable,
                k as u64,
                None,
            );
            grants.insert(ids[k].did().to_string(), gr);
        }
        let creator = ids[0].did().to_string();
        let mut admins: BTreeSet<String> = admin_ids
            .iter()
            .map(|&k| ids[k].did().to_string())
            .collect();
        admins.insert(creator.clone());
        let ctx = Ctx {
            cfg: cfg.clone(),
            g,
            gid: gid.clone(),
            creator,
            admins,
            base: ids[..6].iter().map(|i| i.did().to_string()).collect(),
            grants,
        };
        let mut d = vec![];
        for (dv, &k) in owners.iter().enumerate() {
            let id = ids[k].clone();
            let writer = rng.writer();
            let slot = rng.next_u64();
            let group_key = SigningKey::from_bytes(&rng.bytes32());
            let my_note = EpNote::sign(&id, &gid, slot, &ep("grp", &group_key), 1);
            let is_host = cfg.hosts.contains(&dv);
            d.push(Device {
                w: Writer::new(id.clone(), writer),
                id,
                slot,
                rep: Replica::new(&ctx, is_host),
                group_key,
                my_note,
                host: None,
                acks: BTreeMap::new(),
                op_out: vec![],
                outbox: vec![],
                refused: vec![],
                online: true,
                sched: is_host.then(Sched::default),
                ann: None,
            });
        }
        let mut w = World {
            ctx,
            rng,
            now: 0,
            d,
            hosts: vec![],
            live: BTreeMap::new(),
            cut: BTreeSet::new(),
            frames: vec![],
        };
        for i in 0..w.d.len() {
            let e = w.gep(i);
            w.live.insert(e, i);
        }
        for h in cfg.hosts.clone() {
            let permit = w.permit(h, 10 + h as u64);
            w.d[h].host = Some(HostRecord {
                note: w.d[h].my_note.clone(),
                permit,
            });
            w.hosts.push(h);
        }
        // Join bootstrap (invite/ticket) is a fixture: every member and host accepts every
        // host record; members hand their group note to every host.
        for h in w.hosts.clone() {
            let rec = w.d[h].host.as_ref().unwrap().wire();
            let mut targets = MEMBERS.to_vec();
            targets.extend(w.hosts.iter().copied());
            for i in targets {
                w.d[i].rep.accept_host_record(&w.ctx, &rec).unwrap();
            }
            for m in MEMBERS {
                let n = w.d[m].my_note.wire();
                w.d[h].rep.accept_note(&w.ctx, &n).unwrap();
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
    fn send(&mut self, from: usize, to: usize, kind: &'static str, bytes: Vec<u8>) {
        self.frames.push(Frame {
            from,
            to,
            kind,
            bytes,
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
            d.rep.snap(),
            &d.my_note,
            &d.acks,
            &d.op_out,
            &d.outbox,
            refused,
            &d.host,
            &d.sched,
            &d.ann,
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
    fn permit(&self, h: usize, nonce: u64) -> MGrant {
        MGrant::issue(
            &self.d[OLGA].id,
            &self.ctx.gid,
            &self.did(h),
            Some(self.d[h].slot),
            &[Cap::Host],
            false,
            nonce,
            None,
        )
    }
    fn host_ep(&self, i: usize, h: usize) -> Option<String> {
        self.d[i]
            .rep
            .hosts
            .get(&self.hk(h))
            .map(|r| r.note.claims.endpoint.clone())
    }
    fn accept_host_record(&mut self, i: usize, bytes: &[u8]) -> Result<(), Refuse> {
        self.d[i].rep.accept_host_record(&self.ctx, bytes)
    }
    fn host_accept_note(&mut self, h: usize, bytes: &[u8]) -> Result<(), Refuse> {
        self.d[h].rep.accept_note(&self.ctx, bytes)
    }
    fn accept_op(&mut self, i: usize, bytes: &[u8]) -> Result<Acc, Refuse> {
        self.d[i].rep.accept_op(&self.ctx, bytes)
    }
    fn accept_post(&mut self, i: usize, p: Post) -> Verdict {
        self.d[i].rep.accept_post(&self.ctx, p)
    }
    fn accept_announce(&mut self, i: usize, bytes: &[u8]) -> Result<(), Refuse> {
        let me = self.did(i);
        self.d[i].rep.accept_announce(&self.ctx, bytes, &me)
    }
    fn admits(&mut self, x: usize, y: usize) -> Result<(), Refuse> {
        let Some(r) = self.d[y].host.clone() else {
            return Err(Refuse::NotHost);
        };
        match self.accept_host_record(x, &r.wire()) {
            Ok(()) | Err(Refuse::Stale) => {}
            Err(e) => return Err(e),
        }
        if self.d[x].rep.hosts.get(&self.hk(y)) != Some(&r) {
            return Err(Refuse::Stale);
        }
        Ok(())
    }
    // ---- ops ----
    fn flag_and_submit(&mut self, i: usize, target: usize) -> SignedOp {
        let nonce = self.rng.next_u64();
        let seen = self.d[i].rep.ops.keys().cloned().collect();
        let grant = self.ctx.grants.get(&self.did(i)).cloned();
        let op = SignedOp::sign(
            &self.d[i].id,
            &self.ctx.gid,
            grant,
            nonce,
            seen,
            Op::Flag {
                target: self.did(target),
            },
        );
        assert_eq!(self.accept_op(i, &op.wire()), Ok(Acc::Applied), "flag {i}");
        self.d[i].op_out.push(op.clone());
        self.after();
        op
    }
    // ---- content ----
    fn post(&mut self, m: usize, text: &str) -> Post {
        let seen = self.d[m].rep.ops.keys().cloned().collect();
        self.post_citing(m, text, seen)
    }
    // A post citing an explicit frontier. With a pre-removal frontier this is the D02
    // backdating shape: identical to an honest offline post.
    fn post_citing(&mut self, m: usize, text: &str, frontier: BTreeSet<String>) -> Post {
        let dv = &mut self.d[m];
        dv.w.observe(&dv.rep.board);
        let p = dv.w.post(&self.ctx.gid, text, frontier);
        dv.outbox.push(p.clone());
        p
    }
    // ---- calls (NEW fixture): caller dials the host it knows; host relays to the callee ----
    fn call(&mut self, caller: usize, h: usize, callee: usize) -> Result<(), Refuse> {
        let nonce = self.rng.next_u64();
        let req = CallReq::sign(
            &self.d[caller].id,
            CallBody {
                group: self.ctx.gid.clone(),
                caller: self.did(caller),
                callee: self.did(callee),
                nonce,
            },
        );
        let Some(e) = self.host_ep(caller, h) else {
            return Err(Refuse::NotHost);
        };
        if self.dial(caller, &e) != Ok(h) {
            return Err(Refuse::Unreachable);
        }
        self.send(caller, h, "call", req.wire());
        self.d[h].rep.accept_call(&self.ctx, &req.wire(), None)?;
        let ce = self.d[h]
            .rep
            .dir
            .as_ref()
            .and_then(|dir| dir.get(&self.hk(callee)))
            .map(|n| n.claims.endpoint.clone());
        match ce.map(|e| self.dial(h, &e)) {
            Some(Ok(j)) if j == callee => {}
            _ => return Err(Refuse::Unreachable),
        }
        self.send(h, callee, "call", req.wire());
        let me = self.did(callee);
        self.d[callee]
            .rep
            .accept_call(&self.ctx, &req.wire(), Some(&me))
    }
    // ---- links ----
    fn connect(&mut self, m: usize, h: usize) -> Result<Path, RouteErr> {
        let r = self.connect_inner(m, h);
        self.after();
        r
    }
    // Copied from flags.rs @82d221f `connect_inner`; changed: the removed member's dial gets
    // the signed notice AND (U11, section 11 Q8 = yes) her queued posts are uploaded and
    // judged one by one (signed frontier + cap candidate). Nothing else flows to her.
    fn connect_inner(&mut self, m: usize, h: usize) -> Result<Path, RouteErr> {
        if !self.d[m].online {
            return Err(RouteErr::SelfOffline);
        }
        if !self.d[h].online {
            return Err(RouteErr::PeerOffline);
        }
        let member_side = self.host_ep(m, h).and_then(|e| self.dial(m, &e).ok()) == Some(h);
        if self.d[h].host.is_some() && self.d[h].rep.der.removed.contains(&self.did(m)) {
            if member_side {
                self.removal_notice(m, h);
                for p in std::mem::take(&mut self.d[m].outbox) {
                    self.send(m, h, "post", p.wire());
                    if let Verdict::Rejected(why) = self.accept_post(h, p.clone()) {
                        self.d[m].refused.push((p, why));
                    }
                }
                return Err(RouteErr::Removed);
            }
            return Err(RouteErr::Lost);
        }
        let path = if member_side {
            Path::MemberDialedHost
        } else {
            let e = self.d[h]
                .rep
                .dir
                .as_ref()
                .and_then(|dir| dir.get(&self.hk(m)))
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
    fn connect_from_host(&mut self, h: usize, m: usize) -> Result<Path, RouteErr> {
        let Some(e) = self.d[h]
            .rep
            .dir
            .as_ref()
            .and_then(|dir| dir.get(&self.hk(m)))
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
        self.after();
        Ok(Path::HostDialedMember)
    }
    // Copied from flags.rs @82d221f: the notice is the signed flags + their causal frontier.
    fn removal_notice(&mut self, m: usize, h: usize) {
        let me = self.did(m);
        let fl = self.d[h]
            .rep
            .der
            .flags
            .get(&me)
            .cloned()
            .unwrap_or_default();
        for id in closure(&self.d[h].rep.ops, &fl) {
            let o = self.d[h].rep.ops[&id].clone();
            self.send(h, m, "removal-notice", o.wire());
            let _ = self.accept_op(m, &o.wire());
        }
    }
    // Copied from flags.rs @82d221f; added: a host hands its pending announcement to the member.
    fn exchange(&mut self, m: usize, h: usize) {
        let (hkey, mkey) = (self.hk(h), self.hk(m));
        let note = self.d[m].my_note.clone();
        if self.d[m].acks.get(&hkey).copied().unwrap_or(0) < note.claims.seq {
            self.send(m, h, "ep-note", note.wire());
            let _ = self.host_accept_note(h, &note.wire());
        }
        for o in std::mem::take(&mut self.d[m].op_out) {
            self.send(m, h, "op", o.wire());
            let _ = self.accept_op(h, &o.wire());
        }
        for p in std::mem::take(&mut self.d[m].outbox) {
            self.send(m, h, "post", p.wire());
            if let Verdict::Rejected(why) = self.accept_post(h, p.clone()) {
                self.d[m].refused.push((p, why));
            }
        }
        let recs: Vec<HostRecord> = self.d[h].rep.hosts.values().cloned().collect();
        for r in recs {
            let k = dir_key(&r.note.claims.owner, r.note.claims.slot);
            if self.d[m].rep.hosts.get(&k) != Some(&r) {
                self.send(h, m, "host-record", r.wire());
                let _ = self.accept_host_record(m, &r.wire());
            }
        }
        let ops: Vec<SignedOp> = self.d[h]
            .rep
            .ops
            .iter()
            .filter(|(id, _)| !self.d[m].rep.ops.contains_key(*id))
            .map(|(_, o)| o.clone())
            .collect();
        for o in ops {
            self.send(h, m, "op", o.wire());
            let _ = self.accept_op(m, &o.wire());
        }
        let missing: Vec<Post> = self.d[h]
            .rep
            .store
            .iter()
            .filter(|(id, _)| !self.d[m].rep.store.contains_key(*id))
            .map(|(_, p)| p.clone())
            .collect();
        for p in missing {
            self.send(h, m, "post", p.wire());
            self.accept_post(m, p);
        }
        if let Some(a) = self.d[h].ann.clone() {
            let have = self.d[m].rep.announced.get(&hkey).map(|c| c.body.seq);
            if a.body.rotate_at > self.now && have < Some(a.body.seq) {
                self.send(h, m, "rot-announce", a.wire());
                let _ = self.accept_announce(m, &a.wire());
            }
        }
        let acked = self.d[h]
            .rep
            .dir
            .as_ref()
            .and_then(|dir| dir.get(&mkey))
            .map(|n| n.claims.seq);
        if let Some(s) = acked {
            self.d[m].acks.insert(hkey, s);
        }
        let dv = &mut self.d[m];
        dv.w.observe(&dv.rep.board);
    }
    fn host_sync(&mut self, a: usize, b: usize) -> Result<(), RouteErr> {
        let r = self.host_sync_inner(a, b);
        self.after();
        r
    }
    fn host_sync_inner(&mut self, a: usize, b: usize) -> Result<(), RouteErr> {
        let (ka, kb) = (self.hk(a), self.hk(b));
        let ea = self.d[b]
            .rep
            .hosts
            .get(&ka)
            .map(|r| r.note.claims.endpoint.clone());
        let eb = self.d[a]
            .rep
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
        }
        Ok(())
    }
    fn xfer(&mut self, from: usize, to: usize) {
        let recs: Vec<HostRecord> = self.d[from].rep.hosts.values().cloned().collect();
        for r in recs {
            let k = dir_key(&r.note.claims.owner, r.note.claims.slot);
            if self.d[to].rep.hosts.get(&k) != Some(&r) {
                self.send(from, to, "host-record", r.wire());
                let _ = self.accept_host_record(to, &r.wire());
            }
        }
        let ops: Vec<SignedOp> = self.d[from]
            .rep
            .ops
            .iter()
            .filter(|(id, _)| !self.d[to].rep.ops.contains_key(*id))
            .map(|(_, o)| o.clone())
            .collect();
        for o in ops {
            self.send(from, to, "op", o.wire());
            let _ = self.accept_op(to, &o.wire());
        }
        let notes: Vec<EpNote> = self.d[from]
            .rep
            .dir
            .as_ref()
            .map(|dir| dir.values().cloned().collect())
            .unwrap_or_default();
        for n in notes {
            let k = dir_key(&n.claims.owner, n.claims.slot);
            let cur = self.d[to]
                .rep
                .dir
                .as_ref()
                .and_then(|dir| dir.get(&k))
                .map(|c| c.claims.seq);
            if cur < Some(n.claims.seq) {
                self.send(from, to, "dir-note", n.wire());
                let _ = self.host_accept_note(to, &n.wire());
            }
        }
        let posts: Vec<Post> = self.d[from]
            .rep
            .store
            .iter()
            .filter(|(id, _)| {
                !self.d[to].rep.store.contains_key(*id)
                    && !self.d[to].rep.post_pending.contains_key(*id)
            })
            .map(|(_, p)| p.clone())
            .collect();
        for p in posts {
            self.send(from, to, "post", p.wire());
            self.accept_post(to, p);
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
    // ---- NEW: removal -> announcement -> delayed rotation (U12) ----
    // Runs after every link/op: each host that has derived a removal it has not scheduled
    // yet schedules a rotation (per `Batching`), signs an announcement and pushes it to every
    // remaining member it can reach (its directory no longer holds removed members).
    fn after(&mut self) {
        loop {
            let mut progressed = false;
            for h in self.hosts.clone() {
                let fresh: BTreeSet<String> = {
                    let s = self.d[h].sched.as_ref().unwrap();
                    self.d[h]
                        .rep
                        .der
                        .removed
                        .iter()
                        .filter(|x| !s.learned.contains_key(*x))
                        .cloned()
                        .collect()
                };
                if !fresh.is_empty() {
                    progressed = true;
                    self.schedule(h, fresh);
                }
            }
            if !progressed {
                break;
            }
        }
    }
    fn schedule(&mut self, h: usize, fresh: BTreeSet<String>) {
        let now = self.now;
        let due = now + ROTATION_DELAY;
        let batching = self.ctx.cfg.batching;
        let s = self.d[h].sched.as_mut().unwrap();
        for x in &fresh {
            s.learned.insert(x.clone(), now);
        }
        let rotate_at = match batching {
            Batching::PerRemoval => {
                s.pending.entry(due).or_default().extend(fresh.clone());
                due
            }
            Batching::KeepFirstDeadline => {
                let at = s.pending.keys().next().copied().unwrap_or(due);
                s.pending.entry(at).or_default().extend(fresh.clone());
                at
            }
            Batching::ExtendToLast => {
                let mut all = fresh.clone();
                for (_, v) in std::mem::take(&mut s.pending) {
                    all.extend(v);
                }
                s.pending.insert(due, all);
                due
            }
        };
        s.seq += 1;
        let seq = s.seq;
        let mut cites = BTreeSet::new();
        for x in &fresh {
            let fl = self.d[h].rep.der.flags.get(x).cloned().unwrap_or_default();
            cites.extend(closure(&self.d[h].rep.ops, &fl));
        }
        let body = AnnBody {
            group: self.ctx.gid.clone(),
            host: self.did(h),
            slot: self.d[h].slot,
            seq,
            removed: fresh,
            cites,
            rotate_at,
        };
        let permit = self.d[h].host.as_ref().unwrap().permit.clone();
        self.d[h].ann = Some(Announce::sign(&self.d[h].id, body, permit));
        // The host reaches members itself (section 9 Q6); unreachable members get it on
        // their next link while it is still pending.
        let dir: Vec<EpNote> = self.d[h]
            .rep
            .dir
            .as_ref()
            .map(|dir| dir.values().cloned().collect())
            .unwrap_or_default();
        for n in dir {
            if let Ok(j) = self.dial(h, &n.claims.endpoint) {
                if self.did(j) == n.claims.owner && self.admits(j, h).is_ok() {
                    self.exchange(j, h);
                }
            }
        }
    }
    // Moves the fake clock and performs every rotation that falls due (hosts first, then
    // members under MembersToo). A host offline at its deadline rotates when next advanced.
    fn advance(&mut self, to: u64) {
        assert!(to >= self.now);
        self.now = to;
        loop {
            let due = self
                .hosts
                .iter()
                .filter(|&&h| self.d[h].online)
                .filter_map(|&h| {
                    let (&at, _) = self.d[h].sched.as_ref()?.pending.iter().next()?;
                    (at <= to).then_some((at, h))
                })
                .min();
            let Some((at, h)) = due else { break };
            let s = self.d[h].sched.as_mut().unwrap();
            let set = s.pending.remove(&at).unwrap();
            s.done.push((to, set));
            self.rotate_host(h);
            self.after();
        }
        for m in MEMBERS {
            let me = self.did(m);
            let r = &mut self.d[m].rep;
            if r.member_due.is_some_and(|t| t <= to) && !r.der.removed.contains(&me) {
                r.member_due = None;
                if self.d[m].online {
                    self.rotate_member(m);
                }
            }
        }
        self.after();
    }
    // Copied from flags.rs @82d221f `rotate_member` (no retention).
    fn rotate_member(&mut self, m: usize) -> Vec<usize> {
        let old = self.gep(m);
        self.live.remove(&old);
        self.d[m].group_key = SigningKey::from_bytes(&self.rng.bytes32());
        let seq = self.d[m].my_note.claims.seq + 1;
        let new = self.gep(m);
        let note = EpNote::sign(&self.d[m].id, &self.ctx.gid, self.d[m].slot, &new, seq);
        self.d[m].my_note = note.clone();
        self.live.insert(new, m);
        let mut reached = vec![];
        for h in self.hosts.clone() {
            let Some(e) = self.host_ep(m, h) else {
                continue;
            };
            if self.dial(m, &e) == Ok(h) {
                self.send(m, h, "ep-note", note.wire());
                if self.host_accept_note(h, &note.wire()).is_ok() {
                    let k = self.hk(h);
                    self.d[m].acks.insert(k, seq);
                    reached.push(h);
                }
            }
        }
        reached
    }
    // Copied from flags.rs @82d221f `rotate_host`: new key, old endpoint dead, the host dials
    // every member still in its directory (removed members were dropped), hands over its new
    // record and runs a full exchange; then syncs with the other hosts.
    fn rotate_host(&mut self, h: usize) -> Vec<usize> {
        let old = self.gep(h);
        self.live.remove(&old);
        self.d[h].group_key = SigningKey::from_bytes(&self.rng.bytes32());
        let seq = self.d[h].my_note.claims.seq + 1;
        let new = self.gep(h);
        let note = EpNote::sign(&self.d[h].id, &self.ctx.gid, self.d[h].slot, &new, seq);
        self.d[h].my_note = note.clone();
        self.live.insert(new, h);
        self.d[h].host.as_mut().unwrap().note = note;
        let rec = self.d[h].host.as_ref().unwrap().wire();
        let _ = self.accept_host_record(h, &rec);
        let dir: Vec<EpNote> = self.d[h]
            .rep
            .dir
            .as_ref()
            .map(|dir| dir.values().cloned().collect())
            .unwrap_or_default();
        let mut reached = vec![];
        for n in dir {
            if let Ok(j) = self.dial(h, &n.claims.endpoint) {
                if self.did(j) == n.claims.owner {
                    self.send(h, j, "host-record", rec.clone());
                    if self.admits(j, h).is_ok() {
                        self.exchange(j, h);
                        reached.push(j);
                    }
                }
            }
        }
        for other in self.hosts.clone() {
            if other != h {
                let _ = self.host_sync_inner(h, other);
            }
        }
        reached
    }
    fn assert_converged(&self, devs: &[usize]) {
        let first = &self.d[devs[0]].rep.board;
        for &i in devs {
            let b = &self.d[i].rep.board;
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
    // Every endpoint `i` has ever been told about (frames + state), other than its own.
    fn known_endpoints(&self, i: usize) -> Vec<String> {
        let hay = self.received(i);
        let own = self.gep(i);
        let mut all: Vec<String> = self.live.keys().cloned().collect();
        for dv in &self.d {
            if let Some(r) = &dv.host {
                all.push(r.note.claims.endpoint.clone());
            }
        }
        for f in &self.frames {
            if let Ok(r) = serde_json::from_slice::<HostRecord>(&f.bytes) {
                all.push(r.note.claims.endpoint);
            }
            if let Ok(n) = serde_json::from_slice::<EpNote>(&f.bytes) {
                all.push(n.claims.endpoint);
            }
        }
        all.sort();
        all.dedup();
        all.into_iter()
            .filter(|e| *e != own && has(&hay, e.as_bytes()))
            .collect()
    }
    // U12 end state for a removed device: every endpoint it ever learned is dead, and it
    // cannot link to any host in either direction.
    fn assert_reaches_nobody(&mut self, i: usize) {
        for e in self.known_endpoints(i) {
            assert_eq!(self.dial(i, &e), Err(RouteErr::EndpointGone), "{e}");
        }
        for h in self.hosts.clone() {
            assert_eq!(self.connect(i, h), Err(RouteErr::Lost), "host {h}");
            assert_eq!(self.connect_from_host(h, i), Err(RouteErr::NoAddress));
        }
    }
    fn texts(&self, i: usize) -> BTreeSet<String> {
        self.d[i].rep.board.texts()
    }
}
fn world() -> World {
    World::new(Cfg::default())
}
fn with(f: impl FnOnce(&mut Cfg)) -> World {
    let mut cfg = Cfg::default();
    f(&mut cfg);
    World::new(cfg)
}
fn remaining(gone: &[usize]) -> Vec<usize> {
    MEMBERS.into_iter().filter(|m| !gone.contains(m)).collect()
}
fn with_hosts(devs: Vec<usize>) -> Vec<usize> {
    let mut v = devs;
    v.extend([H1, H2]);
    v
}
// U9: three distinct admins flag `target`; one round delivers the flags to both hosts.
fn remove(w: &mut World, target: usize) -> Vec<SignedOp> {
    let fs = [A1, BOB, DAVE]
        .into_iter()
        .map(|a| w.flag_and_submit(a, target))
        .collect();
    w.round();
    fs
}
fn note_seq(w: &World, i: usize) -> u64 {
    w.d[i].my_note.claims.seq
}

// ---- tests ---------------------------------------------------------------------------------

// KIND: invariant
// U12 detail (decided): refusal is immediate. In the same tick in which the hosts derive the
// removal, and long before any rotation, every honest host refuses carol's NEW post (it cites
// the removal in its signed frontier) and her call request. Members refuse the post too. Every
// refusal leaves the refusing device's state byte-identical, and the callee never hears of
// the call. Control: before the removal carol's call to erin is relayed; after it bob's is.
#[test]
fn removal_refuses_new_posts_and_calls_immediately_at_every_host() {
    let mut w = world();
    w.advance(T0);
    let carol = w.did(CAROL);
    assert_eq!(w.call(CAROL, H1, ERIN), Ok(()), "control: member may call");
    remove(&mut w, CAROL);
    assert_eq!(w.now, T0, "no time has passed");
    for h in [H1, H2] {
        assert!(w.d[h].rep.der.removed.contains(&carol), "host {h}");
        assert_eq!(w.d[h].sched.as_ref().unwrap().learned[&carol], T0);
        assert_eq!(note_seq(&w, h), 1, "not rotated yet");
    }
    assert!(w.d[CAROL].rep.der.removed.contains(&carol), "told");

    let p = w.post(CAROL, "carol after removal");
    w.d[CAROL].outbox.clear();
    for i in with_hosts(remaining(&[CAROL])) {
        let before = w.snap(i);
        assert_eq!(
            w.accept_post(i, p.clone()),
            Verdict::Rejected(Why::NotMember),
            "device {i}"
        );
        assert_eq!(w.snap(i), before, "device {i}");
    }
    let q = w.post(CAROL, "carol after removal, via link");
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    assert_eq!(w.d[CAROL].refused, vec![(q, Why::NotMember)]);

    for h in [H1, H2] {
        let (bh, be) = (w.snap(h), w.snap(ERIN));
        let n = w.frames.len();
        assert_eq!(w.call(CAROL, h, ERIN), Err(Refuse::Removed), "host {h}");
        assert_eq!(w.snap(h), bh, "host {h}");
        assert_eq!(w.snap(ERIN), be);
        assert!(w.frames[n..].iter().all(|f| f.to != ERIN), "no ring");
    }
    assert_eq!(w.call(BOB, H1, ERIN), Ok(()), "control: bob still may call");
    for i in with_hosts(remaining(&[CAROL])) {
        assert!(!w.texts(i).iter().any(|t| t.starts_with("carol after")));
    }
}

// KIND: invariant
// U12 (decided): after the removal every host signs a "rotating soon" announcement and sends
// it to the REMAINING members only (both hosts, all six, none to carol). Each member verifies
// it and records rotate_at = removal + 2 days. One tick before the deadline nothing has
// rotated and carol can still dial the hosts. At the deadline each host rotates exactly once,
// dials every remaining member itself and hands over its new record; members send nothing
// and do not rotate. Afterwards carol reaches nobody: every endpoint she ever learned is
// dead, no frame reaches her, and the hosts hold no address for her. The group keeps working.
#[test]
fn hosts_announce_to_remaining_members_only_then_rotate_after_two_days_and_reconnect_them() {
    let mut w = world();
    w.advance(T0);
    let carol = w.did(CAROL);
    let rest = remaining(&[CAROL]);
    let n0 = w.frames.len();
    remove(&mut w, CAROL);
    for h in [H1, H2] {
        let to: BTreeSet<usize> = w.frames[n0..]
            .iter()
            .filter(|f| f.kind == "rot-announce" && f.from == h)
            .map(|f| f.to)
            .collect();
        assert_eq!(
            to,
            rest.iter().copied().collect::<BTreeSet<usize>>(),
            "host {h}"
        );
    }
    assert!(
        !w.frames
            .iter()
            .any(|f| f.kind == "rot-announce" && f.to == CAROL)
    );
    assert!(!has(&w.received(CAROL), b"rotate_at"));
    assert!(w.d[CAROL].rep.announced.is_empty());
    for &m in &rest {
        assert_eq!(w.d[m].rep.announced.len(), 2, "member {m}");
        for a in w.d[m].rep.announced.values() {
            assert_eq!(a.body.rotate_at, T0 + ROTATION_DELAY);
            assert_eq!(a.body.removed, BTreeSet::from([carol.clone()]));
        }
    }

    w.advance(T0 + ROTATION_DELAY - 1);
    let old: Vec<String> = [H1, H2].iter().map(|&h| w.gep(h)).collect();
    for (k, h) in [H1, H2].into_iter().enumerate() {
        assert_eq!(note_seq(&w, h), 1);
        assert_eq!(w.dial(CAROL, &old[k]), Ok(h), "still live during the delay");
    }

    let n1 = w.frames.len();
    w.advance(T0 + ROTATION_DELAY);
    for (k, h) in [H1, H2].into_iter().enumerate() {
        assert_eq!(note_seq(&w, h), 2, "host {h} rotated once");
        assert_eq!(w.d[h].sched.as_ref().unwrap().done.len(), 1);
        assert_eq!(w.dial(BOB, &old[k]), Err(RouteErr::EndpointGone));
        for &m in &rest {
            assert_eq!(
                w.d[m].rep.hosts.get(&w.hk(h)),
                w.d[h].host.as_ref(),
                "member {m} reconnected to host {h} by the host"
            );
        }
    }
    for &m in &rest {
        assert_eq!(note_seq(&w, m), 1, "member {m} did not rotate");
    }
    assert!(
        w.frames[n1..].iter().all(|f| w.hosts.contains(&f.from)),
        "members did nothing"
    );
    assert!(w.frames[n1..].iter().all(|f| f.to != CAROL));

    w.assert_reaches_nobody(CAROL);
    w.post(BOB, "after rotation");
    w.round();
    w.assert_converged(&with_hosts(rest));
    assert!(w.texts(H2).contains("after rotation"));
    assert!(!w.texts(CAROL).contains("after rotation"));
}

// KIND: invariant
// U11 + U12 (decided): carol wrote a post while cut off, before the flags (its signed frontier
// predates the removal). One day into the delay she dials H1: she is told, and her queued
// post is uploaded and kept, and reaches every remaining member. A post citing the removal is
// refused. After the hosts rotate, a backdated post (pre-removal frontier) cannot be
// delivered at all: every host address she knows is dead, it stays in her outbox, and no
// other device ever holds it.
#[test]
fn pre_removal_posts_accepted_until_hosts_rotate_then_backdated_post_undeliverable() {
    let mut w = world();
    w.advance(T0);
    let carol = w.did(CAROL);
    let rest = with_hosts(remaining(&[CAROL]));
    w.cut(CAROL, H1);
    w.cut(CAROL, H2);
    w.post(CAROL, "offline before removal");
    remove(&mut w, CAROL);
    assert!(!w.d[CAROL].rep.der.removed.contains(&carol), "not told yet");

    w.advance(T0 + TICKS_PER_DAY);
    w.heal(CAROL, H1);
    w.heal(CAROL, H2);
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    assert!(w.d[CAROL].rep.der.removed.contains(&carol), "told");
    assert!(w.d[CAROL].outbox.is_empty() && w.d[CAROL].refused.is_empty());
    let q = w.post(CAROL, "cites the removal");
    assert_eq!(w.connect(CAROL, H2), Err(RouteErr::Removed));
    assert_eq!(w.d[CAROL].refused, vec![(q, Why::NotMember)]);
    w.round();
    for &i in &rest {
        assert!(w.texts(i).contains("offline before removal"), "device {i}");
        assert!(!w.texts(i).contains("cites the removal"), "device {i}");
    }

    w.advance(T0 + ROTATION_DELAY);
    let b = w.post_citing(CAROL, "backdated after rotation", BTreeSet::new());
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Lost));
    assert_eq!(w.connect(CAROL, H2), Err(RouteErr::Lost));
    w.round();
    assert_eq!(w.d[CAROL].outbox, vec![b.clone()], "never uploaded");
    for &i in &rest {
        assert!(!w.d[i].rep.store.contains_key(&b.id()), "device {i}");
    }
    w.assert_reaches_nobody(CAROL);
}

// KIND: candidate-policy G-HISTORY
// U11 open: bound the removed member's pre-removal uploads? Carol wrote 3 honest posts while
// cut off before the flags. During the delay she is told, then signs 2 backdated posts with
// the same (pre-removal) frontier. Honest and backdated posts have the identical signed shape.
// - RotationOnly: all 5 are kept everywhere; only the rotation ends the window.
// - Count(2): each host keeps the first 2 (both honest) and refuses the rest with UploadCap,
//   state unchanged; H2 receives the same 2 by host sync and also refuses the 3rd. The cap
//   cannot tell honest backlog from backdating, so honest post 3 is lost too. The cap is
//   enforced by hosts only: a member handed post 3 directly accepts it (signed frontier).
#[test]
fn count_cap_on_pre_removal_uploads_vs_rotation_only() {
    for cap in [UploadCap::RotationOnly, UploadCap::Count(2)] {
        let mut w = with(|c| c.cap = cap);
        w.advance(T0);
        w.cut(CAROL, H1);
        w.cut(CAROL, H2);
        let honest: Vec<Post> = (1..=3)
            .map(|k| w.post(CAROL, &format!("honest {k}")))
            .collect();
        remove(&mut w, CAROL);
        w.advance(T0 + TICKS_PER_DAY);
        w.heal(CAROL, H1);
        assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
        let back: Vec<Post> = (1..=2)
            .map(|k| w.post_citing(CAROL, &format!("backdated {k}"), BTreeSet::new()))
            .collect();
        assert_eq!(honest[0].body.ops_seen, back[0].body.ops_seen, "same shape");
        assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
        w.round();
        let carol_texts = |w: &World, i: usize| {
            w.texts(i)
                .into_iter()
                .filter(|t| t.starts_with("honest") || t.starts_with("backdated"))
                .count()
        };
        match cap {
            UploadCap::RotationOnly => {
                assert!(w.d[CAROL].refused.is_empty());
                for i in with_hosts(remaining(&[CAROL])) {
                    assert_eq!(carol_texts(&w, i), 5, "device {i}");
                }
            }
            UploadCap::Count(n) => {
                assert_eq!(n, 2);
                let why: Vec<Why> = w.d[CAROL].refused.iter().map(|(_, y)| *y).collect();
                assert_eq!(why, vec![Why::UploadCap; 3]);
                for i in with_hosts(remaining(&[CAROL])) {
                    assert_eq!(
                        w.texts(i),
                        w.texts(H1),
                        "device {i} holds what the hosts kept"
                    );
                    assert!(w.texts(i).contains("honest 2"));
                    assert!(!w.texts(i).contains("honest 3"), "honest backlog lost");
                }
                for h in [H1, H2] {
                    let before = w.snap(h);
                    assert_eq!(
                        w.accept_post(h, honest[2].clone()),
                        Verdict::Rejected(Why::UploadCap),
                        "host {h}"
                    );
                    assert_eq!(w.snap(h), before, "host {h}");
                }
                assert_eq!(w.accept_post(ERIN, honest[2].clone()), Verdict::Accepted);
            }
        }
    }
}

// KIND: candidate-policy G-TIME
// U12 open: several removals inside one delay window. Carol is removed at T0; erin (who got
// carol's announcement as a member) is cut off, writes an offline post, and is removed at
// T0+1d. At T0+2d+1h erin and carol try to upload:
// - KeepFirstDeadline: one rotation at T0+2d covering both; erin's window was only 1 day.
// - ExtendToLast: one rotation at T0+3d covering both; both keep a window (carol's grows).
// - PerRemoval: rotations at T0+2d and T0+3d. The FIRST already cuts erin off (removed
//   members are not reconnected), so the second rotation adds cost but no window.
// In all three, both removed members reach nobody at the end.
#[test]
fn several_removals_in_one_window_keep_first_vs_extend_vs_one_per_removal() {
    for b in [
        Batching::KeepFirstDeadline,
        Batching::ExtendToLast,
        Batching::PerRemoval,
    ] {
        let mut w = with(|c| c.batching = b);
        w.advance(T0);
        let (carol, erin) = (w.did(CAROL), w.did(ERIN));
        remove(&mut w, CAROL);
        for a in w.d[ERIN].rep.announced.values() {
            assert_eq!(a.body.rotate_at, T0 + ROTATION_DELAY, "erin was a member");
        }
        w.advance(T0 + TICKS_PER_DAY);
        w.cut(ERIN, H1);
        w.cut(ERIN, H2);
        let e1 = w.post(ERIN, "erin offline");
        remove(&mut w, ERIN);
        w.advance(T0 + ROTATION_DELAY);
        w.advance(T0 + ROTATION_DELAY + TICKS_PER_HOUR);
        w.heal(ERIN, H1);
        w.heal(ERIN, H2);
        let erin_try = w.connect(ERIN, H1);
        let carol_try = w.connect(CAROL, H1);
        w.advance(T0 + ROTATION_DELAY + TICKS_PER_DAY);
        let done = &w.d[H1].sched.as_ref().unwrap().done;
        let both = BTreeSet::from([carol.clone(), erin.clone()]);
        match b {
            Batching::KeepFirstDeadline => {
                assert_eq!(*done, vec![(T0 + ROTATION_DELAY, both)]);
                assert_eq!(erin_try, Err(RouteErr::Lost));
                assert_eq!(carol_try, Err(RouteErr::Lost));
                assert!(!w.d[H1].rep.store.contains_key(&e1.id()));
            }
            Batching::ExtendToLast => {
                assert_eq!(*done, vec![(T0 + ROTATION_DELAY + TICKS_PER_DAY, both)]);
                assert_eq!(erin_try, Err(RouteErr::Removed));
                assert_eq!(carol_try, Err(RouteErr::Removed), "carol's window grew");
                assert!(w.d[H1].rep.store.contains_key(&e1.id()));
            }
            Batching::PerRemoval => {
                assert_eq!(
                    *done,
                    vec![
                        (T0 + ROTATION_DELAY, BTreeSet::from([carol.clone()])),
                        (
                            T0 + ROTATION_DELAY + TICKS_PER_DAY,
                            BTreeSet::from([erin.clone()])
                        ),
                    ]
                );
                assert_eq!(erin_try, Err(RouteErr::Lost), "cut by the first rotation");
                assert_eq!(carol_try, Err(RouteErr::Lost));
                assert!(!w.d[H1].rep.store.contains_key(&e1.id()));
            }
        }
        let rotations = w.d[H1].sched.as_ref().unwrap().done.len() as u64;
        assert_eq!(note_seq(&w, H1), 1 + rotations);
        w.assert_reaches_nobody(CAROL);
        w.assert_reaches_nobody(ERIN);
    }
}

// KIND: invariant
// U12 (decided: the host reconnects members itself). Erin is offline from before the removal
// until after the rotation, so she never gets the announcement. Back online, the host address
// she holds is dead and she cannot reach a host herself. Each host dials her directory
// endpoint (unchanged: members do not rotate), hands over its new record and syncs; she
// learns the removal, receives the posts she missed, and converges with the group. The
// expired announcement is not delivered.
#[test]
fn member_offline_for_whole_delay_is_reconnected_by_the_host_afterwards() {
    let mut w = world();
    w.advance(T0);
    let carol = w.did(CAROL);
    w.d[ERIN].online = false;
    remove(&mut w, CAROL);
    w.post(BOB, "during the delay");
    w.round();
    assert!(w.d[ERIN].rep.announced.is_empty());
    w.advance(T0 + ROTATION_DELAY);
    w.d[ERIN].online = true;
    w.advance(T0 + ROTATION_DELAY + TICKS_PER_HOUR);
    for h in [H1, H2] {
        let e = w.host_ep(ERIN, h).unwrap();
        assert_eq!(w.dial(ERIN, &e), Err(RouteErr::EndpointGone), "host {h}");
    }
    for h in [H1, H2] {
        assert_eq!(w.connect_from_host(h, ERIN), Ok(Path::HostDialedMember));
        assert_eq!(w.d[ERIN].rep.hosts.get(&w.hk(h)), w.d[h].host.as_ref());
    }
    assert!(w.d[ERIN].rep.der.removed.contains(&carol));
    assert!(w.texts(ERIN).contains("during the delay"));
    assert!(w.d[ERIN].rep.announced.is_empty(), "expired, not delivered");
    assert_eq!(note_seq(&w, ERIN), 1);
    w.post(ERIN, "erin is back");
    w.round();
    w.assert_converged(&with_hosts(remaining(&[CAROL])));
}

// KIND: invariant
// U10 + U12: every member verifies every announcement. Twelve bad announcements are refused
// at every remaining member with state byte-identical: malformed; rotate_at changed after
// signing; signed by judy but naming H1; judy with H1's permit; judy with a self-issued
// permit; an admin's member grant used as a host permit; judy with a genuine owner permit but
// no host record any member accepted; wrong group; slot not matching the permit; naming a DID
// that is not removed; citing an unknown op; and a replay of the accepted announcement.
// A genuine newer announcement is accepted, and the older one replayed after it is Stale.
// Carol never receives an announcement; handed one, she refuses it (Removed), unchanged.
#[test]
fn forged_replayed_or_misdirected_announcements_refused_unchanged_never_sent_to_removed_member() {
    let mut w = world();
    w.advance(T0);
    remove(&mut w, CAROL);
    let rest = remaining(&[CAROL]);
    let g = w.d[H1].ann.clone().unwrap();
    for &m in &rest {
        assert_eq!(w.d[m].rep.announced.get(&w.hk(H1)), Some(&g), "member {m}");
    }
    let h1_permit = g.permit.clone();
    let judy = w.d[H3].id.clone();
    let mut tampered = g.clone();
    tampered.body.rotate_at += TICKS_PER_DAY;
    let mut as_judy = g.body.clone();
    as_judy.host = w.did(H3);
    as_judy.slot = w.d[H3].slot;
    let self_permit = MGrant::issue(
        &judy,
        &w.ctx.gid,
        &w.did(H3),
        Some(w.d[H3].slot),
        &[Cap::Host],
        false,
        1,
        None,
    );
    let mut as_alice = g.body.clone();
    as_alice.host = w.did(A1);
    as_alice.slot = w.d[A1].slot;
    let alice_grant = w.ctx.grants[&w.did(A1)].clone();
    let owner_permit = w.permit(H3, 99);
    let h1 = w.d[H1].id.clone();
    let resign = |f: &dyn Fn(&mut AnnBody)| {
        let mut b = g.body.clone();
        b.seq = 2;
        f(&mut b);
        Announce::sign(&h1, b, h1_permit.clone()).wire()
    };
    let judy_did = w.did(H3);
    let cases: Vec<(&str, Vec<u8>, Refuse)> = vec![
        (
            "malformed",
            b"not an announcement".to_vec(),
            Refuse::Malformed,
        ),
        ("tampered", tampered.wire(), Refuse::BadSignature),
        (
            "judy as H1",
            Announce::sign(&judy, g.body.clone(), h1_permit.clone()).wire(),
            Refuse::BadSignature,
        ),
        (
            "judy, H1 permit",
            Announce::sign(&judy, as_judy.clone(), h1_permit.clone()).wire(),
            Refuse::PermitInvalid,
        ),
        (
            "judy, self permit",
            Announce::sign(&judy, as_judy.clone(), self_permit).wire(),
            Refuse::PermitInvalid,
        ),
        (
            "admin grant",
            Announce::sign(&w.d[A1].id, as_alice, alice_grant).wire(),
            Refuse::PermitInvalid,
        ),
        (
            "unknown host",
            Announce::sign(&judy, as_judy, owner_permit).wire(),
            Refuse::NotHost,
        ),
        (
            "wrong group",
            resign(&|b| b.group = "other".into()),
            Refuse::WrongGroup,
        ),
        (
            "slot",
            resign(&|b| b.slot = b.slot.wrapping_add(1)),
            Refuse::DeviceMismatch,
        ),
        (
            "not removed",
            resign(&|b| b.removed = BTreeSet::from([judy_did.clone()])),
            Refuse::NotRemoved,
        ),
        (
            "unknown op",
            resign(&|b| b.cites = BTreeSet::from(["00".repeat(32)])),
            Refuse::Unknown,
        ),
        ("replay", g.wire(), Refuse::Stale),
    ];
    for (name, bytes, want) in &cases {
        for &m in &rest {
            let before = w.snap(m);
            assert_eq!(w.accept_announce(m, bytes), Err(*want), "{name} at {m}");
            assert_eq!(w.snap(m), before, "{name} at {m}");
        }
    }
    let newer = resign(&|_| {});
    assert_eq!(w.accept_announce(BOB, &newer), Ok(()));
    let before = w.snap(BOB);
    assert_eq!(w.accept_announce(BOB, &g.wire()), Err(Refuse::Stale));
    assert_eq!(w.snap(BOB), before);

    let before = w.snap(CAROL);
    assert_eq!(w.accept_announce(CAROL, &g.wire()), Err(Refuse::Removed));
    assert_eq!(w.snap(CAROL), before);
    w.advance(T0 + ROTATION_DELAY);
    assert!(
        !w.frames
            .iter()
            .any(|f| f.kind == "rot-announce" && f.to == CAROL)
    );
}

// KIND: limitation
// U12 accepted cost: during the 2-day delay carol still holds LIVE host addresses. She can
// dial both hosts at will; each dial gets only the removal notice (no records, posts or
// announcements) and leaves host state unchanged. Under U11 she can also sign NEW posts with
// a pre-removal frontier (D02 backdating); hosts cannot tell them from honest offline posts,
// so they are kept and relayed to every member. Only the rotation ends this.
#[test]
fn during_delay_removed_member_still_reaches_live_hosts_and_backdated_posts_are_kept() {
    let mut w = world();
    w.advance(T0);
    remove(&mut w, CAROL);
    w.advance(T0 + TICKS_PER_DAY);
    for h in [H1, H2] {
        let e = w.host_ep(CAROL, h).unwrap();
        assert_eq!(w.dial(CAROL, &e), Ok(h), "live address");
        let before = w.snap(h);
        let n = w.frames.len();
        for _ in 0..3 {
            assert_eq!(w.connect(CAROL, h), Err(RouteErr::Removed));
        }
        assert_eq!(w.snap(h), before, "host {h} unchanged");
        assert!(
            w.frames[n..]
                .iter()
                .filter(|f| f.to == CAROL)
                .all(|f| f.kind == "removal-notice")
        );
    }
    let b = w.post_citing(CAROL, "backdated during delay", BTreeSet::new());
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    assert!(w.d[CAROL].refused.is_empty());
    w.round();
    for i in with_hosts(remaining(&[CAROL])) {
        assert!(w.d[i].rep.store.contains_key(&b.id()), "device {i}");
    }
    w.advance(T0 + ROTATION_DELAY);
    w.assert_reaches_nobody(CAROL);
}

// KIND: candidate-policy G-DISCOVERY
// U12 open: does a member removal also rotate members' own group endpoints? Under U7 carol
// never learned any member's group endpoint (only hosts hold them), so after the host
// rotation she reaches nobody under both options. MembersToo only adds cost: every
// remaining member rotates once and sends 2 notes (one per host); HostsOnly sends none.
#[test]
fn member_endpoints_hosts_only_vs_members_also_rotate() {
    for mr in [MemberRotation::HostsOnly, MemberRotation::MembersToo] {
        let mut w = with(|c| c.members = mr);
        w.advance(T0);
        let rest = remaining(&[CAROL]);
        let n0 = w.frames.len();
        remove(&mut w, CAROL);
        w.advance(T0 + ROTATION_DELAY);
        let member_notes = w.frames[n0..]
            .iter()
            .filter(|f| f.kind == "ep-note" && MEMBERS.contains(&f.from))
            .count();
        for &m in &rest {
            let want = if mr == MemberRotation::MembersToo {
                2
            } else {
                1
            };
            assert_eq!(note_seq(&w, m), want, "member {m}");
        }
        match mr {
            MemberRotation::HostsOnly => assert_eq!(member_notes, 0),
            MemberRotation::MembersToo => assert_eq!(member_notes, rest.len() * 2),
        }
        let hay = w.received(CAROL);
        for &m in &rest {
            assert!(!has(&hay, w.gep(m).as_bytes()), "member {m} current");
        }
        for f in &w.frames {
            if let Ok(n) = serde_json::from_slice::<EpNote>(&f.bytes) {
                let member = MEMBERS.iter().any(|&m| w.did(m) == n.claims.owner);
                if member && n.claims.owner != w.did(CAROL) {
                    assert!(!has(&hay, n.claims.endpoint.as_bytes()), "any member note");
                }
            }
        }
        w.assert_reaches_nobody(CAROL);
        w.post(BOB, "still works");
        w.round();
        w.assert_converged(&with_hosts(rest));
    }
}

// KIND: limitation
// Each host runs the 2-day delay from when IT derives the removal. H2 is offline at the
// removal and learns it by host sync one day later, so it rotates one day after H1. Between
// the two rotations carol can no longer reach H1 but still reaches H2, where her backdated
// post is kept; host sync then carries it to H1 and every member. The group's effective
// upload cut-off is the LAST host's rotation, not the first.
#[test]
fn host_that_learns_removal_later_rotates_later_so_cut_off_is_the_last_host() {
    let mut w = world();
    w.advance(T0);
    let carol = w.did(CAROL);
    w.d[H2].online = false;
    remove(&mut w, CAROL);
    assert!(!w.d[H2].rep.der.removed.contains(&carol));
    w.advance(T0 + TICKS_PER_DAY);
    w.d[H2].online = true;
    assert_eq!(w.host_sync(H1, H2), Ok(()));
    assert_eq!(
        w.d[H2].sched.as_ref().unwrap().learned[&carol],
        T0 + TICKS_PER_DAY
    );
    w.advance(T0 + ROTATION_DELAY + TICKS_PER_HOUR);
    assert_eq!((note_seq(&w, H1), note_seq(&w, H2)), (2, 1));
    let b = w.post_citing(CAROL, "backdated between rotations", BTreeSet::new());
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Lost));
    assert_eq!(w.connect(CAROL, H2), Err(RouteErr::Removed));
    w.round();
    for i in with_hosts(remaining(&[CAROL])) {
        assert!(w.d[i].rep.store.contains_key(&b.id()), "device {i}");
    }
    w.advance(T0 + TICKS_PER_DAY + ROTATION_DELAY);
    assert_eq!(note_seq(&w, H2), 2);
    w.assert_reaches_nobody(CAROL);
}
