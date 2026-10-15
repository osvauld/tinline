//! Sign-up (the stepped gate) and the first landing: state and updates. A port of the playground's
//! `screens/gate/*` (Gate.svelte, gate.svelte.ts) and `screens/people/AddContact.svelte`; the decisions
//! and their reasons are in docs/design/SIGNUP-HANDOFF.md. The screens are in `signup_view.rs`.
//!
//! Create: Welcome → Name → Passphrase (the identity is made, in memory) → Recovery words (+ Check my
//! copy) → Name this computer (the key is saved and the account committed, as before) → Before you
//! start → Done. Restore and Link keep their existing screens; Welcome's "I have an account" reaches
//! them through Choose.

use std::time::{Duration, Instant};

use iced::{clipboard, Task};
use zeroize::Zeroizing;

use super::{App, Msg, Screen};

/// One step of the create flow (gate.svelte.ts `Step`, desktop subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Welcome,
    Choose,
    Name,
    Pass,
    Phrase,
    Confirm,
    Device,
    Terms,
    Done,
    /// Restore: the 24 recovery words (Restore.svelte). Then Name and Pass in the restore flow.
    Restore,
}

/// 1-based word positions asked on Check my copy.
pub(super) const CONFIRM_AT: [usize; 3] = [4, 11, 19];

/// The create order for the progress header. Desktop has no Permissions step (nothing on a computer
/// asks for one), so it counts five; see SCREENS-PORT-STATUS.md.
const ORDER: [Step; 5] = [Step::Name, Step::Pass, Step::Phrase, Step::Device, Step::Terms];
/// gate.svelte.ts `progressOf` restore: restore, restoreName, restorePass, device, terms (no perms on a computer).
const ORDER_RESTORE: [Step; 5] = [Step::Restore, Step::Name, Step::Pass, Step::Device, Step::Terms];

/// The BIP39 English list (Restore.svelte WORDS).
pub(super) fn words() -> &'static [&'static str; 2048] {
    bip39::Language::English.word_list()
}
pub(super) fn is_word(w: &str) -> bool {
    words().binary_search(&w).is_ok()
}
/// wordlist.ts `suggest`: the first `n` words starting with `p`.
pub(super) fn suggest(p: &str, n: usize) -> Vec<&'static str> {
    if p.is_empty() { return Vec::new(); }
    words().iter().filter(|w| w.starts_with(p)).take(n).copied().collect()
}
pub(super) fn word_id(i: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("gate-rw-{i}"))
}

pub(super) const NAME_ID: &str = "gate-name";
pub(super) const PASS_ID: &str = "gate-pass";
pub(super) const DEVICE_ID: &str = "gate-device";
pub(super) const CONFIRM_ID: [&str; 3] = ["gate-w1", "gate-w2", "gate-w3"];

pub(super) const URL_TERMS: &str = "https://tinline.osvauld.com/terms";
pub(super) const URL_PRIVACY: &str = "https://tinline.osvauld.com/privacy";

/// Done: the line arrives slack and pulls tight this long after the step arrives.
const DONE_PULL: Duration = Duration::from_millis(450);

pub(super) struct Gate {
    pub step: Step,
    history: Vec<Step>,
    /// "Copy my words" was pressed: the same button now says "I've saved them".
    pub copied: bool,
    pub confirm: [String; 3],
    pub confirm_tried: bool,
    pub show_pass: bool,
    /// Unlock: the "Open as" menu under "Welcome back, Maya ⌄" is open.
    pub open_as: bool,
    /// Recovery step: show the recovery key (one line) instead of the 24 words.
    pub as_key: bool,
    /// Restore: a pasted recovery key instead of the 24 words ("" = typing words).
    pub rec_key: String,
    /// A create flow is between "the identity exists in memory" and Done.
    pub creating: bool,
    /// Done: the line has been told to pull tight.
    pub taut: bool,
    /// Tugs reported by the Done mark (the `line` cue plays on the first).
    pub tugs: u32,
    /// The restore flow (Restore → Name → Pass → Device → Terms → Done).
    pub restoring: bool,
    /// Restore: the 24 words, the box last typed in (suggestions follow it), and whether Continue was tried.
    pub words: [String; 24],
    pub active: Option<usize>,
    pub tried: bool,
}

impl Default for Gate {
    fn default() -> Self {
        Gate { step: Step::Welcome, history: Vec::new(), copied: false, confirm: Default::default(), confirm_tried: false, show_pass: false, open_as: false, as_key: false, rec_key: String::new(), creating: false, taut: false, tugs: 0,
            restoring: false, words: Default::default(), active: None, tried: false }
    }
}

impl Gate {
    /// [n, total] for the header; Welcome and Choose are step 0 (all segments empty, the count
    /// hidden but keeping its width). None: no progress shown.
    pub fn progress(&self) -> Option<(usize, usize)> {
        let order: &[Step] = if self.restoring { &ORDER_RESTORE } else { &ORDER };
        match self.step {
            Step::Welcome | Step::Choose => Some((0, order.len())),
            s => order.iter().position(|o| *o == s).map(|i| (i + 1, order.len())),
        }
    }

    /// Steps that cannot be backed out of (gate.svelte.ts `canBack`).
    pub fn can_back(&self) -> bool {
        !matches!(self.step, Step::Welcome | Step::Phrase | Step::Terms | Step::Done) && !self.history.is_empty()
    }

    fn go(&mut self, s: Step) {
        self.history.push(self.step);
        self.step = s;
    }

    fn back(&mut self) {
        if let Some(to) = self.history.pop() {
            self.step = to;
        }
    }

    /// Restore.svelte `bad(i)`: a word that isn't in the list, once tried or when no word starts that way.
    pub fn bad_word(&self, i: usize) -> bool {
        let w = &self.words[i];
        !w.is_empty() && !is_word(w) && (self.tried || suggest(w, 1).is_empty())
    }
    pub fn filled(&self) -> usize {
        self.words.iter().filter(|w| !w.is_empty()).count()
    }

    /// Whether word `i` of Check my copy is wrong (only after a try).
    pub fn wrong(&self, i: usize, phrase: Option<&str>) -> bool {
        let Some(p) = phrase else { return false };
        let want = p.split_whitespace().nth(CONFIRM_AT[i] - 1).unwrap_or("");
        self.confirm_tried && self.confirm[i].trim().to_lowercase() != want
    }
}

/// Passphrase strength, the playground's rule: 4 words or 16 characters is strong, 8 is fair.
pub(super) fn strength(p: &str) -> usize {
    if p.is_empty() {
        return 0;
    }
    let words = p.split_whitespace().count();
    if p.chars().count() >= 16 || words >= 4 {
        3
    } else if p.chars().count() >= 8 {
        2
    } else {
        1
    }
}

/// The add-contact dialog's two ways (AddContact.svelte `TABS`), By code first and the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum AddTab {
    #[default]
    Code,
    Qr,
}

#[derive(Default)]
pub(super) struct AddUi {
    pub tab: AddTab,
    /// The polite hint under "Paste their code" (muted, never red).
    pub hint: String,
    /// A card found on the clipboard, previewed without dialling (`peek_code`).
    pub found: Option<p2pcore::CardPeek>,
    /// The sheet as the other side of a handshake (people.svelte.ts `pp.incoming`): someone used
    /// our code ("Contact request"), or accepted ours while the sheet was closed ("New contact").
    pub incoming: Option<Incoming>,
    /// Accept is running (the button waits).
    pub answering: bool,
}

/// `pp.incoming`: whose line, and where the handshake is.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Incoming {
    pub did: String,
    pub name: String,
    /// `phase === 'joined'`: accepted, the line is open (taut). Else asking (ringing).
    pub joined: bool,
    /// `kind === 'accepted'`: they accepted the request we sent (chip "Accepted your request").
    pub theirs: bool,
}

#[derive(Debug, Clone)]
pub(super) enum Gs {
    Start,
    Choose,
    Back,
    Theme,
    NameGo,
    PassGo,
    PassSkip,
    ShowPass,
    OpenAs,
    AsKey(bool),
    ClearKey,
    CopyWords,
    Saved,
    Check,
    ConfirmIn(usize, String),
    ConfirmGo,
    BackToWords,
    DeviceSug(&'static str),
    Agree,
    Open(&'static str),
    /// Done: time to pull the line tight.
    Pull,
    /// Done: the mark's string went tight (one per tug).
    Tug,
    AddFirst,
    NotNow,
    /// Enter outside a text field: the step's main action.
    Enter,
    // Add contact
    AcTab(AddTab),
    AcCopy,
    AcPaste,
    /// "New code": drop the code we hand out (core `reset_ticket`); the next `my_ticket` mints a fresh one.
    AcNewCode,
    AcPasted(Option<String>),
    AcBack,
    AcAdd,
    AcMessage,
    AcProfile,
    /// The request sheet: Accept / Not now / Message {first} (after Accept, or theirs accepted).
    AcAccept,
    AcNotNow,
    AcIncomingMessage,
    /// A Contacts › "Waiting for you" card's Accept / Not now.
    CardAccept(String),
    CardNotNow(String),
    // Restore
    Word(usize, String),
    WordNext(usize),
    WordPick(&'static str),
    WordsPaste,
    WordsPasted(Option<String>),
    RestoreGo,
}

/// The first word of a name, for "Add Tara" and the like.
pub(super) fn first_name(name: &str) -> String {
    name.split_whitespace().next().unwrap_or(name).to_string()
}

impl App {
    /// Back to the start of sign-up (a fresh install, or "Add an account").
    pub(super) fn gate_reset(&mut self) {
        self.gate = Gate::default();
    }

    /// Starts the restore flow at the 24 words (Choose › Restore my account, or "Add an account" › restore).
    pub(super) fn gate_restore(&mut self) -> Task<Msg> {
        self.gate.restoring = true;
        self.gate.words = Default::default();
        self.gate.tried = false;
        self.gate.active = None;
        self.gate_go(Step::Restore)
    }

    pub(super) fn gate_focus(&self) -> Task<Msg> {
        match self.gate.step {
            Step::Restore => iced::widget::operation::focus(word_id(0)),
            Step::Name => iced::widget::operation::focus(NAME_ID),
            Step::Pass => iced::widget::operation::focus(PASS_ID),
            Step::Device => iced::widget::operation::focus(DEVICE_ID),
            Step::Confirm => iced::widget::operation::focus(CONFIRM_ID[0]),
            _ => Task::none(),
        }
    }

    pub(super) fn gate_go_pub(&mut self, s: Step) -> Task<Msg> {
        self.gate_go(s)
    }

    fn gate_go(&mut self, s: Step) -> Task<Msg> {
        self.notice = None;
        self.gate.go(s);
        if s == Step::Done {
            self.gate.taut = false;
            self.gate.tugs = 0;
            return Task::perform(tokio::time::sleep(DONE_PULL), |_| Msg::Gate(Gs::Pull));
        }
        self.gate_focus()
    }

    /// Identity made (in memory): the recovery words come next. The key is saved after the device
    /// is named, exactly as before.
    pub(super) fn gate_created(&mut self, phrase: String) -> Task<Msg> {
        self.setup = Some(super::Setup { phrase: None });
        self.new_phrase = Some(Zeroizing::new(phrase));
        self.gate.creating = true;
        self.gate.copied = false;
        if self.device_in.trim().is_empty() {
            self.device_in = crate::flow::suggest_label();
        }
        if self.device_in.trim().is_empty() {
            self.device_in = "Home computer".into();
        }
        self.gate_go(Step::Phrase)
    }

    /// The account is saved: on to the terms; the node starts so the first code is ready on Done.
    pub(super) fn gate_saved(&mut self) -> Task<Msg> {
        self.screen = Screen::Onboarding;
        let t = self.gate_go(Step::Terms);
        Task::batch([t, self.start_node()])
    }

    /// Leaves sign-up for the app. `add`: with Add contact open over Chats, on By code.
    fn gate_finish(&mut self, add: bool) -> Task<Msg> {
        self.hide_new_phrase();
        self.gate_reset();
        self.screen = Screen::Home;
        self.notice = None;
        let tab = self.update_chat(super::chat::Cm::Tab(super::chat::SideTab::Chats));
        if add {
            return Task::batch([tab, self.update(Msg::OpenAdd)]);
        }
        tab
    }

    fn hide_new_phrase(&mut self) {
        self.new_phrase = None;
        self.gate.confirm = Default::default();
    }

    pub(super) fn gate_update(&mut self, m: Gs) -> Task<Msg> {
        match m {
            Gs::Start => {
                self.restore = false;
                self.gate.restoring = false;
                return self.gate_go(Step::Name);
            }
            Gs::Choose => return self.gate_go(Step::Choose),
            Gs::Back => {
                if self.gate.can_back() && !self.busy {
                    self.notice = None;
                    self.gate.back();
                    return self.gate_focus();
                }
            }
            Gs::Theme => {
                self.dark = !self.dark;
                self.settings.appearance = Some(self.dark);
                self.save_settings();
            }
            Gs::NameGo => {
                if !self.name_in.trim().is_empty() {
                    return self.gate_go(Step::Pass);
                }
            }
            Gs::PassGo => {
                if !self.pass_in.is_empty() {
                    return self.update(if self.gate.restoring { Msg::Restore } else { Msg::Create });
                }
            }
            Gs::PassSkip => {
                if self.keyring_ok && !self.busy {
                    use zeroize::Zeroize;
                    self.pass_in.zeroize();
                    return self.update(if self.gate.restoring { Msg::Restore } else { Msg::Create });
                }
            }
            Gs::Word(i, raw) => {
                self.notice = None;
                // A recovery key (one line, `tlkey-…`) can go anywhere the words go: it replaces the boxes.
                if p2pcore::is_recovery_key(raw.clone()) {
                    self.gate.rec_key = raw.trim().to_string();
                    return Task::none();
                }
                self.gate.active = Some(i);
                let typed_space = raw.ends_with(' ');
                let toks: Vec<String> = raw.to_lowercase().split(|c: char| !c.is_ascii_lowercase()).filter(|x| !x.is_empty()).map(str::to_string).collect();
                if toks.len() > 1 {
                    // A pasted run of words spreads from this box on.
                    for (k, tk) in toks.iter().enumerate() {
                        if i + k < 24 { self.gate.words[i + k] = tk.chars().take(12).collect(); }
                    }
                    let last = (i + toks.len() - 1).min(23);
                    self.gate.active = Some(last);
                    return iced::widget::operation::focus(word_id(last));
                }
                self.gate.words[i] = toks.first().map(|x| x.chars().take(12).collect()).unwrap_or_default();
                if typed_space && !self.gate.words[i].is_empty() {
                    return self.update(Msg::Gate(Gs::WordNext(i)));
                }
            }
            Gs::WordNext(i) => {
                let w = self.gate.words[i].clone();
                if !w.is_empty() && !is_word(&w) && suggest(&w, 2).len() == 1 {
                    self.gate.words[i] = suggest(&w, 1)[0].to_string();
                }
                if i < 23 {
                    self.gate.active = Some(i + 1);
                    return iced::widget::operation::focus(word_id(i + 1));
                }
                return self.update(Msg::Gate(Gs::RestoreGo));
            }
            Gs::WordPick(w) => {
                if let Some(i) = self.gate.active {
                    self.gate.words[i] = w.to_string();
                    return self.update(Msg::Gate(Gs::WordNext(i)));
                }
            }
            Gs::WordsPaste => return clipboard::read().map(|t| Msg::Gate(Gs::WordsPasted(t))),
            Gs::WordsPasted(t) => match t {
                Some(t) => return self.update(Msg::Gate(Gs::Word(0, t))),
                None => self.notice = Some("Couldn\u{2019}t read the clipboard. Paste into the first box instead.".into()),
            },
            Gs::RestoreGo => {
                if !self.gate.rec_key.is_empty() {
                    // The core checks the key's checksum: a typo is caught before anything is restored.
                    match p2pcore::recovery_phrase_from_input(self.gate.rec_key.clone()) {
                        Ok(p) => {
                            self.phrase_in = Zeroizing::new(p);
                            return self.gate_go(Step::Name);
                        }
                        Err(_) => {
                            self.notice = Some("That key has a typo or is cut short. Paste it again from where you saved it.".into());
                            return Task::none();
                        }
                    }
                }
                self.gate.tried = true;
                let bad = (0..24).find(|&i| self.gate.bad_word(i));
                let empty = (0..24).find(|&i| self.gate.words[i].is_empty());
                if let Some(i) = bad.or(empty) {
                    self.gate.active = Some(i);
                    return iced::widget::operation::focus(word_id(i));
                }
                self.phrase_in = Zeroizing::new(self.gate.words.join(" "));
                return self.gate_go(Step::Name);
            }
            Gs::ShowPass => self.gate.show_pass = !self.gate.show_pass,
            Gs::OpenAs => self.gate.open_as = !self.gate.open_as,
            Gs::AsKey(k) => self.gate.as_key = k,
            Gs::ClearKey => {
                self.gate.rec_key.clear();
                self.notice = None;
            }
            Gs::CopyWords => {
                self.gate.copied = true;
                if let Some(p) = &self.new_phrase {
                    // Words or key: either copy counts as saved.
                    let text = if self.gate.as_key { p2pcore::recovery_key_from_phrase(p.to_string()).unwrap_or_default() } else { p.to_string() };
                    return clipboard::write(text);
                }
            }
            Gs::Saved => return self.gate_go(Step::Device),
            Gs::Check => {
                self.gate.confirm = Default::default();
                self.gate.confirm_tried = false;
                return self.gate_go(Step::Confirm);
            }
            Gs::ConfirmIn(i, v) => {
                if let Some(s) = self.gate.confirm.get_mut(i) {
                    *s = v;
                }
            }
            Gs::ConfirmGo => {
                if self.gate.confirm.iter().any(|a| a.trim().is_empty()) {
                    return Task::none();
                }
                self.gate.confirm_tried = true;
                let phrase = self.new_phrase.as_ref().map(|p| p.to_string());
                if (0..3).all(|i| !self.gate.wrong(i, phrase.as_deref())) {
                    self.gate.back();
                }
            }
            Gs::BackToWords => self.gate.back(),
            Gs::DeviceSug(s) => self.device_in = s.to_string(),
            Gs::Agree => {
                // The button is the agreement (no checkbox); the line above it says so.
                self.hide_new_phrase();
                return self.gate_go(Step::Done);
            }
            Gs::Open(url) => {
                let _ = open::that_detached(url);
            }
            Gs::Pull => {
                if self.gate.step == Step::Done {
                    self.gate.taut = true;
                }
            }
            Gs::Tug => {
                self.gate.tugs += 1;
                if self.gate.tugs == 1 {
                    self.play_line();
                }
            }
            Gs::AddFirst => return self.gate_finish(true),
            Gs::NotNow => return self.gate_finish(false),
            Gs::Enter => return self.gate_enter(),
            Gs::AcTab(t) => {
                self.add_ui.tab = t;
                self.add_ui.found = None;
                self.add_ui.hint.clear();
                self.add_phase = super::AddPhase::Idle;
            }
            Gs::AcCopy => {
                // The bare code only (`OSVC2:…`), no "Add me on Tinline:" around it.
                if let Some(t) = self.ticket.clone() {
                    self.copied_at = Some(Instant::now());
                    return Task::batch([clipboard::write(t), Task::perform(tokio::time::sleep(Duration::from_millis(1600)), |_| Msg::Frame(Instant::now()))]);
                }
            }
            Gs::AcNewCode => {
                if self.demo || self.fetching {
                    return Task::none();
                }
                if let Err(e) = self.node.reset_ticket() {
                    eprintln!("reset_ticket: {e}");
                    self.add_ui.hint = "Couldn\u{2019}t make a new code just now. Try again in a moment.".into();
                    return Task::none();
                }
                self.ticket = None;
                self.qr = None;
                self.copied_at = None;
                return self.fetch_ticket();
            }
            Gs::AcPaste => return clipboard::read().map(|t| Msg::Gate(Gs::AcPasted(t))),
            Gs::AcPasted(text) => self.ac_pasted(text.unwrap_or_default()),
            Gs::AcBack => {
                self.add_ui.found = None;
                self.add_ui.hint.clear();
                self.add_phase = super::AddPhase::Idle;
            }
            Gs::AcAdd => {
                if let Some(p) = &self.add_ui.found
                    && !matches!(self.add_phase, super::AddPhase::Connecting(_))
                {
                    self.add_in = p.ticket.clone();
                    return self.update(Msg::AddPressed);
                }
            }
            Gs::AcMessage => {
                if let super::AddPhase::Added(c) = &self.add_phase {
                    let did = c.did.clone();
                    self.add_phase = super::AddPhase::Idle;
                    self.add_ui = AddUi::default();
                    self.screen = Screen::Home;
                    return self.update(Msg::Select(did));
                }
            }
            Gs::AcAccept => {
                if let Some(i) = self.add_ui.incoming.as_ref().filter(|i| !i.joined) {
                    return self.accept_request(i.did.clone());
                }
            }
            Gs::AcNotNow => {
                // Nothing is sent: the request waits in Contacts › "Waiting for you".
                self.add_ui.incoming = None;
                self.screen = Screen::Home;
            }
            Gs::AcIncomingMessage => {
                if let Some(i) = self.add_ui.incoming.take() {
                    self.add_ui = AddUi::default();
                    self.screen = Screen::Home;
                    return self.update(Msg::Select(i.did));
                }
            }
            Gs::CardAccept(did) => return self.accept_request(did),
            Gs::CardNotNow(did) => {
                self.hidden_requests.insert(did);
            }
            Gs::AcProfile => {
                if let Some(p) = self.add_ui.found.take() {
                    self.add_ui = AddUi::default();
                    self.screen = Screen::Home;
                    return self.update(Msg::ShellSearch(super::shell_search::Action::OpenPerson(p.did)));
                }
            }
        }
        Task::none()
    }

    /// "Paste their code": any text containing a code works (core `find_card` inside `peek_code`).
    /// Every hint is muted and polite: nothing went wrong.
    fn ac_pasted(&mut self, text: String) {
        self.add_phase = super::AddPhase::Idle;
        if text.trim().is_empty() {
            self.add_ui.hint = "Your clipboard is empty right now. When their code arrives, copy it and paste it here.".into();
            return;
        }
        let text = if text.len() > 64 * 1024 { String::new() } else { text };
        self.add_ui.hint = match self.node.peek_code(text) {
            p2pcore::CodePeek::Found { card } => {
                self.add_ui.found = Some(card);
                String::new()
            }
            p2pcore::CodePeek::Own => "That\u{2019}s your own code. Send it to them, and paste theirs here.".into(),
            p2pcore::CodePeek::Expired { name, .. } => {
                format!("That code from {} has expired. Codes last 7 days, so ask them for a fresh one.", first_name(&name))
            }
            p2pcore::CodePeek::NotACode => "That doesn\u{2019}t look like a Tinline code. Could they copy it again for you?".into(),
        };
    }

    /// Accept (the sheet or a Contacts card): they become a contact who may message and call.
    fn accept_request(&mut self, did: String) -> Task<Msg> {
        if self.add_ui.answering {
            return Task::none();
        }
        self.add_ui.answering = true;
        if self.demo {
            let name = self.add_ui.incoming.as_ref().map(|i| i.name.clone()).unwrap_or_default();
            let c = p2pcore::Contact { did: did.clone(), name, device: String::new(), added_at: 0, alias: None, verified: false };
            return Task::done(Msg::RequestAnswered(did, Ok(c)));
        }
        let node = self.node.clone();
        let d = did.clone();
        super::blocking(move || node.accept_contact_request(d).map_err(|e| e.to_string()), move |r| Msg::RequestAnswered(did.clone(), r))
    }

    /// Nothing else is in front (people.svelte.ts `incomingRequest`): the sheet may open by itself,
    /// but never over a call or another dialog.
    pub(super) fn sheet_free(&self) -> bool {
        self.call.is_none()
            && self.waiting.is_none()
            && self.ended.is_none()
            && matches!(self.screen, Screen::Home | Screen::AddContact)
            && self.chat.viewer.is_none()
            && self.shell_search.new_chat.is_none()
            && self.live.dialog.is_none()
            && !(self.groups.on && self.groups.dialog.is_some())
    }

    /// Someone used our code: the knock, and the "Contact request" sheet opens by itself (our own
    /// Add contact sheet turns into it).
    pub(super) fn contact_request(&mut self, r: p2pcore::ContactRequest) -> Task<Msg> {
        self.reload_contacts();
        self.play_cue(crate::cues::knock());
        // Away (window hidden or not focused): a desktop notification. In front the sheet below says it.
        if !(self.win.is_some() && self.focused) {
            super::notify("Contact request", &format!("{} would like to add you", r.name), super::Target::Show);
        }
        if self.sheet_free() {
            self.screen = Screen::AddContact;
            self.add_ui.incoming = Some(Incoming { did: r.did, name: r.name, joined: false, theirs: false });
        }
        Task::none()
    }

    /// They accepted our request: the line opens on our side too (`accepted` in people.svelte.ts).
    /// The sheet still on them turns taut by itself; a closed one opens on the moment.
    pub(super) fn request_accepted(&mut self, c: p2pcore::Contact) -> Task<Msg> {
        self.reload_contacts();
        self.play_line();
        if !(self.win.is_some() && self.focused) {
            super::notify(&c.name, "Accepted your request. Your line is open.", super::Target::Chat(c.did.clone()));
        }
        if self.screen == Screen::AddContact {
            if self.add_ui.found.as_ref().is_some_and(|f| f.did == c.did) {
                self.add_phase = super::AddPhase::Added(c);
            }
        } else if self.sheet_free() {
            self.screen = Screen::AddContact;
            self.add_phase = super::AddPhase::Idle;
            self.add_ui = AddUi::default();
            self.add_ui.incoming = Some(Incoming { did: c.did.clone(), name: c.name.clone(), joined: true, theirs: true });
        }
        Task::none()
    }

    /// The `line` cue (two plucks), once per moment: Done's first tug, or a contact added.
    pub(super) fn play_line(&self) {
        self.play_cue(crate::cues::line());
    }

    /// Any generated sound on the call output (skipped in screenshot runs).
    pub(super) fn play_sound(&self, samples: Option<Vec<f32>>) {
        if let Some(s) = samples { self.play_cue(s); }
    }

    pub(super) fn play_cue(&self, samples: Vec<f32>) {
        if self.demo && crate::test_env("P2P_SHOT").is_some() {
            return;
        }
        let out = self.ctl.devices.lock().unwrap().1.clone();
        crate::audio::play_cue(out, samples);
    }

    fn gate_enter(&mut self) -> Task<Msg> {
        if self.busy {
            return Task::none();
        }
        let m = match self.gate.step {
            Step::Welcome => Gs::Start,
            Step::Choose => return Task::none(),
            Step::Name => Gs::NameGo,
            Step::Pass => Gs::PassGo,
            Step::Phrase if self.gate.copied => Gs::Saved,
            Step::Phrase => Gs::CopyWords,
            Step::Confirm => Gs::ConfirmGo,
            Step::Device => return self.update(Msg::DeviceGo),
            Step::Terms => Gs::Agree,
            Step::Done => Gs::AddFirst,
            Step::Restore => Gs::RestoreGo,
        };
        self.gate_update(m)
    }
}

/// Test-hooks screenshots of sign-up and the first landing (`P2P_DEMO=gate-<step>` / `first-…`),
/// synthetic data only; nothing touches the node.
#[cfg(feature = "test-hooks")]
impl App {
    pub(super) fn signup_demo(&mut self, name: &str) -> bool {
        const WORDS: &str = "abandon ability able about above absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress actual";
        const CODE: &str = "OSVC2:FCEWVDVCGJJE7XYHDYHHPQXEB6TD5E75GBDKTY7A7YHRCQ2LMNOPQRSTUVWXYZ234567ABCDEFGHIJ";
        if let Some(step) = name.strip_prefix("gate-") {
            self.demo = true;
            self.screen = Screen::Onboarding;
            self.keyring_ok = true;
            self.name_in = "Maya Kurian".into();
            self.new_phrase = Some(Zeroizing::new(WORDS.into()));
            self.device_in = "Home computer".into();
            let path: &[Step] = match step {
                "welcome" => &[],
                "choose" => &[Step::Choose],
                "name" | "name-empty" => &[Step::Name],
                "pass" | "pass-typed" | "pass-busy" => &[Step::Name, Step::Pass],
                "phrase" | "phrase-copied" => &[Step::Name, Step::Pass, Step::Phrase],
                "confirm" | "confirm-wrong" => &[Step::Name, Step::Pass, Step::Phrase, Step::Confirm],
                "device" => &[Step::Name, Step::Pass, Step::Phrase, Step::Device],
                "terms" => &[Step::Name, Step::Pass, Step::Phrase, Step::Device, Step::Terms],
                "done" | "done-slack" => &[Step::Name, Step::Pass, Step::Phrase, Step::Device, Step::Terms, Step::Done],
                "restore" | "restore-typing" | "restore-pass" => &[Step::Choose, Step::Restore],
                _ => &[],
            };
            if step.starts_with("restore") {
                self.gate.restoring = true;
            }
            for s in path {
                self.gate.go(*s);
            }
            match step {
                "name-empty" => self.name_in.clear(),
                "pass-typed" => self.pass_in = Zeroizing::new("blue kettle".into()),
                "pass-busy" => {
                    self.pass_in = Zeroizing::new("blue kettle river stone".into());
                    self.busy = true;
                }
                "phrase-copied" => self.gate.copied = true,
                "confirm-wrong" => {
                    self.gate.confirm = ["about".into(), "acid".into(), "across".into()];
                    self.gate.confirm_tried = true;
                }
                "done" => self.gate.taut = true,
                // Five words in, the sixth half typed (suggestions), the third misspelt.
                "restore-typing" => {
                    for (i, w) in ["abandon", "ability", "ablee", "about", "above", "abs"].iter().enumerate() { self.gate.words[i] = w.to_string(); }
                    self.gate.active = Some(5);
                }
                "restore-pass" => {
                    for (i, w) in WORDS.split_whitespace().enumerate() { self.gate.words[i] = w.to_string(); }
                    self.gate.go(Step::Name);
                    self.gate.go(Step::Pass);
                }
                _ => {}
            }
            return true;
        }
        if let Some(v) = name.strip_prefix("first") {
            self.demo = true;
            self.screen = Screen::Home;
            self.profile_name = "Maya Kurian".into();
            // a new account: no placeholder groups/blocking preview rows
            self.groups.enable(false, "Maya Kurian");
            self.contacts.clear();
            self.recents.clear();
            self.ticket = Some(CODE.into());
            self.qr = super::view::qr_of(CODE);
            let tara = p2pcore::Contact { did: "did:key:z6MkTara".into(), name: "Tara Iyer".into(), device: "ab12".into(), added_at: 0, alias: None, verified: false };
            let peek = p2pcore::CardPeek { ticket: "OSVC2:TARA".into(), name: "Tara Iyer".into(), did: tara.did.clone(), known: false };
            let req = p2pcore::ContactRequest { did: tara.did.clone(), name: "Tara Iyer".into(), at: 0, expires_at: 0 };
            self.ticket_exp = Some(1_792_400_000);
            match v {
                "-open" => self.contacts = vec![tara],
                // § 6f: the joiner waiting (empty first screen), the owner asked, both sides opened.
                "-waiting" => self.requests_out = vec![req],
                "-request" | "-request-joined" => {
                    self.screen = Screen::AddContact;
                    self.add_ui.incoming = Some(Incoming { did: tara.did.clone(), name: "Tara Iyer".into(), joined: v == "-request-joined", theirs: false });
                    if v == "-request-joined" {
                        self.contacts = vec![tara];
                    }
                }
                "-accepted" => {
                    self.contacts = vec![tara.clone()];
                    self.screen = Screen::AddContact;
                    self.add_ui.incoming = Some(Incoming { did: tara.did.clone(), name: "Tara Iyer".into(), joined: true, theirs: true });
                }
                "-contacts-requests" => {
                    let _ = self.update_chat(super::Cm::Tab(super::chat::SideTab::Contacts));
                    self.requests_in = vec![req.clone()];
                    self.requests_out = vec![p2pcore::ContactRequest { did: "did:key:z6MkNikhil".into(), name: "Nikhil Das".into(), at: 0, expires_at: 0 }];
                }
                "-add" => self.screen = Screen::AddContact,
                "-add-qr" => {
                    self.screen = Screen::AddContact;
                    self.add_ui.tab = AddTab::Qr;
                }
                "-add-empty" | "-add-junk" | "-add-own" => {
                    self.screen = Screen::AddContact;
                    let text = match v {
                        "-add-empty" => String::new(),
                        "-add-own" => CODE.to_string(),
                        _ => "see you at 6".to_string(),
                    };
                    // the node is not asked: mirror ac_pasted's hints
                    self.add_ui.hint = if text.is_empty() {
                        "Your clipboard is empty right now. When their code arrives, copy it and paste it here.".into()
                    } else if text.contains(CODE) {
                        "That\u{2019}s your own code. Send it to them, and paste theirs here.".into()
                    } else {
                        "That doesn\u{2019}t look like a Tinline code. Could they copy it again for you?".into()
                    };
                }
                "-add-found" | "-add-adding" | "-add-slow" | "-add-requested" | "-add-refused" | "-add-timeout" | "-add-blocked" | "-add-offline" | "-add-added" => {
                    self.screen = Screen::AddContact;
                    self.add_ui.found = Some(peek);
                    self.add_phase = match v {
                        "-add-adding" => super::AddPhase::Connecting(Instant::now() + Duration::from_secs(3600)),
                        "-add-slow" => super::AddPhase::Connecting(Instant::now() - Duration::from_secs(3)),
                        "-add-requested" => super::AddPhase::Requested,
                        "-add-refused" => super::AddPhase::Failed(super::Reach::Refused),
                        "-add-timeout" => super::AddPhase::Failed(super::Reach::Timeout),
                        "-add-blocked" => super::AddPhase::Failed(super::Reach::Blocked),
                        "-add-offline" => super::AddPhase::Failed(super::Reach::Offline),
                        "-add-added" => {
                            self.contacts = vec![tara.clone()];
                            super::AddPhase::Added(tara)
                        }
                        _ => super::AddPhase::Idle,
                    };
                }
                _ => {}
            }
            return true;
        }
        false
    }
}
