//! The engine side of the group API: the records the app sees, built from group state
//! (`group_message`, `group_summary`, `group_detail`), the writes behind `Node::group_*`, and
//! joining with a token. p2pcore's `group::api` declares the records to UniFFI.

use std::sync::Arc;
use std::time::Duration;

use super::engine::{Changed, GroupCore, proof_removes};
use super::record::*;
use super::shard::brief_item;
use super::state::{Item, ItemKind};
use super::store::GroupMeta;
use crate::Error;
use chat_core::doc::day_of;
use crate::GroupRt;
use node_runtime::{OrLog, now_ms};

mod images;
mod types;
pub use types::*;

pub fn kind_of(k: &ItemKind) -> GroupItemKind {
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

pub fn parse_dev(hexs: &str) -> Result<[u8; 32], Error> {
    hex::decode(hexs).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| Error::Protocol("bad device id".into()))
}

impl GroupRt {
    pub fn group_name_of(&self, core: &GroupCore, gid: &str, did: &str) -> String {
        if did == core.did {
            return self.me().map(|m| m.profile.name.clone()).unwrap_or_default();
        }
        if let Some(n) = node_runtime::block::known_name(&self.shared.lock().state, did) {
            return n;
        }
        core.groups.lock().get(gid).and_then(|l| l.state.members.get(did).map(|m| m.name.clone())).unwrap_or_default()
    }

    pub fn group_role(core: &GroupCore, live: &super::engine::Live) -> GroupRole {
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

    pub fn group_message(&self, core: &GroupCore, gid: &str, i: &Item) -> Option<GroupMessage> {
        let hosting = core.with_state(gid, |st| st.is_host(&core.did, &core.dev))?;
        let outgoing = i.author == core.did;
        let failure = if i.peer == core.wid && i.kind == ItemKind::Post {
            core.groups.lock().get(gid).and_then(|l| l.meta.failed_posts.get(&i.id).map(|f| f.reason.clone()))
        } else {
            None
        };
        let delivery = if failure.is_some() {
            GroupDelivery::Failed
        } else if i.peer != core.wid || hosting {
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
            failure,
            before_removal: i.before_removal,
            image: i.image.as_ref().map(|img| GroupImage {
                width: img.w,
                height: img.h,
                size: img.size,
                mime: img.mime.clone(),
                preview: img.preview.clone(),
                state: match core.image_state(gid, img) {
                    super::engine::images::ImgState::Ready => GroupImageState::Ready,
                    super::engine::images::ImgState::Loading => GroupImageState::Loading,
                    super::engine::images::ImgState::Remote => GroupImageState::Remote,
                    super::engine::images::ImgState::Failed => GroupImageState::Failed,
                },
            }),
        })
    }

    pub fn group_summary(&self, core: &GroupCore, gid: &str) -> Option<GroupSummary> {
        let (name, role, members, hosts, here, last, unread, reachable, open, admins_only) = {
            let groups = core.groups.lock();
            let live = groups.get(gid)?;
            live.state.owner.as_ref()?;
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

    pub fn group_detail(&self, core: &GroupCore, gid: &str) -> Result<GroupDetail, Error> {
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
    pub fn group_write(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, body: Body) -> Result<GroupMessage, Error> {
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
            image: None,
            before_removal: false,
            peer: entry.peer,
            counter: entry.counter,
        });
        self.group_message(core, gid, &item).ok_or(Error::NotFound)
    }

    /// The state check every action makes first: we are a current member (not removed, not left).
    pub fn group_member_check(&self, core: &GroupCore, gid: &str) -> Result<(), Error> {
        let groups = core.groups.lock();
        let live = groups.get(gid).ok_or(Error::NotFound)?;
        let ignore = self.ignore_removal.load(std::sync::atomic::Ordering::SeqCst);
        if !ignore && (live.state.is_removed(&core.did) || !live.meta.removal_proof.is_empty()) {
            return Err(Error::Rejected("removed".into()));
        }
        if !live.state.is_member(&core.did) && !(ignore && live.state.is_removed(&core.did)) {
            return Err(Error::Rejected("not a member".into()));
        }
        Ok(())
    }

    pub fn group_admin_check(&self, core: &GroupCore, gid: &str) -> Result<(), Error> {
        self.group_member_check(core, gid)?;
        if !core.groups.lock().get(gid).is_some_and(|l| l.state.is_admin(&core.did)) {
            return Err(Error::Rejected("only admins can do that".into()));
        }
        Ok(())
    }

    /// The full token for `gid`: our signed invite (admins) or none (own devices), and the hosts.
    /// G3: it no longer travels as text; a host keeps it behind a short code (`code.rs`), and
    /// direct invitations carry it over the private channel (`invitations.rs`).
    pub fn group_token(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, invite: Option<SignedInvite>) -> Result<InviteToken, Error> {
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

impl GroupRt {
    pub fn group_owes_pub(&self, core: &GroupCore, gid: &str) -> bool {
        let mine = core.groups.lock().get(gid).map(|l| l.doc.vv().get(&core.wid).copied().unwrap_or(0)).unwrap_or(0);
        if mine == 0 || core.am_host(gid) {
            return false;
        }
        !core.store.acks(gid).or_log_default("reading acks").iter().any(|(_, vv)| loro::VersionVector::decode(vv).ok().and_then(|v| v.get(&core.wid).copied()).unwrap_or(0) >= mine)
    }

    /// Our stored open link names us, a chat host of `gid`, and we still keep its code. A link
    /// at another host is not known to be live (that host may have lost or dropped it): the
    /// caller registers it again.
    pub fn group_open_link_live(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, link: &str) -> bool {
        let Some(sc) = super::code::ShortCode::parse(link) else { return false };
        let _ = self.group_my_addr(core, gid);
        let names_host = core.chat_host_addrs(gid).iter().filter_map(|h| h.open().ok()).find(|a| a.endpoint == sc.endpoint);
        names_host.is_some_and(|a| a.did == core.did && a.dev == core.dev) && core.store.code(&sc.code).ok().flatten().is_some_and(|r| r.kind == super::code::CodeKind::Open && r.by == core.did)
    }

    fn group_forget(&self, core: &GroupCore, gid: &str) {
        core.groups.lock().remove(gid);
        let _ = core.store.forget(gid);
        self.search.forget(&search_ix::group_src(gid));
    }
}

/// While a join waits: how often failed dials to the group's hosts are tried again.
const JOIN_REDIAL: Duration = Duration::from_secs(2);

impl GroupRt {
    /// Joins with a full (fetched or delivered) token: the same checks whatever brought it.
    pub async fn group_join_token(self: &Arc<Self>, t: InviteToken) -> Result<String, Error> {
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
                    joining: t.invite.as_ref().map(cryptography::codec::to_bytes_infallible),
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
