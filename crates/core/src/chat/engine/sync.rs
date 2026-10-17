//! Hello, sync handling, shard cache/persist, receiving and sending sync traffic.

use super::*;

impl Inner {
    // ---- hello / sync -----------------------------------------------------------------------

    /// This node's chat clock: the real one plus the test offset (`Node::chat_clock_offset_for_test`).
    pub(crate) fn chat_now(&self) -> i64 {
        now_ms() + self.chat_core().map(|c| c.clock_ms.load(Ordering::Relaxed)).unwrap_or(0)
    }

    /// The clock the live window and the catch-up decision use: the real one, unless the test
    /// made this device's clock wrong (`Node::chat_clock_skew_for_test`). A message stamped in the
    /// past by `chat_clock_offset_for_test` is judged as one written then by a right clock.
    pub(super) fn chat_wall_now(&self) -> i64 {
        match self.chat_core() {
            Ok(c) if c.clock_wrong.load(Ordering::Relaxed) => now_ms() + c.clock_ms.load(Ordering::Relaxed),
            _ => now_ms(),
        }
    }

    pub(super) fn recent_days(&self) -> Vec<String> {
        let t = self.chat_now();
        (0..HELLO_DAYS).map(|i| day_of(t - i * DAY_MS)).collect()
    }

    pub(super) fn chat_hello(&self, core: &ChatCore, sess: &Session) -> Result<ChatMsg, Error> {
        let me = self.me()?;
        let pair = pair_id(me.id.did(), &sess.did);
        let mut days: Vec<String> = self.recent_days();
        for d in core.store.pending_days(&pair)? {
            if !days.contains(&d) {
                days.push(d);
            }
        }
        let mut shards = Vec::new();
        for d in days {
            if let Some(m) = core.store.shard_meta(&pair, &d)? {
                shards.push((d, m.vv));
            }
            if shards.len() >= MAX_HELLO_SHARDS {
                break;
            }
        }
        let profile_vv = self.profile_doc(&sess.did)?.map(|p| p.vv()).unwrap_or_default();
        shards.push((crate::sync::profiledoc::PROFILE_DOC.into(), profile_vv.encode()));
        Ok(ChatMsg::Hello { shards })
    }

    pub(crate) fn chat_profile_changed(self: &Arc<Self>, did: &str) {
        if let Ok(core) = self.chat_core() {
            for s in self.chat_sessions_of(&core, did) { let _ = s.tx.send(Cmd::Profile); }
        }
        self.chat_kick(did.into());
    }

    pub(super) async fn chat_profile_send(self: &Arc<Self>, sess: &Session, writer: &mut ChatWriter) -> Result<(), Error> {
        let me = self.me()?;
        if me.epoch != sess.epoch || !self.chat_may_dial(&sess.did) { return Err(Error::Rejected("profile not allowed".into())); }
        let p = self.shared.lock().profile.clone().ok_or(Error::Locked)?;
        let since = sess.profile_vv.lock().clone();
        if let Some((update, vv)) = p.export_since(&since) {
            if update.len() > crate::profile::model::MAX_SNAPSHOT { return Err(Error::Protocol("profile update bounds".into())); }
            let doc = crate::sync::profiledoc::PROFILE_DOC;
            let sig = sign_batch(&me.profile.device_secret, doc, &update);
            writer.send(&ChatMsg::Sync { doc: doc.into(), vv: vv.encode(), update, sig }).await?;
        }
        Ok(())
    }

    pub(super) async fn chat_handle(self: &Arc<Self>, core: &Arc<ChatCore>, sess: &Arc<Session>, msg: ChatMsg, writer: &mut ChatWriter) -> Result<(), Error> {
        let me = self.me()?;
        if me.epoch != sess.epoch { return Err(Error::Locked); }
        let pair = pair_id(me.id.did(), &sess.did);
        match msg {
            ChatMsg::Auth { .. } => Err(Error::Protocol("unexpected Auth".into())),
            ChatMsg::Hello { shards } => {
                if shards.len() > MAX_HELLO_SHARDS + 1 {
                    return Err(Error::Protocol("too many shards".into()));
                }
                let mut theirs: HashMap<String, VersionVector> = HashMap::new();
                for (d, vv) in shards {
                    if d == crate::sync::profiledoc::PROFILE_DOC {
                        let held = decode_vv(&vv)?;
                        // Holding all of our profile already is an ack (like `chat_ack` for the
                        // days below): the Ack of an earlier session can be lost when that
                        // session closes first, and nothing new is sent to be acked now, so the
                        // owed profile would redial the contact forever.
                        if self.profile_pending(&sess.did) {
                            self.profile_ack(sess.epoch, &sess.did, &held)?;
                        }
                        *sess.profile_vv.lock() = held;
                        self.chat_profile_send(sess, writer).await?;
                        continue;
                    }
                    if !day_valid(&d) {
                        return Err(Error::Protocol("bad day".into()));
                    }
                    theirs.insert(d, decode_vv(&vv)?);
                }
                let mut days: Vec<String> = self.recent_days();
                for d in core.store.pending_days(&pair)? {
                    if !days.contains(&d) {
                        days.push(d);
                    }
                }
                for d in theirs.keys() {
                    if !days.contains(d) {
                        days.push(d.clone());
                    }
                }
                for day in days {
                    let their_vv = theirs.get(&day).cloned().unwrap_or_default();
                    sess.peer_vv.lock().insert(day.clone(), their_vv.clone());
                    self.chat_ack(core, &me, &sess.did, &pair, &day, &their_vv)?;
                    self.chat_send_own(core, &me, sess, &pair, &day, &their_vv, writer, true).await?;
                }
                Ok(())
            }
            ChatMsg::Sync { doc, vv, update, sig } => {
                if doc == crate::sync::profiledoc::PROFILE_DOC {
                    if !verify_batch(&sess.device, &doc, &update, &sig) { return Err(Error::Protocol("bad profile batch signature".into())); }
                    let vv = self.profile_receive(sess.epoch, &sess.did, &update)?;
                    return writer.send(&ChatMsg::Ack { doc, vv: vv.encode() }).await;
                }
                let their = decode_vv(&vv)?;
                let day = self.chat_doc_day(&me, sess, &doc)?;
                self.chat_receive(core, &me, sess, &doc, &update, &sig, false, writer).await?;
                sess.peer_vv.lock().insert(day, their);
                Ok(())
            }
            ChatMsg::Push { doc, update, sig } => self.chat_receive(core, &me, sess, &doc, &update, &sig, true, writer).await,
            ChatMsg::Ack { doc, vv } => {
                if doc == crate::sync::profiledoc::PROFILE_DOC {
                    let vv = decode_vv(&vv)?;
                    sess.profile_vv.lock().merge(&vv);
                    return self.profile_ack(sess.epoch, &sess.did, &vv);
                }
                let day = self.chat_doc_day(&me, sess, &doc)?;
                let vv = decode_vv(&vv)?;
                sess.peer_vv.lock().entry(day.clone()).and_modify(|v| merge_vv(v, &vv)).or_insert_with(|| vv.clone());
                self.chat_ack(core, &me, &sess.did, &pair, &day, &vv)
            }
            ChatMsg::HistoryReq { before, limit } => {
                if !day_valid(&before) {
                    return Err(Error::Protocol("bad day".into()));
                }
                let days = self.chat_history_days(core, &pair, &before, limit.min(14)).await?;
                writer.send(&ChatMsg::HistoryResp { days }).await
            }
            ChatMsg::HistoryResp { days } => {
                if let Some(tx) = sess.hist.lock().take() {
                    let _ = tx.send(days);
                }
                Ok(())
            }
            ChatMsg::Notice { .. } => Err(Error::Protocol("unexpected notice".into())),
            ChatMsg::ReceiptHello { days } => self.chat_receipt_hello_in(core, sess, days, writer).await,
            ChatMsg::Receipts { day, update } => self.chat_receipts_in(core, sess, &day, &update, writer).await,
            ChatMsg::ReceiptAck { day, vv } => self.chat_receipt_ack_in(core, sess, &day, &vv, writer).await,
        }
    }

    /// `dm/{pair}/{day}` -> day, checking it names this conversation.
    pub(super) fn chat_doc_day(&self, me: &Me, sess: &Session, doc: &str) -> Result<String, Error> {
        doc_day_of(&pair_id(me.id.did(), &sess.did), doc)
    }

    /// Loads the shard into the cache (creating an empty one) and returns the cache, locked, with
    /// the shard in it: another task's load can evict it the moment the lock is let go.
    /// A cached shard that holds other ops than the store (a write whose `chat_persist` failed
    /// after the doc took it) is dropped and loaded again: its ops were never stored, so they
    /// must never be sent, and our next write must not build on them (P10).
    pub(crate) async fn chat_shard<'c>(self: &Arc<Self>, core: &'c Arc<ChatCore>, pair: &str, day: &str) -> Result<ShardCache<'c>, Error> {
        let key = (pair.to_string(), day.to_string());
        {
            let mut cache = core.shards.lock();
            if let Some(s) = cache.get(&key) {
                let stored = core.store.shard_meta(pair, day)?.and_then(|m| decode_vv(&m.vv).ok()).unwrap_or_default();
                if s.vv() == stored {
                    return Ok(cache);
                }
                tracing::warn!("chat: cached {day} differs from the store; reloading it");
                cache.remove(&key);
            }
        }
        let shard = self.chat_load_shard(core, pair, day).await?;
        let mut cache = core.shards.lock();
        if cache.len() >= MAX_SHARDS_CACHED && !cache.contains_key(&key) {
            let today = day_of(self.chat_now());
            cache.retain(|(_, d), _| *d == today);
        }
        cache.entry(key).or_insert(shard);
        Ok(cache)
    }

    /// A shard as stored (a closed one from its snapshot blob), not cached.
    pub(crate) async fn chat_load_shard(self: &Arc<Self>, core: &Arc<ChatCore>, pair: &str, day: &str) -> Result<Shard, Error> {
        let meta = core.store.shard_meta(pair, day)?;
        let (snap, ups) = core.store.shard_rows(pair, day)?;
        let base = match (&snap, meta.as_ref().and_then(|m| m.closed.clone())) {
            (None, Some(c)) => {
                let hub = self.chat_hub().await?;
                let hash = parse_hash(&c.hash)?;
                let k = parse_key(&c.key)?;
                let ct = hub.read_cipher(&hash).await?;
                let plain = tokio::task::spawn_blocking(move || crypt::decrypt_bytes(&k, &ct))
                    .await
                    .map_err(|e| Error::Io(e.to_string()))?
                    .map_err(|e| Error::Io(e.to_string()))?;
                Some(plain)
            }
            _ => snap,
        };
        Shard::load(pair, day, core.wid, base.as_deref(), &ups).map_err(|e| Error::Io(e.to_string()))
    }

    /// Persists a shard change: the update (or a fresh snapshot every so often), the meta, and
    /// `extra` ops, in one transaction. Call with the shard cache locked. On failure the cached
    /// shard is ahead of the store; `chat_shard` reloads it before anyone uses it again.
    /// `recs`: the messages the change touched, for the search index (written after the
    /// commit; a failure there is only logged, reconcile heals it).
    pub(crate) fn chat_persist(&self, core: &ChatCore, pair: &str, shard: &Shard, update: &[u8], mut extra: Vec<Op>, recs: &[&MsgRec]) -> Result<(), Error> {
        let me = self.me()?;
        let Some(did) = self.pair_owner(&me, pair) else {
            return Err(Error::NotFound);
        };
        let day = shard.day.as_str();
        let mut meta = core.store.shard_meta(pair, day)?.unwrap_or_default();
        let prev = meta.vv.clone();
        meta.vv = shard.vv().encode();
        meta.own = shard.own_writers(me.id.did()).into_iter().collect();
        meta.n_messages = shard.messages().map(|m| m.len() as u32).unwrap_or(meta.n_messages);
        if meta.n_updates + 1 > UPDATES_BEFORE_SNAPSHOT {
            extra.extend(core.store.put_snap_ops(pair, day, &shard.snapshot())?);
            meta.n_updates = 0;
        } else {
            extra.push(core.store.put_update_op(pair, day, meta.next_seq, update)?);
            meta.next_seq += 1;
            meta.n_updates += 1;
        }
        extra.push(core.store.put_meta_op(pair, day, &meta)?);
        if core.fail_writes.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok() {
            return Err(Error::Io("test: chat write failed".into()));
        }
        core.store.db.apply(&extra)?;
        self.selfsync_dirty(&doc_name(pair, day));
        self.search_dm(&did, pair, day, recs, Some(prev), meta.vv);
        Ok(())
    }

    /// Registers the file of a message (a new `blob/` record, the read permission) and returns
    /// the ops, plus the hash if it should be downloaded now.
    pub(crate) fn chat_register_file(&self, core: &ChatCore, pair: &str, day: &str, rec: &MsgRec, incoming: bool) -> Result<(Vec<Op>, Option<String>), Error> {
        let Some(f) = &rec.file else { return Ok((vec![], None)) };
        // Validate before it is trusted as a path component or a key.
        parse_hash(&f.hash)?;
        parse_key(&f.key)?;
        let mut ops = core.store.ref_ops(&f.hash, pair)?;
        if core.store.blob(&f.hash)?.is_some() {
            return Ok((ops, None));
        }
        let wanted = !incoming || (f.size <= core.store.effective_auto_download() && f.size <= crypt::MAX_FILE);
        let info = BlobInfo {
            key: f.key.clone(),
            name: f.name.clone(),
            size: f.size,
            mime: f.mime.clone(),
            kind: f.kind.clone(),
            pair: pair.to_string(),
            ready: !incoming,
            wanted,
            failed: false,
            day: day.to_string(),
            msg: rec.id.clone(),
        };
        ops.push(core.store.put_blob_op(&f.hash, &info)?);
        Ok((ops, (incoming && wanted).then(|| f.hash.clone())))
    }

    /// Conversation row after `rec` was added or changed. A newly added incoming message counts
    /// as unread unless it is at or before the read cursor (`ConvMeta::read_upto`).
    pub(crate) fn chat_conv_ops(&self, core: &ChatCore, me: &Me, did: &str, pair: &str, recs: &[(&MsgRec, bool)]) -> Result<(Vec<Op>, ConvMeta), Error> {
        let mut conv = core.store.conv(pair)?.unwrap_or_else(|| ConvMeta { peer_did: did.to_string(), ..Default::default() });
        conv.peer_did = did.to_string();
        for (rec, added) in recs {
            let incoming = rec.author != me.id.did();
            if incoming {
                conv.last_in = conv.last_in.max(rec.at);
                if *added && !rec.deleted && rec.at > conv.read_upto {
                    conv.unread = conv.unread.saturating_add(1);
                }
            }
            let key = (rec.at, rec.id.as_str());
            let newer = conv.last.as_ref().is_none_or(|l| (l.at, l.id.as_str()) <= key);
            if newer || conv.last.as_ref().is_some_and(|l| l.id == rec.id) {
                conv.last = Some(LastMsg { id: rec.id.clone(), at: rec.at, preview: preview(rec), outgoing: !incoming });
            }
        }
        Ok((vec![core.store.conv_op(pair, &conv)?], conv))
    }

    // ---- receiving ---------------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn chat_receive(
        self: &Arc<Self>,
        core: &Arc<ChatCore>,
        me: &Arc<Me>,
        sess: &Arc<Session>,
        doc: &str,
        update: &[u8],
        sig: &[u8],
        live: bool,
        writer: &mut ChatWriter,
    ) -> Result<(), Error> {
        let day = self.chat_doc_day(me, sess, doc)?;
        if !verify_batch(&sess.device, doc, update, sig) {
            return Err(Error::Protocol(Reject::BadSignature.to_string()));
        }
        let pair = pair_id(me.id.did(), &sess.did);
        let held = self.chat_shard(core, &pair, &day).await?;
        let mut added_files = Vec::new();
        let mut unchanged: Option<VersionVector> = None;
        let mut need_history = false;
        let (vv_now, notify) = 'blk: {
            let mut shards = held;
            let shard = shards.get_mut(&(pair.clone(), day.clone())).ok_or(Error::NotFound)?;
            let before = shard.vv();
            let ctx = Ctx { signer_did: sess.did.clone(), now_ms: self.chat_wall_now(), live, history: None };
            let applied = match shard.apply_remote(update, &ctx) {
                Ok(a) => a,
                // Their ops build on ops of ours that we lost (a reinstall): the batch cannot
                // be judged on its own. Ask them for the day as a vouched snapshot instead.
                Err(Reject::Pending) => {
                    need_history = true;
                    unchanged = Some(shard.vv());
                    break 'blk (shard.vv(), (Vec::new(), ConvMeta::default()));
                }
                Err(r) => {
                    self.log(format!("rejected chat batch from {}: {r}", sess.did));
                    return Err(Error::Protocol(format!("rejected batch: {r}")));
                }
            };
            if shard.vv() == before {
                unchanged = Some(shard.vv());
                (shard.vv(), (Vec::new(), ConvMeta::default()))
            } else {
            let (owned, conv, files) = self.chat_commit(core, me, &sess.did, &pair, &day, shard, update, &applied, false)?;
            added_files = files;
            (shard.vv(), (owned, conv))
            }
        };
        if need_history {
            self.chat_pull_day(core, me, sess, &day);
            return Ok(());
        }
        if unchanged.is_some() {
            let _ = writer.send(&ChatMsg::Ack { doc: doc.to_string(), vv: vv_now.encode() }).await;
            return Ok(());
        }
        let (recs, conv) = notify;
        if let Some(ev) = self.ev() {
            for (rec, added) in &recs {
                let m = self.chat_api_message(core, me, &sess.did, &pair, &day, rec);
                if *added { ev.on_message_added(m) } else { ev.on_message_changed(m) }
            }
            if let Some(c) = self.chat_api_chat(core, me, &conv) {
                ev.on_chat_changed(c);
            }
        }
        let _ = writer.send(&ChatMsg::Ack { doc: doc.to_string(), vv: vv_now.encode() }).await;
        // The contact's other devices tick from our acks too: tell them we hold it now.
        for s in self.chat_sessions_of(core, &sess.did) {
            if s.id != sess.id {
                let _ = s.tx.send(Cmd::Ack(day.clone()));
            }
        }
        for h in added_files {
            self.chat_download(sess.did.clone(), h);
        }
        self.chat_receipts_pump();
        Ok(())
    }

    /// Persists an accepted batch (`applied` says what it changed) and returns what to announce:
    /// the touched messages (flag: newly added), the conversation row and the files to fetch.
    /// `all_remote`: the batch came from another of our own devices, so even messages we
    /// authored have no bytes here. Call with the shard cache locked.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    pub(crate) fn chat_commit(
        &self,
        core: &ChatCore,
        me: &Me,
        did: &str,
        pair: &str,
        day: &str,
        shard: &Shard,
        update: &[u8],
        applied: &Applied,
        all_remote: bool,
    ) -> Result<(Vec<(MsgRec, bool)>, ConvMeta, Vec<String>), Error> {
        let msgs = shard.messages().map_err(|e| Error::Io(e.to_string()))?;
        let touched: Vec<&MsgRec> = applied.added.iter().chain(applied.changed.iter()).filter_map(|id| msgs.get(id)).collect();
        let mut ops = Vec::new();
        let mut files = Vec::new();
        for rec in &touched {
            if applied.added.contains(&rec.id) {
                ops.push(core.store.mi_op(pair, &rec.id, day)?);
            }
            let (o, dl) = self.chat_register_file(core, pair, day, rec, all_remote || rec.author != me.id.did())?;
            ops.extend(o);
            if let Some(h) = dl {
                files.push(h);
            }
        }
        let flags: Vec<(&MsgRec, bool)> = touched.iter().map(|r| (*r, applied.added.contains(&r.id))).collect();
        let (cops, conv) = self.chat_conv_ops(core, me, did, pair, &flags)?;
        ops.extend(cops);
        // Recipient delivered receipt obligation, atomic with the acceptance. An own-device
        // relay is not this device's acceptance from the contact (that device issues).
        if !all_remote {
            ops.extend(Self::chat_delivered_intent(core, me, pair, day, &touched)?);
        }
        self.chat_persist(core, pair, shard, update, ops, &touched)?;
        let owned: Vec<(MsgRec, bool)> = flags.iter().map(|(r, a)| ((*r).clone(), *a)).collect();
        Ok((owned, conv, files))
    }

    /// The peer says it holds `their_vv` of a day: our messages up to there are delivered.
    pub(in crate::chat) fn chat_ack(&self, core: &ChatCore, me: &Me, did: &str, pair: &str, day: &str, their_vv: &VersionVector) -> Result<(), Error> {
        let Some(meta) = core.store.shard_meta(pair, day)? else { return Ok(()) };
        let mut acked = match core.store.ack(pair, day)? {
            Some(b) => decode_vv(&b).unwrap_or_default(),
            None => VersionVector::default(),
        };
        let before = acked.clone();
        merge_vv(&mut acked, their_vv);
        if acked == before {
            return Ok(());
        }
        let moved = |peer: u64, c: i32| before.get(&peer).copied().unwrap_or(0) < c && c <= acked.get(&peer).copied().unwrap_or(0);
        let owed = Self::chat_owes(core, &meta, &acked);
        let newly: Vec<String> = core.store.mos(pair, day)?.into_iter().filter(|(_, (p, c))| moved(*p, *c)).map(|(id, _)| id).collect();
        core.store.db.apply(&[core.store.put_ack_op(pair, day, &acked.encode())?, core.store.out_op(pair, day, owed)?])?;
        self.selfsync_dirty(ACK_MARK);
        if let Some(ev) = self.ev() {
            for id in &newly {
                ev.on_delivery_changed(did.to_string(), id.clone(), DeliveryState::Delivered);
            }
            if !newly.is_empty()
                && let Ok(Some(conv)) = core.store.conv(pair)
                && conv.last.as_ref().is_some_and(|l| newly.contains(&l.id))
                && let Some(c) = self.chat_api_chat(core, me, &conv)
            {
                ev.on_chat_changed(c);
            }
        }
        Ok(())
    }

    // ---- sending sync traffic ------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn chat_send_own(
        self: &Arc<Self>,
        core: &Arc<ChatCore>,
        me: &Arc<Me>,
        sess: &Arc<Session>,
        pair: &str,
        day: &str,
        their_vv: &VersionVector,
        writer: &mut ChatWriter,
        sync: bool,
    ) -> Result<(), Error> {
        let Some(meta) = core.store.shard_meta(pair, day)? else { return Ok(()) };
        let name = doc_name(pair, day);
        // Our ops are under any of our writer IDs: this install's, and those of our earlier or
        // other installs whose ops we hold (P10b).
        if !Self::chat_owes(core, &meta, their_vv) {
            return Ok(());
        }
        let out = {
            let shards = self.chat_shard(core, pair, day).await?;
            shards.get(&(pair.to_string(), day.to_string())).and_then(|s| {
                let vv = s.vv();
                let ends: Vec<(u64, i32)> = s.own_writers(me.id.did()).into_iter().filter_map(|p| vv.get(&p).map(|c| (p, *c))).collect();
                // A live push must not create a message the contact refuses for the 24 h live
                // window (an old one of ours not delivered yet, e.g. from another writer ID):
                // that batch goes as a catch-up `Sync` instead, which has no such window and
                // keeps the session up (P10b). One hour of margin for clock skew.
                let catch_up = !sync && s.creates_outside(their_vv, me.id.did(), self.chat_wall_now(), DAY_MS - HOUR_MS);
                s.export_own_since(their_vv, me.id.did()).map(|u| (u, vv, ends, catch_up))
            })
        };
        let Some((update, vv, ends, catch_up)) = out else { return Ok(()) };
        let sig = sign_batch(&me.profile.device_secret, &name, &update);
        let msg = if sync || catch_up {
            ChatMsg::Sync { doc: name, vv: vv.encode(), update, sig }
        } else {
            ChatMsg::Push { doc: name, update, sig }
        };
        writer.send(&msg).await?;
        let mut pv = sess.peer_vv.lock();
        let theirs = pv.entry(day.to_string()).or_default();
        for (p, c) in ends {
            if theirs.get(&p).is_none_or(|t| *t < c) {
                theirs.insert(p, c);
            }
        }
        Ok(())
    }

    /// Whether a contact holding `theirs` of a day lacks ops of ours: of any of our writer IDs
    /// in it (`ShardMeta::own`, plus this install's), not only this install's (P10b).
    pub(crate) fn chat_owes(core: &ChatCore, meta: &ShardMeta, theirs: &VersionVector) -> bool {
        let held = decode_vv(&meta.vv).unwrap_or_default();
        meta.own.iter().chain(std::iter::once(&core.wid)).any(|p| theirs.get(p).copied().unwrap_or(0) < held.get(p).copied().unwrap_or(0))
    }

    pub(super) async fn chat_push(self: &Arc<Self>, core: &Arc<ChatCore>, sess: &Arc<Session>, day: &str, writer: &mut ChatWriter) -> Result<(), Error> {
        let me = self.me()?;
        let pair = pair_id(me.id.did(), &sess.did);
        let theirs = sess.peer_vv.lock().get(day).cloned().unwrap_or_default();
        self.chat_send_own(core, &me, sess, &pair, day, &theirs, writer, false).await
    }
}
