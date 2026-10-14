//! The `Node` video API (UniFFI-exported) and the frame-port helpers behind it.

use super::*;

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
        self.inner.push_yuv_planes(width, height, rotation, y, y_row_stride, u, v, uv_row_stride, uv_pixel_stride, full_range)
    }

    /// `pull_video_frame_rgba` into the caller's buffer (no allocation for the pixels).
    pub fn pull_video_frame_rgba_into(&self, dst: &mut [u8]) -> Pulled {
        self.inner.pull_rgba_into(dst)
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
        let (ys, uvs, ps) = (y_row_stride as usize, uv_row_stride as usize, uv_pixel_stride as usize);
        if !yuv_fits(width, height, &y, ys, &u, &v, uvs, ps) {
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

    /// This node's handle for the platform's direct frame calls (Android `VideoJni`: camera
    /// planes and the RGBA pull through direct `ByteBuffer`s, no copies through UniFFI). The
    /// same for the life of the node; a handle of a dropped node finds nothing.
    pub fn video_frame_handle(&self) -> u64 {
        let mut ports = FRAME_PORTS.lock();
        ports.retain(|(_, w)| w.strong_count() > 0);
        if let Some((h, _)) = ports.iter().find(|(_, w)| std::ptr::eq(w.as_ptr(), Arc::as_ptr(&self.inner))) {
            return *h;
        }
        let h = NEXT_FRAME_PORT.fetch_add(1, Ordering::Relaxed);
        ports.push((h, Arc::downgrade(&self.inner)));
        h
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
