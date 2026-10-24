//! The group half of the search reconcile (`docs/search.md`): on open, each group day doc whose
//! watermark differs from its version (or that has pending posts) is diffed against the index.
//! The 1:1 half and the queries are p2pcore's `search::api`.

use std::collections::HashSet;
use std::sync::Arc;

use search_core::index::{DocRow, Mark};
use search_ix::{NO_BG_RECONCILE, RECONCILE_BATCH, group_row, group_src};

use crate::GroupRt;
use crate::engine::{GroupCore, Live};
use crate::shard::Shard;
use crate::state::Item;

impl GroupRt {
    /// Per day doc of each group, as for DMs: each whose watermark differs from its version (and
    /// each with pending posts) is diffed against the index: the posts that show are written, the
    /// docs of that day that no longer show are dropped. In the background.
    pub fn search_reconcile_groups(self: &Arc<Self>, core: &Arc<GroupCore>) {
        if NO_BG_RECONCILE.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        drop(self.search_reconcile_groups_run(core));
    }

    pub fn search_reconcile_groups_run(self: &Arc<Self>, core: &Arc<GroupCore>) -> n0_future::task::JoinHandle<()> {
        let Some(ix) = self.search.index() else { return self.handle.spawn_blocking(|| {}) };
        let core = core.clone();
        self.handle.spawn_blocking(move || {
            let gids: Vec<String> = core.groups.lock().keys().cloned().collect();
            for gid in gids {
                let src = group_src(&gid);
                let days = core.store.days(&gid).unwrap_or_default();
                let (mut batch, mut stale, mut marks): (Vec<DocRow>, Vec<String>, Vec<Mark>) = Default::default();
                let pending = core.groups.lock().get(&gid).map(|l| l.meta.pending_days.clone()).unwrap_or_default();
                for day in days {
                    if ix.is_closed() || core.closed() {
                        return;
                    }
                    let Some(sm) = core.store.shard_meta(&gid, &day).ok().flatten() else { continue };
                    if !pending.contains(&day) && ix.watermark_is(&src, &day, &sm.vv) {
                        continue;
                    }
                    // The day as cached, or read once without disturbing the cache.
                    let rows: Vec<DocRow> = {
                        let mut groups = core.groups.lock();
                        let Some(Live { state, shards, .. }) = groups.get_mut(&gid) else { break };
                        let items: Vec<Item> = match shards.peek(&day) {
                            Some(s) => s.visible(state).collect(),
                            None => match Shard::load(&core.store, &gid, core.wid, &day, state) {
                                Ok(s) => s.visible(state).collect(),
                                Err(e) => {
                                    tracing::debug!("search reconcile {} {day}: {e}", crate::short(&gid));
                                    continue;
                                }
                            },
                        };
                        items.iter().map(|i| group_row(&gid, &day, i)).collect()
                    };
                    let live: HashSet<String> = rows.iter().map(|r| ix.doc_key(&src, &r.id)).collect();
                    // A day the index has never seen (a rebuild) has nothing stale to find, which
                    // keeps a full rebuild from scanning the group's keys once per day.
                    if ix.has_watermark(&src, &day) {
                        match ix.day_keys(&src, &day) {
                            Ok(k) => stale.extend(k.into_iter().filter(|h| !live.contains(h))),
                            Err(e) => return Self::search_log(e),
                        }
                    }
                    batch.extend(rows);
                    marks.push(Mark { src: src.clone(), day: day.clone(), prev: None, vv: sm.vv });
                    // Days go to the index together (a transaction per few thousand posts): one
                    // transaction per day would cost more than the posts themselves.
                    if batch.len() >= RECONCILE_BATCH
                        && let Err(e) = ix.apply_bulk(&std::mem::take(&mut batch), &std::mem::take(&mut stale), &std::mem::take(&mut marks))
                    {
                        return Self::search_log(e);
                    }
                }
                if let Err(e) = ix.apply_bulk(&batch, &stale, &marks) {
                    return Self::search_log(e);
                }
            }
        })
    }

    fn search_log(e: impl std::fmt::Display) {
        tracing::warn!("search index: {e}");
    }
}
