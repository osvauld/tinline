use super::*;

// 2026-10-14 (Wednesday) 12:00:00 UTC.
const NOW: i64 = 1_791_979_200;
const H: i64 = 3600;
const D: i64 = 86_400;

#[test]
fn calendar_is_right() {
    // 1970-01-01 was a Thursday; 2000-02-29 exists; 2026-10-14 is a Wednesday.
    let c = civil(0, 0);
    assert_eq!((c.year, c.month, c.dom, c.weekday), (1970, 0, 1, 3));
    let c = civil(951_782_400, 0);
    assert_eq!((c.year, c.month, c.dom), (2000, 1, 29));
    let c = civil(NOW, 0);
    assert_eq!(
        (c.year, c.month, c.dom, c.weekday, c.hour),
        (2026, 9, 14, 2, 12)
    );
    let c = civil(-1, 0);
    assert_eq!(
        (c.year, c.month, c.dom, c.hour, c.minute),
        (1969, 11, 31, 23, 59)
    );
}

#[test]
fn ago_golden() {
    for (d, want) in [
        (0, "just now"),
        (59, "just now"),
        (60, "1 min ago"),
        (3599, "59 min ago"),
        (3600, "1 h ago"),
        (86_399, "23 h ago"),
        (86_400, "1 d ago"),
        (13 * D, "13 d ago"),
        (14 * D, "2 w ago"),
        (30 * D, "4 w ago"),
        (-50, "just now"),
    ] {
        assert_eq!(ago(NOW, NOW - d), want, "{d}");
    }
}

#[test]
fn day_label_golden() {
    for (then, off, want) in [
        (NOW, 0, "Today"),
        (NOW + H, 0, "Today"),
        (NOW - 13 * H, 0, "Yesterday"),
        (NOW - 2 * D, 0, "Monday"),
        (NOW - 6 * D, 0, "Thursday"),
        (NOW - 7 * D, 0, "7 Oct"),
        (NOW - 300 * D, 0, "18 Dec 2025"),
        // 23:30 UTC the day before is already today at +02:00.
        (NOW - 12 * H - 1800, 7200, "Today"),
        (NOW - 12 * H - 1800, 0, "Yesterday"),
    ] {
        assert_eq!(day_label(then, NOW, off), want, "{then} {off}");
    }
}

#[test]
fn when_golden() {
    let at = |d: i64| NOW - d;
    for (d, short, long) in [
        (30, "just now", "Today, 11:59"),
        (4 * 60, "4 min ago", "Today, 11:56"),
        (2 * H, "10:00", "Today, 10:00"),
        (D, "Yesterday \u{b7} 12:00", "Yesterday, 12:00"),
        (3 * D, "Sun \u{b7} 12:00", "Sun, 12:00"),
        (10 * D, "4 Oct", "4 Oct, 12:00"),
    ] {
        assert_eq!(when_short(at(d), NOW, 0), short, "short {d}");
        assert_eq!(when_long(at(d), NOW, 0), long, "long {d}");
    }
    assert_eq!(list_time(0, NOW, 0), "");
    assert_eq!(list_time(NOW - 2 * H, NOW, 0), "10:00");
    assert_eq!(list_time(NOW - D, NOW, 0), "Yesterday");
    assert_eq!(list_time(NOW - 3 * D, NOW, 0), "Sunday");
    assert_eq!(month_day_time(1_791_640_200, 0), "Oct 10, 13:50");
    assert_eq!(month_day_time(1_791_640_200, 19_800), "Oct 10, 19:20");
    assert_eq!(clock_time(NOW, -5 * 3600), "07:00");
}

#[test]
fn clocks_and_durations() {
    for (s, want) in [
        (-5, "0:00"),
        (0, "0:00"),
        (7, "0:07"),
        (65, "1:05"),
        (3599, "59:59"),
        (3600, "1:00:00"),
        (3725, "1:02:05"),
    ] {
        assert_eq!(clock(s), want, "{s}");
    }
    assert_eq!(clock_ms(0), "0:00");
    assert_eq!(clock_ms(499), "0:00");
    assert_eq!(clock_ms(500), "0:01");
    assert_eq!(clock_ms(65_400), "1:05");
    for (s, want) in [
        (0, "0 s"),
        (42, "42 s"),
        (59, "59 s"),
        (60, "1 min"),
        (89, "1 min"),
        (90, "2 min"),
        (1080, "18 min"),
        (3900, "1 h 5 min"),
    ] {
        assert_eq!(duration_text(s), want, "{s}");
    }
    for (ms, want) in [
        (0, "1 min"),
        (59_000, "1 min"),
        (300_000, "5 min"),
        (3_600_000, "1 h"),
        (47 * 3_600_000, "47 h"),
        (48 * 3_600_000, "2 days"),
    ] {
        assert_eq!(duration_words(ms), want, "{ms}");
    }
}

#[test]
fn bytes_golden() {
    for (n, want) in [
        (0, "0 B"),
        (999, "999 B"),
        (1_000, "1 KB"),
        (812_345, "812 KB"),
        (999_999, "999 KB"),
        (1_000_000, "1.0 MB"),
        (2_400_000, "2.4 MB"),
        (9_999_999, "10.0 MB"),
        (10_000_000, "10 MB"),
        (184_000_000, "184 MB"),
        (1_200_000_000, "1.2 GB"),
    ] {
        assert_eq!(bytes(n), want, "{n}");
    }
}
