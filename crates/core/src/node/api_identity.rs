//! `Node` API: accounts, identity, passphrase and lock state (UniFFI-exported).

use super::*;

#[uniffi::export]
impl Node {
    #[uniffi::constructor]
    pub fn new(data_dir: String, events: Arc<dyn NodeEvents>) -> Result<Arc<Self>, Error> {
        crate::logging::init();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name(CORE_THREAD)
            .enable_all()
            .build()?;
        let hub = Hub::new(rt.handle().clone(), data_dir, events)?;
        Ok(Arc::new(Self { hub, rt: Some(rt) }))
    }

    pub fn has_identity(&self) -> bool {
        self.open().shared.lock().disk.is_some()
    }

    /// Whether the identity is on disk. False only for a no-passphrase identity that has not
    /// been `commit_identity`ed yet (and with no identity at all).
    pub fn identity_committed(&self) -> bool {
        let slot = self.open();
        let s = slot.shared.lock();
        s.disk.is_some() && s.committed
    }

    /// Makes a no-passphrase identity durable: writes `account.json` and selects the account.
    /// The platform calls it once its keystore has saved `unlock_key()`; until then nothing is
    /// on disk, so a failed keystore write (or a crash) leaves no account whose key is lost, and
    /// `lock` or a restart simply forgets the identity. No-op once committed, and for an
    /// identity made with a passphrase (written at once).
    pub fn commit_identity(&self) -> Result<(), Error> {
        let slot = self.open();
        let _gate = slot.gate.lock();
        slot.commit_locked()
    }

    /// Every account on this device. Needs no unlock: only the clear `account.json` is read.
    pub fn accounts(&self) -> Vec<AccountSummary> {
        let slot = self.open();
        let listed = slot.accounts.list().unwrap_or_else(|e| {
            tracing::warn!("listing accounts: {e}");
            Vec::new()
        });
        let (current, uncommitted) = {
            let s = slot.shared.lock();
            let uncommitted = match (&s.disk, s.committed) {
                (Some(Disk::V2(p)), false) => Some((p.did.clone(), p.name.clone())),
                _ => None,
            };
            (s.acct.as_ref().map(|a| a.id.clone()), uncommitted)
        };
        let mut out: Vec<AccountSummary> = listed
            .into_iter()
            .map(|e| {
                let current = current.as_deref() == Some(e.id.as_str());
                AccountSummary {
                    current,
                    unread: self.hub.unread(&e.id, current),
                    did: e.did,
                    name: e.name,
                    has_passphrase: e.has_passphrase,
                }
            })
            .collect();
        if let Some((did, name)) = uncommitted {
            out.push(AccountSummary { did, name, current: true, has_passphrase: false, unread: 0 });
        }
        out
    }

    /// Makes `did` the current account. Under `OpenOnly` (the default) as always: stops the
    /// endpoint, locks and forgets the one in memory; unlock it afterwards (`unlock` or
    /// `unlock_with_key`). Otherwise (`docs/multi-online.md`): an account running in the
    /// background opens as it is (unlocked, no endpoint restart); any other is loaded locked as
    /// above; the account left keeps running in the background (`OnWake`: until it has been
    /// quiet for a minute; `KeepAll`: for good). `InCall` during a 1:1 call; a group call is
    /// left (or, hosted here, handed to its backup) as on `lock`.
    pub fn switch_account(&self, did: String) -> Result<(), Error> {
        let slot = self.open();
        let id = accounts::id_of(&did)?;
        slot.not_in_call()?;
        let here = slot.shared.lock().acct.as_ref().is_some_and(|a| a.id == id);
        if self.hub.mode() == BackgroundMode::OpenOnly || here {
            let loaded = load_account(&slot.accounts, &id)?;
            // The switcher's dot for the account that goes to sleep.
            self.hub.save_unread_leaving(&slot);
            self.run(|inner| async move { inner.ctx.replace_account(Some(loaded)).await })?;
            self.hub.write_unread(&id, 0);
            self.hub.accounts_changed();
            return Ok(());
        }
        let closing = self.hub.switch_to(&id)?;
        self.close_slots(closing);
        Ok(())
    }

    /// Leaves no account selected, so `has_identity` is false and `create_identity` /
    /// `restore_identity` add a new one. The accounts on disk are untouched; outside `OpenOnly`
    /// the one left keeps running in the background as after `switch_account`. `InCall` during
    /// a 1:1 call; a group call is left as on `switch_account`.
    pub fn begin_new_account(&self) -> Result<(), Error> {
        let slot = self.open();
        slot.not_in_call()?;
        if self.hub.mode() == BackgroundMode::OpenOnly {
            self.hub.save_unread_leaving(&slot);
            return self.run(|inner| async move { inner.ctx.replace_account(None).await });
        }
        let closing = self.hub.open_blank()?;
        self.close_slots(closing);
        Ok(())
    }

    /// Deletes a non-current account and everything in its directory (a background one is
    /// closed first).
    pub fn remove_account(&self, did: String) -> Result<(), Error> {
        let slot = self.open();
        let id = accounts::id_of(&did)?;
        if let Some(woken) = self.hub.take_woken_of(&id) {
            self.close_slots(Some(woken));
        }
        self.hub.forget_hint_key(&id);
        let _life = self.lifecycle()?;
        let current = slot.shared.lock().acct.as_ref().map(|a| a.id.clone());
        if current.as_deref() == Some(id.as_str()) || slot.accounts.current().as_deref() == Some(id.as_str()) {
            return Err(Error::Protocol("the current account cannot be removed".into()));
        }
        Ok(slot.accounts.remove(&id)?)
    }

    /// Names this installation ("Personal phone"), 1-64 characters, no control characters.
    /// Sealed with the account: needs the node unlocked.
    pub fn set_device_label(&self, label: String) -> Result<(), Error> {
        let slot = self.open();
        let label = label.trim().to_string();
        if label.is_empty() || label.chars().count() > 64 || label.chars().any(char::is_control) {
            return Err(Error::Protocol("a device name is 1-64 characters without control characters".into()));
        }
        let me = slot.me()?;
        let db = {
            let s = slot.shared.lock();
            // None: not committed yet, kept in memory and written by the commit.
            s.acct.as_ref().and_then(|a| a.backing.clone()).map(|Backing::Sealed(db)| db)
        };
        if let Some(db) = db {
            store::save_label(&db, &label)?;
        }
        // Our entry in the registry carries the name to the other devices.
        let registered = {
            let mut s = slot.shared.lock();
            s.device_label = Some(label.clone());
            match s.state.registry.iter_mut().find(|e| e.device == me.device) {
                Some(e) if e.label != label => {
                    e.label = label;
                    true
                }
                _ => false,
            }
        };
        if registered {
            slot.persist_for(Some(me.epoch))?;
        }
        Ok(())
    }

    /// The label given to `set_device_label`; `None` until set, and while locked.
    pub fn device_label(&self) -> Option<String> {
        self.open().shared.lock().device_label.clone()
    }

    pub fn lock_state(&self) -> LockState {
        let slot = self.open();
        let s = slot.shared.lock();
        match (&s.disk, &s.me) {
            (None, _) => LockState::NoIdentity,
            (Some(Disk::V2(_)), Some(_)) => LockState::Unlocked,
            (Some(Disk::V2(_)), None) => LockState::Locked,
        }
    }

    /// Whether a passphrase wraps the vault's data key. False for a vault the platform alone
    /// opens (see `unlock_key`).
    pub fn has_passphrase(&self) -> bool {
        matches!(&self.open().shared.lock().disk, Some(Disk::V2(p)) if p.vault.has_passphrase())
    }

    /// Creates an identity and leaves the node unlocked. An empty `passphrase` means none: the
    /// data key is then only in the platform's hands (`unlock_key`). Returns the recovery
    /// phrase; showing it again later needs the passphrase if there is one (`recovery_phrase`).
    /// Slow with a passphrase: runs Argon2id.
    pub fn create_identity(&self, name: String, passphrase: String) -> Result<String, Error> {
        let (_, mnemonic) = identity::generate();
        let phrase = mnemonic.to_string();
        self.set_identity(phrase.clone(), name, passphrase, false)?;
        Ok(phrase)
    }

    /// Like `create_identity`, from a recovery phrase.
    pub fn restore_identity(&self, phrase: String, name: String, passphrase: String) -> Result<(), Error> {
        self.set_identity(phrase.trim().to_string(), name, passphrase, true)
    }

    /// Name, DID and device key; available while locked too (they are stored in the clear).
    pub fn profile(&self) -> Option<ProfileInfo> {
        let slot = self.open();
        let s = slot.shared.lock();
        match (&s.disk, &s.me) {
            (_, Some(me)) => Some(me.info()),
            (Some(Disk::V2(p)), None) => Some(ProfileInfo {
                did: p.did.clone(),
                name: p.name.clone(),
                device: PublicKey::from_bytes(&p.device_public).map(|k| k.to_string()).unwrap_or_default(),
            }),
            _ => None,
        }
    }

    /// Unlocks with the passphrase; `WrongPassphrase` if it does not open the vault. Returns
    /// at once if already unlocked (or a legacy profile); a vault without a passphrase only
    /// opens with `unlock_with_key`. Slow: runs Argon2id.
    pub fn unlock(&self, passphrase: String) -> Result<(), Error> {
        // The slot asked is the slot unlocked, whatever opens meanwhile.
        let slot = self.open();
        let vault = match slot.unlock_target()? {
            Some(v) => v,
            None => return Ok(()),
        };
        let (secrets, dek) = self.kdf(move || vault::open(&vault, &passphrase))??;
        slot.finish_unlock(secrets, dek)?;
        self.hub.unlocked(&slot);
        Ok(())
    }

    /// Unlocks with a data key the platform remembered (from `unlock_key`); fast.
    /// `WrongPassphrase` if it does not open the vault: the platform should then forget it.
    pub fn unlock_with_key(&self, key: Vec<u8>) -> Result<(), Error> {
        let slot = self.open();
        slot.unlock_with_key(key)?;
        // Unlocked: other accounts may run again (wakes, `KeepAll` at the next `start`).
        self.hub.unlocked(&slot);
        Ok(())
    }

    /// The vault's data key while unlocked via a vault, for the platform to remember (wrapped
    /// by its own hardware key). `None` when locked. Stays valid across
    /// `set_passphrase`.
    pub fn unlock_key(&self) -> Option<Vec<u8>> {
        self.open().shared.lock().dek.as_ref().map(|d| d.to_vec())
    }

    /// Sets or changes the passphrase (any non-empty one). On a vault without a passphrase it adds
    /// one (the node must be unlocked); otherwise `old` is required and verified. The data key
    /// is kept, so a remembered `unlock_key` stays valid. Slow: runs Argon2id once or twice.
    pub fn set_passphrase(&self, old: Option<String>, new: String) -> Result<(), Error> {
        let slot = self.open();
        let _life = self.lifecycle()?;
        let snapshot = slot.shared.lock().disk.clone();
        match snapshot.ok_or(Error::NoIdentity)? {
            Disk::V2(p) => {
                vault::check_passphrase(&new)?;
                let vault = p.vault;
                let rewrapped = if vault.has_passphrase() {
                    let old = old.ok_or_else(|| Error::Protocol("the old passphrase is required".into()))?;
                    self.kdf(move || {
                        let (_secrets, dek) = vault::open(&vault, &old)?;
                        vault::rewrap(&vault, &dek, &new)
                    })??
                } else {
                    if old.is_some() {
                        return Err(Error::Protocol("no passphrase is set yet".into()));
                    }
                    // Wrapping needs the data key, which only an unlocked node holds.
                    let dek = slot.shared.lock().dek.clone().ok_or(Error::Locked)?;
                    self.kdf(move || vault::rewrap(&vault, &dek, &new))??
                };
                let mut s = slot.shared.lock();
                // Only the vault changes; keep any name change made meanwhile.
                let Some(Disk::V2(cur)) = s.disk.clone() else { return Err(Error::NoIdentity) };
                let disk = Disk::V2(ProfileV2 { vault: rewrapped, ..cur });
                if !s.committed {
                    // A passphrase makes the data key recoverable, so the identity can be written now.
                    s.disk = Some(disk);
                    drop(s);
                    let _gate = slot.gate.lock();
                    return slot.commit_locked();
                }
                s.acct.as_ref().ok_or(Error::NoIdentity)?.store.save_profile(&disk)?;
                s.disk = Some(disk);
                Ok(())
            }
        }
    }

    /// The recovery phrase, re-derived from the vault: `WrongPassphrase` unless `passphrase`
    /// is right.
    /// Works while locked. Slow: runs Argon2id. With no passphrase set, `passphrase` is
    /// ignored and the node must be unlocked: the platform asks for device authentication
    /// before calling this.
    pub fn recovery_phrase(&self, passphrase: String) -> Result<String, Error> {
        let slot = self.open();
        let vault = {
            let s = slot.shared.lock();
            match &s.disk {
                None => return Err(Error::NoIdentity),
                Some(Disk::V2(p)) if !p.vault.has_passphrase() => {
                    return s.me.as_ref().map(|me| me.profile.mnemonic.clone()).ok_or(Error::Locked);
                }
                Some(Disk::V2(p)) => p.vault.clone(),
            }
        };
        let (secrets, _dek) = self.kdf(move || vault::open(&vault, &passphrase))??;
        Ok(secrets.mnemonic.clone())
    }

    /// Stops the endpoint and forgets the secrets in memory; the node is `Locked` until
    /// `unlock`. No-op without an identity.
    pub fn lock(&self) {
        // Every account running in the background closes too, and the hint keys go; none opens
        // again (no wake, `start` or switch) until the open account is unlocked.
        self.hub.set_locked();
        let background = self.hub.take_woken();
        self.hub.forget_hint_keys();
        if !background.is_empty() {
            self.close_slots(background);
            self.hub.accounts_changed();
        }
        let slot = self.open();
        if !matches!(slot.shared.lock().disk, Some(Disk::V2(_))) {
            return;
        }
        slot.sync.link_cancel();
        let inner = slot.clone();
        self.run_lifecycle(async move {
            {
                // Under the lifecycle lock, so a `start` that is binding right now finishes
                // first and its endpoint is closed here.
                let _life = inner.lifecycle.lock().await;
                inner.group.gcall_shutdown().await;
                inner.close_endpoint().await;
                inner.shared.lock().forget_session();
                inner.chat.chat_close().await;
            }
            inner.emit_status();
        });
    }

    pub fn set_name(&self, name: String) -> Result<(), Error> {
        let slot = self.open();
        if name.len() > 256 || name.chars().any(|c| !proto::safe_name_char(c)) { return Err(Error::Protocol("invalid profile name".into())); }
        if slot.shared.lock().profile.is_some() {
            return Ok(slot.chat.profile_set_field("name".into(), Some(serde_json::json!(name)))?);
        }
        let unlocked = {
            let mut s = slot.shared.lock();
            let mut disk = s.disk.clone().ok_or(Error::NoIdentity)?;
            let Disk::V2(p) = &mut disk;
            p.name = name.clone();
            p.profile_name_pending = true;
            if s.committed {
                s.acct.as_ref().ok_or(Error::NoIdentity)?.store.save_profile(&disk)?;
            }
            s.disk = Some(disk);
            if let Some(me) = s.me.clone() {
                let mut profile = me.profile.clone();
                profile.name = name;
                s.me = Some(Arc::new(Me::load(profile, me.epoch)?));
            }
            // The old ticket carries the old name (and `ticket()` checks it, for the locked case
            // where the sealed state cannot be touched). Reusable codes: the old one still adds
            // contacts until it expires (whoever holds it sees the old name).
            retire_ticket(&mut s.state, true);
            s.me.is_some()
        };
        if unlocked { slot.persist() } else { Ok(()) }.map_err(Error::from)
    }

    /// Drops the ticket we hand out, and every older one still live: the old QR/text stops
    /// working at once and the next `my_ticket` mints a fresh one ("New code"; "the ticket may
    /// have leaked"). Requests that already arrived stay to be answered.
    pub fn reset_ticket(&self) -> Result<(), Error> {
        let slot = self.open();
        slot.me()?;
        retire_ticket(&mut slot.shared.lock().state, false);
        slot.persist().map_err(Error::from)
    }
}
