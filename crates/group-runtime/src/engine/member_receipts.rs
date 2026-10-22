//! Group member receipts in the engine (group step 1): issuance intents in the transactions
//! that accept posts or move the read cursor, the signing pump, policy-checked import, export
//! for dissemination and the status aggregate. Rules: `group::member_receipts`.
//!
//! Wire (`tinline/group/5`, between members and hosts only, U7):
//! - After a session's doc catch-up each side sends `ReceiptHello` (its ledger VV for the recent
//!   days it holds plus its unacknowledged outbox days). The answer is `Receipts` for every day
//!   where the other side lacks something; each import is answered by `ReceiptAck` (what we
//!   hold now, which also clears our outbox mark). The sender merges it into what it already
//!   sent: an Ack answers one frame while later ones may still be in flight, and taking it as
//!   the whole truth sent every in-flight statement again (each resend answered by another
//!   stale Ack). A hello is what resets it.
//! - New statements (our pump, or fresh ones imported from a session) are pushed live to every
//!   other session of the group. A relay re-exports the ledger's ORIGINAL native ops: every
//!   statement keeps its signer's signed bytes, writer and counter byte for byte.
//! - A frame refused as pending (posts/records we lack) is not stored; the day is remembered on
//!   the session and asked again with `ReceiptHello` after our docs advance. Any other refusal
//!   is logged; the session goes on (a receipt never breaks group sync).
//! - Reconnect = a new session: hello again, so lost frames are recovered from VVs.
//! - Public groups (`docs/design/public-group.md` §4): while a group is open (or we are joining
//!   one by its open link) no delivered/read intent is recorded, no `ReceiptHello`/`Receipts`/
//!   `ReceiptAck` is sent, and incoming ones are dropped unread. Own-post delivery (`Waiting` ->
//!   `Delivered`) comes from host `Ack`s, not receipts, and is unchanged. Nothing is backdated:
//!   posts that arrived while open get no receipt if the group closes later.

use std::collections::BTreeMap;

use super::*;
use crate::member_receipts::{
    GroupReceiptStatus, PostRef, admit, aggregate, may_issue, scope,
};
use crate::shard::{PostRow, Shard};
use receipts_core::doc::Entry as ReceiptEntry;
use receipts_core::store::messages::{Issued, MessageReceipts, group_version};
use receipts_core::{Kind, ReceiptError};
use storage::Op;

/// Provisional: newest posts one mark-read step can confirm (as in 1:1). Older ones stay
/// without a read receipt (a hole, never a claim).
pub const MAX_READ_POSTS: usize = 4096;
/// Provisional: days one `ReceiptHello` lists.
pub const MAX_RECEIPT_DAYS: usize = 64;
/// Provisional per-frame native receipt budget (well under `MAX_FRAME`; as chat/6).
pub const RECEIPT_BATCH: crate::doc::batching::BatchLimits =
    crate::doc::batching::BatchLimits {
        bytes: 256 * 1024,
        operations: 64,
        writers: 4096,
    };
/// Provisional: frames one send pass writes for a day; the rest follows the next trigger.
pub const RECEIPT_FRAMES_PER_SEND: usize = 16;

fn rio(e: ReceiptError) -> Error {
    Error::Io(format!("group receipts: {e}"))
}

/// The receipt controller over the group store's own sealed namespace (one per store).
pub fn controller(store: &GroupStore, did: &str, dev: [u8; 32]) -> Arc<MessageReceipts> {
    MessageReceipts::new(
        receipts_core::store::ReceiptStore::new(store.db.clone()),
        did,
        dev,
    )
}

/// One stored post as a receipt decision sees it, with its exact version digest.
fn post_of(shard: &Shard, row: &PostRow) -> Option<(PostRef, [u8; 32])> {
    let raw = shard.doc.raw_value(&row.entry.id)?;
    Some((
        PostRef {
            id: row.entry.id.clone(),
            author: row.entry.rec.author.clone(),
            seen: row.entry.rec.seen.clone(),
            vis: row.vis,
        },
        group_version(&raw),
    ))
}

fn posts_of(shard: &Shard) -> BTreeMap<[u8; 32], PostRef> {
    shard
        .rows
        .iter()
        .filter_map(|r| post_of(shard, r))
        .map(|(p, d)| (d, p))
        .collect()
}

impl GroupCore {
    /// Delivered-intent ops for posts that just became durable and visible here, for the
    /// transaction that stores them. `frontier` is our meta frontier now (signed into each
    /// receipt). A failure to build them is logged and never blocks accepting the posts.
    pub(super) fn delivered_ops(
        &self,
        state: &GroupState,
        frontier: &[(u64, i32)],
        gid: &str,
        day: &str,
        posts: &[(PostRef, [u8; 32])],
    ) -> Vec<Op> {
        if state.open {
            return Vec::new();
        }
        let digests: Vec<[u8; 32]> = posts
            .iter()
            .filter(|(p, _)| may_issue(state, &self.did, frontier, p))
            .map(|(_, d)| *d)
            .collect();
        if digests.is_empty() {
            return Vec::new();
        }
        self.receipts
            .intent_ops_in_context(Kind::Delivered, &scope(gid, day), digests, frontier)
            .unwrap_or_else(|e| {
                tracing::warn!(
                    "group {} day {day}: no delivered intent: {e}",
                    crate::short(gid)
                );
                Vec::new()
            })
    }

    /// Posts a fresh day-doc batch made durable (already inserted into `shard`).
    pub(super) fn fresh_posts(shard: &Shard, ids: &[String]) -> Vec<(PostRef, [u8; 32])> {
        shard
            .rows
            .iter()
            .filter(|r| ids.contains(&r.entry.id))
            .filter_map(|r| post_of(shard, r))
            .collect()
    }

    /// Delivered-intent ops for posts a meta update made show (they were pending).
    pub(super) fn rejudged_ops(&self, live: &mut Live, gid: &str, diffs: &[PostDiff]) -> Vec<Op> {
        let frontier = live.doc.seen();
        let mut ops = Vec::new();
        for diff in diffs {
            let Some(shard) = live.shards.peek(&diff.day) else {
                continue;
            };
            let ids: Vec<String> = diff.put.iter().map(|i| i.id.clone()).collect();
            let posts = Self::fresh_posts(shard, &ids);
            ops.extend(self.delivered_ops(&live.state, &frontier, gid, &diff.day, &posts));
        }
        ops
    }

    /// Moves our read cursor to the newest line (monotonic) and, in the same transaction,
    /// records read intents for the visible posts of others in (old cursor, new cursor] that
    /// we were a member for. With `read_receipts` off (the account setting) the cursor moves
    /// and nothing is recorded: turning it on later never confirms those reads (no backdating).
    /// Call `member_receipts_pump` after it.
    pub fn mark_read(&self, gid: &str, read_receipts: bool) -> Result<(), Error> {
        let mut groups = self.groups.lock();
        let live = groups.get_mut(gid).ok_or(Error::NotFound)?;
        let old = live.meta.read_upto;
        let last_post = live.meta.last.as_ref().map_or(0, |b| b.at);
        let new = live
            .state
            .items
            .last()
            .map_or(0, |i| i.at)
            .max(last_post)
            .max(old);
        // Public group §4: no read receipts while the group is open.
        let read = if read_receipts && !live.state.open {
            self.read_ops(live, gid, old, new)
        } else {
            Ok(Vec::new())
        };
        let extra = read.unwrap_or_else(|e| {
            tracing::warn!("group {}: no read intent: {e}", crate::short(gid));
            Vec::new()
        });
        let mut meta = live.meta.clone();
        meta.read_upto = new;
        meta.unread = 0;
        self.store.put_meta_with(gid, &meta, extra)?;
        live.meta = meta;
        Ok(())
    }

    fn read_ops(&self, live: &mut Live, gid: &str, from: i64, to: i64) -> Result<Vec<Op>, Error> {
        if to <= from {
            return Ok(Vec::new());
        }
        // A post's `at` may sit up to an hour outside its day doc's day.
        let lo = day_of(from.saturating_sub(DAY_MS).max(0));
        let hi = day_of(to.saturating_add(DAY_MS));
        let frontier = live.doc.seen();
        let Live { state, shards, .. } = live;
        let mut found: Vec<(i64, String, [u8; 32])> = Vec::new();
        for day in self.store.days(gid)?.into_iter().rev() {
            if day > hi {
                continue;
            }
            if day < lo || found.len() >= MAX_READ_POSTS {
                break;
            }
            let shard = shards.get(&self.store, gid, self.wid, &day, state)?;
            for r in &shard.rows {
                if r.entry.rec.at > from
                    && r.entry.rec.at <= to
                    && let Some((p, d)) = post_of(shard, r)
                    && may_issue(state, &self.did, &frontier, &p)
                {
                    found.push((r.entry.rec.at, day.clone(), d));
                }
            }
        }
        found.sort_by_key(|f| std::cmp::Reverse(f.0));
        found.truncate(MAX_READ_POSTS);
        let mut by_day: BTreeMap<String, Vec<[u8; 32]>> = BTreeMap::new();
        for (_, day, d) in found {
            by_day.entry(day).or_default().push(d);
        }
        let mut ops = Vec::new();
        for (day, ds) in by_day {
            ops.extend(
                self.receipts
                    .intent_ops_in_context(Kind::Read, &scope(gid, &day), ds, &frontier)
                    .map_err(rio)?,
            );
        }
        Ok(ops)
    }

    /// Signs and commits pending receipt intents (bounded); failures stay queued.
    pub fn member_receipts_pump(&self) -> Result<Vec<Issued>, ReceiptError> {
        self.receipts.pump(&self.account)
    }

    /// `member_receipts_pump`, then the new statements go to every session of their group.
    pub fn member_receipts_pump_and_push(&self) {
        let issued = match self.member_receipts_pump() {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!("group receipts: {e}");
                return;
            }
        };
        let mut days = std::collections::BTreeSet::new();
        for i in issued {
            if let receipts_core::Conversation::Group(gid) = &i.scope.conversation {
                days.insert((gid.clone(), i.scope.day.clone()));
            }
        }
        for (gid, day) in days {
            self.receipts_push(&gid, &day, None);
        }
    }

    // ---- wire ------------------------------------------------------------------------------

    /// Public group §4: no receipt traffic for `gid` while it is open, or while we are joining it
    /// by its open link (before its records arrive we cannot tell it is open).
    pub fn receipts_off(&self, gid: &str) -> bool {
        self.groups
            .lock()
            .get(gid)
            .is_some_and(|l| l.state.open || l.meta.open_join)
    }

    /// The `ReceiptHello` a new session starts with; none while receipts are off.
    pub fn session_receipt_hello(&self, gid: &str) -> Option<GroupMsg> {
        (!self.receipts_off(gid)).then(|| self.receipt_hello(gid))
    }

    /// Our `ReceiptHello` for `gid`: recent held days plus unacknowledged outbox days.
    pub fn receipt_hello(&self, gid: &str) -> GroupMsg {
        let mut days = self.held_recent_days(gid);
        let group = receipts_core::Conversation::Group(gid.to_string());
        for d in self
            .receipts
            .outbox_days(&group, MAX_RECEIPT_DAYS)
            .unwrap_or_default()
        {
            if !days.contains(&d) {
                days.push(d);
            }
        }
        self.receipt_hello_for(gid, days)
    }

    fn receipt_hello_for(&self, gid: &str, days: Vec<String>) -> GroupMsg {
        let days = days
            .into_iter()
            .filter(|d| self.store.shard_meta(gid, d).ok().flatten().is_some())
            .take(MAX_RECEIPT_DAYS)
            .filter_map(|d| Some((d.clone(), self.member_receipts_vv(gid, &d).ok()?.encode())))
            .collect();
        GroupMsg::ReceiptHello { days }
    }

    /// The other side's `ReceiptHello`: remember what it holds, and send what it lacks of those
    /// days and of our outbox days.
    pub fn on_receipt_hello(
        &self,
        sess: &Sess,
        days: Vec<(String, Vec<u8>)>,
    ) -> Result<(), Error> {
        if days.len() > MAX_RECEIPT_DAYS {
            return Err(Error::Protocol("too many receipt days".into()));
        }
        if self.receipts_off(&sess.gid) {
            return Ok(());
        }
        let mut todo = Vec::new();
        for (d, vv) in days {
            if !crate::doc::day_valid(&d) {
                return Err(Error::Protocol("bad receipt day".into()));
            }
            let vv = decode_vv(&vv)?;
            if self.store.shard_meta(&sess.gid, &d)?.is_none() {
                continue;
            }
            // What they hold may complete our outbox.
            self.receipts
                .acknowledged(&scope(&sess.gid, &d), &vv)
                .map_err(rio)?;
            sess.receipt_vv.lock().insert(d.clone(), vv);
            todo.push(d);
        }
        let group = receipts_core::Conversation::Group(sess.gid.clone());
        for d in self
            .receipts
            .outbox_days(&group, MAX_RECEIPT_DAYS)
            .map_err(rio)?
        {
            if !todo.contains(&d) {
                todo.push(d);
            }
        }
        for d in todo {
            self.receipts_send(sess, &d)?;
        }
        Ok(())
    }

    /// Sends `sess` what it lacks of one day's ledger (bounded frames per call).
    pub fn receipts_send(&self, sess: &Sess, day: &str) -> Result<(), Error> {
        if self.receipts_off(&sess.gid) || self.store.shard_meta(&sess.gid, day)?.is_none() {
            return Ok(());
        }
        for _ in 0..RECEIPT_FRAMES_PER_SEND {
            let theirs = sess.receipt_vv.lock().get(day).cloned().unwrap_or_default();
            let Some(batch) = self
                .member_receipts_export(&sess.gid, day, &theirs, RECEIPT_BATCH)
                .map_err(rio)?
            else {
                return Ok(());
            };
            let _ = sess.tx.send(Out::Msg(GroupMsg::Receipts {
                day: day.to_string(),
                update: batch.update,
            }));
            sess.receipt_vv
                .lock()
                .entry(day.to_string())
                .or_default()
                .merge(&batch.frontier);
            if !batch.more {
                break;
            }
        }
        Ok(())
    }

    /// New statements of one day to every session of `gid` except `from`.
    pub fn receipts_push(&self, gid: &str, day: &str, from: Option<u64>) {
        for s in self.sessions_of(gid) {
            if Some(s.id) == from {
                continue;
            }
            if let Err(e) = self.receipts_send(&s, day) {
                tracing::warn!(
                    "group {} receipts to {}: {e}",
                    crate::short(gid),
                    s.did
                );
            }
        }
    }

    /// A `Receipts` frame: import under the member policy, answer `ReceiptAck`, relay fresh
    /// statements to the other sessions. Returns the fresh statements.
    pub fn on_receipts(
        &self,
        sess: &Sess,
        day: &str,
        update: &[u8],
    ) -> Result<Vec<ReceiptEntry>, Error> {
        if !crate::doc::day_valid(day) {
            return Err(Error::Protocol("bad receipt day".into()));
        }
        // Public group §4: dropped unread, unanswered.
        if self.receipts_off(&sess.gid) {
            return Ok(vec![]);
        }
        // Evidence attaches to a day we hold; anything else waits for the day.
        let fresh = if self.store.shard_meta(&sess.gid, day)?.is_none() {
            sess.receipt_waiting.lock().insert(day.to_string());
            vec![]
        } else {
            match self.member_receipts_import(&sess.gid, day, update) {
                Ok(f) => f,
                Err(e) => {
                    let pending = matches!(&e, ReceiptError::Pending)
                        || matches!(&e, ReceiptError::Policy(m) if m.starts_with(crate::member_receipts::PENDING_PREFIX));
                    if pending {
                        sess.receipt_waiting.lock().insert(day.to_string());
                    }
                    tracing::info!(
                        "group {}: receipts of {day} from {} not stored: {e}",
                        crate::short(&sess.gid),
                        sess.did
                    );
                    vec![]
                }
            }
        };
        let vv = self.member_receipts_vv(&sess.gid, day).map_err(rio)?;
        let _ = sess.tx.send(Out::Msg(GroupMsg::ReceiptAck {
            day: day.to_string(),
            vv: vv.encode(),
        }));
        if !fresh.is_empty() {
            self.receipts_push(&sess.gid, day, Some(sess.id));
        }
        Ok(fresh)
    }

    /// A `ReceiptAck`: what the peer holds now; may clear our outbox mark. Merged into what we
    /// sent since (later frames may be in flight); refused frames come back through the peer's
    /// `ReceiptHello`.
    pub fn on_receipt_ack(&self, sess: &Sess, day: &str, vv: &[u8]) -> Result<(), Error> {
        if !crate::doc::day_valid(day) {
            return Err(Error::Protocol("bad receipt day".into()));
        }
        let vv = decode_vv(vv)?;
        if self.receipts_off(&sess.gid) {
            return Ok(());
        }
        if self.store.shard_meta(&sess.gid, day)?.is_some() {
            self.receipts
                .acknowledged(&scope(&sess.gid, day), &vv)
                .map_err(rio)?;
        }
        sess.receipt_vv
            .lock()
            .entry(day.to_string())
            .or_default()
            .merge(&vv);
        Ok(())
    }

    /// After our docs of `gid` advanced: ask again for the days whose receipts waited.
    pub fn receipts_rehello(&self, gid: &str) {
        if self.receipts_off(gid) {
            return;
        }
        for s in self.sessions_of(gid) {
            let days: Vec<String> = std::mem::take(&mut *s.receipt_waiting.lock())
                .into_iter()
                .collect();
            if !days.is_empty() {
                let _ = s.tx.send(Out::Msg(self.receipt_hello_for(gid, days)));
            }
        }
    }

    /// Imports a member receipt update for day `day` of `gid` (from any relay: the verdict
    /// depends only on the statements, our records and posts). Refused statements are not
    /// stored; a `pending: ` policy error waits for records/posts we lack. Returns the new
    /// committed statements.
    pub fn member_receipts_import(
        &self,
        gid: &str,
        day: &str,
        update: &[u8],
    ) -> Result<Vec<ReceiptEntry>, ReceiptError> {
        if !crate::doc::day_valid(day) {
            return Err(ReceiptError::Bounds);
        }
        let mut groups = self.groups.lock();
        let live = groups.get_mut(gid).ok_or(ReceiptError::Scope)?;
        let Live { state, shards, .. } = live;
        let posts = if self
            .store
            .shard_meta(gid, day)
            .map_err(|e| ReceiptError::Store(e.to_string()))?
            .is_some()
        {
            posts_of(
                shards
                    .get(&self.store, gid, self.wid, day, state)
                    .map_err(|e| ReceiptError::Store(e.to_string()))?,
            )
        } else {
            BTreeMap::new()
        };
        let state: &GroupState = state;
        self.receipts
            .import_checked(&scope(gid, day), update, |fresh| {
                admit(state, &posts, fresh)
            })
    }

    /// Native receipt ops of one day a peer holding `theirs` lacks (bounded exporter).
    pub fn member_receipts_export(
        &self,
        gid: &str,
        day: &str,
        theirs: &VersionVector,
        limits: crate::doc::batching::BatchLimits,
    ) -> Result<Option<crate::doc::batching::ExportBatch>, ReceiptError> {
        self.receipts.export(&scope(gid, day), theirs, limits)
    }

    /// Committed frontier of one day's receipt ledger.
    pub fn member_receipts_vv(
        &self,
        gid: &str,
        day: &str,
    ) -> Result<VersionVector, ReceiptError> {
        self.receipts.vv(&scope(gid, day))
    }

    /// The status of post `post_id` of day doc `day`: the sender's mark and per-member detail.
    #[cfg(test)]
    pub fn member_receipt_status(
        &self,
        gid: &str,
        day: &str,
        post_id: &str,
    ) -> Result<GroupReceiptStatus, Error> {
        let mut out = self.member_day_receipts_where(gid, day, |e| e.id == post_id)?;
        out.pop().ok_or(Error::NotFound)
    }

    /// Statuses of the visible posts with `at` in `[from, to]` (author clock ms), only ours if
    /// `mine`: one ledger read per day doc in the range (a page's ticks).
    pub fn member_receipts_range(
        &self,
        gid: &str,
        from: i64,
        to: i64,
        mine: bool,
    ) -> Result<Vec<GroupReceiptStatus>, Error> {
        if to < from {
            return Ok(Vec::new());
        }
        // A post's `at` may sit up to an hour outside its day doc's day.
        let lo = day_of(from.saturating_sub(chat_core::doc::HOUR_MS).max(0));
        let hi = day_of(to.saturating_add(chat_core::doc::HOUR_MS));
        let mut out = Vec::new();
        for day in self.store.days(gid)? {
            if day < lo || day > hi {
                continue;
            }
            out.extend(self.member_day_receipts_where(gid, &day, |e| {
                e.rec.at >= from && e.rec.at <= to && (!mine || e.rec.author == self.did)
            })?);
        }
        Ok(out)
    }

    fn member_day_receipts_where(
        &self,
        gid: &str,
        day: &str,
        want: impl Fn(&Entry) -> bool,
    ) -> Result<Vec<GroupReceiptStatus>, Error> {
        if !crate::doc::day_valid(day) {
            return Err(Error::Protocol("bad day".into()));
        }
        let mut groups = self.groups.lock();
        let live = groups.get_mut(gid).ok_or(Error::NotFound)?;
        if self.store.shard_meta(gid, day)?.is_none() {
            return Ok(Vec::new());
        }
        let Live { state, shards, .. } = live;
        let shard = shards.get(&self.store, gid, self.wid, day, state)?;
        let entries = self.receipts.entries(&scope(gid, day)).map_err(rio)?;
        Ok(shard
            .rows
            .iter()
            .filter(|r| r.vis == Vis::Visible && want(&r.entry))
            .filter_map(|r| post_of(shard, r))
            .map(|(p, d)| aggregate(state, &self.did, &p, &d, &entries))
            .collect())
    }
}
