//! History fetch, compaction and contact removal.

use super::*;

impl Inner {
    // ---- history & compaction ------------------------------------------------------------------------

    pub(super) async fn chat_snapshot_blob(self: &Arc<Self>, core: &Arc<ChatCore>, pair: &str, day: &str) -> Result<HistDay, Error> {
        let meta = core.store.shard_meta(pair, day)?.ok_or(Error::NotFound)?;
        let hub = self.chat_hub().await?;
        if let Some(c) = &meta.closed
            && core.store.shard_rows(pair, day)?.1.is_empty()
            && core.store.shard_rows(pair, day)?.0.is_none()
        {
            let size = hub.complete_size(&parse_hash(&c.hash)?).await.unwrap_or(0);
            return Ok(HistDay { day: day.into(), vv: meta.vv, hash: c.hash.clone(), key: c.key.clone(), size });
        }
        if let Some(s) = core.store.snapcache(pair, day)?
            && s.vv == meta.vv
        {
            return Ok(HistDay { day: day.into(), vv: s.vv, hash: s.hash, key: s.key, size: s.size });
        }
        let snap = self.chat_shard(core, pair, day).await?.get(&(pair.to_string(), day.to_string())).map(|s| s.snapshot()).ok_or(Error::NotFound)?;
        let key = crypt::new_key();
        let hash = hub.add_bytes(key, snap).await?;
        let size = hub.complete_size(&hash).await.unwrap_or(0);
        let hex_hash = hash.to_string();
        let sref = SnapRef { vv: meta.vv.clone(), hash: hex_hash.clone(), key: hex::encode(key), size };
        let mut ops = core.store.ref_ops(&hex_hash, pair)?;
        ops.push(core.store.put_snapcache_op(pair, day, &sref)?);
        let old = core.store.snapcache(pair, day)?;
        core.store.db.apply(&ops)?;
        if let Some(o) = old
            && let Ok(h) = parse_hash(&o.hash)
        {
            let _ = hub.release(&h).await;
        }
        Ok(HistDay { day: day.into(), vv: meta.vv, hash: sref.hash, key: sref.key, size })
    }

    pub(super) async fn chat_history_days(self: &Arc<Self>, core: &Arc<ChatCore>, pair: &str, before: &str, limit: u32) -> Result<Vec<HistDay>, Error> {
        let mut out = Vec::new();
        for d in core.store.shard_days(pair)? {
            if d.as_str() >= before {
                continue;
            }
            if core.store.shard_meta(pair, &d)?.is_none_or(|m| m.n_messages == 0) {
                continue;
            }
            out.push(self.chat_snapshot_blob(core, pair, &d).await?);
            if out.len() as u32 >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Asks the peer for days older than `before` that we do not hold; returns how many arrived.
    pub(crate) async fn chat_fetch_history(self: &Arc<Self>, did: &str, before: String) -> Result<u32, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        self.chat_contact(did, false)?;
        if !day_valid(&before) {
            return Err(Error::Protocol("bad day".into()));
        }
        if self.is_self_did(did) {
            return Ok(0);
        }
        {
            let s = self.shared.lock();
            if crate::block::blocked_me(&s.state, did) {
                return Err(Error::Rejected(crate::BLOCKED.into()));
            }
            if s.state.blocked.contains(did) || !s.state.contacts.iter().any(|c| c.did == did) {
                return Err(Error::Protocol("not a contact you can ask".into()));
            }
        }
        let ep_ready = self.endpoint().is_ok();
        if !ep_ready {
            return Err(Error::NotStarted);
        }
        let mut sess = self.chat_session(&core, did);
        if sess.is_none() {
            self.chat_dial(&core, &me, did).await?;
            for _ in 0..50 {
                sess = self.chat_session(&core, did);
                if sess.is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        let sess = sess.ok_or(Error::Timeout)?;
        let (tx, rx) = oneshot::channel();
        *sess.hist.lock() = Some(tx);
        sess.tx.send(Cmd::History(before, 14)).map_err(|_| Error::Net("session closed".into()))?;
        let days = tokio::time::timeout(Duration::from_secs(30), rx).await.map_err(|_| Error::Timeout)?.map_err(|_| Error::Net("session closed".into()))?;
        let pair = pair_id(me.id.did(), did);
        let mut added = 0;
        for d in days.into_iter().take(14) {
            if !day_valid(&d.day) || core.store.shard_meta(&pair, &d.day)?.is_some() {
                continue;
            }
            if self.chat_import_day(&core, &me, &sess, &pair, d).await? {
                added += 1;
            }
        }
        Ok(added)
    }

    /// A peer-vouched snapshot of a day (see `Ctx::history`): validated like any batch (every op
    /// proven by its author's DID signature), merged into whatever we hold of that day, stored as
    /// the new snapshot. `shards` holds the day (`chat_shard`).
    pub(super) fn chat_apply_history(&self, mut shards: ShardCache<'_>, core: &ChatCore, me: &Me, sess: &Session, pair: &str, day: &str, snap: &[u8]) -> Result<bool, Error> {
        let shard = shards.get_mut(&(pair.to_string(), day.to_string())).ok_or(Error::NotFound)?;
        let before = shard.vv();
        let ctx = Ctx { signer_did: sess.did.clone(), now_ms: now_ms(), live: false, history: Some(me.id.did().to_string()) };
        if let Err(r) = shard.apply_remote(snap, &ctx) {
            self.log(format!("rejected history of {day} from {}: {r}", sess.did));
            return Err(Error::Protocol(format!("rejected history: {r}")));
        }
        if shard.vv() == before {
            return Ok(false);
        }
        let msgs = shard.messages().map_err(|e| Error::Io(e.to_string()))?;
        let mut ops = Vec::new();
        for rec in msgs.values() {
            ops.push(core.store.mi_op(pair, &rec.id, day)?);
            let (o, _) = self.chat_register_file(core, pair, day, rec, true)?;
            ops.extend(o);
        }
        let meta = ShardMeta {
            vv: shard.vv().encode(),
            next_seq: 0,
            n_updates: 0,
            n_messages: msgs.len() as u32,
            closed: None,
            own: shard.own_writers(me.id.did()).into_iter().collect(),
        };
        ops.extend(core.store.put_snap_ops(pair, day, &shard.snapshot())?);
        ops.push(core.store.put_meta_op(pair, day, &meta)?);
        // The last-message row may need to move.
        let recs: Vec<(&MsgRec, bool)> = msgs.values().map(|r| (r, false)).collect();
        let (cops, _) = self.chat_conv_ops(core, me, &sess.did, pair, &recs)?;
        ops.extend(cops);
        let all: Vec<&MsgRec> = msgs.values().collect();
        ops.extend(Self::chat_delivered_intent(core, me, pair, day, &all)?);
        let prev = core.store.shard_meta(pair, day)?.map(|m| m.vv).unwrap_or_default();
        core.store.db.apply(&ops)?;
        self.selfsync_dirty(&doc_name(pair, day));
        self.search_dm(&sess.did, pair, day, &all, Some(prev), meta.vv);
        Ok(true)
    }

    /// Pulls one day from the peer as a vouched snapshot (in the background: the session task
    /// must stay free to read the answer).
    pub(super) fn chat_pull_day(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Arc<Me>, sess: &Arc<Session>, day: &str) {
        let key = (pair_id(me.id.did(), &sess.did), day.to_string());
        if !core.pulling.lock().insert(key.clone()) {
            return;
        }
        let (this, core, me, sess) = (self.clone(), core.clone(), me.clone(), sess.clone());
        self.handle.spawn(async move {
            let res: Result<(), Error> = async {
                let upto = day_of(day_start(&key.1).ok_or(Error::NotFound)? + DAY_MS + HOUR_MS);
                let (tx, rx) = oneshot::channel();
                *sess.hist.lock() = Some(tx);
                sess.tx.send(Cmd::History(upto, 1)).map_err(|_| Error::Net("session closed".into()))?;
                let days = tokio::time::timeout(Duration::from_secs(30), rx).await.map_err(|_| Error::Timeout)?.map_err(|_| Error::Net("session closed".into()))?;
                let Some(d) = days.into_iter().find(|d| d.day == key.1) else { return Ok(()) };
                this.chat_import_day(&core, &me, &sess, &key.0, d).await?;
                let _ = sess.tx.send(Cmd::Ack(key.1.clone()));
                Ok(())
            }
            .await;
            if let Err(e) = res {
                this.log(format!("pulling {} from {}: {e}", key.1, sess.did));
            }
            core.pulling.lock().remove(&key);
        });
    }

    /// Fetches the snapshot blob `d` names from the peer and merges it (`chat_apply_history`).
    pub(super) async fn chat_import_day(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Arc<Me>, sess: &Arc<Session>, pair: &str, d: HistDay) -> Result<bool, Error> {
        if !day_valid(&d.day) || d.size == 0 || d.size > 64 * 1024 * 1024 {
            return Ok(false);
        }
        let (hash, key) = (parse_hash(&d.hash)?, parse_key(&d.key)?);
        let hub = self.chat_hub().await?;
        let conn = self.chat_blob_conn(core, &sess.did).await?;
        hub.fetch(conn, hash, d.size, |_, _| {}).await?;
        let ct = hub.read_cipher(&hash).await?;
        let snap = tokio::task::spawn_blocking(move || crypt::decrypt_bytes(&key, &ct))
            .await
            .map_err(|e| Error::Io(e.to_string()))?
            .map_err(|e| Error::Io(e.to_string()))?;
        let _ = hub.release(&hash).await;
        let shards = self.chat_shard(core, pair, &d.day).await?;
        let added = self.chat_apply_history(shards, core, me, sess, pair, &d.day, &snap)?;
        // After the shard lock is released: sign what that import made durable.
        self.chat_receipts_pump();
        Ok(added)
    }

    /// Days at least `CLOSE_AFTER_DAYS` old become one encrypted snapshot blob each.
    pub(super) async fn chat_compact(self: &Arc<Self>) -> Result<(), Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        let limit = day_of(self.chat_now() - CLOSE_AFTER_DAYS * DAY_MS);
        let mut dids: Vec<String> = self.shared.lock().state.contacts.iter().map(|c| c.did.clone()).collect();
        dids.push(me.id.did().to_string());
        for did in dids {
            let pair = pair_id(me.id.did(), &did);
            for day in core.store.shard_days(&pair)? {
                if day > limit {
                    continue;
                }
                let Some(meta) = core.store.shard_meta(&pair, &day)? else { continue };
                if meta.closed.is_some() && meta.n_updates == 0 && core.store.shard_rows(&pair, &day)?.0.is_none() {
                    continue;
                }
                if core.store.pending_days(&pair)?.contains(&day) {
                    continue;
                }
                let snap = self.chat_shard(&core, &pair, &day).await?.get(&(pair.clone(), day.clone())).map(|s| s.snapshot()).ok_or(Error::NotFound)?;
                let hub = self.chat_hub().await?;
                let key = crypt::new_key();
                let hash = hub.add_bytes(key, snap).await?;
                let hex_hash = hash.to_string();
                let mut ops = core.store.ref_ops(&hex_hash, &pair)?;
                ops.extend(core.store.drop_rows_ops(&pair, &day));
                let mut m = meta.clone();
                m.n_updates = 0;
                m.next_seq = 0;
                m.closed = Some(ClosedRef { hash: hex_hash, key: hex::encode(key) });
                ops.push(core.store.put_meta_op(&pair, &day, &m)?);
                core.store.db.apply(&ops)?;
                if let Some(old) = meta.closed
                    && let Ok(h) = parse_hash(&old.hash)
                {
                    let _ = hub.release(&h).await;
                }
                core.shards.lock().remove(&(pair.clone(), day.clone()));
            }
        }
        Ok(())
    }

    /// For tests: `chat_compact` now; how many days are closed (a snapshot blob, no rows).
    pub(crate) async fn chat_compact_for_test(self: &Arc<Self>) -> Result<u32, Error> {
        self.chat_compact().await?;
        let (me, core) = (self.me()?, self.chat_core()?);
        let mut dids: Vec<String> = self.shared.lock().state.contacts.iter().map(|c| c.did.clone()).collect();
        dids.push(me.id.did().to_string());
        let mut closed = 0;
        for did in dids {
            let pair = pair_id(me.id.did(), &did);
            for day in core.store.shard_days(&pair)? {
                if core.store.shard_meta(&pair, &day)?.is_some_and(|m| m.closed.is_some()) && core.store.shard_rows(&pair, &day)?.0.is_none() {
                    closed += 1;
                }
            }
        }
        Ok(closed)
    }

    // ---- contact removal ---------------------------------------------------------------------------------

    /// Ends every chat session with `did` (a block, either way).
    pub(crate) fn chat_close_sessions(&self, did: &str) {
        let Ok(core) = self.chat_core() else { return };
        core.sessions.lock().retain(|_, s| {
            let hit = s.did == did;
            if hit {
                let _ = s.tx.send(Cmd::Close);
            }
            !hit
        });
    }

    /// Whether `id` is a message of ours to `did` that has not reached them.
    pub(crate) fn chat_pending(&self, did: &str, id: &str) -> bool {
        let (Ok(me), Ok(core)) = (self.me(), self.chat_core()) else { return false };
        let pair = pair_id(me.id.did(), did);
        match core.store.mi(&pair, id).ok().flatten() {
            Some(day) => core.store.mo(&pair, &day, id).ok().flatten().is_some() && Self::chat_delivery(&core, &pair, &day, id) == DeliveryState::Pending,
            None => false,
        }
    }
}
