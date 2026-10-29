//! Lifecycle, sessions, dialing and the per-connection loop.

use super::*;

/// The end reason of a session closed for being idle (`wire::idle_close`).
const IDLE: &str = "idle";

impl ChatRt {
    // ---- lifecycle -------------------------------------------------------------------------

    /// Opens the sealed chat store; called whenever the data key becomes available. True when
    /// this call opened it (the caller then reconciles the search index).
    pub fn chat_open(self: &Arc<Self>) -> bool {
        let (phrase, did) = {
            let s = self.shared.lock();
            match (s.dek.as_ref(), s.me.as_ref()) {
                (Some(_), Some(me)) => (zeroize::Zeroizing::new(me.profile.mnemonic.clone()), me.id.did().to_string()),
                _ => return false,
            }
        };
        if self.core.lock().is_some() {
            return false;
        }
        // Nothing to open before the account has a directory (an uncommitted identity).
        let dir = match self.shared.lock().acct.as_ref() {
            Some(a) => a.store.dir().to_path_buf(),
            None => return false,
        };
        // A chat store that is not there yet starts from nothing: whatever this install wrote
        // before under its writer ID (a lost or deleted store) is not in it, but contacts may hold
        // it. Writing again from counter 0 under the same writer ID would hand them ops they take
        // for ones they have and drop (P10), so a fresh store gets a fresh device ID, hence a
        // fresh writer ID. Saved before the file exists: a crash in between only rotates again.
        let path = dir.join("chat.redb");
        if !path.exists()
            && let Err(e) = self.new_device_id()
        {
            tracing::warn!("chat store: device id: {e}");
            return false;
        }
        // Keyed by the recovery phrase (rekey.rs). The node before a restart may still hold the
        // file for a moment: `open_waiting`.
        let sealed = match node_runtime::store::open_waiting(&path)
            .and_then(|db| node_runtime::rekey::open(db, node_runtime::rekey::key_from_phrase(node_runtime::rekey::Which::Chat, &phrase)))
        {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("chat store: {e}");
                return false;
            }
        };
        let store = ChatStore::new(sealed);
        let value = match self.device_id() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("chat store: device id: {e}");
                return false;
            }
        };
        self.search.open(&dir, &phrase);
        let receipts = receipts_core::store::messages::MessageReceipts::new(receipts_core::store::ReceiptStore::new(store.db.clone()), &did, value);
        let core = Arc::new(ChatCore {
            receipts,
            store,
            dir,
            hub: OnceCell::new(),
            shards: Default::default(),
            sessions: Default::default(),
            dialing: Default::default(),
            downloading: Default::default(),
            next_dl: AtomicU64::new(1),
            pulling: Default::default(),
            dial_sem: Arc::new(Semaphore::new(4)),
            next_session: AtomicU64::new(1),
            presence: crate::chat::presence::Presence::new(),
            activity: Default::default(),
            device_id: value,
            wid: writer_id(&did, &value),
            fail_writes: Default::default(),
            clock_ms: Default::default(),
            clock_wrong: Default::default(),
        });
        *self.core.lock() = Some(core);
        // Receipt intents left by a crash/failed commit before this start.
        self.chat_receipts_pump();
        true
    }

    /// Gives this install a new device ID (`ProfileV2::device_id`) and saves the profile.
    pub(super) fn new_device_id(&self) -> Result<(), Error> {
        let mut s = self.shared.lock();
        let Some(node_runtime::store::Disk::V2(p)) = s.disk.as_ref() else { return Err(Error::NoIdentity) };
        let mut p = p.clone();
        p.device_id = Some(node_runtime::store::new_device_id());
        let disk = node_runtime::store::Disk::V2(p);
        s.acct.as_ref().ok_or(Error::NoIdentity)?.store.save_profile(&disk)?;
        s.disk = Some(disk);
        Ok(())
    }

    /// This install's device ID (`ProfileV2::device_id`, U16); made here once if the profile
    /// has none yet.
    pub(super) fn device_id(&self) -> Result<[u8; 32], Error> {
        let mut s = self.shared.lock();
        let Some(node_runtime::store::Disk::V2(p)) = s.disk.as_ref() else { return Err(Error::NoIdentity) };
        if let Some(i) = p.device_id_bytes() {
            return Ok(i);
        }
        let mut p = p.clone();
        p.device_id = Some(node_runtime::store::new_device_id());
        let disk = node_runtime::store::Disk::V2(p.clone());
        s.acct.as_ref().ok_or(Error::NoIdentity)?.store.save_profile(&disk)?;
        s.disk = Some(disk);
        p.device_id_bytes().ok_or(Error::NoIdentity)
    }

    pub async fn chat_close(self: &Arc<Self>) {
        let core = self.core.lock().take();
        if let Some(core) = core {
            core.close_all_sessions();
            if let Some(h) = core.hub.get() {
                h.shutdown().await;
            }
            // GL: the file goes now, whatever task still holds the core.
            core.store.db.close();
        }
        self.search.close();
    }

    /// `chat_close` without waiting (for `Node`'s drop on a core thread): the store closes, the
    /// blob store is left to its drop.
    pub fn chat_close_now(&self) {
        if let Some(core) = self.core.lock().take() {
            core.close_all_sessions();
            core.store.db.close();
        }
        self.search.close();
    }

    pub fn chat_sessions_close(self: &Arc<Self>) {
        if let Some(core) = self.core.lock().clone() {
            core.close_all_sessions();
        }
    }

    pub fn chat_core(&self) -> Result<Arc<ChatCore>, Error> {
        match self.core.lock().clone() {
            Some(c) => Ok(c),
            None => match self.me() {
                Err(e) => Err(e),
                Ok(_) => Err(Error::Locked),
            },
        }
    }

    pub fn ev(&self) -> Option<Arc<dyn ChatSink>> {
        self.chat_events.lock().clone()
    }

    /// Called once the endpoint is up: reconnect to everyone we owe something, and keep trying.
    /// Not to everyone (2026-10-17): delivery is sender-driven, so a contact who wrote while we
    /// were away dials us; we dial only contacts we have something pending for.
    pub fn chat_started(self: &Arc<Self>) {
        let this = self.clone();
        self.handle.spawn(async move {
            if let Ok(me) = this.me() {
                this.profile_download(me.id.did().into());
                this.profile_apply_deletions(me.epoch, me.id.did());
                let dids = this.shared.lock().state.contacts.iter().map(|c| c.did.clone()).collect::<Vec<_>>();
                for did in dids { this.profile_apply_deletions(me.epoch, &did); }
            }
            this.chat_kick_pending();
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            let mut n = 0u32;
            loop {
                tick.tick().await;
                if this.shared.lock().endpoint.is_none() {
                    break;
                }
                this.chat_kick_pending();
                n += 1;
                if n % 120 == 1
                    && let Err(e) = this.chat_compact().await
                {
                    tracing::warn!("chat compaction: {e}");
                }
            }
        });
    }

    /// Dials every contact we have something pending for (`chat_needs_session`). Not contacts
    /// whose conversation is merely open: links are consent-based (2026-10-17), typing dials.
    pub fn chat_kick_pending(self: &Arc<Self>) {
        let Ok(core) = self.chat_core() else { return };
        let Ok(me) = self.me() else { return };
        let dids: Vec<String> = self.shared.lock().state.contacts.iter().map(|c| c.did.clone()).collect();
        for did in dids {
            self.profile_download(did.clone());
            if self.chat_needs_session(&core, &me, &did) {
                self.chat_kick(did);
            }
        }
    }

    /// Something for `did` that needs a session: unsent ops or profile, wanted downloads, owed
    /// receipts.
    pub(super) fn chat_needs_session(&self, core: &ChatCore, me: &Me, did: &str) -> bool {
        let pair = pair_id(me.id.did(), did);
        self.profile_pending(did) || core.store.pending_days(&pair).map(|d| !d.is_empty()).unwrap_or(false) || self.chat_wants(core, &pair) || Self::chat_receipts_owed(core, &pair)
    }

    /// Whether we want a session with `did` kept open past the idle close: the user here acted
    /// towards it within the idle window, something is pending for it, or a 1:1 call with it is
    /// live. Only our own reasons: a contact cannot hold our link open (2026-10-17).
    pub fn chat_keep(&self, core: &ChatCore, did: &str) -> bool {
        if core.locally_active(did) {
            return true;
        }
        if self.in_call_with(did) {
            return true;
        }
        self.me().is_ok_and(|me| self.chat_needs_session(core, &me, did))
    }

    pub fn in_call_with(&self, did: &str) -> bool {
        self.live.lock().call.as_ref().is_some_and(|c| c.info.peer_did == did)
    }

    /// The user is typing to `did` (`Node::chat_typing`): local activity, and a session dialled
    /// ahead of the send (or our presence sent now on the one that is up). Throttled, so it is
    /// cheap per keystroke. Nothing goes to the contact about the typing itself.
    pub fn chat_typing(self: &Arc<Self>, did: &str) {
        if self.is_self_did(did) {
            return;
        }
        let Ok(core) = self.chat_core() else { return };
        if !core.mark_active(did, Some(node_runtime::wire::typing_throttle())) {
            return;
        }
        self.chat_wake(&core, did);
    }

    /// A live 1:1 call with `did` is activity towards it (2026-10-17): our presence goes to it
    /// while the call is live and for the idle window after.
    pub fn chat_call_activity(self: &Arc<Self>, did: &str) {
        if self.is_self_did(did) {
            return;
        }
        let Ok(core) = self.chat_core() else { return };
        core.mark_active(did, None);
        self.chat_wake(&core, did);
    }

    /// After local activity towards `did`: dial it, or send our presence now on its sessions.
    pub(super) fn chat_wake(self: &Arc<Self>, core: &ChatCore, did: &str) {
        let sessions = self.chat_sessions_of(core, did);
        if sessions.is_empty() {
            self.chat_kick(did.to_string());
        }
        for s in sessions {
            s.cmd(Cmd::Presence);
        }
    }

    /// A new message for `did`: the session reaches one of their devices, and the others hear of
    /// it only from that device, which cannot wake them. Wake those we hold a token for. With no
    /// session yet, the dial does this once it connects.
    pub(super) fn chat_wake_others(self: &Arc<Self>, core: &ChatCore, did: &str) {
        let reached: Vec<[u8; 32]> = self.chat_sessions_of(core, did).iter().map(|s| s.device).collect();
        if !reached.is_empty() {
            self.ctx.wake_others(did, &reached);
        }
    }

    pub(super) fn chat_wants(&self, core: &ChatCore, pair: &str) -> bool {
        core.store
            .wanted_blobs()
            .map(|v| v.iter().any(|(_, b)| b.pair == pair))
            .unwrap_or(false)
    }

    pub(super) fn chat_session(&self, core: &ChatCore, did: &str) -> Option<Arc<Session>> {
        core.sessions.lock().values().find(|s| s.did == did).cloned()
    }

    /// Whether any chat session is up or being dialled (a background account stays open while
    /// one is, `docs/multi-online.md`).
    pub fn chat_busy(&self) -> bool {
        self.chat_core().is_ok_and(|core| !core.sessions.lock().is_empty() || !core.dialing.lock().is_empty())
    }

    pub fn chat_sessions_of(&self, core: &ChatCore, did: &str) -> Vec<Arc<Session>> {
        core.sessions.lock().values().filter(|s| s.did == did).cloned().collect()
    }

    /// Makes sure there is (or soon will be) a session with `did`, retrying with backoff while
    /// there is something to send.
    pub fn chat_kick(self: &Arc<Self>, did: String) {
        let Ok(core) = self.chat_core() else { return };
        if self.chat_session(&core, &did).is_some() || !core.dialing.lock().insert(did.clone()) {
            return;
        }
        let this = self.clone();
        self.handle.spawn(async move {
            let mut delay = Duration::from_secs(2);
            loop {
                if this.chat_session(&core, &did).is_some() {
                    break;
                }
                let Ok(me) = this.me() else { break };
                if this.shared.lock().endpoint.is_none() || !this.chat_may_dial(&did) {
                    break;
                }
                let pair = pair_id(me.id.did(), &did);
                let has_pending = |this: &Arc<Self>| this.chat_dial_has_pending(&core, &did, &pair);
                let permit = core.dial_sem.clone().acquire_owned().await;
                // If the dial has not connected after MSG_EARLY_WAKE_MS and there is something to
                // deliver (now or by then: typing dials before the send), wake the devices we hold
                // a token for while it goes on (docs/push.md). Dropped with it.
                let devices = this.shared.lock().state.contacts.iter().find(|c| c.did == did).map(|c| c.devices.iter().map(|d| d.device).collect()).unwrap_or_default();
                let pending = {
                    let (this, core, did, pair) = (this.clone(), core.clone(), did.clone(), pair.clone());
                    move || this.chat_dial_has_pending(&core, &did, &pair)
                };
                let early = this.ctx.early_wake(&did, devices, pending);
                let (res, unreached) = this.chat_dial_devices(&core, &me, &did).await;
                drop(early);
                drop(permit);
                match res {
                    Ok(()) => {
                        this.ctx.wake_reached(&did);
                        if has_pending(&this) {
                            this.chat_wake_others(&core, &did);
                        }
                        break;
                    }
                    Err(e) => tracing::debug!("chat dial {}: {e}", short(&did)),
                }
                if !has_pending(&this) {
                    break;
                }
                // Something to deliver and a device out of reach: wake it (W2; docs/push.md). The
                // wake crate's throttle decides whether this dial may; a wake that was accepted
                // makes the next dials come sooner than the normal backoff.
                this.ctx.wake_unreached(&did, &unreached).await;
                let pause = this.ctx.fast_redial(&did).unwrap_or_else(|| {
                    let d = delay;
                    delay = (delay * 2).min(Duration::from_secs(60));
                    d
                });
                tokio::time::sleep(pause).await;
            }
            core.dialing.lock().remove(&did);
        });
    }

    /// Something a chat dial of `did` has to deliver: unsent ops or profile, wanted downloads.
    fn chat_dial_has_pending(&self, core: &ChatCore, did: &str, pair: &str) -> bool {
        self.profile_pending(did) || core.store.pending_days(pair).map(|d| !d.is_empty()).unwrap_or(false) || self.chat_wants(core, pair)
    }

    // ---- connections -------------------------------------------------------------------------

    pub(super) fn auth_msg(&self, me: &Me, grant: proto::SignedGrant) -> ChatMsg {
        let relay = self.endpoint().ok().and_then(|ep| relay_of(&ep));
        ChatMsg::Auth { attestation: me.attestation.clone(), grant, relay }
    }

    /// Verifies a peer's `Auth` like a call hello: attested device = the connection's key, a
    /// token we issued them, and they are a contact. Returns their DID (blocked or not: the
    /// caller answers a blocked one with our notice) and the token, which `chat_admit` checks.
    pub(super) fn verify_auth(&self, me: &Me, auth: &ChatMsg, remote: [u8; 32]) -> Result<(String, Option<String>, proto::Grant), Error> {
        let ChatMsg::Auth { attestation, grant, relay } = auth else { return Err(Error::Protocol("expected Auth".into())) };
        let msg = Msg::CallHello { call_id: "chat".into(), attestation: attestation.clone(), grant: grant.clone(), relay: relay.clone(), devices: None, video: None, camera: false };
        let s = self.shared.lock();
        let contacts = &s.state.contacts;
        let who = proto::accept_hello(&me.id, &msg, remote, node_runtime::now_ms(), |did| contacts.iter().any(|c| c.did == did))?;
        Ok((who.did, relay_hint(relay), who.grant))
    }

    /// P6: chat needs a message token of ours that was not taken back (a block's cut-off); a
    /// call token does not open chat. Renews the token when it is close to expiry. An admitted
    /// session (either direction) means `did` is reachable: its wake schedule starts over.
    pub(super) fn chat_admit(self: &Arc<Self>, did: &str, grant: &proto::Grant) -> Result<(), Error> {
        if self.token_check(did, grant, proto::Cap::Message).is_err() {
            self.log(format!("chat refused: {did}: no valid message token"));
            return Err(Error::Rejected("not accepted".into()));
        }
        self.ctx.renew_message_if_due(did, grant);
        self.ctx.wake_reached(did);
        Ok(())
    }

    /// Whether we keep a session with `did` going: a contact that neither side blocks.
    pub(super) fn chat_may_dial(&self, did: &str) -> bool {
        let s = self.shared.lock();
        s.state.contacts.iter().any(|c| c.did == did) && !s.state.blocked.contains(did) && !node_runtime::block::blocked_me(&s.state, did)
    }

    pub(super) async fn chat_dial(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Arc<Me>, did: &str) -> Result<(), Error> {
        self.chat_dial_devices(core, me, did).await.0
    }

    /// `chat_dial`, and when it failed, the devices it could not even connect to (a timeout or a
    /// network error; not one that answered and refused): the ones a wake may help.
    pub(super) async fn chat_dial_devices(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Arc<Me>, did: &str) -> (Result<(), Error>, Vec<[u8; 32]>) {
        let mut unreached = Vec::new();
        let res = self.chat_dial_each(core, me, did, &mut unreached).await;
        if res.is_ok() {
            unreached.clear();
        }
        (res, unreached)
    }

    async fn chat_dial_each(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Arc<Me>, did: &str, unreached: &mut Vec<[u8; 32]>) -> Result<(), Error> {
        let ep = self.endpoint()?;
        let contact = self
            .shared
            .lock()
            .state
            .contacts
            .iter()
            .find(|c| c.did == did)
            .cloned()
            .ok_or(Error::NotFound)?;
        let mut last = Error::NotFound;
        for cd in &contact.devices {
            let addr = match addr_for(&cd.device, cd.relay.as_deref()) {
                Ok(a) => a,
                Err(e) => {
                    last = e;
                    continue;
                }
            };
            self.ctx.list_before(&ep, did, cd.device, cd.relay.as_deref()).await;
            let conn = match tokio::time::timeout(DIAL_TIMEOUT, node_runtime::version::connect(&ep, addr, &node_runtime::version::CHAT)).await {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => {
                    if matches!(e, Error::Protocol(_)) {
                        self.log(format!("chat with {did}: {e}"));
                    }
                    if matches!(e, Error::Net(_) | Error::Timeout) {
                        unreached.push(cd.device);
                    }
                    last = e;
                    continue;
                }
                Err(_) => {
                    unreached.push(cd.device);
                    last = Error::Timeout;
                    continue;
                }
            };
            match self.chat_handshake(core, me, conn, true).await {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// Opens (dialer) or accepts (listener) the stream, exchanges `Auth`, registers the session
    /// and starts its task.
    pub(super) async fn chat_handshake(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Arc<Me>, conn: Connection, dialer: bool) -> Result<(), Error> {
        let remote = *conn.remote_id().as_bytes();
        let (send, recv) = if dialer { conn.open_bi().await } else { conn.accept_bi().await }.map_err(Error::net)?;
        let mut writer = ChatWriter(send);
        let mut reader = ChatReader::new(recv);
        let (did, hint) = if dialer {
            // We know who we are calling: present the grant they gave us.
            let grant = {
                let s = self.shared.lock();
                let c = s.state.contacts.iter().find(|c| c.has_device(&remote));
                c.map(|c| c.grant_from_them.clone())
            };
            let grant = grant.ok_or(Error::NotFound)?;
            writer.send(&self.auth_msg(me, grant)).await?;
            let reply = tokio::time::timeout(AUTH_TIMEOUT, reader.recv_first())
                .await
                .map_err(|_| Error::Timeout)??
                .ok_or_else(|| Error::eof("peer closed"))?;
            if let ChatMsg::Notice { notice } = &reply {
                // They block us: their notice instead of their Auth.
                let did = self.shared.lock().state.contacts.iter().find(|c| c.has_device(&remote)).map(|c| c.did.clone());
                if let Some(did) = did
                    && self.is_block_from(&did, notice)
                {
                    let _ = self.ctx.take_notice(notice);
                    return Err(Error::Rejected(node_runtime::BLOCKED.into()));
                }
                return Err(Error::Protocol("unexpected notice".into()));
            }
            let (did, hint, grant) = self.verify_auth(me, &reply, remote)?;
            if self.shared.lock().state.blocked.contains(&did) {
                return Err(Error::Rejected(node_runtime::BLOCKED.into()));
            }
            self.chat_admit(&did, &grant)?;
            (did, hint)
        } else {
            let first = tokio::time::timeout(AUTH_TIMEOUT, reader.recv_first())
                .await
                .map_err(|_| Error::Timeout)??
                .ok_or_else(|| Error::eof("peer closed before auth"))?;
            let (did, hint, grant) = self.verify_auth(me, &first, remote)?;
            // A DID we block gets our notice instead of our Auth, then the connection ends.
            if let Some(notice) = self.refusal_for(&did) {
                self.log("chat refused: blocked");
                writer.send(&ChatMsg::Notice { notice }).await?;
                let _ = writer.0.finish();
                let _ = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
                return Err(Error::Rejected(node_runtime::BLOCKED.into()));
            }
            self.chat_admit(&did, &grant)?;
            let grant = {
                let s = self.shared.lock();
                s.state.contacts.iter().find(|c| c.did == did).map(|c| c.grant_from_them.clone())
            }
            .ok_or(Error::NotFound)?;
            writer.send(&self.auth_msg(me, grant)).await?;
            (did, hint)
        };
        self.ctx.note_device(me.epoch, &did, remote, hint);
        let (tx, rx) = mpsc::unbounded_channel();
        let my_dev = me.device;
        let sess = Arc::new(Session {
            id: core.next_session.fetch_add(1, Ordering::Relaxed),
            did: did.clone(),
            device: remote,
            epoch: me.epoch,
            profile_vv: Default::default(),
            dialer: if dialer { my_dev } else { remote },
            tx,
            peer_vv: Default::default(),
            receipt_vv: Default::default(),
            hist: Default::default(),
            presence: crate::chat::presence::session_store(),
        });
        // One session per contact: if both sides dialled at once, the one dialled by the lower
        // device key survives on both ends.
        {
            let mut map = core.sessions.lock();
            if let Some(old) = map.get(&remote)
                && old.dialer != sess.dialer
                && old.dialer < sess.dialer
            {
                conn.close(0u32.into(), b"duplicate");
                return Ok(());
            }
            if let Some(old) = map.insert(remote, sess.clone()) {
                let _ = old.tx.send(Cmd::Close);
            }
        }
        self.log(format!("chat session with {did} ({})", if dialer { "dialed" } else { "accepted" }));
        let this = self.clone();
        let core = core.clone();
        self.handle.spawn(async move {
            this.chat_run(core, sess, conn, reader, writer, rx).await;
        });
        Ok(())
    }

    pub async fn handle_chat_incoming(self: Arc<Self>, accepting: Accepting, chat: bool, permit: tokio::sync::OwnedSemaphorePermit) -> Result<(), Error> {
        let conn = tokio::time::timeout(AUTH_TIMEOUT, accepting).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?;
        self.handle_chat_conn(conn, chat, permit).await
    }

    /// `handle_chat_incoming` once the connection is up.
    pub async fn handle_chat_conn(self: Arc<Self>, conn: Connection, chat: bool, permit: tokio::sync::OwnedSemaphorePermit) -> Result<(), Error> {
        let remote = *conn.remote_id().as_bytes();
        if chat {
            let me = self.me()?;
            let core = self.chat_core()?;
            if !self.is_contact_device(&remote) {
                // A new device of a contact is recognised by its Auth; strangers by nothing.
                // Fall through to the handshake, which checks the proof.
            }
            let r = self.chat_handshake(&core, &me, conn.clone(), false).await;
            drop(permit);
            if r.is_err() {
                conn.close(0u32.into(), b"not accepted");
            }
            return r;
        }
        drop(permit);
        // Blobs: only devices we know as a contact's (or in an authenticated session).
        let known = self.is_contact_device(&remote)
            || self.is_own_device(&remote)
            || self.chat_core().map(|c| c.sessions.lock().values().any(|s| s.device == remote)).unwrap_or(false);
        if !known {
            conn.close(0u32.into(), b"not accepted");
            return Ok(());
        }
        let hub = self.chat_hub().await?;
        use iroh::protocol::ProtocolHandler;
        hub.protocol().accept(conn).await.map_err(|e| Error::Net(e.to_string()))
    }

    pub(super) async fn chat_run(
        self: Arc<Self>,
        core: Arc<ChatCore>,
        sess: Arc<Session>,
        conn: Connection,
        mut reader: ChatReader,
        mut writer: ChatWriter,
        mut cmds: mpsc::UnboundedReceiver<Cmd>,
    ) {
        let reason: String = 'run: {
            match self.chat_hello(&core, &sess) {
                Ok(h) => {
                    if let Err(e) = writer.send(&h).await {
                        break 'run e.to_string();
                    }
                }
                Err(e) => break 'run e.to_string(),
            }
            match self.chat_receipt_hello(&core, &sess) {
                Ok(h) => {
                    if let Err(e) = writer.send(&h).await {
                        break 'run e.to_string();
                    }
                }
                Err(e) => tracing::warn!("chat receipts: hello for {}: {e}", short(&sess.did)),
            }
            self.chat_resume_downloads(&core, &sess.did);
            let mut heartbeat = tokio::time::interval(crate::chat::presence::HEARTBEAT);
            // Idle = no ChatMsg either way and no local activity (`chat_keep`); presence datagrams
            // do not count.
            let mut active = std::time::Instant::now();
            let mut idle = tokio::time::interval(node_runtime::wire::idle_check());
            loop {
                tokio::select! {
                    _ = heartbeat.tick() => {
                        self.presence_send(&core, &sess.did, &conn);
                        self.presence_update(&core, &sess.did);
                    }
                    _ = idle.tick() => {
                        if active.elapsed() >= node_runtime::wire::idle_close() && !self.chat_keep(&core, &sess.did) {
                            break 'run IDLE.into();
                        }
                    }
                    d = conn.read_datagram() => match d {
                        Ok(d) => self.presence_receive(&core, &sess, &d),
                        Err(e) => break 'run format!("connection lost: {e}"),
                    },
                    m = reader.recv() => match m {
                        Ok(Some(m)) => {
                            active = std::time::Instant::now();
                            if let Err(e) = self.chat_handle(&core, &sess, m, &mut writer).await {
                                break 'run e.to_string();
                            }
                        }
                        Ok(None) => break 'run "closed".into(),
                        Err(e) => break 'run e.to_string(),
                    },
                    c = cmds.recv() => match c {
                        Some(Cmd::Presence) => self.presence_send(&core, &sess.did, &conn),
                        Some(Cmd::Push(day)) => {
                            active = std::time::Instant::now();
                            if let Err(e) = self.chat_push(&core, &sess, &day, &mut writer).await {
                                break 'run e.to_string();
                            }
                        }
                        Some(Cmd::Ack(day)) => {
                            active = std::time::Instant::now();
                            let me = match self.me() { Ok(m) => m, Err(e) => break 'run e.to_string() };
                            let pair = pair_id(me.id.did(), &sess.did);
                            if let Ok(Some(meta)) = core.store.shard_meta(&pair, &day)
                                && let Err(e) = writer.send(&ChatMsg::Ack { doc: doc_name(&pair, &day), vv: meta.vv }).await
                            {
                                break 'run e.to_string();
                            }
                        }
                        Some(Cmd::History(before, limit)) => {
                            active = std::time::Instant::now();
                            if let Err(e) = writer.send(&ChatMsg::HistoryReq { before, limit }).await {
                                break 'run e.to_string();
                            }
                        }
                        Some(Cmd::Profile) => {
                            active = std::time::Instant::now();
                            if let Err(e) = self.chat_profile_send(&sess, &mut writer).await { break 'run e.to_string(); }
                        }
                        Some(Cmd::Receipts(day)) => {
                            active = std::time::Instant::now();
                            if let Err(e) = self.chat_receipt_send(&core, &sess, &day, &mut writer).await { break 'run e.to_string(); }
                        }
                        Some(Cmd::Close) | None => break 'run "closed locally".into(),
                    },
                    _ = conn.closed() => break 'run "connection lost".into(),
                }
            }
        };
        self.log(format!("chat session with {} ended: {reason}", sess.did));
        {
            let mut map = core.sessions.lock();
            if map.get(&sess.device).is_some_and(|s| s.id == sess.id) {
                map.remove(&sess.device);
            }
        }
        sess.hist.lock().take();
        self.presence_update(&core, &sess.did);
        let _ = writer.0.finish();
        let c2 = conn.clone();
        tokio::spawn(async move {
            let _ = tokio::time::timeout(Duration::from_millis(300), c2.closed()).await;
            c2.close(0u32.into(), b"bye");
        });
        // Something left unsent: try again.
        if let Ok(me) = self.me() {
            let pair = pair_id(me.id.did(), &sess.did);
            if (self.profile_pending(&sess.did) || core.store.pending_days(&pair).map(|d| !d.is_empty()).unwrap_or(false)) && self.shared.lock().endpoint.is_some() {
                let this = self.clone();
                let did = sess.did.clone();
                self.handle.spawn(async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    this.chat_kick(did);
                });
            }
        }
    }
}
