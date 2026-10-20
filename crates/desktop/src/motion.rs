//! Motion tokens and tweens, ported from the playground's lib/fx/motion.ts (the only place
//! durations and easings live) and lib/calls/motion.ts (the cubic-bezier solver). Reduced motion
//! is the one system signal in `mark::reduced_motion`; a reduced tween settles at once.

use std::time::Instant;

/// Durations in ms (motion.ts `DUR`).
pub mod dur {
    pub const CROSSFADE: f32 = 120.0;
    pub const OVERLAY_OPEN: f32 = 240.0;
    pub const INDICATOR: f32 = 200.0;
}

/// Easing curves (motion.ts `EASE`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ease {
    /// cubic-bezier(.2,.8,.2,1)
    Out,
    /// cubic-bezier(.4,0,.2,1)
    InOut,
    Linear,
}

/// The same Newton solve as lib/calls/motion.ts `bezier()`: 8 iterations, 1e-5 tolerance.
pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;
    let fx = |t: f32| ((ax * t + bx) * t + cx) * t;
    let fy = |t: f32| ((ay * t + by) * t + cy) * t;
    let mut t = x;
    for _ in 0..8 {
        let e = fx(t) - x;
        if e.abs() < 1e-5 {
            break;
        }
        let d = (3.0 * ax * t + 2.0 * bx) * t + cx;
        if d.abs() < 1e-6 {
            break;
        }
        t -= e / d;
    }
    fy(t.clamp(0.0, 1.0))
}

impl Ease {
    pub fn apply(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Ease::Out => bezier(0.2, 0.8, 0.2, 1.0, x),
            Ease::InOut => bezier(0.4, 0.0, 0.2, 1.0, x),
            Ease::Linear => x,
        }
    }
}

pub fn reduced() -> bool {
    crate::mark::reduced_motion()
}

/// One animated value. `go` starts from wherever the value is now, so a retarget mid-flight
/// glides instead of jumping (the desktop nav highlight, fades that reverse).
#[derive(Debug, Clone, Copy)]
pub struct Tween {
    from: f32,
    to: f32,
    start: Option<Instant>,
    delay_ms: f32,
    ms: f32,
    ease: Ease,
}

impl Tween {
    pub const fn settled(v: f32) -> Self {
        Tween { from: v, to: v, start: None, delay_ms: 0.0, ms: 0.0, ease: Ease::Linear }
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn at(&self, now: Instant) -> f32 {
        let Some(start) = self.start else { return self.to };
        let el = now.saturating_duration_since(start).as_secs_f32() * 1000.0 - self.delay_ms;
        if el <= 0.0 {
            return self.from;
        }
        if self.ms <= 0.0 || el >= self.ms {
            return self.to;
        }
        self.from + (self.to - self.from) * self.ease.apply(el / self.ms)
    }

    pub fn running(&self, now: Instant) -> bool {
        self.start.is_some_and(|s| now.saturating_duration_since(s).as_secs_f32() * 1000.0 < self.delay_ms + self.ms)
    }

    /// Animate to `to`. Reduced motion settles immediately (CSS `data-motion=reduce` makes
    /// transitions 1 ms); `set` jumps without animating (first layout, like `.hl` before `ready`).
    pub fn go(&mut self, to: f32, ms: f32, ease: Ease, now: Instant) {
        self.go_after(to, 0.0, ms, ease, now);
    }

    pub fn go_after(&mut self, to: f32, delay_ms: f32, ms: f32, ease: Ease, now: Instant) {
        if (to - self.to).abs() < f32::EPSILON && self.start.is_none_or(|_| !self.running(now)) {
            self.to = to;
            return;
        }
        if reduced() {
            self.set(to);
            return;
        }
        self.from = self.at(now);
        self.to = to;
        self.start = Some(now);
        self.delay_ms = delay_ms;
        self.ms = ms;
        self.ease = ease;
    }

    pub fn set(&mut self, v: f32) {
        *self = Tween::settled(v);
    }
}

impl Default for Tween {
    fn default() -> Self {
        Tween::settled(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn motion_bezier_matches_css_endpoints_and_midpoints() {
        for e in [Ease::Out, Ease::InOut] {
            assert!(e.apply(0.0).abs() < 1e-4);
            assert!((e.apply(1.0) - 1.0).abs() < 1e-4);
        }
        // Exact curve values at x = .5 (bisection on the bezier): Out .9461, InOut .7756.
        assert!((Ease::Out.apply(0.5) - 0.9461).abs() < 1e-3, "{}", Ease::Out.apply(0.5));
        assert!((Ease::InOut.apply(0.5) - 0.7756).abs() < 1e-3, "{}", Ease::InOut.apply(0.5));
    }

    #[test]
    fn motion_tween_retarget_starts_from_current_value() {
        let t0 = Instant::now();
        let mut tw = Tween::settled(0.0);
        tw.go(46.0, dur::INDICATOR, Ease::Out, t0);
        let mid = t0 + Duration::from_millis(100);
        let v = tw.at(mid);
        assert!(v > 0.0 && v < 46.0);
        tw.go(92.0, dur::INDICATOR, Ease::Out, mid);
        assert!((tw.at(mid) - v).abs() < 1e-3, "no jump on retarget");
        assert_eq!(tw.at(mid + Duration::from_millis(250)), 92.0);
        assert!(!tw.running(mid + Duration::from_millis(250)));
    }
}

/// The app's running tweens and the frame clock the views read (`now` advances on each
/// `window::frames()` tick while anything runs; nothing ticks when everything has settled).
#[derive(Debug, Clone, Copy)]
pub struct Fx {
    pub now: Instant,
    /// DesktopShell `.hl`: y of the highlight that glides between sidebar items.
    pub nav: Tween,
    /// SearchField `.clr`: opacity of the clear button (crossfade).
    pub clear: Tween,
    /// Settings/profile `Layer`: 0 closed .. 1 open (enter/exit).
    pub layer: Tween,
    /// SignalBars.svelte: each bar's opacity (ghost .28, lit 1), 300 ms fades.
    pub bars: [Tween; 4],
    /// SignalBars.svelte `.wave`: reconnecting, a slow wave through the ghosts since this instant.
    pub wave: Option<Instant>,
}

impl Default for Fx {
    fn default() -> Self {
        Fx { now: Instant::now(), nav: Tween::settled(0.0), clear: Tween::settled(0.0), layer: Tween::settled(0.0), bars: [Tween::settled(GHOST); 4], wave: None }
    }
}

impl Fx {
    pub fn running(&self) -> bool {
        let now = Instant::now();
        self.wave.is_some() || [self.nav, self.clear, self.layer].iter().chain(&self.bars).any(|t| t.running(now))
    }

    /// Light the first `level` bars (0 = all ghosts), or run the reconnecting wave. Fades 300 ms.
    pub fn signal(&mut self, level: u8, wave: bool) {
        let now = Instant::now();
        let wave = wave && !reduced();
        if wave != self.wave.is_some() {
            self.now = now;
            self.wave = wave.then_some(now);
        }
        for (i, b) in self.bars.iter_mut().enumerate() {
            let to = if !wave && (i as u8) < level { 1.0 } else { GHOST };
            if b.target() != to {
                self.now = now;
                b.go(to, 300.0, Ease::Out, now);
            }
        }
    }

    /// Each bar's opacity now. Wave: `sbw` keyframes 0/60/100 % ghost, 25 % .7; 1.6 s, bars 160 ms apart.
    pub fn bar_alphas(&self) -> [f32; 4] {
        let Some(start) = self.wave else { return self.bars.map(|b| b.at(self.now)) };
        let el = self.now.saturating_duration_since(start).as_secs_f32() * 1000.0;
        std::array::from_fn(|i| {
            let t = el - i as f32 * 160.0;
            if t < 0.0 { return GHOST; }
            let p = (t % 1600.0) / 1600.0;
            if p < 0.25 { GHOST + (0.7 - GHOST) * Ease::InOut.apply(p / 0.25) }
            else if p < 0.6 { 0.7 + (GHOST - 0.7) * Ease::InOut.apply((p - 0.25) / 0.35) }
            else { GHOST }
        })
    }
}

/// SignalBars.svelte ghost: unlit bars stay at 28 % so the glyph never changes size.
pub const GHOST: f32 = 0.28;
