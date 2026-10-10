//! Group calls through one call host (docs/design/group-call-spec.md, G6). Sans-IO.
//!
//! - [`HostEngine`]: the call host's side (Join/Leave/Roster/heartbeat, rate limits, speaker
//!   selection, `Active`, phone degrade policy, End).
//! - [`ClientEngine`]: one participant's side (verifies control, tracks Roster and `Active`,
//!   per-slot jitter buffers and the local mix, silence-gated sender, failover to backups).
//! - [`CallBoard`]: a member's view of `CallStart` announcements (generations, glare, split
//!   after redial, which call is shown, whether this device may take over).
//!
//! Inputs are events with a millisecond timestamp (any monotonic clock); outputs are actions
//! for the IO layer (send a control frame, send a datagram, close, dial). Membership and
//! permits are asked through [`CallAuthority`], because real group membership is not in
//! production yet. Not wired into `node.rs` yet.
//!
//! Spec gaps G1–G10 (§9) are settled as modelled in `crates/group-sim/src/group_call.rs`:
//! G1 the CallStart travels with a call-host record (the wiring's job; here `host_permit_ok`);
//! G2 Join carries the joiner's group note (`GroupCallMsg::Join.note`, checked by
//! `endpoint_ok`); G3 the moving average uses datagrams inside the window only; G4 the
//! dominance cap counts frames with >= K other eligible senders; G5 "newer Join" = larger
//! signed `issued_ms`; G9 a takeover CallStart lists the previous backups minus the new host;
//! G10 members refuse a Roster that lists a non-member. G6–G8 are ranking rules (not here).

mod board;
mod client;
mod host;
mod policy;
mod select;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

pub use board::CallBoard;
pub use client::{ClientAction, ClientEnd, ClientEngine, ClientState};
pub use host::{CloseReason, HostAction, HostConfig, HostEngine, HostNotice};
pub use policy::{Class, Cond, PhoneAction, Thermal, phone_policy};
pub use proto::gcall::{EndReason, HostRef};
pub use select::Forwarder;

/// Opus frame length.
pub const FRAME_MS: u64 = 20;
/// §5: host heartbeat period, and how many may be missed before the host counts as lost.
pub const HEARTBEAT_MS: u64 = 1_000;
pub const HEARTBEAT_MISSES: u64 = 3;
/// §5: a member with no control and no datagram for this long is dropped by the host.
pub const MEMBER_DROP_MS: u64 = 5_000;
/// §6 rate limits at the host.
pub const MAX_UP_PPS: u64 = 60;
pub const UP_BURST: u64 = 10;
pub const MAX_CONTROL_PER_S: usize = 10;
pub const JOIN_MIN_GAP_MS: u64 = 2_000;
/// §2.3 / §5: per-backup redial timeout.
pub const ANSWER_TIMEOUT_MS: u64 = 5_000;
/// Q3 (decided as recommended, G6-D3): group-call Opus bitrate.
pub const GROUP_BITRATE_BPS: i32 = 32_000;
/// G6-D1: a gated sender sends `[0x03][level]` this often.
pub const LEVEL_ONLY_MS: u64 = 500;

/// Transport connection handle chosen by the IO layer. [`ConnId::LOCAL`] is the host's own
/// user, whose frames never touch the network (§1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnId(pub u64);

impl ConnId {
    pub const LOCAL: ConnId = ConnId(0);
}

/// Group membership and permits, as the local device currently knows them (§3.2 items 2–4).
pub trait CallAuthority {
    /// `did` is a member of `gid` at the locally validated meta frontier and not removed.
    fn is_member(&self, gid: &str, did: &str) -> bool;
    /// `host` holds an unrevoked `host_call` permit bound to `host.device_slot`, and its
    /// holder is a member.
    fn host_permit_ok(&self, gid: &str, host: &HostRef) -> bool;
    /// The connecting endpoint is `did`'s current group endpoint (H's directory, or the
    /// DID-signed group `note` the Join carried, G2), and that device is not unlinked.
    fn endpoint_ok(&self, gid: &str, did: &str, note: Option<&proto::SignedBlob>, endpoint: &[u8; 32]) -> bool;
}

/// Why a control message or a Join was refused. A refusal never changes engine state, except
/// for per-connection control-rate accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Malformed,
    Oversize,
    BadSignature,
    WrongGroup,
    WrongCall,
    StaleGen,
    NotMember,
    PermitInvalid,
    NotHost,
    EndpointMismatch,
    Replay,
    RateLimited,
    Full,
    Limited,
    Stale,
    Unknown,
    NotParticipant,
    /// Already have this exact CallStart.
    Duplicate,
    /// Lost a glare or split tie-break to a call already known.
    Superseded,
    Refused,
}

impl From<proto::Error> for Refusal {
    fn from(e: proto::Error) -> Self {
        match e {
            proto::Error::BadSignature => Refusal::BadSignature,
            proto::Error::FrameTooLarge => Refusal::Oversize,
            _ => Refusal::Malformed,
        }
    }
}

/// A [`CallAuthority`] backed by plain sets, for tests and until group membership is wired.
#[derive(Debug, Clone, Default)]
pub struct StaticAuthority {
    pub members: BTreeSet<String>,
    /// Removed DIDs (U9/U12); refused even if still in `members`.
    pub removed: BTreeSet<String>,
    /// Valid `host_call` permit digests.
    pub permits: BTreeSet<String>,
    /// DID → current group endpoints (one per linked device).
    pub endpoints: BTreeMap<String, BTreeSet<[u8; 32]>>,
}

impl CallAuthority for StaticAuthority {
    fn is_member(&self, _gid: &str, did: &str) -> bool {
        self.members.contains(did) && !self.removed.contains(did)
    }
    fn host_permit_ok(&self, gid: &str, host: &HostRef) -> bool {
        self.permits.contains(&host.permit_digest) && self.is_member(gid, &host.holder_did)
    }
    fn endpoint_ok(&self, _gid: &str, did: &str, _note: Option<&proto::SignedBlob>, endpoint: &[u8; 32]) -> bool {
        self.endpoints.get(did).is_some_and(|e| e.contains(endpoint))
    }
}
