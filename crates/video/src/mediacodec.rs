//! Android hardware VP8 through the NDK `AMediaCodec` API (design: `docs/design/video-hw-vp8.md`).
//!
//! - [`hardware_vp8_encoder`]: probe for the offer, cached once per process.
//! - [`MediaCodecVp8Encoder`]: [`VideoEncoder`] over the hardware VP8 encoder. ByteBuffer input,
//!   CBR, realtime priority, keyframes on request (`request-sync`), bitrate via `video-bitrate`.
//! - [`MediaCodecVp8Decoder`]: VP8 packets in, I420 out (ByteBuffer output, converted).
//!
//! Both run synchronously on the caller's codec thread with short dequeue timeouts: one call waits
//! at most about a frame (`ENCODE_BUDGET`, `DECODE_BUDGET`), drains every output that is ready,
//! and drops an input frame rather than queueing it when the codec has no free input buffer.
//!
//! Buffer layouts come from the codec, never assumed blindly: the `image-data` `MediaImage2`
//! the framework attaches to flexible-YUV formats when present, else `color-format` + `stride` +
//! `slice-height`. Every read/write is bounds-checked against the buffer size the codec reports;
//! malformed peer packets give errors, and a decoder whose codec failed rebuilds it on the next
//! packet.
//!
//! Format keys are passed as string literals (the `AMEDIAFORMAT_KEY_*` symbols are API-level
//! gated and would make `libp2pcore.so` fail to load on older releases).

// Wired into the call path by pipes.rs (send_codec); until then only the debug self-test uses it.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use ndk_sys as sys;
use crate::{EncoderConfig, Error, I420, Packet, Result, VideoDecoder, VideoEncoder};

const MIME: &CStr = c"video/x-vnd.on2.vp8";

// MediaCodecInfo.CodecCapabilities colour formats.
const COLOR_YUV420_PLANAR: i32 = 19;
const COLOR_YUV420_SEMIPLANAR: i32 = 21;
const COLOR_YUV420_PACKED_SEMIPLANAR: i32 = 39;
const COLOR_YUV420_FLEXIBLE: i32 = 0x7F42_0888;
/// QOMX_COLOR_FormatYVU420SemiPlanar (NV21).
const COLOR_QCOM_YVU420_SEMIPLANAR: i32 = 0x7FA3_0C00;
/// QOMX_COLOR_FORMATYUV420PackedSemiPlanar32m (linear NV12, Venus alignment).
const COLOR_QCOM_YUV420_SEMIPLANAR_32M: i32 = 0x7FA3_0C04;
/// `android._color-format` HAL_PIXEL_FORMAT_YV12 ('YV12': planar, V first, chroma rows aligned
/// to 16). The moto g52's `c2.android.vp8.encoder` names it but reads plain I420: U first and
/// chroma rows of half the stride (V first swapped the colours, the self-test's chroma PSNR 22 dB
/// for both planes; 16-aligned rows were wrong from 540 wide on, where 270 is not 272).
const HAL_YV12: i32 = 0x3231_5659;

const BITRATE_MODE_CBR: i32 = 2;
const BUFFER_FLAG_KEY_FRAME: u32 = sys::AMEDIACODEC_BUFFER_FLAG_KEY_FRAME;
const BUFFER_FLAG_CODEC_CONFIG: u32 = sys::AMEDIACODEC_BUFFER_FLAG_CODEC_CONFIG;
const INFO_TRY_AGAIN_LATER: isize = sys::AMEDIACODEC_INFO_TRY_AGAIN_LATER as isize;
const INFO_OUTPUT_FORMAT_CHANGED: isize = sys::AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED as isize;
const INFO_OUTPUT_BUFFERS_CHANGED: isize = sys::AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED as isize;

/// Longest wait for a free input buffer; after it the frame is dropped.
const INPUT_WAIT: Duration = Duration::from_millis(8);
/// How long `encode` waits for the frame it just queued; whatever is not out comes with the next
/// call. 0: on the Pixel 3a (c2.qti, 720p) a packet comes out ~28 ms after its input, so a 20 ms
/// wait caught only half of them and made `encode` look like 20 ms of work (p50 20.3 ms vs 6.2 ms
/// without, which is the NV12 copy) to the ladder's CPU gate; the cost is up to a frame of delay.
const ENCODE_BUDGET: Duration = Duration::ZERO;
const DECODE_BUDGET: Duration = Duration::from_millis(20);
/// Longest single blocking dequeue, so the budget check runs often.
const POLL: Duration = Duration::from_millis(4);
/// Largest VP8 packet we hand the decoder (a 720p keyframe is ~100 kB).
const MAX_PACKET: usize = 4 << 20;
/// Size the probe configures; every device that has the encoder takes it.
const PROBE_SIZE: (i32, i32) = (640, 360);

/// Software codecs (Google's), never what `hardware_vp8_encoder` means.
const SOFTWARE_PREFIXES: [&str; 3] = ["c2.android.", "OMX.google.", "c2.google."];
/// Vendor VP8 encoders tried by name when the default for the type is a software one.
const VENDOR_VP8_ENCODERS: [&CStr; 9] = [
    c"c2.qti.vp8.encoder",
    c"OMX.qcom.video.encoder.vp8",
    c"c2.exynos.vp8.encoder",
    c"OMX.Exynos.VP8.Encoder",
    c"c2.mtk.vp8.encoder",
    c"OMX.MTK.VIDEO.ENCODER.VP8",
    c"c2.unisoc.vp8.encoder",
    c"OMX.IMG.TOPAZ.VIDEO.Encoder",
    c"OMX.Intel.hw_ve.vp8",
];
/// Google's software VP8 encoders (every Android device has one), for the in-call tuning sheet's
/// "VP8 software": same bitstream as the hardware encoder, so the receiver notices nothing.
const SOFTWARE_VP8_ENCODERS: [&CStr; 2] = [c"c2.android.vp8.encoder", c"OMX.google.vp8.encoder"];
/// Software VP8 decoders, for the self-test's cross-check.
const SOFTWARE_VP8_DECODERS: [&CStr; 2] = [c"c2.android.vp8.decoder", c"OMX.google.vp8.decoder"];

fn is_hardware_name(name: &str) -> bool {
    !SOFTWARE_PREFIXES.iter().any(|p| name.starts_with(p)) && !name.contains(".sw.")
}

fn status(s: sys::media_status_t, what: &str) -> Result<()> {
    if s.0 == 0 {
        Ok(())
    } else {
        Err(Error::Config(format!("{what}: media status {}", s.0)))
    }
}

fn clamp_i32(v: u32) -> i32 {
    v.min(i32::MAX as u32) as i32
}

// ---- AMediaFormat / AMediaCodec wrappers ----

struct Format(NonNull<sys::AMediaFormat>);

impl Format {
    fn new() -> Result<Self> {
        // SAFETY: plain constructor; null is handled.
        Self::own(unsafe { sys::AMediaFormat_new() })
            .ok_or_else(|| Error::Config("AMediaFormat_new".into()))
    }

    fn own(p: *mut sys::AMediaFormat) -> Option<Self> {
        NonNull::new(p).map(Self)
    }

    fn set_i32(&self, key: &CStr, v: i32) -> &Self {
        // SAFETY: valid format; the key is copied.
        unsafe { sys::AMediaFormat_setInt32(self.0.as_ptr(), key.as_ptr(), v) };
        self
    }

    fn set_str(&self, key: &CStr, v: &CStr) -> &Self {
        // SAFETY: valid format; key and value are copied.
        unsafe { sys::AMediaFormat_setString(self.0.as_ptr(), key.as_ptr(), v.as_ptr()) };
        self
    }

    fn i32(&self, key: &CStr) -> Option<i32> {
        let mut v = 0;
        // SAFETY: valid format and out pointer.
        unsafe { sys::AMediaFormat_getInt32(self.0.as_ptr(), key.as_ptr(), &mut v) }.then_some(v)
    }

    /// Inclusive rectangle (left, top, right, bottom).
    fn rect(&self, key: &CStr) -> Option<(i32, i32, i32, i32)> {
        let (mut l, mut t, mut r, mut b) = (0, 0, 0, 0);
        // SAFETY: valid format and out pointers.
        unsafe {
            sys::AMediaFormat_getRect(
                self.0.as_ptr(),
                key.as_ptr(),
                &mut l,
                &mut t,
                &mut r,
                &mut b,
            )
        }
        .then_some((l, t, r, b))
    }

    /// A buffer entry, copied out (the format owns the original).
    fn buffer(&self, key: &CStr) -> Option<Vec<u8>> {
        let mut p: *mut c_void = std::ptr::null_mut();
        let mut n = 0usize;
        // SAFETY: valid format and out pointers; on success `p..p+n` is owned by the format.
        let ok =
            unsafe { sys::AMediaFormat_getBuffer(self.0.as_ptr(), key.as_ptr(), &mut p, &mut n) };
        if !ok || p.is_null() || n == 0 || n > 4096 {
            return None;
        }
        // SAFETY: as above; copied before the format can change.
        Some(unsafe { std::slice::from_raw_parts(p as *const u8, n) }.to_vec())
    }

    fn describe(&self) -> String {
        // SAFETY: the string is owned by the format and valid until the next call on it.
        let s = unsafe { sys::AMediaFormat_toString(self.0.as_ptr()) };
        if s.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned()
        }
    }
}

impl Drop for Format {
    fn drop(&mut self) {
        // SAFETY: we own the format.
        unsafe { sys::AMediaFormat_delete(self.0.as_ptr()) };
    }
}

struct Codec {
    raw: NonNull<sys::AMediaCodec>,
    name: String,
    started: bool,
}

// An AMediaCodec may be used from any thread; we use it only through &mut / & of its owner.
unsafe impl Send for Codec {}

impl Codec {
    fn own(p: *mut sys::AMediaCodec) -> Option<Self> {
        let raw = NonNull::new(p)?;
        let mut name: *mut c_char = std::ptr::null_mut();
        // SAFETY: valid codec; the name is released right after copying (API 28).
        let name = unsafe {
            if sys::AMediaCodec_getName(raw.as_ptr(), &mut name).0 == 0 && !name.is_null() {
                let s = CStr::from_ptr(name).to_string_lossy().into_owned();
                sys::AMediaCodec_releaseName(raw.as_ptr(), name);
                s
            } else {
                String::from("?")
            }
        };
        Some(Self {
            raw,
            name,
            started: false,
        })
    }

    fn by_name(name: &CStr) -> Option<Self> {
        // SAFETY: C string in, owned codec (or null) out.
        Self::own(unsafe { sys::AMediaCodec_createCodecByName(name.as_ptr()) })
    }

    fn encoder_by_type(mime: &CStr) -> Option<Self> {
        // SAFETY: as above.
        Self::own(unsafe { sys::AMediaCodec_createEncoderByType(mime.as_ptr()) })
    }

    fn decoder_by_type(mime: &CStr) -> Option<Self> {
        // SAFETY: as above.
        Self::own(unsafe { sys::AMediaCodec_createDecoderByType(mime.as_ptr()) })
    }

    fn configure(&self, f: &Format, encoder: bool) -> Result<()> {
        let flags = if encoder {
            sys::AMEDIACODEC_CONFIGURE_FLAG_ENCODE as u32
        } else {
            0
        };
        // SAFETY: valid codec and format; no surface, no crypto.
        status(
            unsafe {
                sys::AMediaCodec_configure(
                    self.raw.as_ptr(),
                    f.0.as_ptr(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    flags,
                )
            },
            "configure",
        )
    }

    fn start(&mut self) -> Result<()> {
        // SAFETY: valid, configured codec.
        status(
            unsafe { sys::AMediaCodec_start(self.raw.as_ptr()) },
            "start",
        )?;
        self.started = true;
        Ok(())
    }

    fn set_params(&self, f: &Format) -> Result<()> {
        // SAFETY: valid codec and format.
        status(
            unsafe { sys::AMediaCodec_setParameters(self.raw.as_ptr(), f.0.as_ptr()) },
            "setParameters",
        )
    }

    fn input_format(&self) -> Option<Format> {
        // SAFETY: valid codec; returns an owned format (API 28).
        Format::own(unsafe { sys::AMediaCodec_getInputFormat(self.raw.as_ptr()) })
    }

    fn output_format(&self) -> Option<Format> {
        // SAFETY: valid codec; returns an owned format.
        Format::own(unsafe { sys::AMediaCodec_getOutputFormat(self.raw.as_ptr()) })
    }

    /// A free input buffer's index within `wait`; None when all are busy.
    fn dequeue_input(&self, wait: Duration) -> Result<Option<usize>> {
        // SAFETY: valid, started codec.
        let idx = unsafe {
            sys::AMediaCodec_dequeueInputBuffer(self.raw.as_ptr(), wait.as_micros() as i64)
        };
        if idx == INFO_TRY_AGAIN_LATER {
            return Ok(None);
        }
        usize::try_from(idx)
            .map(Some)
            .map_err(|_| Error::Encode(format!("{}: dequeueInputBuffer {idx}", self.name)))
    }

    /// The bytes of dequeued input buffer `idx` (`&mut self`: one such slice at a time).
    fn input_buffer(&mut self, idx: usize) -> Result<&mut [u8]> {
        let mut cap = 0usize;
        // SAFETY: `idx` is a dequeued input buffer; the codec owns `p..p+cap` until queued.
        let p = unsafe { sys::AMediaCodec_getInputBuffer(self.raw.as_ptr(), idx, &mut cap) };
        if p.is_null() {
            return Err(Error::Encode(format!(
                "{}: no input buffer {idx}",
                self.name
            )));
        }
        // SAFETY: as above; the slice ends before queue_input hands the buffer back.
        Ok(unsafe { std::slice::from_raw_parts_mut(p, cap) })
    }

    fn queue_input(&self, idx: usize, len: usize, pts_us: u64) -> Result<()> {
        // SAFETY: `idx` was dequeued and `len` fits in its buffer (checked by the caller).
        status(
            unsafe { sys::AMediaCodec_queueInputBuffer(self.raw.as_ptr(), idx, 0, len, pts_us, 0) },
            "queueInputBuffer",
        )
    }

    /// One output event within `wait`.
    fn dequeue_output(&self, wait: Duration) -> Output<'_> {
        let mut info = sys::AMediaCodecBufferInfo {
            offset: 0,
            size: 0,
            presentationTimeUs: 0,
            flags: 0,
        };
        // SAFETY: valid, started codec; `info` is writable.
        let idx = unsafe {
            sys::AMediaCodec_dequeueOutputBuffer(
                self.raw.as_ptr(),
                &mut info,
                wait.as_micros() as i64,
            )
        };
        match idx {
            INFO_TRY_AGAIN_LATER => Output::None,
            INFO_OUTPUT_FORMAT_CHANGED => Output::FormatChanged,
            INFO_OUTPUT_BUFFERS_CHANGED => Output::Again,
            i if i < 0 => Output::Error(i),
            i => Output::Buffer(OutBuf {
                codec: self,
                idx: i as usize,
                info,
            }),
        }
    }
}

impl Drop for Codec {
    fn drop(&mut self) {
        // SAFETY: we own the codec; stop only if started.
        unsafe {
            if self.started {
                sys::AMediaCodec_stop(self.raw.as_ptr());
            }
            sys::AMediaCodec_delete(self.raw.as_ptr());
        }
    }
}

enum Output<'a> {
    None,
    Again,
    FormatChanged,
    Error(isize),
    Buffer(OutBuf<'a>),
}

/// A dequeued output buffer; released (not rendered) on drop.
struct OutBuf<'a> {
    codec: &'a Codec,
    idx: usize,
    info: sys::AMediaCodecBufferInfo,
}

impl OutBuf<'_> {
    /// The valid bytes (`offset..offset+size`), bounds-checked against the buffer.
    fn data(&self) -> Option<&[u8]> {
        let mut cap = 0usize;
        // SAFETY: `idx` is a dequeued output buffer; the codec owns `p..p+cap` until release.
        let p = unsafe {
            sys::AMediaCodec_getOutputBuffer(self.codec.raw.as_ptr(), self.idx, &mut cap)
        };
        let (off, len) = (
            usize::try_from(self.info.offset).ok()?,
            usize::try_from(self.info.size).ok()?,
        );
        if p.is_null() || off.checked_add(len)? > cap {
            return None;
        }
        // SAFETY: checked above; lives until self is dropped (release).
        Some(unsafe { std::slice::from_raw_parts(p.add(off), len) })
    }
}

impl Drop for OutBuf<'_> {
    fn drop(&mut self) {
        // SAFETY: returns the buffer we dequeued.
        unsafe { sys::AMediaCodec_releaseOutputBuffer(self.codec.raw.as_ptr(), self.idx, false) };
    }
}

// ---- YUV layouts ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Plane {
    off: usize,
    /// Bytes between rows.
    row: usize,
    /// Bytes between samples (1 planar, 2 semi-planar).
    col: usize,
}

impl Plane {
    /// One past the last byte touched for `w x h` samples starting at sample (x, y).
    fn end(&self, x: usize, y: usize, w: usize, h: usize) -> Option<usize> {
        if w == 0 || h == 0 {
            return Some(self.off);
        }
        let last = (y + h - 1)
            .checked_mul(self.row)?
            .checked_add((x + w - 1).checked_mul(self.col)?)?;
        self.off.checked_add(last)?.checked_add(1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layout {
    y: Plane,
    u: Plane,
    v: Plane,
    /// Where it came from, for logs.
    kind: &'static str,
}

impl Layout {
    fn semi_planar(stride: usize, slice: usize, vu: bool) -> Self {
        let c = stride * slice;
        let (u, v) = if vu { (c + 1, c) } else { (c, c + 1) };
        Self {
            y: Plane {
                off: 0,
                row: stride,
                col: 1,
            },
            u: Plane {
                off: u,
                row: stride,
                col: 2,
            },
            v: Plane {
                off: v,
                row: stride,
                col: 2,
            },
            kind: if vu { "nv21" } else { "nv12" },
        }
    }

    fn planar(stride: usize, slice: usize) -> Self {
        let (cs, ch) = (stride.div_ceil(2), slice.div_ceil(2));
        let u = stride * slice;
        Self {
            y: Plane {
                off: 0,
                row: stride,
                col: 1,
            },
            u: Plane {
                off: u,
                row: cs,
                col: 1,
            },
            v: Plane {
                off: u + cs * ch,
                row: cs,
                col: 1,
            },
            kind: "i420",
        }
    }

    /// `MediaImage2` (frameworks/native media/hardware/VideoAPI.h): 6 u32 header fields, then
    /// 4 planes of {offset u32, colInc i32, rowInc i32, horizSubsampling u32, vertSubsampling u32}.
    fn from_media_image(b: &[u8]) -> Option<Self> {
        let word = |i: usize| -> Option<u32> {
            Some(u32::from_ne_bytes(
                b.get(i * 4..i * 4 + 4)?.try_into().ok()?,
            ))
        };
        let (kind, planes, depth) = (word(0)?, word(1)?, word(4)?);
        if kind != 1 || planes != 3 || depth != 8 {
            return None; // not 8-bit YUV
        }
        let plane = |i: usize, sub: u32| -> Option<Plane> {
            let base = 6 + i * 5;
            let (off, col, row) = (word(base)?, word(base + 1)? as i32, word(base + 2)? as i32);
            if col <= 0 || row <= 0 || word(base + 3)? != sub || word(base + 4)? != sub {
                return None;
            }
            Some(Plane {
                off: off as usize,
                row: row as usize,
                col: col as usize,
            })
        };
        Some(Self {
            y: plane(0, 1)?,
            u: plane(1, 2)?,
            v: plane(2, 2)?,
            kind: "image-data",
        })
    }

    /// The layout a codec reports for `w x h` pictures in `f`.
    fn of(f: &Format, w: usize, h: usize) -> Result<Self> {
        if let Some(l) = f
            .buffer(c"image-data")
            .and_then(|b| Self::from_media_image(&b))
        {
            return Ok(l);
        }
        let color = f.i32(c"color-format").unwrap_or(COLOR_YUV420_FLEXIBLE);
        let stride = f
            .i32(c"stride")
            .and_then(|s| usize::try_from(s).ok())
            .filter(|&s| s >= w)
            .unwrap_or(w);
        let slice = f
            .i32(c"slice-height")
            .and_then(|s| usize::try_from(s).ok())
            .filter(|&s| s >= h)
            .unwrap_or(h);
        // A flexible format names the buffer's kind in `android._color-format`, and Google's
        // software encoders differ there: the Pixel 3a's `c2.android.vp8.encoder` says 19 (I420),
        // the moto g52's says YV12. Writing NV12 into those gave a grey picture with colour lines.
        // Neither is quite what they read (`HAL_YV12`, `or_nv21`); the self-test's chroma PSNR is the
        // check (`mediacodec_vp8_selftest`, software).
        let color = match (color, f.i32(c"android._color-format")) {
            (COLOR_YUV420_FLEXIBLE, Some(HAL_YV12)) => COLOR_YUV420_PLANAR,
            // Only the ones below; anything else keeps the flexible guess (NV12).
            (
                COLOR_YUV420_FLEXIBLE,
                Some(c @ (COLOR_YUV420_PLANAR | COLOR_YUV420_SEMIPLANAR | COLOR_QCOM_YVU420_SEMIPLANAR | COLOR_QCOM_YUV420_SEMIPLANAR_32M)),
            ) => c,
            _ => color,
        };
        match color {
            COLOR_YUV420_PLANAR => Ok(Self::planar(stride, slice)),
            // Flexible without a MediaImage2: hardware codecs use NV12 here (the self-test's
            // chroma PSNR and software-decoder cross-check would show otherwise).
            COLOR_YUV420_SEMIPLANAR
            | COLOR_YUV420_PACKED_SEMIPLANAR
            | COLOR_YUV420_FLEXIBLE
            | COLOR_QCOM_YUV420_SEMIPLANAR_32M => Ok(Self::semi_planar(stride, slice, false)),
            COLOR_QCOM_YVU420_SEMIPLANAR => Ok(Self::semi_planar(stride, slice, true)),
            other => Err(Error::Unsupported(8, other as u32)),
        }
    }

    /// Bytes needed for the region (x, y, w, h) of the luma plane (and its chroma).
    fn end(&self, x: usize, y: usize, w: usize, h: usize) -> Option<usize> {
        let (cw, ch) = I420::chroma(w, h);
        let (cx, cy) = (x / 2, y / 2);
        Some(
            self.y
                .end(x, y, w, h)?
                .max(self.u.end(cx, cy, cw, ch)?)
                .max(self.v.end(cx, cy, cw, ch)?),
        )
    }

    /// NV21 with the same stride and slice height, when this planar layout doesn't fit a `cap`
    /// byte buffer and NV21 does. Google's software encoder on the Pixel 3a names I420 (19) but
    /// hands out 480x640 buffers of 491489 bytes at stride 512: I420 needs 491504, a semi-planar
    /// one 491488; with NV12 the self-test's chroma PSNR was 22 dB for both U and V (swapped).
    /// The moto g52's names YV12 and reads I420 (`HAL_YV12`), which fits its buffers.
    fn or_nv21(&self, w: usize, h: usize, cap: usize) -> Option<Self> {
        if self.y.col != 1 || self.u.col != 1 || self.end(0, 0, w, h).is_some_and(|n| n <= cap) {
            return None;
        }
        let stride = self.y.row;
        let slice = self.u.off.min(self.v.off) / stride;
        let nv21 = Self::semi_planar(stride, slice, true);
        nv21.end(0, 0, w, h).is_some_and(|n| n <= cap).then_some(nv21)
    }

    /// Writes `pic` into `dst`; returns the bytes used.
    fn write(&self, pic: &I420, dst: &mut [u8]) -> Result<usize> {
        let (w, h) = (pic.width, pic.height);
        let need = self
            .end(0, 0, w, h)
            .filter(|&n| n <= dst.len())
            .ok_or(Error::BadFrame(dst.len(), I420::len(w, h)))?;
        let (cw, ch) = I420::chroma(w, h);
        let (y, u, v) = pic.planes();
        put_plane(y, w, h, dst, self.y, 0, 0);
        put_plane(u, cw, ch, dst, self.u, 0, 0);
        put_plane(v, cw, ch, dst, self.v, 0, 0);
        Ok(need)
    }

    /// Reads the region (x, y, w, h) of `src` as I420.
    fn read(&self, src: &[u8], x: usize, y: usize, w: usize, h: usize) -> Result<I420> {
        self.end(x, y, w, h)
            .filter(|&n| n <= src.len())
            .ok_or(Error::BadFrame(src.len(), I420::len(w, h)))?;
        let mut img = I420::new(w, h);
        let (cw, ch) = I420::chroma(w, h);
        let (yp, up, vp) = img.planes_mut();
        get_plane(src, self.y, x, y, w, h, yp);
        get_plane(src, self.u, x / 2, y / 2, cw, ch, up);
        get_plane(src, self.v, x / 2, y / 2, cw, ch, vp);
        Ok(img)
    }
}

/// Packed `w x h` `src` into plane `p` of `dst` (bounds already checked by `Layout::end`).
fn put_plane(src: &[u8], w: usize, h: usize, dst: &mut [u8], p: Plane, x: usize, y: usize) {
    for r in 0..h {
        let s = &src[r * w..(r + 1) * w];
        let d0 = p.off + (y + r) * p.row + x * p.col;
        if p.col == 1 {
            dst[d0..d0 + w].copy_from_slice(s);
        } else {
            for (i, &b) in s.iter().enumerate() {
                dst[d0 + i * p.col] = b;
            }
        }
    }
}

/// Plane `p` of `src` (region x, y, w, h) into packed `dst` (bounds already checked).
fn get_plane(src: &[u8], p: Plane, x: usize, y: usize, w: usize, h: usize, dst: &mut [u8]) {
    for r in 0..h {
        let s0 = p.off + (y + r) * p.row + x * p.col;
        let d = &mut dst[r * w..(r + 1) * w];
        if p.col == 1 {
            d.copy_from_slice(&src[s0..s0 + w]);
        } else {
            for (i, b) in d.iter_mut().enumerate() {
                *b = src[s0 + i * p.col];
            }
        }
    }
}

// ---- probe ----

fn encoder_format(w: i32, h: i32, fps: u32, bps: u32, keyint_s: i32, cbr: bool) -> Result<Format> {
    let f = Format::new()?;
    f.set_str(c"mime", MIME)
        .set_i32(c"width", w)
        .set_i32(c"height", h)
        .set_i32(c"color-format", COLOR_YUV420_FLEXIBLE)
        .set_i32(c"bitrate", clamp_i32(bps))
        .set_i32(c"frame-rate", fps as i32)
        .set_i32(c"i-frame-interval", keyint_s)
        // 0 = realtime; `latency` 1 = one frame in, one out (API 30+, ignored before).
        .set_i32(c"priority", 0)
        .set_i32(c"latency", 1);
    if cbr {
        f.set_i32(c"bitrate-mode", BITRATE_MODE_CBR);
    }
    Ok(f)
}

/// Creates `name` and configures it as a VP8 encoder at `w x h` (CBR, else the codec's mode).
fn configured_encoder(
    name: &CStr,
    w: i32,
    h: i32,
    fps: u32,
    bps: u32,
    keyint_s: i32,
) -> Result<(Codec, bool)> {
    let mut last = Error::Config(format!("{}: cannot create", name.to_string_lossy()));
    for cbr in [true, false] {
        // A failed configure can leave the codec unusable, so each attempt gets a fresh one.
        let Some(c) = Codec::by_name(name) else { break };
        match c.configure(&encoder_format(w, h, fps, bps, keyint_s, cbr)?, true) {
            Ok(()) => return Ok((c, cbr)),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn find_hardware_encoder() -> Option<CString> {
    let usable =
        |n: &CStr| configured_encoder(n, PROBE_SIZE.0, PROBE_SIZE.1, 30, 1_000_000, 10).is_ok();
    // The framework's default for the type is the highest-ranked encoder: the vendor's when it
    // has one (media_codecs.xml order / Codec2 rank).
    if let Some(name) = Codec::encoder_by_type(MIME).map(|c| c.name.clone())
        && is_hardware_name(&name)
        && let Ok(n) = CString::new(name)
        && usable(&n)
    {
        return Some(n);
    }
    VENDOR_VP8_ENCODERS
        .iter()
        .find(|n| usable(n))
        .map(|n| (*n).to_owned())
}

fn hardware_encoder_name() -> Option<&'static CStr> {
    static NAME: OnceLock<Option<CString>> = OnceLock::new();
    NAME.get_or_init(|| {
        let n = find_hardware_encoder();
        tracing::info!("hardware vp8 encoder: {:?}", n);
        n
    })
    .as_deref()
}

/// True iff this device has a hardware VP8 encoder that configures (cached once per process).
pub fn hardware_vp8_encoder() -> bool {
    hardware_encoder_name().is_some()
}

fn software_encoder_name() -> Option<&'static CStr> {
    static NAME: OnceLock<Option<CString>> = OnceLock::new();
    NAME.get_or_init(|| {
        let n = SOFTWARE_VP8_ENCODERS
            .iter()
            .find(|n| configured_encoder(n, PROBE_SIZE.0, PROBE_SIZE.1, 30, 1_000_000, 10).is_ok())
            .map(|n| (*n).to_owned());
        tracing::info!("software vp8 encoder: {:?}", n);
        n
    })
    .as_deref()
}

/// True iff Google's software VP8 encoder configures here (cached once per process). Never part of
/// the offer (Auto keeps the hardware score); only the in-call tuning sheet uses it.
pub fn software_vp8_encoder() -> bool {
    software_encoder_name().is_some()
}

/// True iff the VP8 decoder `MediaCodecVp8Decoder` gets by default (the framework's pick for the
/// type) is a hardware one (cached once per process). A phone with only the software decoder
/// still decodes VP8, so it offers `decode: vp8` but not `hw_decode`.
/// True iff Google's software VP8 decoder exists here (by name; cached once per process).
pub fn software_vp8_decoder() -> bool {
    static SW: OnceLock<bool> = OnceLock::new();
    *SW.get_or_init(|| SOFTWARE_VP8_DECODERS.iter().any(|n| Codec::by_name(n).is_some()))
}

pub fn hardware_vp8_decoder() -> bool {
    static HW: OnceLock<bool> = OnceLock::new();
    *HW.get_or_init(|| {
        let name = Codec::decoder_by_type(MIME).map(|c| c.name.clone());
        tracing::info!("default vp8 decoder: {:?}", name);
        name.is_some_and(|n| is_hardware_name(&n))
    })
}

// ---- encoder ----

/// The hardware VP8 encoder (see the module docs). Errors from `new` mean "use AV1".
pub struct MediaCodecVp8Encoder {
    codec: Codec,
    width: usize,
    height: usize,
    layout: Layout,
    /// The layout was checked against the first input buffer's size.
    layout_fits: bool,
    /// Configured frame rate: frame `n` gets pts `n / fps` (`pts`).
    fps: u32,
    last_pts: Option<u64>,
    /// Next input frame index.
    frame: u64,
    /// (pts, frame, queued at) of inputs whose packet has not come out yet.
    queued: VecDeque<(u64, u64, Instant)>,
    key_pending: bool,
    /// How long `encode` waits for the frame it just queued.
    wait: Duration,
    /// Queue-to-output time of the last packet.
    pub(crate) last_latency: Option<Duration>,
    /// Frames dropped because the codec had no free input buffer.
    pub(crate) dropped: u64,
    cbr: bool,
}

impl MediaCodecVp8Encoder {
    pub fn new(cfg: &EncoderConfig) -> Result<Self> {
        let name = hardware_encoder_name()
            .ok_or_else(|| Error::Config("no hardware vp8 encoder".into()))?;
        Self::with_name(name, cfg)
    }

    /// Google's software encoder (`c2.android.vp8.encoder`), for the in-call tuning sheet.
    pub fn new_software(cfg: &EncoderConfig) -> Result<Self> {
        let name = software_encoder_name()
            .ok_or_else(|| Error::Config("no software vp8 encoder".into()))?;
        Self::with_name(name, cfg)
    }

    fn with_name(name: &CStr, cfg: &EncoderConfig) -> Result<Self> {
        let (w, h) = (i32::try_from(cfg.width), i32::try_from(cfg.height));
        let (Ok(w), Ok(h)) = (w, h) else {
            return Err(Error::Config("size".into()));
        };
        let fps = cfg.fps.max(1);
        let keyint_s = (cfg.keyint / fps as u64).clamp(1, 3600) as i32;
        let (mut codec, cbr) = configured_encoder(name, w, h, fps, cfg.bitrate_bps, keyint_s)?;
        let input = codec
            .input_format()
            .ok_or_else(|| Error::Config("no input format".into()))?;
        let layout = Layout::of(&input, cfg.width, cfg.height)?;
        tracing::info!(
            "vp8 encoder {} {}x{} cbr={cbr} layout={layout:?} input={}",
            codec.name,
            w,
            h,
            input.describe()
        );
        codec.start()?;
        Ok(Self {
            codec,
            width: cfg.width,
            height: cfg.height,
            layout,
            layout_fits: false,
            fps: cfg.fps.max(1),
            last_pts: None,
            frame: 0,
            queued: VecDeque::new(),
            key_pending: false,
            wait: ENCODE_BUDGET,
            last_latency: None,
            dropped: 0,
            cbr,
        })
    }

    /// Codec name, rate mode and input layout, for logs.
    pub(crate) fn describe(&self) -> String {
        format!(
            "{} cbr={} layout={}({:?})",
            self.codec.name, self.cbr, self.layout.kind, self.layout
        )
    }

    /// How long `encode` waits for the frame it just queued (default `ENCODE_BUDGET`).
    pub(crate) fn set_wait(&mut self, wait: Duration) {
        self.wait = wait;
    }

    /// Frame-count pts at the configured rate, like the AV1 encoder: both MediaCodec encoders
    /// budget by timestamps, so with the time each frame was queued the bitrate scaled for a
    /// lower tier (`Sender::encoder_bitrate`, x2 at 15 fps) came out x2 on the wire (self-test
    /// at 15 fps: 4750 kbps for 2500 on the Pixel's hardware encoder; in a call on the software
    /// one, a 250 ms queue, backlog keyframes and the rate down to 100 kbps).
    fn pts(&mut self) -> u64 {
        let pts = self.frame * 1_000_000 / self.fps as u64;
        self.last_pts = Some(pts);
        pts
    }

    /// Takes every ready packet; waits (until `deadline`) while `want` (a pts) has not come out.
    fn drain(
        &mut self,
        out: &mut Vec<Packet>,
        mut want: Option<u64>,
        deadline: Instant,
    ) -> Result<()> {
        loop {
            let now = Instant::now();
            let wait = match want {
                Some(_) if now < deadline => (deadline - now).min(POLL),
                _ => Duration::ZERO,
            };
            match self.codec.dequeue_output(wait) {
                Output::None => {
                    if want.is_none() || Instant::now() >= deadline {
                        return Ok(());
                    }
                }
                Output::Again | Output::FormatChanged => {}
                Output::Error(e) => {
                    return Err(Error::Encode(format!(
                        "{}: dequeueOutputBuffer {e}",
                        self.codec.name
                    )));
                }
                Output::Buffer(b) => {
                    let (pts, flags) = (b.info.presentationTimeUs as u64, b.info.flags);
                    if want.is_some_and(|w| pts >= w) {
                        want = None;
                    }
                    if flags & BUFFER_FLAG_CODEC_CONFIG != 0 {
                        continue;
                    }
                    let data = b
                        .data()
                        .ok_or_else(|| Error::Encode("output buffer bounds".into()))?;
                    if data.is_empty() {
                        continue;
                    }
                    let data = data.to_vec();
                    drop(b);
                    while self.queued.front().is_some_and(|&(p, _, _)| p < pts) {
                        self.queued.pop_front();
                    }
                    let frame = match self.queued.front() {
                        Some(&(p, f, at)) if p == pts => {
                            self.queued.pop_front();
                            self.last_latency = Some(at.elapsed());
                            f
                        }
                        _ => self.frame.saturating_sub(1),
                    };
                    out.push(Packet {
                        data,
                        key: flags & BUFFER_FLAG_KEY_FRAME != 0,
                        frame,
                    });
                }
            }
        }
    }
}

impl VideoEncoder for MediaCodecVp8Encoder {
    fn encode(&mut self, pic: &I420, key: bool) -> Result<Vec<Packet>> {
        let start = Instant::now();
        let want = I420::len(self.width, self.height);
        if pic.width != self.width || pic.height != self.height || pic.data.len() != want {
            return Err(Error::BadFrame(pic.data.len(), want));
        }
        self.key_pending |= key;
        let mut out = Vec::new();
        self.drain(&mut out, None, start)?;
        let frame = self.frame;
        self.frame += 1;
        let Some(idx) = self.codec.dequeue_input(INPUT_WAIT)? else {
            // The codec is behind: drop this frame instead of queueing it (a pending keyframe
            // request waits for the next one).
            self.dropped += 1;
            return Ok(out);
        };
        if !self.layout_fits {
            self.layout_fits = true;
            let cap = self.codec.input_buffer(idx).map_or(0, |b| b.len());
            if let Some(l) = self.layout.or_nv21(self.width, self.height, cap) {
                tracing::info!("vp8 encoder {} input buffer {cap} bytes: layout {l:?} (was {:?})", self.codec.name, self.layout);
                self.layout = l;
            }
        }
        let layout = self.layout;
        let len = match self
            .codec
            .input_buffer(idx)
            .and_then(|buf| layout.write(pic, buf))
        {
            Ok(n) => n,
            Err(e) => {
                // Hand the buffer back empty so the codec does not lose it.
                let _ = self.codec.queue_input(idx, 0, self.last_pts.unwrap_or(0));
                return Err(e);
            }
        };
        if self.key_pending {
            let f = Format::new()?;
            f.set_i32(c"request-sync", 0);
            self.codec.set_params(&f)?;
            self.key_pending = false;
        }
        let pts = self.pts();
        self.codec.queue_input(idx, len, pts)?;
        if self.queued.len() >= 64 {
            self.queued.pop_front();
        }
        self.queued.push_back((pts, frame, Instant::now()));
        self.drain(&mut out, Some(pts), start + self.wait)?;
        Ok(out)
    }

    fn set_bitrate(&mut self, bps: u32) -> Result<()> {
        let f = Format::new()?;
        f.set_i32(c"video-bitrate", clamp_i32(bps));
        self.codec.set_params(&f)
    }
}

// ---- decoder ----

/// VP8 decoder through MediaCodec (the platform's best: hardware where present).
pub struct MediaCodecVp8Decoder {
    codec: Option<Codec>,
    /// Codec to create by name (tests); None = the default for the type.
    name: Option<CString>,
    max_pixels: u32,
    /// Output layout and crop (x, y, w, h), from the last output format.
    out: Option<(Layout, (usize, usize, usize, usize))>,
    pts: u64,
}

impl VideoDecoder for MediaCodecVp8Decoder {
    fn decode(&mut self, packet: &[u8]) -> Result<Vec<I420>> {
        MediaCodecVp8Decoder::decode(self, packet)
    }
}

impl MediaCodecVp8Decoder {
    /// `max_pixels` caps the picture size a peer can make us output.
    pub fn new(max_pixels: u32) -> Result<Self> {
        Self::build(None, max_pixels)
    }

    /// Google's software decoder (`c2.android.vp8.decoder`), for the in-call sheet's
    /// "Decoder: software" on a phone whose default VP8 decoder is the hardware one.
    pub fn new_software(max_pixels: u32) -> Result<Self> {
        let mut last = Error::Config("no software vp8 decoder".into());
        for n in SOFTWARE_VP8_DECODERS {
            match Self::with_name(n, max_pixels) {
                Ok(d) => return Ok(d),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// A specific codec (e.g. the software one, for cross-checks).
    fn with_name(name: &CStr, max_pixels: u32) -> Result<Self> {
        Self::build(Some(name.to_owned()), max_pixels)
    }

    fn build(name: Option<CString>, max_pixels: u32) -> Result<Self> {
        let mut d = Self {
            codec: None,
            name,
            max_pixels,
            out: None,
            pts: 0,
        };
        d.open()?;
        Ok(d)
    }

    fn open(&mut self) -> Result<()> {
        self.codec = None;
        self.out = None;
        let mut c = match &self.name {
            Some(n) => Codec::by_name(n),
            None => Codec::decoder_by_type(MIME),
        }
        .ok_or_else(|| Error::Config("no vp8 decoder".into()))?;
        // Initial size only; the decoder follows the stream (format-changed).
        let (w, h) = if self.max_pixels >= 1280 * 720 {
            (1280, 720)
        } else {
            (640, 360)
        };
        let f = Format::new()?;
        f.set_str(c"mime", MIME)
            .set_i32(c"width", w)
            .set_i32(c"height", h)
            .set_i32(c"color-format", COLOR_YUV420_FLEXIBLE)
            .set_i32(c"priority", 0)
            .set_i32(c"low-latency", 1);
        c.configure(&f, false)?;
        c.start()?;
        tracing::info!("vp8 decoder {}", c.name);
        self.codec = Some(c);
        Ok(())
    }

    pub(crate) fn describe(&self) -> String {
        let name = self.codec.as_ref().map_or("closed", |c| c.name.as_str());
        match &self.out {
            Some((l, crop)) => format!("{name} layout={}({l:?}) crop={crop:?}", l.kind),
            None => format!("{name} layout=unknown"),
        }
    }

    /// Feeds one packet and returns the pictures that came out (normally one). Errors never
    /// panic; after a codec failure the next call starts a fresh codec (needs a keyframe).
    pub(crate) fn decode(&mut self, packet: &[u8]) -> Result<Vec<I420>> {
        if packet.is_empty() || packet.len() > MAX_PACKET {
            return Err(Error::Decode(-22));
        }
        if self.codec.is_none() {
            self.open()?;
        }
        let r = self.feed(packet);
        // -11 busy (packet skipped), -7 packet larger than the input buffer: the codec is fine.
        // Anything else leaves it in an unknown state; rebuild it for the next packet.
        if r.as_ref()
            .is_err_and(|e| !matches!(e, Error::Decode(-11 | -7)))
        {
            self.codec = None;
        }
        r
    }

    fn feed(&mut self, packet: &[u8]) -> Result<Vec<I420>> {
        let start = Instant::now();
        let mut out = Vec::new();
        let dequeue = |c: &Option<Codec>| {
            c.as_ref()
                .ok_or(Error::Decode(-5))?
                .dequeue_input(INPUT_WAIT)
                .map_err(|_| Error::Decode(-5))
        };
        let mut slot = dequeue(&self.codec)?;
        if slot.is_none() {
            // Full: take what is ready and try once more.
            self.drain(&mut out, None, start)?;
            slot = dequeue(&self.codec)?;
        }
        let Some(idx) = slot else {
            return Err(Error::Decode(-11)); // busy: this packet is skipped
        };
        let codec = self.codec.as_mut().ok_or(Error::Decode(-5))?;
        let buf = codec.input_buffer(idx).map_err(|_| Error::Decode(-5))?;
        if buf.len() < packet.len() {
            let _ = codec.queue_input(idx, 0, self.pts);
            return Err(Error::Decode(-7));
        }
        buf[..packet.len()].copy_from_slice(packet);
        self.pts += 33_333;
        codec
            .queue_input(idx, packet.len(), self.pts)
            .map_err(|_| Error::Decode(-5))?;
        let want = self.pts;
        self.drain(&mut out, Some(want), start + DECODE_BUDGET)?;
        Ok(out)
    }

    fn drain(
        &mut self,
        out: &mut Vec<I420>,
        mut want: Option<u64>,
        deadline: Instant,
    ) -> Result<()> {
        let codec = self.codec.as_ref().ok_or(Error::Decode(-5))?;
        loop {
            let now = Instant::now();
            let wait = match want {
                Some(_) if now < deadline => (deadline - now).min(POLL),
                _ => Duration::ZERO,
            };
            match codec.dequeue_output(wait) {
                Output::None => {
                    if want.is_none() || Instant::now() >= deadline {
                        return Ok(());
                    }
                }
                Output::Again => {}
                Output::FormatChanged => self.out = Some(output_layout(codec, self.max_pixels)?),
                Output::Error(e) => return Err(Error::Decode(e as i32)),
                Output::Buffer(b) => {
                    if want.is_some_and(|w| b.info.presentationTimeUs as u64 >= w) {
                        want = None;
                    }
                    if b.info.flags & BUFFER_FLAG_CODEC_CONFIG != 0 || b.info.size <= 0 {
                        continue;
                    }
                    // Output before any format-changed: read the current format.
                    let (layout, (x, y, w, h)) = match self.out {
                        Some(o) => o,
                        None => *self.out.insert(output_layout(codec, self.max_pixels)?),
                    };
                    let data = b.data().ok_or(Error::Decode(-34))?;
                    out.push(
                        layout
                            .read(data, x, y, w, h)
                            .map_err(|_| Error::Decode(-34))?,
                    );
                }
            }
        }
    }
}

/// The decoder's output layout and crop (x, y, w, h); rejects pictures over `max_pixels`.
fn output_layout(codec: &Codec, max_pixels: u32) -> Result<(Layout, (usize, usize, usize, usize))> {
    let f = codec.output_format().ok_or(Error::Decode(-5))?;
    let (fw, fh) = (f.i32(c"width").unwrap_or(0), f.i32(c"height").unwrap_or(0));
    let (l, t, r, b) = f
        .rect(c"crop")
        .or_else(|| {
            Some((
                f.i32(c"crop-left")?,
                f.i32(c"crop-top")?,
                f.i32(c"crop-right")?,
                f.i32(c"crop-bottom")?,
            ))
        })
        .unwrap_or((0, 0, fw - 1, fh - 1));
    let (x, y) = (usize::try_from(l), usize::try_from(t));
    let (w, h) = (usize::try_from(r - l + 1), usize::try_from(b - t + 1));
    let (Ok(x), Ok(y), Ok(w), Ok(h)) = (x, y, w, h) else {
        return Err(Error::Decode(-22));
    };
    if w == 0 || h == 0 || (w as u64) * (h as u64) > max_pixels as u64 {
        return Err(Error::Decode(-75)); // bigger than we accept
    }
    let full_w = (x + w).max(usize::try_from(fw).unwrap_or(0));
    let full_h = (y + h).max(usize::try_from(fh).unwrap_or(0));
    let layout = Layout::of(&f, full_w, full_h)?;
    tracing::info!(
        "vp8 decoder {} output {w}x{h}+{x}+{y} layout={layout:?} format={}",
        codec.name,
        f.describe()
    );
    Ok((layout, (x, y, w, h)))
}

// ---- debug self-test ----

#[cfg(feature = "sim")]
mod selftest {
    use std::sync::Arc;
    use std::sync::mpsc;

    use parking_lot::Mutex;

    use super::*;

    fn psnr(a: &[u8], b: &[u8]) -> f64 {
        let mse = a
            .iter()
            .zip(b)
            .map(|(x, y)| (*x as f64 - *y as f64).powi(2))
            .sum::<f64>()
            / a.len().max(1) as f64;
        if mse == 0.0 {
            99.0
        } else {
            10.0 * (255.0 * 255.0 / mse).log10()
        }
    }

    /// (Y, U, V) PSNR.
    fn psnr3(a: &I420, b: &I420) -> (f64, f64, f64) {
        let ((ay, au, av), (by, bu, bv)) = (a.planes(), b.planes());
        (psnr(ay, by), psnr(au, bu), psnr(av, bv))
    }

    fn pct(v: &mut [f64], q: f64) -> f64 {
        if v.is_empty() {
            return -1.0;
        }
        v.sort_by(|a, b| a.total_cmp(b));
        v[((v.len() - 1) as f64 * q) as usize]
    }

    #[derive(Default)]
    struct Dec {
        /// Per second: (frames, sum Y PSNR, sum U PSNR, sum V PSNR, measured).
        sec: Vec<(u32, f64, f64, f64, u32)>,
        wrong_size: u32,
        errors: u32,
        first_error: Option<String>,
        decoded: u32,
        describe: String,
    }

    const SOURCE_FRAMES: usize = 30;
    /// Packets kept for the software-decoder cross-check (the first seconds).
    const CROSS_CHECK_FRAMES: u64 = 90;

    /// See `mediacodec_vp8_selftest`.
    pub(super) fn run(
        seconds: u32,
        width: u32,
        height: u32,
        wait_ms: i32,
        software: bool,
        fps: u32,
    ) -> std::result::Result<String, String> {
        let (w, h, secs) = (
            width as usize,
            height as usize,
            seconds.clamp(5, 120) as usize,
        );
        let mut lines = Vec::new();
        let mut log = |s: String| {
            tracing::info!("mediacodec_vp8 {s}");
            lines.push(s);
        };
        let probe = Instant::now();
        let hw = hardware_vp8_encoder();
        log(format!(
            "probe hardware_vp8_encoder={hw} name={:?} probe_ms={}",
            hardware_encoder_name(),
            probe.elapsed().as_millis()
        ));
        // Frames go in at `fps` with the bitrate scaled as in a call at that tier
        // (`Sender::encoder_bitrate`: x 30 / fps); `phase_kbps` should still meet the targets.
        let fps = fps.clamp(1, 30);
        let scaled = |bps: u32| bps / fps * 30;
        let (high, low) = (2_500_000u32, 1_000_000u32);
        let cfg = EncoderConfig {
            width: w,
            height: h,
            fps: 30,
            bitrate_bps: scaled(high),
            keyint: 3600 * 30,
            ..Default::default()
        };
        let enc = if software { MediaCodecVp8Encoder::new_software(&cfg) } else { MediaCodecVp8Encoder::new(&cfg) };
        let mut enc = enc.map_err(|e| format!("encoder: {e}"))?;
        if let Ok(ms) = u64::try_from(wait_ms) {
            enc.set_wait(Duration::from_millis(ms));
        }
        log(format!(
            "encoder {} wait_ms={}",
            enc.describe(),
            enc.wait.as_millis()
        ));

        let mut pattern = crate::Pattern::new(w, h);
        let src: Arc<Vec<I420>> =
            Arc::new((0..SOURCE_FRAMES).map(|_| pattern.next_frame()).collect());

        // Decoder on its own thread, like the call's receive side; bounded channel.
        let dec_stats = Arc::new(Mutex::new(Dec {
            sec: vec![Default::default(); secs + 2],
            ..Default::default()
        }));
        let (tx, rx) = mpsc::sync_channel::<(u64, Vec<u8>)>(30);
        let t0 = Instant::now();
        let dec_thread = {
            let (src, stats) = (src.clone(), dec_stats.clone());
            std::thread::Builder::new()
                .name("tl-vp8-selftest-dec".into())
                .spawn(move || {
                    let mut dec = match MediaCodecVp8Decoder::new(1280 * 720) {
                        Ok(d) => d,
                        Err(e) => {
                            stats.lock().first_error = Some(format!("decoder: {e}"));
                            return;
                        }
                    };
                    let mut order: VecDeque<u64> = VecDeque::new();
                    for (frame, data) in rx {
                        order.push_back(frame);
                        let r = dec.decode(&data);
                        let mut st = stats.lock();
                        match r {
                            Ok(pics) => {
                                for p in pics {
                                    let f = order.pop_front().unwrap_or(0);
                                    st.decoded += 1;
                                    let s = (t0.elapsed().as_secs() as usize).min(st.sec.len() - 1);
                                    st.sec[s].0 += 1;
                                    if p.width != w || p.height != h {
                                        st.wrong_size += 1;
                                    } else if st.decoded % 5 == 0 {
                                        let (y, u, v) = psnr3(&p, &src[f as usize % SOURCE_FRAMES]);
                                        let e = &mut st.sec[s];
                                        (e.1, e.2, e.3, e.4) = (e.1 + y, e.2 + u, e.3 + v, e.4 + 1);
                                    }
                                }
                            }
                            Err(e) => {
                                order.pop_back();
                                st.errors += 1;
                                st.first_error.get_or_insert_with(|| format!("{e}"));
                            }
                        }
                    }
                    dec_stats_describe(&stats, &dec);
                })
                .map_err(|e| e.to_string())?
        };

        // Encoder loop on this thread, paced at 30 fps, with the lab's bitrate schedule.
        struct Out {
            at: f64,
            bytes: usize,
            key: bool,
        }
        let mut outs: Vec<Out> = Vec::new();
        let mut enc_ms: Vec<Vec<f64>> = vec![Vec::new(); secs + 1];
        let mut latency_ms: Vec<f64> = Vec::new();
        let mut kept: Vec<(u64, Vec<u8>)> = Vec::new();
        let (mut low_set, mut sync_set, mut high_set) = (false, false, false);
        let mut sync_req: Option<f64> = None;
        let mut sync_resp: Option<f64> = None;
        let mut sent_dropped = 0u32;
        let frame_dur = Duration::from_secs_f64(1.0 / fps as f64);
        let total = secs * fps as usize;
        let mut sec_logged = 0usize;
        let sec_report = |sec: usize,
                          outs: &[Out],
                          enc_ms: &mut [Vec<f64>],
                          dec: &Dec,
                          keys_total: usize,
                          log: &mut dyn FnMut(String)| {
            let bin: Vec<&Out> = outs
                .iter()
                .filter(|o| o.at >= (sec - 1) as f64 && o.at < sec as f64)
                .collect();
            let bytes: usize = bin.iter().map(|o| o.bytes).sum();
            let keys = bin.iter().filter(|o| o.key).count();
            let ms = &mut enc_ms[sec - 1];
            let (p50, p95) = (pct(ms, 0.5), pct(ms, 0.95));
            let d = dec.sec.get(sec - 1).copied().unwrap_or_default();
            let n = d.4.max(1) as f64;
            log(format!(
                "t={sec}s fps={} kbps={} enc_ms_p50={p50:.1} p95={p95:.1} keyframes={keys} (total {keys_total}) dec_fps={} psnr_y={:.1} u={:.1} v={:.1}",
                bin.len(),
                bytes * 8 / 1000,
                d.0,
                d.1 / n,
                d.2 / n,
                d.3 / n
            ));
        };
        for i in 0..total {
            let target = t0 + frame_dur * i as u32;
            let now = Instant::now();
            if target > now {
                std::thread::sleep(target - now);
            }
            let el = t0.elapsed().as_secs_f64();
            if !low_set && el >= 10.0 {
                enc.set_bitrate(scaled(low)).map_err(|e| e.to_string())?;
                low_set = true;
            }
            let mut key = false;
            if !sync_set && el >= 15.0 {
                key = true;
                sync_set = true;
                sync_req = Some(el);
            }
            if !high_set && el >= 20.0 {
                enc.set_bitrate(scaled(high)).map_err(|e| e.to_string())?;
                high_set = true;
            }
            let t = Instant::now();
            let packets = enc
                .encode(&src[i % SOURCE_FRAMES], key)
                .map_err(|e| format!("encode: {e}"))?;
            let took = t.elapsed().as_secs_f64() * 1000.0;
            let at = t0.elapsed().as_secs_f64();
            enc_ms[(at as usize).min(secs)].push(took);
            if !packets.is_empty()
                && let Some(l) = enc.last_latency.take()
            {
                latency_ms.push(l.as_secs_f64() * 1000.0);
            }
            for p in packets {
                if p.key && sync_req.is_some() && sync_resp.is_none() {
                    sync_resp = Some(at);
                }
                outs.push(Out {
                    at,
                    bytes: p.data.len(),
                    key: p.key,
                });
                if p.frame < CROSS_CHECK_FRAMES {
                    kept.push((p.frame, p.data.clone()));
                }
                if tx.try_send((p.frame, p.data)).is_err() {
                    sent_dropped += 1;
                }
            }
            let sec_now = t0.elapsed().as_secs() as usize;
            while sec_logged < sec_now.min(secs) {
                sec_logged += 1;
                let keys_total = outs.iter().filter(|o| o.key).count();
                let dec = dec_stats.lock();
                sec_report(sec_logged, &outs, &mut enc_ms, &dec, keys_total, &mut log);
            }
        }
        drop(tx);
        let _ = dec_thread.join();
        let elapsed = t0.elapsed().as_secs_f64();
        let keys_total = outs.iter().filter(|o| o.key).count();
        {
            let dec = dec_stats.lock();
            while sec_logged < secs {
                sec_logged += 1;
                sec_report(sec_logged, &outs, &mut enc_ms, &dec, keys_total, &mut log);
            }
        }

        // Summary.
        let mut all_ms: Vec<f64> = enc_ms.iter().flatten().copied().collect();
        let phase = |a: f64, b: f64| {
            outs.iter()
                .filter(|o| o.at >= a && o.at < b)
                .map(|o| o.bytes)
                .sum::<usize>() as f64
                * 8.0
                / 1000.0
                / (b - a)
        };
        let end = secs as f64;
        log(format!(
            "SUMMARY size={w}x{h} frames_in={total} packets={} avg_fps={:.2} dropped_by_encoder={} keyframes={keys_total} elapsed_s={elapsed:.1}",
            outs.len(),
            outs.len() as f64 / elapsed,
            enc.dropped
        ));
        log(format!(
            "SUMMARY encode_ms p50={:.1} p95={:.1} max={:.1}",
            pct(&mut all_ms, 0.5),
            pct(&mut all_ms, 0.95),
            all_ms.last().copied().unwrap_or(-1.0)
        ));
        log(format!(
            "SUMMARY queue_to_packet_ms p50={:.1} p95={:.1} (input queued -> packet out)",
            pct(&mut latency_ms, 0.5),
            pct(&mut latency_ms, 0.95)
        ));
        log(format!(
            "SUMMARY phase_kbps 0-10s@2500={:.0} 10-15s@1000={:.0} 15-20s@1000(+sync)={:.0} 20-{secs}s@2500={:.0}",
            phase(0.0, 10.0),
            phase(10.0, 15.0),
            phase(15.0, 20.0_f64.min(end)),
            if end > 20.0 { phase(20.0, end) } else { -1.0 }
        ));
        log(format!(
            "SUMMARY sync_request_at_s={:?} keyframe_latency_ms={}",
            sync_req.map(|s| (s * 10.0).round() / 10.0),
            match (sync_req, sync_resp) {
                (Some(a), Some(b)) => format!("{:.1}", (b - a) * 1000.0),
                _ => "none".into(),
            }
        ));
        {
            let d = dec_stats.lock();
            let (n, y, u, v) = d.sec.iter().fold((0u32, 0.0, 0.0, 0.0), |a, s| {
                (a.0 + s.4, a.1 + s.1, a.2 + s.2, a.3 + s.3)
            });
            let n1 = n.max(1) as f64;
            log(format!(
                "SUMMARY decoder {} decoded={} wrong_size={} errors={} first_error={:?} channel_dropped={sent_dropped} psnr(avg of {n}) y={:.2} u={:.2} v={:.2}",
                d.describe,
                d.decoded,
                d.wrong_size,
                d.errors,
                d.first_error,
                y / n1,
                u / n1,
                v / n1
            ));
        }
        log(cross_check(&kept, &src, w, h));
        Ok(lines.join("\n"))
    }

    fn dec_stats_describe(stats: &Mutex<Dec>, dec: &MediaCodecVp8Decoder) {
        stats.lock().describe = dec.describe();
    }

    /// Decodes the first packets with Google's software VP8 decoder: if its pictures match the
    /// source, the encoder's input layout is right independently of the hardware decoder's.
    fn cross_check(kept: &[(u64, Vec<u8>)], src: &[I420], w: usize, h: usize) -> String {
        let Some(mut dec) = SOFTWARE_VP8_DECODERS
            .iter()
            .find_map(|n| MediaCodecVp8Decoder::with_name(n, 1280 * 720).ok())
        else {
            return "SUMMARY cross_check skipped (no software vp8 decoder)".into();
        };
        let mut order = VecDeque::new();
        let (mut n, mut sum, mut errors) = (0u32, (0.0, 0.0, 0.0), 0u32);
        for (frame, data) in kept {
            order.push_back(*frame);
            // Offline: when the (slow) software decoder is busy, retry instead of skipping.
            let mut r = dec.decode(data);
            for _ in 0..100 {
                if !matches!(r, Err(Error::Decode(-11))) {
                    break;
                }
                r = dec.decode(data);
            }
            match r {
                Ok(pics) => {
                    for p in pics {
                        let f = order.pop_front().unwrap_or(0) as usize;
                        if p.width == w && p.height == h {
                            let (y, u, v) = psnr3(&p, &src[f % SOURCE_FRAMES]);
                            sum = (sum.0 + y, sum.1 + u, sum.2 + v);
                            n += 1;
                        }
                    }
                }
                Err(_) => {
                    order.pop_back();
                    errors += 1;
                }
            }
        }
        let k = n.max(1) as f64;
        format!(
            "SUMMARY cross_check {} frames={n}/{} errors={errors} psnr y={:.2} u={:.2} v={:.2}",
            dec.describe(),
            kept.len(),
            sum.0 / k,
            sum.1 / k,
            sum.2 / k
        )
    }
}

/// Debug self-test of the MediaCodec VP8 path (no camera, no call): synthetic 30 fps frames
/// through `MediaCodecVp8Encoder` for `seconds` with the bitrate schedule 2.5 Mbps, 1.0 Mbps at
/// 10 s, keyframe request at 15 s, 2.5 Mbps at 20 s; packets go to `MediaCodecVp8Decoder` on
/// another thread and decoded pictures are compared with the source. Returns one line per
/// second plus summary lines. `wait_ms` < 0 = the encoder's default wait for its output;
/// `software`: Google's software encoder instead of the default one; `fps` (1-30, 0 = 30): frames
/// at that tier with the call's scaled bitrate. Debug builds run it via DebugReceiver
/// `mediacodec_vp8` (`--ez sw true`, `--ei fps 15`).
#[cfg(feature = "sim")]
pub fn mediacodec_vp8_selftest(seconds: u32, width: u32, height: u32, wait_ms: i32, software: bool, fps: u32) -> std::result::Result<String, String> {
    selftest::run(seconds, width, height, wait_ms, software, if fps == 0 { 30 } else { fps })
}
