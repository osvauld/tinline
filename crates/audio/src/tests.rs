use super::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[derive(Clone, Copy)]
struct Net {
    loss: f64,
    jitter_ms: f64,
    dup: f64,
    /// Sender frames per receiver-pull-period, e.g. 1.01.
    drift: f64,
}

const PERFECT: Net = Net { loss: 0.0, jitter_ms: 0.0, dup: 0.0, drift: 1.0 };

struct Sim {
    tx: Sender,
    rx: Receiver,
    tone: Tone,
    voice: Option<Voice>,
    rng: Rng,
    net: Net,
    inflight: Vec<(u64, Vec<u8>)>,
    out: Vec<i16>,
    tick: u64,
    send_acc: f64,
    max_buffered: u32,
    /// Extra loss override per tick (burst).
    burst: Option<(u64, u64)>,
    mute_tx: Option<(u64, u64)>,
}

impl Sim {
    fn new(net: Net, seed: u64) -> Self {
        Sim {
            tx: Sender::new(32_000).unwrap(),
            rx: Receiver::new().unwrap(),
            tone: Tone::new(440.0, 8000.0),
            voice: None,
            rng: Rng(seed),
            net,
            inflight: vec![],
            out: vec![],
            tick: 0,
            send_acc: 0.0,
            max_buffered: 0,
            burst: None,
            mute_tx: None,
        }
    }

    fn step(&mut self) {
        let now = self.tick * FRAME_MS;
        self.send_acc += self.net.drift;
        while self.send_acc >= 1.0 {
            self.send_acc -= 1.0;
            let mut pcm = [0i16; FRAME];
            let muted = self.mute_tx.is_some_and(|(a, b)| (a..b).contains(&self.tick));
            if !muted {
                match &mut self.voice {
                    Some(v) => v.next_frame(&mut pcm),
                    None => self.tone.next_frame(&mut pcm),
                }
            }
            let dg = self.tx.encode(&pcm).unwrap();
            let in_burst = self.burst.is_some_and(|(a, b)| (a..b).contains(&self.tick));
            if muted {
                continue; // sender silent: nothing on the wire at all
            }
            if in_burst || self.rng.f() < self.net.loss {
                continue;
            }
            let delay = 40.0 + self.rng.f() * 2.0 * self.net.jitter_ms;
            self.inflight.push((now + delay as u64, dg.clone()));
            if self.rng.f() < self.net.dup {
                self.inflight.push((now + delay as u64 + 15, dg));
            }
        }
        // Deliver everything due before the pull instant (now + 10).
        let pull_t = now + 10;
        self.inflight.sort_by_key(|(t, _)| *t);
        let n = self.inflight.partition_point(|(t, _)| *t <= pull_t);
        for (t, dg) in self.inflight.drain(..n).collect::<Vec<_>>() {
            self.rx.push(&dg, t);
        }
        let mut f = [0i16; FRAME];
        self.rx.pull(&mut f, pull_t);
        self.out.extend_from_slice(&f);
        self.max_buffered = self.max_buffered.max(self.rx.stats().buffered_ms);
        self.tick += 1;
    }

    fn run(&mut self, ms: u64) {
        let end = self.tick + ms / FRAME_MS;
        while self.tick < end {
            self.step();
        }
    }

    /// Fraction of 0.5 s windows (after `skip_ms`) whose frequency is within tol of 440.
    fn tone_ratio(&self, from_ms: u64, tol: f32) -> (usize, usize) {
        let w = RATE as usize / 2;
        let start = (from_ms as usize) * 48;
        let (mut good, mut total) = (0, 0);
        for win in self.out[start..].chunks_exact(w) {
            total += 1;
            if estimate_frequency(win).is_some_and(|f| (f - 440.0).abs() <= tol) {
                good += 1;
            }
        }
        (good, total)
    }
}

#[test]
fn level_bounds() {
    assert_eq!(level(&[0; FRAME]), 127);
    let full: Vec<i16> = (0..FRAME).map(|i| if i % 2 == 0 { i16::MAX } else { i16::MIN }).collect();
    assert!(level(&full) <= 1);
    let mut t = Tone::new(440.0, 8000.0);
    let mut f = [0; FRAME];
    t.next_frame(&mut f);
    let l = level(&f);
    assert!((15..=20).contains(&l), "{l}");
}

#[test]
fn estimator_basics() {
    assert_eq!(estimate_frequency(&[0; 4800]), None);
    let mut t = Tone::new(1000.0, 5000.0);
    let mut v = vec![];
    for _ in 0..10 {
        let mut f = [0; FRAME];
        t.next_frame(&mut f);
        v.extend_from_slice(&f);
    }
    assert!((estimate_frequency(&v).unwrap() - 1000.0).abs() < 1.0);
}

#[test]
fn perfect_channel() {
    let mut s = Sim::new(PERFECT, 1);
    s.run(3000);
    let (g, t) = s.tone_ratio(1000, 5.0);
    assert_eq!(g, t, "all windows on tone");
    let r = rms(&s.out[s.out.len() - 24000..]);
    assert!((3500.0..7500.0).contains(&r), "rms {r}"); // 8000/sqrt2 = 5657
    let st = s.rx.stats();
    assert_eq!(st.lost, 0);
    assert_eq!(st.concealed, 0);
    assert!(st.last_level > 0 && st.last_level < 40);
}

#[test]
fn lossy_jittery_network() {
    let net = Net { loss: 0.10, jitter_ms: 40.0, dup: 0.05, drift: 1.0 };
    let mut s = Sim::new(net, 42);
    s.run(30_000);
    let st = s.rx.stats();
    eprintln!("{st:?}");
    let (g, t) = s.tone_ratio(2000, 10.0);
    assert!(g * 10 >= t * 8, "{g}/{t} windows on tone");
    assert!(st.lost > 50, "{st:?}");
    assert!(st.recovered_fec + st.recovered_dred > 0, "{st:?}");
    assert!(st.dropped > 0, "duplicates should be counted");
    assert!(st.target_ms >= 40 && st.target_ms <= 200);
}

#[test]
fn burst_loss_recovers() {
    let mut s = Sim::new(PERFECT, 7);
    s.burst = Some((200, 210)); // 200 ms
    s.run(10_000);
    let st = s.rx.stats();
    eprintln!("{st:?}");
    assert!(st.lost + st.concealed > 0);
    // Tone is back for the last 3 seconds.
    let tail = &s.out[s.out.len() - 3 * RATE as usize..];
    for win in tail.chunks_exact(RATE as usize / 2) {
        let f = estimate_frequency(win).expect("audio resumed");
        assert!((f - 440.0).abs() < 5.0, "{f}");
    }
}

#[test]
fn input_silence_drains_without_growth() {
    let mut s = Sim::new(PERFECT, 3);
    s.mute_tx = Some((100, 400)); // 6 s of no packets
    s.run(4_000);
    // Receiver has drained: last second is silent.
    let tail = &s.out[s.out.len() - RATE as usize..];
    assert!(rms(tail) < 1.0);
    let st = s.rx.stats();
    assert!(st.buffered_ms <= 20, "{st:?}");
    assert!(s.max_buffered <= 200);
    s.run(8_000); // sender resumes at tick 400
    let tail = &s.out[s.out.len() - RATE as usize..];
    assert!((estimate_frequency(tail).unwrap() - 440.0).abs() < 5.0);
}

#[test]
fn peer_restart_resyncs() {
    let mut s = Sim::new(PERFECT, 5);
    s.run(5_000);
    s.tx = Sender::new(32_000).unwrap(); // seq back to 0
    s.run(5_000);
    let tail = &s.out[s.out.len() - 2 * RATE as usize..];
    for win in tail.chunks_exact(RATE as usize / 2) {
        let f = estimate_frequency(win).expect("audio resumed after restart");
        assert!((f - 440.0).abs() < 5.0, "{f}");
    }
}

#[test]
fn clock_drift_bounded() {
    let net = Net { loss: 0.0, jitter_ms: 10.0, dup: 0.0, drift: 1.01 };
    let mut s = Sim::new(net, 9);
    s.run(60_000);
    let st = s.rx.stats();
    eprintln!("{st:?} max {}", s.max_buffered);
    assert!(s.max_buffered <= MAX_TARGET_MS as u32 + HARD_MAX_MS as u32 + 20, "{}", s.max_buffered);
    assert!(st.dropped > 0);
    let (g, t) = s.tone_ratio(2000, 15.0);
    assert!(g * 10 >= t * 8, "{g}/{t}");
}

#[test]
fn slow_sender_underruns_bounded() {
    let net = Net { loss: 0.0, jitter_ms: 10.0, dup: 0.0, drift: 0.99 };
    let mut s = Sim::new(net, 11);
    s.run(30_000);
    assert!(s.rx.stats().buffered_ms <= 200);
}

#[test]
fn short_burst_uses_dred() {
    let mut s = Sim::new(PERFECT, 13);
    s.voice = Some(Voice::new());
    s.burst = Some((200, 203)); // 60 ms: FEC covers only one frame, DRED the rest
    s.run(6_000);
    let st = s.rx.stats();
    eprintln!("{st:?}");
    assert!(st.recovered_dred >= 1, "{st:?}");
    assert!(st.recovered_fec >= 1, "{st:?}");
    assert!(st.concealed <= 1, "{st:?}");
}

/// Crude voiced-speech-like signal: harmonics of a gliding pitch, syllabic envelope, noise.
struct Voice {
    n: u64,
    ph: f64,
    rng: Rng,
}
impl Voice {
    fn new() -> Self {
        Voice { n: 0, ph: 0.0, rng: Rng(99) }
    }
    fn next_frame(&mut self, out: &mut [i16; FRAME]) {
        for s in out.iter_mut() {
            let t = self.n as f64 / RATE as f64;
            let f0 = 120.0 + 25.0 * (t * 1.7).sin();
            self.ph += std::f64::consts::TAU * f0 / RATE as f64;
            let mut v = 0.0;
            for h in 1..=20 {
                let hf = f0 * h as f64;
                let formant = (-((hf - 700.0 - 300.0 * (t * 0.9).sin()) / 400.0).powi(2)).exp() + 0.3 * (-((hf - 2400.0) / 600.0).powi(2)).exp();
                v += formant * (self.ph * h as f64).sin() / h as f64 * 3.0;
            }
            let env = 0.55 + 0.45 * (t * 2.0 * std::f64::consts::PI * 3.0).sin();
            v = v * env * 9000.0 + (self.rng.f() - 0.5) * 300.0;
            *s = v.clamp(-32000.0, 32000.0) as i16;
            self.n += 1;
        }
    }
}

#[test]
fn lossy_speechlike_uses_all_tools() {
    let net = Net { loss: 0.10, jitter_ms: 40.0, dup: 0.05, drift: 1.0 };
    let mut s = Sim::new(net, 4242);
    s.voice = Some(Voice::new());
    s.run(30_000);
    let st = s.rx.stats();
    eprintln!("{st:?}");
    assert!(st.recovered_fec > 0 && st.recovered_dred > 0, "{st:?}");
    assert!(st.recovered_fec + st.recovered_dred + st.concealed >= st.lost, "{st:?}");
}
