//! In-process codec benchmark (libaom RT encode + rav1d decode), so it can run inside the app
//! where the OS schedules threads like it will during a call. Returns a printable table.

use std::time::Instant;

use crate::aom::AomEncoder;
use crate::y4m::{Y4m, scale};
use crate::{Decoder, EncoderConfig, I420, Pattern, VideoEncoder};

fn pct(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p) as usize]
}

/// `footage`: a 4:2:0 y4m (decimated to 30 fps); `None` uses the synthetic pattern.
/// `pin`: run on the fast cores (see [`crate::cpu`]).
pub fn run(footage: Option<&str>, frames: usize, threads: usize, speed: u8, pin: bool) -> Result<String, String> {
    let mut out = String::new();
    if pin {
        out += &format!("pinned to {:?}\n", crate::cpu::pin_to_fast_cores());
    }
    let src: Option<Vec<I420>> = match footage {
        Some(path) => {
            let f = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
            let mut y = Y4m::new(f).map_err(|e| format!("{path}: {e}"))?;
            let step = (y.rate.0 / y.rate.1.max(1) / 30).max(1) as usize;
            let all: Vec<_> = std::iter::from_fn(|| y.next_frame()).step_by(step).take(frames).collect();
            out += &format!("footage {}x{}, {} frames\n", y.width, y.height, all.len());
            Some(all)
        }
        None => None,
    };
    out += &format!("aom speed {speed} threads {threads}\n");
    out += "     res   kbps | enc p50   p95   max | dec p50  p95 | avg B\n";
    for (w, h, kbps) in [(320, 180, 200u32), (640, 360, 600), (960, 540, 1000), (1280, 720, 1500)] {
        let cfg = EncoderConfig { width: w, height: h, bitrate_bps: kbps * 1000, threads, speed, ..Default::default() };
        let mut enc = AomEncoder::new(&cfg).map_err(|e| e.to_string())?;
        let mut dec = Decoder::new(threads, 4096 * 4096).map_err(|e| e.to_string())?;
        let mut pat = Pattern::new(w, h);
        let pics: Vec<I420> = match &src {
            Some(s) => s.iter().map(|p| scale(p, w, h)).collect(),
            None => (0..frames).map(|_| pat.next_frame()).collect(),
        };
        let (mut et, mut dt, mut bytes) = (vec![], vec![], 0usize);
        for (i, p) in pics.iter().enumerate() {
            let t = Instant::now();
            let pkts = enc.encode(p, false).map_err(|e| e.to_string())?;
            let e = t.elapsed().as_secs_f64() * 1e3;
            for pk in pkts {
                bytes += pk.data.len();
                let t = Instant::now();
                dec.decode(&pk.data).map_err(|e| e.to_string())?;
                if i >= 10 {
                    dt.push(t.elapsed().as_secs_f64() * 1e3);
                }
            }
            if i >= 10 {
                et.push(e);
            }
        }
        let max = et.iter().cloned().fold(0.0, f64::max);
        out += &format!(
            "{:>4}x{:<4} {:>5} | {:>7.1} {:>5.1} {:>5.1} | {:>7.1} {:>4.1} | {}\n",
            w,
            h,
            kbps,
            pct(&mut et, 0.5),
            pct(&mut et, 0.95),
            max,
            pct(&mut dt, 0.5),
            pct(&mut dt, 0.95),
            bytes / pics.len().max(1)
        );
    }
    Ok(out)
}
