//! Shared bits of the group-call hosting benchmark (GA-P2, `docs/design/group-call-bench.md`).
//!
//! Not product code: hard-coded participants (deterministic keys), no membership, no signing.
//!
//! Datagram types on the bench ALPN (mirrors group-call-spec §1.1):
//! - `0x01 [audio]` member -> host, `audio` = 1:1 wire `[seq u32][level u8][opus]`
//! - `0x03 [level]` member -> host while gated, every 500 ms (never forwarded)
//! - `0x02 [slot][audio]` host -> member
//! - `0x04 [gen u32][slot..]` host -> member, the forwarded set S (G6-D1 `Active`; a datagram
//!   here instead of the control stream)
//! - `0x10 [8 bytes]` ping, echoed by the host as `0x11 [same 8 bytes]`
use std::time::Duration;

pub use audio::FRAME;
use iroh::endpoint::QuicTransportConfig;
use iroh::{PublicKey, SecretKey};

pub const ALPN: &[u8] = b"tinline/gcall-bench/1";
pub const T_AUDIO_UP: u8 = 0x01;
pub const T_AUDIO_DOWN: u8 = 0x02;
pub const T_LEVEL: u8 = 0x03;
pub const T_ACTIVE: u8 = 0x04;
pub const T_PING: u8 = 0x10;
pub const T_PONG: u8 = 0x11;
pub const FRAME_MS: u64 = 20;
/// Level-only datagram cadence while gated, in frames (500 ms).
pub const LEVEL_EVERY: u32 = 25;
pub const MAX_SLOTS: usize = 64;

/// Slot 0 is the host's own participant; members use slots 1.. .
pub fn secret(slot: u8) -> SecretKey {
    let mut b = [0u8; 32];
    b[0] = 0x6b; // "bench"
    b[1] = slot;
    SecretKey::from_bytes(&b)
}

pub fn public(slot: u8) -> PublicKey {
    secret(slot).public()
}

pub fn transport() -> QuicTransportConfig {
    QuicTransportConfig::builder()
        .keep_alive_interval(Duration::from_secs(5))
        .max_idle_timeout(Some(Duration::from_secs(20).try_into().expect("small timeout")))
        .build()
}

/// xorshift64*, enough for synthetic talk patterns.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn exp(&mut self, mean: f32) -> f32 {
        -mean * (1.0 - self.f()).max(1e-6).ln()
    }
}

/// Speech-like synthetic talker: a harmonic voice (f0 100-250 Hz, 12 harmonics, slow pitch
/// drift, 4 Hz syllable envelope, a little breath noise) in talk spurts, and a -64 dBov noise
/// floor in between so the sender silence gate closes. Spurts are 0.6-3 s; the mean silence is
/// set so each talker speaks a `duty` fraction of the time (independent talkers, Brady-like).
pub struct Voice {
    rng: Rng,
    f0: f32,
    phase: f32,
    t: u64,
    duty: f32,
    frames_left: u32,
    talking: bool,
}

pub struct VoiceFrame {
    pub talking: bool,
    /// First frame of a talk spurt.
    pub onset: bool,
}

impl Voice {
    pub fn new(seed: u64, duty: f32) -> Self {
        let mut rng = Rng::new(seed);
        let f0 = 100.0 + 150.0 * rng.f();
        // Start silent for a random while so talkers do not all start together.
        let frames_left = (rng.f() * 4000.0 / FRAME_MS as f32) as u32 + 10;
        Voice { rng, f0, phase: 0.0, t: 0, duty, frames_left, talking: false }
    }

    pub fn next_frame(&mut self, out: &mut [i16; FRAME]) -> VoiceFrame {
        let mut onset = false;
        if self.frames_left == 0 {
            self.talking = !self.talking;
            let secs = if self.talking {
                onset = true;
                0.6 + 2.4 * self.rng.f()
            } else {
                let mean_talk = 1.8;
                self.rng.exp(mean_talk * (1.0 - self.duty) / self.duty).max(0.3)
            };
            self.frames_left = ((secs * 1000.0) as u64 / FRAME_MS).max(1) as u32;
        }
        self.frames_left -= 1;
        let rate = audio::RATE as f32;
        for (i, s) in out.iter_mut().enumerate() {
            let n = self.t * FRAME as u64 + i as u64;
            let ts = n as f32 / rate;
            let noise = (self.rng.f() - 0.5) * 2.0;
            let v = if self.talking {
                let f0 = self.f0 * (1.0 + 0.05 * (std::f32::consts::TAU * 0.7 * ts).sin());
                self.phase = (self.phase + std::f32::consts::TAU * f0 / rate) % std::f32::consts::TAU;
                let mut x = 0.0;
                for h in 1..=12 {
                    x += (self.phase * h as f32).sin() / h as f32;
                }
                let env = 0.55 + 0.45 * (std::f32::consts::TAU * 4.0 * ts).sin();
                5000.0 * env * x + 300.0 * noise
            } else {
                35.0 * noise
            };
            *s = v.clamp(-32767.0, 32767.0) as i16;
        }
        self.t += 1;
        VoiceFrame { talking: self.talking, onset }
    }
}

/// CPU ticks (utime + stime) of this process.
pub fn cpu_ticks_self() -> u64 {
    let Ok(s) = std::fs::read_to_string("/proc/self/stat") else { return 0 };
    let Some(rest) = s.rsplit_once(')').map(|(_, r)| r) else { return 0 };
    let f: Vec<&str> = rest.split_whitespace().collect();
    // After ")": state is field 3, utime field 14, stime field 15 -> indices 11, 12.
    f.get(11).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
        + f.get(12).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
}

pub fn pct(v: &mut [u32], p: f64) -> u32 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[(((v.len() - 1) as f64) * p).round() as usize]
}

/// Minimal `--key value` argument parsing.
pub struct Args(Vec<String>);

impl Args {
    pub fn parse() -> Self {
        Args(std::env::args().skip(1).collect())
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        let k = format!("--{key}");
        self.0.iter().position(|a| *a == k).and_then(|i| self.0.get(i + 1)).map(|s| s.as_str())
    }
    pub fn flag(&self, key: &str) -> bool {
        let k = format!("--{key}");
        self.0.iter().any(|a| *a == k)
    }
    pub fn num<T: std::str::FromStr>(&self, key: &str, default: T) -> T {
        self.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
    }
}
