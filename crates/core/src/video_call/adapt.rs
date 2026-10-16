//! Receiver-side adaptation policy (docs/design/video-adaptation.md, section 4): the drawn
//! size, the platform's thermal / battery state and our own decode cost become the hint sent
//! to the peer's sender, and the same budget caps our own sending ladder.

use std::time::{Duration, Instant};

use video::media::{Hint, HintReason, LadderConfig};

use super::*;

/// What the platform tells us about the device (`Node::set_video_budget`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, uniffi::Record)]
pub struct VideoBudget {
    /// Android `PowerManager` thermal status (0 none, 1 light, 2 moderate, 3 severe, ...);
    /// 0 where there is no such thing.
    pub thermal: u8,
    /// Battery saver is on.
    pub power_save: bool,
}

/// What caps our sending, as `VideoStats.send_cap` reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct VideoSendCap {
    /// Largest picture (pixels) we may send now: the top rung when nothing caps it.
    pub pixels: u32,
    /// Highest frame rate we may send now.
    pub fps: u32,
    /// Why, for the chip: "none", "display", "decode", "thermal", "battery" or "user".
    pub reason: String,
}

/// The picture label for what we send, from the stats (one text for every app): "HD · 24 fps"
/// while the top size is sent; below it, why ("540p · slow network", "540p · their screen"), else
/// the rate. Empty before a frame was sent.
#[uniffi::export]
pub fn video_send_label(stats: &VideoStats) -> String {
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
            "budget" => "battery saver",
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
#[uniffi::export]
pub fn video_recv_label(stats: &VideoStats) -> String {
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
#[uniffi::export]
pub fn video_codec_label(codec: String, hardware: bool) -> String {
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
const DECODE_MIN_SAMPLES: usize = 10;

/// Pixel cap of a budget: (pixels, fps), `u32::MAX` / `0` for no cap, and why.
pub(super) fn budget_caps(b: VideoBudget, rungs: &[video::media::Rung]) -> (u32, u8, HintReason) {
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

pub(super) struct Policy {
    ladder: LadderConfig,
    /// The drawn size of the remote picture and whether it is on screen.
    view: Option<(u32, u32, bool)>,
    pub(super) budget: VideoBudget,
    /// Frame rate tier our decoder keeps up with (30, 24, 15).
    decode_fps: u8,
    low_since: Option<Instant>,
    applied: Option<Hint>,
    shrink: Option<(Hint, Instant)>,
    /// The peer's offer: highest rung and frame rate.
    pub(super) peer: (usize, u32),
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            ladder: LadderConfig::default(),
            view: None,
            budget: VideoBudget::default(),
            decode_fps: 30,
            low_since: None,
            applied: None,
            shrink: None,
            peer: (usize::MAX, u32::MAX),
        }
    }
}

impl Policy {
    pub(super) fn set_view(&mut self, w: u32, h: u32, visible: bool) {
        self.view = Some((w, h, visible));
    }

    /// The hint for the state now (before hysteresis).
    pub(super) fn wanted(&self) -> Hint {
        let disp = match self.view {
            None => u32::MAX,
            Some((_, _, false)) => 0,
            Some((w, h, true)) => {
                let need = w as u64 * h as u64;
                // The next rung at or above the drawn size, never below the lowest.
                self.ladder
                    .rungs
                    .iter()
                    .find(|r| r.width as u64 * r.height as u64 >= need)
                    .map_or(u32::MAX, |r| r.width * r.height)
            }
        };
        if disp == 0 {
            return Hint {
                max_pixels: 0,
                max_fps: 0,
                reason: HintReason::DisplaySize,
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
        }
    }

    /// A hint to send now, if there is a change worth sending. A change that only makes the
    /// picture smaller has to hold for a second (window resizes); everything else goes at once.
    pub(super) fn update(&mut self, now: Instant) -> Option<Hint> {
        let want = self.wanted();
        let nothing = want.max_pixels == u32::MAX && want.max_fps == 0;
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
    pub(super) fn observe_decode(&mut self, now: Instant, p75_ms: f32, n: usize) -> bool {
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

    /// Our own ladder caps: (highest rung, highest fps) from the peer's offer and the budget.
    pub(super) fn own_caps(&self) -> (usize, u32) {
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
    pub(super) fn send_cap(&self, peer_hint: Option<Hint>) -> (VideoSendCap, &'static str) {
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
                hint_fps = self.ladder.tiers[0];
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
            },
            src,
        )
    }
}

/// Pushes our own caps (offer + budget) into the live sender.
pub(super) fn apply_own_caps(pipe: &Pipe) {
    let Some(send) = pipe.send.lock().clone() else {
        return;
    };
    let (rung, fps) = pipe.policy.lock().own_caps();
    send.with_sender(|s| {
        let l = s.ladder_mut();
        l.set_max(rung);
        l.set_max_fps(fps);
    });
}

/// Computes the hint and hands it to the receive task.
pub(super) fn push_hint(pipe: &Pipe, now: Instant) {
    if let Some(h) = pipe.policy.lock().update(now) {
        tracing::info!(target: "p2pcore::video_call", "video hint max={}px fps={} reason={}", h.max_pixels, h.max_fps, h.reason.as_str());
        *pipe.hint_slot.lock() = Some(h);
        pipe.hint_changed.notify_one();
    }
}

fn step_name(s: (u32, u32, u32)) -> String {
    format!("{}p{}", s.1, s.2)
}

/// Once a second for the life of the call: measured rates, the log lines, decode-cost steering
/// and the hint's hold-down timer.
pub(super) async fn run_policy(pipe: Arc<Pipe>) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let (mut last, mut last_sent, mut last_dec) = (Instant::now(), 0u64, 0u64);
    loop {
        tick.tick().await;
        if pipe.stop.load(Ordering::SeqCst) {
            return;
        }
        let now = Instant::now();
        let secs = now.duration_since(last).as_secs_f32().max(0.001);
        last = now;
        // Decode + render cost, p75 of each over the last 2 s (render is reported by the app).
        let p75_of = |q: &Mutex<VecDeque<(Instant, f32)>>| {
            let mut d = q.lock();
            while d
                .front()
                .is_some_and(|e| now.duration_since(e.0) > Duration::from_secs(2))
            {
                d.pop_front();
            }
            let mut v: Vec<f32> = d.iter().map(|e| e.1).collect();
            v.sort_by(|a, b| a.total_cmp(b));
            (
                v.get(v.len().saturating_sub(1) * 75 / 100)
                    .copied()
                    .unwrap_or(0.0),
                v.len(),
            )
        };
        let (dec, n) = p75_of(&pipe.decode_ms);
        let (render, rn) = p75_of(&pipe.render_ms);
        let p75 = dec + if rn >= DECODE_MIN_SAMPLES { render } else { 0.0 };
        {
            let mut pol = pipe.policy.lock();
            if pol.observe_decode(now, p75, n) {
                tracing::info!(target: "p2pcore::video_call", "video decode+render cost p75={p75:.1}ms (decode {dec:.1}, render {render:.1}): frame rate tier {}", pol.decode_fps);
            }
        }
        push_hint(&pipe, now);
        let send = pipe.send.lock().clone();
        let steps = send
            .as_ref()
            .map(|s| s.with_sender(|s| s.ladder_mut().take_steps()))
            .unwrap_or_default();
        let (cap, src) = {
            let hint = send.as_ref().and_then(|s| s.with_sender(|s| s.hint()));
            pipe.policy.lock().send_cap(hint)
        };
        let mut st = pipe.stats.lock();
        for s in &steps {
            let why = match s.reason {
                video::media::StepReason::Rate => "rate",
                video::media::StepReason::Cpu => "cpu",
                video::media::StepReason::Cap if src == "hint" => "hint",
                video::media::StepReason::Cap => "budget",
            };
            tracing::info!(target: "p2pcore::video_call", "video step {} -> {} reason={why} est={}kbps", step_name(s.from), step_name(s.to), s.bps / 1000);
            st.ladder_steps += 1;
            st.last_step_reason = why.into();
        }
        st.send_fps = ((st.frames_sent - last_sent) as f32 / secs).round() as u32;
        st.recv_fps = ((st.frames_decoded - last_dec) as f32 / secs).round() as u32;
        (last_sent, last_dec) = (st.frames_sent, st.frames_decoded);
        if st.frames_sent > 0 || st.frames_decoded > 0 {
            tracing::info!(
                target: "p2pcore::video_call",
                "video send {}x{}@{} {}kbps cap={}px/{}fps({}) key={} overuse={} starved={} queue={}ms drops={} | recv {}x{}@{} dec={:.1}ms decode_err={}",
                st.send_width, st.send_height, st.send_fps, st.bitrate_bps / 1000,
                cap.pixels, cap.fps, cap.reason, st.keyframes_sent, st.rate_overuse, st.rate_starved,
                st.last_queuing_us / 1000, st.backlog_drops,
                st.recv_width, st.recv_height, st.recv_fps, p75, st.decode_errors,
            );
        }
    }
}
