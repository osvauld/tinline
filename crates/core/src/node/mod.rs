//! The device's one iroh endpoint and everything that rides on it: identity, contacts, and at
//! most one call at a time.
//!
//! Blocking methods are meant for the app's IO threads; long work (dialing, ringing, the call
//! itself) runs on this node's own tokio runtime and reports through `NodeEvents`.

use std::collections::{HashSet, VecDeque};
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
pub(crate) const GRANT_TTL: u64 = 365 * 24 * 3600;
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
/// See `close_wait`.
const CLOSE_WAIT: Duration = Duration::from_secs(2);
/// Longest `start` waits for a previous endpoint to finish closing (iroh's own bound is ~3 s).
const CLOSING_MAX: Duration = Duration::from_secs(10);
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
pub(crate) const REFUSED: &str = "not accepted";

mod api_call;
mod api_contacts;
mod api_identity;
mod accept;
mod call;
mod contacts;
mod devices;
mod lifecycle;
mod outgoing;
mod persist;
mod run;
mod testkit;
mod types;
mod unlock;
mod util;
#[cfg(test)]
mod tests;

pub use self::types::*;
pub(crate) use self::api_contacts::*;
use self::contacts::*;
pub(crate) use self::call::*;
pub(crate) use self::devices::*;
pub(crate) use self::lifecycle::*;
pub(crate) use self::persist::*;
pub(crate) use self::util::*;

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
    /// The call log of own-device sync, beside the account doc; open while unlocked.
    pub(crate) calls: Option<Arc<crate::sync::callsdoc::CallsDoc>>,
    pub(crate) profile: Option<Arc<crate::sync::profiledoc::ProfileDoc>>,
    pub(crate) profile_cache: Option<Arc<crate::sync::profilecache::ProfileCache>>,
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
    pub(crate) call: Option<Arc<Call>>,
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
    /// Held while an endpoint of ours is closing (`close_fully`); `start` binds the next one only
    /// once it is free, so two endpoints never hold this device key at once.
    pub(crate) closing: Arc<tokio::sync::Mutex<()>>,
    pub(crate) chat: Mutex<Option<Arc<crate::chat::engine::ChatCore>>>,
    pub(crate) chat_events: Mutex<Option<Arc<dyn crate::chat::api::ChatEvents>>>,
    pub(crate) link_events: Mutex<Option<Arc<dyn crate::sync::LinkEvents>>>,
    pub(crate) selfsync: crate::sync::engine::SelfSync,
    pub(crate) sync_tx: tokio::sync::watch::Sender<u64>,
    pub(crate) link: crate::sync::link::LinkState,
    /// Group calls (G6), `crate::gcall::net`.
    pub(crate) gcall: crate::gcall::net::Service,
    pub(crate) block: crate::block::BlockRt,
    /// G1: group chat runtime (second endpoint, sessions with hosts); see `group/engine.rs`.
    pub(crate) group: crate::group::engine::GroupRt,
    /// Token delivery (P6, `caps.rs`).
    pub(crate) caps: crate::caps::CapsRt,
    pub(crate) devlist: crate::devlist::DevlistRt,
    pub(crate) profile_events: Mutex<Option<Arc<dyn crate::profile::ProfileEvents>>>,
    pub(crate) profile_downloads: Mutex<HashSet<(u64, String, String)>>,
    pub(crate) profile_fetches: Arc<Semaphore>,
    pub(crate) video: crate::video_call::NodeVideo,
    /// The selected account's search index (`search.redb`), open while its chat store is.
    pub(crate) search: Mutex<Option<Arc<crate::search::index::SearchIndex>>>,
    /// For tests: the next `n` search hook writes are skipped (a crash between commit and index).
    pub(crate) search_skip: AtomicU32,
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
            let can_block = !on_core_thread() && tokio::runtime::Handle::try_current().is_err();
            if can_block {
                let inner = self.inner.clone();
                rt.block_on(async move {
                    // Best effort: a clean close tells peers at once instead of at idle timeout.
                    if let Some(ep) = ep {
                        let _ = tokio::time::timeout(Duration::from_secs(1), ep.close()).await;
                    }
                    let _ = tokio::time::timeout(Duration::from_secs(3), inner.group_close()).await;
                    let _ = tokio::time::timeout(Duration::from_secs(3), inner.chat_close()).await;
                });
            }
            // GL: every store file goes now, whatever task still holds a clone (the tasks are
            // only dropped below, in the background), so a node made next on this directory
            // opens them at once.
            self.inner.group_close_now();
            self.inner.chat_close_now();
            self.inner.shared.lock().forget_session();
            // Never blocks, so it is safe even if the last reference drops on a core thread.
            rt.shutdown_background();
        }
    }
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
        self.inner.handle.spawn(USER_CALL.scope((), async move {
            let _ = tx.send(fut.await);
        }));
        rx.recv().map_err(|_| Error::Protocol("core runtime stopped".into()))
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
}
