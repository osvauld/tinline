//! Atomic original-proof storage and historical cache reconciliation. Ack VVs
//! are projections for existing UI, never portable receipts to forward.
use super::*;
use crate::{
    Error,
    group::{
        own::contents::{doc_name, parse_doc},
        store::StoredAttestation,
    },
    node::Inner,
};
use std::sync::Arc;

pub(crate) const PROOFS_PER_PAGE: usize = 1; // provisional transfer/work budget

pub(crate) fn digest(bytes: &[u8]) -> String {
    hex::encode(cryptography::hash::content(cryptography::domains::ATTESTATION_DIGEST, bytes))
}
pub(crate) fn valid_digest(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn as_vv(frontier: &[(u64, i32)]) -> VersionVector {
    let mut vv = VersionVector::default();
    for (peer, end) in frontier {
        vv.insert(*peer, *end);
    }
    vv
}

impl GroupCore {
    /// Validate and commit while holding the importer lock. No new receipt from
    /// a revoked host is accepted. An identical already-accepted original remains
    /// historical evidence and can reconcile a subsequently imported covered loan.
    pub(crate) fn accept_storage_attestation(
        &self,
        bytes: &[u8],
        group: &str,
        doc: &DocId,
    ) -> Result<bool, AttestationError> {
        if self.closed() {
            return Err(AttestationError::Closed);
        }
        let claim = super::open_attestation(bytes, group, doc)?;
        let id = digest(bytes);
        let mut groups = self.groups.lock();
        let live = groups
            .get_mut(group)
            .ok_or(AttestationError::PendingMetadata)?;
        let existing = self
            .store
            .storage_attestation(group, &id)
            .map_err(store_error)?;
        if let Some(row) = &existing {
            if row.bytes != bytes || &row.doc != doc {
                return Err(AttestationError::Store("proof identity mismatch".into()));
            }
        } else {
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
        }
        let mut meta = live.meta.clone();
        let mut cleared = false;
        match doc {
            DocId::Meta if !meta.own_owed_meta.is_empty() => {
                let owed = VersionVector::decode(&meta.own_owed_meta)
                    .map_err(|e| AttestationError::Store(e.to_string()))?;
                if covers(&as_vv(&claim.stored), &from_vv(&owed)?) {
                    meta.own_owed_meta.clear();
                    cleared = true;
                }
            }
            DocId::Day(day) if meta.owed_days.contains(day) => {
                let held = self.attestation_doc(group, doc)?.vv();
                if covers(&as_vv(&claim.stored), &from_vv(&held)?) {
                    meta.owed_days.remove(day);
                    cleared = true;
                }
            }
            _ => {}
        }
        if existing.is_some() && !cleared {
            return Ok(false);
        }
        // Preserve existing authenticated direct-Ack observations for UI. The
        // union is NOT a signed frontier and never clears debt or travels as proof.
        let mut index = as_vv(&claim.stored);
        if let Some(old) = self
            .store
            .attestation_ack(group, doc, &claim.host, &claim.host_dev)
            .map_err(store_error)?
        {
            let old =
                VersionVector::decode(&old).map_err(|e| AttestationError::Store(e.to_string()))?;
            if old.iter().any(|(_, n)| *n < 0) {
                return Err(AttestationError::Store("negative stored host Ack".into()));
            }
            index.merge(&old);
        }
        if self.closed() {
            return Err(AttestationError::Closed);
        }
        self.store
            .commit_storage_attestation(
                group,
                &id,
                &StoredAttestation {
                    doc: doc.clone(),
                    bytes: bytes.to_vec(),
                },
                &claim.host,
                &claim.host_dev,
                &index.encode(),
                &meta,
            )
            .map_err(store_error)?;
        live.meta = meta;
        Ok(true)
    }
    /// One immutable-proof page; new hash keys behind a cursor require a rescan.
    /// No legacy Ack rows are converted or included.
    pub(crate) fn storage_attestation_page(
        &self,
        group: &str,
        after: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>, AttestationError> {
        if self.closed() {
            return Err(AttestationError::Closed);
        }
        if !scope_ok(group, &DocId::Meta) || after.is_some_and(|id| !valid_digest(id)) {
            return Err(AttestationError::Scope);
        }
        let groups = self.groups.lock();
        if !groups.contains_key(group) {
            return Err(AttestationError::PendingMetadata);
        }
        let mut result = Vec::new();
        for (id, row) in self
            .store
            .storage_attestation_page(group, after, PROOFS_PER_PAGE)
            .map_err(store_error)?
        {
            if digest(&row.bytes) != id {
                return Err(AttestationError::Store("proof digest mismatch".into()));
            }
            super::open_attestation(&row.bytes, group, &row.doc)?;
            result.push((doc_name(group, &row.doc), row.bytes));
        }
        Ok(result)
    }
}
fn store_error(e: Error) -> AttestationError {
    AttestationError::Store(e.to_string())
}
pub(crate) fn protocol_error(e: AttestationError) -> Error {
    match e {
        AttestationError::Store(reason) => Error::Io(reason),
        AttestationError::Closed => Error::Locked,
        other => Error::Protocol(other.to_string()),
    }
}
impl Inner {
    pub(crate) fn own_receipt_page(
        self: &Arc<Self>,
        epoch: u64,
        remote: &[u8; 32],
        group: &str,
        after: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>, Error> {
        self.own_group_admit(epoch, remote)?;
        self.group_core()?
            .storage_attestation_page(group, after)
            .map_err(protocol_error)
    }
    pub(crate) fn own_receipt_accept(
        self: &Arc<Self>,
        epoch: u64,
        remote: &[u8; 32],
        name: &str,
        bytes: &[u8],
    ) -> Result<bool, AttestationError> {
        self.own_group_admit(epoch, remote)
            .map_err(|e| AttestationError::Store(e.to_string()))?;
        let (group, doc) = parse_doc(name).ok_or(AttestationError::Scope)?;
        let core = self.group_core().map_err(store_error)?;
        core.accept_storage_attestation(bytes, &group, &doc)
    }
}
