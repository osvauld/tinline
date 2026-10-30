//! The plain records and the event sink of the group API (p2pcore declares them to UniFFI).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupRole {
    Owner,
    Admin,
    Member,
    /// Removed by 3 admin flags: read-only archive, composer disabled.
    Removed,
    /// Left the group (or never completed joining).
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupItemKind {
    Post,
    /// System lines, "about" `subject_did`.
    Joined,
    Left,
    Removed,
    AdminGranted,
    AdminRevoked,
    HostAdded,
    HostRemoved,
}

/// For our own posts: `Waiting` = only on this device ("Waiting for a host"); `Delivered` = a
/// host acknowledged holding it; `Failed` = the hosts refuse it and this device stopped sending
/// it (`GroupMessage::failure` has the words). Everything else is `Delivered`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupDelivery {
    Waiting,
    Delivered,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupMessage {
    pub id: String,
    pub group_id: String,
    pub kind: GroupItemKind,
    pub author_did: String,
    pub author_name: String,
    /// Who a system line is about (the author for posts).
    pub subject_did: String,
    pub subject_name: String,
    pub at: u64,
    pub text: String,
    pub reply_to: Option<String>,
    pub outgoing: bool,
    pub delivery: GroupDelivery,
    /// Why the post is not posted ("Not posted: only admins post here"); `Some` exactly when
    /// `delivery` is `Failed`. It follows the current failed state, so a post that later gets
    /// carried along again stops being failed.
    pub failure: Option<String>,
    /// A post by a member removed since, kept because it was written before the removal (U11).
    pub before_removal: bool,
    /// The post's picture (public group §5); `text` may then be empty.
    pub image: Option<GroupImage>,
    /// A voice post's recording; `text` and `image` are then empty.
    pub voice: Option<GroupVoice>,
    /// A file post's file (`docs/design/group-files.md`); `text` is then its caption.
    pub file: Option<GroupFile>,
}

/// A file post's file: what the card shows and where it is (`Node::group_fetch_file` loads it,
/// `Node::group_file_path` opens it, `on_group_image` reports changes).
#[derive(Debug, Clone, PartialEq)]
pub struct GroupFile {
    /// The file name as the sender gave it (one path component).
    pub name: String,
    /// Bytes.
    pub size: u64,
    pub mime: String,
    /// `Ready` here; `Loading` on its way (`done` bytes so far); `Remote` tap to download;
    /// `Failed` could not be fetched (with `kept_by_hosts` false: "not available right now,
    /// try later").
    pub state: GroupImageState,
    /// Bytes arrived so far while `Loading` (0 otherwise).
    pub done: u64,
    /// The group's hosts keep this file (it is at most `HOST_KEEP_FILE_MAX`); otherwise they
    /// relay it from whoever has it, when asked.
    pub kept_by_hosts: bool,
    /// Who has a copy now: the author first (unless they deleted it), then members who
    /// downloaded it, in that order. Empty in open groups apart from the author.
    pub holders: Vec<GroupFileHolder>,
}

/// One member holding a file.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupFileHolder {
    pub did: String,
    pub name: String,
}

/// A voice post's recording: what the bubble draws before it is here, and where it is (the same
/// states as a picture; `Node::group_voice_path` opens it, `Node::group_fetch_voice` loads it).
#[derive(Debug, Clone, PartialEq)]
pub struct GroupVoice {
    pub duration_ms: u32,
    /// About 64 peaks, 0..=255.
    pub waveform: Vec<u8>,
    /// Bytes.
    pub size: u64,
    pub state: GroupImageState,
}

/// Where a post's picture is on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupImageState {
    /// Here: `Node::group_image_path` opens it.
    Ready,
    /// On its way from a host.
    Loading,
    /// Not downloaded (the auto-download setting said wait): `Node::group_fetch_image` on tap.
    Remote,
    /// Could not be fetched (tap tries again), or a host dropped it.
    Failed,
}

/// A post's picture: its shown size (draw the box at this shape before it arrives), the 4x3 RGB
/// colour grid for the placeholder, and where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupImage {
    pub width: u32,
    pub height: u32,
    /// Bytes.
    pub size: u64,
    pub mime: String,
    /// 4x3 RGB colours, row by row (empty if the sender made none).
    pub preview: Vec<u8>,
    pub state: GroupImageState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupPage {
    /// Oldest first.
    pub messages: Vec<GroupMessage>,
    /// Cursor for the next older page (`group_messages(before = this)`); `None` at the start.
    pub older: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupMember {
    pub did: String,
    pub name: String,
    pub is_me: bool,
    pub is_owner: bool,
    pub is_admin: bool,
    /// One of their devices hosts the group.
    pub is_host: bool,
    /// Counted admin flags so far (show to admins only).
    pub flags: u32,
    pub joined_at: u64,
    pub invited_by: Option<String>,
    /// Device IDs (hex) this member wrote from: what "make host" picks from.
    pub device_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupHost {
    pub did: String,
    pub name: String,
    pub device_id: String,
    pub appointed_by: Option<String>,
    pub is_me: bool,
    /// This device has a live session with it (or it is this device).
    pub online: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupFlag {
    pub by_did: String,
    pub by_name: String,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupRemoval {
    /// When the 3rd flag was made.
    pub at: u64,
    /// The flags that removed us, each DID-signed and checked on this device.
    pub flags: Vec<GroupFlag>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupDetail {
    pub id: String,
    pub name: String,
    pub owner_did: String,
    pub my_role: GroupRole,
    /// This device hosts the group.
    pub am_host: bool,
    pub members: Vec<GroupMember>,
    pub hosts: Vec<GroupHost>,
    /// Devices that offered to host but hold no permit yet.
    pub host_offers: Vec<GroupHost>,
    /// Devices an admin appointed as host that have not accepted yet (`appointed_by` set).
    pub host_requests: Vec<GroupHost>,
    /// An admin appointed THIS device as host and it has not answered: show "host this
    /// group?" and call `group_accept_host(group, true/false)`.
    pub host_requested_here: bool,
    /// Admins see who was flagged how often in `members`; this is our own removal, if any.
    pub removal: Option<GroupRemoval>,
    /// This device hosts the group's calls (U21: a separate capability from `am_host`).
    pub am_call_host: bool,
    /// Devices that host the group's calls (`online`: this device or a live session).
    pub call_hosts: Vec<GroupHost>,
    /// Devices an admin allowed to host calls that have not accepted yet.
    pub call_host_requests: Vec<GroupHost>,
    /// An admin allowed THIS device to host the group's calls and it has not answered: show
    /// "host calls for this group?" and call `group_accept_call_host(group, true/false)`.
    pub call_host_requested_here: bool,
    /// Anyone may join with the open link (public group §2).
    pub open: bool,
    /// Only the owner and admins post (a channel); hide the composer for others.
    pub admins_only: bool,
    /// Devices offering to host calls, awaiting an admin permission.
    pub call_host_offers: Vec<GroupHost>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupSummary {
    pub id: String,
    pub name: String,
    pub my_role: GroupRole,
    pub member_count: u32,
    pub host_count: u32,
    pub hosted_here: bool,
    /// Some host has a live session with this device.
    pub host_reachable: bool,
    pub last: Option<GroupMessage>,
    pub unread: u32,
    /// Anyone may join with the open link (public group §2).
    pub open: bool,
    /// Only the owner and admins post (a channel).
    pub admins_only: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupInvitePeek {
    pub group_id: String,
    pub name: String,
    pub owner_did: String,
    /// `None` for a token for one's own other devices.
    pub issuer_did: Option<String>,
    /// Unix ms.
    pub expires: Option<u64>,
    pub host_count: u32,
    /// An open link (no invite, no expiry): joining makes us a member of a public group.
    pub open: bool,
}

/// A member online in an open group (`Node::group_online`, public group §3).
#[derive(Debug, Clone, PartialEq)]
pub struct OnlineMember {
    pub did: String,
    /// The name the member shows the group (its join record).
    pub name: String,
    pub is_admin: bool,
}

/// Where the runtime reports to; p2pcore forwards it to the app's callback (`Node::set_group_events`).
/// Called on core threads: implementations must not block.
pub trait GroupSink: Send + Sync {
    /// A message or system line appeared.
    fn on_group_message(&self, message: GroupMessage);
    /// Anything else about the group changed (members, roles, hosts, delivery, removal).
    fn on_group_changed(&self, group_id: String);
    /// The online list of an open group changed (re-read `group_online`). Best effort; may lag
    /// by about 15 s.
    fn on_group_presence(&self, group_id: String);
    /// The picture of post `message_id` changed state (arrived, failed, was asked for): re-read
    /// the message's `image` and `group_image_path`.
    fn on_group_image(&self, group_id: String, message_id: String);
}
