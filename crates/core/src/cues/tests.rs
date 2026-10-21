use super::*;

#[test]
fn line_cue_has_two_plucks_at_the_tug_gap_and_stays_quiet() {
    let s = render(&LINE, &mut Rng(7));
    assert!(s.len() as f64 / RATE as f64 > 1.9, "the G5 rings out for 1.8 s after its 0.12 s start");
    let peak = s.iter().fold(0f32, |a, v| a.max(v.abs()));
    assert!(peak > 0.03 && peak < 0.5, "audible but mellow, got {peak}");
    // energy jumps at the second note's onset
    let rms = |a: usize, b: usize| (s[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32).sqrt();
    let at = |t: f64| (t * RATE as f64) as usize;
    assert!(rms(at(0.0), at(0.01)) > 0.0);
    assert!(rms(at(0.121), at(0.14)) > rms(at(0.10), at(0.119)));
    assert!(s.iter().all(|v| v.is_finite()));
}

#[test]
fn knock_is_two_taps_quieter_than_the_line() {
    let k = render(&KNOCK, &mut Rng(3));
    let l = render(&LINE, &mut Rng(3));
    let peak = |s: &[f32]| s.iter().fold(0f32, |a, v| a.max(v.abs()));
    assert!(peak(&k) > 0.01 && peak(&k) < peak(&l), "knock {} line {}", peak(&k), peak(&l));
    let rms = |a: usize, b: usize| (k[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32).sqrt();
    let at = |t: f64| (t * RATE as f64) as usize;
    assert!(rms(at(0.171), at(0.19)) > rms(at(0.15), at(0.169)), "the second tap lands at 0.17 s");
}

#[test]
fn call_cues_loop_on_their_period_and_palettes_replace_only_their_cues() {
    let ring = call("ring", Palette::Classic, 1.0).unwrap();
    assert_eq!(ring.len(), 3200 * 48);
    assert_eq!(call("ring", Palette::Triad, 1.0).unwrap().len(), 2400 * 48);
    assert_eq!(pattern("ended", Palette::Slack).unwrap().1, classic("ended").unwrap().1);
    assert!(call("nope", Palette::Classic, 1.0).is_none());
    let sent = cue("sent").unwrap();
    let recv = cue("received").unwrap();
    let peak = |s: &[f32]| s.iter().fold(0f32, |a, v| a.max(v.abs()));
    assert!(peak(&sent) < peak(&recv), "send is faint");
    assert_eq!(Palette::from_key("triad"), Palette::Triad);
    assert_eq!(Palette::from_key("?"), Palette::Classic);
}

#[test]
fn pcm_export_is_one_period_of_the_ring_and_unknown_cues_are_none() {
    let r = call_cue_pcm("triad".into(), "ring".into(), 1.0).unwrap();
    assert!(r.looped && r.rate == 48_000);
    assert_eq!(r.samples.len(), 2400 * 48);
    assert!(r.samples.iter().any(|v| v.abs() > 500));
    let c = call_cue_pcm("nope".into(), "connected".into(), 1.0).unwrap();
    assert!(!c.looped, "an unknown palette is classic");
    assert!(call_cue_pcm("classic".into(), "nope".into(), 1.0).is_none());
}

#[test]
fn received_is_one_soft_tin_tap_and_the_toy_set_exports_as_pcm() {
    let r = toy_cue_pcm("received".into(), 1.0).unwrap();
    assert!(!r.looped && r.rate == 48_000);
    let secs = r.samples.len() as f64 / RATE as f64;
    assert!(secs > 0.45 && secs < 0.6, "one tap of 0.45 s, got {secs}");
    let peak = r.samples.iter().map(|v| v.unsigned_abs()).max().unwrap();
    assert!(peak > 500 && peak < 12_000, "audible but calm, got {peak}");
    // the tap is at the start: the first 30 ms carry more energy than the last 30 ms
    let e = |a: &[i16]| a.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
    let n = (RATE as usize) * 30 / 1000;
    assert!(e(&r.samples[..n]) > 10.0 * e(&r.samples[r.samples.len() - n..]));
    let loud = toy_cue_pcm("received".into(), 4.0).unwrap();
    assert!(loud.samples.iter().map(|v| v.unsigned_abs()).max().unwrap() > peak);
    for k in ["sent", "line", "knock"] {
        assert!(toy_cue_pcm(k.into(), 1.0).is_some(), "{k}");
    }
    assert!(toy_cue_pcm("ring".into(), 1.0).is_none());
}

#[test]
fn plucks_differ_each_time() {
    let a = render(&LINE, &mut Rng(1));
    let b = render(&LINE, &mut Rng(2));
    assert_ne!(a[1000], b[1000]);
}
