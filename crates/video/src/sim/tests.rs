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
    assert_eq!(r.keyframes, 1, "no reason to send a second keyframe");
    assert!(r.shown.len() as u64 >= r.captured - 3, "every frame shown");
    assert!(p95(&r, 2 * S, sc.duration_us) < 100.0, "p95 {}", p95(&r, 2 * S, sc.duration_us));
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
    assert!(p95(&r, 13 * S, 30 * S) < 100.0, "back to low latency, p95 {}", p95(&r, 13 * S, 30 * S));
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
    let mut codec = Av1Codec::new(320, 240, sc.fps, sc.sender.start_bps);
    let r = run(&sc, &mut codec);
    println!("{:>10}: {}", "av1", r.summary(sc.duration_us));
    assert_eq!(r.decode_errors, 0);
    assert!(r.shown.len() as u64 > r.captured * 8 / 10);
}
