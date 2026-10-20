use super::*;

#[test]
fn tick_is_the_lab_string_and_slack_hangs_between_level_ends() {
    let p = CRISP;
    let t = tick_pts(&p);
    assert_eq!(t.len(), N);
    assert!((t[0][0] - p.k1[0]).abs() < 1e-9 && (t[0][1] - p.k1[1]).abs() < 1e-9);
    assert!((t[N - 1][0] - p.k2[0]).abs() < 1e-9 && (t[N - 1][1] - p.k2[1]).abs() < 1e-9);
    // It reads as a tick: it leaves K1 heading down-right (no hook back to the left), turns at a corner well
    // below both ends, and the long arm rises to K2.
    assert!(t[1][0] > t[0][0] && t[1][1] > t[0][1], "short arm starts down-right: {:?}", &t[..3]);
    let low = t.iter().cloned().fold([0.0, f64::MIN], |a, q| if q[1] > a[1] { q } else { a });
    assert!(low[1] > p.k1[1] + 3.0 && low[0] > p.k1[0] + 2.0 && low[0] < 12.0, "corner {low:?}");
    // The Bézier's true midpoint sags exactly s from the chord's midpoint, along ψ.
    let c = ctrl(&p);
    let mid = [(p.k1[0] + 2.0 * c[0] + p.k2[0]) / 4.0, (p.k1[1] + 2.0 * c[1] + p.k2[1]) / 4.0];
    let chord = [(p.k1[0] + p.k2[0]) / 2.0, (p.k1[1] + p.k2[1]) / 2.0];
    assert!(((mid[0] - chord[0]).hypot(mid[1] - chord[1]) - p.s).abs() < 1e-9);
    // Evenly spaced points (so any two states interpolate point for point).
    let seg: Vec<f64> = t.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).collect();
    let avg = seg.iter().sum::<f64>() / seg.len() as f64;
    assert!(seg.iter().all(|d| (d - avg).abs() / avg < 0.05), "{seg:?}");
    // The construction is the lab's (tick-math.ts stringPts); with the lab's crisp numbers it matched node to
    // 3 decimals on 2026-10-15 (e.g. point 6 = (6.507, 14.776)). Keep that check on the formula itself.
    let lab = TickP { k1: [6.0, 12.0], k2: [19.5, 4.0], s: 7.5, psi: -35.0, ..p };
    let lt = tick_pts(&lab);
    assert!((lt[6][0] - 6.507).abs() < 2e-3 && (lt[6][1] - 14.776).abs() < 2e-3, "{:?}", lt[6]);
    let s = slack_pts(&t);
    assert_eq!(s.len(), N);
    assert_eq!(s[0], t[0]);
    assert!((s[N - 1][1] - t[0][1]).abs() < 1e-9 && (s[N - 1][0] - t[N - 1][0]).abs() < 1e-9, "level ends");
    assert!(s[N / 2][1] > s[0][1] + 1.0, "it hangs down");
    assert_eq!(vb_w(&p), 26.0);
}

#[test]
fn spring_curves_are_one_shot_and_end_at_rest() {
    let pull = pull_curve();
    assert!(pull[0] < 0.2 && pull[0] >= -0.05, "starts slack: {}", pull[0]);
    assert_eq!(*pull.last().unwrap(), 1.0);
    // The cans recoil off the tight string (recoil 0.5) and are pulled out again, so k dips and returns before
    // it rests: the mark's real connect, not a monotonic ease.
    assert!(pull.iter().any(|k| *k > 0.85));
    assert!(pull.len() < (2.0 * FRAME_HZ) as usize, "settles in under 2 s: {}", pull.len());
    // tinmark.js driven at 120 Hz in node (the lab's own spring), 2026-10-15.
    let lab_pull = [0.011, 0.036, 0.071, 0.115, 0.166, 0.222, 0.284, 0.352, 0.424, 0.504, 0.593, 0.700, 0.859, 0.835, 0.704];
    for (i, want) in lab_pull.iter().enumerate() {
        assert!((pull[i] - want).abs() < 0.01, "pull[{i}] = {} vs lab {want}", pull[i]);
    }
    let pluck = pluck_curve();
    assert_eq!(*pluck.last().unwrap(), 1.0);
    assert!(pluck.iter().any(|k| (k - 1.0).abs() > 0.1), "a visible pluck");
    assert!(pluck.len() < FRAME_HZ as usize, "{}", pluck.len());
    let lab_pluck = [0.939, 0.895, 0.870, 0.866, 0.879, 0.906, 0.941, 0.979, 1.014, 1.043, 1.062, 1.070];
    for (i, want) in lab_pluck.iter().enumerate() {
        assert!((pluck[i] - want).abs() < 0.005, "pluck[{i}] = {} vs lab {want}", pluck[i]);
    }
}
