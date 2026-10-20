use super::*;

/// The chip's echo goes after 2.6 s, a suggestion stays 6 s, in the same label box.
#[test]
fn short_line_keeps_its_own_time() {
    let mut v = VideoUi::default();
    assert_eq!(v.hd_line(), None);
    v.say("Sharper video on", HD_NOTE);
    assert_eq!(v.hd_line(), Some("Sharper video on"));
    let Some(three_ago) = Instant::now().checked_sub(Duration::from_secs(3)) else { return };
    v.hd_note = Some(("Sharper video on".into(), three_ago, HD_NOTE));
    assert_eq!(v.hd_line(), None);
    v.hd_note = Some(("Room for sharper video \u{b7} tap HD".into(), three_ago, HD_SUGGEST));
    assert_eq!(v.hd_line(), Some("Room for sharper video \u{b7} tap HD"));
}
