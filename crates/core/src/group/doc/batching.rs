//! Bounded native-operation export. Not a transport, receipt or readiness gate.
use std::{cmp::Reverse, collections::BinaryHeap};

use loro::{ExportMode, ID, IdSpan, VersionVector};

use super::GroupDoc;

pub(crate) const MAX_EXPORT_BYTES: usize = 12 * 1024 * 1024;
pub(crate) const MAX_EXPORT_OPS: usize = 256;
pub(crate) const MAX_EXPORT_WRITERS: usize = 4096;

/// Work/admission limits for one planning call, not group membership limits.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BatchLimits {
    pub bytes: usize,
    pub operations: usize,
    pub writers: usize,
}

#[derive(Debug)]
pub(crate) struct ExportBatch {
    pub update: Vec<u8>,
    /// Source-known causal frontier covered by this batch plus receiver input.
    /// Advisory only: never a receiver Ack or the source's whole frontier.
    pub frontier: VersionVector,
    pub operations: usize,
    pub more: bool,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum BatchError {
    #[error("group export planning bounds")]
    Bounds,
    #[error("group export writer budget exceeded")]
    Writers,
    #[error("group export source is tainted")]
    Tainted,
    #[error("group export receiver frontier is not causally closed")]
    Frontier,
    #[error("group export needs retained native history")]
    History,
    #[error("group export encoding failed")]
    Encoding,
    #[error("group export indivisible operation needs {required} bytes (limit {limit})")]
    Oversized { required: usize, limit: usize },
}

impl GroupDoc {
    pub(crate) fn export_batch(
        &self,
        theirs: &VersionVector,
        limits: BatchLimits,
    ) -> Result<Option<ExportBatch>, BatchError> {
        NativeExporter {
            doc: &self.doc,
            tainted: self.tainted,
        }
        .export_batch(theirs, limits)
    }
}

/// Shared planner for already-validated native documents. The caller is
/// responsible for content/admission validation; this only plans native spans.
pub(crate) fn export_native_batch(
    doc: &loro::LoroDoc,
    theirs: &VersionVector,
    limits: BatchLimits,
) -> Result<Option<ExportBatch>, BatchError> {
    NativeExporter {
        doc,
        tainted: false,
    }
    .export_batch(theirs, limits)
}
struct NativeExporter<'a> {
    doc: &'a loro::LoroDoc,
    tainted: bool,
}
impl NativeExporter<'_> {
    fn vv(&self) -> VersionVector {
        self.doc.oplog_vv()
    }
    /// Select missing whole operations in increasing Lamport order. Every selected
    /// operation's prerequisites must be in the receiver prefix or earlier selections.
    /// Output spans preserve original IDs/bytes; no map re-insertion or re-signing.
    ///
    /// Limits bound heap size, candidate count and *successful* encoded bytes.
    /// Trial encoding and causal-frontier conversion still have costs dependent on
    /// retained history; this is not a constant-RAM/CPU streaming exporter.
    pub(crate) fn export_batch(
        &self,
        theirs: &VersionVector,
        limits: BatchLimits,
    ) -> Result<Option<ExportBatch>, BatchError> {
        if limits.bytes == 0
            || limits.bytes > MAX_EXPORT_BYTES
            || limits.operations == 0
            || limits.operations > MAX_EXPORT_OPS
            || limits.writers == 0
            || limits.writers > MAX_EXPORT_WRITERS
        {
            return Err(BatchError::Bounds);
        }
        if self.tainted {
            return Err(BatchError::Tainted);
        }
        let mine = self.vv();
        if mine.len() > limits.writers || theirs.len() > limits.writers {
            return Err(BatchError::Writers);
        }
        if theirs.iter().any(|(_, counter)| *counter < 0) {
            return Err(BatchError::Frontier);
        }
        // A receiver may have operations we do not have. Project to common
        // history, never interpret its extra positions as source-authoritative.
        let mut base = VersionVector::default();
        for (peer, end) in mine.iter() {
            let held = theirs.get(peer).copied().unwrap_or(0).min(*end);
            if held > 0 {
                base.insert(*peer, held);
            }
        }
        let closure = self
            .doc
            .frontiers_to_vv(&self.doc.vv_to_frontiers(&base))
            .ok_or(BatchError::History)?;
        if closure != base {
            return Err(BatchError::Frontier);
        }

        let mut heads = BinaryHeap::new();
        for (peer, end) in mine.iter() {
            let start = base.get(peer).copied().unwrap_or(0);
            if start < *end {
                heads.push(Reverse(self.batch_head(*peer, start)?));
            }
        }
        let mut covered = base.clone();
        let mut result = None;
        let mut operations = 0;
        while operations < limits.operations {
            let Some(Reverse((_, peer, counter))) = heads.pop() else {
                break;
            };
            let change = self
                .doc
                .get_change(ID::new(peer, counter))
                .ok_or(BatchError::History)?;
            if !change
                .deps
                .iter()
                .all(|id| covered.get(&id.peer).copied().unwrap_or(0) > id.counter)
            {
                return Err(BatchError::Frontier);
            }
            let mut trial = covered.clone();
            trial.insert(peer, counter + 1);
            // One contiguous missing span per selected writer, deterministic order.
            let mut spans: Vec<_> = trial
                .iter()
                .filter_map(|(peer, end)| {
                    let start = base.get(peer).copied().unwrap_or(0);
                    (*end > start).then(|| IdSpan::new(*peer, start, *end))
                })
                .collect();
            spans.sort_by_key(|span| span.peer);
            let update = self
                .doc
                .export(ExportMode::updates_in_range(spans))
                .map_err(|_| BatchError::Encoding)?;
            if update.len() > limits.bytes {
                if result.is_none() {
                    return Err(BatchError::Oversized {
                        required: update.len(),
                        limit: limits.bytes,
                    });
                }
                break;
            }
            covered = trial;
            result = Some(update);
            operations += 1;
            if counter + 1 < mine.get(&peer).copied().unwrap_or(0) {
                heads.push(Reverse(self.batch_head(peer, counter + 1)?));
            }
        }
        let more = mine
            .iter()
            .any(|(peer, end)| covered.get(peer).copied().unwrap_or(0) < *end);
        Ok(result.map(|update| ExportBatch {
            update,
            frontier: covered,
            operations,
            more,
        }))
    }

    fn batch_head(&self, peer: u64, counter: i32) -> Result<(u32, u64, i32), BatchError> {
        let change = self
            .doc
            .get_change(ID::new(peer, counter))
            .ok_or(BatchError::History)?;
        let offset = counter
            .checked_sub(change.id.counter)
            .ok_or(BatchError::History)?;
        if change.id.peer != peer || offset < 0 || offset as usize >= change.len {
            return Err(BatchError::History);
        }
        let lamport = change
            .lamport
            .checked_add(offset as u32)
            .ok_or(BatchError::History)?;
        Ok((lamport, peer, counter))
    }
}

#[cfg(test)]
mod tests;
