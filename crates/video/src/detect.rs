//! Is the queueing delay in the receiver's reports a queue we feed? Two answers for
//! [`crate::media::Sender`] to choose from (`SenderConfig::detector`):
//!
//! - [`Detector::Fixed`]: any report over `overuse_us` (40 ms) is overuse. Simple and quick on a
//!   clean path, but a path whose delay moves by itself (a mobile downlink's scheduler, a busy
//!   receiver) crosses 40 ms several times a second at any rate: the 2026-10-19 moto -> mobile
//!   call sat at 180p, ~100 kbps, with 3-5 overuse reports a second on a fast path.
//! - [`Detector::Trend`]: Google congestion control's delay-based detector (libwebrtc
//!   `TrendlineEstimator`, draft-ietf-rmcat-gcc-02). A queue we feed grows for as long as we
//!   send above the link's rate, so the delay's *trend* over the last second is what counts,
//!   compared with a threshold that follows the trend's own noise: up slowly when the trend
//!   swings wider (a noisy path), down when it calms, never adapting to a jump far above it
//!   (that is a real queue). A delay that stands still, however high, has no trend.

use std::collections::VecDeque;

/// Which overuse detector the sender's rate control uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Detector {
    #[default]
    Fixed,
    Trend,
}

/// What the delay says about the path, per report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Usage {
    #[default]
    Normal,
    /// A queue is building.
    Over,
    /// The queue is shrinking.
    Under,
}

/// Reports in the regression: 20 at the receiver's 50 ms (one second).
const WINDOW: usize = 20;
/// Exponential smoothing of the delay before the regression (libwebrtc: 0.9 per packet group;
/// ours come every 50 ms, a few groups' worth, so a little less).
const SMOOTHING: f64 = 0.8;
/// The trend (ms of delay per ms) is scaled by min(samples, 60) x this before the threshold.
const GAIN: f64 = 4.0;
const MAX_SAMPLES: f64 = 60.0;
/// Threshold (ms of scaled trend): start, bounds, and how fast it follows (per ms, up and down).
const GAMMA_START: f64 = 12.5;
const GAMMA_MIN: f64 = 6.0;
const GAMMA_MAX: f64 = 600.0;
const K_UP: f64 = 0.0087;
const K_DOWN: f64 = 0.039;
/// A trend this much over the threshold is a queue, not noise: the threshold ignores it.
const MAX_ADAPT_JUMP: f64 = 15.0;
/// Overuse once the trend has been over the threshold this long (ms), on two reports or more.
const OVER_TIME_MS: f64 = 10.0;

#[derive(Clone, Debug)]
pub struct Trendline {
    /// (report time ms, smoothed delay ms), oldest first.
    window: VecDeque<(f64, f64)>,
    smoothed: Option<f64>,
    samples: f64,
    gamma: f64,
    last_ms: Option<f64>,
    over_ms: f64,
    over_n: u32,
    prev_trend: f64,
    usage: Usage,
    /// The last scaled trend (ms), for logs.
    pub modified: f64,
}

impl Default for Trendline {
    fn default() -> Self {
        Self {
            window: VecDeque::with_capacity(WINDOW + 1),
            smoothed: None,
            samples: 0.0,
            gamma: GAMMA_START,
            last_ms: None,
            over_ms: -1.0,
            over_n: 0,
            prev_trend: 0.0,
            usage: Usage::Normal,
            modified: 0.0,
        }
    }
}

impl Trendline {
    /// A report at `now_us` with `queuing_us` of queueing delay (baseline already taken off).
    pub fn update(&mut self, now_us: u64, queuing_us: u64) -> Usage {
        let t = now_us as f64 / 1000.0;
        let q = queuing_us as f64 / 1000.0;
        let dt = self.last_ms.map_or(0.0, |l| (t - l).max(0.0));
        self.last_ms = Some(t);
        let s = match self.smoothed {
            Some(s) => SMOOTHING * s + (1.0 - SMOOTHING) * q,
            None => q,
        };
        self.smoothed = Some(s);
        self.window.push_back((t, s));
        if self.window.len() > WINDOW {
            self.window.pop_front();
        }
        self.samples = (self.samples + 1.0).min(MAX_SAMPLES);
        if self.window.len() < WINDOW {
            return self.usage;
        }
        let trend = slope(&self.window).unwrap_or(self.prev_trend);
        self.detect(trend, dt);
        self.usage
    }

    pub fn usage(&self) -> Usage {
        self.usage
    }

    /// The threshold now (ms of scaled trend), for logs.
    pub fn threshold(&self) -> f64 {
        self.gamma
    }

    fn detect(&mut self, trend: f64, dt: f64) {
        let m = self.samples * trend * GAIN;
        self.modified = m;
        if m > self.gamma {
            if self.over_ms < 0.0 {
                // First report over it: count half its interval.
                self.over_ms = dt / 2.0;
            } else {
                self.over_ms += dt;
            }
            self.over_n += 1;
            if self.over_ms > OVER_TIME_MS && self.over_n > 1 && trend >= self.prev_trend {
                self.over_ms = 0.0;
                self.over_n = 0;
                self.usage = Usage::Over;
            }
        } else if m < -self.gamma {
            self.over_ms = -1.0;
            self.over_n = 0;
            self.usage = Usage::Under;
        } else {
            self.over_ms = -1.0;
            self.over_n = 0;
            self.usage = Usage::Normal;
        }
        self.prev_trend = trend;
        self.adapt(m, dt);
    }

    fn adapt(&mut self, m: f64, dt: f64) {
        if m.abs() > self.gamma + MAX_ADAPT_JUMP {
            return;
        }
        let k = if m.abs() < self.gamma { K_DOWN } else { K_UP };
        // libwebrtc steps `k * dt` per packet group (~5 ms apart); ours are 50 ms apart, where
        // `K_DOWN * 50` is 1.95 and the step would overshoot. The same time constants, exactly.
        self.gamma += (1.0 - (-k * dt.min(100.0)).exp()) * (m.abs() - self.gamma);
        self.gamma = self.gamma.clamp(GAMMA_MIN, GAMMA_MAX);
    }
}

/// Least-squares slope of delay over time.
fn slope(w: &VecDeque<(f64, f64)>) -> Option<f64> {
    let n = w.len() as f64;
    let (mx, my) = w.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (mx / n, my / n);
    let (num, den) = w.iter().fold((0.0, 0.0), |a, p| (a.0 + (p.0 - mx) * (p.1 - my), a.1 + (p.0 - mx).powi(2)));
    (den > 0.0).then(|| num / den)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(d: &mut Trendline, from_ms: u64, n: u64, q: impl Fn(u64) -> u64) -> Vec<Usage> {
        (0..n).map(|i| d.update((from_ms + i * 50) * 1000, q(i) * 1000)).collect()
    }

    #[test]
    fn steady_delay_is_not_overuse_however_high() {
        let mut d = Trendline::default();
        let u = feed(&mut d, 0, 200, |_| 150);
        assert!(u.iter().all(|&u| u == Usage::Normal), "{u:?}");
    }

    #[test]
    fn growing_queue_is_overuse_within_half_a_second() {
        let mut d = Trendline::default();
        feed(&mut d, 0, 40, |_| 5);
        // Sending 20% over the link: +200 ms of queue a second, +10 ms a report.
        let u = feed(&mut d, 2_000, 20, |i| 5 + 10 * i);
        let first = u.iter().position(|&u| u == Usage::Over);
        assert!(first.is_some_and(|i| i <= 10), "{u:?}");
    }

    #[test]
    fn draining_queue_is_underuse() {
        let mut d = Trendline::default();
        feed(&mut d, 0, 40, |i| 5 + 5 * i);
        let u = feed(&mut d, 2_000, 30, |i| 200u64.saturating_sub(10 * i));
        assert!(u.contains(&Usage::Under), "{u:?}");
    }

    #[test]
    fn threshold_rises_on_a_noisy_path() {
        let mut d = Trendline::default();
        feed(&mut d, 0, 40, |_| 10);
        let calm = d.threshold();
        // Up 100 ms over a second and back, again and again.
        let mut top: f64 = 0.0;
        for i in 0..400 {
            d.update((2_000 + i * 50) * 1000, (10 + (i % 40).abs_diff(20) * 5) * 1000);
            top = top.max(d.threshold());
        }
        assert!(calm < 7.0 && top > 15.0, "calm {calm}, top {top}");
    }
}
