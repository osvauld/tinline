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
/// from one the user ended.
pub fn present(reason: &str, name: &str, incoming: bool, answered: bool) -> End {
    match reason {
        "superseded" | "declined_local" => End::Hidden,
        "cancelled" | "no_answer" if incoming && !answered => End::Missed(format!("Missed call from {name}")),
        "busy" if incoming => End::Missed(format!("Missed call from {name}")),
        r => match text(r, name) {
            Some(t) => End::Screen(t),
            None => End::Hidden,
        },
    }
}

/// User-facing text for a reason token; `None` for tokens that are never shown.
pub fn text(reason: &str, name: &str) -> Option<String> {
    Some(match reason {
        "hangup_local" => "You hung up".into(),
        "hangup_remote" => format!("{name} hung up"),
        "declined" => format!("{name} declined"),
        "declined_local" => "You declined".into(),
        "cancelled" => "Call cancelled".into(),
        "no_answer" => "No answer".into(),
        "unreachable" => format!("Couldn\u{2019}t reach {name}"),
        "connection_lost" => "Connection lost".into(),
        "busy" => format!("{name} is busy"),
        "unavailable" => "Turned away while unavailable".into(),
        "superseded" => return None,
        // Tokens from a newer core: say something rather than nothing.
        _ => "Call ended".into(),
    })
}

/// One line for a history row. `missed` is the record's flag.
pub fn history(reason: &str, incoming: bool, missed: bool, secs: u32, name: &str) -> String {
    let dur = |s: u32| {
        if s >= 3600 {
            format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
        } else {
            format!("{}:{:02}", s / 60, s % 60)
        }
    };
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
        _ if secs > 0 => format!("{} \u{b7} {}", if incoming { "Incoming" } else { "Outgoing" }, dur(secs)),
        r => text(r, name).unwrap_or_else(|| "Call".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocabulary() {
        assert_eq!(text("hangup_local", "Ann").as_deref(), Some("You hung up"));
        assert_eq!(text("hangup_remote", "Ann").as_deref(), Some("Ann hung up"));
        assert_eq!(text("declined", "Ann").as_deref(), Some("Ann declined"));
        assert_eq!(text("no_answer", "Ann").as_deref(), Some("No answer"));
        assert_eq!(text("unreachable", "Ann").as_deref(), Some("Couldn\u{2019}t reach Ann"));
        assert_eq!(text("connection_lost", "Ann").as_deref(), Some("Connection lost"));
        assert_eq!(text("superseded", "Ann"), None);
    }

    #[test]
    fn presentation() {
        assert_eq!(present("superseded", "Ann", false, false), End::Hidden);
        assert_eq!(present("declined_local", "Ann", true, false), End::Hidden);
        assert_eq!(present("cancelled", "Ann", true, false), End::Missed("Missed call from Ann".into()));
        assert_eq!(present("cancelled", "Ann", false, false), End::Screen("Call cancelled".into()));
        assert_eq!(present("hangup_remote", "Ann", true, true), End::Screen("Ann hung up".into()));
    }
}
