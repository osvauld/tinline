//! Chat screens: the Chats tab of the sidebar and the conversation in the main pane.
//! Laid out after docs/design/DesktopChat.dc.html.

use chrono::{DateTime, Datelike, Local, TimeZone};
use iced::widget::{button, column, container, mouse_area, progress_bar, row, text, text_editor, Space};
use p2pcore::{AttachmentKind, Chat, Contact, DeliveryState, Message as ChatMsg, TransferState};

use crate::voice::{self, VoiceView};
use super::*;
use crate::app::chat::{preview_of, Cm, SideTab, Thumb, ViewBody, COMPOSER_ID};
use crate::media::{self, Class};

/// Width of the hover actions beside a bubble: three 28 px buttons, 2 px apart.
const ACTIONS_W: f32 = 3.0 * 28.0 + 2.0 * 2.0;

fn local(ms: u64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(ms as i64).single()
}

fn hhmm(ms: u64) -> String {
    local(ms).map(|d| d.format("%H:%M").to_string()).unwrap_or_default()
}

/// "Today", "Yesterday", a weekday within the last week, else "12 Sep" (with the year if old).
fn day_label(ms: u64) -> String {
    let (Some(d), now) = (local(ms), Local::now()) else { return String::new() };
    let days = (now.date_naive() - d.date_naive()).num_days();
    match days {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        2..=6 => d.format("%A").to_string(),
        _ if d.year() == now.year() => d.format("%-d %b").to_string(),
        _ => d.format("%-d %b %Y").to_string(),
    }
}

/// The sidebar's time column: the time today, else the day.
fn list_time(ms: u64) -> String {
    if ms == 0 {
        return String::new();
    }
    match day_label(ms).as_str() {
        "Today" => hhmm(ms),
        other => other.to_string(),
    }
}

fn size_text(b: u64) -> String {
    match b {
        0..=999 => format!("{b} B"),
        1_000..=999_999 => format!("{} KB", b / 1_000),
        1_000_000..=9_999_999 => format!("{:.1} MB", b as f64 / 1e6),
        10_000_000..=999_999_999 => format!("{} MB", b / 1_000_000),
        _ => format!("{:.1} GB", b as f64 / 1e9),
    }
}

fn kind_label(name: &str) -> String {
    std::path::Path::new(name).extension().and_then(|e| e.to_str()).map(|e| e.to_uppercase()).unwrap_or_default()
}

fn rule<'a>(t: Tok) -> El<'a> {
    container(Space::new()).width(Fill).height(1).style(ui::plain(t.line, 0.0)).into()
}

impl App {
    /// Chats | Calls | Contacts, with the unread count on Chats.
    pub(super) fn side_tabs(&self, t: Tok) -> El<'_> {
        let tab = |label_: &'static str, which: SideTab, badge_: u32| {
            let on = self.chat.tab == which;
            let fg = if on { t.on_primary_c } else { t.ink2 };
            let mut r = row![semi(label_, 14.0, fg)].spacing(6).align_y(Alignment::Center);
            if badge_ > 0 {
                r = r.push(count_badge(t, badge_));
            }
            button(container(r).center_x(Fill))
                .padding([7, 4])
                .width(Fill)
                .style(ui::button_style(t, if on { Kind::Tonal } else { Kind::Ghost }, 999.0))
                .on_press(Msg::Chat(Cm::Tab(which)))
        };
        container(
            row![
                tab("Chats", SideTab::Chats, self.chat.unread_total()),
                tab("Calls", SideTab::Calls, 0),
                tab("Contacts", SideTab::Contacts, 0),
            ]
            .spacing(2),
        )
        .padding(3)
        .width(Fill)
        .style(ui::plain(t.surface2, 999.0))
        .into()
    }

    /// The chat rows: one per contact, those with messages first.
    pub(super) fn chat_list(&self, t: Tok, q: &str) -> El<'_> {
        let mut rows: Vec<Chat> = self.chat.chats.clone();
        for c in &self.contacts {
            if !rows.iter().any(|r| r.peer_did == c.did) {
                rows.push(Chat {
                    peer_did: c.did.clone(),
                    peer_name: Self::display(c),
                    preview: String::new(),
                    last_outgoing: false,
                    last_delivery: DeliveryState::Delivered,
                    last_activity: 0,
                    unread: 0,
                });
            }
        }
        rows.retain(|r| self.contacts.iter().any(|c| c.did == r.peer_did));
        let mut list = column![].spacing(2);
        let mut n = 0;
        for r in &rows {
            let name = self.chat_name(&r.peer_did);
            if !q.is_empty() && !name.to_lowercase().contains(q) {
                continue;
            }
            n += 1;
            let sel = self.sel.as_ref() == Some(&r.peer_did) && !self.chat.info;
            let unread = r.unread > 0;
            let fg = if sel { t.on_primary_c } else { t.ink };
            let dim = if sel { t.on_primary_c } else { t.ink2 };
            let nm = if unread { bold(name.clone(), 15.0, fg) } else { semi(name.clone(), 15.0, fg) };
            let mut sub = row![].spacing(4).align_y(Alignment::Center);
            if r.last_outgoing && r.last_activity > 0 {
                sub = sub.push(self.ticks(t, r.last_delivery, 14.0, dim));
            }
            let pv = if unread { semi(r.preview.clone(), 13.0, t.ink) } else { tx(r.preview.clone(), 13.0, dim) }
                .wrapping(iced::widget::text::Wrapping::None);
            sub = sub.push(container(pv).width(Fill).clip(true));
            if unread {
                sub = sub.push(count_badge(t, r.unread));
            }
            list = list.push(
                button(
                    row![
                        avatar(t, &name, &r.peer_did, 40.0),
                        column![row![nm, Space::new().width(Fill), tx(list_time(r.last_activity), 12.0, dim)], sub]
                            .spacing(2)
                            .width(Fill),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .width(Fill)
                .style(ui::row_style(t, sel))
                .on_press(Msg::Select(r.peer_did.clone())),
            );
        }
        if n == 0 {
            list = list.push(
                container(
                    column![
                        semi("No chats yet", 14.0, t.ink),
                        tx(
                            "Messages go straight from your computer to theirs, end-to-end encrypted. Nothing is stored on a server.",
                            13.0,
                            t.ink2
                        ),
                        tx("You can message anyone in your contacts.", 13.0, t.ink2),
                    ]
                    .spacing(6),
                )
                .padding([8, 12]),
            );
        }
        list.into()
    }

    /// One tick = only on this computer (a clock while we are offline); two = on theirs.
    fn ticks(&self, t: Tok, d: DeliveryState, size: f32, color: Color) -> El<'static> {
        match d {
            DeliveryState::Delivered => ui::icon(Icon::CheckCheck, size, t.primary),
            DeliveryState::Pending if self.status.online => ui::icon(Icon::Check, size, color),
            DeliveryState::Pending => ui::icon(Icon::Clock, size - 1.0, color),
        }
    }

    /// The contact's presence, not ours: "Connected" only while one of their devices is live.
    fn conn_text(&self, did: &str) -> String {
        if !self.status.started {
            "Connecting\u{2026}".into()
        } else if self.chat.online.contains(did) {
            "Connected".into()
        } else {
            "Not connected \u{b7} messages wait until you\u{2019}re both online".into()
        }
    }

    pub(super) fn conversation_view<'a>(&'a self, t: Tok, c: &'a Contact) -> El<'a> {
        let name = Self::display(c);
        let calling = self.call.is_some();
        let who = button(
            row![
                avatar(t, &name, &c.did, 40.0),
                column![bold(name.clone(), 17.0, t.ink), tx(self.conn_text(&c.did), 13.0, if self.chat.online.contains(&c.did) { t.primary } else { t.ink2 })]
                    .spacing(1),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([4, 8])
        .style(ui::button_style(t, Kind::Ghost, 10.0))
        .on_press(Msg::Chat(Cm::Info(true)));
        let head = container(
            row![
                who,
                Space::new().width(Fill),
                pill(t, Kind::Primary, Some(Icon::Phone), "Call", (!calling).then(|| Msg::CallPressed(c.did.clone()))),
                icon_btn(t, Icon::ShieldCheck, Msg::Chat(Cm::Info(true))),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([10, 20])
        .width(Fill);

        let mut top = column![head, rule(t)];
        if let Some((title, body, action, error)) = self.problem() {
            top = top.push(container(banner(t, title, body, action, error)).padding([12, 20]));
        }
        if self.notice.is_some() {
            top = top.push(container(self.notice_bar(t)).padding([8, 20]));
        }

        let body: El = if self.chat.drop_hover {
            container(
                container(
                    column![
                        ui::icon(Icon::Download, 32.0, t.primary),
                        bold(format!("Drop to send to {name}"), 20.0, t.ink),
                        tx(
                            format!("Encrypted on this computer. Up to 2 GB each. {name} gets it the next time you\u{2019}re both online."),
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

        column![top, body, self.composer(t, &name)].width(Fill).height(Fill).into()
    }

    fn message_list<'a>(&'a self, t: Tok, name: &str) -> El<'a> {
        let mut col = column![].spacing(6).padding([16, 24]);
        if self.chat.msgs.is_empty() {
            let note = if self.chat.loading {
                "Loading\u{2026}".to_string()
            } else {
                "Messages go straight from your computer to theirs, end-to-end encrypted. Nothing is stored on a server.".to_string()
            };
            return container(
                column![bold(if self.chat.loading { "" } else { "No messages yet" }, 20.0, t.ink), tx(note, 14.0, t.ink2)]
                    .spacing(6)
                    .align_x(Alignment::Center)
                    .max_width(420),
            )
            .center_x(Fill)
            .center_y(Fill)
            .height(Fill)
            .into();
        }
        // Earlier days: on this computer, or still on theirs.
        col = col.push(self.history_note(t, name));
        let mut last_day = String::new();
        for m in &self.chat.msgs {
            let d = day_label(m.at);
            if d != last_day {
                col = col.push(
                    container(container(semi(d.clone(), 12.0, t.ink2)).padding([4, 12]).style(ui::plain(t.surface2, 999.0)))
                        .center_x(Fill)
                        .padding([8, 0]),
                );
                last_day = d;
            }
            col = col.push(self.message_row(t, m, name));
        }
        scroll(t, col).anchor_bottom().width(Fill).height(Fill).into()
    }

    fn history_note<'a>(&'a self, t: Tok, name: &str) -> El<'a> {
        let busy = self.chat.fetching;
        let (msg, btn): (String, Option<&str>) = if self.chat.older.is_some() {
            (String::new(), Some("Load earlier messages"))
        } else if self.chat.no_more {
            (format!("That is everything {name} has."), None)
        } else {
            (
                format!(
                    "Older messages are on {name}\u{2019}s computer. This computer keeps them from when you started using it. Earlier days load when you\u{2019}re both online."
                ),
                Some("Ask for earlier messages"),
            )
        };
        let mut c = column![].spacing(8).align_x(Alignment::Center);
        if !msg.is_empty() {
            c = c.push(tx(msg, 13.0, t.ink2));
        }
        if let Some(b) = btn {
            c = c.push(pill(t, Kind::Quiet, None, if busy { "Loading\u{2026}" } else { b }, (!busy).then_some(Msg::Chat(Cm::Older))));
        }
        container(c).center_x(Fill).padding([4, 40]).into()
    }

    fn message_row<'a>(&'a self, t: Tok, m: &'a ChatMsg, name: &str) -> El<'a> {
        let (bg, fg) = if m.outgoing { (t.primary_c, t.on_primary_c) } else { (t.surface2, t.ink) };
        let dim = ui::alpha(fg, 0.7);
        // No Fill-width descendants: they make every bubble occupy the maximum width.
        // Align the shrink-sized metadata to the trailing edge of the content.
        let mut body = column![].spacing(4).width(Length::Shrink).align_x(Alignment::End);
        if m.deleted {
            body = body.push(tx(
                if m.outgoing { "You deleted this message".to_string() } else { format!("{name} deleted this message") },
                14.0,
                dim,
            ));
        } else {
            if let Some(rid) = &m.reply_to {
                let (who, snippet) = match self.chat.find(rid) {
                    Some(r) => (if r.outgoing { "You".to_string() } else { name.to_string() }, preview_of(r)),
                    None => (String::new(), "Earlier message".to_string()),
                };
                let mut q = column![].spacing(1);
                if !who.is_empty() {
                    q = q.push(semi(who, 12.0, fg));
                }
                q = q.push(tx(snippet, 13.0, dim));
                body = body.push(container(q).padding([6, 10]).width(Length::Shrink).clip(true).style(ui::plain(ui::alpha(fg, 0.1), 8.0)));
            }
            if let Some(a) = &m.attachment {
                body = body.push(self.attachment_view(t, m, a, fg, dim));
            }
            if !m.text.is_empty() {
                body = body.push(text(m.text.clone()).size(15).color(fg));
            }
        }
        let mut meta = row![].spacing(4).align_y(Alignment::Center);
        if m.edited_at.is_some() && !m.deleted {
            meta = meta.push(tx("edited \u{b7}", 11.0, dim));
        }
        meta = meta.push(tx(hhmm(m.at), 11.0, dim));
        if m.outgoing {
            meta = meta.push(self.ticks(t, m.delivery, 14.0, dim));
        }
        body = body.push(meta.width(Length::Shrink));
        let bubble = container(body).width(Length::Shrink).padding([8, 12]).max_width(520).style(ui::plain(bg, 14.0));

        let open = self.chat.menu.as_ref() == Some(&m.id);
        let hot = open || self.chat.hover.as_ref() == Some(&m.id);
        let mut line = row![].spacing(6).align_y(Alignment::Start);
        // The actions' room is kept when they are hidden, so hovering never rewraps the bubble.
        let actions: El = if hot && !m.deleted { self.hover_actions(t, m) } else { Space::new().width(ACTIONS_W).into() };
        if m.outgoing {
            line = line.push(Space::new().width(Fill)).push(actions).push(bubble);
        } else {
            line = line.push(bubble).push(actions).push(Space::new().width(Fill));
        }
        let mut col = column![line].spacing(6);
        if open && !m.deleted {
            let menu = self.message_menu(t, m, name);
            col = col.push(if m.outgoing { container(menu).align_right(Fill) } else { container(menu) });
        }
        mouse_area(col)
            .on_enter(Msg::Chat(Cm::Hover(Some(m.id.clone()))))
            .on_exit(Msg::Chat(Cm::Hover(None)))
            .on_right_press(Msg::Chat(Cm::Menu(m.id.clone())))
            .into()
    }

    fn hover_actions<'a>(&'a self, t: Tok, m: &'a ChatMsg) -> El<'a> {
        let b = |i: Icon, msg: Msg| {
            button(container(ui::icon(i, 16.0, t.ink2)).center_x(28).center_y(28))
                .padding(0)
                .style(ui::button_style(t, Kind::Quiet, 999.0))
                .on_press(msg)
        };
        row![
            b(Icon::Reply, Msg::Chat(Cm::Reply(m.id.clone()))),
            b(Icon::Copy, Msg::Chat(Cm::Copy(preview_of(m)))),
            b(Icon::More, Msg::Chat(Cm::Menu(m.id.clone()))),
        ]
        .spacing(2)
        .into()
    }

    fn message_menu<'a>(&'a self, t: Tok, m: &'a ChatMsg, name: &str) -> El<'a> {
        let item = |i: Icon, label_: &'static str, color: Color, msg: Msg| {
            button(row![ui::icon(i, 16.0, color), semi(label_, 14.0, color)].spacing(10).align_y(Alignment::Center))
                .padding([8, 14])
                .width(Fill)
                .style(ui::row_style(t, false))
                .on_press(msg)
        };
        let mut c = column![
            item(Icon::Reply, "Reply", t.ink, Msg::Chat(Cm::Reply(m.id.clone()))),
            item(Icon::Copy, "Copy text", t.ink, Msg::Chat(Cm::Copy(preview_of(m)))),
        ]
        .spacing(2);
        if m.outgoing {
            if !m.text.is_empty() {
                c = c.push(item(Icon::Pencil, "Edit", t.ink, Msg::Chat(Cm::StartEdit(m.id.clone()))));
            }
            c = c
                .push(item(Icon::Trash, "Delete for both\u{2026}", t.error, Msg::Chat(Cm::Delete(m.id.clone()))))
                .push(container(tx(
                    format!("Edits and deletes reach {name} the next time you\u{2019}re both online."),
                    12.0,
                    t.ink2,
                ))
                .padding([4, 14])
                .max_width(240));
        }
        container(c).padding(6).width(260).style(ui::card(t)).into()
    }

    fn attachment_view<'a>(&'a self, t: Tok, m: &'a ChatMsg, a: &'a p2pcore::Attachment, fg: Color, dim: Color) -> El<'a> {
        let voice = a.kind == AttachmentKind::Voice;
        let class = if voice { Class::SaveOnly } else { media::classify(&a.name, &a.mime) };
        let (done, total) = self.chat.progress.get(&a.hash).copied().unwrap_or((a.transferred, a.size));
        let who = self.chat_name(&m.peer_did);
        let status: String = match a.state {
            TransferState::Ready if voice => String::new(),
            TransferState::Ready if m.outgoing && m.delivery == DeliveryState::Pending && !self.status.online => {
                format!("{} \u{b7} {who} gets it when you\u{2019}re both online", size_text(a.size))
            }
            TransferState::Ready => {
                let k = kind_label(&a.name);
                if k.is_empty() { size_text(a.size) } else { format!("{} \u{b7} {k}", size_text(a.size)) }
            }
            TransferState::Remote if !self.status.online => format!("{} \u{b7} not downloaded", size_text(a.size)),
            TransferState::Remote => format!("{} \u{b7} not downloaded", size_text(a.size)),
            TransferState::Downloading if !self.status.online => {
                format!("{} \u{b7} paused \u{2014} waiting for {who} to come online", size_text(a.size))
            }
            TransferState::Downloading => format!("Downloading \u{b7} {} of {}", size_text(done), size_text(total.max(a.size))),
            TransferState::Failed => format!("{} \u{b7} couldn\u{2019}t download \u{2014} tap to retry", size_text(a.size)),
        };
        let action: Option<El> = match a.state {
            TransferState::Remote => Some(self.chip_btn(Icon::Download, "Download", fg, Msg::Chat(Cm::Download(m.id.clone())))),
            TransferState::Failed => Some(self.chip_btn(Icon::RotateCw, "Tap to retry", fg, Msg::Chat(Cm::Download(m.id.clone())))),
            TransferState::Ready => {
                let mut r = row![].spacing(6);
                if !voice && class.openable() {
                    r = r.push(self.chip_btn(Icon::ExternalLink, "Open", fg, Msg::Chat(Cm::Open(m.id.clone()))));
                }
                Some(r.push(self.chip_btn(Icon::Download, "Save", fg, Msg::Chat(Cm::Save(m.id.clone())))).into())
            }
            TransferState::Downloading => Some(self.chip_btn(Icon::X, "Cancel", fg, Msg::Chat(Cm::Cancel(m.id.clone())))),
        };
        let mut c = column![].spacing(6);
        let thumb = match class {
            Class::Image(_) if a.state == TransferState::Ready => match self.chat.thumbs.get(&a.hash) {
                Some(Thumb::Bad) => None,
                other => Some(other),
            },
            Class::Image(_) => Some(None),
            _ => None,
        };
        if let Some(th) = thumb {
            return self.image_view(t, m, a, th, &status, fg, dim);
        }
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

    /// A picture in a bubble: the thumbnail (tap to enlarge), or a box while it is not here yet.
    #[allow(clippy::too_many_arguments)]
    fn image_view<'a>(&'a self, t: Tok, m: &'a ChatMsg, a: &'a p2pcore::Attachment, th: Option<&'a Thumb>, status: &str, fg: Color, dim: Color) -> El<'a> {
        let (done, total) = self.chat.progress.get(&a.hash).copied().unwrap_or((a.transferred, a.size));
        if let Some(Thumb::Ready { handle, w, h }) = th {
            let (dw, dh) = fit(*w as f32 / 2.0, *h as f32 / 2.0);
            let img = iced::widget::image(handle.clone())
                .width(dw)
                .height(dh)
                .content_fit(iced::ContentFit::Fill)
                .border_radius(10.0);
            return button(img)
                .padding(0)
                .style(|_: &Theme, _| button::Style::default())
                .on_press(Msg::Chat(Cm::View(m.id.clone())))
                .into();
        }
        let loading = a.state == TransferState::Ready;
        let mut c = column![].spacing(6).align_x(Alignment::Center);
        c = c.push(ui::icon(Icon::File, 26.0, fg));
        c = c.push(semi(a.name.clone(), 13.0, fg));
        c = c.push(tx(if loading { "Loading\u{2026}".to_string() } else { status.to_string() }, 12.0, dim));
        match a.state {
            TransferState::Remote => c = c.push(self.chip_btn(Icon::Download, "Download", fg, Msg::Chat(Cm::Download(m.id.clone())))),
            TransferState::Failed => c = c.push(self.chip_btn(Icon::RotateCw, "Tap to retry", fg, Msg::Chat(Cm::Download(m.id.clone())))),
            TransferState::Downloading => {
                if self.status.online {
                    let v = if total > 0 { (done as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
                    c = c.push(progress_bar(0.0..=1.0, v).girth(5).length(180).style(move |_: &Theme| progress_bar::Style {
                        background: iced::Background::Color(ui::alpha(fg, 0.15)),
                        bar: iced::Background::Color(t.primary),
                        border: Border { radius: 3.0.into(), ..Default::default() },
                    }));
                }
                c = c.push(self.chip_btn(Icon::X, "Cancel", fg, Msg::Chat(Cm::Cancel(m.id.clone()))));
            }
            _ => {}
        }
        container(c).width(260).height(150).center_x(260).center_y(150).style(ui::plain(ui::alpha(fg, 0.1), 10.0)).into()
    }

    /// The full-window overlay: a picture, or a text file, over a dark scrim.
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
            bar = bar.push(tx(format!("{} of {}", i + 1, ids.len()), 13.0, ui::alpha(white, 0.7)));
        }
        bar = bar
            .push(self.chip_btn(Icon::ExternalLink, "Open in default app", white, Msg::Chat(Cm::OpenExt(v.id.clone()))))
            .push(self.chip_btn(Icon::Download, "Save", white, Msg::Chat(Cm::Save(v.id.clone()))))
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
            ViewBody::Loading => container(tx("Loading\u{2026}", 15.0, ui::alpha(white, 0.8))).center_x(Fill).center_y(Fill).into(),
            ViewBody::Failed(e) => container(tx(format!("Could not show this: {e}"), 15.0, ui::alpha(white, 0.8))).center_x(Fill).center_y(Fill).into(),
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

    fn composer<'a>(&'a self, t: Tok, name: &str) -> El<'a> {
        let mut col = column![rule(t)].spacing(0);
        // What the next message does to an earlier one.
        let bar = |title: String, snippet: String| -> El<'a> {
            container(
                row![
                    column![semi(title, 13.0, t.primary), tx(snippet, 13.0, t.ink2)].spacing(1).width(Fill),
                    icon_btn(t, Icon::X, Msg::Chat(Cm::CancelCompose)),
                ]
                .align_y(Alignment::Center),
            )
            .padding([6, 20])
            .width(Fill)
            .style(ui::plain(t.surface2, 0.0))
            .into()
        };
        if let Some(m) = self.chat.reply.as_deref().and_then(|id| self.chat.find(id)) {
            col = col.push(bar(format!("Replying to {}", if m.outgoing { "yourself".to_string() } else { name.to_string() }), preview_of(m)));
        } else if let Some(m) = self.chat.editing.as_deref().and_then(|id| self.chat.find(id)) {
            col = col.push(bar("Editing".to_string(), preview_of(m)));
        }
        let editor = text_editor(&self.chat.input)
            .id(COMPOSER_ID)
            .placeholder("Message")
            .on_action(|a| Msg::Chat(Cm::Edit(a)))
            .key_binding(|kp| {
                use iced::keyboard::key::Named;
                use iced::keyboard::Key;
                match kp.key.as_ref() {
                    // Enter sends; Shift+Enter is a new line.
                    Key::Named(Named::Enter) if matches!(kp.status, text_editor::Status::Focused { .. }) => Some(
                        if kp.modifiers.shift() {
                            text_editor::Binding::Enter
                        } else {
                            text_editor::Binding::Custom(Msg::Chat(Cm::Send))
                        },
                    ),
                    _ => text_editor::Binding::from_key_press(kp),
                }
            })
            .padding([10, 14])
            .size(15)
            .min_height(24)
            .max_height(140)
            .style(move |_: &Theme, st| {
                let focused = matches!(st, text_editor::Status::Focused { .. });
                text_editor::Style {
                    background: iced::Background::Color(t.surface),
                    border: Border { radius: 20.0.into(), width: 1.0, color: if focused { t.primary } else { t.line } },
                    placeholder: ui::alpha(t.ink2, 0.8),
                    value: t.ink,
                    selection: ui::alpha(t.primary, 0.3),
                }
            });
        let has_text = !self.chat.input.text().trim().is_empty();
        let last: El = if has_text {
            button(container(ui::icon(Icon::Send, 18.0, t.on_primary)).center_x(40).center_y(40))
                .padding(0)
                .width(40)
                .height(40)
                .style(ui::button_style(t, Kind::Primary, 999.0))
                .on_press(Msg::Chat(Cm::Send))
                .into()
        } else {
            voice::mic_button(t, Msg::Chat(Cm::RecStart))
        };
        let line: El = match &self.chat.rec {
            Some((r, _)) => voice::record_bar(t, r.elapsed_ms(), Msg::Chat(Cm::RecSend), Msg::Chat(Cm::RecCancel)),
            None => row![
                icon_btn_big(t, Icon::Paperclip, Msg::Chat(Cm::Attach)),
                container(editor).width(Fill),
                last,
            ]
            .spacing(8)
            .align_y(Alignment::End)
            .into(),
        };
        let hint = if self.chat.rec.is_some() { "" } else { "Enter to send \u{b7} Shift Enter new line" };
        col = col.push(
            container(column![line, container(tx(hint, 11.0, t.ink2)).padding([0, 48])].spacing(4)).padding([10, 20]),
        );
        container(col).width(Fill).style(ui::plain(t.bg, 0.0)).into()
    }
}

/// The size a picture of `w` x `h` is shown at in a bubble: within 320 x 360, never enlarged.
fn fit(w: f32, h: f32) -> (f32, f32) {
    let k = (320.0 / w).min(360.0 / h).min(1.0);
    ((w * k).max(60.0).round(), (h * k).max(40.0).round())
}

fn icon_btn_big<'a>(t: Tok, i: Icon, on: Msg) -> El<'a> {
    button(container(ui::icon(i, 20.0, t.ink2)).center_x(40).center_y(40))
        .padding(0)
        .width(40)
        .height(40)
        .style(ui::button_style(t, Kind::Ghost, 999.0))
        .on_press(on)
        .into()
}

fn count_badge<'a>(t: Tok, n: u32) -> El<'a> {
    container(semi(if n > 99 { "99+".to_string() } else { n.to_string() }, 12.0, t.on_primary))
        .padding([1, 7])
        .style(ui::plain(t.primary, 999.0))
        .into()
}
