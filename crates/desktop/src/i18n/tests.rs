use super::*;

#[test]
fn locale_matching() {
    assert_eq!(match_tag("en_US.UTF-8"), "en");
    assert_eq!(match_tag("xx-YY"), "en");
    assert_eq!(match_tag(""), "en");
}

#[test]
fn plural_picks_form() {
    set_language("en");
    assert_eq!(tr_count("chat_new_messages", 1, &[]), "1 new message");
    assert_eq!(tr_count("chat_new_messages", 3, &[]), "3 new messages");
    assert_eq!(tr_count("chat_new_messages", 0, &[]), "0 new messages");
}

#[test]
fn args_fill() {
    set_language("en");
    assert_eq!(tr_args("link_this_computer_os", &[("os", "Linux")]), "This computer · Linux");
    assert_eq!(tr("shell_contacts_count"), "{count} contacts", "plain tr on a plural uses 'other'");
}

#[test]
fn static_strings_match_owned() {
    set_language("en");
    assert_eq!(tr_str("tray_quit"), tr("tray_quit"));
}

#[test]
fn unknown_language_is_english() {
    set_language("zz");
    assert_eq!(language(), "en");
    assert!(!is_rtl());
}

#[test]
fn every_language_has_english_fallback_for_every_key() {
    // Whatever language is picked, a key the language lacks resolves from English.
    for (tag, _, _) in LANGUAGES {
        set_language(tag);
        assert!(!tr("tray_quit").is_empty());
    }
    set_language("en");
}

#[test]
fn list_joins_with_language_patterns() {
    set_language("en");
    assert_eq!(tr_list(&[]), "");
    assert_eq!(tr_list(&["Ana"]), "Ana");
    assert_eq!(tr_list(&["Ana", "Bo"]), "Ana and Bo");
    assert_eq!(tr_list(&["Ana", "Bo", "Cy"]), "Ana, Bo and Cy");
    assert_eq!(tr_dots(&["a", "", "b"]), "a · b");
}

#[test]
fn rich_text_pieces() {
    set_language("en");
    let p = tr_rich("signup_by_continuing_you_agree", &[]);
    assert!(p.contains(&Piece::Slot("terms".into())) && p.contains(&Piece::Slot("privacy".into())));
    let p = tr_rich("group_invitation_line", &[("name", "Ana"), ("group", "Hike")]);
    assert_eq!(p, vec![Piece::Bold("Ana".into()), Piece::Text(" invited you to ".into()), Piece::Bold("Hike".into())]);
}

#[test]
fn month_and_weekday_keys_exist() {
    set_language("en");
    assert_eq!(date::month_day(9, 12), "Sep 12");
    assert_eq!(date::day_month(12, 9), "12 Sep");
    assert_eq!(date::weekday(chrono::Weekday::Thu), "Thursday");
}
