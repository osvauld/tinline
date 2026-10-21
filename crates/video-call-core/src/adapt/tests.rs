use super::*;
use std::time::Duration;
use video::media::{Hint, HintReason};

fn px(w: u32, h: u32) -> u32 {
    w * h
}

#[test]
fn nothing_known_says_nothing() {
    let mut p = Policy::default();
    assert_eq!(p.update(Instant::now()), None);
}

#[test]
fn drawn_size_asks_one_rung_above_the_covering_rung() {
    let mut p = Policy::default();
    let t = Instant::now();
    for (w, h, want) in [
        (100, 60, px(640, 360)),
        (320, 180, px(640, 360)),
        (321, 180, px(960, 540)),
        (540, 960, px(1280, 720)),
        (608, 1080, px(1280, 720)),
        (900, 500, px(1280, 720)),
        (1280, 720, px(1280, 720)),
        (1920, 1080, u32::MAX),
    ] {
        let mut p2 = Policy::default();
        p2.set_view(0, w, h, true);
        let h = p2.update(t).unwrap_or_else(|| Hint { max_pixels: u32::MAX, max_fps: 0, reason: HintReason::None, room: false });
        assert_eq!(h.max_pixels, want);
    }
    p.set_view(0, 0, 0, false);
    assert_eq!(p.update(t).unwrap().max_pixels, 0);
}

#[test]
fn a_hidden_second_view_does_not_hide_the_shown_one() {
    // The call screen shows the picture full screen; the strip's tile, composed in the
    // stopped main screen behind it, reports hidden after it (2026-10-20: the Pixel sent
    // 180p15 to the moto's full-screen picture for a whole call).
    let t = Instant::now();
    let mut p = Policy::default();
    p.set_view(1, 1080, 2340, true);
    p.set_view(2, 240, 320, false);
    assert_eq!(p.update(t).map_or(u32::MAX, |h| h.max_pixels), u32::MAX);
    // Both hidden: hidden.
    p.set_view(1, 1080, 2340, false);
    assert_eq!(p.update(t).unwrap().max_pixels, 0);
}

#[test]
fn shrinking_holds_a_second_growing_goes_at_once() {
    let mut p = Policy::default();
    let t = Instant::now();
    p.set_view(0, 1280, 720, true);
    assert_eq!(p.update(t).unwrap().max_pixels, px(1280, 720));
    p.set_view(0, 640, 360, true); // asks for 540p, a shrink: held
    assert_eq!(p.update(t + Duration::from_millis(100)), None);
    p.set_view(0, 641, 361, true); // 540p covers it, so 720p: same as now, the hold is dropped
    assert_eq!(p.update(t + Duration::from_millis(200)), None);
    p.set_view(0, 640, 360, true);
    assert_eq!(p.update(t + Duration::from_millis(600)), None);
    assert_eq!(p.update(t + Duration::from_millis(1700)).unwrap().max_pixels, px(960, 540));
    p.set_view(0, 1280, 720, true);
    assert_eq!(p.update(t + Duration::from_millis(1800)).unwrap().max_pixels, px(1280, 720));
    // The same rung again is not news.
    p.set_view(0, 1200, 700, true);
    assert_eq!(p.update(t + Duration::from_millis(1900)), None);
}

#[test]
fn budget_caps_size_and_rate() {
    let mut p = Policy::default();
    let t = Instant::now();
    p.set_view(0, 1280, 720, true);
    p.update(t);
    p.budget = VideoBudget { thermal: 2, power_save: false };
    let h = p.update(t).unwrap();
    assert_eq!((h.max_pixels, h.max_fps, h.reason), (px(960, 540), 15, HintReason::Thermal));
    p.budget = VideoBudget { thermal: 3, power_save: true };
    let h = p.update(t).unwrap();
    assert_eq!((h.max_pixels, h.max_fps, h.reason), (px(640, 360), 15, HintReason::Thermal));
    p.budget = VideoBudget { thermal: 0, power_save: true };
    let h = p.update(t).unwrap();
    assert_eq!((h.max_pixels, h.max_fps, h.reason), (px(960, 540), 24, HintReason::PowerSave));
    // Our own ladder follows the budget.
    assert_eq!(p.own_caps(), (2, 24));
    p.budget = VideoBudget::default();
    assert_eq!(p.own_caps(), (3, u32::MAX));
}

#[test]
fn decode_cost_steers_the_rate_tier() {
    let mut p = Policy::default();
    let t = Instant::now();
    p.set_view(0, 1280, 720, true);
    p.update(t);
    // 25 ms is 75% of a 33 ms interval: down to 24, then (60 ms is 144% of 41) to 15.
    assert!(p.observe_decode(t, 25.0, 30));
    assert_eq!(p.update(t).unwrap().max_fps, 24);
    assert!(p.observe_decode(t, 30.0, 30));
    let h = p.update(t).unwrap();
    assert_eq!((h.max_fps, h.reason), (15, HintReason::DecodeSlow));
    // Too few samples say nothing; cheap decoding must hold 10 s before a tier up.
    assert!(!p.observe_decode(t, 100.0, 3));
    assert!(!p.observe_decode(t, 5.0, 30));
    assert!(!p.observe_decode(t + Duration::from_secs(9), 5.0, 30));
    assert!(p.observe_decode(t + Duration::from_secs(10), 5.0, 30));
    assert_eq!(p.update(t).unwrap().max_fps, 24);
}

/// One second per step: (received, decoded) frames per tick feed `hw_behind` as the run loop does.
fn run_hw(ticks: &[(u64, u64)], decode_ms: f32, render_ms: f32) -> Policy {
    let mut p = Policy::default();
    let t = Instant::now();
    p.set_view(0, 1280, 720, true);
    p.update(t);
    let mut hist: Vec<(u64, u64)> = Vec::new();
    for (i, &tick) in ticks.iter().enumerate() {
        hist.push(tick);
        let w = &hist[hist.len().saturating_sub(2)..];
        let (rx, dec) = w.iter().fold((0, 0), |a, e| (a.0 + e.0, a.1 + e.1));
        // The hardware path ignores `decode_ms` (pipeline latency).
        let _ = decode_ms;
        p.observe_decode_hw(t + Duration::from_secs(i as u64), hw_behind(rx, dec), render_ms, 30);
    }
    p
}

#[test]
fn hardware_decoder_judged_by_throughput_not_latency() {
    // 25 ms latency per frame (76% of a 33 ms interval: software would drop) but 30 in, 30 out.
    let mut p = run_hw(&[(30, 30); 20], 25.0, 4.0);
    assert_eq!(p.update(Instant::now()).map(|h| h.max_fps).unwrap_or(30), 30);
    // The same latency through the software rule does drop.
    let mut s = Policy::default();
    s.set_view(0, 1280, 720, true);
    s.update(Instant::now());
    assert!(s.observe_decode(Instant::now(), 25.0 + 4.0, 30));
}

#[test]
fn mediacodec_decoder_tier_comes_back() {
    // The moto g52 decodes VP8 in software but through MediaCodec: each call waits up to 20 ms
    // for its picture, which by wall time kept it at 15 fps for good (2026-10-18). Judged by
    // throughput, a tier lost once comes back after 10 s per step.
    let mut p = run_hw(&[(30, 20); 3], 22.0, 3.0);
    assert_eq!(p.update(Instant::now()).unwrap().max_fps, 15);
    let t = Instant::now() + Duration::from_secs(10);
    for i in 0..21 {
        p.observe_decode_hw(t + Duration::from_secs(i), false, 3.0, 30);
    }
    assert_eq!(p.update(t + Duration::from_secs(21)).map(|h| h.max_fps), Some(0));
}

#[test]
fn key_causes_log_text() {
    let c = video::media::KeyCauses { start: 1, rate: 2, request: 5, backlog: 6, ..Default::default() };
    assert_eq!(key_causes_text(&c), "start 1 rate 2 req 5 backlog 6");
}

#[test]
fn hardware_decoder_that_falls_behind_drops_the_tier() {
    // 30 in, 20 out per second, backlog building: down to 24 then 15.
    let mut p = run_hw(&[(30, 20); 3], 10.0, 2.0);
    assert_eq!(p.update(Instant::now()).unwrap().max_fps, 15);
    assert!(hw_behind(60, 40) && !hw_behind(60, 50) && !hw_behind(10, 0));
    // Render cost still counts when throughput is fine: 25 ms of drawing is 75% of 33 ms.
    let mut p2 = run_hw(&[(30, 30); 2], 5.0, 25.0);
    assert_eq!(p2.update(Instant::now()).unwrap().max_fps, 24);
}

#[test]
fn send_cap_names_what_binds() {
    let mut p = Policy::default();
    let (cap, src) = p.send_cap(None);
    assert_eq!((cap.pixels, cap.fps, cap.reason.as_str(), src), (px(1280, 720), 30, "none", ""));
    let hint = Hint { max_pixels: px(320, 180), max_fps: 15, reason: HintReason::Thermal, room: false };
    let (cap, src) = p.send_cap(Some(hint));
    assert_eq!((cap.pixels, cap.fps, cap.reason.as_str(), src), (px(320, 180), 15, "thermal", "hint"));
    let hidden = Hint { max_pixels: 0, max_fps: 0, reason: HintReason::DisplaySize, room: false };
    assert_eq!(p.send_cap(Some(hidden)).0.fps, 15);
    p.budget = VideoBudget { thermal: 0, power_save: true };
    let (cap, src) = p.send_cap(None);
    assert_eq!((cap.pixels, cap.fps, cap.reason.as_str(), src), (px(960, 540), 24, "battery", "budget"));
}

/// The sharper-video rules through the policy, one call per second: the receiver's room flag
/// rides on the hint (sent once when it changes), the core's HD choice follows my sending
/// rule, and battery saver or a hidden picture stop the flag.
#[test]
fn video_call_hd_room_through_the_policy() {
    let send = crate::hd::SendSample {
        frames: 30,
        send_px: px(640, 480) as u64,
        rung_px: px(1280, 720) as u64,
        cap_px: px(1280, 720) as u64,
        rate_bps: 2_000_000,
        encode_p75_us: Some(6_000),
        fps: 30,
        ..Default::default()
    };
    let recv = crate::hd::RecvSample { frames: 30, recv_px: px(640, 480) as u64, cost_ms: 4.0, cost_n: 60, ..Default::default() };
    let t = Instant::now();
    let mut p = Policy::default();
    // A full-screen phone picture: beyond every rung, so no hint at all until the flag.
    p.set_view(0, 1080, 2340, true);
    assert_eq!(p.update(t), None);
    for i in 1..crate::hd::ON_SECS {
        p.observe_room(send, recv);
        assert!(!p.auto.on, "second {i}");
        assert_eq!(p.update(t), None, "second {i}");
    }
    p.observe_room(send, recv);
    assert!(p.auto.on, "the camera goes to HD after ON_SECS");
    for _ in crate::hd::ON_SECS..crate::hd::ROOM_SECS - 1 {
        p.observe_room(send, recv);
    }
    assert_eq!(p.update(t), None);
    p.observe_room(send, recv);
    assert!(p.update(t).unwrap().room, "the flag goes to the peer at once");
    // The app follows: HD captured, the rung's size sent. It stays HD.
    p.hd_on = true;
    p.observe_room(crate::hd::SendSample { send_px: px(1280, 720) as u64, ..send }, recv);
    assert!(p.auto.on);
    // Battery saver caps my hint: the room flag goes, with the cap.
    p.budget = VideoBudget { thermal: 0, power_save: true };
    p.observe_room(send, recv);
    let h = p.update(t).unwrap();
    assert!(!h.room && h.reason == HintReason::PowerSave);
    // Hidden: no room either.
    let mut q = Policy::default();
    q.set_view(0, 1080, 2340, false);
    for _ in 0..30 {
        q.observe_room(send, recv);
    }
    assert!(!q.wanted().room);
}

#[test]
fn send_label_says_why() {
    let st = |w, h, fps, cap: &str, step: &str| VideoStats {
        send_width: w,
        send_height: h,
        send_fps: fps,
        send_cap: VideoSendCap { reason: cap.into(), ..Default::default() },
        last_step_reason: step.into(),
        ..Default::default()
    };
    assert_eq!(send_label(&st(0, 0, 0, "none", "")), "");
    assert_eq!(send_label(&st(1280, 720, 24, "none", "")), "HD \u{b7} 24 fps");
    assert_eq!(send_label(&st(960, 540, 30, "none", "rate")), "540p \u{b7} slow network");
    // A rate step down says "slow network" only while the rate is short of the next size up
    // (720p from 1.3 Mbps); back above it, the plain label while the ladder waits to step up.
    let back = VideoStats { bitrate_bps: 1_400_000, ..st(960, 540, 30, "none", "rate") };
    assert_eq!(send_label(&back), "540p \u{b7} 30 fps");
    let still = VideoStats { bitrate_bps: 1_200_000, ..st(960, 540, 30, "none", "rate") };
    assert_eq!(send_label(&still), "540p \u{b7} slow network");
    assert_eq!(send_label(&st(960, 540, 30, "display", "rate")), "540p \u{b7} their screen");
    assert_eq!(send_label(&st(540, 960, 15, "none", "")), "540p \u{b7} 15 fps");
    // A pin or the peer's offer is not battery saver; only the budget's own flags name one.
    assert_eq!(send_label(&st(960, 540, 30, "none", "pin")), "540p \u{b7} your pin");
    assert_eq!(send_label(&st(960, 540, 30, "none", "budget")), "540p \u{b7} 30 fps");
    let saver = VideoStats { budget: VideoBudget { thermal: 0, power_save: true }, ..st(960, 540, 24, "none", "budget") };
    assert_eq!(send_label(&saver), "540p \u{b7} battery saver");
}

#[test]
fn recv_label_names_their_video() {
    let st = |w, h, fps| VideoStats { recv_width: w, recv_height: h, recv_fps: fps, ..Default::default() };
    assert_eq!(recv_label(&st(0, 0, 0)), "");
    assert_eq!(recv_label(&st(720, 1280, 30)), "Their video \u{b7} HD \u{b7} 30 fps");
    assert_eq!(recv_label(&st(320, 180, 15)), "Their video \u{b7} 180p \u{b7} 15 fps");
    assert_eq!(recv_label(&st(960, 540, 0)), "Their video \u{b7} 540p");
}

#[test]
fn codec_label_names_codec_and_hardware() {
    assert_eq!(codec_label("vp8".into(), true), "VP8 \u{b7} hardware");
    assert_eq!(codec_label("av1".into(), false), "AV1 \u{b7} software");
    assert_eq!(codec_label(String::new(), true), "");
}
