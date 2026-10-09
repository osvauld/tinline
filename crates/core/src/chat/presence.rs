//! Contact presence: who is reachable right now. Each device keeps a Loro `EphemeralStore`
//! with one key, refreshed every `HEARTBEAT`, and sends it as a QUIC datagram on every chat
//! session. The receiver applies it into a store of that session alone, so a contact can only
//! ever speak for its own device. Nothing is stored or retried: an entry not refreshed within
//! `TIMEOUT_MS` expires, and a closed session takes its store with it.
//!
//! Datagrams, not a `ChatMsg` variant: an older app neither reads them nor breaks on them (it
//! just shows the contact as not connected).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

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
    /// Refreshes our entry and sends it on `conn`. Best effort: a lost datagram is replaced by
    /// the next heartbeat.
    pub(crate) fn presence_send(&self, core: &ChatCore, conn: &Connection) {
        core.presence.local.set(KEY, true);
        let mut d = MAGIC.to_vec();
        d.extend_from_slice(&core.presence.local.encode(KEY));
        let _ = conn.send_datagram(Bytes::from(d));
    }

    pub(crate) fn presence_receive(&self, core: &ChatCore, sess: &Session, d: &[u8]) {
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
    /// known rather than "no session yet".
    pub(crate) fn presence_watch(self: &Arc<Self>, did: &str) {
        self.chat_kick(did.to_string());
    }
}
