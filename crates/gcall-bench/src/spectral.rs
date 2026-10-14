//! Dependency-free bounded experimental feature extractor, NOT a product default.
//! 48kHz mono PCM, 1024-sample Hann windows, at most one output per 1920 samples.
//! Bands are coarse energy descriptors, centroid is NOT pitch; no voice/emotion inference.
use std::f64::consts::TAU;

pub const RATE: f64 = 48_000.0;
pub const N: usize = 1024;
pub const HOP: usize = 1920; // 25Hz at 48kHz, independent of render rate

#[derive(Clone, Copy, Default, Debug)]
pub struct Features {
    pub low: f64,
    pub mid: f64,
    pub high: f64,
    pub centroid_hz: f64,
    pub onset: f64,
}
#[derive(Clone, Copy, Default)]
struct Complex {
    re: f64,
    im: f64,
}

pub struct Extractor {
    pcm: [f64; N],
    window: [f64; N],
    fft: [Complex; N],
    previous: [f64; N / 2 + 1],
    head: usize,
    filled: usize,
    since: usize,
    previous_energy: f64,
}
impl Extractor {
    pub fn new() -> Self {
        Self {
            pcm: [0.0; N],
            window: std::array::from_fn(|i| 0.5 - 0.5 * (TAU * i as f64 / (N - 1) as f64).cos()),
            fft: [Complex::default(); N],
            previous: [0.0; N / 2 + 1],
            head: 0,
            filled: 0,
            since: 0,
            previous_energy: 0.0,
        }
    }
    pub fn reset(&mut self) {
        self.pcm.fill(0.0);
        self.previous.fill(0.0);
        self.head = 0;
        self.filled = 0;
        self.since = 0;
        self.previous_energy = 0.0;
    }
    pub fn push(&mut self, pcm: &[i16]) -> Option<Features> {
        for &v in pcm {
            self.pcm[self.head] = v as f64 / 32768.0;
            self.head = (self.head + 1) % N;
        }
        self.filled = self.filled.saturating_add(pcm.len()).min(N);
        self.since = self.since.saturating_add(pcm.len());
        if self.filled < N || self.since < HOP {
            return None;
        }
        self.since %= HOP; // long inputs do not create a burst of fabricated outputs
        for i in 0..N {
            self.fft[i] = Complex {
                re: self.pcm[(self.head + i) % N] * self.window[i],
                im: 0.0,
            };
        }
        for i in 0..N {
            let j = i.reverse_bits() >> (usize::BITS - N.trailing_zeros());
            if j > i {
                self.fft.swap(i, j);
            }
        }
        let mut size = 2;
        while size <= N {
            let angle = -TAU / size as f64;
            let root = Complex {
                re: angle.cos(),
                im: angle.sin(),
            };
            for base in (0..N).step_by(size) {
                let mut w = Complex { re: 1.0, im: 0.0 };
                for offset in 0..size / 2 {
                    let a = self.fft[base + offset];
                    let b = self.fft[base + offset + size / 2];
                    let v = Complex {
                        re: b.re * w.re - b.im * w.im,
                        im: b.re * w.im + b.im * w.re,
                    };
                    self.fft[base + offset] = Complex {
                        re: a.re + v.re,
                        im: a.im + v.im,
                    };
                    self.fft[base + offset + size / 2] = Complex {
                        re: a.re - v.re,
                        im: a.im - v.im,
                    };
                    w = Complex {
                        re: w.re * root.re - w.im * root.im,
                        im: w.re * root.im + w.im * root.re,
                    };
                }
            }
            size *= 2;
        }
        let mut out = Features::default();
        let (mut energy, mut weighted, mut positive_flux) = (0.0, 0.0, 0.0);
        for i in 1..=N / 2 {
            // DC omitted; includes Nyquist
            let v = self.fft[i];
            let power = (v.re * v.re + v.im * v.im) * if i == N / 2 { 1.0 } else { 2.0 };
            let hz = i as f64 * RATE / N as f64;
            if hz < 500.0 {
                out.low += power;
            } else if hz < 2500.0 {
                out.mid += power;
            } else {
                out.high += power;
            }
            energy += power;
            weighted += hz * power;
            positive_flux += (power - self.previous[i]).max(0.0);
            self.previous[i] = power;
        }
        if energy > 1e-12 {
            out.low /= energy;
            out.mid /= energy;
            out.high /= energy;
            out.centroid_hz = weighted / energy;
            out.onset = (positive_flux / energy.max(self.previous_energy)).clamp(0.0, 1.0);
        }
        self.previous_energy = energy;
        Some(out)
    }
}
