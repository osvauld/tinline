//! Simulated call members, all in one process so they share one clock: each has its own iroh
//! endpoint, sends real 32 kbps Opus from a synthetic talker through the G6-D1 silence gate
//! (`Sender::encode_group`), and receives through one `Receiver::new_group` per forwarded slot,
//! honouring the host's `Active` datagrams, and mixes.
//!
//!     members --host 192.168.1.3:7700 --members 7 --participants 8 --secs 600
//!     members --direct --secs 180         # GC-D09 1:1 baseline: two endpoints, loopback, `encode`
//!
//! Measured: transit (send of a datagram -> arrival at another member via the host),
//! mouth-to-ear (start of a talk spurt at the sender -> first audible pulled frame at a receiver,
//! i.e. network + jitter buffer + one frame of capture; no device audio I/O), RTT to the host
//! (ping datagrams), and receiver loss/concealment counters. One `SUMMARY` line at the end.
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

use audio::{FRAME, GroupFrame, Receiver, Sender, Stats};
use bytes::Bytes;
use gcall_bench::*;
use iroh::endpoint::{Connection, presets};
use iroh::{Endpoint, EndpointAddr};
use parking_lot::Mutex;

const RING: usize = 4096;
/// Output RMS above which a pulled frame counts as audible (talk RMS is ~400-4500, the noise
/// floor ~20).
const AUDIBLE_RMS: f32 = 200.0;
/// An onset more than this after the spurt began was a late switch-in, not pipeline latency.
const LATE_ONSET_US: u64 = 1_000_000;

#[derive(Default)]
struct Shared {
    start: Option<Instant>,
    sends: Vec<Mutex<Vec<(u32, u64)>>>,
    onsets: Vec<Mutex<VecDeque<u64>>>,
    transit: Mutex<Vec<u32>>,
    m2e: Mutex<Vec<u32>>,
    late_onsets: AtomicU64,
    rtt: Mutex<Vec<u32>>,
    targets: Mutex<Vec<u32>>,
    stats: Mutex<Vec<Stats>>,
    tx_pkts: AtomicU64,
    tx_bytes: AtomicU64,
    rx_pkts: AtomicU64,
    rx_bytes: AtomicU64,
    udp_tx: AtomicU64,
    udp_rx: AtomicU64,
    spurts: AtomicU64,
    /// Opt-in diagnostic I/O; deliberately disabled for clean capacity runs.
    trace: Option<Mutex<BufWriter<File>>>,
    levels: Option<Mutex<BufWriter<File>>>,
    features: Option<Mutex<feature_trace::Recorder>>,
}

impl Shared {
    fn trace(&self, kind: &str, source: u8, receiver: u8, seq: u32, at: u64) {
        if let Some(out) = &self.trace {
            writeln!(out.lock(), "{kind} {source} {receiver} {seq} {at}")
                .expect("write onset trace");
        }
    }

    fn level_trace(
        &self,
        kind: &str,
        source: u8,
        receiver: u8,
        level: u8,
        audible: bool,
        candidate: bool,
        at: u64,
    ) {
        if let Some(out) = &self.levels {
            writeln!(
                out.lock(),
                "{at} {receiver} {source} {kind} {level} {} {}",
                u8::from(audible),
                u8::from(candidate)
            )
            .expect("write diagnostic level trace");
        }
    }

    fn features(
        &self,
        kind: &'static str,
        source: u8,
        receiver: u8,
        level: u8,
        audible: bool,
        eligible: bool,
        at: u64,
        pcm: &[i16],
    ) {
        if let Some(out) = &self.features {
            out.lock()
                .record(kind, source, receiver, level, audible, eligible, at, pcm);
        }
    }

    fn us(&self) -> u64 {
        self.start.unwrap().elapsed().as_micros() as u64
    }
}

struct Rx {
    group: bool,
    receivers: Vec<Option<Receiver>>,
    audible: [bool; MAX_SLOTS],
    last_matched: [u64; MAX_SLOTS],
    active_gen: u32,
    active: Vec<u8>,
}

struct Member {
    slot: u8,
    /// Direct (1:1) mode: the other end's slot; datagrams carry no type/slot framing.
    direct_peer: Option<u8>,
    conn: Connection,
    sh: Arc<Shared>,
    rx: Mutex<Rx>,
}

impl Member {
    fn send(&self, b: Vec<u8>) {
        let n = b.len() as u64;
        let audio = if self.direct_peer.is_some() {
            Some(b.as_slice())
        } else if b.first() == Some(&T_AUDIO_UP) {
            Some(&b[1..])
        } else {
            None
        };
        let seq = audio
            .filter(|d| d.len() >= 4)
            .map(|d| u32::from_be_bytes(d[..4].try_into().unwrap()));
        if self.conn.send_datagram(Bytes::from(b)).is_ok() {
            if let Some(seq) = seq {
                self.sh
                    .trace("send", self.slot, self.slot, seq, self.sh.us());
            }
            self.sh.tx_pkts.fetch_add(1, Relaxed);
            self.sh.tx_bytes.fetch_add(n, Relaxed);
        }
    }

    fn on_audio(&self, from: u8, audio: &[u8]) {
        let now = self.sh.us();
        if audio.len() <= audio::HEADER {
            return;
        }
        let seq = u32::from_be_bytes([audio[0], audio[1], audio[2], audio[3]]);
        self.sh.trace("receive", from, self.slot, seq, now);
        if from != 0 {
            let (s, t) = self.sh.sends[from as usize].lock()[seq as usize % RING];
            if s == seq && t > 0 && now >= t {
                self.sh.transit.lock().push((now - t) as u32);
            }
        }
        let mut rx = self.rx.lock();
        let group = rx.group;
        rx.receivers[from as usize]
            .get_or_insert_with(|| {
                if group {
                    Receiver::new_group().unwrap()
                } else {
                    Receiver::new().unwrap()
                }
            })
            .push(audio, now / 1000);
    }

    fn on_datagram(&self, d: &[u8]) {
        self.sh.rx_pkts.fetch_add(1, Relaxed);
        self.sh.rx_bytes.fetch_add(d.len() as u64, Relaxed);
        if let Some(p) = self.direct_peer {
            return self.on_audio(p, d);
        }
        match d.first() {
            Some(&T_AUDIO_DOWN) if d.len() > 2 => self.on_audio(d[1], &d[2..]),
            Some(&T_ACTIVE) if d.len() >= 5 => {
                let g = u32::from_be_bytes([d[1], d[2], d[3], d[4]]);
                let mut rx = self.rx.lock();
                if g > rx.active_gen {
                    rx.active_gen = g;
                    rx.active = d[5..].to_vec();
                    let act = rx.active.clone();
                    for (i, r) in rx.receivers.iter_mut().enumerate() {
                        if let Some(r) = r {
                            r.set_active(act.contains(&(i as u8)));
                        }
                    }
                }
            }
            Some(&T_PONG) if d.len() == 9 => {
                let t = u64::from_be_bytes(d[1..9].try_into().unwrap());
                self.sh.rtt.lock().push((self.sh.us() - t) as u32);
            }
            _ => {}
        }
    }

    /// Pull one frame per slot, detect audible onsets, mix.
    fn pull(&self) {
        let now = self.sh.us();
        let mut rx = self.rx.lock();
        let mut frames: Vec<[i16; FRAME]> = Vec::new();
        for slot in 0..MAX_SLOTS {
            let Some(r) = rx.receivers[slot].as_mut() else {
                continue;
            };
            let mut o = [0i16; FRAME];
            let before = r.stats();
            r.pull(&mut o, now / 1000);
            let after = r.stats();
            let idle = r.is_idle();
            let loud = audio::rms(&o) > AUDIBLE_RMS;
            // Diagnostic cached source byte sampled AFTER pull. Recovery/PLC has no matching
            // source metadata: do not claim those frames carry a fresh trustworthy level.
            let candidate = playout_level::candidate(
                loud,
                idle,
                after.last_level,
                before.lost,
                after.lost,
                before.concealed,
                after.concealed,
            );
            self.sh.level_trace(
                if rx.group { "group" } else { "direct" },
                slot as u8,
                self.slot,
                after.last_level,
                loud,
                candidate,
                now,
            );
            self.sh.features(
                if rx.group { "group" } else { "direct" },
                slot as u8,
                self.slot,
                after.last_level,
                loud,
                candidate,
                now,
                &o,
            );
            if loud && !rx.audible[slot] {
                self.sh.trace("audible", slot as u8, self.slot, 0, now);
            }
            if loud && !rx.audible[slot] && slot != 0 {
                let mouth = self.sh.onsets[slot]
                    .lock()
                    .iter()
                    .rev()
                    .find(|&&m| m <= now)
                    .copied();
                if let Some(m) = mouth
                    && m > rx.last_matched[slot]
                {
                    rx.last_matched[slot] = m;
                    if now - m > LATE_ONSET_US {
                        self.sh.late_onsets.fetch_add(1, Relaxed);
                    } else {
                        self.sh.m2e.lock().push((now - m) as u32);
                    }
                }
            }
            rx.audible[slot] = loud;
            frames.push(o);
        }
        let refs: Vec<&[i16]> = frames.iter().map(|f| &f[..]).collect();
        let mut mixed = [0i16; FRAME];
        audio::mix(&refs, &mut mixed);
        std::hint::black_box(&mixed);
    }

    fn finish(&self) {
        let rx = self.rx.lock();
        for r in rx.receivers.iter().flatten() {
            self.sh.stats.lock().push(r.stats());
        }
        let s = self.conn.stats();
        self.sh.udp_tx.fetch_add(s.udp_tx.bytes, Relaxed);
        self.sh.udp_rx.fetch_add(s.udp_rx.bytes, Relaxed);
    }
}

async fn run_member(
    m: Arc<Member>,
    duty: f32,
    secs: u64,
    schedule: Option<(talk_pattern::TalkPattern, u8)>,
) {
    m.sh.trace("ready", m.slot, m.slot, 0, m.sh.us());
    let mut was_talking = false;
    let rd = m.clone();
    let reader = tokio::spawn(async move {
        while let Ok(d) = rd.conn.read_datagram().await {
            rd.on_datagram(&d);
        }
    });
    let group = m.direct_peer.is_none();
    let mut voice = Voice::new(7 + m.slot as u64 * 7919, duty);
    let mut sender = Sender::new(32_000).unwrap();
    let mut gated = 0u32;
    let mut pcm = [0i16; FRAME];
    let mut iv = tokio::time::interval(Duration::from_millis(FRAME_MS));
    let frames = secs * 1000 / FRAME_MS;
    for i in 0..frames {
        iv.tick().await;
        let vf = if let Some((pattern, members)) = schedule {
            let tick = m.sh.us() / (FRAME_MS * 1000);
            voice.next_scheduled_frame(&mut pcm, pattern.talking(m.slot, members, tick))
        } else {
            voice.next_frame(&mut pcm)
        };
        let now = m.sh.us();
        if was_talking && !vf.talking {
            m.sh.trace(
                "speech_end",
                m.slot,
                m.slot,
                0,
                now.saturating_sub(FRAME_MS * 1000),
            );
        }
        was_talking = vf.talking;
        if vf.onset {
            m.sh.trace(
                "onset",
                m.slot,
                m.slot,
                0,
                now.saturating_sub(FRAME_MS * 1000),
            );
            m.sh.spurts.fetch_add(1, Relaxed);
            let mut q = m.sh.onsets[m.slot as usize].lock();
            // The spurt's first sample was captured one frame before this tick.
            q.push_back(now.saturating_sub(FRAME_MS * 1000));
            if q.len() > 64 {
                q.pop_front();
            }
        }
        let local_level;
        let audio = if group {
            match sender.encode_group(&pcm).unwrap() {
                GroupFrame::Opus(d) => {
                    local_level = d[4];
                    gated = 0;
                    Some(d)
                }
                GroupFrame::Gated { level } => {
                    local_level = level;
                    if gated % LEVEL_EVERY == 0 {
                        m.send(vec![T_LEVEL, level]);
                    }
                    gated += 1;
                    None
                }
            }
        } else {
            let d = sender.encode(&pcm).unwrap();
            local_level = d[4];
            Some(d)
        };
        m.sh.level_trace(
            "local",
            m.slot,
            m.slot,
            local_level,
            audio::rms(&pcm) > AUDIBLE_RMS,
            true,
            now,
        );
        m.sh.features(
            "local",
            m.slot,
            m.slot,
            local_level,
            audio::rms(&pcm) > AUDIBLE_RMS,
            true,
            now,
            &pcm,
        );
        if let Some(d) = audio {
            let seq = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
            m.sh.sends[m.slot as usize].lock()[seq as usize % RING] = (seq, now);
            if group {
                let mut v = Vec::with_capacity(d.len() + 1);
                v.push(T_AUDIO_UP);
                v.extend_from_slice(&d);
                m.send(v);
            } else {
                m.send(d);
            }
        }
        if group && i % 50 == 0 {
            let mut v = vec![T_PING];
            v.extend_from_slice(&m.sh.us().to_be_bytes());
            m.send(v);
        }
        m.pull();
        if i % 50 == 25 {
            let rx = m.rx.lock();
            for r in rx.receivers.iter().flatten() {
                if !r.is_idle() {
                    m.sh.targets.lock().push(r.stats().target_ms);
                }
            }
        }
    }
    if let Some(out) = &m.sh.features {
        out.lock().finish(m.slot, m.sh.us());
    }
    m.sh.level_trace("end", m.slot, m.slot, 127, false, false, m.sh.us());
    m.sh.trace("finish", m.slot, m.slot, 0, m.sh.us());
    m.finish();
    reader.abort();
}

async fn endpoint(slot: u8, port: Option<u16>) -> Endpoint {
    let mut b = Endpoint::builder(presets::Minimal)
        .secret_key(secret(slot))
        .alpns(vec![ALPN.to_vec()])
        .transport_config(transport());
    if let Some(p) = port {
        b = b
            .bind_addr(format!("127.0.0.1:{p}").as_str())
            .expect("bind addr");
    }
    b.bind().await.expect("bind")
}

fn new_member(
    slot: u8,
    direct_peer: Option<u8>,
    conn: Connection,
    sh: &Arc<Shared>,
) -> Arc<Member> {
    Arc::new(Member {
        slot,
        direct_peer,
        conn,
        sh: sh.clone(),
        rx: Mutex::new(Rx {
            group: direct_peer.is_none(),
            receivers: (0..MAX_SLOTS).map(|_| None).collect(),
            audible: [false; MAX_SLOTS],
            last_matched: [0; MAX_SLOTS],
            active_gen: 0,
            active: Vec::new(),
        }),
    })
}

fn summary(sh: &Shared, label: &str, secs: u64) {
    let mut tr = sh.transit.lock().clone();
    let mut me = sh.m2e.lock().clone();
    let mut rt = sh.rtt.lock().clone();
    let mut tg = sh.targets.lock().clone();
    if let Some(out) = &sh.trace {
        out.lock().flush().expect("flush onset trace");
    }
    if let Some(out) = &sh.levels {
        out.lock().flush().expect("flush level trace");
    }
    if let Some(out) = &sh.features {
        out.lock().flush();
    }
    let st = sh.stats.lock();
    let sum = |f: fn(&Stats) -> u64| st.iter().map(f).sum::<u64>();
    let ms = |v: u32| v as f64 / 1000.0;
    println!(
        "SUMMARY label={label} secs={secs} transit_n={} transit_p50={:.1} transit_p95={:.1} transit_p99={:.1} \
         m2e_n={} m2e_p50={:.1} m2e_p95={:.1} m2e_max={:.1} late_onsets={} spurts={} rtt_n={} rtt_p50={:.1} rtt_p95={:.1} \
         target_p50={} target_p95={} received={} lost={} fec={} dred={} concealed={} late={} dropped={} \
         tx_pkts={} tx_bytes={} rx_pkts={} rx_bytes={} udp_tx={} udp_rx={}",
        tr.len(),
        ms(pct(&mut tr, 0.5)),
        ms(pct(&mut tr, 0.95)),
        ms(pct(&mut tr, 0.99)),
        me.len(),
        ms(pct(&mut me, 0.5)),
        ms(pct(&mut me, 0.95)),
        ms(pct(&mut me, 1.0)),
        sh.late_onsets.load(Relaxed),
        sh.spurts.load(Relaxed),
        rt.len(),
        ms(pct(&mut rt, 0.5)),
        ms(pct(&mut rt, 0.95)),
        pct(&mut tg, 0.5),
        pct(&mut tg, 0.95),
        sum(|s| s.received),
        sum(|s| s.lost),
        sum(|s| s.recovered_fec),
        sum(|s| s.recovered_dred),
        sum(|s| s.concealed),
        sum(|s| s.late),
        sum(|s| s.dropped),
        sh.tx_pkts.load(Relaxed),
        sh.tx_bytes.load(Relaxed),
        sh.rx_pkts.load(Relaxed),
        sh.rx_bytes.load(Relaxed),
        sh.udp_tx.load(Relaxed),
        sh.udp_rx.load(Relaxed),
    );
}

#[tokio::main]
async fn main() {
    let a = Args::parse();
    let secs: u64 = a.num("secs", 60);
    let label = a.get("label").unwrap_or("run").to_string();
    let sh = Arc::new(Shared {
        start: Some(Instant::now()),
        trace: a.get("onset-trace").map(|path| {
            Mutex::new(BufWriter::new(
                File::create(path).expect("create onset trace"),
            ))
        }),
        levels: a.get("level-trace").map(|path| {
            Mutex::new(BufWriter::new(
                File::create(path).expect("create level trace"),
            ))
        }),
        features: a.get("feature-trace").map(|path| {
            Mutex::new(feature_trace::Recorder::new(
                path,
                a.num("feature-receiver", 1),
            ))
        }),
        sends: (0..MAX_SLOTS)
            .map(|_| Mutex::new(vec![(0, 0); RING]))
            .collect(),
        onsets: (0..MAX_SLOTS)
            .map(|_| Mutex::new(VecDeque::new()))
            .collect(),
        ..Default::default()
    });
    let mut tasks = Vec::new();
    if a.flag("direct") {
        assert!(
            a.get("talk-pattern").is_none_or(|p| p == "random"),
            "controlled patterns require group mode"
        );
        // GC-D09 baseline: a 1:1 call between two endpoints on this machine (loopback),
        // continuous `encode` + `Receiver::new()` exactly like today's 1:1 path.
        let port: u16 = a.num("port", 7710);
        let ep_a = endpoint(1, Some(port)).await;
        let ep_b = endpoint(2, None).await;
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let (ca, cb) = tokio::join!(
            async { ep_a.accept().await.unwrap().await.unwrap() },
            async {
                ep_b.connect(EndpointAddr::new(public(1)).with_ip_addr(addr), ALPN)
                    .await
                    .unwrap()
            }
        );
        for m in [
            new_member(1, Some(2), ca, &sh),
            new_member(2, Some(1), cb, &sh),
        ] {
            tasks.push(tokio::spawn(run_member(m, 0.5, secs, None)));
        }
        for t in tasks {
            t.await.unwrap();
        }
        summary(&sh, &label, secs);
        drop((ep_a, ep_b));
        return;
    }
    let host: SocketAddr = a
        .get("host")
        .expect("--host ip:port")
        .parse()
        .expect("host addr");
    let n: u8 = a.num("members", 2);
    let first: u8 = a.num("first-slot", 1);
    let participants: f32 = a.num("participants", n as f32 + 1.0);
    let duty = 1.0 / participants;
    let schedule = match a.get("talk-pattern").unwrap_or("random") {
        "random" => None,
        "turn-taking" => Some((talk_pattern::TalkPattern::TurnTaking, n)),
        "overlap" => Some((talk_pattern::TalkPattern::Overlap, n)),
        _ => panic!("talk-pattern must be random, turn-taking or overlap"),
    };
    assert!(
        n > 0 && first > 0 && (first as usize + n as usize) <= MAX_SLOTS,
        "invalid member slots"
    );
    assert!(
        schedule.is_none() || first == 1,
        "controlled patterns require first-slot=1"
    );
    let mut eps = Vec::new();
    for i in 0..n {
        let slot = first + i;
        let ep = endpoint(slot, None).await;
        let conn = ep
            .connect(EndpointAddr::new(public(0)).with_ip_addr(host), ALPN)
            .await
            .expect("connect");
        eps.push(ep);
        tasks.push(tokio::spawn(run_member(
            new_member(slot, None, conn, &sh),
            duty,
            secs,
            schedule,
        )));
    }
    eprintln!("{n} members connected to {host}");
    for t in tasks {
        t.await.unwrap();
    }
    summary(&sh, &label, secs);
    for ep in eps {
        ep.close().await;
    }
}
