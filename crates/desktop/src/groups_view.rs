//! Shared pieces of the group screens (used by `group_live_view.rs`): time formats, rules, tags,
//! footers, and Settings > Privacy with the people I block. The placeholder groups preview that
//! used to live here is in `groups_preview_view.rs` (test-hooks and test builds only).

use chrono::{Local, TimeZone};
use iced::widget::{column, container, Space};

use super::*;

fn local_fmt(ms: u64, fmt: &str) -> String {
    Local.timestamp_millis_opt(ms as i64).single().map(|d| d.format(fmt).to_string()).unwrap_or_default()
}

/// "Oct 10, 15:20".
pub(super) fn when(ms: u64) -> String {
    local_fmt(ms, "%b %-d, %H:%M")
}

/// "Oct 12".
pub(super) fn day(ms: u64) -> String {
    local_fmt(ms, "%b %-d")
}

pub(super) fn hhmm(ms: u64) -> String {
    local_fmt(ms, "%H:%M")
}

/// "Sam Rao, Meera K and Priya N".
pub(super) fn and_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [a] => a.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

pub(super) fn rule<'a>(t: Tok) -> El<'a> {
    container(Space::new()).width(Fill).height(1).style(ui::plain(t.line, 0.0)).into()
}

/// The group avatar: the group id's mandala in a rounded square (people are circles).
pub(super) fn gav<'a>(t: Tok, key: &str, size: f32) -> El<'a> {
    ui::group_face(t, key, size)
}

/// A status dot (host rows).
pub(super) fn dot<'a>(c: Color) -> El<'a> {
    container(Space::new()).width(8).height(8).style(ui::plain(c, 4.0)).into()
}

/// Owner / Admin / Host pills and the like.
pub(super) fn tag<'a>(bg: Color, fg: Color, s: &str) -> El<'a> {
    container(semi(s.to_string(), 11.0, fg)).padding([2, 7]).style(ui::plain(bg, 999.0)).into()
}

/// A small grey line in a conversation.
/// Conversation.svelte `.sys`: centred 12/17 muted text, 6 px above and below, at most 86 % wide.
pub(super) fn sys<'a>(t: Tok, s: &str) -> El<'a> {
    container(container(tx(s.to_string(), 12.0, t.ink2).line_height(iced::widget::text::LineHeight::Absolute(17.0.into())).align_x(Alignment::Center)).max_width(480))
        .center_x(Fill)
        .padding([6, 0])
        .into()
}

/// A quiet text button.
pub(super) fn link_btn<'a>(t: Tok, s: &str, color: Color, on: Msg) -> El<'a> {
    button(semi(s.to_string(), 13.0, color)).padding([4, 8]).style(ui::button_style(t, Kind::Ghost, 999.0)).on_press(on).into()
}

/// A text-only pill for the outlined/text buttons of the boards.
pub(super) fn btn<'a>(t: Tok, kind: Kind, s: &str, on: Option<Msg>) -> El<'a> {
    pill(t, kind, None, s, on)
}

/// A footer in place of the composer.
pub(super) fn footer<'a>(t: Tok, body: El<'a>) -> El<'a> {
    column![rule(t), container(body).padding([12, 20]).width(Fill)].into()
}

pub(super) fn ordinal(n: usize) -> String {
    match n {
        1 => "1st".into(),
        2 => "2nd".into(),
        3 => "3rd".into(),
        n => format!("{n}th"),
    }
}

/// The time today, else the day, like the real chat list.
pub(super) fn short_time(ms: u64) -> String {
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

pub(super) fn message_body<'a>(s: &str, fg: Color) -> El<'a> {
    tx(s.to_string(), 14.0, fg).into()
}

impl App {
    /// Whether a placeholder preview group or person is open in the main pane (never in release).
    pub(super) fn preview_open(&self) -> bool {
        #[cfg(any(test, feature = "test-hooks"))]
        return self.groups.open.is_some();
        #[cfg(not(any(test, feature = "test-hooks")))]
        false
    }

    /// Settings › Privacy: the people I block (real, from the core). Test and test-hooks builds add the
    /// placeholder preview below it.
    pub(super) fn privacy_page(&self, t: Tok) -> El<'_> {
        let mut real = column![tx("One list across all your devices: a block made on another device shows here too.", 13.0, t.ink2)].spacing(10);
        for b in &self.blocks.list {
            let name = self.contacts.iter().find(|c| c.did == b.did).map(Self::display).unwrap_or_else(|| b.name.clone());
            let name = if name.is_empty() { "Someone".to_string() } else { name };
            real = real.push(line(
                t,
                &name,
                &format!("Blocked {}", day(b.blocked_at.saturating_mul(1000))),
                btn(t, Kind::Quiet, "Unblock", Some(Msg::Unblock(b.did.clone()))),
            ));
        }
        if self.blocks.list.is_empty() {
            real = real.push(tx("Nobody is blocked. To block someone, open them in Contacts and choose Block.", 14.0, t.ink2));
        }
        real = real.push(
            container(column![semi("Unblocking", 13.0, t.ink), tx(crate::blocks::UNBLOCK_NOTE, 13.0, t.ink2)].spacing(4))
                .padding(12)
                .width(Fill)
                .style(ui::plain(t.surface2, 12.0)),
        );
        let real = section(t, "Blocked people", real.into());
        #[cfg(any(test, feature = "test-hooks"))]
        return self.privacy_page_preview(t, real);
        #[cfg(not(any(test, feature = "test-hooks")))]
        real
    }
}
