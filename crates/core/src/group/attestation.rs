//! Prototype portable host storage statements. No wire negotiation, forwarding,
//! Ack-store updates, debt clearing, UI status or readiness certificate yet.
//! Signatures attest a host's claim; they cannot audit a dishonest host's disk.
use bincode::Options;
use loro::VersionVector;
use serde::{Deserialize, Serialize};

use super::{
    doc::{DocId, GroupDoc},
    engine::GroupCore,
    record::{put_bytes_pub, verify_did_pub},
    state::{GroupState, derive_meta},
};

const DOMAIN: &[u8] = b"tinline-group-storage-attestation-v1\0";
// Provisional payload budget; tuning is separate from schema/algorithm widths.
const MAX_BODY: usize = 128 * 1024;
const SIGNATURE_BYTES: usize = 64; // Ed25519
const BINCODE_VEC_LENGTH_BYTES: usize = 8; // fixed-integer bincode Vec length
pub(crate) const MAX_SIGNED: usize = MAX_BODY + 2 * BINCODE_VEC_LENGTH_BYTES + SIGNATURE_BYTES;
const MAX_WRITERS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StorageAttestation {
    pub version: u8,
    pub group: String,
    pub doc: DocId,
    pub host: String,
    /// The per-install GROUP device ID, not a calling endpoint or transport key.
    pub host_dev: [u8; 32],
    /// Sorted positive exclusive writer counters; no zero/duplicate aliases.
    pub stored: Vec<(u64, i32)>,
    /// Host's persisted metadata context. Not a trusted issuance timestamp.
    pub metadata: Vec<(u64, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SignedStorageAttestation {
    pub body: Vec<u8>,
    pub sig: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AttestationError {
    #[error("storage attestation bounds or shape")]
    Bounds,
    #[error("storage attestation is not canonical")]
    Encoding,
    #[error("storage attestation scope mismatch")]
    Scope,
    #[error("bad storage attestation signature")]
    Signature,
    #[error("storage attestation needs metadata catch-up")]
    PendingMetadata,
    #[error("storage attestation signer is not a current chat host")]
    Authority,
    #[error("storage attestation document not persisted")]
    Missing,
    #[error("storage attestation core closed")]
    Closed,
    #[error("storage attestation store: {0}")]
    Store(String),
}

fn scope_ok(group: &str, doc: &DocId) -> bool {
    group.len() == 64
        && group
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && doc.is_valid()
}
fn sig_input(body: &[u8]) -> Vec<u8> {
    let mut input = DOMAIN.to_vec();
    put_bytes_pub(&mut input, body);
    input
}
fn frontier_ok(frontier: &[(u64, i32)]) -> bool {
    !frontier.is_empty()
        && frontier.len() <= MAX_WRITERS
        && frontier.iter().all(|(_, counter)| *counter > 0)
        && frontier.windows(2).all(|w| w[0].0 < w[1].0)
}
fn from_vv(vv: &VersionVector) -> Result<Vec<(u64, i32)>, AttestationError> {
    if vv.len() > MAX_WRITERS || vv.iter().any(|(_, n)| *n < 0) {
        return Err(AttestationError::Bounds);
    }
    let mut result: Vec<_> = vv
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(p, n)| (*p, *n))
        .collect();
    result.sort_unstable();
    if !frontier_ok(&result) {
        return Err(AttestationError::Missing);
    }
    Ok(result)
}
fn covers(vv: &VersionVector, frontier: &[(u64, i32)]) -> bool {
    frontier
        .iter()
        .all(|(peer, n)| vv.get(peer).copied().unwrap_or(0) >= *n)
}
fn host_allowed(state: &GroupState, did: &str, dev: &[u8; 32]) -> bool {
    state.is_member(did) && !state.is_removed(did) && state.is_host(did, dev)
}

impl StorageAttestation {
    fn check(&self) -> Result<(), AttestationError> {
        if self.version != 1
            || self.host.len() > 200
            || identity::public_key_from_did(&self.host).is_none()
            || !scope_ok(&self.group, &self.doc)
            || !frontier_ok(&self.stored)
            || !frontier_ok(&self.metadata)
            || (self.doc == DocId::Meta && self.stored != self.metadata)
        {
            return Err(AttestationError::Bounds);
        }
        Ok(())
    }
    // Only the persisted issuer below is a runtime minting entry point.
    fn sign(
        &self,
        account: &identity::Identity,
    ) -> Result<SignedStorageAttestation, AttestationError> {
        self.check()?;
        if self.host != account.did() {
            return Err(AttestationError::Signature);
        }
        let body = bincode::serialize(self).map_err(|_| AttestationError::Encoding)?;
        if body.len() > MAX_BODY {
            return Err(AttestationError::Bounds);
        }
        Ok(SignedStorageAttestation {
            sig: account.sign(&sig_input(&body)).to_vec(),
            body,
        })
    }
}

impl SignedStorageAttestation {
    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>, AttestationError> {
        if self.body.len() > MAX_BODY || self.sig.len() != SIGNATURE_BYTES {
            return Err(AttestationError::Bounds);
        }
        bincode::serialize(self).map_err(|_| AttestationError::Encoding)
    }
    /// Signature/scope check ONLY. Hosting authority is checked separately below.
    pub(crate) fn open(
        bytes: &[u8],
        group: &str,
        doc: &DocId,
    ) -> Result<StorageAttestation, AttestationError> {
        if bytes.len() > MAX_SIGNED {
            return Err(AttestationError::Bounds);
        }
        let signed: Self = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_SIGNED as u64)
            .reject_trailing_bytes()
            .deserialize(bytes)
            .map_err(|_| AttestationError::Encoding)?;
        if signed.to_bytes()?.as_slice() != bytes {
            return Err(AttestationError::Encoding);
        }
        let claim: StorageAttestation = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_BODY as u64)
            .reject_trailing_bytes()
            .deserialize(&signed.body)
            .map_err(|_| AttestationError::Encoding)?;
        if bincode::serialize(&claim).map_err(|_| AttestationError::Encoding)? != signed.body {
            return Err(AttestationError::Encoding);
        }
        claim.check()?;
        if claim.group != group || &claim.doc != doc {
            return Err(AttestationError::Scope);
        }
        if !verify_did_pub(&claim.host, &sig_input(&signed.body), &signed.sig) {
            return Err(AttestationError::Signature);
        }
        Ok(claim)
    }
}

impl GroupCore {
    fn attestation_doc(&self, group: &str, doc: &DocId) -> Result<GroupDoc, AttestationError> {
        let (snapshot, updates) = match doc {
            DocId::Meta => self.store.meta_rows(group),
            DocId::Day(day) => self.store.shard_rows(group, day),
        }
        .map_err(|e| AttestationError::Store(e.to_string()))?;
        if snapshot.is_none() && updates.is_empty() {
            return Err(AttestationError::Missing);
        }
        GroupDoc::load_in(group, doc.clone(), self.wid, snapshot.as_deref(), &updates)
            .map_err(|e| AttestationError::Store(e.to_string()))
    }
    /// Gate issuance on current persisted hosting authority and persisted original
    /// operations, never a sent/planned VV or a mutable uncommitted day cache.
    /// Uses the importer lock through signing, freezing membership and appends.
    pub(crate) fn issue_storage_attestation(
        &self,
        group: &str,
        doc: &DocId,
    ) -> Result<SignedStorageAttestation, AttestationError> {
        if self.closed() {
            return Err(AttestationError::Closed);
        }
        if !scope_ok(group, doc) {
            return Err(AttestationError::Scope);
        }
        let groups = self.groups.lock();
        if !groups.contains_key(group) {
            return Err(AttestationError::Missing);
        }
        let meta = self.attestation_doc(group, &DocId::Meta)?;
        let entries = meta
            .entries_trusted()
            .map_err(|e| AttestationError::Store(e.to_string()))?;
        let state = derive_meta(group, &entries, &meta.vv());
        if !host_allowed(&state, &self.did, &self.dev) {
            return Err(AttestationError::Authority);
        }
        let stored = if *doc == DocId::Meta {
            meta.vv()
        } else {
            let accepted = self.attestation_doc(group, doc)?;
            // Validate persisted oplog structure/positions without re-verifying
            // signatures already verified at ingress (group-shards decision 2).
            accepted
                .entries_trusted()
                .map_err(|e| AttestationError::Store(e.to_string()))?;
            accepted.vv()
        };
        let claim = StorageAttestation {
            version: 1,
            group: group.into(),
            doc: doc.clone(),
            host: self.did.clone(),
            host_dev: self.dev,
            stored: from_vv(&stored)?,
            metadata: from_vv(&meta.vv())?,
        };
        let signed = claim.sign(&self.account)?;
        if self.closed() {
            return Err(AttestationError::Closed);
        }
        drop(groups);
        Ok(signed)
    }
    /// First-acceptance authority check using LOCAL persisted metadata. Does not
    /// store/index this proof or change debt/status. Missing context defers; a
    /// claimed old context never overrides current removal/revocation.
    pub(crate) fn verify_storage_attestation(
        &self,
        bytes: &[u8],
        group: &str,
        doc: &DocId,
    ) -> Result<StorageAttestation, AttestationError> {
        if self.closed() {
            return Err(AttestationError::Closed);
        }
        let claim = SignedStorageAttestation::open(bytes, group, doc)?;
        let groups = self.groups.lock();
        if !groups.contains_key(group) {
            return Err(AttestationError::PendingMetadata);
        }
        let meta = self.attestation_doc(group, &DocId::Meta).map_err(|e| {
            if e == AttestationError::Missing {
                AttestationError::PendingMetadata
            } else {
                e
            }
        })?;
        let entries = meta
            .entries_trusted()
            .map_err(|e| AttestationError::Store(e.to_string()))?;
        let state = derive_meta(group, &entries, &meta.vv());
        if !covers(&meta.vv(), &claim.metadata) {
            return Err(AttestationError::PendingMetadata);
        }
        if !host_allowed(&state, &claim.host, &claim.host_dev) {
            return Err(AttestationError::Authority);
        }
        if self.closed() {
            return Err(AttestationError::Closed);
        }
        drop(groups);
        Ok(claim)
    }
}

pub(crate) mod persistence;

#[cfg(test)]
mod tests;
