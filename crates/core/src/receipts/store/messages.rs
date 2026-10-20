//! Message delivered/read receipt runtime (roadmap M3) over the shared ledger.
//!
//! Source adapters (chat/group engines) record an exact issuance INTENT in the
//! same transaction that durably accepts a message version or a local read
//! action. `pump` later signs recipient claims over exactly those versions and
//! completes the intent atomically with the native receipt commit. Projection
//! reads only committed ledger entries. No transport, host Ack or own-device
//! read cursor is consulted: none of them is recipient evidence.
use super::*;
use crate::chat::doc::{MsgRec, pair_id, writer_id};
use std::collections::BTreeMap;

/// Durable, sealed intent rows in the backing chat/group store. Separate from
/// `receipts/` ledger rows and from the call controller's namespace.
const INTENTS: &str = "receipt-runtime/messages/v1/q/";
/// Scopes holding statements WE issued that a counterparty has not acknowledged.
const OUTBOX: &str = "receipt-runtime/messages/v1/out/";
/// Provisional: digests per intent row (split, never truncated).
pub(crate) const MAX_INTENT_DIGESTS: usize = 1024;
/// Provisional: intent rows handled per `pump` call; more stay queued.
pub(crate) const MAX_INTENTS_PER_PUMP: usize = 64;
/// Provisional: idle ledgers kept open per controller (a cache, not retention).
pub(crate) const MAX_OPEN_LEDGERS: usize = 32;
/// Provisional encoded intent ceiling: header plus `MAX_INTENT_DIGESTS` digests.
pub(crate) const MAX_INTENT_BYTES: usize = 64 * 1024;

/// Canonical identity of ONE persisted, verified DM message version: the doc,
/// record id, author, writing install and the author's DID signature, which was
/// verified over the full version content and its exact op span before the
/// version was stored (`chat::doc::apply_remote`/local write). An edit or delete
/// is a new signed version and therefore a new digest: receipts never carry over.
pub(crate) fn dm_version(doc: &str, rec: &MsgRec) -> [u8; 32] {
    cryptography::hash::hasher(cryptography::domains::RECEIPT_DM_VERSION)
        .part(doc.as_bytes())
        .part(rec.id.as_bytes())
        .part(rec.author.as_bytes())
        .part(&rec.dev)
        .part(&rec.did_sig)
        .finish()
}

/// Canonical identity of one stored, verified group record: its exact original
/// signed bytes (group records are immutable; there are no edits).
pub(crate) fn group_version(signed_record: &[u8]) -> [u8; 32] {
    cryptography::hash::content(cryptography::domains::RECEIPT_GROUP_VERSION, signed_record)
}

fn outbox_key(scope: &Scope) -> Result<String, ReceiptError> {
    let p = prefix(scope)?;
    // `receipts/{dm|group}/{id}/{day}/` -> `{OUTBOX}{dm|group}/{id}/{day}`
    let rest = p.strip_prefix("receipts/").ok_or(ReceiptError::Bounds)?;
    Ok(format!("{OUTBOX}{}", rest.trim_end_matches('/')))
}
fn conversation_prefix(c: &Conversation) -> String {
    match c {
        Conversation::Direct(id) => format!("{OUTBOX}dm/{id}/"),
        Conversation::Group(id) => format!("{OUTBOX}group/{id}/"),
    }
}

/// Intent row, format 1 (DM): no conversation context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Intent {
    version: u8,
    kind: Kind,
    scope: Scope,
    messages: Vec<[u8; 32]>,
}
/// Intent row, format 2 (group): adds the issuer's meta-doc frontier at the
/// moment the source state was accepted. It is signed into the receipt's
/// `metadata` and is what group admission judges ("signed while admitted").
/// Both formats are live; the first byte (`version`) selects the shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ContextIntent {
    version: u8,
    kind: Kind,
    scope: Scope,
    messages: Vec<[u8; 32]>,
    metadata: Vec<(u64, i32)>,
}
fn decode_intent(raw: &[u8]) -> Result<ContextIntent, ReceiptError> {
    match raw.first() {
        Some(1) => {
            let i: Intent = cryptography::codec::from_bytes(raw, MAX_INTENT_BYTES)
                .map_err(|_| ReceiptError::Encoding)?;
            Ok(ContextIntent {
                version: 1,
                kind: i.kind,
                scope: i.scope,
                messages: i.messages,
                metadata: vec![],
            })
        }
        Some(2) => cryptography::codec::from_bytes(raw, MAX_INTENT_BYTES)
            .map_err(|_| ReceiptError::Encoding),
        _ => Err(ReceiptError::Bounds),
    }
}

/// One committed receipt the pump issued (for logs/tests; not a UI event).
#[derive(Debug, Clone)]
pub(crate) struct Issued {
    pub scope: Scope,
    #[cfg_attr(not(test), allow(dead_code))] // read by tests only
    pub entry: Entry,
}

/// Evidence projection for one exact version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VersionEvidence {
    pub delivered: Vec<Entry>,
    pub read: Vec<Entry>,
}

pub(crate) struct MessageReceipts {
    store: Arc<ReceiptStore>,
    me: String,
    device: [u8; 32],
    peer: u64,
    ledgers: Mutex<BTreeMap<String, Arc<Ledger>>>,
    /// Serializes intent handling so two pumps never race on one row.
    pumping: Mutex<()>,
}

fn valid_kind(e: &Entry) -> Result<(), String> {
    match e.receipt.kind {
        Kind::Delivered | Kind::Read => Ok(()),
        Kind::Call => Err("call claim in a message receipt ledger".into()),
    }
}

impl MessageReceipts {
    /// One controller per backing namespace (`ReceiptStore` enforces one live
    /// ledger per scope). `device` is this install's scoped device ID.
    pub(crate) fn new(store: Arc<ReceiptStore>, me: &str, device: [u8; 32]) -> Arc<Self> {
        Arc::new(Self {
            store,
            me: me.to_string(),
            device,
            peer: writer_id(me, &device),
            ledgers: Mutex::new(BTreeMap::new()),
            pumping: Mutex::new(()),
        })
    }

    /// Structural admission for already-accepted rows on reopen. Historical
    /// evidence is NOT re-judged against current membership/contact state:
    /// removal only stops new acceptance (user decision 2026-10-15).
    fn stored_ok(me: &str, scope: &Scope, entries: &[Entry]) -> Result<(), String> {
        for e in entries {
            valid_kind(e)?;
            if let Conversation::Direct(pair) = &scope.conversation {
                let issuer = &e.receipt.recipient;
                if issuer != me && pair_id(me, issuer) != *pair {
                    return Err("direct receipt from outside the conversation".into());
                }
            }
        }
        Ok(())
    }

    pub(crate) fn ledger(&self, scope: &Scope) -> Result<Arc<Ledger>, ReceiptError> {
        let key = prefix(scope)?;
        let mut open = self.ledgers.lock();
        if let Some(l) = open.get(&key) {
            return Ok(l.clone());
        }
        if open.len() >= MAX_OPEN_LEDGERS {
            open.retain(|_, l| Arc::strong_count(l) > 1);
        }
        let me = self.me.clone();
        let s = scope.clone();
        let l = Arc::new(self.store.open(scope.clone(), self.peer, move |e| {
            Self::stored_ok(&me, &s, e)
        })?);
        open.insert(key, l.clone());
        Ok(l)
    }

    /// Ops recording an exact issuance obligation. The caller MUST apply them in
    /// the same transaction that durably stores the source state (accepted
    /// version / read action); nothing is signed before that commit.
    pub(crate) fn intent_ops(
        &self,
        kind: Kind,
        scope: &Scope,
        messages: impl IntoIterator<Item = [u8; 32]>,
    ) -> Result<Vec<storage::Op>, ReceiptError> {
        self.intent_ops_in_context(kind, scope, messages, &[])
    }

    /// `intent_ops` with the conversation context to sign: a group receipt
    /// carries the issuer's meta frontier at acceptance (non-empty); a DM
    /// receipt carries none. Checked here so no row can poison the queue.
    pub(crate) fn intent_ops_in_context(
        &self,
        kind: Kind,
        scope: &Scope,
        messages: impl IntoIterator<Item = [u8; 32]>,
        metadata: &[(u64, i32)],
    ) -> Result<Vec<storage::Op>, ReceiptError> {
        if kind == Kind::Call || !scope.valid() {
            return Err(ReceiptError::Bounds);
        }
        let group = matches!(scope.conversation, Conversation::Group(_));
        if group == metadata.is_empty()
            || metadata.len() > MAX_CONTEXT_WRITERS
            || !metadata.iter().all(|(_, end)| *end > 0)
            || !metadata.windows(2).all(|w| w[0].0 < w[1].0)
        {
            return Err(ReceiptError::Bounds);
        }
        let mut all: Vec<[u8; 32]> = messages.into_iter().collect();
        all.sort_unstable();
        all.dedup();
        let mut ops = Vec::new();
        for chunk in all.chunks(MAX_INTENT_DIGESTS) {
            let bytes = if group {
                cryptography::codec::to_bytes(&ContextIntent {
                    version: 2,
                    kind,
                    scope: scope.clone(),
                    messages: chunk.to_vec(),
                    metadata: metadata.to_vec(),
                })
            } else {
                cryptography::codec::to_bytes(&Intent {
                    version: 1,
                    kind,
                    scope: scope.clone(),
                    messages: chunk.to_vec(),
                })
            }
            .map_err(|_| ReceiptError::Encoding)?;
            if bytes.len() > MAX_INTENT_BYTES {
                return Err(ReceiptError::Bounds);
            }
            let mut nonce = [0u8; 8];
            cryptography::rng::fill(&mut nonce);
            let key = format!(
                "{INTENTS}{:020}-{}",
                crate::node::now_ms(),
                hex::encode(nonce)
            );
            ops.push(self.store.db.put_op(&key, &bytes).map_err(io)?);
        }
        Ok(ops)
    }

    /// Digests of `kind` already covered by ANY device of our account in this
    /// scope. One admitted device suffices for the account (user decision).
    fn own_coverage(
        &self,
        ledger: &Ledger,
        kind: Kind,
    ) -> Result<BTreeSet<[u8; 32]>, ReceiptError> {
        let mut out = BTreeSet::new();
        for e in ledger.entries()? {
            if e.receipt.kind == kind && e.receipt.recipient == self.me {
                out.extend(e.receipt.messages.iter().copied());
            }
        }
        Ok(out)
    }

    /// Pending intent rows (bounded page).
    #[cfg(test)]
    pub(crate) fn pending(&self) -> Result<usize, ReceiptError> {
        Ok(self
            .store
            .db
            .keys_after(INTENTS, None, MAX_INTENTS_PER_PUMP)
            .map_err(io)?
            .len())
    }

    /// Signs and durably commits receipts for up to `MAX_INTENTS_PER_PUMP`
    /// intents. Each intent row is deleted in the transaction of its final
    /// receipt write; a failure leaves it queued for the next call. Already
    /// covered versions (identical retry, other own device) are skipped.
    pub(crate) fn pump(&self, account: &identity::Identity) -> Result<Vec<Issued>, ReceiptError> {
        if account.did() != self.me {
            return Err(ReceiptError::Signature);
        }
        let _one = self.pumping.lock();
        let keys = self
            .store
            .db
            .keys_after(INTENTS, None, MAX_INTENTS_PER_PUMP)
            .map_err(io)?;
        let mut issued = Vec::new();
        let mut first_err = None;
        for key in keys {
            match self.complete(account, &key) {
                Ok(mut v) => issued.append(&mut v),
                Err(e) => {
                    tracing::warn!("receipts: intent {key} not completed: {e}");
                    first_err.get_or_insert(e);
                }
            }
        }
        match (issued.is_empty(), first_err) {
            (true, Some(e)) => Err(e),
            _ => Ok(issued),
        }
    }

    fn complete(
        &self,
        account: &identity::Identity,
        key: &str,
    ) -> Result<Vec<Issued>, ReceiptError> {
        let Some(raw) = self.store.db.get(key).map_err(io)? else {
            return Ok(vec![]);
        };
        if raw.len() > MAX_INTENT_BYTES {
            return Err(io("receipt intent too large"));
        }
        let intent = decode_intent(&raw)?;
        if intent.kind == Kind::Call {
            return Err(ReceiptError::Bounds);
        }
        let ledger = self.ledger(&intent.scope)?;
        let covered = self.own_coverage(&ledger, intent.kind)?;
        let wanted: Vec<[u8; 32]> = intent
            .messages
            .iter()
            .copied()
            .filter(|d| !covered.contains(d))
            .collect();
        let done =
            || -> Result<storage::Op, ReceiptError> { Ok(storage::Op::Delete(key.to_string())) };
        if wanted.is_empty() {
            self.store.apply(&[done()?])?;
            return Ok(vec![]);
        }
        let chunks: Vec<&[[u8; 32]]> = wanted.chunks(MAX_MESSAGES).collect();
        let mut out = Vec::new();
        for (n, chunk) in chunks.iter().enumerate() {
            let claim = Receipt {
                scope: intent.scope.clone(),
                kind: intent.kind,
                recipient: self.me.clone(),
                device: self.device,
                messages: chunk.to_vec(),
                metadata: intent.metadata.clone(),
                call: None,
            };
            // Outbox marker rides in every issuing commit (idempotent put); the
            // intent row is completed by the final one.
            let mut extra = vec![
                self.store
                    .db
                    .put_op(&outbox_key(&intent.scope)?, &[])
                    .map_err(io)?,
            ];
            if n + 1 == chunks.len() {
                extra.push(done()?);
            }
            let me = self.me.clone();
            let committed = ledger.write_with_ops(
                &claim,
                account,
                |e| {
                    for x in e {
                        valid_kind(x)?;
                        if x.receipt.recipient != me {
                            return Err("local issuance for another account".into());
                        }
                    }
                    Ok(())
                },
                extra,
            )?;
            out.extend(committed.entries.into_iter().map(|entry| Issued {
                scope: intent.scope.clone(),
                entry,
            }));
        }
        Ok(out)
    }

    /// Evidence from `issuers` (DIDs) that covers exactly `version` in `scope`.
    /// Committed ledger state only; anything absent stays unconfirmed.
    pub(crate) fn evidence(
        &self,
        scope: &Scope,
        version: &[u8; 32],
        issuer: impl Fn(&str) -> bool,
    ) -> Result<VersionEvidence, ReceiptError> {
        let ledger = self.ledger(scope)?;
        let mut out = VersionEvidence {
            delivered: vec![],
            read: vec![],
        };
        for e in ledger.entries()? {
            if !issuer(&e.receipt.recipient) || e.receipt.messages.binary_search(version).is_err() {
                continue;
            }
            match e.receipt.kind {
                Kind::Delivered => out.delivered.push(e),
                Kind::Read => out.read.push(e),
                Kind::Call => {}
            }
        }
        Ok(out)
    }

    /// Imports a DM receipt update from an authenticated contact session for
    /// `session_did` (M4 entry point; NOT wired to a wire frame yet). Only the
    /// session's own and our own account's statements are admitted; call
    /// claims are refused. Returns the newly committed entries.
    pub(crate) fn import_direct(
        &self,
        scope: &Scope,
        session_did: &str,
        update: &[u8],
    ) -> Result<Vec<Entry>, ReceiptError> {
        let Conversation::Direct(pair) = &scope.conversation else {
            return Err(ReceiptError::Scope);
        };
        if session_did == self.me || pair_id(&self.me, session_did) != *pair {
            return Err(ReceiptError::Scope);
        }
        let ledger = self.ledger(scope)?;
        let me = self.me.clone();
        let committed = ledger.apply_remote(update, |fresh| {
            for e in fresh {
                valid_kind(e)?;
                let issuer = &e.receipt.recipient;
                if issuer != session_did && *issuer != me {
                    return Err("receipt issuer is not a party of this conversation".into());
                }
            }
            Ok(())
        })?;
        Ok(committed.entries)
    }

    /// Imports a receipt update under caller policy `admit` (asked with the
    /// statements new to this ledger, before anything commits). Call claims are
    /// always refused. Group ingest uses this with its member admission rule.
    pub(crate) fn import_checked(
        &self,
        scope: &Scope,
        update: &[u8],
        admit: impl FnOnce(&[Entry]) -> Result<(), String>,
    ) -> Result<Vec<Entry>, ReceiptError> {
        let ledger = self.ledger(scope)?;
        let committed = ledger.apply_remote(update, |fresh| {
            for e in fresh {
                valid_kind(e)?;
            }
            admit(fresh)
        })?;
        Ok(committed.entries)
    }

    /// Every committed statement of one scope (a group day's aggregation input).
    pub(crate) fn entries(&self, scope: &Scope) -> Result<Vec<Entry>, ReceiptError> {
        self.ledger(scope)?.entries()
    }

    /// Committed native frontier of one scope's receipt ledger.
    pub(crate) fn vv(&self, scope: &Scope) -> Result<loro::VersionVector, ReceiptError> {
        Ok(self.ledger(scope)?.vv())
    }

    /// Days of `conversation` with own statements not yet acknowledged (bounded).
    pub(crate) fn outbox_days(
        &self,
        conversation: &Conversation,
        limit: usize,
    ) -> Result<Vec<String>, ReceiptError> {
        let p = conversation_prefix(conversation);
        Ok(self
            .store
            .db
            .keys_after(&p, None, limit)
            .map_err(io)?
            .into_iter()
            .filter_map(|k| k.strip_prefix(&p).map(str::to_string))
            .collect())
    }

    /// A counterparty acknowledged holding `theirs` of this scope: once that
    /// covers every statement THIS install wrote there, drop the outbox mark.
    /// Statements from our other installs are their own installs' duty.
    pub(crate) fn acknowledged(
        &self,
        scope: &Scope,
        theirs: &loro::VersionVector,
    ) -> Result<bool, ReceiptError> {
        let key = outbox_key(scope)?;
        // Serialized with issuance: a mark put by a concurrent commit is never lost.
        let _one = self.pumping.lock();
        if self.store.db.get(&key).map_err(io)?.is_none() {
            return Ok(true);
        }
        let ours = self
            .ledger(scope)?
            .vv()
            .get(&self.peer)
            .copied()
            .unwrap_or(0);
        if theirs.get(&self.peer).copied().unwrap_or(0) < ours {
            return Ok(false);
        }
        self.store.apply(&[storage::Op::Delete(key)])?;
        Ok(true)
    }

    /// Native receipt update a peer holding `theirs` lacks (bounded exporter).
    pub(crate) fn export(
        &self,
        scope: &Scope,
        theirs: &loro::VersionVector,
        limits: crate::group::doc::batching::BatchLimits,
    ) -> Result<Option<crate::group::doc::batching::ExportBatch>, ReceiptError> {
        self.ledger(scope)?
            .export_batch(theirs, limits)
            .map_err(|e| ReceiptError::Store(e.to_string()))
    }
}
#[cfg(test)]
mod tests;
