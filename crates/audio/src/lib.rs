//! Voice codec + adaptive jitter buffer for the P2P call path.
//!
//! Pure and synchronous: no threads, no I/O. The caller feeds PCM to [`Sender`], ships the
//! returned datagrams, pushes received datagrams into [`Receiver`] and pulls 20 ms of PCM from
//! it every 20 ms.
//!
//! Wire format: `[seq: u32 BE][level: u8][opus payload]`, 48 kHz mono, 20 ms frames.
//! `level` is the RFC 6464 audio level in -dBov (0 loudest .. 127 silence).

pub mod codec;

use codec::{CodecConfig, Decoder, Dred, Encoder};

pub const RATE: u32 = 48_000;
pub const FRAME: usize = 960;
pub const HEADER: usize = 5;

/// Frame length in ms (also the `frame_ms` of [`CALL_CODEC`]).
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
/// Group calls: RFC 6464 level (-dBov) below which a frame counts as speech, for the sender
/// silence gate and the call host's speaker selection (group-call-spec §1.3, §1.4).
pub const SPEECH_LEVEL: u8 = 50;
/// Group calls: the silence gate closes after this much audio at or above `SPEECH_LEVEL`.
const GATE_HANG_MS: u64 = 300;
/// Group calls: a playing slot with nothing buffered and no datagram for this long ends its
/// talk spurt as if the host had signalled it inactive (fallback for a late `Active`).
pub const SLOT_FALLBACK_MS: u64 = 200;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("opus error {0}")]
    Opus(i32),
    #[error("allocation failed")]
    Alloc,
}

pub type Result<T> = std::result::Result<T, Error>;

/// Call-path codec settings: 48 kHz mono, 20 ms frames, FEC + DRED. `bitrate` is replaced by
/// the caller of [`Sender::new`]; the decoder runs deep PLC on (5) with OSCE (6+) off. OSCE is
/// tuned for 6-12 kbps speech; at our 48 kbps it buys little and reshapes other signals (a
/// 770 Hz tone came out at -6 dB, others +-16%). At 5 tones are flat within 1% and FEC/DRED
/// recovery is unchanged (`cargo run -p audio --example level` shows the levels).
pub const CALL_CODEC: CodecConfig = CodecConfig {
    sample_rate: RATE,
    channels: 1,
    frame_ms: FRAME_MS as u32,
    bitrate: 32_000,
    fec: true,
    dred: DRED_UNITS,
    complexity: 6,
    decoder_complexity: Some(5),
};

/// Decode a packet (`data` empty => PLC, `fec` => the frame *before* this packet) into a full
/// frame, zero-filling whatever the decoder did not write.
fn dec_frame(dec: &mut Decoder, data: &[u8], out: &mut [i16; FRAME], fec: bool) -> Result<()> {
    let n = dec.decode(data, out, fec)?;
    out[n.min(FRAME)..].fill(0);
    Ok(())
}

/// DRED-synthesise the frame ending `offset` samples before the parsed packet, zero-filled.
fn dred_frame(dred: &mut Dred, dec: &mut Decoder, offset: i32, out: &mut [i16; FRAME]) -> Result<()> {
    let n = dred.decode(dec, offset, out)?;
    out[n.min(FRAME)..].fill(0);
    Ok(())
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

/// Mix the slot frames of a group call into `out` (group-call-spec §1.4).
///
/// Sums in i32. Sums within ±`MIX_KNEE` pass through exactly; above the knee a stateless
/// tanh soft knee maps the excess into the remaining headroom, so the output is monotonic in
/// the sum, never wraps and never exceeds i16. A frame shorter than `out` counts as silence
/// past its end; silent slots leave the result unchanged. Pure; no state between frames.
pub use mixing::{mix, mix_sums};
mod mixing;
#[cfg(test)]
mod golden_tests;

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
// Sender
// ---------------------------------------------------------------------------------------------

pub struct Sender {
    enc: Encoder,
    seq: u32,
    scratch: Box<[u8; MAX_PACKET]>,
    /// Silence gate state (`encode_group` only).
    quiet_ms: u64,
    gated: bool,
}

/// Result of [`Sender::encode_group`].
#[derive(Debug, Clone, PartialEq)]
pub enum GroupFrame {
    /// A normal datagram, as from [`Sender::encode`].
    Opus(Vec<u8>),
    /// The silence gate is closed: nothing was encoded, `seq` still advanced by one. The call
    /// layer sends its level-only datagram (`[0x03][level]`) every 500 ms from these.
    Gated { level: u8 },
}

impl Sender {
    /// `bitrate_bps` ~ 32_000 is a good default for voice.
    pub fn new(bitrate_bps: i32) -> Result<Self> {
        let enc = Encoder::new(&CodecConfig { bitrate: bitrate_bps, ..CALL_CODEC })?;
        Ok(Sender { enc, seq: 0, scratch: Box::new([0; MAX_PACKET]), quiet_ms: 0, gated: false })
    }

    /// Group calls only (group-call-spec §1.4): [`encode`](Self::encode) behind a silence
    /// gate. The gate closes after `GATE_HANG_MS` (300 ms) of frames at or above
    /// [`SPEECH_LEVEL`] and opens on the first frame below it. While closed nothing is
    /// encoded, but `seq` keeps counting one per call (one per 20 ms of wall time), so the
    /// resumed stream shows a `seq` jump equal to the pause. On reopening the encoder state is
    /// reset, matching the receiver's fresh decoder. 1:1 calls keep using `encode` (no gate).
    pub fn encode_group(&mut self, pcm: &[i16; FRAME]) -> Result<GroupFrame> {
        let lvl = level(pcm);
        if lvl < SPEECH_LEVEL {
            self.quiet_ms = 0;
            if self.gated {
                self.gated = false;
                self.enc.reset();
            }
        } else {
            self.quiet_ms += FRAME_MS;
            if self.quiet_ms > GATE_HANG_MS {
                self.gated = true;
            }
        }
        if self.gated {
            self.seq = self.seq.wrapping_add(1);
            return Ok(GroupFrame::Gated { level: lvl });
        }
        self.encode(pcm).map(GroupFrame::Opus)
    }

    /// Encode one frame into a full datagram.
    pub fn encode(&mut self, pcm: &[i16; FRAME]) -> Result<Vec<u8>> {
        let n = self.enc.encode(pcm, &mut self.scratch[HEADER..])?;
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
    group: Group,
}

/// Idle/resume state of a group-call slot receiver (group-call-spec §1.4, G6-D1). Unused
/// (`on == false`) for 1:1 calls, which keep the continuous-stream behaviour.
struct Group {
    /// The idle/resume path is enabled (`new_group` or any `set_active` call).
    on: bool,
    /// The call host lists this slot as forwarded.
    active: bool,
    /// Talk spurt ended: decoder and buffer are reset, pulls return silence without any
    /// accounting, the next datagram newer than `last_seq` starts a fresh spurt.
    idle: bool,
    /// Highest `seq` of the previous spurt (stragglers at or below it are ignored while idle).
    last_seq: Option<u32>,
    /// Network jitter learned before the spurt ended; restored on resume.
    jitter: f32,
    /// Fade in the next frame decoded from a packet (decoder state was reset).
    fade_in: bool,
    /// Arrival time of the last accepted datagram.
    last_push_ms: u64,
    /// An underrun began at this time; it counts towards the target only if audio resumes
    /// before the spurt ends (otherwise it was a forwarding gap, not network jitter).
    underrun_at: Option<u64>,
    /// A faded-out frame to emit on the next pull (spurt cut by `set_active(true)`).
    tail: Option<Box<[i16; FRAME]>>,
}

fn fade(out: &mut [i16; FRAME], rising: bool) {
    for (i, s) in out.iter_mut().enumerate() {
        let w = (i as f32 + 0.5) / FRAME as f32;
        let g = if rising { w } else { 1.0 - w };
        *s = (*s as f32 * g) as i16;
    }
}

impl Receiver {
    pub fn new() -> Result<Self> {
        let slots = (0..SLOTS)
            .map(|_| Slot { seq: 0, used: false, len: 0, level: 127, data: Box::new([0; MAX_PACKET]) })
            .collect();
        Ok(Receiver {
            dec: Decoder::new(&CALL_CODEC)?,
            dred: Dred::new(&CALL_CODEC)?,
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
            group: Group {
                on: false,
                active: true,
                idle: false,
                last_seq: None,
                jitter: 0.0,
                fade_in: false,
                last_push_ms: 0,
                underrun_at: None,
                tail: None,
            },
        })
    }

    /// A receiver for one group-call slot: the idle/resume path of group-call-spec §1.4 is on
    /// from the start (initially active). See [`set_active`](Self::set_active).
    pub fn new_group() -> Result<Self> {
        let mut r = Self::new()?;
        r.group.on = true;
        Ok(r)
    }

    /// Group calls: the call host's `Active` signal for this slot (G6-D1). Enables the
    /// idle/resume path; never call it for a 1:1 receiver.
    ///
    /// - `false`: audio already buffered is played out, then one faded-out PLC frame, then
    ///   the slot goes idle (silence, no underrun or jitter accounting) and is reset.
    /// - `true`: the next datagram starts a fresh spurt at the last learned network-jitter
    ///   target; its first frame fades in. If the previous spurt is still playing out, it is
    ///   cut with a faded-out frame and its remaining buffer dropped.
    ///
    /// Without a signal the same end-of-spurt path runs once a playing slot has had nothing
    /// to play and no datagram for [`SLOT_FALLBACK_MS`]; a datagram arriving while idle
    /// resumes the slot. Gaps shorter than that are indistinguishable from network loss.
    pub fn set_active(&mut self, active: bool) {
        self.group.on = true;
        if active && !self.group.active && self.started && !self.group.idle {
            let mut tail = Box::new([0i16; FRAME]);
            self.end_spurt(&mut tail);
            self.group.tail = Some(tail);
        }
        self.group.active = active;
    }

    /// Whether this group slot is idle (talk spurt ended, nothing to play).
    pub fn is_idle(&self) -> bool {
        self.group.idle
    }

    /// End the current talk spurt: `out` gets one faded-out PLC frame if audio was still
    /// audible (silence otherwise), then the slot goes idle and is reset.
    fn end_spurt(&mut self, out: &mut [i16; FRAME]) {
        out.fill(0);
        if self.playing && self.plc_run < MAX_CONCEAL {
            if dec_frame(&mut self.dec, &[], out, false).is_ok() {
                fade(out, false);
            } else {
                out.fill(0);
            }
            self.stats.concealed += 1;
        }
        self.group.jitter = self.jitter;
        self.group.last_seq = self.started.then_some(self.highest);
        for s in &mut self.slots {
            s.used = false;
        }
        self.started = false;
        self.playing = false;
        self.late_streak = 0;
        self.plc_run = 0;
        self.have_transit = false;
        self.dred.parsed = None;
        self.dec.reset();
        self.group.idle = true;
        self.group.fade_in = true;
        self.group.underrun_at = None;
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
        if self.group.on {
            if self.group.idle {
                // Straggler from the spurt that just ended.
                if let Some(l) = self.group.last_seq
                    && (0..SLOTS as i32).contains(&(l.wrapping_sub(seq) as i32))
                {
                    return;
                }
                // A forwarded datagram means the host lists the slot again.
                self.group.idle = false;
                self.group.active = true;
                self.resync(seq);
                self.jitter = self.group.jitter;
            }
            self.group.last_push_ms = now_ms;
            if let Some(t) = self.group.underrun_at.take() {
                // Audio came back before the spurt ended: a real network underrun.
                self.base_ms = (self.base_ms + 20.0).min(MAX_TARGET_MS);
                self.last_underrun_ms = t;
            }
        }
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
        if self.group.on {
            if let Some(t) = self.group.tail.take() {
                out.copy_from_slice(&t[..]);
                return;
            }
            if self.started
                && self.buffered_frames() == 0
                && (!self.group.active || now_ms.saturating_sub(self.group.last_push_ms) >= SLOT_FALLBACK_MS)
            {
                self.end_spurt(out);
                return;
            }
            if self.group.idle {
                return;
            }
            self.pull_group(out, now_ms);
            return;
        }
        self.pull_inner(out, now_ms);
    }

    fn pull_group(&mut self, out: &mut [i16; FRAME], now_ms: u64) {
        let before = self.next_seq;
        let plc_before = self.plc_run;
        self.pull_inner(out, now_ms);
        if self.next_seq != before && self.group.fade_in {
            // First frame from a packet after a decoder reset.
            fade(out, true);
            self.group.fade_in = false;
        } else if plc_before < MAX_CONCEAL && self.plc_run == MAX_CONCEAL {
            // Last PLC frame before concealment gives up: fade to the silence that follows.
            fade(out, false);
        } else if plc_before == MAX_CONCEAL && self.plc_run > MAX_CONCEAL {
            self.group.fade_in = true; // decoder was reset
        }
    }

    fn pull_inner(&mut self, out: &mut [i16; FRAME], now_ms: u64) {
        if !self.started {
            return;
        }
        self.update_target(now_ms);
        let target_frames_ms = self.target_ms as u64;

        if !self.playing {
            // A slot the host dropped plays out what it has without waiting for the target.
            let drain = self.group.on && !self.group.active && self.buffered_frames() > 0;
            if (drain || self.buffered_frames() as u64 * FRAME_MS >= target_frames_ms)
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
                dec_frame(&mut self.dec, data, out, false).is_ok()
            };
            self.stats.last_level = lvl;
            if !ok {
                ok = dec_frame(&mut self.dec, &[], out, false).is_ok();
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
                    if dec_frame(&mut self.dec, data, &mut self.tmp, false).is_ok() {
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
            if self.group.on {
                self.group.underrun_at = Some(now_ms); // counted in `push` if audio resumes
            } else {
                self.base_ms = (self.base_ms + 20.0).min(MAX_TARGET_MS);
                self.last_underrun_ms = now_ms;
            }
        }
        self.stats.concealed += 1;
        if self.plc_run < MAX_CONCEAL {
            self.plc_run += 1;
            if dec_frame(&mut self.dec, &[], out, false).is_err() {
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
            if dec_frame(&mut self.dec, data, out, true).is_ok() {
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
            if self.dred.available(n, data) >= offset && dred_frame(&mut self.dred, &mut self.dec, offset, out).is_ok() {
                self.stats.recovered_dred += 1;
                self.plc_run = 0;
                return;
            }
        }
        // 3. PLC for a bounded run, then silence.
        self.stats.concealed += 1;
        if self.plc_run < MAX_CONCEAL {
            self.plc_run += 1;
            if dec_frame(&mut self.dec, &[], out, false).is_ok() {
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

#[cfg(test)]
mod group_call_tests;
#[cfg(test)]
mod mix_sum_tests;
