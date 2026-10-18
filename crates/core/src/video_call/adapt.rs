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
/// Hardware decoder: frames received in the last 2 s needed before "behind" means anything.
const HW_MIN_RECEIVED: u64 = 20;
/// ...and it is behind when it decoded fewer than this % of them.
const HW_KEEP_UP_PCT: u64 = 80;

/// A hardware decoder is behind when, over the last 2 s, it delivered under 80% of the frames
/// it was given (the 2-slot channel then fills and the reader stops).
pub(super) fn hw_behind(received: u64, decoded: u64) -> bool {
    received >= HW_MIN_RECEIVED && decoded * 100 < received * HW_KEEP_UP_PCT
}

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
    /// The app's HD choice for this call (`Node::set_video_hd`).
    pub(super) hd_on: bool,
    /// Our decoder is asynchronous (hardware, or any MediaCodec one: `codecs::pipelined_decoder`):
    /// its cost is judged by throughput, not wall time.
    pub(super) recv_pipelined: bool,
    /// The sharper-video rules (`hd`): how long each has held, our receiver's `Hint::room`, and
    /// what the app may suggest (`VideoStats::hd_suggest`).
    send_room: hd::Streak,
    recv_room: hd::Streak,
    room: bool,
    pub(super) suggest: &'static str,
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
            hd_on: false,
            recv_pipelined: false,
            send_room: hd::Streak::default(),
            recv_room: hd::Streak::default(),
            room: false,
            suggest: "",
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

    /// One second of the sharper-video rules (`hd`): `send` for what I send (`peer_room`: the
    /// peer's hint says it has room), `recv` with what I receive filled in except the view, which
    /// comes from here. Sets our own hint's room flag and what the app may suggest.
    pub(super) fn observe_room(&mut self, mut send: hd::SendSample, mut recv: hd::RecvSample, peer_room: bool) {
        send.hd_on = self.hd_on;
        let send_room = self.send_room.observe(hd::send_ok(&send));
        self.suggest = hd::suggestion(send_room, peer_room);
        let w = Hint { room: false, ..self.wanted() };
        let shown = matches!(self.view, Some((_, _, true)));
        recv.wanted_px = if shown { w.max_pixels as u64 } else { 0 };
        recv.limited = w.reason != HintReason::DisplaySize || w.max_fps != 0;
        self.room = self.recv_room.observe(hd::recv_ok(&recv));
    }

    /// A hint to send now, if there is a change worth sending. A change that only makes the
    /// picture smaller has to hold for a second (window resizes); everything else goes at once.
    pub(super) fn update(&mut self, now: Instant) -> Option<Hint> {
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

    /// Hardware (asynchronous) decoders: the wall time of `decode` is pipeline latency, not CPU
    /// work, so it says nothing about cost. Judge by throughput (`behind`, see `hw_behind`: counts
    /// as 100% of the interval) and by the render cost, which is real CPU in both cases.
    pub(super) fn observe_decode_hw(&mut self, now: Instant, behind: bool, render_ms: f32, n: usize) -> bool {
        let interval_ms = 1000.0 / self.decode_fps as f32;
        self.observe_decode(now, if behind { interval_ms } else { render_ms }, n)
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

/// Why each keyframe was sent, for the per-second log: "start 1 rate 2 req 5 backlog 6" (zeros left out).
pub(super) fn key_causes_text(c: &video::media::KeyCauses) -> String {
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

fn step_name(s: (u32, u32, u32)) -> String {
    format!("{}p{}", s.1, s.2)
}

/// Once a second for the life of the call: measured rates, the log lines, decode-cost steering
/// and the hint's hold-down timer.
pub(super) async fn run_policy(pipe: Arc<Pipe>) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let (mut last, mut last_sent, mut last_dec) = (Instant::now(), 0u64, 0u64);
    let (mut last_payload, mut last_recv_payload) = (0u64, 0u64);
    // (received, decoded) frames per tick, last two ticks: the hardware decoder's throughput.
    let (mut last_rx, mut hist) = (0u64, std::collections::VecDeque::<(u64, u64)>::new());
    // The sender's (overuse, starved, backlog drops) at the last tick: a change is congestion.
    let mut last_trouble = (0u64, 0u64, 0u64);
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
        let mut recv_room = hd::RecvSample { cost_ms: p75, cost_n: n, ..Default::default() };
        {
            let mut pol = pipe.policy.lock();
            let (rx_total, dec_total, hw) = {
                let st = pipe.stats.lock();
                (st.frames_received, st.frames_decoded, st.recv_hardware || pol.recv_pipelined)
            };
            hist.push_back((rx_total - last_rx, dec_total - last_dec));
            last_rx = rx_total;
            if hist.len() > 2 {
                hist.pop_front();
            }
            let (rx2, dec2) = hist.iter().fold((0, 0), |a, e| (a.0 + e.0, a.1 + e.1));
            recv_room.hw = hw;
            recv_room.behind = hw_behind(rx2, dec2);
            recv_room.frames = hist.back().map_or(0, |e| e.1);
            let changed = if hw {
                // Hardware or MediaCodec: latency is not cost; judge by throughput (+ render).
                let behind = hw_behind(rx2, dec2);
                let r = if rn >= DECODE_MIN_SAMPLES { render } else { 0.0 };
                pol.observe_decode_hw(now, behind, r, DECODE_MIN_SAMPLES.max(n))
            } else {
                pol.observe_decode(now, p75, n)
            };
            if changed {
                tracing::info!(target: "p2pcore::video_call", "video decode+render cost p75={p75:.1}ms (decode {dec:.1}, render {render:.1}, hw={hw}, recv/dec {rx2}/{dec2}): frame rate tier {}", pol.decode_fps);
            }
        }
        push_hint(&pipe, now);
        let send = pipe.send.lock().clone();
        let steps = send
            .as_ref()
            .map(|s| s.with_sender(|s| s.ladder_mut().take_steps()))
            .unwrap_or_default();
        let hint = send.as_ref().and_then(|s| s.with_sender(|s| s.hint()));
        let (cap, src) = pipe.policy.lock().send_cap(hint);
        // What the sharper-video rule needs from the sender (`hd::SendSample`).
        let mut send_room = hd::SendSample { cap_px: cap.pixels as u64, ..Default::default() };
        let mut rung_height = 0;
        let mut key_causes = video::media::KeyCauses::default();
        if let Some(s) = &send {
            let (trouble, draining, rate, rung, fps, enc_p75, causes) = s.with_sender(|s| {
                let l = s.ladder();
                ((s.trace.overuse, s.trace.starved, s.drops), s.draining(), s.bitrate(), l.rung(), l.fps(), l.encode_p75(), s.key_causes)
            });
            key_causes = causes;
            send_room.congested = draining || trouble != last_trouble;
            last_trouble = trouble;
            send_room.rate_bps = rate;
            send_room.rung_px = rung.width as u64 * rung.height as u64;
            rung_height = rung.height;
            send_room.fps = fps;
            send_room.encode_p75_us = enc_p75;
        }
        send_room.cpu_step = steps.iter().any(|s| s.reason == video::media::StepReason::Cpu);
        let mut st = pipe.stats.lock();
        st.send_payload_bps = traffic::payload_bps(st.admitted_payload_bytes.saturating_sub(last_payload), secs);
        st.recv_payload_bps = traffic::payload_bps(st.received_payload_bytes.saturating_sub(last_recv_payload), secs);
        (last_payload, last_recv_payload) = (st.admitted_payload_bytes, st.received_payload_bytes);
        st.decode_p75_ms = dec;
        st.send_rung_height = rung_height;
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
        let mut tm = std::mem::take(&mut *pipe.timing.lock());
        if tm.encode_us.len() + tm.recv_waits as usize > 0 {
            let ms = |v: &mut Vec<u32>, p: usize| {
                v.sort_unstable();
                v.get(v.len().saturating_sub(1) * p / 100).map_or(0.0, |&x| x as f32 / 1000.0)
            };
            tracing::debug!(
                target: "p2pcore::video_call",
                "video timing enc p50/p90/max={:.1}/{:.1}/{:.1}ms capture->send p50/p90/max={:.1}/{:.1}/{:.1}ms frames={} replaced={} recv_wait>5ms={} max={:.1}ms",
                ms(&mut tm.encode_us, 50), ms(&mut tm.encode_us, 90), ms(&mut tm.encode_us, 100),
                ms(&mut tm.lag_us, 50), ms(&mut tm.lag_us, 90), ms(&mut tm.lag_us, 100),
                tm.encode_us.len(), tm.replaced, tm.recv_waits, tm.recv_wait_max_us as f32 / 1000.0,
            );
        }
        st.send_fps = ((st.frames_sent - last_sent) as f32 / secs).round() as u32;
        st.recv_fps = ((st.frames_decoded - last_dec) as f32 / secs).round() as u32;
        send_room.frames = st.frames_sent - last_sent;
        send_room.send_px = st.send_width as u64 * st.send_height as u64;
        send_room.hw = st.send_hardware;
        recv_room.recv_px = st.recv_width as u64 * st.recv_height as u64;
        (last_sent, last_dec) = (st.frames_sent, st.frames_decoded);
        if st.frames_sent > 0 || st.frames_decoded > 0 {
            tracing::info!(
                target: "p2pcore::video_call",
                "video send {}x{}@{} {}kbps cap={}px/{}fps({}) key={} ({}) overuse={} starved={} queue={}ms qoff={}ms drops={} | recv {}x{}@{} dec={:.1}ms decode_err={}",
                st.send_width, st.send_height, st.send_fps, st.bitrate_bps / 1000,
                cap.pixels, cap.fps, cap.reason, st.keyframes_sent, key_causes_text(&key_causes), st.rate_overuse, st.rate_starved,
                st.last_queuing_us / 1000, st.queuing_offset_us / 1000, st.backlog_drops,
                st.recv_width, st.recv_height, st.recv_fps, p75, st.decode_errors,
            );
        }
        drop(st);
        // Sharper video: what the app may suggest, and our own hint's room flag (sent at once
        // when it changes).
        let suggest = {
            let mut pol = pipe.policy.lock();
            let before = (pol.suggest, pol.room);
            pol.observe_room(send_room, recv_room, hint.is_some_and(|h| h.room));
            (before != (pol.suggest, pol.room)).then_some((pol.suggest, pol.room))
        };
        if let Some((s, room)) = suggest {
            tracing::info!(target: "p2pcore::video_call", "video hd suggest={s:?} room={room}");
        }
        push_hint(&pipe, now);
    }
}
