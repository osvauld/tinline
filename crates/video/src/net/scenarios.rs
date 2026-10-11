//! The simulation's call scenarios over real iroh through [`super::impair`], each run next to
//! [`crate::sim::run`] with the same link and seed. Real time: the 30 s ones are `#[ignore]`d,
//! run them alone on a quiet machine:
//! `cargo test -p video --release --features net net_real -- --ignored --nocapture --test-threads=1`.

use super::impair::{Link, Proxy};
use super::*;
use crate::sim::{self, FakeCodec, Report, Scenario, Shown, SimCodec};
use iroh::endpoint::{QuicTransportConfig, presets};
use iroh::{Endpoint, EndpointAddr, SecretKey, TransportAddr};

const ALPN: &[u8] = b"tinline/video-test";
const S: u64 = 1_000_000;

/// The transport config docs/video.md recommends for the call endpoint.
fn call_transport() -> QuicTransportConfig {
    let b = QuicTransportConfig::builder()
        .keep_alive_interval(Duration::from_secs(5))
        .max_concurrent_uni_streams(VarInt::from_u32(8));
    // `VIDEO_CC=bbr|reno` to compare with the default (Cubic).
    let b = match std::env::var("VIDEO_CC").as_deref() {
        Ok("bbr") => b.congestion_controller_factory(Arc::new(noq_proto::congestion::Bbr3Config::default())),
        Ok("reno") => b.congestion_controller_factory(Arc::new(noq_proto::congestion::NewRenoConfig::default())),
        _ => b,
    };
    b.build()
}

struct Path {
    proxy: Proxy,
    _eps: (Endpoint, Endpoint),
    /// Sends video (accepted the connection), behind `[::1]`.
    a: Connection,
    /// Receives, behind `127.0.0.1`.
    b: Connection,
}

async fn path(fwd: Link, back: Link, seed: u64) -> Path {
    let ea = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .clear_ip_transports()
        .bind_addr("[::1]:0")
        .unwrap()
        .transport_config(call_transport())
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .unwrap();
    let eb = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .transport_config(call_transport())
        .bind()
        .await
        .unwrap();
    let a_addr = *ea.bound_sockets().iter().find(|a| a.is_ipv6()).unwrap();
    let proxy = Proxy::new(a_addr, fwd, back, seed).await.unwrap();
    let ea2 = ea.clone();
    let acc = tokio::spawn(async move { ea2.accept().await.unwrap().await.unwrap() });
    let b = eb.connect(EndpointAddr::new(ea.id()).with_ip_addr(proxy.b_side), ALPN).await.unwrap();
    let a = acc.await.unwrap();
    Path { proxy, _eps: (ea, eb), a, b }
}

/// Every path of both connections goes through the proxy, and the proxy saw every byte.
fn assert_no_bypass(p: &Path) {
    for (conn, proxy_addr) in [(&p.a, p.proxy.a_side), (&p.b, p.proxy.b_side)] {
        for path in conn.paths().iter() {
            assert_eq!(path.remote_addr(), &TransportAddr::Ip(proxy_addr), "path bypasses the proxy");
        }
    }
    let g = super::impair::DirStats::get;
    let (ab, ba) = (&p.proxy.a_to_b, &p.proxy.b_to_a);
    assert_eq!(g(&ab.stray) + g(&ba.stray), 0);
    let (sa, sb) = (p.a.stats(), p.b.stats());
    // Whatever is still in flight (or in a socket buffer) when the stats are read.
    let slack = |x: u64, y: u64| x.abs_diff(y) <= 64 * 1024;
    assert!(slack(sa.udp_tx.bytes, g(&ab.in_bytes)), "A sent {} B, proxy got {} B", sa.udp_tx.bytes, g(&ab.in_bytes));
    assert!(slack(sb.udp_rx.bytes, g(&ab.out_bytes)), "B got {} B, proxy sent {} B", sb.udp_rx.bytes, g(&ab.out_bytes));
    assert!(slack(sb.udp_tx.bytes, g(&ba.in_bytes)));
    assert!(slack(sa.udp_rx.bytes, g(&ba.out_bytes)));
}

/// Real-transport counterpart of [`sim::Report`] plus what only the real run has.
struct Real {
    rep: Report,
    resets: u64,
    order_violations: u64,
}

/// Modelled codec time, as in [`Scenario`]: capture -> encoded and decoded -> shown.
const CODEC_US: u64 = 5_000 + 2_000;

/// Runs `sc` over iroh: [`FakeCodec`] frames at the sender's bitrate, 30 fps, for
/// `sc.duration_us` of real time. Outages hit both directions.
async fn run_real(sc: &Scenario) -> (Real, Path) {
    let fwd = Link::from_sim(&sc.link);
    let back = Link { outages: sc.link.outages.clone(), ..Link::delay(sc.link.delay_us, sc.link.jitter_us) };
    let p = path(fwd, back, sc.seed).await;
    let (tx, rx) = tokio::join!(
        VideoSend::accept(p.a.clone(), sc.sender.clone()),
        VideoRecv::new(p.b.clone(), RecvConfig { media: sc.receiver.clone(), ..Default::default() })
    );
    let (tx, mut rx) = (Arc::new(tx.unwrap()), rx.unwrap());
    let epoch = Instant::now();
    p.proxy.start_clock();
    let duration = sc.duration_us;
    let fps = sc.fps;
    let seed = sc.seed;
    let tx2 = tx.clone();
    let send = tokio::spawn(async move {
        let mut codec = FakeCodec::new(fps, seed);
        let mut iv = tokio::time::interval(Duration::from_micros(1_000_000 / fps as u64));
        iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut bitrate = Vec::new();
        let mut captured = 0;
        loop {
            iv.tick().await;
            let now = now_us(epoch);
            if now >= duration {
                return (captured, bitrate);
            }
            let (payload, key) = codec.encode(captured, tx2.wants_keyframe(), tx2.bitrate());
            tx2.send(now, key, &payload).unwrap();
            captured += 1;
            bitrate.push((now, tx2.bitrate()));
        }
    });
    let mut shown = Vec::new();
    let mut order_violations = 0;
    let mut last: Option<(u64, u32, u64)> = None;
    loop {
        let left = duration.saturating_sub(now_us(epoch));
        let Ok(f) = tokio::time::timeout(Duration::from_micros(left), rx.recv()).await else { break };
        let f = f.unwrap();
        let at = now_us(epoch);
        let seq = u64::from_be_bytes(f.payload[1..9].try_into().unwrap());
        let key = f.payload[0] == 1;
        let ok = key == (f.index == 0)
            && last.is_none_or(|(g, i, s)| {
                seq > s && (f.group, f.index) > (g, i) && (f.group != g || f.index == i + 1)
            });
        if !ok {
            order_violations += 1;
        }
        last = Some((f.group, f.index, seq));
        shown.push(Shown { at_us: at + CODEC_US, capture_us: f.capture_us });
    }
    let (captured, bitrate) = send.await.unwrap();
    let g = super::impair::DirStats::get;
    let ab = &p.proxy.a_to_b;
    let st = tx.stats();
    let rep = Report {
        captured,
        shown,
        decode_errors: order_violations,
        keyframes: tx.with_sender(|s| s.keyframes),
        sender_drops: tx.with_sender(|s| s.drops),
        skipped: rx.rx.skipped_frames,
        keyframe_requests: st.keyframe_requests,
        wire_bytes: g(&ab.in_bytes) + 28 * g(&ab.in_pkts),
        lost_packets: g(&ab.drop_queue) + g(&ab.drop_loss) + g(&ab.drop_outage),
        sent_packets: g(&ab.in_pkts),
        bitrate,
    };
    (Real { rep, resets: st.resets, order_violations }, p)
}

fn p95(r: &Report, from: u64, to: u64) -> f64 {
    Report::pct(&r.latencies_ms(from, to), 0.95)
}

fn frozen_ms(r: &Report, from: u64, to: u64) -> u64 {
    r.freezes(200_000, from, to).iter().map(|f| f.1).sum::<u64>() / 1000
}

/// Runs the scenario for real and in the simulation; prints both.
async fn compare(name: &str, sc: &Scenario) -> (Real, Report) {
    let sim = sim::run(sc, &mut FakeCodec::new(sc.fps, sc.seed));
    let (real, p) = run_real(sc).await;
    assert_no_bypass(&p);
    let d = sc.duration_us;
    let load = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    println!("{name:>10} sim : {}", sim.summary(d));
    println!("{name:>10} real: {}", real.rep.summary(d));
    for (who, r) in [("sim", &sim), ("real", &real.rep)] {
        let l = r.latencies_ms(2 * S, d);
        println!(
            "{name:>10} {who:>4}: p50 {:.0} p95 {:.0} ms (2 s..) | frozen >200ms {} ms (1 s..) | bitrate mean {:.0} kbps, last 10 s {:.0}",
            Report::pct(&l, 0.5),
            Report::pct(&l, 0.95),
            frozen_ms(r, S, d),
            r.mean_bitrate(0, d) / 1000.0,
            r.mean_bitrate(d - 10 * S, d) / 1000.0,
        );
        let l = r.latencies_ms(d - 15 * S, d);
        println!("{name:>10} {who:>4}: last 15 s p50 {:.0} p95 {:.0} ms", Report::pct(&l, 0.5), Report::pct(&l, 0.95));
    }
    let path = p.a.paths().iter().find(|x| x.is_selected()).map(|x| (x.rtt(), x.stats().cwnd));
    println!(
        "{name:>10} real: resets {} | rtt/cwnd {path:?} | lost {} B ({} pkts) | load {}\n           a->b {}\n           b->a {}",
        real.resets,
        p.a.stats().lost_bytes,
        p.a.stats().lost_packets,
        load.trim(),
        p.proxy.a_to_b,
        p.proxy.b_to_a,
    );
    (real, sim)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn net_impair_no_bypass() {
    let sc = Scenario { duration_us: 4 * S, ..Scenario::default() };
    let (real, p) = run_real(&sc).await;
    assert_no_bypass(&p);
    let rtt = p.a.paths().iter().find(|x| x.is_selected()).unwrap().rtt();
    assert!(rtt >= Duration::from_millis(40), "rtt {rtt:?}: not going through the 2x20 ms proxy");
    assert_eq!(real.order_violations, 0);
    assert!(real.rep.shown.len() as u64 >= real.rep.captured - 5, "{} of {}", real.rep.shown.len(), real.rep.captured);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "30 s real time"]
async fn net_real_clean() {
    let sc = Scenario::default();
    let (r, _) = compare("clean", &sc).await;
    let d = sc.duration_us;
    assert_eq!(r.order_violations, 0);
    assert!(r.rep.keyframes <= 2, "keyframes {}", r.rep.keyframes);
    assert!(p95(&r.rep, 2 * S, d) < 120.0, "p95 {}", p95(&r.rep, 2 * S, d));
    assert_eq!(frozen_ms(&r.rep, S, d), 0);
    assert!(r.rep.mean_bitrate(20 * S, d) > 1_000_000.0, "ramps up to use a 2 Mbps link");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "30 s real time"]
async fn net_real_loss_3pct() {
    let sc = Scenario { link: sim::Link { loss: 0.03, ..sim::Link::default() }, ..Scenario::default() };
    let (r, _) = compare("loss 3%", &sc).await;
    let d = sc.duration_us;
    assert_eq!(r.order_violations, 0);
    assert!(p95(&r.rep, 2 * S, d) < 250.0, "p95 {}", p95(&r.rep, 2 * S, d));
    assert!(r.rep.freezes(200_000, S, d).len() <= 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "30 s real time"]
async fn net_real_bandwidth_drop() {
    let sc = Scenario {
        link: sim::Link { rate_changes: vec![(10 * S, 300_000)], ..sim::Link::default() },
        ..Scenario::default()
    };
    let (r, _) = compare("2M->300k", &sc).await;
    let d = sc.duration_us;
    assert_eq!(r.order_violations, 0);
    assert!(r.rep.mean_bitrate(15 * S, d) < 300_000.0, "bitrate {}", r.rep.mean_bitrate(15 * S, d));
    assert!(p95(&r.rep, 15 * S, d) < 350.0, "p95 {}", p95(&r.rep, 15 * S, d));
    // No latency creep at the lower rate.
    assert!(p95(&r.rep, 25 * S, d) < 350.0, "p95 {}", p95(&r.rep, 25 * S, d));
    assert!(frozen_ms(&r.rep, 10 * S, d) < 2_000, "frozen {} ms after the drop", frozen_ms(&r.rep, 10 * S, d));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "30 s real time"]
async fn net_real_outage_500ms() {
    let sc = Scenario {
        link: sim::Link { outages: vec![(10 * S, 10 * S + 500_000)], ..sim::Link::default() },
        ..Scenario::default()
    };
    let (r, _) = compare("outage", &sc).await;
    let d = sc.duration_us;
    assert_eq!(r.order_violations, 0);
    // 500 ms dark, then video again within ~1-1.5 s.
    assert!(frozen_ms(&r.rep, 9 * S, d) < 2_000, "frozen {} ms", frozen_ms(&r.rep, 9 * S, d));
    assert!(p95(&r.rep, 13 * S, d) < 120.0, "back to low latency, p95 {}", p95(&r.rep, 13 * S, d));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "30 s real time"]
async fn net_real_relay_path() {
    let sc = Scenario {
        link: sim::Link { delay_us: 75_000, jitter_us: 20_000, rate_bps: 1_500_000, ..sim::Link::default() },
        ..Scenario::default()
    };
    let (r, _) = compare("relay", &sc).await;
    let d = sc.duration_us;
    assert_eq!(r.order_violations, 0);
    assert!(p95(&r.rep, 2 * S, d) < 300.0, "p95 {}", p95(&r.rep, 2 * S, d));
}
