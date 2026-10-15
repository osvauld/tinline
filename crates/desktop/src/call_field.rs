//! The call screen's colour field: five soft, hue-separated masses on Lissajous paths that swell and warm with
//! the call's cue notes. Numerical port of the playground's `lib/calls/field/model.ts` (same constants, same
//! state table, same OKLab mixing, same reduced-motion crossfade) and its WebGL shader (`gl.ts`, here WGSL in
//! `call_field.wgsl`, drawn by iced's wgpu Shader widget). Parity is tested against frames produced by the
//! playground's own model (`tools/call-field/golden.mjs` -> `call_field_golden.txt`).
//!
//! The note schedule is the classic palette's patterns (the field reads them; the app's real ring and cue audio
//! stay as they are). The model is pure: time is passed in (ms), no clocks, no audio.

use std::f64::consts::PI;

const F_MIN: f64 = 196.0;
const F_MAX: f64 = 1568.0;
pub const K: usize = 5;

fn clamp(x: f64, a: f64, b: f64) -> f64 {
    x.max(a).min(b)
}
fn smooth(x: f64) -> f64 {
    let x = clamp(x, 0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
fn lerp(a: f64, b: f64, s: f64) -> f64 {
    a + (b - a) * s
}
/// JS Math.round (half up).
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

// ---- OKLab ----------------------------------------------------------------------------------------------------
fn s2l(c: f64) -> f64 {
    let c = c / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
fn l2s(c: f64) -> f64 {
    let c = clamp(c, 0.0, 1.0);
    255.0 * if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}
pub fn rgb_to_lab(rgb: [f64; 3]) -> [f64; 3] {
    let (r, g, b) = (s2l(rgb[0]), s2l(rgb[1]), s2l(rgb[2]));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}
#[allow(dead_code)]
pub fn lab_to_rgb(c: [f64; 3]) -> [f64; 3] {
    let l = (c[0] + 0.3963377774 * c[1] + 0.2158037573 * c[2]).powi(3);
    let m = (c[0] - 0.1055613458 * c[1] - 0.0638541728 * c[2]).powi(3);
    let s = (c[0] - 0.0894841775 * c[1] - 1.2914855480 * c[2]).powi(3);
    [
        l2s(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        l2s(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        l2s(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
    ]
}
/// OKLab mix; chroma is interpolated separately so a teal->gold mid-mix does not go grey/olive.
fn mix_lab(a: [f64; 3], b: [f64; 3], s: f64) -> [f64; 3] {
    let (l, aa, bb) = (lerp(a[0], b[0], s), lerp(a[1], b[1], s), lerp(a[2], b[2], s));
    let (c0, c1, cm) = (a[1].hypot(a[2]), b[1].hypot(b[2]), aa.hypot(bb));
    let ct = lerp(c0, c1, s);
    let k = if cm > 1e-5 { ct / cm } else { 1.0 };
    [l, aa * k, bb * k]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}
struct ThemeDef {
    bg: [f64; 3],
    teal: [f64; 3],
    gold: [f64; 3],
}
fn theme_def(t: Theme) -> ThemeDef {
    match t {
        Theme::Light => ThemeDef { bg: [245.0, 246.0, 243.0], teal: [110.0, 210.0, 186.0], gold: [248.0, 206.0, 104.0] },
        Theme::Dark => ThemeDef { bg: [15.0, 21.0, 19.0], teal: [20.0, 184.0, 160.0], gold: [236.0, 126.0, 64.0] },
    }
}
#[derive(Clone, Copy)]
struct Labs {
    bg: [f64; 3],
    teal: [f64; 3],
    gold: [f64; 3],
}
fn labs(t: Theme) -> Labs {
    let d = theme_def(t);
    Labs { bg: rgb_to_lab(d.bg), teal: rgb_to_lab(d.teal), gold: rgb_to_lab(d.gold) }
}

// ---- tunables (model.ts DEFAULTS) ------------------------------------------------------------------------------
pub const OMEGA: f64 = 0.09;
pub const SIGMA0: f64 = 0.26;
pub const GAMMA: f64 = 0.5;
pub const A_NORM: f64 = 0.55;
pub const SPILL: f64 = 1.6;
pub const COLOUR_MS: f64 = 150.0;
pub const CAP_LIGHT: f64 = 0.76;
pub const CAP_DARK: f64 = 0.74;
pub const WARP_SCALE: f64 = 1.9;
pub const NOISE_RATE: f64 = 0.35;
pub const DT_MAX: f64 = 100.0;
pub const XF_MS: f64 = 180.0;
pub const PULL_AMT: f64 = 0.65;

/// hx, hy, ax, ay, wx, wy, px, py
const LJ: [[f64; 8]; K] = [
    [-0.55, -0.50, 0.30, 0.34, 1.0000, 1.3247, 0.3, 1.9],
    [0.62, -0.20, 0.34, 0.28, 0.7071, 1.6180, 2.2, 0.7],
    [-0.30, 0.10, 0.28, 0.38, 1.4142, 0.8660, 4.1, 3.3],
    [0.50, 0.45, 0.36, 0.30, 1.1180, 1.3591, 1.0, 5.0],
    [-0.15, 0.80, 0.32, 0.32, 0.9069, 1.7321, 5.5, 2.4],
];

// ---- note patterns (playground fx/audio.ts PAT, classic palette) ----------------------------------------------
/// freq Hz, at s, dur s, peak, attack s
pub type Note = [f64; 5];
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pattern {
    pub period_ms: f64,
    pub notes: &'static [Note],
}
pub const PAT_RING: Pattern = Pattern { period_ms: 3200.0, notes: &[[784.0, 0.0, 1.0, 0.5, 0.02], [988.0, 0.18, 1.0, 0.45, 0.02], [1175.0, 0.36, 1.0, 0.4, 0.02], [1568.0, 0.7, 1.4, 0.3, 0.02]] };
pub const PAT_BACK: Pattern = Pattern { period_ms: 4000.0, notes: &[[392.0, 0.0, 1.2, 0.4, 0.18], [494.0, 0.0, 1.2, 0.35, 0.18]] };
pub const PAT_CONNECTED: Pattern = Pattern { period_ms: 0.0, notes: &[[523.0, 0.0, 0.5, 0.45, 0.02], [784.0, 0.12, 0.7, 0.45, 0.02]] };
pub const PAT_ENDED: Pattern = Pattern { period_ms: 0.0, notes: &[[784.0, 0.0, 0.45, 0.4, 0.02], [523.0, 0.12, 0.7, 0.4, 0.02]] };
pub const PAT_FAILED: Pattern = Pattern { period_ms: 0.0, notes: &[[247.0, 0.0, 0.5, 0.45, 0.02], [196.0, 0.24, 0.8, 0.45, 0.02]] };
pub const PAT_PULSE: Pattern = Pattern { period_ms: 0.0, notes: &[[330.0, 0.0, 0.6, 0.35, 0.02]] };
pub const PAT_WAITING: Pattern = Pattern { period_ms: 3000.0, notes: &[[784.0, 0.0, 0.16, 0.22, 0.012], [784.0, 0.22, 0.2, 0.22, 0.012]] };
/// Offset (s) of the connect cue's landing note (classic palette `hit`): the mark's string reaches taut there.
pub const CONNECT_HIT_S: f64 = 0.12;
/// Measured in the playground: the mark's string reaches taut this long after connect (fx/motion.ts markHit).
pub const MARK_HIT_MS: f64 = 217.0;

fn note_level(n: &Note, tau: f64) -> f64 {
    let (dur, peak, atk) = (n[2], n[3], n[4]);
    if tau <= 0.0 || tau >= dur {
        return 0.0;
    }
    if tau < atk {
        return peak * tau / atk;
    }
    peak * (0.0001 / peak).powf((tau - atk) / (dur - atk))
}
fn pitch_u(f: f64) -> f64 {
    clamp((f / F_MIN).ln() / (F_MAX / F_MIN).ln(), 0.0, 1.0)
}
fn active_notes(p: &Pattern, tau_ms: f64, looped: bool, out: &mut Vec<(Note, f64)>) {
    out.clear();
    if tau_ms < 0.0 {
        return;
    }
    let tau = tau_ms / 1000.0;
    let per = p.period_ms / 1000.0;
    let max_end = p.notes.iter().map(|n| n[1] + n[2]).fold(f64::MIN, f64::max);
    let lp = looped && per != 0.0;
    let k_hi = if lp { (tau / per).floor() as i64 } else { 0 };
    let k_lo = if lp { (((tau - max_end) / per).floor() as i64).max(0) } else { 0 };
    for k in k_lo..=k_hi {
        for n in p.notes {
            let s = tau - k as f64 * per - n[1];
            if s > 0.0 && s < n[2] {
                out.push((*n, s));
            }
        }
    }
}

/// Mulberry32, bit-exact with the JS rng().
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6D2B79F5);
        let s = self.0;
        let mut t = (s ^ (s >> 15)).wrapping_mul(1 | s);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        (t ^ (t >> 14)) as f64 / 4294967296.0
    }
}
struct Burst {
    s: f64,
    e: f64,
    amp: f64,
    ph: f64,
}
/// Simulated speech bursts (model.ts makeVoice): the connected field breathes as if someone were talking.
struct Voice {
    r: Rng,
    bursts: Vec<Burst>,
    t: f64,
}
impl Voice {
    fn new(seed: u32) -> Voice {
        Voice { r: Rng(seed), bursts: Vec::new(), t: 600.0 }
    }
    fn at(&mut self, ms: f64) -> f64 {
        while self.t < ms + 6000.0 {
            let d = 700.0 + self.r.next() * 1800.0;
            let g = 500.0 + self.r.next() * 1500.0;
            let amp = 0.55 + self.r.next() * 0.45;
            let ph = self.r.next() * 6.28;
            self.bursts.push(Burst { s: self.t, e: self.t + d, amp, ph });
            self.t += d + g;
        }
        let mut v: f64 = 0.0;
        let n = self.bursts.len();
        let mut i = n as i64 - 1;
        while i >= 0 && i > n as i64 - 40 {
            let b = &self.bursts[i as usize];
            i -= 1;
            if ms < b.s {
                continue;
            }
            let up = smooth((ms - b.s) / 160.0);
            let rel = if ms > b.e { (-(ms - b.e) / 260.0).exp() } else { 1.0 };
            let syl = 0.78 + 0.22 * (ms / 1000.0 * 2.0 * PI * 3.6 + b.ph).sin() * (ms / 1000.0 * 2.0 * PI * 0.9 + 1.0).sin();
            v = v.max(b.amp * up * rel * syl);
            if ms > b.e + 2000.0 {
                break;
            }
        }
        v
    }
}

// ---- state table -----------------------------------------------------------------------------------------------
#[allow(dead_code, reason = "mirrors the playground state table")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Calm,
    Incoming,
    Outgoing,
    Connecting,
    Active,
    Waiting,
    Reconnecting,
    Ended,
    Failed,
}
impl State {
    #[allow(dead_code, reason = "mirrors the playground state table")]
    pub fn from_name(s: &str) -> Option<State> {
        Some(match s {
            "calm" => State::Calm,
            "incoming" => State::Incoming,
            "outgoing" => State::Outgoing,
            "connecting" => State::Connecting,
            "active" => State::Active,
            "waiting" => State::Waiting,
            "reconnecting" => State::Reconnecting,
            "ended" => State::Ended,
            "failed" => State::Failed,
            _ => return None,
        })
    }
    /// The cue pattern this state's notes come from (STATE_CUE in cues.ts).
    pub fn pattern(self) -> Option<Pattern> {
        match self {
            State::Incoming => Some(PAT_RING),
            State::Outgoing => Some(PAT_BACK),
            State::Active => Some(PAT_CONNECTED),
            State::Waiting => Some(PAT_WAITING),
            State::Reconnecting => Some(PAT_PULSE),
            State::Ended => Some(PAT_ENDED),
            State::Failed => Some(PAT_FAILED),
            State::Calm | State::Connecting => None,
        }
    }
    /// How long after the state change the cue's first note sounds (cues.ts): connect waits so its landing note
    /// meets the string going taut.
    pub fn cue_delay_ms(self, reduced: bool) -> f64 {
        match self {
            State::Active if !reduced => (MARK_HIT_MS - CONNECT_HIT_S * 1000.0).max(0.0),
            _ => 0.0,
        }
    }
    fn cfg(self) -> &'static Cfg {
        &STATES[self as usize]
    }
}

/// Eased fields, in this order: v, a0, warp, pull, spread, chroma, kappa, swell, ak.
const NE: usize = 9;
struct Cfg {
    cue: bool,
    looped: bool,
    e: [f64; NE],
    bias: f64,
    blend: f64,
    voice: bool,
    /// delay, dur (ms)
    fade: Option<(f64, f64)>,
    /// per-field time-constant overrides (ms), indexed like `e`
    tc: [f64; NE],
}
const NO: f64 = 0.0;
const fn tc(pairs: &[(usize, f64)]) -> [f64; NE] {
    let mut t = [NO; NE];
    let mut i = 0;
    while i < pairs.len() {
        t[pairs[i].0] = pairs[i].1;
        i += 1;
    }
    t
}
const WARP: usize = 2;
const PULL: usize = 3;
const SPREAD: usize = 4;
const CHROMA: usize = 5;
static STATES: [Cfg; 9] = [
    // calm
    Cfg { cue: false, looped: false, e: [0.50, 0.8, 0.08, 0.0, 0.0, 1.0, 1.0, 0.30, 0.45], bias: 0.35, blend: 900.0, voice: false, fade: None, tc: tc(&[]) },
    // incoming
    Cfg { cue: true, looped: true, e: [1.00, 0.9, 0.12, 0.0, 0.0, 1.0, 2.6, 0.38, 1.0], bias: 0.74, blend: 300.0, voice: false, fade: None, tc: tc(&[]) },
    // outgoing
    Cfg { cue: true, looped: true, e: [0.42, 0.85, 0.12, 0.0, 0.0, 1.0, 2.2, 0.38, 1.1], bias: 0.08, blend: 450.0, voice: false, fade: None, tc: tc(&[]) },
    // connecting
    Cfg { cue: false, looped: false, e: [0.80, 0.85, 0.012, 1.0, 0.0, 1.0, 1.6, 0.30, 0.7], bias: 0.12, blend: 350.0, voice: false, fade: None, tc: tc(&[(WARP, 330.0), (PULL, 520.0)]) },
    // active
    Cfg { cue: true, looped: false, e: [0.30, 0.6, 0.07, 0.0, 0.0, 1.0, 1.0, 0.30, 0.6], bias: 0.30, blend: 1200.0, voice: true, fade: None, tc: tc(&[(PULL, 1400.0)]) },
    // waiting
    Cfg { cue: true, looped: true, e: [0.34, 0.65, 0.07, 0.0, 0.0, 1.0, 1.2, 0.32, 0.8], bias: 0.52, blend: 900.0, voice: true, fade: None, tc: tc(&[]) },
    // reconnecting
    Cfg { cue: true, looped: false, e: [0.22, 0.55, 0.05, 0.5, 0.0, 0.45, 1.2, 0.30, 0.8], bias: 0.20, blend: 700.0, voice: false, fade: None, tc: tc(&[(PULL, 900.0), (CHROMA, 500.0)]) },
    // ended
    Cfg { cue: true, looped: false, e: [0.60, 0.8, 0.10, 0.0, 0.55, 1.0, 1.6, 0.30, 0.6], bias: 0.40, blend: 300.0, voice: false, fade: Some((0.0, 300.0)), tc: tc(&[(SPREAD, 320.0)]) },
    // failed
    Cfg { cue: true, looped: false, e: [0.30, 0.7, 0.05, 0.0, 0.0, 0.0, 1.2, 0.30, 0.6], bias: 0.20, blend: 500.0, voice: false, fade: Some((350.0, 650.0)), tc: tc(&[(CHROMA, 160.0)]) },
];

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Lobe {
    pub x: f64,
    pub y: f64,
    pub sigma: f64,
    pub alpha: f64,
    pub lab: [f64; 3],
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub tau: f64,
    pub warp: f64,
    pub cap: f64,
    pub bg: [f64; 3],
    pub lobes: [Lobe; K],
    /// False once an end/failed fade has finished: nothing moves any more.
    pub animating: bool,
    /// Reduced motion: when the next note lands (model ms), so a sleeping loop knows when to wake.
    pub wake_at: Option<f64>,
}

#[derive(Clone, Copy)]
struct Cur {
    id: u64,
    state: State,
    t0: f64,
    c0: f64,
    pat: Option<Pattern>,
}
struct Prev {
    st: Cur,
    at: f64,
    ms: f64,
}
struct Xf {
    t0: f64,
    from: ([f64; K], [f64; K], f64),
    to: ([f64; K], [f64; K], f64),
}

pub struct Field {
    theme: Theme,
    labs: Labs,
    reduced: bool,
    mute_g: f64,
    muted: bool,
    voice: Voice,
    /// w, h, avatar x, avatar y (px)
    view: [f64; 4],
    cur: Option<Cur>,
    prevs: Vec<Prev>,
    id: u64,
    s: Option<[f64; NE]>,
    lu: [f64; K],
    tu: [f64; K],
    key: [Option<(u64, i64)>; K],
    tau: f64,
    last_t: Option<f64>,
    fade_u: f64,
    has_frame: bool,
    xf: Option<Xf>,
    r_key: Option<(i64, usize)>,
    r_note_u: Option<f64>,
    disp: ([f64; K], [f64; K], f64),
    scratch: Vec<(Note, f64)>,
}

impl Field {
    pub fn new(theme: Theme, reduced: bool) -> Field {
        Field {
            theme,
            labs: labs(theme),
            reduced,
            mute_g: 1.0,
            muted: false,
            voice: Voice::new(7),
            view: [390.0, 844.0, 195.0, 178.0],
            cur: None,
            prevs: Vec::new(),
            id: 0,
            s: None,
            lu: [0.5; K],
            tu: [0.5; K],
            key: [None; K],
            tau: 0.0,
            last_t: None,
            fade_u: 1.0,
            has_frame: false,
            xf: None,
            r_key: None,
            r_note_u: None,
            disp: ([0.0; K], [0.5; K], 1.0),
            scratch: Vec::new(),
        }
    }

    pub fn state(&self) -> Option<State> {
        self.cur.map(|c| c.state)
    }
    pub fn set_view(&mut self, w: f64, h: f64, avx: f64, avy: f64) {
        self.view = [w, h, avx, avy];
    }
    pub fn set_mute(&mut self, m: bool) {
        self.muted = m;
    }
    pub fn set_theme(&mut self, t: Theme) {
        if t != self.theme {
            self.theme = t;
            self.labs = labs(t);
        }
    }
    pub fn set_reduced(&mut self, r: bool) {
        if r == self.reduced {
            return;
        }
        self.reduced = r;
        if r {
            self.r_key = None;
            self.r_note_u = None;
            self.xf = None;
        } else if let Some(c) = self.cur {
            self.lu = self.disp.1;
            self.s = Some(c.state.cfg().e);
        }
    }

    fn palette_u(cfg: &Cfg, i: usize) -> f64 {
        clamp(cfg.bias + (i as f64 - 2.0) * 0.07, 0.0, 1.0)
    }
    fn curve(&self, u: f64) -> f64 {
        let x = smooth(clamp((u - 0.2) / 0.75, 0.0, 1.0));
        if self.theme == Theme::Dark { smooth(smooth(x)) } else { x }
    }
    fn colour_at(&self, u: f64, chroma: f64) -> [f64; 3] {
        let c = mix_lab(self.labs.teal, self.labs.gold, self.curve(u));
        [c[0], c[1] * chroma, c[2] * chroma]
    }
    fn half_ext(&self) -> [f64; 2] {
        let s = self.view[0].min(self.view[1]);
        [self.view[0] / s / 2.0, self.view[1] / s / 2.0]
    }
    fn avatar_pt(&self) -> [f64; 2] {
        let s = self.view[0].min(self.view[1]);
        [(self.view[2] - self.view[0] / 2.0) / s, (self.view[3] - self.view[1] / 2.0) / s]
    }
    fn lobe_pos(&self, i: usize, tau: f64, h: [f64; 2], pull: f64, spread: f64) -> [f64; 2] {
        let j = LJ[i];
        let (sx, sy) = ((h[0] / 0.5).max(1.0), (h[1] / 0.5).max(1.0));
        let mut x = j[0] * h[0] + j[2] * sx * (j[4] * OMEGA * tau + j[6]).sin();
        let mut y = j[1] * h[1] + j[3] * sy * (j[5] * OMEGA * tau + j[7]).sin();
        if pull > 0.0 {
            let a = self.avatar_pt();
            let g = pull * PULL_AMT;
            x = lerp(x, a[0], g);
            y = lerp(y, a[1], g);
        }
        if spread != 0.0 {
            x *= 1.0 + spread;
            y *= 1.0 + spread;
        }
        [x, y]
    }

    /// `cue_start`: absolute model time (ms) of the cue's first note; None anchors it on the state change.
    pub fn set_state(&mut self, state: State, t: f64, cue_start: Option<f64>) {
        let cfg = state.cfg();
        if let Some(c) = self.cur {
            self.prevs.push(Prev { st: c, at: t, ms: 220.0 });
        }
        self.id += 1;
        self.cur = Some(Cur { id: self.id, state, t0: t, c0: cue_start.unwrap_or(t), pat: state.pattern() });
        if self.s.is_none() {
            self.s = Some(cfg.e);
        }
        for i in 0..K {
            self.tu[i] = Self::palette_u(cfg, i);
            self.key[i] = None;
            if !self.has_frame {
                self.lu[i] = self.tu[i];
            }
        }
        if self.last_t.is_none() {
            self.last_t = Some(t);
        }
        self.r_key = None;
        self.r_note_u = None;
        if self.reduced {
            self.start_xf(t);
        }
    }

    // ---- reduced motion: static gradient, per-note colour crossfade ----
    fn static_target(&self, note_u: Option<f64>) -> ([f64; K], [f64; K], f64) {
        let cfg = self.cur.unwrap().state.cfg();
        let a = if cfg.fade.is_some() { 0.0 } else { cfg.e[1] };
        let mut u = [0.0; K];
        for (i, ui) in u.iter_mut().enumerate() {
            let p = Self::palette_u(cfg, i);
            *ui = match note_u {
                None => p,
                Some(n) => lerp(p, n, 0.75),
            };
        }
        ([a; K], u, cfg.e[CHROMA])
    }
    fn start_xf(&mut self, t0: f64) {
        let to = self.static_target(self.r_note_u);
        self.xf = Some(Xf { t0, from: self.disp, to });
    }
    /// Latest onset <= t (time, (k, i), freq) and next onset > t of a cue started at c0.
    fn scan_onsets(p: Option<Pattern>, looped: bool, c0: f64, t: f64) -> (Option<(f64, (i64, usize), f64)>, Option<f64>) {
        let Some(p) = p else { return (None, None) };
        let per = p.period_ms;
        let lp = looped && per != 0.0;
        let lo = if lp { ((t - c0) / per).floor() as i64 } else { 0 };
        let (mut latest, mut next): (Option<(f64, (i64, usize), f64)>, Option<f64>) = (None, None);
        let hi = if lp { lo + 1 } else { 0 };
        for k in (lo - 1).max(0)..=hi {
            for (i, n) in p.notes.iter().enumerate() {
                let ot = c0 + k as f64 * per + n[1] * 1000.0;
                if ot <= t {
                    if latest.is_none_or(|l| ot >= l.0) {
                        latest = Some((ot, (k, i), n[0]));
                    }
                } else if next.is_none_or(|x| ot < x) {
                    next = Some(ot);
                }
            }
        }
        (latest, next)
    }
    fn reduced_frame(&mut self, t: f64) -> Frame {
        let cur = self.cur.unwrap();
        let cfg = cur.state.cfg();
        let (lt, next) = Self::scan_onsets(cur.pat, cfg.looped, cur.c0, t);
        if cfg.cue {
            if let Some(lt) = lt.filter(|l| cur.pat.is_some() && Some(l.1) != self.r_key) {
                self.r_key = Some(lt.1);
                self.r_note_u = Some(pitch_u(lt.2));
                self.start_xf(lt.0.max(t - XF_MS));
            }
        }
        if self.xf.is_none() {
            self.start_xf(t - XF_MS);
        }
        let xf = self.xf.as_ref().unwrap();
        let e = smooth((t - xf.t0) / XF_MS);
        let (f, g) = (xf.from, xf.to);
        let mut alpha = [0.0; K];
        let mut u = [0.0; K];
        for i in 0..K {
            alpha[i] = lerp(f.0[i], g.0[i], e);
            u[i] = lerp(f.1[i], g.1[i], e);
        }
        let chroma = lerp(f.2, g.2, e);
        self.disp = (alpha, u, chroma);
        let h = self.half_ext();
        let lobes = std::array::from_fn(|i| {
            let p = self.lobe_pos(i, 7.3, h, 0.0, 0.0);
            Lobe { x: p[0], y: p[1], sigma: SIGMA0, alpha: alpha[i], lab: self.colour_at(u[i], chroma) }
        });
        let animating = e < 1.0;
        Frame { tau: 7.3, warp: 0.0, cap: self.cap(), bg: self.labs.bg, lobes, animating, wake_at: if animating { None } else { next } }
    }
    fn cap(&self) -> f64 {
        if self.theme == Theme::Dark { CAP_DARK } else { CAP_LIGHT }
    }

    fn add_notes(&mut self, st: Cur, t: f64, k: f64, lraw: &mut [f64; K], mut mark: Option<&mut [Option<(i64, f64)>; K]>) {
        let cfg = st.state.cfg();
        let Some(pat) = st.pat else { return };
        if !cfg.cue || k <= 0.0 {
            return;
        }
        let mut notes = std::mem::take(&mut self.scratch);
        active_notes(&pat, t - st.c0, cfg.looped, &mut notes);
        for (n, s) in notes.iter() {
            let e = note_level(n, *s);
            let pk = n[3];
            let lv = pk * (e / pk).powf(GAMMA) * k;
            let uu = pitch_u(n[0]) * K as f64;
            for i in 0..K {
                let w = (1.0 - (uu - (i as f64 + 0.5)).abs() / SPILL).max(0.0);
                if w <= 0.0 {
                    continue;
                }
                lraw[i] += lv * w;
                if let Some(m) = mark.as_deref_mut() {
                    if w >= 0.3 {
                        let onset = js_round(t - s * 1000.0) as i64;
                        if m[i].is_none_or(|p| onset >= p.0) {
                            m[i] = Some((onset, n[0]));
                        }
                    }
                }
            }
        }
        self.scratch = notes;
    }

    /// Advance to model time `t` (ms) and return the frame to draw.
    pub fn update(&mut self, t: f64) -> Frame {
        let cur = self.cur.expect("set_state first");
        let dt = match self.last_t {
            None => 0.0,
            Some(l) => clamp(t - l, 0.0, DT_MAX),
        };
        self.last_t = Some(t);
        self.mute_g += ((if self.muted { 0.0 } else { 1.0 }) - self.mute_g) * (1.0 - (-dt / 120.0).exp());
        if self.reduced {
            let f = self.reduced_frame(t);
            self.has_frame = true;
            return f;
        }
        let cfg = cur.state.cfg();
        let s = self.s.get_or_insert(cfg.e);
        for k in 0..NE {
            let tc = (if cfg.tc[k] != NO { cfg.tc[k] } else { cfg.blend }) / 3.2;
            s[k] += (cfg.e[k] - s[k]) * (1.0 - (-dt / tc).exp());
        }
        let s = *s;
        let mut lraw = [0.0; K];
        let mut mark: [Option<(i64, f64)>; K] = [None; K];
        self.add_notes(cur, t, 1.0, &mut lraw, Some(&mut mark));
        self.prevs.retain(|p| t - p.at < p.ms);
        let prevs: Vec<(Cur, f64)> = self.prevs.iter().map(|p| (p.st, 1.0 - smooth((t - p.at) / p.ms))).collect();
        for (st, k) in prevs {
            self.add_notes(st, t, k, &mut lraw, None);
        }
        let l: [f64; K] = lraw.map(|x| (x / A_NORM).tanh());
        let e: f64 = l.iter().sum();
        let ctc = COLOUR_MS.max(cfg.blend / 4.0);
        for i in 0..K {
            if let Some(m) = mark[i] {
                let k2 = (cur.id, m.0);
                if self.key[i] != Some(k2) {
                    self.key[i] = Some(k2);
                    self.tu[i] = pitch_u(m.1);
                }
            }
            self.lu[i] += (self.tu[i] - self.lu[i]) * (1.0 - (-dt / ctc).exp());
        }
        let since = t - cur.t0;
        let vo = if cfg.voice { self.voice.at(since) * self.mute_g } else { 0.0 };
        // [v, a0, warp, pull, spread, chroma, kappa, swell, ak]
        self.tau += dt / 1000.0 * s[0] * (1.0 + s[6] * e + 0.5 * vo);
        let in_fade = cfg.fade.is_some_and(|(d, _)| since >= d);
        let step = match (in_fade, cfg.fade) {
            (true, Some((_, dur))) => -dt / dur,
            _ => dt / 400.0,
        };
        self.fade_u = clamp(self.fade_u + step, 0.0, 1.0);
        let fade = smooth(self.fade_u);
        let h = self.half_ext();
        let lobes: [Lobe; K] = std::array::from_fn(|i| {
            let p = self.lobe_pos(i, self.tau, h, s[PULL], s[SPREAD]);
            let sigma = SIGMA0 * (1.0 + s[7] * l[i] + 0.06 * vo) * (1.0 - 0.3 * s[PULL]);
            let alpha = (s[1] + 0.07 * vo + s[8] * l[i]) * fade;
            Lobe { x: p[0], y: p[1], sigma, alpha, lab: self.colour_at(self.lu[i], s[CHROMA]) }
        });
        self.disp = (lobes.map(|l| l.alpha), self.lu, s[CHROMA]);
        let animating = !(cfg.fade.is_some() && self.fade_u <= 0.0);
        self.has_frame = true;
        Frame { tau: self.tau, warp: s[WARP], cap: self.cap(), bg: self.labs.bg, lobes, animating, wake_at: None }
    }
}

// ---- drawing: iced's wgpu Shader widget ----------------------------------------------------------------------

use iced::widget::shader::{self, Pipeline, Primitive, Viewport};
use iced::{mouse, wgpu, window, Element, Length, Rectangle};
use std::time::Instant;

/// Both textures remain undecided. Only explicit test-hooks previews select a texture;
/// the production call surface does not choose a default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Texture {
    Fog,
    Clouds,
}

/// What the call screen wants the field to show. Layout points are in the widget's own logical px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Want {
    pub state: State,
    pub dark: bool,
    pub muted: bool,
    /// Avatar centre (x, y) inside the field.
    pub avatar: (f32, f32),
    /// Calm patches behind the text block and the controls: centre x, y, half width, half height (px).
    pub calm: [(f32, f32, f32, f32); 2],
}

/// Fog is the call screen's field in the app (as Android and the playground default); `P2P_FIELD=clouds|none` in
/// test-hooks builds compares the other texture or turns it off. With the software renderer (no wgpu) the shader draws
/// nothing and the plain background shows.
pub fn texture() -> Option<Texture> {
    match crate::test_env("P2P_FIELD").as_deref() {
        Some("clouds") => Some(Texture::Clouds),
        Some("none") => None,
        _ => Some(Texture::Fog),
    }
}

/// Dark mode: calm patches behind text (0 = none) and the chroma gain (CloudField.svelte SCRIM_DARK / GAIN_DARK).
const SCRIM_DARK: f32 = 0.45;
const GAIN_DARK: f32 = 1.55;

struct Prog {
    want: Want,
    texture: Texture,
}

pub struct Live {
    field: Option<Field>,
    applied: Option<Want>,
    origin: Instant,
    frame: Option<Frame>,
    sleep_until: Option<f64>,
    size: (f32, f32),
    reduced: bool,
}
impl Default for Live {
    fn default() -> Live {
        Live { field: None, applied: None, origin: Instant::now(), frame: None, sleep_until: None, size: (0.0, 0.0), reduced: false }
    }
}

impl<M> shader::Program<M> for Prog {
    type State = Live;
    type Primitive = Draw;

    fn update(&self, st: &mut Live, event: &iced::Event, bounds: Rectangle, _: mouse::Cursor) -> Option<shader::Action<M>> {
        let iced::Event::Window(window::Event::RedrawRequested(now)) = event else { return None };
        let t = now.saturating_duration_since(st.origin).as_secs_f64() * 1000.0;
        let w = self.want;
        let theme = if w.dark { Theme::Dark } else { Theme::Light };
        let reduced = crate::mark::reduced_motion();
        let f = st.field.get_or_insert_with(|| Field::new(theme, reduced));
        f.set_theme(theme);
        f.set_reduced(reduced);
        f.set_mute(w.muted);
        f.set_view(bounds.width as f64, bounds.height as f64, w.avatar.0 as f64, w.avatar.1 as f64);
        // A sleeping reduced-motion field must redraw after resizing or changing motion preference.
        let size = (bounds.width, bounds.height);
        let changed = st.applied != Some(w) || st.size != size || st.reduced != reduced;
        st.size = size;
        st.reduced = reduced;
        if f.state() != Some(w.state) {
            f.set_state(w.state, t, Some(t + w.state.cue_delay_ms(reduced)));
        }
        st.applied = Some(w);
        if !changed && st.sleep_until.is_some_and(|s| t < s) {
            return None;
        }
        let fr = f.update(t);
        st.frame = Some(fr);
        st.sleep_until = None;
        if fr.animating {
            return Some(shader::Action::request_redraw());
        }
        if let Some(at) = fr.wake_at {
            st.sleep_until = Some(at);
            let wait = std::time::Duration::from_secs_f64(((at - t) / 1000.0).max(0.0));
            return Some(shader::Action::request_redraw_at(*now + wait));
        }
        None
    }

    fn draw(&self, st: &Live, _: mouse::Cursor, bounds: Rectangle) -> Draw {
        let w = self.want;
        let fr = match st.frame {
            Some(f) => f,
            None => {
                let mut f = Field::new(if w.dark { Theme::Dark } else { Theme::Light }, crate::mark::reduced_motion());
                f.set_view(bounds.width as f64, bounds.height as f64, w.avatar.0 as f64, w.avatar.1 as f64);
                f.set_state(w.state, 0.0, None);
                f.update(0.0)
            }
        };
        let short = bounds.width.min(bounds.height).max(1.0);
        let mut d = [[0f32; 4]; 17];
        d[0] = [0.0, 0.0, fr.tau as f32, fr.warp as f32];
        d[1] = [fr.cap as f32, WARP_SCALE as f32, NOISE_RATE as f32, if self.texture == Texture::Clouds { 1.0 } else { 0.0 }];
        d[2] = [fr.bg[0] as f32, fr.bg[1] as f32, fr.bg[2] as f32, if w.dark { GAIN_DARK } else { 1.0 }];
        // uLit: .13 light, .075 dark (gl.ts); [3].z = sRGB target, set in prepare.
        d[3] = [if w.dark { SCRIM_DARK } else { 0.0 }, if fr.bg[0] > 0.5 { 0.13 } else { 0.075 }, 0.0, 0.0];
        // Calm patches (CloudField.svelte measureScrim): radii are (half size + 44 / 30) / .55, in short-side units.
        for (i, c) in w.calm.iter().enumerate() {
            d[4 + i] = [
                (c.0 - bounds.width / 2.0) / short,
                (c.1 - bounds.height / 2.0) / short,
                (c.2 + 44.0) / 0.55 / short,
                (c.3 + 30.0) / 0.55 / short,
            ];
        }
        for (i, l) in fr.lobes.iter().enumerate() {
            d[7 + i] = [l.x as f32, l.y as f32, l.sigma as f32, l.alpha as f32];
            d[12 + i] = [l.lab[0] as f32, l.lab[1] as f32, l.lab[2] as f32, 0.0];
        }
        Draw { d }
    }
}

#[derive(Debug)]
pub struct Draw {
    d: [[f32; 4]; 17],
}

pub struct Gpu {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    bind: wgpu::BindGroup,
    srgb: bool,
}

impl Pipeline for Gpu {
    fn new(device: &wgpu::Device, _: &wgpu::Queue, format: wgpu::TextureFormat) -> Gpu {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("call field"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!("call_field.wgsl"))),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("call field"),
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview: None,
            cache: None,
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("call field uniforms"),
            size: 17 * 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("call field uniforms"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
        });
        Gpu { pipeline, buffer, bind, srgb: format.is_srgb() }
    }
}

impl Primitive for Draw {
    type Pipeline = Gpu;

    fn prepare(&self, p: &mut Gpu, _: &wgpu::Device, queue: &wgpu::Queue, bounds: &Rectangle, viewport: &Viewport) {
        let mut d = self.d;
        // Physical size and origin of the widget, for gl_FragCoord-style pixel maths.
        let sc = viewport.scale_factor() as f32;
        d[0][0] = bounds.width * sc;
        d[0][1] = bounds.height * sc;
        d[3][2] = if p.srgb { 1.0 } else { 0.0 };
        d[6] = [bounds.x * sc, bounds.y * sc, 0.0, 0.0];
        let mut bytes = [0u8; 17 * 16];
        for (i, v) in d.iter().flatten().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_ne_bytes());
        }
        queue.write_buffer(&p.buffer, 0, &bytes);
    }

    fn draw(&self, p: &Gpu, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pass.set_pipeline(&p.pipeline);
        pass.set_bind_group(0, &p.bind, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}

/// The field, filling its parent. Animates itself (redraw requests), sleeps when nothing moves.
pub fn field<'a, M: 'a>(want: Want, texture: Texture) -> Element<'a, M> {
    iced::widget::shader(Prog { want, texture }).width(Length::Fill).height(Length::Fill).into()
}

#[cfg(test)]
#[path = "call_field_tests.rs"]
mod tests;
