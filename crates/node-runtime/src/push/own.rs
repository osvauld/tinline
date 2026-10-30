//! Own devices wake each other (`docs/push.md` "Own devices", 2026-10-29).
//!
//! A phone in push mode sleeps; until now only contacts held its wake tokens, so a change that
//! reached the account's computer (a contact's new picture, a message read there) waited on the
//! phone until it happened to start. Now every device with a push target also issues one `msg`
//! token per other live own device, and hands it over on the own-device link
//! (`SelfMsg::OwnWake`, sync-runtime, after the `cap/own-wake/v1` marker of both Hellos). A device
//! whose sync dial of an own device fails with a change owed wakes that device once, with the
//! usual `msg` throttle (`wake::msg_wake_due`), and the woken device dials its own devices
//! ([`NodeCtx::on_wake`] -> [`NodeCtx::own_woken`]).
//!
//! Bookkeeping in the one `WakeState`:
//! - Issued: keyed by [`own_key`] (`self:<device hex>`), so each own device has its own tag and
//!   an unlinked device's token alone is taken back (revoked at the relay) when the registry
//!   tombstones it.
//! - Held: under our own DID and the device that gave it, so the throttle and the 410/403
//!   handling are the contact ones.
//!
//! Content-free like every wake: the relay sees a token, the push a sealed tag.

use super::*;
use crate::Me;

/// Prefix of the `WakeState::issued` keys of own devices (never a DID: those start `did:`).
pub const OWN_KEY_PREFIX: &str = "self:";

/// The `issued` key of the token we gave own device `device`.
pub fn own_key(device: &[u8; 32]) -> String {
    format!("{OWN_KEY_PREFIX}{}", hex::encode(device))
}

/// The own device an `issued` key names; `None` for a contact's DID.
pub fn own_key_device(key: &str) -> Option<[u8; 32]> {
    hex::decode(key.strip_prefix(OWN_KEY_PREFIX)?)
        .ok()?
        .try_into()
        .ok()
}

/// `device` is another live (not removed) device of this account.
pub(super) fn live_own(st: &State, me_device: &[u8; 32], device: &[u8; 32]) -> bool {
    device != me_device
        && st
            .registry
            .iter()
            .any(|e| e.device == *device && !e.removed && !e.rotated)
}

/// One `msg` token per other live own device (never call: an own device does not ring us).
pub(super) fn own_wanted(st: &State, me_device: &[u8; 32]) -> Vec<(String, Kinds)> {
    st.registry
        .iter()
        .filter(|e| live_own(st, me_device, &e.device))
        .map(|e| (own_key(&e.device), Kinds::MSG))
        .collect()
}

/// Takes back the tokens of own devices that left the registry (revoked at the relay) and drops
/// the tokens they gave us. Whether anything changed.
pub(super) fn forget_gone_own(st: &mut State, me: &Me) -> bool {
    let Some(w) = &st.wake else { return false };
    let gone: Vec<String> = w
        .issued
        .iter()
        .filter(|e| own_key_device(&e.did).is_some_and(|d| !live_own(st, &me.device, &d)))
        .map(|e| e.did.clone())
        .collect();
    let dead: Vec<[u8; 32]> = w
        .held
        .iter()
        .filter(|e| e.did == me.id.did() && !live_own(st, &me.device, &e.device))
        .map(|e| e.device)
        .collect();
    let changed = !gone.is_empty() || !dead.is_empty();
    if let Some(w) = st.wake.as_mut() {
        for key in &gone {
            w.forget_contact(key);
        }
        for d in &dead {
            w.drop_held(me.id.did(), d);
        }
    }
    changed
}

impl NodeCtx {
    /// The token this device issued for own device `device` (`token`, `exp_ms`), to hand over on
    /// the own-device link; `None` without a push target or before the relay sealed it.
    pub fn own_wake_token_for(&self, device: &[u8; 32]) -> Option<(String, u64)> {
        let s = self.shared.lock();
        s.state
            .wake
            .as_ref()
            .filter(|w| w.target.is_some())?
            .issued_for(&own_key(device))
            .map(|i| (i.token.clone(), i.exp_ms))
    }

    /// Own device `device` handed us its token for it (`None`: it has none, drop what we hold).
    /// Kept only from a live own device; saved when it changed. Whether it was taken.
    pub fn own_wake_token_in(
        self: &Arc<Self>,
        device: [u8; 32],
        token: Option<(String, u64)>,
    ) -> bool {
        let Ok(me) = self.me() else { return false };
        let (taken, changed) = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch || !live_own(&s.state, &me.device, &device) {
                return false;
            }
            let did = me.id.did();
            match token {
                None => {
                    let dropped = s
                        .state
                        .wake
                        .as_mut()
                        .is_some_and(|w| w.drop_held(did, &device));
                    (true, dropped)
                }
                Some((token, exp_ms)) => {
                    let w = wake_mut(&mut s.state);
                    let same = w.held.iter().any(|e| {
                        e.did == did
                            && e.device == device
                            && e.held.token == token
                            && e.held.exp_ms == exp_ms
                    });
                    if same {
                        (true, false)
                    } else {
                        let ok = w.hold(did, device, token, exp_ms, Kinds::MSG, now_ms());
                        (ok, ok)
                    }
                }
            }
        };
        if changed {
            tracing::info!(
                "own wake token from device {}: {}",
                crate::short(&hex::encode(device)),
                if taken { "kept" } else { "refused" }
            );
            self.persist_in_background(me.epoch);
        }
        taken
    }

    /// Whether we hold a live `msg` token for own device `device`.
    pub fn can_wake_own(&self, device: &[u8; 32]) -> bool {
        let Ok(me) = self.me() else { return false };
        let s = self.shared.lock();
        let now = now_ms();
        live_own(&s.state, &me.device, device)
            && s.state.wake.as_ref().is_some_and(|w| {
                w.held.iter().any(|e| {
                    e.did == me.id.did()
                        && e.device == *device
                        && e.held.exp_ms > now
                        && e.held.kinds.allows(Kind::Msg)
                })
            })
    }

    /// Own device `device` is owed a change and our dial did not reach it: one content-free `msg`
    /// wake, if we hold its token and the throttle (`wake::msg_wake_due`) allows one now.
    /// `true`: the relay accepted it (202).
    pub async fn wake_own_device(self: &Arc<Self>, device: [u8; 32]) -> bool {
        let Ok(me) = self.me() else { return false };
        let did = me.id.did().to_string();
        let token = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch || !live_own(&s.state, &me.device, &device) {
                return false;
            }
            match s
                .state
                .wake
                .as_mut()
                .and_then(|w| w.wake_for(&did, &device, Kind::Msg, now_ms()))
            {
                Some(t) => t,
                None => return false,
            }
        };
        let status = self.wake_relay().wake(token, Kind::Msg).await;
        {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return false;
            }
            if let Some(w) = s.state.wake.as_mut() {
                w.wake_result(&did, &device, Kind::Msg, status);
            }
        }
        tracing::info!(
            "wake Msg sent for own device {}: relay status {status} (0: not reached)",
            crate::short(&hex::encode(device))
        );
        self.persist_in_background(me.epoch);
        status == 202
    }

    /// An own-device session with `device` came up: its wake schedule starts over.
    pub fn own_reached(self: &Arc<Self>, device: &[u8; 32]) {
        let Ok(me) = self.me() else { return };
        let changed = {
            let mut s = self.shared.lock();
            s.epoch == me.epoch
                && s.state
                    .wake
                    .as_mut()
                    .is_some_and(|w| w.reached_device(me.id.did(), device))
        };
        if changed {
            self.persist_in_background(me.epoch);
        }
    }

    /// An own device woke us: dial our own devices now (the backoff and what each is known to
    /// hold are forgotten), and retry pending pictures at once. Before the endpoint is up the
    /// start does both by itself.
    pub fn own_woken(self: &Arc<Self>) {
        if self.shared.lock().endpoint.is_none() {
            return;
        }
        self.sync_kick();
        if let Some(h) = self.hooks() {
            h.profile_retry_now();
        }
    }
}
