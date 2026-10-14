//! Candidate history / live-binding model shared by the rotation_* contract tests
//! (challenge_contracts, fork_revocation, history_paging, history_adversarial). TEST-ONLY,
//! NOT a production wire format.
//!
//! Divergences between the former per-file copies, kept visible here:
//! - `Claims` derives `Deserialize` + `deny_unknown_fields` (history_adversarial needs it; the
//!   others do not use it, and serialization bytes are unchanged).
//! - `Work` carries `bytes_parsed` (history_adversarial only; the others leave it at 0).
//! - `Permit::wire_len` / `Checkpoint::wire_len` exist for history_paging's size comparison.
//! - challenge_contracts used a private-only `View::initial(owner, id)`; it is
//!   `View::initial_private` here.
#![allow(dead_code)]
use std::collections::BTreeSet;

use identity::Identity;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PERMIT_DOMAIN: &[u8] = b"test-only/device-succession-permit/v1\0";
pub const CHECKPOINT_DOMAIN: &[u8] = b"test-only/device-succession-checkpoint/v1\0";
pub const CHALLENGE_DOMAIN: &[u8] = b"test-only/rotation-live-binding/v1\0";
pub const PAGE: usize = 4;
pub const INTERVAL: u64 = 10;
pub const OVERLAP: u64 = 2;

pub type Id = [u8; 32];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claims {
    pub version: u8,
    pub owner: String,
    pub audience: String,
    pub previous_id: Id,
    pub replacement_id: Id,
    pub endpoint: Id,
    pub predecessor: Id,
    pub generation: u64,
    pub issued_at: u64,
    pub expires_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permit {
    pub claims: Claims,
    pub signature: [u8; 64],
}

impl Permit {
    pub fn bytes(claims: &Claims) -> Vec<u8> {
        [PERMIT_DOMAIN, &serde_json::to_vec(claims).unwrap()].concat()
    }
    pub fn issue(owner: &Identity, claims: Claims) -> Self {
        let signature = owner.sign(&Self::bytes(&claims));
        Self { claims, signature }
    }
    pub fn id(&self) -> Id {
        // Like courier: ID names signed payload, not signature representation.
        Sha256::digest(Self::bytes(&self.claims)).into()
    }
    pub fn wire_len(&self) -> usize {
        Self::bytes(&self.claims).len() + self.signature.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub owner: String,
    pub audience: String,
    // Anchor is a nominated historical record, NOT a permanent current device ID.
    pub anchor: Id,
    pub head: Id,
    pub current_id: Id,
    pub endpoint: Id,
    pub generation: u64,
    pub expires_at: u64,
}

impl View {
    /// The challenge_contracts variant: always the `private` audience, and the anchor does not
    /// hash the audience in (so its anchors differ from `initial(.., "private")`).
    pub fn initial_private(owner: &Identity, device_id: u8) -> Self {
        // Stands for an already authenticated initial nomination.
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

    pub fn initial(owner: &Identity, device_id: u8, audience: &str) -> Self {
        // Stands for an already authenticated initial nomination (not modeled here).
        let anchor =
            Sha256::digest([owner.did().as_bytes(), &[device_id], audience.as_bytes()].concat())
                .into();
        Self {
            owner: owner.did().into(),
            audience: audience.into(),
            anchor,
            head: anchor,
            current_id: [device_id; 32],
            endpoint: [0; 32],
            generation: 0,
            expires_at: 0,
        }
    }
}

/// Work counter kept OUTSIDE receiver state so rejected attempts can be compared byte-for-byte.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Work {
    pub signature_verifications: u64,
    /// Only the adversarial byte-cap tests use this; the other users never touch it.
    pub bytes_parsed: u64,
}

pub type Revoked = BTreeSet<Id>;

/// History only: signatures, predecessors, scope, ordering. No possession, no expiry gate.
pub fn verify_history_page(
    view: &View,
    page: &[Permit],
    now: u64,
    revoked: &Revoked,
    work: &mut Work,
) -> Result<View, &'static str> {
    if page.is_empty() || page.len() > PAGE {
        return Err("page_limit");
    }
    let key = identity::public_key_from_did(&view.owner).ok_or("owner_key")?;
    let mut candidate = view.clone();
    for permit in page {
        let c = &permit.claims;
        if c.owner != candidate.owner {
            return Err("owner_signature");
        }
        work.signature_verifications += 1;
        if !identity::verify(&key, &Permit::bytes(c), &permit.signature) {
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
    Ok(candidate)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Challenge {
    pub receiver: u64,
    pub nonce: u64,
    pub head: Id,
}

impl Challenge {
    pub fn bytes(&self) -> Vec<u8> {
        // Test-only: binds receiver, receiver-issued nonce and the verified head ID. C3 owns
        // the real session/challenge design; this is not a replay-safe handshake.
        [
            CHALLENGE_DOMAIN,
            &self.receiver.to_be_bytes(),
            &self.nonce.to_be_bytes(),
            &self.head,
        ]
        .concat()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveBinding {
    pub head: Id,
    pub generation: u64,
    pub endpoint: Id,
    pub expires_at: u64,
}

/// Live only: possession of the endpoint key named by the NEWEST verified head.
pub fn bind_live(
    view: &View,
    newest: &Permit,
    challenge: &Challenge,
    proof: &[u8; 64],
    now: u64,
    revoked: &Revoked,
    work: &mut Work,
) -> Result<LiveBinding, &'static str> {
    if view.generation == 0 {
        return Err("no_successor");
    }
    // `head` was set only from owner-verified claims; the ID hashes the claims, so equality
    // means these are exactly the verified claims (signature bytes are irrelevant here).
    if newest.id() != view.head {
        return Err("not_head");
    }
    if challenge.head != view.head {
        return Err("challenge_head");
    }
    if revoked.contains(&view.anchor) || revoked.contains(&view.head) {
        return Err("revoked");
    }
    let c = &newest.claims;
    if now < c.issued_at || now >= c.expires_at {
        return Err("expired");
    }
    work.signature_verifications += 1;
    if !identity::verify(&c.endpoint, &challenge.bytes(), proof) {
        return Err("possession");
    }
    Ok(LiveBinding {
        head: view.head,
        generation: view.generation,
        endpoint: c.endpoint,
        expires_at: c.expires_at,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CheckpointClaims {
    pub version: u8,
    pub owner: String,
    pub audience: String,
    pub anchor: Id,
    pub generation: u64,
    pub head: Id,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub claims: CheckpointClaims,
    pub signature: [u8; 64],
}

impl Checkpoint {
    pub fn bytes(claims: &CheckpointClaims) -> Vec<u8> {
        [CHECKPOINT_DOMAIN, &serde_json::to_vec(claims).unwrap()].concat()
    }
    pub fn wire_len(&self) -> usize {
        Self::bytes(&self.claims).len() + self.signature.len()
    }
}

/// CANDIDATE G-HISTORY option B: DID-signed checkpoint naming `{owner, audience, anchor,
/// generation, head}`, shipped with the head permit so later permits can chain to it.
/// It cannot show that the receiver's previous head is an ancestor of the checkpoint head.
pub fn verify_checkpoint(
    view: &View,
    cp: &Checkpoint,
    head_permit: &Permit,
    now: u64,
    revoked: &Revoked,
    work: &mut Work,
) -> Result<View, &'static str> {
    let key = identity::public_key_from_did(&view.owner).ok_or("owner_key")?;
    let k = &cp.claims;
    if k.owner != view.owner {
        return Err("owner_signature");
    }
    work.signature_verifications += 1;
    if !identity::verify(&key, &Checkpoint::bytes(k), &cp.signature) {
        return Err("owner_signature");
    }
    if k.version != 1 || k.audience != view.audience || k.anchor != view.anchor {
        return Err("scope");
    }
    let c = &head_permit.claims;
    if head_permit.id() != k.head || c.generation != k.generation {
        return Err("checkpoint_head");
    }
    if c.owner != view.owner {
        return Err("owner_signature");
    }
    work.signature_verifications += 1;
    if !identity::verify(&key, &Permit::bytes(c), &head_permit.signature) {
        return Err("owner_signature");
    }
    if c.version != 1 || c.audience != view.audience {
        return Err("scope");
    }
    if c.issued_at >= c.expires_at || c.issued_at > now {
        return Err("validity");
    }
    if revoked.contains(&view.anchor) || revoked.contains(&k.head) {
        return Err("revoked");
    }
    // Anti-rollback state = the receiver's own (generation, head) cursor.
    if k.generation <= view.generation {
        if k.generation == view.generation && k.head == view.head {
            return Ok(view.clone());
        }
        return Err(if k.generation == view.generation {
            "fork"
        } else {
            "stale"
        });
    }
    Ok(View {
        head: k.head,
        current_id: c.replacement_id,
        endpoint: c.endpoint,
        generation: c.generation,
        expires_at: c.expires_at,
        ..view.clone()
    })
}
