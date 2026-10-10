//! How many frames does the encoder hold before the first packet, and per-frame cost.
use std::time::Instant;
use video::{Encoder, EncoderConfig, Pattern};

fn main() {
    let (w, h) = (640, 480);
    let mut enc = Encoder::new(&EncoderConfig { width: w, height: h, ..Default::default() }).unwrap();
    let mut pat = Pattern::new(w, h);
    for i in 0..12 {
        let f = pat.next_frame();
        let t = Instant::now();
        let p = enc.encode(&f, false).unwrap();
        let frames: Vec<_> = p.iter().map(|p| (p.frame, p.data.len(), p.key)).collect();
        println!("in {i:2}: {:6.1} ms -> {frames:?}", t.elapsed().as_secs_f64() * 1e3);
    }
}
