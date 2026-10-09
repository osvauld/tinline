//! Own-device sync: the account doc wired to `State`, and the `tinline/self/1` sessions.
//! See docs/protocol.md "Own-device sync".

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iroh::endpoint::{Accepting, Connection};
use loro::VersionVector;

use super::accdoc::{AccDoc, Ctx, DOC_NAME};
use super::frames::{FrameReader, FrameWriter, SelfMsg};
use crate::node::{Inner, Me, addr_for, now, now_ms, relay_of};
use crate::store::{Backing, OwnDevice};
use crate::Error;

const DOC_KEY: &str = "sync/account";
const AUTH_TIMEOUT: Duration = Duration::from_secs(15);
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);

/// One live session with another own device.
pub(crate) struct SelfSlot {
    // see `close`
    dialer: [u8; 32],
    conn: Connection,
}

impl SelfSlot {
    pub(crate) fn close(&self) {
        self.conn.close(0u32.into(), b"unlinked");
    }
}

#[derive(Default)]
pub(crate) struct SelfSync {
    pub sessions: parking_lot::Mutex<HashMap<[u8; 32], SelfSlot>>,
    /// Per device: failed attempts and when to try next.
    pub backoff: parking_lot::Mutex<HashMap<[u8; 32], (u32, Instant)>>,
    pub dialing: parking_lot::Mutex<HashSet<[u8; 32]>>,
    pub kick: tokio::sync::Notify,
}

fn sync_period() -> Duration {
    Duration::from_secs(std::env::var("P2P_SELF_SYNC_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(15))
}

impl Inner {
    fn ctx<'a>(me: &'a Me) -> Ctx<'a> {
        Ctx { my_did: me.id.did(), me_device: me.device, now: now(), now_ms: now_ms() as i64 }
    }

    /// Wakes the sessions (local data changed) and the dial loop.
    pub(crate) fn sync_bump(&self) {
        self.sync_tx.send_modify(|v| *v += 1);
    }

    pub(crate) fn sync_kick(&self) {
        self.selfsync.backoff.lock().clear();
        self.selfsync.kick.notify_one();
    }

    /// Opens the account doc of the unlocked account (called wherever the chat store opens):
    /// loads the sealed snapshot, flushes the working copy into it, merges it back.
    pub(crate) fn acc_open(self: &Arc<Self>) {
        let (me, backing) = {
            let s = self.shared.lock();
            let (Some(me), Some(_)) = (s.me.clone(), s.dek.as_ref()) else { return };
            if s.acc.is_some() {
                return;
            }
            (me, s.acct.as_ref().and_then(|a| a.backing.clone()))
        };
        let snapshot = match &backing {
            Some(Backing::Sealed(db)) => db.get(DOC_KEY).ok().flatten(),
            _ => None,
        };
        let doc = match AccDoc::load(&me.device, snapshot.as_deref()) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("account doc: {e}; starting a new one");
                AccDoc::new(&me.device)
            }
        };
        let changed = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch || s.acc.is_some() {
                return;
            }
            let label = s.device_label.clone();
            ensure_self_registered(&mut s.state.registry, &me, label);
            let calls = s.history.snapshot();
            let ctx = Self::ctx(&me);
            if let Err(e) = doc.write_from(&s.state, &calls, &ctx) {
                tracing::warn!("account doc flush: {e}");
            }
            let mut merged = calls.clone();
            let ch = doc.read_into(&mut s.state, &mut merged, &ctx);
            if ch.history {
                s.history.replace(merged);
            }
            s.acc = Some(Arc::new(doc));
            ch.contacts || ch.history || ch.devices
        };
        self.save_doc();
        if changed {
            self.events.on_contacts_changed();
        }
    }

    fn save_doc(&self) {
        let (acc, backing) = {
            let s = self.shared.lock();
            (s.acc.clone(), s.acct.as_ref().and_then(|a| a.backing.clone()))
        };
        if let (Some(acc), Some(Backing::Sealed(db))) = (acc, backing)
            && let Err(e) = db.put(DOC_KEY, &acc.snapshot())
        {
            tracing::warn!("saving account doc: {e}");
        }
    }

    /// A local write happened (state or call log): flush it into the doc and tell the sessions.
    pub(crate) fn sync_local(&self, epoch: Option<u64>) {
        let Ok(me) = self.me() else { return };
        let changed = {
            let s = self.shared.lock();
            if epoch.is_some_and(|e| e != s.epoch) || s.epoch != me.epoch {
                return;
            }
            let Some(acc) = s.acc.clone() else { return };
            let calls = s.history.snapshot();
            match acc.write_from(&s.state, &calls, &Self::ctx(&me)) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("account doc flush: {e}");
                    false
                }
            }
        };
        if changed {
            self.save_doc();
            self.sync_bump();
        }
    }

    /// Imports a batch from another own device and merges it into the working copy.
    pub(crate) fn apply_account_update(self: &Arc<Self>, epoch: u64, update: &[u8]) -> Result<(), Error> {
        let me = self.me()?;
        let (ch, state_backing) = {
            let mut s = self.shared.lock();
            if s.epoch != epoch || me.epoch != epoch {
                return Err(Error::Locked);
            }
            let acc = s.acc.clone().ok_or(Error::Locked)?;
            let ctx = Self::ctx(&me);
            let calls = s.history.snapshot();
            // Our own unwritten changes go in first, so they merge as concurrent edits.
            acc.write_from(&s.state, &calls, &ctx)?;
            if !acc.import(update)? {
                return Ok(());
            }
            let mut merged = calls;
            let ch = acc.read_into(&mut s.state, &mut merged, &ctx);
            if ch.history {
                s.history.replace(merged);
            }
            if let Some(l) = &ch.label {
                s.device_label = Some(l.clone());
            }
            (ch, s.acct.as_ref().and_then(|a| a.backing.clone()))
        };
        if let (Some(l), Some(Backing::Sealed(db))) = (&ch.label, &state_backing) {
            let _ = crate::store::save_label(db, l);
        }
        self.persist_for(Some(epoch))?;
        if ch.history {
            let _ = self.history().save();
        }
        self.save_doc();
        for did in &ch.removed {
            self.chat_purge(did);
        }
        if ch.contacts {
            self.events.on_contacts_changed();
        }
        if let Some(ev) = self.link_events() {
            if ch.devices {
                ev.on_devices_changed();
            }
            if ch.history {
                ev.on_history_changed();
            }
        }
        self.sync_bump();
        if ch.self_removed {
            let this = self.clone();
            self.handle.spawn(async move {
                this.remove_self_account(epoch).await;
            });
        }
        Ok(())
    }

    /// This device was unlinked: drop the account from it (best effort when a call is running;
    /// the next sync with a device that knows retries).
    pub(crate) async fn remove_self_account(self: Arc<Self>, epoch: u64) {
        let did = {
            let s = self.shared.lock();
            if s.epoch != epoch {
                return;
            }
            match s.me.as_ref() {
                Some(me) => me.id.did().to_string(),
                None => return,
            }
        };
        if self.not_in_call().is_err() {
            return;
        }
        let Ok(id) = crate::accounts::id_of(&did) else { return };
        if self.replace_account(None).await.is_err() {
            return;
        }
        if let Err(e) = self.accounts.remove(&id) {
            tracing::warn!("removing unlinked account: {e}");
        }
        self.log("this device was unlinked; the account was removed");
        if let Some(ev) = self.link_events() {
            ev.on_unlinked(did);
        }
    }

    /// A valid attestation of our DID from a device the registry does not list: a device that
    /// was linked but never announced itself (or one added while we were away).
    fn note_own_device(&self, epoch: u64, att: &proto::SignedAttestation, device: [u8; 32], relay: Option<String>) {
        let added = {
            let mut s = self.shared.lock();
            if s.epoch != epoch {
                return;
            }
            let t = now();
            match s.state.registry.iter_mut().find(|e| e.device == device) {
                Some(e) => {
                    e.last_seen = t;
                    if relay.is_some() && e.relay != relay {
                        e.relay = relay;
                    }
                    false
                }
                None => {
                    s.state.registry.push(OwnDevice { device, attestation: att.clone(), label: String::new(), removed: false, last_seen: t, relay });
                    true
                }
            }
        };
        if added {
            let _ = self.persist_for(Some(epoch));
            if let Some(ev) = self.link_events() {
                ev.on_devices_changed();
            }
        }
    }

    // ---- sessions -----------------------------------------------------------------------

    pub(crate) async fn handle_self_incoming(self: Arc<Self>, accepting: Accepting) -> Result<(), Error> {
        let conn = tokio::time::timeout(AUTH_TIMEOUT, accepting).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?;
        let r = self.clone().self_run(conn.clone(), false).await;
        if r.is_err() {
            conn.close(0u32.into(), b"not accepted");
        }
        r
    }

    async fn self_dial(self: Arc<Self>, device: [u8; 32], relay: Option<String>) -> Result<(), Error> {
        let ep = self.endpoint()?;
        let addr = addr_for(&device, relay.as_deref())?;
        let conn = tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, proto::SELF_ALPN))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(Error::net)?;
        let r = self.clone().self_run(conn.clone(), true).await;
        if r.is_err() {
            conn.close(0u32.into(), b"done");
        }
        r
    }

    fn tombstones(&self) -> HashSet<[u8; 32]> {
        self.shared.lock().state.registry.iter().filter(|e| e.removed).map(|e| e.device).collect()
    }

    /// Keeps one session per device: if both sides dialled at once, the connection dialled by
    /// the lower device key survives on both ends. `false` = this one lost.
    fn register_session(&self, remote: [u8; 32], dialer: [u8; 32], conn: &Connection) -> bool {
        let mut map = self.selfsync.sessions.lock();
        if let Some(old) = map.get(&remote) {
            if old.dialer < dialer {
                return false;
            }
            old.conn.close(0u32.into(), b"duplicate");
        }
        map.insert(remote, SelfSlot { dialer, conn: conn.clone() });
        true
    }

    fn drop_session(&self, remote: [u8; 32], conn: &Connection) {
        let mut map = self.selfsync.sessions.lock();
        if map.get(&remote).is_some_and(|s| s.conn.stable_id() == conn.stable_id()) {
            map.remove(&remote);
        }
    }

    async fn self_run(self: Arc<Self>, conn: Connection, dialer: bool) -> Result<(), Error> {
        let me = self.me()?;
        let epoch = me.epoch;
        let remote = *conn.remote_id().as_bytes();
        let (send, recv) = if dialer { conn.open_bi().await } else { conn.accept_bi().await }.map_err(Error::net)?;
        let mut writer = FrameWriter(send);
        let mut reader = FrameReader::new(recv);
        let relay = self.endpoint().ok().as_ref().and_then(relay_of);
        let my_auth = SelfMsg::Auth { attestation: me.attestation.clone(), relay, unlink: false };
        if dialer {
            writer.send(&my_auth).await?;
        }
        let first: SelfMsg = tokio::time::timeout(AUTH_TIMEOUT, reader.recv())
            .await
            .map_err(|_| Error::Timeout)??
            .ok_or_else(|| Error::Protocol("peer closed before auth".into()))?;
        let SelfMsg::Auth { attestation, relay: remote_relay, unlink } = first else {
            return Err(Error::Protocol("expected auth".into()));
        };
        let verdict = proto::accept_self_hello(&attestation, me.id.did(), remote, &self.tombstones());
        if let Err(e) = &verdict {
            if !dialer && matches!(e, proto::Error::Tombstoned) {
                // A removed device is told so (it has to prove it is ours first).
                writer.send(&my_auth).await?;
                writer.send(&SelfMsg::Unlinked).await?;
                writer.finish();
                let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
                return Ok(());
            }
            return Err(Error::Protocol("not one of our devices".into()));
        }
        if !dialer {
            writer.send(&my_auth).await?;
        }
        if unlink && !dialer {
            // The dialer only wants to tell us we were removed.
            let m: Option<SelfMsg> = tokio::time::timeout(AUTH_TIMEOUT, reader.recv()).await.map_err(|_| Error::Timeout)??;
            if matches!(m, Some(SelfMsg::Unlinked)) {
                self.clone().remove_self_account(epoch).await;
            }
            return Ok(());
        }
        let remote_relay = remote_relay.filter(|r| proto::valid_relay_hint(r));
        self.note_own_device(epoch, &attestation, remote, remote_relay);
        let dialer_key = if dialer { me.device } else { remote };
        if !self.register_session(remote, dialer_key, &conn) {
            return Ok(());
        }
        self.selfsync.backoff.lock().remove(&remote);
        self.log(format!("own-device sync with {} ({})", proto::device_to_text(&remote), if dialer { "dialed" } else { "accepted" }));
        let r = self.clone().self_loop(&me, remote, &mut reader, &mut writer).await;
        self.drop_session(remote, &conn);
        conn.close(0u32.into(), b"done");
        if let Err(e) = &r {
            tracing::debug!("own-device sync ended: {e}");
        }
        r
    }

    fn peer_vv_of(update_vv: &[u8]) -> Option<VersionVector> {
        VersionVector::decode(update_vv).ok()
    }

    async fn self_loop(
        self: Arc<Self>,
        me: &Arc<Me>,
        remote: [u8; 32],
        reader: &mut FrameReader,
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        let epoch = me.epoch;
        let acc = self.shared.lock().acc.clone().ok_or(Error::Locked)?;
        writer.send(&SelfMsg::Hello { docs: vec![(DOC_NAME.into(), acc.vv().encode())] }).await?;
        // What the peer is known to hold; nothing is sent until its Hello says.
        let mut sent: Option<VersionVector> = None;
        let mut changes = self.sync_tx.subscribe();
        loop {
            tokio::select! {
                m = reader.recv::<SelfMsg>() => {
                    let Some(m) = m? else { return Ok(()) };
                    match m {
                        SelfMsg::Hello { docs } => {
                            for (name, vv) in docs {
                                if name == DOC_NAME && let Some(vv) = Self::peer_vv_of(&vv) {
                                    sent = Some(vv);
                                }
                            }
                        }
                        SelfMsg::Sync { doc, vv, update, sig } => {
                            if doc != DOC_NAME {
                                continue;
                            }
                            if !chat_sig_ok(&remote, &doc, &update, &sig) {
                                return Err(Error::Protocol("bad batch signature".into()));
                            }
                            if !update.is_empty() {
                                self.apply_account_update(epoch, &update)?;
                            }
                            if let (Some(theirs), Some(s)) = (Self::peer_vv_of(&vv), sent.as_mut()) {
                                s.merge(&theirs);
                            }
                            self.touch_device(epoch, remote);
                        }
                        SelfMsg::Unlinked => {
                            self.clone().remove_self_account(epoch).await;
                            return Ok(());
                        }
                        SelfMsg::Auth { .. } => return Err(Error::Protocol("unexpected auth".into())),
                    }
                    self.push_changes(me, &acc, &mut sent, writer).await?;
                }
                _ = changes.changed() => {
                    if self.shared.lock().epoch != epoch {
                        return Ok(());
                    }
                    self.push_changes(me, &acc, &mut sent, writer).await?;
                }
            }
        }
    }

    async fn push_changes(&self, me: &Arc<Me>, acc: &Arc<AccDoc>, sent: &mut Option<VersionVector>, writer: &mut FrameWriter) -> Result<(), Error> {
        let Some(theirs) = sent.as_ref() else { return Ok(()) };
        if let Some((update, vv)) = acc.export_since(theirs) {
            let sig = crate::chat::wire::sign_batch(&me.profile.device_secret, DOC_NAME, &update);
            writer.send(&SelfMsg::Sync { doc: DOC_NAME.into(), vv: vv.encode(), update, sig }).await?;
            *sent = Some(vv);
        }
        Ok(())
    }

    fn touch_device(&self, epoch: u64, device: [u8; 32]) {
        let mut s = self.shared.lock();
        if s.epoch == epoch
            && let Some(e) = s.state.registry.iter_mut().find(|e| e.device == device)
        {
            e.last_seen = now();
        }
    }

    // ---- the dial loop --------------------------------------------------------------------

    /// Started with the endpoint: dials every other live device of the registry that has no
    /// session, now and then (and at once on `sync_kick`), with growing pauses per failing device.
    pub(crate) fn selfsync_started(self: &Arc<Self>) {
        let this = self.clone();
        self.handle.spawn(async move {
            loop {
                if this.shared.lock().endpoint.is_none() {
                    break;
                }
                this.dial_round();
                tokio::select! {
                    _ = tokio::time::sleep(sync_period()) => {}
                    _ = this.selfsync.kick.notified() => {}
                }
            }
        });
    }

    fn dial_round(self: &Arc<Self>) {
        let Ok(me) = self.me() else { return };
        let targets: Vec<([u8; 32], Option<String>)> = {
            let s = self.shared.lock();
            if s.epoch != me.epoch {
                return;
            }
            s.state.registry.iter().filter(|e| !e.removed && e.device != me.device).map(|e| (e.device, e.relay.clone())).collect()
        };
        for (device, relay) in targets {
            if self.selfsync.sessions.lock().contains_key(&device) {
                continue;
            }
            if self.selfsync.backoff.lock().get(&device).is_some_and(|(_, at)| Instant::now() < *at) {
                continue;
            }
            if !self.selfsync.dialing.lock().insert(device) {
                continue;
            }
            let this = self.clone();
            self.handle.spawn(async move {
                let r = this.clone().self_dial(device, relay).await;
                this.selfsync.dialing.lock().remove(&device);
                if r.is_err() {
                    let mut b = this.selfsync.backoff.lock();
                    let n = b.get(&device).map_or(0, |(n, _)| *n) + 1;
                    let wait = Duration::from_secs((10u64 << n.min(5)).min(300));
                    b.insert(device, (n, Instant::now() + wait));
                }
            });
        }
    }

    /// For tests: a bare `Auth` exchange with `device`, as whoever we are.
    pub(crate) async fn self_probe(self: &Arc<Self>, device: [u8; 32]) -> Result<(), Error> {
        let me = self.me()?;
        let ep = self.endpoint()?;
        let conn = tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr_for(&device, None)?, proto::SELF_ALPN))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(Error::net)?;
        let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
        let mut writer = FrameWriter(send);
        let mut reader = FrameReader::new(recv);
        writer.send(&SelfMsg::Auth { attestation: me.attestation.clone(), relay: None, unlink: false }).await?;
        let r = tokio::time::timeout(Duration::from_secs(5), reader.recv::<SelfMsg>()).await;
        conn.close(0u32.into(), b"probe");
        match r {
            Ok(Ok(Some(SelfMsg::Auth { .. }))) => Ok(()),
            _ => Err(Error::Protocol("refused".into())),
        }
    }

    /// Tells a removed device that it is (so it drops the account), if it can be reached:
    /// authenticates as one of its own devices, sends `Unlinked`, waits briefly.
    pub(crate) async fn send_unlinked(self: &Arc<Self>, device: [u8; 32], relay: Option<String>) {
        let Ok(me) = self.me() else { return };
        let Ok(ep) = self.endpoint() else { return };
        let Ok(addr) = addr_for(&device, relay.as_deref()) else { return };
        let Ok(Ok(conn)) = tokio::time::timeout(Duration::from_secs(8), ep.connect(addr, proto::SELF_ALPN)).await else { return };
        let run = async {
            let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
            let mut writer = FrameWriter(send);
            let mut reader = FrameReader::new(recv);
            let relay = relay_of(&ep);
            writer.send(&SelfMsg::Auth { attestation: me.attestation.clone(), relay, unlink: true }).await?;
            // Their Auth first: the order the other side expects, and it proves who answered.
            let _ = tokio::time::timeout(Duration::from_secs(5), reader.recv::<SelfMsg>()).await;
            writer.send(&SelfMsg::Unlinked).await?;
            writer.finish();
            let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
            Ok::<_, Error>(())
        };
        let _ = run.await;
        conn.close(0u32.into(), b"done");
    }
}

fn chat_sig_ok(device: &[u8; 32], doc: &str, update: &[u8], sig: &[u8]) -> bool {
    crate::chat::wire::verify_batch(device, doc, update, sig)
}

pub(crate) fn ensure_self_registered(registry: &mut Vec<OwnDevice>, me: &Me, label: Option<String>) {
    if !registry.iter().any(|e| e.device == me.device) {
        registry.insert(
            0,
            OwnDevice {
                device: me.device,
                attestation: me.attestation.clone(),
                label: label.unwrap_or_default(),
                removed: false,
                last_seen: now(),
                relay: None,
            },
        );
    }
}
