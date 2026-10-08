//! One conversation day as a Loro doc, and the rules every device applies to every batch.
//!
//! `messages` is a map `id -> { id, author, at, text, edited_at?, deleted?, reply_to?, file? }`.
//! A device writes only with its own Loro peer id (`peer_id`), and a received batch is judged
//! deterministically from its own content and the receiver's copy of the same doc: the
//! receiver imports it into a scratch fork and checks what changed (see `Shard::apply_remote`).

use std::collections::BTreeMap;

use loro::{ExportMode, LoroDoc, LoroMap, LoroValue, VersionVector};
use serde::{Deserialize, Serialize};

pub const HOUR_MS: i64 = 3_600_000;
pub const DAY_MS: i64 = 24 * HOUR_MS;
pub const MAX_TEXT: usize = 20_000;
const MAX_FILE_JSON: usize = 4096;

/// `pair` for two DIDs: hex BLAKE3 of the sorted, NUL-joined DIDs.
pub fn pair_id(a: &str, b: &str) -> String {
    let (x, y) = if a <= b { (a, b) } else { (b, a) };
    let mut h = blake3::Hasher::new();
    h.update(x.as_bytes());
    h.update(&[0]);
    h.update(y.as_bytes());
    h.finalize().to_hex().to_string()
}

pub fn doc_name(pair: &str, day: &str) -> String {
    format!("dm/{pair}/{day}")
}

/// The Loro peer id of `device` for the doc `name`.
pub fn peer_id(device: &[u8; 32], name: &str) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(b"tinline-loro-peer-v1");
    h.update(device);
    h.update(name.as_bytes());
    let out = h.finalize();
    u64::from_le_bytes(out.as_bytes()[..8].try_into().unwrap())
}

// ---- UTC days ------------------------------------------------------------------------------

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The UTC day (`YYYY-MM-DD`) of a unix-millisecond time.
pub fn day_of(ms: i64) -> String {
    let (y, m, d) = civil_from_days(ms.div_euclid(DAY_MS));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Start of a day in unix ms; `None` if `day` is not `YYYY-MM-DD`.
pub fn day_start(day: &str) -> Option<i64> {
    let b = day.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y: i64 = day[..4].parse().ok()?;
    let m: i64 = day[5..7].parse().ok()?;
    let d: i64 = day[8..].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let ms = days_from_civil(y, m, d) * DAY_MS;
    (day_of(ms) == day).then_some(ms)
}

// ---- records -------------------------------------------------------------------------------

/// What a message carries besides text. Stored as one JSON string, immutable after creation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    /// BLAKE3 of the ciphertext, hex.
    pub hash: String,
    /// The blob's AES-256 key, hex.
    pub key: String,
    pub name: String,
    pub size: u64,
    pub mime: String,
    /// "file" or "voice".
    pub kind: String,
    #[serde(default)]
    pub duration_ms: u32,
    #[serde(default)]
    pub waveform: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsgRec {
    pub id: String,
    pub author: String,
    /// Unix ms.
    pub at: i64,
    pub text: String,
    pub edited_at: Option<i64>,
    pub deleted: bool,
    pub reply_to: Option<String>,
    pub file: Option<FileRef>,
    /// The author's device-key signature over everything above plus the doc id (`msg_sig_input`).
    pub sig: Vec<u8>,
}

const MSG_SIG_DOMAIN: &[u8] = b"tinline-chat-msg-v1";

fn put_str(v: &mut Vec<u8>, s: &str) {
    v.extend_from_slice(&(s.len() as u32).to_be_bytes());
    v.extend_from_slice(s.as_bytes());
}

/// Canonical bytes a message signature covers: doc id (so a message cannot be replayed into
/// another day or pair), id, author, time, reply, file and the current text/edit/delete state.
pub fn msg_sig_input(doc: &str, m: &MsgRec) -> Vec<u8> {
    let mut v = MSG_SIG_DOMAIN.to_vec();
    put_str(&mut v, doc);
    put_str(&mut v, &m.id);
    put_str(&mut v, &m.author);
    v.extend_from_slice(&m.at.to_be_bytes());
    put_str(&mut v, &m.text);
    match m.edited_at {
        None => v.push(0),
        Some(t) => {
            v.push(1);
            v.extend_from_slice(&t.to_be_bytes());
        }
    }
    v.push(m.deleted as u8);
    match &m.reply_to {
        None => v.push(0),
        Some(r) => {
            v.push(1);
            put_str(&mut v, r);
        }
    }
    match &m.file {
        None => v.push(0),
        Some(f) => {
            v.push(1);
            put_str(&mut v, &f.hash);
            put_str(&mut v, &f.key);
            put_str(&mut v, &f.name);
            v.extend_from_slice(&f.size.to_be_bytes());
            put_str(&mut v, &f.mime);
            put_str(&mut v, &f.kind);
            v.extend_from_slice(&f.duration_ms.to_be_bytes());
            v.extend_from_slice(&(f.waveform.len() as u32).to_be_bytes());
            v.extend_from_slice(&f.waveform);
        }
    }
    v
}

pub fn sign_msg(secret: &[u8; 32], doc: &str, m: &MsgRec) -> Vec<u8> {
    cryptography::signature::sign(secret, &msg_sig_input(doc, m)).to_vec()
}

pub fn verify_msg(device: &[u8; 32], doc: &str, m: &MsgRec) -> bool {
    let Ok(sig) = <[u8; 64]>::try_from(m.sig.as_slice()) else { return false };
    cryptography::signature::verify(device, &msg_sig_input(doc, m), &sig)
}

impl MsgRec {
    /// Display order: send time, then id.
    pub fn order(&self) -> (i64, &str) {
        (self.at, &self.id)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Reject {
    #[error("batch does not decode")]
    Undecodable,
    #[error("batch holds ops from another peer id")]
    ForeignPeer,
    #[error("batch is not self-contained (pending ops)")]
    Pending,
    #[error("unexpected structure in the doc")]
    Structure,
    #[error("message {0} is malformed")]
    Malformed(String),
    #[error("author is not the signer")]
    WrongAuthor,
    #[error("message {0} belongs to someone else")]
    NotYours(String),
    #[error("an immutable field of {0} changed")]
    Immutable(String),
    #[error("a message was removed")]
    Removed,
    #[error("timestamp out of range")]
    OutOfRange,
    #[error("signature does not verify")]
    BadSignature,
}

/// What a valid batch changed, for events and unread counts.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Applied {
    pub added: Vec<String>,
    pub changed: Vec<String>,
}

impl Applied {
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty()
    }
}

/// How strictly to judge a batch.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub signer_did: String,
    pub signer_peer: u64,
    pub now_ms: i64,
    /// A live push: `at` must also be within 24 h of now.
    pub live: bool,
    /// A history snapshot the peer vouches for: ops of both parties are accepted, authors must
    /// be `signer_did` or `me_did`.
    pub history: Option<String>,
    /// Attested device keys by DID: whoever authored a new or changed message must have signed
    /// it with one of these. (For our own DID: our own devices.)
    pub keys: Vec<(String, [u8; 32])>,
}

fn read_i64(v: &LoroValue) -> Option<i64> {
    match v {
        LoroValue::I64(n) => Some(*n),
        _ => None,
    }
}

fn read_str(v: &LoroValue) -> Option<String> {
    match v {
        LoroValue::String(s) => Some(s.to_string()),
        _ => None,
    }
}

/// Reads `messages` out of a deep value; `Structure` if anything else is in the doc.
fn read_messages(doc: &LoroDoc) -> Result<BTreeMap<String, MsgRec>, Reject> {
    let root = doc.get_deep_value();
    let LoroValue::Map(top) = root else { return Err(Reject::Structure) };
    let mut out = BTreeMap::new();
    for (name, v) in top.iter() {
        if name != "messages" {
            return Err(Reject::Structure);
        }
        let LoroValue::Map(msgs) = v else { return Err(Reject::Structure) };
        for (id, m) in msgs.iter() {
            let LoroValue::Map(m) = m else { return Err(Reject::Malformed(id.clone())) };
            let bad = || Reject::Malformed(id.clone());
            for k in m.keys() {
                if !matches!(k.as_str(), "id" | "author" | "at" | "text" | "edited_at" | "deleted" | "reply_to" | "file" | "sig") {
                    return Err(bad());
                }
            }
            let get = |k: &str| m.get(k);
            let rec = MsgRec {
                id: get("id").and_then(read_str).ok_or_else(bad)?,
                author: get("author").and_then(read_str).ok_or_else(bad)?,
                at: get("at").and_then(read_i64).ok_or_else(bad)?,
                text: get("text").and_then(read_str).ok_or_else(bad)?,
                edited_at: match get("edited_at") {
                    None => None,
                    Some(v) => Some(read_i64(v).ok_or_else(bad)?),
                },
                deleted: match get("deleted") {
                    None => false,
                    Some(LoroValue::Bool(b)) => *b,
                    Some(_) => return Err(bad()),
                },
                reply_to: match get("reply_to") {
                    None => None,
                    Some(v) => Some(read_str(v).ok_or_else(bad)?),
                },
                sig: get("sig").and_then(read_str).and_then(|h| hex::decode(h).ok()).filter(|b| b.len() == 64).ok_or_else(bad)?,
                file: match get("file") {
                    None => None,
                    Some(v) => {
                        let s = read_str(v).ok_or_else(bad)?;
                        if s.len() > MAX_FILE_JSON {
                            return Err(bad());
                        }
                        Some(serde_json::from_str::<FileRef>(&s).map_err(|_| bad())?)
                    }
                },
            };
            if rec.id != *id
                || rec.text.len() > MAX_TEXT
                || rec.author.len() > 200
                || rec.id.is_empty()
                || rec.id.len() > 64
                || rec.reply_to.as_ref().is_some_and(|r| r.len() > 64)
                || rec.file.as_ref().is_some_and(|f| f.waveform.len() > 256 || f.name.len() > 300)
                || (rec.deleted && !rec.text.is_empty())
            {
                return Err(bad());
            }
            out.insert(id.clone(), rec);
        }
    }
    Ok(out)
}

/// A conversation day. Cheap to clone? No: owns the Loro doc.
pub struct Shard {
    pub day: String,
    doc: LoroDoc,
    name: String,
    pub peer: u64,
}

impl Shard {
    pub fn new(pair: &str, day: &str, device: &[u8; 32]) -> Self {
        let name = doc_name(pair, day);
        let peer = peer_id(device, &name);
        let doc = LoroDoc::new();
        doc.set_peer_id(peer).expect("peer id");
        Self { day: day.to_string(), doc, name, peer }
    }

    /// Rebuilds a shard from its snapshot and the updates appended since.
    pub fn load(pair: &str, day: &str, device: &[u8; 32], snapshot: Option<&[u8]>, updates: &[Vec<u8>]) -> Result<Self, Reject> {
        let s = Self::new(pair, day, device);
        if let Some(b) = snapshot {
            s.doc.import(b).map_err(|_| Reject::Undecodable)?;
        }
        for u in updates {
            s.doc.import(u).map_err(|_| Reject::Undecodable)?;
        }
        s.doc.set_peer_id(s.peer).expect("peer id");
        Ok(s)
    }

    pub fn vv(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    /// Our own device's op count (the tick marker for what we wrote so far).
    pub fn my_counter(&self) -> i32 {
        self.vv().get(&self.peer).copied().unwrap_or(0)
    }

    pub fn snapshot(&self) -> Vec<u8> {
        self.doc.export(ExportMode::Snapshot).expect("snapshot export")
    }

    pub fn messages(&self) -> Result<BTreeMap<String, MsgRec>, Reject> {
        read_messages(&self.doc)
    }

    fn msgs_map(&self) -> LoroMap {
        self.doc.get_map("messages")
    }

    /// Runs a local write and returns the update it produced.
    fn write(&self, f: impl FnOnce(&LoroMap) -> loro::LoroResult<()>) -> Result<Vec<u8>, Reject> {
        let before = self.vv();
        f(&self.msgs_map()).map_err(|_| Reject::Structure)?;
        self.doc.commit();
        self.doc.export(ExportMode::updates(&before)).map_err(|_| Reject::Undecodable)
    }

    /// Adds a message; returns the update to persist and push.
    pub fn add_message(&self, m: &MsgRec, secret: &[u8; 32]) -> Result<Vec<u8>, Reject> {
        let sig = hex::encode(sign_msg(secret, &self.name, m));
        self.write(|map| {
            let e = map.insert_container(&m.id, LoroMap::new())?;
            e.insert("id", m.id.as_str())?;
            e.insert("author", m.author.as_str())?;
            e.insert("at", m.at)?;
            e.insert("text", m.text.as_str())?;
            e.insert("sig", sig.as_str())?;
            if let Some(r) = &m.reply_to {
                e.insert("reply_to", r.as_str())?;
            }
            if let Some(f) = &m.file {
                e.insert("file", serde_json::to_string(f).expect("file json"))?;
            }
            Ok(())
        })
    }

    fn entry(&self, id: &str) -> Result<LoroMap, Reject> {
        match self.msgs_map().get(id) {
            Some(loro::ValueOrContainer::Container(loro::Container::Map(m))) => Ok(m),
            _ => Err(Reject::Structure),
        }
    }

    /// Re-signs `id` after `change` was applied to its record.
    fn resign(&self, id: &str, secret: &[u8; 32], change: impl FnOnce(&mut MsgRec)) -> Result<Vec<u8>, Reject> {
        let e = self.entry(id)?;
        let mut rec = self.messages()?.remove(id).ok_or(Reject::Structure)?;
        change(&mut rec);
        let sig = hex::encode(sign_msg(secret, &self.name, &rec));
        self.write(|_| {
            e.insert("text", rec.text.as_str())?;
            e.insert("deleted", rec.deleted)?;
            e.insert("edited_at", rec.edited_at.unwrap_or(0))?;
            e.insert("sig", sig.as_str())?;
            Ok(())
        })
    }

    pub fn edit(&self, id: &str, text: &str, now_ms: i64, secret: &[u8; 32]) -> Result<Vec<u8>, Reject> {
        self.resign(id, secret, |r| {
            r.text = text.to_string();
            r.edited_at = Some(now_ms);
        })
    }

    pub fn delete(&self, id: &str, now_ms: i64, secret: &[u8; 32]) -> Result<Vec<u8>, Reject> {
        self.resign(id, secret, |r| {
            r.text = String::new();
            r.deleted = true;
            r.edited_at = Some(now_ms);
        })
    }

    /// Only our own ops beyond `theirs`; `None` if they hold all of them. A device sends no
    /// one else's ops, so a receiver can insist on a single peer id per batch.
    pub fn export_own_since(&self, theirs: &VersionVector) -> Option<Vec<u8>> {
        let mine = self.vv();
        let my_end = mine.get(&self.peer).copied().unwrap_or(0);
        if theirs.get(&self.peer).copied().unwrap_or(0) >= my_end {
            return None;
        }
        let mut from = theirs.clone();
        for (p, c) in mine.iter() {
            if *p != self.peer {
                from.insert(*p, *c);
            }
        }
        self.doc.export(ExportMode::updates(&from)).ok()
    }

    /// Validates `update` against this shard and, if it is acceptable in every respect,
    /// imports it. On any `Err` nothing was imported. The verdict depends only on the batch
    /// and this shard's content, never on arrival order.
    pub fn apply_remote(&mut self, update: &[u8], ctx: &Ctx) -> Result<Applied, Reject> {
        let history = ctx.history.is_some();
        let meta = LoroDoc::decode_import_blob_meta(update, true).map_err(|_| Reject::Undecodable)?;
        if !history && meta.partial_end_vv.keys().any(|p| *p != ctx.signer_peer) {
            return Err(Reject::ForeignPeer);
        }
        let old_vv = self.vv();
        let scratch = self.doc.fork();
        let status = scratch.import(update).map_err(|_| Reject::Undecodable)?;
        if status.pending.is_some() {
            return Err(Reject::Pending);
        }
        let new_vv = scratch.oplog_vv();
        let mut grew = false;
        for (p, c) in new_vv.iter() {
            if *c > old_vv.get(p).copied().unwrap_or(0) {
                grew = true;
                if !history && *p != ctx.signer_peer {
                    return Err(Reject::ForeignPeer);
                }
            }
        }
        if !grew {
            return Ok(Applied::default());
        }
        let before = self.messages()?;
        let after = read_messages(&scratch)?;
        let lo = day_start(&self.day).ok_or(Reject::OutOfRange)? - HOUR_MS;
        let hi = lo + DAY_MS + 2 * HOUR_MS;
        let mut applied = Applied::default();
        for id in before.keys() {
            if !after.contains_key(id) {
                return Err(Reject::Removed);
            }
        }
        for (id, new) in &after {
            match before.get(id) {
                None => {
                    let author_ok = match &ctx.history {
                        None => new.author == ctx.signer_did,
                        Some(me) => new.author == ctx.signer_did || new.author == *me,
                    };
                    if !author_ok {
                        return Err(Reject::WrongAuthor);
                    }
                    if new.at < lo || new.at >= hi {
                        return Err(Reject::OutOfRange);
                    }
                    if ctx.live && (new.at - ctx.now_ms).abs() >= DAY_MS {
                        return Err(Reject::OutOfRange);
                    }
                    applied.added.push(id.clone());
                }
                Some(old) if old != new => {
                    if old.id != new.id || old.author != new.author || old.at != new.at || old.reply_to != new.reply_to || old.file != new.file {
                        return Err(Reject::Immutable(id.clone()));
                    }
                    // An old signed version must not roll a newer one back.
                    if new.edited_at < old.edited_at || (old.deleted && !new.deleted) {
                        return Err(Reject::Immutable(id.clone()));
                    }
                    // Text, edited_at and deleted: the author only. (A history snapshot is a
                    // vouched copy of both parties' messages, so either may appear changed.)
                    if !history && old.author != ctx.signer_did {
                        return Err(Reject::NotYours(id.clone()));
                    }
                    applied.changed.push(id.clone());
                }
                Some(_) => {}
            }
        }
        // Every new or changed message must be signed by its author's attested device, bound to
        // this very doc. This is what makes history snapshots safe to take from a peer.
        for new in after.values() {
            if before.get(&new.id) == Some(new) {
                continue;
            }
            let signed = ctx.keys.iter().any(|(did, key)| *did == new.author && verify_msg(key, &self.name, new));
            if !signed {
                return Err(Reject::BadSignature);
            }
        }
        // Accepted: apply to the real doc.
        self.doc.import(update).map_err(|_| Reject::Undecodable)?;
        Ok(applied)
    }
}
