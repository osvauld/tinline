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
    r
}

/// With `SIM_TRACE=1`: every 100 ms of `[from, to)`, the encoder bitrate, bottleneck queue,
/// last report and the latency of frames shown.
fn trace(r: &Report, from: u64, to: u64) {
    if std::env::var_os("SIM_TRACE").is_none() {
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
    assert!(r.switches <= 1, "{:?}", r.sizes);
    assert!(r.shown.len() as u64 >= r.captured - 3, "every frame shown");
    // 960x540 takes ~11 ms to encode on the phone model.
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
    assert_eq!(r.decode_errors, 0);
    // Adapts within a few seconds and stays interactive at the lower rate.
    assert!(r.mean_bitrate(15 * S, 30 * S) < 300_000.0, "bitrate {}", r.mean_bitrate(15 * S, 30 * S));
    assert!(p95(&r, 15 * S, 30 * S) < 120.0, "p95 {}", p95(&r, 15 * S, 30 * S));
    // The queue built before the drop is noticed drains within about a second.
    assert!(p95(&r, 0, 30 * S) < 200.0, "p95 {}", p95(&r, 0, 30 * S));
    assert!(Report::pct(&r.latencies_ms(11_500_000, 30 * S), 1.0) < 150.0);
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
    assert!(Report::pct(&r.latencies_ms(11_500_000, 30 * S), 1.0) < 150.0);
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
    assert!(r.shown.len() as u64 > r.captured * 8 / 10);
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
    // Back up after recovery, one rung at a time, without flapping.
    assert_eq!(shown_height(&r, 35 * S, 40 * S), (540, true));
    assert!(r.switches <= 4, "{} switches: {:?}", r.switches, r.sizes);
    assert!(p95(&r, 21 * S, 40 * S) < 120.0, "p95 {}", p95(&r, 21 * S, 40 * S));
    // The queue built in the ~150 ms before the drop shows drains within about a second.
    assert!(p95(&r, 0, 40 * S) < 150.0, "p95 {}", p95(&r, 0, 40 * S));
    assert!(Report::pct(&r.latencies_ms(11_500_000, 40 * S), 1.0) < 150.0);
    let f: u64 = r.freezes(250_000, S, 40 * S).iter().map(|f| f.1).sum();
    assert!(f < 500_000, "frozen {} ms", f / 1000);
}

#[test]
fn ladder_slow_device_stays_low() {
    // Codec on the little cores: 640x360 takes ~12 ms, 960x540 would take ~21 ms (budget 20).
    let sc = Scenario { device: Device::phone_unpinned(), ..base() };
    let r = go("slow dev", &sc);
    println!("{:>10}  sizes {:?}", "", r.sizes);
    assert_eq!(r.decode_errors, 0);
    assert_eq!(shown_height(&r, 2 * S, sc.duration_us), (360, true));
    assert_eq!(r.switches, 0);
    assert_eq!(r.camera_drops, 0);
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
    assert_eq!(shown_height(&r, 12 * S, 15 * S), (540, true));
    let down = r.sizes.iter().find(|s| s.0 >= 15 * S).expect("steps down");
    assert!(down.0 < 16 * S && down.2 == 360, "{down:?}");
    assert_eq!(shown_height(&r, 16 * S, sc.duration_us), (360, true));
    assert!(r.switches <= 2, "{:?}", r.sizes);
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
