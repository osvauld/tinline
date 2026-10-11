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
//! | 4 | metered, any class and power; only if `expected <= CAP_PHONE_METERED` (G7 for desktops) |
//! | – | busy, thermal >= Moderate, `load >= LOAD_FULL_PCT`, `max_participants < expected`, phone on battery < 30 % |
//!
//! G7: a laptop on battery forwards a few Opus streams for a negligible share of its battery
//! and has the desktop cap, so it ranks with a healthy phone (tier 2); a nearly flat laptop
//! still beats nothing, so it is not excluded but ranked with the weak phones (tier 3). A
//! phone at 30–49 % is still advertising (§2.4) but should be used only after every healthy
//! unmetered host (tier 3). Metered data costs the host's user N−1 streams up, so any metered
//! host is last and held to the metered cap, desktops included.
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

use proto::SignedBlob;
use proto::gcall::MAX_BACKUPS;
use proto::gcall_status::{CallHostStatus, DeviceClass, NetworkKind, PowerState, ThermalLevel, open_status};

use super::policy::{BATTERY_ADVERTISE_PCT, CAP_PHONE_METERED};
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
    /// Participants expected (group members who may join), for `max_participants` and the
    /// metered cap.
    pub expected: usize,
    /// The starter's own device, if it advertises as a candidate (G8).
    pub starter: Option<&'a HostRef>,
    /// Local wall clock, ms since the Unix epoch (same clock as `issued_ms`).
    pub now_ms: u64,
}

/// One admission check, for the collector (cache only what passes) and for [`rank`].
pub fn admit(auth: &(impl CallAuthority + ?Sized), gid: &str, blob: &SignedBlob, now_ms: u64) -> Result<CallHostStatus, Refusal> {
    let s = open_status(blob)?;
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
        return (expected <= CAP_PHONE_METERED).then_some(TIER_METERED);
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

/// Tie-break 3.
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
pub fn rank(auth: &(impl CallAuthority + ?Sized), statuses: &[SignedBlob], ctx: &RankCtx) -> Vec<HostRef> {
    let ok: Vec<CallHostStatus> = statuses.iter().filter_map(|b| admit(auth, ctx.gid, b, ctx.now_ms).ok()).collect();
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
pub fn pick(auth: &(impl CallAuthority + ?Sized), statuses: &[SignedBlob], ctx: &RankCtx) -> Option<Pick> {
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
mod tests {
    use super::super::StaticAuthority;
    use super::*;
    use identity::Identity;
    use proto::gcall::{GCALL_VERSION, MAX_PARTICIPANTS, new_call_id};
    use proto::gcall_status::{STATUS_TTL_MS, sign_status};

    const NOW: u64 = 1_800_000_000_000;

    fn ident(seed: u8) -> Identity {
        let m = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
        identity::recover(&m.to_string()).unwrap()
    }

    fn gid() -> String {
        "f0".repeat(32)
    }

    fn host_ref(id: &Identity, slot: u64) -> HostRef {
        HostRef { holder_did: id.did().into(), device_slot: slot, permit_digest: format!("{}#{slot}", id.did()) }
    }

    fn desktop(r: &HostRef) -> CallHostStatus {
        CallHostStatus {
            v: GCALL_VERSION,
            gid: gid(),
            host_ref: r.clone(),
            class: DeviceClass::Desktop,
            power: PowerState::Mains,
            network: NetworkKind::Unmetered,
            thermal: ThermalLevel::None,
            busy: false,
            load: 0,
            max_participants: MAX_PARTICIPANTS as u8,
            issued_ms: NOW - 1_000,
            ttl_ms: STATUS_TTL_MS,
        }
    }

    fn phone(r: &HostRef, power: PowerState) -> CallHostStatus {
        CallHostStatus { class: DeviceClass::Phone, power, max_participants: 8, ..desktop(r) }
    }

    struct World {
        ids: Vec<Identity>,
        auth: StaticAuthority,
    }

    impl World {
        /// Five members; every device of members 0..=3 has a permit, member 4 has none.
        fn new() -> Self {
            let ids: Vec<Identity> = (1..=5).map(ident).collect();
            let mut auth = StaticAuthority::default();
            for (i, id) in ids.iter().enumerate() {
                auth.members.insert(id.did().into());
                if i < 4 {
                    for slot in 0..2 {
                        auth.permits.insert(host_ref(id, slot).permit_digest);
                    }
                }
            }
            World { ids, auth }
        }
        fn r(&self, i: usize, slot: u64) -> HostRef {
            host_ref(&self.ids[i], slot)
        }
        fn sign(&self, i: usize, s: CallHostStatus) -> SignedBlob {
            sign_status(&self.ids[i], s)
        }
    }

    fn ctx<'a>(cid: &'a str, gid: &'a str, starter: Option<&'a HostRef>) -> RankCtx<'a> {
        RankCtx { gid, call_id: cid, expected: 4, starter, now_ms: NOW }
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    // GC-S04 on the production ranking: every member ranks the same status set, given in a
    // different order, the same way; forged statuses and statuses without a permit are dropped;
    // the starter's own device goes first only in the best tier; tier 0 is ordered by hash.
    #[test]
    fn gcall_rank_gc_s04_same_status_set_gives_identical_ranking_everywhere() {
        let w = World::new();
        let cid = new_call_id();
        let g = gid();
        let (h0, h1, h2, hp) = (w.r(0, 0), w.r(1, 0), w.r(2, 0), w.r(3, 1));
        let mut st = vec![
            w.sign(0, desktop(&h0)),
            w.sign(1, desktop(&h1)),
            w.sign(2, desktop(&h2)),
            w.sign(3, phone(&hp, PowerState::Charging(90))),
        ];
        // Forged: signed by member 1 but claiming member 0's other device.
        let mut forged = w.sign(0, desktop(&w.r(0, 1)));
        forged.signature = w.sign(1, desktop(&h1)).signature;
        st.push(forged);
        // No permit (member 4).
        st.push(w.sign(4, desktop(&w.r(4, 0))));
        let reference = rank(&w.auth, &st, &ctx(&cid, &g, None));
        let mut tier0 = vec![h0.clone(), h1.clone(), h2.clone()];
        tier0.sort_by_key(|r| tie_hash(&cid, r));
        let mut expect = tier0.clone();
        expect.push(hp.clone());
        assert_eq!(reference, expect, "tier 0 ordered by hash, then the charging phone");
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..20 {
            let mut shuffled = st.clone();
            for i in (1..shuffled.len()).rev() {
                shuffled.swap(i, (rng.next() % (i as u64 + 1)) as usize);
            }
            assert_eq!(rank(&w.auth, &shuffled, &ctx(&cid, &g, None)), reference);
        }
        let p = pick(&w.auth, &st, &ctx(&cid, &g, None)).unwrap();
        assert_eq!((p.host, p.backups), (reference[0].clone(), reference[1..=MAX_BACKUPS].to_vec()));
        // Starter preference: in the best tier it goes first; in a worse tier it does not.
        let last0 = tier0.last().unwrap();
        assert_eq!(&rank(&w.auth, &st, &ctx(&cid, &g, Some(last0)))[0], last0);
        assert_eq!(rank(&w.auth, &st, &ctx(&cid, &g, Some(&hp))), reference);
    }

    // G8: the starter preference is per device. Member 0 has two devices (slots 0 and 1) in
    // the same tier; whichever device starts goes first, not "some device of that DID".
    #[test]
    fn gcall_rank_starter_preference_is_per_device_not_per_did() {
        let w = World::new();
        let cid = new_call_id();
        let g = gid();
        let (desk, lap, other) = (w.r(0, 0), w.r(0, 1), w.r(1, 0));
        let st: Vec<CallHostStatus> = vec![desktop(&desk), desktop(&lap), desktop(&other)];
        for r in [&desk, &lap, &other] {
            let got = rank_admitted(&st, &ctx(&cid, &g, Some(r)));
            assert_eq!(&got[0], r);
            // The rest keep their hash order.
            let mut rest: Vec<HostRef> = [&desk, &lap, &other].into_iter().filter(|x| *x != r).cloned().collect();
            rest.sort_by_key(|x| tie_hash(&cid, x));
            assert_eq!(got[1..], rest[..]);
        }
        // A different permit digest on the same device still matches (permit reissued).
        let reissued = HostRef { permit_digest: "new".into(), ..desk.clone() };
        assert_eq!(rank_admitted(&st, &ctx(&cid, &g, Some(&reissued)))[0], desk);
    }

    #[test]
    fn gcall_rank_tiers_and_exclusions() {
        let w = World::new();
        let r = w.r(0, 0);
        let rows: Vec<(&str, CallHostStatus, usize, Option<u8>)> = vec![
            ("desktop mains unmetered", desktop(&r), 4, Some(0)),
            ("headless mains", CallHostStatus { class: DeviceClass::Headless, ..desktop(&r) }, 4, Some(0)),
            ("laptop charging", CallHostStatus { power: PowerState::Charging(40), ..desktop(&r) }, 4, Some(0)),
            ("phone charging", phone(&r, PowerState::Charging(10)), 4, Some(1)),
            ("phone mains", phone(&r, PowerState::Mains), 4, Some(1)),
            ("phone 50 %", phone(&r, PowerState::Battery(50)), 4, Some(2)),
            ("laptop on battery 30 % (G7)", CallHostStatus { power: PowerState::Battery(30), ..desktop(&r) }, 4, Some(2)),
            ("phone 49 % (G7)", phone(&r, PowerState::Battery(49)), 4, Some(3)),
            ("phone 30 % (G7)", phone(&r, PowerState::Battery(30)), 4, Some(3)),
            ("laptop on battery 29 % (G7)", CallHostStatus { power: PowerState::Battery(29), ..desktop(&r) }, 4, Some(3)),
            ("phone 29 %", phone(&r, PowerState::Battery(29)), 4, None),
            ("phone metered, N = cap", CallHostStatus { network: NetworkKind::Metered, ..phone(&r, PowerState::Mains) }, CAP_PHONE_METERED, Some(4)),
            ("phone metered, N > cap", CallHostStatus { network: NetworkKind::Metered, ..phone(&r, PowerState::Mains) }, CAP_PHONE_METERED + 1, None),
            ("phone metered 29 %", CallHostStatus { network: NetworkKind::Metered, ..phone(&r, PowerState::Battery(29)) }, 2, None),
            ("desktop metered (G7)", CallHostStatus { network: NetworkKind::Metered, ..desktop(&r) }, CAP_PHONE_METERED, Some(4)),
            ("desktop metered, N > cap (G7)", CallHostStatus { network: NetworkKind::Metered, ..desktop(&r) }, CAP_PHONE_METERED + 1, None),
            ("thermal light", CallHostStatus { thermal: ThermalLevel::Light, ..desktop(&r) }, 4, Some(0)),
            ("thermal moderate", CallHostStatus { thermal: ThermalLevel::Moderate, ..desktop(&r) }, 4, None),
            ("busy", CallHostStatus { busy: true, ..desktop(&r) }, 4, None),
            ("load full (G6)", CallHostStatus { load: LOAD_FULL_PCT, ..desktop(&r) }, 4, None),
            ("load just below full", CallHostStatus { load: LOAD_FULL_PCT - 1, ..desktop(&r) }, 4, Some(0)),
            ("too small", CallHostStatus { max_participants: 3, ..desktop(&r) }, 4, None),
            ("phone Wi-Fi cap", phone(&r, PowerState::Mains), 9, None),
        ];
        for (case, s, n, want) in rows {
            assert_eq!(tier(&s, n), want, "{case}");
        }
    }

    // Tie-break order: tier, then starter, then load bucket (G6), then hash.
    #[test]
    fn gcall_rank_load_breaks_ties_in_buckets_before_the_hash() {
        let w = World::new();
        let cid = new_call_id();
        let g = gid();
        let (a, b, c) = (w.r(0, 0), w.r(1, 0), w.r(2, 0));
        let st = vec![
            CallHostStatus { load: 60, ..desktop(&a) },
            CallHostStatus { load: 10, ..desktop(&b) },
            CallHostStatus { load: 20, ..desktop(&c) },
            phone(&w.r(3, 0), PowerState::Mains),
        ];
        let got = rank_admitted(&st, &ctx(&cid, &g, None));
        // b and c share bucket 0 (hash decides), a is in bucket 2; the phone is tier 1.
        let mut low = vec![b.clone(), c.clone()];
        low.sort_by_key(|r| tie_hash(&cid, r));
        assert_eq!(got, [low, vec![a.clone(), w.r(3, 0)]].concat());
        // The starter beats a lower load in the same tier.
        assert_eq!(rank_admitted(&st, &ctx(&cid, &g, Some(&a)))[0], a);
        // The hash tie-break depends on the call: some other call id orders b and c the other way.
        let flips = (0..64).any(|_| {
            let other = new_call_id();
            (tie_hash(&other, &b) < tie_hash(&other, &c)) != (tie_hash(&cid, &b) < tie_hash(&cid, &c))
        });
        assert!(flips);
    }

    #[test]
    fn gcall_rank_excludes_expired_foreign_and_unpermitted_statuses() {
        let w = World::new();
        let cid = new_call_id();
        let g = gid();
        let (a, b, c) = (w.r(0, 0), w.r(1, 0), w.r(2, 0));
        // TTL: alive for exactly ttl_ms from issued_ms.
        let old = w.sign(0, CallHostStatus { issued_ms: NOW - STATUS_TTL_MS, ..desktop(&a) });
        let fresh = w.sign(1, CallHostStatus { issued_ms: NOW - STATUS_TTL_MS + 1, ..desktop(&b) });
        let short = w.sign(2, CallHostStatus { issued_ms: NOW - 1_000, ttl_ms: 1_000, ..desktop(&c) });
        assert_eq!(rank(&w.auth, &[old.clone(), fresh.clone(), short.clone()], &ctx(&cid, &g, None)), vec![b.clone()]);
        assert_eq!(admit(&w.auth, &g, &old, NOW), Err(Refusal::Stale));
        // Too far in the future.
        let future = w.sign(0, CallHostStatus { issued_ms: NOW + 60_000, ..desktop(&a) });
        assert_eq!(admit(&w.auth, &g, &future, NOW), Err(Refusal::Stale));
        // Another group's status.
        let foreign = w.sign(0, CallHostStatus { gid: "other".into(), ..desktop(&a) });
        assert_eq!(admit(&w.auth, &g, &foreign, NOW), Err(Refusal::WrongGroup));
        // Revoked permit, removed holder, never permitted.
        let mut auth = w.auth.clone();
        auth.permits.remove(&a.permit_digest);
        let sa = w.sign(0, desktop(&a));
        assert_eq!(admit(&auth, &g, &sa, NOW), Err(Refusal::PermitInvalid));
        let mut auth = w.auth.clone();
        auth.removed.insert(w.ids[1].did().into());
        assert_eq!(admit(&auth, &g, &fresh, NOW), Err(Refusal::PermitInvalid));
        assert_eq!(admit(&w.auth, &g, &w.sign(4, desktop(&w.r(4, 0))), NOW), Err(Refusal::PermitInvalid));
        // Forged.
        let mut forged = sa.clone();
        forged.signature = fresh.signature.clone();
        assert_eq!(admit(&w.auth, &g, &forged, NOW), Err(Refusal::BadSignature));
        assert!(rank(&w.auth, &[forged, foreign, future], &ctx(&cid, &g, None)).is_empty());
    }

    // The latest status per device wins, whatever the order: a device that turned busy drops
    // out even though its older idle status is still alive.
    #[test]
    fn gcall_rank_uses_the_latest_status_per_device() {
        let w = World::new();
        let cid = new_call_id();
        let g = gid();
        let a = w.r(0, 0);
        let idle = desktop(&a);
        let busy = CallHostStatus { busy: true, issued_ms: idle.issued_ms + 1, ..desktop(&a) };
        let other = desktop(&w.r(1, 0));
        for st in [vec![idle.clone(), busy.clone(), other.clone()], vec![busy, idle, other]] {
            assert_eq!(rank_admitted(&st, &ctx(&cid, &g, None)), vec![w.r(1, 0)]);
        }
        // Same issued_ms, different content: still order-independent.
        let x = CallHostStatus { load: 50, ..desktop(&a) };
        let y = CallHostStatus { power: PowerState::Battery(60), ..desktop(&a) };
        let one = rank_admitted(&[x.clone(), y.clone()], &ctx(&cid, &g, None));
        assert_eq!(one, rank_admitted(&[y, x], &ctx(&cid, &g, None)));
        assert_eq!(one.len(), 1);
    }

    // The picker (§2.3): the first candidate refuses, the second does not answer within
    // ANSWER_TIMEOUT_MS, the third hosts. Backups skip candidates that already failed.
    #[test]
    fn gcall_rank_picker_moves_on_after_refusal_and_timeout() {
        let w = World::new();
        let ranking: Vec<HostRef> = (0..4).map(|i| w.r(i, 0)).collect();
        let mut p = Picker::new(ranking.clone());
        let t0 = 1_000;
        let ask = |h: usize, b: &[usize]| {
            PickerStep::Ask(Pick { host: ranking[h].clone(), backups: b.iter().map(|&i| ranking[i].clone()).collect() })
        };
        assert_eq!(p.start(t0), ask(0, &[1, 2]));
        // A refusal from someone not being asked is ignored.
        assert_eq!(p.on_refused(&ranking[2], t0 + 10), None);
        assert_eq!(p.on_refused(&ranking[0], t0 + 10), Some(ask(1, &[2, 3])));
        assert_eq!(p.tick(t0 + 10 + ANSWER_TIMEOUT_MS - 1), None);
        assert_eq!(p.tick(t0 + 10 + ANSWER_TIMEOUT_MS), Some(ask(2, &[3])));
        // A late CallStart from the timed-out candidate is not ours.
        assert!(!p.on_started(&ranking[1]));
        assert!(p.on_started(&ranking[2]));
        assert_eq!(p.tick(t0 + 60_000), None);
        assert_eq!(p.asking(), None);
        // Everyone refuses.
        let mut p = Picker::new(ranking[..2].to_vec());
        p.start(0);
        p.on_refused(&ranking[0], 1);
        assert_eq!(p.on_refused(&ranking[1], 2), Some(PickerStep::Exhausted));
        assert_eq!(Picker::new(vec![]).start(0), PickerStep::Exhausted);
    }
}
