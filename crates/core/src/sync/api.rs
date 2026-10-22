//! UniFFI surface of linking, linked devices and unlinking.

use std::sync::Arc;

use crate::node::{Node, now};
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
        self.inner.sync.set_link_sink(Arc::new(LinkEventsAdapter(events)));
    }

    /// E (unlocked, started): shows a QR (`OSVL1:...`, valid 5 minutes, one scan) for a new
    /// device to scan. Progress arrives through `LinkEvents`.
    pub fn link_show_qr(&self) -> Result<String, Error> {
        Ok(self.inner.sync.link_show_qr()?)
    }

    /// E: scans the QR a new device shows, and dials it.
    pub fn link_scan(&self, qr_text: String) -> Result<(), Error> {
        Ok(self.inner.sync.link_scan(&qr_text)?)
    }

    /// N (no account selected; `begin_new_account` first if needed): shows a QR. `device_label`
    /// is this install's name ("Pixel 8"), shown on E.
    pub fn link_new_show_qr(&self, device_label: String) -> Result<String, Error> {
        self.inner.sync.require_no_account()?;
        let (secret, ep) = self.fresh_link_endpoint()?;
        Ok(self.inner.sync.link_new_show_qr(device_label, secret, ep)?)
    }

    /// N: scans E's QR.
    pub fn link_new_scan(&self, qr_text: String, device_label: String) -> Result<(), Error> {
        self.inner.sync.require_no_account()?;
        let (secret, ep) = self.fresh_link_endpoint()?;
        Ok(self.inner.sync.link_new_scan(&qr_text, device_label, secret, ep)?)
    }

    /// E: the user compared the codes and agrees. With a passphrase on the account it must be
    /// given (`WrongPassphrase` otherwise, and the link stays open to retry). Without one the
    /// platform must have asked for device authentication first (same bar as showing the
    /// recovery phrase, because that is what is sent). `NotFound` if no link is waiting.
    pub fn link_approve(&self, passphrase: Option<String>) -> Result<(), Error> {
        self.inner.me()?;
        self.local_auth(passphrase)?;
        Ok(self.inner.sync.link_approve_cmd()?)
    }

    /// Abandons the link in progress (either side); `on_link_failed("cancelled")` follows if a
    /// peer was connected.
    pub fn link_cancel(&self) {
        self.inner.sync.link_cancel();
    }

    /// The registry of this account: every install, this one first.
    pub fn linked_devices(&self) -> Vec<LinkedDevice> {
        let s = self.inner.shared.lock();
        let Some(me) = s.me.as_ref() else { return Vec::new() };
        let mut out: Vec<LinkedDevice> = s
            .state
            .registry
            .iter()
            // A key this install (or a sibling) rotated away from is not an unlinked device.
            .filter(|e| !e.rotated)
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
        self.inner.persist_for(Some(me.epoch)).map_err(Error::from)
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
            self.inner.sync.selfsync.leaving.store(true, std::sync::atomic::Ordering::SeqCst);
            {
                let mut s = self.inner.shared.lock();
                if let Some(e) = s.state.registry.iter_mut().find(|e| e.device == dev) {
                    e.removed = true;
                }
            }
            // The others are told only once "removed" is on disk: otherwise we would reload
            // un-removed while they have already dropped us.
            if let Err(e) = self.inner.persist_for(Some(me.epoch)) {
                if let Some(e) = self.inner.shared.lock().state.registry.iter_mut().find(|e| e.device == dev) {
                    e.removed = false;
                }
                self.inner.sync.selfsync.leaving.store(false, std::sync::atomic::Ordering::SeqCst);
                return Err(e.into());
            }
            self.inner.sync.wait_pushed(std::time::Duration::from_secs(5));
            let inner = self.inner.clone();
            let epoch = me.epoch;
            let r = self.block_on(async move {
                inner.sync.clone().remove_self_account(epoch).await;
            });
            self.inner.sync.selfsync.leaving.store(false, std::sync::atomic::Ordering::SeqCst);
            return r;
        }
        let relay = {
            let mut s = self.inner.shared.lock();
            let e = s.state.registry.iter_mut().find(|e| e.device == dev).ok_or(Error::NotFound)?;
            e.removed = true;
            e.relay.clone()
        };
        self.inner.persist_for(Some(me.epoch))?;
        if let Some(slot) = self.inner.sync.selfsync.sessions.lock().get(&dev) {
            slot.close();
        }
        let inner = self.inner.clone();
        self.inner.handle.spawn(async move {
            inner.sync.send_unlinked(dev, relay).await;
        });
        // U8: contacts stop dialling the unlinked install.
        self.inner.ctx.devices_changed();
        if let Some(ev) = self.inner.sync.link_events() {
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

    /// For tests: dials `device` on `tinline/self/3` as this node and reports whether the peer
    /// accepted us (answered with its `Auth`).
    #[doc(hidden)]
    pub fn self_probe_for_test(&self, device: String) -> bool {
        let Ok(dev) = proto::device_from_text(&device) else { return false };
        let inner = self.inner.clone();
        self.block_on(async move { inner.sync.self_probe(dev).await.is_ok() }).unwrap_or(false)
    }

    /// For tests: forget the dial backoff and dial now.
    #[doc(hidden)]
    pub fn sync_now_for_test(&self) {
        self.inner.sync_kick();
    }

    /// For tests: offers a hand-made batch for today's shard with `contact` to the validator
    /// that judges batches relayed by our other devices. `kind`: 0 = a message of the contact's
    /// signed by another account's DID key, 1 = a message of ours signed by another account's DID
    /// key, 2 = a stranger's message, validly signed by the stranger, 3 = a genuine message of
    /// ours (control). Each is written under the writer ID its author and device ID give, so only
    /// the signature (or the author) is wrong.
    #[doc(hidden)]
    pub fn forged_own_batch_for_test(&self, contact: String, kind: u8) -> Result<(), Error> {
        use crate::chat::doc::{MsgRec, Shard, Writer, day_of, doc_name, new_device_value, pair_id};
        let me = self.inner.me()?;
        let core = self.inner.chat.chat_core()?;
        let pair = pair_id(me.id.did(), &contact);
        let at = crate::node::now_ms() as i64;
        let day = day_of(at);
        let (stranger, _) = identity::generate();
        let (author, writer) = match kind {
            0 => (contact.clone(), Writer::forging(stranger, new_device_value(), &contact)),
            1 => (me.id.did().to_string(), Writer::forging(stranger, new_device_value(), me.id.did())),
            2 => (stranger.did().to_string(), Writer::new(stranger, new_device_value())),
            _ => (me.id.did().to_string(), self.inner.chat.chat_writer(&me, &core)),
        };
        let shard = Shard::new(&pair, &day, writer.writer_id());
        let rec = MsgRec { id: format!("forged{kind}"), author, at, text: format!("forged {kind}"), edited_at: None, deleted: false, reply_to: None, file: None, dev: [0; 32], did_sig: Vec::new() };
        let update = shard.add_message(&rec, &writer).map_err(|e| Error::Io(e.to_string()))?;
        let inner = self.inner.clone();
        let doc = doc_name(&pair, &day);
        self.block_rt(async move { inner.chat.chat_own_apply(&doc, &update).await })
    }

    /// For tests: does the chat store hold any shard of the conversation between `me_did` and
    /// `peer_did` (`me_did == peer_did`: the "You" docs)?
    #[doc(hidden)]
    pub fn has_chat_docs_for_test(&self, me_did: &str, peer_did: &str) -> bool {
        let Ok(core) = self.inner.chat.chat_core() else { return false };
        let pair = crate::chat::doc::pair_id(me_did, peer_did);
        core.store.shard_days(&pair).map(|d| !d.is_empty()).unwrap_or(false)
    }

    /// For tests (P9): our writer ID (`None` while the chat store is not open).
    #[doc(hidden)]
    pub fn writer_id_for_test(&self) -> Option<u64> {
        self.inner.chat.chat_core().ok().map(|c| c.wid)
    }

    /// For tests (P9): every `(day, Loro peer id, op count)` in our copy of the conversation
    /// with `peer_did`, and per message `(id, peer id of the change that created it)`.
    #[doc(hidden)]
    #[allow(clippy::type_complexity)]
    pub fn chat_doc_peers_for_test(&self, peer_did: String) -> (Vec<(String, u64, i32)>, Vec<(String, u64)>) {
        let inner = self.inner.clone();
        self.block_on(async move {
            let (Ok(me), Ok(core)) = (inner.me(), inner.chat.chat_core()) else { return (vec![], vec![]) };
            let pair = crate::chat::doc::pair_id(me.id.did(), &peer_did);
            let (mut peers, mut created) = (Vec::new(), Vec::new());
            for day in core.store.shard_days(&pair).unwrap_or_default() {
                let Ok(shards) = inner.chat.chat_shard(&core, &pair, &day).await else { continue };
                let Some(s) = shards.get(&(pair.clone(), day.clone())) else { continue };
                peers.extend(s.vv().iter().map(|(p, c)| (day.clone(), *p, *c)));
                for id in s.messages().map(|m| m.into_keys().collect::<Vec<_>>()).unwrap_or_default() {
                    if let Some((p, _)) = s.creator_end(&id) {
                        created.push((id, p));
                    }
                }
            }
            (peers, created)
        })
        .unwrap_or_default()
    }

    /// For tests (P10): the next `n` chat writes (`chat_persist`) fail, as if the disk were full.
    #[doc(hidden)]
    pub fn fail_next_chat_writes_for_test(&self, n: u32) {
        if let Ok(core) = self.inner.chat.chat_core() {
            core.fail_writes.store(n, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// For tests (P10b): stores a message of ours to `peer_did` sent at `at_ms`, written under
    /// another writer ID (a fresh device ID), as if it had just arrived from a sibling device by
    /// own-device sync: pending for the contact, not pushed. Returns its id.
    #[doc(hidden)]
    pub fn write_as_sibling_for_test(&self, peer_did: String, text: String, at_ms: i64) -> Result<String, Error> {
        let inner = self.inner.clone();
        self.block_rt(async move { inner.chat.chat_write_as_sibling_for_test(&peer_did, text, at_ms).await })
    }

    /// For tests: like `write_as_sibling_for_test`, but filed in the shard of `day`
    /// (`YYYY-MM-DD`) whatever `at_ms` is.
    #[doc(hidden)]
    pub fn write_in_day_for_test(&self, peer_did: String, text: String, at_ms: i64, day: String) -> Result<String, Error> {
        let inner = self.inner.clone();
        self.block_rt(async move { inner.chat.chat_write_in_day_for_test(&peer_did, text, at_ms, &day).await })
    }

    /// For tests: messages written now are stamped `ms` ahead of the real clock (negative: a
    /// message written at that past time by a right clock; edits, hello days and compaction use
    /// it too). The live window and catch-up decision keep the real clock.
    #[doc(hidden)]
    pub fn chat_clock_offset_for_test(&self, ms: i64) {
        if let Ok(core) = self.inner.chat.chat_core() {
            core.clock_wrong.store(false, std::sync::atomic::Ordering::Relaxed);
            core.clock_ms.store(ms, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// For tests: this device's clock is wrong by `ms`: as `chat_clock_offset_for_test`, and the
    /// live window and catch-up decision use it as well (0 sets it right again).
    #[doc(hidden)]
    pub fn chat_clock_skew_for_test(&self, ms: i64) {
        if let Ok(core) = self.inner.chat.chat_core() {
            core.clock_ms.store(ms, std::sync::atomic::Ordering::Relaxed);
            core.clock_wrong.store(ms != 0, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// For tests: does the chat store hold any "You" shard?
    #[doc(hidden)]
    pub fn has_you_docs_for_test(&self) -> bool {
        let Ok(me) = self.inner.me() else { return false };
        self.has_chat_docs_for_test(me.id.did(), me.id.did())
    }
}
