//! Public records and the `NodeEvents` callback of the `Node` API.

use super::*;

/// `on_call_state` with `Ended` is also the moment the call history has changed: the UI should
/// re-read `recent_calls` / `calls_with` then (there is no separate history callback).
#[uniffi::export(with_foreign)]
pub trait NodeEvents: Send + Sync {
    fn on_status(&self, status: NodeStatus);
    fn on_contacts_changed(&self);
    fn on_incoming_call(&self, call: CallInfo);
    fn on_call_state(&self, call_id: String, state: CallState);
    fn on_log(&self, line: String);
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct NodeStatus {
    pub started: bool,
    /// Connected to a home relay, i.e. reachable from anywhere.
    pub online: bool,
    pub relay: Option<String>,
    pub endpoint_id: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProfileInfo {
    pub did: String,
    pub name: String,
    pub device: String,
}

/// One account on this device, as the lock screen and the switcher list it. Read from the clear
/// `account.json`, so it is available while everything is locked.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AccountSummary {
    pub did: String,
    pub name: String,
    /// The account `has_identity`, `unlock` and the rest act on.
    pub current: bool,
    pub has_passphrase: bool,
}

/// Whether the identity's secrets are available. `Locked` needs `unlock` (or
/// `unlock_with_key`); `NeedsPassphrase` is a pre-vault install that works but must be
/// converted with `set_passphrase(None, ..)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LockState {
    NoIdentity,
    Locked,
    Unlocked,
    NeedsPassphrase,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Contact {
    pub did: String,
    pub name: String,
    pub device: String,
    pub added_at: u64,
    /// Local-only name we gave them; show it in preference to `name` (what they call
    /// themselves). Also what `CallInfo.peer_name` carries when set.
    pub alias: Option<String>,
    /// We confirmed their safety number in person; reset if their device changes.
    pub verified: bool,
}

/// What a contact card found in some text says, before anyone is dialled: for "Add <name>?"
/// prompts when a card is pasted or sits on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CardPeek {
    /// The card itself, cut out of the surrounding text; hand this to `add_contact`.
    pub ticket: String,
    /// The name they gave themselves. Unverified until they are added.
    pub name: String,
    pub did: String,
    /// Already in our contacts.
    pub known: bool,
}

/// Whether calls ring. While unavailable, callers are turned away without being told why.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Availability {
    pub available: bool,
    /// Unix seconds at which it turns back to available by itself; `None` = until switched on.
    pub until: Option<u64>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CallInfo {
    pub call_id: String,
    pub peer_did: String,
    pub peer_name: String,
    pub incoming: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum CallState {
    Dialing,
    Ringing,
    Active,
    Ended { reason: String },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CallStats {
    pub call_id: String,
    pub state: CallState,
    pub secs: u32,
    pub direct: bool,
    pub rtt_ms: u32,
    pub sent: u64,
    pub received: u64,
    pub lost: u64,
    pub recovered: u64,
    pub concealed: u64,
    pub buffered_ms: u32,
    /// Dominant frequency of the last second of received audio; for the tone self-test.
    pub rx_freq_hz: f32,
    pub rx_rms: f32,
    /// Active, but no media for over 1.5 s and the call has not ended: the path is changing.
    pub reconnecting: bool,
}

impl Me {
    pub(crate) fn load(profile: Profile, epoch: u64) -> Result<Self, Error> {
        let id = identity::recover(&profile.mnemonic).map_err(|_| Error::BadPhrase)?;
        let device = proto::device_public(&profile.device_secret);
        let attestation = proto::attest(&id, device, now());
        Ok(Self { epoch, profile, id, device, attestation })
    }

    pub(crate) fn info(&self) -> ProfileInfo {
        ProfileInfo {
            did: self.id.did().to_string(),
            name: self.profile.name.clone(),
            device: PublicKey::from_bytes(&self.device).map(|k| k.to_string()).unwrap_or_default(),
        }
    }
}
