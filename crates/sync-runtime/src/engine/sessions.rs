//! Own-device authentication, session ownership, and inbound synchronization.

use super::*;

impl SyncRt {
    // ---- sessions -----------------------------------------------------------------------

    pub async fn handle_self_incoming(
        self: Arc<Self>,
        accepting: Accepting,
    ) -> Result<(), Error> {
        let conn = tokio::time::timeout(AUTH_TIMEOUT, accepting)
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(Error::net)?;
        let r = self.clone().self_run(conn.clone(), false).await;
        if r.is_err() {
            conn.close(0u32.into(), b"not accepted");
        }
        r
    }

    pub async fn self_dial(
        self: Arc<Self>,
        device: [u8; 32],
        relay: Option<String>,
    ) -> Result<(), Error> {
        let ep = self.endpoint()?;
        let addr = addr_for(&device, relay.as_deref())?;
        let conn = tokio::time::timeout(
            DIAL_TIMEOUT,
            node_runtime::version::connect(&ep, addr, &node_runtime::version::OWN),
        )
        .await
        .map_err(|_| Error::Timeout)??;
        let r = self.clone().self_run(conn.clone(), true).await;
        if r.is_err() {
            conn.close(0u32.into(), b"done");
        }
        r
    }

    pub fn tombstones(&self) -> HashSet<[u8; 32]> {
        self.shared
            .lock()
            .state
            .registry
            .iter()
            .filter(|e| e.removed)
            .map(|e| e.device)
            .collect()
    }

    /// Keeps one session per device: if both sides dialled at once, the connection dialled by
    /// the lower device key survives on both ends. `false` = this one lost.
    pub fn register_session(
        &self,
        remote: [u8; 32],
        dialer: [u8; 32],
        conn: &Connection,
    ) -> Option<(Acked, Dirty)> {
        let mut map = self.selfsync.sessions.lock();
        if let Some(old) = map.get(&remote) {
            if old.dialer < dialer {
                return None;
            }
            old.conn.close(0u32.into(), b"duplicate");
        }
        let acked: Acked = Default::default();
        let dirty: Dirty = Default::default();
        map.insert(
            remote,
            SelfSlot {
                dialer,
                conn: conn.clone(),
                acked: acked.clone(),
                dirty: dirty.clone(),
            },
        );
        Some((acked, dirty))
    }

    /// Waits (bounded) until every other live device of the registry has a session whose peer
    /// confirmed holding everything we hold (dialling the ones without).
    pub fn wait_pushed(&self, max: Duration) {
        self.sync_kick();
        let end = Instant::now() + max;
        loop {
            let (acc, others) = {
                let s = self.shared.lock();
                let me = s.me.as_ref().map(|m| m.device);
                let others: Vec<[u8; 32]> = s
                    .state
                    .registry
                    .iter()
                    .filter(|e| !e.removed && Some(e.device) != me)
                    .map(|e| e.device)
                    .collect();
                (s.acc.clone(), others)
            };
            let Some(acc) = acc else { return };
            let mine = acc.vv();
            let done = {
                let sessions = self.selfsync.sessions.lock();
                others.iter().all(|d| {
                    sessions.get(d).is_some_and(|s| {
                        s.acked.lock().as_ref().is_some_and(|v| {
                            v.partial_cmp(&mine)
                                .is_some_and(|o| o != std::cmp::Ordering::Less)
                        })
                    })
                })
            };
            if done || Instant::now() > end {
                tracing::debug!(
                    "wait_pushed: done={done} others={} sessions={}",
                    others.len(),
                    self.selfsync.sessions.lock().len()
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn drop_session(&self, remote: [u8; 32], conn: &Connection) {
        let mut map = self.selfsync.sessions.lock();
        if map
            .get(&remote)
            .is_some_and(|s| s.conn.stable_id() == conn.stable_id())
        {
            map.remove(&remote);
        }
    }

    pub async fn self_run(
        self: Arc<Self>,
        conn: Connection,
        dialer: bool,
    ) -> Result<(), Error> {
        let me = self.me()?;
        let epoch = me.epoch;
        let remote = *conn.remote_id().as_bytes();
        let (send, recv) = if dialer {
            conn.open_bi().await
        } else {
            conn.accept_bi().await
        }
        .map_err(Error::net)?;
        let mut writer = FrameWriter(send);
        let mut reader = FrameReader::new(recv);
        let relay = self.endpoint().ok().as_ref().and_then(relay_of);
        let my_auth = SelfMsg::Auth {
            attestation: me.attestation.clone(),
            relay,
            unlink: false,
        };
        if dialer {
            writer.send(&my_auth).await?;
        }
        let first: SelfMsg = tokio::time::timeout(AUTH_TIMEOUT, reader.recv())
            .await
            .map_err(|_| Error::Timeout)??
            .ok_or_else(|| Error::eof("peer closed before auth"))?;
        let SelfMsg::Auth {
            attestation,
            relay: remote_relay,
            unlink,
        } = first
        else {
            return Err(Error::Protocol("expected auth".into()));
        };
        let verdict =
            proto::accept_self_hello(&attestation, me.id.did(), remote, &self.tombstones());
        if let Err(e) = &verdict {
            if !dialer && matches!(e, proto::Error::Tombstoned) {
                // A removed device is told so (it has to prove it is ours first).
                writer.send(&my_auth).await?;
                writer.send(&SelfMsg::Unlinked).await?;
                writer.finish();
                let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
                return Ok(());
            }
            return Err(Error::Protocol("not one of our devices".into()));
        }
        if !dialer {
            writer.send(&my_auth).await?;
        }
        if unlink && !dialer {
            // The dialer only wants to tell us we were removed.
            let m: Option<SelfMsg> = tokio::time::timeout(AUTH_TIMEOUT, reader.recv())
                .await
                .map_err(|_| Error::Timeout)??;
            if matches!(m, Some(SelfMsg::Unlinked)) {
                self.clone().remove_self_account(epoch).await;
            }
            return Ok(());
        }
        let remote_relay = remote_relay.filter(|r| proto::valid_relay_hint(r));
        self.note_own_device(epoch, &attestation, remote, remote_relay);
        let dialer_key = if dialer { me.device } else { remote };
        let Some((acked, dirty)) = self.register_session(remote, dialer_key, &conn) else {
            return Ok(());
        };
        self.selfsync.backoff.lock().remove(&remote);
        self.log(format!(
            "own-device sync with {} ({})",
            proto::device_to_text(&remote),
            if dialer { "dialed" } else { "accepted" }
        ));
        let r = self
            .clone()
            .self_loop(&me, &conn, remote, &acked, &dirty, &mut reader, &mut writer)
            .await;
        self.drop_session(remote, &conn);
        conn.close(0u32.into(), b"done");
        if let Err(e) = &r {
            tracing::debug!("own-device sync with {} ended: {e}", node_runtime::short(&proto::device_to_text(&remote)));
        }
        r
    }

    pub fn peer_vv_of(update_vv: &[u8]) -> Option<VersionVector> {
        VersionVector::decode(update_vv).ok()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn self_loop(
        self: Arc<Self>,
        me: &Arc<Me>,
        conn: &Connection,
        remote: [u8; 32],
        acked: &Acked,
        dirty: &Dirty,
        reader: &mut FrameReader,
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        let epoch = me.epoch;
        let (acc, calls, profile, cache) = {
            let s = self.shared.lock();
            (
                s.acc.clone().ok_or(Error::Locked)?,
                s.calls.clone().ok_or(Error::Locked)?,
                s.profile.clone(),
                s.profile_cache.clone(),
            )
        };
        let mut docs = vec![
            (DOC_NAME.to_string(), acc.vv().encode()),
            (CALLS_DOC.to_string(), calls.vv().encode()),
        ];
        if let Some(p) = &profile {
            docs.push((PROFILE_DOC.into(), p.vv().encode()));
        }
        if let Some(c) = &cache {
            docs.push((PROFILE_CACHE_DOC.into(), c.vv().encode()));
        }
        docs.extend(self.own_content_catalog());
        docs.push((
            group_documents::CAPABILITY.into(),
            VersionVector::default().encode(),
        ));
        docs.push((
            group_history::CAPABILITY.into(),
            VersionVector::default().encode(),
        ));
        docs.push((
            group_receipts::CAPABILITY.into(),
            VersionVector::default().encode(),
        ));
        writer.send(&SelfMsg::Hello { docs }).await?;
        // What the peer is known to hold; nothing is sent until its Hello says.
        let mut sent: Option<VersionVector> = None;
        let mut sent_calls: Option<VersionVector> = None;
        let mut sent_profile: Option<VersionVector> = None;
        let mut sent_cache: Option<VersionVector> = None;
        let mut peer = PeerChat::default();
        let mut changes = self.signal.subscribe();
        // Idle = no stream data either way (QUIC acks and keep-alives do not count): then the
        // session closes cleanly and this device is in sync up to what the session saw.
        let traffic = |c: &Connection| {
            let st = c.stats();
            st.frame_tx.stream + st.frame_rx.stream
        };
        let mut seen = traffic(conn);
        let mut active = Instant::now();
        let mut idle = tokio::time::interval(node_runtime::wire::idle_check());
        loop {
            tokio::select! {
                _ = idle.tick() => {
                    let t = traffic(conn);
                    if t != seen {
                        seen = t;
                        active = Instant::now();
                    } else if active.elapsed() >= node_runtime::wire::idle_close() && !changes.has_changed().unwrap_or(true) {
                        self.selfsync_settled(remote, &changes);
                        writer.finish();
                        let _ = tokio::time::timeout(Duration::from_millis(300), conn.closed()).await;
                        self.log(format!("own-device sync with {} closed: idle", proto::device_to_text(&remote)));
                        return Ok(());
                    }
                }
                m = reader.recv::<SelfMsg>() => {
                    let Some(m) = m? else {
                        // The other device closed (idle on its side): in sync, unless a change
                        // of ours is still unseen.
                        self.selfsync_settled(remote, &changes);
                        return Ok(());
                    };
                    let mut full = false;
                    match m {
                        SelfMsg::Hello { docs } => {
                            if docs.len() > MAX_OWN_DOCS + 4 {
                                return Err(Error::Protocol("too many docs".into()));
                            }
                            peer.group_contents = group_contents::supported(&docs);
                            let enabled = group_history::supported(&docs);
                            if peer.hello && enabled != peer.history.enabled {
                                return Err(Error::Protocol("own history renegotiation".into()));
                            }
                            peer.history.enabled = enabled;
                            let receipts = group_receipts::supported(&docs);
                            if peer.hello && receipts != peer.receipts.enabled {
                                return Err(Error::Protocol("own receipts renegotiation".into()));
                            }
                            peer.receipts.enabled = receipts;
                            for (name, vv) in docs {
                                let Some(vv) = Self::peer_vv_of(&vv) else { continue };
                                if name == DOC_NAME {
                                    acked.lock().get_or_insert_with(VersionVector::new).merge(&vv);
                                    sent = Some(vv);
                                } else if name == CALLS_DOC {
                                    sent_calls = Some(vv);
                                } else if name == PROFILE_DOC {
                                    sent_profile = Some(vv);
                                } else if name == PROFILE_CACHE_DOC {
                                    sent_cache = Some(vv);
                                } else if parse_doc(&name).is_some()
                                    || (peer.group_contents && group_documents::parse_doc(&name).is_some())
                                {
                                    group_contents::remember(&mut peer, name, vv);
                                }
                            }
                            peer.hello = true;
                            full = true;
                        }
                        SelfMsg::Sync { doc, vv, update, sig } => {
                            if !chat_sig_ok(&remote, &doc, &update, &sig) {
                                return Err(Error::Protocol("bad batch signature".into()));
                            }
                            if doc == DOC_NAME {
                                if !update.is_empty() {
                                    self.apply_account_update(epoch, &update)?;
                                    // Tell the sender what we hold now (an empty batch is an ack).
                                    let sig = chat_runtime::chat::wire::sign_batch(&me.profile.device_secret, DOC_NAME, &[]);
                                    writer.send(&SelfMsg::Sync { doc: DOC_NAME.into(), vv: acc.vv().encode(), update: Vec::new(), sig }).await?;
                                }
                                if let Some(theirs) = Self::peer_vv_of(&vv) {
                                    if let Some(s) = sent.as_mut() {
                                        s.merge(&theirs);
                                    }
                                    acked.lock().get_or_insert_with(VersionVector::new).merge(&theirs);
                                }
                                self.touch_device(epoch, remote);
                            } else if doc == CALLS_DOC {
                                if !update.is_empty() {
                                    self.apply_calls_update(epoch, &update)?;
                                    let sig = chat_runtime::chat::wire::sign_batch(&me.profile.device_secret, CALLS_DOC, &[]);
                                    writer.send(&SelfMsg::Sync { doc: CALLS_DOC.into(), vv: calls.vv().encode(), update: Vec::new(), sig }).await?;
                                }
                                if let (Some(s), Some(theirs)) = (sent_calls.as_mut(), Self::peer_vv_of(&vv)) {
                                    s.merge(&theirs);
                                }
                            } else if doc == PROFILE_DOC || doc == PROFILE_CACHE_DOC {
                                if !update.is_empty() {
                                    if doc == PROFILE_DOC { self.chat.apply_profile_update(epoch, &update)?; }
                                    else { self.chat.apply_profile_cache_update(epoch, &update)?; }
                                    let current = if doc == PROFILE_DOC { profile.as_deref().map(|p| &**p) } else { cache.as_deref().map(|p| &**p) }.ok_or(Error::Locked)?;
                                    let sig = chat_runtime::chat::wire::sign_batch(&me.profile.device_secret, &doc, &[]);
                                    writer.send(&SelfMsg::Sync { doc: doc.clone(), vv: current.vv().encode(), update: Vec::new(), sig }).await?;
                                }
                                let sent_doc = if doc == PROFILE_DOC { &mut sent_profile } else { &mut sent_cache };
                                if let (Some(s), Some(v)) = (sent_doc.as_mut(), Self::peer_vv_of(&vv)) { s.merge(&v); }
                            } else if group_documents::parse_doc(&doc).is_some() {
                                if !peer.group_contents {
                                    return Err(Error::Protocol("group contents were not negotiated".into()));
                                }
                                if !update.is_empty() {
                                    // Error ends the session so transient persistence failures
                                    // are retried; never ack a rejected or unpersisted import.
                                    let accepted = self.group.group_own_content_apply(epoch, &remote, &doc, &update)?;
                                    let sig = chat_runtime::chat::wire::sign_batch(&me.profile.device_secret, &doc, &[]);
                                    writer.send(&SelfMsg::Sync { doc: doc.clone(), vv: accepted, update: Vec::new(), sig }).await?;
                                }
                                if let Some(theirs) = Self::peer_vv_of(&vv) {
                                    group_contents::remember(&mut peer, doc.clone(), theirs);
                                }
                                if peer.hello {
                                    self.push_group_doc(me, &remote, &doc, &mut peer, writer).await?;
                                }
                            } else if parse_doc(&doc).is_some() {
                                // A rejected or unusable batch is dropped, the session goes on.
                                if !update.is_empty()
                                    && let Err(e) = self.chat.chat_own_apply(&doc, &update).await
                                {
                                    tracing::debug!("own-device chat batch for {doc}: {e}");
                                }
                                if let Some(theirs) = Self::peer_vv_of(&vv) {
                                    group_contents::remember(&mut peer, doc.clone(), theirs);
                                }
                                if peer.hello {
                                    self.push_chat_doc(me, &doc, &mut peer, writer).await?;
                                }
                            }
                        }
                        SelfMsg::Acks { acks } => {
                            if acks.len() > MAX_OWN_DOCS {
                                return Err(Error::Protocol("too many acks".into()));
                            }
                            for (doc, vv) in &acks {
                                if let Some(v) = Self::peer_vv_of(vv) {
                                    peer.acks.entry(doc.clone()).and_modify(|x| x.merge(&v)).or_insert(v);
                                }
                            }
                            if let Err(e) = self.chat.chat_apply_acks(&acks) {
                                tracing::debug!("own-device acks: {e}");
                            }
                        }
                        SelfMsg::Read { cursors } => {
                            for (pair, c) in &cursors {
                                let e = peer.read.entry(pair.clone()).or_insert(0);
                                *e = (*e).max(*c);
                            }
                            if let Err(e) = self.chat.chat_apply_read(&cursors).await {
                                tracing::debug!("own-device read cursors: {e}");
                            }
                        }
                        SelfMsg::Groups { groups } => {
                            if groups.len() > group_runtime::own::MAX_OWN_GROUPS {
                                return Err(Error::Protocol("too many groups".into()));
                            }
                            self.group.own_group_admit(epoch, &remote)?;
                            self.group.group_own_apply(&groups, peer.group_contents);
                            if peer.history.enabled { peer.history.offer(&self, &groups); }
                            if peer.receipts.enabled { peer.receipts.offer(&self, &groups); }
                        }
                        SelfMsg::Invitations { rows } => {
                            if rows.len() > group_runtime::invitations::MAX_OWN_INVITATIONS {
                                return Err(Error::Protocol("too many invitations".into()));
                            }
                            self.group.group_own_invitations_apply(&rows);
                        }
                        SelfMsg::Unlinked => {
                            self.clone().remove_self_account(epoch).await;
                            return Ok(());
                        }
                        message @ (SelfMsg::GroupHistoryGet { .. } | SelfMsg::GroupHistoryPage { .. }
                            | SelfMsg::GroupWant { .. } | SelfMsg::GroupBatch { .. } | SelfMsg::GroupHistoryChanged { .. }) => {
                            group_history::handle(&self, me, &remote, &mut peer, message, writer).await?;
                        }
                        message @ (SelfMsg::GroupReceiptGet { .. } | SelfMsg::GroupReceiptPage { .. }) => {
                            group_receipts::handle(&self, me, &remote, &mut peer, message, writer).await?;
                        }
                        SelfMsg::Auth { .. } => return Err(Error::Protocol("unexpected auth".into())),
                    }
                    self.push_changes(me, DOC_NAME, &acc, &mut sent, writer).await?;
                    self.push_changes(me, CALLS_DOC, &calls, &mut sent_calls, writer).await?;
                    if let Some(p) = &profile { self.push_changes(me, PROFILE_DOC, p, &mut sent_profile, writer).await?; }
                    if let Some(c) = &cache { self.push_changes(me, PROFILE_CACHE_DOC, c, &mut sent_cache, writer).await?; }
                    if full {
                        self.push_all_chat(me, &remote, &mut peer, writer).await?;
                        self.chat.chat_resume_all();
                    }
                    peer.history.pump(&self, me, &remote, writer).await?;
                    peer.receipts.pump(&self, me, &remote, writer).await?;
                }
                _ = tokio::time::sleep(peer.receipts.delay()), if peer.receipts.enabled => {
                    peer.receipts.pump(&self, me, &remote, writer).await?;
                }
                _ = tokio::time::sleep(peer.history.delay()), if peer.history.enabled => {
                    peer.history.pump(&self, me, &remote, writer).await?;
                }
                _ = changes.changed() => {
                    if self.shared.lock().epoch != epoch {
                        return Ok(());
                    }
                    self.push_changes(me, DOC_NAME, &acc, &mut sent, writer).await?;
                    self.push_changes(me, CALLS_DOC, &calls, &mut sent_calls, writer).await?;
                    if let Some(p) = &profile { self.push_changes(me, PROFILE_DOC, p, &mut sent_profile, writer).await?; }
                    if let Some(c) = &cache { self.push_changes(me, PROFILE_CACHE_DOC, c, &mut sent_cache, writer).await?; }
                    let names: Vec<String> = std::mem::take(&mut *dirty.lock()).into_iter().collect();
                    if peer.hello {
                        for name in names {
                            if name == READ_MARK {
                                self.push_read(&mut peer, writer).await?;
                            } else if name == GROUPS_MARK {
                                self.push_groups(&mut peer, writer).await?;
                            } else if name == group_runtime::invitations::INVITES_MARK {
                                self.push_invitations(&mut peer, writer).await?;
                            } else if name == ACK_MARK {
                                self.push_acks(&mut peer, writer).await?;
                            } else if let Some((gid, _)) = group_documents::parse_doc(&name) {
                                peer.receipts.refresh(&gid);
                                if peer.group_contents {
                                    self.push_group_doc(me, &remote, &name, &mut peer, writer).await?;
                                }
                            } else {
                                self.push_chat_doc(me, &name, &mut peer, writer).await?;
                            }
                        }
                    }
                    peer.history.pump(&self, me, &remote, writer).await?;
                    peer.receipts.pump(&self, me, &remote, writer).await?;
                }
            }
        }
    }
}
