//! `Node` API: push wake-ups (`docs/push.md`, UniFFI-exported). Thin wrappers over the node
//! runtime's `push` module; the logic and its tests live there.

use super::*;

/// Where this device can be woken (the platform push token). Mirrors the relay's targets.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PushTarget {
    /// Tinline (Play): the FCM registration token (`onNewToken`).
    Fcm { token: String },
    /// iOS: the APNs token for message wakes, the PushKit VoIP token for call wakes, and
    /// whether this is a development build (APNs sandbox host).
    Apns { alert: String, voip: Option<String>, sandbox: bool },
    /// Tinline Liberum: the UnifiedPush endpoint URL of the user's distributor.
    UnifiedPush { endpoint: String },
}

impl From<PushTarget> for node_runtime::push::Target {
    fn from(t: PushTarget) -> Self {
        match t {
            PushTarget::Fcm { token } => Self::Fcm { token },
            PushTarget::Apns { alert, voip, sandbox } => Self::Apns { alert, voip, sandbox },
            PushTarget::UnifiedPush { endpoint } => Self::UnifiedPush { endpoint },
        }
    }
}

impl From<node_runtime::push::Target> for PushTarget {
    fn from(t: node_runtime::push::Target) -> Self {
        use node_runtime::push::Target;
        match t {
            Target::Fcm { token } => Self::Fcm { token },
            Target::Apns { alert, voip, sandbox } => Self::Apns { alert, voip, sandbox },
            Target::UnifiedPush { endpoint } => Self::UnifiedPush { endpoint },
        }
    }
}

/// What the app does after a wake push, from `Node::on_wake`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum WakeOutcome {
    /// Nothing to do: not for us, revoked, a blocked contact, or garbage. (A message push on iOS
    /// may still show its generic banner, which the app cannot stop.)
    Ignore,
    /// A call wake that is refused (blocked, stale, not allowed, or we are unavailable). iOS must
    /// still report the call to CallKit and end it at once; Android shows nothing.
    EndCall,
    /// A message wake: we are dialling this contact now (the app starts the node if it has not).
    DialedBack { did: String },
    /// A call wake: ring for `did` now. `name` is the user's own name for the contact (their
    /// alias, else the name they gave), or a neutral label when unknown. The caller dials us; we
    /// do not dial. See `Node::on_wake` for how the ring ends.
    Ring { did: String, name: String },
}

/// What the ring shows when the contact has no name we can read.
const NEUTRAL_RING_NAME: &str = "Someone";

#[uniffi::export]
impl Node {
    /// Sets (or, with `None`, clears) the platform push token of this device. `Some`: contacts get
    /// wake tokens for it; `None` (always-on, no push) takes every token we issued back. Call it
    /// at every start and again whenever the platform hands out a new token (FCM `onNewToken`, a
    /// new APNs or VoIP token, a new UnifiedPush endpoint). Saved with the account; needs it
    /// unlocked. A malformed token is refused.
    pub fn set_push_target(&self, target: Option<PushTarget>) -> Result<(), Error> {
        self.inner.ctx.set_push_target(target.map(Into::into)).map_err(Error::from)
    }

    /// The push target set now, if any.
    pub fn push_target(&self) -> Option<PushTarget> {
        self.inner.ctx.push_target().map(Into::into)
    }

    /// A wake push arrived: `hint` is the opaque text the push carried (FCM data key `h`). Opens
    /// it with this account's own key and acts:
    /// - message wake: dials the contact back (queued if the node is not started yet);
    /// - call wake: `Ring { did, name }`. The app rings at once (CallKit, full-screen
    ///   notification) without dialling. When the caller's call arrives, the normal
    ///   `on_incoming_call` follows (the app matches it to the ring by `did`) and the ring is
    ///   consumed silently. If no call arrives within `WAKE_RING_WINDOW_MS` of the last wake
    ///   (15 s, extended by repeat wakes up to 40 s in all), the ring ends and the app is told
    ///   through the existing callback as `on_call_state("wake:<did>", Ended { reason: "cancelled" })`.
    ///   The `call_id` of a wake ring is always `"wake:"` + the contact's DID. Answering or
    ///   declining it goes through `answer_wake_ring` / `decline_wake_ring`, not `answer` / `decline`.
    ///
    /// Returns promptly (no network, no waiting for the node to start); call it before or after
    /// `start`. **Errors:** `Locked` / `NoIdentity` while no account is open: the hint cannot be
    /// opened, so neither the kind nor the contact is known. The app then shows a generic notice
    /// (a message) or, for an iOS VoIP push, reports a call to CallKit and ends it at once. iOS
    /// needs that answer within seconds of the push, so unlock first when the key is in the
    /// keychain (`unlock_with_key`).
    pub fn on_wake(&self, hint: String) -> Result<WakeOutcome, Error> {
        use node_runtime::push::WakeAction;
        Ok(match self.inner.ctx.on_wake(&hint)? {
            WakeAction::Ignore => WakeOutcome::Ignore,
            WakeAction::IgnoreCall => WakeOutcome::EndCall,
            WakeAction::DialBack { did } => WakeOutcome::DialedBack { did },
            WakeAction::Ring { did } => {
                let name = Some(self.inner.ctx.wake_label(&did)).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| NEUTRAL_RING_NAME.into());
                WakeOutcome::Ring { did, name }
            }
        })
    }

    /// The user answered a wake ring before the caller's call arrived: the call is accepted the
    /// moment it does (show "Connecting…"). `NotFound` once the ring ended, or once the call has
    /// already arrived (then use `answer` with its `call_id`).
    pub fn answer_wake_ring(&self, did: String) -> Result<(), Error> {
        self.inner.ctx.answer_wake_ring(&did).map_err(Error::from)
    }

    /// The user declined a wake ring: a call from that contact arriving within the window is
    /// declined without ringing. No `on_call_state` follows. `NotFound` as for `answer_wake_ring`.
    pub fn decline_wake_ring(&self, did: String) -> Result<(), Error> {
        self.inner.ctx.decline_wake_ring(&did).map_err(Error::from)
    }
}
