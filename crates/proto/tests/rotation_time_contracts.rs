//! C1 time contracts: deterministic fake monotonic + wall clocks driving a candidate
//! rotation scheduler. TEST-ONLY. Every parameter value here (intervals, minimums,
//! validity, skew, jitter, floors) is a fixture, NOT a product default. G-TIME and
//! G-RENEWAL are unresolved; options are modelled side by side as candidate policies.
//!
//! Model boundaries: in-memory state only; "persist" is an ideal atomic struct
//! assignment; no OS clock, no iroh, no filesystem. Owner signatures and endpoint
//! possession proofs are real Ed25519.
use std::collections::{BTreeSet, HashSet};

use ed25519_dalek::{Signer, SigningKey};
use identity::Identity;
use serde::Serialize;
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Copied from rotation_permit_model.rs @01ee106; coordinator consolidates later.
// Only change: `Serialize` derives on `View` so rejection tests can compare bytes.
// ---------------------------------------------------------------------------
const DOMAIN: &[u8] = b"test-only/device-succession-permit/v1\0";
const PAGE: usize = 4;

#[derive(Clone, Debug, Serialize)]
struct Claims {
    version: u8,
    owner: String,
    audience: String,
    previous_id: [u8; 32],
    replacement_id: [u8; 32],
    endpoint: [u8; 32],
    predecessor: [u8; 32],
    generation: u64,
    issued_at: u64,
    expires_at: u64,
}

#[derive(Clone)]
struct Permit {
    claims: Claims,
    signature: [u8; 64],
}

impl Permit {
    fn bytes(claims: &Claims) -> Vec<u8> {
        [DOMAIN, &serde_json::to_vec(claims).unwrap()].concat()
    }
    fn issue(owner: &Identity, claims: Claims) -> Self {
        let signature = owner.sign(&Self::bytes(&claims));
        Self { claims, signature }
    }
    fn id(&self) -> [u8; 32] {
        Sha256::digest(Self::bytes(&self.claims)).into()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct View {
    owner: String,
    audience: String,
    anchor: [u8; 32],
    head: [u8; 32],
    current_id: [u8; 32],
    endpoint: [u8; 32],
    generation: u64,
    expires_at: u64,
}

impl View {
    fn initial(owner: &Identity, device_id: u8) -> Self {
        let anchor = Sha256::digest([owner.did().as_bytes(), &[device_id]].concat()).into();
        Self {
            owner: owner.did().into(),
            audience: "private".into(),
            anchor,
            head: anchor,
            current_id: [device_id; 32],
            endpoint: [0; 32],
            generation: 0,
            expires_at: 0,
        }
    }

    fn page(
        &mut self,
        permits: &[Permit],
        now: u64,
        revoked: &HashSet<[u8; 32]>,
    ) -> Result<(), &'static str> {
        if permits.is_empty() || permits.len() > PAGE {
            return Err("page_limit");
        }
        let mut candidate = self.clone();
        for permit in permits {
            let c = &permit.claims;
            let key = identity::public_key_from_did(&candidate.owner).unwrap();
            if c.owner != candidate.owner
                || !identity::verify(&key, &Permit::bytes(c), &permit.signature)
            {
                return Err("owner_signature");
            }
            if c.version != 1 || c.audience != candidate.audience {
                return Err("scope");
            }
            if c.issued_at >= c.expires_at || c.issued_at > now {
                return Err("validity");
            }
            if revoked.contains(&candidate.anchor)
                || revoked.contains(&candidate.head)
                || revoked.contains(&permit.id())
            {
                return Err("revoked");
            }
            if c.generation == candidate.generation {
                if permit.id() == candidate.head {
                    continue;
                }
                return Err("fork");
            }
            if c.generation < candidate.generation {
                return Err("stale");
            }
            if candidate.generation.checked_add(1) != Some(c.generation) {
                return Err("gap");
            }
            if c.predecessor != candidate.head || c.previous_id != candidate.current_id {
                return Err("predecessor");
            }
            if c.replacement_id == c.previous_id {
                return Err("unchanged_id");
            }
            candidate.head = permit.id();
            candidate.current_id = c.replacement_id;
            candidate.endpoint = c.endpoint;
            candidate.generation = c.generation;
            candidate.expires_at = c.expires_at;
        }
        *self = candidate;
        Ok(())
    }

    fn live(
        &self,
        now: u64,
        revoked: &HashSet<[u8; 32]>,
        challenge: &[u8],
        proof: &[u8; 64],
    ) -> bool {
        self.generation > 0
            && now < self.expires_at
            && !revoked.contains(&self.anchor)
            && !revoked.contains(&self.head)
            && identity::verify(&self.endpoint, challenge, proof)
    }
}
// ------------------------------- end of copy -------------------------------

fn no_revocations() -> HashSet<[u8; 32]> {
    HashSet::new()
}

/// Deterministic fixture endpoint key per (seed, generation). NOT a production scheme;
/// this is not a derivation proposal (G-DISCOVERY), only test determinism.
fn fixture_key(seed: u8, generation: u64) -> SigningKey {
    let mut h = Sha256::new();
    h.update(b"test-only/c1-fixture-endpoint\0");
    h.update([seed]);
    h.update(generation.to_be_bytes());
    SigningKey::from_bytes(&h.finalize().into())
}

// ---------------------------------------------------------------------------
// Fake clocks
// ---------------------------------------------------------------------------

/// Two independent fake clocks. `monotonic` never decreases within a boot and resets
/// to 0 on reboot. `wall` can be set to any value (NTP step, user change, RTC loss).
/// `monotonic_counts_suspend` distinguishes a BOOTTIME-like source (true) from a
/// CLOCK_MONOTONIC-like source that pauses during device suspend (false).
#[derive(Clone, Debug)]
struct Clocks {
    boot: u64,
    monotonic: u64,
    wall: u64,
    monotonic_counts_suspend: bool,
}

impl Clocks {
    fn new(wall: u64) -> Self {
        Self {
            boot: 0,
            monotonic: 0,
            wall,
            monotonic_counts_suspend: true,
        }
    }
    /// Device awake for `d` units: both clocks advance together.
    fn run(&mut self, d: u64) {
        self.monotonic += d;
        self.wall += d;
    }
    /// Device suspended for `d` units (no code runs).
    fn suspend(&mut self, d: u64) {
        self.wall += d;
        if self.monotonic_counts_suspend {
            self.monotonic += d;
        }
    }
    /// Powered off for `d` units, then booted: monotonic restarts at zero.
    fn reboot(&mut self, d: u64) {
        self.wall += d;
        self.boot += 1;
        self.monotonic = 0;
    }
    fn set_wall(&mut self, wall: u64) {
        self.wall = wall;
    }
}

// ---------------------------------------------------------------------------
// Candidate scheduler
// ---------------------------------------------------------------------------

/// How an interval change takes effect (G-TIME option).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum ChangePolicy {
    /// next_deadline = elapsed_now + new_interval.
    FromNow,
    /// next_deadline = last_rotation_at + new_interval, but not earlier than
    /// now + min_floor, and a shortening never moves the deadline later.
    FromLastRotation { min_floor: u64 },
}

/// Where the next deadline lands after a (possibly overdue) rotation (G-TIME option).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum OverduePolicy {
    /// Existing candidate (rotation_permit_lifecycle.rs:33): resume + interval.
    ResumePlusInterval,
    /// Next point on the grid deadline + k*interval strictly after now.
    AlignedGrid,
}

/// How downtime is credited after a reboot, when monotonic time is lost (G-TIME option).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum RebootPolicy {
    /// Credit max(0, wall_now - wall_checkpoint). Any rollback credits 0 downtime.
    CreditNonNegative,
    /// As above, but a rollback larger than `skew` makes the schedule immediately due.
    OverdueOnRollback { skew: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct Params {
    min_interval: u64,
    max_interval: u64,
    /// Signed binding validity in WALL units (issued_at = sender wall now).
    validity: u64,
    change: ChangePolicy,
    overdue: OverduePolicy,
    reboot: RebootPolicy,
}

const TEST_PARAMS: Params = Params {
    min_interval: 5,
    max_interval: 100,
    validity: 15,
    change: ChangePolicy::FromNow,
    overdue: OverduePolicy::ResumePlusInterval,
    reboot: RebootPolicy::CreditNonNegative,
};

/// Durable scheduler state. Scheduler time is `elapsed`: accumulated monotonic time
/// (plus policy-credited reboot downtime). It never decreases. Wall time only feeds
/// signed validity and reboot crediting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Persisted {
    params: Params,
    generation: u64,
    interval: u64,
    elapsed: u64,
    last_rotation_at: u64,
    next_deadline: u64,
    wall_checkpoint: u64,
    seed: u8,
    view: View,
    secret: [u8; 32],
}

/// Volatile: lost on reboot.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Volatile {
    boot: u64,
    mono_checkpoint: u64,
}

#[derive(Clone, Debug)]
struct Device {
    durable: Persisted,
    volatile: Option<Volatile>,
    /// Block policy is deliberately a separate store from the schedule.
    blocked: BTreeSet<String>,
}

impl Device {
    fn new(owner: &Identity, seed: u8, params: Params, interval: u64, clocks: &Clocks) -> Self {
        assert!(interval >= params.min_interval && interval <= params.max_interval);
        let key = fixture_key(seed, 0);
        let mut view = View::initial(owner, 200);
        view.endpoint = key.verifying_key().to_bytes();
        let durable = Persisted {
            params,
            generation: 0,
            interval,
            elapsed: 0,
            last_rotation_at: 0,
            next_deadline: interval,
            wall_checkpoint: clocks.wall,
            seed,
            view,
            secret: key.to_bytes(),
        };
        Self {
            durable,
            volatile: Some(Volatile {
                boot: clocks.boot,
                mono_checkpoint: clocks.monotonic,
            }),
            blocked: BTreeSet::new(),
        }
    }

    fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&self.durable).unwrap()
    }

    /// Simulated power loss: only `durable` (and the separate block store) survives.
    fn crash(&mut self) {
        self.volatile = None;
    }

    /// Fold clock readings into scheduler time. Atomic: on error nothing changes.
    fn sync(&mut self, clocks: &Clocks) -> Result<(), &'static str> {
        let mut d = self.durable.clone();
        let mut force_due = false;
        let credit = match &self.volatile {
            Some(v) if v.boot == clocks.boot => clocks
                .monotonic
                .checked_sub(v.mono_checkpoint)
                .ok_or("monotonic_regression")?,
            _ => {
                // New boot (or crash): monotonic checkpoint lost. Wall time since the
                // last durable checkpoint should be at least the monotonic time since
                // boot; credit the larger. A shortfall means the wall clock went back.
                let expected = d
                    .wall_checkpoint
                    .checked_add(clocks.monotonic)
                    .ok_or("overflow")?;
                if clocks.wall >= expected {
                    clocks.wall - d.wall_checkpoint
                } else {
                    if let RebootPolicy::OverdueOnRollback { skew } = d.params.reboot {
                        force_due = expected - clocks.wall > skew;
                    }
                    clocks.monotonic
                }
            }
        };
        d.elapsed = d.elapsed.checked_add(credit).ok_or("overflow")?;
        if force_due {
            d.next_deadline = d.next_deadline.min(d.elapsed);
        }
        d.wall_checkpoint = clocks.wall;
        self.durable = d;
        self.volatile = Some(Volatile {
            boot: clocks.boot,
            mono_checkpoint: clocks.monotonic,
        });
        Ok(())
    }

    /// Rotate if due. `fail` injects key-generation/endpoint-startup failure.
    /// Returns the signed successor permit when a rotation committed.
    fn poll(
        &mut self,
        owner: &Identity,
        clocks: &Clocks,
        fail: bool,
    ) -> Result<Option<Permit>, &'static str> {
        self.sync(clocks)?;
        let s = &self.durable;
        if s.elapsed < s.next_deadline {
            return Ok(None);
        }
        if fail {
            return Err("startup_failed");
        }
        let mut d = s.clone();
        let generation = d.generation.checked_add(1).ok_or("overflow")?;
        let key = fixture_key(d.seed, generation);
        let expires_at = clocks
            .wall
            .checked_add(d.params.validity)
            .ok_or("overflow")?;
        // Fixture replacement IDs: 1, 2, 3, ... (initial is 200). NOT production IDs.
        let permit = Permit::issue(
            owner,
            Claims {
                version: 1,
                owner: owner.did().into(),
                audience: d.view.audience.clone(),
                previous_id: d.view.current_id,
                replacement_id: [generation as u8; 32],
                endpoint: key.verifying_key().to_bytes(),
                predecessor: d.view.head,
                generation,
                issued_at: clocks.wall,
                expires_at,
            },
        );
        d.view.page(
            std::slice::from_ref(&permit),
            clocks.wall,
            &no_revocations(),
        )?;
        d.generation = generation;
        d.secret = key.to_bytes();
        d.last_rotation_at = d.elapsed;
        d.next_deadline = match d.params.overdue {
            OverduePolicy::ResumePlusInterval => d.elapsed.checked_add(d.interval),
            OverduePolicy::AlignedGrid => {
                let missed = (d.elapsed - d.next_deadline) / d.interval + 1;
                missed
                    .checked_mul(d.interval)
                    .and_then(|x| x.checked_add(d.next_deadline))
            }
        }
        .ok_or("overflow")?;
        self.durable = d;
        Ok(Some(permit))
    }

    /// Change the configured interval. Atomic: invalid input leaves state unchanged.
    fn set_interval(&mut self, clocks: &Clocks, new: u64) -> Result<(), &'static str> {
        self.sync(clocks)?;
        let p = self.durable.params;
        if new == 0 {
            return Err("zero");
        }
        if new < p.min_interval {
            return Err("below_min");
        }
        if new > p.max_interval {
            return Err("above_max");
        }
        let mut d = self.durable.clone();
        d.next_deadline = match p.change {
            ChangePolicy::FromNow => d.elapsed.checked_add(new).ok_or("overflow")?,
            ChangePolicy::FromLastRotation { min_floor } => {
                let from_last = d.last_rotation_at.checked_add(new).ok_or("overflow")?;
                let floor = d.elapsed.checked_add(min_floor).ok_or("overflow")?;
                let cand = from_last.max(floor);
                if new <= d.interval {
                    cand.min(d.next_deadline)
                } else {
                    cand
                }
            }
        };
        d.interval = new;
        self.durable = d;
        Ok(())
    }

    fn block(&mut self, did: &str) {
        self.blocked.insert(did.into());
    }
    fn unblock(&mut self, did: &str) {
        self.blocked.remove(did);
    }

    fn proof(&self, challenge: &[u8]) -> [u8; 64] {
        SigningKey::from_bytes(&self.durable.secret)
            .sign(challenge)
            .to_bytes()
    }
}

/// Receiver fixture: verified view plus its own (correct) wall clock.
struct Receiver {
    view: View,
}

impl Receiver {
    fn new(owner: &Identity) -> Self {
        Self {
            view: View::initial(owner, 200),
        }
    }
    fn accept(&mut self, p: &Permit, wall: u64) -> Result<(), &'static str> {
        self.view
            .page(std::slice::from_ref(p), wall, &no_revocations())
    }
    fn reachable(&self, sender: &Device, wall: u64) -> bool {
        let challenge = b"test-only/c1-session";
        self.view.endpoint == sender.durable.view.endpoint
            && self
                .view
                .live(wall, &no_revocations(), challenge, &sender.proof(challenge))
    }
}

fn params(change: ChangePolicy, overdue: OverduePolicy, reboot: RebootPolicy) -> Params {
    Params {
        change,
        overdue,
        reboot,
        ..TEST_PARAMS
    }
}

// ---------------------------------------------------------------------------
// 1. Deadline independence from blocking
// ---------------------------------------------------------------------------

// KIND: invariant
#[test]
fn no_blocks_rotates_at_each_deadline_and_block_unblock_never_moves_deadline() {
    let (alice, _) = identity::generate();
    let (carol, _) = identity::generate();
    let mut c = Clocks::new(1_000);
    let mut dev = Device::new(&alice, 1, TEST_PARAMS, 10, &c);
    let mut rotated_at = Vec::new();
    for step in 1..=35u64 {
        c.run(1);
        if step % 3 == 0 {
            dev.block(carol.did());
        }
        if step % 7 == 0 {
            dev.unblock(carol.did());
        }
        let before = dev.durable.clone();
        dev.block(carol.did());
        dev.unblock(carol.did());
        dev.block(carol.did());
        assert_eq!(dev.durable, before, "block/unblock touched the schedule");
        if dev.poll(&alice, &c, false).unwrap().is_some() {
            rotated_at.push(step);
        }
    }
    assert_eq!(rotated_at, vec![10, 20, 30]);
    assert_eq!(dev.durable.generation, 3);
    assert_eq!(dev.durable.next_deadline, 40);

    // Same schedule with no block ever applied.
    let mut c2 = Clocks::new(1_000);
    let mut plain = Device::new(&alice, 1, TEST_PARAMS, 10, &c2);
    let mut plain_at = Vec::new();
    for step in 1..=35u64 {
        c2.run(1);
        if plain.poll(&alice, &c2, false).unwrap().is_some() {
            plain_at.push(step);
        }
    }
    assert_eq!(plain_at, rotated_at);
    assert_eq!(plain.bytes(), dev.bytes());
}

// ---------------------------------------------------------------------------
// 2. Interval configuration
// ---------------------------------------------------------------------------

// KIND: invariant
#[test]
fn invalid_interval_inputs_are_rejected_byte_for_byte_unchanged() {
    let (alice, _) = identity::generate();
    for change in [
        ChangePolicy::FromNow,
        ChangePolicy::FromLastRotation { min_floor: 3 },
    ] {
        let wide = Params {
            max_interval: u64::MAX,
            ..params(
                change,
                OverduePolicy::ResumePlusInterval,
                RebootPolicy::CreditNonNegative,
            )
        };
        let mut c = Clocks::new(1_000);
        let mut dev = Device::new(&alice, 1, wide, 10, &c);
        c.run(10);
        dev.poll(&alice, &c, false).unwrap().unwrap(); // last_rotation_at = 10
        c.run(3);
        dev.sync(&c).unwrap();
        let before = dev.bytes();
        for (input, error) in [
            (0, "zero"),
            (1, "below_min"),
            (4, "below_min"),
            (u64::MAX, "overflow"),
            (u64::MAX - 7, "overflow"),
        ] {
            assert_eq!(
                dev.set_interval(&c, input),
                Err(error),
                "{change:?} {input}"
            );
            assert_eq!(dev.bytes(), before);
        }
        let mut narrow = Device::new(
            &alice,
            1,
            params(
                change,
                OverduePolicy::ResumePlusInterval,
                RebootPolicy::CreditNonNegative,
            ),
            10,
            &c,
        );
        let before = narrow.bytes();
        assert_eq!(narrow.set_interval(&c, 101), Err("above_max"));
        assert_eq!(narrow.bytes(), before);
    }
}

// KIND: invariant
#[test]
fn monotonic_regression_within_one_boot_is_rejected_unchanged() {
    let (alice, _) = identity::generate();
    let mut c = Clocks::new(1_000);
    let mut dev = Device::new(&alice, 1, TEST_PARAMS, 10, &c);
    c.run(6);
    dev.sync(&c).unwrap();
    let before = dev.bytes();
    let bad = Clocks {
        monotonic: 2,
        ..c.clone()
    };
    assert_eq!(
        dev.poll(&alice, &bad, false).err(),
        Some("monotonic_regression")
    );
    assert_eq!(dev.set_interval(&bad, 20), Err("monotonic_regression"));
    assert_eq!(dev.bytes(), before);
}

// KIND: candidate-policy G-TIME
#[test]
fn interval_change_apply_from_now_shorter_and_longer_near_deadline() {
    let (alice, _) = identity::generate();
    let p = params(
        ChangePolicy::FromNow,
        OverduePolicy::ResumePlusInterval,
        RebootPolicy::CreditNonNegative,
    );
    for (new, expected_deadline) in [(5, 13), (20, 28), (10, 18)] {
        let mut c = Clocks::new(1_000);
        let mut dev = Device::new(&alice, 1, p, 10, &c);
        c.run(8); // deadline 10 is 2 units away
        dev.set_interval(&c, new).unwrap();
        assert_eq!(dev.durable.next_deadline, expected_deadline);
        assert_eq!(dev.durable.generation, 0);
        // Rotation occurs exactly at the new deadline, then follows the new interval.
        let mut rotations = Vec::new();
        for _ in 0..(expected_deadline - 8 + new) {
            c.run(1);
            if dev.poll(&alice, &c, false).unwrap().is_some() {
                rotations.push(dev.durable.elapsed);
            }
        }
        assert_eq!(rotations, vec![expected_deadline, expected_deadline + new]);
    }
}

// KIND: limitation
#[test]
fn apply_from_now_lets_repeated_reconfiguration_postpone_rotation_indefinitely() {
    // Demonstrates a missing guarantee of the FromNow option: re-saving the setting
    // (even to the same value) shortly before each deadline keeps the old endpoint.
    let (alice, _) = identity::generate();
    let p = params(
        ChangePolicy::FromNow,
        OverduePolicy::ResumePlusInterval,
        RebootPolicy::CreditNonNegative,
    );
    let mut c = Clocks::new(1_000);
    let mut dev = Device::new(&alice, 1, p, 10, &c);
    for _ in 0..50 {
        c.run(9);
        dev.set_interval(&c, 10).unwrap();
        assert!(dev.poll(&alice, &c, false).unwrap().is_none());
    }
    assert_eq!(dev.durable.elapsed, 450);
    assert_eq!(
        dev.durable.generation, 0,
        "45 intervals elapsed without rotation"
    );
}

// KIND: candidate-policy G-TIME
#[test]
fn interval_change_from_last_rotation_with_min_floor_cannot_postpone_rotation() {
    let (alice, _) = identity::generate();
    let p = params(
        ChangePolicy::FromLastRotation { min_floor: 3 },
        OverduePolicy::ResumePlusInterval,
        RebootPolicy::CreditNonNegative,
    );
    // Near-deadline changes at elapsed 8 (last rotation 0, deadline 10).
    for (new, expected) in [
        (5, 10),  // last+5=5 is past; floor now+3=11; shortening never later than 10
        (20, 20), // lengthening counts from last rotation, not from now
        (10, 10), // re-saving same value is a no-op
    ] {
        let mut c = Clocks::new(1_000);
        let mut dev = Device::new(&alice, 1, p, 10, &c);
        c.run(8);
        dev.set_interval(&c, new).unwrap();
        assert_eq!(dev.durable.next_deadline, expected, "new={new}");
    }
    // Earlier shortening where the floor binds: at elapsed 2, set 5 -> max(5, 2+3) = 5.
    let mut c = Clocks::new(1_000);
    let mut dev = Device::new(&alice, 1, p, 10, &c);
    c.run(4);
    dev.set_interval(&c, 5).unwrap();
    assert_eq!(dev.durable.next_deadline, 7); // last+5=5 < now+3=7 -> 7 (< old 10)

    // Repeated reconfiguration cannot postpone: deadline <= last_rotation_at + max_interval
    // and re-saving before each deadline still rotates on the configured cadence.
    let mut c = Clocks::new(1_000);
    let mut dev = Device::new(&alice, 1, p, 10, &c);
    for _ in 0..50 {
        c.run(9);
        dev.set_interval(&c, 10).unwrap();
        dev.poll(&alice, &c, false).unwrap();
        let s = &dev.durable;
        assert!(s.next_deadline <= s.last_rotation_at + s.params.max_interval);
    }
    // Identical to a control device that was never reconfigured (polled every 9 units,
    // so ResumePlusInterval rotates every 18): reconfiguration had no effect.
    let mut c2 = Clocks::new(1_000);
    let mut control = Device::new(&alice, 1, p, 10, &c2);
    for _ in 0..50 {
        c2.run(9);
        control.poll(&alice, &c2, false).unwrap();
    }
    assert_eq!(dev.bytes(), control.bytes());
    assert_eq!(dev.durable.generation, 25);
}

// KIND: candidate-policy G-TIME
#[test]
fn seeded_jitter_keeps_deadline_in_bounded_window_and_independent_of_blocks() {
    // Jitter is NOT in the scheduler; this models the option as a pre-computed,
    // deterministic per-rotation offset applied through FromLastRotation semantics.
    // Open questions: whether jitter exists, its width, and whether it is persisted.
    let (alice, _) = identity::generate();
    let (carol, _) = identity::generate();
    let jitter = [0u64, 3, 1, 2, 0, 3];
    let p = params(
        ChangePolicy::FromLastRotation { min_floor: 0 },
        OverduePolicy::ResumePlusInterval,
        RebootPolicy::CreditNonNegative,
    );
    let mut c = Clocks::new(1_000);
    let mut dev = Device::new(&alice, 1, p, 10, &c);
    let mut at = Vec::new();
    let mut last = 0;
    for j in jitter {
        dev.durable.next_deadline = last + 10 + j; // fixture hook standing for jittered scheduling
        while dev.durable.generation as usize == at.len() {
            c.run(1);
            dev.block(carol.did());
            dev.poll(&alice, &c, false).unwrap();
        }
        last = dev.durable.last_rotation_at;
        at.push(last);
    }
    assert_eq!(at, vec![10, 23, 34, 46, 56, 69]);
    for w in at.windows(2) {
        assert!((10..=13).contains(&(w[1] - w[0])));
    }
}

// ---------------------------------------------------------------------------
// 3. Sleep/resume and restart with k missed periods
// ---------------------------------------------------------------------------

fn overdue_cases(overdue: OverduePolicy) -> Vec<(u64, u64, u64)> {
    // (k missed periods, resume elapsed, expected next deadline) for interval 10,
    // first deadline 10. resume = 10 + 10k + 3 (after); see also before/at below.
    (0..=4u64)
        .map(|k| {
            let resume = 10 + 10 * k + 3;
            let next = match overdue {
                OverduePolicy::ResumePlusInterval => resume + 10,
                OverduePolicy::AlignedGrid => 10 * (k + 2),
            };
            (k, resume, next)
        })
        .collect()
}

fn check_resume_policy(overdue: OverduePolicy) {
    let (alice, _) = identity::generate();
    let p = params(
        ChangePolicy::FromNow,
        overdue,
        RebootPolicy::CreditNonNegative,
    );
    for via_reboot in [false, true] {
        let pause = |c: &mut Clocks, d: u64| {
            if via_reboot {
                c.reboot(d)
            } else {
                c.suspend(d)
            }
        };
        // Before deadline: nothing.
        let mut c = Clocks::new(1_000);
        let mut dev = Device::new(&alice, 1, p, 10, &c);
        c.run(5);
        dev.sync(&c).unwrap();
        if via_reboot {
            dev.crash();
        }
        pause(&mut c, 3);
        assert!(dev.poll(&alice, &c, false).unwrap().is_none());
        assert_eq!((dev.durable.elapsed, dev.durable.generation), (8, 0));
        // Exactly at deadline: one successor, both policies agree on next deadline 20.
        pause(&mut c, 2);
        if via_reboot {
            dev.crash();
        }
        assert!(dev.poll(&alice, &c, false).unwrap().is_some());
        assert_eq!((dev.durable.generation, dev.durable.next_deadline), (1, 20));
        // After deadline with k missed periods: exactly one successor.
        for (k, resume, next) in overdue_cases(overdue) {
            let mut c = Clocks::new(1_000);
            let mut dev = Device::new(&alice, 1, p, 10, &c);
            c.run(5);
            dev.sync(&c).unwrap();
            if via_reboot {
                dev.crash();
            }
            pause(&mut c, resume - 5);
            let permit = dev
                .poll(&alice, &c, false)
                .unwrap()
                .expect("overdue rotation");
            assert_eq!(
                permit.claims.generation, 1,
                "k={k}: no fabricated generations"
            );
            assert_eq!(dev.durable.generation, 1);
            assert_eq!(dev.durable.elapsed, resume);
            assert_eq!(dev.durable.next_deadline, next, "k={k} reboot={via_reboot}");
            assert!(
                dev.poll(&alice, &c, false).unwrap().is_none(),
                "second poll must not rotate"
            );
        }
    }
}

// KIND: candidate-policy G-TIME
#[test]
fn overdue_resume_plus_interval_one_successor_for_sleep_and_restart() {
    check_resume_policy(OverduePolicy::ResumePlusInterval);
}

// KIND: candidate-policy G-TIME
#[test]
fn overdue_aligned_grid_one_successor_for_sleep_and_restart() {
    check_resume_policy(OverduePolicy::AlignedGrid);
}

// KIND: limitation
#[test]
fn monotonic_source_that_pauses_in_suspend_misses_overdue_rotation() {
    // If the platform clock behind `elapsed` stops during suspend (CLOCK_MONOTONIC-like),
    // the scheduler does not see a device asleep for 3.5 intervals as overdue. Receivers
    // still bound the old binding by its signed WALL validity, so the result is
    // unreachability, not indefinite extension.
    let (alice, _) = identity::generate();
    let mut c = Clocks::new(1_000);
    c.monotonic_counts_suspend = false;
    let mut dev = Device::new(&alice, 1, TEST_PARAMS, 10, &c);
    let mut rx = Receiver::new(&alice);
    c.run(10);
    let p1 = dev.poll(&alice, &c, false).unwrap().unwrap();
    rx.accept(&p1, c.wall).unwrap();
    assert!(rx.reachable(&dev, c.wall));
    c.run(2);
    dev.sync(&c).unwrap();
    c.suspend(35);
    assert!(
        dev.poll(&alice, &c, false).unwrap().is_none(),
        "not detected as overdue"
    );
    assert_eq!(dev.durable.elapsed, 12);
    assert_eq!(dev.durable.generation, 1);
    assert!(
        !rx.reachable(&dev, c.wall),
        "signed validity (15 wall units) has lapsed"
    );

    // Contrast: BOOTTIME-like source makes the same sleep overdue -> one successor.
    let mut c = Clocks::new(1_000);
    let mut dev = Device::new(&alice, 1, TEST_PARAMS, 10, &c);
    c.run(12);
    dev.poll(&alice, &c, false).unwrap().unwrap();
    c.suspend(35);
    assert_eq!(
        dev.poll(&alice, &c, false)
            .unwrap()
            .unwrap()
            .claims
            .generation,
        2
    );
}

// ---------------------------------------------------------------------------
// 4. Wall-clock rollback / forward jump
// ---------------------------------------------------------------------------

// KIND: invariant
#[test]
fn elapsed_scheduler_ignores_wall_rollback_and_jump_and_generation_never_decreases() {
    let (alice, _) = identity::generate();
    for overdue in [
        OverduePolicy::ResumePlusInterval,
        OverduePolicy::AlignedGrid,
    ] {
        let p = params(
            ChangePolicy::FromNow,
            overdue,
            RebootPolicy::CreditNonNegative,
        );
        let mut c = Clocks::new(10_000);
        let mut dev = Device::new(&alice, 1, p, 10, &c);
        let mut rotated = Vec::new();
        let mut generation = 0;
        for step in 1..=60u64 {
            match step {
                5 => c.set_wall(c.wall - 45),     // rollback by > interval
                17 => c.set_wall(c.wall + 1_000), // forward jump by >> interval
                33 => c.set_wall(c.wall - 2_000), // rollback again, past original
                _ => {}
            }
            c.run(1);
            if dev.poll(&alice, &c, false).unwrap().is_some() {
                rotated.push(step);
            }
            assert!(dev.durable.generation >= generation);
            generation = dev.durable.generation;
            // Old endpoint never outlives one interval of scheduler time.
            assert!(dev.durable.elapsed - dev.durable.last_rotation_at < 10);
        }
        assert_eq!(rotated, vec![10, 20, 30, 40, 50, 60], "{overdue:?}");
    }
}

/// The scheduler style of scripts/test_rotation_network_model.py:105 (due = now // interval)
/// and a "last + interval" variant, both driven only by wall time.
struct WallOnly {
    generation: u64,
    interval: u64,
    last_wall: u64,
}

impl WallOnly {
    fn poll_last_plus_interval(&mut self, wall: u64) -> bool {
        if wall >= self.last_wall + self.interval {
            self.generation += 1;
            self.last_wall = wall;
            return true;
        }
        false
    }
    fn poll_grid(&mut self, wall: u64) -> u64 {
        let due = wall / self.interval;
        if due > self.generation {
            self.generation = due;
        }
        self.generation
    }
}

// KIND: limitation
#[test]
fn wall_only_scheduler_extends_old_endpoint_on_rollback_and_fabricates_generations_on_jump() {
    // last + interval: a user/NTP rollback just before each deadline keeps the old
    // endpoint indefinitely (here 200 real units, 0 rotations).
    let mut w = WallOnly {
        generation: 0,
        interval: 10,
        last_wall: 1_000,
    };
    let mut wall = 1_000;
    for _ in 0..25 {
        wall += 8;
        assert!(!w.poll_last_plus_interval(wall));
        wall -= 8; // repeated rollback
    }
    assert_eq!(w.generation, 0);
    // Single rollback by 45 delays rotation by 45 extra units.
    let mut w = WallOnly {
        generation: 0,
        interval: 10,
        last_wall: 1_000,
    };
    let mut wall: u64 = 1_005 - 45;
    let mut real = 5;
    while !w.poll_last_plus_interval(wall) {
        wall += 1;
        real += 1;
    }
    assert_eq!(
        real, 55,
        "old endpoint lived 55 real units with interval 10"
    );

    // Grid: a forward jump of 50 units fabricates 5 generation numbers at once, and
    // after a rollback the grid stalls until wall time passes the old maximum.
    let mut g = WallOnly {
        generation: 0,
        interval: 10,
        last_wall: 0,
    };
    assert_eq!(g.poll_grid(10), 1);
    assert_eq!(g.poll_grid(61), 6, "generations 2..=5 never had keys");
    let mut stalled = 0;
    let mut wall = 15;
    while g.poll_grid(wall) == 6 {
        wall += 1;
        stalled += 1;
    }
    assert_eq!(stalled, 55);
}

// KIND: limitation
#[test]
fn signed_validity_uses_sender_wall_time_so_skewed_sender_becomes_unreachable() {
    // Scheduling is unaffected (see invariant above), but issued_at/expires_at are wall
    // values checked against the RECEIVER's wall clock with zero skew tolerance
    // (copied View::page). Forward-skewed sender: update rejected unchanged.
    // Backward-skewed sender (> validity): accepted as history, never live.
    let (alice, _) = identity::generate();
    let receiver_wall = |c: &Clocks, offset: i64| (c.wall as i64 - offset) as u64;

    // Forward skew +1000.
    let mut c = Clocks::new(10_000);
    let mut dev = Device::new(&alice, 1, TEST_PARAMS, 10, &c);
    let mut rx = Receiver::new(&alice);
    c.set_wall(c.wall + 1_000);
    c.run(10);
    let p = dev.poll(&alice, &c, false).unwrap().unwrap();
    let before = serde_json::to_vec(&rx.view).unwrap();
    assert_eq!(rx.accept(&p, receiver_wall(&c, 1_000)), Err("validity"));
    assert_eq!(serde_json::to_vec(&rx.view).unwrap(), before);
    assert!(!rx.reachable(&dev, receiver_wall(&c, 1_000)));
    // Small forward skew (1 unit) is also rejected: zero tolerance is a model choice.
    let mut c = Clocks::new(10_000);
    let mut dev = Device::new(&alice, 1, TEST_PARAMS, 10, &c);
    c.set_wall(c.wall + 1);
    c.run(10);
    let p = dev.poll(&alice, &c, false).unwrap().unwrap();
    assert_eq!(
        Receiver::new(&alice).accept(&p, receiver_wall(&c, 1)),
        Err("validity")
    );

    // Backward skew -40 (> validity 15).
    let mut c = Clocks::new(10_000);
    let mut dev = Device::new(&alice, 1, TEST_PARAMS, 10, &c);
    let mut rx = Receiver::new(&alice);
    c.set_wall(c.wall - 40);
    c.run(10);
    let p = dev.poll(&alice, &c, false).unwrap().unwrap();
    let rw = receiver_wall(&c, -40);
    rx.accept(&p, rw).unwrap();
    assert_eq!(rx.view.generation, 1, "history advanced");
    assert!(!rx.reachable(&dev, rw), "binding expired on arrival");
}

// ---------------------------------------------------------------------------
// 5. Reboot: monotonic reset, persisted wall checkpoint, skew clamp
// ---------------------------------------------------------------------------

fn reboot_with_wall(policy: RebootPolicy, wall_delta: i64) -> Device {
    let (alice, _) = identity::generate();
    let p = params(
        ChangePolicy::FromNow,
        OverduePolicy::ResumePlusInterval,
        policy,
    );
    let mut c = Clocks::new(10_000);
    let mut dev = Device::new(&alice, 1, p, 10, &c);
    c.run(4);
    dev.sync(&c).unwrap();
    dev.crash();
    c.reboot(0);
    c.set_wall((c.wall as i64 + wall_delta) as u64);
    c.run(1);
    dev.poll(&alice, &c, false).unwrap();
    dev
}

// KIND: candidate-policy G-TIME
#[test]
fn reboot_credit_non_negative_counts_forward_downtime_and_ignores_rollback() {
    let p = RebootPolicy::CreditNonNegative;
    // Forward downtime 3: elapsed 4 + 3 + 1 = 8, no rotation.
    let d = reboot_with_wall(p, 3);
    assert_eq!((d.durable.elapsed, d.durable.generation), (8, 0));
    // Forward downtime 37 (> 3 intervals): exactly one successor.
    let d = reboot_with_wall(p, 37);
    assert_eq!(
        (
            d.durable.elapsed,
            d.durable.generation,
            d.durable.next_deadline
        ),
        (42, 1, 52)
    );
    // Small rollback (2) and large rollback (500): credit 0, schedule continues from
    // persisted elapsed; downtime is NOT counted (old endpoint lives longer locally).
    for back in [2, 500] {
        let d = reboot_with_wall(p, -back);
        assert_eq!(
            (d.durable.elapsed, d.durable.generation),
            (5, 0),
            "rollback {back}"
        );
        assert_eq!(d.durable.wall_checkpoint, 10_004 - back as u64 + 1);
    }
}

// KIND: candidate-policy G-TIME
#[test]
fn reboot_rollback_beyond_skew_forces_immediate_rotation() {
    let p = RebootPolicy::OverdueOnRollback { skew: 5 };
    // Rollback within skew: treated as benign correction, credit 0, no rotation.
    let d = reboot_with_wall(p, -5);
    assert_eq!((d.durable.elapsed, d.durable.generation), (5, 0));
    // Rollback beyond skew: downtime unknown -> rotate now, exactly one successor.
    let d = reboot_with_wall(p, -6);
    assert_eq!(
        (
            d.durable.elapsed,
            d.durable.generation,
            d.durable.next_deadline
        ),
        (5, 1, 15)
    );
    let d = reboot_with_wall(p, -500);
    assert_eq!(d.durable.generation, 1);
    // Forward behaves like CreditNonNegative.
    let d = reboot_with_wall(p, 37);
    assert_eq!((d.durable.elapsed, d.durable.generation), (42, 1));
}

// KIND: invariant
#[test]
fn repeated_reboots_with_rollback_never_decrease_elapsed_or_generation() {
    let (alice, _) = identity::generate();
    for reboot in [
        RebootPolicy::CreditNonNegative,
        RebootPolicy::OverdueOnRollback { skew: 5 },
    ] {
        let p = params(
            ChangePolicy::FromNow,
            OverduePolicy::ResumePlusInterval,
            reboot,
        );
        let mut c = Clocks::new(10_000);
        let mut dev = Device::new(&alice, 1, p, 10, &c);
        let (mut e, mut g) = (0, 0);
        for i in 0..40u64 {
            c.run(3);
            dev.poll(&alice, &c, false).unwrap();
            dev.crash();
            c.reboot(1);
            if i % 3 == 0 {
                c.set_wall(c.wall - 50);
            }
            dev.poll(&alice, &c, false).unwrap();
            assert!(dev.durable.elapsed >= e && dev.durable.generation >= g);
            (e, g) = (dev.durable.elapsed, dev.durable.generation);
            // Uptime is always counted, so the old endpoint cannot survive more than
            // one interval of accumulated uptime regardless of wall behaviour.
            assert!(dev.durable.elapsed - dev.durable.last_rotation_at < 10 + 3);
        }
        assert!(g >= 12, "{reboot:?}: generation {g}");
    }
}

// ---------------------------------------------------------------------------
// 6. Permit validity vs rotation interval (G-RENEWAL)
// ---------------------------------------------------------------------------

// KIND: candidate-policy G-RENEWAL
#[test]
fn fail_closed_rotation_failure_beyond_validity_expires_old_binding_unreachable() {
    // Option "no renewal without rotation": if replacement cannot start, the old
    // binding simply expires at its signed validity; peers cannot reach the device.
    let (alice, _) = identity::generate();
    let mut c = Clocks::new(10_000);
    let mut dev = Device::new(&alice, 1, TEST_PARAMS, 10, &c); // validity 15
    let mut rx = Receiver::new(&alice);
    c.run(10);
    let p1 = dev.poll(&alice, &c, false).unwrap().unwrap();
    rx.accept(&p1, c.wall).unwrap();
    let issued = c.wall;
    let mut reachable = Vec::new();
    for _ in 0..20 {
        c.run(1);
        dev.sync(&c).unwrap(); // clock accounting is separate from the rotation attempt
        let before = dev.bytes();
        // Deadline 20 passes; every attempt from then on fails and commits nothing.
        let r = dev.poll(&alice, &c, true);
        if dev.durable.elapsed >= 20 {
            assert_eq!(r.err(), Some("startup_failed"));
            assert_eq!(dev.bytes(), before);
            assert_eq!((dev.durable.generation, dev.durable.next_deadline), (1, 20));
        }
        reachable.push((c.wall - issued, rx.reachable(&dev, c.wall)));
    }
    // Reachable for wall offsets 1..=14, unreachable from 15 (= validity) onwards.
    for (offset, ok) in reachable {
        assert_eq!(ok, offset < 15, "offset {offset}");
    }
    // Recovery: next successful rotation restores reachability with generation 2.
    let p2 = dev.poll(&alice, &c, false).unwrap().unwrap();
    rx.accept(&p2, c.wall).unwrap();
    assert!(rx.reachable(&dev, c.wall));
    assert_eq!(rx.view.generation, 2);
}

// KIND: candidate-policy G-RENEWAL
#[test]
fn validity_shorter_than_interval_leaves_gaps_even_when_rotation_succeeds() {
    let (alice, _) = identity::generate();
    for (validity, expected_unreachable) in [(8u64, 2 * 3), (10, 0), (15, 0)] {
        let p = Params {
            validity,
            ..TEST_PARAMS
        };
        let mut c = Clocks::new(10_000);
        let mut dev = Device::new(&alice, 1, p, 10, &c);
        let mut rx = Receiver::new(&alice);
        let mut unreachable = 0;
        for step in 1..=40u64 {
            c.run(1);
            if let Some(p) = dev.poll(&alice, &c, false).unwrap() {
                rx.accept(&p, c.wall).unwrap();
            }
            if step > 10 && !rx.reachable(&dev, c.wall) {
                unreachable += 1;
            }
        }
        assert_eq!(unreachable, expected_unreachable, "validity {validity}");
    }
}

// KIND: limitation
#[test]
fn renewing_same_endpoint_is_not_expressible_in_copied_permit_claims() {
    // A "renew without rotation" option would re-sign the current endpoint. The copied
    // claims require a changed replacement_id; keeping it is rejected unchanged. A
    // renewal format is an open G-RENEWAL decision, not invented here.
    let (alice, _) = identity::generate();
    let mut c = Clocks::new(10_000);
    let mut dev = Device::new(&alice, 1, TEST_PARAMS, 10, &c);
    let mut rx = Receiver::new(&alice);
    c.run(10);
    rx.accept(&dev.poll(&alice, &c, false).unwrap().unwrap(), c.wall)
        .unwrap();
    let v = &dev.durable.view;
    let renewal = Permit::issue(
        &alice,
        Claims {
            version: 1,
            owner: alice.did().into(),
            audience: v.audience.clone(),
            previous_id: v.current_id,
            replacement_id: v.current_id,
            endpoint: v.endpoint,
            predecessor: v.head,
            generation: v.generation + 1,
            issued_at: c.wall + 5,
            expires_at: c.wall + 5 + 15,
        },
    );
    c.run(5);
    let before = serde_json::to_vec(&rx.view).unwrap();
    assert_eq!(rx.accept(&renewal, c.wall), Err("unchanged_id"));
    assert_eq!(serde_json::to_vec(&rx.view).unwrap(), before);
}
