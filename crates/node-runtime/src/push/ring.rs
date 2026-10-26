//! The receiving side of a wake (`docs/push.md` slice 5): [`NodeCtx::on_wake`] and the "wake
//! ring", the ring a sleeping phone shows from a `call` wake before the caller's `CallHello` has
//! arrived.
//!
//! - `msg` wake: dial back (`chat_kick`, the path a local send takes). If the node is not up yet
//!   the contact is queued and dialled when it starts.
//! - `call` wake: B never dials. A ring is kept per contact for [`WAKE_RING_WINDOW_MS`] from the
//!   wake's arrival. A repeat wake extends the window from itself (never a second ring), capped
//!   at [`WAKE_RING_CAP_MS`] in total. The real `CallHello` consumes the ring silently (the
//!   normal `on_incoming_call` follows); if the user answered early, that call is accepted the
//!   moment it arrives. No `CallHello` in time: the ring ends, reported as
//!   `on_call_state("wake:<did>", Ended { "cancelled" })`.
//!
//! The hint and the DID are never logged above debug.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use wake::WakeAction;

use crate::{CallState, Error, NodeCtx, now_ms};

/// How long a wake ring waits for the caller's `CallHello`, from the wake's arrival
/// (PROVISIONAL: the caller may still be redialling for ~36 s after pressing Call, and repeat
/// call wakes arrive about every 10 s).
pub const WAKE_RING_WINDOW_MS: u64 = 15_000;
/// A ring kept alive by repeat wakes ends at the latest this long after the first one.
pub const WAKE_RING_CAP_MS: u64 = 40_000;

/// What the user decided before the `CallHello` arrived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Early {
    /// Still ringing: the incoming call rings as usual.
    Undecided,
    /// Answered: the incoming call is accepted when it arrives ("Connecting…").
    Answered,
    /// Declined: the incoming call is declined when it arrives, without ringing.
    Declined,
}

pub(super) struct Ring {
    first: Instant,
    expires: Instant,
    early: Early,
}

/// The wake rings of a node and the dial-backs queued for its start.
#[derive(Default)]
pub struct Rings {
    pub(super) map: Mutex<HashMap<String, Ring>>,
    /// Contacts a `msg` wake asked us to dial before the endpoint was up.
    dial_backs: Mutex<BTreeSet<String>>,
    /// Overrides (ms) for tests; 0 = the constants.
    window_ms: AtomicU64,
    cap_ms: AtomicU64,
}

impl Rings {
    fn window(&self) -> Duration {
        Duration::from_millis(match self.window_ms.load(Ordering::Relaxed) {
            0 => WAKE_RING_WINDOW_MS,
            ms => ms,
        })
    }

    fn cap(&self) -> Duration {
        Duration::from_millis(match self.cap_ms.load(Ordering::Relaxed) {
            0 => WAKE_RING_CAP_MS,
            ms => ms,
        })
    }
}

/// The call id a wake ring is reported under in `on_call_state` (the app maps the real call by DID).
pub fn wake_call_id(did: &str) -> String {
    format!("wake:{did}")
}

impl NodeCtx {
    /// A push arrived. Opens the hint with our own key, decides (`WakeState::on_wake`: blocked in
    /// either direction, stale, revoked or unknown tags are refused) and acts:
    /// - `DialBack`: kicks the chat dial to that contact.
    /// - `Ring`: starts or extends the wake ring; no dial. A node that is `unavailable` refuses
    ///   like any call would (`IgnoreCall`).
    /// - `Ignore` / `IgnoreCall`: nothing.
    ///
    /// `Err(Locked | NoIdentity)` while no account is open: the hint cannot even be opened, so the
    /// kind is unknown too.
    pub fn on_wake(self: &Arc<Self>, hint: &str) -> Result<WakeAction, Error> {
        let me = self.me()?;
        let action = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return Err(Error::Locked);
            }
            let st = &mut s.state;
            let blocked: std::collections::HashSet<String> =
                st.blocked.iter().cloned().chain(st.notices.keys().filter(|d| crate::block::blocked_me(st, d)).cloned()).collect();
            match st.wake.as_mut() {
                None => WakeAction::Ignore,
                Some(w) => w.on_wake(hint, now_ms(), |did| blocked.contains(did)),
            }
        };
        match &action {
            WakeAction::DialBack { did } => {
                tracing::info!("wake received: outcome DialBack");
                self.wake_dial_back(did);
            }
            WakeAction::Ring { did } => {
                if self.availability().available {
                    tracing::info!("wake received: outcome Ring");
                    self.wake_ring(did);
                } else {
                    // The call itself would be turned away the same way.
                    tracing::info!("wake received: outcome EndCall (not available for calls)");
                    return Ok(WakeAction::IgnoreCall);
                }
            }
            WakeAction::CodeWake => {
                // A stranger holding our code: nobody to dial. The app keeps the node up for the
                // usual window; their hello arrives as an ordinary contact request.
                tracing::info!("wake received: outcome CodeWake");
                // The code throttle outlives a restart.
                self.persist_in_background(me.epoch);
            }
            WakeAction::Ignore => tracing::info!("wake received: outcome Ignore"),
            WakeAction::IgnoreCall => tracing::info!("wake received: outcome EndCall"),
        }
        Ok(action)
    }

    /// The name of `did` for the ring: the local alias, else the name they gave; blank when unknown.
    pub fn wake_label(&self, did: &str) -> String {
        self.shared.lock().state.contacts.iter().find(|c| c.did == did).map(crate::display_name).unwrap_or_default()
    }

    fn wake_dial_back(self: &Arc<Self>, did: &str) {
        let up = self.shared.lock().endpoint.is_some();
        match (up, self.hooks()) {
            (true, Some(h)) => h.chat_kick(did.to_string()),
            // Not started yet: `wake_started` dials when it is.
            _ => {
                self.push.rings.dial_backs.lock().insert(did.to_string());
            }
        }
    }

    /// After start: the dial-backs asked for while the node was down.
    pub(super) fn wake_dial_backs_started(self: &Arc<Self>) {
        let queued = std::mem::take(&mut *self.push.rings.dial_backs.lock());
        if let Some(h) = self.hooks() {
            for did in queued {
                h.clone().chat_kick(did);
            }
        }
    }

    /// Overrides the ring window and cap (tests); `0` restores a default.
    pub fn set_wake_ring_timing(&self, window_ms: u64, cap_ms: u64) {
        self.push.rings.window_ms.store(window_ms, Ordering::Relaxed);
        self.push.rings.cap_ms.store(cap_ms, Ordering::Relaxed);
    }

    /// Whether a wake ring for `did` is open now.
    pub fn wake_ringing(&self, did: &str) -> bool {
        self.push.rings.map.lock().get(did).is_some_and(|r| r.expires > Instant::now())
    }

    fn wake_ring(self: &Arc<Self>, did: &str) {
        let now = Instant::now();
        let rings = &self.push.rings;
        let spawn = {
            let mut map = rings.map.lock();
            match map.get_mut(did) {
                Some(r) if r.expires > now => {
                    // One ring per contact: a repeat wake moves the end out, up to the cap.
                    r.expires = r.expires.max((now + rings.window()).min(r.first + rings.cap()));
                    false
                }
                _ => {
                    map.insert(did.to_string(), Ring { first: now, expires: now + rings.window(), early: Early::Undecided });
                    true
                }
            }
        };
        if spawn {
            let (this, did) = (self.clone(), did.to_string());
            self.handle.spawn(async move { this.wake_ring_timer(did).await });
        }
    }

    async fn wake_ring_timer(self: Arc<Self>, did: String) {
        loop {
            let Some(end) = self.push.rings.map.lock().get(&did).map(|r| r.expires) else { return };
            tokio::time::sleep_until(end.into()).await;
            let ended = {
                let mut map = self.push.rings.map.lock();
                match map.get(&did) {
                    None => return,
                    Some(r) if r.expires > Instant::now() => continue,
                    Some(_) => map.remove(&did),
                }
            };
            self.wake_ring_ended(&did, ended);
            return;
        }
    }

    /// Tells the app a ring is over, unless the user already dismissed it themselves.
    fn wake_ring_ended(&self, did: &str, ring: Option<Ring>) {
        if ring.is_some_and(|r| r.early != Early::Declined) {
            self.sink.on_call_state(wake_call_id(did), CallState::Ended { reason: "cancelled".into() });
        }
    }

    /// The user answered the wake ring: the call is accepted when its `CallHello` arrives
    /// (within the window). `NotFound`: no open ring (it ended, or the call already arrived: use
    /// `answer` with its call id).
    pub fn answer_wake_ring(&self, did: &str) -> Result<(), Error> {
        let mut map = self.push.rings.map.lock();
        match map.get_mut(did) {
            Some(r) if r.expires > Instant::now() && r.early != Early::Declined => {
                r.early = Early::Answered;
                Ok(())
            }
            _ => Err(Error::NotFound),
        }
    }

    /// The user declined the wake ring: no `on_call_state` follows (the app dismissed it itself)
    /// and a call that arrives within the window is declined unrung.
    pub fn decline_wake_ring(&self, did: &str) -> Result<(), Error> {
        let mut map = self.push.rings.map.lock();
        match map.get_mut(did) {
            Some(r) if r.expires > Instant::now() => {
                r.early = Early::Declined;
                Ok(())
            }
            _ => Err(Error::NotFound),
        }
    }

    /// The caller's `CallHello` arrived: the ring is consumed silently. What the user decided
    /// meanwhile; `None` when there was no (open) ring.
    pub(crate) fn take_wake_ring(&self, did: &str) -> Option<Early> {
        let ring = self.push.rings.map.lock().remove(did)?;
        if ring.expires <= Instant::now() {
            // Too late: the ring was over (the timer has not run yet).
            self.wake_ring_ended(did, Some(ring));
            return None;
        }
        Some(ring.early)
    }
}
