//! Unlocking a slot, and setting an identity on the open one.

use super::*;

impl Inner {
    /// The vault to open, or `None` when there is nothing to unlock.
    pub(super) fn unlock_target(&self) -> Result<Option<vault::Vault>, Error> {
        let s = self.shared.lock();
        match &s.disk {
            None => Err(Error::NoIdentity),
            Some(Disk::V2(_)) if s.me.is_some() => Ok(None),
            Some(Disk::V2(p)) => Ok(Some(p.vault.clone())),
        }
    }

    /// Unlocks with a data key the platform remembered (`Node::unlock_with_key`; a background
    /// account's key from `AccountEvents::account_key`). `WrongPassphrase` if it does not open
    /// the vault.
    pub(crate) fn unlock_with_key(self: &Arc<Self>, mut key: Vec<u8>) -> Result<(), Error> {
        let vault = match self.unlock_target() {
            Ok(Some(v)) => v,
            other => {
                key.zeroize();
                return other.map(|_| ());
            }
        };
        let opened = vault::open_with_key(&vault, &key).map_err(Error::from).and_then(|secrets| {
            <[u8; 32]>::try_from(key.as_slice())
                .map(|k| (secrets, zeroize::Zeroizing::new(k)))
                .map_err(|_| Error::WrongPassphrase)
        });
        key.zeroize();
        let (secrets, dek) = opened?;
        self.finish_unlock(secrets, dek)
    }

    pub(super) fn finish_unlock(self: &Arc<Self>, secrets: Secrets, dek: Dek) -> Result<(), Error> {
        let slot = self;
        let _gate = slot.gate.lock();
        let (store, did, name, started) = {
            let s = slot.shared.lock();
            let Some(Disk::V2(p)) = &s.disk else { return Err(Error::NoIdentity) };
            if s.me.is_some() {
                return Ok(());
            }
            let store = s.acct.as_ref().ok_or(Error::NoIdentity)?.store.clone();
            (store, p.did.clone(), p.name.clone(), s.epoch)
        };
        let me = Me::load(
            Profile { mnemonic: secrets.mnemonic.clone(), name, device_secret: secrets.device_secret },
            0,
        )?;
        if me.id.did() != did {
            return Err(Error::Io("vault does not match profile".into()));
        }
        // The sealed state is read outside `shared`.
        let (db, loaded) = store::load_sealed(&store, &secrets.mnemonic)?;
        let mut me = me;
        let mut s = slot.shared.lock();
        if s.epoch != started {
            // Another account was selected while the passphrase was being checked.
            return Err(Error::Locked);
        }
        s.epoch += 1;
        me.epoch = s.epoch;
        let backing = Backing::Sealed(db);
        s.history = Arc::new(History::new(loaded.calls, Some(backing.clone())));
        s.state = loaded.state;
        s.device_label = loaded.device_label;
        if let Some(a) = s.acct.as_mut() {
            a.backing = Some(backing);
        }
        s.me = Some(Arc::new(me));
        s.dek = Some(dek);
        drop(s);
        slot.chat_open();
        slot.sync.acc_open();
        Ok(())
    }
}

impl Node {
    pub(super) fn set_identity(&self, phrase: String, name: String, passphrase: String, inherited_profile: bool) -> Result<(), Error> {
        let slot = self.open();
        let _gate = slot.gate.lock();
        let profile = Profile { mnemonic: phrase, name, device_secret: proto::new_device_secret() };
        let mut me = Me::load(profile, 0)?;
        let did = me.id.did().to_string();
        let id = accounts::id_of(&did)?;
        // What is selected right now decides: a locked copy of this very account may be restored
        // over (the passphrase was forgotten); an unlocked one is not touched (`AccountExists`,
        // switch to it instead); any other selected account is `HaveIdentity`.
        {
            let s = slot.shared.lock();
            match &s.disk {
                None => {}
                Some(Disk::V2(p)) if p.did == did && s.me.is_none() && s.committed => {}
                Some(Disk::V2(p)) if p.did == did => return Err(Error::AccountExists(did)),
                Some(_) => return Err(Error::HaveIdentity),
            }
        }
        // Running in the background (unlocked): switch to it instead, as above. Two slots never
        // hold one account.
        if self.hub.is_woken(&id) {
            return Err(Error::AccountExists(did));
        }
        // Checked before the slow KDF; `create` / the restore below are the checks that cannot race.
        let existing = slot.accounts.exists(&id)?;
        let mut state = State::default();
        let mut calls = Vec::new();
        let mut label = None;
        if existing {
            let store = Store::open(slot.accounts.dir(&id)?)?;
            let old_device = match store.profile()? {
                Some(Disk::V2(p)) => Some(p.device_public),
                None => None,
            };
            // The data opens with the phrase alone.
            let (_db, loaded) = store::load_sealed(&store, &me.profile.mnemonic)?;
            state = loaded.state;
            calls = loaded.calls;
            label = loaded.device_label;
            // This install's previous device key died with the old vault.
            if let Some(old) = old_device {
                for e in state.registry.iter_mut().filter(|e| e.device == old && old != me.device) {
                    e.removed = true;
                }
            }
        }
        let secrets = Secrets { mnemonic: me.profile.mnemonic.clone(), device_secret: me.profile.device_secret };
        let passphrase = Some(passphrase).filter(|p| !p.is_empty());
        let durable = passphrase.is_some();
        let (vault, dek) = self.kdf(move || vault::seal(&secrets, passphrase.as_deref()))??;
        let disk = Disk::V2(ProfileV2 {
            version: 2,
            name: me.profile.name.clone(),
            did: did.clone(),
            device_public: me.device,
            vault,
            // A restore over an existing directory is a new install too: new device ID, new writer ID.
            device_id: Some(crate::store::new_device_id()),
            profile_name_pending: false,
            profile_inherited: inherited_profile,
        });
        // Without a passphrase nothing is written until the platform has saved the key.
        let acct = if durable {
            Some(if existing {
                slot.restore_account_files(&id, &disk, &me.profile.mnemonic, &state, &calls, label.as_deref())?
            } else {
                slot.create_account_files(&id, &disk, &me.profile.mnemonic, &state, &calls, label.as_deref())?
            })
        } else {
            None
        };
        let mut s = slot.shared.lock();
        s.epoch += 1;
        me.epoch = s.epoch;
        s.history = Arc::new(History::new(calls, acct.as_ref().and_then(|a| a.backing.clone())));
        s.state = state;
        s.device_label = label;
        s.committed = durable;
        s.acct = acct;
        s.disk = Some(disk);
        s.me = Some(Arc::new(me));
        s.dek = Some(dek);
        drop(s);
        slot.chat_open();
        slot.sync.acc_open();
        Ok(())
    }
}
