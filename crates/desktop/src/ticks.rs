//! Receipt ticks drawn as the mark's string (the user's pick from the playground's `?lab=ticks`, preset
//! "crisp", string variant at every size). Geometry and the spring's morph curves come from
//! crates/tin-icons (generated `tin_ticks.rs`); this file only picks the visual state and plays the
//! one-shot transition between two states:
//!
//! - slack → one: the slack string is pulled taut into the tick (`PULL`);
//! - one → two: the second string pulls out of the first, sliding to its offset as it goes taut;
//! - two → read: both strings pluck once (`PLUCK`) while the colour eases to the read ink;
//! - waiting → not delivered: a colour ease only (same slack shape).
//!
//! Nothing loops; reduced motion shows the end state at once. The glyph's slot is fixed (two ticks wide),
//! so nothing moves as the state advances.

use iced::widget::image;
use iced::{Color, Element, Length};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::motion::{dur, Ease};
use crate::tin_ticks as g;

/// What a bubble shows, and the state-change rules: decided in core (`p2pcore::tick_vis`,
/// `tick_transition`, `tick_pose`) so desktop and Android play the same morphs.
pub use p2pcore::TickVis as Vis;

#[derive(Debug, Clone, Copy)]
pub struct Anim {
    pub from: Vis,
    pub start: Instant,
}

/// Inks for one bubble: the meta ink, read, not delivered, and the bubble itself (the knot's halo).
#[derive(Debug, Clone, Copy)]
pub struct Inks {
    pub meta: Color,
    pub read: Color,
    pub bad: Color,
    pub bg: Color,
}

fn ink(v: Vis, i: &Inks) -> Color {
    match v {
        Vis::Read => i.read,
        Vis::Bad => i.bad,
        _ => i.meta,
    }
}

/// One frame: string 1 morph, string 2 morph and offset fraction (None = hidden), colour mix 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pose {
    k0: f32,
    k1: Option<(f32, f32)>,
    mix: f32,
}

fn curve(c: &[f32], secs: f32) -> Option<f32> {
    let i = (secs * g::FRAME_HZ) as usize;
    (i < c.len()).then(|| c[i])
}

fn from_core(p: p2pcore::TickPose, mix: f32) -> Pose {
    Pose { k0: p.k0, k1: p.second.then_some((p.k1, p.k1_offset)), mix }
}

fn rest(to: Vis) -> Pose {
    from_core(p2pcore::tick_pose(to, to, None), 1.0)
}

/// The transition from `from` to `to`, `secs` after it started; None once it has settled. The rule
/// (which curve, whether the ink eases, the shape per sample) is core's; the curves, the colour ease
/// and the clock are ours.
fn pose(from: Vis, to: Vis, secs: f32) -> Option<Pose> {
    let tr = p2pcore::tick_transition(from, to);
    let cms = dur::OVERLAY_OPEN / 1000.0;
    let mix = if tr.colour { Ease::Out.apply((secs / cms).min(1.0)) } else { 1.0 };
    let color_running = tr.colour && secs < cms;
    let x = match tr.curve {
        p2pcore::TickCurve::Pull => curve(g::PULL, secs),
        p2pcore::TickCurve::Pluck => curve(g::PLUCK, secs),
        p2pcore::TickCurve::None => None,
    };
    (x.is_some() || color_running).then(|| from_core(p2pcore::tick_pose(from, to, x), mix))
}

/// Seconds into a transition. Test-hooks `P2P_TICK_AT_MS=n` freezes every transition at n ms (mid-morph screenshots).
fn elapsed(a: &Anim, now: Instant) -> f32 {
    if let Some(ms) = crate::test_env("P2P_TICK_AT_MS").and_then(|v| v.parse::<f32>().ok()) {
        return ms / 1000.0;
    }
    now.saturating_duration_since(a.start).as_secs_f32()
}

/// Whether a transition started at `a` is still running at `now`.
pub fn running(a: &Anim, to: Vis, now: Instant) -> bool {
    !crate::motion::reduced() && pose(a.from, to, elapsed(a, now)).is_some()
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgba(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t)
}

fn hex(c: Color) -> String {
    let v = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", v(c.r), v(c.g), v(c.b))
}

fn path(k: f32, ox: f32) -> String {
    let mut d = String::with_capacity(g::N * 14);
    for i in 0..g::N {
        let (s, t) = (g::SLACK[i], g::TICK[i]);
        let x = s[0] + (t[0] - s[0]) * k + ox;
        let y = s[1] + (t[1] - s[1]) * k;
        d.push_str(&format!("{}{:.3} {:.3}", if i == 0 { 'M' } else { 'L' }, x, y));
    }
    d
}

fn markup(p: &Pose, ink: Color, bg: Color) -> String {
    let (c, b) = (hex(ink), hex(bg));
    let mut s = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{} 0 {} {}" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="{}" stroke="{c}" stroke-width="{}"/>"#,
        g::VB_X, g::VB_W, g::VB_H, path(p.k0, 0.0), g::SW
    );
    if let Some((k, f)) = p.k1 {
        let d = path(k, g::DX * f);
        if g::GAP > 0.0 {
            s.push_str(&format!(r#"<path d="{d}" stroke="{b}" stroke-width="{}"/>"#, g::SW + 2.0 * g::GAP));
        }
        s.push_str(&format!(r#"<path d="{d}" stroke="{c}" stroke-width="{}"/>"#, g::SW));
    }
    s.push_str("</svg>");
    s
}

fn render(svg: &str, w: u32, h: u32, alpha: f32) -> image::Handle {
    use resvg::{tiny_skia, usvg};
    let blank = || image::Handle::from_rgba(1, 1, vec![0, 0, 0, 0]);
    let Ok(tree) = usvg::Tree::from_str(svg, &usvg::Options::default()) else { return blank() };
    let Some(mut pm) = tiny_skia::Pixmap::new(w.max(1), h.max(1)) else { return blank() };
    let ts = tiny_skia::Transform::from_scale(w as f32 / tree.size().width(), h as f32 / tree.size().height());
    resvg::render(&tree, ts, &mut pm.as_mut());
    let mut out = Vec::with_capacity(pm.data().len());
    for p in pm.pixels() {
        let c = p.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), (c.alpha() as f32 * alpha) as u8]);
    }
    image::Handle::from_rgba(w.max(1), h.max(1), out)
}

/// The glyph `size` px tall (slot `size · VB_W / 24` wide) for `to`, playing `anim` if it is running.
pub fn glyph<'a, M: 'a>(to: Vis, anim: Option<&Anim>, inks: Inks, size: f32, now: Instant) -> Element<'a, M> {
    let w = size * g::VB_W / g::VB_H;
    let sc = crate::ui::scale();
    let (pw, ph) = ((w * sc).ceil() as u32, (size * sc).ceil() as u32);
    let live = anim.filter(|_| !crate::motion::reduced()).and_then(|a| pose(a.from, to, elapsed(a, now)).map(|p| (a.from, p)));
    let handle = match live {
        Some((from, p)) => {
            let ink = mix(ink(from, &inks), ink(to, &inks), p.mix);
            render(&markup(&p, ink, inks.bg), pw, ph, ink.a)
        }
        None => {
            // Settled states are cached (a handful per theme/scale).
            type Key = (Vis, u32, u32, [u8; 8]);
            static CACHE: OnceLock<Mutex<HashMap<Key, image::Handle>>> = OnceLock::new();
            let ink = ink(to, &inks);
            let q = |c: Color| [c.r, c.g, c.b, c.a].map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
            let (a, b) = (q(ink), q(inks.bg));
            let key = (to, pw, ph, [a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]]);
            let cache = CACHE.get_or_init(Default::default);
            let hit = cache.lock().unwrap().get(&key).cloned();
            hit.unwrap_or_else(|| {
                let h = render(&markup(&rest(to), ink, inks.bg), pw, ph, ink.a);
                cache.lock().unwrap().insert(key, h.clone());
                h
            })
        }
    };
    image(handle).width(Length::Fixed(w)).height(Length::Fixed(size)).into()
}

/// The screen-reader / tooltip words for a state (VOICE.md: says how things are).
#[allow(dead_code)]
pub fn label(v: Vis) -> &'static str {
    match v {
        Vis::Slack => "Waiting to send",
        Vis::One => "Sent",
        Vis::Two => "Delivered",
        Vis::Read => "Read",
        Vis::Bad => "Not delivered",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions_are_one_shot_and_end_at_the_target_shape() {
        for (from, to) in [(Vis::Slack, Vis::One), (Vis::One, Vis::Two), (Vis::Two, Vis::Read), (Vis::Slack, Vis::Bad), (Vis::Slack, Vis::Two)] {
            assert!(pose(from, to, 0.0).is_some(), "{from:?} -> {to:?} animates");
            assert!(pose(from, to, 3.0).is_none(), "{from:?} -> {to:?} settles (no loop)");
        }
        assert!(pose(Vis::Two, Vis::Two, 0.0).is_none(), "no change, no animation");
        // Slack -> one starts slack and ends taut; read keeps both strings.
        let p = pose(Vis::Slack, Vis::One, 0.0).unwrap();
        assert!(p.k0 < 0.3 && p.k1.is_none());
        assert_eq!(rest(Vis::Read), Pose { k0: 1.0, k1: Some((1.0, 1.0)), mix: 1.0 });
        // Waiting -> not delivered: colour only, the shape stays slack.
        let b = pose(Vis::Slack, Vis::Bad, 0.1).unwrap();
        assert_eq!((b.k0, b.k1), (0.0, None));
        assert!(b.mix > 0.0 && b.mix < 1.0);
    }

    #[test]
    fn every_state_draws_the_same_path_commands() {
        let cmds = |s: String| s.chars().filter(|c| c.is_ascii_alphabetic()).collect::<String>();
        assert_eq!(cmds(path(0.0, 0.0)), cmds(path(1.0, g::DX)));
        assert_eq!(cmds(path(0.0, 0.0)).len(), g::N);
    }
}
