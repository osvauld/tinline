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
        let accounts = AccountDirs::open(data_dir)?;
        accounts.migrate()?;
        let mut shared = Shared::new();
        // A legacy profile keeps working (calls keep ringing) until it is converted.
        if let Some(id) = accounts.current() {
            shared.install(load_account(&accounts, &id)?)?;
        }
        let inner = Arc::new(Inner {
            handle: rt.handle().clone(),
            accounts,
            gate: Mutex::new(()),
            fail_writes: AtomicU32::new(0),
            writing: Mutex::new(()),
            events,
            shared: Mutex::new(shared),
            live: Mutex::new(Live::default()),
            pending: Arc::new(Semaphore::new(MAX_PENDING_HELLOS)),
            reserved: Arc::new(Semaphore::new(RESERVED_HELLOS)),
            lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            closing: Arc::new(tokio::sync::Mutex::new(())),
            chat: Mutex::new(None),
            chat_events: Mutex::new(None),
            link_events: Mutex::new(None),
            selfsync: Default::default(),
            sync_tx: tokio::sync::watch::channel(0).0,
            link: Default::default(),
            gcall: Default::default(),
            block: Default::default(),
            requests: Default::default(),
            group: Default::default(),
            caps: Default::default(),
            devlist: Default::default(),
            profile_events: Default::default(),
            profile_downloads: Default::default(),
            profile_fetches: Arc::new(Semaphore::new(4)),
            video: Default::default(),
            search: Mutex::new(None),
            search_skip: AtomicU32::new(0),
        });
        Ok(Arc::new(Self { inner, rt: Some(rt) }))
    }

    pub fn has_identity(&self) -> bool {
        self.inner.shared.lock().disk.is_some()
    }

    /// Whether the identity is on disk. False only for a no-passphrase identity that has not
    /// been `commit_identity`ed yet (and with no identity at all).
    pub fn identity_committed(&self) -> bool {
        let s = self.inner.shared.lock();
        s.disk.is_some() && s.committed
    }

    /// Makes a no-passphrase identity durable: writes `account.json` and selects the account.
    /// The platform calls it once its keystore has saved `unlock_key()`; until then nothing is
    /// on disk, so a failed keystore write (or a crash) leaves no account whose key is lost, and
    /// `lock` or a restart simply forgets the identity. No-op once committed, and for an
    /// identity made with a passphrase (written at once).
    pub fn commit_identity(&self) -> Result<(), Error> {
        let _gate = self.inner.gate.lock();
        self.inner.commit_locked()
    }

    /// Every account on this device. Needs no unlock: only the clear `account.json` is read.
    pub fn accounts(&self) -> Vec<AccountSummary> {
        let listed = self.inner.accounts.list().unwrap_or_else(|e| {
            tracing::warn!("listing accounts: {e}");
            Vec::new()
        });
        let s = self.inner.shared.lock();
        let current = s.acct.as_ref().map(|a| a.id.clone());
        let mut out: Vec<AccountSummary> = listed
            .into_iter()
            .map(|e| AccountSummary {
                current: current.as_deref() == Some(e.id.as_str()),
                did: e.did,
                name: e.name,
                has_passphrase: e.has_passphrase,
            })
            .collect();
        if let (Some(Disk::V2(p)), false) = (&s.disk, s.committed) {
            out.push(AccountSummary { did: p.did.clone(), name: p.name.clone(), current: true, has_passphrase: false });
        }
        out
    }

    /// Makes `did` the current account: stops the endpoint, locks and forgets the one in memory.
    /// Unlock it as usual afterwards (`unlock` or `unlock_with_key`). `InCall` during a 1:1
    /// call; a group call is left (or, hosted here, handed to its backup) as on `lock`.
    pub fn switch_account(&self, did: String) -> Result<(), Error> {
        let id = accounts::id_of(&did)?;
        self.inner.not_in_call()?;
        let loaded = load_account(&self.inner.accounts, &id)?;
        let inner = self.inner.clone();
        self.block_on(async move { inner.replace_account(Some(loaded)).await })?
    }

    /// Leaves no account selected, so `has_identity` is false and `create_identity` /
    /// `restore_identity` add a new one. The accounts on disk are untouched. `InCall` during a
    /// 1:1 call; a group call is left as on `switch_account`.
    pub fn begin_new_account(&self) -> Result<(), Error> {
        self.inner.not_in_call()?;
        let inner = self.inner.clone();
        self.block_on(async move { inner.replace_account(None).await })?
    }

    /// Deletes a non-current account and everything in its directory.
    pub fn remove_account(&self, did: String) -> Result<(), Error> {
        let id = accounts::id_of(&did)?;
        let _life = self.lifecycle()?;
        let current = self.inner.shared.lock().acct.as_ref().map(|a| a.id.clone());
        if current.as_deref() == Some(id.as_str()) || self.inner.accounts.current().as_deref() == Some(id.as_str()) {
            return Err(Error::Protocol("the current account cannot be removed".into()));
        }
        self.inner.accounts.remove(&id)
    }

    /// Names this installation ("Personal phone"), 1-64 characters, no control characters.
    /// Sealed with the account: needs the node unlocked, and a vault (not a legacy profile).
    pub fn set_device_label(&self, label: String) -> Result<(), Error> {
        let label = label.trim().to_string();
        if label.is_empty() || label.chars().count() > 64 || label.chars().any(char::is_control) {
            return Err(Error::Protocol("a device name is 1-64 characters without control characters".into()));
        }
        let me = self.inner.me()?;
        let db = {
            let s = self.inner.shared.lock();
            match s.acct.as_ref().and_then(|a| a.backing.clone()) {
                Some(Backing::Sealed(db)) => Some(db),
                Some(Backing::Json(_)) => return Err(Error::Protocol("convert the profile with set_passphrase first".into())),
                None => None, // not committed yet: kept in memory, written by the commit
            }
        };
        if let Some(db) = db {
            store::save_label(&db, &label)?;
        }
        // Our entry in the registry carries the name to the other devices.
        let registered = {
            let mut s = self.inner.shared.lock();
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
            self.inner.persist_for(Some(me.epoch))?;
        }
        Ok(())
    }

    /// The label given to `set_device_label`; `None` until set, and while locked.
    pub fn device_label(&self) -> Option<String> {
        self.inner.shared.lock().device_label.clone()
    }

    pub fn lock_state(&self) -> LockState {
        let s = self.inner.shared.lock();
        match (&s.disk, &s.me) {
            (None, _) => LockState::NoIdentity,
            (Some(Disk::Legacy(_)), _) => LockState::NeedsPassphrase,
            (Some(Disk::V2(_)), Some(_)) => LockState::Unlocked,
            (Some(Disk::V2(_)), None) => LockState::Locked,
        }
    }

    /// Whether a passphrase wraps the vault's data key. False for a vault the platform alone
    /// opens (see `unlock_key`) and for a legacy profile.
    pub fn has_passphrase(&self) -> bool {
        matches!(&self.inner.shared.lock().disk, Some(Disk::V2(p)) if p.vault.has_passphrase())
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
        let s = self.inner.shared.lock();
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
        let vault = match self.unlock_target()? {
            Some(v) => v,
            None => return Ok(()),
        };
        let (secrets, dek) = self.kdf(move || vault::open(&vault, &passphrase))??;
        self.finish_unlock(secrets, dek)
    }

    /// Unlocks with a data key the platform remembered (from `unlock_key`); fast.
    /// `WrongPassphrase` if it does not open the vault: the platform should then forget it.
    pub fn unlock_with_key(&self, mut key: Vec<u8>) -> Result<(), Error> {
        let vault = match self.unlock_target() {
            Ok(Some(v)) => v,
            other => {
                key.zeroize();
                return other.map(|_| ());
            }
        };
        let opened = vault::open_with_key(&vault, &key).and_then(|secrets| {
            <[u8; 32]>::try_from(key.as_slice())
                .map(|k| (secrets, zeroize::Zeroizing::new(k)))
                .map_err(|_| Error::WrongPassphrase)
        });
        key.zeroize();
        let (secrets, dek) = opened?;
        self.finish_unlock(secrets, dek)
    }

    /// The vault's data key while unlocked via a vault, for the platform to remember (wrapped
    /// by its own hardware key). `None` when locked or on a legacy profile. Stays valid across
    /// `set_passphrase`.
    pub fn unlock_key(&self) -> Option<Vec<u8>> {
        self.inner.shared.lock().dek.as_ref().map(|d| d.to_vec())
    }

    /// Sets or changes the passphrase (any non-empty one). With `old = None` it converts a
    /// legacy profile (state `NeedsPassphrase`; an empty `new` seals it without a passphrase)
    /// and the clear-text secrets leave profile.json; on a vault without a passphrase it adds
    /// one (the node must be unlocked); otherwise `old` is required and verified. The data key
    /// is kept, so a remembered `unlock_key` stays valid. Slow: runs Argon2id once or twice.
    pub fn set_passphrase(&self, old: Option<String>, new: String) -> Result<(), Error> {
        let _life = self.lifecycle()?;
        let snapshot = self.inner.shared.lock().disk.clone();
        match snapshot.ok_or(Error::NoIdentity)? {
            Disk::Legacy(p) => {
                if old.is_some() {
                    return Err(Error::Protocol("this identity has no passphrase yet".into()));
                }
                let new = Some(new).filter(|n| !n.is_empty());
                let id = identity::recover(&p.mnemonic).map_err(|_| Error::BadPhrase)?;
                let secrets = Secrets { mnemonic: p.mnemonic.clone(), device_secret: p.device_secret };
                let device_public = cryptography::signature::public_key(&p.device_secret);
                let (vault, dek) = self.kdf(move || vault::seal(&secrets, new.as_deref()))??;
                let mut s = self.inner.shared.lock();
                // The name may have changed (set_name) while the KDF ran.
                let name = match s.me.as_ref() {
                    Some(me) => me.profile.name.clone(),
                    None => p.name.clone(),
                };
                let disk = Disk::V2(ProfileV2 {
                    version: 2,
                    name,
                    did: id.did().to_string(),
                    device_public,
                    vault,
                    device_id: Some(crate::store::new_device_id()),
                    profile_name_pending: false,
                    profile_inherited: false,
                });
                let store = s.acct.as_ref().ok_or(Error::NoIdentity)?.store.clone();
                // The vault first: if we die before the state is sealed, the JSON is still
                // there and the next unlock imports it.
                store.save_profile(&disk)?;
                let db = store::seal_into(&store, &p.mnemonic, &dek, &s.state, &s.history.snapshot(), None)?;
                store.remove_legacy_leftovers();
                let backing = Backing::Sealed(db);
                s.history.attach(backing.clone());
                if let Some(a) = s.acct.as_mut() {
                    a.backing = Some(backing);
                }
                s.disk = Some(disk);
                s.dek = Some(dek);
                drop(s);
                self.inner.chat_open();
        self.inner.acc_open();
                Ok(())
            }
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
                    let dek = self.inner.shared.lock().dek.clone().ok_or(Error::Locked)?;
                    self.kdf(move || vault::rewrap(&vault, &dek, &new))??
                };
                let mut s = self.inner.shared.lock();
                // Only the vault changes; keep any name change made meanwhile.
                let Some(Disk::V2(cur)) = s.disk.clone() else { return Err(Error::NoIdentity) };
                let disk = Disk::V2(ProfileV2 { vault: rewrapped, ..cur });
                if !s.committed {
                    // A passphrase makes the data key recoverable, so the identity can be written now.
                    s.disk = Some(disk);
                    drop(s);
                    let _gate = self.inner.gate.lock();
                    return self.inner.commit_locked();
                }
                s.acct.as_ref().ok_or(Error::NoIdentity)?.store.save_profile(&disk)?;
                s.disk = Some(disk);
                Ok(())
            }
        }
    }

    /// The recovery phrase, re-derived from the vault: `WrongPassphrase` unless `passphrase`
    /// is right, and `Locked` on a legacy profile (convert it with `set_passphrase` first).
    /// Works while locked. Slow: runs Argon2id. With no passphrase set, `passphrase` is
    /// ignored and the node must be unlocked: the platform asks for device authentication
    /// before calling this.
    pub fn recovery_phrase(&self, passphrase: String) -> Result<String, Error> {
        let vault = {
            let s = self.inner.shared.lock();
            match &s.disk {
                None => return Err(Error::NoIdentity),
                Some(Disk::Legacy(_)) => return Err(Error::Locked),
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
    /// `unlock`. No-op without a vault (no identity, or a legacy profile).
    pub fn lock(&self) {
        if !matches!(self.inner.shared.lock().disk, Some(Disk::V2(_))) {
            return;
        }
        self.inner.link_cancel();
        let inner = self.inner.clone();
        self.run_lifecycle(async move {
            {
                // Under the lifecycle lock, so a `start` that is binding right now finishes
                // first and its endpoint is closed here.
                let _life = inner.lifecycle.lock().await;
                inner.gcall_shutdown().await;
                inner.close_endpoint().await;
                inner.shared.lock().forget_session();
                inner.chat_close().await;
            }
            inner.emit_status();
        });
    }

    pub fn set_name(&self, name: String) -> Result<(), Error> {
        if name.len() > 256 || name.chars().any(|c| !proto::safe_name_char(c)) { return Err(Error::Protocol("invalid profile name".into())); }
        if self.inner.shared.lock().profile.is_some() {
            return self.inner.profile_set_field("name".into(), Some(serde_json::json!(name)));
        }
        let unlocked = {
            let mut s = self.inner.shared.lock();
            let mut disk = s.disk.clone().ok_or(Error::NoIdentity)?;
            match &mut disk {
                Disk::V2(p) => { p.name = name.clone(); p.profile_name_pending = true; }
                Disk::Legacy(p) => p.name = name.clone(),
            }
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
        if unlocked { self.inner.persist() } else { Ok(()) }
    }

    /// Drops the ticket we hand out, and every older one still live: the old QR/text stops
    /// working at once and the next `my_ticket` mints a fresh one ("New code"; "the ticket may
    /// have leaked"). Requests that already arrived stay to be answered.
    pub fn reset_ticket(&self) -> Result<(), Error> {
        self.inner.me()?;
        retire_ticket(&mut self.inner.shared.lock().state, false);
        self.inner.persist()
    }
}
