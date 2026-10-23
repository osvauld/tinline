//! The node's base runtime: what every engine of the Tinline core stands on, without UniFFI.
//!
//! [`NodeCtx`] holds the account session (`Shared`, `Me`, `Acct`), the device's one private iroh
//! endpoint with its lifecycle, the 1:1 call slot (`Live`, `Call`), the contacts cluster (blocking,
//! call tokens, device lists, contact requests), private endpoint rotation, the plain event sinks
//! ([`NodeSink`], [`ContactRequestSink`]) and the [`OwnDocSignal`] that tells own-device sync
//! something changed. The engines above it (chat, group, own-device sync, group calls, search,
//! profile pictures) live in p2pcore for now; they reach this crate through `NodeCtx` (p2pcore's
//! `Inner` derefs to it) and it reaches them only through [`NodeHooks`], which p2pcore implements
//! on its `Inner` and installs with [`NodeCtx::install_hooks`].
//!
//! # Lock order
//!
//! Every lock below is held for a short, synchronous stretch unless said otherwise; none is held
//! across an `.await` except the two tokio mutexes.
//!
//! - `writing` is taken before `shared` and never while holding it (`persist*`, `accept_hello`
//!   and the other state writers snapshot under `shared`, write under `writing`).
//! - `gate` serialises creating, committing and unlocking an account; `shared` is taken inside it,
//!   never the other way round.
//! - `lifecycle` (tokio) serialises start, stop, lock, set_passphrase, account switches and
//!   endpoint rotation; it is held across the awaits of those operations, hook calls included.
//!   Nothing that runs under it may wait for it again.
//! - `closing` (tokio) is held while an endpoint of ours closes (`close_fully`); `start` binds the
//!   next endpoint only once it is free.
//! - `live` (the 1:1 call slot) before the group-call session: [`NodeCtx::with_no_call`] runs its
//!   closure with `live` held, and the group-call service takes its slot inside that closure
//!   (`gcall_net` crate docs). Nothing may take `live` while holding a group-call session lock.
//! - The group engine's `core` lock is held while it opens its store, under the epoch read of
//!   `shared` (so `shared` is never held while a hook into the group engine runs).
//!
//! **Hooks run with no `NodeCtx` data lock held** (`gate`, `writing`, `shared`, `live`, the
//! contacts-cluster mutexes). The only locks that may be held across a hook call are the two
//! tokio mutexes `lifecycle` and `closing`, by the sequences that exist to be serialised; hook
//! implementations never take them. See `docs/design/runtime-split.md` §3.3.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use identity::Identity;
use iroh::endpoint::{Connection, QuicTransportConfig, presets};
use iroh::{Endpoint, EndpointAddr, PublicKey, RelayUrl, SecretKey, Watcher};
use parking_lot::{Mutex, ReentrantMutex};
use proto::{ContactTicket, Msg};
use sha2::{Digest, Sha512};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};

use account_model::accounts::{self, AccountDirs};
use account_model::vault::{self, Dek, Secrets};
use crate::store::{Backing, CallRecord, ContactDevice, Disk, History, OwnDevice, Profile, ProfileV2, State, Store, StoredContact};
use crate::wire::Ctrl;
use crate::hooks::NodeHooks;

pub mod block;
pub mod caps;
pub mod contact_req;
pub mod devlist;
pub mod error;
pub mod frames;
pub mod hooks;
pub mod rekey;
pub mod store;
pub mod version;
pub mod wire;

mod accept;
mod card;
mod call;
mod contacts;
mod devices;
mod lifecycle;
mod outgoing;
mod ownsync;
mod persist;
mod rotation;
mod run;
mod signal;
mod sink;
#[cfg(test)]
mod tests;
mod types;
mod util;
mod video;

pub use self::accept::*;
pub use self::call::*;
pub use self::card::*;
pub use self::contacts::*;
#[cfg(test)]
pub use self::devices::*;
pub use self::hooks::BoxFuture;
pub use self::lifecycle::*;
pub use self::ownsync::*;
pub use self::persist::*;
pub use self::rotation::*;
pub use self::signal::*;
pub use self::sink::*;
pub use self::types::*;
pub use self::util::*;
pub use block::{BLOCKED, BlockNotice, BlockState, BlockedContact};
pub use caps::{CALLS_NOT_ALLOWED, ContactOrigin};
pub use contact_req::{AddOutcome, ContactRequest};
pub use error::Error;

/// Short, because a ticket is a bearer secret until someone redeems it.
pub const TICKET_TTL: u64 = 7 * 24 * 3600;
/// Long, because renewal happens on every answered call; this only bites a contact who never
/// calls back. Unix ms, like every issued statement.
pub const GRANT_TTL: u64 = 365 * 24 * 3600 * 1000;
pub const DIAL_TIMEOUT: Duration = Duration::from_secs(30);
/// All devices of one contact together; each device alone gets at most `DIAL_TIMEOUT`.
pub const DIAL_TOTAL: Duration = Duration::from_secs(45);
/// Devices remembered per contact: what a device list may hold.
pub const MAX_DEVICES: usize = proto::MAX_LIST_DEVICES;
/// An active call that receives no media for this long is over.
pub const NO_AUDIO_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a turned-away connection gets to read our refusal before we let go of its slot.
pub const LINGER: Duration = Duration::from_secs(2);
/// Longest peer-supplied reason we pass on to the UI.
pub const MAX_REASON_CHARS: usize = 100;
pub const RING_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a second incoming call (one that arrives during a call) waits to be answered.
/// `P2P_WAITING_RING_SECS` overrides it, for tests.
pub const WAITING_RING_TIMEOUT: Duration = Duration::from_secs(30);
/// See `close_wait`.
pub const CLOSE_WAIT: Duration = Duration::from_secs(2);
/// Longest `start` waits for a previous endpoint to finish closing (iroh's own bound is ~3 s).
pub const CLOSING_MAX: Duration = Duration::from_secs(10);
/// An active call with no media for this long shows as `reconnecting` (a path change or a
/// network switch), well before the no-audio timeout ends it.
pub const RECONNECT_AFTER: Duration = Duration::from_millis(1500);
/// Longest local alias for a contact, in characters (same cap as names from peers).
pub const MAX_ALIAS_CHARS: usize = 64;
/// One deadline for a connection to get from first packet to hello, and for the reply to the
/// hello we send when adding a contact.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(15);
/// Connections from strangers we hold open at once until they are accepted; past this new ones
/// are refused rather than queued. A slot is held for the whole life of a connection that has
/// not become a call or a contact, refusals and their linger included.
pub const MAX_PENDING_HELLOS: usize = 24;
/// Extra slots only for devices of our contacts, so a flood of strangers cannot lock them out.
/// (Whose device it is is only known once the handshake is done, so a stranger can hold one of
/// these until then; it moves to the general pool or is dropped right after.)
pub const RESERVED_HELLOS: usize = 8;
/// What a peer is told when we turn it away. The real reason is logged locally: telling an
/// unauthenticated stranger "revoked" vs "not a contact" would leak our contact list.
pub const REFUSED: &str = "not accepted";

pub struct Me {
    /// The session this was unlocked in; see `Shared::epoch`.
    pub epoch: u64,
    pub profile: Profile,
    pub id: Identity,
    pub device: [u8; 32],
    pub attestation: proto::SignedAttestation,
}

pub struct Shared {
    /// What `account.json` holds; present whenever an identity exists, locked or not. (Also
    /// present, in memory only, for an identity not yet `commit_identity`ed.)
    pub disk: Option<Disk>,
    /// Present while unlocked (and for a legacy profile).
    pub me: Option<Arc<Me>>,
    /// The vault's data key while unlocked via a vault; never written to disk by the core.
    pub dek: Option<Dek>,
    /// Empty while locked: it is sealed on disk.
    pub state: State,
    pub endpoint: Option<Endpoint>,
    /// Changes whenever the account in memory is replaced, locked or unlocked. Everything that
    /// runs on its own (call tasks, background writes) remembers the value it started under and
    /// is refused once it differs, so nothing of one account is ever written into another.
    pub epoch: u64,
    /// The account's directory and where its state goes; `None` for no identity and for one not
    /// yet committed. `backing` is `None` while locked.
    pub acct: Option<Acct>,
    /// The call log of the unlocked account; empty while locked.
    pub history: Arc<History>,
    /// Sealed; known only while unlocked.
    pub device_label: Option<String>,
    /// `account.json` is on disk. False only for a no-passphrase identity waiting for
    /// `commit_identity`.
    pub committed: bool,
    /// The account doc of own-device sync; open while unlocked.
    pub acc: Option<Arc<account_model::sync::accdoc::AccDoc>>,
    /// The call log of own-device sync, beside the account doc; open while unlocked.
    pub calls: Option<Arc<account_model::sync::callsdoc::CallsDoc>>,
    pub profile: Option<Arc<account_model::sync::profiledoc::ProfileDoc>>,
    pub profile_cache: Option<Arc<account_model::sync::profilecache::ProfileCache>>,
}

pub struct Acct {
    pub id: String,
    pub store: Store,
    pub backing: Option<Backing>,
}

/// The call slot, apart from `Shared` so the 50 Hz audio threads never wait behind a disk
/// write or a handshake holding `Shared`.
#[derive(Default)]
pub struct Live {
    pub call: Option<Arc<Call>>,
    /// A second incoming call, ringing over the active one. At most one; a third is turned
    /// away busy. Takes the slot if the active call ends first.
    pub waiting: Option<Arc<Call>>,
    /// The call that held the slot before, until the next call has waited for its events to
    /// be delivered (so `Ended` always precedes the next call's `Dialing`).
    pub last: Option<Arc<Call>>,
    pub tone: Option<f32>,
}

/// What the engines above the node share: see the crate docs.
pub struct NodeCtx {
    pub handle: tokio::runtime::Handle,
    pub accounts: AccountDirs,
    /// Serialises creating, committing and unlocking an account (they open its files).
    pub gate: Mutex<()>,
    /// Test knob: this many state writes fail. See `Node::fail_next_writes`.
    pub fail_writes: AtomicU32,
    /// Held while snapshotting and writing state, so writes land in the order taken. Always
    /// taken before `shared`, never while holding it.
    pub writing: Mutex<()>,
    pub sink: Arc<dyn NodeSink>,
    pub shared: Mutex<Shared>,
    pub live: Mutex<Live>,
    pub pending: Arc<Semaphore>,
    pub reserved: Arc<Semaphore>,
    /// Serialises start, stop, lock and set_passphrase.
    pub lifecycle: Arc<tokio::sync::Mutex<()>>,
    /// Held while an endpoint of ours is closing (`close_fully`); `start` binds the next one only
    /// once it is free, so two endpoints never hold this device key at once.
    pub closing: Arc<tokio::sync::Mutex<()>>,
    /// Own-device sync: something changed (`sync_bump`), dial now (`sync_kick`), mark a doc dirty.
    pub signal: OwnDocSignal,
    pub block: block::BlockRt,
    /// Contact requests (`contact_req.rs`).
    pub requests: contact_req::ReqRt,
    /// Token delivery (P6, `caps.rs`).
    pub caps: caps::CapsRt,
    pub devlist: devlist::DevlistRt,
    /// Private endpoint rotation (`rotation.rs`).
    pub rotation: RotationRt,
    pub video: call_video::NodeVideo,
    hooks: OnceLock<Weak<dyn NodeHooks>>,
}

impl NodeCtx {
    pub fn new(handle: tokio::runtime::Handle, accounts: AccountDirs, shared: Shared, sink: Arc<dyn NodeSink>) -> Self {
        NodeCtx {
            handle,
            accounts,
            gate: Mutex::new(()),
            fail_writes: AtomicU32::new(0),
            writing: Mutex::new(()),
            sink,
            shared: Mutex::new(shared),
            live: Mutex::new(Live::default()),
            pending: Arc::new(Semaphore::new(MAX_PENDING_HELLOS)),
            reserved: Arc::new(Semaphore::new(RESERVED_HELLOS)),
            lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            closing: Arc::new(tokio::sync::Mutex::new(())),
            signal: Default::default(),
            block: Default::default(),
            requests: Default::default(),
            caps: Default::default(),
            devlist: Default::default(),
            rotation: Default::default(),
            video: Default::default(),
            hooks: OnceLock::new(),
        }
    }

    /// Installs the engines' side of the seam (p2pcore does it right after building its `Arc`).
    /// Only the first call counts.
    pub fn install_hooks(&self, hooks: Weak<dyn NodeHooks>) {
        let _ = self.hooks.set(hooks);
    }

    /// The engines, or `None` while the node is being dropped (callers then do what they would
    /// with no engine: return or no-op). Never call it with a `NodeCtx` data lock held.
    pub fn hooks(&self) -> Option<Arc<dyn NodeHooks>> {
        self.hooks.get().and_then(Weak::upgrade)
    }
}
