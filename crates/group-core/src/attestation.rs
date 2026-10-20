//! Prototype portable host storage statements. No wire negotiation, forwarding,
//! Ack-store updates, debt clearing, UI status or readiness certificate yet.
//! Signatures attest a host's claim; they cannot audit a dishonest host's disk.
use cryptography::domains;
use loro::VersionVector;
use serde::{Deserialize, Serialize};

use proto::{Signable, Signed, Signer};

use super::{doc::DocId, state::GroupState};

// Provisional payload budget; tuning is separate from schema/algorithm widths.
const MAX_BODY: usize = 128 * 1024;
pub const MAX_SIGNED: usize = MAX_BODY + proto::signed::SIG_LEN;
const MAX_WRITERS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageAttestation {
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

pub type SignedStorageAttestation = Signed<StorageAttestation>;

impl Signable for StorageAttestation {
    const DOMAIN: domains::Domain = domains::STORAGE_ATTESTATION;
    const MAX_BODY: usize = MAX_BODY;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.host)
    }
    fn check(&self) -> proto::Result<()> {
        self.check_shape().map_err(|_| proto::Error::Decode)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AttestationError {
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

/// The attestation names a valid group id and doc.
pub fn scope_ok(group: &str, doc: &DocId) -> bool {
    super::valid_gid(group) && doc.is_valid()
}
fn frontier_ok(frontier: &[(u64, i32)]) -> bool {
    !frontier.is_empty()
        && frontier.len() <= MAX_WRITERS
        && frontier.iter().all(|(_, counter)| *counter > 0)
        && frontier.windows(2).all(|w| w[0].0 < w[1].0)
}
/// A version vector as the sorted positive frontier a claim carries.
pub fn from_vv(vv: &VersionVector) -> Result<Vec<(u64, i32)>, AttestationError> {
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
/// `vv` holds every op of `frontier`.
pub fn covers(vv: &VersionVector, frontier: &[(u64, i32)]) -> bool {
    frontier
        .iter()
        .all(|(peer, n)| vv.get(peer).copied().unwrap_or(0) >= *n)
}
/// `did`/`dev` is a current member host of the group in `state`.
pub fn host_allowed(state: &GroupState, did: &str, dev: &[u8; 32]) -> bool {
    state.is_member(did) && !state.is_removed(did) && state.is_host(did, dev)
}

impl StorageAttestation {
    fn check_shape(&self) -> Result<(), AttestationError> {
        if self.host.len() > 200
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
    // Only the persisted issuer (p2pcore `group/attestation.rs`) is a runtime minting entry point.
    #[cfg(any(test, feature = "testkit"))]
    pub fn sign(
        &self,
        account: &identity::Identity,
    ) -> Result<SignedStorageAttestation, AttestationError> {
        self.check_shape()?;
        Signed::sign(self, account).map_err(|e| match e {
            proto::Error::WrongIssuer => AttestationError::Signature,
            _ => AttestationError::Bounds,
        })
    }
}

/// Signature/scope check ONLY. Hosting authority is checked separately (p2pcore `GroupCore`).
pub fn open_attestation(
    bytes: &[u8],
    group: &str,
    doc: &DocId,
) -> Result<StorageAttestation, AttestationError> {
    let signed =
        SignedStorageAttestation::from_bytes(bytes).map_err(|_| AttestationError::Bounds)?;
    let claim = signed.peek().map_err(|_| AttestationError::Encoding)?;
    claim.check_shape()?;
    if claim.group != group || &claim.doc != doc {
        return Err(AttestationError::Scope);
    }
    signed.open().map_err(|e| match e {
        proto::Error::BadSignature => AttestationError::Signature,
        _ => AttestationError::Encoding,
    })
}

#[cfg(test)]
mod tests;
