//! Bench call host: accepts member connections on the bench ALPN and forwards each Opus
//! datagram, undecoded, to every other participant while its sender is in the selected set S
//! (top-K by the sender-reported level byte, 300 ms average, 6 dB / 200 ms hysteresis, plus
//! the G6-D2 provisional K+1 slot). Also runs the host's own participant (slot 0): one encode
//! and up to K(+1) decodes + mix per 20 ms, as a phone host that is in the call would.
//!
//!     host [--port 7700] [--k 2] [--no-provisional] [--no-participate] [--duty 0.2] [--secs 0]
//!
//! Prints `STAT ...` lines every 5 s (cumulative counters) for scripts/gcall_bench.py.
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

use audio::{FRAME, GroupFrame, Receiver, SPEECH_LEVEL, Sender};
use bytes::Bytes;
use gcall_bench::*;
use iroh::endpoint::{Connection, presets};
use iroh::Endpoint;
use parking_lot::Mutex;

const WINDOW_MS: u64 = 300;
const HYST_DB: f32 = 6.0;
const HYST_MS: u64 = 200;
const PROV_MS: u64 = 200;

struct Sel {
    k: usize,
    provisional: bool,
    hist: Vec<VecDeque<(u64, u8)>>,
    s: Vec<u8>,
    prov: Option<(u8, u64)>,
    prov_spent: [bool; MAX_SLOTS],
    cand_since: [Option<u64>; MAX_SLOTS],
    active_gen: u32,
    changes: u64,
}

impl Sel {
    fn note(&mut self, slot: u8, now: u64, level: u8) {
        self.hist[slot as usize].push_back((now, level));
    }

    fn forwards(&self, slot: u8) -> bool {
        self.s.contains(&slot) || self.prov.is_some_and(|(p, _)| p == slot)
    }

    fn active(&self) -> Vec<u8> {
        let mut v = self.s.clone();
        if let Some((p, _)) = self.prov {
            v.push(p);
        }
        v
    }

    /// Re-evaluates S; true if the forwarded set changed.
    fn update(&mut self, now: u64) -> bool {
        let before = self.active();
        let mut avg = [f32::INFINITY; MAX_SLOTS];
        for (i, h) in self.hist.iter_mut().enumerate() {
            while h.front().is_some_and(|&(t, _)| t + WINDOW_MS < now) {
                h.pop_front();
            }
            if !h.is_empty() {
                avg[i] = h.iter().map(|&(_, l)| l as f32).sum::<f32>() / h.len() as f32;
            }
        }
        let eligible = |i: u8| avg[i as usize] < SPEECH_LEVEL as f32;
        self.s.retain(|&i| eligible(i));
        for i in 0..MAX_SLOTS {
            if !eligible(i as u8) {
                self.prov_spent[i] = false;
                self.cand_since[i] = None;
            }
        }
        let mut cands: Vec<u8> =
            (0..MAX_SLOTS as u8).filter(|&i| eligible(i) && !self.s.contains(&i)).collect();
        cands.sort_by(|a, b| avg[*a as usize].total_cmp(&avg[*b as usize]));
        while self.s.len() < self.k && !cands.is_empty() {
            let c = cands.remove(0);
            self.s.push(c);
            if self.prov.is_some_and(|(p, _)| p == c) {
                self.prov = None;
            }
        }
        if self.s.len() == self.k && self.k > 0 {
            let w = *self.s.iter().max_by(|a, b| avg[**a as usize].total_cmp(&avg[**b as usize])).unwrap();
            for &c in &cands.clone() {
                if avg[c as usize] + HYST_DB <= avg[w as usize] {
                    let since = *self.cand_since[c as usize].get_or_insert(now);
                    if now - since >= HYST_MS {
                        self.s.retain(|&x| x != w);
                        self.s.push(c);
                        self.cand_since[c as usize] = None;
                        cands.retain(|&x| x != c);
                        if self.prov.is_some_and(|(p, _)| p == c) {
                            self.prov = None;
                        }
                        break;
                    }
                } else {
                    self.cand_since[c as usize] = None;
                }
            }
        }
        if self.provisional {
            if let Some((p, t0)) = self.prov
                && (!eligible(p) || now - t0 >= PROV_MS)
            {
                self.prov = None;
            }
            if self.prov.is_none() && self.s.len() == self.k {
                if let Some(&c) = cands.iter().find(|&&c| !self.prov_spent[c as usize]) {
                    self.prov = Some((c, now));
                    self.prov_spent[c as usize] = true;
                }
            }
        }
        let changed = before != self.active();
        if changed {
            self.active_gen += 1;
            self.changes += 1;
        }
        changed
    }
}

struct Local {
    voice: Voice,
    sender: Sender,
    gated: u32,
    rx: Vec<Option<Receiver>>,
}

#[derive(Default)]
struct Counters {
    rx_pkts: AtomicU64,
    rx_bytes: AtomicU64,
    tx_pkts: AtomicU64,
    tx_bytes: AtomicU64,
    tx_fail: AtomicU64,
    fwd_audio: AtomicU64,
}

struct Host {
    start: Instant,
    peers: Mutex<Vec<(u8, Connection)>>,
    sel: Mutex<Sel>,
    local: Option<Mutex<Local>>,
    c: Counters,
    /// UDP counters (rx, tx, rx_pkts, tx_pkts) of connections that have closed.
    closed_udp: Mutex<[u64; 4]>,
}

impl Host {
    fn now(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn send(&self, conn: &Connection, b: Bytes) {
        let n = b.len() as u64;
        if conn.send_datagram(b).is_ok() {
            self.c.tx_pkts.fetch_add(1, Relaxed);
            self.c.tx_bytes.fetch_add(n, Relaxed);
        } else {
            self.c.tx_fail.fetch_add(1, Relaxed);
        }
    }

    fn forward(&self, from: u8, audio: &[u8], now: u64) {
        if !self.sel.lock().forwards(from) {
            return;
        }
        self.c.fwd_audio.fetch_add(1, Relaxed);
        let mut v = Vec::with_capacity(audio.len() + 2);
        v.extend_from_slice(&[T_AUDIO_DOWN, from]);
        v.extend_from_slice(audio);
        let b = Bytes::from(v);
        let targets: Vec<Connection> =
            self.peers.lock().iter().filter(|(s, _)| *s != from).map(|(_, c)| c.clone()).collect();
        for c in &targets {
            self.send(c, b.clone());
        }
        if from != 0
            && let Some(l) = &self.local
        {
            let mut l = l.lock();
            l.rx[from as usize].get_or_insert_with(|| Receiver::new_group().unwrap()).push(audio, now);
        }
    }

    fn on_datagram(&self, from: u8, conn: &Connection, d: &[u8]) {
        self.c.rx_pkts.fetch_add(1, Relaxed);
        self.c.rx_bytes.fetch_add(d.len() as u64, Relaxed);
        let now = self.now();
        match d.first() {
            Some(&T_AUDIO_UP) if d.len() > 1 + audio::HEADER => {
                self.sel.lock().note(from, now, d[5]);
                self.forward(from, &d[1..], now);
            }
            Some(&T_LEVEL) if d.len() == 2 => self.sel.lock().note(from, now, d[1]),
            Some(&T_PING) => {
                let mut v = d.to_vec();
                v[0] = T_PONG;
                self.send(conn, Bytes::from(v));
            }
            _ => {}
        }
    }

    fn tick(&self) {
        let now = self.now();
        if let Some(l) = &self.local {
            let mut pcm = [0i16; FRAME];
            let out = {
                let mut l = l.lock();
                l.voice.next_frame(&mut pcm);
                match l.sender.encode_group(&pcm).unwrap() {
                    GroupFrame::Opus(d) => {
                        l.gated = 0;
                        Some(d)
                    }
                    GroupFrame::Gated { level } => {
                        if l.gated % LEVEL_EVERY == 0 {
                            self.sel.lock().note(0, now, level);
                        }
                        l.gated += 1;
                        None
                    }
                }
            };
            if let Some(d) = out {
                self.sel.lock().note(0, now, d[4]);
                self.forward(0, &d, now);
            }
        }
        let changed = {
            let mut s = self.sel.lock();
            s.update(now).then(|| (s.active_gen, s.active()))
        };
        if let Some((g, act)) = changed {
            let mut v = vec![T_ACTIVE];
            v.extend_from_slice(&g.to_be_bytes());
            v.extend_from_slice(&act);
            let b = Bytes::from(v);
            let targets: Vec<Connection> = self.peers.lock().iter().map(|(_, c)| c.clone()).collect();
            for c in &targets {
                self.send(c, b.clone());
            }
            if let Some(l) = &self.local {
                for (i, r) in l.lock().rx.iter_mut().enumerate() {
                    if let Some(r) = r {
                        r.set_active(act.contains(&(i as u8)));
                    }
                }
            }
        }
        if let Some(l) = &self.local {
            let mut l = l.lock();
            let mut frames: Vec<[i16; FRAME]> = Vec::new();
            for r in l.rx.iter_mut().flatten() {
                if r.is_idle() {
                    continue;
                }
                let mut o = [0i16; FRAME];
                r.pull(&mut o, now);
                frames.push(o);
            }
            let refs: Vec<&[i16]> = frames.iter().map(|f| &f[..]).collect();
            let mut mixed = [0i16; FRAME];
            audio::mix(&refs, &mut mixed);
            std::hint::black_box(&mixed);
        }
    }

    fn stat(&self) {
        let [mut urx, mut utx, mut urxp, mut utxp] = *self.closed_udp.lock();
        let peers = self.peers.lock();
        for (_, c) in peers.iter() {
            let s = c.stats();
            urx += s.udp_rx.bytes;
            utx += s.udp_tx.bytes;
            urxp += s.udp_rx.datagrams;
            utxp += s.udp_tx.datagrams;
        }
        let sel = self.sel.lock();
        println!(
            "STAT t_ms={} peers={} rx_pkts={} rx_bytes={} tx_pkts={} tx_bytes={} tx_fail={} fwd_audio={} \
             udp_rx={} udp_tx={} udp_rx_pkts={} udp_tx_pkts={} cpu_ticks={} sel_changes={}",
            self.now(),
            peers.len(),
            self.c.rx_pkts.load(Relaxed),
            self.c.rx_bytes.load(Relaxed),
            self.c.tx_pkts.load(Relaxed),
            self.c.tx_bytes.load(Relaxed),
            self.c.tx_fail.load(Relaxed),
            self.c.fwd_audio.load(Relaxed),
            urx,
            utx,
            urxp,
            utxp,
            cpu_ticks_self(),
            sel.changes,
        );
    }
}

#[tokio::main]
async fn main() {
    let a = Args::parse();
    let port: u16 = a.num("port", 7700);
    let k: usize = a.num("k", 2);
    let secs: u64 = a.num("secs", 0);
    let participate = !a.flag("no-participate");
    let duty: f32 = a.num("duty", 0.2);
    let ep = Endpoint::builder(presets::Minimal)
        .secret_key(secret(0))
        .alpns(vec![ALPN.to_vec()])
        .transport_config(transport())
        .bind_addr(format!("0.0.0.0:{port}").as_str())
        .expect("bind addr")
        .bind()
        .await
        .expect("bind");
    let local = participate.then(|| {
        Mutex::new(Local {
            voice: Voice::new(1000, duty),
            sender: Sender::new(32_000).unwrap(),
            gated: 0,
            rx: (0..MAX_SLOTS).map(|_| None).collect(),
        })
    });
    let host = Arc::new(Host {
        start: Instant::now(),
        peers: Mutex::new(Vec::new()),
        sel: Mutex::new(Sel {
            k,
            provisional: !a.flag("no-provisional"),
            hist: (0..MAX_SLOTS).map(|_| VecDeque::new()).collect(),
            s: Vec::new(),
            prov: None,
            prov_spent: [false; MAX_SLOTS],
            cand_since: [None; MAX_SLOTS],
            active_gen: 0,
            changes: 0,
        }),
        local,
        c: Counters::default(),
        closed_udp: Mutex::new([0; 4]),
    });
    println!("READY id={} port={port} k={k} participate={participate} pid={}", ep.id(), std::process::id());

    let h = host.clone();
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(Duration::from_millis(FRAME_MS));
        loop {
            iv.tick().await;
            h.tick();
        }
    });
    let h = host.clone();
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(Duration::from_secs(5));
        loop {
            iv.tick().await;
            h.stat();
        }
    });
    let h = host.clone();
    let accept = async move {
        while let Some(inc) = ep.accept().await {
            let h = h.clone();
            tokio::spawn(async move {
                let Ok(conn) = inc.await else { return };
                let id = conn.remote_id();
                let Some(slot) = (1..MAX_SLOTS as u8).find(|&s| public(s) == id) else {
                    conn.close(1u32.into(), b"unknown");
                    return;
                };
                {
                    let mut p = h.peers.lock();
                    p.retain(|(s, _)| *s != slot);
                    p.push((slot, conn.clone()));
                }
                eprintln!("joined slot {slot}");
                while let Ok(d) = conn.read_datagram().await {
                    h.on_datagram(slot, &conn, &d);
                }
                h.peers.lock().retain(|(s, c)| !(*s == slot && c.stable_id() == conn.stable_id()));
                let st = conn.stats();
                let mut cu = h.closed_udp.lock();
                cu[0] += st.udp_rx.bytes;
                cu[1] += st.udp_tx.bytes;
                cu[2] += st.udp_rx.datagrams;
                cu[3] += st.udp_tx.datagrams;
                eprintln!("left slot {slot}");
            });
        }
    };
    if secs > 0 {
        let _ = tokio::time::timeout(Duration::from_secs(secs), accept).await;
        host.stat();
    } else {
        accept.await;
    }
}
