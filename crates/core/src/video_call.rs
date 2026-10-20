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
use proto::{VideoCodec, VideoOffer};
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
mod hd;
mod live;
mod nerds;
mod path;
// The Android MediaCodec VP8 encoder/decoder (`codecs` names what it must provide).
#[cfg(all(target_os = "android", feature = "mediacodec"))]
mod mediacodec;
mod pipes;
mod tuning;
pub use tuning::{VideoTuning, video_tuning_stats_label};
pub use live::{VideoChoice, VideoLive};
pub use nerds::{VideoStatRow, video_stat_rows};
use self::codecs::EncoderChoice;

pub use self::ffi_bytes::Bytes;
#[cfg_attr(not(target_os = "android"), allow(unused_imports))]
pub(crate) use self::frames::chroma_spans;
pub(crate) use self::pipes::fit;
pub use self::adapt::{VideoBudget, VideoSendCap, video_codec_label, video_recv_label, video_send_label};
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
    tuning: Mutex<VideoTuning>,
    events: Mutex<Option<Arc<dyn VideoEvents>>>,
    /// Video state events go out on one thread of their own, in order (`emit_video`).
    dispatch: Mutex<Option<std::sync::mpsc::Sender<(Arc<dyn VideoEvents>, VideoInfo)>>>,
}

impl Default for NodeVideo {
    fn default() -> Self {
        Self { enabled: AtomicBool::new(true), tuning: Mutex::default(), events: Mutex::new(None), dispatch: Mutex::new(None) }
    }
}

impl NodeVideo {
    /// Hands the event to the dispatch thread (started on first use; it ends with the node).
    fn dispatch(&self, ev: Arc<dyn VideoEvents>, info: VideoInfo) {
        let mut tx = self.dispatch.lock();
        if tx.is_none() {
            let (send, recv) = std::sync::mpsc::channel::<(Arc<dyn VideoEvents>, VideoInfo)>();
            let spawned = std::thread::Builder::new().name("tl-video-events".into()).spawn(move || {
                for (ev, info) in recv {
                    ev.on_video_state(info);
                }
            });
            if let Err(e) = spawned {
                tracing::warn!("video events thread: {e}");
                drop(tx);
                ev.on_video_state(info);
                return;
            }
            *tx = Some(send);
        }
        let _ = tx.as_ref().expect("dispatch sender").send((ev, info));
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
    /// This second's send/receive pipeline timings, for the per-second debug log.
    timing: Mutex<Timing>,
    /// The encoder the send thread runs (set when video is decided; a live switch changes it
    /// once the peer has been told, see `live`).
    encoder: Mutex<Option<EncoderChoice>>,
    /// The peer said its video is this codec from its next keyframe (`Msg::VideoCodec`).
    recv_switch: Mutex<Option<VideoCodec>>,
    /// My decoder preference for this call: None Auto, Some(true) hardware, Some(false) software.
    decoder_pref: Mutex<Option<bool>>,
}

/// Where a frame's time goes on this device (debug log `video timing`, once a second): what the
/// rate control must not mistake for the network.
#[derive(Default)]
struct Timing {
    /// Encode time, and camera push -> handed to the transport, per frame sent (us).
    encode_us: Vec<u32>,
    lag_us: Vec<u32>,
    /// Camera frames replaced in the input slot before the encoder took them.
    replaced: u64,
    /// Longest wait to hand a received frame to the decoder (the network isn't read meanwhile,
    /// so arrival times stamped after it are late by that much), and how many waited > 5 ms.
    recv_wait_max_us: u32,
    recv_waits: u32,
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
    /// Either camera was on while video ran: the call log calls it a video call.
    seen: AtomicBool,
    peer_max_pixels: AtomicU32,
    /// The peer's offer once video is decided (what it decodes, whether it follows
    /// `Msg::VideoCodec`), and the encoder Auto means in this call.
    peer_video: Mutex<Option<VideoOffer>>,
    auto_encoder: Mutex<Option<EncoderChoice>>,
    /// A live encoder change from the in-call sheet, for `run_call`: it tells the peer first when
    /// the codec changes, then hands it to the send thread (`Pipe::encoder`).
    encoder_req: watch::Sender<Option<EncoderChoice>>,
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
            seen: AtomicBool::new(false),
            peer_max_pixels: AtomicU32::new(0),
            peer_video: Mutex::new(None),
            auto_encoder: Mutex::new(None),
            encoder_req: watch::channel(None).0,
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

    /// Live encoder requests from the in-call sheet, for `run_call` (see `encoder_req`).
    pub(crate) fn encoder_requests(&self) -> watch::Receiver<Option<EncoderChoice>> {
        self.encoder_req.subscribe()
    }

    /// The codec the send thread runs (or will run first).
    pub(crate) fn sending_codec(&self) -> Option<VideoCodec> {
        self.pipe.encoder.lock().map(|c| c.codec)
    }

    /// Hands a live encoder change to the send thread (after the peer was told, if needed).
    pub(crate) fn use_encoder(&self, choice: EncoderChoice) {
        *self.pipe.encoder.lock() = Some(choice);
    }

    /// The call is placed as a video call: the camera is on from the start, as if `set_camera
    /// (true)` came first. Nothing to emit: the UI hears it with the first `VideoInfo`.
    pub(crate) fn camera_on_from_start(&self) {
        self.camera.send_replace(true);
        self.pipe.resume.store(true, Ordering::SeqCst);
        tracing::info!("video camera on (placed as a video call)");
    }

    pub(crate) fn saw_camera(&self) {
        self.seen.store(true, Ordering::SeqCst);
    }

    /// Either camera was on at some point while video ran.
    pub(crate) fn camera_seen(&self) -> bool {
        self.seen.load(Ordering::SeqCst)
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
    /// Video is enabled on this node (`Node::set_video_enabled`).
    pub(crate) fn video_enabled(&self) -> bool {
        self.video.enabled.load(Ordering::SeqCst)
    }

    /// Our offer, when video is enabled on this node.
    pub(crate) fn video_offer(&self, call: &crate::node::Call) -> Option<VideoOffer> {
        self.video.offer(&call.video.pipe)
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
        // Never call the app back from inside one of its own calls (`set_camera` lands here):
        // on Android the callback runs on a thread JNA attached for it, a nested callback on it
        // detached that thread while Kotlin still ran on it, and ART aborted the app (16 Oct 2026,
        // phone-started video call cut at "Active"). The dispatch thread also keeps them in order.
        let ev = self.video.events.lock().clone();
        if let Some(ev) = ev {
            self.video.dispatch(ev, self.video_info(call));
        }
    }

    /// The call went active. With both offers present the video streams start on `conn`;
    /// either way the UI hears whether there is video.
    pub(crate) fn video_decided(&self, call: &Arc<crate::node::Call>, conn: &Connection, ours: Option<VideoOffer>, theirs: Option<VideoOffer>) {
        let offered = ours.is_some();
        if let (Some(ours), Some(peer)) = (ours, theirs) {
            let v = &call.video;
            // Each end computes both directions from both offers: no extra message.
            let Some((tx, rx)) = proto::video_codecs(&ours, &peer) else {
                v.pipe.stats.lock().tuning_problem = "Requested video codec unsupported by peer; audio only".into();
                self.emit_video(call);
                return;
            };
            *v.codecs.lock() = Some((tx, rx));
            let first = EncoderChoice { codec: tx, hardware: ours.hw_encode.contains(&tx) };
            *v.pipe.encoder.lock() = Some(first);
            *v.auto_encoder.lock() = Some(first);
            *v.peer_video.lock() = Some(peer.clone());
            {
                let mut st = v.pipe.stats.lock();
                st.send_codec = tx.as_str().into();
                st.recv_codec = rx.as_str().into();
                st.send_hardware = first.hardware;
                st.recv_hardware = ours.hw_decode.contains(&rx);
                st.send_encoder = codecs::encoder_name(first).into();
                st.peer_hw_decode = peer.hw_decode.iter().map(|c| c.as_str().to_string()).collect();
                st.live = VideoLive {
                    send_codec: "auto".into(),
                    max_bps: st.tuning.max_bps,
                    fixed_height: st.tuning.fixed_height,
                    fixed_fps: 0,
                    ignore_limits: false,
                    recv_decoder: live::decoder_id(st.tuning.recv_decoder.as_str()).into(),
                };
                *v.pipe.decoder_pref.lock() = live::decoder_pref(&st.live.recv_decoder);
            }
            tracing::info!("video codecs: send {} recv {}", tx.as_str(), rx.as_str());
            v.peer_max_pixels.store(peer.max_pixels, Ordering::SeqCst);
            {
                let mut pol = v.pipe.policy.lock();
                let max_fps = if peer.max_fps == 0 { FPS } else { (peer.max_fps as u32).min(FPS) };
                pol.peer = (pipes::max_rung(&LadderConfig::default(), peer.max_pixels), max_fps);
                pol.recv_pipelined = codecs::pipelined_decoder(rx);
                pol.peer_probes = peer.burst_probe;
            }
            v.negotiated.store(true, Ordering::SeqCst);
            let pipe = v.pipe.clone();
            let send = self.handle.spawn(run_send(conn.clone(), pipe.clone()));
            let recv = self.handle.spawn(run_recv(conn.clone(), pipe, rx, peer.hint_flags, peer.burst_probe));
            let policy = self.handle.spawn(adapt::run_policy(v.pipe.clone()));
            let path = self.handle.spawn(path::watch(conn.clone(), v.pipe.clone()));
            v.tasks.lock().extend([send, recv, policy, path]);
            if v.pipe.stop.load(Ordering::SeqCst) {
                // Ended meanwhile.
                v.stop();
            }
        }
        // No video in this call (the peer has none, or it is off here): a camera turned on before
        // the call was active (a video call placed, or answered with video) goes off, so the app
        // stops capturing and never shows a self-view nobody receives.
        let was_on = !call.video.negotiated() && call.video.camera.send_replace(false);
        if was_on {
            call.video.pipe.input.lock().take();
        }
        if offered || was_on {
            self.emit_video(call);
        }
    }

    /// The peer's `Msg::Video`.
    pub(crate) fn remote_camera(&self, call: &crate::node::Call, on: bool) {
        let v = &call.video;
        if !v.negotiated() || v.remote_camera.swap(on, Ordering::SeqCst) == on {
            return;
        }
        // With "video camera" (ours) these date the start of each picture in a call log.
        tracing::info!("video peer camera {}", if on { "on" } else { "off" });
        if on {
            v.saw_camera();
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
        if v.pipe.input.lock().replace(input).is_some() {
            v.pipe.timing.lock().replaced += 1;
        }
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
