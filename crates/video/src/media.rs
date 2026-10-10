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
}

impl Default for SenderConfig {
    fn default() -> Self {
        Self {
            fps: 30,
            min_bps: 100_000,
            max_bps: 2_000_000,
            start_bps: 500_000,
            max_backlog_us: 300_000,
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

    /// Transport feedback, called every few ms: bytes not yet sent once per group, and the
    /// rate the congestion controller currently allows.
    pub fn feedback(&mut self, unsent: &BTreeMap<u64, usize>, send_rate_bps: u64) -> Vec<Out> {
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
        let target = (send_rate_bps * 85 / 100).clamp(self.cfg.min_bps as u64, self.cfg.max_bps as u64) as u32;
        if backlog_us > self.cfg.max_backlog_us {
            // Behind: abandon the current group, restart from a keyframe at a lower rate.
            if let Some(g) = self.current.take() {
                out.push(Out::Reset { group: g });
            }
            self.want_key = true;
            self.drops += 1;
            self.bitrate = (self.bitrate / 2).max(self.cfg.min_bps).min(target);
        } else if target < self.bitrate {
            self.bitrate = target;
        } else {
            // Ramp up gently: +5% per feedback at most.
            self.bitrate = (self.bitrate + self.bitrate / 20).min(target);
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
        }
    }

    pub fn on_data(&mut self, group: u64, data: &[u8]) -> Result<(), RecvError> {
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
            let ready = self.groups.get(&cur).is_some_and(|g| g.next < g.frames.len());
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
                let _ = ready;
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
