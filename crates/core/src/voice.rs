//! Voice messages: Ogg Opus, 16 kHz mono, ~24 kbps (voip), plus the 64-peak waveform the chat
//! bubble draws. The chat core stores the finished file as an encrypted blob; platforms feed PCM
//! in through [`VoiceRecorder`] and play files back through [`VoiceDecoder`].

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use opusic_sys as sys;
use parking_lot::Mutex;

use crate::Error;

pub const VOICE_RATE: u32 = 16_000;
pub const WAVEFORM_BARS: usize = 64;
const BITRATE: i32 = 24_000;
/// 20 ms at 16 kHz.
const FRAME: usize = 320;
const MAX_PACKET: usize = 1275;
const PAGE_PACKETS: usize = 50;
const SERIAL: u32 = 0x7469_6e6c;

fn audio_err(code: i32) -> Error {
    Error::Audio(format!("opus error {code}"))
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct VoiceInfo {
    pub duration_ms: u32,
    /// Exactly 64 peaks, 0-255, normalised so the loudest is 255 (all zero for silence).
    pub waveform: Vec<u8>,
}

// ---------------------------------------------------------------------------------------------
// Ogg framing
// ---------------------------------------------------------------------------------------------

fn crc_table() -> &'static [u32; 256] {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 != 0 { (r << 1) ^ 0x04c1_1db7 } else { r << 1 };
            }
            *e = r;
        }
        t
    })
}

fn crc(data: &[u8]) -> u32 {
    crc_update(0, data)
}

/// One Ogg page holding whole packets (a packet's lacing may span several segments).
fn page(flags: u8, granule: u64, seq: u32, packets: &[Vec<u8>]) -> Vec<u8> {
    let mut lacing = Vec::new();
    for p in packets {
        lacing.extend(std::iter::repeat_n(255u8, p.len() / 255));
        lacing.push((p.len() % 255) as u8);
    }
    let mut out = Vec::with_capacity(27 + lacing.len() + packets.iter().map(Vec::len).sum::<usize>());
    out.extend_from_slice(b"OggS");
    out.push(0);
    out.push(flags);
    out.extend_from_slice(&granule.to_le_bytes());
    out.extend_from_slice(&SERIAL.to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.push(lacing.len() as u8);
    out.extend_from_slice(&lacing);
    for p in packets {
        out.extend_from_slice(p);
    }
    let c = crc(&out);
    out[22..26].copy_from_slice(&c.to_le_bytes());
    out
}

/// Hard limits applied to a voice file before anything is allocated for it. The recorder has no
/// duration cap of its own; playback accepts up to 30 minutes (24 kbps VBR is about 5.4 MB for
/// that), and 16 MiB leaves a generous margin for louder/noisier audio and page overhead.
pub const MAX_VOICE_FILE: u64 = 16 * 1024 * 1024;
/// 30 minutes of 20 ms packets is 90 000; round up.
const MAX_AUDIO_PACKETS: usize = 100_000;
/// RFC 6716: a packet holds at most 120 ms, i.e. six 20 ms frames of at most 1275 bytes each.
const MAX_AUDIO_PACKET_BYTES: usize = MAX_PACKET * 6;
const MAX_HEAD_BYTES: usize = 19;
const MAX_TAGS_BYTES: usize = 64 * 1024;
const MAX_DURATION_SAMPLES_48K: u64 = 30 * 60 * 48_000;
const MAX_PRESKIP: u16 = 9_600;
/// Samples (16 kHz) one packet may decode to: 120 ms.
const MAX_PACKET_SAMPLES: i32 = 1920;

#[derive(Debug)]
struct Ogg {
    packets: Vec<Vec<u8>>,
    /// Granule position of the final (EOS) page, in 48 kHz samples.
    granule: u64,
}

fn bad_ogg(m: &str) -> Error {
    Error::Audio(format!("not a valid Ogg Opus file: {m}"))
}

fn crc_update(c: u32, data: &[u8]) -> u32 {
    let t = crc_table();
    data.iter().fold(c, |c, &b| (c << 8) ^ t[((c >> 24) as u8 ^ b) as usize])
}

/// Parses and validates a single-stream Ogg file, enforcing every limit before it allocates.
/// Packet 0 must be a bare OpusHead page, the last page must carry EOS and a real granule.
fn read_ogg(data: &[u8]) -> Result<Ogg, Error> {
    if data.len() as u64 > MAX_VOICE_FILE {
        return Err(bad_ogg("file too large"));
    }
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut open = false; // previous page ended inside a packet
    let mut serial = 0u32;
    let mut next_seq = 0u32;
    let mut last_granule = 0u64;
    let mut eos = false;
    let mut pos = 0;
    while pos < data.len() {
        if eos {
            return Err(bad_ogg("data after end of stream"));
        }
        let h = data.get(pos..pos + 27).ok_or_else(|| bad_ogg("truncated header"))?;
        if &h[..4] != b"OggS" {
            return Err(bad_ogg("missing capture pattern"));
        }
        if h[4] != 0 {
            return Err(bad_ogg("unsupported version"));
        }
        let flags = h[5];
        if flags & !0x07 != 0 {
            return Err(bad_ogg("bad header flags"));
        }
        let first = pos == 0;
        if (flags & 0x02 != 0) != first {
            return Err(bad_ogg("BOS must be on the first page only"));
        }
        if (flags & 0x01 != 0) != open {
            return Err(bad_ogg("continuation flag does not match the previous page"));
        }
        let granule = u64::from_le_bytes(h[6..14].try_into().unwrap());
        let ser = u32::from_le_bytes(h[14..18].try_into().unwrap());
        let seq = u32::from_le_bytes(h[18..22].try_into().unwrap());
        let want_crc = u32::from_le_bytes(h[22..26].try_into().unwrap());
        if first {
            serial = ser;
        } else if ser != serial {
            return Err(bad_ogg("more than one logical stream"));
        }
        if seq != next_seq {
            return Err(bad_ogg("page sequence gap"));
        }
        next_seq = next_seq.wrapping_add(1);
        let n = h[26] as usize;
        if n == 0 {
            return Err(bad_ogg("empty segment table"));
        }
        let lacing = data.get(pos + 27..pos + 27 + n).ok_or_else(|| bad_ogg("truncated lacing"))?;
        let body_len: usize = lacing.iter().map(|&b| b as usize).sum();
        let body = data.get(pos + 27 + n..pos + 27 + n + body_len).ok_or_else(|| bad_ogg("truncated page"))?;
        let mut c = crc_update(0, &h[..22]);
        c = crc_update(c, &[0; 4]);
        c = crc_update(c, &h[26..27]);
        c = crc_update(c, lacing);
        c = crc_update(c, body);
        if c != want_crc {
            return Err(bad_ogg("page checksum mismatch"));
        }
        if granule != u64::MAX {
            if granule >= 1 << 63 || granule < last_granule {
                return Err(bad_ogg("granule position out of range"));
            }
            if granule > MAX_DURATION_SAMPLES_48K + MAX_PRESKIP as u64 {
                return Err(bad_ogg("recording too long"));
            }
            last_granule = granule;
        }
        if flags & 0x04 != 0 {
            if granule == u64::MAX {
                return Err(bad_ogg("final page has no granule position"));
            }
            eos = true;
        }
        let mut off = 0;
        for &l in lacing {
            let limit = match packets.len() {
                0 => MAX_HEAD_BYTES,
                1 => MAX_TAGS_BYTES,
                _ => MAX_AUDIO_PACKET_BYTES,
            };
            if cur.len() + l as usize > limit {
                return Err(bad_ogg("packet too large"));
            }
            cur.extend_from_slice(&body[off..off + l as usize]);
            off += l as usize;
            open = l == 255;
            if !open {
                if packets.len() >= MAX_AUDIO_PACKETS + 2 {
                    return Err(bad_ogg("too many packets"));
                }
                packets.push(std::mem::take(&mut cur));
            }
        }
        if first && (open || packets.len() != 1) {
            return Err(bad_ogg("OpusHead must be alone on the first page"));
        }
        pos += 27 + n + body_len;
    }
    if open {
        return Err(bad_ogg("unfinished final packet"));
    }
    if !eos {
        return Err(bad_ogg("missing end of stream (truncated file)"));
    }
    Ok(Ogg { packets, granule: last_granule })
}

// ---------------------------------------------------------------------------------------------
// libopus wrappers
// ---------------------------------------------------------------------------------------------

struct Encoder(*mut sys::OpusEncoder);
// SAFETY: a libopus encoder has no thread affinity; we only ever use it through &mut.
unsafe impl Send for Encoder {}

impl Encoder {
    fn new() -> Result<Self, Error> {
        let mut err = 0;
        // SAFETY: plain constructor call, `err` is a valid out pointer.
        let p = unsafe { sys::opus_encoder_create(VOICE_RATE as i32, 1, sys::OPUS_APPLICATION_VOIP, &mut err) };
        if err != 0 || p.is_null() {
            return Err(audio_err(err));
        }
        let e = Encoder(p);
        e.ctl(sys::OPUS_SET_BITRATE_REQUEST, BITRATE)?;
        e.ctl(sys::OPUS_SET_VBR_REQUEST, 1)?;
        e.ctl(sys::OPUS_SET_COMPLEXITY_REQUEST, 10)?;
        e.ctl(sys::OPUS_SET_SIGNAL_REQUEST, sys::OPUS_SIGNAL_VOICE)?;
        Ok(e)
    }

    fn ctl(&self, request: i32, value: i32) -> Result<(), Error> {
        // SAFETY: self.0 is a live encoder; all requests used here take one opus_int32.
        let r = unsafe { sys::opus_encoder_ctl(self.0, request, value) };
        if r < 0 { Err(audio_err(r)) } else { Ok(()) }
    }

    fn lookahead(&self) -> u32 {
        let mut v = 0i32;
        // SAFETY: OPUS_GET_LOOKAHEAD writes one opus_int32 through the pointer.
        unsafe { sys::opus_encoder_ctl(self.0, sys::OPUS_GET_LOOKAHEAD_REQUEST, &mut v as *mut i32) };
        v.max(0) as u32
    }

    fn encode(&mut self, pcm: &[i16; FRAME]) -> Result<Vec<u8>, Error> {
        let mut buf = [0u8; MAX_PACKET];
        // SAFETY: pcm holds FRAME samples; buf has MAX_PACKET bytes.
        let n = unsafe { sys::opus_encode(self.0, pcm.as_ptr(), FRAME as i32, buf.as_mut_ptr(), MAX_PACKET as i32) };
        if n < 0 {
            return Err(audio_err(n));
        }
        Ok(buf[..n as usize].to_vec())
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: created by opus_encoder_create, destroyed once.
        unsafe { sys::opus_encoder_destroy(self.0) }
    }
}

struct Decoder(*mut sys::OpusDecoder);
// SAFETY: see Encoder.
unsafe impl Send for Decoder {}

impl Decoder {
    fn new() -> Result<Self, Error> {
        let mut err = 0;
        // SAFETY: plain constructor call.
        let p = unsafe { sys::opus_decoder_create(VOICE_RATE as i32, 1, &mut err) };
        if err != 0 || p.is_null() {
            return Err(audio_err(err));
        }
        Ok(Decoder(p))
    }

    fn reset(&mut self) {
        // SAFETY: live decoder; OPUS_RESET_STATE takes no argument.
        unsafe { sys::opus_decoder_ctl(self.0, sys::OPUS_RESET_STATE) };
    }

    fn decode(&mut self, data: &[u8]) -> Result<Vec<i16>, Error> {
        let mut buf = [0i16; 1920];
        // SAFETY: buf holds 1920 samples (120 ms at 16 kHz, the Opus maximum); data is valid for its length.
        let n = unsafe { sys::opus_decode(self.0, data.as_ptr(), data.len() as i32, buf.as_mut_ptr(), buf.len() as i32, 0) };
        if n < 0 {
            return Err(audio_err(n));
        }
        Ok(buf[..n as usize].to_vec())
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: created by opus_decoder_create, destroyed once.
        unsafe { sys::opus_decoder_destroy(self.0) }
    }
}

// ---------------------------------------------------------------------------------------------
// Waveform
// ---------------------------------------------------------------------------------------------

/// Reduces per-frame peaks (0..=32768) to exactly 64 bars, each the max of its slice of the
/// recording, scaled so the loudest bar is 255.
fn waveform(peaks: &[u16]) -> Vec<u8> {
    let n = peaks.len();
    let mut bars = [0u16; WAVEFORM_BARS];
    if n > 0 {
        for (i, b) in bars.iter_mut().enumerate() {
            let lo = (i * n / WAVEFORM_BARS).min(n - 1);
            let hi = ((i + 1) * n / WAVEFORM_BARS).max(lo + 1).min(n);
            *b = peaks[lo..hi].iter().copied().max().unwrap_or(0);
        }
    }
    let max = bars.iter().copied().max().unwrap_or(0) as u32;
    if max == 0 {
        return vec![0; WAVEFORM_BARS];
    }
    bars.iter().map(|&b| ((b as u32 * 255 + max / 2) / max) as u8).collect()
}

// ---------------------------------------------------------------------------------------------
// Recorder
// ---------------------------------------------------------------------------------------------

struct Rec {
    path: PathBuf,
    out: BufWriter<File>,
    enc: Encoder,
    preskip: u32,
    pending: Vec<i16>,
    page_packets: Vec<Vec<u8>>,
    seq: u32,
    frames: u64,
    samples: u64,
    peaks: Vec<u16>,
}

impl Rec {
    fn encode_pending(&mut self, flush_all: bool) -> Result<(), Error> {
        let mut used = 0;
        while self.pending.len() - used >= FRAME {
            let frame: [i16; FRAME] = self.pending[used..used + FRAME].try_into().unwrap();
            self.emit(&frame)?;
            used += FRAME;
        }
        self.pending.drain(..used);
        if flush_all && !self.pending.is_empty() {
            let mut frame = [0i16; FRAME];
            frame[..self.pending.len()].copy_from_slice(&self.pending);
            self.pending.clear();
            self.emit(&frame)?;
        }
        Ok(())
    }

    fn emit(&mut self, frame: &[i16; FRAME]) -> Result<(), Error> {
        let p = self.enc.encode(frame)?;
        if self.page_packets.len() >= PAGE_PACKETS {
            self.flush_page(0, self.frames * FRAME as u64 * 3 + self.preskip as u64)?;
        }
        self.page_packets.push(p);
        self.frames += 1;
        Ok(())
    }

    fn flush_page(&mut self, flags: u8, granule: u64) -> Result<(), Error> {
        let bytes = page(flags, granule, self.seq, &self.page_packets);
        self.seq += 1;
        self.page_packets.clear();
        self.out.write_all(&bytes)?;
        Ok(())
    }
}

/// start -> push PCM -> finish (or cancel). Frames are mono i16 at 16 kHz in any chunk size.
#[derive(uniffi::Object)]
pub struct VoiceRecorder {
    rec: Mutex<Option<Rec>>,
}

#[uniffi::export]
impl VoiceRecorder {
    /// Creates (truncates) the Ogg Opus file at `path` and writes its headers.
    #[uniffi::constructor]
    pub fn start(path: String) -> Result<Arc<Self>, Error> {
        let enc = Encoder::new()?;
        let preskip = enc.lookahead() * 3;
        let path = PathBuf::from(path);
        let mut out = BufWriter::new(File::create(&path)?);
        let mut head = b"OpusHead".to_vec();
        head.push(1);
        head.push(1);
        head.extend_from_slice(&(preskip as u16).to_le_bytes());
        head.extend_from_slice(&VOICE_RATE.to_le_bytes());
        head.extend_from_slice(&0i16.to_le_bytes());
        head.push(0);
        let mut tags = b"OpusTags".to_vec();
        let vendor = b"Tinline";
        tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        tags.extend_from_slice(vendor);
        tags.extend_from_slice(&0u32.to_le_bytes());
        out.write_all(&page(0x02, 0, 0, &[head]))?;
        out.write_all(&page(0, 0, 1, &[tags]))?;
        Ok(Arc::new(Self {
            rec: Mutex::new(Some(Rec {
                path,
                out,
                enc,
                preskip,
                pending: Vec::new(),
                page_packets: Vec::new(),
                seq: 2,
                frames: 0,
                samples: 0,
                peaks: Vec::new(),
            })),
        }))
    }

    /// Appends PCM (mono, 16 kHz). Errors if already finished or cancelled.
    pub fn push(&self, pcm: Vec<i16>) -> Result<(), Error> {
        let mut g = self.rec.lock();
        let r = g.as_mut().ok_or(Error::NotFound)?;
        for c in pcm.chunks(FRAME) {
            r.peaks.push(c.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0));
        }
        r.samples += pcm.len() as u64;
        r.pending.extend_from_slice(&pcm);
        r.encode_pending(false)
    }

    /// Milliseconds of audio pushed so far.
    pub fn elapsed_ms(&self) -> u32 {
        self.rec.lock().as_ref().map_or(0, |r| (r.samples / 16) as u32)
    }

    /// Flushes the encoder and closes the file; returns its duration and waveform.
    pub fn finish(&self) -> Result<VoiceInfo, Error> {
        let mut r = self.rec.lock().take().ok_or(Error::NotFound)?;
        // Lookahead worth of silence so the last real sample makes it out of the encoder.
        let tail = vec![0i16; (r.preskip / 3) as usize];
        r.pending.extend_from_slice(&tail);
        r.encode_pending(true)?;
        if r.frames == 0 {
            r.emit(&[0; FRAME])?;
        }
        let granule = r.samples * 3 + r.preskip as u64;
        r.flush_page(0x04, granule)?;
        r.out.flush()?;
        Ok(VoiceInfo { duration_ms: (r.samples / 16) as u32, waveform: waveform(&r.peaks) })
    }

    /// Drops the recording and deletes the file. Harmless after finish.
    pub fn cancel(&self) {
        if let Some(r) = self.rec.lock().take() {
            let path = r.path.clone();
            drop(r);
            let _ = fs::remove_file(path);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------------------------

struct Dec {
    dec: Decoder,
    packets: Vec<Vec<u8>>,
    /// First sample (including pre-skip) of each packet.
    starts: Vec<u64>,
    next: usize,
    preskip: u64,
    total: u64,
    /// Samples still to throw away (pre-skip, or the run-up after a seek).
    skip: u64,
    delivered: u64,
    carry: Vec<i16>,
}

/// Playback side: open(path) -> read PCM chunks -> done when `read` returns empty.
#[derive(uniffi::Object)]
pub struct VoiceDecoder {
    dec: Mutex<Dec>,
}

#[uniffi::export]
impl VoiceDecoder {
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, Error> {
        let mut f = File::open(path)?;
        let mut data = Vec::new();
        Read::by_ref(&mut f).take(MAX_VOICE_FILE + 1).read_to_end(&mut data)?;
        let Ogg { mut packets, granule } = read_ogg(&data)?;
        drop(data);
        let bad = |m: &str| Error::Audio(format!("not a valid Ogg Opus voice message: {m}"));
        let head = packets.remove(0);
        let tags = packets.remove(0);
        if !tags.starts_with(b"OpusTags") {
            return Err(bad("missing OpusTags"));
        }
        if head.len() != MAX_HEAD_BYTES || !head.starts_with(b"OpusHead") {
            return Err(bad("bad OpusHead"));
        }
        if head[8] >> 4 != 0 {
            return Err(bad("unsupported OpusHead version"));
        }
        if head[9] != 1 {
            return Err(bad("only mono is supported"));
        }
        if head[18] != 0 {
            return Err(bad("unsupported channel mapping"));
        }
        let preskip_raw = u16::from_le_bytes([head[10], head[11]]);
        if preskip_raw > MAX_PRESKIP {
            return Err(bad("pre-skip too large"));
        }
        if packets.is_empty() {
            return Err(bad("no audio"));
        }
        let preskip = preskip_raw as u64 / 3;
        let mut starts = Vec::with_capacity(packets.len());
        let mut at = 0u64;
        for p in &packets {
            starts.push(at);
            // SAFETY: p is a valid slice for its length.
            let n = unsafe { sys::opus_packet_get_nb_samples(p.as_ptr(), p.len() as i32, VOICE_RATE as i32) };
            if n <= 0 || n > MAX_PACKET_SAMPLES {
                return Err(bad("bad Opus packet"));
            }
            at += n as u64;
        }
        let total = (granule / 3).saturating_sub(preskip).min(at.saturating_sub(preskip));
        Ok(Arc::new(Self {
            dec: Mutex::new(Dec {
                dec: Decoder::new()?,
                packets,
                starts,
                next: 0,
                preskip,
                total,
                skip: preskip,
                delivered: 0,
                carry: Vec::new(),
            }),
        }))
    }

    pub fn sample_rate(&self) -> u32 {
        VOICE_RATE
    }

    pub fn duration_ms(&self) -> u32 {
        (self.dec.lock().total / 16) as u32
    }

    pub fn position_ms(&self) -> u32 {
        (self.dec.lock().delivered / 16) as u32
    }

    /// Up to `max_samples` mono i16 samples at 16 kHz; empty at the end of the message.
    pub fn read(&self, max_samples: u32) -> Result<Vec<i16>, Error> {
        let mut d = self.dec.lock();
        let want = (max_samples as u64).min(d.total - d.delivered) as usize;
        let mut out = Vec::with_capacity(want);
        while out.len() < want {
            if d.carry.is_empty() {
                if d.next >= d.packets.len() {
                    break;
                }
                let i = d.next;
                d.next += 1;
                let Dec { dec, packets, .. } = &mut *d;
                let mut pcm = dec.decode(&packets[i])?;
                let cut = (d.skip as usize).min(pcm.len());
                d.skip -= cut as u64;
                pcm.drain(..cut);
                d.carry = pcm;
                continue;
            }
            let take = (want - out.len()).min(d.carry.len());
            out.extend(d.carry.drain(..take));
        }
        d.delivered += out.len() as u64;
        Ok(out)
    }

    /// Jumps to `ms` (clamped to the duration); the next read starts there.
    pub fn seek(&self, ms: u32) {
        let mut d = self.dec.lock();
        let target = (ms as u64 * 16).min(d.total);
        let want = target + d.preskip;
        let idx = d.starts.partition_point(|&s| s <= want).saturating_sub(1);
        // Opus is predictive: run in from a few packets earlier, discarding the output.
        let run_in = idx.saturating_sub(4);
        d.dec.reset();
        d.next = run_in;
        d.skip = want - d.starts.get(run_in).copied().unwrap_or(0);
        d.delivered = target;
        d.carry.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn tmp(name: &str) -> String {
        let d = std::env::temp_dir().join(format!("tinline-voice-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        d.join(name).to_string_lossy().into_owned()
    }

    /// `audio::estimate_frequency` assumes 48 kHz input.
    fn freq(pcm: &[i16]) -> f32 {
        audio::estimate_frequency(pcm).unwrap() * VOICE_RATE as f32 / audio::RATE as f32
    }

    fn sine(hz: f32, ms: u32, amp: f32) -> Vec<i16> {
        (0..ms * 16)
            .map(|i| (amp * (2.0 * std::f32::consts::PI * hz * i as f32 / 16_000.0).sin()) as i16)
            .collect()
    }

    fn record(path: &str, pcm: &[i16]) -> VoiceInfo {
        let r = VoiceRecorder::start(path.into()).unwrap();
        for c in pcm.chunks(441) {
            r.push(c.to_vec()).unwrap();
        }
        r.finish().unwrap()
    }

    fn read_all(path: &str) -> Vec<i16> {
        let d = VoiceDecoder::open(path.into()).unwrap();
        let mut all = Vec::new();
        loop {
            let c = d.read(1000).unwrap();
            if c.is_empty() {
                return all;
            }
            all.extend(c);
        }
    }

    #[test]
    fn round_trip_keeps_duration_and_pitch() {
        let p = tmp("rt.opus");
        let info = record(&p, &sine(440.0, 3000, 12000.0));
        assert_eq!(info.duration_ms, 3000);
        let d = VoiceDecoder::open(p.clone()).unwrap();
        assert_eq!(d.duration_ms(), 3000);
        let pcm = read_all(&p);
        assert_eq!(pcm.len(), 48_000);
        let f = freq(&pcm[3200..]);
        assert!((f - 440.0).abs() < 10.0, "got {f} Hz");
        let kbps = fs::metadata(&p).unwrap().len() * 8 / 3 / 1000;
        assert!(kbps < 40, "{kbps} kbps");
    }

    #[test]
    fn waveform_is_64_normalised_bars() {
        let mut pcm = sine(300.0, 1000, 3000.0);
        pcm.extend(sine(300.0, 1000, 12000.0));
        pcm.extend(vec![0; 16_000]);
        let info = record(&tmp("wf.opus"), &pcm);
        assert_eq!(info.waveform.len(), 64);
        assert_eq!(*info.waveform.iter().max().unwrap(), 255);
        assert!(info.waveform[0] < 100 && info.waveform[30] > 200 && info.waveform[63] == 0);
        // Very short and silent recordings still give 64 bars.
        let short = record(&tmp("short.opus"), &sine(300.0, 100, 5000.0));
        assert_eq!(short.waveform.len(), 64);
        assert_eq!(*short.waveform.iter().max().unwrap(), 255);
        let silent = record(&tmp("silent.opus"), &vec![0; 8000]);
        assert_eq!(silent.waveform, vec![0u8; 64]);
    }

    #[test]
    fn cancel_removes_file() {
        let p = tmp("cancel.opus");
        let r = VoiceRecorder::start(p.clone()).unwrap();
        r.push(sine(300.0, 500, 5000.0)).unwrap();
        assert!(Path::new(&p).exists());
        r.cancel();
        assert!(!Path::new(&p).exists());
        assert!(r.push(vec![0; 10]).is_err());
    }

    #[test]
    fn seek_lands_on_the_right_sample() {
        let p = tmp("seek.opus");
        let mut pcm = sine(300.0, 2000, 10000.0);
        pcm.extend(sine(900.0, 2000, 10000.0));
        record(&p, &pcm);
        let d = VoiceDecoder::open(p).unwrap();
        d.seek(3000);
        assert_eq!(d.position_ms(), 3000);
        let got = d.read(8000).unwrap();
        let f = freq(&got);
        assert!((f - 900.0).abs() < 20.0, "got {f} Hz");
        d.seek(0);
        assert!((freq(&d.read(8000).unwrap()[800..]) - 300.0).abs() < 10.0);
        d.seek(999_999);
        assert!(d.read(100).unwrap().is_empty());
    }

    // ---- hostile-input tests -------------------------------------------------------------

    fn head_pkt() -> Vec<u8> {
        let mut h = b"OpusHead".to_vec();
        h.extend_from_slice(&[1, 1]);
        h.extend_from_slice(&312u16.to_le_bytes());
        h.extend_from_slice(&VOICE_RATE.to_le_bytes());
        h.extend_from_slice(&0i16.to_le_bytes());
        h.push(0);
        h
    }

    fn tags_pkt() -> Vec<u8> {
        let mut t = b"OpusTags".to_vec();
        t.extend_from_slice(&0u32.to_le_bytes());
        t.extend_from_slice(&0u32.to_le_bytes());
        t
    }

    fn silence_pkt() -> Vec<u8> {
        let mut e = Encoder::new().unwrap();
        e.encode(&[0; FRAME]).unwrap()
    }

    /// head, tags, then `n` single-packet audio pages (last is EOS), all valid.
    fn build(head: Vec<u8>, tags: Vec<u8>, audio: &[Vec<u8>]) -> Vec<u8> {
        let mut f = page(0x02, 0, 0, &[head]);
        f.extend(page(0, 0, 1, &[tags]));
        for (i, a) in audio.iter().enumerate() {
            let last = i + 1 == audio.len();
            f.extend(page(if last { 0x04 } else { 0 }, (i as u64 + 1) * 960 + 312, 2 + i as u32, std::slice::from_ref(a)));
        }
        f
    }

    fn good() -> Vec<u8> {
        build(head_pkt(), tags_pkt(), &vec![silence_pkt(); 4])
    }

    /// Rejected at the Ogg layer (and therefore by the decoder).
    #[track_caller]
    fn rejects_ogg(f: &[u8]) {
        assert!(read_ogg(f).is_err(), "accepted a bad file");
        rejects(f);
    }

    /// Rejected by `VoiceDecoder::open`.
    #[track_caller]
    fn rejects(f: &[u8]) {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let p = tmp(&format!("bad{}.opus", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        fs::write(&p, f).unwrap();
        assert!(VoiceDecoder::open(p).is_err());
    }

    #[test]
    fn handmade_good_file_opens() {
        let p = tmp("good.opus");
        fs::write(&p, good()).unwrap();
        assert!(VoiceDecoder::open(p).is_ok());
    }

    #[test]
    fn truncation_always_fails() {
        let f = record_bytes();
        for cut in 0..f.len() {
            assert!(read_ogg(&f[..cut]).is_err(), "cut {cut}");
        }
        let mut s = 12345u64;
        for _ in 0..50 {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let cut = (s >> 33) as usize % f.len();
            rejects_ogg(&f[..cut]);
        }
    }

    fn record_bytes() -> Vec<u8> {
        let p = tmp(&format!("bytes{:?}.opus", std::thread::current().id()));
        record(&p, &sine(440.0, 1500, 8000.0));
        fs::read(p).unwrap()
    }

    #[test]
    fn any_flipped_byte_fails() {
        let f = good();
        for i in 0..f.len() {
            let mut g = f.clone();
            g[i] ^= 0x01;
            assert!(read_ogg(&g).is_err(), "flip at {i} accepted");
        }
        let f = record_bytes();
        for i in (0..f.len()).step_by(7) {
            let mut g = f.clone();
            g[i] ^= 0x80;
            assert!(read_ogg(&g).is_err(), "flip at {i} accepted");
        }
    }

    #[test]
    fn framing_violations_rejected() {
        let a = silence_pkt();
        let h = head_pkt();
        let t = tags_pkt();
        let pg = |flags, gr, ser: u32, seq, pk: &[Vec<u8>]| {
            let mut v = page(flags, gr, seq, pk);
            v[14..18].copy_from_slice(&ser.to_le_bytes());
            let mut z = v.clone();
            z[22..26].copy_from_slice(&[0; 4]);
            let c = crc(&z);
            v[22..26].copy_from_slice(&c.to_le_bytes());
            v
        };
        let s = SERIAL;
        let ok = |extra: Vec<u8>| {
            let mut f = pg(0x02, 0, s, 0, std::slice::from_ref(&h));
            f.extend(pg(0, 0, s, 1, std::slice::from_ref(&t)));
            f.extend(extra);
            f
        };
        rejects_ogg(&ok(pg(0x04, 960, s + 1, 2, std::slice::from_ref(&a)))); // wrong serial
        rejects_ogg(&ok(pg(0x04, 960, s, 3, std::slice::from_ref(&a)))); // skipped sequence
        rejects_ogg(&ok([pg(0, 960, s, 2, std::slice::from_ref(&a)), pg(0x06, 1920, s, 3, std::slice::from_ref(&a))].concat())); // BOS twice
        rejects_ogg(&ok(pg(0x05, 960, s, 2, std::slice::from_ref(&a)))); // continued flag with nothing open
        rejects_ogg(&ok(pg(0x04, 960, s, 2, &[vec![1u8; MAX_AUDIO_PACKET_BYTES + 1]]))); // oversized packet
        rejects_ogg(&ok(pg(0x08, 960, s, 2, std::slice::from_ref(&a)))); // unknown flag
        rejects_ogg(&ok([pg(0, 1920, s, 2, std::slice::from_ref(&a)), pg(0x04, 960, s, 3, std::slice::from_ref(&a))].concat())); // granule backwards
        rejects_ogg(&ok([pg(0x04, 960, s, 2, std::slice::from_ref(&a)), pg(0x04, 1920, s, 3, std::slice::from_ref(&a))].concat())); // after EOS
        rejects_ogg(&ok(pg(0, 960, s, 2, std::slice::from_ref(&a)))); // no EOS
        rejects_ogg(&ok(pg(0x04, 1 << 60, s, 2, std::slice::from_ref(&a)))); // absurd duration
        // Open (255-lacing) packet left dangling at EOS, and a missing continuation.
        rejects_ogg(&ok(raw_page(0x04, 960, 2, &[255], &[0u8; 255])));
        // Version byte.
        let mut v = good();
        v[4] = 1;
        rejects_ogg(&v);
        // Empty audio packet.
        rejects(&build(h.clone(), t.clone(), &[vec![]]));
        // No audio at all.
        let mut f = pg(0x02, 0, s, 0, std::slice::from_ref(&h));
        f.extend(pg(0x04, 0, s, 1, std::slice::from_ref(&t)));
        rejects(&f);
    }

    /// A page with explicit lacing, correct CRC.
    fn raw_page(flags: u8, granule: u64, seq: u32, lacing: &[u8], body: &[u8]) -> Vec<u8> {
        let mut v = b"OggS".to_vec();
        v.extend_from_slice(&[0, flags]);
        v.extend_from_slice(&granule.to_le_bytes());
        v.extend_from_slice(&SERIAL.to_le_bytes());
        v.extend_from_slice(&seq.to_le_bytes());
        v.extend_from_slice(&[0; 4]);
        v.push(lacing.len() as u8);
        v.extend_from_slice(lacing);
        v.extend_from_slice(body);
        let c = crc(&v);
        v[22..26].copy_from_slice(&c.to_le_bytes());
        v
    }

    #[test]
    fn continuation_across_pages_is_accepted_and_checked() {
        // A 300-byte tags packet: 255 bytes on one page (open), 45 on the next (continued).
        let mut t = tags_pkt();
        t.resize(300, 0);
        let build = |second_flags: u8, first_lacing: &[u8]| {
            let mut f = page(0x02, 0, 0, &[head_pkt()]);
            f.extend(raw_page(0, u64::MAX, 1, first_lacing, &t[..255]));
            f.extend(raw_page(second_flags, 0, 2, &[45], &t[255..]));
            f.extend(page(0x04, 960 + 312, 3, &[silence_pkt()]));
            f
        };
        assert!(read_ogg(&build(0x01, &[255])).is_ok());
        rejects_ogg(&build(0x00, &[255])); // open packet but next page not marked continued
        rejects_ogg(&build(0x01, &[254])); // marked continued but previous page closed its packet
    }

    #[test]
    fn bad_opus_head_and_tags_rejected() {
        let a = vec![silence_pkt(); 2];
        let mutate = |i: usize, v: u8| {
            let mut h = head_pkt();
            h[i] = v;
            build(h, tags_pkt(), &a)
        };
        rejects(&mutate(0, b'X')); // magic
        rejects(&mutate(8, 0x10)); // major version
        rejects(&mutate(9, 2)); // stereo
        rejects(&mutate(9, 0)); // zero channels
        rejects(&mutate(18, 1)); // mapping family
        let mut h = head_pkt();
        h[10..12].copy_from_slice(&60000u16.to_le_bytes());
        rejects(&build(h, tags_pkt(), &a)); // preskip
        let mut h = head_pkt();
        h.push(0);
        rejects(&build(h, tags_pkt(), &a)); // length
        let mut t = tags_pkt();
        t[0] = b'X';
        rejects(&build(head_pkt(), t, &a)); // OpusTags magic
        rejects(&build(head_pkt(), vec![b'O'; MAX_TAGS_BYTES + 1], &a)); // tags too big
        // Audio packet that is not Opus-decodable length-wise is bounded by the sample cap.
        let mut v = good();
        v.truncate(v.len() - 1);
        rejects(&v);
    }

    #[test]
    fn size_and_count_limits() {
        rejects_ogg(&vec![0u8; MAX_VOICE_FILE as usize + 1]);
        let n = MAX_AUDIO_PACKETS + 1;
        let mut f = page(0x02, 0, 0, &[head_pkt()]);
        f.extend(page(0, 0, 1, &[tags_pkt()]));
        let a = silence_pkt();
        let mut seq = 2;
        let mut left = n;
        while left > 0 {
            let k = left.min(40);
            left -= k;
            let flags = if left == 0 { 0x04 } else { 0 };
            f.extend(page(flags, 960, seq, &vec![a.clone(); k]));
            seq += 1;
        }
        assert!(f.len() as u64 <= MAX_VOICE_FILE, "test file must hit the packet cap, not the size cap");
        assert!(read_ogg(&f).unwrap_err().to_string().contains("too many packets"));
    }

    #[test]
    fn random_mutations_never_panic() {
        let seeds = [good(), record_bytes()];
        let mut s = 0x1234_5678_9abc_def0u64;
        let mut next = move || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 33) as usize
        };
        let p = tmp("fuzz.opus");
        for it in 0..2000 {
            let mut f = seeds[it % 2].clone();
            for _ in 0..1 + next() % 4 {
                match next() % 4 {
                    0 => {
                        let i = next() % f.len();
                        f[i] = next() as u8;
                    }
                    1 => f.truncate(1 + next() % f.len()),
                    2 => {
                        let i = next() % f.len();
                        f.insert(i, next() as u8);
                    }
                    _ => {
                        // Fix up the CRC of a random page so the structural checks run past it.
                        if let Some(o) = (0..f.len().saturating_sub(27)).filter(|&o| &f[o..o + 4] == b"OggS").nth(next() % 3) {
                            let i = o + 4 + next() % 22;
                            f[i] = next() as u8;
                            if let Some(h) = f.get(o..o + 27) {
                                let n = h[26] as usize;
                                if let Some(lac) = f.get(o + 27..o + 27 + n) {
                                    let end = o + 27 + n + lac.iter().map(|&b| b as usize).sum::<usize>();
                                    if end <= f.len() {
                                        f[o + 22..o + 26].copy_from_slice(&[0; 4]);
                                        let c = crc(&f[o..end]);
                                        f[o + 22..o + 26].copy_from_slice(&c.to_le_bytes());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Ok(o) = read_ogg(&f) {
                assert!(o.packets.len() <= MAX_AUDIO_PACKETS + 2);
                assert!(o.packets.iter().all(|p| p.len() <= MAX_AUDIO_PACKET_BYTES.max(MAX_TAGS_BYTES)));
            }
            fs::write(&p, &f).unwrap();
            if let Ok(d) = VoiceDecoder::open(p.clone()) {
                for _ in 0..5 {
                    if d.read(4000).unwrap_or_default().is_empty() {
                        break;
                    }
                }
                d.seek(next() as u32 % 10_000);
                let _ = d.read(100);
            }
        }
    }

    /// Both ffprobe (container, codec, rate, duration) and opusinfo (page CRCs) when installed.
    #[test]
    fn file_is_valid_ogg_opus() {
        let p = tmp("valid.opus");
        record(&p, &sine(500.0, 2500, 9000.0));
        let Ok(out) = std::process::Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "stream=codec_name,sample_rate,channels:format=format_name,duration", "-of", "default=nw=1", &p])
            .output()
        else {
            eprintln!("ffprobe not installed; skipping");
            return;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(text.contains("codec_name=opus") && text.contains("format_name=ogg") && text.contains("channels=1"), "{text}");
        let dur: f32 = text.lines().find_map(|l| l.strip_prefix("duration=")).unwrap().parse().unwrap();
        assert!((dur - 2.5).abs() < 0.05, "{text}");
        // Decode the whole thing with ffmpeg's libopus-independent path: any CRC or sequence error is reported.
        let dec = std::process::Command::new("ffmpeg").args(["-v", "error", "-i", &p, "-f", "null", "-"]).output();
        if let Ok(d) = dec {
            assert!(d.stderr.is_empty(), "{}", String::from_utf8_lossy(&d.stderr));
        }
    }
}
