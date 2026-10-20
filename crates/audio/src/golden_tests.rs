//! Byte-identical regression for the Opus encoder/decoder settings: a change here means the wire
//! output changed. DRED and deep PLC are off here because libopus's neural code rounds
//! differently per CPU (CI's runner disagreed with both dev machines); `tests.rs` covers those
//! paths by behaviour instead.
use super::*;

/// The call codec without its CPU-dependent neural parts: no DRED, decoder below deep PLC (5).
const PLAIN_CALL: CodecConfig = CodecConfig { dred: 0, decoder_complexity: Some(4), ..CALL_CODEC };

fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x0100_0000_01b3);
    }
}

/// Deterministic speech-like fixture: two tones with a slow envelope plus xorshift noise.
fn fixture(frame: usize) -> [i16; FRAME] {
    let mut s = 0x9e37_79b9_7f4a_7c15u64 ^ (frame as u64).wrapping_mul(0x1000_0000_01b3);
    let mut out = [0i16; FRAME];
    let env = 0.5 + 0.5 * ((frame as f64) * 0.07).sin();
    for (i, o) in out.iter_mut().enumerate() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        let t = (frame * FRAME + i) as f64 / RATE as f64;
        let v = 6000.0 * (std::f64::consts::TAU * 220.0 * t).sin() + 3000.0 * (std::f64::consts::TAU * 1130.0 * t).sin();
        let noise = ((s >> 40) as i64 % 800 - 400) as f64;
        *o = (env * v + noise) as i16;
    }
    out
}

#[test]
fn sender_bytes_golden() {
    let mut tx = Sender::with_codec(&PLAIN_CALL).unwrap();
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for f in 0..150 {
        fnv(&mut h, &tx.encode(&fixture(f)).unwrap());
    }
    assert_eq!(h, SENDER_HASH, "sender hash is {h:#x}");
}

#[test]
fn group_sender_bytes_golden() {
    let mut tx = Sender::with_codec(&CodecConfig { bitrate: 24_000, ..PLAIN_CALL }).unwrap();
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for f in 0..150 {
        // Quiet stretch in the middle closes then reopens the gate.
        let mut pcm = fixture(f);
        if (40..90).contains(&f) {
            pcm = [0; FRAME];
        }
        match tx.encode_group(&pcm).unwrap() {
            GroupFrame::Opus(d) => fnv(&mut h, &d),
            GroupFrame::Gated { level } => fnv(&mut h, &[0xff, level]),
        }
    }
    assert_eq!(h, GROUP_SENDER_HASH, "group sender hash is {h:#x}");
}

#[test]
fn receiver_pcm_golden() {
    let mut tx = Sender::with_codec(&PLAIN_CALL).unwrap();
    let mut rx = Receiver::with_codec(&PLAIN_CALL).unwrap();
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for f in 0..150usize {
        let now = f as u64 * 20;
        let d = tx.encode(&fixture(f)).unwrap();
        // Drop every 7th datagram so the FEC and PLC paths run.
        if f % 7 != 3 {
            rx.push(&d, now);
        }
        let mut out = [0i16; FRAME];
        rx.pull(&mut out, now);
        for s in out {
            fnv(&mut h, &s.to_le_bytes());
        }
    }
    assert_eq!(h, RECEIVER_HASH, "receiver hash is {h:#x}");
}

const SENDER_HASH: u64 = 0x3fb116cfdf791360;
const GROUP_SENDER_HASH: u64 = 0xc239e2fe0fb39dd0;
const RECEIVER_HASH: u64 = 0x6bbef000ab6e61ac;
