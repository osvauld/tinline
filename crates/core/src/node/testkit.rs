//! `*_for_test` hooks. Hidden from docs, and compiled only for unit tests and `dev-tools` builds, so release builds do not ship them.

use super::*;

impl Node {
    /// For tests: (opens, opens that had to wait for a file still held) of the store files
    /// (`account.redb`, `chat.redb`, `group.redb`, `search.redb`) under this node's data
    /// directory, across every node of this process that used it.
    #[doc(hidden)]
    pub fn store_stats_for_test(&self) -> (u64, u64) {
        store::open_stats(self.inner.accounts.root())
    }
}

impl Node {
    /// For tests: how many chat sessions with `did` are up right now.
    #[doc(hidden)]
    pub fn chat_sessions_for_test(&self, did: String) -> u32 {
        self.inner.chat_core().map_or(0, |core| self.inner.chat_sessions_of(&core, &did).len() as u32)
    }

    /// For tests: how many own-device sync sessions are up right now.
    #[doc(hidden)]
    pub fn own_sessions_for_test(&self) -> u32 {
        self.inner.selfsync.sessions.lock().len() as u32
    }
}

impl Node {
    /// For tests: this install's attestation as JSON, to register it with another install of
    /// the same phrase (`add_own_device_for_test`).
    #[doc(hidden)]
    pub fn own_attestation_for_test(&self) -> Result<String, Error> {
        Ok(serde_json::to_string(&self.inner.me()?.attestation)?)
    }

    /// For tests: registers a second install of this account (real linking and sync are task
    /// 34d). Its attestation must verify under our DID. The device list we send changes.
    #[doc(hidden)]
    pub fn add_own_device_for_test(&self, attestation_json: String) -> Result<(), Error> {
        let me = self.inner.me()?;
        let attestation: proto::SignedAttestation = serde_json::from_str(&attestation_json)?;
        let att = proto::verify_attestation(&attestation).map_err(|e| Error::Protocol(e.to_string()))?;
        if att.did != me.id.did() {
            return Err(Error::Protocol("not our account".into()));
        }
        let device = att.device;
        {
            let mut s = self.inner.shared.lock();
            if s.epoch != me.epoch {
                return Err(Error::Locked);
            }
            let reg = &mut s.state.registry;
            match reg.iter_mut().find(|e| e.device == device) {
                Some(e) => {
                    e.attestation = attestation;
                    e.removed = false;
                }
                None => reg.push(OwnDevice { device, attestation, label: String::new(), removed: false, last_seen: now(), relay: None, links: Vec::new(), rotated: false }),
            }
        }
        self.inner.persist_for(Some(me.epoch))?;
        self.inner.devices_changed();
        Ok(())
    }

    /// For tests: offers `list_json` (a `SignedBlob`) to the contact `did` as if it had arrived
    /// in a hello; whether it was taken.
    #[doc(hidden)]
    pub fn offer_device_list_for_test(&self, did: String, list_json: String) -> bool {
        let Ok(me) = self.inner.me() else {
            return false;
        };
        let Ok(blob) = serde_json::from_str::<proto::SignedDeviceList>(&list_json) else {
            return false;
        };
        let taken = self.inner.accept_device_list(me.epoch, &did, &blob);
        let _ = self.inner.persist_for(Some(me.epoch));
        taken
    }

    /// For tests: the device keys we would dial for `did`, in order (text form).
    #[doc(hidden)]
    pub fn contact_devices_for_test(&self, did: String) -> Vec<String> {
        let s = self.inner.shared.lock();
        s.state
            .contacts
            .iter()
            .find(|c| c.did == did)
            .map(|c| c.devices.iter().map(|d| proto::device_to_text(&d.device)).collect())
            .unwrap_or_default()
    }

    /// For tests: this install's device key (text form, as in `contact_devices_for_test`).
    #[doc(hidden)]
    pub fn device_key_for_test(&self) -> Result<String, Error> {
        Ok(proto::device_to_text(&self.inner.me()?.device))
    }

    /// For tests: the device list we currently publish (a JSON `SignedBlob`).
    #[doc(hidden)]
    pub fn own_device_list_for_test(&self) -> Option<String> {
        let me = self.inner.me().ok()?;
        serde_json::to_string(&self.inner.my_device_list(&me)?).ok()
    }

    /// For tests: dies like a killed process. The runtime stops at once and nothing is closed,
    /// so peers hear nothing (no QUIC close, no group-call End) and must notice by timeout.
    #[doc(hidden)]
    pub fn crash_for_test(mut self) {
        if let Some(ep) = self.inner.shared.lock().endpoint.take() {
            std::mem::forget(ep);
        }
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }

    /// For tests: the next `n` writes of account state fail, as if the disk were full.
    #[doc(hidden)]
    pub fn fail_next_writes(&self, n: u32) {
        self.inner.fail_writes.store(n, Ordering::SeqCst);
    }

    /// For tests: replaces this install's device key on disk, as endpoint rotation will (the
    /// same install: same device ID, data key and passphrase). Takes effect on the next unlock
    /// (restart the node).
    #[doc(hidden)]
    pub fn rotate_device_key_on_disk_for_test(&self) -> Result<(), Error> {
        let me = self.inner.me()?;
        let s = self.inner.shared.lock();
        let Some(Disk::V2(cur)) = s.disk.clone() else { return Err(Error::NoIdentity) };
        let dek = s.dek.clone().ok_or(Error::Locked)?;
        let device_secret = proto::new_device_secret();
        let secrets = Secrets { mnemonic: me.profile.mnemonic.clone(), device_secret };
        let vault = vault::reseal(&cur.vault, &dek, &secrets)?;
        let disk = Disk::V2(ProfileV2 { device_public: cryptography::signature::public_key(&device_secret), vault, ..cur });
        Ok(s.acct.as_ref().ok_or(Error::NoIdentity)?.store.save_profile(&disk)?)
    }
}
