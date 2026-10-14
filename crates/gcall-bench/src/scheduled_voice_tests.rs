use super::*;

#[test]
fn scheduled_voice_edges_and_silence_are_explicit() {
    let mut voice = Voice::new(7, 0.5);
    let mut pcm = [0i16; FRAME];
    let quiet = voice.next_scheduled_frame(&mut pcm, false);
    assert!(!quiet.onset && !quiet.talking);
    assert!(audio::rms(&pcm) < 200.0);
    let first = voice.next_scheduled_frame(&mut pcm, true);
    assert!(first.onset && first.talking);
    assert!(audio::rms(&pcm) > 200.0);
    assert!(!voice.next_scheduled_frame(&mut pcm, true).onset);
    let end = voice.next_scheduled_frame(&mut pcm, false);
    assert!(!end.onset && !end.talking);
    assert!(audio::rms(&pcm) < 200.0);
    assert!(voice.next_scheduled_frame(&mut pcm, true).onset);
}

#[test]
fn scheduled_voice_is_reproducible() {
    let mut a = Voice::new(17, 0.5);
    let mut b = Voice::new(17, 0.5);
    let (mut x, mut y) = ([0i16; FRAME], [0i16; FRAME]);
    for tick in 0..120 {
        let talking = talk_pattern::TalkPattern::Overlap.talking(1, 2, tick);
        let a = a.next_scheduled_frame(&mut x, talking);
        let b = b.next_scheduled_frame(&mut y, talking);
        assert_eq!((a.talking, a.onset), (b.talking, b.onset));
        assert_eq!(x, y);
    }
}
