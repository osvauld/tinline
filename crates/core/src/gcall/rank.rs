//! Call-host ranking and the picker (docs/design/group-call-spec.md §2.3), sans-IO.
//!
//! Input: the `CallHostStatus` blobs the starter's device has collected (§2.2). Each is
//! admitted only if it opens (signature by the holder, kind, shape, size), is for this group,
//! is fresh at `now`, and its `host_ref` holds a valid `host_call` permit
//! ([`CallAuthority::host_permit_ok`]). The latest admitted status per device
//! (`holder_did`, `device_slot`) counts.
//!
//! Tiers (lower is better; `None` = not a candidate):
//!
//! | Tier | Candidate |
//! |---|---|
//! | 0 | desktop or headless, on external power (mains or charging), unmetered |
//! | 1 | phone, charging, unmetered |
//! | 2 | phone on battery >= 50 %, unmetered; desktop/headless on battery >= 30 %, unmetered (G7) |
//! | 3 | phone on battery 30–49 %, unmetered; desktop/headless on battery < 30 %, unmetered (G7) |
//! | 4 | metered, any class and power |
//! | – | busy, thermal >= Moderate, `load >= LOAD_FULL_PCT`, `max_participants < expected`, phone on battery < 30 % |
//!
//! G7: a laptop on battery forwards a few Opus streams for a negligible share of its battery
//! and has the desktop cap, so it ranks with a healthy phone (tier 2); a nearly flat laptop
//! still beats nothing, so it is not excluded but ranked with the weak phones (tier 3). A
//! phone at 30–49 % is still advertising (§2.4) but should be used only after every healthy
//! unmetered host (tier 3). Metered data costs the host's user N−1 streams up, so any metered
//! host is last, desktops included; it still takes the full 12 (Q1, 2026-10-14).
//!
//! Tie-breaks inside a tier, in order:
//! 1. The starter's own device, if it is in the best non-empty tier (G8: matched on the device,
//!    `holder_did` + `device_slot`, not on the DID, so a starter's phone does not inherit its
//!    desktop's preference).
//! 2. Lower load, compared in buckets of `LOAD_BUCKET_PCT` (G6), so small fluctuations between
//!    refreshes do not reorder candidates.
//! 3. Lowest `BLAKE3(call_id ‖ 0 ‖ holder_did ‖ 0 ‖ device_slot BE ‖ permit_digest)`.
//!
//! The result depends only on the status set, the context and `now`; input order does not
//! matter (GC-S04).

use std::collections::{BTreeMap, BTreeSet};

use proto::gcall::MAX_BACKUPS;
use proto::gcall_status::SignedStatus;
use proto::gcall_status::{CallHostStatus, DeviceClass, NetworkKind, PowerState, ThermalLevel};

use super::policy::BATTERY_ADVERTISE_PCT;
use super::{ANSWER_TIMEOUT_MS, CallAuthority, HostRef, Refusal};

/// §2.3: a phone on battery at or above this is tier 2.
pub const BATTERY_TIER2_PCT: u8 = 50;
/// G6: load tie-break granularity.
pub const LOAD_BUCKET_PCT: u8 = 25;
/// G6: at or above this load a candidate is treated as busy.
pub const LOAD_FULL_PCT: u8 = 90;
/// Worst tier (metered).
pub const TIER_METERED: u8 = 4;

/// What one ranking is for.
#[derive(Debug, Clone, Copy)]
pub struct RankCtx<'a> {
    pub gid: &'a str,
    /// The new call's id (`proto::gcall::new_call_id`); seeds the hash tie-break.
    pub call_id: &'a str,
    /// Participants expected (group members who may join), for `max_participants`.
    pub expected: usize,
    /// The starter's own device, if it advertises as a candidate (G8).
    pub starter: Option<&'a HostRef>,
    /// Local wall clock, ms since the Unix epoch (same clock as `issued_ms`).
    pub now_ms: u64,
}

/// One admission check, for the collector (cache only what passes) and for [`rank`].
pub fn admit(
    auth: &(impl CallAuthority + ?Sized),
    gid: &str,
    blob: &SignedStatus,
    now_ms: u64,
) -> Result<CallHostStatus, Refusal> {
    let s = blob.open()?;
    if s.gid != gid {
        return Err(Refusal::WrongGroup);
    }
    if !s.is_fresh(now_ms) {
        return Err(Refusal::Stale);
    }
    if !auth.host_permit_ok(gid, &s.host_ref) {
        return Err(Refusal::PermitInvalid);
    }
    Ok(s)
}

/// The §2.3 tier of an admitted status for a call of `expected` participants.
pub fn tier(s: &CallHostStatus, expected: usize) -> Option<u8> {
    if s.busy || s.thermal >= ThermalLevel::Moderate || s.load >= LOAD_FULL_PCT || (s.max_participants as usize) < expected {
        return None;
    }
    let phone = s.class == DeviceClass::Phone;
    if phone && matches!(s.power, PowerState::Battery(p) if p < BATTERY_ADVERTISE_PCT) {
        return None;
    }
    if s.network == NetworkKind::Metered {
        return Some(TIER_METERED);
    }
    Some(match (phone, s.power) {
        (false, p) if p.external() => 0,
        (true, p) if p.external() => 1,
        (true, PowerState::Battery(p)) if p >= BATTERY_TIER2_PCT => 2,
        (false, PowerState::Battery(p)) if p >= BATTERY_ADVERTISE_PCT => 2,
        _ => 3,
    })
}

/// Same device, whatever permit digest it currently advertises.
fn same_device(a: &HostRef, b: &HostRef) -> bool {
    a.holder_did == b.holder_did && a.device_slot == b.device_slot
}

/// Tie-break 3 (§2.3): `BLAKE3(call_id || 0 || holder_did || 0 || device_slot_be || permit_digest)`.
/// Also the §5 split-after-redial fallback in `board`.
pub fn tie_hash(call_id: &str, r: &HostRef) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(call_id.as_bytes());
    h.update(&[0]);
    h.update(r.holder_did.as_bytes());
    h.update(&[0]);
    h.update(&r.device_slot.to_be_bytes());
    h.update(r.permit_digest.as_bytes());
    *h.finalize().as_bytes()
}

/// Rank statuses that already passed [`admit`] (rechecks group and freshness only).
pub fn rank_admitted(statuses: &[CallHostStatus], ctx: &RankCtx) -> Vec<HostRef> {
    // Latest per device; equal issued_ms broken by the serialized status, so input order
    // never matters.
    let mut latest: BTreeMap<(&str, u64), (&CallHostStatus, Vec<u8>)> = BTreeMap::new();
    for s in statuses.iter().filter(|s| s.gid == ctx.gid && s.is_fresh(ctx.now_ms)) {
        let key = (s.host_ref.holder_did.as_str(), s.host_ref.device_slot);
        let bytes = serde_json::to_vec(s).expect("statuses serialize");
        let newer = latest.get(&key).is_none_or(|(c, cb)| (s.issued_ms, &bytes) > (c.issued_ms, cb));
        if newer {
            latest.insert(key, (s, bytes));
        }
    }
    let tiered: Vec<(u8, &CallHostStatus)> =
        latest.values().filter_map(|(s, _)| tier(s, ctx.expected).map(|t| (t, *s))).collect();
    let Some(best) = tiered.iter().map(|(t, _)| *t).min() else {
        return vec![];
    };
    let mut v: Vec<_> = tiered
        .into_iter()
        .map(|(t, s)| {
            let starter = t == best && ctx.starter.is_some_and(|r| same_device(r, &s.host_ref));
            ((t, !starter, s.load / LOAD_BUCKET_PCT, tie_hash(ctx.call_id, &s.host_ref)), s.host_ref.clone())
        })
        .collect();
    v.sort();
    v.into_iter().map(|(_, r)| r).collect()
}

/// §2.3 ranking of collected status blobs; refused blobs are skipped.
pub fn rank(
    auth: &(impl CallAuthority + ?Sized),
    statuses: &[SignedStatus],
    ctx: &RankCtx,
) -> Vec<HostRef> {
    let ok: Vec<CallHostStatus> = statuses
        .iter()
        .filter_map(|b| admit(auth, ctx.gid, b, ctx.now_ms).ok())
        .collect();
    rank_admitted(&ok, ctx)
}

/// A host to ask, and the backups its `CallStart` should carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub host: HostRef,
    /// At most `MAX_BACKUPS`, in rank order, never `host`.
    pub backups: Vec<HostRef>,
}

/// First choice and backups from a ranking, skipping `skip` (candidates that refused or did
/// not answer).
pub fn pick_from(ranking: &[HostRef], skip: &BTreeSet<HostRef>) -> Option<Pick> {
    let mut left = ranking.iter().filter(|r| !skip.contains(*r));
    let host = left.next()?.clone();
    Some(Pick { host, backups: left.take(MAX_BACKUPS).cloned().collect() })
}

/// [`rank`] then [`pick_from`] with nothing skipped.
#[cfg(test)]
pub fn pick(
    auth: &(impl CallAuthority + ?Sized),
    statuses: &[SignedStatus],
    ctx: &RankCtx,
) -> Option<Pick> {
    pick_from(&rank(auth, statuses, ctx), &BTreeSet::new())
}

/// What the starter's IO layer does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerStep {
    /// Send `CallHostRequest` (with `backups`) to `host` through the chat host.
    Ask(Pick),
    /// Nobody left: tell the user no call host is available.
    Exhausted,
}

/// The starter's §2.3 picker: ask candidates in rank order; move on at once after a refusal
/// or an unreachable candidate, and after `ANSWER_TIMEOUT_MS` without an answer. The clock is
/// any monotonic ms clock.
#[derive(Debug, Clone)]
pub struct Picker {
    ranking: Vec<HostRef>,
    failed: BTreeSet<HostRef>,
    asking: Option<(HostRef, u64)>,
    done: bool,
}

impl Picker {
    pub fn new(ranking: Vec<HostRef>) -> Self {
        Picker { ranking, failed: BTreeSet::new(), asking: None, done: false }
    }

    /// The candidate currently asked.
    pub fn asking(&self) -> Option<&HostRef> {
        self.asking.as_ref().map(|(h, _)| h)
    }

    fn next(&mut self, now: u64) -> PickerStep {
        match pick_from(&self.ranking, &self.failed) {
            Some(p) => {
                self.asking = Some((p.host.clone(), now));
                PickerStep::Ask(p)
            }
            None => {
                self.asking = None;
                self.done = true;
                PickerStep::Exhausted
            }
        }
    }

    /// First request.
    pub fn start(&mut self, now: u64) -> PickerStep {
        self.next(now)
    }

    /// `host` refused, or could not be reached. Ignored unless it is the one being asked.
    pub fn on_refused(&mut self, host: &HostRef, now: u64) -> Option<PickerStep> {
        if self.done || self.asking() != Some(host) {
            return None;
        }
        self.failed.insert(host.clone());
        Some(self.next(now))
    }

    /// Moves on when the current candidate has not answered within `ANSWER_TIMEOUT_MS`.
    pub fn tick(&mut self, now: u64) -> Option<PickerStep> {
        let (host, since) = self.asking.clone()?;
        if self.done || now < since + ANSWER_TIMEOUT_MS {
            return None;
        }
        self.failed.insert(host);
        Some(self.next(now))
    }

    /// `host` sent a valid `CallStart`. True if it is the one asked; the picker is then done.
    /// A late `CallStart` from an earlier candidate returns false (glare: `CallBoard` settles).
    pub fn on_started(&mut self, host: &HostRef) -> bool {
        let ours = !self.done && self.asking() == Some(host);
        if ours {
            self.done = true;
            self.asking = None;
        }
        ours
    }
}

#[cfg(test)]
mod tests;
