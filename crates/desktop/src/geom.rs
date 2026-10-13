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
mod tests {
    use super::*;

    /// Serialises like shape.ts (rounded to 0.01) so it can be compared with the node output.
    fn svg(segs: &[Seg]) -> String {
        let f = |n: f32| {
            let r = ((n as f64) * 100.0).round() / 100.0 + 0.0;
            format!("{r}")
        };
        let p = |p: &Point| format!("{} {}", f(p.x), f(p.y));
        segs.iter()
            .map(|s| match s {
                Seg::Move(a) => format!("M{}", p(a)),
                Seg::Line(a) => format!("L{}", p(a)),
                Seg::Cubic(a, b, c) => format!("C{} {} {}", p(a), p(b), p(c)),
                Seg::Close => "Z".into(),
            })
            .collect()
    }

    fn nums(s: &str) -> Vec<f64> {
        s.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|t| !t.is_empty()).map(|t| t.parse().unwrap()).collect()
    }

    fn same(got: &str, want: &str) {
        let (g, w) = (nums(got), nums(want));
        assert_eq!(g.len(), w.len(), "\n got {got}\nwant {want}");
        for (a, b) in g.iter().zip(&w) {
            assert!((a - b).abs() <= 0.011, "{a} vs {b}\n got {got}\nwant {want}");
        }
        let cmds = |s: &str| s.chars().filter(|c| c.is_ascii_alphabetic()).collect::<String>();
        assert_eq!(cmds(got), cmds(want));
    }

    const R: Radii = [18.0, 18.0, 6.0, 18.0];

    // GEOMETRY.md sample outputs.
    #[test]
    fn doc_samples() {
        same(&svg(&outline(120.0, 40.0, R, 0.6, 0.0, 0.0)), "M20 0L91.2 0C101.28 0 106.32 0 110.17 1.96C113.56 3.69 116.31 6.44 118.04 9.83C120 13.68 120 18.72 120 28.8L120 30.4C120 33.76 120 35.44 119.35 36.72C118.77 37.85 117.85 38.77 116.72 39.35C115.44 40 113.76 40 110.4 40L20 40C18.14 40 17.21 40 16.43 39.93C7.73 39.17 0.83 32.27 0.07 23.57C0 22.79 0 21.86 0 20C0 18.14 0 17.21 0.07 16.43C0.83 7.73 7.73 0.83 16.43 0.07C17.21 0 18.14 0 20 0Z");
        same(&svg(&outline(120.0, 60.0, R, 0.6, 0.0, 0.0)), "M28.8 0L91.2 0C101.28 0 106.32 0 110.17 1.96C113.56 3.69 116.31 6.44 118.04 9.83C120 13.68 120 18.72 120 28.8L120 50.4C120 53.76 120 55.44 119.35 56.72C118.77 57.85 117.85 58.77 116.72 59.35C115.44 60 113.76 60 110.4 60L28.8 60C18.72 60 13.68 60 9.83 58.04C6.44 56.31 3.69 53.56 1.96 50.17C0 46.32 0 41.28 0 31.2L0 28.8C0 18.72 0 13.68 1.96 9.83C3.69 6.44 6.44 3.69 9.83 1.96C13.68 0 18.72 0 28.8 0Z");
        same(&svg(&outline(120.0, 60.0, R, 0.0, 0.0, 0.0)), "M18 0L102 0C111.94 0 120 8.06 120 18L120 54C120 57.31 117.31 60 114 60L18 60C8.06 60 0 51.94 0 42L0 18C0 8.06 8.06 0 18 0Z");
        same(&svg(&inset_outline(120.0, 60.0, R, 0.6, -3.0)), "M30 -3L89.4 -3C101.16 -3 107.04 -3 111.53 -0.71C115.49 1.3 118.7 4.51 120.71 8.47C123 12.96 123 18.84 123 30.6L123 48.6C123 53.64 123 56.16 122.02 58.09C121.16 59.78 119.78 61.16 118.09 62.02C116.16 63 113.64 63 108.6 63L30 63C18.8 63 13.21 63 8.89 60.92C4.6 58.86 1.14 55.4 -0.92 51.11C-3 46.79 -3 41.2 -3 30C-3 18.8 -3 13.21 -0.92 8.89C1.14 4.6 4.6 1.14 8.89 -0.92C13.21 -3 18.8 -3 30 -3Z");
    }

    // Values produced by running shape.ts (node) on other sizes: see fixtures/shape_cases.txt.
    #[test]
    fn matches_shape_ts() {
        let mut n = 0;
        for line in include_str!("../fixtures/shape_cases.txt").lines().filter(|l| !l.is_empty()) {
            let (head, d) = line.split_once(' ').unwrap();
            let v: Vec<f32> = head.split(',').map(|t| t.parse().unwrap()).collect();
            let segs = outline(v[0], v[1], [v[2], v[3], v[4], v[5]], v[6], 0.0, 0.0);
            same(&svg(&segs), d);
            n += 1;
        }
        assert!(n >= 8);
    }

    #[test]
    fn one_line_bubble_is_a_pill() {
        // 37 px high, r 18: smoothing degrades to almost nothing, ends are (nearly) semicircles.
        let segs = outline(160.0, 37.0, [18.0; 4], 0.6, 0.0, 0.0);
        let Seg::Move(m) = segs[0] else { panic!() };
        assert!((m.x - 18.5).abs() < 0.6, "start x {}", m.x);
    }

    #[test]
    fn grouping_radii() {
        use Pos::*;
        use Side::*;
        assert_eq!(bubble_radii(Solo, Right, 18.0, 6.0), [18.0, 18.0, 18.0, 18.0]);
        assert_eq!(bubble_radii(First, Right, 18.0, 6.0), [18.0, 18.0, 6.0, 18.0]);
        assert_eq!(bubble_radii(Mid, Right, 18.0, 6.0), [18.0, 6.0, 6.0, 18.0]);
        assert_eq!(bubble_radii(Last, Right, 18.0, 6.0), [18.0, 6.0, 18.0, 18.0]);
        assert_eq!(bubble_radii(First, Left, 18.0, 6.0), [18.0, 18.0, 18.0, 6.0]);
        assert_eq!(bubble_radii(Mid, Left, 18.0, 6.0), [6.0, 18.0, 18.0, 6.0]);
        assert_eq!(bubble_radii(Last, Left, 18.0, 6.0), [6.0, 18.0, 18.0, 18.0]);
        assert_eq!(Pos::of(true, true), Mid);
    }

    #[test]
    fn sharp_box_has_no_curves() {
        let segs = outline(10.0, 10.0, [0.0; 4], 0.6, 0.0, 0.0);
        assert!(segs.iter().all(|s| !matches!(s, Seg::Cubic(..))));
    }
}
