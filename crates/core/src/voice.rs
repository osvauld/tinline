//! Voice messages: Ogg Opus, 16 kHz mono, ~24 kbps (voip), plus the 64-peak waveform the chat
//! bubble draws. The chat core stores the finished file as an encrypted blob; platforms feed PCM
//! in through [`VoiceRecorder`] and play files back through [`VoiceDecoder`].

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use audio::codec::{self, CodecConfig, Decoder, Encoder};
use parking_lot::Mutex;

use crate::Error;

mod ogg;
use self::ogg::*;

pub const VOICE_RATE: u32 = 16_000;
pub const WAVEFORM_BARS: usize = 64;
const BITRATE: i32 = 24_000;
/// 20 ms at 16 kHz.
const FRAME: usize = 320;
const MAX_PACKET: usize = 1275;
const PAGE_PACKETS: usize = 50;
const SERIAL: u32 = 0x7469_6e6c;

fn audio_err(e: audio::Error) -> Error {
    Error::Audio(e.to_string())
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct VoiceInfo {
    pub duration_ms: u32,
    /// Exactly 64 peaks, 0-255, normalised so the loudest is 255 (all zero for silence).
    pub waveform: Vec<u8>,
}

// ---------------------------------------------------------------------------------------------
// libopus (via audio::codec)
// ---------------------------------------------------------------------------------------------

/// Voice-message codec: 16 kHz mono, 24 kbps VBR, complexity 10, no FEC/DRED, default decoder.
const CODEC: CodecConfig = CodecConfig {
    sample_rate: VOICE_RATE,
    channels: 1,
    frame_ms: 20,
    bitrate: BITRATE,
    fec: false,
    dred: 0,
    complexity: 10,
    decoder_complexity: None,
};

fn encode_packet(enc: &mut Encoder, pcm: &[i16; FRAME]) -> Result<Vec<u8>, Error> {
    let mut buf = [0u8; MAX_PACKET];
    let n = enc.encode(pcm, &mut buf).map_err(audio_err)?;
    Ok(buf[..n].to_vec())
}

fn decode_packet(dec: &mut Decoder, data: &[u8]) -> Result<Vec<i16>, Error> {
    // 1920 samples = 120 ms at 16 kHz, the Opus maximum.
    let mut buf = [0i16; 1920];
    let n = dec.decode(data, &mut buf, false).map_err(audio_err)?;
    Ok(buf[..n].to_vec())
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
        let p = encode_packet(&mut self.enc, frame)?;
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
        let enc = Encoder::new(&CODEC).map_err(audio_err)?;
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
        if packets.len() < 3 {
            return Err(bad("no audio")); // head, tags and one audio packet
        }
        let mut hdr = packets.drain(..2);
        let (head, tags) = (hdr.next().unwrap_or_default(), hdr.next().unwrap_or_default());
        drop(hdr);
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
            let n = codec::packet_samples(p, VOICE_RATE);
            if n <= 0 || n > MAX_PACKET_SAMPLES {
                return Err(bad("bad Opus packet"));
            }
            at += n as u64;
        }
        let total = (granule / 3).saturating_sub(preskip).min(at.saturating_sub(preskip));
        Ok(Arc::new(Self {
            dec: Mutex::new(Dec {
                dec: Decoder::new(&CODEC).map_err(audio_err)?,
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
                let mut pcm = decode_packet(dec, &packets[i])?;
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
mod tests;
