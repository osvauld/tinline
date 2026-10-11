//! Group chat screens on the core's real API (state in `group_live.rs`), laid out after
//! docs/design/groups/DesktopGroups: the Groups side tab, the conversation with its members pane,
//! member actions, and the new/join/invite/flag/removal/hosting dialogs.

use iced::widget::{button, column, container, mouse_area, opaque, row, text_input, Space};
use p2pcore::{GroupDelivery, GroupItemKind, GroupMessage, GroupRole};

use super::groups_view::{btn, dot, footer, gav, hhmm, link_btn, message_body, ordinal, rule, short_time, sys, tag, when};
use super::*;
use crate::app::group_live::{self as gl, first_name, short_dev, Action, LDialog, Lm, TokenKind, FLAGS_TO_REMOVE};

fn l(m: Lm) -> Msg {
    Msg::Live(m)
}

const COMPOSER: &str = "group-composer";

impl App {
    /// A DID as the user knows it: their contact name, else what the group calls them.
    fn live_name(&self, did: &str, fallback: &str) -> String {
        if self.is_me(did) || did == self.live.me {
            return "You".into();
        }
        if let Some(c) = self.contacts.iter().find(|c| c.did == did) {
            return Self::display(c);
        }
        if fallback.trim().is_empty() { "Someone".into() } else { fallback.to_string() }
    }

    // ---- the Groups tab ----

    pub(super) fn live_badge(&self) -> u32 {
        self.live.badge()
    }

    pub(super) fn live_list(&self, t: Tok, q: &str) -> El<'_> {
        let mut col = column![
            row![
                pill(t, Kind::Tonal, Some(Icon::Users), "New group", Some(l(Lm::NewGroup))),
                pill(t, Kind::Quiet, Some(Icon::UserPlus), "Join", Some(l(Lm::JoinOpen))),
            ]
            .spacing(6)
            .padding([8, 12]),
        ]
        .spacing(2);
        let ready = self.live.ready;
        col = col.push(
            container(label(t, &format!("Groups \u{b7} {}", self.live.list.len()))).padding(Padding { top: 8.0, bottom: 4.0, left: 12.0, right: 0.0 }),
        );
        let mut any = false;
        for s in &self.live.list {
            if !q.is_empty() && !s.name.to_lowercase().contains(q) {
                continue;
            }
            any = true;
            let sel = self.live.open.as_deref() == Some(s.id.as_str());
            let gone = !gl::active(s.my_role);
            let fade = |c: Color| if gone { ui::alpha(c, 0.55) } else { c };
            let (fg, dim) = if sel { (t.on_primary_c, t.on_primary_c) } else { (fade(t.ink), fade(t.ink2)) };
            let unread = s.unread > 0 && !sel && !gone;
            let at = s.last.as_ref().map_or(0, |m| m.at);
            let mut sub = row![].spacing(4).align_y(Alignment::Center);
            if gl::active(s.my_role) && !s.hosted_here && !s.host_reachable {
                sub = sub.push(ui::icon(Icon::Clock, 13.0, dim));
            }
            sub = sub.push(container(tx(gl::preview(s, &self.live.me), 13.0, dim).wrapping(iced::widget::text::Wrapping::None)).width(Fill).clip(true));
            if unread {
                sub = sub.push(container(semi(s.unread.to_string(), 12.0, t.on_primary)).padding([1, 7]).style(ui::plain(t.primary, 999.0)));
            }
            let name = if unread { bold(s.name.clone(), 15.0, fg) } else { semi(s.name.clone(), 15.0, fg) };
            col = col.push(
                button(
                    row![
                        gav(t, &s.id, 40.0),
                        column![row![name, Space::new().width(Fill), tx(short_time(at), 12.0, dim)], sub].spacing(2).width(Fill),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .width(Fill)
                .style(ui::row_style(t, sel))
                .on_press(l(Lm::Open(s.id.clone()))),
            );
        }
        if !any {
            let msg = if !ready {
                "Unlock to see your groups."
            } else if q.is_empty() {
                "No groups yet. Make one, or join with an invite someone sent you."
            } else {
                "No group matches."
            };
            col = col.push(container(tx(msg, 13.0, t.ink2)).padding([4, 12]));
        }
        col.into()
    }

    // ---- the conversation ----

    pub(super) fn live_pane(&self, t: Tok) -> Option<El<'_>> {
        let d = self.live.detail.as_ref()?;
        self.live.open.as_ref()?;
        let gone = !gl::active(d.my_role);
        let sub_color = if d.my_role == GroupRole::Removed { t.error } else if self.live.waiting_for_host() { t.thread_text } else { t.ink2 };
        let on = !self.live.hide_info;
        let head = row![
            gav(t, &d.id, 36.0),
            column![bold(d.name.clone(), 17.0, t.ink), tx(self.live.status_line(), 13.0, sub_color)].spacing(1).width(Fill),
            button(container(ui::icon(Icon::Users, 20.0, if on { t.on_primary_c } else { t.ink2 })).center_x(36).center_y(36))
                .padding(0)
                .width(36)
                .height(36)
                .style(ui::button_style(t, if on { Kind::Tonal } else { Kind::Ghost }, 8.0))
                .on_press(l(Lm::ToggleInfo)),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        let mut top = column![container(head).padding([10, 20]).width(Fill), rule(t)];
        if self.notice.is_some() {
            top = top.push(container(self.notice_bar(t)).padding([8, 20]));
        }
        if let Some(by) = self.live.host_asked() {
            top = top.push(
                container(
                    container(
                        column![
                            row![ui::icon(Icon::Server, 18.0, t.on_warn), bold(format!("{} asked this computer to host {}", first_name(&by), d.name), 15.0, t.on_warn)]
                                .spacing(8)
                                .align_y(Alignment::Center),
                            tx(
                                "A host keeps a full copy of the group, passes messages on while members\u{2019} phones sleep and learns members\u{2019} group addresses. It needs to stay running. Hosting doesn\u{2019}t make you an admin; you can stop any time.",
                                13.0,
                                t.on_warn
                            ),
                            row![
                                btn(t, Kind::Primary, "Host here", Some(l(Lm::AnswerHost(true)))),
                                btn(t, Kind::Ghost, "Decline", Some(l(Lm::AnswerHost(false)))),
                            ]
                            .spacing(8),
                        ]
                        .spacing(8),
                    )
                    .padding(14)
                    .width(Fill)
                    .style(ui::outlined(t.warn, ui::alpha(t.on_warn, 0.25), 14.0)),
                )
                .padding([12, 20]),
            );
        }
        if self.live.waiting_for_host() {
            let body = if d.hosts.is_empty() {
                "This group has no host yet. Your messages are saved here and send when a member\u{2019}s computer hosts it."
            } else {
                "No host is online. Your messages are saved here and send when one comes back."
            };
            top = top.push(container(banner(t, "Waiting for a host", body, None, false)).padding([12, 20]));
        }

        let mut list = column![].spacing(8).padding([16, 24]);
        if self.live.older.is_some() {
            list = list.push(container(link_btn(t, "Show earlier messages", t.primary, l(Lm::Older))).center_x(Fill));
        }
        for m in &self.live.msgs {
            list = list.push(self.live_item(t, m));
        }
        if self.live.msgs.is_empty() {
            list = list.push(sys(t, "No messages yet"));
        }
        let msgs = scroll(t, list).anchor_bottom().width(Fill).height(Fill);

        let bottom: El = match d.my_role {
            GroupRole::Removed => footer(
                t,
                row![
                    tx("You were removed and can\u{2019}t send messages to this group.", 14.0, t.ink2).width(Fill),
                    btn(t, Kind::Quiet, "Why?", Some(l(Lm::Removal))),
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .into(),
            ),
            GroupRole::Left => footer(t, tx("You left this group. Its messages stay here, read-only.", 14.0, t.ink2).into()),
            _ => self.live_composer(t),
        };
        let conv = column![top, msgs, bottom].width(Fill).height(Fill);
        if !on {
            return Some(conv.into());
        }
        let _ = gone;
        Some(row![conv, self.live_members(t)].into())
    }

    fn live_composer(&self, t: Tok) -> El<'_> {
        let mut c = column![].spacing(6);
        if let Some(id) = &self.live.reply {
            let (who, text_) = match self.live.by_id(id) {
                Some(m) => (self.live_name(&m.author_did, &m.author_name), m.text.clone()),
                None => ("a message".to_string(), String::new()),
            };
            c = c.push(
                row![
                    ui::icon(Icon::Reply, 14.0, t.ink2),
                    container(tx(format!("Replying to {who}: {text_}"), 12.0, t.ink2).wrapping(iced::widget::text::Wrapping::None)).width(Fill).clip(true),
                    button(ui::icon(Icon::X, 14.0, t.ink2)).padding(4).style(ui::button_style(t, Kind::Ghost, 999.0)).on_press(l(Lm::Reply(None))),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
        }
        let value = &self.live.input;
        let can = !value.trim().is_empty() && !self.live.sending;
        c = c.push(
            row![
                text_input("Message the group", value)
                    .id(COMPOSER)
                    .on_input(|v| l(Lm::Input(v)))
                    .on_submit(l(Lm::Send))
                    .padding([11, 16])
                    .size(14)
                    .style(move |th, st| {
                        let mut s = ui::input_style(t)(th, st);
                        s.border.radius = 22.0.into();
                        s
                    }),
                pill(t, Kind::Primary, Some(Icon::Send), "Send", can.then_some(l(Lm::Send))),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        footer(t, c.into())
    }

    fn live_item<'a>(&'a self, t: Tok, m: &'a GroupMessage) -> El<'a> {
        if m.kind == GroupItemKind::Removed && m.subject_did == self.live.me {
            return self.live_removal_card(t);
        }
        if m.kind != GroupItemKind::Post {
            return sys(t, &gl::sys_text(m, &self.live.me));
        }
        let quote = m.reply_to.as_ref().map(|id| match self.live.by_id(id) {
            Some(r) => format!("{}: {}", first_name(&self.live_name(&r.author_did, &r.author_name)), r.text),
            None => "Reply to an earlier message".to_string(),
        });
        let reply_btn = button(ui::icon(Icon::Reply, 14.0, t.ink2))
            .padding(4)
            .style(ui::button_style(t, Kind::Ghost, 999.0))
            .on_press_maybe(self.live.can_post().then(|| l(Lm::Reply(Some(m.id.clone())))));
        if m.outgoing {
            let dim = ui::alpha(t.on_primary_c, 0.75);
            let mut meta = row![tx(hhmm(m.at), 11.0, dim)].spacing(4).align_y(Alignment::Center);
            match m.delivery {
                GroupDelivery::Delivered => meta = meta.push(ui::icon(Icon::Check, 14.0, dim)),
                GroupDelivery::Waiting => meta = meta.push(ui::icon(Icon::Clock, 13.0, dim)).push(tx("Waiting for a host", 11.0, dim)),
            }
            let mut c = column![].spacing(4).align_x(Alignment::End);
            if let Some(q) = quote {
                c = c.push(container(tx(q, 12.0, t.on_primary_c)).padding([4, 8]).style(ui::plain(ui::alpha(t.on_primary_c, 0.1), 8.0)));
            }
            c = c.push(message_body(&m.text, t.on_primary_c)).push(meta);
            let b = container(c).padding([8, 12]).max_width(440).style(ui::plain(t.primary_c, 16.0));
            return row![Space::new().width(Fill), reply_btn, b].spacing(4).align_y(Alignment::Center).into();
        }
        let name = self.live_name(&m.author_did, &m.author_name);
        if self.blocks.blocked_by_me(&m.author_did) {
            // A personal block never changes the group (U9 is the group's tool); the message is
            // folded, not hidden from the record.
            return row![
                container(
                    row![ui::icon(Icon::Ban, 14.0, t.ink2), tx(format!("Message from {name}, whom you blocked"), 13.0, t.ink2)]
                        .spacing(6)
                        .align_y(Alignment::Center)
                )
                .padding([6, 12])
                .style(ui::outlined(t.bg, t.line, 16.0)),
                Space::new().width(Fill)
            ]
            .into();
        }
        let (_, color) = ui::avatar_colors(t, &m.author_did);
        let who = if m.before_removal { format!("{} \u{b7} removed", first_name(&name)) } else { first_name(&name).to_string() };
        let mut c = column![bold(who, 12.0, if m.before_removal { t.ink2 } else { color })].spacing(2);
        if let Some(q) = quote {
            c = c.push(container(tx(q, 12.0, t.ink2)).padding([4, 8]).style(ui::plain(t.surface2, 8.0)));
        }
        c = c.push(message_body(&m.text, t.ink));
        let mut meta = row![tx(hhmm(m.at), 11.0, t.ink2)].spacing(4);
        if m.before_removal {
            meta = meta.push(tx(format!("\u{b7} written before {} was removed", first_name(&name)), 11.0, t.ink2));
        }
        c = c.push(meta);
        let b = container(c).padding([8, 12]).max_width(440).style(ui::outlined(t.surface, t.line, 16.0));
        row![avatar(t, &name, &m.author_did, 28.0), b, reply_btn, Space::new().width(Fill)].spacing(6).align_y(Alignment::Center).into()
    }

    /// In the removed person's chat: when, why and by whom, plainly.
    fn live_removal_card(&self, t: Tok) -> El<'_> {
        let Some(r) = self.live.detail.as_ref().and_then(|d| d.removal.as_ref()) else {
            return sys(t, "You were removed from this group");
        };
        let names: Vec<String> = r.flags.iter().map(|f| self.live_name(&f.by_did, &f.by_name)).collect();
        container(
            column![
                semi(format!("You were removed from the group \u{b7} {}", when(r.at)), 14.0, t.on_error_c),
                bold(format!("Removed after {} admin flags", r.flags.len()), 15.0, t.on_error_c),
                tx(
                    format!(
                        "Flagged by {}. You can still read what you received before; nothing new will arrive.",
                        super::groups_view::and_list(&names)
                    ),
                    13.0,
                    t.on_error_c
                ),
                link_btn(t, "See details", t.on_error_c, l(Lm::Removal)),
            ]
            .spacing(4),
        )
        .padding([12, 16])
        .max_width(520)
        .style(ui::plain(t.error_c, 14.0))
        .into()
    }

    // ---- members pane ----

    fn live_members(&self, t: Tok) -> El<'_> {
        let Some(d) = self.live.detail.as_ref() else { return Space::new().into() };
        let sec = |s: &str| container(label(t, s)).padding(Padding { top: 14.0, bottom: 6.0, left: 16.0, right: 16.0 });
        let admin = self.live.is_admin();
        let mut col = column![sec("Hosts")].spacing(0);
        for h in &d.hosts {
            let who = if h.is_me {
                "This computer".to_string()
            } else {
                format!("{}\u{2019}s device {}", first_name(&self.live_name(&h.did, &h.name)), short_dev(&h.device_id))
            };
            let (c, right) = if h.online { (t.primary, if h.is_me { "hosting" } else { "online" }) } else { (t.ink2, "not connected") };
            col = col.push(
                container(row![dot(c), tx(who, 14.0, t.ink).width(Fill), tx(right, 13.0, t.ink2)].spacing(10).align_y(Alignment::Center))
                    .padding([0, 16])
                    .height(36)
                    .center_y(36),
            );
        }
        if d.hosts.is_empty() {
            col = col.push(container(tx("Needs a host", 13.0, t.error)).padding([4, 16]));
        }
        if admin && !d.host_requests.is_empty() {
            col = col.push(sec("Asked to host"));
            for h in &d.host_requests {
                let who = if h.is_me { "This computer".to_string() } else { format!("{}\u{2019}s device {}", first_name(&self.live_name(&h.did, &h.name)), short_dev(&h.device_id)) };
                col = col.push(
                    container(
                        row![
                            dot(t.thread),
                            tx(who, 14.0, t.ink).width(Fill),
                            tx("no answer yet", 13.0, t.ink2),
                            link_btn(t, "Withdraw", t.error, l(Lm::Revoke(h.did.clone(), h.device_id.clone()))),
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center),
                    )
                    .padding([0, 16])
                    .height(40)
                    .center_y(40),
                );
            }
        }
        if admin && !d.host_offers.is_empty() {
            col = col.push(sec("Offered to host"));
            for h in &d.host_offers {
                let who = if h.is_me { "This computer".to_string() } else { format!("{}\u{2019}s device {}", first_name(&self.live_name(&h.did, &h.name)), short_dev(&h.device_id)) };
                col = col.push(
                    container(
                        row![
                            dot(t.thread),
                            tx(who, 14.0, t.ink).width(Fill),
                            link_btn(t, "Appoint", t.primary, l(Lm::Appoint(h.did.clone(), h.device_id.clone()))),
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center),
                    )
                    .padding([0, 16])
                    .height(40)
                    .center_y(40),
                );
            }
        }
        col = col.push(sec(&format!("Members \u{b7} {}", d.members.len())));
        for m in &d.members {
            let name = self.live_name(&m.did, &m.name);
            let mut r = row![avatar(t, &name, &m.did, 28.0), semi(name.clone(), 14.0, t.ink).width(Fill)].spacing(8).align_y(Alignment::Center);
            if self.blocks.blocked_by_me(&m.did) {
                r = r.push(tx("Blocked", 12.0, t.error));
            }
            if admin && m.flags > 0 {
                r = r.push(tag(t.error_c, t.on_error_c, &format!("{} flag{}", m.flags, if m.flags == 1 { "" } else { "s" })));
            }
            if m.is_owner {
                r = r.push(tag(t.primary_c, t.on_primary_c, "Owner"));
            } else if m.is_admin {
                r = r.push(tag(t.relay_c, t.on_relay_c, "Admin"));
            }
            if m.is_host {
                r = r.push(tag(t.warn, t.on_warn, "Host"));
            }
            let acts = self.live.actions(m);
            let open = self.live.menu.as_deref() == Some(m.did.as_str());
            let b = button(r)
                .padding([8, 16])
                .width(Fill)
                .style(ui::row_style(t, open))
                .on_press_maybe((!acts.is_empty()).then(|| l(Lm::Menu(Some(m.did.clone())))));
            col = col.push(b);
            if open {
                col = col.push(container(self.live_menu(t, m, acts)).padding([4, 12]));
            }
        }

        // What we can do, by role.
        let role_word = match d.my_role {
            GroupRole::Owner => "You own this group",
            GroupRole::Admin => "You are an admin",
            GroupRole::Member => "You are a member",
            GroupRole::Removed => "You were removed",
            GroupRole::Left => "You left",
        };
        let mut foot = column![tx(role_word, 12.0, t.ink2)].spacing(6);
        if gl::active(d.my_role) {
            if admin {
                foot = foot.push(btn(t, Kind::Tonal, "Invite people", Some(l(Lm::Invite))));
            }
            foot = foot.push(btn(t, Kind::Quiet, "Add my other device", Some(l(Lm::DeviceToken))));
            let offered = d.host_offers.iter().any(|h| h.is_me);
            foot = foot.push(if d.am_host || offered {
                btn(t, Kind::Quiet, if d.am_host { "Stop hosting here" } else { "Withdraw host offer" }, Some(l(Lm::AskHost(false))))
            } else {
                btn(t, Kind::Quiet, "Host from this computer", Some(l(Lm::AskHost(true))))
            });
            if d.my_role != GroupRole::Owner {
                foot = foot.push(link_btn(t, "Leave group", t.error, l(Lm::AskLeave)));
            }
        } else if d.removal.is_some() {
            foot = foot.push(btn(t, Kind::Quiet, "Why was I removed?", Some(l(Lm::Removal))));
        }
        container(column![scroll(t, col).height(Fill), rule(t), container(foot).padding([12, 16])])
            .width(290)
            .height(Fill)
            .style(ui::sidebar(t))
            .into()
    }

    fn live_menu<'a>(&'a self, t: Tok, m: &'a p2pcore::GroupMember, acts: Vec<Action>) -> El<'a> {
        let first = first_name(&self.live_name(&m.did, &m.name)).to_string();
        let item = |i: Icon, title: String, sub: String, color: Color, on: Msg| -> El<'a> {
            button(
                row![ui::icon(i, 18.0, color), column![semi(title, 14.0, color), tx(sub, 12.0, t.ink2)].spacing(1).width(Fill)]
                    .spacing(10)
                    .align_y(Alignment::Center),
            )
            .padding([8, 10])
            .width(Fill)
            .style(ui::row_style(t, false))
            .on_press(on)
            .into()
        };
        let by = m.invited_by.as_deref().map(|d| first_name(&self.live_name(d, "")).to_string());
        let mut head = format!("Member since {}", when(m.joined_at));
        if let Some(b) = by {
            head += &format!(" \u{b7} invited by {b}");
        }
        if self.live.is_admin() {
            head += &format!(" \u{b7} {} of {FLAGS_TO_REMOVE} flags", m.flags);
        }
        let mut c = column![container(tx(head, 12.0, t.ink2)).padding([4, 10])].spacing(2);
        let several = m.device_ids.len() > 1;
        for a in acts {
            c = c.push(match a {
                Action::Message => {
                    let contact = self.contacts.iter().any(|c| c.did == m.did);
                    if contact {
                        item(Icon::MessageSquare, "Message privately".into(), "Opens your chat".into(), t.ink, Msg::Select(m.did.clone()))
                    } else if self.blocks.blocked_by_me(&m.did) {
                        item(Icon::Ban, "Message privately".into(), format!("You block {first}; unblock them first"), t.ink2, l(Lm::AskRequest(m.did.clone())))
                    } else {
                        item(Icon::MessageSquare, "Message privately".into(), format!("Sends {first} a chat request through the host"), t.ink, l(Lm::AskRequest(m.did.clone())))
                    }
                }
                Action::MakeAdmin => item(Icon::ShieldCheck, "Make admin".into(), "Can invite, flag members and appoint hosts".into(), t.ink, l(Lm::AskAdmin(m.did.clone(), true))),
                Action::RevokeAdmin => item(Icon::ShieldCheck, "Remove as admin".into(), format!("{first} stays a member"), t.ink, l(Lm::AskAdmin(m.did.clone(), false))),
                Action::Appoint(dev) => item(
                    Icon::Server,
                    if several { format!("Make device {} a host", short_dev(&dev)) } else { "Make host".into() },
                    format!("Hosts once {first} agrees on that device; a computer that stays on is best"),
                    t.ink,
                    l(Lm::Appoint(m.did.clone(), dev)),
                ),
                Action::Revoke(dev) => item(
                    Icon::Server,
                    if several { format!("Stop device {} hosting", short_dev(&dev)) } else { "Stop hosting".into() },
                    "Members stop going through it".into(),
                    t.ink,
                    l(Lm::Revoke(m.did.clone(), dev)),
                ),
                Action::Flag => item(
                    Icon::Flag,
                    "Flag to remove".into(),
                    format!("{FLAGS_TO_REMOVE} flags remove {first} from the group"),
                    t.error,
                    l(Lm::AskFlag(m.did.clone())),
                ),
            });
        }
        container(c).padding(6).width(Fill).style(ui::card(t)).into()
    }

    // ---- chat requests (U3), above the chats ----

    /// Incoming requests (Accept / Decline / Block) and our sent ones, as on the
    /// AndroidChatRequest board: a block above the chats, never mixed into conversations.
    pub(super) fn live_requests(&self, t: Tok) -> Option<El<'_>> {
        let incoming: Vec<_> = self.live.pending_in().collect();
        let sent = &self.live.outgoing;
        if incoming.is_empty() && sent.is_empty() {
            return None;
        }
        let head = |s_: String| container(label(t, &s_)).padding(Padding { top: 8.0, bottom: 4.0, left: 12.0, right: 0.0 });
        let mut col = column![].spacing(6);
        if !incoming.is_empty() {
            col = col.push(head(format!("Requests \u{b7} {}", incoming.len())));
            for r in incoming {
                let name = self.live_name(&r.peer_did, &r.peer_name);
                let first = first_name(&name).to_string();
                col = col.push(
                    container(
                        column![
                            row![
                                avatar(t, &name, &r.peer_did, 36.0),
                                column![semi(name.clone(), 14.0, t.ink), tx(format!("Member of {}", r.group_name), 12.0, t.ink2)].spacing(1).width(Fill),
                            ]
                            .spacing(10)
                            .align_y(Alignment::Center),
                            tx(format!("Accepting adds {first} as a contact for messages only. You can allow calls later."), 12.0, t.ink2),
                            row![
                                btn(t, Kind::Primary, "Accept", Some(l(Lm::AcceptReq(r.id.clone())))),
                                btn(t, Kind::Quiet, "Decline", Some(l(Lm::DeclineReq(r.id.clone())))),
                            ]
                            .spacing(6),
                            link_btn(t, &format!("Block {first}"), t.error, l(Lm::BlockReq(r.id.clone(), r.peer_did.clone()))),
                        ]
                        .spacing(8),
                    )
                    .padding(12)
                    .width(Fill)
                    .style(ui::outlined(t.surface, t.line, 12.0)),
                );
            }
        }
        if !sent.is_empty() {
            col = col.push(head("Sent requests".into()));
            for r in sent {
                let name = self.live_name(&r.peer_did, &r.peer_name);
                let (line, bad) = gl::sent_line(r);
                let mut row_ = row![
                    avatar(t, &name, &r.peer_did, 32.0),
                    column![
                        row![semi(name.clone(), 14.0, t.ink), Space::new().width(Fill), tx(short_time(r.at), 12.0, t.ink2)],
                        tx(line, 12.0, if bad { t.error } else { t.ink2 }),
                    ]
                    .spacing(1)
                    .width(Fill),
                ]
                .spacing(10)
                .align_y(Alignment::Center);
                if r.contact_added && self.contacts.iter().any(|c| c.did == r.peer_did) {
                    row_ = row_.push(link_btn(t, "Open chat", t.primary, Msg::Select(r.peer_did.clone())));
                }
                col = col.push(container(row_).padding([6, 12]));
            }
            col = col.push(container(tx("A request waits at the group\u{2019}s hosts for up to 7 days.", 12.0, t.ink2)).padding([0, 12]));
        }
        col = col.push(container(label(t, "Chats")).padding(Padding { top: 8.0, bottom: 4.0, left: 12.0, right: 0.0 }));
        Some(col.into())
    }

    // ---- settings ----

    /// Settings › Hosting: the groups this computer hosts.
    pub(super) fn live_hosting_page(&self, t: Tok) -> El<'_> {
        let mut c = column![tx(
            "A host keeps a full copy of a group and passes messages on while members\u{2019} phones sleep. Members connect to hosts, not to each other; only hosts learn members\u{2019} group addresses. Tinline keeps hosting while the window is closed (it stays in the tray).",
            14.0,
            t.ink2
        )]
        .spacing(16);
        let hosted: Vec<_> = self.live.list.iter().filter(|s| s.hosted_here).collect();
        if hosted.is_empty() {
            c = c.push(
                container(tx("This computer doesn\u{2019}t host any groups. To host one, open it and choose Host from this computer in the members pane.", 14.0, t.ink2))
                    .padding(16)
                    .width(Fill)
                    .style(ui::card(t)),
            );
            return c.into();
        }
        let mut list = column![].spacing(0);
        for (i, s) in hosted.iter().enumerate() {
            if i > 0 {
                list = list.push(rule(t));
            }
            let only = s.host_count <= 1;
            let sub = format!(
                "{} member{} \u{b7} {}",
                s.member_count,
                if s.member_count == 1 { "" } else { "s" },
                if only { "the only host".to_string() } else { format!("{} hosts", s.host_count) }
            );
            list = list.push(
                container(
                    row![
                        gav(t, &s.id, 36.0),
                        column![semi(s.name.clone(), 15.0, t.ink), tx(sub, 13.0, if only { t.thread_text } else { t.ink2 })].spacing(2).width(Fill),
                        link_btn(t, "Open", t.primary, l(Lm::Open(s.id.clone()))),
                        link_btn(t, "Stop hosting", t.error, l(Lm::StopHostingOf(s.id.clone()))),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([14, 16]),
            );
        }
        c.push(container(list).width(Fill).style(ui::card(t))).into()
    }

    // ---- dialogs ----

    pub(super) fn live_overlay(&self, t: Tok) -> Option<El<'_>> {
        let d = self.live.dialog.as_ref()?;
        let title = |x: String| bold(x, 22.0, t.ink);
        let para = |x: String| tx(x, 14.0, t.ink2);
        let busy = self.live.busy;
        let actions = |cancel: &str, ok: Option<(&str, Kind)>| -> El<'_> {
            let mut r = row![Space::new().width(Fill), btn(t, Kind::Ghost, cancel, (!busy).then(|| l(Lm::Dismiss)))].spacing(8);
            if let Some((label_, kind)) = ok {
                r = r.push(btn(t, kind, if busy { "Working\u{2026}" } else { label_ }, (!busy).then(|| l(Lm::Confirm))));
            }
            r.into()
        };
        let gname = self.live.detail.as_ref().map(|d| d.name.clone()).unwrap_or_default();
        let mut c: iced::widget::Column<'_, Msg> = match d {
            LDialog::NewGroup { name } => column![
                title("New group".into()),
                text_input("Group name", name)
                    .on_input(|v| l(Lm::NameIn(v)))
                    .on_submit(l(Lm::Confirm))
                    .padding(12)
                    .size(14)
                    .style(ui::input_style(t)),
                para("You\u{2019}ll own the group. This computer hosts it at first: it keeps the group and passes messages on while members\u{2019} phones sleep, so it works best on a computer that stays on.".into()),
                para("Then invite people from the members pane.".into()),
            ]
            .spacing(14),
            LDialog::Join { token, peek } => {
                let mut c = column![
                    title("Join a group".into()),
                    text_input("Paste the invite you were sent", token)
                        .on_input(|v| l(Lm::TokenIn(v)))
                        .on_paste(|v| l(Lm::TokenIn(v)))
                        .on_submit(l(Lm::Confirm))
                        .padding(12)
                        .size(13)
                        .font(ui::MONO)
                        .style(ui::input_style(t)),
                ]
                .spacing(14);
                match peek {
                    Some(p) => {
                        let inviter = match &p.issuer_did {
                            Some(did) => format!("Invited by {}", self.live_name(did, "someone who isn\u{2019}t in your contacts")),
                            None => "A link for your own other device".into(),
                        };
                        let owner = self.live_name(&p.owner_did, "someone who isn\u{2019}t in your contacts");
                        let mut info = column![
                            row![gav(t, &p.group_id, 36.0), bold(p.name.clone(), 17.0, t.ink)].spacing(10).align_y(Alignment::Center),
                            tx(format!("{inviter} \u{b7} owner {owner}"), 13.0, t.ink2),
                        ]
                        .spacing(6);
                        if let Some(exp) = p.expires {
                            info = info.push(tx(format!("Invite valid until {}", when(exp.saturating_mul(1000))), 13.0, t.ink2));
                        }
                        c = c.push(container(info).padding(12).width(Fill).style(ui::plain(t.surface2, 12.0))).push(para(
                            "Joining connects this computer to the group\u{2019}s hosts. Members see your name; only hosts learn how to reach this computer. It can take up to a minute."
                                .into(),
                        ));
                    }
                    None if !token.is_empty() => c = c.push(tx("That isn\u{2019}t a group invite. Paste the whole text you were sent.", 13.0, t.error)),
                    None => c = c.push(para("Admins of a group can make an invite. It works for 7 days.".into())),
                }
                c
            }
            LDialog::Token { kind, text } => {
                let (head, body) = match kind {
                    TokenKind::Invite => (
                        format!("Invite people to {gname}"),
                        "Send this to the people you want in the group, over an app you trust. Anyone holding it can join until it expires in 7 days; you can see who joined and flag them out.",
                    ),
                    TokenKind::Device => (
                        format!("Add your other device to {gname}"),
                        "On your other device (same account), open Groups \u{203a} Join and paste or scan this. It joins as you, not as a new member.",
                    ),
                };
                let mut c = column![title(head), para(body.into())].spacing(14);
                match (text, &self.live.qr) {
                    (Some(tok), q) => {
                        if let Some(q) = q {
                            c = c.push(
                                container(
                                    container(canvas(super::QrView(q)).width(Length::Fixed(220.0)).height(Length::Fixed(220.0)))
                                        .padding(8)
                                        .style(ui::plain(Color::WHITE, 12.0)),
                                )
                                .center_x(Fill),
                            );
                        }
                        c = c.push(
                            container(mono(tok.clone(), 11.0, t.ink2).wrapping(iced::widget::text::Wrapping::Glyph))
                                .padding(10)
                                .width(Fill)
                                .max_height(90)
                                .clip(true)
                                .style(ui::plain(t.surface2, 10.0)),
                        );
                        c = c.push(row![
                            Space::new().width(Fill),
                            btn(t, Kind::Ghost, "Close", Some(l(Lm::Dismiss))),
                            pill(t, Kind::Primary, Some(Icon::Copy), "Copy", Some(l(Lm::CopyToken))),
                        ]
                        .spacing(8));
                    }
                    (None, _) if self.live.error.is_none() => c = c.push(para("Making it\u{2026}".into())),
                    (None, _) => c = c.push(row![Space::new().width(Fill), btn(t, Kind::Ghost, "Close", Some(l(Lm::Dismiss)))]),
                }
                c
            }
            LDialog::Flag { did } => {
                let name = self.live.member(did).map(|m| self.live_name(&m.did, &m.name)).unwrap_or_else(|| "this member".into());
                let first = first_name(&name).to_string();
                let (yours, removes) = self.live.flag_after(did);
                let mut meter = row![].spacing(6);
                for i in 1..=FLAGS_TO_REMOVE {
                    let c = if i < yours { ui::alpha(t.error, 0.55) } else if i == yours { t.error } else { t.surface3 };
                    meter = meter.push(container(Space::new()).width(Fill).height(8).style(ui::plain(c, 4.0)));
                }
                column![
                    tx(gname.clone(), 13.0, t.ink2),
                    title(format!("Flag {first} for removal?")),
                    meter,
                    para(if removes {
                        format!("Yours will be flag {yours} of {FLAGS_TO_REMOVE}. {first} is removed right away: hosts refuse their new messages and they see they were removed.")
                    } else {
                        format!("Yours will be flag {yours} of {FLAGS_TO_REMOVE}. Any admin\u{2019}s flags count, yours again too. At {FLAGS_TO_REMOVE} flags {first} is removed right away and sees they were removed.")
                    }),
                    para(format!("A flag is signed by you and can\u{2019}t be taken back. {first} can see who flagged them and when. Messages {first} wrote before the removal still show, marked.")),
                ]
                .spacing(14)
            }
            LDialog::Admin { did, admin } => {
                let name = self.live.member(did).map(|m| self.live_name(&m.did, &m.name)).unwrap_or_else(|| "this member".into());
                let first = first_name(&name).to_string();
                if *admin {
                    column![
                        title(format!("Make {first} an admin?")),
                        para(format!("{first} will be able to invite people, flag members (3 flags remove someone) and appoint hosts. Only you, the owner, can make or remove admins.")),
                    ]
                    .spacing(14)
                } else {
                    column![title(format!("Remove {first} as admin?")), para(format!("{first} stays in the group as a member. Flags they already made still count."))].spacing(14)
                }
            }
            LDialog::Request { did } => {
                let name = self.live.member(did).map(|m| self.live_name(&m.did, &m.name)).unwrap_or_else(|| "this member".into());
                let first = first_name(&name).to_string();
                column![
                    row![
                        avatar(t, &name, did, 40.0),
                        column![title(format!("Ask {first} for a private chat")), tx(format!("Member of {gname}"), 13.0, t.ink2)].spacing(2),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                    para(format!("{first} sees your name and the group. The request goes through the group\u{2019}s host sealed, so the host can\u{2019}t read it.")),
                    para(format!("If {first} accepts, you can message each other directly. Calls stay off until one of you allows them.")),
                ]
                .spacing(14)
            }
            LDialog::Leave => column![
                title(format!("Leave {gname}?")),
                para("Members see that you left. The messages stay on this computer, read-only. To come back, an admin has to invite you again.".into()),
            ]
            .spacing(14),
            LDialog::Host { accept: true } => column![
                title(format!("Host {gname} on this computer?")),
                para("As a host this computer will:".into()),
                para("\u{2022} keep a full copy of the group\n\u{2022} pass messages on for members while their phones sleep\n\u{2022} learn members\u{2019} group addresses (only hosts do)\n\u{2022} need to stay running to be useful".into()),
                para("It starts hosting once an admin appoints it (if one already did, right away). Hosting doesn\u{2019}t make you an admin; you can stop any time.".into()),
            ]
            .spacing(14),
            LDialog::Host { accept: false } => {
                let only = self.live.detail.as_ref().is_some_and(|d| d.am_host && d.hosts.len() == 1);
                let mut c = column![
                    title(format!("Stop hosting {gname} here?")),
                    para("This computer stops passing the group\u{2019}s messages on. You stay a member.".into()),
                ]
                .spacing(14);
                if only {
                    c = c.push(tx(format!("{gname} has no other host: messages will wait on people\u{2019}s devices until another one hosts it."), 14.0, t.error));
                }
                c
            }
            LDialog::Removal => {
                let r = self.live.detail.as_ref().and_then(|d| d.removal.as_ref())?;
                let mut c = column![title("Why you were removed".into())].spacing(12);
                let n = r.flags.len();
                for (i, f) in r.flags.iter().enumerate() {
                    let who = self.live_name(&f.by_did, &f.by_name);
                    let mut sub = format!("Flagged {}", when(f.at));
                    if i + 1 == n {
                        sub += &format!(" \u{b7} {} flag", ordinal(n));
                    }
                    c = c.push(
                        row![avatar(t, &who, &f.by_did, 32.0), column![semi(who.clone(), 14.0, t.ink), tx(sub, 12.0, t.ink2)].spacing(1)]
                            .spacing(10)
                            .align_y(Alignment::Center),
                    );
                }
                c.push(tx("Each flag is signed by the admin who made it and was checked on this computer. Nobody else could have added them.", 13.0, t.ink2))
                    .push(para("You keep the messages already here. The group\u{2019}s hosts refuse your new messages. To rejoin, an admin has to invite you again.".into()))
                    .push(row![Space::new().width(Fill), btn(t, Kind::Primary, "Close", Some(l(Lm::Dismiss)))])
            }
        };
        if let Some(e) = &self.live.error {
            c = c.push(tx(e.clone(), 14.0, t.error));
        }
        c = match d {
            LDialog::NewGroup { .. } => c.push(actions("Cancel", Some(("Create", Kind::Primary)))),
            LDialog::Join { peek, .. } => c.push(actions("Cancel", peek.as_ref().map(|_| ("Join", Kind::Primary)))),
            LDialog::Flag { .. } => c.push(actions("Cancel", Some(("Flag", Kind::Danger)))),
            LDialog::Admin { admin, .. } => c.push(actions("Cancel", Some((if *admin { "Make admin" } else { "Remove as admin" }, Kind::Primary)))),
            LDialog::Leave => c.push(actions("Cancel", Some(("Leave", Kind::Danger)))),
            LDialog::Request { .. } => c.push(actions("Cancel", Some(("Send request", Kind::Primary)))),
            LDialog::Host { accept } => c.push(actions("Cancel", Some(if *accept { ("Host here", Kind::Primary) } else { ("Stop hosting", Kind::Danger) }))),
            LDialog::Token { .. } | LDialog::Removal => c,
        };
        let card = container(c).padding(28).width(540).style(ui::plain(t.surface, 20.0));
        let layer = container(opaque(card))
            .center_x(Fill)
            .center_y(Fill)
            .width(Fill)
            .height(Fill)
            .style(ui::plain(Color { r: 0.06, g: 0.08, b: 0.07, a: 0.42 }, 0.0));
        Some(opaque(mouse_area(layer).on_press(l(Lm::Dismiss))))
    }
}
