//! Sign-up screens (playground `screens/gate/*`, desktop form) and the first landing: Add contact
//! (`AddContact.svelte`) and the one empty state (`EmptyLine.svelte`). State lives in `signup.rs`.
//!
//! Frame (Gate.svelte + StepShell.svelte): a fixed card, min(600, 100%) x min(640, 100%), radius 24,
//! --surface-raised with a --line hairline and --shadow-sheet (dark: + the rim; no underglow),
//! centred on --canvas. Header (12 32 0 padding, 56 tall): back slot 44, brand (mark 24 + "Tinline"
//! 17), progress (6 segments 14x6, the "n of N" count at 42 wide, hidden on step 0 but keeping its
//! width), the 40 theme switch. Body 8 40 16 with 16 gaps; footer 16 40 32, buttons 220 x 48, the
//! primary at the far right, 12 apart. Every size here is there to keep something from moving.

use super::*;
use super::super::signup::{strength, AddTab, Gs, Step, CONFIRM_AT, CONFIRM_ID, DEVICE_ID, NAME_ID, PASS_ID, URL_PRIVACY, URL_TERMS};
use super::super::Reach;
use crate::pg_icons::pgi;
use iced::widget::{column, rich_text, span, stack};

const CARD_W: f32 = 600.0;

/// "Mon 19 Oct" (codeUntil in people.svelte.ts), in local time.
pub(super) fn code_until(exp: u64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_opt(exp as i64, 0).single().map(|d| d.format("%a %-d %b").to_string()).unwrap_or_default()
}
const CARD_H: f32 = 640.0;

fn g(m: Gs) -> Msg {
    Msg::Gate(m)
}

#[derive(Clone, Copy, PartialEq)]
enum B {
    Primary,
    Secondary,
    Text,
}

/// Gate Btn.svelte, desktop: 48 tall, min 220 wide (fixed here: every label fits), radius 14,
/// 15/650. `enter` shows the ↵ keycap: 22x20, radius 6, 13px, the button's own colour at 16 %.
fn gbtn<'a>(t: Tok, label_: &str, kind: B, enter: bool, on: Option<Msg>) -> El<'a> {
    let off = on.is_none();
    let fg = match (kind, off) {
        (B::Primary, false) => t.on_accent,
        (B::Primary, true) => t.ink2,
        (_, false) => t.accent_ink,
        (_, true) => ui::alpha(t.accent_ink, 0.5),
    };
    let mut r = row![semi(label_.to_string(), 15.0, fg).line_height(lh(20.0))].spacing(8).align_y(Alignment::Center);
    if enter {
        let cap = container(tx("\u{21b5}", 13.0, fg).line_height(lh(13.0)))
            .width(22)
            .height(20)
            .center_x(22)
            .center_y(20)
            .style(ui::plain(ui::alpha(fg, if off { 0.10 } else { 0.16 }), 6.0));
        r = r.push(cap);
    }
    // the keycap's -6 end margin: the label and cap sit 3px right of centre, as in CSS
    let inner = container(r).padding(Padding { left: if enter { 6.0 } else { 0.0 }, ..Padding::ZERO }).center_x(Fill).center_y(Fill);
    let style = move |_: &Theme, st: button::Status| {
        let hover = matches!(st, button::Status::Hovered | button::Status::Pressed);
        let (bg, border) = match kind {
            B::Primary if off => (t.surface2, Color::TRANSPARENT),
            B::Primary => (t.accent, Color::TRANSPARENT),
            B::Secondary => (if hover && !off { t.surface2 } else { Color::TRANSPARENT }, if off { ui::alpha(t.line, 0.5) } else { t.line }),
            B::Text => (if hover && !off { t.surface2 } else { Color::TRANSPARENT }, Color::TRANSPARENT),
        };
        button::Style {
            background: Some(iced::Background::Color(bg)),
            text_color: fg,
            border: Border { color: border, width: if border.a > 0.0 { 1.0 } else { 0.0 }, radius: 14.0.into() },
            ..Default::default()
        }
    };
    button(inner).padding(0).width(220).height(48).style(style).on_press_maybe(on).into()
}

/// Field.svelte: label 13/650, a 48 input (14 inline padding, radius 14, --line border; focus: a
/// 2px --accent-ink ring), and a hint/error slot of `reserve` lines (18 each) so nothing below moves.
fn gfield<'a>(t: Tok, label_: &str, input: text_input::TextInput<'a, Msg>, hint: Option<&str>, error: Option<String>, reserve: u8) -> El<'a> {
    gfield_r(t, label_, input, hint, error, reserve, 14.0)
}

/// [`gfield`] with room at the end for an inline button (the passphrase eye: 52).
/// Words | Key: the same secret two ways (recovery step, Settings). Hugs its two buttons.
pub(super) fn words_key_seg<'a>(t: Tok, as_key: bool, on: impl Fn(bool) -> Msg) -> El<'a> {
    let b = |label_: &'static str, on_: bool, msg: Msg| {
        button(container(text(label_).size(14).font(if on_ { ui::SANS_SEMI } else { ui::SANS }).color(t.ink)).center_y(36)).padding([0, 14]).height(36)
            .style(move |_: &Theme, _| button::Style { background: on_.then(|| t.raised.into()), border: Border { radius: 9.0.into(), ..Default::default() }, ..Default::default() })
            .on_press(msg)
    };
    container(row![b("Words", !as_key, on(false)), b("Key", as_key, on(true))].spacing(4)).padding(3)
        .style(move |_: &Theme| container::Style { background: Some(t.surface2.into()), border: Border { radius: 12.0.into(), ..Default::default() }, ..Default::default() })
        .into()
}

/// The recovery key: monospace, broken anywhere (it has no spaces), in the same card look as the words.
pub(super) fn key_box<'a>(t: Tok, key: String) -> El<'a> {
    container(mono(key, 15.0, t.ink).line_height(lh(24.0)).wrapping(iced::widget::text::Wrapping::Glyph)).padding([14, 16]).width(Fill)
        .style(ui::outlined(t.surface, t.line, 10.0)).into()
}

fn gfield_r<'a>(t: Tok, label_: &str, input: text_input::TextInput<'a, Msg>, hint: Option<&str>, error: Option<String>, reserve: u8, right: f32) -> El<'a> {
    let bad = error.is_some();
    let input = input
        .padding(Padding { top: 14.0, right, bottom: 14.0, left: 14.0 })
        .size(15)
        .line_height(lh(20.0))
        .style(move |_, st| {
            let focus = matches!(st, text_input::Status::Focused { .. });
            text_input::Style {
                background: iced::Background::Color(t.surface),
                border: Border {
                    color: if focus { t.accent_ink } else if bad { t.danger } else { t.line },
                    width: if focus { 2.0 } else { 1.0 },
                    radius: 14.0.into(),
                },
                icon: t.ink2,
                placeholder: ui::alpha(t.ink2, 0.8),
                value: t.ink,
                selection: ui::alpha(t.accent, 0.3),
            }
        });
    let mut c = column![semi(label_.to_string(), 13.0, t.ink).line_height(lh(21.0)), input].spacing(6);
    let msg: Option<El> = match (error, hint) {
        (Some(e), _) => Some(semi(e, 13.0, t.danger).line_height(lh(18.0)).into()),
        (None, Some(h)) => Some(tx(h.to_string(), 13.0, t.ink2).line_height(lh(18.0)).into()),
        _ => None,
    };
    if reserve > 0 {
        c = c.push(container(msg.unwrap_or_else(|| Space::new().into())).height(18.0 * reserve as f32));
    } else if let Some(m) = msg {
        c = c.push(m);
    }
    c.into()
}

/// Notice.svelte `info`: --surface-2, radius 14, 12 14 padding, a 20 icon and 13/19 ink text.
fn notice_box<'a>(t: Tok, icon: &'static str, body: String, error: bool) -> El<'a> {
    let (bg, fg) = if error { (t.danger_bg, t.danger) } else { (t.surface2, t.ink) };
    container(row![container(pgi(icon, 20.0, fg)).padding(Padding { top: 1.0, ..Padding::ZERO }), tx(body, 13.0, fg).line_height(lh(19.0)).width(Fill)].spacing(12))
        .padding([12, 14])
        .width(Fill)
        .style(ui::plain(bg, 14.0))
        .into()
}

/// A round icon chip: `size` circle of --accent-soft with the icon in --accent-ink.
fn ic<'a>(t: Tok, icon: &'static str, size: f32, glyph: f32) -> El<'a> {
    container(pgi(icon, glyph, t.accent_ink)).width(size).height(size).center_x(size).center_y(size).style(ui::plain(t.accent_soft, size / 2.0)).into()
}

/// A lead given as sentences: one sentence per line, each wrapped on its own.
fn lead<'a>(t: Tok, lines: &[String], center: bool) -> El<'a> {
    let mut c = column![].spacing(0).width(Fill);
    for l in lines {
        let mut x = tx(l.clone(), 15.0, t.ink2).line_height(lh(22.0)).width(Fill);
        if center {
            x = x.align_x(Alignment::Center);
        }
        c = c.push(x);
    }
    c.into()
}

impl App {
    fn gate_noun(&self) -> &'static str {
        "computer"
    }

    /// The whole sign-up gate: canvas, card, header, the step, its footer.
    pub(super) fn signup_view(&self, t: Tok) -> El<'_> {
        let (body, foot) = self.gate_step(t);
        self.gate_frame(t, self.gate_head(t), body, foot)
    }

    /// The gate card (Gate.svelte desktop): 600 × 640 on the canvas, radius 24, --surface-raised with the rim.
    pub(super) fn gate_frame<'a>(&'a self, t: Tok, head: El<'a>, body: El<'a>, foot: El<'a>) -> El<'a> {
        // padding 1: everything sits inside the card's 1px border, as in CSS
        let card_inner = container(column![head, container(body).height(Fill).width(Fill).clip(true), foot].width(Fill).height(Fill)).padding(1);
        let base = ui::outlined(t.raised, t.line, 24.0);
        let card = ui::raised(container(card_inner).width(Fill).height(Fill), t, ui::Depth::Sheet, base);
        // Dark depth is the rim only (no underglow, decided 2026-10-13).
        let framed = container(card).width(Fill).max_width(CARD_W).height(Fill).max_height(CARD_H);
        container(framed).padding(24).width(Fill).height(Fill).center_x(Fill).center_y(Fill).style(ui::plain(t.canvas, 0.0)).into()
    }

    /// Unlock.svelte in the gate card: the account's avatar (72), "Welcome back, Maya", the passphrase (with the eye),
    /// an error/hint cell that reserves its lines (so a wrong try moves nothing), then Unlock and "I forgot it".
    /// Desktop keeps the other accounts on this computer under the field (switching is the only way to reach them).
    pub(super) fn unlock_card(&self, t: Tok) -> El<'_> {
        let name = self.profile_name.clone();
        // Identities have one nickname: the whole of it, never just the first word.
        let nick = name.trim().to_string();
        let others = self.accounts.iter().any(|a| !a.current);
        let me = self.accounts.iter().find(|a| a.current).map(|a| a.did.clone()).unwrap_or_default();
        let input = text_input("", &self.pass_in).id(PASS_ID).secure(!self.gate.show_pass).on_input(Msg::PassIn).on_submit(Msg::Unlock);
        let eye = button(container(pgi(if self.gate.show_pass { "eyeoff" } else { "eye" }, 20.0, t.ink2)).center_x(44).center_y(44))
            .padding(0).width(44).height(44).style(super::shell_view::wash(t, None, None, 22.0)).on_press(g(Gs::ShowPass));
        let err = self.notice.clone().filter(|_| !self.busy);
        let hint = (err.is_some() || self.busy).then_some("Checking takes a second. It\u{2019}s deliberately slow to stop guessing.");
        // The error takes a reserved 18 px line inside the field block; the hint sits in the rest of the cell.
        let field_ = gfield_r(t, "Passphrase", input, None, err, 1, 52.0);
        let field_ = stack![field_, column![Space::new().height(26), row![Space::new().width(Fill), eye].padding(Padding { right: 2.0, ..Padding::ZERO })]];
        // reserve 4.25 lines (18 px each) for the error and the hint
        let msgs = container(tx(hint.unwrap_or(""), 13.0, t.ink2).line_height(lh(18.0))).height(53).width(Fill);
        // Remember on this computer (Unlock.svelte .rem): 56 tall, label with its line, the switch at the end. The space is kept
        // while the keyring is being asked and when there is none (the switch is simply not offered), so nothing moves.
        let remember: El = match self.remember_avail {
            Some(true) => container(row![
                    column![semi("Remember on this computer", 15.0, t.ink).line_height(lh(20.0)),
                        tx("Skips the passphrase when Tinline starts, using this computer\u{2019}s keyring.", 13.0, t.ink2).line_height(lh(18.0))].spacing(2).width(Fill),
                    super::profile_view::pg_switch(t, self.remember, Msg::Remember(!self.remember)),
                ].spacing(16).align_y(Alignment::Center)).width(Fill).height(56).center_y(56).into(),
            _ => Space::new().width(Fill).height(56).into(),
        };
        let body = column![
            container(self.profile_avatar(t, &name, &me, 72.0)).padding(Padding { top: 12.0, ..Padding::ZERO }),
            self.welcome_title(t, &nick, others),
            column![field_, msgs].spacing(6).width(Fill),
            remember,
        ].spacing(16).align_x(Alignment::Center).width(Fill);
        // "Open as": the identities on this computer, over the field (a menu, so nothing below moves).
        let body: El = if others && self.gate.open_as {
            stack![body, container(self.open_as_menu(t)).padding(Padding { top: 12.0 + 72.0 + 16.0 + 40.0, ..Padding::ZERO }).center_x(Fill)].into()
        } else {
            body.into()
        };
        // .center.anchor: hangs from a fixed point (min(14dvh, 96)), so a new line never re-centres the rest.
        let body = container(body).padding(Padding { top: 8.0 + 40.0, right: 40.0, bottom: 16.0, left: 40.0 }).height(Fill);
        let unlock = gbtn(t, if self.busy { "Unlocking\u{2026}" } else { "Unlock" }, B::Primary, true, (!self.busy && !self.pass_in.is_empty()).then_some(Msg::Unlock));
        let forgot = gbtn(t, "I forgot it", B::Text, false, (!self.busy).then_some(Msg::GoRestore));
        let foot = container(row![Space::new().width(Fill), forgot, unlock].spacing(12).align_y(Alignment::Center))
            .padding(Padding { top: 16.0, right: 40.0, bottom: 32.0, left: 40.0 }).width(Fill);
        let head = container(row![Space::new().width(44).height(44), row![ui::logo(24.0, t), semi("Tinline", 17.0, t.ink).line_height(lh(24.0))].spacing(8).align_y(Alignment::Center), Space::new().width(Fill),
                button(container(pgi(if t.dark { "sun" } else { "moon" }, 20.0, t.ink2)).center_x(40).center_y(40)).padding(0).width(40).height(40)
                    .style(super::shell_view::wash(t, None, Some(t.surface2), 20.0)).on_press(g(Gs::Theme))]
            .spacing(12).align_y(Alignment::Center).height(44))
            .padding(Padding { top: 12.0, right: 22.0, bottom: 0.0, left: 12.0 }).height(56).width(Fill);
        self.gate_frame(t, head.into(), body.into(), foot.into())
    }

    /// "Welcome back, Maya": with other identities here, the nickname is the switcher ("Maya ⌄"), the same type as
    /// the title, a quiet chevron, a 44 px target (lab A, 2026-10-14).
    fn welcome_title(&self, t: Tok, nick: &str, switcher: bool) -> El<'_> {
        if nick.is_empty() {
            return bold("Welcome back", 28.0, t.ink).line_height(lh(34.0)).into();
        }
        if !switcher {
            return bold(format!("Welcome back, {nick}"), 28.0, t.ink).line_height(lh(34.0)).into();
        }
        let who = button(row![bold(nick.to_string(), 28.0, t.ink).line_height(lh(34.0)), pgi("chev", 20.0, t.ink2)].spacing(2).align_y(Alignment::Center))
            .padding([5, 4])
            .style(super::shell_view::wash(t, None, Some(t.surface2), 10.0))
            .on_press_maybe((!self.busy).then_some(g(Gs::OpenAs)));
        row![bold("Welcome back,", 28.0, t.ink).line_height(lh(34.0)), who].spacing(8).align_y(Alignment::Center).into()
    }

    /// The "Open as" menu: every identity here; the current one ticked. Picking another switches (nothing to lock
    /// yet: we are at login), then this screen asks for that one's passphrase, or it opens right away.
    fn open_as_menu(&self, t: Tok) -> El<'_> {
        let mut col = column![tx("Open as", 13.0, t.ink2).line_height(lh(18.0))].spacing(2).padding(8).width(300);
        for a in &self.accounts {
            let sub = if a.has_passphrase { "Asks for your passphrase" } else { "Opens right away" };
            let mut r = row![self.profile_avatar(t, &a.name, &a.did, 32.0), column![semi(a.name.clone(), 14.0, t.ink), tx(sub, 12.0, t.ink2)].spacing(1).width(Fill)]
                .spacing(10)
                .align_y(Alignment::Center);
            if a.current {
                r = r.push(pgi("check", 16.0, t.ink2));
            }
            let msg = if a.current { g(Gs::OpenAs) } else { Msg::SwitchNow(a.did.clone()) };
            col = col.push(button(r).padding([6, 10]).width(Fill).style(super::shell_view::wash(t, a.current.then_some(t.surface), Some(t.surface2), 10.0)).on_press_maybe((!self.busy).then_some(msg)));
        }
        col = col.push(
            button(row![pgi("plus", 18.0, t.ink2), tx("Add another name", 14.0, t.ink)].spacing(10).align_y(Alignment::Center))
                .padding([10, 10])
                .width(Fill)
                .style(super::shell_view::wash(t, None, Some(t.surface2), 10.0))
                .on_press_maybe((!self.busy).then_some(Msg::NewAccount(false))),
        );
        container(col)
            .style(move |_| container::Style { background: Some(t.raised.into()), border: iced::Border { radius: 14.0.into(), ..Default::default() }, shadow: iced::Shadow { color: iced::Color { a: 0.18, ..iced::Color::BLACK }, offset: iced::Vector::new(0.0, 8.0), blur_radius: 24.0 }, ..Default::default() })
            .into()
    }

    fn gate_head(&self, t: Tok) -> El<'_> {
        let back: El = if self.gate.can_back() {
            button(container(pgi("back", 24.0, t.ink)).center_x(44).center_y(44))
                .padding(0)
                .width(44)
                .height(44)
                .style(super::shell_view::wash(t, None, Some(t.surface2), 22.0))
                .on_press_maybe((!self.busy).then_some(g(Gs::Back)))
                .into()
        } else {
            Space::new().width(44).height(44).into()
        };
        let brand = row![ui::logo(24.0, t), semi("Tinline", 17.0, t.ink).line_height(lh(24.0))].spacing(8).align_y(Alignment::Center);
        let mut end = row![].spacing(8).align_y(Alignment::Center);
        if let Some((n, total)) = self.gate.progress() {
            let mut segs = row![].spacing(4);
            for i in 0..total {
                segs = segs.push(container(Space::new()).width(14).height(6).style(ui::plain(if i < n { t.accent } else { t.line }, 3.0)));
            }
            // step 0: the count keeps its width (hidden), so "1 of 5" appears in place
            let count = tx(format!("{n} of {total}"), 13.0, if n == 0 { Color::TRANSPARENT } else { t.ink2 }).line_height(lh(21.0)).width(42);
            end = end.push(row![segs, count].spacing(10).align_y(Alignment::Center));
        }
        let (icon, _label) = if t.dark { ("sun", "Switch to light") } else { ("moon", "Switch to dark") };
        let theme = button(container(pgi(icon, 20.0, t.ink2)).center_x(40).center_y(40))
            .padding(0)
            .width(40)
            .height(40)
            .style(super::shell_view::wash(t, None, Some(t.surface2), 20.0))
            .on_press(g(Gs::Theme));
        // the icon's optical edge lines up with the card padding (margin-inline-end -10)
        end = end.push(theme);
        container(row![back, brand, Space::new().width(Fill), end].spacing(12).align_y(Alignment::Center).height(44))
            .padding(Padding { top: 12.0, right: 22.0, bottom: 0.0, left: 12.0 })
            .height(56)
            .width(Fill)
            .into()
    }

    /// Body (title, lead, content) and footer of the current step.
    fn gate_step(&self, t: Tok) -> (El<'_>, El<'_>) {
        let noun = self.gate_noun();
        let title = |s: &str| bold(s.to_string(), 28.0, t.ink).line_height(lh(34.0));
        let shell = |head: &str, lead_: Vec<String>, content: Vec<El<'static>>| -> iced::widget::Column<'static, Msg> {
            let mut c = column![title(head)].spacing(16).width(Fill);
            if !lead_.is_empty() {
                c = c.push(lead(t, &lead_, false));
            }
            for x in content {
                c = c.push(x);
            }
            c
        };
        let pad = Padding { top: 8.0, right: 40.0, bottom: 16.0, left: 40.0 };
        let foot = |btns: Vec<El<'static>>| -> El<'static> {
            // row-reverse: the first is the primary at the far right
            let mut r = row![Space::new().width(Fill)].spacing(12).align_y(Alignment::Center);
            for b in btns.into_iter().rev() {
                r = r.push(b);
            }
            container(r).padding(Padding { top: 16.0, right: 40.0, bottom: 32.0, left: 40.0 }).width(Fill).into()
        };
        let busy = self.busy;
        match self.gate.step {
            Step::Welcome => {
                let adding = self.return_to.is_some();
                let (head, l) = if adding {
                    ("Add an account", vec!["Create a new account or bring one you already have.".to_string(), format!("Your current account stays on this {noun}.")])
                } else {
                    ("A direct line to your people", vec![format!("Tinline connects your {noun} straight to theirs."), "You just need a nickname. Encrypted end to end.".to_string()])
                };
                // The line is slack here; on Done it pulls taut ("Your line is open").
                let hero = container(ui::line_logo(120.0, t, crate::mark::Look::default(), None::<Msg>)).width(Fill).height(Fill).center_x(Fill).center_y(Fill);
                let warn = column![bold("Not for emergency calls.", 13.0, t.ink2).line_height(lh(18.0)), tx("Use your phone to reach emergency services.", 13.0, t.ink2).line_height(lh(18.0))];
                let body = column![title(head), lead(t, &l, false), hero, warn].spacing(16).width(Fill).height(Fill);
                let mut b = vec![gbtn(t, "Create my account", B::Primary, true, Some(g(Gs::Start))), gbtn(t, "I have an account", B::Secondary, false, Some(g(Gs::Choose)))];
                if adding {
                    b.push(button(semi("Cancel", 15.0, t.accent_ink)).padding([14, 16]).height(48).style(super::shell_view::wash(t, None, Some(t.surface2), 14.0)).on_press(Msg::CancelNew).into());
                }
                (container(body).padding(pad).height(Fill).into(), foot(b))
            }
            Step::Choose => {
                let opt = |icon: &'static str, head: &'static str, sub: String, msg: Msg| -> El<'static> {
                    let style = move |_: &Theme, st: button::Status| button::Style {
                        background: Some(iced::Background::Color(if matches!(st, button::Status::Hovered) { t.surface2 } else { t.surface })),
                        text_color: t.ink,
                        border: Border { color: t.line, width: 1.0, radius: 14.0.into() },
                        ..Default::default()
                    };
                    button(
                        row![
                            ic(t, icon, 44.0, 24.0),
                            column![semi(head, 15.0, t.ink).line_height(lh(21.0)), tx(sub, 13.0, t.ink2).line_height(lh(18.0))].spacing(2).width(Fill),
                            pgi("chevr", 20.0, t.ink2)
                        ]
                        .spacing(14)
                        .align_y(Alignment::Center),
                    )
                    .padding(16)
                    .width(Fill)
                    .style(style)
                    .on_press(msg)
                    .into()
                };
                let opts = column![
                    opt("qr", "Link my other device", "Scan a code on a device that already has your account. Your contacts and chats come along.".into(), Msg::LinkStartNew),
                    opt("key", "Restore my account", format!("Type your 24 recovery words. This {noun} starts with an empty contact list."), Msg::ToggleRestore),
                ]
                .spacing(12);
                let body = shell("Use your account here", vec![format!("Your account is its recovery phrase, so you can bring it to this {noun} two ways.")], vec![opts.into()]);
                (container(body).padding(pad).into(), Space::new().into())
            }
            Step::Restore => {
                let gs = &self.gate;
                // Notice.svelte `warn`: what comes next.
                let note = container(row![container(pgi("info", 20.0, t.warn_ink)).padding(Padding { top: 1.0, ..Padding::ZERO }),
                        tx("Next, you\u{2019}ll choose a passphrase for this account.", 13.0, t.warn_ink).line_height(lh(19.0)).width(Fill)].spacing(12))
                    .padding([12, 14]).width(Fill).style(ui::plain(t.warn, 14.0));
                let paste = button(semi("Paste from clipboard", 15.0, t.accent_ink).line_height(lh(20.0))).padding([6, 12]).style(super::shell_view::wash(t, None, Some(t.surface2), 14.0)).on_press(g(Gs::WordsPaste));
                let tools = row![paste, Space::new().width(Fill), tx(format!("{} of 24", gs.filled()), 13.0, t.ink2)].align_y(Alignment::Center);
                // .grid: four columns, 8 apart; a cell is the number (11, muted, 20 wide) and the word (15/20 mono).
                let mut grid = column![].spacing(8);
                for r in 0..6 {
                    let mut line = row![].spacing(8);
                    for c in 0..4 {
                        let i = r * 4 + c;
                        let bad = gs.bad_word(i);
                        let ok = !gs.words[i].is_empty() && crate::app::signup::is_word(&gs.words[i]);
                        let input = text_input("", &gs.words[i]).id(crate::app::signup::word_id(i)).on_input(move |v| g(Gs::Word(i, v))).on_submit(g(Gs::WordNext(i)))
                            .padding(Padding { top: 10.0, right: 6.0, bottom: 10.0, left: 0.0 }).size(15).line_height(lh(20.0)).font(ui::MONO)
                            .style(move |_, _| text_input::Style { background: Color::TRANSPARENT.into(), border: Border::default(), icon: t.ink2, placeholder: t.ink2, value: t.ink, selection: ui::alpha(t.accent, 0.3) });
                        let cell = container(row![container(tx((i + 1).to_string(), 11.0, t.ink2)).width(20).align_right(20), input].spacing(6).align_y(Alignment::Center))
                            .padding(Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 10.0 }).height(44).width(Fill)
                            .style(move |_: &Theme| container::Style {
                                background: Some(if bad { t.danger_bg } else { t.surface }.into()),
                                border: Border { color: if bad { t.danger } else if ok { t.accent_soft } else { t.line }, width: 1.0, radius: 10.0.into() },
                                ..Default::default()
                            });
                        line = line.push(cell);
                    }
                    grid = grid.push(line);
                }
                let bad: Vec<usize> = (0..24).filter(|&i| gs.bad_word(i)).collect();
                let msg = if let Some(&f) = bad.first() {
                    format!("Word {}{} {} in the recovery word list. Check the spelling.", f + 1, if bad.len() > 1 { format!(" and {} more", bad.len() - 1) } else { String::new() }, if bad.len() > 1 { "aren\u{2019}t" } else { "isn\u{2019}t" })
                } else if gs.tried && gs.filled() < 24 { format!("{} words are still missing.", 24 - gs.filled()) } else { self.notice.clone().unwrap_or_default() };
                let sugg: Vec<&'static str> = gs.active.filter(|&i| !crate::app::signup::is_word(&gs.words[i])).map(|i| crate::app::signup::suggest(&gs.words[i], 4)).unwrap_or_default();
                let mut chips = row![].spacing(8).height(44).align_y(Alignment::Center);
                for w in sugg {
                    chips = chips.push(button(container(text(w).size(14).font(ui::MONO).color(t.accent_ink)).center_y(44)).padding([0, 16]).height(44)
                        .style(move |_: &Theme, _| button::Style { background: Some(t.accent_soft.into()), border: Border { radius: 22.0.into(), ..Default::default() }, ..Default::default() })
                        .on_press(g(Gs::WordPick(w))));
                }
                let dock = column![chips, container(semi(msg, 13.0, t.danger).line_height(lh(18.0))).height(18)].spacing(6);
                if !gs.rec_key.is_empty() {
                    // A pasted recovery key replaces the boxes; "Use my words instead" brings them back.
                    let card = container(column![tx("Recovery key", 12.0, t.ink2), mono(gs.rec_key.clone(), 15.0, t.ink).line_height(lh(24.0)).wrapping(iced::widget::text::Wrapping::Glyph),
                            container(gbtn(t, "Use my words instead", B::Text, false, Some(g(Gs::ClearKey))))].spacing(6))
                        .padding([14, 16]).width(Fill).style(ui::outlined(t.surface, t.accent_soft, 10.0));
                    let err = container(semi(self.notice.clone().unwrap_or_default(), 13.0, t.danger).line_height(lh(18.0))).height(18);
                    let head = column![title("Restore account"), lead(t, &[format!("Your recovery key is in. It stays on this {noun}.")], false), note, card, err].spacing(16).width(Fill);
                    return (container(head).padding(pad).height(Fill).into(), foot(vec![gbtn(t, "Continue", B::Primary, true, Some(g(Gs::RestoreGo)))]));
                }
                let head = column![title("Restore account"), lead(t, &[format!("Type your 24 words in order, or paste them, or your recovery key, into the first box. They stay on this {noun}.")], false), note, tools].spacing(16).width(Fill);
                let body = column![head, iced::widget::scrollable(grid).height(Fill).style(ui::scroll_style(t)), dock].spacing(12).width(Fill).height(Fill);
                (container(body).padding(pad).height(Fill).into(), foot(vec![gbtn(t, "Continue", B::Primary, true, (gs.filled() > 0).then_some(g(Gs::RestoreGo)))]))
            }
            Step::Name => {
                let ok = !self.name_in.trim().is_empty();
                let input = text_input("", &self.name_in).id(NAME_ID).on_input(Msg::NameChanged).on_submit(g(Gs::NameGo));
                let body = shell("What should people call you?", vec!["Only the people you add will see it.".into()], vec![gfield(t, "Your nickname", input, Some("You can change it any time in Settings."), None, 0)]);
                (container(body).padding(pad).into(), foot(vec![gbtn(t, "Continue", B::Primary, true, ok.then_some(g(Gs::NameGo)))]))
            }
            Step::Pass => {
                let s = strength(&self.pass_in);
                let input = text_input("", &self.pass_in).id(PASS_ID).secure(!self.gate.show_pass).on_input(Msg::PassIn).on_submit(g(Gs::PassGo));
                let eye = button(container(pgi(if self.gate.show_pass { "eyeoff" } else { "eye" }, 20.0, t.ink2)).center_x(44).center_y(44))
                    .padding(0)
                    .width(44)
                    .height(44)
                    .style(super::shell_view::wash(t, None, None, 22.0))
                    .on_press(g(Gs::ShowPass));
                let field_ = gfield_r(t, "Passphrase", input, None, None, 0, 52.0);
                let field_ = stack![field_, column![Space::new().height(26), row![Space::new().width(Fill), eye].padding(Padding { right: 2.0, ..Padding::ZERO })]];
                let bar = |i: usize| {
                    let on = s >= i;
                    let c = match s {
                        1 => t.danger,
                        2 => crate::mark::colors(t.dark).1,
                        _ => t.accent,
                    };
                    container(Space::new()).width(28).height(6).style(ui::plain(if on { c } else { t.line }, 3.0))
                };
                let says = ["Any length works. A few random words are easy to remember.", "Short. A few more words make it stronger.", "Fair", "Strong"];
                // Every line shares one cell (the tallest sets the height): a rating never moves the note below.
                let mut cell = stack![].width(Fill);
                for (i, l) in says.iter().enumerate() {
                    cell = cell.push(tx(*l, 13.0, if i == s { t.ink2 } else { Color::TRANSPARENT }).line_height(lh(18.0)).width(Fill));
                }
                let meter = row![container(row![bar(1), bar(2), bar(3)].spacing(4)).padding(Padding { top: 6.0, ..Padding::ZERO }), cell].spacing(10);
                let col = column![field_, meter].spacing(6);
                let note = if self.keyring_ok {
                    "Encrypts your account on this computer. You type it each time Tinline starts. If you forget it, your recovery words let you set a new one.".to_string()
                } else {
                    format!("{} You type it each time Tinline starts. If you forget it, your recovery words let you set a new one.", super::super::NO_KEYRING)
                };
                let mut content = vec![col.into(), notice_box(t, "lock", note, false)];
                if let Some(e) = &self.notice {
                    content.push(notice_box(t, "warning", e.clone(), true));
                }
                let lead_ = if self.keyring_ok { "Optional. A lock, so only you can open Tinline on this computer." } else { "A lock, so only you can open Tinline on this computer." };
                let body = shell("Add a passphrase", vec![lead_.into()], content);
                // Pass.svelte `cta`: the restore flow ends here, so its button says what happens.
                let cta = if self.gate.restoring { "Restore account" } else { "Continue" };
                let go = gbtn(t, if busy { "Encrypting\u{2026}" } else { cta }, B::Primary, true, (!busy && !self.pass_in.is_empty()).then_some(g(Gs::PassGo)));
                let mut b = vec![go];
                if self.keyring_ok {
                    b.push(gbtn(t, "Skip for now", B::Text, false, (!busy).then_some(g(Gs::PassSkip))));
                }
                (container(body).padding(pad).into(), foot(b))
            }
            Step::Phrase => {
                let phrase = self.new_phrase.as_ref().map(|p| p.to_string()).unwrap_or_default();
                let words: Vec<String> = phrase.split_whitespace().map(str::to_string).collect();
                let mut grid = column![].spacing(4);
                for (r, chunk) in words.chunks(3).enumerate() {
                    let mut line = row![].spacing(10);
                    for (c, w) in chunk.iter().enumerate() {
                        let n = r * 3 + c + 1;
                        line = line.push(
                            container(
                                row![tx(n.to_string(), 11.0, t.ink2).width(20).align_x(Alignment::End), mono(w.clone(), 15.0, t.ink).line_height(lh(20.0))]
                                    .spacing(10)
                                    .align_y(Alignment::Center),
                            )
                            .padding([0, 12])
                            .height(30)
                            .center_y(30)
                            .width(Fill)
                            .style(ui::outlined(t.surface, t.line, 8.0)),
                        );
                    }
                    grid = grid.push(line);
                }
                // Words or key: the same secret two ways (core recovery_key.rs). Words suit paper, the key a password manager.
                let as_key = self.gate.as_key;
                let seg = words_key_seg(t, as_key, |k| g(Gs::AsKey(k)));
                let shown: El = if as_key {
                    key_box(t, p2pcore::recovery_key_from_phrase(phrase.clone()).unwrap_or_default())
                } else {
                    container(grid).padding([2, 4]).into()
                };
                let body = shell(
                    if as_key { "Here is your recovery key" } else { "Here are your recovery words" },
                    if as_key {
                        vec!["Your recovery key is the same as your 24 words, as one line.".into(), "Paste it somewhere safe. A password manager works great.".into()]
                    } else {
                        vec!["If you ever switch devices, these 24 words bring your account along.".into(), "Save a copy somewhere safe. Paper works great.".into()]
                    },
                    vec![seg, shown],
                );
                // One primary slot, two moments: the same button, the same size. The key gets no quiz: its checksum
                // catches a bad copy when it is pasted.
                let b = if self.gate.copied {
                    let mut v = vec![gbtn(t, if as_key { "I\u{2019}ve saved it" } else { "I\u{2019}ve saved them" }, B::Primary, true, Some(g(Gs::Saved)))];
                    if !as_key {
                        v.push(gbtn(t, "Check my copy", B::Text, false, Some(g(Gs::Check))));
                    }
                    v
                } else {
                    vec![gbtn(t, if as_key { "Copy my key" } else { "Copy my words" }, B::Primary, true, Some(g(Gs::CopyWords))), gbtn(t, if as_key { "I saved it elsewhere" } else { "I wrote them down" }, B::Text, false, Some(g(Gs::Saved)))]
                };
                (container(scroll(t, container(body).padding(pad))).height(Fill).into(), foot(b))
            }
            Step::Confirm => {
                let phrase = self.new_phrase.as_ref().map(|p| p.to_string());
                let mut col = column![].spacing(14);
                for (i, p) in CONFIRM_AT.iter().enumerate() {
                    let input = text_input("", &self.gate.confirm[i]).id(CONFIRM_ID[i]).font(ui::MONO).on_input(move |v| g(Gs::ConfirmIn(i, v))).on_submit(g(Gs::ConfirmGo));
                    let err = self.gate.wrong(i, phrase.as_deref()).then(|| format!("That isn\u{2019}t word {p}. Check your saved copy."));
                    col = col.push(gfield(t, &format!("Word {p}"), input, None, err, 1));
                }
                let body = shell("Check my copy", vec!["Optional. Type a few words from your saved copy, to be sure it\u{2019}s right.".into()], vec![col.into()]);
                let ok = self.gate.confirm.iter().all(|a| !a.trim().is_empty());
                (
                    container(scroll(t, container(body).padding(pad))).height(Fill).into(),
                    foot(vec![gbtn(t, "Done", B::Primary, true, ok.then_some(g(Gs::ConfirmGo))), gbtn(t, "Back to the words", B::Text, false, Some(g(Gs::BackToWords)))]),
                )
            }
            Step::Device => {
                let input = text_input("", &self.device_in).id(DEVICE_ID).on_input(Msg::DeviceIn).on_submit(Msg::DeviceGo);
                let err = self.notice.clone();
                let mut chips = row![].spacing(8);
                for s in ["Home computer", "Work computer", "Laptop"] {
                    let on = self.device_in == s;
                    let style = move |_: &Theme, st: button::Status| button::Style {
                        background: Some(iced::Background::Color(if on { t.accent_soft } else if matches!(st, button::Status::Hovered) { t.surface2 } else { Color::TRANSPARENT })),
                        text_color: if on { t.accent_ink } else { t.ink },
                        border: Border { color: if on { Color::TRANSPARENT } else { t.line }, width: 1.0, radius: 16.0.into() },
                        ..Default::default()
                    };
                    chips = chips.push(
                        button(container(semi(s, 13.0, if on { t.accent_ink } else { t.ink }).line_height(lh(18.0))).center_y(42))
                            .padding([0, 16])
                            .height(44)
                            .style(style)
                            .on_press(g(Gs::DeviceSug(s))),
                    );
                }
                let body = shell(
                    "Name this computer",
                    vec!["So you can tell your devices apart in Linked devices. Only your own devices see this name; people you call never do.".into()],
                    vec![gfield(t, "Device name", input, Some("You can rename it later."), err, 0), chips.into()],
                );
                let ok = !self.device_in.trim().is_empty() && !busy;
                (container(body).padding(pad).into(), foot(vec![gbtn(t, "Continue", B::Primary, true, ok.then_some(Msg::DeviceGo))]))
            }
            Step::Terms => {
                let pts: [(&str, &str, &str); 4] = [
                    ("shield", "Free of ads and tracking.", "Tinline doesn\u{2019}t collect your contacts, calls or location."),
                    ("lock", "Calls are end-to-end encrypted.", "When a direct line isn\u{2019}t possible, an encrypted relay passes the call along. It can\u{2019}t hear it."),
                    ("key", "Your account is yours.", "It lives on your devices, not on a server. Your recovery words are the way back in, so keep them safe."),
                    ("warning", "Not for emergency calls.", ""),
                ];
                let mut list = column![].spacing(14);
                for (icon, b, r) in pts {
                    let mut txt = column![bold(b, 15.0, t.ink).line_height(lh(21.0))].width(Fill);
                    if !r.is_empty() {
                        txt = txt.push(tx(r, 15.0, t.ink2).line_height(lh(21.0)));
                    }
                    list = list.push(row![ic(t, icon, 36.0, 20.0), txt].spacing(14));
                }
                // No checkbox: the next button is the agreement, and this line sits right above it.
                let link = |s: &'static str, url: &'static str| span::<&'static str, iced::Font>(s).color(t.accent_ink).underline(true).link(url);
                let agree = rich_text([
                    span::<&'static str, iced::Font>("By continuing, you agree to the ").color(t.ink2),
                    link("Terms of Use", URL_TERMS),
                    span(" and have read the ").color(t.ink2),
                    link("Privacy Policy", URL_PRIVACY),
                    span(".").color(t.ink2),
                ])
                .size(13)
                .line_height(lh(19.0))
                .on_link_click(|u| g(Gs::Open(u)));
                let body = column![title("Before you start"), list, Space::new().height(Fill), agree].spacing(16).width(Fill).height(Fill);
                (container(body).padding(pad).height(Fill).into(), foot(vec![gbtn(t, "I agree", B::Primary, true, Some(g(Gs::Agree)))]))
            }
            Step::Done => {
                let first = self.name_in.split_whitespace().next().unwrap_or("").to_string();
                let head = if first.is_empty() { "Your line is open".to_string() } else { format!("Your line is open, {first}") };
                let look = crate::mark::Look {
                    state: if self.gate.taut { crate::mark::MarkState::Taut } else { crate::mark::MarkState::Slack },
                    spring: crate::mark::Spring::line(),
                    ..Default::default()
                };
                let hero = container(ui::line_logo(96.0, t, look, Some(g(Gs::Tug)))).padding(Padding { top: 28.0, bottom: 4.0, ..Padding::ZERO });
                let tip = |icon: &'static str, b: &'static str, r: &'static str| -> El<'static> {
                    row![ic(t, icon, 36.0, 20.0), column![bold(b, 15.0, t.ink).line_height(lh(19.0)), tx(r, 13.0, t.ink2).line_height(lh(19.0))].width(Fill)].spacing(14).into()
                };
                let tips = column![
                    tip("devices", "Two devices are better than one", "Link another device when you can, so losing one never locks you out."),
                    tip("users", "This computer can host your groups", "Groups are hosted on a computer. Keep Tinline open and your groups stay reachable."),
                ]
                .spacing(12)
                .width(420);
                let body = column![
                    hero,
                    bold(head, 28.0, t.ink).line_height(lh(34.0)),
                    lead(t, &["Scan their code or share yours, and you can call each other.".to_string()], true),
                    container(tips).padding(Padding { top: 8.0, ..Padding::ZERO }),
                ]
                .spacing(16)
                .align_x(Alignment::Center)
                .width(Fill);
                (
                    container(body).padding(pad).width(Fill).height(Fill).center_y(Fill).into(),
                    foot(vec![gbtn(t, "Add my first contact", B::Primary, true, Some(g(Gs::AddFirst))), gbtn(t, "Not now", B::Text, false, Some(g(Gs::NotNow)))]),
                )
            }
        }
    }

    // ---- the first landing ----

    /// Whether there is any conversation to list (1:1 with activity, or an active group).
    pub(super) fn has_chats(&self) -> bool {
        self.chat.chats.iter().any(|r| r.last_activity > 0 && self.selectable(&r.peer_did))
            || self.live.list.iter().any(|g| super::super::group_live::active(g.my_role))
    }

    /// The detail pane with nothing open: the first screen's hero while there are no chats,
    /// otherwise the quiet "Choose a chat".
    pub(super) fn detail_empty(&self, t: Tok) -> El<'_> {
        if self.has_chats() { self.shell_empty_detail(t) } else { self.empty_hero(t) }
    }

    /// EmptyLine.svelte `hero`: the desktop detail pane when there are no chats. Mark 88 with the
    /// faded line (taut once someone is added, the LINE_SPRING pull, no cue of its own), a one-line
    /// title (17/26, 600), two reserved lines (13/19, max 300), a 44 button at min 220.
    pub(super) fn empty_hero(&self, t: Tok) -> El<'_> {
        let people: Vec<&p2pcore::Contact> = self.contacts.iter().filter(|c| !self.blocks.blocked_by_me(&c.did)).collect();
        let first = |c: &p2pcore::Contact| Self::display(c).split_whitespace().next().unwrap_or("").to_string();
        let (head, line, act, msg) = match people.as_slice() {
            // Waiting for my first request to be accepted (EmptyLine.svelte, `pp.sent`).
            [] if !self.requests_out.is_empty() => (
                "Your line is ready",
                format!("Your request to {} is waiting. Your line opens when they accept.", super::super::signup::first_name(&self.requests_out[0].name)),
                "Add another contact".to_string(),
                Msg::OpenAdd,
            ),
            [] => ("Your line is ready", "Add someone you\u{2019}d like to talk to, and you can message and call each other.".to_string(), "Add my first contact".to_string(), Msg::OpenAdd),
            [one] => ("Your line is open", format!("Say hello to {} whenever you like.", first(one)), format!("Message {}", first(one)), Msg::Select(one.did.clone())),
            _ => ("Your line is open", "Pick someone and say hello whenever you like.".to_string(), "Start a chat".to_string(), Msg::ShellSearch(super::super::shell_search::Action::NewChat(true))),
        };
        let look = crate::mark::Look {
            state: if people.is_empty() { crate::mark::MarkState::Slack } else { crate::mark::MarkState::Taut },
            spring: crate::mark::Spring::line(),
            ..Default::default()
        };
        let btn_style = move |_: &Theme, st: button::Status| button::Style {
            background: Some(iced::Background::Color(if matches!(st, button::Status::Hovered) { ui::mix(t.accent, Color::WHITE, 0.08) } else { t.accent })),
            text_color: t.on_accent,
            border: Border { radius: 22.0.into(), ..Default::default() },
            ..Default::default()
        };
        let col = column![
            // 18 margin + the 5px the inline SVG's line box adds under it in the playground (measured)
            container(ui::line_logo(88.0, t, look, None::<Msg>)).padding(Padding { bottom: 23.0, ..Padding::ZERO }),
            container(semi(head, 17.0, t.ink).line_height(lh(26.0))).padding(Padding { bottom: 6.0, ..Padding::ZERO }),
            container(tx(line, 13.0, t.ink2).line_height(lh(19.0)).align_x(Alignment::Center).width(300)).height(38).padding(Padding::ZERO),
            Space::new().height(20),
            button(container(semi(act, 15.0, t.on_accent).line_height(lh(20.0))).center_x(Fill).center_y(44)).padding([0, 18]).width(220).height(44).style(btn_style).on_press(msg),
        ]
        .align_x(Alignment::Center);
        container(col).padding(24).width(Fill).height(Fill).center_x(Fill).center_y(Fill).style(ui::plain(t.bg, 0.0)).into()
    }

    /// EmptyLine.svelte `quiet`: a dimmed 40 mark and one muted line, 40 from the top, no button.
    pub(super) fn empty_quiet<'a>(&self, t: Tok, line: &'a str) -> El<'a> {
        let look = crate::mark::Look { dim: true, ..Default::default() };
        container(column![ui::logo_live(40.0, t, look), tx(line, 13.0, t.ink2).line_height(lh(19.0)).align_x(Alignment::Center).width(Fill)].spacing(10).align_x(Alignment::Center).max_width(300))
            .padding(Padding { top: 40.0, right: 24.0, bottom: 0.0, left: 24.0 })
            .width(Fill)
            .center_x(Fill)
            .into()
    }

    /// AddContact.svelte in its Layer dialog (440 wide, 20 20 16 padding): "Add contact", the By code |
    /// QR chips, a fixed 328 body (so the dialog never resizes), the actions.
    pub(super) fn add_overlay(&self, t: Tok) -> Option<El<'_>> {
        if self.screen != Screen::AddContact {
            return None;
        }
        let first = |name: &str| name.split_whitespace().next().unwrap_or(name).to_string();
        let chip = |label_: &'static str, tab: AddTab| -> El<'static> {
            let on = self.add_ui.tab == tab && self.add_ui.found.is_none();
            let on = on || (self.add_ui.tab == tab);
            let style = move |_: &Theme, st: button::Status| button::Style {
                background: Some(iced::Background::Color(if on { t.accent_soft } else if matches!(st, button::Status::Hovered) { t.surface2 } else { Color::TRANSPARENT })),
                text_color: if on { t.accent_ink } else { t.ink },
                border: Border { color: if on { Color::TRANSPARENT } else { t.line }, width: 1.0, radius: 16.0.into() },
                ..Default::default()
            };
            button(container(semi(label_, 13.0, if on { t.accent_ink } else { t.ink }).line_height(lh(18.0))).center_y(30))
                .padding([0, 14])
                .height(32)
                .style(style)
                .on_press(g(Gs::AcTab(tab)))
                .into()
        };
        let pill_btn = |label_: String, primary: bool, on: Option<Msg>| -> El<'static> {
            let off = on.is_none();
            let style = move |_: &Theme, st: button::Status| {
                let hover = matches!(st, button::Status::Hovered) && !off;
                let (bg, border, fg) = if primary {
                    (if hover { ui::mix(t.accent, Color::WHITE, 0.08) } else { t.accent }, Color::TRANSPARENT, t.on_accent)
                } else {
                    (if hover { t.surface2 } else { t.surface }, t.line, t.ink)
                };
                let a = if off { 0.45 } else { 1.0 };
                button::Style {
                    background: Some(iced::Background::Color(ui::alpha(bg, bg.a * a))),
                    text_color: ui::alpha(fg, a),
                    border: Border { color: ui::alpha(border, border.a * a), width: if primary { 0.0 } else { 1.0 }, radius: 22.0.into() },
                    ..Default::default()
                }
            };
            let fg = if primary { t.on_accent } else { t.ink };
            let fg = if off { ui::alpha(fg, 0.45) } else { fg };
            button(container(semi(label_, 15.0, fg).line_height(lh(20.0))).center_x(Fill).center_y(44)).padding([0, 18]).width(Fill).height(44).style(style).on_press_maybe(on).into()
        };
        // AddContact `.cw`: the found screen's primary keeps a box of at least 96 + 2x18, so
        // "Add Tara" / "Adding…" / "Try again" never change its width.
        let fixed = |label_: String, on: Option<Msg>| -> El<'static> {
            let w = (crate::fit::width(&label_, ui::SANS_SEMI, 15.0).ceil() + 36.0).max(132.0);
            container(pill_btn(label_, true, on)).width(w).into()
        };
        let quiet_btn = |label_: String, on: Option<Msg>| -> El<'static> {
            let fg = if on.is_some() { t.ink } else { ui::alpha(t.ink, 0.45) };
            button(container(semi(label_, 15.0, fg).line_height(lh(20.0))).center_y(44))
                .padding([0, 18])
                .height(44)
                .style(super::shell_view::wash(t, None, Some(t.surface2), 22.0))
                .on_press_maybe(on)
                .into()
        };
        let why = |s: String, center: bool| {
            let x = tx(s, 13.0, t.ink2).line_height(lh(19.0)).width(Fill);
            if center { x.align_x(Alignment::Center) } else { x }
        };
        let close = Msg::Back;
        // LineJoin.svelte: me · the mark · them, fixed 64 high. One mark instance per face: the
        // widget tree keeps the same shape across phases, so iced keeps the mark's spring state and
        // slack → ringing → taut animates. Ringing is the call "connecting" sway (1.2 s, 6°); taut
        // is LINE_SPRING. The `line` cue comes from the add itself, not from this mark.
        let join = |them: &str, key: &str, state: crate::mark::MarkState| -> El<'_> {
            let look = crate::mark::Look { state, sway: Some((1.2, 6.0)), spring: crate::mark::Spring::line(), ..Default::default() };
            row![
                self.profile_avatar(t, &self.profile_name, "me", 56.0),
                ui::line_logo(64.0, t, look, None::<Msg>),
                avatar(t, them, key, 56.0),
            ]
            .spacing(6)
            .height(64)
            .align_y(Alignment::Center)
            .into()
        };
        let buttons = |btns: Vec<El<'static>>| -> El<'static> {
            let mut r = row![Space::new().width(Fill)].spacing(8).align_y(Alignment::Center);
            for b in btns {
                r = r.push(b);
            }
            r.into()
        };
        let (body, acts): (El, El) = if let Some(i) = &self.add_ui.incoming {
            // Someone used my code (their side of the handshake), or accepted mine while the sheet
            // was closed. AddContact.svelte `inc`.
            let f = first(&i.name);
            let state = if i.joined { crate::mark::MarkState::Taut } else { crate::mark::MarkState::Ringing };
            let chip_ = container(
                row![pgi("userplus", 14.0, t.accent_ink), semi(if i.theirs { "Accepted your request" } else { "Has your code" }, 13.0, t.accent_ink).line_height(lh(20.0))]
                    .spacing(6)
                    .align_y(Alignment::Center),
            )
            .padding([3, 10])
            .style(ui::plain(t.accent_soft, 13.0));
            let line = if !i.joined {
                format!("{f} wants to add you as a contact. Accepting lets you message and call each other.")
            } else if i.theirs {
                format!("{f} accepted. Your line to {f} is open. Say hello.")
            } else {
                format!("Your line to {f} is open. Say hello.")
            };
            let face = column![
                join(&i.name, &i.did, state),
                semi(i.name.clone(), 20.0, t.ink).line_height(lh(26.0)),
                chip_,
                container(why(line, true).width(300)).height(57),
            ]
            .spacing(8)
            .align_x(Alignment::Center);
            let btns = if i.joined {
                vec![quiet_btn("Done".into(), Some(close.clone())), fixed(format!("Message {f}"), Some(g(Gs::AcIncomingMessage)))]
            } else {
                let off = self.add_ui.answering;
                vec![quiet_btn("Not now".into(), (!off).then(|| g(Gs::AcNotNow))), fixed("Accept".into(), (!off).then(|| g(Gs::AcAccept)))]
            };
            (container(face).width(Fill).center_x(Fill).into(), buttons(btns))
        } else if let Some(p) = &self.add_ui.found {
            let f = first(&p.name);
            let blocked = p.known && self.blocks.blocked_by_me(&p.did);
            let (line, btns): (String, Vec<El>) = match &self.add_phase {
                AddPhase::Added(_) => (format!("Your line to {f} is open. Say hello."), vec![quiet_btn("Done".into(), Some(close.clone())), fixed(format!("Message {f}"), Some(g(Gs::AcMessage)))]),
                AddPhase::Requested => (format!("Request sent. Your line opens when {f} accepts."), vec![fixed("Done".into(), Some(close.clone()))]),
                _ if blocked => (format!("You blocked {f}. Unblock them in Privacy before adding them again."), vec![quiet_btn("Close".into(), Some(close.clone())), fixed("Open profile".into(), Some(g(Gs::AcProfile)))]),
                _ if p.known => (format!("{f} is already in your contacts."), vec![quiet_btn("Close".into(), Some(close.clone())), fixed("Open profile".into(), Some(g(Gs::AcProfile)))]),
                AddPhase::Connecting(at) => {
                    let slow = at.elapsed() >= Duration::from_millis(2500);
                    let l = if slow { format!("Still trying. {f}\u{2019}s Tinline needs to be open and online for this.") } else { format!("Reaching {f}\u{2019}s Tinline\u{2026}") };
                    (l, vec![quiet_btn("Back".into(), None), fixed("Adding\u{2026}".into(), None)])
                }
                AddPhase::Failed(r) => {
                    let back = quiet_btn("Back".into(), Some(g(Gs::AcBack)));
                    match r {
                        Reach::Refused => (
                            format!("{f}\u{2019}s code isn\u{2019}t valid any more. They may have made a new one, so ask {f} for their newest code."),
                            vec![back, fixed("Paste a new code".into(), Some(g(Gs::AcBack)))],
                        ),
                        Reach::Blocked => (format!("You can\u{2019}t add {f} right now. They\u{2019}ve chosen not to be in touch here."), vec![back, fixed("Close".into(), Some(close.clone()))]),
                        Reach::Offline => ("You\u{2019}re offline. Once you\u{2019}re connected, try again.".into(), vec![back, fixed("Try again".into(), Some(g(Gs::AcAdd)))]),
                        Reach::Timeout | Reach::Other(_) => (
                            format!("Couldn\u{2019}t reach {f}\u{2019}s Tinline. It needs to be open and online, so try again when {f}\u{2019}s around."),
                            vec![back, fixed("Try again".into(), Some(g(Gs::AcAdd)))],
                        ),
                    }
                }
                AddPhase::Idle => (
                    format!("Is this the {f} you expected? If you\u{2019}re not sure, check with them first."),
                    vec![quiet_btn("Back".into(), Some(g(Gs::AcBack))), fixed(format!("Add {f}"), Some(g(Gs::AcAdd)))],
                ),
            };
            // slack (recognised) → ringing while the dial runs and while the request waits → taut.
            let state = match &self.add_phase {
                AddPhase::Added(_) => crate::mark::MarkState::Taut,
                _ if p.known => crate::mark::MarkState::Taut,
                AddPhase::Connecting(_) | AddPhase::Requested => crate::mark::MarkState::Ringing,
                _ => crate::mark::MarkState::Slack,
            };
            let face = column![
                join(&p.name, &p.did, state),
                semi(p.name.clone(), 20.0, t.ink).line_height(lh(26.0)),
                // three lines reserved, so the reason can change without moving the face
                container(why(line, true).width(300)).height(57),
            ]
            .spacing(8)
            .align_x(Alignment::Center);
            (container(face).width(Fill).center_x(Fill).into(), buttons(btns))
        } else if self.add_ui.tab == AddTab::Qr {
            let qr: El = match &self.qr {
                Some(q) => container(canvas(QrView(q)).width(Length::Fixed(260.0)).height(Length::Fixed(260.0))).padding(10).style(ui::plain(Color::WHITE, 12.0)).into(),
                None => container(tx("Preparing your code\u{2026}", 13.0, t.ink2)).width(280).height(280).center_x(280).center_y(280).style(ui::plain(t.surface2, 12.0)).into(),
            };
            let mine = column![container(qr).width(296).height(296).center_x(296).center_y(296), why("Let them scan this with Tinline.".into(), true)].spacing(10).align_x(Alignment::Center);
            (container(mine).width(Fill).center_x(Fill).into(), row![Space::new().width(Fill), quiet_btn("Close".into(), Some(close.clone()))].into())
        } else {
            let copied = self.copied_at.is_some_and(|at| at.elapsed() < Duration::from_millis(1600));
            let code = self.ticket.clone().unwrap_or_else(|| "Preparing your code\u{2026}".into());
            let code = crate::fit::ellipsize(&code, ui::MONO, 12.0, 400.0 - 24.0);
            let code = container(mono(code, 12.0, t.ink2).line_height(lh(18.0)).wrapping(iced::widget::text::Wrapping::None)).padding([8, 12]).width(Fill).clip(true).style(ui::plain(t.surface2, 12.0));
            // "New code" sits on the label's line (a text button, same height), so it never moves anything.
            let renew = button(semi("New code", 13.0, t.accent_ink).line_height(lh(21.0)))
                .padding(0)
                .style(super::shell_view::wash(t, None, None, 4.0))
                .on_press_maybe(self.ticket.as_ref().map(|_| g(Gs::AcNewCode)));
            let mine = column![
                row![semi("Your code", 13.0, t.ink).line_height(lh(21.0)), Space::new().width(Fill), renew].align_y(Alignment::Center),
                // Reusable for 7 days (core `my_code`); the date is the code's own `exp`, local time.
                why(match self.ticket_exp {
                    Some(exp) => format!("Send it to anyone you\u{2019}d like to talk to, in any app. It works until {}.", code_until(exp)),
                    None => "Send it to anyone you\u{2019}d like to talk to, in any app.".into(),
                }, false),
                code,
                pill_btn(if copied { "Copied".into() } else { "Copy my code".into() }, true, self.ticket.as_ref().map(|_| g(Gs::AcCopy))),
            ]
            .spacing(8);
            let rule = || container(Space::new()).width(Fill).height(1).style(ui::plain(t.line, 0.0));
            let or = row![rule(), tx("or", 13.0, t.ink2).line_height(lh(21.0)), rule()].spacing(10).align_y(Alignment::Center);
            // Hints are not errors: muted ink, never red (VOICE.md).
            let theirs = column![
                semi("Their code", 13.0, t.ink).line_height(lh(21.0)),
                pill_btn("Paste their code".into(), false, Some(g(Gs::AcPaste))),
                container(why(self.add_ui.hint.clone(), false)).height(38),
            ]
            .spacing(8);
            (column![mine, or, theirs].spacing(12).into(), row![Space::new().width(Fill), quiet_btn("Close".into(), Some(close.clone()))].into())
        };
        let title = match &self.add_ui.incoming {
            Some(i) if !i.joined => "Contact request",
            Some(_) => "New contact",
            None => "Add contact",
        };
        // `.tabs.gone`: hidden but keeping its height, so the sheet doesn't jump when it turns
        // into a request.
        let tabs: El = if self.add_ui.incoming.is_some() {
            Space::new().width(Fill).height(32).into()
        } else {
            row![chip("Code", AddTab::Code), chip("QR", AddTab::Qr)].spacing(8).into()
        };
        let content = column![
            bold(title, 17.0, t.ink).line_height(lh(24.0)),
            tabs,
            container(body).height(328).width(Fill).center_y(328).clip(true),
            container(acts).padding(Padding { top: 2.0, ..Padding::ZERO }),
        ]
        .spacing(12);
        Some(super::shell_view::pg_dialog(t, 1.0, None, container(content).padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 }).into(), close))
    }
}
