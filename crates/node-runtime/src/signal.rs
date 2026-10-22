//! How the node tells own-device sync that something changed, without knowing the sync engine.
//!
//! Local writers (`persist`, the profile, chat and group engines, devices and rotation) call
//! [`OwnDocSignal::bump`], [`OwnDocSignal::dirty`] or [`OwnDocSignal::kick`]; the sync engine owns
//! the per-session dirty sets and the dial backoff and plugs in through [`OwnDocSink`].

use std::sync::{Arc, Weak};

/// The own-doc dirty mark that means "our group list or a group's hosts may have changed" (G2).
pub const GROUPS_MARK: &str = "!groups";

use parking_lot::Mutex;
use tokio::sync::{Notify, watch};

/// The sync engine's side of [`OwnDocSignal`].
pub trait OwnDocSink: Send + Sync {
    /// Marks `name` (a doc, or a mark) as changed for every own-device session.
    fn mark_dirty(&self, name: &str);
    /// Forgets the dial backoff and what each device is known to hold (a kick).
    fn forget_progress(&self);
    /// Which of `devices` have an own-device session up right now.
    fn in_session(&self, devices: &[[u8; 32]]) -> Vec<bool>;
}

#[derive(Default)]
pub struct OwnDocSignal {
    /// Change generation: a device is owed a sync until a session with it ends idle at or above
    /// the generation it saw.
    tx: watch::Sender<u64>,
    /// Wakes the sessions and the dial loop.
    wake: Notify,
    sink: Mutex<Option<Weak<dyn OwnDocSink>>>,
}

impl OwnDocSignal {
    /// Registers the sync engine (p2pcore does it when building the node).
    pub fn set_sink(&self, sink: Weak<dyn OwnDocSink>) {
        *self.sink.lock() = Some(sink);
    }

    fn sink(&self) -> Option<Arc<dyn OwnDocSink>> {
        self.sink.lock().as_ref().and_then(Weak::upgrade)
    }

    /// Wakes the sessions (local data changed) and the dial loop: every other device is owed
    /// this change until a session with it ends idle after seeing it.
    pub fn bump(&self) {
        self.tx.send_modify(|v| *v += 1);
        self.wake.notify_one();
    }

    /// Marks `name` changed for every own-device session, then bumps. With no session, only the
    /// bump: the next session's hellos compare everything anyway.
    pub fn dirty(&self, name: &str) {
        if let Some(s) = self.sink() {
            s.mark_dirty(name);
        }
        self.bump();
    }

    /// Dial every other device now (start, network change, rotation, linking): forgets the
    /// backoff and what each device was known to hold.
    pub fn kick(&self) {
        if let Some(s) = self.sink() {
            s.forget_progress();
        }
        self.wake.notify_one();
    }

    /// Which of `devices` (own devices) are in sync with us right now; all false with no engine.
    pub fn in_session(&self, devices: &[[u8; 32]]) -> Vec<bool> {
        match self.sink() {
            Some(s) => s.in_session(devices),
            None => vec![false; devices.len()],
        }
    }

    /// The current change generation.
    pub fn generation(&self) -> u64 {
        *self.tx.borrow()
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.tx.subscribe()
    }

    /// Resolves at the next `bump` or `kick` (one stored wake-up, like `Notify::notified`).
    pub fn woken(&self) -> tokio::sync::futures::Notified<'_> {
        self.wake.notified()
    }
}

impl crate::NodeCtx {
    /// See [`OwnDocSignal::bump`].
    pub fn sync_bump(&self) {
        self.signal.bump();
    }

    /// See [`OwnDocSignal::kick`].
    pub fn sync_kick(&self) {
        self.signal.kick();
    }

    /// See [`OwnDocSignal::dirty`].
    pub fn selfsync_dirty(&self, name: &str) {
        self.signal.dirty(name);
    }
}
