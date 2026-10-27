use super::*;

fn work() -> AccountRef {
    AccountRef {
        did: "did:key:zWork".into(),
        name: "Work".into(),
    }
}

#[test]
fn ring_names_the_account() {
    assert_eq!(ring_name("Asha", Some(&work())), "Asha \u{2192} Work");
    assert_eq!(ring_name("Asha", None), "Asha");
    assert_eq!(ring_name("  ", Some(&work())), "Someone \u{2192} Work");
}

#[test]
fn calling_line_names_the_account() {
    assert_eq!(
        calling_line("Asha", Some(&work())),
        "Asha is calling you on Work"
    );
    assert_eq!(
        calling_line("", Some(&work())),
        "Someone is calling you on Work"
    );
    assert_eq!(calling_line("Asha", None), "Asha is calling");
    assert_eq!(calling_line(" ", None), "Someone is calling");
}

#[test]
fn switch_line_counts() {
    assert_eq!(switch_line(0), "Tap to switch");
    assert_eq!(switch_line(1), "1 new message \u{b7} tap to switch");
    assert_eq!(switch_line(3), "3 new messages \u{b7} tap to switch");
}
