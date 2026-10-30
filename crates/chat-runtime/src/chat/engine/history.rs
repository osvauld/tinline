//! Day inventories and backfill, compaction and contact removal.

use super::*;

impl ChatRt {
    // ---- history & compaction ------------------------------------------------------------------------

    /// Sends the contact device our inventory of days now, 3 s later and then every 5 minutes
    /// while the session lasts (it may have learnt more from our other devices since). Each one
    /// asks the contact device to send us the days we lack (`chat_days_in`). Ends with the
    /// session (docs/chat.md section 10).
    pub(super) async fn chat_days_sync(self: Arc<Self>, core: Arc<ChatCore>, sess: Arc<Session>) {
        for pass in 0u32.. {
            if self.chat_send_inventory(&core, &sess).is_err() {
                return;
            }
            let wait = if pass == 0 { Duration::from_secs(3) } else { Duration::from_secs(300) };
            tokio::time::sleep(wait).await;
        }
    }

    /// Our inventory, oldest day first, in pages, ended by an empty page.
    pub(super) fn chat_send_inventory(&self, core: &ChatCore, sess: &Session) -> Result<(), ()> {
        let me = self.me().map_err(|_| ())?;
        if me.epoch != sess.epoch {
            return Err(());
        }
        let pair = pair_id(me.id.did(), &sess.did);
        // `shard_days` is newest first.
        let days = core.store.shard_days(&pair).or_log_default("reading the chat days");
        let mut after = None;
        for page in days.iter().rev().collect::<Vec<_>>().chunks(DAYS_PAGE) {
            let page: Vec<(String, Vec<u8>)> = page.iter().filter_map(|d| Some(((*d).clone(), core.store.shard_meta(&pair, d).ok().flatten()?.vv))).collect();
            let next = page.last().map(|(d, _)| d.clone());
            sess.tx.send(Cmd::Days(after.take(), page)).map_err(|_| ())?;
            after = next;
        }
        sess.tx.send(Cmd::Days(after, Vec::new())).map_err(|_| ())
    }

    /// Starts a backfill pass for this session, or has the running one go again when it ends.
    fn chat_backfill(self: &Arc<Self>, core: &Arc<ChatCore>, sess: &Arc<Session>) {
        if sess.backfill_busy.swap(true, Ordering::SeqCst) {
            sess.backfill_again.store(true, Ordering::SeqCst);
            return;
        }
        let (this, core, sess) = (self.clone(), core.clone(), sess.clone());
        self.handle.spawn(async move {
            loop {
                sess.backfill_again.store(false, Ordering::SeqCst);
                if this.chat_backfill_pass(&core, &sess).await.is_err() || !sess.backfill_again.load(Ordering::SeqCst) {
                    break;
                }
            }
            sess.backfill_busy.store(false, Ordering::SeqCst);
        });
    }

    /// Every day the contact device lacks or is behind on, newest first, one at a time with a
    /// pause between, held while the platform pauses backfill.
    async fn chat_backfill_pass(self: &Arc<Self>, core: &Arc<ChatCore>, sess: &Arc<Session>) -> Result<(), ()> {
        let alive = || core.sessions.lock().get(&sess.device).is_some_and(|s| s.id == sess.id);
        let me = self.me().map_err(|_| ())?;
        let pair = pair_id(me.id.did(), &sess.did);
        for day in core.store.shard_days(&pair).or_log_default("reading the chat days") {
            while self.backfill_paused.load(Ordering::SeqCst) {
                if !alive() {
                    return Err(());
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            if !alive() {
                return Err(());
            }
            if !sess.vv_known.lock().contains(&day) {
                continue;
            }
            let theirs = sess.peer_vv.lock().get(&day).cloned().unwrap_or_default();
            let Ok(Some(meta)) = core.store.shard_meta(&pair, &day) else { continue };
            if !Self::chat_lacks(&meta, &theirs) {
                continue;
            }
            let (tx, rx) = oneshot::channel();
            sess.tx.send(Cmd::Backfill(day, tx)).map_err(|_| ())?;
            rx.await.map_err(|_| ())?;
            tokio::time::sleep(Duration::from_millis(BACKFILL_PAUSE_MS)).await;
        }
        Ok(())
    }

    /// The contact device's inventory, one page: what it holds of each day. Its last (empty)
    /// page also says it holds none of the days it did not list, and starts our backfill.
    pub(super) fn chat_days_in(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Me, sess: &Arc<Session>, days: Vec<(String, Vec<u8>)>) -> Result<(), Error> {
        if days.len() > DAYS_PAGE {
            return Err(Error::Protocol("too many days in a page".into()));
        }
        let pair = pair_id(me.id.did(), &sess.did);
        if days.is_empty() {
            let seen = std::mem::take(&mut *sess.inv_seen.lock());
            for d in core.store.shard_days(&pair)? {
                if !seen.contains(&d) {
                    sess.peer_vv.lock().insert(d.clone(), VersionVector::default());
                    sess.vv_known.lock().insert(d);
                }
            }
            self.chat_backfill(core, sess);
            return Ok(());
        }
        let tomorrow = day_of(self.chat_wall_now() + DAY_MS);
        for (d, vv) in days {
            if !day_valid(&d) || d > tomorrow {
                return Err(Error::Protocol("bad day in inventory".into()));
            }
            let vv = decode_vv(&vv)?;
            // Doubles as an acknowledgement of what it holds.
            self.chat_ack(core, me, &sess.did, &pair, &d, &vv)?;
            sess.peer_vv.lock().insert(d.clone(), vv);
            sess.vv_known.lock().insert(d.clone());
            sess.inv_seen.lock().insert(d);
        }
        Ok(())
    }

    /// "Load earlier messages" with nothing older on this device: makes sure a link to the
    /// contact is up (the contact then sends every day we lack, newest first) and waits a while
    /// for days older than `before` to arrive; returns how many did.
    pub async fn chat_fetch_history(self: &Arc<Self>, did: &str, before: String) -> Result<u32, Error> {
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
            if node_runtime::block::blocked_me(&s.state, did) {
                return Err(Error::Rejected(node_runtime::BLOCKED.into()));
            }
            if s.state.blocked.contains(did) || !s.state.contacts.iter().any(|c| c.did == did) {
                return Err(Error::Protocol("not a contact you can ask".into()));
            }
        }
        if self.endpoint().is_err() {
            return Err(Error::NotStarted);
        }
        let pair = pair_id(me.id.did(), did);
        let older = |core: &ChatCore| -> Result<u32, Error> { Ok(core.store.shard_days(&pair)?.iter().filter(|d| d.as_str() < before.as_str()).count() as u32) };
        let had = older(&core)?;
        core.mark_active(did, None);
        let sessions = self.chat_sessions_of(&core, did);
        if sessions.is_empty() {
            self.chat_dial(&core, &me, did).await?;
        }
        // A link that is up: our inventory asks the contact to send what we lack now.
        for s in sessions {
            let _ = self.chat_send_inventory(&core, &s);
        }
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let now = older(&core)?;
            if now > had {
                return Ok(now - had);
            }
        }
        Ok(0)
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
    pub async fn chat_compact_for_test(self: &Arc<Self>) -> Result<u32, Error> {
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
    pub fn chat_close_sessions(&self, did: &str) {
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
    pub fn chat_pending(&self, did: &str, id: &str) -> bool {
        let (Ok(me), Ok(core)) = (self.me(), self.chat_core()) else { return false };
        let pair = pair_id(me.id.did(), did);
        match core.store.mi(&pair, id).or_log_flat("reading the message index") {
            Some(day) => core.store.mo(&pair, &day, id).or_log_flat("reading the delivery counter").is_some() && Self::chat_delivery(&core, &pair, &day, id) == DeliveryState::Pending,
            None => false,
        }
    }
}
