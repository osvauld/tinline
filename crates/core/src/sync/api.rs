//! UniFFI surface of linking, linked devices and unlinking.

use std::sync::Arc;

use crate::node::{Inner, Node, now};
use crate::store::Disk;
use crate::vault;
use crate::Error;

/// Callbacks of the linking flow and of own-device sync. Set with `Node::set_link_events`; calls
/// arrive on a core thread, so do not block in them.
#[uniffi::export(with_foreign)]
pub trait LinkEvents: Send + Sync {
    /// Both devices are connected and proved they know the QR. Show `code` (6 digits, "482 913")
    /// on both screens and `peer_label` (the other device's name); E asks the user to compare and
    /// approve (`link_approve`). On N this means "waiting for approval on the other device".
    fn on_link_code(&self, code: String, peer_label: String);
    /// The link worked. On N: the account now exists in memory, unlocked and NOT yet on disk:
    /// call `set_passphrase(None, new)` or save `unlock_key()` and call `commit_identity()`, then
    /// `start()`. On E: the new device is in the registry (`did`/`name` are the account's).
    fn on_link_done(&self, did: String, name: String);
    /// The link ended without success. `reason` is one of: `expired`, `cancelled`, `rejected`,
    /// `bad_proof`, `mismatch`, `timeout`, `net`, `locked`, `busy`, `bad_grant`, `account_exists`.
    fn on_link_failed(&self, reason: String);
    /// The registry changed (device linked, renamed, unlinked, seen).
    fn on_devices_changed(&self);
    /// Call history changed because another own device synced.
    fn on_history_changed(&self);
    /// Another own device unlinked this one: the account was removed from this device (it is
    /// locked and gone; show the onboarding screen).
    fn on_unlinked(&self, did: String);
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LinkedDevice {
    /// Device key, text form (same as `ProfileInfo.device`).
    pub device: String,
    pub label: String,
    pub this_device: bool,
    /// Unix seconds when we last talked to it; `None` if never.
    pub last_seen: Option<u64>,
    /// Unlinked (tombstone); shown greyed or not at all.
    pub removed: bool,
}

impl Inner {
    pub(crate) fn link_events(&self) -> Option<Arc<dyn LinkEvents>> {
        self.link_events.lock().clone()
    }
}

impl Node {
    fn local_auth(&self, passphrase: Option<String>) -> Result<(), Error> {
        let vault = match &self.inner.shared.lock().disk {
            Some(Disk::V2(p)) if p.vault.has_passphrase() => Some(p.vault.clone()),
            _ => None,
        };
        if let Some(v) = vault {
            let pass = passphrase.ok_or(Error::WrongPassphrase)?;
            self.kdf(move || vault::open(&v, &pass))??;
        }
        Ok(())
    }

    fn fresh_link_endpoint(&self) -> Result<([u8; 32], iroh::Endpoint), Error> {
        let secret = proto::new_device_secret();
        let ep = self.block_on(async move {
            iroh::Endpoint::builder(iroh::endpoint::presets::N0)
                .secret_key(iroh::SecretKey::from_bytes(&secret))
                .alpns(vec![proto::link::LINK_ALPN.to_vec()])
                .bind()
                .await
                .map_err(Error::net)
        })??;
        Ok((secret, ep))
    }
}

#[uniffi::export]
impl Node {
    pub fn set_link_events(&self, events: Arc<dyn LinkEvents>) {
        *self.inner.link_events.lock() = Some(events);
    }

    /// E (unlocked, started): shows a QR (`OSVL1:...`, valid 5 minutes, one scan) for a new
    /// device to scan. Progress arrives through `LinkEvents`.
    pub fn link_show_qr(&self) -> Result<String, Error> {
        self.inner.link_show_qr()
    }

    /// E: scans the QR a new device shows, and dials it.
    pub fn link_scan(&self, qr_text: String) -> Result<(), Error> {
        self.inner.link_scan(&qr_text)
    }

    /// N (no account selected; `begin_new_account` first if needed): shows a QR. `device_label`
    /// is this install's name ("Pixel 8"), shown on E.
    pub fn link_new_show_qr(&self, device_label: String) -> Result<String, Error> {
        self.inner.require_no_account()?;
        let (secret, ep) = self.fresh_link_endpoint()?;
        self.inner.link_new_show_qr(device_label, secret, ep)
    }

    /// N: scans E's QR.
    pub fn link_new_scan(&self, qr_text: String, device_label: String) -> Result<(), Error> {
        self.inner.require_no_account()?;
        let (secret, ep) = self.fresh_link_endpoint()?;
        self.inner.link_new_scan(&qr_text, device_label, secret, ep)
    }

    /// E: the user compared the codes and agrees. With a passphrase on the account it must be
    /// given (`WrongPassphrase` otherwise, and the link stays open to retry). Without one the
    /// platform must have asked for device authentication first (same bar as showing the
    /// recovery phrase, because that is what is sent). `NotFound` if no link is waiting.
    pub fn link_approve(&self, passphrase: Option<String>) -> Result<(), Error> {
        self.inner.me()?;
        self.local_auth(passphrase)?;
        self.inner.link_approve_cmd()
    }

    /// Abandons the link in progress (either side); `on_link_failed("cancelled")` follows if a
    /// peer was connected.
    pub fn link_cancel(&self) {
        self.inner.link_cancel();
    }

    /// The registry of this account: every install, this one first.
    pub fn linked_devices(&self) -> Vec<LinkedDevice> {
        let s = self.inner.shared.lock();
        let Some(me) = s.me.as_ref() else { return Vec::new() };
        let mut out: Vec<LinkedDevice> = s
            .state
            .registry
            .iter()
            .map(|e| LinkedDevice {
                device: proto::device_to_text(&e.device),
                label: e.label.clone(),
                this_device: e.device == me.device,
                last_seen: if e.device == me.device { Some(now()) } else { Some(e.last_seen).filter(|t| *t > 0) },
                removed: e.removed,
            })
            .collect();
        if !out.iter().any(|d| d.this_device) {
            out.insert(0, LinkedDevice {
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

    /// Renames an install (its own name for this one: same as `set_device_label`).
    pub fn rename_device(&self, device: String, label: String) -> Result<(), Error> {
        let dev = proto::device_from_text(&device)?;
        let me = self.inner.me()?;
        let label = proto::sanitize_name(&label);
        if label.is_empty() {
            return Err(Error::Protocol("a device name is 1-64 characters without control characters".into()));
        }
        if dev == me.device {
            self.set_device_label(label.clone())?;
        }
        {
            let mut s = self.inner.shared.lock();
            let e = s.state.registry.iter_mut().find(|e| e.device == dev).ok_or(Error::NotFound)?;
            e.label = label;
        }
        self.inner.persist_for(Some(me.epoch))
    }

    /// Removes an install from the account: it stops syncing and ringing, and is told to drop the
    /// account if it is reachable. Needs the passphrase if the account has one (device
    /// authentication otherwise, by the platform). Unlinking `this_device` removes the account from
    /// this device (`InCall` during a call). Not cryptographic revocation: see the design doc.
    pub fn unlink_device(&self, device: String, passphrase: Option<String>) -> Result<(), Error> {
        let dev = proto::device_from_text(&device)?;
        let me = self.inner.me()?;
        self.local_auth(passphrase)?;
        if dev == me.device {
            self.inner.not_in_call()?;
            // Tell the others first (best effort: they stop dialling this device), then go.
            {
                let mut s = self.inner.shared.lock();
                if let Some(e) = s.state.registry.iter_mut().find(|e| e.device == dev) {
                    e.removed = true;
                }
            }
            let _ = self.inner.persist_for(Some(me.epoch));
            std::thread::sleep(std::time::Duration::from_millis(500));
            let inner = self.inner.clone();
            let epoch = me.epoch;
            return self.block_on(async move {
                inner.remove_self_account(epoch).await;
            });
        }
        let relay = {
            let mut s = self.inner.shared.lock();
            let e = s.state.registry.iter_mut().find(|e| e.device == dev).ok_or(Error::NotFound)?;
            e.removed = true;
            e.relay.clone()
        };
        self.inner.persist_for(Some(me.epoch))?;
        if let Some(slot) = self.inner.selfsync.sessions.lock().get(&dev) {
            slot.close();
        }
        let inner = self.inner.clone();
        self.inner.handle.spawn(async move {
            inner.send_unlinked(dev, relay).await;
        });
        if let Some(ev) = self.inner.link_events() {
            ev.on_devices_changed();
        }
        Ok(())
    }

}

impl Node {
    /// For tests: the redeemed ticket nonces.
    #[doc(hidden)]
    pub fn redeemed_for_test(&self) -> Vec<String> {
        let mut v: Vec<String> = self.inner.shared.lock().state.redeemed.iter().cloned().collect();
        v.sort();
        v
    }

    /// For tests: the blocked DIDs.
    #[doc(hidden)]
    pub fn blocked_for_test(&self) -> Vec<String> {
        let mut v: Vec<String> = self.inner.shared.lock().state.blocked.iter().cloned().collect();
        v.sort();
        v
    }

    /// For tests: dials `device` on `tinline/self/1` as this node and reports whether the peer
    /// accepted us (answered with its `Auth`).
    #[doc(hidden)]
    pub fn self_probe_for_test(&self, device: String) -> bool {
        let Ok(dev) = proto::device_from_text(&device) else { return false };
        let inner = self.inner.clone();
        self.block_on(async move { inner.self_probe(dev).await.is_ok() }).unwrap_or(false)
    }

    /// For tests: forget the dial backoff and dial now.
    #[doc(hidden)]
    pub fn sync_now_for_test(&self) {
        self.inner.sync_kick();
    }
}
