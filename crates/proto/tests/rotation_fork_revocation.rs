//! Packet B3 model: succession FORKS and OLDER-ANCESTOR REVOCATION. Test-only candidate
//! semantics, NOT a production wire format, verifier API, storage design or policy decision.
//!
//! G-FORK, G-REVOCATION and G-AUTHORITY are OPEN. Each is modeled with >= 2 options as receiver
//! parameters (`ForkPolicy`, `RevocationPolicy`, `Authority`); tests labelled
//! `candidate-policy <GATE>` encode one option each and select nothing.
//!
//! Attacker model (docs/design/device-linking.md §0, group contract item 5): every linked device
//! holds the account mnemonic and can therefore produce ANY owner-DID signature: permits,
//! revocations, recoveries, checkpoints. In this file an "attacker with the mnemonic" is modeled
//! by signing with the SAME `Identity` as the honest owner. Nothing here claims such a device can
//! be cryptographically excluded; the `limitation` tests demonstrate that it cannot.
//!
//! Fork evidence (model choice, not policy): a fork PROOF is two distinct owner-signed successors
//! of the SAME predecessor record (same `predecessor` and `previous_id`) on a path the receiver
//! has retained. Any other mismatch is an ordinary rejection with no flag.
//!
//! Fixture endpoint keys and device IDs are derived from fixed seeds ONLY for deterministic
//! tests. Receivers never compute them; this is NOT a future-endpoint derivation scheme.
use std::collections::BTreeSet;

use ed25519_dalek::{Signer, SigningKey};
use identity::Identity;
use serde::Serialize;

const REVOCATION_DOMAIN: &[u8] = b"test-only/device-succession-revocation/v1\0";
const RECOVERY_DOMAIN: &[u8] = b"test-only/device-succession-recovery/v1\0";

#[path = "support/permit_history.rs"]
mod permit_history;
use permit_history::*;
#[path = "support/fixtures.rs"]
mod fixtures;
use fixtures::*;

// ---------------------------------------------------------------------------------------
// B3 model: fork evidence, revocation, recovery, authority options
// ---------------------------------------------------------------------------------------

/// CANDIDATE G-FORK options for what a receiver does after a fork proof is flagged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ForkPolicy {
    /// Stop routing to and extending the path until an (unmodeled) owner resolution.
    FreezePath,
    /// Keep the first-accepted head live; record the conflict for the user/owner.
    KeepHead,
}

/// CANDIDATE G-REVOCATION options for revoking a record after descendants were accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RevocationPolicy {
    /// (a) Revoked record AND all descendants are invalid; path suspended until a fresh
    /// authority-signed recovery from the revoked record's parent.
    Cascade,
    /// (b) Revocation blocks only that record's endpoint for live binding; history and
    /// descendants stand.
    EndpointOnly,
}

/// CANDIDATE G-AUTHORITY options for who may sign revocation/recovery records.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Authority {
    /// Current trust model: the owner DID key, i.e. ANY mnemonic holder.
    OwnerDid,
    /// HYPOTHETICAL (requires a G-AUTHORITY change): a separate key NOT held by linked devices.
    /// How it is generated, held, distributed to contacts and itself recovered is unresolved.
    SeparateRecoveryKey(Id),
}

#[derive(Clone)]
enum AuthSigner {
    Did(Identity),
    Key(SigningKey),
}

impl AuthSigner {
    fn sign(&self, message: &[u8]) -> [u8; 64] {
        match self {
            AuthSigner::Did(identity) => identity.sign(message),
            AuthSigner::Key(key) => key.sign(message).to_bytes(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct RevocationClaims {
    version: u8,
    owner: String,
    audience: String,
    anchor: Id,
    target: Id,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Revocation {
    claims: RevocationClaims,
    signature: [u8; 64],
}

impl Revocation {
    fn bytes(claims: &RevocationClaims) -> Vec<u8> {
        [REVOCATION_DOMAIN, &serde_json::to_vec(claims).unwrap()].concat()
    }
    fn issue(signer: &AuthSigner, view: &View, target: Id) -> Self {
        let claims = RevocationClaims {
            version: 1,
            owner: view.owner.clone(),
            audience: view.audience.clone(),
            anchor: view.anchor,
            target,
        };
        let signature = signer.sign(&Self::bytes(&claims));
        Self { claims, signature }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct RecoveryClaims {
    version: u8,
    owner: String,
    audience: String,
    anchor: Id,
    revoked: Id,
    replacement: Id,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Recovery {
    claims: RecoveryClaims,
    signature: [u8; 64],
}

impl Recovery {
    fn bytes(claims: &RecoveryClaims) -> Vec<u8> {
        [RECOVERY_DOMAIN, &serde_json::to_vec(claims).unwrap()].concat()
    }
    fn issue(signer: &AuthSigner, view: &View, revoked: Id, replacement: &Permit) -> Self {
        let claims = RecoveryClaims {
            version: 1,
            owner: view.owner.clone(),
            audience: view.audience.clone(),
            anchor: view.anchor,
            revoked,
            replacement: replacement.id(),
        };
        let signature = signer.sign(&Self::bytes(&claims));
        Self { claims, signature }
    }
}

/// Retained, shareable fork proof: two owner-signed successors of the same parent.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Conflict {
    generation: u64,
    parent: Id,
    kept: Permit,
    other: Permit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Suspension {
    revoked: Id,
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Accepted,
    Flagged,
}

/// Per-receiver state. Snapshot-compared on every rejection.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Receiver {
    name: u64,
    initial: View,
    view: View,
    // Retained historical evidence (permits accepted after the initial anchor).
    evidence: Vec<Permit>,
    checkpoint: Option<Checkpoint>,
    next_nonce: u64,
    outstanding: BTreeSet<u64>,
    live: Option<LiveBinding>,
    fork_policy: ForkPolicy,
    revocation_policy: RevocationPolicy,
    authority: Authority,
    conflicts: Vec<Conflict>,
    revoked: Revoked,
    revocations: Vec<Revocation>,
    suspended: Option<Suspension>,
}

impl Receiver {
    fn new(
        name: u64,
        initial: &View,
        fork_policy: ForkPolicy,
        revocation_policy: RevocationPolicy,
        authority: Authority,
    ) -> Self {
        Self {
            name,
            initial: initial.clone(),
            view: initial.clone(),
            evidence: Vec::new(),
            checkpoint: None,
            next_nonce: 0,
            outstanding: BTreeSet::new(),
            live: None,
            fork_policy,
            revocation_policy,
            authority,
            conflicts: Vec::new(),
            revoked: Revoked::new(),
            revocations: Vec::new(),
            suspended: None,
        }
    }
    fn cursor(&self) -> u64 {
        self.view.generation
    }
    fn frozen(&self) -> bool {
        self.fork_policy == ForkPolicy::FreezePath && !self.conflicts.is_empty()
    }
    /// Revocations that block HISTORY: all of them under Cascade, none under EndpointOnly.
    fn history_revoked(&self) -> Revoked {
        match self.revocation_policy {
            RevocationPolicy::Cascade => self.revoked.clone(),
            RevocationPolicy::EndpointOnly => Revoked::new(),
        }
    }
    fn known_at(&self, generation: u64) -> Option<&Permit> {
        self.evidence
            .iter()
            .find(|p| p.claims.generation == generation)
    }
    fn view_at(&self, generation: u64) -> Option<View> {
        if generation == 0 {
            return Some(self.initial.clone());
        }
        let p = self.known_at(generation)?;
        Some(View {
            head: p.id(),
            current_id: p.claims.replacement_id,
            endpoint: p.claims.endpoint,
            generation,
            expires_at: p.claims.expires_at,
            ..self.view.clone()
        })
    }
    fn authority_key(&self) -> Option<Id> {
        match &self.authority {
            Authority::OwnerDid => identity::public_key_from_did(&self.view.owner),
            Authority::SeparateRecoveryKey(key) => Some(*key),
        }
    }
    /// Fork PROOF only: a validly owner-signed record that shares its parent with a different
    /// retained record. Already-revoked records are known-bad, not new conflicts.
    fn detect_conflict(&self, p: &Permit, work: &mut Work) -> Option<Conflict> {
        let known = self.known_at(p.claims.generation)?;
        let c = &p.claims;
        if known.id() == p.id()
            || self.revoked.contains(&p.id())
            || c.predecessor != known.claims.predecessor
            || c.previous_id != known.claims.previous_id
            || c.owner != self.view.owner
            || c.audience != self.view.audience
            || c.version != 1
        {
            return None;
        }
        let key = identity::public_key_from_did(&self.view.owner)?;
        work.signature_verifications += 1;
        if !identity::verify(&key, &Permit::bytes(c), &p.signature) {
            return None;
        }
        Some(Conflict {
            generation: c.generation,
            parent: c.predecessor,
            kept: known.clone(),
            other: p.clone(),
        })
    }
    /// Fork detection runs first so evidence is recorded even on a frozen/suspended path.
    /// A flag changes ONLY `conflicts`; view, evidence and live binding are untouched.
    fn offer_page(
        &mut self,
        page: &[Permit],
        now: u64,
        work: &mut Work,
    ) -> Result<Outcome, &'static str> {
        if page.is_empty() || page.len() > PAGE {
            return Err("page_limit");
        }
        for p in page {
            if let Some(conflict) = self.detect_conflict(p, work) {
                if !self
                    .conflicts
                    .iter()
                    .any(|k| k.other.id() == conflict.other.id())
                {
                    self.conflicts.push(conflict);
                }
                return Ok(Outcome::Flagged);
            }
        }
        if self.suspended.is_some() {
            return Err("suspended");
        }
        if self.frozen() {
            return Err("frozen");
        }
        let next = verify_history_page(&self.view, page, now, &self.history_revoked(), work)?;
        let added: Vec<_> = page
            .iter()
            .filter(|p| p.claims.generation > self.view.generation)
            .cloned()
            .collect();
        self.view = next;
        self.evidence.extend(added);
        Ok(Outcome::Accepted)
    }
    /// B1 checkpoint semantics, unchanged (no fork detection is possible across the jump).
    fn accept_checkpoint(
        &mut self,
        cp: &Checkpoint,
        head: &Permit,
        now: u64,
        work: &mut Work,
    ) -> Result<(), &'static str> {
        let next = verify_checkpoint(&self.view, cp, head, now, &self.history_revoked(), work)?;
        if next != self.view {
            self.view = next;
            self.evidence.push(head.clone());
            self.checkpoint = Some(cp.clone());
        }
        Ok(())
    }
    fn accept_revocation(&mut self, r: &Revocation, work: &mut Work) -> Result<(), &'static str> {
        let key = self.authority_key().ok_or("owner_key")?;
        let c = &r.claims;
        if c.owner != self.view.owner {
            return Err("authority");
        }
        work.signature_verifications += 1;
        if !identity::verify(&key, &Revocation::bytes(c), &r.signature) {
            return Err("authority");
        }
        if c.version != 1 || c.audience != self.view.audience || c.anchor != self.view.anchor {
            return Err("scope");
        }
        if self.revoked.contains(&c.target) {
            return Ok(()); // idempotent redelivery
        }
        self.revoked.insert(c.target);
        self.revocations.push(r.clone());
        if self.revocation_policy == RevocationPolicy::Cascade {
            let at = if c.target == self.view.anchor {
                Some(0)
            } else {
                self.evidence
                    .iter()
                    .find(|p| p.id() == c.target)
                    .map(|p| p.claims.generation)
            };
            if let Some(generation) = at {
                if self
                    .suspended
                    .as_ref()
                    .is_none_or(|s| generation < s.generation)
                {
                    self.suspended = Some(Suspension {
                        revoked: c.target,
                        generation,
                    });
                }
            }
        }
        Ok(())
    }
    /// Cascade only: replace the revoked record by a fresh successor of its parent.
    fn accept_recovery(
        &mut self,
        rec: &Recovery,
        permit: &Permit,
        now: u64,
        work: &mut Work,
    ) -> Result<(), &'static str> {
        if self.revocation_policy != RevocationPolicy::Cascade {
            return Err("no_recovery_in_policy");
        }
        let suspension = self.suspended.clone().ok_or("not_suspended")?;
        let key = self.authority_key().ok_or("owner_key")?;
        let c = &rec.claims;
        if c.owner != self.view.owner {
            return Err("authority");
        }
        work.signature_verifications += 1;
        if !identity::verify(&key, &Recovery::bytes(c), &rec.signature) {
            return Err("authority");
        }
        if c.version != 1 || c.audience != self.view.audience || c.anchor != self.view.anchor {
            return Err("scope");
        }
        if c.revoked != suspension.revoked {
            return Err("recovery_target");
        }
        if suspension.generation == 0 {
            // A revoked nomination cannot be repaired by succession (group/initial policy).
            return Err("renomination_required");
        }
        if c.replacement != permit.id() {
            return Err("recovery_replacement");
        }
        let parent = self
            .view_at(suspension.generation - 1)
            .ok_or("history_unavailable")?;
        let next = verify_history_page(
            &parent,
            std::slice::from_ref(permit),
            now,
            &self.revoked,
            work,
        )?;
        self.evidence
            .retain(|p| p.claims.generation < suspension.generation);
        self.evidence.push(permit.clone());
        self.view = next;
        self.suspended = None;
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
        if self.suspended.is_some() {
            return Err("suspended");
        }
        if self.frozen() {
            return Err("frozen");
        }
        let blocked: Revoked = match self.revocation_policy {
            RevocationPolicy::Cascade => self.revoked.clone(),
            RevocationPolicy::EndpointOnly => self
                .revoked
                .iter()
                .filter(|id| **id == self.view.head)
                .copied()
                .collect(),
        };
        let binding = bind_live(&self.view, newest, challenge, proof, now, &blocked, work)?;
        self.outstanding.remove(&challenge.nonce);
        self.live = Some(binding);
        Ok(())
    }
    /// Dialable route requires a live binding to the CURRENT verified head on a path that is
    /// neither suspended nor frozen, and whose head endpoint is not revoked.
    fn route(&self, now: u64) -> Option<Id> {
        let live = self.live.as_ref()?;
        let anchor_blocks = self.revocation_policy == RevocationPolicy::Cascade
            && self.revoked.contains(&self.view.anchor);
        (live.head == self.view.head
            && now < live.expires_at
            && self.suspended.is_none()
            && !self.frozen()
            && !self.revoked.contains(&live.head)
            && !anchor_blocks)
            .then_some(live.endpoint)
    }
}

/// Owner device (or a clone holding the same mnemonic). Holds ONLY the newest endpoint key.
#[derive(Clone)]
struct Sender {
    owner: Identity,
    seed: u8,
    initial: View,
    view: View,
    history: Vec<Permit>,
    live_key: Option<SigningKey>,
}

impl Sender {
    fn new(owner: &Identity, seed: u8, view: View) -> Self {
        Self {
            owner: owner.clone(),
            seed,
            initial: view.clone(),
            view,
            history: Vec::new(),
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
    /// A second branch sharing history BEFORE `generation`, continuing with different fixture
    /// keys/IDs. Models a cloned device or a mnemonic holder signing its own successors.
    fn fork_at(&self, generation: u64, seed: u8) -> Sender {
        let mut s = self.clone();
        s.history.retain(|p| p.claims.generation < generation);
        s.view = match s.history.last() {
            None => s.initial.clone(),
            Some(last) => View {
                head: last.id(),
                current_id: last.claims.replacement_id,
                endpoint: last.claims.endpoint,
                generation: last.claims.generation,
                expires_at: last.claims.expires_at,
                ..s.initial.clone()
            },
        };
        s.seed = seed;
        s.live_key = None;
        s
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
    fn key(&self) -> &SigningKey {
        self.live_key.as_ref().unwrap()
    }
    fn pages_since(&self, cursor: u64) -> Vec<Vec<Permit>> {
        let rest: Vec<_> = self
            .history
            .iter()
            .filter(|p| p.claims.generation > cursor)
            .cloned()
            .collect();
        rest.chunks(PAGE).map(|c| c.to_vec()).collect()
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
}

// --- Test helpers -------------------------------------------------------------------------

const POLICIES: [ForkPolicy; 2] = [ForkPolicy::FreezePath, ForkPolicy::KeepHead];

fn receiver(name: u64, initial: &View, fork: ForkPolicy, rev: RevocationPolicy) -> Receiver {
    Receiver::new(name, initial, fork, rev, Authority::OwnerDid)
}

/// Strict catch-up from the receiver's cursor; every page must be accepted.
fn feed(r: &mut Receiver, s: &Sender, now: u64, work: &mut Work) {
    for page in s.pages_since(r.cursor()) {
        assert_eq!(r.offer_page(&page, now, work), Ok(Outcome::Accepted));
    }
}

/// Issues a challenge, then binds `newest` with a proof by `key`. On rejection asserts the
/// WHOLE receiver equals the post-challenge snapshot.
fn attempt_bind(
    r: &mut Receiver,
    newest: &Permit,
    key: &SigningKey,
    now: u64,
    work: &mut Work,
) -> Result<(), &'static str> {
    let challenge = r.issue_challenge();
    let snapshot = r.clone();
    let proof = key.sign(&challenge.bytes()).to_bytes();
    let result = r.bind(newest, &challenge, &proof, now, work);
    if result.is_err() {
        assert_eq!(*r, snapshot);
    }
    result
}

fn bind_sender(r: &mut Receiver, s: &Sender, now: u64, work: &mut Work) {
    attempt_bind(r, s.newest(), s.key(), now, work).unwrap();
}

/// Offers a page that must be REJECTED; asserts whole state unchanged.
fn reject_page(r: &mut Receiver, page: &[Permit], now: u64, work: &mut Work) -> &'static str {
    let snapshot = r.clone();
    let error = r.offer_page(page, now, work).unwrap_err();
    assert_eq!(*r, snapshot);
    error
}

/// Offers a page that must be FLAGGED; asserts only `conflicts` changed.
fn flag_page(r: &mut Receiver, page: &[Permit], now: u64, work: &mut Work) -> Conflict {
    let before = r.clone();
    assert_eq!(r.offer_page(page, now, work), Ok(Outcome::Flagged));
    let conflict = r.conflicts.last().unwrap().clone();
    let mut expected = before.clone();
    if !before.conflicts.contains(&conflict) {
        expected.conflicts.push(conflict.clone());
    }
    assert_eq!(*r, expected);
    // Idempotent redelivery: whole state unchanged.
    let again = r.clone();
    assert_eq!(r.offer_page(page, now, work), Ok(Outcome::Flagged));
    assert_eq!(*r, again);
    conflict
}

fn reject_revocation(r: &mut Receiver, rev: &Revocation, work: &mut Work) -> &'static str {
    let snapshot = r.clone();
    let error = r.accept_revocation(rev, work).unwrap_err();
    assert_eq!(*r, snapshot);
    error
}

fn reject_recovery(
    r: &mut Receiver,
    rec: &Recovery,
    permit: &Permit,
    now: u64,
    work: &mut Work,
) -> &'static str {
    let snapshot = r.clone();
    let error = r.accept_recovery(rec, permit, now, work).unwrap_err();
    assert_eq!(*r, snapshot);
    error
}

/// Honest owner chain to `generation`, private audience.
fn honest(seed: u8, generation: u64) -> (Identity, View, Sender) {
    let alice = owner(seed);
    let initial = View::initial(&alice, 1, "private");
    let mut sender = Sender::new(&alice, seed, initial.clone());
    sender.rotate_until(generation);
    (alice, initial, sender)
}

/// Clone branch: same parent as the honest record at `generation`, different successor.
fn clone_branch(honest: &Sender, generation: u64, seed: u8, until: u64) -> Sender {
    let mut fork = honest.fork_at(generation, seed);
    fork.rotate_until(until);
    assert_ne!(fork.permit(generation).id(), honest.permit(generation).id());
    assert_eq!(
        fork.permit(generation).claims.predecessor,
        honest.permit(generation).claims.predecessor
    );
    fork
}

// ---------------------------------------------------------------------------------------
// Tests: forks
// ---------------------------------------------------------------------------------------

// KIND: candidate-policy G-FORK
#[test]
fn same_generation_fork_is_flagged_first_accepted_kept_and_freeze_path_stops_live_use() {
    let (_alice, initial, mut honest) = honest(31, 3);
    let clone = clone_branch(&honest, 3, 131, 3);
    let now = 3 * INTERVAL + 1;
    let mut work = Work::default();
    let mut r = receiver(
        7,
        &initial,
        ForkPolicy::FreezePath,
        RevocationPolicy::Cascade,
    );
    feed(&mut r, &honest, now, &mut work);
    bind_sender(&mut r, &honest, now, &mut work);
    assert_eq!(r.route(now), Some(honest.newest().claims.endpoint));

    let conflict = flag_page(&mut r, &[clone.newest().clone()], now, &mut work);
    assert_eq!(conflict.generation, 3);
    assert_eq!(conflict.parent, honest.permit(2).id());
    assert_eq!(conflict.kept, *honest.newest()); // first-accepted kept
    assert_eq!(conflict.other, *clone.newest());
    assert_eq!(r.view, honest.view);
    // Freeze: no route for EITHER branch, no extension, no re-binding.
    assert_eq!(r.route(now), None);
    honest.rotate(now);
    assert_eq!(
        reject_page(&mut r, &[honest.newest().clone()], now, &mut work),
        "frozen"
    );
    assert_eq!(
        attempt_bind(
            &mut r,
            honest.permit(3),
            &fixture_key(31, 3),
            now,
            &mut work
        ),
        Err("frozen")
    );
    assert_eq!(
        attempt_bind(&mut r, clone.newest(), clone.key(), now, &mut work),
        Err("frozen")
    );
    assert_eq!(r.route(now), None);
}

// KIND: candidate-policy G-FORK
#[test]
fn same_generation_fork_is_flagged_first_accepted_kept_and_keep_head_stays_live() {
    let (_alice, initial, mut honest) = honest(32, 3);
    let mut clone = clone_branch(&honest, 3, 132, 3);
    let now = 3 * INTERVAL + 1;
    let mut work = Work::default();
    let mut r = receiver(7, &initial, ForkPolicy::KeepHead, RevocationPolicy::Cascade);
    feed(&mut r, &honest, now, &mut work);
    bind_sender(&mut r, &honest, now, &mut work);

    let conflict = flag_page(&mut r, &[clone.newest().clone()], now, &mut work);
    assert_eq!(conflict.kept, *honest.newest());
    assert_eq!(r.view, honest.view);
    // Keep head: the first-accepted branch remains live and extendable.
    assert_eq!(r.route(now), Some(honest.newest().claims.endpoint));
    assert_eq!(
        attempt_bind(&mut r, clone.newest(), clone.key(), now, &mut work),
        Err("not_head")
    );
    clone.rotate(now);
    assert_eq!(
        reject_page(&mut r, &[clone.newest().clone()], now, &mut work),
        "predecessor"
    );
    let later = 4 * INTERVAL + 1;
    honest.rotate(4 * INTERVAL);
    assert_eq!(
        r.offer_page(&[honest.newest().clone()], later, &mut work),
        Ok(Outcome::Accepted)
    );
    bind_sender(&mut r, &honest, later, &mut work);
    assert_eq!(r.route(later), Some(honest.newest().claims.endpoint));
    assert_eq!(r.conflicts.len(), 1); // still reported; nothing resolved it
}

// KIND: invariant
#[test]
fn older_generation_fork_evidence_after_advancing_is_detected() {
    let (_alice, initial, honest) = honest(33, 9);
    let clone = clone_branch(&honest, 5, 133, 7);
    let now = 9 * INTERVAL + 1;
    for policy in POLICIES {
        for rev in [RevocationPolicy::Cascade, RevocationPolicy::EndpointOnly] {
            let mut work = Work::default();
            let mut r = receiver(7, &initial, policy, rev);
            feed(&mut r, &honest, now, &mut work);
            assert_eq!(r.cursor(), 9);
            // A tail that does not include the divergence point is not a proof: plain rejection.
            let tail = clone.pages_since(5);
            assert_eq!(reject_page(&mut r, &tail[0], now, &mut work), "stale");
            assert!(r.conflicts.is_empty());
            // The divergence point (gen 5 vs gen 5', same parent gen 4) is detected 4 generations
            // after the receiver passed it; head, evidence and live state are untouched.
            let from_divergence = clone.pages_since(4);
            let conflict = flag_page(&mut r, &from_divergence[0], now, &mut work);
            assert_eq!(conflict.generation, 5);
            assert_eq!(conflict.parent, honest.permit(4).id());
            assert_eq!(conflict.kept, *honest.permit(5));
            assert_eq!(r.view, honest.view);
            assert_eq!(r.evidence, honest.history);
        }
    }
}

// KIND: candidate-policy G-FORK
#[test]
fn older_fork_handling_freeze_path_versus_keep_head() {
    let (_alice, initial, mut honest) = honest(34, 9);
    let clone = clone_branch(&honest, 5, 134, 6);
    let now = 9 * INTERVAL + 1;
    let mut work = Work::default();
    let mut frozen = receiver(
        7,
        &initial,
        ForkPolicy::FreezePath,
        RevocationPolicy::Cascade,
    );
    let mut kept = receiver(8, &initial, ForkPolicy::KeepHead, RevocationPolicy::Cascade);
    for r in [&mut frozen, &mut kept] {
        feed(r, &honest, now, &mut work);
        bind_sender(r, &honest, now, &mut work);
        flag_page(r, &clone.pages_since(4)[0], now, &mut work);
    }
    // Freeze: even though gen 9 is four generations past the contested segment.
    assert_eq!(frozen.route(now), None);
    // Keep head: the current head remains live.
    assert_eq!(kept.route(now), Some(honest.newest().claims.endpoint));

    let later = 10 * INTERVAL + 1;
    honest.rotate(10 * INTERVAL);
    let next = [honest.newest().clone()];
    assert_eq!(reject_page(&mut frozen, &next, later, &mut work), "frozen");
    assert_eq!(
        kept.offer_page(&next, later, &mut work),
        Ok(Outcome::Accepted)
    );
    assert_eq!(frozen.conflicts, kept.conflicts);
}

// KIND: limitation
#[test]
fn receiver_that_keeps_only_its_cursor_cannot_detect_an_older_fork() {
    let (_alice, initial, honest) = honest(35, 9);
    let clone = clone_branch(&honest, 5, 135, 6);
    let now = 9 * INTERVAL + 1;
    let mut work = Work::default();
    let mut full = receiver(7, &initial, ForkPolicy::KeepHead, RevocationPolicy::Cascade);
    feed(&mut full, &honest, now, &mut work);
    // Same knowledge of (generation, head) but no retained per-record (generation, id,
    // predecessor) evidence: models a receiver that discards history after verifying it.
    let mut cursor_only = full.clone();
    cursor_only.evidence.clear();
    let divergence = &clone.pages_since(4)[0];
    flag_page(&mut full, divergence, now, &mut work);
    assert_eq!(
        reject_page(&mut cursor_only, divergence, now, &mut work),
        "stale"
    );
    assert!(cursor_only.conflicts.is_empty());
}

// KIND: invariant
#[test]
fn two_receivers_on_different_branches_both_flag_after_exchange_and_neither_switches() {
    let (_alice, initial, honest) = honest(36, 2);
    let clone = clone_branch(&honest, 2, 136, 2);
    let now = 2 * INTERVAL + 1;
    for policy in POLICIES {
        let mut work = Work::default();
        let mut a = receiver(7, &initial, policy, RevocationPolicy::Cascade);
        let mut b = receiver(8, &initial, policy, RevocationPolicy::Cascade);
        let mut c = receiver(9, &initial, policy, RevocationPolicy::Cascade);
        feed(&mut a, &honest, now, &mut work);
        feed(&mut b, &clone, now, &mut work);
        feed(&mut c, &honest, now, &mut work);
        bind_sender(&mut a, &honest, now, &mut work);
        bind_sender(&mut b, &clone, now, &mut work);
        let (view_a, view_b) = (a.view.clone(), b.view.clone());

        // Exchange heads (the evidence each holds).
        let to_a = flag_page(&mut a, &[b.evidence[1].clone()], now, &mut work);
        let to_b = flag_page(&mut b, &[a.evidence[1].clone()], now, &mut work);
        assert_eq!(
            (to_a.kept.clone(), to_a.other.clone()),
            (to_b.other, to_b.kept)
        );
        // A third receiver learns of the fork from A's retained conflict evidence.
        flag_page(&mut c, &[a.conflicts[0].other.clone()], now, &mut work);

        // Detection is not convergence; nobody switched.
        assert_eq!(a.view, view_a);
        assert_eq!(b.view, view_b);
        assert_ne!(a.view, b.view);
        match policy {
            ForkPolicy::FreezePath => {
                assert_eq!((a.route(now), b.route(now)), (None, None));
            }
            ForkPolicy::KeepHead => {
                assert_eq!(a.route(now), Some(honest.newest().claims.endpoint));
                assert_eq!(b.route(now), Some(clone.newest().claims.endpoint));
            }
        }
    }
}

/// STRAWMAN, deliberately NOT a candidate: any owner-signed record at >= the current
/// generation replaces the head (last arrival wins). Used only to show what it would grant.
fn last_arrival_wins(view: &View, permit: &Permit) -> View {
    let key = identity::public_key_from_did(&view.owner).unwrap();
    assert!(identity::verify(
        &key,
        &Permit::bytes(&permit.claims),
        &permit.signature
    ));
    View {
        head: permit.id(),
        current_id: permit.claims.replacement_id,
        endpoint: permit.claims.endpoint,
        generation: permit.claims.generation,
        expires_at: permit.claims.expires_at,
        ..view.clone()
    }
}

// KIND: invariant
#[test]
fn last_arrival_wins_is_rejected_even_for_a_longer_later_signed_branch() {
    let (_alice, initial, honest) = honest(37, 2);
    let now = 2 * INTERVAL + 5;
    // Attacker branch: same parent as gen 2, issued LATER and extended FURTHER, all valid now.
    let mut attacker = honest.fork_at(2, 137);
    for t in [now - 1, now - 1, now - 1, now - 1, now] {
        attacker.rotate(t);
    }
    assert!(attacker.permit(2).claims.issued_at > honest.permit(2).claims.issued_at);
    assert_eq!(attacker.view.generation, 6);
    let strawman = attacker
        .history
        .iter()
        .skip(1)
        .fold(initial.clone(), |v, p| last_arrival_wins(&v, p));
    assert_eq!(strawman.endpoint, attacker.newest().claims.endpoint);

    for policy in POLICIES {
        let mut work = Work::default();
        let mut r = receiver(7, &initial, policy, RevocationPolicy::Cascade);
        feed(&mut r, &honest, now, &mut work);
        bind_sender(&mut r, &honest, now, &mut work);
        flag_page(&mut r, &attacker.pages_since(1)[0], now, &mut work);
        let child = [attacker.permit(3).clone()];
        assert_eq!(reject_page(&mut r, &child, now, &mut work), {
            match policy {
                ForkPolicy::FreezePath => "frozen",
                ForkPolicy::KeepHead => "predecessor",
            }
        });
        assert_eq!(
            attempt_bind(&mut r, attacker.newest(), attacker.key(), now, &mut work),
            Err(if policy == ForkPolicy::FreezePath {
                "frozen"
            } else {
                "not_head"
            })
        );
        assert_eq!(r.view, honest.view);
        assert_ne!(r.view, strawman);
        assert_ne!(r.route(now), Some(attacker.newest().claims.endpoint));
    }
}

// KIND: invariant
#[test]
fn forged_fork_and_revocation_evidence_is_rejected_with_state_unchanged() {
    let (alice, initial, honest) = honest(38, 3);
    let mallory = owner(99);
    let now = 3 * INTERVAL + 1;
    let fork = clone_branch(&honest, 3, 138, 3).newest().clone();
    let mut tampered = fork.clone();
    tampered.signature[0] ^= 1;
    let foreign = Permit::issue(&mallory, fork.claims.clone());
    let mut other_audience = fork.claims.clone();
    other_audience.audience = "group:g1".into();
    let other_audience = Permit::issue(&alice, other_audience);
    // Sibling device path of the same owner: same generation, different anchor/predecessor.
    let mut sibling = Sender::new(&alice, 238, View::initial(&alice, 2, "private"));
    sibling.rotate_until(3);

    for policy in POLICIES {
        let mut work = Work::default();
        let mut r = receiver(7, &initial, policy, RevocationPolicy::Cascade);
        feed(&mut r, &honest, now, &mut work);
        bind_sender(&mut r, &honest, now, &mut work);
        for (permit, error) in [
            (&tampered, "owner_signature"),
            (&foreign, "owner_signature"),
            (&other_audience, "scope"),
            (sibling.newest(), "fork"),
        ] {
            assert_eq!(
                reject_page(&mut r, &[permit.clone()], now, &mut work),
                error
            );
        }
        // Non-mnemonic parties cannot freeze or revoke the path.
        assert!(r.conflicts.is_empty());
        assert_eq!(r.route(now), Some(honest.newest().claims.endpoint));
        let target = honest.permit(2).id();
        let mallory_rev = Revocation::issue(&AuthSigner::Did(mallory.clone()), &r.view, target);
        let mut wrong_owner = mallory_rev.clone();
        wrong_owner.claims.owner = mallory.did().into();
        let key_rev = Revocation::issue(&AuthSigner::Key(fixture_key(38, 3)), &r.view, target);
        let sibling_scope =
            Revocation::issue(&AuthSigner::Did(alice.clone()), &sibling.view, target);
        for (rev, error) in [
            (&mallory_rev, "authority"),
            (&wrong_owner, "authority"),
            (&key_rev, "authority"),
            (&sibling_scope, "scope"),
        ] {
            assert_eq!(reject_revocation(&mut r, rev, &mut work), error);
        }
        assert_eq!(r.route(now), Some(honest.newest().claims.endpoint));
    }
}

// ---------------------------------------------------------------------------------------
// Tests: revocation after descendants were accepted
// ---------------------------------------------------------------------------------------

// KIND: candidate-policy G-REVOCATION
#[test]
fn revocation_option_a_cascade_invalidates_descendants_until_owner_signed_recovery() {
    let (alice, initial, honest) = honest(41, 6);
    let did = AuthSigner::Did(alice.clone());
    let now = 6 * INTERVAL + 1;
    let mut work = Work::default();
    let mut base = receiver(
        7,
        &initial,
        ForkPolicy::FreezePath,
        RevocationPolicy::Cascade,
    );
    feed(&mut base, &honest, now, &mut work);
    bind_sender(&mut base, &honest, now, &mut work);

    // (1) Older intermediate gen 3 revoked after gens 4..6 were accepted.
    let mut r = base.clone();
    r.accept_revocation(
        &Revocation::issue(&did, &r.view, honest.permit(3).id()),
        &mut work,
    )
    .unwrap();
    assert_eq!(
        r.suspended,
        Some(Suspension {
            revoked: honest.permit(3).id(),
            generation: 3
        })
    );
    assert_eq!(r.route(now), None);
    let mut extended = honest.clone();
    extended.rotate(now);
    assert_eq!(
        reject_page(&mut r, &[extended.newest().clone()], now, &mut work),
        "suspended"
    );
    assert_eq!(
        attempt_bind(&mut r, honest.newest(), honest.key(), now, &mut work),
        Err("suspended")
    );
    // A receiver still behind the revoked record cannot page through it.
    let mut behind = receiver(
        8,
        &initial,
        ForkPolicy::FreezePath,
        RevocationPolicy::Cascade,
    );
    behind.revoked = r.revoked.clone();
    assert_eq!(
        reject_page(&mut behind, &honest.pages_since(0)[0], now, &mut work),
        "revoked"
    );
    // Recovery: fresh successor of gen 2, plus an owner-signed recovery naming it.
    let mut recovered = honest.fork_at(3, 141);
    recovered.rotate(now);
    let rec = Recovery::issue(&did, &r.view, honest.permit(3).id(), recovered.newest());
    let wrong_target = Recovery::issue(&did, &r.view, honest.permit(4).id(), recovered.newest());
    assert_eq!(
        reject_recovery(&mut r, &wrong_target, recovered.newest(), now, &mut work),
        "recovery_target"
    );
    assert_eq!(
        reject_recovery(&mut r, &rec, extended.newest(), now, &mut work),
        "recovery_replacement"
    );
    r.accept_recovery(&rec, recovered.newest(), now, &mut work)
        .unwrap();
    assert_eq!(r.view, recovered.view);
    assert_eq!(r.evidence, recovered.history); // descendants 4..6 dropped
    assert_eq!(r.route(now), None); // needs a fresh live binding
    bind_sender(&mut r, &recovered, now, &mut work);
    assert_eq!(r.route(now), Some(recovered.newest().claims.endpoint));
    // Late delivery of the revoked branch is neither accepted nor flagged as a new fork.
    assert_eq!(
        reject_page(&mut r, &[honest.permit(3).clone()], now, &mut work),
        "revoked"
    );
    assert!(r.conflicts.is_empty());

    // (2) Head gen 6 revoked: recovery from gen 5.
    let mut r = base.clone();
    r.accept_revocation(
        &Revocation::issue(&did, &r.view, honest.newest().id()),
        &mut work,
    )
    .unwrap();
    assert_eq!(r.route(now), None);
    let mut recovered = honest.fork_at(6, 142);
    recovered.rotate(now);
    let rec = Recovery::issue(&did, &r.view, honest.newest().id(), recovered.newest());
    r.accept_recovery(&rec, recovered.newest(), now, &mut work)
        .unwrap();
    bind_sender(&mut r, &recovered, now, &mut work);
    assert_eq!(r.route(now), Some(recovered.newest().claims.endpoint));

    // (3) Anchor (nomination) revoked: the whole path is suspended; succession cannot repair it.
    let mut r = base.clone();
    r.accept_revocation(&Revocation::issue(&did, &r.view, initial.anchor), &mut work)
        .unwrap();
    assert_eq!(r.suspended.as_ref().unwrap().generation, 0);
    assert_eq!(r.route(now), None);
    let mut recovered = honest.fork_at(1, 143);
    recovered.rotate(now);
    let rec = Recovery::issue(&did, &r.view, initial.anchor, recovered.newest());
    assert_eq!(
        reject_recovery(&mut r, &rec, recovered.newest(), now, &mut work),
        "renomination_required"
    );
}

// KIND: candidate-policy G-REVOCATION
#[test]
fn revocation_option_b_blocks_only_that_records_endpoint_and_descendants_stand() {
    let (alice, initial, honest) = honest(42, 6);
    let did = AuthSigner::Did(alice.clone());
    let now = 6 * INTERVAL + 1;
    let mut work = Work::default();
    let mut base = receiver(
        7,
        &initial,
        ForkPolicy::FreezePath,
        RevocationPolicy::EndpointOnly,
    );
    feed(&mut base, &honest, now, &mut work);
    bind_sender(&mut base, &honest, now, &mut work);
    let mut next = honest.clone();
    next.rotate(now);

    // (1) Older intermediate gen 3: no effect on the current route or on extension.
    let mut r = base.clone();
    r.accept_revocation(
        &Revocation::issue(&did, &r.view, honest.permit(3).id()),
        &mut work,
    )
    .unwrap();
    assert_eq!(r.suspended, None);
    assert_eq!(r.route(now), Some(honest.newest().claims.endpoint));
    assert_eq!(
        r.offer_page(&[next.newest().clone()], now, &mut work),
        Ok(Outcome::Accepted)
    );
    // A receiver behind the revoked record still pages through it.
    let mut behind = receiver(
        8,
        &initial,
        ForkPolicy::FreezePath,
        RevocationPolicy::EndpointOnly,
    );
    behind.revoked = r.revoked.clone();
    feed(&mut behind, &next, now, &mut work);
    assert_eq!(behind.view, next.view);

    // (2) Head gen 6: its endpoint can no longer bind; an ordinary successor restores routing.
    let mut r = base.clone();
    r.accept_revocation(
        &Revocation::issue(&did, &r.view, honest.newest().id()),
        &mut work,
    )
    .unwrap();
    assert_eq!(r.route(now), None);
    assert_eq!(
        attempt_bind(&mut r, honest.newest(), honest.key(), now, &mut work),
        Err("revoked")
    );
    assert_eq!(
        r.offer_page(&[next.newest().clone()], now, &mut work),
        Ok(Outcome::Accepted)
    );
    bind_sender(&mut r, &next, now, &mut work);
    assert_eq!(r.route(now), Some(next.newest().claims.endpoint));

    // (3) Anchor: the nomination record has no endpoint; descendants stand.
    let mut r = base.clone();
    r.accept_revocation(&Revocation::issue(&did, &r.view, initial.anchor), &mut work)
        .unwrap();
    assert_eq!(r.route(now), Some(honest.newest().claims.endpoint));
    // Recovery records have no meaning under this option.
    let rec = Recovery::issue(&did, &r.view, initial.anchor, next.newest());
    assert_eq!(
        reject_recovery(&mut r, &rec, next.newest(), now, &mut work),
        "no_recovery_in_policy"
    );
}

// KIND: invariant
#[test]
fn holder_of_only_an_old_transport_key_cannot_bind_extend_or_revoke_under_either_option() {
    let (_alice, initial, mut honest) = honest(43, 5);
    let old_key = fixture_key(43, 4); // retired gen-4 endpoint key; NO mnemonic
    let now = 5 * INTERVAL + 1;
    for rev in [RevocationPolicy::Cascade, RevocationPolicy::EndpointOnly] {
        let mut work = Work::default();
        let mut r = receiver(7, &initial, ForkPolicy::KeepHead, rev);
        feed(&mut r, &honest, now, &mut work);
        bind_sender(&mut r, &honest, now, &mut work);
        assert_eq!(
            attempt_bind(&mut r, honest.permit(4), &old_key, now, &mut work),
            Err("not_head")
        );
        assert_eq!(
            attempt_bind(&mut r, honest.newest(), &old_key, now, &mut work),
            Err("possession")
        );
        // A "permit" signed with the transport key is not an owner signature.
        let mut forged = honest.clone();
        forged.rotate(now);
        let forged = Permit {
            claims: forged.newest().claims.clone(),
            signature: old_key
                .sign(&Permit::bytes(&forged.newest().claims))
                .to_bytes(),
        };
        assert_eq!(
            reject_page(&mut r, &[forged], now, &mut work),
            "owner_signature"
        );
        let rev_by_key = Revocation::issue(
            &AuthSigner::Key(old_key.clone()),
            &r.view,
            honest.newest().id(),
        );
        assert_eq!(
            reject_revocation(&mut r, &rev_by_key, &mut work),
            "authority"
        );
        assert_eq!(r.route(now), Some(honest.newest().claims.endpoint));
    }
    // After a further rotation the old key is two generations stale: still nothing.
    honest.rotate(6 * INTERVAL);
    let later = 6 * INTERVAL + 1;
    let mut work = Work::default();
    let mut r = receiver(
        8,
        &initial,
        ForkPolicy::KeepHead,
        RevocationPolicy::EndpointOnly,
    );
    feed(&mut r, &honest, later, &mut work);
    assert_eq!(
        attempt_bind(&mut r, honest.newest(), &old_key, later, &mut work),
        Err("possession")
    );
}

// KIND: limitation
#[test]
fn holder_of_revoked_device_key_and_shared_mnemonic_is_not_excluded_by_either_option() {
    // The stolen device was the gen-4 head: it holds the gen-4 endpoint key AND the mnemonic.
    let (alice, initial, honest) = honest(44, 4);
    let mnemonic = AuthSigner::Did(alice.clone()); // indistinguishable from the owner's signer
    let stolen_key = fixture_key(44, 4);
    let now = 4 * INTERVAL + 1;
    let revoke_head = Revocation::issue(&mnemonic, &initial, honest.newest().id());
    let mut honest_next = honest.clone();
    honest_next.rotate(now);

    // Option (b) EndpointOnly: the revoked key is refused, but the mnemonic signs a successor.
    let mut work = Work::default();
    let mut b = receiver(
        7,
        &initial,
        ForkPolicy::KeepHead,
        RevocationPolicy::EndpointOnly,
    );
    feed(&mut b, &honest, now, &mut work);
    bind_sender(&mut b, &honest, now, &mut work);
    b.accept_revocation(&revoke_head, &mut work).unwrap();
    assert_eq!(
        attempt_bind(&mut b, honest.newest(), &stolen_key, now, &mut work),
        Err("revoked")
    );
    let mut thief = honest.fork_at(5, 144);
    thief.rotate(now);
    assert_eq!(
        b.offer_page(&[thief.newest().clone()], now, &mut work),
        Ok(Outcome::Accepted)
    );
    bind_sender(&mut b, &thief, now, &mut work);
    assert_eq!(b.route(now), Some(thief.newest().claims.endpoint));
    // The honest successor arriving second is merely flagged.
    flag_page(&mut b, &[honest_next.newest().clone()], now, &mut work);
    assert_eq!(b.route(now), Some(thief.newest().claims.endpoint));

    // Option (a) Cascade: descendants of the revoked head are refused, but the mnemonic also
    // signs a "recovery" naming the thief's own replacement.
    let mut a = receiver(8, &initial, ForkPolicy::KeepHead, RevocationPolicy::Cascade);
    feed(&mut a, &honest, now, &mut work);
    bind_sender(&mut a, &honest, now, &mut work);
    a.accept_revocation(&revoke_head, &mut work).unwrap();
    assert_eq!(
        reject_page(&mut a, &[thief.newest().clone()], now, &mut work),
        "suspended"
    );
    let mut thief_recovery = honest.fork_at(4, 145);
    thief_recovery.rotate(now);
    let rec = Recovery::issue(
        &mnemonic,
        &a.view,
        honest.newest().id(),
        thief_recovery.newest(),
    );
    a.accept_recovery(&rec, thief_recovery.newest(), now, &mut work)
        .unwrap();
    bind_sender(&mut a, &thief_recovery, now, &mut work);
    assert_eq!(a.route(now), Some(thief_recovery.newest().claims.endpoint));
    // The owner's own recovery arrives second: no longer suspended; as a page it is a fork.
    let mut owner_recovery = honest.fork_at(4, 146);
    owner_recovery.rotate(now);
    let owner_rec = Recovery::issue(
        &mnemonic,
        &a.view,
        honest.newest().id(),
        owner_recovery.newest(),
    );
    assert_eq!(
        reject_recovery(&mut a, &owner_rec, owner_recovery.newest(), now, &mut work),
        "not_suspended"
    );
    flag_page(&mut a, &[owner_recovery.newest().clone()], now, &mut work);
    assert_eq!(a.route(now), Some(thief_recovery.newest().claims.endpoint));

    // And the thief can revoke the OWNER's records just as easily (denial of service).
    let mut c = receiver(9, &initial, ForkPolicy::KeepHead, RevocationPolicy::Cascade);
    feed(&mut c, &honest, now, &mut work);
    bind_sender(&mut c, &honest, now, &mut work);
    c.accept_revocation(
        &Revocation::issue(&mnemonic, &c.view, honest.permit(2).id()),
        &mut work,
    )
    .unwrap();
    assert_eq!(c.route(now), None);
}

// KIND: candidate-policy G-AUTHORITY
#[test]
fn separate_recovery_authority_blocks_mnemonic_revocation_and_recovery_but_not_mnemonic_permits() {
    // HYPOTHETICAL option: revocation/recovery signed by a key linked devices do not hold.
    // Fixture key from a fixed seed (test determinism only; NOT a proposed derivation).
    let (alice, initial, honest) = honest(45, 4);
    let recovery_key = SigningKey::from_bytes(&fixture_seed(
        b"test-only/hypothetical-recovery-authority\0",
        45,
        0,
    ));
    let authority = AuthSigner::Key(recovery_key.clone());
    let mnemonic = AuthSigner::Did(alice.clone());
    let now = 4 * INTERVAL + 1;
    let mut work = Work::default();

    // Option OwnerDid (current model): a mnemonic-signed revocation is accepted.
    let mut did_r = receiver(7, &initial, ForkPolicy::KeepHead, RevocationPolicy::Cascade);
    feed(&mut did_r, &honest, now, &mut work);
    did_r
        .accept_revocation(
            &Revocation::issue(&mnemonic, &initial, honest.newest().id()),
            &mut work,
        )
        .unwrap();
    assert!(did_r.suspended.is_some());

    // Option SeparateRecoveryKey.
    let mut r = Receiver::new(
        8,
        &initial,
        ForkPolicy::KeepHead,
        RevocationPolicy::Cascade,
        Authority::SeparateRecoveryKey(recovery_key.verifying_key().to_bytes()),
    );
    feed(&mut r, &honest, now, &mut work);
    bind_sender(&mut r, &honest, now, &mut work);
    let thief_rev = Revocation::issue(&mnemonic, &r.view, honest.newest().id());
    assert_eq!(
        reject_revocation(&mut r, &thief_rev, &mut work),
        "authority"
    );
    r.accept_revocation(
        &Revocation::issue(&authority, &r.view, honest.newest().id()),
        &mut work,
    )
    .unwrap();
    assert_eq!(r.route(now), None);
    let mut thief = honest.fork_at(4, 147);
    thief.rotate(now);
    let thief_rec = Recovery::issue(&mnemonic, &r.view, honest.newest().id(), thief.newest());
    assert_eq!(
        reject_recovery(&mut r, &thief_rec, thief.newest(), now, &mut work),
        "authority"
    );
    let mut recovered = honest.fork_at(4, 148);
    recovered.rotate(now);
    let rec = Recovery::issue(
        &authority,
        &r.view,
        honest.newest().id(),
        recovered.newest(),
    );
    r.accept_recovery(&rec, recovered.newest(), now, &mut work)
        .unwrap();
    bind_sender(&mut r, &recovered, now, &mut work);
    // The thief's mnemonic-signed sibling of the recovered record is only a flagged fork...
    flag_page(&mut r, &[thief.newest().clone()], now, &mut work);
    assert_eq!(r.route(now), Some(recovered.newest().claims.endpoint));
    // ...but permits are still DID-signed: the thief can EXTEND the recovered head first.
    let later = 5 * INTERVAL + 1;
    let mut thief_ext = recovered.clone();
    thief_ext.seed = 149;
    thief_ext.rotate(5 * INTERVAL);
    assert_eq!(
        r.offer_page(&[thief_ext.newest().clone()], later, &mut work),
        Ok(Outcome::Accepted)
    );
    bind_sender(&mut r, &thief_ext, later, &mut work);
    assert_eq!(r.route(later), Some(thief_ext.newest().claims.endpoint));
}

// ---------------------------------------------------------------------------------------
// Tests: group assignment boundary (D06 rotation side) and B1 checkpoint limitation
// ---------------------------------------------------------------------------------------

/// Group-side fixture (NOT a group permit verifier). Assignment state is owned by group policy.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Assignment {
    group: String,
    owner: String,
    nominated_anchor: Id,
    revoked: bool,
}

/// Group contract item 3: nominated device's verified path AND a still-authorized assignment.
fn can_host(r: &Receiver, a: &Assignment, now: u64) -> bool {
    !a.revoked
        && r.view.owner == a.owner
        && r.view.audience == a.group
        && r.view.anchor == a.nominated_anchor
        && r.route(now).is_some()
}

// KIND: invariant
#[test]
fn revoked_host_assignment_is_not_restored_by_any_succession() {
    let alice = owner(61);
    let group = "group:g1";
    let initial = View::initial(&alice, 50, group);
    let mut sender = Sender::new(&alice, 61, initial.clone());
    sender.rotate_until(2);
    let did = AuthSigner::Did(alice.clone());
    let mut work = Work::default();
    let mut r = receiver(7, &initial, ForkPolicy::KeepHead, RevocationPolicy::Cascade);
    let now = 2 * INTERVAL + 1;
    feed(&mut r, &sender, now, &mut work);
    bind_sender(&mut r, &sender, now, &mut work);
    let mut assignment = Assignment {
        group: group.into(),
        owner: alice.did().into(),
        nominated_anchor: initial.anchor,
        revoked: false,
    };
    assert!(can_host(&r, &assignment, now));
    assignment.revoked = true;
    let revoked_assignment = assignment.clone();
    assert!(!can_host(&r, &assignment, now));

    // 1. Ordinary succession.
    let now = 3 * INTERVAL + 1;
    sender.rotate(3 * INTERVAL);
    feed(&mut r, &sender, now, &mut work);
    bind_sender(&mut r, &sender, now, &mut work);
    assert!(r.route(now).is_some());
    assert!(!can_host(&r, &assignment, now));
    // 2. Cascade revocation of the head plus owner-signed recovery.
    r.accept_revocation(
        &Revocation::issue(&did, &r.view, sender.newest().id()),
        &mut work,
    )
    .unwrap();
    let mut recovered = sender.fork_at(3, 161);
    recovered.rotate(now);
    let rec = Recovery::issue(&did, &r.view, sender.newest().id(), recovered.newest());
    r.accept_recovery(&rec, recovered.newest(), now, &mut work)
        .unwrap();
    bind_sender(&mut r, &recovered, now, &mut work);
    assert!(r.route(now).is_some());
    assert!(!can_host(&r, &assignment, now));
    // 3. A competing branch (flagged; keep-head keeps the route).
    let fork = clone_branch(&recovered, 3, 162, 3);
    flag_page(&mut r, &[fork.newest().clone()], now, &mut work);
    assert!(!can_host(&r, &assignment, now));
    // 4. A sibling path of the same DID with its own live route.
    let sibling_initial = View::initial(&alice, 51, group);
    let mut sibling = Sender::new(&alice, 163, sibling_initial.clone());
    sibling.rotate(3 * INTERVAL);
    let mut s = receiver(
        8,
        &sibling_initial,
        ForkPolicy::KeepHead,
        RevocationPolicy::Cascade,
    );
    feed(&mut s, &sibling, now, &mut work);
    bind_sender(&mut s, &sibling, now, &mut work);
    assert!(s.route(now).is_some());
    assert!(!can_host(&s, &assignment, now));
    let unrevoked = Assignment {
        revoked: false,
        ..assignment.clone()
    };
    assert!(!can_host(&s, &unrevoked, now)); // sibling never inherits the nomination
    // No succession, revocation or recovery operation touches group assignment state.
    assert_eq!(assignment, revoked_assignment);
}

// KIND: limitation
#[test]
fn strict_history_flags_a_fork_between_receiver_head_and_checkpoint_that_checkpoints_miss() {
    // Receivers know honest gens 1..4. A mnemonic-holding clone diverges at gen 5 (parent gen 4),
    // i.e. strictly between every receiver's head and the later checkpoints.
    let (_alice, initial, honest) = honest(71, 12);
    let clone = clone_branch(&honest, 5, 171, 13);
    let now = 13 * INTERVAL + 1;
    let mut work = Work::default();
    let start = |name| {
        let mut r = receiver(
            name,
            &initial,
            ForkPolicy::KeepHead,
            RevocationPolicy::Cascade,
        );
        let mut w = Work::default();
        assert_eq!(
            r.offer_page(&honest.pages_since(0)[0], now, &mut w),
            Ok(Outcome::Accepted)
        );
        assert_eq!(r.cursor(), 4);
        r
    };

    // Strict: honest pages to 12, then the clone's pages from the shared parent -> flagged.
    let mut strict = start(1);
    feed(&mut strict, &honest, now, &mut work);
    let conflict = flag_page(&mut strict, &clone.pages_since(4)[0], now, &mut work);
    assert_eq!(conflict.generation, 5);
    assert_eq!(strict.view, honest.view);

    // Checkpoint: honest cp@12 accepted, then the clone's HIGHER cp@13 silently replaces it.
    // Anti-rollback by generation behaves as "highest generation wins" across branches.
    let mut cp = start(2);
    let (h12, h12_head) = honest.checkpoint_at(12);
    cp.accept_checkpoint(&h12, &h12_head, now, &mut work)
        .unwrap();
    let (c13, c13_head) = clone.checkpoint_at(13);
    cp.accept_checkpoint(&c13, &c13_head, now, &mut work)
        .unwrap();
    assert_eq!(cp.view, clone.view);
    assert!(cp.conflicts.is_empty());

    // Two checkpoint receivers on different branches at different generations exchange heads:
    // no fork proof is possible (no retained record shares a parent); plain rejections only.
    let mut on_honest = start(3);
    on_honest
        .accept_checkpoint(&h12, &h12_head, now, &mut work)
        .unwrap();
    let mut on_clone = start(4);
    let (c11, c11_head) = clone.checkpoint_at(11);
    on_clone
        .accept_checkpoint(&c11, &c11_head, now, &mut work)
        .unwrap();
    assert_eq!(
        reject_page(&mut on_honest, &[c11_head.clone()], now, &mut work),
        "stale"
    );
    assert_eq!(
        reject_page(&mut on_clone, &[h12_head.clone()], now, &mut work),
        "predecessor"
    );
    assert!(on_honest.conflicts.is_empty() && on_clone.conflicts.is_empty());
    assert_ne!(on_honest.view.head, on_clone.view.head);
    // Even their checkpoints are not comparable: different generations, so neither is "fork".
    let mut probe = on_honest.clone();
    assert_eq!(
        probe.accept_checkpoint(&c11, &c11_head, now, &mut work),
        Err("stale")
    );
}
