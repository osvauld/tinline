//! The group flows on a stand-in that follows the core's rules (docs/group-core.md): owner and
//! admins, 3 flags remove (U9/U15), removed members can't post and see the signed flags, our
//! posts wait for a host, invites only from admins.

use std::cell::RefCell;
use std::collections::BTreeMap;

use p2pcore::{GroupFlag, GroupHost, GroupRemoval};

use super::*;

const ME: &str = "did:key:me";
const DEV: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Clone, Default)]
struct G {
    name: String,
    owner: String,
    /// did -> (name, admin, devices)
    members: BTreeMap<String, (String, bool, Vec<String>)>,
    flags: BTreeMap<String, Vec<String>>,
    removed: bool,
    left: bool,
    /// (did, dev)
    hosts: Vec<(String, String)>,
    /// A host has a live session with us.
    online: bool,
    msgs: Vec<GroupMessage>,
    read: usize,
    /// Public group settings (§1) and who the hosts say is online (§3): (did, name, admin).
    open: bool,
    admins_only: bool,
    present: Vec<(String, String, bool)>,
    open_link: Option<String>,
}

#[derive(Default)]
struct Fake {
    g: RefCell<BTreeMap<String, G>>,
    n: RefCell<u64>,
    /// Tokens group_join accepts, and what it answers.
    tokens: RefCell<BTreeMap<String, Result<String, String>>>,
    marked: RefCell<Vec<String>>,
    /// Chat requests (both directions) and who we block / have as contacts.
    reqs: RefCell<Vec<GroupChatRequest>>,
    blocked: RefCell<Vec<String>>,
    contacts: RefCell<Vec<String>>,
    /// (group, admin who appointed) for "host this group?" on this device.
    asked: RefCell<Option<(String, String)>>,
    /// Invitations, both directions (G3).
    invs: RefCell<Vec<GroupInvitation>>,
    /// "Show me as online" is off on this install.
    hidden: RefCell<bool>,
}

fn msg(gid: &str, id: &str, at: u64, author: &str, name: &str, kind: GroupItemKind, text: &str) -> GroupMessage {
    GroupMessage {
        id: id.into(),
        group_id: gid.into(),
        kind,
        author_did: author.into(),
        author_name: name.into(),
        subject_did: author.into(),
        subject_name: name.into(),
        at,
        text: text.into(),
        reply_to: None,
        outgoing: author == ME,
        delivery: GroupDelivery::Delivered,
        before_removal: false,
        image: None,
    }
}

impl Fake {
    fn tick(&self) -> u64 {
        let mut n = self.n.borrow_mut();
        *n += 1;
        1000 + *n
    }

    /// A group owned by `owner`, with us and `others` as members.
    fn add(&self, gid: &str, owner: &str, admins: &[&str], others: &[&str]) {
        let mut g = G { name: format!("Group {gid}"), owner: owner.into(), online: true, ..Default::default() };
        for d in std::iter::once(owner).chain(std::iter::once(ME)).chain(others.iter().copied()) {
            let admin = d == owner || admins.contains(&d);
            g.members.insert(d.into(), (d.trim_start_matches("did:key:").to_string(), admin, vec![format!("{}dev", d.len())]));
        }
        g.members.get_mut(ME).unwrap().2 = vec![DEV.into()];
        g.hosts.push((owner.into(), format!("{}dev", owner.len())));
        self.g.borrow_mut().insert(gid.into(), g);
    }

    fn incoming(&self, gid: &str, from: &str, text: &str) -> GroupMessage {
        let at = self.tick();
        let m = msg(gid, &format!("m{at}"), at, from, from.trim_start_matches("did:key:"), GroupItemKind::Post, text);
        self.g.borrow_mut().get_mut(gid).unwrap().msgs.push(m.clone());
        m
    }

    fn role(g: &G) -> GroupRole {
        if g.removed {
            GroupRole::Removed
        } else if g.left {
            GroupRole::Left
        } else if g.owner == ME {
            GroupRole::Owner
        } else if g.members.get(ME).is_some_and(|m| m.1) {
            GroupRole::Admin
        } else {
            GroupRole::Member
        }
    }

    fn check_admin(g: &G) -> Result<(), Error> {
        if !matches!(Self::role(g), GroupRole::Owner | GroupRole::Admin) {
            return Err(Error::Rejected("only admins can do that".into()));
        }
        Ok(())
    }

    fn with<T>(&self, gid: &str, f: impl FnOnce(&mut G) -> Result<T, Error>) -> Result<T, Error> {
        let mut gs = self.g.borrow_mut();
        f(gs.get_mut(gid).ok_or(Error::NotFound)?)
    }
}

impl GroupApi for Fake {
    fn groups(&self) -> Result<Vec<GroupSummary>, Error> {
        Ok(self
            .g
            .borrow()
            .iter()
            .map(|(id, g)| GroupSummary {
                id: id.clone(),
                name: g.name.clone(),
                my_role: Fake::role(g),
                member_count: g.members.len() as u32,
                host_count: g.hosts.len() as u32,
                hosted_here: g.hosts.iter().any(|h| h.1 == DEV),
                host_reachable: g.online,
                last: g.msgs.last().cloned(),
                unread: g.msgs[g.read..].iter().filter(|m| !m.outgoing && m.kind == GroupItemKind::Post).count() as u32,
                open: g.open,
                admins_only: g.admins_only,
            })
            .collect())
    }
    fn group_detail(&self, id: String) -> Result<GroupDetail, Error> {
        let gs = self.g.borrow();
        let g = gs.get(&id).ok_or(Error::NotFound)?;
        Ok(GroupDetail {
            id: id.clone(),
            name: g.name.clone(),
            owner_did: g.owner.clone(),
            my_role: Fake::role(g),
            am_host: g.hosts.iter().any(|h| h.1 == DEV),
            members: g
                .members
                .iter()
                .map(|(did, (name, admin, devs))| GroupMember {
                    did: did.clone(),
                    name: name.clone(),
                    is_me: did == ME,
                    is_owner: *did == g.owner,
                    is_admin: *admin,
                    is_host: g.hosts.iter().any(|h| &h.0 == did),
                    flags: g.flags.get(did).map_or(0, |f| f.len() as u32),
                    joined_at: 1,
                    invited_by: None,
                    device_ids: devs.clone(),
                })
                .collect(),
            hosts: g
                .hosts
                .iter()
                .map(|(did, dev)| GroupHost {
                    did: did.clone(),
                    name: did.clone(),
                    device_id: dev.clone(),
                    appointed_by: Some(g.owner.clone()),
                    is_me: dev == DEV,
                    online: g.online || dev == DEV,
                })
                .collect(),
            host_offers: Vec::new(),
            host_requests: self
                .asked
                .borrow()
                .iter()
                .filter(|(gid, _)| *gid == id)
                .map(|(_, by)| GroupHost { did: ME.into(), name: "Me".into(), device_id: DEV.into(), appointed_by: Some(by.clone()), is_me: true, online: true })
                .collect(),
            host_requested_here: self.asked.borrow().as_ref().is_some_and(|(gid, _)| *gid == id),
            removal: g.removed.then(|| GroupRemoval {
                at: 5,
                flags: (1..=3).map(|i| GroupFlag { by_did: format!("did:key:a{i}"), by_name: format!("A{i}"), at: i }).collect(),
            }),
            am_call_host: false,
            call_hosts: Vec::new(),
            call_host_requests: Vec::new(),
            call_host_requested_here: false,
            open: g.open,
            admins_only: g.admins_only,
        })
    }
    fn group_messages(&self, id: String, before: Option<String>, limit: u32) -> Result<GroupPage, Error> {
        let gs = self.g.borrow();
        let g = gs.get(&id).ok_or(Error::NotFound)?;
        let end = match before {
            None => g.msgs.len(),
            Some(c) => g.msgs.iter().position(|m| m.id == c).unwrap_or(0),
        };
        let start = end.saturating_sub(limit as usize);
        Ok(GroupPage { messages: g.msgs[start..end].to_vec(), older: (start > 0).then(|| g.msgs[start].id.clone()) })
    }
    fn group_mark_read(&self, id: String) -> Result<(), Error> {
        self.marked.borrow_mut().push(id.clone());
        self.with(&id, |g| {
            g.read = g.msgs.len();
            Ok(())
        })
    }
    fn group_create(&self, name: String) -> Result<String, Error> {
        let gid = format!("g{}", self.tick());
        self.add(&gid, ME, &[], &[]);
        self.with(&gid, |g| {
            g.name = name;
            g.hosts = vec![(ME.into(), DEV.into())];
            Ok(())
        })?;
        Ok(gid)
    }
    fn group_invite(&self, id: String) -> Result<String, Error> {
        self.with(&id, |g| Fake::check_admin(g).map(|_| format!("tinline://g/{id}")))
    }
    fn group_peek_invite(&self, token: String) -> Option<GroupInvitePeek> {
        self.tokens.borrow().contains_key(&token).then(|| GroupInvitePeek {
            group_id: "joined".into(),
            name: "Hike club".into(),
            owner_did: "did:key:own".into(),
            issuer_did: Some("did:key:own".into()),
            expires: Some(7 * 86_400_000),
            host_count: 1,
            open: false,
        })
    }
    fn group_join(&self, token: String) -> Result<String, Error> {
        let r = self.tokens.borrow().get(&token).cloned().ok_or_else(|| Error::Protocol("not a group invite".into()))?;
        match r {
            Ok(gid) => {
                self.add(&gid, "did:key:own", &[], &[]);
                Ok(gid)
            }
            Err(why) => Err(Error::Rejected(why)),
        }
    }
    fn group_post(&self, id: String, text: String, reply_to: Option<String>) -> Result<GroupMessage, Error> {
        let at = self.tick();
        self.with(&id, |g| {
            match Fake::role(g) {
                GroupRole::Removed => return Err(Error::Rejected("removed".into())),
                GroupRole::Left => return Err(Error::Rejected("not a member".into())),
                _ => {}
            }
            if g.admins_only && Fake::check_admin(g).is_err() {
                return Err(Error::Rejected("admins only".into()));
            }
            let mut m = msg(&id, &format!("m{at}"), at, ME, "Me", GroupItemKind::Post, &text);
            m.reply_to = reply_to;
            if !g.online && !g.hosts.iter().any(|h| h.1 == DEV) {
                m.delivery = GroupDelivery::Waiting;
            }
            g.msgs.push(m.clone());
            Ok(m)
        })
    }
    fn group_flag(&self, id: String, did: String) -> Result<(), Error> {
        self.with(&id, |g| {
            Fake::check_admin(g)?;
            if did == g.owner || !g.members.contains_key(&did) {
                return Err(Error::Rejected("not a member that can be flagged".into()));
            }
            let f = g.flags.entry(did.clone()).or_default();
            f.push(ME.into());
            if f.len() >= 3 {
                g.flags.remove(&did);
                g.members.remove(&did);
            }
            Ok(())
        })
    }
    fn group_set_admin(&self, id: String, did: String, admin: bool) -> Result<(), Error> {
        self.with(&id, |g| {
            if g.owner != ME {
                return Err(Error::Rejected("only the owner can change admins of members".into()));
            }
            g.members.get_mut(&did).ok_or(Error::NotFound)?.1 = admin;
            Ok(())
        })
    }
    fn group_leave(&self, id: String) -> Result<(), Error> {
        self.with(&id, |g| {
            if g.owner == ME {
                return Err(Error::Rejected("the owner cannot leave".into()));
            }
            g.left = true;
            Ok(())
        })
    }
    fn group_appoint_host(&self, id: String, did: String, device_id: String) -> Result<(), Error> {
        self.with(&id, |g| {
            Fake::check_admin(g)?;
            g.hosts.push((did, device_id));
            Ok(())
        })
    }
    fn group_revoke_host(&self, id: String, did: String, device_id: String) -> Result<(), Error> {
        self.with(&id, |g| {
            Fake::check_admin(g)?;
            g.hosts.retain(|h| !(h.0 == did && h.1 == device_id));
            Ok(())
        })
    }
    fn group_accept_host(&self, id: String, accept: bool) -> Result<(), Error> {
        self.asked.borrow_mut().take();
        self.with(&id, |g| {
            if accept {
                g.hosts.push((ME.into(), DEV.into()));
            } else {
                g.hosts.retain(|h| h.1 != DEV);
            }
            Ok(())
        })
    }
    fn group_device_id(&self) -> Result<String, Error> {
        Ok(DEV.into())
    }
    fn group_chat_request(&self, id: String, did: String) -> Result<GroupChatRequest, Error> {
        if self.blocked.borrow().contains(&did) {
            return Err(Error::Rejected("you block them".into()));
        }
        if self.contacts.borrow().contains(&did) {
            return Err(Error::Rejected("already a contact".into()));
        }
        let name = self.with(&id, |g| g.members.get(&did).map(|m| m.0.clone()).ok_or(Error::Rejected("not a member of this group".into())))?;
        let r = GroupChatRequest {
            id: format!("r{}", self.tick()),
            group_id: id.clone(),
            group_name: format!("Group {id}"),
            peer_did: did,
            peer_name: name,
            outgoing: true,
            state: ChatRequestState::Pending,
            at: 1,
            contact_added: false,
        };
        self.reqs.borrow_mut().push(r.clone());
        Ok(r)
    }
    fn group_chat_requests_incoming(&self) -> Result<Vec<GroupChatRequest>, Error> {
        Ok(self.reqs.borrow().iter().filter(|r| !r.outgoing && r.state != ChatRequestState::Blocked).cloned().collect())
    }
    fn group_chat_requests_outgoing(&self) -> Result<Vec<GroupChatRequest>, Error> {
        Ok(self.reqs.borrow().iter().filter(|r| r.outgoing).cloned().collect())
    }
    fn group_chat_accept(&self, request_id: String) -> Result<GroupChatRequest, Error> {
        let mut rs = self.reqs.borrow_mut();
        let r = rs.iter_mut().find(|r| r.id == request_id && !r.outgoing).ok_or(Error::NotFound)?;
        r.state = ChatRequestState::Accepted;
        self.contacts.borrow_mut().push(r.peer_did.clone());
        Ok(r.clone())
    }
    fn group_chat_decline(&self, request_id: String) -> Result<GroupChatRequest, Error> {
        let mut rs = self.reqs.borrow_mut();
        let r = rs.iter_mut().find(|r| r.id == request_id && !r.outgoing).ok_or(Error::NotFound)?;
        r.state = ChatRequestState::Declined;
        Ok(r.clone())
    }
    fn block(&self, did: String) -> Result<(), Error> {
        self.blocked.borrow_mut().push(did);
        Ok(())
    }
    fn group_invite_contacts(&self, id: String, dids: Vec<String>) -> Result<Vec<GroupInvitation>, Error> {
        self.with(&id, |g| Fake::check_admin(g))?;
        if dids.iter().any(|d| self.blocked.borrow().contains(d)) {
            return Err(Error::Rejected("you block them".into()));
        }
        let mut out = Vec::new();
        for d in dids {
            let i = inv(&format!("i{}", self.tick()), &id, true, &d, GroupInvitationState::Pending);
            self.invs.borrow_mut().push(i.clone());
            out.push(i);
        }
        Ok(out)
    }
    fn group_invitations_incoming(&self) -> Result<Vec<GroupInvitation>, Error> {
        Ok(self.invs.borrow().iter().filter(|i| !i.outgoing).cloned().collect())
    }
    fn group_invitations_outgoing(&self, id: String) -> Result<Vec<GroupInvitation>, Error> {
        Ok(self.invs.borrow().iter().filter(|i| i.outgoing && i.group_id == id).cloned().collect())
    }
    fn group_invitation_accept(&self, id: String) -> Result<GroupInvitation, Error> {
        let i = self.answer_inv(&id, GroupInvitationState::Joined)?;
        self.add(&i.group_id, &i.inviter_did, &[], &[]);
        Ok(i)
    }
    fn group_invitation_decline(&self, id: String) -> Result<GroupInvitation, Error> {
        self.answer_inv(&id, GroupInvitationState::Declined)
    }
    fn group_open_link(&self, id: String) -> Result<String, Error> {
        self.with(&id, |g| {
            Fake::check_admin(g)?;
            if !g.online && !g.hosts.iter().any(|h| h.1 == DEV) {
                return Err(Error::Timeout);
            }
            g.open = true;
            Ok(g.open_link.get_or_insert_with(|| format!("tinline://g/open-{id}")).clone())
        })
    }
    fn group_set_open(&self, id: String, open: bool) -> Result<(), Error> {
        self.with(&id, |g| {
            Fake::check_admin(g)?;
            g.open = open;
            if !open {
                g.open_link = None;
            }
            Ok(())
        })
    }
    fn group_set_admins_only(&self, id: String, admins_only: bool) -> Result<(), Error> {
        self.with(&id, |g| {
            Fake::check_admin(g)?;
            g.admins_only = admins_only;
            Ok(())
        })
    }
    fn group_online(&self, id: String) -> Result<Vec<OnlineMember>, Error> {
        let gs = self.g.borrow();
        let g = gs.get(&id).ok_or(Error::NotFound)?;
        if !g.open {
            return Ok(Vec::new());
        }
        Ok(g.present.iter().map(|(did, name, is_admin)| OnlineMember { did: did.clone(), name: name.clone(), is_admin: *is_admin }).collect())
    }
    fn group_presence_visible(&self) -> Result<bool, Error> {
        Ok(!*self.hidden.borrow())
    }
}

impl Fake {
    fn answer_inv(&self, id: &str, st: GroupInvitationState) -> Result<GroupInvitation, Error> {
        let mut v = self.invs.borrow_mut();
        let i = v.iter_mut().find(|i| i.id == id).ok_or(Error::NotFound)?;
        if i.state != GroupInvitationState::Pending {
            return Err(Error::Rejected("already answered".into()));
        }
        i.state = st;
        Ok(i.clone())
    }
}

fn inv(id: &str, gid: &str, outgoing: bool, other: &str, state: GroupInvitationState) -> GroupInvitation {
    let name = other.trim_start_matches("did:key:").to_string();
    GroupInvitation {
        id: id.into(),
        group_id: gid.into(),
        group_name: format!("Group {gid}"),
        inviter_did: if outgoing { ME.into() } else { other.into() },
        inviter_name: if outgoing { "Me".into() } else { name.clone() },
        invitee_did: if outgoing { other.into() } else { ME.into() },
        invitee_name: if outgoing { name } else { "Me".into() },
        outgoing,
        state,
        at: 1,
        expires: 7 * 86_400_000,
        delivered: true,
    }
}

fn live() -> GroupsLive {
    let mut l = GroupsLive::default();
    l.reset(ME.into(), String::new());
    l
}

/// Runs a step and, like the app, its job, until nothing is left to do. Returns the last notice.
fn run(l: &mut GroupsLive, api: &Fake, m: Lm) -> Option<String> {
    let mut out = l.step(api, m);
    let mut said = out.notice.take();
    while let Some(job) = out.job.take() {
        out = l.step(api, job.run(api));
        if out.notice.is_some() {
            said = out.notice.take();
        }
    }
    said
}

#[test]
fn the_list_shows_last_message_and_unread() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &["did:key:priya"]);
    api.add("b", "did:key:sam", &[], &[]);
    api.incoming("a", "did:key:priya", "hello");
    api.incoming("a", "did:key:priya", "anyone?");
    api.incoming("b", "did:key:sam", "old");
    api.g.borrow_mut().get_mut("b").unwrap().removed = true;
    let mut l = live();
    run(&mut l, &api, Lm::Reload);
    assert_eq!(l.my_device, DEV);
    let a = l.summary("a").unwrap();
    assert_eq!(a.unread, 2);
    assert_eq!(preview(a, ME), "priya: anyone?");
    assert_eq!(preview(l.summary("b").unwrap(), ME), "You were removed from this group");
}

#[test]
fn opening_a_group_loads_it_marks_it_read_and_live_messages_land_in_order() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    api.add("b", "did:key:sam", &[], &[]);
    api.incoming("a", "did:key:sam", "one");
    let mut l = live();
    let out = l.step(&api, Lm::Open("a".into()));
    assert!(out.opened);
    assert_eq!(l.msgs.len(), 1);
    assert_eq!(l.detail.as_ref().unwrap().my_role, GroupRole::Member);
    assert_eq!(api.marked.borrow().as_slice(), ["a".to_string()]);

    let two = api.incoming("a", "did:key:sam", "two");
    // An older message delivered late goes in its place, not at the end.
    let mut late = msg("a", "m0", 900, "did:key:sam", "sam", GroupItemKind::Post, "zero");
    late.at = 900;
    l.step(&api, Lm::Message(late));
    l.step(&api, Lm::Message(two.clone()));
    l.step(&api, Lm::Message(two));
    let texts: Vec<&str> = l.msgs.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, ["zero", "one", "two"], "ordered by time, no duplicates");

    // Another group's message only touches the list.
    api.incoming("b", "did:key:sam", "elsewhere");
    let other = api.g.borrow()["b"].msgs[0].clone();
    l.step(&api, Lm::Message(other));
    assert_eq!(l.msgs.len(), 3);
}

#[test]
fn sending_posts_through_a_job_and_shows_waiting_for_a_host() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    api.g.borrow_mut().get_mut("a").unwrap().online = false;
    let mut l = live();
    l.step(&api, Lm::Open("a".into()));
    assert!(l.waiting_for_host());
    assert_eq!(l.status_line(), "2 members \u{b7} waiting for a host");
    l.step(&api, Lm::Input("  hi all ".into()));
    let out = l.step(&api, Lm::Send);
    assert_eq!(out.job, Some(Job::Post { gid: "a".into(), text: "hi all".into(), reply: None }));
    assert!(l.input.is_empty());
    l.step(&api, out.job.unwrap().run(&api));
    let mine = l.msgs.last().unwrap();
    assert!(mine.outgoing);
    assert_eq!(preview(l.summary("a").unwrap(), ME), "You: hi all");

    // Host acknowledges: a Changed event re-reads the window and the mark goes.
    api.g.borrow_mut().get_mut("a").unwrap().msgs.last_mut().unwrap().delivery = GroupDelivery::Delivered;
    l.step(&api, Lm::Changed("a".into()));
}

#[test]
fn a_reply_carries_its_target_and_a_failed_send_gives_the_text_back() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    let first = api.incoming("a", "did:key:sam", "q?");
    let mut l = live();
    l.step(&api, Lm::Open("a".into()));
    l.step(&api, Lm::Reply(Some(first.id.clone())));
    l.step(&api, Lm::Input("yes".into()));
    let out = l.step(&api, Lm::Send);
    assert_eq!(out.job, Some(Job::Post { gid: "a".into(), text: "yes".into(), reply: Some(first.id.clone()) }));
    assert_eq!(l.reply, None);
    // Removed between typing and the core answering.
    api.g.borrow_mut().get_mut("a").unwrap().removed = true;
    let out = l.step(&api, out.job.unwrap().run(&api));
    assert_eq!(out.notice.as_deref(), Some("You were removed from this group."));
    assert_eq!(l.input, "yes");
}

#[test]
fn a_removed_member_cannot_post_and_sees_the_signed_flags() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    api.g.borrow_mut().get_mut("a").unwrap().removed = true;
    let mut l = live();
    l.step(&api, Lm::Open("a".into()));
    assert!(!l.can_post());
    assert!(!l.waiting_for_host());
    assert_eq!(l.status_line(), "You were removed from this group");
    l.step(&api, Lm::Input("still here?".into()));
    let out = l.step(&api, Lm::Send);
    assert_eq!(out.job, None);
    assert!(out.notice.is_some());
    l.step(&api, Lm::Removal);
    assert_eq!(l.dialog, Some(LDialog::Removal));
    assert_eq!(l.detail.as_ref().unwrap().removal.as_ref().unwrap().flags.len(), 3);
    assert!(l.actions(&l.detail.clone().unwrap().members[0]).is_empty(), "no member actions once removed");
}

#[test]
fn member_actions_follow_roles() {
    let api = Fake::default();
    // We own "own"; in "adm" we are an admin; in "mem" a plain member.
    api.add("own", ME, &[], &["did:key:priya"]);
    api.add("adm", "did:key:sam", &[ME], &["did:key:priya"]);
    api.add("mem", "did:key:sam", &[], &["did:key:priya"]);
    let mut l = live();
    let acts = |l: &GroupsLive, did: &str| l.actions(l.member(did).unwrap());

    l.step(&api, Lm::Open("own".into()));
    let p = acts(&l, "did:key:priya");
    assert_eq!(p[0], Action::Message);
    assert_eq!(p[1], Action::MakeAdmin);
    assert!(p.contains(&Action::Flag));
    assert!(p.iter().any(|a| matches!(a, Action::Appoint(_))));
    assert!(acts(&l, ME).is_empty(), "nothing on ourselves");

    l.step(&api, Lm::Open("adm".into()));
    let p = acts(&l, "did:key:priya");
    assert!(!p.contains(&Action::MakeAdmin), "only the owner makes admins");
    assert!(p.contains(&Action::Flag));
    let owner = acts(&l, "did:key:sam");
    assert!(!owner.contains(&Action::Flag), "the owner can't be flagged");
    assert!(owner.iter().any(|a| matches!(a, Action::Revoke(_))), "the owner's host device can be revoked");

    l.step(&api, Lm::Open("mem".into()));
    assert_eq!(acts(&l, "did:key:priya"), vec![Action::Message], "a plain member can only ask for a private chat");
}

#[test]
fn three_flags_remove_and_the_dialog_says_which_flag_ours_is() {
    let api = Fake::default();
    api.add("a", ME, &[], &["did:key:dev"]);
    let mut l = live();
    l.step(&api, Lm::Open("a".into()));
    for n in 1..=3u32 {
        l.step(&api, Lm::AskFlag("did:key:dev".into()));
        assert_eq!(l.flag_after("did:key:dev"), (n, n == 3));
        let said = run(&mut l, &api, Lm::Confirm);
        assert!(said.unwrap().starts_with("Flag recorded"));
        assert_eq!(l.dialog, None);
    }
    assert!(l.member("did:key:dev").is_none(), "removed after the 3rd flag");
}

#[test]
fn a_refused_action_stays_in_its_dialog_with_the_reason() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    let mut l = live();
    l.step(&api, Lm::Open("a".into()));
    // We are not an admin: the core refuses the flag.
    l.step(&api, Lm::AskFlag("did:key:sam".into()));
    run(&mut l, &api, Lm::Confirm);
    assert!(matches!(l.dialog, Some(LDialog::Flag { .. })));
    assert_eq!(l.error.as_deref(), Some("Only admins can do that."));
    assert!(!l.busy);
    l.step(&api, Lm::Dismiss);
    assert_eq!(l.dialog, None);
}

#[test]
fn new_group_needs_a_name_then_opens_it_hosted_here() {
    let api = Fake::default();
    let mut l = live();
    l.step(&api, Lm::NewGroup);
    assert_eq!(l.step(&api, Lm::Confirm).job, None);
    assert!(l.error.is_some());
    l.step(&api, Lm::NameIn("Climbing".into()));
    let said = run(&mut l, &api, Lm::Confirm);
    assert!(said.unwrap().starts_with("Group made"));
    assert_eq!(l.dialog, None);
    let d = l.detail.as_ref().unwrap();
    assert_eq!(d.name, "Climbing");
    assert_eq!(d.my_role, GroupRole::Owner);
    assert!(d.am_host);
    assert_eq!(l.status_line(), "1 member \u{b7} this computer hosts the group");
}

#[test]
fn join_peeks_the_token_then_joins_or_shows_the_refusal() {
    let api = Fake::default();
    api.tokens.borrow_mut().insert("tinline://g/good".into(), Ok("joined".into()));
    api.tokens.borrow_mut().insert("tinline://g/old".into(), Err("invite expired or invalid".into()));
    let mut l = live();
    l.step(&api, Lm::JoinOpen);
    // Peeking is network: a job, never a call on the UI thread; "checking" until it answers.
    let out = l.step(&api, Lm::TokenIn("garbage".into()));
    assert_eq!(out.job, Some(Job::Peek("garbage".into())));
    assert!(matches!(&l.dialog, Some(LDialog::Join { peek: None, checking: true, .. })));
    l.step(&api, out.job.unwrap().run(&api));
    assert!(matches!(&l.dialog, Some(LDialog::Join { peek: None, checking: false, .. })));
    assert_eq!(l.step(&api, Lm::Confirm).job, None, "nothing to join");

    // A stale answer (for what was in the field before) is ignored.
    let stale = l.step(&api, Lm::TokenIn("tinline://g/good".into())).job.unwrap();
    run(&mut l, &api, Lm::TokenIn(" tinline://g/old \n".into()));
    l.step(&api, Lm::Peeked("tinline://g/good".into(), None));
    assert!(matches!(&l.dialog, Some(LDialog::Join { peek: Some(p), .. }) if p.name == "Hike club"));
    drop(stale);
    run(&mut l, &api, Lm::Confirm);
    assert_eq!(l.error.as_deref(), Some("Invite expired or invalid."));
    assert!(l.dialog.is_some());

    run(&mut l, &api, Lm::TokenIn("tinline://g/good".into()));
    let said = run(&mut l, &api, Lm::Confirm);
    assert_eq!(said.as_deref(), Some("You joined the group."));
    assert_eq!(l.open.as_deref(), Some("joined"));
    assert_eq!(l.dialog, None);
}

#[test]
fn the_invite_link_comes_back_into_the_dialog_copies_and_shows_full_size() {
    let api = Fake::default();
    api.add("own", ME, &[], &[]);
    api.add("mem", "did:key:sam", &[], &[]);
    let mut l = live();
    l.step(&api, Lm::Open("own".into()));
    l.step(&api, Lm::QrFull(true));
    assert!(!l.qr_full, "nothing to show yet");
    let out = l.step(&api, Lm::Invite);
    assert!(l.busy);
    assert_eq!(l.dialog, Some(LDialog::Token { text: None }));
    l.step(&api, out.job.unwrap().run(&api));
    assert_eq!(l.dialog, Some(LDialog::Token { text: Some("tinline://g/own".into()) }));
    assert_eq!(l.step(&api, Lm::CopyToken).copy.as_deref(), Some("tinline://g/own"));
    l.step(&api, Lm::QrFull(true));
    assert!(l.qr_full);
    // The first dismiss closes the full-size code, the second the dialog.
    l.step(&api, Lm::Dismiss);
    assert!(!l.qr_full && l.dialog.is_some());
    l.step(&api, Lm::Dismiss);
    assert_eq!(l.dialog, None);

    // A plain member can't invite.
    l.step(&api, Lm::Open("mem".into()));
    run(&mut l, &api, Lm::Invite);
    assert_eq!(l.error.as_deref(), Some("Only admins can do that."));
}

#[test]
fn the_picker_hides_blocked_contacts_and_shows_members_and_pending_invitations() {
    let api = Fake::default();
    api.add("own", ME, &[], &["did:key:sam"]);
    api.invs.borrow_mut().push(inv("x1", "own", true, "did:key:leo", GroupInvitationState::Pending));
    api.invs.borrow_mut().push(inv("x2", "own", true, "did:key:zoe", GroupInvitationState::Declined));
    let mut l = live();
    l.step(&api, Lm::Open("own".into()));
    let contacts: Vec<(String, String)> = ["sam", "leo", "priya", "ravi", "zoe"].iter().map(|n| (format!("did:key:{n}"), n.to_string())).collect();
    let rows = pick_rows(&contacts, l.detail.as_ref().unwrap(), &l.sent, |d| d == "did:key:priya" || d == "did:key:ravi");
    let dids: Vec<&str> = rows.iter().map(|r| r.did.as_str()).collect();
    assert_eq!(dids, ["did:key:zoe", "did:key:leo", "did:key:sam"]);
    assert_eq!(rows[0].why, None, "declined before: can be asked again");
    assert_eq!(rows[1].why, Some("Invited \u{b7} waiting"));
    assert_eq!(rows[2].why, Some("Already a member"));
    assert_eq!(sent_invitation_line(&l.sent[1]), ("Declined", true));
}

#[test]
fn adding_members_sends_invitations_and_shows_their_state() {
    let api = Fake::default();
    api.add("own", ME, &[], &[]);
    let mut l = live();
    l.step(&api, Lm::Open("own".into()));
    l.step(&api, Lm::AddMembers);
    assert_eq!(l.step(&api, Lm::Confirm).job, None);
    assert_eq!(l.error.as_deref(), Some("Pick someone first."));
    l.step(&api, Lm::Pick("did:key:leo".into()));
    l.step(&api, Lm::Pick("did:key:zoe".into()));
    l.step(&api, Lm::Pick("did:key:zoe".into()));
    assert_eq!(l.dialog, Some(LDialog::AddMembers { picked: vec!["did:key:leo".into()] }));
    let said = run(&mut l, &api, Lm::Confirm);
    assert_eq!(said.as_deref(), Some("Invitation sent to leo. They join when they accept."));
    assert_eq!(l.dialog, None);
    assert_eq!(l.sent.len(), 1);
    assert_eq!(sent_invitation_line(&l.sent[0]), ("Invited \u{b7} waiting for an answer", false));
}

#[test]
fn invitations_to_us_are_counted_and_join_or_decline() {
    let api = Fake::default();
    api.invs.borrow_mut().push(inv("in1", "hike", false, "did:key:sam", GroupInvitationState::Pending));
    api.invs.borrow_mut().push(inv("in2", "chess", false, "did:key:ravi", GroupInvitationState::Pending));
    let mut l = live();
    run(&mut l, &api, Lm::Reload);
    assert_eq!(l.pending_invitations().count(), 2);
    let said = run(&mut l, &api, Lm::AcceptInv("in1".into()));
    assert_eq!(said.as_deref(), Some("You joined Group hike."));
    assert_eq!(l.open.as_deref(), Some("hike"));
    let said = run(&mut l, &api, Lm::DeclineInv("in2".into()));
    assert_eq!(said.as_deref(), Some("Declined. ravi sees that you declined."));
    assert_eq!(l.pending_invitations().count(), 0);
    assert_eq!(run(&mut l, &api, Lm::DeclineInv("in2".into())).as_deref(), Some("Already answered."));
    // An event re-reads the list.
    api.invs.borrow_mut().push(inv("in3", "x", false, "did:key:sam", GroupInvitationState::Pending));
    l.step(&api, Lm::InvitationChanged(inv("in3", "x", false, "did:key:sam", GroupInvitationState::Pending)));
    assert_eq!(l.pending_invitations().count(), 1);
}

#[test]
fn hosting_from_this_device_and_appointing_others() {
    let api = Fake::default();
    api.add("a", ME, &[], &["did:key:priya"]);
    api.g.borrow_mut().get_mut("a").unwrap().hosts.clear();
    let mut l = live();
    l.step(&api, Lm::Open("a".into()));
    assert_eq!(l.status_line(), "2 members \u{b7} no host yet");
    l.step(&api, Lm::AskHost(true));
    run(&mut l, &api, Lm::Confirm);
    assert!(l.detail.as_ref().unwrap().am_host);
    let dev = l.member("did:key:priya").unwrap().device_ids[0].clone();
    run(&mut l, &api, Lm::Appoint("did:key:priya".into(), dev.clone()));
    assert!(l.actions(l.member("did:key:priya").unwrap()).contains(&Action::Revoke(dev.clone())));
    run(&mut l, &api, Lm::Revoke("did:key:priya".into(), dev.clone()));
    assert!(l.actions(l.member("did:key:priya").unwrap()).contains(&Action::Appoint(dev)));
    // Settings › Hosting: opens the group and asks.
    l.close();
    let out = l.step(&api, Lm::StopHostingOf("a".into()));
    assert!(out.opened);
    assert_eq!(l.dialog, Some(LDialog::Host { accept: false }));
    run(&mut l, &api, Lm::Confirm);
    assert!(!l.detail.as_ref().unwrap().am_host);
}

#[test]
fn leaving_keeps_the_history_read_only_and_the_owner_cannot_leave() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    api.add("own", ME, &[], &[]);
    api.incoming("a", "did:key:sam", "bye");
    let mut l = live();
    l.step(&api, Lm::Open("own".into()));
    l.step(&api, Lm::AskLeave);
    run(&mut l, &api, Lm::Confirm);
    assert_eq!(l.error.as_deref(), Some("The owner cannot leave."));
    l.step(&api, Lm::Dismiss);
    l.step(&api, Lm::Open("a".into()));
    l.step(&api, Lm::AskLeave);
    run(&mut l, &api, Lm::Confirm);
    assert_eq!(l.role(), Some(GroupRole::Left));
    assert!(!l.can_post());
    assert_eq!(l.msgs.len(), 1);
    assert_eq!(preview(l.summary("a").unwrap(), ME), "You left this group");
}

#[test]
fn system_lines_and_errors_read_plainly() {
    let mut m = msg("a", "x", 1, "did:key:sam", "Sam Rao", GroupItemKind::AdminGranted, "");
    m.subject_did = ME.into();
    assert_eq!(sys_text(&m, ME), "Sam Rao made you an admin");
    m.subject_did = "did:key:p".into();
    m.subject_name = "Priya".into();
    assert_eq!(sys_text(&m, ME), "Sam Rao made Priya an admin");
    m.kind = GroupItemKind::Removed;
    assert_eq!(sys_text(&m, ME), "Priya was removed after 3 admin flags");
    m.kind = GroupItemKind::Joined;
    m.subject_name = String::new();
    assert_eq!(sys_text(&m, ME), "Someone joined");
    assert_eq!(gerr(Error::Timeout), "No host answered in time. Try again when a host is online.");
    assert_eq!(gerr(Error::Rejected("removed".into())), "You were removed from this group.");
}

#[test]
fn older_pages_go_in_front_and_a_vanished_group_closes() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    for i in 0..(PAGE + 5) {
        api.incoming("a", "did:key:sam", &format!("n{i}"));
    }
    let mut l = live();
    l.step(&api, Lm::Open("a".into()));
    assert_eq!(l.msgs.len(), PAGE as usize);
    assert!(l.older.is_some());
    l.step(&api, Lm::Older);
    assert_eq!(l.msgs.len(), PAGE as usize + 5);
    assert_eq!(l.msgs[0].text, "n0");
    assert!(l.older.is_none());
    api.g.borrow_mut().remove("a");
    l.step(&api, Lm::Changed("a".into()));
    assert_eq!(l.open, None);
}

fn incoming(api: &Fake, from: &str, gid: &str) -> String {
    let id = format!("in{}", api.tick());
    api.reqs.borrow_mut().push(GroupChatRequest {
        id: id.clone(),
        group_id: gid.into(),
        group_name: "Hike".into(),
        peer_did: from.into(),
        peer_name: "Priya Nair".into(),
        outgoing: false,
        state: ChatRequestState::Pending,
        at: 5,
        contact_added: false,
    });
    id
}

#[test]
fn message_privately_sends_a_request_that_waits_in_sent_requests() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &["did:key:leo"]);
    let mut l = live();
    l.step(&api, Lm::Open("a".into()));
    l.step(&api, Lm::AskRequest("did:key:leo".into()));
    assert_eq!(l.dialog, Some(LDialog::Request { did: "did:key:leo".into() }));
    let said = run(&mut l, &api, Lm::Confirm);
    assert_eq!(said.as_deref(), Some("Request sent to leo. It waits in Chats until they answer."));
    assert_eq!(l.dialog, None);
    assert_eq!(l.outgoing.len(), 1);
    assert_eq!(sent_line(&l.outgoing[0]), ("Waiting for leo \u{b7} via Group a".to_string(), false));

    // The answer arrives as an event; each end state is worded once.
    let mut r = l.outgoing[0].clone();
    for (st, added, want, bad) in [
        (ChatRequestState::Declined, false, "leo declined", true),
        (ChatRequestState::Blocked, false, "leo blocked you", true),
        (ChatRequestState::Accepted, true, "leo accepted \u{b7} messages only", false),
    ] {
        r.state = st;
        r.contact_added = added;
        assert_eq!(sent_line(&r), (want.to_string(), bad));
    }
    api.reqs.borrow_mut()[0] = r.clone();
    let out = l.step(&api, Lm::RequestChanged(r));
    assert!(out.notice.unwrap().starts_with("leo accepted your request"));
    assert_eq!(l.outgoing[0].state, ChatRequestState::Accepted);

    // An existing contact or someone we block: the core refuses, the dialog says why.
    api.contacts.borrow_mut().push("did:key:sam".into());
    l.step(&api, Lm::AskRequest("did:key:sam".into()));
    run(&mut l, &api, Lm::Confirm);
    assert_eq!(l.error.as_deref(), Some("You already have them as a contact. Message them from Chats."));
}

#[test]
fn incoming_requests_accept_decline_or_block() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &["did:key:priya", "did:key:dev", "did:key:ravi"]);
    let one = incoming(&api, "did:key:priya", "a");
    let two = incoming(&api, "did:key:dev", "a");
    let three = incoming(&api, "did:key:ravi", "a");
    let mut l = live();
    l.step(&api, Lm::Reload);
    assert_eq!(l.pending_in().count(), 3);

    let out = l.step(&api, Lm::AcceptReq(one));
    let out = l.step(&api, out.job.unwrap().run(&api));
    assert_eq!(out.chat.as_deref(), Some("did:key:priya"), "the new chat opens");
    assert!(out.notice.unwrap().contains("messages only"));
    assert!(api.contacts.borrow().contains(&"did:key:priya".to_string()));

    let said = run(&mut l, &api, Lm::DeclineReq(two));
    assert_eq!(said.as_deref(), Some("Declined. Priya is told \u{201c}declined\u{201d}."));

    run(&mut l, &api, Lm::BlockReq(three, "did:key:ravi".into()));
    assert!(api.blocked.borrow().contains(&"did:key:ravi".to_string()), "block is the personal block (P5)");
    assert_eq!(l.pending_in().count(), 0);
}

#[test]
fn an_admin_asking_this_device_to_host_shows_a_prompt() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    *api.asked.borrow_mut() = Some(("a".into(), "did:key:sam".into()));
    let mut l = live();
    l.step(&api, Lm::Open("a".into()));
    assert_eq!(l.host_asked().as_deref(), Some("sam"));
    let out = l.step(&api, Lm::AnswerHost(true));
    assert_eq!(out.job, Some(Job::AcceptHost { gid: "a".into(), accept: true }));
    l.step(&api, out.job.unwrap().run(&api));
    assert_eq!(l.host_asked(), None);
    assert!(l.detail.as_ref().unwrap().am_host);
}

#[path = "group_public_tests.rs"]
mod public;
