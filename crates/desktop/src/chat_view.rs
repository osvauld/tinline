//! Chat screens: the Chats tab of the sidebar and the conversation in the main pane.
//! Laid out after docs/design/DesktopChat.dc.html.

use chrono::{DateTime, Datelike, Local, TimeZone};
use iced::widget::{button, column, container, mouse_area, progress_bar, row, text, text_editor, Space};
use p2pcore::{AttachmentKind, Contact, DeliveryState, Message as ChatMsg, ReceiptProofKind, ReceiptTick, TransferState};

use crate::voice::{self, VoiceView};
use super::*;
use crate::app::chat::{preview_of, Cm, Thumb, ViewBody, COMPOSER_ID};
use crate::blocks::Composer;
use crate::media::{self, Class};
use crate::geom;
use crate::shaped::shaped;

fn local(ms: u64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(ms as i64).single()
}

fn hhmm(ms: u64) -> String {
    local(ms).map(|d| d.format("%H:%M").to_string()).unwrap_or_default()
}

/// "Today", "Yesterday", a weekday within the last week, else "12 Sep" (with the year if old).
/// The words are `present::time::day_label`; the clock and the zone are read here.
fn day_label(ms: u64) -> String {
    let now = Local::now();
    present::time::day_label((ms / 1000) as i64, now.timestamp(), now.offset().local_minus_utc())
}

/// Conversation.svelte day chip (`dayLabel`): Today, Yesterday, else "Thursday 8 Oct" (the year when not this year).
fn chip_day(ms: u64) -> String {
    let (Some(d), now) = (local(ms), Local::now()) else { return String::new() };
    match (now.date_naive() - d.date_naive()).num_days() {
        0 => crate::i18n::tr_str("common_today").into(),
        1 => crate::i18n::tr_str("common_yesterday").into(),
        _ if d.year() == now.year() => d.format("%A %-d %b").to_string(),
        _ => d.format("%A %-d %b %Y").to_string(),
    }
}

/// The sidebar's time column: the time today, else the day (`present::time::list_time`).
pub(super) fn list_time(ms: u64) -> String {
    let now = Local::now();
    present::time::list_time((ms / 1000) as i64, now.timestamp(), now.offset().local_minus_utc())
}

fn size_text(b: u64) -> String {
    present::time::bytes(b)
}

fn kind_label(name: &str) -> String {
    std::path::Path::new(name).extension().and_then(|e| e.to_str()).map(|e| e.to_uppercase()).unwrap_or_default()
}

/// "Oct 10, 15:20" for a block's unix seconds.
fn block_when(secs: u64) -> String {
    local(secs.saturating_mul(1000)).map(|d| d.format("%b %-d, %H:%M").to_string()).unwrap_or_default()
}

/// What a chat row's second line says (ChatsList `preview()` and its draft / waiting forms).
pub(super) enum Preview {
    Plain(String, Option<Color>),
    Draft(String),
    Waiting(String),
}

fn rule<'a>(t: Tok) -> El<'a> {
    container(Space::new()).width(Fill).height(1).style(ui::plain(t.line, 0.0)).into()
}

impl App {
    /// ChatsList.svelte: 1:1 and group conversations together, most recent first, behind the
    /// All / Unread chips. Rows are 72px (8px padding, 48px avatar, 12px gap) inside 8px side margins.
    pub(super) fn chat_list(&self, t: Tok, q: &str) -> El<'_> {
        let unread_only = self.shell_search.unread_only;
        let mut items: Vec<(u64, El)> = Vec::new();
        for r in &self.chat.chats {
            let me = self.is_me(&r.peer_did);
            if !self.selectable(&r.peer_did) || (r.last_activity == 0 && !me) || (me && r.last_activity == 0) {
                continue;
            }
            let name = self.chat_name(&r.peer_did);
            if (!q.is_empty() && !name.to_lowercase().contains(q)) || (unread_only && r.unread == 0) {
                continue;
            }
            let sel = self.sel.as_ref() == Some(&r.peer_did) && !self.chat.info && self.live.open.is_none();
            let draft = self.chat.draft_for(&r.peer_did);
            let pv = if let Some(st) = self.blocks.status(&r.peer_did) {
                Preview::Plain(st.to_string(), Some(t.error))
            } else if !draft.trim().is_empty() {
                Preview::Draft(draft.replace('\n', " "))
            } else if r.last_outgoing && r.last_delivery == DeliveryState::Pending && !self.status.online {
                Preview::Waiting(r.preview.clone())
            } else if me && r.preview.is_empty() {
                Preview::Plain(crate::app::self_sub().to_string(), None)
            } else {
                Preview::Plain(r.preview.clone(), None)
            };
            let face = self.profile_avatar(t, &name, &r.peer_did, 48.0);
            // The mark of my last message (its slot kept from the start: the row's last_outgoing is known now).
            let mark = if r.last_outgoing && matches!(pv, Preview::Plain(..)) && !me {
                self.list_mark(false, &r.peer_did, r.last_id.as_deref())
            } else { None };
            items.push((r.last_activity, self.pg_chat_row(t, sel, face, name, r.last_activity, r.unread, false, pv, Msg::Select(r.peer_did.clone()), mark)));
        }
        for s in &self.live.list {
            if !super::super::group_live::active(s.my_role) || (!q.is_empty() && !s.name.to_lowercase().contains(q)) || (unread_only && s.unread == 0) {
                continue;
            }
            let at = s.last.as_ref().map_or(0, |m| m.at);
            let sel = self.live.open.as_deref() == Some(s.id.as_str());
            let text = super::super::group_live::preview(s, &self.live.me);
            let pv = if s.last.as_ref().is_some_and(|m| m.outgoing && m.delivery == p2pcore::GroupDelivery::Waiting) { Preview::Waiting(text) } else { Preview::Plain(text, None) };
            let face = ui::group_face(t, &s.id, 48.0);
            let mark = if matches!(pv, Preview::Plain(..)) {
                self.list_mark(true, &s.id, s.last.as_ref().filter(|m| m.outgoing && m.kind == p2pcore::GroupItemKind::Post).map(|m| m.id.as_str()))
            } else { None };
            items.push((at, self.pg_chat_row(t, sel, face, s.name.clone(), at, s.unread, false, pv, Msg::Live(super::super::group_live::Lm::Open(s.id.clone())), mark)));
        }
        items.sort_by(|a, b| b.0.cmp(&a.0));
        let mut list = column![].spacing(0);
        if let Some(b) = self.live_requests(t) {
            list = list.push(b);
        }
        let empty = items.is_empty();
        for (_, el) in items {
            list = list.push(el);
        }
        if empty && q.is_empty() {
            // EmptyLine `quiet`: the detail pane has the hero, the column just says where chats go.
            list = list.push(self.empty_quiet(t, if unread_only { crate::i18n::tr_str("chat_youre_all_caught_up") } else { crate::i18n::tr_str("chat_your_chats_show_up") }));
        }
        container(list).padding([0, 8]).width(Fill).into()
    }

    /// One ChatsList row. `when` is listStamp(); the stamp turns accent-ink bold while unread.
    #[allow(clippy::too_many_arguments)]
    /// `mark`: my last message's tick before the preview (`Some(None)` keeps its slot empty until read).
    pub(super) fn pg_chat_row<'a>(&self, t: Tok, sel: bool, face: El<'a>, title: String, at: u64, unread: u32, muted: bool, pv: Preview, on: Msg, mark: Option<Option<crate::ticks::Vis>>) -> El<'a> {
        use crate::fit::{ellipsize, width};
        // 380 pane - 2x8 list margin - 2x8 row padding - 48 avatar - 12 gap = the text column.
        const MID: f32 = 380.0 - 1.0 - 16.0 - 16.0 - 48.0 - 12.0;
        let hot = unread > 0 && !muted;
        let stamp = list_time(at);
        let stamp_font = if hot { ui::SANS_BOLD } else { ui::SANS };
        let stamp_w = width(&stamp, stamp_font, 11.0);
        let name = ellipsize(&title, ui::SANS_SEMI, 15.0, MID - stamp_w - 8.0);
        let top = row![semi(name, 15.0, t.ink).line_height(lh(21.0)).width(Fill).wrapping(iced::widget::text::Wrapping::None),
            text(stamp).size(11).font(stamp_font).line_height(lh(21.0)).color(if hot { t.accent_ink } else { t.ink2 })].spacing(8).align_y(Alignment::Center);
        let badge_w = if unread > 0 { width(&unread.min(99).to_string(), ui::SANS_BOLD, 11.0).max(8.0) + 10.0 + 8.0 } else { 0.0 };
        // The list mark: the conversation's tick component at 12 px, in a fixed slot (no layout shift).
        let mark_w = 12.0 * crate::tin_ticks::VB_W / crate::tin_ticks::VB_H;
        let mark_el: Option<El<'a>> = mark.map(|v| {
            let inks = crate::ticks::Inks { meta: t.ink2, read: t.tick_read, bad: t.tick_bad, bg: if sel { t.accent_soft } else { t.bg } };
            let g: El<'a> = match v {
                Some(v) => crate::ticks::glyph(v, None, inks, 12.0, self.fx.now),
                None => Space::new().width(mark_w).height(12).into(),
            };
            container(g).width(mark_w + 4.0).height(21).center_y(21).into()
        });
        let room = MID - badge_w - if mark_el.is_some() { mark_w + 4.0 } else { 0.0 };
        let line: El = match pv {
            Preview::Plain(s, color) => text(ellipsize(&s, ui::SANS, 13.0, room)).size(13).line_height(lh(21.0)).color(color.unwrap_or(t.ink2)).wrapping(iced::widget::text::Wrapping::None).into(),
            Preview::Draft(s) => {
                let lead = crate::i18n::tr_str("chat_draft");
                let rest = ellipsize(&s, ui::SANS, 13.0, room - width(lead, ui::SANS_SEMI, 13.0));
                iced::widget::rich_text![iced::widget::span::<(), iced::Font>(lead).font(ui::SANS_SEMI).color(t.accent_ink), iced::widget::span::<(), iced::Font>(rest).color(t.ink2)]
                    .size(13).line_height(lh(21.0)).wrapping(iced::widget::text::Wrapping::None).into()
            }
            Preview::Waiting(s) => {
                let lead_w = 13.0 + 3.0 + width(crate::i18n::tr_str("chat_waiting"), ui::SANS_SEMI, 13.0);
                let rest = ellipsize(&format!(" \u{b7} {s}"), ui::SANS, 13.0, room - lead_w);
                row![crate::pg_icons::pgi("clock", 13.0, t.gold_ink), Space::new().width(3), semi(crate::i18n::tr_str("chat_waiting"), 13.0, t.gold_ink).line_height(lh(21.0)),
                    tx(rest, 13.0, t.ink2).line_height(lh(21.0)).wrapping(iced::widget::text::Wrapping::None)].align_y(Alignment::Center).into()
            }
        };
        let line: El<'a> = match mark_el {
            Some(m) => row![m, line].align_y(Alignment::Center).into(),
            None => line,
        };
        let mut bot = row![container(line).width(Fill).clip(true)].spacing(8).align_y(Alignment::Center);
        if unread > 0 {
            bot = bot.push(super::shell_view::t_badge(t, unread, muted));
        }
        button(row![face, column![top, bot].spacing(2).width(Fill)].spacing(12).align_y(Alignment::Center).height(56))
            .padding(8).width(Fill)
            .style(super::shell_view::wash(t, sel.then_some(t.accent_soft), (!sel).then_some(t.surface2), 14.0))
            .on_press(on).into()
    }

    pub(super) fn conversation_view<'a>(&'a self, t: Tok, c: &'a Contact) -> El<'a> {
        let name = Self::display(c);
        let calling = self.call.is_some();
        // "You": no contact behind it, so no presence or call.
        let me = self.is_me(&c.did);
        let contact = me || self.contacts.iter().any(|x| x.did == c.did);
        // Conversation.svelte `.bar`: 60 tall, 8 padding, surface, hairline under. The head (avatar 36, name
        // 17/22 semibold, status 13/18 muted) opens the details; call, video, find and details are 44 round buttons.
        let online = self.chat.online.contains(&c.did);
        let (sub, sub_color) = if me {
            (crate::app::self_sub().to_string(), t.ink2)
        } else if let Some(st) = self.blocks.status(&c.did) {
            (st.to_string(), t.error)
        } else if !contact {
            (crate::i18n::tr_str("chat_not_in_your_contacts").to_string(), t.ink2)
        } else if self.status.started && !self.status.online {
            (crate::i18n::tr_str("chat_waiting_for_connection").to_string(), t.ink2)
        } else {
            use crate::blocks::CallGate;
            let only = matches!(self.blocks.call_gate(&c.did, contact), CallGate::NotAllowed);
            // Presence shows only while they're active with me (consent-based, 2026-10-17); otherwise
            // it is unknown: no word rather than a false "Offline" (the line keeps its height).
            let sub = match (online, only) {
                (true, true) => crate::i18n::tr_str("chat_online_messages_only"),
                (true, false) => crate::i18n::tr_str("common_online"),
                (false, true) => crate::i18n::tr_str("common_messages_only"),
                (false, false) => "",
            };
            (sub.to_string(), t.ink2)
        };
        let fit_name = crate::fit::ellipsize(&name, ui::SANS_SEMI, 17.0, 360.0);
        let who = button(
            row![self.profile_avatar(t, &name, &c.did, 36.0),
                column![semi(fit_name, 17.0, t.ink).line_height(lh(22.0)).wrapping(iced::widget::text::Wrapping::None),
                    tx(sub, 13.0, sub_color).line_height(lh(18.0)).wrapping(iced::widget::text::Wrapping::None)]]
                .spacing(10)
                .align_y(Alignment::Center),
        )
        .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 6.0 })
        .height(44)
        .width(Fill)
        .style(super::shell_view::wash(t, None, Some(t.surface2), 12.0))
        .on_press_maybe((!me && contact).then_some(Msg::Chat(Cm::Info(true))));
        let ib = |icon: &'static str, tip: String, on: Option<Msg>, dim: bool| -> El<'a> {
            let b = button(container(crate::pg_icons::pgi(icon, 24.0, if dim { ui::alpha(t.ink2, 0.45) } else { t.ink })).center_x(44).center_y(44))
                .padding(0).width(44).height(44).style(super::shell_view::wash(t, None, (!dim).then_some(t.surface2), 22.0)).on_press_maybe(on);
            iced::widget::tooltip(b, text(tip).size(13), iced::widget::tooltip::Position::Bottom).into()
        };
        let mut head_row = row![who].spacing(2).align_y(Alignment::Center);
        if !me && contact {
            // Dimmed with the reason as its tooltip while calling isn't possible (blocked, or messages only).
            use crate::blocks::CallGate;
            let gate = self.blocks.call_gate(&c.did, contact);
            let ok = matches!(gate, CallGate::Ok);
            let why = match gate { CallGate::Ok => crate::i18n::tr_str("common_call").to_string(), CallGate::NotAllowed => crate::i18n::tr_str("common_messages_only_calling_needs").into(), CallGate::Closed(_) => crate::i18n::tr_str("common_blocked").into() };
            // My call with them is running (decided 2026-10-14): the phone turns green and returns to it, as a group's does.
            let theirs = self.call.as_ref().is_some_and(|k| k.info.peer_did == c.did && !(k.info.incoming && !k.answered && k.state == p2pcore::CallState::Ringing));
            let phone: El = if theirs {
                let b = button(container(crate::pg_icons::pgi("phone", 24.0, t.accent_ink)).center_x(44).center_y(44)).padding(0).width(44).height(44)
                    .style(super::shell_view::wash(t, Some(t.accent_soft), Some(t.accent_soft), 22.0)).on_press(Msg::CallUi(crate::call_ui::Action::Restore));
                iced::widget::tooltip(b, text(crate::i18n::tr_str("chat_return_to_call")).size(13), iced::widget::tooltip::Position::Bottom).into()
            } else { ib("phone", why.clone(), (ok && !calling).then(|| Msg::CallPressed(c.did.clone())), !ok) };
            head_row = head_row
                .push(phone)
                .push(ib("video", if ok { crate::i18n::tr_str("chat_video_call").into() } else { why }, (ok && !calling && !self.settings.no_video).then(|| Msg::DialVideo(c.did.clone())), !ok));
        }
        head_row = head_row.push(ib("search", crate::i18n::tr_str("chat_find_in_this_chat_ctrl").into(), Some(Msg::Chat(Cm::Find(Some(String::new())))), false));
        if !me && contact {
            head_row = head_row.push(ib("info", crate::i18n::tr_str("chat_chat_details").into(), Some(Msg::Chat(Cm::Info(true))), false));
        }
        // FindBar.svelte covers the header in the same 60 px box, so nothing moves when it opens.
        let head: El = match self.find_bar(t) {
            Some(bar) => bar,
            None => container(head_row).padding(8).height(60).width(Fill).style(ui::plain(t.surface, 0.0)).into(),
        };
        let mut top = column![head, rule(t)];
        if self.notice.is_some() {
            top = top.push(container(self.notice_bar(t)).padding([8, 20]));
        }

        let body: El = if self.chat.drop_hover {
            container(
                container(
                    column![
                        ui::icon(Icon::Download, 32.0, t.primary),
                        bold(crate::i18n::tr_args("chat_drop_to_send_to", &[("name", &(name).to_string())]), 20.0, t.ink),
                        tx(
                            crate::i18n::tr_args("chat_encrypted_on_this_computer", &[("name", &(name).to_string())]),
                            14.0,
                            t.ink2
                        ),
                    ]
                    .spacing(8)
                    .align_x(Alignment::Center),
                )
                .padding(40)
                .center_x(Fill)
                .center_y(Fill)
                .style(ui::outlined(ui::alpha(t.primary, 0.08), t.primary, 20.0)),
            )
            .padding(24)
            .width(Fill)
            .height(Fill)
            .into()
        } else {
            self.message_list(t, &name)
        };

        let contact = me || self.contacts.iter().any(|x| x.did == c.did);
        let bottom = match self.blocks.composer(&c.did, contact) {
            Composer::Open => self.composer(t, &name),
            Composer::BlockedByMe => self.composer_footer(
                t,
                row![
                    tx(present::block::blocked_by_me_line(name.clone()), 14.0, t.ink2).width(Fill),
                    pill(t, Kind::Primary, None, crate::i18n::tr_str("common_unblock"), Some(Msg::Unblock(c.did.clone()))),
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .into(),
            ),
            Composer::BlockedMe => self.composer_footer(
                t,
                tx(present::block::cant_send_text(name.clone()), 14.0, t.ink2).into(),
            ),
            Composer::NotContact => self.composer_footer(
                t,
                row![
                    tx(present::block::not_contact_line(name.clone()), 14.0, t.ink2).width(Fill),
                    pill(t, Kind::Quiet, Some(Icon::UserPlus), crate::i18n::tr_str("common_add_contact"), Some(Msg::OpenAdd)),
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .into(),
            ),
        };
        column![top, body, bottom].width(Fill).height(Fill).into()
    }

    /// In place of the composer while a block is on: chats are never deleted (U24), they just
    /// stop taking new messages.
    fn composer_footer<'a>(&self, t: Tok, body: El<'a>) -> El<'a> {
        column![rule(t), container(body).padding([14, 20]).width(Fill)].into()
    }

    /// The quiet line (we block them) or the signed notice (they block us) under the messages.
    fn block_note<'a>(&self, t: Tok, peer: &str, name: &str) -> El<'a> {
        let first = present::names::first_word(name).to_string();
        if self.blocks.blocked_by_me(peer) {
            let at = self.blocks.list.iter().find(|b| b.did == peer).map(|b| b.blocked_at).unwrap_or(0);
            let when = if at > 0 { format!(" \u{b7} {}", block_when(at)) } else { String::new() };
            return container(container(tx(crate::i18n::tr_args("chat_you_blocked", &[("name", &(first).to_string()),("when", &(when).to_string())]), 12.0, t.ink2)).padding([4, 12]).style(ui::plain(t.surface2, 999.0)))
                .center_x(Fill)
                .padding([8, 0])
                .into();
        }
        let Some(at) = self.blocks.blocked_me(peer) else { return Space::new().into() };
        let (title, body) = (present::block::blocked_you_title(name.to_string()), present::block::blocked_you_body(name.to_string()));
        container(
            container(
                column![
                    bold(title, 15.0, t.on_error_c),
                    tx(body, 13.0, t.on_error_c),
                    tx(crate::i18n::tr_args("chat_signed_by", &[("name", &(first).to_string()),("value", &(block_when(at)).to_string())]), 12.0, ui::alpha(t.on_error_c, 0.8)),
                ]
                .spacing(4),
            )
            .padding([12, 16])
            .max_width(460)
            .style(ui::plain(t.error_c, 14.0)),
        )
        .center_x(Fill)
        .padding([8, 0])
        .into()
    }

    /// Conversation.svelte `.msgs`: 6 16 19 padding on --canvas; day chips, the "n new messages" divider,
    /// call records as pills on the caller's side, and runs of one sender's messages stacked (10 px between
    /// runs, 2 px inside, the facing corners tight). The offline strip lies over the top of the list.
    fn message_list<'a>(&'a self, t: Tok, name: &str) -> El<'a> {
        let first = present::names::first_word(name).to_string();
        if self.chat.msgs.is_empty() {
            let mut empty = column![].spacing(0).align_x(Alignment::Center).max_width(320);
            if !self.chat.loading {
                // .empty: 52 round shield on surface-2, "Say hello to {first}", one line under it.
                empty = empty
                    .push(container(crate::pg_icons::pgi("shield", 24.0, t.ink2)).width(52).height(52).center_x(52).center_y(52).style(ui::plain(t.surface2, 26.0)))
                    .push(Space::new().height(14))
                    .push(semi(crate::i18n::tr_args("chat_say_hello_to", &[("name", &(first).to_string())]), 15.0, t.ink).line_height(lh(21.0)))
                    .push(Space::new().height(4))
                    .push(tx(crate::i18n::tr_str("chat_messages_go_straight_to"), 13.0, t.ink2).line_height(lh(19.0)));
                if let Some(peer) = self.chat.peer.as_deref() {
                    empty = empty.push(self.block_note(t, peer, name));
                }
            }
            let body = container(empty).padding([32, 24]).center_x(Fill).center_y(Fill).height(Fill).style(ui::plain(t.canvas, 0.0));
            return self.with_strip(t, body.into());
        }
        let peer = self.chat.peer.clone().unwrap_or_default();
        let msgs = &self.chat.msgs;
        // Call records with this person, merged in by time.
        let calls: Vec<&p2pcore::CallRecord> = {
            let mut v: Vec<_> = self.recents.iter().filter(|r| r.peer_did == peer).collect();
            v.sort_by_key(|r| r.started_at);
            v
        };
        // The unread divider sits before the oldest of the last n messages from them.
        let n_unread = self.chat.unread_at_open as usize;
        let divider = if n_unread > 0 {
            msgs.iter().rev().filter(|m| !m.outgoing).nth(n_unread - 1).map(|m| m.id.clone())
        } else { None };
        let found = self.find_current();
        let terms: Vec<String> = self.chat.find.as_deref().map(|q| q.split_whitespace().map(str::to_lowercase).collect()).unwrap_or_default();
        let join = |a: &ChatMsg, b: &ChatMsg| a.outgoing == b.outgoing && day_label(a.at) == day_label(b.at) && a.at.abs_diff(b.at) <= geom::GROUP_GAP_MS;
        let name_s = name.to_string();
        let list = iced::widget::responsive(move |size| {
            let name = name_s.as_str();
            // .bub max-width: min(78 %, 560) of the list's content box (16 px each side).
            let max_bub = ((size.width - 32.0) * 0.78).min(560.0);
            let mut col = column![].spacing(0);
            if !self.chat.peer.as_deref().is_some_and(|p| self.is_me(p)) && (self.chat.older.is_some() || self.chat.fetching) {
                col = col.push(container(self.history_note(t, name)).padding(Padding { bottom: 6.0, ..Default::default() }));
            }
            let mut last_day = String::new();
            let mut ci = 0;
            let mut prev_kind_call = false;
            let push_day = |col: iced::widget::Column<'a, Msg>, at: u64, last_day: &mut String| -> (iced::widget::Column<'a, Msg>, bool) {
                let d = day_label(at);
                if d == *last_day { return (col, false); }
                *last_day = d;
                let chip = container(semi(chip_day(at), 11.0, t.ink2).line_height(lh(16.0))).padding([3, 10]).style(move |_: &Theme| container::Style {
                    background: Some(t.surface.into()), border: Border { color: t.hairline, width: 1.0, radius: 999.0.into() }, ..Default::default() });
                (col.push(container(chip).center_x(Fill).padding([10, 0])), true)
            };
            for (i, m) in msgs.iter().enumerate() {
                while ci < calls.len() && calls[ci].started_at * 1000 <= m.at {
                    let (c2, day) = push_day(col, calls[ci].started_at * 1000, &mut last_day);
                    col = c2.push(container(self.call_pill(t, calls[ci])).padding(Padding { top: if day { 0.0 } else { 10.0 }, ..Default::default() }));
                    ci += 1;
                    prev_kind_call = true;
                }
                let (c2, day) = push_day(col, m.at, &mut last_day);
                col = c2;
                let mut lead_gap = if day { 0.0 } else { 10.0 };
                if divider.as_deref() == Some(m.id.as_str()) {
                    let n = n_unread;
                    let rule_ = || container(Space::new()).height(1).width(Fill).style(ui::plain(ui::alpha(t.accent_ink, 0.35), 0.0));
                    col = col.push(container(row![rule_(), bold(crate::i18n::tr_count("chat_new_messages", n as u64, &[]), 11.0, t.accent_ink), rule_()]
                        .spacing(10).align_y(Alignment::Center)).padding(Padding { top: 12.0, bottom: 6.0, ..Default::default() }));
                    lead_gap = 4.0;
                }
                let prev = i > 0 && !prev_kind_call && join(&msgs[i - 1], m) && !day;
                let next_is_call = ci < calls.len() && msgs.get(i + 1).is_some_and(|n| calls[ci].started_at * 1000 <= n.at);
                let next = !next_is_call && msgs.get(i + 1).is_some_and(|n| join(m, n) && day_label(n.at) == day_label(m.at));
                let pos = geom::Pos::of(prev, next);
                let gap = if prev { 2.0 } else { lead_gap };
                let hit = (!terms.is_empty()).then(|| found.as_deref() == Some(m.id.as_str()));
                col = col.push(container(self.message_row(t, m, name, pos, max_bub, hit)).padding(Padding { top: gap, ..Default::default() }));
                prev_kind_call = false;
            }
            while ci < calls.len() {
                let (c2, day) = push_day(col, calls[ci].started_at * 1000, &mut last_day);
                col = c2.push(container(self.call_pill(t, calls[ci])).padding(Padding { top: if day { 0.0 } else { 10.0 }, ..Default::default() }));
                ci += 1;
            }
            col = col.push(container(self.block_note(t, &peer, name)).padding(Padding { top: 6.0, ..Default::default() }));
            // While the strip shows, the list gains its height at the top, so the oldest row can still come fully into view.
            let strip_h = if self.strip_text().is_some() { 33.0 } else { 0.0 };
            let list = scroll(t, container(col).padding(Padding { top: 6.0 + strip_h, right: 16.0, bottom: 19.0, left: 16.0 }))
                .id(super::super::chat::LIST_ID).anchor_bottom().width(Fill).height(Fill);
            container(list).width(Fill).height(Fill).style(ui::plain(t.canvas, 0.0)).into()
        });
        self.with_strip(t, list.into())
    }

    /// FindBar.svelte: field (44 tall, radius 22, surface-2, accent ring when focused) with "n/m" (newest = 1),
    /// then older, newer and close. Enter = older, Shift+Enter = newer, Esc closes.
    fn find_bar<'a>(&'a self, t: Tok) -> Option<El<'a>> {
        let q = self.chat.find.as_ref()?;
        let n = self.find_matches().len();
        let idx = if n == 0 { 0 } else { self.chat.find_cur.min(n - 1) };
        let shown = if n == 0 { 0 } else { n - idx };
        let input = text_input(crate::i18n::tr_str("chat_find_in_this_chat"), q).id(super::super::chat::FIND_ID)
            .on_input(|v| Msg::Chat(Cm::Find(Some(v)))).on_submit(Msg::Chat(Cm::FindStep(true)))
            .padding(Padding { top: 11.5, right: 78.0, bottom: 11.5, left: 44.0 }).size(15).line_height(lh(21.0))
            .style(move |_, status| {
                let focused = matches!(status, iced::widget::text_input::Status::Focused { .. });
                iced::widget::text_input::Style {
                    background: t.surface2.into(),
                    border: Border { color: if focused { t.accent } else { Color::TRANSPARENT }, width: if focused { 2.0 } else { 0.0 }, radius: 22.0.into() },
                    icon: t.ink2, placeholder: t.ink2, value: t.ink, selection: ui::alpha(t.accent, 0.3),
                }
            });
        let field = stack![input,
            container(crate::pg_icons::pgi("search", 18.0, t.ink2)).width(44).height(44).center_x(44).center_y(44),
            container(tx(format!("{shown}/{n}"), 13.0, t.ink2)).width(Fill).height(44).align_right(Fill).center_y(44).padding(Padding { right: 14.0, ..Padding::ZERO })];
        let b = |icon: &'static str, msg: Option<Msg>| -> El<'a> {
            button(container(crate::pg_icons::pgi(icon, 20.0, if msg.is_some() { t.ink } else { ui::alpha(t.ink, 0.4) })).center_x(44).center_y(44))
                .padding(0).width(44).height(44).style(super::shell_view::wash(t, None, msg.is_some().then_some(t.surface2), 22.0)).on_press_maybe(msg).into()
        };
        Some(container(row![container(field).width(Fill), b("chevup", (n > 0).then_some(Msg::Chat(Cm::FindStep(true)))),
                b("chev", (n > 0).then_some(Msg::Chat(Cm::FindStep(false)))), b("x", Some(Msg::Chat(Cm::Find(None))))].spacing(2).align_y(Alignment::Center))
            .padding(8).height(60).width(Fill).style(ui::plain(t.surface, 0.0)).into())
    }

    /// Only my own offline state: theirs is unknown unless they're active with me (consent-based
    /// presence, 2026-10-17), so no "they're offline" strip.
    fn strip_text(&self) -> Option<String> {
        (self.status.started && !self.status.online).then(|| crate::i18n::tr_str("chat_youre_offline_messages_send").to_string())
    }

    /// The strip over the top of the list: I'm offline. It overlays, so nothing moves.
    fn with_strip<'a>(&'a self, t: Tok, body: El<'a>) -> El<'a> {
        let Some(text) = self.strip_text() else { return body };
        let strip = container(row![crate::pg_icons::pgi("off", 16.0, t.ink2), tx(text, 13.0, t.ink2).line_height(lh(18.0))].spacing(8).align_y(Alignment::Center))
            .padding([7, 16]).width(Fill).style(move |_: &Theme| container::Style {
                background: Some(t.surface2.into()), border: Border { color: t.hairline, width: 0.0, radius: 0.0.into() }, ..Default::default() });
        stack![body, column![strip, container(Space::new()).height(1).width(Fill).style(ui::plain(t.hairline, 0.0))]].into()
    }

    /// MessageRow `.callrec`: a one-line pill on the caller's side; tap = call back.
    fn call_pill<'a>(&'a self, t: Tok, r: &p2pcore::CallRecord) -> El<'a> {
        let answered = r.duration_secs > 0;
        let d = r.duration_secs;
        let txt = if answered { crate::i18n::tr_args(if r.video { "chat_call_video_length" } else { "chat_call_voice_length" }, &[("duration", &format!("{}:{:02}", d / 60, d % 60))]) }
            else if r.incoming { crate::i18n::tr_str(match (r.reason == "declined_local", r.video) { (true, true) => "chat_call_declined_video", (true, false) => "chat_call_declined_voice", (false, true) => "chat_call_missed_video", (false, false) => "chat_call_missed_voice" }).to_string() }
            else { match r.reason.as_str() { "no_answer" => crate::i18n::tr_str("common_no_answer"), "declined" => crate::i18n::tr_str("common_declined"), "cancelled" | "hangup_local" => crate::i18n::tr_str("chat_cancelled_call"), _ => crate::i18n::tr_str("chat_call_failed") }.into() };
        let bad = !answered && r.incoming && r.reason != "declined_local";
        let ink = if bad { t.danger } else { t.ink };
        let pill = button(row![crate::pg_icons::pgi(if r.video { "video" } else { "phone" }, 16.0, if bad { t.danger } else { t.ink2 }), semi(txt, 13.0, ink).line_height(lh(18.0)),
                tx(hhmm(r.started_at * 1000), 11.0, t.ink2).line_height(lh(18.0))].spacing(6).height(Fill).align_y(Alignment::Center))
            .padding([0, 12]).height(32)
            .style(move |_: &Theme, st| button::Style {
                background: Some(if matches!(st, button::Status::Hovered) { t.surface2 } else { t.surface }.into()),
                border: Border { color: t.hairline, width: 1.0, radius: 16.0.into() }, text_color: t.ink, ..Default::default() })
            .on_press_maybe(self.call.is_none().then(|| Msg::CallPressed(r.peer_did.clone())));
        if r.incoming { row![pill, Space::new().width(Fill)].into() } else { row![Space::new().width(Fill), pill].into() }
    }

    fn history_note<'a>(&'a self, t: Tok, name: &str) -> El<'a> {
        let busy = self.chat.fetching;
        let (msg, btn): (String, Option<&str>) = if self.chat.older.is_some() {
            (String::new(), Some(crate::i18n::tr_str("chat_load_earlier_messages")))
        } else if self.chat.no_more {
            (crate::i18n::tr_args("chat_that_is_everything_has", &[("name", &(name).to_string())]), None)
        } else {
            (
                crate::i18n::tr_args("chat_older_messages_are_on", &[("name", &(name).to_string())]),
                Some(crate::i18n::tr_str("chat_ask_for_earlier_messages")),
            )
        };
        let mut c = column![].spacing(8).align_x(Alignment::Center);
        if !msg.is_empty() {
            c = c.push(tx(msg, 13.0, t.ink2));
        }
        if let Some(b) = btn {
            c = c.push(pill(t, Kind::Quiet, None, if busy { crate::i18n::tr_str("common_loading") } else { b }, (!busy).then_some(Msg::Chat(Cm::Older))));
        }
        container(c).center_x(Fill).padding([4, 40]).into()
    }

    /// MessageRow.svelte: the bubble (padding 7 12 inside a 1 px edge, 15/21, Bézier corners by position), the
    /// time inline at the end of the last line when it fits, a reply quote with an accent bar, and the 28 px
    /// chevron that opens the menu (its room is kept while hidden, so hovering never rewraps). `hit` = find match
    /// (Some(true) for the current one).
    fn message_row<'a>(&'a self, t: Tok, m: &'a ChatMsg, name: &str, pos: geom::Pos, max_bub: f32, hit: Option<bool>) -> El<'a> {
        use crate::fit::width;
        let (bg, fg) = if m.outgoing { (t.primary_c, t.on_primary_c) } else { (t.bub_in, t.ink) };
        let metac = if m.outgoing { t.bub_meta_out } else { t.bub_meta_in };
        // A photo (Framed, 2026-10-14): the bubble is the photo plus a 3 px rim; caption and time keep to its width.
        let pbox = if m.deleted { None } else { self.photo_box(m) };
        let inner_max = match pbox { Some((bw, _)) => bw - 18.0, None => (max_bub - 26.0).max(80.0) };
        let first = present::names::first_word(name).to_string();
        let side = if m.outgoing { geom::Side::Right } else { geom::Side::Left };
        let radii = geom::bubble_radii(pos, side, geom::R_BUB, geom::R_BUB_TIGHT);
        // .meta: 11px, gap 3: "edited", the time, then my tick (check, or a clock while it waits).
        let mut meta = row![].spacing(3).align_y(Alignment::Center);
        let mut meta_w = 0.0;
        if m.edited_at.is_some() && !m.deleted {
            meta = meta.push(tx("edited", 11.0, metac).line_height(lh(11.0)));
            meta_w += width("edited", ui::SANS, 11.0) + 3.0;
        }
        meta = meta.push(tx(hhmm(m.at), 11.0, metac).line_height(lh(11.0)));
        meta_w += width(&hhmm(m.at), ui::SANS, 11.0);
        if m.outgoing && !m.deleted {
            // The receipt tick (ticks.rs): the mark's string, slack while waiting, pulled taut into one tick
            // when it left, two when their device signed delivery, lavender when it signed read, red slack when
            // not delivered. State from the core's `ReceiptTick`; the slot is always reserved (no layout shift).
            let tw = 12.0 * crate::tin_ticks::VB_W / crate::tin_ticks::VB_H;
            let tick: El = match self.tick_vis_of(m) {
                Some(v) => {
                    let inks = crate::ticks::Inks { meta: metac, read: t.tick_read, bad: t.tick_bad, bg };
                    crate::ticks::glyph(v, self.chat.tick_anims.get(&m.id), inks, 12.0, self.fx.now)
                }
                None => Space::new().width(tw).height(12).into(),
            };
            meta = meta.push(container(tick).width(tw).height(12).center_x(tw).center_y(12));
            meta_w += tw + 3.0;
        }
        let mut body = column![].spacing(0).width(Length::Shrink);
        let text_el: Option<El> = if m.deleted {
            Some(text(if m.outgoing { crate::i18n::tr_str("chat_you_deleted_this_message").to_string() } else { crate::i18n::tr_args("chat_deleted_this_message", &[("name", &(first).to_string())]) })
                .size(15).line_height(lh(21.0)).color(t.ink2).font(iced::Font { style: iced::font::Style::Italic, ..ui::SANS }).into())
        } else {
            if let Some(rid) = &m.reply_to {
                let (who, snippet) = match self.chat.find(rid) {
                    Some(r) => (if r.outgoing { crate::i18n::tr_str("common_you").to_string() } else { first.clone() }, preview_of(r)),
                    None => (String::new(), crate::i18n::tr_str("common_original_message_unavailable").to_string()),
                };
                let mut q = column![].spacing(0);
                if !who.is_empty() {
                    q = q.push(bold(who, 12.0, t.accent_ink).line_height(lh(18.0)));
                }
                q = q.push(tx(crate::fit::ellipsize(&snippet, ui::SANS, 13.0, inner_max - 26.0), 13.0, t.ink2).line_height(lh(18.0)));
                let quote = button(row![container(Space::new()).width(3).height(Fill).style(ui::plain(t.accent, 0.0)),
                        container(q).padding(Padding { top: 5.0, right: 10.0, bottom: 6.0, left: 7.0 })].height(Length::Shrink))
                    .padding(0).width(Fill)
                    .style(move |_: &Theme, _| button::Style { background: Some(t.surface2.into()), border: Border { radius: 12.0.into(), ..Default::default() }, ..Default::default() })
                    .on_press(Msg::Chat(Cm::Jump(rid.clone())));
                body = body.push(container(quote).padding(Padding { top: 2.0, bottom: 6.0, ..Default::default() }).width(Length::Fixed(inner_max.min(320.0))));
            }
            if let Some(a) = &m.attachment {
                body = body.push(match pbox {
                    Some(b) => self.image_view(t, m, a, b, radii.map(|r| (r - 3.0).max(3.0)), fg),
                    None => self.attachment_view(t, m, a, fg, metac),
                });
            }
            (!m.text.is_empty()).then(|| message_text(&m.text, fg))
        };
        // Place the time like the CSS float: same line when the last line has room, else under it.
        let line_text = if m.deleted { if m.outgoing { crate::i18n::tr_str("chat_you_deleted_this_message").to_string() } else { crate::i18n::tr_args("chat_deleted_this_message", &[("name", &(first).to_string())]) } } else { m.text.clone() };
        match (text_el, pbox) {
            (Some(te), Some((bw, _))) => {
                body = body.push(container(te).width(bw).padding(Padding { top: 5.0, right: 9.0, bottom: 0.0, left: 9.0 }));
                body = body.push(container(meta).width(bw).align_right(bw).padding(Padding { top: 6.0, right: 7.0, ..Padding::ZERO }));
            }
            (None, Some((bw, _))) => body = body.push(container(meta).width(bw).align_right(bw).padding(Padding { top: 6.0, right: 7.0, ..Padding::ZERO })),
            (Some(te), None) => {
                let (lines, last) = wrap_last(&line_text, inner_max);
                let fits = last + 10.0 + meta_w <= inner_max;
                body = body.push(if fits && lines == 1 {
                    El::from(row![te, column![Space::new().height(10), meta].padding(Padding { left: 10.0, ..Padding::ZERO })].align_y(Alignment::Start))
                } else if fits {
                    stack![container(te).width(inner_max), container(meta).align_right(Fill).align_bottom(Fill).padding(Padding { bottom: 0.0, ..Padding::ZERO })].into()
                } else {
                    column![te, container(meta).align_right(Fill).padding(Padding { top: 0.0, ..Padding::ZERO })].spacing(0).width(Length::Shrink).into()
                });
            }
            (None, None) => body = body.push(container(meta).align_right(Fill).padding(Padding { top: 6.0, ..Padding::ZERO })),
        }
        let pad = if pbox.is_some() { Padding { top: 3.0, right: 3.0, bottom: 6.0, left: 3.0 } } else { Padding::from([8, 13]) };
        let mut bubble = shaped(container(body).width(Length::Shrink).padding(pad).max_width(max_bub), radii, if m.deleted { Color::TRANSPARENT } else { bg });
        if m.deleted {
            bubble = bubble.stroke(t.hairline, 1.0);
        } else if hit == Some(true) || self.chat.flash.as_ref().is_some_and(|(id, at)| *id == m.id && at.elapsed().as_millis() < 1200) {
            bubble = bubble.stroke(t.accent, 2.0);
        } else if hit == Some(false) {
            bubble = bubble.stroke(ui::alpha(t.accent, 0.5), 1.0);
        } else if !m.outgoing {
            bubble = bubble.stroke(t.bub_in_line, 1.0);
        }
        if self.blocks.not_delivered(&m.peer_did, &m.id) && m.outgoing {
            // .fail: under my message, the not-delivered ink (the tick's), "Not delivered".
            let note = container(tx(present::block::not_delivered_text(name.to_string()), 12.0, t.tick_bad).line_height(lh(16.0))).align_right(Fill).padding(Padding { top: 3.0, right: 2.0, ..Padding::ZERO });
            return self.row_with_menu(t, m, name, bubble.into(), Some(note.into()));
        }
        self.row_with_menu(t, m, name, bubble.into(), None)
    }

    /// The bubble beside its hover chevron (28 round, muted; shown on hover or while its menu is open), and the menu.
    fn row_with_menu<'a>(&'a self, t: Tok, m: &'a ChatMsg, name: &str, bubble: El<'a>, note: Option<El<'a>>) -> El<'a> {
        let open = self.chat.menu.as_ref() == Some(&m.id);
        let hot = open || self.chat.hover.as_ref() == Some(&m.id);
        let more: El = if hot && !m.deleted {
            button(container(crate::pg_icons::pgi("chev", 20.0, if open { t.ink } else { t.ink2 })).center_x(28).center_y(28))
                .padding(0).width(28).height(28)
                .style(move |_: &Theme, st| button::Style {
                    background: (open || matches!(st, button::Status::Hovered)).then_some(t.surface.into()),
                    border: Border { color: if open || matches!(st, button::Status::Hovered) { t.hairline } else { Color::TRANSPARENT }, width: 1.0, radius: 14.0.into() },
                    ..Default::default() })
                .on_press(Msg::Chat(Cm::Menu(m.id.clone()))).into()
        } else { Space::new().width(28).height(28).into() };
        let line = if m.outgoing { row![Space::new().width(Fill), more, bubble] } else { row![bubble, more, Space::new().width(Fill)] }.spacing(4).align_y(Alignment::Center);
        let mut col = column![line];
        if let Some(n) = note { col = col.push(n); }
        if open && !m.deleted {
            let menu = self.message_menu(t, m, name);
            col = col.push(container(if m.outgoing { container(menu).align_right(Fill) } else { container(menu) }).padding(Padding { top: 4.0, ..Padding::ZERO }));
        }
        mouse_area(col)
            .on_enter(Msg::Chat(Cm::Hover(Some(m.id.clone()))))
            .on_exit(Msg::Chat(Cm::Hover(None)))
            .on_right_press(Msg::Chat(Cm::Menu(m.id.clone())))
            .into()
    }

    /// Conversation.svelte `.menu` on desktop: a pop card, rows 44 tall, 14px, icon 20 muted, Delete in danger.
    fn message_menu<'a>(&'a self, t: Tok, m: &'a ChatMsg, _name: &str) -> El<'a> {
        let item = |icon: &'static str, label_: &'static str, danger: bool, msg: Msg| {
            let c = if danger { t.danger } else { t.ink };
            // The row's content centred in its 44 px (a button lays out from the top otherwise, which left the
            // items sitting high with a gap under the last one).
            button(container(row![crate::pg_icons::pgi(icon, 20.0, if danger { t.danger } else { t.ink2 }), tx(label_, 14.0, c)].spacing(12).align_y(Alignment::Center)).height(Fill).center_y(Fill))
                .padding([0, 14]).height(44).width(Fill)
                .style(super::shell_view::wash(t, None, Some(t.surface2), 0.0))
                .on_press(msg)
        };
        let mut c = column![];
        if let Some(lines) = self.receipt_lines(m) {
            let mut info = column![].spacing(2);
            for (i, l) in lines.into_iter().enumerate() {
                info = info.push(tx(l, if i == 0 { 13.0 } else { 12.0 }, if i == 0 { t.ink } else { t.ink2 }).line_height(lh(18.0)).wrapping(iced::widget::text::Wrapping::None));
            }
            c = c.push(container(info).padding(Padding { top: 6.0, right: 14.0, bottom: 8.0, left: 14.0 }).width(Fill));
            c = c.push(container(Space::new().height(1)).width(Fill).style(ui::plain(t.hairline, 0.0)));
        }
        c = c.push(item("reply", crate::i18n::tr_str("chat_reply"), false, Msg::Chat(Cm::Reply(m.id.clone()))));
        if !m.text.is_empty() {
            c = c.push(item("copy", crate::i18n::tr_str("chat_copy"), false, Msg::Chat(Cm::Copy(preview_of(m)))));
        }
        if m.outgoing && !m.text.is_empty() {
            c = c.push(item("edit", crate::i18n::tr_str("chat_edit"), false, Msg::Chat(Cm::StartEdit(m.id.clone()))));
        }
        if m.outgoing {
            c = c.push(item("trash", crate::i18n::tr_str("chat_delete"), true, Msg::Chat(Cm::Delete(m.id.clone()))));
        }
        ui::raised(container(c).padding([6, 0]).width(220), t, ui::Depth::Pop, ui::pop_surface(t, 14.0))
    }

    /// My message's status for its menu: a headline, then one line per signed statement (who signed,
    /// which of their devices, what it says). Only the core's verified receipts count; a statement is a
    /// device's signed claim, so the copy says "on their device", never "they saw it".
    fn receipt_lines(&self, m: &ChatMsg) -> Option<Vec<String>> {
        if !m.outgoing || m.deleted || self.is_me(&m.peer_did) {
            return None;
        }
        let st = self.chat.receipts.get(&m.id)?;
        let first = present::names::first_word(&self.chat_name(&m.peer_did)).to_string();
        let head = match st.tick {
            ReceiptTick::Read => crate::i18n::tr_args("chat_read_on_s_device", &[("name", &(first).to_string())]),
            ReceiptTick::Delivered => crate::i18n::tr_args("chat_on_s_device", &[("name", &(first).to_string())]),
            ReceiptTick::Waiting => crate::i18n::tr_args("chat_waiting_for_s_device", &[("name", &(first).to_string())]),
            ReceiptTick::OnlyHere => crate::i18n::tr_str("chat_only_on_this_computer").to_string(),
            ReceiptTick::None => return None,
        };
        let mut out = vec![head];
        for r in &st.recipients {
            if r.proofs.is_empty() {
                continue;
            }
            out.push(crate::i18n::tr_args("chat_signed_by_2", &[("value", &(present::names::first_word(&self.chat_name(&r.did))).to_string())]));
            // One line per signing device: which of their devices said what.
            let mut devices: Vec<(String, Vec<&str>)> = Vec::new();
            for p in &r.proofs {
                let dev: String = p.signer_device.chars().take(8).collect();
                let what = match p.kind { ReceiptProofKind::Delivered => "delivered", ReceiptProofKind::Read => "read" };
                match devices.iter_mut().find(|(d, _)| *d == dev) {
                    Some((_, w)) if !w.contains(&what) => w.push(what),
                    Some(_) => {}
                    None => devices.push((dev, vec![what])),
                }
            }
            for (dev, what) in devices {
                out.push(crate::i18n::tr_args("chat_device", &[("device", &(dev).to_string()),("value", &(what.join(", ")).to_string())]));
            }
        }
        Some(out)
    }

    fn attachment_view<'a>(&'a self, t: Tok, m: &'a ChatMsg, a: &'a p2pcore::Attachment, fg: Color, dim: Color) -> El<'a> {
        let voice = a.kind == AttachmentKind::Voice;
        let class = if voice { Class::SaveOnly } else { media::classify(&a.name, &a.mime) };
        let (done, total) = self.chat.progress.get(&a.hash).copied().unwrap_or((a.transferred, a.size));
        let who = self.chat_name(&m.peer_did);
        let status: String = match a.state {
            TransferState::Ready if voice => String::new(),
            TransferState::Ready if m.outgoing && m.delivery == DeliveryState::Pending && !self.status.online => {
                crate::i18n::tr_args("chat_gets_it_when_youre", &[("value", &(size_text(a.size)).to_string()),("name", &(who).to_string())])
            }
            TransferState::Ready => {
                let k = kind_label(&a.name);
                if k.is_empty() { size_text(a.size) } else { format!("{} \u{b7} {k}", size_text(a.size)) }
            }
            TransferState::Remote if !self.status.online => crate::i18n::tr_args("chat_not_downloaded", &[("value", &(size_text(a.size)).to_string())]),
            TransferState::Remote => crate::i18n::tr_args("chat_not_downloaded", &[("value", &(size_text(a.size)).to_string())]),
            TransferState::Downloading if !self.status.online => {
                crate::i18n::tr_args("chat_paused_waiting_for_to", &[("value", &(size_text(a.size)).to_string()),("name", &(who).to_string())])
            }
            TransferState::Downloading => crate::i18n::tr_args("chat_downloading_of", &[("value", &(size_text(done)).to_string()),("value2", &(size_text(total.max(a.size))).to_string())]),
            TransferState::Failed => crate::i18n::tr_args("chat_couldnt_download_tap_to", &[("value", &(size_text(a.size)).to_string())]),
        };
        let action: Option<El> = match a.state {
            TransferState::Remote => Some(self.chip_btn(Icon::Download, crate::i18n::tr_str("chat_download"), fg, Msg::Chat(Cm::Download(m.id.clone())))),
            TransferState::Failed => Some(self.chip_btn(Icon::RotateCw, crate::i18n::tr_str("chat_tap_to_retry"), fg, Msg::Chat(Cm::Download(m.id.clone())))),
            TransferState::Ready => {
                let mut r = row![].spacing(6);
                if !voice && class.openable() {
                    r = r.push(self.chip_btn(Icon::ExternalLink, crate::i18n::tr_str("common_open"), fg, Msg::Chat(Cm::Open(m.id.clone()))));
                }
                Some(r.push(self.chip_btn(Icon::Download, crate::i18n::tr_str("common_save"), fg, Msg::Chat(Cm::Save(m.id.clone())))).into())
            }
            TransferState::Downloading => Some(self.chip_btn(Icon::X, crate::i18n::tr_str("common_cancel"), fg, Msg::Chat(Cm::Cancel(m.id.clone())))),
        };
        let mut c = column![].spacing(6);
        if voice {
            let (playing, pos) = match &self.chat.player {
                Some((pid, p)) if *pid == m.id => (!p.is_paused() && !p.finished(), Some(p.position_ms())),
                _ => (false, None),
            };
            let v = VoiceView { waveform: &a.waveform, duration_ms: a.duration_ms, position_ms: pos, playing };
            let (tid, sid) = (m.id.clone(), m.id.clone());
            c = c.push(voice::bubble(t, v, m.outgoing, Msg::Chat(Cm::VoiceToggle(tid)), move |f| Msg::Chat(Cm::VoiceSeek(sid.clone(), f))));
            if !status.is_empty() {
                c = c.push(tx(status, 12.0, dim));
            }
        } else {
            c = c.push(
                row![
                    container(ui::icon(Icon::File, 22.0, fg)).padding(8).style(ui::plain(ui::alpha(fg, 0.12), 10.0)),
                    column![semi(a.name.clone(), 14.0, fg), tx(status, 12.0, dim)].spacing(1).width(Length::Shrink),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            );
        }
        if a.state == TransferState::Downloading && self.status.online {
            let v = if total > 0 { (done as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
            c = c.push(
                progress_bar(0.0..=1.0, v).girth(5).style(move |_: &Theme| progress_bar::Style {
                    background: iced::Background::Color(ui::alpha(fg, 0.15)),
                    bar: iced::Background::Color(t.primary),
                    border: Border { radius: 3.0.into(), ..Default::default() },
                }),
            );
        }
        if let Some(a) = action {
            c = c.push(a);
        }
        c.into()
    }

    /// The box a photo message's picture gets, if it is one shown as a photo (not a picture that failed to decode):
    /// its true shape from the size the core sends (chat/5), else the decoded thumbnail's, else 4:3; at most 300 x 340,
    /// at least 120 wide.
    fn photo_box(&self, m: &ChatMsg) -> Option<(f32, f32)> {
        let a = m.attachment.as_ref()?;
        if a.kind == AttachmentKind::Voice || !matches!(media::classify(&a.name, &a.mime), Class::Image(_)) {
            return None;
        }
        let thumb = self.chat.thumbs.get(&a.hash);
        if matches!(thumb, Some(Thumb::Bad)) {
            return None;
        }
        let (w, h) = match thumb {
            _ if a.width > 0 && a.height > 0 => (a.width as f32, a.height as f32),
            Some(Thumb::Ready { w, h, .. }) => (*w as f32, *h as f32),
            _ => (4.0, 3.0),
        };
        let s = (300.0 / w).min(340.0 / h);
        Some(((w * s).max(120.0).round(), (h * s).round()))
    }

    /// PhotoBody.svelte (Framed, decided 2026-10-14): the photo, its corners [inner] (the bubble's, 3 px in, so they
    /// tighten in a run). Until it is here, the core's 4x3 colour preview fills the box, smoothed; not downloaded: its
    /// size and Download; downloading: the share and Cancel; mine waiting: a chip. Click opens it.
    fn image_view<'a>(&'a self, t: Tok, m: &'a ChatMsg, a: &'a p2pcore::Attachment, (bw, bh): (f32, f32), inner: geom::Radii, fg: Color) -> El<'a> {
        let r = iced::border::Radius { top_left: inner[0], top_right: inner[1], bottom_right: inner[2], bottom_left: inner[3] };
        let mut st = stack![container(Space::new()).width(bw).height(bh)
            .style(move |_: &Theme| iced::widget::container::Style { background: Some(ui::alpha(fg, 0.08).into()), border: Border { radius: r, ..Default::default() }, ..Default::default() })];
        if a.preview.len() == 36 {
            let rgba: Vec<u8> = a.preview.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
            st = st.push(iced::widget::image(iced::widget::image::Handle::from_rgba(4, 3, rgba)).width(bw).height(bh)
                .content_fit(iced::ContentFit::Fill).filter_method(iced::widget::image::FilterMethod::Linear).border_radius(r));
        }
        if let Some(Thumb::Ready { handle, .. }) = self.chat.thumbs.get(&a.hash) {
            st = st.push(button(iced::widget::image(handle.clone()).width(bw).height(bh).content_fit(iced::ContentFit::Cover).border_radius(r))
                .padding(0).style(|_: &Theme, _| button::Style::default()).on_press(Msg::Chat(Cm::View(m.id.clone()))));
        }
        let chip = |s: String| -> El<'a> {
            container(tx(s, 11.0, Color::WHITE).line_height(lh(15.0))).padding([2, 8]).style(ui::plain(Color { a: 0.42, ..Color::BLACK }, 10.0)).into()
        };
        let pill = |label_: &'static str, msg: Msg| -> El<'a> {
            button(container(semi(label_, 13.0, t.on_accent)).center_y(32)).padding([0, 16]).height(32)
                .style(move |_: &Theme, _| button::Style { background: Some(t.accent.into()), border: Border { radius: 16.0.into(), ..Default::default() }, ..Default::default() })
                .on_press(msg).into()
        };
        let mid: Option<El> = match a.state {
            TransferState::Remote if !m.outgoing => Some(column![chip(crate::i18n::tr_args("common_photo", &[("value", &(size_text(a.size)).to_string())])), pill(crate::i18n::tr_str("chat_download"), Msg::Chat(Cm::Download(m.id.clone())))].spacing(8).align_x(Alignment::Center).into()),
            TransferState::Downloading => {
                let (done, total) = self.chat.progress.get(&a.hash).copied().unwrap_or((a.transferred, a.size));
                let pct = if total > 0 { done * 100 / total } else { 0 };
                Some(column![chip(format!("{pct}%")), pill(crate::i18n::tr_str("common_cancel"), Msg::Chat(Cm::Cancel(m.id.clone())))].spacing(8).align_x(Alignment::Center).into())
            }
            TransferState::Failed => Some(pill(crate::i18n::tr_str("common_try_again"), Msg::Chat(Cm::Download(m.id.clone())))),
            _ => None,
        };
        if let Some(e) = mid {
            st = st.push(container(e).center_x(bw).center_y(bh));
        }
        if m.outgoing && m.delivery == DeliveryState::Pending && !self.chat.online.contains(&m.peer_did) {
            let first = present::names::first_word(&self.chat_name(&m.peer_did)).to_string();
            st = st.push(container(chip(crate::i18n::tr_args("chat_sends_when_is_online", &[("name", &(first).to_string())]))).width(bw).height(bh).align_bottom(bh).padding(8));
        }
        st.width(bw).height(bh).into()
    }

    /// The full-window overlay: a picture, or a text file, over a dark scrim.
    /// Conversation.svelte "Delete message?": the playground's dialog. The core deletes for both sides only, so
    /// there is no "Delete for me" here (a core gap, noted in SCREENS-PORT-STATUS.md).
    pub(super) fn delete_overlay<'a>(&'a self, t: Tok) -> Option<El<'a>> {
        let id = self.chat.confirm_delete.clone()?;
        let peer = self.chat.peer.as_deref()?;
        let first = present::names::first_word(&self.chat_name(peer)).to_string();
        let delivered = self.chat.find(&id).is_some_and(|m| m.delivery == DeliveryState::Delivered);
        let body = if !delivered { crate::i18n::tr_str("chat_it_was_never_delivered").to_string() }
            else if self.chat.online.contains(peer) && self.status.online { crate::i18n::tr_args("chat_delete_for_everyone_asks", &[("name", &(first).to_string())]) }
            else { crate::i18n::tr_args("chat_is_offline_delete_for", &[("name", &(first).to_string())]) };
        let btn_ = |label_: &'static str, danger: bool, msg: Msg| -> El<'a> {
            button(container(semi(label_, 15.0, if danger { t.on_danger_btn } else { t.ink })).center_y(44)).padding([0, 18]).height(44)
                .style(move |_: &Theme, st| button::Style {
                    background: Some(if danger { t.danger_btn } else if matches!(st, button::Status::Hovered) { t.surface2 } else { Color::TRANSPARENT }.into()),
                    border: Border { radius: 22.0.into(), ..Default::default() }, ..Default::default() })
                .on_press(msg).into()
        };
        let card = column![
            semi(crate::i18n::tr_str("chat_delete_message"), 17.0, t.ink).line_height(lh(24.0)),
            Space::new().height(10),
            tx(body, 13.0, t.ink2).line_height(lh(19.0)),
            Space::new().height(18),
            row![Space::new().width(Fill), btn_(crate::i18n::tr_str("common_cancel"), false, Msg::Chat(Cm::DeleteCancel)), btn_(crate::i18n::tr_str("chat_delete_for_everyone"), true, Msg::Chat(Cm::DeleteGo(id)))].spacing(8),
        ];
        Some(super::shell_view::pg_dialog(t, 1.0, None, container(card).padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 }).into(), Msg::Chat(Cm::DeleteCancel)))
    }

    pub(super) fn viewer_overlay<'a>(&'a self, t: Tok) -> Option<El<'a>> {
        let v = self.chat.viewer.as_ref()?;
        let white = Color::WHITE;
        let ids = self.chat.viewable_images();
        let pos = ids.iter().position(|x| *x == v.id);
        let is_img = !matches!(v.body, ViewBody::Text(_)) && pos.is_some() || matches!(v.body, ViewBody::Image { .. });
        let mut bar = row![semi(v.name.clone(), 15.0, white).width(Fill).wrapping(iced::widget::text::Wrapping::None)]
            .spacing(8)
            .align_y(Alignment::Center);
        if let Some(i) = pos.filter(|_| ids.len() > 1) {
            bar = bar.push(tx(crate::i18n::tr_args("chat_of", &[("value", &(i + 1).to_string()),("len", &(ids.len()).to_string())]), 13.0, ui::alpha(white, 0.7)));
        }
        bar = bar
            .push(self.chip_btn(Icon::ExternalLink, crate::i18n::tr_str("chat_open_in_default_app"), white, Msg::Chat(Cm::OpenExt(v.id.clone()))))
            .push(self.chip_btn(Icon::Download, crate::i18n::tr_str("common_save"), white, Msg::Chat(Cm::Save(v.id.clone()))))
            .push(
                button(container(ui::icon(Icon::X, 20.0, white)).center_x(36).center_y(36))
                    .padding(0)
                    .style(move |_: &Theme, st| button::Style {
                        background: Some(ui::alpha(white, if matches!(st, button::Status::Hovered | button::Status::Pressed) { 0.22 } else { 0.1 }).into()),
                        border: Border { radius: 999.0.into(), ..Default::default() },
                        ..Default::default()
                    })
                    .on_press(Msg::Chat(Cm::CloseView)),
            );
        let body: El = match &v.body {
            ViewBody::Image { handle, .. } => iced::widget::image(handle.clone())
                .content_fit(iced::ContentFit::Contain)
                .width(Fill)
                .height(Fill)
                .border_radius(6.0)
                .into(),
            ViewBody::Text(txt) => container(scroll(t, container(text(txt.clone()).font(ui::MONO).size(13).color(t.ink)).padding(16)).width(Fill))
                .max_width(860)
                .height(if txt.lines().count() > 30 { Length::Fill } else { Length::Shrink })
                .style(ui::plain(t.surface, 12.0))
                .into(),
            ViewBody::Loading => container(tx(crate::i18n::tr_str("common_loading"), 15.0, ui::alpha(white, 0.8))).center_x(Fill).center_y(Fill).into(),
            ViewBody::Failed(e) => container(tx(crate::i18n::tr_args("chat_could_not_show_this", &[("error", &(e).to_string())]), 15.0, ui::alpha(white, 0.8))).center_x(Fill).center_y(Fill).into(),
        };
        let nav = |i: Icon, d: i32, on: bool| -> El<'a> {
            let b = button(container(ui::icon(i, 26.0, white)).center_x(44).center_y(44))
                .padding(0)
                .style(move |_: &Theme, st| button::Style {
                    background: Some(ui::alpha(white, if matches!(st, button::Status::Hovered | button::Status::Pressed) { 0.22 } else { 0.1 }).into()),
                    border: Border { radius: 999.0.into(), ..Default::default() },
                    ..Default::default()
                });
            container(if on { b.on_press(Msg::Chat(Cm::Step(d))) } else { b }).center_y(Fill).width(52).into()
        };
        let prev_ok = is_img && pos.is_some_and(|i| i > 0);
        let next_ok = is_img && pos.is_some_and(|i| i + 1 < ids.len());
        let mid: El = if is_img {
            row![
                if prev_ok { nav(Icon::ChevronLeft, -1, true) } else { Space::new().width(52).into() },
                mouse_area(container(body).center_x(Fill).center_y(Fill)).on_press(Msg::Chat(Cm::Nop)),
                if next_ok { nav(Icon::ChevronRight, 1, true) } else { Space::new().width(52).into() },
            ]
            .spacing(8)
            .height(Fill)
            .into()
        } else {
            container(body).center_x(Fill).height(Fill).into()
        };
        let page = column![container(bar).padding([0, 4]), mid].spacing(14).width(Fill).height(Fill);
        Some(
            mouse_area(
                container(page)
                    .padding(20)
                    .width(Fill)
                    .height(Fill)
                    .style(ui::plain(Color { r: 0.04, g: 0.05, b: 0.06, a: 0.94 }, 0.0)),
            )
            .on_press(Msg::Chat(Cm::CloseView))
            .into(),
        )
    }

    fn chip_btn<'a>(&self, i: Icon, label_: &'static str, fg: Color, msg: Msg) -> El<'a> {
        button(row![ui::icon(i, 14.0, fg), semi(label_, 13.0, fg)].spacing(6).align_y(Alignment::Center))
            .padding([5, 12])
            .style(move |_: &Theme, st| button::Style {
                background: Some(
                    ui::alpha(fg, if matches!(st, button::Status::Hovered | button::Status::Pressed) { 0.2 } else { 0.1 }).into(),
                ),
                text_color: fg,
                border: Border { radius: 999.0.into(), ..Default::default() },
                ..Default::default()
            })
            .on_press(msg)
            .into()
    }

    /// Composer.svelte: attach (44, muted) · the field (surface, hairline, radius 22, accent edge when focused,
    /// 10 16 padding, 15/22, up to 140 tall) · send (44 round accent), in 8 10 10 on the canvas, bottoms aligned.
    /// Reply / edit shows a chip above the field (radius 12, surface-2, 3 px accent bar). Desktop keeps voice
    /// messages: with nothing typed the send slot is the mic (the playground has no voice messages).
    fn composer<'a>(&'a self, t: Tok, name: &str) -> El<'a> {
        let first = present::names::first_word(name).to_string();
        let ctx = |title: String, snippet: String| -> El<'a> {
            let bar = container(Space::new()).width(3).height(Fill).style(ui::plain(t.accent, 0.0));
            let body = column![semi(title, 13.0, t.accent_ink).line_height(lh(18.0)).wrapping(iced::widget::text::Wrapping::None),
                tx(crate::fit::ellipsize(&snippet, ui::SANS, 13.0, 520.0), 13.0, t.ink2).line_height(lh(18.0)).wrapping(iced::widget::text::Wrapping::None)].width(Fill);
            let x = button(container(crate::pg_icons::pgi("x", 18.0, t.ink2)).center_x(44).center_y(44)).padding(0).width(44).height(44)
                .style(super::shell_view::wash(t, None, Some(t.surface3), 22.0)).on_press(Msg::Chat(Cm::CancelCompose));
            container(row![bar, container(row![body, x].spacing(8).align_y(Alignment::Center)).padding(Padding { top: 0.0, right: 2.0, bottom: 0.0, left: 7.0 }).height(44)].height(44))
                .width(Fill).clip(true).style(ui::plain(t.surface2, 12.0)).into()
        };
        let chip: Option<El> = if let Some(m) = self.chat.reply.as_deref().and_then(|id| self.chat.find(id)) {
            Some(ctx(crate::i18n::tr_args("chat_replying_to", &[("value", &(if m.outgoing { "yourself".to_string() } else { first.clone() }).to_string())]), preview_of(m)))
        } else {
            self.chat.editing.as_deref().and_then(|id| self.chat.find(id)).map(|m| ctx(crate::i18n::tr_str("chat_editing_message").to_string(), preview_of(m)))
        };
        let editing = self.chat.editing.is_some();
        let editor = text_editor(&self.chat.input)
            .id(COMPOSER_ID)
            .placeholder(crate::i18n::tr_str("common_message"))
            .on_action(|a| Msg::Chat(Cm::Edit(a)))
            .key_binding(|kp| {
                use iced::keyboard::key::Named;
                use iced::keyboard::Key;
                match kp.key.as_ref() {
                    // Enter sends; Shift+Enter is a new line; Esc cancels a reply or an edit.
                    Key::Named(Named::Enter) if matches!(kp.status, text_editor::Status::Focused { .. }) => Some(
                        if kp.modifiers.shift() { text_editor::Binding::Enter } else { text_editor::Binding::Custom(Msg::Chat(Cm::Send)) },
                    ),
                    Key::Named(Named::Escape) => Some(text_editor::Binding::Custom(Msg::Chat(Cm::CancelCompose))),
                    _ => text_editor::Binding::from_key_press(kp),
                }
            })
            .padding([10, 16])
            .size(15)
            .line_height(lh(22.0))
            .min_height(22)
            .max_height(140)
            .style(move |_: &Theme, st| {
                let focused = matches!(st, text_editor::Status::Focused { .. });
                text_editor::Style {
                    background: iced::Background::Color(t.surface),
                    border: Border { radius: 22.0.into(), width: 1.0, color: if focused { t.accent } else { t.hairline } },
                    placeholder: t.ink2,
                    value: t.ink,
                    selection: ui::alpha(t.accent, 0.3),
                }
            });
        let has_text = !self.chat.input.text().trim().is_empty();
        let last: El = if has_text || editing {
            button(container(crate::pg_icons::pgi(if editing { "check" } else { "send" }, 20.0, t.on_accent)).center_x(44).center_y(44))
                .padding(0).width(44).height(44)
                .style(move |_: &Theme, _| button::Style { background: Some(ui::alpha(t.accent, if has_text { 1.0 } else { 0.38 }).into()), border: Border { radius: 22.0.into(), ..Default::default() }, ..Default::default() })
                .on_press_maybe(has_text.then_some(Msg::Chat(Cm::Send)))
                .into()
        } else {
            voice::mic_button(t, Msg::Chat(Cm::RecStart))
        };
        let attach = button(container(crate::pg_icons::pgi("clip", 24.0, if editing { ui::alpha(t.ink2, 0.38) } else { t.ink2 })).center_x(44).center_y(44))
            .padding(0).width(44).height(44).style(super::shell_view::wash(t, None, (!editing).then_some(t.surface2), 22.0))
            .on_press_maybe((!editing).then_some(Msg::Chat(Cm::Attach)));
        let line: El = match &self.chat.rec {
            Some((r, _)) => voice::record_bar(t, r.elapsed_ms(), Msg::Chat(Cm::RecSend), Msg::Chat(Cm::RecCancel)),
            None => row![attach, container(editor).width(Fill), last].spacing(6).align_y(Alignment::End).into(),
        };
        let mut col = column![];
        if let Some(c) = chip {
            col = col.push(container(c).padding(Padding { left: 50.0, right: 50.0, bottom: 6.0, ..Padding::ZERO }));
        }
        container(col.push(line)).padding(Padding { top: 8.0, right: 10.0, bottom: 10.0, left: 10.0 }).width(Fill).style(ui::plain(t.canvas, 0.0)).into()
    }
}

/// Message text: only a few emojis show big, emojis inside text a little larger (`present::emoji`).
fn message_text<'a>(s: &'a str, fg: Color) -> El<'a> {
    if present::emoji::emoji_is_big(s.to_string()) {
        return text(s.trim()).size(38).font(ui::EMOJI).color(fg).into();
    }
    let spans: Vec<iced::widget::text::Span<'a, ()>> = present::emoji::emoji_runs(s.to_string())
        .into_iter()
        .map(|run| {
            let sp = iced::widget::span(run.text).color(fg);
            if run.is_emoji { sp.size(15.0 * present::emoji::INLINE_SCALE).font(ui::EMOJI) } else { sp.size(15.0) }
        })
        .collect();
    iced::widget::rich_text(spans).line_height(lh(21.0)).into()
}

/// Greedy word wrap at `max` px (15px body): how many lines, and the last line's width. Used to place the
/// bubble's time the way the playground's float does: on the last line when it fits, else on its own line.
pub(super) fn wrap_last(s: &str, max: f32) -> (usize, f32) {
    use crate::fit::width;
    let space = width(" ", ui::SANS, 15.0);
    let (mut lines, mut cur) = (0usize, 0.0f32);
    for para in s.split('\n') {
        lines += 1;
        cur = 0.0;
        for w in para.split(' ') {
            let ww = width(w, ui::SANS, 15.0);
            if cur > 0.0 && cur + space + ww > max { lines += 1; cur = ww.min(max); }
            else { cur = if cur > 0.0 { cur + space + ww } else { ww.min(max) }; }
        }
    }
    (lines.max(1), cur)
}
