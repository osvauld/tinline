//! Public groups on the desktop (docs/design/public-group.md §8): the header's "N online" and its
//! list, the open link and admins-only switches in Group info, and the composer of an admins-only
//! group. The rules stay in the core; these check what the screen shows and asks for.

use super::*;

fn online(api: &Fake, gid: &str, who: &[(&str, bool)]) {
    api.with(gid, |g| {
        g.present = who.iter().map(|(d, a)| (d.to_string(), d.trim_start_matches("did:key:").to_string(), *a)).collect();
        Ok(())
    })
    .unwrap();
}

fn set(api: &Fake, gid: &str, open: bool, admins_only: bool) {
    api.with(gid, |g| {
        g.open = open;
        g.admins_only = admins_only;
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_online_label_says_500_plus_at_the_cap() {
    assert_eq!(online_label(0), "0 online");
    assert_eq!(online_label(1), "1 online");
    assert_eq!(online_label(499), "499 online");
    // A host's frame stops at 500 DIDs, so 500 may be cut short.
    assert_eq!(online_label(500), "500+ online");
    assert_eq!(online_label(1200), "500+ online");
}

#[test]
fn online_rows_put_admins_first_then_names() {
    let m = |did: &str, name: &str, is_admin: bool| OnlineMember { did: did.into(), name: name.into(), is_admin };
    let list = vec![m("d1", "zoe", false), m("d2", "Ann", false), m("d3", "Max", true), m("d4", "bob", false)];
    let names: Vec<&str> = online_rows(&list).iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Max", "Ann", "bob", "zoe"]);
}

#[test]
fn only_an_open_group_shows_n_online_and_presence_refreshes_it() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &["did:key:priya"]);
    online(&api, "a", &[("did:key:sam", true), ("did:key:priya", false)]);
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    // Private: no count, no list.
    assert_eq!(l.online_label(), None);
    assert!(l.online.is_empty());
    run(&mut l, &api, Lm::ShowOnline);
    assert_eq!(l.dialog, None);

    set(&api, "a", true, false);
    run(&mut l, &api, Lm::Changed("a".into()));
    assert_eq!(l.online_label().as_deref(), Some("2 online"));

    // A presence event for another group changes nothing here; one for this group re-reads.
    online(&api, "a", &[("did:key:sam", true), ("did:key:priya", false), (ME, false)]);
    run(&mut l, &api, Lm::Presence("b".into()));
    assert_eq!(l.online_label().as_deref(), Some("2 online"));
    run(&mut l, &api, Lm::Presence("a".into()));
    assert_eq!(l.online_label().as_deref(), Some("3 online"));

    // The list, and whether I am shown in it.
    *api.hidden.borrow_mut() = true;
    run(&mut l, &api, Lm::ShowOnline);
    assert_eq!(l.dialog, Some(LDialog::Online));
    assert!(!l.presence_visible);
    assert_eq!(online_rows(&l.online)[0].did, "did:key:sam");
    run(&mut l, &api, Lm::Dismiss);
    assert_eq!(l.dialog, None);

    // Leaving the group: the count goes.
    run(&mut l, &api, Lm::Close);
    assert!(l.online.is_empty());
}

#[test]
fn a_member_of_an_admins_only_group_gets_no_composer_and_cannot_send() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    assert!(!l.admins_only_locked());

    set(&api, "a", true, true);
    run(&mut l, &api, Lm::Changed("a".into()));
    assert!(l.admins_only_locked());
    run(&mut l, &api, Lm::Input("hi".into()));
    let said = run(&mut l, &api, Lm::Send);
    assert_eq!(said.as_deref(), Some("Only admins post here."));
    assert_eq!(l.input, "hi");
    assert!(l.msgs.is_empty());

    // The core's refusal (a post raced the switch) reads the same.
    assert_eq!(gerr(Error::Rejected("admins only".into())), "Only admins post here.");
}

#[test]
fn an_admin_of_an_admins_only_group_keeps_the_composer() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[ME], &[]);
    set(&api, "a", false, true);
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    assert!(!l.admins_only_locked());
    run(&mut l, &api, Lm::Input("news".into()));
    run(&mut l, &api, Lm::Send);
    assert_eq!(l.msgs.last().map(|m| m.text.as_str()), Some("news"));
    // A removed member sees the removal footer, not the admins-only line.
    api.g.borrow_mut().get_mut("a").unwrap().removed = true;
    run(&mut l, &api, Lm::Changed("a".into()));
    assert!(!l.admins_only_locked());
}

#[test]
fn opening_asks_first_then_gets_the_link_and_copy_uses_it() {
    let api = Fake::default();
    api.add("a", ME, &[], &[]);
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    assert!(!l.setting_on(Setting::Open));

    // The switch asks first; Cancel changes nothing.
    run(&mut l, &api, Lm::SetOpen(true));
    assert_eq!(l.dialog, Some(LDialog::OpenGroup));
    run(&mut l, &api, Lm::Dismiss);
    assert!(!api.g.borrow()["a"].open);

    run(&mut l, &api, Lm::SetOpen(true));
    let said = run(&mut l, &api, Lm::Confirm);
    assert_eq!(said.as_deref(), Some("Your group is open. Copy the link to share it."));
    assert_eq!(l.dialog, None);
    assert!(l.setting_on(Setting::Open));
    assert_eq!(l.open_link_of_open(), Some("tinline://g/open-a"));
    assert_eq!(l.pending, None);

    // Copy: the link we hold, at once.
    let out = l.step(&api, Lm::CopyOpenLink);
    assert_eq!(out.copy.as_deref(), Some("tinline://g/open-a"));
    assert_eq!(out.job, None);
}

#[test]
fn copy_fetches_the_link_when_this_computer_has_none_yet() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[ME], &[]);
    set(&api, "a", true, false);
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    assert_eq!(l.open_link_of_open(), None);
    let out = l.step(&api, Lm::CopyOpenLink);
    assert_eq!(out.job, Some(Job::OpenLink { gid: "a".into(), copy: true }));
    assert_eq!(l.pending, Some((Setting::Link, true)));
    // A second press while it is on its way asks for nothing more.
    assert_eq!(l.step(&api, Lm::CopyOpenLink).job, None);
    let out = l.step(&api, out.job.unwrap().run(&api));
    assert_eq!(out.copy.as_deref(), Some("tinline://g/open-a"));
    assert!(out.notice.unwrap().starts_with("Copied."));
    assert_eq!(l.pending, None);
}

#[test]
fn closing_the_link_forgets_it_and_errors_show_as_a_notice() {
    let api = Fake::default();
    api.add("a", ME, &[], &[]);
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    run(&mut l, &api, Lm::SetOpen(true));
    run(&mut l, &api, Lm::Confirm);
    assert!(l.open_link_of_open().is_some());

    // While the change is on its way the switch shows the new value.
    let out = l.step(&api, Lm::SetOpen(false));
    assert_eq!(out.job, Some(Job::CloseLink("a".into())));
    assert!(!l.setting_on(Setting::Open));
    let out = l.step(&api, out.job.unwrap().run(&api));
    assert!(out.notice.unwrap().starts_with("The open link is closed."));
    assert!(!l.setting_on(Setting::Open));
    assert_eq!(l.open_link_of_open(), None);
    // Copy does nothing while the group is closed.
    assert_eq!(l.step(&api, Lm::CopyOpenLink), Out::default());

    // Admins only, on and off.
    let said = run(&mut l, &api, Lm::SetAdminsOnly(true));
    assert_eq!(said.as_deref(), Some("Only admins post now. Everyone can still read."));
    assert!(l.setting_on(Setting::AdminsOnly));
    run(&mut l, &api, Lm::SetAdminsOnly(false));
    assert!(!l.setting_on(Setting::AdminsOnly));

    // No host reachable: opening fails inside its dialog, with the reason.
    api.with("a", |g| {
        g.online = false;
        g.hosts.clear();
        Ok(())
    })
    .unwrap();
    run(&mut l, &api, Lm::SetOpen(true));
    run(&mut l, &api, Lm::Confirm);
    assert_eq!(l.dialog, Some(LDialog::OpenGroup));
    assert!(l.error.as_deref().unwrap().starts_with("No host answered"));
    assert!(!l.setting_on(Setting::Open));
}

#[test]
fn members_cannot_flip_the_settings() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    assert_eq!(l.step(&api, Lm::SetOpen(true)), Out::default());
    assert_eq!(l.step(&api, Lm::SetAdminsOnly(true)), Out::default());
    assert_eq!(l.dialog, None);
}

#[test]
fn a_refused_post_shows_the_reason_in_the_waiting_slot_and_never_looks_delivered() {
    let mut m = msg("g", "m1", 1, ME, "Me", GroupItemKind::Post, "written offline");
    assert_eq!(failed_line(&m), None);
    m.delivery = GroupDelivery::Failed;
    m.failure = Some("Not posted: only admins post here".into());
    assert_eq!(failed_line(&m), Some("Not posted: only admins post here"));
    // Someone else's post never carries the line.
    let mut other = msg("g", "m2", 2, "did:key:sam", "Sam", GroupItemKind::Post, "hi");
    other.delivery = GroupDelivery::Failed;
    other.failure = Some("x".into());
    assert_eq!(failed_line(&other), None);
    // The tick is the slack one (not sent), like waiting, from any receipt state.
    assert_eq!(live().tick_vis_of(&m), Some(crate::ticks::Vis::Slack));
}
