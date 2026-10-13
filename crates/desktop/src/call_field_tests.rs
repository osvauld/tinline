//! Parity with the playground's model.ts: `call_field_golden.txt` holds frames produced by the playground's own
//! model (tools/call-field/golden.mjs) for the same scripted call; every lobe must match.
use super::*;

const GOLDEN: &str = include_str!("call_field_golden.txt");

/// The scenarios in golden.mjs: (time ms, state, cue delay ms).
fn scenario(name: &str) -> &'static [(f64, State, f64)] {
    use State::*;
    match name {
        "out" => &[(0.0, Outgoing, 0.0), (2600.0, Connecting, 0.0), (3300.0, Active, 97.0), (9000.0, Waiting, 0.0), (12000.0, Active, 0.0), (14000.0, Reconnecting, 0.0), (16000.0, Active, 97.0), (18000.0, Ended, 0.0)],
        "inc" => &[(0.0, Incoming, 0.0), (4000.0, Connecting, 0.0), (4700.0, Active, 97.0), (8000.0, Failed, 0.0)],
        _ => unreachable!(),
    }
}

fn run(name: &str, theme: Theme, reduced: bool, mute_at: Option<f64>) -> Vec<(i64, Vec<f64>)> {
    let mut f = Field::new(theme, reduced);
    f.set_view(390.0, 844.0, 195.0, 300.0);
    let sc = scenario(name.split('-').next().unwrap());
    let mut si = 0;
    let mut out = Vec::new();
    let mut t: i64 = 0;
    while t <= 20000 {
        while si < sc.len() && sc[si].0 <= t as f64 {
            let (at, st, d) = sc[si];
            si += 1;
            f.set_state(st, at, Some(at + d));
        }
        if mute_at.is_some_and(|m| t as f64 >= m) {
            f.set_mute(true);
        }
        let fr = f.update(t as f64);
        if t % 160 < 16 || (t > 3200 && t < 4200) {
            let mut v = vec![fr.tau, fr.warp];
            for l in fr.lobes {
                v.extend([l.x, l.y, l.sigma, l.alpha, l.lab[0], l.lab[1], l.lab[2]]);
            }
            out.push((t, v));
        }
        t += if t % 1000 == 500 { 7 } else { 16 };
    }
    out
}

fn check(name: &str, theme: Theme, reduced: bool, mute_at: Option<f64>) {
    let want: Vec<(i64, Vec<f64>)> = GOLDEN
        .lines()
        .filter(|l| l.starts_with(&format!("{name} ")))
        .map(|l| {
            let mut it = l.split_whitespace().skip(1);
            let t = it.next().unwrap().parse().unwrap();
            (t, it.map(|x| x.parse().unwrap()).collect())
        })
        .collect();
    let got = run(name, theme, reduced, mute_at);
    assert_eq!(want.len(), got.len(), "{name}: frame count");
    let mut worst: f64 = 0.0;
    for ((tw, w), (tg, g)) in want.iter().zip(&got) {
        assert_eq!(tw, tg);
        assert_eq!(w.len(), 2 + K * 7, "golden frame shape");
        assert_eq!(w.len(), g.len(), "native frame shape");
        for (i, (a, b)) in w.iter().zip(g).enumerate() {
            let err = (a - b).abs() / a.abs().max(1.0);
            worst = worst.max(err);
            assert!(err < 1e-7, "{name} t={tw} value {i}: playground {a}, native {b}");
        }
    }
    eprintln!("{name}: {} frames, worst relative error {worst:e}", got.len());
}

#[test]
fn matches_the_playground_outgoing_call_light() {
    check("out-light", Theme::Light, false, None);
}

#[test]
fn matches_the_playground_outgoing_call_dark_muted() {
    check("out-dark", Theme::Dark, false, Some(10000.0));
}

#[test]
fn matches_the_playground_incoming_call_that_fails() {
    check("inc-light", Theme::Light, false, None);
}

#[test]
fn matches_the_playground_reduced_motion() {
    check("out-reduced", Theme::Light, true, None);
}

#[test]
fn ended_field_fades_and_stops_animating() {
    let mut f = Field::new(Theme::Light, false);
    f.set_state(State::Active, 0.0, None);
    f.update(1000.0);
    f.set_state(State::Ended, 1000.0, None);
    let mut t = 1000.0;
    while f.update(t).animating {
        t += 16.0;
        assert!(t < 2000.0, "the end fade takes 300 ms");
    }
    assert!(f.update(t).lobes.iter().all(|l| l.alpha == 0.0));
}

#[test]
fn reduced_motion_sleeps_until_the_next_note() {
    let mut f = Field::new(Theme::Light, true);
    f.set_state(State::Incoming, 0.0, Some(0.0));
    let mut t = 0.0;
    let mut fr = f.update(t);
    while fr.animating {
        t += 16.0;
        fr = f.update(t);
    }
    let wake = fr.wake_at.expect("a looping ring always has a next note");
    assert!(wake > t && wake <= 3200.0, "wakes at the next ring note, got {wake}");
}

#[test]
fn model_cpu_cost_for_ten_thousand_frames() {
    let mut field = Field::new(Theme::Dark, false);
    field.set_state(State::Incoming, 0.0, None);
    let start = std::time::Instant::now();
    for i in 0..10_000 {
        std::hint::black_box(field.update(i as f64 * 16.0));
    }
    eprintln!("field model: {:.3} us/frame (debug, 10000 frames; excludes GPU)", start.elapsed().as_secs_f64() * 1e6 / 10_000.0);
}

#[test]
fn rng_is_bit_exact_with_mulberry32() {
    // First values of the JS rng(7), computed with the playground's function in Node.
    let mut r = Rng(7);
    let v: Vec<f64> = (0..3).map(|_| r.next()).collect();
    assert_eq!(v, [0.011704753153026104, 0.06195825757458806, 0.97690763277933]);
}
