//! Group chat on the core's real API (G1, `crates/core/src/group/api.rs`, docs/group-core.md):
//! the Groups side tab, a group's conversation, its members pane and the group dialogs. The
//! screens are in `group_live_view.rs`; the placeholder preview (`groups.rs`, `groups_fake.rs`)
//! is kept only for the P2P_DEMO screens and the privacy preview.
//!
//! Every core call goes through `GroupApi`, so the flows are tested with a stand-in that follows
//! the core's rules (`tests` below). Reads (`groups`, `group_detail`, `group_messages`) are
//! in-memory in the core and run on the UI thread; everything that writes or waits on the
//! network is a `Job` the app runs on a blocking thread.
//!
//! Decisions (rotation-open-decisions.md §0): 3 admin flags remove a member (U9/U15) and the
//! removed member sees the signed flags; a post written before a removal still shows, marked
//! (U11); only hosts know member endpoints (U7), so "Waiting for a host" is the honest state of
//! our posts until a host acknowledges them. Chat requests through the host (U3) are not in core
//! yet and have no live UI.

use iced::Task;
use p2pcore::{
    Error, GroupDelivery, GroupDetail, GroupInvitePeek, GroupItemKind, GroupMember, GroupMessage, GroupPage, GroupRole, GroupSummary, Node,
};

/// Messages loaded when a group opens, and per "Show earlier".
pub(super) const PAGE: u32 = 100;
/// Flags that remove a member (U9/U15; `state::REMOVAL_FLAGS` in core).
pub(super) const FLAGS_TO_REMOVE: u32 = 3;

/// The core's group calls.
pub trait GroupApi {
    fn groups(&self) -> Result<Vec<GroupSummary>, Error>;
    fn group_detail(&self, id: String) -> Result<GroupDetail, Error>;
    fn group_messages(&self, id: String, before: Option<String>, limit: u32) -> Result<GroupPage, Error>;
    fn group_mark_read(&self, id: String) -> Result<(), Error>;
    fn group_create(&self, name: String) -> Result<String, Error>;
    fn group_invite(&self, id: String) -> Result<String, Error>;
    fn group_device_token(&self, id: String) -> Result<String, Error>;
    fn group_peek_invite(&self, token: String) -> Option<GroupInvitePeek>;
    fn group_join(&self, token: String) -> Result<String, Error>;
    fn group_post(&self, id: String, text: String, reply_to: Option<String>) -> Result<GroupMessage, Error>;
    fn group_flag(&self, id: String, did: String) -> Result<(), Error>;
    fn group_set_admin(&self, id: String, did: String, admin: bool) -> Result<(), Error>;
    fn group_leave(&self, id: String) -> Result<(), Error>;
    fn group_appoint_host(&self, id: String, did: String, device_id: String) -> Result<(), Error>;
    fn group_revoke_host(&self, id: String, did: String, device_id: String) -> Result<(), Error>;
    fn group_accept_host(&self, id: String, accept: bool) -> Result<(), Error>;
    fn group_device_id(&self) -> Result<String, Error>;
}

impl GroupApi for Node {
    fn groups(&self) -> Result<Vec<GroupSummary>, Error> {
        Node::groups(self)
    }
    fn group_detail(&self, id: String) -> Result<GroupDetail, Error> {
        Node::group_detail(self, id)
    }
    fn group_messages(&self, id: String, before: Option<String>, limit: u32) -> Result<GroupPage, Error> {
        Node::group_messages(self, id, before, limit)
    }
    fn group_mark_read(&self, id: String) -> Result<(), Error> {
        Node::group_mark_read(self, id)
    }
    fn group_create(&self, name: String) -> Result<String, Error> {
        Node::group_create(self, name)
    }
    fn group_invite(&self, id: String) -> Result<String, Error> {
        Node::group_invite(self, id)
    }
    fn group_device_token(&self, id: String) -> Result<String, Error> {
        Node::group_device_token(self, id)
    }
    fn group_peek_invite(&self, token: String) -> Option<GroupInvitePeek> {
        Node::group_peek_invite(self, token)
    }
    fn group_join(&self, token: String) -> Result<String, Error> {
        Node::group_join(self, token)
    }
    fn group_post(&self, id: String, text: String, reply_to: Option<String>) -> Result<GroupMessage, Error> {
        Node::group_post(self, id, text, reply_to)
    }
    fn group_flag(&self, id: String, did: String) -> Result<(), Error> {
        Node::group_flag(self, id, did)
    }
    fn group_set_admin(&self, id: String, did: String, admin: bool) -> Result<(), Error> {
        Node::group_set_admin(self, id, did, admin)
    }
    fn group_leave(&self, id: String) -> Result<(), Error> {
        Node::group_leave(self, id)
    }
    fn group_appoint_host(&self, id: String, did: String, device_id: String) -> Result<(), Error> {
        Node::group_appoint_host(self, id, did, device_id)
    }
    fn group_revoke_host(&self, id: String, did: String, device_id: String) -> Result<(), Error> {
        Node::group_revoke_host(self, id, did, device_id)
    }
    fn group_accept_host(&self, id: String, accept: bool) -> Result<(), Error> {
        Node::group_accept_host(self, id, accept)
    }
    fn group_device_id(&self) -> Result<String, Error> {
        Node::group_device_id(self)
    }
}

/// Words for a failed group call.
pub(super) fn gerr(e: Error) -> String {
    match e {
        Error::Rejected(r) => match r.as_str() {
            "removed" => "You were removed from this group.".into(),
            "not a member" => "You\u{2019}re not a member of this group.".into(),
            r => {
                let mut c = r.chars();
                match c.next() {
                    Some(f) => format!("{}{}.", f.to_uppercase(), c.as_str().trim_end_matches('.')),
                    None => "Refused.".into(),
                }
            }
        },
        Error::Timeout => "No host answered in time. Try again when a host is online.".into(),
        Error::Locked => "Unlock first.".into(),
        Error::NotFound => "That group isn\u{2019}t on this computer.".into(),
        e => e.to_string(),
    }
}

/// What the token dialog shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenKind {
    /// An admin's 7-day invite.
    Invite,
    /// For our own other devices (same DID, no invite).
    Device,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum LDialog {
    NewGroup { name: String },
    /// Paste a token; `peek` once it parses.
    Join { token: String, peek: Option<GroupInvitePeek> },
    /// `text` is None while the core makes it.
    Token { kind: TokenKind, text: Option<String> },
    Flag { did: String },
    Admin { did: String, admin: bool },
    Leave,
    /// Why we were removed: the signed flags.
    Removal,
    /// This device starts (true) or stops hosting the open group.
    Host { accept: bool },
}

/// Work that writes or waits on the network: run off the UI thread, its result comes back as an `Lm`.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Job {
    Create(String),
    Join(String),
    Invite(String),
    DeviceToken(String),
    Post { gid: String, text: String, reply: Option<String> },
    Flag { gid: String, did: String },
    SetAdmin { gid: String, did: String, admin: bool },
    Leave(String),
    Appoint { gid: String, did: String, dev: String },
    Revoke { gid: String, did: String, dev: String },
    AcceptHost { gid: String, accept: bool },
}

impl Job {
    pub fn run(self, api: &dyn GroupApi) -> Lm {
        let did = |r: Result<(), Error>, ok: String| Lm::Did(r.map(|_| ok).map_err(gerr));
        match self {
            Job::Create(name) => Lm::Created(api.group_create(name).map_err(gerr)),
            Job::Join(token) => Lm::Joined(api.group_join(token).map_err(gerr)),
            Job::Invite(gid) => Lm::Token(api.group_invite(gid).map_err(gerr)),
            Job::DeviceToken(gid) => Lm::Token(api.group_device_token(gid).map_err(gerr)),
            Job::Post { gid, text, reply } => {
                let r = api.group_post(gid, text.clone(), reply).map_err(gerr);
                Lm::Posted(r, text)
            }
            Job::Flag { gid, did: who } => did(api.group_flag(gid, who), "Flag recorded. It is signed by you and the member can see it.".into()),
            Job::SetAdmin { gid, did: who, admin } => did(
                api.group_set_admin(gid, who, admin),
                if admin { "Made admin.".into() } else { "No longer an admin.".into() },
            ),
            Job::Leave(gid) => did(api.group_leave(gid), "You left the group. Its messages stay here.".into()),
            Job::Appoint { gid, did: who, dev } => did(
                api.group_appoint_host(gid, who, dev),
                "Appointed. The device hosts once it agrees (Host from this device).".into(),
            ),
            Job::Revoke { gid, did: who, dev } => did(api.group_revoke_host(gid, who, dev), "That device no longer hosts the group.".into()),
            Job::AcceptHost { gid, accept } => did(
                api.group_accept_host(gid, accept),
                if accept {
                    "This device offered to host. It hosts once an admin appoints it.".into()
                } else {
                    "This device stopped hosting the group.".into()
                },
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Lm {
    /// Re-read the list (after unlock, or an event).
    Reload,
    /// Core events.
    Message(GroupMessage),
    Changed(String),
    Open(String),
    Close,
    ToggleInfo,
    /// The member whose menu is open.
    Menu(Option<String>),
    Input(String),
    Send,
    Reply(Option<String>),
    Older,
    NewGroup,
    NameIn(String),
    JoinOpen,
    TokenIn(String),
    Invite,
    DeviceToken,
    CopyToken,
    AskFlag(String),
    AskAdmin(String, bool),
    AskLeave,
    AskHost(bool),
    Removal,
    Appoint(String, String),
    Revoke(String, String),
    Confirm,
    Dismiss,
    Created(Result<String, String>),
    Joined(Result<String, String>),
    Token(Result<String, String>),
    /// The post, and the text to put back on failure.
    Posted(Result<GroupMessage, String>, String),
    /// A plain action: what to tell the user.
    Did(Result<String, String>),
}

/// What a step asks of the app around it.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Out {
    pub job: Option<Job>,
    pub notice: Option<String>,
    /// A group opened: close whatever else the main pane showed.
    pub opened: bool,
    pub copy: Option<String>,
}

#[derive(Default)]
pub(super) struct GroupsLive {
    /// Our DID (for "You" in system lines).
    pub me: String,
    /// This install's device ID (hex), what an admin appoints.
    pub my_device: String,
    pub list: Vec<GroupSummary>,
    pub open: Option<String>,
    pub detail: Option<GroupDetail>,
    /// Oldest first.
    pub msgs: Vec<GroupMessage>,
    pub older: Option<String>,
    pub input: String,
    pub reply: Option<String>,
    /// The members pane beside the chat (on by default).
    pub hide_info: bool,
    pub menu: Option<String>,
    pub dialog: Option<LDialog>,
    /// A dialog's job is running.
    pub busy: bool,
    /// An error shown inside the open dialog.
    pub error: Option<String>,
    /// A post is being written.
    pub sending: bool,
    /// The list was read while unlocked (re-read on unlock and account switch).
    pub ready: bool,
    /// The token dialog's QR.
    pub qr: Option<super::Qr>,
}

/// Member menu entries, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Action {
    MakeAdmin,
    RevokeAdmin,
    /// Appoint this device of theirs.
    Appoint(String),
    /// Stop this device of theirs hosting.
    Revoke(String),
    Flag,
}

pub(super) fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

fn or_someone(name: &str) -> &str {
    if name.trim().is_empty() { "Someone" } else { name }
}

/// "a1b2c3d4": enough of a device ID to tell devices apart.
pub(super) fn short_dev(dev: &str) -> &str {
    &dev[..dev.len().min(8)]
}

impl GroupsLive {
    pub fn reset(&mut self, me: String, my_device: String) {
        *self = GroupsLive { me, my_device, ..Default::default() };
    }

    pub fn summary(&self, gid: &str) -> Option<&GroupSummary> {
        self.list.iter().find(|s| s.id == gid)
    }

    pub fn role(&self) -> Option<GroupRole> {
        self.detail.as_ref().map(|d| d.my_role)
    }

    /// The Groups tab's badge: unread posts in groups we are still in.
    pub fn badge(&self) -> u32 {
        self.list.iter().filter(|s| active(s.my_role)).map(|s| s.unread).sum()
    }

    /// We can post in the open group.
    pub fn can_post(&self) -> bool {
        self.role().is_some_and(active)
    }

    pub fn is_admin(&self) -> bool {
        matches!(self.role(), Some(GroupRole::Owner | GroupRole::Admin))
    }

    pub fn member(&self, did: &str) -> Option<&GroupMember> {
        self.detail.as_ref()?.members.iter().find(|m| m.did == did)
    }

    pub fn by_id(&self, id: &str) -> Option<&GroupMessage> {
        self.msgs.iter().find(|m| m.id == id)
    }

    /// What the menu on `m` offers us.
    pub fn actions(&self, m: &GroupMember) -> Vec<Action> {
        let Some(d) = &self.detail else { return Vec::new() };
        let mut out = Vec::new();
        if m.is_me || !active(d.my_role) {
            return out;
        }
        if d.my_role == GroupRole::Owner && !m.is_owner {
            out.push(if m.is_admin { Action::RevokeAdmin } else { Action::MakeAdmin });
        }
        if self.is_admin() {
            for dev in &m.device_ids {
                if d.hosts.iter().any(|h| h.did == m.did && &h.device_id == dev) {
                    out.push(Action::Revoke(dev.clone()));
                } else {
                    out.push(Action::Appoint(dev.clone()));
                }
            }
            if !m.is_owner {
                out.push(Action::Flag);
            }
        }
        out
    }

    /// The flag dialog's numbers: which flag ours is, and whether it removes them.
    pub fn flag_after(&self, did: &str) -> (u32, bool) {
        let n = self.member(did).map_or(0, |m| m.flags) + 1;
        (n, n >= FLAGS_TO_REMOVE)
    }

    /// The header line under the group's name.
    pub fn status_line(&self) -> String {
        let Some(d) = &self.detail else { return String::new() };
        match d.my_role {
            GroupRole::Removed => return "You were removed from this group".into(),
            GroupRole::Left => return "You left this group".into(),
            _ => {}
        }
        let n = d.members.len();
        let members = format!("{n} member{}", if n == 1 { "" } else { "s" });
        if d.am_host {
            return format!("{members} \u{b7} this computer hosts the group");
        }
        if d.hosts.is_empty() {
            return format!("{members} \u{b7} no host yet");
        }
        if !d.hosts.iter().any(|h| h.online) {
            return format!("{members} \u{b7} waiting for a host");
        }
        let h = d.hosts.len();
        format!("{members} \u{b7} {h} host{}", if h == 1 { "" } else { "s" })
    }

    /// No host is reachable (and we are still in the group).
    pub fn waiting_for_host(&self) -> bool {
        self.detail.as_ref().is_some_and(|d| active(d.my_role) && !d.am_host && !d.hosts.iter().any(|h| h.online))
    }

    /// Every step of the flows. Reads go to `api` right here; writes come back as `Out::job`.
    pub fn step(&mut self, api: &dyn GroupApi, m: Lm) -> Out {
        let mut out = Out::default();
        match m {
            Lm::Reload => self.reload(api),
            Lm::Message(msg) => {
                if self.open.as_deref() == Some(msg.group_id.as_str()) {
                    upsert(&mut self.msgs, msg);
                    let _ = api.group_mark_read(self.open.clone().unwrap_or_default());
                }
                self.reload_list(api);
            }
            Lm::Changed(gid) => {
                self.reload_list(api);
                if self.open.as_deref() == Some(gid.as_str()) {
                    self.reload_open(api);
                }
            }
            Lm::Open(gid) => {
                self.open = Some(gid.clone());
                self.menu = None;
                self.input.clear();
                self.reply = None;
                self.msgs.clear();
                self.older = None;
                self.reload_open(api);
                let _ = api.group_mark_read(gid);
                self.reload_list(api);
                out.opened = true;
            }
            Lm::Close => self.close(),
            Lm::ToggleInfo => self.hide_info = !self.hide_info,
            Lm::Menu(did) => self.menu = if self.menu == did { None } else { did },
            Lm::Input(v) => self.input = v,
            Lm::Reply(id) => self.reply = id,
            Lm::Send => {
                let text = self.input.trim().to_string();
                if text.is_empty() || self.sending {
                    return out;
                }
                let Some(gid) = self.open.clone() else { return out };
                if !self.can_post() {
                    out.notice = Some("You can\u{2019}t send messages to this group.".into());
                    return out;
                }
                self.input.clear();
                self.sending = true;
                out.job = Some(Job::Post { gid, text, reply: self.reply.take() });
            }
            Lm::Posted(r, text) => {
                self.sending = false;
                match r {
                    Ok(msg) => {
                        if self.open.as_deref() == Some(msg.group_id.as_str()) {
                            upsert(&mut self.msgs, msg);
                        }
                        self.reload_list(api);
                    }
                    Err(e) => {
                        if self.input.is_empty() {
                            self.input = text;
                        }
                        out.notice = Some(e);
                    }
                }
            }
            Lm::Older => {
                let (Some(gid), Some(before)) = (self.open.clone(), self.older.clone()) else { return out };
                match api.group_messages(gid, Some(before), PAGE) {
                    Ok(p) => {
                        let mut all = p.messages;
                        all.retain(|m| self.by_id(&m.id).is_none());
                        all.append(&mut self.msgs);
                        self.msgs = all;
                        self.older = p.older;
                    }
                    Err(e) => out.notice = Some(gerr(e)),
                }
            }
            Lm::NewGroup => self.show(LDialog::NewGroup { name: String::new() }),
            Lm::NameIn(v) => {
                if let Some(LDialog::NewGroup { name }) = &mut self.dialog {
                    *name = v.chars().take(100).collect();
                }
            }
            Lm::JoinOpen => self.show(LDialog::Join { token: String::new(), peek: None }),
            Lm::TokenIn(v) => {
                if let Some(LDialog::Join { token, peek }) = &mut self.dialog {
                    *token = v.trim().to_string();
                    *peek = if token.is_empty() { None } else { api.group_peek_invite(token.clone()) };
                    self.error = None;
                }
            }
            Lm::Invite | Lm::DeviceToken => {
                let Some(gid) = self.open.clone() else { return out };
                let kind = if matches!(m, Lm::Invite) { TokenKind::Invite } else { TokenKind::Device };
                self.show(LDialog::Token { kind, text: None });
                self.busy = true;
                out.job = Some(if kind == TokenKind::Invite { Job::Invite(gid) } else { Job::DeviceToken(gid) });
            }
            Lm::Token(r) => {
                self.busy = false;
                if let Some(LDialog::Token { text, .. }) = &mut self.dialog {
                    match r {
                        Ok(t) => *text = Some(t),
                        Err(e) => self.error = Some(e),
                    }
                }
            }
            Lm::CopyToken => {
                if let Some(LDialog::Token { text: Some(t), .. }) = &self.dialog {
                    out.copy = Some(t.clone());
                    out.notice = Some("Copied. Send it over an app you trust; anyone holding it can use it.".into());
                }
            }
            Lm::AskFlag(did) => {
                self.menu = None;
                self.show(LDialog::Flag { did });
            }
            Lm::AskAdmin(did, admin) => {
                self.menu = None;
                self.show(LDialog::Admin { did, admin });
            }
            Lm::AskLeave => self.show(LDialog::Leave),
            Lm::AskHost(accept) => self.show(LDialog::Host { accept }),
            Lm::Removal => self.show(LDialog::Removal),
            Lm::Appoint(did, dev) => {
                self.menu = None;
                if let Some(gid) = self.open.clone() {
                    out.job = Some(Job::Appoint { gid, did, dev });
                }
            }
            Lm::Revoke(did, dev) => {
                self.menu = None;
                if let Some(gid) = self.open.clone() {
                    out.job = Some(Job::Revoke { gid, did, dev });
                }
            }
            Lm::Dismiss => {
                if !self.busy {
                    self.dialog = None;
                    self.error = None;
                }
                self.menu = None;
            }
            Lm::Confirm => out.job = self.confirm(),
            Lm::Created(r) => {
                self.busy = false;
                match r {
                    Ok(gid) => {
                        self.dialog = None;
                        out = self.step(api, Lm::Open(gid));
                        out.notice = Some("Group made. This computer hosts it; invite people from the members pane.".into());
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            Lm::Joined(r) => {
                self.busy = false;
                match r {
                    Ok(gid) => {
                        self.dialog = None;
                        out = self.step(api, Lm::Open(gid));
                        out.notice = Some("You joined the group.".into());
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            Lm::Did(r) => {
                self.busy = false;
                match r {
                    Ok(said) => {
                        self.dialog = None;
                        self.error = None;
                        out.notice = Some(said);
                    }
                    Err(e) if self.dialog.is_some() => self.error = Some(e),
                    Err(e) => out.notice = Some(e),
                }
                self.reload_list(api);
                self.reload_open(api);
            }
        }
        out
    }

    fn show(&mut self, d: LDialog) {
        self.dialog = Some(d);
        self.error = None;
        self.busy = false;
    }

    /// The dialog's action.
    fn confirm(&mut self) -> Option<Job> {
        if self.busy {
            return None;
        }
        let gid = self.open.clone().unwrap_or_default();
        let job = match self.dialog.clone()? {
            LDialog::NewGroup { name } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    self.error = Some("Give the group a name.".into());
                    return None;
                }
                Job::Create(name)
            }
            LDialog::Join { token, peek } => {
                if peek.is_none() {
                    self.error = Some("That isn\u{2019}t a group invite. Paste the whole text you were sent.".into());
                    return None;
                }
                Job::Join(token)
            }
            LDialog::Flag { did } => Job::Flag { gid, did },
            LDialog::Admin { did, admin } => Job::SetAdmin { gid, did, admin },
            LDialog::Leave => Job::Leave(gid),
            LDialog::Host { accept } => Job::AcceptHost { gid, accept },
            LDialog::Token { .. } | LDialog::Removal => {
                self.dialog = None;
                return None;
            }
        };
        self.busy = true;
        self.error = None;
        Some(job)
    }

    pub fn close(&mut self) {
        self.open = None;
        self.detail = None;
        self.msgs.clear();
        self.older = None;
        self.menu = None;
        self.reply = None;
        self.input.clear();
    }

    pub fn reload(&mut self, api: &dyn GroupApi) {
        self.reload_list(api);
        if let Ok(d) = api.group_device_id() {
            self.my_device = d;
        }
        self.reload_open(api);
    }

    fn reload_list(&mut self, api: &dyn GroupApi) {
        // A node without groups (or locked) answers with an error: an empty list.
        self.list = api.groups().unwrap_or_default();
    }

    /// Re-reads the open group's detail and the loaded window of messages (delivery and removal
    /// marks change in place).
    fn reload_open(&mut self, api: &dyn GroupApi) {
        let Some(gid) = self.open.clone() else { return };
        match api.group_detail(gid.clone()) {
            Ok(d) => self.detail = Some(d),
            Err(_) => {
                self.close();
                return;
            }
        }
        let want = (self.msgs.len() as u32).max(PAGE);
        if let Ok(p) = api.group_messages(gid, None, want) {
            self.msgs = p.messages;
            self.older = p.older;
        }
    }
}

/// Still in the group (can read new messages and post).
pub(super) fn active(r: GroupRole) -> bool {
    matches!(r, GroupRole::Owner | GroupRole::Admin | GroupRole::Member)
}

/// Inserts or replaces by id, keeping the core's `(at, id)` order.
pub(super) fn upsert(msgs: &mut Vec<GroupMessage>, m: GroupMessage) {
    if let Some(i) = msgs.iter().position(|x| x.id == m.id) {
        msgs[i] = m;
        return;
    }
    let at = msgs.partition_point(|x| (x.at, x.id.as_str()) < (m.at, m.id.as_str()));
    msgs.insert(at, m);
}

/// A system line in plain words. `me` is our DID.
pub(super) fn sys_text(m: &GroupMessage, me: &str) -> String {
    let who = |did: &str, name: &str| if did == me { "You".to_string() } else { or_someone(name).to_string() };
    let by = who(&m.author_did, &m.author_name);
    let sub = who(&m.subject_did, &m.subject_name);
    let you = m.subject_did == me;
    match m.kind {
        GroupItemKind::Post => m.text.clone(),
        GroupItemKind::Joined => format!("{sub} joined"),
        GroupItemKind::Left => format!("{sub} left"),
        GroupItemKind::Removed if you => format!("You were removed after {FLAGS_TO_REMOVE} admin flags"),
        GroupItemKind::Removed => format!("{sub} was removed after {FLAGS_TO_REMOVE} admin flags"),
        GroupItemKind::AdminGranted if you => format!("{by} made you an admin"),
        GroupItemKind::AdminGranted => format!("{by} made {sub} an admin"),
        GroupItemKind::AdminRevoked if you => format!("{by} removed you as admin"),
        GroupItemKind::AdminRevoked => format!("{by} removed {sub} as admin"),
        GroupItemKind::HostAdded if you => "A device of yours now hosts the group".into(),
        GroupItemKind::HostAdded => format!("A device of {sub}\u{2019}s now hosts the group"),
        GroupItemKind::HostRemoved if you => "A device of yours stopped hosting the group".into(),
        GroupItemKind::HostRemoved => format!("A device of {sub}\u{2019}s stopped hosting the group"),
    }
}

/// The list row's second line.
pub(super) fn preview(s: &GroupSummary, me: &str) -> String {
    match s.my_role {
        GroupRole::Removed => return "You were removed from this group".into(),
        GroupRole::Left => return "You left this group".into(),
        _ => {}
    }
    match &s.last {
        None => String::new(),
        Some(m) if m.kind != GroupItemKind::Post => sys_text(m, me),
        Some(m) if m.outgoing => format!("You: {}", m.text),
        Some(m) => format!("{}: {}", first_name(or_someone(&m.author_name)), m.text),
    }
}

/// The small line under one of our posts.
pub(super) fn delivery_word(m: &GroupMessage) -> Option<&'static str> {
    (m.outgoing && m.kind == GroupItemKind::Post && m.delivery == GroupDelivery::Waiting).then_some("Waiting for a host")
}

impl super::App {
    pub(super) fn update_live(&mut self, m: Lm) -> Task<super::Msg> {
        if self.demo {
            return Task::none();
        }
        let node = self.node.clone();
        let out = self.live.step(&*node, m);
        self.after_live(out)
    }

    fn after_live(&mut self, out: Out) -> Task<super::Msg> {
        if out.opened {
            self.leave_chat();
            self.sel = None;
            self.groups.open = None;
            self.groups.dialog = None;
            self.chat.info = false;
            self.screen = super::Screen::Home;
        }
        match &self.live.dialog {
            Some(LDialog::Token { text: Some(t), .. }) => {
                if self.live.qr.is_none() {
                    self.live.qr = super::view::qr_of(t);
                }
            }
            _ => self.live.qr = None,
        }
        if let Some(n) = out.notice {
            self.notice = Some(n);
        }
        let mut tasks = Vec::new();
        if let Some(t) = out.copy {
            tasks.push(iced::clipboard::write(t));
        }
        if let Some(job) = out.job {
            let node = self.node.clone();
            tasks.push(super::blocking(move || job.run(&*node), super::Msg::Live));
        }
        Task::batch(tasks)
    }

    /// After unlock or an account switch: the groups of the account now open.
    pub(super) fn sync_live(&mut self) {
        if self.demo {
            return;
        }
        let did = self.me.as_ref().map(|m| m.did.clone()).unwrap_or_default();
        let ready = self.lock == p2pcore::LockState::Unlocked;
        if did == self.live.me && ready == self.live.ready {
            return;
        }
        self.live.reset(did, String::new());
        self.live.ready = ready;
        if ready {
            let node = self.node.clone();
            self.live.reload(&*node);
        }
    }
}

#[cfg(test)]
#[path = "group_live_tests.rs"]
mod tests;
