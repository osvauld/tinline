//! Userspace UDP link emulator for tests (netem without root), with the same knobs as
//! [`crate::sim::Link`]: bottleneck rate with a drop-tail byte queue, delay + jitter, random
//! loss, outages and scheduled rate changes, seeded.
//!
//! Endpoint A is bound to `[::1]` only and B to `127.0.0.1` only; the proxy listens on one
//! socket per family and forwards between them. iroh's hole punching (advertised local
//! addresses) can't bypass it: neither endpoint has a socket of the other's family.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

use crate::sim::Rng;

/// IPv4 + UDP header, counted against the rate like on a real link.
const HEADER: usize = 28;

/// One direction of the path. Times are microseconds since [`Proxy::start_clock`].
#[derive(Clone, Debug)]
pub struct Link {
    /// 0 = unlimited (no queue either).
    pub rate_bps: u64,
    pub delay_us: u64,
    /// Extra delay, uniform in [0, jitter).
    pub jitter_us: u64,
    /// Let jitter reorder packets; otherwise a packet never overtakes the one before it.
    pub reorder: bool,
    pub loss: f64,
    pub queue_bytes: usize,
    pub rate_changes: Vec<(u64, u64)>,
    /// `(from, to)`: everything arriving at the proxy in this window is dropped.
    pub outages: Vec<(u64, u64)>,
}

impl Link {
    /// Delay only (the uncongested return path).
    pub fn delay(delay_us: u64, jitter_us: u64) -> Self {
        Self {
            rate_bps: 0,
            delay_us,
            jitter_us,
            reorder: false,
            loss: 0.0,
            queue_bytes: 0,
            rate_changes: vec![],
            outages: vec![],
        }
    }

    /// The forward direction of a [`crate::sim::Link`].
    pub fn from_sim(l: &crate::sim::Link) -> Self {
        Self {
            rate_bps: l.rate_bps,
            delay_us: l.delay_us,
            jitter_us: l.jitter_us,
            reorder: false,
            loss: l.loss,
            queue_bytes: l.queue_bytes,
            rate_changes: l.rate_changes.clone(),
            outages: l.outages.clone(),
        }
    }
}

#[derive(Debug, Default)]
pub struct DirStats {
    pub in_pkts: AtomicU64,
    pub in_bytes: AtomicU64,
    pub out_pkts: AtomicU64,
    pub out_bytes: AtomicU64,
    pub drop_queue: AtomicU64,
    pub drop_loss: AtomicU64,
    pub drop_outage: AtomicU64,
    /// Datagrams from an unexpected source (none expected).
    pub stray: AtomicU64,
}

impl DirStats {
    pub fn get(c: &AtomicU64) -> u64 {
        c.load(Ordering::Relaxed)
    }
}

impl std::fmt::Display for DirStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let g = Self::get;
        write!(
            f,
            "in {} pkts / {} B, out {} pkts / {} B, dropped queue {} loss {} outage {}, stray {}",
            g(&self.in_pkts),
            g(&self.in_bytes),
            g(&self.out_pkts),
            g(&self.out_bytes),
            g(&self.drop_queue),
            g(&self.drop_loss),
            g(&self.drop_outage),
            g(&self.stray)
        )
    }
}

enum Verdict {
    Deliver(u64),
    Queue,
    Loss,
    Outage,
}

struct Shaper {
    cfg: Link,
    rng: Rng,
    /// When the bottleneck finishes sending what is queued.
    link_free: u64,
    last_due: u64,
}

impl Shaper {
    fn rate(&self, now: u64) -> u64 {
        self.cfg.rate_changes.iter().filter(|c| c.0 <= now).map(|c| c.1).next_back().unwrap_or(self.cfg.rate_bps)
    }

    fn on_packet(&mut self, now: u64, len: usize) -> Verdict {
        if self.cfg.outages.iter().any(|&(a, b)| now >= a && now < b) {
            return Verdict::Outage;
        }
        let size = (len + HEADER) as u64;
        let rate = self.rate(now);
        let depart = if rate == 0 {
            now
        } else {
            let queued = self.link_free.saturating_sub(now) * rate / 8 / 1_000_000;
            if queued + size > self.cfg.queue_bytes as u64 {
                return Verdict::Queue;
            }
            self.link_free = self.link_free.max(now) + size * 8 * 1_000_000 / rate;
            self.link_free
        };
        // Lost on the wire, after using the bottleneck.
        if self.cfg.loss > 0.0 && self.rng.f64() < self.cfg.loss {
            return Verdict::Loss;
        }
        let jitter = if self.cfg.jitter_us > 0 { self.rng.next_u64() % self.cfg.jitter_us } else { 0 };
        let mut due = depart + self.cfg.delay_us + jitter;
        if !self.cfg.reorder {
            due = due.max(self.last_due);
        }
        self.last_due = self.last_due.max(due);
        Verdict::Deliver(due)
    }
}

pub struct Proxy {
    /// Where B sends (B's only peer address).
    pub b_side: SocketAddr,
    /// Where A sees B's packets come from.
    pub a_side: SocketAddr,
    pub a_to_b: Arc<DirStats>,
    pub b_to_a: Arc<DirStats>,
    epoch: Arc<Mutex<Instant>>,
    tasks: Vec<JoinHandle<()>>,
}

impl Proxy {
    /// Forwards between `a` (an IPv6 loopback socket address) and whoever talks to
    /// [`Self::b_side`] first.
    pub async fn new(a: SocketAddr, a_to_b: Link, b_to_a: Link, seed: u64) -> std::io::Result<Self> {
        let sock_a = Arc::new(UdpSocket::bind("[::1]:0").await?);
        let sock_b = Arc::new(UdpSocket::bind("127.0.0.1:0").await?);
        let epoch = Arc::new(Mutex::new(Instant::now()));
        let b_addr = Arc::new(Mutex::new(None::<SocketAddr>));
        let stats = (Arc::new(DirStats::default()), Arc::new(DirStats::default()));
        let shaper = |cfg: Link, seed: u64| Shaper { cfg, rng: Rng::new(seed), link_free: 0, last_due: 0 };
        let tasks = vec![
            tokio::spawn(forward(
                sock_a.clone(),
                sock_b.clone(),
                Peer::Fixed(a),
                Peer::Learned(b_addr.clone()),
                shaper(a_to_b, seed),
                epoch.clone(),
                stats.0.clone(),
            )),
            tokio::spawn(forward(
                sock_b.clone(),
                sock_a.clone(),
                Peer::Learn(b_addr),
                Peer::Fixed(a),
                shaper(b_to_a, seed ^ 0x9e37_79b9_7f4a_7c15),
                epoch.clone(),
                stats.1.clone(),
            )),
        ];
        Ok(Self {
            b_side: sock_b.local_addr()?,
            a_side: sock_a.local_addr()?,
            a_to_b: stats.0,
            b_to_a: stats.1,
            epoch,
            tasks,
        })
    }

    /// Schedules (rate changes, outages) count from now.
    pub fn start_clock(&self) {
        *self.epoch.lock().unwrap() = Instant::now();
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

enum Peer {
    Fixed(SocketAddr),
    /// Recorded from the first datagram (B's side).
    Learn(Arc<Mutex<Option<SocketAddr>>>),
    /// Whatever `Learn` recorded.
    Learned(Arc<Mutex<Option<SocketAddr>>>),
}

async fn forward(
    rx: Arc<UdpSocket>,
    tx: Arc<UdpSocket>,
    from: Peer,
    to: Peer,
    mut shaper: Shaper,
    epoch: Arc<Mutex<Instant>>,
    st: Arc<DirStats>,
) {
    let add = |c: &AtomicU64, n: u64| c.fetch_add(n, Ordering::Relaxed);
    let mut queue: BTreeMap<(u64, u64), Vec<u8>> = BTreeMap::new();
    let mut seq = 0u64;
    let mut buf = vec![0u8; 65536];
    let mut last_epoch = *epoch.lock().unwrap();
    loop {
        let e = *epoch.lock().unwrap();
        if e != last_epoch {
            // Clock restarted: rebase what is scheduled (times only grow, so shift by the gap).
            let shift = e.duration_since(last_epoch).as_micros() as u64;
            queue = queue.into_iter().map(|((t, s), p)| ((t.saturating_sub(shift), s), p)).collect();
            shaper.link_free = shaper.link_free.saturating_sub(shift);
            shaper.last_due = shaper.last_due.saturating_sub(shift);
            last_epoch = e;
        }
        let next = queue.first_key_value().map(|(k, _)| e + Duration::from_micros(k.0));
        tokio::select! {
            r = rx.recv_from(&mut buf) => {
                let Ok((n, src)) = r else { return };
                match &from {
                    Peer::Fixed(a) if *a != src => {
                        add(&st.stray, 1);
                        continue;
                    }
                    Peer::Learn(slot) => {
                        let mut s = slot.lock().unwrap();
                        if s.is_some_and(|b| b != src) {
                            add(&st.stray, 1);
                            continue;
                        }
                        *s = Some(src);
                    }
                    _ => {}
                }
                add(&st.in_pkts, 1);
                add(&st.in_bytes, n as u64);
                let now = e.elapsed().as_micros() as u64;
                match shaper.on_packet(now, n) {
                    Verdict::Deliver(due) => {
                        seq += 1;
                        queue.insert((due, seq), buf[..n].to_vec());
                    }
                    Verdict::Queue => _ = add(&st.drop_queue, 1),
                    Verdict::Loss => _ = add(&st.drop_loss, 1),
                    Verdict::Outage => _ = add(&st.drop_outage, 1),
                }
            }
            _ = sleep_until(next) => {
                let now = e.elapsed().as_micros() as u64;
                let dest = match &to {
                    Peer::Fixed(a) => Some(*a),
                    Peer::Learned(slot) | Peer::Learn(slot) => *slot.lock().unwrap(),
                };
                while let Some(entry) = queue.first_entry() {
                    if entry.key().0 > now {
                        break;
                    }
                    let p = entry.remove();
                    if let Some(d) = dest {
                        if tx.send_to(&p, d).await.is_ok() {
                            add(&st.out_pkts, 1);
                            add(&st.out_bytes, p.len() as u64);
                        }
                    }
                }
            }
        }
    }
}

async fn sleep_until(t: Option<Instant>) {
    match t {
        Some(t) => tokio::time::sleep_until(t.into()).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaper_queues_drops_and_delays() {
        let cfg = Link { rate_bps: 1_000_000, queue_bytes: 10_000, ..Link::delay(20_000, 0) };
        let mut s = Shaper { cfg, rng: Rng::new(1), link_free: 0, last_due: 0 };
        // 972 + 28 = 1000 B = 8 ms at 1 Mbps.
        let due: Vec<_> = (0..12)
            .map(|_| match s.on_packet(0, 972) {
                Verdict::Deliver(d) => Some(d),
                _ => None,
            })
            .collect();
        assert_eq!(due[0], Some(28_000));
        assert_eq!(due[9], Some(100_000));
        assert!(due[10].is_none(), "11th packet overflows the 10 kB queue");
        // Once drained, accepted again.
        assert!(matches!(s.on_packet(80_000, 972), Verdict::Deliver(_)));
    }

    #[test]
    fn shaper_rate_change_and_outage() {
        let cfg = Link {
            rate_bps: 1_000_000,
            queue_bytes: 100_000,
            rate_changes: vec![(1_000_000, 100_000)],
            outages: vec![(2_000_000, 2_500_000)],
            ..Link::delay(0, 0)
        };
        let mut s = Shaper { cfg, rng: Rng::new(1), link_free: 0, last_due: 0 };
        assert!(matches!(s.on_packet(0, 972), Verdict::Deliver(8_000)));
        assert!(matches!(s.on_packet(1_000_000, 972), Verdict::Deliver(1_080_000)));
        assert!(matches!(s.on_packet(2_100_000, 972), Verdict::Outage));
        assert!(matches!(s.on_packet(2_500_000, 972), Verdict::Deliver(2_580_000)));
    }

    #[test]
    fn jitter_keeps_order_unless_allowed() {
        for reorder in [false, true] {
            let cfg = Link { reorder, ..Link::delay(10_000, 20_000) };
            let mut s = Shaper { cfg, rng: Rng::new(3), link_free: 0, last_due: 0 };
            let due: Vec<u64> = (0..200)
                .map(|i| match s.on_packet(i * 1000, 100) {
                    Verdict::Deliver(d) => d,
                    _ => unreachable!(),
                })
                .collect();
            let sorted = due.windows(2).all(|w| w[0] <= w[1]);
            assert_eq!(sorted, !reorder);
        }
    }

    #[test]
    fn loss_rate_is_close() {
        let cfg = Link { loss: 0.03, ..Link::delay(0, 0) };
        let mut s = Shaper { cfg, rng: Rng::new(9), link_free: 0, last_due: 0 };
        let lost = (0..100_000).filter(|_| matches!(s.on_packet(0, 100), Verdict::Loss)).count();
        assert!((2_500..3_500).contains(&lost), "{lost}");
    }
}
