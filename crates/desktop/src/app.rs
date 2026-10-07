//! The iced application: onboarding, unlock, the two-pane home, call screens and settings.
//! State and updates live here; `view` builds the screens.

#[path = "view.rs"]
mod view;

#[path = "demo.rs"]
mod demo;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::operation;
use iced::{clipboard, system, theme, window, Size, Subscription, Task};
use p2pcore::{
    Availability, CallInfo, CallRecord, CallState, CallStats, Contact, Error, LockState, Node, NodeStatus,
};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::audio;
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
}

#[derive(PartialEq, Clone, Copy)]
enum Screen {
    Onboarding,
    Unlock,
    SetPass,
    Phrase,
    Home,
    AddContact,
    Settings,
}

/// What the home screen's right pane shows about the selected contact.
#[derive(PartialEq, Clone, Copy, Debug)]
enum Detail {
    View,
    Rename,
    Verify,
    ConfirmRemove,
}

/// Progress of adding a contact from a pasted card.
enum AddPhase {
    Idle,
    Connecting,
    Added(Contact),
    Failed(String),
}

/// A finished call, shown for a few seconds.
struct EndedView {
    reason: String,
    did: String,
    name: String,
    text: String,
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
    /// Test-hooks only: fake data for screenshots; the node is not asked for it.
    demo: bool,
    sel: Option<String>,
    detail: Detail,
    rename_in: String,
    search: String,
    recents: Vec<CallRecord>,
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
    pass2_in: Zeroizing<String>,
    old_in: Zeroizing<String>,
    /// Restoring over an existing (locked) identity, from the Unlock screen.
    replace: bool,
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
    busy: bool,
    notice: Option<String>,
    status: NodeStatus,
    contacts: Vec<Contact>,
    ticket: Option<String>,
    qr: Option<Qr>,
    add_in: String,
    call: Option<CallView>,
    early: Vec<(String, CallState)>,
    /// Call audio + ringer; driven by core events (see `audio::Ctl`), not by this loop.
    ctl: Arc<audio::Ctl>,
    settings: Settings,
    devices: (Vec<String>, Vec<String>),
    name_edit: String,
    ticks: u32,
    fetching: bool,
}

#[derive(Debug, Clone)]
enum Msg {
    Ev(EvBox),
    Tick,
    WindowOpened(#[allow(dead_code)] window::Id),
    CloseReq(window::Id),
    Closed(window::Id),
    Theme(theme::Mode),
    Quit,
    Exit,
    NameChanged(String),
    PassIn(String),
    Pass2In(String),
    OldIn(String),
    Create,
    Created(Result<String, String>),
    ToggleRestore,
    TogglePhraseShow,
    Devices((Vec<String>, Vec<String>)),
    PhraseChanged(String),
    Restore,
    Restored(Result<NodeBox, String>),
    Unlock,
    Unlocked(Result<(), String>),
    GoRestore,
    SetPassSubmit,
    PassSet(Result<(), String>),
    SkipSetPass,
    GoSetPass,
    ChangePassSubmit,
    PassChanged(Result<(), String>),
    ToggleReveal,
    RevealSubmit,
    Revealed(Result<String, String>),
    ToggleChange,
    HidePhrase,
    BackToUnlock,
    PhraseSaved,
    Started(Result<(), String>),
    Ticket(Result<String, String>),
    CopyTicket,
    AddChanged(String),
    AddPressed,
    Added(Result<Contact, String>),
    CallPressed(String),
    CallStarted(Result<CallInfo, String>),
    Remove(String),
    Done(Result<(), String>),
    Answer,
    Decline,
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
    ToggleAvail,
    SetAvail(bool, Option<u64>),
    OpenAdd,
    SettingsTab(u8),
    Key(iced::keyboard::Key, iced::keyboard::Modifiers),
    AddAlias(String),
    SaveAlias,
    CloseEnded,
    CallAgain(String),
    CopySafety,
    NameEdit(String),
    SaveName,
    InDev(String),
    OutDev(String),
    ToneToggled(bool),
}

/// A freshly built node, handed through the (Clone + Debug) message type.
#[derive(Clone)]
struct NodeBox(Arc<std::sync::Mutex<Option<Arc<Node>>>>);
impl std::fmt::Debug for NodeBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Node")
    }
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

/// A desktop notification, off the UI thread (some servers block on show).
fn notify(summary: &str, body: &str) {
    let (summary, body) = (summary.to_string(), body.to_string());
    std::thread::spawn(move || {
        let mut n = notify_rust::Notification::new();
        n.appname("Tinline").summary(&summary).body(&body);
        // Windows toasts are attributed to an AppUserModelID; an installer should register
        // a shortcut with the same id so they show under the app's name and icon.
        #[cfg(windows)]
        n.app_id("com.osvauld.tinline");
        let _ = n.show();
    });
}

fn s<E: ToString>(e: E) -> String {
    e.to_string()
}

/// Plain-language text for errors the passphrase screens can hit. Never includes the passphrase.
fn friendly(e: Error) -> String {
    match e {
        Error::WrongPassphrase => "That passphrase is not right. Try again.".into(),
        Error::WeakPassphrase => "Use a passphrase of at least 8 characters.".into(),
        Error::BadPhrase => "That recovery phrase is not valid. Check the 24 words.".into(),
        Error::Locked => "Unlock first.".into(),
        Error::HaveIdentity => "An identity already exists here.".into(),
        e => e.to_string(),
    }
}

/// Checks a new passphrase and its confirmation before the slow key derivation starts.
fn check_new(pass: &str, again: &str) -> Result<(), String> {
    if pass.chars().count() < 8 {
        Err("Use a passphrase of at least 8 characters.".into())
    } else if pass != again {
        Err("The two passphrases do not match.".into())
    } else {
        Ok(())
    }
}

impl App {
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
        let mut app = App {
            win: None,
            dark: forced_dark.unwrap_or(false),
            forced_dark,
            demo: false,
            sel: None,
            detail: Detail::View,
            rename_in: String::new(),
            search: String::new(),
            recents: node.recent_calls(RECENTS),
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
                LockState::NeedsPassphrase => Screen::SetPass,
                _ => Screen::Home,
            },
            name_in: String::new(),
            pass_in: Zeroizing::default(),
            pass2_in: Zeroizing::default(),
            old_in: Zeroizing::default(),
            replace: false,
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
            busy: false,
            notice: None,
            status: node.status(),
            contacts: node.contacts(),
            ticket: None,
            qr: None,
            add_in: String::new(),
            call: None,
            early: Vec::new(),
            ctl: INIT.get().unwrap().audio.clone(),
            settings,
            devices: (Vec::new(), Vec::new()),
            name_edit: node.profile().map(|p| p.name).unwrap_or_default(),
            ticks: 0,
            fetching: false,
            node,
        };
        if has && crate::test_env("P2P_SCREEN").is_some_and(|v| v == "settings") {
            app.screen = Screen::Settings;
            settings_devices = true;
        }
        *app.ctl.devices.lock().unwrap() = (app.settings.input.clone(), app.settings.output.clone());
        *app.ctl.tone.lock().unwrap() = app.tone_hz();
        let mut tasks = vec![
            system::theme().map(Msg::Theme),
        ];
        if settings_devices {
            tasks.push(blocking(audio::list_devices, Msg::Devices));
        }
        if !init.hidden {
            tasks.push(app.show_window());
        }
        if let Some(name) = crate::test_env("P2P_DEMO") {
            app.load_demo(&name);
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
        (app, Task::batch(tasks))
    }

    fn show_window(&mut self) -> Task<Msg> {
        if let Some(id) = self.win {
            return Task::batch([window::minimize(id, false), window::gain_focus(id)]);
        }
        let (id, task) = window::open(window::Settings {
            size: WINDOW,
            min_size: Some(Size::new(820.0, 560.0)),
            icon: crate::tray::window_icon(),
            ..Default::default()
        });
        self.win = Some(id);
        task.map(Msg::WindowOpened)
    }

    fn start_node(&self) -> Task<Msg> {
        let node = self.node.clone();
        blocking(move || node.start().map_err(s), Msg::Started)
    }

    fn fetch_ticket(&mut self) -> Task<Msg> {
        self.fetching = true;
        let node = self.node.clone();
        blocking(move || node.my_ticket().map_err(s), Msg::Ticket)
    }

    /// Writes the settings on a helper thread (tmp file + rename, so a crash never leaves half a
    /// file). The newest snapshot always wins even if two saves race.
    fn save_settings(&self) {
        static PENDING: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);
        static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());
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
    }

    fn hide_phrase(&mut self) {
        self.revealed = None;
        self.revealed_at = None;
        self.reveal_form = false;
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
                let presentation = reason::present(&reason, &who, c.info.incoming, c.answered);
                self.call = None;
                self.ctl.muted.store(false, Ordering::Relaxed);
                // The core logs the call before it says Ended, so the lists can be re-read now.
                self.refresh_history();
                match presentation {
                    End::Hidden => {}
                    End::Missed(text) => {
                        notify("Tinline", &text);
                        self.notice = Some(text);
                    }
                    End::Screen(text) => {
                        self.ended = Some(EndedView { reason: reason.clone(), did, name: who, text, direct, secs, at: Instant::now() });
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
        self.contacts.iter().find(|c| &c.did == did)
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
        self.notice = None;
        self.ended = None;
        let node = self.node.clone();
        blocking(move || node.call(did).map_err(s), Msg::CallStarted)
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        let identity_may_change = matches!(
            msg,
            Msg::Tick
                | Msg::Ev(_)
                | Msg::Created(_)
                | Msg::Restored(_)
                | Msg::Unlocked(_)
                | Msg::PassSet(_)
                | Msg::PassChanged(_)
                | Msg::Started(_)
                | Msg::Ticket(_)
        );
        let task = self.update_inner(msg);
        if identity_may_change {
            self.refresh_identity();
        }
        task
    }

    fn update_inner(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Ev(b) => {
                let Some(ev) = b.0.lock().unwrap().take() else { return Task::none() };
                return self.on_event(ev);
            }
            Msg::Tick => {
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
                if self.ended.as_ref().is_some_and(|e| e.at.elapsed() >= Duration::from_secs(ENDED_SECS)) {
                    self.ended = None;
                }
                crate::tray::set_status(match self.lock {
                    LockState::Locked => "Locked",
                    LockState::NeedsPassphrase => "Set a passphrase",
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
                }
            }
            Msg::WindowOpened(_) => {}
            Msg::CloseReq(id) => {
                if Some(id) == self.win {
                    self.hide_phrase();
                    if INIT.get().unwrap().tray {
                        self.win = None;
                        return window::close(id);
                    }
                    return window::minimize(id, true);
                }
            }
            Msg::Closed(id) => {
                if Some(id) == self.win {
                    self.hide_phrase();
                    self.win = None;
                }
            }
            Msg::Theme(m) => self.dark = self.forced_dark.unwrap_or(m != theme::Mode::Light),
            Msg::Quit => {
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
                if let Err(e) = check_new(&self.pass_in, &self.pass2_in) {
                    self.notice = Some(e);
                    return Task::none();
                }
                self.busy = true;
                self.notice = Some("Securing your identity...".into());
                let pass = take_secret(&mut self.pass_in);
                self.pass2_in.zeroize();
                let node = self.node.clone();
                return blocking(move || node.create_identity(name, pass).map_err(friendly), Msg::Created);
            }
            Msg::PassIn(v) => self.pass_in = Zeroizing::new(v),
            Msg::Pass2In(v) => self.pass2_in = Zeroizing::new(v),
            Msg::OldIn(v) => self.old_in = Zeroizing::new(v),
            Msg::Created(r) => {
                self.busy = false;
                self.notice = None;
                match r {
                    Ok(p) => {
                        self.new_phrase = Some(Zeroizing::new(p));
                        self.screen = Screen::Phrase;
                        self.name_edit = self.name_in.trim().to_string();
                    }
                    Err(e) => self.notice = Some(e),
                }
            }
            Msg::ToggleRestore => self.restore = !self.restore,
            Msg::PhraseChanged(v) => self.phrase_in = Zeroizing::new(v),
            Msg::TogglePhraseShow => self.show_phrase_in = !self.show_phrase_in,
            Msg::Restore => {
                let name = self.name_in.trim().to_string();
                if name.is_empty() || self.phrase_in.trim().is_empty() {
                    self.notice = Some("Enter your name and recovery phrase".into());
                    return Task::none();
                }
                if self.busy {
                    return Task::none();
                }
                if let Err(e) = check_new(&self.pass_in, &self.pass2_in) {
                    self.notice = Some(e);
                    return Task::none();
                }
                self.busy = true;
                self.notice = Some("Restoring...".into());
                let pass = take_secret(&mut self.pass_in);
                self.pass2_in.zeroize();
                // The typed phrase stays until the restore succeeds, so a wrong passphrase costs no retyping.
                let phrase = self.phrase_in.to_string();
                let (node, replace) = (self.node.clone(), self.replace);
                let (data, tx, ctl) = {
                    let i = INIT.get().unwrap();
                    (i.data.clone(), i.tx.clone(), i.audio.clone())
                };
                let old_did = self.node.profile().map(|p| p.did);
                return blocking(
                    move || {
                        if !replace {
                            node.restore_identity(phrase, name, pass).map_err(friendly)?;
                            return Ok(NodeBox(Arc::new(std::sync::Mutex::new(Some(node)))));
                        }
                        // A locked identity is in the way: set its file aside (still encrypted),
                        // restore into a fresh node, and put the file back if that fails.
                        let file = data.join("profile.json");
                        let aside = data.join(format!(
                            "profile.replaced-{}.json",
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0)
                        ));
                        std::fs::rename(&file, &aside).map_err(s)?;
                        let events = Arc::new(crate::Events { tx, audio: ctl.clone() });
                        let fresh = Node::new(data.to_string_lossy().into(), events)
                            .and_then(|n| n.restore_identity(phrase, name, pass).map(|_| n))
                            .map_err(friendly)
                            .and_then(|n| {
                                // Contacts in state.json belong to the locked identity.
                                if old_did.is_some() && n.profile().map(|p| p.did) != old_did {
                                    return Err("That recovery phrase belongs to a different identity".to_string());
                                }
                                Ok(n)
                            });
                        match fresh {
                            Ok(n) => {
                                ctl.attach(&n);
                                // The new profile is saved; the old one is encrypted under a
                                // passphrase nobody remembers.
                                let _ = std::fs::remove_file(&aside);
                                Ok(NodeBox(Arc::new(std::sync::Mutex::new(Some(n)))))
                            }
                            Err(e) => {
                                let _ = std::fs::rename(&aside, &file);
                                Err(e)
                            }
                        }
                    },
                    Msg::Restored,
                );
            }
            Msg::Restored(r) => {
                self.busy = false;
                match r {
                    Ok(b) => {
                        self.phrase_in.zeroize();
                        self.show_phrase_in = false;
                        if let Some(n) = b.0.lock().unwrap().take()
                            && !Arc::ptr_eq(&n, &self.node)
                        {
                            let old = std::mem::replace(&mut self.node, n);
                            // Dropping a node tears down its runtime; keep that off the UI loop.
                            std::thread::spawn(move || drop(old));
                        }
                        self.name_edit = self.name_in.trim().to_string();
                        self.replace = false;
                        self.screen = Screen::Home;
                        self.notice = None;
                        self.ticket = None;
                        self.qr = None;
                        return self.start_node();
                    }
                    Err(e) => self.notice = Some(e),
                }
            }
            Msg::Unlock => {
                if self.busy || self.pass_in.is_empty() {
                    return Task::none();
                }
                self.busy = true;
                self.notice = None;
                let pass = take_secret(&mut self.pass_in);
                let node = self.node.clone();
                return blocking(move || node.unlock(pass).map_err(friendly), Msg::Unlocked);
            }
            Msg::Unlocked(r) => {
                self.busy = false;
                match r {
                    Ok(()) => {
                        self.screen = Screen::Home;
                        self.notice = None;
                        self.name_edit = self.node.profile().map(|p| p.name).unwrap_or_default();
                        return self.start_node();
                    }
                    Err(e) => {
                        eprintln!("UNLOCK failed: {e}");
                        self.screen = Screen::Unlock;
                        self.notice = Some(e);
                    }
                }
            }
            Msg::GoRestore => {
                self.replace = true;
                self.restore = true;
                self.notice = None;
                self.pass_in.zeroize();
                self.screen = Screen::Onboarding;
            }
            Msg::BackToUnlock => {
                self.replace = false;
                self.restore = false;
                self.notice = None;
                self.pass_in.zeroize();
                self.pass2_in.zeroize();
                self.screen = Screen::Unlock;
            }
            Msg::GoSetPass => {
                self.notice = None;
                self.pass_in.zeroize();
                self.pass2_in.zeroize();
                self.screen = Screen::SetPass;
            }
            Msg::SkipSetPass => {
                self.notice = None;
                self.pass_in.zeroize();
                self.pass2_in.zeroize();
                self.screen = Screen::Home;
            }
            Msg::SetPassSubmit => {
                if self.busy {
                    return Task::none();
                }
                if let Err(e) = check_new(&self.pass_in, &self.pass2_in) {
                    self.notice = Some(e);
                    return Task::none();
                }
                self.busy = true;
                self.notice = Some("Securing your identity...".into());
                let pass = take_secret(&mut self.pass_in);
                self.pass2_in.zeroize();
                let node = self.node.clone();
                return blocking(move || node.set_passphrase(None, pass).map_err(friendly), Msg::PassSet);
            }
            Msg::PassSet(r) => {
                self.busy = false;
                match r {
                    Ok(()) => {
                        self.screen = Screen::Home;
                        self.notice = Some("Passphrase set. You will need it next time you open the app.".into());
                    }
                    Err(e) => self.notice = Some(e),
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
                if self.busy || self.pass_in.is_empty() {
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
                self.pass2_in.zeroize();
                self.notice = None;
            }
            Msg::ChangePassSubmit => {
                if self.busy || self.old_in.is_empty() {
                    return Task::none();
                }
                if let Err(e) = check_new(&self.pass_in, &self.pass2_in) {
                    self.notice = Some(e);
                    return Task::none();
                }
                self.busy = true;
                self.notice = Some("Changing passphrase...".into());
                let (old, new) = (take_secret(&mut self.old_in), take_secret(&mut self.pass_in));
                self.pass2_in.zeroize();
                let node = self.node.clone();
                return blocking(move || node.set_passphrase(Some(old), new).map_err(friendly), Msg::PassChanged);
            }
            Msg::PassChanged(r) => {
                self.busy = false;
                match r {
                    Ok(()) => {
                        self.hide_phrase();
                        self.change_form = false;
                        self.notice = Some("Passphrase changed".into());
                    }
                    Err(e) => self.notice = Some(e),
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
                self.contacts = self.node.contacts();
                match r {
                    Ok(()) => return self.fetch_ticket(),
                    Err(e) => self.notice = Some(format!("Could not start: {e}")),
                }
            }
            Msg::Ticket(r) => {
                self.fetching = false;
                match r {
                Ok(t) => {
                    crate::tlog!("TICKET {t}");
                    self.qr = view::qr_of(&t);
                    self.ticket = Some(t);
                }
                    Err(e) => self.notice = Some(format!("Ticket: {e}")),
                }
            }
            Msg::CopyTicket => {
                if let Some(t) = self.ticket.clone() {
                    self.notice = Some("Contact card copied".into());
                    return clipboard::write(t);
                }
            }
            Msg::AddChanged(v) => self.add_in = v,
            Msg::SettingsTab(n) => self.settings_tab = n,
            Msg::Key(key, mods) => {
                use iced::keyboard::key::Named;
                use iced::keyboard::Key;
                let ringing = self.call.as_ref().is_some_and(|c| c.info.incoming && !c.answered && c.state == CallState::Ringing);
                let active = self.call.as_ref().is_some_and(|c| c.state == CallState::Active);
                match key.as_ref() {
                    Key::Named(Named::Enter) if ringing => return self.update(Msg::Answer),
                    Key::Named(Named::Escape) if ringing => return self.update(Msg::Decline),
                    Key::Named(Named::Escape) if self.call.is_none() && (self.sel.is_some() || self.screen != Screen::Home) => {
                        return self.update(Msg::Home);
                    }
                    Key::Character("k") if mods.command() && self.call.is_none() => return operation::focus(SEARCH_ID),
                    Key::Character("m") if mods.command() && active => return self.update(Msg::ToggleMute),
                    Key::Character("e") if mods.command() && self.call.is_some() => return self.update(Msg::Hangup),
                    _ => {}
                }
            }
            Msg::AddAlias(v) => self.add_alias = v,
            Msg::OpenAdd => {
                self.screen = Screen::AddContact;
                self.add_phase = AddPhase::Idle;
                self.add_in.clear();
                self.add_alias.clear();
                self.notice = None;
            }
            Msg::AddPressed => {
                let t = self.add_in.trim().to_string();
                if t.is_empty() || matches!(self.add_phase, AddPhase::Connecting) {
                    return Task::none();
                }
                self.add_phase = AddPhase::Connecting;
                let node = self.node.clone();
                return blocking(move || node.add_contact(t).map_err(s), Msg::Added);
            }
            Msg::Added(r) => {
                self.contacts = self.node.contacts();
                match r {
                    Ok(c) => {
                        self.add_in.clear();
                        self.add_alias = c.name.clone();
                        self.add_phase = AddPhase::Added(c);
                    }
                    Err(e) => self.add_phase = AddPhase::Failed(e),
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
                    return blocking(
                        move || {
                            node.rename_contact(did, alias).map_err(s)
                        },
                        Msg::Done,
                    );
                }
            }
            Msg::CallPressed(did) => return self.dial(did),
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
                self.sel = Some(did);
                self.detail = Detail::View;
                self.safety = None;
                self.refresh_detail_calls();
                if self.screen != Screen::Home {
                    self.screen = Screen::Home;
                }
            }
            Msg::Home => {
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
            Msg::SearchIn(v) => self.search = v,
            Msg::ToggleAvail => self.avail_open = !self.avail_open,
            Msg::SetAvail(on, hours) => {
                let until = hours.map(|h| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                        + h * 3600
                });
                let node = self.node.clone();
                self.avail = Availability { available: on, until: if on { None } else { until } };
                return blocking(move || node.set_available(on, until).map_err(s), Msg::Done);
            }
            Msg::CloseEnded => self.ended = None,
            Msg::Remove(did) => {
                let node = self.node.clone();
                if self.sel.as_ref() == Some(&did) {
                    self.sel = None;
                    self.detail = Detail::View;
                    self.detail_calls.clear();
                }
                return blocking(move || node.remove_contact(did).map_err(s), Msg::Done);
            }
            Msg::Done(r) => {
                self.contacts = self.node.contacts();
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
                self.hide_phrase();
                self.change_form = false;
                self.pass_in.zeroize();
                self.pass2_in.zeroize();
                self.old_in.zeroize();
                self.notice = None;
                // Enumerating devices can block for a while (PulseAudio/WASAPI).
                return blocking(audio::list_devices, Msg::Devices);
            }
            Msg::Devices(d) => self.devices = d,
            Msg::Back => {
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
                let node = self.node.clone();
                self.notice = Some("Name saved".into());
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            node.set_name(name).map_err(s)?;
                            node.my_ticket().map_err(s)
                        })
                        .await
                        .unwrap_or_else(|e| Err(e.to_string()))
                    },
                    Msg::Ticket,
                );
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
                    self.contacts = self.node.contacts();
                    self.refresh_history();
                }
            }
            Ev::Incoming(info) => {
                let name = info.peer_name.clone();
                self.call = Some(CallView {
                    info: info.clone(),
                    state: CallState::Ringing,
                    answered: false,
                    stats: None,
                });
                self.ended = None;
                notify("Tinline", &format!("{name} is calling"));
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
            Ev::Tray(TrayCmd::Show) => return self.show_window(),
            Ev::Tray(TrayCmd::Quit) => return self.update(Msg::Quit),
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            Subscription::run(events),
            iced::time::every(Duration::from_secs(1)).map(|_| Msg::Tick),
            window::close_requests().map(Msg::CloseReq),
            window::close_events().map(Msg::Closed),
            system::theme_changes().map(Msg::Theme),
            iced::keyboard::listen().filter_map(|e| match e {
                iced::keyboard::Event::KeyPressed { key, modifiers, .. } => Some(Msg::Key(key, modifiers)),
                _ => None,
            }),
        ])
    }
}
