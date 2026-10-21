use super::*;

/// The chip's echo goes after 2.6 s.
#[test]
fn short_line_keeps_its_own_time() {
    let mut v = VideoUi::default();
    assert_eq!(v.hd_line(), None);
    v.say("Sharper video on", HD_NOTE);
    assert_eq!(v.hd_line(), Some("Sharper video on"));
    let Some(three_ago) = Instant::now().checked_sub(Duration::from_secs(3)) else { return };
    v.hd_note = Some(("Sharper video on".into(), three_ago, HD_NOTE));
    assert_eq!(v.hd_line(), None);
}

/// The chip wins for the call; without it Settings, else the core's choice.
#[test]
fn hd_follows_chip_then_setting_then_core() {
    let mut v = VideoUi::default();
    assert!(!v.hd_on(false));
    assert!(v.hd_on(true));
    v.hd_auto = true;
    assert!(v.hd_on(false));
    v.hd_call = Some(false);
    assert!(!v.hd_on(true));
    v.hd_call = Some(true);
    v.hd_auto = false;
    assert!(v.hd_on(false));
}
