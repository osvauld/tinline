//! Group chat on the core's real API (G1, `crates/core/src/group/api.rs`, docs/group-core.md):
//! the Groups side tab, a group's conversation, its members pane and the group dialogs. The
//! screens are in `group_live_view.rs`.
//!
//! Every core call goes through `GroupApi`, so the flows are tested with a stand-in that follows
//! the core's rules (`tests` below). Reads (`groups`, `group_detail`, `group_messages`) are
//! in-memory in the core and run on the UI thread; everything that writes or waits on the
//! network is a `Job` the app runs on a blocking thread.
//!
//! Decisions (rotation-open-decisions.md §0): 3 admin flags remove a member (U9/U15) and the
//! removed member sees the signed flags; a post written before a removal still shows, marked
//! (U11); only hosts know member endpoints (U7), so "Waiting for a host" is the honest state of
//! our posts until a host acknowledges them. G3: admins invite contacts directly ("Add members",
//! blocked contacts never offered); invitations to us show on top of the Groups tab with Join /
//! Decline; invite links are short (`tinline://g/...`) and the core fetches the invite from a host,
//! so peeking is a job, never a UI-thread call. No "add my other device": own-device sync (G2) brings
//! groups to linked devices.

use iced::Task;
use p2pcore::{
    ChatRequestState, Error, GroupChatRequest, GroupDelivery, GroupDetail, GroupInvitation, GroupInvitationState, GroupInvitePeek, GroupItemKind, GroupMember, GroupMessage, GroupPage, GroupRole, GroupSummary, Node, OnlineMember,
};

/// Messages loaded when a group opens, and per "Show earlier".
pub(super) const PAGE: u32 = 100;
/// Flags that remove a member (U9/U15; `state::REMOVAL_FLAGS` in core).
pub(super) const FLAGS_TO_REMOVE: u32 = 3;
/// A host's presence frame lists at most this many DIDs (public group §3): at or above it the count
/// may be cut short, so the header says "500+ online".
pub(super) const ONLINE_CAP: usize = 500;

/// The core's group calls.
pub trait GroupApi {
    fn groups(&self) -> Result<Vec<GroupSummary>, Error>;
    fn group_detail(&self, id: String) -> Result<GroupDetail, Error>;
    fn group_messages(&self, id: String, before: Option<String>, limit: u32) -> Result<GroupPage, Error>;
    fn group_mark_read(&self, id: String) -> Result<(), Error>;
    /// Signed member receipts of my posts in `[from_ms, to_ms]` (the group tick and n/N). Default for test fakes.
    fn group_receipts(&self, _id: String, _from_ms: u64, _to_ms: u64) -> Result<Vec<p2pcore::GroupReceiptStatus>, Error> {
        Ok(Vec::new())
    }
    fn group_create(&self, name: String) -> Result<String, Error>;
    fn group_invite(&self, id: String) -> Result<String, Error>;
    /// Network (fetches from the link's host): only from a job.
    fn group_peek_invite(&self, token: String) -> Option<GroupInvitePeek>;
    fn group_join(&self, token: String) -> Result<String, Error>;
    fn group_post(&self, id: String, text: String, reply_to: Option<String>) -> Result<GroupMessage, Error>;
    fn group_flag(&self, id: String, did: String) -> Result<(), Error>;
    fn group_set_admin(&self, id: String, did: String, admin: bool) -> Result<(), Error>;
    fn group_leave(&self, id: String) -> Result<(), Error>;
    fn group_appoint_host(&self, id: String, did: String, device_id: String) -> Result<(), Error>;
    fn group_revoke_host(&self, id: String, did: String, device_id: String) -> Result<(), Error>;
    fn group_accept_host(&self, id: String, accept: bool) -> Result<(), Error>;
    // Call hosts (GA-I1, U21): a separate capability from chat hosting. Defaults for test fakes.
    fn group_appoint_call_host(&self, _id: String, _did: String, _device_id: String) -> Result<(), Error> {
        Ok(())
    }
    fn group_revoke_call_host(&self, _id: String, _did: String, _device_id: String) -> Result<(), Error> {
        Ok(())
    }
    fn group_accept_call_host(&self, _id: String, _accept: bool) -> Result<(), Error> {
        Ok(())
    }
    fn group_device_id(&self) -> Result<String, Error>;
    // Chat requests through the group's hosts (G2, U3).
    fn group_chat_request(&self, id: String, did: String) -> Result<GroupChatRequest, Error>;
    fn group_chat_requests_incoming(&self) -> Result<Vec<GroupChatRequest>, Error>;
    fn group_chat_requests_outgoing(&self) -> Result<Vec<GroupChatRequest>, Error>;
    fn group_chat_accept(&self, request_id: String) -> Result<GroupChatRequest, Error>;
    fn group_chat_decline(&self, request_id: String) -> Result<GroupChatRequest, Error>;
    /// The personal block (P5), for "Block" on an incoming request.
    fn block(&self, did: String) -> Result<(), Error>;
    // Direct invitations to contacts (G3).
    fn group_invite_contacts(&self, id: String, dids: Vec<String>) -> Result<Vec<GroupInvitation>, Error>;
    fn group_invitations_incoming(&self) -> Result<Vec<GroupInvitation>, Error>;
    fn group_invitations_outgoing(&self, id: String) -> Result<Vec<GroupInvitation>, Error>;
    fn group_invitation_accept(&self, id: String) -> Result<GroupInvitation, Error>;
    fn group_invitation_decline(&self, id: String) -> Result<GroupInvitation, Error>;
    // Public groups (docs/design/public-group.md §2–§3).
    /// Network (registers the code at the hosts): only from a job. Opens the group if it is closed.
    fn group_open_link(&self, id: String) -> Result<String, Error>;
    fn group_set_open(&self, id: String, open: bool) -> Result<(), Error>;
    fn group_set_admins_only(&self, id: String, admins_only: bool) -> Result<(), Error>;
    /// In memory in the core: empty for a private group.
    fn group_online(&self, id: String) -> Result<Vec<OnlineMember>, Error>;
    fn group_presence_visible(&self) -> Result<bool, Error>;
    // Image posts (public group §5). Defaults for test fakes.
    /// Blocking (the core scales and re-encodes the picture): only from a job.
    fn group_post_image(&self, _id: String, _path: String, _caption: String) -> Result<GroupMessage, Error> {
        Err(Error::NotFound)
    }
    fn group_fetch_image(&self, _id: String, _message_id: String) -> Result<(), Error> {
        Ok(())
    }
    /// Blocking the first time (the core opens the picture into a file): only from a job.
    fn group_image_path(&self, _id: String, _message_id: String) -> Option<String> {
        None
    }
    /// Voice posts: load one now (tap, or try again).
    fn group_fetch_voice(&self, _id: String, _message_id: String) -> Result<(), Error> {
        Ok(())
    }
    /// Post the recording at `path` (Ogg Opus from the app's recorder). Blocking (the core reads
    /// and seals the file): only from a job.
    fn group_post_voice(&self, _id: String, _path: String, _waveform: Vec<u8>) -> Result<GroupMessage, Error> {
        Err(Error::NotFound)
    }
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
    fn group_receipts(&self, id: String, from_ms: u64, to_ms: u64) -> Result<Vec<p2pcore::GroupReceiptStatus>, Error> {
        Node::group_receipts(self, id, from_ms, to_ms)
    }
    fn group_create(&self, name: String) -> Result<String, Error> {
        Node::group_create(self, name)
    }
    fn group_invite(&self, id: String) -> Result<String, Error> {
        Node::group_invite(self, id)
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
    fn group_post_image(&self, id: String, path: String, caption: String) -> Result<GroupMessage, Error> {
        Node::group_post_image(self, id, path, caption)
    }
    fn group_fetch_image(&self, id: String, message_id: String) -> Result<(), Error> {
        Node::group_fetch_image(self, id, message_id)
    }
    fn group_image_path(&self, id: String, message_id: String) -> Option<String> {
        Node::group_image_path(self, id, message_id)
    }
    fn group_fetch_voice(&self, id: String, message_id: String) -> Result<(), Error> {
        Node::group_fetch_voice(self, id, message_id)
    }
    fn group_post_voice(&self, id: String, path: String, waveform: Vec<u8>) -> Result<GroupMessage, Error> {
        Node::group_post_voice(self, id, path, waveform)
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
    fn group_appoint_call_host(&self, id: String, did: String, device_id: String) -> Result<(), Error> {
        Node::group_appoint_call_host(self, id, did, device_id)
    }
    fn group_revoke_call_host(&self, id: String, did: String, device_id: String) -> Result<(), Error> {
        Node::group_revoke_call_host(self, id, did, device_id)
    }
    fn group_accept_call_host(&self, id: String, accept: bool) -> Result<(), Error> {
        Node::group_accept_call_host(self, id, accept)
    }
    fn group_accept_host(&self, id: String, accept: bool) -> Result<(), Error> {
        Node::group_accept_host(self, id, accept)
    }
    fn group_device_id(&self) -> Result<String, Error> {
        Node::group_device_id(self)
    }
    fn group_chat_request(&self, id: String, did: String) -> Result<GroupChatRequest, Error> {
        Node::group_chat_request(self, id, did)
    }
    fn group_chat_requests_incoming(&self) -> Result<Vec<GroupChatRequest>, Error> {
        Node::group_chat_requests_incoming(self)
    }
    fn group_chat_requests_outgoing(&self) -> Result<Vec<GroupChatRequest>, Error> {
        Node::group_chat_requests_outgoing(self)
    }
    fn group_chat_accept(&self, request_id: String) -> Result<GroupChatRequest, Error> {
        Node::group_chat_accept(self, request_id)
    }
    fn group_chat_decline(&self, request_id: String) -> Result<GroupChatRequest, Error> {
        Node::group_chat_decline(self, request_id)
    }
    fn block(&self, did: String) -> Result<(), Error> {
        Node::block(self, did)
    }
    fn group_invite_contacts(&self, id: String, dids: Vec<String>) -> Result<Vec<GroupInvitation>, Error> {
        Node::group_invite_contacts(self, id, dids)
    }
    fn group_invitations_incoming(&self) -> Result<Vec<GroupInvitation>, Error> {
        Node::group_invitations_incoming(self)
    }
    fn group_invitations_outgoing(&self, id: String) -> Result<Vec<GroupInvitation>, Error> {
        Node::group_invitations_outgoing(self, id)
    }
    fn group_invitation_accept(&self, id: String) -> Result<GroupInvitation, Error> {
        Node::group_invitation_accept(self, id)
    }
    fn group_invitation_decline(&self, id: String) -> Result<GroupInvitation, Error> {
        Node::group_invitation_decline(self, id)
    }
    fn group_open_link(&self, id: String) -> Result<String, Error> {
        Node::group_open_link(self, id)
    }
    fn group_set_open(&self, id: String, open: bool) -> Result<(), Error> {
        Node::group_set_open(self, id, open)
    }
    fn group_set_admins_only(&self, id: String, admins_only: bool) -> Result<(), Error> {
        Node::group_set_admins_only(self, id, admins_only)
    }
    fn group_online(&self, id: String) -> Result<Vec<OnlineMember>, Error> {
        Node::group_online(self, id)
    }
    fn group_presence_visible(&self) -> Result<bool, Error> {
        Node::group_presence_visible(self)
    }
}

/// Words for a failed group call.
pub(super) fn gerr(e: Error) -> String {
    match e {
        Error::Rejected(r) => match r.as_str() {
            "removed" => crate::i18n::tr_str("group_you_were_removed_from_this_group").into(),
            "not a member" => crate::i18n::tr_str("group_youre_not_a_member").into(),
            "already a contact" => crate::i18n::tr_str("group_you_already_have_them").into(),
            "you block them" => crate::i18n::tr_str("group_you_block_them_unblock").into(),
            "unknown or expired invite code" => crate::i18n::tr_str("group_that_invite_isnt_valid").into(),
            "admins only" => crate::i18n::tr_str("group_only_admins_post_here_2").into(),
            r if r == p2pcore::BLOCKED => crate::i18n::tr_str("group_they_blocked_you_so").into(),
            r => {
                let mut c = r.chars();
                match c.next() {
                    Some(f) => format!("{}{}.", f.to_uppercase(), c.as_str().trim_end_matches('.')),
                    None => crate::i18n::tr_str("group_refused").into(),
                }
            }
        },
        Error::Timeout => crate::i18n::tr_str("group_no_host_answered_in").into(),
        Error::Locked => crate::i18n::tr_str("common_unlock_first").into(),
        Error::NotFound => crate::i18n::tr_str("group_that_group_isnt_on").into(),
        e => e.to_string(),
    }
}

/// One row of the "Add members" picker: who, and why they can't be picked (`None`: pickable).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PickRow {
    pub did: String,
    pub name: String,
    pub why: Option<&'static str>,
    pub invited: Option<GroupInvitation>,
}

/// The picker's rows: contacts we block or that block us (P5) are not offered at all; members show
/// as already in; someone with a pending invitation can't be picked again. Pickable first, by name.
pub(super) fn pick_rows(contacts: &[(String, String)], d: &GroupDetail, sent: &[GroupInvitation], blocked: impl Fn(&str) -> bool) -> Vec<PickRow> {
    let mut rows: Vec<PickRow> = contacts
        .iter()
        .filter(|(did, _)| !blocked(did))
        .map(|(did, name)| {
            let invited = sent.iter().filter(|i| &i.invitee_did == did).max_by_key(|i| i.at).cloned();
            let why = if d.members.iter().any(|m| &m.did == did) {
                Some(crate::i18n::tr_str("group_already_a_member"))
            } else if invited.as_ref().is_some_and(|i| i.state == GroupInvitationState::Pending) {
                Some(crate::i18n::tr_str("group_invited_waiting"))
            } else {
                None
            };
            PickRow { did: did.clone(), name: name.clone(), why, invited }
        })
        .collect();
    rows.sort_by_key(|r| (r.why.is_some(), r.name.to_lowercase()));
    rows
}

/// An invitation we sent, in words, and whether it is bad news.
pub(super) fn sent_invitation_line(i: &GroupInvitation) -> (&'static str, bool) {
    match i.state {
        GroupInvitationState::Pending if i.delivered => (crate::i18n::tr_str("group_invited_waiting_for_an"), false),
        GroupInvitationState::Pending => (crate::i18n::tr_str("group_invited_not_delivered_yet"), false),
        GroupInvitationState::Joined => (crate::i18n::tr_str("group_joined"), false),
        GroupInvitationState::Declined => (crate::i18n::tr_str("common_declined"), true),
        GroupInvitationState::Expired => (crate::i18n::tr_str("group_invitation_expired"), true),
        GroupInvitationState::Blocked => (crate::i18n::tr_str("group_blocked_you"), true),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum LDialog {
    NewGroup { name: String },
    /// Paste a link or code; `checking` while the host is asked, then `peek` if it is valid.
    Join { token: String, peek: Option<GroupInvitePeek>, checking: bool },
    /// The invite link; `text` is None while the core makes it.
    Token { text: Option<String> },
    /// Pick contacts to invite (G3).
    AddMembers { picked: Vec<String> },
    Flag { did: String },
    Admin { did: String, admin: bool },
    Leave,
    /// Why we were removed: the signed flags.
    Removal,
    /// Ask a member (not a contact) for a private chat (U3).
    Request { did: String },
    /// This device starts (true) or stops hosting the open group.
    Host { accept: bool },
    Hosting { did: String, dev: String, local: bool, messages: bool, calls: bool },
    /// Who is online in the open (public) group.
    Online,
    /// Open the group to anyone with the link (confirm first: receipts go off, an online list shows).
    OpenGroup,
}

/// A group setting whose change is on its way to the core (its switch shows the new value meanwhile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Setting {
    Open,
    AdminsOnly,
    /// The open link is being fetched for "Copy link".
    Link,
}

/// Work that writes or waits on the network: run off the UI thread, its result comes back as an `Lm`.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Job {
    Hosting { gid: String, did: String, dev: String, local: bool, admin: bool, messages: bool, calls: bool },
    Create(String),
    Join(String),
    Invite(String),
    /// Ask the link's host what it is for (network).
    Peek(String),
    InviteContacts { gid: String, dids: Vec<String> },
    AcceptInv(String),
    DeclineInv(String),
    Post { gid: String, text: String, reply: Option<String> },
    /// Post the picture at `path` with the composer's text as its caption.
    PostImage { gid: String, path: String, caption: String },
    /// Load a post's picture now (tap to load, or try again).
    FetchImage { gid: String, id: String },
    /// Load a voice post's recording now (tap to load, or try again).
    FetchVoice { gid: String, id: String },
    /// Post the finished recording at `path` (the app's one recorder), then delete the file: the
    /// core keeps its own sealed copy.
    PostVoice { gid: String, path: String, waveform: Vec<u8> },
    /// Open the pictures of these posts into files (the first time decrypts them).
    ImagePaths { gid: String, ids: Vec<String> },
    Flag { gid: String, did: String },
    SetAdmin { gid: String, did: String, admin: bool },
    Leave(String),
    Request { gid: String, did: String },
    AcceptReq(String),
    DeclineReq(String),
    /// Block the sender (P5), then decline.
    BlockReq { id: String, did: String },
    /// The open link (opens the group if closed); `copy`: put it on the clipboard when it comes.
    OpenLink { gid: String, copy: bool },
    /// Close the open link (opening goes through `OpenLink`).
    CloseLink(String),
    AdminsOnly { gid: String, on: bool },
}

impl Job {
    pub fn run(self, api: &dyn GroupApi) -> Lm {
        let did = |r: Result<(), Error>, ok: String| Lm::Did(r.map(|_| ok).map_err(gerr));
        match self {
            Job::Hosting { gid, did: who, dev, local, admin, messages, calls } => {
                // Reload before writing: only change permissions that differ, preserving existing consent.
                let r = (|| {
                    let d = api.group_detail(gid.clone())?;
                    let has = |hs: &[p2pcore::GroupHost]| hs.iter().any(|h| h.did == who && h.device_id == dev);
                    if admin {
                        let granted_messages = has(&d.hosts) || has(&d.host_requests);
                        let granted_calls = has(&d.call_hosts) || has(&d.call_host_requests);
                        if messages != granted_messages {
                            if messages { api.group_appoint_host(gid.clone(), who.clone(), dev.clone())?; }
                            else { api.group_revoke_host(gid.clone(), who.clone(), dev.clone())?; }
                        }
                        if calls != granted_calls {
                            if calls { api.group_appoint_call_host(gid.clone(), who.clone(), dev.clone())?; }
                            else { api.group_revoke_call_host(gid.clone(), who.clone(), dev.clone())?; }
                        }
                    }
                    if local {
                        // Accept only the selection shown in this dialog. Unpermitted selections are offers.
                        let accepted_messages = d.am_host || has(&d.host_offers);
                        let accepted_calls = d.am_call_host || has(&d.call_host_offers);
                        if messages != accepted_messages || (d.host_requested_here && !messages) {
                            api.group_accept_host(gid.clone(), messages)?;
                        }
                        if calls != accepted_calls || (d.call_host_requested_here && !calls) {
                            api.group_accept_call_host(gid.clone(), calls)?;
                        }
                    }
                    Ok(())
                })();
                did(r, crate::i18n::tr("group_hosting_saved"))
            }
            Job::Create(name) => Lm::Created(api.group_create(name).map_err(gerr)),
            Job::Join(token) => Lm::Joined(api.group_join(token).map_err(gerr)),
            Job::Invite(gid) => Lm::Token(api.group_invite(gid).map_err(gerr)),
            Job::Peek(token) => Lm::Peeked(token.clone(), api.group_peek_invite(token)),
            Job::InviteContacts { gid, dids } => Lm::Invited(api.group_invite_contacts(gid, dids).map_err(gerr)),
            Job::AcceptInv(id) => Lm::InvJoined(api.group_invitation_accept(id).map_err(gerr)),
            Job::DeclineInv(id) => Lm::InvDeclined(api.group_invitation_decline(id).map_err(gerr)),
            Job::Post { gid, text, reply } => {
                let r = api.group_post(gid, text.clone(), reply).map_err(gerr);
                Lm::Posted(r, text)
            }
            Job::PostImage { gid, path, caption } => {
                let r = api.group_post_image(gid, path, caption.clone()).map_err(gerr);
                Lm::Posted(r, caption)
            }
            Job::FetchImage { gid, id } => Lm::ImageAsked(api.group_fetch_image(gid, id).map_err(gerr)),
            Job::FetchVoice { gid, id } => Lm::ImageAsked(api.group_fetch_voice(gid, id).map_err(gerr)),
            Job::PostVoice { gid, path, waveform } => {
                let r = api.group_post_voice(gid, path.clone(), waveform).map_err(gerr);
                let _ = std::fs::remove_file(path);
                Lm::Posted(r, String::new())
            }
            Job::ImagePaths { gid, ids } => {
                let paths = ids.into_iter().map(|id| (id.clone(), api.group_image_path(gid.clone(), id))).collect();
                Lm::ImagePaths(paths)
            }
            Job::Flag { gid, did: who } => did(api.group_flag(gid, who), crate::i18n::tr_str("group_flag_recorded_it_is").into()),
            Job::SetAdmin { gid, did: who, admin } => did(
                api.group_set_admin(gid, who, admin),
                if admin { crate::i18n::tr_str("group_made_admin").into() } else { crate::i18n::tr_str("group_no_longer_an_admin").into() },
            ),
            Job::Leave(gid) => did(api.group_leave(gid), crate::i18n::tr_str("group_you_left_the_group").into()),
            Job::Request { gid, did: who } => Lm::Requested(api.group_chat_request(gid, who).map_err(gerr)),
            Job::AcceptReq(id) => Lm::Answered(api.group_chat_accept(id).map_err(gerr)),
            Job::DeclineReq(id) => Lm::Answered(api.group_chat_decline(id).map_err(gerr)),
            Job::BlockReq { id, did: who } => {
                if let Err(e) = api.block(who) {
                    return Lm::Answered(Err(gerr(e)));
                }
                Lm::Answered(api.group_chat_decline(id).map_err(gerr))
            }
            Job::OpenLink { gid, copy } => Lm::OpenLinked(gid.clone(), copy, api.group_open_link(gid).map_err(gerr)),
            Job::CloseLink(gid) => did(
                api.group_set_open(gid, false),
                crate::i18n::tr_str("group_the_open_link_is").into(),
            ),
            Job::AdminsOnly { gid, on } => did(
                api.group_set_admins_only(gid, on),
                if on { crate::i18n::tr_str("group_only_admins_post_now").into() } else { crate::i18n::tr_str("group_everyone_can_post_again").into() },
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
    /// Open the group with its info page showing (the Calls tab avatar).
    OpenInfo(String),
    /// The member whose menu is open.
    Menu(Option<String>),
    Input(String),
    Send,
    Reply(Option<String>),
    Older,
    NewGroup,
    /// Open or close the "Left or removed" fold.
    GoneFold,
    Hover(Option<String>),
    NameIn(String),
    JoinOpen,
    TokenIn(String),
    Peeked(String, Option<GroupInvitePeek>),
    Invite,
    CopyToken,
    /// Show the invite QR full size (or close it).
    QrFull(bool),
    /// G3: the contact picker, a toggle in it, and the outcomes.
    AddMembers,
    Pick(String),
    Invited(Result<Vec<GroupInvitation>, String>),
    AcceptInv(String),
    DeclineInv(String),
    InvJoined(Result<GroupInvitation, String>),
    InvDeclined(Result<GroupInvitation, String>),
    /// An invitation appeared or changed (GroupInvitationEvents).
    InvitationChanged(GroupInvitation),
    AskFlag(String),
    AskAdmin(String, bool),
    AskLeave,
    AskHost(bool),
    Hosting(String, String),
    HostingMessages(bool),
    HostingCalls(bool),
    /// Answer an admin's request that this device host the open group.
    AnswerHost(bool),
    /// Settings › Hosting: open that group and ask to stop hosting it here.
    StopHostingOf(String),
    Removal,
    /// The call button in a group with no call host (true), or its explanation closed (false).
    CallNeedsHost(bool),
    Confirm,
    Dismiss,
    Created(Result<String, String>),
    Joined(Result<String, String>),
    Token(Result<String, String>),
    /// The post, and the text to put back on failure.
    Posted(Result<GroupMessage, String>, String),
    /// A plain action: what to tell the user.
    Did(Result<String, String>),
    /// Chat requests (U3).
    AskRequest(String),
    AcceptReq(String),
    DeclineReq(String),
    BlockReq(String, String),
    /// A request appeared or changed (GroupRequestEvents).
    RequestChanged(GroupChatRequest),
    Requested(Result<GroupChatRequest, String>),
    Answered(Result<GroupChatRequest, String>),
    /// The online list of this group changed (GroupEvents::on_group_presence).
    Presence(String),
    /// The header's "N online": the list.
    ShowOnline,
    /// Group info (admins): the open link on or off, admins only on or off, copy the open link.
    SetOpen(bool),
    SetAdminsOnly(bool),
    CopyOpenLink,
    /// The open link came back: (group, copy it, the link).
    OpenLinked(String, bool, Result<String, String>),
    /// Image posts (public group §5): the composer's picture button (the app opens the file
    /// picker), the picked file, a tap on a picture that is not here, the core's
    /// `on_group_image(group, post)`, opened files `(post, path)` and a fetch request's outcome.
    Attach,
    Picked(Option<std::path::PathBuf>),
    LoadImage(String),
    Image(String, String),
    ImagePaths(Vec<(String, Option<String>)>),
    ImageAsked(Result<(), String>),
    /// Voice posts: load one that is not here; play/pause (the app's one voice player, shared
    /// with 1:1 chats), seek to a fraction, and the opened file to play.
    LoadVoice(String),
    VoiceToggle(String),
    VoiceSeek(String, f32),
    VoiceReady(String, Result<std::path::PathBuf, String>),
}

/// What a step asks of the app around it.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Out {
    pub job: Option<Job>,
    pub notice: Option<String>,
    /// A group opened: close whatever else the main pane showed.
    pub opened: bool,
    pub copy: Option<String>,
    /// Open the 1:1 chat with this DID (an accepted request).
    pub chat: Option<String>,
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
    /// Signed member receipts of my loaded posts, by message id (the group tick and n/N).
    pub receipts: std::collections::HashMap<String, p2pcore::GroupReceiptStatus>,
    /// The tick each of my loaded posts showed last, and the one-shot transitions playing (as 1:1's
    /// `ChatState::{tick_vis, tick_anims}`; the rule is core's `tick_transition`).
    pub tick_vis: std::collections::HashMap<String, crate::ticks::Vis>,
    pub tick_anims: std::collections::HashMap<String, crate::ticks::Anim>,
    pub input: String,
    pub reply: Option<String>,
    /// The members pane beside the chat (on by default).
    pub hide_info: bool,
    /// The call button was pressed in a group with no call host: a calm explanation shows under the header.
    pub call_needs_host: bool,
    /// GroupsList "Left or removed" fold (closed by default).
    pub gone_open: bool,
    /// Unread posts when the open group was opened (the "n new messages" divider).
    pub unread_at_open: u32,
    /// The post the pointer is over (its reply button shows).
    pub hover: Option<String>,
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
    /// Chat requests (U3), newest first.
    pub incoming: Vec<GroupChatRequest>,
    pub outgoing: Vec<GroupChatRequest>,
    /// Invitations to us (G3), newest first; and the ones we sent for the open group.
    pub invitations: Vec<GroupInvitation>,
    pub sent: Vec<GroupInvitation>,
    /// The invitation being joined (its Join button says "Joining...").
    pub joining: Option<String>,
    /// The invite QR fills the window.
    pub qr_full: bool,
    /// Who is online in the open group (public groups only; empty otherwise).
    pub online: Vec<OnlineMember>,
    /// The open link as the core last gave it: (group, link).
    pub open_link: Option<(String, String)>,
    /// A group setting on its way to the core, and the value asked for.
    pub pending: Option<(Setting, bool)>,
    /// This account shows in online lists (read when the list opens).
    pub presence_visible: bool,
    /// Opened pictures of posts, by post id (`group_image_path`).
    pub image_paths: std::collections::HashMap<String, String>,
    /// Posts whose picture file was asked for (once per `on_group_image`).
    pub image_asked: std::collections::HashSet<String>,
}

/// Member menu entries, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Action {
    /// Message privately: their chat if a contact, else a chat request.
    Message,
    Hosting(String),
    MakeAdmin,
    RevokeAdmin,
    Flag,
}

pub(super) fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// "a1b2c3d4": enough of a device ID to tell devices apart.
pub(super) fn short_dev(dev: &str) -> &str {
    &dev[..dev.len().min(8)]
}

impl GroupsLive {
    pub fn reset(&mut self, me: String, my_device: String) {
        *self = GroupsLive { me, my_device, ..Default::default() };
    }

    pub fn role(&self) -> Option<GroupRole> {
        self.detail.as_ref().map(|d| d.my_role)
    }

    #[cfg(test)]
    pub fn summary(&self, gid: &str) -> Option<&GroupSummary> {
        self.list.iter().find(|s| s.id == gid)
    }

    /// Invitations to us still to answer.
    pub fn pending_invitations(&self) -> impl Iterator<Item = &GroupInvitation> {
        self.invitations.iter().filter(|i| !i.outgoing && i.state == GroupInvitationState::Pending)
    }

    /// Incoming requests waiting for an answer (counted on the Chats tab, where they show).
    pub fn pending_in(&self) -> impl Iterator<Item = &GroupChatRequest> {
        self.incoming.iter().filter(|r| r.state == ChatRequestState::Pending)
    }

    /// We can post in the open group.
    pub fn can_post(&self) -> bool {
        self.role().is_some_and(active)
    }

    pub fn is_admin(&self) -> bool {
        matches!(self.role(), Some(GroupRole::Owner | GroupRole::Admin))
    }

    /// An admins-only group (a channel) and we are a member who may not post: no composer, a calm line.
    pub fn admins_only_locked(&self) -> bool {
        self.detail.as_ref().is_some_and(|d| d.admins_only && active(d.my_role)) && !self.is_admin()
    }

    /// The open group's "N online" (None for a private group, or one we are no longer in).
    pub fn online_label(&self) -> Option<String> {
        self.detail.as_ref().filter(|d| d.open && active(d.my_role)).map(|_| online_label(self.online.len()))
    }

    /// A setting's switch: the value on its way to the core, else the group's.
    pub fn setting_on(&self, s: Setting) -> bool {
        match self.pending {
            Some((p, v)) if p == s => v,
            _ => self.detail.as_ref().is_some_and(|d| match s {
                Setting::Open => d.open,
                Setting::AdminsOnly => d.admins_only,
                Setting::Link => false,
            }),
        }
    }

    /// The open link of the open group, if the core gave it to us.
    pub fn open_link_of_open(&self) -> Option<&str> {
        let (g, link) = self.open_link.as_ref()?;
        (self.open.as_deref() == Some(g.as_str())).then_some(link.as_str())
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
        out.push(Action::Message);
        if d.my_role == GroupRole::Owner && !m.is_owner {
            out.push(if m.is_admin { Action::RevokeAdmin } else { Action::MakeAdmin });
        }
        if self.is_admin() {
            for dev in &m.device_ids {
                out.push(Action::Hosting(dev.clone()));
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
            GroupRole::Removed => return crate::i18n::tr("group_you_were_removed_from_this"),
            GroupRole::Left => return crate::i18n::tr("group_you_left_this_group"),
            _ => {}
        }
        let n = d.members.len();
        let members = crate::i18n::tr_count("group_members_count", n as u64, &[]);
        if d.am_host {
            return crate::i18n::tr_dots(&[&members, crate::i18n::tr_str("group_status_this_computer_hosts")]);
        }
        if d.hosts.is_empty() {
            return crate::i18n::tr_dots(&[&members, crate::i18n::tr_str("group_status_no_host_yet")]);
        }
        if !d.hosts.iter().any(|h| h.online) {
            return crate::i18n::tr_dots(&[&members, crate::i18n::tr_str("group_status_waiting_for_a_host")]);
        }
        let h = d.hosts.len();
        crate::i18n::tr_args("group_members_and_hosts", &[("members", &members), ("hosts", &crate::i18n::tr_count("group_hosts_count", h as u64, &[]))])
    }

    /// An admin asked this device to host the open group: who asked (empty when no name is known).
    pub fn host_asked(&self) -> Option<String> {
        let d = self.detail.as_ref()?;
        if !(d.host_requested_here || d.call_host_requested_here) || !active(d.my_role) {
            return None;
        }
        let by = d.host_requests.iter().chain(d.call_host_requests.iter()).find(|h| h.is_me).and_then(|h| h.appointed_by.clone());
        Some(by.map(|did| self.member(&did).map(|m| m.name.clone()).unwrap_or_default()).filter(|n| !n.trim().is_empty()).unwrap_or_default())
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
                    self.sync_ticks();
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
                self.unread_at_open = self.list.iter().find(|s| s.id == gid).map_or(0, |s| s.unread);
                self.hover = None;
                self.open = Some(gid.clone());
                self.menu = None;
                self.input.clear();
                self.reply = None;
                self.msgs.clear();
                self.older = None;
                // A newly opened group shows its ticks as they are (no transition on first render).
                self.tick_vis.clear();
                self.tick_anims.clear();
                self.reload_open(api);
                let _ = api.group_mark_read(gid);
                self.reload_list(api);
                out.opened = true;
            }
            Lm::Close => self.close(),
            Lm::ToggleInfo => self.hide_info = !self.hide_info,
            Lm::OpenInfo(gid) => {
                out = self.step(api, Lm::Open(gid));
                self.hide_info = false;
            }
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
                    out.notice = Some(crate::i18n::tr("group_notice_cant_send"));
                    return out;
                }
                if self.admins_only_locked() {
                    out.notice = Some(crate::i18n::tr("group_only_admins_post_here_2"));
                    return out;
                }
                self.input.clear();
                self.sending = true;
                out.job = Some(Job::Post { gid, text, reply: self.reply.take() });
            }
            Lm::Attach => {}
            Lm::Picked(path) => {
                let (Some(path), Some(gid)) = (path, self.open.clone()) else { return out };
                if self.sending {
                    return out;
                }
                if !self.can_post() {
                    out.notice = Some(crate::i18n::tr("group_notice_cant_send"));
                    return out;
                }
                if self.admins_only_locked() {
                    out.notice = Some(crate::i18n::tr("group_only_admins_post_here_2"));
                    return out;
                }
                // The composer's text goes with the picture as its caption.
                let caption = self.input.trim().to_string();
                self.input.clear();
                self.reply = None;
                self.sending = true;
                out.job = Some(Job::PostImage { gid, path: path.to_string_lossy().into_owned(), caption });
            }
            Lm::LoadImage(id) => {
                let Some(gid) = self.open.clone() else { return out };
                out.job = Some(Job::FetchImage { gid, id });
            }
            Lm::LoadVoice(id) => {
                let Some(gid) = self.open.clone() else { return out };
                out.job = Some(Job::FetchVoice { gid, id });
            }
            // Played by the app (`App::update_live`), which holds the player.
            Lm::VoiceToggle(_) | Lm::VoiceSeek(..) | Lm::VoiceReady(..) => {}
            Lm::Image(gid, id) => {
                self.image_asked.remove(&id);
                if self.open.as_deref() == Some(gid.as_str()) {
                    self.reload_open(api);
                }
            }
            Lm::ImagePaths(paths) => {
                for (id, p) in paths {
                    if let Some(p) = p {
                        self.image_paths.insert(id, p);
                    }
                }
            }
            Lm::ImageAsked(r) => {
                if let Err(e) = r {
                    out.notice = Some(e);
                }
            }
            Lm::Posted(r, text) => {
                self.sending = false;
                match r {
                    Ok(msg) => {
                        if self.open.as_deref() == Some(msg.group_id.as_str()) {
                            upsert(&mut self.msgs, msg);
                            self.sync_ticks();
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
                        // Scrolled-in posts get their ticks without a transition.
                        self.reload_receipts(api, gid_for_receipts(&self.open));
                    }
                    Err(e) => out.notice = Some(gerr(e)),
                }
            }
            Lm::NewGroup => self.show(LDialog::NewGroup { name: String::new() }),
            Lm::GoneFold => self.gone_open = !self.gone_open,
            Lm::Hover(h) => self.hover = h,
            Lm::NameIn(v) => {
                if let Some(LDialog::NewGroup { name }) = &mut self.dialog {
                    *name = v.chars().take(100).collect();
                }
            }
            Lm::JoinOpen => self.show(LDialog::Join { token: String::new(), peek: None, checking: false }),
            Lm::TokenIn(v) => {
                if let Some(LDialog::Join { token, peek, checking }) = &mut self.dialog {
                    let v = v.trim().to_string();
                    if v == *token {
                        return out;
                    }
                    *token = v;
                    *peek = None;
                    *checking = !token.is_empty();
                    self.error = None;
                    if *checking {
                        out.job = Some(Job::Peek(token.clone()));
                    }
                }
            }
            Lm::Peeked(tok, p) => {
                // Only the answer for what is in the field now counts.
                if let Some(LDialog::Join { token, peek, checking }) = &mut self.dialog
                    && *token == tok
                {
                    *peek = p;
                    *checking = false;
                }
            }
            Lm::Invite => {
                let Some(gid) = self.open.clone() else { return out };
                self.show(LDialog::Token { text: None });
                self.qr_full = false;
                self.busy = true;
                out.job = Some(Job::Invite(gid));
            }
            Lm::QrFull(on) => self.qr_full = on && matches!(self.dialog, Some(LDialog::Token { text: Some(_) })),
            Lm::AddMembers => {
                if self.open.is_some() {
                    self.reload_sent(api);
                    self.show(LDialog::AddMembers { picked: Vec::new() });
                }
            }
            Lm::Pick(did) => {
                if let Some(LDialog::AddMembers { picked }) = &mut self.dialog {
                    if let Some(i) = picked.iter().position(|d| *d == did) {
                        picked.remove(i);
                    } else {
                        picked.push(did);
                    }
                    self.error = None;
                }
            }
            Lm::Invited(r) => {
                self.busy = false;
                match r {
                    Ok(v) => {
                        self.dialog = None;
                        out.notice = Some(match v.as_slice() {
                            [one] if !one.invitee_name.trim().is_empty() => crate::i18n::tr_args("group_notice_invitation_sent", &[("name", first_name(&one.invitee_name))]),
                            [_] => crate::i18n::tr("group_notice_invitation_sent_unnamed"),
                            v => crate::i18n::tr_count("group_notice_invitations_sent", v.len() as u64, &[]),
                        });
                    }
                    Err(e) => self.error = Some(e),
                }
                self.reload_sent(api);
            }
            Lm::AcceptInv(id) => {
                if self.joining.is_none() {
                    self.joining = Some(id.clone());
                    out.job = Some(Job::AcceptInv(id));
                }
            }
            Lm::DeclineInv(id) => out.job = Some(Job::DeclineInv(id)),
            Lm::InvJoined(r) => {
                self.joining = None;
                match r {
                    Ok(i) => {
                        out = self.step(api, Lm::Open(i.group_id.clone()));
                        out.notice = Some(crate::i18n::tr_args("group_notice_you_joined", &[("group", &i.group_name)]));
                    }
                    Err(e) => out.notice = Some(e),
                }
                self.reload_invitations(api);
            }
            Lm::InvDeclined(r) => {
                match r {
                    Ok(i) => out.notice = Some(if i.inviter_name.trim().is_empty() { crate::i18n::tr("group_notice_declined_inv_unnamed") } else { crate::i18n::tr_args("group_notice_declined_inv", &[("name", first_name(&i.inviter_name))]) }),
                    Err(e) => out.notice = Some(e),
                }
                self.reload_invitations(api);
            }
            Lm::InvitationChanged(i) => {
                if i.outgoing {
                    self.reload_sent(api);
                } else {
                    self.reload_invitations(api);
                }
            }
            Lm::Token(r) => {
                self.busy = false;
                if let Some(LDialog::Token { text }) = &mut self.dialog {
                    match r {
                        Ok(t) => *text = Some(t),
                        Err(e) => self.error = Some(e),
                    }
                }
            }
            Lm::CopyToken => {
                if let Some(LDialog::Token { text: Some(t) }) = &self.dialog {
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
            Lm::Hosting(who, dev) => {
                self.menu = None;
                if self.my_device.is_empty() {
                    if let Ok(device) = api.group_device_id() { self.my_device = device; }
                }
                let dev = if who == self.me && dev.is_empty() { self.my_device.clone() } else { dev };
                if let Some(d) = &self.detail {
                    let local = who == self.me && dev == self.my_device;
                    let has = |hs: &[p2pcore::GroupHost]| hs.iter().any(|h| h.did == who && h.device_id == dev);
                    let messages = has(&d.hosts) || has(&d.host_requests) || has(&d.host_offers);
                    let calls = has(&d.call_hosts) || has(&d.call_host_requests) || has(&d.call_host_offers);
                    self.show(LDialog::Hosting { did: who, dev, local, messages, calls });
                }
            }
            Lm::HostingMessages(on) => {
                if !self.busy && let Some(LDialog::Hosting { messages, .. }) = &mut self.dialog { *messages = on; }
            }
            Lm::HostingCalls(on) => {
                if !self.busy && let Some(LDialog::Hosting { calls, .. }) = &mut self.dialog { *calls = on; }
            }
            Lm::AskHost(true) => {
                return self.step(api, Lm::Hosting(self.me.clone(), self.my_device.clone()));
            }
            Lm::AskHost(false) => self.show(LDialog::Host { accept: false }),
            Lm::AnswerHost(true) => return self.step(api, Lm::Hosting(self.me.clone(), self.my_device.clone())),
            Lm::AnswerHost(false) => {
                if let (Some(gid), Some(d)) = (self.open.clone(), self.detail.as_ref()) {
                    out.job = Some(Job::Hosting { gid, did: self.me.clone(), dev: self.my_device.clone(),
                        local: true, admin: false, messages: d.am_host, calls: d.am_call_host });
                }
            }
            Lm::StopHostingOf(gid) => {
                out = self.step(api, Lm::Open(gid));
                self.show(LDialog::Host { accept: false });
            }
            Lm::Removal => self.show(LDialog::Removal),
            Lm::CallNeedsHost(on) => self.call_needs_host = on,
            Lm::Dismiss if self.qr_full => self.qr_full = false,
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
                        out.notice = Some(crate::i18n::tr("group_notice_group_made"));
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
                        out.notice = Some(crate::i18n::tr("group_notice_you_joined_the_group"));
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            Lm::AskRequest(did) => {
                self.menu = None;
                self.show(LDialog::Request { did });
            }
            Lm::AcceptReq(id) => out.job = Some(Job::AcceptReq(id)),
            Lm::DeclineReq(id) => out.job = Some(Job::DeclineReq(id)),
            Lm::BlockReq(id, did) => out.job = Some(Job::BlockReq { id, did }),
            Lm::RequestChanged(r) => {
                self.reload_requests(api);
                if r.outgoing && r.state == ChatRequestState::Accepted && r.contact_added {
                    out.notice = Some(if r.peer_name.trim().is_empty() { crate::i18n::tr("group_notice_accepted_yours_unnamed") } else { crate::i18n::tr_args("group_notice_accepted_yours", &[("name", first_name(&r.peer_name))]) });
                }
            }
            Lm::Requested(r) => {
                self.busy = false;
                match r {
                    Ok(req) => {
                        self.dialog = None;
                        out.notice = Some(if req.peer_name.trim().is_empty() { crate::i18n::tr("group_notice_request_sent_unnamed") } else { crate::i18n::tr_args("group_notice_request_sent", &[("name", first_name(&req.peer_name))]) });
                    }
                    Err(e) => self.error = Some(e),
                }
                self.reload_requests(api);
            }
            Lm::Answered(r) => {
                match r {
                    Ok(req) => match req.state {
                        ChatRequestState::Accepted => {
                            out.notice = Some(if req.peer_name.trim().is_empty() { crate::i18n::tr("group_notice_accepted_unnamed") } else { crate::i18n::tr_args("group_notice_accepted", &[("name", first_name(&req.peer_name))]) });
                            out.chat = Some(req.peer_did);
                        }
                        _ => out.notice = Some(if req.peer_name.trim().is_empty() { crate::i18n::tr("group_notice_declined_req_unnamed") } else { crate::i18n::tr_args("group_notice_declined_req", &[("name", first_name(&req.peer_name))]) }),
                    },
                    Err(e) => out.notice = Some(e),
                }
                self.reload_requests(api);
            }
            Lm::Did(r) => {
                self.busy = false;
                self.pending = None;
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
            Lm::Presence(gid) => {
                if self.open.as_deref() == Some(gid.as_str()) {
                    self.reload_online(api);
                }
            }
            Lm::ShowOnline => {
                if self.online_label().is_some() {
                    self.reload_online(api);
                    self.presence_visible = api.group_presence_visible().unwrap_or(true);
                    self.show(LDialog::Online);
                }
            }
            Lm::SetOpen(on) => {
                let Some(gid) = self.open.clone() else { return out };
                if self.pending.is_some() || !self.is_admin() {
                    return out;
                }
                if on {
                    self.show(LDialog::OpenGroup);
                } else {
                    self.pending = Some((Setting::Open, false));
                    out.job = Some(Job::CloseLink(gid));
                }
            }
            Lm::SetAdminsOnly(on) => {
                let Some(gid) = self.open.clone() else { return out };
                if self.pending.is_some() || !self.is_admin() {
                    return out;
                }
                self.pending = Some((Setting::AdminsOnly, on));
                out.job = Some(Job::AdminsOnly { gid, on });
            }
            Lm::CopyOpenLink => {
                let Some(gid) = self.open.clone() else { return out };
                if let Some(link) = self.open_link_of_open() {
                    out.copy = Some(link.to_string());
                    out.notice = Some(crate::i18n::tr("group_notice_link_copied"));
                } else if self.pending.is_none() && self.setting_on(Setting::Open) {
                    self.pending = Some((Setting::Link, true));
                    out.job = Some(Job::OpenLink { gid, copy: true });
                }
            }
            Lm::OpenLinked(gid, copy, r) => {
                self.busy = false;
                self.pending = None;
                match r {
                    Ok(link) => {
                        if matches!(self.dialog, Some(LDialog::OpenGroup)) {
                            self.dialog = None;
                            self.error = None;
                        }
                        self.open_link = Some((gid, link.clone()));
                        if copy {
                            out.copy = Some(link);
                            out.notice = Some(crate::i18n::tr("group_notice_link_copied"));
                        } else {
                            out.notice = Some(crate::i18n::tr("group_notice_group_is_open"));
                        }
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
                    self.error = Some(crate::i18n::tr("group_give_the_group_a_name"));
                    return None;
                }
                Job::Create(name)
            }
            LDialog::Join { token, peek, .. } => {
                if peek.is_none() {
                    self.error = Some(crate::i18n::tr("group_that_link_didnt_work"));
                    return None;
                }
                Job::Join(token)
            }
            LDialog::AddMembers { picked } => {
                if picked.is_empty() {
                    self.error = Some(crate::i18n::tr("group_pick_someone_first"));
                    return None;
                }
                Job::InviteContacts { gid, dids: picked }
            }
            LDialog::Flag { did } => Job::Flag { gid, did },
            LDialog::Admin { did, admin } => Job::SetAdmin { gid, did, admin },
            LDialog::Leave => Job::Leave(gid),
            LDialog::Hosting { did, dev, local, messages, calls } => Job::Hosting {
                gid, did, dev, local, admin: self.is_admin(), messages, calls,
            },
            LDialog::Host { accept } => Job::Hosting {
                gid, did: self.me.clone(), dev: self.my_device.clone(), local: true,
                admin: false, messages: accept, calls: false,
            },
            LDialog::Request { did } => Job::Request { gid, did },
            LDialog::OpenGroup => {
                self.pending = Some((Setting::Open, true));
                Job::OpenLink { gid, copy: false }
            }
            LDialog::Token { .. } | LDialog::Removal | LDialog::Online => {
                self.dialog = None;
                self.qr_full = false;
                return None;
            }
        };
        self.busy = true;
        self.error = None;
        Some(job)
    }

    pub fn close(&mut self) {
        self.call_needs_host = false;
        self.open = None;
        self.detail = None;
        self.msgs.clear();
        self.older = None;
        self.menu = None;
        self.reply = None;
        self.input.clear();
        self.receipts.clear();
        self.tick_vis.clear();
        self.tick_anims.clear();
        self.online.clear();
        self.pending = None;
    }

    pub fn reload(&mut self, api: &dyn GroupApi) {
        self.reload_list(api);
        self.reload_invitations(api);
        if let Ok(d) = api.group_device_id() {
            self.my_device = d;
        }
        self.reload_open(api);
    }

    fn reload_list(&mut self, api: &dyn GroupApi) {
        // A node without groups (or locked) answers with an error: an empty list.
        self.list = api.groups().unwrap_or_default();
        self.reload_requests(api);
    }

    fn reload_invitations(&mut self, api: &dyn GroupApi) {
        self.invitations = api.group_invitations_incoming().unwrap_or_default();
        self.reload_sent(api);
    }

    fn reload_sent(&mut self, api: &dyn GroupApi) {
        self.sent = match &self.open {
            Some(g) => api.group_invitations_outgoing(g.clone()).unwrap_or_default(),
            None => Vec::new(),
        };
    }

    fn reload_requests(&mut self, api: &dyn GroupApi) {
        self.incoming = api.group_chat_requests_incoming().unwrap_or_default();
        self.outgoing = api.group_chat_requests_outgoing().unwrap_or_default();
    }

    /// Re-reads the open group's detail and the loaded window of messages (delivery and removal
    /// marks change in place).
    fn reload_open(&mut self, api: &dyn GroupApi) {
        let Some(gid) = self.open.clone() else { return };
        self.reload_sent(api);
        match api.group_detail(gid.clone()) {
            Ok(d) => {
                // Closing drops the link in the core; a link we kept for this group goes with it.
                if !d.open && self.open_link.as_ref().is_some_and(|(g, _)| *g == gid) {
                    self.open_link = None;
                }
                self.detail = Some(d);
            }
            Err(_) => {
                self.close();
                return;
            }
        }
        self.reload_online(api);
        let want = (self.msgs.len() as u32).max(PAGE);
        if let Ok(p) = api.group_messages(gid.clone(), None, want) {
            self.msgs = p.messages;
            self.older = p.older;
        }
        self.reload_receipts(api, gid);
    }

    /// Who is online in the open group (a cheap in-memory read; a private group has no list).
    fn reload_online(&mut self, api: &dyn GroupApi) {
        self.online = match (&self.open, &self.detail) {
            (Some(gid), Some(d)) if d.open && active(d.my_role) => api.group_online(gid.clone()).unwrap_or_default(),
            _ => Vec::new(),
        };
    }

    /// The member receipts of my posts in the loaded window (one core read per day doc).
    fn reload_receipts(&mut self, api: &dyn GroupApi, gid: String) {
        let mine = self.msgs.iter().filter(|m| m.outgoing);
        let (lo, hi) = mine.fold((u64::MAX, 0u64), |(lo, hi), m| (lo.min(m.at), hi.max(m.at)));
        self.receipts.clear();
        if lo <= hi
            && let Ok(list) = api.group_receipts(gid, lo, hi)
        {
            self.receipts = list.into_iter().map(|r| (r.message_id.clone(), r)).collect();
        }
        self.sync_ticks();
    }

    /// My post's tick: waiting for a host = slack; otherwise the core's group `ReceiptTick` (one
    /// tick until every eligible member delivered, two, then read). None for others' posts.
    pub(super) fn tick_vis_of(&self, m: &GroupMessage) -> Option<crate::ticks::Vis> {
        if !m.outgoing || m.kind != GroupItemKind::Post {
            return None;
        }
        Some(match m.delivery {
            GroupDelivery::Waiting | GroupDelivery::Failed => crate::ticks::Vis::Slack,
            GroupDelivery::Delivered => self
                .receipts
                .get(&m.id)
                .and_then(|r| p2pcore::tick_vis(Some(r.tick), false))
                .unwrap_or(crate::ticks::Vis::One),
        })
    }

    /// Starts a one-shot transition for every loaded post whose tick changed since it was last shown;
    /// a post seen for the first time (open, scroll-in, new) starts settled. Same as 1:1's `sync_ticks`.
    pub(super) fn sync_ticks(&mut self) {
        let now = std::time::Instant::now();
        let next: std::collections::HashMap<String, crate::ticks::Vis> =
            self.msgs.iter().filter_map(|m| self.tick_vis_of(m).map(|v| (m.id.clone(), v))).collect();
        for (id, v) in &next {
            if let Some(old) = self.tick_vis.get(id)
                && old != v
                && !crate::motion::reduced()
            {
                self.tick_anims.insert(id.clone(), crate::ticks::Anim { from: *old, start: now });
            }
        }
        self.tick_anims.retain(|id, a| next.get(id).is_some_and(|v| crate::ticks::running(a, *v, now)));
        self.tick_vis = next;
    }

    /// A group tick transition is playing (keeps the frame clock running).
    pub(super) fn ticks_running(&self) -> bool {
        let now = std::time::Instant::now();
        self.tick_anims.iter().any(|(id, a)| self.tick_vis.get(id).is_some_and(|v| crate::ticks::running(a, *v, now)))
    }
}

/// "12 online"; at the presence cap the count may be cut short: "500+ online".
pub(super) fn online_label(n: usize) -> String {
    if n >= ONLINE_CAP { crate::i18n::tr_args("group_online_capped", &[("count", &ONLINE_CAP.to_string())]) } else { crate::i18n::tr_count("group_online_count", n as u64, &[]) }
}

/// The online list's order: admins first, then by name (case-insensitive), then DID.
pub(super) fn online_rows(list: &[OnlineMember]) -> Vec<&OnlineMember> {
    let mut v: Vec<&OnlineMember> = list.iter().collect();
    v.sort_by_key(|m| (!m.is_admin, m.name.to_lowercase(), m.did.clone()));
    v
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

/// Who a system line names: me, a person with a name, or someone without one.
enum Who {
    Me,
    Named(String),
    Unknown,
}

fn who_of(did: &str, name: &str, me: &str) -> Who {
    if did == me {
        Who::Me
    } else if name.trim().is_empty() {
        Who::Unknown
    } else {
        Who::Named(name.to_string())
    }
}

fn who_name(w: &Who) -> String {
    match w {
        Who::Named(n) => n.clone(),
        _ => String::new(),
    }
}

/// One whole sentence per actor and subject: `keys[actor][subject]`, each in the order
/// me, named, unknown. Names are the only things slotted in.
fn sys_pair(keys: &[[&str; 3]; 3], by: &Who, sub: &Who) -> String {
    let idx = |w: &Who| match w {
        Who::Me => 0,
        Who::Named(_) => 1,
        Who::Unknown => 2,
    };
    crate::i18n::tr_args(keys[idx(by)][idx(sub)], &[("by", &who_name(by)), ("name", &who_name(sub))])
}

/// A system line in plain words. `me` is our DID.
pub(super) fn sys_text(m: &GroupMessage, me: &str) -> String {
    let by = who_of(&m.author_did, &m.author_name, me);
    let sub = who_of(&m.subject_did, &m.subject_name, me);
    let one = |keys: [&str; 3], w: &Who| -> String {
        let key = match w {
            Who::Me => keys[0],
            Who::Named(_) => keys[1],
            Who::Unknown => keys[2],
        };
        crate::i18n::tr_args(key, &[("name", &who_name(w))])
    };
    match m.kind {
        GroupItemKind::Post => m.text.clone(),
        GroupItemKind::Joined => one(["group_sys_joined_you", "group_sys_joined", "group_sys_joined_someone"], &sub),
        GroupItemKind::Left => one(["group_sys_left_you", "group_sys_left", "group_sys_left_someone"], &sub),
        GroupItemKind::Removed => {
            let key = match &sub {
                Who::Me => "group_sys_removed_you",
                Who::Named(_) => "group_sys_removed",
                Who::Unknown => "group_sys_removed_someone",
            };
            crate::i18n::tr_count(key, FLAGS_TO_REMOVE as u64, &[("name", &who_name(&sub))])
        }
        GroupItemKind::AdminGranted => sys_pair(
            &[
                ["group_sys_admin_you_you", "group_sys_admin_you_other", "group_sys_admin_you_someone"],
                ["group_sys_admin_other_you", "group_sys_admin_other_other", "group_sys_admin_other_someone"],
                ["group_sys_admin_someone_you", "group_sys_admin_someone_other", "group_sys_admin_someone_someone"],
            ],
            &by,
            &sub,
        ),
        GroupItemKind::AdminRevoked => sys_pair(
            &[
                ["group_sys_unadmin_you_you", "group_sys_unadmin_you_other", "group_sys_unadmin_you_someone"],
                ["group_sys_unadmin_other_you", "group_sys_unadmin_other_other", "group_sys_unadmin_other_someone"],
                ["group_sys_unadmin_someone_you", "group_sys_unadmin_someone_other", "group_sys_unadmin_someone_someone"],
            ],
            &by,
            &sub,
        ),
        GroupItemKind::HostAdded => one(["group_sys_host_added_you", "group_sys_host_added", "group_sys_host_added_someone"], &sub),
        GroupItemKind::HostRemoved => one(["group_sys_host_removed_you", "group_sys_host_removed", "group_sys_host_removed_someone"], &sub),
    }
}

/// The list row's second line.
pub(super) fn preview(s: &GroupSummary, me: &str) -> String {
    match s.my_role {
        GroupRole::Removed => return crate::i18n::tr("group_you_were_removed_from_this"),
        GroupRole::Left => return crate::i18n::tr("group_you_left_this_group"),
        _ => {}
    }
    match &s.last {
        None => String::new(),
        Some(m) if m.kind != GroupItemKind::Post => sys_text(m, me),
        Some(m) if m.outgoing => crate::i18n::tr_args("group_preview_you", &[("text", &post_line(m))]),
        Some(m) if m.author_name.trim().is_empty() => crate::i18n::tr_args("group_preview_someone", &[("text", &post_line(m))]),
        Some(m) => crate::i18n::tr_args("group_preview_author", &[("name", first_name(&m.author_name)), ("text", &post_line(m))]),
    }
}

/// A post in one line: its text, "Voice message", "Photo" for a picture alone, "Photo · caption"
/// with one.
pub(super) fn post_line(m: &GroupMessage) -> String {
    if m.voice.is_some() {
        return crate::i18n::tr("chat_voice_message");
    }
    match (&m.image, m.text.is_empty()) {
        (Some(_), true) => crate::i18n::tr("group_post_photo"),
        (Some(_), false) => crate::i18n::tr_args("group_post_photo_caption", &[("caption", &m.text)]),
        (None, _) => m.text.clone(),
    }
}

/// The state line of a request we sent ("Waiting for Leo · via Hike"), and whether it is bad news.
/// A block is visible (U5 revised, P5): the requester is told, as on the board.
pub(super) fn sent_line(r: &GroupChatRequest) -> (String, bool) {
    let named = !r.peer_name.trim().is_empty();
    let f = first_name(&r.peer_name).to_string();
    let t = |named_key: &str, other: &str| crate::i18n::tr_args(if named { named_key } else { other }, &[("name", &f), ("group", &r.group_name)]);
    match r.state {
        ChatRequestState::Pending => (t("group_sent_waiting", "group_sent_waiting_unnamed"), false),
        ChatRequestState::Accepted if r.contact_added => (t("group_sent_accepted_messages", "group_sent_accepted_messages_unnamed"), false),
        ChatRequestState::Accepted => (t("group_sent_accepted_connecting", "group_sent_accepted_connecting_unnamed"), false),
        ChatRequestState::Declined => (t("group_sent_declined", "group_sent_declined_unnamed"), true),
        ChatRequestState::Blocked => (t("group_sent_blocked", "group_sent_blocked_unnamed"), true),
    }
}

impl super::App {
    pub(super) fn update_live(&mut self, m: Lm) -> Task<super::Msg> {
        if matches!(m, Lm::Attach) {
            if self.live.open.is_none() || self.live.sending {
                return Task::none();
            }
            return Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Post a picture")
                        .add_filter("Pictures", &["png", "jpg", "jpeg", "webp"])
                        .pick_file()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                |p| super::Msg::Live(Lm::Picked(p)),
            );
        }
        match m {
            Lm::VoiceToggle(id) => return self.live_voice_toggle(id),
            Lm::VoiceSeek(id, f) => {
                if let Some((pid, pl)) = &self.chat.player
                    && *pid == id
                {
                    pl.seek((f * pl.duration_ms() as f32) as u32);
                    return Task::none();
                }
                // Seeking a bubble that is not playing starts it.
                return self.live_voice_toggle(id);
            }
            // Leaving the group stops its voice message.
            Lm::Close | Lm::Open(_) if self.chat.player.as_ref().is_some_and(|(pid, _)| self.live.msgs.iter().any(|x| x.id == *pid)) => {
                self.chat.player = None;
            }
            Lm::VoiceReady(id, r) => {
                match r.and_then(|p| crate::voice::Player::play(&p)) {
                    Ok(pl) => self.chat.player = Some((id, pl)),
                    Err(e) => self.notice = Some(crate::i18n::tr_args("chat_could_not_play", &[("error", e.as_str())])),
                }
                return Task::none();
            }
            _ => {}
        }
        let node = self.node.clone();
        let out = self.live.step(&*node, m);
        let t = self.after_live(out);
        Task::batch([t, self.live_image_paths()])
    }

    /// A finished recording for group `gid` (from the shared recorder, `Cm::RecSend`): post it
    /// off the UI thread. The reply the composer was set to goes with nothing (a voice post has
    /// no quote), so it stays for the next text.
    pub(super) fn live_post_voice(&mut self, gid: String, path: String, waveform: Vec<u8>) -> Task<super::Msg> {
        let node = self.node.clone();
        let job = Job::PostVoice { gid, path, waveform };
        super::blocking(move || job.run(&*node), super::Msg::Live)
    }

    /// Play or pause a voice post: the one player the app has (a 1:1 voice message stops). The
    /// first play opens the recording into a file (a job).
    fn live_voice_toggle(&mut self, id: String) -> Task<super::Msg> {
        if let Some((pid, pl)) = &self.chat.player
            && *pid == id
            && !pl.finished()
        {
            if pl.is_paused() { pl.resume() } else { pl.pause() }
            return Task::none();
        }
        self.chat.player = None;
        let Some(gid) = self.live.open.clone() else { return Task::none() };
        let node = self.node.clone();
        let i = id.clone();
        super::blocking(
            move || node.group_voice_path(gid, i).map(std::path::PathBuf::from).ok_or_else(|| crate::i18n::tr("common_try_again")),
            move |r| super::Msg::Live(Lm::VoiceReady(id.clone(), r)),
        )
    }

    /// Opens the pictures of loaded posts that are here but not opened yet (a job: the first
    /// open decrypts the picture into a file).
    fn live_image_paths(&mut self) -> Task<super::Msg> {
        let Some(gid) = self.live.open.clone() else { return Task::none() };
        let ids: Vec<String> = self
            .live
            .msgs
            .iter()
            .filter(|m| m.image.as_ref().is_some_and(|i| i.state == p2pcore::GroupImageState::Ready))
            .filter(|m| !self.live.image_paths.contains_key(&m.id) && !self.live.image_asked.contains(&m.id))
            .map(|m| m.id.clone())
            .collect();
        if ids.is_empty() {
            return Task::none();
        }
        self.live.image_asked.extend(ids.iter().cloned());
        let node = self.node.clone();
        let job = Job::ImagePaths { gid, ids };
        super::blocking(move || job.run(&*node), super::Msg::Live)
    }

    fn after_live(&mut self, out: Out) -> Task<super::Msg> {
        if out.opened {
            self.leave_chat();
            self.sel = None;
            self.chat.info = false;
            self.screen = super::Screen::Home;
        }
        match &self.live.dialog {
            Some(LDialog::Token { text: Some(t) }) => {
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
        if let Some(did) = out.chat {
            if self.contacts.iter().any(|c| c.did == did) {
                tasks.push(self.update(super::Msg::Select(did)));
            }
        }
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

fn gid_for_receipts(open: &Option<String>) -> String {
    open.clone().unwrap_or_default()
}

/// The line under one of my posts the hosts refuse (`Failed`): the core's words, none otherwise.
/// It sits in the slot of "Waiting for a host", so nothing moves when a post turns failed.
pub(crate) fn failed_line(m: &GroupMessage) -> Option<&str> {
    if m.outgoing && m.delivery == GroupDelivery::Failed { m.failure.as_deref() } else { None }
}

/// Human-readable permission state for one device, shared by the list and the consent banner.
pub(super) fn hosting_status(d: &GroupDetail, did: &str, dev: &str) -> String {
    let has = |hs: &[p2pcore::GroupHost]| hs.iter().any(|h| h.did == did && h.device_id == dev);
    let state = |active, pending, offered| crate::i18n::tr(if active { "group_hosting_active" }
        else if pending { "group_hosting_waiting_device" } else if offered { "group_hosting_waiting_admin" }
        else { "group_hosting_not_enabled" });
    format!("{}: {} · {}: {}", crate::i18n::tr_str("group_hosting_messages"),
        state(has(&d.hosts), has(&d.host_requests), has(&d.host_offers)),
        crate::i18n::tr_str("group_hosting_calls"),
        state(has(&d.call_hosts), has(&d.call_host_requests), has(&d.call_host_offers)))
}
