//! Camera frames as pushed: sources, size checks and plane geometry.

use std::time::Instant;

use video::I420;
use video::convert::{self, Prepare, Range, RgbLayout, Rotation, Yuv420};

/// Largest camera frame side `push_video_frame` takes.
pub const MAX_INPUT_SIDE: u32 = 4096;

/// A camera frame as pushed; converted on the encoder thread.
pub enum Source {
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

pub struct Input {
    pub width: usize,
    pub height: usize,
    pub src: Source,
    pub rotation: u32,
    pub at: Instant,
}

impl Input {
    /// Upright and fit into the rung `rw` x `rh` (see [`fit`]); `prepare`'s center crop then
    /// only trims what even rounding loses.
    pub fn prepare(&self, rw: usize, rh: usize) -> video::Result<I420> {
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
pub fn chroma_views<'a>(u: &'a [u8], v: &'a [u8]) -> (&'a [u8], &'a [u8]) {
    match (u.is_empty(), v.is_empty()) {
        (true, false) => (&v[1..], v),
        (false, true) => (u, &u[1..]),
        _ => (u, v),
    }
}

/// U and V of `YUV_420_888` given as memory spans (address, length): with pixel stride 2 and
/// one starting a byte after the other they are views of one interleaved plane, returned once
/// as their union (the other span empty, as `chroma_views` expects); otherwise as they are.
pub fn chroma_spans(u: (usize, usize), v: (usize, usize), pixel_stride: usize) -> ((usize, usize), (usize, usize)) {
    let (none, union) = ((0, 0), |a: (usize, usize), b: (usize, usize)| (a.0, a.1.max(b.1 + 1)));
    if pixel_stride == 2 && u.1 > 0 && v.1 > 0 {
        if Some(u.0) == v.0.checked_add(1) {
            return (none, union(v, u));
        }
        if Some(v.0) == u.0.checked_add(1) {
            return (union(u, v), none);
        }
    }
    (u, v)
}

/// Whether `len` bytes hold a plane of `w` x `h` samples, rows `stride` apart, samples `step`
/// apart (the last row may end right after its last sample).
pub fn plane_fits(len: usize, w: usize, h: usize, stride: usize, step: usize) -> bool {
    let row = (w - 1) * step + 1;
    step >= 1 && stride >= row && len >= (h - 1) * stride + row
}

/// Whether `YUV_420_888` planes as `push_video_frame_yuv` takes them hold a `width` x `height`
/// frame.
#[allow(clippy::too_many_arguments)]
pub fn yuv_fits(width: u32, height: u32, y: &[u8], y_stride: usize, u: &[u8], v: &[u8], uv_stride: usize, uv_pixel_stride: usize) -> bool {
    if !size_ok(width, height) {
        return false;
    }
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = I420::chroma(w, h);
    let (cu, cv) = chroma_views(u, v);
    let ps = uv_pixel_stride;
    plane_fits(y.len(), w, h, y_stride, 1) && plane_fits(cu.len(), cw, ch, uv_stride, ps) && plane_fits(cv.len(), cw, ch, uv_stride, ps)
}

/// Bytes a plane of `w` x `h` samples, rows `stride` apart, samples `step` apart, spans.
pub fn plane_len(w: usize, h: usize, stride: usize, step: usize) -> usize {
    (h - 1) * stride + (w - 1) * step + 1
}

pub fn size_ok(width: u32, height: u32) -> bool {
    (1..=MAX_INPUT_SIDE).contains(&width) && (1..=MAX_INPUT_SIDE).contains(&height)
}

/// Largest even size of `sw` x `sh`'s aspect within the rung `rw` x `rh` (turned to match the
/// picture's orientation), never upscaled.
pub fn fit(sw: usize, sh: usize, rw: usize, rh: usize) -> (usize, usize) {
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
