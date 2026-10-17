//! Call scenarios. Targets are what a good 1:1 video call should feel like; each test prints
//! its report (`cargo test -p video sim -- --nocapture`).

use super::*;

const S: u64 = 1_000_000;

/// Defaults. Overrides for experiments: `SIM_BACKLOG=unsent` (QUIC-unsent backlog model),
/// `SIM_DRAIN=<pct>`, `SIM_STARVE=<pct>` (100 / 0 turn them off), `SIM_UP_DIP=<pct>` (0: any dip
/// restarts the ladder's step-up hold), `SIM_STAMP_CAPTURE=1` (frames stamped at capture),
/// `SIM_FAST_HOLD=<ms>` (the start's fast step-up hold; 0: off, the controller before it).
fn base() -> Scenario {
    let mut sc = Scenario::default();
    if std::env::var("SIM_BACKLOG").is_ok_and(|v| v == "unsent") {
        sc.backlog = Backlog::Unsent;
    }
    let knob = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<u32>().ok());
    if let Some(v) = knob("SIM_DRAIN") {
        sc.sender.drain_pct = v;
    }
    if let Some(v) = knob("SIM_STARVE") {
        sc.sender.starve_pct = v;
    }
    if let Some(v) = knob("SIM_UP_DIP") {
        sc.sender.ladder.up_dip_pct = v;
    }
    if std::env::var_os("SIM_STAMP_CAPTURE").is_some() {
        sc.stamp_capture = true;
    }
    if let Some(v) = knob("SIM_FAST_HOLD") {
        sc.sender.ladder.up_fast_hold_us = if v == 0 { u64::MAX } else { v as u64 * 1_000 };
    }
    sc
}

fn go(name: &str, sc: &Scenario) -> Report {
    let mut codec = FakeCodec::new(sc.fps, sc.seed);
    let r = run(sc, &mut codec);
    println!("{name:>10}: {}", r.summary(sc.duration_us));
    if std::env::var_os("SIM_STEPS").is_some() {
        println!("{}", r.steps_text());
    }
    r
}

/// With `SIM_TRACE=1`: every 100 ms of `[from, to)`, the encoder bitrate, bottleneck queue,
/// last report and the latency of frames shown.
/// `SIM_FROM=<s> SIM_TO=<s>` replace the window.
fn trace(r: &Report, from: u64, to: u64) {
    if std::env::var_os("SIM_TRACE").is_none() {
        return;
    }
    let secs = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<f64>().ok()).map(|s| (s * 1e6) as u64);
    let (from, to) = (secs("SIM_FROM").unwrap_or(from), secs("SIM_TO").unwrap_or(to));
    if std::env::var("SIM_TRACE").is_ok_and(|v| v == "reports") {
        for (at, rp) in r.reports.iter().filter(|x| x.0 >= from && x.0 < to) {
            let br = r.bitrate.iter().rev().find(|x| x.0 <= *at).map_or(0, |x| x.1);
            let q = r.queue.iter().rev().find(|x| x.0 <= *at).map_or(0, |x| x.1);
            println!(
                "  {:7.3}s delivered {:5} kbps queuing {:4} ms | br after {:5} kbps q {:6} B",
                *at as f64 / 1e6,
                rp.delivered_bps / 1000,
                rp.queuing_us / 1000,
                br / 1000,
                q
            );
        }
        return;
    }
    let mut t = from;
    while t < to {
        let end = t + 100_000;
        let last = |v: &[(u64, u32)]| v.iter().rev().find(|x| x.0 < end).map(|x| x.1);
        let q = r.queue.iter().rev().find(|x| x.0 < end).map(|x| x.1).unwrap_or(0);
        let rep = r.reports.iter().rev().find(|x| x.0 < end).map(|x| (x.1.delivered_bps / 1000, x.1.queuing_us / 1000));
        let shown: Vec<_> = r.shown.iter().filter(|s| s.at_us >= t && s.at_us < end).collect();
        let lat = shown.iter().map(|s| (s.at_us - s.capture_us) / 1000).max();
        let h = shown.last().map(|s| s.height);
        println!(
            "  {:5.1}s br {:5} kbps q {:6} B rep {:?} shown {:2} max lat {:?} ms h {:?}",
            t as f64 / 1e6,
            last(&r.bitrate).unwrap_or(0) / 1000,
            q,
            rep,
            shown.len(),
            lat,
            h
        );
        t = end;
    }
}

fn p95(r: &Report, from: u64, to: u64) -> f64 {
    Report::pct(&r.latencies_ms(from, to), 0.95)
}

#[test]
fn clean_link() {
    let sc = base();
    let r = go("clean", &sc);
    assert_eq!(r.decode_errors, 0);
    assert_eq!(r.keyframes, 1 + r.switches, "keyframes only for resolution changes");
    // 360p -> 540p30 -> 720p24: 720p takes ~18.5 ms to encode on the phone model, over 30 fps's
    // 20 ms budget at the p75 but within 24 fps's 25 ms (size before frame rate, D1).
    assert!(r.switches <= 2, "{:?}", r.sizes);
    assert_eq!(shown_height(&r, 15 * S, sc.duration_us), (720, true));
    assert!(r.shown.len() >= r.encoded.len() - 3, "every frame shown");
    // 122 ms (was 108 with frames stamped at capture: each step up's longer encode read as a
    // queue and held the rate ~20% under this 2 Mbps link; now it runs at ~1.9 Mbps).
    // 131 ms with the sender cap at 3 Mbps (was 128): it now probes past this 2 Mbps link.
    assert!(p95(&r, 2 * S, sc.duration_us) < 135.0, "p95 {}", p95(&r, 2 * S, sc.duration_us));
    assert!(r.freezes(250_000, S, sc.duration_us).is_empty());
    assert!(r.mean_bitrate(20 * S, 30 * S) > 1_000_000.0, "ramps up to use a 2 Mbps link");
}

#[test]
fn random_loss_3pct() {
    let sc = Scenario { link: Link { loss: 0.03, ..Link::default() }, ..base() };
    let r = go("loss 3%", &sc);
    assert_eq!(r.decode_errors, 0);
    assert!(p95(&r, 2 * S, sc.duration_us) < 200.0, "p95 {}", p95(&r, 2 * S, sc.duration_us));
    assert!(r.freezes(250_000, S, sc.duration_us).len() <= 2);
}

#[test]
fn bandwidth_drop() {
    let sc = Scenario {
        link: Link { rate_changes: vec![(10 * S, 300_000)], ..Link::default() },
        ..base()
    };
    let r = go("2M->300k", &sc);
    trace(&r, 9_900_000, 11_500_000);
    assert_eq!(r.decode_errors, 0);
    // Adapts within a few seconds and stays interactive at the lower rate.
    assert!(r.mean_bitrate(15 * S, 30 * S) < 300_000.0, "bitrate {}", r.mean_bitrate(15 * S, 30 * S));
    assert!(p95(&r, 15 * S, 30 * S) < 120.0, "p95 {}", p95(&r, 15 * S, 30 * S));
    // The queue built before the drop is noticed drains within about a second.
    assert!(p95(&r, 0, 30 * S) < 200.0, "p95 {}", p95(&r, 0, 30 * S));
    // Worst frame after the drain (was < 150 ms): the queueing delay is the least delayed frame
    // of the last 100 ms, so the ramp's sawtooth at 300 kbps is seen a frame later (~152 ms).
    // The drain ends at ~11.8 s (was ~10.9 s with capture stamps, which held the rate at ~1.6
    // instead of ~1.9 Mbps on this 2 Mbps link: more is in flight when it drops).
    assert!(Report::pct(&r.latencies_ms(12 * S, 30 * S), 1.0) < 165.0);
    let f: u64 = r.freezes(250_000, 10 * S, 30 * S).iter().map(|f| f.1).sum();
    assert!(f < 300_000, "frozen {} ms after the drop", f / 1000);
}

#[test]
fn deep_buffer_drop() {
    // Cellular-style 256 KB bottleneck buffer (7 s at 300 kbps), 2 Mbps -> 300 kbps at 10 s.
    let sc = Scenario {
        link: Link { queue_bytes: 256 * 1024, rate_changes: vec![(10 * S, 300_000)], ..Link::default() },
        ..base()
    };
    let r = go("deep 300k", &sc);
    trace(&r, 9_800_000, 14 * S);
    assert_eq!(r.decode_errors, 0);
    // Only what was sent before the drop was noticed ever queues: the buffer size doesn't matter.
    let peak = r.queue.iter().map(|q| q.1).max().unwrap();
    assert!(peak < 64 * 1024, "queue peaked at {peak} B");
    assert!(p95(&r, 0, 30 * S) < 200.0, "p95 {}", p95(&r, 0, 30 * S));
    // Worst frame after the drain (was < 150 ms): the queueing delay is the least delayed frame
    // of the last 100 ms, so the ramp's sawtooth at 300 kbps is seen a frame later (~152 ms).
    // The drain ends at ~11.8 s (was ~10.9 s with capture stamps, which held the rate at ~1.6
    // instead of ~1.9 Mbps on this 2 Mbps link: more is in flight when it drops).
    assert!(Report::pct(&r.latencies_ms(12 * S, 30 * S), 1.0) < 165.0);
}

#[test]
fn outage_500ms() {
    let sc = Scenario {
        link: Link { outages: vec![(10 * S, 10 * S + 500_000)], ..Link::default() },
        ..base()
    };
    let r = go("outage", &sc);
    assert_eq!(r.decode_errors, 0);
    let f: u64 = r.freezes(250_000, 9 * S, 30 * S).iter().map(|f| f.1).sum();
    assert!(f < 1_500_000, "frozen {} ms for a 500 ms outage", f / 1000);
    // 720p24 from 8 s since the fast start (was 12 s): ~1.9 Mbps on this 2 Mbps link, p95 ~125 ms
    // as on the clean link (was < 120; over the run 130 -> 129 ms, frozen 0.9 -> 0.7 s).
    assert!(p95(&r, 13 * S, 30 * S) < 135.0, "back to low latency, p95 {}", p95(&r, 13 * S, 30 * S));
}

#[test]
fn relay_path() {
    // Through an iroh relay: longer, jittery path.
    let sc = Scenario {
        link: Link { delay_us: 75_000, jitter_us: 20_000, rate_bps: 1_500_000, ..Link::default() },
        ..base()
    };
    let r = go("relay", &sc);
    assert_eq!(r.decode_errors, 0);
    assert!(p95(&r, 2 * S, sc.duration_us) < 250.0, "p95 {}", p95(&r, 2 * S, sc.duration_us));
}

#[test]
fn real_av1_under_loss_and_outage() {
    // The same pipeline with real libaom packets decoded by rav1d: every frame the receiver
    // hands to the decoder must decode, across group switches and resets.
    let sc = Scenario {
        duration_us: 12 * S,
        link: Link { loss: 0.02, outages: vec![(5 * S, 5 * S + 400_000)], ..Link::default() },
        ..base()
    };
    let mut codec = Av1Codec::new(640, 360, sc.fps, sc.sender.start_bps);
    let r = run(&sc, &mut codec);
    println!("{:>10}: {}", "av1", r.summary(sc.duration_us));
    assert_eq!(r.decode_errors, 0);
    assert!(r.shown.len() > r.encoded.len() * 8 / 10);
}

/// Smallest height shown in `[from, to)`.
fn min_height(r: &Report, from: u64, to: u64) -> u32 {
    r.shown.iter().filter(|s| s.at_us >= from && s.at_us < to).map(|s| s.height).min().unwrap()
}

/// Frame rate (tier) at `t`.
fn fps_at(r: &Report, t: u64) -> u32 {
    r.fps.iter().rev().find(|f| f.0 <= t).map_or(30, |f| f.1)
}

/// Height shown at the end of `[from, to)`, and whether every frame shown there had it.
fn shown_height(r: &Report, from: u64, to: u64) -> (u32, bool) {
    let v: Vec<u32> = r.shown.iter().filter(|s| s.at_us >= from && s.at_us < to).map(|s| s.height).collect();
    let last = *v.last().unwrap();
    (last, v.iter().all(|&h| h == last))
}

#[test]
fn ladder_follows_bandwidth_dip() {
    let sc = Scenario {
        duration_us: 40 * S,
        link: Link { rate_changes: vec![(10 * S, 200_000), (20 * S, 2_000_000)], ..Link::default() },
        ..base()
    };
    let r = go("ladder dip", &sc);
    trace(&r, 9_800_000, 14 * S);
    println!("{:>10}  sizes {:?}", "", r.sizes);
    assert_eq!(r.decode_errors, 0);
    assert!(min_height(&r, 8 * S, 10 * S) >= 540, "uses the 2 Mbps link: {:?}", r.sizes);
    // Down to 320x180 within a second of the drop, and stays there while it lasts.
    let down = r.sizes.iter().find(|s| s.0 >= 10 * S && s.2 == 180).expect("steps down");
    assert!(down.0 < 11 * S, "180p at {} ms", down.0 / 1000);
    // (From 13 s: the frames queued before the drop are shown until ~12.4 s, see below.)
    assert_eq!(shown_height(&r, 13 * S, 20 * S), (180, true));
    // Back up after recovery, one rung at a time (10 s after the step down, 5 s holds), without
    // flapping: 360p at ~26 s, 540p at ~32 s, then (the phone model) 720p24 at ~38 s. The drop
    // starved the link, so it isn't a flap: no block.
    assert!(min_height(&r, 33 * S, 40 * S) >= 540, "{:?}", r.sizes);
    // (6: since the fast start it reaches 540p at 2.4 s and 720p24 at 8.2 s before the drop.)
    assert!(r.switches <= 6, "{} switches: {:?}", r.switches, r.sizes);
    assert!(p95(&r, 21 * S, 40 * S) < 120.0, "p95 {}", p95(&r, 21 * S, 40 * S));
    // The queue built in the ~150 ms before the drop shows drains within about two seconds at
    // 200 kbps: p95 568 ms over the run and 1.6 s frozen (was 114 ms and 0.3 s with frames stamped
    // at capture, whose encode-time "queue" after each step up held the rate at ~1.6 instead of
    // ~1.9 Mbps on this 2 Mbps link; the trade-off is in docs/video.md).
    assert!(p95(&r, 0, 40 * S) < 650.0, "p95 {}", p95(&r, 0, 40 * S));
    // (Up to 37 s: the 720p24 step near the link's capacity costs ~190 ms for a second.)
    let worst = Report::pct(&r.latencies_ms(13 * S, 37 * S), 1.0);
    assert!(worst < 150.0, "{worst}");
    let f: u64 = r.freezes(250_000, S, 40 * S).iter().map(|f| f.1).sum();
    assert!(f < 2_000_000, "frozen {} ms", f / 1000);
}

#[test]
fn ladder_slow_device_stays_low() {
    // Codec on the little cores: 640x360 takes ~12 ms, 960x540 ~21 ms: over 30 fps's 20 ms
    // budget, within 24 fps's 25 ms. Acceptance 5: the bigger picture at a lower frame rate
    // (was 360p30 throughout): 540p15 (predicted from 360p) at ~6 s, then 540p24 once 540p's
    // own encode times show it fits.
    let sc = Scenario { device: Device::phone_unpinned(), ..base() };
    let r = go("slow dev", &sc);
    println!("{:>10}  sizes {:?}", "", r.sizes);
    assert_eq!(r.decode_errors, 0);
    assert_eq!(shown_height(&r, 8 * S, sc.duration_us), (540, true));
    assert_eq!(r.switches, 1);
    assert_eq!(fps_at(&r, sc.duration_us), 24);
    assert_eq!(r.camera_drops, 0);
    assert!(p95(&r, 2 * S, sc.duration_us) < 130.0, "p95 {}", p95(&r, 2 * S, sc.duration_us));
}

#[test]
fn ladder_steps_down_when_throttled() {
    // Fast phone at 960x540 until it throttles to half speed at 15 s (540p then takes ~23 ms).
    let sc = Scenario {
        device: Device { slowdowns: vec![(15 * S, 2.0)], ..Device::phone() },
        ..base()
    };
    let r = go("throttled", &sc);
    println!("{:>10}  sizes {:?}", "", r.sizes);
    assert_eq!(r.decode_errors, 0);
    // 720p24 before (see clean_link); at 2x slower the frame rate goes first, within a second
    // and without a keyframe; then 540p (was: 540p -> 360p at 15.7 s, and stayed).
    assert_eq!(shown_height(&r, 13 * S, 15 * S), (720, true));
    let first = r.steps.iter().find(|s| s.at_us >= 15 * S).expect("steps down");
    assert!(first.at_us < 16 * S && first.to.0 == 1280 && first.to.2 < 24, "{first:?}");
    assert!(min_height(&r, 15 * S, sc.duration_us) >= 540, "{:?}", r.sizes);
    assert_eq!(r.key_causes.cpu + r.key_causes.rate, r.switches);
    assert!(r.switches <= 3, "{:?}", r.sizes);
}

#[test]
fn real_av1_across_resolution_switch() {
    // Real libaom packets through rav1d while the size goes down and back up: one decoder for
    // the whole call, each new size arriving in a keyframe's sequence header.
    let sc = Scenario {
        duration_us: 20 * S,
        link: Link { rate_changes: vec![(6 * S, 200_000), (12 * S, 2_000_000)], ..Link::default() },
        ..base()
    };
    let mut codec = Av1Codec::new(640, 360, sc.fps, sc.sender.start_bps);
    let r = run(&sc, &mut codec);
    println!("{:>10}: {}", "av1 switch", r.summary(sc.duration_us));
    println!("{:>10}  decoded {:?}", "", codec.decoded_sizes);
    trace(&r, 5_800_000, 10 * S);
    assert_eq!(r.decode_errors, 0);
    let small = codec.decoded_sizes.iter().position(|&s| s == (320, 180)).expect("decoded 180p");
    assert!(codec.decoded_sizes[small + 1..].contains(&(640, 360)), "{:?}", codec.decoded_sizes);
    let f: u64 = r.freezes(250_000, S, sc.duration_us).iter().map(|f| f.1).sum();
    // The 540p frames queued before the drop was noticed (see docs/video.md).
    assert!(f < 2 * S, "frozen {} ms", f / 1000);
}

#[test]
fn slow_start() {
    // The first second encodes 4x slower (encoder warm-up, a busy machine at call start), then
    // the phone's normal speed: a moment of slowness must not cost half a minute of 180p.
    let sc = Scenario {
        device: Device { slowdowns: vec![(0, 4.0), (S, 0.25)], ..Device::phone() },
        ..base()
    };
    let r = go("slow start", &sc);
    println!("{:>10}  sizes {:?}", "", r.sizes);
    assert_eq!(r.decode_errors, 0);
    let at540 = r.sizes.iter().find(|s| s.2 == 540).expect("reaches 540p");
    assert!(at540.0 < 10 * S, "540p only at {} ms", at540.0 / 1000);
    // (Then 720p24, see clean_link.)
    assert!(min_height(&r, 12 * S, sc.duration_us) >= 540, "{:?}", r.sizes);
}

#[test]
fn busy_desktop_steady_call() {
    // Clean 2 Mbps link, encoder on a busy desktop (jittery, now and then preempted): a steady
    // call sends keyframes only at the start and for a resolution change.
    let sc = Scenario { duration_us: 50 * S, device: Device::busy_desktop(), ..base() };
    let r = go("busy desk", &sc);
    trace(&r, 48 * S, 48_600_000);
    println!("{:>10}  sizes {:?}", "", r.sizes);
    assert_eq!(r.decode_errors, 0);
    let k = r.key_causes;
    assert_eq!((k.request, k.backlog, k.other), (0, 0, 0), "{k:?}");
    assert!(r.keyframes <= 3, "{} keyframes: {k:?}", r.keyframes);
    assert!(r.mean_bitrate(10 * S, sc.duration_us) > 1_000_000.0);
}

#[test]
fn receiver_clock_drift() {
    // The receiver's clock runs 0.5% fast (a VM, an emulator): the queueing delay measured
    // against the 10 s minimum reads ~50 ms with no queue at all.
    // 40 s (was 30): back up from 180p takes longer with the 10 s wait after a step down and the
    // 5 s holds (180p 10 s -> 360p 20 s -> 540p 29 s).
    let sc = Scenario { duration_us: 40 * S, receiver_clock_ppm: 5_000, ..base() };
    let r = go("drift", &sc);
    trace(&r, 6 * S, 30 * S);
    println!("{:>10}  sizes {:?}", "", r.sizes);
    assert_eq!(r.decode_errors, 0);
    let br = r.mean_bitrate(30 * S, sc.duration_us);
    assert!(br > 1_000_000.0, "stuck at {br}");
}

#[test]
fn slow_receiver() {
    // The receiving device decodes at 25 fps (40 ms a frame) from 5 s to 15 s, then normally.
    // VideoRecv only reads while the decoder keeps up, so this looks like a slow link.
    let sc = Scenario { decode_changes: vec![(5 * S, 40_000), (15 * S, 2_000)], ..base() };
    let r = go("slow recv", &sc);
    trace(&r, 4_800_000, 20 * S);
    println!("{:>10}  sizes {:?}", "", r.sizes);
    assert_eq!(r.decode_errors, 0);
    // Recovers once the decoder does: low latency and the link used again.
    assert!(p95(&r, 18 * S, sc.duration_us) < 120.0, "p95 {}", p95(&r, 18 * S, sc.duration_us));
    let br = r.mean_bitrate(22 * S, sc.duration_us);
    assert!(br > 1_000_000.0, "bitrate {br}");
}

// --- Video adaptation (docs/design/video-adaptation.md, section 7) ---

/// Laptop-class encoder (or a hardware one): 720p in ~9 ms, so only the link limits the ladder.
fn fast() -> Device {
    Device { base_us: 1_500, ns_per_px: 8, jitter: 0.2, slowdowns: vec![], spikes: (0.0, 0), busy: vec![] }
}

/// Resolution switches at or after `from`.
fn res_switches(r: &Report, from: u64) -> usize {
    r.sizes.iter().skip(1).filter(|s| s.0 >= from).count()
}

fn sizes_line(r: &Report) -> String {
    r.sizes.iter().map(|(t, _, h)| format!("{h}p@{:.1}", *t as f64 / 1e6)).collect::<Vec<_>>().join(" ")
}

#[test]
fn adapt_fps_before_resolution() {
    // Acceptance 1: 720p on 2 Mbps, then 700 kbps.
    let sc = Scenario {
        duration_us: 40 * S,
        device: fast(),
        link: Link { rate_changes: vec![(20 * S, 700_000)], ..Link::default() },
        ..base()
    };
    let r = go("fps first", &sc);
    println!("{:>10}  {}", "", sizes_line(&r));
    assert_eq!(r.decode_errors, 0);
    assert_eq!(shown_height(&r, 15 * S, 20 * S), (720, true));
    // 700 kbps delivers ~600 kbps of estimate (85%): under 720p15's 700 kbps floor, so one
    // resolution step, to 540p24, not 360p30 (was: 720p -> 360p).
    assert!(min_height(&r, 7 * S, sc.duration_us) >= 540, "{}", sizes_line(&r));
    // Baseline: p95 116 ms, no freezes (128 ms with the sender cap at 3 Mbps: the 2 Mbps phase
    // of this link is now probed past its capacity).
    assert!(p95(&r, 0, sc.duration_us) <= 130.0, "p95 {}", p95(&r, 0, sc.duration_us));
    assert!(r.freezes(250_000, S, sc.duration_us).is_empty());
    assert_eq!(r.keyframes, 1 + r.switches, "no keyframes for frame-rate steps");
}

#[test]
fn adapt_fps_before_resolution_1mbps() {
    // Acceptance 1 where the estimate covers 720p15 (a 1 Mbps link): the size stays.
    let sc = Scenario {
        duration_us: 40 * S,
        device: fast(),
        link: Link { rate_changes: vec![(20 * S, 1_000_000)], ..Link::default() },
        ..base()
    };
    let r = go("fps 1M", &sc);
    println!("{:>10}  {}", "", sizes_line(&r));
    assert_eq!(r.decode_errors, 0);
    // The first step is a frame-rate one, within the second look, without a keyframe.
    let step = r.steps.iter().find(|s| s.at_us >= 20 * S).expect("steps down");
    assert!(step.at_us < 20_500_000 && step.to.0 == 1280 && step.to.2 < 30, "{step:?}");
    assert_eq!(shown_height(&r, 15 * S, 20_600_000), (720, true));
    // Open issue: at 720p15 each frame is ~50 ms on a 1 Mbps link, the delay signal is noisier,
    // and the estimate drifts below 720p15's floor (540p30 at ~24 s). Still >= 540p.
    assert!(min_height(&r, 7 * S, sc.duration_us) >= 540, "{}", sizes_line(&r));
    assert_eq!(r.keyframes, 1 + r.switches, "no keyframes for frame-rate steps");
    // 129 ms (was 118): since the fast start it runs 720p30 near this 2 Mbps link's capacity
    // from 8 s instead of 12 s, and stays at 720p15 after the drop (was 540p30 from 29 s).
    assert!(p95(&r, 0, sc.duration_us) <= 135.0, "p95 {}", p95(&r, 0, sc.duration_us));
    assert!(r.freezes(250_000, S, sc.duration_us).is_empty());
}

#[test]
fn adapt_no_bouncing_under_loss() {
    // Acceptance 2: 1.5 Mbps, 3% random loss, 60 s.
    let sc = Scenario {
        duration_us: 60 * S,
        device: fast(),
        link: Link { rate_bps: 1_500_000, loss: 0.03, ..Link::default() },
        ..base()
    };
    let r = go("no bounce", &sc);
    println!("{:>10}  {} | after 10 s: {}", "", sizes_line(&r), res_switches(&r, 10 * S));
    assert_eq!(r.decode_errors, 0);
    // Baseline: 2 after 10 s (540p -> 720p at 39.8 s -> 540p 0.3 s later).
    assert!(res_switches(&r, 10 * S) <= 1, "{}", sizes_line(&r));
    assert!(p95(&r, 2 * S, sc.duration_us) < 130.0);
}

#[test]
fn adapt_flapping_link() {
    // Acceptance 6: 2 Mbps / 900 kbps, alternating every 8 s.
    let changes = (1..10u64).map(|i| (i * 8 * S, if i % 2 == 1 { 900_000 } else { 2_000_000 })).collect();
    let sc = Scenario { duration_us: 80 * S, device: fast(), link: Link { rate_changes: changes, ..Link::default() }, ..base() };
    let r = go("flap link", &sc);
    println!("{:>10}  {} | 20-80 s: {}", "", sizes_line(&r), res_switches(&r, 20 * S));
    assert_eq!(r.decode_errors, 0);
    // Baseline: 7 in 20-80 s (540p <-> 720p every 8-16 s). Now 900 kbps holds 720p15 / 540p at
    // a lower tier, and a step up undone within 20 s is blocked.
    assert!(res_switches(&r, 20 * S) <= 2, "{}", sizes_line(&r));
    assert!(p95(&r, 0, sc.duration_us) <= 134.0, "p95 {}", p95(&r, 0, sc.duration_us));
}

/// A fast device on a 3 Mbps link with `hints` from the receiving app.
fn hinted(name: &str, hints: Vec<(u64, Hint)>) -> Report {
    let sc = Scenario {
        duration_us: 50 * S,
        device: fast(),
        link: Link { rate_bps: 3_000_000, ..Link::default() },
        hints,
        ..base()
    };
    let r = go(name, &sc);
    println!("{:>10}  {}", "", sizes_line(&r));
    if std::env::var_os("SIM_STEPS").is_none() {
        println!("{}", r.steps_text());
    }
    assert_eq!(r.decode_errors, 0);
    r
}

#[test]
fn adapt_drawn_size_hint() {
    // Acceptance 3: the picture is drawn 640x360 from 5 s to 25 s.
    use crate::media::HintReason;
    let small = Hint { max_pixels: 640 * 360, max_fps: 0, reason: HintReason::DisplaySize, room: false };
    let r = hinted("size hint", vec![(5 * S, small), (25 * S, Hint::NONE)]);
    // Nothing above 360p while it lasts (from the next keyframe, the hint's, about 0.3 s after it is set).
    let max = r.shown.iter().filter(|s| s.at_us >= 5_300_000 && s.at_us < 25 * S).map(|s| s.height).max();
    assert_eq!(max, Some(360));
    // After the lift: a rung at a time. The path has carried each step's rate with no queue and
    // has shown no limit yet, so the start's 1 s fast hold applies (was 5 s: 720p at 35.9 s).
    let ups: Vec<_> = r.steps.iter().filter(|s| s.at_us >= 25 * S).collect();
    assert!(ups.iter().all(|s| s.to.1 > s.from.1 || s.to.2 > s.from.2), "{ups:?}");
    assert!(ups.windows(2).all(|w| w[1].at_us - w[0].at_us >= S), "{ups:?}");
    let at720 = r.sizes.iter().find(|s| s.0 >= 25 * S && s.2 == 720).expect("720p");
    assert!(at720.0 < 31 * S, "{}", sizes_line(&r));
    // One keyframe for the hint: the cap to 360p at 5 s finds it at 540p since 2.3 s (the fast
    // start; before it was still at 360p).
    assert!(r.key_causes.hint <= 1, "{:?}", r.key_causes);
}

#[test]
fn adapt_fps_hint() {
    // Acceptance 4: the receiver can only show 15 fps from 15 s (at 720p30 by then).
    use crate::media::HintReason;
    let slow = Hint { max_pixels: u32::MAX, max_fps: 15, reason: HintReason::DecodeSlow, room: false };
    let r = hinted("fps hint", vec![(15 * S, slow)]);
    assert_eq!(shown_height(&r, 14 * S, 15 * S), (720, true));
    assert_eq!(fps_at(&r, 14 * S), 30);
    // <= 16 frames a second within 200 ms of the hint (it arrives after the 20 ms one-way delay).
    let n = r.encoded_in(15_200_000, 16_200_000);
    assert!(n <= 16, "{n} frames");
    assert!((14..=16).contains(&r.encoded_in(20 * S, 21 * S)));
    // On the wire no more than the target, no keyframe for it, the size stays.
    let (wire, target) = (r.wire_bps(20 * S, 50 * S), r.mean_bitrate(20 * S, 50 * S));
    assert!(wire <= target * 1.05, "wire {wire:.0} target {target:.0}");
    assert_eq!(r.keyframes, 1 + r.switches);
    assert_eq!(r.key_causes.hint, 0);
    assert_eq!(shown_height(&r, 16 * S, 50 * S), (720, true));
}

#[test]
fn adapt_hw_keyframes() {
    // Step 0 finding: hardware VP8 keyframes are 3-4x a delta and come out a few frames after
    // they were asked for, so every frame in between asked again (2-5 keyframes per switch).
    let sc = Scenario {
        duration_us: 60 * S,
        device: fast(),
        link: Link { rate_bps: 1_800_000, delay_us: 5_000, jitter_us: 5_000, ..Link::default() },
        ..base()
    };
    let mut codec = HwCodec::new(sc.fps, sc.seed);
    let r = run(&sc, &mut codec);
    println!("{:>10}: {}", "hw keys", r.summary(sc.duration_us));
    println!("{:>10}  {} | after 10 s: {}", "", sizes_line(&r), res_switches(&r, 10 * S));
    assert_eq!(r.decode_errors, 0);
    // Baseline: 30 keyframes (20 nobody asked for: asked again while the first was in the
    // encoder), 720p at 7.5 s -> 180p 0.6 s later, 6 switches after 10 s.
    assert_eq!(r.key_causes.other, 0, "one keyframe per switch");
    assert_eq!(r.keyframes, 1 + r.switches);
    let top = r.sizes.iter().position(|s| s.2 == 720).expect("reaches 720p");
    assert_eq!(top, r.sizes.len() - 1, "stays at 720p: {}", sizes_line(&r));
}

/// Latency per second of the bandwidth-dip run (`--ignored`), for looking into a scenario.
#[test]
#[ignore]
fn debug_latency_by_second() {
    let sc = Scenario {
        duration_us: 40 * S,
        link: Link { rate_changes: vec![(10 * S, 200_000), (20 * S, 2_000_000)], ..Link::default() },
        ..base()
    };
    let r = go("ladder dip", &sc);
    for s in 0..40 {
        let l = r.latencies_ms(s * S, (s + 1) * S);
        println!("{s:3}s n {:3} p50 {:5.0} p95 {:5.0}", l.len(), Report::pct(&l, 0.5), Report::pct(&l, 0.95));
    }
}

// --- Home Wi-Fi: short delay spikes on a fast link (docs/video.md, "Wi-Fi delay spikes") ---

/// Pixel 3a <-> laptop on one 2.4 GHz router (real call, 2026-10-16): RTT 7-15 ms, about 0 loss,
/// plenty of capacity, and now and then a second with the RTT at 40-130 ms. Each spike here is
/// 1 s, `extra` ms more one way (both directions, in order, nothing lost).
fn wifi(duration_us: u64, spikes: &[(u64, u64)]) -> Scenario {
    let spikes = spikes.iter().map(|&(at, extra_ms)| (at, at + S, extra_ms * 1_000)).collect();
    Scenario {
        duration_us,
        link: Link {
            rate_bps: 20_000_000,
            delay_us: 4_000,
            jitter_us: 4_000,
            queue_bytes: 512 * 1024,
            spikes,
            ..Link::default()
        },
        ..base()
    }
}

/// Share of the frames shown in `[from, to)` at `min` lines or more.
fn share_at_least(r: &Report, min: u32, from: u64, to: u64) -> f64 {
    let v: Vec<u32> = r.shown.iter().filter(|s| s.at_us >= from && s.at_us < to).map(|s| s.height).collect();
    v.iter().filter(|&&h| h >= min).count() as f64 / v.len().max(1) as f64
}

fn wifi_line(name: &str, r: &Report, from: u64, to: u64) {
    println!(
        "{:>10}  from {} s: mean {:.0} kbps, >= 540p {:.0}% of frames, lowest {}p, p95 {:.0} ms, steps:\n{}",
        name,
        from / S,
        r.mean_bitrate(from, to) / 1000.0,
        100.0 * share_at_least(r, 540, from, to),
        min_height(r, from, to),
        p95(r, from, to),
        r.steps_text()
    );
}

#[test]
fn wifi_delay_spikes() {
    // Spikes like the real call's 43 / 57 / 82 / 127 ms RTT seconds, 20-25 s apart.
    let sc = wifi(90 * S, &[(8 * S, 60), (30 * S, 45), (52 * S, 130), (75 * S, 85)]);
    let r = go("wifi", &sc);
    wifi_line("", &r, 5 * S, sc.duration_us);
    trace(&r, 51_800_000, 54 * S);
    assert_eq!(r.decode_errors, 0);
    // A second of extra delay with nothing lost and the link idle is not a slower link: keep
    // the rate and the picture.
    assert!(r.mean_bitrate(5 * S, sc.duration_us) >= 1_600_000.0, "{:.0} bps", r.mean_bitrate(5 * S, sc.duration_us));
    assert!(share_at_least(&r, 540, 5 * S, sc.duration_us) >= 0.9);
    assert!(min_height(&r, 5 * S, sc.duration_us) >= 360, "{}", sizes_line(&r));
    assert!(r.freezes(250_000, S, sc.duration_us).is_empty());
}

/// The Wi-Fi spikes on an uplink of `rate_bps` (128 KB buffer); latency between the spikes.
fn wifi_capped(name: &str, rate_bps: u64) -> (Report, Vec<f64>) {
    let mut sc = wifi(90 * S, &[(8 * S, 60), (30 * S, 45), (52 * S, 130), (75 * S, 85)]);
    sc.link.rate_bps = rate_bps;
    sc.link.queue_bytes = 128 * 1024;
    let r = go(name, &sc);
    wifi_line("", &r, 5 * S, sc.duration_us);
    trace(&r, 51_900_000, 53_500_000);
    let mut calm: Vec<f64> =
        [(12, 29), (35, 51), (57, 74), (80, 90)].iter().flat_map(|&(a, b)| r.latencies_ms(a * S, b * S)).collect();
    calm.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("{:>10}  between spikes p95 {:.0} ms max {:.0}", "", Report::pct(&calm, 0.95), Report::pct(&calm, 1.0));
    (r, calm)
}

#[test]
fn wifi_delay_spikes_near_capacity() {
    // The same spikes on an uplink only a little above the top rate (the real call's 2 Mbps
    // seconds ran at 20-65 ms RTT, its 500 kbps ones at 7-11 ms: maybe not 20 Mbps): the spikes
    // still cost nothing, and no queue of ours stands between them.
    let (r, calm) = wifi_capped("wifi 2.4M", 2_400_000);
    assert_eq!(r.decode_errors, 0);
    assert!(Report::pct(&calm, 0.95) < 120.0);
    assert!(min_height(&r, 7 * S, 90 * S) >= 540, "{}", sizes_line(&r));
    // Below the top rate the link is full when a spike comes: no burst shows above the old rate,
    // so it is a slower link to the controller (the old rules, down to 180p at 52.9 s). The rate
    // still settles under the link.
    let (r, calm) = wifi_capped("wifi 1.6M", 1_600_000);
    assert_eq!(r.decode_errors, 0);
    // 120 ms (was 102 with capture stamps, at 1205 instead of 1322 kbps).
    assert!(Report::pct(&calm, 0.95) < 130.0, "{}", Report::pct(&calm, 0.95));
    assert!(r.mean_bitrate(10 * S, 90 * S) < 1_600_000.0);
}

#[test]
fn real_av1_wifi_delay_spikes() {
    // The same with real libaom packets decoded by rav1d.
    let sc = wifi(30 * S, &[(8 * S, 85), (20 * S, 130)]);
    let mut codec = Av1Codec::new(640, 360, sc.fps, sc.sender.start_bps);
    let r = run(&sc, &mut codec);
    println!("{:>10}: {}", "av1 wifi", r.summary(sc.duration_us));
    wifi_line("", &r, 5 * S, sc.duration_us);
    trace(&r, 7_900_000, 9_500_000);
    assert_eq!(r.decode_errors, 0);
    assert!(min_height(&r, 5 * S, sc.duration_us) >= 360, "{}", sizes_line(&r));
    assert!(share_at_least(&r, 540, 5 * S, sc.duration_us) >= 0.8);
}

// --- Home LAN with delay bursts (docs/video.md, "Delay bursts on a fast LAN") ---

/// Pixel 3a <-> laptop on one home router (real call, 2026-10-17): plenty of capacity, RTT
/// 5-10 ms with no call but 10-40 ms during it, and every few seconds a burst of 100 ms to ~1 s
/// of extra delay with some loss (router ping over 150 s: 5% lost, 42 of 285 replies > 100 ms,
/// max 1023 ms). One burst lands in the first second of the call. Deterministic per `seed`.
fn lan_bursts(duration_us: u64, rate_bps: u64, seed: u64) -> Scenario {
    let mut rng = Rng::new(seed);
    let mut spikes = Vec::new();
    let mut t = 700_000;
    let mut n = 0;
    while t < duration_us {
        let len = 300_000 + (rng.f64() * 900_000.0) as u64;
        // Mostly 100-300 ms, every fifth up to 600 ms.
        let extra = if n % 5 == 4 {
            300_000 + (rng.f64() * 300_000.0) as u64
        } else {
            100_000 + (rng.f64() * 200_000.0) as u64
        };
        spikes.push((t, t + len, extra));
        let next = t + len + 2 * S + (rng.f64() * 5.0 * S as f64) as u64;
        // In between, stretches of 10-25 ms more one way (the 15-40 ms ping seconds), no loss.
        let mut u = t + len + 300_000;
        while u + 500_000 < next {
            let l = (500_000 + (rng.f64() * 1_500_000.0) as u64).min(next - u - 200_000);
            spikes.push((u, u + l, 10_000 + (rng.f64() * 15_000.0) as u64));
            u += l + 300_000 + (rng.f64() * 1_500_000.0) as u64;
        }
        t = next;
        n += 1;
    }
    Scenario {
        duration_us,
        link: Link {
            rate_bps,
            delay_us: 3_000,
            jitter_us: 6_000,
            queue_bytes: 512 * 1024,
            spikes,
            spike_loss: 0.05,
            ..Link::default()
        },
        ..base()
    }
}

/// Median time from a burst's end (spikes of 50 ms or more) until the target is back at 80% of what it was 0.5 s before
/// the burst (bursts that found it at >= 300 kbps; not back by the next burst: that gap), and
/// how many bursts it came back from at all.
fn burst_recovery(r: &Report, sc: &Scenario) -> (f64, usize, usize) {
    let at = |t: u64| r.bitrate.iter().rev().find(|x| x.0 <= t).map_or(0, |x| x.1);
    let spikes: Vec<_> = sc.link.spikes.iter().filter(|s| s.2 >= 50_000).collect();
    let mut times = Vec::new();
    let mut back = 0;
    for (i, s) in spikes.iter().enumerate() {
        let pre = at(s.0.saturating_sub(500_000));
        if s.0 < 2 * S || pre < 300_000 {
            continue;
        }
        let next = spikes.get(i + 1).map_or(sc.duration_us, |n| n.0);
        let t = r.bitrate.iter().find(|x| x.0 >= s.1 && x.0 < next && x.1 as u64 * 10 >= pre as u64 * 8).map(|x| x.0);
        if t.is_some() {
            back += 1;
        }
        times.push(t.unwrap_or(next) - s.1);
    }
    times.sort_unstable();
    let med = times.get(times.len().saturating_sub(1) / 2).copied().unwrap_or(0);
    (med as f64 / 1e6, back, times.len())
}

fn lan_line(name: &str, r: &Report, sc: &Scenario) {
    let d = sc.duration_us;
    if std::env::var_os("SIM_TRACE").is_some() {
        let b: Vec<String> = sc
            .link
            .spikes
            .iter()
            .filter(|s| s.2 >= 50_000)
            .map(|s| format!("{:.2}-{:.2}s +{}ms", s.0 as f64 / 1e6, s.1 as f64 / 1e6, s.2 / 1000))
            .collect();
        println!("{:>10}  bursts: {}", "", b.join(", "));
    }
    let (rec, back, n) = burst_recovery(r, sc);
    let first540 = r.sizes.iter().find(|s| s.2 >= 540).map_or(f64::NAN, |s| s.0 as f64 / 1e6);
    println!(
        "{:>10}  from 5 s: mean {:.0} kbps, >= 360p {:.0}%, >= 540p {:.0}%, first 540p {:.1} s, under 150 kbps {:.0}% of the time; recovery median {:.1} s ({back}/{n} bursts); p95 {:.0} ms; drops {}, keys {:?}",
        name,
        r.mean_bitrate(5 * S, d) / 1000.0,
        100.0 * share_at_least(r, 360, 5 * S, d),
        100.0 * share_at_least(r, 540, 5 * S, d),
        first540,
        100.0 * r.bitrate.iter().filter(|x| x.0 >= 5 * S && x.1 < 150_000).count() as f64
            / r.bitrate.iter().filter(|x| x.0 >= 5 * S).count().max(1) as f64,
        rec,
        p95(r, 5 * S, d),
        r.sender_drops,
        r.key_causes,
    );
}

#[test]
fn lan_delay_bursts() {
    // Laptop sender (software encoder): the rate gets back to where it was within seconds of a
    // burst instead of creeping up from the floor.
    let sc = lan_bursts(120 * S, 10_000_000, 11);
    let r = go("lan burst", &sc);
    lan_line("", &r, &sc);
    trace(&r, 0, 10 * S);
    println!("{}", r.steps_text());
    assert_eq!(r.decode_errors, 0);
}

#[test]
fn lan_delay_bursts_hw() {
    // Pixel sender: hardware VP8 (output lags 2 frames, keyframes 4x a delta and never under
    // 2.5 KB).
    let sc = lan_bursts(120 * S, 10_000_000, 11);
    let mut codec = HwCodec::new(sc.fps, sc.seed);
    codec.key_min = 2_500;
    let r = run(&sc, &mut codec);
    println!("{:>10}: {}", "lan hw", r.summary(sc.duration_us));
    lan_line("", &r, &sc);
    trace(&r, 0, 10 * S);
    println!("{}", r.steps_text());
    assert_eq!(r.decode_errors, 0);
}

#[test]
fn lan_delay_bursts_slow_link() {
    // The same bursts on a link that really is slow (600 kbps): the rate still settles under it.
    let mut sc = lan_bursts(90 * S, 600_000, 11);
    sc.link.queue_bytes = 64 * 1024;
    let r = go("lan 600k", &sc);
    lan_line("", &r, &sc);
    assert_eq!(r.decode_errors, 0);
    assert!(r.mean_bitrate(10 * S, sc.duration_us) < 550_000.0, "{:.0}", r.mean_bitrate(10 * S, sc.duration_us));
    assert!(r.wire_bps(10 * S, sc.duration_us) < 600_000.0);
}

#[test]
fn lan_delay_bursts_seeds() {
    // The burst pattern and the link's randomness vary a lot from one seed to the next: the
    // same scenario over six seeds, software and hardware encoder, averaged.
    let mut rows = Vec::new();
    for hw in [false, true] {
        for seed in [3, 5, 7, 11, 13, 17] {
            let sc = lan_bursts(120 * S, 10_000_000, seed);
            let r = if hw {
                let mut codec = HwCodec::new(sc.fps, seed);
                codec.key_min = 2_500;
                run(&sc, &mut codec)
            } else {
                run(&sc, &mut FakeCodec::new(sc.fps, seed))
            };
            assert_eq!(r.decode_errors, 0);
            let d = sc.duration_us;
            let low = r.bitrate.iter().filter(|x| x.0 >= 5 * S && x.1 < 150_000).count() as f64
                / r.bitrate.iter().filter(|x| x.0 >= 5 * S).count().max(1) as f64;
            let frozen: u64 = r.freezes(250_000, S, d).iter().map(|f| f.1).sum();
            let row = (
                r.mean_bitrate(5 * S, d) / 1000.0,
                share_at_least(&r, 540, 5 * S, d) * 100.0,
                low * 100.0,
                p95(&r, 5 * S, d),
                frozen as f64 / 1e6,
                r.keyframes as f64,
                burst_recovery(&r, &sc).0,
            );
            println!(
                "{:>10}  {} seed {seed:2}: {:5.0} kbps, >= 540p {:3.0}%, < 150 kbps {:2.0}%, p95 {:4.0} ms, frozen {:4.1} s, keys {:2}, recovery {:.1} s",
                "lan seeds",
                if hw { "hw" } else { "sw" },
                row.0,
                row.1,
                row.2,
                row.3,
                row.4,
                row.5,
                row.6
            );
            rows.push((hw, row));
        }
    }
    for hw in [false, true] {
        let v: Vec<_> = rows.iter().filter(|r| r.0 == hw).map(|r| r.1).collect();
        let n = v.len() as f64;
        let m = |f: fn(&(f64, f64, f64, f64, f64, f64, f64)) -> f64| v.iter().map(f).sum::<f64>() / n;
        println!(
            "{:>10}  {} mean: {:.0} kbps, >= 540p {:.0}%, < 150 kbps {:.1}%, p95 {:.0} ms, frozen {:.1} s, keys {:.1}, recovery {:.1} s",
            "lan seeds",
            if hw { "hw" } else { "sw" },
            m(|r| r.0),
            m(|r| r.1),
            m(|r| r.2),
            m(|r| r.3),
            m(|r| r.4),
            m(|r| r.5),
            m(|r| r.6)
        );
        // Was (b83e596): ~400 kbps, 2% of frames at >= 540p, 18-19% of the time under 150 kbps.
        assert!(m(|r| r.0) > 1_000.0, "mean {:.0} kbps", m(|r| r.0));
        assert!(m(|r| r.1) > 40.0, ">= 540p {:.0}%", m(|r| r.1));
        assert!(m(|r| r.2) < 5.0, "under 150 kbps {:.1}%", m(|r| r.2));
    }
}

// --- Network matrix (docs/video.md, "Delay bursts vs capacity drops") ---
//
// The home LAN is the best case real users will see. Each scenario starts with the call, runs
// 90 s and is averaged over three seeds; `SIM_MATRIX=<name>` runs one.

/// Delay bursts at random: every `gap` (us, lo..hi) a burst `len` long (lo..hi) of `extra` one-way
/// delay (lo..hi), the first at `first`.
fn burst_train(
    rng: &mut Rng,
    first: u64,
    end: u64,
    gap: (u64, u64),
    len: (u64, u64),
    extra: (u64, u64),
) -> Vec<(u64, u64, u64)> {
    let pick = |rng: &mut Rng, (lo, hi): (u64, u64)| lo + (rng.f64() * (hi - lo) as f64) as u64;
    let mut v = Vec::new();
    let mut t = first;
    while t < end {
        let l = pick(rng, len);
        v.push((t, t + l, pick(rng, extra)));
        t += l + pick(rng, gap);
    }
    v
}

/// Bottleneck rate changing every `every` (lo..hi) to a uniform pick in `rate` (lo..hi).
fn rate_walk(rng: &mut Rng, end: u64, every: (u64, u64), rate: (u64, u64)) -> Vec<(u64, u64)> {
    let pick = |rng: &mut Rng, (lo, hi): (u64, u64)| lo + (rng.f64() * (hi - lo) as f64) as u64;
    let mut v = Vec::new();
    let mut t = pick(rng, every);
    while t < end {
        v.push((t, pick(rng, rate)));
        t += pick(rng, every);
    }
    v
}

const MATRIX_US: u64 = 90 * S;

fn matrix_scenario(name: &str, seed: u64) -> Scenario {
    let mut rng = Rng::new(seed ^ 0x6d61);
    let d = MATRIX_US;
    let ms = 1_000;
    let link = match name {
        // 1. Home LAN (the best case): see `lan_bursts`.
        "lan" => return lan_bursts(d, 10_000_000, seed),
        // 2. Busy / cafe Wi-Fi: 2-6 Mbps, changing every 3-8 s; a 100-1000 ms burst every
        //    3-10 s; 2% loss, 5% in the bursts.
        "cafe" => Link {
            rate_bps: 4_000_000,
            delay_us: 8 * ms,
            jitter_us: 10 * ms,
            loss: 0.02,
            queue_bytes: 256 * 1024,
            rate_changes: rate_walk(&mut rng, d, (3 * S, 8 * S), (2_000_000, 6_000_000)),
            spikes: burst_train(&mut rng, 800 * ms, d, (3 * S, 10 * S), (200 * ms, 1_000 * ms), (100 * ms, 1_000 * ms)),
            spike_loss: 0.05,
            ..Link::default()
        },
        // 3. 4G/LTE: RTT 60-80 ms, 1-8 Mbps changing every 2-5 s, a 384 KB buffer (0.4 s at
        //    8 Mbps, 3 s at 1 Mbps), a 1-1.7 s handover stall every 15-30 s.
        "lte" => Link {
            rate_bps: 4_000_000,
            delay_us: 30 * ms,
            jitter_us: 10 * ms,
            loss: 0.005,
            queue_bytes: 384 * 1024,
            rate_changes: rate_walk(&mut rng, d, (2 * S, 5 * S), (1_000_000, 8_000_000)),
            spikes: burst_train(&mut rng, 6 * S, d, (15 * S, 30 * S), (200 * ms, 400 * ms), (1_000 * ms, 1_700 * ms)),
            spike_loss: 0.05,
            ..Link::default()
        },
        // 4. Relayed (India <-> EU): RTT ~250 ms +-40, 3 Mbps, a 100-600 ms burst every 3-8 s.
        "relay" => Link {
            rate_bps: 3_000_000,
            delay_us: 120 * ms,
            jitter_us: 20 * ms,
            loss: 0.005,
            queue_bytes: 256 * 1024,
            spikes: burst_train(&mut rng, 900 * ms, d, (3 * S, 8 * S), (300 * ms, 1_000 * ms), (100 * ms, 600 * ms)),
            spike_loss: 0.03,
            ..Link::default()
        },
        // 5. Weak link: a steady 650 kbps, 20 ms +-15 ms, 0.5% loss.
        "weak" => Link {
            rate_bps: 650_000,
            delay_us: 20 * ms,
            jitter_us: 15 * ms,
            loss: 0.005,
            queue_bytes: 64 * 1024,
            ..Link::default()
        },
        // 6. A real step down: 5 Mbps, then 600 kbps from 15 s on.
        "step" => Link {
            rate_bps: 5_000_000,
            delay_us: 15 * ms,
            jitter_us: 5 * ms,
            queue_bytes: 128 * 1024,
            rate_changes: vec![(15 * S, 600_000)],
            ..Link::default()
        },
        // 6b. The same drop arriving with a stall (a handover to a worse cell): the stall must
        //     not make the old rate look like the path's.
        "step+stall" => Link {
            rate_bps: 5_000_000,
            delay_us: 15 * ms,
            jitter_us: 5 * ms,
            queue_bytes: 128 * 1024,
            rate_changes: vec![(15 * S, 600_000)],
            spikes: vec![(15 * S, 15 * S + 300 * ms, 800 * ms)],
            spike_loss: 0.05,
            ..Link::default()
        },
        _ => panic!("{name}"),
    };
    Scenario { duration_us: d, link, seed, ..base() }
}

/// (mean target kbps, % frames >= 540p, % >= 360p, p50 ms, p95 ms, freezes, frozen s) over
/// `[from, to)`.
fn matrix_row(r: &Report, from: u64, to: u64) -> [f64; 7] {
    let l = r.latencies_ms(from, to);
    let f = r.freezes(250_000, from.max(S), to);
    [
        r.mean_bitrate(from, to) / 1000.0,
        100.0 * share_at_least(r, 540, from, to),
        100.0 * share_at_least(r, 360, from, to),
        Report::pct(&l, 0.5),
        Report::pct(&l, 0.95),
        f.len() as f64,
        f.iter().map(|x| x.1).sum::<u64>() as f64 / 1e6,
    ]
}

fn matrix_line(name: &str, v: &[f64; 7]) -> String {
    format!(
        "{name:>12}: {:5.0} kbps, >= 540p {:3.0}%, >= 360p {:3.0}%, p50 {:4.0} ms, p95 {:5.0} ms, freezes {:4.1} ({:4.1} s)",
        v[0], v[1], v[2], v[3], v[4], v[5], v[6]
    )
}

/// Runs scenario `name` over three seeds; returns the mean row over the call and, for "step",
/// over 20-90 s (after the drop) as well.
fn matrix_run(name: &str) -> ([f64; 7], [f64; 7]) {
    // `SIM_MATRIX_SEEDS=<n>`: seeds 1..=n instead (a 90 s run with bursts is chaotic: one
    // different early decision changes the rest, so three seeds are noisy).
    let n = std::env::var("SIM_MATRIX_SEEDS").ok().and_then(|v| v.parse::<u64>().ok());
    let seeds: Vec<u64> = n.map_or(vec![3, 7, 11], |n| (1..=n).collect());
    let (mut all, mut after) = ([0.0; 7], [0.0; 7]);
    for &seed in &seeds {
        let sc = matrix_scenario(name, seed);
        let r = run(&sc, &mut FakeCodec::new(sc.fps, seed));
        assert_eq!(r.decode_errors, 0, "{name} seed {seed}");
        let a = matrix_row(&r, 0, sc.duration_us);
        let b = matrix_row(&r, 20 * S, sc.duration_us);
        if std::env::var_os("SIM_MATRIX").is_some() {
            println!("{}", matrix_line(&format!("{name}/{seed}"), &a));
            println!("{:>12}  {}", "", sizes_line(&r));
        }
        if std::env::var("SIM_MATRIX_SEED").is_ok_and(|v| v == seed.to_string()) {
            let b: Vec<String> = sc
                .link
                .spikes
                .iter()
                .map(|s| format!("{:.2}-{:.2}s +{}ms", s.0 as f64 / 1e6, s.1 as f64 / 1e6, s.2 / 1000))
                .collect();
            println!("bursts: {}", b.join(", "));
            println!("rate changes: {:?}", sc.link.rate_changes);
            // The seconds with slow frames (p95 over 200 ms).
            for t in 0..sc.duration_us / S {
                let l = r.latencies_ms(t * S, (t + 1) * S);
                if Report::pct(&l, 0.95) > 200.0 {
                    let h = r.shown.iter().rev().find(|x| x.at_us < (t + 1) * S).map_or(0, |x| x.height);
                    let br = r.mean_bitrate(t * S, (t + 1) * S) / 1000.0;
                    println!("  {t:3} s: p95 {:5.0} ms, {} frames, {h}p, {br:.0} kbps", Report::pct(&l, 0.95), l.len());
                }
            }
            trace(&r, 0, 10 * S);
        }
        for i in 0..7 {
            all[i] += a[i] / seeds.len() as f64;
            after[i] += b[i] / seeds.len() as f64;
        }
    }
    println!("{}", matrix_line(name, &all));
    if name.starts_with("step") {
        println!("{}", matrix_line(&format!("{name} 20-90s"), &after));
    }
    (all, after)
}

#[test]
fn network_matrix() {
    let only = std::env::var("SIM_MATRIX").ok();
    for name in ["lan", "cafe", "lte", "relay", "weak", "step", "step+stall"] {
        if only.as_deref().is_some_and(|o| !o.is_empty() && o != "1" && o != name) {
            continue;
        }
        let (all, after) = matrix_run(name);
        // [kbps, % >= 540p, % >= 360p, p50, p95, freezes, frozen s]. `SIM_MATRIX_NOCHECK=1`
        // prints without checking (for the numbers of an older controller).
        if std::env::var_os("SIM_MATRIX_NOCHECK").is_some() {
            continue;
        }
        match name {
            // Was (b83e596): 411 / 478 / 877 kbps.
            "lan" | "cafe" | "lte" => assert!(all[0] > 1_000.0, "{name}: {all:?}"),
            // The trade-off (docs/video.md): a little more rate, more delay. Was 374 kbps, p95 506.
            "relay" => assert!(all[4] < 800.0, "{all:?}"),
            "weak" => assert!(all[0] < 650.0 && all[4] < 200.0 && all[5] == 0.0, "{all:?}"),
            // Down fast and stays down: under the 600 kbps link, no queue, no freezes.
            "step" | "step+stall" => assert!(after[0] < 600.0 && after[4] < 150.0 && after[5] == 0.0, "{after:?}"),
            _ => {}
        }
    }
}

/// The 2026-10-17 laptop -> phone call: a clean home LAN, a software encoder on a laptop that is
/// busy now and then (every 2-7 s, for 0.5-2 s, each frame 20-60 ms late, so fewer of them).
/// Stamped at capture, that lateness read as a queue: an overuse every ~5 s, 180p for 75 of 80 s.
fn busy_sender(seed: u64, stamp_capture: bool) -> Scenario {
    let mut rng = Rng::new(seed ^ 0xb5);
    let busy = burst_train(&mut rng, 3 * S, MATRIX_US, (2 * S, 7 * S), (500_000, 2 * S), (40_000, 60_000));
    let link = Link { rate_bps: 10_000_000, delay_us: 3_000, jitter_us: 3_000, queue_bytes: 256 * 1024, ..Link::default() };
    Scenario {
        duration_us: MATRIX_US,
        link,
        device: Device { busy, ..Device::busy_desktop() },
        stamp_capture,
        seed,
        ..base()
    }
}

#[test]
fn busy_sender_is_not_a_queue() {
    let seeds = [3, 7, 11];
    let mut rows = [[0.0; 7]; 2];
    for (i, capture) in [true, false].into_iter().enumerate() {
        for seed in seeds {
            let sc = busy_sender(seed, capture);
            let r = run(&sc, &mut FakeCodec::new(sc.fps, seed));
            assert_eq!(r.decode_errors, 0);
            let row = matrix_row(&r, 0, sc.duration_us);
            if std::env::var_os("SIM_MATRIX").is_some() {
                println!("{}  overuse {} cam drops {}", matrix_line(&format!("busy/{seed}"), &row), r.reports.iter().filter(|x| x.1.queuing_us > 40_000).count(), r.camera_drops);
                println!("{:>12}  {}", "", sizes_line(&r));
            }
            for k in 0..7 {
                rows[i][k] += row[k] / seeds.len() as f64;
            }
        }
    }
    println!("{}", matrix_line("busy, capture stamp", &rows[0]));
    println!("{}", matrix_line("busy, send stamp", &rows[1]));
    // The link carries 10 Mbps: the busy machine costs frames, not the rate.
    assert!(rows[1][0] > 1_200.0 && rows[1][1] > 50.0, "{:?}", rows[1]);
    assert!(rows[1][0] > rows[0][0] * 1.3, "{:?} vs {:?}", rows[1], rows[0]);
}

// --- Call start on a good link (docs/video.md, "Best case first: the call start") ---

/// A clean home LAN: 10 Mbps, `rtt_ms` round trip (+-20%), 256 KB buffer; with `bursts`, a
/// 200-400 ms window of +300 ms delay every 4-8 s (the first 1.5-3 s in).
fn clean_start_scenario(rtt_ms: u64, bursts: bool, seed: u64) -> Scenario {
    let mut rng = Rng::new(seed ^ 0xc1ea);
    let one_way = rtt_ms * 1_000 / 2;
    let spikes = if bursts {
        let first = 1_500_000 + (rng.f64() * 1_500_000.0) as u64;
        burst_train(&mut rng, first, 30 * S, (4 * S, 8 * S), (200_000, 400_000), (300_000, 300_001))
    } else {
        vec![]
    };
    let link = Link {
        rate_bps: 10_000_000,
        delay_us: one_way * 9 / 10,
        jitter_us: one_way / 5,
        queue_bytes: 256 * 1024,
        spikes,
        spike_loss: 0.05,
        ..Link::default()
    };
    Scenario { duration_us: 30 * S, link, device: fast(), seed, ..base() }
}

/// First time the encoder is at `h` lines or more (NaN: never).
fn first_at(r: &Report, h: u32) -> f64 {
    r.sizes.iter().find(|s| s.2 >= h).map_or(f64::NAN, |s| s.0 as f64 / 1e6)
}

/// (s to 540p, s to 720p, p95 ms and freezes over the first 10 s) per run; software and
/// hardware encoder (laptop and phone senders), three seeds.
fn clean_start_rows(rtt_ms: u64, bursts: bool) -> Vec<(String, [f64; 4])> {
    let mut rows = Vec::new();
    for hw in [false, true] {
        for seed in [3, 7, 11] {
            let sc = clean_start_scenario(rtt_ms, bursts, seed);
            let r = if hw {
                let mut c = HwCodec::new(sc.fps, seed);
                c.key_min = 2_500;
                run(&sc, &mut c)
            } else {
                run(&sc, &mut FakeCodec::new(sc.fps, seed))
            };
            assert_eq!(r.decode_errors, 0);
            let name = format!("{}ms{} {}/{seed}", rtt_ms, if bursts { "+b" } else { "" }, if hw { "hw" } else { "sw" });
            let row = [first_at(&r, 540), first_at(&r, 720), p95(&r, 0, 10 * S), r.freezes(250_000, S, 10 * S).len() as f64];
            println!(
                "{name:>16}: 540p at {:4.1} s, 720p at {:4.1} s, first 10 s p95 {:4.0} ms, freezes {} | {}",
                row[0], row[1], row[2], row[3], sizes_line(&r)
            );
            if std::env::var_os("SIM_STEPS").is_some() {
                println!("{}", r.steps_text());
            }
            trace(&r, 0, 8 * S);
            rows.push((name, row));
        }
    }
    rows
}

#[test]
fn clean_start() {
    let mut all = Vec::new();
    for (rtt, bursts) in [(5, false), (40, false), (10, true)] {
        all.extend(clean_start_rows(rtt, bursts));
    }
    let worst = |i: usize, f: &dyn Fn(&str) -> bool| {
        all.iter().filter(|r| f(&r.0)).map(|r| r.1[i]).fold(0.0f64, |a, b| if b.is_nan() { f64::INFINITY } else { a.max(b) })
    };
    let clean = |n: &str| !n.contains("+b");
    let bursts = |n: &str| n.contains("+b");
    println!(
        "clean start: worst 540p {:.1} s, 720p {:.1} s (clean); with bursts {:.1} / {:.1} s",
        worst(0, &clean),
        worst(1, &clean),
        worst(0, &bursts),
        worst(1, &bursts),
    );

    // Links just above and below the rungs' thresholds (780 kbps for 540p, 1.3 Mbps for 720p):
    // the fast hold needs the wire rate itself, so these start as before, without freezes.
    for rate in [650_000, 1_000_000, 1_500_000] {
        for seed in [3, 7, 11] {
            let mut sc = clean_start_scenario(20, false, seed);
            sc.link.rate_bps = rate;
            sc.link.queue_bytes = 64 * 1024;
            let r = run(&sc, &mut FakeCodec::new(sc.fps, seed));
            let f = r.freezes(250_000, S, 10 * S);
            println!(
                "{:>16}: first 10 s p95 {:4.0} ms max {:4.0}, freezes {}, mean {:4.0} kbps | {}",
                format!("{}k/{seed}", rate / 1000),
                p95(&r, 0, 10 * S),
                Report::pct(&r.latencies_ms(0, 10 * S), 1.0),
                f.len(),
                r.mean_bitrate(0, 10 * S) / 1000.0,
                sizes_line(&r)
            );
            if std::env::var_os("SIM_STEPS").is_some() {
                println!("{}", r.steps_text());
            }
            assert!(f.is_empty(), "{rate} seed {seed}: {f:?}");
        }
    }
    // Was 6.1 / 11.9 s (two 5 s holds, each after ~1 s of ramp and encoder warm-up).
    assert!(worst(0, &clean) <= 3.0 && worst(1, &clean) <= 5.0, "clean start too slow");
    assert!(worst(1, &bursts) <= 8.0, "start with bursts too slow");
    assert!(worst(3, &clean) == 0.0 && worst(2, &clean) < 130.0, "clean start costs latency");
}
