//! Group endpoint rotation on `Node` (`docs/rotation.md`): the per-device policy, when the next
//! rotation is due, and "change it now". No app calls these yet.

use super::*;
use crate::group::rotation::RotationPolicy;

#[uniffi::export]
impl Node {
    /// This device's group endpoint rotation policy (the defaults until one is set).
    pub fn group_rotation_policy(&self) -> Result<RotationPolicy, Error> {
        let core = self.inner.group_core()?;
        self.inner.group_rotation_policy(&core)
    }

    /// Stores this device's rotation policy. `Rejected` below `RotationPolicy::MIN_INTERVAL_MS`,
    /// with a jitter longer than the interval, or with an empty announce window. The next
    /// rotation still counts from the last one, so saving the setting never postpones it.
    pub fn set_group_rotation_policy(&self, policy: RotationPolicy) -> Result<(), Error> {
        let core = self.inner.group_core()?;
        core.store.set_rotation_policy(&policy)?;
        core.kick_pub();
        Ok(())
    }

    /// On a host: when this device's group endpoint will rotate next (unix ms; periodic). `None`
    /// while it hosts no chat and no calls.
    pub fn group_endpoint_rotation_due(&self) -> Option<u64> {
        let core = self.inner.group_core().ok()?;
        self.inner.group_rotation_due(&core).ok().flatten()
    }

    /// Rotates this device's group endpoint now (a Settings "change it now"): the next periodic
    /// rotation counts from this one. `NotStarted` before the group endpoint is up.
    pub fn group_rotate_now(&self) -> Result<(), Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            inner.group_rotate(&core).await
        })?
    }
}
