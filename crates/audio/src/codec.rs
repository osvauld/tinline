//! Thin RAII wrappers over libopus (via opusic-sys), parametrised by a [`CodecConfig`].
//! All unsafe Opus FFI lives here: the call path ([`crate::Sender`]/[`crate::Receiver`]) and
//! voice messages (16 kHz Ogg Opus in the core crate) share it.

use std::ptr;

use opusic_sys as sys;

use crate::{Error, Result};

/// Packet-loss percentage advertised to the encoder when FEC/DRED is on. libopus sizes DRED
/// from this knob (at 32 kbps, 10% yields no DRED at all, 20% ~6 ms, 30% ~300 ms), so we
/// advertise 30% even though real loss is typically lower. Costs ~10 bytes/packet.
const LOSS_PERC: i32 = 30;

/// Everything that distinguishes one Opus use from another. Mono, VOIP application, VBR,
/// voice signal hint for every user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodecConfig {
    pub sample_rate: u32,
    pub channels: u8,
    pub frame_ms: u32,
    pub bitrate: i32,
    /// In-band FEC (also sets the advertised loss percentage and turns DTX off).
    pub fec: bool,
    /// DRED history requested from the encoder, in 10 ms units (0 = off).
    pub dred: i32,
    /// Encoder complexity.
    pub complexity: i32,
    /// Decoder complexity, set (and the decoder reset) at construction when `Some`.
    pub decoder_complexity: Option<i32>,
}

impl CodecConfig {
    /// Samples per channel in one frame.
    pub const fn frame_samples(&self) -> usize {
        (self.sample_rate / 1000 * self.frame_ms) as usize
    }
}

fn check(code: i32) -> Result<i32> {
    if code < 0 { Err(Error::Opus(code)) } else { Ok(code) }
}

pub struct Encoder(*mut sys::OpusEncoder);
// SAFETY: a libopus encoder has no thread affinity; we only ever use it through &mut.
unsafe impl Send for Encoder {}

impl Encoder {
    pub fn new(cfg: &CodecConfig) -> Result<Self> {
        let mut err = 0;
        // SAFETY: plain constructor call, `err` is a valid out pointer.
        let p = unsafe {
            sys::opus_encoder_create(cfg.sample_rate as i32, cfg.channels as i32, sys::OPUS_APPLICATION_VOIP, &mut err)
        };
        check(err)?;
        if p.is_null() {
            return Err(Error::Alloc);
        }
        let mut e = Encoder(p);
        e.ctl(sys::OPUS_SET_BITRATE_REQUEST, cfg.bitrate)?;
        e.ctl(sys::OPUS_SET_VBR_REQUEST, 1)?;
        e.ctl(sys::OPUS_SET_COMPLEXITY_REQUEST, cfg.complexity)?;
        e.ctl(sys::OPUS_SET_SIGNAL_REQUEST, sys::OPUS_SIGNAL_VOICE)?;
        if cfg.fec {
            e.ctl(sys::OPUS_SET_INBAND_FEC_REQUEST, 1)?;
            e.ctl(sys::OPUS_SET_PACKET_LOSS_PERC_REQUEST, LOSS_PERC)?;
            e.ctl(sys::OPUS_SET_DTX_REQUEST, 0)?;
        }
        if cfg.dred > 0 {
            e.ctl(sys::OPUS_SET_DRED_DURATION_REQUEST, cfg.dred)?;
        }
        Ok(e)
    }

    fn ctl(&mut self, request: i32, value: i32) -> Result<()> {
        // SAFETY: self.0 is a live encoder; all requests used here take one opus_int32.
        check(unsafe { sys::opus_encoder_ctl(self.0, request, value) }).map(|_| ())
    }

    /// Clear the coding state (settings are kept), so the next packet decodes cleanly on a
    /// freshly reset decoder.
    pub fn reset(&mut self) {
        // SAFETY: live encoder; OPUS_RESET_STATE takes no argument.
        unsafe { sys::opus_encoder_ctl(self.0, sys::OPUS_RESET_STATE) };
    }

    /// Encoder delay in samples at the configured rate.
    pub fn lookahead(&self) -> u32 {
        let mut v = 0i32;
        // SAFETY: OPUS_GET_LOOKAHEAD writes one opus_int32 through the pointer.
        unsafe { sys::opus_encoder_ctl(self.0, sys::OPUS_GET_LOOKAHEAD_REQUEST, &mut v as *mut i32) };
        v.max(0) as u32
    }

    /// Encode one frame (`pcm.len()` samples) into `out`; returns the packet length.
    pub fn encode(&mut self, pcm: &[i16], out: &mut [u8]) -> Result<usize> {
        // SAFETY: pcm and out are valid for the lengths passed.
        let n = unsafe { sys::opus_encode(self.0, pcm.as_ptr(), pcm.len() as i32, out.as_mut_ptr(), out.len() as i32) };
        check(n).map(|n| n as usize)
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: pointer came from opus_encoder_create and is destroyed once.
        unsafe { sys::opus_encoder_destroy(self.0) }
    }
}

pub struct Decoder(*mut sys::OpusDecoder);
// SAFETY: see Encoder.
unsafe impl Send for Decoder {}

impl Decoder {
    pub fn new(cfg: &CodecConfig) -> Result<Self> {
        let mut err = 0;
        // SAFETY: plain constructor call.
        let p = unsafe { sys::opus_decoder_create(cfg.sample_rate as i32, cfg.channels as i32, &mut err) };
        check(err)?;
        if p.is_null() {
            return Err(Error::Alloc);
        }
        let mut d = Decoder(p);
        if let Some(c) = cfg.decoder_complexity {
            // SAFETY: live decoder, one int argument.
            check(unsafe { sys::opus_decoder_ctl(d.0, sys::OPUS_SET_COMPLEXITY_REQUEST, c) })?;
            d.reset();
        }
        Ok(d)
    }

    pub fn reset(&mut self) {
        // SAFETY: live decoder; OPUS_RESET_STATE takes no argument.
        unsafe { sys::opus_decoder_ctl(self.0, sys::OPUS_RESET_STATE) };
    }

    /// Decode a packet into `out` (its length is the frame size) and return the number of
    /// samples written. `data` empty => PLC; `fec` => recover the frame *before* this packet.
    pub fn decode(&mut self, data: &[u8], out: &mut [i16], fec: bool) -> Result<usize> {
        let (p, len) = if data.is_empty() { (ptr::null(), 0) } else { (data.as_ptr(), data.len() as i32) };
        // SAFETY: `out` is valid for its length; `data` for `len` bytes (or null/0 for PLC).
        let n = unsafe { sys::opus_decode(self.0, p, len, out.as_mut_ptr(), out.len() as i32, fec as i32) };
        check(n).map(|n| n as usize)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: created by opus_decoder_create, destroyed once.
        unsafe { sys::opus_decoder_destroy(self.0) }
    }
}

/// Samples per channel in `packet` at `sample_rate`, or a negative libopus error.
pub fn packet_samples(packet: &[u8], sample_rate: u32) -> i32 {
    // SAFETY: packet is a valid slice for its length.
    unsafe { sys::opus_packet_get_nb_samples(packet.as_ptr(), packet.len() as i32, sample_rate as i32) }
}

/// DRED parser state + parsed redundancy of one packet.
pub struct Dred {
    dec: *mut sys::OpusDREDDecoder,
    state: *mut sys::OpusDRED,
    sample_rate: i32,
    /// Sequence number whose payload is currently parsed into `state`, and how many samples
    /// of history it covers.
    pub(crate) parsed: Option<(u32, i32)>,
}
// SAFETY: see Encoder.
unsafe impl Send for Dred {}

impl Dred {
    pub fn new(cfg: &CodecConfig) -> Result<Self> {
        let mut err = 0;
        // SAFETY: constructors with valid out pointers; null checked below.
        let dec = unsafe { sys::opus_dred_decoder_create(&mut err) };
        check(err)?;
        let state = unsafe { sys::opus_dred_alloc(&mut err) };
        if dec.is_null() || state.is_null() {
            // SAFETY: free whichever half was allocated.
            unsafe {
                if !dec.is_null() {
                    sys::opus_dred_decoder_destroy(dec);
                }
                if !state.is_null() {
                    sys::opus_dred_free(state);
                }
            }
            return Err(Error::Alloc);
        }
        check(err)?;
        Ok(Dred { dec, state, sample_rate: cfg.sample_rate as i32, parsed: None })
    }

    /// Samples of history (before the packet's own audio) that `payload` carries, 0 if none.
    pub fn available(&mut self, seq: u32, payload: &[u8]) -> i32 {
        if let Some((s, n)) = self.parsed
            && s == seq {
                return n;
            }
        let mut end = 0;
        // SAFETY: dec/state are live; payload valid for its length.
        let n = unsafe {
            sys::opus_dred_parse(
                self.dec,
                self.state,
                payload.as_ptr(),
                payload.len() as i32,
                self.sample_rate / 2,
                self.sample_rate,
                &mut end,
                0,
            )
        };
        let n = n.max(0);
        self.parsed = Some((seq, n));
        n
    }

    /// Synthesise the frame that ends `offset` samples before the start of the parsed packet;
    /// returns the number of samples written to `out`.
    pub fn decode(&mut self, dec: &mut Decoder, offset: i32, out: &mut [i16]) -> Result<usize> {
        // SAFETY: decoder and DRED state are live; `out` is valid for its length.
        let n = unsafe { sys::opus_decoder_dred_decode(dec.0, self.state, offset, out.as_mut_ptr(), out.len() as i32) };
        check(n).map(|n| n as usize)
    }
}

impl Drop for Dred {
    fn drop(&mut self) {
        // SAFETY: both allocated in `new`, freed once.
        unsafe {
            sys::opus_dred_decoder_destroy(self.dec);
            sys::opus_dred_free(self.state);
        }
    }
}
