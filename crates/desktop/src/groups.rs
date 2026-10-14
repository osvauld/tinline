//! Groups & privacy PREVIEW: UI state and updates for the screens in docs/design/groups/, driven
//! by the placeholder `groups_fake`. Compiled only for tests and `test-hooks` builds (design
//! screenshots); release builds have no preview and show the live screens only.

use iced::Task;

use super::{App, Msg, Screen};
use crate::groups_fake::{self as gf, Fake, Interval, Perm, Snap};

/// Whether the preview starts switched on when the user never chose.
pub(super) const PREVIEW_DEFAULT: bool = cfg!(feature = "test-hooks");

/// What the main pane shows from the preview.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Open {
    Group(String),
    Person(String),
}

/// A modal over the window.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Dialog {
    /// "Host <group> on this computer?" for a host request id.
    HostAsk(String),
    StopHosting(String),
    Flag { gid: String, did: String },
    Block(String),
    /// Ask a member for a private chat.
    Request { gid: String, did: String },
    /// Tapped call on someone who hasn't allowed calls from us.
    CallRefused(String),
    RemovalDetails(String),
}

pub(super) struct GroupsUi {
    pub on: bool,
    pub fake: Option<Fake>,
    /// What the screens read; refreshed after every change.
    pub snap: Snap,
    pub open: Option<Open>,
    /// The members pane beside a group chat.
    pub members: bool,
    /// The contact panel beside a 1:1 preview chat.
    pub panel: bool,
    /// Member whose action menu is open.
    pub menu: Option<String>,
    pub dialog: Option<Dialog>,
    pub input: String,
    /// The optional note of a chat request.
    pub note: String,
    /// An error shown inside the open dialog.
    pub error: Option<String>,
}

impl GroupsUi {
    pub fn new(on: bool, me_name: &str) -> Self {
        let mut g = GroupsUi {
            on: false,
            fake: None,
            snap: Snap::default(),
            open: None,
            members: true,
            panel: true,
            menu: None,
            dialog: None,
            input: String::new(),
            note: String::new(),
            error: None,
        };
        g.enable(on, me_name);
        g
    }

    /// Fresh sample data when switched on; nothing kept when off.
    pub fn enable(&mut self, on: bool, me_name: &str) {
        self.on = on;
        self.open = None;
        self.menu = None;
        self.dialog = None;
        self.input.clear();
        self.note.clear();
        self.error = None;
        if on {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
            self.fake = Some(Fake::sample(now, me_name));
        } else {
            self.fake = None;
        }
        self.refresh();
    }

    pub fn refresh(&mut self) {
        self.snap = self.fake.as_ref().map(Fake::snapshot).unwrap_or_default();
        // Something removed under us (a deleted group, a contact gone): close it.
        match &self.open {
            Some(Open::Group(id)) if self.snap.group(id).is_none() => self.open = None,
            Some(Open::Person(did)) if self.snap.person(did).is_none() => self.open = None,
            _ => {}
        }
    }

}

/// The preview's own messages (demo screens and the privacy preview). Some lost their buttons when
/// the Groups tab and chat requests moved to the real core (G-UI: `group_live.rs`).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) enum Gm {
    Enable(bool),
    OpenGroup(String),
    OpenPerson(String),
    ToggleMembers,
    TogglePanel,
    Menu(Option<String>),
    MessagePrivately(String, String),
    MakeAdmin(String, String),
    MakeHost(String, String),
    SimHostAccept(String, String),
    AskFlag(String, String),
    AskBlock(String),
    AskStopHosting(String),
    ReviewHostRequest(String),
    DeclineHostRequest(String),
    RemovalDetails(String),
    Confirm,
    Dismiss,
    Input(String),
    Send,
    NoteIn(String),
    Unblock(String),
    SetPerm(String, Perm, bool),
    DismissChip(String),
    Call(String),
    SimAttempt(String),
    KeepRunning(bool),
    AskForHost(String),
    DeleteGroup(String),
    ArchiveGroup(String),
    Interval(Interval),
    /// A button whose feature has no screen yet in the preview.
    Placeholder(&'static str),
}

impl App {
    pub(super) fn update_groups(&mut self, m: Gm) -> Task<Msg> {
        if let Gm::Enable(on) = m {
            let name = self.profile_name.clone();
            self.groups.enable(on, &name);
            if !on && self.settings_tab == 6 {
                self.settings_tab = 7;
            }
            self.settings.groups_preview = Some(on);
            self.save_settings();
            return Task::none();
        }
        if !self.groups.on {
            return Task::none();
        }
        self.groups_step(m);
        self.groups.refresh();
        Task::none()
    }

    fn open_preview(&mut self, f: &Fake, o: Open) {
        self.leave_chat();
        self.sel = None;
        self.screen = Screen::Home;
        self.groups.menu = None;
        self.groups.dialog = None;
        self.groups.input.clear();
        match &o {
            Open::Group(id) => f.mark_group_read(id),
            Open::Person(did) => f.mark_person_read(did),
        }
        self.groups.open = Some(o);
    }

    fn groups_step(&mut self, m: Gm) {
        // Out while the step runs, so the step can change the rest of the UI state freely.
        let Some(f) = self.groups.fake.take() else { return };
        let said = self.groups_cmd(&f, m);
        self.groups.fake = Some(f);
        if said.is_some() {
            self.notice = said;
        }
    }

    /// Runs one preview action; returns what to tell the user.
    fn groups_cmd(&mut self, f: &Fake, m: Gm) -> Option<String> {
        let mut said: Option<String> = None;
        fn fail(said: &mut Option<String>, r: Result<(), gf::GErr>) {
            if let Err(e) = r {
                *said = Some(e.to_string());
            }
        }
        match m {
            Gm::Enable(_) => {}
            Gm::OpenGroup(id) => self.open_preview(f, Open::Group(id)),
            Gm::OpenPerson(did) => self.open_preview(f, Open::Person(did)),
            Gm::ToggleMembers => self.groups.members = !self.groups.members,
            Gm::TogglePanel => self.groups.panel = !self.groups.panel,
            Gm::Menu(did) => self.groups.menu = if self.groups.menu == did { None } else { did },
            Gm::MessagePrivately(gid, did) => {
                self.groups.menu = None;
                if self.groups.snap.is_contact(&did) && !self.groups.snap.blocked_by_me(&did) {
                    self.open_preview(f, Open::Person(did));
                } else {
                    self.groups.note.clear();
                    self.groups.error = None;
                    self.groups.dialog = Some(Dialog::Request { gid, did });
                }
            }
            Gm::MakeAdmin(gid, did) => {
                self.groups.menu = None;
                fail(&mut said, f.make_admin(&gid, &did));
            }
            Gm::MakeHost(gid, did) => {
                self.groups.menu = None;
                fail(&mut said, f.ask_member_to_host(&gid, &did));
            }
            Gm::SimHostAccept(gid, did) => fail(&mut said, f.simulate_host_accept(&gid, &did)),
            Gm::AskFlag(gid, did) => {
                self.groups.menu = None;
                self.groups.dialog = Some(Dialog::Flag { gid, did });
            }
            Gm::AskBlock(did) => {
                self.groups.menu = None;
                self.groups.dialog = Some(Dialog::Block(did));
            }
            Gm::AskStopHosting(gid) => self.groups.dialog = Some(Dialog::StopHosting(gid)),
            Gm::ReviewHostRequest(id) => self.groups.dialog = Some(Dialog::HostAsk(id)),
            Gm::DeclineHostRequest(id) => {
                self.groups.dialog = None;
                fail(&mut said, f.decline_host_request(&id));
            }
            Gm::RemovalDetails(gid) => self.groups.dialog = Some(Dialog::RemovalDetails(gid)),
            Gm::Dismiss => {
                self.groups.dialog = None;
                self.groups.error = None;
                self.groups.menu = None;
            }
            Gm::Confirm => {
                let Some(d) = self.groups.dialog.clone() else { return None };
                let mut close = true;
                match d {
                    Dialog::HostAsk(id) => match f.accept_host_request(&id) {
                        Ok(gid) => {
                            said = Some("This computer now hosts the group. Members were told.".into());
                            self.open_preview(f, Open::Group(gid));
                        }
                        Err(e) => said = Some(e.to_string()),
                    },
                    Dialog::StopHosting(gid) => {
                        fail(&mut said, f.stop_hosting(&gid));
                    }
                    Dialog::Flag { gid, did } => match f.flag_member(&gid, &did) {
                        Ok(o) if o.removed => said = Some("Removed from the group. They are told why.".into()),
                        Ok(o) => said = Some(format!("Flag {} of {} recorded.", o.count, gf::FLAGS_TO_REMOVE)),
                        Err(e) => said = Some(e.to_string()),
                    },
                    Dialog::Block(did) => fail(&mut said, f.block(&did)),
                    Dialog::Request { gid, did } => match f.send_chat_request(&gid, &did, &self.groups.note) {
                        Ok(_) => said = Some("Request sent. It waits at the top of Chats until answered.".into()),
                        Err(gf::GErr::AlreadyContact) => {
                            self.groups.dialog = None;
                            self.open_preview(f, Open::Person(did));
                        }
                        Err(e) => {
                            self.groups.error = Some(e.to_string());
                            close = false;
                        }
                    },
                    Dialog::CallRefused(did) => {
                        fail(&mut said, f.ask_to_allow_calls(&did));
                    }
                    Dialog::RemovalDetails(_) => {}
                }
                if close {
                    self.groups.dialog = None;
                    self.groups.error = None;
                    self.groups.note.clear();
                }
            }
            Gm::Input(v) => self.groups.input = v,
            Gm::NoteIn(v) => self.groups.note = v.chars().take(200).collect(),
            Gm::Send => {
                let text = std::mem::take(&mut self.groups.input);
                let r = match &self.groups.open {
                    Some(Open::Group(id)) => f.send_group_message(id, &text),
                    Some(Open::Person(did)) => f.send_message(did, &text),
                    None => Ok(()),
                };
                match r {
                    Ok(()) => {}
                    Err(gf::GErr::Empty) => {}
                    Err(e) => {
                        self.groups.input = text;
                        said = Some(e.to_string());
                    }
                }
            }
            Gm::Unblock(did) => fail(&mut said, f.unblock(&did)),
            Gm::SetPerm(did, p, on) => fail(&mut said, f.set_permission(&did, p, on)),
            Gm::DismissChip(did) => f.dismiss_calls_chip(&did),
            Gm::Call(did) => match f.call(&did) {
                Ok(()) => said = Some("Calls aren\u{2019}t wired up in the groups preview; nothing was dialled.".into()),
                Err(gf::GErr::CallsNotAllowed(_)) => self.groups.dialog = Some(Dialog::CallRefused(did)),
                Err(e) => said = Some(e.to_string()),
            },
            Gm::SimAttempt(did) => fail(&mut said, f.simulate_attempt(&did).map(|_| ())),
            Gm::KeepRunning(on) => f.set_keep_running(on),
            Gm::AskForHost(gid) => {
                fail(&mut said, f.ask_for_host(&gid));
                said.get_or_insert_with(|| "Members with a desktop were asked to host.".into());
            }
            Gm::DeleteGroup(gid) => fail(&mut said, f.delete_group(&gid)),
            Gm::ArchiveGroup(gid) => {
                fail(&mut said, f.archive_group(&gid));
                said.get_or_insert_with(|| "Kept as an archive. Nothing new will arrive.".into());
            }
            Gm::Interval(i) => f.set_interval(i),
            Gm::Placeholder(what) => said = Some(format!("{what} isn\u{2019}t part of the preview yet.")),
        }
        said
    }
}
