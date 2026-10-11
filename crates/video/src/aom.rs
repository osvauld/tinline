//! libaom realtime AV1 encoder through `csrc/aom_rt.c`: no lookahead, one packet per frame.

use std::ffi::{c_int, c_long, c_uchar, c_uint};

use crate::{EncoderConfig, Error, I420, Packet, Result, VideoEncoder};

#[repr(C)]
struct Raw {
    _p: [u8; 0],
}

unsafe extern "C" {
    fn tl_aom_new(w: c_uint, h: c_uint, fps: c_uint, kbps: c_uint, threads: c_uint, speed: c_int, keyint: c_uint) -> *mut Raw;
    fn tl_aom_set_bitrate(e: *mut Raw, kbps: c_uint) -> c_int;
    fn tl_aom_encode(e: *mut Raw, i420: *const c_uchar, force_key: c_int, out: *mut c_uchar, cap: usize, key: *mut c_int) -> c_long;
    fn tl_aom_free(e: *mut Raw);
}

pub struct AomEncoder {
    raw: *mut Raw,
    width: usize,
    height: usize,
    frame: u64,
    buf: Vec<u8>,
}

// The libaom context is only used through &mut self.
unsafe impl Send for AomEncoder {}

impl AomEncoder {
    /// `cfg.speed` is libaom's cpu-used (realtime: 7..=10, higher is faster); `threads` 0 = 4.
    pub fn new(cfg: &EncoderConfig) -> Result<Self> {
        let threads = if cfg.threads == 0 { 4 } else { cfg.threads };
        // SAFETY: plain values in, owned pointer (or null) out.
        let raw = unsafe {
            tl_aom_new(
                cfg.width as c_uint,
                cfg.height as c_uint,
                cfg.fps,
                cfg.bitrate_bps / 1000,
                threads as c_uint,
                cfg.speed as c_int,
                cfg.keyint as c_uint,
            )
        };
        if raw.is_null() {
            return Err(Error::Config("libaom init failed".into()));
        }
        Ok(Self { raw, width: cfg.width, height: cfg.height, frame: 0, buf: vec![0; 1 << 20] })
    }
}

impl VideoEncoder for AomEncoder {
    fn encode(&mut self, pic: &I420, key: bool) -> Result<Vec<Packet>> {
        let want = I420::len(self.width, self.height);
        if pic.width != self.width || pic.height != self.height || pic.data.len() != want {
            return Err(Error::BadFrame(pic.data.len(), want));
        }
        let mut is_key: c_int = 0;
        // SAFETY: `pic.data` is a full packed I420 frame of the configured size; `buf` is
        // writable for its length.
        let n = unsafe {
            tl_aom_encode(self.raw, pic.data.as_ptr(), key as c_int, self.buf.as_mut_ptr(), self.buf.len(), &mut is_key)
        };
        let frame = self.frame;
        self.frame += 1;
        match n {
            0 => Ok(vec![]),
            n if n > 0 => Ok(vec![Packet { data: self.buf[..n as usize].to_vec(), key: is_key != 0, frame }]),
            n => Err(Error::Config(format!("libaom encode failed ({n})"))),
        }
    }

    fn set_bitrate(&mut self, bps: u32) -> Result<()> {
        // SAFETY: valid encoder.
        match unsafe { tl_aom_set_bitrate(self.raw, bps / 1000) } {
            0 => Ok(()),
            _ => Err(Error::Config("libaom set bitrate failed".into())),
        }
    }
}

impl Drop for AomEncoder {
    fn drop(&mut self) {
        // SAFETY: we own `raw`.
        unsafe { tl_aom_free(self.raw) };
    }
}
