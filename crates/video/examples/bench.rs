//! Realtime AV1 benchmark: per-frame encode/decode cost, size and quality per resolution.
//!
//!     cargo run -p video --release --example bench -- [frames] [threads]
use std::time::Instant;
use video::{Decoder, Encoder, EncoderConfig, Pattern, psnr_y};

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p) as usize]
}

fn main() {
    let args: Vec<usize> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let frames = *args.first().unwrap_or(&90);
    let threads = *args.get(1).unwrap_or(&0);
    println!("frames={frames} threads={threads} (0 = auto), speed 10, 30 fps");
    println!(
        "{:>9} {:>7} | {:>7} {:>7} {:>7} | {:>6} {:>6} | {:>7} {:>6} {:>5}",
        "res", "kbps", "enc p50", "p95", "max", "dec50", "dec95", "key B", "avg B", "psnr"
    );
    for (w, h, kbps) in [(320, 240, 250), (480, 360, 400), (640, 480, 600), (1280, 720, 1500)] {
        let cfg = EncoderConfig {
            width: w,
            height: h,
            bitrate_bps: kbps * 1000,
            threads,
            ..Default::default()
        };
        let mut enc = Encoder::new(&cfg).unwrap();
        let mut dec = Decoder::new(threads, 4096 * 4096).unwrap();
        let mut pat = Pattern::new(w, h);
        let src: Vec<_> = (0..frames).map(|_| pat.next_frame()).collect();
        let (mut et, mut dt, mut sizes, mut q) = (vec![], vec![], vec![], vec![]);
        let mut key = 0;
        for f in &src {
            let t = Instant::now();
            let pkts = enc.encode(f, false).unwrap();
            et.push(t.elapsed().as_secs_f64() * 1e3);
            for p in pkts {
                if p.key {
                    key = p.data.len();
                } else {
                    sizes.push(p.data.len());
                }
                let t = Instant::now();
                let pics = dec.decode(&p.data).unwrap();
                dt.push(t.elapsed().as_secs_f64() * 1e3);
                for pic in pics {
                    q.push(psnr_y(&src[p.frame as usize], &pic));
                }
            }
        }
        // Skip warm-up (the first frames include the lookahead fill and keyframe).
        let mut et = et[10.min(et.len())..].to_vec();
        let max = et.iter().cloned().fold(0.0, f64::max);
        let avg = sizes.iter().sum::<usize>() / sizes.len().max(1);
        println!(
            "{:>4}x{:<4} {:>7} | {:>7.1} {:>7.1} {:>7.1} | {:>6.1} {:>6.1} | {:>7} {:>6} {:>5.1}",
            w,
            h,
            kbps,
            pct(&mut et, 0.5),
            pct(&mut et, 0.95),
            max,
            pct(&mut dt, 0.5),
            pct(&mut dt, 0.95),
            key,
            avg,
            q.iter().sum::<f64>() / q.len() as f64
        );
    }
}
