//! Writing account state and replacing the account in memory.

use super::*;

/// An account's files, read before anything is unlocked.
pub(crate) struct Loaded {
    pub(super) id: String,
    pub(super) disk: Disk,
    pub(super) store: Store,
}

pub(crate) fn load_account(accounts: &AccountDirs, id: &str) -> Result<Loaded, Error> {
    // `Store::open` would create the directory of an account that is not there.
    if !accounts.exists(id)? {
        return Err(Error::NotFound);
    }
    let store = Store::open(accounts.dir(id)?)?;
    let disk = store.profile()?.ok_or(Error::NotFound)?;
    accounts::entry_of(id, &disk)?;
    Ok(Loaded { id: id.to_string(), disk, store })
}

impl Shared {
    pub(crate) fn new() -> Self {
        Shared {
            disk: None,
            me: None,
            dek: None,
            state: State::default(),
            endpoint: None,
            epoch: 1,
            acct: None,
            history: Arc::new(History::empty()),
            device_label: None,
            committed: true,
            acc: None,
            calls: None,
            profile: None,
            profile_cache: None,
        }
    }

    /// Puts the account's files in place, locked; a legacy profile is unlocked and brings its
    /// JSON state with it.
    pub(crate) fn install(&mut self, l: Loaded) -> Result<(), Error> {
        self.forget_session();
        self.epoch += 1;
        let mut backing = None;
        if let Disk::Legacy(p) = &l.disk {
            self.state = l.store.state()?;
            let b = Backing::Json(l.store.clone());
            self.history = Arc::new(History::new(l.store.calls()?, Some(b.clone())));
            self.me = Some(Arc::new(Me::load(p.clone(), self.epoch)?));
            backing = Some(b);
        }
        self.acct = Some(Acct { id: l.id, store: l.store, backing });
        self.disk = Some(l.disk);
        self.committed = true;
        Ok(())
    }

    /// Everything of the account that is secret or sealed leaves memory. An identity that was
    /// never committed has nowhere to come back from, so it goes entirely.
    pub(crate) fn forget_session(&mut self) {
        self.epoch += 1;
        self.me = None;
        self.dek = None;
        self.state = State::default();
        self.history = Arc::new(History::empty());
        self.device_label = None;
        self.acc = None;
        self.calls = None;
        self.profile = None;
        self.profile_cache = None;
        if let Some(a) = self.acct.as_mut()
            && let Some(b) = a.backing.take()
        {
            // GL: the file goes now, not when the last task holding a clone lets go (the next
            // unlock or the next node on this directory opens it at once). Writes still in
            // flight finish first; any later one fails instead of landing after the lock.
            if let Backing::Sealed(db) = b {
                db.close();
            }
        }
        if !self.committed {
            self.disk = None;
            self.acct = None;
            self.committed = true;
        }
    }
}

impl Inner {
    /// Writes a snapshot of `State` to the current account.
    pub(crate) fn persist(&self) -> Result<(), Error> {
        self.persist_for(None)
    }

    /// `persist` for something that belongs to session `epoch`: refused (`Locked`) if the account
    /// in memory has been replaced since. Callers must not hold `shared`.
    pub(crate) fn persist_for(&self, epoch: Option<u64>) -> Result<(), Error> {
        let _w = self.writing.lock();
        let (snapshot, backing) = {
            let s = self.shared.lock();
            if epoch.is_some_and(|g| g != s.epoch) {
                return Err(Error::Locked);
            }
            if s.disk.is_none() {
                return Err(Error::NoIdentity);
            }
            if !s.committed {
                return Ok(());
            }
            match s.acct.as_ref().and_then(|a| a.backing.clone()) {
                Some(b) => (s.state.clone(), b),
                None => return Err(Error::Locked),
            }
        };
        self.write_state(&backing, &snapshot)?;
        self.sync_local(epoch);
        Ok(())
    }

    pub(super) fn write_state(&self, backing: &Backing, state: &State) -> Result<(), Error> {
        if self.fail_writes.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok() {
            return Err(Error::Io("injected write failure".into()));
        }
        backing.save_state(state)
    }

    /// `persist_for` for async code: the write (and its fsyncs) runs on the blocking pool, not on
    /// a worker that other connections need.
    pub(crate) async fn persist_async(self: &Arc<Self>, epoch: u64) -> Result<(), Error> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.persist_for(Some(epoch)))
            .await
            .map_err(|_| Error::Io("persist task failed".into()))?
    }

    /// Fire and forget, for state that is only a cache (a device, a renewed grant).
    pub(crate) fn persist_in_background(self: &Arc<Self>, epoch: u64) {
        let this = self.clone();
        self.handle.spawn_blocking(move || {
            let _ = this.persist_for(Some(epoch));
        });
    }

    pub(crate) fn history(&self) -> Arc<History> {
        self.shared.lock().history.clone()
    }

    pub(crate) fn not_in_call(&self) -> Result<(), Error> {
        let live = self.live.lock();
        if live.call.is_some() || live.waiting.is_some() {
            return Err(Error::InCall);
        }
        Ok(())
    }

    /// Runs `f` while no 1:1 call is active and none can begin (a group call starting).
    pub(crate) fn with_no_call<R>(&self, f: impl FnOnce() -> Result<R, Error>) -> Result<R, Error> {
        let live = self.live.lock();
        if live.call.is_some() || live.waiting.is_some() {
            return Err(Error::InCall);
        }
        let r = f();
        drop(live);
        r
    }

    /// Writes a new account's files: the dir (never an existing one), `account.json`, the sealed
    /// store, then the pointer. What it created is removed again if any step fails.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_account_files(
        &self,
        id: &str,
        disk: &Disk,
        phrase: &str,
        dek: &Dek,
        state: &State,
        calls: &[CallRecord],
        label: Option<&str>,
    ) -> Result<Acct, Error> {
        let dir = self.accounts.create(id)?;
        let built = (|| {
            let store = Store::open(&dir)?;
            store.save_profile(disk)?;
            let db = store::seal_into(&store, phrase, dek, state, calls, label)?;
            self.accounts.select(id)?;
            Ok::<_, Error>(Acct { id: id.to_string(), store, backing: Some(Backing::Sealed(db)) })
        })();
        if built.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        built
    }

    /// Restore over an account dir that is already there (its owner forgot the passphrase): the
    /// sealed stores stay, `account.json` gets the new vault. A store that cannot be re-keyed
    /// (it predates the identity-derived keys and the data key is gone) is renamed
    /// `*.unreadable`, never deleted. Nothing of the old vault survives.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn restore_account_files(
        &self,
        id: &str,
        disk: &Disk,
        phrase: &str,
        dek: &Dek,
        state: &State,
        calls: &[CallRecord],
        label: Option<&str>,
    ) -> Result<Acct, Error> {
        let dir = self.accounts.dir(id)?;
        let store = Store::open(&dir)?;
        for name in ["account.redb", "chat.redb"] {
            let path = dir.join(name);
            if rekey::file_needs_migration(&path)? {
                rekey::quarantine(&path)?;
            }
        }
        let db = store::seal_into(&store, phrase, dek, state, calls, label)?;
        store.save_profile(disk)?;
        self.accounts.select(id)?;
        Ok(Acct { id: id.to_string(), store, backing: Some(Backing::Sealed(db)) })
    }

    /// `commit_identity` with the gate held.
    pub(crate) fn commit_locked(self: &Arc<Self>) -> Result<(), Error> {
        let (disk, dek, phrase, state, calls, label) = {
            let s = self.shared.lock();
            let disk = s.disk.clone().ok_or(Error::NoIdentity)?;
            if s.committed {
                return Ok(());
            }
            let phrase = zeroize::Zeroizing::new(s.me.as_ref().ok_or(Error::Locked)?.profile.mnemonic.clone());
            (disk, s.dek.clone().ok_or(Error::Locked)?, phrase, s.state.clone(), s.history.snapshot(), s.device_label.clone())
        };
        let Disk::V2(p) = &disk else { return Err(Error::NoIdentity) };
        let id = accounts::id_of(&p.did)?;
        let acct = if self.accounts.exists(&id)? {
            // A restore over an existing account of this device.
            self.restore_account_files(&id, &disk, &phrase, &dek, &state, &calls, label.as_deref())?
        } else {
            self.create_account_files(&id, &disk, &phrase, &dek, &state, &calls, label.as_deref())?
        };
        {
            let mut s = self.shared.lock();
            if let Some(b) = &acct.backing {
                s.history.attach(b.clone());
            }
            s.acct = Some(acct);
            s.committed = true;
        }
        self.chat_open();
        self.acc_open();
        // Anything that changed while the files were being written.
        self.persist()
    }

    /// Switches (or, with `None`, deselects) the account in memory. Stops everything of the old one.
    pub(crate) async fn replace_account(self: &Arc<Self>, target: Option<Loaded>) -> Result<(), Error> {
        let _life = self.lifecycle.lock().await;
        self.not_in_call()?;
        match &target {
            Some(l) => self.accounts.select(&l.id)?,
            None => self.accounts.deselect()?,
        }
        self.chat_sessions_close();
        // GA-M4: the group call ends like on stop / lock (Leave as a member, End{host_leaving,
        // next_host} as its host), before the endpoints close so those frames still go out.
        self.gcall_shutdown().await;
        self.close_endpoint().await;
        self.chat_close().await;
        {
            let mut s = self.shared.lock();
            s.forget_session();
            s.disk = None;
            s.acct = None;
            s.committed = true;
            if let Some(l) = target {
                s.install(l)?;
            }
        }
        self.emit_status();
        self.events.on_contacts_changed();
        Ok(())
    }

    /// The setting as of now: an expired "until" counts as available again.
    pub(crate) fn availability(&self) -> Availability {
        let a = self.shared.lock().state.availability.clone();
        match a.until {
            Some(t) if a.unavailable && now() >= t => Availability { available: true, until: None },
            _ if a.unavailable => Availability { available: false, until: a.until },
            _ => Availability { available: true, until: None },
        }
    }

    /// Adds to the call log of session `epoch` and writes it in the background; dropped if that
    /// session is over.
    pub(crate) fn log_call(&self, epoch: u64, rec: CallRecord) {
        let history = {
            let s = self.shared.lock();
            if s.epoch != epoch {
                return;
            }
            s.history.clone()
        };
        history.push(rec);
        self.sync_local(Some(epoch));
        self.handle.spawn_blocking(move || {
            if let Err(e) = history.save() {
                tracing::warn!("saving call history: {e}");
            }
        });
    }

    /// An incoming call that never became a `Call` (turned away unavailable, or busy).
    pub(crate) fn log_refused(&self, epoch: u64, call_id: &str, did: &str, name: &str, reason: &str, missed: bool) {
        self.log_call(epoch, CallRecord {
            call_id: call_id.to_string(),
            peer_did: did.to_string(),
            peer_name: name.to_string(),
            incoming: true,
            started_at: now(),
            duration_secs: 0,
            reason: reason.to_string(),
            direct: false,
            missed,
        });
    }
}
