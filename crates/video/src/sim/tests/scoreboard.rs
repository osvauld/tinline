//! Scoreboard: every scenario under every controller variant, one table.
//!
//! Run it explicitly (it is a long report, so it is `#[ignore]`d):
//! `cargo test --locked -j 6 -p video --release --lib scoreboard -- --ignored --nocapture --test-threads=1`
//!
//! Env:
//! - `SIM_VARIANTS=a,b`: only these variants (default: all in `variants()`).
//! - `SIM_ONLY=<text>`: only scenarios whose name contains the text.
//! - `SIM_SEEDS=<n>`: seeds 1..=n instead of 3, 7, 11 (a 90 s run is chaotic; more seeds, less
//!   noise); `SIM_SEEDS=4,` or `=2,5`: just those.
//! - `SIM_SECONDS=1`: each run's ladder steps and per-second lines (see `real_calls`).
//! - `SIM_SCOREBOARD_OUT=<path>`: also write the data lines to that file. The same lines are always
//!   printed between `--- scoreboard data ---` and `--- end ---`, because a remote worker's file
//!   is not on this machine.
//! - `SIM_SCOREBOARD_BASE=<path>`: a file written that way; each row is followed by a line of
//!   deltas (this run minus the baseline). Rows are matched by scenario and variant.
//!
//! Data format (no serde in the dev-dependencies, so no JSON): one line per row,
//! `scenario<TAB>variant<TAB>` and then the nine numbers below, space separated.
//!
//! Columns (each the mean over seeds, measured from the scenario's start time: 10 s, or 40 s
//! for the mobile ones, whose picture is hidden until 30 s):
//! - kbps: wire rate, all bytes sent, headers and repairs included.
//! - 540p, 360p: % of shown frames at least that tall.
//! - p50, p95: glass-to-glass latency of shown frames, ms.
//! - frz: freezes (gaps between shown frames over 250 ms); frz s: their total seconds.
//! - keys: keyframes sent. steps: ladder steps taken.

use super::*;
use real_calls::{Mobile, moto_relay, moto_to_mobile, seconds, slow_relay};

/// One controller variant: how it changes a scenario. Adding one is one line in `variants()`.
struct Variant {
    name: &'static str,
    apply: fn(&mut Scenario),
}

fn variants() -> Vec<Variant> {
    vec![
        Variant { name: "fixed", apply: |_| {} },
        // Frame delay read at the last byte, as before 2026-10-20 (`low_fps_delay`).
        Variant { name: "lastbyte", apply: |sc| sc.receiver.low_fps_delay = false },
        Variant { name: "trend", apply: |sc| sc.sender.detector = crate::detect::Detector::Trend },
        Variant { name: "fixed+cut", apply: |sc| sc.sender.smooth_cut = true },
        Variant {
            name: "trend+cut",
            apply: |sc| {
                sc.sender.detector = crate::detect::Detector::Trend;
                sc.sender.smooth_cut = true;
            },
        },
        Variant {
            name: "fixed+c+b",
            apply: |sc| {
                sc.sender.smooth_cut = true;
                sc.sender.burst_room = true;
                sc.receiver.report_burst = true;
            },
        },
        Variant {
            name: "trend+c+b",
            apply: |sc| {
                sc.sender.detector = crate::detect::Detector::Trend;
                sc.sender.smooth_cut = true;
                sc.sender.burst_room = true;
                sc.receiver.report_burst = true;
            },
        },
    ]
}

#[derive(Clone, Copy)]
enum Codec {
    Fake,
    Pixel,
}

struct Entry {
    name: &'static str,
    build: fn(u64) -> Scenario,
    codec: Codec,
    /// Where the measured window starts.
    from: u64,
}

fn entries() -> Vec<Entry> {
    let m = |name: &'static str, build: fn(u64) -> Scenario| Entry { name, build, codec: Codec::Fake, from: 10 * S };
    // `moto_to_mobile` shows the picture from 30 s (`LIFT` there is private); wait 10 s more.
    let mobile =
        |name: &'static str, build: fn(u64) -> Scenario| Entry { name, build, codec: Codec::Fake, from: 40 * S };
    vec![
        m("lan", |s| matrix_scenario("lan", s)),
        m("cafe", |s| matrix_scenario("cafe", s)),
        m("lte", |s| matrix_scenario("lte", s)),
        m("relay", |s| matrix_scenario("relay", s)),
        m("weak", |s| matrix_scenario("weak", s)),
        m("step", |s| matrix_scenario("step", s)),
        m("step+stall", |s| matrix_scenario("step+stall", s)),
        Entry { name: "pixel", build: |s| pixel_to_moto(s, false), codec: Codec::Pixel, from: 10 * S },
        Entry { name: "pixel busy", build: |s| pixel_to_moto(s, true), codec: Codec::Pixel, from: 10 * S },
        m("busy_sender", |s| busy_sender(s, false)),
        mobile("mobile spikes", |s| moto_to_mobile(s, Mobile::Spikes)),
        mobile("mobile walk", |s| moto_to_mobile(s, Mobile::Walk)),
        mobile("moto relay", moto_relay),
        Entry { name: "slow relay", build: slow_relay, codec: Codec::Pixel, from: 10 * S },
    ]
}

const COLS: [&str; 9] = ["kbps", "540p", "360p", "p50", "p95", "frz", "frz s", "keys", "steps"];

fn measure(r: &Report, from: u64, to: u64) -> [f64; 9] {
    let l = r.latencies_ms(from, to);
    let f = r.freezes(250_000, from, to);
    [
        r.wire_bps(from, to) / 1000.0,
        100.0 * share_at_least(r, 540, from, to),
        100.0 * share_at_least(r, 360, from, to),
        Report::pct(&l, 0.5),
        Report::pct(&l, 0.95),
        f.len() as f64,
        f.iter().map(|x| x.1).sum::<u64>() as f64 / 1e6,
        r.keyframes as f64,
        r.steps.len() as f64,
    ]
}

fn run_row(e: &Entry, v: &Variant, seeds: &[u64]) -> [f64; 9] {
    let mut sum = [0.0; 9];
    for &seed in seeds {
        let mut sc = (e.build)(seed);
        (v.apply)(&mut sc);
        let r = match e.codec {
            Codec::Fake => run(&sc, &mut FakeCodec::new(sc.fps, sc.seed)),
            Codec::Pixel => {
                let mut c = pixel_codec(&sc);
                run(&sc, &mut c)
            }
        };
        assert_eq!(r.decode_errors, 0, "{} / {} seed {seed}", e.name, v.name);
        if std::env::var_os("SIM_SECONDS").is_some() {
            println!("{} / {} seed {seed}:\n{}", e.name, v.name, r.steps_text());
            seconds(&r, &sc);
            trace(&r, 0, sc.duration_us);
        }
        for (s, x) in sum.iter_mut().zip(measure(&r, e.from, sc.duration_us)) {
            *s += x;
        }
    }
    sum.map(|s| s / seeds.len() as f64)
}

fn data_line(name: &str, variant: &str, v: &[f64; 9]) -> String {
    let nums: Vec<String> = v.iter().map(|x| format!("{x:.3}")).collect();
    format!("{name}\t{variant}\t{}", nums.join(" "))
}

/// Reads a baseline written by `data_line`: (scenario, variant) -> numbers.
fn read_base(path: &str) -> Vec<((String, String), [f64; 9])> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.lines()
        .filter_map(|l| {
            let mut p = l.split('\t');
            let (n, var, nums) = (p.next()?, p.next()?, p.next()?);
            let v: Vec<f64> = nums.split(' ').filter_map(|x| x.parse().ok()).collect();
            Some(((n.to_string(), var.to_string()), <[f64; 9]>::try_from(v).ok()?))
        })
        .collect()
}

fn cells(v: &[f64; 9], signed: bool) -> String {
    let f = |x: f64, w: usize, p: usize| {
        if signed { format!("{x:+w$.p$}") } else { format!("{x:w$.p$}") }
    };
    format!(
        "{} {} {} {} {} {} {} {} {}",
        f(v[0], 6, 0),
        f(v[1], 5, 0),
        f(v[2], 5, 0),
        f(v[3], 5, 0),
        f(v[4], 6, 0),
        f(v[5], 5, 1),
        f(v[6], 6, 1),
        f(v[7], 5, 0),
        f(v[8], 5, 0)
    )
}

#[test]
#[ignore]
fn scoreboard() {
    let seeds: Vec<u64> = match std::env::var("SIM_SEEDS") {
        Ok(v) if v.contains(',') => v.split(',').filter_map(|x| x.parse().ok()).collect(),
        Ok(v) => v.parse::<u64>().map_or(vec![3, 7, 11], |n| (1..=n).collect()),
        Err(_) => vec![3, 7, 11],
    };
    let only = std::env::var("SIM_ONLY").ok();
    let want: Option<Vec<String>> =
        std::env::var("SIM_VARIANTS").ok().map(|v| v.split(',').map(str::to_string).collect());
    let vs: Vec<Variant> =
        variants().into_iter().filter(|v| want.as_ref().is_none_or(|w| w.iter().any(|n| n == v.name))).collect();
    assert!(!vs.is_empty(), "no variant matches SIM_VARIANTS");
    let base = std::env::var("SIM_SCOREBOARD_BASE").ok().map(|p| read_base(&p));

    println!("seeds {seeds:?}; columns are means over seeds");
    println!(
        "{:<14} {:<10} {:>6} {:>5} {:>5} {:>5} {:>6} {:>5} {:>6} {:>5} {:>5}",
        "scenario", "variant", COLS[0], COLS[1], COLS[2], COLS[3], COLS[4], COLS[5], COLS[6], COLS[7], COLS[8]
    );
    let mut data = Vec::new();
    for e in entries().iter().filter(|e| only.as_ref().is_none_or(|o| e.name.contains(o.as_str()))) {
        for v in &vs {
            let row = run_row(e, v, &seeds);
            println!("{:<14} {:<10} {}", e.name, v.name, cells(&row, false));
            if let Some(b) = base.as_ref().and_then(|b| b.iter().find(|x| x.0.0 == e.name && x.0.1 == v.name)) {
                let mut d = [0.0; 9];
                for i in 0..9 {
                    d[i] = row[i] - b.1[i];
                }
                println!("{:<14} {:<10} {}", "", "  vs base", cells(&d, true));
            }
            data.push(data_line(e.name, v.name, &row));
        }
    }
    let text = data.join("\n") + "\n";
    println!("--- scoreboard data ---\n{text}--- end ---");
    if let Ok(path) = std::env::var("SIM_SCOREBOARD_OUT") {
        std::fs::write(&path, &text).unwrap_or_else(|e| panic!("{path}: {e}"));
    }
}
