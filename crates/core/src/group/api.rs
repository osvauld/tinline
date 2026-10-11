//! The UniFFI surface of group chat (G1). Additive to `Node`. Conventions as in chat: times are
//! unix ms, DIDs name people, `group_id` names a group; blocking methods are for the app's IO
//! threads. A locked node fails with `Locked`.
//!
//! Roles: the owner (who created the group) is an admin; admins are made by the owner; any
//! admin may flag (3 flags remove, U9/U15); hosts are member devices appointed by an admin that
//! accepted hosting (`group_accept_host`). Nothing here exposes another member's endpoint.

use std::sync::Arc;
use std::time::Duration;

use super::engine::{Changed, GroupCore, INVITE_TTL, proof_removes};
use super::record::*;
use super::state::{Item, ItemKind};
use super::store::GroupMeta;
use crate::Error;
use crate::node::{Inner, Node, now};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum GroupRole {
    Owner,
    Admin,
    Member,
    /// Removed by 3 admin flags: read-only archive, composer disabled.
    Removed,
    /// Left the group (or never completed joining).
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
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
/// host acknowledged holding it. Everything else is `Delivered`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum GroupDelivery {
    Waiting,
    Delivered,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
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
    /// A post by a member removed since, kept because it was written before the removal (U11).
    pub before_removal: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupPage {
    /// Oldest first.
    pub messages: Vec<GroupMessage>,
    /// Cursor for the next older page (`group_messages(before = this)`); `None` at the start.
    pub older: Option<String>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
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

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupHost {
    pub did: String,
    pub name: String,
    pub device_id: String,
    pub appointed_by: Option<String>,
    pub is_me: bool,
    /// This device has a live session with it (or it is this device).
    pub online: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupFlag {
    pub by_did: String,
    pub by_name: String,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupRemoval {
    /// When the 3rd flag was made.
    pub at: u64,
    /// The flags that removed us, each DID-signed and checked on this device.
    pub flags: Vec<GroupFlag>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
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
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
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
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupInvitePeek {
    pub group_id: String,
    pub name: String,
    pub owner_did: String,
    /// `None` for a token for one's own other devices.
    pub issuer_did: Option<String>,
    /// Unix seconds.
    pub expires: Option<u64>,
    pub host_count: u32,
}

/// Registered with `Node::set_group_events`. Called on core threads: do not block.
#[uniffi::export(with_foreign)]
pub trait GroupEvents: Send + Sync {
    /// A message or system line appeared.
    fn on_group_message(&self, message: GroupMessage);
    /// Anything else about the group changed (members, roles, hosts, delivery, removal).
    fn on_group_changed(&self, group_id: String);
}

fn kind_of(k: &ItemKind) -> GroupItemKind {
    match k {
        ItemKind::Post => GroupItemKind::Post,
        ItemKind::Joined => GroupItemKind::Joined,
        ItemKind::Left => GroupItemKind::Left,
        ItemKind::Removed => GroupItemKind::Removed,
        ItemKind::AdminGranted => GroupItemKind::AdminGranted,
        ItemKind::AdminRevoked => GroupItemKind::AdminRevoked,
        ItemKind::HostAdded => GroupItemKind::HostAdded,
        ItemKind::HostRemoved => GroupItemKind::HostRemoved,
    }
}

fn parse_dev(hexs: &str) -> Result<[u8; 32], Error> {
    hex::decode(hexs).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| Error::Protocol("bad device id".into()))
}

impl Inner {
    fn group_name_of(&self, core: &GroupCore, gid: &str, did: &str) -> String {
        if did == core.did {
            return self.me().map(|m| m.profile.name.clone()).unwrap_or_default();
        }
        if let Some(n) = crate::block::known_name(&self.shared.lock().state, did) {
            return n;
        }
        core.groups.lock().get(gid).and_then(|l| l.state.members.get(did).map(|m| m.name.clone())).unwrap_or_default()
    }

    fn group_role(core: &GroupCore, live: &super::engine::Live) -> GroupRole {
        let s = &live.state;
        if s.is_removed(&core.did) || !live.meta.removal_proof.is_empty() {
            GroupRole::Removed
        } else if s.owner.as_deref() == Some(core.did.as_str()) {
            GroupRole::Owner
        } else if s.is_admin(&core.did) {
            GroupRole::Admin
        } else if s.is_member(&core.did) {
            GroupRole::Member
        } else {
            GroupRole::Left
        }
    }

    pub(crate) fn group_message(&self, core: &GroupCore, gid: &str, i: &Item) -> Option<GroupMessage> {
        let hosting = core.groups.lock().get(gid)?.state.is_host(&core.did, &core.dev);
        let outgoing = i.author == core.did;
        let delivery = if i.peer != core.wid || hosting {
            GroupDelivery::Delivered
        } else {
            let acked = core.store.acks(gid).unwrap_or_default().iter().any(|(_, vv)| {
                loro::VersionVector::decode(vv).ok().and_then(|v| v.get(&core.wid).copied()).unwrap_or(0) > i.counter
            });
            if acked { GroupDelivery::Delivered } else { GroupDelivery::Waiting }
        };
        Some(GroupMessage {
            id: i.id.clone(),
            group_id: gid.to_string(),
            kind: kind_of(&i.kind),
            author_name: self.group_name_of(core, gid, &i.author),
            author_did: i.author.clone(),
            subject_name: self.group_name_of(core, gid, &i.subject),
            subject_did: i.subject.clone(),
            at: i.at.max(0) as u64,
            text: i.text.clone(),
            reply_to: i.reply_to.clone(),
            outgoing,
            delivery,
            before_removal: i.before_removal,
        })
    }

    fn group_summary(&self, core: &GroupCore, gid: &str) -> Option<GroupSummary> {
        let (name, role, members, hosts, here, last, unread, reachable) = {
            let groups = core.groups.lock();
            let live = groups.get(gid)?;
            if live.state.owner.is_none() {
                return None;
            }
            let reachable = live.state.hosts.iter().any(|h| (h.did == core.did && h.dev == core.dev) || core.has_session(gid, &h.did, &h.dev));
            let unread = live.state.items.iter().filter(|i| i.at > live.meta.read_upto && i.author != core.did && i.kind == ItemKind::Post).count() as u32;
            (
                live.state.name.clone(),
                Self::group_role(core, live),
                live.state.members.len() as u32,
                live.state.hosts.len() as u32,
                live.state.is_host(&core.did, &core.dev),
                live.state.items.last().cloned(),
                unread,
                reachable,
            )
        };
        Some(GroupSummary {
            id: gid.to_string(),
            name,
            my_role: role,
            member_count: members,
            host_count: hosts,
            hosted_here: here,
            host_reachable: reachable,
            last: last.and_then(|i| self.group_message(core, gid, &i)),
            unread,
        })
    }

    fn group_detail(&self, core: &GroupCore, gid: &str) -> Result<GroupDetail, Error> {
        let groups = core.groups.lock();
        let live = groups.get(gid).ok_or(Error::NotFound)?;
        let s = live.state.clone();
        let role = Self::group_role(core, live);
        let proof = live.meta.removal_proof.clone();
        drop(groups);
        let owner = s.owner.clone().unwrap_or_default();
        let members = s
            .members
            .values()
            .map(|m| GroupMember {
                did: m.did.clone(),
                name: self.group_name_of(core, gid, &m.did),
                is_me: m.did == core.did,
                is_owner: m.did == owner,
                is_admin: s.is_admin(&m.did),
                is_host: s.hosts.iter().any(|h| h.did == m.did),
                flags: s.flags.get(&m.did).map_or(0, |f| f.len() as u32),
                joined_at: m.joined_at.max(0) as u64,
                invited_by: m.invited_by.clone(),
                device_ids: m.devices.iter().map(hex::encode).collect(),
            })
            .collect();
        let host = |did: &str, dev: &[u8; 32], by: Option<String>| GroupHost {
            did: did.to_string(),
            name: self.group_name_of(core, gid, did),
            device_id: hex::encode(dev),
            appointed_by: by,
            is_me: did == core.did && dev == &core.dev,
            online: (did == core.did && dev == &core.dev) || core.has_session(gid, did, dev),
        };
        let hosts = s.hosts.iter().map(|h| host(&h.did, &h.dev, Some(h.appointed_by.clone()))).collect();
        let host_offers = s.host_offers.iter().map(|(d, v)| host(d, v, None)).collect();
        let host_requests = s.host_requests.iter().map(|h| host(&h.did, &h.dev, Some(h.appointed_by.clone()))).collect();
        let host_requested_here = s.host_requests.iter().any(|h| h.did == core.did && h.dev == core.dev);
        let call_hosts = s.call_hosts.iter().map(|h| host(&h.did, &h.dev, Some(h.appointed_by.clone()))).collect();
        let call_host_requests = s.call_host_requests.iter().map(|h| host(&h.did, &h.dev, Some(h.appointed_by.clone()))).collect();
        let call_host_requested_here = s.call_host_requests.iter().any(|h| h.did == core.did && h.dev == core.dev);
        let flags_on_me = s.removed.get(&core.did).map(|f| f.iter().map(|f| (f.by.clone(), f.at)).collect::<Vec<_>>()).or_else(|| proof_removes(gid, &core.did, &proof));
        let removal = flags_on_me.map(|mut f| {
            f.sort_by_key(|(_, at)| *at);
            GroupRemoval {
                at: f.get(super::state::REMOVAL_FLAGS - 1).map_or(0, |(_, at)| (*at).max(0) as u64),
                flags: f.into_iter().map(|(by, at)| GroupFlag { by_name: self.group_name_of(core, gid, &by), by_did: by, at: at.max(0) as u64 }).collect(),
            }
        });
        Ok(GroupDetail {
            id: gid.to_string(),
            name: s.name.clone(),
            owner_did: owner,
            my_role: role,
            am_host: s.is_host(&core.did, &core.dev),
            members,
            hosts,
            host_offers,
            host_requests,
            host_requested_here,
            removal,
            am_call_host: s.is_call_host(&core.did, &core.dev),
            call_hosts,
            call_host_requests,
            call_host_requested_here,
        })
    }

    /// Writes one record of ours, then relays it and reports.
    fn group_write(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, body: Body) -> Result<GroupMessage, Error> {
        let (entry, changed): (_, Changed) = core.write(gid, body)?;
        let item = changed.new_items.iter().find(|i| i.id == entry.id).cloned();
        self.group_after(core, changed, None);
        let item = item.unwrap_or(Item {
            id: entry.id.clone(),
            kind: ItemKind::Post,
            author: core.did.clone(),
            subject: core.did.clone(),
            at: entry.rec.at,
            text: String::new(),
            reply_to: None,
            before_removal: false,
            peer: entry.peer,
            counter: entry.counter,
        });
        self.group_message(core, gid, &item).ok_or(Error::NotFound)
    }

    /// The state check every action makes first: we are a current member (not removed, not left).
    pub(crate) fn group_member_check(&self, core: &GroupCore, gid: &str) -> Result<(), Error> {
        let groups = core.groups.lock();
        let live = groups.get(gid).ok_or(Error::NotFound)?;
        let ignore = self.group.ignore_removal.load(std::sync::atomic::Ordering::SeqCst);
        if !ignore && (live.state.is_removed(&core.did) || !live.meta.removal_proof.is_empty()) {
            return Err(Error::Rejected("removed".into()));
        }
        if !live.state.is_member(&core.did) && !(ignore && live.state.is_removed(&core.did)) {
            return Err(Error::Rejected("not a member".into()));
        }
        Ok(())
    }

    pub(crate) fn group_admin_check(&self, core: &GroupCore, gid: &str) -> Result<(), Error> {
        self.group_member_check(core, gid)?;
        if !core.groups.lock().get(gid).is_some_and(|l| l.state.is_admin(&core.did)) {
            return Err(Error::Rejected("only admins can do that".into()));
        }
        Ok(())
    }

    /// The full token for `gid`: our signed invite (admins) or none (own devices), and the hosts.
    /// G3: it no longer travels as text; a host keeps it behind a short code (`code.rs`), and
    /// direct invitations carry it over the private channel (`invitations.rs`).
    pub(crate) fn group_token(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, invite: Option<SignedInvite>) -> Result<InviteToken, Error> {
        let _ = self.group_my_addr(core, gid);
        let (name, owner) = {
            let groups = core.groups.lock();
            let l = groups.get(gid).ok_or(Error::NotFound)?;
            (l.state.name.clone(), l.state.owner.clone().unwrap_or_default())
        };
        // Chat hosts only (ours when we host the chat; never our call-only address): a call-only
        // host refuses joiners.
        let all = core.chat_host_addrs(gid);
        if all.is_empty() {
            return Err(Error::Rejected("the group has no reachable host yet".into()));
        }
        Ok(InviteToken { group: gid.to_string(), name, owner, invite, hosts: all })
    }
}

#[uniffi::export]
impl Node {
    pub fn set_group_events(&self, events: Arc<dyn GroupEvents>) {
        *self.inner.group.events.lock() = Some(events);
    }

    /// Groups this account is in (or was removed from / left), most recent activity first.
    pub fn groups(&self) -> Result<Vec<GroupSummary>, Error> {
        let core = self.inner.group_core()?;
        let ids: Vec<String> = core.groups.lock().keys().cloned().collect();
        let mut out: Vec<GroupSummary> = ids.iter().filter_map(|g| self.inner.group_summary(&core, g)).collect();
        out.sort_by(|a, b| b.last.as_ref().map_or(0, |m| m.at).cmp(&a.last.as_ref().map_or(0, |m| m.at)));
        Ok(out)
    }

    pub fn group_detail(&self, group_id: String) -> Result<GroupDetail, Error> {
        let core = self.inner.group_core()?;
        self.inner.group_detail(&core, &group_id)
    }

    /// Up to `limit` messages and system lines older than `before` (a cursor from an earlier
    /// page; `None` = the newest), oldest first.
    pub fn group_messages(&self, group_id: String, before: Option<String>, limit: u32) -> Result<GroupPage, Error> {
        let core = self.inner.group_core()?;
        let items: Vec<Item> = {
            let groups = core.groups.lock();
            groups.get(&group_id).ok_or(Error::NotFound)?.state.items.clone()
        };
        let end = match &before {
            None => items.len(),
            Some(c) => {
                let (at, id) = c.split_once(':').ok_or_else(|| Error::Protocol("bad cursor".into()))?;
                let at: i64 = at.parse().map_err(|_| Error::Protocol("bad cursor".into()))?;
                items.partition_point(|i| (i.at, i.id.as_str()) < (at, id))
            }
        };
        let start = end.saturating_sub(limit.max(1) as usize);
        let older = (start > 0).then(|| format!("{}:{}", items[start].at, items[start].id));
        let messages = items[start..end].iter().filter_map(|i| self.inner.group_message(&core, &group_id, i)).collect();
        Ok(GroupPage { messages, older })
    }

    /// Everything up to now is read (for `unread`).
    pub fn group_mark_read(&self, group_id: String) -> Result<(), Error> {
        let core = self.inner.group_core()?;
        let mut groups = core.groups.lock();
        let live = groups.get_mut(&group_id).ok_or(Error::NotFound)?;
        live.meta.read_upto = live.state.items.last().map_or(0, |i| i.at);
        core.store.put_meta(&group_id, &live.meta)
    }

    /// Creates a group owned by us. This device becomes its first host (PROVISIONAL default:
    /// apps on phones should appoint a desktop and revoke this, see docs/group-core.md).
    pub fn group_create(&self, name: String) -> Result<String, Error> {
        let name = name.trim().to_string();
        if name.is_empty() || name.len() > MAX_NAME {
            return Err(Error::Protocol("group name must be 1-100 bytes".into()));
        }
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            inner.group_ensure(&core).await?;
            let nonce: [u8; 16] = rand::random();
            let gid = group_id(&core.did, &nonce);
            {
                let live = super::engine::Live {
                    doc: super::doc::GroupDoc::new(&gid, core.wid),
                    entries: Vec::new(),
                    state: Default::default(),
                    meta: GroupMeta::default(),
                };
                core.groups.lock().insert(gid.clone(), live);
                core.store.put_meta(&gid, &GroupMeta::default())?;
            }
            let owner_name = inner.me()?.profile.name.clone();
            inner.group_write(&core, &gid, Body::Genesis { name, nonce, owner_name })?;
            inner.group_write(&core, &gid, Body::HostPermit { host: core.did.clone(), host_dev: core.dev, active: true })?;
            inner.group_write(&core, &gid, Body::HostAccept { active: true })?;
            Ok(gid)
        })?
    }

    /// A short invite link (`tinline://g/...`, about 77 characters; for a card, a share sheet or a
    /// QR) for an invite signed by us; admins only. Valid 7 days. A host keeps the invite behind
    /// the link's code (we, if we host; else a host we are connected to, which can take up to 30
    /// s to reach). See `code.rs`.
    pub fn group_invite(&self, group_id: String) -> Result<String, Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            inner.group_admin_check(&core, &group_id)?;
            inner.group_ensure(&core).await?;
            inner.group_relay_ready().await;
            let invite = Invite { group: group_id.clone(), issuer: core.did.clone(), expires: now() + INVITE_TTL, nonce: rand::random() }.sign(&core.account);
            inner.group_issue_code(&core, &group_id, Some(invite)).await
        })?
    }

    /// A short link for our own other devices (same DID): it admits only this account. Valid 7
    /// days.
    pub fn group_device_token(&self, group_id: String) -> Result<String, Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            inner.group_member_check(&core, &group_id)?;
            inner.group_ensure(&core).await?;
            inner.group_relay_ready().await;
            inner.group_issue_code(&core, &group_id, None).await
        })?
    }

    /// What an invite link is for, fetched from the host it names (blocks up to about 20 s;
    /// `None` if the link is not one, the host refuses it, or cannot be reached).
    pub fn group_peek_invite(&self, token: String) -> Option<GroupInvitePeek> {
        let sc = super::code::ShortCode::parse(&token)?;
        let inner = self.inner.clone();
        let t = self.block_on(async move { inner.group_fetch(&sc).await }).ok()?.ok()?;
        let inv = t.invite.as_ref().and_then(|i| i.open());
        if t.invite.is_some() && inv.as_ref().is_none_or(|i| i.group != t.group) {
            return None;
        }
        Some(GroupInvitePeek {
            group_id: t.group.clone(),
            name: t.name.clone(),
            owner_did: t.owner.clone(),
            issuer_did: inv.as_ref().map(|i| i.issuer.clone()),
            expires: inv.map(|i| i.expires),
            host_count: t.hosts.iter().filter(|h| h.open().is_some_and(|a| a.group == t.group)).count() as u32,
        })
    }

    /// Joins with an invite link (or its bare code): fetches the invite from the host the link
    /// names, then joins through the group's hosts and waits (up to about a minute) until this
    /// device holds the group as a member. A host refusal comes back as `Rejected(reason)`.
    pub fn group_join(&self, token: String) -> Result<String, Error> {
        let sc = super::code::ShortCode::parse(&token).ok_or_else(|| Error::Protocol("not a group invite".into()))?;
        let inner = self.inner.clone();
        self.block_on(async move {
            let t = inner.group_fetch(&sc).await?;
            inner.group_join_token(t).await
        })?
    }

    pub fn group_post(&self, group_id: String, text: String, reply_to: Option<String>) -> Result<GroupMessage, Error> {
        if text.is_empty() || text.len() > MAX_TEXT {
            return Err(Error::Protocol("message must be 1-20000 bytes".into()));
        }
        let core = self.inner.group_core()?;
        self.inner.group_member_check(&core, &group_id)?;
        self.inner.group_write(&core, &group_id, Body::Post { text, reply_to })
    }

    /// One admin flag on `member_did` (U9/U15: any 3 remove; final, visible to them).
    pub fn group_flag(&self, group_id: String, member_did: String) -> Result<(), Error> {
        let core = self.inner.group_core()?;
        self.inner.group_admin_check(&core, &group_id)?;
        let ok = core.groups.lock().get(&group_id).is_some_and(|l| l.state.is_member(&member_did) && l.state.owner.as_deref() != Some(member_did.as_str()));
        if !ok {
            return Err(Error::Rejected("not a member that can be flagged".into()));
        }
        self.inner.group_write(&core, &group_id, Body::Flag { member: member_did }).map(|_| ())
    }

    /// Owner only: make `member_did` an admin, or not.
    pub fn group_set_admin(&self, group_id: String, member_did: String, admin: bool) -> Result<(), Error> {
        let core = self.inner.group_core()?;
        self.inner.group_member_check(&core, &group_id)?;
        let ok = core.groups.lock().get(&group_id).is_some_and(|l| l.state.owner.as_deref() == Some(core.did.as_str()) && l.state.is_member(&member_did));
        if !ok {
            return Err(Error::Rejected("only the owner can change admins of members".into()));
        }
        self.inner.group_write(&core, &group_id, Body::Admin { member: member_did, admin }).map(|_| ())
    }

    /// Leaves the group (PROVISIONAL: the owner cannot leave).
    pub fn group_leave(&self, group_id: String) -> Result<(), Error> {
        let core = self.inner.group_core()?;
        self.inner.group_member_check(&core, &group_id)?;
        if core.groups.lock().get(&group_id).is_some_and(|l| l.state.owner.as_deref() == Some(core.did.as_str())) {
            return Err(Error::Rejected("the owner cannot leave".into()));
        }
        self.inner.group_write(&core, &group_id, Body::Leave).map(|_| ())
    }

    /// Admin: device `device_id` of member `did` may host. It hosts once it accepts.
    pub fn group_appoint_host(&self, group_id: String, did: String, device_id: String) -> Result<(), Error> {
        let dev = parse_dev(&device_id)?;
        let core = self.inner.group_core()?;
        self.inner.group_admin_check(&core, &group_id)?;
        if !core.groups.lock().get(&group_id).is_some_and(|l| l.state.is_member(&did)) {
            return Err(Error::Rejected("hosts must be members".into()));
        }
        self.inner.group_write(&core, &group_id, Body::HostPermit { host: did, host_dev: dev, active: true }).map(|_| ())
    }

    /// Admin: device `device_id` of `did` stops hosting. The remaining hosts rotate their group
    /// endpoint after the U12 delay (§10 Q8).
    pub fn group_revoke_host(&self, group_id: String, did: String, device_id: String) -> Result<(), Error> {
        let dev = parse_dev(&device_id)?;
        let core = self.inner.group_core()?;
        self.inner.group_admin_check(&core, &group_id)?;
        self.inner.group_write(&core, &group_id, Body::HostPermit { host: did, host_dev: dev, active: false }).map(|_| ())
    }

    /// This device agrees to host (or stops hosting). Hosting needs this and an admin's permit.
    pub fn group_accept_host(&self, group_id: String, accept: bool) -> Result<(), Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            inner.group_member_check(&core, &group_id)?;
            inner.group_ensure(&core).await?;
            inner.group_write(&core, &group_id, Body::HostAccept { active: accept }).map(|_| ())
        })?
    }

    /// Admin: device `device_id` of member `did` may host the group's CALLS (U21; separate
    /// from chat hosting). It hosts calls once it accepts (`group_accept_call_host`).
    pub fn group_appoint_call_host(&self, group_id: String, did: String, device_id: String) -> Result<(), Error> {
        let dev = parse_dev(&device_id)?;
        let core = self.inner.group_core()?;
        self.inner.group_admin_check(&core, &group_id)?;
        if !core.groups.lock().get(&group_id).is_some_and(|l| l.state.is_member(&did)) {
            return Err(Error::Rejected("call hosts must be members".into()));
        }
        self.inner.group_write(&core, &group_id, Body::CallHostPermit { host: did, host_dev: dev, active: true }).map(|_| ())
    }

    /// Admin: device `device_id` of `did` stops hosting calls. A call it hosts moves to a
    /// backup (members redial, G6 §5).
    pub fn group_revoke_call_host(&self, group_id: String, did: String, device_id: String) -> Result<(), Error> {
        let dev = parse_dev(&device_id)?;
        let core = self.inner.group_core()?;
        self.inner.group_admin_check(&core, &group_id)?;
        self.inner.group_write(&core, &group_id, Body::CallHostPermit { host: did, host_dev: dev, active: false }).map(|_| ())
    }

    /// This device agrees to host the group's calls (or stops). Needs this and an admin's
    /// call-host permit. Hosting exposes this device's group endpoint to every caller.
    pub fn group_accept_call_host(&self, group_id: String, accept: bool) -> Result<(), Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            inner.group_member_check(&core, &group_id)?;
            inner.group_ensure(&core).await?;
            inner.group_write(&core, &group_id, Body::CallHostAccept { active: accept }).map(|_| ())
        })?
    }

    /// This install's device ID for this account (hex): what an admin appoints as host.
    pub fn group_device_id(&self) -> Result<String, Error> {
        Ok(hex::encode(self.inner.group_core()?.dev))
    }

    /// On a host: when this device's group endpoint will rotate (unix ms), because of a removal
    /// or a host revocation in a group it hosts (U12: 2 days after). `None` if none is pending.
    pub fn group_endpoint_rotation_due(&self) -> Option<u64> {
        let core = self.inner.group_core().ok()?;
        self.inner.group_rotation_due(&core)
    }

    /// This device's group endpoint id, once it is up (`None` before any group).
    pub fn group_endpoint_id(&self) -> Option<String> {
        self.inner.group_endpoint().map(|e| e.id().to_string())
    }
}

/// Test hooks (not for apps).
#[uniffi::export]
impl Node {
    /// Group endpoint ids this device holds for `group_id` (hosts' addresses and, on a host, its
    /// members' directory).
    pub fn group_known_endpoints_for_test(&self, group_id: String) -> Vec<String> {
        self.inner.group_core().map(|c| c.known_endpoints(&group_id)).unwrap_or_default()
    }

    /// Host side: the member group endpoints in its directory.
    pub fn group_directory_for_test(&self, group_id: String) -> Vec<String> {
        self.inner.group_core().map(|c| c.store.dir(&group_id).unwrap_or_default().into_iter().map(|(_, d)| d.endpoint).collect()).unwrap_or_default()
    }

    /// Dial only hosts of `did` (`None`: all).
    pub fn group_dial_only_for_test(&self, did: Option<String>) {
        *self.inner.group.dial_only.lock() = did;
        if let Ok(c) = self.inner.group_core() {
            c.close_sessions_except(self.inner.group.dial_only.lock().as_deref());
        }
    }

    /// The U12 rotation delay in seconds (default 2 days).
    pub fn group_set_rotation_delay_for_test(&self, secs: u64) {
        self.inner.group.rotation_delay_ms.store(secs.max(1) * 1000, std::sync::atomic::Ordering::SeqCst);
    }

    /// How long a rotated host keeps telling offline members it moved, in seconds (default: the
    /// rotation delay).
    pub fn group_set_moved_window_for_test(&self, secs: u64) {
        self.inner.group.moved_window_ms.store(secs.max(1) * 1000, std::sync::atomic::Ordering::SeqCst);
    }

    /// The next `n` invite fetches (one network attempt each) fail as if the connection was lost.
    pub fn group_fail_next_fetches_for_test(&self, n: u32) {
        self.inner.group.fail_fetches.store(n, std::sync::atomic::Ordering::SeqCst);
    }

    /// The next `n` dials to a host fail as if the connection was lost.
    pub fn group_fail_next_dials_for_test(&self, n: u32) {
        self.inner.group.fail_dials.store(n, std::sync::atomic::Ordering::SeqCst);
    }

    /// Behave like a modified app that ignores its own removal (keeps posting and dialing).
    pub fn group_ignore_removal_for_test(&self, ignore: bool) {
        self.inner.group.ignore_removal.store(ignore, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Inner {
    pub(crate) fn group_owes_pub(&self, core: &GroupCore, gid: &str) -> bool {
        let mine = core.groups.lock().get(gid).map(|l| l.doc.vv().get(&core.wid).copied().unwrap_or(0)).unwrap_or(0);
        if mine == 0 || core.groups.lock().get(gid).is_some_and(|l| l.state.is_host(&core.did, &core.dev)) {
            return false;
        }
        !core.store.acks(gid).unwrap_or_default().iter().any(|(_, vv)| loro::VersionVector::decode(vv).ok().and_then(|v| v.get(&core.wid).copied()).unwrap_or(0) >= mine)
    }

    fn group_forget(&self, core: &GroupCore, gid: &str) {
        core.groups.lock().remove(gid);
        let _ = core.store.forget(gid);
    }
}

/// While a join waits: how often failed dials to the group's hosts are tried again.
const JOIN_REDIAL: Duration = Duration::from_secs(2);

impl Inner {
    /// Joins with a full (fetched or delivered) token: the same checks whatever brought it.
    pub(crate) async fn group_join_token(self: &Arc<Self>, t: InviteToken) -> Result<String, Error> {
        let inner = self;
        let core = inner.group_core()?;
        let gid = t.group.clone();
        if let Some(inv) = &t.invite
            && inv.open().is_none_or(|i| i.group != gid || i.expires < now())
        {
            return Err(Error::Rejected("invite expired or invalid".into()));
        }
        let already = core.groups.lock().get(&gid).map(|l| l.state.is_member(&core.did));
        match already {
            Some(true) => return Ok(gid),
            Some(false) => {}
            None => {
                let meta = GroupMeta { joining: t.invite.as_ref().map(|i| bincode::serialize(i).expect("serialises")), ..Default::default() };
                let live = super::engine::Live { doc: super::doc::GroupDoc::new(&gid, core.wid), entries: Vec::new(), state: Default::default(), meta: meta.clone() };
                core.groups.lock().insert(gid.clone(), live);
                core.store.put_meta(&gid, &meta)?;
            }
        }
        for h in &t.hosts {
            if let Some(a) = h.open()
                && a.group == gid
            {
                core.store.put_host(&gid, &a.did, &a.dev, h)?;
            }
        }
        core.refusals.lock().remove(&gid);
        inner.group_ensure(&core).await?;
        let end = std::time::Instant::now() + Duration::from_secs(60);
        let mut hurried = std::time::Instant::now();
        loop {
            let (member, joined) = {
                let groups = core.groups.lock();
                let l = groups.get(&gid).ok_or(Error::NotFound)?;
                (l.state.is_member(&core.did), l.meta.joining.is_none())
            };
            let acked = !inner.group_owes_pub(&core, &gid);
            if member && joined && acked {
                return Ok(gid);
            }
            let refused = core.refusals.lock().get(&gid).cloned();
            if let Some(why) = refused.filter(|_| !member) {
                inner.group_forget(&core, &gid);
                return Err(Error::Rejected(why));
            }
            // GL: a dial that failed (lost connection, timeout under load) is tried again every
            // couple of seconds while the user waits, within this deadline.
            if hurried.elapsed() >= JOIN_REDIAL {
                core.hurry(&gid);
                hurried = std::time::Instant::now();
            }
            if std::time::Instant::now() > end {
                if !member {
                    inner.group_forget(&core, &gid);
                }
                return Err(Error::Timeout);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}
