//! Time words. Timestamps are unix seconds; `utc_offset_secs` is the viewer's offset east of UTC
//! at that moment (the caller reads the zone). Nothing here reads a clock.

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// Local civil date and time of a timestamp.
struct Civil {
    /// Days since 1970-01-01 (local).
    day: i64,
    year: i64,
    month: usize,
    dom: i64,
    /// 0 = Monday.
    weekday: usize,
    hour: i64,
    minute: i64,
}

fn civil(ts: i64, off: i32) -> Civil {
    let local = ts + off as i64;
    let day = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dom = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + (month <= 2) as i64;
    Civil {
        day,
        year,
        month: month as usize - 1,
        dom,
        weekday: (day + 3).rem_euclid(7) as usize,
        hour: secs / 3600,
        minute: secs % 3600 / 60,
    }
}

fn hm(c: &Civil) -> String {
    format!("{:02}:{:02}", c.hour, c.minute)
}

fn d_mon(c: &Civil) -> String {
    format!("{} {}", c.dom, MONTHS[c.month])
}

/// Whole calendar days from `then` to `now` in the viewer's zone (negative: `then` is later).
fn days_between(then: i64, now: i64, off: i32) -> i64 {
    civil(now, off).day - civil(then, off).day
}

/// Age without a zone: "just now", "4 min ago", "3 h ago", "5 d ago", "3 w ago".
#[uniffi::export]
pub fn ago(now: i64, then: i64) -> String {
    let d = (now - then).max(0);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86_399 => format!("{} h ago", d / 3600),
        _ if d < 86_400 * 14 => format!("{} d ago", d / 86_400),
        _ => format!("{} w ago", d / (86_400 * 7)),
    }
}

/// Day pill: "Today", "Yesterday", a weekday within the last week, else "12 Sep" (with the year
/// when it is not this year). A time in the future reads "Today".
#[uniffi::export]
pub fn day_label(then: i64, now: i64, utc_offset_secs: i32) -> String {
    let days = days_between(then, now, utc_offset_secs);
    let c = civil(then, utc_offset_secs);
    match days {
        ..=0 => "Today".into(),
        1 => "Yesterday".into(),
        2..=6 => DAYS[c.weekday].into(),
        _ if c.year == civil(now, utc_offset_secs).year => d_mon(&c),
        _ => format!("{} {}", d_mon(&c), c.year),
    }
}

/// "14:02" in the viewer's zone.
#[uniffi::export]
pub fn clock_time(ts: i64, utc_offset_secs: i32) -> String {
    hm(&civil(ts, utc_offset_secs))
}

/// The time today, else the day, for a list's time column. Empty for a zero timestamp.
#[uniffi::export]
pub fn list_time(then: i64, now: i64, utc_offset_secs: i32) -> String {
    if then == 0 {
        return String::new();
    }
    if days_between(then, now, utc_offset_secs) <= 0 {
        clock_time(then, utc_offset_secs)
    } else {
        day_label(then, now, utc_offset_secs)
    }
}

/// "4 min ago", "12:31", "Yesterday \u{b7} 08:15", "Mon \u{b7} 09:12", "12 Sep".
#[uniffi::export]
pub fn when_short(then: i64, now: i64, utc_offset_secs: i32) -> String {
    let (c, days) = (
        civil(then, utc_offset_secs),
        days_between(then, now, utc_offset_secs),
    );
    match now - then {
        ..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", (now - then) / 60),
        _ if days <= 0 => hm(&c),
        _ if days == 1 => format!("Yesterday \u{b7} {}", hm(&c)),
        _ if days < 7 => format!("{} \u{b7} {}", &DAYS[c.weekday][..3], hm(&c)),
        _ => d_mon(&c),
    }
}

/// "Today, 12:31", "Yesterday, 19:04", "Mon, 09:12", "12 Sep, 09:12".
#[uniffi::export]
pub fn when_long(then: i64, now: i64, utc_offset_secs: i32) -> String {
    let (c, days) = (
        civil(then, utc_offset_secs),
        days_between(then, now, utc_offset_secs),
    );
    let day = match days {
        ..=0 => "Today".to_string(),
        1 => "Yesterday".into(),
        2..=6 => DAYS[c.weekday][..3].into(),
        _ => d_mon(&c),
    };
    format!("{day}, {}", hm(&c))
}

/// "Oct 10, 14:02".
#[uniffi::export]
pub fn month_day_time(ts: i64, utc_offset_secs: i32) -> String {
    let c = civil(ts, utc_offset_secs);
    format!("{} {}, {}", MONTHS[c.month], c.dom, hm(&c))
}

/// A running or finished call length: "0:07", "12:05", "1:02:05".
#[uniffi::export]
pub fn clock(secs: i64) -> String {
    let s = secs.max(0);
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// [`clock`] for milliseconds, rounded to the nearest second (voice messages).
#[uniffi::export]
pub fn clock_ms(ms: u32) -> String {
    clock(((ms as i64) + 500) / 1000)
}

/// A short duration: "42 s", "18 min", "1 h 5 min".
#[uniffi::export]
pub fn duration_text(secs: i64) -> String {
    let s = secs.max(0);
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", (s + 30) / 60),
        _ => format!("{} h {} min", s / 3600, s % 3600 / 60),
    }
}

/// A span in words: "5 min", "2 h", "3 days".
#[uniffi::export]
pub fn duration_words(ms: i64) -> String {
    match ms.max(0) {
        m if m < 3_600_000 => format!("{} min", (m / 60_000).max(1)),
        m if m < 48 * 3_600_000 => format!("{} h", m / 3_600_000),
        m => format!("{} days", m / (24 * 3_600_000)),
    }
}

/// File size: "812 B", "812 KB", "2.4 MB", "34 MB", "1.2 GB" (decimal units).
#[uniffi::export]
pub fn bytes(n: u64) -> String {
    match n {
        0..=999 => format!("{n} B"),
        1_000..=999_999 => format!("{} KB", n / 1_000),
        1_000_000..=9_999_999 => format!("{:.1} MB", n as f64 / 1e6),
        10_000_000..=999_999_999 => format!("{} MB", n / 1_000_000),
        _ => format!("{:.1} GB", n as f64 / 1e9),
    }
}

#[cfg(test)]
mod tests;
