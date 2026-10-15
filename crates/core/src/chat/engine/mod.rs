//! The chat runtime: sealed store, shard cache, sessions over `tinline/chat/3`, outbox/retry,
//! blob transfers. Everything hangs off `Inner` (see `node.rs`); the UniFFI methods in `api.rs`
//! are thin wrappers over the `chat_*` functions here.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use iroh::endpoint::{Accepting, Connection};
use iroh_blobs::Hash;
use loro::VersionVector;
use parking_lot::{Mutex, MutexGuard};
use proto::Msg;
use storage::Op;
use tokio::sync::{OnceCell, Semaphore, mpsc, oneshot};

use super::api::{Attachment, AttachmentKind, Chat, ChatEvents, DayPage, DeliveryState, Message, TransferState};
use super::blobs::{BlobHub, Gate, UploadEnd, UploadProgress};
use super::crypt;
use super::doc::*;
use super::store::*;
use super::wire::*;
use super::own::{ACK_MARK, READ_MARK};
use crate::Error;
use crate::node::{Inner, Me, addr_for, now, relay_hint, relay_of};

mod blobs;
mod history;
mod local;
mod read;
pub(crate) mod receipts;
mod session;
mod sync;
mod synth;
#[cfg(test)]
mod tests;

use crate::wire::DIAL_TIMEOUT;
const AUTH_TIMEOUT: Duration = Duration::from_secs(15);
const HELLO_DAYS: i64 = 7;
const MAX_HELLO_SHARDS: usize = 64;
/// A day this old (or older) is compacted into a snapshot blob.
const CLOSE_AFTER_DAYS: i64 = 8;
const UPDATES_BEFORE_SNAPSHOT: u32 = 64;
const MAX_SHARDS_CACHED: usize = 12;

/// Retry delays after a failed fetch while we stay connected; then the blob is marked failed
/// ("Tap to retry"). A reconnect restarts wanted blobs regardless.
const RETRY_BACKOFF: [u64; 5] = [2, 5, 15, 30, 60];

/// One running download (a fetch, or the pause before its next retry).
pub(crate) struct Dl {
    done: u64,
    total: u64,
    /// Sleeping before a retry: a new `chat_download` replaces it at once.
    waiting: bool,
    seq: u64,
    abort: Option<tokio::task::AbortHandle>,
}

/// The shard cache, locked (`chat_shard`).
pub(crate) type ShardCache<'c> = MutexGuard<'c, HashMap<(String, String), Shard>>;

pub struct ChatCore {
    pub store: ChatStore,
    pub dir: PathBuf,
    hub: OnceCell<Arc<BlobHub>>,
    pub(crate) shards: Mutex<HashMap<(String, String), Shard>>,
    /// Live sessions by the contact's device (a person's several devices may each hold one).
    sessions: Mutex<HashMap<[u8; 32], Arc<Session>>>,
    dialing: Mutex<HashSet<String>>,
    downloading: Mutex<HashMap<String, Dl>>,
    next_dl: AtomicU64,
    pulling: Mutex<HashSet<(String, String)>>,
    dial_sem: Arc<Semaphore>,
    next_session: AtomicU64,
    pub(crate) presence: crate::chat::presence::Presence,
    /// Signed recipient delivered/read receipts over this store (M3).
    pub(crate) receipts: Arc<crate::receipts::store::messages::MessageReceipts>,
    /// This install's device ID (U16; `ProfileV2::device_id`): what every version we write
    /// carries as `dev`.
    device_id: [u8; 32],
    /// Our writer ID: `writer_id(our DID, value)`, the Loro peer id we write chat docs with.
    pub(crate) wid: u64,
    /// Test knob: the next this-many `chat_persist` calls fail, as if the disk were full.
    pub(crate) fail_writes: std::sync::atomic::AtomicU32,
    /// Test knob: this node's chat clock runs this many ms ahead of the real one (per node,
    /// unlike `P2P_CHAT_CLOCK_MS_OFFSET`).
    pub(crate) clock_ms: std::sync::atomic::AtomicI64,
    /// With `clock_ms`: the device clock itself is wrong (the live-window and catch-up checks
    /// use it too), not just the time a message is stamped with.
    pub(crate) clock_wrong: std::sync::atomic::AtomicBool,
}

impl ChatCore {
    /// Asks every session to close (each ends its own loop).
    pub(crate) fn close_all_sessions(&self) {
        for s in self.sessions.lock().values() {
            let _ = s.tx.send(Cmd::Close);
        }
    }
}

pub enum Cmd {
    Push(String),
    Ack(String),
    History(String, u32),
    Profile,
    /// Send the receipt-ledger ops of this day the contact device lacks (chat/6).
    Receipts(String),
    Close,
}

pub struct Session {
    id: u64,
    pub did: String,
    pub device: [u8; 32],
    epoch: u64,
    profile_vv: Mutex<VersionVector>,
    dialer: [u8; 32],
    tx: mpsc::UnboundedSender<Cmd>,
    peer_vv: Mutex<HashMap<String, VersionVector>>,
    /// Receipt-ledger version the contact device holds, per day (its `ReceiptHello`/`ReceiptAck`,
    /// advanced optimistically by what we sent).
    receipt_vv: Mutex<HashMap<String, VersionVector>>,
    hist: Mutex<Option<oneshot::Sender<Vec<HistDay>>>>,
    /// What this contact device last said about itself (presence).
    pub(crate) presence: loro::awareness::EphemeralStore,
}

/// `dm/{pair}/{day}` -> day, if it names the conversation `pair`. A contact's session can only
/// ever name its own conversation, so it can neither request nor push the "You" docs.
pub(crate) fn doc_day_of(pair: &str, doc: &str) -> Result<String, Error> {
    let rest = doc.strip_prefix("dm/").ok_or_else(|| Error::Protocol("bad doc".into()))?;
    let (p, day) = rest.split_once('/').ok_or_else(|| Error::Protocol("bad doc".into()))?;
    if p != pair || !day_valid(day) {
        return Err(Error::Protocol("doc is not part of this conversation".into()));
    }
    Ok(day.to_string())
}

fn day_valid(day: &str) -> bool {
    day_start(day).is_some()
}

pub(super) fn decode_vv(b: &[u8]) -> Result<VersionVector, Error> {
    VersionVector::decode(b).map_err(|_| Error::Protocol("bad version vector".into()))
}

fn preview(rec: &MsgRec) -> String {
    if rec.deleted {
        return "Message deleted".into();
    }
    if !rec.text.is_empty() {
        return rec.text.chars().take(120).collect();
    }
    match &rec.file {
        Some(f) if f.kind == "voice" => "Voice message".into(),
        Some(f) => f.name.clone(),
        None => String::new(),
    }
}

fn parse_hash(h: &str) -> Result<Hash, Error> {
    h.parse::<Hash>().map_err(|_| Error::Protocol("bad blob hash".into()))
}

fn parse_key(k: &str) -> Result<[u8; 32], Error> {
    hex::decode(k).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()).ok_or_else(|| Error::Protocol("bad blob key".into()))
}

fn random_msg_id() -> String {
    let mut b = [0u8; 16];
    cryptography::rng::fill(&mut b);
    hex::encode(b)
}

/// The chat clock. `P2P_CHAT_CLOCK_MS_OFFSET` shifts it (a test knob for the day-shard tests).
fn now_ms() -> i64 {
    static OFFSET: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    let off = *OFFSET.get_or_init(|| std::env::var("P2P_CHAT_CLOCK_MS_OFFSET").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0) + off
}


pub(crate) fn merge_vv(into: &mut VersionVector, other: &VersionVector) {
    for (p, c) in other.iter() {
        let cur = into.get(p).copied().unwrap_or(0);
        if *c > cur {
            into.insert(*p, *c);
        }
    }
}
