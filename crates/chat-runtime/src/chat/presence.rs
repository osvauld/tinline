//! Contact presence: who is reachable right now. Each device keeps a Loro `EphemeralStore`
//! with one key, refreshed every `HEARTBEAT`, and sends it as a QUIC datagram on every chat
//! session. The receiver applies it into a store of that session alone, so a contact can only
//! ever speak for its own device. Nothing is stored or retried: an entry not refreshed within
//! `TIMEOUT_MS` expires, and a closed session takes its store with it.
//!
//! Datagrams, not a `ChatMsg` variant: an older app neither reads them nor breaks on them (it
//! just shows the contact as not connected).
//!
//! Consent-based (decided 2026-10-17): we send presence on a session only while the user here
//! is active with that contact (typed in the conversation, `Node::chat_typing`, or wrote to it
//! within `wire::idle_close()`). A contact who dials only to deliver a message therefore learns
//! nothing beyond that delivery, and "not online" means unknown, not away. Sessions with no
//! `ChatMsg` traffic and none of our own reasons to stay close after the idle time; a contact
//! cannot ask us to keep one open. (An older `tk1` keep datagram from a peer is ignored: no
//! `tp1` prefix.)

use std::collections::HashSet;
use std::time::Duration;

use bytes::Bytes;
use iroh::endpoint::Connection;
use loro::awareness::EphemeralStore;
use parking_lot::Mutex;

use super::engine::{ChatCore, Session};
use crate::ChatRt;

pub const HEARTBEAT: Duration = Duration::from_secs(8);
/// Sender clock vs ours: the store drops entries stamped older than this, so it also bounds the
/// clock skew presence tolerates.
pub const TIMEOUT_MS: i64 = 30_000;
const MAGIC: &[u8] = b"tp1";
const KEY: &str = "online";
/// Our one key encodes to a few dozen bytes; anything much bigger is not presence.
const MAX_DATAGRAM: usize = 512;

/// Ours (sent) and which contacts we last reported online.
pub struct Presence {
    local: EphemeralStore,
    online: Mutex<HashSet<String>>,
}

impl Presence {
    pub fn new() -> Self {
        Self { local: EphemeralStore::new(TIMEOUT_MS), online: Default::default() }
    }
}

/// What a contact's device told us about itself.
pub fn session_store() -> EphemeralStore {
    EphemeralStore::new(TIMEOUT_MS)
}

/// A presence datagram's payload, if `d` is one.
pub fn parse(d: &[u8]) -> Option<&[u8]> {
    if d.len() > MAX_DATAGRAM {
        return None;
    }
    d.strip_prefix(MAGIC)
}

impl ChatRt {
    /// Refreshes our entry and sends it on `conn`, but only while the user here is active
    /// with `did` (the consent rule above); otherwise sends nothing. Best effort: a lost
    /// datagram is replaced by the next heartbeat.
    pub fn presence_send(&self, core: &ChatCore, did: &str, conn: &Connection) {
        // A live call counts as activity: keep the window fresh while it lasts.
        if self.in_call_with(did) {
            core.mark_active(did, None);
        }
        if !core.locally_active(did) {
            return;
        }
        core.presence.local.set(KEY, true);
        let mut d = MAGIC.to_vec();
        d.extend_from_slice(&core.presence.local.encode(KEY));
        let _ = conn.send_datagram(Bytes::from(d));
    }

    pub fn presence_receive(&self, core: &ChatCore, sess: &Session, d: &[u8]) {
        let Some(payload) = parse(d) else { return };
        if sess.presence.apply(payload).is_ok() {
            self.presence_update(core, &sess.did);
        }
    }

    /// Recomputes whether `did` is online (any of its devices with a live entry) and reports a
    /// change.
    pub fn presence_update(&self, core: &ChatCore, did: &str) {
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

    pub fn presence_of(&self, did: &str) -> bool {
        self.chat_core().is_ok_and(|core| core.presence.online.lock().contains(did))
    }
}
