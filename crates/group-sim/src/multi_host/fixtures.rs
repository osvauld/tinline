use super::*;

pub(super) const POST_DOMAIN: &[u8] = b"tinline/group-sim/multi-host-post/not-production/v1\0";
pub(super) const NOTE_DOMAIN: &[u8] = b"tinline/group-sim/multi-host-endpoint-note/not-production/v1\0";
pub(super) const GRANT_DOMAIN: &[u8] = b"tinline/group-sim/multi-host-grant/not-production/v1\0";
pub(super) const OP_DOMAIN: &[u8] = b"tinline/group-sim/multi-host-op/not-production/v1\0";
pub(super) const NOTICE_DOMAIN: &[u8] = b"tinline/group-sim/multi-host-block-notice/not-production/v1\0";
pub(super) const DOC: &str = "group/w2m/2026-10-10";
pub(super) const MAX_PENDING_POSTS: usize = 16;
pub(super) const MAX_CHAIN: usize = 8;
// U9: 3 flags from 3 DISTINCT admins (section 8 Q1/Q5 lean; threshold not decided).
pub(super) const REMOVAL_FLAGS: usize = 3;

// Devices. olga = owner/creator; alice (2 devices) and bob = admins; carol = member;
// hana = member whose desktop H1 is a host and whose phone HP is an ordinary member device;
// hugo (H2) and ivan (H3) = separate host binaries with their own DIDs (not members).
pub(super) const OLGA: usize = 0;
pub(super) const A1: usize = 1;
pub(super) const A2: usize = 2;
pub(super) const BOB: usize = 3;
pub(super) const CAROL: usize = 4;
pub(super) const HP: usize = 5;
pub(super) const H1: usize = 6;
pub(super) const H2: usize = 7;
pub(super) const H3: usize = 8;
pub(super) const MEMBERS: [usize; 6] = [OLGA, A1, A2, BOB, CAROL, HP];

// ---- helpers -------------------------------------------------------------------------------

pub(super) fn hex32(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
pub(super) fn has(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}
pub(super) fn ident(seed: u8) -> Identity {
    let mnemonic = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
    identity::recover(&mnemonic.to_string()).unwrap()
}
pub(super) fn ep(prefix: &str, k: &SigningKey) -> String {
    format!("{prefix}-{}", hex32(&k.verifying_key().to_bytes()))
}
pub(super) fn signed<T: Serialize>(domain: &[u8], body: &T) -> Vec<u8> {
    [domain, &serde_json::to_vec(body).unwrap()].concat()
}
pub(super) fn valid(did: &str, bytes: &[u8], sig: &[u8]) -> bool {
    let (Some(key), Ok(sig)) = (public_key_from_did(did), <[u8; 64]>::try_from(sig)) else {
        return false;
    };
    verify(&key, bytes, &sig)
}
pub(super) fn digest<T: Serialize>(x: &T) -> String {
    blake3::hash(&serde_json::to_vec(x).unwrap())
        .to_hex()
        .to_string()
}
pub(super) fn dir_key(did: &str, slot: u64) -> String {
    format!("{did}#{slot}")
}
pub(super) fn pair(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

// Copied from host_only.rs @dcdf4a4. Deterministic SplitMix64: the ONLY source of writer IDs,
// route slots (device IDs) and transport keys. Never seeded by key material.
pub(super) struct SimRng(pub(super) u64);
impl SimRng {
    pub(super) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub(super) fn bytes32(&mut self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for c in out.chunks_mut(8) {
            c.copy_from_slice(&self.next_u64().to_le_bytes());
        }
        out
    }
    pub(super) fn writer(&mut self) -> u64 {
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
pub(super) enum Cap {
    Read,
    Post,
    Delegate,
    Admin,
    Host,
    // G6 (`group_call.rs`): call-host permit leaf; NOT implied by `Host` (spec §2.1, R05).
    CallHost,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MGrantBody {
    pub(super) version: u8,
    pub(super) group: String,
    pub(super) issuer: String,
    pub(super) holder: String,
    pub(super) device: Option<u64>, // host permits only: the ONE nominated hosting device (route slot)
    pub(super) caps: BTreeSet<Cap>,
    pub(super) delegable: bool,
    pub(super) nonce: u64, // fixture stand-in for appointment order
    pub(super) parent: Option<Box<MGrant>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MGrant {
    pub(super) body: MGrantBody,
    pub(super) sig: Vec<u8>,
}
impl MGrant {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn issue(
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
    pub(super) fn digest(&self) -> String {
        digest(self)
    }
    // Copied walk from authority.rs `Grant::allows`; added: a device-bound grant (a host
    // permit) can never be a parent.
    pub(super) fn allows(&self, g: &Genesis, holder: &str, cap: Cap) -> bool {
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
pub(super) struct PostBody {
    pub(super) group: String,
    pub(super) doc: String,
    pub(super) author: String,
    pub(super) writer: u64,
    pub(super) seq: u64,
    pub(super) text: String,
    pub(super) update: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Post {
    pub(super) body: PostBody,
    pub(super) sig: Vec<u8>,
}
impl Post {
    pub(super) fn sign(id: &Identity, body: PostBody) -> Self {
        let sig = id.sign(&signed(POST_DOMAIN, &body)).to_vec();
        Self { body, sig }
    }
    pub(super) fn verify(&self) -> bool {
        valid(
            &self.body.author,
            &signed(POST_DOMAIN, &self.body),
            &self.sig,
        )
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) enum Why {
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
pub(super) enum Verdict {
    Accepted,
    Duplicate,
    Pending,
    Rejected(Why),
}
pub(super) struct Board {
    pub(super) group: String,
    pub(super) roster: BTreeSet<String>,
    pub(super) doc: LoroDoc,
    pub(super) accepted: BTreeMap<String, Post>,
    pub(super) pending: BTreeMap<String, Post>,
    pub(super) writers: BTreeMap<u64, String>,
    pub(super) last_seq: BTreeMap<u64, u64>,
    pub(super) order: Vec<String>,
}
impl Board {
    pub(super) fn new(group: String, roster: BTreeSet<String>) -> Self {
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
    pub(super) fn precheck(&self, p: &Post) -> Result<(), Why> {
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
    pub(super) fn receive(&mut self, p: Post) -> Verdict {
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
    pub(super) fn snapshot(&self) -> Vec<u8> {
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
    pub(super) fn texts(&self) -> BTreeSet<String> {
        self.accepted
            .values()
            .map(|p| p.body.text.clone())
            .collect()
    }
}

// ---- DID-signed endpoint notes (copied from host_only.rs @dcdf4a4) -------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Audience {
    Group(String),
    Private,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct NoteClaims {
    pub(super) owner: String,
    pub(super) audience: Audience,
    pub(super) slot: u64,
    pub(super) endpoint: String,
    pub(super) seq: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct EpNote {
    pub(super) claims: NoteClaims,
    pub(super) sig: Vec<u8>,
}
impl EpNote {
    pub(super) fn sign(id: &Identity, audience: Audience, slot: u64, endpoint: &str, seq: u64) -> Self {
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
    pub(super) fn verify(&self) -> bool {
        valid(
            &self.claims.owner,
            &signed(NOTE_DOMAIN, &self.claims),
            &self.sig,
        )
    }
    pub(super) fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}
// What a host presents to members and other hosts: its DID-signed note + its permit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct HostRecord {
    pub(super) note: EpNote,
    pub(super) permit: MGrant,
}
impl HostRecord {
    pub(super) fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}

// ---- DID-signed group ops and block notice -------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Op {
    Admit { grant: MGrant },
    Flag { target: String },
    RemoveHost { permit: String }, // permit digest
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct OpBody {
    pub(super) group: String,
    pub(super) issuer: String,
    pub(super) issuer_grant: Option<MGrant>, // None only for the group creator
    pub(super) nonce: u64,
    pub(super) op: Op,
}
