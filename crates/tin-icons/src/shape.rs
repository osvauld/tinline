//! Smoothed rounded rectangles: a line-for-line port of playground `src/lib/geom/shape.ts` (`roundedRectPath`).
//! The construction is written up in GEOMETRY.md. Output is lines and cubics only; tests compare it with the
//! TypeScript output byte for byte (tests/fixtures/shape.txt).

use kurbo::{PathEl, Point};

use crate::path::{Path, PathError};

/// Physical corner radii [top-left, top-right, bottom-right, bottom-left].
pub type Radii = [f64; 4];

/// The app's default corner smoothing (tokens.css `--smooth`).
pub const SMOOTH: f64 = 0.6;

#[derive(Clone, Copy, Debug, Default)]
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
        s = s.min((budget / r - 1.0).max(0.0)); // 1. degrade smoothing first
        if r > budget {
            r = budget; // 2. then the radius itself
            s = 0.0;
        }
    }
    let p = (1.0 + s) * r;
    let arc_measure = 90.0 * (1.0 - s);
    let arc = (arc_measure / 2.0).to_radians().sin() * r * std::f64::consts::SQRT_2;
    let alpha = (45.0 * s).to_radians();
    let p3p4 = r * ((90.0 - arc_measure) / 4.0).to_radians().tan();
    let c = p3p4 * alpha.cos();
    let d = c * alpha.tan();
    let b = (p - arc - c - d) / 3.0;
    let a = 2.0 * b;
    let h = (4.0 / 3.0) * (arc_measure / 4.0).to_radians().tan() * r;
    Corner { r, p, a, b, c, d, arc, alpha, h }
}

fn budgets(w: f64, h: f64, [tl, tr, br, bl]: Radii, s: f64) -> [f64; 4] {
    let ext = |r: f64| (1.0 + s) * r;
    let side = |len: f64, ra: f64, rb: f64| -> (f64, f64) {
        let need = ext(ra) + ext(rb);
        if need <= len || need == 0.0 {
            (f64::INFINITY, f64::INFINITY)
        } else {
            (len * ra / (ra + rb), len * rb / (ra + rb))
        }
    };
    let (top, right, bottom, left) = (side(w, tl, tr), side(h, tr, br), side(w, br, bl), side(h, bl, tl));
    [top.0.min(left.1), top.1.min(right.0), right.1.min(bottom.0), bottom.1.min(left.0)]
}

/// Values the TypeScript rounds to 0.01 before comparing; keep the same decisions here.
fn r2(n: f64) -> f64 {
    (n * 100.0).round() / 100.0
}

fn corner(out: &mut Vec<PathEl>, s: Point, din: (f64, f64), dout: (f64, f64), k: &Corner) -> Point {
    let pt = |u: f64, v: f64| Point::new(s.x + din.0 * u + dout.0 * v, s.y + din.1 * u + dout.1 * v);
    let e1 = (k.a + k.b + k.c, k.d);
    let e2 = (e1.0 + k.arc, e1.1 + k.arc);
    let transition = k.a + k.b + k.c + k.d > 1e-9;
    if transition {
        out.push(PathEl::CurveTo(pt(k.a, 0.0), pt(k.a + k.b, 0.0), pt(e1.0, e1.1)));
    }
    if k.arc > 1e-9 {
        let (ca, sa) = (k.alpha.cos(), k.alpha.sin());
        out.push(PathEl::CurveTo(pt(e1.0 + k.h * ca, e1.1 + k.h * sa), pt(e2.0 - k.h * sa, e2.1 - k.h * ca), pt(e2.0, e2.1)));
    }
    if transition {
        out.push(PathEl::CurveTo(pt(e2.0 + k.d, e2.1 + k.c), pt(e2.0 + k.d, e2.1 + k.b + k.c), pt(k.p, k.p)));
    }
    pt(k.p, k.p)
}

/// The outline of a w×h box with its top-left at (x, y), clockwise from the top edge.
pub fn rounded_rect(x: f64, y: f64, w: f64, h: f64, radii: Radii, smoothing: f64) -> Result<Path, PathError> {
    let (w, h) = (w.max(0.0), h.max(0.0));
    let s = smoothing.clamp(0.0, 1.0);
    let bud = budgets(w, h, radii, s);
    let k: Vec<Corner> = (0..4).map(|i| corner_params(radii[i].min(w / 2.0).min(h / 2.0), smoothing, bud[i])).collect();
    let (tl, tr, br, bl) = (&k[0], &k[1], &k[2], &k[3]);
    let mut out = vec![PathEl::MoveTo(Point::new(x + tl.p, y))];
    let mut cur = Point::new(x + tl.p, y);
    let line = |out: &mut Vec<PathEl>, cur: &mut Point, lx: f64, ly: f64| {
        if r2(lx) != r2(cur.x) || r2(ly) != r2(cur.y) {
            out.push(PathEl::LineTo(Point::new(lx, ly)));
        }
        *cur = Point::new(lx, ly);
    };
    line(&mut out, &mut cur, x + w - tr.p, y);
    if tr.r > 0.0 { cur = corner(&mut out, Point::new(x + w - tr.p, y), (1.0, 0.0), (0.0, 1.0), tr) }
    line(&mut out, &mut cur, x + w, y + h - br.p);
    if br.r > 0.0 { cur = corner(&mut out, Point::new(x + w, y + h - br.p), (0.0, 1.0), (-1.0, 0.0), br) }
    line(&mut out, &mut cur, x + bl.p, y + h);
    if bl.r > 0.0 { cur = corner(&mut out, Point::new(x + bl.p, y + h), (-1.0, 0.0), (0.0, -1.0), bl) }
    line(&mut out, &mut cur, x, y + tl.p);
    if tl.r > 0.0 { corner(&mut out, Point::new(x, y + tl.p), (0.0, -1.0), (1.0, 0.0), tl); }
    out.push(PathEl::ClosePath);
    Path::new(out)
}

/// Uniform radius, app smoothing: what icons use.
pub fn rr(x: f64, y: f64, w: f64, h: f64, r: f64) -> Path {
    rounded_rect(x, y, w, h, [r; 4], SMOOTH).expect("finite rounded rect")
}
