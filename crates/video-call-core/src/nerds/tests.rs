use super::*;
use crate::adapt::VideoSendCap;

/// The 0.1.35 Pixel readout (2026-10-18): 720x1280 at 15 fps, capped to 15 fps by the moto's
/// decoder, receiving 180x240. Rung, actual size and cap each say what they are.
fn pixel() -> VideoStats {
    VideoStats {
        send_codec: "vp8".into(),
        send_hardware: true,
        send_encoder: "MediaCodec".into(),
        send_width: 720,
        send_height: 1280,
        send_fps: 15,
        send_rung_height: 720,
        send_rung_fps: 15,
        send_payload_bps: 2_400_000,
        bitrate_bps: 2_850_000,
        encode_ms: 6.04,
        send_cap: VideoSendCap { pixels: 1280 * 720, fps: 15, reason: "decode".into(), source: "peer".into() },
        peer_hw_decode: vec![],
        recv_codec: "av1".into(),
        recv_decoder: "rav1d".into(),
        recv_width: 180,
        recv_height: 240,
        recv_fps: 15,
        frames_received: 100,
        recv_payload_bps: 300_000,
        decode_p75_ms: 8.0,
        ..Default::default()
    }
}

#[test]
fn rows_keep_rung_size_and_cap_apart() {
    let rows = stat_rows(pixel(), 42, 0.3);
    let v = |i: usize| rows[i].value.as_str();
    assert_eq!(v(0), "VP8 \u{b7} hardware (MediaCodec)");
    assert_eq!(v(1), "720p (portrait 720x1280) \u{b7} 15 fps");
    assert_eq!(v(2), "720p \u{b7} 15 fps tier \u{b7} 0 steps");
    assert_eq!(v(3), "2400 kbps sent \u{b7} target 2850 \u{b7} encode 6.0 ms (wall time)");
    assert_eq!(v(4), "720p \u{b7} 15 fps: their decoder (VP8 \u{b7} software)");
    assert_eq!(v(5), "Auto");
    assert_eq!(v(6), "AV1 \u{b7} software (rav1d)");
    assert_eq!(v(7), "180p (portrait 180x240) \u{b7} 15 fps");
    assert_eq!(v(9), "42 ms round trip \u{b7} 0.3% lost");
    let mut lan = pixel();
    (lan.path_kind, lan.path_addr, lan.path_rtt_ms) = ("lan".into(), "IPv4 private".into(), 4);
    assert_eq!(connection_text(&lan, 42, 0.0), "Direct LAN (IPv4 private) \u{b7} 4 ms round trip \u{b7} 0.0% lost");
    assert_eq!(v(10), DASH);
}

/// Same rows, same labels whether or not video runs: the sheet never changes height.
#[test]
fn rows_are_fixed() {
    let full: Vec<_> = stat_rows(pixel(), 42, 0.0).into_iter().map(|r| (r.section, r.label)).collect();
    let empty: Vec<_> = stat_rows(VideoStats::default(), 0, 0.0).into_iter().map(|r| (r.section, r.label)).collect();
    assert_eq!(full, empty);
    assert_eq!(full.len(), 11);
    assert!(stat_rows(VideoStats::default(), 0, 0.0).iter().all(|r| r.value == DASH || r.label == "This call"));
}

#[test]
fn limits_name_their_owner_and_value() {
    let mut s = pixel();
    s.send_cap = VideoSendCap { pixels: 640 * 360, fps: 15, reason: "thermal".into(), source: "device".into() };
    s.budget.thermal = 3;
    assert_eq!(limit_text(&s), "360p \u{b7} 15 fps: heat on your device (Android severe)");
    s.live = VideoLive { fixed_height: 720, fixed_fps: 30, ignore_limits: true, ..Default::default() };
    assert!(limit_text(&s).starts_with("Limits ignored: may stutter or heat up (would be 360p"));
    assert_eq!(stat_rows(s.clone(), 0, 0.0)[10].value, "Limits ignored: may stutter or heat up");
    assert_eq!(pin_text(&s.live), "Pin 720p \u{b7} 30 fps");
    s.live = VideoLive { max_bps: 500_000, ..Default::default() };
    assert_eq!(pin_text(&s.live), "Pin cap 0.5 Mbps");
    s.send_cap = VideoSendCap { pixels: 1280 * 720, fps: 30, reason: "none".into(), source: String::new() };
    assert_eq!(limit_text(&s), "None");
    s.send_cap.source = "peer".into();
    s.send_cap.reason = "display".into();
    assert_eq!(cap_reason(&s), "their screen size");
}
