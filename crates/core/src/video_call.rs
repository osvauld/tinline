//! Video in 1:1 calls (design: `docs/video.md`, "Call protocol").
//!
//! Negotiated in `CallHello`/`Accept` (`proto::VideoOffer` on both = video); once the call is
//! active both sides run `video::net` on the call's connection, each a `VideoSend` and a
//! `VideoRecv`, for the whole call. The camera only decides whether frames are pushed:
//! `Msg::Video { camera }` tells the peer, so its UI can show the picture or the avatar.
//!
//! Platform side: `push_video_frame` (I420 from the camera, any size and rotation) feeds a
//! latest-wins slot; the encoder thread (pinned to the fast cores, 2 libaom threads) rotates,
//! scales to the sender's ladder size, encodes and sends. The decoder thread (pinned, rav1d)
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
    pub data: Vec<u8>,
    /// Counts decoded frames; a gap means frames were replaced before being pulled.
    pub seq: u64,
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

struct Input {
    pic: I420,
    rotation: u32,
    at: Instant,
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
        let pic = rotate(input.pic, input.rotation);
        let (rw, rh) = send.with_sender(|s| s.resolution());
        let (w, h) = fit(pic.width, pic.height, rw as usize, rh as usize);
        let pic = video::y4m::scale(&pic, w, h);
        let bitrate = send.bitrate();
        let mut key = send.wants_keyframe() | pipe.resume.swap(false, Ordering::SeqCst);
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
        send.with_sender(|s| s.encode_time(epoch.elapsed().as_micros() as u64, took.as_micros() as u64));
        let mut st = pipe.stats.lock();
        for p in packets {
            if send.send(capture_us, p.key, &p.data).is_err() {
                return;
            }
            st.frames_sent += 1;
            st.keyframes_sent += p.key as u64;
        }
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
                    let frame = VideoFrame { width: p.width as u32, height: p.height as u32, data: p.data, seq };
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

/// Rotates clockwise by `deg` (0, 90, 180, 270; anything else is taken as 0).
pub(crate) fn rotate(pic: I420, deg: u32) -> I420 {
    if !matches!(deg, 90 | 180 | 270) {
        return pic;
    }
    let (w, h) = (pic.width, pic.height);
    let (nw, nh) = if deg == 180 { (w, h) } else { (h, w) };
    let mut out = I420::new(nw, nh);
    let (cw, ch) = I420::chroma(w, h);
    let (sy, su, sv) = pic.planes();
    let (dy, du, dv) = out.planes_mut();
    rotate_plane(sy, w, h, dy, deg);
    rotate_plane(su, cw, ch, du, deg);
    rotate_plane(sv, cw, ch, dv, deg);
    out
}

fn rotate_plane(src: &[u8], w: usize, h: usize, dst: &mut [u8], deg: u32) {
    match deg {
        180 => {
            for (d, s) in dst.iter_mut().zip(src.iter().rev()) {
                *d = *s;
            }
        }
        90 => {
            // dst is h wide: row y is source column y, read bottom-up.
            for y in 0..w {
                for x in 0..h {
                    dst[y * h + x] = src[(h - 1 - x) * w + y];
                }
            }
        }
        _ => {
            for y in 0..w {
                for x in 0..h {
                    dst[y * h + x] = src[x * w + (w - 1 - y)];
                }
            }
        }
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
    pub fn push_video_frame(&self, width: u32, height: u32, rotation: u32, data: Vec<u8>) -> bool {
        if width == 0 || height == 0 || width > MAX_INPUT_SIDE || height > MAX_INPUT_SIDE {
            return false;
        }
        let (w, h) = (width as usize, height as usize);
        if data.len() != I420::len(w, h) {
            return false;
        }
        let Some(call) = self.inner.current_video_call() else { return false };
        let v = &call.video;
        if !v.negotiated() || !v.camera() || v.pipe.stop.load(Ordering::SeqCst) {
            return false;
        }
        *v.pipe.input.lock() = Some(Input { pic: I420 { width: w, height: h, data }, rotation, at: Instant::now() });
        v.pipe.wake.notify_one();
        true
    }

    /// The newest decoded frame of the peer's video not pulled yet, else `None`. Poll it on the
    /// display's clock. Nothing while the peer's camera is off.
    pub fn pull_video_frame(&self) -> Option<VideoFrame> {
        let call = self.inner.current_video_call()?;
        let f = call.video.pipe.output.lock().take();
        f
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

    #[test]
    fn rotations_compose() {
        let mut pic = I420::new(6, 4);
        for (i, b) in pic.data.iter_mut().enumerate() {
            *b = i as u8;
        }
        let r90 = rotate(pic.clone(), 90);
        assert_eq!((r90.width, r90.height), (4, 6));
        // Top-left after a clockwise turn is the old bottom-left.
        assert_eq!(r90.data[0], pic.data[3 * 6]);
        assert_eq!(rotate(rotate(pic.clone(), 90), 270), pic);
        assert_eq!(rotate(rotate(pic.clone(), 180), 180), pic);
        assert_eq!(rotate(r90, 90), rotate(pic.clone(), 180));
        assert_eq!(rotate(pic.clone(), 45), pic);
    }

    #[test]
    fn peer_cap_limits_the_ladder() {
        let l = LadderConfig::default();
        assert_eq!(max_rung(&l, 1280 * 720), 3);
        assert_eq!(max_rung(&l, 640 * 360), 1);
        assert_eq!(max_rung(&l, 1), 0);
    }
}
