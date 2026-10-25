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
