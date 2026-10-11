//! PLACEHOLDER data for the groups, hosting, blocking, chat-request, permission and
//! address-change screens (docs/design/groups/). None of these features exist in the core yet;
//! this in-memory model lets the desktop screens be seen and clicked now.
//!
//! It is shaped like the API the core is expected to grow: plain records out (`Snap` and the
//! types in it), `&self` commands that return `Result<_, GErr>`, state behind a lock. When the
//! core lands, the UI swaps `Fake` for the node and keeps the records; the `simulate_*` and
//! `advance` methods are preview-only and have no core counterpart.
//!
//! Nothing here touches the network, keys or the node. Sample data matches the boards.

use std::sync::Mutex;

/// Unix milliseconds.
pub type Ms = u64;

pub const MIN: Ms = 60_000;
pub const HOUR: Ms = 60 * MIN;
pub const DAY: Ms = 24 * HOUR;

/// Our own DID in the sample data.
pub const ME: &str = "did:preview:me";
/// Flags that remove a member.
pub const FLAGS_TO_REMOVE: usize = 3;
/// A chat request nobody answers expires after this.
pub const REQUEST_TTL: Ms = 7 * DAY;
/// One request per person per this period.
pub const REQUEST_EVERY: Ms = DAY;
/// After a removal, hosts refresh their addresses this much later.
pub const REFRESH_AFTER: Ms = 2 * DAY;

// ---- records ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Owner,
    Admin,
    Member,
}

impl Role {
    pub fn word(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
        }
    }
    pub fn is_admin(self) -> bool {
        matches!(self, Role::Owner | Role::Admin)
    }
}

/// One admin flag against a member. Signed by the admin in the real protocol.
#[derive(Clone, Debug, PartialEq)]
pub struct Flag {
    pub by_did: String,
    pub by_name: String,
    pub by_role: Role,
    pub at: Ms,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub did: String,
    pub name: String,
    pub role: Role,
    /// One of this member's desktops hosts the group (the "Host" pill).
    pub host: bool,
    pub flags: Vec<Flag>,
    pub since: Ms,
    pub invited_by: String,
    /// A linked desktop that could be asked to host ("Make host"); phones can't host.
    pub desktop: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Host {
    pub id: String,
    pub label: String,
    pub owner_did: String,
    pub this_computer: bool,
    pub online: bool,
    pub last_seen: Ms,
    /// Members connected through this host right now.
    pub connected: u32,
    pub appointed_by: String,
    /// Asked to host; the member has not accepted yet.
    pub pending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupDelivery {
    /// A host took it.
    Sent,
    /// No host reachable; it waits on this device.
    WaitingForHost,
    /// We were removed; written offline before the removal, hosts take it until they refresh.
    BeforeRemoval { until: Ms },
}

#[derive(Clone, Debug, PartialEq)]
pub enum GroupItem {
    Msg {
        id: String,
        author_did: String,
        author: String,
        text: String,
        at: Ms,
        outgoing: bool,
        delivery: GroupDelivery,
        /// Written by someone before they were removed, arriving late.
        before_removal: bool,
    },
    /// A plain grey line: joins, removals, host changes. Never addresses or keys.
    Note { id: String, at: Ms, text: String },
}

impl GroupItem {
    #[allow(dead_code)]
    pub fn at(&self) -> Ms {
        match self {
            GroupItem::Msg { at, .. } | GroupItem::Note { at, .. } => *at,
        }
    }
}

/// Why and when we were removed from a group.
#[derive(Clone, Debug, PartialEq)]
pub struct Removal {
    pub at: Ms,
    pub flags: Vec<Flag>,
    /// When the hosts refresh their addresses and stop accepting anything from us.
    pub refresh_at: Ms,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub members: Vec<Member>,
    pub hosts: Vec<Host>,
    pub items: Vec<GroupItem>,
    pub storage: u64,
    pub unread: u32,
    /// Set when we were removed; the group stays, greyed, until deleted.
    pub removed: Option<Removal>,
    pub archived: bool,
    /// Someone was asked to host because of the single-host warning.
    pub host_asked: bool,
}

impl Group {
    pub fn me(&self) -> Option<&Member> {
        self.members.iter().find(|m| m.did == ME)
    }
    pub fn my_role(&self) -> Option<Role> {
        self.me().map(|m| m.role)
    }
    pub fn am_admin(&self) -> bool {
        self.removed.is_none() && self.my_role().is_some_and(Role::is_admin)
    }
    pub fn member(&self, did: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.did == did)
    }
    #[allow(dead_code)] // core API shape; the screens don't need it yet
    pub fn owner(&self) -> Option<&Member> {
        self.members.iter().find(|m| m.role == Role::Owner)
    }
    pub fn this_computer_hosts(&self) -> bool {
        self.hosts.iter().any(|h| h.this_computer && !h.pending)
    }
    /// Accepted hosts (not the ones still asked).
    pub fn active_hosts(&self) -> impl Iterator<Item = &Host> {
        self.hosts.iter().filter(|h| !h.pending)
    }
    /// Some host can take messages now.
    pub fn reachable(&self) -> bool {
        self.active_hosts().any(|h| h.online)
    }
    #[allow(dead_code)]
    pub fn last_at(&self) -> Ms {
        self.items.last().map(GroupItem::at).unwrap_or(0)
    }
}

/// Who blocked whom. A block is personal: it never changes group membership.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    None,
    ByMe { at: Ms, attempts: u32, how: Known },
    ByThem { at: Ms },
}

/// How we knew someone we blocked (the blocked list says it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Known {
    Contact,
    ChatRequest,
    Group,
}

/// What a contact may do to us. Kept through a block, so unblocking restores it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Perms {
    pub message: bool,
    pub call: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Perm {
    Message,
    Call,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PDelivery {
    Delivered,
    Read,
    /// Not delivered, with the reason shown under the bubble.
    NotDelivered(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PMsg {
    pub id: String,
    pub outgoing: bool,
    pub at: Ms,
    pub text: String,
    pub delivery: PDelivery,
    /// A grey line, not a bubble.
    pub note: bool,
}

/// Someone we have a 1:1 relationship with (a contact, or a person we blocked).
#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub did: String,
    pub name: String,
    pub contact: bool,
    /// The group a request came through.
    pub via_group: Option<String>,
    pub verified: bool,
    pub perms: Perms,
    /// Whether they let us call them.
    pub they_allow_calls: bool,
    /// The one-time "Allow calls" chip after accepting a request.
    pub calls_chip: bool,
    pub block: Block,
    pub msgs: Vec<PMsg>,
    /// Both sides hold each other's current address.
    pub address_ok: bool,
    /// When they last got our new address.
    pub they_got_mine: Ms,
    pub unread: u32,
}

impl Person {
    pub fn blocked_by_me(&self) -> bool {
        matches!(self.block, Block::ByMe { .. })
    }
    #[allow(dead_code)] // core API shape; the screens don't need it yet
    pub fn blocked_me(&self) -> bool {
        matches!(self.block, Block::ByThem { .. })
    }
    /// What they can do right now (a block turns everything off).
    #[allow(dead_code)] // core API shape; the screens don't need it yet
    pub fn can_message_me(&self) -> bool {
        self.block == Block::None && self.perms.message
    }
    #[allow(dead_code)] // core API shape; the screens don't need it yet
    pub fn can_call_me(&self) -> bool {
        self.block == Block::None && self.perms.call
    }
    pub fn can_call_them(&self) -> bool {
        self.block == Block::None && self.they_allow_calls
    }
    pub fn messages_only(&self) -> bool {
        self.block == Block::None && !self.perms.call
    }
    pub fn last_at(&self) -> Ms {
        self.msgs.last().map(|m| m.at).unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SentState {
    Waiting,
    Declined,
    Expired,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SentRequest {
    pub id: String,
    pub to_did: String,
    pub to_name: String,
    pub group: String,
    pub note: String,
    pub at: Ms,
    pub state: SentState,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IncomingRequest {
    pub id: String,
    pub from_did: String,
    pub from_name: String,
    pub group: String,
    pub shared_groups: u32,
    pub note: String,
    pub at: Ms,
}

/// A member asks this computer to host a group.
#[derive(Clone, Debug, PartialEq)]
pub struct HostRequest {
    pub id: String,
    pub group_id: String,
    pub group_name: String,
    pub from_did: String,
    pub from_name: String,
    pub from_role: Role,
    pub members: u32,
    pub est_bytes: u64,
    pub at: Ms,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interval {
    Day,
    ThreeDays,
    Week,
}

impl Interval {
    pub fn ms(self) -> Ms {
        match self {
            Interval::Day => DAY,
            Interval::ThreeDays => 3 * DAY,
            Interval::Week => 7 * DAY,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Interval::Day => "Day",
            Interval::ThreeDays => "3 days",
            Interval::Week => "Week",
        }
    }
    /// "every day", "every 3 days", "every week".
    pub fn every(self) -> &'static str {
        match self {
            Interval::Day => "every day",
            Interval::ThreeDays => "every 3 days",
            Interval::Week => "every week",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AddressStatus {
    pub interval: Interval,
    pub last_change: Ms,
    pub next_change: Ms,
    /// Contacts that already hold the current address, of `contacts`.
    pub told: u32,
    pub contacts: u32,
}

/// Everything the screens show, read in one go after each change.
#[derive(Clone, Debug, Default)]
pub struct Snap {
    pub now: Ms,
    pub groups: Vec<Group>,
    pub people: Vec<Person>,
    pub sent: Vec<SentRequest>,
    pub incoming: Vec<IncomingRequest>,
    pub host_requests: Vec<HostRequest>,
    pub keep_running: bool,
    pub address: Option<AddressStatus>,
}

impl Snap {
    pub fn group(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }
    pub fn person(&self, did: &str) -> Option<&Person> {
        self.people.iter().find(|p| p.did == did)
    }
    pub fn blocked_by_me(&self, did: &str) -> bool {
        self.person(did).is_some_and(Person::blocked_by_me)
    }
    pub fn is_contact(&self, did: &str) -> bool {
        self.person(did).is_some_and(|p| p.contact)
    }
    /// Groups this computer hosts.
    pub fn hosted(&self) -> impl Iterator<Item = &Group> {
        self.groups.iter().filter(|g| g.removed.is_none() && g.this_computer_hosts())
    }
}

/// What a flag did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlagOutcome {
    pub count: usize,
    pub removed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GErr {
    NoSuchGroup,
    NoSuchMember,
    NoSuchPerson,
    NoSuchRequest,
    NotAdmin,
    /// We were removed from the group.
    Removed,
    /// Owners and ourselves can't be flagged (placeholder policy).
    CannotFlag,
    AlreadyAdmin,
    NoDesktop(String),
    AlreadyHost,
    NotHosting,
    AlreadyContact,
    /// One request per person per day.
    RateLimited(String),
    /// A request to them is still waiting.
    AlreadyAsked(String),
    BlockedByMe(String),
    BlockedMe(String),
    CallsNotAllowed(String),
    Empty,
}

impl std::fmt::Display for GErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GErr::NoSuchGroup => write!(f, "That group is no longer here."),
            GErr::NoSuchMember => write!(f, "That person is no longer a member."),
            GErr::NoSuchPerson => write!(f, "That person is no longer here."),
            GErr::NoSuchRequest => write!(f, "That request is no longer here."),
            GErr::NotAdmin => write!(f, "Only the owner and admins can do that."),
            GErr::Removed => write!(f, "You were removed from this group."),
            GErr::CannotFlag => write!(f, "The owner and you can\u{2019}t be flagged."),
            GErr::AlreadyAdmin => write!(f, "They are already an admin."),
            GErr::NoDesktop(n) => write!(f, "{n} has no linked desktop to host from."),
            GErr::AlreadyHost => write!(f, "That desktop already hosts this group."),
            GErr::NotHosting => write!(f, "This computer doesn\u{2019}t host that group."),
            GErr::AlreadyContact => write!(f, "You already have a chat with them."),
            GErr::RateLimited(n) => write!(f, "You can send {n} one request a day. Try again tomorrow."),
            GErr::AlreadyAsked(n) => write!(f, "Your request to {n} is still waiting."),
            GErr::BlockedByMe(n) => write!(f, "You blocked {n}. Unblock first."),
            GErr::BlockedMe(n) => write!(f, "{n} blocked you."),
            GErr::CallsNotAllowed(n) => write!(f, "{n} hasn\u{2019}t allowed calls from you."),
            GErr::Empty => write!(f, "Type a message first."),
        }
    }
}

// ---- the fake ----

#[derive(Clone, Debug, Default)]
struct State {
    now: Ms,
    seq: u64,
    me_name: String,
    groups: Vec<Group>,
    people: Vec<Person>,
    sent: Vec<SentRequest>,
    incoming: Vec<IncomingRequest>,
    host_requests: Vec<HostRequest>,
    keep_running: bool,
    address: Option<AddressStatus>,
}

/// The placeholder source. All state is in memory and resets when the app starts.
pub struct Fake {
    st: Mutex<State>,
}

impl State {
    /// A fresh id and a timestamp a second after the last one, so order is stable.
    fn next(&mut self, prefix: &str) -> (String, Ms) {
        self.seq += 1;
        self.now += 1000;
        (format!("{prefix}-{}", self.seq), self.now)
    }

    fn group_mut(&mut self, id: &str) -> Result<&mut Group, GErr> {
        self.groups.iter_mut().find(|g| g.id == id).ok_or(GErr::NoSuchGroup)
    }

    fn person_mut(&mut self, did: &str) -> Result<&mut Person, GErr> {
        self.people.iter_mut().find(|p| p.did == did).ok_or(GErr::NoSuchPerson)
    }

    /// A person's name from wherever we know them.
    fn name_of(&self, did: &str) -> Option<String> {
        self.people
            .iter()
            .find(|p| p.did == did)
            .map(|p| p.name.clone())
            .or_else(|| self.groups.iter().flat_map(|g| &g.members).find(|m| m.did == did).map(|m| m.name.clone()))
            .or_else(|| self.incoming.iter().find(|r| r.from_did == did).map(|r| r.from_name.clone()))
    }

    fn note(&mut self, gid: &str, text: String) {
        let (id, at) = self.next("note");
        if let Ok(g) = self.group_mut(gid) {
            g.items.push(GroupItem::Note { id, at, text });
        }
    }

    fn person_note(&mut self, did: &str, text: String) {
        let (id, at) = self.next("pnote");
        if let Ok(p) = self.person_mut(did) {
            p.msgs.push(PMsg { id, outgoing: false, at, text, delivery: PDelivery::Delivered, note: true });
        }
    }

    /// Messages waiting for a host go out once one is reachable.
    fn flush_waiting(&mut self, gid: &str) {
        if let Ok(g) = self.group_mut(gid)
            && g.reachable()
        {
            for it in g.items.iter_mut() {
                if let GroupItem::Msg { delivery: d @ GroupDelivery::WaitingForHost, .. } = it {
                    *d = GroupDelivery::Sent;
                }
            }
        }
    }

    /// A new contact for messages only (calls off both ways), from a request.
    fn add_request_contact(&mut self, did: &str, name: &str, group: &str, first: Vec<PMsg>) {
        let p = Person {
            did: did.into(),
            name: name.into(),
            contact: true,
            via_group: Some(group.into()),
            verified: false,
            perms: Perms { message: true, call: false },
            they_allow_calls: false,
            calls_chip: true,
            block: Block::None,
            msgs: first,
            address_ok: true,
            they_got_mine: self.now,
            unread: 0,
        };
        match self.people.iter_mut().find(|x| x.did == did) {
            Some(x) => {
                let mut msgs = std::mem::take(&mut x.msgs);
                msgs.extend(p.msgs.clone());
                *x = Person { msgs, ..p };
            }
            None => self.people.push(p),
        }
    }
}

impl Fake {
    /// The sample data from the boards, `now` in unix ms, `me_name` for lines that name us.
    pub fn sample(now: Ms, me_name: &str) -> Self {
        Fake { st: Mutex::new(sample_state(now, me_name)) }
    }

    fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.st.lock().unwrap())
    }

    pub fn snapshot(&self) -> Snap {
        self.with(|s| Snap {
            now: s.now,
            groups: s.groups.clone(),
            people: s.people.clone(),
            sent: s.sent.clone(),
            incoming: s.incoming.clone(),
            host_requests: s.host_requests.clone(),
            keep_running: s.keep_running,
            address: s.address.clone(),
        })
    }

    // ---- group chat ----

    pub fn send_group_message(&self, gid: &str, text: &str) -> Result<(), GErr> {
        let text = text.trim();
        if text.is_empty() {
            return Err(GErr::Empty);
        }
        self.with(|s| {
            let me = s.me_name.clone();
            let (id, at) = s.next("gmsg");
            let g = s.group_mut(gid)?;
            if g.removed.is_some() {
                return Err(GErr::Removed);
            }
            let delivery = if g.reachable() { GroupDelivery::Sent } else { GroupDelivery::WaitingForHost };
            g.items.push(GroupItem::Msg {
                id,
                author_did: ME.into(),
                author: me,
                text: text.into(),
                at,
                outgoing: true,
                delivery,
                before_removal: false,
            });
            Ok(())
        })
    }

    pub fn mark_group_read(&self, gid: &str) {
        self.with(|s| {
            if let Ok(g) = s.group_mut(gid) {
                g.unread = 0;
            }
        })
    }

    pub fn mark_person_read(&self, did: &str) {
        self.with(|s| {
            if let Ok(p) = s.person_mut(did) {
                p.unread = 0;
            }
        })
    }

    // ---- member actions (admins) ----

    pub fn make_admin(&self, gid: &str, did: &str) -> Result<(), GErr> {
        self.with(|s| {
            let g = s.group_mut(gid)?;
            if !g.am_admin() {
                return Err(GErr::NotAdmin);
            }
            let m = g.members.iter_mut().find(|m| m.did == did).ok_or(GErr::NoSuchMember)?;
            if m.role.is_admin() {
                return Err(GErr::AlreadyAdmin);
            }
            m.role = Role::Admin;
            let name = m.name.clone();
            s.note(gid, format!("{name} is now an admin"));
            Ok(())
        })
    }

    /// Asks one of the member's desktops to host; it shows as asked until they accept.
    pub fn ask_member_to_host(&self, gid: &str, did: &str) -> Result<(), GErr> {
        self.with(|s| {
            let me = s.me_name.clone();
            let g = s.group_mut(gid)?;
            if !g.am_admin() {
                return Err(GErr::NotAdmin);
            }
            let m = g.member(did).ok_or(GErr::NoSuchMember)?.clone();
            let label = m.desktop.clone().ok_or_else(|| GErr::NoDesktop(m.name.clone()))?;
            if g.hosts.iter().any(|h| h.owner_did == did) {
                return Err(GErr::AlreadyHost);
            }
            g.hosts.push(Host {
                id: format!("host-{did}"),
                label: label.clone(),
                owner_did: did.into(),
                this_computer: false,
                online: false,
                last_seen: 0,
                connected: 0,
                appointed_by: me,
                pending: true,
            });
            s.note(gid, format!("{label} was asked to host"));
            Ok(())
        })
    }

    /// Preview: the member accepts hosting on their desktop.
    pub fn simulate_host_accept(&self, gid: &str, did: &str) -> Result<(), GErr> {
        self.with(|s| {
            let now = s.now;
            let g = s.group_mut(gid)?;
            let members = g.members.len() as u32;
            let h = g.hosts.iter_mut().find(|h| h.owner_did == did && h.pending).ok_or(GErr::NoSuchRequest)?;
            h.pending = false;
            h.online = true;
            h.last_seen = now;
            h.connected = members.saturating_sub(1);
            let label = h.label.clone();
            if let Some(m) = g.members.iter_mut().find(|m| m.did == did) {
                m.host = true;
            }
            s.note(gid, format!("{label} now hosts the group"));
            s.flush_waiting(gid);
            Ok(())
        })
    }

    /// One admin flag. At `FLAGS_TO_REMOVE` the member is removed at once. The same admin
    /// flagging again counts (per the board).
    pub fn flag_member(&self, gid: &str, did: &str) -> Result<FlagOutcome, GErr> {
        self.with(|s| {
            let (me_name, now) = (s.me_name.clone(), s.now + 1000);
            s.now = now;
            let g = s.group_mut(gid)?;
            if g.removed.is_some() {
                return Err(GErr::Removed);
            }
            let my_role = g.my_role().filter(|r| r.is_admin()).ok_or(GErr::NotAdmin)?;
            let m = g.members.iter_mut().find(|m| m.did == did).ok_or(GErr::NoSuchMember)?;
            if m.role == Role::Owner || m.did == ME {
                return Err(GErr::CannotFlag);
            }
            m.flags.push(Flag { by_did: ME.into(), by_name: me_name, by_role: my_role, at: now });
            let count = m.flags.len();
            let removed = count >= FLAGS_TO_REMOVE;
            if removed {
                let name = m.name.clone();
                g.members.retain(|m| m.did != did);
                g.hosts.retain(|h| h.owner_did != did);
                s.note(gid, format!("{name} was removed after {FLAGS_TO_REMOVE} admin flags"));
            }
            Ok(FlagOutcome { count, removed })
        })
    }

    /// The removed person's "Delete group".
    pub fn delete_group(&self, gid: &str) -> Result<(), GErr> {
        self.with(|s| {
            let n = s.groups.len();
            s.groups.retain(|g| g.id != gid);
            if s.groups.len() == n { Err(GErr::NoSuchGroup) } else { Ok(()) }
        })
    }

    /// The removed person's "Keep as archive".
    pub fn archive_group(&self, gid: &str) -> Result<(), GErr> {
        self.with(|s| {
            s.group_mut(gid)?.archived = true;
            Ok(())
        })
    }

    // ---- hosting on this computer ----

    pub fn set_keep_running(&self, on: bool) {
        self.with(|s| s.keep_running = on)
    }

    /// Accepting signs an acceptance; the group announces the new host.
    pub fn accept_host_request(&self, id: &str) -> Result<String, GErr> {
        self.with(|s| {
            let i = s.host_requests.iter().position(|r| r.id == id).ok_or(GErr::NoSuchRequest)?;
            let r = s.host_requests.remove(i);
            let now = s.now;
            let g = s.group_mut(&r.group_id)?;
            if g.this_computer_hosts() {
                return Err(GErr::AlreadyHost);
            }
            let members = g.members.len() as u32;
            g.hosts.insert(
                0,
                Host {
                    id: "host-this".into(),
                    label: "This computer".into(),
                    owner_did: ME.into(),
                    this_computer: true,
                    online: true,
                    last_seen: now,
                    connected: members.saturating_sub(1),
                    appointed_by: r.from_name.clone(),
                    pending: false,
                },
            );
            if g.storage == 0 {
                g.storage = r.est_bytes;
            }
            if let Some(m) = g.members.iter_mut().find(|m| m.did == ME) {
                m.host = true;
            }
            s.note(&r.group_id, "This computer now hosts the group".into());
            s.flush_waiting(&r.group_id);
            Ok(r.group_id)
        })
    }

    pub fn decline_host_request(&self, id: &str) -> Result<(), GErr> {
        self.with(|s| {
            let n = s.host_requests.len();
            s.host_requests.retain(|r| r.id != id);
            if s.host_requests.len() == n { Err(GErr::NoSuchRequest) } else { Ok(()) }
        })
    }

    /// Stops hosting; the group's admins are told and see "Needs a host" if none is left.
    pub fn stop_hosting(&self, gid: &str) -> Result<(), GErr> {
        self.with(|s| {
            let g = s.group_mut(gid)?;
            if !g.this_computer_hosts() {
                return Err(GErr::NotHosting);
            }
            g.hosts.retain(|h| !h.this_computer);
            if let Some(m) = g.members.iter_mut().find(|m| m.did == ME) {
                m.host = false;
            }
            let none_left = g.active_hosts().next().is_none();
            s.note(
                gid,
                if none_left {
                    "This computer stopped hosting. The group needs a host.".into()
                } else {
                    "This computer stopped hosting".into()
                },
            );
            Ok(())
        })
    }

    /// The single-host warning's "Ask a member to host".
    pub fn ask_for_host(&self, gid: &str) -> Result<(), GErr> {
        self.with(|s| {
            s.group_mut(gid)?.host_asked = true;
            s.note(gid, "Members with a desktop were asked to host".into());
            Ok(())
        })
    }

    // ---- chat requests ----

    /// "Message privately" on a member who is not a contact. Sealed to them via the host.
    pub fn send_chat_request(&self, gid: &str, did: &str, note: &str) -> Result<String, GErr> {
        self.with(|s| {
            let g = s.groups.iter().find(|g| g.id == gid).ok_or(GErr::NoSuchGroup)?;
            if g.removed.is_some() {
                return Err(GErr::Removed);
            }
            let m = g.member(did).ok_or(GErr::NoSuchMember)?;
            let (name, group) = (m.name.clone(), g.name.clone());
            if let Some(p) = s.people.iter().find(|p| p.did == did) {
                match p.block {
                    Block::ByMe { .. } => return Err(GErr::BlockedByMe(name)),
                    Block::ByThem { .. } => return Err(GErr::BlockedMe(name)),
                    Block::None if p.contact => return Err(GErr::AlreadyContact),
                    Block::None => {}
                }
            }
            if s.sent.iter().any(|r| r.to_did == did && r.state == SentState::Waiting) {
                return Err(GErr::AlreadyAsked(name));
            }
            if s.sent.iter().any(|r| r.to_did == did && s.now.saturating_sub(r.at) < REQUEST_EVERY) {
                return Err(GErr::RateLimited(name));
            }
            let (id, at) = s.next("req");
            s.sent.insert(
                0,
                SentRequest { id: id.clone(), to_did: did.into(), to_name: name, group, note: note.trim().into(), at, state: SentState::Waiting },
            );
            Ok(id)
        })
    }

    pub fn cancel_request(&self, id: &str) -> Result<(), GErr> {
        self.with(|s| {
            let n = s.sent.len();
            s.sent.retain(|r| r.id != id);
            if s.sent.len() == n { Err(GErr::NoSuchRequest) } else { Ok(()) }
        })
    }

    /// Accepting adds them for messages only; the note becomes the first message. Returns their DID.
    pub fn accept_request(&self, id: &str) -> Result<String, GErr> {
        self.with(|s| {
            let i = s.incoming.iter().position(|r| r.id == id).ok_or(GErr::NoSuchRequest)?;
            let r = s.incoming.remove(i);
            let (nid, nat) = s.next("pnote");
            let (mid, _) = s.next("pmsg");
            let mut first = vec![PMsg {
                id: nid,
                outgoing: false,
                at: nat,
                text: format!("You accepted {}\u{2019}s request from {}", first_name(&r.from_name), r.group),
                delivery: PDelivery::Delivered,
                note: true,
            }];
            if !r.note.is_empty() {
                first.push(PMsg { id: mid, outgoing: false, at: r.at, text: r.note.clone(), delivery: PDelivery::Delivered, note: false });
            }
            s.add_request_contact(&r.from_did, &r.from_name, &r.group, first);
            Ok(r.from_did)
        })
    }

    /// They are told "declined".
    pub fn decline_request(&self, id: &str) -> Result<(), GErr> {
        self.with(|s| {
            let n = s.incoming.len();
            s.incoming.retain(|r| r.id != id);
            if s.incoming.len() == n { Err(GErr::NoSuchRequest) } else { Ok(()) }
        })
    }

    /// The third choice: they are told they are blocked and can't send more requests.
    pub fn block_request(&self, id: &str) -> Result<(), GErr> {
        self.with(|s| {
            let i = s.incoming.iter().position(|r| r.id == id).ok_or(GErr::NoSuchRequest)?;
            let r = s.incoming.remove(i);
            let at = s.now;
            match s.people.iter_mut().find(|p| p.did == r.from_did) {
                Some(p) => p.block = Block::ByMe { at, attempts: 0, how: Known::ChatRequest },
                None => s.people.push(blocked_stranger(&r.from_did, &r.from_name, at, Known::ChatRequest)),
            }
            Ok(())
        })
    }

    /// Preview: the person answers our request.
    pub fn simulate_reply(&self, id: &str, accept: bool) -> Result<(), GErr> {
        self.with(|s| {
            let i = s.sent.iter().position(|r| r.id == id && r.state == SentState::Waiting).ok_or(GErr::NoSuchRequest)?;
            if !accept {
                s.sent[i].state = SentState::Declined;
                return Ok(());
            }
            let r = s.sent.remove(i);
            let (nid, nat) = s.next("pnote");
            let (mid, _) = s.next("pmsg");
            let mut first = vec![PMsg {
                id: nid,
                outgoing: false,
                at: nat,
                text: format!("{} accepted your request from {}", first_name(&r.to_name), r.group),
                delivery: PDelivery::Delivered,
                note: true,
            }];
            if !r.note.is_empty() {
                first.insert(0, PMsg { id: mid, outgoing: true, at: r.at, text: r.note.clone(), delivery: PDelivery::Read, note: false });
            }
            s.add_request_contact(&r.to_did, &r.to_name, &r.group, first);
            Ok(())
        })
    }

    /// Preview: time passes; waiting requests older than `REQUEST_TTL` expire.
    pub fn advance(&self, ms: Ms) {
        self.with(|s| {
            s.now += ms;
            let now = s.now;
            for r in s.sent.iter_mut() {
                if r.state == SentState::Waiting && now.saturating_sub(r.at) >= REQUEST_TTL {
                    r.state = SentState::Expired;
                }
            }
        })
    }

    // ---- people: block, permissions, messages ----

    /// Blocks on all our devices. They are told; shared groups are unaffected.
    pub fn block(&self, did: &str) -> Result<(), GErr> {
        self.with(|s| {
            let at = s.now + 1000;
            s.now = at;
            let name = s.name_of(did).ok_or(GErr::NoSuchPerson)?;
            match s.people.iter_mut().find(|p| p.did == did) {
                Some(p) => {
                    let how = if p.contact { Known::Contact } else { Known::Group };
                    p.block = Block::ByMe { at, attempts: 0, how };
                }
                None => s.people.push(blocked_stranger(did, &name, at, Known::Group)),
            }
            // A waiting request to someone we block is withdrawn.
            s.sent.retain(|r| !(r.to_did == did && r.state == SentState::Waiting));
            s.incoming.retain(|r| r.from_did != did);
            Ok(())
        })
    }

    /// They can message us again and get our current address; calls only if allowed before.
    pub fn unblock(&self, did: &str) -> Result<(), GErr> {
        self.with(|s| {
            let p = s.person_mut(did)?;
            if !p.blocked_by_me() {
                return Ok(());
            }
            p.block = Block::None;
            let (contact, name) = (p.contact, p.name.clone());
            if contact {
                s.person_note(did, format!("You unblocked {name}"));
            } else {
                // Not a contact: nothing to keep once the block is gone.
                s.people.retain(|p| p.did != did);
            }
            Ok(())
        })
    }

    pub fn set_permission(&self, did: &str, perm: Perm, on: bool) -> Result<(), GErr> {
        self.with(|s| {
            let p = s.person_mut(did)?;
            match perm {
                Perm::Message => p.perms.message = on,
                Perm::Call => {
                    p.perms.call = on;
                    p.calls_chip = false;
                }
            }
            Ok(())
        })
    }

    pub fn dismiss_calls_chip(&self, did: &str) {
        self.with(|s| {
            if let Ok(p) = s.person_mut(did) {
                p.calls_chip = false;
            }
        })
    }

    pub fn send_message(&self, did: &str, text: &str) -> Result<(), GErr> {
        let text = text.trim();
        if text.is_empty() {
            return Err(GErr::Empty);
        }
        self.with(|s| {
            let (id, at) = s.next("pmsg");
            let p = s.person_mut(did)?;
            match p.block {
                Block::ByMe { .. } => return Err(GErr::BlockedByMe(p.name.clone())),
                Block::ByThem { .. } => return Err(GErr::BlockedMe(p.name.clone())),
                Block::None => {}
            }
            p.msgs.push(PMsg { id, outgoing: true, at, text: text.into(), delivery: PDelivery::Delivered, note: false });
            Ok(())
        })
    }

    /// Placeholder call: refused unless they allow calls from us. Never dials.
    pub fn call(&self, did: &str) -> Result<(), GErr> {
        self.with(|s| {
            let p = s.person_mut(did)?;
            match p.block {
                Block::ByMe { .. } => Err(GErr::BlockedByMe(p.name.clone())),
                Block::ByThem { .. } => Err(GErr::BlockedMe(p.name.clone())),
                Block::None if !p.they_allow_calls => Err(GErr::CallsNotAllowed(p.name.clone())),
                Block::None => Ok(()),
            }
        })
    }

    /// Sends the plain "<me> would like to be able to call you" message.
    pub fn ask_to_allow_calls(&self, did: &str) -> Result<(), GErr> {
        let me = self.with(|s| s.me_name.clone());
        self.send_message(did, &format!("{} would like to be able to call you", first_name(&me)))
    }

    /// Preview: a blocked person tries to message us; counted, never shown or notified.
    pub fn simulate_attempt(&self, did: &str) -> Result<u32, GErr> {
        self.with(|s| {
            let p = s.person_mut(did)?;
            match &mut p.block {
                Block::ByMe { attempts, .. } => {
                    *attempts += 1;
                    Ok(*attempts)
                }
                _ => Err(GErr::NoSuchPerson),
            }
        })
    }

    // ---- address changes ----

    pub fn set_interval(&self, i: Interval) {
        self.with(|s| {
            if let Some(a) = s.address.as_mut() {
                a.interval = i;
                a.next_change = a.last_change + i.ms();
            }
        })
    }
}

pub fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

fn blocked_stranger(did: &str, name: &str, at: Ms, how: Known) -> Person {
    Person {
        did: did.into(),
        name: name.into(),
        contact: false,
        via_group: None,
        verified: false,
        perms: Perms { message: false, call: false },
        they_allow_calls: false,
        calls_chip: false,
        block: Block::ByMe { at, attempts: 0, how },
        msgs: Vec::new(),
        address_ok: false,
        they_got_mine: 0,
        unread: 0,
    }
}

// ---- sample data (the boards) ----

fn did(k: &str) -> String {
    format!("did:preview:{k}")
}

fn member(k: &str, name: &str, role: Role, since: Ms, invited_by: &str, desktop: Option<&str>) -> Member {
    Member {
        did: if k == "me" { ME.into() } else { did(k) },
        name: name.into(),
        role,
        host: false,
        flags: Vec::new(),
        since,
        invited_by: invited_by.into(),
        desktop: desktop.map(str::to_string),
    }
}

fn msg(id: &str, k: &str, author: &str, text: &str, at: Ms) -> GroupItem {
    GroupItem::Msg {
        id: id.into(),
        author_did: if k == "me" { ME.into() } else { did(k) },
        author: author.into(),
        text: text.into(),
        at,
        outgoing: k == "me",
        delivery: GroupDelivery::Sent,
        before_removal: false,
    }
}

fn note(id: &str, text: &str, at: Ms) -> GroupItem {
    GroupItem::Note { id: id.into(), at, text: text.into() }
}

fn pmsg(id: &str, outgoing: bool, at: Ms, text: &str, delivery: PDelivery) -> PMsg {
    PMsg { id: id.into(), outgoing, at, text: text.into(), delivery, note: false }
}

fn contact(k: &str, name: &str, msgs: Vec<PMsg>) -> Person {
    Person {
        did: did(k),
        name: name.into(),
        contact: true,
        via_group: None,
        verified: true,
        perms: Perms { message: true, call: true },
        they_allow_calls: true,
        calls_chip: false,
        block: Block::None,
        msgs,
        address_ok: true,
        they_got_mine: 0,
        unread: 0,
    }
}

fn flag(k: &str, name: &str, role: Role, at: Ms) -> Flag {
    Flag { by_did: did(k), by_name: name.into(), by_role: role, at }
}

fn sample_state(now: Ms, me_name: &str) -> State {
    let me = if me_name.trim().is_empty() { "You".to_string() } else { me_name.trim().to_string() };
    let ago = |ms: Ms| now.saturating_sub(ms);

    // Weekend hike: Meera owns it; this computer and Meera's desktop host it.
    let mut hike_members = vec![
        member("meera", "Meera K", Role::Owner, ago(40 * DAY), "", Some("Meera\u{2019}s desktop")),
        member("me", &me, Role::Admin, ago(38 * DAY), "Meera K", None),
        member("sam", "Sam Rao", Role::Admin, ago(38 * DAY), "Meera K", Some("Sam\u{2019}s desktop")),
        member("priya", "Priya N", Role::Admin, ago(30 * DAY), "Sam Rao", None),
        member("dev", "Dev T", Role::Member, ago(20 * DAY), "Sam Rao", Some("Dev\u{2019}s laptop")),
        member("arjun", "Arjun Oommen", Role::Member, ago(25 * DAY), "Meera K", Some("Arjun\u{2019}s desktop")),
        member("omar", "Omar F", Role::Member, ago(8 * DAY), "Sam Rao", None),
        member("nina", "Nina S", Role::Member, ago(5 * DAY), "Priya N", None),
    ];
    hike_members[0].host = true;
    hike_members[1].host = true;
    hike_members[4].flags.push(flag("sam", "Sam Rao", Role::Admin, ago(DAY + 3 * HOUR)));
    let hike = Group {
        id: "hike".into(),
        name: "Weekend hike".into(),
        members: hike_members,
        hosts: vec![
            Host {
                id: "host-this".into(),
                label: "This computer".into(),
                owner_did: ME.into(),
                this_computer: true,
                online: true,
                last_seen: now,
                connected: 7,
                appointed_by: "Meera".into(),
                pending: false,
            },
            Host {
                id: "host-meera".into(),
                label: "Meera\u{2019}s desktop".into(),
                owner_did: did("meera"),
                this_computer: false,
                online: false,
                last_seen: ago(2 * HOUR),
                connected: 0,
                appointed_by: "Meera".into(),
                pending: false,
            },
        ],
        items: vec![
            note("h1", "Leo joined, invited by Sam", ago(2 * DAY)),
            msg("h2", "sam", "Sam Rao", "Saturday 7am at the trailhead. Bring rain jackets.", ago(5 * HOUR)),
            msg("h3", "priya", "Priya N", "Can someone give me a lift from the station?", ago(4 * HOUR + 40 * MIN)),
            msg("h4", "me", &me, "I can, I\u{2019}ll message you.", ago(4 * HOUR + 30 * MIN)),
            msg("h5", "arjun", "Arjun Oommen", "Who\u{2019}s bringing the stove?", ago(3 * HOUR)),
            note("h6", "Leo was removed after 3 admin flags", ago(2 * HOUR)),
            GroupItem::Msg {
                id: "h7".into(),
                author_did: did("leo"),
                author: "Leo B".into(),
                text: "Sent this on the train yesterday, just got signal.".into(),
                at: ago(2 * HOUR - 5 * MIN),
                outgoing: false,
                delivery: GroupDelivery::Sent,
                before_removal: true,
            },
            msg("h8", "sam", "Sam Rao", "Map attached: trail-loop.gpx", ago(80 * MIN)),
        ],
        storage: 42_000_000,
        unread: 4,
        removed: None,
        archived: false,
        host_asked: false,
    };

    // Family: we own it and this computer is its only host.
    let mut fam_members = vec![
        member("me", &me, Role::Owner, ago(200 * DAY), "", None),
        member("asha", "Asha O", Role::Member, ago(199 * DAY), &me, None),
        member("thomas", "Thomas O", Role::Member, ago(199 * DAY), &me, Some("Thomas\u{2019} PC")),
        member("anil", "Anil O", Role::Member, ago(150 * DAY), &me, None),
        member("rhea", "Rhea O", Role::Member, ago(90 * DAY), &me, None),
    ];
    fam_members[0].host = true;
    let family = Group {
        id: "family".into(),
        name: "Family".into(),
        members: fam_members,
        hosts: vec![Host {
            id: "host-this".into(),
            label: "This computer".into(),
            owner_did: ME.into(),
            this_computer: true,
            online: true,
            last_seen: now,
            connected: 5,
            appointed_by: "you".into(),
            pending: false,
        }],
        items: vec![
            msg("f1", "asha", "Asha O", "Lovely lunch today", ago(DAY + 4 * HOUR)),
            msg("f2", "me", &me, "Photos from Sunday", ago(DAY + 3 * HOUR)),
        ],
        storage: 380_000_000,
        unread: 0,
        removed: None,
        archived: false,
        host_asked: false,
    };

    // Climbing Sundays: its only host is offline; Meera asks this computer to host it.
    let mut climb_members = vec![
        member("meera", "Meera K", Role::Owner, ago(60 * DAY), "", Some("Meera\u{2019}s desktop")),
        member("me", &me, Role::Member, ago(50 * DAY), "Meera K", None),
        member("sam", "Sam Rao", Role::Admin, ago(60 * DAY), "Meera K", Some("Sam\u{2019}s desktop")),
        member("tara", "Tara M", Role::Member, ago(30 * DAY), "Sam Rao", None),
        member("joel", "Joel P", Role::Member, ago(30 * DAY), "Sam Rao", None),
        member("ana", "Ana R", Role::Member, ago(10 * DAY), "Meera K", None),
    ];
    climb_members[2].host = true;
    let waiting = |id: &str, text: &str, at: Ms| {
        let mut m = msg(id, "me", &me, text, at);
        if let GroupItem::Msg { delivery, .. } = &mut m {
            *delivery = GroupDelivery::WaitingForHost;
        }
        m
    };
    let climb = Group {
        id: "climb".into(),
        name: "Climbing Sundays".into(),
        members: climb_members,
        hosts: vec![Host {
            id: "host-sam".into(),
            label: "Sam\u{2019}s desktop".into(),
            owner_did: did("sam"),
            this_computer: false,
            online: false,
            last_seen: ago(DAY + 2 * HOUR),
            connected: 0,
            appointed_by: "Meera".into(),
            pending: false,
        }],
        items: vec![
            msg("c1", "sam", "Sam Rao", "Who has the tent?", ago(26 * HOUR)),
            waiting("c2", "I do", ago(3 * HOUR)),
            waiting("c3", "Also two sleeping mats", ago(3 * HOUR - 2 * MIN)),
        ],
        storage: 0,
        unread: 0,
        removed: None,
        archived: false,
        host_asked: false,
    };

    // Book club: we were removed after three admin flags.
    let removed_at = ago(3 * HOUR);
    let book = Group {
        id: "book".into(),
        name: "Book club".into(),
        members: vec![
            member("meera", "Meera K", Role::Owner, ago(90 * DAY), "", None),
            member("sam", "Sam Rao", Role::Admin, ago(90 * DAY), "Meera K", Some("Sam\u{2019}s desktop")),
            member("priya", "Priya N", Role::Admin, ago(80 * DAY), "Meera K", None),
        ],
        hosts: vec![Host {
            id: "host-sam".into(),
            label: "Sam\u{2019}s desktop".into(),
            owner_did: did("sam"),
            this_computer: false,
            online: true,
            last_seen: now,
            connected: 3,
            appointed_by: "Meera".into(),
            pending: false,
        }],
        items: vec![
            msg("b1", "sam", "Sam Rao", "Next pick: The Overstory.", ago(DAY + 5 * HOUR)),
            msg("b2", "me", &me, "I\u{2019}ll bring the snacks", ago(DAY + 4 * HOUR)),
            note("b3", "You were removed from the group", removed_at),
            GroupItem::Msg {
                id: "b4".into(),
                author_did: ME.into(),
                author: me.clone(),
                text: "Running late, start without me".into(),
                at: removed_at - 20 * MIN,
                outgoing: true,
                delivery: GroupDelivery::BeforeRemoval { until: removed_at + REFRESH_AFTER },
                before_removal: false,
            },
        ],
        storage: 9_000_000,
        unread: 0,
        removed: Some(Removal {
            at: removed_at,
            flags: vec![
                flag("sam", "Sam Rao", Role::Admin, ago(DAY + 2 * HOUR)),
                flag("meera", "Meera K", Role::Owner, ago(8 * HOUR)),
                flag("priya", "Priya N", Role::Admin, removed_at),
            ],
            refresh_at: removed_at + REFRESH_AFTER,
        }),
        archived: false,
        host_asked: false,
    };

    let mut priya = contact(
        "priya",
        "Priya N",
        vec![
            PMsg {
                id: "p0".into(),
                outgoing: false,
                at: ago(4 * HOUR + 20 * MIN),
                text: "You accepted Priya\u{2019}s request from Weekend hike".into(),
                delivery: PDelivery::Delivered,
                note: true,
            },
            pmsg("p1", false, ago(4 * HOUR + 21 * MIN), "Hi, it\u{2019}s Priya. Can I ask about the lift?", PDelivery::Delivered),
            pmsg("p2", true, ago(4 * HOUR + 15 * MIN), "Sure, I\u{2019}m driving from the station at 6:30", PDelivery::Read),
        ],
    );
    priya.via_group = Some("Weekend hike".into());
    priya.perms = Perms { message: true, call: false };
    priya.they_allow_calls = false;
    priya.calls_chip = true;
    priya.they_got_mine = ago(DAY);

    let mut arjun = contact(
        "arjun",
        "Arjun Oommen",
        vec![
            pmsg("a1", false, ago(DAY + 6 * HOUR), "Are you coming Saturday?", PDelivery::Delivered),
            pmsg("a2", true, ago(DAY + 5 * HOUR), "Not sure yet", PDelivery::Read),
        ],
    );
    arjun.block = Block::ByMe { at: ago(40 * MIN), attempts: 2, how: Known::Contact };

    let mut kiran = contact(
        "kiran",
        "Kiran D",
        vec![
            pmsg("k1", true, ago(DAY + 2 * HOUR), "Are you coming Saturday?", PDelivery::Read),
            pmsg("k2", false, ago(DAY + HOUR), "Not sure yet", PDelivery::Delivered),
            pmsg("k3", true, ago(50 * MIN), "Hello?", PDelivery::NotDelivered("Kiran blocked you".into())),
        ],
    );
    kiran.block = Block::ByThem { at: ago(HOUR) };

    let meera = contact("meera", "Meera K", vec![pmsg("m1", false, ago(2 * DAY), "Can your desktop host the climbing group?", PDelivery::Delivered)]);
    let sam = contact("sam", "Sam Rao", vec![pmsg("s1", false, ago(3 * DAY), "Trail map is in the group", PDelivery::Delivered)]);
    let dev = blocked_stranger(&did("dev"), "Dev T", ago(7 * DAY), Known::ChatRequest);

    let mut people = vec![meera, sam, priya, arjun, kiran, dev];
    for p in people.iter_mut().filter(|p| p.contact && p.they_got_mine == 0) {
        p.they_got_mine = ago(DAY);
    }

    State {
        now,
        seq: 0,
        me_name: me.clone(),
        groups: vec![hike, family, climb, book],
        people,
        sent: vec![
            SentRequest {
                id: "sent-tara".into(),
                to_did: did("tara"),
                to_name: "Tara M".into(),
                group: "Climbing Sundays".into(),
                note: String::new(),
                at: ago(7 * DAY),
                state: SentState::Declined,
            },
            SentRequest {
                id: "sent-ravi".into(),
                to_did: did("ravi"),
                to_name: "Ravi J".into(),
                group: "Book club".into(),
                note: "Hi Ravi, about the book swap".into(),
                at: ago(10 * DAY),
                state: SentState::Expired,
            },
        ],
        incoming: vec![IncomingRequest {
            id: "in-nina".into(),
            from_did: did("nina"),
            from_name: "Nina S".into(),
            group: "Weekend hike".into(),
            shared_groups: 1,
            note: "Hi, it\u{2019}s Nina from the hike. Are there seats left in your car?".into(),
            at: ago(35 * MIN),
        }],
        host_requests: vec![HostRequest {
            id: "host-climb".into(),
            group_id: "climb".into(),
            group_name: "Climbing Sundays".into(),
            from_did: did("meera"),
            from_name: "Meera K".into(),
            from_role: Role::Owner,
            members: 6,
            est_bytes: 25_000_000,
            at: ago(2 * DAY),
        }],
        keep_running: true,
        address: Some(AddressStatus {
            interval: Interval::ThreeDays,
            last_change: ago(21 * HOUR),
            next_change: ago(21 * HOUR) + Interval::ThreeDays.ms(),
            told: 11,
            contacts: 12,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Ms = 1_760_000_000_000;

    fn fake() -> Fake {
        Fake::sample(NOW, "Maya Fernandes")
    }

    fn member_flags(f: &Fake, gid: &str, did: &str) -> Option<usize> {
        f.snapshot().group(gid).and_then(|g| g.member(did)).map(|m| m.flags.len())
    }

    #[test]
    fn sample_matches_the_boards() {
        let s = fake().snapshot();
        let hike = s.group("hike").unwrap();
        assert_eq!(hike.members.len(), 8);
        assert!(hike.this_computer_hosts());
        assert!(hike.am_admin());
        assert_eq!(hike.hosts.len(), 2);
        assert_eq!(s.hosted().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["hike", "family"]);
        assert!(s.group("book").unwrap().removed.is_some());
        assert!(!s.group("climb").unwrap().reachable());
        assert_eq!(s.host_requests.len(), 1);
        assert_eq!(s.incoming.len(), 1);
        assert!(s.person(&did("arjun")).unwrap().blocked_by_me());
        assert!(s.person(&did("kiran")).unwrap().blocked_me());
        assert!(s.person(&did("priya")).unwrap().messages_only());
    }

    #[test]
    fn three_flags_remove_a_member() {
        let f = fake();
        let dev = did("dev");
        assert_eq!(member_flags(&f, "hike", &dev), Some(1));
        let o = f.flag_member("hike", &dev).unwrap();
        assert_eq!(o, FlagOutcome { count: 2, removed: false });
        // The same admin flagging again counts.
        let o = f.flag_member("hike", &dev).unwrap();
        assert_eq!(o, FlagOutcome { count: 3, removed: true });
        let s = f.snapshot();
        let g = s.group("hike").unwrap();
        assert!(g.member(&dev).is_none());
        assert_eq!(g.members.len(), 7);
        assert!(matches!(g.items.last(), Some(GroupItem::Note { text, .. }) if text == "Dev T was removed after 3 admin flags"));
        // Gone: nothing more to flag.
        assert_eq!(f.flag_member("hike", &dev), Err(GErr::NoSuchMember));
    }

    #[test]
    fn only_admins_flag_and_never_the_owner_or_self() {
        let f = fake();
        assert_eq!(f.flag_member("hike", &did("meera")), Err(GErr::CannotFlag));
        assert_eq!(f.flag_member("hike", ME), Err(GErr::CannotFlag));
        // In Climbing Sundays we are a plain member.
        assert_eq!(f.flag_member("climb", &did("tara")), Err(GErr::NotAdmin));
        assert_eq!(f.flag_member("book", &did("sam")), Err(GErr::Removed));
        assert_eq!(member_flags(&f, "climb", &did("tara")), Some(0));
    }

    #[test]
    fn make_admin_and_make_host() {
        let f = fake();
        let omar = did("omar");
        f.make_admin("hike", &omar).unwrap();
        assert_eq!(f.snapshot().group("hike").unwrap().member(&omar).unwrap().role, Role::Admin);
        assert_eq!(f.make_admin("hike", &omar), Err(GErr::AlreadyAdmin));
        // Omar has no desktop; Dev has a laptop.
        assert_eq!(f.ask_member_to_host("hike", &omar), Err(GErr::NoDesktop("Omar F".into())));
        f.ask_member_to_host("hike", &did("dev")).unwrap();
        let s = f.snapshot();
        let h = s.group("hike").unwrap().hosts.iter().find(|h| h.owner_did == did("dev")).unwrap().clone();
        assert!(h.pending);
        assert_eq!(f.ask_member_to_host("hike", &did("dev")), Err(GErr::AlreadyHost));
        f.simulate_host_accept("hike", &did("dev")).unwrap();
        let s = f.snapshot();
        let g = s.group("hike").unwrap();
        assert!(g.hosts.iter().any(|h| h.owner_did == did("dev") && !h.pending && h.online));
        assert!(g.member(&did("dev")).unwrap().host);
    }

    #[test]
    fn block_and_unblock_keep_permissions() {
        let f = fake();
        let priya = did("priya");
        f.set_permission(&priya, Perm::Call, true).unwrap();
        f.block(&priya).unwrap();
        let s = f.snapshot();
        let p = s.person(&priya).unwrap();
        assert!(p.blocked_by_me());
        assert!(!p.can_message_me() && !p.can_call_me());
        assert_eq!(f.send_message(&priya, "hi"), Err(GErr::BlockedByMe("Priya N".into())));
        // A block is personal: she stays in the group.
        assert!(s.group("hike").unwrap().member(&priya).is_some());
        f.unblock(&priya).unwrap();
        let s = f.snapshot();
        let p = s.person(&priya).unwrap();
        assert_eq!(p.block, Block::None);
        // Calls come back only because they were allowed before.
        assert!(p.can_call_me() && p.can_message_me());
        f.send_message(&priya, "hi again").unwrap();
    }

    #[test]
    fn blocking_a_member_who_is_not_a_contact() {
        let f = fake();
        let omar = did("omar");
        f.block(&omar).unwrap();
        let s = f.snapshot();
        let p = s.person(&omar).unwrap();
        assert!(!p.contact);
        assert_eq!(p.block, Block::ByMe { at: NOW + 1000, attempts: 0, how: Known::Group });
        assert!(s.group("hike").unwrap().member(&omar).is_some());
        assert_eq!(f.send_chat_request("hike", &omar, ""), Err(GErr::BlockedByMe("Omar F".into())));
        f.unblock(&omar).unwrap();
        assert!(f.snapshot().person(&omar).is_none());
    }

    #[test]
    fn blocked_attempts_are_counted() {
        let f = fake();
        let arjun = did("arjun");
        assert_eq!(f.simulate_attempt(&arjun), Ok(3));
        assert_eq!(f.simulate_attempt(&did("meera")), Err(GErr::NoSuchPerson));
    }

    #[test]
    fn blocked_by_them_refuses_messages_and_calls() {
        let f = fake();
        let kiran = did("kiran");
        assert_eq!(f.send_message(&kiran, "hello"), Err(GErr::BlockedMe("Kiran D".into())));
        assert_eq!(f.call(&kiran), Err(GErr::BlockedMe("Kiran D".into())));
    }

    #[test]
    fn chat_request_lifecycle() {
        let f = fake();
        let omar = did("omar");
        let id = f.send_chat_request("hike", &omar, "  About the lift  ").unwrap();
        let s = f.snapshot();
        assert_eq!(s.sent[0].state, SentState::Waiting);
        assert_eq!(s.sent[0].note, "About the lift");
        assert_eq!(f.send_chat_request("hike", &omar, ""), Err(GErr::AlreadyAsked("Omar F".into())));
        f.simulate_reply(&id, true).unwrap();
        let s = f.snapshot();
        assert!(s.sent.iter().all(|r| r.id != id));
        let p = s.person(&omar).unwrap();
        assert!(p.contact && p.messages_only() && !p.can_call_them() && p.calls_chip);
        assert_eq!(p.via_group.as_deref(), Some("Weekend hike"));
        assert_eq!(f.send_chat_request("hike", &omar, ""), Err(GErr::AlreadyContact));
        // Members who are contacts already open their chat instead.
        assert_eq!(f.send_chat_request("hike", &did("sam"), ""), Err(GErr::AlreadyContact));
    }

    #[test]
    fn declined_requests_are_rate_limited_then_expire() {
        let f = fake();
        let omar = did("omar");
        let id = f.send_chat_request("hike", &omar, "").unwrap();
        f.simulate_reply(&id, false).unwrap();
        assert_eq!(f.snapshot().sent[0].state, SentState::Declined);
        // One request per person per day.
        assert_eq!(f.send_chat_request("hike", &omar, ""), Err(GErr::RateLimited("Omar F".into())));
        f.advance(DAY);
        let id2 = f.send_chat_request("hike", &omar, "").unwrap();
        f.advance(REQUEST_TTL - 1);
        assert_eq!(f.snapshot().sent.iter().find(|r| r.id == id2).unwrap().state, SentState::Waiting);
        f.advance(1);
        assert_eq!(f.snapshot().sent.iter().find(|r| r.id == id2).unwrap().state, SentState::Expired);
        f.cancel_request(&id2).unwrap();
        assert!(f.snapshot().sent.iter().all(|r| r.id != id2));
    }

    #[test]
    fn incoming_request_accept_decline_block() {
        let f = fake();
        let nina = did("nina");
        let got = f.accept_request("in-nina").unwrap();
        assert_eq!(got, nina);
        let s = f.snapshot();
        assert!(s.incoming.is_empty());
        let p = s.person(&nina).unwrap();
        assert!(p.contact && p.perms == Perms { message: true, call: false });
        assert!(p.msgs[0].note);
        assert!(p.msgs[1].text.starts_with("Hi, it\u{2019}s Nina"));
        assert_eq!(f.accept_request("in-nina"), Err(GErr::NoSuchRequest));

        let f = fake();
        f.decline_request("in-nina").unwrap();
        assert!(f.snapshot().incoming.is_empty() && f.snapshot().person(&nina).is_none());

        let f = fake();
        f.block_request("in-nina").unwrap();
        let s = f.snapshot();
        assert!(s.incoming.is_empty());
        assert!(matches!(s.person(&nina).unwrap().block, Block::ByMe { how: Known::ChatRequest, .. }));
    }

    #[test]
    fn permission_toggles() {
        let f = fake();
        let priya = did("priya");
        assert!(f.snapshot().person(&priya).unwrap().calls_chip);
        f.set_permission(&priya, Perm::Call, true).unwrap();
        let s = f.snapshot();
        let p = s.person(&priya).unwrap();
        assert!(p.can_call_me() && !p.calls_chip && !p.messages_only());
        f.set_permission(&priya, Perm::Message, false).unwrap();
        assert!(!f.snapshot().person(&priya).unwrap().can_message_me());
        assert_eq!(f.set_permission(&did("nobody"), Perm::Call, true), Err(GErr::NoSuchPerson));
        // Calling someone who hasn't allowed us never dials.
        assert_eq!(f.call(&priya), Err(GErr::CallsNotAllowed("Priya N".into())));
        assert_eq!(f.call(&did("meera")), Ok(()));
        f.ask_to_allow_calls(&priya).unwrap();
        let s = f.snapshot();
        assert_eq!(s.person(&priya).unwrap().msgs.last().unwrap().text, "Maya would like to be able to call you");
    }

    #[test]
    fn hosting_stop_and_start() {
        let f = fake();
        f.stop_hosting("family").unwrap();
        let s = f.snapshot();
        let g = s.group("family").unwrap();
        assert!(!g.this_computer_hosts() && !g.reachable());
        assert!(!g.me().unwrap().host);
        assert_eq!(f.stop_hosting("family"), Err(GErr::NotHosting));
        assert_eq!(s.hosted().count(), 1);
        // Messages now wait for a host.
        f.send_group_message("family", "hello").unwrap();
        let s = f.snapshot();
        assert!(matches!(s.group("family").unwrap().items.last(), Some(GroupItem::Msg { delivery: GroupDelivery::WaitingForHost, .. })));

        // Accepting Meera's request makes Climbing Sundays reachable and sends what waited.
        let gid = f.accept_host_request("host-climb").unwrap();
        assert_eq!(gid, "climb");
        let s = f.snapshot();
        let g = s.group("climb").unwrap();
        assert!(g.this_computer_hosts() && g.reachable());
        assert!(g.items.iter().all(|i| !matches!(i, GroupItem::Msg { delivery: GroupDelivery::WaitingForHost, .. })));
        assert!(s.host_requests.is_empty());
        assert_eq!(f.accept_host_request("host-climb"), Err(GErr::NoSuchRequest));

        f.set_keep_running(false);
        assert!(!f.snapshot().keep_running);
    }

    #[test]
    fn declining_a_host_request() {
        let f = fake();
        f.decline_host_request("host-climb").unwrap();
        let s = f.snapshot();
        assert!(s.host_requests.is_empty() && !s.group("climb").unwrap().this_computer_hosts());
    }

    #[test]
    fn removed_group_refuses_messages_and_can_be_deleted() {
        let f = fake();
        assert_eq!(f.send_group_message("book", "hi"), Err(GErr::Removed));
        f.archive_group("book").unwrap();
        assert!(f.snapshot().group("book").unwrap().archived);
        f.delete_group("book").unwrap();
        assert!(f.snapshot().group("book").is_none());
        assert_eq!(f.delete_group("book"), Err(GErr::NoSuchGroup));
    }

    #[test]
    fn address_interval() {
        let f = fake();
        f.set_interval(Interval::Week);
        let a = f.snapshot().address.unwrap();
        assert_eq!(a.interval, Interval::Week);
        assert_eq!(a.next_change, a.last_change + 7 * DAY);
    }

    #[test]
    fn empty_messages_are_refused() {
        let f = fake();
        assert_eq!(f.send_group_message("hike", "   "), Err(GErr::Empty));
        assert_eq!(f.send_message(&did("meera"), ""), Err(GErr::Empty));
    }
}
