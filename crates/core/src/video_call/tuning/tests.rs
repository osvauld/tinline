use super::*;

#[test]
fn changing_settings_does_not_mutate_an_existing_offer_or_call() {
    let settings = NodeVideo::default();
    let first = CallVideo::default();
    let original = settings.offer(&first.pipe).unwrap();
    let next = VideoTuning {
        send_codec: "av1".into(),
        max_bps: 1_000_000,
        fixed_height: 360,
        recv_decoder: "sw".into(),
        rate_control: "probe".into(),
    };
    *settings.tuning.lock() = next.clone();
    assert_eq!(first.pipe.stats.lock().tuning, VideoTuning::default());
    let second = CallVideo::default();
    let new_offer = settings.offer(&second.pipe).unwrap();
    assert_eq!(new_offer.encode, vec![VideoCodec::Av1]);
    assert_eq!(second.pipe.stats.lock().tuning, next);
    assert_eq!(original.encode, codecs::local_offer_codecs().encode);
    // Forcing a codec never hides that our sender reads sharper-video hint flags.
    assert!(original.hint_flags && new_offer.hint_flags);
    settings.enabled.store(false, Ordering::SeqCst);
    assert!(settings.offer(&Pipe::default()).is_none());
}
