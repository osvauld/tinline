//! The device's one iroh endpoint and everything that rides on it: identity, contacts, and at
//! most one call at a time.
//!
//! Blocking methods are meant for the app's IO threads; long work (dialing, ringing, the call
//! itself) runs on this node's own tokio runtime and reports through `NodeEvents`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use identity::Identity;
use iroh::endpoint::{Connection, QuicTransportConfig, presets};
use iroh::{Endpoint, EndpointAddr, PublicKey, RelayUrl, SecretKey, Watcher};
use proto::{ContactTicket, Msg};
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::store::{Profile, State, Store, StoredContact};
use crate::wire::Ctrl;
use crate::Error;

const TICKET_TTL: u64 = 30 * 24 * 3600;
/// Long, because renewal happens on every answered call; this only bites a contact who never
/// calls back.
const GRANT_TTL: u64 = 365 * 24 * 3600;
const DIAL_TIMEOUT: Duration = Duration::from_secs(30);
const RING_TIMEOUT: Duration = Duration::from_secs(60);
const HELLO_TIMEOUT: Duration = Duration::from_secs(15);
/// Unauthenticated connections we hold open at once while waiting for their hello; past this
/// new ones are refused rather than queued.
const MAX_PENDING_HELLOS: usize = 32;
/// What a peer is told when we turn it away. The real reason is logged locally: telling an
/// unauthenticated stranger "revoked" vs "not a contact" would leak our contact list.
const REFUSED: &str = "not accepted";

#[uniffi::export(with_foreign)]
pub trait NodeEvents: Send + Sync {
    fn on_status(&self, status: NodeStatus);
    fn on_contacts_changed(&self);
    fn on_incoming_call(&self, call: CallInfo);
    fn on_call_state(&self, call_id: String, state: CallState);
    fn on_log(&self, line: String);
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct NodeStatus {
    pub started: bool,
    /// Connected to a home relay, i.e. reachable from anywhere.
    pub online: bool,
    pub relay: Option<String>,
    pub endpoint_id: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProfileInfo {
    pub did: String,
    pub name: String,
    pub device: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Contact {
    pub did: String,
    pub name: String,
    pub device: String,
    pub added_at: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CallInfo {
    pub call_id: String,
    pub peer_did: String,
    pub peer_name: String,
    pub incoming: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum CallState {
    Dialing,
    Ringing,
    Active,
    Ended { reason: String },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CallStats {
    pub call_id: String,
    pub state: CallState,
    pub secs: u32,
    pub direct: bool,
    pub rtt_ms: u32,
    pub sent: u64,
    pub received: u64,
    pub lost: u64,
    pub recovered: u64,
    pub concealed: u64,
    pub buffered_ms: u32,
    /// Dominant frequency of the last second of received audio; for the tone self-test.
    pub rx_freq_hz: f32,
    pub rx_rms: f32,
}

enum Cmd {
    Answer,
    Decline,
    Hangup,
}

struct Call {
    info: CallInfo,
    conn: Mutex<Option<Connection>>,
    state: Mutex<CallState>,
    cmd: mpsc::UnboundedSender<Cmd>,
    started: Instant,
    sender: Mutex<audio::Sender>,
    receiver: Mutex<audio::Receiver>,
    mic: Mutex<Vec<i16>>,
    /// The last second played, for `rx_freq_hz`.
    played: Mutex<VecDeque<i16>>,
    sent: Mutex<u64>,
    tone: Mutex<Option<(f32, audio::Tone)>>,
    /// Set once by `end_call`; later calls are no-ops, so every path may call it.
    ended: Mutex<bool>,
}

/// Ends the call when dropped unless it already ended: covers every early return (and panic)
/// between taking the slot and `run_call` finishing it.
struct SlotGuard<'a> {
    inner: &'a Inner,
    call: Arc<Call>,
}

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        self.inner.end_call(&self.call, "ended".into());
    }
}

impl Call {
    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn state(&self) -> CallState {
        self.state.lock().unwrap().clone()
    }
}

struct Me {
    profile: Profile,
    id: Identity,
    device: [u8; 32],
    attestation: proto::SignedAttestation,
}

struct Shared {
    me: Option<Arc<Me>>,
    state: State,
    endpoint: Option<Endpoint>,
}

/// The call slot, apart from `Shared` so the 50 Hz audio threads never wait behind a disk
/// write or a handshake holding `Shared`.
#[derive(Default)]
struct Live {
    call: Option<Arc<Call>>,
    tone: Option<f32>,
}

struct Inner {
    handle: tokio::runtime::Handle,
    store: Store,
    /// Held while snapshotting and writing state, so writes land in the order taken. Always
    /// taken before `shared`, never while holding it.
    writing: Mutex<()>,
    events: Arc<dyn NodeEvents>,
    shared: Mutex<Shared>,
    live: Mutex<Live>,
    pending: Arc<Semaphore>,
}

/// Blocking methods (`start`, `stop`, `add_contact`, `my_ticket`) are for the app's own
/// threads; called from inside a `NodeEvents` callback they fail (or, for `stop`, go async)
/// instead of blocking the runtime that delivers the callback.
#[derive(uniffi::Object)]
pub struct Node {
    inner: Arc<Inner>,
    rt: Option<tokio::runtime::Runtime>,
}

impl Drop for Node {
    fn drop(&mut self) {
        let ep = self.inner.shared.lock().unwrap().endpoint.take();
        if let Some(rt) = self.rt.take() {
            if let Some(ep) = ep {
                // Best effort: a clean close tells peers at once instead of at idle timeout.
                if tokio::runtime::Handle::try_current().is_err() {
                    rt.block_on(async {
                        let _ = tokio::time::timeout(Duration::from_secs(1), ep.close()).await;
                    });
                }
            }
            // Never blocks, so it is safe even if the last reference drops on a core thread.
            rt.shutdown_background();
        }
    }
}

#[uniffi::export]
impl Node {
    #[uniffi::constructor]
    pub fn new(data_dir: String, events: Arc<dyn NodeEvents>) -> Result<Arc<Self>, Error> {
        crate::logging::init();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("p2pcore")
            .enable_all()
            .build()?;
        let store = Store::open(data_dir)?;
        let state = store.state()?;
        let me = match store.profile()? {
            Some(p) => Some(Arc::new(Me::load(p)?)),
            None => None,
        };
        let inner = Arc::new(Inner {
            handle: rt.handle().clone(),
            store,
            writing: Mutex::new(()),
            events,
            shared: Mutex::new(Shared { me, state, endpoint: None }),
            live: Mutex::new(Live::default()),
            pending: Arc::new(Semaphore::new(MAX_PENDING_HELLOS)),
        });
        Ok(Arc::new(Self { inner, rt: Some(rt) }))
    }

    pub fn has_identity(&self) -> bool {
        self.inner.shared.lock().unwrap().me.is_some()
    }

    /// Returns the recovery phrase; it is also kept so `recovery_phrase` can show it again.
    pub fn create_identity(&self, name: String) -> Result<String, Error> {
        let (_, mnemonic) = identity::generate();
        let phrase = mnemonic.to_string();
        self.set_identity(phrase.clone(), name)?;
        Ok(phrase)
    }

    pub fn restore_identity(&self, phrase: String, name: String) -> Result<(), Error> {
        self.set_identity(phrase.trim().to_string(), name)
    }

    pub fn profile(&self) -> Option<ProfileInfo> {
        let s = self.inner.shared.lock().unwrap();
        s.me.as_ref().map(|me| me.info())
    }

    pub fn recovery_phrase(&self) -> Option<String> {
        let s = self.inner.shared.lock().unwrap();
        s.me.as_ref().map(|me| me.profile.mnemonic.clone())
    }

    pub fn set_name(&self, name: String) -> Result<(), Error> {
        {
            let mut s = self.inner.shared.lock().unwrap();
            let me = s.me.as_ref().ok_or(Error::NoIdentity)?;
            let mut profile = me.profile.clone();
            profile.name = name;
            self.inner.store.save_profile(&profile)?;
            s.me = Some(Arc::new(Me::load(profile)?));
            // The old ticket carries the old name.
            s.state.ticket = None;
        }
        self.inner.persist()
    }

    /// Binds the endpoint and starts accepting calls. Idempotent.
    pub fn start(&self) -> Result<(), Error> {
        let inner = self.inner.clone();
        self.block_on(inner.start())?
    }

    pub fn stop(&self) {
        let ep = self.inner.shared.lock().unwrap().endpoint.take();
        if let Some(ep) = ep {
            let close = async move {
                let _ = tokio::time::timeout(Duration::from_secs(2), ep.close()).await;
            };
            if tokio::runtime::Handle::try_current().is_ok() {
                // From a callback: can't wait here, so finish the close in the background.
                self.inner.handle.spawn(close);
            } else {
                self.inner.handle.block_on(close);
            }
        }
        self.inner.emit_status();
    }

    /// The platform saw the network change (wifi ↔ cellular); re-probe paths now rather than
    /// waiting for the next timeout.
    pub fn network_changed(&self) {
        let ep = self.inner.shared.lock().unwrap().endpoint.clone();
        if let Some(ep) = ep {
            self.inner.handle.spawn(async move { ep.network_change().await });
        }
    }

    pub fn status(&self) -> NodeStatus {
        self.inner.status()
    }

    /// Our contact ticket (`OSVC2:…`), stable until someone redeems it.
    pub fn my_ticket(&self) -> Result<String, Error> {
        self.inner.ticket()
    }

    /// Redeems someone's ticket: dials them, exchanges grants, stores them. Blocks until done.
    pub fn add_contact(&self, ticket: String) -> Result<Contact, Error> {
        let inner = self.inner.clone();
        self.block_on(inner.add_contact(ticket))?
    }

    pub fn contacts(&self) -> Vec<Contact> {
        let s = self.inner.shared.lock().unwrap();
        s.state.contacts.iter().map(Contact::from).collect()
    }

    /// Forgets them and blocks their DID, so the grant we gave them no longer rings us; any
    /// call with them ends. Adding them again lifts the block.
    pub fn remove_contact(&self, did: String) -> Result<(), Error> {
        {
            let mut s = self.inner.shared.lock().unwrap();
            s.state.contacts.retain(|c| c.did != did);
            s.state.blocked.insert(did.clone());
        }
        self.inner.persist()?;
        let call = self.inner.live.lock().unwrap().call.clone();
        if let Some(call) = call.filter(|c| c.info.peer_did == did) {
            let _ = call.cmd.send(Cmd::Hangup);
        }
        self.inner.events.on_contacts_changed();
        Ok(())
    }

    /// Starts calling `did`; progress arrives through `on_call_state`.
    pub fn call(&self, did: String) -> Result<CallInfo, Error> {
        self.inner.clone().start_call(did)
    }

    pub fn answer(&self, call_id: String) -> Result<(), Error> {
        self.inner.command(&call_id, Cmd::Answer)
    }

    pub fn decline(&self, call_id: String) -> Result<(), Error> {
        self.inner.command(&call_id, Cmd::Decline)
    }

    pub fn hangup(&self, call_id: String) -> Result<(), Error> {
        self.inner.command(&call_id, Cmd::Hangup)
    }

    pub fn current_call(&self) -> Option<CallInfo> {
        self.inner.live.lock().unwrap().call.as_ref().map(|c| c.info.clone())
    }

    /// Microphone PCM, 48 kHz mono, any length; sent in 20 ms frames while a call is active.
    pub fn push_mic(&self, pcm: Vec<i16>) {
        self.inner.push_mic(&pcm);
    }

    /// The next `audio::FRAME` samples to play; silence when no call is active.
    pub fn pull_speaker(&self) -> Vec<i16> {
        self.inner.pull_speaker()
    }

    /// `push_mic` as little-endian PCM16 bytes: a `ByteArray` on the Kotlin side rather than
    /// a boxed `List<Short>`, which would allocate 50 times a second.
    pub fn push_mic_pcm16(&self, pcm: Vec<u8>) {
        let samples: Vec<i16> = pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
        self.inner.push_mic(&samples);
    }

    /// `pull_speaker` as little-endian PCM16 bytes.
    pub fn pull_speaker_pcm16(&self) -> Vec<u8> {
        self.inner.pull_speaker().iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    pub fn call_stats(&self) -> Option<CallStats> {
        self.inner.call_stats()
    }

    /// Replace the microphone with a sine of this frequency (or `None` to stop); a self-test
    /// that does not depend on the device having a real mic.
    pub fn set_test_tone(&self, hz: Option<f32>) {
        self.inner.live.lock().unwrap().tone = hz;
    }
}

impl Me {
    fn load(profile: Profile) -> Result<Self, Error> {
        let id = identity::recover(&profile.mnemonic).map_err(|_| Error::BadPhrase)?;
        let device = proto::device_public(&profile.device_secret);
        let attestation = proto::attest(&id, device, now());
        Ok(Self { profile, id, device, attestation })
    }

    fn info(&self) -> ProfileInfo {
        ProfileInfo {
            did: self.id.did().to_string(),
            name: self.profile.name.clone(),
            device: PublicKey::from_bytes(&self.device).map(|k| k.to_string()).unwrap_or_default(),
        }
    }
}

impl From<&StoredContact> for Contact {
    fn from(c: &StoredContact) -> Self {
        Contact {
            did: c.did.clone(),
            name: c.name.clone(),
            device: c
                .devices
                .first()
                .and_then(|d| PublicKey::from_bytes(d).ok())
                .map(|k| k.to_string())
                .unwrap_or_default(),
            added_at: c.added_at,
        }
    }
}

impl Node {
    /// Runs `fut` to completion from an app thread; refuses on a core thread, where blocking
    /// would stall (or panic) the runtime that is calling us.
    fn block_on<T>(&self, fut: impl std::future::Future<Output = T>) -> Result<T, Error> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(Error::Protocol("blocking call made from a NodeEvents callback".into()));
        }
        Ok(self.inner.handle.block_on(fut))
    }

    fn set_identity(&self, phrase: String, name: String) -> Result<(), Error> {
        let mut s = self.inner.shared.lock().unwrap();
        if s.me.is_some() {
            return Err(Error::HaveIdentity);
        }
        let profile = Profile { mnemonic: phrase, name, device_secret: proto::new_device_secret() };
        let me = Me::load(profile)?;
        self.inner.store.save_profile(&me.profile)?;
        s.me = Some(Arc::new(me));
        Ok(())
    }
}

impl Inner {
    /// Writes a snapshot of `State`. Callers must not hold `shared`.
    fn persist(&self) -> Result<(), Error> {
        let _w = self.writing.lock().unwrap();
        let snapshot = self.shared.lock().unwrap().state.clone();
        self.store.save_state(&snapshot)
    }

    fn me(&self) -> Result<Arc<Me>, Error> {
        self.shared.lock().unwrap().me.clone().ok_or(Error::NoIdentity)
    }

    fn endpoint(&self) -> Result<Endpoint, Error> {
        self.shared.lock().unwrap().endpoint.clone().ok_or(Error::NotStarted)
    }

    fn log(&self, line: impl Into<String>) {
        let line = line.into();
        tracing::info!("{line}");
        self.events.on_log(line);
    }

    fn status(&self) -> NodeStatus {
        let s = self.shared.lock().unwrap();
        match &s.endpoint {
            Some(ep) => {
                let relay = ep.addr().relay_urls().next().map(|u| u.to_string());
                NodeStatus {
                    started: true,
                    online: relay.is_some(),
                    relay,
                    endpoint_id: ep.id().to_string(),
                }
            }
            None => NodeStatus {
                started: false,
                online: false,
                relay: None,
                endpoint_id: s.me.as_ref().map(|m| m.info().device).unwrap_or_default(),
            },
        }
    }

    fn emit_status(&self) {
        self.events.on_status(self.status());
    }

    async fn start(self: Arc<Self>) -> Result<(), Error> {
        let me = self.me()?;
        if self.shared.lock().unwrap().endpoint.is_some() {
            return Ok(());
        }
        let transport = QuicTransportConfig::builder()
            // Keeps a ringing call and NAT bindings alive; the relay link has its own pings.
            .keep_alive_interval(Duration::from_secs(5))
            .max_idle_timeout(Some(Duration::from_secs(20).try_into().expect("small timeout")))
            .build();
        let mut builder = Endpoint::builder(presets::N0)
            .secret_key(SecretKey::from_bytes(&me.profile.device_secret))
            .alpns(vec![proto::ALPN.to_vec()])
            .transport_config(transport);
        // Test knob: no UDP of our own, so every packet goes through the relay — the path a
        // call takes when hole punching fails.
        if std::env::var_os("P2P_RELAY_ONLY").is_some() {
            builder = builder.clear_ip_transports();
            self.log("relay-only mode");
        }
        let ep = builder.bind().await.map_err(Error::net)?;
        {
            let mut s = self.shared.lock().unwrap();
            if s.endpoint.is_some() {
                drop(s);
                ep.close().await;
                return Ok(());
            }
            s.endpoint = Some(ep.clone());
        }
        self.log(format!("endpoint {} bound", ep.id()));
        self.emit_status();

        let this = self.clone();
        let watch_ep = ep.clone();
        self.handle.spawn(async move {
            let mut addrs = watch_ep.watch_addr().stream();
            use n0_future::StreamExt;
            while let Some(_addr) = addrs.next().await {
                this.emit_status();
            }
        });

        let this = self.clone();
        self.handle.spawn(async move {
            while let Some(incoming) = ep.accept().await {
                // Bounded: a flood of strangers gets refused instead of costing a task each.
                let Ok(permit) = this.pending.clone().try_acquire_owned() else {
                    incoming.refuse();
                    continue;
                };
                let this = this.clone();
                tokio::spawn(async move {
                    if let Err(e) = this.clone().handle_incoming(incoming, permit).await {
                        tracing::debug!("incoming: {e}");
                    }
                });
            }
            this.log("endpoint closed");
        });
        Ok(())
    }

    fn ticket(&self) -> Result<String, Error> {
        let me = self.me()?;
        // Right after start the relay isn't known yet; a ticket without it leaves the joiner
        // to DNS discovery, so give the relay a few seconds to come up first.
        let relay_now = |ep: &Endpoint| ep.addr().relay_urls().next().map(|u| u.to_string());
        let relay = self.endpoint().ok().and_then(|ep| {
            let deadline = Instant::now() + Duration::from_secs(8);
            loop {
                let r = relay_now(&ep);
                if r.is_some() || Instant::now() > deadline {
                    break r;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let mut s = self.shared.lock().unwrap();
        let now = now();
        if let Some(text) = &s.state.ticket
            && let Ok(t) = ContactTicket::from_text(text)
            && let Ok(claim) = t.verify(now)
            // Still ours (identity unchanged), still days of life left, and points at our
            // current relay so the joiner can dial without discovery.
            && claim.iss == me.id.did()
            && claim.exp > now + 24 * 3600
            && (relay.is_none() || claim.relay == relay)
        {
            return Ok(text.clone());
        }
        let ticket = proto::issue_contact_ticket(
            &me.id,
            me.device,
            &me.profile.name,
            relay,
            now,
            TICKET_TTL,
        );
        let text = ticket.to_text();
        s.state.ticket = Some(text.clone());
        drop(s);
        self.persist()?;
        Ok(text)
    }

    async fn add_contact(self: Arc<Self>, text: String) -> Result<Contact, Error> {
        let me = self.me()?;
        let ep = self.endpoint()?;
        let ticket = ContactTicket::from_text(&text)?;
        let (mut hello, pending) = proto::contact_hello(
            &me.id,
            me.device,
            &ticket,
            &me.profile.name,
            now(),
            GRANT_TTL,
        )?;
        if let Msg::ContactHello { relay, .. } = &mut hello {
            *relay = relay_of(&ep);
        }
        let claim = ticket.verify(now())?;
        if claim.iss == me.id.did() {
            return Err(Error::Protocol("that is your own ticket".into()));
        }
        let addr = addr_for(&proto::device_from_text(&claim.device)?, claim.relay.as_deref())?;
        self.log(format!("adding contact {}: dialing {}", claim.name, addr.id));
        let conn = tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, proto::ALPN))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(Error::net)?;
        let remote = *conn.remote_id().as_bytes();
        let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
        let mut ctrl = Ctrl::new(send, recv);
        ctrl.send(&hello).await?;
        let reply = tokio::time::timeout(HELLO_TIMEOUT, ctrl.recv())
            .await
            .map_err(|_| Error::Timeout)??
            .ok_or_else(|| Error::Protocol("peer closed".into()))?;
        let new = match reply {
            Msg::Reject { reason } => return Err(Error::Rejected(reason)),
            other => proto::accept_contact_welcome(&me.id, &pending, &other, remote, now())?,
        };
        ctrl.finish();
        conn.close(0u32.into(), b"added");
        let contact = self.save_contact(new, claim.relay)?;
        self.log(format!("contact {} added", contact.name));
        Ok(contact)
    }

    fn save_contact(&self, new: proto::NewContact, relay: Option<String>) -> Result<Contact, Error> {
        let stored = StoredContact {
            did: new.did.clone(),
            name: new.name,
            devices: vec![new.device],
            relay,
            grant_from_them: new.grant_from_them,
            added_at: now(),
        };
        let contact = Contact::from(&stored);
        {
            let mut s = self.shared.lock().unwrap();
            s.state.blocked.remove(&new.did);
            match s.state.contacts.iter_mut().find(|c| c.did == new.did) {
                Some(existing) => {
                    existing.name = stored.name;
                    existing.devices.retain(|d| *d != new.device);
                    existing.devices.insert(0, new.device);
                    if stored.relay.is_some() {
                        existing.relay = stored.relay;
                    }
                    existing.grant_from_them = stored.grant_from_them;
                }
                None => s.state.contacts.push(stored),
            }
        }
        self.persist()?;
        self.events.on_contacts_changed();
        Ok(contact)
    }

    async fn handle_incoming(
        self: Arc<Self>,
        incoming: iroh::endpoint::Incoming,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<(), Error> {
        let conn = incoming.await.map_err(Error::net)?;
        let remote = *conn.remote_id().as_bytes();
        let (send, recv) = tokio::time::timeout(HELLO_TIMEOUT, conn.accept_bi())
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(Error::net)?;
        let mut ctrl = Ctrl::new(send, recv);
        let first = tokio::time::timeout(HELLO_TIMEOUT, ctrl.recv())
            .await
            .map_err(|_| Error::Timeout)??
            .ok_or_else(|| Error::Protocol("peer closed before hello".into()))?;
        // The hello is in; what follows is either quick or authenticated.
        drop(permit);
        let me = self.me()?;
        match first {
            hello @ Msg::ContactHello { .. } => {
                let Msg::ContactHello { relay: hint, .. } = &hello else { unreachable!() };
                let hint = relay_hint(hint);
                // Check and spend the nonce under one lock, so two hellos racing on the same
                // ticket can't both get in.
                let accepted = {
                    let mut s = self.shared.lock().unwrap();
                    let r = proto::accept_contact_hello(
                        &me.id,
                        me.device,
                        &hello,
                        remote,
                        now(),
                        &s.state.redeemed,
                        GRANT_TTL,
                    );
                    if let Ok((_, new)) = &r {
                        s.state.redeemed.insert(new.redeemed_nonce.clone());
                        // Spent: the next `my_ticket` mints a fresh one.
                        s.state.ticket = None;
                    }
                    r
                };
                match accepted {
                    Ok((welcome, new)) => {
                        self.persist()?;
                        ctrl.send(&welcome).await?;
                        ctrl.finish();
                        let contact = self.save_contact(new, hint)?;
                        self.log(format!("contact {} added us", contact.name));
                        // Let the joiner read the welcome and close first.
                        let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
                    }
                    Err(e) => {
                        self.log(format!("rejected contact hello: {e}"));
                        ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
                        ctrl.finish();
                        let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
                    }
                }
                Ok(())
            }
            hello @ Msg::CallHello { .. } => self.incoming_call(conn, ctrl, hello, remote, &me).await,
            other => {
                tracing::debug!("unexpected first message {}", msg_name(&other));
                ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
                Ok(())
            }
        }
    }

    async fn incoming_call(
        self: Arc<Self>,
        conn: Connection,
        mut ctrl: Ctrl,
        hello: Msg,
        remote: [u8; 32],
        me: &Me,
    ) -> Result<(), Error> {
        let Msg::CallHello { call_id, relay: hint, .. } = &hello else { unreachable!() };
        let (call_id, hint) = (call_id.clone(), relay_hint(hint));
        // It reaches the UI and logs: keep it to what we ourselves generate.
        if call_id.is_empty() || call_id.len() > 64 || !call_id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
            return Ok(());
        }
        let verified = {
            let s = self.shared.lock().unwrap();
            let contacts = &s.state.contacts;
            proto::accept_call_hello(
                &me.id,
                &hello,
                remote,
                now(),
                &s.state.revoked,
                |did| contacts.iter().any(|c| c.did == did) && !s.state.blocked.contains(did),
            )
        };
        let caller = match verified {
            Ok(c) => c,
            Err(e) => {
                self.log(format!("rejected call: {e}"));
                ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
                ctrl.finish();
                let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
                return Ok(());
            }
        };
        let name = {
            let s = self.shared.lock().unwrap();
            s.state.contacts.iter().find(|c| c.did == caller.did).map(|c| c.name.clone()).unwrap_or_default()
        };
        let info = CallInfo { call_id, peer_did: caller.did.clone(), peer_name: name, incoming: true };
        self.yield_on_glare(&caller.did, me.device, remote);
        let (call, cmds) = match self.begin_call(info.clone(), CallState::Ringing) {
            Ok(c) => c,
            Err(_) => {
                ctrl.send(&Msg::Busy).await?;
                ctrl.finish();
                let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
                return Ok(());
            }
        };
        // From here every exit must free the slot; the guard does it if nothing else did.
        let _slot = SlotGuard { inner: &self, call: call.clone() };
        *call.conn.lock().unwrap() = Some(conn.clone());
        // A new device for a known contact: dial it next time.
        self.note_device(&caller.did, remote, hint);
        ctrl.send(&Msg::Ringing).await?;
        self.log(format!("incoming call from {}", info.peer_name));
        self.events.on_incoming_call(info.clone());
        self.events.on_call_state(info.call_id.clone(), CallState::Ringing);
        self.clone().run_call(call, conn, ctrl, cmds, Some(caller.did)).await;
        Ok(())
    }

    /// Both sides dialled each other at once: each would answer the other's hello with Busy
    /// and both calls die. The call from the lower device key wins; the higher side drops its
    /// own outgoing call so the incoming one can take the slot.
    fn yield_on_glare(&self, peer: &str, mine: [u8; 32], theirs: [u8; 32]) {
        let ours = self.live.lock().unwrap().call.clone();
        if let Some(ours) = ours
            && !ours.info.incoming
            && ours.info.peer_did == peer
            && matches!(ours.state(), CallState::Dialing | CallState::Ringing)
            && mine > theirs
        {
            self.end_call(&ours, "they called at the same time".into());
        }
    }

    /// The caller's device and relay as of this call, so our next call to them dials straight
    /// there.
    fn note_device(&self, did: &str, device: [u8; 32], relay: Option<String>) {
        let mut s = self.shared.lock().unwrap();
        let Some(c) = s.state.contacts.iter_mut().find(|c| c.did == did) else { return };
        let mut changed = false;
        if c.devices.first() != Some(&device) {
            c.devices.retain(|d| *d != device);
            c.devices.insert(0, device);
            changed = true;
        }
        if relay.is_some() && c.relay != relay {
            c.relay = relay;
            changed = true;
        }
        drop(s);
        if changed {
            let _ = self.persist();
        }
    }

    fn begin_call(
        &self,
        info: CallInfo,
        state: CallState,
    ) -> Result<(Arc<Call>, mpsc::UnboundedReceiver<Cmd>), Error> {
        let mut live = self.live.lock().unwrap();
        if live.call.is_some() {
            return Err(Error::Busy);
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let call = Arc::new(Call {
            info,
            conn: Mutex::new(None),
            state: Mutex::new(state),
            cmd: tx,
            started: Instant::now(),
            sender: Mutex::new(audio::Sender::new(48_000)?),
            receiver: Mutex::new(audio::Receiver::new()?),
            mic: Mutex::new(Vec::with_capacity(audio::FRAME * 2)),
            played: Mutex::new(VecDeque::with_capacity(audio::RATE as usize)),
            sent: Mutex::new(0),
            tone: Mutex::new(None),
            ended: Mutex::new(false),
        });
        live.call = Some(call.clone());
        Ok((call, rx))
    }

    fn start_call(self: Arc<Self>, did: String) -> Result<CallInfo, Error> {
        let me = self.me()?;
        let ep = self.endpoint()?;
        let contact = {
            let s = self.shared.lock().unwrap();
            s.state.contacts.iter().find(|c| c.did == did).cloned().ok_or(Error::NotFound)?
        };
        let info = CallInfo {
            call_id: random_id(),
            peer_did: did,
            peer_name: contact.name.clone(),
            incoming: false,
        };
        let (call, mut cmds) = self.begin_call(info.clone(), CallState::Dialing)?;
        self.events.on_call_state(info.call_id.clone(), CallState::Dialing);
        let this = self.clone();
        self.handle.spawn(async move {
            // Hanging up while dialing must not wait out the dial timeout.
            let cancelled = async {
                loop {
                    match cmds.recv().await {
                        Some(Cmd::Answer) => continue,
                        _ => break,
                    }
                }
            };
            let dialed = tokio::select! {
                d = this.dial(&ep, &me, &contact, &call.info.call_id) => Some(d),
                _ = cancelled => None,
            };
            match dialed {
                // Ended while dialing (glare): drop the fresh connection, don't start a call.
                Some(Ok((conn, _))) if *call.ended.lock().unwrap() => conn.close(0u32.into(), b"bye"),
                Some(Ok((conn, ctrl))) => {
                    *call.conn.lock().unwrap() = Some(conn.clone());
                    this.run_call(call, conn, ctrl, cmds, None).await;
                }
                Some(Err(e)) => this.end_call(&call, format!("could not reach {}: {e}", contact.name)),
                None => this.end_call(&call, "cancelled".into()),
            }
        });
        Ok(info)
    }

    async fn dial(
        &self,
        ep: &Endpoint,
        me: &Me,
        contact: &StoredContact,
        call_id: &str,
    ) -> Result<(Connection, Ctrl), Error> {
        let mut last = Error::NotFound;
        for device in &contact.devices {
            let addr = addr_for(device, contact.relay.as_deref())?;
            self.log(format!("dialing {} at {}", contact.name, addr.id));
            let conn = match tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, proto::ALPN)).await {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => {
                    last = Error::net(e);
                    continue;
                }
                Err(_) => {
                    last = Error::Timeout;
                    continue;
                }
            };
            let (send, recv) = match conn.open_bi().await {
                Ok(s) => s,
                Err(e) => {
                    last = Error::net(e);
                    continue;
                }
            };
            let mut ctrl = Ctrl::new(send, recv);
            let mut hello = proto::call_hello(
                &me.id,
                me.attestation.clone(),
                contact.grant_from_them.clone(),
                call_id.to_string(),
            );
            if let Msg::CallHello { relay, .. } = &mut hello {
                *relay = relay_of(ep);
            }
            ctrl.send(&hello).await?;
            return Ok((conn, ctrl));
        }
        Err(last)
    }

    /// Drives one call from ringing to the end, on either side. `incoming_from` is the caller's
    /// DID when we are the callee, so the answer can carry a renewed grant.
    async fn run_call(
        self: Arc<Self>,
        call: Arc<Call>,
        conn: Connection,
        mut ctrl: Ctrl,
        mut cmds: mpsc::UnboundedReceiver<Cmd>,
        incoming_from: Option<String>,
    ) {
        let ring_deadline = tokio::time::Instant::now() + RING_TIMEOUT;
        let mut datagrams: Option<oneshot::Sender<()>> = None;
        let reason = loop {
            if *call.ended.lock().unwrap() {
                break "ended".into();
            }
            let active = call.state() == CallState::Active;
            tokio::select! {
                msg = ctrl.recv() => match msg {
                    Ok(Some(Msg::Ringing)) if !active => self.set_state(&call, CallState::Ringing),
                    Ok(Some(Msg::Accept { renewed_grant })) if incoming_from.is_none() && !active => {
                        if let Some(g) = renewed_grant {
                            self.renew_grant(&call.info.peer_did, g);
                        }
                        datagrams = Some(self.start_media(&call, &conn));
                        self.set_state(&call, CallState::Active);
                    }
                    Ok(Some(Msg::Decline { reason })) => break format!("declined: {reason}"),
                    Ok(Some(Msg::Busy)) => break "busy".to_string(),
                    Ok(Some(Msg::Reject { reason })) => break format!("rejected: {reason}"),
                    Ok(Some(Msg::Hangup)) | Ok(None) => {
                        break if active { "hung up".into() } else if incoming_from.is_some() { "missed".into() } else { "ended".into() };
                    }
                    Ok(Some(other)) => self.log(format!("ignoring {} mid-call", msg_name(&other))),
                    Err(e) => break format!("connection lost: {e}"),
                },
                cmd = cmds.recv() => match cmd {
                    Some(Cmd::Answer) if incoming_from.is_some() && !active => {
                        let me = match self.me() { Ok(m) => m, Err(e) => break e.to_string() };
                        let renewed = incoming_from.as_deref().map(|did| proto::issue_grant(&me.id, did, now(), GRANT_TTL));
                        if let Err(e) = ctrl.send(&Msg::Accept { renewed_grant: renewed }).await {
                            break format!("connection lost: {e}");
                        }
                        datagrams = Some(self.start_media(&call, &conn));
                        self.set_state(&call, CallState::Active);
                    }
                    Some(Cmd::Answer) => {}
                    // Not yet answered: the callee refusing is a decline, the caller giving up
                    // is a hangup (the callee shows it as missed).
                    Some(Cmd::Decline) | Some(Cmd::Hangup) if !active && incoming_from.is_some() => {
                        let _ = ctrl.send(&Msg::Decline { reason: "declined".into() }).await;
                        break "declined".into();
                    }
                    Some(Cmd::Decline) | Some(Cmd::Hangup) if !active => {
                        let _ = ctrl.send(&Msg::Hangup).await;
                        break "cancelled".into();
                    }
                    Some(Cmd::Decline) | Some(Cmd::Hangup) | None => {
                        let _ = ctrl.send(&Msg::Hangup).await;
                        break "hung up".into();
                    }
                },
                why = conn.closed() => break format!("connection lost: {why}"),
                _ = tokio::time::sleep_until(ring_deadline), if !active => {
                    let _ = ctrl.send(&Msg::Hangup).await;
                    break if incoming_from.is_some() { "missed".into() } else { "no answer".into() };
                }
            }
        };
        ctrl.finish();
        drop(datagrams);
        // Free the slot now, so a call placed right after this one isn't Busy; the connection
        // gets a moment in the background for the final frame to leave.
        call.conn.lock().unwrap().take();
        self.end_call(&call, reason);
        tokio::spawn(async move {
            let _ = tokio::time::timeout(Duration::from_millis(300), conn.closed()).await;
            conn.close(0u32.into(), b"bye");
        });
    }

    /// Spawns the datagram reader; dropping the returned sender stops it.
    fn start_media(&self, call: &Arc<Call>, conn: &Connection) -> oneshot::Sender<()> {
        let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
        let call = call.clone();
        let conn = conn.clone();
        self.handle.spawn(async move {
            loop {
                tokio::select! {
                    d = conn.read_datagram() => match d {
                        Ok(d) => call.receiver.lock().unwrap().push(&d, call.now_ms()),
                        Err(_) => break,
                    },
                    _ = &mut stop_rx => break,
                }
            }
        });
        stop_tx
    }

    fn renew_grant(&self, did: &str, grant: proto::SignedGrant) {
        let found = {
            let mut s = self.shared.lock().unwrap();
            s.state.contacts.iter_mut().find(|c| c.did == did).map(|c| c.grant_from_them = grant).is_some()
        };
        if found {
            let _ = self.persist();
        }
    }

    fn set_state(&self, call: &Call, state: CallState) {
        *call.state.lock().unwrap() = state.clone();
        self.log(format!("call {}: {state:?}", call.info.call_id));
        self.events.on_call_state(call.info.call_id.clone(), state);
    }

    /// Frees the slot, closes the connection and emits Ended — once per call, however many
    /// paths get here.
    fn end_call(&self, call: &Arc<Call>, reason: String) {
        if std::mem::replace(&mut *call.ended.lock().unwrap(), true) {
            return;
        }
        {
            let mut live = self.live.lock().unwrap();
            if live.call.as_ref().is_some_and(|c| Arc::ptr_eq(c, call)) {
                live.call = None;
            }
        }
        if let Some(conn) = call.conn.lock().unwrap().take() {
            conn.close(0u32.into(), b"bye");
        }
        self.set_state(call, CallState::Ended { reason });
    }

    fn command(&self, call_id: &str, cmd: Cmd) -> Result<(), Error> {
        let live = self.live.lock().unwrap();
        let call = live.call.as_ref().filter(|c| c.info.call_id == call_id).ok_or(Error::NotFound)?;
        call.cmd.send(cmd).map_err(|_| Error::NotFound)
    }

    fn active_call(&self) -> Option<(Arc<Call>, Option<f32>)> {
        let live = self.live.lock().unwrap();
        let call = live.call.clone()?;
        (call.state() == CallState::Active).then_some((call, live.tone))
    }

    fn push_mic(&self, pcm: &[i16]) {
        let Some((call, tone)) = self.active_call() else { return };
        let Some(conn) = call.conn.lock().unwrap().clone() else { return };
        let mut mic = call.mic.lock().unwrap();
        mic.extend_from_slice(pcm);
        let mut frame = [0i16; audio::FRAME];
        while mic.len() >= audio::FRAME {
            frame.copy_from_slice(&mic[..audio::FRAME]);
            mic.drain(..audio::FRAME);
            if let Some(hz) = tone {
                let mut t = call.tone.lock().unwrap();
                if t.as_ref().is_none_or(|(f, _)| *f != hz) {
                    *t = Some((hz, audio::Tone::new(hz, 10_000.0)));
                }
                t.as_mut().unwrap().1.next_frame(&mut frame);
            }
            let datagram = match call.sender.lock().unwrap().encode(&frame) {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("encode: {e}");
                    continue;
                }
            };
            if conn.send_datagram(Bytes::from(datagram)).is_ok() {
                *call.sent.lock().unwrap() += 1;
            }
        }
    }

    fn pull_speaker(&self) -> Vec<i16> {
        let mut out = [0i16; audio::FRAME];
        if let Some((call, _)) = self.active_call() {
            call.receiver.lock().unwrap().pull(&mut out, call.now_ms());
            let mut played = call.played.lock().unwrap();
            played.extend(out.iter().copied());
            let excess = played.len().saturating_sub(audio::RATE as usize);
            played.drain(..excess);
        }
        out.to_vec()
    }

    fn call_stats(&self) -> Option<CallStats> {
        let call = self.live.lock().unwrap().call.clone()?;
        let rx = call.receiver.lock().unwrap().stats();
        let (direct, rtt_ms) = call
            .conn
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|c| {
                c.paths()
                    .iter()
                    .find(|p| p.is_selected())
                    .map(|p| (p.is_ip(), p.rtt().as_millis() as u32))
            })
            .unwrap_or((false, 0));
        let played: Vec<i16> = call.played.lock().unwrap().iter().copied().collect();
        Some(CallStats {
            call_id: call.info.call_id.clone(),
            state: call.state(),
            secs: call.started.elapsed().as_secs() as u32,
            direct,
            rtt_ms,
            sent: *call.sent.lock().unwrap(),
            received: rx.received,
            lost: rx.lost,
            recovered: rx.recovered_fec + rx.recovered_dred,
            concealed: rx.concealed,
            buffered_ms: rx.buffered_ms as u32,
            rx_freq_hz: audio::estimate_frequency(&played).unwrap_or(0.0),
            rx_rms: audio::rms(&played),
        })
    }
}

/// An unsigned relay hint from a peer, kept only if it is a sane relay URL: it gets stored
/// and dialled, so junk must not get in.
fn relay_hint(hint: &Option<String>) -> Option<String> {
    hint.as_deref()
        .filter(|h| h.len() <= 200 && h.starts_with("https://"))
        .and_then(|h| h.parse::<RelayUrl>().ok())
        .map(|u| u.to_string())
}

fn relay_of(ep: &Endpoint) -> Option<String> {
    ep.addr().relay_urls().next().map(|u| u.to_string())
}

fn addr_for(device: &[u8; 32], relay: Option<&str>) -> Result<EndpointAddr, Error> {
    let id = PublicKey::from_bytes(device).map_err(|_| Error::Protocol("bad device key".into()))?;
    let mut addr = EndpointAddr::new(id);
    if let Some(url) = relay.and_then(|r| r.parse::<RelayUrl>().ok()) {
        addr = addr.with_relay_url(url);
    }
    Ok(addr)
}

fn msg_name(m: &Msg) -> &'static str {
    match m {
        Msg::ContactHello { .. } => "ContactHello",
        Msg::ContactWelcome { .. } => "ContactWelcome",
        Msg::Reject { .. } => "Reject",
        Msg::CallHello { .. } => "CallHello",
        Msg::Ringing => "Ringing",
        Msg::Accept { .. } => "Accept",
        Msg::Decline { .. } => "Decline",
        Msg::Busy => "Busy",
        Msg::Hangup => "Hangup",
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn random_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}
