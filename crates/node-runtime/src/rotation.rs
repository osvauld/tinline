//! Private (1:1) endpoint rotation (packet F; `docs/rotation.md` "Private endpoint").
//!
//! The private endpoint key is the device key. Each install rotates it on its own timer, on the
//! one `RotationPolicy` the group endpoint uses (stored in `group.redb`): `due = made + interval +
//! jitter`, nothing else moves it (not blocking, unlinking or anything else).
//!
//! A rotation, under the lifecycle lock:
//! 1. Stage: a fresh key in memory; `PrivateEp::pending` (public parts and the succession link)
//!    and the overlap key (the current secret) are written to the sealed state.
//! 2. Bind the new endpoint while the old one keeps serving.
//! 3. Promote: the vault and `account.json` get the new key in one atomic file write, and the
//!    node swaps `Me` and its endpoint in memory at the same moment.
//! 4. Complete: the registry tombstones the old key as `rotated` and gives the new one the link,
//!    `made`/`jitter`/`announce_until` are set, the pending row goes.
//! 5. The old endpoint keeps answering for `PRIVATE_OVERLAP_MS` (`overlap_incoming`), then closes.
//! 6. Announce: the re-signed device list goes to every contact we may tell (`devlist.rs`), our
//!    own devices learn it through the account doc.
//!
//! A crash before the promote leaves the old key (the staged one was never on disk); after it,
//! the new one, and the next start completes step 4 from `pending` (`private_rotation_started`).

use super::*;
pub use group_core::rotation::{RotationPolicy, draw_jitter, rotation_due};
use crate::store::{OverlapKey, PendingRotation, PrivateEp};
use zeroize::Zeroizing;

/// How long the previous private address keeps answering after a rotation (decided
/// 2026-10-16): contacts that dial it are told the new one; then it closes.
pub const PRIVATE_OVERLAP_MS: u64 = 24 * 3600 * 1000;
/// Succession links an install keeps in its registry entry (about a month at the default
/// interval): a contact away for longer than that sees a new install and must verify again.
pub const KEPT_LINKS: usize = 4;
/// How long a rotation that is due waits for a 1:1 or group call to end (an hour past the
/// deadline); both endpoints use it.
pub const CALL_ROTATION_DEFER_MS: u64 = 3_600_000;
/// How often the schedule is looked at.
const TICK: Duration = Duration::from_secs(2);

/// Runtime state that is not persisted.
#[derive(Default)]
pub struct RotationRt {
    /// A rotation is running: one at a time.
    running: AtomicBool,
    /// The previous endpoint while it overlaps.
    pub overlap: Mutex<Option<Endpoint>>,
    /// Bumped on every start: a tick loop of an earlier start stops.
    generation: AtomicU32,
    /// For tests: the overlap in ms instead of `PRIVATE_OVERLAP_MS`.
    pub overlap_ms_for_test: Mutex<Option<u64>>,
}

struct Running<'a>(&'a AtomicBool);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Step 4 on the state: the old key is a `rotated` tombstone, the new one carries its links, the
/// schedule restarts from `now` and contacts are tried until `now + announce`.
pub fn complete_rotation(st: &mut State, me: &Me, p: &PendingRotation, label: String, now: u64, policy: &RotationPolicy) {
    let mut links = Vec::new();
    for e in st.registry.iter_mut().filter(|e| e.device == p.old) {
        e.removed = true;
        e.rotated = true;
        links = std::mem::take(&mut e.links);
    }
    links.push(p.link.clone());
    let keep = links.len().saturating_sub(KEPT_LINKS);
    links.drain(..keep);
    match st.registry.iter_mut().find(|e| e.device == me.device) {
        Some(e) => {
            e.links = links;
            e.removed = false;
        }
        None => st.registry.insert(0, OwnDevice {
            device: me.device,
            attestation: me.attestation.clone(),
            label,
            removed: false,
            last_seen: now / 1000,
            relay: None,
            links,
            rotated: false,
        }),
    }
    st.private_ep = PrivateEp {
        made: now,
        jitter: draw_jitter(policy.jitter_ms),
        announce_until: now.saturating_add(policy.announce_ms),
        pending: None,
    };
    // The ticket names the old key: the next one shown carries the new address. The old one stays
    // good until it expires, while the old address still answers.
    retire_ticket(st, true);
}

impl NodeCtx {
    /// The rotation policy in force: the one `set_group_rotation_policy` stores drives both
    /// endpoints. `Err` while the group store is not open (the schedule then waits).
    pub fn private_rotation_policy(self: &Arc<Self>) -> Result<RotationPolicy, Error> {
        match self.hooks() {
            Some(h) => h.group_rotation_policy(),
            None => Err(Error::NotStarted),
        }
    }

    pub fn overlap_ms(&self) -> u64 {
        self.rotation.overlap_ms_for_test.lock().unwrap_or(PRIVATE_OVERLAP_MS)
    }

    /// When the private endpoint is due to rotate (unix ms), `None` before it is scheduled or
    /// while the policy cannot be read.
    pub fn private_rotation_due(self: &Arc<Self>) -> Option<u64> {
        let policy = self.private_rotation_policy().ok()?;
        let ep = self.shared.lock().state.private_ep.clone();
        if ep.made == 0 {
            return None;
        }
        rotation_due(ep.made, &policy, ep.jitter, now_ms(), true)
    }

    /// The endpoint whose key is `key`: the current one or the overlapping previous one.
    pub fn endpoint_of(&self, key: [u8; 32]) -> Option<Endpoint> {
        if let Ok(ep) = self.endpoint()
            && *ep.id().as_bytes() == key
        {
            return Some(ep);
        }
        self.rotation.overlap.lock().clone().filter(|ep| *ep.id().as_bytes() == key)
    }

    /// After the endpoint is up (start): finish or drop a rotation a stop interrupted, schedule
    /// the first key, bring back the overlapping previous endpoint, start the tick loop.
    pub async fn private_rotation_started(self: &Arc<Self>) {
        let Ok(me) = self.me() else { return };
        let policy = self.private_rotation_policy().unwrap_or_default();
        let now = now_ms();
        let overlap = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return;
            }
            let label = s.device_label.clone().unwrap_or_default();
            let st = &mut s.state;
            if let Some(p) = st.private_ep.pending.clone() {
                if p.new == me.device {
                    // Promoted, not completed: complete it now (`made` from when it was staged).
                    complete_rotation(st, &me, &p, label, p.at, &policy);
                } else {
                    // Never promoted: the staged key is gone, the old one serves on.
                    st.private_ep.pending = None;
                    st.overlap = None;
                }
            }
            if st.private_ep.made == 0 {
                st.private_ep.made = now;
                st.private_ep.jitter = draw_jitter(policy.jitter_ms);
            }
            let live = st.overlap.as_ref().is_some_and(|o| o.until > now && cryptography::signature::public_key(&o.secret) != me.device);
            if !live {
                st.overlap = None;
            }
            st.overlap.as_ref().map(|o| Zeroizing::new(o.secret))
        };
        self.persist_logged(me.epoch).await;
        if let Some(secret) = overlap
            && self.rotation.overlap.lock().is_none()
        {
            match self.bind_private(&secret).await {
                Ok(ep) => {
                    self.log("previous endpoint answers until its overlap ends");
                    *self.rotation.overlap.lock() = Some(ep.clone());
                    self.serve_private(ep, false);
                }
                Err(e) => self.log(format!("previous endpoint: {e}")),
            }
        }
        let generation = self.rotation.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let this = self.clone();
        self.handle.spawn(async move {
            loop {
                tokio::time::sleep(TICK).await;
                if this.rotation.generation.load(Ordering::SeqCst) != generation || this.endpoint().is_err() {
                    break;
                }
                if let Err(e) = this.private_rotation_tick().await {
                    tracing::debug!("private rotation: {e}");
                }
            }
        });
    }

    async fn private_rotation_tick(self: &Arc<Self>) -> Result<(), Error> {
        let me = self.me()?;
        let now = now_ms();
        let (close_overlap, window_over) = {
            let s = self.shared.lock();
            let st = &s.state;
            let until = st.overlap.as_ref().map_or(0, |o| o.until);
            (self.rotation.overlap.lock().is_some() && until <= now, st.private_ep.announce_until != 0 && st.private_ep.announce_until <= now)
        };
        if close_overlap {
            self.close_overlap(me.epoch).await;
        }
        if window_over {
            self.announce_window_over(&me).await;
        }
        let Some(due) = self.private_rotation_due() else { return Ok(()) };
        if due > now {
            return Ok(());
        }
        // Not in a call (it runs on this endpoint), for at most an hour past the deadline.
        let busy = { let l = self.live.lock(); l.call.is_some() || l.waiting.is_some() };
        if busy && now < due + CALL_ROTATION_DEFER_MS {
            return Ok(());
        }
        self.private_rotate().await
    }

    /// Closes the previous endpoint at the end of its overlap and forgets its key.
    async fn close_overlap(self: &Arc<Self>, epoch: u64) {
        let ep = self.rotation.overlap.lock().take();
        {
            let mut s = self.shared.lock();
            if s.epoch == epoch {
                s.state.overlap = None;
            }
        }
        self.persist_logged(epoch).await;
        if let Some(ep) = ep {
            self.log("previous endpoint closed");
            let gate = self.closing.clone().lock_owned().await;
            let _ = tokio::time::timeout(close_wait(), self.close_fully(ep, gate)).await;
        }
    }

    /// The announce window ended: every contact our note never reached is `lost_touch`, and
    /// background tries stop (a later session with them still delivers it).
    async fn announce_window_over(self: &Arc<Self>, me: &Me) {
        let lost: Vec<String> = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return;
            }
            let st = &mut s.state;
            st.private_ep.announce_until = 0;
            let lost: Vec<String> = st.list_outbox.iter().filter(|d| st.contacts.iter().any(|c| &c.did == *d)).cloned().collect();
            st.list_outbox.clear();
            st.lost_touch.extend(lost.iter().cloned());
            lost
        };
        self.persist_logged(me.epoch).await;
        if !lost.is_empty() {
            self.log(format!("lost touch with {} contacts after the announce window", lost.len()));
            self.sink.on_contacts_changed();
        }
    }

    /// An authenticated exchange with `did` worked: we are in touch.
    pub fn touched(self: &Arc<Self>, epoch: u64, did: &str) {
        let changed = {
            let mut s = self.shared.lock();
            s.epoch == epoch && s.state.lost_touch.remove(did)
        };
        if changed {
            self.persist_in_background(epoch);
            self.sink.on_contacts_changed();
        }
    }

    /// `did` holds an address of ours that is no longer current: queue our list for them.
    pub fn owe_list(self: &Arc<Self>, did: &str) {
        let Ok(me) = self.me() else { return };
        let queued = {
            let mut s = self.shared.lock();
            let same = s.epoch == me.epoch;
            let st = &mut s.state;
            same && st.contacts.iter().any(|c| c.did == did)
                && !st.blocked.contains(did)
                && !crate::block::blocked_me(st, did)
                && st.list_outbox.insert(did.to_string())
        };
        if queued {
            self.persist_in_background(me.epoch);
        }
        self.lists_kick(did.to_string());
    }

    /// A connection to the previous endpoint while it overlaps. A contact's device we may tell is
    /// told our new address (our list goes to them from the new endpoint) and closed; our own
    /// devices get a sync from the new one. Anyone else is handled as on the current endpoint,
    /// so a DID we block is refused at its hello exactly as there and never learns the new
    /// address, and a hello on a ticket naming this key is answered under it.
    pub async fn overlap_incoming(self: Arc<Self>, accepting: iroh::endpoint::Accepting, alpn: Vec<u8>, on: [u8; 32], permit: OwnedSemaphorePermit, reserved: bool) -> Result<(), Error> {
        let started = tokio::time::Instant::now();
        let conn = tokio::time::timeout(HELLO_TIMEOUT, accepting).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?;
        let remote = *conn.remote_id().as_bytes();
        let (contact, refused, own) = {
            let s = self.shared.lock();
            let st = &s.state;
            let did = st.contacts.iter().find(|c| c.has_device(&remote)).map(|c| c.did.clone());
            let refused = did.as_ref().is_some_and(|d| st.blocked.contains(d) || crate::block::blocked_me(st, d));
            let own = st.registry.iter().any(|e| e.device == remote && !e.removed);
            (did, refused, own)
        };
        if let Some(did) = contact.filter(|_| !refused) {
            drop(permit);
            self.log(format!("{} reached our previous address: sending the new one", crate::short(&did)));
            self.owe_list(&did);
            conn.close(0u32.into(), b"moved");
            return Ok(());
        }
        if own {
            drop(permit);
            self.sync_kick();
            conn.close(0u32.into(), b"moved");
            return Ok(());
        }
        if alpn.as_slice() == chat_core::wire::CHAT_ALPN {
            return match self.hooks() {
                Some(h) => h.chat_conn(conn, true, permit).await,
                None => Ok(()),
            };
        }
        if alpn.as_slice() == proto::ALPN {
            return self.handle_incoming_conn(conn, permit, reserved, on, started).await;
        }
        drop(permit);
        conn.close(0u32.into(), b"moved");
        Ok(())
    }

    /// Rotates the private endpoint now (crash-safe; see the module docs).
    pub async fn private_rotate(self: &Arc<Self>) -> Result<(), Error> {
        if self.rotation.running.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let _running = Running(&self.rotation.running);
        let _life = self.lifecycle.lock().await;
        let me = self.me()?;
        let old_ep = self.endpoint()?;
        if !self.shared.lock().committed {
            return Err(Error::Rejected("the identity is not saved yet".into()));
        }
        let policy = self.private_rotation_policy()?;
        let next = Zeroizing::new(proto::new_device_secret());
        let new_pub = cryptography::signature::public_key(&next);
        let now = now_ms();
        let pending = PendingRotation {
            old: me.device,
            new: new_pub,
            link: proto::sign_succession(&me.profile.device_secret, me.id.did(), new_pub),
            at: now,
        };
        // 1. Stage.
        let replaced = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return Err(Error::Locked);
            }
            s.state.private_ep.pending = Some(pending.clone());
            s.state.overlap.replace(OverlapKey { secret: me.profile.device_secret, until: now.saturating_add(self.overlap_ms()) })
        };
        let unstage = |this: &Arc<Self>| {
            let mut s = this.shared.lock();
            if s.epoch == me.epoch {
                s.state.private_ep.pending = None;
                s.state.overlap = replaced.clone();
            }
        };
        if let Err(e) = self.persist_async(me.epoch).await {
            unstage(self);
            return Err(e);
        }
        // 2. Bind the new endpoint while the old one serves.
        let new_ep = match self.bind_private(&next).await {
            Ok(ep) => ep,
            Err(e) => {
                unstage(self);
                self.persist_logged(me.epoch).await;
                return Err(e);
            }
        };
        // 3. Promote (one atomic file write) and swap in memory at the same moment.
        let promoted = (|| -> Result<Arc<Me>, Error> {
            let _gate = self.gate.lock();
            let mut s = self.shared.lock();
            if s.epoch != me.epoch || s.me.as_ref().map(|m| m.device) != Some(me.device) {
                return Err(Error::Locked);
            }
            let Some(Disk::V2(cur)) = s.disk.clone() else { return Err(Error::NoIdentity) };
            let dek = s.dek.clone().ok_or(Error::Locked)?;
            let secrets = Secrets { mnemonic: me.profile.mnemonic.clone(), device_secret: *next };
            let vault = vault::reseal(&cur.vault, &dek, &secrets)?;
            let disk = Disk::V2(ProfileV2 { device_public: new_pub, vault, ..cur });
            s.acct.as_ref().ok_or(Error::NoIdentity)?.store.save_profile(&disk)?;
            s.disk = Some(disk);
            let mut profile = me.profile.clone();
            profile.device_secret = *next;
            let new_me = Arc::new(Me::load(profile, me.epoch)?);
            s.me = Some(new_me.clone());
            s.endpoint = Some(new_ep.clone());
            // 4. Complete (persisted below; a crash first is completed on the next start).
            let label = s.device_label.clone().unwrap_or_default();
            complete_rotation(&mut s.state, &new_me, &pending, label, now, &policy);
            Ok(new_me)
        })();
        let new_me = match promoted {
            Ok(m) => m,
            Err(e) => {
                unstage(self);
                self.persist_logged(me.epoch).await;
                let _ = tokio::time::timeout(Duration::from_secs(3), new_ep.close()).await;
                return Err(e);
            }
        };
        // The live endpoint is already the new one; a failed write leaves the state (contact
        // notifications, overlap) unsaved until the next write. Loud, since nothing else shows it.
        if let Err(e) = self.persist_async(me.epoch).await {
            tracing::warn!("endpoint rotation: the new account state was NOT saved ({e}); the next write retries");
        }
        // 5. The old endpoint overlaps; one rotated before it closes now.
        let older = self.rotation.overlap.lock().replace(old_ep.clone());
        if let Some(ep) = older {
            let gate = self.closing.clone().lock_owned().await;
            let _ = tokio::time::timeout(close_wait(), self.close_fully(ep, gate)).await;
        }
        // Sessions on the old endpoint end; the old accept loop now answers as an overlap.
        if let Some(h) = self.hooks() {
            h.chat_sessions_close();
        }
        if let Some(h) = self.hooks() {
            h.selfsync_sessions_close();
        }
        self.serve_private(new_ep.clone(), true);
        self.log("private endpoint rotated");
        self.emit_status();
        drop(_life);
        // 6. Announce.
        self.devices_changed();
        self.sync_kick();
        if let Some(h) = self.hooks() {
            h.chat_kick_pending();
        }
        let _ = new_me;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
