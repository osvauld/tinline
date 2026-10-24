//! The account's device registry as the app sees it: the list, renaming, unlinking. p2pcore's
//! `Node` forwards to these (and keeps the vault authentication and the UniFFI record).

use std::sync::Arc;

use crate::SyncRt;
use node_runtime::{Error, now};

/// One install of the account (p2pcore's `LinkedDevice` is this, field for field).
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceEntry {
    /// Device key, text form.
    pub device: String,
    pub label: String,
    pub this_device: bool,
    /// Unix seconds when we last talked to it; `None` if never.
    pub last_seen: Option<u64>,
    /// Unlinked (tombstone).
    pub removed: bool,
}

impl SyncRt {
    /// The registry of this account: every install, this one first.
    pub fn linked_devices(&self) -> Vec<DeviceEntry> {
        let s = self.shared.lock();
        let Some(me) = s.me.as_ref() else { return Vec::new() };
        let mut out: Vec<DeviceEntry> = s
            .state
            .registry
            .iter()
            // A key this install (or a sibling) rotated away from is not an unlinked device.
            .filter(|e| !e.rotated)
            .map(|e| DeviceEntry {
                device: proto::device_to_text(&e.device),
                label: e.label.clone(),
                this_device: e.device == me.device,
                last_seen: if e.device == me.device { Some(now()) } else { Some(e.last_seen).filter(|t| *t > 0) },
                removed: e.removed,
            })
            .collect();
        if !out.iter().any(|d| d.this_device) {
            out.insert(0, DeviceEntry {
                device: proto::device_to_text(&me.device),
                label: s.device_label.clone().unwrap_or_default(),
                this_device: true,
                last_seen: Some(now()),
                removed: false,
            });
        }
        out.sort_by_key(|d| (!d.this_device, d.removed));
        out
    }

    /// Renames an install (its own name for this one: `set_own_label` is `set_device_label`).
    pub fn rename_device<E: From<Error>>(
        &self,
        dev: [u8; 32],
        label: &str,
        set_own_label: impl FnOnce(String) -> Result<(), E>,
    ) -> Result<(), E> {
        let me = self.me()?;
        let label = proto::sanitize_name(label);
        if label.is_empty() {
            return Err(Error::Protocol("a device name is 1-64 characters without control characters".into()).into());
        }
        if dev == me.device {
            set_own_label(label.clone())?;
        }
        {
            let mut s = self.shared.lock();
            let e = s.state.registry.iter_mut().find(|e| e.device == dev).ok_or(Error::NotFound)?;
            e.label = label;
        }
        self.persist_for(Some(me.epoch)).map_err(E::from)
    }

    /// Unlinking this install, first half (the caller has authenticated and found `dev` to be
    /// `this_device`): tells the others it is leaving, marks it removed and puts that on disk
    /// (reverted if that fails), then waits for the others to be told. The caller then runs
    /// `remove_self_account` and `unlink_self_done`.
    pub fn unlink_self_begin(&self, dev: [u8; 32], epoch: u64) -> Result<(), Error> {
        self.not_in_call()?;
        // Tell the others first (best effort: they stop dialling this device), then go.
        self.selfsync.leaving.store(true, std::sync::atomic::Ordering::SeqCst);
        {
            let mut s = self.shared.lock();
            if let Some(e) = s.state.registry.iter_mut().find(|e| e.device == dev) {
                e.removed = true;
            }
        }
        // The others are told only once "removed" is on disk: otherwise we would reload
        // un-removed while they have already dropped us.
        if let Err(e) = self.persist_for(Some(epoch)) {
            if let Some(e) = self.shared.lock().state.registry.iter_mut().find(|e| e.device == dev) {
                e.removed = false;
            }
            self.selfsync.leaving.store(false, std::sync::atomic::Ordering::SeqCst);
            return Err(e);
        }
        self.wait_pushed(std::time::Duration::from_secs(5));
        Ok(())
    }

    /// Unlinking this install, last half: the account is gone from this device.
    pub fn unlink_self_done(&self) {
        self.selfsync.leaving.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Removes another install of the account: it stops syncing and ringing, and is told to drop
    /// the account if it is reachable.
    pub fn unlink_other_device(self: &Arc<Self>, dev: [u8; 32], epoch: u64) -> Result<(), Error> {
        let relay = {
            let mut s = self.shared.lock();
            let e = s.state.registry.iter_mut().find(|e| e.device == dev).ok_or(Error::NotFound)?;
            e.removed = true;
            e.relay.clone()
        };
        self.persist_for(Some(epoch))?;
        if let Some(slot) = self.selfsync.sessions.lock().get(&dev) {
            slot.close();
        }
        let this = self.clone();
        self.handle.spawn(async move {
            this.send_unlinked(dev, relay).await;
        });
        // U8: contacts stop dialling the unlinked install.
        self.ctx.devices_changed();
        if let Some(ev) = self.link_events() {
            ev.on_devices_changed();
        }
        Ok(())
    }
}
