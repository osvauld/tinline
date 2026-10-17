//! End's glyph, drawn so it can move (crates/tin-icons/src/cut.rs; playground EndGlyph.svelte `xcut`): two strings
//! crossed as a swirl X, one cut where the other crosses it. Pressing End morphs `hangup` into `hangup-go` (the cut
//! string goes slack, its halves fall back from the cut) once, in the switch time with the playground's overshoot.
//! Both shapes are the same three quadratics, so each number moves in a straight line (as the CSS `d` transition does).

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use iced::mouse;
use iced::widget::canvas::{self, Canvas, LineCap, LineJoin, Path, Stroke};
use iced::{Color, Element, Point, Rectangle, Renderer, Theme};

/// The motion (playground `--dur-switch`).
pub const MOTION: Duration = Duration::from_millis(120);
/// How long the call screen stays after my own End, so the glyph is seen.
pub const HOLD: Duration = Duration::from_millis(320);

/// Each shape as its quadratics: [x0, y0, cx, cy, x1, y1].
fn shapes() -> &'static (Vec<[f32; 6]>, Vec<[f32; 6]>) {
    static S: OnceLock<(Vec<[f32; 6]>, Vec<[f32; 6]>)> = OnceLock::new();
    S.get_or_init(|| {
        let quads = |g: &crate::tin_icons::Glyph| -> Vec<[f32; 6]> {
            let d = g.string.split("d=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("");
            let n: Vec<f32> = d.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter_map(|s| s.parse().ok()).collect();
            n.chunks_exact(6).map(|c| [c[0], c[1], c[2], c[3], c[4], c[5]]).collect()
        };
        let (rest, go) = (quads(&crate::tin_icons::HANGUP[2]), quads(&crate::tin_icons::HANGUP_GO[2]));
        debug_assert_eq!(rest.len(), go.len(), "hangup and hangup-go differ in shape");
        (rest, go)
    })
}

/// The playground's give: cubic-bezier(.34, 1.4, .64, 1), solved for x by bisection.
fn give(t: f32) -> f32 {
    let bez = |a: f32, b: f32, s: f32| 3.0 * (1.0 - s) * (1.0 - s) * s * a + 3.0 * (1.0 - s) * s * s * b + s * s * s;
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let m = (lo + hi) / 2.0;
        if bez(0.34, 0.64, m) < t { lo = m } else { hi = m }
    }
    bez(1.4, 1.0, (lo + hi) / 2.0)
}

/// How far the glyph is into its motion: 0 = at rest, 1 = let go; None while nothing was pressed.
pub fn progress(pressed_at: Option<Instant>, reduced: bool) -> f32 {
    match pressed_at {
        None => 0.0,
        Some(_) if reduced => 1.0,
        Some(t) => give((t.elapsed().as_secs_f32() / MOTION.as_secs_f32()).min(1.0)),
    }
}

/// Still moving: the window should keep sending frames.
pub fn running(pressed_at: Option<Instant>) -> bool {
    pressed_at.is_some_and(|t| t.elapsed() < MOTION + Duration::from_millis(40))
}

struct Glyph {
    p: f32,
    color: Color,
}

impl<M> canvas::Program<M> for Glyph {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let k = bounds.width / 24.0;
        let (rest, go) = shapes();
        let path = Path::new(|b| {
            for (r, g) in rest.iter().zip(go) {
                let v = |i: usize| (r[i] + (g[i] - r[i]) * self.p) * k;
                b.move_to(Point::new(v(0), v(1)));
                b.quadratic_curve_to(Point::new(v(2), v(3)), Point::new(v(4), v(5)));
            }
        });
        // 1.75: the icon set's stroke (crates/tin-icons STROKE).
        frame.stroke(&path, Stroke::default().with_color(self.color).with_width(1.75 * k)
            .with_line_cap(LineCap::Round).with_line_join(LineJoin::Round));
        vec![frame.into_geometry()]
    }
}

/// The glyph at `size` px in `color`, `p` into its motion.
pub fn view<'a, M: 'a>(p: f32, size: f32, color: Color) -> Element<'a, M> {
    Canvas::new(Glyph { p, color }).width(size).height(size).into()
}
