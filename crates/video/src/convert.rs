//! Frame plumbing between cameras/renderers and the codec: strided 4:2:0 (Android
//! `YUV_420_888`, NV12/NV21), YUYV and RGB(A) in; crop, scale, rotate, mirror; RGBA out.
//!
//! Colour: BT.601 limited range throughout (what the encoder is fed and the decoder returns;
//! `csrc/aom_rt.c` signals no colour description, which decoders treat as 601-ish limited).
//! MJPEG cameras must be decoded to one of these formats first (not handled here).

use crate::{Error, I420, Result};

/// Clockwise rotation applied to the camera image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    /// From Android `ImageInfo.rotationDegrees` (0/90/180/270; anything else is `R0`).
    pub fn from_degrees(d: i32) -> Self {
        match d.rem_euclid(360) {
            90 => Self::R90,
            180 => Self::R180,
            270 => Self::R270,
            _ => Self::R0,
        }
    }

    fn swaps(self) -> bool {
        matches!(self, Self::R90 | Self::R270)
    }
}

/// Sample range of the source. Cameras on Android usually deliver full range (JFIF).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Limited,
    Full,
}

/// Rectangle in source pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

/// A borrowed 4:2:0 picture with arbitrary strides: Android `YUV_420_888` (pixel stride 1 =
/// planar, 2 = NV12/NV21 views where `u` and `v` overlap), NV12 (`u = uv`, `v = &uv[1..]`),
/// or an [`I420`]. Planes may end right after the last pixel (no trailing row padding).
#[derive(Clone, Copy, Debug)]
pub struct Yuv420<'a> {
    pub y: &'a [u8],
    pub y_stride: usize,
    pub u: &'a [u8],
    pub v: &'a [u8],
    pub uv_stride: usize,
    pub uv_pixel_stride: usize,
    pub width: usize,
    pub height: usize,
}

impl<'a> Yuv420<'a> {
    pub fn from_i420(p: &'a I420) -> Self {
        let (y, u, v) = p.planes();
        let (cw, _) = I420::chroma(p.width, p.height);
        Self { y, y_stride: p.width, u, v, uv_stride: cw, uv_pixel_stride: 1, width: p.width, height: p.height }
    }

    /// NV12 (`vu = false`) or NV21 (`vu = true`): Y plane plus one interleaved chroma plane.
    pub fn nv12(y: &'a [u8], y_stride: usize, uv: &'a [u8], uv_stride: usize, w: usize, h: usize, vu: bool) -> Self {
        let (first, second) = (uv, uv.get(1..).unwrap_or(&[]));
        let (u, v) = if vu { (second, first) } else { (first, second) };
        Self { y, y_stride, u, v, uv_stride, uv_pixel_stride: 2, width: w, height: h }
    }

    /// Plane views of `r` (origin already even), checked against the slice lengths.
    fn planes(&self, r: Rect) -> Result<[Plane<'a>; 3]> {
        let (cx, cy) = (r.x / 2, r.y / 2);
        let (cw, ch) = I420::chroma(r.w, r.h);
        let ps = self.uv_pixel_stride;
        Ok([
            Plane::new(self.y, r.y * self.y_stride + r.x, self.y_stride, 1, r.w, r.h)?,
            Plane::new(self.u, cy * self.uv_stride + cx * ps, self.uv_stride, ps, cw, ch)?,
            Plane::new(self.v, cy * self.uv_stride + cx * ps, self.uv_stride, ps, cw, ch)?,
        ])
    }
}

/// Strided read-only plane view.
#[derive(Clone, Copy)]
struct Plane<'a> {
    data: &'a [u8],
    stride: usize,
    step: usize,
    w: usize,
    h: usize,
}

impl<'a> Plane<'a> {
    fn new(data: &'a [u8], off: usize, stride: usize, step: usize, w: usize, h: usize) -> Result<Self> {
        let need = if w == 0 || h == 0 { off } else { off + (h - 1) * stride + (w - 1) * step + 1 };
        if data.len() < need || step == 0 {
            return Err(Error::BadFrame(data.len(), need));
        }
        Ok(Self { data: &data[off.min(data.len())..], stride, step, w, h })
    }

    /// Row `y`, exactly `(w - 1) * step + 1` bytes.
    fn row(&self, y: usize) -> &'a [u8] {
        let s = y * self.stride;
        &self.data[s..s + (self.w - 1) * self.step + 1]
    }
}

/// What the call pipeline wants from a camera frame.
#[derive(Clone, Copy, Debug)]
pub struct Prepare {
    pub rotate: Rotation,
    /// Flip the result horizontally (self-view of a front camera; send unmirrored).
    pub mirror: bool,
    /// Output size after rotation; rounded down to even (the encoder needs even sizes).
    pub width: usize,
    pub height: usize,
    pub range: Range,
}

/// Strided 4:2:0 -> tightly packed I420 of the (optional) crop, no scaling. An odd crop
/// origin is moved one pixel up/left so chroma stays aligned.
pub fn yuv420_to_i420(src: &Yuv420, crop: Option<Rect>) -> Result<I420> {
    let r = clip(src, crop)?;
    let mut out = I420::new(r.w, r.h);
    let [py, pu, pv] = src.planes(r)?;
    let (dy, du, dv) = out.planes_mut();
    copy_plane(py, dy);
    copy_plane(pu, du);
    copy_plane(pv, dv);
    Ok(out)
}

/// The capture pipeline in one pass over the source: crop (optional), center-crop to the
/// output aspect, scale, range-convert, rotate and mirror. Scaling happens before rotation so
/// the rotation touches only output-sized data; the unrotated, unscaled case is a plain copy.
pub fn prepare(src: &Yuv420, crop: Option<Rect>, p: &Prepare) -> Result<I420> {
    let (ow, oh) = (p.width & !1, p.height & !1);
    if ow == 0 || oh == 0 {
        return Err(Error::BadFrame(0, 2));
    }
    let (pw, ph) = if p.rotate.swaps() { (oh, ow) } else { (ow, oh) };
    let r = aspect_crop(clip(src, crop)?, pw, ph);
    let mut pre = I420::new(pw, ph);
    let [py, pu, pv] = src.planes(r)?;
    {
        let (dy, du, dv) = pre.planes_mut();
        let (cw, ch) = I420::chroma(pw, ph);
        scale_plane(py, dy, pw, ph);
        scale_plane(pu, du, cw, ch);
        scale_plane(pv, dv, cw, ch);
    }
    if p.range == Range::Full {
        full_to_limited(&mut pre);
    }
    Ok(transform(&pre, p.rotate, p.mirror))
}

/// Center-crops `src` to the aspect of `w` x `h` and scales it to that size (`w`, `h` as given).
pub fn fit(src: &I420, w: usize, h: usize) -> I420 {
    let v = Yuv420::from_i420(src);
    let r = aspect_crop(Rect { x: 0, y: 0, w: src.width, h: src.height }, w, h);
    let mut out = I420::new(w, h);
    let [py, pu, pv] = v.planes(r).expect("I420 planes are complete");
    let (dy, du, dv) = out.planes_mut();
    let (cw, ch) = I420::chroma(w, h);
    scale_plane(py, dy, w, h);
    scale_plane(pu, du, cw, ch);
    scale_plane(pv, dv, cw, ch);
    out
}

/// Area-average downscale (nearest-neighbour upscale) of every plane, no cropping.
pub fn scale(src: &I420, w: usize, h: usize) -> I420 {
    if (src.width, src.height) == (w, h) {
        return src.clone();
    }
    let v = Yuv420::from_i420(src);
    let r = Rect { x: 0, y: 0, w: src.width, h: src.height };
    let mut out = I420::new(w, h);
    let [py, pu, pv] = v.planes(r).expect("I420 planes are complete");
    let (dy, du, dv) = out.planes_mut();
    let (cw, ch) = I420::chroma(w, h);
    scale_plane(py, dy, w, h);
    scale_plane(pu, du, cw, ch);
    scale_plane(pv, dv, cw, ch);
    out
}

fn clip(src: &Yuv420, crop: Option<Rect>) -> Result<Rect> {
    let mut r = crop.unwrap_or(Rect { x: 0, y: 0, w: src.width, h: src.height });
    if r.x + r.w > src.width || r.y + r.h > src.height || r.w == 0 || r.h == 0 {
        return Err(Error::BadFrame(r.x + r.w, src.width));
    }
    r.x &= !1;
    r.y &= !1;
    Ok(r)
}

/// Largest centered sub-rectangle of `r` with aspect `w:h`, origin even.
fn aspect_crop(r: Rect, w: usize, h: usize) -> Rect {
    let (cw, ch) = if r.w * h > r.h * w { ((r.h * w / h).max(1), r.h) } else { (r.w, (r.w * h / w).max(1)) };
    Rect { x: (r.x + (r.w - cw) / 2) & !1, y: (r.y + (r.h - ch) / 2) & !1, w: cw, h: ch }
}

fn copy_plane(src: Plane, dst: &mut [u8]) {
    if src.w == 0 {
        return;
    }
    for (y, d) in dst.chunks_exact_mut(src.w).enumerate().take(src.h) {
        copy_row(src.row(y), src.step, d);
    }
}

fn copy_row(s: &[u8], step: usize, d: &mut [u8]) {
    match step {
        1 => d.copy_from_slice(s),
        2 => {
            for (i, d) in d.iter_mut().enumerate() {
                *d = s[2 * i];
            }
        }
        _ => {
            for (d, s) in d.iter_mut().zip(s.iter().step_by(step)) {
                *d = *s;
            }
        }
    }
}

/// Scales a strided plane into a dense `dw` x `dh` plane. Paths: copy, exact 2:1 box, general
/// area average (integer box edges; nearest neighbour where upscaling).
fn scale_plane(src: Plane, dst: &mut [u8], dw: usize, dh: usize) {
    if dw == 0 || dh == 0 {
        return;
    }
    if (src.w, src.h) == (dw, dh) {
        return copy_plane(src, dst);
    }
    if src.w == 2 * dw && src.h == 2 * dh {
        let mut a = vec![0u8; src.w];
        let mut b = vec![0u8; src.w];
        for (y, d) in dst.chunks_exact_mut(dw).enumerate() {
            copy_row(src.row(2 * y), src.step, &mut a);
            copy_row(src.row(2 * y + 1), src.step, &mut b);
            for (i, d) in d.iter_mut().enumerate() {
                let s = a[2 * i] as u16 + a[2 * i + 1] as u16 + b[2 * i] as u16 + b[2 * i + 1] as u16;
                *d = ((s + 2) >> 2) as u8;
            }
        }
        return;
    }
    let xs: Vec<(usize, usize)> = (0..dw)
        .map(|x| {
            let x0 = x * src.w / dw;
            (x0, ((x + 1) * src.w / dw).max(x0 + 1) - x0)
        })
        .collect();
    let mut row = vec![0u8; src.w];
    let mut acc = vec![0u32; src.w];
    for (y, d) in dst.chunks_exact_mut(dw).enumerate() {
        let y0 = y * src.h / dh;
        let y1 = ((y + 1) * src.h / dh).max(y0 + 1);
        acc.fill(0);
        for yy in y0..y1 {
            copy_row(src.row(yy), src.step, &mut row);
            for (a, &s) in acc.iter_mut().zip(&row) {
                *a += s as u32;
            }
        }
        let ny = (y1 - y0) as u32;
        for (d, &(x0, nx)) in d.iter_mut().zip(&xs) {
            let sum: u32 = acc[x0..x0 + nx].iter().sum();
            let n = ny * nx as u32;
            *d = ((sum + n / 2) / n) as u8;
        }
    }
}

/// Full-range (JFIF) samples to limited range, in place.
pub fn full_to_limited(p: &mut I420) {
    let mut ly = [0u8; 256];
    let mut lc = [0u8; 256];
    for i in 0..256 {
        ly[i] = (16.0 + i as f64 * 219.0 / 255.0).round() as u8;
        lc[i] = (128.0 + (i as f64 - 128.0) * 224.0 / 255.0).round() as u8;
    }
    let n = p.width * p.height;
    let (y, c) = p.data.split_at_mut(n);
    y.iter_mut().for_each(|s| *s = ly[*s as usize]);
    c.iter_mut().for_each(|s| *s = lc[*s as usize]);
}

/// Rotates (clockwise) and optionally mirrors (after rotating) an I420 picture.
pub fn transform(src: &I420, rot: Rotation, mirror: bool) -> I420 {
    if rot == Rotation::R0 && !mirror {
        return src.clone();
    }
    let (w, h) = if rot.swaps() { (src.height, src.width) } else { (src.width, src.height) };
    let mut out = I420::new(w, h);
    let (sy, su, sv) = src.planes();
    let (scw, sch) = I420::chroma(src.width, src.height);
    let (dy, du, dv) = out.planes_mut();
    transform_plane(sy, src.width, src.height, dy, rot, mirror);
    transform_plane(su, scw, sch, du, rot, mirror);
    transform_plane(sv, scw, sch, dv, rot, mirror);
    out
}

fn transform_plane(src: &[u8], sw: usize, sh: usize, dst: &mut [u8], rot: Rotation, mirror: bool) {
    if sw == 0 || sh == 0 {
        return;
    }
    let (dw, dh) = if rot.swaps() { (sh, sw) } else { (sw, sh) };
    let (w, h) = (sw as isize, sh as isize);
    // Source index of destination (x, y) = base + x * ax + y * ay.
    let (mut base, mut ax, ay) = match rot {
        Rotation::R0 => (0, 1, w),
        Rotation::R90 => ((h - 1) * w, -w, 1),
        Rotation::R180 => (h * w - 1, -1, -w),
        Rotation::R270 => (w - 1, w, -1),
    };
    if mirror {
        base += (dw as isize - 1) * ax;
        ax = -ax;
    }
    if ay.abs() == w {
        // Rows map to rows: no tiling needed.
        for (y, d) in dst.chunks_exact_mut(dw).enumerate() {
            let start = base + y as isize * ay;
            if ax == 1 {
                d.copy_from_slice(&src[start as usize..start as usize + dw]);
            } else {
                let s = &src[(start + 1 - dw as isize) as usize..=start as usize];
                for (d, s) in d.iter_mut().zip(s.iter().rev()) {
                    *d = *s;
                }
            }
        }
        return;
    }
    // Rows map to columns: walk 32x32 tiles to stay in cache.
    const T: usize = 32;
    for ty in (0..dh).step_by(T) {
        for tx in (0..dw).step_by(T) {
            for y in ty..(ty + T).min(dh) {
                let row = &mut dst[y * dw..(y + 1) * dw];
                let b = base + y as isize * ay;
                for (x, d) in row.iter_mut().enumerate().take((tx + T).min(dw)).skip(tx) {
                    *d = src[(b + x as isize * ax) as usize];
                }
            }
        }
    }
}

/// Packed YUYV (YUY2: `Y0 U Y1 V`) -> I420; chroma averaged over row pairs. `stride` in bytes.
pub fn yuyv_to_i420(src: &[u8], stride: usize, w: usize, h: usize) -> Result<I420> {
    let (cw, ch) = I420::chroma(w, h);
    let need = if h == 0 { 0 } else { (h - 1) * stride + 4 * cw };
    if src.len() < need {
        return Err(Error::BadFrame(src.len(), need));
    }
    let mut out = I420::new(w, h);
    let (dy, du, dv) = out.planes_mut();
    for y in 0..h {
        let row = &src[y * stride..y * stride + 4 * cw];
        for (d, s) in dy[y * w..(y + 1) * w].iter_mut().zip(row.iter().step_by(2)) {
            *d = *s;
        }
    }
    for cy in 0..ch {
        let a = &src[2 * cy * stride..2 * cy * stride + 4 * cw];
        let b = if 2 * cy + 1 < h { &src[(2 * cy + 1) * stride..(2 * cy + 1) * stride + 4 * cw] } else { a };
        let (u, v) = (&mut du[cy * cw..(cy + 1) * cw], &mut dv[cy * cw..(cy + 1) * cw]);
        for (i, (u, v)) in u.iter_mut().zip(v.iter_mut()).enumerate() {
            *u = ((a[4 * i + 1] as u16 + b[4 * i + 1] as u16 + 1) >> 1) as u8;
            *v = ((a[4 * i + 3] as u16 + b[4 * i + 3] as u16 + 1) >> 1) as u8;
        }
    }
    Ok(out)
}

/// Byte layout of packed RGB input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RgbLayout {
    Rgb,
    Bgr,
    Rgba,
    Bgra,
}

/// Packed RGB -> I420 (BT.601 limited). Chroma from the 2x2 RGB average. `stride` in bytes.
pub fn rgb_to_i420(src: &[u8], stride: usize, w: usize, h: usize, layout: RgbLayout) -> Result<I420> {
    match layout {
        RgbLayout::Rgb => rgb_impl::<3, 0, 1, 2>(src, stride, w, h),
        RgbLayout::Bgr => rgb_impl::<3, 2, 1, 0>(src, stride, w, h),
        RgbLayout::Rgba => rgb_impl::<4, 0, 1, 2>(src, stride, w, h),
        RgbLayout::Bgra => rgb_impl::<4, 2, 1, 0>(src, stride, w, h),
    }
}

fn rgb_impl<const N: usize, const R: usize, const G: usize, const B: usize>(
    src: &[u8],
    stride: usize,
    w: usize,
    h: usize,
) -> Result<I420> {
    let need = if h == 0 { 0 } else { (h - 1) * stride + N * w };
    if src.len() < need {
        return Err(Error::BadFrame(src.len(), need));
    }
    let mut out = I420::new(w, h);
    let (cw, _) = I420::chroma(w, h);
    let (dy, du, dv) = out.planes_mut();
    for y in 0..h {
        let row = &src[y * stride..y * stride + N * w];
        for (d, p) in dy[y * w..(y + 1) * w].iter_mut().zip(row.chunks_exact(N)) {
            let (r, g, b) = (p[R] as i32, p[G] as i32, p[B] as i32);
            *d = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
        }
    }
    for (cy, (u, v)) in du.chunks_exact_mut(cw.max(1)).zip(dv.chunks_exact_mut(cw.max(1))).enumerate() {
        let a = &src[2 * cy * stride..2 * cy * stride + N * w];
        let b = if 2 * cy + 1 < h { &src[(2 * cy + 1) * stride..(2 * cy + 1) * stride + N * w] } else { a };
        for (i, (u, v)) in u.iter_mut().zip(v.iter_mut()).enumerate() {
            let x1 = (2 * i + 1).min(w - 1);
            let px = [&a[2 * i * N..], &a[x1 * N..], &b[2 * i * N..], &b[x1 * N..]];
            let sum = |c: usize| px.iter().map(|p| p[c] as i32).sum::<i32>();
            // 2x2 sums; the extra factor 4 folds into the shift.
            let (r, g, bl) = (sum(R), sum(G), sum(B));
            *u = ((-38 * r - 74 * g + 112 * bl + 512 + (128 << 10)) >> 10) as u8;
            *v = ((112 * r - 94 * g - 18 * bl + 512 + (128 << 10)) >> 10) as u8;
        }
    }
    Ok(out)
}

/// I420 (BT.601 limited) -> RGBA, `out` = `width * height * 4` bytes. Android `ARGB_8888`
/// bitmaps and `ANativeWindow` `RGBA_8888` buffers, and iced images, all use this byte order.
pub fn i420_to_rgba(src: &I420, out: &mut [u8]) -> Result<()> {
    let (w, h) = (src.width, src.height);
    if out.len() < w * h * 4 {
        return Err(Error::BadFrame(out.len(), w * h * 4));
    }
    let (sy, su, sv) = src.planes();
    let (cw, _) = I420::chroma(w, h);
    for y in 0..h {
        let yrow = &sy[y * w..(y + 1) * w];
        let urow = &su[(y / 2) * cw..(y / 2 + 1) * cw];
        let vrow = &sv[(y / 2) * cw..(y / 2 + 1) * cw];
        let orow = &mut out[y * w * 4..(y + 1) * w * 4];
        for (((o, ys), &u), &v) in orow.chunks_mut(8).zip(yrow.chunks(2)).zip(urow).zip(vrow) {
            let (d, e) = (u as i32 - 128, v as i32 - 128);
            let (rc, gc, bc) = (409 * e + 128, -100 * d - 208 * e + 128, 516 * d + 128);
            for (o, &yy) in o.chunks_exact_mut(4).zip(ys) {
                let c = 298 * (yy as i32 - 16);
                o[0] = ((c + rc) >> 8).clamp(0, 255) as u8;
                o[1] = ((c + gc) >> 8).clamp(0, 255) as u8;
                o[2] = ((c + bc) >> 8).clamp(0, 255) as u8;
                o[3] = 255;
            }
        }
    }
    Ok(())
}

pub fn to_rgba(src: &I420) -> Vec<u8> {
    let mut out = vec![0; src.width * src.height * 4];
    i420_to_rgba(src, &mut out).expect("sized");
    out
}

#[cfg(test)]
mod tests;
