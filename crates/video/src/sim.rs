//! Deterministic simulation of a video call: camera -> encoder -> [`media::Sender`] -> a modelled
//! QUIC connection over a bottleneck link -> [`media::Receiver`] -> decoder -> screen.
//!
//! Everything runs on a virtual clock in fixed ticks, seeded RNG, no threads, so a scenario
//! gives the same numbers every run. The QUIC model is deliberately simple but has the parts
//! that matter for video: a bottleneck queue (bufferbloat), random loss and outages, per-stream
//! retransmission, newest-stream-first scheduling, stream resets, and a loss-based congestion
//! window that the sender reads its rate from.

use std::collections::{BTreeMap, VecDeque};

use crate::media::{Out, RateReport, Receiver, ReceiverConfig, Sender, SenderConfig};
use crate::{Decoder, EncoderConfig, I420, Pattern, VideoEncoder};

const TICK_US: u64 = 250;
const MSS: usize = 1200;
const OVERHEAD: usize = 40;

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The path between the two peers. All times in microseconds.
#[derive(Clone, Debug)]
pub struct Link {
    pub rate_bps: u64,
    /// One-way propagation delay.
    pub delay_us: u64,
    /// Extra random one-way delay, uniform in [0, jitter).
    pub jitter_us: u64,
    /// Random loss probability per packet.
    pub loss: f64,
    /// Bottleneck buffer; beyond it packets are dropped.
    pub queue_bytes: usize,
    /// `(at, rate_bps)`: the bottleneck rate changes at `at`.
    pub rate_changes: Vec<(u64, u64)>,
    /// `(from, to)`: everything sent in this window is lost.
    pub outages: Vec<(u64, u64)>,
}

impl Default for Link {
    fn default() -> Self {
        Self {
            rate_bps: 2_000_000,
            delay_us: 20_000,
            jitter_us: 2_000,
            loss: 0.0,
            queue_bytes: 64 * 1024,
            rate_changes: vec![],
            outages: vec![],
        }
    }
}

/// What produces encoded frames in the simulation.
pub trait SimCodec {
    /// Encodes the frame captured at `index` at `width`x`height`; returns (payload, is_key).
    /// A size change must produce a keyframe, as a real encoder re-created at the new size does.
    fn encode(&mut self, index: u64, key: bool, bitrate_bps: u32, width: u32, height: u32) -> (Vec<u8>, bool);
    /// Decodes a frame on the receiver; `false` means the decoder rejected it.
    fn decode(&mut self, payload: &[u8]) -> bool;
}

/// Size-only codec. Deltas are +-30% around the target, but a small picture can't absorb
/// much: at most `max_bpp` bits per pixel. Keyframes cost more the more bits per pixel the
/// deltas get (fit to libaom on real footage: key/delta ~7 at 320x180 / 200 kbps, ~3.3 at
/// 720p / 1.5 Mbps).
pub struct FakeCodec {
    pub fps: u32,
    pub max_bpp: f64,
    rng: Rng,
    size: (u32, u32),
    /// Decoder side: size of the last keyframe; a delta must match it.
    key_size: Option<(u32, u32)>,
}

impl FakeCodec {
    pub fn new(fps: u32, seed: u64) -> Self {
        Self { fps, max_bpp: 0.3, rng: Rng::new(seed), size: (0, 0), key_size: None }
    }
}

const FAKE_HEADER: usize = 13;

impl SimCodec for FakeCodec {
    fn encode(&mut self, index: u64, key: bool, bitrate_bps: u32, width: u32, height: u32) -> (Vec<u8>, bool) {
        let key = key || self.size != (width, height);
        self.size = (width, height);
        let px = width as f64 * height as f64;
        let mean = (bitrate_bps as f64 / self.fps as f64).min(px * self.max_bpp) / 8.0;
        let bpp = mean * 8.0 / px;
        let size = if key { mean * (63.0 * bpp).clamp(2.5, 7.0) } else { mean * (0.7 + 0.6 * self.rng.f64()) };
        let mut p = vec![0u8; (size as usize).max(FAKE_HEADER)];
        p[0] = key as u8;
        p[1..9].copy_from_slice(&index.to_be_bytes());
        p[9..11].copy_from_slice(&(width as u16).to_be_bytes());
        p[11..13].copy_from_slice(&(height as u16).to_be_bytes());
        (p, key)
    }
    fn decode(&mut self, payload: &[u8]) -> bool {
        let size = (
            u16::from_be_bytes([payload[9], payload[10]]) as u32,
            u16::from_be_bytes([payload[11], payload[12]]) as u32,
        );
        if payload[0] == 1 {
            self.key_size = Some(size);
        }
        self.key_size == Some(size)
    }
}

/// Real AV1: the synthetic camera pattern through libaom, decoded with rav1d. A new size
/// re-creates the encoder (its first frame is a keyframe); the decoder is never reset.
pub struct Av1Codec {
    enc: crate::aom::AomEncoder,
    dec: Decoder,
    pattern: Pattern,
    fps: u32,
    bitrate: u32,
    size: (u32, u32),
    /// Sizes the decoder produced, in order, without repeats.
    pub decoded_sizes: Vec<(u32, u32)>,
}

impl Av1Codec {
    pub fn new(width: u32, height: u32, fps: u32, bitrate_bps: u32) -> Self {
        Self {
            enc: Self::encoder(width, height, fps, bitrate_bps),
            dec: Decoder::new(1, 4096 * 4096).unwrap(),
            pattern: Pattern::new(width as usize, height as usize),
            fps,
            bitrate: bitrate_bps,
            size: (width, height),
            decoded_sizes: vec![],
        }
    }

    fn encoder(width: u32, height: u32, fps: u32, bitrate_bps: u32) -> crate::aom::AomEncoder {
        let (width, height) = (width as usize, height as usize);
        let cfg = EncoderConfig { width, height, fps, bitrate_bps, keyint: 100_000, ..Default::default() };
        crate::aom::AomEncoder::new(&cfg).unwrap()
    }
}

impl SimCodec for Av1Codec {
    fn encode(&mut self, _index: u64, key: bool, bitrate_bps: u32, width: u32, height: u32) -> (Vec<u8>, bool) {
        if self.size != (width, height) {
            self.enc = Self::encoder(width, height, self.fps, bitrate_bps);
            self.pattern = Pattern::new(width as usize, height as usize);
            self.size = (width, height);
            self.bitrate = bitrate_bps;
        }
        if bitrate_bps != self.bitrate {
            self.enc.set_bitrate(bitrate_bps).unwrap();
            self.bitrate = bitrate_bps;
        }
        let pic: I420 = self.pattern.next_frame();
        let p = self.enc.encode(&pic, key).unwrap();
        let p = p.into_iter().next().expect("realtime encoder emits one packet per frame");
        (p.data, p.key)
    }
    fn decode(&mut self, payload: &[u8]) -> bool {
        let Ok(pics) = self.dec.decode(payload) else { return false };
        let [pic] = &pics[..] else { return false };
        let size = (pic.width as u32, pic.height as u32);
        if self.decoded_sizes.last() != Some(&size) {
            self.decoded_sizes.push(size);
        }
        true
    }
}

/// Encode speed of the sending device: `base_us + ns_per_px * pixels`, +-`jitter`.
#[derive(Clone, Debug)]
pub struct Device {
    pub base_us: u64,
    pub ns_per_px: u64,
    /// Relative spread of the encode time, uniform in [-jitter, +jitter].
    pub jitter: f64,
    /// `(at, factor)`: encoding gets `factor` x slower from `at` (thermal throttling).
    pub slowdowns: Vec<(u64, f64)>,
}

impl Device {
    /// moto g52 (Snapdragon 680), codec pinned to the A73 cores: 3.1 ms at 320x180, 6.3 at
    /// 640x360, 11.4 at 960x540, 18.5 at 720p.
    pub fn phone() -> Self {
        Self { base_us: 2_100, ns_per_px: 18, jitter: 0.2, slowdowns: vec![] }
    }

    /// The same phone with the codec on the A53 cores: 6.7 / 14.8 / 22.6 / 33.3 ms.
    pub fn phone_unpinned() -> Self {
        Self { base_us: 4_900, ns_per_px: 31, jitter: 0.2, slowdowns: vec![] }
    }

    fn encode_us(&self, now_us: u64, width: u32, height: u32, rng: &mut Rng) -> u64 {
        let t = (self.base_us + self.ns_per_px * width as u64 * height as u64 / 1000) as f64;
        let slow: f64 = self.slowdowns.iter().filter(|s| s.0 <= now_us).map(|s| s.1).product();
        (t * slow * (1.0 + self.jitter * (2.0 * rng.f64() - 1.0))) as u64
    }
}

/// What the transport reports to [`Sender::feedback`] as a group's backlog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backlog {
    /// Bytes QUIC has not put on the wire yet.
    Unsent,
    /// What `video::net` reports: bytes written more than srtt + 40 ms ago that the receiver
    /// has not acked yet, so it includes the bottleneck queue.
    Unacked,
}

#[derive(Clone, Debug)]
pub struct Scenario {
    pub duration_us: u64,
    pub fps: u32,
    pub link: Link,
    /// Encode time per frame; the encoder runs one frame at a time.
    pub device: Device,
    /// Decode + render time per frame.
    pub decode_us: u64,
    pub sender: SenderConfig,
    pub receiver: ReceiverConfig,
    pub backlog: Backlog,
    pub seed: u64,
}

impl Default for Scenario {
    fn default() -> Self {
        Self {
            duration_us: 30_000_000,
            fps: 30,
            link: Link::default(),
            device: Device::phone(),
            decode_us: 2_000,
            sender: SenderConfig::default(),
            receiver: ReceiverConfig::default(),
            backlog: Backlog::Unacked,
            seed: 7,
        }
    }
}

/// One displayed frame.
#[derive(Clone, Debug)]
pub struct Shown {
    pub at_us: u64,
    pub capture_us: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Default)]
pub struct Report {
    pub captured: u64,
    pub shown: Vec<Shown>,
    pub decode_errors: u64,
    pub keyframes: u64,
    /// Camera frames dropped because the encoder was still busy.
    pub camera_drops: u64,
    /// Resolution changes, and (time, width, height) of each new size at the encoder.
    pub switches: u64,
    pub sizes: Vec<(u64, u32, u32)>,
    pub sender_drops: u64,
    pub skipped: u64,
    pub keyframe_requests: u64,
    pub wire_bytes: u64,
    pub lost_packets: u64,
    pub sent_packets: u64,
    /// (time, encoder bitrate) every feedback.
    pub bitrate: Vec<(u64, u32)>,
    /// (time, bytes in the bottleneck buffer) every feedback.
    pub queue: Vec<(u64, usize)>,
    /// (arrival at the sender, report).
    pub reports: Vec<(u64, RateReport)>,
}

impl Report {
    /// Glass-to-glass latencies (ms) of frames shown in `[from, to)`.
    pub fn latencies_ms(&self, from: u64, to: u64) -> Vec<f64> {
        let mut v: Vec<f64> = self
            .shown
            .iter()
            .filter(|s| s.at_us >= from && s.at_us < to)
            .map(|s| (s.at_us - s.capture_us) as f64 / 1000.0)
            .collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v
    }

    pub fn pct(v: &[f64], p: f64) -> f64 {
        if v.is_empty() { f64::INFINITY } else { v[((v.len() - 1) as f64 * p) as usize] }
    }

    /// Gaps between shown frames longer than `min_us`, as (start, length) in `[from, to)`.
    pub fn freezes(&self, min_us: u64, from: u64, to: u64) -> Vec<(u64, u64)> {
        let mut out = Vec::new();
        let mut prev = from;
        for s in self.shown.iter().filter(|s| s.at_us >= from && s.at_us < to) {
            if s.at_us - prev > min_us {
                out.push((prev, s.at_us - prev));
            }
            prev = s.at_us;
        }
        if to - prev > min_us {
            out.push((prev, to - prev));
        }
        out
    }

    pub fn mean_bitrate(&self, from: u64, to: u64) -> f64 {
        let v: Vec<f64> =
            self.bitrate.iter().filter(|(t, _)| *t >= from && *t < to).map(|(_, b)| *b as f64).collect();
        v.iter().sum::<f64>() / v.len().max(1) as f64
    }

    pub fn summary(&self, duration_us: u64) -> String {
        let l = self.latencies_ms(0, duration_us);
        let f = self.freezes(250_000, 1_000_000, duration_us);
        let sizes: Vec<String> =
            self.sizes.iter().map(|(t, _, h)| format!("{}p@{:.1}", h, *t as f64 / 1e6)).collect();
        format!(
            "shown {}/{} | latency p50 {:.0} p95 {:.0} max {:.0} ms | freezes {} ({:.1} s) | key {} drops {} skipped {} req {} cam {} | decode err {} | {:.0} kbps wire, loss {:.1}% | sizes {}",
            self.shown.len(),
            self.captured,
            Self::pct(&l, 0.5),
            Self::pct(&l, 0.95),
            Self::pct(&l, 1.0),
            f.len(),
            f.iter().map(|x| x.1).sum::<u64>() as f64 / 1e6,
            self.keyframes,
            self.sender_drops,
            self.skipped,
            self.keyframe_requests,
            self.camera_drops,
            self.decode_errors,
            self.wire_bytes as f64 * 8.0 / (duration_us as f64 / 1e6) / 1000.0,
            100.0 * self.lost_packets as f64 / self.sent_packets.max(1) as f64,
            sizes.join(" "),
        )
    }
}

struct TxStream {
    data: Vec<u8>,
    next: usize,
    retx: VecDeque<(usize, usize)>,
    /// The app finished the stream; `fin_sent` once the FIN is in flight (cleared if lost).
    fin: bool,
    fin_sent: bool,
    reset: bool,
    /// Packets in flight; the stream is kept until they are acked or lost.
    unacked: usize,
    /// (time, `data.len()` after that write).
    history: VecDeque<(u64, usize)>,
    /// Bytes the receiver app has acked (`video::net`'s app-level ack).
    app_acked: usize,
}

impl TxStream {
    fn new() -> Self {
        Self {
            data: vec![],
            next: 0,
            retx: VecDeque::new(),
            fin: false,
            fin_sent: false,
            reset: false,
            unacked: 0,
            history: VecDeque::new(),
            app_acked: 0,
        }
    }

    fn unsent(&self) -> usize {
        if self.reset {
            return 0;
        }
        self.data.len() - self.next + self.retx.iter().map(|r| r.1).sum::<usize>()
    }

    /// Written at or before `horizon` and not acked by the receiver app.
    fn unacked_by(&mut self, horizon: u64) -> usize {
        if self.reset {
            return 0;
        }
        while self.history.len() > 1 && self.history[1].0 <= horizon {
            self.history.pop_front();
        }
        let written = match self.history.front() {
            Some(&(at, n)) if at <= horizon => n,
            _ => 0,
        };
        written.saturating_sub(self.app_acked)
    }
}

#[derive(Clone)]
struct Pkt {
    group: u64,
    off: usize,
    data: Vec<u8>,
    fin: bool,
    size: usize,
    sent_us: u64,
}

#[derive(Default)]
struct RxStream {
    /// Out-of-order segments by offset.
    segs: BTreeMap<usize, Vec<u8>>,
    delivered: usize,
    fin_at: Option<usize>,
    done: bool,
}

enum Ev {
    /// Data packet reaches the receiver.
    Arrive(Pkt),
    /// Sender learns the packet arrived.
    Ack(Pkt),
    /// Sender learns the packet was lost.
    Lost(Pkt),
    /// Stream reset reaches the receiver.
    ResetRx(u64),
    /// Keyframe request reaches the sender.
    KeyReq,
    /// Rate report reaches the sender.
    Report(RateReport),
    /// The receiver app's ack of `offset` bytes of a group reaches the sender.
    AppAck(u64, usize),
}

/// Runs `sc` with `codec` and returns what the receiver saw.
pub fn run(sc: &Scenario, codec: &mut dyn SimCodec) -> Report {
    let mut rng = Rng::new(sc.seed);
    let mut rep = Report::default();
    let mut sender = Sender::new(sc.sender.clone());
    let mut receiver = Receiver::new(sc.receiver.clone());

    // Sender-side QUIC state.
    let mut tx: BTreeMap<u64, TxStream> = BTreeMap::new();
    let mut cwnd = 10.0 * MSS as f64;
    let mut ssthresh = f64::INFINITY;
    let mut inflight = 0usize;
    let mut srtt = (2 * sc.link.delay_us) as f64;
    let mut recovery_start = 0u64;
    let mut pace_next = 0u64;
    // Link state.
    let mut rate = sc.link.rate_bps;
    let mut link_free = 0u64;
    // Receiver-side QUIC state.
    let mut rx: BTreeMap<u64, RxStream> = BTreeMap::new();
    // Events by due time (stable order for equal times).
    let mut evs: BTreeMap<(u64, u64), Ev> = BTreeMap::new();
    let mut ev_seq = 0u64;
    let mut push = |evs: &mut BTreeMap<(u64, u64), Ev>, at: u64, e: Ev| {
        ev_seq += 1;
        evs.insert((at, ev_seq), e);
    };
    // Frames waiting for the encoder / decoder to finish: (done_at, ...).
    // (done_at, capture, key, width, height, encode_us)
    let mut encoding: VecDeque<(u64, u64, bool, u32, u32, u64)> = VecDeque::new();
    let mut encoder_free = 0u64;
    let mut enc_rng = Rng::new(sc.seed ^ 0x5eed);
    let mut sizes: BTreeMap<u64, (u32, u32)> = BTreeMap::new();
    let mut decode_busy_until = 0u64;
    let frame_us = 1_000_000 / sc.fps as u64;
    let mut next_capture = 0u64;
    let mut index = 0u64;
    let mut next_feedback = 0u64;
    let mut next_keycheck = 0u64;

    let mut now = 0u64;
    while now < sc.duration_us {
        for &(at, r) in &sc.link.rate_changes {
            if at > now.saturating_sub(TICK_US) && at <= now {
                rate = r;
            }
        }

        // Camera + encoder.
        if now >= next_capture {
            rep.captured += 1;
            next_capture += frame_us;
            if encoding.len() >= 2 {
                rep.camera_drops += 1;
            } else {
                // The camera frame is scaled to the size the sender wants when captured.
                let (w, h) = sender.resolution();
                let t = sc.device.encode_us(now, w, h, &mut enc_rng);
                encoder_free = encoder_free.max(now) + t;
                encoding.push_back((encoder_free, now, sender.wants_keyframe(), w, h, t));
            }
        }
        while encoding.front().is_some_and(|f| f.0 <= now) {
            let (_, capture, key, w, h, t) = encoding.pop_front().unwrap();
            let (payload, is_key) = codec.encode(index, key, sender.bitrate(), w, h);
            index += 1;
            if rep.sizes.last().is_none_or(|s| (s.1, s.2) != (w, h)) {
                rep.sizes.push((now, w, h));
            }
            sizes.insert(capture, (w, h));
            sender.encode_time(now, t);
            let outs = sender.frame(capture, is_key, &payload);
            apply(now, &outs, &mut tx, &mut |g| push(&mut evs, reset_at(now, link_free, &sc.link), Ev::ResetRx(g)));
        }

        // Sender feedback: per-group backlog and the congestion controller's rate.
        if now >= next_feedback {
            let horizon = now.saturating_sub(srtt as u64 + 40_000);
            let unsent: BTreeMap<u64, usize> = match sc.backlog {
                Backlog::Unsent => tx.iter().map(|(g, s)| (*g, s.unsent())).collect(),
                Backlog::Unacked => tx.iter_mut().map(|(g, s)| (*g, s.unacked_by(horizon))).collect(),
            };
            let outs = sender.feedback(&unsent);
            apply(now, &outs, &mut tx, &mut |g| push(&mut evs, reset_at(now, link_free, &sc.link), Ev::ResetRx(g)));
            rep.bitrate.push((now, sender.bitrate()));
            rep.queue.push((now, (link_free.saturating_sub(now) * rate / 8 / 1_000_000) as usize));
            // Forget streams that are reset, or fully sent, FIN'd and acked.
            tx.retain(|_, s| !(s.unacked == 0 && (s.reset || (s.fin_sent && s.unsent() == 0))));
            next_feedback += 10_000;
        }

        // Transmit: retransmissions first, then new data, newest group first; within the
        // congestion window and paced at 1.25 x cwnd / srtt.
        while (inflight + MSS) as f64 <= cwnd && pace_next <= now {
            let Some((g, off, data, fin)) = next_packet(&mut tx) else { break };
            let size = data.len() + OVERHEAD;
            let p = Pkt { group: g, off, data, fin, size, sent_us: now };
            inflight += size;
            rep.sent_packets += 1;
            rep.wire_bytes += size as u64;
            pace_next = now + (size as f64 * srtt / (1.25 * cwnd)) as u64;
            let in_outage = sc.link.outages.iter().any(|&(a, b)| now >= a && now < b);
            let queued = link_free.saturating_sub(now) * rate / 8 / 1_000_000;
            let dropped = in_outage || queued as usize + size > sc.link.queue_bytes || rng.f64() < sc.link.loss;
            let depart = link_free.max(now) + size as u64 * 8 * 1_000_000 / rate.max(1);
            if !(in_outage || queued as usize + size > sc.link.queue_bytes) {
                link_free = depart;
            }
            let jitter = if sc.link.jitter_us > 0 { rng.next_u64() % sc.link.jitter_us } else { 0 };
            let arrive = depart + sc.link.delay_us + jitter;
            if dropped {
                rep.lost_packets += 1;
                // Detected roughly one RTT plus reordering slack after it would have arrived.
                let detect = arrive + sc.link.delay_us + (srtt / 8.0) as u64 + 1_000;
                push(&mut evs, detect, Ev::Lost(p));
            } else {
                push(&mut evs, arrive, Ev::Arrive(p.clone()));
                push(&mut evs, arrive + sc.link.delay_us, Ev::Ack(p));
            }
        }

        // Deliver due events.
        while let Some((&(at, seq), _)) = evs.first_key_value() {
            if at > now {
                break;
            }
            let ev = evs.remove(&(at, seq)).unwrap();
            match ev {
                Ev::Arrive(p) => {
                    let s = rx.entry(p.group).or_default();
                    if s.done {
                        continue;
                    }
                    if p.fin {
                        s.fin_at = Some(p.off);
                    } else if p.off + p.data.len() > s.delivered {
                        s.segs.insert(p.off, p.data);
                    }
                    // Hand contiguous bytes to the app.
                    while let Some((&o, _)) = s.segs.first_key_value() {
                        if o > s.delivered {
                            break;
                        }
                        let d = s.segs.pop_first().unwrap().1;
                        if o + d.len() > s.delivered {
                            let skip = s.delivered - o;
                            s.delivered = o + d.len();
                            if receiver.on_data(at, p.group, &d[skip..]).is_err() {
                                rep.decode_errors += 1;
                            }
                        }
                    }
                    push(&mut evs, at + sc.link.delay_us, Ev::AppAck(p.group, s.delivered));
                    if s.fin_at == Some(s.delivered) {
                        s.done = true;
                        receiver.on_finish(p.group);
                    }
                }
                Ev::Ack(p) => {
                    inflight -= p.size;
                    if let Some(s) = tx.get_mut(&p.group) {
                        s.unacked -= 1;
                    }
                    let rtt = (at - p.sent_us) as f64;
                    srtt = 0.875 * srtt + 0.125 * rtt;
                    if cwnd < ssthresh {
                        cwnd += p.size as f64;
                    } else {
                        cwnd += (MSS * p.size) as f64 / cwnd;
                    }
                }
                Ev::Lost(p) => {
                    inflight -= p.size;
                    if p.sent_us >= recovery_start {
                        cwnd = (cwnd * 0.7).max(2.0 * MSS as f64);
                        ssthresh = cwnd;
                        recovery_start = at;
                    }
                    if let Some(s) = tx.get_mut(&p.group) {
                        s.unacked -= 1;
                        if !s.reset {
                            if p.fin {
                                s.fin_sent = false;
                            } else {
                                s.retx.push_back((p.off, p.data.len()));
                            }
                        }
                    }
                }
                Ev::ResetRx(g) => {
                    rx.entry(g).or_default().done = true;
                    receiver.on_reset(g);
                }
                Ev::KeyReq => sender.request_keyframe(),
                Ev::AppAck(g, off) => {
                    if let Some(s) = tx.get_mut(&g) {
                        s.app_acked = s.app_acked.max(off);
                    }
                }
                Ev::Report(r) => {
                    sender.on_report(at, &r);
                    rep.reports.push((at, r));
                }
            }
        }

        // Receiver: decode and show in order, one at a time.
        if decode_busy_until <= now
            && let Some(f) = receiver.poll(now)
        {
            if !codec.decode(&f.payload) {
                rep.decode_errors += 1;
            }
            decode_busy_until = now + sc.decode_us;
            // Older entries are frames that were never shown.
            let (width, height) = sizes.remove(&f.capture_us).unwrap_or_default();
            sizes = sizes.split_off(&f.capture_us);
            rep.shown.push(Shown { at_us: decode_busy_until, capture_us: f.capture_us, width, height });
        }
        if let Some(r) = receiver.report(now) {
            push(&mut evs, now + sc.link.delay_us, Ev::Report(r));
        }
        if now >= next_keycheck {
            if receiver.wants_keyframe(now) {
                rep.keyframe_requests += 1;
                push(&mut evs, now + sc.link.delay_us, Ev::KeyReq);
            }
            next_keycheck += 50_000;
        }

        now += TICK_US;
    }
    rep.keyframes = sender.keyframes;
    rep.sender_drops = sender.drops;
    rep.switches = sender.ladder().switches;
    rep.skipped = receiver.skipped_frames;
    rep
}

/// RESET_STREAM goes out in a packet behind whatever already sits in the bottleneck buffer.
fn reset_at(now: u64, link_free: u64, link: &Link) -> u64 {
    link_free.max(now) + link.delay_us
}

fn apply(now: u64, outs: &[Out], tx: &mut BTreeMap<u64, TxStream>, reset: &mut dyn FnMut(u64)) {
    for o in outs {
        match o {
            Out::Open { group } => {
                tx.insert(*group, TxStream::new());
            }
            Out::Write { group, data } => {
                if let Some(s) = tx.get_mut(group) {
                    s.data.extend_from_slice(data);
                    s.history.push_back((now, s.data.len()));
                }
            }
            Out::Finish { group } => {
                if let Some(s) = tx.get_mut(group) {
                    s.fin = true;
                }
            }
            Out::Reset { group } => {
                if let Some(s) = tx.get_mut(group).filter(|s| !s.reset) {
                    s.reset = true;
                    s.retx.clear();
                    reset(*group);
                }
            }
        }
    }
}

/// Next packet to send: (group, offset, bytes, fin). A FIN is its own empty packet.
fn next_packet(tx: &mut BTreeMap<u64, TxStream>) -> Option<(u64, usize, Vec<u8>, bool)> {
    for (g, s) in tx.iter_mut().rev() {
        if s.reset {
            continue;
        }
        if let Some((off, len)) = s.retx.pop_front() {
            let take = len.min(MSS);
            if take < len {
                s.retx.push_front((off + take, len - take));
            }
            s.unacked += 1;
            return Some((*g, off, s.data[off..off + take].to_vec(), false));
        }
        let left = s.data.len() - s.next;
        if left > 0 {
            let off = s.next;
            let take = left.min(MSS);
            s.next += take;
            s.unacked += 1;
            return Some((*g, off, s.data[off..off + take].to_vec(), false));
        }
        if s.fin && !s.fin_sent {
            s.fin_sent = true;
            s.unacked += 1;
            return Some((*g, s.data.len(), vec![], true));
        }
    }
    None
}

#[cfg(test)]
mod tests;
