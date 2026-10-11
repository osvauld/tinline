//! AV1 video codec for the call path (spike).
//!
//! Pure and synchronous like `audio`: no I/O. The caller feeds I420 frames to a
//! [`VideoEncoder`] and ships the returned [`Packet`]s; [`Decoder`] turns received packets back
//! into I420. Encoders: [`aom::AomEncoder`] (libaom realtime) and [`Encoder`] (rav1e, kept for
//! comparison: it holds ~4 frames and is too slow for calls). Decoder: rav1d (Rust port of
//! dav1d), which is the side that parses peer-controlled bytes.

use std::ffi::c_int;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::headers::DAV1D_PIXEL_LAYOUT_I420;
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib as d;

pub mod aom;
pub mod bench;
pub mod convert;
pub mod cpu;
#[cfg(feature = "rav1e")]
mod rav1e_enc;
#[cfg(feature = "rav1e")]
pub use rav1e_enc::Encoder;
pub mod media;
#[cfg(feature = "net")]
pub mod net;
pub mod sim;
pub mod y4m;

/// Anything that turns camera frames into AV1 packets.
pub trait VideoEncoder: Send {
    /// Encodes one frame; `key` forces a keyframe (after loss or for a new subscriber).
    fn encode(&mut self, pic: &I420, key: bool) -> Result<Vec<Packet>>;
    fn set_bitrate(&mut self, bps: u32) -> Result<()>;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("encoder config: {0}")]
    Config(String),
    #[error("encoder: {0}")]
    Encode(String),
    #[error("decoder error {0}")]
    Decode(i32),
    #[error("unsupported picture (bpc {0}, layout {1})")]
    Unsupported(i32, u32),
    #[error("frame is {0} bytes, expected {1}")]
    BadFrame(usize, usize),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Tightly packed 8-bit I420 picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct I420 {
    pub width: usize,
    pub height: usize,
    /// Y plane (`width * height`), then U and V (`cw * ch` each).
    pub data: Vec<u8>,
}

impl I420 {
    pub fn chroma(width: usize, height: usize) -> (usize, usize) {
        (width.div_ceil(2), height.div_ceil(2))
    }

    pub fn len(width: usize, height: usize) -> usize {
        let (cw, ch) = Self::chroma(width, height);
        width * height + 2 * cw * ch
    }

    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, data: vec![0; Self::len(width, height)] }
    }

    pub fn planes(&self) -> (&[u8], &[u8], &[u8]) {
        let y = self.width * self.height;
        let (cw, ch) = Self::chroma(self.width, self.height);
        let c = cw * ch;
        (&self.data[..y], &self.data[y..y + c], &self.data[y + c..y + 2 * c])
    }

    pub fn planes_mut(&mut self) -> (&mut [u8], &mut [u8], &mut [u8]) {
        let y = self.width * self.height;
        let (cw, ch) = Self::chroma(self.width, self.height);
        let c = cw * ch;
        let (ys, rest) = self.data.split_at_mut(y);
        let (us, vs) = rest.split_at_mut(c);
        (ys, us, &mut vs[..c])
    }
}

#[derive(Clone, Debug)]
pub struct EncoderConfig {
    pub width: usize,
    pub height: usize,
    pub fps: u32,
    pub bitrate_bps: u32,
    /// Maximum frames between keyframes.
    pub keyint: u64,
    /// rav1e speed preset 0..=10 (10 fastest).
    pub speed: u8,
    /// Worker threads; 0 = rav1e's choice.
    pub threads: usize,
    /// Tiles (parallelism inside one frame); 0 = automatic.
    pub tiles: usize,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
            fps: 30,
            bitrate_bps: 600_000,
            keyint: 300,
            speed: 10,
            threads: 0,
            tiles: 0,
        }
    }
}

/// One encoded frame: a temporal unit of AV1 OBUs.
#[derive(Clone, Debug)]
pub struct Packet {
    pub data: Vec<u8>,
    pub key: bool,
    /// Index of the input frame this packet encodes.
    pub frame: u64,
}

const EAGAIN: i32 = 11;

pub struct Decoder {
    ctx: Option<Dav1dContext>,
}

// rav1d's context is internally synchronised (it is the dav1d C API).
unsafe impl Send for Decoder {}

impl Decoder {
    /// `threads` 0 = automatic. `max_pixels` caps the frame size a peer can make us allocate.
    pub fn new(threads: usize, max_pixels: u32) -> Result<Self> {
        let mut s = MaybeUninit::<Dav1dSettings>::uninit();
        // SAFETY: dav1d_default_settings fully initialises the struct.
        let mut s = unsafe {
            d::dav1d_default_settings(NonNull::new_unchecked(s.as_mut_ptr()));
            s.assume_init()
        };
        s.n_threads = threads as c_int;
        // No frame delay: a frame comes out as soon as its data is in.
        s.max_frame_delay = 1;
        s.frame_size_limit = max_pixels;
        let mut ctx: Option<Dav1dContext> = None;
        // SAFETY: both pointers are valid for the call.
        let r = unsafe { d::dav1d_open(Some(NonNull::from(&mut ctx)), Some(NonNull::from(&mut s))) };
        if r.0 < 0 {
            return Err(Error::Decode(r.0));
        }
        Ok(Self { ctx })
    }

    /// Feeds one packet and returns the pictures it completed (normally exactly one).
    pub fn decode(&mut self, packet: &[u8]) -> Result<Vec<I420>> {
        let mut data = Dav1dData {
            data: None,
            sz: 0,
            r#ref: None,
            m: Default::default(),
        };
        // SAFETY: dav1d_data_create allocates `len` bytes owned by `data`.
        let buf = unsafe { d::dav1d_data_create(Some(NonNull::from(&mut data)), packet.len()) };
        if buf.is_null() {
            return Err(Error::Decode(-12));
        }
        // SAFETY: `buf` points at `packet.len()` writable bytes.
        unsafe { std::ptr::copy_nonoverlapping(packet.as_ptr(), buf, packet.len()) };
        let mut out = Vec::new();
        loop {
            // SAFETY: ctx is open; `data` is a valid Dav1dData.
            let r = unsafe { d::dav1d_send_data(self.ctx, Some(NonNull::from(&mut data))) };
            let pending = r.0 == -EAGAIN;
            if r.0 < 0 && !pending {
                // SAFETY: releases what send_data did not consume.
                unsafe { d::dav1d_data_unref(Some(NonNull::from(&mut data))) };
                return Err(Error::Decode(r.0));
            }
            self.pictures(&mut out)?;
            if !pending || data.sz == 0 {
                break;
            }
        }
        Ok(out)
    }

    fn pictures(&mut self, out: &mut Vec<I420>) -> Result<()> {
        loop {
            let mut pic = MaybeUninit::<Dav1dPicture>::zeroed();
            // SAFETY: ctx is open; `pic` is writable.
            let r = unsafe {
                d::dav1d_get_picture(self.ctx, Some(NonNull::new_unchecked(pic.as_mut_ptr())))
            };
            if r.0 == -EAGAIN {
                return Ok(());
            }
            if r.0 < 0 {
                return Err(Error::Decode(r.0));
            }
            // SAFETY: get_picture succeeded, so `pic` is initialised.
            let mut pic = unsafe { pic.assume_init() };
            let res = copy_picture(&pic);
            // SAFETY: we own this reference.
            unsafe { d::dav1d_picture_unref(Some(NonNull::from(&mut pic))) };
            out.push(res?);
        }
    }
}

fn copy_picture(pic: &Dav1dPicture) -> Result<I420> {
    if pic.p.bpc != 8 || pic.p.layout != DAV1D_PIXEL_LAYOUT_I420 {
        return Err(Error::Unsupported(pic.p.bpc, pic.p.layout));
    }
    let (w, h) = (pic.p.w as usize, pic.p.h as usize);
    let (cw, ch) = I420::chroma(w, h);
    let mut img = I420::new(w, h);
    let (y, u, v) = img.planes_mut();
    let copy = |dst: &mut [u8], src: *const u8, stride: isize, pw: usize, ph: usize| {
        for row in 0..ph {
            // SAFETY: dav1d guarantees `ph` rows of at least `pw` bytes at `stride`.
            let s = unsafe { std::slice::from_raw_parts(src.offset(row as isize * stride), pw) };
            dst[row * pw..(row + 1) * pw].copy_from_slice(s);
        }
    };
    let plane = |i: usize| pic.data[i].map(|p| p.as_ptr() as *const u8).ok_or(Error::Decode(-22));
    copy(y, plane(0)?, pic.stride[0], w, h);
    copy(u, plane(1)?, pic.stride[1], cw, ch);
    copy(v, plane(2)?, pic.stride[1], cw, ch);
    Ok(img)
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: closes the context we opened.
        unsafe { d::dav1d_close(Some(NonNull::from(&mut self.ctx))) };
    }
}

/// PSNR of the luma planes, for tests and benchmarks.
pub fn psnr_y(a: &I420, b: &I420) -> f64 {
    let (ya, _, _) = a.planes();
    let (yb, _, _) = b.planes();
    let mse = ya.iter().zip(yb).map(|(x, y)| (*x as f64 - *y as f64).powi(2)).sum::<f64>()
        / ya.len() as f64;
    if mse == 0.0 { 99.0 } else { 10.0 * (255.0 * 255.0 / mse).log10() }
}

/// Synthetic "camera" for tests and benchmarks: a textured background panning slowly, a
/// moving face-sized blob, and sensor-like noise, so the encoder sees realistic motion and
/// detail rather than flat colour.
pub struct Pattern {
    width: usize,
    height: usize,
    t: u64,
    seed: u32,
}

impl Pattern {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, t: 0, seed: 0x9e37_79b9 }
    }

    fn noise(&mut self) -> i32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 7) as i32 - 3
    }

    pub fn next_frame(&mut self) -> I420 {
        let (w, h, t) = (self.width, self.height, self.t as f32);
        let mut img = I420::new(w, h);
        let pan = (t * 0.7) as usize;
        let (bx, by) = (
            w as f32 * (0.5 + 0.25 * (t / 40.0).sin()),
            h as f32 * (0.5 + 0.15 * (t / 25.0).cos()),
        );
        let r2 = (h as f32 * 0.22).powi(2);
        let mut ys = vec![0u8; w * h];
        for yy in 0..h {
            for xx in 0..w {
                let gx = xx + pan;
                let tex = ((gx / 16 + yy / 16) % 2) as i32 * 40
                    + ((gx as f32 * 0.11).sin() * 20.0 + (yy as f32 * 0.07).cos() * 20.0) as i32;
                let d2 = (xx as f32 - bx).powi(2) + (yy as f32 - by).powi(2);
                let base = if d2 < r2 { 170 + (d2 / r2 * 30.0) as i32 } else { 90 + tex };
                ys[yy * w + xx] = (base + self.noise()).clamp(0, 255) as u8;
            }
        }
        let (cw, ch) = I420::chroma(w, h);
        let (y, u, v) = img.planes_mut();
        y.copy_from_slice(&ys);
        for cy in 0..ch {
            for cx in 0..cw {
                let d2 = (cx as f32 * 2.0 - bx).powi(2) + (cy as f32 * 2.0 - by).powi(2);
                let face = d2 < r2;
                u[cy * cw + cx] = if face { 110 } else { (128 + ((cx + pan / 2) % 32) as i32 - 16) as u8 };
                v[cy * cw + cx] = if face { 150 } else { 128 };
            }
        }
        self.t += 1;
        img
    }
}

#[cfg(test)]
mod tests;
