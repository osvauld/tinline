//! Receiver-side adaptation policy (docs/design/video-adaptation.md, section 4): the drawn
//! size, the platform's thermal / battery state and our own decode cost become the hint sent
//! to the peer's sender, and the same budget caps our own sending ladder.

use std::time::{Duration, Instant};

use video::media::{Hint, HintReason, LadderConfig};

use crate::hd;
use crate::stats::VideoStats;

/// What the platform tells us about the device (`Node::set_video_budget`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VideoBudget {
    /// Android `PowerManager` thermal status (0 none, 1 light, 2 moderate, 3 severe, ...);
    /// 0 where there is no such thing.
    pub thermal: u8,
    /// Battery saver is on.
    pub power_save: bool,
}

/// What caps our sending, as `VideoStats.send_cap` reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VideoSendCap {
    /// Largest picture (pixels) we may send now: the top rung when nothing caps it.
    pub pixels: u32,
    /// Highest frame rate we may send now.
    pub fps: u32,
    /// Why, for the chip: "none", "display", "decode", "thermal", "battery" or "user".
    pub reason: String,
    /// Whose limit it is: "peer" (their hint: their screen, decoder, heat, battery saver),
    /// "device" (my own budget, or their offer's largest size), "" when nothing caps it.
    pub source: String,
}

/// The picture label for what we send, from the stats (one text for every app): "HD · 24 fps"
/// while the top size is sent; below it, why ("540p · slow network", "540p · their screen"), else
/// the rate. Empty before a frame was sent.
pub fn send_label(stats: &VideoStats) -> String {
    let (w, h) = (stats.send_width, stats.send_height);
    if w == 0 || h == 0 {
        return String::new();
    }
    let hd = w.max(h) >= 1280 && w.min(h) >= 720;
    let name = if hd { "HD".to_string() } else { format!("{}p", w.min(h)) };
    let fps = || {
        if stats.send_fps > 0 {
            format!(" \u{b7} {} fps", stats.send_fps)
        } else {
            String::new()
        }
    };
    if hd {
        return format!("{name}{}", fps());
    }
    let why = match stats.send_cap.reason.as_str() {
        "display" => "their screen",
        "decode" => "their device",
        "thermal" => "warm device",
        "battery" => "battery saver",
        "user" => "their choice",
        _ => match stats.last_step_reason.as_str() {
            // Only while the rate is still short of the next size up: once it is back, the
            // ladder is just waiting out its holds (the reason used to stick until HD).
            "rate" if rate_short(stats) => "slow network",
            "rate" => "",
            "cpu" => "busy device",
            "hint" => "their screen",
            "pin" => "your pin",
            // Our own cap that is not a pin: name it only when the budget says why (the peer's
            // offer caps it too; that used to be called "battery saver" with saver off).
            "budget" if stats.budget.power_save => "battery saver",
            "budget" if stats.budget.thermal >= 2 => "warm device",
            _ => "",
        },
    };
    if why.is_empty() {
        format!("{name}{}", fps())
    } else {
        format!("{name} \u{b7} {why}")
    }
}

/// The send rate is below what the next size up needs (the ladder's step-up threshold at 30 fps).
fn rate_short(stats: &VideoStats) -> bool {
    let cfg = LadderConfig::default();
    let short = stats.send_width.min(stats.send_height);
    cfg.rungs
        .iter()
        .find(|r| r.height > short)
        .is_some_and(|r| (stats.bitrate_bps as u64) * 100 < r.min_bps as u64 * cfg.up_margin_pct as u64)
}

/// The picture label for what I receive ("Their video \u{b7} 540p \u{b7} 24 fps"); empty before
/// the first decoded frame. Both apps show it on the remote picture.
pub fn recv_label(stats: &VideoStats) -> String {
    let (w, h) = (stats.recv_width, stats.recv_height);
    if w == 0 || h == 0 {
        return String::new();
    }
    let name = if w.max(h) >= 1280 && w.min(h) >= 720 { "HD".to_string() } else { format!("{}p", w.min(h)) };
    if stats.recv_fps > 0 {
        format!("Their video \u{b7} {name} \u{b7} {} fps", stats.recv_fps)
    } else {
        format!("Their video \u{b7} {name}")
    }
}

/// The codec of one direction for call details, from `VideoStats.{send,recv}_codec` and
/// `{send,recv}_hardware`: "VP8 \u{b7} hardware", "AV1 \u{b7} software". Empty before the codec is decided.
pub fn codec_label(codec: String, hardware: bool) -> String {
    if codec.is_empty() {
        return String::new();
    }
    format!("{} \u{b7} {}", codec.to_uppercase(), if hardware { "hardware" } else { "software" })
}

/// A hint change that only shrinks the picture waits this long to be sure (window drags).
const SHRINK_HOLD: Duration = Duration::from_secs(1);
/// Decode cost above this % of the frame interval: one frame-rate tier down.
const DECODE_HIGH_PCT: f32 = 60.0;
/// ...below this % for `DECODE_UP_AFTER`: one tier up.
const DECODE_LOW_PCT: f32 = 30.0;
const DECODE_UP_AFTER: Duration = Duration::from_secs(10);
/// Decode times needed in the window before they steer anything.
pub const DECODE_MIN_SAMPLES: usize = 10;
/// Hardware decoder: frames received in the last 2 s needed before "behind" means anything.
const HW_MIN_RECEIVED: u64 = 20;
/// ...and it is behind when it decoded fewer than this % of them.
const HW_KEEP_UP_PCT: u64 = 80;

/// A hardware decoder is behind when, over the last 2 s, it delivered under 80% of the frames
/// it was given (the 2-slot channel then fills and the reader stops).
pub fn hw_behind(received: u64, decoded: u64) -> bool {
    received >= HW_MIN_RECEIVED && decoded * 100 < received * HW_KEEP_UP_PCT
}

/// Pixel cap of a budget: (pixels, fps), `u32::MAX` / `0` for no cap, and why.
pub fn budget_caps(b: VideoBudget, rungs: &[video::media::Rung]) -> (u32, u8, HintReason) {
    let px = |h: u32| {
        rungs
            .iter()
            .rev()
            .find(|r| r.height <= h)
            .map_or(0, |r| r.width * r.height)
    };
    let mut cap = (u32::MAX, 0u8, HintReason::None);
    let mut add = |p: u32, f: u8, why: HintReason| {
        cap.0 = cap.0.min(p);
        cap.1 = if cap.1 == 0 { f } else { cap.1.min(f) };
        if cap.2 == HintReason::None || why == HintReason::Thermal {
            cap.2 = why;
        }
    };
    if b.thermal >= 3 {
        add(px(360), 15, HintReason::Thermal);
    } else if b.thermal == 2 {
        add(px(540), 15, HintReason::Thermal);
    }
    if b.power_save {
        add(px(540), 24, HintReason::PowerSave);
    }
    cap
}

pub struct Policy {
    ladder: LadderConfig,
    /// The drawn size of the remote picture and whether it is on screen: the largest shown of
    /// `views`, else hidden (None before any).
    view: Option<(u32, u32, bool)>,
    /// Every place the app draws the remote picture, by the app's id for it: the call screen
    /// and the in-app strip's tile can both be composed, and the one reported last used to win
    /// (a stopped screen's tile said "hidden" and the peer sent 180p15 to a full-screen picture).
    views: std::collections::BTreeMap<u32, (u32, u32, bool)>,
    pub budget: VideoBudget,
    /// Frame rate tier our decoder keeps up with (30, 24, 15).
    pub decode_fps: u8,
    low_since: Option<Instant>,
    applied: Option<Hint>,
    shrink: Option<(Hint, Instant)>,
    /// The peer's offer: highest rung and frame rate.
    pub peer: (usize, u32),
    /// The peer's offer says `burst_probe`.
    pub peer_probes: bool,
    /// The camera captures HD now (`Node::set_video_hd`).
    pub hd_on: bool,
    /// Our decoder is asynchronous (hardware, or any MediaCodec one: `codecs::pipelined_decoder`):
    /// its cost is judged by throughput, not wall time.
    pub recv_pipelined: bool,
    /// The sharper-video rules (`hd`): the core's HD choice for the camera
    /// (`VideoStats::hd_auto`), and how long our receiver's `Hint::room` has held.
    pub auto: hd::Auto,
    recv_room: hd::Streak,
    pub room: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            ladder: LadderConfig::default(),
            view: None,
            views: Default::default(),
            budget: VideoBudget::default(),
            decode_fps: 30,
            low_since: None,
            applied: None,
            shrink: None,
            peer: (usize::MAX, u32::MAX),
            peer_probes: false,
            hd_on: false,
            recv_pipelined: false,
            auto: hd::Auto::default(),
            recv_room: hd::Streak::default(),
            room: false,
        }
    }
}

impl Policy {
    pub fn set_view(&mut self, id: u32, w: u32, h: u32, visible: bool) {
        self.views.insert(id, (w, h, visible));
        let shown = self.views.values().filter(|v| v.2).max_by_key(|v| v.0 as u64 * v.1 as u64);
        self.view = Some(shown.copied().unwrap_or((w, h, false)));
    }

    /// The hint for the state now (before hysteresis).
    pub fn wanted(&self) -> Hint {
        let disp = match self.view {
            None => u32::MAX,
            Some((_, _, false)) => 0,
            Some((w, h, true)) => {
                let need = w as u64 * h as u64;
                // One rung above the rung that covers the drawn size: a picture drawn at 540p
                // is sent at 720p (sharper when scaled; covers a portrait full-screen
                // picture that falls between two rungs). The top rung stays the top; a
                // drawn size beyond every rung is unlimited.
                let rungs = &self.ladder.rungs;
                rungs
                    .iter()
                    .position(|r| r.width as u64 * r.height as u64 >= need)
                    .map_or(u32::MAX, |i| {
                        let r = &rungs[(i + 1).min(rungs.len() - 1)];
                        r.width * r.height
                    })
            }
        };
        if disp == 0 {
            return Hint {
                max_pixels: 0,
                max_fps: 0,
                reason: HintReason::DisplaySize,
                room: false,
            };
        }
        let (bp, bf, br) = budget_caps(self.budget, &self.ladder.rungs);
        let df = if self.decode_fps >= 30 {
            0
        } else {
            self.decode_fps
        };
        let fps = match (bf, df) {
            (0, d) => d,
            (b, 0) => b,
            (b, d) => b.min(d),
        };
        let reason = if bp < disp || bf != 0 {
            br
        } else if df != 0 {
            HintReason::DecodeSlow
        } else {
            HintReason::DisplaySize
        };
        Hint {
            max_pixels: disp.min(bp),
            max_fps: fps,
            reason,
            room: self.room,
        }
    }

    /// One second of the sharper-video rules (`hd`): `send` for what I send, `recv` with what I
    /// receive filled in except the view, which comes from here. Sets the core's HD choice for
    /// the camera and our own hint's room flag.
    pub fn observe_room(&mut self, mut send: hd::SendSample, mut recv: hd::RecvSample) {
        send.hd_on = self.hd_on;
        self.auto.observe(&send);
        let w = Hint { room: false, ..self.wanted() };
        let shown = matches!(self.view, Some((_, _, true)));
        recv.wanted_px = if shown { w.max_pixels as u64 } else { 0 };
        recv.limited = w.reason != HintReason::DisplaySize || w.max_fps != 0;
        self.room = self.recv_room.observe(hd::recv_ok(&recv), hd::ROOM_SECS);
    }

    /// A hint to send now, if there is a change worth sending. A change that only makes the
    /// picture smaller has to hold for a second (window resizes); everything else goes at once.
    pub fn update(&mut self, now: Instant) -> Option<Hint> {
        let want = self.wanted();
        // No cap and no flag: nothing to say (a full-screen phone picture is beyond every rung).
        let nothing = want.max_pixels == u32::MAX && want.max_fps == 0 && !want.room;
        if self.applied == Some(want) || (self.applied.is_none() && nothing) {
            self.shrink = None;
            return None;
        }
        if let Some(cur) = self.applied {
            let shrinks = want.reason == HintReason::DisplaySize
                && want.max_fps == cur.max_fps
                && want.max_pixels < cur.max_pixels
                && want.max_pixels != 0;
            if shrinks {
                let since = match self.shrink {
                    Some((h, t)) if h.max_pixels == want.max_pixels => t,
                    _ => now,
                };
                self.shrink = Some((want, since));
                if now.duration_since(since) < SHRINK_HOLD {
                    return None;
                }
            }
        }
        self.shrink = None;
        self.applied = Some(want);
        Some(want)
    }

    /// Decode cost (p75 over the last 2 s, `n` samples): one tier down above 60% of the frame
    /// interval, one up after 10 s below 30%. Returns whether the tier changed.
    pub fn observe_decode(&mut self, now: Instant, p75_ms: f32, n: usize) -> bool {
        if n < DECODE_MIN_SAMPLES {
            return false;
        }
        let interval_ms = 1000.0 / self.decode_fps as f32;
        let pct = p75_ms * 100.0 / interval_ms;
        if pct > DECODE_HIGH_PCT && self.decode_fps > 15 {
            self.decode_fps = if self.decode_fps > 24 { 24 } else { 15 };
            self.low_since = None;
            return true;
        }
        if pct >= DECODE_LOW_PCT || self.decode_fps >= 30 {
            self.low_since = None;
            return false;
        }
        let since = *self.low_since.get_or_insert(now);
        if now.duration_since(since) >= DECODE_UP_AFTER {
            self.decode_fps = if self.decode_fps < 24 { 24 } else { 30 };
            self.low_since = Some(now);
            return true;
        }
        false
    }

    /// Hardware (asynchronous) decoders: the wall time of `decode` is pipeline latency, not CPU
    /// work, so it says nothing about cost. Judge by throughput (`behind`, see `hw_behind`: counts
    /// as 100% of the interval) and by the render cost, which is real CPU in both cases.
    pub fn observe_decode_hw(&mut self, now: Instant, behind: bool, render_ms: f32, n: usize) -> bool {
        let interval_ms = 1000.0 / self.decode_fps as f32;
        self.observe_decode(now, if behind { interval_ms } else { render_ms }, n)
    }

    /// Our own ladder caps: (highest rung, highest fps) from the peer's offer and the budget.
    pub fn own_caps(&self) -> (usize, u32) {
        let (bp, bf, _) = budget_caps(self.budget, &self.ladder.rungs);
        let rung = self
            .ladder
            .rungs
            .iter()
            .rposition(|r| r.width * r.height <= bp)
            .unwrap_or(0);
        let fps = if bf == 0 { u32::MAX } else { bf as u32 };
        (rung.min(self.peer.0), fps.min(self.peer.1))
    }

    /// What caps our sending now: the peer's offer, our budget and the peer's hint. The second
    /// value says which one binds ("hint", "budget" or "").
    pub fn send_cap(&self, peer_hint: Option<Hint>) -> (VideoSendCap, &'static str) {
        let rungs = &self.ladder.rungs;
        let top = rungs.last().map_or(0, |r| r.width * r.height);
        let top_fps = *self.ladder.tiers.last().unwrap_or(&30);
        let (own_rung, own_fps) = self.own_caps();
        let own_px = rungs.get(own_rung).map_or(top, |r| r.width * r.height);
        let (mut px, mut fps) = (own_px, own_fps.min(top_fps));
        let (mut hint_px, mut hint_fps, mut hint_why) = (top, top_fps, HintReason::None);
        if let Some(h) = peer_hint {
            hint_why = h.reason;
            if h.max_pixels == 0 {
                hint_px = rungs[0].width * rungs[0].height;
                // The rung's lowest tier (see `Rung::min_fps`).
                hint_fps = self.ladder.tiers.iter().copied().find(|&t| t >= rungs[0].min_fps).unwrap_or(top_fps);
            } else {
                hint_px = rungs
                    .iter()
                    .rev()
                    .find(|r| r.width as u64 * r.height as u64 <= h.max_pixels as u64)
                    .map_or(rungs[0].width * rungs[0].height, |r| r.width * r.height);
                if h.max_fps != 0 {
                    hint_fps = h.max_fps as u32;
                }
            }
        }
        let hint_binds = hint_px < px || hint_fps < fps;
        let own_binds = own_px < top || own_fps < top_fps;
        px = px.min(hint_px);
        fps = fps.min(hint_fps);
        let (why, src) = if hint_binds {
            (hint_why, "hint")
        } else if own_binds {
            (budget_caps(self.budget, rungs).2, "budget")
        } else {
            (HintReason::None, "")
        };
        (
            VideoSendCap {
                pixels: px,
                fps,
                reason: why.as_str().into(),
                source: match src {
                    "hint" => "peer",
                    "budget" => "device",
                    _ => "",
                }
                .into(),
            },
            src,
        )
    }
}

/// Why each keyframe was sent, for the per-second log: "start 1 rate 2 req 5 backlog 6" (zeros left out).
pub fn key_causes_text(c: &video::media::KeyCauses) -> String {
    let all = [
        ("start", c.start),
        ("rate", c.rate),
        ("cpu", c.cpu),
        ("hint", c.hint),
        ("req", c.request),
        ("backlog", c.backlog),
        ("other", c.other),
    ];
    all.iter().filter(|e| e.1 > 0).map(|(n, v)| format!("{n} {v}")).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests;
