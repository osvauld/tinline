//! Dates in the language in use: month and weekday names are catalog keys, and each
//! shape ("Sep 12", "12 Sep", "Thursday 8 Oct") is one pattern key, so the order is
//! the translator's. chrono's `%b`/`%A` are English only and must not be used for text.

use chrono::{DateTime, Datelike, Local, Weekday};

use super::{tr_args, tr_str};

/// Short month name, `m` is 1..=12.
pub fn month_short(m: u32) -> &'static str {
    tr_str(match m {
        1 => "date_month_1",
        2 => "date_month_2",
        3 => "date_month_3",
        4 => "date_month_4",
        5 => "date_month_5",
        6 => "date_month_6",
        7 => "date_month_7",
        8 => "date_month_8",
        9 => "date_month_9",
        10 => "date_month_10",
        11 => "date_month_11",
        _ => "date_month_12",
    })
}

/// Full weekday name.
pub fn weekday(w: Weekday) -> &'static str {
    tr_str(match w {
        Weekday::Mon => "date_weekday_mon",
        Weekday::Tue => "date_weekday_tue",
        Weekday::Wed => "date_weekday_wed",
        Weekday::Thu => "date_weekday_thu",
        Weekday::Fri => "date_weekday_fri",
        Weekday::Sat => "date_weekday_sat",
        Weekday::Sun => "date_weekday_sun",
    })
}

/// "Sep 12".
pub fn month_day(month: u32, day: u32) -> String {
    tr_args("date_month_day", &[("month", month_short(month)), ("day", &day.to_string())])
}

/// "12 Sep".
pub fn day_month(day: u32, month: u32) -> String {
    tr_args("date_day_month", &[("day", &day.to_string()), ("month", month_short(month))])
}

/// "Sep 12, 15:20".
pub fn month_day_time(d: &DateTime<Local>) -> String {
    tr_args("date_month_day_time", &[("month", month_short(d.month())), ("day", &d.day().to_string()), ("time", &d.format("%H:%M").to_string())])
}

/// "Thursday 8 Oct", with the year ("Thursday 8 Oct 2025") when `year` is set.
pub fn weekday_date(d: &DateTime<Local>, year: bool) -> String {
    let args = [("weekday", weekday(d.weekday())), ("day", &d.day().to_string()), ("month", month_short(d.month())), ("year", &d.year().to_string())];
    tr_args(if year { "date_weekday_date_year" } else { "date_weekday_date" }, &args)
}

/// A label that stands alone, like the day chip: first letter upper case
/// (French and Finnish write weekday names in lower case mid-sentence).
pub fn standalone(s: String) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_lowercase() => c.to_uppercase().chain(chars).collect(),
        _ => s,
    }
}

/// "Sep 12" for a unix time in ms, local time.
pub fn local_month_day(ms: u64) -> String {
    use chrono::TimeZone;
    Local.timestamp_millis_opt(ms as i64).single().map(|d| month_day(d.month(), d.day())).unwrap_or_default()
}
