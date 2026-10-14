#[path = "../../crates/gcall-bench/src/spectral.rs"]
mod spectral;
use spectral::*;

fn tone(freqs: &[(f64, f64)]) -> [i16; HOP] {
    std::array::from_fn(|i| {
        freqs
            .iter()
            .map(|&(f, amp)| amp * (std::f64::consts::TAU * f * i as f64 / RATE).sin())
            .sum::<f64>() as i16
    })
}

#[test]
fn silence_has_no_energy_or_invented_pitch() {
    let mut s = Extractor::new();
    let f = s.push(&[0; HOP]).unwrap();
    assert_eq!(f.low + f.mid + f.high, 0.0);
    assert_eq!(f.centroid_hz, 0.0);
    assert_eq!(f.onset, 0.0);
}

#[test]
fn equal_amplitude_low_and_high_tones_are_distinguished() {
    let low = Extractor::new().push(&tone(&[(200.0, 4000.0)])).unwrap();
    let high = Extractor::new().push(&tone(&[(4000.0, 4000.0)])).unwrap();
    assert!(low.low > 0.98 && low.high < 0.01);
    assert!(high.high > 0.98 && high.low < 0.01);
    assert!((low.centroid_hz - 200.0).abs() < 60.0);
    assert!((high.centroid_hz - 4000.0).abs() < 60.0);
}

#[test]
fn mixture_retains_both_frequency_components() {
    let f = Extractor::new()
        .push(&tone(&[(200.0, 3000.0), (4000.0, 3000.0)]))
        .unwrap();
    assert!((f.low - 0.5).abs() < 0.05);
    assert!((f.high - 0.5).abs() < 0.05);
    assert!((f.centroid_hz - 2100.0).abs() < 100.0);
}

#[test]
fn frame_chunking_does_not_change_the_spectrum() {
    let pcm = tone(&[(1200.0, 4000.0)]);
    let a = Extractor::new().push(&pcm).unwrap();
    let mut s = Extractor::new();
    assert!(s.push(&pcm[..960]).is_none());
    let b = s.push(&pcm[960..]).unwrap();
    assert!((a.centroid_hz - b.centroid_hz).abs() < 1e-9);
    assert!((a.mid - b.mid).abs() < 1e-9);
}

#[test]
fn attack_is_positive_and_decay_is_not_a_new_onset() {
    let mut s = Extractor::new();
    let strong = tone(&[(1000.0, 6000.0)]);
    assert!(s.push(&strong).unwrap().onset > 0.9);
    assert!(s.push(&tone(&[(1000.0, 1000.0)])).unwrap().onset < 0.01);
}

#[test]
fn reset_clears_history_and_output_is_bounded() {
    let mut s = Extractor::new();
    s.push(&tone(&[(1000.0, 6000.0)]));
    s.reset();
    let f = s.push(&[0; HOP]).unwrap();
    assert_eq!(f.onset, 0.0);
    for _ in 0..100 {
        let f = s.push(&tone(&[(500.0, 7000.0), (3500.0, 5000.0)])).unwrap();
        assert!((f.low + f.mid + f.high - 1.0).abs() < 1e-8);
        assert!((0.0..=1.0).contains(&f.onset));
        assert!((0.0..=RATE / 2.0).contains(&f.centroid_hz));
    }
}
