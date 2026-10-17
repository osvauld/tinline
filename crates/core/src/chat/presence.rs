//! Contact presence: who is reachable right now. Each device keeps a Loro `EphemeralStore`
//! with one key, refreshed every `HEARTBEAT`, and sends it as a QUIC datagram on every chat
//! session. The receiver applies it into a store of that session alone, so a contact can only
//! ever speak for its own device. Nothing is stored or retried: an entry not refreshed within
//! `TIMEOUT_MS` expires, and a closed session takes its store with it.
//!
//! Datagrams, not a `ChatMsg` variant: an older app neither reads them nor breaks on them (it
//! just shows the contact as not connected).
//!
//! Sessions are not held open (decided 2026-10-17): one with no `ChatMsg` traffic for
//! `wire::idle_close()` closes, unless a side wants it kept. The second datagram kind,
//! `tk1` + one byte (1 = keep, 0 = no need), says so on every heartbeat and whenever it changes:
//! the sender is watching the conversation, has something pending for this contact, or is in a
//! call with them. It expires after `KEEP_TTL` without a refresh. Older apps ignore it (no `tp1`
//! prefix) and never close idle sessions themselves. Presence is therefore known only while a
//! session is up, which in practice means while the conversation is open.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use iroh::endpoint::Connection;
use loro::awareness::EphemeralStore;
use parking_lot::Mutex;

use super::engine::{ChatCore, Session};
use crate::node::Inner;

pub(crate) const HEARTBEAT: Duration = Duration::from_secs(8);
/// Sender clock vs ours: the store drops entries stamped older than this, so it also bounds the
/// clock skew presence tolerates.
pub(crate) const TIMEOUT_MS: i64 = 30_000;
const MAGIC: &[u8] = b"tp1";
/// The keep datagram: `KEEP_MAGIC` + 1 byte.
const KEEP_MAGIC: &[u8] = b"tk1";
/// A keep request not refreshed within this lapses (a few lost heartbeats).
const KEEP_TTL: Duration = Duration::from_secs(30);
const KEY: &str = "online";
/// Our one key encodes to a few dozen bytes; anything much bigger is not presence.
const MAX_DATAGRAM: usize = 512;

/// Ours (sent) and which contacts we last reported online.
pub(crate) struct Presence {
    local: EphemeralStore,
    online: Mutex<HashSet<String>>,
}

impl Presence {
    pub(crate) fn new() -> Self {
        Self { local: EphemeralStore::new(TIMEOUT_MS), online: Default::default() }
    }
}

/// What a contact's device told us about itself.
pub(crate) fn session_store() -> EphemeralStore {
    EphemeralStore::new(TIMEOUT_MS)
}

/// A presence datagram's payload, if `d` is one.
pub(crate) fn parse(d: &[u8]) -> Option<&[u8]> {
    if d.len() > MAX_DATAGRAM {
        return None;
    }
    d.strip_prefix(MAGIC)
}

impl Inner {
    /// Refreshes our entry and sends it on `conn`, with our keep request for this session. Best
    /// effort: a lost datagram is replaced by the next heartbeat.
    pub(crate) fn presence_send(&self, core: &ChatCore, sess: &Session, conn: &Connection) {
        core.presence.local.set(KEY, true);
        let mut d = MAGIC.to_vec();
        d.extend_from_slice(&core.presence.local.encode(KEY));
        let _ = conn.send_datagram(Bytes::from(d));
        self.keep_send(core, sess, conn);
    }

    /// Tells the contact device whether we want this session kept open.
    pub(crate) fn keep_send(&self, core: &ChatCore, sess: &Session, conn: &Connection) {
        let keep = self.chat_keep_local(core, &sess.did);
        let mut d = KEEP_MAGIC.to_vec();
        d.push(keep as u8);
        let _ = conn.send_datagram(Bytes::from(d));
    }

    pub(crate) fn presence_receive(&self, core: &ChatCore, sess: &Session, d: &[u8]) {
        if let Some(rest) = d.strip_prefix(KEEP_MAGIC) {
            let keep = rest.first().is_some_and(|b| *b == 1);
            *sess.peer_keep.lock() = keep.then(|| Instant::now() + KEEP_TTL);
            return;
        }
        let Some(payload) = parse(d) else { return };
        if sess.presence.apply(payload).is_ok() {
            self.presence_update(core, &sess.did);
        }
    }

    /// Recomputes whether `did` is online (any of its devices with a live entry) and reports a
    /// change.
    pub(crate) fn presence_update(&self, core: &ChatCore, did: &str) {
        let online = self.chat_sessions_of(core, did).iter().any(|s| {
            s.presence.remove_outdated();
            s.presence.get(KEY).is_some()
        });
        let changed = {
            let mut set = core.presence.online.lock();
            if online { set.insert(did.to_string()) } else { set.remove(did) }
        };
        if changed && let Some(ev) = self.ev() {
            ev.on_presence_changed(did.to_string(), online);
        }
    }

    pub(crate) fn presence_of(&self, did: &str) -> bool {
        self.chat_core().is_ok_and(|core| core.presence.online.lock().contains(did))
    }

    /// Opening a conversation: make sure a session is up (or being dialled), so presence is
    /// known rather than "no session yet", and keep it up until `presence_unwatch`.
    pub(crate) fn presence_watch(self: &Arc<Self>, did: &str) {
        let Ok(core) = self.chat_core() else { return };
        core.watched.lock().insert(did.to_string());
        for s in self.chat_sessions_of(&core, did) {
            s.cmd(super::engine::Cmd::Keep);
        }
        self.chat_kick(did.to_string());
    }

    /// The conversation closed: the session may idle out again (and the contact device is told
    /// at once that we no longer need it).
    pub(crate) fn presence_unwatch(&self, did: &str) {
        let Ok(core) = self.chat_core() else { return };
        if !core.watched.lock().remove(did) {
            return;
        }
        for s in self.chat_sessions_of(&core, did) {
            s.cmd(super::engine::Cmd::Keep);
        }
    }
}
