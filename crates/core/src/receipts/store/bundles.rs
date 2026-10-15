//! Proof-aware packaging over the existing whole-operation native planner.
//! Internal API only: no wire names, negotiation or new authority policy.
use super::*;
use crate::group::doc::batching::{BatchError, BatchLimits, ExportBatch};
use crate::receipts::evidence::Evidence;

#[derive(Debug)]
pub(crate) struct EvidenceBatch {
    pub native: ExportBatch,
    pub evidence: Vec<Evidence>,
}
#[derive(Debug, thiserror::Error)]
pub(crate) enum EvidenceExportError {
    #[error(transparent)]
    Native(#[from] BatchError),
    #[error(transparent)]
    Receipt(#[from] ReceiptError),
}
impl Ledger {
    /// Package exact originals for the selected native interval. Native byte/op
    /// limits do not cover sidecar bytes; they have their own bounded count/codec.
    /// Advisory frontier is never a receiver Ack. History-dependent trials/forks
    /// are unchanged; holding this scope's ledger lock is not a global scheduler.
    pub(crate) fn export_with_evidence(
        &self,
        theirs: &VersionVector,
        mut limits: BatchLimits,
    ) -> Result<Option<EvidenceBatch>, EvidenceExportError> {
        let live = self.state.lock();
        loop {
            let Some(native) = live.doc.export_batch(theirs, limits)? else {
                return Ok(None);
            };
            let mut ids = BTreeSet::new();
            for entry in live.doc.entries_between(theirs, &native.frontier)? {
                if let Some(call) = entry.receipt.call {
                    ids.extend(call.evidence);
                }
            }
            if ids.len() > MAX_IMPORT_EVIDENCE {
                // Re-plan a smaller causal prefix, never trim an already encoded
                // update or advertise a frontier whose proofs were dropped.
                if native.operations <= 1 {
                    return Err(ReceiptError::Bounds.into());
                }
                limits.operations = native.operations / 2;
                continue;
            }
            let mut evidence = Vec::new();
            for id in ids {
                evidence.push(self.evidence(&id)?.ok_or_else(|| {
                    ReceiptError::Policy("export supporting evidence missing".into())
                })?);
            }
            return Ok(Some(EvidenceBatch { native, evidence }));
        }
    }
}
