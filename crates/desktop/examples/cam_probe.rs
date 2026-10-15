//! Camera smoke test: `cargo run -p desktop --release --example cam_probe [pattern]`. Grabs 30
//! frames from the first camera, prints format, size, fps, luma range and the per-frame cost of
//! the desktop video path, then closes the camera.

#![allow(dead_code)]

#[path = "../src/camera.rs"]
mod camera;
#[path = "../src/frames.rs"]
mod frames;

use std::sync::mpsc;
use std::time::{Duration, Instant};

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

fn time(n: usize, mut f: impl FnMut()) -> f64 {
    median(
        (0..n)
            .map(|_| {
                let t = Instant::now();
                f();
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect(),
    )
}

fn main() {
    let fake = std::env::args().nth(1);
    let Some(src) = camera::find(fake.as_deref()) else {
        println!("no camera");
        return;
    };
    println!("source: {src:?}");
    let (tx, rx) = mpsc::channel();
    let (otx, orx) = mpsc::channel();
    let t0 = Instant::now();
    let cam = camera::Camera::start_with_quality(
        src,
        camera::Quality::Auto,
        move |o| {
            let _ = otx.send(o);
        },
        move |p| {
            let _ = tx.send((Instant::now(), p));
        },
    );
    match orx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(o)) => println!("opened in {:.0} ms: {} {}x{}", t0.elapsed().as_secs_f64() * 1e3, o.format, o.width, o.height),
        Ok(Err(e)) => return println!("error: {e}"),
        Err(_) => return println!("timeout opening"),
    }
    let mut got = Vec::new();
    while got.len() < 30 {
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(f) => got.push(f),
            Err(_) => break,
        }
    }
    drop(cam);
    if got.len() < 2 {
        return println!("only {} frames", got.len());
    }
    // The first frames come as soon as streaming starts; rate from frame 5 on.
    let skip = 5.min(got.len() - 2);
    let span = got.last().unwrap().0 - got[skip].0;
    let fps = (got.len() - 1 - skip) as f64 / span.as_secs_f64();
    let p = &got.last().unwrap().1;
    let y = &p.data[..p.width * p.height];
    let (lo, hi) = (y.iter().min().unwrap(), y.iter().max().unwrap());
    println!("frames {} size {}x{} fps {fps:.1} luma {lo}..{hi}", got.len(), p.width, p.height);

    let yuyv: Vec<u8> = (0..p.width * p.height * 2).map(|i| (i * 7) as u8).collect();
    let conv = time(30, || drop(frames::to_i420(frames::Pix::Yuyv, &yuyv, 0, p.width, p.height, video::convert::Range::Limited)));
    let selfv = time(30, || drop(frames::self_view(p)));
    let hd = video::convert::scale(p, 1280, 720);
    let mut out = Vec::new();
    let rgba720 = time(30, || {
        let r = frames::rgba(&hd);
        out = r.data;
    });
    let handle = time(30, || drop(iced::widget::image::Handle::from_rgba(1280, 720, out.clone())));
    println!(
        "ms/frame (median): yuyv->i420 {}x{} {conv:.2}, self-view {selfv:.2}, i420->rgba 1280x720 {rgba720:.2}, \
         Handle::from_rgba 720p (incl. copy) {handle:.2}",
        p.width, p.height
    );
    // Let the capture thread see the stop flag and close the device.
    std::thread::sleep(Duration::from_millis(200));
}
