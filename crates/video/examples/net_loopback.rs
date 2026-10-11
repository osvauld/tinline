//! Real call path on one machine: camera pattern -> libaom -> iroh (loopback) -> rav1d.
//!
//!     cargo run -p video --release --features net --example net_loopback -- [frames] [width] [height]
//!
//! Prints capture-to-decoded latency p50/p95 and what the rate control did.
use std::sync::Arc;
use std::time::{Duration, Instant};

use iroh::endpoint::presets;
use iroh::{Endpoint, SecretKey};
use video::media::SenderConfig;
use video::net::{RecvConfig, VideoRecv, VideoSend};
use video::{Decoder, EncoderConfig, Pattern, VideoEncoder, aom::AomEncoder};

const ALPN: &[u8] = b"tinline/video-loopback";

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let args: Vec<usize> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let frames = *args.first().unwrap_or(&300) as u64;
    let (w, h) = (*args.get(1).unwrap_or(&640), *args.get(2).unwrap_or(&360));
    let ea = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .unwrap();
    let eb = Endpoint::builder(presets::Minimal).secret_key(SecretKey::generate()).bind().await.unwrap();
    let ea2 = ea.clone();
    let acc = tokio::spawn(async move { ea2.accept().await.unwrap().await.unwrap() });
    let cb = eb.connect(ea.addr(), ALPN).await.unwrap();
    let ca = acc.await.unwrap();
    let (tx, rx) = tokio::join!(
        VideoSend::accept(ca, SenderConfig::default()),
        VideoRecv::new(cb, RecvConfig::default())
    );
    let (tx, mut rx) = (Arc::new(tx.unwrap()), rx.unwrap());
    let epoch = Instant::now();

    let txs = tx.clone();
    let sender = tokio::task::spawn_blocking(move || {
        let cfg = EncoderConfig { width: w, height: h, keyint: 100_000, threads: 2, ..Default::default() };
        let mut enc = AomEncoder::new(&cfg).unwrap();
        let mut pattern = Pattern::new(w, h);
        let mut bps = 0;
        for i in 0..frames {
            let due = epoch + Duration::from_micros(i * 1_000_000 / 30);
            std::thread::sleep(due.saturating_duration_since(Instant::now()));
            let pic = pattern.next_frame();
            let capture = epoch.elapsed().as_micros() as u64;
            if txs.bitrate() != bps {
                bps = txs.bitrate();
                enc.set_bitrate(bps).unwrap();
            }
            for p in enc.encode(&pic, txs.wants_keyframe()).unwrap() {
                txs.send(capture, p.key, &p.data).unwrap();
            }
        }
    });

    let mut dec = Decoder::new(2, 4096 * 4096).unwrap();
    let mut lat = Vec::new();
    let mut errors = 0;
    loop {
        let f = match tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
            Ok(f) => f.unwrap(),
            Err(_) => break,
        };
        match dec.decode(&f.payload) {
            Ok(pics) if !pics.is_empty() => {
                lat.push((epoch.elapsed().as_micros() as u64 - f.capture_us) as f64 / 1000.0)
            }
            Ok(_) => {}
            Err(_) => {
                errors += 1;
                rx.request_keyframe();
            }
        }
    }
    sender.await.unwrap();
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |p: f64| lat.get(((lat.len().max(1) - 1) as f64 * p) as usize).copied().unwrap_or(0.0);
    println!("{w}x{h}: {} of {frames} frames decoded, {errors} decode errors", lat.len());
    println!("latency capture -> decoded: p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms", pct(0.5), pct(0.95), pct(1.0));
    println!("bitrate now {} bps; send {:?}", tx.bitrate(), tx.stats());
    println!("recv {:?}", rx.stats());
}
