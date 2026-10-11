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
}

#[derive(Default)]
struct Fake {
    g: RefCell<BTreeMap<String, G>>,
    n: RefCell<u64>,
    /// Tokens group_join accepts, and what it answers.
    tokens: RefCell<BTreeMap<String, Result<String, String>>>,
    marked: RefCell<Vec<String>>,
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
            removal: g.removed.then(|| GroupRemoval {
                at: 5,
                flags: (1..=3).map(|i| GroupFlag { by_did: format!("did:key:a{i}"), by_name: format!("A{i}"), at: i }).collect(),
            }),
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
        self.with(&id, |g| Fake::check_admin(g).map(|_| format!("TLGRP1:{id}")))
    }
    fn group_device_token(&self, id: String) -> Result<String, Error> {
        self.with(&id, |_| Ok(format!("TLGRP1:dev:{id}")))
    }
    fn group_peek_invite(&self, token: String) -> Option<GroupInvitePeek> {
        self.tokens.borrow().contains_key(&token).then(|| GroupInvitePeek {
            group_id: "joined".into(),
            name: "Hike club".into(),
            owner_did: "did:key:own".into(),
            issuer_did: Some("did:key:own".into()),
            expires: Some(7 * 86400),
            host_count: 1,
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
fn the_list_shows_last_message_and_unread_and_the_badge_skips_removed_groups() {
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
    assert_eq!(l.badge(), 2, "a removed group's unread does not count");
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
    assert_eq!(l.badge(), 0);

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
    assert_eq!(l.badge(), 1);
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
    assert_eq!(delivery_word(mine), Some("Waiting for a host"));
    assert_eq!(preview(l.summary("a").unwrap(), ME), "You: hi all");

    // Host acknowledges: a Changed event re-reads the window and the mark goes.
    api.g.borrow_mut().get_mut("a").unwrap().msgs.last_mut().unwrap().delivery = GroupDelivery::Delivered;
    l.step(&api, Lm::Changed("a".into()));
    assert_eq!(delivery_word(l.msgs.last().unwrap()), None);
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
    assert_eq!(p[0], Action::MakeAdmin);
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
    assert!(acts(&l, "did:key:priya").is_empty());
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
    api.tokens.borrow_mut().insert("TLGRP1:good".into(), Ok("joined".into()));
    api.tokens.borrow_mut().insert("TLGRP1:old".into(), Err("invite expired or invalid".into()));
    let mut l = live();
    l.step(&api, Lm::JoinOpen);
    l.step(&api, Lm::TokenIn("garbage".into()));
    assert!(matches!(&l.dialog, Some(LDialog::Join { peek: None, .. })));
    assert_eq!(l.step(&api, Lm::Confirm).job, None, "nothing to join");

    l.step(&api, Lm::TokenIn(" TLGRP1:old \n".into()));
    assert!(matches!(&l.dialog, Some(LDialog::Join { peek: Some(p), .. }) if p.name == "Hike club"));
    run(&mut l, &api, Lm::Confirm);
    assert_eq!(l.error.as_deref(), Some("Invite expired or invalid."));
    assert!(l.dialog.is_some());

    l.step(&api, Lm::TokenIn("TLGRP1:good".into()));
    let said = run(&mut l, &api, Lm::Confirm);
    assert_eq!(said.as_deref(), Some("You joined the group."));
    assert_eq!(l.open.as_deref(), Some("joined"));
    assert_eq!(l.dialog, None);
}

#[test]
fn invite_and_device_tokens_come_back_into_the_dialog_and_copy() {
    let api = Fake::default();
    api.add("own", ME, &[], &[]);
    api.add("mem", "did:key:sam", &[], &[]);
    let mut l = live();
    l.step(&api, Lm::Open("own".into()));
    let out = l.step(&api, Lm::Invite);
    assert!(l.busy);
    assert_eq!(l.dialog, Some(LDialog::Token { kind: TokenKind::Invite, text: None }));
    l.step(&api, out.job.unwrap().run(&api));
    assert_eq!(l.dialog, Some(LDialog::Token { kind: TokenKind::Invite, text: Some("TLGRP1:own".into()) }));
    assert_eq!(l.step(&api, Lm::CopyToken).copy.as_deref(), Some("TLGRP1:own"));

    // A plain member can't invite, but can add their own other device.
    l.step(&api, Lm::Open("mem".into()));
    run(&mut l, &api, Lm::Invite);
    assert_eq!(l.error.as_deref(), Some("Only admins can do that."));
    run(&mut l, &api, Lm::DeviceToken);
    assert_eq!(l.dialog, Some(LDialog::Token { kind: TokenKind::Device, text: Some("TLGRP1:dev:mem".into()) }));
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
    l.step(&api, Lm::AskHost(false));
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
