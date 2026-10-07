//! Voice messages: Ogg Opus, 16 kHz mono, ~24 kbps (voip), plus the 64-peak waveform the chat
//! bubble draws. The chat core stores the finished file as an encrypted blob; platforms feed PCM
//! in through [`VoiceRecorder`] and play files back through [`VoiceDecoder`].

use std::fs::{self, File};
use std::io::{BufWriter, Write};
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
    let t = crc_table();
    data.iter().fold(0u32, |c, &b| (c << 8) ^ t[((c >> 24) as u8 ^ b) as usize])
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

/// Reads every packet of a single-stream Ogg file (pages may split packets across pages).
/// Returns the packets and the granule position of the last page.
fn read_ogg(data: &[u8]) -> Result<(Vec<Vec<u8>>, u64), Error> {
    let bad = |m: &str| Error::Audio(format!("not a valid Ogg file: {m}"));
    let mut packets = Vec::new();
    let mut cur = Vec::new();
    let mut last_granule = 0u64;
    let mut pos = 0;
    while pos < data.len() {
        let h = data.get(pos..pos + 27).ok_or_else(|| bad("truncated header"))?;
        if &h[..4] != b"OggS" {
            return Err(bad("missing capture pattern"));
        }
        let n = h[26] as usize;
        let lacing = data.get(pos + 27..pos + 27 + n).ok_or_else(|| bad("truncated lacing"))?;
        let body_len: usize = lacing.iter().map(|&b| b as usize).sum();
        let body = data.get(pos + 27 + n..pos + 27 + n + body_len).ok_or_else(|| bad("truncated page"))?;
        let granule = u64::from_le_bytes(h[6..14].try_into().unwrap());
        if granule != u64::MAX {
            last_granule = granule;
        }
        let mut off = 0;
        for &l in lacing {
            cur.extend_from_slice(&body[off..off + l as usize]);
            off += l as usize;
            if l < 255 {
                packets.push(std::mem::take(&mut cur));
            }
        }
        pos += 27 + n + body_len;
    }
    Ok((packets, last_granule))
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
    let mut bars = vec![0u16; WAVEFORM_BARS];
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
        let (mut packets, granule) = read_ogg(&fs::read(path)?)?;
        if packets.len() < 2 || !packets[0].starts_with(b"OpusHead") || !packets[1].starts_with(b"OpusTags") {
            return Err(Error::Audio("not an Ogg Opus voice message".into()));
        }
        let head = packets.remove(0);
        packets.remove(0);
        if head.len() < 19 {
            return Err(Error::Audio("short OpusHead".into()));
        }
        let preskip = u16::from_le_bytes([head[10], head[11]]) as u64 / 3;
        let mut starts = Vec::with_capacity(packets.len());
        let mut at = 0u64;
        for p in &packets {
            starts.push(at);
            // SAFETY: p is a valid slice for its length.
            let n = unsafe { sys::opus_packet_get_nb_samples(p.as_ptr(), p.len() as i32, VOICE_RATE as i32) };
            if n < 0 {
                return Err(audio_err(n));
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
