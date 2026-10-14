//! `CallHostStatus` (docs/design/group-call-spec.md §2.2): a call-host candidate's signed,
//! self-reported conditions. It travels as a group `Ephemeral` through the chat hosts and
//! lives at most [`STATUS_TTL_MS`]; the candidate re-sends it every [`STATUS_REFRESH_MS`] while
//! eligible and at once when its conditions change.
//!
//! Signed by `host_ref.holder_did` in the shared group-call domain `tinline/gcall/v1\0`, with
//! its own kind tag `"t": "host_status"` inside the signed payload, so a status can never be
//! opened as a call-control message (`Join`, `Leave`, ...) or the reverse.
//!
//! Pure: this module checks the signature, version, shape, size, group and freshness. The
//! `host_call` permit behind `host_ref` and the ranking are `p2pcore::gcall::rank`'s job. All
//! fields are advisory (§2.2 [A]): a lying host only gets itself worse quality.

use super::gcall::{GCALL_DOMAIN, GCALL_VERSION, HostRef, MAX_PARTICIPANTS, text_ok};
use super::*;

/// §2.2: a status lives at most this long; a longer `ttl_ms` is refused.
pub const STATUS_TTL_MS: u64 = 60_000;
/// §2.2: re-send period while eligible.
pub const STATUS_REFRESH_MS: u64 = 30_000;
/// A status issued up to this far in the receiver's future still counts (device clocks
/// differ); it still expires at `issued_ms + ttl_ms`.
pub const MAX_CLOCK_SKEW_MS: u64 = 5_000;
/// Largest signed status payload, in bytes before base64. A real one is about 400 B.
pub const MAX_STATUS: usize = 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClass {
    Desktop,
    Headless,
    Phone,
}

/// Battery values are percent, 0..=100.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PowerState {
    /// No battery (desktop, headless box) or a full battery on external power.
    Mains,
    /// On external power with a battery.
    Charging(u8),
    /// Unplugged.
    Battery(u8),
}

impl PowerState {
    /// Mains or charging.
    pub fn external(self) -> bool {
        !matches!(self, PowerState::Battery(_))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NetworkKind {
    Unmetered,
    /// Mobile data, or a network the OS reports as metered.
    Metered,
}

/// Android `PowerManager.THERMAL_STATUS_*` order (desktops report `None` unless they know).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ThermalLevel {
    None,
    Light,
    Moderate,
    Severe,
    Critical,
}

/// Signed by `host_ref.holder_did`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallHostStatus {
    pub v: u8,
    pub gid: String,
    /// The advertising device (U4 revised: no transport key).
    pub host_ref: HostRef,
    pub class: DeviceClass,
    pub power: PowerState,
    pub network: NetworkKind,
    pub thermal: ThermalLevel,
    /// Not taking a new call: already hosting one (one session per node), hosting switched
    /// off, or the §2.4 policy says stop advertising.
    pub busy: bool,
    /// Percent (0..=100) of the device's capacity already used by other work: CPU, a 1:1 call
    /// of its user, chat-host relaying. 0 = idle. Ranking tie-break 2 (G6).
    pub load: u8,
    /// Participant cap under the current conditions (§2.4, Q1), 2..=12.
    pub max_participants: u8,
    /// Issuer's wall clock, ms since the Unix epoch.
    pub issued_ms: u64,
    /// 1..=`STATUS_TTL_MS`.
    pub ttl_ms: u64,
}

impl CallHostStatus {
    /// End of life (exclusive), on the issuer's clock.
    pub fn expires_ms(&self) -> u64 {
        self.issued_ms
            .saturating_add(self.ttl_ms.min(STATUS_TTL_MS))
    }

    /// Alive at `now_ms` (receiver's clock), allowing `MAX_CLOCK_SKEW_MS` of future dating.
    pub fn is_fresh(&self, now_ms: u64) -> bool {
        self.issued_ms <= now_ms.saturating_add(MAX_CLOCK_SKEW_MS) && now_ms < self.expires_ms()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum StatusClaim {
    HostStatus(CallHostStatus),
}

fn shape_ok(s: &CallHostStatus) -> bool {
    let pct_ok = match s.power {
        PowerState::Mains => true,
        PowerState::Charging(p) | PowerState::Battery(p) => p <= 100,
    };
    text_ok(&s.gid)
        && text_ok(&s.host_ref.holder_did)
        && text_ok(&s.host_ref.permit_digest)
        && pct_ok
        && s.load <= 100
        && s.max_participants >= 2
        && s.max_participants as usize <= MAX_PARTICIPANTS
        && (1..=STATUS_TTL_MS).contains(&s.ttl_ms)
}

/// `status.host_ref.holder_did` must be `id`'s DID.
pub fn sign_status(id: &Identity, status: CallHostStatus) -> SignedBlob {
    debug_assert_eq!(
        id.did(),
        status.host_ref.holder_did,
        "a status is signed by its host's holder"
    );
    sign_blob(id, GCALL_DOMAIN, &StatusClaim::HostStatus(status))
}

/// Size, signature (by `host_ref.holder_did`), kind, version and shape. Not the group, the
/// clock or the permit; see [`open_status_for`].
pub fn open_status(blob: &SignedBlob) -> Result<CallHostStatus> {
    if blob.payload.len() > MAX_STATUS.div_ceil(3) * 4 {
        return Err(Error::FrameTooLarge);
    }
    let StatusClaim::HostStatus(s) = open_blob(blob, GCALL_DOMAIN, |c: &StatusClaim| match c {
        StatusClaim::HostStatus(s) => s.host_ref.holder_did.as_str(),
    })?;
    if s.v != GCALL_VERSION {
        return Err(Error::UnknownVersion);
    }
    if !shape_ok(&s) {
        return Err(Error::Decode);
    }
    Ok(s)
}

/// [`open_status`], plus: for group `gid` (else `UnexpectedMessage`) and alive at `now_ms`
/// (else `Expired`).
pub fn open_status_for(blob: &SignedBlob, gid: &str, now_ms: u64) -> Result<CallHostStatus> {
    let s = open_status(blob)?;
    if s.gid != gid {
        return Err(Error::UnexpectedMessage);
    }
    if !s.is_fresh(now_ms) {
        return Err(Error::Expired);
    }
    Ok(s)
}

#[cfg(test)]
mod tests;
