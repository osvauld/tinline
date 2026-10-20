use super::*;

#[test]
fn vocabulary() {
    assert_eq!(text("hangup_local", "Ann", false).as_deref(), Some("You hung up"));
    assert_eq!(text("hangup_remote", "Ann", false).as_deref(), Some("Ann hung up"));
    assert_eq!(text("declined", "Ann", false).as_deref(), Some("Ann declined"));
    assert_eq!(text("no_answer", "Ann", false).as_deref(), Some("No answer"));
    assert_eq!(text("unreachable", "Ann", false).as_deref(), Some("Couldn\u{2019}t reach Ann"));
    assert_eq!(text("connection_lost", "Ann", false).as_deref(), Some("Connection lost"));
    assert_eq!(text("superseded", "Ann", false), None);
    assert_eq!(text("answered_elsewhere", "Ann", false).as_deref(), Some("Answered on another device"));
    assert_eq!(history("declined_elsewhere", true, false, 0, "Ann"), "Declined on another device");
    assert_eq!(text("blocked", "Ann Lee", false).as_deref(), Some("Ann blocked you"));
    assert_eq!(history("blocked", false, false, 0, "Ann"), "Blocked");
    assert_eq!(text(p2pcore::CALLS_NOT_ALLOWED, "Ann Lee", false).as_deref(), Some("Ann hasn\u{2019}t allowed calls from you"));
}

#[test]
fn presentation() {
    assert_eq!(present("superseded", "Ann", false, false, false), End::Hidden);
    assert_eq!(present("declined_local", "Ann", true, false, false), End::Hidden);
    assert_eq!(present("cancelled", "Ann", true, false, false), End::Missed("Missed call from Ann".into()));
    assert_eq!(present("cancelled", "Ann", false, false, false), End::Screen("You cancelled the call".into()));
    assert_eq!(present("hangup_remote", "Ann", true, true, false), End::Screen("Ann hung up".into()));
}
