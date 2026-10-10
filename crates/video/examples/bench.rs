//! Realtime AV1 benchmark: per-frame encode/decode cost, size and quality per resolution.
//!
//!     cargo run -p video --release --example bench -- [aom|rav1e] [frames] [threads] [speed]
//!
//! With `Y4M=<file>` it encodes real footage (decimated to 30 fps, scaled per resolution)
//! instead of the synthetic pattern.
use std::time::Instant;
use video::{Decoder, Encoder, EncoderConfig, Pattern, VideoEncoder, aom::AomEncoder, psnr_y};

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p) as usize]
}

fn main() {
    let kind = std::env::args().nth(1).unwrap_or("aom".into());
    let args: Vec<usize> = std::env::args().skip(2).filter_map(|a| a.parse().ok()).collect();
    let frames = *args.first().unwrap_or(&90);
    let threads = *args.get(1).unwrap_or(&0);
    let speed = *args.get(2).unwrap_or(&10) as u8;
    println!("{kind}: frames={frames} threads={threads} (0 = auto), speed {speed}, 30 fps");
    // Other work on the machine skews timings; show it so a noisy run is visible.
    if let Ok(l) = std::fs::read_to_string("/proc/loadavg") {
        println!("loadavg {}", l.split_whitespace().take(3).collect::<Vec<_>>().join(" "));
    }
    println!(
        "{:>9} {:>7} | {:>7} {:>7} {:>7} | {:>6} {:>6} | {:>7} {:>6} {:>5} {:>4}",
        "res", "kbps", "enc p50", "p95", "max", "dec50", "dec95", "key B", "avg B", "psnr", "held"
    );
    let footage: Option<Vec<video::I420>> = std::env::var("Y4M").ok().map(|path| {
        let mut y = video::y4m::Y4m::new(std::fs::File::open(&path).unwrap()).unwrap();
        let step = (y.rate.0 / y.rate.1.max(1) / 30).max(1) as usize;
        let all: Vec<_> = std::iter::from_fn(|| y.next_frame()).step_by(step).take(frames).collect();
        println!("footage {path}: {}x{}, {} frames at 30 fps", y.width, y.height, all.len());
        all
    });
    let sizes: &[(usize, usize, u32)] = if footage.is_some() {
        &[(320, 180, 200), (480, 270, 350), (640, 360, 600), (960, 540, 1000), (1280, 720, 1500)]
    } else {
        &[(320, 240, 250), (480, 360, 400), (640, 480, 600), (1280, 720, 1500)]
    };
    for &(w, h, kbps) in sizes {
        let cfg = EncoderConfig {
            width: w,
            height: h,
            bitrate_bps: kbps * 1000,
            threads,
            speed,
            ..Default::default()
        };
        let mut enc: Box<dyn VideoEncoder> = match kind.as_str() {
            "rav1e" => Box::new(Encoder::new(&cfg).unwrap()),
            _ => Box::new(AomEncoder::new(&cfg).unwrap()),
        };
        let mut held = 0;
        let mut first = true;
        let mut dec = Decoder::new(threads, 4096 * 4096).unwrap();
        let mut pat = Pattern::new(w, h);
        let src: Vec<_> = match &footage {
            Some(f) => f.iter().map(|p| video::y4m::scale(p, w, h)).collect(),
            None => (0..frames).map(|_| pat.next_frame()).collect(),
        };
        let (mut et, mut dt, mut sizes, mut q) = (vec![], vec![], vec![], vec![]);
        let mut key = 0;
        for f in &src {
            let t = Instant::now();
            let pkts = enc.encode(f, false).unwrap();
            et.push(t.elapsed().as_secs_f64() * 1e3);
            if first && pkts.is_empty() {
                held += 1;
            }
            first &= pkts.is_empty();
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
            "{:>4}x{:<4} {:>7} | {:>7.1} {:>7.1} {:>7.1} | {:>6.1} {:>6.1} | {:>7} {:>6} {:>5.1} {:>4}",
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
            q.iter().sum::<f64>() / q.len() as f64,
            held
        );
    }
}
