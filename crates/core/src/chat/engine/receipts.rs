//! DM source adapter for signed recipient receipts (roadmap M3). Owned by the
//! receipt-runtime lane; the engine calls these at its durable commit points.
//!
//! - Delivered: an intent row for the exact incoming versions a contact batch
//!   or contact history snapshot made durable, in THAT transaction.
//! - Read: an intent row for the exact incoming, undeleted versions shown on
//!   this device whose `at` lies in the local mark-read step (old, new cursor],
//!   in the transaction that moves the cursor. A cursor that arrives from
//!   another of our devices issues nothing here (that device issued its own).
//! - `chat_receipts_pump` signs and commits pending intents (bounded).
//!
//! - Exchange (chat/6, roadmap M4): `ReceiptHello` per session, `Receipts` with
//!   original native ledger ops, `ReceiptAck`. Imports admit only this
//!   conversation's two DIDs; post-commit, our affected outgoing messages get
//!   `on_message_changed` so apps re-read `chat_message_receipts`.
//!
//! Never consulted: the contact's `Ack` VV (legacy tick), host/cache storage or
//! our own unread cursor. Those are not recipient-signed evidence.

use super::*;
use crate::receipts::store::messages::{VersionEvidence, dm_version};
use crate::receipts::{Conversation, Kind, ReceiptError, Scope, api};
use std::collections::BTreeMap;

/// Provisional: newest read versions one mark-read step can cover. Older
/// versions beyond it stay without a read receipt (a hole, never a claim).
pub(crate) const MAX_READ_VERSIONS: usize = 4096;
/// Provisional: days one `ReceiptHello` lists beyond the recent hello days.
pub(crate) const MAX_RECEIPT_DAYS: usize = 64;
/// Provisional per-frame native receipt budget (well under `MAX_FRAME`).
const RECEIPT_BATCH: crate::group::doc::batching::BatchLimits =
    crate::group::doc::batching::BatchLimits { bytes: 256 * 1024, operations: 64, writers: 4096 };
/// Provisional: frames one send pass writes for a day; the rest follows the next trigger.
const RECEIPT_FRAMES_PER_SEND: usize = 16;

/// Provisional: closed days one mark-read step loads from their blobs (newest first).
pub(crate) const MAX_CLOSED_READ_DAYS: usize = 64;

/// Messages of closed days loaded for a read step, by day.
pub(crate) type ClosedDays = BTreeMap<String, BTreeMap<String, MsgRec>>;

/// The shard days a read step `(from, to]` may touch: a version's `at` may sit up to an hour
/// outside its shard's day (one day of margin).
fn read_day_range(from: i64, to: i64) -> (String, String) {
    (day_of(from.saturating_sub(DAY_MS).max(0)), day_of(to.saturating_add(DAY_MS)))
}

/// The exact incoming, undeleted versions a read step `(from, to]` covers, newest
/// `MAX_READ_VERSIONS` only (older ones stay a hole), by day.
pub(crate) fn read_versions(pair: &str, me: &str, from: i64, to: i64, days: &[(String, BTreeMap<String, MsgRec>)]) -> BTreeMap<String, Vec<[u8; 32]>> {
    let mut found: Vec<(i64, String, [u8; 32])> = Vec::new();
    for (day, msgs) in days {
        let doc = doc_name(pair, day);
        for r in msgs.values() {
            if r.author != me && !r.deleted && r.at > from && r.at <= to {
                found.push((r.at, day.clone(), dm_version(&doc, r)));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.truncate(MAX_READ_VERSIONS);
    let mut by_day: BTreeMap<String, Vec<[u8; 32]>> = BTreeMap::new();
    for (_, day, v) in found {
        by_day.entry(day).or_default().push(v);
    }
    by_day
}

fn rio(e: ReceiptError) -> Error {
    Error::Io(format!("receipts: {e}"))
}

pub(crate) fn dm_scope(pair: &str, day: &str) -> Scope {
    Scope { conversation: Conversation::Direct(pair.to_string()), day: day.to_string() }
}

impl Inner {
    /// Ops to add to the transaction that durably stores `recs` (accepted from
    /// the contact): one delivered obligation for the incoming versions.
    pub(crate) fn chat_delivered_intent(core: &ChatCore, me: &Me, pair: &str, day: &str, recs: &[&MsgRec]) -> Result<Vec<Op>, Error> {
        let doc = doc_name(pair, day);
        let versions: Vec<[u8; 32]> = recs.iter().filter(|r| r.author != me.id.did()).map(|r| dm_version(&doc, r)).collect();
        if versions.is_empty() {
            return Ok(vec![]);
        }
        core.receipts.intent_ops(Kind::Delivered, &dm_scope(pair, day), versions).map_err(rio)
    }

    /// A day's messages exactly as stored (never a cached doc ahead of the
    /// store). `None` for a closed day held only as a snapshot blob.
    fn chat_stored_messages(core: &ChatCore, pair: &str, day: &str) -> Result<Option<std::collections::BTreeMap<String, MsgRec>>, Error> {
        let Some(meta) = core.store.shard_meta(pair, day)? else { return Ok(None) };
        {
            let cache = core.shards.lock();
            if let Some(s) = cache.get(&(pair.to_string(), day.to_string()))
                && s.vv().encode() == meta.vv
            {
                return s.messages().map(Some).map_err(|e| Error::Io(e.to_string()));
            }
        }
        let (snap, ups) = core.store.shard_rows(pair, day)?;
        if meta.closed.is_some() && snap.is_none() {
            return Ok(None);
        }
        let shard = Shard::load(pair, day, core.wid, snap.as_deref(), &ups).map_err(|e| Error::Io(e.to_string()))?;
        shard.messages().map(Some).map_err(|e| Error::Io(e.to_string()))
    }

    /// Ops for a local mark-read step from `from` (exclusive) to `to`
    /// (inclusive), both author-clock ms as in `ConvMeta::read_upto`.
    /// Nothing while the account's read-receipt setting is off: the cursor still moves, so
    /// turning it on later confirms only reads from then on (no backdating).
    /// `closed`: the messages of closed days (older than 8 days, held only as a snapshot blob)
    /// that `chat_closed_for_read` loaded before the step; without them those days are skipped.
    pub(crate) fn chat_read_intent(core: &ChatCore, me: &Me, pair: &str, from: i64, to: i64, closed: &ClosedDays) -> Result<Vec<Op>, Error> {
        if to <= from || !core.store.read_receipts() {
            return Ok(vec![]);
        }
        let (lo, hi) = read_day_range(from, to);
        let mut days: Vec<(String, BTreeMap<String, MsgRec>)> = Vec::new();
        for day in core.store.shard_days(pair)? {
            if day < lo || day > hi {
                continue;
            }
            match Self::chat_stored_messages(core, pair, &day)? {
                Some(msgs) => days.push((day, msgs)),
                None => {
                    if let Some(msgs) = closed.get(&day) {
                        days.push((day, msgs.clone()));
                    }
                }
            }
        }
        let by_day = read_versions(pair, me.id.did(), from, to, &days);
        let mut ops = Vec::new();
        for (day, vs) in by_day {
            ops.extend(core.receipts.intent_ops(Kind::Read, &dm_scope(pair, &day), vs).map_err(rio)?);
        }
        Ok(ops)
    }

    /// Before a mark-read step: the closed days in its range (newest first, bounded), loaded from
    /// their snapshot blobs, so reading an old unread message still issues its read receipt.
    /// Nothing when read receipts are off or nothing is unread. A day that fails to load is skipped
    /// (logged); its messages stay without a read receipt.
    pub(crate) async fn chat_closed_for_read(self: &Arc<Self>, core: &Arc<ChatCore>, pair: &str, from: i64, to: i64) -> ClosedDays {
        let mut out = ClosedDays::new();
        if to <= from || !core.store.read_receipts() {
            return out;
        }
        let (lo, hi) = read_day_range(from, to);
        let Ok(mut days) = core.store.shard_days(pair) else { return out };
        days.retain(|d| *d >= lo && *d <= hi);
        let mut seen = 0usize;
        for day in days.into_iter().rev() {
            if out.len() >= MAX_CLOSED_READ_DAYS || seen >= MAX_READ_VERSIONS {
                break;
            }
            let closed = core.store.shard_meta(pair, &day).ok().flatten().is_some_and(|m| m.closed.is_some());
            if !closed || core.store.shard_rows(pair, &day).map(|r| r.0.is_some()).unwrap_or(true) {
                continue;
            }
            match self.chat_load_shard(core, pair, &day).await.and_then(|s| s.messages().map_err(|e| Error::Io(e.to_string()))) {
                Ok(msgs) => {
                    seen += msgs.len();
                    out.insert(day, msgs);
                }
                Err(e) => tracing::warn!("chat receipts: closed day {day} not loaded for read: {e}"),
            }
        }
        out
    }

    /// Signs/commits pending receipt intents (bounded); failures stay queued.
    /// Live sessions of the affected contacts are told to send the new ops.
    pub(crate) fn chat_receipts_pump(&self) {
        let (Ok(me), Ok(core)) = (self.me(), self.chat_core()) else { return };
        let issued = match core.receipts.pump(&me.id) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!("chat receipts: {e}");
                return;
            }
        };
        let mut seen = std::collections::BTreeSet::new();
        for i in issued {
            let Conversation::Direct(pair) = &i.scope.conversation else { continue };
            if !seen.insert((pair.clone(), i.scope.day.clone())) {
                continue;
            }
            if let Some(did) = self.pair_owner(&me, pair).filter(|d| d != me.id.did()) {
                for s in self.chat_sessions_of(&core, &did) {
                    let _ = s.tx.send(Cmd::Receipts(i.scope.day.clone()));
                }
            }
        }
    }

    /// We hold statements of ours this contact has not acknowledged.
    pub(crate) fn chat_receipts_owed(core: &ChatCore, pair: &str) -> bool {
        core.receipts.outbox_days(&Conversation::Direct(pair.to_string()), 1).is_ok_and(|d| !d.is_empty())
    }

    // ---- exchange (chat/6) ---------------------------------------------------------------------

    /// Our receipt-ledger version per day: recent days we hold, plus unacknowledged ones.
    pub(super) fn chat_receipt_hello(&self, core: &ChatCore, sess: &Session) -> Result<ChatMsg, Error> {
        let me = self.me()?;
        let pair = pair_id(me.id.did(), &sess.did);
        let mut days = self.recent_days();
        for d in core.receipts.outbox_days(&Conversation::Direct(pair.clone()), MAX_RECEIPT_DAYS).map_err(rio)? {
            if !days.contains(&d) {
                days.push(d);
            }
        }
        let mut out = Vec::new();
        for d in days {
            if out.len() >= MAX_RECEIPT_DAYS {
                break;
            }
            if core.store.shard_meta(&pair, &d)?.is_some() {
                let vv = core.receipts.vv(&dm_scope(&pair, &d)).map_err(rio)?;
                out.push((d, vv.encode()));
            }
        }
        Ok(ChatMsg::ReceiptHello { days: out })
    }

    pub(super) async fn chat_receipt_hello_in(self: &Arc<Self>, core: &Arc<ChatCore>, sess: &Arc<Session>, days: Vec<(String, Vec<u8>)>, writer: &mut ChatWriter) -> Result<(), Error> {
        if days.len() > MAX_RECEIPT_DAYS {
            return Err(Error::Protocol("too many receipt days".into()));
        }
        let me = self.me()?;
        let pair = pair_id(me.id.did(), &sess.did);
        let mut todo = Vec::new();
        for (d, vv) in days {
            if !day_valid(&d) {
                return Err(Error::Protocol("bad day".into()));
            }
            let vv = decode_vv(&vv)?;
            if core.store.shard_meta(&pair, &d)?.is_none() {
                continue;
            }
            // Their hello also says what they hold: it may complete our outbox.
            core.receipts.acknowledged(&dm_scope(&pair, &d), &vv).map_err(rio)?;
            sess.receipt_vv.lock().insert(d.clone(), vv);
            todo.push(d);
        }
        for d in core.receipts.outbox_days(&Conversation::Direct(pair.clone()), MAX_RECEIPT_DAYS).map_err(rio)? {
            if !todo.contains(&d) {
                todo.push(d);
            }
        }
        for d in todo {
            self.chat_receipt_send(core, sess, &d, writer).await?;
        }
        Ok(())
    }

    /// Sends what the session's device lacks of one day's receipt ledger (bounded).
    pub(super) async fn chat_receipt_send(self: &Arc<Self>, core: &Arc<ChatCore>, sess: &Arc<Session>, day: &str, writer: &mut ChatWriter) -> Result<(), Error> {
        let me = self.me()?;
        let pair = pair_id(me.id.did(), &sess.did);
        if core.store.shard_meta(&pair, day)?.is_none() {
            return Ok(());
        }
        let scope = dm_scope(&pair, day);
        for _ in 0..RECEIPT_FRAMES_PER_SEND {
            let theirs = sess.receipt_vv.lock().get(day).cloned().unwrap_or_default();
            let Some(batch) = core.receipts.export(&scope, &theirs, RECEIPT_BATCH).map_err(rio)? else { return Ok(()) };
            writer.send(&ChatMsg::Receipts { day: day.to_string(), update: batch.update }).await?;
            merge_vv(sess.receipt_vv.lock().entry(day.to_string()).or_default(), &batch.frontier);
            if !batch.more {
                break;
            }
        }
        Ok(())
    }

    pub(super) async fn chat_receipts_in(self: &Arc<Self>, core: &Arc<ChatCore>, sess: &Arc<Session>, day: &str, update: &[u8], writer: &mut ChatWriter) -> Result<(), Error> {
        if !day_valid(day) {
            return Err(Error::Protocol("bad day".into()));
        }
        let me = self.me()?;
        let pair = pair_id(me.id.did(), &sess.did);
        let scope = dm_scope(&pair, day);
        // Evidence attaches to a day we hold; anything else is not stored.
        let fresh = if core.store.shard_meta(&pair, day)?.is_none() {
            vec![]
        } else {
            match core.receipts.import_direct(&scope, &sess.did, update) {
                Ok(f) => f,
                Err(e) => {
                    // Not stored, not projected; the chat session itself goes on.
                    self.log(format!("rejected receipts of {day} from {}: {e}", sess.did));
                    vec![]
                }
            }
        };
        let vv = core.receipts.vv(&scope).map_err(rio)?;
        writer.send(&ChatMsg::ReceiptAck { day: day.to_string(), vv: vv.encode() }).await?;
        if !fresh.is_empty() {
            self.chat_receipts_changed(core, &me, &sess.did, &pair, day, &fresh).await;
        }
        Ok(())
    }

    pub(super) async fn chat_receipt_ack_in(self: &Arc<Self>, core: &Arc<ChatCore>, sess: &Arc<Session>, day: &str, vv: &[u8], _writer: &mut ChatWriter) -> Result<(), Error> {
        if !day_valid(day) {
            return Err(Error::Protocol("bad day".into()));
        }
        let me = self.me()?;
        let pair = pair_id(me.id.did(), &sess.did);
        let vv = decode_vv(vv)?;
        if core.store.shard_meta(&pair, day)?.is_some() {
            core.receipts.acknowledged(&dm_scope(&pair, day), &vv).map_err(rio)?;
        }
        // Authoritative: what they hold (a refused frame is resent on the next trigger).
        sess.receipt_vv.lock().insert(day.to_string(), vv);
        Ok(())
    }

    /// After a committed import: our outgoing messages whose shown version the
    /// contact's new statements cover changed their receipt status.
    async fn chat_receipts_changed(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Me, did: &str, pair: &str, day: &str, fresh: &[crate::receipts::doc::Entry]) {
        let Some(ev) = self.ev() else { return };
        let covered: std::collections::BTreeSet<[u8; 32]> =
            fresh.iter().filter(|e| e.receipt.recipient == did).flat_map(|e| e.receipt.messages.iter().copied()).collect();
        if covered.is_empty() {
            return;
        }
        let msgs = match self.chat_shard(core, pair, day).await {
            Ok(shards) => shards.get(&(pair.to_string(), day.to_string())).and_then(|s| s.messages().ok()),
            Err(_) => None,
        };
        let doc = doc_name(pair, day);
        for rec in msgs.unwrap_or_default().values() {
            if rec.author == me.id.did() && covered.contains(&dm_version(&doc, rec)) {
                ev.on_message_changed(self.chat_api_message(core, me, did, pair, day, rec));
            }
        }
    }

    /// The status of one stored version: committed recipient evidence plus the
    /// display tick. The tick's only legacy input is "this device still owes it"
    /// (`DeliveryState::Pending`); an unsigned transport ack never becomes Delivered.
    fn chat_receipt_status(core: &ChatCore, me: &Me, did: &str, pair: &str, day: &str, rec: &MsgRec) -> Result<api::MessageReceiptStatus, Error> {
        let version = dm_version(&doc_name(pair, day), rec);
        let mut recipients = Vec::new();
        let outgoing = rec.author == me.id.did();
        if did != me.id.did() {
            // Our message: the contact confirms it. Theirs: our own account's
            // statements (what this account told, or will tell, the author).
            let recipient = if outgoing { did.to_string() } else { me.id.did().to_string() };
            let ev: VersionEvidence = core.receipts.evidence(&dm_scope(pair, day), &version, |d| d == recipient).map_err(rio)?;
            recipients.push(api::recipient(&recipient, &ev));
        }
        let tick = if !outgoing || did == me.id.did() || rec.deleted {
            api::ReceiptTick::None
        } else {
            match recipients.first().map(|r| r.level) {
                Some(api::ReceiptLevel::Read) => api::ReceiptTick::Read,
                Some(api::ReceiptLevel::Delivered) => api::ReceiptTick::Delivered,
                _ if Self::chat_delivery(core, pair, day, &rec.id) == DeliveryState::Pending => api::ReceiptTick::OnlyHere,
                _ => api::ReceiptTick::Waiting,
            }
        };
        Ok(api::MessageReceiptStatus { message_id: rec.id.clone(), version: hex::encode(version), tick, recipients })
    }

    /// Recipient evidence for every message of one day page (one shard and one ledger
    /// read), in the page's order. For list rendering instead of one call per row.
    pub(crate) async fn chat_day_receipts(self: &Arc<Self>, did: &str, day: &str) -> Result<Vec<api::MessageReceiptStatus>, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        self.chat_contact(did, false)?;
        if !day_valid(day) {
            return Err(Error::Protocol("bad day".into()));
        }
        let pair = pair_id(me.id.did(), did);
        if core.store.shard_meta(&pair, day)?.is_none() {
            return Ok(vec![]);
        }
        let mut recs: Vec<MsgRec> = {
            let shards = self.chat_shard(&core, &pair, day).await?;
            let shard = shards.get(&(pair.clone(), day.to_string())).ok_or(Error::NotFound)?;
            shard.messages().map_err(|e| Error::Io(e.to_string()))?.into_values().collect()
        };
        recs.sort_by(|a, b| a.order().cmp(&b.order()));
        recs.iter().map(|rec| Self::chat_receipt_status(&core, &me, did, &pair, day, rec)).collect()
    }

    /// Recipient evidence for the version of `id` this device shows.
    pub(crate) async fn chat_message_receipts(self: &Arc<Self>, did: &str, id: &str) -> Result<api::MessageReceiptStatus, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        self.chat_contact(did, false)?;
        let pair = pair_id(me.id.did(), did);
        let day = core.store.mi(&pair, id)?.ok_or(Error::NotFound)?;
        let rec = {
            let shards = self.chat_shard(&core, &pair, &day).await?;
            let shard = shards.get(&(pair.clone(), day.clone())).ok_or(Error::NotFound)?;
            shard.messages().map_err(|e| Error::Io(e.to_string()))?.remove(id).ok_or(Error::NotFound)?
        };
        Self::chat_receipt_status(&core, &me, did, &pair, &day, &rec)
    }

    /// The status of each conversation's last message, when it is ours (the chat
    /// list's tick). Reads stored rows only; never a cached doc ahead of the store.
    pub(crate) fn chat_list_receipts(&self) -> Result<Vec<api::ChatReceiptRow>, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        let dids: Vec<String> = self.shared.lock().state.contacts.iter().map(|c| c.did.clone()).collect();
        let mut out = Vec::new();
        for did in dids {
            let pair = pair_id(me.id.did(), &did);
            let Some(last) = core.store.conv(&pair)?.and_then(|c| c.last) else { continue };
            if !last.outgoing {
                continue;
            }
            let Some(day) = core.store.mi(&pair, &last.id)? else { continue };
            let Some(rec) = Self::chat_stored_messages(&core, &pair, &day)?.and_then(|mut m| m.remove(&last.id)) else { continue };
            out.push(api::ChatReceiptRow { peer_did: did.clone(), status: Self::chat_receipt_status(&core, &me, &did, &pair, &day, &rec)? });
        }
        Ok(out)
    }
}
