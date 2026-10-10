//! Two iroh endpoints on loopback (no relay, no discovery).

use super::*;
use iroh::endpoint::presets;
use iroh::{Endpoint, SecretKey};

const ALPN: &[u8] = b"tinline/video-test";

struct Pair {
    _eps: (Endpoint, Endpoint),
    /// Sending side (accepted the connection).
    a: Connection,
    b: Connection,
}

async fn pair() -> Pair {
    let ea = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .unwrap();
    let eb = Endpoint::builder(presets::Minimal).secret_key(SecretKey::generate()).bind().await.unwrap();
    let ea2 = ea.clone();
    let acc = tokio::spawn(async move { ea2.accept().await.unwrap().await.unwrap() });
    let b = eb.connect(ea.addr(), ALPN).await.unwrap();
    let a = acc.await.unwrap();
    Pair { _eps: (ea, eb), a, b }
}

async fn video(p: &Pair, rcfg: RecvConfig) -> (Arc<VideoSend>, VideoRecv) {
    let (tx, rx) = tokio::join!(
        VideoSend::accept(p.a.clone(), SenderConfig::default()),
        VideoRecv::new(p.b.clone(), rcfg)
    );
    (Arc::new(tx.unwrap()), rx.unwrap())
}

fn payload(seq: u64, key: bool, size: usize) -> Vec<u8> {
    let mut p = vec![0u8; size.max(9)];
    p[0] = key as u8;
    p[1..9].copy_from_slice(&seq.to_be_bytes());
    for (i, b) in p[9..].iter_mut().enumerate() {
        *b = (seq as usize * 31 + i) as u8;
    }
    p
}

/// Checks the payload is intact; returns (key, seq).
fn check(p: &[u8]) -> (bool, u64) {
    let seq = u64::from_be_bytes(p[1..9].try_into().unwrap());
    for (i, b) in p[9..].iter().enumerate() {
        assert_eq!(*b, (seq as usize * 31 + i) as u8, "payload of frame {seq} corrupted");
    }
    (p[0] == 1, seq)
}

/// Frames at `every`, sized for the sender's bitrate at 30 fps; a keyframe when asked or every
/// `key_every` frames. `lag(now)` is subtracted from the capture time.
fn spawn_sender(
    tx: Arc<VideoSend>,
    frames: u64,
    every: Duration,
    key_every: u64,
    lag: fn(u64) -> u64,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let start = Instant::now();
        let mut iv = tokio::time::interval(every);
        for seq in 0..frames {
            iv.tick().await;
            let now = now_us(start);
            let key = tx.wants_keyframe() || (key_every > 0 && seq % key_every == 0);
            let mean = tx.bitrate() as usize / 8 / 30;
            let size = if key { mean * 4 } else { mean };
            tx.send(now - lag(now), key, &payload(seq, key, size)).unwrap();
        }
    })
}

/// Asserts decode order: (group, index) increasing, keyframe exactly at index 0.
#[derive(Default)]
struct Order {
    last: Option<(u64, u32, u64)>,
}

impl Order {
    fn push(&mut self, f: &Frame) -> u64 {
        let (key, seq) = check(&f.payload);
        assert_eq!(key, f.index == 0, "frame {seq}: keyframe must start its group");
        if let Some((g, i, s)) = self.last {
            assert!((f.group, f.index) > (g, i), "out of order: {:?} after {:?}", (f.group, f.index), (g, i));
            assert!(seq > s);
            if f.group == g {
                assert_eq!(f.index, i + 1);
            }
        }
        self.last = Some((f.group, f.index, seq));
        seq
    }
}

fn no_lag(_: u64) -> u64 {
    0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn net_frames_arrive_in_order() {
    let p = pair().await;
    let (tx, mut rx) = video(&p, RecvConfig::default()).await;
    let send = spawn_sender(tx.clone(), 300, Duration::from_millis(5), 60, no_lag);
    let mut order = Order::default();
    let mut seqs = Vec::new();
    let mut groups = std::collections::BTreeSet::new();
    while seqs.len() < 300 {
        let f = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.expect("stalled").unwrap();
        seqs.push(order.push(&f));
        groups.insert(f.group);
    }
    send.await.unwrap();
    assert_eq!(seqs, (0..300).collect::<Vec<_>>());
    assert_eq!(groups.len(), 5);
    assert_eq!(tx.with_sender(|s| s.drops), 0);
    assert_eq!(tx.stats().resets, 0);
    let rs = rx.stats();
    assert_eq!((rs.rejected, rs.resets), (0, 0));
    println!("send {:?}\nrecv {:?}", tx.stats(), rs);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn net_stalled_receiver_resets_and_resumes() {
    let p = pair().await;
    let (tx, mut rx) = video(&p, RecvConfig::default()).await;
    // 30 fps for 3 s; the receiver reads 1 s, pauses 1 s, reads again.
    let send = spawn_sender(tx.clone(), 90, Duration::from_millis(33), 0, no_lag);
    let start = Instant::now();
    let mut order = Order::default();
    let mut before = None;
    while start.elapsed() < Duration::from_secs(1) {
        let f = rx.recv().await.unwrap();
        order.push(&f);
        before = Some(f.group);
    }
    let before = before.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(tx.stats().resets >= 1, "sender must give up on the stalled group");
    assert!(tx.with_sender(|s| s.drops) >= 1);
    let mut new_group = None;
    let mut late = Vec::new();
    loop {
        let Ok(f) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await else { break };
        let f = f.unwrap();
        let seq = order.push(&f);
        if f.group > before && new_group.is_none() {
            assert_eq!(f.index, 0);
            new_group = Some((f.group, seq));
        }
        late.push(seq);
    }
    send.await.unwrap();
    let (g, first) = new_group.expect("never resumed from a new group");
    assert_eq!(*late.last().unwrap(), 89, "last frame must arrive");
    assert!(tx.with_sender(|s| s.keyframes) >= 2);
    println!("resumed at group {g} frame {first}; send {:?}\nrecv {:?}", tx.stats(), rx.stats());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn net_keyframe_request_starts_group() {
    let p = pair().await;
    let (tx, mut rx) = video(&p, RecvConfig::default()).await;
    let send = spawn_sender(tx.clone(), 60, Duration::from_millis(20), 0, no_lag);
    let mut order = Order::default();
    for _ in 0..20 {
        let f = rx.recv().await.unwrap();
        order.push(&f);
        assert_eq!(f.group, 0);
    }
    rx.request_keyframe();
    let f = loop {
        let f = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await.expect("no new group").unwrap();
        order.push(&f);
        if f.group > 0 {
            break f;
        }
    };
    assert_eq!((f.group, f.index), (1, 0));
    assert!(tx.stats().keyframe_requests >= 1);
    send.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn net_rate_reports_lower_bitrate() {
    let p = pair().await;
    let (tx, mut rx) = video(&p, RecvConfig::default()).await;
    let start_bps = tx.bitrate();
    // Capture times fall behind by 10% of elapsed time after 0.5 s: a growing queue.
    fn lag(now: u64) -> u64 {
        now.saturating_sub(500_000) / 10
    }
    let send = spawn_sender(tx.clone(), 75, Duration::from_millis(33), 0, lag);
    let mut order = Order::default();
    let mut min_bps = u32::MAX;
    let reader = async {
        loop {
            let f = rx.recv().await.unwrap();
            order.push(&f);
        }
    };
    let watch = async {
        while !send.is_finished() {
            min_bps = min_bps.min(tx.bitrate());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::select! {
        _ = reader => {}
        _ = watch => {}
    }
    let st = tx.stats();
    println!("bitrate {start_bps} -> min {min_bps}; send {st:?}");
    assert!(st.reports >= 15, "reports: {}", st.reports);
    assert!(min_bps < start_bps * 8 / 10, "bitrate {min_bps} not lowered from {start_bps}");
}

/// The peer's raw half: accepts the control stream and writes group streams by hand.
async fn evil_peer(p: &Pair, cfg: RecvConfig) -> VideoRecv {
    let (rx, ctrl) = tokio::join!(VideoRecv::new(p.b.clone(), cfg), p.a.accept_bi());
    std::mem::forget(ctrl.unwrap());
    rx.unwrap()
}

async fn raw_group(c: &Connection, data: &[u8], finish: bool) -> SendStream {
    let mut s = c.open_uni().await.unwrap();
    s.write_all(data).await.unwrap();
    if finish {
        s.finish().unwrap();
    }
    s
}

fn frame(group: Option<u64>, len: u32, payload: &[u8]) -> Vec<u8> {
    let mut d = group.map(|g| g.to_be_bytes().to_vec()).unwrap_or_default();
    d.extend_from_slice(&len.to_be_bytes());
    d.extend_from_slice(&0u64.to_be_bytes());
    d.extend_from_slice(payload);
    d
}

/// Runs `recv` for a while, expecting nothing.
async fn idle(rx: &mut VideoRecv) {
    let r = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await;
    assert!(r.is_err(), "unexpected {r:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn net_malicious_streams_are_rejected() {
    let p = pair().await;
    let mut rx = evil_peer(&p, RecvConfig::default()).await;
    let c = &p.a;
    let mut keep = Vec::new();
    // Frame header claiming 1 GiB: rejected at the header, nothing buffered.
    keep.push(raw_group(c, &frame(Some(1), 1 << 30, b""), false).await);
    idle(&mut rx).await;
    assert_eq!(rx.stats().rejected, 1);
    // Short header, then an absurd group id.
    raw_group(c, b"abc", true).await;
    keep.push(raw_group(c, &frame(Some(u64::MAX), 3, b"key"), false).await);
    idle(&mut rx).await;
    assert_eq!(rx.stats().rejected, 2);
    // A valid group still plays.
    keep.push(raw_group(c, &frame(Some(5), 2, b"k5"), false).await);
    let f = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await.unwrap().unwrap();
    assert_eq!((f.group, f.index, &f.payload[..]), (5, 0, &b"k5"[..]));
    // Duplicate and older groups are refused.
    raw_group(c, &frame(Some(5), 3, b"dup"), true).await;
    raw_group(c, &frame(Some(3), 3, b"old"), true).await;
    idle(&mut rx).await;
    assert_eq!(rx.stats().stale_streams, 2);
    assert_eq!(rx.stats().rejected, 2);
    // Group 5 continues.
    keep[2].write_all(&frame(None, 2, b"d1")).await.unwrap();
    let f = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await.unwrap().unwrap();
    assert_eq!((f.group, f.index, &f.payload[..]), (5, 1, &b"d1"[..]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn net_too_many_groups_is_an_error() {
    let p = pair().await;
    let mut rx = evil_peer(&p, RecvConfig { max_groups: 4, ..Default::default() }).await;
    let mut keep = Vec::new();
    for g in 10..20u64 {
        // Header plus half a frame header: never playable, held open.
        let mut d = g.to_be_bytes().to_vec();
        d.extend_from_slice(&[0, 0, 1]);
        keep.push(raw_group(&p.a, &d, false).await);
        // Spaced out: streams arriving together race, and the older ones are refused as stale.
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let r = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await.unwrap();
    assert!(matches!(r, Err(NetError::Protocol(_))), "{r:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn net_buffer_limit_is_an_error() {
    let p = pair().await;
    let mut rx = evil_peer(&p, RecvConfig { max_buffered: 1 << 20, ..Default::default() }).await;
    let mut s = raw_group(&p.a, &frame(Some(0), 3, b"key"), false).await;
    let f = rx.recv().await.unwrap();
    assert_eq!(f.index, 0);
    // 2 MB of valid frames in one group: the receiver would have to keep them all.
    let writer = tokio::spawn(async move {
        let chunk = frame(None, 20_000, &[7u8; 20_000]);
        for _ in 0..100 {
            if s.write_all(&chunk).await.is_err() {
                break;
            }
        }
    });
    let r = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Err(e) = rx.recv().await {
                return e;
            }
        }
    })
    .await
    .unwrap();
    assert!(matches!(r, NetError::Protocol(_)), "{r:?}");
    writer.abort();
}

#[test]
fn control_round_trip() {
    let msgs = [
        Control::Report(RateReport { delivered_bps: 123_456, queuing_us: 7_890 }),
        Control::Keyframe,
        Control::Ack { group: 42, offset: 1 << 40 },
    ];
    let mut b = Vec::new();
    for m in &msgs {
        m.encode(&mut b);
    }
    let mut got = Vec::new();
    let mut at = 0;
    while let Some((m, n)) = Control::decode(&b[at..]).unwrap() {
        got.push(m);
        at += n;
    }
    assert_eq!(got, msgs);
    assert_eq!(at, b.len());
    assert!(Control::decode(&b[..5]).unwrap().is_none());
    assert!(Control::decode(&[1, 0, 0]).unwrap().is_none());
    assert!(Control::decode(&[9]).is_err());
}
