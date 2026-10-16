//! Group member receipts in the engine (group step 1): issuance intents in the transactions
//! that accept posts or move the read cursor, the signing pump, policy-checked import, export
//! for dissemination and the status aggregate. Rules: `group::member_receipts`.
//!
//! Not wired to the group wire (`tinline/group/3`) yet: `member_receipts_export` /
//! `member_receipts_import` are what a receipts frame would carry (see the receipt roadmap,
//! "Group message receipts"). Deterministic multi-node tests drive them in process.

use std::collections::BTreeMap;

use super::*;
use crate::group::member_receipts::{
    GroupReceiptStatus, PostRef, admit, aggregate, may_issue, scope,
};
use crate::group::shard::{PostRow, Shard};
use crate::receipts::doc::Entry as ReceiptEntry;
use crate::receipts::store::messages::{Issued, MessageReceipts, group_version};
use crate::receipts::{Kind, ReceiptError};
use storage::Op;

/// Provisional: newest posts one mark-read step can confirm (as in 1:1). Older ones stay
/// without a read receipt (a hole, never a claim).
pub(crate) const MAX_READ_POSTS: usize = 4096;

fn rio(e: ReceiptError) -> Error {
    Error::Io(format!("group receipts: {e}"))
}

/// The receipt controller over the group store's own sealed namespace (one per store).
pub(crate) fn controller(store: &GroupStore, did: &str, dev: [u8; 32]) -> Arc<MessageReceipts> {
    MessageReceipts::new(
        crate::receipts::store::ReceiptStore::new(store.db.clone()),
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
                    crate::group::short(gid)
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
    /// we were a member for. Call `member_receipts_pump` after it.
    pub(crate) fn mark_read(&self, gid: &str) -> Result<(), Error> {
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
        let extra = self.read_ops(live, gid, old, new).unwrap_or_else(|e| {
            tracing::warn!("group {}: no read intent: {e}", crate::group::short(gid));
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
        found.sort_by(|a, b| b.0.cmp(&a.0));
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
    pub(crate) fn member_receipts_pump(&self) -> Result<Vec<Issued>, ReceiptError> {
        self.receipts.pump(&self.account)
    }

    /// Imports a member receipt update for day `day` of `gid` (from any relay: the verdict
    /// depends only on the statements, our records and posts). Refused statements are not
    /// stored; a `pending: ` policy error waits for records/posts we lack. Returns the new
    /// committed statements.
    pub(crate) fn member_receipts_import(
        &self,
        gid: &str,
        day: &str,
        update: &[u8],
    ) -> Result<Vec<ReceiptEntry>, ReceiptError> {
        if !crate::group::doc::day_valid(day) {
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
    pub(crate) fn member_receipts_export(
        &self,
        gid: &str,
        day: &str,
        theirs: &VersionVector,
        limits: crate::group::doc::batching::BatchLimits,
    ) -> Result<Option<crate::group::doc::batching::ExportBatch>, ReceiptError> {
        self.receipts.export(&scope(gid, day), theirs, limits)
    }

    /// Committed frontier of one day's receipt ledger.
    pub(crate) fn member_receipts_vv(
        &self,
        gid: &str,
        day: &str,
    ) -> Result<VersionVector, ReceiptError> {
        self.receipts.vv(&scope(gid, day))
    }

    /// The status of post `post_id` of day doc `day`: the sender's mark and per-member detail.
    pub(crate) fn member_receipt_status(
        &self,
        gid: &str,
        day: &str,
        post_id: &str,
    ) -> Result<GroupReceiptStatus, Error> {
        let mut out = self.member_day_receipts_where(gid, day, |id| id == post_id)?;
        out.pop().ok_or(Error::NotFound)
    }

    /// `member_receipt_status` for every visible post of one day doc, oldest first.
    pub(crate) fn member_day_receipts(
        &self,
        gid: &str,
        day: &str,
    ) -> Result<Vec<GroupReceiptStatus>, Error> {
        self.member_day_receipts_where(gid, day, |_| true)
    }

    fn member_day_receipts_where(
        &self,
        gid: &str,
        day: &str,
        want: impl Fn(&str) -> bool,
    ) -> Result<Vec<GroupReceiptStatus>, Error> {
        if !crate::group::doc::day_valid(day) {
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
            .filter(|r| r.vis == Vis::Visible && want(&r.entry.id))
            .filter_map(|r| post_of(shard, r))
            .map(|(p, d)| aggregate(state, &self.did, &p, &d, &entries))
            .collect())
    }
}
