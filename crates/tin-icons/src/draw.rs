//! The mark's construction (playground `src/lib/brand/MARK-MATH.md`), reduced to icon pieces on the 24 grid.
//! Builders return SVG path data; `Path::svg` turns it into checked lines and cubics.

use crate::path::num as n;

/// sin θ for the can view angle θ = 30.6°: a rim of radius b is an ellipse with horizontal semi-axis b·sin θ.
pub fn view() -> f64 {
    30.6_f64.to_radians().sin()
}

/// Rim of a can lying on its side (mouth faces left or right): ellipse (b·sinθ, b) at (cx, cy).
pub fn rim_v(cx: f64, cy: f64, b: f64) -> String {
    let a = b * view();
    format!("M{} {}A{} {} 0 1 1 {} {}A{} {} 0 1 1 {} {}", n(cx), n(cy - b), n(a), n(b), n(cx), n(cy + b), n(a), n(b), n(cx), n(cy - b))
}

/// A can on its side: rim at cx, body of length `len` behind it, back cap of the same ellipse.
/// `dir` +1 = mouth faces right, -1 = faces left. (For a mirrored pair, draw one and mirror it.)
pub fn can_side(cx: f64, cy: f64, b: f64, len: f64, dir: f64) -> String {
    let a = b * view();
    let bx = cx - dir * len;
    let sweep = if dir > 0.0 { 0 } else { 1 };
    format!(
        "{}M{} {}H{}A{} {} 0 0 {} {} {}H{}",
        rim_v(cx, cy, b), n(cx), n(cy - b), n(bx), n(a), n(b), sweep, n(bx), n(cy + b), n(cx)
    )
}

/// Rim of a standing can (mouth up).
pub fn rim_h(cx: f64, cy: f64, b: f64) -> String {
    let a = b * view();
    format!("M{} {}A{} {} 0 1 1 {} {}A{} {} 0 1 1 {} {}", n(cx - b), n(cy), n(b), n(a), n(cx + b), n(cy), n(b), n(a), n(cx - b), n(cy))
}

/// A standing can: rim at cy, body down `len`, rounded bottom.
pub fn can_up(cx: f64, cy: f64, b: f64, len: f64) -> String {
    let a = b * view();
    format!(
        "{}M{} {}V{}A{} {} 0 0 0 {} {}V{}",
        rim_h(cx, cy, b), n(cx - b), n(cy), n(cy + len), n(b), n(a), n(cx + b), n(cy + len), n(cy)
    )
}

/// The mouth: the rim scaled by `mu`, concentric, filled. Without it a rim reads as a ring, not a can.
pub fn mouth_v(cx: f64, cy: f64, b: f64, mu: f64) -> String {
    let (a, bb) = (b * view() * mu, b * mu);
    format!("M{} {}A{} {} 0 1 1 {} {}A{} {} 0 1 1 {} {}Z", n(cx), n(cy - bb), n(a), n(bb), n(cx), n(cy + bb), n(a), n(bb), n(cx), n(cy - bb))
}

pub fn mouth_h(cx: f64, cy: f64, b: f64, mu: f64) -> String {
    let (a, bb) = (b * view() * mu, b * mu);
    format!("M{} {}A{} {} 0 1 1 {} {}A{} {} 0 1 1 {} {}Z", n(cx - bb), n(cy), n(bb), n(a), n(cx + bb), n(cy), n(bb), n(a), n(cx - bb), n(cy))
}

/// The string: one quadratic Bézier with its control 2s below the chord's midpoint, so the midpoint sags
/// exactly s (MARK-MATH §2). s = 0 is taut and is still written as Q, like the mark.
pub fn sag(x1: f64, y1: f64, x2: f64, y2: f64, s: f64) -> String {
    format!("M{} {}Q{} {} {} {}", n(x1), n(y1), n((x1 + x2) / 2.0), n((y1 + y2) / 2.0 + 2.0 * s), n(x2), n(y2))
}

/// Arrowhead at the tip; (dx, dy) is the direction of travel, so the head sits on the curve's real tangent.
pub fn head(tx: f64, ty: f64, dx: f64, dy: f64, len: f64, deg: f64) -> String {
    let m = dx.hypot(dy);
    let (ux, uy, r) = (dx / m, dy / m, deg.to_radians());
    let c = r.cos();
    let p = |s: f64| {
        let sn = (s * r).sin();
        (tx - len * (ux * c - uy * sn), ty - len * (uy * c + ux * sn))
    };
    let (a, b) = (p(1.0), p(-1.0));
    format!("M{} {}L{} {}L{} {}", n(a.0), n(a.1), n(tx), n(ty), n(b.0), n(b.1))
}

/// A quadratic curve ending in an arrowhead on its end tangent.
pub fn q_arrow(x1: f64, y1: f64, cx: f64, cy: f64, x2: f64, y2: f64, len: f64) -> String {
    format!("M{} {}Q{} {} {} {}{}", n(x1), n(y1), n(cx), n(cy), n(x2), n(y2), head(x2, y2, x2 - cx, y2 - cy, len, 40.0))
}

/// A clockwise circular arc from a0 to a1 degrees (screen space), with its head at a1.
pub fn arc_arrow(cx: f64, cy: f64, r: f64, a0: f64, a1: f64, len: f64) -> String {
    let p = |a: f64| (cx + r * a.to_radians().cos(), cy + r * a.to_radians().sin());
    let ((x0, y0), (x1, y1), t) = (p(a0), p(a1), a1.to_radians());
    let large = if (a1 - a0) % 360.0 > 180.0 { 1 } else { 0 };
    format!("M{} {}A{} {} 0 {} 1 {} {}{}", n(x0), n(y0), n(r), n(r), large, n(x1), n(y1), head(x1, y1, -t.sin(), t.cos(), len, 40.0))
}

pub fn circle(cx: f64, cy: f64, r: f64) -> String {
    format!("M{} {}A{} {} 0 1 0 {} {}A{} {} 0 1 0 {} {}Z", n(cx - r), n(cy), n(r), n(r), n(cx + r), n(cy), n(r), n(r), n(cx - r), n(cy))
}
