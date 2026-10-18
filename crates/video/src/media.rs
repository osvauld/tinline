//! Video over QUIC, sans-IO: one unidirectional stream per group of pictures (MoQ style).
//!
//! A group starts with a keyframe and holds every frame up to the next keyframe, so a group can
//! always be decoded on its own and losing one never breaks the next. QUIC retransmits inside a
//! stream, so frames of a group arrive complete and in order; groups are independent, and the
//! transport sends the newest group first. When the link can't keep up the sender gives up on
//! the stale group (stream reset), forces a keyframe and lowers the bitrate.
//!
//! Stream layout: `[group: u64 BE]` then frames `[len: u32 BE][stamp_us: u64 BE][payload]`;
//! `stamp_us` is when the frame was handed to the transport (sender clock; see below).
//! The transport is told what to do through [`Out`] and reports back with plain method calls,
//! so the same code runs on iroh and in [`crate::sim`].
//!
//! Rate control is delay-based (the idea behind WebRTC's GCC), done at this layer so it needs
//! nothing from the transport: the receiver compares each frame's arrival time with the time the
//! sender handed it to the transport. The clock offset between the devices cancels out, so a
//! *rise* over the recent minimum is queueing delay. Not the capture time: then the sender's own
//! camera -> encoder time counts as delay, and a software encoder on a busy machine (40-60 ms
//! late for seconds at a time, 2026-10-17 laptop -> phone call) reads as a queue in the path. It sends a [`RateReport`] every 50 ms; the sender backs off to the
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
            max_bps: 3_000_000,
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

/// How long a block lasts: `first_us`, doubling with every further block up to `max_us`, and
/// back to `first_us` once `forget_us` has passed since the last one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backoff {
    pub first_us: u64,
    pub max_us: u64,
    pub forget_us: u64,
}

#[derive(Clone, Debug)]
pub struct LadderConfig {
    /// Smallest first.
    pub rungs: Vec<Rung>,
    /// Frame-rate tiers, lowest first (tiers above the capture rate are dropped). At `f` fps a
    /// rung's floor is `min_bps x (0.4 + 0.6 x f / 30)`: fewer frames need fewer bits, but not
    /// proportionally fewer, since each frame gets sharper.
    pub tiers: Vec<u32>,
    /// Highest rung index allowed (device or peer capability).
    pub max_rung: usize,
    /// Developer test mode: hold this resolution, ignoring rate/CPU stepping. Caps still win.
    /// None preserves normal adaptation. Does not pin bitrate or upscale camera frames.
    pub fixed_rung: Option<usize>,
    /// Highest frame rate allowed (device or peer capability, `VideoOffer.max_fps`).
    pub max_fps: u32,
    /// Step up once the bitrate is this % of the next step's floor...
    pub up_margin_pct: u32,
    /// ...for this long, below it for no more than `up_dip_pct` % of that in all (a delay burst
    /// every few seconds cuts the rate for a few hundred ms; that is no reason never to step up),
    pub up_hold_us: u64,
    pub up_dip_pct: u32,
    /// and no sooner than this after stepping down.
    pub up_after_down_us: u64,
    /// Until the first step down of the call, a step up needs only this long when what the path
    /// carried with no queue (`Ladder::proven_bps`, measured at the receiver) has stayed at or
    /// above the threshold, no dips allowed. The long hold guards against an estimate that ran
    /// ahead of the link; delivered bytes with a flat delay are the link's own answer.
    pub up_fast_hold_us: u64,
    /// Median encode time over a second above this % of the current tier's frame interval: too
    /// slow for the step. Stepping up needs the p75, scaled to the bigger picture, to fit the
    /// new step's budget.
    pub encode_budget_pct: u32,
    /// After a step down for encode time, the step left (and all above it) stays out of reach.
    pub cpu_block: Backoff,
    /// A step up that a step down for the bitrate undoes within this long...
    pub flap_window_us: u64,
    /// ...is blocked (with the steps above it) for this long.
    pub flap_block: Backoff,
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
            tiers: vec![15, 24, 30],
            max_rung: 3,
            fixed_rung: None,
            max_fps: 30,
            up_margin_pct: 130,
            up_hold_us: 5_000_000,
            up_dip_pct: 25,
            up_after_down_us: 10_000_000,
            up_fast_hold_us: 1_000_000,
            encode_budget_pct: 60,
            cpu_block: Backoff { first_us: 5_000_000, max_us: 30_000_000, forget_us: 60_000_000 },
            flap_window_us: 20_000_000,
            flap_block: Backoff { first_us: 30_000_000, max_us: 120_000_000, forget_us: 300_000_000 },
        }
    }
}

/// See `Sender::starve_after_us`: after a drain, and at the start (first keyframe, ramp).
const STARVE_QUIET_US: u64 = 300_000;
const STARVE_START_US: u64 = 1_000_000;
/// See `Sender::queuing`.
const BASELINE_PROBE_US: u64 = 5_000_000;
const MAX_Q_OFFSET_US: u64 = 100_000;
/// The queue baseline is learned only at or below this rate (1 Mbps, half the old 2 Mbps cap;
/// an absolute value so raising `max_bps` doesn't change behaviour on links under 2 Mbps).
const BASELINE_MAX_BPS: u32 = 1_000_000;
/// After a decrease, the second look comes this much later (three more reports).
const SETTLE_US: u64 = RATE_WINDOW_US + REPORT_US;
/// A keyframe asked of the encoder isn't asked again for this long (a hardware encoder's output
/// lags its input by a few frames; asking on each of them gave 2-5 keyframes per switch).
const KEY_ASK_US: u64 = 500_000;
/// A keyframe request from the receiver or transport within this long of sending a keyframe is
/// for that keyframe, still on its way (about an RTT + 200 ms on a LAN; this layer has no RTT,
/// `video::net` could pass its srtt). Longer swallowed real requests (a decode error right
/// after a keyframe).
const KEY_DEDUPE_US: u64 = 250_000;
/// After a keyframe, the queueing its own burst causes (its size at the current rate, at most
/// `KEY_ALLOWANCE_MAX_US`, falling to nothing over this long) is not overuse.
const KEY_GRACE_US: u64 = 1_000_000;
const KEY_ALLOWANCE_MAX_US: u64 = 200_000;
/// A step down within this long of a starved report is for a link that got slower, not a flap.
const REAL_DROP_US: u64 = 1_000_000;
/// Inside a Wi-Fi spike the delay is growing (a queue of ours) when it passes this % of its
/// lowest so far plus `GROW_US`. A spike's per-frame delay varies 60-100% of its height.
const GROW_PCT: u64 = 150;
const GROW_US: u64 = 20_000;
/// The backlog is the queue in the path, on its way out, while a report within this long
/// showed bytes arriving (a Wi-Fi stall delivers nothing for 50-150 ms, then all of it).
const DELIVERING_US: u64 = 500_000;
/// `Sender::stall_was_spike`: settle reports at this % of the rate before the episode, a burst
/// at this %, a stall this recent.
const SPIKE_NEAR_PCT: u64 = 75;
const SPIKE_BURST_PCT: u64 = 110;
const SPIKE_GAP_PCT: u64 = 10;
const STALL_RECENT_US: u64 = 500_000;
/// After a stall found to be a spike: no starvation verdicts and no further cuts for flat delay
/// for this long (the real call's spikes lasted about a second, and the reports come late).
const SPIKE_HOLD_US: u64 = 1_500_000;
/// ...while the drain stays at or above this % of the rate before the episode: a queue of ours
/// at the link's capacity drains well before that.
const SPIKE_DRAIN_PCT: u64 = 70;
/// A report delivering under this % of the estimate got next to nothing through. Nothing for
/// longer than a lost packet's recovery explains (`STALL_MIN_US`, or 1.5 round trips if more:
/// QUIC finds a loss about an RTT later and resends it), or a report that long after the
/// previous one: a stall (Wi-Fi retries, a delay burst, a handover), not a slower link, which
/// still delivers its rate.
const GAP_PCT: u64 = 1;
const STALL_MIN_US: u64 = 140_000;
const RTT_BUCKET_US: u64 = 5_000_000;
/// How long after its last empty report a stall still explains an overuse episode.
const STALL_HOLD_US: u64 = 1_000_000;
const REPORT_GAP_US: u64 = 150_000;
/// An overuse episode with a stall in it (or just before) cuts the estimate and the drain no
/// lower than this % of the rate before: nothing getting through says nothing about capacity.
const STALL_KEEP_PCT: u64 = 50;
/// After a stall the ramp climbs back to the rate the path carried before at this % per
/// (smoothed) round trip, at least 100 ms (the normal ramp is 8% per 100 ms), starting a round
/// trip (at least 200 ms) after the last decrease: on a 250 ms relay path, a faster climb is
/// ahead of what the reports can show.
const RECOVER_PCT: u64 = 25;
const RECOVER_AFTER_US: u64 = 200_000;
/// Below this rate the ramp's cap looks at the delivered rate over this long.
const RECENT_US: u64 = 500_000;
const LOW_RATE_BPS: u32 = 400_000;
/// Delivered above this % of what we sent: a queue in the path is draining at the link's rate.
const DRAINING_PCT: u64 = 110;

/// Encode times are kept over this long; a step down for encode time needs a full window (a
/// second's worth of frames at the current tier).
const ENCODE_WINDOW_US: u64 = 1_000_000;
/// Samples needed before encode time is trusted for a step up.
const ENCODE_MIN: usize = 10;
/// Frames ignored after a new encoder (start, size change): libaom's first frames are slower,
/// and so is a machine that is just starting the call.
const ENCODE_WARMUP: u32 = 15;

/// Why the ladder last stepped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepReason {
    /// The bitrate estimate.
    Rate,
    /// Encode time.
    Cpu,
    /// A cap: the peer's hint or the device's own limit.
    Cap,
}

/// One ladder step, for logs (`video step 720p30 -> 720p15 reason=rate`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub at_us: u64,
    /// (width, height, fps) before and after.
    pub from: (u32, u32, u32),
    pub to: (u32, u32, u32),
    pub reason: StepReason,
    /// The bitrate estimate the ladder last saw.
    pub bps: u32,
}

/// Steps kept for [`Ladder::take_steps`] if nobody takes them.
const STEP_LOG: usize = 64;

#[derive(Clone, Copy, Debug, Default)]
struct Block {
    len: u64,
    last: u64,
    /// Steps above this are blocked until the time.
    cap: Option<(usize, u64)>,
}

impl Block {
    fn apply(&mut self, b: &Backoff, now_us: u64, cap: usize) {
        self.len = if self.len == 0 || now_us.saturating_sub(self.last) > b.forget_us {
            b.first_us
        } else {
            (self.len * 2).min(b.max_us)
        };
        self.last = now_us;
        self.cap = Some((cap, now_us + self.len));
    }

    fn allows(&self, now_us: u64, step: usize) -> bool {
        !matches!(self.cap, Some((c, until)) if now_us < until && step > c)
    }
}

/// Picks resolution and frame rate from the bitrate and the measured encode time, over steps
/// `(rung, tier)` ordered frame rate first: 180p15 < 180p24 < 180p30 < 360p15 < ... < 720p30.
/// Steps down at once, to the highest step whose floor the bitrate covers (a frame-rate step
/// needs no keyframe); up one step at a time after the bitrate has held above the next step's
/// threshold. A step up in resolution lands at the highest tier the bitrate and encoder allow
/// at the new size, so a better picture never comes with a lower frame rate.
pub struct Ladder {
    cfg: LadderConfig,
    cur: usize,
    above_since: Option<u64>,
    /// Since when `proven_bps` has covered the next step's threshold (see `up_fast_hold_us`).
    fast_since: Option<u64>,
    /// Time spent below the next step's threshold since `above_since`, and the last look.
    dip_us: u64,
    last_look_us: u64,
    last_down_us: Option<u64>,
    /// (time, encode time us) over the last `ENCODE_WINDOW_US` at the current rung, after the
    /// first `warmup` frames.
    encode: std::collections::VecDeque<(u64, u64)>,
    warmup: u32,
    /// Caps from the peer's hint: highest rung, highest fps.
    hint_rung: usize,
    hint_fps: u32,
    cpu: Block,
    flap: Block,
    /// Steps up taken within `flap_window_us`: (step, time, whether it changed the size).
    ups: Vec<(usize, u64, bool)>,
    /// Resolution changes, and frame-rate-only steps.
    pub switches: u64,
    pub fps_steps: u64,
    pub last_reason: Option<StepReason>,
    steps_log: std::collections::VecDeque<Step>,
    last_bps: u32,
    /// Set by the sender while the link has just been seen to get much slower (starved): a
    /// step down then is a real change, not a flap.
    pub real_drop: bool,
    /// Set by the sender: what the path carried lately with no queue (median delivered rate of
    /// the last 0.5 s; 0 while queueing or draining, and for good once an overuse episode has
    /// shown the path's capacity). See `LadderConfig::up_fast_hold_us`.
    pub proven_bps: u32,
}

impl Ladder {
    /// `fps` is the capture rate: no tier above it.
    pub fn new(mut cfg: LadderConfig, fps: u32, start_bps: u32) -> Self {
        assert!(!cfg.rungs.is_empty());
        let fps = fps.max(1);
        cfg.tiers.retain(|&t| t > 0 && t <= fps);
        cfg.tiers.sort_unstable();
        if cfg.tiers.is_empty() {
            cfg.tiers.push(fps);
        }
        let mut l = Self {
            cfg,
            cur: 0,
            above_since: None,
            fast_since: None,
            dip_us: 0,
            last_look_us: 0,
            last_down_us: None,
            encode: Default::default(),
            warmup: ENCODE_WARMUP,
            hint_rung: usize::MAX,
            hint_fps: u32::MAX,
            cpu: Block::default(),
            flap: Block::default(),
            ups: Vec::new(),
            switches: 0,
            fps_steps: 0,
            last_reason: None,
            steps_log: Default::default(),
            last_bps: start_bps,
            real_drop: false,
            proven_bps: 0,
        };
        l.cur = l.start(start_bps);
        l
    }

    /// The highest rung whose floor `bps` covers at the highest allowed frame rate: a call
    /// starts at full frame rate (the frame rate goes down first, but the start isn't a step
    /// down; and keyframes at a lower tier are bigger).
    fn start(&self, bps: u32) -> usize {
        if let Some(k) = self.fixed_step() {
            return k;
        }
        let nt = self.nt();
        let top = (0..nt).rev().find(|&t| self.capped(t)).unwrap_or(0);
        (0..self.cfg.rungs.len())
            .rev()
            .map(|r| r * nt + top)
            .find(|&k| self.capped(k) && self.floor(k) <= bps as u64)
            .unwrap_or(top)
    }

    /// Fixed test size, bounded by device/peer caps and hints, at the highest allowed tier.
    fn fixed_step(&self) -> Option<usize> {
        let rung = self.cfg.fixed_rung?.min(self.cfg.rungs.len() - 1)
            .min(self.cfg.max_rung).min(self.hint_rung);
        (rung * self.nt()..(rung + 1) * self.nt()).rev().find(|&k| self.capped(k))
    }

    fn nt(&self) -> usize {
        self.cfg.tiers.len()
    }

    fn rung_of(&self, k: usize) -> usize {
        k / self.nt()
    }

    fn fps_of(&self, k: usize) -> u32 {
        self.cfg.tiers[k % self.nt()]
    }

    fn pixels_of(&self, k: usize) -> u64 {
        self.cfg.rungs[self.rung_of(k)].pixels()
    }

    /// Lowest bitrate step `k` holds.
    fn floor(&self, k: usize) -> u64 {
        self.cfg.rungs[self.rung_of(k)].min_bps as u64 * (20 + self.fps_of(k) as u64) / 50
    }

    fn budget_us(&self, k: usize) -> u64 {
        1_000_000 * self.cfg.encode_budget_pct as u64 / (100 * self.fps_of(k) as u64)
    }

    pub fn rung(&self) -> Rung {
        self.cfg.rungs[self.rung_of(self.cur)]
    }

    /// Rung index (resolution).
    pub fn index(&self) -> usize {
        self.rung_of(self.cur)
    }

    /// Frame rate of the current tier.
    pub fn fps(&self) -> u32 {
        self.fps_of(self.cur)
    }

    /// Time between encoded frames at the current tier.
    pub fn frame_interval_us(&self) -> u64 {
        1_000_000 / self.fps() as u64
    }

    /// Highest rung index the peer or device can take.
    pub fn set_max(&mut self, max: usize) {
        self.cfg.max_rung = max;
    }

    /// Highest frame rate the peer or device can take.
    pub fn set_max_fps(&mut self, fps: u32) {
        self.cfg.max_fps = fps;
    }

    /// The peer's hint (see [`Hint`]): the highest rung at or below `max_pixels` (`0`: not
    /// shown, keep the lowest rung at the lowest tier), at most `max_fps` (`0`: no limit). A
    /// cap below the current step applies at the next [`Self::update`]; a raised one goes
    /// through the normal rules for stepping up.
    pub fn set_hint(&mut self, max_pixels: u32, max_fps: u8) {
        if max_pixels == 0 {
            (self.hint_rung, self.hint_fps) = (0, self.cfg.tiers[0]);
            return;
        }
        self.hint_rung = self.cfg.rungs.iter().rposition(|r| r.pixels() <= max_pixels as u64).unwrap_or(0);
        self.hint_fps = if max_fps == 0 { u32::MAX } else { max_fps as u32 };
    }

    /// Allowed by the caps (device, peer, hint). The lowest tier is always allowed.
    fn capped(&self, k: usize) -> bool {
        let max_rung = self.cfg.max_rung.min(self.hint_rung);
        let max_fps = self.cfg.max_fps.min(self.hint_fps);
        self.rung_of(k) <= max_rung && (self.fps_of(k) <= max_fps || k.is_multiple_of(self.nt()))
    }

    /// Highest allowed step at or below `to` whose floor `bps` covers.
    fn fit_below(&self, to: usize, bps: u32) -> usize {
        (0..=to).rev().find(|&k| self.capped(k) && self.floor(k) <= bps as u64).unwrap_or(0)
    }

    /// `pct` percentile of the encode times, given at least `min` of them.
    fn encode_pct(&self, pct: usize, min: usize) -> Option<u64> {
        if self.encode.len() < min {
            return None;
        }
        let mut v: Vec<u64> = self.encode.iter().map(|e| e.1).collect();
        v.sort_unstable();
        Some(v[(v.len() - 1) * pct / 100])
    }

    /// p75 of the encode times of the last second at the current size (us), given at least 10;
    /// `None` right after a size change.
    pub fn encode_p75(&self) -> Option<u64> {
        self.encode_pct(75, ENCODE_MIN)
    }

    /// Steps taken since the last call (at most the last 64).
    pub fn take_steps(&mut self) -> Vec<Step> {
        self.steps_log.drain(..).collect()
    }

    fn describe(&self, k: usize) -> (u32, u32, u32) {
        let r = self.cfg.rungs[self.rung_of(k)];
        (r.width, r.height, self.fps_of(k))
    }

    fn set(&mut self, now_us: u64, to: usize, why: StepReason) {
        let resized = self.rung_of(to) != self.rung_of(self.cur);
        if self.steps_log.len() >= STEP_LOG {
            self.steps_log.pop_front();
        }
        let step = Step { at_us: now_us, from: self.describe(self.cur), to: self.describe(to), reason: why, bps: self.last_bps };
        self.steps_log.push_back(step);
        if to < self.cur {
            if why != StepReason::Cap {
                self.last_down_us = Some(now_us);
            }
            // A rung entered in the window and still held (a frame-rate step down inside it) is
            // undone when it is left: that block covers the whole rung.
            let nt = self.nt();
            self.ups.retain(|u| u.0 <= to || (u.2 && u.0 / nt == to / nt));
        } else {
            self.ups.push((to, now_us, resized));
        }
        let window = self.cfg.flap_window_us;
        self.ups.retain(|u| now_us.saturating_sub(u.1) <= window);
        self.cur = to;
        self.above_since = None;
        self.fast_since = None;
        self.dip_us = 0;
        self.last_reason = Some(why);
        if resized {
            self.encode.clear();
            self.warmup = ENCODE_WARMUP;
            self.switches += 1;
        } else {
            self.fps_steps += 1;
        }
    }

    /// One frame took `us` to encode. Returns true if the step changed.
    pub fn encode_time(&mut self, now_us: u64, us: u64) -> bool {
        if self.cfg.fixed_rung.is_some() {
            return false;
        }
        if self.warmup > 0 {
            self.warmup -= 1;
            return false;
        }
        self.encode.push_back((now_us, us));
        while self.encode.front().is_some_and(|e| now_us.saturating_sub(e.0) > ENCODE_WINDOW_US) {
            self.encode.pop_front();
        }
        // Sustained: half the frames of the last second were too slow (a busy machine preempting
        // the odd frame isn't helped by a smaller picture). A second's worth of frames at the
        // current tier, so 15 fps reacts as fast as 30.
        let full = (ENCODE_WINDOW_US * self.fps() as u64 / 1_000_000) as usize;
        let cur = self.cur;
        let Some(median) = self.encode_pct(50, full) else { return false };
        if cur == 0 || median <= self.budget_us(cur) {
            return false;
        }
        // The highest step below that the encoder keeps up with: a lower frame rate first.
        let px = self.pixels_of(cur);
        let to = (0..cur)
            .rev()
            .find(|&k| self.capped(k) && median * self.pixels_of(k) / px <= self.budget_us(k))
            .unwrap_or(0);
        self.cpu.apply(&self.cfg.cpu_block, now_us, cur - 1);
        self.set(now_us, to, StepReason::Cpu);
        true
    }

    /// Re-evaluates for the current target bitrate. Returns true if the step changed.
    pub fn update(&mut self, now_us: u64, bps: u32) -> bool {
        self.update_held(now_us, bps, false)
    }

    /// [`Self::update`], but with `hold` only a lowered cap applies yet.
    pub fn update_held(&mut self, now_us: u64, bps: u32, hold: bool) -> bool {
        self.last_bps = bps;
        if let Some(to) = self.fixed_step() {
            if to != self.cur {
                self.set(now_us, to, StepReason::Cap);
                return true;
            }
            return false;
        }
        if !self.capped(self.cur) {
            let to = self.fit_below(self.cur, bps);
            self.set(now_us, to, StepReason::Cap);
            return true;
        }
        if hold {
            return false;
        }
        let down = self.fit_below(self.cur, bps);
        if down < self.cur {
            // A step up undone this soon: that step (and those above) waits a while. A new size
            // given up again blocks its whole rung: landing at a lower tier of it would be the
            // same keyframe again.
            let window = self.cfg.flap_window_us;
            let undone = self.ups.iter().filter(|u| u.0 > down && now_us - u.1 <= window).min_by_key(|u| u.0);
            if let Some(&(b, _, resized)) = undone.filter(|_| !self.real_drop) {
                let left = resized && self.rung_of(down) < self.rung_of(b);
                let first = if left { self.rung_of(b) * self.nt() } else { b };
                self.flap.apply(&self.cfg.flap_block, now_us, first - 1);
            }
            self.set(now_us, down, StepReason::Rate);
            return true;
        }
        let dt = now_us.saturating_sub(self.last_look_us);
        self.last_look_us = now_us;
        let Some(next) = self.next_up(now_us, bps) else {
            self.fast_since = None;
            // A dip: the hold goes on unless the dips add up to too much of it.
            if self.above_since.is_some() {
                self.dip_us += dt;
                if self.dip_us * 100 > self.cfg.up_hold_us * self.cfg.up_dip_pct as u64 {
                    self.above_since = None;
                    self.dip_us = 0;
                }
            }
            return false;
        };
        let since = *self.above_since.get_or_insert(now_us);
        let settled = self.last_down_us.is_none_or(|t| now_us - t >= self.cfg.up_after_down_us);
        // The call start (no step down yet): the path itself has carried the next step's rate.
        let proven = self.last_down_us.is_none()
            && self.proven_bps > 0
            && self.proven_bps as u64 * 100 >= self.floor(self.rate_step(next)) * self.cfg.up_margin_pct as u64;
        self.fast_since = if proven { self.fast_since.or(Some(now_us)) } else { None };
        let fast = self.fast_since.is_some_and(|t| now_us - t >= self.cfg.up_fast_hold_us);
        if (now_us - since >= self.cfg.up_hold_us && settled) || fast {
            self.set(now_us, next, StepReason::Rate);
            return true;
        }
        false
    }

    /// The step whose floor decides whether `k` can be reached: a new size at a lower tier than
    /// the current one needs its rate at the current tier (see `next_up`).
    fn rate_step(&self, k: usize) -> usize {
        let (nt, cur) = (self.nt(), self.cur);
        if self.rung_of(k) > self.rung_of(cur) && k % nt < cur % nt { self.rung_of(k) * nt + cur % nt } else { k }
    }

    /// The step to go up to if the bitrate and encode time allow it now: the next tier at this
    /// rung, or else the next rung at the highest tier that fits. A new size needs the bitrate
    /// for it at the current frame rate: going down trades frame rate for size, going up never
    /// does for the bitrate (at a lower tier every frame, and the keyframe, is bigger and the
    /// delay signal noisier; 540p30 -> 720p15 at 1 Mbps fell back within 3 s under 3% loss).
    /// Only a slow encoder may land a new size at a lower tier (D1: size before frame rate).
    fn next_up(&self, now_us: u64, bps: u32) -> Option<usize> {
        let (nt, cur) = (self.nt(), self.cur);
        let px = self.pixels_of(cur);
        let p75 = self.encode_pct(75, ENCODE_MIN);
        let ok = |k: usize| {
            let rate_at = self.rate_step(k);
            self.capped(k)
                && self.cpu.allows(now_us, k)
                && self.flap.allows(now_us, k)
                && bps as u64 * 100 >= self.floor(rate_at) * self.cfg.up_margin_pct as u64
                // Would it still encode in time? Assume time grows with pixels.
                && p75.is_some_and(|p| p * self.pixels_of(k) / px <= self.budget_us(k))
        };
        let r = self.rung_of(cur);
        if let Some(k) = (cur + 1..(r + 1) * nt).find(|&k| self.capped(k)) {
            return ok(k).then_some(k);
        }
        if r + 1 >= self.cfg.rungs.len() {
            return None;
        }
        ((r + 1) * nt..(r + 2) * nt).rev().find(|&k| ok(k))
    }
}

/// Receiver -> sender: what is worth sending now (`video::net` message 4). See
/// docs/design/video-adaptation.md.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hint {
    /// Largest picture worth sending (the drawn size, rounded up to a rung); `0`: not shown.
    pub max_pixels: u32,
    /// Frames per second the receiver can decode and show; `0`: no limit.
    pub max_fps: u8,
    /// For the sender's chip and logs only.
    pub reason: HintReason,
    /// The receiver has room for a sharper picture than it gets: it draws the picture at 540p
    /// or more, its decoder would keep up at that size, and nothing of its own (battery,
    /// heat) holds it back. Never a cap and never acted on by itself: the sending app may
    /// suggest sharper video to its person (docs/video.md, "Suggesting sharper video").
    /// Wire: message 5 (`net`), only to a peer whose offer says it reads it.
    pub room: bool,
}

impl Hint {
    pub const NONE: Hint = Hint { max_pixels: u32::MAX, max_fps: 0, reason: HintReason::None, room: false };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum HintReason {
    #[default]
    None = 0,
    DisplaySize = 1,
    DecodeSlow = 2,
    Thermal = 3,
    PowerSave = 4,
    User = 5,
}

impl HintReason {
    /// For logs and the chip.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DisplaySize => "display",
            Self::DecodeSlow => "decode",
            Self::Thermal => "thermal",
            Self::PowerSave => "battery",
            Self::User => "user",
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Self::None,
            1 => Self::DisplaySize,
            2 => Self::DecodeSlow,
            3 => Self::Thermal,
            4 => Self::PowerSave,
            5 => Self::User,
            _ => return None,
        })
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
    /// Resolution change for a cap (the peer's hint).
    pub hint: u64,
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
    Hint,
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
    /// When the encoder was last asked for a keyframe (`poll_keyframe`'s clock).
    key_asked_us: Option<u64>,
    /// When the last keyframe was sent, and until when queueing up to the allowance is its own
    /// burst rather than overuse.
    last_key_us: Option<u64>,
    key_grace: Option<(u64, u64)>,
    /// Capture time from which the next frame is taken (`take_frame`).
    next_take_us: Option<u64>,
    hint: Option<Hint>,
    /// Estimate of what the path carries; drives the ladder.
    bitrate: u32,
    last_decrease_us: u64,
    /// A queue is standing in the path: send well below the delivered rate until it is gone.
    drain: Option<u32>,
    episode: Option<Episode>,
    spike: Option<Spike>,
    /// Last report that delivered under half of the estimate (a stall, or a slower link).
    stall_us: Option<u64>,
    /// Last report that got next to nothing through, or came late (see `GAP_PCT`).
    gap_us: Option<u64>,
    /// Last report with an empty path that carried at least half the estimate.
    healthy_us: Option<u64>,
    /// The rate the path last carried with no queue; lowered when an overuse episode without a
    /// stall shows the capacity is less. After a stall the ramp climbs back to it fast
    /// (`RECOVER_PCT`).
    good: u32,
    /// An overuse episode without a stall has shown the path's capacity: the call start's fast
    /// step-ups are over (see `LadderConfig::up_fast_hold_us`).
    limit_seen: bool,
    /// A backlog reset was made in the current stall.
    stall_reset: bool,
    /// The last report got next to nothing through, and since when that has been so.
    empty_prev: bool,
    empty_since: Option<u64>,
    /// (time, delivered) of the reports of the last `RECENT_US`.
    recent_delivered: std::collections::VecDeque<(u64, u64)>,
    /// The last overuse episode had a stall in it: climb back to `good` fast.
    recover: bool,
    /// Round-trip time from the transport (0: unknown): the lowest of this 5 s bucket and the
    /// one before (a delay burst inflates the smoothed RTT just when the stall rules need the
    /// path's own).
    rtt_us: u64,
    rtt_buckets: (u64, u64, u64),
    /// The smoothed RTT as last given (paces the climb after a stall).
    srtt_us: u64,
    /// Queueing delay at the last cut: an episode cuts again only when it grows past this (a
    /// queue we feed), not while it stays flat (a delay burst, or our drain at work).
    cut_q: u64,
    /// A report within `DELIVERING_US` showed bytes getting through.
    delivering: bool,
    last_delivered_us: Option<u64>,
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
    /// Last report that counted as starved.
    starved_us: Option<u64>,
    ladder: Ladder,
    now_us: u64,
    pub keyframes: u64,
    /// When set, every report with the encoder bitrate and rung after it (for tests).
    pub report_log: Option<Vec<(u64, RateReport, u32, usize)>>,
    pub drops: u64,
    /// Why the rate went where it did (for call logs).
    pub trace: RateTrace,
}

/// From the first overuse report until the queueing delay is under `underuse_us` again.
#[derive(Clone, Copy, Debug)]
struct Episode {
    /// The estimate when it began.
    pre: u32,
    /// Second looks taken in it, and whether the first was put off (see `stall_was_spike`).
    looks: u32,
    put_off: bool,
    /// A stall came with it (see `GAP_PCT`): its cuts keep `STALL_KEEP_PCT` of `pre`, and it is
    /// no evidence about capacity.
    stall: bool,
    /// Cuts taken in it (the second look not counted).
    cuts: u32,
}

/// A Wi-Fi spike under way (see `Sender::stall_was_spike`).
#[derive(Clone, Copy, Debug)]
struct Spike {
    until_us: u64,
    /// Lowest queueing delay since; more than `GROW_PCT` of it is a queue of ours.
    low_q: u64,
    /// The drain goes no lower.
    floor: u32,
}

/// Receiver reports as the rate controller saw them, cumulative over the call (for call logs:
/// a sender stuck at the floor shows which signal keeps cutting it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RateTrace {
    pub reports: u64,
    /// Reports whose queueing delay, less the learned baseline, was above `overuse_us`.
    pub overuse: u64,
    /// Reports that counted as starved (delivered well short of what we sent).
    pub starved: u64,
    /// Cuts the second look found were for a Wi-Fi stall (see `Sender::stall_was_spike`), and
    /// overuse reports held through the spike after one.
    pub spikes: u64,
    pub held: u64,
    /// The last report: raw queueing delay and delivered rate; the learned baseline.
    pub last_queuing_us: u64,
    pub last_delivered_bps: u32,
    pub q_offset_us: u64,
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
            key_asked_us: None,
            last_key_us: None,
            key_grace: None,
            next_take_us: None,
            hint: None,
            bitrate,
            last_decrease_us: 0,
            drain: None,
            episode: None,
            spike: None,
            stall_us: None,
            gap_us: None,
            healthy_us: None,
            good: 0,
            limit_seen: false,
            stall_reset: false,
            empty_prev: false,
            empty_since: None,
            recent_delivered: Default::default(),
            recover: false,
            rtt_us: 0,
            rtt_buckets: (0, u64::MAX, u64::MAX),
            srtt_us: 0,
            cut_q: 0,
            delivering: false,
            last_delivered_us: None,
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
            starved_us: None,
            ladder,
            now_us: 0,
            keyframes: 0,
            report_log: None,
            drops: 0,
            trace: RateTrace::default(),
        }
    }

    /// Target bitrate on the wire (what stats report).
    pub fn bitrate(&self) -> u32 {
        self.drain.map_or(self.bitrate, |d| d.min(self.bitrate))
    }

    /// Bitrate to configure the encoder with. The encoder stays configured at the capture rate
    /// (changing its frame rate re-creates it, a keyframe; MediaCodec can't at all) and budgets
    /// `bitrate / capture fps` per frame, so at a lower tier it gets `bitrate x capture / tier`:
    /// the right per-frame budget, and the target on the wire since only the tier's frames are
    /// encoded. Not while a keyframe is due: encoders size it as a multiple of the per-frame
    /// budget, and at 24 fps a 720p keyframe came out twice the 30 fps one (~55 KB, 220 ms of
    /// queue on a 2 Mbps link) for no better picture.
    pub fn encoder_bitrate(&self) -> u32 {
        if self.wants_keyframe() {
            return self.bitrate();
        }
        let fps = self.ladder.fps().max(1) as u64;
        (self.bitrate() as u64 * self.cfg.fps.max(1) as u64 / fps).min(u32::MAX as u64) as u32
    }

    /// Frames per second to encode now (the ladder's tier).
    pub fn fps(&self) -> u32 {
        self.ladder.fps()
    }

    /// Time between encoded frames at the current tier.
    pub fn frame_interval_us(&self) -> u64 {
        self.ladder.frame_interval_us()
    }

    /// Whether to encode the frame captured at `capture_us` (camera clock, rising): the camera
    /// keeps capturing at its own rate and the encoder skips frames to the tier's rate, on
    /// average exactly (24 of 30 at 24 fps, not every other one).
    pub fn take_frame(&mut self, capture_us: u64) -> bool {
        let iv = self.frame_interval_us();
        // A little early is fine: camera timestamps jitter by a few ms.
        let slack = iv / 6;
        if let Some(due) = self.next_take_us
            && capture_us + slack < due
            && due <= capture_us + 2 * iv
        {
            return false;
        }
        let due = self.next_take_us.unwrap_or(capture_us);
        // Behind by more than a frame (a pause, a slow encoder): restart the schedule here.
        let base = if capture_us >= due + iv || due > capture_us + 2 * iv { capture_us } else { due };
        self.next_take_us = Some(base + iv);
        true
    }

    /// The path's round-trip time (QUIC's smoothed RTT), for timings that must scale with it.
    pub fn set_rtt(&mut self, rtt_us: u64) {
        let (since, prev, cur) = &mut self.rtt_buckets;
        if self.now_us >= *since + RTT_BUCKET_US {
            (*since, *prev, *cur) = (self.now_us, *cur, u64::MAX);
        }
        *cur = (*cur).min(rtt_us);
        self.srtt_us = rtt_us;
        self.rtt_us = (*prev).min(*cur);
    }

    /// The receiver's hint (from `video::net`, or the simulation). A lower cap applies at once
    /// (a frame-rate step without a keyframe), a raised one through the ladder's rules for
    /// stepping up.
    /// A change of [`Hint::room`] alone is only stored (it never steers the ladder).
    pub fn set_hint(&mut self, hint: Hint) {
        let caps = |h: Hint| (h.max_pixels, h.max_fps, h.reason);
        let same_caps = self.hint.is_some_and(|h| caps(h) == caps(hint));
        self.hint = Some(hint);
        if !same_caps {
            self.ladder.set_hint(hint.max_pixels, hint.max_fps);
            self.relayer();
        }
    }

    pub fn hint(&self) -> Option<Hint> {
        self.hint
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

    pub fn ladder_mut(&mut self) -> &mut Ladder {
        &mut self.ladder
    }

    /// The encoder took `us` for the last frame.
    pub fn encode_time(&mut self, now_us: u64, us: u64) {
        self.now_us = self.now_us.max(now_us);
        let rung = self.ladder.index();
        // Only a new size needs a keyframe; a frame-rate step is just fewer frames.
        if self.ladder.encode_time(now_us, us) && self.ladder.index() != rung {
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
        let rung = self.ladder.index();
        // A link that starved just now got slower for real: stepping down isn't a flap.
        self.ladder.real_drop = self.starved_us.is_some_and(|t| self.now_us.saturating_sub(t) < REAL_DROP_US);
        if self.ladder.update_held(self.now_us, self.bitrate, settling) && self.ladder.index() != rung {
            let cap = self.ladder.last_reason == Some(StepReason::Cap);
            self.want(if cap { KeyCause::Hint } else { KeyCause::Rate });
        }
        self.bitrate = self.bitrate.min(self.ladder.rung().max_bps);
    }

    /// The next frame handed to the encoder must be a keyframe.
    pub fn wants_keyframe(&self) -> bool {
        self.want_key || self.current.is_none()
    }

    /// Whether to ask the encoder for a keyframe with the frame captured at `now_us` (any
    /// rising clock). Like [`Self::wants_keyframe`], but once asked not again for
    /// `KEY_ASK_US` while the keyframe is on its way out of the encoder: a hardware encoder's
    /// output lags its input, and asking with every frame meanwhile made one keyframe each.
    pub fn poll_keyframe(&mut self, now_us: u64) -> bool {
        if !self.wants_keyframe() {
            return false;
        }
        if self.key_asked_us.is_some_and(|t| now_us >= t && now_us - t < KEY_ASK_US) {
            return false;
        }
        self.key_asked_us = Some(now_us);
        true
    }

    /// The receiver asked for a keyframe (it stalled or just joined). Ignored within
    /// `KEY_DEDUPE_US` of sending one: that keyframe is still on its way.
    pub fn request_keyframe(&mut self) {
        if self.last_key_us.is_some_and(|t| self.now_us.saturating_sub(t) < KEY_DEDUPE_US) && self.current.is_some() {
            return;
        }
        self.want(KeyCause::Request);
    }

    /// One encoded frame, handed to the transport at `stamp_us` (any rising sender clock; the
    /// receiver measures queueing from it, so not the capture time). Delta frames with no open
    /// group are dropped (they can't be decoded).
    pub fn frame(&mut self, stamp_us: u64, key: bool, payload: &[u8]) -> Vec<Out> {
        let mut out = Vec::new();
        if key {
            let c = &mut self.key_causes;
            match self.key_cause.take() {
                _ if self.keyframes == 0 => c.start += 1,
                Some(KeyCause::Rate) => c.rate += 1,
                Some(KeyCause::Cpu) => c.cpu += 1,
                Some(KeyCause::Hint) => c.hint += 1,
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
            self.key_asked_us = None;
            self.keyframes += 1;
            // Its burst queues for about its size at our rate: not a sign of a slower link.
            let now = self.now_us;
            let burst = (payload.len() as u64 * 8 * 1_000_000 / self.bitrate().max(1) as u64).min(KEY_ALLOWANCE_MAX_US);
            let carry = self.key_allowance(now);
            self.key_grace = Some((now + KEY_GRACE_US, (carry + burst).min(KEY_ALLOWANCE_MAX_US)));
            self.last_key_us = Some(now);
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
        data.extend_from_slice(&stamp_us.to_be_bytes());
        data.extend_from_slice(payload);
        out.push(Out::Write { group: g, data });
        out
    }

    /// A receiver report arrived.
    pub fn on_report(&mut self, now_us: u64, r: &RateReport) {
        self.now_us = self.now_us.max(now_us);
        self.rate(now_us, r);
        self.ladder.proven_bps = self.proven_bps();
        self.relayer();
        let (br, rung) = (self.bitrate(), self.ladder.index());
        if let Some(log) = &mut self.report_log {
            log.push((now_us, r.clone(), br, rung));
        }
    }

    /// What the path carried lately with no queue: the median delivered rate of the last
    /// `RECENT_US`, while no overuse episode, drain or spike is on and the queueing delay is
    /// under `underuse_us`; else 0. Always 0 once the path's capacity has shown (`limit_seen`).
    fn proven_bps(&self) -> u32 {
        if self.limit_seen
            || self.episode.is_some() || self.drain.is_some() || self.spike.is_some() || self.queuing_us >= self.cfg.underuse_us {
            return 0;
        }
        let mut v: Vec<u64> = self.recent_delivered.iter().map(|d| d.1).collect();
        v.sort_unstable();
        v.get(v.len().saturating_sub(1) / 2).map_or(0, |&d| d.min(u32::MAX as u64) as u32)
    }

    /// Queueing the last keyframe's own burst explains: its size at our rate right after it,
    /// down to nothing over `KEY_GRACE_US` (the burst drains; a queue that stays is ours).
    fn key_allowance(&self, now_us: u64) -> u64 {
        match self.key_grace {
            Some((until, a)) if now_us < until => a * (until - now_us) / KEY_GRACE_US,
            _ => 0,
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
        // Only at or below 1 Mbps: higher up, a delay that stays between the thresholds is
        // usually our own standing queue at the link's capacity, and learning it lets it grow.
        if above < self.cfg.underuse_us || self.bitrate > BASELINE_MAX_BPS {
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
        let rtt = self.rtt_us * 3 / 2;
        let late = self.last_report_us.is_some_and(|t| now_us.saturating_sub(t) > REPORT_GAP_US.max(rtt));
        let sent = self.sent_bps(now_us).unwrap_or(0);
        let empty = delivered * 100 < self.bitrate as u64 * GAP_PCT;
        // The first empty report's window began a rate window before it.
        let since = *self.empty_since.get_or_insert(now_us.saturating_sub(RATE_WINDOW_US));
        if !empty {
            self.empty_since = None;
        }
        if late || (empty && now_us - since >= STALL_MIN_US.max(rtt)) {
            self.gap_us = Some(now_us);
        }
        if !empty && !late {
            self.stall_reset = false;
        }
        self.empty_prev = empty;
        let gap_recent = self.gap_us.is_some_and(|t| now_us - t < STALL_RECENT_US);
        self.recent_delivered.push_back((now_us, delivered));
        while self.recent_delivered.front().is_some_and(|d| now_us - d.0 > RECENT_US) {
            self.recent_delivered.pop_front();
        }
        if delivered > 0 {
            self.last_delivered_us = Some(now_us);
        }
        self.delivering = self.last_delivered_us.is_some_and(|t| now_us - t < DELIVERING_US);
        if delivered * 2 < self.bitrate as u64 {
            self.stall_us = Some(now_us);
        }
        let queuing_us = self.queuing(now_us, r.queuing_us);
        self.queuing_us = queuing_us;
        // After a keyframe, the queueing its own burst causes is not overuse.
        let underuse_us = self.cfg.underuse_us;
        let overuse_us = self.cfg.overuse_us + self.key_allowance(now_us);
        let stuck = self.drain.is_some() && sent * 2 >= delivered;
        self.drain_stuck_since = if stuck { self.drain_stuck_since.or(Some(now_us)) } else { None };
        let short = now_us >= self.starve_after_us
            && delivered > 0
            && delivered * 100 < sent * self.cfg.starve_pct as u64
            && sent - delivered >= self.cfg.starve_min_bps as u64;
        self.starved_n = if short { self.starved_n + 1 } else { 0 };
        let starved = self.starved_n >= self.cfg.starve_reports;
        if starved {
            self.starved_us = Some(now_us);
        }
        let over = queuing_us > overuse_us || starved;
        if over && self.episode.is_none() {
            let pre = self.bitrate;
            self.episode = Some(Episode { pre, looks: 0, put_off: false, stall: gap_recent, cuts: 0 });
        }
        // ...for as long as the stall is fresh: a queue still standing `STALL_HOLD_US` (or two
        // round trips) after the last of it is ours, and the episode goes back to the normal
        // rules (a drop that came with a stall, like a handover to a worse cell).
        let stall_fresh = self.gap_us.is_some_and(|t| now_us - t < STALL_HOLD_US.max(2 * self.rtt_us));
        if let Some(ep) = &mut self.episode {
            ep.stall = gap_recent || (ep.stall && stall_fresh);
        }
        // A cut in a stall keeps half the rate before.
        let keep = self.episode.filter(|ep| ep.stall).map_or(0, |ep| (ep.pre as u64 * STALL_KEEP_PCT / 100) as u32);
        let t = &mut self.trace;
        t.reports += 1;
        t.overuse += (queuing_us > overuse_us) as u64;
        t.starved += starved as u64;
        (t.last_queuing_us, t.last_delivered_bps, t.q_offset_us) = (r.queuing_us, r.delivered_bps, self.q_offset);
        if self.settle_until.is_some() {
            self.settle_delivered.push(delivered);
        }
        let mut second_look = self.settle_until.is_some_and(|t| now_us >= t);
        // At the second look: the median of what got through in the reports since the first (a
        // 100 ms window holds three or four packets at 300 kbps, so one report is +-30%, and a
        // stream stalled on a lost packet reads low in one).
        let median = {
            let v = &mut self.settle_delivered;
            v.sort_unstable();
            v.get(v.len().saturating_sub(1) / 2).copied().unwrap_or(delivered)
        };
        let settled = (median * 85 / 100) as u32;
        if second_look && self.put_off_look(now_us) {
            second_look = false;
        }
        if second_look && self.stall_was_spike(now_us) {
            // The cut was for a Wi-Fi stall, not a slower link: keep 15% of it, drop the drain
            // (there's no queue of ours), and hold through the rest of the spike.
            let pre = self.episode.map_or(self.pre_cut, |ep| ep.pre);
            self.settle_until = None;
            second_look = false;
            self.bitrate = self.bitrate.max(pre * 85 / 100).max(self.cfg.min_bps);
            self.drain = None;
            self.starved_n = 0;
            self.starved_us = None;
            self.starve_after_us = self.starve_after_us.max(now_us + SPIKE_HOLD_US);
            self.spike = Some(Spike {
                until_us: now_us + SPIKE_HOLD_US,
                low_q: queuing_us,
                floor: (pre as u64 * SPIKE_DRAIN_PCT / 100) as u32,
            });
            self.trace.spikes += 1;
        }
        if over {
            // Inside a spike (see above), more delay that doesn't grow is still the spike: a
            // drain no lower than the floor, no further cut. Growing delay is a queue we feed.
            if let Some(sp) = &mut self.spike
                && !starved
                && now_us < sp.until_us
                && queuing_us <= sp.low_q * GROW_PCT / 100 + GROW_US
            {
                sp.low_q = sp.low_q.min(queuing_us);
                let floor = sp.floor;
                self.trace.held += 1;
                self.start_drain(delivered, queuing_us, false);
                self.drain = self.drain.map(|d| d.max(floor));
                return;
            }
            self.spike = None;
            // A queue is building: send no faster than what actually gets through, and below it
            // until the queue is gone (each 100 ms of it delays every frame). One rate window
            // after a decrease, look again: the first delivered rate still held bytes from
            // before a drop, and the ladder waits for this one.
            // Later cuts in the episode only while the delay grows or the link delivers more
            // than we send (a queue of ours draining at the link's rate): otherwise delivered
            // just follows our own lowered rate, and cutting to it again and again under a flat
            // delay (a Wi-Fi delay burst) took 1.4 Mbps to 200 kbps in a second.
            let cuts = self.episode.map_or(0, |ep| ep.cuts);
            let growing = starved || queuing_us > self.cut_q + GROW_US || delivered * 100 > sent * DRAINING_PCT;
            let due = now_us - self.last_decrease_us > 200_000 || self.last_decrease_us == 0;
            let cut = second_look || (due && (cuts == 0 || growing));
            if cut {
                let target = (delivered * 85 / 100) as u32;
                if second_look {
                    // Replaces the first cut, which may have been too deep as well (a gap in
                    // what the encoder sent).
                    self.bitrate = self.pre_cut.min(settled).max(keep).max(self.cfg.min_bps);
                    self.settle_until = None;
                    // Queueing while delivering: the path carries no more than this.
                    if keep == 0 {
                        self.good = self.good.min((median as u32).max(self.cfg.min_bps));
                        self.limit_seen = true;
                    }
                } else {
                    if self.settle_until.is_none() {
                        self.pre_cut = self.bitrate;
                        self.settle_until = Some(now_us + SETTLE_US);
                        self.settle_delivered.clear();
                    }
                    self.bitrate = self.bitrate.min(target.max(keep)).max(self.cfg.min_bps);
                    if let Some(ep) = &mut self.episode {
                        ep.cuts += 1;
                    }
                }
                self.cut_q = queuing_us;
                self.last_decrease_us = now_us;
            }
            if cut || growing || self.drain.is_none() {
                self.start_drain(delivered, queuing_us, starved);
            }
            if keep > 0 && !starved {
                self.drain = self.drain.map(|d| d.max(keep));
            }
            return;
        }

        if second_look {
            // The queue was gone a rate window later: a blip, keep no more of the cut than the
            // delivered rate asks for.
            self.bitrate = self.bitrate.max(self.pre_cut.min(settled)).max(keep);
            self.settle_until = None;
        }
        if queuing_us < underuse_us {
            if let Some(ep) = self.episode.take() {
                self.recover = ep.stall;
            }
            self.spike = None;
        }
        if queuing_us < underuse_us && self.drain.take().is_some() {
            self.starve_after_us = now_us + STARVE_QUIET_US;
        }
        let path_ok = self.drain.is_none() && queuing_us < underuse_us;
        if path_ok && delivered * 2 >= self.bitrate as u64 {
            self.healthy_us = Some(now_us);
        }
        let since_cut = now_us - self.last_decrease_us;
        let dt = self.sent_prev.1.min(200_000);
        if self.bitrate >= self.good {
            self.recover = false;
        }
        let srtt = self.srtt_us;
        if path_ok && self.recover && since_cut > RECOVER_AFTER_US.max(srtt) {
            // Back toward the rate the path carried before a stall or a delay burst: fast, and
            // not held to what was delivered (that followed our own lowered rate).
            let per = srtt.max(100_000);
            let up = self.bitrate + (self.bitrate as u64 * RECOVER_PCT * dt / 100 / per) as u32;
            self.bitrate = up.min(self.good).min(self.cfg.max_bps);
        } else if path_ok && since_cut > 500_000 {
            // Path is empty: +8% per 100 ms, never far above what was delivered (at low rates
            // the most of the last half second: at 100-200 kbps a 100 ms window holds one or two
            // packets, and the latest alone held the real call's climb to ~10 kbps a second).
            let top = if self.bitrate < LOW_RATE_BPS {
                self.recent_delivered.iter().map(|d| d.1).max().unwrap_or(delivered).max(delivered)
            } else {
                delivered
            };
            let cap = (top * 3 / 2).max(self.cfg.min_bps as u64) as u32;
            let up = self.bitrate + (self.bitrate as u64 * 8 * dt / 100 / 100_000) as u32;
            self.bitrate = up.min(self.bitrate.max(cap)).min(self.cfg.max_bps);
        }
        if path_ok && self.episode.is_none() {
            self.good = self.good.max(self.bitrate);
        }
    }

    /// At an overuse episode's first second look: was its cut for a Wi-Fi stall rather than a
    /// slower link? The reports since the cut carried the rate from before the episode (two of
    /// them at 75% or more of it), and either one shortly before delivered under half of the
    /// estimate (the stall: a spike begins with 50-150 ms of nothing) or one since delivered
    /// over 110% of it and `starve_min_bps` more (the burst after). A link that got slower, or one we fill to its
    /// capacity, delivers steadily at its rate: no stall, no burst above it.
    /// The first second look comes once more `SETTLE_US` later when a report since the cut
    /// delivered next to nothing (under `SPIKE_GAP_PCT` of the rate before): the stall is still
    /// on and its burst still to come (a 130 ms spike on a link near our rate, a tail loss
    /// waiting for its probe timeout). The ladder waits with it; the drain doesn't.
    fn put_off_look(&mut self, now_us: u64) -> bool {
        let Some(ep) = &mut self.episode else { return false };
        let gap = self.settle_delivered.iter().any(|&d| d * 100 < ep.pre as u64 * SPIKE_GAP_PCT);
        if ep.looks > 0 || ep.put_off || !gap {
            return false;
        }
        ep.put_off = true;
        self.settle_until = Some(now_us + SETTLE_US);
        true
    }

    fn stall_was_spike(&mut self, now_us: u64) -> bool {
        let Some(ep) = &mut self.episode else { return false };
        ep.looks += 1;
        if ep.looks > 1 {
            return false;
        }
        let pre = ep.pre as u64;
        let near = self.settle_delivered.iter().filter(|&&d| d * 100 >= pre * SPIKE_NEAR_PCT).count();
        // By `starve_min_bps` too: at a few hundred kbps one report is +-30% (3-4 packets).
        let excess = self.cfg.starve_min_bps as u64;
        let burst = self.settle_delivered.iter().any(|&d| d * 100 >= pre * SPIKE_BURST_PCT && d >= pre + excess);
        let stalled = self.stall_us.is_some_and(|t| now_us - t < STALL_RECENT_US);
        near >= 2 && (stalled || burst)
    }

    /// Sends below the delivered rate until a queue of `queuing_us` is gone.
    fn start_drain(&mut self, delivered: u64, queuing_us: u64, starved: bool) {
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
        // A stall: reports late or getting nothing through, after a healthy one. Nothing is
        // acked because nothing moves; it all comes out in a burst when the stall ends.
        let now = self.now_us;
        let quiet = self.empty_prev
            || self.last_report_us.is_some_and(|t| now.saturating_sub(t) > REPORT_GAP_US.max(self.rtt_us * 3 / 2))
            || self.gap_us.is_some_and(|t| now.saturating_sub(t) < REPORT_GAP_US);
        let stall_for = self.healthy_us.filter(|_| quiet).map(|t| now.saturating_sub(t));
        // One reset per stall: the next keyframe would queue behind the same stall.
        let again = quiet && self.stall_reset;
        // Frames wait in a queue our rate doesn't drain (a slow decoder): a lower bitrate can't
        // help, only a keyframe the receiver can jump to.
        let stuck = self.drain_stuck_since.is_some_and(|t| self.now_us.saturating_sub(t) > self.cfg.drain_us);
        let stale = stuck && self.queuing_us > self.cfg.max_backlog_us;
        if (backlog_us > self.cfg.max_backlog_us && !draining && !again) || stale {
            // Behind: abandon the current group, restart from a keyframe at a lower rate.
            if let Some(g) = self.current.take() {
                out.push(Out::Reset { group: g });
            }
            self.want(KeyCause::Backlog);
            self.drops += 1;
            if stale {
                self.drain_stuck_since = None;
                self.queuing_us = 0;
            } else if stall_for.is_some() {
                // A stall says nothing about capacity: the rate rules see the reports.
                self.stall_reset = true;
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
    /// The sender's stamp (`Sender::frame`): when it was handed to the transport.
    pub capture_us: u64,
    pub payload: Vec<u8>,
}

/// Receiver -> sender, every [`REPORT_US`] by default.
#[derive(Clone, Debug, PartialEq)]
pub struct RateReport {
    /// Video bytes received over the window, as a rate.
    pub delivered_bps: u32,
    /// (arrival - sender's stamp) above the minimum of the last 10 s, for the least delayed
    /// frame that arrived in the rate window (the latest frame if none did).
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
    /// What the app last said is worth receiving, and whether the sender has it yet.
    hint: Option<Hint>,
    hint_pending: bool,
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
            hint: None,
            hint_pending: false,
        }
    }

    /// What is worth receiving now (drawn size, decode capacity, thermal state). Goes to the
    /// sender through [`Self::take_hint`] once, and again on every change.
    pub fn set_hint(&mut self, hint: Hint) {
        if self.hint != Some(hint) {
            self.hint = Some(hint);
            self.hint_pending = true;
        }
    }

    pub fn hint(&self) -> Option<Hint> {
        self.hint
    }

    /// A hint to send to the sender, if it hasn't had this one.
    pub fn take_hint(&mut self) -> Option<Hint> {
        std::mem::take(&mut self.hint_pending).then_some(self.hint).flatten()
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

    #[test]
    fn fixed_test_rung_ignores_rate_and_cpu_but_obeys_caps() {
        let cfg = LadderConfig { fixed_rung: Some(2), ..Default::default() };
        let mut l = Ladder::new(cfg, 30, 500_000);
        assert_eq!((l.index(), l.fps()), (2, 30));
        assert!(!l.update(1_000_000, 100_000));
        for i in 0..100 { assert!(!l.encode_time(i * 33_333, 90_000)); }
        assert_eq!((l.index(), l.fps()), (2, 30));
        l.set_max(1);
        l.set_max_fps(15);
        assert!(l.update_held(4_000_000, 100_000, true));
        assert_eq!((l.index(), l.fps()), (1, 15));
        l.set_max(3);
        l.set_max_fps(30);
        l.update(5_000_000, 100_000);
        assert_eq!((l.index(), l.fps()), (2, 30));
        l.set_hint(0, 0);
        l.update(6_000_000, 100_000);
        assert_eq!((l.index(), l.fps()), (0, 15));
        l.set_hint(1280 * 720, 24);
        l.update(7_000_000, 100_000);
        assert_eq!((l.index(), l.fps()), (2, 24));
    }

    #[test]
    fn fixed_test_mode_keeps_network_backoff_and_keyframes_on_cap_changes() {
        let cfg = SenderConfig {
            ladder: LadderConfig { fixed_rung: Some(2), ..Default::default() },
            ..Default::default()
        };
        let mut tx = Sender::new(cfg);
        tx.frame(0, true, b"k");
        tx.on_report(1_000_000, &RateReport { delivered_bps: 300_000, queuing_us: 100_000 });
        assert_eq!(tx.bitrate(), 240_000);
        assert!(tx.draining());
        assert_eq!(tx.resolution(), (960, 540));
        assert!(!tx.wants_keyframe(), "a rate drop alone must not resize a fixed test");
        tx.set_hint(Hint { max_pixels: 640 * 360, max_fps: 30, reason: HintReason::DisplaySize, room: false });
        assert_eq!(tx.resolution(), (640, 360));
        assert!(tx.wants_keyframe(), "a safety cap resize still needs a keyframe");
    }

    #[test]
    fn fixed_test_rung_is_bounded_at_start_and_cannot_index_past_ladder() {
        let cfg = LadderConfig { fixed_rung: Some(usize::MAX), max_rung: 1, max_fps: 24, ..Default::default() };
        let l = Ladder::new(cfg, 30, 100_000);
        assert_eq!((l.index(), l.fps()), (1, 24));
    }

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
    fn wifi_stall_is_not_a_slower_link() {
        // Real call (Pixel 3a <-> laptop, one 2.4 GHz router): a second of +40-130 ms delay,
        // starting with a stall and a burst, collapsed 2 Mbps to ~100 kbps and 180p.
        // The real call's sender was capped at 2 Mbps (now 3): pinned, so the numbers below stay the
        // call's own.
        let mut tx = Sender::new(SenderConfig { start_bps: 2_000_000, max_bps: 2_000_000, ..Default::default() });
        tx.frame(0, true, b"k");
        reports(&mut tx, 0, 2_000 * MS, 2_000_000, RateReport { delivered_bps: 2_000_000, queuing_us: 5_000 });
        assert_eq!((tx.bitrate(), tx.resolution()), (2_000_000, (1280, 720)));
        // The stall: next to nothing arrives, which looks like a starved link.
        reports(&mut tx, 2_000 * MS, 2_100 * MS, 2_000_000, RateReport { delivered_bps: 100_000, queuing_us: 5_000 });
        assert!(tx.draining());
        // The burst: what was held arrives late, faster than we send.
        let burst = RateReport { delivered_bps: 2_600_000, queuing_us: 90_000 };
        reports(&mut tx, 2_100 * MS, 2_300 * MS, 2_000_000, burst);
        assert_eq!(tx.trace.spikes, 1);
        // The cut is taken back to 15%; the delay is still up, so a drain, but no deeper than 70%
        // of the rate before (1.4 Mbps).
        assert!((1_400_000..=1_700_000).contains(&tx.bitrate()), "{}", tx.bitrate());
        assert_eq!(tx.resolution(), (1280, 720), "the ladder never stepped");
        // The rest of the spike: the delay stays up, the link carries what we send.
        let flat = RateReport { delivered_bps: 1_700_000, queuing_us: 100_000 };
        reports(&mut tx, 2_300 * MS, 3_100 * MS, 1_700_000, flat);
        assert!(tx.trace.held > 0);
        assert!(tx.bitrate() >= 1_400_000, "the drain stops at 70% of the rate before: {}", tx.bitrate());
        assert_eq!(tx.resolution(), (1280, 720));
        // Delay back to normal: the drain ends and the rate climbs from 1.7 Mbps.
        reports(&mut tx, 3_100 * MS, 4_500 * MS, 1_700_000, RateReport { delivered_bps: 1_700_000, queuing_us: 5_000 });
        assert!(!tx.draining());
        assert!(tx.bitrate() > 1_700_000, "{}", tx.bitrate());
    }

    #[test]
    fn delay_burst_is_not_a_slower_link() {
        // Real call (Pixel 3a <-> laptop, home LAN, 2026-10-17): a few delay bursts a minute
        // (nothing for a few hundred ms, then everything late) held both ends at 100-190 kbps.
        let mut tx = Sender::new(SenderConfig { start_bps: 1_500_000, ..Default::default() });
        tx.frame(0, true, b"k");
        reports(&mut tx, 0, 2_000 * MS, 1_500_000, RateReport { delivered_bps: 1_500_000, queuing_us: 5_000 });
        let (rate, size) = (tx.bitrate(), tx.resolution());
        // Nothing gets through for 300 ms; the backlog reset doesn't halve the rate.
        reports(&mut tx, 2_000 * MS, 2_300 * MS, rate as u64, RateReport { delivered_bps: 0, queuing_us: 5_000 });
        tx.feedback(&BTreeMap::from([(0, 200_000)]));
        assert_eq!(tx.drops, 1);
        assert!(tx.feedback(&BTreeMap::from([(0, 200_000)])).is_empty(), "one reset per stall");
        // Then it all arrives, late.
        let late = RateReport { delivered_bps: rate, queuing_us: 250_000 };
        reports(&mut tx, 2_300 * MS, 2_700 * MS, rate as u64 / 2, late);
        assert!(tx.bitrate() >= rate / 2, "a stall keeps half the rate: {}", tx.bitrate());
        // The delay is back to normal: back to the rate before within a second.
        let ok = RateReport { delivered_bps: rate / 2, queuing_us: 5_000 };
        reports(&mut tx, 2_700 * MS, 3_700 * MS, rate as u64 / 2, ok);
        assert!(tx.bitrate() >= rate * 9 / 10, "{} of {rate}", tx.bitrate());
        assert_eq!(tx.resolution(), size);
    }

    #[test]
    fn slower_link_is_not_remembered() {
        // A queue that builds while the link delivers steadily is the link's capacity: the
        // ramp after it is the normal one, not the fast climb back after a stall.
        let mut tx = Sender::new(SenderConfig { start_bps: 1_500_000, ..Default::default() });
        tx.frame(0, true, b"k");
        reports(&mut tx, 0, 2_000 * MS, 1_500_000, RateReport { delivered_bps: 1_500_000, queuing_us: 5_000 });
        for i in 0..10u64 {
            let r = RateReport { delivered_bps: 500_000, queuing_us: 50_000 + i * 15_000 };
            reports(&mut tx, (2_000 + i * 50) * MS, (2_050 + i * 50) * MS, 500_000, r);
        }
        assert!(tx.bitrate() < 500_000);
        let ok = RateReport { delivered_bps: 450_000, queuing_us: 5_000 };
        reports(&mut tx, 2_500 * MS, 3_500 * MS, 450_000, ok);
        assert!(tx.bitrate() < 800_000, "{}", tx.bitrate());
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
        // A report with nothing delivered is a Wi-Fi stall as often as not (50-150 ms, then a
        // burst): no reset yet.
        tx.on_report(1_050_000, &RateReport { delivered_bps: 0, queuing_us: 600_000 });
        assert!(tx.feedback(&BTreeMap::from([(0, 50_000)])).is_empty());
        // Nothing for half a second: the receiver stalled, reset.
        tx.on_report(1_500_000, &RateReport { delivered_bps: 0, queuing_us: 600_000 });
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

    /// (rung index, fps).
    fn at(l: &Ladder) -> (usize, u32) {
        (l.index(), l.fps())
    }

    /// Encode times of `us` at 30 fps from `*t` while the step is `step`; returns when it changed.
    fn encode_at(l: &mut Ladder, t: &mut u64, step: usize, us: u64) -> u64 {
        while l.cur == step {
            l.encode_time(*t, us);
            l.update(*t, 5_000_000);
            *t += 33 * MS;
        }
        *t
    }

    /// Feeds three fast encode times (30 fps) and `bps` every 100 ms over `[from, to)`; returns
    /// steps.
    fn hold(l: &mut Ladder, from: u64, to: u64, bps: u32) -> u64 {
        let mut steps = 0;
        let mut t = from;
        while t < to {
            for _ in 0..3 {
                l.encode_time(t, 3 * MS);
            }
            steps += l.update(t, bps) as u64;
            t += 100 * MS;
        }
        steps
    }

    #[test]
    fn ladder_starts_at_the_step_the_start_rate_covers() {
        // At full frame rate: 500 kbps would cover 540p15 (420 kbps), but a call starts at 30.
        assert_eq!(at(&ladder(500_000)), (1, 30));
        assert_eq!(at(&ladder(300_000)), (1, 30));
        assert_eq!(at(&ladder(100_000)), (0, 30));
        assert_eq!(at(&ladder(5_000_000)), (3, 30));
    }

    #[test]
    fn ladder_floors_scale_with_frame_rate() {
        let l = ladder(0);
        // 720p: 1 Mbps at 30 fps, 88% at 24, 70% at 15.
        let k = |rung: usize, tier: usize| rung * 3 + tier;
        assert_eq!((l.floor(k(3, 2)), l.floor(k(3, 1)), l.floor(k(3, 0))), (1_000_000, 880_000, 700_000));
        // The step order is also the floor order.
        assert!((1..4 * 3).all(|i| l.floor(i) >= l.floor(i - 1)));
    }

    #[test]
    fn ladder_steps_down_at_once_frame_rate_first() {
        let mut l = ladder(5_000_000);
        // 900 kbps: 720p24 (880 kbps), a frame-rate step.
        assert!(l.update(0, 900_000));
        assert_eq!((at(&l), l.switches, l.fps_steps), ((3, 24), 0, 1));
        // 200 kbps skips to 360p15 (175 kbps), 150 kbps to the bottom rung.
        assert!(l.update(0, 200_000));
        assert_eq!(at(&l), (1, 15));
        assert!(l.update(0, 150_000));
        assert_eq!((l.rung().width, l.rung().height), (320, 180));
    }

    #[test]
    fn ladder_steps_up_only_after_hold() {
        let mut l = ladder(300_000);
        assert_eq!(at(&l), (1, 30));
        // 700 kbps is above 540p30's floor (600k), and would hold 540p24 on the way down, but
        // a new size needs its 130% up threshold at the current 30 fps (780k): stays.
        assert_eq!(hold(&mut l, 0, 10_000 * MS, 700_000), 0);
        // 800 kbps clears it; steps up after the 5 s hold, not before.
        assert_eq!(hold(&mut l, 10_000 * MS, 14_900 * MS, 800_000), 0);
        assert_eq!(hold(&mut l, 14_900 * MS, 15_200 * MS, 800_000), 1);
        assert_eq!(at(&l), (2, 30));
        // A short dip below the threshold (a delay burst's cut) doesn't restart the hold...
        let mut l = ladder(300_000);
        hold(&mut l, 0, 2_000 * MS, 800_000);
        hold(&mut l, 2_000 * MS, 2_400 * MS, 700_000);
        assert_eq!(hold(&mut l, 2_400 * MS, 5_200 * MS, 800_000), 1);
        // ...dips adding up to more than 25% of it do.
        let mut l = ladder(300_000);
        hold(&mut l, 0, 2_000 * MS, 800_000);
        hold(&mut l, 2_000 * MS, 2_800 * MS, 700_000);
        hold(&mut l, 2_800 * MS, 3_500 * MS, 800_000);
        hold(&mut l, 3_500 * MS, 4_200 * MS, 700_000);
        assert_eq!(hold(&mut l, 4_200 * MS, 9_000 * MS, 800_000), 0);
        assert_eq!(hold(&mut l, 9_000 * MS, 9_400 * MS, 800_000), 1);
    }

    #[test]
    fn ladder_steps_up_fast_on_proven_rate_at_the_start() {
        // The path carried 800 kbps with no queue: 540p after the 1 s fast hold, not 5 s.
        let mut l = ladder(300_000);
        l.proven_bps = 800_000;
        assert_eq!(hold(&mut l, 0, 900 * MS, 800_000), 0);
        assert_eq!(hold(&mut l, 900 * MS, 1_200 * MS, 800_000), 1);
        assert_eq!(at(&l), (2, 30));
        // The estimate alone isn't enough: under the threshold on the wire, the long hold.
        let mut l = ladder(300_000);
        l.proven_bps = 700_000;
        assert_eq!(hold(&mut l, 0, 4_900 * MS, 800_000), 0);
        // Any dip of the proven rate restarts the fast hold.
        let mut l = ladder(300_000);
        l.proven_bps = 800_000;
        hold(&mut l, 0, 800 * MS, 800_000);
        l.proven_bps = 0;
        hold(&mut l, 800 * MS, 900 * MS, 800_000);
        l.proven_bps = 800_000;
        assert_eq!(hold(&mut l, 900 * MS, 1_800 * MS, 800_000), 0);
        assert_eq!(hold(&mut l, 1_800 * MS, 2_000 * MS, 800_000), 1);
        // After the call's first step down the normal holds apply again.
        let mut l = ladder(5_000_000);
        l.proven_bps = 5_000_000;
        assert!(l.update(0, 900_000));
        assert_eq!(hold(&mut l, 100 * MS, 9_900 * MS, 2_000_000), 0);
    }

    #[test]
    fn ladder_resolution_step_up_keeps_the_frame_rate_it_can() {
        // 360p30 with plenty of rate: the next rung at 30 fps, not at 15.
        let mut l = ladder(300_000);
        assert_eq!(hold(&mut l, 0, 5_200 * MS, 2_000_000), 1);
        assert_eq!(at(&l), (2, 30));
        // At 15 fps the tiers come back one at a time (10 s after the step down).
        let mut l = ladder(5_000_000);
        assert!(l.update(0, 450_000));
        assert_eq!(at(&l), (2, 15));
        assert_eq!(hold(&mut l, 0, 10_200 * MS, 2_000_000), 1);
        assert_eq!(at(&l), (2, 24));
        // A slow encoder may land a new size at a lower tier: 360p at 9 ms predicts 540p at
        // ~20 ms, over 30 fps's 20 ms... (see ladder_predicts_encode_time_before_stepping_up)
    }

    #[test]
    fn ladder_waits_after_a_down_switch() {
        let mut l = ladder(300_000);
        hold(&mut l, 0, 1_000 * MS, 300_000);
        assert!(l.update(1_000 * MS, 100_000));
        assert_eq!(at(&l), (0, 30));
        // The rate recovers at once, but no step up within 10 s of the step down.
        assert_eq!(hold(&mut l, 1_100 * MS, 11_000 * MS, 400_000), 0);
        assert_eq!(hold(&mut l, 11_000 * MS, 11_200 * MS, 400_000), 1);
        assert_eq!(at(&l), (1, 30));
    }

    #[test]
    fn ladder_does_not_flap_around_a_floor() {
        // Bitrate wobbling around 360p's floor: one frame-rate step down, the size stays.
        let mut l = ladder(300_000);
        let mut steps = 0;
        for i in 0..300u64 {
            let bps = if i % 7 < 3 { 215_000 } else { 285_000 };
            l.encode_time(i * 100 * MS, 3 * MS);
            steps += l.update(i * 100 * MS, bps) as u64;
        }
        assert_eq!(steps, 1);
        assert_eq!((at(&l), l.switches), ((1, 15), 0));
    }

    #[test]
    fn ladder_flap_block() {
        // 540p30 -> 720p30, undone by the rate 3 s later: 720p30 is blocked 30 s (then the
        // 5 s hold), where the plain rules would have gone back up after 10 s.
        let mut l = ladder(650_000);
        assert_eq!(at(&l), (2, 30));
        assert_eq!(hold(&mut l, 0, 5_100 * MS, 2_000_000), 1);
        assert_eq!(at(&l), (3, 30));
        assert!(l.update(8_000 * MS, 650_000));
        assert_eq!(at(&l), (2, 30));
        assert_eq!(hold(&mut l, 8_000 * MS, 42_900 * MS, 2_000_000), 0);
        assert_eq!(hold(&mut l, 42_900 * MS, 43_100 * MS, 2_000_000), 1);
        assert_eq!(at(&l), (3, 30));
        // Again: 60 s.
        assert!(l.update(45_000 * MS, 650_000));
        assert_eq!(hold(&mut l, 45_000 * MS, 109_900 * MS, 2_000_000), 0);
        assert_eq!(hold(&mut l, 109_900 * MS, 110_100 * MS, 2_000_000), 1);
        // A step up that holds longer than 20 s isn't a flap: back to the plain 10 s.
        assert!(l.update(140_000 * MS, 650_000));
        assert_eq!(hold(&mut l, 140_000 * MS, 149_900 * MS, 2_000_000), 0);
        assert_eq!(hold(&mut l, 149_900 * MS, 150_100 * MS, 2_000_000), 1);
        // A link that really got slower (the sender saw it starve) isn't a flap either.
        l.real_drop = true;
        assert!(l.update(152_000 * MS, 650_000));
        l.real_drop = false;
        assert_eq!(hold(&mut l, 152_000 * MS, 161_900 * MS, 2_000_000), 0);
        assert_eq!(hold(&mut l, 161_900 * MS, 162_100 * MS, 2_000_000), 1);
    }

    #[test]
    fn ladder_drops_frame_rate_when_encoding_is_too_slow() {
        let mut l = Ladder::new(LadderConfig::default(), 30, 5_000_000);
        assert_eq!(at(&l), (3, 30));
        // 30 fps: budget is 60% of 33.3 ms = 20 ms. 25 ms frames: down after the encoder's
        // warm-up and a full second of them, to 24 fps (25 ms budget), same size.
        let mut t = 0;
        let start = l.cur;
        encode_at(&mut l, &mut t, start, 25 * MS);
        assert_eq!(t, (ENCODE_WARMUP as u64 + 30) * 33 * MS);
        assert_eq!((at(&l), l.switches), ((3, 24), 0));
        // Fast again (throttling ended): 720p30 stays blocked 5 s, and no step up within 10 s of
        // the step down.
        let (down, step) = (t, l.cur);
        encode_at(&mut l, &mut t, step, 6 * MS);
        assert!(t - down > 9_900 * MS && t - down < 10_500 * MS, "back up after {} ms", (t - down) / MS);
        assert_eq!(at(&l), (3, 30));
    }

    #[test]
    fn ladder_drops_size_when_even_15_fps_is_too_slow() {
        let mut l = Ladder::new(LadderConfig::default(), 30, 5_000_000);
        let mut t = 0;
        let start = l.cur;
        // 45 ms at 720p: over 15 fps's 40 ms; 540p would take ~25 ms, which fits 15 fps only.
        encode_at(&mut l, &mut t, start, 45 * MS);
        assert_eq!((at(&l), l.switches), ((2, 15), 1));
    }

    #[test]
    fn ladder_cpu_block_backs_off() {
        // A device that is too slow for 720p30 every time it gets there: blocked 5, 10, 20, 30 s;
        // back up after the block and the 5 s hold, at the earliest 10 s after the step down.
        let mut l = Ladder::new(LadderConfig::default(), 30, 5_000_000);
        let top = l.cur;
        let mut t = 0;
        let mut blocks = vec![];
        for _ in 0..5 {
            let down = encode_at(&mut l, &mut t, top, 25 * MS);
            let up = encode_at(&mut l, &mut t, top - 1, 6 * MS);
            blocks.push((up - down) / 1_000_000);
        }
        assert_eq!(blocks, vec![10, 15, 25, 35, 35]);
        // A minute without trouble: back to 5 s.
        encode_at_for(&mut l, &mut t, 70_000 * MS);
        let down = encode_at(&mut l, &mut t, top, 25 * MS);
        let up = encode_at(&mut l, &mut t, top - 1, 6 * MS);
        assert_eq!((up - down) / 1_000_000, 10);
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
        assert_eq!((at(&l), l.switches, l.fps_steps), ((3, 30), 0, 0));
    }

    #[test]
    fn ladder_predicts_encode_time_before_stepping_up() {
        // Encode time grows with pixels: `us_540` at 540p.
        let run = |us_540: u64| {
            let mut l = ladder(650_000);
            assert_eq!(at(&l), (2, 30));
            let mut t = 0;
            for _ in 0..1800 {
                l.encode_time(t, us_540 * l.rung().pixels() / (960 * 540));
                l.update(t, 5_000_000);
                t += 33 * MS;
            }
            (at(&l), l.switches + l.fps_steps)
        };
        // 540p at 12 ms predicts 720p at ~21 ms: over 30 fps's 20 ms, within 24 fps's 25 ms.
        assert_eq!(run(12 * MS), ((3, 24), 1));
        // 8 ms (~14 ms at 720p) fits 30 fps.
        assert_eq!(run(8 * MS), ((3, 30), 1));
    }

    #[test]
    fn ladder_respects_max_rung() {
        let mut l = ladder(5_000_000);
        l.set_max(1);
        assert!(l.update(0, 5_000_000));
        assert_eq!(at(&l), (1, 30));
        assert_eq!(hold(&mut l, 0, 60_000 * MS, 5_000_000), 0);
    }

    #[test]
    fn ladder_hint_caps() {
        let mut l = ladder(5_000_000);
        // An fps cap applies at once, even while the rate is being looked at again.
        l.set_hint(u32::MAX, 15);
        assert!(l.update_held(0, 5_000_000, true));
        assert_eq!((at(&l), l.last_reason), ((3, 15), Some(StepReason::Cap)));
        // Not shown: the bottom rung at the bottom tier, and not a step "down" for the rules.
        l.set_hint(0, 0);
        assert!(l.update(0, 5_000_000));
        assert_eq!(at(&l), (0, 15));
        // A 640x360 picture: up to 360p30 a step every 5 s (no extra wait after a cap).
        l.set_hint(640 * 360, 0);
        assert_eq!(hold(&mut l, 0, 4_900 * MS, 5_000_000), 0);
        // (The first step waits for 25 encode times at the new size: warm-up and the minimum.)
        let n = hold(&mut l, 4_900 * MS, 16_500 * MS, 5_000_000);
        assert_eq!(n, 3, "{:?}", l.take_steps());
        assert_eq!(at(&l), (1, 30));
        assert_eq!(hold(&mut l, 5_100 * MS, 60_000 * MS, 5_000_000), 0);
    }

    #[test]
    fn sender_resolution_change_forces_keyframe() {
        let mut tx = Sender::new(SenderConfig { start_bps: 650_000, ..Default::default() });
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

    #[test]
    fn sender_frame_rate_step_needs_no_keyframe() {
        let mut tx = Sender::new(SenderConfig { start_bps: 1_500_000, ..Default::default() });
        assert_eq!((tx.resolution(), tx.fps()), ((1280, 720), 30));
        tx.frame(0, true, b"k");
        // Settles at 85% of 1.1 Mbps = 935 kbps: 720p24.
        for i in 0..4 {
            tx.on_report(1_000_000 + i * 50_000, &RateReport { delivered_bps: 1_100_000, queuing_us: 100_000 });
        }
        assert_eq!((tx.resolution(), tx.fps()), ((1280, 720), 24));
        assert!(!tx.wants_keyframe());
        // The encoder (configured for 30 fps) gets the per-frame budget of 24 fps.
        assert_eq!(tx.encoder_bitrate() as u64, tx.bitrate() as u64 * 30 / 24);
    }

    #[test]
    fn sender_takes_frames_at_the_tier_rate() {
        let count = |hint_fps: u8| {
            let mut tx = Sender::new(SenderConfig { start_bps: 1_500_000, ..Default::default() });
            tx.set_hint(Hint { max_pixels: u32::MAX, max_fps: hint_fps, reason: HintReason::DecodeSlow, room: false });
            // Camera at 30 fps with +-3 ms of timestamp jitter, 10 s.
            (0..300u64).filter(|i| tx.take_frame(10_000 + i * 33_333 + (i * 37 % 7) * 1_000 - 3_000)).count()
        };
        assert_eq!(count(0), 300);
        assert!((239..=241).contains(&count(24)), "{}", count(24));
        assert!((149..=151).contains(&count(15)), "{}", count(15));
    }

    #[test]
    fn sender_hint_caps() {
        let mut tx = Sender::new(SenderConfig { start_bps: 1_500_000, ..Default::default() });
        tx.frame(0, true, b"k");
        // A frame-rate cap: no keyframe.
        tx.set_hint(Hint { max_pixels: u32::MAX, max_fps: 15, reason: HintReason::Thermal, room: false });
        assert_eq!((tx.resolution(), tx.fps(), tx.wants_keyframe()), ((1280, 720), 15, false));
        // A size cap: a keyframe at the new size.
        tx.set_hint(Hint { max_pixels: 640 * 360, max_fps: 0, reason: HintReason::DisplaySize, room: false });
        assert_eq!((tx.resolution(), tx.fps(), tx.wants_keyframe()), ((640, 360), 30, true));
        tx.frame(10_000, true, b"k");
        assert_eq!(tx.key_causes.hint, 1);
        // Hidden: 180p15 (D4).
        tx.set_hint(Hint { max_pixels: 0, max_fps: 0, reason: HintReason::DisplaySize, room: false });
        assert_eq!((tx.resolution(), tx.fps()), ((320, 180), 15));
    }

    #[test]
    fn receiver_sends_each_hint_once() {
        let mut rx = Receiver::new(ReceiverConfig::default());
        assert_eq!(rx.take_hint(), None);
        let h = Hint { max_pixels: 640 * 360, max_fps: 24, reason: HintReason::DisplaySize, room: false };
        rx.set_hint(h);
        assert_eq!(rx.take_hint(), Some(h));
        assert_eq!(rx.take_hint(), None);
        rx.set_hint(h);
        assert_eq!(rx.take_hint(), None, "unchanged");
        rx.set_hint(Hint { max_fps: 15, ..h });
        assert_eq!(rx.take_hint().map(|h| h.max_fps), Some(15));
        assert_eq!(HintReason::from_u8(3), Some(HintReason::Thermal));
        assert_eq!(HintReason::from_u8(9), None);
    }

    #[test]
    fn keyframe_asked_once_while_the_encoder_lags() {
        let mut tx = Sender::new(SenderConfig::default());
        assert!(tx.poll_keyframe(0));
        // The encoder's output lags: the next frames aren't asked again...
        assert!(!tx.poll_keyframe(33_000));
        assert!(!tx.poll_keyframe(66_000));
        // ...unless the keyframe never came.
        assert!(tx.poll_keyframe(600_000));
        tx.encode_time(600_000, 1_000);
        tx.frame(600_000, true, b"k");
        assert!(!tx.poll_keyframe(633_000));
        // A request right after the keyframe is for that keyframe; later ones count.
        tx.encode_time(700_000, 1_000);
        tx.request_keyframe();
        assert!(!tx.wants_keyframe());
        tx.encode_time(900_000, 1_000);
        tx.request_keyframe();
        assert!(tx.poll_keyframe(900_000));
    }

    #[test]
    fn keyframe_burst_is_not_overuse() {
        let mut tx = Sender::new(SenderConfig { start_bps: 1_000_000, ..Default::default() });
        tx.encode_time(2_000_000, 1_000);
        // 12 KB at 1 Mbps queues ~96 ms behind it.
        tx.frame(2_000_000, true, &[0; 12_000]);
        let r = RateReport { delivered_bps: 1_000_000, queuing_us: 90_000 };
        tx.on_report(2_050_000, &r);
        assert!(!tx.draining());
        assert_eq!(tx.trace.overuse, 0);
        // Half a second later the burst has drained: a queue that stays is overuse.
        tx.on_report(2_500_000, &r);
        assert!(tx.draining());
    }
}
