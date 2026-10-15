//! Generated UI sounds (no sample files): a port of the playground's synth
//! (docs/design/playground/src/lib/fx/audio.ts, SIGNUP-HANDOFF.md § 6d). Wired: `line` (sign-up Done
//! and a contact added, both sides) and `knock` (someone asks to add you); the others belong to the
//! chat port.
//!
//! Instruments: `Sine` (the call chimes: a sine and its octave at 18 %, linear attack, exponential
//! decay), `Pluck` (the line: Karplus-Strong, a softened noise burst circulating in a delay one period
//! long, averaged each pass, decaying to -60 dB over the note). Each pluck is detuned by up to
//! ±4 cents, so it never sounds stamped. `Tok` (the can: five inharmonic partials of a small struck
//! tin, higher ones dying sooner, plus a 4 ms tap of softened noise). The mix goes through a 5 kHz low-pass, as `cue()` does.
//! Lives in the core so the desktop and Android play the same samples (`call_cue_pcm` for the apps over uniffi).

/// Sample rate of the rendered cue (the call audio's rate).
pub const RATE: u32 = 48_000;

/// Offset of the `line` cue's second note (s): the mark's two tugs land this far apart.
pub const LINE_GAP_S: f64 = 0.12;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Inst {
    Sine,
    Pluck,
    Tok,
}

/// (frequency Hz, start s, length s, peak, attack s, instrument), as `PAT` in audio.ts.
type Note = (f64, f64, f64, f64, f64, Inst);

/// `line`: the line pulls tight in two tugs, D5 then G5 with a soft G4 under the second.
const LINE: [Note; 3] = [
    (587.0, 0.0, 1.4, 0.55, 0.003, Inst::Pluck),
    (784.0, LINE_GAP_S, 1.8, 0.55, 0.003, Inst::Pluck),
    (392.0, LINE_GAP_S, 1.6, 0.12, 0.06, Inst::Sine),
];

/// `knock`: someone asks to add you. Two tin taps, the second a little lower (about -4 dB vs Connected).
const KNOCK: [Note; 2] = [(640.0, 0.0, 0.35, 0.34, 0.002, Inst::Tok), (600.0, 0.17, 0.45, 0.3, 0.002, Inst::Tok)];

/// audio.ts: `vol` 0.5 × 0.25 × the cue's gain.
const MASTER: f64 = 0.5 * 0.25;

/// A tiny xorshift so each play differs (the browser uses Math.random).
struct Rng(u64);
impl Rng {
    fn seeded() -> Rng {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        Rng(t | 1)
    }
    /// Uniform in [0, 1).
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Karplus-Strong, normalised to peak 1 with a 256-sample fade at the end (audio.ts pluckBuf + norm).
fn pluck(f: f64, dur: f64, rng: &mut Rng) -> Vec<f32> {
    let soft = 0.45;
    let sr = RATE as f64;
    let n = (sr * dur).ceil() as usize;
    let mut y = vec![0f64; n];
    let period = sr / f - 0.5;
    let m = period.floor() as usize;
    let g = period - m as f64;
    let decay = 0.001f64.powf(1.0 / (f * dur));
    let mut lp = 0.0;
    for v in y.iter_mut().take((m + 2).min(n)) {
        lp += soft * (rng.next() * 2.0 - 1.0 - lp);
        *v = lp;
    }
    for i in m + 2..n {
        y[i] = decay * 0.5 * ((1.0 - g) * (y[i - m] + y[i - m - 1]) + g * (y[i - m - 1] + y[i - m - 2]));
    }
    norm(y)
}

/// audio.ts tokBuf: (ratio, amplitude, decay share) per partial; each decays to -60 dB over its share
/// of the note, with a random phase.
fn tok(f: f64, dur: f64, rng: &mut Rng) -> Vec<f32> {
    const P: [(f64, f64, f64); 5] = [(1.0, 1.0, 1.0), (1.51, 0.55, 0.55), (2.03, 0.35, 0.4), (2.47, 0.22, 0.28), (3.31, 0.12, 0.18)];
    let sr = RATE as f64;
    let n = (sr * dur).ceil() as usize;
    let mut y = vec![0f64; n];
    for (r, a, d) in P {
        let w = 2.0 * std::f64::consts::PI * f * r / sr;
        let k = 1.0 / (sr * dur * d / 6.9);
        let ph = rng.next() * 6.28;
        for (i, v) in y.iter_mut().enumerate() {
            *v += a * (-(i as f64) * k).exp() * (w * i as f64 + ph).sin();
        }
    }
    let mut lp = 0.0;
    let tap = (sr * 0.004) as usize;
    for (i, v) in y.iter_mut().enumerate().take(tap.min(n)) {
        lp += 0.3 * (rng.next() * 2.0 - 1.0 - lp);
        *v += 0.6 * lp * (1.0 - i as f64 / tap as f64);
    }
    norm(y)
}

fn norm(mut y: Vec<f64>) -> Vec<f32> {
    let mx = y.iter().fold(0f64, |a, v| a.max(v.abs()));
    if mx > 0.0 {
        y.iter_mut().for_each(|v| *v /= mx);
    }
    let len = y.len();
    let fade = 256.min(len);
    for i in 0..fade {
        y[len - 1 - i] *= i as f64 / fade as f64;
    }
    y.into_iter().map(|v| v as f32).collect()
}

/// RBJ low-pass biquad (Q = 1/√2), the Web Audio `lowpass` default.
fn lowpass(x: &mut [f32], cutoff: f64) {
    let w0 = 2.0 * std::f64::consts::PI * cutoff / RATE as f64;
    let (s, c) = w0.sin_cos();
    let alpha = s / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
    let a0 = 1.0 + alpha;
    let (b0, b1, b2) = ((1.0 - c) / 2.0 / a0, (1.0 - c) / a0, (1.0 - c) / 2.0 / a0);
    let (a1, a2) = (-2.0 * c / a0, (1.0 - alpha) / a0);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    for v in x.iter_mut() {
        let x0 = *v as f64;
        let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0;
        *v = y0 as f32;
    }
}

fn render(notes: &[Note], rng: &mut Rng) -> Vec<f32> {
    render_with(notes, rng, 5000.0, 1.0)
}

/// `render` with the player's low-pass and gain: `cue()` layers at 5 kHz, `play()` (calls) uses 3.2 kHz.
fn render_with(notes: &[Note], rng: &mut Rng, cutoff: f64, gain: f64) -> Vec<f32> {
    let sr = RATE as f64;
    let end = notes.iter().map(|n| n.1 + n.2).fold(0.0, f64::max) + 0.05;
    let mut out = vec![0f32; (end * sr).ceil() as usize];
    for &(f, at, dur, peak, atk, inst) in notes {
        let t0 = (at * sr).round() as usize;
        match inst {
            Inst::Pluck | Inst::Tok => {
                let human = 2f64.powf((rng.next() - 0.5) * 8.0 / 1200.0);
                let buf = if inst == Inst::Pluck { pluck(f * human, dur, rng) } else { tok(f * human, dur, rng) };
                for (i, v) in buf.iter().enumerate() {
                    let t = i as f64 / sr;
                    let env = if t < atk { peak * t / atk } else { peak };
                    if let Some(o) = out.get_mut(t0 + i) {
                        *o += (*v as f64 * env) as f32;
                    }
                }
            }
            Inst::Sine => {
                let len = ((dur + 0.05) * sr) as usize;
                for i in 0..len {
                    let t = i as f64 / sr;
                    // linear ramp to the peak, then exponential to 0.0001 at `dur`
                    let env = if t < atk { peak * t / atk } else if t < dur { peak * (0.0001f64 / peak).powf((t - atk) / (dur - atk)) } else { 0.0 };
                    let ph = 2.0 * std::f64::consts::PI * f * t;
                    let v = env * (ph.sin() + 0.18 * (2.0 * ph).sin());
                    if let Some(o) = out.get_mut(t0 + i) {
                        *o += v as f32;
                    }
                }
            }
        }
    }
    lowpass(&mut out, cutoff);
    out.iter_mut().for_each(|v| *v = (*v as f64 * MASTER * gain) as f32);
    out
}

/// The `line` cue at 48 kHz mono, ready to play. Different on every call (detune, noise).
pub fn line() -> Vec<f32> {
    render(&LINE, &mut Rng::seeded())
}

/// The `knock` cue (someone asks to add you), 48 kHz mono. Different on every call.
pub fn knock() -> Vec<f32> {
    render(&KNOCK, &mut Rng::seeded())
}

/// Settings > Sounds: the call sound set. `Classic` is PAT; `Slack` and `Triad` replace the call cues they name
/// (audio.ts PALETTES; the sign-up and message sounds never change).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Palette { #[default] Classic, Slack, Triad }

impl Palette {
    pub const ALL: [Palette; 3] = [Palette::Classic, Palette::Slack, Palette::Triad];
    pub fn key(self) -> &'static str { match self { Palette::Classic => "classic", Palette::Slack => "slack", Palette::Triad => "triad" } }
    pub fn from_key(k: &str) -> Palette { Palette::ALL.into_iter().find(|p| p.key() == k).unwrap_or_default() }
    /// Sounds.svelte: the name and one line on what it sounds like.
    pub fn label(self) -> (&'static str, &'static str) {
        match self {
            Palette::Classic => ("Classic chime", "A bright four-note chime."),
            Palette::Slack => ("Slack \u{2192} taut", "Two close notes beat like a loose string, then one pulls tight."),
            Palette::Triad => ("Triad knock", "Three rising notes, like a knock."),
        }
    }
}

const S: Inst = Inst::Sine;
/// audio.ts PAT (the classic call cues): (period ms, notes); period 0 = a one-shot.
fn classic(kind: &str) -> Option<(u32, Vec<Note>)> {
    Some(match kind {
        "ring" => (3200, vec![(784.0, 0.0, 1.0, 0.5, 0.02, S), (988.0, 0.18, 1.0, 0.45, 0.02, S), (1175.0, 0.36, 1.0, 0.4, 0.02, S), (1568.0, 0.7, 1.4, 0.3, 0.02, S)]),
        "back" => (4000, vec![(392.0, 0.0, 1.2, 0.4, 0.18, S), (494.0, 0.0, 1.2, 0.35, 0.18, S)]),
        "connected" => (0, vec![(523.0, 0.0, 0.5, 0.45, 0.02, S), (784.0, 0.12, 0.7, 0.45, 0.02, S)]),
        "ended" => (0, vec![(784.0, 0.0, 0.45, 0.4, 0.02, S), (523.0, 0.12, 0.7, 0.4, 0.02, S)]),
        "failed" => (0, vec![(247.0, 0.0, 0.5, 0.45, 0.02, S), (196.0, 0.24, 0.8, 0.45, 0.02, S)]),
        "pulse" => (0, vec![(330.0, 0.0, 0.6, 0.35, 0.02, S)]),
        "waiting" => (3000, vec![(784.0, 0.0, 0.16, 0.22, 0.012, S), (784.0, 0.22, 0.2, 0.22, 0.012, S)]),
        "gstart" => (0, vec![(587.0, 0.0, 0.3, 0.3, 0.02, S), (784.0, 0.14, 0.5, 0.3, 0.02, S)]),
        "gjoin" => (0, vec![(659.0, 0.0, 0.32, 0.3, 0.02, S)]),
        "gleft" => (0, vec![(494.0, 0.0, 0.36, 0.26, 0.02, S)]),
        "sent" => (0, vec![(1175.0, 0.0, 0.5, 0.18, 0.003, Inst::Pluck)]),
        "received" => (0, vec![(880.0, 0.0, 0.45, 0.32, 0.002, Inst::Tok)]),
        _ => return None,
    })
}

/// audio.ts PALETTES: what `slack` and `triad` replace.
fn palette_override(p: Palette, kind: &str) -> Option<(u32, Vec<Note>)> {
    Some(match (p, kind) {
        (Palette::Slack, "ring") => (2600, vec![(392.0, 0.0, 0.9, 0.35, 0.08, S), (396.0, 0.0, 0.9, 0.3, 0.08, S), (587.0, 1.05, 0.45, 0.5, 0.015, S)]),
        (Palette::Slack, "back") => (2200, vec![(392.0, 0.0, 1.1, 0.32, 0.12, S), (396.0, 0.0, 1.1, 0.28, 0.12, S)]),
        (Palette::Slack, "connected") => (0, vec![(587.0, 0.0, 0.5, 0.5, 0.015, S)]),
        (Palette::Slack, "waiting") => (3000, vec![(587.0, 0.0, 0.16, 0.2, 0.012, S), (587.0, 0.22, 0.2, 0.22, 0.012, S)]),
        (Palette::Slack, "gstart") => (0, vec![(392.0, 0.0, 0.4, 0.28, 0.03, S), (587.0, 0.16, 0.55, 0.3, 0.03, S)]),
        (Palette::Slack, "gjoin") => (0, vec![(587.0, 0.0, 0.34, 0.26, 0.02, S)]),
        (Palette::Slack, "gleft") => (0, vec![(392.0, 0.0, 0.4, 0.24, 0.03, S)]),
        (Palette::Triad, "ring") => (2400, vec![(392.0, 0.0, 0.32, 0.45, 0.015, S), (494.0, 0.16, 0.32, 0.45, 0.015, S), (587.0, 0.32, 0.5, 0.5, 0.015, S)]),
        (Palette::Triad, "back") => (3400, vec![(392.0, 0.0, 0.4, 0.3, 0.1, S), (494.0, 0.22, 0.4, 0.3, 0.1, S), (587.0, 0.44, 0.55, 0.32, 0.1, S)]),
        (Palette::Triad, "connected") => (0, vec![(392.0, 0.0, 0.3, 0.4, 0.02, S), (587.0, 0.14, 0.5, 0.5, 0.02, S)]),
        (Palette::Triad, "waiting") => (3000, vec![(494.0, 0.0, 0.16, 0.2, 0.012, S), (587.0, 0.2, 0.2, 0.22, 0.012, S)]),
        (Palette::Triad, "gstart") => (0, vec![(494.0, 0.0, 0.28, 0.26, 0.02, S), (587.0, 0.12, 0.5, 0.3, 0.02, S)]),
        (Palette::Triad, "gjoin") => (0, vec![(587.0, 0.0, 0.3, 0.26, 0.02, S)]),
        (Palette::Triad, "gleft") => (0, vec![(494.0, 0.0, 0.34, 0.24, 0.02, S)]),
        _ => return None,
    })
}

fn pattern(kind: &str, p: Palette) -> Option<(u32, Vec<Note>)> {
    palette_override(p, kind).or_else(|| classic(kind))
}

/// A call cue as `play()` makes it (3.2 kHz low-pass, `gain`): one shot, or one period of a looping cue,
/// padded with silence to the period so playing it back to back loops it. 48 kHz mono.
pub fn call(kind: &str, p: Palette, gain: f64) -> Option<Vec<f32>> {
    let (period, notes) = pattern(kind, p)?;
    let mut out = render_with(&notes, &mut Rng::seeded(), 3200.0, gain);
    if period > 0 {
        out.resize((period as u64 * RATE as u64 / 1000) as usize, 0.0);
    }
    Some(out)
}

/// A rendered call cue for the apps that cannot call the Rust API directly (Android).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CuePcm {
    /// Mono signed 16-bit samples.
    pub samples: Vec<i16>,
    pub rate: u32,
    /// True for a looping cue (ring, back, waiting): `samples` is exactly one period, play it back to back.
    pub looped: bool,
}

/// Settings > Sounds on Android: renders call cue `cue` (`ring`, `back`, `connected`, `ended`, ...) in sound set
/// `palette` (`classic`, `slack`, `triad`; anything else is classic) at `gain` (1.0 = the desktop level).
/// Different on every call (detune, noise). `None` for an unknown cue.
#[uniffi::export]
pub fn call_cue_pcm(palette: String, cue: String, gain: f64) -> Option<CuePcm> {
    let p = Palette::from_key(&palette);
    let looped = pattern(&cue, p)?.0 > 0;
    let f = call(&cue, p, gain)?;
    let samples = f.iter().map(|v| (v.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
    Some(CuePcm { samples, rate: RATE, looped })
}

/// A one-shot that layers over whatever plays (`cue()`): sent, received. 48 kHz mono.
pub fn cue(kind: &str) -> Option<Vec<f32>> {
    let (_, notes) = classic(kind)?;
    Some(render(&notes, &mut Rng::seeded()))
}

#[cfg(test)]
mod tests {
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
    fn plucks_differ_each_time() {
        let a = render(&LINE, &mut Rng(1));
        let b = render(&LINE, &mut Rng(2));
        assert_ne!(a[1000], b[1000]);
    }
}
