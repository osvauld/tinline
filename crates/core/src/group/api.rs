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
use super::shard::brief_item;
use super::state::{Item, ItemKind};
use super::store::GroupMeta;
use crate::Error;
use crate::chat::doc::day_of;
use crate::node::{Inner, Node, now_ms};

mod hooks;
mod hosts;
mod presence;
mod rotation;
mod types;
pub use types::*;

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
    pub(crate) fn group_name_of(&self, core: &GroupCore, gid: &str, did: &str) -> String {
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
        let hosting = core.with_state(gid, |st| st.is_host(&core.did, &core.dev))?;
        let outgoing = i.author == core.did;
        let delivery = if i.peer != core.wid || hosting {
            GroupDelivery::Delivered
        } else {
            // Our post sits in the day doc of its time, any other record in the meta doc; some
            // host acknowledged holding it.
            let acks = if i.kind == ItemKind::Post { core.store.acks_day(gid, &day_of(i.at)) } else { core.store.acks(gid) };
            let acked = acks.unwrap_or_default().iter().any(|(_, vv)| {
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
        let (name, role, members, hosts, here, last, unread, reachable, open, admins_only) = {
            let groups = core.groups.lock();
            let live = groups.get(gid)?;
            if live.state.owner.is_none() {
                return None;
            }
            let reachable = live.state.hosts.iter().any(|h| (h.did == core.did && h.dev == core.dev) || core.has_session(gid, &h.did, &h.dev));
            // The row comes from the group's local state: no day doc is loaded for the list.
            let last_post = live.meta.last.as_ref().map(|b| brief_item(b, &live.state));
            let last = match (last_post, live.state.items.last()) {
                (Some(p), Some(s)) if (s.at, &s.id) > (p.at, &p.id) => Some(s.clone()),
                (Some(p), _) => Some(p),
                (None, s) => s.cloned(),
            };
            (
                live.state.name.clone(),
                Self::group_role(core, live),
                live.state.members.len() as u32,
                live.state.hosts.len() as u32,
                live.state.is_host(&core.did, &core.dev),
                last,
                live.meta.unread,
                reachable,
                live.state.open,
                live.state.admins_only,
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
            open,
            admins_only,
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
            open: s.open,
            admins_only: s.admins_only,
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
        Ok(InviteToken { group: gid.to_string(), name, owner, invite, hosts: all, open: false })
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
    /// page; `None` = the newest), oldest first. Pages across day docs backwards, loading only
    /// the days it needs; `older` is `None` once every day we hold has been shown (history
    /// still arriving in the background shows up on the next call).
    pub fn group_messages(&self, group_id: String, before: Option<String>, limit: u32) -> Result<GroupPage, Error> {
        let core = self.inner.group_core()?;
        let cursor = match &before {
            None => None,
            Some(c) => {
                let (at, id) = c.split_once(':').ok_or_else(|| Error::Protocol("bad cursor".into()))?;
                Some((at.parse::<i64>().map_err(|_| Error::Protocol("bad cursor".into()))?, id.to_string()))
            }
        };
        let limit = limit.max(1) as usize;
        let before_key = |i: &Item| cursor.as_ref().is_none_or(|(at, id)| (i.at, i.id.as_str()) < (*at, id.as_str()));
        let (items, more) = {
            let mut groups = core.groups.lock();
            let live = groups.get_mut(&group_id).ok_or(Error::NotFound)?;
            let days = core.store.days(&group_id)?;
            // A post sits in the day doc of its time, or of a neighbouring day within the hour
            // of tolerance: start one hour later than the cursor.
            let mut idx = match &cursor {
                None => days.len(),
                Some((at, _)) => {
                    let start = day_of(at.saturating_add(crate::chat::doc::HOUR_MS));
                    days.partition_point(|d| *d <= start)
                }
            };
            let mut items: Vec<Item> = live.state.items.iter().filter(|i| before_key(i)).cloned().collect();
            let mut posts = 0;
            while idx > 0 && posts < limit {
                idx -= 1;
                let super::engine::Live { state, shards, .. } = live;
                let shard = shards.get(&core.store, &group_id, core.wid, &days[idx], state)?;
                let before = items.len();
                items.extend(shard.visible(state).filter(|i| before_key(i)));
                posts += items.len() - before;
            }
            items.sort_by(|a, b| (a.at, &a.id).cmp(&(b.at, &b.id)));
            (items, idx > 0)
        };
        let start = items.len().saturating_sub(limit);
        let older = (start > 0 || more).then(|| items.get(start).map(|i| format!("{}:{}", i.at, i.id))).flatten();
        let messages = items[start..].iter().filter_map(|i| self.inner.group_message(&core, &group_id, i)).collect();
        Ok(GroupPage { messages, older })
    }

    /// Everything up to now is read (for `unread`).
    /// Signed member receipts of MY posts with `at` in `[from_ms, to_ms]` (a loaded page): the
    /// group tick (`Waiting` one tick, `Delivered` when every eligible member confirmed, `Read`
    /// when every eligible member read) and n/N. Blocking. Re-read after `on_group_changed`.
    pub fn group_receipts(&self, group_id: String, from_ms: u64, to_ms: u64) -> Result<Vec<crate::group::member_receipts::GroupReceiptStatus>, Error> {
        let core = self.inner.group_core()?;
        core.member_receipts_range(&group_id, from_ms as i64, to_ms as i64, true)
    }

    /// One post's member receipts (any author: members see everyone's), for its details.
    /// `at_ms` is the post's `GroupMessage.at`. Blocking.
    pub fn group_message_receipts(&self, group_id: String, message_id: String, at_ms: u64) -> Result<crate::group::member_receipts::GroupReceiptStatus, Error> {
        let core = self.inner.group_core()?;
        let at = at_ms as i64;
        core.member_receipts_range(&group_id, at, at, false)?.into_iter().find(|s| s.message_id == message_id).ok_or(Error::NotFound)
    }

    /// The cursor only moves forward; the step signs read receipts for the posts of others it
    /// covers (`engine::member_receipts`).
    pub fn group_mark_read(&self, group_id: String) -> Result<(), Error> {
        let core = self.inner.group_core()?;
        // The account's read-receipt setting lives with the chat settings. Unknown = off: never
        // send a read confirmation the user may have turned off.
        let reads = self.inner.chat_core().map(|c| c.store.read_receipts()).unwrap_or(false);
        core.mark_read(&group_id, reads)?;
        core.member_receipts_pump_and_push();
        Ok(())
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
            let nonce: [u8; 16] = cryptography::rng::bytes();
            let gid = group_id(&core.did, &nonce);
            {
                core.groups.lock().insert(gid.clone(), super::engine::Live::empty(&gid, core.wid, GroupMeta::default()));
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
            let invite = issue_invite(&core.account, &group_id, now_ms(), INVITE_TTL);
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

    /// The group's permanent open link (public group §2; owner and admins): anyone holding it
    /// may join without an invite while the group is open. Opens the group if it is not open
    /// (`Open{active: true}`) and has every chat host reachable now keep one code with no expiry;
    /// asking again returns the same link while the host it names still hosts. Closing
    /// (`group_set_open(id, false)`) drops it.
    pub fn group_open_link(&self, group_id: String) -> Result<String, Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            inner.group_admin_check(&core, &group_id)?;
            inner.group_ensure(&core).await?;
            inner.group_relay_ready().await;
            if !core.with_state(&group_id, |st| st.open).unwrap_or(false) {
                inner.group_write(&core, &group_id, Body::Open { active: true })?;
            }
            // Only a link made in this open period (under the `Open` record in force now).
            let stored = core.groups.lock().get(&group_id).and_then(|l| l.meta.open_link.clone().filter(|_| l.meta.open_link_record.is_some() && l.meta.open_link_record == l.state.open_record));
            if let Some(link) = &stored
                && inner.group_open_link_live(&core, &group_id, link)
            {
                return Ok(link.clone());
            }
            // A stored link at another host (which may have lost it) is registered again under the
            // same code; otherwise a new one.
            let reuse = stored.as_deref().and_then(super::code::ShortCode::parse).map(|sc| sc.code);
            let link = inner.group_open_code(&core, &group_id, reuse).await?;
            let mut groups = core.groups.lock();
            if let Some(live) = groups.get_mut(&group_id) {
                live.meta.open_link = Some(link.clone());
                live.meta.open_link_record = live.state.open_record;
                core.store.put_meta(&group_id, &live.meta)?;
            }
            Ok(link)
        })?
    }

    /// Opens or closes the group to joins without an invite (owner and admins). Closing refuses
    /// new open joiners at once and drops the open link on the hosts we reach; members stay.
    /// Opening alone makes no link (`group_open_link`).
    pub fn group_set_open(&self, group_id: String, open: bool) -> Result<(), Error> {
        let core = self.inner.group_core()?;
        self.inner.group_admin_check(&core, &group_id)?;
        if core.with_state(&group_id, |st| st.open).unwrap_or(false) != open {
            self.inner.group_write(&core, &group_id, Body::Open { active: open })?;
        }
        if !open {
            let link = {
                let mut groups = core.groups.lock();
                let live = groups.get_mut(&group_id).ok_or(Error::NotFound)?;
                let link = live.meta.open_link.take();
                live.meta.open_link_record = None;
                if link.is_some() {
                    core.store.put_meta(&group_id, &live.meta)?;
                }
                link
            };
            if let Some(link) = link {
                self.inner.group_drop_open_code(&core, &group_id, &link);
            }
        }
        Ok(())
    }

    /// Only the owner and admins may post (a channel), or everyone again (owner and admins).
    pub fn group_set_admins_only(&self, group_id: String, admins_only: bool) -> Result<(), Error> {
        let core = self.inner.group_core()?;
        self.inner.group_admin_check(&core, &group_id)?;
        if core.with_state(&group_id, |st| st.admins_only).unwrap_or(false) != admins_only {
            self.inner.group_write(&core, &group_id, Body::AdminsOnly { active: admins_only })?;
        }
        Ok(())
    }

    /// What an invite link is for, fetched from the host it names (blocks up to about 20 s;
    /// `None` if the link is not one, the host refuses it, or cannot be reached).
    pub fn group_peek_invite(&self, token: String) -> Option<GroupInvitePeek> {
        let sc = super::code::ShortCode::parse(&token)?;
        let inner = self.inner.clone();
        let t = self
            .block_on(async move { inner.group_fetch(&sc).await })
            .ok()?
            .ok()?;
        let inv = t.invite.as_ref().and_then(|i| i.open().ok());
        if t.invite.is_some() && inv.as_ref().is_none_or(|i| i.claim.group != t.group) {
            return None;
        }
        Some(GroupInvitePeek {
            group_id: t.group.clone(),
            name: t.name.clone(),
            owner_did: t.owner.clone(),
            issuer_did: inv.as_ref().map(|i| i.iss.clone()),
            expires: inv.map(|i| i.exp_ms),
            open: t.open && t.invite.is_none(),
            host_count: t
                .hosts
                .iter()
                .filter(|h| h.open().ok().is_some_and(|a| a.group == t.group))
                .count() as u32,
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
        // A channel (public group §1): only the owner and admins post. Others' posts would be
        // hidden on every device anyway (`post_visible`).
        if core.with_state(&group_id, |st| st.admins_only && !st.is_admin(&core.did)).unwrap_or(false) {
            return Err(Error::Rejected("admins only".into()));
        }
        self.inner.group_write(&core, &group_id, Body::Post { text, reply_to })
    }

    /// Why a post of ours will not be posted (`None`: it is not failed). A post written offline
    /// while an admin turned admins-only on is refused by the hosts: this device stops sending
    /// it and keeps it here with the reason ("Not posted: only admins post here"); its
    /// `delivery` stays `Waiting`. A later post of ours on the same day carries it along again.
    pub fn group_post_failure(&self, group_id: String, message_id: String) -> Result<Option<String>, Error> {
        let core = self.inner.group_core()?;
        let groups = core.groups.lock();
        let live = groups.get(&group_id).ok_or(Error::NotFound)?;
        Ok(live.meta.failed_posts.get(&message_id).map(|f| f.reason.clone()))
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

    /// This install's device ID for this account (hex): what an admin appoints as host.
    pub fn group_device_id(&self) -> Result<String, Error> {
        Ok(hex::encode(self.inner.group_core()?.dev))
    }

    /// This device's group endpoint id, once it is up (`None` before any group).
    pub fn group_endpoint_id(&self) -> Option<String> {
        self.inner.group_endpoint().map(|e| e.id().to_string())
    }
}

impl Inner {
    pub(crate) fn group_owes_pub(&self, core: &GroupCore, gid: &str) -> bool {
        let mine = core.groups.lock().get(gid).map(|l| l.doc.vv().get(&core.wid).copied().unwrap_or(0)).unwrap_or(0);
        if mine == 0 || core.am_host(gid) {
            return false;
        }
        !core.store.acks(gid).unwrap_or_default().iter().any(|(_, vv)| loro::VersionVector::decode(vv).ok().and_then(|v| v.get(&core.wid).copied()).unwrap_or(0) >= mine)
    }

    /// Our stored open link names us, a chat host of `gid`, and we still keep its code. A link
    /// at another host is not known to be live (that host may have lost or dropped it): the
    /// caller registers it again.
    fn group_open_link_live(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, link: &str) -> bool {
        let Some(sc) = super::code::ShortCode::parse(link) else { return false };
        let _ = self.group_my_addr(core, gid);
        let names_host = core.chat_host_addrs(gid).iter().filter_map(|h| h.open().ok()).find(|a| a.endpoint == sc.endpoint);
        names_host.is_some_and(|a| a.did == core.did && a.dev == core.dev) && core.store.code(&sc.code).ok().flatten().is_some_and(|r| r.kind == super::code::CodeKind::Open && r.by == core.did)
    }

    fn group_forget(&self, core: &GroupCore, gid: &str) {
        core.groups.lock().remove(gid);
        let _ = core.store.forget(gid);
        self.search_forget(&crate::search::api::group_src(gid));
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
        if !super::valid_gid(&gid) {
            return Err(Error::Rejected("invalid group id".into()));
        }
        if let Some(inv) = &t.invite
            && redeem_invite(inv, &gid, &core.did, now_ms(), &proto::Unrecorded).is_none()
        {
            return Err(Error::Rejected("invite expired or invalid".into()));
        }
        let already = core.groups.lock().get(&gid).map(|l| l.state.is_member(&core.did));
        match already {
            Some(true) => return Ok(gid),
            Some(false) => {}
            None => {
                // An open link carries no invite: we write `OpenJoin` once we hold the group.
                let meta = GroupMeta {
                    joining: t.invite.as_ref().map(|i| cryptography::codec::to_bytes(i).expect("serialises")),
                    open_join: t.open && t.invite.is_none(),
                    ..Default::default()
                };
                core.groups.lock().insert(gid.clone(), super::engine::Live::empty(&gid, core.wid, meta.clone()));
                core.store.put_meta(&gid, &meta)?;
            }
        }
        for h in &t.hosts {
            if let Some(a) = h.open().ok()
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
                (l.state.is_member(&core.did), l.meta.joining.is_none() && !l.meta.open_join)
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
