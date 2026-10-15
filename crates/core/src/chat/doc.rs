//! One conversation day as a Loro doc, and the rules every device applies to every batch.
//!
//! `messages` is a map `id -> record`, a record `{ id, author, at, reply_to?, file?, v:<writer>... }`
//! whose immutable fields are written once, at creation, and which holds one version per writer:
//! under `v:<writer ID as 16 hex digits>` (`version_key`) the latest version that writer wrote,
//! one atomic map value `{ text, edited_at?, deleted, dev, did_sig }` (`version_value`). Strict
//! model (P9c, U18; no compatibility with earlier apps or data):
//!
//! - Every message version carries, as record fields, the device ID of the install that wrote it
//!   (`dev`, random per install, U16) and its author's DID signature (`did_sig`) over the
//!   version's content, `dev` and the span of ops that wrote it (`WrittenAt`: Loro peer id, first
//!   counter, op count; `msg_sig_input`). The span ends with the insert of the writer's own
//!   version key. A version is one value, so a field-by-field merge never mixes two versions.
//! - What a message shows is its highest-ranked version (`rank`, P10): a delete above any edit,
//!   then the later `edited_at`, then the higher writer ID. Concurrent edits from two of the
//!   author's devices sit side by side under two keys, so every replica ranks the same versions
//!   and shows the same one, whatever Loro's own last-writer-wins order would pick. A writer's own
//!   versions never go back (older edit time, undone delete): judged per span, from the ops alone.
//! - A device writes every chat doc under its writer ID `writer_id(DID, device ID)` as the Loro
//!   peer id; nothing else.
//! - A receiver accepts a batch only if every new op lies in a span that ends with a `did_sig`
//!   which verifies, under the author's DID, for that very span, and whose peer id is
//!   `writer_id(author, dev)` (`Shard::apply_remote`). There is no other acceptance path: device
//!   keys, device lists and remembered state play no part, so every device reaches the same
//!   verdict on the same batch.

use std::collections::{BTreeMap, BTreeSet};

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

const WRITER_ID_DOMAIN: &[u8] = b"tinline-writer-id-v1";

/// The record key a writer's version of a message lives under: `v:` and its writer ID as 16
/// lowercase hex digits. Only that writer ever writes it.
pub fn version_key(writer: u64) -> String {
    format!("v:{writer:016x}")
}

/// The writer ID a record key names, if it is a version key in canonical form.
fn writer_of_key(k: &str) -> Option<u64> {
    let h = k.strip_prefix("v:")?;
    if h.len() != 16 || !h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    u64::from_str_radix(h, 16).ok()
}

/// How competing versions of one message rank; the highest is what the message shows (P10). A
/// delete beats any edit, then the later edit time, then the higher writer ID. Every replica
/// ranks the same versions the same way, so all of them show the same one in any arrival order.
fn rank(m: &MsgRec, writer: u64) -> (bool, Option<i64>, u64) {
    (m.deleted, m.edited_at, writer)
}

/// The writer ID (U16) of the account `did` on the install whose device ID is `device`: the
/// first 64 bits (little-endian) of BLAKE3 over a domain, the length-prefixed DID and the device
/// ID. The app writes every chat doc with it as the Loro peer id, and every receiver recomputes
/// it from the author and the `dev` of each version. It depends on no device or transport key.
/// `u64::MAX` is not a valid Loro peer id; that one output maps to `u64::MAX - 1`.
pub fn writer_id(did: &str, device: &[u8; 32]) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(WRITER_ID_DOMAIN);
    h.update(&(did.len() as u32).to_be_bytes());
    h.update(did.as_bytes());
    h.update(device);
    let id = u64::from_le_bytes(h.finalize().as_bytes()[..8].try_into().unwrap());
    if id == u64::MAX { u64::MAX - 1 } else { id }
}

/// A fresh random device ID: generated once per account per install, never derived.
pub fn new_device_value() -> [u8; 32] {
    let mut v = [0u8; 32];
    cryptography::rng::fill(&mut v);
    v
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
    /// Pictures (`tinline/chat/5`): the size shown (EXIF orientation applied) and a 4x3 RGB colour grid, hex
    /// ([`crate::chat::photo`]). 0x0 and empty for anything else, and for records written before /5.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub width: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub height: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub preview: String,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

impl FileRef {
    /// The picture's size and preview are present (a /5 photo).
    pub fn has_picture(&self) -> bool {
        self.width != 0 || self.height != 0 || !self.preview.is_empty()
    }
}

/// One version of a message, as its record reads.
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
    /// The device ID of the install that wrote this version: `writer_id(author, dev)` is the
    /// Loro peer id it was written under. Set by the writer (`Writer`).
    pub dev: [u8; 32],
    /// The author's DID signature over this version (`msg_sig_input`). Set by the writer.
    pub did_sig: Vec<u8>,
}

/// The P9b domain was `tinline-chat-msg-did-v2` (signature in a commit message); v3 signs the
/// record-field form, including `dev`.
const MSG_SIG_DOMAIN: &[u8] = b"tinline-chat-msg-v3";

/// The span of ops that writes one message version: its Loro peer id (the writer ID), the
/// counter of its first op and its op count, the last op being the insert of `v` (which holds
/// the `did_sig`). The DID
/// signature covers it, so it proves "this DID wrote this version as exactly these ops under this
/// writer ID", and a copy of the signature on any other span does not verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WrittenAt {
    pub peer: u64,
    pub counter: i32,
    pub len: u32,
}

fn put_str(v: &mut Vec<u8>, s: &str) {
    v.extend_from_slice(&(s.len() as u32).to_be_bytes());
    v.extend_from_slice(s.as_bytes());
}

/// What the author's DID key signs: domain, doc id (so a message cannot be replayed into another
/// day or pair), id, author, time, text, edit, delete, reply, file, then the device ID and the
/// span (`WrittenAt`: peer id u64 BE, counter i32 BE, length u32 BE). Strings are u32 BE
/// length-prefixed; options are a 0/1 byte then the value.
pub fn msg_sig_input(doc: &str, m: &MsgRec, at: &WrittenAt) -> Vec<u8> {
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
            // /5: the picture's size and preview are signed too. Only when present, so every record written
            // before /5 (no picture fields) keeps exactly the input it was signed with.
            if f.has_picture() {
                v.push(2);
                v.extend_from_slice(&f.width.to_be_bytes());
                v.extend_from_slice(&f.height.to_be_bytes());
                put_str(&mut v, &f.preview);
            }
        }
    }
    v.extend_from_slice(&m.dev);
    v.extend_from_slice(&at.peer.to_be_bytes());
    v.extend_from_slice(&at.counter.to_be_bytes());
    v.extend_from_slice(&at.len.to_be_bytes());
    v
}

/// The DID signature of `m` (its `dev` included; its `did_sig` ignored) written as the span `at`.
pub fn sign_msg(account: &identity::Identity, doc: &str, m: &MsgRec, at: &WrittenAt) -> Vec<u8> {
    account.sign(&msg_sig_input(doc, m, at)).to_vec()
}

/// `m.did_sig` is the signature of this version of `m`, written as the span `at`, by the key of
/// its author's DID.
pub fn verify_msg(doc: &str, m: &MsgRec, at: &WrittenAt) -> bool {
    let Some(key) = identity::public_key_from_did(&m.author) else { return false };
    let Ok(sig) = <[u8; 64]>::try_from(m.did_sig.as_slice()) else { return false };
    identity::verify(&key, &msg_sig_input(doc, m, at), &sig)
}

/// `m`, as it reads after the span `at`, proves that span: `at.peer` is its author's writer ID
/// for its `dev`, and its `did_sig` verifies for exactly that span.
fn proves(doc: &str, m: &MsgRec, at: &WrittenAt) -> Result<(), Reject> {
    if writer_id(&m.author, &m.dev) != at.peer {
        return Err(Reject::ForeignPeer);
    }
    if !verify_msg(doc, m, at) {
        return Err(Reject::BadSignature);
    }
    Ok(())
}

/// Who writes a message version: the account (its DID key signs) on this install (its device ID,
/// hence its writer ID).
pub struct Writer {
    account: identity::Identity,
    device: [u8; 32],
    peer: u64,
}

impl Writer {
    pub fn new(account: identity::Identity, device: [u8; 32]) -> Self {
        let peer = writer_id(account.did(), &device);
        Self { account, device, peer }
    }

    /// For test hooks only: signs with `account` but writes under the writer ID that `as_did`
    /// would have for `device` (a forgery receivers must refuse).
    pub(crate) fn forging(account: identity::Identity, device: [u8; 32], as_did: &str) -> Self {
        Self { peer: writer_id(as_did, &device), account, device }
    }

    /// The Loro peer id this writer writes under.
    pub fn writer_id(&self) -> u64 {
        self.peer
    }
}

impl MsgRec {
    /// Display order: send time, then id.
    pub fn order(&self) -> (i64, &str) {
        (self.at, &self.id)
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum Reject {
    #[error("batch does not decode")]
    Undecodable,
    #[error("ops under a peer id that is not their author's writer ID")]
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
    #[error("ops not covered by a DID-signed version")]
    Unsigned,
}

/// What a valid batch changed, for events and unread counts.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Applied {
    pub added: Vec<String>,
    pub changed: Vec<String>,
}

/// How strictly to judge a batch. Nothing here names a key: the verdict depends only on the
/// batch, the receiving doc and these.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// The DID the batch comes from (the session's contact; for own-device sync, ourselves).
    pub signer_did: String,
    pub now_ms: i64,
    /// A live push: `at` must also be within 24 h of now.
    pub live: bool,
    /// A copy that may hold both parties' ops (a contact's vouched history snapshot, or
    /// own-device sync): `Some(the other party)`. Authors must be `signer_did` or this, and
    /// either may appear changed. `None`: only the signer's own ops and messages.
    pub history: Option<String>,
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

fn read_hex<const N: usize>(v: Option<&LoroValue>) -> Option<[u8; N]> {
    v.and_then(read_str).and_then(|h| hex::decode(h).ok()).and_then(|b| b.try_into().ok())
}

/// The value of a version (`rec`, signed `did_sig`), as its writer files it under its version key.
pub fn version_value(rec: &MsgRec, did_sig: &[u8]) -> LoroValue {
    let mut v: std::collections::HashMap<&str, LoroValue> = std::collections::HashMap::new();
    v.insert("text", rec.text.as_str().into());
    v.insert("deleted", rec.deleted.into());
    if let Some(t) = rec.edited_at {
        v.insert("edited_at", t.into());
    }
    v.insert("dev", hex::encode(rec.dev).into());
    v.insert("did_sig", hex::encode(did_sig).into());
    LoroValue::from(v)
}

/// Every version of every message: `id -> writer ID -> version` (each a whole `MsgRec`: the
/// record's immutable fields and that writer's version).
type Records = BTreeMap<String, BTreeMap<u64, MsgRec>>;

/// Reads `messages` out of a deep value; `Structure` if anything else is in the doc,
/// `Malformed` for a record with an unknown, missing or ill-formed field, or no version. (That a
/// version's `dev` gives the writer ID it is filed under is part of its proof, `proves`.)
fn read_records(doc: &LoroDoc) -> Result<Records, Reject> {
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
            let mut versions = BTreeMap::new();
            for (k, val) in m.iter() {
                if matches!(k.as_str(), "id" | "author" | "at" | "reply_to" | "file") {
                    continue;
                }
                let Some(writer) = writer_of_key(k) else { return Err(bad()) };
                let LoroValue::Map(v) = val else { return Err(bad()) };
                versions.insert(writer, v);
            }
            if versions.is_empty() {
                return Err(bad());
            }
            let base = |k: &str| m.get(k);
            let id_f = base("id").and_then(read_str).ok_or_else(bad)?;
            let author = base("author").and_then(read_str).ok_or_else(bad)?;
            let at = base("at").and_then(read_i64).ok_or_else(bad)?;
            let reply_to = match base("reply_to") {
                None => None,
                Some(v) => Some(read_str(v).ok_or_else(bad)?),
            };
            let file = match base("file") {
                None => None,
                Some(v) => {
                    let s = read_str(v).ok_or_else(bad)?;
                    if s.len() > MAX_FILE_JSON {
                        return Err(bad());
                    }
                    Some(serde_json::from_str::<FileRef>(&s).map_err(|_| bad())?)
                }
            };
            let mut recs = BTreeMap::new();
            for (writer, v) in versions {
                for k in v.keys() {
                    if !matches!(k.as_str(), "text" | "edited_at" | "deleted" | "dev" | "did_sig") {
                        return Err(bad());
                    }
                }
                let rec = MsgRec {
                    id: id_f.clone(),
                    author: author.clone(),
                    at,
                    text: v.get("text").and_then(read_str).ok_or_else(bad)?,
                    edited_at: match v.get("edited_at") {
                        None => None,
                        Some(v) => Some(read_i64(v).ok_or_else(bad)?),
                    },
                    deleted: match v.get("deleted") {
                        None => false,
                        Some(LoroValue::Bool(b)) => *b,
                        Some(_) => return Err(bad()),
                    },
                    reply_to: reply_to.clone(),
                    file: file.clone(),
                    dev: read_hex::<32>(v.get("dev")).ok_or_else(bad)?,
                    did_sig: read_hex::<64>(v.get("did_sig")).ok_or_else(bad)?.to_vec(),
                };
                if rec.id != *id
                    || rec.text.len() > MAX_TEXT
                    || rec.author.len() > 200
                    || rec.id.is_empty()
                    || rec.id.len() > 64
                    || rec.reply_to.as_ref().is_some_and(|r| r.len() > 64)
                    || rec.file.as_ref().is_some_and(|f| {
                        f.waveform.len() > 256
                            || f.name.len() > 300
                            || f.width > 100_000
                            || f.height > 100_000
                            || f.preview.len() > 2 * crate::chat::photo::PREVIEW_LEN
                    })
                    || (rec.deleted && !rec.text.is_empty())
                {
                    return Err(bad());
                }
                recs.insert(writer, rec);
            }
            out.insert(id.clone(), recs);
        }
    }
    Ok(out)
}

/// What each message shows: its highest-ranked version (`rank`).
fn winners(records: &Records) -> BTreeMap<String, MsgRec> {
    records
        .iter()
        .filter_map(|(id, vs)| vs.iter().max_by_key(|(w, m)| rank(m, **w)).map(|(_, m)| (id.clone(), m.clone())))
        .collect()
}

fn read_messages(doc: &LoroDoc) -> Result<BTreeMap<String, MsgRec>, Reject> {
    read_records(doc).map(|r| winners(&r))
}

/// One op of a batch, as far as the proof is concerned.
struct OpAt {
    peer: u64,
    counter: i32,
    /// The message it writes: the key it sets in `messages`, or the record it writes a field of.
    msg: Option<String>,
    /// It inserts its writer's own version of that record: the last op of a version's span.
    closes: bool,
}

/// Splits the ops of `doc` between `from` and `to` into spans of one message each, ending with
/// the insert of the writer's own version key of that record (`WrittenAt`, message id).
/// `Unsigned` if any op is not part of such a span: an op on anything but `messages` and its
/// records, a removal or another writer's version key in a record, a span over two messages, or
/// ops after the last version of a peer.
fn spans(doc: &LoroDoc, from: &VersionVector, to: &VersionVector, containers: &std::collections::HashMap<loro::ContainerID, String>) -> Result<Vec<(WrittenAt, String)>, Reject> {
    let root = loro::ContainerID::new_root("messages", loro::ContainerType::Map);
    let mut ops: Vec<OpAt> = Vec::new();
    for c in doc.export_json_updates_without_peer_compression(from, to).changes {
        for op in c.ops {
            let loro::JsonOpContent::Map(m) = &op.content else { return Err(Reject::Unsigned) };
            let key = match m {
                loro::JsonMapOp::Insert { key, .. } | loro::JsonMapOp::Delete { key } => key,
            };
            let (msg, closes) = if op.container == root {
                (Some(key.clone()), false)
            } else {
                let insert = matches!(m, loro::JsonMapOp::Insert { .. });
                let closes = insert && *key == version_key(c.id.peer);
                // Besides its own version, a writer sets only the immutable fields of a record.
                if !insert || !(closes || matches!(key.as_str(), "id" | "author" | "at" | "reply_to" | "file")) {
                    return Err(Reject::Unsigned);
                }
                (containers.get(&op.container).cloned(), closes)
            };
            ops.push(OpAt { peer: c.id.peer, counter: op.counter, msg, closes });
        }
    }
    ops.sort_by_key(|o| (o.peer, o.counter));
    let mut out = Vec::new();
    let mut open: Option<(u64, i32, String)> = None;
    let mut next: Option<(u64, i32)> = None;
    for o in ops {
        let Some(msg) = o.msg else { return Err(Reject::Unsigned) };
        // Contiguous within a peer (Loro guarantees it; a gap would mean ops we cannot place).
        if let Some((p, c)) = next
            && p == o.peer
            && c != o.counter
        {
            return Err(Reject::Unsigned);
        }
        let (peer, start, m) = match open.take() {
            Some((p, s, m)) if p == o.peer => (p, s, m),
            Some(_) => return Err(Reject::Unsigned),
            None => (o.peer, o.counter, msg.clone()),
        };
        if m != msg {
            return Err(Reject::Unsigned);
        }
        if o.closes {
            let len = u32::try_from(o.counter - start + 1).map_err(|_| Reject::Unsigned)?;
            out.push((WrittenAt { peer, counter: start, len }, m));
        } else {
            open = Some((peer, start, m));
        }
        next = Some((o.peer, o.counter + 1));
    }
    if open.is_some() {
        return Err(Reject::Unsigned);
    }
    Ok(out)
}

/// A conversation day. Owns the Loro doc, which writes under our writer ID.
pub struct Shard {
    pub day: String,
    doc: LoroDoc,
    name: String,
}

impl Shard {
    /// An empty day that writes under `peer` (our writer ID).
    pub fn new(pair: &str, day: &str, peer: u64) -> Self {
        let name = doc_name(pair, day);
        let doc = LoroDoc::new();
        doc.set_peer_id(peer).expect("peer id");
        Self { day: day.to_string(), doc, name }
    }

    /// The peer id we write with: our writer ID.
    pub fn peer(&self) -> u64 {
        self.doc.peer_id()
    }

    /// Rebuilds a shard from its snapshot and the updates appended since; writes under `peer`.
    pub fn load(pair: &str, day: &str, peer: u64, snapshot: Option<&[u8]>, updates: &[Vec<u8>]) -> Result<Self, Reject> {
        let s = Self::new(pair, day, peer);
        if let Some(b) = snapshot {
            s.doc.import(b).map_err(|_| Reject::Undecodable)?;
        }
        for u in updates {
            s.doc.import(u).map_err(|_| Reject::Undecodable)?;
        }
        s.doc.set_peer_id(peer).expect("peer id");
        Ok(s)
    }

    pub fn vv(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    /// Our own op count (the tick marker for what we wrote so far).
    pub fn my_counter(&self) -> i32 {
        self.vv().get(&self.peer()).copied().unwrap_or(0)
    }

    /// Who created message `id` (peer) and that peer's op count once the creating change is
    /// in (the tick marker of a message written on another device). A merged change makes it
    /// later, never earlier.
    pub fn creator_end(&self, id: &str) -> Option<(u64, i32)> {
        let loro::ContainerID::Normal { peer, counter, .. } = loro::ContainerTrait::id(&self.entry(id).ok()?) else { return None };
        let ch = self.doc.get_change(loro::ID::new(peer, counter))?;
        Some((peer, ch.id.counter + ch.len as i32))
    }

    pub fn snapshot(&self) -> Vec<u8> {
        self.doc.export(ExportMode::Snapshot).expect("snapshot export")
    }

    /// Every message in the doc (all of them passed `apply_remote`, or are ours).
    pub fn messages(&self) -> Result<BTreeMap<String, MsgRec>, Reject> {
        read_messages(&self.doc)
    }

    fn msgs_map(&self) -> LoroMap {
        self.doc.get_map("messages")
    }

    /// Writes the version `rec` as one span under the writer's ID: `fields` writes the record's
    /// immutable fields if it is new (and returns the record), then, as the last op, the
    /// writer's version (`version_key`) with `dev` and the `did_sig` over the version and the
    /// span. Returns the update it produced.
    fn write(&self, w: &Writer, rec: &MsgRec, fields: impl FnOnce(&LoroMap) -> loro::LoroResult<LoroMap>) -> Result<Vec<u8>, Reject> {
        let peer = w.writer_id();
        if self.doc.peer_id() != peer {
            self.doc.set_peer_id(peer).map_err(|_| Reject::Structure)?;
        }
        let mut rec = rec.clone();
        rec.dev = w.device;
        let before = self.vv();
        let counter = before.get(&peer).copied().unwrap_or(0);
        let e = fields(&self.msgs_map()).map_err(|_| Reject::Structure)?;
        let len = u32::try_from(self.doc.get_pending_txn_len() + 1).map_err(|_| Reject::Structure)?;
        let at = WrittenAt { peer, counter, len };
        e.insert(&version_key(peer), version_value(&rec, &sign_msg(&w.account, &self.name, &rec, &at))).map_err(|_| Reject::Structure)?;
        let written = self.doc.get_pending_txn_len();
        self.doc.commit();
        // The span is what was signed (receivers would refuse it otherwise).
        if written != len as usize {
            return Err(Reject::Structure);
        }
        self.doc.export(ExportMode::updates(&before)).map_err(|_| Reject::Undecodable)
    }

    /// Adds a message (its `dev` and `did_sig` are the writer's); returns the update to persist
    /// and push.
    pub fn add_message(&self, m: &MsgRec, w: &Writer) -> Result<Vec<u8>, Reject> {
        self.write(w, m, |map| {
            let e = map.insert_container(&m.id, LoroMap::new())?;
            e.insert("id", m.id.as_str())?;
            e.insert("author", m.author.as_str())?;
            e.insert("at", m.at)?;
            if let Some(r) = &m.reply_to {
                e.insert("reply_to", r.as_str())?;
            }
            if let Some(f) = &m.file {
                e.insert("file", serde_json::to_string(f).expect("file json"))?;
            }
            Ok(e)
        })
    }

    fn entry(&self, id: &str) -> Result<LoroMap, Reject> {
        match self.msgs_map().get(id) {
            Some(loro::ValueOrContainer::Container(loro::Container::Map(m))) => Ok(m),
            _ => Err(Reject::Structure),
        }
    }

    /// Writes a new version of `id`, made from the version it shows with `change` applied, as
    /// one op: our version key. Its edit time is `now_ms`, or just after the version it shows if
    /// that is later (a skewed clock), so it outranks everything this device has seen. A deleted
    /// message stays deleted (`Immutable`).
    fn resign(&self, id: &str, w: &Writer, now_ms: i64, change: impl FnOnce(&mut MsgRec)) -> Result<Vec<u8>, Reject> {
        let e = self.entry(id)?;
        let mut rec = self.messages()?.remove(id).ok_or(Reject::Structure)?;
        if rec.deleted {
            return Err(Reject::Immutable(id.to_string()));
        }
        let at = rec.edited_at.map_or(now_ms, |t| now_ms.max(t.saturating_add(1)));
        change(&mut rec);
        rec.edited_at = Some(at);
        self.write(w, &rec, |_| Ok(e))
    }

    pub fn edit(&self, id: &str, text: &str, now_ms: i64, w: &Writer) -> Result<Vec<u8>, Reject> {
        self.resign(id, w, now_ms, |r| r.text = text.to_string())
    }

    pub fn delete(&self, id: &str, now_ms: i64, w: &Writer) -> Result<Vec<u8>, Reject> {
        self.resign(id, w, now_ms, |r| {
            r.text = String::new();
            r.deleted = true;
        })
    }

    /// Our writer IDs in this day: this shard's own (`peer`) and every writer ID that wrote a
    /// version of a message authored by `me` (our DID). A writer ID is `writer_id(DID, device
    /// ID)` and every op lies in a span its author's DID signature proves (`apply_remote`), so
    /// these are exactly the writer IDs of our installs: before a restore, after it, or on our
    /// other devices (P10b).
    pub fn own_writers(&self, me: &str) -> BTreeSet<u64> {
        let mut out: BTreeSet<u64> = read_records(&self.doc)
            .map(|r| r.values().flat_map(|vs| vs.iter().filter(|(_, m)| m.author == me).map(|(w, _)| *w)).collect())
            .unwrap_or_default();
        out.insert(self.peer());
        out
    }

    /// Whether `theirs` lacks ops of any of `writers`.
    pub fn owes(&self, theirs: &VersionVector, writers: &BTreeSet<u64>) -> bool {
        let mine = self.vv();
        writers.iter().any(|p| theirs.get(p).copied().unwrap_or(0) < mine.get(p).copied().unwrap_or(0))
    }

    /// Only our ops (under any of our writer IDs, `own_writers(me)`) beyond `theirs`; `None` if
    /// they hold all of them. A device sends a contact no one else's ops, but every op of ours
    /// it holds, whichever of our installs wrote it (P10b): a message written before a restore
    /// or on another of our devices still reaches the contact. Each is proven by our DID
    /// signature, so the contact takes it from any of our devices.
    pub fn export_own_since(&self, theirs: &VersionVector, me: &str) -> Option<Vec<u8>> {
        let ours = self.own_writers(me);
        if !self.owes(theirs, &ours) {
            return None;
        }
        let mine = self.vv();
        let mut from = theirs.clone();
        for (p, c) in mine.iter() {
            if !ours.contains(p) {
                from.insert(*p, *c);
            }
        }
        self.doc.export(ExportMode::updates(&from)).ok()
    }

    /// Whether what `export_own_since(theirs, me)` sends would create a message whose `at` is
    /// `window` ms or more away from `now_ms`: one a receiver refuses in a live push (`Ctx::live`).
    pub fn creates_outside(&self, theirs: &VersionVector, me: &str, now_ms: i64, window: i64) -> bool {
        let Ok(msgs) = self.messages() else { return false };
        msgs.values().any(|m| {
            m.author == me
                && (m.at - now_ms).abs() >= window
                // The op that created its record (Loro may merge later ops into that change,
                // so not `creator_end`): they lack it iff their count for its peer is not past it.
                && self.entry(&m.id).ok().is_some_and(|e| match loro::ContainerTrait::id(&e) {
                    loro::ContainerID::Normal { peer, counter, .. } => theirs.get(&peer).copied().unwrap_or(0) <= counter,
                    _ => false,
                })
        })
    }

    /// Every op beyond `theirs`, whoever wrote it; `None` if they hold all we do. For our own
    /// devices (own-device sync) and history snapshots.
    pub fn export_all_since(&self, theirs: &VersionVector) -> Option<Vec<u8>> {
        if theirs.partial_cmp(&self.vv()).is_some_and(|o| o != std::cmp::Ordering::Less) {
            return None;
        }
        self.doc.export(ExportMode::updates(theirs)).ok()
    }

    /// Validates `update` against this shard and, if it is acceptable in every respect,
    /// imports it. On any `Err` nothing was imported. The verdict depends only on the batch,
    /// this shard's content and `ctx`, never on arrival order or on anything a device remembers.
    pub fn apply_remote(&mut self, update: &[u8], ctx: &Ctx) -> Result<Applied, Reject> {
        let history = ctx.history.is_some();
        LoroDoc::decode_import_blob_meta(update, true).map_err(|_| Reject::Undecodable)?;
        let old_vv = self.vv();
        let scratch = self.doc.fork();
        let status = scratch.import(update).map_err(|_| Reject::Undecodable)?;
        if status.pending.is_some() {
            return Err(Reject::Pending);
        }
        let new_vv = scratch.oplog_vv();
        if !new_vv.iter().any(|(p, c)| *c > old_vv.get(p).copied().unwrap_or(0)) {
            return Ok(Applied::default());
        }
        let before_all = read_records(&self.doc)?;
        let after_all = read_records(&scratch)?;
        let (before, after) = (winners(&before_all), winners(&after_all));
        // Per message: who may add or change what.
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
                        Some(other) => new.author == ctx.signer_did || new.author == *other,
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
                    // Text, edited_at and deleted: the author only. (A history copy holds both
                    // parties' versions, so either may appear changed; each is still proven by
                    // its author's signature below.)
                    if !history && old.author != ctx.signer_did {
                        return Err(Reject::NotYours(id.clone()));
                    }
                    applied.changed.push(id.clone());
                }
                Some(_) => {}
            }
        }
        // Every new op lies in a span that its author's DID signature proves, under the author's
        // writer ID for the version's device ID (P9c, U18). This is what makes history snapshots
        // and own-device relays safe to take, and what keeps anyone from writing under another
        // DID's writer ID (seen here before or not).
        let containers: std::collections::HashMap<loro::ContainerID, String> = after
            .keys()
            .filter_map(|id| match scratch.get_map("messages").get(id) {
                Some(loro::ValueOrContainer::Container(loro::Container::Map(m))) => Some((loro::ContainerTrait::id(&m), id.clone())),
                _ => None,
            })
            .collect();
        let spans = spans(&scratch, &old_vv, &new_vv, &containers)?;
        let mut proven: BTreeMap<&str, Vec<MsgRec>> = BTreeMap::new();
        // Per (message, writer): its version before the span at hand. `spans` lists each peer's
        // spans in counter order; before the first, it is the version our doc holds (only that
        // writer writes its key, and our doc holds all its ops before this batch).
        let mut prior: BTreeMap<(&str, u64), MsgRec> = BTreeMap::new();
        for (at, id) in &spans {
            let version = self.version_after(&scratch, &after_all, id, at)?;
            // Live: a contact sends only its own ops.
            if !history && version.author != ctx.signer_did {
                return Err(Reject::ForeignPeer);
            }
            // A writer's versions never go back: no older edit time, no undone delete (P10). From
            // the ops alone, so every device reaches the same verdict in any delivery order.
            let prev = prior.get(&(id.as_str(), at.peer)).or_else(|| before_all.get(id).and_then(|vs| vs.get(&at.peer)));
            if prev.is_some_and(|p| (version.deleted, version.edited_at) < (p.deleted, p.edited_at)) {
                return Err(Reject::Immutable(id.clone()));
            }
            prior.insert((id.as_str(), at.peer), version.clone());
            let Some((key, _)) = after.get_key_value(id.as_str()) else { return Err(Reject::Unsigned) };
            proven.entry(key.as_str()).or_default().push(version);
        }
        // What each touched message ends up showing is one of the versions proven here.
        for id in applied.added.iter().chain(&applied.changed) {
            if !proven.get(id.as_str()).is_some_and(|vs| vs.contains(&after[id])) {
                return Err(Reject::BadSignature);
            }
        }
        // Accepted: apply to the real doc.
        self.doc.import(update).map_err(|_| Reject::Undecodable)?;
        Ok(applied)
    }

    /// The version of message `id` that the span `at` writes (under its writer's key), if it
    /// proves that span (`proves`): the current one when it does, else the one as of the span's
    /// last op.
    fn version_after(&self, scratch: &LoroDoc, after: &Records, id: &str, at: &WrittenAt) -> Result<MsgRec, Reject> {
        if let Some(m) = after.get(id).and_then(|vs| vs.get(&at.peer))
            && proves(&self.name, m, at).is_ok()
        {
            return Ok(m.clone());
        }
        let last = loro::ID::new(at.peer, at.counter + at.len as i32 - 1);
        let then = scratch.fork_at(&loro::Frontiers::from_id(last)).map_err(|_| Reject::Unsigned)?;
        let m = read_records(&then)?.remove(id).and_then(|mut vs| vs.remove(&at.peer)).ok_or(Reject::Unsigned)?;
        proves(&self.name, &m, at)?;
        Ok(m)
    }
}
