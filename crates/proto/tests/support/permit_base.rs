//! Candidate succession-permit model shared by the rotation_* contract tests
//! (permit_model, time_contracts, storage_contracts). TEST-ONLY, NOT a production wire format.
//!
//! This is the "base" variant: `Permit` has no `PartialEq`/`Debug`, `View::initial(owner,
//! device_id)` is always the `private` audience, and `View` is `Serialize` so rejection tests
//! can compare bytes. The history/paging family (challenge, fork, paging, adversarial) uses the
//! richer `permit_history` variant instead; the two are kept apart on purpose because their
//! anchors, derives and verification entry points differ.
#![allow(dead_code)]
use std::collections::HashSet;

use identity::Identity;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const DOMAIN: &[u8] = b"test-only/device-succession-permit/v1\0";
pub const PAGE: usize = 4;

#[derive(Clone, Debug, Serialize)]
pub struct Claims {
    pub version: u8,
    pub owner: String,
    pub audience: String,
    pub previous_id: [u8; 32],
    pub replacement_id: [u8; 32],
    pub endpoint: [u8; 32],
    pub predecessor: [u8; 32],
    pub generation: u64,
    pub issued_at: u64,
    pub expires_at: u64,
}

#[derive(Clone)]
pub struct Permit {
    pub claims: Claims,
    pub signature: [u8; 64],
}

impl Permit {
    pub fn bytes(claims: &Claims) -> Vec<u8> {
        [DOMAIN, &serde_json::to_vec(claims).unwrap()].concat()
    }
    pub fn issue(owner: &Identity, claims: Claims) -> Self {
        let signature = owner.sign(&Self::bytes(&claims));
        Self { claims, signature }
    }
    pub fn id(&self) -> [u8; 32] {
        Sha256::digest(Self::bytes(&self.claims)).into()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct View {
    pub owner: String,
    pub audience: String,
    pub anchor: [u8; 32],
    pub head: [u8; 32],
    pub current_id: [u8; 32],
    pub endpoint: [u8; 32],
    pub generation: u64,
    pub expires_at: u64,
}

impl View {
    pub fn initial(owner: &Identity, device_id: u8) -> Self {
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

    pub fn page(
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

    pub fn live(
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

pub fn no_revocations() -> HashSet<[u8; 32]> {
    HashSet::new()
}
