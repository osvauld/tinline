//! Video in 1:1 calls (design: `docs/video.md`, "Call protocol").
//!
//! Negotiated in `CallHello`/`Accept` (`proto::VideoOffer` on both = video); once the call is
//! active both sides run `video::net` on the call's connection, each a `VideoSend` and a
//! `VideoRecv`, for the whole call. The camera only decides whether frames are pushed:
//! `Msg::Video { camera }` tells the peer, so its UI can show the picture or the avatar.
//!
//! Platform side: `push_video_frame` (I420), `push_video_frame_yuv` (Android `YUV_420_888`
//! planes as they come), `push_video_frame_rgba`/`_yuyv` (desktop cameras), any size and
//! rotation, feed a latest-wins slot; the encoder thread (pinned to the fast cores, 2 libaom
//! threads) converts it with `video::convert::prepare` (crop, scale to the sender's ladder
//! size, range, rotate in one pass), encodes and sends. The decoder thread (pinned, rav1d)
//! puts decoded frames into another latest-wins slot that the renderer reads with
//! `pull_video_frame` on its own clock (vsync), like `pull_speaker` for audio: a slow renderer
//! drops frames instead of queueing them, and no platform code runs on the codec threads.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use call_video::{RawFrame, VideoSink};
use parking_lot::Mutex;
use proto::VideoOffer;

use crate::node::Inner;

pub use call_video::{Pulled, VideoInfo};
pub use video_call_core::{VideoBudget, VideoLive, VideoSendCap, VideoStatRow, VideoStats, VideoTuning};

/// Video state of the current call, as the call screen shows it.
#[uniffi::remote(Record)]
pub struct VideoInfo {
    pub call_id: String,
    /// Both sides offered video, so it runs. False = audio only (the peer refused or can't).
    pub negotiated: bool,
    pub local_camera: bool,
    pub remote_camera: bool,
}

/// A decoded picture: tightly packed I420 (Y, then U, then V; chroma `ceil(w/2) x ceil(h/2)`),
/// upright (the sender rotated it).
#[derive(Debug, Clone, uniffi::Record)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub data: Bytes,
    /// Counts decoded frames; a gap means frames were replaced before being pulled.
    pub seq: u64,
}

impl From<RawFrame> for VideoFrame {
    fn from(f: RawFrame) -> Self {
        VideoFrame { width: f.width, height: f.height, data: Bytes(f.data), seq: f.seq }
    }
}

mod adapt;
mod api;
mod ffi_bytes;
mod live;
// The debug self-test export of the Android MediaCodec VP8 path (the codec is `video::mediacodec`).
#[cfg(all(target_os = "android", feature = "mediacodec"))]
mod mediacodec;
mod nerds;
mod tuning;
pub use tuning::video_tuning_stats_label;
pub use live::VideoChoice;
pub use nerds::video_stat_rows;

pub use self::ffi_bytes::Bytes;
#[cfg_attr(not(target_os = "android"), allow(unused_imports))]
pub(crate) use video_call_core::frames::chroma_spans;
pub use self::adapt::{video_codec_label, video_recv_label, video_send_label};

/// Video stats, one read per second. The record lives in `video_call_core::stats`; this declares
/// it to UniFFI.
#[uniffi::remote(Record)]
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
    /// The call's selected path (`video_call::path`): "lan", "internet", "relay" or "" before it is
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
    /// carry 540p or more and a 640x480 capture holds the picture back (`video_call::hd`,
    /// docs/video.md "Sharper video"). The app reopens the camera when it changes, unless the HD
    /// chip or Settings decide for it.
    pub hd_auto: bool,
}

/// Video state changes of the current call (`negotiated` is known when the call goes active;
/// camera on/off on either side). Frames are pulled, not pushed: see `pull_video_frame`.
#[uniffi::export(rust, foreign)]
pub trait VideoEvents: Send + Sync {
    fn on_video_state(&self, info: VideoInfo);
}

/// Hands the runtime's video state to the app's `VideoEvents`.
struct VideoEventsAdapter(Arc<dyn VideoEvents>);

impl VideoSink for VideoEventsAdapter {
    fn on_video_state(&self, info: call_video::VideoInfo) {
        self.0.on_video_state(info);
    }
}

impl Inner {
    /// Video is enabled on this node (`Node::set_video_enabled`).
    pub(crate) fn video_enabled(&self) -> bool {
        self.video.enabled()
    }

    /// Our offer, when video is enabled on this node.
    pub(crate) fn video_offer(&self, call: &crate::node::Call) -> Option<VideoOffer> {
        self.video.offer_for(&call.video)
    }

    pub(crate) fn emit_video(&self, call: &crate::node::Call) {
        self.video.emit(&call.info.call_id, &call.video);
    }

    /// The call went active. With both offers present the video streams start on `conn`;
    /// either way the UI hears whether there is video.
    pub(crate) fn video_decided(&self, call: &Arc<crate::node::Call>, conn: &iroh::endpoint::Connection, ours: Option<VideoOffer>, theirs: Option<VideoOffer>) {
        call.video.decide(&self.handle, conn, ours, theirs, &self.video, &call.info.call_id);
    }

    /// The peer's `Msg::Video`.
    pub(crate) fn remote_camera(&self, call: &crate::node::Call, on: bool) {
        if call.video.set_remote_camera(on) {
            self.emit_video(call);
        }
    }

    /// The peer's `Msg::VideoCodec`: its video is `codec` from its next keyframe.
    pub(crate) fn remote_codec(&self, call: &crate::node::Call, codec: proto::VideoCodec) {
        call.video.remote_codec(codec);
    }

    fn current_video_call(&self) -> Option<Arc<crate::node::Call>> {
        self.live.lock().call.clone()
    }

    /// `push_video_frame_yuv` from borrowed planes: each copied once, only the bytes the frame
    /// spans.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_yuv_planes(
        &self,
        width: u32,
        height: u32,
        rotation: u32,
        y: &[u8],
        y_stride: usize,
        u: &[u8],
        v: &[u8],
        uv_stride: usize,
        uv_pixel_stride: usize,
        full_range: bool,
    ) -> bool {
        let Some(call) = self.current_video_call() else { return false };
        call.video.push_yuv_planes(width, height, rotation, y, y_stride, u, v, uv_stride, uv_pixel_stride, full_range)
    }

    pub(crate) fn pull_rgba_into(&self, dst: &mut [u8]) -> Pulled {
        let Some(call) = self.current_video_call() else { return Pulled::None };
        call.video.pull_rgba_into(dst)
    }
}

/// Nodes reachable by the platform's direct frame calls (Android: `video_jni`), by handle: the
/// platform never holds a pointer into Rust, so a stale or bogus handle is just "no node".
static FRAME_PORTS: Mutex<Vec<(u64, std::sync::Weak<Inner>)>> = Mutex::new(Vec::new());
static NEXT_FRAME_PORT: AtomicU64 = AtomicU64::new(1);

/// The node behind a `Node::video_frame_handle`, while it lives.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn frame_port(handle: u64) -> Option<Arc<Inner>> {
    FRAME_PORTS.lock().iter().find(|(h, _)| *h == handle).and_then(|(_, w)| w.upgrade())
}

#[cfg(test)]
mod tests;
