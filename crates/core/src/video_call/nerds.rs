//! "Stats for nerds": the in-call sheet's live readout, one text for both apps. Always the same
//! rows in the same order (blank values become "–"), so the sheet never changes height while it
//! refreshes. Names keep the rung, the actual size and the cap apart: the rung is what the ladder
//! aims at, the size is what the camera picture became (aspect kept, never upscaled), and the
//! limit says whose cap it is and its value.
use super::*;

/// One row of the sheet: `section` "send", "recv" or "link" (the apps group by it).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VideoStatRow {
    pub section: String,
    pub label: String,
    pub value: String,
}

const DASH: &str = "\u{2013}";

/// "720p (portrait 720x1280)", "360p (480x360)"; a dash before the first frame.
pub(super) fn size_name(w: u32, h: u32) -> String {
    if w == 0 || h == 0 {
        return DASH.into();
    }
    let p = w.min(h);
    if h > w { format!("{p}p (portrait {w}x{h})") } else { format!("{p}p ({w}x{h})") }
}

/// A pixel cap as its rung ("720p"), or the raw count off the ladder.
fn rung_of_pixels(px: u32) -> String {
    LadderConfig::default()
        .rungs
        .iter()
        .find(|r| r.width * r.height == px)
        .map_or_else(|| format!("{px} px"), |r| format!("{}p", r.height))
}

fn thermal_name(t: u8) -> &'static str {
    match t {
        0 => "none",
        1 => "light",
        2 => "moderate",
        3 => "severe",
        4 => "critical",
        5 => "emergency",
        _ => "shutdown",
    }
}

/// Whose limit `send_cap` is, in words: "their decoder (VP8 software)", "heat on your device
/// (Android severe)". Empty when nothing caps it.
pub(super) fn cap_reason(s: &VideoStats) -> String {
    let c = &s.send_cap;
    let theirs_hw = s.peer_hw_decode.iter().any(|x| *x == s.send_codec);
    match (c.source.as_str(), c.reason.as_str()) {
        ("peer", "display") => "their screen size".into(),
        ("peer", "decode") => format!("their decoder ({})", video_codec_label(s.send_codec.clone(), theirs_hw)),
        ("peer", "thermal") => "heat on their device".into(),
        ("peer", "battery") => "battery saver on their device".into(),
        ("peer", "user") => "their choice".into(),
        ("peer", _) => "their hint".into(),
        ("device", "thermal") => format!("heat on your device (Android {})", thermal_name(s.budget.thermal)),
        ("device", "battery") => "battery saver on your device".into(),
        ("device", _) => "their offer (the most they take)".into(),
        _ => String::new(),
    }
}

/// The limit row: "720p · 15 fps: their decoder (VP8 software)", "None", or, with the test
/// switch holding a pin above it, the limit and the warning.
pub(super) fn limit_text(s: &VideoStats) -> String {
    let why = cap_reason(s);
    if why.is_empty() {
        return if s.live.ignore_limits { "None now · limits ignored: may stutter or heat up".into() } else { "None".into() };
    }
    let cap = format!("{} \u{b7} {} fps: {why}", rung_of_pixels(s.send_cap.pixels), s.send_cap.fps);
    if s.live.ignore_limits {
        format!("Limits ignored: may stutter or heat up (would be {cap})")
    } else {
        cap
    }
}

/// "Pin 720p · 24 fps · cap 1 Mbps"; "Auto" with nothing pinned.
pub(super) fn pin_text(l: &VideoLive) -> String {
    let mut v = Vec::new();
    if l.fixed_height != 0 {
        v.push(format!("{}p", l.fixed_height));
    }
    if l.fixed_fps != 0 {
        v.push(format!("{} fps", l.fixed_fps));
    }
    if l.max_bps != 0 {
        v.push(format!("cap {} Mbps", l.max_bps as f32 / 1e6));
    }
    if v.is_empty() { "Auto".into() } else { format!("Pin {}", v.join(" \u{b7} ")) }
}

/// "Direct LAN (IPv4 private) · 4 ms round trip · 0.3% lost": the path from `video_call::path`, the RTT
/// measured on it (else the app's `rtt_ms`), and the audio loss the app measures.
pub(super) fn connection_text(s: &VideoStats, rtt_ms: u32, loss_pct: f64) -> String {
    let rtt = if s.path_rtt_ms != 0 { s.path_rtt_ms } else { rtt_ms };
    let kind = super::path::kind_name(&s.path_kind);
    let mut v = Vec::new();
    if !kind.is_empty() {
        v.push(format!("{kind} ({})", s.path_addr));
    }
    if rtt != 0 {
        v.push(format!("{rtt} ms round trip \u{b7} {loss_pct:.1}% lost"));
    }
    v.join(" \u{b7} ")
}

fn codec_text(codec: &str, hw: bool, lib: &str) -> String {
    if codec.is_empty() {
        return DASH.into();
    }
    let label = video_codec_label(codec.into(), hw);
    if lib.is_empty() { label } else { format!("{label} ({lib})") }
}

/// The sheet's rows from one `video_stats()` read and the call's `rtt_ms` / `loss_pct` (the app
/// has them from `call_stats()`). Always 11 rows, same labels, same order.
#[uniffi::export]
pub fn video_stat_rows(stats: VideoStats, rtt_ms: u32, loss_pct: f64) -> Vec<VideoStatRow> {
    let s = &stats;
    let row = |section: &str, label: &str, value: String| VideoStatRow {
        section: section.into(),
        label: label.into(),
        value: if value.is_empty() { DASH.into() } else { value },
    };
    let sending = s.send_width != 0;
    let fps = |f: u32, on: bool| if on { format!(" \u{b7} {f} fps") } else { String::new() };
    let wall = if s.send_hardware { " (wall time)" } else { "" };
    let note = if !s.tuning_problem.is_empty() {
        s.tuning_problem.clone()
    } else if s.live.ignore_limits {
        "Limits ignored: may stutter or heat up".into()
    } else {
        String::new()
    };
    vec![
        row("send", "Your video", codec_text(&s.send_codec, s.send_hardware, &s.send_encoder)),
        row("send", "Size", if sending { format!("{}{}", size_name(s.send_width, s.send_height), fps(s.send_fps, true)) } else { String::new() }),
        row(
            "send",
            "Rung",
            if s.send_rung_height == 0 {
                String::new()
            } else {
                let last = if s.last_step_reason.is_empty() { String::new() } else { format!(", last {}", s.last_step_reason) };
                format!("{}p \u{b7} {} fps tier \u{b7} {} steps{last}", s.send_rung_height, s.send_rung_fps, s.ladder_steps)
            },
        ),
        row(
            "send",
            "Rate",
            if sending {
                format!("{} kbps sent \u{b7} target {} \u{b7} encode {:.1} ms{wall}", s.send_payload_bps / 1000, s.bitrate_bps / 1000, s.encode_ms)
            } else {
                String::new()
            },
        ),
        row("send", "Limit", if s.send_cap.pixels == 0 { String::new() } else { limit_text(s) }),
        row("send", "This call", pin_text(&s.live)),
        row("recv", "Their video", codec_text(&s.recv_codec, s.recv_hardware, &s.recv_decoder)),
        row("recv", "Size", if s.recv_width != 0 { format!("{}{}", size_name(s.recv_width, s.recv_height), fps(s.recv_fps, true)) } else { String::new() }),
        row(
            "recv",
            "Rate",
            if s.frames_received != 0 {
                format!("{} kbps received \u{b7} decode p75 {:.1} ms \u{b7} {} errors", s.recv_payload_bps / 1000, s.decode_p75_ms, s.decode_errors)
            } else {
                String::new()
            },
        ),
        row("link", "Connection", connection_text(s, rtt_ms, loss_pct)),
        row("link", "Note", note),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let rows = video_stat_rows(pixel(), 42, 0.3);
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
        let full: Vec<_> = video_stat_rows(pixel(), 42, 0.0).into_iter().map(|r| (r.section, r.label)).collect();
        let empty: Vec<_> = video_stat_rows(VideoStats::default(), 0, 0.0).into_iter().map(|r| (r.section, r.label)).collect();
        assert_eq!(full, empty);
        assert_eq!(full.len(), 11);
        assert!(video_stat_rows(VideoStats::default(), 0, 0.0).iter().all(|r| r.value == DASH || r.label == "This call"));
    }

    #[test]
    fn limits_name_their_owner_and_value() {
        let mut s = pixel();
        s.send_cap = VideoSendCap { pixels: 640 * 360, fps: 15, reason: "thermal".into(), source: "device".into() };
        s.budget.thermal = 3;
        assert_eq!(limit_text(&s), "360p \u{b7} 15 fps: heat on your device (Android severe)");
        s.live = VideoLive { fixed_height: 720, fixed_fps: 30, ignore_limits: true, ..Default::default() };
        assert!(limit_text(&s).starts_with("Limits ignored: may stutter or heat up (would be 360p"));
        assert_eq!(video_stat_rows(s.clone(), 0, 0.0)[10].value, "Limits ignored: may stutter or heat up");
        assert_eq!(pin_text(&s.live), "Pin 720p \u{b7} 30 fps");
        s.live = VideoLive { max_bps: 500_000, ..Default::default() };
        assert_eq!(pin_text(&s.live), "Pin cap 0.5 Mbps");
        s.send_cap = VideoSendCap { pixels: 1280 * 720, fps: 30, reason: "none".into(), source: String::new() };
        assert_eq!(limit_text(&s), "None");
        s.send_cap.source = "peer".into();
        s.send_cap.reason = "display".into();
        assert_eq!(cap_reason(&s), "their screen size");
    }
}
