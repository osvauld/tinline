//! Groups & privacy PREVIEW screens (placeholder data, see `groups_fake`), laid out after
//! docs/design/groups/DesktopGroups.dc.html, with behaviour from the Android boards beside it:
//! group chat with the members pane, member actions, flag/block/request dialogs, the 1:1
//! states (blocked, blocked you, messages only), the contact panel, Settings › Hosting and
//! Settings › Privacy (address changes, blocked people, the preview switch).

use chrono::{Local, TimeZone};
use iced::widget::{button, column, container, mouse_area, opaque, row, text_input, Space};

use super::*;
use crate::app::groups::{Dialog, Gm, Open};
use crate::groups_fake::{
    self as gf, first_name, Block, Group, GroupDelivery, GroupItem, Host, Interval, Known, Member, PDelivery, Perm, Person, Role,
};

fn g(m: Gm) -> Msg {
    Msg::Groups(m)
}

fn local_fmt(ms: u64, fmt: &str) -> String {
    Local.timestamp_millis_opt(ms as i64).single().map(|d| d.format(fmt).to_string()).unwrap_or_default()
}

/// "Oct 10, 15:20".
fn when(ms: u64) -> String {
    local_fmt(ms, "%b %-d, %H:%M")
}

/// "Oct 12".
fn day(ms: u64) -> String {
    local_fmt(ms, "%b %-d")
}

fn hhmm(ms: u64) -> String {
    local_fmt(ms, "%H:%M")
}

fn ago_ms(ms: u64) -> String {
    ago(now_secs(), ms / 1000)
}

fn mb(bytes: u64) -> String {
    if bytes >= 1_000_000_000 { format!("{:.1} GB", bytes as f64 / 1e9) } else { format!("{} MB", (bytes / 1_000_000).max(1)) }
}

/// "Sam Rao, Meera K and Priya N".
fn and_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [a] => a.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn rule<'a>(t: Tok) -> El<'a> {
    container(Space::new()).width(Fill).height(1).style(ui::plain(t.line, 0.0)).into()
}

/// The group avatar: a rounded square, not a circle.
fn gav<'a>(t: Tok, key: &str, size: f32) -> El<'a> {
    let (bg, fg) = ui::avatar_colors(t, key);
    container(ui::icon(Icon::Users, size * 0.5, fg))
        .width(size)
        .height(size)
        .center_x(size)
        .center_y(size)
        .style(ui::plain(bg, size * 0.28))
        .into()
}

fn dot<'a>(c: Color) -> El<'a> {
    container(Space::new()).width(8).height(8).style(ui::plain(c, 4.0)).into()
}

/// Owner / Admin / Host pills and the like.
fn tag<'a>(bg: Color, fg: Color, s: &str) -> El<'a> {
    container(semi(s.to_string(), 11.0, fg)).padding([2, 7]).style(ui::plain(bg, 999.0)).into()
}

fn role_tag<'a>(t: Tok, r: Role) -> Option<El<'a>> {
    match r {
        Role::Owner => Some(tag(t.primary_c, t.on_primary_c, "Owner")),
        Role::Admin => Some(tag(t.relay_c, t.on_relay_c, "Admin")),
        Role::Member => None,
    }
}

/// A small grey line in a conversation.
fn sys<'a>(t: Tok, s: &str) -> El<'a> {
    container(container(tx(s.to_string(), 12.0, t.ink2)).padding([4, 12]).style(ui::plain(t.surface2, 999.0)))
        .center_x(Fill)
        .padding([4, 0])
        .into()
}

/// A quiet text button.
fn link_btn<'a>(t: Tok, s: &str, color: Color, on: Msg) -> El<'a> {
    button(semi(s.to_string(), 13.0, color)).padding([4, 8]).style(ui::button_style(t, Kind::Ghost, 999.0)).on_press(on).into()
}

/// A text-only pill for the outlined/text buttons of the boards.
fn btn<'a>(t: Tok, kind: Kind, s: &str, on: Option<Msg>) -> El<'a> {
    pill(t, kind, None, s, on)
}

fn composer_input<'a>(t: Tok, placeholder: &str, value: &str) -> El<'a> {
    row![
        text_input(placeholder, value)
            .on_input(|v| g(Gm::Input(v)))
            .on_submit(g(Gm::Send))
            .padding([11, 16])
            .size(14)
            .style(move |th, st| {
                let mut s = ui::input_style(t)(th, st);
                s.border.radius = 22.0.into();
                s
            }),
        pill(t, Kind::Primary, Some(Icon::Send), "Send", (!value.trim().is_empty()).then_some(g(Gm::Send))),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// A footer in place of the composer.
fn footer<'a>(t: Tok, body: El<'a>) -> El<'a> {
    column![rule(t), container(body).padding([12, 20]).width(Fill)].into()
}

impl App {
    fn preview_name(&self, did: &str, fallback: &str) -> String {
        if did == gf::ME {
            return "You".into();
        }
        self.groups.snap.person(did).map(|p| p.name.clone()).unwrap_or_else(|| fallback.to_string())
    }

    // ---- chats list ----

    /// Preview rows for the Chats tab, as (last activity, row), to merge with the real chats.
    pub(super) fn preview_rows(&self, t: Tok, q: &str) -> Vec<(u64, El<'_>)> {
        if !self.groups.on {
            return Vec::new();
        }
        let s = &self.groups.snap;
        let mut out = Vec::new();
        for gr in &s.groups {
            if !q.is_empty() && !gr.name.to_lowercase().contains(q) {
                continue;
            }
            let sel = self.groups.open.as_ref() == Some(&Open::Group(gr.id.clone()));
            let removed = gr.removed.is_some();
            let preview = if removed {
                "You were removed from this group".to_string()
            } else {
                match gr.items.last() {
                    Some(GroupItem::Msg { author_did, author, text, .. }) if s.blocked_by_me(author_did) => {
                        let _ = (author, text);
                        "Message from someone you blocked".to_string()
                    }
                    Some(GroupItem::Msg { outgoing: true, text, .. }) => format!("You: {text}"),
                    Some(GroupItem::Msg { author, text, .. }) => format!("{}: {text}", first_name(author)),
                    Some(GroupItem::Note { text, .. }) => text.clone(),
                    None => String::new(),
                }
            };
            let fade = |c: Color| if removed { ui::alpha(c, 0.55) } else { c };
            let (fg, dim) = if sel { (t.on_primary_c, t.on_primary_c) } else { (fade(t.ink), fade(t.ink2)) };
            let unread = gr.unread > 0 && !sel;
            let mut sub = row![container(tx(preview, 13.0, dim).wrapping(iced::widget::text::Wrapping::None)).width(Fill).clip(true)]
                .spacing(4)
                .align_y(Alignment::Center);
            if unread {
                sub = sub.push(container(semi(gr.unread.to_string(), 12.0, t.on_primary)).padding([1, 7]).style(ui::plain(t.primary, 999.0)));
            }
            let name = if unread { bold(gr.name.clone(), 15.0, fg) } else { semi(gr.name.clone(), 15.0, fg) };
            out.push((
                gr.last_at(),
                button(
                    row![
                        gav(t, &gr.id, 40.0),
                        column![row![name, Space::new().width(Fill), tx(short_time(gr.last_at()), 12.0, dim)], sub].spacing(2).width(Fill),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .width(Fill)
                .style(ui::row_style(t, sel))
                .on_press(g(Gm::OpenGroup(gr.id.clone())))
                .into(),
            ));
        }
        for p in s.people.iter().filter(|p| p.contact) {
            if !q.is_empty() && !p.name.to_lowercase().contains(q) {
                continue;
            }
            let sel = self.groups.open.as_ref() == Some(&Open::Person(p.did.clone()));
            let (fg, dim) = if sel { (t.on_primary_c, t.on_primary_c) } else { (t.ink, t.ink2) };
            let (sub, color) = match p.block {
                Block::ByMe { .. } => ("Blocked".to_string(), t.error),
                Block::ByThem { .. } => ("Blocked you".to_string(), t.error),
                Block::None if p.messages_only() => ("Messages only".to_string(), dim),
                Block::None => (p.msgs.iter().rev().find(|m| !m.note).map(|m| m.text.clone()).unwrap_or_default(), dim),
            };
            out.push((
                p.last_at(),
                button(
                    row![
                        avatar(t, &p.name, &p.did, 40.0),
                        column![
                            row![semi(p.name.clone(), 15.0, fg), Space::new().width(Fill), tx(short_time(p.last_at()), 12.0, dim)],
                            container(tx(sub, 13.0, if sel { dim } else { color }).wrapping(iced::widget::text::Wrapping::None)).width(Fill).clip(true),
                        ]
                        .spacing(2)
                        .width(Fill),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .width(Fill)
                .style(ui::row_style(t, sel))
                .on_press(g(Gm::OpenPerson(p.did.clone())))
                .into(),
            ));
        }
        out
    }

    /// Host requests, incoming chat requests and our sent ones: blocks above the chats, never
    /// mixed into conversations.
    pub(super) fn requests_block(&self, t: Tok) -> Option<El<'_>> {
        if !self.groups.on {
            return None;
        }
        let s = &self.groups.snap;
        if s.host_requests.is_empty() && s.incoming.is_empty() && s.sent.is_empty() {
            return None;
        }
        let head = |s_: String| container(label(t, &s_)).padding(Padding { top: 8.0, bottom: 4.0, left: 12.0, right: 0.0 });
        fn card<'a>(t: Tok, c: El<'a>) -> El<'a> {
            container(c).padding(12).width(Fill).style(ui::outlined(t.surface, t.line, 12.0)).into()
        }
        let mut col = column![].spacing(6);
        if !s.host_requests.is_empty() {
            col = col.push(head("Host requests".into()));
            for r in &s.host_requests {
                col = col.push(card(
                    t,
                    column![
                        row![ui::icon(Icon::Server, 18.0, t.primary), semi(format!("Host {}?", r.group_name), 14.0, t.ink)]
                            .spacing(8)
                            .align_y(Alignment::Center),
                        tx(format!("{} asked this computer to host", first_name(&r.from_name)), 13.0, t.ink2),
                        row![
                            btn(t, Kind::Primary, "Review", Some(g(Gm::ReviewHostRequest(r.id.clone())))),
                            btn(t, Kind::Ghost, "Decline", Some(g(Gm::DeclineHostRequest(r.id.clone())))),
                        ]
                        .spacing(6),
                    ]
                    .spacing(6)
                    .into(),
                ));
            }
        }
        if !s.incoming.is_empty() {
            col = col.push(head(format!("Requests \u{b7} {}", s.incoming.len())));
            for r in &s.incoming {
                let first = first_name(&r.from_name).to_string();
                let shared = if r.shared_groups == 1 { "you share 1 group".to_string() } else { format!("you share {} groups", r.shared_groups) };
                let mut c = column![
                    row![
                        avatar(t, &r.from_name, &r.from_did, 36.0),
                        column![semi(r.from_name.clone(), 14.0, t.ink), tx(format!("Member of {} \u{b7} {shared}", r.group), 12.0, t.ink2)]
                            .spacing(1)
                            .width(Fill),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                ]
                .spacing(8);
                if !r.note.is_empty() {
                    c = c.push(container(tx(r.note.clone(), 13.0, t.ink)).padding([6, 10]).width(Fill).style(ui::plain(t.surface2, 10.0)));
                }
                c = c
                    .push(tx(format!("Accepting adds {first} as a contact for messages only. You can allow calls later."), 12.0, t.ink2))
                    .push(
                        row![
                            btn(t, Kind::Primary, "Accept", Some(g(Gm::AcceptReq(r.id.clone())))),
                            btn(t, Kind::Quiet, "Decline", Some(g(Gm::DeclineReq(r.id.clone())))),
                        ]
                        .spacing(6),
                    )
                    .push(link_btn(t, &format!("Block {first}"), t.error, g(Gm::BlockReq(r.id.clone()))));
                col = col.push(card(t, c.into()));
            }
        }
        if !s.sent.is_empty() {
            col = col.push(head("Sent requests".into()));
            for r in &s.sent {
                let first = first_name(&r.to_name).to_string();
                let (status, color) = match r.state {
                    gf::SentState::Waiting => (format!("Waiting for {first} \u{b7} via {}", r.group), t.ink2),
                    gf::SentState::Declined => (format!("{first} declined"), t.ink2),
                    gf::SentState::Expired => ("No answer after 7 days \u{b7} request expired".to_string(), t.error),
                };
                let mut c = column![
                    row![
                        avatar(t, &r.to_name, &r.to_did, 32.0),
                        column![
                            row![semi(r.to_name.clone(), 14.0, t.ink), Space::new().width(Fill), tx(ago_ms(r.at), 12.0, t.ink2)],
                            tx(status, 12.0, color),
                        ]
                        .spacing(1)
                        .width(Fill),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                ]
                .spacing(4);
                let mut actions = row![].spacing(2);
                if r.state == gf::SentState::Waiting {
                    actions = actions
                        .push(link_btn(t, "Cancel", t.ink2, g(Gm::CancelReq(r.id.clone()))))
                        .push(link_btn(t, "Preview: accepts", t.relay, g(Gm::SimReply(r.id.clone(), true))))
                        .push(link_btn(t, "declines", t.relay, g(Gm::SimReply(r.id.clone(), false))));
                } else {
                    actions = actions.push(link_btn(t, "Clear", t.ink2, g(Gm::CancelReq(r.id.clone()))));
                }
                c = c.push(actions);
                col = col.push(container(c).padding([6, 12]));
            }
            col = col.push(
                container(tx("You can send one request per person per day. A request expires after 7 days.", 12.0, t.ink2)).padding([0, 12]),
            );
            if s.sent.iter().any(|r| r.state == gf::SentState::Waiting) {
                col = col.push(container(link_btn(t, "Preview: a week passes", t.relay, g(Gm::SimWeek))).padding([0, 4]));
            }
        }
        col = col.push(head("Chats".into()));
        Some(col.into())
    }

    // ---- the main pane ----

    pub(super) fn preview_pane(&self, t: Tok) -> Option<El<'_>> {
        if !self.groups.on {
            return None;
        }
        match self.groups.open.as_ref()? {
            Open::Group(id) => self.groups.snap.group(id).map(|gr| self.group_view(t, gr)),
            Open::Person(did) => self.groups.snap.person(did).map(|p| self.person_view(t, p)),
        }
    }

    fn top_extras<'a>(&'a self, t: Tok, mut col: iced::widget::Column<'a, Msg>) -> iced::widget::Column<'a, Msg> {
        if self.notice.is_some() {
            col = col.push(container(self.notice_bar(t)).padding([8, 20]));
        }
        col
    }

    fn group_view<'a>(&'a self, t: Tok, gr: &'a Group) -> El<'a> {
        let removed = gr.removed.is_some();
        let n = gr.members.len();
        let hosts = gr.active_hosts().count();
        let (sub, sub_color) = if removed {
            ("You\u{2019}re no longer a member".to_string(), t.error)
        } else if !gr.reachable() {
            ("Waiting for a host".to_string(), t.thread_text)
        } else if gr.this_computer_hosts() {
            (format!("{n} members \u{b7} this computer hosts this group"), t.ink2)
        } else {
            (format!("{n} members \u{b7} hosted by {hosts} desktop{}", if hosts == 1 { "" } else { "s" }), t.ink2)
        };
        let mut head = row![
            gav(t, &gr.id, 36.0),
            column![bold(gr.name.clone(), 17.0, t.ink), tx(sub, 13.0, sub_color)].spacing(1).width(Fill),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        if !removed {
            let on = self.groups.members;
            head = head.push(
                button(container(ui::icon(Icon::Users, 20.0, if on { t.on_primary_c } else { t.ink2 })).center_x(36).center_y(36))
                    .padding(0)
                    .width(36)
                    .height(36)
                    .style(ui::button_style(t, if on { Kind::Tonal } else { Kind::Ghost }, 8.0))
                    .on_press(g(Gm::ToggleMembers)),
            );
        }
        let mut top = column![container(head).padding([10, 20]).width(Fill), rule(t)];
        top = self.top_extras(t, top);
        if !removed && !gr.reachable() {
            let body = if hosts == 0 {
                "This group has no host yet. Your messages are saved and will send when a desktop hosts it."
            } else {
                "No host desktop is online. Your messages are saved and will send when one comes back."
            };
            top = top.push(container(banner(t, "Waiting for a host", body, None, false)).padding([12, 20]));
        }

        let mut list = column![].spacing(8).padding([16, 24]);
        for it in &gr.items {
            list = list.push(self.group_item(t, gr, it));
        }
        let msgs = scroll(t, list).anchor_bottom().width(Fill).height(Fill);

        let bottom: El = if let Some(r) = &gr.removed {
            let _ = r;
            let actions: El = if gr.archived {
                tx("Kept as an archive. Nothing new will arrive.", 13.0, t.ink2).into()
            } else {
                row![
                    btn(t, Kind::Danger, "Delete group", Some(g(Gm::DeleteGroup(gr.id.clone())))),
                    btn(t, Kind::Quiet, "Keep as archive", Some(g(Gm::ArchiveGroup(gr.id.clone())))),
                ]
                .spacing(8)
                .into()
            };
            footer(
                t,
                row![tx("You can\u{2019}t send messages to this group.", 14.0, t.ink2).width(Fill), actions].spacing(12).align_y(Alignment::Center).into(),
            )
        } else {
            footer(t, composer_input(t, "Message the group", &self.groups.input))
        };
        let conv = column![top, msgs, bottom].width(Fill).height(Fill);
        if removed || !self.groups.members {
            return conv.into();
        }
        row![conv, self.members_pane(t, gr)].into()
    }

    fn group_item<'a>(&'a self, t: Tok, gr: &'a Group, it: &'a GroupItem) -> El<'a> {
        match it {
            GroupItem::Note { text, .. } if gr.removed.is_some() && text.starts_with("You were removed") => self.removal_card(t, gr),
            GroupItem::Note { text, .. } => sys(t, text),
            GroupItem::Msg { author_did, author, text, at, outgoing, delivery, before_removal, .. } => {
                if *outgoing {
                    let mut meta = row![tx(hhmm(*at), 11.0, ui::alpha(t.on_primary_c, 0.7))].spacing(4).align_y(Alignment::Center);
                    let dim = ui::alpha(t.on_primary_c, 0.75);
                    match delivery {
                        GroupDelivery::Sent => meta = meta.push(ui::icon(Icon::Check, 14.0, dim)),
                        GroupDelivery::WaitingForHost => {
                            meta = meta.push(ui::icon(Icon::Clock, 13.0, dim)).push(tx("Waiting for a host", 11.0, dim));
                        }
                        GroupDelivery::BeforeRemoval { until } => {
                            meta = meta.push(tx(format!("Written offline before removal \u{b7} sending until {}", day(*until)), 11.0, dim));
                        }
                    }
                    let b = container(column![message_body(text, t.on_primary_c), meta].spacing(4).align_x(Alignment::End))
                        .padding([8, 12])
                        .max_width(440)
                        .style(ui::plain(t.primary_c, 16.0));
                    return row![Space::new().width(Fill), b].into();
                }
                let name = self.preview_name(author_did, author);
                if self.groups.snap.blocked_by_me(author_did) {
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
                let removed_author = *before_removal;
                let (_, color) = ui::avatar_colors(t, author_did);
                let who = if removed_author { format!("{} \u{b7} removed", first_name(&name)) } else { first_name(&name).to_string() };
                let mut c = column![bold(who, 12.0, if removed_author { t.ink2 } else { color }), message_body(text, t.ink)].spacing(2);
                let mut meta = row![tx(hhmm(*at), 11.0, t.ink2)].spacing(4);
                if removed_author {
                    meta = meta.push(tx(format!("\u{b7} written before {} was removed", first_name(&name)), 11.0, t.ink2));
                }
                c = c.push(meta);
                let b = container(c).padding([8, 12]).max_width(440).style(ui::outlined(t.surface, t.line, 16.0));
                row![b, Space::new().width(Fill)].into()
            }
        }
    }

    /// In the removed person's chat: when, why and by whom, plainly.
    fn removal_card<'a>(&'a self, t: Tok, gr: &'a Group) -> El<'a> {
        let Some(r) = &gr.removed else { return Space::new().into() };
        let names: Vec<String> = r.flags.iter().map(|f| f.by_name.clone()).collect();
        container(
            column![
                semi(format!("You were removed from the group \u{b7} {}", when(r.at)), 14.0, t.on_error_c),
                bold(format!("Removed after {} admin flags", r.flags.len()), 15.0, t.on_error_c),
                tx(
                    format!("Flagged by {}. You can still read what you received before; nothing new will arrive.", and_list(&names)),
                    13.0,
                    t.on_error_c
                ),
                link_btn(t, "See details", t.on_error_c, g(Gm::RemovalDetails(gr.id.clone()))),
            ]
            .spacing(4),
        )
        .padding([12, 16])
        .max_width(520)
        .style(ui::plain(t.error_c, 14.0))
        .into()
    }

    fn members_pane<'a>(&'a self, t: Tok, gr: &'a Group) -> El<'a> {
        let sec = |s: &str| container(label(t, s)).padding(Padding { top: 14.0, bottom: 6.0, left: 16.0, right: 16.0 });
        let mut col = column![sec("Hosts")].spacing(0);
        for h in &gr.hosts {
            col = col.push(self.host_row(t, gr, h));
        }
        if gr.active_hosts().next().is_none() {
            col = col.push(container(tx("Needs a host", 13.0, t.error)).padding([4, 16]));
        }
        col = col.push(sec(&format!("Members \u{b7} {}", gr.members.len())));
        let admin = gr.am_admin();
        for m in &gr.members {
            let me = m.did == gf::ME;
            let name = if me { "You".to_string() } else { self.preview_name(&m.did, &m.name) };
            let mut r = row![avatar(t, &name, &m.did, 28.0), semi(name.clone(), 14.0, t.ink).width(Fill)].spacing(10).align_y(Alignment::Center);
            if self.groups.snap.blocked_by_me(&m.did) {
                r = r.push(tx("Blocked", 12.0, t.error));
            }
            if admin && !m.flags.is_empty() {
                let n = m.flags.len();
                r = r.push(tag(t.error_c, t.on_error_c, &format!("{n} flag{}", if n == 1 { "" } else { "s" })));
            }
            if let Some(p) = role_tag(t, m.role) {
                r = r.push(p);
            }
            if m.host {
                r = r.push(tag(t.warn, t.on_warn, "Host"));
            }
            let open = self.groups.menu.as_deref() == Some(m.did.as_str());
            let b = button(r)
                .padding([8, 16])
                .width(Fill)
                .style(ui::row_style(t, open))
                .on_press_maybe((!me).then(|| g(Gm::Menu(Some(m.did.clone())))));
            col = col.push(if me { Element::from(b) } else { mouse_area(b).on_right_press(g(Gm::Menu(Some(m.did.clone())))).into() });
            if open {
                col = col.push(container(self.member_menu(t, gr, m)).padding([4, 12]));
            }
        }
        let foot = container(
            row![
                button(container(semi("Invite", 14.0, t.primary)).center_x(Fill))
                    .padding([10, 16])
                    .width(Fill)
                    .style(move |th, st| {
                        let mut s = ui::button_style(t, Kind::Ghost, 999.0)(th, st);
                        s.border = Border { color: t.line, width: 1.0, radius: 999.0.into() };
                        s.text_color = t.primary;
                        s
                    })
                    .on_press(g(Gm::Placeholder("Invite"))),
                link_btn(t, "Group settings", t.primary, g(Gm::Placeholder("Group settings"))),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([12, 16]);
        container(column![scroll(t, col).height(Fill), rule(t), foot])
            .width(280)
            .height(Fill)
            .style(ui::sidebar(t))
            .into()
    }

    fn host_row<'a>(&'a self, t: Tok, gr: &'a Group, h: &'a Host) -> El<'a> {
        let (c, right): (Color, String) = if h.pending {
            (t.thread, "asked".into())
        } else if h.this_computer {
            (t.primary, format!("{} online", h.connected))
        } else if h.online {
            (t.primary, "online".into())
        } else {
            let d = now_secs().saturating_sub(h.last_seen / 1000);
            (t.ink2, if d >= 86400 { format!("offline {} d", d / 86400) } else { format!("offline {} h", (d / 3600).max(1)) })
        };
        let mut r = row![dot(c), tx(h.label.clone(), 14.0, t.ink).width(Fill), tx(right, 13.0, t.ink2)].spacing(10).align_y(Alignment::Center);
        if h.pending {
            r = r.push(link_btn(t, "Preview: accept", t.relay, g(Gm::SimHostAccept(gr.id.clone(), h.owner_did.clone()))));
        }
        container(r).padding([0, 16]).height(40).center_y(40).into()
    }

    /// Right-click (or click) on a member: the phone's action sheet, as a menu.
    fn member_menu<'a>(&'a self, t: Tok, gr: &'a Group, m: &'a Member) -> El<'a> {
        let first = first_name(&m.name).to_string();
        let admin = gr.am_admin();
        let item = |i: Icon, title: String, sub: String, color: Color, on: Option<Msg>| -> El<'a> {
            let fg = if on.is_some() { color } else { ui::alpha(color, 0.5) };
            button(
                row![ui::icon(i, 18.0, fg), column![semi(title, 14.0, fg), tx(sub, 12.0, t.ink2)].spacing(1).width(Fill)]
                    .spacing(10)
                    .align_y(Alignment::Center),
            )
            .padding([8, 10])
            .width(Fill)
            .style(ui::row_style(t, false))
            .on_press_maybe(on)
            .into()
        };
        let mut head = format!("Member since {} \u{b7} invited by {}", day(m.since), if m.invited_by.is_empty() { "nobody" } else { first_name(&m.invited_by) });
        if admin {
            head += &format!(" \u{b7} {} of {} flags", m.flags.len(), gf::FLAGS_TO_REMOVE);
        }
        let contact = self.groups.snap.is_contact(&m.did);
        let blocked = self.groups.snap.blocked_by_me(&m.did);
        let mut c = column![container(tx(head, 12.0, t.ink2)).padding([4, 10])].spacing(2);
        c = c.push(item(
            Icon::MessageSquare,
            "Message privately".into(),
            if contact { "Opens your chat".into() } else { format!("Sends {first} a chat request") },
            t.ink,
            (!blocked).then(|| g(Gm::MessagePrivately(gr.id.clone(), m.did.clone()))),
        ));
        if admin {
            if !m.role.is_admin() {
                c = c.push(item(Icon::ShieldCheck, "Make admin".into(), "Can flag members and appoint hosts".into(), t.ink, Some(g(Gm::MakeAdmin(gr.id.clone(), m.did.clone())))));
            }
            let has_host = gr.hosts.iter().any(|h| h.owner_did == m.did);
            let (sub, on) = match (&m.desktop, has_host) {
                (_, true) => ("One of their desktops already hosts or was asked".to_string(), None),
                (Some(_), false) => (format!("Only for one of {first}\u{2019}s desktops"), Some(g(Gm::MakeHost(gr.id.clone(), m.did.clone())))),
                (None, false) => (format!("{first} has no linked desktop; phones can\u{2019}t host"), None),
            };
            c = c.push(item(Icon::Server, "Make host".into(), sub, t.ink, on));
        }
        // Block is for contacts (U19); it never removes anyone from the group.
        if blocked {
            c = c.push(item(Icon::Ban, format!("Unblock {first}"), "They can message you again".into(), t.ink, Some(g(Gm::Unblock(m.did.clone())))));
        } else if contact {
            c = c.push(item(Icon::Ban, format!("Block {first}"), format!("Just for you. {first} stays in the group"), t.error, Some(g(Gm::AskBlock(m.did.clone())))));
        }
        if admin && m.role != Role::Owner {
            c = c.push(item(
                Icon::Flag,
                "Flag to remove".into(),
                format!("{} flags remove {first} from the group", gf::FLAGS_TO_REMOVE),
                t.error,
                Some(g(Gm::AskFlag(gr.id.clone(), m.did.clone()))),
            ));
        }
        container(c).padding(6).width(Fill).style(ui::card(t)).into()
    }

    fn person_view<'a>(&'a self, t: Tok, p: &'a Person) -> El<'a> {
        let first = first_name(&p.name).to_string();
        let (sub, sub_color) = match p.block {
            Block::ByMe { .. } => ("Blocked".to_string(), t.error),
            Block::ByThem { .. } => ("Blocked you".to_string(), t.error),
            Block::None if p.messages_only() => ("Messages only".to_string(), t.ink2),
            Block::None => (if p.verified { "Contact \u{b7} verified".to_string() } else { "Contact".to_string() }, t.ink2),
        };
        let call_ok = p.block == Block::None;
        let head = row![
            avatar(t, &p.name, &p.did, 40.0),
            column![bold(p.name.clone(), 17.0, t.ink), tx(sub, 13.0, sub_color)].spacing(1).width(Fill),
            pill(t, if p.can_call_them() { Kind::Primary } else { Kind::Quiet }, Some(if p.can_call_them() { Icon::Phone } else { Icon::PhoneOff }), "Call", call_ok.then(|| g(Gm::Call(p.did.clone())))),
            button(container(ui::icon(Icon::ShieldCheck, 20.0, if self.groups.panel { t.on_primary_c } else { t.ink2 })).center_x(36).center_y(36))
                .padding(0)
                .width(36)
                .height(36)
                .style(ui::button_style(t, if self.groups.panel { Kind::Tonal } else { Kind::Ghost }, 8.0))
                .on_press(g(Gm::TogglePanel)),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let top = self.top_extras(t, column![container(head).padding([10, 20]).width(Fill), rule(t)]);

        let mut list = column![].spacing(8).padding([16, 24]);
        for m in &p.msgs {
            if m.note {
                list = list.push(sys(t, &m.text));
                continue;
            }
            let (bg, fg) = if m.outgoing { (t.primary_c, t.on_primary_c) } else { (t.surface, t.ink) };
            let dim = ui::alpha(fg, 0.7);
            let mut meta = row![tx(hhmm(m.at), 11.0, dim)].spacing(4).align_y(Alignment::Center);
            let mut failed = None;
            if m.outgoing {
                match &m.delivery {
                    PDelivery::Read => meta = meta.push(ui::icon(Icon::CheckCheck, 14.0, t.primary)).push(tx("Read", 11.0, dim)),
                    PDelivery::Delivered => meta = meta.push(ui::icon(Icon::CheckCheck, 14.0, dim)),
                    PDelivery::NotDelivered(why) => failed = Some(why.clone()),
                }
            }
            if let Some(why) = failed {
                meta = meta.push(tx(format!("Not delivered: {why}"), 11.0, t.error));
            }
            let style = ui::outlined(bg, if m.outgoing { bg } else { t.line }, 16.0);
            let b = container(column![message_body(&m.text, fg), meta].spacing(4)).padding([8, 12]).max_width(440).style(style);
            list = list.push(if m.outgoing { row![Space::new().width(Fill), b] } else { row![b, Space::new().width(Fill)] });
        }
        match p.block {
            Block::ByMe { at, attempts, .. } => {
                list = list.push(sys(t, &format!("You blocked {first} \u{b7} {}", when(at))));
                if attempts > 0 {
                    list = list.push(sys(t, &format!("{first} tried to message you {attempts} time{} since", if attempts == 1 { "" } else { "s" })));
                }
                list = list.push(container(link_btn(t, "Preview: they try to message you", t.relay, g(Gm::SimAttempt(p.did.clone())))).center_x(Fill));
            }
            Block::ByThem { at } => {
                let groups: Vec<String> = self
                    .groups
                    .snap
                    .groups
                    .iter()
                    .filter(|gr| gr.removed.is_none() && gr.member(&p.did).is_some())
                    .map(|gr| gr.name.clone())
                    .collect();
                let mut body = "You can\u{2019}t message or call them. Your earlier messages stay here.".to_string();
                if !groups.is_empty() {
                    body += &format!(" You\u{2019}re both still in {}.", and_list(&groups));
                }
                list = list.push(
                    container(
                        container(
                            column![
                                bold(format!("{first} blocked you"), 15.0, t.on_error_c),
                                tx(body, 13.0, t.on_error_c),
                                tx(format!("Signed by {first} \u{b7} {}", when(at)), 12.0, ui::alpha(t.on_error_c, 0.8)),
                            ]
                            .spacing(4),
                        )
                        .padding([12, 16])
                        .max_width(460)
                        .style(ui::plain(t.error_c, 14.0)),
                    )
                    .center_x(Fill),
                );
            }
            Block::None => {
                if p.contact && p.calls_chip && !p.perms.call {
                    list = list.push(
                        container(
                            container(
                                row![
                                    tx(format!("Calls are off for {first}."), 13.0, t.ink2),
                                    link_btn(t, "Allow calls", t.primary, g(Gm::SetPerm(p.did.clone(), Perm::Call, true))),
                                    button(ui::icon(Icon::X, 14.0, t.ink2)).padding(4).style(ui::button_style(t, Kind::Ghost, 999.0)).on_press(g(Gm::DismissChip(p.did.clone()))),
                                ]
                                .spacing(4)
                                .align_y(Alignment::Center),
                            )
                            .padding([2, 6])
                            .style(ui::plain(t.surface2, 999.0)),
                        )
                        .center_x(Fill),
                    );
                }
            }
        }
        let msgs = scroll(t, list).anchor_bottom().width(Fill).height(Fill);
        let bottom: El = match p.block {
            Block::ByMe { .. } => footer(
                t,
                row![
                    tx(format!("You blocked {first}. They can\u{2019}t message or call you."), 14.0, t.ink2).width(Fill),
                    btn(t, Kind::Primary, "Unblock", Some(g(Gm::Unblock(p.did.clone())))),
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .into(),
            ),
            Block::ByThem { .. } => footer(t, tx(format!("You can\u{2019}t send messages to {first}."), 14.0, t.ink2).into()),
            Block::None => footer(t, composer_input(t, "Message", &self.groups.input)),
        };
        let conv = column![top, msgs, bottom].width(Fill).height(Fill);
        if !self.groups.panel {
            return conv.into();
        }
        row![conv, self.contact_panel(t, p)].into()
    }

    /// Separate Message and Call switches, a plain connection status, and Block (U19: no
    /// Remove for contacts).
    fn contact_panel<'a>(&'a self, t: Tok, p: &'a Person) -> El<'a> {
        let first = first_name(&p.name).to_string();
        let origin = match (&p.via_group, p.verified) {
            (Some(gr), true) => format!("From {gr} \u{b7} verified"),
            (Some(gr), false) => format!("From {gr}"),
            (None, true) => "Contact \u{b7} verified".into(),
            (None, false) => "Contact".into(),
        };
        let blocked = p.block != Block::None;
        let sw = |on: bool, perm: Perm| -> El<'a> {
            let tg = toggler(on).style(ui::toggle_style(t));
            if blocked {
                tg.into()
            } else {
                let did = p.did.clone();
                tg.on_toggle(move |v| g(Gm::SetPerm(did.clone(), perm, v))).into()
            }
        };
        let msg_sub = if p.perms.message {
            if p.via_group.is_some() { "On since you accepted the request".to_string() } else { "On".to_string() }
        } else {
            format!("Off. {first}\u{2019}s messages are refused")
        };
        let call_sub = if p.perms.call { "On".to_string() } else { format!("Off. Turn on to let {first} ring this computer") };
        let interval = self.groups.snap.address.as_ref().map(|a| a.interval.every()).unwrap_or("on a schedule");
        let conn = if p.blocked_by_me() {
            format!("{first} won\u{2019}t get your new address when it next changes.")
        } else if p.address_ok {
            format!("Address up to date on both sides. Yours changes {interval}.")
        } else {
            format!("{first} hasn\u{2019}t got your new address yet. They\u{2019}ll get it when either of you next connects.")
        };
        let mut c = column![
            container(
                column![avatar(t, &p.name, &p.did, 64.0), bold(p.name.clone(), 18.0, t.ink), tx(origin, 13.0, t.ink2)]
                    .spacing(6)
                    .align_x(Alignment::Center)
            )
            .center_x(Fill),
            container(label(t, &format!("What {first} can do"))).padding(Padding { top: 8.0, ..Default::default() }),
            line(t, "Message you", &msg_sub, sw(p.perms.message, Perm::Message)),
            line(t, "Call you", &call_sub, sw(p.perms.call, Perm::Call)),
        ]
        .spacing(12);
        if blocked {
            c = c.push(tx("Blocked: nothing gets through until you unblock. The switches come back as they were.", 12.0, t.ink2));
        }
        c = c
            .push(container(label(t, "Connection")).padding(Padding { top: 8.0, ..Default::default() }))
            .push(tx(conn, 13.0, t.ink2))
            .push(Space::new().height(Fill));
        c = c.push(match p.block {
            Block::ByMe { .. } => btn(t, Kind::Quiet, &format!("Unblock {first}"), Some(g(Gm::Unblock(p.did.clone())))),
            _ => btn(t, Kind::Danger, &format!("Block {first}"), Some(g(Gm::AskBlock(p.did.clone())))),
        });
        container(c).padding(20).width(320).height(Fill).style(ui::sidebar(t)).into()
    }

    // ---- settings ----

    pub(super) fn hosting_page(&self, t: Tok) -> El<'_> {
        let s = &self.groups.snap;
        let mut c = column![
            tx(
                "This computer keeps a full copy of these groups and passes messages on when members\u{2019} phones are asleep. Members connect to it, not to each other.",
                14.0,
                t.ink2
            ),
            container(line(
                t,
                "Keep hosting when the window is closed",
                "Runs in the background and starts with the computer (preview: not applied yet)",
                toggler(s.keep_running).on_toggle(|v| g(Gm::KeepRunning(v))).style(ui::toggle_style(t)).into(),
            ))
            .padding(16)
            .style(ui::card(t)),
        ]
        .spacing(16);
        for r in &s.host_requests {
            c = c.push(
                container(
                    row![
                        gav(t, &r.group_id, 36.0),
                        column![
                            semi(format!("{} asks this computer to host {}", first_name(&r.from_name), r.group_name), 15.0, t.ink),
                            tx(format!("{} members \u{b7} about {} \u{b7} {}", r.members, mb(r.est_bytes), ago_ms(r.at)), 13.0, t.ink2),
                        ]
                        .spacing(2)
                        .width(Fill),
                        btn(t, Kind::Primary, "Review", Some(g(Gm::ReviewHostRequest(r.id.clone())))),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding(16)
                .style(ui::card(t)),
            );
        }
        let hosted: Vec<&Group> = s.hosted().collect();
        if hosted.is_empty() {
            c = c.push(container(tx("This computer doesn\u{2019}t host any groups.", 14.0, t.ink2)).padding(16).width(Fill).style(ui::card(t)));
        } else {
            let mut list = column![].spacing(0);
            for (i, gr) in hosted.iter().enumerate() {
                if i > 0 {
                    list = list.push(rule(t));
                }
                let me = gr.my_role();
                let this = gr.hosts.iter().find(|h| h.this_computer);
                let who = if me == Some(Role::Owner) {
                    "You are owner".to_string()
                } else {
                    format!("Appointed by {}", this.map(|h| first_name(&h.appointed_by).to_string()).unwrap_or_default())
                };
                let co: Vec<String> = gr.active_hosts().filter(|h| !h.this_computer).map(|h| h.label.clone()).collect();
                let co = if co.is_empty() { "only host".to_string() } else { format!("co-host: {}", co.join(", ")) };
                let connected = this.map(|h| h.connected).unwrap_or(0);
                list = list.push(
                    container(
                        row![
                            gav(t, &gr.id, 36.0),
                            column![
                                semi(gr.name.clone(), 15.0, t.ink),
                                tx(format!("{who} \u{b7} {connected} of {} connected \u{b7} {} \u{b7} {co}", gr.members.len(), mb(gr.storage)), 13.0, t.ink2),
                            ]
                            .spacing(2)
                            .width(Fill),
                            link_btn(t, "Stop hosting", t.error, g(Gm::AskStopHosting(gr.id.clone()))),
                        ]
                        .spacing(12)
                        .align_y(Alignment::Center),
                    )
                    .padding([14, 16]),
                );
            }
            c = c.push(container(list).width(Fill).style(ui::card(t)));
        }
        for gr in hosted.iter().filter(|gr| gr.active_hosts().count() == 1) {
            let action: El = if gr.host_asked {
                tx("Members with a desktop were asked to host.", 13.0, t.on_warn).into()
            } else {
                button(semi("Ask a member to host", 14.0, t.on_warn))
                    .padding([9, 18])
                    .style(move |_: &Theme, st| button::Style {
                        background: Some(ui::alpha(t.on_warn, if matches!(st, button::Status::Hovered) { 0.12 } else { 0.0 }).into()),
                        text_color: t.on_warn,
                        border: Border { color: ui::alpha(t.on_warn, 0.4), width: 1.0, radius: 999.0.into() },
                        ..Default::default()
                    })
                    .on_press(g(Gm::AskForHost(gr.id.clone())))
                    .into()
            };
            c = c.push(
                container(
                    column![
                        bold(format!("{} has only this computer as host", gr.name), 15.0, t.on_warn),
                        tx(
                            format!("If it is off, {} messages wait on people\u{2019}s phones. Ask another member to host from their desktop.", gr.name),
                            14.0,
                            t.on_warn
                        ),
                        action,
                    ]
                    .spacing(10),
                )
                .padding(16)
                .width(Fill)
                .style(ui::outlined(t.warn, ui::alpha(t.on_warn, 0.25), 16.0)),
            );
        }
        c.into()
    }

    /// Settings › Privacy: the preview switch, then (when on) address changes and blocked people.
    pub(super) fn privacy_page(&self, t: Tok) -> El<'_> {
        let switch = section(
            t,
            "Preview",
            line(
                t,
                "Preview groups & privacy (placeholder data)",
                "Shows the planned groups, hosting, blocking and address-change screens with sample people. Nothing is sent and nothing is saved; the sample resets when Tinline restarts.",
                toggler(self.groups.on).on_toggle(|v| g(Gm::Enable(v))).style(ui::toggle_style(t)).into(),
            ),
        );
        let mut c = column![switch].spacing(20);
        if !self.groups.on {
            return c.into();
        }
        let s = &self.groups.snap;
        if let Some(a) = &s.address {
            let seg = |i: Interval| -> El<'_> {
                let on = a.interval == i;
                button(container(semi(i.label(), 14.0, if on { t.on_primary_c } else { t.ink })).center_x(Fill))
                    .padding([8, 12])
                    .width(Fill)
                    .style(ui::button_style(t, if on { Kind::Tonal } else { Kind::Ghost }, 999.0))
                    .on_press(g(Gm::Interval(i)))
                    .into()
            };
            let kv = |k: &str, v: String| -> El<'_> { row![tx(k.to_string(), 14.0, t.ink2).width(Fill), semi(v, 14.0, t.ink)].into() };
            let next = a.next_change.saturating_sub(s.now);
            let next = if next < DAY_MS { "Within a day".to_string() } else { format!("In {} days", next / DAY_MS) };
            let mut status = column![
                kv("Last change", when(a.last_change)),
                kv("Next change", next),
                kv("Contacts told", format!("{} of {}", a.told, a.contacts)),
            ]
            .spacing(8);
            let behind = a.contacts.saturating_sub(a.told);
            if behind > 0 {
                status = status.push(tx(
                    format!(
                        "{behind} contact{} hasn\u{2019}t been online since. They\u{2019}ll get it when either of you next connects.",
                        if behind == 1 { "" } else { "s" }
                    ),
                    13.0,
                    t.ink2,
                ));
            }
            c = c.push(section(
                t,
                "Address changes",
                column![
                    tx(
                        "This computer gets a new network address on a schedule. Your contacts and group hosts are told automatically; blocked people and anyone else holding an old address lose the way to reach you.",
                        14.0,
                        t.ink2
                    ),
                    semi("Change my address every", 14.0, t.ink),
                    container(row![seg(Interval::Day), seg(Interval::ThreeDays), seg(Interval::Week)].spacing(2))
                        .padding(3)
                        .style(ui::plain(t.surface2, 999.0)),
                    container(status).padding(14).width(Fill).style(ui::plain(t.surface2, 12.0)),
                    tx(
                        "Shorter intervals cut off old addresses sooner but use a little more battery, and a contact who is offline for longer than two of your intervals may need to re-share a QR.",
                        13.0,
                        t.thread_text
                    ),
                    tx("Linked devices each change their own address; the setting syncs.", 13.0, t.ink2),
                ]
                .spacing(12)
                .into(),
            ));
        }
        let mut bl = column![tx("One list across all your devices: a block made on another device shows here too.", 13.0, t.ink2)].spacing(10);
        let mut any = false;
        for p in &s.people {
            let Block::ByMe { at, how, .. } = p.block else { continue };
            any = true;
            let how = match how {
                Known::Contact => "contact",
                Known::ChatRequest => "from a chat request",
                Known::Group => "from a group",
            };
            bl = bl.push(line(t, &p.name, &format!("Blocked {} \u{b7} {how}", day(at)), btn(t, Kind::Quiet, "Unblock", Some(g(Gm::Unblock(p.did.clone()))))));
        }
        if !any {
            bl = bl.push(tx("Nobody is blocked.", 14.0, t.ink2));
        }
        bl = bl.push(
            container(
                column![
                    semi("Unblocking", 13.0, t.ink),
                    tx(
                        "They can message you again and get your current address. Calls come back only if they were allowed before. They\u{2019}re told they\u{2019}re unblocked.",
                        13.0,
                        t.ink2
                    ),
                ]
                .spacing(4),
            )
            .padding(12)
            .width(Fill)
            .style(ui::plain(t.surface2, 12.0)),
        );
        c = c.push(section(t, "Blocked people", bl.into()));
        c.into()
    }

    // ---- dialogs ----

    pub(super) fn groups_overlay(&self, t: Tok) -> Option<El<'_>> {
        if !self.groups.on {
            return None;
        }
        let d = self.groups.dialog.as_ref()?;
        let s = &self.groups.snap;
        let title = |x: String| bold(x, 22.0, t.ink);
        let para = |x: String| tx(x, 14.0, t.ink2);
        let actions = |cancel: &str, ok: Option<(&str, Kind)>| -> El<'_> {
            let mut r = row![Space::new().width(Fill), btn(t, Kind::Ghost, cancel, Some(g(Gm::Dismiss)))].spacing(8);
            if let Some((label_, kind)) = ok {
                r = r.push(btn(t, kind, label_, Some(g(Gm::Confirm))));
            }
            r.into()
        };
        let body: El = match d {
            Dialog::HostAsk(id) => {
                let r = s.host_requests.iter().find(|r| &r.id == id)?;
                column![
                    title(format!("Host {} on this computer?", r.group_name)),
                    para(format!("{} ({}) asks this computer to host. As a host it will:", first_name(&r.from_name), r.from_role.word())),
                    para(format!(
                        "\u{2022} keep a full copy of the group, about {}\n\u{2022} pass messages on for the {} members when their phones sleep\n\u{2022} know when members are online and their group addresses\n\u{2022} stay running in the background",
                        mb(r.est_bytes),
                        r.members
                    )),
                    para("Hosting doesn\u{2019}t make you an admin. You can stop any time.".into()),
                    actions("Not now", Some(("Host this group", Kind::Primary))),
                ]
                .spacing(14)
                .into()
            }
            Dialog::StopHosting(gid) => {
                let gr = s.group(gid)?;
                let only = gr.active_hosts().count() == 1;
                let mut c = column![
                    title(format!("Stop hosting {}?", gr.name)),
                    para(format!("This computer stops passing {} messages on. The group\u{2019}s admins are told and see \u{201c}Needs a host\u{201d}.", gr.name)),
                ]
                .spacing(14);
                if only {
                    c = c.push(tx(
                        format!("{} has no other host: messages will wait on people\u{2019}s phones until another desktop hosts it.", gr.name),
                        14.0,
                        t.error,
                    ));
                }
                c.push(actions("Cancel", Some(("Stop hosting", Kind::Danger)))).into()
            }
            Dialog::Flag { gid, did } => {
                let gr = s.group(gid)?;
                let m = gr.member(did)?;
                let first = first_name(&m.name).to_string();
                let yours = m.flags.len() + 1;
                let mut meter = row![].spacing(6);
                for i in 1..=gf::FLAGS_TO_REMOVE {
                    let c = if i < yours { ui::alpha(t.error, 0.55) } else if i == yours { t.error } else { t.surface3 };
                    meter = meter.push(container(Space::new()).width(Fill).height(8).style(ui::plain(c, 4.0)));
                }
                let mut c = column![
                    tx(gr.name.clone(), 13.0, t.ink2),
                    title(format!("Flag {first} for removal?")),
                    meter,
                    para(if yours >= gf::FLAGS_TO_REMOVE {
                        format!("Yours will be flag {yours} of {}. {first} is removed right away: their new messages are refused and they are told they were removed.", gf::FLAGS_TO_REMOVE)
                    } else {
                        format!("Yours will be flag {yours} of {n}. At {n} flags {first} is removed right away: their new messages are refused and they are told they were removed.", n = gf::FLAGS_TO_REMOVE)
                    }),
                    para(format!("Flags can\u{2019}t be taken back. {first} can see who flagged them.")),
                ]
                .spacing(14);
                c = c.push(actions("Cancel", Some(("Flag", Kind::Danger))));
                c.into()
            }
            Dialog::Block(did) => {
                let p = s.person(did);
                let name = p.map(|p| p.name.clone()).or_else(|| s.groups.iter().find_map(|gr| gr.member(did)).map(|m| m.name.clone()))?;
                let first = first_name(&name).to_string();
                let shared: Vec<String> = s.groups.iter().filter(|gr| gr.removed.is_none() && gr.member(did).is_some()).map(|gr| gr.name.clone()).collect();
                let mut bullets = vec![
                    format!("{first} can\u{2019}t message or call you, on any of your devices."),
                    format!("{first} is told you blocked them."),
                    format!("{first} won\u{2019}t get your new address next time it changes, so they lose any way to reach this computer."),
                ];
                if !shared.is_empty() {
                    bullets.push(format!("You both stay in {}. Their group messages show collapsed for you.", and_list(&shared)));
                }
                bullets.push("Your chat history stays on this computer.".into());
                let text_ = bullets.iter().map(|b| format!("\u{2022} {b}")).collect::<Vec<_>>().join("\n");
                column![
                    row![avatar(t, &name, did, 40.0), title(format!("Block {first}?"))].spacing(12).align_y(Alignment::Center),
                    para(text_),
                    actions("Cancel", Some(("Block", Kind::Danger))),
                ]
                .spacing(14)
                .into()
            }
            Dialog::Request { gid, did } => {
                let gr = s.group(gid)?;
                let m = gr.member(did)?;
                let first = first_name(&m.name).to_string();
                let mut c = column![
                    row![
                        avatar(t, &m.name, did, 40.0),
                        column![title(format!("Ask {first} for a private chat")), tx(format!("Member of {}", gr.name), 13.0, t.ink2)].spacing(2),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                    text_input("Add a note (optional)", &self.groups.note)
                        .on_input(|v| g(Gm::NoteIn(v)))
                        .on_submit(g(Gm::Confirm))
                        .padding(12)
                        .size(14)
                        .style(ui::input_style(t)),
                    para(format!("{first} sees your name, the group and your note. The request goes through the group\u{2019}s host sealed, so the host can\u{2019}t read it.")),
                    para(format!("If {first} accepts, you can message each other directly. Calls stay off until one of you allows them.")),
                ]
                .spacing(14);
                if let Some(e) = &self.groups.error {
                    c = c.push(tx(e.clone(), 14.0, t.error));
                }
                c.push(actions("Cancel", Some(("Send request", Kind::Primary)))).into()
            }
            Dialog::CallRefused(did) => {
                let p = s.person(did)?;
                let first = first_name(&p.name).to_string();
                column![
                    row![ui::icon(Icon::PhoneOff, 24.0, t.ink2), title(format!("{first} hasn\u{2019}t allowed calls from you"))].spacing(12).align_y(Alignment::Center),
                    para(format!("You can still message {first}. Ask them to turn on \u{201c}Call you\u{201d} for you in their contact page. Their phone never rings for a refused call.")),
                    row![
                        Space::new().width(Fill),
                        btn(t, Kind::Ghost, "OK", Some(g(Gm::Dismiss))),
                        btn(t, Kind::Primary, &format!("Ask {first} to allow calls"), Some(g(Gm::Confirm))),
                    ]
                    .spacing(8),
                ]
                .spacing(14)
                .into()
            }
            Dialog::RemovalDetails(gid) => {
                let gr = s.group(gid)?;
                let r = gr.removed.as_ref()?;
                let mut c = column![title("Why you were removed".into())].spacing(12);
                let n = r.flags.len();
                for (i, f) in r.flags.iter().enumerate() {
                    let mut sub = format!("Flagged {}", when(f.at));
                    if i + 1 == n {
                        sub += &format!(" \u{b7} {} flag", ordinal(n));
                    }
                    c = c.push(
                        row![
                            avatar(t, &f.by_name, &f.by_did, 32.0),
                            column![semi(format!("{} \u{b7} {}", f.by_name, f.by_role.word()), 14.0, t.ink), tx(sub, 12.0, t.ink2)].spacing(1),
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center),
                    );
                }
                c = c
                    .push(tx("Each flag is signed by the admin who made it and was checked by this computer. Nobody else could have added them.", 13.0, t.ink2))
                    .push(semi("What happens now", 15.0, t.ink))
                    .push(para(format!(
                        "You keep the messages already on this computer. The group\u{2019}s hosts stop accepting your new messages and calls now, and refresh their addresses on {}, after which this computer can\u{2019}t reach them at all.",
                        day(r.refresh_at)
                    )))
                    .push(para("To rejoin, an admin has to invite you again.".into()));
                let mut acts = row![Space::new().width(Fill)].spacing(8);
                if let Some(f) = r.flags.iter().find(|f| s.is_contact(&f.by_did) && !s.blocked_by_me(&f.by_did)) {
                    acts = acts.push(btn(t, Kind::Quiet, &format!("Message {} privately", first_name(&f.by_name)), Some(g(Gm::OpenPerson(f.by_did.clone())))));
                }
                acts = acts.push(btn(t, Kind::Primary, "Close", Some(g(Gm::Dismiss))));
                c.push(acts).into()
            }
        };
        let card = container(body).padding(28).width(520).style(ui::plain(t.surface, 20.0));
        let layer = container(opaque(card))
            .center_x(Fill)
            .center_y(Fill)
            .width(Fill)
            .height(Fill)
            .style(ui::plain(Color { r: 0.06, g: 0.08, b: 0.07, a: 0.42 }, 0.0));
        Some(opaque(mouse_area(layer).on_press(g(Gm::Dismiss))))
    }
}

const DAY_MS: u64 = gf::DAY;

fn ordinal(n: usize) -> String {
    match n {
        1 => "1st".into(),
        2 => "2nd".into(),
        3 => "3rd".into(),
        n => format!("{n}th"),
    }
}

/// The time today, else the day, like the real chat list.
fn short_time(ms: u64) -> String {
    if ms == 0 {
        return String::new();
    }
    let today = Local::now().date_naive();
    match Local.timestamp_millis_opt(ms as i64).single() {
        Some(d) if d.date_naive() == today => d.format("%H:%M").to_string(),
        Some(d) if (today - d.date_naive()).num_days() == 1 => "Yesterday".into(),
        Some(d) => d.format("%b %-d").to_string(),
        None => String::new(),
    }
}

fn message_body<'a>(s: &str, fg: Color) -> El<'a> {
    tx(s.to_string(), 14.0, fg).into()
}
