//! Linking a device (docs/design/device-linking.md §1-§2) over `tinline/link/1`.
//!
//! Roles: E = the device that already has the account, N = the one that gets it; either may show
//! the QR (the displayer listens, the scanner dials). One session at a time. The QR is single
//! use: the first connection to a displayed QR consumes it, whatever happens next.

use std::sync::Arc;
use std::time::{Duration, Instant};

use iroh::endpoint::{Accepting, Connection, Endpoint, RecvStream, SendStream};
use proto::link::{self as pl, LinkMsg, LinkQr, LinkRole};
use tokio::sync::mpsc;

use crate::node::{Inner, Me, addr_for, now, relay_of};
use crate::store::{Disk, OwnDevice, ProfileV2, State};
use crate::vault::{self, Secrets};
use crate::Error;

const HELLO_TIMEOUT: Duration = Duration::from_secs(15);
const DONE_TIMEOUT: Duration = Duration::from_secs(60);
const APPROVE_TIMEOUT: Duration = Duration::from_secs(300);

pub(crate) enum LinkCmd {
    Approve,
    Cancel,
}

/// Everything one link session needs once its peer is connected.
pub(crate) struct LinkRun {
    id: u64,
    is_e: bool,
    scanner: bool,
    qr: LinkQr,
    my_device: [u8; 32],
    label: String,
    rx: mpsc::UnboundedReceiver<LinkCmd>,
    /// N without an account yet: the key its endpoint (and later the account) uses.
    fresh: Option<([u8; 32], Endpoint)>,
    /// E: the unlocked session it started in.
    epoch: u64,
}

pub(crate) struct LinkSlot {
    id: u64,
    tx: mpsc::UnboundedSender<LinkCmd>,
    /// A displayed QR nobody has connected to yet.
    pending: Option<(LinkRun, Instant)>,
}

#[derive(Default)]
pub(crate) struct LinkState {
    pub slot: parking_lot::Mutex<Option<LinkSlot>>,
    pub next_id: std::sync::atomic::AtomicU64,
}

fn ttl_secs() -> u64 {
    std::env::var("P2P_LINK_TTL_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(pl::LINK_TTL_SECS)
}

struct LinkIo {
    send: SendStream,
    recv: RecvStream,
    buf: Vec<u8>,
}

impl LinkIo {
    async fn send(&mut self, m: &LinkMsg) -> Result<(), Error> {
        self.send.write_all(&pl::encode_link_frame(m)).await.map_err(Error::net)
    }

    async fn recv(&mut self) -> Result<LinkMsg, Error> {
        let mut chunk = [0u8; 4096];
        loop {
            if let Some((m, used)) = pl::decode_link_frame(&self.buf)? {
                self.buf.drain(..used);
                return Ok(m);
            }
            match self.recv.read(&mut chunk).await.map_err(Error::net)? {
                Some(n) => self.buf.extend_from_slice(&chunk[..n]),
                None => return Err(Error::Protocol("closed".into())),
            }
        }
    }
}

impl Inner {
    fn reserve_link(&self) -> Result<(u64, mpsc::UnboundedSender<LinkCmd>, mpsc::UnboundedReceiver<LinkCmd>), Error> {
        let mut slot = self.link.slot.lock();
        if slot.is_some() {
            return Err(Error::Protocol("a link is already in progress".into()));
        }
        let id = self.link.next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let (tx, rx) = mpsc::unbounded_channel();
        *slot = Some(LinkSlot { id, tx: tx.clone(), pending: None });
        Ok((id, tx, rx))
    }

    fn release_link(&self, id: u64) {
        let mut slot = self.link.slot.lock();
        if slot.as_ref().is_some_and(|s| s.id == id) {
            *slot = None;
        }
    }

    pub(crate) fn link_cancel(&self) {
        if let Some(s) = self.link.slot.lock().as_ref() {
            let _ = s.tx.send(LinkCmd::Cancel);
        }
        // A QR nobody scanned yet has no running task to hear it.
        let mut slot = self.link.slot.lock();
        if slot.as_ref().is_some_and(|s| s.pending.is_some()) {
            *slot = None;
        }
    }

    pub(crate) fn link_approve_cmd(&self) -> Result<(), Error> {
        match self.link.slot.lock().as_ref() {
            Some(s) if s.pending.is_none() => s.tx.send(LinkCmd::Approve).map_err(|_| Error::NotFound),
            _ => Err(Error::NotFound),
        }
    }

    fn fail(&self, id: u64, reason: &str) {
        self.release_link(id);
        self.log(format!("link failed: {reason}"));
        if let Some(ev) = self.link_events() {
            ev.on_link_failed(reason.to_string());
        }
    }

    // ---- E -------------------------------------------------------------------------------

    /// E shows a QR and waits for N to scan it.
    pub(crate) fn link_show_qr(self: &Arc<Self>) -> Result<String, Error> {
        let me = self.me()?;
        let ep = self.endpoint()?;
        let (id, _tx, rx) = self.reserve_link()?;
        let qr = pl::new_link_qr(me.device, relay_of(&ep), now(), ttl_secs());
        let text = qr.to_text();
        let label = self.shared.lock().device_label.clone().unwrap_or_default();
        let run = LinkRun { id, is_e: true, scanner: false, qr, my_device: me.device, label, rx, fresh: None, epoch: me.epoch };
        self.arm_pending(id, run);
        Ok(text)
    }

    /// E scans N's QR and dials it.
    pub(crate) fn link_scan(self: &Arc<Self>, text: &str) -> Result<(), Error> {
        let me = self.me()?;
        let ep = self.endpoint()?;
        let qr = LinkQr::from_text(text)?;
        if qr.is_expired(now()) {
            return Err(Error::Protocol("that code has expired".into()));
        }
        let (id, _tx, rx) = self.reserve_link()?;
        let label = self.shared.lock().device_label.clone().unwrap_or_default();
        let run = LinkRun { id, is_e: true, scanner: true, qr, my_device: me.device, label, rx, fresh: None, epoch: me.epoch };
        self.spawn_scan(ep, run);
        Ok(())
    }

    // ---- N -------------------------------------------------------------------------------

    /// N (no account on this device yet) shows a QR.
    pub(crate) fn link_new_show_qr(self: &Arc<Self>, label: String, secret: [u8; 32], ep: Endpoint) -> Result<String, Error> {
        self.require_no_account()?;
        let (id, _tx, rx) = self.reserve_link()?;
        let device = proto::device_public(&secret);
        let qr = pl::new_link_qr(device, relay_of(&ep), now(), ttl_secs());
        let text = qr.to_text();
        let run = LinkRun { id, is_e: false, scanner: false, qr, my_device: device, label: proto::sanitize_name(&label), rx, fresh: Some((secret, ep.clone())), epoch: 0 };
        self.arm_pending(id, run);
        // The temporary endpoint takes the one connection the QR allows.
        let this = self.clone();
        self.handle.spawn(async move {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(ttl_secs() + 5);
            if let Ok(Some(incoming)) = tokio::time::timeout_at(deadline, ep.accept()).await
                && let Ok(accepting) = incoming.accept()
            {
                let _ = this.clone().handle_link_incoming(accepting).await;
            }
        });
        Ok(text)
    }

    /// N scans E's QR.
    pub(crate) fn link_new_scan(self: &Arc<Self>, text: &str, label: String, secret: [u8; 32], ep: Endpoint) -> Result<(), Error> {
        self.require_no_account()?;
        let qr = LinkQr::from_text(text)?;
        if qr.is_expired(now()) {
            return Err(Error::Protocol("that code has expired".into()));
        }
        let (id, _tx, rx) = self.reserve_link()?;
        let device = proto::device_public(&secret);
        let run = LinkRun { id, is_e: false, scanner: true, qr, my_device: device, label: proto::sanitize_name(&label), rx, fresh: Some((secret, ep.clone())), epoch: 0 };
        self.spawn_scan(ep, run);
        Ok(())
    }

    pub(crate) fn require_no_account(&self) -> Result<(), Error> {
        if self.shared.lock().disk.is_some() {
            return Err(Error::HaveIdentity);
        }
        Ok(())
    }

    // ---- shared ----------------------------------------------------------------------------

    fn arm_pending(self: &Arc<Self>, id: u64, run: LinkRun) {
        let deadline = Instant::now() + Duration::from_secs(ttl_secs());
        if let Some(s) = self.link.slot.lock().as_mut() {
            s.pending = Some((run, deadline));
        }
        // An unscanned QR dies with its time.
        let this = self.clone();
        self.handle.spawn(async move {
            tokio::time::sleep(Duration::from_secs(ttl_secs()) + Duration::from_millis(200)).await;
            let expired = {
                let mut slot = this.link.slot.lock();
                match slot.as_ref() {
                    Some(s) if s.id == id && s.pending.is_some() => {
                        *slot = None;
                        true
                    }
                    _ => false,
                }
            };
            if expired {
                this.log("link failed: expired");
                if let Some(ev) = this.link_events() {
                    ev.on_link_failed("expired".into());
                }
            }
        });
    }

    fn spawn_scan(self: &Arc<Self>, ep: Endpoint, run: LinkRun) {
        let this = self.clone();
        self.handle.spawn(async move {
            let id = run.id;
            let relay = run.qr.relay.clone();
            let addr = match addr_for(&run.qr.device, relay.as_deref()) {
                Ok(a) => a,
                Err(_) => return this.fail(id, "rejected"),
            };
            let conn = match tokio::time::timeout(Duration::from_secs(20), ep.connect(addr, pl::LINK_ALPN)).await {
                Ok(Ok(c)) => c,
                Ok(Err(_)) => return this.fail(id, "net"),
                Err(_) => return this.fail(id, "timeout"),
            };
            this.link_session(run, conn).await;
        });
    }

    /// The displayer's side of a new connection: consumes the QR.
    pub(crate) async fn handle_link_incoming(self: Arc<Self>, accepting: Accepting) -> Result<(), Error> {
        let conn = tokio::time::timeout(HELLO_TIMEOUT, accepting).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?;
        let run = {
            let mut slot = self.link.slot.lock();
            match slot.as_mut().and_then(|s| s.pending.take()) {
                Some((run, deadline)) if Instant::now() < deadline && !run.qr.is_expired(now()) => Some(run),
                Some((run, _)) => {
                    // Expired: the QR is spent, say so.
                    let id = run.id;
                    *slot = None;
                    drop(slot);
                    self.fail(id, "expired");
                    None
                }
                None => None,
            }
        };
        match run {
            Some(run) => {
                self.link_session(run, conn).await;
                Ok(())
            }
            None => {
                conn.close(0u32.into(), b"rejected");
                Ok(())
            }
        }
    }

    async fn link_session(self: Arc<Self>, mut run: LinkRun, conn: Connection) {
        let id = run.id;
        let r = self.clone().link_inner(&mut run, &conn).await;
        // N's temporary endpoint is no longer needed (the account has the same device key).
        match r {
            Ok(()) => {
                self.release_link(id);
                let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
            }
            Err(tok) => {
                conn.close(0u32.into(), b"rejected");
                self.fail(id, tok);
            }
        }
        if let Some((_, ep)) = run.fresh.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), ep.close()).await;
        }
    }

    async fn link_inner(self: Arc<Self>, run: &mut LinkRun, conn: &Connection) -> Result<(), &'static str> {
        let remote = *conn.remote_id().as_bytes();
        let (e_dev, n_dev) = if run.is_e { (run.my_device, remote) } else { (remote, run.my_device) };
        let secret = run.qr.secret;
        // The QR names the displayer's device; the transport must agree.
        if run.scanner && remote != run.qr.device {
            return Err("mismatch");
        }
        let (send, recv) = if run.scanner { conn.open_bi().await } else { conn.accept_bi().await }.map_err(|_| "net")?;
        let mut io = LinkIo { send, recv, buf: Vec::new() };
        let (my_role, their_role) = if run.scanner { (LinkRole::Scanner, LinkRole::Displayer) } else { (LinkRole::Displayer, LinkRole::Scanner) };
        let mine = pl::link_hello(&secret, &e_dev, &n_dev, my_role, &run.my_device, &run.label);
        let theirs = if run.scanner {
            io.send(&mine).await.map_err(|_| "net")?;
            tokio::time::timeout(HELLO_TIMEOUT, io.recv()).await.map_err(|_| "timeout")?.map_err(|_| "rejected")?
        } else {
            let m = tokio::time::timeout(HELLO_TIMEOUT, io.recv()).await.map_err(|_| "timeout")?.map_err(|_| "rejected")?;
            if pl::accept_link_hello(&secret, &e_dev, &n_dev, their_role, &remote, &m).is_err() {
                let _ = io.send(&LinkMsg::Reject { reason: "rejected".into() }).await;
                return Err("bad_proof");
            }
            io.send(&mine).await.map_err(|_| "net")?;
            m
        };
        if run.scanner && pl::accept_link_hello(&secret, &e_dev, &n_dev, their_role, &remote, &theirs).is_err() {
            return Err(if matches!(theirs, LinkMsg::Reject { .. }) { "rejected" } else { "bad_proof" });
        }
        let LinkMsg::LinkHello { label: peer_label, .. } = &theirs else { return Err("rejected") };
        let code = pl::confirm_code(&secret, &e_dev, &n_dev);
        if let Some(ev) = self.link_events() {
            ev.on_link_code(code, peer_label.clone());
        }
        if run.is_e {
            self.link_e_finish(run, &mut io, remote, &secret, &e_dev, &n_dev).await
        } else {
            self.link_n_finish(run, &mut io, &secret, &e_dev, &n_dev).await
        }
    }

    async fn link_e_finish(
        self: &Arc<Self>,
        run: &mut LinkRun,
        io: &mut LinkIo,
        remote: [u8; 32],
        secret: &[u8; 16],
        e_dev: &[u8; 32],
        n_dev: &[u8; 32],
    ) -> Result<(), &'static str> {
        let deadline = tokio::time::Instant::now() + APPROVE_TIMEOUT;
        tokio::select! {
            c = run.rx.recv() => {
                if !matches!(c, Some(LinkCmd::Approve)) {
                    let _ = io.send(&LinkMsg::Reject { reason: "rejected".into() }).await;
                    return Err("cancelled");
                }
            }
            // The peer gave up (or sent nonsense) while we waited for the user.
            _ = io.recv() => return Err("rejected"),
            _ = tokio::time::sleep_until(deadline) => {
                let _ = io.send(&LinkMsg::Reject { reason: "rejected".into() }).await;
                return Err("timeout");
            }
        }
        let me = self.me().map_err(|_| "locked")?;
        if me.epoch != run.epoch {
            return Err("locked");
        }
        let registry = {
            let s = self.shared.lock();
            if s.epoch != me.epoch {
                return Err("locked");
            }
            s.state
                .registry
                .iter()
                .map(|e| pl::RegistryEntry { attestation: e.attestation.clone(), label: e.label.clone(), removed: e.removed })
                .collect::<Vec<_>>()
        };
        let body = pl::LinkGrantBody { mnemonic: me.profile.mnemonic.clone(), account_name: me.profile.name.clone(), registry };
        let grant = pl::seal_link_grant_body(secret, e_dev, n_dev, &body);
        io.send(&grant).await.map_err(|_| "net")?;
        let done = tokio::time::timeout(DONE_TIMEOUT, io.recv()).await.map_err(|_| "timeout")?.map_err(|_| "rejected")?;
        let linked = pl::accept_link_done(&done, me.id.did(), remote).map_err(|_| "mismatch")?;
        let LinkMsg::LinkDone { attestation, .. } = done else { return Err("rejected") };
        let relay = if run.scanner { run.qr.relay.clone() } else { None };
        {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return Err("locked");
            }
            match s.state.registry.iter_mut().find(|e| e.device == remote) {
                Some(e) => {
                    e.attestation = attestation;
                    e.label = linked.label.clone();
                    e.removed = false;
                    e.last_seen = now();
                }
                None => s.state.registry.push(OwnDevice { device: remote, attestation, label: linked.label.clone(), removed: false, last_seen: now(), relay }),
            }
        }
        let _ = self.persist_for(Some(me.epoch));
        self.log(format!("linked device {}", proto::device_to_text(&remote)));
        if let Some(ev) = self.link_events() {
            ev.on_devices_changed();
            ev.on_link_done(me.id.did().to_string(), me.profile.name.clone());
        }
        self.sync_kick();
        Ok(())
    }

    async fn link_n_finish(
        self: &Arc<Self>,
        run: &mut LinkRun,
        io: &mut LinkIo,
        secret: &[u8; 16],
        e_dev: &[u8; 32],
        n_dev: &[u8; 32],
    ) -> Result<(), &'static str> {
        let deadline = tokio::time::Instant::now() + APPROVE_TIMEOUT;
        let msg = loop {
            tokio::select! {
                c = run.rx.recv() => {
                    if !matches!(c, Some(LinkCmd::Approve)) {
                        return Err("cancelled");
                    }
                }
                m = io.recv() => break m.map_err(|_| "rejected")?,
                _ = tokio::time::sleep_until(deadline) => return Err("timeout"),
            }
        };
        if matches!(msg, LinkMsg::Reject { .. }) {
            return Err("rejected");
        }
        let body = pl::open_link_grant_body(secret, e_dev, n_dev, &msg).map_err(|_| "bad_grant")?;
        let (device_secret, _) = run.fresh.as_ref().ok_or("rejected")?;
        let device_secret = *device_secret;
        let me = self.create_linked_account(&body, device_secret, &run.label, run.qr.relay.clone().filter(|_| run.scanner), e_dev)?;
        io.send(&LinkMsg::LinkDone { attestation: me.attestation.clone(), label: run.label.clone() }).await.map_err(|_| "net")?;
        self.acc_open();
        self.log(format!("linked as {}", me.id.did()));
        if let Some(ev) = self.link_events() {
            ev.on_link_done(me.id.did().to_string(), me.profile.name.clone());
        }
        Ok(())
    }

    /// The account of a `LinkGrant`, created like `create_identity` without a passphrase: in
    /// memory, uncommitted, with this install's own device key and data key. The registry comes
    /// from the grant plus this device.
    fn create_linked_account(
        &self,
        body: &pl::LinkGrantBody,
        device_secret: [u8; 32],
        label: &str,
        e_relay: Option<String>,
        e_dev: &[u8; 32],
    ) -> Result<Arc<Me>, &'static str> {
        let _gate = self.gate.lock();
        if self.shared.lock().disk.is_some() {
            return Err("busy");
        }
        let id = identity::recover(body.mnemonic.trim()).map_err(|_| "bad_grant")?;
        let did = id.did().to_string();
        let mut registry = Vec::new();
        for r in &body.registry {
            let a = proto::verify_attestation(&r.attestation).map_err(|_| "bad_grant")?;
            if a.did != did {
                return Err("mismatch");
            }
            let device = a.device_key().map_err(|_| "bad_grant")?;
            registry.push(OwnDevice {
                device,
                attestation: r.attestation.clone(),
                label: proto::sanitize_name(&r.label),
                removed: r.removed,
                last_seen: 0,
                relay: if &device == e_dev { e_relay.clone() } else { None },
            });
        }
        if !registry.iter().any(|e| &e.device == e_dev && !e.removed) {
            return Err("mismatch");
        }
        let dir_id = crate::accounts::id_of(&did).map_err(|_| "bad_grant")?;
        if self.accounts.exists(&dir_id).map_err(|_| "rejected")? {
            return Err("account_exists");
        }
        let profile = crate::store::Profile { mnemonic: body.mnemonic.trim().to_string(), name: proto::sanitize_name(&body.account_name), device_secret };
        let mut me = Me::load(profile, 0).map_err(|_| "bad_grant")?;
        let secrets = Secrets { mnemonic: me.profile.mnemonic.clone(), device_secret };
        let (vault, dek) = vault::seal(&secrets, None).map_err(|_| "rejected")?;
        let disk = Disk::V2(ProfileV2 { version: 2, name: me.profile.name.clone(), did: did.clone(), device_public: me.device, vault });
        registry.retain(|e| e.device != me.device);
        registry.insert(0, OwnDevice { device: me.device, attestation: me.attestation.clone(), label: label.to_string(), removed: false, last_seen: now(), relay: None });
        let state = State { registry, ..State::default() };
        let me = {
            let mut s = self.shared.lock();
            s.epoch += 1;
            me.epoch = s.epoch;
            s.history = Arc::new(crate::store::History::new(Vec::new(), None));
            s.state = state;
            s.device_label = Some(label.to_string()).filter(|l| !l.is_empty());
            s.committed = false;
            s.acct = None;
            s.disk = Some(disk);
            let me = Arc::new(me);
            s.me = Some(me.clone());
            s.dek = Some(dek);
            me
        };
        // Same sequence as `create_identity`'s end: the chat store and the account doc.
        // (Both wait for the commit; this is for an identity that already has files.)
        Ok(me)
    }
}
