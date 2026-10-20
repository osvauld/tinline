use super::*;

#[test]
fn downsampler_keeps_rate_and_level() {
    let mut d = Down::new(48_000, 16_000);
    let mut out = Vec::new();
    for i in 0..48_000 {
        d.push((2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin(), |o| out.push(o));
    }
    assert!((out.len() as i32 - 16_000).abs() <= 1, "{}", out.len());
    let peak = out.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    assert!(peak > 0.9 && peak <= 1.0, "{peak}");
    // Odd ratio (44.1k) stays in step too.
    let mut d = Down::new(44_100, 16_000);
    let mut n = 0i32;
    for _ in 0..44_100 {
        d.push(0.5, |_| n += 1);
    }
    assert!((n - 16_000).abs() <= 1, "{n}");
}

#[test]
fn upsampler_produces_the_ratio() {
    let mut u = Up::new(16_000, 48_000);
    let mut n = 0i32;
    for _ in 0..50 {
        u.process(&[0.1; 320], |_| n += 1);
    }
    assert!((n - 48_000).abs() <= 3, "{n}");
}
