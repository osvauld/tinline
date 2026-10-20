use super::*;
use std::time::Duration;

#[test]
fn motion_bezier_matches_css_endpoints_and_midpoints() {
    for e in [Ease::Out, Ease::InOut] {
        assert!(e.apply(0.0).abs() < 1e-4);
        assert!((e.apply(1.0) - 1.0).abs() < 1e-4);
    }
    // Exact curve values at x = .5 (bisection on the bezier): Out .9461, InOut .7756.
    assert!((Ease::Out.apply(0.5) - 0.9461).abs() < 1e-3, "{}", Ease::Out.apply(0.5));
    assert!((Ease::InOut.apply(0.5) - 0.7756).abs() < 1e-3, "{}", Ease::InOut.apply(0.5));
}

#[test]
fn motion_tween_retarget_starts_from_current_value() {
    let t0 = Instant::now();
    let mut tw = Tween::settled(0.0);
    tw.go(46.0, dur::INDICATOR, Ease::Out, t0);
    let mid = t0 + Duration::from_millis(100);
    let v = tw.at(mid);
    assert!(v > 0.0 && v < 46.0);
    tw.go(92.0, dur::INDICATOR, Ease::Out, mid);
    assert!((tw.at(mid) - v).abs() < 1e-3, "no jump on retarget");
    assert_eq!(tw.at(mid + Duration::from_millis(250)), 92.0);
    assert!(!tw.running(mid + Duration::from_millis(250)));
}
