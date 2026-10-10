//! Call scenarios. Targets are what a good 1:1 video call should feel like; each test prints
//! its report (`cargo test -p video sim -- --nocapture`).

use super::*;

const S: u64 = 1_000_000;

fn go(name: &str, sc: &Scenario) -> Report {
    let mut codec = FakeCodec::new(sc.fps, sc.seed);
    let r = run(sc, &mut codec);
    println!("{name:>10}: {}", r.summary(sc.duration_us));
    r
}

fn p95(r: &Report, from: u64, to: u64) -> f64 {
    Report::pct(&r.latencies_ms(from, to), 0.95)
}

#[test]
fn clean_link() {
    let sc = Scenario::default();
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
    let sc = Scenario { link: Link { loss: 0.03, ..Link::default() }, ..Scenario::default() };
    let r = go("loss 3%", &sc);
    assert_eq!(r.decode_errors, 0);
    assert!(p95(&r, 2 * S, sc.duration_us) < 200.0, "p95 {}", p95(&r, 2 * S, sc.duration_us));
    assert!(r.freezes(250_000, S, sc.duration_us).len() <= 2);
}

#[test]
fn bandwidth_drop() {
    let sc = Scenario {
        link: Link { rate_changes: vec![(10 * S, 300_000)], ..Link::default() },
        ..Scenario::default()
    };
    let r = go("2M->300k", &sc);
    assert_eq!(r.decode_errors, 0);
    // Adapts within a few seconds and stays interactive at the lower rate.
    assert!(r.mean_bitrate(15 * S, 30 * S) < 300_000.0, "bitrate {}", r.mean_bitrate(15 * S, 30 * S));
    assert!(p95(&r, 15 * S, 30 * S) < 300.0, "p95 {}", p95(&r, 15 * S, 30 * S));
    let f: u64 = r.freezes(250_000, 10 * S, 30 * S).iter().map(|f| f.1).sum();
    assert!(f < 2 * S, "frozen {} ms after the drop", f / 1000);
}

#[test]
fn outage_500ms() {
    let sc = Scenario {
        link: Link { outages: vec![(10 * S, 10 * S + 500_000)], ..Link::default() },
        ..Scenario::default()
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
        ..Scenario::default()
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
        ..Scenario::default()
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
        ..Scenario::default()
    };
    let r = go("ladder dip", &sc);
    println!("{:>10}  sizes {:?}", "", r.sizes);
    assert_eq!(r.decode_errors, 0);
    assert_eq!(shown_height(&r, 8 * S, 10 * S), (540, true), "uses the 2 Mbps link");
    // Down to 320x180 within a second of the drop, and stays there while it lasts.
    let down = r.sizes.iter().find(|s| s.0 >= 10 * S && s.2 == 180).expect("steps down");
    assert!(down.0 < 11 * S, "180p at {} ms", down.0 / 1000);
    // What was already in the bottleneck queue (64 KB: 2.6 s at 200 kbps) still drains first.
    assert_eq!(shown_height(&r, 14 * S, 20 * S), (180, true));
    // Back up after recovery, one rung at a time, without flapping.
    assert_eq!(shown_height(&r, 35 * S, 40 * S), (540, true));
    assert!(r.switches <= 4, "{} switches: {:?}", r.switches, r.sizes);
    assert!(p95(&r, 21 * S, 40 * S) < 120.0, "p95 {}", p95(&r, 21 * S, 40 * S));
    let f: u64 = r.freezes(250_000, S, 40 * S).iter().map(|f| f.1).sum();
    assert!(f < 2 * S, "frozen {} ms", f / 1000);
}

#[test]
fn ladder_slow_device_stays_low() {
    // Codec on the little cores: 640x360 takes ~12 ms, 960x540 would take ~21 ms (budget 20).
    let sc = Scenario { device: Device::phone_unpinned(), ..Scenario::default() };
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
        ..Scenario::default()
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
        ..Scenario::default()
    };
    let mut codec = Av1Codec::new(640, 360, sc.fps, sc.sender.start_bps);
    let r = run(&sc, &mut codec);
    println!("{:>10}: {}", "av1 switch", r.summary(sc.duration_us));
    println!("{:>10}  decoded {:?}", "", codec.decoded_sizes);
    assert_eq!(r.decode_errors, 0);
    let small = codec.decoded_sizes.iter().position(|&s| s == (320, 180)).expect("decoded 180p");
    assert!(codec.decoded_sizes[small + 1..].contains(&(640, 360)), "{:?}", codec.decoded_sizes);
    let f: u64 = r.freezes(250_000, S, sc.duration_us).iter().map(|f| f.1).sum();
    // Mostly the 180p keyframe waiting behind 2.6 s of queued 540p data (see docs/video.md).
    assert!(f < 4 * S, "frozen {} ms", f / 1000);
}
