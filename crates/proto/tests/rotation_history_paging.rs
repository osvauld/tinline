//! Packet B1 model: multi-page succession history kept SEPARATE from live possession of the
//! newest endpoint key. Test-only candidate semantics, NOT a production wire format, verifier
//! API, timer or storage design.
//!
//! Two operations:
//! - `verify_history_page(view, page)`: owner signatures, predecessors, scope, generation order.
//!   Never consults an endpoint key; expired records may advance historical knowledge.
//! - `bind_live(view, newest, challenge, proof, now)`: possession of the NEWEST verified head's
//!   endpoint key only. Only a live binding to the current verified head yields a dialable route.
//!
//! G-HISTORY is OPEN. Strict full history and an owner-signed checkpoint are both modeled as
//! `candidate-policy G-HISTORY` tests; neither is selected here.
//!
//! Fixture endpoint keys and device IDs are derived from fixed seeds ONLY for deterministic
//! tests. Receivers never compute them; this is NOT a future-endpoint derivation scheme.
use std::collections::BTreeSet;

use ed25519_dalek::{Signer, SigningKey};
use identity::Identity;

#[path = "support/permit_history.rs"]
mod permit_history;
use permit_history::*;
#[path = "support/fixtures.rs"]
mod fixtures;
use fixtures::*;

/// Per-receiver state. Snapshot-compared on every rejection.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Receiver {
    name: u64,
    view: View,
    // Retained historical evidence (permits accepted after the initial anchor).
    evidence: Vec<Permit>,
    checkpoint: Option<Checkpoint>,
    revoked: Revoked,
    // Allocation high-water; outstanding set. Persistence semantics belong to C3/C2.
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
            checkpoint: None,
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
    fn accept_checkpoint(
        &mut self,
        cp: &Checkpoint,
        head: &Permit,
        now: u64,
        work: &mut Work,
    ) -> Result<(), &'static str> {
        let next = verify_checkpoint(&self.view, cp, head, now, &self.revoked, work)?;
        if next != self.view {
            self.view = next;
            self.evidence.push(head.clone());
            self.checkpoint = Some(cp.clone());
        }
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
    /// Dialable route requires a live binding to the CURRENT verified head, unexpired.
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
    /// Strict paging from a receiver-reported cursor (cursor exchange itself is not modeled).
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
    fn checkpoint_at(&self, generation: u64) -> (Checkpoint, Permit) {
        let head = self.permit(generation).clone();
        let claims = CheckpointClaims {
            version: 1,
            owner: self.view.owner.clone(),
            audience: self.view.audience.clone(),
            anchor: self.view.anchor,
            generation,
            head: head.id(),
        };
        let signature = self.owner.sign(&Checkpoint::bytes(&claims));
        (Checkpoint { claims, signature }, head)
    }
    fn prove(&self, challenge: &Challenge) -> [u8; 64] {
        self.live_key
            .as_ref()
            .unwrap()
            .sign(&challenge.bytes())
            .to_bytes()
    }
}

fn catch_up(receiver: &mut Receiver, sender: &Sender, now: u64, work: &mut Work) -> usize {
    let pages = sender.pages_since(receiver.cursor()).unwrap();
    for page in &pages {
        receiver.accept_page(page, now, work).unwrap();
    }
    pages.len()
}

/// 13 successors at t=10..130; catch-up at t=135 when only gen 13 is unexpired.
fn thirteen(seed: u8) -> (Identity, View, Sender) {
    let alice = owner(seed);
    let initial = View::initial(&alice, 1, "private");
    let mut sender = Sender::new(&alice, seed, initial.clone());
    sender.rotate_until(13);
    (alice, initial, sender)
}
const CATCH_UP_AT: u64 = 135;

// ---------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------

// KIND: invariant
#[test]
fn multi_page_catch_up_then_live_binding_proves_only_the_newest_key() {
    let (_alice, initial, sender) = thirteen(11);
    let now = CATCH_UP_AT;
    // Every intermediate record is expired at catch-up time; only the head is current.
    for g in 1..13 {
        assert!(sender.permit(g).claims.expires_at <= now);
    }
    assert!(now < sender.newest().claims.expires_at);
    // The sender holds only the newest key; intermediate keys are gone.
    assert_eq!(
        sender.live_key.as_ref().unwrap().verifying_key().to_bytes(),
        sender.newest().claims.endpoint
    );

    let mut receiver = Receiver::new(7, initial);
    let mut work = Work::default();
    let pages = catch_up(&mut receiver, &sender, now, &mut work);
    assert_eq!(pages, 4); // 4 + 4 + 4 + 1
    assert_eq!(work.signature_verifications, 13);
    assert_eq!(receiver.view, sender.view);
    assert_eq!(receiver.evidence.len(), 13);
    assert_eq!(receiver.route(now), None);

    let challenge = receiver.issue_challenge();
    receiver
        .bind(
            sender.newest(),
            &challenge,
            &sender.prove(&challenge),
            now,
            &mut work,
        )
        .unwrap();
    assert_eq!(work.signature_verifications, 14); // exactly one possession check
    assert_eq!(receiver.route(now), Some(sender.newest().claims.endpoint));
    assert_eq!(receiver.live.as_ref().unwrap().generation, 13);
    // The last record of page 3 (gen 12) is NOT what the binding names.
    assert_ne!(receiver.route(now), Some(sender.permit(12).claims.endpoint));
}

// KIND: invariant
#[test]
fn retired_intermediate_key_cannot_bind_live_and_state_is_unchanged() {
    let (_alice, initial, sender) = thirteen(12);
    let mut receiver = Receiver::new(7, initial);
    let mut work = Work::default();
    catch_up(&mut receiver, &sender, CATCH_UP_AT, &mut work);
    let challenge = receiver.issue_challenge();
    let snapshot = receiver.clone();
    // Adversary retained the retired gen-12 key (last record of page 3).
    let retired = fixture_key(12, 12);
    let retired_proof = retired.sign(&challenge.bytes()).to_bytes();
    let gen12 = sender.permit(12).clone();
    for (now, newest, proof, error) in [
        (CATCH_UP_AT, sender.newest(), &retired_proof, "possession"),
        (CATCH_UP_AT, &gen12, &retired_proof, "not_head"),
        // Even while gen 12 is still unexpired, it is not the verified head.
        (12 * INTERVAL + 1, &gen12, &retired_proof, "not_head"),
    ] {
        assert_eq!(
            receiver.bind(newest, &challenge, proof, now, &mut work),
            Err(error)
        );
        assert_eq!(receiver, snapshot);
        assert_eq!(receiver.route(now), None);
    }
    // Negative controls: a challenge not issued by this receiver, then a consumed one.
    let foreign = Challenge {
        receiver: 8,
        ..challenge.clone()
    };
    assert_eq!(
        receiver.bind(
            sender.newest(),
            &foreign,
            &sender.prove(&foreign),
            CATCH_UP_AT,
            &mut work
        ),
        Err("challenge_unknown")
    );
    assert_eq!(receiver, snapshot);
    receiver
        .bind(
            sender.newest(),
            &challenge,
            &sender.prove(&challenge),
            CATCH_UP_AT,
            &mut work,
        )
        .unwrap();
    let bound = receiver.clone();
    assert_eq!(
        receiver.bind(
            sender.newest(),
            &challenge,
            &sender.prove(&challenge),
            CATCH_UP_AT,
            &mut work
        ),
        Err("challenge_unknown")
    );
    assert_eq!(receiver, bound);
}

// KIND: invariant
#[test]
fn verified_history_without_live_proof_advances_knowledge_but_gives_no_route() {
    let (_alice, initial, sender) = thirteen(13);
    let mut receiver = Receiver::new(7, initial.clone());
    let mut work = Work::default();
    catch_up(&mut receiver, &sender, CATCH_UP_AT, &mut work);
    assert_eq!(receiver.view.generation, 13);
    assert_eq!(receiver.view.endpoint, sender.newest().claims.endpoint);
    assert!(receiver.live.is_none());
    // Head permit is unexpired, yet without possession there is no dialable route.
    for now in [CATCH_UP_AT, sender.newest().claims.expires_at - 1] {
        assert_eq!(receiver.route(now), None);
    }
    // A wrong-key proof also leaves knowledge advanced and route absent.
    let challenge = receiver.issue_challenge();
    let snapshot = receiver.clone();
    let stranger = fixture_key(99, 1);
    assert_eq!(
        receiver.bind(
            sender.newest(),
            &challenge,
            &stranger.sign(&challenge.bytes()).to_bytes(),
            CATCH_UP_AT,
            &mut work
        ),
        Err("possession")
    );
    assert_eq!(receiver, snapshot);
    // Expired head: history still verifies, live binding refused.
    let mut late = Receiver::new(8, initial);
    catch_up(&mut late, &sender, 200, &mut work);
    assert_eq!(late.view, sender.view);
    let c = late.issue_challenge();
    let snapshot = late.clone();
    assert_eq!(
        late.bind(sender.newest(), &c, &sender.prove(&c), 200, &mut work),
        Err("expired")
    );
    assert_eq!(late, snapshot);
    assert_eq!(late.route(200), None);
}

// KIND: invariant
#[test]
fn live_proof_before_the_verified_head_is_reached_is_rejected() {
    let (_alice, initial, sender) = thirteen(14);
    let mut receiver = Receiver::new(7, initial);
    let mut work = Work::default();
    let pages = sender.pages_since(0).unwrap();
    receiver
        .accept_page(&pages[0], CATCH_UP_AT, &mut work)
        .unwrap();
    receiver
        .accept_page(&pages[1], CATCH_UP_AT, &mut work)
        .unwrap();
    assert_eq!(receiver.cursor(), 8);
    let challenge = receiver.issue_challenge();
    let snapshot = receiver.clone();
    // Genuine newest key and genuine newest permit, but the receiver has not verified to it.
    assert_eq!(
        receiver.bind(
            sender.newest(),
            &challenge,
            &sender.prove(&challenge),
            CATCH_UP_AT,
            &mut work
        ),
        Err("not_head")
    );
    assert_eq!(receiver, snapshot);
    // The receiver's own head (gen 8) is expired; its retired key cannot bind either.
    let gen8_proof = fixture_key(14, 8).sign(&challenge.bytes()).to_bytes();
    assert_eq!(
        receiver.bind(
            sender.permit(8),
            &challenge,
            &gen8_proof,
            CATCH_UP_AT,
            &mut work
        ),
        Err("expired")
    );
    assert_eq!(receiver, snapshot);
    assert_eq!(receiver.route(CATCH_UP_AT), None);
    // A challenge issued at gen 8 does not carry over once the head advances.
    for page in &pages[2..] {
        receiver.accept_page(page, CATCH_UP_AT, &mut work).unwrap();
    }
    let snapshot = receiver.clone();
    assert_eq!(
        receiver.bind(
            sender.newest(),
            &challenge,
            &sender.prove(&challenge),
            CATCH_UP_AT,
            &mut work
        ),
        Err("challenge_head")
    );
    assert_eq!(receiver, snapshot);
}

// KIND: invariant
#[test]
fn interrupted_paging_resumes_from_the_receivers_cursor() {
    let (_alice, initial, mut sender) = thirteen(15);
    let mut receiver = Receiver::new(7, initial);
    let mut work = Work::default();
    let pages = sender.pages_since(receiver.cursor()).unwrap();
    receiver
        .accept_page(&pages[0], CATCH_UP_AT, &mut work)
        .unwrap();
    receiver
        .accept_page(&pages[1], CATCH_UP_AT, &mut work)
        .unwrap();
    // Page 3 arrives damaged (last record's signature corrupted): whole page rejected.
    let mut damaged = pages[2].clone();
    damaged.last_mut().unwrap().signature[0] ^= 1;
    let snapshot = receiver.clone();
    assert_eq!(
        receiver.accept_page(&damaged, CATCH_UP_AT, &mut work),
        Err("owner_signature")
    );
    assert_eq!(receiver, snapshot);
    assert_eq!(receiver.cursor(), 8);
    // Receiver goes offline; sender keeps rotating on its own timer (no block involved).
    let old_newest = sender.newest().clone();
    sender.rotate_until(15);
    let now = 155;
    let resumed = sender.pages_since(receiver.cursor()).unwrap();
    assert_eq!(resumed.iter().map(Vec::len).collect::<Vec<_>>(), vec![4, 3]); // gens 9..=15
    for page in &resumed {
        receiver.accept_page(page, now, &mut work).unwrap();
    }
    assert_eq!(receiver.view, sender.view);
    assert_eq!(receiver.evidence, sender.history);
    // The newest key at interruption time (gen 13) is now retired.
    let challenge = receiver.issue_challenge();
    let snapshot = receiver.clone();
    let gen13_proof = fixture_key(15, 13).sign(&challenge.bytes()).to_bytes();
    assert_eq!(
        receiver.bind(&old_newest, &challenge, &gen13_proof, now, &mut work),
        Err("not_head")
    );
    assert_eq!(receiver, snapshot);
    receiver
        .bind(
            sender.newest(),
            &challenge,
            &sender.prove(&challenge),
            now,
            &mut work,
        )
        .unwrap();
    assert_eq!(receiver.route(now), Some(sender.newest().claims.endpoint));
}

// KIND: invariant
#[test]
fn possession_by_a_key_cannot_substitute_for_owner_signed_history() {
    let (alice, initial, sender) = thirteen(16);
    let mallory = owner(200);
    let mut receiver = Receiver::new(7, initial);
    let mut work = Work::default();
    catch_up(&mut receiver, &sender, CATCH_UP_AT, &mut work);
    // Correctly chained gen-14 claims signed by someone other than the owner DID.
    let attacker_key = fixture_key(200, 14);
    let claims = Claims {
        version: 1,
        owner: alice.did().into(),
        audience: "private".into(),
        previous_id: receiver.view.current_id,
        replacement_id: fixture_device_id(200, 14),
        endpoint: attacker_key.verifying_key().to_bytes(),
        predecessor: receiver.view.head,
        generation: 14,
        issued_at: CATCH_UP_AT,
        expires_at: CATCH_UP_AT + INTERVAL,
    };
    let forged = Permit::issue(&mallory, claims);
    let challenge = receiver.issue_challenge();
    let snapshot = receiver.clone();
    assert_eq!(
        receiver.accept_page(std::slice::from_ref(&forged), CATCH_UP_AT, &mut work),
        Err("owner_signature")
    );
    assert_eq!(receiver, snapshot);
    let proof = attacker_key.sign(&challenge.bytes()).to_bytes();
    assert_eq!(
        receiver.bind(&forged, &challenge, &proof, CATCH_UP_AT, &mut work),
        Err("not_head")
    );
    assert_eq!(receiver, snapshot);
    assert_eq!(receiver.route(CATCH_UP_AT), None);
}

// KIND: invariant
#[test]
fn route_requires_live_binding_to_the_current_head_and_lapses_with_it() {
    let (_alice, initial, mut sender) = thirteen(17);
    let mut receiver = Receiver::new(7, initial);
    let mut work = Work::default();
    catch_up(&mut receiver, &sender, CATCH_UP_AT, &mut work);
    let c = receiver.issue_challenge();
    receiver
        .bind(
            sender.newest(),
            &c,
            &sender.prove(&c),
            CATCH_UP_AT,
            &mut work,
        )
        .unwrap();
    let bound_endpoint = sender.newest().claims.endpoint;
    assert_eq!(receiver.route(CATCH_UP_AT), Some(bound_endpoint));
    // Sender rotates at t=140; receiver verifies gen 14 before the gen-13 binding expires (142).
    sender.rotate(140);
    catch_up(&mut receiver, &sender, 141, &mut work);
    assert_eq!(receiver.route(141), None); // old binding is not reused for a new head
    assert_eq!(receiver.live.as_ref().unwrap().endpoint, bound_endpoint); // retained, not routed
    let c = receiver.issue_challenge();
    receiver
        .bind(sender.newest(), &c, &sender.prove(&c), 141, &mut work)
        .unwrap();
    assert_eq!(receiver.route(141), Some(sender.newest().claims.endpoint));
    assert_eq!(receiver.route(sender.newest().claims.expires_at), None);
    // Revoking the bound head withdraws the route even though history is retained.
    receiver.revoked.insert(receiver.view.head);
    assert_eq!(receiver.route(141), None);
    assert_eq!(receiver.evidence.len(), 14);
}

/// Shared G-HISTORY scenario: 40 rotations; receiver cursor at gen 0 (predates checkpoint at 30).
fn history_scenario(seed: u8) -> (View, Sender, u64) {
    let alice = owner(seed);
    let initial = View::initial(&alice, 1, "private");
    let mut sender = Sender::new(&alice, seed, initial.clone());
    sender.rotate_until(40);
    (initial, sender, 40 * INTERVAL + 5)
}

// KIND: candidate-policy G-HISTORY
#[test]
fn strict_full_history_option_requires_every_record_since_the_cursor() {
    let (initial, sender, now) = history_scenario(21);
    let mut work = Work::default();

    // A: sender retained everything -> stale receiver verifies all 40 and binds live.
    let mut stale = Receiver::new(7, initial.clone());
    assert_eq!(catch_up(&mut stale, &sender, now, &mut work), 10);
    assert_eq!(work.signature_verifications, 40);
    let c = stale.issue_challenge();
    stale
        .bind(sender.newest(), &c, &sender.prove(&c), now, &mut work)
        .unwrap();
    assert_eq!(stale.view, sender.view);

    // B: sender pruned through gen 29 -> a cursor predating it cannot be served.
    let mut pruned = sender.clone();
    pruned.prune_through(29);
    let mut stale2 = Receiver::new(8, initial.clone());
    let snapshot = stale2.clone();
    assert_eq!(
        pruned.pages_since(stale2.cursor()),
        Err("history_unavailable")
    );
    assert_eq!(stale2, snapshot);
    assert_eq!(stale2.route(now), None);
    // Skipping straight to the retained tail is a gap, not a jump.
    let tail = pruned.pages_since(29).unwrap();
    assert_eq!(stale2.accept_page(&tail[0], now, &mut work), Err("gap"));
    assert_eq!(stale2, snapshot);
    // A receiver whose cursor is inside the retained range is unaffected.
    let mut recent = Receiver::new(9, initial.clone());
    for page in sender.pages_since(0).unwrap().iter().take(8) {
        recent.accept_page(page, now, &mut work).unwrap();
    }
    assert_eq!(recent.cursor(), 32);
    for page in pruned.pages_since(32).unwrap() {
        recent.accept_page(&page, now, &mut work).unwrap();
    }
    assert_eq!(recent.view, sender.view);

    // C: a revoked intermediate record (gen 7) is detected because it must be verified.
    let mut strict_revoked = Receiver::new(10, initial);
    strict_revoked.revoked.insert(sender.permit(7).id());
    let pages = sender.pages_since(0).unwrap();
    strict_revoked
        .accept_page(&pages[0], now, &mut work)
        .unwrap();
    let snapshot = strict_revoked.clone();
    assert_eq!(
        strict_revoked.accept_page(&pages[1], now, &mut work),
        Err("revoked")
    );
    assert_eq!(strict_revoked, snapshot);
}

// KIND: candidate-policy G-HISTORY
#[test]
fn checkpoint_option_lets_a_stale_cursor_jump_with_anti_rollback_and_scope_checks() {
    let (initial, mut sender, now) = history_scenario(22);
    let (cp, cp_head) = sender.checkpoint_at(30);
    let (old_cp, old_head) = sender.checkpoint_at(20);
    let revoked_gen7 = sender.permit(7).id();
    sender.prune_through(29);
    let mut work = Work::default();

    // Same scenario as the strict test: cursor 0 predates the checkpoint, history pruned.
    let mut stale = Receiver::new(7, initial.clone());
    stale
        .accept_checkpoint(&cp, &cp_head, now, &mut work)
        .unwrap();
    assert_eq!(stale.cursor(), 30);
    for page in sender.pages_since(stale.cursor()).unwrap() {
        stale.accept_page(&page, now, &mut work).unwrap();
    }
    assert_eq!(stale.view, sender.view);
    assert_eq!(work.signature_verifications, 2 + 10);
    assert_eq!(stale.evidence.len(), 11);
    let c = stale.issue_challenge();
    stale
        .bind(sender.newest(), &c, &sender.prove(&c), now, &mut work)
        .unwrap();
    assert_eq!(stale.route(now), Some(sender.newest().claims.endpoint));

    // A revoked intermediate (gen 7) is NOT detected: the checkpoint skips its evidence.
    let mut skipped = Receiver::new(8, initial.clone());
    skipped.revoked.insert(revoked_gen7);
    skipped
        .accept_checkpoint(&cp, &cp_head, now, &mut work)
        .unwrap();
    assert_eq!(skipped.cursor(), 30);

    // Rejections leave state unchanged.
    let at_head = stale.clone();
    let mut tampered = cp.clone();
    tampered.claims.generation = 31;
    let mut wrong_anchor = cp.clone();
    wrong_anchor.claims.anchor = [9; 32];
    let wrong_anchor = Checkpoint {
        signature: owner(22).sign(&Checkpoint::bytes(&wrong_anchor.claims)),
        ..wrong_anchor
    };
    let mut wrong_audience = cp.claims.clone();
    wrong_audience.audience = "group:g1".into();
    let wrong_audience = Checkpoint {
        signature: owner(22).sign(&Checkpoint::bytes(&wrong_audience)),
        claims: wrong_audience,
    };
    let foreign = Checkpoint {
        signature: owner(201).sign(&Checkpoint::bytes(&cp.claims)),
        ..cp.clone()
    };
    let cases: Vec<(&Receiver, &Checkpoint, &Permit, &str)> = vec![
        (&skipped, &old_cp, &old_head, "stale"), // rollback below cursor
        (&at_head, &cp, &cp_head, "stale"),      // receiver already past the checkpoint
        (&at_head, &tampered, &cp_head, "owner_signature"),
        (&at_head, &foreign, &cp_head, "owner_signature"),
        (&skipped, &wrong_anchor, &cp_head, "scope"),
        (&skipped, &wrong_audience, &cp_head, "scope"),
    ];
    for (base, checkpoint, head, error) in cases {
        let mut r = base.clone();
        assert_eq!(
            r.accept_checkpoint(checkpoint, head, now, &mut work),
            Err(error)
        );
        assert_eq!(&r, base);
    }
    // Head permit must be the one the checkpoint names.
    let mut r = Receiver::new(9, initial.clone());
    let snapshot = r.clone();
    assert_eq!(
        r.accept_checkpoint(&cp, sender.permit(31), now, &mut work),
        Err("checkpoint_head")
    );
    assert_eq!(r, snapshot);
    // Idempotent re-delivery at the same head.
    let mut again = Receiver::new(10, initial);
    again
        .accept_checkpoint(&cp, &cp_head, now, &mut work)
        .unwrap();
    let snapshot = again.clone();
    again
        .accept_checkpoint(&cp, &cp_head, now, &mut work)
        .unwrap();
    assert_eq!(again, snapshot);
}

// KIND: limitation
#[test]
fn checkpoint_cannot_detect_a_fork_between_the_receivers_head_and_the_checkpoint() {
    // Honest chain to gen 5 is known by the receiver. A clone holding the shared mnemonic
    // (G-AUTHORITY: indistinguishable owner signatures) forks at gen 5 and checkpoints gen 12.
    let alice = owner(23);
    let initial = View::initial(&alice, 1, "private");
    let mut honest = Sender::new(&alice, 23, initial.clone());
    honest.rotate_until(5);
    let mut fork = Sender::new(&alice, 23, initial.clone());
    fork.rotate_until(4);
    fork.seed = 77; // different replacement IDs/endpoints from gen 5 on
    fork.rotate_until(12);
    assert_ne!(fork.permit(5).id(), honest.permit(5).id());
    let now = 12 * INTERVAL + 1;
    let mut work = Work::default();

    let mut receiver = Receiver::new(7, initial);
    catch_up(&mut receiver, &honest, now, &mut work);
    assert_eq!(receiver.cursor(), 5);

    // Strict: the fork's gen-6 record does not chain to the receiver's gen-5 head.
    let snapshot = receiver.clone();
    let fork_tail = fork.pages_since(5).unwrap();
    assert_eq!(
        receiver.clone().accept_page(&fork_tail[0], now, &mut work),
        Err("predecessor")
    );
    let fork_full = fork.pages_since(4).unwrap();
    assert_eq!(
        receiver.clone().accept_page(&fork_full[0], now, &mut work),
        Err("fork")
    );
    assert_eq!(receiver, snapshot);

    // Checkpoint: accepted, silently abandoning the receiver's known gen-5 head.
    let (cp, head) = fork.checkpoint_at(12);
    receiver
        .accept_checkpoint(&cp, &head, now, &mut work)
        .unwrap();
    assert_eq!(receiver.view, fork.view);
    // The conflicting honest gen-5 evidence is still retained, with no fork signal raised.
    assert!(
        receiver
            .evidence
            .iter()
            .any(|p| p.id() == honest.permit(5).id())
    );
}

// --- Group adapter contract (mirrors crates/group-sim/src/reachability.rs; read-only copy) ---

#[derive(Clone, Debug, PartialEq, Eq)]
struct BindingEvidence {
    owner: String,
    device_id: Id,
    generation: u64,
    audience: String,
}

/// Mirrors `HostDirectory::install` plus the documented adapter contract: "full verified
/// ancestry since nominated root, with no omitted endpoint-bearing predecessors".
fn group_contract_accepts(
    nominated_initial: Id,
    owner: &str,
    group: &str,
    path: &[BindingEvidence],
) -> bool {
    let Some(first) = path.first() else {
        return false;
    };
    first.device_id == nominated_initial
        && path.iter().all(|r| r.owner == owner && r.audience == group)
        && path
            .iter()
            .enumerate()
            .all(|(i, r)| r.generation == i as u64)
}

/// What a receiver could export to the group adapter from its retained evidence.
fn export_path(initial: &View, receiver: &Receiver) -> Vec<BindingEvidence> {
    let root = BindingEvidence {
        owner: initial.owner.clone(),
        device_id: initial.current_id,
        generation: 0,
        audience: initial.audience.clone(),
    };
    std::iter::once(root)
        .chain(receiver.evidence.iter().map(|p| BindingEvidence {
            owner: p.claims.owner.clone(),
            device_id: p.claims.replacement_id,
            generation: p.claims.generation,
            audience: p.claims.audience.clone(),
        }))
        .collect()
}

// KIND: candidate-policy G-HISTORY
#[test]
fn group_full_ancestry_contract_is_met_by_strict_history_but_not_by_a_checkpoint() {
    let alice = owner(24);
    let group = "group:g1";
    let initial = View::initial(&alice, 50, group);
    let mut sender = Sender::new(&alice, 24, initial.clone());
    sender.rotate_until(20);
    let now = 20 * INTERVAL + 1;
    let mut work = Work::default();

    let mut strict = Receiver::new(7, initial.clone());
    catch_up(&mut strict, &sender, now, &mut work);
    let strict_path = export_path(&initial, &strict);
    assert_eq!(strict_path.len(), 21);
    assert!(group_contract_accepts(
        initial.current_id,
        alice.did(),
        group,
        &strict_path
    ));

    let (cp, head) = sender.checkpoint_at(15);
    let mut checkpointed = Receiver::new(8, initial.clone());
    checkpointed
        .accept_checkpoint(&cp, &head, now, &mut work)
        .unwrap();
    for page in sender.pages_since(15).unwrap() {
        checkpointed.accept_page(&page, now, &mut work).unwrap();
    }
    assert_eq!(checkpointed.view, strict.view); // same current knowledge...
    let with_root = export_path(&initial, &checkpointed);
    let without_root = &with_root[1..];
    // ...but generations 1..=14 are omitted endpoint-bearing predecessors.
    assert_eq!(with_root.len(), 7);
    assert!(!group_contract_accepts(
        initial.current_id,
        alice.did(),
        group,
        &with_root
    ));
    assert!(!group_contract_accepts(
        initial.current_id,
        alice.did(),
        group,
        without_root
    ));
    // Treating the checkpoint head as the root would be a group-contract CHANGE (different
    // root, non-zero first generation); this model does not make that change.
    assert_ne!(without_root[0].device_id, initial.current_id);
}

// KIND: candidate-policy G-HISTORY
#[test]
fn work_bounds_for_one_thousand_rotations_under_both_options() {
    const N: u64 = 1000;
    const CP_GEN: u64 = 960; // fixture: checkpoint 40 rotations behind head
    let alice = owner(25);
    let initial = View::initial(&alice, 1, "private");
    let mut sender = Sender::new(&alice, 25, initial.clone());
    sender.rotate_until(N);
    let now = N * INTERVAL + 1;
    let pages = |records: u64| records.div_ceil(PAGE as u64);

    // Strict: receiver at cursor 0 verifies every record, then one possession check.
    let mut work = Work::default();
    let mut strict = Receiver::new(7, initial.clone());
    let strict_pages = catch_up(&mut strict, &sender, now, &mut work) as u64;
    let c = strict.issue_challenge();
    strict
        .bind(sender.newest(), &c, &sender.prove(&c), now, &mut work)
        .unwrap();
    assert_eq!(strict_pages, pages(N));
    assert_eq!(work.signature_verifications, N + 1);
    assert_eq!(strict.evidence.len() as u64, N); // receiver-retained records
    assert_eq!(sender.history.len() as u64, N); // sender must retain back to oldest cursor
    let strict_bytes: usize = sender.history.iter().map(Permit::wire_len).sum();

    // Checkpoint: checkpoint + head permit + tail, then one possession check.
    let (cp, head) = sender.checkpoint_at(CP_GEN);
    let mut pruned = sender.clone();
    pruned.prune_through(CP_GEN - 1);
    let mut work = Work::default();
    let mut ck = Receiver::new(8, initial);
    ck.accept_checkpoint(&cp, &head, now, &mut work).unwrap();
    let tail = pruned.pages_since(ck.cursor()).unwrap();
    for page in &tail {
        ck.accept_page(page, now, &mut work).unwrap();
    }
    let c = ck.issue_challenge();
    ck.bind(pruned.newest(), &c, &pruned.prove(&c), now, &mut work)
        .unwrap();
    let tail_len = N - CP_GEN;
    assert_eq!(tail.len() as u64, pages(tail_len));
    assert_eq!(work.signature_verifications, 2 + tail_len + 1);
    assert_eq!(ck.evidence.len() as u64, 1 + tail_len);
    assert!(ck.checkpoint.is_some()); // + one checkpoint record
    assert_eq!(pruned.history.len() as u64, 1 + tail_len);
    let ck_bytes: usize =
        cp.wire_len() + pruned.history.iter().map(Permit::wire_len).sum::<usize>();

    assert_eq!(strict.view, ck.view);
    assert_eq!(strict.route(now), ck.route(now));
    // Bytes scale with records; checkpoint bytes are bounded by tail length, not N.
    assert!(
        strict_bytes > 20 * ck_bytes,
        "strict={strict_bytes} checkpoint={ck_bytes}"
    );
    eprintln!(
        "B1 bounds N={N}: strict sigs={} records={} pages={} bytes={strict_bytes}; \
        checkpoint sigs={} records={} pages={} bytes={ck_bytes}",
        N + 1,
        N,
        pages(N),
        2 + tail_len + 1,
        2 + tail_len,
        1 + pages(tail_len)
    );
}
