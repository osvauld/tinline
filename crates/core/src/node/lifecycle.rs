//! Start/stop of the endpoint, tickets and relay helpers.

use super::*;

#[uniffi::export]
impl Node {
    /// Binds the endpoint and starts accepting calls. Idempotent.
    pub fn start(&self) -> Result<(), Error> {
        let inner = self.inner.clone();
        self.block_on(inner.start())?
    }

    pub fn stop(&self) {
        let inner = self.inner.clone();
        self.run_lifecycle(async move {
            {
                let _life = inner.lifecycle.lock().await;
                inner.chat_sessions_close();
                inner.gcall_shutdown().await;
                inner.close_endpoint().await;
            }
            inner.emit_status();
        });
    }

    /// The platform saw the network change (wifi ↔ cellular); re-probe paths now rather than
    /// waiting for the next timeout.
    pub fn network_changed(&self) {
        let ep = self.inner.shared.lock().endpoint.clone();
        if let Some(ep) = ep {
            self.inner.handle.spawn(async move { ep.network_change().await });
            self.inner.chat_kick_pending();
            self.inner.sync_kick();
        }
    }

    pub fn status(&self) -> NodeStatus {
        self.inner.status()
    }

    /// Our contact ticket (`OSVC2:…`). Reusable: any number of people can add us with it until
    /// it expires (7 days) or `reset_ticket`; each addition is a request we accept or not.
    /// Replaced (the old one stays good until it expires) on its last day, after a rename and
    /// when our relay changes.
    pub fn my_ticket(&self) -> Result<String, Error> {
        let inner = self.inner.clone();
        self.block_on(async move { inner.ticket().await })?
    }
}

impl Inner {
    pub(crate) async fn close_endpoint(&self) {
        // G1: the group endpoint goes down with the private one (stop, lock, account switch).
        self.group_close().await;
        let ep = self.shared.lock().endpoint.take();
        if let Some(ep) = ep {
            let gate = self.closing.clone().lock_owned().await;
            let _ = tokio::time::timeout(close_wait(), self.close_fully(ep, gate)).await;
        }
    }

    /// Closes `ep` to the end on a task of its own, holding `gate` (a guard of `closing`) until
    /// then; the returned future waits for that. iroh refuses new connections from the first
    /// step of a close but leaves the relay connections up until the open connections have
    /// drained (about 3 s at worst). A close abandoned half way would leave this device key
    /// registered at its home relay, refusing every dial that arrives there, for as long as
    /// anything holds the endpoint: after a restart on another home relay, a contact dialling the
    /// old one would never ring us (P11).
    pub(crate) fn close_fully(
        &self,
        ep: Endpoint,
        gate: tokio::sync::OwnedMutexGuard<()>,
    ) -> impl std::future::Future<Output = ()> + Send + 'static {
        let task = self.handle.spawn(async move {
            ep.close().await;
            drop(gate);
        });
        async move {
            let _ = task.await;
        }
    }

    pub(crate) fn me(&self) -> Result<Arc<Me>, Error> {
        let s = self.shared.lock();
        match (&s.me, &s.disk) {
            (Some(me), _) => Ok(me.clone()),
            (None, Some(_)) => Err(Error::Locked),
            (None, None) => Err(Error::NoIdentity),
        }
    }

    pub(crate) fn endpoint(&self) -> Result<Endpoint, Error> {
        self.shared.lock().endpoint.clone().ok_or(Error::NotStarted)
    }

    pub(crate) fn log(&self, line: impl Into<String>) {
        let line = line.into();
        tracing::info!("{line}");
        self.events.on_log(line);
    }

    pub(crate) fn status(&self) -> NodeStatus {
        let s = self.shared.lock();
        match &s.endpoint {
            Some(ep) => {
                let relay = ep.addr().relay_urls().next().map(|u| u.to_string());
                NodeStatus {
                    started: true,
                    online: relay.is_some(),
                    relay,
                    endpoint_id: ep.id().to_string(),
                }
            }
            None => NodeStatus {
                started: false,
                online: false,
                relay: None,
                endpoint_id: match (&s.me, &s.disk) {
                    (Some(m), _) => m.info().device,
                    (None, Some(Disk::V2(p))) => {
                        PublicKey::from_bytes(&p.device_public).map(|k| k.to_string()).unwrap_or_default()
                    }
                    _ => String::new(),
                },
            },
        }
    }

    pub(crate) fn emit_status(&self) {
        self.events.on_status(self.status());
    }

    pub(crate) async fn start(self: Arc<Self>) -> Result<(), Error> {
        let _life = self.lifecycle.lock().await;
        let me = self.me()?;
        if self.shared.lock().endpoint.is_some() {
            return Ok(());
        }
        // The previous endpoint (or the link endpoint, which has the same key) finishes closing
        // first. Bounded: iroh's close is, but never let a stuck one keep us offline.
        if tokio::time::timeout(CLOSING_MAX, self.closing.lock()).await.is_err() {
            self.log("previous endpoint still closing; starting anyway");
        }
        let transport = QuicTransportConfig::builder()
            // Keeps a ringing call and NAT bindings alive; the relay link has its own pings.
            .keep_alive_interval(Duration::from_secs(5))
            .max_idle_timeout(Some(Duration::from_secs(20).try_into().expect("small timeout")))
            // Only call video uses uni streams (one per group, `video::net`): bounds what a peer
            // can open at once. `VideoRecv` lowers it per connection, but can't take back credit
            // granted at the handshake.
            .max_concurrent_uni_streams(16u32.into())
            .build();
        let mut alpns = vec![
            proto::ALPN.to_vec(),
            crate::chat::wire::CHAT_ALPN.to_vec(),
            iroh_blobs::ALPN.to_vec(),
            proto::SELF_ALPN.to_vec(),
            proto::link::LINK_ALPN.to_vec(),
        ];
        // Superseded versions (P9c): listed only so an old app is refused with a clear reason.
        alpns.extend(crate::version::old_alpns());
        let mut builder = Endpoint::builder(presets::N0)
            .secret_key(SecretKey::from_bytes(&me.profile.device_secret))
            .alpns(alpns)
            .transport_config(transport);
        // Test knob: no UDP of our own, so every packet goes through the relay — the path a
        // call takes when hole punching fails.
        if std::env::var_os("P2P_RELAY_ONLY").is_some() {
            builder = builder.clear_ip_transports();
            self.log("relay-only mode");
        }
        let ep = builder.bind().await.map_err(Error::net)?;
        // Locked meanwhile (cannot happen under the lifecycle lock, but an endpoint must never
        // outlive the secrets it was made from).
        if self.me().is_err() {
            ep.close().await;
            return Err(Error::Locked);
        }
        // Decide under the lock, await outside it (a guard across an await isn't Send).
        let lost_race = {
            let mut s = self.shared.lock();
            let taken = s.endpoint.is_some();
            if !taken {
                s.endpoint = Some(ep.clone());
            }
            taken
        };
        if lost_race {
            ep.close().await;
            return Ok(());
        }
        self.log(format!("endpoint {} bound", ep.id()));
        self.emit_status();
        {
            // GL: the group runtime may open its store from background tasks again.
            let _slot = self.group.core.lock();
            self.group.parked.store(false, Ordering::SeqCst);
        }
        self.chat_started();
        self.selfsync_started();
        self.notices_started();
        self.welcomes_started();
        self.group_started();
        self.tokens_started();
        // After a restore (new device key) or a change made while we were offline: tell contacts.
        self.devices_changed();
        self.lists_started();
        self.group_invites_started();
        self.sync_kick();

        let this = self.clone();
        let watch_ep = ep.clone();
        self.handle.spawn(async move {
            let mut addrs = watch_ep.watch_addr().stream();
            use n0_future::StreamExt;
            while let Some(_addr) = addrs.next().await {
                this.emit_status();
            }
        });

        let this = self.clone();
        self.handle.spawn(async move {
            let id = ep.id();
            while let Some(incoming) = ep.accept().await {
                // Bounded: a flood of strangers gets refused instead of costing a task each.
                // The reserved slots are for contacts' devices, which we can only tell apart
                // after the handshake.
                let (permit, reserved) = match this.pending.clone().try_acquire_owned() {
                    Ok(p) => (p, false),
                    Err(_) => match this.reserved.clone().try_acquire_owned() {
                        Ok(p) => (p, true),
                        Err(_) => {
                            incoming.refuse();
                            continue;
                        }
                    },
                };
                let this = this.clone();
                tokio::spawn(async move {
                    let mut accepting = match incoming.accept() {
                        Ok(a) => a,
                        Err(_) => return,
                    };
                    let alpn = match accepting.alpn().await {
                        Ok(a) => a,
                        Err(_) => return,
                    };
                    // An app that speaks a superseded version (P9c): closed at once with
                    // `VERSION_MISMATCH` and a reason naming both versions.
                    if let Some(v) = crate::version::superseded(&alpn) {
                        drop(permit);
                        this.log(format!("refused {} from an older app ({})", v.name, String::from_utf8_lossy(&alpn)));
                        if let Err(e) = crate::version::refuse(accepting, v, &alpn).await {
                            tracing::debug!("refusing an old version: {e}");
                        }
                        return;
                    }
                    if alpn.as_slice() == proto::SELF_ALPN {
                        drop(permit);
                        if let Err(e) = this.clone().handle_self_incoming(accepting).await {
                            tracing::debug!("incoming self-sync: {e}");
                        }
                        return;
                    }
                    if alpn.as_slice() == proto::link::LINK_ALPN {
                        drop(permit);
                        if let Err(e) = this.clone().handle_link_incoming(accepting).await {
                            tracing::debug!("incoming link: {e}");
                        }
                        return;
                    }
                    if alpn.as_slice() == crate::chat::wire::CHAT_ALPN || alpn.as_slice() == iroh_blobs::ALPN {
                        let chat = alpn.as_slice() == crate::chat::wire::CHAT_ALPN;
                        if let Err(e) = this.clone().handle_chat_incoming(accepting, chat, permit).await {
                            tracing::debug!("incoming chat: {e}");
                        }
                        return;
                    }
                    if let Err(e) = this.clone().handle_incoming(accepting, permit, reserved).await {
                        tracing::debug!("incoming: {e}");
                    }
                });
            }
            this.log(format!("endpoint {id} closed"));
        });
        Ok(())
    }

    pub(crate) async fn ticket(self: &Arc<Self>) -> Result<String, Error> {
        let me = self.me()?;
        // Right after start the relay isn't known yet; a ticket without it leaves the joiner
        // to DNS discovery, so give the relay a few seconds to come up first.
        let relay = match self.endpoint() {
            Ok(ep) => {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
                loop {
                    let r = relay_of(&ep);
                    if r.is_some() || tokio::time::Instant::now() > deadline {
                        break r;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            Err(_) => None,
        };
        let text = {
            let mut s = self.shared.lock();
            let now = now();
            if let Some(text) = &s.state.ticket
                && let Ok(t) = ContactTicket::from_text(text)
                && let Ok(claim) = t.verify(now)
                // Still ours (identity and device key unchanged: a restore makes a new key and a
                // joiner dials the one named here), still days of life left, and points at our
                // current relay so the joiner can dial without discovery.
                && claim.iss == me.id.did()
                && claim.device == me.device
                && claim.name == me.profile.name
                && claim.exp > now + 24 * 3600
                && (relay.is_none() || claim.relay == relay)
            {
                return Ok(text.clone());
            }
            // The one it replaces was handed out and stays good until it expires.
            if s.state
                .ticket
                .as_deref()
                .and_then(|t| ContactTicket::from_text(t).ok())
                .and_then(|t| t.verify(now).ok())
                .is_some_and(|c| c.iss == me.id.did() && c.device == me.device)
            {
                retire_ticket(&mut s.state, true);
            }
            s.state.live_tickets.retain(|_, exp| *exp > now);
            let ticket = proto::issue_contact_ticket(
                &me.id,
                me.device,
                &me.profile.name,
                relay,
                now,
                TICKET_TTL,
            );
            let text = ticket.to_text();
            if let Ok(c) = ticket.verify(now) {
                s.state.live_tickets.insert(c.nonce, c.exp);
            }
            s.state.ticket = Some(text.clone());
            text
        };
        self.persist_async(me.epoch).await?;
        Ok(text)
    }

    /// A one-time contact ticket of this device for `did` only (accepting a group chat request,
    /// U3): redeemable once, by that DID, with message-only tokens, until `ttl` seconds from now.
    pub(crate) async fn request_ticket(self: &Arc<Self>, did: &str, ttl: u64) -> Result<String, Error> {
        let me = self.me()?;
        let ep = self.endpoint()?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        let relay = loop {
            let r = relay_of(&ep);
            if r.is_some() || tokio::time::Instant::now() > deadline {
                break r;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let now = now();
        let ticket = proto::issue_contact_ticket(&me.id, me.device, &me.profile.name, relay, now, ttl);
        let nonce = ticket.verify(now)?.nonce;
        {
            let mut s = self.shared.lock();
            s.state.req_tickets.retain(|_, t| t.exp >= now);
            s.state.req_tickets.insert(nonce, crate::store::ReqTicket { did: did.to_string(), exp: now + ttl });
        }
        self.persist_async(me.epoch).await?;
        Ok(ticket.to_text())
    }
}

pub(crate) fn relay_of(ep: &Endpoint) -> Option<String> {
    ep.addr().relay_urls().next().map(|u| u.to_string())
}

/// `relay_of`, waiting up to `max` for a just-started endpoint to pick its relay: a code shown
/// without one leaves the scanner to DNS discovery. Blocks; callers are off the UI thread.
pub(crate) fn relay_within(ep: &Endpoint, max: Duration) -> Option<String> {
    let end = std::time::Instant::now() + max;
    loop {
        let r = relay_of(ep);
        if r.is_some() || std::time::Instant::now() > end {
            return r;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub(crate) fn addr_for(device: &[u8; 32], relay: Option<&str>) -> Result<EndpointAddr, Error> {
    let id = PublicKey::from_bytes(device).map_err(|_| Error::Protocol("bad device key".into()))?;
    let mut addr = EndpointAddr::new(id);
    if let Some(url) = relay.and_then(|r| r.parse::<RelayUrl>().ok()) {
        addr = addr.with_relay_url(url);
    }
    Ok(addr)
}

/// An unsigned relay hint from a peer, kept only if it is a sane relay URL: it gets stored
/// and dialled, so junk must not get in.
pub(crate) fn relay_hint(hint: &Option<String>) -> Option<String> {
    hint.as_deref()
        .filter(|h| h.len() <= 200 && h.starts_with("https://"))
        .and_then(|h| h.parse::<RelayUrl>().ok())
        .map(|u| u.to_string())
}
