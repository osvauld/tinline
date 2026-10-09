//! Candidate succession-permit semantics, NOT a production wire format or courier port.
//! Follows courier's domain-separated signed-claims pattern. Succession is deliberately
//! not a delegation chain. Expired records may prove history, never current access.
use std::collections::HashSet;

use ed25519_dalek::{Signer, SigningKey};
use identity::Identity;
use serde::Serialize;
use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"test-only/device-succession-permit/v1\0";
const PAGE: usize = 4;

#[path = "support/rotation_permit_network.rs"]
mod network;

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
        // Like courier: ID names signed payload, not signature representation.
        Sha256::digest(Self::bytes(&self.claims)).into()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct View {
    owner: String,
    audience: String,
    // Anchor is a nominated historical record, NOT a permanent current device ID.
    anchor: [u8; 32],
    head: [u8; 32],
    current_id: [u8; 32],
    endpoint: [u8; 32],
    generation: u64,
    expires_at: u64,
}

impl View {
    fn initial(owner: &Identity, device_id: u8) -> Self {
        // These anchors stand for already authenticated initial nominations.
        let anchor = Sha256::digest([owner.did().as_bytes(), &[device_id]].concat()).into();
        Self { owner: owner.did().into(), audience: "private".into(), anchor, head: anchor,
            current_id: [device_id; 32], endpoint: [0; 32], generation: 0, expires_at: 0 }
    }

    fn page(&mut self, permits: &[Permit], now: u64, revoked: &HashSet<[u8; 32]>) -> Result<(), &'static str> {
        if permits.is_empty() || permits.len() > PAGE { return Err("page_limit"); }
        let mut candidate = self.clone();
        for permit in permits {
            let c = &permit.claims;
            let key = identity::public_key_from_did(&candidate.owner).unwrap();
            if c.owner != candidate.owner || !identity::verify(&key, &Permit::bytes(c), &permit.signature) {
                return Err("owner_signature");
            }
            if c.version != 1 || c.audience != candidate.audience { return Err("scope"); }
            if c.issued_at >= c.expires_at || c.issued_at > now { return Err("validity"); }
            if revoked.contains(&candidate.anchor) || revoked.contains(&candidate.head) || revoked.contains(&permit.id()) {
                return Err("revoked");
            }
            if c.generation == candidate.generation {
                if permit.id() == candidate.head { continue; }
                return Err("fork");
            }
            if c.generation < candidate.generation { return Err("stale"); }
            if candidate.generation.checked_add(1) != Some(c.generation) { return Err("gap"); }
            if c.predecessor != candidate.head || c.previous_id != candidate.current_id {
                return Err("predecessor");
            }
            if c.replacement_id == c.previous_id { return Err("unchanged_id"); }
            candidate.head = permit.id();
            candidate.current_id = c.replacement_id;
            candidate.endpoint = c.endpoint;
            candidate.generation = c.generation;
            candidate.expires_at = c.expires_at;
        }
        // Atomic model transition. Expired history advances knowledge, NOT live permission.
        *self = candidate;
        Ok(())
    }

    fn live(&self, now: u64, revoked: &HashSet<[u8; 32]>, challenge: &[u8], proof: &[u8; 64]) -> bool {
        // Simplified possession fixture. Production must bind a fresh challenge to the
        // connection/context (or use iroh's authenticated transport), not accept arbitrary bytes.
        self.generation > 0 && now < self.expires_at
            && !revoked.contains(&self.anchor) && !revoked.contains(&self.head)
            && identity::verify(&self.endpoint, challenge, proof)
    }
}

fn next(owner: &Identity, previous: &View, id: u8, issued_at: u64, expires_at: u64) -> (Permit, SigningKey) {
    let key = SigningKey::from_bytes(&proto::new_device_secret());
    let p = Permit::issue(owner, Claims {
        version: 1, owner: owner.did().into(), audience: previous.audience.clone(),
        previous_id: previous.current_id, replacement_id: [id; 32],
        endpoint: key.verifying_key().to_bytes(), predecessor: previous.head,
        generation: previous.generation + 1, issued_at, expires_at,
    });
    (p, key)
}

#[test]
fn owner_replaces_id_and_proves_new_endpoint_possession_not_old_key_possession() {
    let (alice, _) = identity::generate();
    let mut view = View::initial(&alice, 1);
    let (permit, key) = next(&alice, &view, 2, 1, 20);
    view.page(&[permit], 2, &HashSet::new()).unwrap();
    assert_eq!(view.current_id, [2; 32]);
    let challenge = b"test-only/fresh-session-123";
    assert!(view.live(2, &HashSet::new(), challenge, &key.sign(challenge).to_bytes()));
    let wrong = SigningKey::from_bytes(&proto::new_device_secret());
    assert!(!view.live(2, &HashSet::new(), challenge, &wrong.sign(challenge).to_bytes()));
    assert!(!view.live(20, &HashSet::new(), challenge, &key.sign(challenge).to_bytes()));
}

#[test]
fn sibling_path_and_other_owner_cannot_replace_nominated_device() {
    let (alice, _) = identity::generate();
    let (other, _) = identity::generate();
    let initial = View::initial(&alice, 1);
    let sibling = View::initial(&alice, 3);
    let (sibling_permit, _) = next(&alice, &sibling, 4, 1, 20);
    let (valid, _) = next(&alice, &initial, 2, 1, 20);
    for (p, error) in [(sibling_permit, "predecessor"),
                       (Permit::issue(&other, valid.claims), "owner_signature")] {
        let mut view = initial.clone();
        assert_eq!(view.page(&[p], 2, &HashSet::new()), Err(error));
        assert_eq!(view, initial);
    }
}

#[test]
fn expired_history_can_catch_up_past_delegation_depth_but_cannot_authorize_live_use() {
    let (alice, _) = identity::generate();
    let initial = View::initial(&alice, 1);
    let mut issuer = initial.clone();
    let mut history = Vec::new();
    let mut latest_key = None;
    // 12 rotations, intentionally greater than courier's delegation limit of eight.
    for generation in 1..=12u8 {
        let (p, key) = next(&alice, &issuer, generation + 1, generation as u64, generation as u64 + 2);
        issuer.page(std::slice::from_ref(&p), generation as u64, &HashSet::new()).unwrap();
        history.push(p);
        latest_key = Some(key);
    }
    let mut receiver = initial;
    for page in history.chunks(PAGE) { receiver.page(page, 13, &HashSet::new()).unwrap(); }
    assert_eq!(receiver, issuer);
    let challenge = b"test-only/reconnect";
    let sig = latest_key.unwrap().sign(challenge).to_bytes();
    assert!(receiver.live(13, &HashSet::new(), challenge, &sig));
    assert!(!receiver.live(14, &HashSet::new(), challenge, &sig));
}

#[test]
fn tampering_audience_fork_and_revoked_predecessor_leave_state_unchanged() {
    let (alice, _) = identity::generate();
    let mut initial = View::initial(&alice, 1);
    let (first, _) = next(&alice, &initial, 2, 1, 20);
    initial.page(std::slice::from_ref(&first), 2, &HashSet::new()).unwrap();
    let (second, _) = next(&alice, &initial, 3, 2, 20);
    let mut tampered = second.clone();
    tampered.claims.replacement_id[0] ^= 1;
    let mut foreign_scope = second.claims.clone();
    foreign_scope.audience = "group:g1".into();
    let mut fork = first.claims.clone();
    fork.replacement_id = [99; 32];
    for (p, revoked, error) in [
        (tampered, HashSet::new(), "owner_signature"),
        (Permit::issue(&alice, foreign_scope), HashSet::new(), "scope"),
        (Permit::issue(&alice, fork), HashSet::new(), "fork"),
        (second, HashSet::from([initial.head]), "revoked"),
    ] {
        let mut view = initial.clone();
        assert_eq!(view.page(&[p], 3, &revoked), Err(error));
        assert_eq!(view, initial);
    }
}

#[test]
fn host_rights_follow_only_nominated_path_and_are_not_restored_by_succession() {
    // Group nomination is a trusted policy fixture, NOT a group permit verifier.
    let (alice, _) = identity::generate();
    let mut nominated = View::initial(&alice, 1);
    let sibling = View::initial(&alice, 2);
    let nominated_anchor = nominated.anchor;
    let can_host = |view: &View, assignment_revoked: bool| {
        view.owner == alice.did() && view.anchor == nominated_anchor && !assignment_revoked
    };
    let (p, _) = next(&alice, &nominated, 3, 1, 20);
    nominated.page(&[p], 2, &HashSet::new()).unwrap();
    assert!(can_host(&nominated, false));
    assert!(!can_host(&sibling, false));
    assert!(!can_host(&nominated, true));
}

#[test]
fn invalid_later_record_rejects_entire_page_and_duplicate_is_idempotent() {
    let (alice, _) = identity::generate();
    let initial = View::initial(&alice, 1);
    let (p, _) = next(&alice, &initial, 2, 1, 20);
    let mut sender = initial.clone();
    sender.page(std::slice::from_ref(&p), 2, &HashSet::new()).unwrap();
    let (mut bad, _) = next(&alice, &sender, 3, 2, 20);
    bad.signature[0] ^= 1;
    let mut receiver = initial.clone();
    assert_eq!(receiver.page(&[p.clone(), bad], 3, &HashSet::new()), Err("owner_signature"));
    assert_eq!(receiver, initial);
    receiver.page(std::slice::from_ref(&p), 3, &HashSet::new()).unwrap();
    receiver.page(&[p], 3, &HashSet::new()).unwrap();
    assert_eq!(receiver, sender);
}
