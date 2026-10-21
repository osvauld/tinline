//! The call's adaptation runtime: the once-a-second policy task, the hint hand-off and the own-caps
//! push into the live sender. The policy itself (drawn size, budget, decode cost, labels) is
//! `video_call_core::adapt`; this declares its records to UniFFI and exports the picture labels.

use std::time::{Duration, Instant};

use video_call_core::adapt::{DECODE_MIN_SAMPLES, hw_behind, key_causes_text};
use video_call_core::hd;

use super::*;

/// What the platform tells us about the device (`Node::set_video_budget`).
#[uniffi::remote(Record)]
pub struct VideoBudget {
    /// Android `PowerManager` thermal status (0 none, 1 light, 2 moderate, 3 severe, ...);
    /// 0 where there is no such thing.
    pub thermal: u8,
    /// Battery saver is on.
    pub power_save: bool,
}

/// What caps our sending, as `VideoStats.send_cap` reports it.
#[uniffi::remote(Record)]
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
#[uniffi::export]
pub fn video_send_label(stats: &VideoStats) -> String {
    video_call_core::adapt::send_label(stats)
}

/// The picture label for what I receive ("Their video \u{b7} 540p \u{b7} 24 fps"); empty before
/// the first decoded frame. Both apps show it on the remote picture.
#[uniffi::export]
pub fn video_recv_label(stats: &VideoStats) -> String {
    video_call_core::adapt::recv_label(stats)
}

/// The codec of one direction for call details, from `VideoStats.{send,recv}_codec` and
/// `{send,recv}_hardware`: "VP8 \u{b7} hardware", "AV1 \u{b7} software". Empty before the codec is decided.
#[uniffi::export]
pub fn video_codec_label(codec: String, hardware: bool) -> String {
    video_call_core::adapt::codec_label(codec, hardware)
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
    let (mut last_payload, mut last_recv_payload) = (0u64, 0u64);
    // (received, decoded) frames per tick, last two ticks: the hardware decoder's throughput.
    let (mut last_rx, mut hist) = (0u64, std::collections::VecDeque::<(u64, u64)>::new());
    // The sender's (overuse, starved, backlog drops) at the last tick: a change is congestion.
    let mut last_trouble = (0u64, 0u64, 0u64);
    // (burst kbps, roomy reports, probes) for the per-second line (`SenderConfig::burst_room`).
    let mut rate_line = (0u32, 0u64, 0u64);
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
        let mut key_causes = video::media::KeyCauses::default();
        if let Some(s) = &send {
            let (trouble, draining, rate, rung, fps, enc_p75, causes, probe) = s.with_sender(|s| {
                let l = s.ladder();
                let probe = (s.burst_bps(), s.trace.roomy, s.probes);
                ((s.trace.overuse, s.trace.starved, s.drops), s.draining(), s.bitrate(), l.rung(), l.fps(), l.encode_p75(), s.key_causes, probe)
            });
            rate_line = probe;
            key_causes = causes;
            send_room.congested = draining || trouble != last_trouble;
            last_trouble = trouble;
            send_room.rate_bps = rate;
            send_room.rung_px = rung.width as u64 * rung.height as u64;
            send_room.fps = fps;
            send_room.encode_p75_us = enc_p75;
        }
        send_room.cpu_step = steps.iter().any(|s| s.reason == video::media::StepReason::Cpu);
        let mut st = pipe.stats.lock();
        st.send_payload_bps = video_call_core::traffic::payload_bps(st.admitted_payload_bytes.saturating_sub(last_payload), secs);
        st.recv_payload_bps = video_call_core::traffic::payload_bps(st.received_payload_bytes.saturating_sub(last_recv_payload), secs);
        (last_payload, last_recv_payload) = (st.admitted_payload_bytes, st.received_payload_bytes);
        st.decode_p75_ms = dec;
        // `send_rung_height` comes with each sent frame (`pipes::encode_loop`), not from here.
        let pinned = st.live.fixed_height != 0;
        for s in &steps {
            let why = match s.reason {
                video::media::StepReason::Rate => "rate",
                video::media::StepReason::Cpu => "cpu",
                video::media::StepReason::Cap if src == "hint" => "hint",
                // A pinned size (stats for nerds) moves the ladder as a cap step: say so.
                video::media::StepReason::Cap if pinned => "pin",
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
                "video send {}x{}@{} {}kbps cap={}px/{}fps({}) key={} ({}) overuse={} starved={} queue={}ms qoff={}ms drops={} enc={:.1}ms rc={} burst={}kbps roomy={} probes={} | recv {}x{}@{} dec={:.1}ms decode_err={}",
                st.send_width, st.send_height, st.send_fps, st.bitrate_bps / 1000,
                cap.pixels, cap.fps, cap.reason, st.keyframes_sent, key_causes_text(&key_causes), st.rate_overuse, st.rate_starved,
                st.last_queuing_us / 1000, st.queuing_offset_us / 1000, st.backlog_drops,
                send_room.encode_p75_us.unwrap_or(0) as f32 / 1000.0,
                if st.tuning.rate_control.is_empty() { "auto" } else { st.tuning.rate_control.as_str() },
                rate_line.0 / 1000, rate_line.1, rate_line.2,
                st.recv_width, st.recv_height, st.recv_fps, p75, st.decode_errors,
            );
        }
        drop(st);
        // Sharper video: the core's HD choice for the camera, and our own hint's room flag (sent
        // at once when it changes).
        let changed = {
            let mut pol = pipe.policy.lock();
            let before = (pol.auto.on, pol.room);
            pol.observe_room(send_room, recv_room);
            (before != (pol.auto.on, pol.room)).then_some((pol.auto.on, pol.room))
        };
        if let Some((auto, room)) = changed {
            tracing::info!(target: "p2pcore::video_call", "video hd auto={auto} room={room}");
        }
        push_hint(&pipe, now);
    }
}
