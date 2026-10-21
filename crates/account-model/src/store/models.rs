//! The persisted models: profile, contacts, `State` and the call log record.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use zeroize::ZeroizeOnDrop;


/// Current on-disk profile (`"version": 2`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileV2 {
    pub version: u32,
    pub name: String,
    pub did: String,
    pub device_public: [u8; 32],
    pub vault: crate::vault::Vault,
    /// This install's device ID (hex of 32 random bytes; U16's "device value", P9b): new when
    /// the account is created, linked or restored over this directory, kept when only the device
    /// key changes (endpoint rotation). Never derived from any key. The chat writer ID is
    /// `writer_id(DID, device ID)`. Absent in profiles from before P9b until the chat store
    /// first opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// A name edited while locked must be applied as a local profile edit on next unlock.
    #[serde(default)]
    pub profile_name_pending: bool,
    /// A newly linked/restored install must learn the account profile, not publish defaults
    /// that could overwrite the existing account's name or availability.
    #[serde(default)]
    pub profile_inherited: bool,
}

/// A fresh device ID (`ProfileV2::device_id`).
pub fn new_device_id() -> String {
    hex::encode(crdt::new_device_value())
}

impl ProfileV2 {
    /// The device ID as bytes, if the profile has a well-formed one.
    pub fn device_id_bytes(&self) -> Option<[u8; 32]> {
        self.device_id.as_deref().and_then(|h| hex::decode(h).ok()).and_then(|b| b.try_into().ok())
    }
}

/// The account's `account.json`.
#[derive(Clone)]
pub enum Disk {
    V2(ProfileV2),
}

/// What an unlocked identity is loaded from (the vault's secrets plus the name).
#[derive(Clone, Serialize, Deserialize, ZeroizeOnDrop)]
pub struct Profile {
    pub mnemonic: String,
    pub name: String,
    /// The install's iroh secret key; random per install, attested by the DID.
    pub device_secret: [u8; 32],
}

/// One callable device of a contact, with the relay it was last seen on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactDevice {
    pub device: [u8; 32],
    #[serde(default)]
    pub relay: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "RawContact")]
pub struct StoredContact {
    pub did: String,
    pub name: String,
    /// Calls dial all of them. Without a `device_list`: newest first (at most
    /// `proto::MAX_LIST_DEVICES`); with one: the list's order.
    pub devices: Vec<ContactDevice>,
    /// The newest signed device list we hold for them; once present it, not `note_device`,
    /// decides which devices we dial.
    pub device_list: Option<proto::SignedDeviceList>,
    /// Their message token for us (U13): opens chat with them. Replaced by a renewal that
    /// outlives it.
    pub grant_from_them: proto::SignedGrant,
    /// Their call token for us, if they let us call (`None`: we can only message them).
    /// Replaced by a renewal that outlives it; dropped when they answer "calls not allowed".
    pub call_grant_from_them: Option<proto::SignedGrant>,
    /// Whether WE let them call us (we issued them a call token and have not taken it back).
    /// Ticket contacts: true; group chat-request contacts: false (U13). Synced to own devices.
    pub calls_allowed: bool,
    pub added_at: u64,
    /// Local-only name we gave them; `name` stays what they call themselves.
    pub alias: Option<String>,
    /// We compared safety numbers with them in person. Reset when their device changes.
    pub verified: bool,
}

impl StoredContact {
    pub fn has_device(&self, device: &[u8; 32]) -> bool {
        self.devices.iter().any(|d| &d.device == device)
    }

    pub fn device_keys(&self) -> Vec<[u8; 32]> {
        self.devices.iter().map(|d| d.device).collect()
    }

    pub fn relay_for(&self, device: &[u8; 32]) -> Option<&str> {
        self.devices.iter().find(|d| &d.device == device).and_then(|d| d.relay.as_deref())
    }
}

/// A device as older versions stored it (a bare key) or as now.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawDevice {
    Old([u8; 32]),
    New(ContactDevice),
}

/// Reads both shapes: before multi-device a contact had `devices: [key]` and one contact-wide
/// `relay`, which becomes the hint of the first (newest) device.
#[derive(Deserialize)]
struct RawContact {
    did: String,
    name: String,
    devices: Vec<RawDevice>,
    #[serde(default)]
    relay: Option<String>,
    #[serde(default)]
    device_list: Option<proto::SignedDeviceList>,
    grant_from_them: proto::SignedGrant,
    #[serde(default)]
    call_grant_from_them: Option<proto::SignedGrant>,
    #[serde(default = "yes")]
    calls_allowed: bool,
    added_at: u64,
    #[serde(default)]
    alias: Option<String>,
    #[serde(default)]
    verified: bool,
}

fn yes() -> bool {
    true
}

impl From<RawContact> for StoredContact {
    fn from(r: RawContact) -> Self {
        let mut legacy_relay = r.relay;
        let devices = r
            .devices
            .into_iter()
            .map(|d| match d {
                RawDevice::Old(device) => ContactDevice { device, relay: legacy_relay.take() },
                RawDevice::New(d) => d,
            })
            .collect();
        StoredContact {
            did: r.did,
            name: r.name,
            devices,
            device_list: r.device_list,
            call_grant_from_them: r.call_grant_from_them,
            grant_from_them: r.grant_from_them,
            calls_allowed: r.calls_allowed,
            added_at: r.added_at,
            alias: r.alias,
            verified: r.verified,
        }
    }
}

/// One install of this account, as our own registry knows it. Sealed with the rest of the state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnDevice {
    pub device: [u8; 32],
    pub attestation: proto::SignedAttestation,
    pub label: String,
    #[serde(default)]
    pub removed: bool,
    #[serde(default)]
    pub last_seen: u64,
    /// The relay it was last known on (our own entry is refreshed from the endpoint).
    #[serde(default)]
    pub relay: Option<String>,
    /// The install's recent key changes ending at `device` (oldest first, at most
    /// `node::KEPT_LINKS`), carried in our device list so contacts keep `verified`.
    #[serde(default)]
    pub links: Vec<proto::Succession>,
    /// This key was rotated away (private endpoint rotation): a tombstone like `removed`, but
    /// the install lives on under a newer key, so it is not shown as unlinked.
    #[serde(default)]
    pub rotated: bool,
}

/// "Not now": calls are turned away (and not shown) until `until`, or until switched back on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AvailabilityState {
    pub unavailable: bool,
    pub until: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub contacts: Vec<StoredContact>,
    /// Per holder DID: tokens we issued at or before these times (unix ms) are refused
    /// (P6, U13). A block raises both, "disallow calls" the call one. Only ever raised; synced
    /// to own devices (max wins). Unblocking does not lower it: the contact needs fresh tokens.
    pub cut_offs: HashMap<String, CutOffs>,
    /// Tokens this device still has to deliver, per DID: capability names ("message", "call").
    /// Local (only the device that queued them delivers); fresh tokens are minted on delivery.
    pub token_outbox: HashMap<String, BTreeSet<String>>,
    /// DIDs we block (U19: the contact is kept). Everything from them is refused with the notice
    /// in `sent_notices`. Always the DIDs whose `sent_notices` entry is a block.
    pub blocked: HashSet<String>,
    /// The latest block / unblock notice we signed for each DID (`proto::sign_block_notice`);
    /// synced to own devices, which hand out the same notice instead of signing their own.
    pub sent_notices: HashMap<String, proto::SignedBlockNotice>,
    /// Notices this device still has to deliver (local: only the device that signed one sends it).
    pub notice_outbox: HashSet<String>,
    /// The latest notice each DID sent us, verified on intake; a block there is "<name> blocked
    /// you". Kept after an unblock so an older notice replayed later is refused.
    pub notices: HashMap<String, HeldNotice>,
    /// Redeemed tickets, a record only: `proto::redeemed_key(joiner, nonce)`. Whether a ticket
    /// still adds someone is `store::Tickets`.
    pub redeemed: HashSet<String>,
    /// The ticket we hand out (reusable until it expires or is reset), so the QR stays stable
    /// across launches.
    pub ticket: Option<String>,
    pub availability: AvailabilityState,
    /// Our own installs (task 34c: seeded with this device; linking fills it later).
    pub registry: Vec<OwnDevice>,
    /// The latest `DeviceList` we signed from the registry; sent in every call hello and accept,
    /// and pushed to contacts when its device set changes (U8, `devlist.rs`).
    pub own_list: Option<proto::SignedDeviceList>,
    /// Contacts this device still has to send its current `own_list` to (local: the device that
    /// saw the change pushes; the list is read fresh on each attempt).
    pub list_outbox: HashSet<String>,
    /// Contacts owed our current profile version, delivered on existing chat control sessions.
    pub profile_outbox: HashSet<String>,
    /// The device keys (text form, sorted) of the last list queued for every contact, so a change
    /// is pushed once even if a call re-signed the list first.
    pub pushed_list_keys: Vec<String>,
    /// One-time contact tickets we handed out by accepting a group chat request (U3), by
    /// invite nonce: only that DID may redeem one, and it gets message-only tokens (P6). Local:
    /// the ticket names this device's private endpoint.
    pub req_tickets: HashMap<String, ReqTicket>,
    /// Reusable contact codes (2026-10-13): invite nonce -> `exp` of every ticket of ours that
    /// still adds contacts. The one in `ticket` plus older ones replaced by a rename, a relay
    /// change or the last day of life, which stay good until they expire. `reset_ticket` and a
    /// block clear it. Local: a ticket names this device.
    pub live_tickets: BTreeMap<String, u64>,
    /// Contact requests made with our code, held for the user's Accept, by joiner DID (one per
    /// DID; a later hello replaces the held one). Local and PROVISIONAL: the hello is addressed
    /// to this device, so only it can answer. Kept until the code they used expires.
    pub contact_requests: BTreeMap<String, HeldRequest>,
    /// Requests we sent with someone's code that they hold for their user, by owner DID, until
    /// their dialled-back `ContactWelcome` arrives. Local.
    pub sent_requests: BTreeMap<String, SentRequest>,
    /// Welcomes for accepted requests still to be delivered, by joiner DID (retried like
    /// `notice_outbox`). Local: the device that accepted dials.
    pub welcome_outbox: BTreeMap<String, WelcomeOut>,
    /// This install's private endpoint schedule (`node::rotation`). Local.
    pub private_ep: PrivateEp,
    /// The previous private endpoint key, answering for `PRIVATE_OVERLAP_MS` after a rotation
    /// so contacts that still dial it learn the new one. Local; sealed like the rest.
    pub overlap: Option<OverlapKey>,
    /// Contacts we lost touch with: our rotation note never reached any of their devices during
    /// the whole announce window (`node::rotation`). Cleared by any authenticated session with
    /// them or a fresh ticket swap. Local.
    pub lost_touch: BTreeSet<String>,
}

/// See `State::private_ep`. `made == 0`: not scheduled yet (set on the first start).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivateEp {
    /// When the current key became current (unix ms, local clock).
    pub made: u64,
    /// The random extra delay drawn for the current key, ms.
    pub jitter: u64,
    /// Until when contacts still owed our rotation note are tried (unix ms; 0 = no window).
    pub announce_until: u64,
    /// A rotation whose new key may or may not have been promoted when we stopped: resolved on
    /// the next unlock by comparing with the key the vault holds.
    pub pending: Option<PendingRotation>,
}

/// See `PrivateEp::pending`. Public data only: the new secret lives in the vault once promoted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRotation {
    pub old: [u8; 32],
    pub new: [u8; 32],
    /// `old` naming `new`, signed by `old`.
    pub link: proto::Succession,
    /// When it was staged (unix ms).
    pub at: u64,
}

/// See `State::overlap`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlapKey {
    pub secret: [u8; 32],
    /// Closes at (unix ms).
    pub until: u64,
}

impl std::fmt::Debug for OverlapKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverlapKey").field("until", &self.until).finish_non_exhaustive()
    }
}

/// See `State::contact_requests`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldRequest {
    /// The joiner's `ContactHello`, checked when it arrived and checked again on Accept.
    pub hello: proto::Msg,
    /// The joiner's device (the connection's, equal to the attested one).
    pub device: [u8; 32],
    /// The joiner's relay hint from the hello, sanitised.
    pub relay: Option<String>,
    pub name: String,
    /// Our ticket's nonce they used, and its `exp` (unix seconds): the request lasts as long.
    pub nonce: String,
    pub exp: u64,
    /// When it first arrived (unix ms).
    pub at: u64,
    /// Our device key the hello was addressed to (its ticket names it); the Welcome is made
    /// and sent under that key. `None`: the current one.
    #[serde(default)]
    pub on: Option<[u8; 32]>,
}

/// See `State::sent_requests`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentRequest {
    pub pending: proto::PendingContact,
    /// Their ticket's relay (sanitised), stored with the contact once they accept.
    pub relay: Option<String>,
    /// Whether our hello gave them a call token (`policy_for`), so we let them call.
    pub calls_allowed: bool,
    /// Their ticket's `exp` (unix seconds).
    pub exp: u64,
    /// When we sent it (unix ms).
    pub at: u64,
    /// They answered `ContactPending`. False while dialling, or when the dial failed after the
    /// hello may have reached them: kept (not listed) so a late Accept still lands.
    pub delivered: bool,
}

/// See `State::welcome_outbox`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WelcomeOut {
    pub welcome: proto::Msg,
    pub device: [u8; 32],
    pub relay: Option<String>,
    /// Our key the Welcome is made under (`HeldRequest::on`): it is dialled from the endpoint
    /// of that key, so after a rotation only while the old one still overlaps.
    #[serde(default)]
    pub from: Option<[u8; 32]>,
    /// Give up after this (unix seconds).
    pub until: u64,
}

/// See `State::req_tickets`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReqTicket {
    pub did: String,
    /// Unix seconds.
    pub exp: u64,
}

/// See `State::cut_offs`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CutOffs {
    pub message: u64,
    pub call: u64,
}

impl CutOffs {
    /// Raises each field to the other's (merge between own devices).
    pub fn merge(&mut self, other: &CutOffs) {
        self.message = self.message.max(other.message);
        self.call = self.call.max(other.call);
    }
}

/// A notice someone sent us, with the name we knew them by (their contact entry is deleted on a
/// block, U20, but the kept conversation still needs a name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldNotice {
    pub notice: proto::SignedBlockNotice,
    pub name: String,
    /// From the verified notice: a block (true) or an unblock, and its `at` (unix ms).
    pub blocked: bool,
    pub at: u64,
}

/// Most calls kept in the call log; the oldest fall off.
pub const MAX_HISTORY: usize = 500;

/// One finished call, as the UI lists it. `reason` is one of the stable end reasons in
/// `docs/protocol.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallRecord {
    pub call_id: String,
    pub peer_did: String,
    /// Their name when the call happened (our alias for them if we had one).
    pub peer_name: String,
    pub incoming: bool,
    /// Unix seconds when the call started (dialing, or the first ring).
    pub started_at: u64,
    /// Seconds from answer to end; 0 if it never became active.
    pub duration_secs: u32,
    pub reason: String,
    /// Media went over a direct path rather than a relay (as last seen).
    pub direct: bool,
    /// Incoming and we never answered it (not one we declined, nor one turned away while
    /// unavailable).
    pub missed: bool,
    /// A video call: placed as one (`CallInfo.video`), or a camera was on while it was active.
    #[serde(default)]
    pub video: bool,
}
