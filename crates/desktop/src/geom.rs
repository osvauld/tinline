//! Smooth rounded outlines: a port of docs/design/playground/src/lib/geom/shape.ts (construction in
//! GEOMETRY.md). Lines and cubic Béziers only; Figma-style corner smoothing; per-corner radii.
//! `outline` is pure and testable; shaped.rs rasterises it.

use iced::Point;

/// Corner smoothing token (`--smooth`).
pub const SMOOTH: f32 = 0.6;
/// Bubble outer and stack-facing corner radii (`--r-bub`, `--r-bub-tight`).
pub const R_BUB: f32 = 18.0;
pub const R_BUB_TIGHT: f32 = 6.0;

/// Physical radii: top-left, top-right, bottom-right, bottom-left.
pub type Radii = [f32; 4];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Close,
}

/// Position in a run of messages from the same sender.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pos {
    Solo,
    First,
    Mid,
    Last,
}

impl Pos {
    /// Same rule as Conversation.svelte: `a` = joined with the previous message, `b` = with the next.
    pub fn of(joined_prev: bool, joined_next: bool) -> Pos {
        match (joined_prev, joined_next) {
            (true, true) => Pos::Mid,
            (true, false) => Pos::Last,
            (false, true) => Pos::First,
            (false, false) => Pos::Solo,
        }
    }
}

/// Messages from one sender this close together form a run.
pub const GROUP_GAP_MS: u64 = 5 * 60_000;

/// Position of item `i` in a run, where `join(a, b)` says two neighbours belong to the same run.
pub fn run_pos<T>(items: &[T], i: usize, join: impl Fn(&T, &T) -> bool) -> Pos {
    let prev = i > 0 && join(&items[i - 1], &items[i]);
    let next = items.get(i + 1).is_some_and(|n| join(&items[i], n));
    Pos::of(prev, next)
}

/// Space above a bubble: 10 px between runs, 2 px inside one (Conversation.svelte).
pub fn gap_above(pos: Pos) -> f32 {
    if matches!(pos, Pos::Mid | Pos::Last) { 2.0 } else { 10.0 }
}

/// Which side of the bubble faces the screen edge it sits against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

/// Bubble corner radii. My messages sit against the right edge, theirs against the left.
pub fn bubble_radii(pos: Pos, side: Side, outer: f32, tight: f32) -> Radii {
    let top = if matches!(pos, Pos::Mid | Pos::Last) { tight } else { outer };
    let bot = if matches!(pos, Pos::Mid | Pos::First) { tight } else { outer };
    match side {
        Side::Right => [outer, top, bot, outer],
        Side::Left => [top, outer, outer, bot],
    }
}

/// 0.01 rounding used by shape.ts to decide whether a straight segment has length.
fn r2(n: f64) -> f64 {
    (n * 100.0).round() / 100.0 + 0.0
}

#[derive(Clone, Copy, Default)]
struct Corner {
    r: f64,
    p: f64,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    arc: f64,
    alpha: f64,
    h: f64,
}

fn corner_params(radius: f64, smoothing: f64, budget: f64) -> Corner {
    let mut r = radius.max(0.0);
    let mut s = smoothing.clamp(0.0, 1.0);
    if r == 0.0 {
        return Corner::default();
    }
    if (1.0 + s) * r > budget {
        s = s.min((budget / r - 1.0).max(0.0));
        if r > budget {
            r = budget;
            s = 0.0;
        }
    }
    let p = (1.0 + s) * r;
    let arc_measure = 90.0 * (1.0 - s);
    let rad = std::f64::consts::PI / 180.0;
    let arc = ((arc_measure / 2.0) * rad).sin() * r * std::f64::consts::SQRT_2;
    let alpha = (45.0 * s) * rad;
    let p3p4 = r * (((90.0 - arc_measure) / 4.0) * rad).tan();
    let c = p3p4 * alpha.cos();
    let d = c * alpha.tan();
    let b = (p - arc - c - d) / 3.0;
    let a = 2.0 * b;
    let h = (4.0 / 3.0) * ((arc_measure / 4.0) * rad).tan() * r;
    Corner { r, p, a, b, c, d, arc, alpha, h }
}

fn budgets(w: f64, h: f64, [tl, tr, br, bl]: [f64; 4], s: f64) -> [f64; 4] {
    let ext = |r: f64| (1.0 + s) * r;
    let side = |len: f64, ra: f64, rb: f64| -> (f64, f64) {
        let need = ext(ra) + ext(rb);
        if need <= len || need == 0.0 {
            (f64::INFINITY, f64::INFINITY)
        } else {
            (len * ra / (ra + rb), len * rb / (ra + rb))
        }
    };
    let top = side(w, tl, tr);
    let right = side(h, tr, br);
    let bottom = side(w, br, bl);
    let left = side(h, bl, tl);
    [top.0.min(left.1), top.1.min(right.0), right.1.min(bottom.0), bottom.1.min(left.0)]
}

/// One corner as cubics. `s` is the start on the incoming side, `din` / `dout` the unit directions.
fn corner(out: &mut Vec<Seg>, s: (f64, f64), din: (f64, f64), dout: (f64, f64), k: &Corner) -> (f64, f64) {
    let pt = |u: f64, v: f64| Point::new((s.0 + din.0 * u + dout.0 * v) as f32, (s.1 + din.1 * u + dout.1 * v) as f32);
    let Corner { a, b, c, d, arc, alpha, h, p, .. } = *k;
    let e1 = (a + b + c, d);
    let e2 = (e1.0 + arc, e1.1 + arc);
    if a + b + c + d > 1e-9 {
        out.push(Seg::Cubic(pt(a, 0.0), pt(a + b, 0.0), pt(e1.0, e1.1)));
    }
    if arc > 1e-9 {
        let (ca, sa) = (alpha.cos(), alpha.sin());
        out.push(Seg::Cubic(pt(e1.0 + h * ca, e1.1 + h * sa), pt(e2.0 - h * sa, e2.1 - h * ca), pt(e2.0, e2.1)));
    }
    if a + b + c + d > 1e-9 {
        out.push(Seg::Cubic(pt(e2.0 + d, e2.1 + c), pt(e2.0 + d, e2.1 + b + c), pt(p, p)));
    }
    (s.0 + din.0 * p + dout.0 * p, s.1 + din.1 * p + dout.1 * p)
}

/// The outline of a w×h box with its top-left at (x, y), clockwise from the top edge.
pub fn outline(w: f32, h: f32, radii: Radii, smoothing: f32, x: f32, y: f32) -> Vec<Seg> {
    let (w, h, x, y) = (w.max(0.0) as f64, h.max(0.0) as f64, x as f64, y as f64);
    let s = (smoothing as f64).clamp(0.0, 1.0);
    let rr = radii.map(|r| r as f64);
    let bud = budgets(w, h, rr, s);
    let k: Vec<Corner> = (0..4).map(|i| corner_params(rr[i].min(w / 2.0).min(h / 2.0), s, bud[i])).collect();
    let (tl, tr, br, bl) = (k[0], k[1], k[2], k[3]);
    let pf = |px: f64, py: f64| Point::new(px as f32, py as f32);
    let mut out = vec![Seg::Move(pf(x + tl.p, y))];
    let mut cur = (x + tl.p, y);
    let line = |out: &mut Vec<Seg>, cur: &mut (f64, f64), lx: f64, ly: f64| {
        if r2(lx) != r2(cur.0) || r2(ly) != r2(cur.1) {
            out.push(Seg::Line(pf(lx, ly)));
        }
        *cur = (lx, ly);
    };
    line(&mut out, &mut cur, x + w - tr.p, y);
    if tr.r != 0.0 {
        cur = corner(&mut out, (x + w - tr.p, y), (1.0, 0.0), (0.0, 1.0), &tr);
    }
    line(&mut out, &mut cur, x + w, y + h - br.p);
    if br.r != 0.0 {
        cur = corner(&mut out, (x + w, y + h - br.p), (0.0, 1.0), (-1.0, 0.0), &br);
    }
    line(&mut out, &mut cur, x + bl.p, y + h);
    if bl.r != 0.0 {
        cur = corner(&mut out, (x + bl.p, y + h), (-1.0, 0.0), (0.0, -1.0), &bl);
    }
    line(&mut out, &mut cur, x, y + tl.p);
    if tl.r != 0.0 {
        corner(&mut out, (x, y + tl.p), (0.0, -1.0), (1.0, 0.0), &tl);
    }
    out.push(Seg::Close);
    out
}

/// The same shape shrunk by `inset` px on every side (negative = grown), concentric with the original;
/// coordinates stay in the original box's space.
pub fn inset_outline(w: f32, h: f32, radii: Radii, smoothing: f32, inset: f32) -> Vec<Seg> {
    outline(w - 2.0 * inset, h - 2.0 * inset, radii.map(|r| (r - inset).max(0.0)), smoothing, inset, inset)
}

#[cfg(test)]
#[path = "geom_tests.rs"]
mod tests;
