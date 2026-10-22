//! Live controls for the current call (the in-call "stats for nerds" sheet, developer builds):
//! send codec, test size and bitrate cap, applied to the running sender. Settings > Diagnostics
//! tuning (`tuning`) stays the next-call default; this overrides it for this call only.
//!
//! - Size and cap are ours alone: `Sender::set_test_limits` on the running sender.
//! - VP8 hardware <-> VP8 software is the same bitstream: a new encoder and a keyframe.
//! - VP8 <-> AV1 changes what the peer decodes: `run_call` first sends `Msg::VideoCodec`
//!   ("my video is X from my next keyframe"), then the send thread swaps encoders; the peer's
//!   decoder swaps at the first keyframe of X. Only to a peer whose offer says `codec_switch`
//!   and that decodes X (`docs/video.md`, "Switching codecs mid-call").
//!
//! - The decoder (Auto / hardware / software, where this device has both for the arriving codec)
//!   is mine alone: the decode thread swaps at the next keyframe and asks the sender for one
//!   through the existing keyframe request (video control message 2). No protocol change.
//!
//! Nothing here turns a camera on: without my camera nothing is encoded, whatever is chosen.
use super::*;
use super::codecs::{EncoderChoice, decoder_kinds, decoder_name, local_encoders, local_offer_codecs};

/// This call's live overrides. `send_codec`: "auto" (what the call negotiated), "vp8-hw",
/// "vp8-sw" or "av1"; `max_bps` 0 or 0.5/1/2/3 Mbps; `fixed_height` 0, 360, 540 or 720 (the
/// pin); `fixed_fps` 0, 15, 24 or 30 (with a pinned size exactly that, else a ceiling);
/// `ignore_limits` (only with a pinned size): the pin holds against heat, battery saver and the
/// peer's hints, never above what the peer's offer takes; `recv_decoder` "auto", "hw" or "sw".
/// Per call, never saved: the next call starts from Settings again.
#[uniffi::remote(Record)]
pub struct VideoLive {
    pub send_codec: String,
    pub max_bps: u32,
    pub fixed_height: u32,
    pub fixed_fps: u32,
    pub ignore_limits: bool,
    pub recv_decoder: String,
}

/// "" and "auto" are Auto.
pub(super) fn decoder_id(id: &str) -> &str {
    if id.is_empty() { "auto" } else { id }
}

/// A decoder id as the decode thread's preference: None Auto, Some(true) hardware.
pub(super) fn decoder_pref(id: &str) -> Option<bool> {
    match id {
        "hw" => Some(true),
        "sw" => Some(false),
        _ => None,
    }
}

/// The decoder choices for `codec` arriving now: Auto, then hardware and software only where
/// this device has both (AV1 here is software only, so it gets Auto alone, labelled so).
pub(super) fn decoder_choices(codec: VideoCodec) -> Vec<VideoChoice> {
    let k = decoder_kinds(codec);
    let name = |hw| format!("{}, {}", video_codec_label(codec.as_str().into(), hw), decoder_name(codec, hw));
    let note = if k.hardware && k.software {
        String::new()
    } else {
        format!("The only {} decoder on this device", codec.as_str().to_uppercase())
    };
    let mut v = vec![VideoChoice { id: "auto".into(), label: format!("Auto ({})", name(k.auto_hardware)), available: true, why: note }];
    if k.hardware && k.software {
        for (id, hw) in [("hw", true), ("sw", false)] {
            v.push(VideoChoice { id: id.into(), label: name(hw), available: true, why: String::new() });
        }
    }
    v
}

/// One send-codec choice for the sheet. `available` false = shown greyed with `why`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VideoChoice {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub why: String,
}

fn choice_label(c: EncoderChoice) -> String {
    video_codec_label(c.codec.as_str().into(), c.hardware)
}

/// Whether `want` can be used in this call: `Err(why)` for the sheet and `set_video_live`.
/// `current` is the codec the peer decodes now.
pub(super) fn check(want: EncoderChoice, current: VideoCodec, peer: &VideoOffer, local: &[EncoderChoice]) -> Result<(), String> {
    if !local.contains(&want) {
        return Err("This device has no such encoder".into());
    }
    if !peer.decode.contains(&want.codec) {
        return Err(format!("Their device doesn’t decode {}", want.codec.as_str().to_uppercase()));
    }
    if want.codec != current && !peer.codec_switch {
        return Err("Their Tinline can’t switch codecs during a call. It applies from your next call".into());
    }
    Ok(())
}

/// The sender config for this call's live size, fps and cap: the pin bounded by what the peer's
/// offer takes (its decoder refuses more), whatever `ignore_limits` says.
pub(super) fn sender_config(pipe: &Pipe) -> SenderConfig {
    let mut cfg = SenderConfig::default();
    let (live, tuning) = {
        let st = pipe.stats.lock();
        (st.live.clone(), st.tuning.clone())
    };
    live.limits().configure(&mut cfg);
    tuning.configure_rate(&mut cfg, pipe.policy.lock().peer_probes);
    let (offer_rung, offer_fps) = pipe.policy.lock().peer;
    let l = &mut cfg.ladder;
    l.fixed_rung = l.fixed_rung.map(|r| r.min(offer_rung));
    l.fixed_fps = (live.fixed_fps != 0).then(|| live.fixed_fps.min(offer_fps));
    l.ignore_caps = live.ignore_limits && l.fixed_rung.is_some();
    cfg
}

/// Pushes this call's size, fps and cap into the running sender (no-op before it runs).
pub(super) fn apply_limits(pipe: &Pipe) {
    let Some(send) = pipe.send.lock().clone() else { return };
    let cfg = sender_config(pipe);
    send.with_sender(|s| s.set_test_limits(&cfg));
}

impl Inner {
    /// The peer's `Msg::VideoCodec`: its video is `codec` from its next keyframe.
    pub(crate) fn remote_codec(&self, call: &crate::node::Call, codec: VideoCodec) {
        let v = &call.video;
        if !v.negotiated() {
            return;
        }
        if !local_offer_codecs().decode.contains(&codec) {
            tracing::info!("video peer codec {}: not decoded here, ignored", codec.as_str());
            return;
        }
        tracing::info!("video peer codec -> {} (from its next keyframe)", codec.as_str());
        *v.pipe.recv_switch.lock() = Some(codec);
    }
}

#[uniffi::export]
impl Node {
    /// The current call's live overrides (`None` without a call with video).
    pub fn video_live(&self) -> Option<VideoLive> {
        let call = self.inner.current_video_call()?;
        call.video.negotiated().then(|| call.video.pipe.stats.lock().live.clone())
    }

    /// The send codecs for this call: Auto (what the call negotiated) and each encoder this device
    /// has, with whether the peer takes it now. Empty without a call with video.
    pub fn video_live_codecs(&self) -> Vec<VideoChoice> {
        let Some(call) = self.inner.current_video_call() else { return Vec::new() };
        let v = &call.video;
        let (Some(peer), Some(auto)) = (v.peer_video.lock().clone(), *v.auto_encoder.lock()) else { return Vec::new() };
        let current = v.pipe.encoder.lock().map_or(auto.codec, |c| c.codec);
        let local = local_encoders();
        let mut out = vec![VideoChoice { id: "auto".into(), label: format!("Auto ({})", choice_label(auto)), available: true, why: String::new() }];
        for c in &local {
            let ok = check(*c, current, &peer, &local);
            out.push(VideoChoice { id: c.id().into(), label: choice_label(*c), available: ok.is_ok(), why: ok.err().unwrap_or_default() });
        }
        out
    }

    /// The decoder choices for the codec arriving now (see `VideoLive::recv_decoder`). Empty
    /// without a call with video.
    pub fn video_live_decoders(&self) -> Vec<VideoChoice> {
        let Some(call) = self.inner.current_video_call() else { return Vec::new() };
        if !call.video.negotiated() {
            return Vec::new();
        }
        let codec = match call.video.pipe.stats.lock().recv_codec.as_str() {
            "vp8" => VideoCodec::Vp8,
            _ => VideoCodec::Av1,
        };
        decoder_choices(codec)
    }

    /// Applies `live` to the current call at once: the size and cap to the running sender, a new
    /// encoder (and, for another codec, `Msg::VideoCodec` to the peer first). Errors leave the
    /// call as it was. `NotFound` without a call, `Protocol` without video or for a choice the
    /// call can't take (the message says why).
    pub fn set_video_live(&self, live: VideoLive) -> Result<(), Error> {
        let call = self.inner.current_video_call().ok_or(Error::NotFound)?;
        let v = &call.video;
        if !v.negotiated() {
            return Err(Error::Protocol("no video in this call".into()));
        }
        live.limits().validate(&local_offer_codecs()).map_err(|e| Error::Protocol(e.to_string()))?;
        let peer = v.peer_video.lock().clone().ok_or(Error::NotFound)?;
        let auto = (*v.auto_encoder.lock()).ok_or(Error::NotFound)?;
        let want = match live.send_codec.as_str() {
            "" | "auto" => auto,
            id => EncoderChoice::from_id(id).ok_or_else(|| Error::Protocol("unknown send codec".into()))?,
        };
        let current = v.pipe.encoder.lock().map_or(auto.codec, |c| c.codec);
        if want != auto {
            check(want, current, &peer, &local_encoders()).map_err(Error::Protocol)?;
        }
        if !matches!(live.recv_decoder.as_str(), "" | "auto" | "hw" | "sw") {
            return Err(Error::Protocol("unknown decoder".into()));
        }
        if !matches!(live.fixed_fps, 0 | 15 | 24 | 30) {
            return Err(Error::Protocol("unsupported frame rate".into()));
        }
        if live.ignore_limits && live.fixed_height == 0 {
            return Err(Error::Protocol("Pin a size first: Ignore limits holds a pinned size".into()));
        }
        let live = VideoLive {
            send_codec: if want == auto { "auto".into() } else { want.id().into() },
            recv_decoder: decoder_id(&live.recv_decoder).into(),
            ..live
        };
        let before = std::mem::replace(&mut v.pipe.stats.lock().live, live.clone());
        tracing::info!(
            "video live: codec {} size {} fps {} cap {} ignore_limits={} decoder {} (was {before:?})",
            live.send_codec, live.fixed_height, live.fixed_fps, live.max_bps, live.ignore_limits, live.recv_decoder
        );
        apply_limits(&v.pipe);
        *v.pipe.decoder_pref.lock() = decoder_pref(&live.recv_decoder);
        v.encoder_req.send_if_modified(|r| std::mem::replace(r, Some(want)) != Some(want));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
}
