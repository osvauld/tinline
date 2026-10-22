//! The group API records, declared to UniFFI (the runtime's, `group_runtime::api`, are plain),
//! and the app's event callback.

use std::sync::Arc;

pub use group_runtime::api::*;

#[uniffi::remote(Enum)]
pub enum GroupRole {
    Owner,
    Admin,
    Member,
    /// Removed by 3 admin flags: read-only archive, composer disabled.
    Removed,
    /// Left the group (or never completed joining).
    Left,
}

#[uniffi::remote(Enum)]
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
#[uniffi::remote(Enum)]
pub enum GroupDelivery {
    Waiting,
    Delivered,
    Failed,
}

#[uniffi::remote(Record)]
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
    #[uniffi(default = None)]
    pub image: Option<GroupImage>,
}

/// Where a post's picture is on this device.
#[uniffi::remote(Enum)]
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
#[uniffi::remote(Record)]
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

#[uniffi::remote(Record)]
pub struct GroupPage {
    /// Oldest first.
    pub messages: Vec<GroupMessage>,
    /// Cursor for the next older page (`group_messages(before = this)`); `None` at the start.
    pub older: Option<String>,
}

#[uniffi::remote(Record)]
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

#[uniffi::remote(Record)]
pub struct GroupHost {
    pub did: String,
    pub name: String,
    pub device_id: String,
    pub appointed_by: Option<String>,
    pub is_me: bool,
    /// This device has a live session with it (or it is this device).
    pub online: bool,
}

#[uniffi::remote(Record)]
pub struct GroupFlag {
    pub by_did: String,
    pub by_name: String,
    pub at: u64,
}

#[uniffi::remote(Record)]
pub struct GroupRemoval {
    /// When the 3rd flag was made.
    pub at: u64,
    /// The flags that removed us, each DID-signed and checked on this device.
    pub flags: Vec<GroupFlag>,
}

#[uniffi::remote(Record)]
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
    #[uniffi(default = false)]
    pub open: bool,
    /// Only the owner and admins post (a channel); hide the composer for others.
    #[uniffi(default = false)]
    pub admins_only: bool,
}

#[uniffi::remote(Record)]
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
    #[uniffi(default = false)]
    pub open: bool,
    /// Only the owner and admins post (a channel).
    #[uniffi(default = false)]
    pub admins_only: bool,
}

#[uniffi::remote(Record)]
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
    #[uniffi(default = false)]
    pub open: bool,
}

/// A member online in an open group (`Node::group_online`, public group §3).
#[uniffi::remote(Record)]
pub struct OnlineMember {
    pub did: String,
    /// The name the member shows the group (its join record).
    pub name: String,
    pub is_admin: bool,
}

/// Registered with `Node::set_group_events`. Called on core threads: do not block.
#[uniffi::export(rust, foreign)]
pub trait GroupEvents: Send + Sync {
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

/// Hands the runtime's events to the app's `GroupEvents`.
pub(crate) struct GroupEventsAdapter(pub(crate) Arc<dyn GroupEvents>);

impl group_runtime::api::GroupSink for GroupEventsAdapter {
    fn on_group_message(&self, message: GroupMessage) {
        self.0.on_group_message(message);
    }

    fn on_group_changed(&self, group_id: String) {
        self.0.on_group_changed(group_id);
    }

    fn on_group_presence(&self, group_id: String) {
        self.0.on_group_presence(group_id);
    }

    fn on_group_image(&self, group_id: String, message_id: String) {
        self.0.on_group_image(group_id, message_id);
    }
}
