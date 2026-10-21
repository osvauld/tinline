//! Opening, stopping, and binding the account-scoped group runtime.

use super::*;

impl Inner {
    // ---- lifecycle -------------------------------------------------------------------------

    /// The group runtime of the unlocked account, opened on first use.
    ///
    /// GL: a core of another session (an earlier unlock, another account) lets its store go
    /// before the new one opens, and while the runtime is parked (after stop / lock / a switch,
    /// until the next start) only the app's own calls open it, never a leftover task.
    pub(crate) fn group_core(self: &Arc<Self>) -> Result<Arc<GroupCore>, Error> {
        let epoch = self.shared.lock().epoch;
        // Held while opening, so two callers never open the store twice.
        let mut slot = self.group.core.lock();
        if let Some(c) = slot.clone()
            && c.epoch == epoch
            && !c.closed()
        {
            return Ok(c);
        }
        if let Some(old) = slot.take() {
            old.shut();
            if let Ok(mut ep) = old.ep.try_lock()
                && let Some(ep) = ep.take()
            {
                self.handle.spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(3), ep.close()).await;
                });
            }
        }
        if self.group.parked.load(Ordering::SeqCst) && !crate::node::user_call() {
            return Err(Error::NotStarted);
        }
        let me = self.me()?;
        let (dir, profile_dev) = {
            let s = self.shared.lock();
            let dir = s
                .acct
                .as_ref()
                .ok_or(Error::Locked)?
                .store
                .dir()
                .to_path_buf();
            let dev = match s.disk.as_ref() {
                Some(crate::store::Disk::V2(p)) => p.device_id_bytes(),
                _ => None,
            };
            (dir, dev)
        };
        // G2 stage 1 (P10 for groups): a group store made from nothing writes under a fresh
        // device ID, so a lost `group.redb` never makes us reuse a (writer ID, counter) that some
        // host may still hold. A store from G1 (no `dev/id` row yet) keeps the profile's ID.
        let path = dir.join("group.redb");
        let existed = path.exists();
        let store = crate::group::store::open(path, &me.profile.mnemonic)?;
        // A key staged by a rotation that never finished never becomes current (rotation.md).
        store.ep_discard_pending()?;
        let dev = store.device_id(if existed { profile_dev } else { None })?;
        let did = me.id.did().to_string();
        let wid = writer_id(&did, &dev);
        let mut groups = HashMap::new();
        for (gid, meta) in store.groups()? {
            let (snap, ups) = store.meta_rows(&gid)?;
            let doc = match GroupDoc::load(&gid, wid, snap.as_deref(), &ups) {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("group {gid}: {e}");
                    continue;
                }
            };
            // Our own sealed rows: verified when they entered the store, not again. Day docs are
            // not read here at all.
            let entries = doc.entries_trusted().unwrap_or_default();
            let state = derive_meta(&gid, &entries, &doc.vv());
            groups.insert(
                gid,
                Live {
                    doc,
                    entries,
                    state,
                    meta,
                    shards: Shards::default(),
                },
            );
        }
        let receipts = member_receipts::controller(&store, &did, dev);
        let core = Arc::new(GroupCore {
            epoch,
            store,
            account: me.id.clone(),
            did,
            dev,
            wid,
            groups: Mutex::new(groups),
            ep: tokio::sync::Mutex::new(None),
            ep_id: Mutex::new(None),
            sessions: Default::default(),
            dialing: Default::default(),
            backoff: Default::default(),
            refusals: Default::default(),
            my_addr: Default::default(),
            announced: Default::default(),
            next: AtomicU64::new(1),
            kick: Notify::new(),
            closed: AtomicBool::new(false),
            loops: AtomicBool::new(false),
            rotating: AtomicBool::new(false),
            announcing: Default::default(),
            compacted: Default::default(),
            compacting: Default::default(),
            codes: Default::default(),
            receipts,
        });
        *slot = Some(core.clone());
        // Receipt intents committed before a stop are signed now (bounded; see M3).
        core.member_receipts_pump_and_push();
        self.search_reconcile_groups(&core);
        Ok(core)
    }

    /// Stops everything of the group runtime (lock, stop, account switch). The store is closed
    /// before this returns (GL): tasks still holding the core find it closed and cannot keep the
    /// file, and none opens it again until the next start (`parked`).
    pub(crate) async fn group_close(&self) {
        let core = {
            let mut slot = self.group.core.lock();
            self.group.parked.store(true, Ordering::SeqCst);
            slot.take()
        };
        if let Some(core) = core {
            core.shut();
            let ep = core.ep.lock().await.take();
            *core.ep_id.lock() = None;
            if let Some(ep) = ep {
                let _ = tokio::time::timeout(Duration::from_secs(3), ep.close()).await;
            }
        }
    }

    /// `group_close` without waiting (for `Node`'s drop on a core thread): the store closes; the
    /// group endpoint is closed on a task.
    pub(crate) fn group_close_now(&self) {
        let core = {
            let mut slot = self.group.core.lock();
            self.group.parked.store(true, Ordering::SeqCst);
            slot.take()
        };
        if let Some(core) = core {
            core.shut();
        }
    }

    /// Called once the private endpoint is up: brings the group endpoint up if we have groups.
    pub(crate) fn group_started(self: &Arc<Self>) {
        let this = self.clone();
        self.handle.spawn(async move {
            let Ok(core) = this.group_core() else { return };
            if core.groups.lock().is_empty() {
                return;
            }
            if let Err(e) = this.group_ensure(&core).await {
                tracing::warn!("group endpoint: {e}");
            }
        });
    }

    /// Binds the group endpoint (once) and starts the accept and dial loops.
    pub(crate) async fn group_ensure(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
    ) -> Result<Endpoint, Error> {
        let ep = self.group_bind(core).await?;
        if !core.loops.swap(true, Ordering::SeqCst) {
            // A rotation's `Moved` campaign survives a restart (`ep/announce`).
            self.group_announce_moved(core, RESUME_ANNOUNCE_WAIT);
            let (this, c) = (self.clone(), core.clone());
            self.handle
                .spawn(async move { this.group_dial_loop(c).await });
        }
        Ok(ep)
    }

    /// Binds the group endpoint if it is not bound, with its accept loop (not the dial loop).
    pub(super) async fn group_bind(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
    ) -> Result<Endpoint, Error> {
        if self.shared.lock().endpoint.is_none() {
            return Err(Error::NotStarted);
        }
        let mut slot = core.ep.lock().await;
        if let Some(ep) = slot.as_ref() {
            return Ok(ep.clone());
        }
        if core.closed() {
            return Err(Error::Locked);
        }
        let secret = self.group_ep_state(core)?.secret;
        let ep = bind_group_endpoint(&secret).await?;
        *slot = Some(ep.clone());
        *core.ep_id.lock() = Some(ep.clone());
        drop(slot);
        self.log(format!("group endpoint {} bound", ep.id()));
        self.group_serve(core, &ep);
        Ok(ep)
    }

    /// Starts the accept loop of a freshly bound group endpoint.
    pub(super) fn group_serve(self: &Arc<Self>, core: &Arc<GroupCore>, ep: &Endpoint) {
        let (this, c, e) = (self.clone(), core.clone(), ep.clone());
        self.handle
            .spawn(async move { this.group_accept_loop(c, e).await });
    }

    /// Waits (up to 5 s) for the group endpoint to know its home relay, so the addresses we
    /// hand out carry it.
    pub(crate) async fn group_relay_ready(&self) {
        let Some(ep) = self.group_endpoint() else {
            return;
        };
        for _ in 0..50 {
            if relay_of(&ep).is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub(crate) fn group_endpoint(&self) -> Option<Endpoint> {
        self.group
            .core
            .lock()
            .as_ref()
            .and_then(|c| c.ep_id.lock().clone())
    }

    /// Our signed host address for `gid` (refreshed when the relay changes), if we host its chat
    /// or its calls.
    pub(crate) fn group_my_addr(&self, core: &GroupCore, gid: &str) -> Option<SignedHostAddr> {
        let hosting = core
            .groups
            .lock()
            .get(gid)
            .is_some_and(|l| hosts_or_calls(&l.state, &core.did, &core.dev));
        if !hosting {
            core.my_addr.lock().remove(gid);
            return None;
        }
        let ep = core.ep_id.lock().clone()?;
        let relay = relay_of(&ep);
        let mut cache = core.my_addr.lock();
        if let Some(s) = cache.get(gid)
            && s.open()
                .ok()
                .is_some_and(|a| a.relay == relay && a.endpoint == *ep.id().as_bytes())
        {
            return Some(s.clone());
        }
        let a = HostAddr {
            group: gid.to_string(),
            did: core.did.clone(),
            dev: core.dev,
            endpoint: *ep.id().as_bytes(),
            relay,
            at: now_ms(),
        };
        let a = proto::Signed::sign(&a, &core.account).ok()?;
        cache.insert(gid.to_string(), a.clone());
        Some(a)
    }

    pub(super) fn group_ev(&self) -> Option<Arc<dyn GroupEvents>> {
        self.group.events.lock().clone()
    }
}

/// A group endpoint under `secret` (not yet installed in the core).
pub(super) async fn bind_group_endpoint(secret: &[u8; 32]) -> Result<Endpoint, Error> {
    let transport = QuicTransportConfig::builder()
        .keep_alive_interval(Duration::from_secs(5))
        .max_idle_timeout(Some(
            Duration::from_secs(20).try_into().expect("small timeout"),
        ))
        .build();
    let mut builder = Endpoint::builder(presets::N0)
        .secret_key(SecretKey::from_bytes(secret))
        .alpns(vec![GROUP_ALPN.to_vec(), proto::gcall::GCALL_ALPN.to_vec()])
        .transport_config(transport);
    if std::env::var_os("P2P_RELAY_ONLY").is_some() {
        builder = builder.clear_ip_transports();
    }
    builder.bind().await.map_err(Error::net)
}
