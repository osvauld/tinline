//! libvpx VP8 decoder through `csrc/vpx_dec.c` (desktop: phones with a hardware VP8 encoder
//! send VP8, `docs/design/video-hw-vp8.md`). It parses peer-controlled bytes in C, so every
//! keyframe's size is checked against `max_pixels` before libvpx sees it, decoded sizes are
//! checked again, and libvpx itself is built with a frame size cap (`scripts/build_libvpx.py`).

use std::ffi::{c_int, c_uchar, c_uint};

use crate::{Error, I420, Result, VideoDecoder};

#[repr(C)]
struct Raw {
    _p: [u8; 0],
}

unsafe extern "C" {
    fn tl_vpx_new(threads: c_uint) -> *mut Raw;
    fn tl_vpx_decode(d: *mut Raw, data: *const c_uchar, len: usize) -> c_int;
    fn tl_vpx_next(d: *mut Raw, w: *mut c_uint, h: *mut c_uint) -> c_int;
    fn tl_vpx_copy(d: *mut Raw, out: *mut c_uchar, cap: usize) -> c_int;
    fn tl_vpx_free(d: *mut Raw);
}

/// Width and height of a VP8 keyframe (RFC 6386 9.1), `None` for an interframe. Errors on a
/// keyframe header that is cut short or lacks the start code.
pub fn keyframe_size(packet: &[u8]) -> Result<Option<(u32, u32)>> {
    let Some(&tag) = packet.first() else {
        return Err(Error::Decode(-1));
    };
    if tag & 1 != 0 {
        return Ok(None);
    }
    if packet.len() < 10 || packet[3..6] != [0x9d, 0x01, 0x2a] {
        return Err(Error::Decode(-1));
    }
    let w = u16::from_le_bytes([packet[6], packet[7]]) & 0x3fff;
    let h = u16::from_le_bytes([packet[8], packet[9]]) & 0x3fff;
    Ok(Some((w as u32, h as u32)))
}

pub struct Vp8Decoder {
    raw: *mut Raw,
    max_pixels: u64,
}

// The libvpx context is only used through &mut self.
unsafe impl Send for Vp8Decoder {}

impl Vp8Decoder {
    /// `max_pixels` caps the frame size a peer can make us allocate (larger keyframes are
    /// refused before decoding).
    pub fn new(max_pixels: u32) -> Result<Self> {
        // SAFETY: plain value in, owned pointer (or null) out.
        let raw = unsafe { tl_vpx_new(1) };
        if raw.is_null() {
            return Err(Error::Config("libvpx init failed".into()));
        }
        Ok(Self {
            raw,
            max_pixels: max_pixels as u64,
        })
    }

    fn fits(&self, w: u32, h: u32) -> Result<()> {
        if w == 0 || h == 0 || w as u64 * h as u64 > self.max_pixels {
            return Err(Error::TooLarge(w, h));
        }
        Ok(())
    }
}

impl VideoDecoder for Vp8Decoder {
    fn decode(&mut self, packet: &[u8]) -> Result<Vec<I420>> {
        if let Some((w, h)) = keyframe_size(packet)? {
            self.fits(w, h)?;
        }
        // SAFETY: `raw` is open; `packet` is readable for its length.
        let r = unsafe { tl_vpx_decode(self.raw, packet.as_ptr(), packet.len()) };
        if r != 0 {
            return Err(Error::Decode(r));
        }
        let mut out = Vec::new();
        loop {
            let (mut w, mut h): (c_uint, c_uint) = (0, 0);
            // SAFETY: `raw` is open; both pointers are writable.
            match unsafe { tl_vpx_next(self.raw, &mut w, &mut h) } {
                0 => return Ok(out),
                1 => {}
                _ => return Err(Error::Unsupported(8, 0)),
            }
            self.fits(w, h)?;
            let mut img = I420::new(w as usize, h as usize);
            // SAFETY: `img.data` is writable for its length, which is the packed frame size.
            if unsafe { tl_vpx_copy(self.raw, img.data.as_mut_ptr(), img.data.len()) } != 0 {
                return Err(Error::BadFrame(img.data.len(), 0));
            }
            out.push(img);
        }
    }
}

impl Drop for Vp8Decoder {
    fn drop(&mut self) {
        // SAFETY: we own `raw`.
        unsafe { tl_vpx_free(self.raw) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `testdata/vp8_96x64.ivf`: 8 frames, keyframe first, made once with libvpx 1.16's vpxenc:
    ///
    /// ```text
    /// ffmpeg -f lavfi -i testsrc=size=96x64:rate=30 -frames:v 8 -pix_fmt yuv420p raw.yuv
    /// vpxenc --codec=vp8 --rt --cpu-used=8 -w 96 -h 64 --i420 --fps=30/1 --ivf \
    ///     --kf-max-dist=999 --target-bitrate=150 --limit=8 -o vp8_96x64.ivf raw.yuv
    /// ```
    const FIXTURE: &[u8] = include_bytes!("../testdata/vp8_96x64.ivf");

    /// The fixture's packets (IVF: 32-byte file header, then 12-byte frame headers).
    pub(crate) fn packets() -> Vec<&'static [u8]> {
        assert_eq!(&FIXTURE[..4], b"DKIF");
        assert_eq!(&FIXTURE[8..12], b"VP80");
        let mut at = u16::from_le_bytes([FIXTURE[6], FIXTURE[7]]) as usize;
        let mut out = Vec::new();
        while at + 12 <= FIXTURE.len() {
            let n = u32::from_le_bytes(FIXTURE[at..at + 4].try_into().unwrap()) as usize;
            out.push(&FIXTURE[at + 12..at + 12 + n]);
            at += 12 + n;
        }
        out
    }

    fn rng(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    #[test]
    fn decodes_the_fixture() {
        let pkts = packets();
        assert_eq!(pkts.len(), 8);
        assert_eq!(keyframe_size(pkts[0]).unwrap(), Some((96, 64)));
        assert_eq!(keyframe_size(pkts[1]).unwrap(), None);
        let mut dec = Vp8Decoder::new(1280 * 720).unwrap();
        for (i, p) in pkts.iter().enumerate() {
            let pics = dec.decode(p).unwrap();
            assert_eq!(pics.len(), 1, "frame {i}");
            assert_eq!((pics[0].width, pics[0].height), (96, 64));
            assert_eq!(pics[0].data.len(), I420::len(96, 64));
        }
    }

    #[test]
    fn oversize_keyframe_is_refused_before_decoding() {
        let pkts = packets();
        let mut dec = Vp8Decoder::new(96 * 64 - 1).unwrap();
        assert!(matches!(dec.decode(pkts[0]), Err(Error::TooLarge(96, 64))));
        // A forged keyframe header claiming 16383x16383 never reaches libvpx.
        let mut big = pkts[0].to_vec();
        big[6..10].copy_from_slice(&[0xff, 0x3f, 0xff, 0x3f]);
        let mut dec = Vp8Decoder::new(1280 * 720).unwrap();
        assert!(matches!(
            dec.decode(&big),
            Err(Error::TooLarge(16383, 16383))
        ));
        // Zero-sized keyframe.
        big[6..10].fill(0);
        assert!(dec.decode(&big).is_err());
    }

    #[test]
    fn truncated_and_garbage_are_errors() {
        let pkts = packets();
        let mut dec = Vp8Decoder::new(1280 * 720).unwrap();
        assert!(dec.decode(&[]).is_err());
        assert!(dec.decode(&pkts[0][..5]).is_err());
        // An interframe before any keyframe.
        assert!(dec.decode(pkts[1]).is_err());
        let mut dec = Vp8Decoder::new(1280 * 720).unwrap();
        for cut in [10, 20, pkts[0].len() / 2] {
            // Must not panic; libvpx may return a (corrupt-flagged) frame or an error.
            let _ = dec.decode(&pkts[0][..cut]);
        }
        // A good keyframe still decodes afterwards.
        let mut dec = Vp8Decoder::new(1280 * 720).unwrap();
        assert_eq!(dec.decode(pkts[0]).unwrap().len(), 1);
    }

    #[test]
    fn random_bytes_never_panic() {
        let pkts = packets();
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut dec = Vp8Decoder::new(640 * 480).unwrap();
        let mut errors = 0;
        for i in 0..4000 {
            let len = (rng(&mut seed) % 2000) as usize;
            let mut p: Vec<u8> = (0..len).map(|_| rng(&mut seed) as u8).collect();
            if i % 4 == 0 && !p.is_empty() {
                // Plausible keyframe headers reach deeper into the decoder.
                let k = pkts[0];
                let n = p.len().min(k.len()).min(10);
                p[..n].copy_from_slice(&k[..n]);
            } else if i % 4 == 1 {
                // Real packets with flipped bytes.
                p = pkts[(i / 4) % pkts.len()].to_vec();
                for _ in 0..1 + rng(&mut seed) % 8 {
                    let at = (rng(&mut seed) as usize) % p.len();
                    p[at] ^= rng(&mut seed) as u8 | 1;
                }
            }
            match dec.decode(&p) {
                Ok(pics) => {
                    for pic in pics {
                        assert!(pic.width * pic.height <= 640 * 480);
                        assert_eq!(pic.data.len(), I420::len(pic.width, pic.height));
                    }
                }
                Err(_) => errors += 1,
            }
            if i % 500 == 0 {
                dec = Vp8Decoder::new(640 * 480).unwrap();
            }
        }
        assert!(errors > 0);
    }
}
