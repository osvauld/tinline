//! Transactional receipt ledger over an EXISTING sealed store. No new vault,
//! runtime lifecycle hooks, role policy, transport or UI API. One factory must
//! own the namespace per backing store; share its live ledgers via Arc.
use super::{
    doc::{Entry, ReceiptDoc},
    *,
};
use loro::VersionVector;
use parking_lot::Mutex;
use std::{collections::BTreeSet, sync::Arc};
use storage::Sealed;

pub(crate) struct ReceiptStore {
    db: Sealed,
    opened: Mutex<BTreeSet<String>>,
    joins: Mutex<std::sync::Weak<joins::JoinController>>,
    #[cfg(test)]
    fail_commit: std::sync::atomic::AtomicBool,
}
pub(crate) struct Ledger {
    store: Arc<ReceiptStore>,
    prefix: String,
    state: Mutex<Live>,
}
struct Live {
    doc: ReceiptDoc,
    seq: u64,
}
#[derive(Serialize, Deserialize)]
struct StoredState {
    seq: u64,
    vv: Vec<u8>,
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Position {
    pub peer: u64,
    pub counter: i32,
}
/// Provisional sidecar budget, not a negotiated frame or retention ceiling.
pub(crate) const MAX_IMPORT_EVIDENCE: usize = super::calls::MAX_CALL_EVIDENCE;
pub(crate) struct ImportClaim {
    pub entry: Entry,
    /// Only says the signed body was already durable; not proof of historical
    /// authority or of a previously verified supporting-evidence policy.
    pub already_stored: bool,
}
pub(crate) struct Committed {
    pub entries: Vec<Entry>,
    pub update: Option<Vec<u8>>,
    pub frontier: VersionVector,
}
fn io(e: impl std::fmt::Display) -> ReceiptError {
    ReceiptError::Store(e.to_string())
}
fn prefix(scope: &Scope) -> Result<String, ReceiptError> {
    if !scope.valid() {
        return Err(ReceiptError::Bounds);
    }
    let (kind, id) = match &scope.conversation {
        Conversation::Direct(id) => ("dm", id),
        Conversation::Group(id) => ("group", id),
    };
    Ok(format!("receipts/{kind}/{id}/{}/", scope.day))
}
impl ReceiptStore {
    pub(crate) fn new(db: Sealed) -> Arc<Self> {
        Arc::new(Self {
            db,
            opened: Mutex::new(BTreeSet::new()),
            joins: Mutex::new(std::sync::Weak::new()),
            #[cfg(test)]
            fail_commit: false.into(),
        })
    }
    pub(crate) fn open(
        self: &Arc<Self>,
        scope: Scope,
        peer: u64,
        admit: impl FnOnce(&[Entry]) -> Result<(), String>,
    ) -> Result<Ledger, ReceiptError> {
        let p = prefix(&scope)?;
        if !self.opened.lock().insert(p.clone()) {
            return Err(ReceiptError::Policy(
                "receipt scope already open; share the live ledger".into(),
            ));
        }
        let result = (|| {
            let saved = self.db.get(&format!("{p}state")).map_err(io)?;
            let rows = self.db.scan(&format!("{p}u/")).map_err(io)?;
            let state: StoredState = saved
                .map(|b| bincode::deserialize(&b).map_err(io))
                .transpose()?
                .unwrap_or(StoredState {
                    seq: 0,
                    vv: VersionVector::default().encode(),
                });
            if state.seq != rows.len() as u64 {
                return Err(io("receipt sequence/rows mismatch"));
            }
            for (n, (key, _)) in rows.iter().enumerate() {
                if key != &format!("{p}u/{:020}", n as u64 + 1) {
                    return Err(io("receipt sequence gap"));
                }
            }
            let updates: Vec<_> = rows.into_iter().map(|(_, b)| b).collect();
            let doc = ReceiptDoc::load(scope, peer, None, &updates, admit)?;
            if VersionVector::decode(&state.vv).map_err(io)? != doc.vv() {
                return Err(io("receipt persisted frontier mismatch"));
            }
            // Index is a tiny pointer, not a duplicated body or forged summary.
            for e in doc.entries()? {
                let raw = self
                    .db
                    .get(&format!("{p}index/{}", e.id))
                    .map_err(io)?
                    .ok_or_else(|| io("receipt index missing"))?;
                let pos: Position = bincode::deserialize(&raw).map_err(io)?;
                if pos
                    != (Position {
                        peer: e.peer,
                        counter: e.counter,
                    })
                {
                    return Err(io("receipt index position mismatch"));
                }
            }
            Ok(Ledger {
                store: self.clone(),
                prefix: p.clone(),
                state: Mutex::new(Live {
                    doc,
                    seq: state.seq,
                }),
            })
        })();
        if result.is_err() {
            self.opened.lock().remove(&p);
        }
        result
    }
    fn apply_ops(&self, ops: &[storage::Op]) -> Result<(), ReceiptError> {
        #[cfg(test)]
        if self
            .fail_commit
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(io("injected receipt commit failure"));
        }
        self.db.apply(ops).map_err(io)
    }

    #[cfg(test)]
    pub(crate) fn fail_next_commit(&self) {
        self.fail_commit
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
impl Drop for Ledger {
    fn drop(&mut self) {
        self.store.opened.lock().remove(&self.prefix);
    }
}
impl Ledger {
    /// Low-level durably stored claim; caller must verify real source content/
    /// lifecycle and admission under its appropriate locks before issuing it.
    pub(crate) fn write(
        &self,
        claim: &Receipt,
        account: &identity::Identity,
        admit: impl FnOnce(&[Entry]) -> Result<(), String>,
    ) -> Result<Committed, ReceiptError> {
        let mut live = self.state.lock();
        let mut candidate = live.doc.checkpoint()?;
        let Some((entry, update)) = candidate.write(claim, account, admit)? else {
            return Ok(Committed {
                entries: vec![],
                update: None,
                frontier: live.doc.vv(),
            });
        };
        self.commit(&mut live, candidate, update, vec![entry], &[])
    }
    pub(crate) fn apply_remote(
        &self,
        update: &[u8],
        admit: impl FnOnce(&[Entry]) -> Result<(), String>,
    ) -> Result<Committed, ReceiptError> {
        let mut live = self.state.lock();
        let mut candidate = live.doc.checkpoint()?;
        let entries = candidate.apply_remote(update, admit)?;
        if candidate.vv() == live.doc.vv() {
            return Ok(Committed {
                entries,
                update: None,
                frontier: live.doc.vv(),
            });
        }
        self.commit(&mut live, candidate, update.to_vec(), entries, &[])
    }
    /// Strict import with supporting originals. Caller must check protocol
    /// semantics, authority/context and peer admission, even for duplicate pulls.
    /// No sidecar-only recovery or adoption of dangling legacy claims is implied.
    pub(crate) fn apply_remote_with_evidence(
        &self,
        update: &[u8],
        supplied: &[super::evidence::Evidence],
        admit: impl FnOnce(&[ImportClaim], &[super::evidence::Evidence]) -> Result<(), String>,
    ) -> Result<Committed, ReceiptError> {
        use std::collections::BTreeMap;
        if supplied.len() > MAX_IMPORT_EVIDENCE {
            return Err(ReceiptError::Bounds);
        }
        let mut sidecars = BTreeMap::new();
        for e in supplied {
            e.blob()?;
            if sidecars.insert(e.id, e.clone()).is_some() {
                return Err(ReceiptError::Bounds);
            }
        }
        let mut live = self.state.lock();
        let before = live.doc.vv();
        let mut candidate = live.doc.checkpoint()?;
        let fresh = candidate.apply_remote(update, |_| Ok(()))?;
        let mut touched = BTreeMap::new();
        for e in candidate.entries_since(&before)? {
            touched.entry(e.id.clone()).or_insert(e);
        }
        // A repeated native response has no new interval, but supplied proofs
        // must still belong to durable statements and pass the caller's gate.
        if !sidecars.is_empty() {
            let mut newly_supplied = BTreeSet::new();
            for id in sidecars.keys() {
                if self.evidence(id)?.is_none() {
                    newly_supplied.insert(*id);
                }
            }
            for e in candidate.entries()? {
                // New sidecars must not accidentally upgrade an older dangling
                // claim sharing an original, even when this update advances VV.
                let heals_known = live.doc.raw(&e.id).is_some()
                    && e.receipt
                        .call
                        .as_ref()
                        .is_some_and(|c| c.evidence.iter().any(|id| newly_supplied.contains(id)));
                // Complete proof-set matching avoids dragging every other valid
                // event sharing a CallStart into a duplicate replay's budget.
                let replay = candidate.vv() == before
                    && e.receipt.call.as_ref().is_some_and(|c| {
                        !c.evidence.is_empty()
                            && c.evidence.iter().all(|id| sidecars.contains_key(id))
                    });
                if heals_known || replay {
                    touched.entry(e.id.clone()).or_insert(e);
                }
            }
        }
        let mut wanted = BTreeSet::new();
        let mut claims = Vec::new();
        for (_, entry) in touched {
            if let Some(c) = &entry.receipt.call {
                wanted.extend(c.evidence.iter().copied());
            }
            claims.push(ImportClaim {
                already_stored: live.doc.raw(&entry.id).is_some(),
                entry,
            });
        }
        if wanted.len() > MAX_IMPORT_EVIDENCE || sidecars.keys().any(|id| !wanted.contains(id)) {
            return Err(ReceiptError::Bounds);
        }
        let mut resolved = Vec::new();
        for id in wanted {
            let old = self.evidence(&id)?;
            // A known body's missing proofs are not automatically upgraded by
            // this import path. Explicit recovery/migration needs its own policy.
            if old.is_none()
                && claims.iter().any(|c| {
                    c.already_stored
                        && c.entry
                            .receipt
                            .call
                            .as_ref()
                            .is_some_and(|call| call.evidence.contains(&id))
                })
            {
                return Err(io(
                    "known receipt lacks original evidence; explicit recovery required",
                ));
            }
            let proof = match (old, sidecars.get(&id)) {
                (Some(old), Some(new)) if old.bytes != new.bytes => {
                    return Err(io("protocol evidence identity mismatch"));
                }
                (Some(old), _) => old,
                (None, Some(new)) => new.clone(),
                (None, None) => {
                    return Err(ReceiptError::Policy(
                        "supporting receipt evidence missing".into(),
                    ));
                }
            };
            resolved.push(proof);
        }
        admit(&claims, &resolved).map_err(ReceiptError::Policy)?;
        if candidate.vv() == before {
            return Ok(Committed {
                entries: fresh,
                update: None,
                frontier: before,
            });
        }
        self.commit(&mut live, candidate, update.to_vec(), fresh, &resolved)
    }
    /// Strict local call bridge: exact supporting originals and receipt commit
    /// together. This does not itself perform protocol role/lifecycle checks.
    pub(crate) fn write_with_evidence(
        &self,
        claim: &Receipt,
        account: &identity::Identity,
        evidence: &[super::evidence::Evidence],
        admit: impl FnOnce(&[Entry]) -> Result<(), String>,
    ) -> Result<Committed, ReceiptError> {
        self.write_with_evidence_and_ops(claim, account, evidence, admit, vec![])
    }
    /// Store-owned controller sidecars only: native receipt, originals and intent
    /// completion must share a transaction. Never exposed to protocol/UI callers.
    fn write_with_evidence_and_ops(
        &self,
        claim: &Receipt,
        account: &identity::Identity,
        evidence: &[super::evidence::Evidence],
        admit: impl FnOnce(&[Entry]) -> Result<(), String>,
        extra: Vec<storage::Op>,
    ) -> Result<Committed, ReceiptError> {
        let mut live = self.state.lock();
        let call = claim.call.as_ref().ok_or(ReceiptError::Bounds)?;
        if claim.kind != Kind::Call || evidence.len() > super::calls::MAX_CALL_EVIDENCE {
            return Err(ReceiptError::Bounds);
        }
        let mut ids = Vec::new();
        for e in evidence {
            e.blob()?;
            ids.push(e.id);
        }
        ids.sort_unstable();
        if !ids.windows(2).all(|w| w[0] < w[1]) || ids != call.evidence {
            return Err(ReceiptError::Bounds);
        }
        let mut candidate = live.doc.checkpoint()?;
        // Stage without publishing, then invoke the gate even for a duplicate.
        // Low-level ReceiptDoc::write skips admission on an identical body; a
        // live lifecycle bridge must still recheck the local device/session.
        let Some((entry, update)) = candidate.write(claim, account, |_| Ok(()))? else {
            let existing = live
                .doc
                .entries()?
                .into_iter()
                .find(|e| &e.receipt == claim)
                .ok_or_else(|| io("duplicate receipt entry missing"))?;
            admit(std::slice::from_ref(&existing)).map_err(ReceiptError::Policy)?;
            // An already accepted identical receipt must have its originals too;
            // never silently repair partial evidence outside a transaction.
            for e in evidence {
                if self.evidence(&e.id)?.as_ref().map(|x| &x.bytes) != Some(&e.bytes) {
                    return Err(io("accepted call receipt lacks original evidence"));
                }
            }
            if !extra.is_empty() {
                self.store.apply_ops(&extra)?;
            }
            return Ok(Committed {
                entries: vec![],
                update: None,
                frontier: live.doc.vv(),
            });
        };
        admit(std::slice::from_ref(&entry)).map_err(ReceiptError::Policy)?;
        self.commit_with_ops(&mut live, candidate, update, vec![entry], evidence, extra)
    }
    pub(crate) fn evidence(
        &self,
        id: &[u8; 32],
    ) -> Result<Option<super::evidence::Evidence>, ReceiptError> {
        let bytes = self
            .store
            .db
            .get(&format!("{}evidence/{}", self.prefix, hex::encode(id)))
            .map_err(io)?;
        bytes
            .map(|bytes| {
                let e = super::evidence::Evidence { id: *id, bytes };
                e.blob()?;
                Ok(e)
            })
            .transpose()
    }
    fn commit(
        &self,
        live: &mut Live,
        candidate: ReceiptDoc,
        update: Vec<u8>,
        entries: Vec<Entry>,
        evidence: &[super::evidence::Evidence],
    ) -> Result<Committed, ReceiptError> {
        self.commit_with_ops(live, candidate, update, entries, evidence, vec![])
    }
    fn commit_with_ops(
        &self,
        live: &mut Live,
        candidate: ReceiptDoc,
        update: Vec<u8>,
        entries: Vec<Entry>,
        evidence: &[super::evidence::Evidence],
        extra: Vec<storage::Op>,
    ) -> Result<Committed, ReceiptError> {
        let seq = live
            .seq
            .checked_add(1)
            .ok_or_else(|| io("receipt sequence exhausted"))?;
        let state = StoredState {
            seq,
            vv: candidate.vv().encode(),
        };
        let mut ops = vec![
            self.store
                .db
                .put_op(&format!("{}u/{seq:020}", self.prefix), &update)
                .map_err(io)?,
            self.store
                .db
                .put_op(
                    &format!("{}state", self.prefix),
                    &bincode::serialize(&state).map_err(io)?,
                )
                .map_err(io)?,
        ];
        for e in &entries {
            let pos = Position {
                peer: e.peer,
                counter: e.counter,
            };
            ops.push(
                self.store
                    .db
                    .put_op(
                        &format!("{}index/{}", self.prefix, e.id),
                        &bincode::serialize(&pos).map_err(io)?,
                    )
                    .map_err(io)?,
            );
        }
        for e in evidence {
            let path = format!("{}evidence/{}", self.prefix, hex::encode(e.id));
            if let Some(old) = self.store.db.get(&path).map_err(io)? {
                if old != e.bytes {
                    return Err(io("protocol evidence identity mismatch"));
                }
            } else {
                ops.push(self.store.db.put_op(&path, &e.bytes).map_err(io)?);
            }
        }
        ops.extend(extra);
        self.store.apply_ops(&ops)?;
        let frontier = candidate.vv();
        live.doc = candidate;
        live.seq = seq;
        Ok(Committed {
            entries,
            update: Some(update),
            frontier,
        })
    }
    pub(crate) fn vv(&self) -> VersionVector {
        self.state.lock().doc.vv()
    }
    pub(crate) fn entries(&self) -> Result<Vec<Entry>, ReceiptError> {
        self.state.lock().doc.entries()
    }
    pub(crate) fn raw(&self, id: &str) -> Option<Vec<u8>> {
        self.state.lock().doc.raw(id)
    }
    pub(crate) fn export_batch(
        &self,
        theirs: &VersionVector,
        limits: crate::group::doc::batching::BatchLimits,
    ) -> Result<
        Option<crate::group::doc::batching::ExportBatch>,
        crate::group::doc::batching::BatchError,
    > {
        self.state.lock().doc.export_batch(theirs, limits)
    }
}
pub(crate) mod bundles;
pub(crate) mod joins;
#[cfg(test)]
mod tests;
