//! The device's one iroh endpoint and everything that rides on it: identity, contacts, and at
//! most one call at a time.
//!
//! Blocking methods are meant for the app's IO threads; long work (dialing, ringing, the call
//! itself) runs on this node's own tokio runtime and reports through `NodeEvents`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use identity::Identity;
use iroh::endpoint::{Connection, QuicTransportConfig, presets};
use iroh::{Endpoint, EndpointAddr, PublicKey, RelayUrl, SecretKey, Watcher};
use parking_lot::{Mutex, ReentrantMutex};
use proto::{ContactTicket, Msg};
use sha2::{Digest, Sha512};
use tokio::sync::{OwnedMutexGuard, OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use zeroize::Zeroize;

use crate::accounts::{self, AccountDirs};
use crate::store::{self, Backing, CallRecord, ContactDevice, Disk, History, OwnDevice, Profile, ProfileV2, State, Store, StoredContact};
use crate::rekey;
use crate::vault::{self, Dek, Secrets};
use crate::wire::Ctrl;
use crate::Error;

/// Short, because a ticket is a bearer secret until someone redeems it.
const TICKET_TTL: u64 = 7 * 24 * 3600;
/// Long, because renewal happens on every answered call; this only bites a contact who never
/// calls back.
const GRANT_TTL: u64 = 365 * 24 * 3600;
const DIAL_TIMEOUT: Duration = Duration::from_secs(30);
/// All devices of one contact together; each device alone gets at most `DIAL_TIMEOUT`.
const DIAL_TOTAL: Duration = Duration::from_secs(45);
/// Devices remembered per contact: what a device list may hold.
const MAX_DEVICES: usize = proto::MAX_LIST_DEVICES;
/// An active call that receives no media for this long is over.
const NO_AUDIO_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a turned-away connection gets to read our refusal before we let go of its slot.
const LINGER: Duration = Duration::from_secs(2);
/// Longest peer-supplied reason we pass on to the UI.
const MAX_REASON_CHARS: usize = 100;
const RING_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a second incoming call (one that arrives during a call) waits to be answered.
/// `P2P_WAITING_RING_SECS` overrides it, for tests.
const WAITING_RING_TIMEOUT: Duration = Duration::from_secs(30);
/// An active call with no media for this long shows as `reconnecting` (a path change or a
/// network switch), well before the no-audio timeout ends it.
const RECONNECT_AFTER: Duration = Duration::from_millis(1500);
/// Longest local alias for a contact, in characters (same cap as names from peers).
const MAX_ALIAS_CHARS: usize = 64;
/// One deadline for a connection to get from first packet to hello, and for the reply to the
/// hello we send when adding a contact.
const HELLO_TIMEOUT: Duration = Duration::from_secs(15);
/// Connections from strangers we hold open at once until they are accepted; past this new ones
/// are refused rather than queued. A slot is held for the whole life of a connection that has
/// not become a call or a contact, refusals and their linger included.
const MAX_PENDING_HELLOS: usize = 24;
/// Extra slots only for devices of our contacts, so a flood of strangers cannot lock them out.
/// (Whose device it is is only known once the handshake is done, so a stranger can hold one of
/// these until then; it moves to the general pool or is dropped right after.)
const RESERVED_HELLOS: usize = 8;
/// What a peer is told when we turn it away. The real reason is logged locally: telling an
/// unauthenticated stranger "revoked" vs "not a contact" would leak our contact list.
const REFUSED: &str = "not accepted";

/// `on_call_state` with `Ended` is also the moment the call history has changed: the UI should
/// re-read `recent_calls` / `calls_with` then (there is no separate history callback).
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

/// One account on this device, as the lock screen and the switcher list it. Read from the clear
/// `account.json`, so it is available while everything is locked.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AccountSummary {
    pub did: String,
    pub name: String,
    /// The account `has_identity`, `unlock` and the rest act on.
    pub current: bool,
    pub has_passphrase: bool,
}

/// Whether the identity's secrets are available. `Locked` needs `unlock` (or
/// `unlock_with_key`); `NeedsPassphrase` is a pre-vault install that works but must be
/// converted with `set_passphrase(None, ..)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LockState {
    NoIdentity,
    Locked,
    Unlocked,
    NeedsPassphrase,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Contact {
    pub did: String,
    pub name: String,
    pub device: String,
    pub added_at: u64,
    /// Local-only name we gave them; show it in preference to `name` (what they call
    /// themselves). Also what `CallInfo.peer_name` carries when set.
    pub alias: Option<String>,
    /// We confirmed their safety number in person; reset if their device changes.
    pub verified: bool,
}

/// What a contact card found in some text says, before anyone is dialled: for "Add <name>?"
/// prompts when a card is pasted or sits on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CardPeek {
    /// The card itself, cut out of the surrounding text; hand this to `add_contact`.
    pub ticket: String,
    /// The name they gave themselves. Unverified until they are added.
    pub name: String,
    pub did: String,
    /// Already in our contacts.
    pub known: bool,
}

/// Whether calls ring. While unavailable, callers are turned away without being told why.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Availability {
    pub available: bool,
    /// Unix seconds at which it turns back to available by itself; `None` = until switched on.
    pub until: Option<u64>,
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
    /// Active, but no media for over 1.5 s and the call has not ended: the path is changing.
    pub reconnecting: bool,
}

fn waiting_ring_timeout() -> Duration {
    std::env::var("P2P_WAITING_RING_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(WAITING_RING_TIMEOUT, Duration::from_secs)
}

pub(crate) enum Cmd {
    Answer,
    Decline,
    Hangup,
}

pub(crate) struct Call {
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
    /// Held while a state change is made and delivered, so one call's events reach the UI in
    /// the order they happened. Reentrant: a callback may start the next call.
    notify: ReentrantMutex<()>,
    /// When media last arrived (or the call went active); feeds the no-audio watchdog.
    last_rx: Mutex<Instant>,
    /// Unix seconds when the call began, for the history.
    started_at: u64,
    /// When it went active, for the duration.
    active_at: Mutex<Option<Instant>>,
    /// Whether the selected path was direct when last looked at (the connection is gone by the
    /// time the call has ended).
    direct: Mutex<bool>,
    /// An incoming call that arrived during another call and sits in `Live::waiting`, not
    /// yet in the slot. Declining it tells the caller `Busy`.
    waiting: AtomicBool,
    /// "End & answer" was chosen: answer this call the moment it takes over the slot.
    answer_on_promote: AtomicBool,
    /// The session (see `Shared::epoch`) it belongs to: a call that outlives a switch of account
    /// must not write into the next one.
    epoch: u64,
    /// Woken when the call ends from outside its own task (glare, lock), so an outgoing call
    /// that is still dialling several devices stops at once.
    ended_wake: tokio::sync::Notify,
}

/// Ends the call when dropped unless it already ended: covers every early return (and panic)
/// between taking the slot and `run_call` finishing it.
struct SlotGuard<'a> {
    inner: &'a Inner,
    call: Arc<Call>,
}

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        self.inner.end_call(&self.call, "connection_lost".into());
    }
}

impl Call {
    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn state(&self) -> CallState {
        self.state.lock().clone()
    }
}

/// What one dialled device of the callee reports to `run_outgoing`.
enum LegEvent {
    /// Connected and hello sent: the device is being rung (or about to be).
    Connected,
    Ringing,
    /// The device answered; the connection and control stream are handed over.
    Accepted {
        renewed_grant: Option<proto::SignedGrant>,
        devices: Option<proto::SignedBlob>,
        conn: Connection,
        ctrl: Box<Ctrl>,
    },
    Declined(String),
    Busy,
    /// It rang and the callee's ring timed out (it sent `Hangup`).
    GaveUp,
    /// Could not connect, refused (`Reject`: not a contact there, blocked, unavailable), or the
    /// connection died.
    Failed(String),
}

/// One connection of a fanned-out outgoing call: connects, sends the hello, relays what the
/// device answers, and on a message from the coordinator sends it (a `Cancel`) and closes.
struct Leg {
    ep: Endpoint,
    device: ContactDevice,
    hello: Msg,
    idx: usize,
    events: mpsc::UnboundedSender<(usize, LegEvent)>,
    cmds: mpsc::UnboundedReceiver<Msg>,
    name: String,
}

impl Leg {
    async fn run(mut self) {
        let idx = self.idx;
        let fail = |events: &mpsc::UnboundedSender<(usize, LegEvent)>, why: String| {
            let _ = events.send((idx, LegEvent::Failed(why)));
        };
        let addr = match addr_for(&self.device.device, self.device.relay.as_deref()) {
            Ok(a) => a,
            Err(e) => return fail(&self.events, e.to_string()),
        };
        tracing::debug!("dialing {} at {}", self.name, addr.id);
        let connect = tokio::time::timeout(DIAL_TIMEOUT, self.ep.connect(addr, proto::ALPN));
        let conn = tokio::select! {
            c = connect => match c {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => return fail(&self.events, e.to_string()),
                Err(_) => return fail(&self.events, "timed out".into()),
            },
            // Told to stop before we even connected: nothing was sent, nothing to say.
            _ = self.cmds.recv() => return,
        };
        let (send, recv) = match conn.open_bi().await {
            Ok(s) => s,
            Err(e) => return fail(&self.events, e.to_string()),
        };
        let mut ctrl = Ctrl::new(send, recv);
        if let Err(e) = ctrl.send(&self.hello).await {
            conn.close(0u32.into(), b"bye");
            return fail(&self.events, e.to_string());
        }
        let _ = self.events.send((idx, LegEvent::Connected));
        loop {
            tokio::select! {
                msg = ctrl.recv() => {
                    let ev = match msg {
                        Ok(Some(Msg::Ringing)) => {
                            let _ = self.events.send((idx, LegEvent::Ringing));
                            continue;
                        }
                        Ok(Some(Msg::Accept { renewed_grant, devices })) => {
                            let keep = conn.clone();
                            let ev = LegEvent::Accepted { renewed_grant, devices, conn, ctrl: Box::new(ctrl) };
                            if self.events.send((idx, ev)).is_err() {
                                keep.close(0u32.into(), b"bye");
                            }
                            return;
                        }
                        Ok(Some(Msg::Decline { reason })) => LegEvent::Declined(reason),
                        Ok(Some(Msg::Busy)) => LegEvent::Busy,
                        Ok(Some(Msg::Hangup)) => LegEvent::GaveUp,
                        Ok(Some(Msg::Reject { reason })) => LegEvent::Failed(format!("rejected: {}", clip_reason(&reason))),
                        Ok(Some(other)) => {
                            tracing::debug!("ignoring {} before the answer", msg_name(&other));
                            continue;
                        }
                        Ok(None) => LegEvent::Failed("closed".into()),
                        Err(e) => LegEvent::Failed(e.to_string()),
                    };
                    let _ = self.events.send((idx, ev));
                    conn.close(0u32.into(), b"bye");
                    return;
                }
                cmd = self.cmds.recv() => {
                    if let Some(msg) = cmd {
                        let _ = ctrl.send(&msg).await;
                    }
                    ctrl.finish();
                    // Let the frame leave; the callee closes first when it can.
                    let _ = tokio::time::timeout(Duration::from_millis(300), conn.closed()).await;
                    conn.close(0u32.into(), b"bye");
                    return;
                }
                _ = conn.closed() => {
                    return fail(&self.events, "connection lost".into());
                }
            }
        }
    }
}

pub(crate) struct Me {
    /// The session this was unlocked in; see `Shared::epoch`.
    pub(crate) epoch: u64,
    pub(crate) profile: Profile,
    pub(crate) id: Identity,
    pub(crate) device: [u8; 32],
    pub(crate) attestation: proto::SignedAttestation,
}

pub(crate) struct Shared {
    /// What `account.json` holds; present whenever an identity exists, locked or not. (Also
    /// present, in memory only, for an identity not yet `commit_identity`ed.)
    pub(crate) disk: Option<Disk>,
    /// Present while unlocked (and for a legacy profile).
    pub(crate) me: Option<Arc<Me>>,
    /// The vault's data key while unlocked via a vault; never written to disk by the core.
    pub(crate) dek: Option<Dek>,
    /// Empty while locked: it is sealed on disk.
    pub(crate) state: State,
    pub(crate) endpoint: Option<Endpoint>,
    /// Changes whenever the account in memory is replaced, locked or unlocked. Everything that
    /// runs on its own (call tasks, background writes) remembers the value it started under and
    /// is refused once it differs, so nothing of one account is ever written into another.
    pub(crate) epoch: u64,
    /// The account's directory and where its state goes; `None` for no identity and for one not
    /// yet committed. `backing` is `None` while locked.
    pub(crate) acct: Option<Acct>,
    /// The call log of the unlocked account; empty while locked.
    pub(crate) history: Arc<History>,
    /// Sealed; known only while unlocked.
    pub(crate) device_label: Option<String>,
    /// `account.json` is on disk. False only for a no-passphrase identity waiting for
    /// `commit_identity`.
    pub(crate) committed: bool,
    /// The account doc of own-device sync; open while unlocked.
    pub(crate) acc: Option<Arc<crate::sync::accdoc::AccDoc>>,
}

pub(crate) struct Acct {
    pub(crate) id: String,
    pub(crate) store: Store,
    pub(crate) backing: Option<Backing>,
}

/// The call slot, apart from `Shared` so the 50 Hz audio threads never wait behind a disk
/// write or a handshake holding `Shared`.
#[derive(Default)]
pub(crate) struct Live {
    call: Option<Arc<Call>>,
    /// A second incoming call, ringing over the active one. At most one; a third is turned
    /// away busy. Takes the slot if the active call ends first.
    waiting: Option<Arc<Call>>,
    /// The call that held the slot before, until the next call has waited for its events to
    /// be delivered (so `Ended` always precedes the next call's `Dialing`).
    last: Option<Arc<Call>>,
    tone: Option<f32>,
}

pub(crate) struct Inner {
    pub(crate) handle: tokio::runtime::Handle,
    pub(crate) accounts: AccountDirs,
    /// Serialises creating, committing and unlocking an account (they open its files).
    pub(crate) gate: Mutex<()>,
    /// Test knob: this many state writes fail. See `Node::fail_next_writes`.
    pub(crate) fail_writes: AtomicU32,
    /// Held while snapshotting and writing state, so writes land in the order taken. Always
    /// taken before `shared`, never while holding it.
    pub(crate) writing: Mutex<()>,
    pub(crate) events: Arc<dyn NodeEvents>,
    pub(crate) shared: Mutex<Shared>,
    pub(crate) live: Mutex<Live>,
    pub(crate) pending: Arc<Semaphore>,
    pub(crate) reserved: Arc<Semaphore>,
    /// Serialises start, stop, lock and set_passphrase.
    pub(crate) lifecycle: Arc<tokio::sync::Mutex<()>>,
    pub(crate) chat: Mutex<Option<Arc<crate::chat::engine::ChatCore>>>,
    pub(crate) chat_events: Mutex<Option<Arc<dyn crate::chat::api::ChatEvents>>>,
    pub(crate) link_events: Mutex<Option<Arc<dyn crate::sync::LinkEvents>>>,
    pub(crate) selfsync: crate::sync::engine::SelfSync,
    pub(crate) sync_tx: tokio::sync::watch::Sender<u64>,
    pub(crate) link: crate::sync::link::LinkState,
}

/// Blocking methods (`start`, `stop`, `add_contact`, `my_ticket`) are for the app's own
/// threads; called from inside a `NodeEvents` callback they fail (or, for `stop`, go async)
/// instead of blocking the runtime that delivers the callback.
#[derive(uniffi::Object)]
pub struct Node {
    pub(crate) inner: Arc<Inner>,
    rt: Option<tokio::runtime::Runtime>,
}

impl Drop for Node {
    fn drop(&mut self) {
        let ep = self.inner.shared.lock().endpoint.take();
        if let Some(rt) = self.rt.take() {
            if let Some(ep) = ep {
                // Best effort: a clean close tells peers at once instead of at idle timeout.
                if !on_core_thread() && tokio::runtime::Handle::try_current().is_err() {
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
            .worker_threads(4)
            .thread_name(CORE_THREAD)
            .enable_all()
            .build()?;
        let accounts = AccountDirs::open(data_dir)?;
        accounts.migrate()?;
        let mut shared = Shared::new();
        // A legacy profile keeps working (calls keep ringing) until it is converted.
        if let Some(id) = accounts.current() {
            shared.install(load_account(&accounts, &id)?)?;
        }
        let inner = Arc::new(Inner {
            handle: rt.handle().clone(),
            accounts,
            gate: Mutex::new(()),
            fail_writes: AtomicU32::new(0),
            writing: Mutex::new(()),
            events,
            shared: Mutex::new(shared),
            live: Mutex::new(Live::default()),
            pending: Arc::new(Semaphore::new(MAX_PENDING_HELLOS)),
            reserved: Arc::new(Semaphore::new(RESERVED_HELLOS)),
            lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            chat: Mutex::new(None),
            chat_events: Mutex::new(None),
            link_events: Mutex::new(None),
            selfsync: Default::default(),
            sync_tx: tokio::sync::watch::channel(0).0,
            link: Default::default(),
        });
        Ok(Arc::new(Self { inner, rt: Some(rt) }))
    }

    pub fn has_identity(&self) -> bool {
        self.inner.shared.lock().disk.is_some()
    }

    /// Whether the identity is on disk. False only for a no-passphrase identity that has not
    /// been `commit_identity`ed yet (and with no identity at all).
    pub fn identity_committed(&self) -> bool {
        let s = self.inner.shared.lock();
        s.disk.is_some() && s.committed
    }

    /// Makes a no-passphrase identity durable: writes `account.json` and selects the account.
    /// The platform calls it once its keystore has saved `unlock_key()`; until then nothing is
    /// on disk, so a failed keystore write (or a crash) leaves no account whose key is lost, and
    /// `lock` or a restart simply forgets the identity. No-op once committed, and for an
    /// identity made with a passphrase (written at once).
    pub fn commit_identity(&self) -> Result<(), Error> {
        let _gate = self.inner.gate.lock();
        self.inner.commit_locked()
    }

    /// Every account on this device. Needs no unlock: only the clear `account.json` is read.
    pub fn accounts(&self) -> Vec<AccountSummary> {
        let listed = self.inner.accounts.list().unwrap_or_else(|e| {
            tracing::warn!("listing accounts: {e}");
            Vec::new()
        });
        let s = self.inner.shared.lock();
        let current = s.acct.as_ref().map(|a| a.id.clone());
        let mut out: Vec<AccountSummary> = listed
            .into_iter()
            .map(|e| AccountSummary {
                current: current.as_deref() == Some(e.id.as_str()),
                did: e.did,
                name: e.name,
                has_passphrase: e.has_passphrase,
            })
            .collect();
        if let (Some(Disk::V2(p)), false) = (&s.disk, s.committed) {
            out.push(AccountSummary { did: p.did.clone(), name: p.name.clone(), current: true, has_passphrase: false });
        }
        out
    }

    /// Makes `did` the current account: stops the endpoint, locks and forgets the one in memory.
    /// Unlock it as usual afterwards (`unlock` or `unlock_with_key`). `InCall` during a call.
    pub fn switch_account(&self, did: String) -> Result<(), Error> {
        let id = accounts::id_of(&did)?;
        self.inner.not_in_call()?;
        let loaded = load_account(&self.inner.accounts, &id)?;
        let inner = self.inner.clone();
        self.block_on(async move { inner.replace_account(Some(loaded)).await })?
    }

    /// Leaves no account selected, so `has_identity` is false and `create_identity` /
    /// `restore_identity` add a new one. The accounts on disk are untouched. `InCall` during a call.
    pub fn begin_new_account(&self) -> Result<(), Error> {
        self.inner.not_in_call()?;
        let inner = self.inner.clone();
        self.block_on(async move { inner.replace_account(None).await })?
    }

    /// Deletes a non-current account and everything in its directory.
    pub fn remove_account(&self, did: String) -> Result<(), Error> {
        let id = accounts::id_of(&did)?;
        let _life = self.lifecycle()?;
        let current = self.inner.shared.lock().acct.as_ref().map(|a| a.id.clone());
        if current.as_deref() == Some(id.as_str()) || self.inner.accounts.current().as_deref() == Some(id.as_str()) {
            return Err(Error::Protocol("the current account cannot be removed".into()));
        }
        self.inner.accounts.remove(&id)
    }

    /// Names this installation ("Personal phone"), 1-64 characters, no control characters.
    /// Sealed with the account: needs the node unlocked, and a vault (not a legacy profile).
    pub fn set_device_label(&self, label: String) -> Result<(), Error> {
        let label = label.trim().to_string();
        if label.is_empty() || label.chars().count() > 64 || label.chars().any(char::is_control) {
            return Err(Error::Protocol("a device name is 1-64 characters without control characters".into()));
        }
        self.inner.me()?;
        let db = {
            let s = self.inner.shared.lock();
            match s.acct.as_ref().and_then(|a| a.backing.clone()) {
                Some(Backing::Sealed(db)) => Some(db),
                Some(Backing::Json(_)) => return Err(Error::Protocol("convert the profile with set_passphrase first".into())),
                None => None, // not committed yet: kept in memory, written by the commit
            }
        };
        if let Some(db) = db {
            store::save_label(&db, &label)?;
        }
        self.inner.shared.lock().device_label = Some(label);
        Ok(())
    }

    /// The label given to `set_device_label`; `None` until set, and while locked.
    pub fn device_label(&self) -> Option<String> {
        self.inner.shared.lock().device_label.clone()
    }

    pub fn lock_state(&self) -> LockState {
        let s = self.inner.shared.lock();
        match (&s.disk, &s.me) {
            (None, _) => LockState::NoIdentity,
            (Some(Disk::Legacy(_)), _) => LockState::NeedsPassphrase,
            (Some(Disk::V2(_)), Some(_)) => LockState::Unlocked,
            (Some(Disk::V2(_)), None) => LockState::Locked,
        }
    }

    /// Whether a passphrase wraps the vault's data key. False for a vault the platform alone
    /// opens (see `unlock_key`) and for a legacy profile.
    pub fn has_passphrase(&self) -> bool {
        matches!(&self.inner.shared.lock().disk, Some(Disk::V2(p)) if p.vault.has_passphrase())
    }

    /// Creates an identity and leaves the node unlocked. An empty `passphrase` means none: the
    /// data key is then only in the platform's hands (`unlock_key`). Returns the recovery
    /// phrase; showing it again later needs the passphrase if there is one (`recovery_phrase`).
    /// Slow with a passphrase: runs Argon2id.
    pub fn create_identity(&self, name: String, passphrase: String) -> Result<String, Error> {
        let (_, mnemonic) = identity::generate();
        let phrase = mnemonic.to_string();
        self.set_identity(phrase.clone(), name, passphrase)?;
        Ok(phrase)
    }

    /// Like `create_identity`, from a recovery phrase.
    pub fn restore_identity(&self, phrase: String, name: String, passphrase: String) -> Result<(), Error> {
        self.set_identity(phrase.trim().to_string(), name, passphrase)
    }

    /// Name, DID and device key; available while locked too (they are stored in the clear).
    pub fn profile(&self) -> Option<ProfileInfo> {
        let s = self.inner.shared.lock();
        match (&s.disk, &s.me) {
            (_, Some(me)) => Some(me.info()),
            (Some(Disk::V2(p)), None) => Some(ProfileInfo {
                did: p.did.clone(),
                name: p.name.clone(),
                device: PublicKey::from_bytes(&p.device_public).map(|k| k.to_string()).unwrap_or_default(),
            }),
            _ => None,
        }
    }

    /// Unlocks with the passphrase; `WrongPassphrase` if it does not open the vault. Returns
    /// at once if already unlocked (or a legacy profile); a vault without a passphrase only
    /// opens with `unlock_with_key`. Slow: runs Argon2id.
    pub fn unlock(&self, passphrase: String) -> Result<(), Error> {
        let vault = match self.unlock_target()? {
            Some(v) => v,
            None => return Ok(()),
        };
        let (secrets, dek) = self.kdf(move || vault::open(&vault, &passphrase))??;
        self.finish_unlock(secrets, dek)
    }

    /// Unlocks with a data key the platform remembered (from `unlock_key`); fast.
    /// `WrongPassphrase` if it does not open the vault: the platform should then forget it.
    pub fn unlock_with_key(&self, mut key: Vec<u8>) -> Result<(), Error> {
        let vault = match self.unlock_target() {
            Ok(Some(v)) => v,
            other => {
                key.zeroize();
                return other.map(|_| ());
            }
        };
        let opened = vault::open_with_key(&vault, &key).and_then(|secrets| {
            <[u8; 32]>::try_from(key.as_slice())
                .map(|k| (secrets, zeroize::Zeroizing::new(k)))
                .map_err(|_| Error::WrongPassphrase)
        });
        key.zeroize();
        let (secrets, dek) = opened?;
        self.finish_unlock(secrets, dek)
    }

    /// The vault's data key while unlocked via a vault, for the platform to remember (wrapped
    /// by its own hardware key). `None` when locked or on a legacy profile. Stays valid across
    /// `set_passphrase`.
    pub fn unlock_key(&self) -> Option<Vec<u8>> {
        self.inner.shared.lock().dek.as_ref().map(|d| d.to_vec())
    }

    /// Sets or changes the passphrase (any non-empty one). With `old = None` it converts a
    /// legacy profile (state `NeedsPassphrase`; an empty `new` seals it without a passphrase)
    /// and the clear-text secrets leave profile.json; on a vault without a passphrase it adds
    /// one (the node must be unlocked); otherwise `old` is required and verified. The data key
    /// is kept, so a remembered `unlock_key` stays valid. Slow: runs Argon2id once or twice.
    pub fn set_passphrase(&self, old: Option<String>, new: String) -> Result<(), Error> {
        let _life = self.lifecycle()?;
        let snapshot = self.inner.shared.lock().disk.clone();
        match snapshot.ok_or(Error::NoIdentity)? {
            Disk::Legacy(p) => {
                if old.is_some() {
                    return Err(Error::Protocol("this identity has no passphrase yet".into()));
                }
                let new = Some(new).filter(|n| !n.is_empty());
                let id = identity::recover(&p.mnemonic).map_err(|_| Error::BadPhrase)?;
                let secrets = Secrets { mnemonic: p.mnemonic.clone(), device_secret: p.device_secret };
                let device_public = proto::device_public(&p.device_secret);
                let (vault, dek) = self.kdf(move || vault::seal(&secrets, new.as_deref()))??;
                let mut s = self.inner.shared.lock();
                // The name may have changed (set_name) while the KDF ran.
                let name = match s.me.as_ref() {
                    Some(me) => me.profile.name.clone(),
                    None => p.name.clone(),
                };
                let disk = Disk::V2(ProfileV2 {
                    version: 2,
                    name,
                    did: id.did().to_string(),
                    device_public,
                    vault,
                });
                let store = s.acct.as_ref().ok_or(Error::NoIdentity)?.store.clone();
                // The vault first: if we die before the state is sealed, the JSON is still
                // there and the next unlock imports it.
                store.save_profile(&disk)?;
                let db = store::seal_into(&store, &p.mnemonic, &dek, &s.state, &s.history.snapshot(), None)?;
                store.remove_legacy_leftovers();
                let backing = Backing::Sealed(db);
                s.history.attach(backing.clone());
                if let Some(a) = s.acct.as_mut() {
                    a.backing = Some(backing);
                }
                s.disk = Some(disk);
                s.dek = Some(dek);
                drop(s);
                self.inner.chat_open();
        self.inner.acc_open();
                Ok(())
            }
            Disk::V2(p) => {
                vault::check_passphrase(&new)?;
                let vault = p.vault;
                let rewrapped = if vault.has_passphrase() {
                    let old = old.ok_or_else(|| Error::Protocol("the old passphrase is required".into()))?;
                    self.kdf(move || {
                        let (_secrets, dek) = vault::open(&vault, &old)?;
                        vault::rewrap(&vault, &dek, &new)
                    })??
                } else {
                    if old.is_some() {
                        return Err(Error::Protocol("no passphrase is set yet".into()));
                    }
                    // Wrapping needs the data key, which only an unlocked node holds.
                    let dek = self.inner.shared.lock().dek.clone().ok_or(Error::Locked)?;
                    self.kdf(move || vault::rewrap(&vault, &dek, &new))??
                };
                let mut s = self.inner.shared.lock();
                // Only the vault changes; keep any name change made meanwhile.
                let Some(Disk::V2(cur)) = s.disk.clone() else { return Err(Error::NoIdentity) };
                let disk = Disk::V2(ProfileV2 { vault: rewrapped, ..cur });
                if !s.committed {
                    // A passphrase makes the data key recoverable, so the identity can be written now.
                    s.disk = Some(disk);
                    drop(s);
                    let _gate = self.inner.gate.lock();
                    return self.inner.commit_locked();
                }
                s.acct.as_ref().ok_or(Error::NoIdentity)?.store.save_profile(&disk)?;
                s.disk = Some(disk);
                Ok(())
            }
        }
    }

    /// The recovery phrase, re-derived from the vault: `WrongPassphrase` unless `passphrase`
    /// is right, and `Locked` on a legacy profile (convert it with `set_passphrase` first).
    /// Works while locked. Slow: runs Argon2id. With no passphrase set, `passphrase` is
    /// ignored and the node must be unlocked: the platform asks for device authentication
    /// before calling this.
    pub fn recovery_phrase(&self, passphrase: String) -> Result<String, Error> {
        let vault = {
            let s = self.inner.shared.lock();
            match &s.disk {
                None => return Err(Error::NoIdentity),
                Some(Disk::Legacy(_)) => return Err(Error::Locked),
                Some(Disk::V2(p)) if !p.vault.has_passphrase() => {
                    return s.me.as_ref().map(|me| me.profile.mnemonic.clone()).ok_or(Error::Locked);
                }
                Some(Disk::V2(p)) => p.vault.clone(),
            }
        };
        let (secrets, _dek) = self.kdf(move || vault::open(&vault, &passphrase))??;
        Ok(secrets.mnemonic.clone())
    }

    /// Stops the endpoint and forgets the secrets in memory; the node is `Locked` until
    /// `unlock`. No-op without a vault (no identity, or a legacy profile).
    pub fn lock(&self) {
        if !matches!(self.inner.shared.lock().disk, Some(Disk::V2(_))) {
            return;
        }
        self.inner.link_cancel();
        let inner = self.inner.clone();
        self.run_lifecycle(async move {
            {
                // Under the lifecycle lock, so a `start` that is binding right now finishes
                // first and its endpoint is closed here.
                let _life = inner.lifecycle.lock().await;
                inner.close_endpoint().await;
                inner.shared.lock().forget_session();
                inner.chat_close().await;
            }
            inner.emit_status();
        });
    }

    pub fn set_name(&self, name: String) -> Result<(), Error> {
        let unlocked = {
            let mut s = self.inner.shared.lock();
            let mut disk = s.disk.clone().ok_or(Error::NoIdentity)?;
            match &mut disk {
                Disk::V2(p) => p.name = name.clone(),
                Disk::Legacy(p) => p.name = name.clone(),
            }
            if s.committed {
                s.acct.as_ref().ok_or(Error::NoIdentity)?.store.save_profile(&disk)?;
            }
            s.disk = Some(disk);
            if let Some(me) = s.me.clone() {
                let mut profile = me.profile.clone();
                profile.name = name;
                s.me = Some(Arc::new(Me::load(profile, me.epoch)?));
            }
            // The old ticket carries the old name (and `ticket()` checks it, for the locked case
            // where the sealed state cannot be touched).
            s.state.ticket = None;
            s.me.is_some()
        };
        if unlocked { self.inner.persist() } else { Ok(()) }
    }

    /// Drops the ticket we hand out: the old QR/text stops working at once and the next
    /// `my_ticket` mints a fresh one. For "the ticket may have leaked".
    pub fn reset_ticket(&self) -> Result<(), Error> {
        self.inner.me()?;
        self.inner.shared.lock().state.ticket = None;
        self.inner.persist()
    }

    /// Binds the endpoint and starts accepting calls. Idempotent.
    pub fn start(&self) -> Result<(), Error> {
        let inner = self.inner.clone();
        self.block_on(inner.start())?
    }

    pub fn stop(&self) {
        let inner = self.inner.clone();
        self.run_lifecycle(async move {
            {
                let _life = inner.lifecycle.lock().await;
                inner.chat_sessions_close();
                inner.close_endpoint().await;
            }
            inner.emit_status();
        });
    }

    /// The platform saw the network change (wifi ↔ cellular); re-probe paths now rather than
    /// waiting for the next timeout.
    pub fn network_changed(&self) {
        let ep = self.inner.shared.lock().endpoint.clone();
        if let Some(ep) = ep {
            self.inner.handle.spawn(async move { ep.network_change().await });
            self.inner.chat_kick_pending();
            self.inner.sync_kick();
        }
    }

    pub fn status(&self) -> NodeStatus {
        self.inner.status()
    }

    /// Our contact ticket (`OSVC2:…`), stable until someone redeems it.
    pub fn my_ticket(&self) -> Result<String, Error> {
        let inner = self.inner.clone();
        self.block_on(async move { inner.ticket().await })?
    }

    /// Finds a contact card in `text` (a pasted message, the clipboard) and reads it without
    /// dialling anyone. `None` when there is no valid, unexpired card or it is our own.
    pub fn peek_card(&self, text: String) -> Option<CardPeek> {
        let ticket = find_card(&text)?;
        let claim = ContactTicket::from_text(&ticket).ok()?.verify(now()).ok()?;
        let me = self.inner.me().ok()?;
        if claim.iss == me.id.did() {
            return None;
        }
        let known = self.inner.shared.lock().state.contacts.iter().any(|c| c.did == claim.iss);
        Some(CardPeek { ticket, name: claim.name, did: claim.iss, known })
    }

    /// Redeems someone's ticket: dials them, exchanges grants, stores them. Blocks until done.
    pub fn add_contact(&self, ticket: String) -> Result<Contact, Error> {
        let inner = self.inner.clone();
        self.block_on(inner.add_contact(ticket))?
    }

    pub fn contacts(&self) -> Vec<Contact> {
        let s = self.inner.shared.lock();
        s.state.contacts.iter().map(Contact::from).collect()
    }

    /// Sets the name only we see for `did`; `None` or blank clears it. Trimmed, stripped of
    /// control characters and cut to 64 characters.
    pub fn rename_contact(&self, did: String, alias: Option<String>) -> Result<(), Error> {
        let alias = alias
            .map(|a| proto::sanitize_name(a.trim()).chars().take(MAX_ALIAS_CHARS).collect::<String>())
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty());
        self.update_contact(&did, |c| c.alias = alias)
    }

    /// Marks that the safety numbers were compared (or not).
    pub fn set_verified(&self, did: String, verified: bool) -> Result<(), Error> {
        self.update_contact(&did, |c| c.verified = verified)
    }

    /// 60 digits in 12 groups of 5, the same on both phones: read it out or compare it in
    /// person. Derived from the two long-term identity keys only, not from any device.
    pub fn safety_number(&self, did: String) -> Result<String, Error> {
        let mine = self.profile().ok_or(Error::NoIdentity)?.did;
        safety_number(&mine, &did)
    }

    /// Finished calls, newest first, at most `limit`. Re-read on `on_call_state` `Ended`.
    pub fn recent_calls(&self, limit: u32) -> Vec<CallRecord> {
        self.inner.history().list(None, limit as usize)
    }

    /// Finished calls with one person, newest first.
    pub fn calls_with(&self, did: String, limit: u32) -> Vec<CallRecord> {
        self.inner.history().list(Some(&did), limit as usize)
    }

    /// Turns calls on or off. Off, incoming calls are not shown and the caller just fails to
    /// get through; they are logged with reason `unavailable`. `until` (unix seconds) switches
    /// it back on by itself. Adding contacts still works.
    pub fn set_available(&self, available: bool, until: Option<u64>) -> Result<(), Error> {
        self.inner.me()?;
        self.inner.shared.lock().state.availability = crate::store::AvailabilityState {
            unavailable: !available,
            until: if available { None } else { until },
        };
        self.inner.persist()
    }

    pub fn availability(&self) -> Availability {
        self.inner.availability()
    }

    /// Forgets them and blocks their DID, so the grant we gave them no longer rings us; any
    /// call with them ends. Adding them again lifts the block.
    pub fn remove_contact(&self, did: String) -> Result<(), Error> {
        self.inner.me()?;
        {
            let mut s = self.inner.shared.lock();
            s.state.contacts.retain(|c| c.did != did);
            s.state.blocked.insert(did.clone());
            // A ticket they may have seen is of no further use.
            s.state.ticket = None;
        }
        let history = self.inner.history();
        history.remove_peer(&did);
        self.inner.chat_purge(&did);
        // The removal holds in memory whether or not it reached the disk, so act on it either
        // way and report the failed write afterwards. (A call ending now is not logged: the
        // peer is no longer a contact.)
        let saved = self.inner.persist().and(history.save());
        self.inner.sync_local(None);
        let calls: Vec<_> = {
            let live = self.inner.live.lock();
            live.call.iter().chain(live.waiting.iter()).cloned().collect()
        };
        for call in calls.into_iter().filter(|c| c.info.peer_did == did) {
            let _ = call.cmd.send(Cmd::Hangup);
        }
        self.inner.events.on_contacts_changed();
        saved
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

    /// Ends the active call (a local hangup) and answers the waiting one `call_id`, which
    /// then becomes the current call.
    pub fn end_and_answer(&self, call_id: String) -> Result<(), Error> {
        let live = self.inner.live.lock();
        let w = live.waiting.as_ref().filter(|c| c.info.call_id == call_id).ok_or(Error::NotFound)?;
        let cur = live.call.as_ref().ok_or(Error::NotFound)?;
        w.answer_on_promote.store(true, Ordering::SeqCst);
        cur.cmd.send(Cmd::Hangup).map_err(|_| Error::NotFound)
    }

    /// The second incoming call ringing over the current call, if any. Declining it
    /// (`decline`) tells its caller `busy`; ignoring it ends it with `no_answer` after 30 s.
    pub fn waiting_call(&self) -> Option<CallInfo> {
        self.inner.live.lock().waiting.as_ref().map(|c| c.info.clone())
    }

    pub fn current_call(&self) -> Option<CallInfo> {
        self.inner.live.lock().call.as_ref().map(|c| c.info.clone())
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
        self.inner.live.lock().tone = hz;
    }
}

impl Me {
    pub(crate) fn load(profile: Profile, epoch: u64) -> Result<Self, Error> {
        let id = identity::recover(&profile.mnemonic).map_err(|_| Error::BadPhrase)?;
        let device = proto::device_public(&profile.device_secret);
        let attestation = proto::attest(&id, device, now());
        Ok(Self { epoch, profile, id, device, attestation })
    }

    pub(crate) fn info(&self) -> ProfileInfo {
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
                .and_then(|d| PublicKey::from_bytes(&d.device).ok())
                .map(|k| k.to_string())
                .unwrap_or_default(),
            added_at: c.added_at,
            alias: c.alias.clone(),
            verified: c.verified,
        }
    }
}

/// What the UI should call this contact: our alias if any, else their own name.
pub(crate) fn display_name(c: &StoredContact) -> String {
    c.alias.clone().unwrap_or_else(|| c.name.clone())
}

/// The safety number for the pair: each side's 30 digits come from its own key, and the two
/// halves are joined in sorted order, so both phones compute the same string.
fn safety_number(mine: &str, theirs: &str) -> Result<String, Error> {
    let half = |did: &str| -> Result<String, Error> {
        let key = identity::public_key_from_did(did).ok_or_else(|| Error::Protocol("not a valid DID".into()))?;
        let mut h = Sha512::new().chain_update(b"tinline-safety-v1").chain_update(key).finalize();
        // Iterated, as a (cheap) brake on grinding keys whose digits look alike.
        for _ in 0..5200 {
            h = Sha512::new().chain_update(h).chain_update(key).finalize();
        }
        Ok(h[..30]
            .chunks_exact(5)
            .map(|c| format!("{:05}", c.iter().fold(0u64, |n, b| n << 8 | *b as u64) % 100_000))
            .collect())
    };
    let (a, b) = (half(mine)?, half(theirs)?);
    let all = if a <= b { a + &b } else { b + &a };
    Ok(all.as_bytes().chunks(5).map(|g| std::str::from_utf8(g).unwrap_or("")).collect::<Vec<_>>().join(" "))
}

impl Node {
    /// Runs `fut` on the core runtime and waits for it. Waiting on a plain channel works from
    /// any app thread, including another runtime's (iced's tokio, `spawn_blocking`); only the
    /// core's own threads are refused, since blocking one could starve the future itself.
    pub(crate) fn block_on<T: Send + 'static>(
        &self,
        fut: impl std::future::Future<Output = T> + Send + 'static,
    ) -> Result<T, Error> {
        if on_core_thread() {
            return Err(Error::Protocol("blocking call made from a NodeEvents callback".into()));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.inner.handle.spawn(async move {
            let _ = tx.send(fut.await);
        });
        rx.recv().map_err(|_| Error::Protocol("core runtime stopped".into()))
    }

    fn update_contact(&self, did: &str, f: impl FnOnce(&mut StoredContact)) -> Result<(), Error> {
        {
            let mut s = self.inner.shared.lock();
            f(s.state.contacts.iter_mut().find(|c| c.did == did).ok_or(Error::NotFound)?);
        }
        let saved = self.inner.persist();
        self.inner.events.on_contacts_changed();
        saved
    }

    /// Takes the lifecycle lock for the caller's whole operation; refuses on core threads.
    fn lifecycle(&self) -> Result<OwnedMutexGuard<()>, Error> {
        let l = self.inner.lifecycle.clone();
        self.block_on(async move { l.lock_owned().await })
    }

    /// Runs a lifecycle step and waits for it; from a callback it can't wait, so it finishes in
    /// the background.
    fn run_lifecycle(&self, fut: impl std::future::Future<Output = ()> + Send + 'static) {
        if on_core_thread() {
            self.inner.handle.spawn(fut);
        } else {
            let _ = self.block_on(fut);
        }
    }

    /// Runs a slow, blocking computation (the KDF) on the runtime's blocking pool and waits for
    /// it, so it never runs while `shared` is held and never on a callback thread.
    pub(crate) fn kdf<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> Result<T, Error> {
        self.block_on(async move {
            tokio::task::spawn_blocking(f).await.map_err(|_| Error::Protocol("kdf task failed".into()))
        })?
    }

    /// The vault to open, or `None` when there is nothing to unlock.
    fn unlock_target(&self) -> Result<Option<vault::Vault>, Error> {
        let s = self.inner.shared.lock();
        match &s.disk {
            None => Err(Error::NoIdentity),
            Some(Disk::Legacy(_)) => Ok(None),
            Some(Disk::V2(_)) if s.me.is_some() => Ok(None),
            Some(Disk::V2(p)) => Ok(Some(p.vault.clone())),
        }
    }

    fn finish_unlock(&self, secrets: Secrets, dek: Dek) -> Result<(), Error> {
        let _gate = self.inner.gate.lock();
        let (store, did, name, started) = {
            let s = self.inner.shared.lock();
            let Some(Disk::V2(p)) = &s.disk else { return Err(Error::NoIdentity) };
            if s.me.is_some() {
                return Ok(());
            }
            let store = s.acct.as_ref().ok_or(Error::NoIdentity)?.store.clone();
            (store, p.did.clone(), p.name.clone(), s.epoch)
        };
        let me = Me::load(
            Profile { mnemonic: secrets.mnemonic.clone(), name, device_secret: secrets.device_secret },
            0,
        )?;
        if me.id.did() != did {
            return Err(Error::Io("vault does not match profile".into()));
        }
        // The sealed state is read (and a first-time import from JSON done) outside `shared`.
        let (db, loaded) = store::load_sealed(&store, &secrets.mnemonic, Some(&dek))?;
        let mut me = me;
        let mut s = self.inner.shared.lock();
        if s.epoch != started {
            // Another account was selected while the passphrase was being checked.
            return Err(Error::Locked);
        }
        s.epoch += 1;
        me.epoch = s.epoch;
        let backing = Backing::Sealed(db);
        s.history = Arc::new(History::new(loaded.calls, Some(backing.clone())));
        s.state = loaded.state;
        s.device_label = loaded.device_label;
        if let Some(a) = s.acct.as_mut() {
            a.backing = Some(backing);
        }
        s.me = Some(Arc::new(me));
        s.dek = Some(dek);
        drop(s);
        self.inner.chat_open();
        self.inner.acc_open();
        Ok(())
    }

    fn set_identity(&self, phrase: String, name: String, passphrase: String) -> Result<(), Error> {
        let _gate = self.inner.gate.lock();
        let profile = Profile { mnemonic: phrase, name, device_secret: proto::new_device_secret() };
        let mut me = Me::load(profile, 0)?;
        let did = me.id.did().to_string();
        let id = accounts::id_of(&did)?;
        // What is selected right now decides: a locked copy of this very account may be restored
        // over (the passphrase was forgotten); an unlocked one is not touched (`AccountExists`,
        // switch to it instead); any other selected account is `HaveIdentity`.
        {
            let s = self.inner.shared.lock();
            match &s.disk {
                None => {}
                Some(Disk::V2(p)) if p.did == did && s.me.is_none() && s.committed => {}
                Some(Disk::V2(p)) if p.did == did => return Err(Error::AccountExists(did)),
                Some(Disk::Legacy(_)) if s.me.as_ref().is_some_and(|m| m.id.did() == did) => return Err(Error::AccountExists(did)),
                Some(_) => return Err(Error::HaveIdentity),
            }
        }
        // Checked before the slow KDF; `create` / the restore below are the checks that cannot race.
        let existing = self.inner.accounts.exists(&id)?;
        let mut state = State::default();
        let mut calls = Vec::new();
        let mut label = None;
        if existing {
            let store = Store::open(self.inner.accounts.dir(&id)?)?;
            let old_device = match store.profile()? {
                Some(Disk::V2(p)) => Some(p.device_public),
                Some(Disk::Legacy(_)) => return Err(Error::AccountExists(did)),
                None => None,
            };
            // The data opens with the phrase alone. A store from before the identity-derived
            // keys that was never unlocked since cannot be read without the lost data key; it is
            // set aside when the restore is written, and the account starts empty.
            if !rekey::file_needs_migration(&store.dir().join("account.redb"))? {
                let (_db, loaded) = store::load_sealed(&store, &me.profile.mnemonic, None)?;
                state = loaded.state;
                calls = loaded.calls;
                label = loaded.device_label;
            }
            // This install's previous device key died with the old vault.
            if let Some(old) = old_device {
                for e in state.registry.iter_mut().filter(|e| e.device == old && old != me.device) {
                    e.removed = true;
                }
            }
        }
        let secrets = Secrets { mnemonic: me.profile.mnemonic.clone(), device_secret: me.profile.device_secret };
        let passphrase = Some(passphrase).filter(|p| !p.is_empty());
        let durable = passphrase.is_some();
        let (vault, dek) = self.kdf(move || vault::seal(&secrets, passphrase.as_deref()))??;
        let disk = Disk::V2(ProfileV2 {
            version: 2,
            name: me.profile.name.clone(),
            did: did.clone(),
            device_public: me.device,
            vault,
        });
        // Without a passphrase nothing is written until the platform has saved the key.
        let acct = if durable {
            Some(if existing {
                self.inner.restore_account_files(&id, &disk, &me.profile.mnemonic, &dek, &state, &calls, label.as_deref())?
            } else {
                self.inner.create_account_files(&id, &disk, &me.profile.mnemonic, &dek, &state, &calls, label.as_deref())?
            })
        } else {
            None
        };
        let mut s = self.inner.shared.lock();
        s.epoch += 1;
        me.epoch = s.epoch;
        s.history = Arc::new(History::new(calls, acct.as_ref().and_then(|a| a.backing.clone())));
        s.state = state;
        s.device_label = label;
        s.committed = durable;
        s.acct = acct;
        s.disk = Some(disk);
        s.me = Some(Arc::new(me));
        s.dek = Some(dek);
        drop(s);
        self.inner.chat_open();
        self.inner.acc_open();
        Ok(())
    }

    /// For tests: this install's attestation as JSON, to register it with another install of
    /// the same phrase (`add_own_device_for_test`).
    #[doc(hidden)]
    pub fn own_attestation_for_test(&self) -> Result<String, Error> {
        Ok(serde_json::to_string(&self.inner.me()?.attestation)?)
    }

    /// For tests: registers a second install of this account (real linking and sync are task
    /// 34d). Its attestation must verify under our DID. The device list we send changes.
    #[doc(hidden)]
    pub fn add_own_device_for_test(&self, attestation_json: String) -> Result<(), Error> {
        let me = self.inner.me()?;
        let attestation: proto::SignedAttestation = serde_json::from_str(&attestation_json)?;
        let att = proto::verify_attestation(&attestation).map_err(|e| Error::Protocol(e.to_string()))?;
        if att.did != me.id.did() {
            return Err(Error::Protocol("not our account".into()));
        }
        let device = att.device_key().map_err(|e| Error::Protocol(e.to_string()))?;
        {
            let mut s = self.inner.shared.lock();
            if s.epoch != me.epoch {
                return Err(Error::Locked);
            }
            let reg = &mut s.state.registry;
            match reg.iter_mut().find(|e| e.device == device) {
                Some(e) => {
                    e.attestation = attestation;
                    e.removed = false;
                }
                None => reg.push(OwnDevice { device, attestation, label: String::new(), removed: false, last_seen: now(), relay: None }),
            }
        }
        self.inner.persist_for(Some(me.epoch))
    }

    /// For tests: offers `list_json` (a `SignedBlob`) to the contact `did` as if it had arrived
    /// in a hello; whether it was taken.
    #[doc(hidden)]
    pub fn offer_device_list_for_test(&self, did: String, list_json: String) -> bool {
        let Ok(me) = self.inner.me() else { return false };
        let Ok(blob) = serde_json::from_str::<proto::SignedBlob>(&list_json) else { return false };
        let taken = self.inner.accept_device_list(me.epoch, &did, &blob);
        let _ = self.inner.persist_for(Some(me.epoch));
        taken
    }

    /// For tests: the device keys we would dial for `did`, in order (text form).
    #[doc(hidden)]
    pub fn contact_devices_for_test(&self, did: String) -> Vec<String> {
        let s = self.inner.shared.lock();
        s.state
            .contacts
            .iter()
            .find(|c| c.did == did)
            .map(|c| c.devices.iter().map(|d| proto::device_to_text(&d.device)).collect())
            .unwrap_or_default()
    }

    /// For tests: this install's device key (text form, as in `contact_devices_for_test`).
    #[doc(hidden)]
    pub fn device_key_for_test(&self) -> Result<String, Error> {
        Ok(proto::device_to_text(&self.inner.me()?.device))
    }

    /// For tests: the device list we currently publish (a JSON `SignedBlob`).
    #[doc(hidden)]
    pub fn own_device_list_for_test(&self) -> Option<String> {
        let me = self.inner.me().ok()?;
        serde_json::to_string(&self.inner.my_device_list(&me)?).ok()
    }

    /// For tests: the next `n` writes of account state fail, as if the disk were full.
    #[doc(hidden)]
    pub fn fail_next_writes(&self, n: u32) {
        self.inner.fail_writes.store(n, Ordering::SeqCst);
    }
}

/// An account's files, read before anything is unlocked.
pub(crate) struct Loaded {
    id: String,
    disk: Disk,
    store: Store,
}

pub(crate) fn load_account(accounts: &AccountDirs, id: &str) -> Result<Loaded, Error> {
    // `Store::open` would create the directory of an account that is not there.
    if !accounts.exists(id)? {
        return Err(Error::NotFound);
    }
    let store = Store::open(accounts.dir(id)?)?;
    let disk = store.profile()?.ok_or(Error::NotFound)?;
    accounts::entry_of(id, &disk)?;
    Ok(Loaded { id: id.to_string(), disk, store })
}

impl Shared {
    pub(crate) fn new() -> Self {
        Shared {
            disk: None,
            me: None,
            dek: None,
            state: State::default(),
            endpoint: None,
            epoch: 1,
            acct: None,
            history: Arc::new(History::empty()),
            device_label: None,
            committed: true,
            acc: None,
        }
    }

    /// Puts the account's files in place, locked; a legacy profile is unlocked and brings its
    /// JSON state with it.
    pub(crate) fn install(&mut self, l: Loaded) -> Result<(), Error> {
        self.forget_session();
        self.epoch += 1;
        let mut backing = None;
        if let Disk::Legacy(p) = &l.disk {
            self.state = l.store.state()?;
            let b = Backing::Json(l.store.clone());
            self.history = Arc::new(History::new(l.store.calls()?, Some(b.clone())));
            self.me = Some(Arc::new(Me::load(p.clone(), self.epoch)?));
            backing = Some(b);
        }
        self.acct = Some(Acct { id: l.id, store: l.store, backing });
        self.disk = Some(l.disk);
        self.committed = true;
        Ok(())
    }

    /// Everything of the account that is secret or sealed leaves memory. An identity that was
    /// never committed has nowhere to come back from, so it goes entirely.
    pub(crate) fn forget_session(&mut self) {
        self.epoch += 1;
        self.me = None;
        self.dek = None;
        self.state = State::default();
        self.history = Arc::new(History::empty());
        self.device_label = None;
        self.acc = None;
        if let Some(a) = self.acct.as_mut() {
            a.backing = None;
        }
        if !self.committed {
            self.disk = None;
            self.acct = None;
            self.committed = true;
        }
    }
}

impl Inner {
    /// Writes a snapshot of `State` to the current account.
    pub(crate) fn persist(&self) -> Result<(), Error> {
        self.persist_for(None)
    }

    /// `persist` for something that belongs to session `epoch`: refused (`Locked`) if the account
    /// in memory has been replaced since. Callers must not hold `shared`.
    pub(crate) fn persist_for(&self, epoch: Option<u64>) -> Result<(), Error> {
        let _w = self.writing.lock();
        let (snapshot, backing) = {
            let s = self.shared.lock();
            if epoch.is_some_and(|g| g != s.epoch) {
                return Err(Error::Locked);
            }
            if s.disk.is_none() {
                return Err(Error::NoIdentity);
            }
            if !s.committed {
                return Ok(());
            }
            match s.acct.as_ref().and_then(|a| a.backing.clone()) {
                Some(b) => (s.state.clone(), b),
                None => return Err(Error::Locked),
            }
        };
        self.write_state(&backing, &snapshot)?;
        self.sync_local(epoch);
        Ok(())
    }

    fn write_state(&self, backing: &Backing, state: &State) -> Result<(), Error> {
        if self.fail_writes.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok() {
            return Err(Error::Io("injected write failure".into()));
        }
        backing.save_state(state)
    }

    /// `persist_for` for async code: the write (and its fsyncs) runs on the blocking pool, not on
    /// a worker that other connections need.
    pub(crate) async fn persist_async(self: &Arc<Self>, epoch: u64) -> Result<(), Error> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.persist_for(Some(epoch)))
            .await
            .map_err(|_| Error::Io("persist task failed".into()))?
    }

    /// Fire and forget, for state that is only a cache (a device, a renewed grant).
    pub(crate) fn persist_in_background(self: &Arc<Self>, epoch: u64) {
        let this = self.clone();
        self.handle.spawn_blocking(move || {
            let _ = this.persist_for(Some(epoch));
        });
    }

    pub(crate) fn history(&self) -> Arc<History> {
        self.shared.lock().history.clone()
    }

    pub(crate) fn not_in_call(&self) -> Result<(), Error> {
        let live = self.live.lock();
        if live.call.is_some() || live.waiting.is_some() {
            return Err(Error::InCall);
        }
        Ok(())
    }

    /// Writes a new account's files: the dir (never an existing one), `account.json`, the sealed
    /// store, then the pointer. What it created is removed again if any step fails.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_account_files(
        &self,
        id: &str,
        disk: &Disk,
        phrase: &str,
        dek: &Dek,
        state: &State,
        calls: &[CallRecord],
        label: Option<&str>,
    ) -> Result<Acct, Error> {
        let dir = self.accounts.create(id)?;
        let built = (|| {
            let store = Store::open(&dir)?;
            store.save_profile(disk)?;
            let db = store::seal_into(&store, phrase, dek, state, calls, label)?;
            self.accounts.select(id)?;
            Ok::<_, Error>(Acct { id: id.to_string(), store, backing: Some(Backing::Sealed(db)) })
        })();
        if built.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        built
    }

    /// Restore over an account dir that is already there (its owner forgot the passphrase): the
    /// sealed stores stay, `account.json` gets the new vault. A store that cannot be re-keyed
    /// (it predates the identity-derived keys and the data key is gone) is renamed
    /// `*.unreadable`, never deleted. Nothing of the old vault survives.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn restore_account_files(
        &self,
        id: &str,
        disk: &Disk,
        phrase: &str,
        dek: &Dek,
        state: &State,
        calls: &[CallRecord],
        label: Option<&str>,
    ) -> Result<Acct, Error> {
        let dir = self.accounts.dir(id)?;
        let store = Store::open(&dir)?;
        for name in ["account.redb", "chat.redb"] {
            let path = dir.join(name);
            if rekey::file_needs_migration(&path)? {
                rekey::quarantine(&path)?;
            }
        }
        let db = store::seal_into(&store, phrase, dek, state, calls, label)?;
        store.save_profile(disk)?;
        self.accounts.select(id)?;
        Ok(Acct { id: id.to_string(), store, backing: Some(Backing::Sealed(db)) })
    }

    /// `commit_identity` with the gate held.
    pub(crate) fn commit_locked(self: &Arc<Self>) -> Result<(), Error> {
        let (disk, dek, phrase, state, calls, label) = {
            let s = self.shared.lock();
            let disk = s.disk.clone().ok_or(Error::NoIdentity)?;
            if s.committed {
                return Ok(());
            }
            let phrase = zeroize::Zeroizing::new(s.me.as_ref().ok_or(Error::Locked)?.profile.mnemonic.clone());
            (disk, s.dek.clone().ok_or(Error::Locked)?, phrase, s.state.clone(), s.history.snapshot(), s.device_label.clone())
        };
        let Disk::V2(p) = &disk else { return Err(Error::NoIdentity) };
        let id = accounts::id_of(&p.did)?;
        let acct = if self.accounts.exists(&id)? {
            // A restore over an existing account of this device.
            self.restore_account_files(&id, &disk, &phrase, &dek, &state, &calls, label.as_deref())?
        } else {
            self.create_account_files(&id, &disk, &phrase, &dek, &state, &calls, label.as_deref())?
        };
        {
            let mut s = self.shared.lock();
            if let Some(b) = &acct.backing {
                s.history.attach(b.clone());
            }
            s.acct = Some(acct);
            s.committed = true;
        }
        self.chat_open();
        self.acc_open();
        // Anything that changed while the files were being written.
        self.persist()
    }

    /// Switches (or, with `None`, deselects) the account in memory. Stops everything of the old one.
    pub(crate) async fn replace_account(self: &Arc<Self>, target: Option<Loaded>) -> Result<(), Error> {
        let _life = self.lifecycle.lock().await;
        self.not_in_call()?;
        match &target {
            Some(l) => self.accounts.select(&l.id)?,
            None => self.accounts.deselect()?,
        }
        self.chat_sessions_close();
        self.close_endpoint().await;
        self.chat_close().await;
        {
            let mut s = self.shared.lock();
            s.forget_session();
            s.disk = None;
            s.acct = None;
            s.committed = true;
            if let Some(l) = target {
                s.install(l)?;
            }
        }
        self.emit_status();
        self.events.on_contacts_changed();
        Ok(())
    }

    /// The setting as of now: an expired "until" counts as available again.
    pub(crate) fn availability(&self) -> Availability {
        let a = self.shared.lock().state.availability.clone();
        match a.until {
            Some(t) if a.unavailable && now() >= t => Availability { available: true, until: None },
            _ if a.unavailable => Availability { available: false, until: a.until },
            _ => Availability { available: true, until: None },
        }
    }

    /// Adds to the call log of session `epoch` and writes it in the background; dropped if that
    /// session is over.
    pub(crate) fn log_call(&self, epoch: u64, rec: CallRecord) {
        let history = {
            let s = self.shared.lock();
            if s.epoch != epoch {
                return;
            }
            s.history.clone()
        };
        history.push(rec);
        self.sync_local(Some(epoch));
        self.handle.spawn_blocking(move || {
            if let Err(e) = history.save() {
                tracing::warn!("saving call history: {e}");
            }
        });
    }

    /// An incoming call that never became a `Call` (turned away unavailable, or busy).
    pub(crate) fn log_refused(&self, epoch: u64, call_id: &str, did: &str, name: &str, reason: &str, missed: bool) {
        self.log_call(epoch, CallRecord {
            call_id: call_id.to_string(),
            peer_did: did.to_string(),
            peer_name: name.to_string(),
            incoming: true,
            started_at: now(),
            duration_secs: 0,
            reason: reason.to_string(),
            direct: false,
            missed,
        });
    }

    pub(crate) async fn close_endpoint(&self) {
        let ep = self.shared.lock().endpoint.take();
        if let Some(ep) = ep {
            let _ = tokio::time::timeout(Duration::from_secs(2), ep.close()).await;
        }
    }

    pub(crate) fn me(&self) -> Result<Arc<Me>, Error> {
        let s = self.shared.lock();
        match (&s.me, &s.disk) {
            (Some(me), _) => Ok(me.clone()),
            (None, Some(_)) => Err(Error::Locked),
            (None, None) => Err(Error::NoIdentity),
        }
    }

    pub(crate) fn endpoint(&self) -> Result<Endpoint, Error> {
        self.shared.lock().endpoint.clone().ok_or(Error::NotStarted)
    }

    pub(crate) fn log(&self, line: impl Into<String>) {
        let line = line.into();
        tracing::info!("{line}");
        self.events.on_log(line);
    }

    pub(crate) fn status(&self) -> NodeStatus {
        let s = self.shared.lock();
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
                endpoint_id: match (&s.me, &s.disk) {
                    (Some(m), _) => m.info().device,
                    (None, Some(Disk::V2(p))) => {
                        PublicKey::from_bytes(&p.device_public).map(|k| k.to_string()).unwrap_or_default()
                    }
                    _ => String::new(),
                },
            },
        }
    }

    pub(crate) fn emit_status(&self) {
        self.events.on_status(self.status());
    }

    pub(crate) async fn start(self: Arc<Self>) -> Result<(), Error> {
        let _life = self.lifecycle.lock().await;
        let me = self.me()?;
        if self.shared.lock().endpoint.is_some() {
            return Ok(());
        }
        let transport = QuicTransportConfig::builder()
            // Keeps a ringing call and NAT bindings alive; the relay link has its own pings.
            .keep_alive_interval(Duration::from_secs(5))
            .max_idle_timeout(Some(Duration::from_secs(20).try_into().expect("small timeout")))
            .build();
        let mut builder = Endpoint::builder(presets::N0)
            .secret_key(SecretKey::from_bytes(&me.profile.device_secret))
            .alpns(vec![
                proto::ALPN.to_vec(),
                crate::chat::wire::CHAT_ALPN.to_vec(),
                iroh_blobs::ALPN.to_vec(),
                proto::SELF_ALPN.to_vec(),
                proto::link::LINK_ALPN.to_vec(),
            ])
            .transport_config(transport);
        // Test knob: no UDP of our own, so every packet goes through the relay — the path a
        // call takes when hole punching fails.
        if std::env::var_os("P2P_RELAY_ONLY").is_some() {
            builder = builder.clear_ip_transports();
            self.log("relay-only mode");
        }
        let ep = builder.bind().await.map_err(Error::net)?;
        // Locked meanwhile (cannot happen under the lifecycle lock, but an endpoint must never
        // outlive the secrets it was made from).
        if self.me().is_err() {
            ep.close().await;
            return Err(Error::Locked);
        }
        // Decide under the lock, await outside it (a guard across an await isn't Send).
        let lost_race = {
            let mut s = self.shared.lock();
            let taken = s.endpoint.is_some();
            if !taken {
                s.endpoint = Some(ep.clone());
            }
            taken
        };
        if lost_race {
            ep.close().await;
            return Ok(());
        }
        self.log(format!("endpoint {} bound", ep.id()));
        self.emit_status();
        self.chat_started();
        self.selfsync_started();
        self.sync_kick();

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
                // The reserved slots are for contacts' devices, which we can only tell apart
                // after the handshake.
                let (permit, reserved) = match this.pending.clone().try_acquire_owned() {
                    Ok(p) => (p, false),
                    Err(_) => match this.reserved.clone().try_acquire_owned() {
                        Ok(p) => (p, true),
                        Err(_) => {
                            incoming.refuse();
                            continue;
                        }
                    },
                };
                let this = this.clone();
                tokio::spawn(async move {
                    let mut accepting = match incoming.accept() {
                        Ok(a) => a,
                        Err(_) => return,
                    };
                    let alpn = match accepting.alpn().await {
                        Ok(a) => a,
                        Err(_) => return,
                    };
                    if alpn.as_slice() == proto::SELF_ALPN {
                        drop(permit);
                        if let Err(e) = this.clone().handle_self_incoming(accepting).await {
                            tracing::debug!("incoming self-sync: {e}");
                        }
                        return;
                    }
                    if alpn.as_slice() == proto::link::LINK_ALPN {
                        drop(permit);
                        if let Err(e) = this.clone().handle_link_incoming(accepting).await {
                            tracing::debug!("incoming link: {e}");
                        }
                        return;
                    }
                    if alpn.as_slice() == crate::chat::wire::CHAT_ALPN || alpn.as_slice() == iroh_blobs::ALPN {
                        let chat = alpn.as_slice() == crate::chat::wire::CHAT_ALPN;
                        if let Err(e) = this.clone().handle_chat_incoming(accepting, chat, permit).await {
                            tracing::debug!("incoming chat: {e}");
                        }
                        return;
                    }
                    if let Err(e) = this.clone().handle_incoming(accepting, permit, reserved).await {
                        tracing::debug!("incoming: {e}");
                    }
                });
            }
            this.log("endpoint closed");
        });
        Ok(())
    }

    pub(crate) async fn ticket(self: &Arc<Self>) -> Result<String, Error> {
        let me = self.me()?;
        // Right after start the relay isn't known yet; a ticket without it leaves the joiner
        // to DNS discovery, so give the relay a few seconds to come up first.
        let relay = match self.endpoint() {
            Ok(ep) => {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
                loop {
                    let r = relay_of(&ep);
                    if r.is_some() || tokio::time::Instant::now() > deadline {
                        break r;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            Err(_) => None,
        };
        let text = {
            let mut s = self.shared.lock();
            let now = now();
            if let Some(text) = &s.state.ticket
                && let Ok(t) = ContactTicket::from_text(text)
                && let Ok(claim) = t.verify(now)
                // Still ours (identity unchanged), still days of life left, and points at our
                // current relay so the joiner can dial without discovery.
                && claim.iss == me.id.did()
                && claim.name == me.profile.name
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
            text
        };
        self.persist_async(me.epoch).await?;
        Ok(text)
    }

    pub(crate) async fn add_contact(self: Arc<Self>, text: String) -> Result<Contact, Error> {
        let me = self.me()?;
        let ticket = ContactTicket::from_text(&text)?;
        let claim = ticket.verify(now())?;
        if claim.iss == me.id.did() {
            return Err(Error::Protocol("that is your own ticket".into()));
        }
        // The relay is signed by its owner but gets dialled and stored here: same sanity check
        // as for a hint, and a ticket that fails it is refused rather than half-used.
        let relay = relay_hint(&claim.relay);
        if claim.relay.is_some() && relay.is_none() {
            return Err(Error::Protocol("that ticket names a relay we won't use".into()));
        }
        let ep = self.endpoint()?;
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
        let addr = addr_for(&proto::device_from_text(&claim.device)?, relay.as_deref())?;
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
            Msg::Reject { reason } => return Err(Error::Rejected(clip_reason(&reason))),
            other => proto::accept_contact_welcome(&me.id, &pending, &other, remote, now())?,
        };
        ctrl.finish();
        conn.close(0u32.into(), b"added");
        // We scanned their ticket ourselves: that is the one way back from a block.
        let contact = self.save_contact(me.epoch, new, relay, true).await?;
        self.log(format!("contact {} added", contact.name));
        Ok(contact)
    }

    /// `lift_block`: only when the user scanned this contact's ticket; a contact who arrives on
    /// their own (they scanned ours) must not undo a removal.
    pub(crate) async fn save_contact(
        self: &Arc<Self>,
        epoch: u64,
        new: proto::NewContact,
        relay: Option<String>,
        lift_block: bool,
    ) -> Result<Contact, Error> {
        let stored = stored_contact(&new, relay);
        let contact = Contact::from(&stored);
        {
            let mut s = self.shared.lock();
            if s.epoch != epoch {
                return Err(Error::Locked);
            }
            apply_contact(&mut s.state, stored, new.device, lift_block)?;
        }
        self.persist_async(epoch).await?;
        self.events.on_contacts_changed();
        Ok(contact)
    }

    /// An incoming `ContactHello`, answered with a `Welcome` only after the contact, the spent
    /// nonce and the dropped ticket are durable in one write. Blocking: runs on the blocking
    /// pool. If the write fails nothing changes in memory either, so the ticket is still good
    /// and the joiner can retry.
    fn accept_hello(&self, me: &Me, hello: &Msg, remote: [u8; 32], hint: Option<String>) -> Result<(Msg, Contact), HelloError> {
        let _w = self.writing.lock();
        let (mut next, backing) = {
            let s = self.shared.lock();
            if s.epoch != me.epoch {
                return Err(HelloError::Failed(Error::Locked));
            }
            (s.state.clone(), s.committed.then(|| s.acct.as_ref().and_then(|a| a.backing.clone())).flatten())
        };
        let now = now();
        let current = current_nonce(&next, now);
        let (welcome, new) = proto::accept_contact_hello(&me.id, me.device, hello, remote, now, &next.redeemed, GRANT_TTL)
            .map_err(|e| HelloError::Refused(e.to_string()))?;
        // Only the ticket we hand out right now can be redeemed: an older one that leaked (or
        // was reset) is dead even though its nonce is unspent.
        if current.as_deref() != Some(new.redeemed_nonce.as_str()) {
            return Err(HelloError::Refused("not the current ticket".into()));
        }
        if next.blocked.contains(&new.did) {
            return Err(HelloError::Refused("blocked".into()));
        }
        let stored = stored_contact(&new, hint);
        let contact = Contact::from(&stored);
        let apply = |st: &mut State| {
            st.redeemed.insert(new.redeemed_nonce.clone());
            // Spent: the next `my_ticket` mints a fresh one.
            st.ticket = None;
            apply_contact(st, stored.clone(), new.device, false)
        };
        apply(&mut next).map_err(HelloError::Failed)?;
        if let Some(b) = &backing {
            self.write_state(b, &next).map_err(HelloError::Failed)?;
        }
        let mut s = self.shared.lock();
        if s.epoch != me.epoch {
            return Err(HelloError::Failed(Error::Locked));
        }
        apply(&mut s.state).map_err(HelloError::Failed)?;
        Ok((welcome, contact))
    }

    /// The slot in `permit` is held until this connection has become a call or a contact, or is
    /// over; `reserved` says it came from the contacts-only pool.
    pub(crate) async fn handle_incoming(
        self: Arc<Self>,
        incoming: iroh::endpoint::Accepting,
        mut permit: OwnedSemaphorePermit,
        reserved: bool,
    ) -> Result<(), Error> {
        // One deadline for the whole way to the hello.
        let (conn, remote, mut ctrl, first) = tokio::time::timeout(HELLO_TIMEOUT, async {
            let conn = incoming.await.map_err(Error::net)?;
            let remote = *conn.remote_id().as_bytes();
            if reserved && !self.is_contact_device(&remote) {
                // A stranger in a contact's slot: it may stay only if the general pool has room.
                match self.pending.clone().try_acquire_owned() {
                    Ok(p) => permit = p,
                    Err(_) => {
                        conn.close(0u32.into(), b"busy");
                        return Err(Error::Busy);
                    }
                }
            }
            let (send, recv) = conn.accept_bi().await.map_err(Error::net)?;
            let mut ctrl = Ctrl::new(send, recv);
            let first = ctrl
                .recv()
                .await?
                .ok_or_else(|| Error::Protocol("peer closed before hello".into()))?;
            Ok::<_, Error>((conn, remote, ctrl, first))
        })
        .await
        .map_err(|_| Error::Timeout)??;
        let me = self.me()?;
        match first {
            hello @ Msg::ContactHello { .. } => {
                let Msg::ContactHello { relay: hint, .. } = &hello else { unreachable!() };
                let hint = relay_hint(hint);
                let (this, me2, hello2) = (self.clone(), me.clone(), hello.clone());
                let accepted = tokio::task::spawn_blocking(move || this.accept_hello(&me2, &hello2, remote, hint))
                    .await
                    .map_err(|_| Error::Io("contact task failed".into()))?;
                match accepted {
                    Ok((welcome, contact)) => {
                        // Durable now; only then does the joiner hear of it.
                        self.events.on_contacts_changed();
                        ctrl.send(&welcome).await?;
                        ctrl.finish();
                        drop(permit);
                        self.log(format!("contact {} added us", contact.name));
                        // Let the joiner read the welcome and close first.
                        let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
                    }
                    Err(HelloError::Refused(e)) => {
                        self.log(format!("rejected contact hello: {e}"));
                        ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
                        ctrl.finish();
                        let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                    }
                    Err(HelloError::Failed(e)) => {
                        // Nothing was saved, so no Welcome: the joiner must not hold a contact
                        // that we would forget.
                        let _ = ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await;
                        ctrl.finish();
                        let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                        return Err(e);
                    }
                }
                Ok(())
            }
            hello @ Msg::CallHello { .. } => self.incoming_call(conn, ctrl, hello, remote, &me, permit).await,
            other => {
                tracing::debug!("unexpected first message {}", msg_name(&other));
                ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
                ctrl.finish();
                let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                Ok(())
            }
        }
    }

    pub(crate) fn is_contact_device(&self, device: &[u8; 32]) -> bool {
        self.shared.lock().state.contacts.iter().any(|c| c.has_device(device))
    }

    pub(crate) async fn incoming_call(
        self: Arc<Self>,
        conn: Connection,
        mut ctrl: Ctrl,
        hello: Msg,
        remote: [u8; 32],
        me: &Me,
        permit: OwnedSemaphorePermit,
    ) -> Result<(), Error> {
        let Msg::CallHello { call_id, relay: hint, devices: list, .. } = &hello else { unreachable!() };
        let (call_id, hint, list) = (call_id.clone(), relay_hint(hint), list.clone());
        // It reaches the UI and logs: keep it to what we ourselves generate.
        if call_id.is_empty() || call_id.len() > 64 || !call_id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
            ctrl.finish();
            let _ = tokio::time::timeout(LINGER, conn.closed()).await;
            return Ok(());
        }
        let verified = {
            let s = self.shared.lock();
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
                let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                return Ok(());
            }
        };
        let name = {
            let s = self.shared.lock();
            s.state.contacts.iter().find(|c| c.did == caller.did).map(display_name).unwrap_or_default()
        };
        if !self.availability().available {
            // Same answer as any other refusal: the caller must not learn we are "away".
            self.log("incoming call turned away: unavailable");
            self.log_refused(me.epoch, &call_id, &caller.did, &name, "unavailable", false);
            ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
            ctrl.finish();
            let _ = tokio::time::timeout(LINGER, conn.closed()).await;
            return Ok(());
        }
        let info = CallInfo { call_id, peer_did: caller.did.clone(), peer_name: name, incoming: true };
        self.yield_on_glare(&caller.did, me.device, remote);
        let (call, cmds) = match self.begin_call(info.clone(), CallState::Ringing, false, me.epoch) {
            Ok(c) => c,
            // In a call: ring quietly over it, if nothing is waiting there already.
            Err(_) => match self.begin_waiting(info.clone(), me.epoch) {
                Ok(c) => {
                    self.log("incoming call while in a call: waiting");
                    c
                }
                Err(_) => {
                    self.log_refused(me.epoch, &info.call_id, &info.peer_did, &info.peer_name, "busy", true);
                    ctrl.send(&Msg::Busy).await?;
                    ctrl.finish();
                    let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                    return Ok(());
                }
            },
        };
        // From here every exit must free the slot; the guard does it if nothing else did.
        let _slot = SlotGuard { inner: &self, call: call.clone() };
        *call.conn.lock() = Some(conn.clone());
        // Their device list first (it decides whether this device is dialled), then this device.
        if let Some(list) = &list {
            self.accept_device_list(me.epoch, &caller.did, list);
        }
        self.note_device(me.epoch, &caller.did, remote, hint);
        ctrl.send(&Msg::Ringing).await?;
        self.log(format!("incoming call from {}", info.peer_name));
        {
            // Not if the call already ended meanwhile: the UI never heard of it.
            let _order = call.notify.lock();
            if !matches!(call.state(), CallState::Ended { .. }) {
                self.events.on_incoming_call(info.clone());
                self.events.on_call_state(info.call_id.clone(), CallState::Ringing);
            }
        }
        // Authenticated and holding the call slot: no longer one of the unproven connections.
        drop(permit);
        self.clone().run_call(call, conn, ctrl, cmds, Some(caller.did), None).await;
        Ok(())
    }

    /// Both sides dialled each other at once: each would answer the other's hello with Busy
    /// and both calls die. The call from the lower device key wins; the higher side drops its
    /// own outgoing call so the incoming one can take the slot.
    pub(crate) fn yield_on_glare(&self, peer: &str, mine: [u8; 32], theirs: [u8; 32]) {
        let ours = self.live.lock().call.clone();
        if let Some(ours) = ours
            && !ours.info.incoming
            && ours.info.peer_did == peer
            && matches!(ours.state(), CallState::Dialing | CallState::Ringing)
            && mine > theirs
        {
            self.end_call(&ours, "superseded".into());
        }
    }

    /// The caller's device and relay as of this call, so our next call to them dials straight
    /// there. A device we have not seen before resets `verified`, exactly as when they add us.
    /// A contact whose signed device list we hold is dialled by that list only: a device missing
    /// from it is accepted for this call (same DID) but not added.
    pub(crate) fn note_device(self: &Arc<Self>, epoch: u64, did: &str, device: [u8; 32], relay: Option<String>) {
        let changed = {
            let mut s = self.shared.lock();
            if s.epoch != epoch {
                return;
            }
            let Some(c) = s.state.contacts.iter_mut().find(|c| c.did == did) else { return };
            let mut changed = false;
            if c.has_device(&device) {
                if let Some(d) = c.devices.iter_mut().find(|d| d.device == device)
                    && relay.is_some()
                    && d.relay != relay
                {
                    d.relay = relay;
                    changed = true;
                }
                if c.device_list.is_none() && c.devices.first().map(|d| d.device) != Some(device) {
                    let at = c.devices.iter().position(|d| d.device == device).unwrap_or(0);
                    let d = c.devices.remove(at);
                    c.devices.insert(0, d);
                    changed = true;
                }
            } else {
                changed = std::mem::replace(&mut c.verified, false);
                if c.device_list.is_none() {
                    c.devices.insert(0, ContactDevice { device, relay });
                    c.devices.truncate(MAX_DEVICES);
                    changed = true;
                }
            }
            changed
        };
        if changed {
            self.persist_in_background(epoch);
            self.events.on_contacts_changed();
        }
    }

    /// A contact's signed `DeviceList` from a hello or an accept: kept if it verifies for that
    /// DID and is newer than the one we hold. A new device in it resets `verified`.
    pub(crate) fn accept_device_list(self: &Arc<Self>, epoch: u64, did: &str, blob: &proto::SignedBlob) -> bool {
        let changed = {
            let mut s = self.shared.lock();
            if s.epoch != epoch {
                return false;
            }
            match s.state.contacts.iter_mut().find(|c| c.did == did) {
                Some(c) => merge_device_list(c, blob),
                None => false,
            }
        };
        if changed {
            self.persist_in_background(epoch);
            self.events.on_contacts_changed();
        }
        changed
    }

    /// Our own `DeviceList`: every live install of the registry (this device first, on its
    /// current relay). Re-signed with a higher `seq` only when its content changed.
    pub(crate) fn my_device_list(self: &Arc<Self>, me: &Me) -> Option<proto::SignedBlob> {
        let relay = self.endpoint().ok().as_ref().and_then(relay_of);
        let (blob, dirty) = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return None;
            }
            let label = s.device_label.clone().unwrap_or_default();
            let st = &mut s.state;
            let mut dirty = false;
            match st.registry.iter_mut().find(|e| e.device == me.device) {
                Some(e) => {
                    if relay.is_some() && e.relay != relay {
                        e.relay = relay;
                        dirty = true;
                    }
                    if !label.is_empty() && e.label != label {
                        e.label = label;
                        dirty = true;
                    }
                    e.removed = false;
                }
                None => {
                    st.registry.insert(0, OwnDevice {
                        device: me.device,
                        attestation: me.attestation.clone(),
                        label,
                        removed: false,
                        last_seen: now(),
                        relay,
                    });
                    dirty = true;
                }
            }
            let mut want: Vec<([u8; 32], Option<String>)> = vec![(me.device, None)];
            for e in &st.registry {
                if e.device == me.device {
                    want[0].1 = e.relay.clone();
                } else if !e.removed {
                    want.push((e.device, e.relay.clone()));
                }
            }
            want.truncate(MAX_DEVICES);
            let same = st.own_list.as_ref().is_some_and(|b| {
                proto::verify_device_list(b, me.id.did()).is_ok_and(|l| {
                    l.devices.len() == want.len()
                        && l.devices.iter().zip(&want).all(|(e, (d, r))| e.device_key().ok() == Some(*d) && &e.relay == r)
                })
            });
            if !same {
                let prev = st
                    .own_list
                    .as_ref()
                    .and_then(|b| proto::verify_device_list(b, me.id.did()).ok())
                    .map_or(0, |l| l.seq);
                st.own_list = Some(proto::sign_device_list(&me.id, &want, now_ms().max(prev + 1)));
                dirty = true;
            }
            (st.own_list.clone(), dirty)
        };
        if dirty {
            self.persist_in_background(me.epoch);
        }
        blob
    }

    fn new_call(
        info: CallInfo,
        state: CallState,
        epoch: u64,
    ) -> Result<(Arc<Call>, mpsc::UnboundedReceiver<Cmd>), Error> {
        // Everything slow or fallible comes before the slot is taken.
        let sender = audio::Sender::new(48_000)?;
        let receiver = audio::Receiver::new()?;
        let (tx, rx) = mpsc::unbounded_channel();
        let call = Arc::new(Call {
            info,
            conn: Mutex::new(None),
            state: Mutex::new(state),
            cmd: tx,
            started: Instant::now(),
            sender: Mutex::new(sender),
            receiver: Mutex::new(receiver),
            mic: Mutex::new(Vec::with_capacity(audio::FRAME * 2)),
            played: Mutex::new(VecDeque::with_capacity(audio::RATE as usize)),
            sent: Mutex::new(0),
            tone: Mutex::new(None),
            ended: Mutex::new(false),
            notify: ReentrantMutex::new(()),
            last_rx: Mutex::new(Instant::now()),
            started_at: now(),
            active_at: Mutex::new(None),
            direct: Mutex::new(false),
            waiting: AtomicBool::new(false),
            answer_on_promote: AtomicBool::new(false),
            epoch,
            ended_wake: tokio::sync::Notify::new(),
        });
        Ok((call, rx))
    }

    /// Takes the call slot. With `announce` the first state is delivered to the UI before any
    /// other thread can see (and end) the call, and after the previous call's events.
    pub(crate) fn begin_call(
        &self,
        info: CallInfo,
        state: CallState,
        announce: bool,
        epoch: u64,
    ) -> Result<(Arc<Call>, mpsc::UnboundedReceiver<Cmd>), Error> {
        let (call, rx) = Self::new_call(info, state.clone(), epoch)?;
        {
            let _first = call.notify.lock();
            let previous = {
                let mut live = self.live.lock();
                if live.call.is_some() {
                    return Err(Error::Busy);
                }
                live.call = Some(call.clone());
                live.last.take()
            };
            // The previous call's `Ended` is still being delivered on another thread, perhaps.
            let _previous = previous.as_ref().map(|p| p.notify.lock());
            if announce {
                self.log(format!("call {}: {state:?}", call.info.call_id));
                self.events.on_call_state(call.info.call_id.clone(), state);
            }
        }
        Ok((call, rx))
    }

    /// Parks an incoming call beside the active one. Busy unless there is an answered call
    /// and nothing already waiting.
    fn begin_waiting(&self, info: CallInfo, epoch: u64) -> Result<(Arc<Call>, mpsc::UnboundedReceiver<Cmd>), Error> {
        let (call, rx) = Self::new_call(info, CallState::Ringing, epoch)?;
        call.waiting.store(true, Ordering::SeqCst);
        let mut live = self.live.lock();
        match &live.call {
            Some(c) if c.state() == CallState::Active && live.waiting.is_none() => {
                live.waiting = Some(call.clone());
                Ok((call, rx))
            }
            _ => Err(Error::Busy),
        }
    }

    pub(crate) fn start_call(self: Arc<Self>, did: String) -> Result<CallInfo, Error> {
        let me = self.me()?;
        let ep = self.endpoint()?;
        let contact = {
            let s = self.shared.lock();
            s.state.contacts.iter().find(|c| c.did == did).cloned().ok_or(Error::NotFound)?
        };
        let info = CallInfo {
            call_id: random_id(),
            peer_did: did,
            peer_name: display_name(&contact),
            incoming: false,
        };
        let (call, cmds) = self.begin_call(info.clone(), CallState::Dialing, true, me.epoch)?;
        let this = self.clone();
        self.handle.spawn(async move {
            // A panic below must not leave the slot busy forever.
            let _slot = SlotGuard { inner: &this, call: call.clone() };
            this.clone().run_outgoing(ep, me, contact, call, cmds).await;
        });
        Ok(info)
    }

    /// One logical call to a person: one call id, dialled at all their known devices at once
    /// (design: `docs/design/device-linking.md` section 7). We are the coordinator:
    ///
    /// - the first `Accept` wins and its connection becomes the call; every other leg gets
    ///   `Cancel(AnsweredElsewhere)` (a leg that accepted too, simultaneously, is sent the same
    ///   and ends on its side);
    /// - a `Decline` from any device ends the call `declined`, the rest get
    ///   `Cancel(DeclinedElsewhere)`;
    /// - `Busy` / `Reject` / a dead connection only drop that leg; when none is left the call ends
    ///   `no_answer` if some device rang and gave up, else `busy` if every device we reached said
    ///   busy, else `unreachable`;
    /// - hanging up while dialling or ringing sends `Cancel(CallerHangup)` to every leg (a plain
    ///   `Hangup` when we hold no device list for them: that is how a peer from before device lists
    ///   understands it).
    ///
    /// Media is read from the winning connection only; messages of the other legs are not read
    /// once they are cancelled.
    async fn run_outgoing(
        self: Arc<Self>,
        ep: Endpoint,
        me: Arc<Me>,
        contact: StoredContact,
        call: Arc<Call>,
        mut cmds: mpsc::UnboundedReceiver<Cmd>,
    ) {
        let list = self.my_device_list(&me);
        let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<(usize, LegEvent)>();
        let mut legs: Vec<Option<mpsc::UnboundedSender<Msg>>> = Vec::new();
        for (idx, device) in contact.devices.iter().enumerate() {
            let (tx, rx) = mpsc::unbounded_channel();
            legs.push(Some(tx));
            let leg = Leg {
                ep: ep.clone(),
                device: device.clone(),
                hello: {
                    let mut hello = proto::call_hello(
                        &me.id,
                        me.attestation.clone(),
                        contact.grant_from_them.clone(),
                        call.info.call_id.clone(),
                    );
                    if let Msg::CallHello { relay, devices, .. } = &mut hello {
                        *relay = relay_of(&ep);
                        *devices = list.clone();
                    }
                    hello
                },
                idx,
                events: ev_tx.clone(),
                cmds: rx,
                name: contact.name.clone(),
            };
            self.handle.spawn(leg.run());
        }
        drop(ev_tx);
        // What each leg is doing, to decide the end when none is left.
        #[derive(Clone, Copy, PartialEq)]
        enum LegPhase {
            Dialing,
            Live,
            Busy,
            Gone,
            GaveUp,
        }
        let mut state = vec![LegPhase::Dialing; legs.len()];
        // Telling the others is queued; each leg task sends it and closes.
        let cancel_others = |legs: &mut Vec<Option<mpsc::UnboundedSender<Msg>>>, keep: Option<usize>, msg: Msg| {
            for (i, l) in legs.iter_mut().enumerate() {
                if Some(i) != keep
                    && let Some(tx) = l.take()
                {
                    let _ = tx.send(msg.clone());
                }
            }
        };
        let hangup_msg = if contact.device_list.is_some() {
            Msg::Cancel { reason: proto::CancelReason::CallerHangup }
        } else {
            Msg::Hangup
        };
        let dial_deadline = tokio::time::Instant::now() + DIAL_TOTAL;
        let mut ring_deadline: Option<tokio::time::Instant> = None;
        let reason: String = loop {
            if *call.ended.lock() {
                // Ended from outside (glare: a sibling's simultaneous call won; lock).
                cancel_others(&mut legs, None, hangup_msg.clone());
                return;
            }
            if state.iter().all(|s| matches!(s, LegPhase::Busy | LegPhase::Gone | LegPhase::GaveUp)) {
                break if state.contains(&LegPhase::GaveUp) {
                    "no_answer"
                } else if state.contains(&LegPhase::Busy) {
                    "busy"
                } else {
                    "unreachable"
                }
                .into();
            }
            tokio::select! {
                ev = ev_rx.recv() => {
                    let Some((idx, ev)) = ev else { break "unreachable".into() };
                    match ev {
                        LegEvent::Connected => {
                            state[idx] = LegPhase::Live;
                            ring_deadline.get_or_insert(tokio::time::Instant::now() + RING_TIMEOUT);
                        }
                        LegEvent::Ringing => {
                            if call.state() == CallState::Dialing {
                                self.set_state(&call, CallState::Ringing);
                            }
                        }
                        LegEvent::Accepted { renewed_grant, devices, conn, ctrl } => {
                            legs[idx] = None;
                            if *call.ended.lock() {
                                conn.close(0u32.into(), b"bye");
                                continue;
                            }
                            if let Some(g) = renewed_grant {
                                self.renew_grant(call.epoch, &call.info.peer_did, g);
                            }
                            if let Some(list) = &devices {
                                self.accept_device_list(call.epoch, &call.info.peer_did, list);
                            }
                            cancel_others(&mut legs, None, Msg::Cancel { reason: proto::CancelReason::AnsweredElsewhere });
                            // A device that answered at the same moment has an `Accepted` still in
                            // the queue: it gets the same cancel.
                            Self::cancel_late_answers(&self.handle, ev_rx, Msg::Cancel { reason: proto::CancelReason::AnsweredElsewhere });
                            *call.conn.lock() = Some(conn.clone());
                            let datagrams = self.start_media(&call, &conn);
                            self.set_state(&call, CallState::Active);
                            self.clone().run_call(call, conn, *ctrl, cmds, None, Some(datagrams)).await;
                            return;
                        }
                        LegEvent::Declined(why) => {
                            self.log(format!("declined: {}", clip_reason(&why)));
                            cancel_others(&mut legs, Some(idx), Msg::Cancel { reason: proto::CancelReason::DeclinedElsewhere });
                            break "declined".into();
                        }
                        LegEvent::Busy => {
                            legs[idx] = None;
                            state[idx] = LegPhase::Busy;
                        }
                        LegEvent::GaveUp => {
                            legs[idx] = None;
                            state[idx] = LegPhase::GaveUp;
                        }
                        LegEvent::Failed(why) => {
                            self.log(format!("device {idx} of {}: {why}", contact.name));
                            legs[idx] = None;
                            state[idx] = LegPhase::Gone;
                        }
                    }
                }
                cmd = cmds.recv() => match cmd {
                    Some(Cmd::Answer) => {}
                    // Decline / hangup while dialling or ringing, or the handle went away.
                    _ => {
                        cancel_others(&mut legs, None, hangup_msg.clone());
                        break "cancelled".into();
                    }
                },
                _ = call.ended_wake.notified() => {}
                _ = tokio::time::sleep_until(dial_deadline), if ring_deadline.is_none() => {
                    cancel_others(&mut legs, None, hangup_msg.clone());
                    break "unreachable".into();
                }
                _ = async { tokio::time::sleep_until(ring_deadline.expect("guarded")).await }, if ring_deadline.is_some() => {
                    cancel_others(&mut legs, None, Msg::Hangup);
                    break "no_answer".into();
                }
            }
        };
        // Answers that crossed our decision (or a hangup) still need telling.
        let late = match reason.as_str() {
            "declined" => Some(Msg::Cancel { reason: proto::CancelReason::DeclinedElsewhere }),
            "cancelled" => Some(hangup_msg.clone()),
            _ => None,
        };
        if let Some(late) = late {
            Self::cancel_late_answers(&self.handle, ev_rx, late);
        }
        self.end_call(&call, reason);
    }

    /// Keeps reading the legs' events after the call was decided: a device whose `Accept` was
    /// already on its way gets `msg` (a `Cancel`) and is closed, so it stops instead of waiting
    /// on a call nobody will carry. Ends when the last leg task is gone.
    fn cancel_late_answers(
        handle: &tokio::runtime::Handle,
        mut events: mpsc::UnboundedReceiver<(usize, LegEvent)>,
        msg: Msg,
    ) {
        handle.spawn(async move {
            while let Some((_, ev)) = events.recv().await {
                if let LegEvent::Accepted { conn, mut ctrl, .. } = ev {
                    let _ = ctrl.send(&msg).await;
                    ctrl.finish();
                    let _ = tokio::time::timeout(Duration::from_millis(300), conn.closed()).await;
                    conn.close(0u32.into(), b"bye");
                }
            }
        });
    }

    /// Drives one call from ringing to the end, on either side. `incoming_from` is the caller's
    /// DID when we are the callee, so the answer can carry a renewed grant.
    pub(crate) async fn run_call(
        self: Arc<Self>,
        call: Arc<Call>,
        conn: Connection,
        mut ctrl: Ctrl,
        mut cmds: mpsc::UnboundedReceiver<Cmd>,
        incoming_from: Option<String>,
        preactive: Option<oneshot::Sender<()>>,
    ) {
        let ring = if call.waiting.load(Ordering::SeqCst) { waiting_ring_timeout() } else { RING_TIMEOUT };
        let ring_deadline = tokio::time::Instant::now() + ring;
        let mut datagrams: Option<oneshot::Sender<()>> = preactive;
        let mut watchdog = tokio::time::interval(Duration::from_secs(5));
        watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // How a call that dies under us ends, by who we are and how far it got.
        let lost = |active: bool| -> String {
            if active { "connection_lost" } else if incoming_from.is_some() { "cancelled" } else { "unreachable" }.into()
        };
        let reason: String = loop {
            if *call.ended.lock() {
                break "ended".into();
            }
            let active = call.state() == CallState::Active;
            tokio::select! {
                msg = ctrl.recv() => match msg {
                    Ok(Some(Msg::Ringing)) if !active => self.set_state(&call, CallState::Ringing),
<<<<<<< HEAD
                    Ok(Some(Msg::Accept { renewed_grant })) if incoming_from.is_none() && !active => {
                        if let Some(g) = renewed_grant {
                            self.renew_grant(call.epoch, &call.info.peer_did, g);
=======
                    // The caller told us another of our devices took the call (or declined it),
                    // or that it gave up. If we had answered meanwhile, we stop at once.
                    Ok(Some(Msg::Cancel { reason })) if incoming_from.is_some() => {
                        self.log(format!("call cancelled by the caller: {reason:?}"));
                        break match reason {
                            proto::CancelReason::AnsweredElsewhere => "answered_elsewhere",
                            proto::CancelReason::DeclinedElsewhere => "declined_elsewhere",
                            proto::CancelReason::CallerHangup if active => "hangup_remote",
                            proto::CancelReason::CallerHangup => "cancelled",
>>>>>>> 08ade9b (core: contacts dial all devices of a person; signed DeviceList distributed; first answer wins, global decline, answered/declined elsewhere)
                        }
                        .into();
                    }
                    Ok(Some(Msg::Decline { reason })) => {
                        self.log(format!("declined: {}", clip_reason(&reason)));
                        break "declined".into();
                    }
                    Ok(Some(Msg::Busy)) => break "busy".into(),
                    // A refusal (not a contact there, blocked, or away) reads as "couldn't reach".
                    Ok(Some(Msg::Reject { reason })) => {
                        self.log(format!("rejected: {}", clip_reason(&reason)));
                        break "unreachable".into();
                    }
                    Ok(Some(Msg::Hangup)) => {
                        // A callee only hangs up unanswered when its ring timed out; a refusal is a Decline.
                        break if active { "hangup_remote" } else if incoming_from.is_some() { "cancelled" } else { "no_answer" }.into();
                    }
                    Ok(None) => break if active { "hangup_remote".into() } else { lost(false) },
                    Ok(Some(other)) => self.log(format!("ignoring {} mid-call", msg_name(&other))),
                    Err(e) => {
                        self.log(format!("connection lost: {e}"));
                        break lost(active);
                    }
                },
                cmd = cmds.recv() => match cmd {
                    Some(Cmd::Answer) if incoming_from.is_some() && !active => {
                        let me = match self.me() { Ok(m) => m, Err(_) => break "hangup_local".into() };
                        let renewed = incoming_from.as_deref().map(|did| proto::issue_grant(&me.id, did, now(), GRANT_TTL));
<<<<<<< HEAD
                        if let Err(e) = ctrl.send(&Msg::Accept { renewed_grant: renewed }).await {
=======
                        let devices = self.my_device_list(&me);
                        if let Err(e) = ctrl.send(&Msg::Accept { renewed_grant: renewed, devices }).await {
>>>>>>> 08ade9b (core: contacts dial all devices of a person; signed DeviceList distributed; first answer wins, global decline, answered/declined elsewhere)
                            self.log(format!("connection lost: {e}"));
                            break lost(false);
                        }
                        datagrams = Some(self.start_media(&call, &conn));
                        self.set_state(&call, CallState::Active);
                    }
                    Some(Cmd::Answer) => {}
                    // Not yet answered: the callee refusing is a decline, the caller giving up
                    // is a hangup (the callee shows it as missed).
                    Some(Cmd::Decline) | Some(Cmd::Hangup) if !active && incoming_from.is_some() => {
                        // Declined over another call: the caller is told we are busy.
                        let reply = if call.waiting.load(Ordering::SeqCst) {
                            Msg::Busy
                        } else {
                            Msg::Decline { reason: "declined".into() }
                        };
                        let _ = ctrl.send(&reply).await;
                        break "declined_local".into();
                    }
                    Some(Cmd::Decline) | Some(Cmd::Hangup) if !active => {
                        let _ = ctrl.send(&Msg::Hangup).await;
                        break "cancelled".into();
                    }
                    Some(Cmd::Decline) | Some(Cmd::Hangup) | None => {
                        let _ = ctrl.send(&Msg::Hangup).await;
                        break "hangup_local".into();
                    }
                },
                why = conn.closed() => {
                    self.log(format!("connection lost: {why}"));
                    break lost(active);
                }
                _ = watchdog.tick(), if active => {
                    if call.last_rx.lock().elapsed() > NO_AUDIO_TIMEOUT {
                        let _ = ctrl.send(&Msg::Hangup).await;
                        break "connection_lost".into();
                    }
                }
                _ = tokio::time::sleep_until(ring_deadline), if !active => {
                    let _ = ctrl.send(&Msg::Hangup).await;
                    break "no_answer".into();
                }
            }
        };
        ctrl.finish();
        drop(datagrams);
        // Free the slot now, so a call placed right after this one isn't Busy; the connection
        // gets a moment in the background for the final frame to leave.
        self.note_direct(&call);
        call.conn.lock().take();
        self.end_call(&call, reason);
        tokio::spawn(async move {
            let _ = tokio::time::timeout(Duration::from_millis(300), conn.closed()).await;
            conn.close(0u32.into(), b"bye");
        });
    }

    /// Spawns the datagram reader; dropping the returned sender stops it.
    pub(crate) fn start_media(&self, call: &Arc<Call>, conn: &Connection) -> oneshot::Sender<()> {
        let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
        let call = call.clone();
        let conn = conn.clone();
        *call.last_rx.lock() = Instant::now();
        self.handle.spawn(async move {
            loop {
                tokio::select! {
                    d = conn.read_datagram() => match d {
                        Ok(d) => {
                            *call.last_rx.lock() = Instant::now();
                            call.receiver.lock().push(&d, call.now_ms());
                        }
                        Err(_) => break,
                    },
                    _ = &mut stop_rx => break,
                }
            }
        });
        stop_tx
    }

    /// A callee's `Accept` may carry a fresh grant for us. It replaces the stored one only if it
    /// is validly signed by them for us and outlives the old one; anything else is ignored, so a
    /// peer can't shorten or break the grant we call them with.
    pub(crate) fn renew_grant(self: &Arc<Self>, epoch: u64, did: &str, grant: proto::SignedGrant) {
        let Ok(me) = self.me() else { return };
        let replaced = {
            let mut s = self.shared.lock();
            if s.epoch != epoch {
                return;
            }
            match s.state.contacts.iter_mut().find(|c| c.did == did) {
                Some(c) if grant_outlives(&c.grant_from_them, &grant, did, me.id.did(), now()) => {
                    c.grant_from_them = grant;
                    true
                }
                _ => false,
            }
        };
        if replaced {
            self.persist_in_background(epoch);
        }
    }

    /// Never overwrites `Ended`, and the change and its delivery are one step, so the UI sees a
    /// call's states in the order they happened.
    pub(crate) fn set_state(&self, call: &Call, state: CallState) {
        let _order = call.notify.lock();
        {
            let mut cur = call.state.lock();
            if matches!(*cur, CallState::Ended { .. }) {
                return;
            }
            *cur = state.clone();
        }
        if state == CallState::Active {
            call.active_at.lock().get_or_insert_with(Instant::now);
        }
        self.log(format!("call {}: {state:?}", call.info.call_id));
        self.events.on_call_state(call.info.call_id.clone(), state);
    }

    /// Frees the slot, closes the connection and emits Ended — once per call, however many
    /// paths get here. The slot is free before `Ended` is delivered (a UI that reacts to it may
    /// place the next call at once); the next call's first event waits for this one's.
    pub(crate) fn end_call(&self, call: &Arc<Call>, reason: String) {
        if std::mem::replace(&mut *call.ended.lock(), true) {
            return;
        }
        let _order = call.notify.lock();
        self.note_direct(call);
        call.ended_wake.notify_one();
        let state = CallState::Ended { reason: reason.clone() };
        *call.state.lock() = state.clone();
        // A waiting call takes over the slot when the active one ends.
        let mut promoted = None;
        {
            let mut live = self.live.lock();
            if live.call.as_ref().is_some_and(|c| Arc::ptr_eq(c, call)) {
                live.call = None;
                live.last = Some(call.clone());
                if let Some(w) = live.waiting.take() {
                    w.waiting.store(false, Ordering::SeqCst);
                    live.call = Some(w.clone());
                    promoted = Some(w);
                }
            } else if live.waiting.as_ref().is_some_and(|c| Arc::ptr_eq(c, call)) {
                live.waiting = None;
            }
        }
        if let Some(conn) = call.conn.lock().take() {
            conn.close(0u32.into(), b"bye");
        }
        self.record_ended(call, &reason);
        self.log(format!("call {}: {state:?}", call.info.call_id));
        self.events.on_call_state(call.info.call_id.clone(), state);
        if let Some(w) = promoted {
            if w.answer_on_promote.load(Ordering::SeqCst) {
                let _ = w.cmd.send(Cmd::Answer);
            } else {
                // Now an ordinary ringing call: the UI hears of it as a fresh incoming call.
                let _order = w.notify.lock();
                if !matches!(w.state(), CallState::Ended { .. }) {
                    self.events.on_incoming_call(w.info.clone());
                    self.events.on_call_state(w.info.call_id.clone(), CallState::Ringing);
                }
            }
        }
    }

    /// Remembers whether the call is on a direct path while we still have its connection.
    pub(crate) fn note_direct(&self, call: &Call) {
        if let Some((direct, _)) = call.conn.lock().as_ref().and_then(selected_path) {
            *call.direct.lock() = direct;
        }
    }

    /// Writes the call into the history (before `Ended` is delivered, so the UI's re-read sees
    /// it). Not for a call we gave up on in favour of theirs, nor for a contact since removed.
    pub(crate) fn record_ended(&self, call: &Call, reason: &str) {
        if reason == "superseded" {
            return;
        }
        let name = {
            let s = self.shared.lock();
            if s.epoch != call.epoch {
                return;
            }
            match s.state.contacts.iter().find(|c| c.did == call.info.peer_did) {
                Some(c) => display_name(c),
                None => return,
            }
        };
        let answered = call.active_at.lock().is_some();
        let duration_secs = call
            .active_at
            .lock()
            .filter(|_| reason != "answered_elsewhere")
            .map_or(0, |t| t.elapsed().as_secs() as u32);
        self.log_call(call.epoch, CallRecord {
            call_id: call.info.call_id.clone(),
            peer_did: call.info.peer_did.clone(),
            peer_name: name,
            incoming: call.info.incoming,
            started_at: call.started_at,
            duration_secs,
            reason: reason.to_string(),
            direct: *call.direct.lock(),
            missed: call.info.incoming
                && !answered
                && !matches!(reason, "declined_local" | "answered_elsewhere" | "declined_elsewhere"),
        });
    }

    pub(crate) fn command(&self, call_id: &str, cmd: Cmd) -> Result<(), Error> {
        let live = self.live.lock();
        let call = live
            .call
            .iter()
            .chain(live.waiting.iter())
            .find(|c| c.info.call_id == call_id)
            .ok_or(Error::NotFound)?;
        call.cmd.send(cmd).map_err(|_| Error::NotFound)
    }

    pub(crate) fn active_call(&self) -> Option<(Arc<Call>, Option<f32>)> {
        let live = self.live.lock();
        let call = live.call.clone()?;
        (call.state() == CallState::Active).then_some((call, live.tone))
    }

    pub(crate) fn push_mic(&self, pcm: &[i16]) {
        let Some((call, tone)) = self.active_call() else { return };
        let Some(conn) = call.conn.lock().clone() else { return };
        let mut mic = call.mic.lock();
        mic.extend_from_slice(pcm);
        let mut frame = [0i16; audio::FRAME];
        while mic.len() >= audio::FRAME {
            frame.copy_from_slice(&mic[..audio::FRAME]);
            mic.drain(..audio::FRAME);
            if let Some(hz) = tone {
                let mut t = call.tone.lock();
                if t.as_ref().is_none_or(|(f, _)| *f != hz) {
                    *t = Some((hz, audio::Tone::new(hz, 10_000.0)));
                }
                t.as_mut().unwrap().1.next_frame(&mut frame);
            }
            let datagram = match call.sender.lock().encode(&frame) {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("encode: {e}");
                    continue;
                }
            };
            if conn.send_datagram(Bytes::from(datagram)).is_ok() {
                *call.sent.lock() += 1;
            }
        }
    }

    pub(crate) fn pull_speaker(&self) -> Vec<i16> {
        let mut out = [0i16; audio::FRAME];
        if let Some((call, _)) = self.active_call() {
            call.receiver.lock().pull(&mut out, call.now_ms());
            let mut played = call.played.lock();
            played.extend(out.iter().copied());
            let excess = played.len().saturating_sub(audio::RATE as usize);
            played.drain(..excess);
        }
        out.to_vec()
    }

    pub(crate) fn call_stats(&self) -> Option<CallStats> {
        let call = self.live.lock().call.clone()?;
        let rx = call.receiver.lock().stats();
        let (direct, rtt_ms) = call.conn.lock().as_ref().and_then(selected_path).unwrap_or((false, 0));
        if call.state() == CallState::Active {
            *call.direct.lock() = direct;
        }
        let state = call.state();
        let played: Vec<i16> = call.played.lock().iter().copied().collect();
        Some(CallStats {
            call_id: call.info.call_id.clone(),
            reconnecting: state == CallState::Active && call.last_rx.lock().elapsed() > RECONNECT_AFTER,
            state,
            secs: call.started.elapsed().as_secs() as u32,
            direct,
            rtt_ms,
            sent: *call.sent.lock(),
            received: rx.received,
            lost: rx.lost,
            recovered: rx.recovered_fec + rx.recovered_dred,
            concealed: rx.concealed,
            buffered_ms: rx.buffered_ms,
            rx_freq_hz: audio::estimate_frequency(&played).unwrap_or(0.0),
            rx_rms: audio::rms(&played),
        })
    }
}

/// Whether the connection's selected path is direct (not via a relay), and its round trip.
fn selected_path(c: &Connection) -> Option<(bool, u32)> {
    c.paths().iter().find(|p| p.is_selected()).map(|p| (p.is_ip(), p.rtt().as_millis() as u32))
}

/// An unsigned relay hint from a peer, kept only if it is a sane relay URL: it gets stored
/// and dialled, so junk must not get in.
pub(crate) fn relay_hint(hint: &Option<String>) -> Option<String> {
    hint.as_deref()
        .filter(|h| h.len() <= 200 && h.starts_with("https://"))
        .and_then(|h| h.parse::<RelayUrl>().ok())
        .map(|u| u.to_string())
}

/// The nonce of the ticket we currently hand out, if it is still valid.
enum HelloError {
    /// Not acceptable (bad, spent, old or blocked ticket): the joiner is turned away.
    Refused(String),
    /// Acceptable, but we could not make it durable.
    Failed(Error),
}

fn stored_contact(new: &proto::NewContact, relay: Option<String>) -> StoredContact {
    StoredContact {
        did: new.did.clone(),
        name: proto::sanitize_name(&new.name),
        devices: vec![ContactDevice { device: new.device, relay }],
        device_list: None,
        grant_from_them: new.grant_from_them.clone(),
        added_at: now(),
        alias: None,
        verified: false,
    }
}

/// Adds `stored`, or refreshes the contact of that DID. A device we have not seen before resets
/// `verified`: the safety numbers were compared with another phone.
fn apply_contact(st: &mut State, stored: StoredContact, device: [u8; 32], lift_block: bool) -> Result<(), Error> {
    if lift_block {
        st.blocked.remove(&stored.did);
    } else if st.blocked.contains(&stored.did) {
        return Err(Error::Rejected(REFUSED.into()));
    }
    match st.contacts.iter_mut().find(|c| c.did == stored.did) {
        Some(existing) => {
            existing.name = stored.name;
            let relay = stored
                .devices
                .first()
                .and_then(|d| d.relay.clone())
                .or_else(|| existing.relay_for(&device).map(String::from));
            if !existing.has_device(&device) {
                existing.verified = false;
            }
            existing.devices.retain(|d| d.device != device);
            existing.devices.insert(0, ContactDevice { device, relay });
            existing.devices.truncate(MAX_DEVICES);
            existing.grant_from_them = stored.grant_from_them;
        }
        None => st.contacts.push(stored),
    }
    Ok(())
}

/// Replaces the contact's devices by `blob`'s if it verifies for their DID and is newer than the
/// list we hold. Hints are kept for devices we knew; a device we did not know resets `verified`.
/// Returns whether anything changed.
pub(crate) fn merge_device_list(c: &mut StoredContact, blob: &proto::SignedBlob) -> bool {
    let Ok(list) = proto::verify_device_list(blob, &c.did) else { return false };
    if c.device_list.as_ref().is_some_and(|old| !proto::newer(blob, old)) {
        return false;
    }
    let devices: Vec<ContactDevice> = list
        .devices
        .iter()
        .filter_map(|e| {
            let device = e.device_key().ok()?;
            let relay = relay_hint(&e.relay).or_else(|| c.relay_for(&device).map(String::from));
            Some(ContactDevice { device, relay })
        })
        .collect();
    if devices.is_empty() {
        return false;
    }
    if devices.iter().any(|d| !c.has_device(&d.device)) {
        c.verified = false;
    }
    c.devices = devices;
    c.device_list = Some(blob.clone());
    true
}

fn current_nonce(state: &State, now: u64) -> Option<String> {
    let t = ContactTicket::from_text(state.ticket.as_deref()?).ok()?;
    Some(t.verify(now).ok()?.nonce)
}

/// Whether `new` is a good grant from `issuer` to `holder` and expires after `old`.
pub(crate) fn grant_outlives(old: &proto::SignedGrant, new: &proto::SignedGrant, issuer: &str, holder: &str, now: u64) -> bool {
    let none = std::collections::HashSet::new();
    let Ok(new) = proto::verify_grant(new, issuer, holder, now, &none) else { return false };
    // An old grant that no longer verifies (expired) is beaten by any valid new one.
    let old_exp = proto::verify_grant(old, issuer, holder, now, &none).map_or(0, |g| g.exp);
    new.exp > old_exp
}

/// A reason string from the peer, for the UI: no control characters, bounded length.
pub(crate) fn clip_reason(reason: &str) -> String {
    reason.chars().filter(|c| !c.is_control()).take(MAX_REASON_CHARS).collect()
}

pub(crate) fn relay_of(ep: &Endpoint) -> Option<String> {
    ep.addr().relay_urls().next().map(|u| u.to_string())
}

pub(crate) fn addr_for(device: &[u8; 32], relay: Option<&str>) -> Result<EndpointAddr, Error> {
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

/// Every thread of the core runtime carries this name (workers and blocking pool alike).
const CORE_THREAD: &str = "p2pcore";

fn on_core_thread() -> bool {
    std::thread::current().name() == Some(CORE_THREAD)
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

pub(crate) fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub(crate) fn random_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_800_000_000;

    #[test]
    fn renewed_grant_must_be_valid_and_later() {
        let (them, _) = identity::generate();
        let (me, _) = identity::generate();
        let (other, _) = identity::generate();
        let old = proto::issue_grant(&them, me.did(), T0, 1000);
        let newer = proto::issue_grant(&them, me.did(), T0 + 500, 1000);
        let shorter = proto::issue_grant(&them, me.did(), T0, 500);
        let ok = |new: &proto::SignedGrant| grant_outlives(&old, new, them.did(), me.did(), T0 + 10);
        assert!(ok(&newer));
        assert!(!ok(&shorter), "must not shorten");
        assert!(!ok(&old), "same expiry is not an extension");
        // Wrong issuer, wrong holder.
        assert!(!ok(&proto::issue_grant(&other, me.did(), T0 + 500, 1000)));
        assert!(!ok(&proto::issue_grant(&them, other.did(), T0 + 500, 1000)));
        // Bad signature: a longer-lived payload under another grant's signature.
        let long = proto::issue_grant(&them, me.did(), T0 + 500, 100_000);
        let forged = proto::SignedGrant { payload: long.payload.clone(), signature: newer.signature.clone() };
        assert!(!ok(&forged));
        // An expired stored grant is beaten by any valid one.
        assert!(grant_outlives(&old, &newer, them.did(), me.did(), T0 + 1200));
    }

    #[test]
    fn safety_numbers_match_on_both_sides_and_differ_between_pairs() {
        let (a, _) = identity::generate();
        let (b, _) = identity::generate();
        let (c, _) = identity::generate();
        let ab = safety_number(a.did(), b.did()).unwrap();
        assert_eq!(ab, safety_number(b.did(), a.did()).unwrap());
        assert_ne!(ab, safety_number(a.did(), c.did()).unwrap());
        assert_ne!(ab, safety_number(b.did(), c.did()).unwrap());
        let groups: Vec<&str> = ab.split(' ').collect();
        assert_eq!(groups.len(), 12);
        assert!(groups.iter().all(|g| g.len() == 5 && g.bytes().all(|b| b.is_ascii_digit())));
        assert!(safety_number(a.did(), "did:key:zNope").is_err());
    }

    #[test]
    fn only_the_current_ticket_has_a_nonce() {
        let (id, _) = identity::generate();
        let dev = proto::device_public(&proto::new_device_secret());
        let mut st = State::default();
        assert_eq!(current_nonce(&st, T0), None);
        let t = proto::issue_contact_ticket(&id, dev, "x", None, T0, 600);
        st.ticket = Some(t.to_text());
        let n = current_nonce(&st, T0 + 1).unwrap();
        assert_eq!(n, t.verify(T0 + 1).unwrap().nonce);
        assert_eq!(current_nonce(&st, T0 + 601), None, "expired");
        st.ticket = None;
        assert_eq!(current_nonce(&st, T0 + 1), None);
    }

    #[test]
    fn reasons_and_relays_are_bounded() {
        assert_eq!(clip_reason("a\u{0}b\n").as_str(), "ab");
        assert_eq!(clip_reason(&"é".repeat(500)).chars().count(), MAX_REASON_CHARS);
        assert!(relay_hint(&Some("http://relay.example/".into())).is_none());
        assert!(relay_hint(&Some(format!("https://{}.example/", "a".repeat(300)))).is_none());
        assert!(relay_hint(&Some("https://use1-1.relay.n0.iroh.link./".into())).is_some());
        assert!(relay_hint(&None).is_none());
    }
}

/// The first word of `text` that looks like a contact card (`OSVC2:` or `osvc1.`), so a card
/// pasted with a greeting around it still works.
fn find_card(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '(' | ')'))
        .map(|w| w.trim_end_matches(['.', ',', ';', '!', '?']))
        .find(|w| {
            w.get(..6).is_some_and(|h| h.eq_ignore_ascii_case("OSVC2:") || h.eq_ignore_ascii_case("osvc1."))
                && w.len() > 20
        })
        .map(str::to_string)
}


#[cfg(test)]
mod find_card_tests {
    use super::find_card;

    #[test]
    fn finds_a_card_in_surrounding_text() {
        let card = "OSVC2:MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43U";
        assert_eq!(find_card(card).as_deref(), Some(card));
        assert_eq!(find_card(&format!("Add me on Tinline:\n{card}.")).as_deref(), Some(card));
        assert_eq!(find_card(&format!("\"{card}\" thanks")).as_deref(), Some(card));
        assert_eq!(find_card(&format!("osvc2:{}", &card[6..])).as_deref(), Some(&*format!("osvc2:{}", &card[6..])));
        assert_eq!(find_card("hello there"), None);
        assert_eq!(find_card("OSVC2:short"), None);
    }
}
