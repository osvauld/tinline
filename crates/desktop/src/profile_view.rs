//! Settings, My profile and Person profile in the detail pane, after playground screens/settings
//! and screens/profile (profile.css furniture). Cards and buttons are CSS-radius surfaces in the
//! source (border-radius, not the smoothed shape), so iced radii are the faithful port here.
use super::*;
use super::super::profile_ui::{self as pf, Action, Layer, Page};
use super::shell_view::{pg_dialog, wash};
use crate::pg_icons::pgi;
use iced::border::Radius;
use iced::widget::{column, mouse_area, slider, stack, text_editor};
use serde_json::Value;

fn m(a: Action) -> Msg {
    Msg::Profile(a)
}

/// A card row that gets the card's outer corners for its hover wash (`overflow: hidden`).
pub(super) type RowFn<'a> = Box<dyn FnOnce(Radius) -> El<'a> + 'a>;

pub(super) fn hairline<'a>(t: Tok) -> El<'a> {
    container(Space::new()).width(Fill).height(1).style(ui::plain(t.hairline, 0.0)).into()
}

/// `.pf-card > * + * { border-block-start }` loses to `.pf-act { border: 0 }`: only rows that are
/// not `.pf-act` (FieldRow, Availability) draw the hairline above themselves, when not first.
pub(super) fn sep_top<'a>(t: Tok, r: Radius, el: El<'a>) -> El<'a> {
    if r.top_left > 0.0 { el } else { column![hairline(t), el].into() }
}

/// `.pf-card`: --r-ctl, --surface, inset 1px hairline (drawn over the rows, like the box-shadow).
pub(super) fn pf_card<'a>(t: Tok, rows: Vec<RowFn<'a>>) -> El<'a> {
    let n = rows.len();
    let mut c = column![].spacing(0);
    for (i, r) in rows.into_iter().enumerate() {
        let top = if i == 0 { 14.0 } else { 0.0 };
        let bottom = if i + 1 == n { 14.0 } else { 0.0 };
        c = c.push(r(Radius { top_left: top, top_right: top, bottom_right: bottom, bottom_left: bottom }));
    }
    let border = move |_: &iced::Theme| iced::widget::container::Style {
        border: Border { color: t.hairline, width: 1.0, radius: 14.0.into() },
        ..Default::default()
    };
    stack![container(c).width(Fill).style(ui::plain(t.surface, 14.0)), container(Space::new()).width(Fill).height(Fill).style(border)].into()
}

/// `.pf-sec > h2`: 12px, 650, uppercase, muted, 16px line, 0 4 8 margins.
pub(super) fn pf_h2<'a>(t: Tok, title: &str) -> El<'a> {
    container(bold(title.to_uppercase(), 12.0, t.ink2).line_height(lh(16.0))).padding(Padding { top: 0.0, right: 4.0, bottom: 8.0, left: 4.0 }).into()
}

/// `.pf-note`: 13/19 muted, 10px above, 4px in.
pub(super) fn pf_note<'a>(t: Tok, s: impl Into<String>) -> El<'a> {
    container(tx(s, 13.0, t.ink2).line_height(lh(19.0))).padding(Padding { top: 10.0, right: 4.0, bottom: 0.0, left: 4.0 }).width(Fill).into()
}

/// `.pf-sec`: 0 16 padding, 24 above (or `top`).
pub(super) fn pf_sec<'a>(top: f32, content: El<'a>) -> El<'a> {
    container(content).padding(Padding { top, right: 16.0, bottom: 0.0, left: 16.0 }).width(Fill).into()
}

/// A titled section with its card and optional notes.
pub(super) fn section<'a>(t: Tok, title: &str, rows: Vec<RowFn<'a>>, notes: Vec<String>) -> El<'a> {
    let mut c = column![pf_h2(t, title), pf_card(t, rows)].spacing(0);
    for n in notes {
        c = c.push(pf_note(t, n));
    }
    pf_sec(24.0, c.into())
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Btn {
    Primary,
    Outline,
    Quiet,
    Danger,
    DangerQuiet,
}

fn brighten(c: Color, f: f32) -> Color {
    Color { r: (c.r * f).min(1.0), g: (c.g * f).min(1.0), b: (c.b * f).min(1.0), a: c.a }
}

/// groups/Btn.svelte: 44px, 0 18 padding, 22 radius, 600 15/20, optional 18px icon with an 8px gap.
pub(super) fn pg_btn<'a>(t: Tok, variant: Btn, icon: Option<&'static str>, caption: &str, on: Option<Msg>) -> El<'a> {
    let disabled = on.is_none();
    let (bg, fg, line) = match variant {
        Btn::Primary => (Some(t.accent), t.on_accent, None),
        Btn::Outline => (Some(t.surface), t.ink, Some(t.line)),
        Btn::Quiet => (None, t.ink, None),
        Btn::Danger => (Some(t.danger_btn), t.on_danger_btn, None),
        Btn::DangerQuiet => (None, t.danger, None),
    };
    let fade = move |c: Color| if disabled { ui::alpha(c, c.a * 0.45) } else { c };
    let mut r = row![].spacing(8).align_y(Alignment::Center);
    if let Some(i) = icon {
        r = r.push(pgi(i, 18.0, fade(fg)));
    }
    r = r.push(semi(caption.to_string(), 15.0, fade(fg)).line_height(lh(20.0)).wrapping(iced::widget::text::Wrapping::None));
    let style = move |_: &iced::Theme, status: iced::widget::button::Status| {
        use iced::widget::button::Status as S;
        let hover = matches!(status, S::Hovered | S::Pressed);
        let fill = match (variant, hover) {
            (Btn::Primary | Btn::Danger, true) => bg.map(|c| brighten(c, 1.08)),
            (Btn::Outline | Btn::Quiet | Btn::DangerQuiet, true) => Some(t.surface2),
            _ => bg,
        };
        iced::widget::button::Style {
            background: fill.map(|c| iced::Background::Color(fade(c))),
            text_color: fg,
            border: Border { color: line.map(fade).unwrap_or(Color::TRANSPARENT), width: if line.is_some() { 1.0 } else { 0.0 }, radius: 22.0.into() },
            ..Default::default()
        }
    };
    button(container(r).height(44).center_y(44)).padding([0, 19]).height(44).style(style).on_press_maybe(on).into()
}

/// lib/ui/Switch.svelte: 44x26, 13 radius, --line / --accent track, 20px --surface knob with --shadow-lift.
pub(super) fn pg_switch<'a>(t: Tok, on: bool, msg: Msg) -> El<'a> {
    let knob = ui::raised(container(Space::new()).width(20).height(20), t, ui::Depth::Lift, ui::plain(t.surface, 10.0));
    let track = container(row![Space::new().width(if on { 18 } else { 0 }), knob]).padding(3).width(44).height(26)
        .style(ui::plain(if on { t.accent } else { t.line }, 13.0));
    button(track).padding(0).style(|_, _| iced::widget::button::Style::default()).on_press(msg).into()
}

/// IconButton: 44 round, 22px icon, --surface-2 on hover.
pub(super) fn icon_button<'a>(t: Tok, icon: &'static str, color: Color, on: Msg) -> El<'a> {
    button(container(pgi(icon, 22.0, color)).width(44).height(44).center_x(44).center_y(44))
        .padding(0).width(44).height(44).style(wash(t, None, Some(t.surface2), 22.0)).on_press(on).into()
}

/// `.pf-act`: 60px min, 8 16 padding, 14 gap, 20px muted icon, 600 15/20 title, 13/18 muted line.
#[allow(clippy::too_many_arguments)]
pub(super) fn pf_act<'a>(t: Tok, icon: Option<&'static str>, title: String, sub: Option<(String, Color, bool)>, end: Option<El<'a>>, on: Option<Msg>, danger: bool, r: Radius) -> El<'a> {
    let ink = if danger { t.danger } else { t.ink };
    let mut row_ = row![].spacing(14).align_y(Alignment::Center);
    if let Some(i) = icon {
        row_ = row_.push(pgi(i, 20.0, if danger { t.danger } else { t.ink2 }));
    }
    let mut txt = column![semi(title, 15.0, ink).line_height(lh(20.0)).wrapping(iced::widget::text::Wrapping::None)].spacing(1).width(Fill);
    if let Some((s, c, strong)) = sub {
        txt = txt.push(text(s).size(13).color(c).font(if strong { ui::SANS_SEMI } else { ui::SANS }).line_height(lh(18.0)));
    }
    row_ = row_.push(txt);
    if let Some(e) = end {
        row_ = row_.push(e);
    }
    let body = row![Space::new().height(44), row_].align_y(Alignment::Center);
    match on {
        Some(msg) => button(body).padding([8, 16]).width(Fill)
            .style(move |_, status| {
                let hover = matches!(status, iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed);
                iced::widget::button::Style {
                    background: hover.then_some(iced::Background::Color(t.surface2)),
                    border: Border { radius: r, ..Default::default() },
                    ..Default::default()
                }
            })
            .on_press(msg).into(),
        None => container(body).padding([8, 16]).width(Fill).into(),
    }
}

fn chevron<'a>(t: Tok) -> El<'a> {
    pgi("chevr", 18.0, t.ink2)
}

/// `.pf-err`: 10 12 padding, 12 radius, --danger-bg / --danger, 13/19.
pub(super) fn pf_err<'a>(t: Tok, s: String) -> El<'a> {
    container(tx(s, 13.0, t.danger).line_height(lh(19.0))).padding([10, 12]).width(Fill).style(ui::plain(t.danger_bg, 12.0)).into()
}

/// `.pf-dlg`: 20 20 16 padding; h2 17/24 650; `.pf-acts` 18px above, 8 gap, end-aligned.
pub(super) fn pf_dlg<'a>(t: Tok, title: String, body: Vec<El<'a>>, acts: Vec<El<'a>>) -> El<'a> {
    let mut c = column![container(bold(title, 17.0, t.ink).line_height(lh(24.0))).padding(Padding { bottom: 4.0, ..Padding::ZERO })].spacing(0);
    for b in body {
        c = c.push(container(b).padding(Padding { top: 8.0, ..Padding::ZERO }).width(Fill));
    }
    let mut a = row![Space::new().width(Fill)].spacing(8).align_y(Alignment::Center);
    for x in acts {
        a = a.push(x);
    }
    c = c.push(container(a).padding(Padding { top: 18.0, ..Padding::ZERO }));
    container(c).padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 }).width(Fill).into()
}

fn lead<'a>(t: Tok, s: String) -> El<'a> {
    tx(s, 15.0, t.ink).line_height(lh(21.0)).into()
}
fn para<'a>(t: Tok, s: String) -> El<'a> {
    tx(s, 13.0, t.ink2).line_height(lh(19.0)).into()
}

/// The text inputs of FieldRow / AddInfo: 10 14 padding, --r-ctl, 1px --line, --surface; focus --accent.
fn pf_input_style(t: Tok) -> impl Fn(&iced::Theme, iced::widget::text_input::Status) -> iced::widget::text_input::Style {
    move |_, status| {
        let focused = matches!(status, iced::widget::text_input::Status::Focused { .. });
        iced::widget::text_input::Style {
            background: iced::Background::Color(t.surface),
            border: Border { color: if focused { t.accent } else { t.line }, width: 1.0, radius: 14.0.into() },
            icon: t.ink2,
            placeholder: t.ink2,
            value: t.ink,
            selection: ui::alpha(t.accent, 0.3),
        }
    }
}

/// Value.svelte for structured values: lists, and nested key/value rows behind a 2px hairline.
fn value_view<'a>(t: Tok, v: &Value, depth: usize) -> El<'a> {
    if depth > 4 {
        return tx("\u{2026}", 15.0, t.ink2).line_height(lh(21.0)).into();
    }
    if let Some(s) = pf::scalar(v) {
        return tx(s, 15.0, t.ink).line_height(lh(21.0)).into();
    }
    let nest = |items: Vec<El<'a>>| -> El<'a> {
        let mut c = column![].spacing(8);
        for i in items {
            c = c.push(i);
        }
        container(row![container(Space::new()).width(2).height(Fill).style(ui::plain(t.hairline, 0.0)), container(c).padding(Padding { left: 10.0, ..Padding::ZERO })])
            .padding(Padding { top: 6.0, ..Padding::ZERO }).into()
    };
    match v {
        Value::Array(a) if a.iter().all(|x| pf::scalar(x).is_some()) => {
            let s = a.iter().filter_map(pf::scalar).collect::<Vec<_>>().join(", ");
            tx(if s.is_empty() { "\u{2014}".into() } else { s }, 15.0, t.ink).line_height(lh(21.0)).into()
        }
        Value::Array(a) => nest(a.iter().map(|x| value_view(t, x, depth + 1)).collect()),
        Value::Object(o) => nest(o.iter().map(|(k, x)| column![tx(pf::humanize(k), 13.0, t.ink2).line_height(lh(18.0)), value_view(t, x, depth + 1)].spacing(1).into()).collect()),
        _ => Space::new().into(),
    }
}

impl App {
    /// Avatar.svelte with `person`: the decoded picture when ready, else initials (a faint ring while awaiting).
    pub(super) fn profile_avatar<'a>(&'a self, t: Tok, name: &str, key: &str, size: f32) -> El<'a> {
        let did = if key == "me" { self.me.as_ref().map(|c| c.did.as_str()).unwrap_or(key) } else { key };
        let cached = self.profiles.cache.get(did);
        if let Some(h) = cached.and_then(|c| c.photo.as_ref()) {
            return image(h.clone()).width(size).height(size).content_fit(ContentFit::Fill).filter_method(iced::widget::image::FilterMethod::Linear).into();
        }
        let base = avatar(t, name, if key == "me" { "me" } else { did }, size);
        if cached.is_some_and(|c| c.awaiting) {
            let (_, ink) = ui::avatar_colors(t, if key == "me" { "me" } else { did });
            let ring = move |_: &iced::Theme| iced::widget::container::Style {
                border: Border { color: ui::alpha(ink, 0.45 * 0.5), width: (size * 0.34 * 0.03).max(1.5), radius: (size / 2.0).into() },
                ..Default::default()
            };
            return stack![base, container(Space::new()).width(size).height(size).style(ring)].into();
        }
        base
    }

    pub(super) fn profile_shell(&self, t: Tok) -> El<'_> {
        row![self.sidebar(t), self.call_strip_pane(t, self.profile_page(t))].height(Fill).into()
    }

    /// `.pf-bar`: 60px, --surface, hairline below; back IconButton then the 17px/600 title.
    fn pf_bar(&self, t: Tok, title: &str) -> El<'_> {
        column![
            container(row![icon_button(t, "back", t.ink, m(Action::Back)), semi(title.to_string(), 17.0, t.ink).line_height(lh(21.0))].spacing(4).align_y(Alignment::Center))
                .padding(Padding { top: 8.0, right: 12.0, bottom: 7.0, left: 8.0 })
                .width(Fill)
                .height(60)
                .center_y(60)
                .style(ui::plain(t.surface, 0.0)),
            hairline(t)
        ]
        .into()
    }

    fn profile_page(&self, t: Tok) -> El<'_> {
        let page = self.profiles.page.clone().unwrap_or(Page::Settings);
        let (title, body): (&str, El) = match &page {
            Page::Settings => ("Settings", self.settings_landing(t)),
            Page::Notifications => ("Notifications", self.settings_unported(t, "Notifications")),
            Page::Sounds => ("Sounds", self.settings_unported(t, "Sounds")),
            Page::Me => ("My profile", self.my_profile(t)),
            Page::Person(did) => ("Profile", self.person_profile(t, did)),
            Page::Native(tab) => (
                match tab {
                    0 => "This computer",
                    1 => "Devices",
                    2 => "Calls on this computer",
                    3 => "Audio devices",
                    4 => "Security",
                    5 => "About",
                    6 => "Hosting",
                    7 => "Privacy",
                    _ => "Accounts on this computer",
                },
                self.native_page(t, *tab),
            ),
        };
        let base: El = container(column![self.pf_bar(t, title), body].width(Fill).height(Fill)).width(Fill).height(Fill).style(ui::plain(t.bg, 0.0)).into();
        let mut layers = vec![base];
        if let Some(f) = self.profile_float(t) {
            layers.push(f);
        }
        if let Some(l) = self.profile_layer(t) {
            layers.push(l);
        }
        if layers.len() == 1 { layers.pop().unwrap() } else { iced::widget::Stack::with_children(layers).into() }
    }

    /// `.pf-scroll` + `.pf-col`: max 560 centred, `bottom` px of room at the end.
    fn pf_scroll<'a>(&'a self, t: Tok, body: El<'a>, bottom: f32) -> El<'a> {
        scroll(t, container(container(body).max_width(560).width(Fill)).width(Fill).center_x(Fill).padding(Padding { bottom, ..Padding::ZERO }))
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn settings_landing(&self, t: Tok) -> El<'_> {
        let status = self.own_profile().and_then(|c| c.fields.get("status")).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let (av_text, busy) = if self.avail.available {
            ("Available".to_string(), false)
        } else {
            (self.avail.until.map(|u| format!("Busy until {}", pf::until_label(u))).unwrap_or_else(|| "Busy".into()), true)
        };
        let name = self.profile_name.clone();
        let me_row: RowFn = Box::new(move |r: Radius| {
            let mut lines = column![semi(name.clone(), 17.0, t.ink).line_height(lh(24.0))].spacing(1).width(Fill);
            if let Some(s) = status {
                lines = lines.push(tx(s, 13.0, t.ink2).line_height(lh(18.0)));
            }
            lines = lines.push(semi(av_text, 13.0, if busy { t.warn_ink } else { t.ink2 }).line_height(lh(18.0)));
            button(row![self.profile_avatar(t, &name, "me", 64.0), lines, chevron(t)].spacing(16).align_y(Alignment::Center))
                .padding([12, 16]).width(Fill)
                .style(move |_, st| iced::widget::button::Style {
                    background: matches!(st, iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed).then_some(iced::Background::Color(t.surface2)),
                    border: Border { radius: r, ..Default::default() },
                    ..Default::default()
                })
                .on_press(m(Action::Open(Page::Me)))
                .into()
        });
        let mut accounts: Vec<RowFn> = Vec::new();
        let sel_line = if self.status.online { "Selected \u{b7} online" } else { "Selected \u{b7} connecting" };
        accounts.push(Box::new(move |_| {
            let body = row![self.profile_avatar(t, &self.profile_name, "me", 36.0),
                column![semi(self.profile_name.clone(), 15.0, t.ink).line_height(lh(20.0)), semi(sel_line, 13.0, t.accent_ink).line_height(lh(18.0))].spacing(1).width(Fill),
                pgi("check", 18.0, t.ink2)]
                .spacing(14)
                .align_y(Alignment::Center);
            container(row![Space::new().height(44), body].align_y(Alignment::Center)).padding([8, 16]).width(Fill).into()
        }));
        for a in self.accounts.iter().filter(|a| !a.current) {
            let (name, did) = (a.name.clone(), a.did.clone());
            accounts.push(Box::new(move |r| {
                let face = avatar(t, &name, &did, 36.0);
                let body = row![face, column![semi(name.clone(), 15.0, t.ink).line_height(lh(20.0)), tx("Paused \u{b7} tap to switch", 13.0, t.ink2).line_height(lh(18.0))].spacing(1).width(Fill)]
                    .spacing(14)
                    .align_y(Alignment::Center);
                button(row![Space::new().height(44), body].align_y(Alignment::Center)).padding([8, 16]).width(Fill)
                    .style(move |_, st| iced::widget::button::Style {
                        background: matches!(st, iced::widget::button::Status::Hovered).then_some(iced::Background::Color(t.surface2)),
                        border: Border { radius: r, ..Default::default() },
                        ..Default::default()
                    })
                    .on_press(Msg::SwitchAsk(did.clone()))
                    .into()
            }));
        }
        accounts.push(Box::new(move |r| pf_act(t, Some("plus"), "Add another account".into(), None, Some(chevron(t)), Some(Msg::NewAccount(false)), false, r)));
        let areas: Vec<RowFn> = [("devices", "Devices", Page::Native(1)), ("shield", "Privacy", Page::Native(7)), ("bell", "Notifications", Page::Notifications), ("vol", "Sounds", Page::Sounds)]
            .into_iter()
            .map(|(icon, label, page)| -> RowFn { Box::new(move |r| pf_act(t, Some(icon), label.into(), None, Some(chevron(t)), Some(m(Action::Open(page))), false, r)) })
            .collect();
        let mut c = column![
            pf_sec(16.0, pf_card(t, vec![me_row])),
            pf_sec(24.0, column![pf_card(t, accounts),
                container(tx(format!("Only the selected account is online on this computer. Switching pauses the other one: it receives no calls until you select it again."), 13.0, t.ink2).line_height(lh(18.0)))
                    .padding(Padding { top: 8.0, right: 16.0, bottom: 0.0, left: 16.0 })].into()),
            pf_sec(24.0, pf_card(t, areas)),
        ]
        .spacing(0);
        // Native-only, necessary on a computer: device audio, video admission, key storage. Not a playground area.
        c = c.push(section(t, "This computer", vec![Box::new(move |r| pf_act(t, Some("sliders"), "Audio, video and storage".into(), Some(("Settings that belong to this computer only".into(), t.ink2, false)), Some(chevron(t)), Some(m(Action::Open(Page::Native(0)))), false, r))], vec![]));
        if self.notice.is_some() {
            c = c.push(container(self.notice_bar(t)).padding(16));
        }
        self.pf_scroll(t, c.into(), 40.0)
    }

    /// Notifications / Sounds: the playground's controls have no desktop service behind them yet.
    fn settings_unported(&self, t: Tok, what: &str) -> El<'_> {
        let line = if what == "Notifications" {
            "Per-kind alert switches and the permission list aren\u{2019}t connected on this desktop yet. Calls and messages still use the system notifications this app already shows."
        } else {
            "Quiet and the call sound sets aren\u{2019}t connected to this desktop\u{2019}s audio yet. Microphone, speaker and the test tone are under This computer."
        };
        let rows: Vec<RowFn> = vec![Box::new(move |r| pf_act(t, Some("sliders"), "Audio devices".into(), Some(("Microphone, speaker and test tone".into(), t.ink2, false)), Some(chevron(t)), Some(m(Action::Open(Page::Native(3)))), false, r))];
        let c = column![pf_sec(16.0, para(t, line.into())), section(t, "On this computer", rows, vec![])];
        self.pf_scroll(t, c.into(), 40.0)
    }

    fn my_profile(&self, t: Tok) -> El<'_> {
        let own = self.own_profile();
        let fields = own.map(|c| c.fields.clone()).unwrap_or_default();
        let has_pic = own.is_some_and(|c| c.hash.is_some());
        // .hero: 24 20 4; .btns 18 above, equal-width (min 140) pair, 8 gap.
        let mut btns = row![].spacing(8);
        let pair = |el: El<'static>| container(el).width(Length::Shrink);
        if has_pic {
            btns = btns.push(pair(pg_btn(t, Btn::Outline, Some("camera"), "Change photo", Some(m(Action::PickPhoto)))))
                .push(container(pg_btn(t, Btn::Outline, None, "Remove", Some(m(Action::RemovePhoto)))).width(167));
        } else {
            btns = btns.push(pair(pg_btn(t, Btn::Outline, Some("camera"), "Add photo", Some(m(Action::PickPhoto)))));
        }
        let hero = container(column![self.profile_avatar(t, &self.profile_name, "me", 112.0), container(btns).padding(Padding { top: 18.0, ..Padding::ZERO })].align_x(Alignment::Center))
            .padding(Padding { top: 24.0, right: 20.0, bottom: 4.0, left: 20.0 })
            .width(Fill)
            .center_x(Fill);
        let name = Value::String(self.profile_name.clone());
        let status = fields.get("status").cloned();
        let about = fields.get("about").cloned();
        let profile: Vec<RowFn> = vec![
            Box::new(move |r| self.field_row(t, "name", "Name", Some(name), FieldKind::Text, false, false, "Not set", "Your name", "", r)),
            Box::new(move |r| self.field_row(t, "status", "Status", status, FieldKind::Text, true, false, "Add a status", "What are you up to?", "", r)),
            Box::new(move |r| self.field_row(t, "about", "About", about, FieldKind::Multiline, true, false, "Add a few words about you", "", "", r)),
        ];
        let mut c = column![hero, section(t, "Profile", profile, vec![])].spacing(0);
        c = c.push(section(t, "Availability", vec![Box::new(move |r| sep_top(t, r, self.availability(t)))],
            vec!["Your contacts can see this, and it\u{2019}s the same on all your devices. It affects whether calls reach you, and it\u{2019}s separate from whether you\u{2019}re online.".into()]));
        let mut extras: Vec<RowFn> = Vec::new();
        let keys = pf::extra_keys(&fields);
        let has_other = keys.iter().any(|k| pf::known(k).is_none());
        for k in keys {
            let v = fields.get(&k).cloned();
            let kind = if pf::known(&k).is_some_and(|x| x.1) { FieldKind::Link } else { FieldKind::Text };
            let hint = pf::known(&k).map(|x| x.2).unwrap_or("");
            extras.push(Box::new(move |r| self.field_row(t, &k, &pf::label(&k), v, kind, true, false, "Not set", "", hint, r)));
        }
        extras.push(Box::new(move |r| pf_act(t, Some("plus"), "Add information".into(), None, None, Some(m(Action::Add)), false, r)));
        let mut notes = vec![pf::FIELD_HELP.to_string()];
        if has_other {
            notes.push("Some information came from another device or app version. It\u{2019}s shown as stored and kept when you edit anything else.".into());
        }
        c = c.push(section(t, "Additional information", extras, notes));
        self.pf_scroll(t, c.into(), 96.0)
    }

    /// Availability.svelte: Available | Busy segmented control, the line (or a refusal) with
    /// "Saved", and while Busy the length chips. 14 16 padding, 10 gap.
    fn availability(&self, t: Tok) -> El<'_> {
        let available = self.avail.available;
        let seg = |label: &'static str, on: bool, msg: Msg| -> El<'_> {
            let cap = semi(label, 15.0, if on { t.ink } else { t.ink2 }).line_height(lh(21.0));
            let b = button(container(cap).width(Fill).height(44).center_x(Fill).center_y(44)).padding(0).width(Fill)
                .style(move |_, _| iced::widget::button::Style {
                    background: on.then_some(iced::Background::Color(t.surface)),
                    border: Border { color: if on { t.hairline } else { Color::TRANSPARENT }, width: if on { 1.0 } else { 0.0 }, radius: 12.0.into() },
                    shadow: if on { ui::Depth::Lift.single(t) } else { iced::Shadow::default() },
                    ..Default::default()
                })
                .on_press(msg);
            b.into()
        };
        let seg = container(row![seg("Available", available, m(Action::Available)), seg("Busy", !available, m(Action::Busy(self.profiles.busy_choice.unwrap_or("1h"))))].spacing(4))
            .padding(4).width(Fill).style(ui::plain(t.surface2, 16.0));
        let line = if available { "Available".to_string() } else { self.avail.until.map(|u| format!("Busy until {}", pf::until_label(u))).unwrap_or_else(|| "Busy until you change it".into()) };
        let mut now = row![].spacing(10).align_y(Alignment::Center);
        if self.profiles.av_error.is_empty() {
            now = now.push(tx(line, 13.0, t.ink).line_height(lh(19.0)).wrapping(iced::widget::text::Wrapping::None));
        } else {
            now = now.push(tx(self.profiles.av_error.clone(), 13.0, t.danger).line_height(lh(19.0)).width(Fill).wrapping(iced::widget::text::Wrapping::None));
        }
        if self.profiles.saved.contains_key("availability") {
            now = now.push(Space::new().width(Fill)).push(row![pgi("check", 13.0, t.accent_ink), semi(if self.status.online { "Saved" } else { "Saved \u{b7} syncs when connected" }, 13.0, t.accent_ink).line_height(lh(19.0))].spacing(3).align_y(Alignment::Center));
        }
        let mut c = column![seg, container(now).height(19)].spacing(10);
        if !available {
            let active = if self.avail.until.is_none() { "open" } else { self.profiles.busy_choice.unwrap_or("1h") };
            let mut chips = row![].spacing(8);
            for (id, label) in pf::BUSY_CHOICES {
                let on = active == id;
                chips = chips.push(button(container(semi(label, 13.0, if on { t.accent_ink } else { t.ink }).line_height(lh(21.0))).height(34).center_y(34))
                    .padding(Padding { top: 0.0, right: 15.0, bottom: 0.0, left: 15.0 }).height(36)
                    .style(move |_, st| iced::widget::button::Style {
                        background: if on { Some(iced::Background::Color(t.accent_soft)) } else if matches!(st, iced::widget::button::Status::Hovered) { Some(iced::Background::Color(t.surface2)) } else { None },
                        border: Border { color: if on { Color::TRANSPARENT } else { t.line }, width: 1.0, radius: 16.0.into() },
                        ..Default::default()
                    })
                    .on_press(m(Action::Busy(id))));
            }
            c = c.push(chips.wrap());
        }
        container(c).padding(Padding { top: 14.0, right: 16.0, bottom: 14.0, left: 16.0 }).width(Fill).into()
    }

    /// FieldRow.svelte. View: label over value (tap to edit; pencil; link tool); edit: input, the
    /// reserved hint/error line (36px), Save / Cancel (Remove for optional); remove: an inline ask.
    #[allow(clippy::too_many_arguments)]
    fn field_row<'a>(&'a self, t: Tok, key: &str, label_: &str, value: Option<Value>, kind: FieldKind, optional: bool, readonly: bool, empty: &str, placeholder: &str, hint: &str, r: Radius) -> El<'a> {
        sep_top(t, r, self.field_row_inner(t, key, label_, value, kind, optional, readonly, empty, placeholder, hint, r))
    }

    #[allow(clippy::too_many_arguments)]
    fn field_row_inner<'a>(&'a self, t: Tok, key: &str, label_: &str, value: Option<Value>, kind: FieldKind, optional: bool, readonly: bool, empty: &str, placeholder: &str, hint: &str, r: Radius) -> El<'a> {
        let key = key.to_string();
        let label_ = label_.to_string();
        let scalar = value.as_ref().and_then(pf::scalar);
        let editable = !readonly && (value.is_none() || scalar.is_some());
        if let Some(e) = self.profiles.edit.as_ref().filter(|e| e.key == key) {
            if e.removing {
                let c = column![
                    semi(format!("Remove \u{201c}{label_}\u{201d} from your profile?"), 15.0, t.ink).line_height(lh(21.0)),
                    tx("It disappears from your profile. Earlier versions can remain in synced history.", 13.0, t.ink2).line_height(lh(19.0)),
                ]
                .spacing(6);
                let mut c = c;
                if !e.error.is_empty() {
                    c = c.push(tx(e.error.clone(), 13.0, t.danger).line_height(lh(18.0)));
                }
                c = c.push(row![Space::new().width(Fill), pg_btn(t, Btn::Quiet, None, "Cancel", Some(m(Action::CancelEdit))), pg_btn(t, Btn::Danger, None, "Remove", (!self.profiles.pending).then(|| m(Action::Remove(key.clone()))))].spacing(8));
                return container(c).padding(Padding { top: 14.0, right: 16.0, bottom: 12.0, left: 16.0 }).width(Fill).into();
            }
            let input: El = if kind == FieldKind::Multiline {
                text_editor(&e.body).on_action(|a| m(Action::Body(a))).height(106).padding([10, 14]).font(ui::SANS).size(15).line_height(lh(21.0))
                    .style(move |_, status| {
                        let focused = matches!(status, text_editor::Status::Focused { .. });
                        text_editor::Style {
                            background: iced::Background::Color(t.surface),
                            border: Border { color: if focused { t.accent } else { t.line }, width: 1.0, radius: 14.0.into() },
                            placeholder: t.ink2,
                            value: t.ink,
                            selection: ui::alpha(t.accent, 0.3),
                        }
                    })
                    .into()
            } else {
                text_input(placeholder, &e.text).id(FIELD_ID).on_input(|s| m(Action::Input(s))).on_submit(m(Action::Save))
                    .padding([10.5, 14.0]).size(15).line_height(lh(21.0)).font(ui::SANS).style(pf_input_style(t)).into()
            };
            let limit = if key == "name" || key == "alias" { pf::LIMIT_NAME } else { pf::LIMIT_TEXT };
            let bytes = e.text.len();
            let count: El = if bytes as f32 > limit as f32 * 0.8 {
                tx(format!("{bytes} of {limit} bytes"), 13.0, if bytes > limit { t.danger } else { t.ink2 }).line_height(lh(18.0)).into()
            } else {
                Space::new().into()
            };
            let below: El = if !e.error.is_empty() {
                tx(e.error.clone(), 13.0, t.danger).line_height(lh(18.0)).into()
            } else if !hint.is_empty() {
                tx(hint.to_string(), 13.0, t.ink2).line_height(lh(18.0)).into()
            } else {
                Space::new().into()
            };
            let mut acts = row![].spacing(8).align_y(Alignment::Center);
            if optional && value.is_some() {
                acts = acts.push(pg_btn(t, Btn::DangerQuiet, None, "Remove", (!self.profiles.pending).then(|| m(Action::Remove(key.clone())))));
            }
            acts = acts.push(Space::new().width(Fill)).push(pg_btn(t, Btn::Quiet, None, "Cancel", Some(m(Action::CancelEdit)))).push(pg_btn(t, Btn::Primary, None, "Save", (!self.profiles.pending).then_some(m(Action::Save))));
            return container(column![row![semi(label_, 13.0, t.ink2).line_height(lh(18.0)).width(Fill), count], input, container(below).height(36).width(Fill), acts].spacing(6))
                .padding(Padding { top: 14.0, right: 16.0, bottom: 12.0, left: 16.0 })
                .width(Fill)
                .into();
        }
        // View mode.
        let saved = self.profiles.saved.contains_key(&key);
        let mut lbl = row![].spacing(8).align_y(Alignment::Center);
        if !label_.is_empty() {
            lbl = lbl.push(tx(label_.clone(), 13.0, t.ink2).line_height(lh(18.0)));
        }
        if saved {
            lbl = lbl.push(row![pgi("check", 13.0, t.accent_ink), semi(if self.status.online { "Saved" } else { "Saved \u{b7} syncs when connected" }, 13.0, t.accent_ink).line_height(lh(18.0))].spacing(3).align_y(Alignment::Center));
        }
        let mut txt = column![].spacing(1).width(Fill);
        if !label_.is_empty() || saved {
            txt = txt.push(lbl);
        }
        txt = match (&scalar, &value) {
            (Some(s), _) if !s.is_empty() => txt.push(tx(s.clone(), 15.0, t.ink).line_height(lh(21.0))),
            (None, Some(v)) => txt.push(value_view(t, v, 0)),
            _ => txt.push(tx(empty.to_string(), 15.0, t.ink2).line_height(lh(21.0))),
        };
        if key == "alias"
            && let Some(p) = self.profile_person()
            && let Some(a) = &p.alias
        {
            txt = txt.push(container(tx(format!("Shown to you as {a} \u{b7} their name is {}", p.name), 13.0, t.ink2).line_height(lh(18.0))).padding(Padding { top: 2.0, ..Padding::ZERO }));
        }
        let bare = label_.is_empty();
        let main: El = if editable {
            container(row![Space::new().height(44), txt].align_y(Alignment::Center)).padding([10, 16]).width(Fill).into()
        } else {
            container(txt).padding(if bare { [14, 16] } else { [12, 16] }).width(Fill).into()
        };
        let mut line = row![main].align_y(Alignment::Center).width(Fill);
        if kind == FieldKind::Link
            && let Some(u) = scalar.as_deref().and_then(pf::safe_url)
        {
            line = line.push(button(container(pgi("link", 20.0, t.ink2)).width(44).height(44).center_x(44).center_y(44)).padding(0)
                .style(|_, _| iced::widget::button::Style::default()).on_press(m(Action::AskLink(u))));
        }
        if editable {
            // The pencil is a pointer convenience inside the row's own press target.
            line = line.push(container(container(pgi("edit", 18.0, ui::alpha(t.ink2, 0.8))).width(44).height(44).center_x(44).center_y(44)).padding(Padding { right: 6.0, ..Padding::ZERO }));
        }
        if !readonly && optional && !editable {
            line = line.push(button(container(pgi("trash", 20.0, t.ink2)).width(44).height(44).center_x(44).center_y(44)).padding(0)
                .style(|_, _| iced::widget::button::Style::default()).on_press(m(Action::AskRemove(key.clone()))));
        }
        if editable {
            // `.line.live:hover`: the whole row washes (the card's corners clip it) and edits on press.
            return button(line).padding(0).width(Fill)
                .style(move |_, st| iced::widget::button::Style {
                    background: matches!(st, iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed).then_some(iced::Background::Color(t.surface2)),
                    border: Border { radius: r, ..Default::default() },
                    ..Default::default()
                })
                .on_press(m(Action::Edit(key.clone())))
                .into();
        }
        line.into()
    }

    fn person_profile(&self, t: Tok, did: &str) -> El<'_> {
        let Some(p) = self.contacts.iter().chain(self.kept.iter()).find(|c| c.did == did) else {
            return self.pf_scroll(t, container(tx("Person not found.", 15.0, t.ink2).line_height(lh(21.0))).padding([24, 20]).into(), 40.0);
        };
        let contact = self.contacts.iter().any(|c| c.did == did);
        let cache = self.profiles.cache.get(did);
        let name = p.name.clone();
        let first = name.split_whitespace().next().unwrap_or(&name).to_string();
        let blocked = self.blocks.blocked_by_me(did);
        let fields = cache.map(|c| c.fields.clone()).unwrap_or_default();
        let status = fields.get("status").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let mut hero = column![self.profile_avatar(t, &name, did, 112.0), container(bold(name.clone(), 22.0, t.ink).line_height(lh(28.0))).padding(Padding { top: 16.0, ..Padding::ZERO })].align_x(Alignment::Center);
        if let Some(s) = status {
            hero = hero.push(container(tx(s, 15.0, t.ink2).line_height(lh(21.0)).align_x(Alignment::Center)).max_width(320).padding(Padding { top: 4.0, ..Padding::ZERO }));
        }
        if cache.is_some() {
            // `.avslot`: the Busy chip's own reserved row (26px, 6 above), so it coming and going moves nothing.
            let av = fields.get("availability");
            let unavailable = av.and_then(|a| a.get("available")).and_then(Value::as_bool) == Some(false);
            let until = av.and_then(|a| a.get("until")).and_then(Value::as_u64).map(|u| if u > 10_000_000_000 { u / 1000 } else { u });
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let busy = unavailable && until.is_none_or(|u| u > now);
            let chip: El = if busy {
                let label = until.map(|u| format!("Busy until {}", pf::until_label(u))).unwrap_or_else(|| "Busy".into());
                container(semi(label, 13.0, t.warn_ink).line_height(lh(20.0))).padding([3, 12]).style(ui::plain(t.warn, 13.0)).into()
            } else {
                Space::new().into()
            };
            hero = hero.push(container(container(chip).width(Fill).height(26).center_x(Fill).center_y(26)).padding(Padding { top: 6.0, ..Padding::ZERO }));
        }
        if contact {
            use crate::blocks::CallGate;
            let (call_on, why) = match self.blocks.call_gate(did, true) {
                _ if blocked => (None, "You blocked them".to_string()),
                CallGate::Ok => (self.call.is_none().then(|| Msg::CallPressed(did.to_string())), String::new()),
                CallGate::NotAllowed => (None, format!("Messages only: calling needs {first}\u{2019}s permission")),
                CallGate::Closed(_) => (None, String::new()),
            };
            hero = hero.push(container(row![pg_btn(t, Btn::Primary, Some("chat"), "Message", (!blocked).then(|| Msg::Select(did.to_string()))), pg_btn(t, Btn::Outline, Some("phone"), "Call", call_on)].spacing(8)).padding(Padding { top: 14.0, ..Padding::ZERO }));
            // `.why`: reserved 19px line, 8 above: flipping Allow calls moves nothing.
            hero = hero.push(container(container(tx(why, 13.0, t.ink2).line_height(lh(19.0))).height(19)).padding(Padding { top: 8.0, ..Padding::ZERO }));
        }
        let mut c = column![container(hero).padding(Padding { top: 24.0, right: 20.0, bottom: 4.0, left: 20.0 }).width(Fill).center_x(Fill)].spacing(0);
        if blocked {
            c = c.push(container(container(row![pgi("off", 16.0, t.ink2), tx(format!("You blocked {first}. Their profile no longer updates."), 13.0, t.ink).line_height(lh(19.0))].spacing(8).align_y(Alignment::Center))
                .padding([10, 14]).width(Fill).style(ui::plain(t.surface2, 14.0))).padding(Padding { top: 12.0, right: 16.0, bottom: 0.0, left: 16.0 }));
        }
        if cache.is_none() {
            let line = if contact { format!("{first}\u{2019}s profile hasn\u{2019}t arrived yet. It shows here once it syncs.") } else { "No profile is available.".to_string() };
            c = c.push(container(tx(line, 13.0, t.ink2).line_height(lh(19.0)).align_x(Alignment::Center).width(Fill)).padding(Padding { top: 20.0, right: 24.0, bottom: 0.0, left: 24.0 }));
        } else {
            if let Some(about) = fields.get("about").filter(|v| v.as_str().is_some_and(|s| !s.is_empty())).cloned() {
                c = c.push(section(t, "About", vec![Box::new(move |r| self.field_row(t, "about", "", Some(about), FieldKind::Multiline, false, true, "", "", "", r))], vec![]));
            }
            let keys = pf::extra_keys(&fields);
            if !keys.is_empty() {
                let rows: Vec<RowFn> = keys
                    .into_iter()
                    .map(|k| -> RowFn {
                        let v = fields.get(&k).cloned();
                        let kind = if pf::known(&k).is_some_and(|x| x.1) { FieldKind::Link } else { FieldKind::Text };
                        Box::new(move |r| self.field_row(t, &k, &pf::label(&k), v, kind, false, true, "Not set", "", "", r))
                    })
                    .collect();
                c = c.push(section(t, "Additional information", rows, vec![]));
            }
        }
        if contact {
            let alias = p.alias.clone().map(Value::String);
            let allowed = self.blocks.calls_allowed(did);
            let d1 = did.to_string();
            let d2 = did.to_string();
            let first2 = first.clone();
            let rows: Vec<RowFn> = vec![
                Box::new(move |r| self.field_row(t, "alias", "Nickname", alias, FieldKind::Text, true, false, "Add a nickname", &self.profile_person().map(|p| p.name.clone()).unwrap_or_default(), "", r)),
                Box::new(move |r| pf_act(t, Some("phone"), "Allow calls".into(), Some(("Off means messages only".into(), t.ink2, false)), Some(pg_switch(t, allowed, Msg::AllowCalls(d1.clone(), !allowed))), None, false, r)),
                Box::new(move |r| {
                    if blocked {
                        pf_act(t, Some("off"), format!("Unblock {first2}"), None, None, Some(Msg::Unblock(d2.clone())), false, r)
                    } else {
                        pf_act(t, Some("off"), format!("Block {first2}"), None, None, Some(m(Action::AskBlock(d2.clone()))), true, r)
                    }
                }),
            ];
            // `.mine`: 36 above, a hairline inset 16, then 22 more before the heading.
            let mine = column![
                container(hairline(t)).padding([0, 16]),
                container(column![pf_h2(t, &format!("Your settings for {first}")), pf_card(t, rows), pf_note(t, format!("Only you see these. A nickname changes what you call {first}, not their profile."))])
                    .padding(Padding { top: 22.0, right: 16.0, bottom: 0.0, left: 16.0 }),
            ];
            c = c.push(container(mine).padding(Padding { top: 36.0, ..Padding::ZERO }));
        } else {
            c = c.push(container(tx(format!("{first} isn\u{2019}t one of your contacts. You know them through a group. Looking at a profile doesn\u{2019}t add anyone."), 13.0, t.ink2).line_height(lh(19.0)).align_x(Alignment::Center).width(Fill))
                .padding(Padding { top: 20.0, right: 24.0, bottom: 0.0, left: 24.0 }));
        }
        self.pf_scroll(t, c.into(), 40.0)
    }

    fn native_page(&self, t: Tok, tab: u8) -> El<'_> {
        if tab == 0 {
            let rows: Vec<RowFn> = [("mic", "Audio devices", 3u8), ("phone", "Video & call admission", 2), ("lock", "Security", 4), ("server", "Hosting", 6), ("info", "About & licences", 5), ("trash", "Accounts stored on this computer", 11)]
                .into_iter()
                .map(|(icon, name, id)| -> RowFn { Box::new(move |r| pf_act(t, Some(icon), name.into(), None, Some(chevron(t)), Some(m(Action::Open(Page::Native(id)))), false, r)) })
                .collect();
            return self.pf_scroll(t, section(t, "This computer", rows, vec!["These stay on this computer. They aren\u{2019}t part of your account.".into()]), 40.0);
        }
        self.native_settings_view(t)
    }

    /// A status that arrives by itself floats over the page bottom (`.pf-float`) instead of moving it.
    fn profile_float(&self, t: Tok) -> Option<El<'_>> {
        if self.profiles.page != Some(Page::Me) {
            return None;
        }
        let card: El = if !self.profiles.pic_error.is_empty() {
            ui::raised(
                container(row![tx(self.profiles.pic_error.clone(), 13.0, t.danger).line_height(lh(19.0)).width(Fill),
                    button(container(pgi("x", 18.0, t.danger)).width(44).height(44).center_x(44).center_y(44)).padding(0)
                        .style(wash(t, None, Some(ui::alpha(t.danger, 0.12)), 22.0)).on_press(m(Action::DismissPicError))]
                    .spacing(8).align_y(Alignment::Center))
                    .padding(Padding { top: 4.0, right: 4.0, bottom: 4.0, left: 12.0 })
                    .max_width(520).width(Fill),
                t, ui::Depth::Pop, ui::plain(t.danger_bg, 12.0))
        } else if self.profiles.offline_saves && !self.status.online {
            ui::raised(
                container(row![pgi("clock", 16.0, t.ink2), tx("Saved on this device. Will sync when you\u{2019}re connected.", 13.0, t.ink).line_height(lh(19.0))].spacing(8).align_y(Alignment::Center))
                    .padding([10, 14]).max_width(520).width(Fill),
                t, ui::Depth::Pop, ui::outlined(t.raised, t.hairline, 14.0))
        } else {
            return None;
        };
        Some(container(card).padding(16).width(Fill).height(Fill).align_bottom(Fill).center_x(Fill).into())
    }

    fn profile_layer(&self, t: Tok) -> Option<El<'_>> {
        let layer = self.profiles.layer.as_ref()?;
        let close = m(Action::CloseLayer);
        let cancel = |label: &str| pg_btn(t, Btn::Quiet, None, label, Some(m(Action::CloseLayer)));
        let content: El = match layer {
            Layer::Add { pick: None, .. } => {
                let have = self.own_profile().map(|c| c.fields.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
                let mut list = column![].spacing(0);
                for (k, label, _, _, icon) in pf::KNOWN.iter().filter(|k| !have.iter().any(|h| h == k.0)) {
                    list = list.push(pf_act(t, Some(icon), label.to_string(), None, None, Some(m(Action::AddPick(Some(k.to_string())))), false, 14.0.into()));
                }
                list = list.push(pf_act(t, Some("plus"), "Something else".into(), Some(("Name it yourself".into(), t.ink2, false)), None, Some(m(Action::AddPick(Some(String::new())))), false, 14.0.into()));
                pf_dlg(t, "Add information".into(), vec![para(t, "Your contacts will see it on your profile.".into()), container(list).padding(Padding { top: 2.0, right: -12.0, bottom: 0.0, left: -12.0 }).into()], vec![cancel("Cancel")])
            }
            Layer::Add { pick: Some(pick), name, value, error } => {
                let title = if pick.is_empty() { "Something else".to_string() } else { pf::label(pick) };
                let field_label = |s: String| container(semi(s, 13.0, t.ink2).line_height(lh(18.0))).padding(Padding { top: 6.0, ..Padding::ZERO });
                let mut form = column![].spacing(6);
                if pick.is_empty() {
                    form = form.push(field_label("What is it called?".into())).push(text_input("For example, Favourite tea", name).id(pf::ADD_NAME_ID).on_input(|v| m(Action::AddName(v))).on_submit(m(Action::AddSave))
                        .padding([10.5, 14.0]).size(15).line_height(lh(21.0)).font(ui::SANS).style(pf_input_style(t)));
                }
                let hint = pf::known(pick).map(|k| k.2).unwrap_or("");
                form = form.push(field_label(if pick.is_empty() { "Value".into() } else { pf::label(pick) })).push(text_input(hint, value).id(pf::ADD_VALUE_ID).on_input(|v| m(Action::AddValue(v))).on_submit(m(Action::AddSave))
                    .padding([10.5, 14.0]).size(15).line_height(lh(21.0)).font(ui::SANS).style(pf_input_style(t)));
                if !error.is_empty() {
                    form = form.push(container(pf_err(t, error.clone())).padding(Padding { top: 4.0, ..Padding::ZERO }));
                }
                pf_dlg(t, title, vec![container(form).padding(Padding { top: 4.0, ..Padding::ZERO }).into()],
                    vec![pg_btn(t, Btn::Quiet, None, "Back", Some(m(Action::AddPick(None)))), pg_btn(t, Btn::Primary, None, "Add", (!self.profiles.pending).then_some(m(Action::AddSave)))])
            }
            Layer::Picture { busy, error, crop, .. } => {
                let stage: El = match &self.profiles.preview {
                    Some(p) => image(p.stage.clone()).width(pf::VIEW).height(pf::VIEW).into(),
                    None => container(Space::new()).width(pf::VIEW).height(pf::VIEW).style(ui::plain(t.surface2, 14.0)).into(),
                };
                let stage = mouse_area(stage).on_press(m(Action::DragStart)).on_move(|p| m(Action::DragAt(p))).on_release(m(Action::DragEnd)).interaction(iced::mouse::Interaction::Grab);
                let rect = |px: f32| -> El<'_> { crate::ui::glyph(crate::pg_icons::STROKE, r#"<rect x="3" y="4" width="18" height="16" rx="3"/>"#, px, t.ink2) };
                let zoom = row![rect(16.0), slider(1.0..=4.0, crop.zoom, |z| m(Action::Zoom(z))).step(0.01).width(Fill), rect(22.0)].spacing(10).align_y(Alignment::Center).width(260).height(44);
                let mut sizes = row![].spacing(14).align_y(Alignment::End);
                for (i, px) in [72.0f32, 48.0, 40.0].into_iter().enumerate() {
                    sizes = sizes.push(match &self.profiles.preview {
                        Some(p) => image(p.sizes[i].clone()).width(px).height(px).into(),
                        None => El::from(container(Space::new()).width(px).height(px).style(ui::plain(t.surface2, px / 2.0))),
                    });
                }
                let mut c = column![
                    container(bold("Position your picture", 17.0, t.ink).line_height(lh(24.0))).width(Fill).padding(Padding { bottom: 14.0, ..Padding::ZERO }),
                    column![stage, tx("Drag to move the picture, or use the arrow keys. Plus and minus zoom.", 13.0, t.ink2).line_height(lh(19.0)).align_x(Alignment::Center), zoom].spacing(12).align_x(Alignment::Center),
                    container(sizes).padding(Padding { top: 6.0, ..Padding::ZERO }),
                ]
                .align_x(Alignment::Center);
                if self.own_profile().is_some_and(|c| c.hash.is_some()) {
                    c = c.push(container(tx(pf::PICTURE_HELP, 13.0, t.ink2).line_height(lh(19.0))).padding(Padding { top: 14.0, ..Padding::ZERO }).width(Fill));
                }
                if !error.is_empty() {
                    c = c.push(container(pf_err(t, error.clone())).padding(Padding { top: 10.0, ..Padding::ZERO }));
                }
                c = c.push(container(row![Space::new().width(Fill), cancel("Cancel"), pg_btn(t, Btn::Primary, None, "Use photo", (!busy && self.profiles.preview.is_some()).then_some(m(Action::UsePhoto)))].spacing(8)).padding(Padding { top: 18.0, ..Padding::ZERO }));
                container(c).padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 }).width(Fill).into()
            }
            Layer::RemovePhoto { error } => {
                let mut body = vec![lead(t, "Your initials will show instead.".into()), para(t, pf::PICTURE_HELP.into())];
                if !error.is_empty() {
                    body.push(pf_err(t, error.clone()));
                }
                pf_dlg(t, "Remove your picture?".into(), body, vec![cancel("Cancel"), pg_btn(t, Btn::Danger, None, "Remove picture", (!self.profiles.pending).then_some(m(Action::ConfirmRemovePhoto)))])
            }
            Layer::Link(u) => pf_dlg(t, "Open this link?".into(),
                vec![mono(u.clone(), 13.0, t.ink).line_height(lh(18.0)).into(), para(t, "It opens in your browser, outside Tinline. Tinline doesn\u{2019}t load it for you, and can\u{2019}t vouch for what\u{2019}s there.".into())],
                vec![cancel("Cancel"), pg_btn(t, Btn::Primary, None, "Open link", Some(m(Action::OpenLink)))]),
            Layer::Block(did) => {
                let name = self.contacts.iter().find(|c| &c.did == did).map(|c| c.name.split_whitespace().next().unwrap_or(&c.name).to_string()).unwrap_or_else(|| "them".into());
                pf_dlg(t, format!("Block {name}?"),
                    vec![lead(t, format!("You won\u{2019}t get calls or messages from {name}, and their profile updates stop.")), para(t, format!("Groups you share aren\u{2019}t affected. You can unblock {name} here at any time."))],
                    vec![cancel("Cancel"), pg_btn(t, Btn::Danger, None, "Block", Some(m(Action::ConfirmBlock)))])
            }
        };
        Some(pg_dialog(t, 1.0, None, content, close))
    }
}

pub(super) const FIELD_ID: &str = "profile-field";

#[derive(Clone, Copy, PartialEq)]
pub(super) enum FieldKind {
    Text,
    Multiline,
    Link,
}
