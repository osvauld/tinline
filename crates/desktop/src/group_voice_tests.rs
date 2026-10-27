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
