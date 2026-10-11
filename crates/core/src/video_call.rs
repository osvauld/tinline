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
use std::time::Instant;

use iroh::endpoint::Connection;
use parking_lot::{Condvar, Mutex};
use proto::VideoOffer;
use tokio::sync::{Notify, mpsc, watch};
use tokio::task::JoinHandle;
use video::aom::AomEncoder;
use video::convert::{self, Prepare, Range, RgbLayout, Rotation, Yuv420};
use video::media::{Frame, LadderConfig, SenderConfig};
use video::net::{RecvConfig, VideoRecv, VideoSend};
use video::{Decoder, EncoderConfig, I420, VideoEncoder};

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

/// Bytes across UniFFI in one memcpy each way. Same wire form and foreign type as `Vec<u8>`
/// (Kotlin `ByteArray`), but UniFFI lifts/lowers a `Vec<u8>` element by element, which costs
/// milliseconds per video frame on a phone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

impl Bytes {
    pub fn into_inner(self) -> Vec<u8> {
        self.0
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(v: Vec<u8>) -> Self {
        Self(v)
    }
}

impl std::ops::Deref for Bytes {
    type Target = Vec<u8>;
    fn deref(&self) -> &Vec<u8> {
        &self.0
    }
}

impl std::ops::DerefMut for Bytes {
    fn deref_mut(&mut self) -> &mut Vec<u8> {
        &mut self.0
    }
}

// SAFETY: the same serialized form (`i32` BE length, then the bytes) and metadata as UniFFI's
// own `Vec<u8>`, so the foreign side treats it exactly like one.
unsafe impl uniffi::FfiConverter<crate::UniFfiTag> for Bytes {
    type FfiType = uniffi::RustBuffer;

    fn lower(obj: Self) -> uniffi::RustBuffer {
        let mut buf = Vec::with_capacity(4 + obj.0.len());
        Self::write(obj, &mut buf);
        uniffi::RustBuffer::from_vec(buf)
    }

    fn try_lift(v: uniffi::RustBuffer) -> uniffi::Result<Self> {
        let mut v = v.destroy_into_vec();
        let mut r = &v[..];
        let n = read_len(&mut r)?;
        if r.len() != n {
            uniffi::deps::anyhow::bail!("bytes: {} trailing", r.len() as i64 - n as i64);
        }
        v.drain(..4);
        Ok(Self(v))
    }

    fn write(obj: Self, buf: &mut Vec<u8>) {
        let n = i32::try_from(obj.0.len()).expect("bytes over 2 GiB");
        buf.extend_from_slice(&n.to_be_bytes());
        buf.extend_from_slice(&obj.0);
    }

    fn try_read(buf: &mut &[u8]) -> uniffi::Result<Self> {
        let n = read_len(buf)?;
        uniffi::check_remaining(buf, n)?;
        let (b, rest) = buf.split_at(n);
        *buf = rest;
        Ok(Self(b.to_vec()))
    }

    const TYPE_ID_META: uniffi::MetadataBuffer = <Vec<u8> as uniffi::TypeId<crate::UniFfiTag>>::TYPE_ID_META;
}

uniffi::derive_ffi_traits!(local Bytes);

fn read_len(buf: &mut &[u8]) -> uniffi::Result<usize> {
    uniffi::check_remaining(buf, 4)?;
    let (n, rest) = buf.split_at(4);
    *buf = rest;
    Ok(usize::try_from(i32::from_be_bytes(n.try_into().expect("4 bytes")))?)
}

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
}

/// Video state changes of the current call (`negotiated` is known when the call goes active;
/// camera on/off on either side). Frames are pulled, not pushed: see `pull_video_frame`.
#[uniffi::export(with_foreign)]
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

/// A camera frame as pushed; converted on the encoder thread.
enum Source {
    I420(I420),
    /// Android `YUV_420_888` planes (`u`/`v` with pixel stride 2 are the NV12/NV21 views).
    Yuv {
        y: Vec<u8>,
        y_stride: usize,
        u: Vec<u8>,
        v: Vec<u8>,
        uv_stride: usize,
        uv_pixel_stride: usize,
        full_range: bool,
    },
    Rgba(Vec<u8>),
    Yuyv { data: Vec<u8>, stride: usize },
}

struct Input {
    width: usize,
    height: usize,
    src: Source,
    rotation: u32,
    at: Instant,
}

impl Input {
    /// Upright and fit into the rung `rw` x `rh` (see [`fit`]); `prepare`'s center crop then
    /// only trims what even rounding loses.
    fn prepare(&self, rw: usize, rh: usize) -> video::Result<I420> {
        let rotate = Rotation::from_degrees(self.rotation as i32);
        let (sw, sh) = if matches!(rotate, Rotation::R90 | Rotation::R270) {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        let (width, height) = fit(sw, sh, rw, rh);
        let mut p = Prepare { rotate, mirror: false, width, height, range: Range::Limited };
        let (w, h) = (self.width, self.height);
        let packed;
        let yuv = match &self.src {
            Source::I420(pic) => Yuv420::from_i420(pic),
            Source::Yuv { y, y_stride, u, v, uv_stride, uv_pixel_stride, full_range } => {
                if *full_range {
                    p.range = Range::Full;
                }
                let (u, v) = chroma_views(u, v);
                Yuv420 {
                    y,
                    y_stride: *y_stride,
                    u,
                    v,
                    uv_stride: *uv_stride,
                    uv_pixel_stride: *uv_pixel_stride,
                    width: w,
                    height: h,
                }
            }
            Source::Rgba(data) => {
                packed = convert::rgb_to_i420(data, w * 4, w, h, RgbLayout::Rgba)?;
                Yuv420::from_i420(&packed)
            }
            Source::Yuyv { data, stride } => {
                packed = convert::yuyv_to_i420(data, *stride, w, h)?;
                Yuv420::from_i420(&packed)
            }
        };
        convert::prepare(&yuv, None, &p)
    }
}

/// U and V; an empty one is taken from the other's interleaved plane (NV21: only `v`, NV12:
/// only `u`, each a whole interleaved chroma plane).
fn chroma_views<'a>(u: &'a [u8], v: &'a [u8]) -> (&'a [u8], &'a [u8]) {
    match (u.is_empty(), v.is_empty()) {
        (true, false) => (&v[1..], v),
        (false, true) => (u, &u[1..]),
        _ => (u, v),
    }
}

/// Whether `len` bytes hold a plane of `w` x `h` samples, rows `stride` apart, samples `step`
/// apart (the last row may end right after its last sample).
fn plane_fits(len: usize, w: usize, h: usize, stride: usize, step: usize) -> bool {
    let row = (w - 1) * step + 1;
    step >= 1 && stride >= row && len >= (h - 1) * stride + row
}

fn size_ok(width: u32, height: u32) -> bool {
    (1..=MAX_INPUT_SIDE).contains(&width) && (1..=MAX_INPUT_SIDE).contains(&height)
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
}

/// One call's video.
pub(crate) struct CallVideo {
    /// We offered video in this call (hello or accept) and the peer did too.
    negotiated: AtomicBool,
    /// What the peer offered in its hello (callee side), until we answer.
    pub(crate) peer_offer: Mutex<Option<VideoOffer>>,
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
        self.video.enabled.load(Ordering::SeqCst).then_some(VideoOffer { max_pixels: MAX_PIXELS })
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
        if let (Some(_), Some(peer)) = (ours, theirs) {
            let v = &call.video;
            v.peer_max_pixels.store(peer.max_pixels, Ordering::SeqCst);
            v.negotiated.store(true, Ordering::SeqCst);
            let pipe = v.pipe.clone();
            let send = self.handle.spawn(run_send(conn.clone(), pipe.clone(), peer.max_pixels));
            let recv = self.handle.spawn(run_recv(conn.clone(), pipe));
            v.tasks.lock().extend([send, recv]);
            if v.pipe.stop.load(Ordering::SeqCst) {
                // Ended meanwhile.
                v.stop();
            }
        }
        if ours.is_some() {
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

/// Ladder rung cap for a peer that takes at most `max_pixels`.
fn max_rung(ladder: &LadderConfig, max_pixels: u32) -> usize {
    ladder
        .rungs
        .iter()
        .rposition(|r| r.width as u64 * r.height as u64 <= max_pixels as u64)
        .unwrap_or(0)
        .min(ladder.max_rung)
}

async fn run_send(conn: Connection, pipe: Arc<Pipe>, peer_max_pixels: u32) {
    let mut cfg = SenderConfig::default();
    cfg.ladder.max_rung = max_rung(&cfg.ladder, peer_max_pixels);
    let send = match VideoSend::accept(conn, cfg).await {
        Ok(s) => s,
        Err(e) => {
            tracing::info!("video send: {e}");
            return;
        }
    };
    let spawned = std::thread::Builder::new()
        .name("tl-video-enc".into())
        .spawn(move || encode_loop(pipe, send));
    if let Err(e) = spawned {
        tracing::warn!("video encoder thread: {e}");
    }
}

fn encode_loop(pipe: Arc<Pipe>, send: VideoSend) {
    video::cpu::pin_to_fast_cores();
    let epoch = Instant::now();
    let mut enc: Option<(AomEncoder, usize, usize)> = None;
    let mut rate = 0;
    loop {
        let input = {
            let mut slot = pipe.input.lock();
            loop {
                if pipe.stop.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(i) = slot.take() {
                    break i;
                }
                pipe.wake.wait(&mut slot);
            }
        };
        if send.is_closed() {
            tracing::info!("video send: the peer's receiver went away");
            return;
        }
        let capture_us = input.at.saturating_duration_since(epoch).as_micros() as u64;
        // One look at the sender: a ladder step in between would otherwise give a keyframe at
        // the old size and another at the new one.
        let ((rw, rh), bitrate, wants_key) = send.with_sender(|s| (s.resolution(), s.bitrate(), s.wants_keyframe()));
        let pic = match input.prepare(rw as usize, rh as usize) {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("video frame: {e}");
                continue;
            }
        };
        let (w, h) = (pic.width, pic.height);
        let mut key = wants_key | pipe.resume.swap(false, Ordering::SeqCst);
        if enc.as_ref().is_none_or(|e| (e.1, e.2) != (w, h)) {
            let cfg = EncoderConfig {
                width: w,
                height: h,
                fps: FPS,
                bitrate_bps: bitrate,
                threads: ENCODER_THREADS,
                ..Default::default()
            };
            match AomEncoder::new(&cfg) {
                Ok(e) => enc = Some((e, w, h)),
                Err(e) => {
                    tracing::warn!("video encoder: {e}");
                    continue;
                }
            }
            rate = bitrate;
            key = true;
        } else if bitrate != rate {
            let _ = enc.as_mut().expect("encoder").0.set_bitrate(bitrate);
            rate = bitrate;
        }
        let t = Instant::now();
        let packets = match enc.as_mut().expect("encoder").0.encode(&pic, key) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("video encode: {e}");
                enc = None;
                continue;
            }
        };
        let took = t.elapsed();
        let mut st = pipe.stats.lock();
        for p in packets {
            if send.send(capture_us, p.key, &p.data).is_err() {
                return;
            }
            st.frames_sent += 1;
            st.keyframes_sent += p.key as u64;
        }
        send.encode_time(took.as_micros() as u64);
        st.send_width = w as u32;
        st.send_height = h as u32;
        st.bitrate_bps = rate;
        st.encode_ms = took.as_secs_f32() * 1000.0;
    }
}

async fn run_recv(conn: Connection, pipe: Arc<Pipe>) {
    let mut recv = match VideoRecv::new(conn, RecvConfig::default()).await {
        Ok(r) => r,
        Err(e) => {
            tracing::info!("video recv: {e}");
            return;
        }
    };
    // Small: a decoder that falls behind stops the reading, which the sender sees as backlog.
    let (tx, rx) = mpsc::channel::<Frame>(2);
    let dec_pipe = pipe.clone();
    let spawned = std::thread::Builder::new()
        .name("tl-video-dec".into())
        .spawn(move || decode_loop(dec_pipe, rx));
    if let Err(e) = spawned {
        tracing::warn!("video decoder thread: {e}");
        return;
    }
    loop {
        tokio::select! {
            f = recv.recv() => match f {
                Ok(f) => {
                    if tx.send(f).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    tracing::info!("video recv: {e}");
                    return;
                }
            },
            _ = pipe.key_wanted.notified() => recv.request_keyframe(),
        }
    }
}

fn decode_loop(pipe: Arc<Pipe>, mut rx: mpsc::Receiver<Frame>) {
    video::cpu::pin_to_fast_cores();
    let mut dec = match Decoder::new(DECODER_THREADS, MAX_PIXELS) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("video decoder: {e}");
            return;
        }
    };
    // After an error only a keyframe (a new group) can be decoded.
    let mut broken = false;
    while let Some(f) = rx.blocking_recv() {
        if broken && f.index != 0 {
            continue;
        }
        match dec.decode(&f.payload) {
            Ok(pics) => {
                broken = false;
                for p in pics {
                    let seq = pipe.seq.fetch_add(1, Ordering::SeqCst) + 1;
                    {
                        let mut st = pipe.stats.lock();
                        st.frames_decoded += 1;
                        st.recv_width = p.width as u32;
                        st.recv_height = p.height as u32;
                    }
                    let frame = VideoFrame { width: p.width as u32, height: p.height as u32, data: p.data.into(), seq };
                    *pipe.output.lock() = Some(frame);
                }
            }
            Err(e) => {
                tracing::debug!("video decode: {e}");
                broken = true;
                pipe.stats.lock().decode_errors += 1;
                pipe.key_wanted.notify_one();
            }
        }
    }
}

/// An I420 `VideoFrame` as RGBA.
fn to_rgba(f: VideoFrame) -> VideoFrame {
    let pic = I420 { width: f.width as usize, height: f.height as usize, data: f.data.0 };
    VideoFrame { data: convert::to_rgba(&pic).into(), ..f }
}

/// Largest even size of `sw` x `sh`'s aspect within the rung `rw` x `rh` (turned to match the
/// picture's orientation), never upscaled.
pub(crate) fn fit(sw: usize, sh: usize, rw: usize, rh: usize) -> (usize, usize) {
    let (rw, rh) = if sh > sw { (rh, rw) } else { (rw, rh) };
    let (w, h) = if sw * rh <= sh * rw {
        let h = sh.min(rh);
        (sw * h / sh, h)
    } else {
        let w = sw.min(rw);
        (w, sh * w / sw)
    };
    ((w & !1).max(2), (h & !1).max(2))
}

impl Node {
    /// One camera frame: tightly packed I420 of `width` x `height`, to be turned `rotation`
    /// degrees clockwise (0/90/180/270) to be upright. Any size up to 4096 a side; it is
    /// scaled to what the link carries. Latest wins: a frame not yet taken by the encoder is
    /// replaced. False if not sent (no video running, camera off, or a malformed frame).
    pub fn push_video_frame(&self, width: u32, height: u32, rotation: u32, data: Vec<u8>) -> bool {
        if !size_ok(width, height) || data.len() != I420::len(width as usize, height as usize) {
            return false;
        }
        let pic = I420 { width: width as usize, height: height as usize, data };
        self.inner.push_input(width, height, rotation, Source::I420(pic))
    }
}

#[uniffi::export]
impl Node {
    pub fn set_video_events(&self, events: Arc<dyn VideoEvents>) {
        *self.inner.video.events.lock() = Some(events);
    }

    /// Whether calls offer (outgoing) and take (incoming) video. Default on. Off = every call
    /// is audio only; the peer sees `negotiated == false`.
    pub fn set_video_enabled(&self, enabled: bool) {
        self.inner.video.enabled.store(enabled, Ordering::SeqCst);
    }

    pub fn video_enabled(&self) -> bool {
        self.inner.video.enabled.load(Ordering::SeqCst)
    }

    /// Turns our camera on or off in the current call (also before it is answered: it applies
    /// once video runs). Push frames only while it is on; frames pushed while off are dropped.
    /// `NotFound` without a call; `Protocol` once active without video.
    pub fn set_camera(&self, on: bool) -> Result<(), Error> {
        let call = self.inner.current_video_call().ok_or(Error::NotFound)?;
        if call.state() == CallState::Active && !call.video.negotiated() {
            return Err(Error::Protocol("no video in this call".into()));
        }
        let changed = call.video.camera.send_if_modified(|c| std::mem::replace(c, on) != on);
        if changed {
            if on {
                call.video.pipe.resume.store(true, Ordering::SeqCst);
            } else {
                call.video.pipe.input.lock().take();
            }
            self.inner.emit_video(&call);
        }
        Ok(())
    }

    /// One camera frame: tightly packed I420 of `width` x `height`, to be turned `rotation`
    /// degrees clockwise (0/90/180/270) to be upright. Any size up to 4096 a side; it is
    /// scaled to what the link carries. Latest wins: a frame not yet taken by the encoder is
    /// replaced. False if not sent (no video running, camera off, or a malformed frame).
    /// (Exported as `push_video_frame`; Rust callers use the `Vec<u8>` one.)
    #[uniffi::method(name = "push_video_frame")]
    pub fn push_video_frame_ffi(&self, width: u32, height: u32, rotation: u32, data: Bytes) -> bool {
        self.push_video_frame(width, height, rotation, data.0)
    }

    /// One CameraX `YUV_420_888` frame as its planes (`ImageProxy.planes[i].buffer`,
    /// `rowStride`, `pixelStride`; U and V share theirs), copied as they are: the conversion
    /// (crop, scale, range, rotate) runs on the encoder thread. With pixel stride 2 the U and
    /// V buffers are two views of one interleaved plane; pass that plane once and the other
    /// empty (NV21, V first: `v` = the V buffer, `u` empty; NV12: the reverse) to copy a third
    /// less. `full_range`: the samples are full range (JFIF), as Android cameras normally
    /// deliver. Otherwise like `push_video_frame`.
    #[allow(clippy::too_many_arguments)]
    pub fn push_video_frame_yuv(
        &self,
        width: u32,
        height: u32,
        rotation: u32,
        y: Bytes,
        y_row_stride: u32,
        u: Bytes,
        v: Bytes,
        uv_row_stride: u32,
        uv_pixel_stride: u32,
        full_range: bool,
    ) -> bool {
        if !size_ok(width, height) {
            return false;
        }
        let (w, h) = (width as usize, height as usize);
        let (cw, ch) = I420::chroma(w, h);
        let (ys, uvs, ps) = (y_row_stride as usize, uv_row_stride as usize, uv_pixel_stride as usize);
        let (cu, cv) = chroma_views(&u, &v);
        if !plane_fits(y.len(), w, h, ys, 1) || !plane_fits(cu.len(), cw, ch, uvs, ps) || !plane_fits(cv.len(), cw, ch, uvs, ps) {
            return false;
        }
        let src = Source::Yuv { y: y.0, y_stride: ys, u: u.0, v: v.0, uv_stride: uvs, uv_pixel_stride: ps, full_range };
        self.inner.push_input(width, height, rotation, src)
    }

    /// One frame of packed RGBA (`width * height * 4` bytes), converted on the encoder thread.
    /// Otherwise like `push_video_frame`.
    pub fn push_video_frame_rgba(&self, width: u32, height: u32, rotation: u32, data: Vec<u8>) -> bool {
        if !size_ok(width, height) || data.len() != width as usize * height as usize * 4 {
            return false;
        }
        self.inner.push_input(width, height, rotation, Source::Rgba(data))
    }

    /// One frame of packed YUYV (YUY2), rows `stride` bytes apart (at least `width * 2`; even
    /// width), converted on the encoder thread. Otherwise like `push_video_frame`.
    pub fn push_video_frame_yuyv(&self, width: u32, height: u32, rotation: u32, stride: u32, data: Vec<u8>) -> bool {
        let w = width as usize;
        if !size_ok(width, height) || !w.is_multiple_of(2) || !plane_fits(data.len(), w * 2, height as usize, stride as usize, 1) {
            return false;
        }
        self.inner.push_input(width, height, rotation, Source::Yuyv { data, stride: stride as usize })
    }

    /// The newest decoded frame of the peer's video not pulled yet, else `None`. Poll it on the
    /// display's clock. Nothing while the peer's camera is off.
    pub fn pull_video_frame(&self) -> Option<VideoFrame> {
        let call = self.inner.current_video_call()?;
        call.video.pipe.output.lock().take()
    }

    /// Like `pull_video_frame`, but `data` is packed RGBA (`width * height * 4` bytes; Android
    /// `ARGB_8888` bitmaps and iced images use this byte order), converted on the caller's
    /// thread.
    pub fn pull_video_frame_rgba(&self) -> Option<VideoFrame> {
        self.pull_video_frame().map(to_rgba)
    }

    pub fn video_info(&self) -> Option<VideoInfo> {
        let call = self.inner.current_video_call()?;
        Some(self.inner.video_info(&call))
    }

    pub fn video_stats(&self) -> Option<VideoStats> {
        let call = self.inner.current_video_call()?;
        let s = call.video.pipe.stats.lock().clone();
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_aspect_and_orientation() {
        assert_eq!(fit(1280, 720, 640, 360), (640, 360));
        assert_eq!(fit(640, 480, 640, 360), (480, 360));
        assert_eq!(fit(720, 1280, 640, 360), (360, 640));
        assert_eq!(fit(320, 240, 1280, 720), (320, 240));
        assert_eq!(fit(1279, 721, 1280, 720), (1276, 720));
    }

    fn input(width: usize, height: usize, rotation: u32, src: Source) -> Input {
        Input { width, height, src, rotation, at: Instant::now() }
    }

    fn numbered(w: usize, h: usize) -> I420 {
        let mut pic = I420::new(w, h);
        for (i, b) in pic.data.iter_mut().enumerate() {
            *b = 16 + (i % 200) as u8;
        }
        pic
    }

    #[test]
    fn rotations_compose() {
        let pic = numbered(6, 4);
        let turn = |p: &I420, deg| input(p.width, p.height, deg, Source::I420(p.clone())).prepare(1280, 720).unwrap();
        let r90 = turn(&pic, 90);
        assert_eq!((r90.width, r90.height), (4, 6));
        // Top-left after a clockwise turn is the old bottom-left.
        assert_eq!(r90.data[0], pic.data[3 * 6]);
        assert_eq!(turn(&r90, 270), pic);
        assert_eq!(turn(&turn(&pic, 180), 180), pic);
        assert_eq!(turn(&r90, 90), turn(&pic, 180));
        assert_eq!(turn(&pic, 45), pic);
    }

    #[test]
    fn frames_fit_the_rung_upright() {
        let pic = numbered(1280, 720);
        let p = input(1280, 720, 0, Source::I420(pic.clone())).prepare(640, 360).unwrap();
        assert_eq!((p.width, p.height), (640, 360));
        let p = input(1280, 720, 90, Source::I420(pic)).prepare(640, 360).unwrap();
        assert_eq!((p.width, p.height), (360, 640));
        let p = input(640, 480, 270, Source::I420(numbered(640, 480))).prepare(640, 360).unwrap();
        assert_eq!((p.width, p.height), (360, 480));
    }

    /// A camera frame as Android's `YUV_420_888` with pixel stride 2 (NV21 memory: V then U
    /// interleaved, `u` = `v[1..]`), padded rows, last row unpadded; full range.
    fn android_planes(pic: &I420) -> (Vec<u8>, usize, Vec<u8>, Vec<u8>, usize) {
        let (w, h) = (pic.width, pic.height);
        let (cw, ch) = I420::chroma(w, h);
        let (sy, su, sv) = pic.planes();
        let ys = w + 16;
        let mut y = vec![0u8; (h - 1) * ys + w];
        for r in 0..h {
            y[r * ys..r * ys + w].copy_from_slice(&sy[r * w..(r + 1) * w]);
        }
        let uvs = 2 * cw + 8;
        let mut vu = vec![0u8; (ch - 1) * uvs + 2 * cw];
        for r in 0..ch {
            for c in 0..cw {
                vu[r * uvs + 2 * c] = sv[r * cw + c];
                vu[r * uvs + 2 * c + 1] = su[r * cw + c];
            }
        }
        let u = vu[1..].to_vec();
        vu.pop();
        (y, ys, u, vu, uvs)
    }

    #[test]
    fn android_yuv_arrives_upright() {
        // Landscape sensor frame, phone held upright: rotation 90.
        let pic = numbered(640, 480);
        let (y, ys, u, v, uvs) = android_planes(&pic);
        assert!(plane_fits(y.len(), 640, 480, ys, 1) && plane_fits(u.len(), 320, 240, uvs, 2) && plane_fits(v.len(), 320, 240, uvs, 2));
        let src = Source::Yuv { y, y_stride: ys, u, v, uv_stride: uvs, uv_pixel_stride: 2, full_range: false };
        let got = input(640, 480, 90, src).prepare(1280, 720).unwrap();
        assert_eq!((got.width, got.height), (480, 640));
        let want = convert::transform(&pic, Rotation::R90, false);
        assert_eq!(got, want);
        // Scaled into a rung, still portrait and 3:4.
        let (y, ys, u, v, uvs) = android_planes(&pic);
        let src = Source::Yuv { y, y_stride: ys, u, v, uv_stride: uvs, uv_pixel_stride: 2, full_range: true };
        let got = input(640, 480, 90, src).prepare(640, 360).unwrap();
        assert_eq!((got.width, got.height), (360, 480));
        assert!(got.data.iter().all(|&b| (16..=240).contains(&b)), "limited range");
    }

    #[test]
    fn nv21_as_one_plane() {
        let pic = numbered(640, 480);
        let (y, ys, u, v, uvs) = android_planes(&pic);
        let vu: Vec<u8> = [v[0]].into_iter().chain(u.iter().copied()).collect();
        let (cu, cv) = chroma_views(&[], &vu);
        assert!(plane_fits(cu.len(), 320, 240, uvs, 2) && plane_fits(cv.len(), 320, 240, uvs, 2));
        let src = Source::Yuv { y, y_stride: ys, u: vec![], v: vu, uv_stride: uvs, uv_pixel_stride: 2, full_range: false };
        let got = input(640, 480, 90, src).prepare(1280, 720).unwrap();
        assert_eq!(got, convert::transform(&pic, Rotation::R90, false));
    }

    /// `Bytes` crosses as `Vec<u8>` does, much faster. `--nocapture` prints the cost of one
    /// 640x480 I420 frame each way (the Rust half of the FFI copy).
    #[test]
    fn bytes_cross_like_vec() {
        use uniffi::{Lift, Lower, RustBuffer};
        type T = crate::UniFfiTag;
        let data: Vec<u8> = (0..640 * 480 * 3 / 2).map(|i| i as u8).collect();
        let time = |f: &mut dyn FnMut()| {
            let t = Instant::now();
            for _ in 0..20 {
                f();
            }
            t.elapsed().as_secs_f64() * 1000.0 / 20.0
        };
        let wire = <Vec<u8> as Lower<T>>::lower_into_rust_buffer(data.clone()).destroy_into_vec();
        assert_eq!(<Bytes as Lower<T>>::lower_into_rust_buffer(Bytes(data.clone())).destroy_into_vec(), wire);
        assert_eq!(<Bytes as Lift<T>>::try_lift(RustBuffer::from_vec(wire.clone())).unwrap().0, data);
        assert!(<Bytes as Lift<T>>::try_lift(RustBuffer::from_vec(wire[..wire.len() - 1].to_vec())).is_err());
        let frame = VideoFrame { width: 640, height: 480, data: Bytes(data.clone()), seq: 1 };
        let mut r = &<VideoFrame as Lower<T>>::lower_into_rust_buffer(frame).destroy_into_vec()[..];
        assert_eq!(<VideoFrame as Lift<T>>::try_read(&mut r).unwrap().data.0, data);
        let lift_vec = time(&mut || drop(<Vec<u8> as Lift<T>>::try_lift(RustBuffer::from_vec(wire.clone())).unwrap()));
        let lift_bytes = time(&mut || drop(<Bytes as Lift<T>>::try_lift(RustBuffer::from_vec(wire.clone())).unwrap()));
        let lower_vec = time(&mut || <Vec<u8> as Lower<T>>::lower(data.clone()).destroy());
        let lower_bytes = time(&mut || <Bytes as Lower<T>>::lower(Bytes(data.clone())).destroy());
        println!("640x480 I420 ms (incl. a clone): lift Vec<u8> {lift_vec:.3} Bytes {lift_bytes:.3}, lower Vec<u8> {lower_vec:.3} Bytes {lower_bytes:.3}");
    }

    #[test]
    fn short_planes_are_refused() {
        assert!(plane_fits(10, 4, 2, 6, 1));
        assert!(!plane_fits(9, 4, 2, 6, 1));
        assert!(!plane_fits(100, 4, 2, 3, 1));
        assert!(plane_fits(7, 2, 2, 4, 2) && !plane_fits(6, 2, 2, 4, 2));
    }

    #[test]
    fn rgba_and_yuyv_inputs() {
        let rgba: Vec<u8> = (0..64 * 48).flat_map(|_| [200, 40, 40, 255]).collect();
        let got = input(64, 48, 180, Source::Rgba(rgba)).prepare(1280, 720).unwrap();
        assert_eq!((got.width, got.height), (64, 48));
        let yuyv: Vec<u8> = (0..64 * 48 / 2).flat_map(|_| [100, 90, 110, 160]).collect();
        let got = input(64, 48, 90, Source::Yuyv { data: yuyv, stride: 128 }).prepare(1280, 720).unwrap();
        assert_eq!((got.width, got.height), (48, 64));
        assert_eq!(got.data[0], 100);
    }

    #[test]
    fn rgba_pull_converts() {
        let mut pic = I420::new(4, 2);
        pic.data.fill(128);
        pic.data[..8].fill(235);
        let f = to_rgba(VideoFrame { width: 4, height: 2, data: pic.data.into(), seq: 7 });
        assert_eq!((f.width, f.height, f.seq, f.data.len()), (4, 2, 7, 32));
        assert!(f.data.chunks(4).all(|p| p == [255, 255, 255, 255]));
    }

    #[test]
    fn peer_cap_limits_the_ladder() {
        let l = LadderConfig::default();
        assert_eq!(max_rung(&l, 1280 * 720), 3);
        assert_eq!(max_rung(&l, 640 * 360), 1);
        assert_eq!(max_rung(&l, 1), 0);
    }
}
