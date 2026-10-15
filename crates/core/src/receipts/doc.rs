//! Dedicated grow-only native receipt map. No persistence/status/transport here.
//! Imports stage on a fork, require explicit caller admission and retain exact
//! original signed bytes and original Loro writer/counters through relays.
use super::*;
use crate::{
    chat::doc::writer_id,
    group::doc::batching::{BatchError, BatchLimits, ExportBatch, export_native_batch},
};
use loro::{ExportMode, LoroDoc, LoroValue, VersionVector};
use std::collections::BTreeMap;
const ROOT: &str = "receipts";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub id: String,
    pub receipt: Receipt,
    pub peer: u64,
    pub counter: i32,
}
pub(crate) struct ReceiptDoc {
    scope: Scope,
    doc: LoroDoc,
}
impl ReceiptDoc {
    pub(crate) fn new(scope: Scope, peer: u64) -> Result<Self, ReceiptError> {
        if !scope.valid() {
            return Err(ReceiptError::Bounds);
        }
        let doc = LoroDoc::new();
        doc.set_peer_id(peer).map_err(|_| ReceiptError::Writer)?;
        Ok(Self { scope, doc })
    }
    pub(crate) fn vv(&self) -> VersionVector {
        self.doc.oplog_vv()
    }
    /// Low-level staged write; not a durably issued acknowledgement. The caller
    /// must checkpoint/persist before exposing, forwarding or projecting it.
    pub(crate) fn write(
        &mut self,
        claim: &Receipt,
        account: &identity::Identity,
        admit: impl FnOnce(&[Entry]) -> Result<(), String>,
    ) -> Result<Option<(Entry, Vec<u8>)>, ReceiptError> {
        let bytes = claim.sign_claim(account)?.to_bytes();
        let claim = open_receipt(&bytes, &self.scope)?;
        if writer_id(&claim.recipient, &claim.device) != self.doc.peer_id() {
            return Err(ReceiptError::Writer);
        }
        let id = receipt_id(&bytes);
        if self.raw(&id).is_some() {
            return Ok(None);
        }
        let before = self.vv();
        let entry = Entry {
            id: id.clone(),
            receipt: claim,
            peer: self.doc.peer_id(),
            counter: before.get(&self.doc.peer_id()).copied().unwrap_or(0),
        };
        admit(std::slice::from_ref(&entry)).map_err(ReceiptError::Policy)?;
        let scratch = self.doc.fork();
        scratch
            .set_peer_id(self.doc.peer_id())
            .map_err(|_| ReceiptError::Writer)?;
        scratch
            .get_map(ROOT)
            .insert(&id, LoroValue::Binary(bytes.into()))
            .map_err(|_| ReceiptError::Structure)?;
        scratch.commit();
        let update = scratch
            .export(ExportMode::updates(&before))
            .map_err(|_| ReceiptError::Encoding)?;
        if update.len() > MAX_UPDATE_BYTES {
            return Err(ReceiptError::Bounds);
        }
        self.doc = scratch;
        Ok(Some((entry, update)))
    }
    pub(crate) fn apply_remote(
        &mut self,
        update: &[u8],
        admit: impl FnOnce(&[Entry]) -> Result<(), String>,
    ) -> Result<Vec<Entry>, ReceiptError> {
        if update.len() > MAX_UPDATE_BYTES {
            return Err(ReceiptError::Bounds);
        }
        LoroDoc::decode_import_blob_meta(update, true).map_err(|_| ReceiptError::Encoding)?;
        let before = self.vv();
        let scratch = self.doc.fork();
        scratch
            .set_peer_id(self.doc.peer_id())
            .map_err(|_| ReceiptError::Writer)?;
        let status = scratch.import(update).map_err(|_| ReceiptError::Encoding)?;
        if status.pending.is_some() {
            return Err(ReceiptError::Pending);
        }
        if scratch.is_shallow() {
            return Err(ReceiptError::Structure);
        }
        let entries = self.inspect(&scratch, &before, &scratch.oplog_vv())?;
        let mut fresh = BTreeMap::new();
        for e in entries {
            if self.raw(&e.id).is_none() {
                fresh.entry(e.id.clone()).or_insert(e);
            }
        }
        let fresh: Vec<_> = fresh.into_values().collect();
        // Even an empty freshness set can carry VV advancement. Persist the
        // resulting document, not just fresh IDs, in a future durable adapter.
        admit(&fresh).map_err(ReceiptError::Policy)?;
        self.doc = scratch;
        Ok(fresh)
    }
    pub(crate) fn snapshot(&self) -> Result<Vec<u8>, ReceiptError> {
        self.doc
            .export(ExportMode::Snapshot)
            .map_err(|_| ReceiptError::Encoding)
    }
    /// Full reconstruction with signatures/structure and caller admission checks.
    /// This is not a trusted sealed-row fast path and is history-dependent.
    pub(crate) fn load(
        scope: Scope,
        peer: u64,
        snapshot: Option<&[u8]>,
        updates: &[Vec<u8>],
        admit: impl FnOnce(&[Entry]) -> Result<(), String>,
    ) -> Result<Self, ReceiptError> {
        let s = Self::new(scope, peer)?;
        for bytes in snapshot
            .into_iter()
            .chain(updates.iter().map(Vec::as_slice))
        {
            if bytes.len() > MAX_UPDATE_BYTES {
                return Err(ReceiptError::Bounds);
            }
            let status = s.doc.import(bytes).map_err(|_| ReceiptError::Encoding)?;
            if status.pending.is_some() {
                return Err(ReceiptError::Pending);
            }
        }
        if s.doc.is_shallow() {
            return Err(ReceiptError::Structure);
        }
        s.doc.set_peer_id(peer).map_err(|_| ReceiptError::Writer)?;
        admit(&s.entries()?).map_err(ReceiptError::Policy)?;
        Ok(s)
    }
    pub(crate) fn checkpoint(&self) -> Result<Self, ReceiptError> {
        let doc = self.doc.fork();
        doc.set_peer_id(self.doc.peer_id())
            .map_err(|_| ReceiptError::Writer)?;
        Ok(Self {
            scope: self.scope.clone(),
            doc,
        })
    }
    pub(crate) fn export_batch(
        &self,
        theirs: &VersionVector,
        limits: BatchLimits,
    ) -> Result<Option<ExportBatch>, BatchError> {
        export_native_batch(&self.doc, theirs, limits)
    }
    pub(crate) fn raw(&self, id: &str) -> Option<Vec<u8>> {
        match self.doc.get_map(ROOT).get(id) {
            Some(loro::ValueOrContainer::Value(LoroValue::Binary(b))) => Some(b.to_vec()),
            _ => None,
        }
    }
    pub(crate) fn entries(&self) -> Result<Vec<Entry>, ReceiptError> {
        let mut unique = BTreeMap::new();
        for e in self.inspect(&self.doc, &VersionVector::default(), &self.vv())? {
            unique.entry(e.id.clone()).or_insert(e);
        }
        Ok(unique.into_values().collect())
    }
    /// Original insert statements in a native interval, including known IDs
    /// under new counters. Not an authorization or acknowledgement frontier.
    pub(crate) fn entries_since(&self, from: &VersionVector) -> Result<Vec<Entry>, ReceiptError> {
        self.entries_between(from, &self.vv())
    }
    pub(crate) fn entries_between(
        &self,
        from: &VersionVector,
        to: &VersionVector,
    ) -> Result<Vec<Entry>, ReceiptError> {
        self.inspect(&self.doc, from, to)
    }
    fn inspect(
        &self,
        doc: &LoroDoc,
        from: &VersionVector,
        to: &VersionVector,
    ) -> Result<Vec<Entry>, ReceiptError> {
        crate::signed_map::inserts(
            doc,
            from,
            to,
            ROOT,
            ReceiptError::Structure,
            |key, bytes, peer, counter| {
                if receipt_id(bytes) != key {
                    return Err(ReceiptError::Key);
                }
                let receipt = open_receipt(bytes, &self.scope)?;
                if writer_id(&receipt.recipient, &receipt.device) != peer {
                    return Err(ReceiptError::Writer);
                }
                Ok(Entry {
                    id: key.to_string(),
                    receipt,
                    peer,
                    counter,
                })
            },
        )
    }
}
#[cfg(test)]
mod tests;
