//! The iced application: onboarding, unlock, the two-pane home, call screens and settings.
//! State and updates live here; `view` builds the screens.

#[path = "view.rs"]
mod view;

#[path = "demo.rs"]
mod demo;
#[cfg(feature = "test-hooks")]
#[path = "demo_pg.rs"]
mod demo_pg;

#[path = "chat.rs"]
mod chat;

#[path = "signup.rs"]
mod signup;
use signup::{AddUi, Gate, Gs};

#[path = "link.rs"]
mod link;

#[cfg(any(test, feature = "test-hooks"))]
#[path = "groups.rs"]
mod groups;
#[cfg(any(test, feature = "test-hooks"))]
use groups::{GroupsUi, Gm};
#[path = "group_live.rs"]
mod group_live;
#[path = "gcall.rs"]
mod gcall;
use group_live::{GroupsLive, Lm};
#[path = "vcall.rs"]
mod vcall;
use vcall::{VideoUi, Vm};
use link::{LinkState, Role, UnlinkedInfo};

use chat::{ChatState, Cm};
#[path = "shell_search.rs"]
mod shell_search;
#[path = "profile_ui.rs"]
mod profile_ui;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::operation;
use iced::{clipboard, system, theme, window, Size, Subscription, Task};
use p2pcore::{
    AccountSummary, Availability, CallInfo, CallRecord, CallState, CallStats, Contact, Error, LockState, Node, NodeStatus,
};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::audio;
use crate::flow::{self, SetupErr};
use crate::keystore;
use crate::reason::{self, End};
use crate::tray::TrayCmd;
use crate::ui;
use crate::{Ev, INIT};

const DEFAULT_LABEL: &str = "System default";
const REVEAL_SECS: u64 = 60;
/// How long the call-ended screen stays before closing itself.
const ENDED_SECS: u64 = 4;
const RECENTS: u32 = 50;
const WINDOW: Size = Size::new(1100.0, 720.0);

pub fn run() -> iced::Result {
    iced::daemon(App::boot, App::update, App::view)
        .title(|_: &App, _| "Tinline".to_string())
        .theme(|a: &App, _| ui::theme(a.dark))
        // Loaded before the first frame (font::load is async and text laid out earlier keeps the
        // fallback face).
        .font(ui::FIGTREE_BYTES)
        .font(ui::PLEX_REGULAR_BYTES)
        .font(ui::PLEX_MEDIUM_BYTES)
        .default_font(ui::SANS)
        .subscription(App::subscription)
        .run()
}

#[derive(Default, Serialize, Deserialize, Clone)]
struct Settings {
    input: Option<String>,
    output: Option<String>,
    tone: bool,
    /// The groups & privacy preview switch (tests and test-hooks only).
    #[serde(default)]
    #[cfg(any(test, feature = "test-hooks"))]
    groups_preview: Option<bool>,
    /// Calls are audio only (no video offered or taken).
    #[serde(default)]
    no_video: bool,
    /// Device-local capture preference; applies at the next camera start.
    #[serde(default)]
    prefer_hd: bool,
    #[serde(default)]
    camera_device: Option<String>,
    /// Appearance: None follows the system; Some(dark) is an explicit choice (the sign-up switch).
    #[serde(default)]
    appearance: Option<bool>,
    /// Settings > Sounds: Quiet, and the call sound set ("classic", "slack", "triad").
    #[serde(default)]
    quiet: bool,
    #[serde(default)]
    palette: String,
    /// Settings > Notifications, stored as what is switched off (so a missing file means all on).
    #[serde(default)]
    no_notify_messages: bool,
    #[serde(default)]
    no_notify_calls: bool,
    #[serde(default)]
    no_notify_groups: bool,
}

impl Settings {
    /// Hands the sound and alert choices to the places that read them (audio threads, notify sites).
    fn apply_alerts(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        crate::audio::QUIET.store(self.quiet, Relaxed);
        crate::audio::RING_CALLS.store(!self.no_notify_calls, Relaxed);
        let p = crate::cues::Palette::from_key(&self.palette);
        crate::audio::PALETTE.store(crate::cues::Palette::ALL.iter().position(|x| *x == p).unwrap_or(0) as u8, Relaxed);
        NOTIFY_MESSAGES.store(!self.no_notify_messages, Relaxed);
        NOTIFY_GROUPS.store(!self.no_notify_groups, Relaxed);
    }
}

/// Onboarding in progress between creating the identity and the first Home.
struct Setup {
    /// The 24 words, to show once the setup is complete (not for a restore).
    phrase: Option<Zeroizing<String>>,
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum Screen {
    Onboarding,
    Unlock,
    /// Last onboarding step: name this computer.
    NameDevice,
    /// The key could not be saved; the identity is still only in memory.
    KeyFailed,
    Phrase,
    Home,
    AddContact,
    Settings,
    /// Linking a device (name, QR, code, approve, done), either role.
    Link,
    /// New device linked: protect the account on this computer, then commit it.
    LinkSecure,
    /// New device linked and saved: contacts and history are coming over.
    LinkSync,
    /// This computer was removed from its account.
    Unlinked,
}

/// What the home screen's right pane shows about the selected contact.
#[derive(PartialEq, Clone, Copy, Debug)]
enum Detail {
    View,
    Rename,
    Verify,
    /// "Block X?" (U19: a contact's action is Block; there is no Remove).
    ConfirmBlock,
}

/// Progress of adding a contact from a pasted card.
enum AddPhase {
    Idle,
    /// Dialling them since then ("Still trying…" after 2.5 s).
    Connecting(Instant),
    /// Their device holds it as a request: "Request sent. Your line opens when Tara accepts."
    Requested,
    Added(Contact),
    Failed(Reach),
}

/// Why an add didn't work, from the core's `AddError` kind (never its display text); the copy
/// is SIGNUP-HANDOFF.md § 6f.
#[derive(Debug, Clone, PartialEq)]
enum Reach {
    /// `AddError::Unreachable`: the dial or the answer timed out (their device is off or offline).
    Timeout,
    /// `AddError::Refused`: their device turned the code away (reset, replaced and expired, or used by us).
    Refused,
    /// `AddError::Blocked`: they block us.
    Blocked,
    /// This device is offline (checked before dialling, or `AddError::Offline`).
    Offline,
    /// Anything else the core reports; shown with the Timeout line (see SCREENS-PORT-STATUS.md).
    Other(String),
}

impl Reach {
    fn of(e: p2pcore::AddError) -> Reach {
        use p2pcore::AddError as A;
        match e {
            A::Unreachable => Reach::Timeout,
            A::Refused => Reach::Refused,
            A::Blocked => Reach::Blocked,
            A::Offline => Reach::Offline,
            e => Reach::Other(e.to_string()),
        }
    }
}

/// A finished call, shown for a few seconds.
struct EndedView {
    #[allow(dead_code, reason = "unwired by the UI port (b57ee23); UI owner to decide: wire back or delete (the ended card does not show why or direct/relayed)")]
    reason: String,
    did: String,
    name: String,
    text: String,
    #[allow(dead_code, reason = "unwired by the UI port (b57ee23); UI owner to decide: wire back or delete")]
    direct: bool,
    secs: u32,
    at: Instant,
}

struct CallView {
    info: CallInfo,
    state: CallState,
    answered: bool,
    stats: Option<CallStats>,
}

struct Qr {
    n: usize,
    dark: Vec<bool>,
}

struct App {
    node: Arc<Node>,
    win: Option<window::Id>,
    dark: bool,
    /// A test can pin the scheme; otherwise the app follows the OS.
    forced_dark: Option<bool>,
    /// What the system asks for (the last `Msg::Theme`), for Settings › Appearance › "Match my computer".
    sys_dark: bool,
    /// Test-hooks only: fake data for screenshots; the node is not asked for it.
    demo: bool,
    sel: Option<String>,
    detail: Detail,
    rename_in: String,
    search: String,
    shell_search: shell_search::State,
    profiles: profile_ui::State,
    /// Running transitions (nav highlight, fades); see `crate::motion`.
    fx: crate::motion::Fx,
    recents: Vec<CallRecord>,
    /// Group calls this device was in or missed (core `group_call_history`), newest first.
    group_log: Vec<p2pcore::GroupCallRecord>,
    detail_calls: Vec<CallRecord>,
    avail: Availability,
    avail_open: bool,
    add_phase: AddPhase,
    add_alias: String,
    settings_tab: u8,
    safety: Option<String>,
    ended: Option<EndedView>,
    screen: Screen,
    name_in: String,
    pass_in: Zeroizing<String>,
    old_in: Zeroizing<String>,
    /// Whether a passphrase is set; cached so `view` never calls into the node.
    has_pass: bool,
    /// Settings text about where the key lives when there is no passphrase.
    key_home: &'static str,
    /// The key still sits in the old plaintext `unlock.key` (existing installs): Settings warns.
    file_key: bool,
    /// Whether the OS keyring can keep a key. When it cannot, a passphrase is required.
    keyring_ok: bool,
    /// The "Remember on this computer" switch: Some(keyring available) once probed (None = still asking), and its state.
    remember_avail: Option<bool>,
    remember: bool,
    /// Every account on this device (clear data only), cached for `view`.
    accounts: Vec<AccountSummary>,
    acct_menu: bool,
    /// Account the user is being asked to confirm switching to.
    switch_ask: Option<String>,
    /// Account the user is being asked to confirm removing.
    remove_ask: Option<String>,
    /// The account that was current when "new account" / "restore" started; Cancel goes back to it.
    return_to: Option<String>,
    /// Restore found the account already on this computer.
    exists_hint: bool,
    /// An identity created/restored in this session that is still in onboarding.
    setup: Option<Setup>,
    device_in: String,
    device_label: String,
    device_edit: String,
    key_fail: Option<String>,
    revealed: Option<Zeroizing<String>>,
    /// When the phrase was revealed; it hides itself after `REVEAL_SECS`.
    revealed_at: Option<Instant>,
    reveal_form: bool,
    change_form: bool,
    restore: bool,
    phrase_in: Zeroizing<String>,
    show_phrase_in: bool,
    new_phrase: Option<Zeroizing<String>>,
    /// Cached so `view` never calls into the node.
    lock: LockState,
    profile_name: String,
    /// "You" (saved items for our own devices) as a contact-shaped row for the chat UI; never a real contact.
    me: Option<Contact>,
    busy: bool,
    notice: Option<String>,
    /// Bumped when a message is sent; the sidebar mark plucks its string on each change.
    mark_pluck: u64,
    status: NodeStatus,
    contacts: Vec<Contact>,
    /// Requests made with our code, waiting for our answer (Contacts › "Waiting for you").
    requests_in: Vec<p2pcore::ContactRequest>,
    /// Requests we sent, waiting for theirs (Contacts › "Waiting for them", the empty state).
    requests_out: Vec<p2pcore::ContactRequest>,
    /// "Not now" on a Contacts card: hidden for this session (the request stays in the core).
    hidden_requests: std::collections::HashSet<String>,
    ticket: Option<String>,
    /// When our code stops working (unix seconds): "It works until Mon 19 Oct."
    ticket_exp: Option<u64>,
    /// When "Copy card" was pressed; the button reads "Copied" for a moment after.
    copied_at: Option<Instant>,
    qr: Option<Qr>,
    add_in: String,
    call: Option<CallView>,
    call_ui: crate::call_ui::State,
    /// The call was placed with the video button: turn the camera on when the call finds one.
    video_intent: bool,
    /// A second incoming call, shown as a banner over the call; the call carries on until the user chooses.
    waiting: Option<CallInfo>,
    /// "End & answer" was chosen: this call takes over when the current one ends.
    takeover: Option<CallInfo>,
    early: Vec<(String, CallState)>,
    /// Call audio + ringer; driven by core events (see `audio::Ctl`), not by this loop.
    ctl: Arc<audio::Ctl>,
    settings: Settings,
    devices: (Vec<String>, Vec<String>),
    name_edit: String,
    ticks: u32,
    /// Test-hooks `P2P_SHOT_EVERY`: screenshots written so far.
    shots: u32,
    fetching: bool,
    chat: ChatState,
    /// The window has keyboard focus (false with no window).
    focused: bool,
    /// A contact card found on the clipboard, offered as "Add <name>?".
    offer: Option<p2pcore::CardPeek>,
    /// Tickets of cards the user said "Not now" to; not offered again by the clipboard watcher.
    dismissed: std::collections::HashSet<String>,
    link: LinkState,
    /// The account's devices (removed ones hidden), cached for `view`.
    linked: Vec<p2pcore::LinkedDevice>,
    /// Rename in progress: device key and the text so far.
    dev_rename: Option<(String, String)>,
    /// Devices.svelte: the device whose actions dialog is open.
    dev_menu: Option<String>,
    /// The group call on this computer, and the calls running in my groups.
    gcall: gcall::State,
    /// Privacy pages (privacy/*.svelte): their dialogs and fields.
    pv: Pv,
    /// Device the user is being asked to confirm unlinking.
    unlink_ask: Option<String>,
    /// This computer joined an account by linking and has not saved it yet.
    link_joined: bool,
    unlinked: Option<UnlinkedInfo>,
    /// Groups, hosting, blocking and permissions preview on placeholder data.
    #[cfg(any(test, feature = "test-hooks"))]
    groups: GroupsUi,
    /// Group chat on the core (G1): the Groups tab.
    live: GroupsLive,
    /// Real contact blocking, read from the core (`crate::blocks`).
    blocks: crate::blocks::Blocks,
    /// Kept, read-only chats of people who are no longer contacts (their block deleted our
    /// entry for them, U20), as contact-shaped rows for the chat screens; never real contacts.
    kept: Vec<Contact>,
    video: VideoUi,
    /// Sign-up (the stepped gate).
    gate: Gate,
    /// The add-contact dialog's tab, hint and found card.
    add_ui: AddUi,
}

/// Where a click on a notification should lead.
#[derive(Debug, Clone)]
pub enum Target {
    /// Just bring the window up (calls: the call screen is already what it shows).
    Show,
    /// The conversation with this contact.
    Chat(String),
}

#[derive(Debug, Clone)]
enum Msg {
    Ev(EvBox),
    Tick,
    /// A display frame while a transition runs.
    Frame(Instant),
    WindowOpened(#[allow(dead_code)] window::Id),
    CloseReq(window::Id),
    Closed(window::Id),
    Theme(theme::Mode),
    /// Settings › Appearance: None follows the system, Some(dark) is a choice.
    Appearance(Option<bool>),
    Quit,
    Exit,
    NameChanged(String),
    PassIn(String),
    OldIn(String),
    Create,
    Created(Result<String, String>),
    DeviceIn(String),
    DeviceGo,
    Finished(Result<(), SetupErr>),
    RetryKey,
    KeyFailPass,
    SwitchAsk(String),
    /// Switch without the sidebar confirmation (lock screens, where the choice is the click).
    SwitchNow(String),
    SwitchGo,
    Switched(Result<bool, String>),
    /// Start a new account (`true`: restore from a phrase).
    NewAccount(bool),
    NewAccountBegun(Result<(), String>, bool),
    CancelNew,
    DeviceEdit(String),
    SaveDevice,
    DeviceSaved(Result<String, String>),
    RemoveAsk(String),
    RemoveCancel,
    RemoveGo(String),
    Removed(Result<String, String>),
    KeyringProbed(bool),
    /// "Remember on this computer": (the keyring can keep a key, the current account has it on).
    RememberProbed(bool, bool),
    /// The switch on Unlock (applied after the unlock) or on Privacy > Passphrase (applied now).
    Remember(bool),
    RememberSet(Result<bool, String>),
    ToggleRestore,
    Devices((Vec<String>, Vec<String>)),
    Restore,
    /// Err carries (message, the account already exists here).
    Restored(Result<(), (String, bool)>),
    Unlock,
    Unlocked(Result<(), String>),
    GoRestore,
    ChangePassSubmit,
    PassChanged(Result<bool, String>),
    ToggleReveal,
    RevealSubmit,
    Revealed(Result<String, String>),
    ToggleChange,
    HidePhrase,
    CopyPhrase,
    PhraseSaved,
    Started(Result<(), String>),
    Ticket(Result<p2pcore::MyCode, String>),
    AddPressed,
    Added(Result<p2pcore::AddOutcome, Reach>),
    /// Accept on a contact request answered (the DID, the new contact).
    RequestAnswered(String, Result<Contact, String>),
    CallPressed(String),
    /// The conversation's video button: dial, and turn the camera on once the call has video.
    DialVideo(String),
    /// Settings > Notifications / Sounds.
    Alerts(Alerts),
    CallStarted(Result<CallInfo, String>),
    CallUi(crate::call_ui::Action),
    /// Block a contact (after the confirmation).
    Block(String),
    Unblock(String),
    /// "Allow calls" for a contact (P6).
    AllowCalls(String, bool),
    Done(Result<(), String>),
    Answer,
    Decline,
    DeclineWaiting,
    EndAnswer,
    Hangup,
    ToggleMute,
    OpenSettings,
    Back,
    Select(String),
    /// Back to the home pane: no contact selected.
    Home,
    SetDetail(Detail),
    RenameIn(String),
    RenameSave,
    RenameClear,
    SetVerified(bool),
    SearchIn(String),
    ShellSearch(shell_search::Action),
    Profile(profile_ui::Action),
    SetAvail(bool, Option<u64>),
    OpenAdd,
    SettingsTab(u8),
    Key(iced::keyboard::Key, iced::keyboard::Modifiers),
    #[allow(dead_code, reason = "unwired by the UI port (b57ee23); UI owner to decide: wire back or delete")]
    AddAlias(String),
    #[allow(dead_code, reason = "unwired by the UI port (b57ee23); UI owner to decide: wire back or delete")]
    SaveAlias,
    #[allow(dead_code, reason = "unwired by the UI port (b57ee23); UI owner to decide: wire back or delete")]
    CloseEnded,
    /// The ended call's hold ran out (only closes the ended view it was started for).
    EndedHoldDone(Option<Instant>),
    #[allow(dead_code, reason = "unwired by the UI port (b57ee23); UI owner to decide: wire back or delete")]
    CallAgain(String),
    CopySafety,
    NameEdit(String),
    SaveName,
    InDev(String),
    OutDev(String),
    ToneToggled(bool),
    Chat(Cm),
    /// The window gained or lost keyboard focus.
    Focus(window::Id, bool),
    Scale(f32),
    ClipText(bool, Option<String>),
    OfferAdd,
    OfferDismiss,
    LinkStartNew,
    LinkStartExisting,
    NewLink,
    NewLinkBegun(Result<(), String>),
    LinkNameGo,
    LinkQr(Result<String, String>),
    LinkRegen,
    LinkPasteToggle,
    LinkPasteIn(String),
    LinkPasteGo,
    LinkScanned(Result<(), String>),
    LinkPassIn(String),
    LinkApprove,
    LinkApproved(Result<(), String>),
    LinkCancel,
    LinkDoneOk,
    LinkSecureGo,
    LinkSyncDone,
    DevRenameStart(String),
    /// Open (Some) or close a device's actions dialog.
    DevMenu(Option<String>),
    Privacy(PvMsg),
    GCall(gcall::Gm),
    DevRenameIn(String),
    DevRenameSave,
    DevRenameCancel,
    DevDone(Result<(), String>),
    UnlinkAsk(String),
    UnlinkCancel,
    UnlinkGo,
    /// Ok((this device, DID)).
    Unlinked(Result<(bool, String), String>),
    UnlinkedOk,
    /// Test-hooks: the window's pixels, written to P2P_SHOT.
    Shot(window::Screenshot),
    /// Test-hooks: `P2P_SHOT_EVERY=dir` writes the window to dir/NNN.png twice a second (demos of live state).
    ShotEvery,
    #[cfg(any(test, feature = "test-hooks"))]
    Groups(Gm),
    Live(Lm),
    Video(Vm),
    Gate(Gs),
}

/// `Ev` carries non-Clone data from the core; Debug/Clone for the message type go through here.
struct EvBox(std::sync::Mutex<Option<Ev>>);
impl Clone for EvBox {
    fn clone(&self) -> Self {
        EvBox(std::sync::Mutex::new(None))
    }
}
impl std::fmt::Debug for EvBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ev")
    }
}

fn events() -> impl iced::futures::Stream<Item = Msg> {
    use iced::futures::SinkExt;
    iced::stream::channel(64, async |mut out| {
        let rx = crate::EV_RX.get().and_then(|m| m.lock().unwrap().take());
        let Some(mut rx) = rx else { return };
        while let Some(ev) = rx.recv().await {
            let _ = out.send(Msg::Ev(EvBox(std::sync::Mutex::new(Some(ev))))).await;
        }
    })
}

fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
    m: impl Fn(T) -> Msg + Send + 'static,
) -> Task<Msg> {
    Task::perform(async move { tokio::task::spawn_blocking(f).await.ok() }, move |r| match r {
        Some(v) => m(v),
        None => Msg::Done(Err("task failed".into())),
    })
}

fn settings_path() -> std::path::PathBuf {
    INIT.get().unwrap().data.join("desktop-settings.json")
}

/// Moves a secret out of its field (no copy left behind) to hand to the core.
fn take_secret(z: &mut Zeroizing<String>) -> String {
    std::mem::take(&mut **z)
}

/// Widget id of the rename field, so it can take focus.
const RENAME_ID: &str = "rename";
const SEARCH_ID: &str = "search";

/// Privacy pages' local state.
#[derive(Default)]
pub(crate) struct Pv {
    /// Blocked: the "Block someone" picker is open.
    pub pick: bool,
    /// Blocked: "Unblock {name}?" is open for this DID.
    pub unblock: Option<String>,
    /// Recovery phrase: the words are showing (they hide again after 30 s).
    pub shown: bool,
    pub shown_at: Option<Instant>,
    /// Recovery phrase page: show the recovery key (one line) instead of the words.
    pub as_key: bool,
    /// Passphrase: the repeat field, the three field errors, and "Passphrase changed".
    pub again: Zeroizing<String>,
    pub errs: [String; 3],
    pub done: bool,
    /// PassField's eye: the passphrase fields on these pages show their text.
    pub reveal: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum PvMsg {
    Pick(bool),
    PickPerson(String),
    Unblock(Option<String>),
    UnblockGo,
    Words(bool),
    /// Words | Key on the recovery phrase page.
    AsKey(bool),
    /// Copy the recovery key (only the key has a copy button: it is meant for a password manager).
    CopyKey,
    Again(String),
    Change,
    Eye,
}

/// Settings > Notifications / Sounds changes.
#[derive(Debug, Clone)]
pub(crate) enum Alerts {
    Quiet(bool),
    Palette(crate::cues::Palette),
    /// Play a call cue once in the current sound set (Sounds' preview chips).
    Preview(&'static str),
    Messages(bool),
    Calls(bool),
    Groups(bool),
}

/// Settings > Notifications: alert me about messages (from people), calls, groups. Read by every notify site.
pub(crate) static NOTIFY_MESSAGES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
pub(crate) static NOTIFY_GROUPS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// A desktop notification, off the UI thread (some servers block on show). Clicking it (the
/// "default" action, Linux) sends `target` back into the app.
fn notify(summary: &str, body: &str, target: Target) {
    let (summary, body) = (summary.to_string(), body.to_string());
    std::thread::spawn(move || {
        let mut n = notify_rust::Notification::new();
        n.appname("Tinline").summary(&summary).body(&body);
        // Windows toasts are attributed to an AppUserModelID; an installer should register
        // a shortcut with the same id so they show under the app's name and icon.
        #[cfg(windows)]
        n.app_id("com.osvauld.tinline");
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            n.action("default", "Open");
            if let Ok(handle) = n.show() {
                // Blocks until the notification is clicked or closed; this thread is its own.
                handle.wait_for_action(|action| {
                    if action == "default"
                        && let Some(init) = INIT.get()
                    {
                        let _ = init.tx.send(Ev::Open(target));
                    }
                });
            }
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        {
            let _ = target;
            let _ = n.show();
        }
    });
}

fn s<E: ToString>(e: E) -> String {
    e.to_string()
}

/// Plain-language text for errors the passphrase screens can hit. Never includes the passphrase.
fn friendly(e: Error) -> String {
    match e {
        Error::WrongPassphrase => "That passphrase didn\u{2019}t open the account.".into(),
        Error::WeakPassphrase => "Type a passphrase first.".into(),
        Error::BadPhrase => "That phrase isn\u{2019}t valid. Check each word and their order.".into(),
        Error::Locked => "Unlock first.".into(),
        Error::NotStarted => "Tinline is still starting. Try again in a moment.".into(),
        Error::HaveIdentity => "An identity already exists here.".into(),
        Error::AccountExists(_) => "That account is already on this computer.".into(),
        Error::InCall => "You\u{2019}re on a call. Hang up before switching accounts.".into(),
        e => e.to_string(),
    }
}

/// Why a passphrase is required on a computer without a secure keyring.
const NO_KEYRING: &str = "This computer has no secure keyring, so Tinline needs a passphrase to protect your account.";

/// What Settings says about the stored key; empty with a passphrase (nothing is stored then).
fn key_home(node: &Node, has_pass: bool) -> (&'static str, bool) {
    if has_pass {
        return ("", false);
    }
    let Some(did) = keystore::current_did(node) else { return ("", false) };
    match keystore::location(keystore::system(), &INIT.get().unwrap().data, &did) {
        Some(keystore::Where::Keyring) => ("Your key is kept in this computer\u{2019}s keyring, so Tinline opens without asking.", false),
        Some(keystore::Where::File) => (
            "Your key is kept in a plain file in Tinline\u{2019}s data folder. Anyone who copies that folder can open your account. Add a passphrase to protect it; the file is then deleted.",
            true,
        ),
        None => ("", false),
    }
}

impl App {
    fn refresh_pass(&mut self) {
        self.has_pass = self.node.has_passphrase();
        (self.key_home, self.file_key) = key_home(&self.node, self.has_pass);
    }

    /// Re-reads what the account UI shows (accounts list, device name, passphrase state).
    fn refresh_accounts(&mut self) {
        if self.demo {
            return;
        }
        self.accounts = self.node.accounts();
        self.device_label = self.node.device_label().unwrap_or_default();
    }

    /// Everything tied to the account that was in memory is dropped when another one is selected.
    fn reset_account_state(&mut self) {
        self.leave_chat();
        let tab = self.chat.tab;
        #[cfg(feature = "test-hooks")]
        let fake = self.chat.fake.take();
        let account_epoch = self.chat.account_epoch.checked_add(1).expect("chat account epoch exhausted");
        self.chat = ChatState::default();
        self.chat.account_epoch = account_epoch;
        self.chat.tab = tab;
        self.profiles = profile_ui::State::default();
        self.install_profile_events();
        #[cfg(feature = "test-hooks")]
        {
            self.chat.fake = fake;
        }
        self.sel = None;
        self.detail = Detail::View;
        self.search.clear();
        self.safety = None;
        self.ended = None;
        self.waiting = None;
        self.takeover = None;
        self.detail_calls.clear();
        self.add_phase = AddPhase::Idle;
        self.add_ui = AddUi::default();
        self.requests_in.clear();
        self.requests_out.clear();
        self.ticket = None;
        self.ticket_exp = None;
        self.qr = None;
        self.fetching = false;
        self.avail_open = false;
        self.acct_menu = false;
        self.switch_ask = None;
        self.remove_ask = None;
        self.hide_phrase();
        self.change_form = false;
        self.pass_in.zeroize();
        self.old_in.zeroize();
        self.dev_rename = None;
        self.unlink_ask = None;
        self.linked.clear();
        self.refresh_pass();
        self.refresh_identity();
        self.refresh_accounts();
        self.refresh_linked();
        #[cfg(any(test, feature = "test-hooks"))]
        {
            let preview_on = self.groups.on;
            self.groups.enable(preview_on, &self.profile_name);
        }
        self.reload_contacts();
        self.recents = self.node.recent_calls(RECENTS);
        self.group_log = self.node.group_call_history(None).unwrap_or_default();
        self.avail = self.node.availability();
        self.name_edit = self.profile_name.clone();
        self.device_edit = self.device_label.clone();
    }

    /// Identity exists (in memory until the key is saved): ask for this computer's name.
    fn begin_name_device(&mut self, phrase: Option<Zeroizing<String>>) {
        self.setup = Some(Setup { phrase });
        if self.device_in.trim().is_empty() {
            self.device_in = flow::suggest_label();
        }
        self.notice = None;
        self.screen = Screen::NameDevice;
    }

    /// Asks the keyring whether it can keep a key and whether "Remember on this computer" is on for the current account.
    fn probe_remember(&self) -> Task<Msg> {
        let data = INIT.get().unwrap().data.clone();
        let did = keystore::current_did(&self.node).unwrap_or_default();
        blocking(move || (keystore::available(keystore::system(), &data), keystore::remembered(keystore::system(), &data, &did)), |(ok, on)| Msg::RememberProbed(ok, on))
    }

    fn probe_keyring(&self) -> Task<Msg> {
        let data = INIT.get().unwrap().data.clone();
        blocking(move || keystore::available(keystore::system(), &data), Msg::KeyringProbed)
    }

    fn boot() -> (App, Task<Msg>) {
        let init = INIT.get().expect("init");
        let node = init.node.clone();
        let settings: Settings = std::fs::read(settings_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let has = node.has_identity();
        let lock = node.lock_state();
        let mut settings_devices = false;
        let test_unlock = init.test_pass.clone().filter(|_| lock == LockState::Locked);
        let forced_dark = crate::test_env("P2P_DARK").map(|v| v == "1");
        settings.apply_alerts();
        #[cfg(any(test, feature = "test-hooks"))]
        let preview_on = settings.groups_preview.unwrap_or(groups::PREVIEW_DEFAULT);
        #[cfg(any(test, feature = "test-hooks"))]
        let me_name = node.profile().map(|p| p.name).unwrap_or_default();
        let mut app = App {
            win: None,
            dark: forced_dark.or(settings.appearance).unwrap_or(false),
            forced_dark,
            sys_dark: false,
            demo: false,
            sel: None,
            detail: Detail::View,
            rename_in: String::new(),
            search: String::new(),
            shell_search: shell_search::State::default(),
            profiles: profile_ui::State::default(),
            fx: Default::default(),
            recents: node.recent_calls(RECENTS),
            group_log: node.group_call_history(None).unwrap_or_default(),
            detail_calls: Vec::new(),
            avail: node.availability(),
            avail_open: false,
            add_phase: AddPhase::Idle,
            add_alias: String::new(),
            settings_tab: 0,
            safety: None,
            ended: None,
            screen: match lock {
                LockState::NoIdentity => Screen::Onboarding,
                LockState::Locked if test_unlock.is_none() => Screen::Unlock,
                _ => Screen::Home,
            },
            name_in: String::new(),
            pass_in: Zeroizing::default(),
            old_in: Zeroizing::default(),
            has_pass: node.has_passphrase(),
            key_home: key_home(&node, node.has_passphrase()).0,
            file_key: key_home(&node, node.has_passphrase()).1,
            keyring_ok: true,
            remember_avail: None,
            remember: false,
            accounts: node.accounts(),
            acct_menu: false,
            switch_ask: None,
            remove_ask: None,
            return_to: None,
            exists_hint: false,
            setup: None,
            device_in: String::new(),
            device_label: node.device_label().unwrap_or_default(),
            device_edit: node.device_label().unwrap_or_default(),
            key_fail: None,
            revealed: None,
            revealed_at: None,
            reveal_form: false,
            change_form: false,
            restore: false,
            phrase_in: Zeroizing::default(),
            show_phrase_in: false,
            new_phrase: None,
            lock,
            profile_name: node.profile().map(|p| p.name).unwrap_or_default(),
            me: node.profile().map(|p| self_contact(&p.did)),
            busy: false,
            notice: None,
            mark_pluck: 0,
            status: node.status(),
            contacts: node.contacts(),
            requests_in: node.contact_requests(),
            requests_out: node.sent_contact_requests(),
            hidden_requests: Default::default(),
            ticket: None,
            ticket_exp: None,
            copied_at: None,
            qr: None,
            add_in: String::new(),
            call: None,
            call_ui: crate::call_ui::State::default(),
            video_intent: false,
            waiting: None,
            takeover: None,
            early: Vec::new(),
            ctl: INIT.get().unwrap().audio.clone(),
            settings,
            devices: (Vec::new(), Vec::new()),
            name_edit: node.profile().map(|p| p.name).unwrap_or_default(),
            ticks: 0,
            shots: 0,
            fetching: false,
            chat: ChatState::default(),
            focused: false,
            offer: None,
            dismissed: Default::default(),
            link: LinkState::new(Role::New),
            linked: Vec::new(),
            dev_rename: None,
            dev_menu: None,
            gcall: gcall::State::default(),
            pv: Pv::default(),
            unlink_ask: None,
            link_joined: false,
            unlinked: None,
            #[cfg(any(test, feature = "test-hooks"))]
            groups: GroupsUi::new(preview_on, &me_name),
            live: GroupsLive::default(),
            blocks: Default::default(),
            kept: Vec::new(),
            video: VideoUi::default(),
            gate: Gate::default(),
            add_ui: AddUi::default(),
            node,
        };
        if has && crate::test_env("P2P_SCREEN").is_some_and(|v| v == "settings") {
            app.screen = Screen::Settings;
            settings_devices = true;
        }
        app.install_profile_events();
        *app.ctl.devices.lock().unwrap() = (app.settings.input.clone(), app.settings.output.clone());
        *app.ctl.tone.lock().unwrap() = app.tone_hz();
        app.node.set_video_enabled(!app.settings.no_video);
        let mut tasks = vec![
            system::theme().map(Msg::Theme),
        ];
        if settings_devices {
            tasks.push(blocking(audio::list_devices, Msg::Devices));
        }
        if !init.hidden {
            tasks.push(app.open_window(false));
        }
        if app.screen == Screen::Onboarding {
            tasks.push(app.probe_keyring());
        }
        if app.screen == Screen::Unlock && app.has_pass && !app.demo {
            tasks.push(app.probe_remember());
        }
        #[cfg(feature = "test-hooks")]
        if crate::test_env("P2P_CHAT_FAKE").is_some_and(|v| v == "1") {
            app.chat.fake = Some(Arc::new(chat::chat_fake::Fake::new(Some(init.tx.clone()))));
        }
        if let Some(name) = crate::test_env("P2P_DEMO") {
            app.load_demo(&name);
            // pg-chat-*: load the open conversation like a click would.
            if name.starts_with("pg-chat-") && let Some(d) = app.sel.clone() {
                tasks.push(app.open_chat(&d));
                // pg-chat-<id>-find / -reply: the find bar open on "landlord", or replying to their newest message.
                if name.ends_with("-find") { app.chat.find = Some("landlord".into()); }
                if name.ends_with("-reply") { app.chat.reply = Some(format!("{}-13", &name["pg-chat-".len()..name.len() - "-reply".len()])); }
            }
            tasks.push(app.ensure_thumbs());
            tasks.push(app.gate_focus());
            if let Some(peer) = app.chat.peer.clone() {
                match name.as_str() {
                    "chat-viewer" => tasks.push(app.update_chat(Cm::View(format!("{peer}-12")))),
                    "chat-text" => tasks.push(app.update_chat(Cm::Open(format!("{peer}-14")))),
                    _ => {}
                }
            }
            return (app, Task::batch(tasks));
        }
        if let Some(pass) = test_unlock {
            app.busy = true;
            let node = app.node.clone();
            tasks.push(blocking(move || node.unlock(pass).map_err(friendly), Msg::Unlocked));
        } else if has && lock != LockState::Locked {
            // Locked identities stay offline until the passphrase is entered.
            tasks.push(app.start_node());
        }
        tasks.push(app.refresh_profiles());
        (app, Task::batch(tasks))
    }

    /// Test-hooks: `P2P_OPEN=calls|contacts|chat:<contact name>` picks the screen after unlock.
    #[cfg(feature = "test-hooks")]
    fn test_open(&mut self, v: &str) -> Task<Msg> {
        match v {
            "calls" => self.update_chat(Cm::Tab(chat::SideTab::Calls)),
            "contacts" => self.update_chat(Cm::Tab(chat::SideTab::Contacts)),
            _ => {
                let did = v.strip_prefix("chat:").and_then(|n| match n {
                    "You" => self.me.as_ref().map(|m| m.did.clone()),
                    _ => self.node.contacts().into_iter().find(|c| c.name == n).map(|c| c.did),
                });
                match did {
                    Some(did) => Task::batch([self.update_chat(Cm::Tab(chat::SideTab::Chats)), self.update(Msg::Select(did))]),
                    None => Task::none(),
                }
            }
        }
    }

    /// Brings the window up for the user: opens it if closed, un-minimises, and asks the
    /// compositor to put it in front of them (see `crate::raise`).
    fn show_window(&mut self) -> Task<Msg> {
        crate::raise::to_user();
        self.open_window(true)
    }

    fn open_window(&mut self, attention: bool) -> Task<Msg> {
        let ask = move |id| if attention { window::request_user_attention(id, Some(window::UserAttention::Informational)) } else { Task::none() };
        if let Some(id) = self.win {
            return Task::batch([
                window::minimize(id, false),
                window::gain_focus(id),
                ask(id),
            ]);
        }
        let (id, task) = window::open(window::Settings {
            size: WINDOW,
            min_size: Some(Size::new(820.0, 560.0)),
            icon: crate::tray::window_icon(),
            ..Default::default()
        });
        self.win = Some(id);
        Task::batch([
            task.map(Msg::WindowOpened),
            ask(id),
        ])
    }

    /// Whether the home screen is what the user is looking at, so a card offer makes sense.
    fn can_offer(&self) -> bool {
        self.win.is_some()
            && !self.demo
            && self.call.is_none()
            && self.ended.is_none()
            && self.screen == Screen::Home
            && self.lock != LockState::Locked
            && self.lock != LockState::NoIdentity
    }

    fn check_clipboard(&self, explicit: bool) -> Task<Msg> {
        if !explicit && !self.can_offer() {
            return Task::none();
        }
        clipboard::read().map(move |t| Msg::ClipText(explicit, t))
    }

    fn start_node(&self) -> Task<Msg> {
        let node = self.node.clone();
        blocking(move || node.start().map_err(s), Msg::Started)
    }

    fn fetch_ticket(&mut self) -> Task<Msg> {
        self.fetching = true;
        let node = self.node.clone();
        blocking(move || node.my_code().map_err(s), Msg::Ticket)
    }

    /// Writes the settings on a helper thread (tmp file + rename, so a crash never leaves half a
    /// file). The newest snapshot always wins even if two saves race.
    fn save_settings(&self) {
        static PENDING: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);
        static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        self.settings.apply_alerts();
        let Ok(b) = serde_json::to_vec_pretty(&self.settings) else { return };
        *PENDING.lock().unwrap() = Some(b);
        let path = settings_path();
        std::thread::spawn(move || {
            let _w = WRITE.lock().unwrap();
            let Some(b) = PENDING.lock().unwrap().take() else { return };
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, b).is_ok() && std::fs::rename(&tmp, &path).is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
        });
    }

    /// Refreshes what `view` shows about the identity; the node is only asked here, never per redraw.
    fn refresh_identity(&mut self) {
        if self.demo {
            return;
        }
        self.lock = self.node.lock_state();
        self.profile_name = self.node.profile().map(|p| p.name).unwrap_or_default();
        self.me = self.node.profile().map(|p| self_contact(&p.did));
        self.sync_live();
    }

    /// The "You" conversation: our own DID.
    pub(super) fn is_me(&self, did: &str) -> bool {
        self.me.as_ref().is_some_and(|m| m.did == did)
    }

    fn hide_phrase(&mut self) {
        self.revealed = None;
        self.revealed_at = None;
        self.reveal_form = false;
        self.pv.shown = false;
        self.pv.shown_at = None;
    }

    fn tone_hz(&self) -> Option<f32> {
        INIT.get().unwrap().env_tone.or(self.settings.tone.then_some(440.0))
    }

    fn apply_call_state(&mut self, id: &str, state: CallState) -> Task<Msg> {
        let Some(c) = self.call.as_mut().filter(|c| c.info.call_id == id) else { return Task::none() };
        c.state = state.clone();
        match state {
            CallState::Active => {
                c.answered = true;
                // The in-call screen has device pickers.
                if self.devices.0.is_empty() && !self.demo {
                    return blocking(audio::list_devices, Msg::Devices);
                }
            }
            CallState::Ended { reason } => {
                let who = c.info.peer_name.clone();
                let (direct, secs) = c.stats.as_ref().map(|s| (s.direct, s.secs)).unwrap_or((false, 0));
                let did = c.info.peer_did.clone();
                let mut presentation = reason::present(&reason, &who, c.info.incoming, c.answered);
                // "blocked" ends the call on both sides: say which side blocked. "calls not
                // allowed" turns their call token off here.
                if reason == p2pcore::CALLS_NOT_ALLOWED {
                    self.refresh_blocks();
                }
                if reason == p2pcore::BLOCKED {
                    self.refresh_blocks();
                    if self.blocks.blocked_by_me(&did) {
                        presentation = End::Screen(format!("You blocked {}", crate::blocks::first_name(&who)));
                    }
                }
                self.call = None;
                self.video_reset();
                // End & answer: straight on to the new call, no "call ended" screen in between.
                if let Some(next) = self.takeover.take() {
                    self.call = Some(CallView { info: next, state: CallState::Ringing, answered: false, stats: None });
                    self.refresh_history();
                    return Task::none();
                }
                self.ctl.muted.store(false, Ordering::Relaxed);
                // The core logs the call before it says Ended, so the lists can be re-read now.
                self.refresh_history();
                match presentation {
                    End::Hidden => {}
                    End::Missed(text) => {
                        if crate::audio::RING_CALLS.load(Ordering::Relaxed) { notify("Tinline", &text, Target::Show); }
                        self.notice = Some(text);
                    }
                    // call.svelte.ts #finish: my own hang-up leaves at once; otherwise the call screen holds the
                    // reason for a moment (0.9 s, or 2.6 s when it couldn't connect), then goes.
                    End::Screen(_) if reason == "hangup_local" => {}
                    End::Screen(text) => {
                        let failed = matches!(reason.as_str(), "unreachable" | "connection_lost" | "failed");
                        let hold = if failed { 2600 } else { 900 };
                        self.ended = Some(EndedView { reason: reason.clone(), did, name: who, text, direct, secs, at: Instant::now() });
                        let at = self.ended.as_ref().map(|e| e.at);
                        return Task::perform(tokio::time::sleep(Duration::from_millis(hold)), move |_| Msg::EndedHoldDone(at));
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn refresh_history(&mut self) {
        if self.demo {
            return;
        }
        self.recents = self.node.recent_calls(RECENTS);
        self.group_log = self.node.group_call_history(None).unwrap_or_default();
        self.avail = self.node.availability();
        self.refresh_detail_calls();
    }

    fn refresh_detail_calls(&mut self) {
        if self.demo {
            return;
        }
        self.detail_calls = match &self.sel {
            Some(did) => self.node.calls_with(did.clone(), 30),
            None => Vec::new(),
        };
    }

    /// The name to show for a contact: ours if we gave one, else theirs.
    fn display(c: &Contact) -> String {
        c.alias.clone().filter(|a| !a.is_empty()).unwrap_or_else(|| c.name.clone())
    }

    fn selected(&self) -> Option<&Contact> {
        let did = self.sel.as_ref()?;
        self.contacts
            .iter()
            .find(|c| &c.did == did)
            .or_else(|| self.me.as_ref().filter(|m| &m.did == did))
            .or_else(|| self.kept.iter().find(|c| &c.did == did))
    }

    fn is_contact(&self, did: &str) -> bool {
        self.contacts.iter().any(|c| c.did == did)
    }

    /// Whose conversation can be opened: contacts, "You", and kept chats.
    pub(super) fn selectable(&self, did: &str) -> bool {
        self.is_contact(did) || self.is_me(did) || self.kept.iter().any(|c| c.did == did)
    }

    /// Contacts from the core, then everything that depends on them.
    fn reload_contacts(&mut self) {
        self.contacts = self.node.contacts();
        if !self.demo {
            self.requests_in = self.node.contact_requests();
            self.requests_out = self.node.sent_contact_requests();
        }
        self.refresh_blocks();
    }

    /// Re-reads block states for every conversation we show, the kept chats, and why our
    /// pending messages in the open conversation are not delivered. Cheap core reads.
    pub(super) fn refresh_blocks(&mut self) {
        if self.demo {
            return;
        }
        let mut dids: Vec<String> = self.contacts.iter().map(|c| c.did.clone()).collect();
        let more: Vec<String> = self.chat.chats.iter().map(|c| c.peer_did.clone()).filter(|d| !self.is_me(d) && !dids.contains(d)).collect();
        dids.extend(more);
        self.blocks.refresh(&*self.node, dids);
        self.kept = self
            .chat
            .chats
            .iter()
            .filter(|c| !self.is_me(&c.peer_did) && !self.is_contact(&c.peer_did))
            .map(|c| Contact {
                did: c.peer_did.clone(),
                name: c.peer_name.clone(),
                device: String::new(),
                added_at: 0,
                alias: None,
                verified: false,
                lost_touch: false,
            })
            .collect();
        self.refresh_undelivered();
    }

    /// Why our pending messages in the open conversation are not delivered.
    pub(super) fn refresh_undelivered(&mut self) {
        if self.demo {
            return;
        }
        let Some(peer) = self.chat.peer.clone() else { return };
        let pending: Vec<&str> = self
            .chat
            .msgs
            .iter()
            .filter(|m| m.outgoing && m.delivery == p2pcore::DeliveryState::Pending)
            .map(|m| m.id.as_str())
            .collect();
        self.blocks.refresh_undelivered(&*self.node, &peer, pending);
    }

    fn rename(&mut self, clear: bool) -> Task<Msg> {
        let Some(did) = self.sel.clone() else { return Task::none() };
        let alias = if clear { None } else { Some(self.rename_in.trim().to_string()) };
        let node = self.node.clone();
        self.detail = Detail::View;
        blocking(move || node.rename_contact(did, alias).map_err(s), Msg::Done)
    }

    /// Starts a call (ignored while one is on) to `did`.
    fn dial(&mut self, did: String) -> Task<Msg> {
        if self.call.is_some() {
            return Task::none();
        }
        use crate::blocks::{CallGate, Composer};
        let gate = if self.demo { CallGate::Ok } else { self.blocks.call_gate(&did, self.is_contact(&did)) };
        if gate != CallGate::Ok {
            let name = self.chat_name(&did);
            self.notice = Some(match gate {
                CallGate::Closed(Composer::BlockedByMe) => format!("You blocked {name}. Unblock to call."),
                CallGate::Closed(Composer::BlockedMe) => format!("Can\u{2019}t call: {} blocked you", crate::blocks::first_name(&name)),
                CallGate::NotAllowed => crate::blocks::calls_not_allowed_text(&name),
                _ => format!("{name} is no longer in your contacts."),
            });
            return Task::none();
        }
        self.notice = None;
        self.ended = None;
        let node = self.node.clone();
        blocking(move || node.call(did).map_err(s), Msg::CallStarted)
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        if let Msg::Frame(now) = msg {
            self.fx.now = now;
            return Task::none();
        }
        let identity_may_change = matches!(
            msg,
            Msg::Tick
                | Msg::Ev(_)
                | Msg::Created(_)
                | Msg::Restored(_)
                | Msg::Unlocked(_)
                | Msg::PassChanged(_)
                | Msg::Started(_)
                | Msg::Switched(_)
                | Msg::Finished(_)
                | Msg::Ticket(_)
        );
        let account = self.accounts.iter().find(|a| a.current && self.lock == LockState::Unlocked).map(|a| a.did.as_str());
        if self.shell_search.bind(account) { self.search.clear(); }
        // Serialise UI account mutations with profile writes; reads still reject late epochs.
        if self.profiles.pending && matches!(msg, Msg::SwitchGo | Msg::SwitchNow(_) | Msg::NewAccount(_) | Msg::NewLink | Msg::GoRestore | Msg::RemoveGo(_)) {
            self.notice = Some("Wait for the profile save to finish.".into());
            return Task::none();
        }
        let old_owner = self.profile_owner();
        let task = self.update_inner(msg);
        self.settle_motion();
        self.call_ui.bind(self.call.as_ref().map(|c| c.info.call_id.as_str()));
        if identity_may_change {
            self.refresh_identity();
        }
        if old_owner != self.profile_owner() {
            // Invalidate profile presentation even if the owner changes before
            // its account-worker completion. Keep unrelated navigation intact.
            self.chat.account_epoch = self.chat.account_epoch.checked_add(1).expect("account epoch exhausted");
            self.profiles = profile_ui::State::default();
            self.install_profile_events();
            return Task::batch([task, self.refresh_profiles()]);
        }
        task
    }

    fn update_inner(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Frame(now) => self.fx.now = now,
            Msg::Ev(b) => {
                let Some(ev) = b.0.lock().unwrap().take() else { return Task::none() };
                return self.on_event(ev);
            }
            Msg::Tick => {
                if let (Some(_), Some(id)) = (crate::test_env("P2P_SHOT"), self.win) {
                    self.ticks += 1;
                    // Pictures decode in the background: wait for them (at most ~30 ticks).
                    let busy = self.chat.thumbs.values().any(|t| matches!(t, chat::Thumb::Loading))
                        || self.chat.viewer.as_ref().is_some_and(|v| matches!(v.body, chat::ViewBody::Loading));
                    let after = crate::test_env("P2P_SHOT_AFTER").and_then(|v| v.parse().ok()).unwrap_or(3);
                    if self.ticks >= after && (!busy || self.ticks >= after + 30) {
                        return window::screenshot(id).map(Msg::Shot);
                    }
                }
                if self.demo {
                    return Task::none();
                }
                self.ticks += 1;
                self.status = self.node.status();
                self.lock = self.node.lock_state();
                // `until` switches availability back on inside the core; follow it.
                self.avail = self.node.availability();
                if self.revealed_at.is_some_and(|t| t.elapsed() >= Duration::from_secs(REVEAL_SECS)) {
                    self.hide_phrase();
                }
                // privacy/Phrase.svelte: the words hide again after 30 s.
                if self.pv.shown_at.is_some_and(|t| t.elapsed() >= Duration::from_secs(30)) {
                    self.pv.shown = false;
                    self.pv.shown_at = None;
                }
                if self.ended.as_ref().is_some_and(|e| e.at.elapsed() >= Duration::from_secs(ENDED_SECS)) {
                    self.ended = None;
                }
                crate::tray::set_status(match self.lock {
                    LockState::Locked => "Locked",
                    _ if !self.status.started => "Starting...",
                    _ if self.status.online && !self.avail.available => "Not available",
                    _ if self.status.online => "Available",
                    _ => "Offline",
                });
                let in_call = self.call.as_ref().is_some_and(|c| c.state == CallState::Active);
                crate::tray::set_state(if in_call {
                    crate::tray::State::InCall
                } else if self.status.online && self.avail.available && self.lock != LockState::Locked {
                    crate::tray::State::Available
                } else {
                    crate::tray::State::Unavailable
                });
                crate::tray::set_dark(self.dark);
                if let Some(c) = self.call.as_mut() {
                    c.stats = self.node.call_stats();
                    if let Some(st) = &c.stats { self.call_ui.loss.push(st.received, st.lost); }
                }
                let auto = self.auto_call();
                return Task::batch([self.link_drive(), auto]);
            }
            Msg::WindowOpened(id) => return window::scale_factor(id).map(Msg::Scale),
            Msg::Scale(f) => ui::set_scale(f),
            Msg::CloseReq(id) => {
                if Some(id) == self.win {
                    self.hide_phrase();
                    if INIT.get().unwrap().tray {
                        self.win = None;
                        self.focused = false;
                        return window::close(id);
                    }
                    return window::minimize(id, true);
                }
            }
            Msg::Closed(id) => {
                if Some(id) == self.win {
                    self.hide_phrase();
                    self.win = None;
                    self.focused = false;
                }
            }
            Msg::Theme(m) => {
                self.sys_dark = m != theme::Mode::Light;
                self.dark = self.forced_dark.or(self.settings.appearance).unwrap_or(self.sys_dark);
            }
            Msg::Appearance(v) => {
                self.settings.appearance = v;
                self.dark = self.forced_dark.or(v).unwrap_or(self.sys_dark);
                self.save_settings();
            }
            Msg::Gate(g) => return self.gate_update(g),
            Msg::Quit => {
                chat::wipe_voice_dir();
                crate::media::wipe_now();
                let node = self.node.clone();
                self.ctl.stop_all();
                // A hung shutdown must not keep the app (and its tray icon) alive.
                return Task::perform(
                    async move {
                        let stop = tokio::task::spawn_blocking(move || node.stop());
                        let _ = tokio::time::timeout(Duration::from_secs(3), stop).await;
                    },
                    |_| Msg::Exit,
                );
            }
            Msg::Exit => return iced::exit(),
            Msg::NameChanged(v) => self.name_in = v,
            Msg::Create => {
                let name = self.name_in.trim().to_string();
                if name.is_empty() {
                    self.notice = Some("Enter a name first".into());
                    return Task::none();
                }
                if self.busy {
                    return Task::none();
                }
                if self.pass_in.is_empty() && !self.keyring_ok {
                    self.notice = Some(NO_KEYRING.into());
                    return Task::none();
                }
                self.busy = true;
                self.notice = None;
                // An empty passphrase means none: the key then lives in the keyring.
                let pass = take_secret(&mut self.pass_in);
                let node = self.node.clone();
                return blocking(move || node.create_identity(name, pass).map_err(friendly), Msg::Created);
            }
            Msg::PassIn(v) => self.pass_in = Zeroizing::new(v),
            Msg::OldIn(v) => self.old_in = Zeroizing::new(v),
            Msg::Created(r) => {
                self.busy = false;
                self.notice = None;
                match r {
                    Ok(p) => {
                        self.refresh_pass();
                        self.name_edit = self.name_in.trim().to_string();
                        return self.gate_created(p);
                    }
                    Err(e) => self.notice = Some(e),
                }
            }
            Msg::DeviceIn(v) => self.device_in = v,
            Msg::DeviceGo => {
                if self.busy {
                    return Task::none();
                }
                let Some(label) = flow::valid_label(&self.device_in) else {
                    self.notice = Some("Give this computer a name of 1 to 64 characters.".into());
                    return Task::none();
                };
                self.busy = true;
                self.notice = None;
                let (node, data) = (self.node.clone(), INIT.get().unwrap().data.clone());
                return blocking(move || flow::finish_setup(keystore::system(), &data, &node, Some(&label)), Msg::Finished);
            }
            Msg::RetryKey => {
                if self.busy {
                    return Task::none();
                }
                self.busy = true;
                let (node, data) = (self.node.clone(), INIT.get().unwrap().data.clone());
                return blocking(move || flow::finish_setup(keystore::system(), &data, &node, None), Msg::Finished);
            }
            Msg::KeyFailPass => {
                if self.busy {
                    return Task::none();
                }
                if self.pass_in.is_empty() {
                    self.notice = Some(friendly(Error::WeakPassphrase));
                    return Task::none();
                }
                self.busy = true;
                self.notice = None;
                let pass = take_secret(&mut self.pass_in);
                let (node, data) = (self.node.clone(), INIT.get().unwrap().data.clone());
                return blocking(
                    move || flow::add_passphrase_instead(keystore::system(), &data, &node, pass).map_err(SetupErr::Other),
                    Msg::Finished,
                );
            }
            Msg::Finished(r) => {
                self.busy = false;
                match r {
                    Ok(()) => {
                        self.key_fail = None;
                        self.notice = None;
                        self.refresh_pass();
                        self.refresh_identity();
                        self.refresh_accounts();
                        self.name_edit = self.profile_name.clone();
                        self.device_edit = self.device_label.clone();
                        self.return_to = None;
                        if std::mem::take(&mut self.link_joined) {
                            // Linked: no recovery phrase to show; contacts and history follow.
                            crate::tlog!("LINK_SAVED");
                            self.screen = Screen::LinkSync;
                            return self.start_node();
                        }
                        if self.gate.creating {
                            self.setup = None;
                            return self.gate_saved();
                        }
                        match self.setup.take().and_then(|s| s.phrase) {
                            Some(p) => {
                                self.new_phrase = Some(p);
                                self.screen = Screen::Phrase;
                            }
                            None => {
                                self.screen = Screen::Home;
                                return self.start_node();
                            }
                        }
                    }
                    Err(SetupErr::KeyNotSaved(e)) => {
                        eprintln!("keystore: could not store the key: {e}");
                        self.key_fail = Some(e);
                        self.pass_in.zeroize();
                        self.screen = Screen::KeyFailed;
                    }
                    Err(SetupErr::Other(e)) => self.notice = Some(e),
                }
            }
            Msg::KeyringProbed(ok) if !self.demo => self.keyring_ok = ok,
            Msg::KeyringProbed(_) => {}
            Msg::RememberProbed(ok, on) if !self.demo => {
                self.remember_avail = Some(ok);
                self.remember = ok && on;
            }
            Msg::RememberProbed(..) => {}
            Msg::Remember(on) => {
                self.remember = on;
                // On the unlock screen the choice waits for the unlock; on the Passphrase page it applies now.
                if self.screen != Screen::Unlock && self.lock == LockState::Unlocked && self.has_pass && !self.demo {
                    let (node, data) = (self.node.clone(), INIT.get().unwrap().data.clone());
                    return blocking(move || flow::apply_remember(keystore::system(), &data, &node, on).map(|_| on), Msg::RememberSet);
                }
            }
            Msg::RememberSet(r) => match r {
                Ok(on) => self.remember = on,
                Err(_) => {
                    self.remember = false;
                    self.notice = Some("This computer\u{2019}s keyring wouldn\u{2019}t keep it, so Tinline will keep asking for your passphrase.".into());
                }
            },
            Msg::ToggleRestore => {
                // Choose › Restore my account: the 24 words, in the sign-up card.
                self.restore = true;
                return self.gate_restore();
            }
            Msg::Restore => {
                let name = self.name_in.trim().to_string();
                if name.is_empty() || self.phrase_in.trim().is_empty() {
                    self.notice = Some("Enter your name and recovery phrase".into());
                    return Task::none();
                }
                if self.busy {
                    return Task::none();
                }
                if self.pass_in.is_empty() && !self.keyring_ok {
                    self.notice = Some(NO_KEYRING.into());
                    return Task::none();
                }
                self.busy = true;
                self.exists_hint = false;
                self.notice = Some("Restoring...".into());
                let pass = take_secret(&mut self.pass_in);
                // The typed phrase stays until the restore succeeds, so a mistake costs no retyping.
                let phrase = self.phrase_in.to_string();
                let node = self.node.clone();
                return blocking(
                    move || {
                        node.restore_identity(phrase, name, pass)
                            .map_err(|e| {
                                let exists = matches!(e, Error::AccountExists(_));
                                (friendly(e), exists)
                            })
                    },
                    Msg::Restored,
                );
            }
            Msg::Restored(r) => {
                self.busy = false;
                match r {
                    Ok(()) => {
                        self.phrase_in.zeroize();
                        self.show_phrase_in = false;
                        self.notice = None;
                        self.refresh_pass();
                        self.name_edit = self.name_in.trim().to_string();
                        if self.gate.restoring {
                            // In the card: name this computer next, then the terms and Done.
                            self.setup = Some(Setup { phrase: None });
                            self.gate.creating = true;
                            self.gate.words = Default::default();
                            if self.device_in.trim().is_empty() { self.device_in = flow::suggest_label(); }
                            return self.gate_go_pub(signup::Step::Device);
                        }
                        self.begin_name_device(None);
                    }
                    Err((e, exists)) => {
                        self.exists_hint = exists;
                        self.accounts = self.node.accounts();
                        self.notice = Some(e);
                    }
                }
            }
            Msg::Unlock if !self.has_pass => {
                if self.busy {
                    return Task::none();
                }
                self.busy = true;
                self.notice = None;
                let node = self.node.clone();
                let data = INIT.get().unwrap().data.clone();
                return blocking(
                    move || {
                        if keystore::auto_unlock(keystore::system(), &data, &node) { Ok(()) } else { Err("Still can\u{2019}t find the key.".to_string()) }
                    },
                    Msg::Unlocked,
                );
            }
            Msg::Unlock => {
                if self.busy || self.pass_in.is_empty() {
                    return Task::none();
                }
                self.busy = true;
                self.notice = None;
                let pass = take_secret(&mut self.pass_in);
                let node = self.node.clone();
                let data = INIT.get().unwrap().data.clone();
                let (remember, avail) = (self.remember, self.remember_avail == Some(true));
                return blocking(move || {
                    node.unlock(pass).map_err(friendly)?;
                    // "Remember on this computer", applied only after a correct passphrase. A keyring failure never blocks the unlock.
                    if avail {
                        if let Err(e) = flow::apply_remember(keystore::system(), &data, &node, remember) { eprintln!("keystore: remember: {e}"); }
                    }
                    Ok(())
                }, Msg::Unlocked);
            }
            Msg::Unlocked(r) => {
                self.busy = false;
                match r {
                    Ok(()) => {
                        self.screen = Screen::Home;
                        self.notice = None;
                        self.name_edit = self.node.profile().map(|p| p.name).unwrap_or_default();
                        self.refresh_accounts();
                        self.device_edit = self.device_label.clone();
                        self.refresh_pass();
                        return self.start_node();
                    }
                    Err(e) => {
                        eprintln!("UNLOCK failed: {e}");
                        self.screen = Screen::Unlock;
                        self.notice = Some(e);
                    }
                }
            }
            Msg::GoRestore => return self.update(Msg::NewAccount(true)),
            Msg::NewAccount(restore) => {
                if self.busy {
                    return Task::none();
                }
                if self.call.is_some() {
                    self.notice = Some(friendly(Error::InCall));
                    return Task::none();
                }
                self.busy = true;
                self.acct_menu = false;
                self.notice = None;
                // Cancel returns to the account that is selected now.
                self.return_to = keystore::current_did(&self.node);
                let node = self.node.clone();
                return blocking(move || node.begin_new_account().map_err(friendly), move |r| Msg::NewAccountBegun(r, restore));
            }
            Msg::NewAccountBegun(r, restore) => {
                self.busy = false;
                match r {
                    Ok(()) => {
                        self.reset_account_state();
                        self.gate_reset();
                        self.restore = restore;
                        self.exists_hint = false;
                        self.name_in.clear();
                        self.phrase_in.zeroize();
                        self.notice = None;
                        self.screen = Screen::Onboarding;
                        if restore {
                            return Task::batch([self.probe_keyring(), self.gate_restore()]);
                        }
                        return self.probe_keyring();
                    }
                    Err(e) => {
                        self.return_to = None;
                        self.notice = Some(e);
                    }
                }
            }
            Msg::CancelNew => {
                self.restore = false;
                self.phrase_in.zeroize();
                self.pass_in.zeroize();
                self.notice = None;
                self.exists_hint = false;
                if let Some(did) = self.return_to.take() {
                    self.switch_ask = Some(did);
                    return self.update(Msg::SwitchGo);
                }
            }
            Msg::ToggleReveal => {
                let open = !self.reveal_form;
                self.hide_phrase();
                self.reveal_form = open;
                self.pass_in.zeroize();
                self.notice = None;
            }
            Msg::HidePhrase => self.hide_phrase(),
            Msg::RevealSubmit => {
                // With no passphrase the plain confirm on screen is the check.
                if self.busy || (self.has_pass && self.pass_in.is_empty()) {
                    return Task::none();
                }
                self.busy = true;
                self.notice = None;
                let pass = take_secret(&mut self.pass_in);
                let node = self.node.clone();
                return blocking(move || node.recovery_phrase(pass).map_err(friendly), Msg::Revealed);
            }
            Msg::Revealed(r) => {
                self.busy = false;
                match r {
                    Ok(p) => {
                        self.revealed = Some(Zeroizing::new(p));
                        self.revealed_at = Some(Instant::now());
                    }
                    Err(e) => self.notice = Some(e),
                }
            }
            Msg::ToggleChange => {
                self.change_form = !self.change_form;
                self.old_in.zeroize();
                self.pass_in.zeroize();
                self.notice = None;
            }
            Msg::ChangePassSubmit => {
                if self.busy || (self.has_pass && self.old_in.is_empty()) {
                    return Task::none();
                }
                if self.pass_in.is_empty() {
                    self.notice = Some(friendly(Error::WeakPassphrase));
                    return Task::none();
                }
                self.busy = true;
                self.notice = Some(if self.has_pass { "Changing passphrase..." } else { "Adding passphrase..." }.into());
                let adding = !self.has_pass;
                let (old, new) = (take_secret(&mut self.old_in), take_secret(&mut self.pass_in));
                let node = self.node.clone();
                let data = INIT.get().unwrap().data.clone();
                return blocking(
                    move || {
                        // The same data key is rewrapped, so nothing is re-encrypted.
                        node.set_passphrase((!adding).then_some(old), new).map_err(friendly)?;
                        // From now on Tinline asks for the passphrase at start: drop the stored key
                        // (and the old plaintext file, if that is where it was).
                        keystore::sync(keystore::system(), &data, &node)?;
                        Ok(adding)
                    },
                    Msg::PassChanged,
                );
            }
            Msg::PassChanged(r) => {
                self.busy = false;
                match r {
                    Ok(added) => {
                        self.refresh_pass();
                        self.hide_phrase();
                        self.change_form = false;
                        self.notice = Some(if added { "Passphrase added. You will need it next time Tinline starts." } else { "Passphrase changed" }.into());
                        self.pv.done = true;
                    }
                    Err(e) => {
                        // A wrong current passphrase belongs under its field.
                        if self.profiles.page == Some(profile_ui::Page::Native(24)) { self.pv.errs[0] = "That isn\u{2019}t your current passphrase.".into(); } else { self.notice = Some(e); }
                    }
                }
            }
            Msg::PhraseSaved => {
                self.new_phrase = None;
                self.screen = Screen::Home;
                self.notice = None;
                return self.start_node();
            }
            Msg::Started(r) => {
                self.status = self.node.status();
                self.reload_contacts();
                // Unlocking opens the call log too; nothing else reloads it until the next call.
                self.refresh_history();
                match r {
                    Ok(()) => {
                        #[cfg(feature = "test-hooks")]
                        if let Some(v) = crate::test_env("P2P_OPEN") {
                            return Task::batch([self.fetch_ticket(), self.refresh_chats(), self.test_open(&v)]);
                        }
                        return Task::batch([self.fetch_ticket(), self.refresh_chats(), self.check_clipboard(false)]);
                    }
                    Err(e) => self.notice = Some(format!("Could not start: {e}")),
                }
            }
            Msg::Ticket(r) => {
                self.fetching = false;
                match r {
                Ok(c) => {
                    crate::tlog!("TICKET {}", c.code);
                    self.qr = view::qr_of(&c.code);
                    self.ticket = Some(c.code);
                    self.ticket_exp = Some(c.expires_at);
                }
                    Err(e) => self.notice = Some(format!("Ticket: {e}")),
                }
            }
            Msg::CopyPhrase => {
                let phrase = if self.screen == Screen::Phrase {
                    self.new_phrase.as_ref()
                } else {
                    self.revealed.as_ref()
                };
                if let Some(phrase) = phrase {
                    let text = phrase.to_string();
                    self.notice = Some("Your recovery words are copied.".into());
                    return clipboard::write(text);
                }
            }
            Msg::SettingsTab(n) => {
                self.settings_tab = n;
                self.dev_rename = None;
                self.unlink_ask = None;
                self.notice = None;
                if n == 1 {
                    self.refresh_linked();
                }
            }
            Msg::Key(key, mods) => {
                use iced::keyboard::key::Named;
                use iced::keyboard::Key;
                let ringing = self.call.as_ref().is_some_and(|c| c.info.incoming && !c.answered && c.state == CallState::Ringing);
                let active = self.call.as_ref().is_some_and(|c| c.state == CallState::Active);
                // Cropper.svelte keys: arrows move (Shift = 40px), + / - zoom by .1, Home recentres.
                if let Some(profile_ui::Layer::Picture { crop, .. }) = &self.profiles.layer {
                    use profile_ui::Action as A;
                    let d = if mods.shift() { 40.0 } else { 10.0 };
                    let a = match key.as_ref() {
                        Key::Named(Named::ArrowLeft) => Some(A::Nudge(d, 0.0)),
                        Key::Named(Named::ArrowRight) => Some(A::Nudge(-d, 0.0)),
                        Key::Named(Named::ArrowUp) => Some(A::Nudge(0.0, d)),
                        Key::Named(Named::ArrowDown) => Some(A::Nudge(0.0, -d)),
                        Key::Named(Named::Home) => Some(A::Recenter),
                        Key::Character("+" | "=") => Some(A::Zoom(crop.zoom + 0.1)),
                        Key::Character("-" | "_") => Some(A::Zoom(crop.zoom - 0.1)),
                        _ => None,
                    };
                    if let Some(a) = a {
                        return self.profile_update(a);
                    }
                }
                // Sign-up: Enter continues on every step (a focused text field submits itself first).
                if self.screen == Screen::Onboarding
                    && !self.restore
                    && self.call.is_none()
                    && matches!(key.as_ref(), Key::Named(Named::Enter))
                    && !(mods.shift() || mods.control() || mods.alt() || mods.logo())
                {
                    return self.gate_update(Gs::Enter);
                }
                match key.as_ref() {
                    // Layer.svelte: Escape closes the open dialog first (new chat, then profile layers).
                    Key::Named(Named::Escape) if self.shell_search.new_chat.is_some() => return self.shell_search_update(shell_search::Action::NewChat(false)),
                    Key::Named(Named::Escape) if self.profiles.layer.is_some() => return self.profile_update(profile_ui::Action::CloseLayer),
                    Key::Named(Named::Escape) if self.profiles.edit.is_some() => return self.profile_update(profile_ui::Action::CancelEdit),
                    Key::Named(Named::Escape) if self.chat.viewer.is_some() => return self.update_chat(Cm::CloseView),
                    Key::Named(Named::Escape) if self.chat.find.is_some() => return self.update_chat(Cm::Find(None)),
                    Key::Named(Named::ArrowLeft) if self.chat.viewer.is_some() => return self.update_chat(Cm::Step(-1)),
                    Key::Named(Named::ArrowRight) if self.chat.viewer.is_some() => return self.update_chat(Cm::Step(1)),
                    Key::Named(Named::Escape) if self.chat.rec.is_some() => return self.update_chat(Cm::RecCancel),
                    Key::Named(Named::Enter) if ringing => return self.update(Msg::Answer),
                    Key::Named(Named::Escape) if self.call_ui.about => return self.update(Msg::CallUi(crate::call_ui::Action::CloseAbout)),
                    Key::Named(Named::Escape) if ringing => return self.update(Msg::Decline),
                    Key::Named(Named::Escape) if self.call.is_some() && !self.call_ui.minimized => return self.update(Msg::CallUi(crate::call_ui::Action::Minimize)),
                    #[cfg(any(test, feature = "test-hooks"))]
                    Key::Named(Named::Escape) if self.groups.dialog.is_some() => return self.update_groups(Gm::Dismiss),
                    Key::Named(Named::Escape) if self.live.dialog.is_some() => return self.update_live(Lm::Dismiss),
                    Key::Named(Named::Escape) if self.call.is_none() && self.shell_search.active => return self.shell_search_update(shell_search::Action::Escape),
                    Key::Named(Named::Escape) if self.call.is_none() && self.live.open.is_some() => return self.update_live(Lm::Close),
                    // DesktopShell has no Escape-to-close for the detail pane: Escape belongs to the field,
                    // the open layer, the composer's reply/edit context and the call screen only.
                    // Add contact / link screens stand in for the playground's Layer dialogs, which Escape closes.
                    Key::Named(Named::Escape) if self.call.is_none() && !matches!(self.screen, Screen::Home | Screen::Settings) => return self.update(Msg::Home),
                    // Only reaches here when no text input took the paste.
                    Key::Character("v") if mods.command() && self.can_offer() && self.chat.viewer.is_none() => {
                        return self.check_clipboard(true);
                    }
                    Key::Character("k") if mods.command() && self.call.is_none() => return self.shell_search_update(shell_search::Action::Focus),
                    Key::Character("f") if mods.command() && self.call.is_none() && self.chat.peer.is_some() && !self.chat.info => {
                        return self.update_chat(chat::Cm::Find(Some(self.chat.find.clone().unwrap_or_default())));
                    }
                    Key::Character("m") if mods.command() && active => return self.update(Msg::ToggleMute),
                    Key::Character("e") if mods.command() && self.call.is_some() => return self.update(Msg::Hangup),
                    _ => {}
                }
            }
            Msg::Focus(id, on) => {
                if Some(id) != self.win {
                    return Task::none();
                }
                self.focused = on;
                if on {
                    // What arrived while the user was elsewhere counts as read now.
                    let mut tasks = vec![self.check_clipboard(false)];
                    if self.chat_visible()
                        && let Some(peer) = self.chat.peer.clone()
                    {
                        tasks.push(self.mark_read(&peer));
                    }
                    return Task::batch(tasks);
                }
            }
            Msg::ClipText(explicit, text) => {
                if !explicit && !self.can_offer() {
                    return Task::none();
                }
                let Some(peek) = text.filter(|t| t.len() < 64 * 1024).and_then(|t| self.node.peek_card(t)) else {
                    return Task::none();
                };
                if peek.known {
                    if explicit {
                        self.notice = Some(format!("{} is already in your contacts", peek.name));
                    }
                } else if explicit || !self.dismissed.contains(&peek.ticket) {
                    self.offer = Some(peek);
                }
            }
            Msg::OfferAdd => {
                if let Some(p) = self.offer.take() {
                    self.add_in = p.ticket;
                    return self.update(Msg::AddPressed);
                }
            }
            Msg::OfferDismiss => {
                if let Some(p) = self.offer.take() {
                    self.dismissed.insert(p.ticket);
                }
            }
            Msg::AddAlias(v) => self.add_alias = v,
            Msg::OpenAdd => {
                self.screen = Screen::AddContact;
                self.add_ui = AddUi::default();
                self.add_phase = AddPhase::Idle;
                self.add_in.clear();
                self.add_alias.clear();
                self.notice = None;
            }
            Msg::AddPressed => {
                let mut t = self.add_in.trim().to_string();
                if t.is_empty() || matches!(self.add_phase, AddPhase::Connecting(_)) {
                    return Task::none();
                }
                // A card pasted with text around it (a chat message, a mail) is cut out of it.
                if let Some(p) = self.node.peek_card(t.clone()) {
                    t = p.ticket.clone();
                    self.add_ui.found = Some(p);
                }
                self.offer = None;
                self.screen = Screen::AddContact;
                if self.status.started && !self.status.online {
                    self.add_phase = AddPhase::Failed(Reach::Offline);
                    return Task::none();
                }
                self.add_phase = AddPhase::Connecting(Instant::now());
                let node = self.node.clone();
                // "Reaching X's Tinline…" turns into "Still trying…" after 2.5 s.
                let slow = Task::perform(tokio::time::sleep(Duration::from_millis(2500)), |_| Msg::Frame(Instant::now()));
                return Task::batch([slow, blocking(move || node.add_contact_by_code(t).map_err(Reach::of), Msg::Added)]);
            }
            Msg::Added(r) => {
                self.reload_contacts();
                match r {
                    Ok(p2pcore::AddOutcome::Added { contact: c }) => {
                        self.add_in.clear();
                        self.add_alias = c.name.clone();
                        self.add_phase = AddPhase::Added(c);
                        // The line opens: one `line` cue, here (the first screen's tug plays none).
                        self.play_line();
                    }
                    // Delivered: the line keeps its connecting sway while they decide.
                    Ok(p2pcore::AddOutcome::Requested { .. }) => {
                        self.add_in.clear();
                        self.add_phase = AddPhase::Requested;
                    }
                    Err(e) => self.add_phase = AddPhase::Failed(e),
                }
            }
            Msg::RequestAnswered(did, r) => {
                self.add_ui.answering = false;
                self.reload_contacts();
                match r {
                    Ok(_) => {
                        // Both lines go taut together: ours now, theirs when the Welcome lands.
                        if let Some(i) = self.add_ui.incoming.as_mut().filter(|i| i.did == did) {
                            i.joined = true;
                        }
                        self.play_line();
                    }
                    Err(e) => {
                        eprintln!("accept_contact_request: {e}");
                        self.notice = Some("Couldn\u{2019}t accept just now. Try again in a moment.".into());
                    }
                }
            }
            Msg::SaveAlias => {
                if let AddPhase::Added(c) = &self.add_phase {
                    let alias = self.add_alias.trim().to_string();
                    let alias = (!alias.is_empty() && alias != c.name).then_some(alias);
                    let (node, did) = (self.node.clone(), c.did.clone());
                    self.sel = Some(did.clone());
                    self.detail = Detail::View;
                    self.screen = Screen::Home;
                    self.add_phase = AddPhase::Idle;
                    let open = self.open_chat(&did.clone());
                    return Task::batch([
                        open,
                        blocking(move || node.rename_contact(did, alias).map_err(s), Msg::Done),
                    ]);
                }
            }
            Msg::CallUi(action) => {
                let can_minimize = self.call.as_ref().is_some_and(|c| !(c.info.incoming && !c.answered && c.state == CallState::Ringing));
                self.call_ui.apply(action, can_minimize);
            }
            Msg::CallPressed(did) => {
                self.video_intent = false;
                return self.dial(did);
            }
            Msg::GCall(m) => return self.gcall_update(m),
            Msg::Privacy(m) => match m {
                PvMsg::Pick(on) => self.pv.pick = on,
                PvMsg::PickPerson(did) => {
                    self.pv.pick = false;
                    return self.profile_update(profile_ui::Action::AskBlock(did));
                }
                PvMsg::Unblock(d) => self.pv.unblock = d,
                PvMsg::UnblockGo => {
                    let Some(did) = self.pv.unblock.take() else { return Task::none() };
                    return self.update(Msg::Unblock(did));
                }
                PvMsg::Words(on) => {
                    self.pv.shown = on;
                    self.pv.shown_at = on.then(Instant::now);
                    if on { self.revealed_at = Some(Instant::now()); }
                }
                PvMsg::AsKey(k) => self.pv.as_key = k,
                PvMsg::CopyKey => {
                    if let Some(p) = &self.revealed {
                        if let Ok(k) = p2pcore::recovery_key_from_phrase(p.to_string()) {
                            self.notice = Some("Your recovery key is copied.".into());
                            return iced::clipboard::write(k);
                        }
                    }
                }
                PvMsg::Eye => self.pv.reveal = !self.pv.reveal,
                PvMsg::Again(v) => {
                    self.pv.again = Zeroizing::new(v);
                    self.pv.errs = Default::default();
                    self.pv.done = false;
                }
                PvMsg::Change => {
                    // ChangePass.svelte's checks, with sign-up's rule that any length works.
                    let mut e: [String; 3] = Default::default();
                    if self.pass_in.is_empty() { e[1] = "Type a new passphrase.".into(); }
                    else if self.has_pass && *self.pass_in == *self.old_in { e[1] = "Pick one that is different from the current one.".into(); }
                    if e[1].is_empty() && *self.pv.again != *self.pass_in { e[2] = "The two passphrases differ.".into(); }
                    if self.has_pass && self.old_in.is_empty() { e[0] = "Type your current passphrase.".into(); }
                    let bad = e.iter().any(|x| !x.is_empty());
                    self.pv.errs = e;
                    if bad { return Task::none(); }
                    self.pv.again.zeroize();
                    return self.update(Msg::ChangePassSubmit);
                }
            },
            Msg::Alerts(a) => {
                match a {
                    Alerts::Quiet(on) => self.settings.quiet = on,
                    Alerts::Palette(p) => {
                        self.settings.palette = p.key().into();
                        self.settings.apply_alerts();
                        crate::audio::play_call_cue(self.ctl.devices.lock().unwrap().1.clone(), "ring");
                    }
                    Alerts::Preview(kind) => {
                        crate::audio::play_call_cue(self.ctl.devices.lock().unwrap().1.clone(), kind);
                        return Task::none();
                    }
                    Alerts::Messages(on) => self.settings.no_notify_messages = !on,
                    Alerts::Calls(on) => self.settings.no_notify_calls = !on,
                    Alerts::Groups(on) => self.settings.no_notify_groups = !on,
                }
                self.save_settings();
            }
            Msg::DialVideo(did) => {
                self.video_intent = true;
                return self.dial(did);
            }
            Msg::CallAgain(did) => return self.dial(did),
            Msg::CallStarted(r) => match r {
                Ok(info) => {
                    let id = info.call_id.clone();
                    self.ended = None;
                    self.call = Some(CallView { info, state: CallState::Dialing, answered: false, stats: None });
                    let mut tasks = Vec::new();
                    for (eid, st) in std::mem::take(&mut self.early) {
                        if eid == id {
                            tasks.push(self.apply_call_state(&eid, st));
                        }
                    }
                    return Task::batch(tasks);
                }
                Err(e) => self.notice = Some(format!("Could not call: {e}")),
            },
            Msg::Select(did) => {
                #[cfg(any(test, feature = "test-hooks"))]
                {
                    self.groups.open = None;
                }
                self.live.close();
                self.sel = Some(did);
                self.chat.info = false;
                self.detail = Detail::View;
                self.safety = None;
                self.refresh_detail_calls();
                if self.screen != Screen::Home {
                    self.screen = Screen::Home;
                }
                let did = self.sel.clone().unwrap_or_default();
                return self.open_chat(&did);
            }
            Msg::Home => {
                #[cfg(any(test, feature = "test-hooks"))]
                {
                    self.groups.open = None;
                    self.groups.dialog = None;
                }
                self.live.close();
                self.live.dialog = None;
                self.leave_chat();
                self.sel = None;
                self.detail = Detail::View;
                self.safety = None;
                if self.call.is_none() {
                    self.screen = Screen::Home;
                }
            }
            Msg::SetDetail(d) => {
                self.detail = d;
                self.safety = None;
                if d == Detail::Rename {
                    self.rename_in = self.selected().map(|c| c.alias.clone().unwrap_or_default()).unwrap_or_default();
                    return operation::focus(RENAME_ID);
                }
                if d == Detail::Verify
                    && let Some(did) = self.sel.clone()
                {
                    match self.node.safety_number(did) {
                        Ok(n) => self.safety = Some(n),
                        Err(e) => {
                            self.notice = Some(friendly(e));
                            self.detail = Detail::View;
                        }
                    }
                }
            }
            Msg::RenameIn(v) => self.rename_in = v,
            Msg::RenameSave => return self.rename(false),
            Msg::RenameClear => return self.rename(true),
            Msg::SetVerified(v) => {
                if let Some(did) = self.sel.clone() {
                    let node = self.node.clone();
                    self.detail = Detail::View;
                    self.safety = None;
                    return blocking(move || node.set_verified(did, v).map_err(s), Msg::Done);
                }
            }
            Msg::CopySafety => {
                if let Some(n) = self.safety.clone() {
                    self.notice = Some("Safety number copied".into());
                    return clipboard::write(n);
                }
            }
            Msg::SearchIn(v) => return self.shell_search_query(v),
            Msg::ShellSearch(action) => return self.shell_search_update(action),
            Msg::Profile(action) => return self.profile_update(action),
            Msg::SetAvail(on, hours) => {
                let until = hours.map(|h| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                        + h * 3600
                });
                return self.profile_write(profile_ui::Save::Availability, move |node| node.set_available(on, until).map_err(s));
            }
            Msg::CloseEnded => self.ended = None,
            Msg::EndedHoldDone(at) => {
                if self.ended.as_ref().map(|e| e.at) == at { self.ended = None; }
            }
            // The contact, the chat and the call history stay (U19, U24); only the state changes.
            Msg::Block(did) => {
                self.detail = Detail::View;
                let node = self.node.clone();
                return blocking(move || crate::blocks::BlockApi::block(&*node, did).map_err(s), Msg::Done);
            }
            Msg::AllowCalls(did, on) => {
                let node = self.node.clone();
                return blocking(move || node.allow_calls(did, on).map_err(s), Msg::Done);
            }
            Msg::Unblock(did) => {
                let node = self.node.clone();
                return blocking(move || crate::blocks::BlockApi::unblock(&*node, did).map_err(s), Msg::Done);
            }
            Msg::Done(r) => {
                self.reload_contacts();
                self.refresh_history();
                if let Err(e) = r {
                    self.notice = Some(e);
                }
            }
            Msg::Answer => {
                if let Some(c) = &self.call {
                    self.ctl.stop_ring();
                    let (node, id) = (self.node.clone(), c.info.call_id.clone());
                    return blocking(move || node.answer(id).map_err(s), Msg::Done);
                }
            }
            Msg::Decline => {
                if let Some(c) = &self.call {
                    self.ctl.stop_ring();
                    let (node, id) = (self.node.clone(), c.info.call_id.clone());
                    return blocking(move || node.decline(id).map_err(s), Msg::Done);
                }
            }
            Msg::DeclineWaiting => {
                if let Some(w) = self.waiting.take() {
                    self.ctl.stop_ring();
                    let node = self.node.clone();
                    return blocking(move || node.decline(w.call_id).map_err(s), Msg::Done);
                }
            }
            Msg::EndAnswer => {
                if let Some(w) = self.waiting.take() {
                    self.ctl.stop_ring();
                    self.takeover = Some(w.clone());
                    let node = self.node.clone();
                    return blocking(move || node.end_and_answer(w.call_id).map_err(s), Msg::Done);
                }
            }
            Msg::Hangup => {
                if let Some(c) = &self.call {
                    let (node, id) = (self.node.clone(), c.info.call_id.clone());
                    return blocking(move || node.hangup(id).map_err(s), Msg::Done);
                }
            }
            Msg::ToggleMute => {
                let m = !self.ctl.muted.load(Ordering::Relaxed);
                self.ctl.muted.store(m, Ordering::Relaxed);
            }
            Msg::OpenSettings => {
                self.screen = Screen::Settings;
                self.profiles.page = Some(profile_ui::Page::Settings);
                self.profiles.history.clear();
                self.profiles.layer = None;
                self.profiles.edit = None;
                self.leave_chat();
                self.acct_menu = false;
                self.remove_ask = None;
                self.refresh_accounts();
                self.refresh_linked();
                self.device_edit = self.device_label.clone();
                self.hide_phrase();
                self.change_form = false;
                self.pass_in.zeroize();
                self.old_in.zeroize();
                self.notice = None;
                // Enumerating devices can block for a while (PulseAudio/WASAPI).
                return Task::batch([blocking(audio::list_devices, Msg::Devices), self.refresh_profiles()]);
            }
            Msg::Devices(d) => self.devices = d,
            Msg::Back => {
                if self.screen == Screen::Settings { return self.profile_update(profile_ui::Action::Back); }
                if self.screen == Screen::AddContact {
                    self.add_ui.incoming = None;
                }
                self.screen = Screen::Home;
                self.hide_phrase();
                self.notice = None;
            }
            Msg::NameEdit(v) => self.name_edit = v,
            Msg::SaveName => {
                let name = self.name_edit.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                return self.profile_write(profile_ui::Save::Field("name".into()), move |node| node.set_name(name).map_err(s));
            }
            Msg::InDev(d) => {
                self.settings.input = (d != DEFAULT_LABEL).then_some(d);
                self.ctl.devices.lock().unwrap().0 = self.settings.input.clone();
                self.save_settings();
            }
            Msg::OutDev(d) => {
                self.settings.output = (d != DEFAULT_LABEL).then_some(d);
                self.ctl.devices.lock().unwrap().1 = self.settings.output.clone();
                self.save_settings();
            }
            Msg::SwitchAsk(did) => {
                if self.call.is_some() {
                    self.notice = Some(friendly(Error::InCall));
                    return Task::none();
                }
                self.switch_ask = Some(did);
            }
            Msg::SwitchNow(did) => {
                self.gate.open_as = false;
                self.switch_ask = Some(did);
                return self.update(Msg::SwitchGo);
            }
            Msg::SwitchGo => {
                let Some(did) = self.switch_ask.take() else { return Task::none() };
                if self.busy {
                    return Task::none();
                }
                if self.call.is_some() {
                    self.notice = Some(friendly(Error::InCall));
                    return Task::none();
                }
                self.busy = true;
                self.notice = None;
                let (node, data) = (self.node.clone(), INIT.get().unwrap().data.clone());
                return blocking(move || flow::switch_to(keystore::system(), &data, &node, &did).map_err(friendly), Msg::Switched);
            }
            Msg::Switched(r) => {
                self.busy = false;
                match r {
                    Ok(unlocked) => {
                        self.reset_account_state();
                        self.remember_avail = None;
                        self.restore = false;
                        self.return_to = None;
                        self.screen = Screen::Home;
                        if unlocked {
                            return self.start_node();
                        }
                        self.screen = Screen::Unlock;
                        return self.probe_remember();
                    }
                    Err(e) => self.notice = Some(e),
                }
            }
            Msg::DeviceEdit(v) => self.device_edit = v,
            Msg::SaveDevice => {
                let Some(label) = flow::valid_label(&self.device_edit) else {
                    self.notice = Some("A device name is 1 to 64 characters.".into());
                    return Task::none();
                };
                let node = self.node.clone();
                return blocking(
                    move || node.set_device_label(label.clone()).map(|_| label).map_err(s),
                    Msg::DeviceSaved,
                );
            }
            Msg::DeviceSaved(r) => match r {
                Ok(l) => {
                    self.device_label = l.clone();
                    self.device_edit = l;
                    self.notice = Some("Device name saved".into());
                }
                Err(e) => self.notice = Some(e),
            },
            Msg::RemoveAsk(did) => self.remove_ask = Some(did),
            Msg::RemoveCancel => self.remove_ask = None,
            Msg::RemoveGo(did) => {
                if self.busy { return Task::none(); }
                self.busy = true;
                self.remove_ask = None;
                let (node, data) = (self.node.clone(), INIT.get().unwrap().data.clone());
                return blocking(
                    move || flow::remove(keystore::system(), &data, &node, &did).map(|_| "Account removed from this computer".to_string()).map_err(friendly),
                    Msg::Removed,
                );
            }
            Msg::Removed(r) => {
                self.busy = false;
                self.refresh_accounts();
                self.accounts = self.node.accounts();
                self.notice = Some(match r {
                    Ok(m) | Err(m) => m,
                });
            }
            Msg::Chat(m) => return self.update_chat(m),
            #[cfg(any(test, feature = "test-hooks"))]
            Msg::Groups(m) => return self.update_groups(m),
            Msg::Live(m) => return self.update_live(m),
            m @ (Msg::LinkStartNew
            | Msg::LinkStartExisting
            | Msg::NewLink
            | Msg::NewLinkBegun(_)
            | Msg::LinkNameGo
            | Msg::LinkQr(_)
            | Msg::LinkRegen
            | Msg::LinkPasteToggle
            | Msg::LinkPasteIn(_)
            | Msg::LinkPasteGo
            | Msg::LinkScanned(_)
            | Msg::LinkPassIn(_)
            | Msg::LinkApprove
            | Msg::LinkApproved(_)
            | Msg::LinkCancel
            | Msg::LinkDoneOk
            | Msg::LinkSecureGo
            | Msg::LinkSyncDone
            | Msg::DevRenameStart(_)
            | Msg::DevMenu(_)
            | Msg::DevRenameIn(_)
            | Msg::DevRenameSave
            | Msg::DevRenameCancel
            | Msg::DevDone(_)
            | Msg::UnlinkAsk(_)
            | Msg::UnlinkCancel
            | Msg::UnlinkGo
            | Msg::Unlinked(_)
            | Msg::UnlinkedOk) => return self.link_update(m),
            Msg::Video(m) => return self.video_update(m),
            Msg::ShotEvery => {
                if let Some(id) = self.win {
                    return window::screenshot(id).map(Msg::Shot);
                }
            }
            Msg::Shot(shot) => {
                if let Some(dir) = crate::test_env("P2P_SHOT_EVERY") {
                    self.shots += 1;
                    let path = format!("{dir}/{:03}.png", self.shots);
                    if let Ok(f) = std::fs::File::create(&path) {
                        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), shot.size.width, shot.size.height);
                        enc.set_color(png::ColorType::Rgba);
                        enc.set_depth(png::BitDepth::Eight);
                        let _ = enc.write_header().and_then(|mut w| w.write_image_data(&shot.rgba));
                    }
                    return Task::none();
                }
                if let Some(path) = crate::test_env("P2P_SHOT") {
                    let mut ok = false;
                    if let Ok(f) = std::fs::File::create(&path) {
                        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), shot.size.width, shot.size.height);
                        enc.set_color(png::ColorType::Rgba);
                        enc.set_depth(png::BitDepth::Eight);
                        ok = enc.write_header().and_then(|mut w| w.write_image_data(&shot.rgba)).is_ok();
                    }
                    crate::tlog!("SHOT {path} {ok}");
                    let _ = ok;
                    return iced::exit();
                }
            }
            Msg::ToneToggled(on) => {
                self.settings.tone = on;
                *self.ctl.tone.lock().unwrap() = self.tone_hz();
                self.save_settings();
                if self.call.as_ref().is_some_and(|c| c.state == CallState::Active) {
                    self.node.set_test_tone(self.tone_hz());
                }
            }
        }
        Task::none()
    }

    fn on_event(&mut self, ev: Ev) -> Task<Msg> {
        match ev {
            Ev::Status(st) => {
                let need_ticket = st.started && self.ticket.is_none() && !self.fetching;
                self.status = st;
                if need_ticket {
                    return self.fetch_ticket();
                }
            }
            Ev::Contacts => {
                if !self.demo {
                    self.reload_contacts();
                    // Our code may have been replaced (a rename, a block): show the current one.
                    let ticket = if self.ticket.is_some() && !self.fetching { self.fetch_ticket() } else { Task::none() };
                    if self.offer.as_ref().is_some_and(|o| self.contacts.iter().any(|c| c.did == o.did)) {
                        self.offer = None;
                    }
                    self.refresh_history();
                    // Linked just now: the first contacts mean the sync has started to land.
                    if self.screen == Screen::LinkSync && !self.contacts.is_empty() {
                        self.screen = Screen::Home;
                    }
                    return Task::batch([ticket, self.refresh_chats(), self.refresh_profiles()]);
                }
            }
            Ev::Profile { epoch, did } => {
                if epoch == self.chat.account_epoch {
                    if let Some(cached) = self.profiles.cache.get_mut(&did) { cached.photo = None; }
                    return self.refresh_profile(did);
                }
            }
            Ev::Chat(c) => return self.on_chat_event(c),
            Ev::Group(g) => {
                // Notifications › Groups: a post from someone else, while that group isn't in front of me; invitations too.
                if NOTIFY_GROUPS.load(Ordering::Relaxed) {
                    let attended = self.win.is_some() && self.focused;
                    match &g {
                        crate::GroupEv::Message(m) if !m.outgoing && m.kind == p2pcore::GroupItemKind::Post
                            && !(attended && self.live.open.as_deref() == Some(m.group_id.as_str())) && !self.blocks.blocked_by_me(&m.author_did) => {
                            let group = self.live.list.iter().find(|s| s.id == m.group_id).map(|s| s.name.clone()).unwrap_or_else(|| "A group".into());
                            let who = m.author_name.split_whitespace().next().unwrap_or(&m.author_name).to_string();
                            notify(&group, &format!("{who}: {}", m.text), Target::Show);
                            self.play_received();
                        }
                        // Away: "Book club · Priya invited you to the group". In front: one knock; the Groups tab counts it.
                        crate::GroupEv::Invitation(i) if !i.outgoing && i.state == p2pcore::GroupInvitationState::Pending => {
                            if attended { self.play_cue(crate::cues::knock()); }
                            else { notify(&i.group_name, &format!("{} invited you to the group", i.inviter_name.split_whitespace().next().unwrap_or(&i.inviter_name)), Target::Show); }
                        }
                        _ => {}
                    }
                }
                let t = self.update_live(match g {
                    crate::GroupEv::Message(m) => Lm::Message(m),
                    crate::GroupEv::Changed(id) => Lm::Changed(id),
                    crate::GroupEv::Request(r) => Lm::RequestChanged(r),
                    crate::GroupEv::Invitation(i) => Lm::InvitationChanged(i),
                });
                // The chat list's group marks follow group changes.
                return Task::batch([t, self.refresh_marks()]);
            }
            Ev::Link(l) => return self.link_event(l),
            Ev::Incoming(info) if self.call.as_ref().is_some_and(|c| c.state == CallState::Active && c.info.call_id != info.call_id) => {
                if crate::audio::RING_CALLS.load(Ordering::Relaxed) { notify("Tinline", &format!("{} is calling", info.peer_name), Target::Show); }
                let mut tasks = vec![self.show_window()];
                if let Some(act) = INIT.get().unwrap().waiting_action.clone() {
                    let (node, id) = (self.node.clone(), info.call_id.clone());
                    if act == "answer" {
                        self.takeover = Some(info.clone());
                    }
                    self.ctl.stop_ring();
                    tasks.push(blocking(
                        move || {
                            std::thread::sleep(Duration::from_secs(1));
                            if act == "answer" { node.end_and_answer(id) } else { node.decline(id) }.map_err(s)
                        },
                        Msg::Done,
                    ));
                    self.waiting = None;
                    return Task::batch(tasks);
                }
                self.waiting = Some(info);
                return Task::batch(tasks);
            }
            Ev::Incoming(info) => {
                // Its ordinary ring: a waiting call whose call ended, or a fresh one.
                self.waiting = None;
                let name = info.peer_name.clone();
                self.call = Some(CallView {
                    info: info.clone(),
                    state: CallState::Ringing,
                    answered: false,
                    stats: None,
                });
                self.ended = None;
                if crate::audio::RING_CALLS.load(Ordering::Relaxed) { notify("Tinline", &format!("{name} is calling"), Target::Show); }
                let mut tasks = vec![self.show_window()];
                if let Some(secs) = INIT.get().unwrap().auto_answer {
                    let (node, id) = (self.node.clone(), info.call_id);
                    self.ctl.stop_ring();
                    tasks.push(blocking(
                        move || {
                            std::thread::sleep(Duration::from_secs_f64(secs));
                            node.answer(id).map_err(s)
                        },
                        Msg::Done,
                    ));
                }
                return Task::batch(tasks);
            }
            Ev::State(id, st) if self.waiting.as_ref().is_some_and(|w| w.call_id == id) => {
                if let CallState::Ended { reason } = st {
                    let w = self.waiting.take().unwrap();
                    self.refresh_history();
                    // Unanswered, or the caller gave up: a missed call like any other.
                    if let End::Missed(text) = reason::present(&reason, &w.peer_name, true, false) {
                        if crate::audio::RING_CALLS.load(Ordering::Relaxed) { notify("Tinline", &text, Target::Show); }
                        self.notice = Some(text);
                    }
                }
            }
            Ev::State(id, st) => {
                if self.call.as_ref().is_some_and(|c| c.info.call_id == id) {
                    return self.apply_call_state(&id, st);
                } else {
                    // An outgoing call's first states can beat the reply to `call`.
                    self.early.push((id, st));
                    let n = self.early.len();
                    if n > 16 {
                        self.early.drain(..n - 16);
                    }
                }
            }
            Ev::AudioNotice(m) => self.notice = Some(m),
            Ev::Video(info) => return self.video_state(info),
            // The group call screens come with the call UI packet; audio already follows these.
            Ev::GroupCall(e) => return self.gcall_update(gcall::Gm::Ev(e)),
            Ev::Request(r) => return self.contact_request(r),
            Ev::Accepted(c) => return self.request_accepted(c),
            Ev::Tray(TrayCmd::Show) => return self.show_window(),
            Ev::Open(target) => {
                let mut tasks = vec![self.show_window()];
                if let Target::Chat(did) = target
                    && self.selectable(&did)
                    && self.call.is_none()
                {
                    tasks.push(self.update(Msg::Select(did)));
                }
                return Task::batch(tasks);
            }
            Ev::Tray(TrayCmd::Quit) => return self.update(Msg::Quit),
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            Subscription::run(events),
            iced::time::every(Duration::from_secs(1)).map(|_| Msg::Tick),
            if crate::test_env("P2P_SHOT_EVERY").is_some() { iced::time::every(Duration::from_millis(500)).map(|_| Msg::ShotEvery) } else { Subscription::none() },
            window::close_requests().map(Msg::CloseReq),
            window::close_events().map(Msg::Closed),
            system::theme_changes().map(Msg::Theme),
            iced::event::listen_with(|e, _, id| match e {
                iced::Event::Window(window::Event::Focused) => Some(Msg::Focus(id, true)),
                iced::Event::Window(window::Event::Unfocused) => Some(Msg::Focus(id, false)),
                iced::Event::Window(window::Event::Rescaled(f)) => Some(Msg::Scale(f)),
                iced::Event::Window(window::Event::FileDropped(p)) => Some(Msg::Chat(Cm::Dropped(p))),
                iced::Event::Window(window::Event::FileHovered(_)) => Some(Msg::Chat(Cm::DropHover(true))),
                iced::Event::Window(window::Event::FilesHoveredLeft) => Some(Msg::Chat(Cm::DropHover(false))),
                _ => None,
            }),
            if self.fx.running() || self.ticks_running() || self.live.ticks_running() { window::frames().map(Msg::Frame) } else { Subscription::none() },
            if self.video_ticking() {
                iced::time::every(Duration::from_millis(33)).map(|_| Msg::Video(Vm::Tick))
            } else {
                Subscription::none()
            },
            if self.chat.rec.is_some() || self.chat.player.is_some() {
                iced::time::every(Duration::from_millis(100)).map(|_| Msg::Chat(Cm::Nop))
            } else {
                Subscription::none()
            },
            iced::keyboard::listen().filter_map(|e| match e {
                iced::keyboard::Event::KeyPressed { key, modifiers, .. } => Some(Msg::Key(key, modifiers)),
                _ => None,
            }),
        ])
    }
}

/// Subtitle of the "You" conversation in the list and its header.
pub(super) const SELF_SUB: &str = "Saved items \u{b7} only on your devices";

fn self_contact(did: &str) -> Contact {
    Contact { did: did.to_string(), name: "You".into(), device: String::new(), added_at: 0, alias: None, verified: false, lost_touch: false }
}
