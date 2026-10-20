//! The one place a core end-reason token (docs/protocol.md, "End reasons") becomes words.

/// How an ended call is presented.
#[derive(Debug, PartialEq, Eq)]
pub enum End {
    /// Nothing to show (a superseded duplicate, or a call the user declined themselves).
    Hidden,
    /// An incoming call nobody answered: a banner and a notification, not a screen.
    Missed(String),
    /// The call-ended screen, with this headline.
    Screen(String),
}

/// `name` is what the user calls the other person. `incoming` and `answered` tell a missed call
/// from one the user ended; `blocked_by_me` says whose block ended a "blocked" call. Which calls
/// are hidden or missed, and the headline, is `present::call::end_view`.
pub fn present(reason: &str, name: &str, incoming: bool, answered: bool, blocked_by_me: bool) -> End {
    use present::call::{end_view, EndKind};
    let v = end_view(reason.into(), name.into(), incoming, answered, blocked_by_me);
    match v.kind {
        EndKind::Hidden => End::Hidden,
        EndKind::Missed => End::Missed(v.text),
        EndKind::Screen => End::Screen(v.text),
    }
}

/// User-facing text for a reason token; `None` for tokens that are never shown
/// (`present::call::end_reason_text`; this is the outgoing-call reading for the directional tokens).
pub fn text(reason: &str, name: &str, blocked_by_me: bool) -> Option<String> {
    present::call::end_reason_text(reason.into(), name.into(), false, blocked_by_me)
}

/// One line for a history row. `missed` is the record's flag.
pub fn history(reason: &str, incoming: bool, missed: bool, secs: u32, name: &str) -> String {
    let dur = |s: u32| present::time::clock(s as i64);
    if missed {
        return "Missed".into();
    }
    match reason {
        "unreachable" => "Couldn\u{2019}t reach".into(),
        "declined" => "Declined".into(),
        "declined_local" => "Declined".into(),
        "unavailable" => "Not available".into(),
        "no_answer" => "No answer".into(),
        "cancelled" => "Cancelled".into(),
        "answered_elsewhere" => "Answered on another device".into(),
        "declined_elsewhere" => "Declined on another device".into(),
        "blocked" => "Blocked".into(),
        "calls not allowed" => "Calls not allowed".into(),
        _ if secs > 0 => format!("{} \u{b7} {}", if incoming { "Incoming" } else { "Outgoing" }, dur(secs)),
        r => text(r, name, false).unwrap_or_else(|| "Call".into()),
    }
}

#[cfg(test)]
#[path = "reason_tests.rs"]
mod tests;
