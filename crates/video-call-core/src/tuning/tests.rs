use super::*;
use proto::{VideoCodec, VideoOffer, video_codecs as negotiated_codecs};

fn codecs() -> LocalCodecs {
    LocalCodecs {
        decode: vec![VideoCodec::Av1, VideoCodec::Vp8],
        encode: vec![VideoCodec::Vp8, VideoCodec::Av1],
        hw_decode: vec![VideoCodec::Vp8],
        hw_encode: vec![VideoCodec::Vp8],
    }
}
fn offer(c: LocalCodecs) -> VideoOffer {
    VideoOffer {
        max_pixels: 1280 * 720,
        max_fps: 30,
        decode: c.decode,
        encode: c.encode,
        hw_decode: c.hw_decode,
        hw_encode: c.hw_encode,
        hint_flags: true,
        codec_switch: true,
        burst_probe: true,
    }
}

#[test]
fn force_filters_only_send_and_beats_hardware_score() {
    let mut c = codecs();
    VideoTuning {
        send_codec: "av1".into(),
        ..Default::default()
    }
    .filter(&mut c);
    assert_eq!(c.encode, vec![VideoCodec::Av1]);
    assert!(c.hw_encode.is_empty());
    assert_eq!(c.decode.len(), 2);
    assert_eq!(
        negotiated_codecs(&offer(c), &offer(codecs())),
        Some((VideoCodec::Av1, VideoCodec::Vp8))
    );
}

#[test]
fn no_common_codec_is_not_a_silent_av1_fallback() {
    let mut sender = codecs();
    VideoTuning {
        send_codec: "vp8".into(),
        ..Default::default()
    }
    .filter(&mut sender);
    let mut peer = codecs();
    peer.decode = vec![VideoCodec::Av1];
    let (a, b) = (offer(sender), offer(peer));
    assert_eq!(negotiated_codecs(&a, &b), None);
    assert_eq!(negotiated_codecs(&b, &a), None);
}

#[test]
fn validates_controls_and_local_capability() {
    let mut c = codecs();
    c.encode = vec![VideoCodec::Av1];
    for t in [
        VideoTuning {
            send_codec: "vp8".into(),
            ..Default::default()
        },
        VideoTuning {
            send_codec: "h264".into(),
            ..Default::default()
        },
        VideoTuning {
            max_bps: 42,
            ..Default::default()
        },
        VideoTuning {
            fixed_height: 1080,
            ..Default::default()
        },
        VideoTuning {
            recv_decoder: "gpu".into(),
            ..Default::default()
        },
        VideoTuning {
            rate_control: "bbr".into(),
            ..Default::default()
        },
    ] {
        assert!(t.validate(&c).is_err());
    }
    assert!(VideoTuning::default().validate(&c).is_ok());
}

#[test]
fn readout_distinguishes_measured_payload_rate_from_target() {
    let s = VideoStats {
        send_width: 960,
        send_height: 540,
        send_payload_bps: 730_000,
        bitrate_bps: 1_000_000,
        decode_p75_ms: 12.5,
        frames_received: 3,
        send_rung_height: 540,
        send_rung_fps: 30,
        ..Default::default()
    };
    let label = tuning_stats_label(s);
    assert!(label.contains("730 kbps sent \u{b7} target 1000"), "{label}");
    assert!(label.contains("decode p75 12.5 ms"), "{label}");
    assert!(label.contains("Your rung: 540p \u{b7} 30 fps tier"), "{label}");
    assert!(label.contains("Your size: 540p (960x540)"), "{label}");
    assert_eq!(
        tuning_stats_label(VideoStats {
            tuning_problem: "unsupported".into(),
            ..Default::default()
        }),
        "unsupported"
    );
}

#[test]
fn defaults_leave_sender_untouched_and_rate_is_only_a_cap() {
    let mut cfg = SenderConfig::default();
    VideoTuning::default().configure(&mut cfg);
    assert_eq!(cfg.max_bps, 3_000_000);
    assert_eq!(cfg.ladder.fixed_rung, None);
    VideoTuning {
        max_bps: 1_000_000,
        fixed_height: 540,
        ..Default::default()
    }
    .configure(&mut cfg);
    assert_eq!(cfg.max_bps, 1_000_000);
    assert_eq!(cfg.start_bps, 500_000);
    assert_eq!(cfg.ladder.fixed_rung, Some(2));
    assert!(cfg.ladder.rungs.iter().all(|r| r.max_bps == 1_000_000));
}

#[test]
fn rate_control_probes_only_a_peer_that_takes_probes() {
    let t = |rc: &str| VideoTuning { rate_control: rc.into(), ..Default::default() };
    let mut cfg = SenderConfig::default();
    // The default is Cut+probe.
    for rc in ["", "auto", "probe"] {
        let mut cfg = SenderConfig::default();
        t(rc).configure_rate(&mut cfg, true);
        assert!(cfg.smooth_cut && cfg.burst_room, "{rc:?}");
        assert_eq!(cfg.detector, video::detect::Detector::Fixed);
    }
    t("plain").configure_rate(&mut cfg, true);
    assert!(!cfg.smooth_cut && !cfg.burst_room);
    t("cut").configure_rate(&mut cfg, true);
    assert!(cfg.smooth_cut && !cfg.burst_room);
    t("probe").configure_rate(&mut cfg, false);
    assert!(cfg.smooth_cut && !cfg.burst_room);
    t("trend").configure_rate(&mut cfg, true);
    assert!(cfg.smooth_cut && cfg.burst_room);
    assert_eq!(cfg.detector, video::detect::Detector::Trend);
}
