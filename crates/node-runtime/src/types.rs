//! Records of the node API that the runtime itself produces: call, contact and status values.
//! p2pcore declares them to UniFFI (`node/types.rs`).

use super::*;

#[derive(Debug, Clone)]
pub struct NodeStatus {
    pub started: bool,
    /// Connected to a home relay, i.e. reachable from anywhere.
    pub online: bool,
    pub relay: Option<String>,
    pub endpoint_id: String,
}
#[derive(Debug, Clone)]
pub struct ProfileInfo {
    pub did: String,
    pub name: String,
    pub device: String,
}
#[derive(Debug, Clone)]
pub struct Contact {
    pub did: String,
    pub name: String,
    pub device: String,
    pub added_at: u64,
    /// Local-only name we gave them; show it in preference to `name` (what they call
    /// themselves). Also what `CallInfo.peer_name` carries when set.
    pub alias: Option<String>,
    /// We confirmed their safety number in person; reset when a device of theirs appears that
    /// is not a rotated key of one we knew (a new install).
    pub verified: bool,
    /// Neither side can reach the other any more: our rotation note never reached them during
    /// its whole announce window (both moved while apart). The fix is a fresh ticket/QR swap;
    /// any session with them clears it.
    pub lost_touch: bool,
}
/// What a contact card found in some text says, before anyone is dialled: for "Add <name>?"
/// prompts when a card is pasted or sits on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Availability {
    pub available: bool,
    /// Unix seconds at which it turns back to available by itself; `None` = until switched on.
    pub until: Option<u64>,
}
#[derive(Debug, Clone)]
pub struct CallInfo {
    pub call_id: String,
    pub peer_did: String,
    pub peer_name: String,
    pub incoming: bool,
    /// Placed as a video call (the caller pressed Video and calls with its camera on). The callee
    /// learns it from `CallHello.camera`; either camera can still go on in any call.
    pub video: bool,
    /// The account of this device the call is for, when it is not the open one (a call that
    /// rings a background account, `docs/multi-online.md`). `None`: the open account.
    pub account: Option<AccountRef>,
}
/// One account of this device, as a background event names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRef {
    pub did: String,
    pub name: String,
}
#[derive(Debug, Clone, PartialEq)]
pub enum CallState {
    Dialing,
    Ringing,
    Active,
    Ended { reason: String },
}
#[derive(Debug, Clone)]
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
    /// Still dialling, and the relay took a wake for the callee's phone: "Waking their phone…".
    pub waking: bool,
}
impl Me {
    pub fn load(profile: Profile, epoch: u64) -> Result<Self, Error> {
        let id = identity::recover(&profile.mnemonic).map_err(|_| Error::BadPhrase)?;
        let device = cryptography::signature::public_key(&profile.device_secret);
        let attestation = proto::attest(&id, device, now());
        Ok(Self { epoch, profile, id, device, attestation })
    }

    pub fn info(&self) -> ProfileInfo {
        ProfileInfo {
            did: self.id.did().to_string(),
            name: self.profile.name.clone(),
            device: PublicKey::from_bytes(&self.device).map(|k| k.to_string()).unwrap_or_default(),
        }
    }
}
