//! Voice posts on the desktop: the list line says "Voice message", a recording that is not here
//! loads on tap, and play/seek are left to the app (it holds the one voice player). The rules
//! (format, limits, hosts, auto-download) stay in the core.

use super::*;

fn with_voice(m: &mut GroupMessage, state: p2pcore::GroupImageState) {
    m.voice = Some(p2pcore::GroupVoice { duration_ms: 2_000, waveform: vec![128; 64], size: 6_000, state });
}

#[test]
fn a_voice_post_reads_as_voice_message_and_loads_on_tap() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    let mut m = api.incoming("a", "did:key:sam", "");
    with_voice(&mut m, p2pcore::GroupImageState::Remote);
    api.with("a", |g| {
        *g.msgs.last_mut().unwrap() = m.clone();
        Ok(())
    })
    .unwrap();
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    assert_eq!(preview(l.summary("a").unwrap(), ME), "sam: Voice message");
    let out = l.step(&api, Lm::LoadVoice(m.id.clone()));
    assert_eq!(out.job, Some(Job::FetchVoice { gid: "a".into(), id: m.id.clone() }));
    l.step(&api, out.job.unwrap().run(&api));
    assert_eq!(api.fetched.borrow().as_slice(), [format!("voice:{}", m.id)]);
    // Playing is the app's: the step asks for nothing.
    assert_eq!(l.step(&api, Lm::VoiceToggle(m.id.clone())), Out::default());
    assert_eq!(l.step(&api, Lm::VoiceSeek(m.id.clone(), 0.5)), Out::default());
}

#[test]
fn a_finished_recording_posts_as_a_voice_post_and_its_file_goes() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    // Text being written stays for later: a voice post does not take it.
    l.step(&api, Lm::Input("half a thought".into()));
    let file = std::env::temp_dir().join(format!("tinline-gv-test-{}.ogg", std::process::id()));
    std::fs::write(&file, b"ogg").unwrap();
    let job = Job::PostVoice { gid: "a".into(), path: file.to_string_lossy().into_owned(), waveform: vec![200; 64] };
    let said = l.step(&api, job.run(&api)).notice;
    assert_eq!(said, None);
    assert!(!file.exists(), "the core keeps its own copy; ours is deleted");
    let mine = l.msgs.last().unwrap();
    assert!(mine.outgoing && mine.voice.as_ref().is_some_and(|v| v.waveform == vec![200; 64]));
    assert_eq!(preview(l.summary("a").unwrap(), ME), "You: Voice message");
    assert_eq!(l.input, "half a thought");
}

#[test]
fn a_member_of_an_admins_only_group_cannot_post_a_recording() {
    let api = Fake::default();
    api.add("a", "did:key:sam", &[], &[]);
    api.with("a", |g| {
        g.admins_only = true;
        Ok(())
    })
    .unwrap();
    let mut l = live();
    run(&mut l, &api, Lm::Open("a".into()));
    // The composer (and its mic) is replaced by the calm line; the core refuses one sent anyway.
    assert!(l.admins_only_locked());
    let job = Job::PostVoice { gid: "a".into(), path: "/nowhere/rec.ogg".into(), waveform: vec![] };
    let said = l.step(&api, job.run(&api)).notice;
    assert_eq!(said.as_deref(), Some("Only admins post here."));
    assert!(l.msgs.iter().all(|m| m.voice.is_none()));
}
