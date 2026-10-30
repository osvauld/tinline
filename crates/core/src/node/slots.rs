//! Several accounts reachable on one device (`docs/multi-online.md`): the account slots of a
//! `Node`.
//!
//! Every slot is a full `Inner` (its own `NodeCtx`, endpoint and stores on the same data root),
//! all on the `Node`'s one runtime. The **open** slot is the account the API and the screens act
//! on; the others are **woken** (background) slots: an account a wake push opened (`OnWake`),
//! one the user switched away from, or, on the desktop (`KeepAll`), every account with a
//! remembered key.
//!
//! Rules kept here:
//! - Only the open slot ever writes the `current` selection file (`switch` selects; a woken slot
//!   is only loaded and unlocked, never created or restored).
//! - Two slots never hold the same account: a load checks the open slot and every woken one
//!   under `switching`.
//! - Events: the open slot forwards everything the platform registered; a woken slot forwards
//!   only its 1:1 call events (`SlotSink`, with `CallInfo.account` set) and `AccountEvents`
//!   (`BackgroundChat`). A switch flips both (`set_open`).
//! - One 1:1 call at a time across the slots: they share one `CallFloor` (node-runtime).
//!
//! - Locked (`lock`, or a process that started with its account locked) means every account
//!   stays shut until the open one is unlocked: no wake, `start` or switch opens another.
//!
//! Lock order: `switching` (held across a load or a switch, never by runtime tasks: the reaper
//! only `try_lock`s it) before `slots`, `sinks` and the other short data locks, which are never
//! held across a callback, a load or an await. **No platform callback runs under `switching`**
//! (`account_key`, `AccountEvents`, `NodeEvents`): the platform may switch or lock from inside
//! one.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{OnceLock, Weak};
use std::time::Instant;

use parking_lot::{Condvar, Mutex};

use super::gated::*;

use super::*;
use chat_runtime::chat::api::{Chat, DeliveryState, Message};

/// How long a woken slot stays up with no chat session, ring, dial-back or call (same as a
/// chat link's idle close, `wire::IDLE_CLOSE`).
pub const WOKEN_IDLE_MS: u64 = 60_000;

/// How long a background message waits before its notice: an own device that relays it
/// usually sends the read cursor that says it was read there within this (same session).
const NOTICE_SETTLE_MS: u64 = 1_500;

/// How long a load waits for the same account's slot to finish closing.
const CLOSE_WAIT: Duration = Duration::from_secs(10);

/// Which accounts besides the open one run on this device. Set by the platform before `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, uniffi::Enum)]
pub enum BackgroundMode {
    /// Only the open account runs; a wake for another one is ignored (Liberum, the default).
    #[default]
    OpenOnly,
    /// Another account opens in the background on a wake push for it and closes again once it
    /// is quiet (Play, iOS).
    OnWake,
    /// Every account with a remembered key keeps its endpoint up, and none idles out (desktop).
    KeepAll,
}

/// Background accounts (`set_account_events`). Separate from `NodeEvents`: `account_key` is a
/// question the core asks, not an event, and adding a method to `NodeEvents` would break every
/// platform's implementation at once; a platform that never sets this behaves as `OpenOnly`.
#[uniffi::export(rust, foreign)]
pub trait AccountEvents: Send + Sync {
    /// The data key the platform remembered for `did` (from `unlock_key`, Android Keystore / iOS
    /// Keychain / the OS keyring), or `None` (a passphrase-only account, or not remembered).
    /// Asked on the thread that called `on_wake` or `start`, never on a core thread; blocking,
    /// it should answer within a second (iOS must report a VoIP push to CallKit in seconds).
    fn account_key(&self, did: String) -> Option<Vec<u8>>;
    /// A 1:1 message arrived for a background account: post a notification naming it. Called on
    /// core threads: do not block.
    fn on_background_message(&self, account: AccountRef, from_did: String, from_name: String);
    /// Unread counts or the running accounts changed: refresh the switcher (`accounts`). Called
    /// on core threads: do not block.
    fn on_accounts_changed(&self);
}

/// Which slot an `Inner` is, and whether it is the open one.
pub(crate) struct SlotGate {
    open: AtomicBool,
    /// Counts the times the slot became the open one: a sink installed then passes events only
    /// while it still is (`gated.rs`).
    generation: AtomicU64,
    /// Who this slot's account is, for the events a background slot sends.
    account: Mutex<Option<AccountRef>>,
    hub: OnceLock<Weak<Hub>>,
    inner: OnceLock<Weak<Inner>>,
}

impl SlotGate {
    pub(crate) fn new(open: bool, account: Option<AccountRef>) -> Self {
        SlotGate { open: AtomicBool::new(open), generation: AtomicU64::new(0), account: Mutex::new(account), hub: OnceLock::new(), inner: OnceLock::new() }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Open, and still in the term that began at `generation`.
    pub(crate) fn passes(&self, generation: u64) -> bool {
        self.is_open() && self.generation() == generation
    }

    /// Opens or closes the gate (opening starts a new term).
    pub(crate) fn set(&self, open: bool) {
        if open {
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
        self.open.store(open, Ordering::SeqCst);
    }

    fn account(&self) -> Option<AccountRef> {
        self.account.lock().clone()
    }

    fn hub(&self) -> Option<Arc<Hub>> {
        self.hub.get().and_then(Weak::upgrade)
    }
}

/// The node runtime's events of one slot, gated: a background slot passes on only its calls.
struct SlotSink {
    events: Arc<dyn NodeEvents>,
    gate: Arc<SlotGate>,
}

impl NodeSink for SlotSink {
    fn on_status(&self, status: NodeStatus) {
        if self.gate.is_open() {
            self.events.on_status(status);
        }
    }

    fn on_contacts_changed(&self) {
        if self.gate.is_open() {
            self.events.on_contacts_changed();
        }
    }

    fn on_incoming_call(&self, mut call: CallInfo) {
        if !self.gate.is_open() {
            call.account = self.gate.account();
        }
        self.events.on_incoming_call(call);
    }

    fn on_call_state(&self, call_id: String, state: CallState) {
        self.events.on_call_state(call_id, state);
    }

    fn on_log(&self, line: String) {
        if self.gate.is_open() {
            self.events.on_log(line);
        }
    }
}

/// The chat events of a background slot: an arriving 1:1 message becomes
/// `on_background_message` (once it has settled, if it is still unread); nothing else leaves
/// the slot.
struct BackgroundChat {
    gate: Arc<SlotGate>,
}

impl chat_runtime::ChatSink for BackgroundChat {
    fn on_message_added(&self, message: Message) {
        if message.outgoing || self.gate.is_open() {
            return;
        }
        let (Some(hub), Some(account), Some(inner)) = (self.gate.hub(), self.gate.account(), self.gate.inner.get().and_then(Weak::upgrade)) else {
            return;
        };
        // A note to self relayed by an own device is not someone writing to this account.
        if message.peer_did == account.did {
            return;
        }
        // Off this callback: an own device may relay the message with the read cursor that
        // says it was read there (own-device sync sends them in no fixed order), so the notice
        // waits for it; the count reads the chat list, the file write is a disk write.
        let h = hub.clone();
        hub.handle.spawn(async move {
            tokio::time::sleep(Duration::from_millis(NOTICE_SETTLE_MS)).await;
            let _ = tokio::task::spawn_blocking(move || h.background_notice(&inner, account, message)).await;
        });
    }

    fn on_message_changed(&self, _message: Message) {}
    fn on_chat_changed(&self, _chat: Chat) {}
    fn on_delivery_changed(&self, _peer_did: String, _message_id: String, _delivery: DeliveryState) {}
    fn on_transfer_progress(&self, _peer_did: String, _hash: String, _done: u64, _total: u64, _outgoing: bool) {}
    fn on_presence_changed(&self, _peer_did: String, _online: bool) {}
}

/// What the platform registered with the `set_*_events` setters; installed in the open slot.
#[derive(Default, Clone)]
pub(crate) struct Sinks {
    pub(crate) chat: Option<Arc<dyn chat_runtime::ChatSink>>,
    pub(crate) profile: Option<Arc<dyn chat_runtime::ProfileSink>>,
    pub(crate) video: Option<Arc<dyn call_video::VideoSink>>,
    pub(crate) link: Option<Arc<dyn sync_runtime::LinkSink>>,
    pub(crate) group: Option<Arc<dyn group_runtime::api::GroupSink>>,
    pub(crate) group_call: Option<Arc<dyn group_runtime::calls::GroupCallSink>>,
    pub(crate) group_invitation: Option<Arc<dyn group_runtime::invitations::GroupInvitationSink>>,
    pub(crate) group_request: Option<Arc<dyn group_runtime::requests::GroupRequestSink>>,
    pub(crate) contact_request: Option<Arc<dyn ContactRequestSink>>,
}

/// Test knobs every slot gets, not only the one open when they were set.
#[derive(Default)]
pub(crate) struct Knobs {
    pub(crate) auto_accept: bool,
    /// A slot's close waits this long before it starts (tests: a wake during a close).
    pub(crate) close_delay_ms: u64,
}

struct Woken {
    inner: Arc<Inner>,
    /// Since when nothing has kept it up (`None`: busy, or just woken).
    idle_since: Option<Instant>,
}

struct Slots {
    open: Arc<Inner>,
    /// By account id (the account's directory name).
    woken: HashMap<String, Woken>,
}

/// Everything the slots of one `Node` share.
pub(crate) struct Hub {
    pub(crate) handle: tokio::runtime::Handle,
    root: PathBuf,
    events: Arc<dyn NodeEvents>,
    floor: Arc<CallFloor>,
    slots: Mutex<Slots>,
    /// Serialises loading, switching and closing slots (see the module docs).
    switching: Mutex<()>,
    sinks: Mutex<Sinks>,
    account_events: Mutex<Option<Arc<dyn AccountEvents>>>,
    mode: Mutex<BackgroundMode>,
    /// The last `set_push_target` of this process, for slots that open later.
    push: Mutex<Option<Option<PushTarget>>>,
    /// Hint keys of accounts woken before, so a later wake finds its account without unlocking
    /// every other one again. Kept while the process lives; `lock` forgets them.
    hint_keys: Mutex<HashMap<String, zeroize::Zeroizing<[u8; 32]>>>,
    /// The app is locked: no account opens in the background until the open one is unlocked.
    locked: AtomicBool,
    /// Accounts whose slot is closing; a load of one waits for `closed` first, so that two slots
    /// never hold one account's stores.
    closing: Mutex<HashSet<String>>,
    closed: Condvar,
    /// Held while a call wake decides whether it may ring (one ring across the accounts).
    ringing: Mutex<()>,
    pub(crate) knobs: Mutex<Knobs>,
    /// `WOKEN_IDLE_MS`, or a test's override.
    idle_ms: AtomicU64,
}

/// Builds one slot on `handle`, holding `loaded` (locked) or no account.
fn build_slot(
    handle: &tokio::runtime::Handle,
    root: &std::path::Path,
    events: &Arc<dyn NodeEvents>,
    floor: &Arc<CallFloor>,
    loaded: Option<Loaded>,
    open: bool,
) -> Result<Arc<Inner>, Error> {
    let accounts = AccountDirs::open(root)?;
    let account = loaded.as_ref().map(|l| account_ref(&l.disk));
    let mut shared = Shared::new();
    if let Some(l) = loaded {
        shared.install(l)?;
    }
    let gate = Arc::new(SlotGate::new(open, account));
    let sink = Arc::new(SlotSink { events: events.clone(), gate: gate.clone() });
    let ctx = Arc::new(NodeCtx::new(handle.clone(), accounts, shared, sink).with_floor(floor.clone()));
    let selfsync: Arc<sync_runtime::engine::SelfSync> = Default::default();
    let sink: std::sync::Weak<dyn OwnDocSink> = Arc::downgrade(&(selfsync.clone() as Arc<dyn OwnDocSink>));
    ctx.signal.set_sink(sink);
    let search: Arc<search_ix::SearchIx> = Default::default();
    let chat = Arc::new(chat_runtime::ChatRt::new(ctx.clone(), search.clone()));
    let group = Arc::new(group_runtime::GroupRt::new(ctx.clone(), search.clone()));
    {
        // Group files auto-download within the 1:1 attachment size limit (docs/design/group-files.md §3).
        let chat = Arc::downgrade(&chat);
        *group.file_auto_limit.lock() = Some(Arc::new(move || chat.upgrade()?.chat_core().ok().map(|c| c.store.auto_download())));
    }
    let sync = Arc::new(sync_runtime::SyncRt::new(ctx.clone(), chat.clone(), group.clone(), selfsync));
    let inner = Arc::new(Inner { ctx, chat, group, sync, search, slot: gate.clone() });
    inner.ctx.join_floor();
    let gcall_host: std::sync::Weak<group_runtime::GroupRt> = Arc::downgrade(&inner.group);
    inner.group.gcall.install_host(gcall_host);
    let host: std::sync::Weak<Inner> = Arc::downgrade(&inner);
    inner.ctx.install_hooks(host);
    let _ = gate.inner.set(Arc::downgrade(&inner));
    Ok(inner)
}

fn account_ref(disk: &Disk) -> AccountRef {
    let Disk::V2(p) = disk;
    AccountRef { did: p.did.clone(), name: p.name.clone() }
}

/// The account a slot holds: its id (directory name), if it has one on disk.
fn id_of_slot(inner: &Inner) -> Option<String> {
    inner.shared.lock().acct.as_ref().map(|a| a.id.clone())
}

impl Hub {
    /// The hub, its open slot holding the `current` account (locked), and the idle reaper.
    pub(crate) fn new(handle: tokio::runtime::Handle, data_dir: String, events: Arc<dyn NodeEvents>) -> Result<Arc<Self>, Error> {
        let root = PathBuf::from(data_dir);
        let accounts = AccountDirs::open(&root)?;
        let loaded = match accounts.current() {
            Some(id) => Some(load_account(&accounts, &id)?),
            None => None,
        };
        // The account opens locked: the app is locked until it is unlocked.
        let locked = loaded.is_some();
        let floor: Arc<CallFloor> = Default::default();
        let open = build_slot(&handle, &root, &events, &floor, loaded, true)?;
        let hub = Arc::new(Hub {
            handle,
            root,
            events,
            floor,
            slots: Mutex::new(Slots { open: open.clone(), woken: HashMap::new() }),
            switching: Mutex::new(()),
            sinks: Default::default(),
            account_events: Mutex::new(None),
            mode: Mutex::new(BackgroundMode::default()),
            push: Mutex::new(None),
            hint_keys: Mutex::new(HashMap::new()),
            locked: AtomicBool::new(locked),
            closing: Mutex::new(HashSet::new()),
            closed: Condvar::new(),
            ringing: Mutex::new(()),
            knobs: Default::default(),
            idle_ms: AtomicU64::new(WOKEN_IDLE_MS),
        });
        let _ = open.slot.hub.set(Arc::downgrade(&hub));
        hub.spawn_reaper();
        Ok(hub)
    }

    pub(crate) fn open(&self) -> Arc<Inner> {
        self.slots.lock().open.clone()
    }

    /// The open slot first, then every woken one.
    pub(crate) fn all_slots(&self) -> Vec<Arc<Inner>> {
        let s = self.slots.lock();
        std::iter::once(s.open.clone()).chain(s.woken.values().map(|w| w.inner.clone())).collect()
    }

    fn woken(&self) -> Vec<Arc<Inner>> {
        self.slots.lock().woken.values().map(|w| w.inner.clone()).collect()
    }

    /// Whether a woken slot holds the account `id`.
    pub(crate) fn is_woken(&self, id: &str) -> bool {
        self.slots.lock().woken.contains_key(id)
    }

    pub(crate) fn mode(&self) -> BackgroundMode {
        *self.mode.lock()
    }

    pub(crate) fn set_mode(&self, mode: BackgroundMode) {
        *self.mode.lock() = mode;
    }

    pub(crate) fn account_events(&self) -> Option<Arc<dyn AccountEvents>> {
        self.account_events.lock().clone()
    }

    pub(crate) fn set_account_events(&self, events: Arc<dyn AccountEvents>) {
        *self.account_events.lock() = Some(events);
    }

    pub(crate) fn accounts_changed(&self) {
        if let Some(ev) = self.account_events() {
            ev.on_accounts_changed();
        }
    }

    /// `lock`: from now on no account opens in the background (decided 2026-10-27, option a).
    pub(crate) fn set_locked(&self) {
        self.locked.store(true, Ordering::SeqCst);
    }

    /// `slot` was unlocked (or given an identity): if it is the open one, the app is unlocked.
    pub(crate) fn unlocked(&self, slot: &Inner) {
        if slot.slot.is_open() && slot.me().is_ok() {
            self.locked.store(false, Ordering::SeqCst);
        }
    }

    pub(crate) fn is_locked(&self) -> bool {
        self.locked.load(Ordering::SeqCst)
    }

    #[cfg(any(test, feature = "dev-tools"))]
    pub(crate) fn set_idle_ms(&self, ms: u64) {
        self.idle_ms.store(if ms == 0 { WOKEN_IDLE_MS } else { ms }, Ordering::Relaxed);
    }

    // ---- events ----

    /// Records what the platform registered and installs it in the open slot.
    pub(crate) fn set_sinks(&self, f: impl FnOnce(&mut Sinks)) {
        f(&mut self.sinks.lock());
        self.install_sinks(&self.open());
    }

    /// The open slot gets the platform's sinks, each gated to this term of it being open
    /// (`gated.rs`); a woken one only `BackgroundChat`. The video sink follows the call, which
    /// never changes slots mid-call.
    fn install_sinks(&self, inner: &Arc<Inner>) {
        let g = &inner.slot;
        let s = if g.is_open() {
            let s = self.sinks.lock().clone();
            Sinks {
                chat: s.chat.map(|x| GatedChat::new(x, g)),
                profile: s.profile.map(|x| GatedProfile::new(x, g)),
                video: s.video,
                link: s.link.map(|x| GatedLink::new(x, g)),
                group: s.group.map(|x| GatedGroup::new(x, g)),
                group_call: s.group_call.map(|x| GatedGroupCall::new(x, g)),
                group_invitation: s.group_invitation.map(|x| GatedInvitation::new(x, g)),
                group_request: s.group_request.map(|x| GatedRequest::new(x, g)),
                contact_request: s.contact_request.map(|x| GatedContactRequest::new(x, g)),
            }
        } else {
            Sinks { chat: Some(Arc::new(BackgroundChat { gate: g.clone() })), ..Sinks::default() }
        };
        *inner.chat.chat_events.lock() = s.chat;
        *inner.chat.profile_events.lock() = s.profile;
        match s.video {
            Some(v) => inner.video.set_sink(v),
            None => inner.video.clear_sink(),
        }
        *inner.sync.link_events.lock() = s.link;
        *inner.group.events.lock() = s.group;
        *inner.group.calls.events.lock() = s.group_call;
        *inner.group.inv_events.lock() = s.group_invitation;
        *inner.group.req_events.lock() = s.group_request;
        *inner.requests.events.lock() = s.contact_request;
    }

    /// Flips the slot's gate and its sinks.
    fn set_open(&self, inner: &Arc<Inner>, open: bool) {
        if !open {
            // The events it sends from now on name it.
            let disk = inner.shared.lock().disk.clone();
            *inner.slot.account.lock() = disk.as_ref().map(account_ref);
        }
        inner.slot.set(open);
        self.install_sinks(inner);
    }

    // ---- slots ----

    fn new_slot(self: &Arc<Self>, loaded: Option<Loaded>, open: bool) -> Result<Arc<Inner>, Error> {
        let inner = build_slot(&self.handle, &self.root, &self.events, &self.floor, loaded, open)?;
        let _ = inner.slot.hub.set(Arc::downgrade(self));
        inner.requests.auto_accept.store(self.knobs.lock().auto_accept, Ordering::SeqCst);
        self.install_sinks(&inner);
        Ok(inner)
    }

    fn dirs(&self) -> Result<AccountDirs, Error> {
        Ok(AccountDirs::open(&self.root)?)
    }

    /// Loads the account `id` into a new background slot and unlocks it with `key`. Not yet
    /// in `woken`; the caller adds it (`switching` held).
    fn load_unlocked(self: &Arc<Self>, id: &str, key: Vec<u8>) -> Result<Arc<Inner>, Error> {
        let loaded = load_account(&self.dirs()?, id)?;
        let inner = self.new_slot(Some(loaded), false)?;
        if let Err(e) = inner.unlock_with_key(key) {
            self.handle.spawn(self.clone().close_slot(inner));
            return Err(e);
        }
        if let Some(k) = inner.ctx.wake_hint_key() {
            self.hint_keys.lock().insert(id.to_string(), zeroize::Zeroizing::new(k));
        }
        // The same device token for every account.
        if let Some(target) = self.push.lock().clone()
            && let Err(e) = inner.ctx.set_push_target(target.map(Into::into))
        {
            tracing::warn!("push target for a background account: {e}");
        }
        Ok(inner)
    }

    /// `switching` held: puts `inner` into `woken`.
    fn add_woken(&self, id: String, inner: Arc<Inner>) {
        self.slots.lock().woken.insert(id, Woken { inner, idle_since: None });
    }

    /// Takes every woken slot out (they are closed by the caller).
    pub(crate) fn take_woken(&self) -> Vec<Arc<Inner>> {
        let _sw = self.switching.lock();
        self.slots.lock().woken.drain().map(|(_, w)| w.inner).collect()
    }

    /// Takes the woken slot of `id` out, if there is one (closed by the caller).
    pub(crate) fn take_woken_of(&self, id: &str) -> Option<Arc<Inner>> {
        let _sw = self.switching.lock();
        self.slots.lock().woken.remove(id).map(|w| w.inner)
    }

    /// `lock`: hint keys leave memory with everything else.
    pub(crate) fn forget_hint_keys(&self) {
        self.hint_keys.lock().clear();
    }

    /// `remove_account`: the removed account's hint key goes with it.
    pub(crate) fn forget_hint_key(&self, id: &str) {
        self.hint_keys.lock().remove(id);
    }

    /// Waits until no slot of `id` is closing (`CLOSE_WAIT` at most; never on a core thread,
    /// where the close itself may need the worker). False: still closing.
    fn wait_closed(&self, id: &str) -> bool {
        let mut closing = self.closing.lock();
        if !closing.contains(id) {
            return true;
        }
        if super::on_core_thread() {
            return false;
        }
        let deadline = Instant::now() + CLOSE_WAIT;
        while closing.contains(id) {
            if self.closed.wait_until(&mut closing, deadline).timed_out() {
                return !closing.contains(id);
            }
        }
        true
    }

    /// Makes `next` the open slot (selecting `id`, or no account for `None`). The previous open
    /// slot becomes a woken one when it can run on (unlocked, on disk, not `OpenOnly`); otherwise
    /// it is returned for the caller to close. `switching` must be held; the caller tells the
    /// platform afterwards (`announce_open`), with it released.
    fn make_open(self: &Arc<Self>, next: Arc<Inner>, id: Option<&str>) -> Result<Option<Arc<Inner>>, Error> {
        let dirs = self.dirs()?;
        match id {
            Some(id) => dirs.select(id)?,
            None => dirs.deselect()?,
        }
        let prev = self.open();
        let prev_id = id_of_slot(&prev);
        let keep = self.mode() != BackgroundMode::OpenOnly && prev.me().is_ok() && prev.shared.lock().committed;
        // The account left goes quiet on the screens before the next one opens: never both.
        match prev_id {
            Some(_) if keep => self.set_open(&prev, false),
            _ => prev.slot.set(false),
        }
        {
            let mut s = self.slots.lock();
            if let Some(id) = id {
                s.woken.remove(id);
            }
            s.open = next.clone();
        }
        self.set_open(&next, true);
        if let Some(id) = id {
            // Opened: its own read cursors take over from the switcher's count.
            self.write_unread(id, 0);
        }
        let closing = match prev_id {
            Some(pid) if keep => {
                // Its group call ends as on a switch today; the screens cannot show it any more.
                let p = prev.clone();
                self.handle.spawn(async move { p.group.gcall_shutdown().await });
                self.add_woken(pid, prev);
                None
            }
            _ => Some(prev),
        };
        Ok(closing)
    }

    /// After a switch, with `switching` released: the screens refresh for the open slot.
    fn announce_open(&self, next: &Inner) {
        next.emit_status();
        next.sink.on_contacts_changed();
        self.accounts_changed();
    }

    /// The open slot is about to be left: a wake ring answered early on it must not connect in
    /// the background, so it rings again instead (`wake_rings_unanswer`). Then, as before, no
    /// switch during a 1:1 call. (A call that arrived meanwhile is `InCall`; it rings, unanswered,
    /// in the slot that stays open.)
    fn leave_open(&self) -> Result<(), Error> {
        let open = self.open();
        open.not_in_call()?;
        open.ctx.wake_rings_unanswer();
        Ok(open.not_in_call()?)
    }

    /// `switch_account` to an account that is not open, outside `OpenOnly`: a woken slot opens
    /// as it is; any other account is loaded locked into a new slot. Returns the slot to close.
    pub(crate) fn switch_to(self: &Arc<Self>, id: &str) -> Result<Option<Arc<Inner>>, Error> {
        let (closing, next) = {
            let _sw = self.switching.lock();
            self.open().not_in_call()?;
            let woken = self.slots.lock().woken.get(id).map(|w| w.inner.clone());
            let next = match woken {
                Some(w) => w,
                None => {
                    if !self.wait_closed(id) {
                        return Err(Error::Busy);
                    }
                    let loaded = load_account(&self.dirs()?, id)?;
                    self.new_slot(Some(loaded), false)?
                }
            };
            self.leave_open()?;
            (self.make_open(next.clone(), Some(id))?, next)
        };
        self.announce_open(&next);
        Ok(closing)
    }

    /// `begin_new_account` outside `OpenOnly`: an empty slot opens. Returns the slot to close.
    pub(crate) fn open_blank(self: &Arc<Self>) -> Result<Option<Arc<Inner>>, Error> {
        let (closing, next) = {
            let _sw = self.switching.lock();
            self.open().not_in_call()?;
            let next = self.new_slot(None, false)?;
            self.leave_open()?;
            (self.make_open(next.clone(), None)?, next)
        };
        self.announce_open(&next);
        Ok(closing)
    }

    /// Closes a slot for good: its call floor, endpoints, sessions and stores; its unread
    /// count is saved first. The account counts as closing from this call (not from the first
    /// poll of the future), so that a load of it waits (`wait_closed`).
    pub(crate) fn close_slot(self: Arc<Self>, inner: Arc<Inner>) -> impl std::future::Future<Output = ()> + Send + 'static {
        let id = id_of_slot(&inner);
        if let Some(id) = &id {
            self.closing.lock().insert(id.clone());
        }
        async move {
            let delay = self.knobs.lock().close_delay_ms;
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            inner.sync.link_cancel();
            {
                let _life = inner.lifecycle.lock().await;
                inner.chat.chat_sessions_close();
                inner.group.gcall_shutdown().await;
                inner.close_endpoint().await;
                self.save_unread(&inner);
                inner.chat.chat_close().await;
                inner.group.group_close_now();
                inner.shared.lock().forget_session();
            }
            inner.floor.release(inner.slot_id);
            if let Some(id) = id {
                self.closing.lock().remove(&id);
                self.closed.notify_all();
            }
            self.accounts_changed();
        }
    }

    // ---- wakes ----

    /// The accounts on disk that no slot holds (`switching` held).
    fn sleeping(&self) -> Vec<accounts::Entry> {
        let open_id = id_of_slot(&self.open());
        let entries = self.dirs().and_then(|d| Ok(d.list()?)).unwrap_or_else(|e| {
            tracing::warn!("listing accounts: {e}");
            Vec::new()
        });
        entries.into_iter().filter(|e| open_id.as_deref() != Some(e.id.as_str()) && !self.is_woken(&e.id)).collect()
    }

    /// Loads the sleeping account `e` with the `key` the platform gave (asked with no lock
    /// held) and keeps it running if `wanted`. `switching` is taken again here, and everything
    /// rechecked: the app may have been locked, or the account opened, meanwhile; a slot of it
    /// still closing is waited for.
    fn load_sleeping(self: &Arc<Self>, e: &accounts::Entry, mut key: Vec<u8>, wanted: impl FnOnce(&Inner) -> bool) -> Option<Arc<Inner>> {
        let _sw = self.switching.lock();
        let held = self.is_locked() || id_of_slot(&self.open()).as_deref() == Some(e.id.as_str()) || self.is_woken(&e.id);
        let closed = !held && self.wait_closed(&e.id);
        if !closed {
            if !held {
                tracing::warn!("a background account is still closing; not opened");
            }
            key.zeroize();
            return None;
        }
        match self.load_unlocked(&e.id, key) {
            Ok(inner) if wanted(&inner) => {
                self.add_woken(e.id.clone(), inner.clone());
                Some(inner)
            }
            Ok(inner) => {
                self.handle.spawn(self.clone().close_slot(inner));
                None
            }
            Err(err) => {
                tracing::warn!("opening a background account: {err}");
                None
            }
        }
    }

    /// The background slot a wake `hint` is for: a running woken slot whose key opens it; else,
    /// with `load` (`OnWake`), another account on disk the platform has a key for, loaded and
    /// unlocked now. `None`: no account here opens it, or the app is locked.
    pub(crate) fn wake_slot(self: &Arc<Self>, hint: &str, load: bool) -> Option<Arc<Inner>> {
        let running = |hub: &Hub| {
            let mut s = hub.slots.lock();
            s.woken.values_mut().find(|w| w.inner.wake_opens(hint)).map(|w| {
                w.idle_since = None;
                w.inner.clone()
            })
        };
        let entries = {
            let _sw = self.switching.lock();
            if self.is_locked() {
                return None;
            }
            if let Some(w) = running(self) {
                return Some(w);
            }
            if !load {
                return None;
            }
            self.sleeping()
        };
        for e in entries {
            // Known not to be this account's wake: no unlock needed to tell.
            if let Some(k) = self.hint_keys.lock().get(&e.id)
                && !node_runtime::push::hint_opens(k, hint)
            {
                continue;
            }
            // A passphrase-only account (no key here) sleeps until it is unlocked.
            let Some(key) = self.account_events().and_then(|ev| ev.account_key(e.did.clone())) else { continue };
            let Some(inner) = self.load_sleeping(&e, key, |i| i.wake_opens(hint)) else {
                // Not this account's wake; or locked meanwhile; or another wake opened it,
                // perhaps for this very hint.
                if self.is_locked() {
                    return None;
                }
                match running(self) {
                    Some(w) => return Some(w),
                    None => continue,
                }
            };
            self.accounts_changed();
            return Some(inner);
        }
        None
    }

    /// `KeepAll`: every account the platform has a key for runs in a woken slot (they start
    /// with `start`). Accounts that fail to open are skipped; nothing opens while locked.
    pub(crate) fn open_all(self: &Arc<Self>) {
        let entries = {
            let _sw = self.switching.lock();
            if self.is_locked() {
                return;
            }
            self.sleeping()
        };
        let mut opened = false;
        for e in entries {
            let Some(key) = self.account_events().and_then(|ev| ev.account_key(e.did.clone())) else { continue };
            opened |= self.load_sleeping(&e, key, |_| true).is_some();
        }
        if opened {
            self.accounts_changed();
        }
    }

    // ---- calls ----

    /// The woken slot that has the call `call_id`.
    pub(crate) fn woken_with_call(&self, call_id: &str) -> Option<(String, Arc<Inner>)> {
        self.slots.lock().woken.iter().find(|(_, w)| w.inner.has_call(call_id)).map(|(id, w)| (id.clone(), w.inner.clone()))
    }

    /// One ring at a time across the accounts: a call wake for `slot` is turned away (`EndCall`)
    /// while another slot holds the call floor or rings for a wake, or, for a background slot,
    /// while the user is in a group call (a call to the open account during its own group call
    /// rings as on one account, and is busy when it arrives).
    pub(crate) fn ring_refused(&self, slot: &Inner) -> bool {
        let (floor, id) = (&slot.floor, slot.slot_id);
        floor.held_by_other(id) || floor.wake_ring_elsewhere(id) || (!slot.slot.is_open() && floor.group_call_elsewhere(id))
    }

    /// Held while a call wake rings or is refused, so two wakes cannot both ring.
    pub(crate) fn ring_gate(&self) -> parking_lot::MutexGuard<'_, ()> {
        self.ringing.lock()
    }

    /// The woken slot ringing for a wake from `did`.
    pub(crate) fn woken_ringing(&self, did: &str) -> Option<(String, Arc<Inner>)> {
        self.slots.lock().woken.iter().find(|(_, w)| w.inner.wake_ringing(did)).map(|(id, w)| (id.clone(), w.inner.clone()))
    }

    // ---- idle ----

    fn spawn_reaper(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        self.handle.spawn(async move {
            loop {
                let pause = match weak.upgrade() {
                    Some(hub) => hub.idle_tick(),
                    None => return,
                };
                tokio::time::sleep(pause).await;
            }
        });
    }

    /// One look at the woken slots: a slot quiet for the idle time closes (`OnWake`; under
    /// `KeepAll` none does). Returns the pause before the next look.
    fn idle_tick(self: &Arc<Self>) -> Duration {
        let idle = Duration::from_millis(self.idle_ms.load(Ordering::Relaxed));
        let pause = (idle / 4).clamp(Duration::from_millis(50), Duration::from_secs(5));
        if self.mode() == BackgroundMode::KeepAll {
            return pause;
        }
        // A load or a switch is under way: look again next time.
        let Some(_sw) = self.switching.try_lock() else { return pause };
        let now = Instant::now();
        let mut closing = Vec::new();
        self.slots.lock().woken.retain(|_, w| {
            if slot_busy(&w.inner) {
                w.idle_since = None;
                return true;
            }
            let since = *w.idle_since.get_or_insert(now);
            if now.duration_since(since) >= idle {
                closing.push(w.inner.clone());
                return false;
            }
            true
        });
        for inner in closing {
            self.handle.spawn(self.clone().close_slot(inner));
        }
        pause
    }

    // ---- unread ----

    /// Unread 1:1 messages of a slot: the sum of its conversations' counts.
    fn unread_of(inner: &Inner) -> u32 {
        inner.chat.chat_list().map(|l| l.iter().map(|c| c.unread).sum()).unwrap_or(0)
    }

    fn unread_path(&self, id: &str) -> Option<PathBuf> {
        Some(self.dirs().ok()?.dir(id).ok()?.join("unread"))
    }

    /// A background message has settled: notify unless the account opened meanwhile or the
    /// message was read on another own device (its read cursor is past it).
    fn background_notice(&self, inner: &Arc<Inner>, account: AccountRef, message: Message) {
        if inner.slot.is_open() {
            return;
        }
        let pair = chat_runtime::chat::doc::pair_id(&account.did, &message.peer_did);
        let cursor = inner.chat.chat_read_cursors().into_iter().find(|(p, _)| *p == pair).map_or(0, |(_, c)| c);
        if !message.deleted && i64::try_from(message.at).unwrap_or(i64::MAX) > cursor {
            let name = inner.ctx.wake_label(&message.peer_did);
            if let Some(ev) = self.account_events() {
                ev.on_background_message(account, message.peer_did, name);
            }
        }
        self.save_unread(inner);
        self.accounts_changed();
    }

    /// Saves a background slot's unread count in its account dir (a clear number only).
    pub(crate) fn save_unread(&self, inner: &Inner) {
        if inner.slot.is_open() || inner.me().is_err() {
            return;
        }
        if let Some(id) = id_of_slot(inner) {
            self.write_unread(&id, Self::unread_of(inner));
        }
    }

    /// Saves the open slot's unread count as it goes to sleep (`OpenOnly` switches).
    pub(crate) fn save_unread_leaving(&self, inner: &Inner) {
        if inner.me().is_err() {
            return;
        }
        if let Some(id) = id_of_slot(inner) {
            self.write_unread(&id, Self::unread_of(inner));
        }
    }

    pub(crate) fn write_unread(&self, id: &str, n: u32) {
        let Some(path) = self.unread_path(id) else { return };
        let r = if n == 0 {
            match std::fs::remove_file(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                r => r,
            }
        } else {
            std::fs::write(&path, n.to_string())
        };
        if let Err(e) = r {
            tracing::warn!("saving an account's unread count: {e}");
        }
    }

    /// What the switcher shows for account `id`: 0 for the open account, the live count of a
    /// woken one, else what it had when it last closed.
    pub(crate) fn unread(&self, id: &str, current: bool) -> u32 {
        if current {
            return 0;
        }
        let woken = self.slots.lock().woken.get(id).map(|w| w.inner.clone());
        if let Some(w) = woken {
            return Self::unread_of(&w);
        }
        self.unread_path(id).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| s.trim().parse().ok()).unwrap_or(0)
    }

    // ---- tests ----

    #[cfg(any(test, feature = "dev-tools"))]
    pub(crate) fn hint_keys_len(&self) -> usize {
        self.hint_keys.lock().len()
    }
}

impl Inner {
    /// Who this slot's account is, for what it reports while in the background.
    pub(crate) fn slot_account(&self) -> Option<AccountRef> {
        self.slot.account()
    }
}

/// Something keeps a woken slot up: a call (or ring), a wake ring or queued dial-back, a chat
/// session or dial, an own-device session.
fn slot_busy(inner: &Inner) -> bool {
    inner.not_in_call().is_err()
        || inner.wake_busy()
        || inner.chat.chat_busy()
        || !inner.sync.selfsync.sessions.lock().is_empty()
}

impl Hub {
    /// `set_push_target` for every running slot; remembered for slots that open later.
    pub(crate) fn set_push_target(&self, target: Option<PushTarget>) -> Result<(), Error> {
        *self.push.lock() = Some(target.clone());
        for w in self.woken() {
            if let Err(e) = w.ctx.set_push_target(target.clone().map(Into::into)) {
                tracing::warn!("push target for a background account: {e}");
            }
        }
        self.open().ctx.set_push_target(target.map(Into::into)).map_err(Error::from)
    }

    /// Every running background slot.
    pub(crate) fn background(&self) -> Vec<Arc<Inner>> {
        self.woken()
    }
}
