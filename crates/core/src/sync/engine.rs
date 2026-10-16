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

use super::accdoc::{AccDoc, Ctx, DOC_NAME};
use super::callsdoc::{CALLS_DOC, CallsDoc};
use super::doc::Doc;
use super::frames::{FrameReader, FrameWriter, SelfMsg};
use super::profilecache::PROFILE_CACHE_DOC;
use super::profiledoc::PROFILE_DOC;
use crate::Error;
use crate::chat::own::{ACK_MARK, GROUPS_MARK, MAX_OWN_DOCS, READ_MARK, parse_doc};
use crate::group::own::contents as group_documents;
use crate::node::{Inner, Me, addr_for, now, relay_of};
use crate::store::{Backing, OwnDevice};

mod account;
mod devices;
mod group_contents;
mod group_history;
mod group_receipts;
mod push;
mod sessions;

const DOC_KEY: &str = "sync/account";
const CALLS_KEY: &str = "sync/calls";
const AUTH_TIMEOUT: Duration = Duration::from_secs(15);
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);

/// One live session with another own device.
pub(crate) struct SelfSlot {
    dialer: [u8; 32],
    conn: Connection,
    /// What the peer has confirmed it holds of the account doc (its `Hello`s and batches), not
    /// merely what was written to the stream: an unlink closes the endpoint right after.
    pub(crate) acked: Acked,
    /// Docs (and `READ_MARK`) changed locally that this session has not looked at yet (34g).
    pub(crate) dirty: Dirty,
}

type Dirty = Arc<parking_lot::Mutex<HashSet<String>>>;
type Acked = Arc<parking_lot::Mutex<Option<VersionVector>>>;

/// What the other device is known to hold of the chat shards and read cursors.
#[derive(Default)]
struct PeerChat {
    docs: HashMap<String, VersionVector>,
    read: HashMap<String, i64>,
    acks: HashMap<String, VersionVector>,
    hello: bool,
    /// Only negotiated on the authenticated own-device connection, not membership.
    group_contents: bool,
    history: group_history::History,
    receipts: group_receipts::Receipts,
    /// The group list last sent (G2), to send only changes.
    groups: Option<Vec<Vec<u8>>>,
    /// The invitation rows last sent (G3).
    invitations: Option<Vec<Vec<u8>>>,
}

impl SelfSlot {
    pub(crate) fn close(&self) {
        self.conn.close(0u32.into(), b"unlinked");
    }
}

#[derive(Default)]
pub(crate) struct SelfSync {
    pub sessions: parking_lot::Mutex<HashMap<[u8; 32], SelfSlot>>,
    /// Per device: failed attempts and when to try next.
    pub backoff: parking_lot::Mutex<HashMap<[u8; 32], (u32, Instant)>>,
    pub dialing: parking_lot::Mutex<HashSet<[u8; 32]>>,
    pub kick: tokio::sync::Notify,
    /// This device is unlinking itself: its own tombstone coming back in a batch must not remove
    /// the account before `unlink_device` has pushed that tombstone to the others.
    pub leaving: std::sync::atomic::AtomicBool,
}

fn sync_period() -> Duration {
    Duration::from_secs(
        std::env::var("P2P_SELF_SYNC_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15),
    )
}

fn chat_sig_ok(device: &[u8; 32], doc: &str, update: &[u8], sig: &[u8]) -> bool {
    crate::chat::wire::verify_batch(device, doc, update, sig)
}

pub(crate) fn ensure_self_registered(
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
