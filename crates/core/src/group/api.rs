//! The UniFFI surface of group chat (G1). Additive to `Node`. Conventions as in chat: times are
//! unix ms, DIDs name people, `group_id` names a group; blocking methods are for the app's IO
//! threads. A locked node fails with `Locked`.
//!
//! Roles: the owner (who created the group) is an admin; admins are made by the owner; any
//! admin may flag (3 flags remove, U9/U15); hosts are member devices appointed by an admin that
//! accepted hosting (`group_accept_host`). Nothing here exposes another member's endpoint.

use std::sync::Arc;

use super::engine::INVITE_TTL;
use super::record::*;
use super::state::Item;
use super::store::GroupMeta;
use crate::Error;
use crate::chat::doc::day_of;
use crate::node::{Node, now_ms};

mod hooks;
mod hosts;
mod images;
mod presence;
mod rotation;
mod types;
pub use types::*;

#[uniffi::export]
impl Node {
    pub fn set_group_events(&self, events: Arc<dyn GroupEvents>) {
        self.inner.group.set_group_sink(Arc::new(GroupEventsAdapter(events)));
    }

    /// Groups this account is in (or was removed from / left), most recent activity first.
    pub fn groups(&self) -> Result<Vec<GroupSummary>, Error> {
        let core = self.inner.group.group_core()?;
        let ids: Vec<String> = core.groups.lock().keys().cloned().collect();
        let mut out: Vec<GroupSummary> = ids.iter().filter_map(|g| self.inner.group.group_summary(&core, g)).collect();
        out.sort_by(|a, b| b.last.as_ref().map_or(0, |m| m.at).cmp(&a.last.as_ref().map_or(0, |m| m.at)));
        Ok(out)
    }

    pub fn group_detail(&self, group_id: String) -> Result<GroupDetail, Error> {
        let core = self.inner.group.group_core()?;
        Ok(self.inner.group.group_detail(&core, &group_id)?)
    }

    /// Up to `limit` messages and system lines older than `before` (a cursor from an earlier
    /// page; `None` = the newest), oldest first. Pages across day docs backwards, loading only
    /// the days it needs; `older` is `None` once every day we hold has been shown (history
    /// still arriving in the background shows up on the next call).
    pub fn group_messages(&self, group_id: String, before: Option<String>, limit: u32) -> Result<GroupPage, Error> {
        let core = self.inner.group.group_core()?;
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
        let messages = items[start..].iter().filter_map(|i| self.inner.group.group_message(&core, &group_id, i)).collect();
        Ok(GroupPage { messages, older })
    }

    /// Everything up to now is read (for `unread`).
    /// Signed member receipts of MY posts with `at` in `[from_ms, to_ms]` (a loaded page): the
    /// group tick (`Waiting` one tick, `Delivered` when every eligible member confirmed, `Read`
    /// when every eligible member read) and n/N. Blocking. Re-read after `on_group_changed`.
    pub fn group_receipts(&self, group_id: String, from_ms: u64, to_ms: u64) -> Result<Vec<crate::group::member_receipts::GroupReceiptStatus>, Error> {
        let core = self.inner.group.group_core()?;
        Ok(core.member_receipts_range(&group_id, from_ms as i64, to_ms as i64, true)?)
    }

    /// One post's member receipts (any author: members see everyone's), for its details.
    /// `at_ms` is the post's `GroupMessage.at`. Blocking.
    pub fn group_message_receipts(&self, group_id: String, message_id: String, at_ms: u64) -> Result<crate::group::member_receipts::GroupReceiptStatus, Error> {
        let core = self.inner.group.group_core()?;
        let at = at_ms as i64;
        core.member_receipts_range(&group_id, at, at, false)?.into_iter().find(|s| s.message_id == message_id).ok_or(Error::NotFound)
    }

    /// The cursor only moves forward; the step signs read receipts for the posts of others it
    /// covers (`engine::member_receipts`).
    pub fn group_mark_read(&self, group_id: String) -> Result<(), Error> {
        let core = self.inner.group.group_core()?;
        // The account's read-receipt setting lives with the chat settings. Unknown = off: never
        // send a read confirmation the user may have turned off.
        let reads = self.inner.chat.chat_core().map(|c| c.store.read_receipts()).unwrap_or(false);
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
            let core = inner.group.group_core()?;
            inner.group.group_ensure(&core).await?;
            let nonce: [u8; 16] = cryptography::rng::bytes();
            let gid = group_id(&core.did, &nonce);
            {
                core.groups.lock().insert(gid.clone(), super::engine::Live::empty(&gid, core.wid, GroupMeta::default()));
                core.store.put_meta(&gid, &GroupMeta::default())?;
            }
            let owner_name = inner.me()?.profile.name.clone();
            inner.group.group_write(&core, &gid, Body::Genesis { name, nonce, owner_name })?;
            inner.group.group_write(&core, &gid, Body::HostPermit { host: core.did.clone(), host_dev: core.dev, active: true })?;
            inner.group.group_write(&core, &gid, Body::HostAccept { active: true })?;
            Ok(gid)
        })?
    }

    /// A short invite link (`tinline://g/...`, about 77 characters; for a card, a share sheet or a
    /// QR) for an invite signed by us; admins only. Valid 7 days. A host keeps the invite behind
    /// the link's code (we, if we host; else a host we are connected to, which can take up to 30
    /// s to reach). See `code.rs`.
    pub fn group_invite(&self, group_id: String) -> Result<String, Error> {
        self.run(|inner| async move {
            let core = inner.group.admin_core(&group_id)?;
            inner.group.group_ensure(&core).await?;
            inner.group.group_relay_ready().await;
            let invite = issue_invite(&core.account, &group_id, now_ms(), INVITE_TTL);
            inner.group.group_issue_code(&core, &group_id, Some(invite)).await
        })
    }

    /// A short link for our own other devices (same DID): it admits only this account. Valid 7
    /// days.
    pub fn group_device_token(&self, group_id: String) -> Result<String, Error> {
        self.run(|inner| async move {
            let core = inner.group.member_core(&group_id)?;
            inner.group.group_ensure(&core).await?;
            inner.group.group_relay_ready().await;
            inner.group.group_issue_code(&core, &group_id, None).await
        })
    }

    /// The group's permanent open link (public group §2; owner and admins): anyone holding it
    /// may join without an invite while the group is open. Opens the group if it is not open
    /// (`Open{active: true}`) and has every chat host reachable now keep one code with no expiry;
    /// asking again returns the same link while the host it names still hosts. Closing
    /// (`group_set_open(id, false)`) drops it.
    pub fn group_open_link(&self, group_id: String) -> Result<String, Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group.admin_core(&group_id)?;
            inner.group.group_ensure(&core).await?;
            inner.group.group_relay_ready().await;
            if !core.with_state(&group_id, |st| st.open).unwrap_or(false) {
                inner.group.group_write(&core, &group_id, Body::Open { active: true })?;
            }
            // Only a link made in this open period (under the `Open` record in force now).
            let stored = core.groups.lock().get(&group_id).and_then(|l| l.meta.open_link.clone().filter(|_| l.meta.open_link_record.is_some() && l.meta.open_link_record == l.state.open_record));
            if let Some(link) = &stored
                && inner.group.group_open_link_live(&core, &group_id, link)
            {
                return Ok(link.clone());
            }
            // A stored link at another host (which may have lost it) is registered again under the
            // same code; otherwise a new one.
            let reuse = stored.as_deref().and_then(super::code::ShortCode::parse).map(|sc| sc.code);
            let link = inner.group.group_open_code(&core, &group_id, reuse).await?;
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
        let core = self.inner.group.admin_core(&group_id)?;
        if core.with_state(&group_id, |st| st.open).unwrap_or(false) != open {
            self.inner.group.group_write(&core, &group_id, Body::Open { active: open })?;
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
                self.inner.group.group_drop_open_code(&core, &group_id, &link);
            }
        }
        Ok(())
    }

    /// Only the owner and admins may post (a channel), or everyone again (owner and admins).
    pub fn group_set_admins_only(&self, group_id: String, admins_only: bool) -> Result<(), Error> {
        let core = self.inner.group.admin_core(&group_id)?;
        if core.with_state(&group_id, |st| st.admins_only).unwrap_or(false) != admins_only {
            self.inner.group.group_write(&core, &group_id, Body::AdminsOnly { active: admins_only })?;
        }
        Ok(())
    }

    /// What an invite link is for, fetched from the host it names (blocks up to about 20 s;
    /// `None` if the link is not one, the host refuses it, or cannot be reached).
    pub fn group_peek_invite(&self, token: String) -> Option<GroupInvitePeek> {
        let sc = super::code::ShortCode::parse(&token)?;
        let inner = self.inner.clone();
        let t = self
            .block_on(async move { inner.group.group_fetch(&sc).await })
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
        self.run(|inner| async move {
            let t = inner.group.group_fetch(&sc).await?;
            inner.group.group_join_token(t).await
        })
    }

    pub fn group_post(&self, group_id: String, text: String, reply_to: Option<String>) -> Result<GroupMessage, Error> {
        if text.is_empty() || text.len() > MAX_TEXT {
            return Err(Error::Protocol("message must be 1-20000 bytes".into()));
        }
        let core = self.inner.group.member_core(&group_id)?;
        // A channel (public group §1): only the owner and admins post. Others' posts would be
        // hidden on every device anyway (`post_visible`).
        if core.with_state(&group_id, |st| st.admins_only && !st.is_admin(&core.did)).unwrap_or(false) {
            return Err(Error::Rejected("admins only".into()));
        }
        Ok(self.inner.group.group_write(&core, &group_id, Body::Post { text, reply_to, image: None })?)
    }

    /// Why a post of ours will not be posted (`None`: it is not failed). A post written offline
    /// while an admin turned admins-only on is refused by the hosts: this device stops sending
    /// it and keeps it here with the reason ("Not posted: only admins post here"); its
    /// `delivery` is `Failed` while it is in this list. A later post of ours on the same day carries it along again.
    pub fn group_post_failure(&self, group_id: String, message_id: String) -> Result<Option<String>, Error> {
        let core = self.inner.group.group_core()?;
        let groups = core.groups.lock();
        let live = groups.get(&group_id).ok_or(Error::NotFound)?;
        Ok(live.meta.failed_posts.get(&message_id).map(|f| f.reason.clone()))
    }

    /// One admin flag on `member_did` (U9/U15: any 3 remove; final, visible to them).
    pub fn group_flag(&self, group_id: String, member_did: String) -> Result<(), Error> {
        let core = self.inner.group.admin_core(&group_id)?;
        let ok = core.groups.lock().get(&group_id).is_some_and(|l| l.state.is_member(&member_did) && l.state.owner.as_deref() != Some(member_did.as_str()));
        if !ok {
            return Err(Error::Rejected("not a member that can be flagged".into()));
        }
        Ok(self.inner.group.group_write(&core, &group_id, Body::Flag { member: member_did }).map(|_| ())?)
    }

    /// Owner only: make `member_did` an admin, or not.
    pub fn group_set_admin(&self, group_id: String, member_did: String, admin: bool) -> Result<(), Error> {
        let core = self.inner.group.member_core(&group_id)?;
        let ok = core.groups.lock().get(&group_id).is_some_and(|l| l.state.owner.as_deref() == Some(core.did.as_str()) && l.state.is_member(&member_did));
        if !ok {
            return Err(Error::Rejected("only the owner can change admins of members".into()));
        }
        Ok(self.inner.group.group_write(&core, &group_id, Body::Admin { member: member_did, admin }).map(|_| ())?)
    }

    /// Leaves the group (PROVISIONAL: the owner cannot leave).
    pub fn group_leave(&self, group_id: String) -> Result<(), Error> {
        let core = self.inner.group.member_core(&group_id)?;
        if core.groups.lock().get(&group_id).is_some_and(|l| l.state.owner.as_deref() == Some(core.did.as_str())) {
            return Err(Error::Rejected("the owner cannot leave".into()));
        }
        Ok(self.inner.group.group_write(&core, &group_id, Body::Leave).map(|_| ())?)
    }

    /// This install's device ID for this account (hex): what an admin appoints as host.
    pub fn group_device_id(&self) -> Result<String, Error> {
        Ok(hex::encode(self.inner.group.group_core()?.dev))
    }

    /// This device's group endpoint id, once it is up (`None` before any group).
    pub fn group_endpoint_id(&self) -> Option<String> {
        self.inner.group.group_endpoint().map(|e| e.id().to_string())
    }
}
