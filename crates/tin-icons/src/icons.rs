//! The Tinline line icon set. One definition per icon, evaluated per size tier.
//!
//! Rules (study: playground `icons.html`):
//! - 24 grid, stroke 1.75, round caps and joins; fills only for can mouths and dots.
//! - Anything about talking (calls, audio, camera, links, chat, people, the bell) is drawn from the mark:
//!   rims at the mark's view angle, the string as one sagging quadratic, mirrored pairs drawn once.
//! - Plain controls (back, close, chevrons, check, search, trash…) stay plain, with the app's smoothed corners.
//! - Warning and shield are kept only until the screens stop using them (sign-up's mellow mood retires them).

use kurbo::Affine;

use crate::draw::*;
use crate::path::Path;
use crate::shape::rr;

pub const STROKE: f64 = 1.75;
pub const GRID: f64 = 24.0;

/// Size tier. Like the mark (MARK-MATH §4), small sizes are tuned rather than scaled: the can mouth shrinks
/// so 16–20 px cans don't fill in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    S16,
    S20,
    S24,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::S16, Tier::S20, Tier::S24];
    pub fn px(self) -> u32 {
        match self {
            Tier::S16 => 16,
            Tier::S20 => 20,
            Tier::S24 => 24,
        }
    }
    /// Mouth size relative to the rim.
    fn mu(self) -> f64 {
        match self {
            Tier::S16 => 0.34,
            Tier::S20 => 0.42,
            Tier::S24 => 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Stroked outline.
    Line,
    /// Filled (can mouths, dots).
    Fill,
    /// The string: stroked, and the one part a UI may colour separately (gold, like the logo).
    String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Drawn from the mark's geometry.
    Mark,
    /// Already Tinline's own glyph before this set (Android `Glyphs`).
    Ours,
    /// A plain control, kept.
    Kept,
    /// Still emitted while screens use it; to be replaced (see `note`).
    Retire,
}

#[derive(Clone, Debug)]
pub struct Part {
    pub role: Role,
    pub path: Path,
}

#[derive(Clone, Debug)]
pub struct Icon {
    pub name: &'static str,
    pub kind: Kind,
    pub note: &'static str,
    pub parts: Vec<Part>,
}

fn p(role: Role, d: impl AsRef<str>) -> Part {
    let d = d.as_ref();
    Part { role, path: Path::svg(d).unwrap_or_else(|e| panic!("bad icon path {d:?}: {e}")) }
}
fn line(d: impl AsRef<str>) -> Part { p(Role::Line, d) }
fn fill(d: impl AsRef<str>) -> Part { p(Role::Fill, d) }
fn string(d: impl AsRef<str>) -> Part { p(Role::String, d) }
fn shape(path: Path) -> Part { Part { role: Role::Line, path } }

/// x ↦ 24 − x: the right can of a pair is always the mirror of the left (MARK-MATH §1).
fn mirrored(parts: &[Part]) -> Vec<Part> {
    let m = Affine::new([-1.0, 0.0, 0.0, 1.0, GRID, 0.0]);
    parts.iter().map(|q| Part { role: q.role, path: q.path.transformed(m) }).collect()
}

const CAN_B: f64 = 4.4;
const CAN_L: f64 = 3.6;
const OFF: &str = "M3.5 3.5L20.5 20.5";

fn can_pair(cx: f64, cy: f64, mu: f64) -> Vec<Part> {
    let left = vec![line(can_side(cx, cy, CAN_B, CAN_L, 1.0)), fill(mouth_v(cx, cy, CAN_B, mu))];
    let mut v = mirrored(&left);
    v.splice(0..0, left);
    v
}

fn mic(mu: f64) -> Vec<Part> {
    vec![line(can_up(12.0, 4.9, 3.5, 6.4)), fill(mouth_h(12.0, 4.9, 3.5, mu)), line("M5.5 11A6.5 6.5 0 0 0 18.5 11M12 17.5V21M9 21h6")]
}
fn speaker(mu: f64) -> Vec<Part> {
    vec![line(can_side(9.4, 12.0, 4.8, 3.4, 1.0)), fill(mouth_v(9.4, 12.0, 4.8, mu))]
}
fn camera(mu: f64) -> Vec<Part> {
    vec![shape(rr(3.0, 7.0, 11.0, 10.0, 2.5)), line("M14 10.2L18.6 8M14 13.8L18.6 16"), line(rim_v(18.6, 12.0, 4.0)), fill(mouth_v(18.6, 12.0, 4.0, mu))]
}
fn bell() -> Vec<Part> {
    let a = 7.2 * view();
    vec![line("M6 16V11a6 6 0 0 1 12 0v5"), line(format!("M4.8 16.2A7.2 {a} 0 0 0 19.2 16.2H4.8")), line("M10.2 20.2a2 2 0 0 0 3.6 0")]
}
fn person(x: f64) -> Vec<Part> {
    vec![line(circle(x, 8.0, 3.6)), line(format!("M{} 20.5A7 6.3 0 0 1 {} 20.5", x - 7.0, x + 7.0))]
}
fn ends() -> Vec<Part> {
    vec![fill(circle(4.0, 12.0, 2.4)), fill(circle(20.0, 12.0, 2.4))]
}
fn with(mut v: Vec<Part>, more: impl IntoIterator<Item = Part>) -> Vec<Part> {
    v.extend(more);
    v
}

/// Every icon at one tier, in a stable order (generated files and the contact sheet follow it).
pub fn all(t: Tier) -> Vec<Icon> {
    use Kind::*;
    let mu = t.mu();
    let i = |name, kind, note, parts| Icon { name, kind, note, parts };
    vec![
        // — talking: drawn from the mark —
        i("call", Mark, "The mark in line: two cans and the slack string. Starts or shows a call.",
          with(can_pair(7.8, 11.0, mu), [string(sag(7.8, 11.0, 16.2, 11.0, 3.4))])),
        i("callend", Mark, "Cans apart, string dropped: the line is down. No red handset.",
          with(can_pair(6.8, 8.5, mu), [string("M6.8 8.5Q10 12.5 9.6 19.5"), string("M17.2 8.5Q14 12.5 14.4 19.5")])),
        i("phone", Mark, "One can, the string running off to someone. Small inline places: call history, menus.",
          vec![line(can_side(8.6, 8.6, CAN_B, CAN_L, 1.0)), fill(mouth_v(8.6, 8.6, CAN_B, mu)), string("M8.6 8.6Q15 22 21.5 14.5")]),
        i("call-in", Kept, "Direction arrow for call history.", vec![line("M17 7L7 17M15 17H7V9")]),
        i("call-out", Kept, "Direction arrow for call history.", vec![line("M7 17L17 7M9 7h8v8")]),
        i("mic", Mark, "A standing can on a stand.", mic(mu)),
        i("micoff", Mark, "Mic with the shared 45° slash.", with(mic(mu), [line(OFF)])),
        i("vol", Mark, "The can is the speaker: mouth right, two sound arcs.",
          with(speaker(mu), [line("M15 9a4.2 4.2 0 0 1 0 6M17.8 6.4a7.9 7.9 0 0 1 0 11.2")])),
        i("voloff", Mark, "Speaker can, a small cross where the sound would be.",
          with(speaker(mu), [line("M15.5 9.5l5 5M20.5 9.5l-5 5")])),
        i("video", Mark, "Smoothed body, lens drawn as a can mouth.", camera(mu)),
        i("camoff", Mark, "Camera with the shared slash.", with(camera(mu), [line(OFF)])),
        i("link", Mark, "Linked devices: two can mouths face on, a short sagging string.",
          vec![line(rim_v(5.6, 12.0, 4.6)), fill(mouth_v(5.6, 12.0, 4.6, mu)), line(rim_v(18.4, 12.0, 4.6)), fill(mouth_v(18.4, 12.0, 4.6, mu)),
               string(sag(5.6 + 4.6 * view(), 12.0, 18.4 - 4.6 * view(), 12.0, 2.2))]),
        i("direct", Ours, "Direct connection: two ends, taut string.", with(ends(), [string("M6.4 12H17.6")])),
        i("relayed", Ours, "Through a relay: the string passes through a sealed box.",
          with(ends(), [string("M6.4 12h3M14.6 12h3"), shape(rr(9.5, 9.0, 5.0, 6.0, 1.4))])),
        i("chat", Mark, "Smoothed bubble; the tail is a short curve.",
          vec![shape(rr(3.0, 4.0, 18.0, 13.0, 3.0)), line("M8 17Q7.6 19.4 5.4 20.8Q9.6 20.6 12.4 17")]),
        i("send", Mark, "Sent down the line: a thrown curve, head on its tangent.", vec![line(q_arrow(4.5, 19.5, 13.0, 18.5, 19.5, 5.5, 6.0))]),
        i("reply", Mark, "Arrow on a sagging curve.", vec![line(q_arrow(20.0, 19.0, 20.0, 9.5, 5.0, 9.5, 5.0))]),
        i("forward", Mark, "Forward a message: reply, mirrored.", vec![line(q_arrow(4.0, 19.0, 4.0, 9.5, 19.0, 9.5, 5.0))]),
        i("retry", Mark, "Arc arrow, head on the tangent.", vec![line(arc_arrow(12.0, 12.0, 7.5, -30.0, 250.0, 4.5))]),
        i("user", Mark, "Head and half-ellipse shoulders.", person(12.0)),
        i("users", Mark, "Two people, same shapes as user.",
          vec![line(circle(9.0, 8.3, 3.3)), line("M2.8 20.5A6.2 5.8 0 0 1 15.2 20.5"), line("M15.6 5.4a3 3 0 0 1 0 5.8M17.8 14.6a6 6 0 0 1 3.6 5.9")]),
        i("userplus", Mark, "One person and a plus: Add my first contact.", with(person(9.5), [line("M19 7.5v6M16 10.5h6")])),
        i("contact", Mark, "A contact card.", vec![shape(rr(4.0, 3.0, 16.0, 18.0, 2.5)), line(circle(12.0, 10.0, 3.0)), line("M7.5 17A4.8 3.6 0 0 1 16.5 17")]),
        i("bell", Mark, "A bell whose mouth is a can rim, for the knock.", bell()),
        i("mute", Mark, "Bell with the shared slash.", with(bell(), [line(OFF)])),
        // — sign-up and plain controls —
        i("back", Kept, "Plain on purpose.", vec![line("M15 5l-7 7 7 7")]),
        i("next", Kept, "Android round Continue: a plain arrow, the circle is the button.", vec![line("M5 12H18.5"), line(head(19.0, 12.0, 1.0, 0.0, 6.0, 42.0))]),
        i("enter", Kept, "Drawn ↵ for the desktop keycap.", vec![line(q_arrow(19.0, 5.0, 19.0, 15.0, 6.0, 15.0, 4.5))]),
        i("sun", Kept, "Theme switch.", vec![line(circle(12.0, 12.0, 4.0)),
          line("M12 2.5v1.8M12 19.7v1.8M2.5 12h1.8M19.7 12h1.8M5.3 5.3l1.3 1.3M17.4 17.4l1.3 1.3M5.3 18.7l1.3-1.3M17.4 6.6l1.3-1.3")]),
        i("moon", Kept, "Theme switch.", vec![line("M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5Z")]),
        i("copy", Kept, "Copy my words: two smoothed cards.", vec![shape(rr(9.0, 9.0, 11.0, 11.0, 2.5)), line("M5 15V7.5A2.5 2.5 0 0 1 7.5 5H15")]),
        i("scan", Kept, "Scan their code.", vec![line("M4 8.5V6.5A2.5 2.5 0 0 1 6.5 4h2M15.5 4h2A2.5 2.5 0 0 1 20 6.5v2M20 15.5v2a2.5 2.5 0 0 1-2.5 2.5h-2M8.5 20h-2A2.5 2.5 0 0 1 4 17.5v-2"),
          shape(rr(8.5, 8.5, 7.0, 7.0, 1.5))]),
        i("qr", Kept, "Show my code.", vec![shape(rr(4.0, 4.0, 6.0, 6.0, 1.5)), shape(rr(14.0, 4.0, 6.0, 6.0, 1.5)), shape(rr(4.0, 14.0, 6.0, 6.0, 1.5)),
          line("M14 14h2v2h-2ZM18 14h2v2M14 18h2v2h4v-2")]),
        i("chevr", Kept, "", vec![line("M10 7l5 5-5 5")]),
        i("chev", Kept, "", vec![line("M7 10l5 5 5-5")]),
        i("chevup", Kept, "", vec![line("M7 14l5-5 5 5")]),
        i("x", Kept, "", vec![line("M6 6l12 12M18 6L6 18")]),
        i("plus", Kept, "", vec![line("M12 5v14M5 12h14")]),
        i("check", Kept, "", vec![line("M5 12.5l4.5 4.5L19 7")]),
        i("search", Kept, "", vec![line(circle(11.0, 11.0, 7.0)), line("M20 20l-4-4")]),
        i("clip", Kept, "", vec![line("M21 11l-9 9a5 5 0 0 1-7-7l9-9a3.5 3.5 0 0 1 5 5l-9 9a2 2 0 0 1-3-3l8-8")]),
        i("info", Kept, "", vec![line(circle(12.0, 12.0, 9.0)), line("M12 11v6M12 7.5v.01")]),
        i("clock", Kept, "", vec![line(circle(12.0, 12.0, 9.0)), line("M12 7v5l3 2")]),
        i("edit", Kept, "", vec![line("M4 20h4L19 9l-4-4L4 16v4ZM13 7l4 4")]),
        i("trash", Kept, "", vec![line("M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13")]),
        i("file", Kept, "", vec![line("M7 3h7l5 5v13H7ZM14 3v5h5")]),
        i("image", Kept, "", vec![shape(rr(3.0, 4.0, 18.0, 16.0, 3.0)), line(circle(9.0, 10.0, 1.6)), line("M4 18l5-5 4 4 3-3 4 4")]),
        i("key", Kept, "", vec![line(circle(8.0, 15.0, 4.0)), line("M11 12l9-9M16 7l3 3M14 9l2 2")]),
        i("lock", Kept, "", vec![shape(rr(5.0, 11.0, 14.0, 10.0, 2.5)), line("M8 11V8a4 4 0 0 1 8 0v3")]),
        i("eye", Kept, "", vec![line("M2 12s3.5-6.5 10-6.5S22 12 22 12s-3.5 6.5-10 6.5S2 12 2 12Z"), line(circle(12.0, 12.0, 3.0))]),
        i("eyeoff", Kept, "", vec![line("M3 3l18 18M10.6 5.6A9.6 9.6 0 0 1 12 5.5c6.5 0 10 6.5 10 6.5a17 17 0 0 1-3.2 3.9M6.4 6.9A16.6 16.6 0 0 0 2 12s3.5 6.5 10 6.5a9.5 9.5 0 0 0 4-.9M9.9 9.9a3 3 0 0 0 4.2 4.2")]),
        i("off", Kept, "Block: circle and the shared slash.", vec![line(circle(12.0, 12.0, 9.0)), line("M5.6 5.6l12.8 12.8")]),
        i("camera", Kept, "Take a photo.", vec![line("M4 8h3l2-3h6l2 3h3v12H4Z"), line(circle(12.0, 13.5, 3.5))]),
        i("devices", Kept, "", vec![line("M3 6h13v9H3ZM1 18h17M18 9h4v9h-4Z")]),
        i("smartphone", Kept, "", vec![shape(rr(7.0, 2.5, 10.0, 19.0, 2.5)), line("M11 18.5h2")]),
        i("laptop", Kept, "", vec![shape(rr(4.5, 5.0, 15.0, 10.0, 1.5)), line("M2.5 19h19")]),
        i("server", Kept, "", vec![shape(rr(3.0, 4.0, 18.0, 7.0, 2.0)), shape(rr(3.0, 13.0, 18.0, 7.0, 2.0)), line("M7 7.5v.01M7 16.5v.01")]),
        i("battery", Kept, "", vec![shape(rr(2.0, 7.0, 17.0, 10.0, 2.0)), line("M22 11v2M6 10v4M10 10v4")]),
        i("leave", Kept, "", vec![line("M10 4H5v16h5M15 8l4 4-4 4M19 12H9")]),
        i("flag", Kept, "", vec![line("M5 21V4M5 4h11l-2 4 2 4H5")]),
        i("gear", Kept, "", vec![line(circle(12.0, 12.0, 3.0)),
          line("M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z")]),
        i("sliders", Kept, "", vec![line("M4 6h8M18 6h2M4 12h2M12 12h8M4 18h10"), line(circle(15.0, 6.0, 2.5)), line(circle(9.0, 12.0, 2.5)), line(circle(17.0, 18.0, 2.5))]),
        i("warning", Retire, "Retire: hints and warnings speak in words (VOICE.md). Kept until screens stop using it.",
          vec![line("M12 3l10 18H2ZM12 10v5M12 18v.01")]),
        i("shield", Retire, "Retire: the empty chat gets the mark instead. Kept until screens stop using it.",
          vec![line("M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6Z")]),
    ]
}
