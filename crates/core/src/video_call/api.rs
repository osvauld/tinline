//! The `Node` video API (UniFFI-exported) and the frame-port helpers behind it.

use super::*;
use crate::node::CallState;
use crate::{Error, Node};

impl Node {
    /// One camera frame: tightly packed I420 of `width` x `height`, to be turned `rotation`
    /// degrees clockwise (0/90/180/270) to be upright. Any size up to 4096 a side; it is
    /// scaled to what the link carries. Latest wins: a frame not yet taken by the encoder is
    /// replaced. False if not sent (no video running, camera off, or a malformed frame).
    pub fn push_video_frame(&self, width: u32, height: u32, rotation: u32, data: Vec<u8>) -> bool {
        self.open().current_video_call().is_some_and(|c| c.video.push_i420(width, height, rotation, data))
    }

    /// `push_video_frame_yuv` from borrowed planes (each copied once, no FFI buffers): what
    /// Android's JNI entry point (`video_jni`) calls with the camera's direct buffers.
    #[allow(clippy::too_many_arguments)]
    pub fn push_video_planes(
        &self,
        width: u32,
        height: u32,
        rotation: u32,
        y: &[u8],
        y_row_stride: usize,
        u: &[u8],
        v: &[u8],
        uv_row_stride: usize,
        uv_pixel_stride: usize,
        full_range: bool,
    ) -> bool {
        self.open().push_yuv_planes(width, height, rotation, y, y_row_stride, u, v, uv_row_stride, uv_pixel_stride, full_range)
    }

    /// `pull_video_frame_rgba` into the caller's buffer (no allocation for the pixels).
    pub fn pull_video_frame_rgba_into(&self, dst: &mut [u8]) -> Pulled {
        self.open().pull_rgba_into(dst)
    }
}

#[uniffi::export]
impl Node {
    pub fn set_video_events(&self, events: Arc<dyn VideoEvents>) {
        self.hub.set_sinks(|s| s.video = Some(Arc::new(VideoEventsAdapter(events))));
    }

    /// Whether calls offer (outgoing) and take (incoming) video. Default on. Off = every call
    /// is audio only; the peer sees `negotiated == false`.
    pub fn set_video_enabled(&self, enabled: bool) {
        self.open().video.set_enabled(enabled);
    }

    pub fn video_enabled(&self) -> bool {
        self.open().video.enabled()
    }

    /// Turns our camera on or off in the current call (also before it is answered: it applies
    /// once video runs). Push frames only while it is on; frames pushed while off are dropped.
    /// `NotFound` without a call; `Protocol` once active without video.
    pub fn set_camera(&self, on: bool) -> Result<(), Error> {
        let slot = self.open();
        let call = slot.current_video_call().ok_or(Error::NotFound)?;
        if call.video.set_camera(on, call.state() == CallState::Active)? {
            slot.emit_video(&call);
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
        let (ys, uvs, ps) = (y_row_stride as usize, uv_row_stride as usize, uv_pixel_stride as usize);
        self.open().current_video_call().is_some_and(|c| c.video.push_yuv(width, height, rotation, y.0, ys, u.0, v.0, uvs, ps, full_range))
    }

    /// One frame of packed RGBA (`width * height * 4` bytes), converted on the encoder thread.
    /// Otherwise like `push_video_frame`.
    pub fn push_video_frame_rgba(&self, width: u32, height: u32, rotation: u32, data: Vec<u8>) -> bool {
        self.open().current_video_call().is_some_and(|c| c.video.push_rgba(width, height, rotation, data))
    }

    /// One frame of packed YUYV (YUY2), rows `stride` bytes apart (at least `width * 2`; even
    /// width), converted on the encoder thread. Otherwise like `push_video_frame`.
    pub fn push_video_frame_yuyv(&self, width: u32, height: u32, rotation: u32, stride: u32, data: Vec<u8>) -> bool {
        self.open().current_video_call().is_some_and(|c| c.video.push_yuyv(width, height, rotation, stride, data))
    }

    /// The newest decoded frame of the peer's video not pulled yet, else `None`. Poll it on the
    /// display's clock. Nothing while the peer's camera is off.
    pub fn pull_video_frame(&self) -> Option<VideoFrame> {
        self.open().current_video_call()?.video.take_frame().map(VideoFrame::from)
    }

    /// Like `pull_video_frame`, but `data` is packed RGBA (`width * height * 4` bytes; Android
    /// `ARGB_8888` bitmaps and iced images use this byte order), converted on the caller's
    /// thread.
    pub fn pull_video_frame_rgba(&self) -> Option<VideoFrame> {
        self.open().current_video_call()?.video.take_frame().map(|f| VideoFrame::from(f.into_rgba()))
    }

    /// This node's handle for the platform's direct frame calls (Android `VideoJni`: camera
    /// planes and the RGBA pull through direct `ByteBuffer`s, no copies through UniFFI). The
    /// same for the life of the node; a handle of a dropped node finds nothing.
    pub fn video_frame_handle(&self) -> u64 {
        let mut ports = FRAME_PORTS.lock();
        ports.retain(|(_, w)| w.strong_count() > 0);
        if let Some((h, _)) = ports.iter().find(|(_, w)| std::ptr::eq(w.as_ptr(), Arc::as_ptr(&self.hub))) {
            return *h;
        }
        let h = NEXT_FRAME_PORT.fetch_add(1, Ordering::Relaxed);
        ports.push((h, Arc::downgrade(&self.hub)));
        h
    }

    pub fn video_info(&self) -> Option<VideoInfo> {
        let call = self.open().current_video_call()?;
        Some(call.video.info(&call.info.call_id))
    }

    pub fn video_stats(&self) -> Option<VideoStats> {
        Some(self.open().current_video_call()?.video.stats())
    }

    /// The camera captures HD (1280x720) now: the app's per-call choice, else its setting, else
    /// the core's own (`VideoStats::hd_auto`). Call when the call's video starts and on every
    /// change. Ignored without a call.
    pub fn set_video_hd(&self, on: bool) {
        if let Some(call) = self.open().current_video_call() {
            call.video.set_hd(on);
        }
    }

    /// Where and how big the remote picture is drawn (device pixels), and whether it is on
    /// screen. Call on every layout change; the core asks for one ladder rung above the one that covers it, holds a
    /// shrink for a second and tells the peer's sender (`docs/design/video-adaptation.md`).
    /// Hidden: the peer sends 180p at 15 fps. Ignored without a call.
    pub fn set_video_view(&self, width_px: u32, height_px: u32, visible: bool) {
        self.set_video_view_of(0, width_px, height_px, visible);
    }

    /// [`Self::set_video_view`] for one of several places the app draws the remote picture
    /// (`view`: the app's id for it, stable while it is composed). The peer is asked for the
    /// largest one shown; hidden only when none is.
    pub fn set_video_view_of(&self, view: u32, width_px: u32, height_px: u32, visible: bool) {
        if let Some(call) = self.open().current_video_call() {
            call.video.set_view(view, width_px, height_px, visible);
        }
    }

    /// How long the app spent pulling and drawing one remote frame (ms). Call once per drawn
    /// frame; the core adds its p75 (2 s) to the decode cost when steering the frame rate.
    /// Ignored without a call.
    pub fn report_video_render_ms(&self, ms: f32) {
        if let Some(call) = self.open().current_video_call() {
            call.video.report_render_ms(ms);
        }
    }

    /// The device's thermal and battery state. Lowers what we ask the peer to send and what we
    /// send ourselves (MODERATE: 15 fps up to 540p, SEVERE: 15 fps up to 360p, battery saver:
    /// 24 fps up to 540p). Ignored without a call.
    pub fn set_video_budget(&self, budget: VideoBudget) {
        if let Some(call) = self.open().current_video_call() {
            call.video.set_budget(budget);
        }
    }
}
