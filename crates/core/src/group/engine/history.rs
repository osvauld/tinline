//! Older days and compaction of day docs (`docs/group-shards.md` §3, §4).
//!
//! No separate history protocol: every day is an ordinary Loro doc and every member holds a copy
//! of all of them. A session exchanges an inventory (`Days`: every day held with its version
//! vector, in pages), then each side sends `Update` for every day the other lacks or is behind
//! on, newest first, a few a second. That sync running until both inventories match is the
//! backfill: a new member is filled by its host, a new or wiped host by the members connected to
//! it. Members still dial only hosts (U7) and never serve each other.

use super::*;
use crate::group::shard::CLOSE_AFTER_DAYS;

/// Pause between two day updates (a new member of a year-old group needs ~365 of them).
const BACKFILL_PAUSE_MS: u64 = 200;
const INVENTORY_WAIT: Duration = Duration::from_secs(30);

#[cfg(test)]
pub(super) fn export_day_for_durability_test(core: &GroupCore, gid: &str, day: &str, theirs: &VersionVector) -> Option<Vec<u8>> {
    core.day_update_since(gid, day, theirs, false)
}

impl GroupCore {
    /// Called only after the admitted host's meta Ack has been stored. The same
    /// lock as import prevents an old Ack from clearing a newer borrowed frontier.
    fn clear_own_meta_owed(&self, gid: &str, theirs: &VersionVector) -> Result<bool, Error> {
        let mut groups = self.groups.lock();
        let Some(live) = groups.get_mut(gid) else { return Ok(false) };
        if live.meta.own_owed_meta.is_empty() { return Ok(false); }
        let owed = decode_vv(&live.meta.own_owed_meta)?;
        if !matches!(theirs.partial_cmp(&owed), Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater)) {
            return Ok(false);
        }
        let mut meta = live.meta.clone();
        meta.own_owed_meta.clear();
        self.store.put_meta(gid, &meta)?;
        live.meta = meta;
        Ok(true)
    }

    /// Whether this session's peer may be given the days: a member we host (not removed, an
    /// accepted member, not a joiner), or a host we dialled.
    fn days_allowed(&self, sess: &Sess) -> bool {
        let groups = self.groups.lock();
        let Some(l) = groups.get(&sess.gid) else { return false };
        if sess.hosting { l.state.is_member(&sess.did) && !l.state.is_removed(&sess.did) } else { l.state.is_host(&sess.did, &sess.dev) }
    }

    /// What a peer holding `theirs` of day doc `day` lacks (read from the store, the cache is
    /// left alone); `None` if nothing. `to_host`: the peer is a host we dialled, so posts it
    /// would refuse stay out (`admit::upload_cut`).
    fn day_update_since(&self, gid: &str, day: &str, theirs: &VersionVector, to_host: bool) -> Option<Vec<u8>> {
        let cut = if to_host {
            let mut groups = self.groups.lock();
            let live = groups.get_mut(gid)?;
            let Live { state, shards, meta, .. } = live;
            let need = (state.admins_only && !state.is_admin(&self.did)) || meta.failed_posts.values().any(|f| f.day == day);
            if !need || state.is_host(&self.did, &self.dev) {
                Vec::new()
            } else {
                let shard = shards.get(&self.store, gid, self.wid, day, state).ok()?;
                admit::upload_cut(state, meta, &self.did, day, &shard.rows, theirs)
            }
        } else {
            Vec::new()
        };
        let (snap, ups) = self.store.shard_rows(gid, day).ok()?;
        let doc = GroupDoc::load_in(gid, DocId::Day(day.to_string()), self.wid, snap.as_deref(), &ups).ok()?;
        doc.export_since_before(theirs, &cut)
    }
}

impl Inner {
    #[cfg(test)]
    pub(crate) fn group_ack_for_durability_test(&self, core: &GroupCore, gid: &str, did: &str, dev: &[u8; 32], doc: &DocId, vv: &[u8]) {
        self.group_acked(core, gid, did, dev, doc, vv);
    }

    /// Records that `host` acknowledged holding `vv` of `doc`: delivery state of our posts, and
    /// whether we still owe the day to a host.
    pub(super) fn group_acked(&self, core: &GroupCore, gid: &str, did: &str, dev: &[u8; 32], doc: &DocId, vv: &[u8]) {
        let DocId::Day(day) = doc else {
            let Ok(theirs) = decode_vv(vv) else { return };
            if core.store.put_ack(gid, did, dev, vv).is_ok() {
                let _ = core.clear_own_meta_owed(gid, &theirs);
            }
            return;
        };
        if core.store.put_ack_day(gid, day, did, dev, vv).is_err() { return; }
        let Ok(theirs) = decode_vv(vv) else { return };
        let mut groups = core.groups.lock();
        let Some(live) = groups.get_mut(gid) else { return };
        // The obligation can include a phone's original writer, not core.wid.
        // Read the persisted frontier under the same group lock as append so a
        // concurrent new write cannot be cleared by an older acknowledgement.
        let Ok(Some(shard)) = core.store.shard_meta(gid, day) else { return };
        let Ok(held) = decode_vv(&shard.vv) else { return };
        let covered = matches!(theirs.partial_cmp(&held), Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater));
        if live.meta.owed_days.contains(day) && covered {
            let mut meta = live.meta.clone();
            meta.owed_days.remove(day);
            // An IO failure must leave the in-memory obligation intact too.
            if core.store.put_meta(gid, &meta).is_ok() { live.meta = meta; }
        }
    }

    /// Sends the peer our inventory of days and then every day it lacks or is behind on, newest
    /// first, a few a second; again 3 s later and then every 5 minutes (the peer may only now
    /// accept us, or have learnt more). Ends with the session.
    pub(super) async fn group_days_sync(self: Arc<Self>, core: Arc<GroupCore>, sess: Arc<Sess>) {
        let alive = || !core.closed() && core.sessions.lock().contains_key(&sess.id);
        for pass in 0u32.. {
            // Wait until we hold the group's meta records (and, on a host, until the peer is a
            // member), so what we receive can be judged and the peer will answer.
            loop {
                if !alive() {
                    return;
                }
                let ready = core.groups.lock().get(&sess.gid).is_some_and(|l| l.state.owner.is_some() && (!sess.hosting || l.state.is_member(&sess.did))) && core.days_allowed(&sess);
                if ready {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            if self.group_days_pass(&core, &sess, pass == 0, &alive).await.is_err() {
                return;
            }
            let wait = if pass == 0 { Duration::from_secs(3) } else { Duration::from_secs(300) };
            tokio::time::sleep(wait).await;
        }
    }

    async fn group_days_pass(self: &Arc<Self>, core: &Arc<GroupCore>, sess: &Arc<Sess>, first: bool, alive: &impl Fn() -> bool) -> Result<(), ()> {
        // Our inventory, in pages, ended by an empty page.
        let days = core.store.days(&sess.gid).unwrap_or_default();
        let mut after = None;
        for page in days.chunks(DAYS_PAGE) {
            let page: Vec<(String, Vec<u8>)> = page.iter().filter_map(|d| Some((d.clone(), core.store.shard_meta(&sess.gid, d).ok().flatten()?.vv))).collect();
            let next = page.last().map(|(d, _)| d.clone());
            sess.tx.send(Out::Msg(GroupMsg::Days { after: after.take(), days: page })).map_err(|_| ())?;
            after = next;
        }
        sess.tx.send(Out::Msg(GroupMsg::Days { after, days: Vec::new() })).map_err(|_| ())?;
        // The first pass waits for the peer's inventory (what it already holds); later ones use
        // what the acks and its updates told us since.
        if first {
            let end = Instant::now() + INVENTORY_WAIT;
            while !sess.inv_done.load(Ordering::SeqCst) && Instant::now() < end {
                if !alive() {
                    return Err(());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        // Newest first: every day the peer lacks or is behind on.
        for day in days.iter().rev() {
            while self.group.backfill_paused.load(Ordering::SeqCst) {
                if !alive() {
                    return Err(());
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            if !alive() {
                return Err(());
            }
            let theirs = sess.peer_days.lock().get(day).cloned().unwrap_or_default();
            let update = core.day_update_since(&sess.gid, day, &theirs, !sess.hosting);
            let Some(update) = update else { continue };
            let doc = DocId::Day(day.clone());
            if let Ok(meta) = loro::LoroDoc::decode_import_blob_meta(&update, false) {
                sess.peer_has(&doc, &meta.partial_end_vv);
            }
            sess.tx.send(Out::Msg(GroupMsg::Update { doc, update })).map_err(|_| ())?;
            let pause = match self.group.backfill_pause_ms.load(Ordering::SeqCst) {
                0 => BACKFILL_PAUSE_MS,
                v => v,
            };
            tokio::time::sleep(Duration::from_millis(pause)).await;
        }
        Ok(())
    }

    // ---- compaction ----------------------------------------------------------------------------

    /// Days at least `CLOSE_AFTER_DAYS` old become one snapshot row each (as in 1:1 chat);
    /// returns how many days it closed. Nothing is dropped.
    /// `group_compact` that also counts as the hourly run (tests: nothing compacts behind them).
    pub(crate) fn group_compact_now(&self, core: &GroupCore) -> u32 {
        *core.compacted.lock() = Some(std::time::Instant::now());
        self.group_compact(core)
    }

    pub(crate) fn group_compact(&self, core: &GroupCore) -> u32 {
        let _one = core.compacting.lock();
        let limit = day_of(now_ms() as i64 - CLOSE_AFTER_DAYS * DAY_MS);
        let mut closed = 0;
        let gids: Vec<String> = core.groups.lock().keys().cloned().collect();
        for gid in gids {
            for day in core.store.days(&gid).unwrap_or_default() {
                if day >= limit || core.closed() {
                    break;
                }
                // Under the group lock, so no update lands between the snapshot and the rows.
                let mut groups = core.groups.lock();
                let Some(live) = groups.get_mut(&gid) else { break };
                let Ok(Some(mut sm)) = core.store.shard_meta(&gid, &day) else { continue };
                if sm.closed {
                    continue;
                }
                let snap = match live.shards.peek_mut(&day) {
                    Some(s) => s.doc.snapshot(),
                    None => {
                        let Ok((snap, ups)) = core.store.shard_rows(&gid, &day) else { continue };
                        let Ok(doc) = GroupDoc::load_in(&gid, DocId::Day(day.clone()), core.wid, snap.as_deref(), &ups) else { continue };
                        doc.snapshot()
                    }
                };
                if let Err(e) = core.store.compact_shard(&gid, &day, &mut sm, &snap) {
                    tracing::warn!("group compaction: {e}");
                    continue;
                }
                if let Some(s) = live.shards.peek_mut(&day) {
                    s.meta = sm;
                }
                closed += 1;
            }
        }
        closed
    }
}
