//! Screens. Everything here is a pure function of `App`; styling comes from `ui`.

use std::sync::atomic::Ordering;

use iced::widget::{
    button, canvas, checkbox, column, container, pick_list, row, text, text_input, toggler, Space,
};
use iced::{Alignment, Border, Color, Element, Fill, Length, Padding, Point, Rectangle, Renderer, Size, Theme};
use p2pcore::{CallRecord, CallState, LockState};

use super::{
    AddPhase, App, Detail, Msg, Qr, Screen, DEFAULT_LABEL, RENAME_ID, SEARCH_ID,
};
use crate::ui::{self, Icon, Kind, Tok};

type El<'a> = Element<'a, Msg>;

fn scroll<'a>(t: Tok, c: impl Into<El<'a>>) -> iced::widget::Scrollable<'a, Msg> {
    iced::widget::scrollable(c).style(ui::scroll_style(t))
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

fn avatar<'a>(t: Tok, name: &str, key: &str, size: f32) -> El<'a> {
    let (bg, fg) = ui::avatar_colors(t, key);
    container(semi(ui::initials(name), size * 0.38, fg))
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

/// A square icon-only button.
fn icon_btn<'a>(t: Tok, i: Icon, on: Msg) -> El<'a> {
    button(container(ui::icon(i, 20.0, t.ink2)).center_x(36).center_y(36))
        .padding(0)
        .width(36)
        .height(36)
        .style(ui::button_style(t, Kind::Ghost, 999.0))
        .on_press(on)
        .into()
}

fn field<'a>(t: Tok, title: &str, input: text_input::TextInput<'a, Msg>) -> El<'a> {
    column![semi(title.to_string(), 13.0, t.ink2), input.padding(11).size(15).style(ui::input_style(t))]
        .spacing(6)
        .into()
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

/// 0-4 bars from round-trip time and loss.
fn quality(rtt_ms: u32, loss_pct: f64) -> (u8, &'static str) {
    if rtt_ms < 100 && loss_pct < 1.0 {
        (4, "Excellent")
    } else if rtt_ms < 200 && loss_pct < 3.0 {
        (3, "Good")
    } else if rtt_ms < 350 && loss_pct < 8.0 {
        (2, "OK")
    } else {
        (1, "Poor")
    }
}

/// Banner content: title, body, optional action, is-error.
type Problem = (&'static str, &'static str, Option<(&'static str, Msg)>, bool);

impl App {
    pub(super) fn t(&self) -> Tok {
        ui::tok(self.dark)
    }

    pub(super) fn view(&self, _id: iced::window::Id) -> El<'_> {
        let t = self.t();
        let body: El = if self.call.is_some() {
            row![self.sidebar(t), self.call_view(t)].into()
        } else if self.ended.is_some() {
            row![self.sidebar(t), self.ended_view(t)].into()
        } else {
            match self.screen {
                Screen::Onboarding => self.onboarding_view(t),
                Screen::Unlock => self.unlock_view(t),
                Screen::SetPass => self.setpass_view(t),
                Screen::Phrase => self.phrase_view(t),
                Screen::Home => self.home_view(t),
                Screen::AddContact => self.add_view(t),
                Screen::Settings => self.settings_view(t),
            }
        };
        container(body).width(Fill).height(Fill).style(ui::plain(t.bg, 0.0)).into()
    }

    // ---- shared pieces ----

    fn notice_bar(&self, t: Tok) -> El<'_> {
        match &self.notice {
            Some(n) => container(tx(n.clone(), 14.0, t.on_primary_c))
                .padding([10, 14])
                .width(Fill)
                .style(ui::plain(t.primary_c, 10.0))
                .into(),
            None => Space::new().into(),
        }
    }

    /// A centred card on the background, for the screens before the home screen.
    fn shell<'a>(&'a self, t: Tok, title: &str, sub: &str, content: El<'a>) -> El<'a> {
        let head = column![
            ui::logo(48.0, t.primary, t.thread),
            bold(title.to_string(), 26.0, t.ink),
            tx(sub.to_string(), 15.0, t.ink2),
        ]
        .spacing(8)
        .align_x(Alignment::Start);
        let card = container(column![head, content].spacing(20))
            .padding(32)
            .width(460)
            .style(ui::card(t));
        scroll(t, container(card).center_x(Fill).padding([32, 0]).width(Fill)).height(Fill).into()
    }

    fn pass_field<'a>(&'a self, t: Tok, title: &'a str, value: &'a str, on: fn(String) -> Msg, submit: Option<Msg>) -> El<'a> {
        let mut input = text_input("", value).secure(true).on_input(on);
        if let Some(m) = submit {
            input = input.on_submit(m);
        }
        field(t, title, input)
    }

    fn new_pass_fields(&self, t: Tok, submit: Msg) -> El<'_> {
        column![
            self.pass_field(t, "Passphrase (at least 8 characters)", &self.pass_in, Msg::PassIn, None),
            self.pass_field(t, "Repeat passphrase", &self.pass2_in, Msg::Pass2In, Some(submit)),
        ]
        .spacing(12)
        .into()
    }

    fn wide<'a>(&self, label_: &str, kind: Kind, t: Tok, on: Option<Msg>) -> El<'a> {
        let fg = match kind {
            Kind::Primary => t.on_primary,
            _ => t.ink,
        };
        button(container(semi(label_.to_string(), 15.0, fg)).center_x(Fill))
            .padding([13, 22])
            .width(Fill)
            .style(ui::button_style(t, kind, 999.0))
            .on_press_maybe(on)
            .into()
    }

    // ---- before home ----

    fn onboarding_view(&self, t: Tok) -> El<'_> {
        let mut col = column![field(
            t,
            "Your name",
            text_input("e.g. Maya", &self.name_in).on_input(Msg::NameChanged)
        )]
        .spacing(12);
        if self.restore {
            col = col
                .push(field(
                    t,
                    "Recovery phrase (24 words)",
                    text_input("word word word ...", &self.phrase_in)
                        .secure(!self.show_phrase_in)
                        .on_input(Msg::PhraseChanged),
                ))
                .push(checkbox(self.show_phrase_in).label("Show the words").on_toggle(|_| Msg::TogglePhraseShow));
            if self.replace {
                col = col.push(tx(
                    "This replaces the locked identity on this computer. Its encrypted file is kept aside, not deleted.",
                    13.0,
                    t.error,
                ));
            }
        }
        col = col
            .push(self.new_pass_fields(t, if self.restore { Msg::Restore } else { Msg::Create }))
            .push(tx(
                "This passphrase locks your recovery phrase and keys on this computer. You type it each time Tinline starts. It cannot be recovered: if you forget it, you can only restore with your recovery phrase.",
                13.0,
                t.ink2,
            ));
        if self.restore {
            col = col
                .push(self.wide("Restore", Kind::Primary, t, (!self.busy).then_some(Msg::Restore)))
                .push(self.wide(
                    if self.replace { "Back to unlock" } else { "Create a new identity instead" },
                    Kind::Ghost,
                    t,
                    Some(if self.replace { Msg::BackToUnlock } else { Msg::ToggleRestore }),
                ));
        } else {
            col = col
                .push(self.wide("Create identity", Kind::Primary, t, (!self.busy).then_some(Msg::Create)))
                .push(self.wide("Restore from recovery phrase", Kind::Ghost, t, Some(Msg::ToggleRestore)));
        }
        col = col.push(self.notice_bar(t));
        self.shell(
            t,
            "Welcome to Tinline",
            "A direct line between two people. No phone number, no ads, no tracking.",
            col.into(),
        )
    }

    fn unlock_view(&self, t: Tok) -> El<'_> {
        let name = self.profile_name.clone();
        let mut col = column![
            self.pass_field(t, "Passphrase", &self.pass_in, Msg::PassIn, Some(Msg::Unlock)),
        ]
        .spacing(12);
        if let Some(n) = &self.notice {
            col = col.push(tx(n.clone(), 13.0, t.error));
        }
        col = col
            .push(self.wide(
                if self.busy { "Unlocking..." } else { "Unlock" },
                Kind::Primary,
                t,
                (!self.busy).then_some(Msg::Unlock),
            ))
            .push(tx(
                if self.busy { "Checking takes a second \u{2014} it\u{2019}s deliberately slow to stop guessing." } else { "" },
                12.0,
                t.ink2,
            ))
            .push(semi("Forgot it?", 14.0, t.ink))
            .push(tx(
                "Your recovery phrase can restore this account and set a new passphrase.",
                13.0,
                t.ink2,
            ))
            .push(self.wide("Restore with recovery phrase", Kind::Quiet, t, (!self.busy).then_some(Msg::GoRestore)));
        let title = if name.is_empty() { "Welcome back".to_string() } else { format!("Welcome back, {name}") };
        self.shell(t, &title, "Enter your passphrase to open Tinline. Calls can\u{2019}t reach you until you do.", col.into())
    }

    fn setpass_view(&self, t: Tok) -> El<'_> {
        let col = column![
            self.new_pass_fields(t, Msg::SetPassSubmit),
            tx(
                "You will type it each time Tinline starts. It cannot be recovered; your recovery phrase can still restore your identity if you forget it.",
                13.0,
                t.ink2
            ),
            self.wide(
                if self.busy { "Working..." } else { "Set passphrase" },
                Kind::Primary,
                t,
                (!self.busy).then_some(Msg::SetPassSubmit)
            ),
            self.wide("Not now", Kind::Ghost, t, (!self.busy).then_some(Msg::SkipSetPass)),
            self.notice_bar(t),
        ]
        .spacing(12);
        self.shell(
            t,
            "Set a passphrase",
            "Your recovery phrase and keys are stored on this computer without protection. Choose a passphrase to encrypt them.",
            col.into(),
        )
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
            return Some(("You\u{2019}re offline", "Calls can\u{2019}t reach you until you\u{2019}re back on the internet.", None, true));
        }
        if !self.avail.available {
            return Some((
                "You\u{2019}re not available",
                "Calls won\u{2019}t ring until you turn it back on.",
                Some(("Turn on", Msg::SetAvail(true, None))),
                false,
            ));
        }
        if self.lock == LockState::NeedsPassphrase {
            return Some((
                "Set a passphrase",
                "Protects your account if someone gets your computer.",
                Some(("Set", Msg::GoSetPass)),
                false,
            ));
        }
        None
    }

    /// "Available for calls  Change" at the foot of the sidebar; Change opens the availability panel.
    fn avail_footer(&self, t: Tok) -> El<'_> {
        let (txt, dot) = if !self.status.started {
            ("Connecting\u{2026}", t.ink2)
        } else if !self.status.online {
            ("Offline", t.error)
        } else if !self.avail.available {
            ("Not available", t.thread)
        } else {
            ("Available for calls", t.primary)
        };
        let bar = row![
            container(Space::new()).width(8).height(8).style(ui::plain(dot, 4.0)),
            semi(txt, 14.0, t.ink),
            Space::new().width(Fill),
            button(semi("Change", 13.0, t.primary))
                .padding([4, 10])
                .style(ui::button_style(t, Kind::Ghost, 999.0))
                .on_press(Msg::ToggleAvail),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let mut c = column![].spacing(10);
        if self.avail_open {
            c = c.push(self.avail_panel(t));
        }
        c.push(container(bar).padding([4, 4])).into()
    }

    fn avail_panel(&self, t: Tok) -> El<'_> {
        let opt = |s: &'static str, m: Msg| {
            button(row![semi(s, 14.0, t.ink), Space::new().width(Fill), ui::icon(Icon::ChevronRight, 16.0, t.ink2)])
                .padding([9, 12])
                .width(Fill)
                .style(ui::row_style(t, false))
                .on_press(m)
        };
        let mut col = column![
            row![
                column![
                    semi("Available for calls", 15.0, t.ink),
                    tx("Your line stays open while Tinline runs.", 12.0, t.ink2)
                ]
                .spacing(2)
                .width(Fill),
                toggler(self.avail.available).on_toggle(|on| Msg::SetAvail(on, None)).style(ui::toggle_style(t)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(10);
        if self.avail.available {
            col = col
                .push(label(t, "Take a break"))
                .push(opt("For 1 hour", Msg::SetAvail(false, Some(1))))
                .push(opt("For 8 hours", Msg::SetAvail(false, Some(8))))
                .push(opt("Until I turn it back on", Msg::SetAvail(false, None)))
                .push(tx(
                    "While you\u{2019}re away, people who call see \u{201c}Couldn\u{2019}t reach you\u{201d}. Nothing is queued and nobody is told why.",
                    12.0,
                    t.ink2,
                ));
        } else if let Some(u) = self.avail.until {
            col = col.push(tx(format!("Back on in {}.", until_text(u)), 12.0, t.ink2));
        }
        container(col).padding(14).width(Fill).style(ui::outlined(t.surface2, t.line, 12.0)).into()
    }

    fn contact_sub(&self, did: &str) -> String {
        match self.recents.iter().find(|r| r.peer_did == did) {
            Some(r) => format!("Last call {}", ago(now_secs(), r.started_at)),
            None => "Not called yet".into(),
        }
    }

    fn record_row(&self, t: Tok, r: &CallRecord, with_name: bool) -> El<'_> {
        let missed = r.missed;
        let dim = if missed || r.reason == "unreachable" { t.error } else { t.ink2 };
        let name = self.name_of(&r.peer_did, &r.peer_name);
        let what = crate::reason::history(&r.reason, r.incoming, r.missed, r.duration_secs, &name);
        let when = ago(now_secs(), r.started_at);
        let mut left = column![].spacing(2);
        if with_name {
            left = left.push(semi(name, 15.0, if missed { t.error } else { t.ink }));
            let short = if r.missed { "Missed".to_string() } else if r.duration_secs == 0 { what.clone() } else if r.incoming { "Incoming".into() } else { "Outgoing".into() };
            left = left.push(tx(format!("{short} \u{b7} {when}"), 13.0, dim));
        } else {
            left = left.push(semi(what, 14.0, if missed { t.error } else { t.ink }));
            left = left.push(tx(when, 12.0, t.ink2));
        }
        let mut r_ = row![
            ui::icon(if r.incoming { Icon::Incoming } else { Icon::Outgoing }, 18.0, dim),
            left.width(Fill)
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        if r.duration_secs > 0 && !with_name {
            r_ = r_.push(if r.direct {
                badge(t.primary_c, t.on_primary_c, None, "Direct")
            } else {
                badge(t.relay_c, t.on_relay_c, None, "Relayed")
            });
        }
        r_.into()
    }

    /// Our name for a person: the contact's alias or name when we still have them, else the
    /// name recorded with the call.
    fn name_of(&self, did: &str, fallback: &str) -> String {
        self.contacts.iter().find(|c| c.did == did).map(Self::display).unwrap_or_else(|| fallback.to_string())
    }

    fn sidebar(&self, t: Tok) -> El<'_> {
        let head = row![
            ui::logo(28.0, t.primary, t.thread),
            bold("Tinline", 20.0, t.ink),
            Space::new().width(Fill),
            icon_btn(t, Icon::UserPlus, Msg::OpenAdd),
            icon_btn(t, Icon::Sliders, Msg::OpenSettings),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        let mut col = column![head].spacing(12);
        let bare = move |_: &Theme, _: text_input::Status| text_input::Style {
            background: iced::Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            icon: t.ink2,
            placeholder: ui::alpha(t.ink2, 0.8),
            value: t.ink,
            selection: ui::alpha(t.primary, 0.3),
        };
        col = col.push(
            container(
                row![
                    text_input("Search", &self.search)
                        .id(SEARCH_ID)
                        .on_input(Msg::SearchIn)
                        .padding([9, 4])
                        .size(14)
                        .style(bare),
                    mono("Ctrl K", 11.0, t.ink2),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([0, 12])
            .style(ui::outlined(t.surface, t.line, 8.0)),
        );

        let q = self.search.trim().to_lowercase();
        let mut list = column![].spacing(2);
        let known = |did: &str| self.contacts.iter().any(|c| c.did == did);
        let recents: Vec<&CallRecord> = self
            .recents
            .iter()
            .filter(|r| known(&r.peer_did))
            .filter(|_| q.is_empty())
            .take(3)
            .collect();
        if !recents.is_empty() {
            list = list.push(container(label(t, "Recent")).padding(Padding { top: 8.0, bottom: 4.0, left: 12.0, right: 0.0 }));
            for r in recents {
                let name = self.name_of(&r.peer_did, &r.peer_name);
                list = list.push(
                    button(
                        row![avatar(t, &name, &r.peer_did, 36.0), self.record_row(t, r, true)]
                            .spacing(12)
                            .align_y(Alignment::Center),
                    )
                    .padding([8, 12])
                    .width(Fill)
                    .style(ui::row_style(t, false))
                    .on_press(Msg::Select(r.peer_did.clone())),
                );
            }
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
        container(
            column![col, scroll(t, list).height(Fill), self.avail_footer(t)]
                .spacing(12),
        )
        .padding(16)
        .width(300)
        .height(Fill)
        .style(ui::sidebar(t))
        .into()
    }

    fn home_view(&self, t: Tok) -> El<'_> {
        let mut main = column![].spacing(16);
        if let Some((title, body, action, error)) = self.problem() {
            main = main.push(banner(t, title, body, action, error));
        }
        if self.notice.is_some() {
            main = main.push(self.notice_bar(t));
        }
        main = main.push(match self.selected() {
            Some(c) => self.detail_view(t, c),
            None => self.code_view(t),
        });
        row![
            self.sidebar(t),
            scroll(t, container(main).padding(32).max_width(760).width(Fill)).width(Fill).height(Fill)
        ]
        .into()
    }

    /// My code: shown when no contact is selected, and as the empty state.
    fn code_view(&self, t: Tok) -> El<'_> {
        let qr: El = match &self.qr {
            Some(q) => container(canvas(QrView(q)).width(Length::Fixed(240.0)).height(Length::Fixed(240.0)))
                .padding(10)
                .style(ui::plain(Color::WHITE, 12.0))
                .into(),
            None => container(tx("Preparing your code\u{2026}", 14.0, t.ink2)).width(260).height(260).center_x(260).center_y(260).into(),
        };
        let head: El = if self.contacts.is_empty() {
            column![
                bold("Your line is ready", 28.0, t.ink),
                tx(
                    "Add the first person you want to call. You\u{2019}ll both need Tinline open for a moment \u{2014} side by side, or over a video call.",
                    15.0,
                    t.ink2
                ),
            ]
            .spacing(8)
            .into()
        } else {
            column![
                bold("My code", 28.0, t.ink),
                tx("Select a contact on the left to call them, or show this code to add someone.", 15.0, t.ink2),
            ]
            .spacing(8)
            .into()
        };
        let card = container(
            row![
                qr,
                column![
                    semi(self.profile_name.clone(), 18.0, t.ink),
                    tx("Works once. Share it with someone who should be able to call you.", 14.0, t.ink2),
                    row![
                        pill(t, Kind::Primary, Some(Icon::UserPlus), if self.contacts.is_empty() { "Add your first contact" } else { "Add contact" }, Some(Msg::OpenAdd)),
                        pill(t, Kind::Quiet, Some(Icon::Copy), "Copy code", self.ticket.as_ref().map(|_| Msg::CopyTicket)),
                    ]
                    .spacing(10),
                    tx("Waiting for them to add it \u{2014} keep Tinline open.", 13.0, t.ink2),
                ]
                .spacing(14)
                .width(Fill),
            ]
            .spacing(28)
            .align_y(Alignment::Center),
        )
        .padding(28)
        .style(ui::card(t));
        column![head, card].spacing(24).into()
    }

    fn detail_view<'a>(&'a self, t: Tok, c: &'a p2pcore::Contact) -> El<'a> {
        let name = Self::display(c);
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
        let head = row![avatar(t, &name, &c.did, 72.0), column![title, badges].spacing(10).width(Fill)]
            .spacing(20)
            .align_y(Alignment::Center);
        let actions = row![
            pill(t, Kind::Primary, Some(Icon::Phone), "Call", Some(Msg::CallPressed(c.did.clone()))),
            pill(t, Kind::Quiet, Some(Icon::Pencil), "Rename", Some(Msg::SetDetail(Detail::Rename))),
            pill(t, Kind::Quiet, Some(Icon::ShieldCheck), "Safety number", Some(Msg::SetDetail(Detail::Verify))),
            pill(t, Kind::Danger, Some(Icon::Trash), "Remove", Some(Msg::SetDetail(Detail::ConfirmRemove))),
        ]
        .spacing(10);

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
                                format!("If they don\u{2019}t match, remove {name} and add them again face to face."),
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
            Detail::ConfirmRemove => Some(
                container(
                    column![
                        semi(format!("Remove {name}?"), 17.0, t.ink),
                        tx(
                            "They won\u{2019}t be able to call you, and you won\u{2019}t be able to call them. Your call history with them is deleted from this computer. To talk again, you\u{2019}ll need to add each other again.",
                            14.0,
                            t.ink2
                        ),
                        row![
                            Space::new().width(Fill),
                            pill(t, Kind::Ghost, None, "Cancel", Some(Msg::SetDetail(Detail::View))),
                            pill(t, Kind::End, None, "Remove", Some(Msg::Remove(c.did.clone()))),
                        ]
                        .spacing(8),
                    ]
                    .spacing(12),
                )
                .padding(24)
                .style(ui::card(t))
                .into(),
            ),
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

        let mut col = column![head, actions].spacing(20);
        if let Some(p) = panel {
            col = col.push(p);
        }
        col.push(container(hist).padding(20).width(Fill).style(ui::card(t))).into()
    }

    // ---- add contact ----

    /// Add contact: a card over the dimmed window, our code and their card side by side.
    fn add_view(&self, t: Tok) -> El<'_> {
        let head = row![
            bold("Add a contact", 22.0, t.ink),
            Space::new().width(Fill),
            icon_btn(t, Icon::X, Msg::Back),
        ]
        .align_y(Alignment::Center);

        let body: El = match &self.add_phase {
            AddPhase::Connecting => column![
                bold("Connecting\u{2026}", 20.0, t.ink),
                tx("Exchanging keys directly between your computers. Keep Tinline open on both.", 14.0, t.ink2),
            ]
            .spacing(8)
            .into(),
            AddPhase::Added(c) => {
                let shown = c.name.clone();
                column![
                    row![avatar(t, &shown, &c.did, 56.0), bold(format!("{shown} is added"), 22.0, t.ink)]
                        .spacing(14)
                        .align_y(Alignment::Center),
                    tx(format!("You can now call each other. {shown}\u{2019}s computer shows the same thing."), 14.0, t.ink2),
                    field(
                        t,
                        "Save as",
                        text_input("Name", &self.add_alias).on_input(Msg::AddAlias).on_submit(Msg::SaveAlias)
                    ),
                    tx(format!("Only you see this name. They called themselves \u{201c}{shown}\u{201d}."), 13.0, t.ink2),
                    pill(t, Kind::Primary, None, "Done", Some(Msg::SaveAlias)),
                ]
                .spacing(12)
                .into()
            }
            AddPhase::Failed(e) => column![
                bold("Couldn\u{2019}t add", 20.0, t.ink),
                tx(
                    "Both computers need Tinline open and online at the same time. Ask them to keep their code on screen, then try again.",
                    14.0,
                    t.ink2
                ),
                semi("Other reasons", 13.0, t.ink2),
                tx("\u{b7} The code was already used \u{2014} ask for a new one.", 13.0, t.ink2),
                tx("\u{b7} One of you is offline or on a network that blocks it.", 13.0, t.ink2),
                mono(e.clone(), 12.0, t.ink2),
                pill(t, Kind::Primary, None, "Try again", Some(Msg::OpenAdd)),
            ]
            .spacing(8)
            .into(),
            AddPhase::Idle => {
                let qr: El = match &self.qr {
                    Some(q) => container(canvas(QrView(q)).width(Length::Fixed(200.0)).height(Length::Fixed(200.0)))
                        .padding(8)
                        .style(ui::plain(Color::WHITE, 12.0))
                        .into(),
                    None => container(tx("Preparing your code\u{2026}", 14.0, t.ink2)).width(216).height(216).center_x(216).center_y(216).into(),
                };
                let mine = column![
                    label(t, "They scan your code"),
                    qr,
                    tx("Works once. Share it with someone who should be able to call you.", 13.0, t.ink2),
                    pill(t, Kind::Quiet, Some(Icon::Copy), "Copy card", self.ticket.as_ref().map(|_| Msg::CopyTicket)),
                ]
                .spacing(12)
                .width(Fill);
                let theirs = column![
                    label(t, "Or add theirs"),
                    text_input("Paste their card (OSVC2:\u{2026})", &self.add_in)
                        .on_input(Msg::AddChanged)
                        .on_submit(Msg::AddPressed)
                        .padding(12)
                        .size(14)
                        .font(ui::MONO)
                        .style(ui::input_style(t)),
                    pill(t, Kind::Primary, None, "Add contact", (!self.add_in.trim().is_empty()).then_some(Msg::AddPressed)),
                    tx(
                        "Tip: a card is safest sent over an app you already trust. Anyone who gets it first could use it instead.",
                        13.0,
                        t.ink2
                    ),
                ]
                .spacing(12)
                .width(Fill);
                row![mine, container(Space::new()).width(1).height(Fill).style(ui::plain(t.line, 0.0)), theirs]
                    .spacing(28)
                    .height(Length::Shrink)
                    .into()
            }
        };
        let card = container(
            column![head, body, tx("You both need Tinline open and online while adding.", 12.0, t.ink2)].spacing(18),
        )
        .padding(28)
        .max_width(760)
        .width(Fill)
        .style(ui::card(t));
        let dim = if t.dark { Color::from_rgb(0.04, 0.06, 0.05) } else { Color::from_rgb(0.86, 0.88, 0.86) };
        container(scroll(t, container(card).padding(32).center_x(Fill).width(Fill)).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(ui::plain(dim, 0.0))
            .into()
    }

    // ---- settings ----

    fn settings_view(&self, t: Tok) -> El<'_> {
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

        let profile = section(t,
            "Profile",
            row![
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
            .into(),
        );
        let calls = section(t,
            "Calls",
            column![
                line(t,
                    "Available for calls",
                    if self.avail.available { "Your line is open" } else { "Calls won\u{2019}t ring until you turn it back on" },
                    toggler(self.avail.available).on_toggle(|on| Msg::SetAvail(on, None)).style(ui::toggle_style(t)).into(),
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
        let security: El = if self.lock == LockState::NeedsPassphrase {
            column![
                tx("Your keys are not protected by a passphrase yet.", 14.0, t.error),
                pill(t, Kind::Primary, Some(Icon::Lock), "Set a passphrase", Some(Msg::GoSetPass)),
            ]
            .spacing(10)
            .into()
        } else {
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
                    .push(pill(t, Kind::Quiet, None, "Hide", Some(Msg::HidePhrase)));
            } else if self.reveal_form {
                c = c
                    .push(self.pass_field(t, "Enter your passphrase", &self.pass_in, Msg::PassIn, Some(Msg::RevealSubmit)))
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
                    "Needs your passphrase",
                    pill(t, Kind::Quiet, Some(Icon::Key), "Show", Some(Msg::ToggleReveal)),
                ));
            }
            if self.change_form {
                c = c
                    .push(self.pass_field(t, "Current passphrase", &self.old_in, Msg::OldIn, None))
                    .push(self.new_pass_fields(t, Msg::ChangePassSubmit))
                    .push(
                        row![
                            pill(t, Kind::Primary, None, if self.busy { "Working..." } else { "Change passphrase" }, (!self.busy).then_some(Msg::ChangePassSubmit)),
                            pill(t, Kind::Ghost, None, "Cancel", Some(Msg::ToggleChange)),
                        ]
                        .spacing(8),
                    );
            } else {
                c = c.push(line(t,
                    "Passphrase",
                    "Unlocks Tinline when it starts",
                    pill(t, Kind::Quiet, Some(Icon::Lock), "Change", Some(Msg::ToggleChange)),
                ));
            }
            c.into()
        };
        let about = section(t,
            "About",
            column![
                row![ui::logo(40.0, t.primary, t.thread), column![bold("Tinline", 18.0, t.ink), tx(format!("Version {} \u{b7} by Osvauld", env!("CARGO_PKG_VERSION")), 13.0, t.ink2)].spacing(2)].spacing(12).align_y(Alignment::Center),
                tx("Free and open source under the GPL. No ads, no tracking, no analytics.", 14.0, t.ink2),
                tx("Fonts: Figtree and IBM Plex Mono, SIL Open Font License 1.1. Icons: Lucide (ISC).", 13.0, t.ink2),
                tx("Not for emergency calls.", 13.0, t.ink2),
            ]
            .spacing(10)
            .into(),
        );
        let quit = pill(t, Kind::Danger, Some(Icon::LogOut), "Quit Tinline", Some(Msg::Quit));
        let items = ["Profile", "Calls & availability", "Audio devices", "Security", "About"];
        let mut nav = column![
            button(row![ui::icon(Icon::ArrowLeft, 18.0, t.ink2), semi("Back", 14.0, t.ink2)].spacing(8).align_y(Alignment::Center))
                .padding([8, 12])
                .style(ui::button_style(t, Kind::Ghost, 999.0))
                .on_press(Msg::Back),
            Space::new().height(8),
        ]
        .spacing(2);
        for (i, name) in items.iter().enumerate() {
            let on = self.settings_tab as usize == i;
            nav = nav.push(
                button(semi(*name, 14.0, if on { t.on_primary_c } else { t.ink }))
                    .padding([10, 14])
                    .width(Fill)
                    .style(ui::row_style(t, on))
                    .on_press(Msg::SettingsTab(i as u8)),
            );
        }
        let nav = container(nav).padding(16).width(240).height(Fill).style(ui::sidebar(t));
        let page: El = match self.settings_tab {
            0 => profile,
            1 => calls,
            2 => audio,
            3 => section(t, "Security", security),
            _ => column![about, quit].spacing(20).into(),
        };
        let content = column![bold(items[(self.settings_tab as usize).min(4)], 24.0, t.ink), self.notice_bar(t), page].spacing(20);
        row![
            nav,
            scroll(t, container(container(content).max_width(640).width(Fill)).padding(32).center_x(Fill).width(Fill)).height(Fill)
        ]
        .into()
    }

    // ---- calls ----

    fn call_shell<'a>(&'a self, t: Tok, top: El<'a>, middle: El<'a>, bottom: El<'a>) -> El<'a> {
        container(
            column![
                container(top).center_x(Fill),
                Space::new().height(Fill),
                middle,
                Space::new().height(Fill),
                bottom,
                self.notice_bar(t),
            ]
            .spacing(16)
            .align_x(Alignment::Center),
        )
        .padding(40)
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .into()
    }

    fn round_btn<'a>(&self, t: Tok, kind: Kind, icon: Icon, text_: &str, on: Option<Msg>, active: bool) -> El<'a> {
        let fg = match kind {
            Kind::Accept | Kind::End => Color::WHITE,
            _ if active => t.on_primary,
            _ => t.ink,
        };
        let style = ui::button_style(t, if active { Kind::Primary } else { kind }, 999.0);
        column![
            button(container(ui::icon(icon, 28.0, fg)).center_x(68).center_y(68))
                .padding(0)
                .width(68)
                .height(68)
                .style(style)
                .on_press_maybe(on),
            tx(text_.to_string(), 13.0, t.ink2),
        ]
        .spacing(8)
        .align_x(Alignment::Center)
        .into()
    }

    fn call_view(&self, t: Tok) -> El<'_> {
        let c = self.call.as_ref().unwrap();
        let secs = c.stats.as_ref().map(|s| s.secs).unwrap_or(0);
        let incoming_ring = c.info.incoming && !c.answered && c.state == CallState::Ringing;
        let active = c.state == CallState::Active;
        let reconnecting = active && c.stats.as_ref().is_some_and(|s| s.reconnecting);

        let top: El = match (&c.state, &c.stats) {
            (CallState::Active, Some(st)) => {
                let total = st.received + st.lost;
                let loss = if total > 0 { st.lost as f64 * 100.0 / total as f64 } else { 0.0 };
                let (n, word) = quality(st.rtt_ms, loss);
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
            row![ui::icon(Icon::Lock, 14.0, t.ink2), tx("End-to-end encrypted", 13.0, t.ink2)]
                .spacing(6)
                .align_y(Alignment::Center)
                .into()
        } else if let Some(st) = c.stats.as_ref().filter(|_| active) {
            if st.direct {
                row![ui::icon(Icon::Lock, 14.0, t.ink2), tx(format!("End-to-end encrypted \u{b7} straight to {}", c.info.peer_name), 13.0, t.ink2)]
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
        let middle = column![
            avatar(t, &c.info.peer_name, &c.info.peer_did, 128.0),
            bold(c.info.peer_name.clone(), 32.0, t.ink),
            status,
            note,
        ]
        .spacing(12)
        .align_x(Alignment::Center);

        let muted = self.ctl.muted.load(Ordering::Relaxed);
        let bottom: El = if incoming_ring {
            row![
                self.round_btn(t, Kind::End, Icon::PhoneOff, "Decline", Some(Msg::Decline), false),
                self.round_btn(t, Kind::Accept, Icon::Phone, "Answer", Some(Msg::Answer), false),
            ]
            .spacing(48)
            .into()
        } else {
            row![
                self.round_btn(
                    t,
                    Kind::Quiet,
                    if muted { Icon::MicOff } else { Icon::Mic },
                    if muted { "Muted" } else { "Mute" },
                    active.then_some(Msg::ToggleMute),
                    muted
                ),
                self.round_btn(t, Kind::End, Icon::PhoneOff, if active { "Hang up" } else { "Cancel" }, Some(Msg::Hangup), false),
            ]
            .spacing(48)
            .into()
        };
        let hint = if incoming_ring {
            "Enter answers \u{b7} Esc declines"
        } else if active {
            "End-to-end encrypted \u{b7} Ctrl M mute \u{b7} Ctrl E end"
        } else {
            "Ctrl E cancels"
        };
        let mut bottom_col = column![bottom].spacing(18).align_x(Alignment::Center);
        if active {
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
        self.call_shell(t, top, middle.into(), bottom_col.into())
    }

    fn ended_view(&self, t: Tok) -> El<'_> {
        let e = self.ended.as_ref().unwrap();
        let unreachable = e.reason == "unreachable";
        let mut mid = column![
            avatar(t, &e.name, &e.did, 112.0),
            tx(if unreachable { "Couldn\u{2019}t reach" } else { "Call ended" }, 16.0, t.ink2),
            bold(e.text.clone(), 30.0, t.ink),
        ]
        .spacing(12)
        .align_x(Alignment::Center);
        if e.secs > 0 {
            mid = mid.push(mono(format!("{:02}:{:02}", e.secs / 60, e.secs % 60), 20.0, t.ink2));
            mid = mid.push(if e.direct {
                badge(t.primary_c, t.on_primary_c, Some(Icon::Direct), "Direct")
            } else {
                badge(t.relay_c, t.on_relay_c, Some(Icon::Relayed), "Relayed \u{b7} encrypted")
            });
        }
        if unreachable {
            mid = mid.push(container(tx(
                format!("{} may be offline, out of signal, or not taking calls right now. Tinline can\u{2019}t leave messages.", e.name),
                14.0,
                t.ink2,
            )).max_width(440));
        }
        let bottom = column![
            row![
                pill(t, Kind::Primary, Some(Icon::Phone), if unreachable { "Try again" } else { "Call again" }, Some(Msg::CallAgain(e.did.clone()))),
                pill(t, Kind::Quiet, None, "Close", Some(Msg::CloseEnded)),
            ]
            .spacing(12),
            tx("Closes by itself after a few seconds.", 12.0, t.ink2),
        ]
        .spacing(10)
        .align_x(Alignment::Center);
        self.call_shell(t, Space::new().into(), mid.into(), bottom.into())
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

fn until_text(until: u64) -> String {
    let d = until.saturating_sub(now_secs());
    if d >= 3600 { format!("{} h {} min", d / 3600, d / 60 % 60) } else { format!("{} min", (d / 60).max(1)) }
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
        let cell = (bounds.width.min(bounds.height) / total as f32).floor().max(1.0);
        let off = (bounds.width.min(bounds.height) - cell * total as f32) / 2.0;
        for y in 0..self.0.n {
            for x in 0..self.0.n {
                if self.0.dark[y * self.0.n + x] {
                    frame.fill_rectangle(
                        Point::new(off + (x + quiet) as f32 * cell, off + (y + quiet) as f32 * cell),
                        Size::new(cell, cell),
                        Color::BLACK,
                    );
                }
            }
        }
        vec![frame.into_geometry()]
    }
}
