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
//! is queueing delay. It sends a [`RateReport`] every 100 ms; the sender backs off to the
//! delivered rate as soon as a queue builds and only ramps up while the path is empty. A
//! loss-based congestion controller alone fills the bottleneck buffer first (bufferbloat), which
//! the simulation showed as seconds of delay.

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
        }
    }
}

pub struct Sender {
    cfg: SenderConfig,
    next_group: u64,
    /// Group currently being written, if any.
    current: Option<u64>,
    /// Older groups whose streams the transport may still be sending.
    previous: Vec<u64>,
    want_key: bool,
    bitrate: u32,
    last_decrease_us: u64,
    pub keyframes: u64,
    pub drops: u64,
}

impl Sender {
    pub fn new(cfg: SenderConfig) -> Self {
        let bitrate = cfg.start_bps;
        Self {
            cfg,
            next_group: 0,
            current: None,
            previous: Vec::new(),
            want_key: true,
            bitrate,
            last_decrease_us: 0,
            keyframes: 0,
            drops: 0,
        }
    }

    /// Encoder target to use for the next frame.
    pub fn bitrate(&self) -> u32 {
        self.bitrate
    }

    /// The next frame handed to the encoder must be a keyframe.
    pub fn wants_keyframe(&self) -> bool {
        self.want_key || self.current.is_none()
    }

    /// The receiver asked for a keyframe (it stalled or just joined).
    pub fn request_keyframe(&mut self) {
        self.want_key = true;
    }

    /// One encoded frame. Delta frames with no open group are dropped (they can't be decoded).
    pub fn frame(&mut self, capture_us: u64, key: bool, payload: &[u8]) -> Vec<Out> {
        let mut out = Vec::new();
        if key {
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
        let mut data = Vec::with_capacity(FRAME_HEADER + payload.len());
        data.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        data.extend_from_slice(&capture_us.to_be_bytes());
        data.extend_from_slice(payload);
        out.push(Out::Write { group: g, data });
        out
    }

    /// A receiver report arrived.
    pub fn on_report(&mut self, now_us: u64, r: &RateReport) {
        let delivered = r.delivered_bps as u64;
        if r.queuing_us > self.cfg.overuse_us {
            // A queue is building: send no faster than what actually gets through.
            if now_us - self.last_decrease_us > 200_000 || self.last_decrease_us == 0 {
                let target = (delivered * 85 / 100) as u32;
                self.bitrate = self.bitrate.min(target).max(self.cfg.min_bps);
                self.last_decrease_us = now_us;
            }
        } else if r.queuing_us < self.cfg.underuse_us && now_us - self.last_decrease_us > 500_000 {
            // Path is empty: +8% per report, never far above what was delivered.
            let cap = (delivered * 3 / 2).max(self.cfg.min_bps as u64) as u32;
            let up = self.bitrate + self.bitrate * 8 / 100;
            self.bitrate = up.min(self.bitrate.max(cap)).min(self.cfg.max_bps);
        }
    }

    /// Transport feedback, called every few ms: bytes not yet sent, per group.
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
        if backlog_us > self.cfg.max_backlog_us {
            // Behind: abandon the current group, restart from a keyframe at a lower rate.
            if let Some(g) = self.current.take() {
                out.push(Out::Reset { group: g });
            }
            self.want_key = true;
            self.drops += 1;
            self.bitrate = (self.bitrate / 2).max(self.cfg.min_bps);
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

/// Receiver -> sender, every [`REPORT_US`].
#[derive(Clone, Debug, PartialEq)]
pub struct RateReport {
    /// Video bytes received over the window, as a rate.
    pub delivered_bps: u32,
    /// Latest frame's (arrival - capture) above the minimum of the last 10 s.
    pub queuing_us: u64,
}

pub const REPORT_US: u64 = 100_000;
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
}

impl Default for ReceiverConfig {
    fn default() -> Self {
        Self { stall_us: 600_000, max_frame: 1 << 20 }
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
    window_bytes: u64,
    window_start_us: u64,
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
            window_bytes: 0,
            window_start_us: 0,
            transit: Default::default(),
        }
    }

    pub fn on_data(&mut self, now_us: u64, group: u64, data: &[u8]) -> Result<(), RecvError> {
        self.window_bytes += data.len() as u64;
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
        let span = now_us - self.window_start_us;
        if span < REPORT_US {
            return None;
        }
        let delivered_bps = (self.window_bytes * 8 * 1_000_000 / span) as u32;
        self.window_bytes = 0;
        self.window_start_us = now_us;
        let min = self.transit.iter().map(|t| t.1).min()?;
        let last = self.transit.back()?.1;
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
        assert_eq!(tx.bitrate(), 255_000, "backs off to 85% of delivered");
        // No ramp right after a decrease, then +8% per empty-path report.
        tx.on_report(1_100_000, &RateReport { delivered_bps: 300_000, queuing_us: 0 });
        assert_eq!(tx.bitrate(), 255_000);
        tx.on_report(1_600_000, &RateReport { delivered_bps: 300_000, queuing_us: 0 });
        assert!(tx.bitrate() > 255_000 && tx.bitrate() < start);
    }
}
