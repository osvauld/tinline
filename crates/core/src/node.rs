//! The device's one iroh endpoint and everything that rides on it: identity, contacts, and at
//! most one call at a time.
//!
//! Blocking methods are meant for the app's IO threads; long work (dialing, ringing, the call
//! itself) runs on this node's own tokio runtime and reports through `NodeEvents`.

use std::collections::VecDeque;
use std::sync::Arc;
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

use crate::store::{CallRecord, Disk, History, Profile, ProfileV2, State, Store, StoredContact};
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
/// Devices remembered per contact, newest first.
const MAX_DEVICES: usize = 4;
/// An active call that receives no media for this long is over.
const NO_AUDIO_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a turned-away connection gets to read our refusal before we let go of its slot.
const LINGER: Duration = Duration::from_secs(2);
/// Longest peer-supplied reason we pass on to the UI.
const MAX_REASON_CHARS: usize = 100;
const RING_TIMEOUT: Duration = Duration::from_secs(60);
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

pub(crate) struct Me {
    pub(crate) profile: Profile,
    pub(crate) id: Identity,
    pub(crate) device: [u8; 32],
    pub(crate) attestation: proto::SignedAttestation,
}

pub(crate) struct Shared {
    /// What `profile.json` holds; present whenever an identity exists, locked or not.
    pub(crate) disk: Option<Disk>,
    /// Present while unlocked (and for a legacy profile).
    pub(crate) me: Option<Arc<Me>>,
    /// The vault's data key while unlocked via a vault; never written to disk by the core.
    pub(crate) dek: Option<Dek>,
    pub(crate) state: State,
    pub(crate) endpoint: Option<Endpoint>,
}

/// The call slot, apart from `Shared` so the 50 Hz audio threads never wait behind a disk
/// write or a handshake holding `Shared`.
#[derive(Default)]
pub(crate) struct Live {
    call: Option<Arc<Call>>,
    /// The call that held the slot before, until the next call has waited for its events to
    /// be delivered (so `Ended` always precedes the next call's `Dialing`).
    last: Option<Arc<Call>>,
    tone: Option<f32>,
}

pub(crate) struct Inner {
    pub(crate) handle: tokio::runtime::Handle,
    pub(crate) store: Store,
    /// Held while snapshotting and writing state, so writes land in the order taken. Always
    /// taken before `shared`, never while holding it.
    pub(crate) writing: Mutex<()>,
    pub(crate) events: Arc<dyn NodeEvents>,
    pub(crate) shared: Mutex<Shared>,
    pub(crate) live: Mutex<Live>,
    pub(crate) history: Arc<History>,
    pub(crate) pending: Arc<Semaphore>,
    pub(crate) reserved: Arc<Semaphore>,
    /// Serialises start, stop, lock and set_passphrase.
    pub(crate) lifecycle: Arc<tokio::sync::Mutex<()>>,
    pub(crate) chat: Mutex<Option<Arc<crate::chat::engine::ChatCore>>>,
    pub(crate) chat_events: Mutex<Option<Arc<dyn crate::chat::api::ChatEvents>>>,
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
        let store = Store::open(data_dir)?;
        let state = store.state()?;
        let history = Arc::new(History::new(store.calls()?));
        let disk = store.profile()?;
        // A legacy profile keeps working (calls keep ringing) until it is converted.
        let me = match &disk {
            Some(Disk::Legacy(p)) => Some(Arc::new(Me::load(p.clone())?)),
            _ => None,
        };
        let inner = Arc::new(Inner {
            handle: rt.handle().clone(),
            store,
            writing: Mutex::new(()),
            events,
            shared: Mutex::new(Shared { disk, me, dek: None, state, endpoint: None }),
            live: Mutex::new(Live::default()),
            history,
            pending: Arc::new(Semaphore::new(MAX_PENDING_HELLOS)),
            reserved: Arc::new(Semaphore::new(RESERVED_HELLOS)),
            lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            chat: Mutex::new(None),
            chat_events: Mutex::new(None),
        });
        Ok(Arc::new(Self { inner, rt: Some(rt) }))
    }

    pub fn has_identity(&self) -> bool {
        self.inner.shared.lock().disk.is_some()
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

    /// Creates an identity sealed under `passphrase` (at least 8 characters, else
    /// `WeakPassphrase`) and leaves the node unlocked. Returns the recovery phrase; showing it
    /// again later needs the passphrase (`recovery_phrase`). Slow: runs Argon2id.
    pub fn create_identity(&self, name: String, passphrase: String) -> Result<String, Error> {
        vault::check_passphrase(&passphrase)?;
        let (_, mnemonic) = identity::generate();
        let phrase = mnemonic.to_string();
        self.set_identity(phrase.clone(), name, passphrase)?;
        Ok(phrase)
    }

    pub fn restore_identity(&self, phrase: String, name: String, passphrase: String) -> Result<(), Error> {
        vault::check_passphrase(&passphrase)?;
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
    /// at once if already unlocked (or a legacy profile). Slow: runs Argon2id.
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

    /// Sets or changes the passphrase (at least 8 characters). With `old = None` it converts a
    /// legacy profile (state `NeedsPassphrase`) and the clear-text secrets leave profile.json;
    /// otherwise `old` is required and verified. The data key is kept, so a remembered
    /// `unlock_key` stays valid. Slow: runs Argon2id once or twice.
    pub fn set_passphrase(&self, old: Option<String>, new: String) -> Result<(), Error> {
        vault::check_passphrase(&new)?;
        let _life = self.lifecycle()?;
        let snapshot = self.inner.shared.lock().disk.clone();
        match snapshot.ok_or(Error::NoIdentity)? {
            Disk::Legacy(p) => {
                if old.is_some() {
                    return Err(Error::Protocol("this identity has no passphrase yet".into()));
                }
                let id = identity::recover(&p.mnemonic).map_err(|_| Error::BadPhrase)?;
                let secrets = Secrets { mnemonic: p.mnemonic.clone(), device_secret: p.device_secret };
                let device_public = proto::device_public(&p.device_secret);
                let (vault, dek) = self.kdf(move || vault::seal(&secrets, &new))??;
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
                self.inner.store.save_profile(&disk)?;
                self.inner.store.remove_legacy_leftovers();
                s.disk = Some(disk);
                s.dek = Some(dek);
                drop(s);
                self.inner.chat_open();
                Ok(())
            }
            Disk::V2(p) => {
                let old = old.ok_or_else(|| Error::Protocol("the old passphrase is required".into()))?;
                let vault = p.vault;
                let rewrapped = self.kdf(move || {
                    let (_secrets, dek) = vault::open(&vault, &old)?;
                    vault::rewrap(&vault, &dek, &new)
                })??;
                let mut s = self.inner.shared.lock();
                // Only the vault changes; keep any name change made meanwhile.
                let Some(Disk::V2(cur)) = s.disk.clone() else { return Err(Error::NoIdentity) };
                let disk = Disk::V2(ProfileV2 { vault: rewrapped, ..cur });
                self.inner.store.save_profile(&disk)?;
                s.disk = Some(disk);
                Ok(())
            }
        }
    }

    /// The recovery phrase, re-derived from the vault: `WrongPassphrase` unless `passphrase`
    /// is right, and `Locked` on a legacy profile (convert it with `set_passphrase` first).
    /// Works while locked. Slow: runs Argon2id.
    pub fn recovery_phrase(&self, passphrase: String) -> Result<String, Error> {
        let vault = {
            let s = self.inner.shared.lock();
            match &s.disk {
                None => return Err(Error::NoIdentity),
                Some(Disk::Legacy(_)) => return Err(Error::Locked),
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
        let inner = self.inner.clone();
        self.run_lifecycle(async move {
            {
                // Under the lifecycle lock, so a `start` that is binding right now finishes
                // first and its endpoint is closed here.
                let _life = inner.lifecycle.lock().await;
                inner.close_endpoint().await;
                {
                    let mut s = inner.shared.lock();
                    s.me = None;
                    s.dek = None;
                }
                inner.chat_close().await;
            }
            inner.emit_status();
        });
    }

    pub fn set_name(&self, name: String) -> Result<(), Error> {
        {
            let mut s = self.inner.shared.lock();
            let mut disk = s.disk.clone().ok_or(Error::NoIdentity)?;
            match &mut disk {
                Disk::V2(p) => p.name = name.clone(),
                Disk::Legacy(p) => p.name = name.clone(),
            }
            self.inner.store.save_profile(&disk)?;
            s.disk = Some(disk);
            if let Some(me) = s.me.clone() {
                let mut profile = me.profile.clone();
                profile.name = name;
                s.me = Some(Arc::new(Me::load(profile)?));
            }
            // The old ticket carries the old name.
            s.state.ticket = None;
        }
        self.inner.persist()
    }

    /// Drops the ticket we hand out: the old QR/text stops working at once and the next
    /// `my_ticket` mints a fresh one. For "the ticket may have leaked".
    pub fn reset_ticket(&self) -> Result<(), Error> {
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
        self.inner.history.list(None, limit as usize)
    }

    /// Finished calls with one person, newest first.
    pub fn calls_with(&self, did: String, limit: u32) -> Vec<CallRecord> {
        self.inner.history.list(Some(&did), limit as usize)
    }

    /// Turns calls on or off. Off, incoming calls are not shown and the caller just fails to
    /// get through; they are logged with reason `unavailable`. `until` (unix seconds) switches
    /// it back on by itself. Adding contacts still works.
    pub fn set_available(&self, available: bool, until: Option<u64>) -> Result<(), Error> {
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
        {
            let mut s = self.inner.shared.lock();
            s.state.contacts.retain(|c| c.did != did);
            s.state.blocked.insert(did.clone());
            // A ticket they may have seen is of no further use.
            s.state.ticket = None;
        }
        self.inner.history.remove_peer(&did);
        self.inner.chat_purge(&did);
        // The removal holds in memory whether or not it reached the disk, so act on it either
        // way and report the failed write afterwards. (A call ending now is not logged: the
        // peer is no longer a contact.)
        let saved = self.inner.persist().and(self.inner.history.save(&self.inner.store));
        let call = self.inner.live.lock().call.clone();
        if let Some(call) = call.filter(|c| c.info.peer_did == did) {
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
    pub(crate) fn load(profile: Profile) -> Result<Self, Error> {
        let id = identity::recover(&profile.mnemonic).map_err(|_| Error::BadPhrase)?;
        let device = proto::device_public(&profile.device_secret);
        let attestation = proto::attest(&id, device, now());
        Ok(Self { profile, id, device, attestation })
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
                .and_then(|d| PublicKey::from_bytes(d).ok())
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
    fn kdf<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> Result<T, Error> {
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
        let mut s = self.inner.shared.lock();
        let Some(Disk::V2(p)) = &s.disk else { return Err(Error::NoIdentity) };
        if s.me.is_some() {
            return Ok(());
        }
        let me = Me::load(Profile {
            mnemonic: secrets.mnemonic.clone(),
            name: p.name.clone(),
            device_secret: secrets.device_secret,
        })?;
        if me.id.did() != p.did {
            return Err(Error::Io("vault does not match profile".into()));
        }
        s.me = Some(Arc::new(me));
        s.dek = Some(dek);
        drop(s);
        self.inner.chat_open();
        Ok(())
    }

    fn set_identity(&self, phrase: String, name: String, passphrase: String) -> Result<(), Error> {
        if self.inner.shared.lock().disk.is_some() {
            return Err(Error::HaveIdentity);
        }
        let profile = Profile { mnemonic: phrase, name, device_secret: proto::new_device_secret() };
        let me = Me::load(profile)?;
        let secrets = Secrets { mnemonic: me.profile.mnemonic.clone(), device_secret: me.profile.device_secret };
        let (vault, dek) = self.kdf(move || vault::seal(&secrets, &passphrase))??;
        let disk = Disk::V2(ProfileV2 {
            version: 2,
            name: me.profile.name.clone(),
            did: me.id.did().to_string(),
            device_public: me.device,
            vault,
        });
        let mut s = self.inner.shared.lock();
        if s.disk.is_some() {
            return Err(Error::HaveIdentity);
        }
        self.inner.store.save_profile(&disk)?;
        s.disk = Some(disk);
        s.me = Some(Arc::new(me));
        s.dek = Some(dek);
        drop(s);
        self.inner.chat_open();
        Ok(())
    }
}

impl Inner {
    /// Writes a snapshot of `State`. Callers must not hold `shared`.
    pub(crate) fn persist(&self) -> Result<(), Error> {
        let _w = self.writing.lock();
        let snapshot = self.shared.lock().state.clone();
        self.store.save_state(&snapshot)
    }

    /// `persist` for async code: the write (and its fsyncs) runs on the blocking pool, not on a
    /// worker that other connections need.
    pub(crate) async fn persist_async(self: &Arc<Self>) -> Result<(), Error> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.persist())
            .await
            .map_err(|_| Error::Io("persist task failed".into()))?
    }

    /// Fire and forget, for state that is only a cache (a device, a renewed grant).
    pub(crate) fn persist_in_background(self: &Arc<Self>) {
        let this = self.clone();
        self.handle.spawn_blocking(move || {
            let _ = this.persist();
        });
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

    /// Adds to the call log and writes it in the background.
    pub(crate) fn log_call(&self, rec: CallRecord) {
        self.history.push(rec);
        let (history, store) = (self.history.clone(), self.store.clone());
        self.handle.spawn_blocking(move || {
            if let Err(e) = history.save(&store) {
                tracing::warn!("saving call history: {e}");
            }
        });
    }

    /// An incoming call that never became a `Call` (turned away unavailable, or busy).
    pub(crate) fn log_refused(&self, call_id: &str, did: &str, name: &str, reason: &str, missed: bool) {
        self.log_call(CallRecord {
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
            .alpns(vec![proto::ALPN.to_vec(), crate::chat::wire::CHAT_ALPN.to_vec(), iroh_blobs::ALPN.to_vec()])
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
        self.persist_async().await?;
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
        let contact = self.save_contact(new, relay, true).await?;
        self.log(format!("contact {} added", contact.name));
        Ok(contact)
    }

    /// `lift_block`: only when the user scanned this contact's ticket; a contact who arrives on
    /// their own (they scanned ours) must not undo a removal.
    pub(crate) async fn save_contact(
        self: &Arc<Self>,
        new: proto::NewContact,
        relay: Option<String>,
        lift_block: bool,
    ) -> Result<Contact, Error> {
        let stored = StoredContact {
            did: new.did.clone(),
            name: proto::sanitize_name(&new.name),
            devices: vec![new.device],
            relay,
            grant_from_them: new.grant_from_them,
            added_at: now(),
            alias: None,
            verified: false,
        };
        let contact = Contact::from(&stored);
        {
            let mut s = self.shared.lock();
            if lift_block {
                s.state.blocked.remove(&new.did);
            } else if s.state.blocked.contains(&new.did) {
                return Err(Error::Rejected(REFUSED.into()));
            }
            match s.state.contacts.iter_mut().find(|c| c.did == new.did) {
                Some(existing) => {
                    existing.name = stored.name;
                    // A different phone than the one we compared numbers with.
                    if !existing.devices.contains(&new.device) {
                        existing.verified = false;
                    }
                    existing.devices.retain(|d| *d != new.device);
                    existing.devices.insert(0, new.device);
                    existing.devices.truncate(MAX_DEVICES);
                    if stored.relay.is_some() {
                        existing.relay = stored.relay;
                    }
                    existing.grant_from_them = stored.grant_from_them;
                }
                None => s.state.contacts.push(stored),
            }
        }
        self.persist_async().await?;
        self.events.on_contacts_changed();
        Ok(contact)
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
                // Check and spend the nonce under one lock, so two hellos racing on the same
                // ticket can't both get in.
                let accepted = {
                    let mut s = self.shared.lock();
                    let now = now();
                    let current = current_nonce(&s.state, now);
                    let r = proto::accept_contact_hello(
                        &me.id,
                        me.device,
                        &hello,
                        remote,
                        now,
                        &s.state.redeemed,
                        GRANT_TTL,
                    )
                    .map_err(|e| e.to_string())
                    .and_then(|(welcome, new)| {
                        // Only the ticket we hand out right now can be redeemed: an older one
                        // that leaked (or was reset) is dead even though its nonce is unspent.
                        if current.as_deref() != Some(new.redeemed_nonce.as_str()) {
                            Err("not the current ticket".to_string())
                        } else if s.state.blocked.contains(&new.did) {
                            Err("blocked".to_string())
                        } else {
                            Ok((welcome, new))
                        }
                    });
                    if let Ok((_, new)) = &r {
                        s.state.redeemed.insert(new.redeemed_nonce.clone());
                        // Spent: the next `my_ticket` mints a fresh one.
                        s.state.ticket = None;
                    }
                    r
                };
                match accepted {
                    Ok((welcome, new)) => {
                        if let Err(e) = self.persist_async().await {
                            // The nonce is spent in memory but not on disk: say no rather than
                            // add a contact that a restart would forget.
                            let _ = ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await;
                            ctrl.finish();
                            return Err(e);
                        }
                        ctrl.send(&welcome).await?;
                        ctrl.finish();
                        let contact = self.save_contact(new, hint, false).await?;
                        drop(permit);
                        self.log(format!("contact {} added us", contact.name));
                        // Let the joiner read the welcome and close first.
                        let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
                    }
                    Err(e) => {
                        self.log(format!("rejected contact hello: {e}"));
                        ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
                        ctrl.finish();
                        let _ = tokio::time::timeout(LINGER, conn.closed()).await;
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
        self.shared.lock().state.contacts.iter().any(|c| c.devices.contains(device))
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
        let Msg::CallHello { call_id, relay: hint, .. } = &hello else { unreachable!() };
        let (call_id, hint) = (call_id.clone(), relay_hint(hint));
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
            self.log_refused(&call_id, &caller.did, &name, "unavailable", false);
            ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
            ctrl.finish();
            let _ = tokio::time::timeout(LINGER, conn.closed()).await;
            return Ok(());
        }
        let info = CallInfo { call_id, peer_did: caller.did.clone(), peer_name: name, incoming: true };
        self.yield_on_glare(&caller.did, me.device, remote);
        let (call, cmds) = match self.begin_call(info.clone(), CallState::Ringing, false) {
            Ok(c) => c,
            Err(_) => {
                self.log_refused(&info.call_id, &info.peer_did, &info.peer_name, "busy", true);
                ctrl.send(&Msg::Busy).await?;
                ctrl.finish();
                let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                return Ok(());
            }
        };
        // From here every exit must free the slot; the guard does it if nothing else did.
        let _slot = SlotGuard { inner: &self, call: call.clone() };
        *call.conn.lock() = Some(conn.clone());
        // A new device for a known contact: dial it next time.
        self.note_device(&caller.did, remote, hint);
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
        self.clone().run_call(call, conn, ctrl, cmds, Some(caller.did)).await;
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
    /// there.
    pub(crate) fn note_device(self: &Arc<Self>, did: &str, device: [u8; 32], relay: Option<String>) {
        let mut s = self.shared.lock();
        let Some(c) = s.state.contacts.iter_mut().find(|c| c.did == did) else { return };
        let mut changed = false;
        if c.devices.first() != Some(&device) {
            c.devices.retain(|d| *d != device);
            c.devices.insert(0, device);
            c.devices.truncate(MAX_DEVICES);
            changed = true;
        }
        if relay.is_some() && c.relay != relay {
            c.relay = relay;
            changed = true;
        }
        drop(s);
        if changed {
            self.persist_in_background();
        }
    }

    /// Takes the call slot. With `announce` the first state is delivered to the UI before any
    /// other thread can see (and end) the call, and after the previous call's events.
    pub(crate) fn begin_call(
        &self,
        info: CallInfo,
        state: CallState,
        announce: bool,
    ) -> Result<(Arc<Call>, mpsc::UnboundedReceiver<Cmd>), Error> {
        // Everything slow or fallible comes before the slot is taken.
        let sender = audio::Sender::new(48_000)?;
        let receiver = audio::Receiver::new()?;
        let (tx, rx) = mpsc::unbounded_channel();
        let call = Arc::new(Call {
            info,
            conn: Mutex::new(None),
            state: Mutex::new(state.clone()),
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
        });
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
        let (call, mut cmds) = self.begin_call(info.clone(), CallState::Dialing, true)?;
        let this = self.clone();
        self.handle.spawn(async move {
            // A panic below must not leave the slot busy forever.
            let _slot = SlotGuard { inner: &this, call: call.clone() };
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
                d = async {
                    tokio::time::timeout(DIAL_TOTAL, this.dial(&ep, &me, &contact, &call.info.call_id))
                        .await
                        .unwrap_or(Err(Error::Timeout))
                } => Some(d),
                _ = cancelled => None,
            };
            match dialed {
                // Ended while dialing (glare): drop the fresh connection, don't start a call.
                Some(Ok((conn, _))) if *call.ended.lock() => conn.close(0u32.into(), b"bye"),
                Some(Ok((conn, ctrl))) => {
                    *call.conn.lock() = Some(conn.clone());
                    this.clone().run_call(call, conn, ctrl, cmds, None).await;
                }
                Some(Err(e)) => {
                    this.log(format!("could not reach {}: {e}", contact.name));
                    this.end_call(&call, "unreachable".into());
                }
                None => this.end_call(&call, "cancelled".into()),
            }
        });
        Ok(info)
    }

    pub(crate) async fn dial(
        &self,
        ep: &Endpoint,
        me: &Me,
        contact: &StoredContact,
        call_id: &str,
    ) -> Result<(Connection, Ctrl), Error> {
        let mut last = Error::NotFound;
        for device in &contact.devices {
            let addr = match addr_for(device, contact.relay.as_deref()) {
                Ok(a) => a,
                Err(e) => {
                    last = e;
                    continue;
                }
            };
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
    pub(crate) async fn run_call(
        self: Arc<Self>,
        call: Arc<Call>,
        conn: Connection,
        mut ctrl: Ctrl,
        mut cmds: mpsc::UnboundedReceiver<Cmd>,
        incoming_from: Option<String>,
    ) {
        let ring_deadline = tokio::time::Instant::now() + RING_TIMEOUT;
        let mut datagrams: Option<oneshot::Sender<()>> = None;
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
                    Ok(Some(Msg::Accept { renewed_grant })) if incoming_from.is_none() && !active => {
                        if let Some(g) = renewed_grant {
                            self.renew_grant(&call.info.peer_did, g);
                        }
                        datagrams = Some(self.start_media(&call, &conn));
                        self.set_state(&call, CallState::Active);
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
                        break if active { "hangup_remote" } else if incoming_from.is_some() { "cancelled" } else { "declined" }.into();
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
                        if let Err(e) = ctrl.send(&Msg::Accept { renewed_grant: renewed }).await {
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
                        let _ = ctrl.send(&Msg::Decline { reason: "declined".into() }).await;
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
    pub(crate) fn renew_grant(self: &Arc<Self>, did: &str, grant: proto::SignedGrant) {
        let Ok(me) = self.me() else { return };
        let replaced = {
            let mut s = self.shared.lock();
            match s.state.contacts.iter_mut().find(|c| c.did == did) {
                Some(c) if grant_outlives(&c.grant_from_them, &grant, did, me.id.did(), now()) => {
                    c.grant_from_them = grant;
                    true
                }
                _ => false,
            }
        };
        if replaced {
            self.persist_in_background();
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
        let state = CallState::Ended { reason: reason.clone() };
        *call.state.lock() = state.clone();
        {
            let mut live = self.live.lock();
            if live.call.as_ref().is_some_and(|c| Arc::ptr_eq(c, call)) {
                live.call = None;
                live.last = Some(call.clone());
            }
        }
        if let Some(conn) = call.conn.lock().take() {
            conn.close(0u32.into(), b"bye");
        }
        self.record_ended(call, &reason);
        self.log(format!("call {}: {state:?}", call.info.call_id));
        self.events.on_call_state(call.info.call_id.clone(), state);
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
            match s.state.contacts.iter().find(|c| c.did == call.info.peer_did) {
                Some(c) => display_name(c),
                None => return,
            }
        };
        let answered = call.active_at.lock().is_some();
        let duration_secs = call.active_at.lock().map_or(0, |t| t.elapsed().as_secs() as u32);
        self.log_call(CallRecord {
            call_id: call.info.call_id.clone(),
            peer_did: call.info.peer_did.clone(),
            peer_name: name,
            incoming: call.info.incoming,
            started_at: call.started_at,
            duration_secs,
            reason: reason.to_string(),
            direct: *call.direct.lock(),
            missed: call.info.incoming && !answered && reason != "declined_local",
        });
    }

    pub(crate) fn command(&self, call_id: &str, cmd: Cmd) -> Result<(), Error> {
        let live = self.live.lock();
        let call = live.call.as_ref().filter(|c| c.info.call_id == call_id).ok_or(Error::NotFound)?;
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
            buffered_ms: rx.buffered_ms as u32,
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
fn current_nonce(state: &State, now: u64) -> Option<String> {
    let t = ContactTicket::from_text(state.ticket.as_deref()?).ok()?;
    Some(t.verify(now).ok()?.nonce)
}

/// Whether `new` is a good grant from `issuer` to `holder` and expires after `old`.
fn grant_outlives(old: &proto::SignedGrant, new: &proto::SignedGrant, issuer: &str, holder: &str, now: u64) -> bool {
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
