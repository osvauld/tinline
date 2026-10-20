//! Prototype portable host storage statements. No wire negotiation, forwarding,
//! Ack-store updates, debt clearing, UI status or readiness certificate yet.
//! Signatures attest a host's claim; they cannot audit a dishonest host's disk.
//! The statement codec is `group_core::attestation`, re-exported here; this module keeps the
//! issuer and verifier over this device's persisted group docs.
pub(crate) use group_core::attestation::*;

use super::{
    doc::{DocId, GroupDoc},
    engine::GroupCore,
    state::derive_meta,
};

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
    #[cfg(test)]
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
    #[cfg(test)]
    pub(crate) fn verify_storage_attestation(
        &self,
        bytes: &[u8],
        group: &str,
        doc: &DocId,
    ) -> Result<StorageAttestation, AttestationError> {
        if self.closed() {
            return Err(AttestationError::Closed);
        }
        let claim = open_attestation(bytes, group, doc)?;
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
