//! The Tinline mark (Mark D): two cans joined by a fixed-length string, as an iced canvas widget.
//!
//! A port of docs/design/playground/src/lib/brand/{tinmark.js,tiers.js}: the same geometry per optical
//! size tier, the same spring/string model and constants. The string has a fixed length; when the cans
//! move apart (`delta`) the sag follows from the length (exact parabola arc length), so a taut string is
//! the cans pulled out until the sag is gone. States: slack (rest), ringing (pendulum sway), taut
//! (connected: the cans pull apart, then the string twangs once), plus a quiet pluck on send.
//!
//! The animation drives itself: while the model is moving the widget asks for the next frame, when it
//! has settled it asks for nothing, so a still mark costs no CPU. Reduced motion jumps to end poses.

use iced::mouse;
use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::{window, Color, Element, Length, Point, Rectangle, Renderer, Theme};
use std::sync::OnceLock;
use std::time::Instant;

// ---- constants (tinmark.js DEF) ------------------------------------------------------------------------------
const BASE_C: f64 = 7.5;
const BASE_B: f64 = 5.5;
const BASE_THETA: f64 = 30.6;
const BASE_MU: f64 = 0.62;
const BASE_L: f64 = 3.0;
const BASE_S0: f64 = 3.5;
const BASE_STROKE: f64 = 1.5;
const PHI0_DEG: f64 = 12.0;
const SWAY_T: f64 = 1.6;
const TWANG_A: f64 = 0.5;
const PLUCK_A: f64 = 0.32;
const PLUCK_SWING_DEG: f64 = 150.0;
const TWANG_F: f64 = 7.0;
const TWANG_TAU: f64 = 0.11;
const HIT_OVER: f64 = 1.1;
const CONNECT_HZ: f64 = 3.0;
const END_ZETA: f64 = 1.6;
const END_MS: f64 = 260.0;
/// Fixed physics step (tinmark.js: ceil(dt / (1/480))).
const SUBSTEP: f64 = 1.0 / 480.0;
/// In-app colours per theme (playground: --accent / --gold in light and dark).
pub fn colors(dark: bool) -> (Color, Color) {
    let rgb = |r: u8, g: u8, b: u8| Color::from_rgb8(r, g, b);
    if dark { (rgb(0x11, 0x7E, 0x6B), rgb(0xE8, 0xB0, 0x4A)) } else { (rgb(0x0B, 0x6B, 0x5B), rgb(0xC9, 0x8A, 0x1E)) }
}

/// What the brand-row mark shows (status.svelte.ts statusLook, same numbers).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CallPhase {
    Incoming,
    Outgoing,
    Connecting,
    Active,
    Reconnecting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Net {
    Offline,
    Connecting,
    Online,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusLook {
    pub state: MarkState,
    pub sway: Option<(f64, f64)>,
    pub dim: bool,
    pub label: &'static str,
}

/// Call phase wins over the connection state. Sway periods are locked to the cue patterns
/// (ringtone 3.2 s = two swings, ringback 4 s = two swings).
pub fn status_look(call: Option<CallPhase>, net: Net) -> StatusLook {
    let ringing = |period, amp, label| StatusLook { state: MarkState::Ringing, sway: Some((period, amp)), dim: false, label };
    match call {
        Some(CallPhase::Incoming) => return ringing(1.6, 12.0, "Incoming call"),
        Some(CallPhase::Outgoing) => return ringing(2.0, 9.0, "Calling"),
        Some(CallPhase::Connecting) => return ringing(1.2, 6.0, "Connecting"),
        Some(CallPhase::Active) => return StatusLook { state: MarkState::Taut, sway: None, dim: false, label: "In a call" },
        Some(CallPhase::Reconnecting) => return ringing(2.0, 4.0, "Reconnecting"),
        None => {}
    }
    match net {
        Net::Offline => StatusLook { state: MarkState::Slack, sway: None, dim: true, label: "No connection" },
        Net::Connecting => ringing(3.2, 5.0, "Connecting"),
        Net::Online => StatusLook { state: MarkState::Taut, sway: None, dim: false, label: "Connected to the network" },
    }
}

/// The mint of the can mouths (Mark.svelte: --tm-mouth).
const MOUTH: Color = Color { r: 0x9F as f32 / 255.0, g: 0xD6 as f32 / 255.0, b: 0xC8 as f32 / 255.0, a: 1.0 };

fn rad(d: f64) -> f64 {
    d.to_radians()
}

// ---- length model --------------------------------------------------------------------------------------------
/// Exact arc length of the parabola spanning `w` with sag `s`.
fn len_exact(w: f64, s: f64) -> f64 {
    let s = s.abs();
    if s < 1e-9 {
        return w;
    }
    0.5 * (w * w + 16.0 * s * s).sqrt() + (w * w / (8.0 * s)) * (4.0 * s / w).asinh()
}

/// Sag that gives string length `ell` over span `w`.
fn sag_of_w(w: f64, ell: f64) -> f64 {
    if w >= ell {
        return 0.0;
    }
    let sa = (3.0 * w * (ell - w) / 8.0).sqrt();
    let (mut lo, mut hi) = (0.0, sa.max(1e-6) * 1.5);
    while len_exact(w, hi) < ell {
        hi *= 2.0;
    }
    for _ in 0..40 {
        let m = (lo + hi) / 2.0;
        if len_exact(w, m) < ell {
            lo = m;
        } else {
            hi = m;
        }
    }
    (lo + hi) / 2.0
}

// ---- tiers (tiers.js) ---------------------------------------------------------------------------------------
/// Hand-hinted small tiers in px: (B, c, a, b, L, mu, ko, stroke, s, sT).
const SMALL: [(u32, f64, f64, f64, f64, f64, f64, f64, f64, Option<f64>); 6] = [
    (12, 3.5, 1.5, 4.0, 1.5, 0.5, 0.75, 1.5, 2.5, Some(0.5)),
    (14, 4.5, 1.5, 5.0, 2.0, 0.5, 0.75, 1.5, 3.0, Some(0.5)),
    (16, 5.0, 2.0, 5.0, 2.0, 0.5, 1.25, 1.75, 3.0, Some(0.5)),
    (20, 6.0, 2.5, 6.0, 2.5, 0.5, 1.25, 1.75, 3.5, Some(0.75)),
    (24, 7.5, 3.0, 6.0, 3.0, 0.6, 0.0, 1.75, 3.5, None),
    (28, 8.5, 3.5, 7.0, 3.5, 0.6, 0.0, 1.75, 4.0, None),
];
const TIER_SIZES: [u32; 15] = [12, 14, 16, 20, 24, 28, 32, 40, 48, 64, 96, 128, 256, 512, 1024];

/// Nearest designed tier for a rendered size (ties go to the smaller one).
pub fn pick_tier(px: f32) -> u32 {
    let px = px as f64;
    if px > 1024.0 {
        return 1024;
    }
    let mut best = TIER_SIZES[0];
    for t in TIER_SIZES {
        let (dt, db) = ((t as f64 - px).abs(), (best as f64 - px).abs());
        if dt < db {
            best = t;
        }
    }
    best
}

/// Everything about one tier, in model units (24-grid) unless noted.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    c: f64,
    a: f64,
    b: f64,
    ko: f64,
    l: f64,
    mu: f64,
    stroke: f64,
    s0: f64,
    /// Small tiers (<= 20 px): the cans never move; "taut" is the sag easing from `s0` to `s_t`.
    sag_only: bool,
    s_t: f64,
    /// Viewbox: origin and side, model units.
    vb: (f64, f64, f64),
    ell: f64,
    dt: f64,
    /// Sway period (s) and amplitude (degrees).
    period: f64,
    amp: f64,
}

impl Params {
    pub fn tier(px: f32) -> Params {
        let big_b = pick_tier(px);
        let half = |v: f64| (v * 2.0).round() / 2.0;
        // px-level spec: (c, a, b, L, mu, ko, sw, s, sT), u
        let (c, a, b, l, mu, ko, sw, s, st, u) = if let Some(t) = SMALL.iter().find(|t| t.0 == big_b) {
            (t.1, t.2, t.3, t.4, t.5, t.6, t.7, t.8, t.9, 1.0)
        } else {
            let u = big_b as f64 / 24.0;
            let exact = big_b >= 96;
            let sn = |v: f64, f: &dyn Fn(f64) -> f64| if exact { v } else { f(v) };
            let a_raw = BASE_B * rad(BASE_THETA).sin() * u;
            (
                sn(BASE_C * u, &half),
                sn(a_raw, &half),
                sn(BASE_B * u, &f64::round),
                sn(BASE_L * u, &half),
                BASE_MU,
                0.0,
                (1.0f64).max(sn(BASE_STROKE * u, &half)).max(1.5),
                BASE_S0 * u,
                None,
                u,
            )
        };
        let half0 = big_b as f64 / (2.0 * u);
        let c_m = 12.0 - (big_b as f64 / 2.0 - c) / u;
        let w0 = 24.0 - 2.0 * c_m;
        let s0 = s / u;
        let ell = len_exact(w0, s0);
        Params {
            c: c_m,
            a: a / u,
            b: b / u,
            ko: ko / u,
            l: l / u,
            mu: if mu > 0.0 { mu } else { 0.0001 },
            stroke: sw / u,
            s0,
            sag_only: st.is_some(),
            s_t: st.map_or(0.0, |v| v / u),
            vb: (12.0 - half0, 12.0 - half0, big_b as f64 / u),
            ell,
            dt: ((ell - w0) / 2.0).max(0.0),
            period: SWAY_T,
            amp: PHI0_DEG,
        }
    }
}

// ---- the model (tinmark.js attach) ----------------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Slack,
    Taut,
}

/// The spring/string state. Pure: no clock, no window.
#[derive(Debug, Clone)]
pub struct Model {
    p: Params,
    /// Outward displacement of the cans, and its velocity.
    d: f64,
    v: f64,
    /// String swing angle (rad) and its velocity.
    phi: f64,
    phv: f64,
    ring: bool,
    ring_phase: f64,
    target: Target,
    hit: bool,
    /// Twang clock (< 0: none) and amplitude (0: default).
    tw: f64,
    tw_a: f64,
    /// Reduced motion: every state change jumps to its end pose.
    pub reduced: bool,
}

/// What to draw: cans displaced by `delta`, string of sag `s` swung by `phi`.
#[derive(Debug, Clone, Copy)]
pub struct Pose {
    pub delta: f64,
    pub sag: f64,
    pub phi: f64,
}

impl Model {
    pub fn new(p: Params) -> Model {
        Model { p, d: 0.0, v: 0.0, phi: 0.0, phv: 0.0, ring: false, ring_phase: 0.0, target: Target::Slack, hit: false, tw: -1.0, tw_a: 0.0, reduced: false }
    }

    pub fn params(&self) -> &Params {
        &self.p
    }

    /// Sway period (s) and amplitude (degrees), e.g. locked to a ringtone.
    pub fn set_sway(&mut self, period: f64, amp: f64) {
        self.p.period = period;
        self.p.amp = amp;
    }

    /// Jump to rest or fully taut with no motion.
    pub fn set(&mut self, taut: bool) {
        self.ring = false;
        self.d = if taut { self.p.dt } else { 0.0 };
        self.v = 0.0;
        self.phi = 0.0;
        self.phv = 0.0;
        self.tw = -1.0;
        self.target = if taut { Target::Taut } else { Target::Slack };
        self.hit = taut && self.p.dt > 0.0;
    }

    fn snap(&mut self, taut: bool) {
        self.set(taut);
        self.hit = taut;
    }

    pub fn ring(&mut self, on: bool) {
        if self.reduced {
            self.ring = false;
            if self.target == Target::Taut || self.d > 0.0 {
                self.snap(false);
            }
            return;
        }
        self.ring = on;
        if on {
            self.target = Target::Slack;
            self.hit = false;
        }
    }

    pub fn connect(&mut self) {
        if self.reduced {
            self.snap(true);
            return;
        }
        self.ring = false;
        self.target = Target::Taut;
        self.hit = self.d >= self.p.dt && self.hit;
    }

    pub fn end(&mut self) {
        if self.reduced {
            self.snap(false);
            return;
        }
        self.ring = false;
        self.target = Target::Slack;
        self.hit = false;
        self.tw = -1.0;
        if self.phi.abs() < rad(1.0) {
            self.phv += rad(242.0); // a ~6 degree swing as the slack returns
        }
    }

    /// A quiet acknowledgement (message sent): a small twang if taut, else a little swing. Never stacks.
    pub fn pluck(&mut self) {
        if self.reduced {
            return;
        }
        if self.target == Target::Taut && self.d >= self.p.dt - 1e-9 {
            if self.tw < 0.0 {
                self.tw_a = PLUCK_A;
                self.tw = 0.0;
            }
        } else if self.phi.abs() < rad(1.0) && self.phv.abs() < rad(20.0) {
            self.phv += rad(PLUCK_SWING_DEG);
        }
    }

    fn end_omega(&self) -> f64 {
        // calibrate omega so |D| < 0.01 (from DT) takes END_MS for zeta = END_ZETA
        let z = END_ZETA;
        let (k, m) = (z + (z * z - 1.0).sqrt(), z - (z * z - 1.0).sqrt());
        let thr = 0.01 / self.p.dt.max(1e-6);
        let x = |u: f64| (k * (-m * u).exp() - m * (-k * u).exp()) / (k - m);
        let (mut lo, mut hi) = (0.0, 200.0);
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            if x(mid) > thr {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        hi / (END_MS / 1000.0)
    }

    fn twang_amp(&self) -> f64 {
        if self.tw_a != 0.0 { self.tw_a } else { TWANG_A }
    }

    fn step_once(&mut self, h: f64) {
        let wc = 2.0 * std::f64::consts::PI * CONNECT_HZ;
        let (z, w, tgt) = if self.target == Target::Taut { (1.0, wc, self.p.dt * HIT_OVER) } else { (END_ZETA, self.end_omega(), 0.0) };
        let a = -2.0 * z * w * self.v - w * w * (self.d - tgt);
        self.v += a * h;
        self.d += self.v * h;
        if self.target == Target::Taut && self.d >= self.p.dt {
            self.d = self.p.dt;
            self.v = 0.0;
            if !self.hit {
                self.hit = true;
                self.tw = 0.0;
                self.tw_a = 0.0;
            }
        }
        if self.d < 0.0 {
            self.d = 0.0;
            if self.v < 0.0 {
                self.v = 0.0;
            }
        }
        if self.ring {
            self.ring_phase += 2.0 * std::f64::consts::PI / self.p.period * h;
            let tgt_phi = rad(self.p.amp) * self.ring_phase.sin();
            let k = 1.0 - (-h / 0.05).exp();
            let old = self.phi;
            self.phi += (tgt_phi - self.phi) * k;
            self.phv = if h > 0.0 { (self.phi - old) / h } else { 0.0 };
        } else {
            let taut = self.target == Target::Taut;
            let (zz, ww) = if taut { (1.0, wc) } else { (0.5, 2.0 * std::f64::consts::PI * 3.5) };
            self.phv += (-2.0 * zz * ww * self.phv - ww * ww * self.phi) * h;
            self.phi += self.phv * h;
        }
        if self.tw >= 0.0 {
            self.tw += h;
            if self.twang_amp() * (-self.tw / TWANG_TAU).exp() < 0.03 {
                self.tw = -1.0;
            }
        }
    }

    /// Whether anything is still moving (the widget keeps asking for frames only while this holds).
    pub fn active(&self) -> bool {
        if self.ring || self.tw >= 0.0 {
            return true;
        }
        let moving = if self.target == Target::Taut { self.d < self.p.dt } else { self.d >= 0.01 };
        moving || self.phi.abs() > rad(0.2) || self.phv.abs() > rad(2.0)
    }

    /// Advances by `dt` seconds (physics at fixed 1/480 s steps). Returns whether it is still active;
    /// when it has just settled, the resting pose is made exact.
    pub fn advance(&mut self, dt: f64) -> bool {
        if self.reduced {
            self.snap(self.target == Target::Taut);
            return false;
        }
        let n = ((dt / SUBSTEP).ceil() as usize).max(1);
        let h = dt / n as f64;
        for _ in 0..n {
            self.step_once(h);
        }
        if self.active() {
            return true;
        }
        if self.target == Target::Slack {
            self.d = 0.0;
            self.v = 0.0;
        }
        self.phi = 0.0;
        self.phv = 0.0;
        self.tw = -1.0;
        false
    }

    fn sag(&self) -> f64 {
        if self.p.sag_only {
            let k = if self.p.dt > 0.0 { (self.d / self.p.dt).min(1.0) } else { 0.0 };
            return self.p.s0 + (self.p.s_t - self.p.s0) * k;
        }
        sag_of_w(24.0 - 2.0 * (self.p.c - self.d), self.p.ell)
    }

    fn twang(&self) -> f64 {
        if self.tw < 0.0 {
            return 0.0;
        }
        self.twang_amp() * (-self.tw / TWANG_TAU).exp() * (2.0 * std::f64::consts::PI * TWANG_F * self.tw).sin()
    }

    pub fn pose(&self) -> Pose {
        Pose { delta: if self.p.sag_only { 0.0 } else { self.d }, sag: self.sag() + self.twang(), phi: self.phi }
    }
}

// ---- drawing --------------------------------------------------------------------------------------------------
const KAPPA: f64 = 0.552_284_749_8;

struct Space {
    ox: f64,
    oy: f64,
    k: f64,
    mirror: bool,
}

impl Space {
    fn pt(&self, x: f64, y: f64) -> Point {
        let x = if self.mirror { 24.0 - x } else { x };
        Point::new(((x - self.ox) * self.k) as f32, ((y - self.oy) * self.k) as f32)
    }
}

fn ellipse(sp: &Space, cx: f64, rx: f64, ry: f64) -> Path {
    let (cy, k) = (12.0, KAPPA);
    Path::new(|p| {
        p.move_to(sp.pt(cx + rx, cy));
        p.bezier_curve_to(sp.pt(cx + rx, cy + k * ry), sp.pt(cx + k * rx, cy + ry), sp.pt(cx, cy + ry));
        p.bezier_curve_to(sp.pt(cx - k * rx, cy + ry), sp.pt(cx - rx, cy + k * ry), sp.pt(cx - rx, cy));
        p.bezier_curve_to(sp.pt(cx - rx, cy - k * ry), sp.pt(cx - k * rx, cy - ry), sp.pt(cx, cy - ry));
        p.bezier_curve_to(sp.pt(cx + k * rx, cy - ry), sp.pt(cx + rx, cy - k * ry), sp.pt(cx + rx, cy));
        p.close();
    })
}

/// The can body: flat front at the rim, rounded back.
fn body(sp: &Space, p: &Params, cx: f64) -> Path {
    let (ym, yp, l, a, b, k) = (12.0 - p.b, 12.0 + p.b, p.l, p.a, p.b, KAPPA);
    let bx = cx - l;
    Path::new(|q| {
        q.move_to(sp.pt(cx, ym));
        q.line_to(sp.pt(bx, ym));
        q.bezier_curve_to(sp.pt(bx - k * a, ym), sp.pt(bx - a, 12.0 - k * b), sp.pt(bx - a, 12.0));
        q.bezier_curve_to(sp.pt(bx - a, 12.0 + k * b), sp.pt(bx - k * a, yp), sp.pt(bx, yp));
        q.line_to(sp.pt(cx, yp));
        q.close();
    })
}

/// Draws the mark filling `size` x `size` px in `frame`.
fn paint(frame: &mut canvas::Frame, size: f32, p: &Params, pose: Pose, can: Color, string: Color, mouth: Color) {
    let k = size as f64 / p.vb.2;
    let cx = p.c - pose.delta;
    for mirror in [false, true] {
        let sp = Space { ox: p.vb.0, oy: p.vb.1, k, mirror };
        frame.fill(&body(&sp, p, cx), can);
        frame.fill(&ellipse(&sp, cx, p.a, p.b), can);
        frame.fill(&ellipse(&sp, cx, p.mu * p.a, p.mu * p.b), mouth);
    }
    let sp = Space { ox: p.vb.0, oy: p.vb.1, k, mirror: false };
    let x1 = p.c - pose.delta + p.ko;
    let x2 = 24.0 - x1;
    let line = Path::new(|q| {
        q.move_to(sp.pt(x1, 12.0));
        q.quadratic_curve_to(sp.pt(12.0 + 2.0 * pose.sag * pose.phi.sin(), 12.0 + 2.0 * pose.sag * pose.phi.cos()), sp.pt(x2, 12.0));
    });
    frame.stroke(&line, Stroke::default().with_width((p.stroke * k) as f32).with_color(string).with_line_cap(canvas::LineCap::Round).with_line_join(canvas::LineJoin::Round));
}

// ---- widget -----------------------------------------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkState {
    #[default]
    Slack,
    Ringing,
    Taut,
}

/// What the mark should show. `pluck` is a counter: a change plucks the string once.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Look {
    pub state: MarkState,
    /// Ringing sway: period (s) and amplitude (degrees), locked to the cue that is playing.
    pub sway: Option<(f64, f64)>,
    pub pluck: u64,
    /// No connection: the whole mark at 45 % (the string in the muted ink).
    pub dim: bool,
}

/// True when the user asked the system for less motion (env `TINLINE_REDUCE_MOTION`, GNOME
/// "enable-animations" off). The desktop app has no setting of its own.
pub fn reduced_motion() -> bool {
    static R: OnceLock<bool> = OnceLock::new();
    *R.get_or_init(|| {
        if std::env::var("TINLINE_REDUCE_MOTION").is_ok_and(|v| v != "0" && !v.is_empty()) {
            return true;
        }
        #[cfg(target_os = "linux")]
        if let Ok(o) = std::process::Command::new("gsettings").args(["get", "org.gnome.desktop.interface", "enable-animations"]).output() {
            return String::from_utf8_lossy(&o.stdout).trim() == "false";
        }
        false
    })
}

fn model_for(size: f32, look: Look) -> Model {
    let mut m = Model::new(Params::tier(size));
    m.reduced = reduced_motion();
    if let Some((t, a)) = look.sway {
        m.set_sway(t, a);
    }
    m.set(look.state == MarkState::Taut);
    if look.state == MarkState::Ringing {
        m.ring(true);
    }
    m
}

struct Prog {
    size: f32,
    look: Look,
    can: Color,
    string: Color,
}

#[derive(Default)]
struct Live {
    model: Option<Model>,
    look: Look,
    last: Option<Instant>,
}

impl<M> canvas::Program<M> for Prog {
    type State = Live;

    fn update(&self, st: &mut Live, event: &iced::Event, _: Rectangle, _: mouse::Cursor) -> Option<canvas::Action<M>> {
        let iced::Event::Window(window::Event::RedrawRequested(now)) = event else { return None };
        let look = self.look;
        let mut woke = false;
        match st.model.as_mut() {
            None => {
                st.model = Some(model_for(self.size, look));
                st.look = look;
                woke = look.state == MarkState::Ringing;
            }
            Some(m) if st.look != look => {
                let prev = st.look;
                if let Some((t, a)) = look.sway {
                    m.set_sway(t, a);
                }
                if look.state != prev.state {
                    match look.state {
                        MarkState::Taut => m.connect(),
                        MarkState::Ringing => m.ring(true),
                        MarkState::Slack => {
                            m.ring(false);
                            m.end();
                        }
                    }
                }
                if look.pluck != prev.pluck {
                    m.pluck();
                }
                st.look = look;
                woke = true;
            }
            Some(_) => {}
        }
        let m = st.model.as_mut()?;
        let dt = match st.last {
            Some(l) if !woke => now.saturating_duration_since(l).as_secs_f64().min(0.05),
            _ => 0.0,
        };
        st.last = Some(*now);
        if m.active() || woke {
            if m.advance(dt) {
                return Some(canvas::Action::request_redraw());
            }
        } else {
            st.last = None;
        }
        None
    }

    fn draw(&self, st: &Live, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let fresh;
        let m = match &st.model {
            Some(m) => m,
            None => {
                fresh = model_for(self.size, self.look);
                &fresh
            }
        };
        let fade = |c: Color| if self.look.dim { Color { a: c.a * 0.45, ..c } } else { c };
        paint(&mut frame, self.size, m.params(), m.pose(), fade(self.can), fade(self.string), fade(MOUTH));
        vec![frame.into_geometry()]
    }
}

/// The mark at `size` logical px, in the given colours. Static unless `look` changes; animates itself.
pub fn mark<'a, M: 'a>(size: f32, can: Color, string: Color, look: Look) -> Element<'a, M> {
    Canvas::new(Prog { size, look, can, string }).width(Length::Fixed(size)).height(Length::Fixed(size)).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Numeric arc length of the drawn string for a pose.
    fn drawn_len(p: &Params, pose: Pose) -> f64 {
        let x1 = p.c - pose.delta + p.ko;
        let x2 = 24.0 - x1;
        let (cx, cy) = (12.0 + 2.0 * pose.sag * pose.phi.sin(), 12.0 + 2.0 * pose.sag * pose.phi.cos());
        let at = |t: f64| {
            let u = 1.0 - t;
            (u * u * x1 + 2.0 * u * t * cx + t * t * x2, u * u * 12.0 + 2.0 * u * t * cy + t * t * 12.0)
        };
        let n = 4000;
        let mut len = 0.0;
        let mut prev = at(0.0);
        for i in 1..=n {
            let q = at(i as f64 / n as f64);
            len += ((q.0 - prev.0).powi(2) + (q.1 - prev.1).powi(2)).sqrt();
            prev = q;
        }
        len
    }

    #[test]
    fn string_length_is_conserved_while_the_cans_move() {
        for px in [24.0, 28.0, 32.0, 48.0, 128.0] {
            let p = Params::tier(px);
            for i in 0..=10 {
                let m = Model { d: p.dt * i as f64 / 10.0, ..Model::new(p) };
                let pose = Pose { delta: m.d, sag: m.sag(), phi: 0.0 };
                assert!((drawn_len(&p, pose) - p.ell).abs() < 1e-3, "tier {px} d={}", m.d);
            }
        }
    }

    #[test]
    fn connect_pulls_taut_once_and_settles() {
        let mut m = Model::new(Params::tier(48.0));
        m.connect();
        let mut hit_twang = false;
        let mut t = 0.0;
        while m.advance(1.0 / 60.0) {
            hit_twang |= m.tw >= 0.0;
            t += 1.0 / 60.0;
            assert!(t < 3.0, "still moving after 3 s");
        }
        assert!(hit_twang, "the string twangs when it goes tight");
        assert_eq!(m.d, m.p.dt);
        assert!(m.pose().sag.abs() < 1e-6, "taut string has no sag");
        assert!(!m.active());
    }

    #[test]
    fn sway_runs_until_ended_then_comes_to_rest() {
        let mut m = Model::new(Params::tier(28.0));
        m.ring(true);
        let mut peak: f64 = 0.0;
        for _ in 0..300 {
            assert!(m.advance(1.0 / 60.0), "ringing never idles");
            peak = peak.max(m.phi.abs());
        }
        assert!((peak - rad(PHI0_DEG)).abs() < rad(1.0), "sways to the amplitude, got {}", peak.to_degrees());
        m.ring(false);
        m.end();
        let mut t = 0.0;
        while m.advance(1.0 / 60.0) {
            t += 1.0 / 60.0;
            assert!(t < 4.0);
        }
        assert_eq!((m.d, m.phi), (0.0, 0.0));
    }

    #[test]
    fn end_returns_a_taut_string_to_slack() {
        let mut m = Model::new(Params::tier(64.0));
        m.set(true);
        m.end();
        let mut t = 0.0;
        while m.advance(1.0 / 60.0) {
            t += 1.0 / 60.0;
            assert!(t < 3.0);
        }
        assert_eq!(m.d, 0.0);
        assert!((m.pose().sag - m.p.s0).abs() < 1e-9);
    }

    #[test]
    fn pluck_is_quiet_and_does_not_stack() {
        let mut m = Model::new(Params::tier(28.0));
        m.set(true);
        m.pluck();
        assert_eq!(m.tw_a, PLUCK_A);
        let before = m.tw;
        m.advance(0.01);
        m.pluck();
        assert!(m.tw > before, "a running twang is left alone");
        let mut t = 0.0;
        while m.advance(1.0 / 60.0) {
            t += 1.0 / 60.0;
            assert!(t < 2.0);
        }
        // slack: a small swing that dies out
        let mut s = Model::new(Params::tier(28.0));
        s.pluck();
        assert!(s.advance(1.0 / 60.0));
        while s.advance(1.0 / 60.0) {}
        assert_eq!(s.phi, 0.0);
    }

    #[test]
    fn reduced_motion_jumps_to_end_poses() {
        let mut m = Model::new(Params::tier(48.0));
        m.reduced = true;
        m.connect();
        assert!(!m.advance(1.0 / 60.0));
        assert_eq!(m.d, m.p.dt);
        m.ring(true);
        assert!(!m.ring && m.d == 0.0);
        m.pluck();
        assert!(!m.active());
    }

    #[test]
    fn small_tiers_stay_put_and_only_the_sag_changes() {
        let mut m = Model::new(Params::tier(16.0));
        assert!(m.p.sag_only);
        let slack = m.pose();
        m.set(true);
        let taut = m.pose();
        assert_eq!((slack.delta, taut.delta), (0.0, 0.0));
        assert!(taut.sag < slack.sag);
    }

    #[test]
    fn status_look_matches_the_playground_table() {
        let l = status_look(Some(CallPhase::Incoming), Net::Online);
        assert_eq!((l.state, l.sway, l.dim), (MarkState::Ringing, Some((1.6, 12.0)), false));
        assert_eq!(status_look(Some(CallPhase::Outgoing), Net::Offline).sway, Some((2.0, 9.0)));
        assert_eq!(status_look(Some(CallPhase::Connecting), Net::Online).sway, Some((1.2, 6.0)));
        assert_eq!(status_look(Some(CallPhase::Active), Net::Offline).state, MarkState::Taut);
        assert_eq!(status_look(Some(CallPhase::Reconnecting), Net::Online).sway, Some((2.0, 4.0)));
        assert_eq!(status_look(None, Net::Connecting).sway, Some((3.2, 5.0)));
        let off = status_look(None, Net::Offline);
        assert_eq!((off.state, off.dim), (MarkState::Slack, true));
        assert_eq!(status_look(None, Net::Online).state, MarkState::Taut);
    }

    #[test]
    fn tier_picking_matches_the_table() {
        assert_eq!(pick_tier(28.0), 28);
        assert_eq!(pick_tier(30.0), 28);
        assert_eq!(pick_tier(48.0), 48);
        assert_eq!(pick_tier(44.0), 40);
        assert_eq!(pick_tier(18.0), 16);
    }
}
