use super::*;
use crate::i18n::{tr, tr_args, tr_count, tr_str};

fn work() -> AccountRef {
    AccountRef {
        did: "did:key:zWork".into(),
        name: "Work".into(),
    }
}

// These check which text is picked and what is filled in, not the wording: they compare with the
// translation table, so a copy edit in i18n/ does not break them.

#[test]
fn ring_names_the_account() {
    let ring = |name: &str| tr_args("accounts_ring_name", &[("name", name), ("account", "Work")]);
    assert_eq!(ring_name("Asha", Some(&work())), ring("Asha"));
    assert!(
        ring_name("Asha", Some(&work())).contains("Asha")
            && ring_name("Asha", Some(&work())).contains("Work")
    );
    assert_eq!(ring_name("Asha", None), "Asha");
    assert_eq!(
        ring_name("  ", Some(&work())),
        ring(tr_str("common_someone"))
    );
}

#[test]
fn calling_line_names_the_account() {
    assert_eq!(
        calling_line("Asha", Some(&work())),
        tr_args(
            "accounts_is_calling_you_on",
            &[("name", "Asha"), ("account", "Work")]
        )
    );
    assert!(calling_line("Asha", Some(&work())).contains("Asha"));
    assert_eq!(
        calling_line("", Some(&work())),
        tr_args("accounts_someone_is_calling_you_on", &[("account", "Work")])
    );
    assert!(calling_line("", Some(&work())).contains("Work"));
    assert_eq!(
        calling_line("Asha", None),
        tr_args("common_is_calling", &[("name", "Asha")])
    );
    assert_eq!(calling_line(" ", None), tr("common_someone_is_calling"));
}

#[test]
fn switch_line_counts() {
    assert_eq!(switch_line(0), tr("accounts_tap_to_switch"));
    assert_eq!(
        switch_line(1),
        tr_count("accounts_new_tap_to_switch", 1, &[])
    );
    assert_eq!(
        switch_line(3),
        tr_count("accounts_new_tap_to_switch", 3, &[])
    );
    assert!(switch_line(1).contains('1') && switch_line(3).contains('3'));
}
