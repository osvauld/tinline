//! Group core controls, durable document writes, and remote-update admission.

use super::*;

impl GroupCore {
    /// The version vector of a doc of `live` (the group's entry in `groups`, already locked).
    pub fn doc_vv_of(&self, live: &Live, gid: &str, doc: &DocId) -> VersionVector {
        match doc {
            DocId::Meta => live.doc.vv(),
            DocId::Day(d) => match live.shards.peek(d) {
                Some(s) => s.doc.vv(),
                None => self
                    .store
                    .shard_meta(gid, d)
                    .ok()
                    .flatten()
                    .and_then(|m| decode_vv(&m.vv).ok())
                    .unwrap_or_default(),
            },
        }
    }

    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// GL: someone is waiting for `gid` (a join): forget the dial backoff of its hosts and dial
    /// now, so a failed dial is tried again within seconds instead of at the slow backoff.
    pub fn hurry(&self, gid: &str) {
        self.backoff.lock().retain(|(g, _), _| g != gid);
        self.kick.notify_waiters();
    }

    /// Marks the core closed, ends its sessions and loops, and closes its store (for every
    /// holder, after the operations in flight). The endpoint is the caller's.
    pub fn shut(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.kick.notify_waiters();
        for s in self.sessions.lock().values() {
            let _ = s.tx.send(Out::Close);
        }
        self.store.db.close();
    }

    pub fn sessions_of(&self, gid: &str) -> Vec<Arc<Sess>> {
        self.sessions
            .lock()
            .values()
            .filter(|s| s.gid == gid)
            .cloned()
            .collect()
    }

    /// Test knob support: drops sessions with hosts other than `only`'s.
    pub fn close_sessions_except(&self, only: Option<&str>) {
        let Some(only) = only else { return };
        let mut ss = self.sessions.lock();
        ss.retain(|_, s| {
            let keep = s.hosting || s.did == only;
            if !keep {
                let _ = s.tx.send(Out::Close);
            }
            keep
        });
    }

    pub fn has_session(&self, gid: &str, did: &str, dev: &[u8; 32]) -> bool {
        self.sessions
            .lock()
            .values()
            .any(|s| s.gid == gid && s.did == did && &s.dev == dev)
    }

    /// The group endpoint keys this device holds for `gid` (hosts' addresses; on a host, also
    /// its members' directory; and every live session's remote) — for the U7 tests.
    pub fn known_endpoints(&self, gid: &str) -> Vec<String> {
        let mut out: HashSet<String> = HashSet::new();
        for a in self.store.hosts(gid).unwrap_or_default() {
            if let Some(a) = a.open().ok()
                && !(a.did == self.did && a.dev == self.dev)
            {
                out.insert(
                    iroh::PublicKey::from_bytes(&a.endpoint)
                        .map(|k| k.to_string())
                        .unwrap_or_default(),
                );
            }
        }
        for (_, d) in self.store.dir(gid).unwrap_or_default() {
            out.insert(d.endpoint);
        }
        let mut v: Vec<String> = out.into_iter().collect();
        v.sort();
        v
    }

    /// Writes one record of ours into `gid`; returns the entry and what changed. A post goes
    /// into the day doc of its time, every other record into the meta doc.
    pub fn write(&self, gid: &str, body: Body) -> Result<(Entry, Changed), Error> {
        self.write_at(gid, body, now_ms() as i64)
    }

    /// `write` with the record's time given (tests write into past days).
    pub fn write_at(&self, gid: &str, body: Body, at: i64) -> Result<(Entry, Changed), Error> {
        let mut groups = self.groups.lock();
        let live = groups.get_mut(gid).ok_or(Error::NotFound)?;
        let rec = Record {
            group: gid.to_string(),
            author: self.did.clone(),
            dev: self.dev,
            at,
            seen: live.doc.seen(),
            body,
        };
        if rec.body.is_post() {
            self.write_post(live, gid, rec)
        } else {
            self.write_meta(live, gid, rec)
        }
    }

    /// Test hook (a modified app): writes `body` claiming the signed frontier `seen` instead of
    /// our meta version vector.
    #[cfg(any(test, feature = "testkit"))]
    pub fn write_seen_for_test(&self, gid: &str, body: Body, seen: Vec<(u64, i32)>) -> Result<(Entry, Changed), Error> {
        let mut groups = self.groups.lock();
        let live = groups.get_mut(gid).ok_or(Error::NotFound)?;
        let rec = Record { group: gid.to_string(), author: self.did.clone(), dev: self.dev, at: now_ms() as i64, seen, body };
        if rec.body.is_post() { self.write_post(live, gid, rec) } else { self.write_meta(live, gid, rec) }
    }

    /// Test hook (a modified app): a fresh identity writes `OpenJoin{name}` claiming `seen`; the
    /// record goes into our meta doc as if it had arrived, and on to our sessions. Returns the
    /// throwaway DID.
    #[cfg(any(test, feature = "testkit"))]
    pub fn forge_open_join_for_test(&self, gid: &str, name: String, seen: Vec<(u64, i32)>) -> Result<(String, Option<Changed>), Error> {
        let x = identity::generate().0;
        let rec = Record { group: gid.to_string(), author: x.did().to_string(), dev: [0x5a; 32], at: now_ms() as i64, seen, body: Body::OpenJoin { name } };
        let update = {
            let groups = self.groups.lock();
            groups.get(gid).ok_or(Error::NotFound)?.doc.foreign_update_for_test(&x, &rec)
        };
        let changed = self.apply(gid, &DocId::Meta, &update, |_, _, _| Ok(())).map_err(|e| Error::Protocol(e.to_string()))?;
        Ok((rec.author, changed))
    }

    pub(super) fn write_meta(
        &self,
        live: &mut Live,
        gid: &str,
        rec: Record,
    ) -> Result<(Entry, Changed), Error> {
        let previous_doc = live.doc.checkpoint();
        let previous_meta = live.meta.clone();
        let previous_entries = live.entries.len();
        let (entry, update) = live
            .doc
            .write(&self.account, &rec)
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let before: HashSet<String> = live.state.items.iter().map(|i| i.id.clone()).collect();
        live.entries.push(entry.clone());
        live.rederive(gid);
        let doc = &live.doc;
        if let Err(e) = self
            .store
            .append_meta(gid, &mut live.meta, &update, || doc.snapshot())
        {
            live.restore_meta(gid, previous_doc, previous_meta, previous_entries);
            return Err(e.into());
        }
        let new_items = live
            .state
            .items
            .iter()
            .filter(|i| !before.contains(&i.id))
            .cloned()
            .collect();
        Ok((
            entry,
            Changed {
                gid: gid.to_string(),
                doc: DocId::Meta,
                doc_vv: live.doc.vv().encode(),
                new_items,
                posts: Vec::new(),
                history: false,
            },
        ))
    }

    pub(super) fn write_post(
        &self,
        live: &mut Live,
        gid: &str,
        rec: Record,
    ) -> Result<(Entry, Changed), Error> {
        let previous_meta = live.meta.clone();
        let day = day_of(rec.at);
        let Live {
            state,
            shards,
            meta,
            ..
        } = live;
        let shard = shards.get(&self.store, gid, self.wid, &day, state)?;
        let prev = shard.doc.vv().encode();
        let (entry, update) = shard
            .doc
            .write(&self.account, &rec)
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let vis = post_visible(state, &entry.rec.author, &entry.rec.seen);
        shard.insert(entry.clone(), vis);
        let shown: Vec<Item> = post_item(&entry, state)
            .filter(|_| vis == Vis::Visible)
            .into_iter()
            .collect();
        note_posts(meta, &self.did, &shown);
        meta.owed_days.insert(day.clone());
        // Our failed posts of this day ride along now (an upload carries every earlier op of
        // ours): they are no longer failed (`admit::fail_refused_posts`).
        meta.failed_posts.retain(|_, f| f.day != day);
        if vis == Vis::Pending {
            meta.pending_days.insert(day.clone());
        }
        if meta.newest_day.as_ref().is_none_or(|d| *d < day) {
            meta.newest_day = Some(day.clone());
        }
        let vv = shard.doc.vv().encode();
        let doc = &shard.doc;
        if let Err(e) = self.store.append_shard(
            gid,
            &day,
            &mut shard.meta,
            vv.clone(),
            &update,
            || doc.snapshot(),
            Some(meta),
        ) {
            *meta = previous_meta;
            shards.remove(&day);
            return Err(e.into());
        }
        let posts = vec![PostDiff {
            day: day.clone(),
            put: shown.clone(),
            prev,
            vv: vv.clone(),
        }];
        Ok((
            entry,
            Changed {
                gid: gid.to_string(),
                doc: DocId::Day(day),
                doc_vv: vv,
                new_items: shown,
                posts,
                history: false,
            },
        ))
    }

    /// Validates and applies a remote update of one doc; `admit` is the session's policy, asked
    /// with the group's current state and meta entries and the batch's new entries before
    /// anything is imported.
    pub fn apply(
        &self,
        gid: &str,
        doc: &DocId,
        update: &[u8],
        admit: impl FnOnce(&GroupState, &[Entry], &[Entry]) -> Result<(), String>,
    ) -> Result<Option<Changed>, Reject> {
        self.apply_ingress(gid, doc, update, false, admit)
    }

    pub fn apply_own_contents(
        &self,
        gid: &str,
        doc: &DocId,
        update: &[u8],
    ) -> Result<Option<Changed>, Reject> {
        self.apply_ingress(gid, doc, update, true, |_, _, _| Ok(()))
    }

    pub(super) fn apply_ingress(
        &self,
        gid: &str,
        doc: &DocId,
        update: &[u8],
        own_forward: bool,
        admit: impl FnOnce(&GroupState, &[Entry], &[Entry]) -> Result<(), String>,
    ) -> Result<Option<Changed>, Reject> {
        if !doc.is_valid() {
            return Err(Reject::WrongDoc);
        }
        let mut groups = self.groups.lock();
        let Some(live) = groups.get_mut(gid) else {
            return Err(Reject::Policy("unknown group".into()));
        };
        match doc {
            DocId::Meta => self.apply_meta(live, gid, update, own_forward, admit),
            DocId::Day(day) => self.apply_day(live, gid, day, update, own_forward, admit),
        }
    }

    pub(super) fn apply_meta(
        &self,
        live: &mut Live,
        gid: &str,
        update: &[u8],
        own_forward: bool,
        admit: impl FnOnce(&GroupState, &[Entry], &[Entry]) -> Result<(), String>,
    ) -> Result<Option<Changed>, Reject> {
        let previous_doc = live.doc.checkpoint();
        let previous_meta = live.meta.clone();
        let previous_entries = live.entries.len();
        let fresh = {
            let Live {
                doc,
                entries,
                state,
                ..
            } = &mut *live;
            doc.apply_remote(update, |fresh| admit(state, entries, fresh))?
        };
        // A known record ID can still arrive in a newly accepted op range.
        // Persist that VV before acknowledging it, even without a new item.
        if fresh.is_empty() && live.doc.vv() == previous_doc.vv() {
            return Ok(None);
        }
        let before: HashSet<String> = live.state.items.iter().map(|i| i.id.clone()).collect();
        live.entries.extend(fresh);
        live.rederive(gid);
        // Posts that were waiting for meta records may show now.
        let (posts, mut new_items) = self.rejudge_pending(live, gid);
        let receipt_ops = self.rejudged_ops(live, gid, &posts);
        if own_forward {
            // Borrowed writers don't appear under our core.wid. Persist the
            // validated frontier with the ops before issuing a cache receipt.
            live.meta.own_owed_meta = live.doc.vv().encode();
        }
        let doc = &live.doc;
        if let Err(e) = self.store.append_meta_with(
            gid,
            &mut live.meta,
            update,
            || doc.snapshot(),
            receipt_ops,
        ) {
            live.restore_meta(gid, previous_doc, previous_meta, previous_entries);
            return Err(Reject::Persistence(e.to_string()));
        }
        new_items.extend(
            live.state
                .items
                .iter()
                .filter(|i| !before.contains(&i.id))
                .cloned(),
        );
        Ok(Some(Changed {
            gid: gid.to_string(),
            doc: DocId::Meta,
            doc_vv: live.doc.vv().encode(),
            new_items,
            posts,
            history: false,
        }))
    }

    /// Judges again the posts that named meta records we lacked, in every day that has some.
    pub(super) fn rejudge_pending(&self, live: &mut Live, gid: &str) -> (Vec<PostDiff>, Vec<Item>) {
        let (mut diffs, mut shown) = (Vec::new(), Vec::new());
        let Live {
            state,
            shards,
            meta,
            ..
        } = live;
        for day in meta.pending_days.clone() {
            let shard = match shards.get(&self.store, gid, self.wid, &day, state) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("group {} day {day}: {e}", crate::short(gid));
                    continue;
                }
            };
            let now_shown = shard.rejudge(state);
            if !shard.has_pending() {
                meta.pending_days.remove(&day);
            }
            if !now_shown.is_empty() {
                let vv = shard.doc.vv().encode();
                diffs.push(PostDiff {
                    day,
                    put: now_shown.clone(),
                    prev: vv.clone(),
                    vv,
                });
                shown.extend(now_shown);
            }
        }
        note_posts(meta, &self.did, &shown);
        (diffs, shown)
    }

    pub(super) fn apply_day(
        &self,
        live: &mut Live,
        gid: &str,
        day: &str,
        update: &[u8],
        own_forward: bool,
        admit: impl FnOnce(&GroupState, &[Entry], &[Entry]) -> Result<(), String>,
    ) -> Result<Option<Changed>, Reject> {
        let previous_meta = live.meta.clone();
        let Live {
            doc: meta_doc,
            entries,
            state,
            shards,
            meta,
            ..
        } = live;
        let shard = shards
            .get(&self.store, gid, self.wid, day, state)
            .map_err(|e| Reject::Policy(e.to_string()))?;
        let prev_vv = shard.doc.vv();
        let prev = prev_vv.encode();
        let fresh = match shard
            .doc
            .apply_remote_inplace(update, |fresh| admit(state, entries, fresh))
        {
            Ok(f) => f,
            Err(e) => {
                // A refused batch may have left ops in the doc: forget it, the store has the rest.
                if shard.doc.tainted() {
                    shards.remove(day);
                }
                return Err(e);
            }
        };
        let fresh: Vec<Entry> = fresh.into_iter().filter(|e| !shard.has(e)).collect();
        if shard.doc.vv() == prev_vv {
            return Ok(None);
        }
        if own_forward && fresh.iter().any(|e| e.rec.author == self.did) {
            // An own-device phone write keeps its original writer, not ours.
            // Persist the upstream obligation atomically with its day ops/VV.
            meta.owed_days.insert(day.to_string());
        }
        let mut shown = Vec::new();
        let fresh_ids: Vec<String> = fresh.iter().map(|e| e.id.clone()).collect();
        for e in fresh {
            let vis = post_visible(state, &e.rec.author, &e.rec.seen);
            if vis == Vis::Pending {
                meta.pending_days.insert(day.to_string());
            }
            if vis == Vis::Visible {
                shown.extend(post_item(&e, state));
            }
            shard.insert(e, vis);
        }
        note_posts(meta, &self.did, &shown);
        if meta.newest_day.as_deref().is_none_or(|d| d < day) {
            meta.newest_day = Some(day.to_string());
        }
        // Delivered receipts: posts this device accepted from the group. A batch one of our
        // own devices forwarded adds none: that device accepted it and issues (as in 1:1).
        let receipt_ops = if own_forward {
            Vec::new()
        } else {
            let posts = Self::fresh_posts(shard, &fresh_ids);
            self.delivered_ops(state, &meta_doc.seen(), gid, day, &posts)
        };
        let vv = shard.doc.vv().encode();
        let doc = &shard.doc;
        if let Err(e) = self.store.append_shard_with(
            gid,
            day,
            &mut shard.meta,
            vv.clone(),
            update,
            || doc.snapshot(),
            Some(meta),
            receipt_ops,
        ) {
            *meta = previous_meta;
            // Never acknowledge or retain a day VV that exists only in memory.
            // The next access reloads the last accepted rows from the store.
            shards.remove(day);
            return Err(Reject::Persistence(e.to_string()));
        }
        let posts = vec![PostDiff {
            day: day.to_string(),
            put: shown.clone(),
            prev,
            vv: vv.clone(),
        }];
        Ok(Some(Changed {
            gid: gid.to_string(),
            doc: DocId::Day(day.to_string()),
            doc_vv: vv,
            new_items: shown,
            posts,
            history: false,
        }))
    }
}
