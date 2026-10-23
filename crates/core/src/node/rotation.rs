//! `Node` API for private endpoint rotation (the rotation itself is `node_runtime`'s `rotation.rs`,
//! packet F; `docs/rotation.md` "Private endpoint").

use super::*;
use zeroize::Zeroizing;
use crate::store::{OverlapKey, PendingRotation};

#[uniffi::export]
impl Node {
    /// When this device's private endpoint will rotate next (unix ms; periodic, on the same
    /// policy as the group endpoint, `set_group_rotation_policy`). `None` while locked.
    pub fn private_endpoint_rotation_due(&self) -> Option<u64> {
        self.inner.ctx.private_rotation_due()
    }

    /// Rotates this device's private endpoint now (a Settings "change it now"); the next periodic
    /// rotation counts from this one. `NotStarted` before the endpoint is up.
    pub fn private_rotate_now(&self) -> Result<(), Error> {
        self.run(|inner| async move { inner.ctx.private_rotate().await })
    }
}

impl Node {
    /// For tests: the overlap of the previous endpoint, in ms (`None`: `PRIVATE_OVERLAP_MS`).
    #[doc(hidden)]
    pub fn set_private_overlap_for_test(&self, ms: Option<u64>) {
        *self.inner.rotation.overlap_ms_for_test.lock() = ms;
    }

    /// For tests: contacts marked `lost_touch`.
    #[doc(hidden)]
    pub fn lost_touch_for_test(&self) -> Vec<String> {
        self.inner.shared.lock().state.lost_touch.iter().cloned().collect()
    }

    /// For tests: a rotation that stops half way, as a crash would leave it: staged (the pending
    /// row and overlap key written), and with `promoted` also the new key written to the vault
    /// and `account.json`, but nothing swapped in memory. Reopen the node next.
    #[doc(hidden)]
    pub fn private_rotate_interrupted_for_test(&self, promoted: bool) -> Result<(), Error> {
        let inner = &self.inner;
        let me = inner.me()?;
        let next = Zeroizing::new(proto::new_device_secret());
        let new_pub = cryptography::signature::public_key(&next);
        let now = now_ms();
        {
            let mut s = inner.shared.lock();
            s.state.private_ep.pending = Some(PendingRotation {
                old: me.device,
                new: new_pub,
                link: proto::sign_succession(&me.profile.device_secret, me.id.did(), new_pub),
                at: now,
            });
            s.state.overlap = Some(OverlapKey { secret: me.profile.device_secret, until: now.saturating_add(inner.overlap_ms()) });
        }
        inner.persist_for(Some(me.epoch))?;
        if promoted {
            let s = inner.shared.lock();
            let Some(Disk::V2(cur)) = s.disk.clone() else { return Err(Error::NoIdentity) };
            let dek = s.dek.clone().ok_or(Error::Locked)?;
            let secrets = Secrets { mnemonic: me.profile.mnemonic.clone(), device_secret: *next };
            let vault = vault::reseal(&cur.vault, &dek, &secrets)?;
            let disk = Disk::V2(ProfileV2 { device_public: new_pub, vault, ..cur });
            s.acct.as_ref().ok_or(Error::NoIdentity)?.store.save_profile(&disk)?;
        }
        Ok(())
    }

    /// For tests: whether the previous endpoint is still bound.
    #[doc(hidden)]
    pub fn overlap_open_for_test(&self) -> bool {
        self.inner.rotation.overlap.lock().is_some()
    }
}

