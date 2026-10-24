use super::*;

fn offer(decode: Vec<VideoCodec>, codec_switch: bool) -> VideoOffer {
    VideoOffer {
        max_pixels: MAX_PIXELS,
        max_fps: 30,
        decode,
        encode: vec![VideoCodec::Av1],
        hw_decode: vec![],
        hw_encode: vec![],
        hint_flags: true,
        codec_switch,
        burst_probe: false,
    }
}

const AV1: EncoderChoice = EncoderChoice { codec: VideoCodec::Av1, hardware: false };
const VP8_HW: EncoderChoice = EncoderChoice { codec: VideoCodec::Vp8, hardware: true };
const VP8_SW: EncoderChoice = EncoderChoice { codec: VideoCodec::Vp8, hardware: false };

/// A Pixel-like device (all three encoders) sending VP8 hardware to a peer.
#[test]
fn switch_rules_follow_the_peers_offer() {
    let local = [AV1, VP8_HW, VP8_SW];
    let new_peer = offer(vec![VideoCodec::Av1, VideoCodec::Vp8], true);
    let old_peer = offer(vec![VideoCodec::Av1, VideoCodec::Vp8], false);
    let av1_only = offer(vec![VideoCodec::Av1], true);
    // Hardware <-> software VP8 is sender-local: any peer, even one without the flag.
    assert_eq!(check(VP8_SW, VideoCodec::Vp8, &old_peer, &local), Ok(()));
    assert_eq!(check(VP8_HW, VideoCodec::Vp8, &old_peer, &local), Ok(()));
    // Another codec needs the peer's `codec_switch`, and it must decode it.
    assert_eq!(check(AV1, VideoCodec::Vp8, &new_peer, &local), Ok(()));
    assert!(check(AV1, VideoCodec::Vp8, &old_peer, &local).unwrap_err().contains("next call"));
    assert!(check(VP8_SW, VideoCodec::Av1, &av1_only, &local).unwrap_err().contains("decode VP8"));
    // Only encoders this device has.
    assert!(check(VP8_HW, VideoCodec::Av1, &new_peer, &[AV1, VP8_SW]).is_err());
}

/// Stand-in decoder kinds: both kinds for AV1 (two rav1d instances), so the swap rules run
/// on any host; `make` counts what it built.
fn both_kinds(_: VideoCodec) -> codecs::DecoderKinds {
    codecs::DecoderKinds { hardware: true, software: true, auto_hardware: true }
}
static BUILT: parking_lot::Mutex<Vec<bool>> = parking_lot::Mutex::new(Vec::new());
fn make(c: VideoCodec, hw: bool, max: u32) -> video::Result<Box<dyn video::VideoDecoder>> {
    BUILT.lock().push(hw);
    codecs::new_decoder(c, false, max)
}

/// Decoder: hardware -> software mid-stream. The old decoder keeps decoding until a keyframe,
/// one keyframe request goes out, the swap happens at the keyframe, and nothing fails.
#[test]
fn decoder_preference_swaps_at_the_next_keyframe_without_errors() {
    let (w, h) = (320, 180);
    let mut enc = video::aom::AomEncoder::new(&EncoderConfig { width: w, height: h, fps: 30, bitrate_bps: 300_000, threads: 1, keyint: 1000, ..Default::default() }).unwrap();
    let mut cam = video::Pattern::new(w, h);
    let mut frames = Vec::new();
    for g in 0..2u64 {
        for i in 0..15u32 {
            for p in enc.encode(&cam.next_frame(), i == 0).unwrap() {
                frames.push(Frame { group: g, index: i, capture_us: 0, payload: p.data });
            }
        }
    }
    assert_eq!(frames.len(), 30);
    let pipe = Arc::new(Pipe::default());
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    let p2 = pipe.clone();
    let dec = std::thread::spawn(move || {
        super::pipes::decode_loop_with(p2, rx, VideoCodec::Av1, codecs::Decoders { kinds: both_kinds, make })
    });
    let wait_decoded = |n: u64| {
        let end = Instant::now() + std::time::Duration::from_secs(10);
        while pipe.stats.lock().frames_decoded < n {
            assert!(Instant::now() < end, "decoded {}", pipe.stats.lock().frames_decoded);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    };
    let mut it = frames.into_iter();
    for f in it.by_ref().take(5) {
        tx.blocking_send(f).unwrap();
    }
    wait_decoded(5);
    assert!(pipe.stats.lock().recv_hardware, "Auto starts on the hardware kind");
    *pipe.decoder_pref.lock() = decoder_pref("sw");
    let asked = { let p = pipe.clone(); std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        rt.block_on(async { tokio::time::timeout(std::time::Duration::from_secs(10), p.key_wanted.notified()).await.is_ok() })
    }) };
    std::thread::sleep(std::time::Duration::from_millis(50));
    for f in it.by_ref().take(10) {
        tx.blocking_send(f).unwrap();
    }
    wait_decoded(15);
    assert!(asked.join().unwrap(), "a keyframe was asked for");
    assert!(pipe.stats.lock().recv_hardware, "still the old decoder until the keyframe");
    for f in it {
        tx.blocking_send(f).unwrap();
    }
    drop(tx);
    dec.join().unwrap();
    let st = pipe.stats.lock().clone();
    assert_eq!((st.frames_decoded, st.decode_errors), (30, 0), "{st:?}");
    assert!(!st.recv_hardware && st.recv_decoder == "rav1d", "{st:?}");
    assert_eq!(*BUILT.lock(), vec![true, false]);
}

#[test]
fn decoder_choices_offer_no_fake_kinds() {
    // AV1 is rav1d only: Auto alone, labelled as the only one.
    let av1 = decoder_choices(VideoCodec::Av1);
    assert_eq!(av1.len(), 1);
    assert_eq!(av1[0].label, "Auto (AV1 \u{b7} software, rav1d)");
    assert_eq!(av1[0].why, "The only AV1 decoder on this device");
    assert_eq!(decoder_pref("hw"), Some(true));
    assert_eq!(decoder_pref("auto"), None);
    assert_eq!(decoder_id(""), "auto");
}

#[test]
fn choice_ids_round_trip() {
    for c in [AV1, VP8_HW, VP8_SW] {
        assert_eq!(EncoderChoice::from_id(c.id()), Some(c));
    }
    assert_eq!(EncoderChoice::from_id("h264"), None);
    assert_eq!(choice_label(VP8_SW), "VP8 \u{b7} software");
}

/// Every device can name its encoders; AV1 is always one of them.
#[test]
fn local_encoders_always_have_av1() {
    let l = local_encoders();
    assert_eq!(l[0], AV1);
    #[cfg(not(target_os = "android"))]
    {
        // Desktop: libvpx (software) with feature `vp8`, never a hardware VP8 encoder.
        assert_eq!(l.contains(&VP8_SW), cfg!(feature = "vp8"));
        assert!(!l.contains(&VP8_HW));
    }
}

#[test]
fn keyframes_name_their_codec() {
    let mut enc = video::aom::AomEncoder::new(&EncoderConfig { width: 320, height: 180, fps: 30, bitrate_bps: 300_000, threads: 1, ..Default::default() }).unwrap();
    let pic = video::Pattern::new(320, 180).next_frame();
    let mut key = Vec::new();
    for _ in 0..3 {
        if let Some(p) = enc.encode(&pic, true).unwrap().into_iter().find(|p| p.key) {
            key = p.data;
            break;
        }
    }
    assert!(codecs::keyframe_is(VideoCodec::Av1, &key) && !codecs::keyframe_is(VideoCodec::Vp8, &key), "{:02x?}", &key[..8]);
    #[cfg(feature = "vp8")]
    {
        let mut enc = video::vpx::Vp8Encoder::new(&EncoderConfig { width: 320, height: 180, fps: 30, bitrate_bps: 300_000, threads: 1, ..Default::default() }).unwrap();
        let mut key = Vec::new();
        for _ in 0..3 {
            if let Some(p) = enc.encode(&pic, true).unwrap().into_iter().find(|p| p.key) {
                key = p.data;
                break;
            }
        }
        assert!(codecs::keyframe_is(VideoCodec::Vp8, &key) && !codecs::keyframe_is(VideoCodec::Av1, &key), "{:02x?}", &key[..8]);
    }
    assert!(!codecs::keyframe_is(VideoCodec::Av1, &[]) && !codecs::keyframe_is(VideoCodec::Vp8, &[0; 4]));
}
