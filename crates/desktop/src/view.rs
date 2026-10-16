//! Screens. Everything here is a pure function of `App`; styling comes from `ui`.

use std::sync::atomic::Ordering;
use std::time::Duration;

use iced::widget::{
    button, canvas, column, container, image, pick_list, row, stack, text, text_input, toggler, Space,
};
use iced::{Alignment, Border, Color, ContentFit, Element, Fill, Length, Padding, Point, Rectangle, Renderer, Size, Theme};
use p2pcore::{CallState, LockState};

use super::{
    AddPhase, App, Detail, Msg, Qr, Screen, DEFAULT_LABEL, RECENTS, RENAME_ID, SEARCH_ID,
};
use crate::ui::{self, Icon, Kind, Tok};

#[path = "chat_view.rs"]
mod chat_view;
#[path = "gate_view.rs"]
mod gate_view;
#[path = "signup_view.rs"]
mod signup_view;
#[path = "link_view.rs"]
mod link_view;
#[path = "groups_view.rs"]
mod groups_view;
#[cfg(any(test, feature = "test-hooks"))]
#[path = "groups_preview_view.rs"]
mod groups_preview_view;
#[path = "group_live_view.rs"]
mod group_live_view;
#[path = "shell_view.rs"]
mod shell_view;
#[path = "profile_view.rs"]
mod profile_view;
#[path = "call_screen.rs"]
mod call_screen;
#[path = "gcall_view.rs"]
mod gcall_view;
type El<'a> = Element<'a, Msg>;

fn scroll<'a>(t: Tok, c: impl Into<El<'a>>) -> iced::widget::Scrollable<'a, Msg> {
    iced::widget::scrollable(c).style(ui::scroll_style(t))
}

/// An absolute CSS line height in px (iced's f32 conversion is relative to the font size).
fn lh(px: f32) -> iced::widget::text::LineHeight {
    iced::widget::text::LineHeight::Absolute(px.into())
}

fn tx<'a>(s: impl Into<String>, size: f32, color: Color) -> iced::widget::Text<'a> {
    text(s.into()).size(size).color(color)
}

fn semi<'a>(s: impl Into<String>, size: f32, color: Color) -> iced::widget::Text<'a> {
    tx(s, size, color).font(ui::SANS_SEMI)
}

fn bold<'a>(s: impl Into<String>, size: f32, color: Color) -> iced::widget::Text<'a> {
    tx(s, size, color).font(ui::SANS_BOLD)
}

fn mono<'a>(s: impl Into<String>, size: f32, color: Color) -> iced::widget::Text<'a> {
    tx(s, size, color).font(ui::MONO)
}

/// An uppercase section label.
fn label<'a>(t: Tok, s: &str) -> iced::widget::Text<'a> {
    semi(s.to_uppercase(), 12.0, t.ink2).font(ui::SANS_SEMI)
}

/// Person avatar: the key's fingerprint mandala (picture handled by `profile_avatar`), else Avatar.svelte initials: hue pair, 700 weight, round(size * .34) px, a circle (intentional, not smoothed).
fn avatar<'a>(t: Tok, name: &str, key: &str, size: f32) -> El<'a> {
    // A DID key draws its fingerprint mandala; anything else ("me" before the account loads, group ids) keeps the initials.
    if let Some(face) = ui::fingerprint_face(t, key, size) {
        return face;
    }
    let (bg, fg) = ui::avatar_colors(t, key);
    container(bold(ui::initials(name), (size * 0.34).round(), fg).line_height(lh((size * 0.34).round() * 1.2)))
        .width(size)
        .height(size)
        .center_x(size)
        .center_y(size)
        .style(ui::plain(bg, size / 2.0))
        .into()
}

/// A pill button with an optional leading icon. `on = None` disables it.
fn pill<'a>(t: Tok, kind: Kind, icon: Option<Icon>, text_: &str, on: Option<Msg>) -> El<'a> {
    let fg = match kind {
        Kind::Primary => t.on_primary,
        Kind::Tonal => t.on_primary_c,
        Kind::Accept | Kind::End => Color::WHITE,
        Kind::Danger => t.error,
        _ => t.ink,
    };
    let mut r = row![].spacing(8).align_y(Alignment::Center);
    if let Some(i) = icon {
        r = r.push(ui::icon(i, 18.0, fg));
    }
    r = r.push(semi(text_.to_string(), 15.0, fg));
    button(r)
        .padding([12, 22])
        .style(ui::button_style(t, kind, 999.0))
        .on_press_maybe(on)
        .into()
}

/// Field.svelte look for the screens around sign-up: label 13/650 ink, a 48 input (radius 14, --line; focus:
/// 2px --accent-ink).
fn field<'a>(t: Tok, title: &str, input: text_input::TextInput<'a, Msg>) -> El<'a> {
    let input = input.padding(Padding { top: 14.0, right: 14.0, bottom: 14.0, left: 14.0 }).size(15).line_height(lh(20.0)).style(move |_, st| {
        let focus = matches!(st, text_input::Status::Focused { .. });
        text_input::Style {
            background: t.surface.into(),
            border: Border { color: if focus { t.accent_ink } else { t.line }, width: if focus { 2.0 } else { 1.0 }, radius: 14.0.into() },
            icon: t.ink2, placeholder: ui::alpha(t.ink2, 0.8), value: t.ink, selection: ui::alpha(t.accent, 0.3),
        }
    });
    column![semi(title.to_string(), 13.0, t.ink).line_height(lh(21.0)), input].spacing(6).into()
}

fn banner<'a>(t: Tok, title: &str, body: &str, action: Option<(&str, Msg)>, error: bool) -> El<'a> {
    let (bg, fg) = if error { (t.error_c, t.on_error_c) } else { (t.warn, t.on_warn) };
    let mut r = row![
        column![semi(title.to_string(), 14.0, fg), tx(body.to_string(), 13.0, fg)].spacing(2).width(Fill),
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    if let Some((label_, msg)) = action {
        r = r.push(
            button(semi(label_.to_string(), 14.0, fg))
                .padding([8, 16])
                .style(move |_: &Theme, st| button::Style {
                    background: Some(
                        ui::alpha(fg, if matches!(st, button::Status::Hovered | button::Status::Pressed) { 0.2 } else { 0.12 })
                            .into(),
                    ),
                    text_color: fg,
                    border: Border { radius: 999.0.into(), ..Default::default() },
                    ..Default::default()
                })
                .on_press(msg),
        );
    }
    container(r).padding([12, 16]).width(Fill).style(ui::plain(bg, 12.0)).into()
}

fn badge<'a>(bg: Color, fg: Color, icon: Option<Icon>, s: &str) -> El<'a> {
    let mut r = row![].spacing(6).align_y(Alignment::Center);
    if let Some(i) = icon {
        r = r.push(ui::icon(i, 16.0, fg));
    }
    r = r.push(semi(s.to_string(), 13.0, fg));
    container(r).padding([5, 12]).style(ui::plain(bg, 999.0)).into()
}

/// Age without a time zone: "4 min ago", "3 h ago", "5 d ago".
fn ago(now: u64, then: u64) -> String {
    let d = now.saturating_sub(then);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86399 => format!("{} h ago", d / 3600),
        _ if d < 86400 * 14 => format!("{} d ago", d / 86400),
        _ => format!("{} w ago", d / (86400 * 7)),
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// 1-4 bars from round-trip time and loss, with the call screen's words (call_ui::strength).
fn quality(rtt_ms: u32, loss_pct: f64) -> (u8, &'static str) {
    let n = crate::call_ui::strength(rtt_ms, loss_pct);
    (n, crate::call_ui::SIGNAL_WORD[n as usize])
}

/// Banner content: title, body, optional action, is-error.
type Problem = (&'static str, &'static str, Option<(&'static str, Msg)>, bool);

impl App {
    pub(super) fn t(&self) -> Tok {
        ui::tok(self.dark)
    }

    pub(super) fn view(&self, _id: iced::window::Id) -> El<'_> {
        let t = self.t();
        let body: El = if self.call.is_some() && !self.call_ui.minimized && self.incoming_card().is_none() {
            self.call_view(t)
        } else if self.gcall.live() && !self.gcall.minimized {
            self.gcall_view(t)
        } else if self.ended.is_some() {
            container(self.ended_view(t)).width(Fill).height(Fill).style(ui::plain(t.bg, 0.0)).into()
        } else {
            match self.screen {
                Screen::Onboarding => self.signup_view(t),
                Screen::Unlock => self.unlock_view(t),
                Screen::NameDevice => self.name_device_view(t),
                Screen::KeyFailed => self.key_failed_view(t),
                Screen::Phrase => self.phrase_view(t),
                Screen::Home => self.home_view(t),
                // Add contact opens over Chats (a dialog), not as its own screen.
                Screen::AddContact => self.home_view(t),
                Screen::Settings => self.profile_shell(t),
                Screen::Link => self.link_screen(t),
                Screen::LinkSecure => self.link_secure_screen(t),
                Screen::LinkSync => self.link_sync_screen(t),
                Screen::Unlinked => self.unlinked_screen(t),
            }
        };
        let base = container(body).width(Fill).height(Fill).style(ui::plain(t.bg, 0.0));
        let overlay = self.add_overlay(t).or_else(|| self.viewer_overlay(t)).or_else(|| self.delete_overlay(t)).or_else(|| self.new_chat_overlay(t)).or_else(|| self.live_overlay(t));
        #[cfg(any(test, feature = "test-hooks"))]
        let overlay = overlay.or_else(|| self.groups_overlay(t));
        let base: El = match overlay {
            Some(o) => iced::widget::stack![base, o].into(),
            None => base.into(),
        };
        // "Sam started a call in Weekend hike": a quiet banner over the app (not over a call screen).
        match self.gcall_announce(t).filter(|_| matches!(self.screen, Screen::Home)) {
            Some(a) => iced::widget::stack![base, a].into(),
            None => base,
        }
    }

    /// The ringing incoming call that shows as the card (not opened to the full screen yet), if any.
    fn incoming_card(&self) -> Option<&super::CallView> {
        self.call.as_ref().filter(|c| c.info.incoming && !c.answered && c.state == CallState::Ringing && !self.call_ui.expanded)
    }

    /// RingHead.svelte (desktop): the sidebar's brand row turns into the caller, in place (same box, nothing moves):
    /// avatar 28, name 13/16 650 over "Incoming call" 11/14 muted, then Decline and Answer as 28 px circles in 32 px
    /// boxes. Tapping the caller opens the full incoming screen.
    pub(super) fn ring_head(&self, t: Tok) -> Option<El<'_>> {
        if let Some(w) = self.ring_head_waiting(t) { return Some(w); }
        let c = self.incoming_card()?;
        Some(self.ring_row(t, &c.info.peer_name, &c.info.peer_did, "Incoming call", Msg::CallUi(crate::call_ui::Action::Restore), Msg::Decline, Msg::Answer))
    }

    fn ring_row(&self, t: Tok, name: &str, did: &str, sub: &str, open: Msg, decline: Msg, answer: Msg) -> El<'_> {
        let who = button(row![self.profile_avatar(t, name, did, 28.0),
                column![semi(crate::fit::ellipsize(name, ui::SANS_SEMI, 13.0, 88.0), 13.0, t.ink).line_height(lh(16.0)).wrapping(iced::widget::text::Wrapping::None),
                    tx(sub.to_string(), 11.0, t.ink2).line_height(lh(14.0)).wrapping(iced::widget::text::Wrapping::None)]].spacing(8).align_y(Alignment::Center))
            .padding(0).height(32).width(Fill).style(shell_view::wash(t, None, None, 14.0)).on_press(open);
        let rb = |hang: bool, bg: Color, fg: Color, msg: Msg| -> El<'_> {
            let icon: El = if hang { crate::ui::glyph(crate::pg_icons::STROKE, r#"<g transform="rotate(135 12 12)"><path d="M5 4h3.5l1.5 4.5-2.2 1.4a11 11 0 0 0 6.3 6.3l1.4-2.2L20 15.5V19a1.5 1.5 0 0 1-1.6 1.5A16 16 0 0 1 3.5 5.6 1.5 1.5 0 0 1 5 4z"/></g>"#, 16.0, fg) } else { crate::pg_icons::pgi("phone", 16.0, fg) };
            button(container(container(icon).center_x(28).center_y(28).style(ui::plain(bg, 14.0))).center_x(32).center_y(32))
                .padding(0).width(32).height(32).style(|_: &Theme, _| button::Style::default()).on_press(msg).into()
        };
        row![who, rb(true, t.danger_btn, t.on_danger_btn, decline), rb(false, t.accent, t.on_accent, answer)].align_y(Alignment::Center).height(32).into()
    }

    // ---- shared pieces ----

    fn notice_bar(&self, t: Tok) -> El<'_> {
        match &self.notice {
            Some(n) => ui::raised(container(tx(n.clone(), 14.0, t.on_primary_c)).padding([10, 14]).width(Fill), t, ui::Depth::Pop, ui::plain(t.primary_c, 10.0)),
            None => Space::new().into(),
        }
    }

    /// A centred card on the background, for the screens before the home screen.
    fn shell<'a>(&'a self, t: Tok, title: &str, sub: &str, content: El<'a>) -> El<'a> {
        gate_view::frame(t, title, sub, content)
    }

    fn pass_field<'a>(&'a self, t: Tok, title: &'a str, value: &'a str, on: fn(String) -> Msg, submit: Option<Msg>) -> El<'a> {
        let mut input = text_input("", value).secure(true).on_input(on);
        if let Some(m) = submit {
            input = input.on_submit(m);
        }
        field(t, title, input)
    }

    /// One passphrase field of any length: no repeat, no strength rules.
    fn new_pass_field(&self, t: Tok, title: &'static str, submit: Msg) -> El<'_> {
        self.pass_field(t, title, &self.pass_in, Msg::PassIn, Some(submit))
    }

    /// A full-width gate action (Btn.svelte desktop look): 48 tall, radius 14, 15/650. Primary is accent; the
    /// others are text in --accent-ink (danger in --danger), with a --surface-2 wash on hover.
    fn wide<'a>(&self, label_: &str, kind: Kind, t: Tok, on: Option<Msg>) -> El<'a> {
        let off = on.is_none();
        let fg = match kind {
            Kind::Primary if off => t.ink2,
            Kind::Primary => t.on_accent,
            Kind::Danger => t.danger,
            _ if off => ui::alpha(t.accent_ink, 0.5),
            _ => t.accent_ink,
        };
        let style = move |_: &Theme, st: button::Status| {
            let hover = matches!(st, button::Status::Hovered | button::Status::Pressed) && !off;
            let bg = match kind {
                Kind::Primary if off => t.surface2,
                Kind::Primary => t.accent,
                _ if hover => t.surface2,
                _ => Color::TRANSPARENT,
            };
            let border = if matches!(kind, Kind::Quiet) { if off { ui::alpha(t.line, 0.5) } else { t.line } } else { Color::TRANSPARENT };
            button::Style { background: Some(bg.into()), text_color: fg, border: Border { color: border, width: if border.a > 0.0 { 1.0 } else { 0.0 }, radius: 14.0.into() }, ..Default::default() }
        };
        button(container(semi(label_.to_string(), 15.0, fg).line_height(lh(20.0))).center_x(Fill).center_y(48))
            .padding(0)
            .height(48)
            .width(Fill)
            .style(style)
            .on_press_maybe(on)
            .into()
    }

    // ---- before home ----

    /// Last onboarding step. The key is saved and the account committed only when this is submitted.
    fn name_device_view(&self, t: Tok) -> El<'_> {
        let col = column![
            field(
                t,
                "Device name",
                text_input("Work desktop", &self.device_in).on_input(Msg::DeviceIn).on_submit(Msg::DeviceGo),
            ),
            tx("You can rename it later.", 12.0, t.ink2),
            self.notice_bar(t),
            self.wide(if self.busy { "Saving..." } else { "Continue" }, Kind::Primary, t, (!self.busy).then_some(Msg::DeviceGo)),
        ]
        .spacing(12);
        self.shell(
            t,
            "Name this computer",
            "So you can tell your devices apart in Linked devices. Only your own devices see this name; people you call never do.",
            col.into(),
        )
    }

    /// The key could not be stored. The account is not saved yet, so nothing is lost: retry, or
    /// protect it with a passphrase instead.
    fn key_failed_view(&self, t: Tok) -> El<'_> {
        let mut col = column![tx(
            "Tinline couldn\u{2019}t store the key that opens your account on this computer, so your account has not been saved yet. Nothing is lost.",
            14.0,
            t.ink,
        )]
        .spacing(12);
        if let Some(e) = &self.key_fail {
            col = col.push(tx(e.clone(), 12.0, t.error));
        }
        col = col
            .push(self.wide(if self.busy { "Trying..." } else { "Retry" }, Kind::Primary, t, (!self.busy).then_some(Msg::RetryKey)))
            .push(label(t, "Or"))
            .push(self.new_pass_field(t, "Passphrase", Msg::KeyFailPass))
            .push(self.wide("Add a passphrase instead", Kind::Quiet, t, (!self.busy).then_some(Msg::KeyFailPass)))
            .push(self.notice_bar(t));
        self.shell(t, "Couldn\u{2019}t save your account", "The system keyring did not accept the key.", col.into())
    }

    /// Locked with no passphrase: the stored key is gone, so only the 24 words bring the account back.
    fn key_lost_view(&self, t: Tok) -> El<'_> {
        let mut col = column![tx(
            "You didn\u{2019}t set a passphrase, so the 24-word recovery phrase is the only way back. Your contacts stay.",
            14.0,
            t.ink,
        )]
        .spacing(12);
        if let Some(n) = &self.notice {
            col = col.push(tx(n.clone(), 13.0, t.error));
        }
        col = col
            .push(self.wide("Restore with recovery phrase", Kind::Primary, t, (!self.busy).then_some(Msg::GoRestore)))
            .push(self.wide("Try again", Kind::Ghost, t, (!self.busy).then_some(Msg::Unlock)))
            .push(self.other_accounts(t));
        self.shell(
            t,
            "Can\u{2019}t open your account",
            "This computer no longer has the key that opens it, and its keyring entry is missing. Once you restore, your calls come through again.",
            col.into(),
        )
    }

    fn other_accounts(&self, t: Tok) -> El<'_> {
        let mut c = column![].spacing(8);
        let others: Vec<_> = self.accounts.iter().filter(|a| !a.current).collect();
        if others.is_empty() {
            return c.into();
        }
        c = c.push(label(t, "Other accounts"));
        for a in others {
            c = c.push(
                button(
                    row![avatar(t, &a.name, &a.did, 28.0), semi(a.name.clone(), 14.0, t.ink), Space::new().width(Fill), ui::icon(Icon::Lock, 16.0, t.ink2)]
                        .spacing(10)
                        .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .width(Fill)
                .style(ui::row_style(t, false))
                .on_press_maybe((!self.busy).then(|| Msg::SwitchNow(a.did.clone()))),
            );
        }
        c.into()
    }

    /// Unlock: the gate card (signup_view `unlock_card`); with no passphrase, the key-lost card.
    fn unlock_view(&self, t: Tok) -> El<'_> {
        if !self.has_pass {
            return self.key_lost_view(t);
        }
        self.unlock_card(t)
    }

    fn phrase_view(&self, t: Tok) -> El<'_> {
        let phrase = self.new_phrase.as_ref().map(|p| p.to_string()).unwrap_or_default();
        let words: Vec<&str> = phrase.split_whitespace().collect();
        let mut grid = column![].spacing(6);
        for (r, chunk) in words.chunks(4).enumerate() {
            let mut line = row![].spacing(8);
            for (c, w) in chunk.iter().enumerate() {
                line = line.push(
                    container(
                        row![
                            mono(format!("{:>2}", r * 4 + c + 1), 12.0, t.ink2),
                            mono((*w).to_string(), 14.0, t.ink)
                        ]
                        .spacing(8),
                    )
                    .padding([8, 10])
                    .width(Fill)
                    .style(ui::plain(t.surface2, 8.0)),
                );
            }
            grid = grid.push(line);
        }
        let col = column![
            grid,
            tx(
                "Anyone who sees these words can become you. Write them down and keep them somewhere safe.",
                13.0,
                t.ink2
            ),
            self.wide("Copy recovery phrase", Kind::Quiet, t, Some(Msg::CopyPhrase)),
            tx("Paste only into a trusted password manager; the clipboard contains your account key.", 13.0, t.ink2),
            self.wide("I have saved it", Kind::Primary, t, Some(Msg::PhraseSaved)),
        ]
        .spacing(16);
        self.shell(
            t,
            "Your recovery phrase",
            "These 24 words are the only way to restore your identity on another device.",
            col.into(),
        )
    }

    // ---- home ----

    fn problem(&self) -> Option<Problem> {
        if self.lock == LockState::Locked {
            return None;
        }
        if self.status.started && !self.status.online {
            return Some(("You\u{2019}re offline", "Your calls come through again once you\u{2019}re back online.", None, true));
        }
        if !self.avail.available {
            return Some((
                "You\u{2019}re not available",
                "Calls won\u{2019}t ring until you turn it back on.",
                Some(("Turn on", Msg::SetAvail(true, None))),
                false,
            ));
        }
        None
    }

    fn contact_sub(&self, did: &str) -> String {
        if let Some(st) = self.blocks.status(did) {
            return st.to_string();
        }
        match self.recents.iter().find(|r| r.peer_did == did) {
            Some(r) => format!("Last call {}", ago(now_secs(), r.started_at)),
            None => "Not called yet".into(),
        }
    }

    /// Our name for a person: the contact's alias or name when we still have them, else the
    /// name recorded with the call.
    fn name_of(&self, did: &str, fallback: &str) -> String {
        self.contacts.iter().find(|c| c.did == did).map(Self::display).unwrap_or_else(|| fallback.to_string())
    }

    /// What the sidebar mark shows: the call phase (dialing / ringing / active / reconnecting) from the
    /// open call, else the node: started and on a home relay = online, started without one = connecting,
    /// not started (locked, starting) = offline.
    fn mark_look(&self) -> crate::mark::Look {
        use crate::mark::{status_look, CallPhase, Net};
        let call = self.call.as_ref().and_then(|c| match &c.state {
            CallState::Dialing => Some(CallPhase::Outgoing),
            CallState::Ringing if c.info.incoming && !c.answered => Some(CallPhase::Incoming),
            CallState::Ringing => Some(CallPhase::Outgoing),
            CallState::Active if c.stats.as_ref().is_some_and(|s| s.reconnecting) => Some(CallPhase::Reconnecting),
            CallState::Active => Some(CallPhase::Active),
            CallState::Ended { .. } => None,
        });
        let net = match (self.status.started, self.status.online) {
            (true, true) => Net::Online,
            (true, false) => Net::Connecting,
            _ => Net::Offline,
        };
        let l = status_look(call, net);
        crate::mark::Look { state: l.state, sway: l.sway, dim: l.dim, pluck: self.mark_pluck, ..Default::default() }
    }

    pub(super) fn list_pane(&self, t: Tok) -> El<'_> {
        let mut col = column![self.desktop_search_field(t)].spacing(0);
        if self.shell_search.active {
            return self.sidebar_frame(t, col, self.shell_search_results(t));
        }
        let title = match self.chat.tab {
            super::chat::SideTab::Chats => "Chats", super::chat::SideTab::Groups => "Groups",
            super::chat::SideTab::Calls => "Calls", super::chat::SideTab::Contacts => "Contacts",
        };
        col = col.push(self.list_header(t, title));
        let q = String::new();
        if self.chat.tab == super::chat::SideTab::Chats {
            col = col.push(self.chat_chips(t));
            return self.sidebar_frame(t, col, self.chat_list(t, &q));
        }
        if self.chat.tab == super::chat::SideTab::Contacts {
            return self.sidebar_frame(t, col, self.contacts_list(t));
        }
        if self.chat.tab == super::chat::SideTab::Groups {
            return self.sidebar_frame(t, col, self.live_list(t, &q));
        }
        let calls_tab = self.chat.tab == super::chat::SideTab::Calls;
        let mut list = column![].spacing(2);
        if calls_tab {
            return self.sidebar_frame(t, col, self.calls_list(t));
        }
        let shown: Vec<&p2pcore::Contact> = self
            .contacts
            .iter()
            .filter(|c| q.is_empty() || Self::display(c).to_lowercase().contains(&q) || c.name.to_lowercase().contains(&q))
            .collect();
        list = list.push(
            container(label(t, &format!("Contacts \u{b7} {}", self.contacts.len())))
                .padding(Padding { top: 12.0, bottom: 4.0, left: 12.0, right: 0.0 }),
        );
        let mut count = 0;
        for c in shown {
            count += 1;
            let name = Self::display(c);
            let sel = self.sel.as_ref() == Some(&c.did);
            list = list.push(
                button(
                    row![
                        avatar(t, &name, &c.did, 40.0),
                        column![semi(name, 15.0, if sel { t.on_primary_c } else { t.ink }), tx(self.contact_sub(&c.did), 13.0, if sel { t.on_primary_c } else { t.ink2 })]
                            .spacing(2)
                            .width(Fill),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .width(Fill)
                .style(ui::row_style(t, sel))
                .on_press(Msg::Select(c.did.clone())),
            );
        }
        if self.contacts.is_empty() {
            list = list.push(container(tx("Contacts you add appear here.", 13.0, t.ink2)).padding([4, 12]));
        } else if !q.is_empty() && count == 0 {
            list = list.push(container(tx(
                "Not here? Tinline has no directory \u{2014} add people with their code.",
                13.0,
                t.ink2,
            )).padding([4, 12]));
        }
        self.sidebar_frame(t, col, list.into())
    }

    /// CallsList.svelte: every call, newest first. Avatar = their profile, row = the chat, phone = call back
    /// (dimmed with the reason as its tooltip when calling isn't possible). Rows 68 tall in 8px side margins.
    fn calls_list(&self, t: Tok) -> El<'_> {
        use shell_view::wash;
        let known = |did: &str| self.contacts.iter().any(|c| c.did == did);
        let mut items: Vec<(u64, El<'_>)> = Vec::new();
        let mut any = false;
        for r in self.recents.iter().filter(|r| known(&r.peer_did)).take(RECENTS as usize) {
            any = true;
            let name = self.name_of(&r.peer_did, &r.peer_name);
            let answered = r.duration_secs > 0;
            let missed = r.incoming && r.missed;
            let what = match (r.incoming, answered) {
                (false, true) => "Outgoing",
                (true, true) => "Incoming",
                (true, false) => if missed { "Missed" } else { "Declined" },
                (false, false) => match r.reason.as_str() {
                    "no_answer" => "No answer",
                    "declined" => "Declined",
                    "cancelled" | "hangup_local" => "Cancelled",
                    _ => "Couldn\u{2019}t connect",
                },
            };
            let d = r.duration_secs;
            let dur = if d >= 3600 { format!("{}:{:02}:{:02}", d / 3600, d % 3600 / 60, d % 60) } else { format!("{}:{:02}", d / 60, d % 60) };
            let sub_c = if missed { t.error } else { t.ink2 };
            let sub = row![crate::pg_icons::pgi(if r.incoming { "call-in" } else { "call-out" }, 15.0, sub_c),
                tx(if answered { format!("{what} \u{b7} {dur}") } else { what.to_string() }, 13.0, sub_c).line_height(lh(21.0))].spacing(5).align_y(Alignment::Center);
            let sel = self.sel.as_ref() == Some(&r.peer_did) && self.live.open.is_none();
            let pf = button(self.profile_avatar(t, &name, &r.peer_did, 48.0)).padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 8.0 })
                .style(wash(t, None, None, 14.0)).on_press(Msg::ShellSearch(super::shell_search::Action::OpenPerson(r.peer_did.clone())));
            let row_ = button(row![column![semi(name.clone(), 15.0, t.ink).line_height(lh(21.0)).wrapping(iced::widget::text::Wrapping::None), sub].spacing(2).width(Fill),
                    tx(chat_view::list_time(r.started_at * 1000), 11.0, t.ink2)].spacing(12).align_y(Alignment::Center))
                .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 12.0 }).height(68).width(Fill)
                .style(wash(t, None, None, 14.0));
            use crate::blocks::CallGate;
            let gate = self.blocks.call_gate(&r.peer_did, true);
            let ok = matches!(gate, CallGate::Ok);
            // The row calls them (decided 2026-10-14: the chat lives in Chats). In a call: back to it. Someone I can't
            // call: their profile, which says why.
            let press = if self.call.is_some() { Msg::CallUi(crate::call_ui::Action::Restore) }
                else if ok { Msg::CallPressed(r.peer_did.clone()) }
                else { Msg::ShellSearch(super::shell_search::Action::OpenPerson(r.peer_did.clone())) };
            let row_ = row_.on_press(press.clone());
            let why = match gate { CallGate::Ok => "Call".to_string(), CallGate::NotAllowed => "Messages only: calling needs their permission".into(), CallGate::Closed(_) => "Blocked".into() };
            let back = button(container(crate::pg_icons::pgi("phone", 22.0, if ok { t.accent_ink } else { ui::alpha(t.ink2, 0.45) })).center_x(44).center_y(44))
                .padding(0).width(44).height(44).style(wash(t, None, ok.then_some(t.surface2), 22.0))
                .on_press(press.clone());
            let back = iced::widget::tooltip(back, text(why).size(13), iced::widget::tooltip::Position::Left);
            let item = button(row![pf, row_, back].spacing(2).align_y(Alignment::Center)).padding(0).width(Fill).on_press(press)
                .style(wash(t, sel.then_some(t.accent_soft), Some(if sel { t.accent_soft } else { t.surface2 }), 14.0));
            items.push((r.started_at, item.into()));
        }
        // Group calls (CallsList.svelte): the group's avatar opens its info; the row starts or joins a call there.
        for r in &self.group_log {
            let Some(g) = self.live.list.iter().find(|g| g.id == r.group_id && super::group_live::active(g.my_role)) else { continue };
            any = true;
            let gid = g.id.clone();
            let sel = self.live.open.as_deref() == Some(gid.as_str());
            // The group's own mandala in its rounded square (FINGERPRINT-AVATARS.md, Groups).
            let face = ui::group_face(t, &gid, 48.0);
            let pf = button(face).padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 8.0 })
                .style(wash(t, None, None, 14.0)).on_press(Msg::Live(super::group_live::Lm::OpenInfo(gid.clone())));
            let sub = row![crate::pg_icons::pgi("users", 15.0, t.ink2), tx(p2pcore::group_call_label(r.clone()), 13.0, t.ink2).line_height(lh(21.0))].spacing(5).align_y(Alignment::Center);
            let press = if self.call.is_some() { Msg::CallUi(crate::call_ui::Action::Restore) } else { Msg::GCall(super::gcall::Gm::Call(gid.clone())) };
            let row_ = button(row![column![semi(g.name.clone(), 15.0, t.ink).line_height(lh(21.0)).wrapping(iced::widget::text::Wrapping::None), sub].spacing(2).width(Fill),
                    tx(chat_view::list_time(r.started_at * 1000), 11.0, t.ink2)].spacing(12).align_y(Alignment::Center))
                .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 12.0 }).height(68).width(Fill)
                .style(wash(t, None, None, 14.0)).on_press(press.clone());
            let back = button(container(crate::pg_icons::pgi("phone", 22.0, t.accent_ink)).center_x(44).center_y(44))
                .padding(0).width(44).height(44).style(wash(t, None, Some(t.surface2), 22.0)).on_press(press.clone());
            let back = iced::widget::tooltip(back, text("Start or join a call").size(13), iced::widget::tooltip::Position::Left);
            let item = button(row![pf, row_, back].spacing(2).align_y(Alignment::Center)).padding(0).width(Fill).on_press(press)
                .style(wash(t, sel.then_some(t.accent_soft), Some(if sel { t.accent_soft } else { t.surface2 }), 14.0));
            items.push((r.started_at, item.into()));
        }
        if !any {
            return self.empty_quiet(t, "Calls you make and receive show up here.");
        }
        items.sort_by(|a, b| b.0.cmp(&a.0));
        let list = items.into_iter().fold(column![], |c, (_, e)| c.push(e));
        container(list).padding([0, 8]).width(Fill).into()
    }

    /// `.listpane`: 380px on --surface with a 1px hairline end edge (no gap to its neighbours).
    fn sidebar_frame<'a>(&'a self, t: Tok, col: iced::widget::Column<'a, Msg>, list: El<'a>) -> El<'a> {
        row![
            container(column![col, scroll(t, list).height(Fill)].spacing(0)).width(379).height(Fill).style(ui::plain(t.surface, 0.0)),
            container(Space::new()).width(1).height(Fill).style(ui::plain(t.hairline, 0.0)),
        ]
        .width(380)
        .height(Fill)
        .into()
    }

    fn home_view(&self, t: Tok) -> El<'_> {
        if self.sel.is_none() && self.live.open.is_none() && !self.preview_open() && self.offer.is_none() && self.notice.is_none() && self.problem().is_none() {
            return self.framed(t, self.detail_empty(t));
        }
        let mut main = column![].spacing(16);
        if let Some((title, body, action, error)) = self.problem() {
            main = main.push(banner(t, title, body, action, error));
        }
        if self.notice.is_some() {
            main = main.push(self.notice_bar(t));
        }
        if let Some(pane) = self.live_pane(t) {
            return self.framed(t, pane);
        }
        #[cfg(any(test, feature = "test-hooks"))]
        if let Some(pane) = self.preview_pane(t) {
            return self.framed(t, pane);
        }
        if let Some(c) = self.selected().filter(|_| !self.chat.info) {
            let pane: El = match self.offer_bar(t) {
                Some(o) => column![container(o).padding([12, 16]), self.conversation_view(t, c)].width(Fill).height(Fill).into(),
                None => self.conversation_view(t, c),
            };
            return self.framed(t, pane);
        }
        if let Some(o) = self.offer_bar(t) {
            main = main.push(o);
        }
        main = main.push(match self.selected() {
            Some(c) => self.detail_view(t, c),
            None => self.detail_empty(t),
        });
        self.framed(t, scroll(t, container(main).padding(32).max_width(760).width(Fill)).width(Fill).height(Fill).into())
    }

    /// "Add <name>?" for a card found on the clipboard.
    fn offer_bar(&self, t: Tok) -> Option<El<'_>> {
        let o = self.offer.as_ref()?;
        Some(
            container(
                row![
                    avatar(t, &o.name, &o.did, 36.0),
                    column![semi(format!("Add {}?", o.name), 15.0, t.ink), tx("Their card is on your clipboard.", 13.0, t.ink2)]
                        .spacing(2)
                        .width(Fill),
                    pill(t, Kind::Primary, None, "Add", Some(Msg::OfferAdd)),
                    pill(t, Kind::Ghost, None, "Not now", Some(Msg::OfferDismiss)),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding([12, 16])
            .width(Fill)
            .style(ui::card(t))
            .into(),
        )
    }

    fn detail_view<'a>(&'a self, t: Tok, c: &'a p2pcore::Contact) -> El<'a> {
        let name = Self::display(c);
        let back = pill(t, Kind::Ghost, Some(Icon::ArrowLeft), "Home", Some(Msg::Home));
        let mut title = column![bold(name.clone(), 28.0, t.ink)].spacing(4);
        let added = format!("Added {}", utc_date(c.added_at));
        title = title.push(tx(
            if c.alias.as_ref().is_some_and(|a| !a.is_empty()) {
                format!("Calls themself \u{201c}{}\u{201d} \u{b7} {added}", c.name)
            } else {
                added
            },
            14.0,
            t.ink2,
        ));
        let mut badges = row![].spacing(8);
        if c.verified {
            badges = badges.push(badge(t.primary_c, t.on_primary_c, Some(Icon::ShieldCheck), "Verified"));
        }
        if let Some(st) = self.blocks.status(&c.did) {
            badges = badges.push(badge(t.error_c, t.on_error_c, Some(Icon::PhoneOff), st));
        }
        let blocked_by_me = self.blocks.blocked_by_me(&c.did);
        // Disabled while a block is on; tapping when they haven't allowed calls says so.
        let gate = self.blocks.call_gate(&c.did, true);
        let can_call = self.demo || !matches!(gate, crate::blocks::CallGate::Closed(_));
        let call_icon = if self.demo || self.blocks.can_call(&c.did, true) { Icon::Phone } else { Icon::PhoneOff };
        let head = row![avatar(t, &name, &c.did, 72.0), column![title, badges].spacing(10).width(Fill)]
            .spacing(20)
            .align_y(Alignment::Center);
        let actions = row![
            pill(t, Kind::Primary, Some(call_icon), "Call", can_call.then(|| Msg::CallPressed(c.did.clone()))),
            pill(t, Kind::Quiet, Some(Icon::MessageSquare), "Message", Some(Msg::Chat(super::chat::Cm::Info(false)))),
            pill(t, Kind::Quiet, Some(Icon::Pencil), "Rename", Some(Msg::SetDetail(Detail::Rename))),
            pill(t, Kind::Quiet, Some(Icon::ShieldCheck), "Safety number", Some(Msg::SetDetail(Detail::Verify))),
            // U19: Block is the contact action; there is no Remove.
            if blocked_by_me {
                pill(t, Kind::Quiet, None, "Unblock", Some(Msg::Unblock(c.did.clone())))
            } else {
                pill(t, Kind::Danger, Some(Icon::PhoneOff), "Block", Some(Msg::SetDetail(Detail::ConfirmBlock)))
            },
        ]
        .spacing(10);
        // "Allow calls" (U13, P6): whether they may ring us. Off while we block them (a block
        // turns everything off; the switch comes back as it was).
        let allow_calls: Option<El> = (!self.demo).then(|| {
            let on = self.blocks.calls_allowed(&c.did);
            let first = crate::blocks::first_name(&name).to_string();
            let sub = if blocked_by_me {
                "Blocked: nothing gets through until you unblock.".to_string()
            } else if on {
                format!("{first} can ring this computer and your other devices.")
            } else {
                format!("Off. {first} can message you; their calls don\u{2019}t ring.")
            };
            let did = c.did.clone();
            let tg = toggler(on).style(ui::toggle_style(t));
            let tg = if blocked_by_me { tg } else { tg.on_toggle(move |v| Msg::AllowCalls(did.clone(), v)) };
            line(t, "Allow calls", &sub, tg.into())
        });

        let panel: Option<El> = match self.detail {
            Detail::View => None,
            Detail::Rename => Some(
                container(
                    column![
                        semi("Name on this computer", 15.0, t.ink),
                        tx(
                            format!("Only you see this. {} still calls themself \u{201c}{}\u{201d}.", name, c.name),
                            13.0,
                            t.ink2
                        ),
                        text_input("Name", &self.rename_in)
                            .id(RENAME_ID)
                            .on_input(Msg::RenameIn)
                            .on_submit(Msg::RenameSave)
                            .padding(11)
                            .size(15)
                            .style(ui::input_style(t)),
                        row![
                            pill(t, Kind::Ghost, None, "Use their name", Some(Msg::RenameClear)),
                            Space::new().width(Fill),
                            pill(t, Kind::Ghost, None, "Cancel", Some(Msg::SetDetail(Detail::View))),
                            pill(t, Kind::Primary, None, "Save", Some(Msg::RenameSave)),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    ]
                    .spacing(10),
                )
                .padding(20)
                .style(ui::card(t))
                .into(),
            ),
            Detail::Verify => {
                let digits = self.safety.clone().unwrap_or_default();
                let groups: Vec<String> = digits
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                let groups = if groups.len() == 1 {
                    digits.as_bytes().chunks(5).map(|c| String::from_utf8_lossy(c).into_owned()).collect()
                } else {
                    groups
                };
                let mut grid = column![].spacing(10);
                for line in groups.chunks(4) {
                    let mut r = row![].spacing(12);
                    for g in line {
                        r = r.push(
                            container(mono(g.clone(), 20.0, t.ink))
                                .padding([10, 14])
                                .width(Fill)
                                .center_x(Fill)
                                .style(ui::plain(t.surface2, 10.0)),
                        );
                    }
                    grid = grid.push(r);
                }
                Some(
                    container(
                        column![
                            semi(format!("Verify {name}"), 17.0, t.ink),
                            tx(
                                format!("Compare these numbers with {name}, in person or on a Tinline call. If they match, your calls go only to them \u{2014} nobody is in the middle."),
                                14.0,
                                t.ink2
                            ),
                            grid,
                            tx("Same on both computers. Read them aloud in groups.", 13.0, t.ink2),
                            row![
                                pill(t, Kind::Primary, Some(Icon::Check), "They match", Some(Msg::SetVerified(true))),
                                pill(t, Kind::Quiet, Some(Icon::Copy), "Copy", Some(Msg::CopySafety)),
                                pill(t, Kind::Danger, Some(Icon::X), "They don\u{2019}t match", Some(Msg::SetVerified(false))),
                                Space::new().width(Fill),
                                pill(t, Kind::Ghost, None, "Close", Some(Msg::SetDetail(Detail::View))),
                            ]
                            .spacing(8),
                            tx(
                                format!("If they don\u{2019}t match, block {name} and add each other again face to face."),
                                13.0,
                                t.ink2
                            ),
                        ]
                        .spacing(14),
                    )
                    .padding(24)
                    .style(ui::card(t))
                    .into(),
                )
            }
            Detail::ConfirmBlock => Some({
                let (title_, lines) = crate::blocks::block_dialog(&name);
                let text_ = lines.iter().map(|b| format!("\u{2022} {b}")).collect::<Vec<_>>().join("\n");
                container(
                    column![
                        semi(title_, 17.0, t.ink),
                        tx(text_, 14.0, t.ink2),
                        row![
                            Space::new().width(Fill),
                            pill(t, Kind::Ghost, None, "Cancel", Some(Msg::SetDetail(Detail::View))),
                            pill(t, Kind::End, None, "Block", Some(Msg::Block(c.did.clone()))),
                        ]
                        .spacing(8),
                    ]
                    .spacing(12),
                )
                .padding(24)
                .style(ui::card(t))
                .into()
            }),
        };

        let cell = |s: String, w: f32, c: Color| container(tx(s, 13.0, c)).width(w);
        let mut hist = column![
            semi(format!("Calls with {name}"), 16.0, t.ink),
            row![
                Space::new().width(28),
                container(label(t, "Call")).width(Fill),
                container(label(t, "When")).width(150),
                container(label(t, "Length")).width(80),
                container(label(t, "Path")).width(90),
            ]
            .spacing(8),
        ]
        .spacing(10);
        if self.detail_calls.is_empty() {
            hist = hist.push(container(tx("No calls yet.", 14.0, t.ink2)).padding([8, 0]));
        }
        for r in &self.detail_calls {
            let bad = r.missed || r.reason == "unreachable";
            let fg = if bad { t.error } else { t.ink };
            let what = if r.missed {
                "Missed".to_string()
            } else if r.duration_secs == 0 {
                crate::reason::history(&r.reason, r.incoming, r.missed, 0, &name)
            } else if r.incoming {
                "Incoming".into()
            } else {
                "Outgoing".into()
            };
            let len = if r.duration_secs == 0 {
                "\u{2014}".to_string()
            } else {
                format!("{}:{:02}", r.duration_secs / 60, r.duration_secs % 60)
            };
            let path: El = if r.duration_secs == 0 {
                cell("\u{2014}".into(), 90.0, t.ink2).into()
            } else if r.direct {
                container(badge(t.primary_c, t.on_primary_c, None, "Direct")).width(90).into()
            } else {
                container(badge(t.relay_c, t.on_relay_c, None, "Relayed")).width(90).into()
            };
            hist = hist.push(
                row![
                    container(ui::icon(if r.incoming { Icon::Incoming } else { Icon::Outgoing }, 16.0, if bad { t.error } else { t.ink2 })).width(28),
                    container(semi(what, 14.0, fg)).width(Fill),
                    cell(ago(now_secs(), r.started_at), 150.0, t.ink2),
                    container(mono(len, 13.0, t.ink2)).width(80),
                    path,
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        hist = hist.push(tx("History stays on this computer only.", 12.0, t.ink2));

        let mut col = column![back, head, actions].spacing(20);
        if let Some(a) = allow_calls {
            col = col.push(container(a).padding(16).width(Fill).style(ui::card(t)));
        }
        if let Some(p) = panel {
            col = col.push(p);
        }
        col.push(container(hist).padding(20).width(Fill).style(ui::card(t))).into()
    }

    // ---- add contact ----

    // Add contact is the dialog in signup_view.rs (AddContact.svelte).

    // ---- settings ----

    fn native_settings_view(&self, t: Tok) -> El<'_> {
        let opts = |v: &Vec<String>| {
            let mut o = vec![DEFAULT_LABEL.to_string()];
            o.extend(v.iter().cloned());
            o
        };
        let sel = |v: &Option<String>| Some(v.clone().unwrap_or_else(|| DEFAULT_LABEL.to_string()));
        let pick = |opts_: Vec<String>, selected: Option<String>, on: fn(String) -> Msg| -> El<'_> {
            pick_list(opts_, selected, on)
                .width(280)
                .padding(9)
                .text_size(14)
                .style(ui::pick_style(t))
                .menu_style(ui::menu_style(t))
                .into()
        };

        let name_row: El = row![
                text_input("Your name", &self.name_edit)
                    .on_input(Msg::NameEdit)
                    .on_submit(Msg::SaveName)
                    .padding(11)
                    .size(15)
                    .style(ui::input_style(t)),
                pill(t, Kind::Tonal, None, "Save", Some(Msg::SaveName)),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into();
        let mut acct = column![
            line(t, "Your name", "Shown to the people you add", Space::new().into()),
            name_row,
            line(t, "This computer\u{2019}s name", "Only your own devices see it", Space::new().into()),
            row![
                text_input("Work desktop", &self.device_edit)
                    .on_input(Msg::DeviceEdit)
                    .on_submit(Msg::SaveDevice)
                    .padding(11)
                    .size(15)
                    .style(ui::input_style(t)),
                pill(t, Kind::Tonal, None, "Save", Some(Msg::SaveDevice)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(12);
        let others: Vec<_> = self.accounts.iter().filter(|a| !a.current).collect();
        if !others.is_empty() {
            acct = acct.push(label(t, "Other accounts on this computer"));
            for a in others {
                if self.remove_ask.as_deref() == Some(a.did.as_str()) {
                    acct = acct.push(
                        column![
                            tx(format!("Remove {} from this computer? Its contacts, chats and call history here are deleted. Your recovery phrase brings back the identity, not those.", a.name), 13.0, t.error),
                            row![
                                pill(t, Kind::Danger, Some(Icon::Trash), "Remove", Some(Msg::RemoveGo(a.did.clone()))),
                                pill(t, Kind::Ghost, None, "Keep", Some(Msg::RemoveCancel)),
                            ]
                            .spacing(8),
                        ]
                        .spacing(8),
                    );
                } else {
                    acct = acct.push(line(t,
                        &a.name,
                        if a.has_passphrase { "Locked \u{b7} needs its passphrase" } else { "Locked" },
                        pill(t, Kind::Danger, Some(Icon::Trash), "Remove", Some(Msg::RemoveAsk(a.did.clone()))),
                    ));
                }
            }
        }
        let profile = section(t, "Account", acct.into());
        let devices = self.linked_devices_page(t);
        let calls = section(t,
            "Calls",
            column![
                line(t,
                    "Available for calls",
                    if self.avail.available { "Your line is open" } else { "Calls won\u{2019}t ring until you turn it back on" },
                    toggler(self.avail.available).on_toggle(|on| Msg::SetAvail(on, None)).style(ui::toggle_style(t)).into(),
                ),
                line(t,
                    "Video in calls",
                    "Your camera stays off until you turn it on in the call",
                    toggler(!self.settings.no_video).on_toggle(|on| Msg::Video(super::Vm::Enabled(on))).style(ui::toggle_style(t)).into(),
                ),
            ]
            .spacing(12)
            .into(),
        );
        let audio = section(t,
            "Audio",
            column![
                line(t, "Microphone", "", pick(opts(&self.devices.0), sel(&self.settings.input), Msg::InDev)),
                line(t, "Speaker", "Device changes apply from the next call", pick(opts(&self.devices.1), sel(&self.settings.output), Msg::OutDev)),
                line(t,
                    "Send a test tone",
                    "Instead of the microphone, to check the connection",
                    toggler(self.settings.tone).on_toggle(Msg::ToneToggled).style(ui::toggle_style(t)).into(),
                ),
            ]
            .spacing(14)
            .into(),
        );
        let security: El = {
            let mut c = column![].spacing(12);
            if let Some(p) = &self.revealed {
                let words: Vec<&str> = p.split_whitespace().collect();
                let mut grid = column![].spacing(6);
                for (r, chunk) in words.chunks(4).enumerate() {
                    let mut ln = row![].spacing(8);
                    for (i, w) in chunk.iter().enumerate() {
                        ln = ln.push(
                            container(row![mono(format!("{:>2}", r * 4 + i + 1), 12.0, t.ink2), mono((*w).to_string(), 14.0, t.ink)].spacing(8))
                                .padding([8, 10])
                                .width(Fill)
                                .style(ui::plain(t.surface2, 8.0)),
                        );
                    }
                    grid = grid.push(ln);
                }
                c = c
                    .push(tx("Anyone who sees these words can become you. Check nobody is looking.", 13.0, t.error))
                    .push(grid)
                    .push(pill(t, Kind::Quiet, Some(Icon::Copy), "Copy recovery phrase", Some(Msg::CopyPhrase)))
                    .push(tx("Paste only into a trusted password manager; the clipboard contains your account key.", 13.0, t.ink2))
                    .push(pill(t, Kind::Quiet, None, "Hide", Some(Msg::HidePhrase)));
            } else if self.reveal_form {
                c = c
                    .push(if self.has_pass {
                        self.pass_field(t, "Enter your passphrase", &self.pass_in, Msg::PassIn, Some(Msg::RevealSubmit))
                    } else {
                        tx("Anyone who sees these words can become you. Check nobody is looking, then show them.", 13.0, t.error).into()
                    })
                    .push(
                        row![
                            pill(t, Kind::Primary, None, if self.busy { "Checking..." } else { "Show recovery phrase" }, (!self.busy).then_some(Msg::RevealSubmit)),
                            pill(t, Kind::Ghost, None, "Cancel", Some(Msg::ToggleReveal)),
                        ]
                        .spacing(8),
                    );
            } else {
                c = c.push(line(t,
                    "Recovery phrase",
                    if self.has_pass { "Needs your passphrase" } else { "Asks you to confirm first" },
                    pill(t, Kind::Quiet, Some(Icon::Key), "Show", Some(Msg::ToggleReveal)),
                ));
            }
            if self.change_form {
                if self.has_pass {
                    c = c.push(self.pass_field(t, "Current passphrase", &self.old_in, Msg::OldIn, None));
                }
                c = c
                    .push(self.new_pass_field(t, if self.has_pass { "New passphrase" } else { "Passphrase" }, Msg::ChangePassSubmit))
                    .push(
                        row![
                            pill(t, Kind::Primary, None, if self.busy { "Working..." } else if self.has_pass { "Change passphrase" } else { "Add passphrase" }, (!self.busy).then_some(Msg::ChangePassSubmit)),
                            pill(t, Kind::Ghost, None, "Cancel", Some(Msg::ToggleChange)),
                        ]
                        .spacing(8),
                    );
            } else {
                c = c.push(line(t,
                    "Passphrase",
                    if self.has_pass { "Unlocks Tinline when it starts" } else if self.file_key { "Recommended \u{2014} your key is in a plain file" } else { "Optional \u{2014} encrypts your account on this computer" },
                    pill(t, Kind::Quiet, Some(Icon::Lock), if self.has_pass { "Change" } else { "Add passphrase" }, Some(Msg::ToggleChange)),
                ));
                if !self.has_pass {
                    c = c.push(tx(self.key_home, 13.0, if self.file_key { t.error } else { t.ink2 }));
                    if self.file_key {
                        c = c.push(pill(t, Kind::Primary, Some(Icon::Lock), "Add a passphrase", Some(Msg::ToggleChange)));
                    }
                }
            }
            c.into()
        };
        // (tab, name) in nav order. Hosting lists the groups this computer hosts (the demo shows
        // the preview's placeholder page instead).
        let mut items: Vec<(u8, &str)> = vec![(0, "Account"), (1, "Linked devices"), (7, "Privacy"), (6, "Hosting")];
        items.extend([(2, "Calls & availability"), (3, "Audio devices"), (4, "Security")]);
        let mut nav = column![
            button(row![ui::icon(Icon::ArrowLeft, 18.0, t.ink2), semi("Back", 14.0, t.ink2)].spacing(8).align_y(Alignment::Center))
                .padding([8, 12])
                .style(ui::button_style(t, Kind::Ghost, 999.0))
                .on_press(Msg::Back),
            Space::new().height(8),
        ]
        .spacing(2);
        for (i, name) in items.iter().copied() {
            let on = self.settings_tab == i;
            nav = nav.push(
                button(semi(name, 14.0, if on { t.on_primary_c } else { t.ink }))
                    .padding([10, 14])
                    .width(Fill)
                    .style(ui::row_style(t, on))
                    .on_press(Msg::SettingsTab(i)),
            );
        }
        let nav = container(nav).padding(16).width(240).height(Fill).style(ui::sidebar(t));
        let page: El = match self.settings_tab {
            0 | 11 => profile,
            1 => devices,
            2 => calls,
            3 => audio,
            4 => section(t, "Security", security),
            #[cfg(any(test, feature = "test-hooks"))]
            6 if self.demo && self.groups.on => self.hosting_page(t),
            6 => self.live_hosting_page(t),
            7 => self.privacy_page(t),
            _ => Space::new().into(),
        };
        let title_ = items.iter().find(|(i, _)| *i == self.settings_tab).map(|(_, n)| *n).unwrap_or("Account");
        let _ = (nav, title_); // Native-only content stays reachable without a second settings sidebar.
        let content = column![self.notice_bar(t), page].spacing(20);
        scroll(t, container(container(content).max_width(560).width(Fill)).padding(16).center_x(Fill).width(Fill)).height(Fill).into()
    }

    // ---- calls ----

    /// `fill`: the middle takes all the height (the video stage) instead of floating centred.
    fn call_shell<'a>(&'a self, t: Tok, top: El<'a>, middle: El<'a>, bottom: El<'a>, fill: bool) -> El<'a> {
        let col = if fill {
            column![container(top).center_x(Fill), middle, bottom, self.notice_bar(t)]
        } else {
            column![
                container(top).center_x(Fill),
                Space::new().height(Fill),
                middle,
                Space::new().height(Fill),
                bottom,
                self.notice_bar(t),
            ]
        };
        container(col.spacing(16).align_x(Alignment::Center))
        .padding(if fill { 24 } else { 40 })
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .into()
    }

    fn round_btn<'a>(&self, t: Tok, kind: Kind, icon: Icon, text_: &str, on: Option<Msg>, active: bool) -> El<'a> {
        let fg = match kind {
            Kind::Accept | Kind::End => Color::WHITE,
            _ if active => t.bg,
            _ => t.ink,
        };
        let style = ui::button_style(t, if active { Kind::Primary } else { kind }, 999.0);
        // Lift: the round call buttons sit above the surface. The fill under the button is its own.
        let fill = match if active { Kind::Primary } else { kind } {
            Kind::Primary => t.ink,
            Kind::Accept => t.accept,
            Kind::End => t.end,
            _ if t.dark => Color { r: t.surface.r * 0.86 + t.ink.r * 0.14, g: t.surface.g * 0.86 + t.ink.g * 0.14, b: t.surface.b * 0.86 + t.ink.b * 0.14, a: 1.0 },
            _ => t.surface,
        };
        let enabled = on.is_some();
        let circle = button(container(ui::icon(icon, 28.0, fg)).center_x(64).center_y(64))
            .padding(0)
            .width(64)
            .height(64)
            .style(move |theme, status| button::Style { background: None, border: Border::default(), ..style(theme, status) })
            .on_press_maybe(on);
        let circle = crate::shaped::shaped(circle, [32.0; 4], fill);
        let circle = ui::raised(circle, t, if enabled { ui::Depth::Lift } else { ui::Depth::Flat }, ui::plain(Color::TRANSPARENT, 32.0));
        column![
            circle,
            container(tx(text_.to_string(), 13.0, t.ink)).height(18).center_x(76),
        ]
        .spacing(8)
        .width(76)
        .align_x(Alignment::Center)
        .into()
    }

    // Preserve the actual camera renderer while the voice screen is ported independently.
    fn video_call_view(&self, t: Tok) -> El<'_> {
        let c = self.call.as_ref().unwrap();
        let secs = c.stats.as_ref().map(|s| s.secs).unwrap_or(0);
        let incoming_ring = c.info.incoming && !c.answered && c.state == CallState::Ringing;
        let active = c.state == CallState::Active;
        let reconnecting = active && c.stats.as_ref().is_some_and(|s| s.reconnecting);

        let top: El = match (&c.state, &c.stats) {
            (CallState::Active, Some(st)) => {
                let (n, word) = quality(st.rtt_ms, self.call_ui.loss.pct());
                row![
                    if st.direct {
                        badge(t.primary_c, t.on_primary_c, Some(Icon::Direct), "Direct")
                    } else {
                        badge(t.relay_c, t.on_relay_c, Some(Icon::Relayed), "Relayed \u{b7} encrypted")
                    },
                    container(
                        row![ui::bars(n, t.ink2, ui::alpha(t.ink2, 0.3)), semi(word, 13.0, t.ink2)]
                            .spacing(6)
                            .align_y(Alignment::Center)
                    )
                    .padding([5, 12])
                    .style(ui::plain(t.surface2, 999.0)),
                ]
                .spacing(8)
                .into()
            }
            _ => Space::new().into(),
        };
        // The way back to the app (as on the voice screen); Esc does the same.
        let minimize = iced::widget::tooltip(
            button(container(ui::icon(Icon::ChevronDown, 24.0, t.ink)).center_x(44).center_y(44))
                .padding(0)
                .width(44)
                .height(44)
                .style(ui::button_style(t, Kind::Ghost, 999.0))
                .on_press_maybe(active.then_some(Msg::CallUi(crate::call_ui::Action::Minimize))),
            "Minimize call",
            iced::widget::tooltip::Position::Bottom,
        );
        let top: El = row![minimize, container(top).center_x(Fill), Space::new().width(44)]
            .align_y(Alignment::Center)
            .into();

        let status: El = match (&c.state, incoming_ring) {
            (_, true) => tx("is calling you", 18.0, t.ink2).into(),
            (CallState::Dialing, _) => tx("Calling\u{2026}", 18.0, t.ink2).into(),
            (CallState::Ringing, _) => tx("Ringing\u{2026}", 18.0, t.ink2).into(),
            (CallState::Active, _) if reconnecting => semi("Reconnecting\u{2026}", 18.0, t.thread_text).into(),
            (CallState::Active, _) => mono(format!("{:02}:{:02}", secs / 60, secs % 60), 22.0, t.ink).into(),
            (CallState::Ended { .. }, _) => tx("Call ended", 18.0, t.ink2).into(),
        };
        let note: El = if reconnecting {
            tx(
                "Your network changed. Hold on \u{2014} Tinline is finding the line again.",
                14.0,
                t.ink2,
            )
            .into()
        } else if incoming_ring || matches!(c.state, CallState::Dialing | CallState::Ringing) {
            Space::new().height(18).into()
        } else if let Some(st) = c.stats.as_ref().filter(|_| active) {
            if st.direct {
                row![ui::icon(Icon::Lock, 14.0, t.ink2), tx("End-to-end encrypted", 13.0, t.ink2)]
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .into()
            } else {
                container(tx(
                    "Relayed: a direct line wasn\u{2019}t possible on this network, so an encrypted relay is passing the call along. It can\u{2019}t hear you.",
                    13.0,
                    t.ink2,
                ))
                .max_width(460)
                .into()
            }
        } else {
            Space::new().into()
        };
        let live = active && self.video.live();
        let middle: El = if live {
            column![
                self.video_stage(t, &c.info),
                row![bold(c.info.peer_name.clone(), 20.0, t.ink), status].spacing(14).align_y(Alignment::Center),
            ]
            .spacing(12)
            .align_x(Alignment::Center)
            .height(Fill)
            .into()
        } else {
            column![
                self.profile_avatar(t, &c.info.peer_name, &c.info.peer_did, 128.0),
                bold(c.info.peer_name.clone(), 32.0, t.ink),
                status,
                note,
            ]
            .spacing(12)
            .align_x(Alignment::Center)
            .into()
        };

        let muted = self.ctl.muted.load(Ordering::Relaxed);
        let bottom: El = if incoming_ring {
            row![
                self.round_btn(t, Kind::End, Icon::PhoneOff, "Decline", Some(Msg::Decline), false),
                self.round_btn(t, Kind::Accept, Icon::Phone, "Answer", Some(Msg::Answer), false),
            ]
            .spacing(48)
            .into()
        } else {
            let mut r = row![self.round_btn(
                t,
                Kind::Quiet,
                if muted { Icon::MicOff } else { Icon::Mic },
                if muted { "Muted" } else { "Mute" },
                active.then_some(Msg::ToggleMute),
                muted
            )];
            if active && self.video.negotiated() {
                let on = self.video.local_on();
                let label = match (self.video.source.as_ref(), on) {
                    (Some(None), _) => "No camera",
                    (_, true) => "Camera on",
                    _ => "Camera",
                };
                r = r.push(self.round_btn(
                    t,
                    Kind::Quiet,
                    if on { Icon::Video } else { Icon::VideoOff },
                    label,
                    self.video.has_camera().then_some(Msg::Video(super::Vm::ToggleCamera)),
                    on,
                ));
            }
            r.push(self.round_btn(t, Kind::End, Icon::PhoneOff, if active { "Hang up" } else { "Cancel" }, Some(Msg::Hangup), false))
                .spacing(48)
                .into()
        };
        let hint = if incoming_ring {
            "Enter answers \u{b7} Esc declines"
        } else if active {
            "End-to-end encrypted \u{b7} Ctrl M mute \u{b7} Ctrl E end \u{b7} Esc minimizes"
        } else {
            "Ctrl E cancels"
        };
        let mut bottom_col = column![bottom].spacing(18).align_x(Alignment::Center);
        if active && !live {
            let (ins, outs) = &self.devices;
            let opts = |v: &Vec<String>| {
                let mut o = vec![DEFAULT_LABEL.to_string()];
                o.extend(v.iter().cloned());
                o
            };
            let pick = |title: &'static str, o: Vec<String>, sel: Option<String>, on: fn(String) -> Msg| -> El<'_> {
                column![
                    label(t, title),
                    pick_list(o, sel, on).width(250).padding(9).text_size(14).style(ui::pick_style(t)).menu_style(ui::menu_style(t)),
                ]
                .spacing(6)
                .into()
            };
            bottom_col = bottom_col.push(
                row![
                    pick("Microphone", opts(ins), Some(self.settings.input.clone().unwrap_or_else(|| DEFAULT_LABEL.into())), Msg::InDev),
                    pick("Speaker", opts(outs), Some(self.settings.output.clone().unwrap_or_else(|| DEFAULT_LABEL.into())), Msg::OutDev),
                ]
                .spacing(16),
            );
            bottom_col = bottom_col.push(tx("Device changes apply from the next call.", 12.0, t.ink2));
        }
        bottom_col = bottom_col.push(tx(hint, 12.0, t.ink2));
        let shell = self.call_shell(t, top, middle, bottom_col.into(), live);
        match &self.waiting {
            Some(w) => column![self.waiting_banner(t, w), shell].into(),
            None => shell,
        }
    }

    /// The peer's video large, ours mirrored in the corner; their avatar while their camera is off.
    fn video_stage<'a>(&'a self, t: Tok, peer: &p2pcore::CallInfo) -> El<'a> {
        let v = &self.video;
        let main: El = match (&v.remote, v.remote_on()) {
            (Some(h), true) => image(h.clone()).content_fit(ContentFit::Contain).width(Fill).height(Fill).into(),
            (_, on) => column![
                self.profile_avatar(t, &peer.peer_name, &peer.peer_did, 112.0),
                tx(if on { "Video is starting\u{2026}" } else { "Camera off" }, 15.0, t.ink2),
            ]
            .spacing(14)
            .align_x(Alignment::Center)
            .into(),
        };
        let mut st = stack![container(main).center_x(Fill).center_y(Fill).style(ui::plain(t.surface2, 16.0))];
        if v.local_on() {
            // The chip's short line borrows the picture label for 2.6 s; the label's box does not change.
            let label = v.hd_line().map(str::to_string).unwrap_or_else(|| {
                v.send_size.map_or_else(|| "Starting video\u{2026}".to_string(), |(w, h)| crate::camera::sending_label(w, h))
            });
            st = st.push(container(tx(label, 13.0, t.ink)).padding(14));
            // My picture, with the HD chip on its top-start corner (it is about what I send).
            let hd = v.hd_on(self.settings.prefer_hd);
            let paused = crate::camera::hd_paused(hd, v.send_size);
            let tile: El = match v.local.as_ref() {
                Some(h) => image(h.clone()).width(220).border_radius(10.0).into(),
                None => container(Space::new()).width(220).height(124).style(ui::plain(t.surface2, 10.0)).into(),
            };
            let tile = stack![tile, self.hd_chip(t, hd, paused)];
            st = st.push(container(tile).align_right(Fill).align_bottom(Fill).padding(14));
        }
        st.width(Fill).height(Fill).into()
    }

    /// The HD chip (playground HdChip.svelte): "HD", 24 px tall, radius 7, 1.5 px outline. Off = outline, on = filled
    /// (white over video), paused = on but dimmed to 55%. The 48 px square around it is the touch target.
    fn hd_chip<'a>(&'a self, t: Tok, on: bool, paused: bool) -> El<'a> {
        let _ = t;
        let a = if paused { 0.55 } else { 1.0 };
        let (fg, fill) = if on { (Color::from_rgb(0.07, 0.07, 0.07), Color::WHITE) } else { (Color::WHITE, Color::TRANSPARENT) };
        let (fg, fill) = (ui::alpha(fg, a), ui::alpha(fill, if on { a } else { 0.0 }));
        let pill = container(bold("HD", 12.0, fg)).center_y(24).padding([0, 7]).style(move |_: &Theme| container::Style {
            background: Some(fill.into()),
            border: Border { color: ui::alpha(if on { Color::WHITE } else { fg }, a), width: 1.5, radius: 7.0.into() },
            ..Default::default()
        });
        let label = if paused { "Sharper video on, paused while the connection is weak" } else if on { "Sharper video on, up to 720p" } else { "Sharper video off" };
        let b = button(container(pill).center_x(48).center_y(48)).padding(0).width(48).height(48)
            .style(|_: &Theme, _| button::Style::default()).on_press(Msg::Video(super::Vm::ToggleHd));
        iced::widget::tooltip(b, label, iced::widget::tooltip::Position::Top).into()
    }

    /// WaitingCard.svelte: someone else is calling during my call (core: Live::waiting). A calm card just above the
    /// controls: avatar 40, "<name> is calling" over "during your call", Decline and End & answer as two equal 44 px
    /// pills, and what End & answer does. No hold: End & answer ends this call.
    fn waiting_banner<'a>(&'a self, t: Tok, w: &p2pcore::CallInfo) -> El<'a> {
        let name = if w.peer_name.is_empty() { "Unknown".to_string() } else { w.peer_name.clone() };
        let current = self.call.as_ref().map(|c| c.info.peer_name.split_whitespace().next().unwrap_or("").to_string()).unwrap_or_default();
        let pill_style = |bg: Color, hover: Color, fg: Color| move |_: &Theme, st: button::Status| button::Style {
            background: Some(if matches!(st, button::Status::Hovered | button::Status::Pressed) { hover } else { bg }.into()),
            text_color: fg,
            border: Border { radius: 22.0.into(), ..Default::default() },
            ..Default::default()
        };
        let dec_bg = ui::mix(t.surface, t.ink, 0.09);
        let dec = button(container(semi("Decline", 13.0, t.ink)).center_x(Fill).center_y(44)).padding(0).width(Fill).height(44)
            .style(pill_style(dec_bg, ui::mix(t.surface, t.ink, 0.15), t.ink)).on_press(Msg::DeclineWaiting);
        let ans = button(container(semi("End & answer", 13.0, t.on_accent)).center_x(Fill).center_y(44)).padding(0).width(Fill).height(44)
            .style(pill_style(t.accent, ui::mix(t.accent, Color::WHITE, 0.08), t.on_accent)).on_press(Msg::EndAnswer);
        let who = row![
            self.profile_avatar(t, &name, &w.peer_did, 40.0),
            column![
                container(semi(format!("{name} is calling"), 15.0, t.ink).line_height(lh(20.0)).wrapping(iced::widget::text::Wrapping::None)).clip(true).width(Fill),
                tx("during your call", 13.0, t.ink2).line_height(lh(18.0)),
            ].width(Fill),
        ].spacing(12).align_y(Alignment::Center);
        let hint = container(row![
            tx("Ends your call with", 12.0, t.ink2).line_height(lh(16.0)).wrapping(iced::widget::text::Wrapping::None),
            container(tx(current, 12.0, t.ink2).line_height(lh(16.0)).wrapping(iced::widget::text::Wrapping::None)).clip(true),
        ].spacing(4)).center_x(Fill);
        let card = container(column![who, row![dec, ans].spacing(8), hint].spacing(12))
            .padding(Padding { top: 14.0, right: 14.0, bottom: 12.0, left: 14.0 }).width(Fill).max_width(360);
        container(ui::raised(card, t, ui::Depth::Pop, ui::pop_surface(t, 22.0))).center_x(Fill).into()
    }

    /// RingHead.svelte (desktop), waiting form: while my call is minimised, the brand row turns into whoever is calling
    /// during it ("Calling during your call"); Decline refuses, End & answer swaps. Same box as the incoming form.
    pub(super) fn ring_head_waiting(&self, t: Tok) -> Option<El<'_>> {
        let w = self.waiting.as_ref().filter(|_| self.call_ui.minimized)?;
        Some(self.ring_row(t, &w.peer_name, &w.peer_did, "Calling during your call", Msg::CallUi(crate::call_ui::Action::Restore), Msg::DeclineWaiting, Msg::EndAnswer))
    }

    /// CallScreen.svelte after the end: the same screen (mark, avatar, name), the reason where the status was and the
    /// call's length under it, for a moment; then it goes (see `EndedHoldDone`). No buttons: the record is in the chat.
    fn ended_view(&self, t: Tok) -> El<'_> {
        let e = self.ended.as_ref().unwrap();
        iced::widget::responsive(move |size| {
            let g = crate::call_ui::Geometry::new(size.height);
            let head: El = container(container(ui::logo(28.0, t)).center_x(Fill)).height(48).padding([0, 20]).into();
            let who: El = column![
                self.profile_avatar(t, &e.name, &e.did, 128.0),
                Space::new().height(22),
                container(semi(e.name.clone(), 28.0, t.ink).wrapping(iced::widget::text::Wrapping::None)).height(34).width(Fill).center_x(Fill).clip(true),
                Space::new().height(6),
                container(semi(e.text.clone(), 17.0, t.ink)).height(30).center_x(Fill).center_y(30),
                Space::new().height(10),
                container(tx(if e.secs > 0 { format!("{:02}:{:02}", e.secs / 60, e.secs % 60) } else { String::new() }, 13.0, t.ink)).height(18).center_x(Fill),
            ].align_x(Alignment::Center).width(Fill).into();
            stack![Space::new().width(Fill).height(Fill),
                container(head).padding(Padding { top: 8.0, ..Padding::ZERO }).width(Fill),
                container(container(who).padding([0, 20])).padding(Padding { top: g.avatar_top.max(0.0), ..Padding::ZERO }).width(Fill)]
                .width(Fill).height(Fill).into()
        }).into()
    }
}

fn section<'a>(t: Tok, title: &str, body: El<'a>) -> El<'a> {
    column![label(t, title), container(body).padding(20).width(Fill).style(ui::card(t))].spacing(8).into()
}

fn line<'a>(t: Tok, title: &str, sub: &str, right: El<'a>) -> El<'a> {
    row![
        column![semi(title.to_string(), 15.0, t.ink), tx(sub.to_string(), 13.0, t.ink2)].spacing(2).width(Fill),
        right
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

/// "12 Sep" in UTC (no time-zone database here; the day can be off by one near midnight).
fn utc_date(secs: u64) -> String {
    let z = (secs / 86400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let _ = era;
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{day} {}", M[(month - 1) as usize])
}

pub(super) fn qr_of(t: &str) -> Option<Qr> {
    let code = qrcode::QrCode::new(t.as_bytes()).ok()?;
    Some(Qr {
        n: code.width(),
        dark: code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect(),
    })
}

struct QrView<'a>(&'a Qr);

impl canvas::Program<Msg> for QrView<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::WHITE);
        let quiet = 1usize;
        let total = self.0.n + 2 * quiet;
        // The code fills its box: cells are fractional, and each edge is rounded to a whole pixel so neighbours meet with
        // no seams. (Flooring the cell size left a dense link ticket at ~2/3 of the box.)
        let side = bounds.width.min(bounds.height);
        let cell = side / total as f32;
        let edge = |i: usize| (i as f32 * cell).round();
        for y in 0..self.0.n {
            for x in 0..self.0.n {
                if self.0.dark[y * self.0.n + x] {
                    let (x0, y0) = (edge(x + quiet), edge(y + quiet));
                    frame.fill_rectangle(Point::new(x0, y0), Size::new(edge(x + quiet + 1) - x0, edge(y + quiet + 1) - y0), Color::BLACK);
                }
            }
        }
        vec![frame.into_geometry()]
    }
}
