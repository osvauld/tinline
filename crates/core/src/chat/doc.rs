//! One conversation day as a Loro doc, and the rules every device applies to every batch.
//!
//! `messages` is a map `id -> { id, author, at, text, edited_at?, deleted?, reply_to?, file? }`.
//! A device writes only with its own Loro peer id (`peer_id`), and a received batch is judged
//! deterministically from its own content and the receiver's copy of the same doc: the
//! receiver imports it into a scratch fork and checks what changed (see `Shard::apply_remote`).
//!
//! Every message version is signed twice (P8): by the author's DID key (`did_sig`, carried as
//! the commit message of the change that wrote it, see `DidSigNote`) and, for apps that predate
//! it, by the writing device's key (the record's `sig`). Receivers prefer the DID signature.
//!
//! P9 (U16): the Loro peer id a device writes with is its writer ID, `writer_id(DID, device
//! value)`, where the device value is random per account per install. Every note carries the
//! value; receivers recompute the hash and refuse ops under a writer ID that the change does
//! not prove (`apply_remote`). Ops under device-derived peer ids (`peer_id`, apps before P9)
//! stay judged by the legacy rule.
//!
//! P9b (U17): the DID signature also covers where the version was written: the Loro peer id,
//! first counter and length of the change that carries it (`WrittenAt`), so it proves "this DID
//! wrote this version as this change under this writer ID", and a note cannot be moved to
//! another change. Ops under a writer ID must come from proven writer changes of its author;
//! ops under any other peer id must be under a legacy peer id derived from a device key of the
//! author we know, or (history only) write nothing but the contact's own messages. Once a DID
//! has DID-signed versions, its versions from then on (by version time) need a DID signature.

use std::collections::{BTreeMap, BTreeSet};

use loro::{CommitOptions, ExportMode, LoroDoc, LoroMap, LoroValue, VersionVector};
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

/// The Loro peer id of `device` for the doc `name`: how apps before P9 wrote (the "legacy
/// peer"). It is derived from the device key, which is also the network address.
pub fn peer_id(device: &[u8; 32], name: &str) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(b"tinline-loro-peer-v1");
    h.update(device);
    h.update(name.as_bytes());
    let out = h.finalize();
    u64::from_le_bytes(out.as_bytes()[..8].try_into().unwrap())
}

const WRITER_ID_DOMAIN: &[u8] = b"tinline-writer-id-v1";

/// The writer ID (U16) of the account `did` on the install whose random device value is
/// `value`: the first 64 bits (little-endian) of BLAKE3 over a domain, the length-prefixed DID
/// and the value. The app writes every chat doc with it as the Loro peer id, and every receiver
/// recomputes it from the author's DID and the device value the post carries (`DidSigNote::dv`).
/// It depends on no device or transport key. `u64::MAX` is not a valid Loro peer id; that one
/// output maps to `u64::MAX - 1` (on every side alike).
pub fn writer_id(did: &str, value: &[u8; 32]) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(WRITER_ID_DOMAIN);
    h.update(&(did.len() as u32).to_be_bytes());
    h.update(did.as_bytes());
    h.update(value);
    let id = u64::from_le_bytes(h.finalize().as_bytes()[..8].try_into().unwrap());
    if id == u64::MAX { u64::MAX - 1 } else { id }
}

/// A fresh random device value: generated once per account per install, never derived.
pub fn new_device_value() -> [u8; 32] {
    use rand::RngCore;
    let mut v = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut v);
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
/// The DID-key signature of a message version covers the same bytes under its own domain, plus
/// where the version was written (`WrittenAt`). v1 (P8/P9, never released) did not bind that.
const MSG_DID_SIG_DOMAIN: &[u8] = b"tinline-chat-msg-did-v2";

/// The change that writes a DID-signed version: its Loro peer id (the writer ID, or the legacy
/// peer id in compatibility mode), the counter of its first op and its op count. The DID
/// signature covers it, so it proves who wrote this version under which peer id, and a copy of
/// the note on any other change (another peer, counter or length) does not verify.
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

/// Canonical bytes a message signature covers: doc id (so a message cannot be replayed into
/// another day or pair), id, author, time, reply, file and the current text/edit/delete state.
pub fn msg_sig_input(doc: &str, m: &MsgRec) -> Vec<u8> {
    sig_input(MSG_SIG_DOMAIN, doc, m)
}

/// What the author's DID key signs: `msg_sig_input` under `MSG_DID_SIG_DOMAIN`, then the
/// change that writes it (`WrittenAt`: peer id u64 BE, counter i32 BE, length u32 BE).
pub fn msg_did_sig_input(doc: &str, m: &MsgRec, at: &WrittenAt) -> Vec<u8> {
    let mut v = sig_input(MSG_DID_SIG_DOMAIN, doc, m);
    v.extend_from_slice(&at.peer.to_be_bytes());
    v.extend_from_slice(&at.counter.to_be_bytes());
    v.extend_from_slice(&at.len.to_be_bytes());
    v
}

fn sig_input(domain: &[u8], doc: &str, m: &MsgRec) -> Vec<u8> {
    let mut v = domain.to_vec();
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

pub fn sign_msg_did(account: &identity::Identity, doc: &str, m: &MsgRec, at: &WrittenAt) -> Vec<u8> {
    account.sign(&msg_did_sig_input(doc, m, at)).to_vec()
}

/// `did_sig` is a signature of this version of `m`, written by the change `at`, by the key of
/// its author's DID.
pub fn verify_msg_did(doc: &str, m: &MsgRec, at: &WrittenAt, did_sig: &[u8]) -> bool {
    let Some(key) = identity::public_key_from_did(&m.author) else { return false };
    let Ok(sig) = <[u8; 64]>::try_from(did_sig) else { return false };
    identity::verify(&key, &msg_did_sig_input(doc, m, at), &sig)
}

/// The time of a message version: its last edit, else its creation. What the per-DID
/// requirement (`Ctx::did_since`) compares.
pub fn version_time(m: &MsgRec) -> i64 {
    m.edited_at.unwrap_or(m.at)
}

/// The DID signature of one message version, as the commit message of the change that wrote
/// it. Not a field of the record: an app that predates it rejects a record with any unknown
/// field (`read_messages`), but it ignores commit messages, and Loro keeps them through
/// updates, snapshots and relays. `sig` names the version: it is that version's device
/// signature (Ed25519 is deterministic, so a version and its device key fix it).
///
/// P9 adds `dv`, the writer's device value: receivers recompute `writer_id(author, dv)` and
/// require it to be the peer id of the change carrying the note, and the note stays under
/// `MAX_NOTE` with it. P9b (`DID_SIG_NOTE` v2): `did_sig` also covers the carrying change
/// (`WrittenAt`). Apps before P8 ignore commit messages; v1 notes (P8/P9 test builds, never
/// released) are not notes to us.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DidSigNote {
    /// Always `DID_SIG_NOTE`.
    t: String,
    id: String,
    /// Hex of the record's `sig` at this version.
    sig: String,
    /// Hex of the DID signature (`sign_msg_did`).
    did_sig: String,
    /// Hex of the writer's device value (P9); absent in notes written by P8 apps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dv: Option<String>,
}

const DID_SIG_NOTE: &str = "tinline-did-sig-v2";
/// A commit message longer than this is no note of ours.
pub const MAX_NOTE: usize = 512;

/// The commit message carrying `did_sig` for the version of message `id` whose device
/// signature is `sig` (as a P8 app writes it: no device value; tests).
#[cfg(test)]
pub fn did_sig_note(id: &str, sig: &[u8], did_sig: &[u8]) -> String {
    did_sig_note_dv(id, sig, did_sig, None)
}

/// `did_sig_note` that also carries the writer's device value `dv` (P9).
pub fn did_sig_note_dv(id: &str, sig: &[u8], did_sig: &[u8], dv: Option<&[u8; 32]>) -> String {
    let n = DidSigNote { t: DID_SIG_NOTE.into(), id: id.into(), sig: hex::encode(sig), did_sig: hex::encode(did_sig), dv: dv.map(hex::encode) };
    serde_json::to_string(&n).expect("note json")
}

fn parse_note(msg: &str) -> Option<DidSigNote> {
    if msg.len() > MAX_NOTE {
        return None;
    }
    serde_json::from_str::<DidSigNote>(msg).ok().filter(|n| n.t == DID_SIG_NOTE)
}

impl DidSigNote {
    /// The device value, if the note carries a well-formed one.
    fn value(&self) -> Option<[u8; 32]> {
        self.dv.as_deref().and_then(|h| hex::decode(h).ok()).and_then(|b| b.try_into().ok())
    }
}

/// One change of a doc: its peer and the note it carries, if any.
struct ChangeNote {
    peer: u64,
    note: Option<DidSigNote>,
}

/// The changes of `doc` beyond `from` (each as stored: a change with a note is never merged
/// with another, see `Change::can_merge_right`).
fn changes_since(doc: &LoroDoc, from: &VersionVector) -> Vec<ChangeNote> {
    let mut out = Vec::new();
    for (peer, end) in doc.oplog_vv().iter() {
        let mut at = from.get(peer).copied().unwrap_or(0);
        while at < *end {
            let Some(ch) = doc.get_change(loro::ID::new(*peer, at)) else { break };
            out.push(ChangeNote { peer: *peer, note: ch.message.as_deref().and_then(parse_note) });
            let next = ch.id.counter + ch.len as i32;
            if next <= at {
                break;
            }
            at = next;
        }
    }
    out
}

/// What one op of a change writes to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// The key of `messages` (a message id): the op creates (or replaces) that message.
    Key(String),
    /// A container: a message record, or anything else.
    Container(loro::ContainerID),
    /// An op on `messages` that is not a map op.
    Other,
}

/// A change that a batch adds: where it is, its note, and what its ops write to.
struct NewChange {
    at: WrittenAt,
    note: Option<DidSigNote>,
    targets: Vec<Target>,
}

/// The changes of `doc` between `from` and `to`, with their ops (as Loro's JSON export, peer ids
/// uncompressed).
fn new_changes(doc: &LoroDoc, from: &VersionVector, to: &VersionVector) -> Vec<NewChange> {
    let root = loro::ContainerID::new_root("messages", loro::ContainerType::Map);
    doc.export_json_updates_without_peer_compression(from, to)
        .changes
        .into_iter()
        .map(|c| {
            let targets = c
                .ops
                .iter()
                .map(|op| match (&op.container, &op.content) {
                    (id, loro::JsonOpContent::Map(m)) if *id == root => match m {
                        loro::JsonMapOp::Insert { key, .. } | loro::JsonMapOp::Delete { key } => Target::Key(key.clone()),
                    },
                    (id, _) if *id == root => Target::Other,
                    (id, _) => Target::Container(id.clone()),
                })
                .collect();
            let len = u32::try_from(c.op_len()).unwrap_or(0);
            NewChange { at: WrittenAt { peer: c.id.peer, counter: c.id.counter, len }, note: c.msg.as_deref().and_then(parse_note), targets }
        })
        .collect()
}

/// A DID signature found in a change, with the change that carries it.
struct NotedSig {
    at: WrittenAt,
    did_sig: Vec<u8>,
}

/// The DID signatures that the changes of `doc` beyond `from` carry, by (message id, hex of the
/// version's device signature). A new or changed version was written by one of these changes.
fn did_sigs_since(changes: &[NewChange]) -> BTreeMap<(String, String), Vec<NotedSig>> {
    let mut out: BTreeMap<(String, String), Vec<NotedSig>> = BTreeMap::new();
    for c in changes {
        if let Some(n) = &c.note {
            // Junk hex counts as a signature that does not verify, not as no signature.
            let s = NotedSig { at: c.at, did_sig: hex::decode(&n.did_sig).unwrap_or_default() };
            out.entry((n.id.clone(), n.sig.clone())).or_default().push(s);
        }
    }
    out
}

/// Whether the note `n`, on a change under `peer`, claims that peer as a writer ID: it carries a
/// device value `dv` and `writer_id(author of the noted message, dv)` is that peer id. (The
/// hash alone; `Shard::apply_remote` also checks the signature.)
fn claims_writer(peer: u64, n: Option<&DidSigNote>, msgs: &BTreeMap<String, MsgRec>) -> bool {
    let Some(n) = n else { return false };
    let (Some(dv), Some(m)) = (n.value(), msgs.get(&n.id)) else { return false };
    writer_id(&m.author, &dv) == peer
}

/// Who writes a message version, with which keys, and under which Loro peer id.
pub struct Writer {
    /// This install's device secret: the record's `sig`, still produced for apps that predate
    /// DID signatures (and verified by them).
    device: [u8; 32],
    /// The account; `None` writes as such an app did (device signature only; tests).
    account: Option<identity::Identity>,
    /// This install's device value for the account (P9): carried in every note.
    value: Option<[u8; 32]>,
    /// Write under the writer ID (`writer_id(did, value)`). Off: under the shard's legacy peer
    /// id, as every writer without a writer ID does; for contacts with a device not yet known
    /// to judge writer IDs (it would refuse the batch). The notes still carry `value`, which
    /// tells the contact that we can.
    as_writer: bool,
}

impl Writer {
    /// DID and device signature, legacy peer id, no device value: as a P8 app wrote (tests).
    #[cfg(test)]
    pub fn new(device: [u8; 32], account: identity::Identity) -> Self {
        Self { device, account: Some(account), value: None, as_writer: false }
    }

    /// Device signature only, legacy peer id, as before P8.
    pub fn legacy(device: [u8; 32]) -> Self {
        Self { device, account: None, value: None, as_writer: false }
    }

    /// P9: DID and device signature, the device value in every note, and the writer ID as the
    /// peer id unless `as_writer` is off (see the field).
    pub fn p9(device: [u8; 32], account: identity::Identity, value: [u8; 32], as_writer: bool) -> Self {
        Self { device, account: Some(account), value: Some(value), as_writer }
    }

    /// The writer ID, if this writer writes under one.
    pub fn writer_id(&self) -> Option<u64> {
        match (&self.account, &self.value) {
            (Some(a), Some(v)) if self.as_writer => Some(writer_id(a.did(), v)),
            _ => None,
        }
    }

    /// The commit message for the version `m` with device signature `sig`, written by the
    /// change `at`: the DID-signature note, if this writer has an account.
    fn note(&self, doc: &str, m: &MsgRec, sig: &[u8], at: WrittenAt) -> Option<String> {
        self.account.as_ref().map(|a| did_sig_note_dv(&m.id, sig, &sign_msg_did(a, doc, m, &at), self.value.as_ref()))
    }
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
    /// New messages kept hidden: their signature did not verify (see `Ctx::hide_unverified`).
    pub hidden: Vec<String>,
    /// Peer ids the batch proved to be writer IDs (a proven writer change, see `apply_remote`),
    /// for `Ctx::known_writers`.
    pub writers: Vec<u64>,
    /// A change of the batch carried a device value: its writer judges writer IDs (P9).
    pub dv_seen: bool,
    /// DIDs whose versions in this batch verified by a DID signature, with the earliest
    /// version time among them: for `Ctx::did_since` (P9b).
    pub did_signed: Vec<(String, i64)>,
}

impl Applied {
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.hidden.is_empty()
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
    /// Only for history (a vouched snapshot, own-device sync): a message by this DID (the
    /// contact) that is new to us, or still hidden, and whose signature does not verify against
    /// `keys` (e.g. signed by a device key of theirs we never learned) is kept hidden instead of
    /// failing the whole batch. Its ops are imported, since later ops may build on them, but
    /// `Shard::messages` never returns it. Anything else that does not verify still fails it.
    pub hide_unverified: Option<String>,
    /// Peer ids known to be writer IDs (P9; seen proven in earlier batches, any doc). A change
    /// under one of these, or under a writer ID proven anywhere in this doc, or under our own,
    /// must prove it again; a version written under one is never judged by the legacy rule.
    pub known_writers: Vec<u64>,
    /// DIDs that have DID-signed versions (P9b, U17 b), with the earliest version time seen
    /// DID-signed. A version by such a DID at or after that time without a DID signature counts
    /// as not signed (hidden in history if it is the contact's, else the batch is refused).
    /// The batch's own DID-signed versions count too.
    pub did_since: Vec<(String, i64)>,
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
    /// Our legacy peer id in this doc (`peer_id(device, name)`).
    pub legacy: u64,
    /// Our writer ID (P9), once the app set it or wrote under it. Our ops are under this and
    /// `legacy` (a device that upgraded mid-day writes under both).
    pub writer: Option<u64>,
    /// Messages in the doc whose signature we could not verify (`Ctx::hide_unverified`). Not
    /// part of the doc: persisted next to it (`ShardMeta::hidden`).
    pub hidden: BTreeSet<String>,
}

impl Shard {
    pub fn new(pair: &str, day: &str, device: &[u8; 32]) -> Self {
        let name = doc_name(pair, day);
        let peer = peer_id(device, &name);
        let doc = LoroDoc::new();
        doc.set_peer_id(peer).expect("peer id");
        Self { day: day.to_string(), doc, name, legacy: peer, writer: None, hidden: BTreeSet::new() }
    }

    /// The peer id the doc writes with now: the legacy one until a P9 writer wrote.
    pub fn peer(&self) -> u64 {
        self.doc.peer_id()
    }

    /// Our peer ids in this doc: the legacy one, our writer ID, and whatever we write with now.
    pub fn own_peers(&self) -> Vec<u64> {
        let mut v = vec![self.legacy];
        for p in self.writer.into_iter().chain([self.peer()]) {
            if !v.contains(&p) {
                v.push(p);
            }
        }
        v
    }

    pub fn is_own_peer(&self, p: u64) -> bool {
        self.own_peers().contains(&p)
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
        s.doc.set_peer_id(s.legacy).expect("peer id");
        Ok(s)
    }

    pub fn vv(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    /// Our own device's op count (the tick marker for what we wrote so far).
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

    /// The messages we accepted: every message in the doc but the hidden ones.
    pub fn messages(&self) -> Result<BTreeMap<String, MsgRec>, Reject> {
        let mut all = read_messages(&self.doc)?;
        all.retain(|id, _| !self.hidden.contains(id));
        Ok(all)
    }

    fn msgs_map(&self) -> LoroMap {
        self.doc.get_map("messages")
    }

    /// Runs a local write as one change under `peer` and returns the update it produced. The
    /// change's commit message is `note` of where it is written (`WrittenAt`: known once its
    /// ops are, before the commit).
    fn write(&self, peer: u64, note: impl FnOnce(WrittenAt) -> Option<String>, f: impl FnOnce(&LoroMap) -> loro::LoroResult<()>) -> Result<Vec<u8>, Reject> {
        if self.doc.peer_id() != peer {
            self.doc.set_peer_id(peer).map_err(|_| Reject::Structure)?;
        }
        let before = self.vv();
        let counter = before.get(&peer).copied().unwrap_or(0);
        f(&self.msgs_map()).map_err(|_| Reject::Structure)?;
        let len = u32::try_from(self.doc.get_pending_txn_len()).map_err(|_| Reject::Structure)?;
        let mut opts = CommitOptions::new();
        opts.commit_msg = note(WrittenAt { peer, counter, len }).map(Into::into);
        self.doc.commit_with(opts);
        self.doc.export(ExportMode::updates(&before)).map_err(|_| Reject::Undecodable)
    }

    /// Adds a message; returns the update to persist and push.
    pub fn add_message(&self, m: &MsgRec, w: &Writer) -> Result<Vec<u8>, Reject> {
        let raw = sign_msg(&w.device, &self.name, m);
        let sig = hex::encode(&raw);
        self.write(w.writer_id().unwrap_or(self.legacy), |at| w.note(&self.name, m, &raw, at), |map| {
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
    fn resign(&self, id: &str, w: &Writer, change: impl FnOnce(&mut MsgRec)) -> Result<Vec<u8>, Reject> {
        let e = self.entry(id)?;
        let mut rec = self.messages()?.remove(id).ok_or(Reject::Structure)?;
        change(&mut rec);
        let raw = sign_msg(&w.device, &self.name, &rec);
        let sig = hex::encode(&raw);
        self.write(w.writer_id().unwrap_or(self.legacy), |at| w.note(&self.name, &rec, &raw, at), |_| {
            e.insert("text", rec.text.as_str())?;
            e.insert("deleted", rec.deleted)?;
            e.insert("edited_at", rec.edited_at.unwrap_or(0))?;
            e.insert("sig", sig.as_str())?;
            Ok(())
        })
    }

    pub fn edit(&self, id: &str, text: &str, now_ms: i64, w: &Writer) -> Result<Vec<u8>, Reject> {
        self.resign(id, w, |r| {
            r.text = text.to_string();
            r.edited_at = Some(now_ms);
        })
    }

    pub fn delete(&self, id: &str, now_ms: i64, w: &Writer) -> Result<Vec<u8>, Reject> {
        self.resign(id, w, |r| {
            r.text = String::new();
            r.deleted = true;
            r.edited_at = Some(now_ms);
        })
    }

    /// Whether `theirs` lacks ops of ours.
    pub fn owes(&self, theirs: &VersionVector) -> bool {
        let mine = self.vv();
        self.own_peers().iter().any(|p| theirs.get(p).copied().unwrap_or(0) < mine.get(p).copied().unwrap_or(0))
    }

    /// Only our own ops (under our legacy peer id and our writer ID) beyond `theirs`; `None` if
    /// they hold all of them. A device sends no one else's ops, so a receiver can insist that a
    /// batch holds only the sender's legacy peer id and writer IDs it proves (`apply_remote`).
    pub fn export_own_since(&self, theirs: &VersionVector) -> Option<Vec<u8>> {
        if !self.owes(theirs) {
            return None;
        }
        let mine = self.vv();
        let mut from = theirs.clone();
        for (p, c) in mine.iter() {
            if !self.is_own_peer(*p) {
                from.insert(*p, *c);
            }
        }
        self.doc.export(ExportMode::updates(&from)).ok()
    }

    /// Every op beyond `theirs`, whoever wrote it; `None` if they hold all we do. Only for our
    /// own devices (own-device sync): a contact is only ever sent our own ops.
    pub fn export_all_since(&self, theirs: &VersionVector) -> Option<Vec<u8>> {
        if theirs.partial_cmp(&self.vv()).is_some_and(|o| o != std::cmp::Ordering::Less) {
            return None;
        }
        self.doc.export(ExportMode::updates(theirs)).ok()
    }

    /// Validates `update` against this shard and, if it is acceptable in every respect,
    /// imports it. On any `Err` nothing was imported. The verdict depends only on the batch,
    /// this shard's content (hidden messages included) and `ctx`, never on arrival order.
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
        let grown: Vec<u64> = new_vv.iter().filter(|(p, c)| **c > old_vv.get(p).copied().unwrap_or(0)).map(|(p, _)| *p).collect();
        if grown.is_empty() {
            return Ok(Applied::default());
        }
        let before = read_messages(&self.doc)?;
        let after = read_messages(&scratch)?;
        let new_changes = new_changes(&scratch, &old_vv, &new_vv);
        let containers: std::collections::HashMap<loro::ContainerID, String> = after
            .keys()
            .filter_map(|id| match scratch.get_map("messages").get(id) {
                Some(loro::ValueOrContainer::Container(loro::Container::Map(m))) => Some((loro::ContainerTrait::id(&m), id.clone())),
                _ => None,
            })
            .collect();
        // P9b, U17 b: the DIDs that DID-sign, from when (earlier batches, and this one).
        let did_sigs = did_sigs_since(&new_changes);
        let mut did_signed: BTreeMap<String, i64> = BTreeMap::new();
        for new in after.values() {
            if before.get(&new.id) == Some(new) {
                continue;
            }
            if let Some(sigs) = did_sigs.get(&(new.id.clone(), hex::encode(&new.sig)))
                && sigs.iter().all(|s| verify_msg_did(&self.name, new, &s.at, &s.did_sig))
            {
                let t = did_signed.entry(new.author.clone()).or_insert(i64::MAX);
                *t = (*t).min(version_time(new));
            }
        }
        let since = |did: &str| -> Option<i64> {
            let stored = ctx.did_since.iter().filter(|(d, _)| d == did).map(|(_, t)| *t).min();
            match (stored, did_signed.get(did).copied()) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            }
        };
        // Writer IDs (P9). A peer id is a writer ID once a change under it carries a note whose
        // device value hashes, with the noted message's author, to it (`claims_writer`). Known:
        // remembered from earlier batches, our own, or claimed anywhere in this doc.
        let mut known: BTreeSet<u64> = ctx.known_writers.iter().copied().chain(self.own_peers().into_iter().filter(|p| *p != self.legacy)).collect();
        known.extend(changes_since(&scratch, &VersionVector::default()).iter().filter(|c| claims_writer(c.peer, c.note.as_ref(), &after)).map(|c| c.peer));
        // A proven writer change (P9b): it claims its peer id as the author's writer ID, its DID
        // signature verifies on the version it leaves behind and is bound to this very change
        // (`WrittenAt`), it writes only that message, and that message changes in this batch.
        let proven: Vec<bool> = new_changes.iter().map(|c| self.proven_writer_change(c, &scratch, &before, &after, &containers)).collect();
        // Legacy peer ids we can account for: derived from a device key we know (by DID), and
        // ours. In a live batch: the session's.
        let legacy_of = |p: u64, did: Option<&str>| {
            p == self.legacy || (!history && p == ctx.signer_peer) || ctx.keys.iter().any(|(d, k)| did.is_none_or(|did| did == d) && peer_id(k, &self.name) == p)
        };
        for (c, ok) in new_changes.iter().zip(&proven) {
            if *ok {
                continue;
            }
            // Under a writer ID: only proven writer changes (P9). A change that claims a writer
            // ID but fails the proof is refused too.
            if known.contains(&c.at.peer) || claims_writer(c.at.peer, c.note.as_ref(), &after) {
                return Err(Reject::ForeignPeer);
            }
            // A legacy peer id we can account for: one of the author's known device keys (for a
            // DID-signed version: compatibility mode, or an app from before P9), ours, or live
            // the session's.
            let author = c.note.as_ref().map(|n| after.get(&n.id).map(|m| m.author.as_str()));
            if author.is_none_or(|a| a.is_some()) && legacy_of(c.at.peer, author.flatten()) {
                continue;
            }
            // Any other peer id (a device key we never learned) only in history, and only for
            // the contact's own messages: such a peer id may also be a writer ID we have not
            // seen yet, so no message of ours (or anyone else's) is ever accepted under it.
            if !history {
                return Err(Reject::ForeignPeer);
            }
            let contacts = |t: &Target| {
                let id = match t {
                    Target::Key(k) => Some(k),
                    Target::Container(cid) => containers.get(cid),
                    Target::Other => None,
                };
                id.and_then(|id| after.get(id)).is_some_and(|m| ctx.hide_unverified.as_deref() == Some(m.author.as_str()))
            };
            let noted_ok = c.note.as_ref().is_none_or(|n| after.get(&n.id).is_some_and(|m| ctx.hide_unverified.as_deref() == Some(m.author.as_str())));
            if c.targets.is_empty() || !noted_ok || !c.targets.iter().all(contacts) {
                return Err(Reject::ForeignPeer);
            }
        }
        // A live batch holds only the signer's legacy peer id and writer IDs of the signer's
        // DID that the batch itself proves.
        if !history {
            for p in &grown {
                let signers = |(c, ok): (&NewChange, &bool)| *ok && c.at.peer == *p && c.note.as_ref().and_then(|n| after.get(&n.id)).is_some_and(|m| m.author == ctx.signer_did);
                if *p != ctx.signer_peer && !new_changes.iter().zip(&proven).any(signers) {
                    return Err(Reject::ForeignPeer);
                }
            }
        }
        let lo = day_start(&self.day).ok_or(Reject::OutOfRange)? - HOUR_MS;
        let hi = lo + DAY_MS + 2 * HOUR_MS;
        let writers: BTreeSet<u64> = new_changes.iter().zip(&proven).filter(|(_, ok)| **ok).map(|(c, _)| c.at.peer).collect();
        let mut applied = Applied {
            writers: writers.into_iter().collect(),
            dv_seen: new_changes.iter().any(|c| c.note.as_ref().is_some_and(|n| n.value().is_some())),
            did_signed: did_signed.iter().map(|(d, t)| (d.clone(), *t)).collect(),
            ..Default::default()
        };
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
        // Every new or changed message must be signed by its author, bound to this very doc. This
        // is what makes history snapshots safe to take from a peer. A version that carries a DID
        // signature is judged by it alone: it must verify against the author's DID for the change
        // that wrote it, and a valid device signature never rescues it. Only a version without
        // one (written by an app that predates them) is judged by the device signature against
        // the attested `keys`, and only while its author has no DID-signed version at or before
        // its time (`Ctx::did_since`, P9b). History is judged per message: one of the contact's
        // that we cannot verify stays hidden.
        let mut hidden = self.hidden.clone();
        for new in after.values() {
            if before.get(&new.id) == Some(new) {
                continue;
            }
            let signed = match did_sigs.get(&(new.id.clone(), hex::encode(&new.sig))) {
                Some(sigs) => sigs.iter().all(|s| verify_msg_did(&self.name, new, &s.at, &s.did_sig)),
                None => {
                    since(&new.author).is_none_or(|t| version_time(new) < t)
                        && ctx.keys.iter().any(|(did, key)| *did == new.author && verify_msg(key, &self.name, new))
                }
            };
            let id = &new.id;
            if signed {
                // A hidden message now verifies (e.g. signed again with a key we know): it is
                // new to whoever reads this shard.
                if hidden.remove(id) {
                    applied.changed.retain(|c| c != id);
                    applied.added.push(id.clone());
                }
            } else if history && ctx.hide_unverified.as_deref() == Some(new.author.as_str()) && (!before.contains_key(id) || hidden.contains(id)) {
                hidden.insert(id.clone());
                applied.added.retain(|a| a != id);
                applied.changed.retain(|c| c != id);
                applied.hidden.push(id.clone());
            } else {
                return Err(Reject::BadSignature);
            }
        }
        // Accepted: apply to the real doc.
        self.doc.import(update).map_err(|_| Reject::Undecodable)?;
        self.hidden = hidden;
        Ok(applied)
    }

    /// Whether the new change `c` of `scratch` is a proven writer change (P9b): its note names a
    /// message whose author's writer ID for the note's device value is `c`'s peer id; the
    /// version of that message right after `c` is the one the note names (by device
    /// signature) and changed in this batch; its DID signature verifies for exactly this change
    /// (`WrittenAt`); and `c` writes nothing but that message.
    fn proven_writer_change(&self, c: &NewChange, scratch: &LoroDoc, before: &BTreeMap<String, MsgRec>, after: &BTreeMap<String, MsgRec>, containers: &std::collections::HashMap<loro::ContainerID, String>) -> bool {
        let Some(n) = &c.note else { return false };
        if !claims_writer(c.at.peer, Some(n), after) || c.at.len == 0 {
            return false;
        }
        if after.get(&n.id) == before.get(&n.id) {
            return false;
        }
        let only_it = c.targets.iter().all(|t| match t {
            Target::Key(k) => *k == n.id,
            Target::Container(cid) => containers.get(cid) == Some(&n.id),
            Target::Other => false,
        });
        if !only_it {
            return false;
        }
        let Ok(did_sig) = hex::decode(&n.did_sig) else { return false };
        // The version the change leaves behind: the current one if the note names it, else as
        // the doc was right after the change.
        let current = after.get(&n.id).filter(|m| hex::encode(&m.sig) == n.sig).cloned();
        let version = current.or_else(|| {
            let last = loro::ID::new(c.at.peer, c.at.counter + c.at.len as i32 - 1);
            let then = scratch.fork_at(&loro::Frontiers::from_id(last)).ok()?;
            read_messages(&then).ok()?.remove(&n.id).filter(|m| hex::encode(&m.sig) == n.sig)
        });
        version.is_some_and(|m| verify_msg_did(&self.name, &m, &c.at, &did_sig))
    }
}
