use super::*;

fn t(reason: &str, name: &str, incoming: bool, by_me: bool) -> Option<String> {
    end_reason_text(reason.into(), name.into(), incoming, by_me)
}

#[test]
fn vocabulary_golden() {
    let ann = "Ann";
    for (reason, incoming, by_me, want) in [
        ("hangup_local", false, false, Some("You hung up")),
        ("hangup_remote", false, false, Some("Ann hung up")),
        ("declined", false, false, Some("Ann declined")),
        ("declined_local", true, false, Some("You declined")),
        ("cancelled", true, false, Some("Ann stopped calling")),
        ("cancelled", false, false, Some("You cancelled the call")),
        ("no_answer", false, false, Some("No answer")),
        ("no_answer", true, false, Some("Missed call")),
        ("busy", false, false, Some("Ann is on another call")),
        ("busy", true, false, Some("Missed call")),
        (
            "unreachable",
            false,
            false,
            Some("Couldn\u{2019}t reach Ann"),
        ),
        ("connection_lost", false, false, Some("Connection lost")),
        (
            "unavailable",
            true,
            false,
            Some("Turned away while you were not available"),
        ),
        (
            "answered_elsewhere",
            true,
            false,
            Some("Answered on another device"),
        ),
        (
            "declined_elsewhere",
            true,
            false,
            Some("Declined on another device"),
        ),
        ("blocked", false, false, Some("Ann blocked you")),
        ("blocked", false, true, Some("You blocked Ann")),
        (
            "calls not allowed",
            false,
            false,
            Some("Ann hasn\u{2019}t allowed calls from you"),
        ),
        ("superseded", false, false, None),
        ("from_a_newer_core", false, false, Some("Call ended")),
    ] {
        assert_eq!(
            t(reason, ann, incoming, by_me).as_deref(),
            want,
            "{reason} incoming={incoming}"
        );
    }
}

#[test]
fn names_are_trimmed_first_named_and_defaulted() {
    assert_eq!(
        t("blocked", "Ann Lee", false, false).as_deref(),
        Some("Ann blocked you")
    );
    assert_eq!(
        t("blocked", "Ann Lee", false, true).as_deref(),
        Some("You blocked Ann")
    );
    assert_eq!(
        t("calls not allowed", "Priya N", false, false).as_deref(),
        Some("Priya hasn\u{2019}t allowed calls from you")
    );
    assert_eq!(
        t("hangup_remote", "  ", false, false).as_deref(),
        Some("They hung up")
    );
    assert_eq!(
        t("hangup_remote", " Ann Lee ", false, false).as_deref(),
        Some("Ann Lee hung up")
    );
}

#[test]
fn view_golden() {
    let v = |r, inc, ans, by_me| end_view(r, "Ann".into(), inc, ans, by_me);
    let hidden = EndView {
        kind: EndKind::Hidden,
        text: String::new(),
    };
    let missed = EndView {
        kind: EndKind::Missed,
        text: "Missed call from Ann".into(),
    };
    let screen = |s: &str| EndView {
        kind: EndKind::Screen,
        text: s.into(),
    };
    assert_eq!(v("superseded".into(), false, false, false), hidden);
    assert_eq!(v("declined_local".into(), true, false, false), hidden);
    assert_eq!(v("cancelled".into(), true, false, false), missed);
    assert_eq!(v("no_answer".into(), true, false, false), missed);
    assert_eq!(v("busy".into(), true, true, false), missed);
    assert_eq!(
        v("cancelled".into(), false, false, false),
        screen("You cancelled the call")
    );
    assert_eq!(
        v("cancelled".into(), true, true, false),
        screen("Ann stopped calling")
    );
    assert_eq!(
        v("hangup_remote".into(), true, true, false),
        screen("Ann hung up")
    );
    assert_eq!(
        v("blocked".into(), false, false, true),
        screen("You blocked Ann")
    );
}

#[test]
fn classes_and_missed() {
    assert_eq!(classify_end("unreachable".into()), EndClass::Unreachable);
    for r in ["declined", "declined_local", "busy"] {
        assert_eq!(classify_end(r.into()), EndClass::Declined);
    }
    assert_eq!(classify_end("superseded".into()), EndClass::Superseded);
    assert_eq!(classify_end("hangup_local".into()), EndClass::Normal);
    for (r, inc, ans, want) in [
        ("cancelled", true, false, true),
        ("cancelled", true, true, false),
        ("no_answer", true, false, true),
        ("busy", true, false, true),
        ("busy", false, false, false),
        ("cancelled", false, false, false),
        ("hangup_remote", true, false, false),
    ] {
        assert_eq!(
            is_missed_reason(r.into(), inc, ans),
            want,
            "{r} {inc} {ans}"
        );
    }
}

#[test]
fn history_golden() {
    let h = |r: &str, inc, missed, secs| history_line(r.into(), inc, missed, secs, "Ann".into());
    assert_eq!(
        h("declined_elsewhere", true, false, 0),
        "Declined on another device"
    );
    assert_eq!(h("blocked", false, false, 0), "Blocked");
    assert_eq!(h("calls not allowed", false, false, 0), "Calls not allowed");
    assert_eq!(h("hangup_remote", true, true, 0), "Missed");
    assert_eq!(h("unreachable", false, false, 0), "Couldn\u{2019}t reach");
    assert_eq!(h("unavailable", true, false, 0), "Not available");
    assert_eq!(h("hangup_remote", true, false, 65), "Incoming \u{b7} 1:05");
    assert_eq!(
        h("hangup_local", false, false, 3725),
        "Outgoing \u{b7} 1:02:05"
    );
    assert_eq!(h("hangup_remote", false, false, 0), "Ann hung up");
    assert_eq!(h("superseded", false, false, 0), "Call");
}

#[test]
fn quality_golden() {
    for (rtt, loss, bars, label) in [
        (0, 0.0, 4, "Excellent"),
        (99, 0.9, 4, "Excellent"),
        (100, 0.0, 3, "Good"),
        (50, 1.0, 3, "Good"),
        (199, 2.9, 3, "Good"),
        (200, 0.0, 2, "OK"),
        (349, 7.9, 2, "OK"),
        (350, 0.0, 1, "Poor"),
        (10, 8.0, 1, "Poor"),
    ] {
        assert_eq!(
            call_quality(rtt, loss),
            Quality {
                bars,
                label: label.into()
            },
            "{rtt} {loss}"
        );
    }
}

#[test]
fn stats_line_golden() {
    let s = AudioStats {
        secs: 12,
        direct: true,
        rtt_ms: 40,
        sent: 600,
        received: 590,
        lost: 10,
        recovered: 4,
        concealed: 6,
        buffered_ms: 60,
        rx_freq_hz: 440.04,
        rx_rms: 1234.6,
    };
    assert_eq!(
        audio_stats_line(s),
        "stats secs=12 direct=true rtt=40 sent=600 recv=590 lost=10 recovered=4 concealed=6 buf=60 freq=440.0 rms=1235"
    );
    assert_eq!(stat_value(Some(29.97)), "30.0");
    assert_eq!(stat_value(None), "unknown");
}
