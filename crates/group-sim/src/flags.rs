//! W2-F: group removal by admin flags (U9) and group-level rotation on host revocation
//! (`docs/design/rotation-open-decisions.md` section 10 Q8, decided).
//!
//! Status: proposal — not approved. Simulation only; no production API, wire format or UI.
//! Decisions modelled: U4 revised (DID-signed content, random writer IDs), U5 revised (the
//! removed member is told with a verifiable signed notice), U7 + section 9 Q5 (multiple hosts;
//! only hosts hold member group endpoints), U9 (admins flag; at 3 flags the member is removed
//! and its grant revoked; flags/removal are DID-signed group ops every device verifies), and
//! section 10 Q8 (a removed host triggers a group-level rotation of every member's and every
//! remaining host's group endpoint; notes go only to permitted hosts).
//!
//! The U9 details in section 8 are NOT decided. Each is a `Cfg` parameter (G-AUTHORITY,
//! G-TIME) and the post rule around the 3rd flag reuses the D01/D02 candidates (signed
//! frontier vs removal wins) from `policy_candidates.rs`. `Cfg::default()` values are fixture
//! defaults chosen so other tests have *some* value; they are not selections.
//!
//! Provenance: `SimRng`, `MGrant` (+ `Cap`), `Post`/`Writer`/`Board`, `EpNote`, `HostRecord`,
//! `SignedOp` and the `World` link/host-sync/rotation machinery are COPIED from
//! `multi_host.rs` @0924939 (itself from `host_only.rs`/`authority.rs`) and changed as follows:
//! - `Board` has no roster: membership is decided by the policy layer (`Replica`), which keeps
//!   a store of policy-accepted posts and rebuilds the real Loro board when a rule drops posts.
//! - Posts carry `ops_seen` (the author's signed op frontier, as in the D01/D02 `Header`).
//! - Ops carry `seen` (the issuer's op frontier); every op is authorized at its own frontier,
//!   and ops whose frontier is missing are held pending. Membership is DERIVED from the op set.
//! - `Op` gains `Withdraw`, `Admit { lifts }` (rejoin) and flag `at`/`seen_posts`.
//! - A host that accepts a `RemoveHost` rotates and itself pushes (full exchange) to members.
//! - No rogue fixture, writer-claim rule, retention, host cap, chat request or block notice.
//!
//! Fake `u64` time, deterministic seeds, in-memory only.
use super::*;
use identity::{public_key_from_did, verify};

const POST_DOMAIN: &[u8] = b"tinline/group-sim/flag-removal-post/not-production/v1\0";
const NOTE_DOMAIN: &[u8] = b"tinline/group-sim/flag-removal-endpoint-note/not-production/v1\0";
const GRANT_DOMAIN: &[u8] = b"tinline/group-sim/flag-removal-grant/not-production/v1\0";
const OP_DOMAIN: &[u8] = b"tinline/group-sim/flag-removal-op/not-production/v1\0";
const DOC: &str = "group/w2f/2026-10-10";
const MAX_PENDING: usize = 16;
const MAX_CHAIN: usize = 8;
// U9 (decided): 3 flags. Whether they must come from distinct admins is `FlagCount`.
const FLAG_THRESHOLD: usize = 3;

// Devices. olga = owner/creator; alice (2 devices), bob, dave = admins by default;
// carol, erin = members; hugo (H1), ivan (H2), judy (H3) = host binaries with their own DIDs
// and a {Host} permit from the owner (not members, not admins).
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

// ---- helpers (copied from multi_host.rs @0924939) ------------------------------------------

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

// Copied from multi_host.rs @0924939. Deterministic SplitMix64: the ONLY source of writer IDs,
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

// ---- grants and host permits (copied from multi_host.rs @0924939) --------------------------

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
    fn digest(&self) -> String {
        digest(self)
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

// ---- DID-signed content (copied from multi_host.rs @0924939 + `ops_seen`) ------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PostBody {
    group: String,
    doc: String,
    author: String,
    writer: u64,
    seq: u64,
    ops_seen: BTreeSet<String>, // author's signed op frontier (D01/D02 "signed frontier")
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
    RemovedConcurrent,
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
// Copied from multi_host.rs @0924939 `Board` WITHOUT the roster check (the policy layer in
// `Replica` decides membership). Real Loro import + expected-content check unchanged.
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

// ---- DID-signed endpoint notes and host records (copied from multi_host.rs @0924939) ------

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

// ---- DID-signed group ops ------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Op {
    /// U9 flag. `at` = issuer's CLAIMED time (only the expiry candidates read it);
    /// `seen_posts` = posts the issuer had accepted (read only by the removal-wins rule).
    Flag {
        target: String,
        at: u64,
        seen_posts: BTreeSet<String>,
    },
    /// Withdraw one's own flag (candidate; section 8 Q4).
    Withdraw { flag: String },
    /// Admit a holder with a grant. Non-empty `lifts` = readmission that lifts these flags
    /// (rejoin candidate; section 8 Q7).
    Admit {
        grant: MGrant,
        lifts: BTreeSet<String>,
    },
    /// Remove a host by permit digest (W2-M); triggers group-level rotation (section 10 Q8).
    RemoveHost { permit: String },
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
    fn flag_at(&self) -> u64 {
        match &self.body.op {
            Op::Flag { at, .. } => *at,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refuse {
    Malformed,
    BadSignature,
    WrongGroup,
    PermitInvalid,
    DeviceMismatch,
    Revoked,
    Stale,
    NotAdmin,
    NotMember,
    NotHost,
    NotFlaggable,
    NotAllowed,
    NotFlagger,
    UnknownFlag,
    AfterThreshold,
    IssuerRemoved,
    Removed,
    GrantRevoked,
    BadLift,
    Limit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Acc {
    Applied,
    Duplicate,
    Pending,
}

// ---- configuration (open policy as parameters; defaults are NOT selections) ----------------

/// Section 8 Q1 (G-AUTHORITY): 3 flags from 3 distinct admin DIDs (lean) or any 3 flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FlagCount {
    DistinctAdmins,
    AnyFlags,
}
/// Section 8 Q2 (G-AUTHORITY): group with fewer than 3 admins (owner counted as an admin).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SmallGroup {
    OwnerAlone,
    ThresholdIsAdminCount,
    NoRemoval,
}
/// Section 8 Q3 (G-AUTHORITY): can the owner/creator be flagged?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnerRule {
    Protected,
    Flaggable,
}
/// Section 8 Q9 (G-AUTHORITY): can admins be flagged by other admins?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdminRule {
    Immune,
    Flaggable,
}
/// Section 8 Q4a (G-AUTHORITY): can a flag be withdrawn before the threshold?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Withdrawal {
    Final,
    BeforeThreshold,
}
/// Section 8 Q4b (G-TIME): do flags expire?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expiry {
    Never,
    /// Counted flags' signed (claimed) `at` must lie within this many ticks of each other.
    SignedWindow(u64),
    /// A flag counts while receiver_clock < arrival + ttl; removal latches once reached.
    ReceiverClock(u64),
}
/// Section 8 Q8 = D01/D02: posts made before/concurrently with the 3rd flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PostRule {
    /// SF: author is a member at the post's own signed op frontier.
    SignedFrontier,
    /// RW: SF, and a removed author's post is kept only if some flag on the author had
    /// already seen it (`seen_posts`). One sub-option of D01 "removal wins"; not designed.
    RemovalWins,
}
/// Section 8 Q7 (G-AUTHORITY): can a removed DID be invited back?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rejoin {
    Allowed,
    Forbidden,
}
#[derive(Clone)]
struct Cfg {
    seed: u64,
    hosts: Vec<usize>,
    admins: Vec<usize>, // devices whose DID gets an admin grant (owner is always admin)
    count: FlagCount,
    small: SmallGroup,
    owner: OwnerRule,
    admin: AdminRule,
    withdrawal: Withdrawal,
    expiry: Expiry,
    post: PostRule,
    rejoin: Rejoin,
}
impl Default for Cfg {
    fn default() -> Self {
        Self {
            seed: 0x5EED_00F1,
            hosts: vec![H1, H2],
            admins: vec![A1, BOB, DAVE],
            count: FlagCount::DistinctAdmins,
            small: SmallGroup::ThresholdIsAdminCount,
            owner: OwnerRule::Protected,
            admin: AdminRule::Immune,
            withdrawal: Withdrawal::Final,
            expiry: Expiry::Never,
            post: PostRule::SignedFrontier,
            rejoin: Rejoin::Forbidden,
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
    admins: BTreeSet<String>, // fixture admin set (counted for the threshold), incl. owner
    base: BTreeSet<String>,   // initial members
    grants: BTreeMap<String, MGrant>, // DID -> its bootstrap grant (creator has none)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
struct Derived {
    members: BTreeSet<String>, // base + admitted
    removed: BTreeSet<String>,
    roster: BTreeSet<String>, // members - removed
    revoked_grants: BTreeSet<String>,
    flags: BTreeMap<String, BTreeSet<String>>, // target -> effective (counted) flag op ids
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
    fn threshold_met(&self, fl: &[&SignedOp]) -> bool {
        let issuers: BTreeSet<&str> = fl.iter().map(|f| f.body.issuer.as_str()).collect();
        let count = match self.cfg.count {
            FlagCount::DistinctAdmins => issuers.len(),
            FlagCount::AnyFlags => fl.len(),
        };
        let n = self.admins.len();
        if n >= FLAG_THRESHOLD {
            return count >= FLAG_THRESHOLD;
        }
        match self.cfg.small {
            SmallGroup::OwnerAlone => issuers.contains(self.creator.as_str()),
            SmallGroup::ThresholdIsAdminCount => n > 0 && count >= n,
            SmallGroup::NoRemoval => false,
        }
    }
    fn removal_met(&self, fl: &[&SignedOp]) -> bool {
        match self.cfg.expiry {
            Expiry::SignedWindow(win) => fl.iter().any(|start| {
                let t = start.flag_at();
                let sub: Vec<&SignedOp> = fl
                    .iter()
                    .copied()
                    .filter(|f| f.flag_at() >= t && f.flag_at() <= t.saturating_add(win))
                    .collect();
                self.threshold_met(&sub)
            }),
            _ => self.threshold_met(fl),
        }
    }
    // Membership as a pure function of an op set. `live` = (arrival times, receiver clock),
    // used only by the ReceiverClock candidate on the receiver's whole current state.
    fn derive(
        &self,
        ops: &BTreeMap<String, SignedOp>,
        ids: &BTreeSet<String>,
        live: Option<(&BTreeMap<String, u64>, u64)>,
    ) -> Derived {
        let mut withdrawn = BTreeSet::new();
        let mut lifted = BTreeSet::new();
        let mut members = self.base.clone();
        let mut readmitted = BTreeSet::new();
        let mut admit_grants: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for id in ids {
            let Some(o) = ops.get(id) else { continue };
            match &o.body.op {
                Op::Withdraw { flag } => {
                    withdrawn.insert(flag.clone());
                }
                Op::Admit { grant, lifts } => {
                    let h = grant.body.holder.clone();
                    members.insert(h.clone());
                    admit_grants
                        .entry(h.clone())
                        .or_default()
                        .push(grant.digest());
                    if !lifts.is_empty() {
                        readmitted.insert(h);
                        lifted.extend(lifts.iter().cloned());
                    }
                }
                _ => {}
            }
        }
        let mut by_target: BTreeMap<String, Vec<&SignedOp>> = BTreeMap::new();
        let mut flags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for id in ids {
            let Some(o) = ops.get(id) else { continue };
            let Op::Flag { target, .. } = &o.body.op else {
                continue;
            };
            if withdrawn.contains(id) || lifted.contains(id) {
                continue;
            }
            if let (Some((arrived, now)), Expiry::ReceiverClock(ttl)) = (live, self.cfg.expiry) {
                if arrived.get(id).is_none_or(|t| t + ttl <= now) {
                    continue;
                }
            }
            by_target.entry(target.clone()).or_default().push(o);
            flags.entry(target.clone()).or_default().insert(id.clone());
        }
        let removed: BTreeSet<String> = by_target
            .iter()
            .filter(|(_, fl)| self.removal_met(fl))
            .map(|(t, _)| t.clone())
            .collect();
        let mut revoked_grants = BTreeSet::new();
        for h in removed.iter().chain(readmitted.iter()) {
            if let Some(g) = self.grants.get(h) {
                revoked_grants.insert(g.digest());
            }
        }
        for h in &removed {
            for d in admit_grants.get(h).into_iter().flatten() {
                revoked_grants.insert(d.clone());
            }
        }
        let roster = members.difference(&removed).cloned().collect();
        Derived {
            members,
            removed,
            roster,
            revoked_grants,
            flags,
        }
    }
    // Checks that need no group state (signature/group checked by the caller).
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
        match &op.body.op {
            Op::Flag { target, .. } => {
                if *target == self.creator && self.cfg.owner == OwnerRule::Protected {
                    return Err(Refuse::NotFlaggable);
                }
                if *target != self.creator
                    && self.admins.contains(target)
                    && self.cfg.admin == AdminRule::Immune
                {
                    return Err(Refuse::NotFlaggable);
                }
            }
            Op::Withdraw { .. } => {
                if self.cfg.withdrawal == Withdrawal::Final {
                    return Err(Refuse::NotAllowed);
                }
            }
            Op::Admit { grant, lifts } => {
                if !grant.allows(&self.g, &grant.body.holder, Cap::Post) {
                    return Err(Refuse::PermitInvalid);
                }
                if !lifts.is_empty() && self.cfg.rejoin == Rejoin::Forbidden {
                    return Err(Refuse::NotAllowed);
                }
                if !lifts.is_subset(&op.body.seen) {
                    return Err(Refuse::BadLift);
                }
            }
            Op::RemoveHost { .. } => {}
        }
        Ok(())
    }
}

// ---- per-device policy + content state -----------------------------------------------------

struct Replica {
    board: Board,
    hosts: BTreeMap<String, HostRecord>, // "<did>#<slot>" -> accepted host record
    revoked_permits: BTreeSet<String>,
    ops: BTreeMap<String, SignedOp>,
    op_pending: BTreeMap<String, SignedOp>,
    arrived: BTreeMap<String, u64>,
    latched: BTreeSet<String>, // ReceiverClock only
    der: Derived,
    store: BTreeMap<String, Post>, // policy-accepted posts (board = materialisation)
    post_pending: BTreeMap<String, Post>,
    dir: Option<BTreeMap<String, EpNote>>, // hosts only (U7): member group notes
    rotate_due: bool,
}
impl Replica {
    fn new(ctx: &Ctx, host: bool) -> Self {
        Self {
            board: Board::new(ctx.gid.clone()),
            hosts: BTreeMap::new(),
            revoked_permits: BTreeSet::new(),
            ops: BTreeMap::new(),
            op_pending: BTreeMap::new(),
            arrived: BTreeMap::new(),
            latched: BTreeSet::new(),
            der: ctx.derive(&BTreeMap::new(), &BTreeSet::new(), None),
            store: BTreeMap::new(),
            post_pending: BTreeMap::new(),
            dir: host.then(BTreeMap::new),
            rotate_due: false,
        }
    }
    fn snap(&self) -> Vec<u8> {
        serde_json::to_vec(&(
            self.board.snapshot(),
            &self.hosts,
            &self.revoked_permits,
            &self.ops,
            &self.op_pending,
            &self.arrived,
            &self.latched,
            &self.der,
            &self.store,
            &self.post_pending,
            &self.dir,
            self.rotate_due,
        ))
        .unwrap()
    }
    // Delivery-order-independent outcome (no arrival times, no board insertion order).
    fn outcome(&self) -> Vec<u8> {
        serde_json::to_vec(&(
            &self.der,
            self.ops.keys().collect::<Vec<_>>(),
            self.store.keys().collect::<Vec<_>>(),
            self.board.accepted.keys().collect::<Vec<_>>(),
            self.op_pending.keys().collect::<Vec<_>>(),
            self.post_pending.keys().collect::<Vec<_>>(),
            &self.revoked_permits,
        ))
        .unwrap()
    }
    fn accept_op(&mut self, ctx: &Ctx, bytes: &[u8], now: u64) -> Result<Acc, Refuse> {
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
        self.apply_op(id, op, now);
        self.drain_ops(ctx, now);
        self.recompute(ctx, now);
        Ok(Acc::Applied)
    }
    // Every op is authorized at its OWN signed frontier, so the verdict does not depend on
    // delivery order (ops with stale frontiers are the D02-style limitation).
    fn frontier_check(&self, ctx: &Ctx, op: &SignedOp) -> Result<(), Refuse> {
        let fr = closure(&self.ops, &op.body.seen);
        let d = ctx.derive(&self.ops, &fr, None);
        if d.removed.contains(&op.body.issuer) {
            return Err(Refuse::IssuerRemoved);
        }
        match &op.body.op {
            Op::Flag { target, .. } => {
                if !d.roster.contains(target) {
                    return Err(Refuse::NotMember);
                }
            }
            Op::Withdraw { flag } => {
                if !fr.contains(flag) {
                    return Err(Refuse::UnknownFlag);
                }
                let f = &self.ops[flag];
                let Op::Flag { target, .. } = &f.body.op else {
                    return Err(Refuse::UnknownFlag);
                };
                if f.body.issuer != op.body.issuer {
                    return Err(Refuse::NotFlagger);
                }
                if d.removed.contains(target) {
                    return Err(Refuse::AfterThreshold);
                }
            }
            Op::Admit { grant, lifts } => {
                if d.revoked_grants.contains(&grant.digest()) {
                    return Err(Refuse::GrantRevoked);
                }
                let h = &grant.body.holder;
                if lifts.is_empty() {
                    if d.removed.contains(h) {
                        return Err(Refuse::Removed);
                    }
                } else if !d.removed.contains(h)
                    || !lifts.iter().all(|l| {
                        matches!(self.ops.get(l).map(|o| &o.body.op),
                            Some(Op::Flag { target, .. }) if target == h)
                    })
                {
                    return Err(Refuse::BadLift);
                }
            }
            Op::RemoveHost { .. } => {}
        }
        Ok(())
    }
    fn apply_op(&mut self, id: String, op: SignedOp, now: u64) {
        if let Op::RemoveHost { permit } = &op.body.op {
            if self.revoked_permits.insert(permit.clone()) {
                self.hosts.retain(|_, r| r.permit.digest() != *permit);
                self.rotate_due = true; // section 10 Q8: group-level rotation
            }
        }
        self.arrived.insert(id.clone(), now);
        self.ops.insert(id, op);
    }
    fn drain_ops(&mut self, ctx: &Ctx, now: u64) {
        loop {
            let ready = self
                .op_pending
                .iter()
                .find(|(_, o)| o.body.seen.iter().all(|s| self.ops.contains_key(s)))
                .map(|(k, _)| k.clone());
            let Some(id) = ready else { break };
            let op = self.op_pending.remove(&id).unwrap();
            if self.frontier_check(ctx, &op).is_ok() {
                self.apply_op(id, op, now);
            }
        }
    }
    fn tick(&mut self, ctx: &Ctx, now: u64) {
        self.recompute(ctx, now);
    }
    fn recompute(&mut self, ctx: &Ctx, now: u64) {
        let all: BTreeSet<String> = self.ops.keys().cloned().collect();
        let receiver_clock = matches!(ctx.cfg.expiry, Expiry::ReceiverClock(_));
        let live = receiver_clock.then_some((&self.arrived, now));
        let mut d = ctx.derive(&self.ops, &all, live);
        if receiver_clock {
            let lifted: BTreeSet<String> = self
                .ops
                .values()
                .filter_map(|o| match &o.body.op {
                    Op::Admit { grant, lifts } if !lifts.is_empty() => {
                        Some(grant.body.holder.clone())
                    }
                    _ => None,
                })
                .collect();
            self.latched.extend(d.removed.iter().cloned());
            self.latched.retain(|t| !lifted.contains(t));
            d.removed = self.latched.clone();
            d.roster = d.members.difference(&d.removed).cloned().collect();
        }
        self.der = d;
        let removed = self.der.removed.clone();
        if let Some(dir) = self.dir.as_mut() {
            dir.retain(|_, n| !removed.contains(&n.claims.owner));
        }
        let mut dropped = false;
        for id in self.store.keys().cloned().collect::<Vec<_>>() {
            let p = self.store[&id].clone();
            if self.post_ok(ctx, &p).is_err() {
                self.store.remove(&id);
                dropped = true;
            }
        }
        if dropped {
            self.rebuild_board(ctx);
        }
        self.drain_posts(ctx);
    }
    fn post_ok(&self, ctx: &Ctx, p: &Post) -> Result<(), Why> {
        let fr = closure(&self.ops, &p.body.ops_seen);
        let d = ctx.derive(&self.ops, &fr, None);
        if !d.roster.contains(&p.body.author) {
            return Err(Why::NotMember);
        }
        if ctx.cfg.post == PostRule::RemovalWins && self.der.removed.contains(&p.body.author) {
            let pid = p.id();
            let seen_by_a_flag = self.ops.values().any(|o| {
                matches!(&o.body.op, Op::Flag { target, seen_posts, .. }
                    if *target == p.body.author && seen_posts.contains(&pid))
            });
            if !seen_by_a_flag {
                return Err(Why::RemovedConcurrent);
            }
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
        if !self.der.members.contains(&p.body.author) {
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
        let v = self.board.receive(p.clone());
        if matches!(v, Verdict::Accepted | Verdict::Pending) {
            self.store.insert(id, p);
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
    // Real Loro board rebuilt from the surviving policy-accepted posts. A dropped post leaves
    // later posts on the same writer chain stranded in `board.pending` (D01 Q4).
    fn rebuild_board(&mut self, ctx: &Ctx) {
        let mut posts: Vec<Post> = self.store.values().cloned().collect();
        posts.sort_by_key(|p| (p.body.writer, p.body.seq));
        self.board = Board::new(ctx.gid.clone());
        for p in posts {
            self.board.receive(p);
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
        if self.revoked_permits.contains(&r.permit.digest()) {
            return Err(Refuse::Revoked);
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
}

// ---- devices and world (copied from multi_host.rs @0924939, trimmed) -----------------------

struct Device {
    id: Identity,
    slot: u64, // random device ID / route slot; owner-replaceable, never key-derived
    w: Writer,
    rep: Replica,
    group_key: SigningKey,
    my_note: EpNote,
    host: Option<HostRecord>, // own record if this device is a host
    acks: BTreeMap<String, u64>,
    op_out: Vec<SignedOp>,
    outbox: Vec<Post>,
    refused: Vec<(Post, Why)>,
    online: bool,
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
        let ids: Vec<Identity> = (60u8..69).map(ident).collect();
        let owners = [0usize, 1, 1, 2, 3, 4, 5, 6, 7, 8];
        let g = Genesis::create(&ids[0], [9; 32]);
        let gid = g.id();
        let admin_ids: BTreeSet<usize> = cfg.admins.iter().map(|&dv| owners[dv]).collect();
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
            d.push(Device {
                w: Writer::new(id.clone(), writer),
                id,
                slot,
                rep: Replica::new(&ctx, cfg.hosts.contains(&dv)),
                group_key,
                my_note,
                host: None,
                acks: BTreeMap::new(),
                op_out: vec![],
                outbox: vec![],
                refused: vec![],
                online: true,
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
                w.accept_host_record(i, &rec).unwrap();
            }
            for m in MEMBERS {
                let n = w.d[m].my_note.wire();
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
        // Owner appoints every host (no admin-issued permit, so no cascade question here).
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
    fn permit_digest(&self, h: usize) -> String {
        self.d[h].host.as_ref().unwrap().permit.digest()
    }
    fn accept_host_record(&mut self, i: usize, bytes: &[u8]) -> Result<(), Refuse> {
        self.d[i].rep.accept_host_record(&self.ctx, bytes)
    }
    fn host_accept_note(&mut self, h: usize, bytes: &[u8]) -> Result<(), Refuse> {
        self.d[h].rep.accept_note(&self.ctx, bytes)
    }
    fn accept_op(&mut self, i: usize, bytes: &[u8]) -> Result<Acc, Refuse> {
        let now = self.now;
        self.d[i].rep.accept_op(&self.ctx, bytes, now)
    }
    fn accept_post(&mut self, i: usize, p: Post) -> Verdict {
        self.d[i].rep.accept_post(&self.ctx, p)
    }
    // `x` admits a link from host `y`: y presents its current record; x must accept it.
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
    fn op_with(&self, i: usize, nonce: u64, seen: BTreeSet<String>, op: Op) -> SignedOp {
        let grant = self.ctx.grants.get(&self.did(i)).cloned();
        SignedOp::sign(&self.d[i].id, &self.ctx.gid, grant, nonce, seen, op)
    }
    fn op(&mut self, i: usize, op: Op) -> SignedOp {
        let nonce = self.rng.next_u64();
        let seen = self.d[i].rep.ops.keys().cloned().collect();
        self.op_with(i, nonce, seen, op)
    }
    fn flag(&mut self, i: usize, target: &str) -> SignedOp {
        let seen_posts = self.d[i].rep.store.keys().cloned().collect();
        let at = self.now;
        self.op(
            i,
            Op::Flag {
                target: target.into(),
                at,
                seen_posts,
            },
        )
    }
    // Hand-built flag for explored traces: explicit frontier, claimed time and seen posts.
    fn flag_with(
        &self,
        i: usize,
        nonce: u64,
        seen: &[&SignedOp],
        target: usize,
        at: u64,
        seen_posts: &[&Post],
    ) -> SignedOp {
        self.op_with(
            i,
            nonce,
            seen.iter().map(|o| o.id()).collect(),
            Op::Flag {
                target: self.did(target),
                at,
                seen_posts: seen_posts.iter().map(|p| p.id()).collect(),
            },
        )
    }
    fn submit(&mut self, i: usize, op: SignedOp) -> Result<Acc, Refuse> {
        let r = self.accept_op(i, &op.wire());
        self.d[i].op_out.push(op);
        self.run_rotations();
        r
    }
    fn flag_and_submit(&mut self, i: usize, target: usize) -> SignedOp {
        let t = self.did(target);
        let f = self.flag(i, &t);
        assert_eq!(self.submit(i, f.clone()), Ok(Acc::Applied), "flag by {i}");
        f
    }
    // ---- content ----
    fn post(&mut self, m: usize, text: &str) -> Post {
        let dv = &mut self.d[m];
        dv.w.observe(&dv.rep.board);
        let seen = dv.rep.ops.keys().cloned().collect();
        let p = dv.w.post(&self.ctx.gid, text, seen);
        dv.outbox.push(p.clone());
        p
    }
    // ---- links ----
    fn connect(&mut self, m: usize, h: usize) -> Result<Path, RouteErr> {
        let r = self.connect_inner(m, h);
        self.run_rotations();
        r
    }
    // Member/host link: member dials the host it knows, else the host dials the member's
    // directory endpoint. An honest host answers a removed member's dial with the signed
    // flags (U5 revised) and opens no session.
    fn connect_inner(&mut self, m: usize, h: usize) -> Result<Path, RouteErr> {
        if !self.d[m].online {
            return Err(RouteErr::SelfOffline);
        }
        if !self.d[h].online {
            return Err(RouteErr::PeerOffline);
        }
        let hkey = self.hk(h);
        let member_side = self.d[m]
            .rep
            .hosts
            .get(&hkey)
            .map(|r| r.note.claims.endpoint.clone())
            .and_then(|e| self.dial(m, &e).ok())
            == Some(h);
        if self.d[h].host.is_some() && self.d[h].rep.der.removed.contains(&self.did(m)) {
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
        self.run_rotations();
        Ok(Path::HostDialedMember)
    }
    // The notice is the signed flags themselves (+ their causal frontier) — W2-M shape.
    fn removal_notice(&mut self, m: usize, h: usize) {
        let me = self.did(m);
        let fl = self.d[h]
            .rep
            .der
            .flags
            .get(&me)
            .cloned()
            .unwrap_or_default();
        let ids = closure(&self.d[h].rep.ops, &fl);
        for id in ids {
            let o = self.d[h].rep.ops[&id].clone();
            self.send(h, m, "removal-notice", o.wire());
            let _ = self.accept_op(m, &o.wire());
        }
    }
    fn exchange(&mut self, m: usize, h: usize) {
        let (hkey, mkey) = (self.hk(h), self.hk(m));
        // member -> host
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
        // host -> member
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
        self.run_rotations();
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
    // ---- rotation ----
    // Section 10 Q8: a device that newly accepts a host removal rotates its group endpoint
    // once. Members send the new note only to hosts still permitted in their view; remaining
    // hosts rotate and reconnect members themselves. The removed host never rotates here.
    fn run_rotations(&mut self) {
        while let Some(i) = (0..self.d.len()).find(|&i| self.d[i].rep.rotate_due) {
            self.d[i].rep.rotate_due = false;
            let revoked_self = self.d[i]
                .host
                .as_ref()
                .is_some_and(|r| self.d[i].rep.revoked_permits.contains(&r.permit.digest()));
            if revoked_self || !self.d[i].online {
                continue;
            }
            if self.d[i].host.is_some() {
                self.rotate_host(i);
            } else {
                self.rotate_member(i);
            }
        }
    }
    // Copied from multi_host.rs @0924939 (no retention): note to every permitted host.
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
            let Some(e) = self.d[m]
                .rep
                .hosts
                .get(&self.hk(h))
                .map(|r| r.note.claims.endpoint.clone())
            else {
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
    // Copied from multi_host.rs @0924939 `rotate_host`; changed: after handing over the new
    // record the host runs a full exchange, so the revocation op reaches each member too.
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
    // Revocation wave used by the host-revocation tests: the owner signs `RemoveHost` and
    // hands it to a legitimate host, which rotates and pushes it to every member it reaches.
    fn revoke_host(&mut self, issuer: usize, via: usize, gone: usize) -> SignedOp {
        let permit = self.permit_digest(gone);
        let op = self.op(issuer, Op::RemoveHost { permit });
        assert_eq!(self.submit(issuer, op.clone()), Ok(Acc::Applied));
        self.connect(issuer, via).unwrap();
        op
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
}
fn world() -> World {
    World::new(Cfg::default())
}
fn with(f: impl FnOnce(&mut Cfg)) -> World {
    let mut cfg = Cfg::default();
    f(&mut cfg);
    World::new(cfg)
}
fn everyone(w: &World) -> Vec<usize> {
    let mut v = MEMBERS.to_vec();
    v.extend(w.hosts.iter().copied());
    v
}

// ---- delivery-order exploration (pattern from policy_candidates.rs `explore`) --------------

#[derive(Clone)]
enum Ev {
    Op(SignedOp),
    Post(Post),
}
fn deliver(rep: &mut Replica, ctx: &Ctx, ev: &Ev, now: u64) {
    match ev {
        Ev::Op(o) => {
            let _ = rep.accept_op(ctx, &o.wire(), now);
        }
        Ev::Post(p) => {
            let _ = rep.accept_post(ctx, p.clone());
        }
    }
}
fn permutations(n: usize) -> Vec<Vec<usize>> {
    fn go(prefix: &mut Vec<usize>, rest: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if rest.is_empty() {
            out.push(prefix.clone());
            return;
        }
        for i in 0..rest.len() {
            let x = rest.remove(i);
            prefix.push(x);
            go(prefix, rest, out);
            prefix.pop();
            rest.insert(i, x);
        }
    }
    let mut out = vec![];
    go(&mut vec![], &mut (0..n).collect(), &mut out);
    out
}
struct Explored {
    orders: usize,
    finals: BTreeSet<Vec<u8>>,
    retro_post: bool,    // some order showed a post accepted, then dropped it
    retro_removal: bool, // some order showed a DID removed, then not removed
    last: Replica,
}
fn explore(ctx: &Ctx, events: &[Ev]) -> Explored {
    let (mut finals, mut retro_post, mut retro_removal) = (BTreeSet::new(), false, false);
    let mut last = None;
    let orders = permutations(events.len());
    for order in &orders {
        let mut r = Replica::new(ctx, false);
        let (mut shown, mut removed) = (BTreeSet::new(), BTreeSet::new());
        for &k in order {
            deliver(&mut r, ctx, &events[k], 0);
            shown.extend(r.board.accepted.keys().cloned());
            removed.extend(r.der.removed.iter().cloned());
        }
        retro_post |= shown.iter().any(|p| !r.board.accepted.contains_key(p));
        retro_removal |= removed.iter().any(|d| !r.der.removed.contains(d));
        finals.insert(r.outcome());
        last = Some(r);
    }
    Explored {
        orders: orders.len(),
        finals,
        retro_post,
        retro_removal,
        last: last.unwrap(),
    }
}
fn set(xs: &[&str]) -> BTreeSet<String> {
    xs.iter().map(|s| s.to_string()).collect()
}
fn ids(ops: &[&SignedOp]) -> BTreeSet<String> {
    ops.iter().map(|o| o.id()).collect()
}

// ---- tests ---------------------------------------------------------------------------------

// KIND: invariant
// U9 (decided): admins flag a member; at 3 flags the member is removed and its grant revoked.
// Here the 3 flags come from 3 distinct admins, so this holds under either `FlagCount`
// candidate. Flags are DID-signed ops that EVERY device verifies and applies itself. After 2
// flags no roster changes anywhere. After the 3rd, every device derives the same state:
// members, both hosts, and carol herself once told. The hosts drop her group endpoint. Her
// revoked grant cannot be re-presented in an Admit: GrantRevoked, state byte-identical.
#[test]
fn three_flags_from_distinct_admins_remove_member_and_revoke_grant_everywhere() {
    let mut w = world();
    let carol = w.did(CAROL);
    w.post(CAROL, "carol before");
    w.round();
    w.flag_and_submit(A1, CAROL);
    w.flag_and_submit(BOB, CAROL);
    w.round();
    for i in everyone(&w) {
        let der = &w.d[i].rep.der;
        assert!(der.roster.contains(&carol), "device {i}");
        assert!(der.removed.is_empty(), "device {i}");
        assert_eq!(der.flags[&carol].len(), 2, "device {i}");
    }
    w.flag_and_submit(DAVE, CAROL);
    w.round();
    let grant = w.ctx.grants[&carol].clone();
    let first = w.d[OLGA].rep.der.clone();
    for i in everyone(&w) {
        assert_eq!(w.d[i].rep.der, first, "device {i}");
    }
    assert!(first.removed.contains(&carol) && !first.roster.contains(&carol));
    assert!(first.revoked_grants.contains(&grant.digest()));
    assert_eq!(first.flags[&carol].len(), 3);
    for h in [H1, H2] {
        let dir = w.d[h].rep.dir.as_ref().unwrap();
        assert!(dir.values().all(|n| n.claims.owner != carol), "host {h}");
    }
    let readmit = w.op(
        OLGA,
        Op::Admit {
            grant,
            lifts: BTreeSet::new(),
        },
    );
    for i in everyone(&w) {
        let before = w.snap(i);
        assert_eq!(
            w.accept_op(i, &readmit.wire()),
            Err(Refuse::GrantRevoked),
            "device {i}"
        );
        assert_eq!(w.snap(i), before, "device {i}");
    }
    w.assert_converged(&[OLGA, A1, A2, BOB, DAVE, ERIN, H1, H2]);
}

// KIND: candidate-policy G-AUTHORITY
// Section 8 Q1: does "3 flags" mean 3 distinct admin DIDs (the lean) or any 3 flags? Alice
// flags carol three times from two devices: A1 twice and A2 once, all under one DID.
// - DistinctAdmins: this counts as 1 admin, so carol stays. Bob and dave then complete the
//   removal.
// - AnyFlags: one admin removes carol alone, just by repeating the flag.
#[test]
fn same_admin_flagging_three_times_distinct_admins_vs_any_flags() {
    for count in [FlagCount::DistinctAdmins, FlagCount::AnyFlags] {
        let mut w = with(|c| c.count = count);
        let carol = w.did(CAROL);
        w.flag_and_submit(A1, CAROL);
        w.flag_and_submit(A1, CAROL);
        w.flag_and_submit(A2, CAROL);
        w.round();
        for i in everyone(&w) {
            let der = &w.d[i].rep.der;
            assert_eq!(der.flags[&carol].len(), 3, "{count:?} device {i}");
            assert_eq!(
                der.removed.contains(&carol),
                count == FlagCount::AnyFlags,
                "{count:?} device {i}"
            );
        }
        if count == FlagCount::DistinctAdmins {
            w.flag_and_submit(BOB, CAROL);
            w.flag_and_submit(DAVE, CAROL);
            w.round();
            for i in everyone(&w) {
                assert!(w.d[i].rep.der.removed.contains(&carol), "device {i}");
            }
        }
    }
}

// KIND: invariant
// Flags commute. Removal is a pure function of the verified op set, and each op is
// authorized at its own signed frontier.
// (a) Three admin flags and two posts, delivered to a fresh replica in all 120 orders, give
//     ONE final outcome. A re-delivered flag is a Duplicate, and the state is unchanged.
// (b) Two partitioned sides: H1 with alice's flag (and olga, carol, erin), and H2 with bob's
//     and dave's flags. Neither side removes carol, since one side has 1 flag and the other
//     has 2. After H1-H2 heal, host sync carries the ops. Every device then derives the same
//     state, and carol is removed.
#[test]
fn flags_commute_across_delivery_orders_and_two_partitioned_hosts() {
    let w = world();
    let carol = w.did(CAROL);
    let fa = w.flag_with(A1, 1, &[], CAROL, 0, &[]);
    let fb = w.flag_with(BOB, 2, &[], CAROL, 0, &[]);
    let fd = w.flag_with(DAVE, 3, &[], CAROL, 0, &[]);
    let mut cw = Writer::new(w.d[CAROL].id.clone(), 0xC0C0);
    let q = cw.post(&w.ctx.gid, "carol", BTreeSet::new());
    let mut ew = Writer::new(w.d[ERIN].id.clone(), 0xE0E0);
    let p = ew.post(&w.ctx.gid, "erin", BTreeSet::new());
    let evs = [
        Ev::Op(fa.clone()),
        Ev::Op(fb),
        Ev::Op(fd),
        Ev::Post(q),
        Ev::Post(p),
    ];
    let mut x = explore(&w.ctx, &evs);
    assert_eq!(x.orders, 120);
    assert_eq!(x.finals.len(), 1);
    assert!(!x.retro_post && !x.retro_removal);
    assert!(x.last.der.removed.contains(&carol));
    assert_eq!(x.last.board.texts(), set(&["carol", "erin"]));
    let before = x.last.snap();
    assert_eq!(x.last.accept_op(&w.ctx, &fa.wire(), 0), Ok(Acc::Duplicate));
    assert_eq!(x.last.snap(), before);

    let mut w = world();
    w.cut(H1, H2);
    for m in [OLGA, A1, A2, CAROL, ERIN] {
        w.cut(m, H2);
    }
    for m in [BOB, DAVE] {
        w.cut(m, H1);
    }
    w.flag_and_submit(A1, CAROL);
    w.flag_and_submit(BOB, CAROL);
    w.flag_and_submit(DAVE, CAROL);
    w.round();
    assert_eq!(w.d[H1].rep.der.flags[&carol].len(), 1);
    assert_eq!(w.d[H2].rep.der.flags[&carol].len(), 2);
    for i in everyone(&w) {
        assert!(w.d[i].rep.der.removed.is_empty(), "device {i}");
    }
    w.heal(H1, H2);
    w.round();
    let first = w.d[OLGA].rep.der.clone();
    assert!(first.removed.contains(&carol));
    for i in everyone(&w) {
        assert_eq!(w.d[i].rep.der, first, "device {i}");
    }
}

// KIND: invariant
// Flags are neither host fiat nor forgeable. Every device refuses each of the following, and
// its full state stays byte-identical:
// - malformed bytes;
// - a flag signed by another key that names alice as issuer (forged);
// - alice's real flag with its target swapped after signing;
// - a flag by member erin under her member grant;
// - a flag by erin under a self-issued "admin" grant;
// - a flag by host binary hugo (H1), presenting its {Host} permit as the issuer grant;
// - a flag by hugo with no grant;
// - a flag for another group;
// - a flag on a DID that is not a member.
// Positive control: alice's genuine flag is applied everywhere.
#[test]
fn forged_non_admin_host_only_wrong_group_and_non_member_flags_are_refused_unchanged() {
    let mut w = world();
    let carol = w.did(CAROL);
    let gid = w.ctx.gid.clone();
    let alice = w.d[A1].id.clone();
    let alice_grant = w.ctx.grants[&w.did(A1)].clone();
    let erin = w.d[ERIN].id.clone();
    let erin_grant = w.ctx.grants[&w.did(ERIN)].clone();
    let hugo = w.d[H1].id.clone();
    let host_permit = w.d[H1].host.as_ref().unwrap().permit.clone();
    let mallory = ident(77);
    let flag = |target: &str| Op::Flag {
        target: target.into(),
        at: 0,
        seen_posts: BTreeSet::new(),
    };
    let none = BTreeSet::new;
    let forged = {
        let body = OpBody {
            group: gid.clone(),
            issuer: alice.did().into(),
            issuer_grant: Some(alice_grant.clone()),
            nonce: 1,
            seen: none(),
            op: flag(&carol),
        };
        let sig = mallory.sign(&signed(OP_DOMAIN, &body)).to_vec();
        SignedOp { body, sig }
    };
    let mut tampered = SignedOp::sign(
        &alice,
        &gid,
        Some(alice_grant.clone()),
        2,
        none(),
        flag(&w.did(ERIN)),
    );
    tampered.body.op = flag(&carol);
    let self_admin = MGrant::issue(
        &erin,
        &gid,
        erin.did(),
        None,
        &[Cap::Read, Cap::Post, Cap::Delegate, Cap::Admin],
        true,
        4,
        None,
    );
    let cases: Vec<(&str, Vec<u8>, Refuse)> = vec![
        ("malformed", b"{not an op".to_vec(), Refuse::Malformed),
        ("forged issuer", forged.wire(), Refuse::BadSignature),
        ("target swapped", tampered.wire(), Refuse::BadSignature),
        (
            "member erin",
            SignedOp::sign(&erin, &gid, Some(erin_grant), 3, none(), flag(&carol)).wire(),
            Refuse::NotAdmin,
        ),
        (
            "self-issued admin grant",
            SignedOp::sign(&erin, &gid, Some(self_admin), 5, none(), flag(&carol)).wire(),
            Refuse::NotAdmin,
        ),
        (
            "host permit as issuer grant",
            SignedOp::sign(&hugo, &gid, Some(host_permit), 6, none(), flag(&carol)).wire(),
            Refuse::NotAdmin,
        ),
        (
            "host without grant",
            SignedOp::sign(&hugo, &gid, None, 7, none(), flag(&carol)).wire(),
            Refuse::NotAdmin,
        ),
        (
            "other group",
            SignedOp::sign(
                &alice,
                "other-group",
                Some(alice_grant.clone()),
                8,
                none(),
                flag(&carol),
            )
            .wire(),
            Refuse::WrongGroup,
        ),
        (
            "non-member target",
            SignedOp::sign(
                &alice,
                &gid,
                Some(alice_grant.clone()),
                9,
                none(),
                flag(mallory.did()),
            )
            .wire(),
            Refuse::NotMember,
        ),
    ];
    for i in everyone(&w) {
        for (name, bytes, want) in &cases {
            let before = w.snap(i);
            assert_eq!(w.accept_op(i, bytes), Err(*want), "{name} at {i}");
            assert_eq!(w.snap(i), before, "{name} at {i}");
        }
    }
    let good = SignedOp::sign(&alice, &gid, Some(alice_grant), 10, none(), flag(&carol));
    for i in everyone(&w) {
        assert_eq!(w.accept_op(i, &good.wire()), Ok(Acc::Applied), "device {i}");
        assert_eq!(w.d[i].rep.der.flags[&carol].len(), 1);
    }
}

// KIND: candidate-policy G-AUTHORITY
// Section 8 Q2: what happens in a group with fewer than 3 admins? Here only olga (the owner)
// and alice are admins. Counting the owner as an admin is itself an open question. Bob is a
// plain member, so his flag is refused (NotAdmin, state unchanged).
// - OwnerAlone: the owner's single flag removes; alice's flag alone does not.
// - ThresholdIsAdminCount (= 2): removal needs both olga and alice.
// - NoRemoval: carol is never removed, even with every admin's flag.
#[test]
fn fewer_than_three_admins_owner_alone_vs_admin_count_vs_no_removal() {
    for small in [
        SmallGroup::OwnerAlone,
        SmallGroup::ThresholdIsAdminCount,
        SmallGroup::NoRemoval,
    ] {
        let mk = || {
            with(|c| {
                c.admins = vec![A1];
                c.small = small;
            })
        };
        let mut w = mk();
        let carol = w.did(CAROL);
        assert_eq!(w.ctx.admins.len(), 2);
        let by_bob = w.flag(BOB, &carol);
        for i in everyone(&w) {
            let before = w.snap(i);
            assert_eq!(w.accept_op(i, &by_bob.wire()), Err(Refuse::NotAdmin));
            assert_eq!(w.snap(i), before);
        }
        // Owner's flag alone.
        w.flag_and_submit(OLGA, CAROL);
        w.round();
        for i in everyone(&w) {
            let removed = w.d[i].rep.der.removed.contains(&carol);
            assert_eq!(removed, small == SmallGroup::OwnerAlone, "{small:?} at {i}");
        }
        // Alice first, then the owner.
        let mut w = mk();
        w.flag_and_submit(A1, CAROL);
        w.round();
        for i in everyone(&w) {
            assert!(w.d[i].rep.der.removed.is_empty(), "{small:?} at {i}");
        }
        w.flag_and_submit(OLGA, CAROL);
        w.round();
        for i in everyone(&w) {
            let der = &w.d[i].rep.der;
            assert_eq!(der.flags[&carol].len(), 2);
            assert_eq!(
                der.removed.contains(&carol),
                small != SmallGroup::NoRemoval,
                "{small:?} at {i}"
            );
            assert_eq!(der.roster.contains(&carol), small == SmallGroup::NoRemoval);
        }
    }
}

// KIND: candidate-policy G-AUTHORITY
// Section 8 Q3: can the owner/creator be flagged?
// - Protected (the lean): each admin's flag on olga is refused (NotFlaggable), state unchanged.
// - Flaggable: three admins remove olga. Her later ops are refused (IssuerRemoved, state
//   unchanged), and so are her posts. But the grants and host permits she issued still
//   verify, because chains root at the genesis creator key, not at a "current owner". So
//   alice's flag on erin is applied, and the hosts olga appointed still relay. Whether
//   removing the owner should cascade, or transfer ownership, is undecided.
#[test]
fn owner_protected_vs_flaggable() {
    for owner in [OwnerRule::Protected, OwnerRule::Flaggable] {
        let mut w = with(|c| c.owner = owner);
        let olga = w.did(OLGA);
        let erin = w.did(ERIN);
        if owner == OwnerRule::Protected {
            for a in [A1, BOB, DAVE] {
                let f = w.flag(a, &olga);
                for i in everyone(&w) {
                    let before = w.snap(i);
                    assert_eq!(w.accept_op(i, &f.wire()), Err(Refuse::NotFlaggable));
                    assert_eq!(w.snap(i), before);
                }
            }
            continue;
        }
        for a in [A1, BOB, DAVE] {
            w.flag_and_submit(a, OLGA);
        }
        w.round();
        for i in everyone(&w) {
            assert!(w.d[i].rep.der.removed.contains(&olga), "device {i}");
        }
        let late = w.flag(OLGA, &erin);
        let p = w.post(OLGA, "owner after removal");
        for i in everyone(&w) {
            let before = w.snap(i);
            assert_eq!(w.accept_op(i, &late.wire()), Err(Refuse::IssuerRemoved));
            assert_eq!(
                w.accept_post(i, p.clone()),
                Verdict::Rejected(Why::NotMember)
            );
            assert_eq!(w.snap(i), before, "device {i}");
        }
        w.flag_and_submit(A1, ERIN);
        w.post(BOB, "hosts appointed by the removed owner still relay");
        w.round();
        for i in everyone(&w) {
            if i != OLGA {
                assert_eq!(w.d[i].rep.der.flags[&erin].len(), 1, "device {i}");
            }
        }
        assert!(
            w.d[CAROL]
                .rep
                .board
                .texts()
                .contains("hosts appointed by the removed owner still relay")
        );
    }
}

// KIND: candidate-policy G-AUTHORITY
// Section 8 Q4a: can an admin withdraw a flag before the threshold?
// - Final: a withdrawal op is refused (NotAllowed), state unchanged.
// - BeforeThreshold: alice flags, bob flags, alice withdraws, dave flags. That leaves 2
//   effective flags, so carol stays. Two withdrawals are refused, state unchanged:
//   withdrawing someone else's flag (NotFlagger), and withdrawing once the removal is in the
//   withdrawer's frontier (AfterThreshold).
// - "Before" can only be judged at the withdrawal's own signed frontier. So a withdrawal
//   CONCURRENT with the 3rd flag cancels a removal that replicas receiving the 3rd flag first
//   already showed. Over all 24 orders there is one final outcome (not removed), but some
//   orders show a retroactive un-removal.
#[test]
fn flag_withdrawal_final_vs_before_threshold() {
    let mut w = world();
    let fa = w.flag_and_submit(A1, CAROL);
    w.round();
    let wd = w.op(A1, Op::Withdraw { flag: fa.id() });
    for i in everyone(&w) {
        let before = w.snap(i);
        assert_eq!(w.accept_op(i, &wd.wire()), Err(Refuse::NotAllowed));
        assert_eq!(w.snap(i), before);
    }

    let mut w = with(|c| c.withdrawal = Withdrawal::BeforeThreshold);
    let carol = w.did(CAROL);
    let fa = w.flag_and_submit(A1, CAROL);
    let fb = w.flag_and_submit(BOB, CAROL);
    w.round();
    let wd = w.op(A1, Op::Withdraw { flag: fa.id() });
    assert_eq!(w.submit(A1, wd), Ok(Acc::Applied));
    w.round();
    let not_mine = w.op(DAVE, Op::Withdraw { flag: fb.id() });
    for i in everyone(&w) {
        let before = w.snap(i);
        assert_eq!(w.accept_op(i, &not_mine.wire()), Err(Refuse::NotFlagger));
        assert_eq!(w.snap(i), before);
    }
    w.flag_and_submit(DAVE, CAROL);
    w.round();
    for i in everyone(&w) {
        let der = &w.d[i].rep.der;
        assert!(!der.removed.contains(&carol), "device {i}");
        assert_eq!(der.flags[&carol].len(), 2, "device {i}");
    }
    w.flag_and_submit(A1, CAROL); // alice flags again
    w.round();
    for i in everyone(&w) {
        assert!(w.d[i].rep.der.removed.contains(&carol), "device {i}");
    }
    let too_late = w.op(BOB, Op::Withdraw { flag: fb.id() });
    for i in everyone(&w) {
        let before = w.snap(i);
        assert_eq!(
            w.accept_op(i, &too_late.wire()),
            Err(Refuse::AfterThreshold)
        );
        assert_eq!(w.snap(i), before);
    }

    let w = with(|c| c.withdrawal = Withdrawal::BeforeThreshold);
    let fa = w.flag_with(A1, 1, &[], CAROL, 0, &[]);
    let fb = w.flag_with(BOB, 2, &[], CAROL, 0, &[]);
    let fd = w.flag_with(DAVE, 3, &[], CAROL, 0, &[]);
    let wd = w.op_with(A1, 4, ids(&[&fa, &fb]), Op::Withdraw { flag: fa.id() });
    let mut first_flags_then_withdraw = Replica::new(&w.ctx, false);
    for o in [&fa, &fb, &fd] {
        first_flags_then_withdraw
            .accept_op(&w.ctx, &o.wire(), 0)
            .unwrap();
    }
    assert!(first_flags_then_withdraw.der.removed.contains(&carol));
    first_flags_then_withdraw
        .accept_op(&w.ctx, &wd.wire(), 0)
        .unwrap();
    assert!(!first_flags_then_withdraw.der.removed.contains(&carol));
    let x = explore(&w.ctx, &[Ev::Op(fa), Ev::Op(fb), Ev::Op(fd), Ev::Op(wd)]);
    assert_eq!(x.orders, 24);
    assert_eq!(x.finals.len(), 1);
    assert!(!x.last.der.removed.contains(&carol));
    assert!(x.retro_removal);
}

// KIND: candidate-policy G-TIME
// Section 8 Q4b: do flags expire? The flags carry claimed times 0, 50 and 300.
// - Never: carol is removed.
// - SignedWindow(100): the three flags never fall within 100 ticks of each other, so carol
//   stays. But `at` is chosen by the signer: dave re-signs his flag claiming at=10, and it
//   counts.
// - ReceiverClock(100): replica X gets alice's flag at t=0 and the other two at t=200.
//   Replica Y gets all three at t=200. With the same op set but different arrival times, Y
//   removes carol and X never does, because alice's flag expired at X before the 3rd
//   arrived. The divergence is permanent.
#[test]
fn flag_expiry_never_vs_signed_window_vs_receiver_clock() {
    for expiry in [
        Expiry::Never,
        Expiry::SignedWindow(100),
        Expiry::ReceiverClock(100),
    ] {
        let w = with(|c| c.expiry = expiry);
        let carol = w.did(CAROL);
        let fa = w.flag_with(A1, 1, &[], CAROL, 0, &[]);
        let fb = w.flag_with(BOB, 2, &[], CAROL, 50, &[]);
        let fd = w.flag_with(DAVE, 3, &[], CAROL, 300, &[]);
        let mut y = Replica::new(&w.ctx, false);
        for f in [&fa, &fb, &fd] {
            y.accept_op(&w.ctx, &f.wire(), 200).unwrap();
        }
        match expiry {
            Expiry::Never => assert!(y.der.removed.contains(&carol)),
            Expiry::SignedWindow(_) => {
                assert!(!y.der.removed.contains(&carol));
                let backdated = w.flag_with(DAVE, 4, &[], CAROL, 10, &[]);
                y.accept_op(&w.ctx, &backdated.wire(), 300).unwrap();
                assert!(y.der.removed.contains(&carol));
            }
            Expiry::ReceiverClock(_) => {
                assert!(y.der.removed.contains(&carol));
                let mut x = Replica::new(&w.ctx, false);
                x.accept_op(&w.ctx, &fa.wire(), 0).unwrap();
                x.accept_op(&w.ctx, &fb.wire(), 200).unwrap();
                x.accept_op(&w.ctx, &fd.wire(), 200).unwrap();
                assert_eq!(
                    x.ops.keys().collect::<Vec<_>>(),
                    y.ops.keys().collect::<Vec<_>>()
                );
                assert!(!x.der.removed.contains(&carol));
                x.tick(&w.ctx, 1_000);
                y.tick(&w.ctx, 1_000);
                assert!(!x.der.removed.contains(&carol));
                assert!(y.der.removed.contains(&carol), "latched");
            }
        }
    }
}

// KIND: invariant
// After a removal (U9 + U7 + U5 revised), every honest host has dropped carol's group
// endpoint. Each one refuses, with state unchanged at every host and member: her link, her
// posts citing her current frontier, and her new rotation note. A host can no longer dial her
// (NoAddress). She is told explicitly: on her next dial, an honest host returns the three
// signed flags, and she verifies them herself. A repeat attempt gets the same notice, and her
// state does not change.
#[test]
fn removed_member_refused_by_every_honest_host_endpoint_dropped_and_told_with_signed_flags() {
    let mut w = world();
    let carol = w.did(CAROL);
    w.post(CAROL, "carol before");
    w.round();
    w.cut(CAROL, H1);
    w.cut(CAROL, H2);
    for a in [A1, BOB, DAVE] {
        w.flag_and_submit(a, CAROL);
    }
    w.round();
    let carol_ep = w.gep(CAROL);
    for h in [H1, H2] {
        assert!(w.d[h].rep.der.removed.contains(&carol));
        let dir = w.d[h].rep.dir.as_ref().unwrap();
        assert!(dir.values().all(|n| n.claims.owner != carol));
        assert!(!has(&w.snap(h), carol_ep.as_bytes()), "host {h}");
    }
    assert!(!w.d[CAROL].rep.der.removed.contains(&carol), "not told yet");
    w.heal(CAROL, H1);
    let n = w.frames.len();
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    let notices: Vec<&Frame> = w.frames[n..]
        .iter()
        .filter(|f| f.to == CAROL && f.kind == "removal-notice")
        .collect();
    assert_eq!(notices.len(), 3);
    assert!(notices.iter().all(|f| f.from == H1));
    assert!(w.d[CAROL].rep.der.removed.contains(&carol));
    assert_eq!(w.d[CAROL].rep.der.flags[&carol].len(), 3);
    let after = w.snap(CAROL);
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    assert_eq!(w.snap(CAROL), after, "same notice again, no change");

    let p = w.post(CAROL, "carol after notice");
    let note = EpNote::sign(
        &w.d[CAROL].id,
        &w.ctx.gid,
        w.d[CAROL].slot,
        "grp-carol-rotated",
        2,
    );
    for i in everyone(&w) {
        if i == CAROL {
            continue;
        }
        let before = w.snap(i);
        assert_eq!(
            w.accept_post(i, p.clone()),
            Verdict::Rejected(Why::NotMember),
            "device {i}"
        );
        if w.d[i].host.is_some() {
            assert_eq!(w.host_accept_note(i, &note.wire()), Err(Refuse::NotMember));
            assert_eq!(w.connect_from_host(i, CAROL), Err(RouteErr::NoAddress));
        }
        assert_eq!(w.snap(i), before, "device {i}");
    }
}

// Shared D01/D02 trace for the two post-rule candidates. Carol's writer: q1 (seen by alice's
// flag), q2 (honest, concurrent: frontier before any flag), then the three flags, then q3
// (written after removal but citing the old, empty frontier = backdated) and q4 (cites the
// removal).
struct D01 {
    q: [Post; 4],
    f: [SignedOp; 3],
}
fn d01(post: PostRule) -> (World, D01) {
    let w = with(|c| c.post = post);
    let gid = w.ctx.gid.clone();
    let mut cw = Writer::new(w.d[CAROL].id.clone(), 0xC0C0);
    let q1 = cw.post(&gid, "q1 seen by alice", BTreeSet::new());
    let q2 = cw.post(&gid, "q2 honest offline", BTreeSet::new());
    let fa = w.flag_with(A1, 1, &[], CAROL, 0, &[&q1]);
    let fb = w.flag_with(BOB, 2, &[], CAROL, 0, &[]);
    let fd = w.flag_with(DAVE, 3, &[], CAROL, 0, &[]);
    let q3 = cw.post(&gid, "q3 backdated", BTreeSet::new());
    let q4 = cw.post(&gid, "q4 cites removal", ids(&[&fa, &fb, &fd]));
    (
        w,
        D01 {
            q: [q1, q2, q3, q4],
            f: [fa, fb, fd],
        },
    )
}
impl D01 {
    fn events(&self) -> Vec<Ev> {
        let mut v: Vec<Ev> = self.q[..3].iter().cloned().map(Ev::Post).collect();
        v.extend(self.f.iter().cloned().map(Ev::Op));
        v
    }
}

// KIND: candidate-policy D01
// Section 8 Q8 asks about posts around the 3rd flag (= D01/D02). This test applies the
// SIGNED FRONTIER rule. Over all 720 orders of q1..q3 and the three flags, there is one
// outcome: q1, q2 and q3 are all kept, never retroactively.
// - q3 is backdated: its signed fields are shaped exactly like honest q2's, so it is kept.
//   That is the D02 limitation of this rule.
// - q4 cites the removal and is refused, state unchanged.
// - World part: in the U7 topology a removed member cannot upload at all, because honest
//   hosts refuse her link. Her honest offline post therefore never reaches the group, even
//   though any replica handed that post accepts it under this rule.
#[test]
fn posts_around_third_flag_signed_frontier_keeps_concurrent_and_backdated_posts() {
    let (w, t) = d01(PostRule::SignedFrontier);
    let carol = w.did(CAROL);
    let mut x = explore(&w.ctx, &t.events());
    assert_eq!(x.orders, 720);
    assert_eq!(x.finals.len(), 1);
    assert!(!x.retro_post);
    assert!(x.last.der.removed.contains(&carol));
    assert_eq!(
        x.last.board.texts(),
        set(&["q1 seen by alice", "q2 honest offline", "q3 backdated"])
    );
    assert_eq!(t.q[1].body.ops_seen, t.q[2].body.ops_seen);
    let before = x.last.snap();
    assert_eq!(
        x.last.accept_post(&w.ctx, t.q[3].clone()),
        Verdict::Rejected(Why::NotMember)
    );
    assert_eq!(x.last.snap(), before);

    let mut w = world();
    w.cut(CAROL, H1);
    w.cut(CAROL, H2);
    let offline = w.post(CAROL, "written offline before the flags");
    for a in [A1, BOB, DAVE] {
        w.flag_and_submit(a, CAROL);
    }
    w.round();
    w.heal(CAROL, H1);
    w.heal(CAROL, H2);
    assert_eq!(w.connect(CAROL, H1), Err(RouteErr::Removed));
    w.round();
    assert_eq!(w.d[CAROL].outbox.len(), 1, "still queued, never uploaded");
    for i in everyone(&w) {
        if i != CAROL {
            let texts = w.d[i].rep.board.texts();
            assert!(!texts.contains("written offline before the flags"), "{i}");
        }
    }
    assert_eq!(w.accept_post(H1, offline), Verdict::Accepted);
}

// KIND: candidate-policy D01
// Section 8 Q8 (= D01/D02), under the REMOVAL WINS rule. Here a removed author's post is
// kept only if some flag had seen it. Over all 720 orders there is one outcome: only q1 is
// kept, and q2 and q3 are dropped. The backdated q3 is refused, but so is honest q2.
// Some orders show q2 and then drop it (retroactive). Replaying one such order shows it:
// q1 and q2 are shown, the flags arrive, q2 disappears, and the board is rebuilt on real
// Loro. q3 is then refused (RemovedConcurrent) and q4 (NotMember), state unchanged.
#[test]
fn posts_around_third_flag_removal_wins_drops_unseen_posts_retroactively() {
    let (w, t) = d01(PostRule::RemovalWins);
    let carol = w.did(CAROL);
    let x = explore(&w.ctx, &t.events());
    assert_eq!(x.orders, 720);
    assert_eq!(x.finals.len(), 1);
    assert!(x.retro_post);
    assert!(x.last.der.removed.contains(&carol));
    assert_eq!(x.last.board.texts(), set(&["q1 seen by alice"]));

    let mut r = Replica::new(&w.ctx, false);
    assert_eq!(r.accept_post(&w.ctx, t.q[0].clone()), Verdict::Accepted);
    assert_eq!(r.accept_post(&w.ctx, t.q[1].clone()), Verdict::Accepted);
    assert_eq!(r.board.accepted.len(), 2);
    for f in &t.f {
        r.accept_op(&w.ctx, &f.wire(), 0).unwrap();
    }
    assert_eq!(r.board.texts(), set(&["q1 seen by alice"]));
    let before = r.snap();
    assert_eq!(
        r.accept_post(&w.ctx, t.q[2].clone()),
        Verdict::Rejected(Why::RemovedConcurrent)
    );
    assert_eq!(
        r.accept_post(&w.ctx, t.q[3].clone()),
        Verdict::Rejected(Why::NotMember)
    );
    assert_eq!(r.snap(), before);
}

// KIND: candidate-policy G-AUTHORITY
// Section 8 Q7: can a removed DID be invited back?
// - Allowed: alice signs an Admit with a NEW grant that lifts the three flags; it must cite
//   them in its frontier. Over all 120 orders there is one outcome, even though the
//   readmission may arrive before the flags it lifts (it then waits pending). Carol is a
//   member again, and her post citing the readmission is kept. Her OLD grant stays revoked
//   and the new one is live. Re-presenting the old grant is refused (GrantRevoked).
// - Forbidden: the same readmission is refused (NotAllowed), state unchanged. Carol stays
//   removed. Her post citing the readmission stays pending forever, because its frontier
//   never becomes known.
// Under both rules, a plain Admit (no lifts) of a removed DID is refused (Removed), state
// unchanged.
#[test]
fn rejoin_after_removal_allowed_vs_forbidden() {
    for rejoin in [Rejoin::Allowed, Rejoin::Forbidden] {
        let w = with(|c| c.rejoin = rejoin);
        let carol = w.did(CAROL);
        let gid = w.ctx.gid.clone();
        let fa = w.flag_with(A1, 1, &[], CAROL, 0, &[]);
        let fb = w.flag_with(BOB, 2, &[], CAROL, 0, &[]);
        let fd = w.flag_with(DAVE, 3, &[], CAROL, 0, &[]);
        let lifts = ids(&[&fa, &fb, &fd]);
        let alice_grant = w.ctx.grants[&w.did(A1)].clone();
        let new_grant = MGrant::issue(
            &w.d[A1].id,
            &gid,
            &carol,
            None,
            &[Cap::Read, Cap::Post],
            false,
            77,
            Some(alice_grant),
        );
        let readmit = w.op_with(
            A1,
            5,
            lifts.clone(),
            Op::Admit {
                grant: new_grant.clone(),
                lifts: lifts.clone(),
            },
        );
        let plain = w.op_with(
            OLGA,
            6,
            lifts.clone(),
            Op::Admit {
                grant: new_grant.clone(),
                lifts: BTreeSet::new(),
            },
        );
        let old = w.op_with(
            A1,
            7,
            lifts.clone(),
            Op::Admit {
                grant: w.ctx.grants[&carol].clone(),
                lifts: lifts.clone(),
            },
        );
        let mut cw = Writer::new(w.d[CAROL].id.clone(), 0xC0C1);
        let back = cw.post(&gid, "carol is back", ids(&[&readmit]));

        let mut r = Replica::new(&w.ctx, false);
        for f in [&fa, &fb, &fd] {
            r.accept_op(&w.ctx, &f.wire(), 0).unwrap();
        }
        assert!(r.der.removed.contains(&carol));
        let before = r.snap();
        assert_eq!(r.accept_op(&w.ctx, &plain.wire(), 0), Err(Refuse::Removed));
        assert_eq!(r.snap(), before);
        let evs = [
            Ev::Op(fa.clone()),
            Ev::Op(fb.clone()),
            Ev::Op(fd.clone()),
            Ev::Op(readmit.clone()),
            Ev::Post(back.clone()),
        ];
        match rejoin {
            Rejoin::Allowed => {
                assert_eq!(
                    r.accept_op(&w.ctx, &old.wire(), 0),
                    Err(Refuse::GrantRevoked)
                );
                assert_eq!(r.snap(), before);
                let x = explore(&w.ctx, &evs);
                assert_eq!(x.orders, 120);
                assert_eq!(x.finals.len(), 1);
                let der = &x.last.der;
                assert!(!der.removed.contains(&carol) && der.roster.contains(&carol));
                assert!(der.revoked_grants.contains(&w.ctx.grants[&carol].digest()));
                assert!(!der.revoked_grants.contains(&new_grant.digest()));
                assert_eq!(x.last.board.texts(), set(&["carol is back"]));
            }
            Rejoin::Forbidden => {
                assert_eq!(
                    r.accept_op(&w.ctx, &readmit.wire(), 0),
                    Err(Refuse::NotAllowed)
                );
                assert_eq!(r.snap(), before);
                let x = explore(&w.ctx, &evs);
                assert_eq!(x.finals.len(), 1);
                assert!(x.last.der.removed.contains(&carol));
                assert!(x.last.store.is_empty());
                assert!(x.last.post_pending.contains_key(&back.id()));
            }
        }
    }
}

// KIND: candidate-policy G-AUTHORITY
// Section 8 Q9: can admins flag other admins?
// - Immune: flags on alice by olga, bob and dave are refused (NotFlaggable), state unchanged.
// - Flaggable: alice first flags erin. Then olga, bob and dave remove alice, and both of her
//   devices lose membership and admin power. Her later flag, whose frontier includes her
//   removal, is refused (IssuerRemoved), state unchanged, and so is her post. Her earlier
//   flag on erin still counts: with bob's and dave's flags, it removes erin. A flag alice
//   signs with a stale frontier (one that does not cite her removal) is still applied. The
//   D02 backdating limitation applies to ops too.
#[test]
fn admins_flagging_admins_immune_vs_flaggable() {
    for rule in [AdminRule::Immune, AdminRule::Flaggable] {
        let mut w = with(|c| c.admin = rule);
        let alice = w.did(A1);
        let erin = w.did(ERIN);
        if rule == AdminRule::Immune {
            for a in [OLGA, BOB, DAVE] {
                let f = w.flag(a, &alice);
                for i in everyone(&w) {
                    let before = w.snap(i);
                    assert_eq!(w.accept_op(i, &f.wire()), Err(Refuse::NotFlaggable));
                    assert_eq!(w.snap(i), before);
                }
            }
            continue;
        }
        let earlier = w.flag_and_submit(A1, ERIN);
        w.round();
        for a in [OLGA, BOB, DAVE] {
            w.flag_and_submit(a, A1);
        }
        w.round();
        for i in everyone(&w) {
            let der = &w.d[i].rep.der;
            assert!(der.removed.contains(&alice) && !der.roster.contains(&alice));
        }
        let late = w.flag(A2, &erin);
        let p = w.post(A2, "alice after removal");
        for i in everyone(&w) {
            if i == A2 {
                continue;
            }
            let before = w.snap(i);
            assert_eq!(w.accept_op(i, &late.wire()), Err(Refuse::IssuerRemoved));
            assert_eq!(
                w.accept_post(i, p.clone()),
                Verdict::Rejected(Why::NotMember)
            );
            assert_eq!(w.snap(i), before, "device {i}");
        }
        let stale = w.op_with(
            A1,
            99,
            ids(&[&earlier]),
            Op::Flag {
                target: erin.clone(),
                at: 0,
                seen_posts: BTreeSet::new(),
            },
        );
        assert_eq!(w.accept_op(H1, &stale.wire()), Ok(Acc::Applied));
        w.flag_and_submit(BOB, ERIN);
        w.flag_and_submit(DAVE, ERIN);
        w.round();
        for i in everyone(&w) {
            if i != A1 && i != A2 {
                assert!(w.d[i].rep.der.removed.contains(&erin), "device {i}");
            }
        }
    }
}

// KIND: invariant
// Section 10 Q8 (decided): group-level rotation on host revocation. There are three hosts.
// 1. The owner signs RemoveHost for H3 and hands it to the legitimate host H1.
// 2. H1 rotates, and itself reconnects every member, delivering the signed revocation.
// 3. H2 gets the revocation by host sync.
// 4. Each member and remaining host rotates its group endpoint exactly once, and sends the
//    new note only to permitted hosts.
// Afterwards every address H3 ever knew is dead, and H3 receives no frame at all. It holds
// none of the new endpoints and none of the new content. Its record, its links and its host
// sync are all refused, with state unchanged.
#[test]
fn host_revocation_rotates_the_group_and_revoked_host_reaches_nobody() {
    let mut w = with(|c| c.hosts = vec![H1, H2, H3]);
    w.post(ERIN, "before revocation");
    w.round();
    let mut h3_knows: BTreeSet<String> = w.d[H3]
        .rep
        .dir
        .as_ref()
        .unwrap()
        .values()
        .map(|n| n.claims.endpoint.clone())
        .collect();
    let h3_key = w.hk(H3);
    h3_knows.extend(
        w.d[H3]
            .rep
            .hosts
            .iter()
            .filter(|(k, _)| **k != h3_key)
            .map(|(_, r)| r.note.claims.endpoint.clone()),
    );
    assert_eq!(h3_knows.len(), MEMBERS.len() + 2);
    assert!(h3_knows.iter().all(|e| w.dial(H3, e).is_ok()));
    let permit = w.permit_digest(H3);
    let n = w.frames.len();
    w.revoke_host(OLGA, H1, H3);
    w.round();
    for i in everyone(&w) {
        if i == H3 {
            continue;
        }
        let rep = &w.d[i].rep;
        assert!(rep.revoked_permits.contains(&permit), "device {i}");
        assert!(!rep.hosts.contains_key(&h3_key), "device {i}");
        assert_eq!(w.d[i].my_note.claims.seq, 2, "device {i} rotated once");
    }
    for e in &h3_knows {
        assert_eq!(w.dial(H3, e), Err(RouteErr::EndpointGone));
    }
    assert!(w.frames[n..].iter().all(|f| f.to != H3));
    assert!(
        w.frames[n..]
            .iter()
            .filter(|f| f.kind == "ep-note")
            .all(|f| f.to == H1 || f.to == H2)
    );
    let hay = w.received(H3);
    for i in 0..w.d.len() {
        if i != H3 {
            assert!(
                !has(&hay, w.gep(i).as_bytes()),
                "H3 holds new endpoint of {i}"
            );
        }
    }
    let rec = w.d[H3].host.as_ref().unwrap().wire();
    for m in MEMBERS {
        let before = w.snap(m);
        assert_eq!(w.accept_host_record(m, &rec), Err(Refuse::Revoked));
        assert_eq!(w.connect(m, H3), Err(RouteErr::Lost));
        assert_eq!(w.connect_from_host(H3, m), Err(RouteErr::EndpointGone));
        assert_eq!(w.snap(m), before);
    }
    assert!(w.host_sync(H1, H3).is_err());
    assert!(w.host_sync(H2, H3).is_err());
    w.post(BOB, "after revocation");
    w.round();
    w.assert_converged(&[OLGA, A1, A2, BOB, DAVE, CAROL, ERIN, H1, H2]);
    assert!(w.d[CAROL].rep.board.texts().contains("after revocation"));
    assert!(!w.d[H3].rep.board.texts().contains("after revocation"));
    assert!(w.d[H3].rep.board.texts().contains("before revocation"));
}

// KIND: limitation
// Two limits of section 10 Q8:
// (1) Content the revoked host already held stays with it.
// (2) A member that has not yet been reached still trusts H3. Erin is cut from H1 and H2:
//     H3 can still dial her unchanged endpoint, and her new post goes to H3 only. That post
//     reaches no other device, and it is not re-sent after she learns of the revocation, so
//     it is stranded at the revoked host. Once H1 reaches her, she rotates and H3 loses her
//     address.
#[test]
fn host_revocation_held_content_and_unreached_member_keeps_using_revoked_host() {
    let mut w = with(|c| c.hosts = vec![H1, H2, H3]);
    w.post(BOB, "held before revocation");
    w.round();
    w.cut(ERIN, H1);
    w.cut(ERIN, H2);
    w.revoke_host(OLGA, H1, H3);
    w.round();
    assert!(w.d[H3].rep.board.texts().contains("held before revocation"));
    let erin_old = w.gep(ERIN);
    assert_eq!(w.d[ERIN].my_note.claims.seq, 1);
    assert!(w.d[ERIN].rep.hosts.contains_key(&w.hk(H3)));
    assert_eq!(w.dial(H3, &erin_old), Ok(ERIN));
    w.post(ERIN, "erin to the revoked host");
    assert_eq!(w.connect(ERIN, H3), Ok(Path::MemberDialedHost));
    assert!(
        w.d[H3]
            .rep
            .board
            .texts()
            .contains("erin to the revoked host")
    );
    w.heal(ERIN, H1);
    assert_eq!(w.connect(ERIN, H1), Ok(Path::HostDialedMember));
    w.round();
    assert_eq!(w.d[ERIN].my_note.claims.seq, 2);
    assert!(!w.d[ERIN].rep.hosts.contains_key(&w.hk(H3)));
    assert_eq!(w.dial(H3, &erin_old), Err(RouteErr::EndpointGone));
    assert!(!has(&w.received(H3), w.gep(ERIN).as_bytes()));
    for i in [OLGA, A1, A2, BOB, DAVE, CAROL, H1, H2] {
        let texts = w.d[i].rep.board.texts();
        assert!(!texts.contains("erin to the revoked host"), "device {i}");
    }
}

// KIND: limitation
// Section 10 Q8 does not cover an UNDETECTED rogue host. After H3's revocation and the group
// rotation, suppose H2 is also hostile but nobody has revoked it. It still holds every
// member's new group endpoint and every new post. Rotation shuts out only the hosts that were
// revoked.
#[test]
fn undetected_rogue_host_still_learns_every_new_endpoint_after_group_rotation() {
    let mut w = with(|c| c.hosts = vec![H1, H2, H3]);
    w.revoke_host(OLGA, H1, H3);
    w.round();
    w.post(CAROL, "after rotation");
    w.round();
    let hay = w.received(H2);
    for m in MEMBERS {
        assert_eq!(w.d[m].my_note.claims.seq, 2);
        assert!(
            has(&hay, w.gep(m).as_bytes()),
            "H2 holds new endpoint of {m}"
        );
    }
    assert!(w.d[H2].rep.board.texts().contains("after rotation"));
}
