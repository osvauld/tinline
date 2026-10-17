//! End, Decline, Leave (`hangup`): two of the mark's strings crossed as a swirl X, one cut where the other crosses it
//! (chosen 2026-10-16 in the playground's `?lab=endcall`, EndGlyph.svelte `xcut`; the two must stay in step).
//!
//! A string from A to B with sag s along the unit normal n is the parabola
//! `P(u) = A + u·(B − A) + 4·s·u·(1 − u)·n`, u in [0, 1]: a quadratic Bézier with control `M + 2·s·n` (M = the chord's
//! midpoint). The whole strand runs top-left to bottom-right; the cut strand is the same strand turned 90° about the
//! centre (top-right to bottom-left), so they cross where, with h = half the X, a = 2u − 1 (whole), b = 2v − 1 (cut)
//! and c = √2·h/s,
//!
//! ```text
//! 1 − b² = c·a,  1 − a² = −c·b   ⇒   (a² − 1)² = c²·(1 − c·a)
//! ```
//!
//! which Newton's method solves from a = 0 in a few steps. The cut is the whole strand's stroke plus a clearance on
//! each side, measured across the cut strand, so it looks the same at any crossing angle θ: its half-length along
//! the cut strand is `(stroke/2 + clear) / sin θ`, turned into u by the strand's speed there. Each piece left is a
//! piece of the same parabola (its control point is the blossom of the two cut parameters).
//!
//! Pressing End (`hangup-go`, the apps morph to it): the cut strand goes slack (sag 2.4 → 4.2) and the clearance opens
//! (2.6 → 3.8), so its halves fall back from the cut; the whole strand stays where it is.

use crate::icons::STROKE;

/// The X's corners: (X, X) to (X + W, X + W).
const X: f64 = 5.4;
const W: f64 = 24.0 - 2.0 * X;
/// The whole strand's sag, and the cut strand's at rest.
pub const SAG: f64 = 2.4;
/// The cut strand once End is pressed (slack), and the clearances at rest / pressed.
pub const SAG_GO: f64 = 4.2;
pub const CLEAR: f64 = 2.6;
pub const CLEAR_GO: f64 = 3.8;

type Pt = [f64; 2];

/// A string as a quadratic Bézier: ends `a`, `b`, control `c`.
#[derive(Clone, Copy, Debug)]
pub struct Strand {
    pub a: Pt,
    pub c: Pt,
    pub b: Pt,
}

impl Strand {
    /// From A to B, sagging `sag` along the normal (−dy, dx)/w turned by `side` (+1 or −1).
    pub fn new(a: Pt, b: Pt, sag: f64, side: f64) -> Self {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let w = dx.hypot(dy);
        let n = [-dy / w * side, dx / w * side];
        Strand { a, b, c: [(a[0] + b[0]) / 2.0 + 2.0 * sag * n[0], (a[1] + b[1]) / 2.0 + 2.0 * sag * n[1]] }
    }
    pub fn at(&self, u: f64) -> Pt {
        let f = |k: usize| (1.0 - u).powi(2) * self.a[k] + 2.0 * u * (1.0 - u) * self.c[k] + u * u * self.b[k];
        [f(0), f(1)]
    }
    pub fn tangent(&self, u: f64) -> Pt {
        let f = |k: usize| 2.0 * (1.0 - u) * (self.c[k] - self.a[k]) + 2.0 * u * (self.b[k] - self.c[k]);
        [f(0), f(1)]
    }
    /// The piece u0..u1, itself a parabola: ends P(u0), P(u1), control the blossom (1−u0)(1−u1)A + ((1−u0)u1 + u0(1−u1))C + u0·u1·B.
    pub fn piece(&self, u0: f64, u1: f64) -> Strand {
        let f = |k: usize| (1.0 - u0) * (1.0 - u1) * self.a[k] + ((1.0 - u0) * u1 + u0 * (1.0 - u1)) * self.c[k] + u0 * u1 * self.b[k];
        Strand { a: self.at(u0), c: [f(0), f(1)], b: self.at(u1) }
    }
    pub fn d(&self) -> String {
        let r = |v: f64| crate::path::num((v * 100.0).round() / 100.0);
        format!("M{} {}Q{} {} {} {}", r(self.a[0]), r(self.a[1]), r(self.c[0]), r(self.c[1]), r(self.b[0]), r(self.b[1]))
    }
}

/// The whole strand (top-left to bottom-right).
pub fn whole() -> Strand {
    Strand::new([X, X], [X + W, X + W], SAG, -1.0)
}

/// The cut strand before the cut, with sag `sag` (top-right to bottom-left).
pub fn crossed(sag: f64) -> Strand {
    Strand::new([X + W, X], [X, X + W], sag, -1.0)
}

/// Where the strands cross: (u on the whole strand, v on the cut strand), from the quartic above.
pub fn crossing() -> (f64, f64) {
    let c = std::f64::consts::SQRT_2 * (W / 2.0) / SAG;
    let mut a = 0.0_f64;
    for _ in 0..8 {
        a -= ((a * a - 1.0).powi(2) - c * c + c.powi(3) * a) / (4.0 * a * (a * a - 1.0) + c.powi(3));
    }
    ((1.0 + a) / 2.0, (1.0 + (a * a - 1.0) / c) / 2.0)
}

/// The glyph: the whole strand and the two pieces of the cut one, at rest or once End is pressed.
pub fn glyph(pressed: bool) -> [Strand; 3] {
    let (sag, clear) = if pressed { (SAG_GO, CLEAR_GO) } else { (SAG, CLEAR) };
    let (u, v) = crossing();
    let (w, rest) = (whole(), crossed(SAG));
    let (tw, tc) = (w.tangent(u), rest.tangent(v));
    let speed = tc[0].hypot(tc[1]);
    let sin = (tw[0] * tc[1] - tw[1] * tc[0]).abs() / (tw[0].hypot(tw[1]) * speed);
    let du = (STROKE / 2.0 + clear) / sin / speed;
    let cut = crossed(sag);
    [w, cut.piece(0.0, v - du), cut.piece(v + du, 1.0)]
}
