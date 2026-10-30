//! File posts on the desktop (docs/design/group-files.md): the file button posts the picked file
//! with the composer's text as its caption; a card's Download, Cancel, Save and Delete my copy go
//! to the core; the list line says "File: name"; the holder line names others, never me. The
//! rules (sealing, hosts, auto-download) stay in the core.

use super::*;

fn with_file(m: &mut GroupMessage, state: p2pcore::GroupImageState, holders: &[(&str, &str)]) {
    m.file = Some(p2pcore::GroupFile {
        name: "Trip plan.pdf".into(),
        size: 300_000,
        mime: "application/pdf".into(),
        state,
        done: 0,
        kept_by_hosts: true,
        holders: holders.iter().map(|(d, n)| p2pcore::GroupFileHolder { did: (*d).into(), name: (*n).into() }).collect(),
    });
}

#[test]
fn a_picked_file_posts_with_the_composer_text_as_its_caption() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    assert_eq!(l.step(&api, Lm::PickedFile(None)).job, None, "nothing picked");
    l.step(&api, Lm::Input("  the plan ".into()));
    let out = l.step(&api, Lm::PickedFile(Some("/home/me/plan.pdf".into())));
    assert_eq!(out.job, Some(Job::PostFile { gid: "a".into(), path: "/home/me/plan.pdf".into(), caption: "the plan".into() }));
    assert!(l.input.is_empty() && l.sending);
    l.step(&api, out.job.unwrap().run(&api));
    assert!(!l.sending);
    let mine = l.msgs.last().unwrap();
    assert_eq!(mine.file.as_ref().unwrap().name, "plan.pdf");
    assert_eq!(preview(l.summary("a").unwrap(), ME), "You: File: plan.pdf");
}

#[test]
fn a_member_of_an_admins_only_group_cannot_post_a_file() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    api.with("a", |g| {
        g.admins_only = true;
        Ok(())
    })
    .unwrap();
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    let out = l.step(&api, Lm::PickedFile(Some("/home/me/plan.pdf".into())));
    assert_eq!(out.job, None);
    assert_eq!(out.notice.as_deref(), Some("Only admins post here."));
}

#[test]
fn a_file_card_downloads_cancels_saves_and_deletes_through_the_core() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    let mut m = api.incoming("a", "did:key:sam", "");
    with_file(&mut m, p2pcore::GroupImageState::Remote, &[("did:key:sam", "sam")]);
    api.with("a", |g| {
        *g.msgs.last_mut().unwrap() = m.clone();
        Ok(())
    })
    .unwrap();
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    let id = m.id.clone();
    for (msg, want) in [(Lm::LoadFile(id.clone()), Job::FetchFile { gid: "a".into(), id: id.clone() }), (Lm::CancelFile(id.clone()), Job::CancelFile { gid: "a".into(), id: id.clone() }), (Lm::DeleteFile(id.clone()), Job::DeleteFile { gid: "a".into(), id: id.clone() })] {
        let out = l.step(&api, msg);
        assert_eq!(out.job.as_ref(), Some(&want));
        l.step(&api, out.job.unwrap().run(&api));
    }
    let out = l.step(&api, Lm::SaveFile(id.clone()));
    let Some(Job::SaveFile { dest, .. }) = out.job.clone() else { panic!("{out:?}") };
    assert!(dest.ends_with("Trip plan.pdf") || dest.contains("Trip plan"), "{dest}");
    let said = l.step(&api, out.job.unwrap().run(&api)).notice;
    assert!(said.is_some_and(|s| s.contains("Trip plan")));
    assert_eq!(api.fetched.borrow().as_slice(), [format!("file:{id}"), format!("cancel:{id}"), format!("delete:{id}"), format!("save:{id}:{dest}")]);
}

#[test]
fn the_holder_line_names_others_never_me() {
    let mut m = msg("a", "m1", 1, "did:key:sam", "Sam Lee", GroupItemKind::Post, "");
    let name = |h: &p2pcore::GroupFileHolder| h.name.clone();
    with_file(&mut m, p2pcore::GroupImageState::Ready, &[(ME, "me")]);
    assert_eq!(gl_holders(&m, name), None, "only me");
    with_file(&mut m, p2pcore::GroupImageState::Ready, &[]);
    assert_eq!(gl_holders(&m, name), None, "nobody");
    with_file(&mut m, p2pcore::GroupImageState::Ready, &[(ME, "me"), ("did:key:anoop", "Anoop K")]);
    assert_eq!(gl_holders(&m, name).as_deref(), Some("Anoop is also hosting this file"));
    with_file(&mut m, p2pcore::GroupImageState::Remote, &[("did:key:anoop", "Anoop K"), (ME, "me"), ("did:key:bea", "Bea"), ("did:key:cy", "Cy")]);
    assert_eq!(gl_holders(&m, name).as_deref(), Some("Anoop and 2 others have this file"));
    with_file(&mut m, p2pcore::GroupImageState::Remote, &[("did:key:anoop", "Anoop K"), ("did:key:bea", "Bea")]);
    assert_eq!(gl_holders(&m, name).as_deref(), Some("Anoop and 1 other have this file"));
}

fn gl_holders(m: &GroupMessage, name: impl Fn(&p2pcore::GroupFileHolder) -> String) -> Option<String> {
    holders_line(m.file.as_ref().unwrap(), ME, name)
}
