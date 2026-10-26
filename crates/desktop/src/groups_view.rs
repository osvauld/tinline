//! Shared pieces of the group screens (used by `group_live_view.rs`): time formats, rules, tags,
//! footers, and Settings > Privacy with the people I block.

use chrono::{Local, TimeZone};
use iced::widget::{column, container, Space};

use super::*;

fn local_fmt(ms: u64, fmt: &str) -> String {
    Local.timestamp_millis_opt(ms as i64).single().map(|d| d.format(fmt).to_string()).unwrap_or_default()
}

/// "Oct 10, 15:20".
pub(super) fn when(ms: u64) -> String {
    Local.timestamp_millis_opt(ms as i64).single().map(|d| crate::i18n::date::month_day_time(&d)).unwrap_or_default()
}

/// "Oct 12".
pub(super) fn day(ms: u64) -> String {
    crate::i18n::date::local_month_day(ms)
}

pub(super) fn hhmm(ms: u64) -> String {
    local_fmt(ms, "%H:%M")
}


pub(super) fn rule<'a>(t: Tok) -> El<'a> {
    container(Space::new()).width(Fill).height(1).style(ui::plain(t.line, 0.0)).into()
}

/// The group avatar: the group id's mandala in a rounded square (people are circles).
pub(super) fn gav<'a>(t: Tok, key: &str, size: f32) -> El<'a> {
    ui::group_face(t, key, size)
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

/// The time today, else the day, like the real chat list.
pub(super) fn short_time(ms: u64) -> String {
    if ms == 0 {
        return String::new();
    }
    let today = Local::now().date_naive();
    match Local.timestamp_millis_opt(ms as i64).single() {
        Some(d) if d.date_naive() == today => d.format("%H:%M").to_string(),
        Some(d) if (today - d.date_naive()).num_days() == 1 => crate::i18n::tr_str("common_yesterday").into(),
        Some(d) => crate::i18n::date::month_day(chrono::Datelike::month(&d), chrono::Datelike::day(&d)),
        None => String::new(),
    }
}

pub(super) fn message_body<'a>(s: &str, fg: Color) -> El<'a> {
    tx(s.to_string(), 14.0, fg).into()
}

impl App {
    /// Settings › Privacy: the people I block (real, from the core).
    pub(super) fn privacy_page(&self, t: Tok) -> El<'_> {
        let mut real = column![tx(crate::i18n::tr_str("group_one_list_across_all"), 13.0, t.ink2)].spacing(10);
        for b in &self.blocks.list {
            let name = self.contacts.iter().find(|c| c.did == b.did).map(Self::display).unwrap_or_else(|| b.name.clone());
            let name = if name.is_empty() { crate::i18n::tr_str("common_someone").to_string() } else { name };
            real = real.push(line(
                t,
                &name,
                &crate::i18n::tr_args("group_blocked_2", &[("value", &(day(b.blocked_at.saturating_mul(1000))).to_string())]),
                btn(t, Kind::Quiet, crate::i18n::tr_str("common_unblock"), Some(Msg::Unblock(b.did.clone()))),
            ));
        }
        if self.blocks.list.is_empty() {
            real = real.push(tx(crate::i18n::tr_str("group_nobody_is_blocked_to"), 14.0, t.ink2));
        }
        real = real.push(
            container(column![semi(crate::i18n::tr_str("group_unblocking"), 13.0, t.ink), tx(present::block::unblock_note(), 13.0, t.ink2)].spacing(4))
                .padding(12)
                .width(Fill)
                .style(ui::plain(t.surface2, 12.0)),
        );
        let real = section(t, crate::i18n::tr_str("common_blocked_people"), real.into());
        real
    }
}
