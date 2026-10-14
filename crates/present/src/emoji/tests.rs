use super::*;

#[test]
fn only_count_golden() {
    for (input, want) in [
        ("\u{1F44D}", Some(1)),
        (" \u{1F602}\u{1F602} ", Some(2)),
        ("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", Some(1)), // ZWJ family
        ("\u{1F44D}\u{1F3FD}", Some(1)),                          // skin tone
        ("\u{1F1EE}\u{1F1F3}\u{1F1E9}\u{1F1EA}", Some(2)),        // two flags
        ("\u{2764}\u{FE0F}", Some(1)),
        ("ok \u{1F44D}", None),
        ("", None),
        ("   ", None),
    ] {
        assert_eq!(emoji_only_count(input.into()), want, "{input:?}");
    }
}

#[test]
fn big_boundary() {
    assert!(emoji_is_big("\u{1F389}\u{1F389}\u{1F389}".into()));
    assert!(!emoji_is_big("\u{1F389}\u{1F389}\u{1F389}\u{1F389}".into()));
    assert!(!emoji_is_big("hi".into()));
}

#[test]
fn runs_golden() {
    let r = |t: &str, e| EmojiRun {
        text: t.into(),
        is_emoji: e,
    };
    assert_eq!(
        emoji_runs("hi \u{1F44B}\u{1F3FD} there".into()),
        vec![
            r("hi ", false),
            r("\u{1F44B}\u{1F3FD}", true),
            r(" there", false)
        ]
    );
    assert_eq!(emoji_runs("plain".into()), vec![r("plain", false)]);
    assert_eq!(emoji_runs("".into()), vec![]);
    assert_eq!(emoji_runs("\u{1F600}".into()), vec![r("\u{1F600}", true)]);
}
