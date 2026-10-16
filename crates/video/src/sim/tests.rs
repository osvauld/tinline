//! Call scenarios. Targets are what a good 1:1 video call should feel like; each test prints
//! its report (`cargo test -p video sim -- --nocapture`).

use super::*;

const S: u64 = 1_000_000;

/// Defaults. Overrides for experiments: `SIM_BACKLOG=unsent` (QUIC-unsent backlog model),
/// `SIM_DRAIN=<pct>`, `SIM_STARVE=<pct>` (100 / 0 turn them off).
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
fn trace(r: &Report, from: u64, to: u64) {
    if std::env::var_os("SIM_TRACE").is_none() {
        return;
    }
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
    assert!(p95(&r, 2 * S, sc.duration_us) < 120.0, "p95 {}", p95(&r, 2 * S, sc.duration_us));
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
    assert!(Report::pct(&r.latencies_ms(11_500_000, 30 * S), 1.0) < 165.0);
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
    assert!(Report::pct(&r.latencies_ms(11_500_000, 30 * S), 1.0) < 165.0);
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
    assert!(p95(&r, 13 * S, 30 * S) < 120.0, "back to low latency, p95 {}", p95(&r, 13 * S, 30 * S));
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
    assert_eq!(shown_height(&r, 8 * S, 10 * S), (540, true), "uses the 2 Mbps link");
    // Down to 320x180 within a second of the drop, and stays there while it lasts.
    let down = r.sizes.iter().find(|s| s.0 >= 10 * S && s.2 == 180).expect("steps down");
    assert!(down.0 < 11 * S, "180p at {} ms", down.0 / 1000);
    assert_eq!(shown_height(&r, 12 * S, 20 * S), (180, true));
    // Back up after recovery, one rung at a time (10 s after the step down, 5 s holds), without
    // flapping: 360p at ~26 s, 540p at ~32 s, then (the phone model) 720p24 at ~38 s. The drop
    // starved the link, so it isn't a flap: no block.
    assert!(min_height(&r, 33 * S, 40 * S) >= 540, "{:?}", r.sizes);
    assert!(r.switches <= 5, "{} switches: {:?}", r.switches, r.sizes);
    assert!(p95(&r, 21 * S, 40 * S) < 120.0, "p95 {}", p95(&r, 21 * S, 40 * S));
    // The queue built in the ~150 ms before the drop shows drains within about a second.
    assert!(p95(&r, 0, 40 * S) < 150.0, "p95 {}", p95(&r, 0, 40 * S));
    // (Up to 37 s: the 720p24 step near the link's capacity costs ~190 ms for a second.)
    assert!(Report::pct(&r.latencies_ms(11_500_000, 37 * S), 1.0) < 150.0);
    let f: u64 = r.freezes(250_000, S, 40 * S).iter().map(|f| f.1).sum();
    assert!(f < 500_000, "frozen {} ms", f / 1000);
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
    Device { base_us: 1_500, ns_per_px: 8, jitter: 0.2, slowdowns: vec![], spikes: (0.0, 0) }
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
    // Baseline: p95 116 ms, no freezes.
    assert!(p95(&r, 0, sc.duration_us) <= 116.0, "p95 {}", p95(&r, 0, sc.duration_us));
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
    assert!(p95(&r, 0, sc.duration_us) <= 125.0, "p95 {}", p95(&r, 0, sc.duration_us));
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
    let small = Hint { max_pixels: 640 * 360, max_fps: 0, reason: HintReason::DisplaySize };
    let r = hinted("size hint", vec![(5 * S, small), (25 * S, Hint::NONE)]);
    // Nothing above 360p while it lasts (from the next keyframe: here it was 360p already).
    let max = r.shown.iter().filter(|s| s.at_us >= 5_300_000 && s.at_us < 25 * S).map(|s| s.height).max();
    assert_eq!(max, Some(360));
    // After the lift: a rung at a time with the 5 s hold, 720p by 45 s.
    let ups: Vec<_> = r.steps.iter().filter(|s| s.at_us >= 25 * S).collect();
    assert!(ups.iter().all(|s| s.to.1 > s.from.1 || s.to.2 > s.from.2), "{ups:?}");
    assert!(ups.windows(2).all(|w| w[1].at_us - w[0].at_us >= 5 * S), "{ups:?}");
    let at720 = r.sizes.iter().find(|s| s.0 >= 25 * S && s.2 == 720).expect("720p");
    assert!(at720.0 < 45 * S, "{}", sizes_line(&r));
    assert_eq!(r.key_causes.hint, 0);
}

#[test]
fn adapt_fps_hint() {
    // Acceptance 4: the receiver can only show 15 fps from 15 s (at 720p30 by then).
    use crate::media::HintReason;
    let slow = Hint { max_pixels: u32::MAX, max_fps: 15, reason: HintReason::DecodeSlow };
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
