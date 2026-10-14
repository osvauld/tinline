//! Live and offline viewers share this actual PCM -> spectrum -> model fixture.
mod motion;
mod fixture;
#[path = "../../crates/gcall-bench/src/spectral.rs"] mod spectral;
use motion::Model;

#[test]
fn tone_buttons_keep_intensity_but_change_shape() {
    fn run(low: bool, high: bool) -> Model {
        let mut m = Model::new();
        let mut e = spectral::Extractor::new();
        let mut sample = 0;
        for _ in 0..90 {
            fixture::tone(&mut m, &mut e, &mut sample, low, high);
            m.step(1.0 / 30.0);
        }
        m
    }
    let low = run(true, false);
    let high = run(false, true);
    let mix = run(true, true);
    assert!((low.fast - high.fast).abs() < 1e-12);
    assert!((low.fast - mix.fast).abs() < 1e-12);
    assert!(low.spectral_balance < 0.1 && high.spectral_balance > 0.9);
    assert!(mix.spectral_balance > 0.3 && mix.spectral_balance < 0.7);
    assert!((low.point(0, 0.21).y - high.point(0, 0.21).y).abs() > 0.001);
}

#[test]
fn timeline_ends_without_an_eligible_speaker() {
    let mut m = Model::new();
    let mut e = std::array::from_fn(|_| spectral::Extractor::new());
    let mut sample = 0;
    for _ in 0..600 {
        fixture::synthetic(&mut m, &mut e, &mut sample);
        m.step(1.0 / 30.0);
    }
    assert_eq!(m.target(), 0.0);
    assert!(!m.feed(0, 35, true, false, m.time()));
}
