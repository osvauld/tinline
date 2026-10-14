//! Video over QUIC, sans-IO: one unidirectional stream per group of pictures (MoQ style).
//!
//! A group starts with a keyframe and holds every frame up to the next keyframe, so a group can
//! always be decoded on its own and losing one never breaks the next. QUIC retransmits inside a
//! stream, so frames of a group arrive complete and in order; groups are independent, and the
//! transport sends the newest group first. When the link can't keep up the sender gives up on
//! the stale group (stream reset), forces a keyframe and lowers the bitrate.
//!
//! Stream layout: `[group: u64 BE]` then frames `[len: u32 BE][capture_us: u64 BE][payload]`.
//! The transport is told what to do through [`Out`] and reports back with plain method calls,
//! so the same code runs on iroh and in [`crate::sim`].
//!
//! Rate control is delay-based (the idea behind WebRTC's GCC), done at this layer so it needs
//! nothing from the transport: the receiver compares each frame's arrival time with its capture
//! time. The clock offset between the devices cancels out, so a *rise* over the recent minimum
//! is queueing delay. It sends a [`RateReport`] every 50 ms; the sender backs off to the
//! delivered rate as soon as a queue builds (or far less gets through than it sends), drains
//! that queue below the delivered rate, and only ramps up while the path is empty. A
//! loss-based congestion controller alone fills the bottleneck buffer first (bufferbloat), which
//! the simulation showed as seconds of delay.
//!
//! Resolution follows the bitrate through a [`Ladder`] and changes only at a new group: the
//! sender asks for a keyframe and the app encodes it at [`Sender::resolution`]. The AV1 sequence
//! header in the keyframe carries the new size, so the wire format doesn't change.

use std::collections::BTreeMap;

pub const STREAM_HEADER: usize = 8;
pub const FRAME_HEADER: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Out {
    /// Open a new stream for `group`, highest priority so far.
    Open { group: u64 },
    Write { group: u64, data: Vec<u8> },
    /// No more frames in `group` (it may still be sending).
    Finish { group: u64 },
    /// Abandon `group`: stop (re)transmitting it.
    Reset { group: u64 },
}

#[derive(Clone, Debug)]
pub struct SenderConfig {
    pub fps: u32,
    pub min_bps: u32,
    pub max_bps: u32,
    pub start_bps: u32,
    /// Unsent video worth more than this much time means we are behind: drop and restart.
    pub max_backlog_us: u64,
    /// Queueing delay above this: back off to the delivered rate.
    pub overuse_us: u64,
    /// Queueing delay below this: allowed to ramp up.
    pub underuse_us: u64,
    /// Delivered below this % of what we sent over the last two reports, and short of it by at
    /// least `starve_min_bps`: the link got slower. Seen a report or two before the queueing
    /// delay, which lags while big frames crawl through the slower link.
    pub starve_pct: u32,
    pub starve_min_bps: u32,
    /// ...in this many reports in a row (a lost packet stalls a stream for about an RTT, which
    /// can look like starvation in one report).
    pub starve_reports: u32,
    /// A standing queue is drained within this time: the encoder gets the delivered rate minus
    /// the queue (queueing delay x delivered) spread over it...
    pub drain_us: u64,
    /// ...but no less than this % of the delivered rate.
    pub drain_pct: u32,
    pub ladder: LadderConfig,
}

impl Default for SenderConfig {
    fn default() -> Self {
        Self {
            fps: 30,
            min_bps: 100_000,
            max_bps: 2_000_000,
            start_bps: 500_000,
            max_backlog_us: 300_000,
            overuse_us: 40_000,
            underuse_us: 15_000,
            starve_pct: 50,
            starve_min_bps: 300_000,
            starve_reports: 2,
            drain_us: 500_000,
            drain_pct: 25,
            ladder: LadderConfig::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rung {
    pub width: u32,
    pub height: u32,
    /// Below this bitrate the rung is left for a smaller one.
    pub min_bps: u32,
    /// The encoder gets no more than this at this size: more bits barely help a small picture,
    /// and they would make the keyframe after a step down needlessly large.
    pub max_bps: u32,
}

impl Rung {
    fn pixels(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

#[derive(Clone, Debug)]
pub struct LadderConfig {
    /// Smallest first.
    pub rungs: Vec<Rung>,
    /// Highest rung index allowed (device or peer capability).
    pub max_rung: usize,
    /// Step up once the bitrate is this % of the next rung's `min_bps`...
    pub up_margin_pct: u32,
    /// ...for this long without a break,
    pub up_hold_us: u64,
    /// and no sooner than this after stepping down.
    pub up_after_down_us: u64,
    /// Median encode time over a second above this % of the frame interval: too slow for the
    /// rung. Stepping up needs the p75 scaled to the bigger picture to fit it.
    pub encode_budget_pct: u32,
    /// After stepping down for encode time, the rung stays out of reach this long, doubling
    /// with every further step down for encode time up to `cpu_block_max_us`...
    pub cpu_block_us: u64,
    pub cpu_block_max_us: u64,
    /// ...and back to `cpu_block_us` after this long without one.
    pub cpu_forget_us: u64,
}

impl Default for LadderConfig {
    fn default() -> Self {
        let r = |width, height, min_bps, max_bps| Rung { width, height, min_bps, max_bps };
        Self {
            rungs: vec![
                r(320, 180, 0, 500_000),
                r(640, 360, 250_000, 1_200_000),
                r(960, 540, 600_000, 2_000_000),
                r(1280, 720, 1_000_000, 3_000_000),
            ],
            max_rung: 3,
            up_margin_pct: 130,
            up_hold_us: 2_500_000,
            up_after_down_us: 4_000_000,
            encode_budget_pct: 60,
            cpu_block_us: 5_000_000,
            cpu_block_max_us: 30_000_000,
            cpu_forget_us: 60_000_000,
        }
    }
}

/// See `Sender::starve_after_us`: after a drain, and at the start (first keyframe, ramp).
const STARVE_QUIET_US: u64 = 300_000;
const STARVE_START_US: u64 = 1_000_000;
/// See `Sender::queuing`.
const BASELINE_PROBE_US: u64 = 5_000_000;
const MAX_Q_OFFSET_US: u64 = 100_000;
/// After a decrease, the second look comes this much later (three more reports).
const SETTLE_US: u64 = RATE_WINDOW_US + REPORT_US;

/// Encode times kept (about a second of video); a step down for encode time needs a full window.
const ENCODE_WINDOW: usize = 30;
/// Samples needed before encode time is trusted for a step up.
const ENCODE_MIN: usize = 10;
/// Frames ignored after a new encoder (start, size change): libaom's first frames are slower,
/// and so is a machine that is just starting the call.
const ENCODE_WARMUP: u32 = 15;

/// Picks the resolution from the bitrate and the measured encode time. Steps down at once,
/// up one rung at a time and only after the bitrate has held above the next rung's threshold.
pub struct Ladder {
    cfg: LadderConfig,
    frame_us: u64,
    cur: usize,
    above_since: Option<u64>,
    last_down_us: Option<u64>,
    /// Encode times (us) at the current rung, after the first `warmup` frames.
    encode: std::collections::VecDeque<u64>,
    warmup: u32,
    /// Rungs above this are blocked until the time, for being too slow to encode.
    cpu_cap: Option<(usize, u64)>,
    /// Current block length and when it was last applied.
    cpu_block: (u64, u64),
    pub switches: u64,
}

impl Ladder {
    pub fn new(cfg: LadderConfig, fps: u32, start_bps: u32) -> Self {
        assert!(!cfg.rungs.is_empty());
        let mut l = Self {
            cfg,
            frame_us: 1_000_000 / fps.max(1) as u64,
            cur: 0,
            above_since: None,
            last_down_us: None,
            encode: Default::default(),
            warmup: ENCODE_WARMUP,
            cpu_cap: None,
            cpu_block: (0, 0),
            switches: 0,
        };
        l.cur = l.fit(start_bps);
        l
    }

    pub fn rung(&self) -> Rung {
        self.cfg.rungs[self.cur]
    }

    pub fn index(&self) -> usize {
        self.cur
    }

    /// Highest rung index the peer or device can take.
    pub fn set_max(&mut self, max: usize) {
        self.cfg.max_rung = max;
    }

    fn max(&self, now_us: u64) -> usize {
        let cap = match self.cpu_cap {
            Some((r, until)) if now_us < until => r,
            _ => usize::MAX,
        };
        self.cfg.max_rung.min(cap).min(self.cfg.rungs.len() - 1)
    }

    /// Highest rung whose floor `bps` covers (ignoring caps).
    fn fit(&self, bps: u32) -> usize {
        let top = self.cfg.max_rung.min(self.cfg.rungs.len() - 1);
        (0..=top).rev().find(|&i| self.cfg.rungs[i].min_bps <= bps).unwrap_or(0)
    }

    /// `pct` percentile of the encode times, given at least `min` of them.
    fn encode_pct(&self, pct: usize, min: usize) -> Option<u64> {
        if self.encode.len() < min {
            return None;
        }
        let mut v: Vec<u64> = self.encode.iter().copied().collect();
        v.sort_unstable();
        Some(v[(v.len() - 1) * pct / 100])
    }

    fn budget_us(&self) -> u64 {
        self.frame_us * self.cfg.encode_budget_pct as u64 / 100
    }

    fn set(&mut self, now_us: u64, to: usize) {
        if to < self.cur {
            self.last_down_us = Some(now_us);
        }
        self.cur = to;
        self.above_since = None;
        self.encode.clear();
        self.warmup = ENCODE_WARMUP;
        self.switches += 1;
    }

    /// One frame took `us` to encode. Returns true if the rung changed.
    pub fn encode_time(&mut self, now_us: u64, us: u64) -> bool {
        if self.warmup > 0 {
            self.warmup -= 1;
            return false;
        }
        self.encode.push_back(us);
        if self.encode.len() > ENCODE_WINDOW {
            self.encode.pop_front();
        }
        // Sustained: half the frames of the last second were too slow (a busy machine preempting
        // the odd frame isn't helped by a smaller picture).
        if self.cur > 0 && self.encode_pct(50, ENCODE_WINDOW).is_some_and(|p| p > self.budget_us()) {
            let (block, last) = self.cpu_block;
            let block = if block == 0 || now_us.saturating_sub(last) > self.cfg.cpu_forget_us {
                self.cfg.cpu_block_us
            } else {
                (block * 2).min(self.cfg.cpu_block_max_us)
            };
            self.cpu_block = (block, now_us);
            self.cpu_cap = Some((self.cur - 1, now_us + block));
            self.set(now_us, self.cur - 1);
            return true;
        }
        false
    }

    /// Re-evaluates for the current target bitrate. Returns true if the rung changed.
    pub fn update(&mut self, now_us: u64, bps: u32) -> bool {
        self.update_held(now_us, bps, false)
    }

    /// [`Self::update`], but with `hold` nothing changes yet.
    pub fn update_held(&mut self, now_us: u64, bps: u32, hold: bool) -> bool {
        if hold {
            return false;
        }
        let max = self.max(now_us);
        let down = self.fit(bps).min(max);
        if down < self.cur {
            self.set(now_us, down);
            return true;
        }
        if hold {
            return false;
        }
        let next = self.cur + 1;
        if next > max {
            self.above_since = None;
            return false;
        }
        let r = self.cfg.rungs[next];
        let above = bps as u64 * 100 >= r.min_bps as u64 * self.cfg.up_margin_pct as u64;
        // Would the bigger picture still encode in time? Assume time grows with pixels.
        let fast = match self.encode_pct(75, ENCODE_MIN) {
            Some(p) => p * r.pixels() / self.rung().pixels() <= self.budget_us(),
            None => false,
        };
        if !(above && fast) {
            self.above_since = None;
            return false;
        }
        let since = *self.above_since.get_or_insert(now_us);
        let settled = self.last_down_us.is_none_or(|t| now_us - t >= self.cfg.up_after_down_us);
        if now_us - since >= self.cfg.up_hold_us && settled {
            self.set(now_us, next);
            return true;
        }
        false
    }
}

/// Why keyframes were sent (`Sender::key_causes`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyCauses {
    /// First frame, or the first after the transport lost the open group.
    pub start: u64,
    /// Ladder step for the bitrate.
    pub rate: u64,
    /// Ladder step down for encode time.
    pub cpu: u64,
    /// Asked for by the receiver (stall, decode error) or the transport.
    pub request: u64,
    /// The sender reset a group that fell behind.
    pub backlog: u64,
    /// The encoder made one nobody asked for.
    pub other: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyCause {
    Rate,
    Cpu,
    Request,
    Backlog,
}

pub struct Sender {
    cfg: SenderConfig,
    next_group: u64,
    /// Group currently being written, if any.
    current: Option<u64>,
    /// Older groups whose streams the transport may still be sending.
    previous: Vec<u64>,
    want_key: bool,
    key_cause: Option<KeyCause>,
    pub key_causes: KeyCauses,
    /// Estimate of what the path carries; drives the ladder.
    bitrate: u32,
    last_decrease_us: u64,
    /// A queue is standing in the path: send well below the delivered rate until it is gone.
    drain: Option<u32>,
    /// Last report showed bytes getting through.
    delivering: bool,
    /// Since when we have been draining while still sending at least half of what gets
    /// through: a drain that deep queueing doesn't make deeper means the queue is not in the
    /// network (a receiver that decodes slower than we send), so a lower rate can't empty it.
    drain_stuck_since: Option<u64>,
    /// Last report's queueing delay (less `q_offset`).
    queuing_us: u64,
    /// Learned queueing baseline, and (since, lowest queueing above it) while it stays above
    /// `underuse_us` (see `queuing`).
    q_offset: u64,
    high_q: Option<(u64, u64)>,
    /// After a decrease, the report at or after this takes a second look (see `rate`), and
    /// the estimate before that decrease.
    settle_until: Option<u64>,
    pre_cut: u32,
    /// Delivered rates reported since that decrease.
    settle_delivered: Vec<u64>,
    /// Delta-frame bytes handed to the transport since the last report, and in the report
    /// interval before.
    sent: u64,
    sent_prev: (u64, u64),
    last_report_us: Option<u64>,
    /// No starvation verdicts before this: the first reports, and right after a drain when our
    /// own rate jumps back up ahead of what the receiver has seen yet.
    starve_after_us: u64,
    starved_n: u32,
    ladder: Ladder,
    now_us: u64,
    pub keyframes: u64,
    /// When set, every report with the encoder bitrate and rung after it (for tests).
    pub report_log: Option<Vec<(u64, RateReport, u32, usize)>>,
    pub drops: u64,
}

impl Sender {
    pub fn new(cfg: SenderConfig) -> Self {
        let bitrate = cfg.start_bps;
        let ladder = Ladder::new(cfg.ladder.clone(), cfg.fps, bitrate);
        Self {
            cfg,
            next_group: 0,
            current: None,
            previous: Vec::new(),
            want_key: true,
            key_cause: None,
            key_causes: KeyCauses::default(),
            bitrate,
            last_decrease_us: 0,
            drain: None,
            delivering: false,
            drain_stuck_since: None,
            queuing_us: 0,
            q_offset: 0,
            high_q: None,
            settle_until: None,
            pre_cut: 0,
            settle_delivered: Vec::new(),
            sent: 0,
            sent_prev: (0, 0),
            last_report_us: None,
            starve_after_us: 0,
            starved_n: 0,
            ladder,
            now_us: 0,
            keyframes: 0,
            report_log: None,
            drops: 0,
        }
    }

    /// Encoder target to use for the next frame.
    pub fn bitrate(&self) -> u32 {
        self.drain.map_or(self.bitrate, |d| d.min(self.bitrate))
    }

    /// Whether a standing queue is being drained.
    pub fn draining(&self) -> bool {
        self.drain.is_some()
    }

    /// Size (width, height) to encode the next frame at. When it changes the sender also wants a
    /// keyframe, so the new size always starts a new group.
    pub fn resolution(&self) -> (u32, u32) {
        let r = self.ladder.rung();
        (r.width, r.height)
    }

    pub fn ladder(&self) -> &Ladder {
        &self.ladder
    }

    /// The encoder took `us` for the last frame.
    pub fn encode_time(&mut self, now_us: u64, us: u64) {
        self.now_us = self.now_us.max(now_us);
        if self.ladder.encode_time(now_us, us) {
            self.want(KeyCause::Cpu);
        }
        self.relayer();
    }

    fn want(&mut self, cause: KeyCause) {
        if !self.want_key {
            self.want_key = true;
            self.key_cause = Some(cause);
        }
    }

    fn relayer(&mut self) {
        // The ladder waits for the second look: without it a drop went 540p -> 360p -> 180p (a
        // keyframe each), and a stream stalled ~100 ms on a tail loss (two starved reports)
        // went straight to 180p. Not forever if reports stop coming.
        let settling = self.settle_until.is_some_and(|t| self.now_us < t + RATE_WINDOW_US);
        if self.ladder.update_held(self.now_us, self.bitrate, settling) {
            self.want(KeyCause::Rate);
        }
        self.bitrate = self.bitrate.min(self.ladder.rung().max_bps);
    }

    /// The next frame handed to the encoder must be a keyframe.
    pub fn wants_keyframe(&self) -> bool {
        self.want_key || self.current.is_none()
    }

    /// The receiver asked for a keyframe (it stalled or just joined).
    pub fn request_keyframe(&mut self) {
        self.want(KeyCause::Request);
    }

    /// One encoded frame. Delta frames with no open group are dropped (they can't be decoded).
    pub fn frame(&mut self, capture_us: u64, key: bool, payload: &[u8]) -> Vec<Out> {
        let mut out = Vec::new();
        if key {
            let c = &mut self.key_causes;
            match self.key_cause.take() {
                _ if self.keyframes == 0 => c.start += 1,
                Some(KeyCause::Rate) => c.rate += 1,
                Some(KeyCause::Cpu) => c.cpu += 1,
                Some(KeyCause::Request) => c.request += 1,
                Some(KeyCause::Backlog) => c.backlog += 1,
                None if self.current.is_none() => c.start += 1,
                None => c.other += 1,
            }
            if let Some(g) = self.current.take() {
                out.push(Out::Finish { group: g });
                self.previous.push(g);
            }
            let g = self.next_group;
            self.next_group += 1;
            self.current = Some(g);
            self.want_key = false;
            self.keyframes += 1;
            out.push(Out::Open { group: g });
            out.push(Out::Write { group: g, data: g.to_be_bytes().to_vec() });
        }
        let Some(g) = self.current else { return out };
        // A keyframe takes several report intervals to cross the link: counting it made our own
        // step up look like a starved link (and step straight back down).
        if !key {
            self.sent += (FRAME_HEADER + payload.len()) as u64;
        }
        let mut data = Vec::with_capacity(FRAME_HEADER + payload.len());
        data.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        data.extend_from_slice(&capture_us.to_be_bytes());
        data.extend_from_slice(payload);
        out.push(Out::Write { group: g, data });
        out
    }

    /// A receiver report arrived.
    pub fn on_report(&mut self, now_us: u64, r: &RateReport) {
        self.now_us = self.now_us.max(now_us);
        self.rate(now_us, r);
        self.relayer();
        let (br, rung) = (self.bitrate(), self.ladder.index());
        if let Some(log) = &mut self.report_log {
            log.push((now_us, r.clone(), br, rung));
        }
    }

    /// What we sent over the last two report intervals, as a rate.
    fn sent_bps(&mut self, now_us: u64) -> Option<u64> {
        let Some(last) = self.last_report_us.replace(now_us) else {
            self.starve_after_us = now_us + STARVE_START_US;
            self.sent = 0;
            return None;
        };
        let (prev_bytes, prev_us) = self.sent_prev;
        self.sent_prev = (self.sent, now_us - last);
        let bytes = std::mem::take(&mut self.sent) + prev_bytes;
        let span = now_us - last + prev_us;
        (span > 0).then(|| bytes * 8 * 1_000_000 / span)
    }

    /// The report's queueing delay less a learned baseline. Every rate cut drains the path
    /// below `underuse_us` within a second or two, so a delay that hasn't been that low for
    /// `BASELINE_PROBE_US` is not a queue we cause (a receiver clock running fast, a path with
    /// a standing queue) and would otherwise pin us at the floor: its minimum over that time
    /// joins the baseline (at most `MAX_Q_OFFSET_US` in all). Lower reports lower it again.
    fn queuing(&mut self, now_us: u64, q: u64) -> u64 {
        self.q_offset = self.q_offset.min(q);
        let above = q - self.q_offset;
        // Only below half the top rate: higher up, a delay that stays between the thresholds is
        // usually our own standing queue at the link's capacity, and learning it lets it grow.
        if above < self.cfg.underuse_us || self.bitrate > self.cfg.max_bps / 2 {
            self.high_q = None;
        } else {
            let (since, low) = self.high_q.get_or_insert((now_us, above));
            *low = (*low).min(above);
            if now_us - *since >= BASELINE_PROBE_US {
                self.q_offset = (self.q_offset + *low).min(MAX_Q_OFFSET_US);
                self.high_q = None;
            }
        }
        q.saturating_sub(self.q_offset)
    }

    fn rate(&mut self, now_us: u64, r: &RateReport) {
        let delivered = r.delivered_bps as u64;
        let sent = self.sent_bps(now_us).unwrap_or(0);
        self.delivering = delivered > 0;
        let queuing_us = self.queuing(now_us, r.queuing_us);
        self.queuing_us = queuing_us;
        let stuck = self.drain.is_some() && sent * 2 >= delivered;
        self.drain_stuck_since = if stuck { self.drain_stuck_since.or(Some(now_us)) } else { None };
        let short = now_us >= self.starve_after_us
            && delivered > 0
            && delivered * 100 < sent * self.cfg.starve_pct as u64
            && sent - delivered >= self.cfg.starve_min_bps as u64;
        self.starved_n = if short { self.starved_n + 1 } else { 0 };
        let starved = self.starved_n >= self.cfg.starve_reports;
        if self.settle_until.is_some() {
            self.settle_delivered.push(delivered);
        }
        let second_look = self.settle_until.is_some_and(|t| now_us >= t);
        // At the second look: the median of what got through in the reports since the first (a
        // 100 ms window holds three or four packets at 300 kbps, so one report is +-30%, and a
        // stream stalled on a lost packet reads low in one).
        let settled = {
            let v = &mut self.settle_delivered;
            v.sort_unstable();
            (v.get(v.len().saturating_sub(1) / 2).copied().unwrap_or(delivered) * 85 / 100) as u32
        };
        if queuing_us > self.cfg.overuse_us || starved {
            // A queue is building: send no faster than what actually gets through, and below it
            // until the queue is gone (each 100 ms of it delays every frame). One rate window
            // after a decrease, look again: the first delivered rate still held bytes from
            // before a drop, and the ladder waits for this one.
            if second_look || now_us - self.last_decrease_us > 200_000 || self.last_decrease_us == 0 {
                let target = (delivered * 85 / 100) as u32;
                if second_look {
                    // Replaces the first cut, which may have been too deep as well (a gap in
                    // what the encoder sent).
                    self.bitrate = self.pre_cut.min(settled).max(self.cfg.min_bps);
                    self.settle_until = None;
                } else {
                    if self.settle_until.is_none() {
                        self.pre_cut = self.bitrate;
                        self.settle_until = Some(now_us + SETTLE_US);
                        self.settle_delivered.clear();
                    }
                    self.bitrate = self.bitrate.min(target).max(self.cfg.min_bps);
                }
                self.last_decrease_us = now_us;
            }
            // Starved: the queue is bigger than the delay shows yet, drain as fast as allowed.
            let queue_pct = if starved { 100 } else { queuing_us * 100 / self.cfg.drain_us.max(1) };
            let pct = 100u64.saturating_sub(queue_pct).max(self.cfg.drain_pct as u64);
            if pct < 100 {
                // Not below `drain_pct` of the floor: with a receiver that is slow to decode,
                // what gets through follows what we send and the drain would spiral to nothing.
                let floor = self.cfg.min_bps as u64 * self.cfg.drain_pct as u64 / 100;
                let d = (delivered * pct / 100).max(floor) as u32;
                self.drain = Some(self.drain.map_or(d, |old| old.min(d)));
            }
            return;
        }
        if second_look {
            // The queue was gone a rate window later: a blip, keep no more of the cut than the
            // delivered rate asks for.
            self.bitrate = self.bitrate.max(self.pre_cut.min(settled));
            self.settle_until = None;
        }
        if queuing_us < self.cfg.underuse_us && self.drain.take().is_some() {
            self.starve_after_us = now_us + STARVE_QUIET_US;
        }
        if self.drain.is_none() && queuing_us < self.cfg.underuse_us && now_us - self.last_decrease_us > 500_000 {
            // Path is empty: +8% per 100 ms, never far above what was delivered.
            let cap = (delivered * 3 / 2).max(self.cfg.min_bps as u64) as u32;
            let dt = self.sent_prev.1.min(200_000);
            let up = self.bitrate + (self.bitrate as u64 * 8 * dt / 100 / 100_000) as u32;
            self.bitrate = up.min(self.bitrate.max(cap)).min(self.cfg.max_bps);
        }
    }

    /// Transport feedback, called every few ms: per group, bytes not delivered yet that should
    /// have been (`video::net`: written more than an RTT + slack ago and not acked by the
    /// receiver; this includes the queue in the path).
    pub fn feedback(&mut self, unsent: &BTreeMap<u64, usize>) -> Vec<Out> {
        let mut out = Vec::new();
        // Older groups that still have unsent data are stale: the newer keyframe replaces them.
        self.previous.retain(|g| match unsent.get(g) {
            Some(&n) if n > 0 => {
                out.push(Out::Reset { group: *g });
                false
            }
            Some(_) => true,
            None => false,
        });
        let backlog: usize = unsent.values().sum();
        let backlog_us = backlog as u64 * 8 * 1_000_000 / (self.bitrate.max(1) as u64);
        // While draining, the backlog is the queue in the path, already on its way out.
        let draining = self.drain.is_some() && self.delivering;
        // Frames wait in a queue our rate doesn't drain (a slow decoder): a lower bitrate can't
        // help, only a keyframe the receiver can jump to.
        let stuck = self.drain_stuck_since.is_some_and(|t| self.now_us.saturating_sub(t) > self.cfg.drain_us);
        let stale = stuck && self.queuing_us > self.cfg.max_backlog_us;
        if (backlog_us > self.cfg.max_backlog_us && !draining) || stale {
            // Behind: abandon the current group, restart from a keyframe at a lower rate.
            if let Some(g) = self.current.take() {
                out.push(Out::Reset { group: g });
            }
            self.want(KeyCause::Backlog);
            self.drops += 1;
            if stale {
                self.drain_stuck_since = None;
                self.queuing_us = 0;
            } else {
                self.bitrate = (self.bitrate / 2).max(self.cfg.min_bps);
                self.settle_until = None;
                self.relayer();
            }
        }
        out
    }
}

/// A frame ready for the decoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub group: u64,
    /// Index inside the group; 0 is the keyframe.
    pub index: u32,
    pub capture_us: u64,
    pub payload: Vec<u8>,
}

/// Receiver -> sender, every [`REPORT_US`] by default.
#[derive(Clone, Debug, PartialEq)]
pub struct RateReport {
    /// Video bytes received over the window, as a rate.
    pub delivered_bps: u32,
    /// (arrival - capture) above the minimum of the last 10 s, for the least delayed frame
    /// that arrived in the rate window (the latest frame if none did).
    pub queuing_us: u64,
}

pub const REPORT_US: u64 = 50_000;
/// Delivered rate is measured over this long (sliding), so 50 ms reports aren't too noisy.
pub const RATE_WINDOW_US: u64 = 100_000;
const BASELINE_US: u64 = 10_000_000;

#[derive(Default)]
struct Group {
    buf: Vec<u8>,
    /// Bytes of `buf` already parsed into frames.
    parsed: usize,
    frames: Vec<Frame>,
    /// Next index in `frames` to hand out.
    next: usize,
    finished: bool,
    header: bool,
}

#[derive(Clone, Debug)]
pub struct ReceiverConfig {
    /// No new frame for this long while a group is open: ask for a keyframe.
    pub stall_us: u64,
    /// Largest frame accepted (a peer must not make us buffer without bound).
    pub max_frame: usize,
    /// A [`RateReport`] at most this often...
    pub report_us: u64,
    /// ...with the delivered rate over this long.
    pub rate_window_us: u64,
}

impl Default for ReceiverConfig {
    fn default() -> Self {
        Self { stall_us: 600_000, max_frame: 1 << 20, report_us: REPORT_US, rate_window_us: RATE_WINDOW_US }
    }
}

pub struct Receiver {
    cfg: ReceiverConfig,
    groups: BTreeMap<u64, Group>,
    /// Group we are playing; older ones are discarded.
    current: Option<u64>,
    last_progress_us: u64,
    last_request_us: Option<u64>,
    pub skipped_frames: u64,
    /// (arrival, bytes) over the last `rate_window_us`.
    arrivals: std::collections::VecDeque<(u64, usize)>,
    last_report_us: u64,
    /// (arrival - capture) of recent frames, for the queueing baseline.
    transit: std::collections::VecDeque<(u64, i64)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RecvError {
    FrameTooLarge(usize),
}

impl Receiver {
    pub fn new(cfg: ReceiverConfig) -> Self {
        Self {
            cfg,
            groups: BTreeMap::new(),
            current: None,
            last_progress_us: 0,
            last_request_us: None,
            skipped_frames: 0,
            arrivals: Default::default(),
            last_report_us: 0,
            transit: Default::default(),
        }
    }

    pub fn on_data(&mut self, now_us: u64, group: u64, data: &[u8]) -> Result<(), RecvError> {
        self.arrivals.push_back((now_us, data.len()));
        if self.current.is_some_and(|c| group < c) {
            return Ok(());
        }
        let g = self.groups.entry(group).or_default();
        g.buf.extend_from_slice(data);
        if !g.header && g.buf.len() >= STREAM_HEADER {
            g.header = true;
            g.parsed = STREAM_HEADER;
        }
        while g.header && g.buf.len() - g.parsed >= FRAME_HEADER {
            let p = g.parsed;
            let len = u32::from_be_bytes(g.buf[p..p + 4].try_into().unwrap()) as usize;
            if len > self.cfg.max_frame {
                return Err(RecvError::FrameTooLarge(len));
            }
            if g.buf.len() - p < FRAME_HEADER + len {
                break;
            }
            let capture_us = u64::from_be_bytes(g.buf[p + 4..p + 12].try_into().unwrap());
            let payload = g.buf[p + FRAME_HEADER..p + FRAME_HEADER + len].to_vec();
            let index = g.frames.len() as u32;
            g.frames.push(Frame { group, index, capture_us, payload });
            // Remote clock, so only differences of this are meaningful.
            self.transit.push_back((now_us, now_us as i64 - capture_us as i64));
            while self.transit.front().is_some_and(|t| now_us - t.0 > BASELINE_US) {
                self.transit.pop_front();
            }
            g.parsed += FRAME_HEADER + len;
        }
        if g.parsed > 0 {
            g.buf.drain(..g.parsed);
            g.parsed = 0;
        }
        Ok(())
    }

    pub fn on_finish(&mut self, group: u64) {
        if let Some(g) = self.groups.get_mut(&group) {
            g.finished = true;
        }
    }

    pub fn on_reset(&mut self, group: u64) {
        if let Some(g) = self.groups.get_mut(&group) {
            g.finished = true;
            g.buf.clear();
        }
    }

    /// Next frame to decode, in order. Jumps to a newer group as soon as its keyframe is in and
    /// the current group has nothing ready (finished, reset or stalled), so latency never piles
    /// up behind a stale group.
    pub fn poll(&mut self, now_us: u64) -> Option<Frame> {
        loop {
            let cur = match self.current {
                Some(c) => c,
                None => {
                    // Start at the newest group whose keyframe is complete.
                    let g = *self.groups.iter().rev().find(|(_, g)| !g.frames.is_empty())?.0;
                    self.switch_to(g);
                    g
                }
            };
            let newer = self
                .groups
                .range(cur + 1..)
                .rev()
                .find(|(_, g)| !g.frames.is_empty())
                .map(|(k, _)| *k);
            if let Some(n) = newer {
                // A newer keyframe is in: skip whatever of the current group is still queued.
                if let Some(g) = self.groups.get(&cur) {
                    self.skipped_frames += (g.frames.len() - g.next) as u64;
                }
                self.switch_to(n);
                continue;
            }
            let g = self.groups.get_mut(&cur)?;
            if g.next < g.frames.len() {
                let f = g.frames[g.next].clone();
                g.next += 1;
                self.last_progress_us = now_us;
                return Some(f);
            }
            return None;
        }
    }

    fn switch_to(&mut self, g: u64) {
        self.current = Some(g);
        self.groups = self.groups.split_off(&g);
    }

    /// Rate report to send, at most every [`REPORT_US`].
    pub fn report(&mut self, now_us: u64) -> Option<RateReport> {
        if now_us - self.last_report_us < self.cfg.report_us {
            return None;
        }
        self.last_report_us = now_us;
        let from = now_us.saturating_sub(self.cfg.rate_window_us);
        while self.arrivals.front().is_some_and(|a| a.0 <= from) {
            self.arrivals.pop_front();
        }
        let span = (now_us - from).max(1);
        let bytes: usize = self.arrivals.iter().map(|a| a.1).sum();
        let delivered_bps = (bytes as u64 * 8 * 1_000_000 / span) as u32;
        let min = self.transit.iter().map(|t| t.1).min()?;
        // The least delayed frame of the rate window: a queue delays every frame, while one late
        // frame (a retransmission, an encoder that was preempted) is not a queue.
        let recent = self.transit.iter().rev().take_while(|t| t.0 > from).map(|t| t.1).min();
        let last = recent.unwrap_or(self.transit.back()?.1);
        Some(RateReport { delivered_bps, queuing_us: (last - min) as u64 })
    }

    /// Whether to send a keyframe request now (rate-limited to one per `stall_us`).
    pub fn wants_keyframe(&mut self, now_us: u64) -> bool {
        let stalled = now_us.saturating_sub(self.last_progress_us) > self.cfg.stall_us;
        let recent = self.last_request_us.is_some_and(|t| now_us - t < self.cfg.stall_us);
        if stalled && !recent {
            self.last_request_us = Some(now_us);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(rx: &mut Receiver, outs: &[Out], chunk: usize) {
        for o in outs {
            match o {
                Out::Write { group, data } => {
                    for c in data.chunks(chunk) {
                        rx.on_data(0, *group, c).unwrap();
                    }
                }
                Out::Finish { group } => rx.on_finish(*group),
                Out::Reset { group } => rx.on_reset(*group),
                Out::Open { .. } => {}
            }
        }
    }

    #[test]
    fn frames_survive_any_chunking() {
        for chunk in [1, 3, 12, 1200] {
            let mut tx = Sender::new(SenderConfig::default());
            let mut rx = Receiver::new(ReceiverConfig::default());
            let mut outs = tx.frame(10, true, b"key");
            outs.extend(tx.frame(20, false, b"delta-1"));
            outs.extend(tx.frame(30, false, b""));
            feed(&mut rx, &outs, chunk);
            let got: Vec<_> = std::iter::from_fn(|| rx.poll(0)).collect();
            assert_eq!(got.len(), 3, "chunk {chunk}");
            assert_eq!((got[0].index, got[0].capture_us, &got[0].payload[..]), (0, 10, &b"key"[..]));
            assert_eq!(&got[1].payload[..], b"delta-1");
            assert!(got[2].payload.is_empty());
        }
    }

    #[test]
    fn delta_without_group_is_dropped() {
        let mut tx = Sender::new(SenderConfig::default());
        assert!(tx.wants_keyframe());
        assert!(tx.frame(0, false, b"x").is_empty());
    }

    #[test]
    fn newer_keyframe_skips_stale_group() {
        let mut tx = Sender::new(SenderConfig::default());
        let mut rx = Receiver::new(ReceiverConfig::default());
        let mut outs = tx.frame(0, true, b"k0");
        outs.extend(tx.frame(1, false, b"d1"));
        outs.extend(tx.frame(2, false, b"d2"));
        feed(&mut rx, &outs, 1200);
        assert_eq!(rx.poll(0).unwrap().payload, b"k0");
        feed(&mut rx, &tx.frame(3, true, b"k1"), 1200);
        // Group 1's keyframe is in: the two queued deltas of group 0 are skipped.
        let got: Vec<_> = std::iter::from_fn(|| rx.poll(0)).map(|f| f.payload).collect();
        assert_eq!(got, vec![b"k1".to_vec()]);
        assert_eq!(rx.skipped_frames, 2);
        // Late data for the old group is ignored.
        rx.on_data(0, 0, b"anything").unwrap();
        assert!(rx.poll(0).is_none());
    }

    #[test]
    fn oversized_frame_is_rejected() {
        let mut rx = Receiver::new(ReceiverConfig { max_frame: 100, ..Default::default() });
        let mut d = 0u64.to_be_bytes().to_vec();
        d.extend_from_slice(&1000u32.to_be_bytes());
        d.extend_from_slice(&0u64.to_be_bytes());
        assert_eq!(rx.on_data(0, 0, &d), Err(RecvError::FrameTooLarge(1000)));
    }

    #[test]
    fn backlog_resets_and_forces_keyframe() {
        let mut tx = Sender::new(SenderConfig::default());
        tx.frame(0, true, b"k");
        assert!(!tx.wants_keyframe());
        let before = tx.bitrate();
        // 1 MB unsent at 500 kbps is 16 s of video: way past the 300 ms limit.
        let outs = tx.feedback(&BTreeMap::from([(0, 1_000_000)]));
        assert_eq!(outs, vec![Out::Reset { group: 0 }]);
        assert!(tx.wants_keyframe());
        assert!(tx.bitrate() < before);
    }

    #[test]
    fn rate_follows_queueing_delay() {
        let mut tx = Sender::new(SenderConfig::default());
        let start = tx.bitrate();
        tx.on_report(1_000_000, &RateReport { delivered_bps: 300_000, queuing_us: 100_000 });
        // Estimate: 85% of delivered. 100 ms of queue drains within 500 ms: 80% of delivered.
        assert_eq!(tx.bitrate(), 240_000);
        assert!(tx.draining());
        // Queue gone: back to the estimate, no ramp right after a decrease, then +8% / 100 ms.
        tx.on_report(1_100_000, &RateReport { delivered_bps: 300_000, queuing_us: 0 });
        assert_eq!((tx.bitrate(), tx.draining()), (255_000, false));
        tx.on_report(1_600_000, &RateReport { delivered_bps: 300_000, queuing_us: 0 });
        assert!(tx.bitrate() > 255_000 && tx.bitrate() < start);
    }

    /// One report every 50 ms over `[from, to)`, after sending `sent_bps` worth of frames.
    fn reports(tx: &mut Sender, from: u64, to: u64, sent_bps: u64, r: RateReport) {
        let mut t = from;
        while t < to {
            tx.frame(t, false, &vec![0; (sent_bps / 8 / 20) as usize - FRAME_HEADER]);
            tx.on_report(t, &r);
            t += 50 * MS;
        }
    }

    #[test]
    fn starved_link_drains_before_the_delay_shows() {
        let mut tx = Sender::new(SenderConfig { start_bps: 1_500_000, ..Default::default() });
        tx.frame(0, true, b"k");
        // Queueing between the thresholds: neither ramps nor backs off.
        let steady = RateReport { delivered_bps: 1_500_000, queuing_us: 20_000 };
        reports(&mut tx, 0, 2_000 * MS, 1_500_000, steady);
        assert_eq!((tx.bitrate(), tx.draining()), (1_500_000, false));
        // The link drops to 200 kbps: delivered collapses while queueing still looks fine.
        let slow = RateReport { delivered_bps: 200_000, queuing_us: 20_000 };
        reports(&mut tx, 2_000 * MS, 2_050 * MS, 1_500_000, slow.clone());
        assert!(!tx.draining(), "one report could be a lost packet");
        reports(&mut tx, 2_050 * MS, 2_100 * MS, 1_500_000, slow);
        assert!(tx.draining());
        assert_eq!(tx.bitrate(), 50_000, "25% of delivered while the queue drains");
        assert_eq!(tx.resolution(), (1280, 720), "the ladder waits for a second look");
        reports(&mut tx, 2_100 * MS, 2_250 * MS, 50_000, RateReport { delivered_bps: 200_000, queuing_us: 20_000 });
        assert_eq!(tx.resolution(), (320, 180), "the ladder follows the estimate, 170 kbps");
        assert_eq!(tx.bitrate(), 50_000);
    }

    #[test]
    fn short_delivery_gaps_are_not_starvation() {
        let mut tx = Sender::new(SenderConfig { start_bps: 1_000_000, ..Default::default() });
        tx.frame(0, true, b"k");
        let ok = RateReport { delivered_bps: 1_000_000, queuing_us: 5_000 };
        let gap = RateReport { delivered_bps: 100_000, queuing_us: 5_000 };
        // A stream stalled behind a lost packet every 200 ms; and at the start of the call.
        for i in 0..40u64 {
            let r = if i % 4 == 0 { gap.clone() } else { ok.clone() };
            reports(&mut tx, i * 50 * MS, (i + 1) * 50 * MS, 1_000_000, r);
            assert!(!tx.draining(), "report {i}");
        }
        // Small rates: the excess is below `starve_min_bps`, the delay signal handles those.
        let mut tx = Sender::new(SenderConfig { start_bps: 300_000, ..Default::default() });
        tx.frame(0, true, b"k");
        let low = RateReport { delivered_bps: 100_000, queuing_us: 5_000 };
        reports(&mut tx, 0, 3_000 * MS, 300_000, low);
        assert!(!tx.draining());
    }

    #[test]
    fn no_backlog_reset_while_draining() {
        let mut tx = Sender::new(SenderConfig { start_bps: 1_000_000, ..Default::default() });
        tx.frame(0, true, b"k");
        tx.on_report(1_000_000, &RateReport { delivered_bps: 400_000, queuing_us: 600_000 });
        assert!(tx.draining());
        // 50 KB unacked (the queue in the path), but the receiver is getting data: let it drain.
        assert!(tx.feedback(&BTreeMap::from([(0, 50_000)])).is_empty());
        // Nothing gets through any more: that is a stall, reset.
        tx.on_report(1_050_000, &RateReport { delivered_bps: 0, queuing_us: 600_000 });
        assert_eq!(tx.feedback(&BTreeMap::from([(0, 50_000)])), vec![Out::Reset { group: 0 }]);
    }

    const MS: u64 = 1_000;

    /// A ladder past its encoder warm-up, with enough fast encode times to step up.
    fn ladder(start_bps: u32) -> Ladder {
        let mut l = Ladder::new(LadderConfig::default(), 30, start_bps);
        for _ in 0..ENCODE_WARMUP as usize + ENCODE_MIN {
            l.encode_time(0, 3 * MS);
        }
        l
    }

    /// Encode times of `us` at 30 fps from `*t` while the rung is `rung`; returns when it changed.
    fn encode_at(l: &mut Ladder, t: &mut u64, rung: usize, us: u64) -> u64 {
        while l.index() == rung {
            l.encode_time(*t, us);
            l.update(*t, 5_000_000);
            *t += 33 * MS;
        }
        *t
    }

    /// Feeds three fast encode times (30 fps) and `bps` every 100 ms over `[from, to)`; returns
    /// switches.
    fn hold(l: &mut Ladder, from: u64, to: u64, bps: u32) -> u64 {
        let mut switches = 0;
        let mut t = from;
        while t < to {
            for _ in 0..3 {
                l.encode_time(t, 3 * MS);
            }
            switches += l.update(t, bps) as u64;
            t += 100 * MS;
        }
        switches
    }

    #[test]
    fn ladder_starts_at_the_rung_the_start_rate_covers() {
        assert_eq!(ladder(500_000).index(), 1);
        assert_eq!(ladder(100_000).index(), 0);
        assert_eq!(ladder(5_000_000).index(), 3);
    }

    #[test]
    fn ladder_steps_down_at_once_and_skips_rungs() {
        let mut l = ladder(5_000_000);
        assert!(l.update(0, 200_000));
        assert_eq!((l.rung().width, l.rung().height), (320, 180));
    }

    #[test]
    fn ladder_steps_up_only_after_hold() {
        let mut l = ladder(300_000);
        assert_eq!(l.index(), 1);
        // 700 kbps is above 540p's floor but not its 130% up threshold: stays.
        assert_eq!(hold(&mut l, 0, 10_000 * MS, 700_000), 0);
        // 800 kbps clears it; steps up after the 2.5 s hold, not before.
        assert_eq!(hold(&mut l, 10_000 * MS, 12_400 * MS, 800_000), 0);
        assert_eq!(hold(&mut l, 12_400 * MS, 12_700 * MS, 800_000), 1);
        assert_eq!(l.index(), 2);
        // A dip below the threshold restarts the hold.
        let mut l = ladder(300_000);
        hold(&mut l, 0, 2_000 * MS, 800_000);
        hold(&mut l, 2_000 * MS, 2_100 * MS, 500_000);
        assert_eq!(hold(&mut l, 2_100 * MS, 4_500 * MS, 800_000), 0);
        assert_eq!(hold(&mut l, 4_500 * MS, 4_700 * MS, 800_000), 1);
    }

    #[test]
    fn ladder_waits_after_a_down_switch() {
        let mut l = ladder(300_000);
        hold(&mut l, 0, 1_000 * MS, 300_000);
        assert!(l.update(1_000 * MS, 100_000));
        // The rate recovers at once, but no step up within 4 s of the step down.
        assert_eq!(hold(&mut l, 1_100 * MS, 5_000 * MS, 400_000), 0);
        assert_eq!(hold(&mut l, 5_000 * MS, 5_200 * MS, 400_000), 1);
        assert_eq!(l.index(), 1);
    }

    #[test]
    fn ladder_does_not_flap_around_a_floor() {
        // Bitrate wobbling around 360p's floor: one step down, nothing else.
        let mut l = ladder(300_000);
        let mut switches = 0;
        for i in 0..300u64 {
            let bps = if i % 7 < 3 { 215_000 } else { 285_000 };
            l.encode_time(i * 100 * MS, 3 * MS);
            switches += l.update(i * 100 * MS, bps) as u64;
        }
        assert_eq!(switches, 1);
        assert_eq!(l.index(), 0);
    }

    #[test]
    fn ladder_steps_down_when_encoding_is_too_slow() {
        let mut l = Ladder::new(LadderConfig::default(), 30, 5_000_000);
        assert_eq!(l.index(), 3);
        // 30 fps: budget is 60% of 33.3 ms = 20 ms. 25 ms frames: down after the encoder's
        // warm-up and a full second of them.
        let mut t = 0;
        while l.index() == 3 {
            l.encode_time(t, 25 * MS);
            t += 33 * MS;
        }
        assert_eq!(t, (ENCODE_WARMUP as usize + ENCODE_WINDOW) as u64 * 33 * MS);
        assert_eq!(l.index(), 2);
        // Fast again (throttling ended): 720p stays blocked for 5 s, then returns after the hold.
        let down = t;
        encode_at(&mut l, &mut t, 2, 6 * MS);
        assert!(t - down > 7_400 * MS && t - down < 8_500 * MS, "back up after {} ms", (t - down) / MS);
        assert_eq!(l.index(), 3);
    }

    #[test]
    fn ladder_cpu_block_backs_off() {
        // A device that is too slow for 720p every time it gets there: blocked 5, 10, 20, 30 s.
        let mut l = Ladder::new(LadderConfig::default(), 30, 5_000_000);
        let mut t = 0;
        let mut blocks = vec![];
        for _ in 0..5 {
            let down = encode_at(&mut l, &mut t, 3, 25 * MS);
            let up = encode_at(&mut l, &mut t, 2, 6 * MS);
            blocks.push((up - down) / 1_000_000);
        }
        // Block + 2.5 s hold.
        assert_eq!(blocks, vec![7, 12, 22, 32, 32]);
        // A minute without trouble: back to 5 s.
        encode_at_for(&mut l, &mut t, 70_000 * MS);
        let down = encode_at(&mut l, &mut t, 3, 25 * MS);
        let up = encode_at(&mut l, &mut t, 2, 6 * MS);
        assert_eq!((up - down) / 1_000_000, 7);
    }

    fn encode_at_for(l: &mut Ladder, t: &mut u64, us: u64) {
        let end = *t + us;
        while *t < end {
            l.encode_time(*t, 6 * MS);
            l.update(*t, 5_000_000);
            *t += 33 * MS;
        }
    }

    #[test]
    fn ladder_ignores_preempted_frames() {
        // Fast encodes with one frame in 8 preempted for 40 ms: a smaller picture wouldn't help.
        let mut l = ladder(5_000_000);
        let mut t = 0;
        for i in 0..1800u64 {
            l.encode_time(t, if i % 8 == 0 { 40 * MS } else { 8 * MS });
            l.update(t, 5_000_000);
            t += 33 * MS;
        }
        assert_eq!((l.index(), l.switches), (3, 0));
    }

    #[test]
    fn ladder_predicts_encode_time_before_stepping_up() {
        let mut l = ladder(800_000);
        assert_eq!(l.index(), 2);
        // 540p at 12 ms predicts 720p at ~21 ms, over the 20 ms budget: stays, on any link.
        let mut t = 0;
        for _ in 0..1800 {
            l.encode_time(t, 12 * MS);
            l.update(t, 5_000_000);
            t += 33 * MS;
        }
        assert_eq!((l.index(), l.switches), (2, 0));
        // 8 ms (~14 ms at 720p) fits.
        for _ in 0..150 {
            l.encode_time(t, 8 * MS);
            l.update(t, 5_000_000);
            t += 33 * MS;
        }
        assert_eq!(l.index(), 3);
    }

    #[test]
    fn ladder_respects_max_rung() {
        let mut l = ladder(5_000_000);
        l.set_max(1);
        assert!(l.update(0, 5_000_000));
        assert_eq!(l.index(), 1);
        assert_eq!(hold(&mut l, 0, 60_000 * MS, 5_000_000), 0);
    }

    #[test]
    fn sender_resolution_change_forces_keyframe() {
        let mut tx = Sender::new(SenderConfig { start_bps: 800_000, ..Default::default() });
        assert_eq!(tx.resolution(), (960, 540));
        tx.frame(0, true, b"k");
        assert!(!tx.wants_keyframe());
        // A queue builds and the rate falls below 540p's floor: smaller, from a new keyframe,
        // once the reports over the next 150 ms confirm the rate.
        tx.on_report(1_000_000, &RateReport { delivered_bps: 400_000, queuing_us: 100_000 });
        assert_eq!(tx.resolution(), (960, 540));
        tx.on_report(1_050_000, &RateReport { delivered_bps: 400_000, queuing_us: 100_000 });
        assert_eq!(tx.resolution(), (960, 540));
        tx.on_report(1_100_000, &RateReport { delivered_bps: 400_000, queuing_us: 100_000 });
        assert_eq!(tx.resolution(), (960, 540));
        tx.on_report(1_150_000, &RateReport { delivered_bps: 400_000, queuing_us: 100_000 });
        assert_eq!(tx.resolution(), (640, 360));
        assert!(tx.wants_keyframe());
        assert!(tx.bitrate() <= 1_200_000);
        let outs = tx.frame(33_000, true, b"k2");
        assert_eq!(outs[..2], [Out::Finish { group: 0 }, Out::Open { group: 1 }]);
    }
}
