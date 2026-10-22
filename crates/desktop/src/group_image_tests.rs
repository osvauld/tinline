//! Image posts on the desktop (docs/design/public-group.md §5): the picture button posts the picked
//! file with the composer's text as its caption, a picture that is not here loads on tap, and the
//! list line says "Photo". The rules (scaling, hosts, auto-download) stay in the core.

use super::*;

fn with_image(m: &mut GroupMessage, state: p2pcore::GroupImageState) {
    m.image = Some(p2pcore::GroupImage { width: 800, height: 600, size: 120_000, mime: "image/jpeg".into(), preview: vec![], state });
}

#[test]
fn a_picked_picture_posts_with_the_composer_text_as_its_caption() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    // No file picked: nothing happens.
    assert_eq!(l.step(&api, Lm::Picked(None)).job, None);
    l.step(&api, Lm::Input("  the view ".into()));
    let out = l.step(&api, Lm::Picked(Some("/home/me/view.png".into())));
    assert_eq!(out.job, Some(Job::PostImage { gid: "a".into(), path: "/home/me/view.png".into(), caption: "the view".into() }));
    assert!(l.input.is_empty() && l.sending);
    l.step(&api, out.job.unwrap().run(&api));
    assert!(!l.sending);
    let mine = l.msgs.last().unwrap();
    assert_eq!(mine.text, "the view");
    assert!(mine.image.is_some());
    assert_eq!(preview(l.summary("a").unwrap(), ME), "You: Photo \u{b7} the view");

    // A file the core refuses: the caption comes back, the reason shows.
    l.step(&api, Lm::Input("oops".into()));
    let said = run(&mut l, &api, Lm::Picked(Some("/home/me/notes.txt".into())));
    assert!(said.is_some_and(|s| s.contains("only PNG, JPEG and WebP")));
    assert_eq!(l.input, "oops");
}

#[test]
fn a_member_of_an_admins_only_group_cannot_post_a_picture() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    api.with("a", |g| {
        g.admins_only = true;
        Ok(())
    })
    .unwrap();
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    let out = l.step(&api, Lm::Picked(Some("/home/me/view.png".into())));
    assert_eq!(out.job, None);
    assert_eq!(out.notice.as_deref(), Some("Only admins post here."));
}

#[test]
fn a_picture_loads_on_tap_and_opens_into_a_file() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    let mut m = api.incoming("a", "did:key:sam", "");
    with_image(&mut m, p2pcore::GroupImageState::Remote);
    api.with("a", |g| {
        *g.msgs.last_mut().unwrap() = m.clone();
        Ok(())
    })
    .unwrap();
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    assert_eq!(preview(l.summary("a").unwrap(), ME), "sam: Photo");
    let out = l.step(&api, Lm::LoadImage(m.id.clone()));
    assert_eq!(out.job, Some(Job::FetchImage { gid: "a".into(), id: m.id.clone() }));
    l.step(&api, out.job.unwrap().run(&api));
    assert_eq!(api.fetched.borrow().as_slice(), [m.id.clone()]);

    // The core says it is here: the open thread re-reads it and the file is opened by a job.
    with_image(&mut m, p2pcore::GroupImageState::Ready);
    api.with("a", |g| {
        *g.msgs.last_mut().unwrap() = m.clone();
        Ok(())
    })
    .unwrap();
    l.image_asked.insert(m.id.clone());
    l.step(&api, Lm::Image("a".into(), m.id.clone()));
    assert!(!l.image_asked.contains(&m.id), "asked again after the event");
    assert_eq!(l.msgs.last().unwrap().image.as_ref().unwrap().state, p2pcore::GroupImageState::Ready);
    l.step(&api, Job::ImagePaths { gid: "a".into(), ids: vec![m.id.clone()] }.run(&api));
    assert_eq!(l.image_paths.get(&m.id).map(String::as_str), Some(format!("/pictures/{}.jpg", m.id).as_str()));
}
