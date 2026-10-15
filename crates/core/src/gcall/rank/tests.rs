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
    CallHostStatus { class: DeviceClass::Phone, power, ..desktop(r) }
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
        ("phone metered, N = 12", CallHostStatus { network: NetworkKind::Metered, ..phone(&r, PowerState::Mains) }, MAX_PARTICIPANTS, Some(4)),
        ("phone metered 29 %", CallHostStatus { network: NetworkKind::Metered, ..phone(&r, PowerState::Battery(29)) }, 2, None),
        ("desktop metered, N = 12 (G7)", CallHostStatus { network: NetworkKind::Metered, ..desktop(&r) }, MAX_PARTICIPANTS, Some(4)),
        ("thermal light", CallHostStatus { thermal: ThermalLevel::Light, ..desktop(&r) }, 4, Some(0)),
        ("thermal moderate", CallHostStatus { thermal: ThermalLevel::Moderate, ..desktop(&r) }, 4, None),
        ("busy", CallHostStatus { busy: true, ..desktop(&r) }, 4, None),
        ("load full (G6)", CallHostStatus { load: LOAD_FULL_PCT, ..desktop(&r) }, 4, None),
        ("load just below full", CallHostStatus { load: LOAD_FULL_PCT - 1, ..desktop(&r) }, 4, Some(0)),
        ("too small", CallHostStatus { max_participants: 3, ..desktop(&r) }, 4, None),
        ("phone takes 12", phone(&r, PowerState::Mains), MAX_PARTICIPANTS, Some(1)),
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
