//! Backing-owned Joined intent controller, not a live Node hook or UI backend.
//! A caller must witness the real transition, freeze source/authority context,
//! handle capture failure, and schedule retries outside media/state locks.
use super::*;
use crate::{
    chat::doc::writer_id,
    gcall::CallAuthority,
    receipts::{
        evidence::{Evidence, MAX_PROTOCOL_EVIDENCE_BYTES},
        participation::{JoinProofs, ParticipationView, prepare_join, verify_joined_entry},
    },
};
use proto::gcall as w;
const PENDING: &str = "receipt-runtime/group-joins/v1/pending/";
const DONE: &str = "receipt-runtime/group-joins/v1/done/";
const DOMAIN: &[u8] = b"tinline-call-joined-transition-v1\0";
/// Provisional bounds, not permission to drop an unrecorded event.
pub(crate) const MAX_PENDING_JOINS: usize = 64;
pub(crate) const MAX_JOIN_INTENT_BYTES: usize = 64 * 1024;
pub(crate) const MAX_JOIN_DONE_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct JoinContext {
    pub source_epoch: u64,
    pub issuer: String,
    pub device: [u8; 32],
}
#[derive(Serialize, Deserialize)]
struct Intent {
    version: u8,
    context: JoinContext,
    scope: Scope,
    signed: Vec<u8>,
    originals: Vec<Vec<u8>>,
}
#[derive(Serialize, Deserialize)]
struct Done {
    version: u8,
    context: JoinContext,
    scope: Scope,
    receipt_id: String,
    start_id: [u8; 32],
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CapturedJoin {
    Pending(String),
    /// Durable completion index only. Not a proof-backed UI status; use the
    /// ledger projection/commit result, which verifies the original evidence.
    AlreadyCommitted(String),
}
pub(crate) struct JoinCommit {
    pub view: ParticipationView,
    /// Only the first native receipt insertion. Lost notifications require
    /// reconciliation from durable history, not an exactly-once callback claim.
    pub newly_committed: bool,
}
pub(crate) struct JoinController {
    store: Arc<ReceiptStore>,
    // Serializes capture/completion in this backing namespace. Never call back
    // into the controller from admission, or hold a media lock around a call.
    cursor: Mutex<Option<String>>,
}
fn policy(s: &str) -> ReceiptError {
    ReceiptError::Policy(s.into())
}
fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn encode<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, ReceiptError> {
    let bytes = cryptography::codec::to_bytes(value).map_err(|_| ReceiptError::Encoding)?;
    if bytes.len() > limit {
        return Err(ReceiptError::Bounds);
    }
    Ok(bytes)
}
fn decode<T: serde::de::DeserializeOwned + Serialize>(
    bytes: &[u8],
    limit: usize,
) -> Result<T, ReceiptError> {
    if bytes.len() > limit {
        return Err(ReceiptError::Bounds);
    }
    cryptography::codec::from_bytes_canonical(bytes, limit).map_err(|_| ReceiptError::Encoding)
}
fn context_matches(context: &JoinContext, claim: &Receipt) -> Result<(), ReceiptError> {
    if context.issuer != claim.recipient || context.device != claim.device {
        return Err(policy("joined intent source mismatch"));
    }
    Ok(())
}
fn proofs(evidence: &[Evidence]) -> Result<JoinProofs, ReceiptError> {
    let (mut start, mut join, mut roster) = (None, None, None);
    if evidence.len() != 3 {
        return Err(ReceiptError::Bounds);
    }
    for e in evidence {
        let blob = e.blob()?;
        let slot = if w::open_start(&blob).is_ok() {
            &mut start
        } else if w::open_join(&blob).is_ok() {
            &mut join
        } else if w::open_roster(&blob).is_ok() {
            &mut roster
        } else {
            return Err(policy("unsupported joined intent original"));
        };
        if slot.replace(blob).is_some() {
            return Err(policy("duplicate joined intent original"));
        }
    }
    Ok(JoinProofs {
        start: start.ok_or_else(|| policy("joined start missing"))?,
        expected_join: join.ok_or_else(|| policy("joined Join missing"))?,
        roster: roster.ok_or_else(|| policy("joined Roster missing"))?,
    })
}
fn transition_id(claim: &Receipt, p: &JoinProofs) -> Result<String, ReceiptError> {
    let join = Evidence::from_blob(&p.expected_join)?;
    let mut h = blake3::Hasher::new();
    h.update(DOMAIN);
    h.update(&(claim.recipient.len() as u64).to_be_bytes());
    h.update(claim.recipient.as_bytes());
    h.update(&claim.device);
    h.update(&join.id);
    Ok(h.finalize().to_hex().to_string())
}
impl Intent {
    fn open(&self, id: &str) -> Result<(Receipt, Vec<Evidence>, JoinProofs), ReceiptError> {
        if self.version != 1 || !valid_id(id) {
            return Err(ReceiptError::Bounds);
        }
        let claim = SignedReceipt::open(&self.signed, &self.scope)?;
        context_matches(&self.context, &claim)?;
        if self.originals.len() != 3 {
            return Err(ReceiptError::Bounds);
        }
        let mut originals = Vec::new();
        for bytes in &self.originals {
            let blob: proto::SignedBlob = decode(bytes, MAX_PROTOCOL_EVIDENCE_BYTES)?;
            originals.push(Evidence::from_blob(&blob)?);
        }
        let p = proofs(&originals)?;
        let entry = doc::Entry {
            id: receipt_id(&self.signed),
            peer: writer_id(&claim.recipient, &claim.device),
            counter: 0,
            receipt: claim.clone(),
        };
        verify_joined_entry(&entry, |id| {
            Ok(originals.iter().find(|e| e.id == *id).cloned())
        })?;
        if transition_id(&claim, &p)? != id {
            return Err(policy("joined intent identity mismatch"));
        }
        Ok((claim, originals, p))
    }
}
impl ReceiptStore {
    /// One controller per factory/backing namespace; no competing ledger factory.
    pub(crate) fn join_controller(self: &Arc<Self>) -> Arc<JoinController> {
        let mut slot = self.joins.lock();
        if let Some(controller) = slot.upgrade() {
            return controller;
        }
        let controller = Arc::new(JoinController {
            store: self.clone(),
            cursor: Mutex::new(None),
        });
        *slot = Arc::downgrade(&controller);
        controller
    }
}
impl JoinController {
    /// Call only for a witnessed actual Joined transition. The callback must
    /// check the frozen account/device/source and transition under caller locks.
    /// Returns success only once the exact signed body and originals are durable.
    pub(crate) fn capture(
        &self,
        account: &identity::Identity,
        device: [u8; 32],
        source_epoch: u64,
        observed_at_ms: u64,
        metadata: Vec<(u64, i32)>,
        p: &JoinProofs,
        auth: &dyn CallAuthority,
        admit: impl FnOnce(&JoinContext, &Receipt) -> Result<(), String>,
    ) -> Result<CapturedJoin, ReceiptError> {
        let (claim, originals) = prepare_join(account, device, observed_at_ms, metadata, p, auth)?;
        let context = JoinContext {
            source_epoch,
            issuer: account.did().into(),
            device,
        };
        admit(&context, &claim).map_err(ReceiptError::Policy)?;
        let id = transition_id(&claim, p)?;
        let _lock = self.cursor.lock();
        let old = self.store.db.get(&format!("{PENDING}{id}")).map_err(io)?;
        if let Some(done) = self.store.db.get(&format!("{DONE}{id}")).map_err(io)? {
            if old.is_some() {
                return Err(io("joined intent is both pending and completed"));
            }
            let done: Done = decode(&done, MAX_JOIN_DONE_BYTES)?;
            if done.version != 1
                || done.context != context
                || !done.scope.valid()
                || done.scope.conversation != claim.scope.conversation
                || !valid_id(&done.receipt_id)
                || done.start_id != Evidence::from_blob(&p.start)?.id
            {
                return Err(policy("completed joined intent source/scope mismatch"));
            }
            return Ok(CapturedJoin::AlreadyCommitted(id));
        }
        if let Some(old) = old {
            let old: Intent = decode(&old, MAX_JOIN_INTENT_BYTES)?;
            let (_, _, old_proofs) = old.open(&id)?;
            if old.context != context
                || Evidence::from_blob(&old_proofs.start)?.id != Evidence::from_blob(&p.start)?.id
            {
                return Err(policy("queued joined intent source/start mismatch"));
            }
            // Same expected Join: preserve the FIRST time, metadata and roster.
            // A later callback/new roster must not re-sign the observation.
            return Ok(CapturedJoin::Pending(id));
        }
        let keys = self
            .store
            .db
            .keys_after(PENDING, None, MAX_PENDING_JOINS + 1)
            .map_err(io)?;
        if keys.len() >= MAX_PENDING_JOINS {
            return Err(policy("joined intent queue full; caller must retain event"));
        }
        let intent = Intent {
            version: 1,
            context,
            scope: claim.scope.clone(),
            signed: claim.sign_claim(account)?.to_bytes()?,
            originals: originals.into_iter().map(|e| e.bytes).collect(),
        };
        let bytes = encode(&intent, MAX_JOIN_INTENT_BYTES)?;
        self.store.apply_ops(&[self
            .store
            .db
            .put_op(&format!("{PENDING}{id}"), &bytes)
            .map_err(io)?])?;
        Ok(CapturedJoin::Pending(id))
    }
    /// At most one key per call; rotate BEFORE retry so a failing scope cannot
    /// starve other intents. No automatic retry timer, abandonment or eviction.
    pub(crate) fn next_pending(&self) -> Result<Option<String>, ReceiptError> {
        let mut cursor = self.cursor.lock();
        let after = cursor.as_ref().map(|s| format!("{PENDING}{s}"));
        let mut keys = self
            .store
            .db
            .keys_after(PENDING, after.as_deref(), 1)
            .map_err(io)?;
        if keys.is_empty() && after.is_some() {
            keys = self.store.db.keys_after(PENDING, None, 1).map_err(io)?;
        }
        let Some(key) = keys.pop() else {
            return Ok(None);
        };
        let id = key
            .strip_prefix(PENDING)
            .ok_or(ReceiptError::Key)?
            .to_string();
        *cursor = Some(id.clone());
        if !valid_id(&id) {
            return Err(ReceiptError::Key);
        }
        Ok(Some(id))
    }
    /// Caller supplies the one backing-owned ledger and must recheck current
    /// source/device/authority admission. Old epochs are NOT automatically rebound
    /// on restart. The callback must not reenter this controller.
    pub(crate) fn try_commit(
        &self,
        id: &str,
        ledger: &Ledger,
        account: &identity::Identity,
        auth: &dyn CallAuthority,
        admit: impl FnOnce(&JoinContext, &Receipt) -> Result<(), String>,
    ) -> Result<JoinCommit, ReceiptError> {
        if !valid_id(id) || !Arc::ptr_eq(&self.store, &ledger.store) {
            return Err(ReceiptError::Scope);
        }
        let _lock = self.cursor.lock();
        let pending_key = format!("{PENDING}{id}");
        let done_key = format!("{DONE}{id}");
        let pending = self.store.db.get(&pending_key).map_err(io)?;
        let done = self.store.db.get(&done_key).map_err(io)?;
        if pending.is_some() && done.is_some() {
            return Err(io("joined intent is both pending and completed"));
        }
        let (context, claim, originals, p, expected_id, extra) = if let Some(bytes) = pending {
            let intent: Intent = decode(&bytes, MAX_JOIN_INTENT_BYTES)?;
            let (claim, originals, p) = intent.open(id)?;
            let expected_id = receipt_id(&intent.signed);
            let done = Done {
                version: 1,
                context: intent.context.clone(),
                scope: claim.scope.clone(),
                receipt_id: expected_id.clone(),
                start_id: Evidence::from_blob(&p.start)?.id,
            };
            let extra = vec![
                self.store
                    .db
                    .put_op(&done_key, &encode(&done, MAX_JOIN_DONE_BYTES)?)
                    .map_err(io)?,
                storage::Op::Delete(pending_key),
            ];
            (intent.context, claim, originals, p, expected_id, extra)
        } else if let Some(bytes) = done {
            let done: Done = decode(&bytes, MAX_JOIN_DONE_BYTES)?;
            if done.version != 1 || !valid_id(&done.receipt_id) || !done.scope.valid() {
                return Err(ReceiptError::Bounds);
            }
            let entry = ledger
                .entries()?
                .into_iter()
                .find(|e| e.id == done.receipt_id)
                .ok_or_else(|| io("completed joined receipt missing"))?;
            if entry.receipt.scope != done.scope {
                return Err(ReceiptError::Scope);
            }
            let mut originals = Vec::new();
            for reference in &entry
                .receipt
                .call
                .as_ref()
                .ok_or(ReceiptError::Bounds)?
                .evidence
            {
                originals.push(
                    ledger
                        .evidence(reference)?
                        .ok_or_else(|| io("completed joined evidence missing"))?,
                );
            }
            verify_joined_entry(&entry, |id| {
                Ok(originals.iter().find(|e| e.id == *id).cloned())
            })?;
            let p = proofs(&originals)?;
            if transition_id(&entry.receipt, &p)? != id
                || done.start_id != Evidence::from_blob(&p.start)?.id
            {
                return Err(policy("completed joined identity mismatch"));
            }
            (
                done.context,
                entry.receipt,
                originals,
                p,
                done.receipt_id,
                vec![],
            )
        } else {
            return Err(policy("joined intent not found"));
        };
        context_matches(&context, &claim)?;
        let observed = claim
            .call
            .as_ref()
            .ok_or(ReceiptError::Bounds)?
            .observed_at_ms;
        let (checked, _) = prepare_join(
            account,
            claim.device,
            observed,
            claim.metadata.clone(),
            &p,
            auth,
        )?;
        if checked != claim {
            return Err(policy("joined intent changed before retry"));
        }
        let entry = doc::Entry {
            id: expected_id,
            peer: writer_id(&claim.recipient, &claim.device),
            counter: 0,
            receipt: claim.clone(),
        };
        let view = verify_joined_entry(&entry, |id| {
            Ok(originals.iter().find(|e| e.id == *id).cloned())
        })?;
        let committed = ledger.write_with_evidence_and_ops(
            &claim,
            account,
            &originals,
            |_| admit(&context, &claim),
            extra,
        )?;
        Ok(JoinCommit {
            view,
            newly_committed: !committed.entries.is_empty(),
        })
    }
}

#[cfg(test)]
mod tests;
