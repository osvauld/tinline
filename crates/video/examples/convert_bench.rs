//! Frame conversion cost per frame (median ms): capture paths into the encoder and the render
//! path out of the decoder. `cargo run -p video --release --example convert_bench -- [iters]`

use std::hint::black_box;
use std::time::Instant;

use video::I420;
use video::convert::{self, Prepare, Range, RgbLayout, Rotation, Yuv420};

fn time(name: &str, iters: usize, mut f: impl FnMut()) {
    f();
    let mut t: Vec<f64> = (0..iters)
        .map(|_| {
            let s = Instant::now();
            f();
            s.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    t.sort_by(f64::total_cmp);
    println!("{name:<52} {:>7.3} ms  (p95 {:.3})", t[iters / 2], t[iters * 95 / 100]);
}

/// Android-like YUV_420_888 buffers, pixel stride 2 (NV21 layout), rows padded to 64 bytes.
fn camera(w: usize, h: usize) -> (Vec<u8>, usize, Vec<u8>, usize) {
    let stride = w.next_multiple_of(64);
    let y = (0..(h - 1) * stride + w).map(|i| (i * 7 % 251) as u8).collect();
    let uv = (0..(h / 2 - 1) * stride + w).map(|i| (i * 13 % 241) as u8).collect();
    (y, stride, uv, stride)
}

fn main() {
    let iters: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(200);
    let prep = |rotate, w, h| Prepare { rotate, mirror: false, width: w, height: h, range: Range::Full };

    for (cw, ch, rot, ow, oh) in [
        (640, 480, Rotation::R90, 360, 640),
        (640, 480, Rotation::R90, 640, 360),
        (640, 480, Rotation::R0, 640, 360),
        (1280, 720, Rotation::R90, 360, 640),
        (1280, 720, Rotation::R0, 960, 540),
    ] {
        let (y, ys, uv, uvs) = camera(cw, ch);
        let src = Yuv420::nv12(&y, ys, &uv, uvs, cw, ch, true);
        let p = prep(rot, ow, oh);
        time(&format!("YUV_420_888 ps2 {cw}x{ch} {rot:?} -> {ow}x{oh}"), iters, || {
            black_box(convert::prepare(&src, None, &p).unwrap());
        });
    }

    for (w, h) in [(640, 360), (1280, 720)] {
        let pic = I420 { width: w, height: h, data: (0..I420::len(w, h)).map(|i| (i % 251) as u8).collect() };
        let mut out = vec![0; w * h * 4];
        time(&format!("I420 {w}x{h} -> RGBA"), iters, || convert::i420_to_rgba(&pic, &mut out).unwrap());
    }

    let yuyv: Vec<u8> = (0..640 * 480 * 2).map(|i| (i % 253) as u8).collect();
    time("YUYV 640x480 -> I420", iters, || {
        black_box(convert::yuyv_to_i420(&yuyv, 1280, 640, 480).unwrap());
    });
    let bgra: Vec<u8> = (0..1280 * 720 * 4).map(|i| (i % 253) as u8).collect();
    time("BGRA 1280x720 -> I420", iters, || {
        black_box(convert::rgb_to_i420(&bgra, 5120, 1280, 720, RgbLayout::Bgra).unwrap());
    });
}
