//! Settings, My profile and Person profile in the detail pane, after playground screens/settings
//! and screens/profile (profile.css furniture). Cards and buttons are CSS-radius surfaces in the
//! source (border-radius, not the smoothed shape), so iced radii are the faithful port here.
use super::*;
use super::super::profile_ui::{self as pf, Action, Layer, Page};
use super::shell_view::{pg_dialog, wash};
use crate::pg_icons::pgi;
use iced::border::Radius;
use iced::widget::{column, mouse_area, rich_text, slider, span, stack, text_editor};
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

/// people.svelte.ts `lastSeenText`.
fn last_seen(now: u64, at: Option<u64>) -> String {
    let Some(at) = at else { return "Not synced yet".into() };
    let m = (now.saturating_sub(at) as f64 / 60.0).round() as u64;
    if m < 2 { return "Last seen just now".into(); }
    if m < 60 { return format!("Last seen {m} minutes ago"); }
    let h = (m as f64 / 60.0).round() as u64;
    if h < 24 { return format!("Last seen {h} {} ago", if h == 1 { "hour" } else { "hours" }); }
    let d = (h as f64 / 24.0).round() as u64;
    format!("Last seen {d} {} ago", if d == 1 { "day" } else { "days" })
}

fn chevron<'a>(t: Tok) -> El<'a> {
    pgi("chevr", 18.0, t.ink2)
}

/// storage fmtBytes: GB with one decimal, MB and KB whole.
pub(super) fn fmt_bytes(n: u64) -> String {
    if n >= 1_000_000_000 { format!("{:.1} GB", n as f64 / 1e9) }
    else if n >= 1_000_000 { format!("{} MB", (n as f64 / 1e6).round()) }
    else if n >= 1000 { format!("{} KB", (n as f64 / 1e3).round()) }
    else { format!("{n} B") }
}

/// Settings radio row (Sounds / Storage / Video): 22px ring with a 12px accent dot, title, optional muted line.
pub(super) fn pf_radio<'a>(t: Tok, title: String, sub: Option<String>, on: bool, msg: Msg, height: f32, r: Radius) -> El<'a> {
    let radio = container(container(Space::new()).width(if on { 12 } else { 0 }).height(if on { 12 } else { 0 }).style(ui::plain(t.accent, 6.0)))
        .width(22).height(22).center_x(22).center_y(22)
        .style(move |_: &iced::Theme| iced::widget::container::Style { border: Border { color: if on { t.accent } else { t.line }, width: 2.0, radius: 11.0.into() }, ..Default::default() });
    let mut words = column![semi(title, 15.0, t.ink).line_height(lh(20.0))].spacing(1).width(Fill);
    if let Some(s) = sub { words = words.push(tx(s, 13.0, t.ink2).line_height(lh(18.0))); }
    button(row![radio, words].spacing(14).align_y(Alignment::Center).height(height - 16.0))
        .padding([8, 16]).height(height).width(Fill)
        .style(move |_, status| iced::widget::button::Style {
            background: matches!(status, iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed).then_some(iced::Background::Color(t.surface2)),
            border: Border { radius: r, ..Default::default() }, ..Default::default() })
        .on_press(msg).into()
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
        let base = ui::fingerprint_face(t, did, size).unwrap_or_else(|| avatar(t, name, if key == "me" { "me" } else { did }, size));
        if cached.is_some_and(|c| c.awaiting) {
            let ink = ui::fingerprint_ink(t, did).unwrap_or_else(|| ui::avatar_colors(t, if key == "me" { "me" } else { did }).1);
            let ring = move |_: &iced::Theme| iced::widget::container::Style {
                border: Border { color: ui::alpha(ink, 0.45 * 0.5), width: (size * 0.34 * 0.03).max(1.5), radius: (size / 2.0).into() },
                ..Default::default()
            };
            return stack![base, container(Space::new()).width(size).height(size).style(ring)].into();
        }
        base
    }

    pub(super) fn profile_shell(&self, t: Tok) -> El<'_> {
        self.framed(t, self.profile_page(t))
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
            Page::Notifications => ("Notifications", self.notifications_page(t)),
            Page::Sounds => ("Sounds", self.sounds_page(t)),
            Page::Video => ("Video", self.video_page(t)),
            Page::Storage => ("Storage", self.storage_page(t)),
            Page::About => ("About Tinline", self.about_page(t)),
            Page::Me => ("My profile", self.my_profile(t)),
            Page::Person(did) => ("Profile", self.person_profile(t, did)),
            Page::Native(1) => ("Devices", self.devices_page(t)),
            Page::Native(7) => ("Privacy", self.privacy_landing(t)),
            Page::Native(21) => ("Blocked people", self.blocked_page(t)),
            Page::Native(22) => ("Who can call me", self.calls_page(t)),
            Page::Native(23) => ("Recovery phrase", self.phrase_page(t)),
            Page::Native(24) => ("Passphrase", self.passphrase_page(t)),
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
        if let Some(d) = self.privacy_layer(t) {
            layers.push(d);
        }
        if let Some(d) = self.devices_layer(t) {
            layers.push(d);
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
        let areas: Vec<RowFn> = [("devices", "Devices", Page::Native(1)), ("shield", "Privacy", Page::Native(7)), ("bell", "Notifications", Page::Notifications), ("vol", "Sounds", Page::Sounds),
        ("video", "Video", Page::Video), ("save", "Storage", Page::Storage), ("info", "About Tinline", Page::About)]
            .into_iter()
            .map(|(icon, label, page)| -> RowFn { Box::new(move |r| pf_act(t, Some(icon), label.into(), None, Some(chevron(t)), Some(m(Action::Open(page))), false, r)) })
            .collect();
        // Settings.svelte LOOKS (2026-10-14): the same choice the sign-up switch makes; switches at once.
        let look = self.settings.appearance;
        let looks: Vec<RowFn> = [(None, "Match my computer", "Light or dark, as this computer is set"), (Some(false), "Light", ""), (Some(true), "Dark", "")]
            .into_iter().map(|(v, name, info): (Option<bool>, &'static str, &'static str)| -> RowFn<'_> {
            Box::new(move |r| {
                let on = look == v;
                let radio = container(container(Space::new()).width(if on { 12 } else { 0 }).height(if on { 12 } else { 0 }).style(ui::plain(t.accent, 6.0)))
                    .width(22).height(22).center_x(22).center_y(22)
                    .style(move |_: &iced::Theme| iced::widget::container::Style { border: Border { color: if on { t.accent } else { t.line }, width: 2.0, radius: 11.0.into() }, ..Default::default() });
                let mut words = column![semi(name, 15.0, t.ink).line_height(lh(20.0))].spacing(1).width(Fill);
                if !info.is_empty() { words = words.push(tx(info, 13.0, t.ink2).line_height(lh(18.0))); }
                button(row![radio, words].spacing(14).align_y(Alignment::Center).height(40))
                    .padding([8, 16]).height(56).width(Fill)
                    .style(move |_, status| iced::widget::button::Style {
                        background: matches!(status, iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed).then_some(iced::Background::Color(t.surface2)),
                        border: Border { radius: r, ..Default::default() }, ..Default::default() })
                    .on_press(Msg::Appearance(v)).into()
            })
        }).collect();
        let mut c = column![
            pf_sec(16.0, pf_card(t, vec![me_row])),
            pf_sec(24.0, column![pf_card(t, accounts),
                container(tx(format!("Only the selected account is online on this computer. Switching pauses the other one: it receives no calls until you select it again."), 13.0, t.ink2).line_height(lh(18.0)))
                    .padding(Padding { top: 8.0, right: 16.0, bottom: 0.0, left: 16.0 })].into()),
            pf_sec(24.0, column![pf_h2(t, "Appearance"), pf_card(t, looks)].into()),
            pf_sec(24.0, pf_card(t, areas)),
        ]
        .spacing(0);
        // Native-only, necessary on a computer: device audio, video admission, key storage. Not a playground area.
        c = c.push(section(t, "This computer", vec![Box::new(move |r| pf_act(t, Some("sliders"), "Audio, security and hosting".into(), Some(("Settings that belong to this computer only".into(), t.ink2, false)), Some(chevron(t)), Some(m(Action::Open(Page::Native(0)))), false, r))], vec![]));
        if self.notice.is_some() {
            c = c.push(container(self.notice_bar(t)).padding(16));
        }
        self.pf_scroll(t, c.into(), 40.0)
    }

    /// devices/Devices.svelte: this device, the devices linked to my account, and linking a new one. Each row opens
    /// its actions (Rename; Unlink for the others). The core has no device kind, so the others use the devices glyph.
    fn devices_page(&self, t: Tok) -> El<'_> {
        let now = super::now_secs();
        let live = |d: &p2pcore::LinkedDevice| d.last_seen.is_some_and(|s| now.saturating_sub(s) < 120);
        let stale = |d: &p2pcore::LinkedDevice| d.last_seen.is_none_or(|s| now.saturating_sub(s) >= 30 * 86400);
        let row = |d: &'_ p2pcore::LinkedDevice| -> RowFn<'_> {
            let name = if d.label.is_empty() { "Unnamed device".to_string() } else { d.label.clone() };
            let (sub, color, strong) = if d.this_device { ("This device \u{b7} Online".to_string(), t.ink2, false) }
                else if live(d) { ("Online now".into(), t.accent_ink, true) }
                else if let Some(s) = d.last_seen.filter(|_| stale(d)) { (format!("Not seen for {} days", now.saturating_sub(s) / 86400), t.warn_ink, true) }
                else { (last_seen(now, d.last_seen), t.ink2, false) };
            let icon = if d.this_device { "laptop" } else { "devices" };
            let dev = d.device.clone();
            Box::new(move |r| pf_act(t, Some(icon), name, Some((sub, color, strong)), Some(chevron(t)), Some(Msg::DevMenu(Some(dev))), false, r))
        };
        let me: Vec<RowFn> = self.linked.iter().filter(|d| d.this_device).map(row).collect();
        let mut others: Vec<RowFn> = self.linked.iter().filter(|d| !d.this_device).map(row).collect();
        let n_others = others.len();
        others.push(Box::new(move |r| pf_act(t, Some("plus"), "Link a new device".into(), Some(("Show a QR code on the new device, then approve it here".into(), t.ink2, false)),
            Some(chevron(t)), (!self.busy).then_some(Msg::LinkStartExisting), false, r)));
        let note = if n_others == 0 {
            "Only this device has your account. Link another to keep a second copy of your chats. Your recovery phrase brings back your account, not your messages."
        } else {
            "Every linked device is a full copy of you: it can read your chats and act as you. Names here are private to your devices; contacts never see them."
        };
        let mut c = column![];
        if !me.is_empty() {
            c = c.push(pf_sec(16.0, column![pf_h2(t, "This device"), pf_card(t, me)].into()));
        }
        c = c.push(section(t, "Linked devices", others, vec![note.into()]));
        self.pf_scroll(t, c.into(), 40.0)
    }

    /// Devices.svelte's layers: a device's actions, renaming it, unlinking it (honest: not a lock).
    fn devices_layer(&self, t: Tok) -> Option<El<'_>> {
        if self.profiles.page != Some(Page::Native(1)) {
            return None;
        }
        let find = |id: &str| self.linked.iter().find(|d| d.device == id);
        let name_of = |d: &p2pcore::LinkedDevice| if d.label.is_empty() { "Unnamed device".to_string() } else { d.label.clone() };
        let now = super::now_secs();
        let close = |m: Msg| pg_btn(t, Btn::Quiet, None, "Close", Some(m));
        if let Some(id) = &self.dev_menu {
            let d = find(id)?;
            let name = name_of(d);
            let sub = if d.this_device { "This device".to_string() } else if d.last_seen.is_some_and(|s| now.saturating_sub(s) < 120) { "Online now".into() } else { last_seen(now, d.last_seen) };
            let mut items = column![hairline(t), pf_act(t, Some("edit"), "Rename".into(), None, None, Some(Msg::DevRenameStart(d.device.clone())), false, 0.0.into())];
            if !d.this_device {
                items = items.push(hairline(t)).push(pf_act(t, Some("leave"), "Unlink".into(), None, None, Some(Msg::UnlinkAsk(d.device.clone())), true, 0.0.into()));
            }
            let body = vec![para(t, sub), container(items).padding(Padding { top: 4.0, right: -20.0, bottom: 0.0, left: -20.0 }).into()];
            return Some(pg_dialog(t, 1.0, None, pf_dlg(t, name, body, vec![close(Msg::DevMenu(None))]), Msg::DevMenu(None)));
        }
        if let Some((_, text)) = &self.dev_rename {
            let ok = crate::flow::valid_label(text).is_some();
            let field = column![semi("Name", 13.0, t.ink).line_height(lh(18.0)),
                text_input("", text).on_input(Msg::DevRenameIn).on_submit(Msg::DevRenameSave).padding([10.5, 14.0]).size(15).line_height(lh(21.0)).style(pf_input_style(t))].spacing(6);
            let body = vec![para(t, "Only your own devices see this name.".into()), container(field).padding(Padding { top: 4.0, ..Padding::ZERO }).into()];
            return Some(pg_dialog(t, 1.0, None, pf_dlg(t, "Rename device".into(), body,
                vec![pg_btn(t, Btn::Quiet, None, "Cancel", Some(Msg::DevRenameCancel)), pg_btn(t, Btn::Primary, None, "Save", ok.then_some(Msg::DevRenameSave))]), Msg::DevRenameCancel));
        }
        if let Some(id) = &self.unlink_ask {
            let d = find(id)?;
            if d.this_device {
                return None;
            }
            let name = name_of(d);
            let online = d.last_seen.is_some_and(|s| now.saturating_sub(s) < 120);
            let notlock = container(row![pgi("warning", 18.0, t.warn_ink),
                    rich_text![span::<(), iced::Font>("This is not a lock.").font(ui::SANS_SEMI).color(t.warn_ink),
                        span::<(), iced::Font>(format!(" Anyone who still has your recovery phrase, or can unlock {name}, can still act as you. If it is lost or stolen, the only full fix is a new account.")).color(t.warn_ink)]
                        .size(13).line_height(lh(19.0))].spacing(10))
                .padding([10, 12]).width(Fill).style(ui::plain(t.warn, 12.0));
            let mut body = vec![
                lead(t, format!("{name} will stop syncing with your account and stop ringing when you\u{2019}re called. {}", if online { "Tinline tells it now." } else { "It is offline, so it finds out the next time it connects." })),
                notlock.into(),
                para(t, "Contacts you haven\u{2019}t blocked get your updated device list when they\u{2019}re next online.".into()),
            ];
            if self.has_pass {
                body.push(self.pf_pass(t, "Enter your passphrase to confirm", &self.link.pass, Msg::LinkPassIn, Some(Msg::UnlinkGo)));
            }
            let go = (!self.busy && (!self.has_pass || !self.link.pass.is_empty())).then_some(Msg::UnlinkGo);
            return Some(pg_dialog(t, 1.0, None, pf_dlg(t, format!("Unlink {name}?"), body,
                vec![pg_btn(t, Btn::Quiet, None, "Cancel", Some(Msg::UnlinkCancel)), pg_btn(t, Btn::Danger, None, if self.busy { "Unlinking\u{2026}" } else { "Unlink" }, go)]), Msg::UnlinkCancel));
        }
        None
    }

    /// privacy/Privacy.svelte: one row per area. Read receipts and online visibility aren't offered: the core has neither.
    fn privacy_landing(&self, t: Tok) -> El<'_> {
        let blocked = self.blocks.list.len();
        let msg_only = self.contacts.iter().filter(|c| !self.blocks.blocked_by_me(&c.did) && !self.blocks.calls_allowed(&c.did)).count();
        let rows_: [(&'static str, &'static str, String, u8); 4] = [
            ("off", "Blocked people", if blocked == 0 { "No one".into() } else { format!("{blocked} {}", if blocked == 1 { "person" } else { "people" }) }, 21),
            ("phone", "Who can call me", if msg_only == 0 { "All contacts can call".into() } else { format!("{msg_only} {} messages only", if msg_only == 1 { "contact is" } else { "contacts are" }) }, 22),
            ("key", "Recovery phrase", if self.has_pass { "Needs your passphrase".into() } else { "Shown after you confirm".into() }, 23),
            ("lock", "Passphrase", "Protects Tinline on this device".into(), 24),
        ];
        let rows: Vec<RowFn> = rows_.into_iter().map(|(icon, label, sub, id)| -> RowFn<'_> {
            Box::new(move |r| pf_act(t, Some(icon), label.into(), Some((sub, t.ink2, false)), Some(chevron(t)), Some(m(Action::Open(Page::Native(id)))), false, r))
        }).collect();
        let c = pf_sec(16.0, column![pf_card(t, rows),
            pf_note(t, "Your contacts see your name, picture and profile. Tinline doesn\u{2019}t hide who you talk to, and every device linked to your account can read your chats.")].into());
        self.pf_scroll(t, c, 40.0)
    }

    /// PersonRow inside a card: avatar 44, name, a line under it, something at the end; 12 px side padding.
    fn person_row<'a>(&'a self, t: Tok, did: &str, name: String, sub: String, end: El<'a>, r: Radius) -> El<'a> {
        let _ = r;
        container(row![self.profile_avatar(t, &name, did, 44.0),
                column![semi(name, 15.0, t.ink).line_height(lh(21.0)).wrapping(iced::widget::text::Wrapping::None), tx(sub, 13.0, t.ink2).line_height(lh(18.0))].spacing(1).width(Fill),
                end].spacing(12).align_y(Alignment::Center).height(64))
            .padding([0, 12]).width(Fill).into()
    }

    /// privacy/Blocked.svelte: the people I blocked (Unblock), "Block someone", and an honest note.
    fn blocked_page(&self, t: Tok) -> El<'_> {
        let mut list: Vec<&p2pcore::BlockedContact> = self.blocks.list.iter().collect();
        list.sort_by_key(|b| b.name.to_lowercase());
        let mut rows: Vec<RowFn> = list.into_iter().map(|b| -> RowFn<'_> {
            let first = b.name.split_whitespace().next().unwrap_or(&b.name).to_string();
            let _ = first;
            Box::new(move |r| self.person_row(t, &b.did, b.name.clone(), "Told you blocked them".into(),
                pg_btn(t, Btn::Outline, None, "Unblock", Some(Msg::Privacy(crate::app::PvMsg::Unblock(Some(b.did.clone()))))), r))
        }).collect();
        if rows.is_empty() {
            rows.push(Box::new(move |_| container(tx("No one is blocked.", 13.0, t.ink2)).padding([18, 16]).width(Fill).center_x(Fill).into()));
        }
        rows.push(Box::new(move |r| pf_act(t, Some("off"), "Block someone".into(), None, None, Some(Msg::Privacy(crate::app::PvMsg::Pick(true))), false, r)));
        let c = pf_sec(16.0, column![pf_h2(t, "Blocked"), pf_card(t, rows),
            pf_note(t, "Blocking is not silent. The person is told, their app stops contacting you and removes you from their contacts. Your chat history stays on your devices. Groups you share aren\u{2019}t affected.")].into());
        self.pf_scroll(t, c, 40.0)
    }

    /// privacy/CallsAllowed.svelte: each contact's Allow calls (the same switch as on their profile). The
    /// playground's "New contacts can call me" default has no core setting yet, so it isn't shown.
    fn calls_page(&self, t: Tok) -> El<'_> {
        let mut people: Vec<&p2pcore::Contact> = self.contacts.iter().filter(|c| !self.blocks.blocked_by_me(&c.did)).collect();
        people.sort_by_key(|c| Self::display(c).to_lowercase());
        let rows: Vec<RowFn> = people.into_iter().map(|c| -> RowFn<'_> {
            let on = self.blocks.calls_allowed(&c.did);
            Box::new(move |r| self.person_row(t, &c.did, Self::display(c), if on { "Can call you".into() } else { "Messages only".into() },
                pg_switch(t, on, Msg::AllowCalls(c.did.clone(), !on)), r))
        }).collect();
        let c = pf_sec(16.0, column![pf_h2(t, "Contacts"), pf_card(t, rows),
            pf_note(t, "Someone you allowed can ring you; someone on messages only can still write. People you blocked can do neither.")].into());
        self.pf_scroll(t, c, 40.0)
    }

    /// privacy/Phrase.svelte: behind the passphrase; then the 24 words, hidden until asked, hiding again after
    /// 30 s; no copy button on purpose.
    fn phrase_page(&self, t: Tok) -> El<'_> {
        let lead_ = |s: &str| -> El<'_> { container(tx(s.to_string(), 15.0, t.ink).line_height(lh(21.0))).padding(Padding { top: 0.0, right: 4.0, bottom: 12.0, left: 4.0 }).into() };
        let Some(words) = self.revealed.as_ref() else {
            let mut c = column![lead_("Your recovery phrase is the only way to get your account back if you lose every device. Anyone who has it can act as you.")].spacing(6);
            if self.has_pass {
                c = c.push(self.pf_pass(t, "Enter your passphrase to continue", &self.pass_in, Msg::PassIn, Some(Msg::RevealSubmit)));
            }
            if let Some(n) = self.notice.as_ref().filter(|_| self.has_pass) {
                c = c.push(tx(n.clone(), 13.0, t.danger).line_height(lh(19.0)));
            }
            c = c.push(container(pg_btn(t, Btn::Primary, None, if self.busy { "Checking\u{2026}" } else { "Continue" }, (!self.busy && (!self.has_pass || !self.pass_in.is_empty())).then_some(Msg::RevealSubmit))).align_right(Fill).padding(Padding { top: 6.0, ..Padding::ZERO }));
            return self.pf_scroll(t, pf_sec(16.0, c.into()), 40.0);
        };
        let shown = self.pv.shown;
        let as_key = self.pv.as_key;
        let seg = super::signup_view::words_key_seg(t, as_key, |k| Msg::Privacy(crate::app::PvMsg::AsKey(k)));
        if as_key {
            // The key: the same secret as one line, for a password manager, so it alone can be copied.
            let key = p2pcore::recovery_key_from_phrase(words.to_string()).unwrap_or_default();
            let shown_key: El = if shown { super::signup_view::key_box(t, key) } else { super::signup_view::key_box(t, "\u{2022}".repeat(24)) };
            let left = self.pv.shown_at.map(|a| 30u64.saturating_sub(a.elapsed().as_secs())).unwrap_or(30);
            let bar = row![pg_btn(t, if shown { Btn::Outline } else { Btn::Primary }, Some(if shown { "eyeoff" } else { "eye" }), if shown { "Hide key" } else { "Show key" }, Some(Msg::Privacy(crate::app::PvMsg::Words(!shown)))),
                pg_btn(t, Btn::Outline, Some("copy"), "Copy my key", Some(Msg::Privacy(crate::app::PvMsg::CopyKey))),
                tx(if shown { format!("Hides in {left} s") } else { String::new() }, 13.0, t.ink2)].spacing(12).align_y(Alignment::Center);
            let c = column![lead_("Your recovery key is the same as your 24 words, as one line. A password manager is a good place for it."), container(seg).padding(Padding { bottom: 10.0, ..Padding::ZERO }), shown_key,
                container(bar).padding(Padding { top: 12.0, ..Padding::ZERO }),
                pf_note(t, "Anyone who has it can act as you, just like the words. Tinline never asks for it except to restore your account.")];
            return self.pf_scroll(t, pf_sec(16.0, c.into()), 40.0);
        }
        let mut grid = column![].spacing(0);
        let list: Vec<&str> = words.split_whitespace().collect();
        for chunk in list.chunks(3).enumerate() {
            let (ri, ws) = chunk;
            let mut r = row![].spacing(0);
            for (ci, w) in ws.iter().enumerate() {
                let n = ri * 3 + ci + 1;
                r = r.push(row![container(tx(n.to_string(), 12.0, t.ink2)).width(22).align_right(22),
                        if shown { El::from(semi(w.to_string(), 15.0, t.ink)) } else { El::from(bold("\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}", 15.0, t.ink2)) }]
                    .spacing(8).align_y(Alignment::Center).width(Fill).height(36));
            }
            grid = grid.push(r);
        }
        let card = container(grid).padding([8, 16]).width(Fill).style(move |_: &iced::Theme| iced::widget::container::Style {
            background: Some(t.surface.into()), border: Border { color: t.hairline, width: 1.0, radius: 14.0.into() }, ..Default::default() });
        let left = self.pv.shown_at.map(|a| 30u64.saturating_sub(a.elapsed().as_secs())).unwrap_or(30);
        let bar = row![pg_btn(t, if shown { Btn::Outline } else { Btn::Primary }, Some(if shown { "eyeoff" } else { "eye" }), if shown { "Hide words" } else { "Show words" }, Some(Msg::Privacy(crate::app::PvMsg::Words(!shown)))),
            tx(if shown { format!("Hides in {left} s") } else { String::new() }, 13.0, t.ink2)].spacing(12).align_y(Alignment::Center);
        let c = column![lead_("Write these 24 words down, in order, and keep them somewhere private."), container(seg).padding(Padding { bottom: 10.0, ..Padding::ZERO }), card, container(bar).padding(Padding { top: 12.0, ..Padding::ZERO }),
            pf_note(t, "There is no copy button on purpose: the phrase stays off the clipboard. Tinline never asks for it except to restore your account. The phrase brings back your account, not your messages.")];
        self.pf_scroll(t, pf_sec(16.0, c.into()), 40.0)
    }

    /// privacy/ChangePass.svelte: current (when there is one), new, new again; "Passphrase changed" in place.
    fn passphrase_page(&self, t: Tok) -> El<'_> {
        let e = &self.pv.errs;
        let err = |s: &str| -> El<'_> { container(tx(s.to_string(), 13.0, t.danger).line_height(lh(19.0))).height(19).into() };
        let mut f = column![].spacing(6);
        if self.has_pass {
            f = f.push(self.pf_pass(t, "Current passphrase", &self.old_in, Msg::OldIn, None)).push(err(&e[0]));
        }
        f = f.push(self.pf_pass(t, "New passphrase", &self.pass_in, Msg::PassIn, None)).push(if e[1].is_empty() { container(tx("Any length works. A few words make it stronger.", 13.0, t.ink2).line_height(lh(19.0))).height(19).into() } else { err(&e[1]) })
            .push(self.pf_pass(t, "New passphrase again", &self.pv.again, |v| Msg::Privacy(crate::app::PvMsg::Again(v)), Some(Msg::Privacy(crate::app::PvMsg::Change)))).push(err(&e[2]));
        let saved: El = if self.pv.done { row![pgi("check", 16.0, t.accent_ink), semi("Passphrase changed", 13.0, t.accent_ink)].spacing(6).align_y(Alignment::Center).into() } else { Space::new().height(22).into() };
        let ready = !self.busy && !self.pass_in.is_empty() && !self.pv.again.is_empty();
        f = f.push(row![container(saved).width(Fill), pg_btn(t, Btn::Primary, None, if self.has_pass { "Change passphrase" } else { "Add passphrase" }, ready.then_some(Msg::Privacy(crate::app::PvMsg::Change)))].align_y(Alignment::Center));
        // Remember on this computer (ChangePass.svelte): the switch, or why there is none.
        let remember: Option<El> = self.has_pass.then(|| {
            let row_: RowFn = if self.remember_avail == Some(false) {
                Box::new(move |r| pf_act(t, Some("key"), "Remember on this computer".into(),
                    Some(("This computer has no keyring, so Tinline asks for your passphrase each time it starts.".into(), t.ink2, false)), None, None, false, r))
            } else {
                let on = self.remember;
                Box::new(move |r| pf_act(t, Some("key"), "Remember on this computer".into(),
                    Some(("Skips the passphrase when Tinline starts, using this computer\u{2019}s keyring.".into(), t.ink2, false)),
                    Some(pg_switch(t, on, Msg::Remember(!on))), None, false, r))
            };
            pf_sec(16.0, pf_card(t, vec![row_]))
        });
        let c = column![f, pf_note(t, "The passphrase protects Tinline on this device only; your other devices keep their own. Tinline can\u{2019}t recover a forgotten passphrase. If that happens, your recovery phrase restores your account, though not your messages.")];
        let mut page = column![].spacing(0);
        if let Some(r) = remember { page = page.push(r); }
        self.pf_scroll(t, page.push(pf_sec(if self.has_pass { 24.0 } else { 16.0 }, c.into())).into(), 40.0)
    }

    /// people/PassField.svelte: label 13 semibold, a 44 field (--r-ctl, --line, accent when focused) with the eye
    /// at its end (show / hide the text).
    fn pf_pass<'a>(&'a self, t: Tok, label_: &'a str, value: &'a str, on: impl Fn(String) -> Msg + 'a, submit: Option<Msg>) -> El<'a> {
        let shown = self.pv.reveal;
        let mut input = text_input("", value).secure(!shown).on_input(on).padding(Padding { top: 11.5, right: 48.0, bottom: 11.5, left: 14.0 }).size(15).line_height(lh(21.0)).style(pf_input_style(t));
        if let Some(m) = submit { input = input.on_submit(m); }
        let eye = button(container(pgi(if shown { "eyeoff" } else { "eye" }, 20.0, t.ink2)).center_x(44).center_y(44)).padding(0).width(44).height(44)
            .style(wash(t, None, Some(t.surface2), 22.0)).on_press(Msg::Privacy(crate::app::PvMsg::Eye));
        column![semi(label_, 13.0, t.ink).line_height(lh(18.0)), stack![input, container(eye).align_right(Fill)]].spacing(6).into()
    }

    /// Blocked.svelte's layers: pick someone to block (then the profile's Block dialog), and "Unblock {name}?".
    fn privacy_layer(&self, t: Tok) -> Option<El<'_>> {
        use crate::app::PvMsg;
        if self.profiles.page != Some(Page::Native(21)) {
            return None;
        }
        if self.pv.pick {
            let mut people: Vec<&p2pcore::Contact> = self.contacts.iter().filter(|c| !self.blocks.blocked_by_me(&c.did)).collect();
            people.sort_by_key(|c| Self::display(c).to_lowercase());
            let mut list = column![].spacing(0);
            for c in &people {
                list = list.push(button(row![self.profile_avatar(t, &Self::display(c), &c.did, 44.0), semi(Self::display(c), 15.0, t.ink)].spacing(12).align_y(Alignment::Center).height(64))
                    .padding([0, 8]).width(Fill).style(wash(t, None, Some(t.surface2), 14.0)).on_press(Msg::Privacy(PvMsg::PickPerson(c.did.clone()))));
            }
            if people.is_empty() {
                list = list.push(container(tx("No one left to block.", 13.0, t.ink2)).padding([18, 16]).width(Fill).center_x(Fill));
            }
            let body = vec![para(t, "Pick a contact.".into()), container(iced::widget::scrollable(list).height(Length::Shrink)).max_height(320).into()];
            return Some(pg_dialog(t, 1.0, None, pf_dlg(t, "Block someone".into(), body, vec![pg_btn(t, Btn::Quiet, None, "Cancel", Some(Msg::Privacy(PvMsg::Pick(false))))]), Msg::Privacy(PvMsg::Pick(false))));
        }
        if let Some(did) = &self.pv.unblock {
            let name = self.blocks.list.iter().find(|b| &b.did == did).map(|b| b.name.clone()).unwrap_or_default();
            let first = name.split_whitespace().next().unwrap_or(&name).to_string();
            let body = vec![lead(t, format!("{first}\u{2019}s app can reach you again. It removed you from their contacts when you blocked them, so you\u{2019}ll both need to add each other again before you can message or call.")),
                para(t, "Your chat history is still here.".into())];
            return Some(pg_dialog(t, 1.0, None, pf_dlg(t, format!("Unblock {first}?"), body,
                vec![pg_btn(t, Btn::Quiet, None, "Cancel", Some(Msg::Privacy(PvMsg::Unblock(None)))), pg_btn(t, Btn::Primary, None, "Unblock", Some(Msg::Privacy(PvMsg::UnblockGo)))]), Msg::Privacy(PvMsg::Unblock(None))));
        }
        None
    }

    /// settings/Notifications.svelte: "Alert me about" (Messages, Calls, Groups), then Permissions. On this
    /// computer the system has no notification permission to ask for, and the microphone is "allowed" when one is
    /// found; there is no battery row (the phone's).
    fn notifications_page(&self, t: Tok) -> El<'_> {
        use crate::app::Alerts;
        let st = &self.settings;
        let sw = |icon: &'static str, title: &'static str, sub: &'static str, on: bool, f: fn(bool) -> Alerts| -> RowFn<'_> {
            Box::new(move |r| pf_act(t, Some(icon), title.into(), Some((sub.into(), t.ink2, false)), Some(pg_switch(t, on, Msg::Alerts(f(!on)))), None, false, r))
        };
        let alerts: Vec<RowFn> = vec![
            sw("chat", "Messages", "From people, not groups", !st.no_notify_messages, Alerts::Messages),
            sw("phone", "Calls", "Ringing when someone calls", !st.no_notify_calls, Alerts::Calls),
            sw("users", "Groups", "Posts, invites and requests", !st.no_notify_groups, Alerts::Groups),
        ];
        let mic = !self.devices.0.is_empty();
        let perm = |icon: &'static str, title: &'static str, ok: bool, no: &'static str| -> RowFn<'_> {
            Box::new(move |r| pf_act(t, Some(icon), title.into(), Some((if ok { "Allowed".into() } else { no.into() }, if ok { t.ink2 } else { t.warn_ink }, !ok)), None, None, false, r))
        };
        let perms: Vec<RowFn> = vec![perm("bell", "Notifications", true, ""), perm("mic", "Microphone", mic, "No microphone found")];
        let c = column![
            pf_sec(12.0, column![pf_h2(t, "Alert me about"), pf_card(t, alerts),
                pf_note(t, "A muted chat never alerts you, whatever is switched on here. Missed calls from one person fold into one notification.")].into()),
            section(t, "Permissions", perms, vec!["These are set by your system, not by Tinline. Tinline asks only when it needs one.".into()]),
        ];
        self.pf_scroll(t, c.into(), 40.0)
    }

    /// settings/Sounds.svelte: Quiet, then the call sound set (radios; picking one plays its ringtone once),
    /// preview chips and a note that says whether previews can be heard.
    fn sounds_page(&self, t: Tok) -> El<'_> {
        use crate::app::Alerts;
        use crate::cues::Palette;
        let quiet = self.settings.quiet;
        let quiet_row: Vec<RowFn> = vec![Box::new(move |r| pf_act(t, Some("mute"), "Quiet".into(), Some(("No sounds from Tinline: ringing, call tones, joins".into(), t.ink2, false)),
            Some(pg_switch(t, quiet, Msg::Alerts(Alerts::Quiet(!quiet)))), None, false, r))];
        let cur = Palette::from_key(&self.settings.palette);
        let radios: Vec<RowFn> = Palette::ALL.into_iter().map(|p| -> RowFn<'_> {
            Box::new(move |r| {
                let (name, info) = p.label();
                let on = p == cur;
                // .radio: 22 round, 2px ring (--line, --accent when on), 12px accent dot.
                let radio = container(container(Space::new()).width(if on { 12 } else { 0 }).height(if on { 12 } else { 0 }).style(ui::plain(t.accent, 6.0)))
                    .width(22).height(22).center_x(22).center_y(22)
                    .style(move |_: &iced::Theme| iced::widget::container::Style { border: Border { color: if on { t.accent } else { t.line }, width: 2.0, radius: 11.0.into() }, ..Default::default() });
                button(row![radio, column![semi(name, 15.0, t.ink).line_height(lh(20.0)), tx(info, 13.0, t.ink2).line_height(lh(18.0))].spacing(1).width(Fill)]
                        .spacing(14).align_y(Alignment::Center).height(48))
                    .padding([8, 16]).height(64).width(Fill)
                    .style(move |_, status| iced::widget::button::Style {
                        background: matches!(status, iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed).then_some(iced::Background::Color(t.surface2)),
                        border: Border { radius: r, ..Default::default() }, ..Default::default() })
                    .on_press(Msg::Alerts(Alerts::Palette(p))).into()
            })
        }).collect();
        let chip = |label: &'static str, kind: &'static str| -> El<'_> {
            button(row![pgi("vol", 16.0, t.ink), semi(label, 13.0, t.ink).line_height(lh(18.0))].spacing(6).align_y(Alignment::Center))
                .padding([0, 14]).height(32)
                .style(move |_, status| iced::widget::button::Style {
                    // tokens.css .t-chip: transparent, 1px --line, 16 radius.
                    background: matches!(status, iced::widget::button::Status::Hovered).then_some(t.surface2.into()),
                    border: Border { color: t.line, width: 1.0, radius: 16.0.into() }, ..Default::default() })
                .on_press(Msg::Alerts(Alerts::Preview(kind))).into()
        };
        let chips = container(row![chip("Ringtone", "ring"), chip("Connected", "connected"), chip("Call ended", "ended")].spacing(8)).padding(Padding { top: 12.0, right: 4.0, bottom: 0.0, left: 4.0 });
        let note = if quiet { "Quiet is on, so previews are silent." } else { "Tap a sound to hear it. The choice applies to the next call." };
        let c = column![
            pf_sec(16.0, column![pf_h2(t, "Quiet"), pf_card(t, quiet_row)].into()),
            pf_sec(24.0, column![pf_h2(t, "Call sounds"), pf_card(t, radios), chips, pf_note(t, note)].into()),
        ];
        self.pf_scroll(t, c.into(), 40.0)
    }

    /// Settings > Video (CallsVideo.svelte): "Sharper video" (each call starts this way; the HD chip changes one call), then the computer's cameras.
    fn video_page(&self, t: Tok) -> El<'_> {
        let hd = self.settings.prefer_hd;
        let sub = if hd { "Up to 720p when the connection allows. Uses more data and battery." } else { "Adjusts by itself for smooth calls." };
        let quality: Vec<RowFn> = vec![Box::new(move |r| pf_act(t, Some("video"), "Sharper video".into(), Some((sub.into(), t.ink2, false)),
            Some(pg_switch(t, hd, Msg::Video(super::super::Vm::PreferHd(!hd)))), None, false, r))];
        let note = "A new camera is used the next time your camera starts.";
        // The saved choice may be unplugged right now; it stays listed (selected) until the person picks another.
        let mut cams: Vec<crate::camera::Info> = self.video.devices.clone();
        if let Some(saved) = self.settings.camera_device.as_ref().filter(|s| !cams.iter().any(|c| &c.path == *s)) {
            cams.push(crate::camera::Info { path: saved.clone(), label: saved.clone(), usb: false });
        }
        let chosen = self.settings.camera_device.clone().or_else(|| cams.first().map(|c| c.path.clone()));
        let mut c = column![pf_sec(16.0, column![pf_h2(t, "Video quality"), pf_card(t, quality), pf_note(t, "Every video call starts this way. During a call, tap HD to change it for that call.")].into())];
        let rows: Vec<RowFn> = cams.into_iter().map(|cam| -> RowFn<'_> {
            let on = chosen.as_deref() == Some(cam.path.as_str());
            Box::new(move |r| pf_radio(t, cam.label, cam.usb.then(|| "USB".to_string()), on, Msg::Video(super::super::Vm::SelectCamera(cam.path)), 56.0, r))
        }).collect();
        if rows.is_empty() {
            c = c.push(pf_sec(24.0, column![pf_h2(t, "Camera"), pf_note(t, "No camera found. Plug one in and open this page again."), pf_note(t, note)].into()));
        } else {
            c = c.push(pf_sec(24.0, column![pf_h2(t, "Camera"), pf_card(t, rows), pf_note(t, note)].into()));
        }
        self.pf_scroll(t, c.into(), 40.0)
    }

    /// Settings > Storage (Storage.svelte): what Tinline keeps here by kind, when pictures download, clearing the cache.
    /// Sizes are measured (attachments by type, the chat file, the decrypted copies cache).
    fn storage_page(&self, t: Tok) -> El<'_> {
        use p2pcore::PictureDownload as Dl;
        let info = self.profiles.storage;
        let kinds: [(&str, u64, Color); 4] = match info {
            Some(i) => [("Pictures", i.usage.pictures, t.accent), ("Files", i.usage.files, t.thread), ("Messages", i.usage.messages, t.ink2), ("Cache", i.cache, t.line)],
            None => [("Pictures", 0, t.accent), ("Files", 0, t.thread), ("Messages", 0, t.ink2), ("Cache", 0, t.line)],
        };
        let total: u64 = kinds.iter().map(|k| k.1).sum();
        // .bar: 10 high, 5 radius, 2 gap, a 3px floor per kind present.
        let mut bar = row![].spacing(2).height(10);
        for (_, n, c) in kinds.iter().filter(|k| k.1 > 0) {
            bar = bar.push(container(Space::new()).width(Length::FillPortion(((*n as f64 / total as f64) * 1000.0).ceil().max(1.0) as u16)).height(10).style(ui::plain(*c, 0.0)));
        }
        let bar = container(bar).width(Fill).height(10).clip(true).style(ui::plain(t.surface2, 5.0));
        let mut legend = column![].spacing(6);
        for (label, n, c) in kinds {
            legend = legend.push(row![container(Space::new()).width(10).height(10).style(ui::plain(c, 3.0)), tx(label, 13.0, t.ink).line_height(lh(18.0)),
                Space::new().width(Fill), tx(fmt_bytes(n), 13.0, t.ink2).line_height(lh(18.0))].spacing(8).align_y(Alignment::Center));
        }
        let use_card: RowFn = Box::new(move |_| container(column![tx(fmt_bytes(total), 17.0, t.ink).line_height(lh(24.0)), bar, legend].spacing(12)).padding(16).width(Fill).into());
        let cur = info.map(|i| i.download).unwrap_or_default();
        // The core cannot see whether this computer is on Wi-Fi, so "On Wi-Fi" is a phone choice only.
        let dl: Vec<RowFn> = [(Dl::Always, "Always", "On Wi-Fi and mobile data"), (Dl::Tap, "When I tap them", "Each shows a soft preview until then")]
            .into_iter().map(|(d, name, sub)| -> RowFn<'_> { Box::new(move |r| pf_radio(t, name.into(), Some(sub.into()), cur == d, m(Action::SetDownload(d)), 56.0, r)) }).collect();
        let cache = info.map(|i| i.cache).unwrap_or(0);
        let cleared = self.profiles.cache_cleared;
        let sub = if cache > 0 { format!("{} of pictures and files you\u{2019}ve opened. They\u{2019}re prepared again when you open them.", fmt_bytes(cache)) }
            else if cleared { "Cleared".to_string() } else { "Nothing to clear".to_string() };
        let clear: RowFn = Box::new(move |r| pf_act(t, Some("trash"), "Clear cache".into(), Some((sub, t.ink2, false)), None, (cache > 0).then(|| m(Action::ClearCache)), false, r));
        let c = column![
            pf_sec(16.0, column![pf_h2(t, "On this computer"), pf_card(t, vec![use_card])].into()),
            pf_sec(24.0, column![pf_h2(t, "Download pictures"), pf_card(t, dl)].into()),
            pf_sec(24.0, column![pf_card(t, vec![clear]), pf_note(t, "To free more space, remove pictures or files from their chat.")].into()),
        ];
        self.pf_scroll(t, c.into(), 40.0)
    }

    /// Settings > About Tinline (About.svelte): the mark, the version, who makes it, the open-source libraries, the source.
    fn about_page(&self, t: Tok) -> El<'_> {
        let open_ = self.profiles.licences_open;
        let hero = container(column![
            ui::logo(56.0, t),
            container(bold("Tinline", 17.0, t.ink).line_height(lh(24.0))).padding(Padding { top: 8.0, ..Padding::ZERO }),
            tx(format!("Version {}", env!("CARGO_PKG_VERSION")), 13.0, t.ink2).line_height(lh(18.0)),
            tx("Made by Osvauld", 13.0, t.ink2).line_height(lh(18.0)),
        ].spacing(4).align_x(Alignment::Center)).width(Fill).center_x(Fill).padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 });
        let mut libs = p2pcore::open_source_licences();
        libs.extend([("Iced", "MIT"), ("WebRTC audio processing", "BSD-3-Clause")].map(|(n, l)| p2pcore::OpenSourceLicence { name: n.into(), licence: l.into() }));
        let licences: RowFn = Box::new(move |r| pf_act(t, Some("licence"), "Open-source licences".into(), Some(("The libraries Tinline is built on".into(), t.ink2, false)),
            Some(pgi(if open_ { "chevup" } else { "chev" }, 18.0, t.ink2)), Some(m(Action::ToggleLicences)), false, r));
        let list: RowFn = Box::new(move |_| {
            let mut c = column![].spacing(8);
            for l in libs {
                c = c.push(row![semi(l.name, 13.0, t.ink).line_height(lh(18.0)), Space::new().width(Fill), tx(l.licence, 13.0, t.ink2).line_height(lh(18.0))].spacing(12));
            }
            container(c).padding(Padding { top: 4.0, right: 16.0, bottom: 12.0, left: 50.0 }).width(Fill).into()
        });
        let source: RowFn = Box::new(move |r| pf_act(t, Some("code"), "Source code".into(), Some(("Tinline is open source".into(), t.ink2, false)),
            Some(chevron(t)), Some(m(Action::OpenSource)), false, r));
        let mut rows = vec![licences];
        if open_ { rows.push(list); }
        rows.push(source);
        let c = column![
            pf_sec(24.0, hero.into()),
            pf_sec(24.0, column![pf_card(t, rows), pf_note(t, "Tinline has no ads and no tracking. Your messages and calls go between your devices and the people you talk to.")].into()),
        ];
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
            let rows: Vec<RowFn> = [("mic", "Audio devices", 3u8), ("phone", "Calls & availability", 2), ("lock", "Security", 4), ("server", "Hosting", 6), ("trash", "Accounts stored on this computer", 11)]
                .into_iter()
                .map(|(icon, name, id)| -> RowFn { Box::new(move |r| pf_act(t, Some(icon), name.into(), None, Some(chevron(t)), Some(m(Action::Open(Page::Native(id)))), false, r)) })
                .collect();
            let quit = pf_sec(24.0, container(pg_btn(t, Btn::DangerQuiet, Some("leave"), "Quit Tinline", Some(Msg::Quit))).into());
            return self.pf_scroll(t, column![section(t, "This computer", rows, vec!["These stay on this computer. They aren\u{2019}t part of your account.".into()]), quit].into(), 40.0);
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
                let zoom = row![rect(16.0), slider(1.0..=4.0, crop.zoom, |z| m(Action::Zoom(z))).step(0.01_f32).width(Fill), rect(22.0)].spacing(10).align_y(Alignment::Center).width(260).height(44);
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
