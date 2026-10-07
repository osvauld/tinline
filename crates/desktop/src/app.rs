//! The iced application: onboarding, home, call and settings screens.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use iced::widget::{
    button, canvas, checkbox, column, container, pick_list, row, scrollable,
    text, text_input, Space,
};
use iced::{
    clipboard, system, theme, window, Alignment, Color, Element, Fill, Length, Point, Rectangle,
    Renderer, Size, Subscription, Task, Theme,
};
use p2pcore::{CallInfo, CallState, CallStats, Contact, Error, LockState, Node, NodeStatus};
use serde::{Deserialize, Serialize};

use crate::audio::{self, Session};
use crate::tray::TrayCmd;
use crate::{Ev, INIT};

const DEFAULT_LABEL: &str = "System default";

pub fn run() -> iced::Result {
    iced::daemon(App::boot, App::update, App::view)
        .title(|_: &App, _| "Osvauld Calls".to_string())
        .theme(|a: &App, _| if a.dark { Theme::Dark } else { Theme::Light })
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
    Settings,
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
    screen: Screen,
    name_in: String,
    pass_in: String,
    pass2_in: String,
    old_in: String,
    /// Restoring over an existing (locked) identity, from the Unlock screen.
    replace: bool,
    revealed: Option<String>,
    reveal_form: bool,
    change_form: bool,
    restore: bool,
    phrase_in: String,
    new_phrase: Option<String>,
    busy: bool,
    notice: Option<String>,
    status: NodeStatus,
    contacts: Vec<Contact>,
    ticket: Option<String>,
    qr: Option<Qr>,
    add_in: String,
    call: Option<CallView>,
    early: Vec<(String, CallState)>,
    muted: Arc<AtomicBool>,
    audio: Option<Session>,
    ring: Option<Session>,
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
        let test_unlock = init.test_pass.clone().filter(|_| lock == LockState::Locked);
        let mut app = App {
            win: None,
            dark: true,
            screen: match lock {
                LockState::NoIdentity => Screen::Onboarding,
                LockState::Locked if test_unlock.is_none() => Screen::Unlock,
                LockState::NeedsPassphrase => Screen::SetPass,
                _ => Screen::Home,
            },
            name_in: String::new(),
            pass_in: String::new(),
            pass2_in: String::new(),
            old_in: String::new(),
            replace: false,
            revealed: None,
            reveal_form: false,
            change_form: false,
            restore: false,
            phrase_in: String::new(),
            new_phrase: None,
            busy: false,
            notice: None,
            status: node.status(),
            contacts: node.contacts(),
            ticket: None,
            qr: None,
            add_in: String::new(),
            call: None,
            early: Vec::new(),
            muted: Arc::new(AtomicBool::new(false)),
            audio: None,
            ring: None,
            settings,
            devices: (Vec::new(), Vec::new()),
            name_edit: node.profile().map(|p| p.name).unwrap_or_default(),
            ticks: 0,
            fetching: false,
            node,
        };
        if has && std::env::var("P2P_SCREEN").is_ok_and(|v| v == "settings") {
            app.devices = audio::list_devices();
            app.screen = Screen::Settings;
        }
        let mut tasks = vec![system::theme().map(Msg::Theme)];
        if !init.hidden {
            tasks.push(app.show_window());
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
            size: Size::new(460.0, 780.0),
            min_size: Some(Size::new(380.0, 560.0)),
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

    fn save_settings(&self) {
        if let Ok(b) = serde_json::to_vec_pretty(&self.settings) {
            let _ = std::fs::write(settings_path(), b);
        }
    }


    fn tone_hz(&self) -> Option<f32> {
        INIT.get().unwrap().env_tone.or(self.settings.tone.then_some(440.0))
    }

    fn apply_call_state(&mut self, id: &str, state: CallState) {
        let Some(c) = self.call.as_mut().filter(|c| c.info.call_id == id) else { return };
        c.state = state.clone();
        match state {
            CallState::Active => {
                c.answered = true;
                self.ring = None;
                self.muted.store(false, Ordering::Relaxed);
                self.node.set_test_tone(self.tone_hz());
                self.audio = Some(audio::start_call(
                    self.node.clone(),
                    self.settings.input.clone(),
                    self.settings.output.clone(),
                    self.muted.clone(),
                ));
            }
            CallState::Ended { reason } => {
                let missed = c.info.incoming && !c.answered;
                let who = c.info.peer_name.clone();
                self.audio = None;
                self.ring = None;
                self.node.set_test_tone(None);
                self.call = None;
                self.notice = Some(if missed {
                    format!("Missed call from {who}")
                } else {
                    format!("Call with {who} ended ({reason})")
                });
            }
            _ => {}
        }
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Ev(b) => {
                let Some(ev) = b.0.lock().unwrap().take() else { return Task::none() };
                return self.on_event(ev);
            }
            Msg::Tick => {
                self.ticks += 1;
                self.status = self.node.status();
                crate::tray::set_status(match self.node.lock_state() {
                    LockState::Locked => "Locked",
                    LockState::NeedsPassphrase => "Set a passphrase",
                    _ if !self.status.started => "Starting...",
                    _ if self.status.online => "Online",
                    _ => "Offline",
                });
                if let Some(c) = self.call.as_mut() {
                    c.stats = self.node.call_stats();
                    if let Some(st) = &c.stats
                        && st.state == CallState::Active
                    {
                        {
                            eprintln!(
                                "STATS t={} direct={} rtt={}ms sent={} recv={} lost={} recovered={} concealed={} buf={}ms freq={:.1} rms={:.0}",
                                st.secs, st.direct, st.rtt_ms, st.sent, st.received, st.lost,
                                st.recovered, st.concealed, st.buffered_ms, st.rx_freq_hz, st.rx_rms
                            );
                        }
                    }
                }
            }
            Msg::WindowOpened(_) => {}
            Msg::CloseReq(id) => {
                if Some(id) == self.win {
                    if INIT.get().unwrap().tray {
                        self.win = None;
                        return window::close(id);
                    }
                    return window::minimize(id, true);
                }
            }
            Msg::Closed(id) => {
                if Some(id) == self.win {
                    self.win = None;
                }
            }
            Msg::Theme(m) => self.dark = m != theme::Mode::Light,
            Msg::Quit => {
                let node = self.node.clone();
                self.audio = None;
                self.ring = None;
                return blocking(move || node.stop(), |_| Msg::Exit);
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
                let pass = std::mem::take(&mut self.pass_in);
                self.pass2_in.clear();
                let node = self.node.clone();
                return blocking(move || node.create_identity(name, pass).map_err(friendly), Msg::Created);
            }
            Msg::PassIn(v) => self.pass_in = v,
            Msg::Pass2In(v) => self.pass2_in = v,
            Msg::OldIn(v) => self.old_in = v,
            Msg::Created(r) => {
                self.busy = false;
                self.notice = None;
                match r {
                    Ok(p) => {
                        self.new_phrase = Some(p);
                        self.screen = Screen::Phrase;
                        self.name_edit = self.name_in.trim().to_string();
                    }
                    Err(e) => self.notice = Some(e),
                }
            }
            Msg::ToggleRestore => self.restore = !self.restore,
            Msg::PhraseChanged(v) => self.phrase_in = v,
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
                let pass = std::mem::take(&mut self.pass_in);
                self.pass2_in.clear();
                let phrase = std::mem::take(&mut self.phrase_in);
                let (node, replace) = (self.node.clone(), self.replace);
                let (data, tx) = {
                    let i = INIT.get().unwrap();
                    (i.data.clone(), i.tx.clone())
                };
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
                        let fresh = Node::new(data.to_string_lossy().into(), Arc::new(crate::Events { tx }))
                            .and_then(|n| n.restore_identity(phrase, name, pass).map(|_| n));
                        match fresh {
                            Ok(n) => Ok(NodeBox(Arc::new(std::sync::Mutex::new(Some(n))))),
                            Err(e) => {
                                let _ = std::fs::rename(&aside, &file);
                                Err(friendly(e))
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
                let pass = std::mem::take(&mut self.pass_in);
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
                self.pass_in.clear();
                self.screen = Screen::Onboarding;
            }
            Msg::BackToUnlock => {
                self.replace = false;
                self.restore = false;
                self.notice = None;
                self.pass_in.clear();
                self.pass2_in.clear();
                self.screen = Screen::Unlock;
            }
            Msg::GoSetPass => {
                self.notice = None;
                self.pass_in.clear();
                self.pass2_in.clear();
                self.screen = Screen::SetPass;
            }
            Msg::SkipSetPass => {
                self.notice = None;
                self.pass_in.clear();
                self.pass2_in.clear();
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
                let pass = std::mem::take(&mut self.pass_in);
                self.pass2_in.clear();
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
                self.reveal_form = !self.reveal_form;
                self.revealed = None;
                self.pass_in.clear();
                self.notice = None;
            }
            Msg::HidePhrase => {
                self.revealed = None;
                self.reveal_form = false;
            }
            Msg::RevealSubmit => {
                if self.busy || self.pass_in.is_empty() {
                    return Task::none();
                }
                self.busy = true;
                self.notice = None;
                let pass = std::mem::take(&mut self.pass_in);
                let node = self.node.clone();
                return blocking(move || node.recovery_phrase(pass).map_err(friendly), Msg::Revealed);
            }
            Msg::Revealed(r) => {
                self.busy = false;
                match r {
                    Ok(p) => self.revealed = Some(p),
                    Err(e) => self.notice = Some(e),
                }
            }
            Msg::ToggleChange => {
                self.change_form = !self.change_form;
                self.old_in.clear();
                self.pass_in.clear();
                self.pass2_in.clear();
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
                let (old, new) = (std::mem::take(&mut self.old_in), std::mem::take(&mut self.pass_in));
                self.pass2_in.clear();
                let node = self.node.clone();
                return blocking(move || node.set_passphrase(Some(old), new).map_err(friendly), Msg::PassChanged);
            }
            Msg::PassChanged(r) => {
                self.busy = false;
                match r {
                    Ok(()) => {
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
                    eprintln!("TICKET {t}");
                    self.qr = qr_of(&t);
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
            Msg::AddPressed => {
                let t = self.add_in.trim().to_string();
                if t.is_empty() || self.busy {
                    return Task::none();
                }
                self.busy = true;
                self.notice = Some("Adding contact...".into());
                let node = self.node.clone();
                return blocking(move || node.add_contact(t).map_err(s), Msg::Added);
            }
            Msg::Added(r) => {
                self.busy = false;
                self.contacts = self.node.contacts();
                match r {
                    Ok(c) => {
                        self.add_in.clear();
                        self.notice = Some(format!("Added {}", c.name));
                    }
                    Err(e) => self.notice = Some(format!("Could not add: {e}")),
                }
            }
            Msg::CallPressed(did) => {
                if self.call.is_some() {
                    return Task::none();
                }
                self.notice = None;
                let node = self.node.clone();
                return blocking(move || node.call(did).map_err(s), Msg::CallStarted);
            }
            Msg::CallStarted(r) => match r {
                Ok(info) => {
                    let id = info.call_id.clone();
                    self.call = Some(CallView { info, state: CallState::Dialing, answered: false, stats: None });
                    for (eid, st) in std::mem::take(&mut self.early) {
                        if eid == id {
                            self.apply_call_state(&eid, st);
                        }
                    }
                }
                Err(e) => self.notice = Some(format!("Could not call: {e}")),
            },
            Msg::Remove(did) => {
                let node = self.node.clone();
                return blocking(move || node.remove_contact(did).map_err(s), Msg::Done);
            }
            Msg::Done(r) => {
                self.contacts = self.node.contacts();
                if let Err(e) = r {
                    self.notice = Some(e);
                }
            }
            Msg::Answer => {
                if let Some(c) = &self.call {
                    self.ring = None;
                    let (node, id) = (self.node.clone(), c.info.call_id.clone());
                    return blocking(move || node.answer(id).map_err(s), Msg::Done);
                }
            }
            Msg::Decline => {
                if let Some(c) = &self.call {
                    self.ring = None;
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
                let m = !self.muted.load(Ordering::Relaxed);
                self.muted.store(m, Ordering::Relaxed);
            }
            Msg::OpenSettings => {
                self.devices = audio::list_devices();
                self.screen = Screen::Settings;
                self.revealed = None;
                self.reveal_form = false;
                self.change_form = false;
                self.pass_in.clear();
                self.pass2_in.clear();
                self.old_in.clear();
                self.notice = None;
            }
            Msg::Back => {
                self.screen = Screen::Home;
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
                self.save_settings();
            }
            Msg::OutDev(d) => {
                self.settings.output = (d != DEFAULT_LABEL).then_some(d);
                self.save_settings();
            }
            Msg::ToneToggled(on) => {
                self.settings.tone = on;
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
            Ev::Contacts => self.contacts = self.node.contacts(),
            Ev::Incoming(info) => {
                let name = info.peer_name.clone();
                let busy = self.call.is_some();
                self.call = Some(CallView {
                    info: info.clone(),
                    state: CallState::Ringing,
                    answered: false,
                    stats: None,
                });
                if !busy {
                    self.ring = Some(audio::start_ring(self.settings.output.clone()));
                }
                std::thread::spawn(move || {
                    let _ = notify_rust::Notification::new()
                        .appname("Osvauld Calls")
                        .summary("Incoming call")
                        .body(&format!("{name} is calling"))
                        .show();
                });
                let mut tasks = vec![self.show_window()];
                if let Some(secs) = INIT.get().unwrap().auto_answer {
                    let (node, id) = (self.node.clone(), info.call_id);
                    self.ring = None;
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
                    self.apply_call_state(&id, st);
                } else {
                    // An outgoing call's first states can beat the reply to `call`.
                    self.early.push((id, st));
                    let n = self.early.len();
                    if n > 16 {
                        self.early.drain(..n - 16);
                    }
                }
            }
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
        ])
    }

    // ---- views ----

    fn view(&self, _id: window::Id) -> Element<'_, Msg> {
        let body: Element<Msg> = if self.call.is_some() {
            self.call_view()
        } else {
            match self.screen {
                Screen::Onboarding => self.onboarding_view(),
                Screen::Unlock => self.unlock_view(),
                Screen::SetPass => self.setpass_view(),
                Screen::Phrase => self.phrase_view(),
                Screen::Home => self.home_view(),
                Screen::Settings => self.settings_view(),
            }
        };
        container(
            container(scrollable(container(body).padding(20).width(Fill)).width(Fill).height(Fill))
                .max_width(480)
                .height(Fill),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .into()
    }

    fn notice_view(&self) -> Element<'_, Msg> {
        match &self.notice {
            Some(n) => container(text(n.clone()).size(14))
                .padding([8, 12])
                .width(Fill)
                .style(container::rounded_box)
                .into(),
            None => Space::new().into(),
        }
    }

    fn pass_field<'a>(&'a self, label: &'a str, value: &'a str, on: fn(String) -> Msg, submit: Option<Msg>) -> Element<'a, Msg> {
        let mut input = text_input("", value).secure(true).on_input(on).padding(10);
        if let Some(m) = submit {
            input = input.on_submit(m);
        }
        column![text(label).size(13).style(text::secondary), input].spacing(4).into()
    }

    fn new_pass_fields(&self, submit: Msg) -> Element<'_, Msg> {
        column![
            self.pass_field("Passphrase (at least 8 characters)", &self.pass_in, Msg::PassIn, None),
            self.pass_field("Repeat passphrase", &self.pass2_in, Msg::Pass2In, Some(submit)),
        ]
        .spacing(8)
        .into()
    }

    fn onboarding_view(&self) -> Element<'_, Msg> {
        let mut col = column![
            Space::new().height(30),
            text("Osvauld Calls").size(30),
            text("Private voice calls, directly between devices.").size(15).style(text::secondary),
            Space::new().height(10),
            text("Your name").size(13).style(text::secondary),
            text_input("e.g. Abe", &self.name_in).on_input(Msg::NameChanged).padding(10),
        ]
        .spacing(8);
        if self.restore {
            col = col
                .push(text("Recovery phrase (24 words)").size(13).style(text::secondary))
                .push(
                    text_input("word word word ...", &self.phrase_in)
                        .on_input(Msg::PhraseChanged)
                        .padding(10),
                );
            if self.replace {
                col = col.push(
                    text("This replaces the locked identity on this computer. Its encrypted file is kept aside, not deleted.")
                        .size(13)
                        .style(text::danger),
                );
            }
        }
        col = col
            .push(self.new_pass_fields(if self.restore { Msg::Restore } else { Msg::Create }))
            .push(
                text("This passphrase locks your recovery phrase and keys on this computer. You type it each time the app starts. It cannot be recovered: if you forget it, you can only restore with your recovery phrase.")
                    .size(13)
                    .style(text::secondary),
            )
            .push(Space::new().height(6));
        if self.restore {
            col = col
                .push(big_button("Restore", (!self.busy).then_some(Msg::Restore)))
                .push(
                    button(text(if self.replace { "Back to unlock" } else { "Create a new identity instead" }))
                        .style(button::text)
                        .on_press(if self.replace { Msg::BackToUnlock } else { Msg::ToggleRestore }),
                );
        } else {
            col = col
                .push(big_button("Create identity", (!self.busy).then_some(Msg::Create)))
                .push(button(text("Restore from recovery phrase")).style(button::text).on_press(Msg::ToggleRestore));
        }
        col.push(self.notice_view()).into()
    }

    fn unlock_view(&self) -> Element<'_, Msg> {
        let name = self.node.profile().map(|p| p.name).unwrap_or_default();
        column![
            Space::new().height(40),
            text("Osvauld Calls").size(30),
            text(format!("Locked. Enter the passphrase for {name} to go online.")).size(15).style(text::secondary),
            Space::new().height(10),
            self.pass_field("Passphrase", &self.pass_in, Msg::PassIn, Some(Msg::Unlock)),
            big_button(if self.busy { "Unlocking..." } else { "Unlock" }, (!self.busy).then_some(Msg::Unlock)),
            self.notice_view(),
            Space::new().height(10),
            text("Forgot it? The passphrase cannot be recovered, but your 24-word recovery phrase can restore this identity.")
                .size(12)
                .style(text::secondary),
            button(text("Restore from recovery phrase")).style(button::text).on_press_maybe((!self.busy).then_some(Msg::GoRestore)),
        ]
        .spacing(10)
        .into()
    }

    fn setpass_view(&self) -> Element<'_, Msg> {
        column![
            Space::new().height(30),
            text("Set a passphrase").size(26),
            text("Your recovery phrase and keys are currently stored on this computer without protection. Choose a passphrase to encrypt them.")
                .size(14)
                .style(text::secondary),
            self.new_pass_fields(Msg::SetPassSubmit),
            text("You will type it each time the app starts. It cannot be recovered; your recovery phrase can still restore your identity if you forget it.")
                .size(13)
                .style(text::secondary),
            big_button(if self.busy { "Working..." } else { "Set passphrase" }, (!self.busy).then_some(Msg::SetPassSubmit)),
            button(text("Not now")).style(button::text).on_press_maybe((!self.busy).then_some(Msg::SkipSetPass)),
            self.notice_view(),
        ]
        .spacing(12)
        .into()
    }

    fn phrase_view(&self) -> Element<'_, Msg> {
        let phrase = self.new_phrase.clone().unwrap_or_default();
        column![
            Space::new().height(30),
            text("Your recovery phrase").size(26),
            text("Write these 24 words down and keep them safe. They are the only way to restore your identity on another device.")
                .size(14)
                .style(text::secondary),
            container(text(phrase).size(17)).padding(16).width(Fill).style(container::bordered_box),
            big_button("I have saved it", Some(Msg::PhraseSaved)),
        ]
        .spacing(14)
        .into()
    }

    fn home_view(&self) -> Element<'_, Msg> {
        let pill: Element<Msg> = if !self.status.started {
            text("Starting...").size(13).style(text::secondary).into()
        } else if self.status.online {
            text("● Online")
                .size(14)
                .style(text::success)
                .into()
        } else {
            text("● Offline").size(14).style(text::danger).into()
        };
        let header = row![
            column![
                text(self.node.profile().map(|p| p.name).unwrap_or_default()).size(24),
                pill,
            ]
            .spacing(6),
            Space::new().width(Fill),
            button(text("Settings")).style(ghost).on_press(Msg::OpenSettings),
        ]
        .align_y(Alignment::Center);

        let card: Element<Msg> = {
            let qr: Element<Msg> = match &self.qr {
                Some(q) => container(
                    canvas(QrView(q)).width(Fill).height(Length::Fixed(360.0)),
                )
                .center_x(Fill)
                .into(),
                None => text("Preparing your card...").size(14).style(text::secondary).into(),
            };
            container(
                column![
                    text("My contact card").size(17),
                    text("Show this QR code or send the code to someone who should be able to call you.")
                        .size(13)
                        .style(text::secondary),
                    qr,
                    button(text("Copy contact code").center().width(Fill))
                        .padding(10)
                        .width(Fill)
                        .style(ghost)
                        .on_press_maybe(self.ticket.as_ref().map(|_| Msg::CopyTicket)),
                ]
                .spacing(10),
            )
            .padding(16)
            .width(Fill)
            .style(container::bordered_box)
            .into()
        };

        let mut list = column![text("Contacts").size(17)].spacing(8);
        if self.contacts.is_empty() {
            list = list.push(text("No contacts yet. Paste a contact code below.").size(14).style(text::secondary));
        }
        for c in &self.contacts {
            list = list.push(
                container(
                    row![
                        column![
                            text(c.name.clone()).size(16),
                            text(format!("device {}", &c.device[..c.device.len().min(8)])).size(11).style(text::secondary),
                        ]
                        .width(Fill),
                        button(text("Call")).style(button::success).on_press(Msg::CallPressed(c.did.clone())),
                        button(text("Remove")).style(button::text).on_press(Msg::Remove(c.did.clone())),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .style(container::rounded_box),
            );
        }

        let add = column![
            text("Add contact").size(17),
            row![
                text_input("Paste a contact code (OSVC2:...)", &self.add_in)
                    .on_input(Msg::AddChanged)
                    .on_submit(Msg::AddPressed)
                    .padding(10),
                button(text("Add")).padding(10).on_press_maybe((!self.busy).then_some(Msg::AddPressed)),
            ]
            .spacing(8),
        ]
        .spacing(8);

        let banner: Element<Msg> = if self.node.lock_state() == LockState::NeedsPassphrase {
            container(
                column![
                    text("Protect your identity").size(17),
                    text("Your recovery phrase and keys are stored without a passphrase. Set one to encrypt them.")
                        .size(13),
                    button(text("Set a passphrase")).style(button::primary).on_press(Msg::GoSetPass),
                ]
                .spacing(8),
            )
            .padding(16)
            .width(Fill)
            .style(container::danger)
            .into()
        } else {
            Space::new().into()
        };
        column![header, banner, self.notice_view(), card, list, add].spacing(18).into()
    }

    fn call_view(&self) -> Element<'_, Msg> {
        let c = self.call.as_ref().unwrap();
        let secs = c.stats.as_ref().map(|s| s.secs).unwrap_or(0);
        let state = match (&c.state, c.info.incoming) {
            (CallState::Dialing, _) => "Calling...".to_string(),
            (CallState::Ringing, true) => "Incoming call".to_string(),
            (CallState::Ringing, false) => "Ringing...".to_string(),
            (CallState::Active, _) => format!("{:02}:{:02}", secs / 60, secs % 60),
            (CallState::Ended { reason }, _) => format!("Ended ({reason})"),
        };
        let stats = match (&c.state, &c.stats) {
            (CallState::Active, Some(st)) => {
                let total = st.received + st.lost;
                let loss = if total > 0 { st.lost as f64 * 100.0 / total as f64 } else { 0.0 };
                format!(
                    "{} - rtt {} ms - loss {:.1}%",
                    if st.direct { "Direct P2P" } else { "Relayed (encrypted)" },
                    st.rtt_ms,
                    loss
                )
            }
            _ => String::new(),
        };
        let ringing_in = c.info.incoming && !c.answered && c.state == CallState::Ringing;
        let actions: Element<Msg> = if ringing_in {
            row![
                big_button_style("Decline", Some(Msg::Decline), button::danger),
                big_button_style("Answer", Some(Msg::Answer), button::success),
            ]
            .spacing(14)
            .into()
        } else {
            let muted = self.muted.load(Ordering::Relaxed);
            row![
                big_button_style(
                    if muted { "Unmute" } else { "Mute" },
                    (c.state == CallState::Active).then_some(Msg::ToggleMute),
                    if muted { button::primary } else { ghost },
                ),
                big_button_style("Hang up", Some(Msg::Hangup), button::danger),
            ]
            .spacing(14)
            .into()
        };
        column![
            Space::new().height(60),
            container(
                text(c.info.peer_name.chars().next().map(|ch| ch.to_uppercase().to_string()).unwrap_or_default())
                    .size(48)
            )
            .width(110)
            .height(110)
            .center_x(110)
            .center_y(110)
            .style(container::bordered_box),
            text(c.info.peer_name.clone()).size(30),
            text(state).size(20).style(text::secondary),
            text(stats).size(13).style(text::secondary),
            Space::new().height(40),
            actions,
            self.notice_view(),
        ]
        .spacing(12)
        .align_x(Alignment::Center)
        .width(Fill)
        .into()
    }

    fn settings_view(&self) -> Element<'_, Msg> {
        let opts = |v: &Vec<String>| {
            let mut o = vec![DEFAULT_LABEL.to_string()];
            o.extend(v.iter().cloned());
            o
        };
        let sel = |v: &Option<String>| Some(v.clone().unwrap_or_else(|| DEFAULT_LABEL.to_string()));
        let legacy = self.node.lock_state() == LockState::NeedsPassphrase;
        let security: Element<Msg> = if legacy {
            column![
                text("Your keys are not protected by a passphrase yet.").size(13).style(text::danger),
                button(text("Set a passphrase")).style(button::primary).on_press(Msg::GoSetPass),
            ]
            .spacing(8)
            .into()
        } else {
            let mut c = column![].spacing(8);
            if let Some(p) = &self.revealed {
                c = c
                    .push(container(text(p.clone()).size(15)).padding(14).width(Fill).style(container::bordered_box))
                    .push(button(text("Hide recovery phrase")).style(ghost).on_press(Msg::HidePhrase));
            } else if self.reveal_form {
                c = c
                    .push(self.pass_field("Passphrase", &self.pass_in, Msg::PassIn, Some(Msg::RevealSubmit)))
                    .push(
                        row![
                            button(text(if self.busy { "Checking..." } else { "Show" }))
                                .style(button::primary)
                                .on_press_maybe((!self.busy).then_some(Msg::RevealSubmit)),
                            button(text("Cancel")).style(ghost).on_press(Msg::ToggleReveal),
                        ]
                        .spacing(8),
                    );
            } else {
                c = c.push(button(text("Show recovery phrase")).style(ghost).on_press(Msg::ToggleReveal));
            }
            if self.change_form {
                c = c
                    .push(self.pass_field("Current passphrase", &self.old_in, Msg::OldIn, None))
                    .push(self.new_pass_fields(Msg::ChangePassSubmit))
                    .push(
                        row![
                            button(text(if self.busy { "Working..." } else { "Change passphrase" }))
                                .style(button::primary)
                                .on_press_maybe((!self.busy).then_some(Msg::ChangePassSubmit)),
                            button(text("Cancel")).style(ghost).on_press(Msg::ToggleChange),
                        ]
                        .spacing(8),
                    );
            } else {
                c = c.push(button(text("Change passphrase")).style(ghost).on_press(Msg::ToggleChange));
            }
            c.into()
        };
        column![
            row![
                button(text("Back")).style(ghost).on_press(Msg::Back),
                text("Settings").size(24),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
            self.notice_view(),
            text("Name").size(13).style(text::secondary),
            row![
                text_input("Your name", &self.name_edit).on_input(Msg::NameEdit).on_submit(Msg::SaveName).padding(10),
                button(text("Save")).padding(10).on_press(Msg::SaveName),
            ]
            .spacing(8),
            text("Microphone").size(13).style(text::secondary),
            pick_list(opts(&self.devices.0), sel(&self.settings.input), Msg::InDev).width(Fill).padding(8),
            text("Speaker").size(13).style(text::secondary),
            pick_list(opts(&self.devices.1), sel(&self.settings.output), Msg::OutDev).width(Fill).padding(8),
            text("Device changes apply from the next call.").size(12).style(text::secondary),
            checkbox(self.settings.tone).label("Send a test tone instead of the microphone").on_toggle(Msg::ToneToggled),
            security,
            Space::new().height(10),
            button(text("Quit Osvauld Calls")).style(button::danger).on_press(Msg::Quit),
        ]
        .spacing(10)
        .into()
    }
}

fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let pair = match status {
        button::Status::Hovered | button::Status::Pressed => p.background.strong,
        _ => p.background.weak,
    };
    let dim = if status == button::Status::Disabled { 0.5 } else { 1.0 };
    button::Style {
        background: Some(pair.color.into()),
        text_color: Color { a: dim, ..p.background.base.text },
        border: iced::Border { radius: 8.0.into(), ..Default::default() },
        ..Default::default()
    }
}

fn big_button(label: &str, on: Option<Msg>) -> Element<'_, Msg> {
    big_button_style(label, on, button::primary)
}

fn big_button_style<'a>(
    label: &'a str,
    on: Option<Msg>,
    style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> Element<'a, Msg> {
    button(text(label).size(16).center().width(Fill))
        .padding(12)
        .width(Fill)
        .style(style)
        .on_press_maybe(on)
        .into()
}

fn qr_of(t: &str) -> Option<Qr> {
    let code = qrcode::QrCode::new(t.as_bytes()).ok()?;
    Some(Qr {
        n: code.width(),
        dark: code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect(),
    })
}

struct QrView<'a>(&'a Qr);

impl canvas::Program<Msg> for QrView<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::WHITE);
        let quiet = 2usize;
        let total = self.0.n + 2 * quiet;
        let cell = (bounds.width.min(bounds.height) / total as f32).floor().max(1.0);
        let off = (bounds.width.min(bounds.height) - cell * total as f32) / 2.0;
        for y in 0..self.0.n {
            for x in 0..self.0.n {
                if self.0.dark[y * self.0.n + x] {
                    frame.fill_rectangle(
                        Point::new(off + (x + quiet) as f32 * cell, off + (y + quiet) as f32 * cell),
                        Size::new(cell, cell),
                        Color::BLACK,
                    );
                }
            }
        }
        vec![frame.into_geometry()]
    }
}
