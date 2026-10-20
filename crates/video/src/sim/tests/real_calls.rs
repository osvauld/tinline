//! Scenarios fitted to real calls, from their logcat `video send` lines (one a second: size, fps,
//! encoder rate, overuse count, the last report's queueing). Each prints a second-by-second line
//! in the log's shape next to the target, so a sim run reads like the call it stands for:
//! `cargo test -p video --release --lib real_calls -- --nocapture --test-threads=1`
//! (`SIM_SECONDS=1` for the per-second lines, `SIM_SEED=<n>` for one seed).

use super::*;
use crate::media::HintReason;

/// One line a second, shaped like the device log's `video send` line: rung, frames encoded,
/// encoder rate, reports over the overuse threshold that second and the last report's queueing.
pub(super) fn seconds(r: &Report, sc: &Scenario) {
    if std::env::var_os("SIM_SECONDS").is_none() {
        return;
    }
    let mut t = 0;
    while t + S <= sc.duration_us {
        let end = t + S;
        let h = r.sizes.iter().rev().find(|x| x.0 < end).map_or(0, |x| x.2);
        let fps = r.encoded_in(t, end);
        let br = r.bitrate.iter().rev().find(|x| x.0 < end).map_or(0, |x| x.1);
        let reps: Vec<_> = r.reports.iter().filter(|x| x.0 >= t && x.0 < end).collect();
        let over = reps.iter().filter(|x| x.1.queuing_us > sc.sender.overuse_us).count();
        let q = reps.last().map_or(0, |x| x.1.queuing_us / 1000);
        println!("  {:3}s send {h}p@{fps} {:4}kbps overuse+{over} queue={q}ms", t / S, br / 1000);
        t = end;
    }
}

/// Last report's queueing once a second from `from` on, as percentiles (p25/50/75/90, max):
/// what the device log's `queue=` column gives.
fn queue_pcts(r: &Report, from: u64, to: u64) -> [u64; 5] {
    let mut v = Vec::new();
    let mut t = from;
    while t + S <= to {
        if let Some(x) = r.reports.iter().rev().find(|x| x.0 < t + S) {
            v.push(x.1.queuing_us / 1000);
        }
        t += S;
    }
    v.sort_unstable();
    let p = |q: f64| v.get(((v.len().saturating_sub(1)) as f64 * q) as usize).copied().unwrap_or(0);
    [p(0.25), p(0.5), p(0.75), p(0.9), p(1.0)]
}

fn overuse_per_s(r: &Report, sc: &Scenario, from: u64, to: u64) -> f64 {
    let n = r.reports.iter().filter(|x| x.0 >= from && x.0 < to && x.1.queuing_us > sc.sender.overuse_us).count();
    n as f64 / ((to - from) as f64 / 1e6)
}

fn seeds() -> Vec<u64> {
    let all = [3, 7, 11];
    all.into_iter().filter(|s| std::env::var("SIM_SEED").map_or(true, |v| v == s.to_string())).collect()
}

/// The moto g52 sending software AV1 while it decodes the other side's video in software as
/// well: ~18 ms a frame at 360p, ~35 ms at 540p, so 540p only at 15 fps (the 2026-10-19 relay
/// call: `send 360x480@24`, then `480x640@15`).
fn moto_busy() -> Device {
    Device { base_us: 4_000, ns_per_px: 60, jitter: 0.15, slowdowns: vec![], spikes: (0.0, 0), busy: vec![] }
}

// --- 2026-10-19 21:52-21:57: moto g52 (home Wi-Fi) -> a phone on mobile data, direct ---
//
// Real call: the picture was first hidden on the receiver (the double-picture bug: a 180p15 cap,
// `reason=display`); once that cap went, it stayed at 180x320 at 25-30 fps, ~100 kbps (25-240),
// on a path that had room for far more. The last report's queueing, once a second: p25 13, p50 27, p75 58, p90
// 154 ms, max 987; 3-5 reports a second over the 40 ms overuse threshold. Delay that does not
// follow our rate: the mobile downlink's scheduler holds packets and lets them go in bursts.

/// Mobile downlink delay (the radio scheduler and the phone's power save): every 100-900 ms a
/// 30-250 ms stretch where packets wait 30-200 ms more and then leave in order, in a burst;
/// every 15-40 s a longer stall (300-600 ms, up to a second more). Nothing lost.
fn mobile_downlink(rng: &mut Rng, end: u64) -> Vec<(u64, u64, u64)> {
    let ms = 1_000;
    let mut v = burst_train(rng, 300 * ms, end, (100 * ms, 900 * ms), (30 * ms, 250 * ms), (30 * ms, 200 * ms));
    let long = burst_train(rng, 10 * S, end, (15 * S, 40 * S), (300 * ms, 600 * ms), (500 * ms, 1_000 * ms));
    // A long stall replaces the short ones it overlaps.
    v.retain(|s| !long.iter().any(|l| s.0 < l.1 && l.0 < s.1));
    v.extend(long);
    v.sort_unstable();
    v
}

/// The same downlink as a smooth delay walk instead (see `Link::delay_walk`): every 150-600 ms
/// a new extra delay, 0-40 ms most of the time, 40-120 ms a third of the time, 120-300 ms one
/// time in ten, packets never stalled.
fn mobile_walk(rng: &mut Rng, end: u64) -> Vec<(u64, u64)> {
    let ms = 1_000;
    let mut v = vec![(0, 0)];
    let mut t = 0;
    while t < end {
        t += 150 * ms + (rng.f64() * 450_000.0) as u64;
        let p = rng.f64();
        let (lo, hi) = if p < 0.6 {
            (0, 40)
        } else if p < 0.9 {
            (40, 120)
        } else {
            (120, 300)
        };
        v.push((t, (lo + (rng.f64() * (hi - lo) as f64) as u64) * ms));
    }
    v
}

/// Which mobile-downlink model `moto_to_mobile` uses: `SIM_MOBILE=spikes|walk|both`.
#[derive(Clone, Copy, Debug)]
pub(super) enum Mobile {
    Spikes,
    Walk,
}

pub(super) fn mobile_models() -> Vec<Mobile> {
    match std::env::var("SIM_MOBILE").as_deref() {
        Ok("spikes") => vec![Mobile::Spikes],
        Ok("walk") => vec![Mobile::Walk],
        _ => vec![Mobile::Spikes, Mobile::Walk],
    }
}

pub(super) fn moto_to_mobile(seed: u64, model: Mobile) -> Scenario {
    let mut rng = Rng::new(seed ^ 0x6d6f);
    let mut sc = base();
    sc.duration_us = 120 * S;
    sc.seed = seed;
    sc.device = Device::phone();
    sc.decode_us = 6_000;
    let ms = 1_000;
    sc.link = Link {
        rate_bps: 8_000_000,
        delay_us: 35 * ms,
        jitter_us: 5 * ms,
        loss: 0.002,
        queue_bytes: 384 * 1024,
        ..Link::default()
    };
    match model {
        Mobile::Spikes => sc.link.spikes = mobile_downlink(&mut rng, sc.duration_us),
        Mobile::Walk => sc.link.delay_walk = mobile_walk(&mut rng, sc.duration_us),
    }
    // Hidden (180p15) until `LIFT`, then shown full screen.
    let hidden = Hint { max_pixels: 0, max_fps: 0, reason: HintReason::DisplaySize, room: false };
    let shown = Hint { max_pixels: u32::MAX, max_fps: 0, reason: HintReason::None, room: false };
    sc.hints = vec![(0, hidden), (LIFT, shown)];
    sc
}

const LIFT: u64 = 30 * S;

// Targets the controller doesn't meet yet (docs/design/video-sim-tuning.md): run them by name,
// with `--ignored`.
#[test]
#[ignore]
fn real_moto_to_mobile() {
    let mut fails = Vec::new();
    for (model, seed) in mobile_models().into_iter().flat_map(|m| seeds().into_iter().map(move |s| (m, s))) {
        let sc = moto_to_mobile(seed, model);
        let r = run(&sc, &mut FakeCodec::new(sc.fps, seed));
        let d = sc.duration_us;
        let from = LIFT + 10 * S;
        let q = queue_pcts(&r, from, d);
        let share = share_at_least(&r, 360, from, d);
        let kbps = r.mean_bitrate(from, d) / 1000.0;
        println!("mobile {model:?}/{seed}: {}", r.summary(d));
        println!(
            "  10 s after the cap goes: >= 360p {:.0}%, {kbps:.0} kbps, {:.1} overuse reports/s, queue p25/50/75/90/max {q:?} ms (real: 13/27/58/154/987, 3-5/s, 100 kbps) | {}",
            100.0 * share,
            overuse_per_s(&r, &sc, from, d),
            sizes_line(&r)
        );
        if std::env::var_os("SIM_STEPS").is_some() {
            println!("{}", r.steps_text());
        }
        seconds(&r, &sc);
        assert_eq!(r.decode_errors, 0);
        // 8 Mbps with delay that no rate causes: back to 360p within 10 s of the cap going, not
        // 180p at 100 kbps.
        if share < 0.8 || kbps < 500.0 {
            fails.push(format!("{model:?} seed {seed}: {:.0}% >= 360p, {kbps:.0} kbps", 100.0 * share));
        }
    }
    assert!(fails.is_empty(), "{fails:#?}");
}

// --- 2026-10-19 22:04: moto g52 -> Pixel 8 over a relay ---
//
// Real call: 360x480 at 22-25 fps (the moto's CPU: it also decodes the Pixel's 720p VP8 in
// software, 25-36 ms a frame), 1.2 Mbps, queue p90 10 ms; two dips a minute where the relay
// carried less than we sent (`starved`). After 24 s: `360p30 -> 540p15 reason=rate est=1200`,
// 1.5 Mbps, one report at 63 ms, then `540p15 -> 360p24 est=221kbps` and 300-480 kbps after.
// One delay step after a step up cut the rate to a fifth.

pub(super) fn moto_relay(seed: u64) -> Scenario {
    let mut rng = Rng::new(seed ^ 0x7265);
    let mut sc = base();
    sc.duration_us = 60 * S;
    sc.seed = seed;
    sc.device = moto_busy();
    sc.decode_us = 5_000;
    let ms = 1_000;
    let d = sc.duration_us;
    sc.link = Link {
        // 1.2 Mbps went through with 1-6 ms of queueing: the relay path itself is fast.
        rate_bps: 10_000_000,
        delay_us: 60 * ms,
        jitter_us: 4 * ms,
        loss: 0.002,
        queue_bytes: 256 * 1024,
        // The `starved` dips: twice a minute the relay carries 0.5-0.8 Mbps for 200-400 ms.
        rate_changes: relay_dips(&mut rng, d),
        // Relay hiccups: every 5-15 s, 100-300 ms where packets wait 50-90 ms more (the 63 ms
        // report).
        spikes: burst_train(&mut rng, 3 * S, d, (5 * S, 15 * S), (100 * ms, 300 * ms), (50 * ms, 90 * ms)),
        ..Link::default()
    };
    sc
}

// --- 2026-10-20 09:57-10:03: Pixel 3a (hw VP8) -> moto g52, both on mobile data, via the relay ---
//
// Real call (Today, 226 s): srtt median 367 ms, p90 458 ms. The Pixel's estimate swung
// 100 <-> 500 kbps: 180p30 -> 360p30 at ~500 kbps, then back within 2 s at 100-170 kbps, every
// 10-13 s; 35 steps, 75 keyframes, 58 starved reports. Each step up's keyframe filled the path
// and starved it, so the flap guard (which treats a starved drop as the link getting slower)
// never fired. Cut (117 s): 13 steps, 45 keyframes.

/// The slow relay path: ~350 ms round trip, 250-700 kbps that changes every 3-8 s, a short
/// buffer (~300 ms at 600 kbps), and every 8-20 s a 150-400 ms stretch where packets wait
/// 80-250 ms more.
pub(super) fn slow_relay(seed: u64) -> Scenario {
    let mut rng = Rng::new(seed ^ 0x736c);
    let mut sc = base();
    sc.duration_us = 120 * S;
    sc.seed = seed;
    sc.device = fast();
    sc.decode_us = 6_000;
    let ms = 1_000;
    let d = sc.duration_us;
    sc.link = Link {
        rate_bps: 450_000,
        delay_us: 165 * ms,
        jitter_us: 15 * ms,
        loss: 0.003,
        queue_bytes: 24 * 1024,
        rate_changes: rate_walk(&mut rng, d, (3 * S, 8 * S), (250_000, 700_000)),
        spikes: burst_train(&mut rng, 5 * S, d, (8 * S, 20 * S), (150 * ms, 400 * ms), (80 * ms, 250 * ms)),
        ..Link::default()
    };
    sc
}

fn relay_dips(rng: &mut Rng, end: u64) -> Vec<(u64, u64)> {
    let pick = |rng: &mut Rng, (lo, hi): (u64, u64)| lo + (rng.f64() * (hi - lo) as f64) as u64;
    let mut v = Vec::new();
    let mut t = pick(rng, (10 * S, 30 * S));
    while t < end {
        v.push((t, pick(rng, (500_000, 800_000))));
        t += pick(rng, (200_000, 400_000));
        v.push((t, 10_000_000));
        t += pick(rng, (20 * S, 40 * S));
    }
    v
}

#[test]
#[ignore]
fn real_moto_relay() {
    let mut fails = Vec::new();
    for seed in seeds() {
        let sc = moto_relay(seed);
        let r = run(&sc, &mut FakeCodec::new(sc.fps, seed));
        let d = sc.duration_us;
        let kbps = r.mean_bitrate(10 * S, d) / 1000.0;
        // Over 3 s: a dip the relay really had (0.5-0.8 Mbps for up to 400 ms) and the drain
        // after it don't count; the call's cut to 221 kbps for the rest of it does.
        let low = (10..d / S - 2).map(|t| r.mean_bitrate(t * S, (t + 3) * S) as u64).min().unwrap_or(0) / 1000;
        let lowest = min_height(&r, 10 * S, d);
        println!("relay/{seed}: {}", r.summary(d));
        println!(
            "  from 10 s: {kbps:.0} kbps, lowest 3 s {low} kbps, lowest {lowest}p, p95 {:.0} ms (real: 1.2 Mbps, then 221 kbps after one 63 ms report) | {}",
            p95(&r, 10 * S, d),
            sizes_line(&r)
        );
        if std::env::var_os("SIM_STEPS").is_some() {
            println!("{}", r.steps_text());
        }
        seconds(&r, &sc);
        assert_eq!(r.decode_errors, 0);
        // A fast relay with short dips and hiccups: never below 360p or 500 kbps once going.
        if lowest < 360 || low < 500 || kbps < 900.0 {
            fails.push(format!("seed {seed}: {kbps:.0} kbps, lowest {low} kbps / {lowest}p"));
        }
    }
    assert!(fails.is_empty(), "{fails:#?}");
}

// --- 2026-10-20 11:55-11:59: Pixel 3a <-> moto g52, both ways at once, via the relay ---
//
// Real call (srtt ~364 ms): the Pixel (hw VP8) climbed to 480x640 at 3 Mbps within 21 s, held it
// for ~40 s, then fell to 180p within 3 s (11:56:05-13: 3000 -> 78 kbps, overuse 34 -> 82); the
// moto (software AV1, 360p at 1.2 Mbps) fell to 180p at 11:58:46. The two steps up and falls
// lined up: each phone's upload carries its own video and its feedback (acks, reports) on the
// other phone's, so one side filling its upload makes the other side's feedback late, which
// reads as that side's own path queueing. `run_duplex` models this; the uplink rates are
// guesses within what the call showed (the Pixel's held 3 Mbps for a while, the moto's 1.2).

/// Pixel -> moto (`.0`, over the Pixel's upload) and moto -> Pixel (`.1`, the moto's upload).
/// Fitted to the call's first minute: the Pixel's path carried 3 Mbps with 5 ms of queueing
/// (11:55:23-11:56:02), so a clean 3.5 Mbps upload that falls to 1 Mbps at 60 s (the fall the
/// call had at 11:56:05; when the cell's rate fell or the moto's upload filled, the log can't
/// tell). The moto's upload: 1.5 Mbps (it held 1.2 Mbps for a minute). `SIM_DUPLEX_FALL=<kbps>`
/// sets what the Pixel's falls to.
pub(super) fn duplex_relay(seed: u64) -> (Scenario, Scenario) {
    let ms = 1_000;
    let d = 120 * S;
    let fall = std::env::var("SIM_DUPLEX_FALL").ok().and_then(|v| v.parse::<u64>().ok()).map_or(1_000_000, |k| k * 1000);
    // `SIM_JITTER_MS`: the 14:38 call's relay swung +-40 ms (audio rtt p10 262, p90 345 ms).
    let jitter = std::env::var("SIM_JITTER_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(3);
    let side = |rng: &mut Rng, rate: u64, changes: Vec<(u64, u64)>| Link {
        rate_bps: rate,
        delay_us: 180 * ms,
        jitter_us: jitter * ms,
        // No loss: over the relay, iroh's QUIC rides the relay's TCP connection, which resends
        // what it loses (QUIC sees delay instead). With 0.2% loss at this RTT the model's QUIC
        // window alone held the rate near 600 kbps, where the call carried 3 Mbps.
        loss: 0.0,
        queue_bytes: 128 * 1024,
        rate_changes: changes,
        spikes: burst_train(rng, 5 * S, d, (15 * S, 30 * S), (100 * ms, 300 * ms), (50 * ms, 150 * ms)),
        ..Link::default()
    };
    let mut rng = Rng::new(seed ^ 0x6475);
    let mut pixel = base();
    pixel.duration_us = d;
    pixel.seed = seed;
    pixel.device = fast();
    // The moto decodes VP8 in software: 9-18 ms a frame (11:57 log).
    pixel.decode_us = 12_000;
    pixel.link = side(&mut rng, 3_500_000, vec![(60 * S, fall)]);
    let mut moto = base();
    moto.duration_us = d;
    moto.seed = seed ^ 0x6d;
    moto.device = moto_busy();
    // The Pixel decodes AV1 with rav1d: ~8 ms at 360p.
    moto.decode_us = 8_000;
    moto.link = side(&mut rng, 1_500_000, vec![]);
    (pixel, moto)
}

#[test]
#[ignore]
fn real_duplex_relay() {
    // Before the Pixel's upload falls (10-60 s) and after it (65 s on).
    let row = |name: &str, r: &Report, sc: &Scenario| {
        row_in(name, r, sc, 10 * S, 60 * S);
        row_in("  after 60 s", r, sc, 65 * S, sc.duration_us);
    };
    fn row_in(name: &str, r: &Report, sc: &Scenario, from: u64, to: u64) {
        let overuse = r.reports.iter().filter(|x| x.0 >= from && x.0 < to && x.1.queuing_us > sc.sender.overuse_us).count();
        let shown = r.shown.iter().filter(|s| s.at_us >= from && s.at_us < to).count();
        println!(
            "  {name:<14} {:5.0} kbps | >=540p {:3.0}% | {:4.1} fps shown | p95 {:5.0} ms | frz {:4.1} s | steps {:3} | overuse {overuse:3}",
            r.mean_bitrate(from, to) / 1000.0,
            100.0 * share_at_least(r, 540, from, to),
            shown as f64 / ((to - from) as f64 / 1e6),
            p95(r, from, to),
            r.freezes(250_000, from, to).iter().map(|f| f.1).sum::<u64>() as f64 / 1e6,
            r.steps.iter().filter(|x| x.at_us >= from && x.at_us < to).count(),
        );
    }
    for seed in seeds() {
        let (a, b) = duplex_relay(seed);
        let (ra, rb) = run_duplex(&a, &mut pixel_codec(&a), &b, &mut FakeCodec::new(b.fps, b.seed));
        let sa = run(&a, &mut pixel_codec(&a));
        let sb = run(&b, &mut FakeCodec::new(b.fps, b.seed));
        println!("duplex relay / seed {seed} (alone: the other direction's feedback has no queue to wait in)");
        row("pixel together", &ra, &a);
        row("pixel alone", &sa, &a);
        row("moto together", &rb, &b);
        row("moto alone", &sb, &b);
        if std::env::var_os("SIM_STEPS").is_some() {
            println!("pixel:\n{}\nmoto:\n{}", ra.steps_text(), rb.steps_text());
        }
        assert_eq!(ra.decode_errors + rb.decode_errors, 0);
    }
}
