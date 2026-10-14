//! Playground call icons as native Iced vector paths, not low-resolution raster glyphs.
//! Uses the already-present usvg parser once for exact SVG arc/rect-to-Bezier conversion.
use iced::{
    Color, Point, Radians, Rectangle, Renderer, Theme, Vector, mouse,
    widget::canvas::{self, LineCap, LineJoin, Path, Stroke},
};
use std::sync::LazyLock;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Mic,
    MicOff,
    Hangup,
    Speaker,
    SpeakerOn,
}
// Verbatim from design-refinement's lib/ui/icons.ts. Hangup is phone rotated 135 degrees.
const MIC: &str =
    r#"<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>"#;
const MIC_OFF: &str = r#"<path d="M15 9.3V6a3 3 0 0 0-5.7-1.3M9 9v2a3 3 0 0 0 4.6 2.5M5 11a7 7 0 0 0 11.5 5.4M19 11a7 7 0 0 1-.6 2.8M12 18v3M3 3l18 18"/>"#;
const PHONE: &str = r#"<path d="M5 4h3.5l1.5 4.5-2.2 1.4a11 11 0 0 0 6.3 6.3l1.4-2.2L20 15.5V19a1.5 1.5 0 0 1-1.6 1.5A16 16 0 0 1 3.5 5.6 1.5 1.5 0 0 1 5 4z"/>"#;
const SPEAKER: &str = r#"<path d="M4 9v6h4l5 4V5L8 9zM16.5 9a4 4 0 0 1 0 6"/>"#;
fn convert(body: &str) -> Vec<Path> {
    use resvg::tiny_skia::PathSegment;
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.75">{body}</svg>"##
    );
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).expect("fixed call icon SVG");
    fn walk(group: &usvg::Group, out: &mut Vec<Path>) {
        for node in group.children() {
            match node {
                usvg::Node::Group(g) => walk(g, out),
                usvg::Node::Path(p) => out.push(Path::new(|b| {
                    for s in p.data().segments() {
                        match s {
                            PathSegment::MoveTo(p) => b.move_to(Point::new(p.x, p.y)),
                            PathSegment::LineTo(p) => b.line_to(Point::new(p.x, p.y)),
                            PathSegment::QuadTo(c, p) => {
                                b.quadratic_curve_to(Point::new(c.x, c.y), Point::new(p.x, p.y))
                            }
                            PathSegment::CubicTo(a, c, p) => b.bezier_curve_to(
                                Point::new(a.x, a.y),
                                Point::new(c.x, c.y),
                                Point::new(p.x, p.y),
                            ),
                            PathSegment::Close => b.close(),
                        }
                    }
                })),
                _ => {}
            }
        }
    }
    let mut paths = Vec::new();
    walk(tree.root(), &mut paths);
    paths
}
static PATHS: LazyLock<[Vec<Path>; 4]> = LazyLock::new(|| {
    [
        convert(MIC),
        convert(MIC_OFF),
        convert(PHONE),
        convert(SPEAKER),
    ]
});
pub struct Glyph {
    pub kind: Kind,
    pub colour: Color,
}
impl<M> canvas::Program<M> for Glyph {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        f.translate(Vector::new(bounds.width / 2., bounds.height / 2.));
        f.scale(bounds.width.min(bounds.height) / 24.);
        if self.kind == Kind::Hangup {
            f.rotate(Radians(135_f32.to_radians()));
        }
        f.translate(Vector::new(-12., -12.));
        let index = match self.kind {
            Kind::Mic => 0,
            Kind::MicOff => 1,
            Kind::Hangup => 2,
            Kind::Speaker | Kind::SpeakerOn => 3,
        };
        let stroke = Stroke::default()
            .with_color(self.colour)
            .with_width(1.75)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round);
        for path in &PATHS[index] {
            f.stroke(path, stroke);
        }
        vec![f.into_geometry()]
    }
}
