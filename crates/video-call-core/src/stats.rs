//! The video stats the apps read each second (`VideoStats`) and the per-call live overrides they
//! carry (`VideoLive`). Filled by p2pcore's call runtime; the text built from them lives in
//! `adapt` (picture labels) and `nerds` (the sheet).

use crate::adapt::{VideoBudget, VideoSendCap};
use crate::tuning::VideoTuning;

/// This call's live overrides. `send_codec`: "auto" (what the call negotiated), "vp8-hw",
/// "vp8-sw" or "av1"; `max_bps` 0 or 0.5/1/2/3 Mbps; `fixed_height` 0, 360, 540 or 720 (the
/// pin); `fixed_fps` 0, 15, 24 or 30 (with a pinned size exactly that, else a ceiling);
/// `ignore_limits` (only with a pinned size): the pin holds against heat, battery saver and the
/// peer's hints, never above what the peer's offer takes; `recv_decoder` "auto", "hw" or "sw".
/// Per call, never saved: the next call starts from Settings again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VideoLive {
    pub send_codec: String,
    pub max_bps: u32,
    pub fixed_height: u32,
    pub fixed_fps: u32,
    pub ignore_limits: bool,
    pub recv_decoder: String,
}

impl VideoLive {
    /// The size and cap as tuning values (the codecs are handled here, not by `tuning`).
    pub fn limits(&self) -> VideoTuning {
        VideoTuning { max_bps: self.max_bps, fixed_height: self.fixed_height, ..Default::default() }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoStats {
    pub frames_sent: u64,
    pub keyframes_sent: u64,
    pub send_width: u32,
    pub send_height: u32,
    pub bitrate_bps: u32,
    pub encode_ms: f32,
    /// Decode wall-time p75 over two seconds (hardware pipeline latency, not CPU cost).
    pub decode_p75_ms: f32,
    /// Last measured second's admitted/received payload rate, not QUIC wire throughput.
    pub send_payload_bps: u32,
    pub recv_payload_bps: u32,
    /// The ladder rung (its landscape height: 180/360/540/720) and frame-rate tier the last sent
    /// frame was prepared for. The frame itself may be smaller (aspect kept, never upscaled):
    /// `send_width` x `send_height` is the actual size.
    pub send_rung_height: u32,
    pub send_rung_fps: u32,
    /// Settings snapshotted with our offer; later changes apply to a future call.
    pub tuning: VideoTuning,
    /// This call's live overrides from the in-call sheet (`Node::set_video_live`); starts as the
    /// snapshot's size and cap with the codec on Auto.
    pub live: VideoLive,
    pub tuning_problem: String,
    pub frames_decoded: u64,
    pub decode_errors: u64,
    pub recv_width: u32,
    pub recv_height: u32,
    /// Video payload accepted into the local sender, NOT transmitted/ACKed QUIC bytes.
    pub admitted_payload_bytes: u64,
    /// Complete validated video payload received before decode/decoder-queue admission.
    pub received_payload_bytes: u64,
    pub frames_received: u64,
    /// Receiver rate reports so far, and how many said "queueing" (delay above the overuse
    /// threshold) or "starved"; restarts after a backlog. For call logs.
    pub rate_reports: u64,
    pub rate_overuse: u64,
    pub rate_starved: u64,
    pub backlog_drops: u64,
    /// The last report's raw queueing delay and delivered rate, and the learned baseline.
    pub last_queuing_us: u64,
    pub last_delivered_bps: u32,
    pub queuing_offset_us: u64,
    pub draining: bool,
    /// Codec of each direction for this call ("av1" or "vp8"), empty before video is decided.
    pub send_codec: String,
    pub recv_codec: String,
    /// Whether the encoder we send with / the decoder we receive with is a hardware codec
    /// (MediaCodec hardware; libaom, rav1d and libvpx are software). False before video is decided.
    pub send_hardware: bool,
    pub recv_hardware: bool,
    /// The library behind each: "libaom", "rav1d", "libvpx", "MediaCodec" (the platform's
    /// default, hardware here) or "MediaCodec c2.android" (Google's software one). Empty before.
    pub send_encoder: String,
    pub recv_decoder: String,
    /// My device's budget (`set_video_budget`) and the codecs the peer decodes in hardware (its
    /// offer), so the sheet can name a cap: "360p: heat here (severe)", "their decoder (VP8 software)".
    pub budget: VideoBudget,
    pub peer_hw_decode: Vec<String>,
    /// The in-call "Ignore limits" switch is on and my pinned size/fps is above what the caps allow.
    pub limits_ignored: bool,
    /// The call's selected path (`path`): "lan", "internet", "relay" or "" before it is
    /// known; the remote address's type ("IPv4 private"; never the address); QUIC's RTT on it.
    pub path_kind: String,
    pub path_addr: String,
    pub path_rtt_ms: u32,
    /// Frames per second sent and decoded, measured over the last second.
    pub send_fps: u32,
    pub recv_fps: u32,
    /// What caps our sending now (the peer's hint, our own budget, the peer's offer).
    pub send_cap: VideoSendCap,
    /// Ladder steps so far, and the reason of the last: "rate", "cpu", "hint", "pin" or "budget".
    pub ladder_steps: u64,
    pub last_step_reason: String,
    /// The core wants the camera at HD (1280x720): the line, the encoder and the peer's screen
    /// carry 540p or more and a 640x480 capture holds the picture back (`hd`,
    /// docs/video.md "Sharper video"). The app reopens the camera when it changes, unless the HD
    /// chip or Settings decide for it.
    pub hd_auto: bool,
}

impl VideoStats {
    pub fn record_admitted(&mut self, len: usize) {
        crate::traffic::record(&mut self.admitted_payload_bytes, &mut self.frames_sent, len);
    }
    pub fn record_received(&mut self, len: usize) {
        crate::traffic::record(&mut self.received_payload_bytes, &mut self.frames_received, len);
    }
}

#[cfg(test)]
mod tests;
