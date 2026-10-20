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
