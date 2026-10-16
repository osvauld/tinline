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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::collections::VecDeque;
use std::time::Instant;

use iroh::endpoint::Connection;
use parking_lot::{Condvar, Mutex};
use proto::{VideoCodec, VideoOffer, send_codec};
use tokio::sync::{Notify, mpsc, watch};
use tokio::task::JoinHandle;
use video::convert::{self, Prepare, Range, RgbLayout, Rotation, Yuv420};
use video::media::{Frame, Hint, LadderConfig, SenderConfig};
use video::net::{RecvConfig, VideoRecv, VideoSend};
use video::{EncoderConfig, I420, VideoEncoder};

use crate::node::{CallState, Inner, Node};
use crate::Error;

/// Largest picture we decode, and what we offer to receive (the top rung, 720p).
pub(crate) const MAX_PIXELS: u32 = 1280 * 720;
/// Largest camera frame side `push_video_frame` takes.
const MAX_INPUT_SIDE: u32 = 4096;
const ENCODER_THREADS: usize = 2;
const DECODER_THREADS: usize = 2;
const FPS: u32 = 30;

/// Video state of the current call, as the call screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
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

mod adapt;
mod api;
mod codecs;
mod ffi_bytes;
mod frames;
// The Android MediaCodec VP8 encoder/decoder (`codecs` names what it must provide).
#[cfg(all(target_os = "android", feature = "mediacodec"))]
mod mediacodec;
mod pipes;

pub use self::ffi_bytes::Bytes;
#[cfg_attr(not(target_os = "android"), allow(unused_imports))]
pub(crate) use self::frames::chroma_spans;
pub(crate) use self::pipes::fit;
pub use self::adapt::{VideoBudget, VideoSendCap, video_send_label};
use self::frames::*;
use self::pipes::*;

#[path = "video_call/traffic.rs"]
mod traffic;

#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct VideoStats {
    pub frames_sent: u64,
    pub keyframes_sent: u64,
    pub send_width: u32,
    pub send_height: u32,
    pub bitrate_bps: u32,
    pub encode_ms: f32,
    pub frames_decoded: u64,
    pub decode_errors: u64,
    pub recv_width: u32,
    pub recv_height: u32,
    /// AV1 payload accepted into the local sender, NOT transmitted/ACKed QUIC bytes.
    pub admitted_payload_bytes: u64,
    /// Complete validated AV1 payload received before decode/decoder-queue admission.
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
    /// Frames per second sent and decoded, measured over the last second.
    pub send_fps: u32,
    pub recv_fps: u32,
    /// What caps our sending now (the peer's hint, our own budget, the peer's offer).
    pub send_cap: VideoSendCap,
    /// Ladder steps so far, and the reason of the last: "rate", "cpu", "hint" or "budget".
    pub ladder_steps: u64,
    pub last_step_reason: String,
}

impl VideoStats {
    fn record_admitted(&mut self, len: usize) {
        traffic::record(&mut self.admitted_payload_bytes, &mut self.frames_sent, len);
    }
    fn record_received(&mut self, len: usize) {
        traffic::record(&mut self.received_payload_bytes, &mut self.frames_received, len);
    }
}

/// Video state changes of the current call (`negotiated` is known when the call goes active;
/// camera on/off on either side). Frames are pulled, not pushed: see `pull_video_frame`.
#[uniffi::export(rust, foreign)]
pub trait VideoEvents: Send + Sync {
    fn on_video_state(&self, info: VideoInfo);
}

/// Node-wide video settings.
pub(crate) struct NodeVideo {
    enabled: AtomicBool,
    events: Mutex<Option<Arc<dyn VideoEvents>>>,
}

impl Default for NodeVideo {
    fn default() -> Self {
        Self { enabled: AtomicBool::new(true), events: Mutex::new(None) }
    }
}

/// Between the platform's threads and the codec threads of one call.
#[derive(Default)]
struct Pipe {
    input: Mutex<Option<Input>>,
    wake: Condvar,
    stop: AtomicBool,
    /// The camera came (back) on: start a new group.
    resume: AtomicBool,
    output: Mutex<Option<VideoFrame>>,
    seq: AtomicU64,
    /// The decoder failed: ask the sender for a keyframe.
    key_wanted: Notify,
    stats: Mutex<VideoStats>,
    /// Drawn size, budget and decode cost (`adapt`), turned into hints for the peer.
    policy: Mutex<adapt::Policy>,
    /// The newest hint for the receive task, and the wake for it.
    hint_slot: Mutex<Option<Hint>>,
    hint_changed: Notify,
    /// Our sender, once running (caps, steps and the peer's hint are read from it).
    send: Mutex<Option<Arc<VideoSend>>>,
    /// (when, ms) of recent decodes.
    decode_ms: Mutex<VecDeque<(Instant, f32)>>,
    /// (when, ms) of recent frame pulls + draws, reported by the app (`report_video_render_ms`).
    render_ms: Mutex<VecDeque<(Instant, f32)>>,
}

/// One call's video.
pub(crate) struct CallVideo {
    /// We offered video in this call (hello or accept) and the peer did too.
    negotiated: AtomicBool,
    /// What the peer offered in its hello (callee side), until we answer.
    pub(crate) peer_offer: Mutex<Option<VideoOffer>>,
    /// Codecs of (our sender, our receiver), once video is decided.
    codecs: Mutex<Option<(VideoCodec, VideoCodec)>>,
    camera: watch::Sender<bool>,
    remote_camera: AtomicBool,
    peer_max_pixels: AtomicU32,
    pipe: Arc<Pipe>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Default for CallVideo {
    fn default() -> Self {
        Self {
            negotiated: AtomicBool::new(false),
            peer_offer: Mutex::new(None),
            codecs: Mutex::new(None),
            camera: watch::channel(false).0,
            remote_camera: AtomicBool::new(false),
            peer_max_pixels: AtomicU32::new(0),
            pipe: Arc::default(),
            tasks: Mutex::new(Vec::new()),
        }
    }
}

impl CallVideo {
    pub(crate) fn negotiated(&self) -> bool {
        self.negotiated.load(Ordering::SeqCst)
    }

    /// Changes of the local camera, for `run_call` to tell the peer.
    pub(crate) fn camera_changes(&self) -> watch::Receiver<bool> {
        self.camera.subscribe()
    }

    pub(crate) fn camera(&self) -> bool {
        *self.camera.borrow()
    }

    /// Ends the codec threads and the streams. Idempotent.
    pub(crate) fn stop(&self) {
        self.pipe.stop.store(true, Ordering::SeqCst);
        self.pipe.wake.notify_all();
        for t in self.tasks.lock().drain(..) {
            t.abort();
        }
        self.pipe.output.lock().take();
    }
}

impl Drop for CallVideo {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Inner {
    /// Our offer, when video is enabled on this node.
    pub(crate) fn video_offer(&self) -> Option<VideoOffer> {
        self.video.enabled.load(Ordering::SeqCst).then(|| {
            let (decode, encode) = codecs::local_offer_codecs();
            VideoOffer { max_pixels: MAX_PIXELS, max_fps: FPS as u8, decode, encode }
        })
    }

    fn video_info(&self, call: &crate::node::Call) -> VideoInfo {
        let v = &call.video;
        VideoInfo {
            call_id: call.info.call_id.clone(),
            negotiated: v.negotiated(),
            local_camera: v.camera(),
            remote_camera: v.remote_camera.load(Ordering::SeqCst),
        }
    }

    pub(crate) fn emit_video(&self, call: &crate::node::Call) {
        let ev = self.video.events.lock().clone();
        if let Some(ev) = ev {
            ev.on_video_state(self.video_info(call));
        }
    }

    /// The call went active. With both offers present the video streams start on `conn`;
    /// either way the UI hears whether there is video.
    pub(crate) fn video_decided(&self, call: &Arc<crate::node::Call>, conn: &Connection, ours: Option<VideoOffer>, theirs: Option<VideoOffer>) {
        let offered = ours.is_some();
        if let (Some(ours), Some(peer)) = (ours, theirs) {
            let v = &call.video;
            // Each end computes both directions from both offers: no extra message.
            let (tx, rx) = (send_codec(&ours, &peer), send_codec(&peer, &ours));
            *v.codecs.lock() = Some((tx, rx));
            {
                let mut st = v.pipe.stats.lock();
                st.send_codec = tx.as_str().into();
                st.recv_codec = rx.as_str().into();
            }
            tracing::info!("video codecs: send {} recv {}", tx.as_str(), rx.as_str());
            v.peer_max_pixels.store(peer.max_pixels, Ordering::SeqCst);
            {
                let mut pol = v.pipe.policy.lock();
                let max_fps = if peer.max_fps == 0 { FPS } else { (peer.max_fps as u32).min(FPS) };
                pol.peer = (pipes::max_rung(&LadderConfig::default(), peer.max_pixels), max_fps);
            }
            v.negotiated.store(true, Ordering::SeqCst);
            let pipe = v.pipe.clone();
            let send = self.handle.spawn(run_send(conn.clone(), pipe.clone(), tx));
            let recv = self.handle.spawn(run_recv(conn.clone(), pipe, rx));
            let policy = self.handle.spawn(adapt::run_policy(v.pipe.clone()));
            v.tasks.lock().extend([send, recv, policy]);
            if v.pipe.stop.load(Ordering::SeqCst) {
                // Ended meanwhile.
                v.stop();
            }
        }
        if offered {
            self.emit_video(call);
        }
    }

    /// The peer's `Msg::Video`.
    pub(crate) fn remote_camera(&self, call: &crate::node::Call, on: bool) {
        let v = &call.video;
        if !v.negotiated() || v.remote_camera.swap(on, Ordering::SeqCst) == on {
            return;
        }
        if !on {
            v.pipe.output.lock().take();
        }
        self.emit_video(call);
    }

    fn current_video_call(&self) -> Option<Arc<crate::node::Call>> {
        self.live.lock().call.clone()
    }

    /// Hands a checked frame to the current call's encoder thread (latest wins).
    fn push_input(&self, width: u32, height: u32, rotation: u32, src: Source) -> bool {
        let Some(call) = self.current_video_call() else { return false };
        let v = &call.video;
        if !v.negotiated() || !v.camera() || v.pipe.stop.load(Ordering::SeqCst) {
            return false;
        }
        let input = Input { width: width as usize, height: height as usize, src, rotation, at: Instant::now() };
        *v.pipe.input.lock() = Some(input);
        v.pipe.wake.notify_one();
        true
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

/// What `pull_video_frame_rgba_into` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pulled {
    /// No new frame.
    None,
    /// A frame of this size waits but the buffer is under `width * height * 4` bytes; it stays.
    TooSmall { width: u32, height: u32 },
    /// The buffer starts with the frame as packed RGBA.
    Frame { width: u32, height: u32, seq: u64 },
}

impl Inner {
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
        if !yuv_fits(width, height, y, y_stride, u, v, uv_stride, uv_pixel_stride) {
            return false;
        }
        let (w, h) = (width as usize, height as usize);
        let (cw, ch) = I420::chroma(w, h);
        let chroma = plane_len(cw, ch, uv_stride, uv_pixel_stride);
        // One interleaved plane (u or v empty) spans one byte more than each of its views.
        let take = |p: &[u8], other: &[u8]| {
            let n = if other.is_empty() { chroma + 1 } else { chroma };
            if p.is_empty() { Vec::new() } else { p[..n].to_vec() }
        };
        let src = Source::Yuv {
            y: y[..plane_len(w, h, y_stride, 1)].to_vec(),
            y_stride,
            u: take(u, v),
            v: take(v, u),
            uv_stride,
            uv_pixel_stride,
            full_range,
        };
        self.push_input(width, height, rotation, src)
    }

    pub(crate) fn pull_rgba_into(&self, dst: &mut [u8]) -> Pulled {
        let Some(call) = self.current_video_call() else { return Pulled::None };
        let f = {
            let mut slot = call.video.pipe.output.lock();
            match slot.as_ref() {
                None => return Pulled::None,
                Some(f) if dst.len() < f.width as usize * f.height as usize * 4 => {
                    return Pulled::TooSmall { width: f.width, height: f.height };
                }
                Some(_) => slot.take().expect("checked"),
            }
        };
        let (width, height, seq) = (f.width, f.height, f.seq);
        let pic = I420 { width: width as usize, height: height as usize, data: f.data.0 };
        match convert::i420_to_rgba(&pic, dst) {
            Ok(()) => Pulled::Frame { width, height, seq },
            Err(_) => Pulled::None,
        }
    }
}

#[cfg(test)]
mod tests;
