//! Account and call document persistence and linked-device registry updates.

use super::*;

impl Inner {
    pub(super) fn ctx<'a>(me: &'a Me) -> Ctx<'a> {
        Ctx {
            my_did: me.id.did(),
            me_device: me.device,
            now_ms: crate::node::now_ms(),
        }
    }

    /// Wakes the sessions (local data changed) and the dial loop.
    pub(crate) fn sync_bump(&self) {
        self.sync_tx.send_modify(|v| *v += 1);
    }

    pub(crate) fn sync_kick(&self) {
        self.selfsync.backoff.lock().clear();
        self.selfsync.kick.notify_one();
    }

    /// Opens the account doc of the unlocked account (called wherever the chat store opens):
    /// loads the sealed snapshot, flushes the working copy into it, merges it back.
    pub(crate) fn acc_open(self: &Arc<Self>) {
        if let Err(e) = self.profile_open() {
            tracing::debug!("profile open: {e}");
        }
        let (me, backing) = {
            let s = self.shared.lock();
            let (Some(me), Some(_)) = (s.me.clone(), s.dek.as_ref()) else {
                return;
            };
            if s.acc.is_some() {
                return;
            }
            (me, s.acct.as_ref().and_then(|a| a.backing.clone()))
        };
        let snapshot = |key| match &backing {
            Some(Backing::Sealed(db)) => db.get(key).ok().flatten(),
            _ => None,
        };
        let doc = match AccDoc::load(&me.device, snapshot(DOC_KEY).as_deref()) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("account doc: {e}; starting a new one");
                AccDoc::new(&me.device)
            }
        };
        // The first open on a build with a separate call doc seeds it from the local call log.
        let calls_doc = match CallsDoc::load(&me.device, snapshot(CALLS_KEY).as_deref()) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("calls doc: {e}; starting a new one");
                CallsDoc::new(&me.device)
            }
        };
        let changed = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch || s.acc.is_some() {
                return;
            }
            let label = s.device_label.clone();
            ensure_self_registered(&mut s.state.registry, &me, label);
            let ctx = Self::ctx(&me);
            if let Err(e) = doc.write_from(&s.state, &ctx) {
                tracing::warn!("account doc flush: {e}");
            }
            let ch = doc.read_into(&mut s.state, &ctx);
            let mut calls = s.history.snapshot();
            if let Err(e) = calls_doc.write_from(&calls) {
                tracing::warn!("calls doc flush: {e}");
            }
            let history = calls_doc.read_into(&mut calls);
            if history {
                s.history.replace(calls);
            }
            s.acc = Some(Arc::new(doc));
            s.calls = Some(Arc::new(calls_doc));
            ch.contacts || history || ch.devices
        };
        self.save_doc();
        if changed {
            self.events.on_contacts_changed();
        }
    }

    pub(super) fn save_doc(&self) {
        let (acc, calls, backing) = {
            let s = self.shared.lock();
            (
                s.acc.clone(),
                s.calls.clone(),
                s.acct.as_ref().and_then(|a| a.backing.clone()),
            )
        };
        let Some(Backing::Sealed(db)) = backing else {
            return;
        };
        let docs = [
            (DOC_KEY, acc.as_deref().map(|d| &**d)),
            (CALLS_KEY, calls.as_deref().map(|d| &**d)),
        ];
        for (key, doc) in docs {
            if let Some(doc) = doc
                && let Err(e) = db.put(key, &doc.snapshot())
            {
                tracing::warn!("saving {key}: {e}");
            }
        }
    }

    /// A local write happened (state or call log): flush it into the doc and tell the sessions.
    pub(crate) fn sync_local(&self, epoch: Option<u64>) {
        let Ok(me) = self.me() else { return };
        let changed = {
            let s = self.shared.lock();
            if epoch.is_some_and(|e| e != s.epoch) || s.epoch != me.epoch {
                return;
            }
            let (Some(acc), Some(calls)) = (s.acc.clone(), s.calls.clone()) else {
                return;
            };
            let a = acc
                .write_from(&s.state, &Self::ctx(&me))
                .unwrap_or_else(|e| {
                    tracing::warn!("account doc flush: {e}");
                    false
                });
            let c = calls.write_from(&s.history.snapshot()).unwrap_or_else(|e| {
                tracing::warn!("calls doc flush: {e}");
                false
            });
            a || c
        };
        if changed {
            self.save_doc();
            self.sync_bump();
        }
    }

    /// Imports a batch from another own device and merges it into the working copy.
    pub(crate) fn apply_account_update(
        self: &Arc<Self>,
        epoch: u64,
        update: &[u8],
    ) -> Result<(), Error> {
        let me = self.me()?;
        let (ch, state_backing) = {
            let mut s = self.shared.lock();
            if s.epoch != epoch || me.epoch != epoch {
                return Err(Error::Locked);
            }
            let acc = s.acc.clone().ok_or(Error::Locked)?;
            let ctx = Self::ctx(&me);
            // Our own unwritten changes go in first, so they merge as concurrent edits.
            acc.write_from(&s.state, &ctx)?;
            if !acc.import(update)? {
                return Ok(());
            }
            // A contact entry deleted by a block notice keeps its calls and chat history (U20).
            let ch = acc.read_into(&mut s.state, &ctx);
            if let Err(e) = Self::profile_project(&mut s) {
                tracing::warn!("profile projection: {e}");
            }
            if let Some(l) = &ch.label {
                s.device_label = Some(l.clone());
            }
            (ch, s.acct.as_ref().and_then(|a| a.backing.clone()))
        };
        if let (Some(l), Some(Backing::Sealed(db))) = (&ch.label, &state_backing) {
            let _ = crate::store::save_label(db, l);
        }
        self.persist_for(Some(epoch))?;
        self.save_doc();
        if ch.contacts {
            self.events.on_contacts_changed();
            let dids = self
                .shared
                .lock()
                .state
                .contacts
                .iter()
                .map(|c| c.did.clone())
                .collect::<Vec<_>>();
            for did in dids {
                self.profile_download(did.clone());
                self.profile_for_new_contact(&did);
            }
        }
        if ch.devices {
            // U8: a sibling linked or unlinked a device; our list changes too.
            self.devices_changed();
        }
        if let Some(ev) = self.link_events()
            && ch.devices
        {
            ev.on_devices_changed();
        }
        self.sync_bump();
        if ch.self_removed
            && !self
                .selfsync
                .leaving
                .load(std::sync::atomic::Ordering::SeqCst)
        {
            let this = self.clone();
            self.handle.spawn(async move {
                this.remove_self_account(epoch).await;
            });
        }
        Ok(())
    }

    /// Imports a batch of the call log from another own device and merges it into ours.
    pub(crate) fn apply_calls_update(&self, epoch: u64, update: &[u8]) -> Result<(), Error> {
        let changed = {
            let s = self.shared.lock();
            if s.epoch != epoch {
                return Err(Error::Locked);
            }
            let calls = s.calls.clone().ok_or(Error::Locked)?;
            let mut mine = s.history.snapshot();
            calls.write_from(&mine)?;
            if !calls.import(update)? {
                return Ok(());
            }
            let changed = calls.read_into(&mut mine);
            if changed {
                s.history.replace(mine);
            }
            changed
        };
        self.save_doc();
        if changed {
            let _ = self.history().save();
            if let Some(ev) = self.link_events() {
                ev.on_history_changed();
            }
        }
        self.sync_bump();
        Ok(())
    }

    /// This device was unlinked: drop the account from it (best effort when a call is running;
    /// the next sync with a device that knows retries).
    pub(crate) async fn remove_self_account(self: Arc<Self>, epoch: u64) {
        let did = {
            let s = self.shared.lock();
            if s.epoch != epoch {
                return;
            }
            match s.me.as_ref() {
                Some(me) => me.id.did().to_string(),
                None => return,
            }
        };
        if self.not_in_call().is_err() {
            return;
        }
        let Ok(id) = crate::accounts::id_of(&did) else {
            return;
        };
        if self.replace_account(None).await.is_err() {
            return;
        }
        if let Err(e) = self.accounts.remove(&id) {
            tracing::warn!("removing unlinked account: {e}");
        }
        self.log("this device was unlinked; the account was removed");
        if let Some(ev) = self.link_events() {
            ev.on_unlinked(did);
        }
    }

    /// A valid attestation of our DID from a device the registry does not list: a device that
    /// was linked but never announced itself (or one added while we were away).
    pub(super) fn note_own_device(
        self: &Arc<Self>,
        epoch: u64,
        att: &proto::SignedAttestation,
        device: [u8; 32],
        relay: Option<String>,
    ) {
        let added = {
            let mut s = self.shared.lock();
            if s.epoch != epoch {
                return;
            }
            let t = now();
            match s.state.registry.iter_mut().find(|e| e.device == device) {
                Some(e) => {
                    e.last_seen = t;
                    if relay.is_some() && e.relay != relay {
                        e.relay = relay;
                    }
                    false
                }
                None => {
                    s.state.registry.push(OwnDevice {
                        device,
                        attestation: att.clone(),
                        label: String::new(),
                        removed: false,
                        last_seen: t,
                        relay,
                        links: Vec::new(),
                        rotated: false,
                    });
                    true
                }
            }
        };
        if added {
            let _ = self.persist_for(Some(epoch));
            self.devices_changed();
            if let Some(ev) = self.link_events() {
                ev.on_devices_changed();
            }
        }
    }
}
