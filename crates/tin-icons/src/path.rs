//! A checked Bézier path on kurbo, and the one text form every platform reads.
//!
//! The checks follow osvauld2's `runtime::frame::Path` (bounded command count, finite coordinates, every
//! drawing command inside an open subpath). The parser takes the SVG path syntax the designs are written in,
//! but turns elliptical arcs into cubics with a tight tolerance (kurbo's own `from_svg` uses 0.1 units, which
//! shows at construction size), so the output only ever holds M, L, Q, C and Z: exactly what Compose
//! `PathParser`, iced/resvg and the browser draw identically (docs: playground/src/lib/geom/GEOMETRY.md).

use kurbo::{Arc, BezPath, PathEl, Point, Rect, Shape, SvgArc, Vec2};

pub const MAX_COMMANDS: usize = 4096;
pub const MAX_COORDINATE: f64 = 10_000.0;
/// Arc → cubic tolerance in grid units (1/24 of an icon): invisible even at the 120 px construction view.
pub const ARC_TOLERANCE: f64 = 0.002;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PathError {
    #[error("path has {count} commands; maximum is {MAX_COMMANDS}")]
    TooManyCommands { count: usize },
    #[error("path command {index} ({command}) needs an open subpath")]
    InvalidSequence { index: usize, command: &'static str },
    #[error("path command {index} has a non-finite or out-of-range coordinate")]
    InvalidCoordinate { index: usize },
    #[error("can't read path data at byte {at}: {what}")]
    Parse { at: usize, what: &'static str },
}

/// A validated path with its bounds.
#[derive(Clone, Debug)]
pub struct Path {
    bez: BezPath,
    bounds: Rect,
}

impl Path {
    pub fn new(elements: Vec<PathEl>) -> Result<Self, PathError> {
        if elements.len() > MAX_COMMANDS {
            return Err(PathError::TooManyCommands { count: elements.len() });
        }
        let mut open = false;
        for (index, el) in elements.iter().enumerate() {
            let (command, points): (&'static str, &[Point]) = match el {
                PathEl::MoveTo(p) => ("move", std::slice::from_ref(p)),
                PathEl::LineTo(p) => ("line", std::slice::from_ref(p)),
                PathEl::QuadTo(a, b) => ("quad", &[*a, *b]),
                PathEl::CurveTo(a, b, c) => ("cubic", &[*a, *b, *c]),
                PathEl::ClosePath => ("close", &[]),
            };
            if !points.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.x.abs() <= MAX_COORDINATE && p.y.abs() <= MAX_COORDINATE) {
                return Err(PathError::InvalidCoordinate { index });
            }
            match command {
                "move" => open = true,
                _ if !open => return Err(PathError::InvalidSequence { index, command }),
                "close" => open = false,
                _ => {}
            }
        }
        let bez = BezPath::from_vec(elements);
        let bounds = bez.bounding_box();
        Ok(Self { bez, bounds })
    }

    /// Parses SVG path data (M L H V Q T C S A Z, absolute and relative).
    pub fn svg(d: &str) -> Result<Self, PathError> {
        Self::new(parse(d)?)
    }

    pub fn bezier(&self) -> &BezPath {
        &self.bez
    }

    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    pub fn transformed(&self, a: kurbo::Affine) -> Self {
        let mut bez = self.bez.clone();
        bez.apply_affine(a);
        let bounds = bez.bounding_box();
        Self { bez, bounds }
    }

    /// The canonical text form: absolute M/L/Q/C/Z, coordinates rounded to 0.01, single spaces
    /// (the same format as shape.ts `roundedRectPath`, so the two can be compared byte for byte).
    pub fn d(&self) -> String {
        let mut s = String::new();
        let pt = |s: &mut String, p: Point| {
            s.push_str(&num(p.x));
            s.push(' ');
            s.push_str(&num(p.y));
        };
        for el in self.bez.elements() {
            match *el {
                PathEl::MoveTo(p) => { s.push('M'); pt(&mut s, p) }
                PathEl::LineTo(p) => { s.push('L'); pt(&mut s, p) }
                PathEl::QuadTo(a, b) => { s.push('Q'); pt(&mut s, a); s.push(' '); pt(&mut s, b) }
                PathEl::CurveTo(a, b, c) => {
                    s.push('C'); pt(&mut s, a); s.push(' '); pt(&mut s, b); s.push(' '); pt(&mut s, c)
                }
                PathEl::ClosePath => s.push('Z'),
            }
        }
        s
    }
}

/// Rounds to 0.01 and prints the shortest form ("2.5", "3", never "-0").
pub fn num(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0 + 0.0;
    format!("{r}")
}

fn parse(d: &str) -> Result<Vec<PathEl>, PathError> {
    let mut lx = Lexer { b: d.as_bytes(), i: 0 };
    let mut out = Vec::new();
    let (mut cur, mut start) = (Point::ZERO, Point::ZERO);
    let mut prev: Option<(u8, Point)> = None; // last control point and its kind, reflected by S/T
    let mut cmd = 0u8;
    loop {
        lx.skip();
        if lx.i >= lx.b.len() {
            break;
        }
        let c = lx.b[lx.i];
        if c.is_ascii_alphabetic() {
            cmd = c;
            lx.i += 1;
        } else if cmd == 0 {
            return Err(PathError::Parse { at: lx.i, what: "expected a command" });
        } else if cmd == b'M' {
            cmd = b'L'; // implicit lineto after the first moveto pair
        } else if cmd == b'm' {
            cmd = b'l';
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { cur.to_vec2() } else { Vec2::ZERO };
        let up = cmd.to_ascii_uppercase();
        let mut next = None;
        match up {
            b'M' => { cur = lx.pt()? + base; start = cur; out.push(PathEl::MoveTo(cur)) }
            b'L' => { cur = lx.pt()? + base; out.push(PathEl::LineTo(cur)) }
            b'H' => { cur = Point::new(lx.n()? + base.x, cur.y); out.push(PathEl::LineTo(cur)) }
            b'V' => { cur = Point::new(cur.x, lx.n()? + base.y); out.push(PathEl::LineTo(cur)) }
            b'Q' | b'T' => {
                let a = if up == b'Q' { lx.pt()? + base } else { reflect(prev, b'Q', cur) };
                cur = lx.pt()? + base;
                out.push(PathEl::QuadTo(a, cur));
                next = Some((b'Q', a));
            }
            b'C' | b'S' => {
                let a = if up == b'C' { lx.pt()? + base } else { reflect(prev, b'C', cur) };
                let b = lx.pt()? + base;
                cur = lx.pt()? + base;
                out.push(PathEl::CurveTo(a, b, cur));
                next = Some((b'C', b));
            }
            b'A' => {
                let (rx, ry, rot) = (lx.n()?, lx.n()?, lx.n()?);
                let (large_arc, sweep) = (lx.flag()?, lx.flag()?);
                let to = lx.pt()? + base;
                let arc = SvgArc { from: cur, to, radii: Vec2::new(rx, ry), x_rotation: rot.to_radians(), large_arc, sweep };
                match Arc::from_svg_arc(&arc) {
                    Some(a) => a.to_cubic_beziers(ARC_TOLERANCE, |p1, p2, p3| out.push(PathEl::CurveTo(p1, p2, p3))),
                    None => out.push(PathEl::LineTo(to)),
                }
                cur = to;
            }
            b'Z' => { out.push(PathEl::ClosePath); cur = start; cmd = 0 }
            _ => return Err(PathError::Parse { at: lx.i, what: "unknown command" }),
        }
        prev = next;
    }
    Ok(out)
}

fn reflect(prev: Option<(u8, Point)>, kind: u8, cur: Point) -> Point {
    match prev {
        Some((k, l)) if k == kind => cur + (cur - l),
        _ => cur,
    }
}

struct Lexer<'a> {
    b: &'a [u8],
    i: usize,
}

impl Lexer<'_> {
    fn skip(&mut self) {
        while self.i < self.b.len() && (self.b[self.i].is_ascii_whitespace() || self.b[self.i] == b',') {
            self.i += 1;
        }
    }
    fn n(&mut self) -> Result<f64, PathError> {
        self.skip();
        let s = self.i;
        let mut j = s;
        if j < self.b.len() && matches!(self.b[j], b'-' | b'+') {
            j += 1;
        }
        let (mut dot, mut digits) = (false, false);
        while j < self.b.len() {
            match self.b[j] {
                b'0'..=b'9' => digits = true,
                b'.' if !dot => dot = true,
                b'e' | b'E' if digits => {
                    j += 1;
                    if j < self.b.len() && matches!(self.b[j], b'-' | b'+') {
                        j += 1;
                    }
                    continue;
                }
                _ => break,
            }
            j += 1;
        }
        if !digits {
            return Err(PathError::Parse { at: s, what: "expected a number" });
        }
        self.i = j;
        std::str::from_utf8(&self.b[s..j]).ok().and_then(|t| t.parse().ok()).ok_or(PathError::Parse { at: s, what: "bad number" })
    }
    fn pt(&mut self) -> Result<Point, PathError> {
        Ok(Point::new(self.n()?, self.n()?))
    }
    /// Arc flags may be written without separators ("0 1 1" or "011").
    fn flag(&mut self) -> Result<bool, PathError> {
        self.skip();
        match self.b.get(self.i) {
            Some(b'0') => { self.i += 1; Ok(false) }
            Some(b'1') => { self.i += 1; Ok(true) }
            _ => Err(PathError::Parse { at: self.i, what: "expected an arc flag" }),
        }
    }
}
