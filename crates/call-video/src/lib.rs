//! Video in 1:1 calls (design: `docs/video.md`, "Call protocol"). The runtime without UniFFI;
//! p2pcore wraps it in the `Node` video API.
//!
//! Negotiated in `CallHello`/`Accept` (`proto::VideoOffer` on both = video); once the call is
//! active both sides run `video::net` on the call's connection, each a `VideoSend` and a
//! `VideoRecv`, for the whole call. The camera only decides whether frames are pushed:
//! `Msg::Video { camera }` tells the peer, so its UI can show the picture or the avatar.
//!
//! Platform side: `CallVideo::push_*` (I420, Android `YUV_420_888` planes as they come, RGBA and
//! YUYV desktop cameras), any size and rotation, feed a latest-wins slot; the encoder thread
//! (pinned to the fast cores, 2 libaom threads) converts it with `video::convert::prepare` (crop,
//! scale to the sender's ladder size, range, rotate in one pass), encodes and sends. The decoder
//! thread (pinned, rav1d) puts decoded frames into another latest-wins slot that the renderer
//! reads with `take_frame` on its own clock (vsync), like `pull_speaker` for audio: a slow
//! renderer drops frames instead of queueing them, and no platform code runs on the codec threads.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use iroh::endpoint::Connection;
use parking_lot::{Condvar, Mutex};
use proto::{VideoCodec, VideoOffer};
use tokio::runtime::Handle;
use tokio::sync::{Notify, mpsc, watch};
use tokio::task::JoinHandle;
use video::convert;
use video::media::{Frame, Hint, LadderConfig, SenderConfig};
use video::net::{RecvConfig, VideoRecv, VideoSend};
use video::{EncoderConfig, I420, VideoEncoder};

/// Largest picture we decode, and what we offer to receive (the top rung, 720p).
pub const MAX_PIXELS: u32 = 1280 * 720;
const ENCODER_THREADS: usize = 2;
const DECODER_THREADS: usize = 2;
const FPS: u32 = 30;

mod adapt;
mod codecs;
mod live;
mod path;
mod pipes;
mod tuning;
pub use codecs::EncoderChoice;
pub use live::VideoChoice;
pub use video_call_core::frames::chroma_spans;
pub use video_call_core::{VideoBudget, VideoLive, VideoSendCap, VideoStats, VideoTuning};
use self::pipes::*;
use video_call_core::frames::*;

/// What can go wrong in the video controls. p2pcore maps it to its own `Error`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Protocol(String),
}

/// Video state of the current call, as the call screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoInfo {
    pub call_id: String,
    /// Both sides offered video, so it runs. False = audio only (the peer refused or can't).
    pub negotiated: bool,
    pub local_camera: bool,
    pub remote_camera: bool,
}

/// A decoded picture: tightly packed I420 (Y, then U, then V; chroma `ceil(w/2) x ceil(h/2)`),
/// upright (the sender rotated it).
#[derive(Debug, Clone)]
pub struct RawFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    /// Counts decoded frames; a gap means frames were replaced before being pulled.
    pub seq: u64,
}

impl RawFrame {
    /// This I420 frame as packed RGBA.
    pub fn into_rgba(self) -> RawFrame {
        let pic = I420 { width: self.width as usize, height: self.height as usize, data: self.data };
        RawFrame { data: convert::to_rgba(&pic), ..self }
    }
}

/// What `CallVideo::pull_rgba_into` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pulled {
    /// No new frame.
    None,
    /// A frame of this size waits but the buffer is under `width * height * 4` bytes; it stays.
    TooSmall { width: u32, height: u32 },
    /// The buffer starts with the frame as packed RGBA.
    Frame { width: u32, height: u32, seq: u64 },
}

/// Where video state changes of the current call go (`on_video_state` is called on one thread of
/// its own, in order). Frames are pulled, not pushed: see `CallVideo::take_frame`.
pub trait VideoSink: Send + Sync {
    fn on_video_state(&self, info: VideoInfo);
}

type Dispatch = std::sync::mpsc::Sender<(Arc<dyn VideoSink>, VideoInfo)>;

/// Node-wide video settings.
pub struct NodeVideo {
    enabled: AtomicBool,
    tuning: Mutex<VideoTuning>,
    sink: Mutex<Option<Arc<dyn VideoSink>>>,
    /// Video state events go out on one thread of their own, in order (`emit`).
    dispatch: Mutex<Option<Dispatch>>,
}

impl Default for NodeVideo {
    fn default() -> Self {
        Self { enabled: AtomicBool::new(true), tuning: Mutex::default(), sink: Mutex::new(None), dispatch: Mutex::new(None) }
    }
}

impl NodeVideo {
    /// Video is enabled on this node: calls offer (outgoing) and take (incoming) it.
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    /// Where `emit` delivers video state.
    pub fn set_sink(&self, sink: Arc<dyn VideoSink>) {
        *self.sink.lock() = Some(sink);
    }

    /// No sink: a background account's call shows no video events.
    pub fn clear_sink(&self) {
        *self.sink.lock() = None;
    }

    /// Tells the sink how `call_id`'s video stands now.
    pub fn emit(&self, call_id: &str, video: &CallVideo) {
        // Never call the app back from inside one of its own calls (`set_camera` lands here):
        // on Android the callback runs on a thread JNA attached for it, a nested callback on it
        // detached that thread while Kotlin still ran on it, and ART aborted the app (16 Oct 2026,
        // phone-started video call cut at "Active"). The dispatch thread also keeps them in order.
        let sink = self.sink.lock().clone();
        if let Some(sink) = sink {
            self.dispatch(sink, video.info(call_id));
        }
    }

    /// Hands the event to the dispatch thread (started on first use; it ends with the node).
    fn dispatch(&self, ev: Arc<dyn VideoSink>, info: VideoInfo) {
        let mut tx = self.dispatch.lock();
        if tx.is_none() {
            let (send, recv) = std::sync::mpsc::channel::<(Arc<dyn VideoSink>, VideoInfo)>();
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
    output: Mutex<Option<RawFrame>>,
    seq: AtomicU64,
    /// The decoder failed: ask the sender for a keyframe.
    key_wanted: Notify,
    stats: Mutex<VideoStats>,
    /// Drawn size, budget and decode cost (`adapt`), turned into hints for the peer.
    policy: Mutex<video_call_core::adapt::Policy>,
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
pub struct CallVideo {
    /// We offered video in this call (hello or accept) and the peer did too.
    negotiated: AtomicBool,
    /// What the peer offered in its hello (callee side), until we answer.
    pub peer_offer: Mutex<Option<VideoOffer>>,
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
    pub fn negotiated(&self) -> bool {
        self.negotiated.load(Ordering::SeqCst)
    }

    /// Changes of the local camera, for `run_call` to tell the peer.
    pub fn camera_changes(&self) -> watch::Receiver<bool> {
        self.camera.subscribe()
    }

    pub fn camera(&self) -> bool {
        *self.camera.borrow()
    }

    /// Live encoder requests from the in-call sheet, for `run_call` (see `encoder_req`).
    pub fn encoder_requests(&self) -> watch::Receiver<Option<EncoderChoice>> {
        self.encoder_req.subscribe()
    }

    /// The codec the send thread runs (or will run first).
    pub fn sending_codec(&self) -> Option<VideoCodec> {
        self.pipe.encoder.lock().map(|c| c.codec)
    }

    /// Hands a live encoder change to the send thread (after the peer was told, if needed).
    pub fn use_encoder(&self, choice: EncoderChoice) {
        *self.pipe.encoder.lock() = Some(choice);
    }

    /// The call is placed as a video call: the camera is on from the start, as if `set_camera
    /// (true)` came first. Nothing to emit: the UI hears it with the first `VideoInfo`.
    pub fn camera_on_from_start(&self) {
        self.camera.send_replace(true);
        self.pipe.resume.store(true, Ordering::SeqCst);
        tracing::info!("video camera on (placed as a video call)");
    }

    pub fn saw_camera(&self) {
        self.seen.store(true, Ordering::SeqCst);
    }

    /// Either camera was on at some point while video ran.
    pub fn camera_seen(&self) -> bool {
        self.seen.load(Ordering::SeqCst)
    }

    /// Ends the codec threads and the streams. Idempotent.
    pub fn stop(&self) {
        self.pipe.stop.store(true, Ordering::SeqCst);
        self.pipe.wake.notify_all();
        for t in self.tasks.lock().drain(..) {
            t.abort();
        }
        self.pipe.output.lock().take();
    }

    /// The video state of call `call_id`, as the call screen shows it.
    pub fn info(&self, call_id: &str) -> VideoInfo {
        VideoInfo {
            call_id: call_id.to_string(),
            negotiated: self.negotiated(),
            local_camera: self.camera(),
            remote_camera: self.remote_camera.load(Ordering::SeqCst),
        }
    }

    /// Turns our camera on or off (also before the call is answered: it applies once video runs).
    /// `active`: the call is active. `Ok(true)` = it changed, so the caller emits the new state;
    /// `Protocol` once active without video.
    pub fn set_camera(&self, on: bool, active: bool) -> Result<bool, Error> {
        if active && !self.negotiated() {
            return Err(Error::Protocol("no video in this call".into()));
        }
        let changed = self.camera.send_if_modified(|c| std::mem::replace(c, on) != on);
        if changed {
            // The moment the person turned it on: the call log's start of "my camera" (the first
            // `video send` line after it shows when frames got through).
            tracing::info!("video camera {}", if on { "on" } else { "off" });
            if on {
                self.pipe.resume.store(true, Ordering::SeqCst);
            } else {
                self.pipe.input.lock().take();
            }
        }
        Ok(changed)
    }

    /// The peer's `Msg::Video`. True = it changed, so the caller emits the new state.
    pub fn set_remote_camera(&self, on: bool) -> bool {
        if !self.negotiated() || self.remote_camera.swap(on, Ordering::SeqCst) == on {
            return false;
        }
        // With "video camera" (ours) these date the start of each picture in a call log.
        tracing::info!("video peer camera {}", if on { "on" } else { "off" });
        if on {
            self.saw_camera();
        }
        if !on {
            self.pipe.output.lock().take();
        }
        true
    }

    /// The call went active. With both offers present the video streams start on `conn`;
    /// either way the UI hears whether there is video (through `nv`, as call `call_id`).
    pub fn decide(&self, handle: &Handle, conn: &Connection, ours: Option<VideoOffer>, theirs: Option<VideoOffer>, nv: &NodeVideo, call_id: &str) {
        let offered = ours.is_some();
        if let (Some(ours), Some(peer)) = (ours, theirs) {
            let v = self;
            // Each end computes both directions from both offers: no extra message.
            let Some((tx, rx)) = proto::video_codecs(&ours, &peer) else {
                v.pipe.stats.lock().tuning_problem = "Requested video codec unsupported by peer; audio only".into();
                nv.emit(call_id, self);
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
            let send = handle.spawn(run_send(conn.clone(), pipe.clone()));
            let recv = handle.spawn(run_recv(conn.clone(), pipe, rx, peer.hint_flags, peer.burst_probe));
            let policy = handle.spawn(adapt::run_policy(v.pipe.clone()));
            let path = handle.spawn(path::watch(conn.clone(), v.pipe.clone()));
            v.tasks.lock().extend([send, recv, policy, path]);
            if v.pipe.stop.load(Ordering::SeqCst) {
                // Ended meanwhile.
                v.stop();
            }
        }
        // No video in this call (the peer has none, or it is off here): a camera turned on before
        // the call was active (a video call placed, or answered with video) goes off, so the app
        // stops capturing and never shows a self-view nobody receives.
        let was_on = !self.negotiated() && self.camera.send_replace(false);
        if was_on {
            self.pipe.input.lock().take();
        }
        if offered || was_on {
            nv.emit(call_id, self);
        }
    }

    /// Hands a checked frame to the encoder thread (latest wins).
    fn push_input(&self, width: u32, height: u32, rotation: u32, src: Source) -> bool {
        if !self.negotiated() || !self.camera() || self.pipe.stop.load(Ordering::SeqCst) {
            return false;
        }
        let input = Input { width: width as usize, height: height as usize, src, rotation, at: Instant::now() };
        if self.pipe.input.lock().replace(input).is_some() {
            self.pipe.timing.lock().replaced += 1;
        }
        self.pipe.wake.notify_one();
        true
    }

    /// One camera frame: tightly packed I420 of `width` x `height`, to be turned `rotation`
    /// degrees clockwise (0/90/180/270) to be upright. Any size up to 4096 a side; it is
    /// scaled to what the link carries. Latest wins: a frame not yet taken by the encoder is
    /// replaced. False if not sent (no video running, camera off, or a malformed frame).
    pub fn push_i420(&self, width: u32, height: u32, rotation: u32, data: Vec<u8>) -> bool {
        if !size_ok(width, height) || data.len() != I420::len(width as usize, height as usize) {
            return false;
        }
        let pic = I420 { width: width as usize, height: height as usize, data };
        self.push_input(width, height, rotation, Source::I420(pic))
    }

    /// One `YUV_420_888` frame as its planes, owned (see `push_yuv_planes` for borrowed ones).
    #[allow(clippy::too_many_arguments)]
    pub fn push_yuv(
        &self,
        width: u32,
        height: u32,
        rotation: u32,
        y: Vec<u8>,
        y_stride: usize,
        u: Vec<u8>,
        v: Vec<u8>,
        uv_stride: usize,
        uv_pixel_stride: usize,
        full_range: bool,
    ) -> bool {
        if !yuv_fits(width, height, &y, y_stride, &u, &v, uv_stride, uv_pixel_stride) {
            return false;
        }
        let src = Source::Yuv { y, y_stride, u, v, uv_stride, uv_pixel_stride, full_range };
        self.push_input(width, height, rotation, src)
    }

    /// `push_yuv` from borrowed planes: each copied once, only the bytes the frame spans.
    #[allow(clippy::too_many_arguments)]
    pub fn push_yuv_planes(
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

    /// One frame of packed RGBA (`width * height * 4` bytes), converted on the encoder thread.
    pub fn push_rgba(&self, width: u32, height: u32, rotation: u32, data: Vec<u8>) -> bool {
        if !size_ok(width, height) || data.len() != width as usize * height as usize * 4 {
            return false;
        }
        self.push_input(width, height, rotation, Source::Rgba(data))
    }

    /// One frame of packed YUYV (YUY2), rows `stride` bytes apart (at least `width * 2`; even
    /// width), converted on the encoder thread.
    pub fn push_yuyv(&self, width: u32, height: u32, rotation: u32, stride: u32, data: Vec<u8>) -> bool {
        let w = width as usize;
        if !size_ok(width, height) || !w.is_multiple_of(2) || !plane_fits(data.len(), w * 2, height as usize, stride as usize, 1) {
            return false;
        }
        self.push_input(width, height, rotation, Source::Yuyv { data, stride: stride as usize })
    }

    /// The newest decoded frame of the peer's video not pulled yet, else `None`.
    pub fn take_frame(&self) -> Option<RawFrame> {
        self.pipe.output.lock().take()
    }

    /// `take_frame` as packed RGBA into the caller's buffer (no allocation for the pixels).
    pub fn pull_rgba_into(&self, dst: &mut [u8]) -> Pulled {
        let f = {
            let mut slot = self.pipe.output.lock();
            match slot.as_ref() {
                None => return Pulled::None,
                Some(f) if dst.len() < f.width as usize * f.height as usize * 4 => {
                    return Pulled::TooSmall { width: f.width, height: f.height };
                }
                Some(_) => slot.take().expect("checked"),
            }
        };
        let (width, height, seq) = (f.width, f.height, f.seq);
        let pic = I420 { width: width as usize, height: height as usize, data: f.data };
        match convert::i420_to_rgba(&pic, dst) {
            Ok(()) => Pulled::Frame { width, height, seq },
            Err(_) => Pulled::None,
        }
    }
}

impl Drop for CallVideo {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests;
