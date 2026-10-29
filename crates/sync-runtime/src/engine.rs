//! Own-device sync: the account doc wired to `State`, and the `tinline/self/2` sessions.
//! See docs/protocol.md "Own-device sync".
//!
//! Shared session state lives here. `account` integrates persisted documents with local state;
//! `devices` manages dialing and unlink notifications; `sessions` authenticates peers and
//! dispatches incoming updates; `push` sends document changes and cursors. The `group_*` modules
//! retain negotiated group-content, history, and storage-receipt handling.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iroh::endpoint::{Accepting, Connection};
use loro::VersionVector;

use crate::accdoc::{AccDoc, DOC_NAME};
use crate::callsdoc::{CALLS_DOC, CallsDoc};
use crate::doc::Doc;
use crate::frames::{FrameReader, FrameWriter, SelfMsg};
use crate::profilecache::PROFILE_CACHE_DOC;
use crate::profiledoc::PROFILE_DOC;
use crate::Error;
use chat_runtime::chat::own::{ACK_MARK, GROUPS_MARK, MAX_OWN_DOCS, READ_MARK, parse_doc};
use group_runtime::own::contents as group_documents;
use crate::SyncRt;
use node_runtime::{Me, addr_for, now, relay_of};
use node_runtime::store::{Backing, OwnDevice};

pub mod account;
pub mod devices;
pub mod dm_receipts;
pub mod group_contents;
pub mod group_history;
pub mod group_receipts;
pub mod push;
pub mod sessions;

use node_runtime::{CALLS_KEY, DOC_KEY};
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(15);
pub const DIAL_TIMEOUT: Duration = Duration::from_secs(10);

/// One live session with another own device.
pub struct SelfSlot {
    pub dialer: [u8; 32],
    pub conn: Connection,
    /// What the peer has confirmed it holds of the account doc (its `Hello`s and batches), not
    /// merely what was written to the stream: an unlink closes the endpoint right after.
    pub acked: Acked,
    /// Docs (and `READ_MARK`) changed locally that this session has not looked at yet (34g).
    pub dirty: Dirty,
}

pub type Dirty = Arc<parking_lot::Mutex<HashSet<String>>>;
pub type Acked = Arc<parking_lot::Mutex<Option<VersionVector>>>;

/// What the other device is known to hold of the chat shards and read cursors.
#[derive(Default)]
pub struct PeerChat {
    pub docs: HashMap<String, VersionVector>,
    pub read: HashMap<String, i64>,
    pub acks: HashMap<String, VersionVector>,
    pub hello: bool,
    /// Only negotiated on the authenticated own-device connection, not membership.
    pub group_contents: bool,
    pub history: group_history::History,
    pub receipts: group_receipts::Receipts,
    /// Signed DM receipt ledgers (cap/dm-receipts/v1).
    pub dm_receipts: dm_receipts::DmReceipts,
    /// The group list last sent (G2), to send only changes.
    pub groups: Option<Vec<Vec<u8>>>,
    /// The invitation rows last sent (G3).
    pub invitations: Option<Vec<Vec<u8>>>,
}

impl node_runtime::OwnDocSink for SelfSync {
    fn mark_dirty(&self, name: &str) {
        for s in self.sessions.lock().values() {
            s.dirty.lock().insert(name.to_string());
        }
    }

    fn forget_progress(&self) {
        self.backoff.lock().clear();
        self.synced.lock().clear();
    }

    fn in_session(&self, devices: &[[u8; 32]]) -> Vec<bool> {
        let sessions = self.sessions.lock();
        devices.iter().map(|d| sessions.contains_key(d)).collect()
    }
}

impl SelfSlot {
    pub fn close(&self) {
        self.conn.close(0u32.into(), b"unlinked");
    }
}

#[derive(Default)]
pub struct SelfSync {
    pub sessions: parking_lot::Mutex<HashMap<[u8; 32], SelfSlot>>,
    /// Per device: failed attempts and when to try next.
    pub backoff: parking_lot::Mutex<HashMap<[u8; 32], (u32, Instant)>>,
    pub dialing: parking_lot::Mutex<HashSet<[u8; 32]>>,
    /// Per device: the change generation (`SyncRt::sync_tx`) it is known to hold everything up
    /// to, set when a session with it ends idle. A device below the current generation (or
    /// missing: after start or `sync_kick`) is owed a sync and gets dialled; others are not
    /// (sessions are not held open, 2026-10-17).
    pub synced: parking_lot::Mutex<HashMap<[u8; 32], u64>>,
    /// This device is unlinking itself: its own tombstone coming back in a batch must not remove
    /// the account before `unlink_device` has pushed that tombstone to the others.
    pub leaving: std::sync::atomic::AtomicBool,
}

pub fn sync_period() -> Duration {
    Duration::from_secs(
        std::env::var("P2P_SELF_SYNC_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15),
    )
}

pub fn chat_sig_ok(device: &[u8; 32], doc: &str, update: &[u8], sig: &[u8]) -> bool {
    chat_runtime::chat::wire::verify_batch(device, doc, update, sig)
}

pub fn ensure_self_registered(
    registry: &mut Vec<OwnDevice>,
    me: &Me,
    label: Option<String>,
) {
    if !registry.iter().any(|e| e.device == me.device) {
        registry.insert(
            0,
            OwnDevice {
                device: me.device,
                attestation: me.attestation.clone(),
                label: label.unwrap_or_default(),
                removed: false,
                last_seen: now(),
                relay: None,
                links: Vec::new(),
                rotated: false,
            },
        );
    }
}
