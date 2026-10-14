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
use iced::widget::canvas::{self, Canvas};
use resvg::tiny_skia as sk;
use std::collections::HashMap;
use iced::{window, Color, Element, Length, Rectangle, Renderer, Theme};
use std::sync::{Mutex, OnceLock};
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
const CONNECT_HZ: f64 = 3.0;
const END_ZETA: f64 = 1.6;
const END_MS: f64 = 260.0;
/// Offset of the `line` cue's second note (s, cues.rs `LINE`): the two tugs land this far apart.
pub const LINE_GAP_S: f64 = 0.12;

/// Per-use spring parameters of the one model (tinmark.js DEF and its overrides, MARK-MATH.md
/// "Connect", 2026-10-13). The defaults are the everyday pull (calls, the status mark): the pull
/// arrives with speed (`hit_over`), the cans bounce back off the tight string (`recoil` = restitution),
/// and an undamped `settle_hz` pull carries them out again until a bounce is below `recoil_min`
/// (grid units/s). `tugs` > 0 instead tugs exactly that many times, each a full twang, then rests.
/// `windup` (grid units over `windup_ms`) gathers the cans in before the pull; off by default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    pub hit_over: f64,
    pub recoil: f64,
    pub recoil_min: f64,
    pub settle_hz: f64,
    pub windup: f64,
    pub windup_ms: f64,
    pub tugs: u32,
}

impl Default for Spring {
    fn default() -> Self {
        Spring { hit_over: 1.6, recoil: 0.5, recoil_min: 1.2, settle_hz: 2.0, windup: 0.0, windup_ms: 180.0, tugs: 0 }
    }
}

impl Spring {
    /// The old dead stop (recoil 0): the string goes tight and the cans stay put.
    #[allow(dead_code)]
    pub fn dead_stop() -> Self {
        Spring { hit_over: 1.1, recoil: 0.0, ..Spring::default() }
    }

    /// springs.ts LINE_SPRING, the "line opens" pull (sign-up Done, the first contact on the empty
    /// state): two tugs in the rhythm of the `line` cue, so the gap is read from the cue itself.
    pub fn line() -> Self {
        let gap = LINE_GAP_S;
        Spring { tugs: 2, recoil: 0.95, recoil_min: 0.0, settle_hz: 1.0 / (2.0 * gap), ..Spring::default() }
    }
}

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
    spring: Spring,
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
            spring: Spring::default(),
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
    /// Tugs since the last connect, the speed of the first one, and the windup time left (s).
    tugs: u32,
    v0: f64,
    wind: f64,
    /// Every tug so far (the widget reports each new one, e.g. to play the `line` cue on the first).
    pub tug_count: u64,
    /// The string has gone tight since the last slack (the sign-up line colour warms on this).
    pub tight: bool,
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
        Model { p, d: 0.0, v: 0.0, phi: 0.0, phv: 0.0, ring: false, ring_phase: 0.0, target: Target::Slack, hit: false, tw: -1.0, tw_a: 0.0, tugs: 0, v0: 0.0, wind: 0.0, tug_count: 0, tight: false, reduced: false }
    }

    /// Spring parameters for this use (Done's two tugs, the default pull for calls).
    pub fn set_spring(&mut self, s: Spring) {
        self.p.spring = s;
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
        self.wind = 0.0;
        self.tight = taut;
    }

    /// Reduced-motion jump. A fresh taut still counts as the first tug, so the colour warms and
    /// the cue plays.
    fn snap(&mut self, taut: bool) {
        let fresh = taut && !self.hit;
        self.set(taut);
        self.hit = taut;
        if fresh {
            self.tug_count += 1;
        }
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
        self.v0 = 0.0;
        self.tugs = 0;
        let sp = self.p.spring;
        self.wind = if sp.windup > 0.0 && self.d < 0.01 { sp.windup_ms / 1000.0 } else { 0.0 };
    }

    pub fn end(&mut self) {
        if self.reduced {
            self.snap(false);
            return;
        }
        self.ring = false;
        self.target = Target::Slack;
        self.hit = false;
        self.tight = false;
        self.wind = 0.0;
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
        let tau = 2.0 * std::f64::consts::PI;
        let wc = tau * CONNECT_HZ;
        let sp = self.p.spring;
        let (z, w, tgt) = if self.wind > 0.0 {
            // anticipation: the cans gather in before the pull
            self.wind -= h;
            (1.0, tau / (sp.windup_ms / 1000.0) * 1.2, -sp.windup)
        } else if self.target == Target::Taut && self.hit {
            // after the tug: the string pulls the recoiling cans back out (undamped)
            (0.0, tau * sp.settle_hz, self.p.dt)
        } else if self.target == Target::Taut {
            (1.0, wc, self.p.dt * sp.hit_over)
        } else {
            (END_ZETA, self.end_omega(), 0.0)
        };
        let a = -2.0 * z * w * self.v - w * w * (self.d - tgt);
        self.v += a * h;
        self.d += self.v * h;
        if self.target == Target::Taut && self.d >= self.p.dt && self.v <= 0.0 {
            // Resting at (or recoiling from) taut: not a tug. (tinmark.js counts this as one and
            // re-plucks every frame once `tugs` have run out, so the twang never ends.)
            self.d = self.p.dt;
        } else if self.target == Target::Taut && self.d >= self.p.dt {
            // The string goes tight and tugs the cans back: they recoil (velocity reflected,
            // `recoil` = restitution) and each smaller bounce re-plucks the string, until the bounce
            // is below `recoil_min` and they rest at DT. recoil 0 = the old dead stop.
            let v = self.v;
            self.d = self.p.dt;
            self.tugs += 1;
            let last = sp.tugs > 0 && self.tugs >= sp.tugs;
            self.v = if !last && v * sp.recoil >= sp.recoil_min && sp.recoil > 0.0 { -v * sp.recoil } else { 0.0 };
            if !self.hit {
                self.hit = true;
                self.tw = 0.0;
                self.tw_a = 0.0;
            } else if sp.tugs > 0 {
                self.tw = 0.0;
                self.tw_a = TWANG_A * 0.8;
            } else if v > sp.recoil_min {
                self.tw = 0.0;
                let v0 = if self.v0 > 0.0 { self.v0 } else { v };
                self.tw_a = TWANG_A.min(TWANG_A * v / v0) * 0.6;
            }
            if v > 0.0 {
                self.tug_count += 1;
                self.tight = true;
            }
            if self.v0 == 0.0 {
                self.v0 = v;
            }
        }
        if self.d < -sp.windup {
            self.d = -sp.windup;
            if self.v < 0.0 {
                self.v = 0.0;
            }
        }
        if self.d < 0.0 && self.wind <= 0.0 && self.target != Target::Taut {
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
        if self.wind > 0.0 {
            return true;
        }
        let moving = if self.target == Target::Taut { self.d < self.p.dt || self.v != 0.0 } else { self.d >= 0.01 };
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
// Rasterised with tiny-skia at device pixels (exact anti-aliasing; iced's MSAA on the 10-bit surface gives
// coarse edge coverage) and drawn as an image, cached by (tier, pose, colours, scale).
const KAPPA: f64 = 0.552_284_749_8;

struct Space {
    ox: f64,
    oy: f64,
    k: f64,
    mirror: bool,
}

impl Space {
    fn pt(&self, x: f64, y: f64) -> (f32, f32) {
        let x = if self.mirror { 24.0 - x } else { x };
        (((x - self.ox) * self.k) as f32, ((y - self.oy) * self.k) as f32)
    }
}

fn ellipse(sp: &Space, cx: f64, rx: f64, ry: f64) -> Option<sk::Path> {
    let (cy, k) = (12.0, KAPPA);
    let mut b = sk::PathBuilder::new();
    let m = |b: &mut sk::PathBuilder, c1: (f64, f64), c2: (f64, f64), e: (f64, f64)| {
        let (a, c, d) = (sp.pt(c1.0, c1.1), sp.pt(c2.0, c2.1), sp.pt(e.0, e.1));
        b.cubic_to(a.0, a.1, c.0, c.1, d.0, d.1);
    };
    let p = sp.pt(cx + rx, cy);
    b.move_to(p.0, p.1);
    m(&mut b, (cx + rx, cy + k * ry), (cx + k * rx, cy + ry), (cx, cy + ry));
    m(&mut b, (cx - k * rx, cy + ry), (cx - rx, cy + k * ry), (cx - rx, cy));
    m(&mut b, (cx - rx, cy - k * ry), (cx - k * rx, cy - ry), (cx, cy - ry));
    m(&mut b, (cx + k * rx, cy - ry), (cx + rx, cy - k * ry), (cx + rx, cy));
    b.close();
    b.finish()
}

/// The can body: flat front at the rim, rounded back.
fn body(sp: &Space, p: &Params, cx: f64) -> Option<sk::Path> {
    let (ym, yp, l, a, b, k) = (12.0 - p.b, 12.0 + p.b, p.l, p.a, p.b, KAPPA);
    let bx = cx - l;
    let mut q = sk::PathBuilder::new();
    let pt = |x, y| sp.pt(x, y);
    let s = pt(cx, ym);
    q.move_to(s.0, s.1);
    let e = pt(bx, ym);
    q.line_to(e.0, e.1);
    let (c1, c2, d) = (pt(bx - k * a, ym), pt(bx - a, 12.0 - k * b), pt(bx - a, 12.0));
    q.cubic_to(c1.0, c1.1, c2.0, c2.1, d.0, d.1);
    let (c1, c2, d) = (pt(bx - a, 12.0 + k * b), pt(bx - k * a, yp), pt(bx, yp));
    q.cubic_to(c1.0, c1.1, c2.0, c2.1, d.0, d.1);
    let e = pt(cx, yp);
    q.line_to(e.0, e.1);
    q.close();
    q.finish()
}

fn skc(c: Color) -> sk::Paint<'static> {
    let mut p = sk::Paint::default();
    p.set_color(sk::Color::from_rgba(c.r, c.g, c.b, c.a).unwrap_or(sk::Color::TRANSPARENT));
    p.anti_alias = true;
    p
}

/// The mark as straight-alpha RGBA, `px` x `px` device pixels for a `size` logical px box.
fn rasterise(px: u32, p: &Params, pose: Pose, can: Color, string: Color, mouth: Color) -> iced::widget::image::Handle {
    match pixels(px, p, pose, can, string, mouth) {
        Some(out) => iced::widget::image::Handle::from_rgba(px, px, out),
        None => iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 0]),
    }
}

fn pixels(px: u32, p: &Params, pose: Pose, can: Color, string: Color, mouth: Color) -> Option<Vec<u8>> {
    let mut pm = sk::Pixmap::new(px, px)?;
    let k = px as f64 / p.vb.2;
    let cx = p.c - pose.delta;
    let id = sk::Transform::identity();
    for mirror in [false, true] {
        let sp = Space { ox: p.vb.0, oy: p.vb.1, k, mirror };
        if let Some(b) = body(&sp, p, cx) {
            pm.fill_path(&b, &skc(can), sk::FillRule::Winding, id, None);
        }
        if let Some(r) = ellipse(&sp, cx, p.a, p.b) {
            pm.fill_path(&r, &skc(can), sk::FillRule::Winding, id, None);
        }
        if let Some(m) = ellipse(&sp, cx, p.mu * p.a, p.mu * p.b) {
            pm.fill_path(&m, &skc(mouth), sk::FillRule::Winding, id, None);
        }
    }
    let sp = Space { ox: p.vb.0, oy: p.vb.1, k, mirror: false };
    let x1 = p.c - pose.delta + p.ko;
    let x2 = 24.0 - x1;
    let mut q = sk::PathBuilder::new();
    let a = sp.pt(x1, 12.0);
    q.move_to(a.0, a.1);
    let c = sp.pt(12.0 + 2.0 * pose.sag * pose.phi.sin(), 12.0 + 2.0 * pose.sag * pose.phi.cos());
    let e = sp.pt(x2, 12.0);
    q.quad_to(c.0, c.1, e.0, e.1);
    if let Some(line) = q.finish() {
        let st = sk::Stroke { width: (p.stroke * k) as f32, line_cap: sk::LineCap::Round, line_join: sk::LineJoin::Round, ..Default::default() };
        pm.stroke_path(&line, &skc(string), &st, id, None);
    }
    let mut out = Vec::with_capacity(pm.data().len());
    for p in pm.pixels() {
        let c = p.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Some(out)
}

type Key = (u32, u32, [u64; 3], [u32; 12]);

/// Identical poses share one raster (a still mark rasterises once; moving marks fill and recycle the map).
fn shared(px: u32, tier: u32, pose: Pose, cols: [Color; 3], make: impl FnOnce() -> iced::widget::image::Handle) -> iced::widget::image::Handle {
    static CACHE: OnceLock<Mutex<HashMap<Key, iced::widget::image::Handle>>> = OnceLock::new();
    let c = |c: Color| [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()];
    let [a, b, d] = cols;
    let key: Key = (px, tier, [pose.delta.to_bits(), pose.sag.to_bits(), pose.phi.to_bits()], [c(a), c(b), c(d)].concat().try_into().unwrap());
    let cache = CACHE.get_or_init(Default::default);
    let mut map = cache.lock().unwrap();
    if let Some(h) = map.get(&key) {
        return h.clone();
    }
    if map.len() > 128 {
        map.clear();
    }
    let h = make();
    map.insert(key, h.clone());
    h
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
    /// Spring parameters for this use (default: the everyday pull).
    pub spring: Spring,
    /// Sign-up and first-screen marks (Mark.svelte `lineColor`): a slack line is a faded gold
    /// (gold 55 % into --line) that warms to full gold on the first tug, over --dur-switch.
    pub line_color: bool,
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
    m.set_spring(look.spring);
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

/// --dur-switch: the line colour's warm-up.
const WARM_S: f64 = 0.12;

struct Prog<M> {
    size: f32,
    look: Look,
    can: Color,
    string: Color,
    /// The faded slack colour of a `line_color` mark.
    slack: Color,
    /// Published on every tug of the string going tight (sign-up Done plays the `line` cue on it).
    on_tug: Option<M>,
}

#[derive(Default)]
struct Live {
    model: Option<Model>,
    look: Look,
    last: Option<Instant>,
    /// Tugs already reported.
    told: u64,
    /// Line colour warm-up: 0 slack colour .. 1 full gold.
    warm: f64,
}

impl<M: Clone> canvas::Program<M> for Prog<M> {
    type State = Live;

    fn update(&self, st: &mut Live, event: &iced::Event, _: Rectangle, _: mouse::Cursor) -> Option<canvas::Action<M>> {
        let iced::Event::Window(window::Event::RedrawRequested(now)) = event else { return None };
        let look = self.look;
        let mut woke = false;
        match st.model.as_mut() {
            None => {
                let m = model_for(self.size, look);
                // A mark that mounts already taut shows taut, without replaying the tug or the cue.
                st.told = m.tug_count;
                st.warm = if m.tight { 1.0 } else { 0.0 };
                st.model = Some(m);
                st.look = look;
                woke = look.state == MarkState::Ringing;
            }
            Some(m) if st.look != look => {
                let prev = st.look;
                if let Some((t, a)) = look.sway {
                    m.set_sway(t, a);
                }
                if look.spring != prev.spring {
                    m.set_spring(look.spring);
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
        let warming = |st: &Live| st.warm > 0.0 && st.warm < 1.0;
        let mut more = false;
        if m.active() || woke {
            more = m.advance(dt);
        }
        let m = st.model.as_ref()?;
        let tight = m.tight;
        let new_tug = m.tug_count > st.told;
        st.told = m.tug_count;
        // The colour warms from the first tug (reduced motion: the warm-up is a cut) and cools
        // back to the slack colour at once when the line goes slack again.
        if !tight {
            st.warm = 0.0;
        } else if st.warm < 1.0 {
            st.warm = if m.reduced { 1.0 } else { (st.warm + dt / WARM_S).max(1e-6).min(1.0) };
        }
        more |= warming(st);
        if !more && !warming(st) {
            st.last = None;
        }
        match (new_tug, &self.on_tug) {
            (true, Some(msg)) => Some(canvas::Action::publish(msg.clone())),
            _ if more => Some(canvas::Action::request_redraw()),
            _ => None,
        }
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
        let string = if self.look.line_color { crate::ui::mix(self.slack, self.string, ease_out(st.warm) as f32) } else { self.string };
        let (can, string, mouth) = (fade(self.can), fade(string), fade(MOUTH));
        // Device-pixel size and origin: exact edge pixels, no resampling.
        let sc = crate::ui::scale();
        let px = (self.size * sc).round().max(1.0) as u32;
        let at = Rectangle {
            x: (bounds.x * sc).round() / sc - bounds.x,
            y: (bounds.y * sc).round() / sc - bounds.y,
            width: px as f32 / sc,
            height: px as f32 / sc,
        };
        let pose = m.pose();
        let handle = shared(px, pick_tier(self.size), pose, [can, string, mouth], || rasterise(px, m.params(), pose, can, string, mouth));
        frame.draw_image(at, canvas::Image::new(handle).filter_method(iced::widget::image::FilterMethod::Nearest).snap(true));
        vec![frame.into_geometry()]
    }
}

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

/// The mark at `size` logical px, in the given colours. Static unless `look` changes; animates itself.
pub fn mark<'a, M: Clone + 'a>(size: f32, can: Color, string: Color, look: Look) -> Element<'a, M> {
    mark_with(size, can, string, string, look, None)
}

/// [`mark`] with the faded slack colour of a `line_color` mark and a message for each tug.
pub fn mark_with<'a, M: Clone + 'a>(size: f32, can: Color, string: Color, slack: Color, look: Look, on_tug: Option<M>) -> Element<'a, M> {
    Canvas::new(Prog { size, look, can, string, slack, on_tug }).width(Length::Fixed(size)).height(Length::Fixed(size)).into()
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

    /// Drives a connect at 60 fps; returns (tug times in s, settle time in s).
    fn pull(m: &mut Model) -> (Vec<f64>, f64) {
        m.connect();
        let (mut t, mut tugs, mut seen) = (0.0, Vec::new(), m.tug_count);
        while m.advance(1.0 / 600.0) {
            t += 1.0 / 600.0;
            if m.tug_count > seen {
                seen = m.tug_count;
                tugs.push(t);
            }
            assert!(t < 3.0, "still moving after 3 s");
        }
        (tugs, t)
    }

    #[test]
    fn connect_pulls_taut_once_and_settles() {
        // The everyday pull (MARK-MATH 2026-10-13): arrives with speed, recoils off the tight
        // string, bounces a few times smaller each time, and rests taut.
        let mut m = Model::new(Params::tier(96.0));
        let (tugs, settled) = pull(&mut m);
        assert!(tugs.len() >= 2, "the cans bounce back off the string at least once, got {}", tugs.len());
        assert!(settled < 1.2, "settles about 770 ms after connect, got {settled:.3}");
        assert_eq!(m.d, m.p.dt);
        assert!(m.pose().sag.abs() < 1e-6, "taut string has no sag");
        assert!(!m.active());
        // recoil 0 restores the old dead stop: one tug, never pulled back in.
        let mut dead = Model::new(Params::tier(96.0));
        dead.set_spring(Spring::dead_stop());
        let (tugs, _) = pull(&mut dead);
        assert_eq!(tugs.len(), 1);
        assert_eq!(dead.d, dead.p.dt);
    }

    #[test]
    fn line_spring_tugs_twice_on_the_cue_gap_and_rests() {
        let mut m = Model::new(Params::tier(96.0));
        m.set_spring(Spring::line());
        let mut give: f64 = 0.0;
        m.connect();
        let (mut t, mut tugs, mut seen) = (0.0, Vec::new(), 0);
        while m.advance(1.0 / 600.0) {
            t += 1.0 / 600.0;
            if m.tug_count > seen {
                seen = m.tug_count;
                tugs.push(t);
            }
            if tugs.len() == 1 {
                give = give.max(m.p.dt - m.d);
            }
            assert!(t < 3.0);
        }
        assert_eq!(tugs.len(), 2, "exactly two tugs");
        let gap = tugs[1] - tugs[0];
        assert!((0.10..0.16).contains(&gap), "the tugs land ~130 ms apart (cue gap 120 ms), got {gap:.3}");
        // about 1.6 px at 96 between the tugs (grid units x 4 px)
        let px = give * 96.0 / 24.0;
        assert!((1.0..2.4).contains(&px), "the line gives about 1.6 px, got {px:.2}");
        assert!(m.tight && m.d == m.p.dt && !m.active());
    }

    #[test]
    fn reduced_motion_line_still_counts_one_tug() {
        let mut m = Model::new(Params::tier(96.0));
        m.set_spring(Spring::line());
        m.reduced = true;
        m.connect();
        assert_eq!(m.tug_count, 1, "the colour warms and the cue plays");
        assert!(m.tight && m.d == m.p.dt);
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

    /// Drives the widget's update() with redraw events 16 ms apart, as the window would.
    struct Rig {
        st: Live,
        t: Instant,
        size: f32,
    }
    impl Rig {
        fn new(size: f32) -> Rig {
            Rig { st: Live::default(), t: Instant::now(), size }
        }
        /// One frame; true when the widget asked for another one.
        fn frame(&mut self, look: Look) -> bool {
            let prog = Prog::<()> { size: self.size, look, can: Color::BLACK, string: Color::BLACK, slack: Color::BLACK, on_tug: None };
            self.t += std::time::Duration::from_millis(16);
            let ev = iced::Event::Window(window::Event::RedrawRequested(self.t));
            let a = <Prog<()> as canvas::Program<()>>::update(&prog, &mut self.st, &ev, Rectangle::with_size(iced::Size::new(self.size, self.size)), mouse::Cursor::Unavailable);
            a.is_some()
        }
        fn model(&self) -> &Model {
            self.st.model.as_ref().unwrap()
        }
    }

    #[test]
    fn widget_ringing_keeps_asking_for_frames_and_the_string_moves() {
        let mut r = Rig::new(28.0);
        let look = Look { state: MarkState::Ringing, sway: Some((1.6, 12.0)), ..Look::default() };
        let mut phis = std::collections::BTreeSet::new();
        for _ in 0..100 {
            assert!(r.frame(look), "ringing must request the next frame");
            phis.insert((r.model().pose().phi * 1e4) as i64);
        }
        assert!(phis.len() > 50, "the swing angle changes frame to frame");
        assert!(r.model().pose().phi.abs() > 0.0);
    }

    #[test]
    fn widget_connect_twangs_once_then_goes_quiet_and_send_plucks() {
        let mut r = Rig::new(28.0);
        let slack = Look::default();
        assert!(!r.frame(slack), "a still mark asks for nothing");
        let taut = Look { state: MarkState::Taut, ..Look::default() };
        let mut frames = 0;
        let mut twanged = false;
        while r.frame(taut) {
            frames += 1;
            twanged |= r.model().tw >= 0.0;
            assert!(frames < 400);
        }
        assert!(twanged, "the string twangs when it goes tight");
        assert!(r.model().hit && r.model().tug_count >= 1);
        assert_eq!(r.model().pose().delta, r.model().params().dt);
        assert!(!r.frame(taut), "still again: no more frames");
        // send: pluck counter changes -> a small twang, then quiet
        let plucked = Look { pluck: 1, ..taut };
        assert!(r.frame(plucked));
        assert_eq!(r.model().tw_a, PLUCK_A);
        let mut n = 0;
        while r.frame(plucked) {
            n += 1;
            assert!(n < 100);
        }
        assert!(!r.frame(plucked));
        // and on a slack mark a pluck is a little swing
        let mut s = Rig::new(28.0);
        assert!(!s.frame(slack));
        assert!(s.frame(Look { pluck: 1, ..slack }));
        assert!(s.model().phv.abs() > 0.0);
    }

    #[test]
    fn widget_reports_each_tug_and_warms_the_line_but_not_when_mounted_taut() {
        let drive = |start: MarkState, end: MarkState| -> (usize, f64) {
            let mut st = Live::default();
            let mut t = Instant::now();
            let mut tugs = 0;
            let look = |state| Look { state, spring: Spring::line(), line_color: true, ..Look::default() };
            for i in 0..200 {
                let prog = Prog::<u8> { size: 96.0, look: look(if i == 0 { start } else { end }), can: Color::BLACK, string: Color::BLACK, slack: Color::WHITE, on_tug: Some(7) };
                t += std::time::Duration::from_millis(16);
                let ev = iced::Event::Window(window::Event::RedrawRequested(t));
                if let Some(a) = <Prog<u8> as canvas::Program<u8>>::update(&prog, &mut st, &ev, Rectangle::with_size(iced::Size::new(96.0, 96.0)), mouse::Cursor::Unavailable) {
                    if a.into_inner().0 == Some(7) {
                        tugs += 1;
                    }
                }
            }
            (tugs, st.warm)
        };
        let (tugs, warm) = drive(MarkState::Slack, MarkState::Taut);
        assert_eq!((tugs, warm), (2, 1.0), "two tugs reported, the line fully warm");
        let (tugs, warm) = drive(MarkState::Taut, MarkState::Taut);
        assert_eq!((tugs, warm), (0, 1.0), "mounted taut: no replayed tug or cue");
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
