//! Renderer-independent experimental motion maths. No audio callbacks, heap allocation,
//! transport, avatar/UI dependencies, or production defaults. Input is a playout observation.
use std::f64::consts::TAU;

pub const MAX_SOURCES: usize = 32;
pub const CONTOURS: usize = 4;
pub const SEGMENTS: usize = 8;
pub const PARTICLES: usize = 32;
pub const TRAIL: usize = 12;
const ARC_SAMPLES: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
impl std::ops::Sub for Point {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        Self {
            x: self.x - b.x,
            y: self.y - b.y,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cubic {
    pub start: Point,
    pub c1: Point,
    pub c2: Point,
    pub end: Point,
}

#[derive(Clone, Copy, Debug)]
pub struct ShapeFeatures {
    pub low: f64,
    pub high: f64,
    pub centroid_hz: f64,
    pub onset: f64,
}
impl Default for ShapeFeatures {
    fn default() -> Self {
        Self {
            low: 1.0 / 3.0,
            high: 1.0 / 3.0,
            centroid_hz: 2000.0,
            onset: 0.0,
        }
    }
}

#[derive(Clone, Copy)]
struct Source {
    level: u8,
    audible: bool,
    muted: bool,
    at: f64,
    features: ShapeFeatures,
}
impl Default for Source {
    fn default() -> Self {
        Self {
            level: 127,
            audible: false,
            muted: false,
            at: f64::NEG_INFINITY,
            features: ShapeFeatures::default(),
        }
    }
}

/// Experimental tunables, deliberately not app/product defaults.
#[derive(Clone, Copy)]
pub struct Tunables {
    pub attack_s: f64,
    pub release_s: f64,
    pub slow_attack_s: f64,
    pub slow_release_s: f64,
    pub stale_s: f64,
    pub travel_per_s: f64,
}
impl Default for Tunables {
    fn default() -> Self {
        Self {
            attack_s: 0.09,
            release_s: 0.42,
            slow_attack_s: 0.8,
            slow_release_s: 1.5,
            stale_s: 0.2,
            travel_per_s: 0.028,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Particle {
    pub distance: f64,
    pub contour: usize,
    pub trail: [Point; TRAIL],
    pub trail_len: usize,
    pub head: usize,
}

pub struct Model {
    sources: [Source; MAX_SOURCES],
    roster: [bool; MAX_SOURCES],
    active: bool,
    now: f64,
    trail_elapsed: f64,
    pub fast: f64,
    pub slow: f64,
    pub phase: f64,
    pub spectral_balance: f64,
    pub onset: f64,
    reduced: bool,
    geometry_envelope: [f64; 2],
    pub aspect: f64,
    pub tune: Tunables,
    pub particles: [Particle; PARTICLES],
}

impl Model {
    pub fn new() -> Self {
        let mut model = Self {
            sources: [Source::default(); MAX_SOURCES],
            roster: [true; MAX_SOURCES],
            active: true,
            now: 0.0,
            trail_elapsed: 0.0,
            fast: 0.0,
            slow: 0.0,
            phase: 0.0,
            spectral_balance: 0.5,
            onset: 0.0,
            reduced: false,
            geometry_envelope: [0.0; 2],
            aspect: 2.0,
            tune: Tunables::default(),
            particles: std::array::from_fn(|i| Particle {
                distance: (i / CONTOURS) as f64 / (PARTICLES / CONTOURS) as f64,
                contour: i % CONTOURS,
                trail: [Point::default(); TRAIL],
                trail_len: 0,
                head: 0,
            }),
        };
        let maps: [ArcMap; CONTOURS] =
            std::array::from_fn(|i| ArcMap::new(|x| model.point(i, x), model.aspect));
        // Persistent pool exists from frame zero, including reduced-motion startup.
        for p in &mut model.particles {
            let map = &maps[p.contour];
            p.trail = std::array::from_fn(|j| {
                map.sample(
                    (p.distance
                        - model.tune.travel_per_s * (TRAIL - 1 - j) as f64 / 30.0 / map.length())
                    .rem_euclid(1.0),
                )
            });
            p.trail_len = TRAIL;
        }
        model
    }
    pub fn set_reduced(&mut self, reduced: bool) {
        self.reduced = reduced;
    }
    pub fn is_reduced(&self) -> bool {
        self.reduced
    }
    pub fn time(&self) -> f64 {
        self.now
    }
    pub fn feed(&mut self, source: usize, level: u8, audible: bool, muted: bool, at: f64) -> bool {
        if source >= MAX_SOURCES
            || !self.roster[source]
            || !self.active
            || level > 127
            || !at.is_finite()
            || at > self.now + 1e-6
            || at < self.sources[source].at
        {
            return false;
        }
        self.sources[source] = Source {
            level,
            audible,
            muted,
            at,
            features: ShapeFeatures::default(),
        };
        true
    }
    pub fn feed_spectrum(
        &mut self,
        source: usize,
        level: u8,
        audible: bool,
        muted: bool,
        at: f64,
        features: ShapeFeatures,
    ) -> bool {
        if ![
            features.low,
            features.high,
            features.centroid_hz,
            features.onset,
        ]
        .iter()
        .all(|v| v.is_finite())
            || !(0.0..=1.0).contains(&features.low)
            || !(0.0..=1.0).contains(&features.high)
            || features.low + features.high > 1.000001
            || !(0.0..=24000.0).contains(&features.centroid_hz)
            || !(0.0..=1.0).contains(&features.onset)
        {
            return false;
        }
        if !self.feed(source, level, audible, muted, at) {
            return false;
        }
        self.sources[source].features = features;
        true
    }
    fn spectral_target(&self) -> (f64, f64) {
        let (mut weight, mut balance, mut onset) = (0.0, 0.0, 0.0_f64);
        if self.active {
            for (i, s) in self.sources.iter().enumerate() {
                if self.roster[i] && s.audible && !s.muted && self.now - s.at <= self.tune.stale_s {
                    let w = ((60.0 - s.level as f64) / 50.0).clamp(0.0, 1.0).powi(2);
                    let f = s.features;
                    let b = 0.65 * f.high / (f.low + f.high).max(1e-12)
                        + 0.35 * (f.centroid_hz / 4000.0).clamp(0.0, 1.0);
                    weight += w;
                    balance += w * b;
                    if w > 0.0 {
                        onset = onset.max(f.onset);
                    }
                }
            }
        }
        (if weight > 0.0 { balance / weight } else { 0.5 }, onset)
    }
    pub fn set_roster(&mut self, sources: &[usize]) {
        self.roster.fill(false);
        for &s in sources {
            if s < MAX_SOURCES {
                self.roster[s] = true;
            }
        }
        for i in 0..MAX_SOURCES {
            if !self.roster[i] {
                self.sources[i] = Source::default();
            }
        }
    }
    pub fn end_call(&mut self) {
        self.active = false;
        self.sources.fill(Source::default());
    }
    pub fn start_call(&mut self) {
        self.active = true;
        self.sources.fill(Source::default());
    }
    pub fn target(&self) -> f64 {
        if !self.active {
            return 0.0;
        }
        let mut power = 0.0;
        for (i, s) in self.sources.iter().enumerate() {
            if self.roster[i] && s.audible && !s.muted && self.now - s.at <= self.tune.stale_s {
                // Display transfer curve from existing -dBov bytes, NOT measured loudness.
                let v = ((60.0 - s.level as f64) / 50.0).clamp(0.0, 1.0);
                power += v * v;
            }
        }
        power.sqrt().min(1.0)
    }
    pub fn step(&mut self, dt: f64) {
        assert!(dt.is_finite() && dt >= 0.0);
        self.now += dt;
        let target = self.target();
        self.fast = smooth(
            self.fast,
            target,
            dt,
            self.tune.attack_s,
            self.tune.release_s,
        );
        self.slow = smooth(
            self.slow,
            target,
            dt,
            self.tune.slow_attack_s,
            self.tune.slow_release_s,
        );
        if !self.reduced && self.active {
            let (balance, onset) = self.spectral_target();
            self.spectral_balance = smooth(self.spectral_balance, balance, dt, 0.35, 0.6);
            self.onset = smooth(self.onset, onset, dt, 0.12, 0.7);
            self.geometry_envelope[0] = smooth(self.geometry_envelope[0], self.fast, dt, 0.15, 0.2);
            self.geometry_envelope[1] = smooth(self.geometry_envelope[1], self.slow, dt, 0.35, 0.6);
            self.phase += dt * (0.18 + 0.06 * self.geometry_envelope[1] + 0.015 * self.onset);
            let maps: [ArcMap; CONTOURS] =
                std::array::from_fn(|i| ArcMap::new(|x| self.point(i, x), self.aspect));
            self.trail_elapsed += dt;
            let sample_trail = self.trail_elapsed + 1e-9 >= 1.0 / 30.0;
            if sample_trail {
                self.trail_elapsed %= 1.0 / 30.0;
            }
            for p in &mut self.particles {
                let map = &maps[p.contour];
                p.distance = (p.distance
                    + dt * self.tune.travel_per_s
                        * (1.0 + 0.2 * self.geometry_envelope[1] + 0.1 * self.onset)
                        / map.length())
                .rem_euclid(1.0);
                if sample_trail {
                    p.trail[p.head] = map.sample(p.distance);
                    p.head = (p.head + 1) % TRAIL;
                    p.trail_len = (p.trail_len + 1).min(TRAIL);
                }
            }
        }
    }
    pub fn opacity(&self) -> f64 {
        // Reduced motion changes presence only, never travel/deformation.
        ((if self.active { 0.055 } else { 0.0 }) + 0.12 * self.fast + 0.055 * self.slow).min(0.25)
    }
    fn y_and_slope(&self, contour: usize, x: f64) -> (f64, f64) {
        let base = 0.28 + contour as f64 * 0.13;
        let phase = self.phase;
        let [fast, slow] = self.geometry_envelope;
        let a = TAU * x + phase + contour as f64 * 0.45;
        let b = 2.0 * TAU * x - phase * 0.6 + contour as f64 * 0.25;
        let c = 3.0 * TAU * x + phase * 0.4;
        let balance = self.spectral_balance;
        let onset = self.onset;
        let second = 0.18 + 0.24 * balance;
        let third = 0.4 + 0.6 * balance;
        let d = 4.0 * TAU * x + phase * 0.2;
        let amplitude = 0.016 + slow * 0.009;
        let y = base
            + amplitude * ((1.0 - second) * a.sin() + second * b.sin())
            + fast * 0.012 * third * c.sin()
            + onset * 0.005 * d.sin();
        let slope = amplitude * ((1.0 - second) * TAU * a.cos() + second * 2.0 * TAU * b.cos())
            + fast * 0.036 * TAU * third * c.cos()
            + onset * 0.02 * TAU * d.cos();
        (y, slope)
    }
    pub fn point(&self, contour: usize, x: f64) -> Point {
        Point {
            x,
            y: self.y_and_slope(contour, x).0,
        }
    }
    pub fn contour(&self, contour: usize) -> [Cubic; SEGMENTS] {
        std::array::from_fn(|i| {
            let x0 = i as f64 / SEGMENTS as f64;
            let x1 = (i + 1) as f64 / SEGMENTS as f64;
            let dx = (x1 - x0) / 3.0;
            let (y0, slope0) = self.y_and_slope(contour, x0);
            let (y1, slope1) = self.y_and_slope(contour, x1);
            Cubic {
                start: Point { x: x0, y: y0 },
                end: Point { x: x1, y: y1 },
                c1: Point {
                    x: x0 + dx,
                    y: y0 + dx * slope0,
                },
                c2: Point {
                    x: x1 - dx,
                    y: y1 - dx * slope1,
                },
            }
        })
    }
}

fn smooth(value: f64, target: f64, dt: f64, attack: f64, release: f64) -> f64 {
    let tau = if target > value { attack } else { release };
    assert!(tau > 0.0 && tau.is_finite());
    target + (value - target) * (-dt / tau).exp()
}

pub struct ArcMap {
    points: [Point; ARC_SAMPLES + 1],
    lengths: [f64; ARC_SAMPLES + 1],
}
impl ArcMap {
    pub fn new(mut point: impl FnMut(f64) -> Point, aspect: f64) -> Self {
        assert!(aspect.is_finite() && aspect > 0.0);
        let points = std::array::from_fn(|i| point(i as f64 / ARC_SAMPLES as f64));
        let mut lengths = [0.0; ARC_SAMPLES + 1];
        for i in 1..=ARC_SAMPLES {
            let d = points[i] - points[i - 1];
            lengths[i] = lengths[i - 1] + (d.x * aspect).hypot(d.y);
        }
        Self { points, lengths }
    }
    pub fn length(&self) -> f64 {
        self.lengths[ARC_SAMPLES]
    }
    pub fn sample(&self, fraction: f64) -> Point {
        let distance = fraction.clamp(0.0, 1.0) * self.length();
        let hi = self
            .lengths
            .partition_point(|&d| d < distance)
            .clamp(1, ARC_SAMPLES);
        let lo = hi - 1;
        let f = (distance - self.lengths[lo]) / (self.lengths[hi] - self.lengths[lo]).max(1e-12);
        let a = self.points[lo];
        let b = self.points[hi];
        Point {
            x: a.x + (b.x - a.x) * f,
            y: a.y + (b.y - a.y) * f,
        }
    }
}
