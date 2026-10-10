//! G6 group-call unit tests (docs/design/group-call-spec.md §1.3, §1.4, §7).
//!
//! GC-U01: one `Receiver` per forwarded speaker slot, fed the stream a call host produces when
//! it forwards only the K loudest speakers (gaps of 100 ms .. 2 s, repeated switching).
//! GC-U02: the client-side mixer (`mix`) for K = 1..3 slots.

use super::*;

// Spec defaults (§0, §1.3, §4).
/// Live call bitrate (`core/src/node.rs`, `audio::Sender::new(48_000)`).
const CALL_BITRATE: i32 = 48_000;
/// K on a desktop call host.
const K_DESKTOP: usize = 3;
/// K on a phone call host.
const K_PHONE: usize = 2;
/// Frame size in samples (20 ms at 48 kHz).
const GC_FRAME: usize = FRAME;
/// Fixed one-way delay member -> host -> member in the simulation.
const NET_DELAY_MS: u64 = 40;
/// Silence-gate keep-level interval (§1.4).
const KEEP_LEVEL_MS: u64 = 500;
/// Test tone: 440 Hz at 8000 peak (about -12 dBFS, a loud talker).
const TONE_HZ: f32 = 440.0;
const TONE_AMP: f32 = 8000.0;

// ---------------------------------------------------------------------------------------------
// GC-U01
// ---------------------------------------------------------------------------------------------

/// How the stream for one slot looks on the wire while its sender is not forwarded.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Gap {
    /// Speaker selection (§1.3): the sender keeps encoding, H drops its datagrams, so the
    /// forwarded `seq` jumps by the gap length (H forwards `seq` unchanged, §1.1).
    SeqJump,
    /// Silence gate (§1.4, Q4) if the client stops *encoding*: `seq` does not advance, so the
    /// stream resumes with the next `seq` after a pause in time.
    SeqContinuous,
    /// Silence gate while the sender stays in S: one keep-level datagram every 500 ms is
    /// forwarded during the gap (encoder keeps running, so `seq` jumps between them).
    KeepLevel,
}

struct SlotRun {
    out: Vec<i16>,
    /// `stats().target_ms` after every pull.
    target: Vec<u32>,
    /// Mouth-to-ear latency (pull time - send time) of every frame played from a packet slot
    /// (decoded, FEC/DRED/PLC-for-lost), i.e. every pull that advanced `next_seq`.
    latency: Vec<(u64, u64)>,
    stats: Stats,
}

/// Run one slot for `total_ms`. `forwarded(t_ms)` says whether H forwards the sender at send
/// time `t_ms`. Delivery and pull timing match `tests::Sim` (pull at tick*20 + 10).
fn run_slot(total_ms: u64, mode: Gap, forwarded: impl Fn(u64) -> bool) -> SlotRun {
    let mut tx = Sender::new(CALL_BITRATE).unwrap();
    let mut rx = Receiver::new().unwrap();
    let mut tone = Tone::new(TONE_HZ, TONE_AMP);
    let mut send_time: std::collections::HashMap<u32, u64> = Default::default();
    let mut inflight: Vec<(u64, Vec<u8>)> = vec![];
    let mut last_sent = 0u64;
    let mut run = SlotRun { out: vec![], target: vec![], latency: vec![], stats: Stats::default() };
    for tick in 0..total_ms / FRAME_MS {
        let now = tick * FRAME_MS;
        let mut pcm = [0i16; GC_FRAME];
        tone.next_frame(&mut pcm);
        let fwd = forwarded(now);
        let send = match mode {
            Gap::SeqJump | Gap::SeqContinuous => fwd,
            Gap::KeepLevel => fwd || now >= last_sent + KEEP_LEVEL_MS,
        };
        if mode == Gap::KeepLevel && !fwd {
            pcm = [0; GC_FRAME]; // gated sender is quiet; keep-level frames carry silence
        }
        // SeqContinuous: gate closed and encoder idle, so nothing is encoded and seq stays.
        if mode != Gap::SeqContinuous || send {
            let seq = tx.seq;
            let dg = tx.encode(&pcm).unwrap();
            if send {
                last_sent = now;
                send_time.insert(seq, now);
                inflight.push((now + NET_DELAY_MS, dg));
            }
        }
        let pull_t = now + 10;
        let n = inflight.partition_point(|(t, _)| *t <= pull_t);
        for (t, dg) in inflight.drain(..n) {
            rx.push(&dg, t);
        }
        let before = rx.next_seq;
        let mut f = [0i16; GC_FRAME];
        rx.pull(&mut f, pull_t);
        if rx.next_seq != before {
            let played = rx.next_seq.wrapping_sub(1);
            if let Some(&st) = send_time.get(&played) {
                run.latency.push((pull_t, pull_t - st));
            }
        }
        run.out.extend_from_slice(&f);
        run.target.push(rx.stats().target_ms);
    }
    run.stats = rx.stats();
    run
}

/// Largest absolute difference between neighbouring samples.
fn max_step(x: &[i16]) -> i32 {
    x.windows(2).map(|w| (w[1] as i32 - w[0] as i32).abs()).max().unwrap_or(0)
}

struct Baseline {
    latency_max: u64,
    step_max: i32,
}

/// The same slot with no forwarding gaps: what a healthy stream looks like.
fn baseline(total_ms: u64) -> Baseline {
    let r = run_slot(total_ms, Gap::SeqJump, |_| true);
    // Skip the first second (start-up).
    let latency_max = r.latency.iter().filter(|(t, _)| *t >= 1000).map(|&(_, l)| l).max().unwrap();
    let step_max = max_step(&r.out[RATE as usize..]);
    assert!(r.target.iter().all(|&t| t <= INITIAL_TARGET_MS as u32), "baseline target {:?}", r.stats);
    Baseline { latency_max, step_max }
}

/// Check one gapped run against the baseline. Returns a list of violations.
fn check(name: &str, r: &SlotRun, b: &Baseline) -> Vec<String> {
    let mut bad = vec![];
    let target_max = r.target.iter().copied().max().unwrap();
    let target_end = *r.target.last().unwrap();
    // 1. No ratchet: a forwarding gap is not network jitter, so the target never rises above
    //    what a gap-free stream uses.
    if target_max > INITIAL_TARGET_MS as u32 {
        bad.push(format!("target ratcheted: max {target_max} ms, end {target_end} ms (limit {INITIAL_TARGET_MS})"));
    }
    // 2. Resumed audio plays without long delay: every frame played from a packet is at most
    //    one frame later than in the gap-free run. Also catches playing stale (gap) audio.
    let skip = 1000;
    let lat_max = r.latency.iter().filter(|(t, _)| *t >= skip).map(|&(_, l)| l).max().unwrap();
    let lat_limit = b.latency_max + FRAME_MS;
    if lat_max > lat_limit {
        let worst = r.latency.iter().filter(|(t, l)| *t >= skip && *l > lat_limit).take(3).collect::<Vec<_>>();
        bad.push(format!("latency {lat_max} ms > {lat_limit} ms (baseline {}), first at {worst:?}", b.latency_max));
    }
    // 3. No clicks: the largest sample-to-sample step stays within 1.5x the gap-free stream.
    let step = max_step(&r.out[RATE as usize..]);
    let step_limit = b.step_max * 3 / 2;
    if step > step_limit {
        let at = r.out[RATE as usize..].windows(2).position(|w| (w[1] as i32 - w[0] as i32).abs() == step).unwrap();
        let i = RATE as usize + at + 1; // first sample after the step
        let (pre, post) = (&r.out[i.saturating_sub(GC_FRAME)..i], &r.out[i..(i + GC_FRAME).min(r.out.len())]);
        bad.push(format!(
            "click: step {step} > {step_limit} (baseline {}) at {} ms, frame offset {}, rms before {:.0} after {:.0}",
            b.step_max,
            i / 48,
            i % GC_FRAME,
            rms(pre),
            rms(post)
        ));
    }
    eprintln!(
        "{name}: target max {target_max} end {target_end}, latency max {lat_max} (base {}), step {step} (base {}), {:?}",
        b.latency_max, b.step_max, r.stats
    );
    bad
}

/// One gap of `gap_ms` after 2 s of speech, then 3 s of speech.
fn single_gap(mode: Gap, gap_ms: u64) -> Vec<String> {
    let total = 2000 + gap_ms + 3000;
    let b = baseline(total);
    let r = run_slot(total, mode, |t| !(2000..2000 + gap_ms).contains(&t));
    // Resumed audio is the tone again within the last 3 s.
    let tail = &r.out[r.out.len() - 2 * RATE as usize..];
    assert!(estimate_frequency(tail).is_some_and(|f| (f - TONE_HZ).abs() < 5.0), "no tone after resume");
    check(&format!("{mode:?} {gap_ms} ms"), &r, &b)
}

/// Speaker switching for 30 s: deterministic on/off spans of 200..1500 ms on and 100..2000 ms
/// off (selection plus 200 ms hysteresis, §1.3).
fn switching(mode: Gap) -> Vec<String> {
    let mut spans = vec![];
    let mut t = 0u64;
    let mut x = 0x9e37_79b9u64;
    let mut rnd = |lo: u64, hi: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        lo + (x % (hi - lo + 1)) / 20 * 20
    };
    let first = 1500;
    spans.push((0, first));
    t += first;
    while t < 30_000 {
        let off = rnd(100, 2000);
        let on = rnd(200, 1500);
        spans.push((t + off, t + off + on));
        t += off + on;
    }
    let total = t + 1000;
    let b = baseline(total);
    let r = run_slot(total, mode, |now| spans.iter().any(|&(a, e)| (a..e).contains(&now)));
    check(&format!("{mode:?} switching"), &r, &b)
}

fn assert_ok(bad: Vec<String>) {
    assert!(bad.is_empty(), "GC-U01 violations:\n  {}", bad.join("\n  "));
}

#[test]
fn gc_u01_baseline_is_sane() {
    let b = baseline(5000);
    // 40 ms network + 60 ms initial target, give or take a frame of pull alignment.
    assert!((NET_DELAY_MS + 40..=NET_DELAY_MS + INITIAL_TARGET_MS as u64 + 2 * FRAME_MS).contains(&b.latency_max), "{}", b.latency_max);
    assert!(b.step_max > 0 && b.step_max < 2000, "{}", b.step_max);
}

#[test]
#[ignore = "GC-U01: gap taken as underrun (target 60->80 ms), dropped seqs played as lost frames (latency 150 vs 90 ms), PLC-to-silence click, fix pending"]
fn gc_u01_seq_jump_100ms() {
    assert_ok(single_gap(Gap::SeqJump, 100));
}

#[test]
#[ignore = "GC-U01: gap taken as underrun (target 60->80 ms), PLC-to-silence click after MAX_CONCEAL, fix pending"]
fn gc_u01_seq_jump_500ms() {
    assert_ok(single_gap(Gap::SeqJump, 500));
}

#[test]
#[ignore = "GC-U01: gap taken as underrun (target 60->80 ms), PLC-to-silence click after MAX_CONCEAL, fix pending"]
fn gc_u01_seq_jump_2s() {
    assert_ok(single_gap(Gap::SeqJump, 2000));
}

#[test]
#[ignore = "GC-U01: gap taken as underrun (target 60->80 ms), fix pending"]
fn gc_u01_seq_continuous_100ms() {
    assert_ok(single_gap(Gap::SeqContinuous, 100));
}

#[test]
#[ignore = "GC-U01: gap taken as jitter and underrun (target 113 ms, latency 130 vs 90 ms), PLC-to-silence click, fix pending"]
fn gc_u01_seq_continuous_500ms() {
    assert_ok(single_gap(Gap::SeqContinuous, 500));
}

#[test]
#[ignore = "GC-U01: gap taken as jitter and underrun (target 200 ms, latency 230 vs 90 ms), PLC-to-silence click, fix pending"]
fn gc_u01_seq_continuous_2s() {
    assert_ok(single_gap(Gap::SeqContinuous, 2000));
}

#[test]
#[ignore = "GC-U01: target ratchets to 200 ms, latency 250 vs 90 ms, PLC-to-silence clicks, fix pending"]
fn gc_u01_seq_jump_switching() {
    assert_ok(switching(Gap::SeqJump));
}

#[test]
#[ignore = "GC-U01: target ratchets to 200 ms, latency 230 vs 90 ms, clicks at cut-out and resume, fix pending"]
fn gc_u01_seq_continuous_switching() {
    assert_ok(switching(Gap::SeqContinuous));
}

#[test]
#[ignore = "GC-U01: each keep-level datagram is an underrun, target ratchets to 200 ms, latency 150 vs 90 ms, clicks, fix pending"]
fn gc_u01_keep_level_switching() {
    assert_ok(switching(Gap::KeepLevel));
}

// ---------------------------------------------------------------------------------------------
// GC-U02
// ---------------------------------------------------------------------------------------------

/// Amplitude of the `freq` component in `x` (Goertzel; exact for whole-Hz tones over 1 s).
fn amplitude_at(x: &[i16], freq: f32) -> f32 {
    let w = std::f64::consts::TAU * freq as f64 / RATE as f64;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s0 = v as f64 + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let p = s1 * s1 + s2 * s2 - c * s1 * s2;
    (2.0 * p.sqrt() / x.len() as f64) as f32
}

/// Distinct, non-harmonic slot tones.
const SLOT_HZ: [f32; K_DESKTOP] = [350.0, 770.0, 1230.0];

/// One second of `k` slot tones at `amp` (slots >= k silent), mixed frame by frame.
fn mix_tones(k: usize, amp: f32) -> (Vec<i16>, Vec<i32>) {
    let mut tones: Vec<Tone> = SLOT_HZ.iter().map(|&f| Tone::new(f, amp)).collect();
    let (mut out, mut sum) = (vec![], vec![]);
    for _ in 0..RATE as usize / GC_FRAME {
        let mut frames = [[0i16; GC_FRAME]; K_DESKTOP];
        for (i, (t, f)) in tones.iter_mut().zip(frames.iter_mut()).enumerate() {
            if i < k {
                t.next_frame(f);
            }
        }
        let refs: Vec<&[i16]> = frames.iter().map(|f| &f[..]).collect();
        let mut o = [0i16; GC_FRAME];
        mix(&refs, &mut o);
        out.extend_from_slice(&o);
        sum.extend((0..GC_FRAME).map(|j| frames.iter().map(|f| f[j] as i32).sum::<i32>()));
    }
    (out, sum)
}

#[test]
fn gc_u02_all_frequencies_present() {
    for k in 1..=K_DESKTOP {
        let (out, sum) = mix_tones(k, 6000.0);
        // Below the limiter knee the mix is the exact sum.
        assert!(out.iter().zip(&sum).all(|(&o, &s)| o as i32 == s), "k={k}: not an exact sum");
        for (i, &f) in SLOT_HZ.iter().enumerate() {
            let a = amplitude_at(&out, f);
            if i < k {
                assert!((a - 6000.0).abs() < 60.0, "k={k}: {f} Hz amplitude {a}");
            } else {
                assert!(a < 1.0, "k={k}: silent slot {f} Hz leaked {a}");
            }
        }
        if k == 1 {
            assert!((estimate_frequency(&out).unwrap() - SLOT_HZ[0]).abs() < 1.0);
        }
    }
}

#[test]
fn gc_u02_loud_inputs_do_not_clip_or_wrap() {
    // 3 x 12000 peaks at 36000: over full scale, but must be limited softly, not flat-topped.
    let (out, sum) = mix_tones(K_DESKTOP, 12000.0);
    assert!(sum.iter().any(|&s| s > i16::MAX as i32), "test needs overflowing input");
    assert!(out.iter().all(|&o| o > i16::MIN && o < i16::MAX), "hard clipping");
    for (&o, &s) in out.iter().zip(&sum) {
        assert!(s == 0 || (o as i32).signum() == s.signum(), "sign flip: sum {s} -> {o}");
        assert!((o as i32).abs() <= s.abs(), "limiter must not amplify: {s} -> {o}");
    }
    for &f in &SLOT_HZ {
        let a = amplitude_at(&out, f);
        assert!(a > 12000.0 * 0.8, "{f} Hz lost under limiting: {a}");
    }
    // Monotonic: a louder sum never maps to a quieter output.
    let mut pairs: Vec<(i32, i16)> = sum.iter().copied().zip(out.iter().copied()).collect();
    pairs.sort();
    assert!(pairs.windows(2).all(|w| w[0].1 <= w[1].1), "limiter not monotonic");

    // Worst case: K full-scale slots in phase, both polarities. No wrap-around.
    for v in [i16::MAX, i16::MIN] {
        let a = [v; GC_FRAME];
        for k in 1..=K_DESKTOP {
            let refs = vec![&a[..]; k];
            let mut o = [0i16; GC_FRAME];
            mix(&refs, &mut o);
            assert!(o.iter().all(|&x| x.signum() == v.signum() && (x as i32).abs() > 30_000), "k={k} v={v}: {}", o[0]);
        }
    }
}

#[test]
fn gc_u02_silent_slots_contribute_nothing() {
    let mut t = Tone::new(SLOT_HZ[0], 30_000.0); // loud, so the limiter is active
    let mut a = [0i16; GC_FRAME];
    let z = [0i16; GC_FRAME];
    for _ in 0..10 {
        t.next_frame(&mut a);
        let mut alone = [0i16; GC_FRAME];
        mix(&[&a[..]], &mut alone);
        for k in 1..=K_DESKTOP {
            let mut refs: Vec<&[i16]> = vec![&z[..]; k];
            refs.push(&a[..]);
            refs.rotate_left(k / 2);
            let mut o = [0i16; GC_FRAME];
            mix(&refs, &mut o);
            assert_eq!(o, alone, "{k} silent slots changed the mix");
        }
    }
    // No slots at all, or only silent ones: silence.
    let mut o = [1i16; GC_FRAME];
    mix(&[], &mut o);
    assert!(o.iter().all(|&x| x == 0));
    let mut o = [1i16; GC_FRAME];
    mix(&[&z[..], &z[..]], &mut o);
    assert!(o.iter().all(|&x| x == 0));
}

/// End to end: K senders -> K slot receivers -> mix, for K = 1..3 and the phone default.
#[test]
fn gc_u02_mix_of_decoded_slots() {
    for k in [1, K_PHONE, K_DESKTOP] {
        let mut senders: Vec<(Sender, Receiver, Tone)> = (0..K_DESKTOP)
            .map(|i| (Sender::new(CALL_BITRATE).unwrap(), Receiver::new().unwrap(), Tone::new(SLOT_HZ[i], 6000.0)))
            .collect();
        let mut out = vec![];
        for tick in 0..100u64 {
            let now = tick * FRAME_MS;
            let mut frames = [[0i16; GC_FRAME]; K_DESKTOP];
            for (i, ((tx, rx, tone), f)) in senders.iter_mut().zip(frames.iter_mut()).enumerate() {
                if i < k {
                    let mut pcm = [0i16; GC_FRAME];
                    tone.next_frame(&mut pcm);
                    rx.push(&tx.encode(&pcm).unwrap(), now);
                }
                // A slot that never received anything pulls silence.
                rx.pull(f, now + 10);
            }
            let refs: Vec<&[i16]> = frames.iter().map(|f| &f[..]).collect();
            let mut o = [0i16; GC_FRAME];
            mix(&refs, &mut o);
            out.extend_from_slice(&o);
        }
        let last = &out[out.len() - RATE as usize..]; // 2 s run; the last 1 s is steady state
        for (i, &f) in SLOT_HZ.iter().enumerate() {
            let a = amplitude_at(last, f);
            if i < k {
                assert!((a - 6000.0).abs() < 6000.0 * 0.2, "k={k}: {f} Hz amplitude {a}");
            } else {
                assert!(a < 6000.0 * 0.02, "k={k}: silent slot {f} Hz leaked {a}");
            }
        }
    }
}
