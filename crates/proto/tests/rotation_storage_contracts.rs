//! C2 storage boundaries and failure matrix. TEST-ONLY model.
//!
//! A sender device rotates its endpoint through named steps (`GenerateKey`, `SealCommit`,
//! `StartEndpoint`, `Announce(peer)`, `RetireOld`, `ShutdownOld`); receivers process an
//! announcement through `Verify`, `Commit`, `Ack`. A fault injector can crash before/after,
//! or fail, any step. State is split into explicit durable and volatile parts.
//!
//! Model boundaries (NOT modelled): fsync, power loss, torn writes, redb transactions,
//! vault sealing, Android process death/Keystore, OS clocks, iroh. A "durable write" is
//! an ideal atomic struct assignment (`Sender::write`); a crash drops every volatile field
//! and stops every endpoint of the device. G-STORAGE and G-RETIREMENT are unresolved:
//! commit orderings and retirement policies are modelled side by side.
//!
//! Fake time: one `u64` tick counter used both as scheduler time and signed wall time
//! (clock divergence is C1's subject, `rotation_time_contracts.rs`). Owner signatures and
//! announcement possession proofs are real Ed25519. Every numeric value is a fixture.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use ed25519_dalek::{Signer, SigningKey};
use identity::Identity;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[path = "support/permit_base.rs"]
mod permit_base;
use permit_base::*;

/// Byte-level comparison of durable state needs permits to serialize; the copied fixture
/// has a `[u8; 64]` signature, so serialize it as a byte vector here (copy left untouched).
impl Serialize for Permit {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        (&self.claims, self.signature.to_vec()).serialize(s)
    }
}

// Copied from rotation_time_contracts.rs @f3cf3ed (`fixture_key`), domain renamed and
// indexed by a global key-generation counter so a regenerated key always differs.
/// Deterministic fixture endpoint key. NOT a production scheme and NOT a derivation
/// proposal (G-DISCOVERY is unselected): it only makes test runs reproducible.
fn fixture_key(counter: u32) -> SigningKey {
    let mut h = Sha256::new();
    h.update(b"test-only/c2-fixture-endpoint\0");
    h.update(counter.to_be_bytes());
    SigningKey::from_bytes(&h.finalize().into())
}

type Ep = [u8; 32];

/// Test-only fixture values, NOT product defaults.
const INTERVAL: u64 = 10;
const VALIDITY: u64 = 15;
const DEVICE: u8 = 1;
const PEERS: [&str; 3] = ["bob", "carol", "dave"];
const BLOCKED: &str = "carol";

fn successor(owner: &Identity, view: &View, key: &SigningKey, now: u64) -> Permit {
    let endpoint = key.verifying_key().to_bytes();
    // Fixture replacement ID bound to the key so a regenerated key gets a new ID.
    let replacement_id = Sha256::digest([b"test-only/c2-id\0".as_slice(), &endpoint].concat());
    Permit::issue(
        owner,
        Claims {
            version: 1,
            owner: owner.did().into(),
            audience: view.audience.clone(),
            previous_id: view.current_id,
            replacement_id: replacement_id.into(),
            endpoint,
            predecessor: view.head,
            generation: view.generation + 1,
            issued_at: now,
            expires_at: now + VALIDITY,
        },
    )
}

/// Possession proof for an announcement, bound to the recipient and the head permit.
/// A fixture only; challenge/session design is packet C3.
fn challenge(peer: &str, head: &[u8; 32]) -> Vec<u8> {
    [
        b"test-only/c2-announce\0".as_slice(),
        peer.as_bytes(),
        b"\0",
        head,
    ]
    .concat()
}

// ---------------------------------------------------------------------------
// Steps, faults, policies
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    GenerateKey,
    SealCommit,
    StartEndpoint,
    Announce(&'static str),
    RetireOld,
    ShutdownOld,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RStep {
    Verify,
    Commit,
    Ack,
}

/// Fault injected into one run. `Fail` = the step returns an error and the process keeps
/// running; `Crash*` = the process dies (volatile state and endpoints lost).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault<S> {
    None,
    CrashBefore(S),
    CrashAfter(S),
    Fail(S),
}

#[derive(Debug, PartialEq, Eq)]
enum Stop {
    Crashed,
    Failed(Step),
}

#[derive(Debug, PartialEq, Eq)]
enum RErr {
    Crashed,
    Failed(RStep),
    Rejected(&'static str),
}

/// Commit ordering (G-STORAGE option).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum Order {
    /// One atomic write adopts the successor as current and moves the old key into
    /// `retiring`, BEFORE the new endpoint starts or anything is announced.
    CommitFirst,
    /// Two-phase: persist `pending {permit, secret}` before start/announce; promote it to
    /// current in the same write that retires the old key.
    PendingThenPromote,
    /// Rejected ordering kept as a counterexample: start + announce before any commit.
    AnnounceBeforeCommit,
}

/// What happens when the replacement cannot be brought up (G-RETIREMENT option).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum StartFailure {
    /// Stop the old endpoint at the rotation deadline; device unreachable until retry works.
    FailClosed,
    /// Keep the old endpoint serving, visibly degraded, until
    /// min(deadline + grace, old signed expires_at); then stop it.
    DegradedGrace { grace: u64 },
}

/// Existing sessions on the old endpoint at retirement (G-RETIREMENT option).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum Overlap {
    Terminate,
    /// Old endpoint refuses new sessions but keeps existing ones until
    /// min(retire + max, old signed expires_at).
    Bounded {
        max: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct Policy {
    order: Order,
    start_failure: StartFailure,
    overlap: Overlap,
}

const BASE: Policy = Policy {
    order: Order::CommitFirst,
    start_failure: StartFailure::FailClosed,
    overlap: Overlap::Terminate,
};

fn policy(order: Order) -> Policy {
    Policy { order, ..BASE }
}

// ---------------------------------------------------------------------------
// World: everything observable outside the device (the test oracle)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Ev {
    Start(Ep, u64),
    Stop(Ep, u64, &'static str),
    Retired(Ep, u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Session {
    peer: &'static str,
    ep: Ep,
    opened: u64,
    closed: Option<(u64, &'static str)>,
}

#[derive(Clone)]
struct Msg {
    permits: Vec<Permit>,
    proof: [u8; 64],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AckOrder {
    AfterCommit,
    /// Rejected ordering kept as a counterexample.
    BeforeCommit,
}

/// Receiver: durable = `view` only. Inbox models the network (survives a receiver crash
/// except the message being processed); the verify candidate is volatile.
struct Receiver {
    name: &'static str,
    view: View,
    inbox: VecDeque<Msg>,
    received: usize,
    ack_order: AckOrder,
}

impl Receiver {
    fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&self.view).unwrap()
    }

    fn gate(&self, fault: Fault<RStep>, step: RStep) -> Result<(), RErr> {
        match fault {
            Fault::CrashBefore(s) if s == step => Err(RErr::Crashed),
            Fault::Fail(s) if s == step => Err(RErr::Failed(step)),
            _ => Ok(()),
        }
    }

    fn after(&self, fault: Fault<RStep>, step: RStep) -> Result<(), RErr> {
        if fault == Fault::CrashAfter(step) {
            return Err(RErr::Crashed);
        }
        Ok(())
    }

    fn commit(&mut self, candidate: View, fault: Fault<RStep>) -> Result<(), RErr> {
        self.gate(fault, RStep::Commit)?;
        self.view = candidate; // ideal atomic durable write
        self.after(fault, RStep::Commit)
    }

    fn ack(
        &mut self,
        generation: u64,
        acks: &mut Vec<(&'static str, u64)>,
        fault: Fault<RStep>,
    ) -> Result<(), RErr> {
        self.gate(fault, RStep::Ack)?;
        acks.push((self.name, generation));
        self.after(fault, RStep::Ack)
    }

    fn deliver(
        &mut self,
        now: u64,
        acks: &mut Vec<(&'static str, u64)>,
        fault: Fault<RStep>,
    ) -> Result<Option<u64>, RErr> {
        let Some(msg) = self.inbox.pop_front() else {
            return Ok(None);
        };
        self.received += 1;
        self.gate(fault, RStep::Verify)?;
        let mut candidate = self.view.clone();
        let newest = msg.permits.last().ok_or(RErr::Rejected("empty"))?;
        // Fixture: drop the already-verified prefix (B1 owns paging/cursors).
        let fresh: Vec<Permit> = msg
            .permits
            .iter()
            .filter(|p| p.claims.generation >= candidate.generation)
            .cloned()
            .collect();
        if fresh.is_empty() {
            return Err(RErr::Rejected("stale"));
        }
        candidate
            .page(&fresh, now, &no_revocations())
            .map_err(RErr::Rejected)?;
        if !identity::verify(
            &newest.claims.endpoint,
            &challenge(self.name, &newest.id()),
            &msg.proof,
        ) {
            return Err(RErr::Rejected("possession"));
        }
        self.after(fault, RStep::Verify)?;
        let generation = candidate.generation;
        match self.ack_order {
            AckOrder::AfterCommit => {
                self.commit(candidate, fault)?;
                self.ack(generation, acks, fault)?;
            }
            AckOrder::BeforeCommit => {
                self.ack(generation, acks, fault)?;
                self.commit(candidate, fault)?;
            }
        }
        Ok(Some(generation))
    }
}

struct World {
    now: u64,
    online: BTreeMap<Ep, u64>,
    draining: BTreeMap<Ep, u64>,
    log: Vec<Ev>,
    /// generation -> every endpoint key that left the device for it (started endpoint or
    /// announced signed permit). The "no generation reuse" oracle.
    escaped: BTreeMap<u64, BTreeSet<Ep>>,
    keygens: u32,
    writes: u32,
    acks: Vec<(&'static str, u64)>,
    ack_log: Vec<(&'static str, u64)>,
    receivers: BTreeMap<&'static str, Receiver>,
    sessions: Vec<Session>,
}

impl World {
    fn new(owner: &Identity) -> Self {
        let receivers = PEERS
            .iter()
            .map(|&name| {
                let r = Receiver {
                    name,
                    view: View::initial(owner, DEVICE),
                    inbox: VecDeque::new(),
                    received: 0,
                    ack_order: AckOrder::AfterCommit,
                };
                (name, r)
            })
            .collect();
        Self {
            now: 0,
            online: BTreeMap::new(),
            draining: BTreeMap::new(),
            log: Vec::new(),
            escaped: BTreeMap::new(),
            keygens: 0,
            writes: 0,
            acks: Vec::new(),
            ack_log: Vec::new(),
            receivers,
            sessions: Vec::new(),
        }
    }

    fn keygen(&mut self) -> SigningKey {
        let key = fixture_key(self.keygens);
        self.keygens += 1;
        key
    }

    fn start(&mut self, ep: Ep, generation: u64) {
        assert!(!self.online.contains_key(&ep), "double start");
        self.online.insert(ep, self.now);
        self.log.push(Ev::Start(ep, self.now));
        self.escaped.entry(generation).or_default().insert(ep);
    }

    fn stop_at(&mut self, ep: Ep, at: u64, reason: &'static str) {
        if self.online.remove(&ep).is_none() {
            return;
        }
        self.draining.remove(&ep);
        self.log.push(Ev::Stop(ep, at, reason));
        for s in &mut self.sessions {
            if s.ep == ep && s.closed.is_none() {
                s.closed = Some((at, reason));
            }
        }
    }

    fn stop(&mut self, ep: Ep, reason: &'static str) {
        self.stop_at(ep, self.now, reason);
    }

    /// The device process died: every endpoint and session of it is gone.
    fn crash_device(&mut self) {
        for ep in self.online.keys().copied().collect::<Vec<_>>() {
            self.stop(ep, "crash");
        }
    }

    fn advance(&mut self, now: u64) {
        assert!(now >= self.now);
        self.now = now;
        let due: Vec<(Ep, u64)> = self
            .draining
            .iter()
            .filter(|(_, until)| **until <= now)
            .map(|(e, u)| (*e, *u))
            .collect();
        for (ep, until) in due {
            self.stop_at(ep, until, "overlap_cap");
        }
    }

    fn open(&mut self, peer: &'static str, ep: Ep) -> Result<usize, &'static str> {
        if !self.online.contains_key(&ep) {
            return Err("offline");
        }
        if self.draining.contains_key(&ep) {
            return Err("draining");
        }
        self.sessions.push(Session {
            peer,
            ep,
            opened: self.now,
            closed: None,
        });
        Ok(self.sessions.len() - 1)
    }

    fn end(&mut self, id: usize) {
        assert!(self.sessions[id].closed.is_none());
        self.sessions[id].closed = Some((self.now, "ended"));
    }

    fn deliver_one(&mut self, name: &str, fault: Fault<RStep>) -> Result<Option<u64>, RErr> {
        let now = self.now;
        let mut acks = Vec::new();
        let r = self
            .receivers
            .get_mut(name)
            .unwrap()
            .deliver(now, &mut acks, fault);
        self.acks.extend(acks.iter().copied());
        self.ack_log.extend(acks);
        r
    }

    /// Deliver every queued message without faults; returns rejections.
    fn deliver_all(&mut self) -> Vec<(&'static str, RErr)> {
        let mut rejected = Vec::new();
        for name in PEERS {
            while !self.receivers[name].inbox.is_empty() {
                if let Err(e) = self.deliver_one(name, Fault::None) {
                    rejected.push((name, e));
                }
            }
        }
        rejected
    }

    fn inboxes_empty(&self) -> bool {
        self.receivers.values().all(|r| r.inbox.is_empty())
    }

    fn receiver_bytes(&self) -> Vec<Vec<u8>> {
        self.receivers.values().map(Receiver::bytes).collect()
    }
}

// ---------------------------------------------------------------------------
// Sender device
// ---------------------------------------------------------------------------

/// Schedule fields copied (subset) from `Persisted` in rotation_time_contracts.rs @f3cf3ed.
/// C1's `elapsed` is collapsed into the single fake tick here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Schedule {
    generation: u64,
    interval: u64,
    last_rotation_at: u64,
    next_deadline: u64,
}

#[derive(Clone, Serialize)]
struct Pending {
    permit: Permit,
    secret: [u8; 32],
    due_at: u64,
}

#[derive(Clone, Serialize)]
struct Retiring {
    endpoint: Ep,
    secret: [u8; 32],
    due_at: u64,
    expires_at: u64,
}

/// Everything that survives a crash. Block policy lives here only to show it is
/// honoured across recovery; it never affects the schedule.
#[derive(Clone, Serialize)]
struct SenderDurable {
    policy: Policy,
    schedule: Schedule,
    view: View,
    secret: [u8; 32],
    history: Vec<Permit>,
    pending: Option<Pending>,
    retiring: Option<Retiring>,
    retired: BTreeSet<Ep>,
    acked: BTreeMap<&'static str, u64>,
    contacts: BTreeSet<&'static str>,
    blocked: BTreeSet<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Healthy,
    Degraded { until: u64 },
    Unreachable,
}

#[derive(Clone, Copy, Debug)]
struct Old {
    ep: Ep,
    due_at: u64,
    expires_at: u64,
}

struct Sender {
    d: SenderDurable,
    // ---- volatile ----
    draft: Option<(Permit, [u8; 32])>,
    status: Status,
    old: Option<Old>,
    sent: BTreeSet<(&'static str, u64)>,
}

fn adopt(d: &mut SenderDurable, permit: Permit, secret: [u8; 32]) {
    let at = permit.claims.issued_at;
    d.view
        .page(std::slice::from_ref(&permit), at, &no_revocations())
        .expect("own successor verifies");
    d.secret = secret;
    d.schedule.generation = permit.claims.generation;
    d.schedule.last_rotation_at = at;
    // C1 candidate ResumePlusInterval with resume = rotation time (persisted in permit).
    d.schedule.next_deadline = at + d.schedule.interval;
    d.history.push(permit);
}

impl Sender {
    fn new(owner: &Identity, policy: Policy, w: &mut World) -> Self {
        let key = w.keygen();
        let mut view = View::initial(owner, DEVICE);
        view.endpoint = key.verifying_key().to_bytes();
        Self {
            d: SenderDurable {
                policy,
                schedule: Schedule {
                    generation: 0,
                    interval: INTERVAL,
                    last_rotation_at: 0,
                    next_deadline: INTERVAL,
                },
                view,
                secret: key.to_bytes(),
                history: Vec::new(),
                pending: None,
                retiring: None,
                retired: BTreeSet::new(),
                acked: PEERS.iter().map(|p| (*p, 0)).collect(),
                contacts: PEERS.into_iter().collect(),
                blocked: [BLOCKED].into_iter().collect(),
            },
            draft: None,
            status: Status::Healthy,
            old: None,
            sent: BTreeSet::new(),
        }
    }

    fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&self.d).unwrap()
    }

    /// Ideal atomic durable write.
    fn write(&mut self, w: &mut World, f: impl FnOnce(&mut SenderDurable)) {
        let mut d = self.d.clone();
        f(&mut d);
        self.d = d;
        w.writes += 1;
    }

    fn crash(&mut self, w: &mut World) {
        self.draft = None;
        self.status = Status::Healthy;
        self.old = None;
        self.sent.clear();
        w.crash_device();
    }

    fn gate(&mut self, w: &mut World, fault: Fault<Step>, step: Step) -> Result<(), Stop> {
        match fault {
            Fault::CrashBefore(s) if s == step => {
                self.crash(w);
                Err(Stop::Crashed)
            }
            Fault::Fail(s) if s == step => Err(Stop::Failed(step)),
            _ => Ok(()),
        }
    }

    fn after(&mut self, w: &mut World, fault: Fault<Step>, step: Step) -> Result<(), Stop> {
        if fault == Fault::CrashAfter(step) {
            self.crash(w);
            return Err(Stop::Crashed);
        }
        Ok(())
    }

    /// The endpoint that the in-flight (or due) rotation replaces.
    fn old_info(&self) -> Old {
        let d = &self.d;
        match (&d.retiring, &d.pending) {
            (Some(r), _) => Old {
                ep: r.endpoint,
                due_at: r.due_at,
                expires_at: r.expires_at,
            },
            (None, Some(p)) => Old {
                ep: d.view.endpoint,
                due_at: p.due_at,
                expires_at: d.view.expires_at,
            },
            (None, None) => Old {
                ep: d.view.endpoint,
                due_at: d.schedule.next_deadline,
                expires_at: d.view.expires_at,
            },
        }
    }

    /// One scheduler wake-up (also the restart path: recovery = tick with empty volatile).
    fn tick(&mut self, w: &mut World, owner: &Identity, fault: Fault<Step>) -> Result<(), Stop> {
        self.sent.clear();
        let r = self.run(w, owner, fault);
        if let Err(Stop::Failed(step)) = r {
            self.on_failure(w, step);
        }
        if r != Err(Stop::Crashed) {
            if let Status::Degraded { until } = self.status {
                if w.now >= until {
                    w.stop(self.old_info().ep, "grace_expired");
                    self.status = Status::Unreachable;
                }
            }
        }
        r
    }

    fn run(&mut self, w: &mut World, owner: &Identity, fault: Fault<Step>) -> Result<(), Stop> {
        // 1. Finish durable in-flight work first; never start an old key before that.
        if self.d.pending.is_some() || self.d.retiring.is_some() {
            self.finish(w, fault)?;
        }
        // 2. Overdue: rotate BEFORE bringing the overdue current key online.
        if w.now >= self.d.schedule.next_deadline {
            self.rotate(w, owner, fault)?;
        }
        // 3. Serve the committed current key; retry announcements from durable originals.
        let current = self.d.view.endpoint;
        if !w.online.contains_key(&current) {
            self.gate(w, fault, Step::StartEndpoint)?;
            w.start(current, self.d.view.generation);
            self.after(w, fault, Step::StartEndpoint)?;
        }
        self.announce(w, fault)?;
        self.status = Status::Healthy;
        Ok(())
    }

    fn rotate(&mut self, w: &mut World, owner: &Identity, fault: Fault<Step>) -> Result<(), Stop> {
        self.gate(w, fault, Step::GenerateKey)?;
        let key = w.keygen();
        let permit = successor(owner, &self.d.view, &key, w.now);
        self.draft = Some((permit, key.to_bytes()));
        self.after(w, fault, Step::GenerateKey)?;
        if self.d.policy.order == Order::AnnounceBeforeCommit {
            let (ep, generation) = {
                let p = &self.draft.as_ref().unwrap().0;
                (p.claims.endpoint, p.claims.generation)
            };
            self.gate(w, fault, Step::StartEndpoint)?;
            w.start(ep, generation);
            self.after(w, fault, Step::StartEndpoint)?;
            self.announce(w, fault)?;
        }
        self.seal(w, fault)?;
        self.finish(w, fault)
    }

    fn seal(&mut self, w: &mut World, fault: Fault<Step>) -> Result<(), Stop> {
        self.gate(w, fault, Step::SealCommit)?;
        let (permit, secret) = self.draft.clone().unwrap();
        let order = self.d.policy.order;
        self.write(w, |d| {
            if order == Order::PendingThenPromote {
                d.pending = Some(Pending {
                    permit,
                    secret,
                    due_at: d.schedule.next_deadline,
                });
            } else {
                d.retiring = Some(Retiring {
                    endpoint: d.view.endpoint,
                    secret: d.secret,
                    due_at: d.schedule.next_deadline,
                    expires_at: d.view.expires_at,
                });
                adopt(d, permit, secret);
            }
        });
        self.draft = None;
        self.after(w, fault, Step::SealCommit)
    }

    fn finish(&mut self, w: &mut World, fault: Fault<Step>) -> Result<(), Stop> {
        let old = self.old_info();
        self.old = Some(old);
        let (new_ep, new_gen) = match &self.d.pending {
            Some(p) => (p.permit.claims.endpoint, p.permit.claims.generation),
            None => (self.d.view.endpoint, self.d.view.generation),
        };
        if !w.online.contains_key(&new_ep) {
            self.gate(w, fault, Step::StartEndpoint)?;
            w.start(new_ep, new_gen);
            self.after(w, fault, Step::StartEndpoint)?;
        }
        self.announce(w, fault)?;
        self.gate(w, fault, Step::RetireOld)?;
        self.write(w, |d| {
            if let Some(p) = d.pending.take() {
                adopt(d, p.permit, p.secret);
            }
            d.retiring = None;
            d.retired.insert(old.ep);
        });
        w.log.push(Ev::Retired(old.ep, w.now));
        self.after(w, fault, Step::RetireOld)?;
        self.gate(w, fault, Step::ShutdownOld)?;
        match self.d.policy.overlap {
            Overlap::Terminate => w.stop(old.ep, "retired"),
            Overlap::Bounded { max } => {
                let until = (w.now + max).min(old.expires_at);
                if until <= w.now {
                    w.stop(old.ep, "retired");
                } else if w.online.contains_key(&old.ep) {
                    w.draining.insert(old.ep, until);
                }
            }
        }
        self.after(w, fault, Step::ShutdownOld)?;
        self.old = None;
        Ok(())
    }

    fn on_failure(&mut self, w: &mut World, step: Step) {
        match step {
            Step::GenerateKey | Step::SealCommit | Step::StartEndpoint => {
                // Uncommitted draft never left the device: discarding it is safe.
                self.draft = None;
                let old = self.old_info();
                match self.d.policy.start_failure {
                    StartFailure::FailClosed => {
                        w.stop(old.ep, "fail_closed");
                        self.status = Status::Unreachable;
                    }
                    StartFailure::DegradedGrace { grace } => {
                        // Window derived from DURABLE due_at/expiry: a restart cannot extend it.
                        let until = (old.due_at + grace).min(old.expires_at);
                        if w.now < until && w.online.contains_key(&old.ep) {
                            self.status = Status::Degraded { until };
                        } else {
                            w.stop(old.ep, "grace_expired");
                            self.status = Status::Unreachable;
                        }
                    }
                }
            }
            // New endpoint is serving; the old one must not, even though its durable
            // retirement record could not be written (retried next tick).
            Step::RetireOld => w.stop(self.old.unwrap().ep, "retire_failed"),
            // Close error: escalate to a forced transport stop (assumed to succeed).
            Step::ShutdownOld => w.stop(self.old.unwrap().ep, "forced_shutdown"),
            Step::Announce(_) => unreachable!("announce failures are per-peer and retried"),
        }
    }

    /// Chain to announce and the secret proving possession of its head.
    fn chain(&self) -> (Vec<Permit>, [u8; 32]) {
        let mut chain = self.d.history.clone();
        if let Some(p) = &self.d.pending {
            chain.push(p.permit.clone());
            return (chain, p.secret);
        }
        if self.d.policy.order == Order::AnnounceBeforeCommit {
            if let Some((p, s)) = &self.draft {
                chain.push(p.clone());
                return (chain, *s);
            }
        }
        (chain, self.d.secret)
    }

    fn announce(&mut self, w: &mut World, fault: Fault<Step>) -> Result<(), Stop> {
        let (chain, secret) = self.chain();
        let Some(head) = chain.last().cloned() else {
            return Ok(());
        };
        let generation = head.claims.generation;
        let key = SigningKey::from_bytes(&secret);
        let peers: Vec<&'static str> = self
            .d
            .contacts
            .iter()
            .copied()
            .filter(|p| !self.d.blocked.contains(p))
            .collect();
        for peer in peers {
            let acked = self.d.acked[peer];
            if acked >= generation || self.sent.contains(&(peer, generation)) {
                continue;
            }
            let step = Step::Announce(peer);
            if fault == Fault::Fail(step) {
                continue; // send error: peer stays unacked, retried from durable originals
            }
            self.gate(w, fault, step)?;
            let permits: Vec<Permit> = chain
                .iter()
                .filter(|p| p.claims.generation > acked)
                .cloned()
                .collect();
            assert!(permits.len() <= PAGE);
            for p in &permits {
                w.escaped
                    .entry(p.claims.generation)
                    .or_default()
                    .insert(p.claims.endpoint);
            }
            let proof = key.sign(&challenge(peer, &head.id())).to_bytes();
            w.receivers
                .get_mut(peer)
                .unwrap()
                .inbox
                .push_back(Msg { permits, proof });
            self.sent.insert((peer, generation));
            self.after(w, fault, step)?;
        }
        Ok(())
    }

    fn process_acks(&mut self, w: &mut World) {
        for (peer, generation) in std::mem::take(&mut w.acks) {
            if self.d.acked[peer] < generation {
                self.write(w, |d| {
                    d.acked.insert(peer, generation);
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Harness helpers and oracles
// ---------------------------------------------------------------------------

fn settle(w: &mut World, s: &mut Sender) {
    let rejected = w.deliver_all();
    assert!(rejected.is_empty(), "unexpected rejections {rejected:?}");
    s.process_acks(w);
}

/// Sender at generation 1 (rotated at t=10, expires 25), contacts acked, key0 retired.
fn setup(owner: &Identity, policy: Policy) -> (World, Sender) {
    let mut w = World::new(owner);
    let mut s = Sender::new(owner, policy, &mut w);
    s.tick(&mut w, owner, Fault::None).unwrap();
    w.advance(10);
    s.tick(&mut w, owner, Fault::None).unwrap();
    settle(&mut w, &mut s);
    assert_eq!(s.d.view.generation, 1);
    assert_eq!(s.d.view.expires_at, 10 + VALIDITY);
    assert_eq!(s.d.acked["bob"], 1);
    assert_eq!(s.d.acked["dave"], 1);
    (w, s)
}

fn pubkey(counter: u32) -> Ep {
    fixture_key(counter).verifying_key().to_bytes()
}

/// Invariants every acceptable storage policy must keep, checked on the external log.
fn assert_invariants(w: &World, s: &Sender, ctx: &str) {
    // I1: no generation used for two different keys.
    for (generation, eps) in &w.escaped {
        assert_eq!(
            eps.len(),
            1,
            "{ctx}: generation {generation} escaped with {eps:?}"
        );
    }
    for p in &s.d.history {
        if let Some(eps) = w.escaped.get(&p.claims.generation) {
            assert!(
                eps.contains(&p.claims.endpoint),
                "{ctx}: durable/escaped disagree"
            );
        }
    }
    // I2: retired never revived (no Start after its Retired event).
    for (i, ev) in w.log.iter().enumerate() {
        if let Ev::Retired(ep, _) = ev {
            assert!(
                s.d.retired.contains(ep),
                "{ctx}: retirement lost from durable state"
            );
            let revived = w.log[i..]
                .iter()
                .any(|e| matches!(e, Ev::Start(x, _) if x == ep));
            assert!(!revived, "{ctx}: retired endpoint restarted");
        }
    }
    for ep in &s.d.retired {
        assert!(
            !w.online.contains_key(ep) || w.draining.contains_key(ep),
            "{ctx}: retired endpoint serving new sessions"
        );
    }
    // I3: ack only after commit.
    for (peer, generation) in &w.ack_log {
        assert!(
            w.receivers[peer].view.generation >= *generation,
            "{ctx}: {peer} acked {generation} without durable commit"
        );
    }
    // I4: blocked contact never receives an update (policy store survives recovery).
    assert_eq!(
        w.receivers[BLOCKED].received, 0,
        "{ctx}: blocked contact got update"
    );
    assert_eq!(w.receivers[BLOCKED].view.generation, 0, "{ctx}");
    // Durable secret matches the committed current endpoint.
    assert_eq!(
        SigningKey::from_bytes(&s.d.secret)
            .verifying_key()
            .to_bytes(),
        s.d.view.endpoint,
        "{ctx}"
    );
}

/// I5: the old endpoint served only before `limit` and is offline now.
fn assert_served_only_before(w: &World, ep: Ep, limit: u64, ctx: &str) {
    for ev in &w.log {
        match ev {
            Ev::Start(e, t) if *e == ep => assert!(*t < limit, "{ctx}: started at {t}"),
            Ev::Stop(e, t, _) if *e == ep => assert!(*t <= limit, "{ctx}: stopped at {t}"),
            _ => {}
        }
    }
    assert!(!w.online.contains_key(&ep), "{ctx}: still online");
}

fn assert_receivers_at_head(w: &World, s: &Sender, ctx: &str) {
    for peer in ["bob", "dave"] {
        assert_eq!(w.receivers[peer].view.head, s.d.view.head, "{ctx}: {peer}");
        assert_eq!(s.d.acked[peer], s.d.view.generation, "{ctx}: {peer} ack");
    }
}

const SENDER_STEPS: [Step; 7] = [
    Step::GenerateKey,
    Step::SealCommit,
    Step::StartEndpoint,
    Step::Announce("bob"),
    Step::Announce("dave"),
    Step::RetireOld,
    Step::ShutdownOld,
];

/// Fault-free control: rotate at `at`, after discarding `skip` fixture keys.
fn control(owner: &Identity, order: Order, at: u64, skip: u32) -> Sender {
    let (mut w, mut s) = setup(owner, policy(order));
    w.advance(at);
    w.keygens += skip;
    s.tick(&mut w, owner, Fault::None).unwrap();
    settle(&mut w, &mut s);
    s.tick(&mut w, owner, Fault::None).unwrap();
    settle(&mut w, &mut s);
    s
}

// ---------------------------------------------------------------------------
// 1. Crash at every sender step
// ---------------------------------------------------------------------------

// KIND: invariant
#[test]
fn crash_before_or_after_every_sender_step_recovers_without_reuse_revival_or_extension() {
    let (alice, _) = identity::generate();
    for order in [Order::CommitFirst, Order::PendingThenPromote] {
        for step in SENDER_STEPS {
            for fault in [Fault::CrashBefore(step), Fault::CrashAfter(step)] {
                let ctx = format!("{order:?} {fault:?}");
                let (mut w, mut s) = setup(&alice, policy(order));
                let k1 = s.d.view.endpoint;
                w.advance(20);
                assert_eq!(s.tick(&mut w, &alice, fault), Err(Stop::Crashed), "{ctx}");
                assert!(w.online.is_empty(), "{ctx}");
                let rejected = w.deliver_all(); // whatever escaped before the crash
                assert!(rejected.is_empty(), "{ctx}: {rejected:?}");
                w.advance(23);
                s.tick(&mut w, &alice, Fault::None).unwrap(); // restart = recovery
                settle(&mut w, &mut s);
                s.tick(&mut w, &alice, Fault::None).unwrap();
                settle(&mut w, &mut s);

                assert_invariants(&w, &s, &ctx);
                assert_served_only_before(&w, k1, 20, &ctx);
                assert_eq!(s.d.view.generation, 2, "{ctx}: exactly one successor");
                assert!(s.d.pending.is_none() && s.d.retiring.is_none(), "{ctx}");
                assert!(s.d.retired.contains(&k1), "{ctx}");
                assert_eq!(
                    w.online.keys().copied().collect::<Vec<_>>(),
                    vec![s.d.view.endpoint]
                );
                assert_receivers_at_head(&w, &s, &ctx);

                // Before the durable commit nothing escaped, so a new key is allowed.
                let pre_commit = matches!(
                    fault,
                    Fault::CrashBefore(Step::GenerateKey)
                        | Fault::CrashAfter(Step::GenerateKey)
                        | Fault::CrashBefore(Step::SealCommit)
                );
                let discarded = u32::from(
                    fault == Fault::CrashAfter(Step::GenerateKey)
                        || fault == Fault::CrashBefore(Step::SealCommit),
                );
                assert_eq!(w.keygens, 3 + discarded, "{ctx}");
                if !pre_commit {
                    assert_eq!(s.d.view.endpoint, pubkey(2), "{ctx}: committed key resumed");
                }
                // Whole durable state equals a fault-free control run.
                let at = if pre_commit { 23 } else { 20 };
                let c = control(&alice, order, at, discarded);
                assert_eq!(
                    s.bytes(),
                    c.bytes(),
                    "{ctx}: durable state differs from control"
                );
            }
        }
    }
}

// KIND: limitation
#[test]
fn announce_before_commit_ordering_reuses_a_generation_for_two_keys_after_crash() {
    // Rejected ordering, kept to show why the commit must precede start/announce.
    let (alice, _) = identity::generate();
    let (mut w, mut s) = setup(&alice, policy(Order::AnnounceBeforeCommit));
    w.advance(20);
    let crash = Fault::CrashAfter(Step::Announce("bob"));
    assert_eq!(s.tick(&mut w, &alice, crash), Err(Stop::Crashed));
    assert_eq!(s.d.view.generation, 1, "nothing durable about generation 2");
    w.advance(23);
    s.tick(&mut w, &alice, Fault::None).unwrap();
    let rejected = w.deliver_all();
    s.process_acks(&mut w);
    assert_eq!(w.escaped[&2].len(), 2, "generation 2 used for two keys");
    assert_eq!(rejected, vec![("bob", RErr::Rejected("fork"))]);
    // Split view: bob holds the pre-crash key, dave the post-crash key.
    let (bob, dave) = (&w.receivers["bob"].view, &w.receivers["dave"].view);
    assert_eq!((bob.generation, dave.generation), (2, 2));
    assert_ne!(bob.endpoint, dave.endpoint);
    assert_eq!(dave.head, s.d.view.head);
    assert_ne!(bob.head, s.d.view.head);
    // The sender believes bob is current (bob acked generation 2 of the lost key).
    assert_eq!(s.d.acked["bob"], 2);
    assert!(!w.online.contains_key(&bob.endpoint));
}

// ---------------------------------------------------------------------------
// 2. Step failures (process keeps running)
// ---------------------------------------------------------------------------

// KIND: invariant
#[test]
fn failures_before_the_durable_commit_change_nothing_and_leak_nothing() {
    let (alice, _) = identity::generate();
    for order in [Order::CommitFirst, Order::PendingThenPromote] {
        for step in [Step::GenerateKey, Step::SealCommit] {
            let ctx = format!("{order:?} {step:?}");
            let (mut w, mut s) = setup(&alice, policy(order));
            w.advance(20);
            let (before, escaped, receivers, writes, keygens) = (
                s.bytes(),
                w.escaped.clone(),
                w.receiver_bytes(),
                w.writes,
                w.keygens,
            );
            assert_eq!(
                s.tick(&mut w, &alice, Fault::Fail(step)),
                Err(Stop::Failed(step))
            );
            assert_eq!(s.bytes(), before, "{ctx}: durable state changed");
            assert_eq!(w.escaped, escaped, "{ctx}: draft escaped");
            assert_eq!(w.receiver_bytes(), receivers, "{ctx}");
            assert!(w.inboxes_empty(), "{ctx}");
            assert_eq!(w.writes, writes, "{ctx}");
            let discarded = u32::from(step == Step::SealCommit);
            assert_eq!(w.keygens, keygens + discarded, "{ctx}");
            assert!(s.draft.is_none(), "{ctx}: draft retained after failure");

            w.advance(21);
            s.tick(&mut w, &alice, Fault::None).unwrap();
            settle(&mut w, &mut s);
            assert_eq!(s.d.view.generation, 2, "{ctx}");
            assert_eq!(s.d.view.endpoint, pubkey(keygens + discarded), "{ctx}");
            assert_invariants(&w, &s, &ctx);
            assert_receivers_at_head(&w, &s, &ctx);
        }
    }
}

// KIND: candidate-policy G-RETIREMENT
#[test]
fn start_endpoint_failure_fail_closed_stops_old_at_deadline_and_retries_the_same_key() {
    let (alice, _) = identity::generate();
    for order in [Order::CommitFirst, Order::PendingThenPromote] {
        let ctx = format!("{order:?}");
        let (mut w, mut s) = setup(&alice, policy(order));
        let k1 = s.d.view.endpoint;
        w.advance(20);
        let fail = Fault::Fail(Step::StartEndpoint);
        assert_eq!(
            s.tick(&mut w, &alice, fail),
            Err(Stop::Failed(Step::StartEndpoint))
        );
        assert_eq!(s.status, Status::Unreachable);
        assert!(w.online.is_empty(), "{ctx}: old must stop at the deadline");
        let committed = match order {
            Order::PendingThenPromote => s.d.pending.as_ref().unwrap().permit.claims.endpoint,
            _ => s.d.view.endpoint,
        };
        assert_eq!(committed, pubkey(2), "{ctx}");
        assert!(!w.escaped.contains_key(&2) && w.inboxes_empty(), "{ctx}");
        let durable = s.bytes();

        // Explicit unreachability: bob's binding of k1 is unexpired but nobody serves it.
        w.advance(22);
        let bob = &w.receivers["bob"].view;
        assert!(bob.endpoint == k1 && w.now < bob.expires_at && !w.online.contains_key(&k1));
        assert_eq!(
            s.tick(&mut w, &alice, fail),
            Err(Stop::Failed(Step::StartEndpoint))
        );
        assert_eq!(
            s.bytes(),
            durable,
            "{ctx}: retry must not regenerate or rewrite"
        );
        assert_eq!(w.keygens, 3, "{ctx}");

        w.advance(24);
        s.tick(&mut w, &alice, Fault::None).unwrap();
        settle(&mut w, &mut s);
        assert_eq!(
            (s.d.view.endpoint, s.status, w.keygens),
            (committed, Status::Healthy, 3)
        );
        assert_invariants(&w, &s, &ctx);
        assert_served_only_before(&w, k1, 20, &ctx);
        assert_receivers_at_head(&w, &s, &ctx);
    }
}

// KIND: candidate-policy G-RETIREMENT
#[test]
fn start_endpoint_failure_degraded_grace_keeps_old_visibly_until_bounded_grace() {
    let (alice, _) = identity::generate();
    let fail = Fault::Fail(Step::StartEndpoint);
    for order in [Order::CommitFirst, Order::PendingThenPromote] {
        // grace 3 -> 23; grace 100 is capped by the old signed expiry 25.
        for (grace, until) in [(3, 23), (100, 25)] {
            let ctx = format!("{order:?} grace {grace}");
            let p = Policy {
                start_failure: StartFailure::DegradedGrace { grace },
                ..policy(order)
            };
            let (mut w, mut s) = setup(&alice, p);
            let k1 = s.d.view.endpoint;
            w.advance(20);
            assert!(s.tick(&mut w, &alice, fail).is_err());
            assert_eq!(s.status, Status::Degraded { until }, "{ctx}");
            assert!(
                w.online.contains_key(&k1),
                "{ctx}: old keeps serving, visibly"
            );
            w.advance(21);
            let call = w.open("bob", k1).unwrap();
            w.advance(22);
            assert!(s.tick(&mut w, &alice, fail).is_err());
            assert_eq!(
                s.status,
                Status::Degraded { until },
                "{ctx}: window not extended"
            );
            w.advance(until);
            assert!(s.tick(&mut w, &alice, fail).is_err());
            assert_eq!(s.status, Status::Unreachable, "{ctx}");
            assert_eq!(
                w.sessions[call].closed,
                Some((until, "grace_expired")),
                "{ctx}"
            );
            assert_eq!(w.keygens, 3, "{ctx}: retries reuse the committed key");

            w.advance(until + 1);
            s.tick(&mut w, &alice, Fault::None).unwrap();
            settle(&mut w, &mut s);
            assert_eq!(s.d.view.endpoint, pubkey(2), "{ctx}");
            assert_invariants(&w, &s, &ctx);
            assert_served_only_before(&w, k1, until, &ctx);
        }
    }
    // A crash inside the grace window cannot restart the old endpoint.
    let p = Policy {
        start_failure: StartFailure::DegradedGrace { grace: 3 },
        ..BASE
    };
    let (mut w, mut s) = setup(&alice, p);
    let k1 = s.d.view.endpoint;
    w.advance(20);
    assert!(s.tick(&mut w, &alice, fail).is_err());
    w.advance(21);
    s.crash(&mut w);
    w.advance(22);
    assert!(s.tick(&mut w, &alice, fail).is_err());
    assert_eq!(s.status, Status::Unreachable);
    assert!(w.online.is_empty());
    w.advance(30);
    s.tick(&mut w, &alice, Fault::None).unwrap();
    settle(&mut w, &mut s);
    assert_invariants(&w, &s, "crash in grace");
    assert_served_only_before(&w, k1, 21, "crash in grace");
}

// KIND: invariant
#[test]
fn announce_failure_to_one_peer_is_retried_from_the_persisted_original_and_isolated() {
    let (alice, _) = identity::generate();
    for order in [Order::CommitFirst, Order::PendingThenPromote] {
        let ctx = format!("{order:?}");
        let (mut w, mut s) = setup(&alice, policy(order));
        let k1 = s.d.view.endpoint;
        w.advance(20);
        s.tick(&mut w, &alice, Fault::Fail(Step::Announce("dave")))
            .unwrap();
        assert!(w.receivers["dave"].inbox.is_empty() && w.receivers["bob"].inbox.len() == 1);
        // An unreachable peer does not hold back retirement of the old key.
        assert!(
            s.d.retired.contains(&k1) && !w.online.contains_key(&k1),
            "{ctx}"
        );
        let original = serde_json::to_vec(s.d.history.last().unwrap()).unwrap();
        settle(&mut w, &mut s);
        assert_eq!((s.d.acked["bob"], s.d.acked["dave"]), (2, 1), "{ctx}");

        // Crash before the retry: the retry is rebuilt from durable history.
        s.crash(&mut w);
        w.advance(22);
        s.tick(&mut w, &alice, Fault::None).unwrap();
        assert!(
            w.receivers["bob"].inbox.is_empty(),
            "{ctx}: acked peer not re-sent"
        );
        let msg = w.receivers["dave"].inbox.front().unwrap().clone();
        assert_eq!(msg.permits.len(), 1, "{ctx}");
        assert_eq!(
            serde_json::to_vec(&msg.permits[0]).unwrap(),
            original,
            "{ctx}"
        );
        settle(&mut w, &mut s);
        assert_receivers_at_head(&w, &s, &ctx);
        assert_invariants(&w, &s, &ctx);
    }
}

// KIND: invariant
#[test]
fn retire_and_shutdown_failures_never_let_the_old_endpoint_serve_again() {
    let (alice, _) = identity::generate();
    for order in [Order::CommitFirst, Order::PendingThenPromote] {
        let ctx = format!("{order:?} RetireOld");
        let (mut w, mut s) = setup(&alice, policy(order));
        let k1 = s.d.view.endpoint;
        w.advance(20);
        let fail = Fault::Fail(Step::RetireOld);
        assert_eq!(
            s.tick(&mut w, &alice, fail),
            Err(Stop::Failed(Step::RetireOld))
        );
        assert!(
            !w.online.contains_key(&k1) && w.online.contains_key(&pubkey(2)),
            "{ctx}"
        );
        assert!(!s.d.retired.contains(&k1), "{ctx}");
        match order {
            Order::PendingThenPromote => {
                // Durable state still names k1 as current; the pending record wins on restart.
                assert_eq!(s.d.view.endpoint, k1);
                assert!(s.d.pending.is_some());
            }
            _ => assert_eq!(s.d.retiring.as_ref().unwrap().endpoint, k1),
        }
        s.crash(&mut w);
        w.advance(22);
        s.tick(&mut w, &alice, Fault::None).unwrap();
        settle(&mut w, &mut s);
        assert!(s.d.retired.contains(&k1) && s.d.pending.is_none() && s.d.retiring.is_none());
        assert_invariants(&w, &s, &ctx);
        assert_served_only_before(&w, k1, 20, &ctx);

        let ctx = format!("{order:?} ShutdownOld");
        let (mut w, mut s) = setup(&alice, policy(order));
        let k1 = s.d.view.endpoint;
        w.advance(20);
        let fail = Fault::Fail(Step::ShutdownOld);
        assert_eq!(
            s.tick(&mut w, &alice, fail),
            Err(Stop::Failed(Step::ShutdownOld))
        );
        assert!(s.d.retired.contains(&k1), "{ctx}");
        assert!(
            w.log.contains(&Ev::Stop(k1, 20, "forced_shutdown")),
            "{ctx}"
        );
        w.advance(21);
        s.tick(&mut w, &alice, Fault::None).unwrap();
        settle(&mut w, &mut s);
        assert_invariants(&w, &s, &ctx);
        assert_served_only_before(&w, k1, 20, &ctx);
    }
}

// ---------------------------------------------------------------------------
// 3. Receiver: ack only after commit
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum RCase {
    Fault(Fault<RStep>),
    TamperedSignature,
    ForeignPossession,
}

// KIND: invariant
#[test]
fn receiver_crash_or_failure_before_commit_sends_no_ack_and_leaves_state_unchanged() {
    let (alice, _) = identity::generate();
    for case in [
        RCase::Fault(Fault::CrashBefore(RStep::Verify)),
        RCase::Fault(Fault::CrashAfter(RStep::Verify)),
        RCase::Fault(Fault::CrashBefore(RStep::Commit)),
        RCase::Fault(Fault::Fail(RStep::Commit)),
        RCase::TamperedSignature,
        RCase::ForeignPossession,
    ] {
        let ctx = format!("{case:?}");
        let (mut w, mut s) = setup(&alice, BASE);
        w.advance(20);
        s.tick(&mut w, &alice, Fault::None).unwrap();
        let before = w.receivers["bob"].bytes();
        let acks = w.ack_log.len();
        let fault = match case {
            RCase::Fault(f) => f,
            RCase::TamperedSignature => {
                let msg = w
                    .receivers
                    .get_mut("bob")
                    .unwrap()
                    .inbox
                    .front_mut()
                    .unwrap();
                msg.permits[0].signature[0] ^= 1;
                Fault::None
            }
            RCase::ForeignPossession => {
                let msg = w
                    .receivers
                    .get_mut("bob")
                    .unwrap()
                    .inbox
                    .front_mut()
                    .unwrap();
                let head = msg.permits.last().unwrap().id();
                msg.proof = fixture_key(999).sign(&challenge("bob", &head)).to_bytes();
                Fault::None
            }
        };
        let expected = match case {
            RCase::Fault(Fault::Fail(step)) => RErr::Failed(step),
            RCase::Fault(_) => RErr::Crashed,
            RCase::TamperedSignature => RErr::Rejected("owner_signature"),
            RCase::ForeignPossession => RErr::Rejected("possession"),
        };
        assert_eq!(w.deliver_one("bob", fault), Err(expected), "{ctx}");
        assert_eq!(
            w.receivers["bob"].bytes(),
            before,
            "{ctx}: durable state changed"
        );
        assert_eq!(w.ack_log.len(), acks, "{ctx}: ack without commit");
        settle(&mut w, &mut s); // dave
        assert_eq!(s.d.acked["bob"], 1, "{ctx}");

        // Unacked -> sender retries from its persisted original; idempotent outcome.
        w.advance(21);
        s.tick(&mut w, &alice, Fault::None).unwrap();
        settle(&mut w, &mut s);
        assert_receivers_at_head(&w, &s, &ctx);
        assert_invariants(&w, &s, &ctx);
    }
}

// KIND: invariant
#[test]
fn receiver_crash_or_ack_failure_after_commit_keeps_update_and_redelivery_is_idempotent() {
    let (alice, _) = identity::generate();
    for fault in [
        Fault::CrashAfter(RStep::Commit),
        Fault::CrashBefore(RStep::Ack),
        Fault::Fail(RStep::Ack),
        Fault::CrashAfter(RStep::Ack),
    ] {
        let ctx = format!("{fault:?}");
        let (mut w, mut s) = setup(&alice, BASE);
        w.advance(20);
        s.tick(&mut w, &alice, Fault::None).unwrap();
        assert!(w.deliver_one("bob", fault).is_err(), "{ctx}");
        let bob = &w.receivers["bob"];
        assert_eq!(
            (bob.view.generation, bob.view.head),
            (2, s.d.view.head),
            "{ctx}"
        );
        let committed = bob.bytes();
        let acked = w.ack_log.contains(&("bob", 2));
        assert_eq!(acked, fault == Fault::CrashAfter(RStep::Ack), "{ctx}");
        settle(&mut w, &mut s);
        assert_eq!(s.d.acked["bob"], if acked { 2 } else { 1 }, "{ctx}");

        w.advance(21);
        s.tick(&mut w, &alice, Fault::None).unwrap();
        let resent = !w.receivers["bob"].inbox.is_empty();
        assert_eq!(resent, !acked, "{ctx}: retry exactly when unacked");
        settle(&mut w, &mut s);
        assert_eq!(
            w.receivers["bob"].bytes(),
            committed,
            "{ctx}: redelivery not idempotent"
        );
        assert_receivers_at_head(&w, &s, &ctx);
        assert_invariants(&w, &s, &ctx);
    }
}

// KIND: limitation
#[test]
fn ack_before_commit_ordering_acknowledges_an_update_that_a_crash_then_loses() {
    // Rejected ordering, kept to show why the ack must follow the durable commit.
    let (alice, _) = identity::generate();
    let (mut w, mut s) = setup(&alice, BASE);
    w.receivers.get_mut("bob").unwrap().ack_order = AckOrder::BeforeCommit;
    w.advance(20);
    s.tick(&mut w, &alice, Fault::None).unwrap();
    let before = w.receivers["bob"].bytes();
    assert_eq!(
        w.deliver_one("bob", Fault::CrashBefore(RStep::Commit)),
        Err(RErr::Crashed)
    );
    assert_eq!(w.receivers["bob"].bytes(), before);
    settle(&mut w, &mut s);
    assert_eq!(s.d.acked["bob"], 2, "sender believes bob has generation 2");
    w.advance(21);
    s.tick(&mut w, &alice, Fault::None).unwrap();
    assert!(
        w.receivers["bob"].inbox.is_empty(),
        "no retry: the lost update is never resent"
    );
    let bob = &w.receivers["bob"].view;
    assert_eq!(bob.generation, 1);
    assert!(s.d.retired.contains(&bob.endpoint) && !w.online.contains_key(&bob.endpoint));
    assert!(w
        .ack_log
        .iter()
        .any(|(p, g)| *p == "bob" && *g > bob.generation));
}

// ---------------------------------------------------------------------------
// 4. Pending next key (two-phase) vs commit-first
// ---------------------------------------------------------------------------

// KIND: candidate-policy G-STORAGE
#[test]
fn pending_or_committed_next_key_is_persisted_before_announce_and_restart_resumes_it() {
    let (alice, _) = identity::generate();
    for order in [Order::PendingThenPromote, Order::CommitFirst] {
        let ctx = format!("{order:?}");
        let (mut w, mut s) = setup(&alice, policy(order));
        let k1 = s.d.view.endpoint;
        w.advance(20);
        let writes = w.writes;
        let crash = Fault::CrashAfter(Step::StartEndpoint);
        assert_eq!(s.tick(&mut w, &alice, crash), Err(Stop::Crashed));
        assert!(w.inboxes_empty(), "{ctx}: crashed before any announce");
        let (permit, secret) = match order {
            Order::PendingThenPromote => {
                // Old key is still the durable current; the next key is a pending record.
                assert_eq!((s.d.view.endpoint, s.d.view.generation), (k1, 1));
                assert!(s.d.retiring.is_none());
                let p = s.d.pending.as_ref().unwrap();
                (serde_json::to_vec(&p.permit).unwrap(), p.secret)
            }
            _ => {
                // Next key is already the durable current; the old key is a retiring record.
                assert_eq!(s.d.view.generation, 2);
                assert!(s.d.pending.is_none());
                assert_eq!(s.d.retiring.as_ref().unwrap().endpoint, k1);
                (
                    serde_json::to_vec(s.d.history.last().unwrap()).unwrap(),
                    s.d.secret,
                )
            }
        };
        // Two secrets sit in durable state until RetireOld under BOTH orderings.
        let (keygens, escaped) = (w.keygens, w.escaped[&2].clone());

        w.advance(23);
        s.tick(&mut w, &alice, Fault::None).unwrap();
        settle(&mut w, &mut s);
        assert_eq!(
            w.keygens, keygens,
            "{ctx}: restart must not generate a new key"
        );
        assert_eq!(w.escaped[&2], escaped, "{ctx}");
        assert_eq!(s.d.secret, secret, "{ctx}");
        assert_eq!(
            serde_json::to_vec(s.d.history.last().unwrap()).unwrap(),
            permit
        );
        assert_receivers_at_head(&w, &s, &ctx);
        assert_invariants(&w, &s, &ctx);
        // Same cost: two rotation writes (seal/pending + retire/promote) + two ack writes.
        assert_eq!(w.writes - writes, 4, "{ctx}");
    }
}

// ---------------------------------------------------------------------------
// 5. Active sessions on the old endpoint at retirement
// ---------------------------------------------------------------------------

/// No session survives across `t` on the endpoint: there is no uninterrupted call.
fn assert_no_session_spans(w: &World, ep: Ep, t: u64) {
    for s in &w.sessions {
        if s.ep == ep && s.opened < t {
            let (closed, _) = s.closed.expect("session still open on retired endpoint");
            assert!(closed <= t, "{} kept old endpoint past {t}", s.peer);
        }
    }
}

// KIND: candidate-policy G-RETIREMENT
#[test]
fn terminate_policy_interrupts_active_old_sessions_at_retirement() {
    let (alice, _) = identity::generate();
    let (mut w, mut s) = setup(&alice, BASE);
    let k1 = s.d.view.endpoint;
    w.advance(15);
    let call = w.open("bob", k1).unwrap();
    w.advance(18);
    let transfer = w.open("dave", k1).unwrap();
    w.advance(20);
    s.tick(&mut w, &alice, Fault::None).unwrap();
    settle(&mut w, &mut s);
    let k2 = s.d.view.endpoint;
    assert_eq!(w.sessions[call].closed, Some((20, "retired")));
    assert_eq!(w.sessions[transfer].closed, Some((20, "retired")));
    assert_eq!(w.open("bob", k1), Err("offline"));
    // Continuation is a NEW session on the new endpoint, not the same call.
    let again = w.open("bob", k2).unwrap();
    assert_ne!(again, call);
    assert!(w.sessions[again].opened >= w.sessions[call].closed.unwrap().0);
    assert_no_session_spans(&w, k1, 20);
    assert_invariants(&w, &s, "terminate");
}

// KIND: candidate-policy G-RETIREMENT
#[test]
fn bounded_overlap_drains_existing_sessions_until_cap_and_long_calls_are_still_cut() {
    let (alice, _) = identity::generate();
    // max 3 -> cap 23; max 100 is capped by the old signed expiry 25.
    for (max, cap) in [(3, 23), (100, 25)] {
        let ctx = format!("overlap {max}");
        let p = Policy {
            overlap: Overlap::Bounded { max },
            ..BASE
        };
        let (mut w, mut s) = setup(&alice, p);
        let k1 = s.d.view.endpoint;
        w.advance(15);
        let call = w.open("bob", k1).unwrap();
        w.advance(18);
        let transfer = w.open("dave", k1).unwrap();
        w.advance(20);
        s.tick(&mut w, &alice, Fault::None).unwrap();
        settle(&mut w, &mut s);
        assert!(s.d.retired.contains(&k1), "{ctx}");
        assert_eq!(w.draining.get(&k1), Some(&cap), "{ctx}");
        assert_eq!(w.open("bob", k1), Err("draining"), "{ctx}: no new sessions");
        w.advance(22);
        w.end(transfer); // finished inside the window: not interrupted
        w.advance(cap);
        assert_eq!(w.sessions[transfer].closed, Some((22, "ended")), "{ctx}");
        assert_eq!(w.sessions[call].closed, Some((cap, "overlap_cap")), "{ctx}");
        assert_no_session_spans(&w, k1, cap);
        assert_invariants(&w, &s, &ctx);
        assert_served_only_before(&w, k1, cap, &ctx);
    }
    // Crash during the overlap: drained sessions die and the old key never restarts.
    let p = Policy {
        overlap: Overlap::Bounded { max: 3 },
        ..BASE
    };
    let (mut w, mut s) = setup(&alice, p);
    let k1 = s.d.view.endpoint;
    w.advance(15);
    let call = w.open("bob", k1).unwrap();
    w.advance(20);
    s.tick(&mut w, &alice, Fault::None).unwrap();
    w.advance(21);
    s.crash(&mut w);
    assert_eq!(w.sessions[call].closed, Some((21, "crash")));
    w.advance(22);
    s.tick(&mut w, &alice, Fault::None).unwrap();
    settle(&mut w, &mut s);
    assert_eq!(
        w.online.keys().copied().collect::<Vec<_>>(),
        vec![s.d.view.endpoint]
    );
    assert_invariants(&w, &s, "crash in overlap");
    assert_served_only_before(&w, k1, 21, "crash in overlap");
}
