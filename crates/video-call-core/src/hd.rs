//! Sharper video by itself: when to open the camera at 1280x720 instead of 640x480 (docs/video.md,
//! "Sharper video"). The ladder decides the picture sent; the camera only bounds it. With a
//! 640x480 capture the ladder can sit at its 720p rung and still send 480p, so when the line, the
//! encoder and the peer's screen could all carry more, the core says "HD" ([`Auto`],
//! `VideoStats::hd_auto`) and the app reopens the camera. The HD chip still overrides it for the
//! call, and Settings > Video "Prefer HD" keeps the camera at HD. Mobile data is no exception;
//! Android says once when HD runs on a metered network.
//!
//! Two directions, one rule each, evaluated once a second by `adapt::run_policy`:
//! - **Sending** ([`send_ok`]): my capture is not HD and my picture is smaller than my ladder
//!   already allows, because of the capture, with the rate, the encoder and the peer's caps to
//!   spare. Held [`ON_SECS`] in a row, the camera goes to HD; below 540p for [`OFF_SECS`] in a
//!   row ([`keep_ok`]), back to 640x480.
//! - **Receiving** ([`recv_ok`]): I draw the peer's picture big, I get less than 540p, and my
//!   decoder would keep up at 540p. That sets [`video::media::Hint::room`] for the peer's logs.
//!
//! A rule has to hold for its seconds in a row ([`Streak`]): a borderline link that dips now and
//! then never switches the camera.

/// The smallest picture sharper video brings on every device: 960x540 (phones aim for 540p,
/// computers for 720p). HD off sends at most 640x480.
pub const SHARPER_PIXELS: u64 = 960 * 540;
/// The rate my line has to carry: the ladder's step-up threshold for 720p30 (floor 1 Mbps x
/// 130%), the point at which an HD capture would climb to the top rung. Above the 540p rung's
/// threshold (780 kbps) on purpose: HD should lead to a clearly sharper picture, not to
/// one rung that a dip takes back.
pub const ROOM_BPS: u32 = 1_300_000;
/// Seconds in a row the sending rule must hold before the camera goes to HD: the ladder's own
/// 5 s hold for a step up, on top of the rate having reached the 720p threshold.
pub const ON_SECS: u32 = 5;
/// Seconds in a row below 540p (the ladder's rung or the peer's cap) before the camera goes back
/// to 640x480. Reopening the camera costs a keyframe and a short freeze, so a dip doesn't; an
/// HD capture under a 360p rung is only scaled down, it never sends more.
pub const OFF_SECS: u32 = 15;
/// Seconds in a row the receiving rule must hold for the room flag.
pub const ROOM_SECS: u32 = 10;
/// Encode / decode + render time budget, % of the frame interval: the ladder's and the decode
/// policy's step-down threshold (60%). A size that would be predicted above it is not "room".
const BUDGET_PCT: u64 = 60;

/// One second of my sending, as `run_policy` sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SendSample {
    /// The camera captures HD now (`Node::set_video_hd`).
    pub hd_on: bool,
    /// Frames sent in the last second.
    pub frames: u64,
    /// Size of the picture sent (pixels), and of the ladder's rung (what the rate allows).
    pub send_px: u64,
    pub rung_px: u64,
    /// The cap from the peer's hint, its offer and my budget (`VideoSendCap::pixels`).
    pub cap_px: u64,
    /// The sender's rate estimate.
    pub rate_bps: u32,
    /// Overuse, starvation or a backlog drop since the last sample, or a queue being drained.
    pub congested: bool,
    /// The ladder stepped down for encode time since the last sample.
    pub cpu_step: bool,
    /// p75 encode time at the current size (us) and the current frame rate.
    pub encode_p75_us: Option<u64>,
    pub fps: u32,
    /// A hardware encoder: its wall time is pipeline latency, not CPU work, and it handles 720p.
    pub hw: bool,
}

/// My line, encoder and the peer's caps could carry a sharper picture than my 640x480 capture
/// gives, this second.
pub fn send_ok(s: &SendSample) -> bool {
    if s.hd_on || s.frames == 0 || s.send_px == 0 {
        return false;
    }
    // The capture is what holds the picture back: the ladder already allows a bigger one.
    let capture_bound = s.rung_px >= SHARPER_PIXELS && s.send_px < s.rung_px;
    let peer_takes_it = s.cap_px >= SHARPER_PIXELS;
    let line = s.rate_bps >= ROOM_BPS && !s.congested;
    let cpu = !s.cpu_step && (s.hw || fits(s.encode_p75_us, s.send_px, s.fps));
    capture_bound && peer_takes_it && line && cpu
}

/// An HD capture still pays this second: the ladder's rung and the peer's cap are 540p or more.
/// No frames (the camera reopening, or off) counts as keeping it.
pub fn keep_ok(s: &SendSample) -> bool {
    s.frames == 0 || (s.rung_px >= SHARPER_PIXELS && s.cap_px >= SHARPER_PIXELS)
}

/// One second of what I receive and draw.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RecvSample {
    /// The hint I would send without the flag: shown (`max_pixels` > 0), not capped by my
    /// battery, heat or decoder (reason `DisplaySize`), and how big it asks for.
    pub wanted_px: u64,
    pub limited: bool,
    /// Frames decoded in the last second, and their size (pixels).
    pub frames: u64,
    pub recv_px: u64,
    /// p75 decode + render time (ms) over 2 s and how many samples; for a hardware decoder,
    /// whether it fell behind (it then has no meaningful time, see `adapt::hw_behind`).
    pub cost_ms: f32,
    pub cost_n: usize,
    pub hw: bool,
    pub behind: bool,
}

/// I draw the picture big enough for 540p, get less, and would decode and draw 540p in time.
pub fn recv_ok(s: &RecvSample) -> bool {
    if s.limited || s.frames == 0 || s.recv_px == 0 {
        return false;
    }
    let big = s.wanted_px >= SHARPER_PIXELS && s.recv_px < SHARPER_PIXELS;
    let keeps_up = if s.hw {
        !s.behind
    } else {
        s.cost_n >= 10
            && (s.cost_ms as f64) * 1000.0 * (SHARPER_PIXELS as f64) / (s.recv_px as f64)
                <= budget_us(30) as f64
    };
    big && keeps_up
}

fn budget_us(fps: u32) -> u64 {
    1_000_000 * BUDGET_PCT / (100 * fps.max(1) as u64)
}

/// The p75 encode time scaled to a 540p picture fits the frame budget (time grows with pixels).
fn fits(p75_us: Option<u64>, px: u64, fps: u32) -> bool {
    p75_us.is_some_and(|p| px > 0 && p * SHARPER_PIXELS / px <= budget_us(fps))
}

/// Seconds in a row a rule has held; true from the `secs`th on, false at once when it breaks.
#[derive(Debug, Default)]
pub struct Streak(u32);

impl Streak {
    pub fn observe(&mut self, ok: bool, secs: u32) -> bool {
        self.0 = if ok { self.0.saturating_add(1) } else { 0 };
        self.0 >= secs
    }
}

/// The core's HD choice for the camera (`VideoStats::hd_auto`): on after [`send_ok`] held
/// [`ON_SECS`], off again after [`keep_ok`] failed for [`OFF_SECS`] in a row.
#[derive(Debug, Default)]
pub struct Auto {
    pub on: bool,
    room: Streak,
    below: Streak,
}

impl Auto {
    pub fn observe(&mut self, s: &SendSample) -> bool {
        if self.on {
            if self.below.observe(!keep_ok(s), OFF_SECS) {
                *self = Self::default();
            }
        } else if self.room.observe(send_ok(s), ON_SECS) {
            self.on = true;
            self.room = Streak::default();
        }
        self.on
    }
}

#[cfg(test)]
mod tests;
