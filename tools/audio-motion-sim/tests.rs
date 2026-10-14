//! Standalone model tests; no renderer, audio hardware or production API.
#[path = "motion.rs"]
mod motion;
use motion::*;

#[test]
fn envelopes_are_frame_rate_independent_for_constant_input() {
    fn run(hz: usize) -> Model {
        let mut m = Model::new();
        for _ in 0..hz * 2 {
            m.feed(0, 30, true, false, m.time());
            m.step(1.0 / hz as f64);
        }
        m
    }
    let a = run(30);
    let b = run(120);
    assert!((a.fast - b.fast).abs() < 1e-9);
    assert!((a.slow - b.slow).abs() < 1e-9);
}

#[test]
fn arrival_is_not_a_playout_input() {
    let mut m = Model::new();
    m.feed(1, 0, false, false, 0.0);
    m.step(0.02);
    assert_eq!(m.fast, 0.0);
    m.feed(1, 30, true, false, m.time());
    m.step(0.02);
    assert!(m.fast > 0.0);
}

#[test]
fn stale_mute_roster_and_end_cannot_resurrect_input() {
    let mut m = Model::new();
    m.feed(2, 10, true, false, 0.0);
    m.step(0.02);
    m.feed(2, 10, true, true, m.time());
    assert_eq!(m.target(), 0.0);
    m.feed(2, 10, true, false, m.time());
    m.set_roster(&[0, 1]);
    assert_eq!(m.target(), 0.0);
    assert!(!m.feed(2, 10, true, false, m.time()));
    m.feed(1, 10, true, false, m.time());
    m.step(0.5);
    assert_eq!(m.target(), 0.0);
    m.end_call();
    assert!(!m.feed(1, 10, true, false, m.time()));
    m.start_call();
    assert_eq!(m.target(), 0.0);
}

#[test]
fn future_and_out_of_order_updates_are_rejected() {
    let mut m = Model::new();
    assert!(!m.feed(0, 30, true, false, 1.0));
    m.step(0.1);
    assert!(m.feed(0, 30, true, false, 0.1));
    assert!(!m.feed(0, 0, true, false, 0.0));
    assert!(!m.feed(0, 255, true, false, 0.1));
}

#[test]
fn phase_never_restarts_when_levels_change() {
    let mut m = Model::new();
    m.step(1.0);
    let phase = m.phase;
    m.feed(0, 0, true, false, m.time());
    assert_eq!(m.phase, phase);
    m.step(0.02);
    assert!(m.phase > phase);
}

#[test]
fn joined_curves_have_matching_tangents() {
    let m = Model::new();
    let curve = m.contour(0);
    for pair in curve.windows(2) {
        assert_eq!(pair[0].end, pair[1].start);
        let a = pair[0].end - pair[0].c2;
        let b = pair[1].c1 - pair[1].start;
        assert!((a.x - b.x).abs() < 1e-12 && (a.y - b.y).abs() < 1e-12);
    }
}

#[test]
fn reduced_motion_freezes_geometry_and_particle_travel_but_retains_opacity() {
    let mut m = Model::new();
    m.set_reduced(true);
    let curve = m.contour(0);
    let position = m.particles[0].distance;
    let phase = m.phase;
    m.feed(0, 10, true, false, 0.0);
    m.step(0.1);
    assert_eq!(curve, m.contour(0));
    assert_eq!(position, m.particles[0].distance);
    assert_eq!(phase, m.phase);
    assert!(m.opacity() > 0.0);
}

#[test]
fn toggling_reduced_motion_does_not_snap_live_geometry() {
    let mut m = Model::new();
    for _ in 0..60 {
        m.feed(0, 10, true, false, m.time());
        m.step(1.0 / 60.0);
    }
    let before = m.contour(0);
    m.set_reduced(true);
    assert_eq!(before, m.contour(0));
    m.step(0.5);
    assert_eq!(before, m.contour(0));
    m.set_reduced(false);
    assert_eq!(before, m.contour(0));
}

#[test]
fn long_overlap_run_keeps_fixed_pool_trails_and_geometry_bounded() {
    let mut m = Model::new();
    for _ in 0..36_000 {
        for source in 0..MAX_SOURCES {
            m.feed_spectrum(
                source,
                0,
                true,
                false,
                m.time(),
                ShapeFeatures {
                    low: 0.0,
                    high: 1.0,
                    centroid_hz: 24000.0,
                    onset: 1.0,
                },
            );
        }
        m.step(1.0 / 60.0);
        assert!(m.fast <= 1.0 && m.slow <= 1.0 && m.opacity() <= 0.25);
        assert_eq!(m.particles.len(), PARTICLES);
        for p in &m.particles {
            assert!(p.distance >= 0.0 && p.distance < 1.0);
            assert!(p.trail_len <= TRAIL);
        }
        for i in 0..CONTOURS {
            for c in m.contour(i) {
                assert!(
                    [c.start, c.c1, c.c2, c.end]
                        .iter()
                        .all(|p| p.x.is_finite() && p.y >= 0.0 && p.y <= 1.0)
                );
            }
        }
    }
}

#[test]
fn equal_intensity_different_spectra_change_shape_not_just_scale() {
    fn run(features: ShapeFeatures) -> Model {
        let mut m = Model::new();
        for _ in 0..60 {
            assert!(m.feed_spectrum(0, 30, true, false, m.time(), features));
            m.step(1.0 / 60.0);
        }
        m
    }
    let a = run(ShapeFeatures {
        low: 1.0,
        high: 0.0,
        centroid_hz: 200.0,
        onset: 0.0,
    });
    let b = run(ShapeFeatures {
        low: 0.0,
        high: 1.0,
        centroid_hz: 4000.0,
        onset: 0.0,
    });
    assert!((a.fast - b.fast).abs() < 1e-12);
    assert!((a.point(0, 0.21).y - b.point(0, 0.21).y).abs() > 0.001);
    assert_eq!(a.particles.len(), b.particles.len());
}

#[test]
fn nonfinite_spectral_features_are_not_accepted() {
    let mut m = Model::new();
    assert!(!m.feed_spectrum(
        0,
        30,
        true,
        false,
        0.0,
        ShapeFeatures {
            low: 0.5,
            high: 0.5,
            centroid_hz: f64::NAN,
            onset: 0.0
        }
    ));
    assert_eq!(m.target(), 0.0);
}

#[test]
fn arc_map_returns_uniform_distance_on_straight_line() {
    let map = ArcMap::new(|x| Point { x, y: 0.5 }, 2.0);
    for i in 0..10 {
        let a = map.sample(i as f64 / 10.0);
        let b = map.sample((i + 1) as f64 / 10.0);
        assert!((b.x - a.x - 0.1).abs() < 1e-10);
    }
}
