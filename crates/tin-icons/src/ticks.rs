//! Receipt ticks drawn as the mark's string: the user's pick from the playground lab `?lab=ticks`
//! (branch lab/ticks, `screens/lab/tick-math.ts` + `TickGlyph.svelte`), preset "crisp", the STRING
//! variant at every size (not the hybrid Euler corner).
//!
//! MARK-MATH.md §2: one quadratic Bézier `M K1 Q C K2`, `C = (K1 + K2)/2 + 2s·(sin ψ, cos ψ)`, so the curve's
//! midpoint sags by exactly `s` in direction ψ. A tick is that string with uneven anchors and the sag swung
//! toward the short arm. Waiting is the same string slack between level ends. Every state is the same
//! N-point polyline (identical commands), so a state change is a true morph: `lerp(SLACK, TICK, k)`.
//!
//! The morph parameter over time comes from the mark's own spring (tinmark.js / desktop `mark.rs`, default
//! parameters), sampled here once so iced and Compose play back the same numbers:
//! - pull (slack → tick, and the second string pulling out of the first): `k = 1 − s/s0`;
//! - pluck (delivered → read): `k = 1 − 2s/s0` around the taut tick.
//!
//! Coordinates: the 24-unit icon grid, y down. Slot: viewBox x from `VB_X`, width `VB_W`, height 24.

pub type Pt = [f64; 2];

/// Lab `TickP`, string-variant fields only (plus the small-variant offset, which sets the slot width).
#[derive(Debug, Clone, Copy)]
pub struct TickP {
    pub k1: Pt,
    pub k2: Pt,
    /// Sag at the midpoint (units).
    pub s: f64,
    /// Swing ψ in degrees (0 = straight down, negative = toward the short arm).
    pub psi: f64,
    /// Stroke width (units of the 24 grid).
    pub sw: f64,
    /// Second tick offset (units).
    pub dx: f64,
    /// Over-under gap where the second string crosses the first (units).
    pub gap: f64,
    /// The lab's small-size offset: only used for the slot width (`20 + max(dx, dx_small)`).
    pub dx_small: f64,
}

/// The lab's `PRESETS.crisp` string, tuned on 2026-10-16 after the 11–12 px review (the user's go-ahead): the lab
/// values (K1 (6,12), sag 7.5, ψ −35°, dx 5) read as "//" with a small hook at 12 px, because the curve left K1
/// heading down-LEFT. Now the short arm is longer (K1 up and left), the control point sits below the corner so the
/// string leaves K1 down-right and turns tightly (sag 9.1, ψ −21°), and the second tick sits 6 units over (the slot
/// is unchanged: 20 + max(dx, 6) = 26). Still one quadratic string, no Euler corner.
pub const CRISP: TickP = TickP { k1: [4.5, 11.5], k2: [20.0, 4.5], s: 9.1, psi: -21.0, sw: 2.1, dx: 6.0, gap: 1.0, dx_small: 6.0 };
/// Points per string; every state uses exactly this many.
pub const N: usize = 40;
/// Rest sag of the mark's slack string (MARK-MATH §2).
pub const S0: f64 = 3.5;
/// Slot: viewBox x origin and width (24 tall).
pub const VB_X: f64 = 2.0;
pub fn vb_w(p: &TickP) -> f64 {
    20.0 + p.dx.max(p.dx_small)
}
/// Spring samples per second in `PULL` / `PLUCK`.
pub const FRAME_HZ: f64 = 120.0;

fn quad(a: Pt, c: Pt, b: Pt, n: usize) -> Vec<Pt> {
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            [u * u * a[0] + 2.0 * t * u * c[0] + t * t * b[0], u * u * a[1] + 2.0 * t * u * c[1] + t * t * b[1]]
        })
        .collect()
}

/// tick-math.ts `resample`: n points evenly spaced along the polyline's length.
pub fn resample(pts: &[Pt], n: usize) -> Vec<Pt> {
    let mut acc = vec![0.0];
    for i in 1..pts.len() {
        acc.push(acc[i - 1] + (pts[i][0] - pts[i - 1][0]).hypot(pts[i][1] - pts[i - 1][1]));
    }
    let l = *acc.last().unwrap_or(&0.0);
    let mut out = Vec::with_capacity(n);
    let mut j = 1;
    for k in 0..n {
        let d = l * k as f64 / (n - 1) as f64;
        while j < pts.len() - 1 && acc[j] < d {
            j += 1;
        }
        let t = if acc[j] == acc[j - 1] { 0.0 } else { (d - acc[j - 1]) / (acc[j] - acc[j - 1]) };
        out.push([pts[j - 1][0] + (pts[j][0] - pts[j - 1][0]) * t, pts[j - 1][1] + (pts[j][1] - pts[j - 1][1]) * t]);
    }
    out
}

/// Control point C for the tick (tick-math.ts `ctrl`).
pub fn ctrl(p: &TickP) -> Pt {
    let (mx, my) = ((p.k1[0] + p.k2[0]) / 2.0, (p.k1[1] + p.k2[1]) / 2.0);
    let r = p.psi.to_radians();
    [mx + 2.0 * p.s * r.sin(), my + 2.0 * p.s * r.cos()]
}

/// The taut tick (tick-math.ts `stringPts`).
pub fn tick_pts(p: &TickP) -> Vec<Pt> {
    resample(&quad(p.k1, ctrl(p), p.k2, 64), N)
}

/// The slack string between level ends (tick-math.ts `slackPts(tick, level = 1)`): from the short arm's end
/// to the point level with it under the long arm's end, hanging straight down with the mark's rest sag
/// scaled to the span (`S0 · w / 9 · 0.7`).
pub fn slack_pts(tick: &[Pt]) -> Vec<Pt> {
    let a = tick[0];
    let t = tick[tick.len() - 1];
    let b = [t[0], a[1]];
    let s = S0 * (b[0] - a[0]).hypot(b[1] - a[1]) / 9.0 * 0.7;
    resample(&quad(a, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0 + 2.0 * s], b, 64), N)
}

pub fn lerp(a: &[Pt], b: &[Pt], k: f64) -> Vec<Pt> {
    a.iter().zip(b).map(|(p, q)| [p[0] + (q[0] - p[0]) * k, p[1] + (q[1] - p[1]) * k]).collect()
}

// ---- the mark's spring (tinmark.js DEF; same model as crates/desktop/src/mark.rs) ------------------------------
const C: f64 = 7.5;
const TWANG_A: f64 = 0.5;
const PLUCK_A: f64 = 0.32;
const TWANG_F: f64 = 7.0;
const TWANG_TAU: f64 = 0.11;
const CONNECT_HZ: f64 = 3.0;
const HIT_OVER: f64 = 1.6;
const RECOIL: f64 = 0.5;
const RECOIL_MIN: f64 = 1.2;
const SETTLE_HZ: f64 = 2.0;
const SUBSTEP: f64 = 1.0 / 480.0;

fn len_exact(w: f64, s: f64) -> f64 {
    let s = s.abs();
    if s < 1e-9 {
        return w;
    }
    0.5 * (w * w + 16.0 * s * s).sqrt() + (w * w / (8.0 * s)) * (4.0 * s / w).asinh()
}

fn sag_of_w(w: f64, ell: f64) -> f64 {
    if w >= ell {
        return 0.0;
    }
    let sa = (3.0 * w * (ell - w) / 8.0).sqrt();
    let (mut lo, mut hi) = (0.0, sa.max(1e-6) * 1.5);
    while len_exact(w, hi) < ell {
        hi *= 2.0;
    }
    for _ in 0..40 {
        let m = (lo + hi) / 2.0;
        if len_exact(w, m) < ell {
            lo = m;
        } else {
            hi = m;
        }
    }
    (lo + hi) / 2.0
}

/// The connect / pluck part of the mark model (no sway, no windup).
struct Spring {
    ell: f64,
    dt: f64,
    d: f64,
    v: f64,
    hit: bool,
    tw: f64,
    tw_a: f64,
    v0: f64,
}

impl Spring {
    fn new(taut: bool) -> Spring {
        let w0 = 24.0 - 2.0 * C;
        let ell = len_exact(w0, S0);
        let dt = ((ell - w0) / 2.0).max(0.0);
        Spring { ell, dt, d: if taut { dt } else { 0.0 }, v: 0.0, hit: taut, tw: -1.0, tw_a: 0.0, v0: 0.0 }
    }
    fn amp(&self) -> f64 {
        if self.tw_a != 0.0 { self.tw_a } else { TWANG_A }
    }
    fn step(&mut self, h: f64) {
        let tau = 2.0 * std::f64::consts::PI;
        let (z, w, tgt) = if self.hit { (0.0, tau * SETTLE_HZ, self.dt) } else { (1.0, tau * CONNECT_HZ, self.dt * HIT_OVER) };
        let a = -2.0 * z * w * self.v - w * w * (self.d - tgt);
        self.v += a * h;
        self.d += self.v * h;
        if self.d >= self.dt && self.v <= 0.0 {
            self.d = self.dt;
        } else if self.d >= self.dt {
            let v = self.v;
            self.d = self.dt;
            self.v = if v * RECOIL >= RECOIL_MIN { -v * RECOIL } else { 0.0 };
            if !self.hit {
                self.hit = true;
                self.tw = 0.0;
                self.tw_a = 0.0;
            } else if v > RECOIL_MIN {
                self.tw = 0.0;
                let v0 = if self.v0 > 0.0 { self.v0 } else { v };
                self.tw_a = TWANG_A.min(TWANG_A * v / v0) * 0.6;
            }
            if self.v0 == 0.0 {
                self.v0 = v;
            }
        }
        if self.tw >= 0.0 {
            self.tw += h;
            if self.amp() * (-self.tw / TWANG_TAU).exp() < 0.03 {
                self.tw = -1.0;
            }
        }
    }
    fn active(&self) -> bool {
        self.tw >= 0.0 || self.d < self.dt || self.v != 0.0
    }
    fn sag(&self) -> f64 {
        let twang = if self.tw < 0.0 { 0.0 } else { self.amp() * (-self.tw / TWANG_TAU).exp() * (2.0 * std::f64::consts::PI * TWANG_F * self.tw).sin() };
        sag_of_w(24.0 - 2.0 * (C - self.d), self.ell) + twang
    }
    /// Frames at `FRAME_HZ` until settled (bounded at 4 s), mapped by `k`; the last sample is the rest value.
    fn run(mut self, k: impl Fn(f64) -> f64, rest: f64) -> Vec<f64> {
        let frame = 1.0 / FRAME_HZ;
        let n = (frame / SUBSTEP).ceil() as usize;
        let mut out = Vec::new();
        for _ in 0..(4.0 * FRAME_HZ) as usize {
            for _ in 0..n {
                self.step(frame / n as f64);
            }
            if !self.active() {
                break;
            }
            out.push(k(self.sag()));
        }
        out.push(rest);
        out
    }
}

/// Slack → taut morph parameter per frame (lab: `1 − s/s0`; > 1 while the twang overshoots).
pub fn pull_curve() -> Vec<f64> {
    let mut sp = Spring::new(false);
    sp.hit = false;
    sp.run(|s| 1.0 - s / S0, 1.0)
}

/// One quiet pluck of the taut string (lab: `1 − 2s/s0`), for delivered → read.
pub fn pluck_curve() -> Vec<f64> {
    let mut sp = Spring::new(true);
    sp.tw_a = PLUCK_A;
    sp.tw = 0.0;
    sp.run(|s| 1.0 - 2.0 * s / S0, 1.0)
}

#[cfg(test)]
mod tests;
