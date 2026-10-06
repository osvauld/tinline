//! Voice codec + adaptive jitter buffer for the P2P call path.
//!
//! Pure and synchronous: no threads, no I/O. The caller feeds PCM to [`Sender`], ships the
//! returned datagrams, pushes received datagrams into [`Receiver`] and pulls 20 ms of PCM from
//! it every 20 ms.
//!
//! Wire format: `[seq: u32 BE][level: u8][opus payload]`, 48 kHz mono, 20 ms frames.
//! `level` is the RFC 6464 audio level in -dBov (0 loudest .. 127 silence).

use std::ptr;

use opusic_sys as sys;

pub const RATE: u32 = 48_000;
pub const FRAME: usize = 960;
pub const HEADER: usize = 5;

const FRAME_MS: u64 = 20;
/// Largest datagram payload we encode/accept.
const MAX_PACKET: usize = 1500;
/// Ring size of the jitter buffer in frames (1.28 s).
const SLOTS: usize = 64;
/// A packet this many frames ahead of the playout point triggers a resync.
const RESYNC_AHEAD: i32 = 50;
/// This many consecutive late packets (nothing accepted in between) trigger a resync
/// (peer restarted with a lower seq).
const RESYNC_LATE_STREAK: u32 = 8;
/// Consecutive PLC frames before we give up and play silence.
const MAX_CONCEAL: u32 = 5;
const MIN_TARGET_MS: f32 = 40.0;
const MAX_TARGET_MS: f32 = 200.0;
const INITIAL_TARGET_MS: f32 = 60.0;
/// Start compressing (cross-faded frame skip) above target + this.
const COMPRESS_MS: u64 = 40;
/// Discard without decoding above target + this.
const HARD_MAX_MS: u64 = 120;
/// DRED history requested from the encoder, in 10 ms units.
const DRED_UNITS: i32 = 30;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("opus error {0}")]
    Opus(i32),
    #[error("allocation failed")]
    Alloc,
}

pub type Result<T> = std::result::Result<T, Error>;

fn check(code: i32) -> Result<i32> {
    if code < 0 { Err(Error::Opus(code)) } else { Ok(code) }
}

/// RFC 6464 level of a frame: `-dBov` of its RMS, clamped to 0..=127 (127 = silence).
pub fn level(frame: &[i16]) -> u8 {
    let r = rms(frame) as f64;
    if r < 1.0 {
        return 127;
    }
    let db = 20.0 * (r / 32768.0).log10();
    (-db).round().clamp(0.0, 127.0) as u8
}

/// Root-mean-square amplitude (in i16 units).
pub fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let s: f64 = samples.iter().map(|&x| (x as f64) * (x as f64)).sum();
    (s / samples.len() as f64).sqrt() as f32
}

/// Sine generator for tests / self-test.
pub struct Tone {
    phase: f64,
    step: f64,
    amp: f64,
}

impl Tone {
    pub fn new(freq_hz: f32, amplitude: f32) -> Self {
        Tone {
            phase: 0.0,
            step: std::f64::consts::TAU * freq_hz as f64 / RATE as f64,
            amp: amplitude as f64,
        }
    }

    pub fn next_frame(&mut self, out: &mut [i16; FRAME]) {
        for s in out.iter_mut() {
            *s = (self.phase.sin() * self.amp) as i16;
            self.phase += self.step;
        }
        if self.phase > std::f64::consts::TAU {
            self.phase -= std::f64::consts::TAU;
        }
    }
}

/// Estimate the dominant frequency of a (near) pure tone with interpolated rising zero
/// crossings. `None` if the signal is quiet (RMS < 100) or has too few crossings.
pub fn estimate_frequency(samples: &[i16]) -> Option<f32> {
    if samples.len() < 2 || rms(samples) < 100.0 {
        return None;
    }
    let mut first = 0.0f64;
    let mut last = 0.0f64;
    let mut n = 0u32;
    for i in 1..samples.len() {
        let (a, b) = (samples[i - 1] as f64, samples[i] as f64);
        if a < 0.0 && b >= 0.0 {
            let t = (i - 1) as f64 + (-a) / (b - a);
            if n == 0 {
                first = t;
            }
            last = t;
            n += 1;
        }
    }
    if n < 3 {
        return None;
    }
    Some((RATE as f64 * (n - 1) as f64 / (last - first)) as f32)
}

// ---------------------------------------------------------------------------------------------
// Thin RAII wrappers over libopus (via opusic-sys). All unsafe lives here.
// ---------------------------------------------------------------------------------------------

struct Encoder(*mut sys::OpusEncoder);
// SAFETY: a libopus encoder has no thread affinity; we only ever use it through &mut.
unsafe impl Send for Encoder {}

impl Encoder {
    fn new() -> Result<Self> {
        let mut err = 0;
        // SAFETY: plain constructor call, `err` is a valid out pointer.
        let p = unsafe { sys::opus_encoder_create(RATE as i32, 1, sys::OPUS_APPLICATION_VOIP, &mut err) };
        check(err)?;
        if p.is_null() { Err(Error::Alloc) } else { Ok(Encoder(p)) }
    }

    fn ctl(&mut self, request: i32, value: i32) -> Result<()> {
        // SAFETY: self.0 is a live encoder; all requests used here take one opus_int32.
        check(unsafe { sys::opus_encoder_ctl(self.0, request, value) }).map(|_| ())
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: pointer came from opus_encoder_create and is destroyed once.
        unsafe { sys::opus_encoder_destroy(self.0) }
    }
}

struct Decoder(*mut sys::OpusDecoder);
// SAFETY: see Encoder.
unsafe impl Send for Decoder {}

impl Decoder {
    fn new() -> Result<Self> {
        let mut err = 0;
        // SAFETY: plain constructor call.
        let p = unsafe { sys::opus_decoder_create(RATE as i32, 1, &mut err) };
        check(err)?;
        if p.is_null() {
            return Err(Error::Alloc);
        }
        let mut d = Decoder(p);
        // Complexity >= 5 lets libopus 1.6 use its neural enhancement (OSCE) / deep PLC.
        // SAFETY: live decoder, one int argument.
        check(unsafe { sys::opus_decoder_ctl(d.0, sys::OPUS_SET_COMPLEXITY_REQUEST, 6i32) })?;
        d.reset();
        Ok(d)
    }

    fn reset(&mut self) {
        // SAFETY: live decoder; OPUS_RESET_STATE takes no argument.
        unsafe { sys::opus_decoder_ctl(self.0, sys::OPUS_RESET_STATE) };
    }

    /// Decode a packet (`data` empty => PLC, `fec` => recover the frame *before* this packet).
    fn decode(&mut self, data: &[u8], out: &mut [i16; FRAME], fec: bool) -> Result<()> {
        let (p, len) = if data.is_empty() { (ptr::null(), 0) } else { (data.as_ptr(), data.len() as i32) };
        // SAFETY: `out` holds FRAME samples; `data` is valid for `len` bytes (or null/0 for PLC).
        let n = check(unsafe { sys::opus_decode(self.0, p, len, out.as_mut_ptr(), FRAME as i32, fec as i32) })?;
        out[(n as usize).min(FRAME)..].fill(0);
        Ok(())
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: created by opus_decoder_create, destroyed once.
        unsafe { sys::opus_decoder_destroy(self.0) }
    }
}

/// DRED parser state + parsed redundancy of one packet.
struct Dred {
    dec: *mut sys::OpusDREDDecoder,
    state: *mut sys::OpusDRED,
    /// Sequence number whose payload is currently parsed into `state`, and how many samples
    /// of history it covers.
    parsed: Option<(u32, i32)>,
}
// SAFETY: see Encoder.
unsafe impl Send for Dred {}

impl Dred {
    fn new() -> Result<Self> {
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
        Ok(Dred { dec, state, parsed: None })
    }

    /// Samples of history (before the packet's own audio) that `payload` carries, 0 if none.
    fn available(&mut self, seq: u32, payload: &[u8]) -> i32 {
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
                RATE as i32 / 2,
                RATE as i32,
                &mut end,
                0,
            )
        };
        let n = n.max(0);
        self.parsed = Some((seq, n));
        n
    }

    /// Synthesise the frame that ends `offset` samples before the start of the parsed packet.
    fn decode(&mut self, dec: &mut Decoder, offset: i32, out: &mut [i16; FRAME]) -> Result<()> {
        // SAFETY: decoder and DRED state are live; `out` holds FRAME samples.
        let n = unsafe { sys::opus_decoder_dred_decode(dec.0, self.state, offset, out.as_mut_ptr(), FRAME as i32) };
        check(n)?;
        out[(n as usize).min(FRAME)..].fill(0);
        Ok(())
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

// ---------------------------------------------------------------------------------------------
// Sender
// ---------------------------------------------------------------------------------------------

pub struct Sender {
    enc: Encoder,
    seq: u32,
    scratch: Box<[u8; MAX_PACKET]>,
}

impl Sender {
    /// `bitrate_bps` ~ 32_000 is a good default for voice.
    pub fn new(bitrate_bps: i32) -> Result<Self> {
        let mut enc = Encoder::new()?;
        enc.ctl(sys::OPUS_SET_BITRATE_REQUEST, bitrate_bps)?;
        enc.ctl(sys::OPUS_SET_VBR_REQUEST, 1)?;
        enc.ctl(sys::OPUS_SET_COMPLEXITY_REQUEST, 6)?;
        enc.ctl(sys::OPUS_SET_SIGNAL_REQUEST, sys::OPUS_SIGNAL_VOICE)?;
        enc.ctl(sys::OPUS_SET_INBAND_FEC_REQUEST, 1)?;
        // libopus sizes DRED from this knob (at 32 kbps, 10% yields no DRED at all, 20% ~6 ms,
        // 30% ~300 ms), so we advertise 30% even though real loss is typically lower. Costs
        // ~10 bytes/packet.
        enc.ctl(sys::OPUS_SET_PACKET_LOSS_PERC_REQUEST, 30)?;
        enc.ctl(sys::OPUS_SET_DTX_REQUEST, 0)?;
        enc.ctl(sys::OPUS_SET_DRED_DURATION_REQUEST, DRED_UNITS)?;
        Ok(Sender { enc, seq: 0, scratch: Box::new([0; MAX_PACKET]) })
    }

    /// Encode one frame into a full datagram.
    pub fn encode(&mut self, pcm: &[i16; FRAME]) -> Result<Vec<u8>> {
        let cap = (MAX_PACKET - HEADER) as i32;
        // SAFETY: pcm has FRAME samples; scratch has MAX_PACKET bytes >= HEADER + cap.
        let n = check(unsafe {
            sys::opus_encode(self.enc.0, pcm.as_ptr(), FRAME as i32, self.scratch.as_mut_ptr().add(HEADER), cap)
        })? as usize;
        self.scratch[..4].copy_from_slice(&self.seq.to_be_bytes());
        self.scratch[4] = level(pcm);
        self.seq = self.seq.wrapping_add(1);
        Ok(self.scratch[..HEADER + n].to_vec())
    }
}

// ---------------------------------------------------------------------------------------------
// Receiver
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stats {
    /// Datagrams accepted from the network (including late/duplicate ones).
    pub received: u64,
    /// Frames that were missing at their playout time while later audio was buffered.
    pub lost: u64,
    pub recovered_fec: u64,
    pub recovered_dred: u64,
    /// Frames synthesised by PLC or replaced by silence.
    pub concealed: u64,
    /// Packets that arrived after their playout slot.
    pub late: u64,
    /// Duplicates, plus frames discarded to keep latency bounded.
    pub dropped: u64,
    pub buffered_ms: u32,
    pub target_ms: u32,
    pub last_level: u8,
}

struct Slot {
    seq: u32,
    used: bool,
    len: usize,
    level: u8,
    data: Box<[u8; MAX_PACKET]>,
}

pub struct Receiver {
    dec: Decoder,
    dred: Dred,
    slots: Vec<Slot>,
    started: bool,
    playing: bool,
    next_seq: u32,
    highest: u32,
    late_streak: u32,
    plc_run: u32,
    // jitter estimation (RFC 3550 style, in ms)
    base_seq: u32,
    prev_transit: f64,
    have_transit: bool,
    jitter: f32,
    // adaptive target
    base_ms: f32,
    target_ms: f32,
    last_underrun_ms: u64,
    last_decay_ms: u64,
    stats: Stats,
    tmp: Box<[i16; FRAME]>,
}

impl Receiver {
    pub fn new() -> Result<Self> {
        let slots = (0..SLOTS)
            .map(|_| Slot { seq: 0, used: false, len: 0, level: 127, data: Box::new([0; MAX_PACKET]) })
            .collect();
        Ok(Receiver {
            dec: Decoder::new()?,
            dred: Dred::new()?,
            slots,
            started: false,
            playing: false,
            next_seq: 0,
            highest: 0,
            late_streak: 0,
            plc_run: 0,
            base_seq: 0,
            prev_transit: 0.0,
            have_transit: false,
            jitter: 0.0,
            base_ms: INITIAL_TARGET_MS,
            target_ms: INITIAL_TARGET_MS,
            last_underrun_ms: 0,
            last_decay_ms: 0,
            stats: Stats { target_ms: INITIAL_TARGET_MS as u32, last_level: 127, ..Stats::default() },
            tmp: Box::new([0; FRAME]),
        })
    }

    pub fn stats(&self) -> Stats {
        let mut s = self.stats;
        s.buffered_ms = (self.buffered_frames() as u64 * FRAME_MS) as u32;
        s.target_ms = self.target_ms as u32;
        s
    }

    fn buffered_frames(&self) -> u32 {
        if !self.started {
            return 0;
        }
        let d = self.highest.wrapping_sub(self.next_seq) as i32;
        if d < 0 { 0 } else { d as u32 + 1 }
    }

    fn slot(&self, seq: u32) -> Option<&Slot> {
        let s = &self.slots[seq as usize % SLOTS];
        (s.used && s.seq == seq).then_some(s)
    }

    fn free(&mut self, seq: u32) {
        let s = &mut self.slots[seq as usize % SLOTS];
        if s.used && s.seq == seq {
            s.used = false;
        }
    }

    fn resync(&mut self, seq: u32) {
        for s in &mut self.slots {
            s.used = false;
        }
        self.started = true;
        self.playing = false;
        self.next_seq = seq;
        self.highest = seq;
        self.late_streak = 0;
        self.plc_run = 0;
        self.base_seq = seq;
        self.have_transit = false;
        self.jitter = 0.0;
        self.dred.parsed = None;
        self.dec.reset();
    }

    /// Feed a datagram received from the network at `now_ms` (any monotonic clock).
    pub fn push(&mut self, datagram: &[u8], now_ms: u64) {
        if datagram.len() <= HEADER || datagram.len() > MAX_PACKET {
            return;
        }
        let seq = u32::from_be_bytes([datagram[0], datagram[1], datagram[2], datagram[3]]);
        self.stats.received += 1;
        if !self.started {
            self.resync(seq);
        }
        let mut d = seq.wrapping_sub(self.next_seq) as i32;
        if d < 0 {
            self.stats.late += 1;
            self.late_streak += 1;
            if self.late_streak < RESYNC_LATE_STREAK {
                return;
            }
            self.resync(seq);
            d = 0;
        } else if d >= RESYNC_AHEAD {
            self.resync(seq);
            d = 0;
        }
        let _ = d;
        self.late_streak = 0;

        // Interarrival jitter, measured against the sender's 20 ms cadence.
        let rel = seq.wrapping_sub(self.base_seq) as i32 as f64;
        let transit = now_ms as f64 - rel * FRAME_MS as f64;
        if self.have_transit {
            let dd = (transit - self.prev_transit).abs() as f32;
            self.jitter += (dd - self.jitter) / 16.0;
        }
        self.prev_transit = transit;
        self.have_transit = true;

        let slot = &mut self.slots[seq as usize % SLOTS];
        if slot.used && slot.seq == seq {
            self.stats.dropped += 1; // duplicate
            return;
        }
        slot.used = true;
        slot.seq = seq;
        slot.len = datagram.len() - HEADER;
        slot.level = datagram[4];
        slot.data[..slot.len].copy_from_slice(&datagram[HEADER..]);
        if (seq.wrapping_sub(self.highest) as i32) > 0 || self.buffered_frames() == 0 {
            self.highest = seq;
        }
    }

    fn update_target(&mut self, now_ms: u64) {
        if now_ms.saturating_sub(self.last_underrun_ms) > 3000 && now_ms.saturating_sub(self.last_decay_ms) > 3000 {
            self.base_ms = (self.base_ms - 5.0).max(MIN_TARGET_MS);
            self.last_decay_ms = now_ms;
        }
        let wanted = 3.0 * self.jitter + 20.0;
        self.target_ms = wanted.max(self.base_ms).clamp(MIN_TARGET_MS, MAX_TARGET_MS);
    }

    /// Lowest buffered seq at or after `next_seq`.
    fn first_buffered(&self) -> Option<u32> {
        (0..SLOTS as u32).map(|i| self.next_seq.wrapping_add(i)).find(|&s| self.slot(s).is_some())
    }

    /// Fill `out` with the next 20 ms of audio. Always writes the full frame.
    pub fn pull(&mut self, out: &mut [i16; FRAME], now_ms: u64) {
        out.fill(0);
        if !self.started {
            return;
        }
        self.update_target(now_ms);
        let target_frames_ms = self.target_ms as u64;

        if !self.playing {
            if self.buffered_frames() as u64 * FRAME_MS >= target_frames_ms
                && let Some(s) = self.first_buffered() {
                    self.next_seq = s;
                    self.playing = true;
                    self.plc_run = 0;
                }
            if !self.playing {
                return;
            }
        }

        // Latency control: hard discard far above target.
        while self.buffered_frames() as u64 * FRAME_MS > target_frames_ms + HARD_MAX_MS {
            self.free(self.next_seq);
            self.next_seq = self.next_seq.wrapping_add(1);
            self.stats.dropped += 1;
        }

        let seq = self.next_seq;
        if let Some(slot) = self.slot(seq) {
            let (len, lvl) = (slot.len, slot.level);
            let mut ok = {
                let data = &self.slots[seq as usize % SLOTS].data[..len];
                self.dec.decode(data, out, false).is_ok()
            };
            self.stats.last_level = lvl;
            if !ok {
                ok = self.dec.decode(&[], out, false).is_ok();
                self.stats.concealed += 1;
            } else {
                self.plc_run = 0;
            }
            let _ = ok;
            self.free(seq);
            self.next_seq = seq.wrapping_add(1);
            // Compress: skip one frame with a cross-fade when we are too far ahead.
            if self.buffered_frames() as u64 * FRAME_MS > target_frames_ms + COMPRESS_MS {
                let n = self.next_seq;
                if let Some(s2) = self.slot(n) {
                    let len2 = s2.len;
                    let data = &self.slots[n as usize % SLOTS].data[..len2];
                    if self.dec.decode(data, &mut self.tmp, false).is_ok() {
                        for (i, (o, b)) in out.iter_mut().zip(self.tmp.iter()).enumerate() {
                            let w = i as f32 / FRAME as f32;
                            *o = (*o as f32 * (1.0 - w) + *b as f32 * w) as i16;
                        }
                        self.stats.dropped += 1;
                        self.free(n);
                        self.next_seq = n.wrapping_add(1);
                    }
                }
            }
            return;
        }

        if self.buffered_frames() > 0 && (self.highest.wrapping_sub(seq) as i32) > 0 {
            // Missing frame with newer audio buffered: recover it.
            self.stats.lost += 1;
            self.conceal_lost(seq, out);
            self.next_seq = seq.wrapping_add(1);
            return;
        }

        // Underrun: nothing at or after next_seq. Conceal, but keep waiting for `next_seq`.
        if self.plc_run == 0 {
            self.base_ms = (self.base_ms + 20.0).min(MAX_TARGET_MS);
            self.last_underrun_ms = now_ms;
        }
        self.stats.concealed += 1;
        if self.plc_run < MAX_CONCEAL {
            self.plc_run += 1;
            if self.dec.decode(&[], out, false).is_err() {
                out.fill(0);
            }
        } else {
            // Give up: silence, fresh decoder, rebuffer before resuming.
            if self.plc_run == MAX_CONCEAL {
                self.plc_run += 1;
                self.dec.reset();
            }
            self.playing = false;
        }
    }

    /// Recover frame `seq` (missing) using FEC, then DRED, then PLC, then silence.
    fn conceal_lost(&mut self, seq: u32, out: &mut [i16; FRAME]) {
        let next = seq.wrapping_add(1);
        // 1. In-band FEC from the immediately following packet.
        if let Some(s) = self.slot(next) {
            let len = s.len;
            let data = &self.slots[next as usize % SLOTS].data[..len];
            if self.dec.decode(data, out, true).is_ok() {
                self.stats.recovered_fec += 1;
                self.plc_run = 0;
                return;
            }
        }
        // 2. DRED from the nearest later packet.
        let mut n = next;
        while self.slot(n).is_none() && (n.wrapping_sub(self.highest) as i32) < 0 {
            n = n.wrapping_add(1);
        }
        if let Some(s) = self.slot(n) {
            let len = s.len;
            let offset = n.wrapping_sub(seq) as i32 * FRAME as i32;
            let data = &self.slots[n as usize % SLOTS].data[..len];
            if self.dred.available(n, data) >= offset && self.dred.decode(&mut self.dec, offset, out).is_ok() {
                self.stats.recovered_dred += 1;
                self.plc_run = 0;
                return;
            }
        }
        // 3. PLC for a bounded run, then silence.
        self.stats.concealed += 1;
        if self.plc_run < MAX_CONCEAL {
            self.plc_run += 1;
            if self.dec.decode(&[], out, false).is_ok() {
                return;
            }
        } else if self.plc_run == MAX_CONCEAL {
            self.plc_run += 1;
            self.dec.reset();
        }
        out.fill(0);
    }
}

#[cfg(test)]
mod tests;
