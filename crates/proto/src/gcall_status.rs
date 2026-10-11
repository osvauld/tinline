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

use super::gcall::{GCALL_VERSION, HostRef, MAX_PARTICIPANTS};
use super::*;

/// Same bytes as the domain in `gcall.rs` (private there); the kind tag below separates them.
const GCALL_DOMAIN: &[u8] = b"tinline/gcall/v1\0";
/// §2.2: a status lives at most this long; a longer `ttl_ms` is refused.
pub const STATUS_TTL_MS: u64 = 60_000;
/// §2.2: re-send period while eligible.
pub const STATUS_REFRESH_MS: u64 = 30_000;
/// A status issued up to this far in the receiver's future still counts (device clocks
/// differ); it still expires at `issued_ms + ttl_ms`.
pub const MAX_CLOCK_SKEW_MS: u64 = 5_000;
/// Largest signed status payload, in bytes before base64. A real one is about 400 B.
pub const MAX_STATUS: usize = 1024;
const MAX_TEXT: usize = 128;

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
        self.issued_ms.saturating_add(self.ttl_ms.min(STATUS_TTL_MS))
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

fn text_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_TEXT
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
    debug_assert_eq!(id.did(), status.host_ref.holder_did, "a status is signed by its host's holder");
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
mod tests {
    use super::super::gcall;
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn ident(seed: u8) -> Identity {
        let m = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
        identity::recover(&m.to_string()).unwrap()
    }

    fn gid() -> String {
        "f0".repeat(32)
    }

    fn status(id: &Identity) -> CallHostStatus {
        CallHostStatus {
            v: GCALL_VERSION,
            gid: gid(),
            host_ref: HostRef { holder_did: id.did().into(), device_slot: u64::MAX, permit_digest: "d".repeat(64) },
            class: DeviceClass::Phone,
            power: PowerState::Battery(100),
            network: NetworkKind::Metered,
            thermal: ThermalLevel::Critical,
            busy: true,
            load: 100,
            max_participants: MAX_PARTICIPANTS as u8,
            issued_ms: NOW,
            ttl_ms: STATUS_TTL_MS,
        }
    }

    #[test]
    fn gcall_status_round_trips_and_stays_small() {
        let h = ident(1);
        let s = status(&h);
        let blob = sign_status(&h, s.clone());
        assert_eq!(open_status_for(&blob, &gid(), NOW).unwrap(), s);
        // The largest legal status (64-char gid and digest, extreme values) is well inside the bound.
        let json = serde_json::to_vec(&StatusClaim::HostStatus(s)).unwrap();
        assert!(json.len() < MAX_STATUS / 2, "{} B", json.len());
        for p in [PowerState::Mains, PowerState::Charging(0), PowerState::Battery(42)] {
            let s = CallHostStatus { power: p, ..status(&h) };
            assert_eq!(open_status(&sign_status(&h, s.clone())).unwrap(), s);
        }
    }

    #[test]
    fn gcall_status_forged_tampered_and_cross_kind_are_refused() {
        let h = ident(1);
        let m = ident(2);
        // Signed by someone other than the claimed holder.
        let forged = sign_blob(&m, GCALL_DOMAIN, &StatusClaim::HostStatus(status(&h)));
        assert_eq!(open_status(&forged), Err(Error::BadSignature));
        // Tampered after signing (a lie about busy).
        let good = sign_status(&h, status(&h));
        let mut s = status(&h);
        s.busy = false;
        let payload = enc(serde_json::to_vec(&StatusClaim::HostStatus(s)).unwrap());
        let tampered = SignedBlob { payload, signature: good.signature.clone() };
        assert_eq!(open_status(&tampered), Err(Error::BadSignature));
        // Another domain.
        let other = sign_blob(&h, b"osvauld/p2p/grant/v1\0", &StatusClaim::HostStatus(status(&h)));
        assert_eq!(open_status(&other), Err(Error::BadSignature));
        // Same domain, other kinds: a Leave is not a status, a status is not a Leave or Start.
        let cid = gcall::new_call_id();
        let leave = gcall::sign_leave(&h, &gid(), &cid, 1, NOW);
        assert_eq!(open_status(&leave), Err(Error::Decode));
        assert!(gcall::open_leave(&good).is_err());
        assert!(gcall::open_start(&good).is_err());
    }

    #[test]
    fn gcall_status_for_another_group_or_expired_is_refused() {
        let h = ident(1);
        let blob = sign_status(&h, status(&h));
        assert_eq!(open_status_for(&blob, "other-group", NOW), Err(Error::UnexpectedMessage));
        // Alive for exactly ttl_ms.
        assert!(open_status_for(&blob, &gid(), NOW + STATUS_TTL_MS - 1).is_ok());
        assert_eq!(open_status_for(&blob, &gid(), NOW + STATUS_TTL_MS), Err(Error::Expired));
        // A short TTL ends earlier.
        let short = sign_status(&h, CallHostStatus { ttl_ms: 1_000, ..status(&h) });
        assert_eq!(open_status_for(&short, &gid(), NOW + 1_000), Err(Error::Expired));
        // Future-dated within the skew allowance counts; beyond it does not.
        assert!(open_status_for(&blob, &gid(), NOW - MAX_CLOCK_SKEW_MS).is_ok());
        assert_eq!(open_status_for(&blob, &gid(), NOW - MAX_CLOCK_SKEW_MS - 1), Err(Error::Expired));
    }

    #[test]
    fn gcall_status_oversize_and_bad_shape_are_refused() {
        let h = ident(1);
        // Oversize: refused before decoding or verifying.
        let big = CallHostStatus { gid: "g".repeat(2 * MAX_STATUS), ..status(&h) };
        assert_eq!(open_status(&sign_status(&h, big)), Err(Error::FrameTooLarge));
        let junk = SignedBlob { payload: "A".repeat(MAX_STATUS * 2), signature: String::new() };
        assert_eq!(open_status(&junk), Err(Error::FrameTooLarge));
        // Shape bounds.
        let bad: Vec<CallHostStatus> = vec![
            CallHostStatus { gid: "g".repeat(MAX_TEXT + 1), ..status(&h) },
            CallHostStatus { gid: String::new(), ..status(&h) },
            CallHostStatus { power: PowerState::Battery(101), ..status(&h) },
            CallHostStatus { power: PowerState::Charging(101), ..status(&h) },
            CallHostStatus { load: 101, ..status(&h) },
            CallHostStatus { max_participants: 1, ..status(&h) },
            CallHostStatus { max_participants: MAX_PARTICIPANTS as u8 + 1, ..status(&h) },
            CallHostStatus { ttl_ms: 0, ..status(&h) },
            CallHostStatus { ttl_ms: STATUS_TTL_MS + 1, ..status(&h) },
        ];
        for s in bad {
            assert_eq!(open_status(&sign_status(&h, s.clone())), Err(Error::Decode), "{s:?}");
        }
        let mut s = status(&h);
        s.host_ref.permit_digest = String::new();
        assert_eq!(open_status(&sign_status(&h, s)), Err(Error::Decode));
        let s = CallHostStatus { v: GCALL_VERSION + 1, ..status(&h) };
        assert_eq!(open_status(&sign_status(&h, s)), Err(Error::UnknownVersion));
    }
}
