//! Calls: end-reason words, history lines, connection quality and the stats line.
//!
//! End reasons are the stable tokens of `CallState::Ended` / `CallRecord.reason`
//! (docs/protocol.md, "End reasons"). Unknown tokens read "Call ended".

use crate::names::first_word;
use crate::time::clock;

/// How an ended call is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum EndKind {
    /// Nothing to show (a superseded duplicate, or a call we declined ourselves).
    Hidden,
    /// An incoming call nobody answered: a banner and a notification, not a screen.
    Missed,
    /// The call-ended screen.
    Screen,
}

/// What to show for an ended call. `text` is empty when `kind` is `Hidden`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EndView {
    pub kind: EndKind,
    pub text: String,
}

/// Coarse class of a reason, for icons and colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum EndClass {
    Normal,
    Unreachable,
    Declined,
    Superseded,
}

fn who(name: &str) -> &str {
    let n = name.trim();
    if n.is_empty() { "They" } else { n }
}

/// Class of an end reason.
#[uniffi::export]
pub fn classify_end(reason: String) -> EndClass {
    match reason.as_str() {
        "unreachable" => EndClass::Unreachable,
        "declined" | "declined_local" | "busy" => EndClass::Declined,
        "superseded" => EndClass::Superseded,
        _ => EndClass::Normal,
    }
}

/// An incoming call that rang out or was given up on without us answering it.
#[uniffi::export]
pub fn is_missed_reason(reason: String, incoming: bool, answered: bool) -> bool {
    incoming
        && match reason.as_str() {
            "cancelled" | "no_answer" => !answered,
            "busy" => true,
            _ => false,
        }
}

/// The words for an end reason, or `None` for tokens never shown. `incoming`: the call came in
/// (the same token reads differently from each side). `blocked_by_me`: for "blocked", whether the
/// block was ours.
#[uniffi::export]
pub fn end_reason_text(
    reason: String,
    name: String,
    incoming: bool,
    blocked_by_me: bool,
) -> Option<String> {
    let who = who(&name);
    let first = first_word(who);
    Some(match reason.as_str() {
        "hangup_local" => "You hung up".into(),
        "hangup_remote" => format!("{who} hung up"),
        "declined" => format!("{who} declined"),
        "declined_local" => "You declined".into(),
        "cancelled" if incoming => format!("{who} stopped calling"),
        "cancelled" => "You cancelled the call".into(),
        "no_answer" if incoming => "Missed call".into(),
        "no_answer" => "No answer".into(),
        "busy" if incoming => "Missed call".into(),
        "busy" => format!("{who} is on another call"),
        "unreachable" => format!("Couldn\u{2019}t reach {who}"),
        "connection_lost" => "Connection lost".into(),
        "unavailable" => "Turned away while you were not available".into(),
        "answered_elsewhere" => "Answered on another device".into(),
        "declined_elsewhere" => "Declined on another device".into(),
        "blocked" if blocked_by_me => format!("You blocked {first}"),
        "blocked" => format!("{first} blocked you"),
        // P6: they let us message them but not call.
        "calls not allowed" => format!("{first} hasn\u{2019}t allowed calls from you"),
        "superseded" => return None,
        _ => "Call ended".into(),
    })
}

/// Everything a screen needs for an ended call. `answered`: we picked up before it ended.
#[uniffi::export]
pub fn end_view(
    reason: String,
    name: String,
    incoming: bool,
    answered: bool,
    blocked_by_me: bool,
) -> EndView {
    let hidden = EndView {
        kind: EndKind::Hidden,
        text: String::new(),
    };
    if matches!(reason.as_str(), "superseded" | "declined_local") {
        return hidden;
    }
    if is_missed_reason(reason.clone(), incoming, answered) {
        return EndView {
            kind: EndKind::Missed,
            text: format!("Missed call from {}", who(&name)),
        };
    }
    match end_reason_text(reason, name, incoming, blocked_by_me) {
        Some(text) => EndView {
            kind: EndKind::Screen,
            text,
        },
        None => hidden,
    }
}

/// One line for a history row. `missed` is the record's flag.
#[uniffi::export]
pub fn history_line(
    reason: String,
    incoming: bool,
    missed: bool,
    secs: u32,
    name: String,
) -> String {
    if missed {
        return "Missed".into();
    }
    match reason.as_str() {
        "unreachable" => "Couldn\u{2019}t reach".into(),
        "declined" | "declined_local" => "Declined".into(),
        "unavailable" => "Not available".into(),
        "no_answer" => "No answer".into(),
        "cancelled" => "Cancelled".into(),
        "answered_elsewhere" => "Answered on another device".into(),
        "declined_elsewhere" => "Declined on another device".into(),
        "blocked" => "Blocked".into(),
        "calls not allowed" => "Calls not allowed".into(),
        _ if secs > 0 => format!(
            "{} \u{b7} {}",
            if incoming { "Incoming" } else { "Outgoing" },
            clock(secs as i64)
        ),
        r => end_reason_text(r.into(), name, incoming, false).unwrap_or_else(|| "Call".into()),
    }
}

/// Connection quality as signal bars.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Quality {
    /// 1 (poor) to 4 (excellent).
    pub bars: u8,
    pub label: String,
}

/// Bars from round-trip time and packet loss: 4 Strong, 3 Good, 2 Fair, 1 Weak (the words both apps already used).
#[uniffi::export]
pub fn call_quality(rtt_ms: u32, loss_pct: f64) -> Quality {
    let (bars, label) = if rtt_ms < 100 && loss_pct < 1.0 {
        (4, "Strong")
    } else if rtt_ms < 200 && loss_pct < 3.0 {
        (3, "Good")
    } else if rtt_ms < 350 && loss_pct < 8.0 {
        (2, "Fair")
    } else {
        (1, "Weak")
    };
    Quality {
        bars,
        label: label.into(),
    }
}

/// The word for a number of bars (1 to 4); anything else reads "Weak".
#[uniffi::export]
pub fn signal_word(bars: u8) -> String {
    match bars {
        4 => "Strong",
        3 => "Good",
        2 => "Fair",
        _ => "Weak",
    }
    .into()
}

/// The numbers of an audio call's stats (the core's `CallStats`), passed in plain.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AudioStats {
    pub secs: u32,
    pub direct: bool,
    pub rtt_ms: u32,
    pub sent: u64,
    pub received: u64,
    pub lost: u64,
    pub recovered: u64,
    pub concealed: u64,
    pub buffered_ms: u32,
    pub rx_freq_hz: f64,
    pub rx_rms: f64,
}

/// The one-line log form of audio stats.
#[uniffi::export]
pub fn audio_stats_line(s: AudioStats) -> String {
    format!(
        "stats secs={} direct={} rtt={} sent={} recv={} lost={} recovered={} concealed={} buf={} freq={:.1} rms={:.0}",
        s.secs,
        s.direct,
        s.rtt_ms,
        s.sent,
        s.received,
        s.lost,
        s.recovered,
        s.concealed,
        s.buffered_ms,
        s.rx_freq_hz,
        s.rx_rms
    )
}

/// A measured rate for a stats line: one decimal, or "unknown".
#[uniffi::export]
pub fn stat_value(v: Option<f64>) -> String {
    v.map_or_else(|| "unknown".into(), |v| format!("{v:.1}"))
}

#[cfg(test)]
mod tests;
