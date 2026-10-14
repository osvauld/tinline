//! Packet B2 model: adversarial delivery, replay and replacement-ID collisions against the B1
//! `verify_history_page` / `bind_live` split. Test-only candidate semantics, NOT a production
//! wire format, verifier API, queue, timer or storage design.
//!
//! Invariant under test: invalid input never changes accepted receiver state. Every rejection
//! compares a clone of the WHOLE receiver-side state (accepted view, evidence, live binding,
//! nonces, high-water mark AND the pending queue) before and after.
//!
//! Open gates are modeled, not decided:
//! - byte caps (`TEST_*_BYTE_CAP`, test-only values) as two `candidate-policy G-HISTORY` variants;
//! - pending-queue drop policy as two `candidate-policy G-HISTORY` variants;
//! - forks are only rejected here; detection/handling belongs to packet B3 (G-FORK).
//!
//! Fixture endpoint keys and device IDs are derived from fixed seeds ONLY for deterministic
//! tests. Receivers never compute them; this is NOT a future-endpoint derivation scheme.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use ed25519_dalek::{Signer, SigningKey};
use identity::Identity;

#[path = "support/permit_history.rs"]
mod permit_history;
use permit_history::*;
#[path = "support/fixtures.rs"]
mod fixtures;
use fixtures::*;

/// Per-receiver accepted state (B1 `Receiver` minus checkpoint).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Receiver {
    name: u64,
    view: View,
    evidence: Vec<Permit>,
    revoked: Revoked,
    next_nonce: u64,
    outstanding: BTreeSet<u64>,
    live: Option<LiveBinding>,
}

impl Receiver {
    fn new(name: u64, view: View) -> Self {
        Self {
            name,
            view,
            evidence: Vec::new(),
            revoked: Revoked::new(),
            next_nonce: 0,
            outstanding: BTreeSet::new(),
            live: None,
        }
    }
    fn cursor(&self) -> u64 {
        self.view.generation
    }
    fn accept_page(
        &mut self,
        page: &[Permit],
        now: u64,
        work: &mut Work,
    ) -> Result<(), &'static str> {
        let next = verify_history_page(&self.view, page, now, &self.revoked, work)?;
        let added: Vec<_> = page
            .iter()
            .filter(|p| p.claims.generation > self.view.generation)
            .cloned()
            .collect();
        self.view = next;
        self.evidence.extend(added);
        Ok(())
    }
    fn issue_challenge(&mut self) -> Challenge {
        self.next_nonce += 1;
        self.outstanding.insert(self.next_nonce);
        Challenge {
            receiver: self.name,
            nonce: self.next_nonce,
            head: self.view.head,
        }
    }
    fn bind(
        &mut self,
        newest: &Permit,
        challenge: &Challenge,
        proof: &[u8; 64],
        now: u64,
        work: &mut Work,
    ) -> Result<(), &'static str> {
        if challenge.receiver != self.name || !self.outstanding.contains(&challenge.nonce) {
            return Err("challenge_unknown");
        }
        let binding = bind_live(
            &self.view,
            newest,
            challenge,
            proof,
            now,
            &self.revoked,
            work,
        )?;
        self.outstanding.remove(&challenge.nonce);
        self.live = Some(binding);
        Ok(())
    }
    fn route(&self, now: u64) -> Option<Id> {
        let live = self.live.as_ref()?;
        (live.head == self.view.head
            && now < live.expires_at
            && !self.revoked.contains(&live.head)
            && !self.revoked.contains(&self.view.anchor))
        .then_some(live.endpoint)
    }
}

/// Owner device. Holds ONLY the newest endpoint key; every predecessor key is dropped on rotation.
#[derive(Clone)]
struct Sender {
    owner: Identity,
    seed: u8,
    view: View,
    history: Vec<Permit>,
    pruned_through: u64,
    live_key: Option<SigningKey>,
}

impl Sender {
    fn new(owner: &Identity, seed: u8, view: View) -> Self {
        Self {
            owner: owner.clone(),
            seed,
            view,
            history: Vec::new(),
            pruned_through: 0,
            live_key: None,
        }
    }
    fn rotate(&mut self, now: u64) {
        let generation = self.view.generation + 1;
        let key = fixture_key(self.seed, generation);
        let permit = Permit::issue(
            &self.owner,
            Claims {
                version: 1,
                owner: self.view.owner.clone(),
                audience: self.view.audience.clone(),
                previous_id: self.view.current_id,
                replacement_id: fixture_device_id(self.seed, generation),
                endpoint: key.verifying_key().to_bytes(),
                predecessor: self.view.head,
                generation,
                issued_at: now,
                expires_at: now + INTERVAL + OVERLAP,
            },
        );
        self.view = verify_history_page(
            &self.view,
            std::slice::from_ref(&permit),
            now,
            &Revoked::new(),
            &mut Work::default(),
        )
        .unwrap();
        self.history.push(permit);
        self.live_key = Some(key); // previous key dropped: retired
    }
    fn rotate_until(&mut self, generation: u64) {
        while self.view.generation < generation {
            self.rotate((self.view.generation + 1) * INTERVAL);
        }
    }
    fn newest(&self) -> &Permit {
        self.history.last().unwrap()
    }
    fn permit(&self, generation: u64) -> &Permit {
        self.history
            .iter()
            .find(|p| p.claims.generation == generation)
            .unwrap()
    }
    fn pages_since(&self, cursor: u64) -> Result<Vec<Vec<Permit>>, &'static str> {
        if cursor < self.pruned_through {
            return Err("history_unavailable");
        }
        let rest: Vec<_> = self
            .history
            .iter()
            .filter(|p| p.claims.generation > cursor)
            .cloned()
            .collect();
        Ok(rest.chunks(PAGE).map(|c| c.to_vec()).collect())
    }
    fn prune_through(&mut self, generation: u64) {
        assert!(generation < self.view.generation);
        self.history.retain(|p| p.claims.generation > generation);
        self.pruned_through = generation;
    }
    fn prove(&self, challenge: &Challenge) -> [u8; 64] {
        self.live_key
            .as_ref()
            .unwrap()
            .sign(&challenge.bytes())
            .to_bytes()
    }
}

// =======================================================================================
// B2 model: wire decoding, byte caps, bounded pending queue, recovery, ID registry
// =======================================================================================

/// TEST-ONLY byte caps. NOT product values; G-HISTORY (bounded catch-up) is open.
const TEST_RECORD_BYTE_CAP: usize = 1024;
const TEST_PAGE_BYTE_CAP: usize = 4096;
/// TEST-ONLY per-sender (per path) pending-page bound.
const TEST_PENDING_PAGES: usize = 3;

/// Candidate G-HISTORY byte-cap variants, both checked BEFORE any JSON parse or signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ByteCap {
    /// Each framed record (`4 + claims + 64`) must fit the cap.
    PerRecord(usize),
    /// The page's total framed bytes must fit the cap (records may vary in size).
    PerPage(usize),
}

/// Test-only frame: `u32 BE claims_len || claims JSON || 64-byte signature`.
fn encode_record(p: &Permit) -> Vec<u8> {
    frame(&serde_json::to_vec(&p.claims).unwrap(), &p.signature)
}
fn frame(claims_json: &[u8], signature: &[u8]) -> Vec<u8> {
    [
        &(claims_json.len() as u32).to_be_bytes()[..],
        claims_json,
        signature,
    ]
    .concat()
}
fn encode_page(page: &[Permit]) -> Vec<Vec<u8>> {
    page.iter().map(encode_record).collect()
}

fn decode_record(frame: &[u8], cap: ByteCap, work: &mut Work) -> Result<Permit, &'static str> {
    let len_bytes: [u8; 4] = frame
        .get(..4)
        .ok_or("truncated")?
        .try_into()
        .map_err(|_| "truncated")?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    let need = 4 + len + 64;
    if let ByteCap::PerRecord(max) = cap {
        if need > max {
            return Err("oversized");
        }
    }
    if frame.len() < need {
        return Err("truncated");
    }
    if frame.len() > need {
        return Err("trailing_bytes");
    }
    work.bytes_parsed += len as u64;
    let claims: Claims = serde_json::from_slice(&frame[4..4 + len]).map_err(|_| "malformed")?;
    let signature = frame[4 + len..].try_into().map_err(|_| "truncated")?;
    Ok(Permit { claims, signature })
}

fn decode_page(
    frames: &[Vec<u8>],
    cap: ByteCap,
    work: &mut Work,
) -> Result<Vec<Permit>, &'static str> {
    if frames.is_empty() || frames.len() > PAGE {
        return Err("page_limit");
    }
    if let ByteCap::PerPage(max) = cap {
        if frames.iter().map(Vec::len).sum::<usize>() > max {
            return Err("oversized");
        }
    }
    frames.iter().map(|f| decode_record(f, cap, work)).collect()
}

/// Signature/scope/internal-contiguity check for a page that cannot yet chain to the cursor.
/// Only owner-signed, internally chained pages may occupy pending slots.
fn preverify_future_page(
    view: &View,
    page: &[Permit],
    now: u64,
    work: &mut Work,
) -> Result<(), &'static str> {
    let key = identity::public_key_from_did(&view.owner).ok_or("owner_key")?;
    for (i, permit) in page.iter().enumerate() {
        let c = &permit.claims;
        if c.owner != view.owner {
            return Err("owner_signature");
        }
        work.signature_verifications += 1;
        if !identity::verify(&key, &Permit::bytes(c), &permit.signature) {
            return Err("owner_signature");
        }
        if c.version != 1 || c.audience != view.audience {
            return Err("scope");
        }
        if c.issued_at >= c.expires_at || c.issued_at > now {
            return Err("validity");
        }
        if let Some(prev) = i.checked_sub(1).map(|j| &page[j]) {
            if prev.claims.generation.checked_add(1) != Some(c.generation) {
                return Err("gap");
            }
            if c.predecessor != prev.id() || c.previous_id != prev.claims.replacement_id {
                return Err("predecessor");
            }
        }
    }
    Ok(())
}

/// Candidate G-HISTORY drop policies for a full per-sender pending queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropPolicy {
    /// Keep what is queued; refuse the new arrival (state unchanged).
    RefuseNewArrival,
    /// Evict the oldest-arrived pending page to admit the new arrival.
    EvictOldestArrival,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Outcome {
    Applied { cursor: u64 },
    Duplicate,
    Buffered { pending: usize },
    RefusedFull,
    EvictedOldest { evicted_first_generation: u64 },
    RecoveryRequired { high_water: u64 },
}

/// Receiver-side state for ONE sender path. All fields are compared on every rejection.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Inbox {
    /// `None` = retained view/evidence lost or pruned (storage fixture); cursor unknown.
    receiver: Option<Receiver>,
    /// Persisted anti-rollback mark; never lowered, survives loss of `receiver`.
    high_water: u64,
    high_water_head: Id,
    pending: VecDeque<Vec<Permit>>,
    policy: DropPolicy,
}

impl Inbox {
    fn new(receiver: Receiver, policy: DropPolicy) -> Self {
        Self {
            high_water: receiver.cursor(),
            high_water_head: receiver.view.head,
            receiver: Some(receiver),
            pending: VecDeque::new(),
            policy,
        }
    }
    fn r(&self) -> &Receiver {
        self.receiver.as_ref().unwrap()
    }
    fn r_mut(&mut self) -> &mut Receiver {
        self.receiver.as_mut().unwrap()
    }
    fn recovery(&self) -> Outcome {
        Outcome::RecoveryRequired {
            high_water: self.high_water,
        }
    }
    fn mark(&mut self) {
        let (generation, head) = {
            let r = self.r();
            (r.cursor(), r.view.head)
        };
        if generation > self.high_water {
            self.high_water = generation;
            self.high_water_head = head;
        }
    }

    fn deliver(
        &mut self,
        page: &[Permit],
        now: u64,
        work: &mut Work,
    ) -> Result<Outcome, &'static str> {
        let Some(r) = &self.receiver else {
            // Cursor unknown: never restart from the nominated anchor, never buffer.
            return Ok(self.recovery());
        };
        if page.is_empty() || page.len() > PAGE {
            return Err("page_limit");
        }
        let cursor = r.cursor();
        let first = page[0].claims.generation;
        let last = page[page.len() - 1].claims.generation;
        if last <= cursor {
            // Idempotent only if every record is already retained evidence (by signed-claims ID).
            let known: BTreeSet<Id> = r.evidence.iter().map(Permit::id).collect();
            if page.iter().all(|p| known.contains(&p.id())) {
                return Ok(Outcome::Duplicate);
            }
            // Otherwise let the B1 verifier name it (stale/fork/...); B3 owns fork handling.
            let mut probe = r.clone();
            probe.accept_page(page, now, work)?;
            return Err("stale");
        }
        if first > cursor + 1 {
            preverify_future_page(&r.view, page, now, work)?;
            let ids: Vec<Id> = page.iter().map(Permit::id).collect();
            if self
                .pending
                .iter()
                .any(|q| q.iter().map(Permit::id).eq(ids.iter().copied()))
            {
                return Ok(Outcome::Duplicate);
            }
            if self.pending.len() < TEST_PENDING_PAGES {
                self.pending.push_back(page.to_vec());
                return Ok(Outcome::Buffered {
                    pending: self.pending.len(),
                });
            }
            return Ok(match self.policy {
                DropPolicy::RefuseNewArrival => Outcome::RefusedFull,
                DropPolicy::EvictOldestArrival => {
                    let evicted = self.pending.pop_front().unwrap();
                    self.pending.push_back(page.to_vec());
                    Outcome::EvictedOldest {
                        evicted_first_generation: evicted[0].claims.generation,
                    }
                }
            });
        }
        // Chains (or starts at) the cursor: B1 verifier is atomic per page.
        self.r_mut().accept_page(page, now, work)?;
        self.mark();
        self.drain(now, work);
        Ok(Outcome::Applied {
            cursor: self.r().cursor(),
        })
    }

    /// Apply pending pages that now chain. A pending page that fails verification is discarded
    /// (model choice; whether to keep it as fork evidence is B3/G-FORK).
    fn drain(&mut self, now: u64, work: &mut Work) {
        loop {
            let cursor = self.r().cursor();
            self.pending
                .retain(|p| p[p.len() - 1].claims.generation > cursor);
            let Some(i) = self
                .pending
                .iter()
                .position(|p| p[0].claims.generation == cursor + 1)
            else {
                return;
            };
            let page = self.pending.remove(i).unwrap();
            if self.r_mut().accept_page(&page, now, work).is_ok() {
                self.mark();
            }
        }
    }

    fn deliver_wire(
        &mut self,
        frames: &[Vec<u8>],
        cap: ByteCap,
        now: u64,
        work: &mut Work,
    ) -> Result<Outcome, &'static str> {
        if self.receiver.is_none() {
            return Ok(self.recovery());
        }
        let page = decode_page(frames, cap, work)?;
        self.deliver(&page, now, work)
    }

    /// Receiver-driven catch-up from its own cursor.
    fn catch_up_from(
        &mut self,
        sender: &Sender,
        now: u64,
        work: &mut Work,
    ) -> Result<Outcome, &'static str> {
        let Some(r) = &self.receiver else {
            return Ok(self.recovery());
        };
        let pages = match sender.pages_since(r.cursor()) {
            Ok(pages) => pages,
            Err("history_unavailable") => return Ok(self.recovery()),
            Err(e) => return Err(e),
        };
        for page in &pages {
            self.deliver(page, now, work)?;
        }
        Ok(Outcome::Applied {
            cursor: self.r().cursor(),
        })
    }

    /// Re-establish a cursor after loss. Must reach the high-water mark AND contain the
    /// high-water head (else it could be a rollback or a different branch).
    fn reseed(&mut self, candidate: Receiver) -> Result<(), &'static str> {
        if candidate.cursor() < self.high_water {
            return Err("rollback");
        }
        let has_head = candidate.view.head == self.high_water_head
            || candidate
                .evidence
                .iter()
                .any(|p| p.id() == self.high_water_head);
        if !has_head {
            return Err("high_water_head_missing");
        }
        self.receiver = Some(candidate);
        self.mark();
        Ok(())
    }
}

/// Issue a correctly chained next record with a CHOSEN replacement ID / endpoint (adversarial
/// or colliding fixtures). Signed by `signer`, which may or may not be the path owner.
fn issue_next(
    signer: &Identity,
    view: &View,
    replacement_id: Id,
    endpoint: Id,
    now: u64,
) -> Permit {
    Permit::issue(
        signer,
        Claims {
            version: 1,
            owner: view.owner.clone(),
            audience: view.audience.clone(),
            previous_id: view.current_id,
            replacement_id,
            endpoint,
            predecessor: view.head,
            generation: view.generation + 1,
            issued_at: now,
            expires_at: now + INTERVAL + OVERLAP,
        },
    )
}

fn advance(view: &View, permit: &Permit, now: u64) -> View {
    verify_history_page(
        view,
        std::slice::from_ref(permit),
        now,
        &Revoked::new(),
        &mut Work::default(),
    )
    .unwrap()
}

fn assert_rejected(inbox: &mut Inbox, page: &[Permit], now: u64, expected: &str, work: &mut Work) {
    let snapshot = inbox.clone();
    assert_eq!(inbox.deliver(page, now, work), Err(expected));
    assert_eq!(*inbox, snapshot, "state changed on {expected}");
}

/// Owner with 12 rotations at t=10..120 -> exactly three full pages P1, P2, P3.
fn twelve(alice: &Identity, seed: u8) -> (View, Sender) {
    let initial = View::initial(alice, 1, "private");
    let mut sender = Sender::new(alice, seed, initial.clone());
    sender.rotate_until(12);
    (initial, sender)
}
const T12: u64 = 125; // gen 12 expires at 132

// =======================================================================================
// 1. Dropped, duplicate and reordered pages
// =======================================================================================

// KIND: invariant
#[test]
fn dropped_middle_page_is_buffered_without_touching_accepted_state_then_recovered() {
    let alice = owner(60);
    let (initial, sender) = twelve(&alice, 60);
    let pages = sender.pages_since(0).unwrap();
    assert_eq!(pages.len(), 3);
    let mut inbox = Inbox::new(Receiver::new(7, initial), DropPolicy::RefuseNewArrival);
    let mut work = Work::default();

    assert_eq!(
        inbox.deliver(&pages[0], T12, &mut work),
        Ok(Outcome::Applied { cursor: 4 })
    );
    // P2 dropped in transit; P3 arrives. Accepted state must not move.
    let accepted = inbox.r().clone();
    let high_water = inbox.high_water;
    assert_eq!(
        inbox.deliver(&pages[2], T12, &mut work),
        Ok(Outcome::Buffered { pending: 1 })
    );
    assert_eq!(*inbox.r(), accepted);
    assert_eq!(inbox.high_water, high_water);
    assert_eq!(inbox.r().route(T12), None);
    // Genuine newest permit + genuine newest key still cannot bind: head not verified.
    let c = inbox.r_mut().issue_challenge();
    let snapshot = inbox.clone();
    assert_eq!(
        inbox
            .r_mut()
            .bind(sender.newest(), &c, &sender.prove(&c), T12, &mut work),
        Err("not_head")
    );
    assert_eq!(inbox, snapshot);
    // Re-delivering the buffered page is idempotent.
    assert_eq!(
        inbox.deliver(&pages[2], T12, &mut work),
        Ok(Outcome::Duplicate)
    );
    assert_eq!(inbox, snapshot);

    // Recovery: receiver-driven resend from its cursor (4) sends P2, P3; P2 drains buffered P3,
    // the resent P3 is then a duplicate.
    assert_eq!(
        inbox.catch_up_from(&sender, T12, &mut work),
        Ok(Outcome::Applied { cursor: 12 })
    );
    assert!(inbox.pending.is_empty());
    assert_eq!(inbox.r().view, sender.view);
    assert_eq!(inbox.r().evidence, sender.history);
    assert_eq!(inbox.high_water, 12);
    let c = inbox.r_mut().issue_challenge();
    inbox
        .r_mut()
        .bind(sender.newest(), &c, &sender.prove(&c), T12, &mut work)
        .unwrap();
    assert_eq!(inbox.r().route(T12), Some(sender.newest().claims.endpoint));
}

// KIND: invariant
#[test]
fn all_permutations_of_three_pages_with_duplicates_converge_to_the_in_order_state() {
    let alice = owner(61);
    let (initial, sender) = twelve(&alice, 61);
    let pages = sender.pages_since(0).unwrap();
    let mut work = Work::default();
    let mut reference = Inbox::new(
        Receiver::new(7, initial.clone()),
        DropPolicy::RefuseNewArrival,
    );
    for p in &pages {
        reference.deliver(p, T12, &mut work).unwrap();
    }
    assert_eq!(reference.r().view, sender.view);

    let perms = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for policy in [DropPolicy::RefuseNewArrival, DropPolicy::EvictOldestArrival] {
        for perm in perms {
            let mut inbox = Inbox::new(Receiver::new(7, initial.clone()), policy);
            for &i in &perm {
                // Each page delivered twice back-to-back: the second is always a no-op.
                let before = inbox.clone();
                let outcome = inbox.deliver(&pages[i], T12, &mut work).unwrap();
                match outcome {
                    Outcome::Buffered { .. } => {
                        assert_eq!(inbox.receiver, before.receiver, "{perm:?}");
                        assert_eq!(inbox.high_water, before.high_water, "{perm:?}");
                    }
                    Outcome::Applied { .. } => assert!(inbox.r().cursor() > before.r().cursor()),
                    other => panic!("{perm:?} page {i}: unexpected {other:?}"),
                }
                // Never a route before the head is reached, whatever the order.
                if inbox.r().cursor() < 12 {
                    assert_eq!(inbox.r().route(T12), None);
                }
                let after_first = inbox.clone();
                assert_eq!(
                    inbox.deliver(&pages[i], T12, &mut work),
                    Ok(Outcome::Duplicate),
                    "{perm:?} page {i}"
                );
                assert_eq!(inbox, after_first);
                assert!(inbox.pending.len() <= TEST_PENDING_PAGES);
            }
            assert_eq!(*inbox.r(), *reference.r(), "{policy:?} {perm:?}");
            assert_eq!(inbox.high_water, 12);
            assert!(inbox.pending.is_empty());
        }
    }
}

// =======================================================================================
// 2. Wrong predecessor and splices
// =======================================================================================

// KIND: invariant
#[test]
fn wrong_predecessor_or_previous_id_rejects_the_whole_page_atomically() {
    let alice = owner(62);
    let (initial, sender) = twelve(&alice, 62);
    let mut inbox = Inbox::new(Receiver::new(7, initial), DropPolicy::RefuseNewArrival);
    let mut work = Work::default();
    inbox
        .deliver(&sender.pages_since(0).unwrap()[0], T12, &mut work)
        .unwrap();
    let at4 = inbox.r().view.clone();
    let key5 = fixture_key(62, 5).verifying_key().to_bytes();

    // Owner-signed gen 5 with a predecessor that is not the receiver's head.
    let mut bad_pred = issue_next(&alice, &at4, fixture_device_id(62, 5), key5, 50).claims;
    bad_pred.predecessor = sender.permit(3).id();
    // Owner-signed gen 5 with the right predecessor but a previous_id that is not current.
    let mut bad_prev = issue_next(&alice, &at4, fixture_device_id(62, 5), key5, 50).claims;
    bad_prev.previous_id = sender.permit(3).claims.replacement_id;
    // Same claims as a genuine record but with a predecessor from the anchor (rewind).
    let mut rewind = issue_next(&alice, &at4, fixture_device_id(62, 5), key5, 50).claims;
    rewind.predecessor = at4.anchor;
    for claims in [bad_pred, bad_prev, rewind] {
        let bad = Permit::issue(&alice, claims);
        assert_rejected(
            &mut inbox,
            std::slice::from_ref(&bad),
            T12,
            "predecessor",
            &mut work,
        );
        // A valid record FIRST does not make the page partially accepted.
        let page = [sender.permit(5).clone(), {
            let mut c = sender.permit(6).claims.clone();
            c.predecessor = bad.id();
            Permit::issue(&alice, c)
        }];
        assert_rejected(&mut inbox, &page, T12, "predecessor", &mut work);
    }
    assert_eq!(inbox.r().cursor(), 4);
}

// KIND: invariant
#[test]
fn splices_across_owner_audience_and_device_path_are_rejected_unchanged() {
    let alice = owner(63);
    let bob = owner(64);
    let (initial, sender) = twelve(&alice, 63);
    let mut inbox = Inbox::new(Receiver::new(7, initial), DropPolicy::RefuseNewArrival);
    let mut work = Work::default();
    inbox
        .deliver(&sender.pages_since(0).unwrap()[0], T12, &mut work)
        .unwrap();
    let at4 = inbox.r().view.clone();
    let key = fixture_key(99, 5).verifying_key().to_bytes();

    // Owner splice: Bob's own record (owner = Bob) chained onto Alice's head.
    let mut bob_claims = issue_next(&bob, &at4, [5; 32], key, 50).claims;
    bob_claims.owner = bob.did().into();
    let bob_record = Permit::issue(&bob, bob_claims);
    // Owner splice: Alice-labelled claims signed by Bob.
    let bob_signed_alice = issue_next(&bob, &at4, [5; 32], key, 50);

    // Audience splice: Alice-signed group record whose predecessor/previous_id are the
    // receiver's PRIVATE head.
    let mut group_claims = issue_next(&alice, &at4, [5; 32], key, 50).claims;
    group_claims.audience = "group:g1".into();
    let group_record = Permit::issue(&alice, group_claims);

    // Device-path splice: Alice's sibling path (device 2) record, genuinely owner-signed.
    let sibling_initial = View::initial(&alice, 2, "private");
    let mut sibling = Sender::new(&alice, 163, sibling_initial);
    sibling.rotate_until(5);
    // As-is at the same generation: its predecessor/previous_id belong to the sibling path.
    let sibling_gen5 = sibling.permit(5).clone();
    // Re-pointed predecessor but the sibling's previous_id kept.
    let mut repointed = sibling_gen5.claims.clone();
    repointed.predecessor = at4.head;
    let repointed = Permit::issue(&alice, repointed);

    for (page, expected) in [
        (vec![bob_record], "owner_signature"),
        (vec![bob_signed_alice], "owner_signature"),
        (vec![group_record], "scope"),
        (vec![sibling_gen5.clone()], "predecessor"),
        (vec![repointed], "predecessor"),
        // Mixed page: valid own gen 5, then the sibling's gen 6.
        (
            vec![sender.permit(5).clone(), sibling.permit(5).clone()],
            "fork",
        ),
        (
            vec![sender.permit(5).clone(), {
                let mut s = sibling.clone();
                s.rotate_until(6);
                s.permit(6).clone()
            }],
            "predecessor",
        ),
    ] {
        assert_rejected(&mut inbox, &page, T12, expected, &mut work);
    }
    // A sibling page that would be a FUTURE page is not buffered either: not internally
    // chained to anything we hold and pre-verification still checks owner/scope.
    let foreign_future: Vec<Permit> = vec![{
        let mut c = sender.permit(7).claims.clone();
        c.owner = bob.did().into();
        Permit::issue(&bob, c)
    }];
    assert_rejected(
        &mut inbox,
        &foreign_future,
        T12,
        "owner_signature",
        &mut work,
    );
    assert!(inbox.pending.is_empty());
    assert_eq!(inbox.r().view, at4);
}

// =======================================================================================
// 3. Malformed frames and byte caps
// =======================================================================================

// KIND: invariant
#[test]
fn malformed_frames_and_overlong_pages_are_rejected_before_signature_work() {
    let alice = owner(65);
    let (initial, sender) = twelve(&alice, 65);
    let mut inbox = Inbox::new(Receiver::new(7, initial), DropPolicy::RefuseNewArrival);
    let cap = ByteCap::PerRecord(TEST_RECORD_BYTE_CAP);
    let good = sender.permit(1).clone();
    let good_frame = encode_record(&good);
    let claims_json = serde_json::to_vec(&good.claims).unwrap();
    let with = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut v = serde_json::to_value(&good.claims).unwrap();
        f(&mut v);
        frame(&serde_json::to_vec(&v).unwrap(), &good.signature)
    };

    let cases: Vec<(Vec<Vec<u8>>, &str)> = vec![
        (
            vec![good_frame[..good_frame.len() - 1].to_vec()],
            "truncated",
        ), // 63-byte sig
        (vec![frame(&claims_json, &[])], "truncated"), // no sig
        (vec![good_frame[..3].to_vec()], "truncated"), // no length
        (
            vec![[good_frame.clone(), vec![0]].concat()],
            "trailing_bytes",
        ),
        (vec![with(&|v| v["version"] = 300.into())], "malformed"), // not a u8
        (vec![with(&|v| v["extra"] = 1.into())], "malformed"),     // unknown field
        (vec![frame(b"{not json", &good.signature)], "malformed"),
        (vec![good_frame.clone(); PAGE + 1], "page_limit"),
        (vec![], "page_limit"),
    ];
    for (frames, expected) in cases {
        let mut work = Work::default();
        let snapshot = inbox.clone();
        assert_eq!(
            inbox.deliver_wire(&frames, cap, T12, &mut work),
            Err(expected)
        );
        assert_eq!(inbox, snapshot);
        assert_eq!(work.signature_verifications, 0, "{expected}");
    }
    let mut work = Work::default();
    // Oversized length prefix: rejected without reading or allocating the claimed length.
    let huge = [&u32::MAX.to_be_bytes()[..], &[0u8; 8]].concat();
    for cap in [
        ByteCap::PerRecord(TEST_RECORD_BYTE_CAP),
        ByteCap::PerPage(TEST_PAGE_BYTE_CAP),
    ] {
        let snapshot = inbox.clone();
        let expected = if matches!(cap, ByteCap::PerRecord(_)) {
            "oversized"
        } else {
            "truncated"
        };
        assert_eq!(
            inbox.deliver_wire(&[huge.clone()], cap, T12, &mut work),
            Err(expected)
        );
        assert_eq!(inbox, snapshot);
    }
    assert_eq!(work, Work::default());

    // Unknown but well-formed version, genuinely owner-signed: verifier rejects as scope.
    let mut v2 = good.claims.clone();
    v2.version = 2;
    let v2 = Permit::issue(&alice, v2);
    let snapshot = inbox.clone();
    assert_eq!(
        inbox.deliver_wire(&encode_page(&[v2]), cap, T12, &mut work),
        Err("scope")
    );
    assert_eq!(inbox, snapshot);
    // Page longer than PAGE as decoded permits (bypassing the wire layer) also rejected.
    let five: Vec<Permit> = sender.history[..PAGE + 1].to_vec();
    assert_rejected(&mut inbox, &five, T12, "page_limit", &mut work);
    // Internally gapped future page (gens 5, 6, 8) is NOT buffered.
    let gapped = vec![
        sender.permit(5).clone(),
        sender.permit(6).clone(),
        sender.permit(8).clone(),
    ];
    assert_rejected(&mut inbox, &gapped, T12, "gap", &mut work);
    assert!(inbox.pending.is_empty());

    // Non-canonical wire encoding (pretty JSON) decodes to the same signed claims and ID, so
    // duplicate detection keys on the signed payload, not on wire bytes.
    let pretty = frame(
        &serde_json::to_vec_pretty(&good.claims).unwrap(),
        &good.signature,
    );
    assert_ne!(pretty, good_frame);
    let pcap = ByteCap::PerPage(TEST_PAGE_BYTE_CAP); // pretty JSON exceeds the record cap
    let decoded = decode_page(&[pretty.clone()], pcap, &mut work).unwrap();
    assert_eq!(decoded[0].id(), good.id());
    inbox
        .deliver_wire(&encode_page(&sender.history[..4]), cap, T12, &mut work)
        .unwrap();
    let snapshot = inbox.clone();
    assert_eq!(
        inbox.deliver_wire(&[pretty], pcap, T12, &mut work),
        Ok(Outcome::Duplicate)
    );
    assert_eq!(inbox, snapshot);
}

/// Honest owner with a legitimately long audience string (e.g. a long group identifier).
fn long_audience_sender(alice: &Identity, seed: u8, generations: u64) -> (View, Sender) {
    let audience = format!("group:{}", "x".repeat(600));
    let initial = View::initial(alice, 50, &audience);
    let mut sender = Sender::new(alice, seed, initial.clone());
    sender.rotate_until(generations);
    (initial, sender)
}

// KIND: candidate-policy G-HISTORY
#[test]
fn per_record_byte_cap_rejects_unparsed_but_makes_long_honest_records_unrepresentable() {
    let cap = ByteCap::PerRecord(TEST_RECORD_BYTE_CAP);
    let alice = owner(66);
    let (initial, sender) = twelve(&alice, 66);
    let normal = encode_record(sender.permit(1));
    assert!(normal.len() <= TEST_RECORD_BYTE_CAP);
    let mut work = Work::default();
    let mut inbox = Inbox::new(Receiver::new(7, initial), DropPolicy::RefuseNewArrival);
    for page in sender.pages_since(0).unwrap() {
        inbox
            .deliver_wire(&encode_page(&page), cap, T12, &mut work)
            .unwrap();
    }
    assert_eq!(inbox.r().view, sender.view);

    // Adversarial junk just over the cap: rejected with ZERO bytes parsed.
    let junk = frame(&vec![b' '; TEST_RECORD_BYTE_CAP], &[0; 64]);
    let mut w = Work::default();
    let snapshot = inbox.clone();
    assert_eq!(
        inbox.deliver_wire(&[junk], cap, T12, &mut w),
        Err("oversized")
    );
    assert_eq!(inbox, snapshot);
    assert_eq!(w, Work::default());

    // Honest long-audience path: every record is over the per-record cap -> never deliverable.
    let (long_initial, long) = long_audience_sender(&alice, 67, 4);
    let long_len = encode_record(long.permit(1)).len();
    assert!(long_len > TEST_RECORD_BYTE_CAP, "{long_len}");
    let mut long_inbox = Inbox::new(Receiver::new(8, long_initial), DropPolicy::RefuseNewArrival);
    let snapshot = long_inbox.clone();
    for page in long.pages_since(0).unwrap() {
        for single in page.chunks(1) {
            assert_eq!(
                long_inbox.deliver_wire(&encode_page(single), cap, 45, &mut w),
                Err("oversized")
            );
        }
    }
    assert_eq!(long_inbox, snapshot);
    // => a per-record cap is also a cap on audience/owner string length (schema question).
}

// KIND: candidate-policy G-HISTORY
#[test]
fn per_page_byte_cap_bounds_parse_work_and_forces_byte_aware_paging() {
    let cap = ByteCap::PerPage(TEST_PAGE_BYTE_CAP);
    let alice = owner(68);
    let (long_initial, long) = long_audience_sender(&alice, 68, 12);
    let long_len = encode_record(long.permit(1)).len();
    assert!(long_len > TEST_RECORD_BYTE_CAP && long_len < TEST_PAGE_BYTE_CAP);
    assert!(PAGE * long_len > TEST_PAGE_BYTE_CAP);
    let now = 125;
    let mut work = Work::default();

    // Count-based paging (PAGE records): every full page is over the byte cap.
    let mut inbox = Inbox::new(
        Receiver::new(7, long_initial.clone()),
        DropPolicy::RefuseNewArrival,
    );
    for page in long.pages_since(0).unwrap() {
        let snapshot = inbox.clone();
        assert_eq!(
            inbox.deliver_wire(&encode_page(&page), cap, now, &mut work),
            Err("oversized")
        );
        assert_eq!(inbox, snapshot);
    }
    // Byte-aware paging (fit both PAGE and the byte cap) delivers the same honest history.
    let mut pages: Vec<Vec<Permit>> = Vec::new();
    for p in &long.history {
        let fits = pages.last().is_some_and(|last: &Vec<Permit>| {
            last.len() < PAGE
                && last.iter().map(|q| encode_record(q).len()).sum::<usize>()
                    + encode_record(p).len()
                    <= TEST_PAGE_BYTE_CAP
        });
        if fits {
            pages.last_mut().unwrap().push(p.clone());
        } else {
            pages.push(vec![p.clone()]);
        }
    }
    assert!(pages.len() > long.history.len().div_ceil(PAGE));
    for page in &pages {
        inbox
            .deliver_wire(&encode_page(page), cap, now, &mut work)
            .unwrap();
    }
    assert_eq!(inbox.r().view, long.view);

    // A single adversarial frame under the page cap IS parsed (bounded by the cap) and then
    // rejected; contrast with the per-record variant, which parses nothing.
    let junk = frame(&vec![b' '; TEST_PAGE_BYTE_CAP - 100], &[0; 64]);
    let mut w = Work::default();
    let snapshot = inbox.clone();
    assert_eq!(
        inbox.deliver_wire(&[junk.clone()], cap, now, &mut w),
        Err("malformed")
    );
    assert_eq!(inbox, snapshot);
    assert!(w.bytes_parsed > 0 && w.bytes_parsed <= TEST_PAGE_BYTE_CAP as u64);
    assert_eq!(w.signature_verifications, 0);
    let mut w2 = Work::default();
    assert_eq!(
        inbox.deliver_wire(
            &[junk],
            ByteCap::PerRecord(TEST_RECORD_BYTE_CAP),
            now,
            &mut w2
        ),
        Err("oversized")
    );
    assert_eq!(w2.bytes_parsed, 0);
}

// =======================================================================================
// 4. Queue saturation: two candidate drop policies
// =======================================================================================

struct QueueTrace {
    outcomes: Vec<Outcome>,
    max_pending: usize,
    cursor_after_gap_filled: u64,
    pending_after_gap_filled: Vec<u64>,
    final_view_matches: bool,
}

/// 40 honest rotations (10 pages). P1 is lost; P2..P10 arrive, then a mnemonic-holding clone
/// floods 3 owner-signed future fork pages, then a non-owner floods 5 forged pages. Then P1
/// arrives, then the receiver resends from its cursor.
fn queue_scenario(policy: DropPolicy) -> QueueTrace {
    let alice = owner(69);
    let mallory = owner(70);
    let initial = View::initial(&alice, 1, "private");
    let mut sender = Sender::new(&alice, 69, initial.clone());
    sender.rotate_until(40);
    // Clone holds the shared mnemonic (G-AUTHORITY): same owner signature, different branch.
    let mut clone = Sender::new(&alice, 69, initial.clone());
    clone.rotate_until(20);
    clone.seed = 169;
    clone.rotate_until(40);
    let now = 405;
    let pages = sender.pages_since(0).unwrap();
    assert_eq!(pages.len(), 10);
    let mut inbox = Inbox::new(Receiver::new(7, initial), policy);
    let mut work = Work::default();
    let mut outcomes = Vec::new();
    let mut max_pending = 0;

    for page in &pages[1..] {
        let accepted = inbox.receiver.clone();
        let before = inbox.clone();
        let o = inbox.deliver(page, now, &mut work).unwrap();
        assert_eq!(inbox.receiver, accepted); // buffering never moves accepted state
        if o == Outcome::RefusedFull {
            assert_eq!(inbox, before); // refusal leaves EVERYTHING unchanged
        }
        max_pending = max_pending.max(inbox.pending.len());
        outcomes.push(o);
    }
    for page in &clone.pages_since(20).unwrap()[2..] {
        let before = inbox.clone();
        let o = inbox.deliver(page, now, &mut work).unwrap();
        assert_eq!(inbox.receiver, before.receiver);
        max_pending = max_pending.max(inbox.pending.len());
        outcomes.push(o);
    }
    // Non-owner forgeries cannot occupy pending slots under either policy.
    for page in &pages[1..6] {
        let forged: Vec<Permit> = page
            .iter()
            .map(|p| Permit::issue(&mallory, p.claims.clone()))
            .collect();
        assert_rejected(&mut inbox, &forged, now, "owner_signature", &mut work);
    }
    assert!(max_pending <= TEST_PENDING_PAGES);

    let o = inbox.deliver(&pages[0], now, &mut work).unwrap();
    assert!(matches!(o, Outcome::Applied { .. }));
    let cursor_after_gap_filled = inbox.r().cursor();
    let pending_after_gap_filled = inbox
        .pending
        .iter()
        .map(|p| p[0].claims.generation)
        .collect();
    // Explicit recovery: receiver-driven resend from its own cursor converges in both variants.
    inbox.catch_up_from(&sender, now, &mut work).unwrap();
    assert!(inbox.pending.len() <= TEST_PENDING_PAGES);
    QueueTrace {
        outcomes,
        max_pending,
        cursor_after_gap_filled,
        pending_after_gap_filled,
        final_view_matches: inbox.r().view == sender.view && inbox.high_water == 40,
    }
}

// KIND: candidate-policy G-HISTORY
#[test]
fn full_pending_queue_refuse_new_arrival_is_bounded_and_keeps_the_nearest_pages() {
    let t = queue_scenario(DropPolicy::RefuseNewArrival);
    assert_eq!(t.max_pending, TEST_PENDING_PAGES);
    // P2, P3, P4 buffered; P5..P10 and all clone pages refused (state unchanged each time).
    assert_eq!(
        &t.outcomes[..3],
        &[
            Outcome::Buffered { pending: 1 },
            Outcome::Buffered { pending: 2 },
            Outcome::Buffered { pending: 3 }
        ]
    );
    assert!(t.outcomes[3..].iter().all(|o| *o == Outcome::RefusedFull));
    // Filling P1 drains P2..P4 immediately: the nearest pages were kept.
    assert_eq!(t.cursor_after_gap_filled, 16);
    assert!(t.pending_after_gap_filled.is_empty());
    assert!(t.final_view_matches);
}

// KIND: candidate-policy G-HISTORY
#[test]
fn full_pending_queue_evict_oldest_arrival_is_bounded_but_a_clone_flood_evicts_honest_pages() {
    let t = queue_scenario(DropPolicy::EvictOldestArrival);
    assert_eq!(t.max_pending, TEST_PENDING_PAGES);
    assert_eq!(t.outcomes.len(), 9 + 3);
    assert!(
        t.outcomes[3..]
            .iter()
            .all(|o| matches!(o, Outcome::EvictedOldest { .. }))
    );
    // The three most recent arrivals are the clone's fork pages (gens 29, 33, 37): every
    // honest pending page was evicted, so filling P1 advances only to gen 4.
    assert_eq!(t.cursor_after_gap_filled, 4);
    assert_eq!(t.pending_after_gap_filled, vec![29, 33, 37]);
    // Resend from cursor 4 converges; fork pages fail to chain on drain and are discarded
    // (B3/G-FORK decides whether they should be kept as evidence instead).
    assert!(t.final_view_matches);
}

// =======================================================================================
// 5. Missing retained history -> explicit RecoveryRequired, high-water kept
// =======================================================================================

// KIND: invariant
#[test]
fn lost_or_unservable_cursor_yields_recovery_required_and_never_lowers_high_water() {
    let alice = owner(71);
    let (initial, mut sender) = twelve(&alice, 71);
    let mut work = Work::default();
    let mut inbox = Inbox::new(
        Receiver::new(7, initial.clone()),
        DropPolicy::RefuseNewArrival,
    );
    inbox.catch_up_from(&sender, T12, &mut work).unwrap();
    assert_eq!(inbox.high_water, 12);
    let head12 = inbox.r().view.head;

    // (a) Receiver lost its retained view/evidence (storage fixture); high-water persisted.
    let mut lost = inbox.clone();
    lost.receiver = None;
    let snapshot = lost.clone();
    let genesis_page = sender.pages_since(0).unwrap()[0].clone();
    sender.rotate(130);
    let next_page = sender.pages_since(12).unwrap()[0].clone();
    for page in [&genesis_page, &next_page] {
        assert_eq!(
            lost.deliver(page, 131, &mut work),
            Ok(Outcome::RecoveryRequired { high_water: 12 })
        );
        assert_eq!(lost, snapshot); // nothing buffered, nothing applied, mark kept
    }
    assert_eq!(
        lost.deliver_wire(
            &encode_page(&next_page),
            ByteCap::PerPage(TEST_PAGE_BYTE_CAP),
            131,
            &mut work
        ),
        Ok(Outcome::RecoveryRequired { high_water: 12 })
    );
    assert_eq!(
        lost.catch_up_from(&sender, 131, &mut work),
        Ok(Outcome::RecoveryRequired { high_water: 12 })
    );
    assert_eq!(lost, snapshot);
    // Restarting from the nominated anchor (gen 0) is a rollback and is refused.
    assert_eq!(
        lost.reseed(Receiver::new(7, initial.clone())),
        Err("rollback")
    );
    assert_eq!(lost, snapshot);
    // A different branch past the mark (mnemonic clone forking at gen 10) lacks the
    // high-water head and is refused.
    let mut clone = Sender::new(&alice, 71, initial.clone());
    clone.rotate_until(9);
    clone.seed = 171;
    clone.rotate_until(13);
    let mut other = Receiver::new(7, initial.clone());
    for p in clone.pages_since(0).unwrap() {
        other.accept_page(&p, 131, &mut work).unwrap();
    }
    assert_eq!(lost.reseed(other), Err("high_water_head_missing"));
    assert_eq!(lost, snapshot);
    // One explicit recovery: full strict re-verification from the anchor that reaches and
    // contains the high-water head (only possible while the sender still has the history).
    let mut fresh = Receiver::new(7, initial.clone());
    for p in sender.pages_since(0).unwrap() {
        fresh.accept_page(&p, 131, &mut work).unwrap();
    }
    lost.reseed(fresh).unwrap();
    assert_eq!(lost.high_water, 13);
    assert!(lost.r().evidence.iter().any(|p| p.id() == head12));

    // (b) Sender pruned below the receiver's cursor: explicit outcome, not a silent gap.
    let mut behind = Inbox::new(Receiver::new(8, initial), DropPolicy::RefuseNewArrival);
    behind
        .deliver(&sender.pages_since(0).unwrap()[0], 131, &mut work)
        .unwrap();
    let mut pruned = sender.clone();
    pruned.prune_through(8);
    let snapshot = behind.clone();
    assert_eq!(
        behind.catch_up_from(&pruned, 131, &mut work),
        Ok(Outcome::RecoveryRequired { high_water: 4 })
    );
    assert_eq!(behind, snapshot);
    // Pushing the retained tail anyway only fills the bounded queue; nothing is applied.
    for page in pruned.pages_since(8).unwrap() {
        let accepted = behind.receiver.clone();
        behind.deliver(&page, 131, &mut work).unwrap();
        assert_eq!(behind.receiver, accepted);
    }
    assert_eq!(behind.high_water, 4);
    assert!(behind.pending.len() <= TEST_PENDING_PAGES);
}

// =======================================================================================
// 6. Replayed expired historical permits
// =======================================================================================

// KIND: invariant
#[test]
fn replayed_expired_historical_permit_cannot_create_a_live_binding_or_route() {
    let alice = owner(72);
    let initial = View::initial(&alice, 1, "private");
    let mut sender = Sender::new(&alice, 72, initial.clone());
    sender.rotate_until(13);
    let now = 135; // gens 1..=12 expired, gen 13 current until 142
    let mut work = Work::default();

    // Receiver behind at gen 11. Attacker kept the retired gen-12 key and replays gen 12.
    let mut inbox = Inbox::new(
        Receiver::new(7, initial.clone()),
        DropPolicy::RefuseNewArrival,
    );
    for chunk in sender.history[..11].chunks(PAGE) {
        inbox.deliver(chunk, now, &mut work).unwrap();
    }
    let gen12 = sender.permit(12).clone();
    assert!(gen12.claims.expires_at <= now);
    // Expired history is still valid history: knowledge advances...
    assert_eq!(
        inbox.deliver(std::slice::from_ref(&gen12), now, &mut work),
        Ok(Outcome::Applied { cursor: 12 })
    );
    // ...but no live binding with the retired key and no route.
    let c = inbox.r_mut().issue_challenge();
    let retired_proof = fixture_key(72, 12).sign(&c.bytes()).to_bytes();
    let snapshot = inbox.clone();
    assert_eq!(
        inbox
            .r_mut()
            .bind(&gen12, &c, &retired_proof, now, &mut work),
        Err("expired")
    );
    assert_eq!(inbox, snapshot);
    assert_eq!(inbox.r().route(now), None);
    // Retired endpoint stays in evidence (usable for HISTORICAL signature checks, cf. A1 O2)
    // without ever being a route.
    assert!(
        inbox
            .r()
            .evidence
            .iter()
            .any(|p| p.claims.endpoint == gen12.claims.endpoint)
    );

    // Honest head arrives and binds.
    inbox
        .deliver(std::slice::from_ref(sender.newest()), now, &mut work)
        .unwrap();
    let c = inbox.r_mut().issue_challenge();
    let honest_proof = sender.prove(&c);
    inbox
        .r_mut()
        .bind(sender.newest(), &c, &honest_proof, now, &mut work)
        .unwrap();
    let bound = inbox.clone();
    // Replays after binding: old pages, the expired permit, the recorded (challenge, proof).
    for page in sender.pages_since(0).unwrap() {
        assert_eq!(inbox.deliver(&page, now, &mut work), Ok(Outcome::Duplicate));
    }
    assert_eq!(
        inbox.deliver(std::slice::from_ref(&gen12), now, &mut work),
        Ok(Outcome::Duplicate)
    );
    assert_eq!(
        inbox
            .r_mut()
            .bind(sender.newest(), &c, &honest_proof, now, &mut work),
        Err("challenge_unknown")
    );
    assert_eq!(inbox, bound);
    assert_eq!(inbox.r().route(now), Some(sender.newest().claims.endpoint));

    // Sender goes silent; head expires. Replaying the (now expired) head permit with the
    // genuine key and a fresh challenge cannot renew the route.
    let later = sender.newest().claims.expires_at;
    assert_eq!(inbox.r().route(later), None);
    let c = inbox.r_mut().issue_challenge();
    let snapshot = inbox.clone();
    assert_eq!(
        inbox
            .r_mut()
            .bind(sender.newest(), &c, &sender.prove(&c), later, &mut work),
        Err("expired")
    );
    assert_eq!(inbox, snapshot);
    assert_eq!(inbox.r().route(later), None);

    // A fresh receiver replaying the whole (fully expired) history: history only, no route.
    let mut fresh = Inbox::new(Receiver::new(8, initial), DropPolicy::RefuseNewArrival);
    fresh
        .catch_up_from(&sender, later + 100, &mut work)
        .unwrap();
    assert_eq!(fresh.r().view, sender.view);
    assert_eq!(fresh.r().route(later + 100), None);
    assert!(fresh.r().live.is_none());
}

// =======================================================================================
// 7. Group audience never needs private ancestors
// =======================================================================================

// KIND: invariant
#[test]
fn group_audience_verifies_without_private_ancestors_and_rejects_private_permits() {
    let alice = owner(73);
    let group = "group:g1";
    let private_initial = View::initial(&alice, 1, "private");
    let mut private = Sender::new(&alice, 73, private_initial.clone());
    private.rotate_until(14);
    let group_initial = View::initial(&alice, 50, group);
    let mut grp = Sender::new(&alice, 173, group_initial.clone());
    grp.rotate_until(9);
    let now = 95;
    let mut work = Work::default();

    // The group receiver holds ONLY the group nomination; no private state at all.
    let mut inbox = Inbox::new(
        Receiver::new(7, group_initial),
        DropPolicy::RefuseNewArrival,
    );
    inbox.catch_up_from(&grp, now, &mut work).unwrap();
    assert_eq!(inbox.r().view, grp.view);
    assert_eq!(work.signature_verifications, 9); // group records only
    let dump = format!("{inbox:?}");
    for p in &private.history {
        for secret in [p.id(), p.claims.endpoint, p.claims.replacement_id] {
            assert!(!dump.contains(&format!("{secret:?}")));
        }
    }
    assert!(!dump.contains(&format!("{:?}", private_initial.anchor)));

    // Private permits presented on the group path.
    let at9 = inbox.r().view.clone();
    let key = fixture_key(99, 10).verifying_key().to_bytes();
    // (i) private record chained onto the group head (audience still private) -> scope.
    let mut chained = issue_next(&alice, &at9, [10; 32], key, now).claims;
    chained.audience = "private".into();
    let chained = Permit::issue(&alice, chained);
    // (ii) private permit relabelled as group without re-signing -> signature fails.
    let mut relabelled = private.permit(1).clone();
    relabelled.claims.audience = group.into();
    // (iii) group-scoped record whose predecessor is a private record -> predecessor.
    let mut cross = issue_next(&alice, &at9, [10; 32], key, now).claims;
    cross.predecessor = private.newest().id();
    let cross = Permit::issue(&alice, cross);
    // (iv) private page at FUTURE generations (13, 14) is not buffered: pre-verification
    // checks audience before a page may occupy a pending slot.
    let private_future = private.pages_since(12).unwrap()[0].clone();
    assert_eq!(private_future[0].claims.generation, 13);
    for (page, expected) in [
        (vec![chained], "scope"),
        (vec![relabelled], "owner_signature"),
        (vec![cross], "predecessor"),
        (vec![private.permit(1).clone()], "scope"),
        (private_future.clone(), "scope"),
    ] {
        assert_rejected(&mut inbox, &page, now, expected, &mut work);
    }
    assert!(inbox.pending.is_empty());
    // Symmetric: a private receiver rejects group permits.
    let mut priv_inbox = Inbox::new(
        Receiver::new(8, private_initial),
        DropPolicy::RefuseNewArrival,
    );
    assert_rejected(
        &mut priv_inbox,
        std::slice::from_ref(grp.permit(1)),
        now,
        "scope",
        &mut work,
    );
}

// =======================================================================================
// 8. Replacement-ID collisions
// =======================================================================================

/// Path-local ID check that consults only THIS path's retained evidence.
fn own_history_ids(r: &Receiver) -> BTreeSet<Id> {
    r.evidence
        .iter()
        .flat_map(|p| [p.claims.previous_id, p.claims.replacement_id])
        .collect()
}

fn path_local_with_evidence(
    r: &mut Receiver,
    page: &[Permit],
    now: u64,
    work: &mut Work,
) -> Result<(), &'static str> {
    let mut seen = own_history_ids(r);
    seen.insert(r.view.current_id);
    let mut candidate = r.clone();
    candidate.accept_page(page, now, work)?;
    for p in page.iter().filter(|p| p.claims.generation > r.cursor()) {
        if !seen.insert(p.claims.replacement_id) {
            return Err("own_historical_id_reuse");
        }
    }
    *r = candidate;
    Ok(())
}

/// Registry-level state for one (owner, audience): every path and every ID ever seen.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Registry {
    paths: BTreeMap<Id, Receiver>, // anchor -> receiver
    ids: BTreeMap<Id, (Id, u64)>,  // device ID -> (anchor, generation) first seen
}

impl Registry {
    fn new(views: &[View]) -> Self {
        let paths = views
            .iter()
            .map(|v| (v.anchor, Receiver::new(7, v.clone())))
            .collect();
        let ids = views
            .iter()
            .map(|v| (v.current_id, (v.anchor, 0)))
            .collect();
        Self { paths, ids }
    }
    fn deliver(
        &mut self,
        anchor: &Id,
        page: &[Permit],
        now: u64,
        work: &mut Work,
    ) -> Result<(), &'static str> {
        let r = self.paths.get(anchor).ok_or("unknown_path")?;
        let mut candidate = r.clone();
        candidate.accept_page(page, now, work)?;
        let mut ids = self.ids.clone();
        for p in page.iter().filter(|p| p.claims.generation > r.cursor()) {
            let id = p.claims.replacement_id;
            if self
                .paths
                .iter()
                .any(|(a, other)| a != anchor && other.view.current_id == id)
            {
                return Err("sibling_current_id_collision");
            }
            match ids.get(&id) {
                Some((a, _)) if a != anchor => return Err("sibling_historical_id_collision"),
                Some(_) => return Err("own_historical_id_reuse"),
                None => {
                    ids.insert(id, (*anchor, p.claims.generation));
                }
            }
        }
        self.paths.insert(*anchor, candidate);
        self.ids = ids;
        Ok(())
    }
}

/// Mirror of `HostDirectory::install` in crates/group-sim/src/reachability.rs:49-72 (read-only
/// copy): checks first device ID and owner/audience of every record; trusts the adapter for
/// continuity.
fn group_mock_install(
    initial_device: Id,
    owner: &str,
    group: &str,
    path: &[(String, Id, String)],
) -> bool {
    let Some(first) = path.first() else {
        return false;
    };
    first.1 == initial_device && path.iter().all(|(o, _, a)| o == owner && a == group)
}

struct CollisionFixture {
    alice: Identity,
    ours: View,    // our path at gen 2: ids [1;32] -> o1 -> o2
    sibling: View, // sibling at gen 2: ids [2;32] -> s1 -> s2
    our_pages: Vec<Permit>,
    sibling_pages: Vec<Permit>,
    o1: Id,
    s1: Id,
    s2: Id,
}

fn collision_fixture() -> CollisionFixture {
    let alice = owner(74);
    let mut ours = View::initial(&alice, 1, "private");
    let mut sibling = View::initial(&alice, 2, "private");
    let (o1, o2, s1, s2) = ([11; 32], [12; 32], [21; 32], [22; 32]);
    let mut our_pages = Vec::new();
    let mut sibling_pages = Vec::new();
    for (g, (o, s)) in [(o1, s1), (o2, s2)].into_iter().enumerate() {
        let t = (g as u64 + 1) * INTERVAL;
        let po = issue_next(
            &alice,
            &ours,
            o,
            fixture_key(74, g as u64 + 1).verifying_key().to_bytes(),
            t,
        );
        let ps = issue_next(
            &alice,
            &sibling,
            s,
            fixture_key(174, g as u64 + 1).verifying_key().to_bytes(),
            t,
        );
        ours = advance(&ours, &po, t);
        sibling = advance(&sibling, &ps, t);
        our_pages.push(po);
        sibling_pages.push(ps);
    }
    CollisionFixture {
        alice,
        ours,
        sibling,
        our_pages,
        sibling_pages,
        o1,
        s1,
        s2,
    }
}

// KIND: limitation
#[test]
fn path_local_verification_cannot_see_sibling_or_pruned_id_collisions() {
    let f = collision_fixture();
    let now = 35;
    let mut work = Work::default();
    let initial = View::initial(&f.alice, 1, "private");
    let mut base = Receiver::new(7, initial.clone());
    base.accept_page(&f.our_pages, now, &mut work).unwrap();
    assert_eq!(base.view, f.ours);
    let endpoint = fixture_key(74, 3).verifying_key().to_bytes();
    let next_with = |id: Id| issue_next(&f.alice, &f.ours, id, endpoint, 30);

    // B1 path-local verifier: ALL of these are accepted (only `unchanged_id` is checked).
    for id in [f.s2, f.s1, [2; 32], f.o1, [1; 32]] {
        let mut r = base.clone();
        r.accept_page(&[next_with(id)], now, &mut work).unwrap();
        assert_eq!(r.view.current_id, id);
    }
    // Path-local + this path's retained evidence: own history detected, siblings NOT.
    for (id, expected) in [
        (f.s2, Ok(())),
        (f.s1, Ok(())),
        (f.o1, Err("own_historical_id_reuse")),
        ([1; 32], Err("own_historical_id_reuse")),
    ] {
        let mut r = base.clone();
        let result = path_local_with_evidence(&mut r, &[next_with(id)], now, &mut work);
        assert_eq!(result, expected);
        if result.is_err() {
            assert_eq!(r, base);
        }
    }
    // If retained evidence was pruned (e.g. checkpoint/bounded-history option), own reuse of
    // an ID from before the pruned range is invisible again.
    let mut pruned = base.clone();
    pruned.evidence.clear();
    assert_eq!(
        path_local_with_evidence(&mut pruned, &[next_with(f.o1)], now, &mut work),
        Ok(())
    );
    // Endpoint reuse across paths is likewise invisible path-locally.
    let sibling_endpoint = f.sibling_pages[1].claims.endpoint;
    let mut r = base.clone();
    r.accept_page(
        &[issue_next(
            &f.alice,
            &f.ours,
            [13; 32],
            sibling_endpoint,
            30,
        )],
        now,
        &mut work,
    )
    .unwrap();
    assert_eq!(r.view.endpoint, f.sibling.endpoint);

    // Group mock (D2 §5): a `[nominated, sibling]` path is installed because continuity is
    // delegated to the adapter and the mock path carries no predecessor/previous_id.
    let path = vec![
        (f.alice.did().to_string(), [1; 32], "group:g1".to_string()),
        (f.alice.did().to_string(), f.s2, "group:g1".to_string()),
    ];
    assert!(group_mock_install(
        [1; 32],
        f.alice.did(),
        "group:g1",
        &path
    ));
}

// KIND: invariant
#[test]
fn registry_level_id_index_rejects_every_collision_without_touching_any_path() {
    let f = collision_fixture();
    let now = 35;
    let mut work = Work::default();
    let ours0 = View::initial(&f.alice, 1, "private");
    let sib0 = View::initial(&f.alice, 2, "private");
    let mut reg = Registry::new(&[ours0.clone(), sib0.clone()]);
    reg.deliver(&ours0.anchor, &f.our_pages, now, &mut work)
        .unwrap();
    reg.deliver(&sib0.anchor, &f.sibling_pages, now, &mut work)
        .unwrap();
    assert_eq!(reg.paths[&sib0.anchor].view, f.sibling);
    let endpoint = fixture_key(74, 3).verifying_key().to_bytes();
    let next_with = |id: Id| issue_next(&f.alice, &f.ours, id, endpoint, 30);

    for (id, expected) in [
        (f.s2, "sibling_current_id_collision"),
        (f.s1, "sibling_historical_id_collision"),
        ([2; 32], "sibling_historical_id_collision"),
        (f.o1, "own_historical_id_reuse"),
        ([1; 32], "own_historical_id_reuse"),
    ] {
        let snapshot = reg.clone();
        assert_eq!(
            reg.deliver(&ours0.anchor, &[next_with(id)], now, &mut work),
            Err(expected)
        );
        assert_eq!(reg, snapshot); // neither our path nor the sibling path changed
    }
    // Duplicate ID WITHIN one page (X -> Y -> X over three new records) also caught.
    let p3 = next_with([13; 32]);
    let at3 = advance(&f.ours, &p3, 30);
    let p4 = issue_next(&f.alice, &at3, [14; 32], endpoint, 31);
    let at4 = advance(&at3, &p4, 31);
    let p5 = issue_next(&f.alice, &at4, [13; 32], endpoint, 32);
    let snapshot = reg.clone();
    assert_eq!(
        reg.deliver(&ours0.anchor, &[p3.clone(), p4, p5], now, &mut work),
        Err("own_historical_id_reuse")
    );
    assert_eq!(reg, snapshot);
    // Fresh ID passes; the index grows by one entry per rotation (bounded only by history).
    reg.deliver(&ours0.anchor, &[p3], now, &mut work).unwrap();
    assert_eq!(reg.ids.len(), 2 + 2 + 2 + 1);
    assert_eq!(reg.paths[&sib0.anchor].view, f.sibling);
}
