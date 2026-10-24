//! Group chat screens on the core's real API (state in `group_live.rs`), laid out after
//! docs/design/groups/DesktopGroups: the Groups side tab, the conversation with its members pane,
//! member actions, and the new/join/invite/flag/removal/hosting dialogs.

use iced::widget::{button, column, container, mouse_area, opaque, row, text_input, Space};
use p2pcore::{GroupDelivery, GroupItemKind, GroupMessage, GroupRole};

use super::groups_view::{btn, footer, gav, hhmm, link_btn, message_body, ordinal, rule, short_time, sys, tag, when};
use super::*;
use crate::geom;
use crate::shaped::shaped;
use crate::app::group_live::{self as gl, first_name, short_dev, Action, LDialog, Lm, FLAGS_TO_REMOVE};

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

    /// GroupsList.svelte: the New group row, invitations, "Your groups · n" (rows as in Chats), then the folded
    /// "Left or removed" section. Desktop keeps a Join row (paste an invite link) under New group: the playground
    /// assumes links open the app, this build can't.
    pub(super) fn live_list(&self, t: Tok, q: &str) -> El<'_> {
        use super::chat_view::Preview;
        // .sec: 12px 650 uppercase, .06em, muted, 18 16 6; the count after a middle dot at 80 %.
        let sec = |caption: &str, n: usize| -> El<'_> {
            row![semi(caption.to_uppercase(), 12.0, t.ink2).line_height(lh(16.0)),
                semi(format!("\u{b7}  {n}"), 12.0, ui::alpha(t.ink2, 0.8)).line_height(lh(16.0))].spacing(6).into()
        };
        let act = |icon: &'static str, title: &'static str, sub: &'static str, msg: Msg| -> El<'_> {
            container(button(row![container(crate::pg_icons::pgi(icon, 22.0, t.accent_ink)).width(48).height(48).center_x(48).center_y(48).style(ui::plain(t.accent_soft, 24.0)),
                    column![semi(title, 15.0, t.ink).line_height(lh(21.0)), tx(sub, 13.0, t.ink2).line_height(lh(18.0))].spacing(2)]
                    .spacing(12).align_y(Alignment::Center).height(48))
                .padding(8).width(Fill).height(64).style(super::shell_view::wash(t, None, Some(t.surface2), 14.0)).on_press(msg)).padding([0, 8]).into()
        };
        let mut col = column![act("plus", "New group", "You\u{2019}ll be the owner", l(Lm::NewGroup)),
            act("userplus", "Join a group", "Paste an invite link", l(Lm::JoinOpen))];
        let ready = self.live.ready;
        // Invitations to you: cards (surface on desktop = --bg, hairline ring, radius --r-ctl, 14 padding).
        let pending: Vec<_> = self.live.pending_invitations().collect();
        if !pending.is_empty() {
            col = col.push(container(sec("Invitations to you", pending.len())).padding(Padding { top: 18.0, right: 16.0, bottom: 6.0, left: 16.0 }));
            for i in pending {
                let who = first_name(&self.live_name(&i.inviter_did, &i.inviter_name)).to_string();
                let joining = self.live.joining.as_deref() == Some(i.id.as_str());
                let busy = self.live.joining.is_some();
                let line = iced::widget::rich_text![
                    iced::widget::span::<(), iced::Font>(who).font(ui::SANS_SEMI).color(t.ink),
                    iced::widget::span::<(), iced::Font>(" invited you to ").color(t.ink),
                    iced::widget::span::<(), iced::Font>(i.group_name.clone()).font(ui::SANS_SEMI).color(t.ink),
                ].size(15).line_height(lh(20.0));
                let card = column![
                    row![self.profile_avatar(t, &i.inviter_name, &i.inviter_did, 40.0), line].spacing(12).align_y(Alignment::Center),
                    row![Space::new().width(Fill),
                        btn(t, Kind::Quiet, "Decline", (!busy).then(|| l(Lm::DeclineInv(i.id.clone())))),
                        btn(t, Kind::Primary, if joining { "Joining\u{2026}" } else { "Accept" }, (!busy).then(|| l(Lm::AcceptInv(i.id.clone())))),
                    ].spacing(8),
                ].spacing(10);
                let style = move |_: &iced::Theme| container::Style {
                    background: Some(t.bg.into()),
                    border: Border { color: t.line, width: 1.0, radius: 14.0.into() },
                    ..Default::default()
                };
                col = col.push(container(container(card).padding(14).width(Fill).style(style)).padding([6, 8]));
            }
        }
        let shown = |s: &&p2pcore::GroupSummary| q.is_empty() || s.name.to_lowercase().contains(q);
        let active: Vec<_> = self.live.list.iter().filter(|s| gl::active(s.my_role)).filter(shown).collect();
        let gone: Vec<_> = self.live.list.iter().filter(|s| !gl::active(s.my_role)).filter(shown).collect();
        col = col.push(container(sec("Your groups", active.len())).padding(Padding { top: 18.0, right: 16.0, bottom: 6.0, left: 16.0 }));
        if active.is_empty() {
            let msg = if !ready { "Unlock to see your groups." } else if q.is_empty() { "Groups you start or join show up here." } else { "No group matches." };
            col = col.push(container(tx(msg, 13.0, t.ink2).line_height(lh(19.0))).padding([8, 20]));
        }
        // The group's own mandala (the left-or-removed rows rely on their dimmed text; the art is not faded).
        let face = |gid: &str| -> El<'_> { gav(t, gid, 48.0) };
        let mut rows = column![];
        for s in active {
            let at = s.last.as_ref().map_or(0, |m| m.at);
            let sel = self.live.open.as_deref() == Some(s.id.as_str());
            let text = gl::preview(s, &self.live.me);
            // GroupsList `.callpv`: a running call takes the preview line (accent ink).
            let in_call = self.gcall.gid.as_deref() == Some(s.id.as_str()) && self.gcall.live();
            let pv = if in_call { Preview::Plain("You\u{2019}re in the call".into(), Some(t.accent_ink)) }
                else if self.gcall.live.contains_key(&s.id) { Preview::Plain("Call in progress".into(), Some(t.accent_ink)) }
                else if !s.hosted_here && !s.host_reachable { Preview::Waiting(text) } else { Preview::Plain(text, None) };
            let mark = if matches!(pv, Preview::Plain(_, None)) {
                self.list_mark(true, &s.id, s.last.as_ref().filter(|m| m.outgoing && m.kind == GroupItemKind::Post).map(|m| m.id.as_str()))
            } else { None };
            rows = rows.push(self.pg_chat_row(t, sel, face(&s.id), s.name.clone(), at, s.unread, false, pv, l(Lm::Open(s.id.clone())), mark));
        }
        col = col.push(container(rows).padding([0, 8]));
        if !gone.is_empty() {
            let open = self.live.gone_open;
            let chev = container(crate::pg_icons::pgi(if open { "chevup" } else { "chev" }, 18.0, t.ink2));
            col = col.push(container(button(row![sec("Left or removed", gone.len()), Space::new().width(Fill), chev].align_y(Alignment::Center))
                .padding([0, 16]).height(44).width(Fill).style(super::shell_view::wash(t, None, None, 0.0)).on_press(l(Lm::GoneFold)))
                .padding(Padding { top: 6.0, ..Padding::ZERO }));
            if open {
                let mut rows = column![];
                for s in gone {
                    let sel = self.live.open.as_deref() == Some(s.id.as_str());
                    let at = s.last.as_ref().map_or(0, |m| m.at);
                    let what = if s.my_role == GroupRole::Removed { "Removed" } else { "You left" };
                    let sub = if at > 0 { format!("{what} \u{b7} {}", day_only(at)) } else { what.to_string() };
                    rows = rows.push(button(row![face(&s.id), column![semi(s.name.clone(), 15.0, ui::alpha(t.ink, 0.72)).line_height(lh(21.0)),
                            tx(sub, 13.0, t.ink2).line_height(lh(21.0))].spacing(2).width(Fill)].spacing(12).align_y(Alignment::Center).height(56))
                        .padding(8).width(Fill).style(super::shell_view::wash(t, sel.then_some(t.accent_soft), (!sel).then_some(t.surface2), 14.0))
                        .on_press(l(Lm::Open(s.id.clone()))));
                }
                col = col.push(container(rows).padding([0, 8]));
            }
        }
        container(col).padding(Padding { bottom: 24.0, ..Padding::ZERO }).into()
    }

    // ---- the conversation ----

    pub(super) fn live_pane(&self, t: Tok) -> Option<El<'_>> {
        let d = self.live.detail.as_ref()?;
        self.live.open.as_ref()?;
        let gone = !gl::active(d.my_role);
        let sub_color = if d.my_role == GroupRole::Removed { t.error } else if self.live.waiting_for_host() { t.thread_text } else { t.ink2 };
        let on = !self.live.hide_info;
        // Conversation.svelte `.bar` for a group: the head (36 group avatar, name 17/22, "9 members" 13/18), then
        // the details button (here it shows or hides the members pane beside the chat).
        let face = gav(t, self.live.open.as_deref().unwrap_or_default(), 36.0);
        let head_btn = button(row![face, column![semi(d.name.clone(), 17.0, t.ink).line_height(lh(22.0)).wrapping(iced::widget::text::Wrapping::None),
                tx(self.live.status_line(), 13.0, sub_color).line_height(lh(18.0)).wrapping(iced::widget::text::Wrapping::None)]].spacing(10).align_y(Alignment::Center))
            .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 6.0 }).height(44).width(Fill)
            .style(super::shell_view::wash(t, None, Some(t.surface2), 12.0)).on_press(l(Lm::ToggleInfo));
        let info = button(container(crate::pg_icons::pgi("info", 24.0, if on { t.accent_ink } else { t.ink })).center_x(44).center_y(44)).padding(0).width(44).height(44)
            .style(super::shell_view::wash(t, on.then_some(t.accent_soft), Some(t.surface2), 22.0)).on_press(l(Lm::ToggleInfo));
        // The call button: start a call, join the one running, or return to mine (live: accent-soft). It stays
        // in place while the group has no call host: pressing it then explains, calmly, what a call needs.
        let gid = d.id.clone();
        let running = self.gcall.live.contains_key(&gid) || (self.gcall.gid.as_deref() == Some(gid.as_str()) && self.gcall.live());
        let no_host = d.call_hosts.is_empty() && !running;
        let tip = if self.gcall.gid.as_deref() == Some(gid.as_str()) && self.gcall.live() { "Return to group call" } else if running { "Join group call" }
            else if no_host { "Group call (needs a call host)" } else { "Start group call" };
        let press = if no_host { l(Lm::CallNeedsHost(!self.live.call_needs_host)) } else { Msg::GCall(crate::app::gcall::Gm::Call(gid.clone())) };
        let call_btn = button(container(crate::pg_icons::pgi("phone", 24.0, if running { t.accent_ink } else { t.ink })).center_x(44).center_y(44))
            .padding(0).width(44).height(44)
            .style(super::shell_view::wash(t, (running || (no_host && self.live.call_needs_host)).then_some(t.accent_soft), Some(t.surface2), 22.0))
            .on_press_maybe((!gone).then_some(press));
        let call_btn = iced::widget::tooltip(call_btn, iced::widget::text(tip).size(13), iced::widget::tooltip::Position::Bottom);
        // Public group §8: "12 online" in an open group opens the online list. The label keeps the width of
        // "500+ online", so a changing count moves nothing beside it.
        let mut head = row![head_btn].spacing(2).align_y(Alignment::Center);
        if let Some(label_) = self.live.online_label() {
            let w = crate::fit::width(&gl::online_label(gl::ONLINE_CAP), ui::SANS_SEMI, 13.0).ceil();
            let dot = container(Space::new()).width(8).height(8).style(ui::plain(if self.live.online.is_empty() { t.ink2 } else { t.accent }, 4.0));
            let b = button(container(row![dot, container(semi(label_, 13.0, t.ink2).line_height(lh(18.0)).wrapping(iced::widget::text::Wrapping::None)).width(w)]
                    .spacing(6).align_y(Alignment::Center)).height(44).center_y(44))
                .padding([0, 12]).height(44)
                .style(super::shell_view::wash(t, None, Some(t.surface2), 22.0)).on_press(l(Lm::ShowOnline));
            head = head.push(iced::widget::tooltip(b, iced::widget::text("Who\u{2019}s online").size(13), iced::widget::tooltip::Position::Bottom));
        }
        let head = head.push(call_btn).push(info);
        let mut top = column![container(head).padding(8).height(60).width(Fill).style(ui::plain(t.surface, 0.0)), rule(t)];
        if !gone && let Some(b) = self.gcall_banner(t, &d.id) {
            top = top.push(b).push(rule(t));
        }
        if self.notice.is_some() {
            top = top.push(container(self.notice_bar(t)).padding([8, 20]));
        }
        if self.live.call_needs_host && d.call_hosts.is_empty() && !gone {
            let admin = self.live.is_admin();
            let mut acts = row![].spacing(8);
            if admin {
                acts = acts.push(btn(t, Kind::Primary, "Host calls on this computer", Some(l(Lm::CallsHere(true)))));
            }
            acts = acts.push(btn(t, Kind::Ghost, "Not now", Some(l(Lm::CallNeedsHost(false)))));
            top = top.push(
                container(
                    column![
                        bold("A call needs a call host", 15.0, t.ink),
                        tx(if admin { "A call host is a member\u{2019}s computer or phone that carries the call. You can host calls on this computer, or appoint a member in Group info." }
                            else { "A call host is a member\u{2019}s computer or phone that carries the call. An admin can appoint one in Group info." }, 13.0, t.ink2),
                        acts,
                    ].spacing(6),
                ).padding([12, 20]).width(Fill).style(ui::plain(t.surface2, 0.0)),
            ).push(rule(t));
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
        // `.msgs`: 6 16 19 on the canvas; day chips; runs of one author stack (10 px between, 2 px inside).
        let strip = self.live.waiting_for_host();
        let body: El = iced::widget::responsive(move |size| {
            // .bub max-width: min(78 %, 560) of the list's content box.
            let max_bub = ((size.width - 32.0) * 0.78).min(560.0);
            let mut list = column![].spacing(0).padding(Padding { top: 6.0 + if strip { 33.0 } else { 0.0 }, right: 16.0, bottom: 19.0, left: 16.0 });
            if self.live.older.is_some() {
                list = list.push(container(link_btn(t, "Show earlier messages", t.primary, l(Lm::Older))).center_x(Fill));
            }
            let join = |a: &GroupMessage, b: &GroupMessage| {
                a.kind == GroupItemKind::Post
                    && b.kind == GroupItemKind::Post
                    && a.outgoing == b.outgoing
                    && (a.outgoing || a.author_did == b.author_did)
                    && !self.blocks.blocked_by_me(&b.author_did)
                    && a.at.abs_diff(b.at) <= geom::GROUP_GAP_MS
                    && super::groups_view::day(a.at) == super::groups_view::day(b.at)
            };
            let mut last_day = String::new();
            // This group's call log, oldest first, merged in by time (MessageRow `.callrec`).
            let open_id = self.live.open.clone().unwrap_or_default();
            let calls: Vec<&p2pcore::GroupCallRecord> = {
                let mut v: Vec<_> = self.group_log.iter().filter(|r| r.group_id == open_id).collect();
                v.sort_by_key(|r| r.started_at);
                v
            };
            let mut ci = 0;
            let day_chip = |at: u64| -> El<'_> {
                let chip = container(semi(chip_day(at), 11.0, t.ink2).line_height(lh(16.0))).padding([3, 10]).style(move |_: &iced::Theme| container::Style {
                    background: Some(t.surface.into()), border: iced::Border { color: t.hairline, width: 1.0, radius: 999.0.into() }, ..Default::default() });
                container(chip).center_x(Fill).padding([10, 0]).into()
            };
            // Before the oldest of the posts that were unread when the group was opened.
            let n_unread = self.live.unread_at_open as usize;
            let divider = if n_unread > 0 { self.live.msgs.iter().rev().filter(|m| !m.outgoing && m.kind == GroupItemKind::Post).nth(n_unread - 1).map(|m| m.id.clone()) } else { None };
            for (i, m) in self.live.msgs.iter().enumerate() {
                while ci < calls.len() && calls[ci].started_at * 1000 <= m.at {
                    let at = calls[ci].started_at * 1000;
                    let dk = super::groups_view::day(at);
                    let new_day = dk != last_day;
                    if new_day {
                        last_day = dk;
                        list = list.push(day_chip(at));
                    }
                    list = list.push(container(self.group_call_pill(t, calls[ci])).padding(Padding { top: if new_day { 0.0 } else { 10.0 }, ..Default::default() }));
                    ci += 1;
                }
                let dk = super::groups_view::day(m.at);
                let new_day = dk != last_day;
                if new_day {
                    last_day = dk;
                    let chip = container(semi(chip_day(m.at), 11.0, t.ink2).line_height(lh(16.0))).padding([3, 10]).style(move |_: &iced::Theme| container::Style {
                        background: Some(t.surface.into()), border: iced::Border { color: t.hairline, width: 1.0, radius: 999.0.into() }, ..Default::default() });
                    list = list.push(container(chip).center_x(Fill).padding([10, 0]));
                }
                let pos = geom::run_pos(&self.live.msgs, i, join);
                let mut gap = if new_day { 0.0 } else if m.kind != GroupItemKind::Post { 6.0 } else { geom::gap_above(pos) };
                if divider.as_deref() == Some(m.id.as_str()) {
                    let rule_ = || container(Space::new()).height(1).width(Fill).style(ui::plain(ui::alpha(t.accent_ink, 0.35), 0.0));
                    list = list.push(container(row![rule_(), bold(format!("{n_unread} new {}", if n_unread == 1 { "message" } else { "messages" }), 11.0, t.accent_ink), rule_()]
                        .spacing(10).align_y(Alignment::Center)).padding(Padding { top: 12.0, bottom: 6.0, ..Default::default() }));
                    gap = 4.0;
                }
                list = list.push(container(self.live_item(t, m, pos, max_bub)).padding(iced::Padding { top: gap, ..Default::default() }));
            }
            while ci < calls.len() {
                let at = calls[ci].started_at * 1000;
                let dk = super::groups_view::day(at);
                let new_day = dk != last_day;
                if new_day {
                    last_day = dk;
                    list = list.push(day_chip(at));
                }
                list = list.push(container(self.group_call_pill(t, calls[ci])).padding(Padding { top: if new_day { 0.0 } else { 10.0 }, ..Default::default() }));
                ci += 1;
            }
            if self.live.msgs.is_empty() && calls.is_empty() {
                list = list.push(sys(t, "No messages yet"));
            }

            container(scroll(t, list).anchor_bottom().width(Fill).height(Fill)).width(Fill).height(Fill).style(ui::plain(t.canvas, 0.0)).into()
        }).into();
        // The strip over the top of the list (gold: waiting for a host). It overlays, so nothing moves.
        let msgs: El = if strip {
            let text = "Waiting for a host. No group host is online; your posts are saved and send when one is.";
            let s_ = container(row![crate::pg_icons::pgi("clock", 16.0, t.warn_ink), tx(text, 13.0, t.warn_ink).line_height(lh(18.0))].spacing(8).align_y(Alignment::Center))
                .padding([7, 16]).width(Fill).style(ui::plain(t.warn, 0.0));
            iced::widget::stack![body, column![s_, container(Space::new()).height(1).width(Fill).style(ui::plain(t.hairline, 0.0))]].into()
        } else { body };

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
            _ if self.live.admins_only_locked() => self.live_admins_only_line(t),
            _ => self.live_composer(t),
        };
        let conv = column![top, msgs, bottom].width(Fill).height(Fill);
        if !on {
            return Some(conv.into());
        }
        let _ = gone;
        // GroupInfo.svelte on desktop: the info page takes the detail pane (back returns to the conversation).
        Some(self.group_info_page(t))
    }

    /// Composer.svelte for a group: the reply chip, the picture button (public group §5; the text
    /// goes with the picture as its caption), the field and the round send button.
    fn live_composer(&self, t: Tok) -> El<'_> {
        let mut c = column![].spacing(6);
        if let Some(id) = &self.live.reply {
            let (who, text_) = match self.live.by_id(id) {
                Some(m) if m.outgoing => ("yourself".to_string(), gl::post_line(m)),
                Some(m) => (first_name(&self.live_name(&m.author_did, &m.author_name)).to_string(), gl::post_line(m)),
                None => ("a message".to_string(), String::new()),
            };
            let bar = container(Space::new()).width(3).height(Fill).style(ui::plain(t.accent, 0.0));
            let x = button(container(crate::pg_icons::pgi("x", 18.0, t.ink2)).center_x(44).center_y(44)).padding(0).width(44).height(44)
                .style(super::shell_view::wash(t, None, Some(t.surface3), 22.0)).on_press(l(Lm::Reply(None)));
            let body = column![semi(format!("Replying to {who}"), 13.0, t.accent_ink).line_height(lh(18.0)),
                tx(crate::fit::ellipsize(&text_, ui::SANS, 13.0, 520.0), 13.0, t.ink2).line_height(lh(18.0)).wrapping(iced::widget::text::Wrapping::None)].width(Fill);
            c = c.push(container(row![bar, container(row![body, x].spacing(8).align_y(Alignment::Center)).padding(Padding { top: 0.0, right: 2.0, bottom: 0.0, left: 7.0 }).height(44)].height(44))
                .width(Fill).clip(true).style(ui::plain(t.surface2, 12.0)));
        }
        let value = &self.live.input;
        let can = !value.trim().is_empty() && !self.live.sending;
        let field = text_input("Message", value)
            .id(COMPOSER)
            .on_input(|v| l(Lm::Input(v)))
            .on_submit(l(Lm::Send))
            .padding([11, 16])
            .size(15)
            .line_height(lh(22.0))
            .style(move |_, st| {
                let focused = matches!(st, iced::widget::text_input::Status::Focused { .. });
                iced::widget::text_input::Style {
                    background: t.surface.into(),
                    border: iced::Border { color: if focused { t.accent } else { t.hairline }, width: 1.0, radius: 22.0.into() },
                    icon: t.ink2, placeholder: t.ink2, value: t.ink, selection: ui::alpha(t.accent, 0.3),
                }
            });
        let send = button(container(crate::pg_icons::pgi("send", 20.0, t.on_accent)).center_x(44).center_y(44)).padding(0).width(44).height(44)
            .style(move |_: &iced::Theme, _| iced::widget::button::Style { background: Some(ui::alpha(t.accent, if can { 1.0 } else { 0.38 }).into()), border: iced::Border { radius: 22.0.into(), ..Default::default() }, ..Default::default() })
            .on_press_maybe(can.then_some(l(Lm::Send)));
        let busy = self.live.sending;
        let attach = button(container(crate::pg_icons::pgi("image", 24.0, if busy { ui::alpha(t.ink2, 0.38) } else { t.ink2 })).center_x(44).center_y(44))
            .padding(0).width(44).height(44).style(super::shell_view::wash(t, None, (!busy).then_some(t.surface2), 22.0))
            .on_press_maybe((!busy).then_some(l(Lm::Attach)));
        c = c.push(row![attach, container(field).width(Fill), send].spacing(6).align_y(Alignment::End));
        container(c).padding(Padding { top: 8.0, right: 10.0, bottom: 10.0, left: 10.0 }).width(Fill).style(ui::plain(t.canvas, 0.0)).into()
    }

    /// An admins-only group (a channel), for a member: one calm line in the composer's place, at the
    /// composer's height (8 + 44 + 10), so nothing moves when an admin switches it on or off.
    fn live_admins_only_line(&self, t: Tok) -> El<'_> {
        let line = container(tx("Only admins post here", 14.0, t.ink2).line_height(lh(20.0))).center_x(Fill).center_y(44);
        container(line).padding(Padding { top: 8.0, right: 10.0, bottom: 10.0, left: 10.0 }).width(Fill).style(ui::plain(t.canvas, 0.0)).into()
    }

    /// MessageRow `.callrec` for a group call: "Group call · 19 min · 4 people", or "Missed group call · 5 min"
    /// (calm: never the error colour). A call I started sits on my side. Tap = start or join a call here.
    fn group_call_pill<'a>(&'a self, t: Tok, r: &p2pcore::GroupCallRecord) -> El<'a> {
        let stamp = hhmm(r.started_at * 1000);
        let gid = r.group_id.clone();
        let gone = self.live.detail.as_ref().is_none_or(|d| !gl::active(d.my_role));
        let label = p2pcore::group_call_label(r.clone());
        let pill = button(row![crate::pg_icons::pgi("phone", 16.0, t.ink2), semi(label, 13.0, t.ink).line_height(lh(18.0)),
                tx(stamp, 11.0, t.ink2).line_height(lh(18.0))].spacing(6).height(Fill).align_y(Alignment::Center))
            .padding([0, 12]).height(32)
            .style(move |_: &iced::Theme, st| button::Style {
                background: Some(if matches!(st, button::Status::Hovered) { t.surface2 } else { t.surface }.into()),
                border: Border { color: t.hairline, width: 1.0, radius: 16.0.into() }, text_color: t.ink, ..Default::default() })
            .on_press_maybe((!gone).then(|| Msg::GCall(crate::app::gcall::Gm::Call(gid))));
        if !r.missed && r.started_by == self.live.me { row![Space::new().width(Fill), pill].into() } else { row![pill, Space::new().width(Fill)].into() }
    }

    fn live_item<'a>(&'a self, t: Tok, m: &'a GroupMessage, pos: geom::Pos, max_bub: f32) -> El<'a> {
        if m.kind == GroupItemKind::Removed && m.subject_did == self.live.me {
            return self.live_removal_card(t);
        }
        if m.kind != GroupItemKind::Post {
            return sys(t, &gl::sys_text(m, &self.live.me));
        }
        let quote = m.reply_to.as_ref().map(|id| match self.live.by_id(id) {
            Some(r) => (if r.outgoing { "You".to_string() } else { first_name(&self.live_name(&r.author_did, &r.author_name)).to_string() }, gl::post_line(r), r.outgoing),
            None => (String::new(), "Original message unavailable".to_string(), false),
        });
        let name = self.live_name(&m.author_did, &m.author_name);
        if !m.outgoing && self.blocks.blocked_by_me(&m.author_did) {
            // A personal block never changes the group (U9 is the group's tool); the message is
            // folded, not hidden from the record.
            return row![
                container(row![ui::icon(Icon::Ban, 14.0, t.ink2), tx(format!("Message from {name}, whom you blocked"), 13.0, t.ink2)].spacing(6).align_y(Alignment::Center))
                    .padding([6, 12]).style(ui::outlined(t.bg, t.line, 16.0)),
                Space::new().width(Fill)
            ].into();
        }
        let (fg, bg) = if m.outgoing { (t.on_primary_c, t.primary_c) } else { (t.ink, t.bub_in) };
        let metac = if m.outgoing { t.bub_meta_out } else { t.bub_meta_in };
        let inner = max_bub - 26.0;
        let mut c = column![].spacing(0).width(Length::Shrink);
        // MessageRow `.who`: the sender's first name in their hue, on the first of their run.
        if !m.outgoing && !matches!(pos, geom::Pos::Mid | geom::Pos::Last) {
            let (_, hue) = ui::avatar_colors(t, &m.author_did);
            let who = if m.before_removal { format!("{} \u{b7} removed", first_name(&name)) } else { first_name(&name).to_string() };
            c = c.push(container(bold(who, 13.0, if m.before_removal { t.ink2 } else { hue }).line_height(lh(18.0))).padding(Padding { bottom: 1.0, ..Padding::ZERO }));
        }
        if let Some((who, text_, mine)) = quote {
            let mut q = column![].spacing(0);
            if !who.is_empty() {
                let (_, hue) = ui::avatar_colors(t, &m.author_did);
                q = q.push(bold(who, 12.0, if mine { t.accent_ink } else { hue }).line_height(lh(18.0)));
            }
            q = q.push(tx(crate::fit::ellipsize(&text_, ui::SANS, 13.0, inner - 26.0), 13.0, t.ink2).line_height(lh(18.0)));
            c = c.push(container(container(row![container(Space::new()).width(3).height(Fill).style(ui::plain(t.accent, 0.0)), container(q).padding(Padding { top: 5.0, right: 10.0, bottom: 6.0, left: 7.0 })].height(Length::Shrink))
                .width(Length::Fixed(inner.min(320.0))).style(ui::plain(t.surface2, 12.0))).padding(Padding { top: 2.0, bottom: 6.0, ..Padding::ZERO }));
        }
        let mut meta = row![tx(hhmm(m.at), 11.0, metac).line_height(lh(11.0))].spacing(3).align_y(Alignment::Center);
        let mut meta_w = crate::fit::width(&hhmm(m.at), ui::SANS, 11.0);
        if m.outgoing {
            // Receipt ticks (ticks.rs): waiting for a host = the slack string; sent = one tick; every eligible
            // member signed delivered = two ticks; every one read = the read ink (core's `GroupReceiptStatus`).
            // Host storage is never shown as members' delivery/read. One-shot transitions as in 1:1 (core's rule).
            let st = self.live.receipts.get(&m.id);
            let v = self.live.tick_vis_of(m).unwrap_or(crate::ticks::Vis::One);
            let tw = 12.0 * crate::tin_ticks::VB_W / crate::tin_ticks::VB_H;
            let inks = crate::ticks::Inks { meta: metac, read: t.tick_read, bad: t.tick_bad, bg };
            let glyph: El = container(crate::ticks::glyph(v, self.live.tick_anims.get(&m.id), inks, 12.0, self.fx.now)).width(tw).height(12).center_x(tw).center_y(12).into();
            // No details view for group posts yet: the n/N lines and who sit in the tick's tooltip (no layout shift).
            // An open group sends no member receipts (public group §4): "Delivered 0/N" would mislead, so no n/N.
            let open_group = self.live.detail.as_ref().is_some_and(|d| d.open);
            meta = meta.push(match st.filter(|r| r.eligible > 0 && !open_group) {
                Some(r) => El::from(iced::widget::tooltip(glyph, iced::widget::text(group_receipt_lines(r).join("\n")).size(13), iced::widget::tooltip::Position::Top)),
                None => glyph,
            });
            meta_w += tw + 3.0;
        }
        if let Some(img) = &m.image {
            c = c.push(container(self.live_image(t, m, img, inner, fg)).padding(Padding { top: 2.0, bottom: 4.0, ..Padding::ZERO }));
        }
        let te = message_body(&m.text, fg);
        let (lines, last) = live_wrap(&m.text, inner);
        let fits = last + 10.0 + meta_w <= inner;
        c = c.push(if m.text.is_empty() {
            // A picture without a caption: the time under it, on the right.
            El::from(container(meta).align_right(Fill))
        } else if fits && lines == 1 {
            El::from(row![te, column![Space::new().height(10), meta].padding(Padding { left: 10.0, ..Padding::ZERO })])
        } else if fits {
            iced::widget::stack![container(te).width(inner), container(meta).align_right(Fill).align_bottom(Fill)].into()
        } else {
            column![te, container(meta).align_right(Fill)].width(Length::Shrink).into()
        });
        if m.before_removal {
            c = c.push(tx(format!("written before {} was removed", first_name(&name)), 11.0, t.ink2));
        }
        let side = if m.outgoing { geom::Side::Right } else { geom::Side::Left };
        let radii = geom::bubble_radii(pos, side, geom::R_BUB, geom::R_BUB_TIGHT);
        let mut b = shaped(container(c).padding([8, 13]).max_width(max_bub), radii, bg);
        if !m.outgoing { b = b.stroke(t.bub_in_line, 1.0); }
        // The reply action: a 28 px button beside the bubble (its room kept, so nothing reflows).
        let hot = self.live.hover.as_deref() == Some(m.id.as_str());
        let reply_btn: El = if self.live.can_post() && hot {
            button(container(crate::pg_icons::pgi("reply", 18.0, t.ink2)).center_x(28).center_y(28)).padding(0).width(28).height(28)
                .style(super::shell_view::wash(t, None, Some(t.surface), 14.0)).on_press(l(Lm::Reply(Some(m.id.clone())))).into()
        } else { Space::new().width(28).height(28).into() };
        let line = if m.outgoing { row![Space::new().width(Fill), reply_btn, b] } else { row![b, reply_btn, Space::new().width(Fill)] }.spacing(4).align_y(Alignment::Center);
        let mut col = column![line];
        // MessageRow `.wait`: under my post while no host has it, gold, with a clock.
        if m.outgoing && m.delivery == GroupDelivery::Waiting {
            col = col.push(container(row![crate::pg_icons::pgi("clock", 12.0, t.gold_ink), semi("Waiting for a host", 12.0, t.gold_ink).line_height(lh(16.0))].spacing(4).align_y(Alignment::Center))
                .align_right(Fill).padding(Padding { top: 3.0, right: 2.0, ..Padding::ZERO }));
        }
        // Failed takes the same slot, in the error ink: the hosts refuse this post and it is not sent.
        if let Some(why) = crate::app::group_live::failed_line(m) {
            col = col.push(container(row![crate::pg_icons::pgi("warning", 12.0, t.error), semi(why, 12.0, t.error).line_height(lh(16.0))].spacing(4).align_y(Alignment::Center))
                .align_right(Fill).padding(Padding { top: 3.0, right: 2.0, ..Padding::ZERO }));
        }
        mouse_area(col).on_enter(l(Lm::Hover(Some(m.id.clone())))).on_exit(l(Lm::Hover(None))).into()
    }

    /// A post's picture (public group §5), drawn like a 1:1 photo (PhotoBody): its box at the
    /// picture's shape from the start, the 4x3 colour preview until it is here, then the picture.
    /// Not downloaded (the auto-download setting said wait): its size and "Load photo"; on its way:
    /// "Loading..."; failed: "Try again".
    fn live_image<'a>(&'a self, t: Tok, m: &'a GroupMessage, img: &'a p2pcore::GroupImage, inner: f32, fg: Color) -> El<'a> {
        use p2pcore::GroupImageState as St;
        let (w, h) = (img.width.max(1) as f32, img.height.max(1) as f32);
        let s = (inner.min(300.0) / w).min(340.0 / h);
        let (bw, bh) = ((w * s).max(80.0).round(), (h * s).max(40.0).round());
        let r: iced::border::Radius = 10.0.into();
        let mut st = iced::widget::stack![container(Space::new()).width(bw).height(bh)
            .style(move |_: &iced::Theme| iced::widget::container::Style { background: Some(ui::alpha(fg, 0.08).into()), border: Border { radius: r, ..Default::default() }, ..Default::default() })];
        if img.preview.len() == 36 {
            let rgba: Vec<u8> = img.preview.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
            st = st.push(iced::widget::image(iced::widget::image::Handle::from_rgba(4, 3, rgba)).width(bw).height(bh)
                .content_fit(iced::ContentFit::Fill).filter_method(iced::widget::image::FilterMethod::Linear).border_radius(r));
        }
        if img.state == St::Ready
            && let Some(path) = self.live.image_paths.get(&m.id)
        {
            st = st.push(iced::widget::image(iced::widget::image::Handle::from_path(path)).width(bw).height(bh).content_fit(iced::ContentFit::Cover).border_radius(r));
        }
        let chip = |s: String| -> El<'a> {
            container(tx(s, 11.0, Color::WHITE).line_height(lh(15.0))).padding([2, 8]).style(ui::plain(Color { a: 0.42, ..Color::BLACK }, 10.0)).into()
        };
        let pill = |label_: &'static str, msg: Msg| -> El<'a> {
            button(container(semi(label_, 13.0, t.on_accent)).center_y(32)).padding([0, 16]).height(32)
                .style(move |_: &iced::Theme, _| button::Style { background: Some(t.accent.into()), border: Border { radius: 16.0.into(), ..Default::default() }, ..Default::default() })
                .on_press(msg).into()
        };
        let mid: Option<El> = match img.state {
            St::Remote => Some(column![chip(format!("Photo \u{b7} {}", present::time::bytes(img.size))), pill("Load photo", l(Lm::LoadImage(m.id.clone())))].spacing(8).align_x(Alignment::Center).into()),
            St::Loading => Some(chip("Loading\u{2026}".into())),
            St::Failed => Some(pill("Try again", l(Lm::LoadImage(m.id.clone())))),
            St::Ready => None,
        };
        if let Some(e) = mid {
            st = st.push(container(e).center_x(bw).center_y(bh));
        }
        st.width(bw).height(bh).into()
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

    // ---- group info (GroupInfo.svelte) ----

    /// The group info page: hero, invites (admins), hosts, call hosts, members, leave. What I can do depends on my role.
    /// The core has no per-group mute, so "Mute notifications" isn't offered (SCREENS-PORT-STATUS.md).
    fn group_info_page<'a>(&'a self, t: Tok) -> El<'a> {
        let Some(d) = self.live.detail.as_ref() else { return Space::new().into() };
        let admin = self.live.is_admin();
        let active = gl::active(d.my_role);
        let bar = container(row![
            button(container(crate::pg_icons::pgi("back", 24.0, t.ink)).center_x(44).center_y(44)).padding(0).width(44).height(44)
                .style(super::shell_view::wash(t, None, Some(t.surface2), 22.0)).on_press(l(Lm::ToggleInfo)),
            semi(if d.my_role == GroupRole::Removed { "Removed" } else { "Group info" }, 17.0, t.ink),
        ].spacing(4).align_y(Alignment::Center)).padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 8.0 }).height(60).width(Fill).style(ui::plain(t.surface, 0.0));
        let h2 = |s: &str, n: Option<usize>| -> El<'a> {
            let mut r = row![semi(s.to_uppercase(), 12.0, t.ink2).line_height(lh(16.0))].spacing(6);
            if let Some(n) = n { r = r.push(semi(format!("\u{b7}  {n}"), 12.0, ui::alpha(t.ink2, 0.8)).line_height(lh(16.0))); }
            container(r).padding(Padding { top: 0.0, right: 4.0, bottom: 8.0, left: 4.0 }).into()
        };
        let note = |s: String| -> El<'a> { container(tx(s, 13.0, t.ink2).line_height(lh(19.0))).padding(Padding { top: 10.0, right: 4.0, bottom: 0.0, left: 4.0 }).width(Fill).into() };
        let sec = |c: iced::widget::Column<'a, Msg>| -> El<'a> { container(c).padding(Padding { top: 24.0, right: 16.0, bottom: 0.0, left: 16.0 }).width(Fill).into() };
        let card = |rows: Vec<El<'a>>| -> El<'a> {
            let mut c = column![].spacing(0);
            for (i, r) in rows.into_iter().enumerate() {
                if i > 0 { c = c.push(container(Space::new()).height(1).width(Fill).style(ui::plain(t.hairline, 0.0))); }
                c = c.push(r);
            }
            container(c).width(Fill).clip(true).style(move |_: &Theme| container::Style { background: Some(t.surface.into()), border: iced::Border { color: t.hairline, width: 1.0, radius: 14.0.into() }, ..Default::default() }).into()
        };
        // .act: 60 tall, 8 16, 14 gap, a 20 muted icon, 600 15/20 title, 13/18 muted line.
        let act = |icon: &'static str, title: String, sub: Option<String>, end: Option<&'static str>, on: Option<Msg>, danger: bool| -> El<'a> {
            let ink = if danger { t.danger } else { t.ink };
            let mut txt = column![semi(title, 15.0, ink).line_height(lh(20.0))].spacing(1).width(Fill);
            if let Some(s) = sub { txt = txt.push(tx(s, 13.0, t.ink2).line_height(lh(18.0))); }
            let mut r = row![crate::pg_icons::pgi(icon, 20.0, if danger { t.danger } else { t.ink2 }), txt].spacing(14).align_y(Alignment::Center).height(44);
            if let Some(e) = end { r = r.push(crate::pg_icons::pgi(e, 18.0, t.ink2)); }
            let off = on.is_none();
            button(r).padding([8, 16]).width(Fill).style(super::shell_view::wash(t, None, (!off).then_some(t.surface2), 0.0)).on_press_maybe(on)
                .into()
        };
        let status = |on: bool, text: String| -> El<'a> {
            container(row![container(container(Space::new()).width(8).height(8).style(ui::plain(if on { t.accent } else { t.ink2 }, 4.0))).padding(Padding { top: 6.0, ..Padding::ZERO }),
                tx(text, 13.0, if on { t.ink } else { t.warn_ink }).line_height(lh(19.0))].spacing(8))
                .padding(Padding { top: 0.0, right: 4.0, bottom: 6.0, left: 4.0 }).into()
        };
        let mrow = |did: &str, name: String, sub: String, end: El<'a>| -> El<'a> {
            row![self.profile_avatar(t, &name, did, 40.0), column![semi(name, 15.0, t.ink).line_height(lh(20.0)).wrapping(iced::widget::text::Wrapping::None),
                tx(sub, 13.0, t.ink2).line_height(lh(18.0)).wrapping(iced::widget::text::Wrapping::None)].spacing(1).width(Fill), end]
                .spacing(12).align_y(Alignment::Center).height(60).into()
        };
        let textbtn = |label_: &'static str, color: Color, m: Msg| -> El<'a> {
            button(semi(label_, 15.0, color)).padding([10, 11]).style(super::shell_view::wash(t, None, Some(t.surface2), 22.0)).on_press(m).into()
        };
        let mut col = column![].spacing(0);
        // .hero: avatar 72, name 22/28 650, "9 members · You are an admin".
        let role_line = match d.my_role { GroupRole::Owner => "You are the owner", GroupRole::Admin => "You are an admin", GroupRole::Member => "You are a member", GroupRole::Removed => "", GroupRole::Left => "" };
        let hero_sub = match d.my_role {
            GroupRole::Left => "You left this group".to_string(),
            GroupRole::Removed => "You\u{2019}re no longer a member".to_string(),
            _ => format!("{} member{} \u{b7} {role_line}", d.members.len(), if d.members.len() == 1 { "" } else { "s" }),
        };
        col = col.push(container(column![
            gav(t, self.live.open.as_deref().unwrap_or_default(), 72.0),
            Space::new().height(12),
            semi(d.name.clone(), 22.0, t.ink).line_height(lh(28.0)),
            tx(hero_sub, 13.0, t.ink2).line_height(lh(19.0)),
        ].spacing(2).align_x(Alignment::Center)).padding(Padding { top: 16.0, right: 20.0, bottom: 8.0, left: 20.0 }).center_x(Fill));
        if d.my_role == GroupRole::Removed {
            col = col.push(sec(column![act("info", "Why was I removed?".into(), None, Some("chevr"), Some(l(Lm::Removal)), false)]));
        } else if d.my_role == GroupRole::Left {
            col = col.push(container(tx("You can still read the posts on this device. To come back, someone in the group has to invite you again.", 13.0, t.ink2).line_height(lh(19.0))).padding([24, 20]));
        } else {
            // ---- invites (admins)
            if admin {
                let no_host = d.hosts.is_empty();
                let mut c = column![h2("Invites", None), card(vec![
                    act("userplus", "Invite contacts".into(), Some("A private invite to each person".into()), Some("chevr"), Some(l(Lm::AddMembers)), false),
                    act("link", "Create invite link".into(), Some(if no_host { "Appoint a host first. A link names one host.".into() } else { "Valid for 7 days".into() }), Some("plus"), (!no_host).then_some(l(Lm::Invite)), false),
                ])];
                let sent: Vec<_> = self.live.sent.iter().filter(|i| !d.members.iter().any(|m| m.did == i.invitee_did)).collect();
                for i in sent {
                    let name = self.live_name(&i.invitee_did, &i.invitee_name);
                    let (line, _) = gl::sent_invitation_line(i);
                    c = c.push(mrow(&i.invitee_did, name, line.to_string(), Space::new().into()));
                }
                col = col.push(sec(c));
                // ---- public group (§8): the open link, and who may post. Each line under a title keeps the
                // height of its longest variant, so flipping a switch moves nothing.
                let open_on = self.live.setting_on(gl::Setting::Open);
                let ao_on = self.live.setting_on(gl::Setting::AdminsOnly);
                let fetching = self.live.pending.is_some_and(|(s_, _)| s_ == gl::Setting::Link);
                let switch_row = |icon: &'static str, title: &'static str, subs: [&'static str; 2], on: bool, m: Msg| -> El<'a> {
                    let sub = crate::tallest::tallest(subs.iter().map(|s_| El::from(tx(*s_, 13.0, t.ink2).line_height(lh(18.0)))).collect(), if on { 0 } else { 1 });
                    container(row![crate::pg_icons::pgi(icon, 20.0, t.ink2), column![semi(title, 15.0, t.ink).line_height(lh(20.0)), sub].spacing(1).width(Fill),
                            super::profile_view::pg_switch(t, on, m)].spacing(14).align_y(Alignment::Center).height(44))
                        .padding([8, 16]).width(Fill).into()
                };
                let copy_sub = if fetching { "Getting the link\u{2026}".to_string() }
                    else if let Some(link) = self.live.open_link_of_open() { crate::fit::ellipsize(link, ui::SANS, 13.0, 420.0) }
                    else if open_on { "A link that doesn\u{2019}t expire".to_string() }
                    else { "Turn on Open link to share it".to_string() };
                let c = column![h2("Public group", None), card(vec![
                    switch_row("link", "Open link", ["Anyone with the link can join", "Only people you invite can join"], open_on, l(Lm::SetOpen(!open_on))),
                    act("copy", "Copy link".into(), Some(copy_sub), None, (open_on && !fetching).then_some(l(Lm::CopyOpenLink)), false),
                    switch_row("shield", "Admins only", ["Only admins post; everyone reads", "Everyone in the group can post"], ao_on, l(Lm::SetAdminsOnly(!ao_on))),
                ]), note("The open link doesn\u{2019}t expire. Closing it stops new joins; everyone already in stays. While the group is open, no read receipts are sent in it and members see who is online.".into())];
                col = col.push(sec(c));
            }
            // ---- hosts
            let up = d.am_host || d.hosts.iter().any(|h| h.online);
            let host_line = if d.hosts.is_empty() { "This group has no host. Posts wait until a host is appointed.".to_string() }
                // This computer is said in words, not listed as a row: its "Stop hosting" button below stands for it.
                else if d.am_host && d.hosts.len() == 1 { "This computer hosts the group. Posts go through right away.".to_string() }
                else if up { format!("{} host{} online{}. Posts go through right away.", d.hosts.iter().filter(|h| h.online || h.is_me).count().max(1), if d.hosts.len() == 1 { "" } else { "s" }, if d.am_host { ", this computer too" } else { "" }) }
                else { "No host is online. Your posts are saved and send when one is.".to_string() };
            let mut c = column![h2("Hosts", None), status(up && !d.hosts.is_empty(), host_line)];
            for h in d.hosts.iter().filter(|h| !h.is_me) {
                let name = self.live_name(&h.did, &h.name);
                let calls = d.call_hosts.iter().any(|x| x.did == h.did && x.device_id == h.device_id);
                let sub = format!("{}{}", if h.online || h.is_me { "Online" } else { "Offline" }, if calls { " \u{b7} also hosts calls" } else { "" });
                let end: El = if admin { textbtn("Remove", t.ink, l(Lm::Revoke(h.did.clone(), h.device_id.clone()))) } else { Space::new().into() };
                c = c.push(mrow(&h.did, name, sub, end));
            }
            for h in &d.host_requests {
                let name = if h.is_me { "This device".to_string() } else { self.live_name(&h.did, &h.name) };
                let end: El = if admin { textbtn("Cancel", t.ink, l(Lm::Revoke(h.did.clone(), h.device_id.clone()))) } else { Space::new().into() };
                c = c.push(mrow(&h.did, name, "Waiting to accept".into(), end));
            }
            if admin {
                for h in &d.host_offers {
                    let name = if h.is_me { "This device".to_string() } else { self.live_name(&h.did, &h.name) };
                    c = c.push(mrow(&h.did, name, "Offered to host".into(), textbtn("Appoint", t.accent_ink, l(Lm::Appoint(h.did.clone(), h.device_id.clone())))));
                }
            }
            // Desktop: this computer can offer to host (a host stays on); the playground leaves this to an admin's menu.
            let offered = d.host_offers.iter().any(|h| h.is_me);
            c = c.push(container(card(vec![if d.am_host || offered {
                act("server", if d.am_host { "Stop hosting on this computer".into() } else { "Withdraw the offer to host".into() }, None, None, Some(l(Lm::AskHost(false))), false)
            } else {
                act("server", "Host this group on this computer".into(), Some("A host stays on and passes posts on while others are offline".into()), None, Some(l(Lm::AskHost(true))), false)
            }])).padding(Padding { top: 8.0, ..Padding::ZERO }));
            c = c.push(note(format!("Hosts store the group and pass posts on when others are offline. They are members you trust: a host can read everything posted in this group.{}",
                if admin { " Appoint a host from a member\u{2019}s menu; they have to accept. Removing a host changes the group\u{2019}s address within 2 days, so that device can no longer reach it." } else { "" })));
            col = col.push(sec(c));
            // ---- call hosts
            let ready = d.call_hosts.len();
            let ch_line = if ready == 0 { "This group has no call host. Nobody can start a call until an admin appoints one.".to_string() }
                else if d.am_call_host && d.call_hosts.len() == 1 { "This computer carries the calls.".to_string() }
                else { format!("{ready} call host{} ready. Calls start through {}.", if ready == 1 { "" } else { "s" }, if ready == 1 { "it" } else { "them" }) };
            let mut c = column![h2("Call hosts", None), status(ready > 0, ch_line)];
            for h in d.call_hosts.iter().filter(|h| !h.is_me) {
                let name = self.live_name(&h.did, &h.name);
                // Removes the CALL host (was wired to the chat-host revoke).
                let end: El = if admin { textbtn("Remove", t.ink, l(Lm::RevokeCall(h.did.clone(), h.device_id.clone()))) } else { Space::new().into() };
                c = c.push(mrow(&h.did, name, if h.online || h.is_me { "Online".into() } else { "Offline".into() }, end));
            }
            for h in &d.call_host_requests {
                let name = if h.is_me { "This device".to_string() } else { self.live_name(&h.did, &h.name) };
                c = c.push(mrow(&h.did, name, "Waiting to accept".into(), Space::new().into()));
            }
            // This computer: carry the calls (an admin can appoint it in one step; a member accepts when asked).
            let here: Option<El> = if d.am_call_host {
                Some(act("phone", "Stop hosting calls on this computer".into(), None, None, Some(l(Lm::CallsHere(false))), false))
            } else if d.call_host_requested_here {
                Some(act("phone", "Host calls on this computer".into(), Some("An admin asked this computer to carry the group\u{2019}s calls".into()), None, Some(l(Lm::CallsHere(true))), false))
            } else if admin {
                Some(act("phone", "Host calls on this computer".into(), Some("Carries the group\u{2019}s calls; a computer that stays on is best".into()), None, Some(l(Lm::CallsHere(true))), false))
            } else {
                None
            };
            if let Some(h) = here {
                c = c.push(container(card(vec![h])).padding(Padding { top: 8.0, ..Padding::ZERO }));
            }
            c = c.push(note(format!("A call host carries the call. Everyone in a call connects to it, and it passes the audio on, so it can hear the call. That is separate from chat hosting: a chat host is not a call host unless you also appoint it as one. Call hosts are members you trust, and each one chooses to accept.{}",
                if admin { " Appoint one from a member\u{2019}s menu, or host calls on this computer." } else { "" })));
            col = col.push(sec(c));
            // ---- members: owner, admins, then members; me first among equals. A row opens its actions.
            let mut members: Vec<&p2pcore::GroupMember> = d.members.iter().collect();
            members.sort_by_key(|m| (if m.is_owner { 0 } else if m.is_admin { 1 } else { 2 }, !m.is_me));
            let mut c = column![h2("Members", Some(d.members.len()))];
            for m in members {
                let name = self.live_name(&m.did, &m.name);
                let shown = if m.is_me { format!("{name} (you)") } else { name.clone() };
                let mut sub = match &m.invited_by { Some(by) => format!("Invited by {}", first_name(&self.live_name(by, ""))), None => format!("Since {}", super::groups_view::day(m.joined_at * 1000)) };
                if admin && m.flags > 0 && !m.is_owner { sub += &format!(" \u{b7} {} of {} flags", m.flags, gl::FLAGS_TO_REMOVE); }
                if self.blocks.blocked_by_me(&m.did) { sub += " \u{b7} Blocked"; }
                let mut marks = row![].spacing(4).align_y(Alignment::Center);
                if m.is_owner { marks = marks.push(tag(t.primary_c, t.on_primary_c, "Owner")); } else if m.is_admin { marks = marks.push(tag(t.relay_c, t.on_relay_c, "Admin")); }
                if m.is_host { marks = marks.push(container(crate::pg_icons::pgi("server", 14.0, t.warn_ink)).width(22).height(22).center_x(22).center_y(22).style(ui::plain(t.warn, 11.0))); }
                let acts = self.live.actions(m);
                let open = self.live.menu.as_deref() == Some(m.did.as_str());
                let b = button(row![self.profile_avatar(t, &name, &m.did, 40.0), column![semi(shown, 15.0, t.ink).line_height(lh(20.0)).wrapping(iced::widget::text::Wrapping::None),
                        tx(sub, 13.0, t.ink2).line_height(lh(18.0)).wrapping(iced::widget::text::Wrapping::None)].spacing(1).width(Fill), marks].spacing(12).align_y(Alignment::Center).height(48))
                    .padding([6, 8]).width(Fill).style(super::shell_view::wash(t, open.then_some(t.surface2), Some(t.surface2), 14.0))
                    .on_press_maybe((!acts.is_empty()).then(|| l(Lm::Menu(if open { None } else { Some(m.did.clone()) }))));
                c = c.push(b);
                if open {
                    c = c.push(container(self.live_menu(t, m, acts)).padding([4, 12]));
                }
            }
            col = col.push(sec(c));
            // ---- leave
            if active {
                let leave = if d.my_role == GroupRole::Owner {
                    act("leave", "Leave group".into(), Some("The owner can\u{2019}t leave yet".into()), None, None, false)
                } else {
                    act("leave", "Leave group".into(), None, None, Some(l(Lm::AskLeave)), true)
                };
                col = col.push(sec(column![card(vec![leave])]));
            }
        }
        let body = scroll(t, container(container(col.padding(Padding { bottom: 32.0, ..Padding::ZERO })).max_width(640)).center_x(Fill)).height(Fill);
        // What an action here did (or why it didn't), as over the conversation.
        let mut page = column![bar, rule(t)];
        if self.notice.is_some() {
            page = page.push(container(self.notice_bar(t)).padding([8, 20]));
        }
        container(page.push(body).width(Fill).height(Fill)).width(Fill).height(Fill).style(ui::plain(t.bg, 0.0)).into()
    }

    // ---- members pane ----

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
                Action::AppointCall(dev) => item(
                    Icon::Phone,
                    if several { format!("Make device {} a call host", short_dev(&dev)) } else { "Make call host".into() },
                    format!("Asks {first} to carry group calls on that device; they have to accept"),
                    t.ink,
                    l(Lm::AppointCall(m.did.clone(), dev)),
                ),
                Action::RevokeCall(dev) => item(
                    Icon::Phone,
                    if several { format!("Stop device {} carrying calls", short_dev(&dev)) } else { "Stop hosting calls".into() },
                    "Calls go through the other call hosts".into(),
                    t.ink,
                    l(Lm::RevokeCall(m.did.clone(), dev)),
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
        // The invite QR at full size (click anywhere to go back).
        if self.live.qr_full
            && let Some(q) = &self.live.qr
        {
            let big = container(canvas(super::QrView(q)).width(Length::Fixed(560.0)).height(Length::Fixed(560.0))).padding(24).style(ui::plain(Color::WHITE, 16.0));
            let layer = container(column![big, tx("Click anywhere to go back", 14.0, Color::WHITE)].spacing(12).align_x(Alignment::Center))
                .center_x(Fill)
                .center_y(Fill)
                .width(Fill)
                .height(Fill)
                .style(ui::plain(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.85 }, 0.0));
            return Some(opaque(mouse_area(layer).on_press(l(Lm::QrFull(false)))));
        }
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
            LDialog::Join { token, peek, checking } => {
                let mut c = column![
                    title("Join a group".into()),
                    text_input("Paste the invite link (tinline://g/\u{2026}) or code", token)
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
                            info = info.push(tx(format!("Invite valid until {}", when(exp)), 13.0, t.ink2));
                        }
                        c = c.push(container(info).padding(12).width(Fill).style(ui::plain(t.surface2, 12.0))).push(para(
                            "Joining connects this computer to the group\u{2019}s hosts. Members see your name; only hosts learn how to reach this computer. It can take up to a minute."
                                .into(),
                        ));
                    }
                    None if *checking => c = c.push(para("Checking with the group\u{2019}s host\u{2026}".into())),
                    None if !token.is_empty() => c = c.push(tx("That link didn\u{2019}t work: it isn\u{2019}t a group invite, it expired, or its host is offline.", 13.0, t.error)),
                    None => c = c.push(para("Admins of a group can make an invite. It works for 7 days.".into())),
                }
                c
            }
            LDialog::Token { text } => {
                let mut c = column![
                    title(format!("Invite link for {gname}")),
                    para("Anyone with this link or QR can join for 7 days. Share it over an app you trust; you see who joined and can flag them out. To invite your contacts directly, use Add members.".into()),
                ]
                .spacing(14);
                match (text, &self.live.qr) {
                    (Some(tok), q) => {
                        if let Some(q) = q {
                            // Large and high contrast; click for full size.
                            c = c.push(
                                button(
                                    container(canvas(super::QrView(q)).width(Length::Fixed(400.0)).height(Length::Fixed(400.0)))
                                        .padding(12)
                                        .style(ui::plain(Color::WHITE, 12.0)),
                                )
                                .padding(0)
                                .style(button::text)
                                .on_press(l(Lm::QrFull(true))),
                            );
                            c = c.push(tx("Click the code to show it full size.", 12.0, t.ink2));
                        }
                        c = c.push(container(mono(tok.clone(), 13.0, t.ink).wrapping(iced::widget::text::Wrapping::Glyph)).padding(10).width(Fill).style(ui::plain(t.surface2, 10.0)));
                        c = c.push(row![
                            Space::new().width(Fill),
                            btn(t, Kind::Ghost, "Close", Some(l(Lm::Dismiss))),
                            pill(t, Kind::Primary, Some(Icon::Copy), "Copy link", Some(l(Lm::CopyToken))),
                        ]
                        .spacing(8));
                    }
                    (None, _) if self.live.error.is_none() => c = c.push(para("Making it\u{2026}".into())),
                    (None, _) => c = c.push(row![Space::new().width(Fill), btn(t, Kind::Ghost, "Close", Some(l(Lm::Dismiss)))]),
                }
                c.align_x(Alignment::Center)
            }
            LDialog::AddMembers { picked } => {
                let d = self.live.detail.as_ref()?;
                let contacts: Vec<(String, String)> = self.contacts.iter().map(|c| (c.did.clone(), Self::display(c))).collect();
                let rows = gl::pick_rows(&contacts, d, &self.live.sent, |did| self.blocks.blocked_by_me(did) || self.blocks.blocked_me(did).is_some());
                let mut list = column![].spacing(2);
                if rows.is_empty() {
                    list = list.push(para("No contacts to add. People you block, or who block you, aren\u{2019}t listed.".into()));
                }
                for r in rows {
                    let on = picked.contains(&r.did);
                    let sub = r.why.map(str::to_string).or_else(|| r.invited.as_ref().map(|i| gl::sent_invitation_line(i).0.to_string()));
                    let mut info = column![semi(r.name.clone(), 14.0, if r.why.is_none() { t.ink } else { t.ink2 })].spacing(1);
                    if let Some(sub) = sub {
                        info = info.push(tx(sub, 12.0, t.ink2));
                    }
                    let mut line = row![avatar(t, &r.name, &r.did, 32.0), info.width(Fill)].spacing(10).align_y(Alignment::Center);
                    if r.why.is_none() {
                        line = line.push(ui::icon(if on { Icon::Check } else { Icon::UserPlus }, 18.0, if on { t.primary } else { t.ink2 }));
                    }
                    list = list.push(
                        button(line)
                            .padding([6, 10])
                            .width(Fill)
                            .style(ui::row_style(t, on))
                            .on_press_maybe(r.why.is_none().then(|| l(Lm::Pick(r.did.clone())))),
                    );
                }
                column![
                    title(format!("Add to {gname}")),
                    para("Each person gets the invitation in your 1:1 chat and joins when they accept. It is valid for 7 days.".into()),
                    scroll(t, list).height(Length::Fixed(320.0)),
                ]
                .spacing(14)
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
            LDialog::Online => {
                let rows = gl::online_rows(&self.live.online);
                let n = rows.len();
                let mut list = column![].spacing(2);
                if rows.is_empty() {
                    list = list.push(para("No one is online right now. People show here while they\u{2019}re connected.".into()));
                }
                for m in rows {
                    let name = self.live_name(&m.did, &m.name);
                    let mut line = row![self.profile_avatar(t, &name, &m.did, 32.0), semi(name.clone(), 14.0, t.ink).wrapping(iced::widget::text::Wrapping::None).width(Fill)]
                        .spacing(10)
                        .align_y(Alignment::Center);
                    if m.is_admin {
                        line = line.push(tag(t.relay_c, t.on_relay_c, "Admin"));
                    }
                    list = list.push(container(line).padding([6, 10]).width(Fill));
                }
                let mut c = column![tx(gname.clone(), 13.0, t.ink2), title(gl::online_label(n)),
                    para("Members a host has seen in the last minute. It can be a few seconds behind.".into())].spacing(14);
                if n >= gl::ONLINE_CAP {
                    c = c.push(para(format!("A host lists up to {} people at a time, so there may be more.", gl::ONLINE_CAP)));
                }
                if !self.live.presence_visible {
                    c = c.push(para("You\u{2019}re not shown to others here. You can change that in Settings \u{203a} Privacy.".into()));
                }
                c.push(scroll(t, list).height(Length::Fixed(320.0))).push(row![Space::new().width(Fill), btn(t, Kind::Primary, "Close", Some(l(Lm::Dismiss)))])
            }
            LDialog::OpenGroup => column![
                title(format!("Open {gname} to anyone with the link?")),
                para("Anyone holding the link can join, with no invite and no expiry. Share it where you\u{2019}d like people to find the group.".into()),
                para("While it\u{2019}s open, no read receipts are sent in this group, and members see who is online. Each person can hide themselves in their settings.".into()),
                para("You can close the link any time; everyone already in stays.".into()),
            ]
            .spacing(14),
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
            LDialog::AddMembers { picked } => {
                let label_ = if picked.len() > 1 { format!("Invite {} people", picked.len()) } else { "Invite".to_string() };
                let busy = self.live.busy;
                c.push(row![
                    Space::new().width(Fill),
                    btn(t, Kind::Ghost, "Cancel", (!busy).then(|| l(Lm::Dismiss))),
                    btn(t, Kind::Primary, if busy { "Sending\u{2026}" } else { &label_ }, (!busy && !picked.is_empty()).then(|| l(Lm::Confirm))),
                ]
                .spacing(8))
            }
            LDialog::Flag { .. } => c.push(actions("Cancel", Some(("Flag", Kind::Danger)))),
            LDialog::Admin { admin, .. } => c.push(actions("Cancel", Some((if *admin { "Make admin" } else { "Remove as admin" }, Kind::Primary)))),
            LDialog::Leave => c.push(actions("Cancel", Some(("Leave", Kind::Danger)))),
            LDialog::Request { .. } => c.push(actions("Cancel", Some(("Send request", Kind::Primary)))),
            LDialog::Host { accept } => c.push(actions("Cancel", Some(if *accept { ("Host here", Kind::Primary) } else { ("Stop hosting", Kind::Danger) }))),
            LDialog::OpenGroup => c.push(actions("Cancel", Some(("Open the group", Kind::Primary)))),
            LDialog::Token { .. } | LDialog::Removal | LDialog::Online => c,
        };
        let card = ui::raised(container(c).padding(28).width(540), t, ui::Depth::Sheet, ui::sheet_surface(t, 24.0));
        let layer = container(opaque(card))
            .center_x(Fill)
            .center_y(Fill)
            .width(Fill)
            .height(Fill)
            .style(ui::plain(Color { r: 0.06, g: 0.08, b: 0.07, a: 0.42 }, 0.0));
        Some(opaque(mouse_area(layer).on_press(l(Lm::Dismiss))))
    }
}

/// groups.ts `dayOnly`: "Sep 29".
fn day_only(ms: u64) -> String {
    use chrono::{Local, TimeZone};
    Local.timestamp_millis_opt(ms as i64).single().map(|d| d.format("%b %-d").to_string()).unwrap_or_default()
}

/// Conversation.svelte day chip: Today, Yesterday, else "Thursday 8 Oct".
fn chip_day(ms: u64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let (Some(d), now) = (Local.timestamp_millis_opt(ms as i64).single(), Local::now()) else { return String::new() };
    match (now.date_naive() - d.date_naive()).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ if d.year() == now.year() => d.format("%A %-d %b").to_string(),
        _ => d.format("%A %-d %b %Y").to_string(),
    }
}

/// Greedy wrap at `max` px (15px body): line count and the last line's width (where the time goes).
fn live_wrap(s: &str, max: f32) -> (usize, f32) {
    use crate::fit::width;
    let space = width(" ", ui::SANS, 15.0);
    let (mut lines, mut cur) = (0usize, 0.0f32);
    for para in s.split('\n') {
        lines += 1;
        cur = 0.0;
        for w in para.split(' ') {
            let ww = width(w, ui::SANS, 15.0);
            if cur > 0.0 && cur + space + ww > max { lines += 1; cur = ww.min(max); } else { cur = if cur > 0.0 { cur + space + ww } else { ww.min(max) }; }
        }
    }
    (lines.max(1), cur)
}

/// "Delivered n/N", "Read n/N", then who: eligible members by their level, and kept statements of
/// people no longer counted. A statement is a device's signed claim ("on their device").
pub(super) fn group_receipt_lines(r: &p2pcore::GroupReceiptStatus) -> Vec<String> {
    use p2pcore::ReceiptLevel as L;
    let who = |m: &p2pcore::GroupMemberReceipts| if m.name.is_empty() { "Someone".to_string() } else { first_name(&m.name).to_string() };
    let names = |want: L| -> Vec<String> { r.members.iter().filter(|m| m.eligible && m.receipts.level == want).map(who).collect() };
    let mut out = vec![format!("Delivered {}/{}", r.delivered, r.eligible), format!("Read {}/{}", r.read, r.eligible)];
    for (label, want) in [("Read", L::Read), ("On their device", L::Delivered), ("Not yet", L::Unconfirmed)] {
        let n = names(want);
        if !n.is_empty() {
            out.push(format!("{label}: {}", n.join(", ")));
        }
    }
    let gone: Vec<String> = r.members.iter().filter(|m| !m.eligible).map(who).collect();
    if !gone.is_empty() {
        out.push(format!("Also confirmed: {}", gone.join(", ")));
    }
    out
}
