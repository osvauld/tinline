//! Packet C3 model: live-possession challenge storage options for endpoint rotation.
//! Test-only candidate semantics, NOT a production wire format, handshake or storage design.
//!
//! G-STORAGE is OPEN. Two candidates are modelled side by side; neither is selected here:
//!
//! (a) `NonceReceiver` — the existing fixture (`support/rotation_permit_network.rs:96-124`):
//!     a per-receiver DURABLE nonce high-water plus a VOLATILE outstanding set (capacity 8,
//!     TTL 5). The challenge is not tied to the connection that asked for it.
//! (b) `SessionReceiver` — session-bound: the proof signs a transcript holding a
//!     receiver-chosen random challenge and BOTH endpoint keys; it is valid only inside the one
//!     session it was issued in; nothing about challenges is persisted.
//!
//! Model assumptions (all fake, all in memory):
//! - "Session" stands for an authenticated transport connection. `remote` is the key the
//!   transport authenticated (as `conn.remote_id()` is used by today's hellos). The model
//!   TRUSTS that value; it does not model TLS. Nobody can open a session as a key they lack.
//! - Durable state survives `restart`; volatile state does not. Every durable mutation
//!   increments `disk_writes` and is an IDEAL atomic commit (no torn writes; see C2).
//! - Randomness for (b) is a deterministic stand-in: `H(boot_entropy || draw)`. `boot_entropy`
//!   models CSPRNG output at boot. This is NOT a protocol derivation scheme.
//! - Fixture endpoint keys come from fixed seeds only so tests are deterministic; receivers
//!   never compute them. This is NOT a future-endpoint derivation scheme.
//! - Failed accepts do NOT consume the challenge (as B1 chose); success consumes it.
//! - Rejections leave the whole receiver (including `disk_writes`) byte-for-byte unchanged.
use std::collections::BTreeMap;

use ed25519_dalek::{Signer, SigningKey};
use identity::Identity;
use sha2::{Digest, Sha256};

#[path = "support/permit_history.rs"]
mod permit_history;
use permit_history::*;

const NONCE_DOMAIN: &[u8] = b"test-only/rotation-possession/nonce/v1\0";
const SESSION_DOMAIN: &[u8] = b"test-only/rotation-possession/session/v1\0";
// Existing variant (a) fixture values (rotation_permit_network.rs:101,104). NOT product defaults.
const CAPACITY: usize = 8;
const TTL: u64 = 5;
const VALIDITY: u64 = 100;


fn verify_page(view: &View, permits: &[Permit], now: u64) -> Result<View, &'static str> {
    if permits.is_empty() || permits.len() > PAGE {
        return Err("page_limit");
    }
    let key = identity::public_key_from_did(&view.owner).ok_or("owner_key")?;
    let mut candidate = view.clone();
    for permit in permits {
        let c = &permit.claims;
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
    Ok(candidate)
}

fn seed32(domain: &[u8], seed: u64) -> Id {
    Sha256::digest([domain, &seed.to_be_bytes()].concat()).into()
}

/// Deterministic fixture endpoint key. Receivers never compute this.
fn key(seed: u64) -> SigningKey {
    SigningKey::from_bytes(&seed32(b"test-only/c3-fixture-endpoint\0", seed))
}

fn public(k: &SigningKey) -> Id {
    k.verifying_key().to_bytes()
}

/// Owner-signed successor for `previous` whose endpoint is `key(seed)`.
fn successor(owner: &Identity, previous: &View, seed: u64, now: u64) -> (Permit, SigningKey) {
    let k = key(seed);
    let p = Permit::issue(
        owner,
        Claims {
            version: 1,
            owner: owner.did().into(),
            audience: previous.audience.clone(),
            previous_id: previous.current_id,
            replacement_id: seed32(b"test-only/c3-fixture-device-id\0", seed),
            endpoint: public(&k),
            predecessor: previous.head,
            generation: previous.generation + 1,
            issued_at: now,
            expires_at: now + VALIDITY,
        },
    );
    (p, k)
}

// ---------------------------------------------------------------------------------------
// Accepted (durable) receiver state shared by both candidates.
// ---------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
struct Live {
    head: Id,
    endpoint: Id,
    bound_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct Accepted {
    views: BTreeMap<String, View>,
    live: BTreeMap<String, Live>,
}

impl Accepted {
    fn with(views: &[&View]) -> Self {
        Self {
            views: views
                .iter()
                .map(|v| (v.owner.clone(), (*v).clone()))
                .collect(),
            live: BTreeMap::new(),
        }
    }
    /// Dry-run: verify the page and that its newest record is currently valid.
    fn check(&self, page: &[Permit], now: u64) -> Result<(String, View), &'static str> {
        let owner = &page.first().ok_or("page_limit")?.claims.owner;
        let view = self.views.get(owner).ok_or("unknown_owner")?;
        let next = verify_page(view, page, now)?;
        if next.generation == 0 || now >= next.expires_at {
            return Err("expired");
        }
        Ok((owner.clone(), next))
    }
    fn apply(&self, owner: String, next: View, now: u64) -> Self {
        let mut out = self.clone();
        out.live.insert(
            owner.clone(),
            Live {
                head: next.head,
                endpoint: next.endpoint,
                bound_at: now,
            },
        );
        out.views.insert(owner, next);
        out
    }
}

fn newest(page: &[Permit]) -> &Permit {
    page.last().expect("non-empty page")
}

/// How challenge capacity is partitioned. `global` is always enforced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Quota {
    /// Existing fixture: one shared pool per receiver.
    GlobalOnly,
    /// At most n outstanding per transport-authenticated requester key.
    PerRemote(usize),
    /// At most n outstanding per owner DID named by the (verified) page.
    PerOwner(usize),
}

// ---------------------------------------------------------------------------------------
// Candidate (a): durable nonce high-water + volatile outstanding set.
// ---------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
struct NonceChallenge {
    receiver: Id,
    nonce: u64,
    binding: Id, // newest permit ID the challenge was issued for
}

impl NonceChallenge {
    fn bytes(&self) -> Vec<u8> {
        [
            NONCE_DOMAIN,
            &self.receiver,
            &self.nonce.to_be_bytes(),
            &self.binding,
        ]
        .concat()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Outstanding {
    requester: Id,
    owner: String,
    binding: Id,
    deadline: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NonceReceiver {
    endpoint: Id,
    quota: Quota,
    // Durable.
    accepted: Accepted,
    high_water: u64,
    // Last durably written high-water. Equals `high_water` when `persist_high_water`.
    persisted_high_water: u64,
    // Volatile.
    outstanding: BTreeMap<u64, Outstanding>,
    // Model knob: false demonstrates a counter that is NOT written before issuance.
    persist_high_water: bool,
    disk_writes: u64,
}

impl NonceReceiver {
    fn new(endpoint: Id, accepted: Accepted, quota: Quota) -> Self {
        Self {
            endpoint,
            quota,
            accepted,
            high_water: 0,
            persisted_high_water: 0,
            outstanding: BTreeMap::new(),
            persist_high_water: true,
            disk_writes: 0,
        }
    }

    /// Issue a challenge to a connection authenticated as `requester` for an update `page`.
    /// All work happens on a candidate; it is committed only on success.
    fn issue(
        &mut self,
        now: u64,
        requester: Id,
        page: &[Permit],
    ) -> Result<NonceChallenge, &'static str> {
        let (owner, _) = self.accepted.check(page, now)?;
        let mut next = self.clone();
        next.outstanding.retain(|_, o| now < o.deadline);
        if next.outstanding.len() >= CAPACITY {
            return Err("challenge_capacity");
        }
        match self.quota {
            Quota::GlobalOnly => {}
            Quota::PerRemote(n) => {
                if next
                    .outstanding
                    .values()
                    .filter(|o| o.requester == requester)
                    .count()
                    >= n
                {
                    return Err("quota");
                }
            }
            Quota::PerOwner(n) => {
                if next
                    .outstanding
                    .values()
                    .filter(|o| o.owner == owner)
                    .count()
                    >= n
                {
                    return Err("quota");
                }
            }
        }
        next.high_water = next.high_water.checked_add(1).ok_or("nonce_exhausted")?;
        if next.persist_high_water {
            // Write-ahead of issuance: the nonce must be durable before it leaves the device.
            next.persisted_high_water = next.high_water;
            next.disk_writes += 1;
        }
        let binding = newest(page).id();
        next.outstanding.insert(
            next.high_water,
            Outstanding {
                requester,
                owner,
                binding,
                deadline: now + TTL,
            },
        );
        let challenge = NonceChallenge {
            receiver: self.endpoint,
            nonce: next.high_water,
            binding,
        };
        *self = next;
        Ok(challenge)
    }

    /// Accept a page plus possession proof. NOT bound to the requesting connection.
    fn accept(
        &mut self,
        now: u64,
        page: &[Permit],
        nonce: u64,
        proof: &[u8; 64],
    ) -> Result<(), &'static str> {
        let o = self.outstanding.get(&nonce).ok_or("challenge_unknown")?;
        if now >= o.deadline {
            return Err("challenge_expired");
        }
        let n = newest(page);
        if n.id() != o.binding {
            return Err("binding");
        }
        let challenge = NonceChallenge {
            receiver: self.endpoint,
            nonce,
            binding: o.binding,
        };
        if !identity::verify(&n.claims.endpoint, &challenge.bytes(), proof) {
            return Err("possession");
        }
        let (owner, view) = self.accepted.check(page, now)?;
        let mut next = self.clone();
        next.outstanding.remove(&nonce);
        next.accepted = self.accepted.apply(owner, view, now);
        next.disk_writes += 1;
        *self = next;
        Ok(())
    }

    fn restart(&mut self) {
        self.outstanding.clear();
        self.high_water = self.persisted_high_water;
    }
}

fn prove_nonce(k: &SigningKey, c: &NonceChallenge) -> [u8; 64] {
    k.sign(&c.bytes()).to_bytes()
}

// ---------------------------------------------------------------------------------------
// Candidate (b): session-bound transcript; nothing about challenges persisted.
// ---------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
struct Session {
    remote: Id, // transport-authenticated key (model input, trusted)
    owner: Option<String>,
    challenge: Option<(Id, Id)>, // (random challenge, newest permit ID)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SessionChallenge {
    session: u64,
    receiver: Id,
    challenge: Id,
    binding: Id,
}

fn transcript(receiver: Id, sender: Id, challenge: Id, binding: Id) -> Vec<u8> {
    [SESSION_DOMAIN, &receiver, &sender, &challenge, &binding].concat()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SessionReceiver {
    endpoint: Id,
    quota: Quota,
    // Durable.
    accepted: Accepted,
    // Volatile. Session IDs restart at 0 every boot (like a per-process connection id).
    sessions: BTreeMap<u64, Session>,
    next_session: u64,
    boot_entropy: Id,
    draws: u64,
    disk_writes: u64,
}

impl SessionReceiver {
    fn new(endpoint: Id, accepted: Accepted, quota: Quota, boot_entropy: Id) -> Self {
        Self {
            endpoint,
            quota,
            accepted,
            sessions: BTreeMap::new(),
            next_session: 0,
            boot_entropy,
            draws: 0,
            disk_writes: 0,
        }
    }

    /// A transport session authenticated as `remote`. No disk write.
    fn open(&mut self, remote: Id) -> u64 {
        let id = self.next_session;
        self.next_session += 1;
        self.sessions.insert(
            id,
            Session {
                remote,
                owner: None,
                challenge: None,
            },
        );
        id
    }

    fn close(&mut self, session: u64) {
        self.sessions.remove(&session);
    }

    fn challenge(
        &mut self,
        now: u64,
        session: u64,
        page: &[Permit],
    ) -> Result<SessionChallenge, &'static str> {
        let s = self.sessions.get(&session).ok_or("session_unknown")?;
        if s.challenge.is_some() {
            return Err("challenge_pending");
        }
        let (owner, _) = self.accepted.check(page, now)?;
        // Admission is bound to the session: only the holder of the page's newest endpoint
        // key can reach this point, because the transport authenticated `remote`.
        if s.remote != newest(page).claims.endpoint {
            return Err("remote_mismatch");
        }
        let pending = || self.sessions.values().filter(|s| s.challenge.is_some());
        if pending().count() >= CAPACITY {
            return Err("challenge_capacity");
        }
        match self.quota {
            Quota::GlobalOnly => {}
            Quota::PerRemote(n) => {
                if pending().filter(|p| p.remote == s.remote).count() >= n {
                    return Err("quota");
                }
            }
            Quota::PerOwner(n) => {
                if pending()
                    .filter(|p| p.owner.as_deref() == Some(owner.as_str()))
                    .count()
                    >= n
                {
                    return Err("quota");
                }
            }
        }
        let mut next = self.clone();
        let random: Id =
            Sha256::digest([next.boot_entropy.as_slice(), &next.draws.to_be_bytes()].concat())
                .into();
        next.draws += 1;
        let binding = newest(page).id();
        let entry = next.sessions.get_mut(&session).unwrap();
        entry.owner = Some(owner);
        entry.challenge = Some((random, binding));
        *self = next;
        Ok(SessionChallenge {
            session,
            receiver: self.endpoint,
            challenge: random,
            binding,
        })
    }

    fn accept(
        &mut self,
        now: u64,
        session: u64,
        page: &[Permit],
        proof: &[u8; 64],
    ) -> Result<(), &'static str> {
        let s = self.sessions.get(&session).ok_or("session_unknown")?;
        let (random, binding) = s.challenge.ok_or("challenge_unknown")?;
        let n = newest(page);
        if n.id() != binding {
            return Err("binding");
        }
        if s.remote != n.claims.endpoint {
            return Err("remote_mismatch");
        }
        let t = transcript(self.endpoint, s.remote, random, binding);
        if !identity::verify(&n.claims.endpoint, &t, proof) {
            return Err("possession");
        }
        let (owner, view) = self.accepted.check(page, now)?;
        let mut next = self.clone();
        let entry = next.sessions.get_mut(&session).unwrap();
        entry.challenge = None;
        entry.owner = None;
        next.accepted = self.accepted.apply(owner, view, now);
        next.disk_writes += 1;
        *self = next;
        Ok(())
    }

    fn restart(&mut self, boot_entropy: Id) {
        self.sessions.clear();
        self.next_session = 0;
        self.draws = 0;
        self.boot_entropy = boot_entropy;
    }
}

fn prove_session(k: &SigningKey, c: &SessionChallenge) -> [u8; 64] {
    k.sign(&transcript(c.receiver, public(k), c.challenge, c.binding))
        .to_bytes()
}

// ---------------------------------------------------------------------------------------
// Fixture world: a receiver knows two contacts (alice, bob) at generation 0.
// ---------------------------------------------------------------------------------------

struct World {
    alice: Identity,
    bob: Identity,
    accepted: Accepted,
    a_page: Vec<Permit>,
    a_key: SigningKey,
    b_page: Vec<Permit>,
    b_key: SigningKey,
}

fn world(now: u64) -> World {
    let (alice, _) = identity::generate();
    let (bob, _) = identity::generate();
    let va = View::initial_private(&alice, 1);
    let vb = View::initial_private(&bob, 1);
    let (pa, ka) = successor(&alice, &va, 101, now);
    let (pb, kb) = successor(&bob, &vb, 201, now);
    World {
        accepted: Accepted::with(&[&va, &vb]),
        alice,
        bob,
        a_page: vec![pa],
        a_key: ka,
        b_page: vec![pb],
        b_key: kb,
    }
}

fn receiver_key() -> Id {
    public(&key(9_000))
}

fn entropy(boot: u64) -> Id {
    seed32(b"test-only/c3-boot-entropy\0", boot)
}

// =======================================================================================
// Invariants (must hold for either candidate)
// =======================================================================================

// KIND: invariant
#[test]
fn both_variants_bind_only_the_newest_endpoint_key_and_reject_unchanged() {
    let now = 10;
    let w = world(now);
    let wrong = key(666);

    // (a) A valid challenge answered by a key that is not the newest endpoint.
    let mut a = NonceReceiver::new(receiver_key(), w.accepted.clone(), Quota::GlobalOnly);
    let c = a.issue(now, public(&w.a_key), &w.a_page).unwrap();
    let before = a.clone();
    assert_eq!(
        a.accept(now, &w.a_page, c.nonce, &prove_nonce(&wrong, &c)),
        Err("possession")
    );
    assert_eq!(a, before);
    // Proof for alice's challenge cannot carry bob's page (challenge binds the permit ID).
    assert_eq!(
        a.accept(now, &w.b_page, c.nonce, &prove_nonce(&w.b_key, &c)),
        Err("binding")
    );
    assert_eq!(a, before);
    a.accept(now, &w.a_page, c.nonce, &prove_nonce(&w.a_key, &c))
        .unwrap();
    assert_eq!(a.accepted.live[w.alice.did()].endpoint, public(&w.a_key));

    // (b) A session authenticated as some other key cannot even obtain a challenge.
    let mut b = SessionReceiver::new(
        receiver_key(),
        w.accepted.clone(),
        Quota::GlobalOnly,
        entropy(1),
    );
    let s_wrong = b.open(public(&wrong));
    let before = b.clone();
    assert_eq!(b.challenge(now, s_wrong, &w.a_page), Err("remote_mismatch"));
    assert_eq!(b, before);
    let s = b.open(public(&w.a_key));
    let c = b.challenge(now, s, &w.a_page).unwrap();
    let before = b.clone();
    assert_eq!(
        b.accept(now, s, &w.a_page, &prove_session(&wrong, &c)),
        Err("possession")
    );
    assert_eq!(b, before);
    assert_eq!(
        b.accept(now, s, &w.b_page, &prove_session(&w.b_key, &c)),
        Err("binding")
    );
    assert_eq!(b, before);
    b.accept(now, s, &w.a_page, &prove_session(&w.a_key, &c))
        .unwrap();
    assert_eq!(b.accepted.live[w.alice.did()].endpoint, public(&w.a_key));
    assert!(b.accepted.live.get(w.bob.did()).is_none());
}

// KIND: invariant
#[test]
fn nonce_high_water_never_wraps_and_exhaustion_survives_restart() {
    let now = 10;
    let w = world(now);
    let mut a = NonceReceiver::new(receiver_key(), w.accepted.clone(), Quota::GlobalOnly);
    a.high_water = u64::MAX - 1;
    a.persisted_high_water = u64::MAX - 1;
    let last = a.issue(now, public(&w.a_key), &w.a_page).unwrap();
    assert_eq!(last.nonce, u64::MAX);
    let before = a.clone();
    assert_eq!(
        a.issue(now, public(&w.b_key), &w.b_page),
        Err("nonce_exhausted")
    );
    assert_eq!(a, before);
    // The last issued challenge is still usable.
    a.accept(now, &w.a_page, last.nonce, &prove_nonce(&w.a_key, &last))
        .unwrap();
    // Exhaustion is durable: a restart cannot recover issuance without a new receiver key or
    // an explicit reset policy (both unresolved). This is a permanent liveness failure.
    a.restart();
    let before = a.clone();
    assert_eq!(
        a.issue(now, public(&w.b_key), &w.b_page),
        Err("nonce_exhausted")
    );
    assert_eq!(a, before);
}

// =======================================================================================
// Candidate (a): durable nonce high-water + volatile outstanding set
// =======================================================================================

// KIND: candidate-policy G-STORAGE
#[test]
fn nonce_replay_across_sessions_is_rejected_but_answer_is_not_session_bound() {
    let now = 10;
    let w = world(now);
    let mut a = NonceReceiver::new(receiver_key(), w.accepted.clone(), Quota::GlobalOnly);
    // Connection X (authenticated as some relay/old key) asks; the answer arrives elsewhere.
    let asker = public(&key(777));
    let c = a.issue(now, asker, &w.a_page).unwrap();
    let proof = prove_nonce(&w.a_key, &c);
    // Property of (a): the challenge is not tied to the connection that requested it, so
    // the proof is accepted the first time regardless of which session carries it.
    a.accept(now + 1, &w.a_page, c.nonce, &proof).unwrap();
    let after = a.clone();
    // Replay of the same (nonce, proof) on any later session: consumed, unchanged.
    assert_eq!(
        a.accept(now + 2, &w.a_page, c.nonce, &proof),
        Err("challenge_unknown")
    );
    assert_eq!(a, after);
    // A fresh challenge in a later session gets a strictly larger nonce; the old proof fails.
    let c2 = a.issue(now + 2, public(&w.a_key), &w.a_page).unwrap();
    assert!(c2.nonce > c.nonce);
    let before = a.clone();
    assert_eq!(
        a.accept(now + 2, &w.a_page, c2.nonce, &proof),
        Err("possession")
    );
    assert_eq!(a, before);
}

// KIND: candidate-policy G-STORAGE
#[test]
fn nonce_replay_after_receiver_restart_is_rejected_by_durable_high_water() {
    let now = 10;
    let w = world(now);
    let mut a = NonceReceiver::new(receiver_key(), w.accepted.clone(), Quota::GlobalOnly);
    let c = a.issue(now, public(&w.a_key), &w.a_page).unwrap();
    let captured = prove_nonce(&w.a_key, &c); // produced, but receiver crashed before accept
    a.restart();
    assert!(a.outstanding.is_empty());
    let before = a.clone();
    assert_eq!(
        a.accept(now + 1, &w.a_page, c.nonce, &captured),
        Err("challenge_unknown")
    );
    assert_eq!(a, before);
    // Post-restart issuance never reuses the old nonce, so the captured proof stays useless.
    let attacker = public(&key(778));
    let c2 = a.issue(now + 1, attacker, &w.a_page).unwrap();
    assert_eq!(c2.nonce, c.nonce + 1);
    let before = a.clone();
    assert_eq!(
        a.accept(now + 1, &w.a_page, c2.nonce, &captured),
        Err("possession")
    );
    assert_eq!(a, before);
    assert!(a.accepted.live.is_empty());
}

// KIND: limitation
#[test]
fn nonce_counter_without_write_ahead_reissues_old_nonce_and_accepts_captured_proof() {
    // Demonstrates WHY (a) needs a durable write per issued challenge (or per reserved block).
    let now = 10;
    let w = world(now);
    let mut a = NonceReceiver::new(receiver_key(), w.accepted.clone(), Quota::GlobalOnly);
    a.persist_high_water = false;
    let c = a.issue(now, public(&w.a_key), &w.a_page).unwrap();
    let captured = prove_nonce(&w.a_key, &c);
    a.restart(); // counter falls back to the last persisted value (0)
    // The honest device is now offline. An attacker WITHOUT the key asks for a challenge.
    let attacker = public(&key(779));
    let c2 = a.issue(now + 1, attacker, &w.a_page).unwrap();
    assert_eq!(c2, c);
    a.accept(now + 1, &w.a_page, c2.nonce, &captured).unwrap();
    // A live binding was recorded at now+1 from a proof that was never produced for it.
    assert_eq!(a.accepted.live[w.alice.did()].bound_at, now + 1);
    assert_eq!(a.disk_writes, 1); // only the accept wrote; zero writes for issuance
}

// KIND: candidate-policy G-STORAGE
#[test]
fn nonce_capacity_exhaustion_costs_disk_writes_and_recycles_only_after_ttl() {
    let now = 10;
    let w = world(now);
    let mut a = NonceReceiver::new(receiver_key(), w.accepted.clone(), Quota::GlobalOnly);
    // Any connection can request; the page is a captured, valid public update of alice.
    for i in 0..CAPACITY as u64 {
        a.issue(now, public(&key(1_000 + i)), &w.a_page).unwrap();
    }
    assert_eq!(a.disk_writes, CAPACITY as u64); // writes forced with no accepted update
    let before = a.clone();
    assert_eq!(
        a.issue(now + TTL - 1, public(&w.a_key), &w.a_page),
        Err("challenge_capacity")
    );
    assert_eq!(a, before);
    // Capacity recycles at the TTL boundary; the flood can repeat at CAPACITY writes per TTL.
    let c = a.issue(now + TTL, public(&w.a_key), &w.a_page).unwrap();
    assert_eq!(a.outstanding.len(), 1);
    assert_eq!(a.disk_writes, CAPACITY as u64 + 1);
    a.accept(now + TTL, &w.a_page, c.nonce, &prove_nonce(&w.a_key, &c))
        .unwrap();
    assert_eq!(a.disk_writes, CAPACITY as u64 + 2);
}

// KIND: limitation
#[test]
fn nonce_variant_admission_cannot_isolate_peers_from_an_unauthenticated_requester() {
    // In (a) issuance happens before possession, so the requester can be anyone holding a
    // captured (public) update. Each quota choice below is defeated by a different flood.
    let now = 10;
    let w = world(now);
    for quota in [Quota::GlobalOnly, Quota::PerRemote(2), Quota::PerOwner(2)] {
        let mut a = NonceReceiver::new(receiver_key(), w.accepted.clone(), quota);
        match quota {
            // One key floods the shared pool with alice's page; bob is blocked.
            Quota::GlobalOnly => {
                for _ in 0..CAPACITY {
                    a.issue(now, public(&key(1)), &w.a_page).unwrap();
                }
            }
            // Fresh keys are free: Sybil requesters fill the pool, 2 each.
            Quota::PerRemote(n) => {
                for i in 0..CAPACITY {
                    a.issue(now, public(&key(10 + (i / n) as u64)), &w.a_page)
                        .unwrap();
                }
            }
            // Targeted: replaying BOB's own captured page exhausts bob's quota.
            Quota::PerOwner(n) => {
                for i in 0..n {
                    a.issue(now, public(&key(20 + i as u64)), &w.b_page)
                        .unwrap();
                }
            }
        }
        let before = a.clone();
        let r = a.issue(now, public(&w.b_key), &w.b_page);
        assert!(
            r == Err("challenge_capacity") || r == Err("quota"),
            "{quota:?}: {r:?}"
        );
        assert_eq!(a, before);
    }
}

// KIND: candidate-policy G-STORAGE
#[test]
fn nonce_per_remote_quota_isolates_a_single_flooding_key() {
    let now = 10;
    let w = world(now);
    let mut a = NonceReceiver::new(receiver_key(), w.accepted.clone(), Quota::PerRemote(2));
    let flooder = public(&key(1));
    a.issue(now, flooder, &w.a_page).unwrap();
    a.issue(now, flooder, &w.a_page).unwrap();
    let before = a.clone();
    assert_eq!(a.issue(now, flooder, &w.a_page), Err("quota"));
    assert_eq!(a, before);
    let c = a.issue(now, public(&w.b_key), &w.b_page).unwrap();
    a.accept(now, &w.b_page, c.nonce, &prove_nonce(&w.b_key, &c))
        .unwrap();
    assert_eq!(a.accepted.live[w.bob.did()].endpoint, public(&w.b_key));
}

// =======================================================================================
// Candidate (b): session-bound transcript, nothing persisted for challenges
// =======================================================================================

// KIND: candidate-policy G-STORAGE
#[test]
fn session_proof_is_rejected_in_any_other_session_or_receiver_and_after_use() {
    let now = 10;
    let w = world(now);
    let ra = receiver_key();
    let mut b = SessionReceiver::new(ra, w.accepted.clone(), Quota::GlobalOnly, entropy(1));
    let s1 = b.open(public(&w.a_key));
    let c1 = b.challenge(now, s1, &w.a_page).unwrap();
    let proof1 = prove_session(&w.a_key, &c1);
    // Same remote, second session: a different random challenge, so proof1 fails.
    let s2 = b.open(public(&w.a_key));
    let c2 = b.challenge(now, s2, &w.a_page).unwrap();
    assert_ne!(c1.challenge, c2.challenge);
    let before = b.clone();
    assert_eq!(b.accept(now, s2, &w.a_page, &proof1), Err("possession"));
    assert_eq!(b, before);
    // Proof1 used in its own session succeeds once.
    b.accept(now, s1, &w.a_page, &proof1).unwrap();
    let after = b.clone();
    assert_eq!(
        b.accept(now, s1, &w.a_page, &proof1),
        Err("challenge_unknown")
    );
    assert_eq!(b, after);
    // After the session closes the proof has nowhere to go.
    b.close(s1);
    let before = b.clone();
    assert_eq!(
        b.accept(now, s1, &w.a_page, &proof1),
        Err("session_unknown")
    );
    assert_eq!(b, before);
    // Another receiver that happens to draw the SAME challenge still rejects: the transcript
    // names the receiver's endpoint key.
    let mut other = SessionReceiver::new(
        public(&key(9_001)),
        w.accepted.clone(),
        Quota::GlobalOnly,
        entropy(1),
    );
    let so = other.open(public(&w.a_key));
    let co = other.challenge(now, so, &w.a_page).unwrap();
    assert_eq!(co.challenge, c1.challenge);
    let before = other.clone();
    assert_eq!(other.accept(now, so, &w.a_page, &proof1), Err("possession"));
    assert_eq!(other, before);
}

// KIND: candidate-policy G-STORAGE
#[test]
fn session_replay_after_receiver_restart_fails_although_session_ids_repeat() {
    let now = 10;
    let w = world(now);
    let mut b = SessionReceiver::new(
        receiver_key(),
        w.accepted.clone(),
        Quota::GlobalOnly,
        entropy(1),
    );
    let s = b.open(public(&w.a_key));
    let c = b.challenge(now, s, &w.a_page).unwrap();
    let captured = prove_session(&w.a_key, &c); // receiver crashes before accept
    b.restart(entropy(2));
    let before = b.clone();
    assert_eq!(
        b.accept(now + 1, s, &w.a_page, &captured),
        Err("session_unknown")
    );
    assert_eq!(b, before);
    // Session numbering restarts: the new session reuses id 0. Safety comes from the random
    // challenge, not from any persisted counter.
    let s_new = b.open(public(&w.a_key));
    assert_eq!(s_new, s);
    let c_new = b.challenge(now + 1, s_new, &w.a_page).unwrap();
    let before = b.clone();
    assert_eq!(
        b.accept(now + 1, s_new, &w.a_page, &captured),
        Err("possession")
    );
    assert_eq!(b, before);
    b.accept(now + 1, s_new, &w.a_page, &prove_session(&w.a_key, &c_new))
        .unwrap();
    assert_eq!(b.disk_writes, 1);
}

// KIND: limitation
#[test]
fn session_replay_succeeds_if_challenge_randomness_repeats_after_restart() {
    // (b) moves the freshness requirement from disk to the RNG. A receiver that reboots with
    // repeated entropy re-issues the same challenge in the same-numbered session.
    let now = 10;
    let w = world(now);
    let mut b = SessionReceiver::new(
        receiver_key(),
        w.accepted.clone(),
        Quota::GlobalOnly,
        entropy(1),
    );
    let s = b.open(public(&w.a_key));
    let c = b.challenge(now, s, &w.a_page).unwrap();
    let captured = prove_session(&w.a_key, &c);
    b.restart(entropy(1)); // BAD: same entropy as the previous boot
    let s2 = b.open(public(&w.a_key));
    let c2 = b.challenge(now + 1, s2, &w.a_page).unwrap();
    assert_eq!(c2, c);
    // Note: this replay still needs a session authenticated as the endpoint key, so in the
    // model only the key holder could exploit it; it shows the transcript alone is not fresh.
    b.accept(now + 1, s2, &w.a_page, &captured).unwrap();
    assert_eq!(b.accepted.live[w.alice.did()].bound_at, now + 1);
}

// KIND: candidate-policy G-STORAGE
#[test]
fn session_capacity_exhaustion_costs_no_disk_writes_and_recycles_on_close() {
    let now = 10;
    let w = world(now);
    let mut b = SessionReceiver::new(
        receiver_key(),
        w.accepted.clone(),
        Quota::GlobalOnly,
        entropy(1),
    );
    // Only the holder of alice's newest key can obtain challenges for alice's page.
    let mut held = Vec::new();
    for _ in 0..CAPACITY {
        let s = b.open(public(&w.a_key));
        b.challenge(now, s, &w.a_page).unwrap();
        held.push(s);
    }
    assert_eq!(b.disk_writes, 0);
    let sb = b.open(public(&w.b_key));
    let before = b.clone();
    assert_eq!(b.challenge(now, sb, &w.b_page), Err("challenge_capacity"));
    assert_eq!(b, before);
    // No TTL in this model: capacity returns when a session closes (transport idle timeout
    // in production; value unverified).
    b.close(held[0]);
    let cb = b.challenge(now, sb, &w.b_page).unwrap();
    b.accept(now, sb, &w.b_page, &prove_session(&w.b_key, &cb))
        .unwrap();
    assert_eq!(b.disk_writes, 1);
    // One pending challenge per session: a second request in the same session is refused.
    let s = held[1];
    let before = b.clone();
    assert_eq!(b.challenge(now, s, &w.a_page), Err("challenge_pending"));
    assert_eq!(b, before);
}

// KIND: candidate-policy G-STORAGE
#[test]
fn session_per_owner_quota_isolates_peers_and_strangers_cannot_consume_it() {
    let now = 10;
    let w = world(now);
    let mut b = SessionReceiver::new(
        receiver_key(),
        w.accepted.clone(),
        Quota::PerOwner(2),
        entropy(1),
    );
    // Strangers with fresh keys replaying bob's captured page are refused before capacity.
    for i in 0..CAPACITY as u64 {
        let s = b.open(public(&key(30 + i)));
        let before = b.clone();
        assert_eq!(b.challenge(now, s, &w.b_page), Err("remote_mismatch"));
        assert_eq!(b, before);
    }
    // A misbehaving contact (alice) forks her own DID to mint many fresh endpoint keys; each
    // fork is a valid generation-1 page with a key she holds. PerOwner caps her at 2.
    let va = w.accepted.views[w.alice.did()].clone();
    for seed in 0..4u64 {
        let (fork, k) = successor(&w.alice, &va, 500 + seed, now);
        let s = b.open(public(&k));
        let before = b.clone();
        let r = b.challenge(now, s, &[fork]);
        if seed < 2 {
            assert!(r.is_ok());
        } else {
            assert_eq!(r, Err("quota"));
            assert_eq!(b, before);
        }
    }
    // Bob is unaffected.
    let sb = b.open(public(&w.b_key));
    let cb = b.challenge(now, sb, &w.b_page).unwrap();
    b.accept(now, sb, &w.b_page, &prove_session(&w.b_key, &cb))
        .unwrap();
    assert_eq!(b.accepted.live[w.bob.did()].endpoint, public(&w.b_key));
}

// =======================================================================================
// Comparison
// =======================================================================================

// KIND: candidate-policy G-STORAGE
#[test]
fn durable_writes_per_honest_update_a_two_b_one() {
    let now = 10;
    let w = world(now);
    let mut a = NonceReceiver::new(receiver_key(), w.accepted.clone(), Quota::GlobalOnly);
    let c = a.issue(now, public(&w.a_key), &w.a_page).unwrap();
    a.accept(now, &w.a_page, c.nonce, &prove_nonce(&w.a_key, &c))
        .unwrap();
    let mut b = SessionReceiver::new(
        receiver_key(),
        w.accepted.clone(),
        Quota::GlobalOnly,
        entropy(1),
    );
    let s = b.open(public(&w.a_key));
    let c = b.challenge(now, s, &w.a_page).unwrap();
    b.accept(now, s, &w.a_page, &prove_session(&w.a_key, &c))
        .unwrap();
    // Same accepted outcome; (a) also wrote the nonce high-water before issuance.
    assert_eq!(a.accepted, b.accepted);
    assert_eq!((a.disk_writes, b.disk_writes), (2, 1));
    // A failed/unanswered attempt: (a) still paid one write, (b) none.
    let _ = a.issue(now, public(&key(42)), &w.b_page).unwrap();
    let s = b.open(public(&key(42)));
    assert_eq!(b.challenge(now, s, &w.b_page), Err("remote_mismatch"));
    assert_eq!((a.disk_writes, b.disk_writes), (3, 1));
}
