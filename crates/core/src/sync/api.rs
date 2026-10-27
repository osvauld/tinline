//! UniFFI surface of linking, linked devices and unlinking.

use std::sync::Arc;

use crate::node::Node;
use crate::store::Disk;
use crate::vault;
use crate::Error;

/// Callbacks of the linking flow and of own-device sync. Set with `Node::set_link_events`; calls
/// arrive on a core thread, so do not block in them.
#[uniffi::export(rust, foreign)]
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

impl From<sync_runtime::DeviceEntry> for LinkedDevice {
    fn from(d: sync_runtime::DeviceEntry) -> Self {
        Self { device: d.device, label: d.label, this_device: d.this_device, last_seen: d.last_seen, removed: d.removed }
    }
}

sink_adapter!(
    /// Forwards the runtime's `LinkSink` to the app's `LinkEvents`.
    pub(crate) LinkEventsAdapter(LinkEvents) => sync_runtime::LinkSink {
        fn on_link_code(code: String, peer_label: String);
        fn on_link_done(did: String, name: String);
        fn on_link_failed(reason: String);
        fn on_devices_changed();
        fn on_history_changed();
        fn on_unlinked(did: String);
    }
);

impl Node {
    fn local_auth(&self, passphrase: Option<String>) -> Result<(), Error> {
        let slot = self.open();
        let vault = match &slot.shared.lock().disk {
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
        self.hub.set_sinks(|s| s.link = Some(Arc::new(LinkEventsAdapter(events))));
    }

    /// E (unlocked, started): shows a QR (`OSVL1:...`, valid 5 minutes, one scan) for a new
    /// device to scan. Progress arrives through `LinkEvents`.
    pub fn link_show_qr(&self) -> Result<String, Error> {
        Ok(self.open().sync.link_show_qr()?)
    }

    /// E: scans the QR a new device shows, and dials it.
    pub fn link_scan(&self, qr_text: String) -> Result<(), Error> {
        Ok(self.open().sync.link_scan(&qr_text)?)
    }

    /// N (no account selected; `begin_new_account` first if needed): shows a QR. `device_label`
    /// is this install's name ("Pixel 8"), shown on E.
    pub fn link_new_show_qr(&self, device_label: String) -> Result<String, Error> {
        let slot = self.open();
        slot.sync.require_no_account()?;
        let (secret, ep) = self.fresh_link_endpoint()?;
        Ok(slot.sync.link_new_show_qr(device_label, secret, ep)?)
    }

    /// N: scans E's QR.
    pub fn link_new_scan(&self, qr_text: String, device_label: String) -> Result<(), Error> {
        let slot = self.open();
        slot.sync.require_no_account()?;
        let (secret, ep) = self.fresh_link_endpoint()?;
        Ok(slot.sync.link_new_scan(&qr_text, device_label, secret, ep)?)
    }

    /// E: the user compared the codes and agrees. With a passphrase on the account it must be
    /// given (`WrongPassphrase` otherwise, and the link stays open to retry). Without one the
    /// platform must have asked for device authentication first (same bar as showing the
    /// recovery phrase, because that is what is sent). `NotFound` if no link is waiting.
    pub fn link_approve(&self, passphrase: Option<String>) -> Result<(), Error> {
        let slot = self.open();
        slot.me()?;
        self.local_auth(passphrase)?;
        Ok(slot.sync.link_approve_cmd()?)
    }

    /// Abandons the link in progress (either side); `on_link_failed("cancelled")` follows if a
    /// peer was connected.
    pub fn link_cancel(&self) {
        self.open().sync.link_cancel();
    }

    /// The registry of this account: every install, this one first.
    pub fn linked_devices(&self) -> Vec<LinkedDevice> {
        self.open().sync.linked_devices().into_iter().map(LinkedDevice::from).collect()
    }

    /// Renames an install (its own name for this one: same as `set_device_label`).
    pub fn rename_device(&self, device: String, label: String) -> Result<(), Error> {
        let dev = proto::device_from_text(&device)?;
        self.open().sync.rename_device(dev, &label, |l| self.set_device_label(l))
    }

    /// Removes an install from the account: it stops syncing and ringing, and is told to drop the
    /// account if it is reachable. Needs the passphrase if the account has one (device
    /// authentication otherwise, by the platform). Unlinking `this_device` removes the account from
    /// this device (`InCall` during a call). Not cryptographic revocation: see the design doc.
    pub fn unlink_device(&self, device: String, passphrase: Option<String>) -> Result<(), Error> {
        let slot = self.open();
        let dev = proto::device_from_text(&device)?;
        let me = slot.me()?;
        self.local_auth(passphrase)?;
        if dev == me.device {
            slot.sync.unlink_self_begin(dev, me.epoch)?;
            let epoch = me.epoch;
            let r = self.run(move |inner| async move {
                inner.sync.clone().remove_self_account(epoch).await;
                Ok(())
            });
            slot.sync.unlink_self_done();
            return r;
        }
        Ok(slot.sync.unlink_other_device(dev, me.epoch)?)
    }
}

impl Node {
    /// For tests: the redeemed ticket nonces.
    #[doc(hidden)]
    pub fn redeemed_for_test(&self) -> Vec<String> {
        let mut v: Vec<String> = self.open().shared.lock().state.redeemed.iter().cloned().collect();
        v.sort();
        v
    }

    /// For tests: the blocked DIDs.
    #[doc(hidden)]
    pub fn blocked_for_test(&self) -> Vec<String> {
        let mut v: Vec<String> = self.open().shared.lock().state.blocked.iter().cloned().collect();
        v.sort();
        v
    }

    /// For tests: dials `device` on `tinline/self/3` as this node and reports whether the peer
    /// accepted us (answered with its `Auth`).
    #[doc(hidden)]
    pub fn self_probe_for_test(&self, device: String) -> bool {
        let Ok(dev) = proto::device_from_text(&device) else { return false };
        let inner = self.open();
        self.block_on(async move { inner.sync.self_probe(dev).await.is_ok() }).unwrap_or(false)
    }

    /// For tests: forget the dial backoff and dial now.
    #[doc(hidden)]
    pub fn sync_now_for_test(&self) {
        self.open().sync_kick();
    }
}
