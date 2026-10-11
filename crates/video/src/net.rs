//! iroh transport for [`crate::media`]: one QUIC uni stream per group, one control bi stream.
//!
//! [`VideoSend`] runs a [`Sender`]: each [`Out`] becomes `open_uni` (priority = group, so the
//! newest group goes first), writes, `finish` or `reset`. [`VideoRecv`] runs a [`Receiver`]
//! over the accepted uni streams and opens the control stream, on which it sends rate reports,
//! keyframe requests and acks.
//!
//! Backlog: QUIC accepts writes into a send buffer of up to `send_window` (10 MB by default),
//! so "not yet accepted by write" says nothing until seconds of video are queued. The receiver
//! therefore acks the bytes it has read per group, and the sender counts as unsent whatever was
//! written more than one RTT (+ slack) ago and is still unacked: that is video queued anywhere
//! (send buffer, bottleneck, or a receiver that stopped reading).
//!
//! Control stream (receiver -> sender), after a version byte: `1 [delivered_bps u32]
//! [queuing_us u64]` rate report, `2` keyframe request, `3 [group u64][offset u64]` ack (stream
//! bytes read, header included). All big-endian. The other direction is unused.
//!
//! The video layer owns `accept_uni` and (for the sender) the next `accept_bi` on the call's
//! connection.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use iroh::endpoint::{Connection, ConnectionError, ReadError, RecvStream, SendStream, VarInt};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::media::{Frame, Out, RateReport, Receiver, ReceiverConfig, Sender, SenderConfig};

pub const CONTROL_VERSION: u8 = 1;
/// Group ids above this are garbage (and would overflow the receiver's arithmetic).
pub const MAX_GROUP_ID: u64 = 1 << 48;
const TICK: Duration = Duration::from_millis(10);
const ACK_US: u64 = 10_000;
/// Acks can't come back sooner than one RTT plus this (ack interval, scheduling).
const ACK_SLACK_US: u64 = 40_000;
const READ_CHUNK: usize = 16 * 1024;
/// Reset / stop codes.
const CODE_STALE: u32 = 1;
const CODE_REJECTED: u32 = 2;

#[derive(Clone, Debug, thiserror::Error)]
pub enum NetError {
    #[error("connection: {0}")]
    Connection(#[from] ConnectionError),
    #[error("video stream closed")]
    Closed,
    #[error("protocol: {0}")]
    Protocol(&'static str),
}

/// Receiver -> sender control message.
#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    Report(RateReport),
    Keyframe,
    /// `offset` bytes of `group`'s stream have been read.
    Ack { group: u64, offset: u64 },
}

impl Control {
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Control::Report(r) => {
                out.push(1);
                out.extend_from_slice(&r.delivered_bps.to_be_bytes());
                out.extend_from_slice(&r.queuing_us.to_be_bytes());
            }
            Control::Keyframe => out.push(2),
            Control::Ack { group, offset } => {
                out.push(3);
                out.extend_from_slice(&group.to_be_bytes());
                out.extend_from_slice(&offset.to_be_bytes());
            }
        }
    }

    /// One message from the front of `buf` and its length; `None` if incomplete.
    pub fn decode(buf: &[u8]) -> Result<Option<(Control, usize)>, NetError> {
        let Some(&t) = buf.first() else { return Ok(None) };
        let len = match t {
            1 => 13,
            2 => 1,
            3 => 17,
            _ => return Err(NetError::Protocol("unknown control message")),
        };
        if buf.len() < len {
            return Ok(None);
        }
        let u64_at = |i: usize| u64::from_be_bytes(buf[i..i + 8].try_into().unwrap());
        let msg = match t {
            1 => Control::Report(RateReport {
                delivered_bps: u32::from_be_bytes(buf[1..5].try_into().unwrap()),
                queuing_us: u64_at(5),
            }),
            2 => Control::Keyframe,
            _ => Control::Ack { group: u64_at(1), offset: u64_at(9) },
        };
        Ok(Some((msg, len)))
    }
}

fn now_us(epoch: Instant) -> u64 {
    epoch.elapsed().as_micros() as u64
}

fn priority(group: u64) -> i32 {
    (group & 0x7fff_ffff) as i32
}

#[derive(Clone, Debug, Default)]
pub struct SendStats {
    pub frames: u64,
    pub bytes: u64,
    pub resets: u64,
    pub reports: u64,
    pub keyframe_requests: u64,
}

struct GroupTx {
    /// Dropped on finish.
    data: Option<mpsc::UnboundedSender<Bytes>>,
    reset: Option<oneshot::Sender<()>>,
    /// The stream task ended (all acked by QUIC, stopped by the peer, or failed).
    done: Arc<AtomicBool>,
    written: u64,
    /// (time, `written` after that write), back to the ack horizon.
    history: VecDeque<(u64, u64)>,
    acked: u64,
}

impl GroupTx {
    /// Bytes written at or before `t`; forgets history older than needed.
    fn written_by(&mut self, t: u64) -> u64 {
        while self.history.len() > 1 && self.history[1].0 <= t {
            self.history.pop_front();
        }
        match self.history.front() {
            Some(&(at, n)) if at <= t => n,
            _ => 0,
        }
    }
}

struct SendState {
    tx: Sender,
    conn: Connection,
    rt: tokio::runtime::Handle,
    epoch: Instant,
    groups: BTreeMap<u64, GroupTx>,
    closed: bool,
    stats: SendStats,
}

impl SendState {
    fn apply(&mut self, outs: Vec<Out>) {
        let now = now_us(self.epoch);
        for o in outs {
            match o {
                Out::Open { group } => {
                    let (data, data_rx) = mpsc::unbounded_channel();
                    let (reset, reset_rx) = oneshot::channel();
                    let done = Arc::new(AtomicBool::new(false));
                    self.rt.spawn(run_group(self.conn.clone(), group, data_rx, reset_rx, done.clone()));
                    let g = GroupTx {
                        data: Some(data),
                        reset: Some(reset),
                        done,
                        written: 0,
                        history: VecDeque::new(),
                        acked: 0,
                    };
                    self.groups.insert(group, g);
                }
                Out::Write { group, data } => {
                    let Some(g) = self.groups.get_mut(&group) else { continue };
                    let Some(tx) = &g.data else { continue };
                    g.written += data.len() as u64;
                    self.stats.bytes += data.len() as u64;
                    let _ = tx.send(Bytes::from(data));
                    g.history.push_back((now, g.written));
                }
                Out::Finish { group } => {
                    if let Some(g) = self.groups.get_mut(&group) {
                        g.data = None;
                    }
                }
                Out::Reset { group } => {
                    if let Some(mut g) = self.groups.remove(&group) {
                        if let Some(r) = g.reset.take() {
                            let _ = r.send(());
                        }
                        self.stats.resets += 1;
                    }
                }
            }
        }
    }

    fn rtt_us(&self) -> u64 {
        let paths = self.conn.paths();
        let rtt = paths.iter().find(|p| p.is_selected()).map(|p| p.rtt()).unwrap_or_default();
        rtt.as_micros() as u64
    }

    fn feedback(&mut self) {
        let now = now_us(self.epoch);
        let horizon = now.saturating_sub(self.rtt_us() + ACK_SLACK_US);
        let mut lost_current = false;
        self.groups.retain(|_, g| {
            let ended = g.done.load(Ordering::Relaxed);
            // An unfinished group whose stream died can take no more frames.
            lost_current |= ended && g.data.is_some();
            let delivered = g.data.is_none() && g.acked >= g.written;
            !ended && !delivered
        });
        if lost_current {
            self.tx.request_keyframe();
        }
        let unsent: BTreeMap<u64, usize> = self
            .groups
            .iter_mut()
            .map(|(k, g)| (*k, g.written_by(horizon).saturating_sub(g.acked) as usize))
            .collect();
        let outs = self.tx.feedback(&unsent);
        self.apply(outs);
    }

    fn on_control(&mut self, msg: Control) {
        match msg {
            Control::Report(r) => {
                self.stats.reports += 1;
                self.tx.on_report(now_us(self.epoch), &r);
            }
            Control::Keyframe => {
                self.stats.keyframe_requests += 1;
                self.tx.request_keyframe();
            }
            Control::Ack { group, offset } => {
                if let Some(g) = self.groups.get_mut(&group) {
                    g.acked = g.acked.max(offset.min(g.written));
                }
            }
        }
    }
}

async fn run_group(
    conn: Connection,
    group: u64,
    mut data: mpsc::UnboundedReceiver<Bytes>,
    mut reset: oneshot::Receiver<()>,
    done: Arc<AtomicBool>,
) {
    let reset_code = VarInt::from_u32(CODE_STALE);
    'run: {
        let mut s: SendStream = tokio::select! {
            biased;
            _ = &mut reset => break 'run,
            s = conn.open_uni() => match s {
                Ok(s) => s,
                Err(_) => break 'run,
            },
        };
        let _ = s.set_priority(priority(group));
        loop {
            tokio::select! {
                biased;
                r = &mut reset => {
                    if r.is_ok() {
                        let _ = s.reset(reset_code);
                    }
                    break 'run;
                }
                d = data.recv() => match d {
                    Some(b) => {
                        let mut off = 0;
                        while off < b.len() {
                            tokio::select! {
                                biased;
                                r = &mut reset => {
                                    if r.is_ok() {
                                        let _ = s.reset(reset_code);
                                    }
                                    break 'run;
                                }
                                w = s.write(&b[off..]) => match w {
                                    Ok(n) => off += n,
                                    Err(_) => break 'run,
                                },
                            }
                        }
                    }
                    None => {
                        let _ = s.finish();
                        // Keep the stream: the sender may still give up on it.
                        tokio::select! {
                            r = &mut reset => if r.is_ok() {
                                let _ = s.reset(reset_code);
                            },
                            _ = s.stopped() => {}
                        }
                        break 'run;
                    }
                },
            }
        }
    }
    done.store(true, Ordering::Relaxed);
}

/// Sending half of a call's video. Encoder loop: read [`Self::bitrate`] and
/// [`Self::wants_keyframe`], encode, [`Self::send`].
pub struct VideoSend {
    state: Arc<Mutex<SendState>>,
    driver: JoinHandle<()>,
}

impl VideoSend {
    /// Waits for the peer's [`VideoRecv`] to open the control stream.
    pub async fn accept(conn: Connection, cfg: SenderConfig) -> Result<Self, NetError> {
        let (mut ctrl_out, mut ctrl) = conn.accept_bi().await?;
        let mut v = [0u8; 1];
        ctrl.read_exact(&mut v).await.map_err(|_| NetError::Protocol("control stream"))?;
        if v[0] != CONTROL_VERSION {
            return Err(NetError::Protocol("control version"));
        }
        let _ = ctrl_out.finish();
        let state = Arc::new(Mutex::new(SendState {
            tx: Sender::new(cfg),
            conn,
            rt: tokio::runtime::Handle::current(),
            epoch: Instant::now(),
            groups: BTreeMap::new(),
            closed: false,
            stats: SendStats::default(),
        }));
        let driver = tokio::spawn(drive_send(state.clone(), ctrl));
        Ok(Self { state, driver })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SendState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// One encoded frame. Never blocks: a slow link shows up as resets and a lower bitrate.
    pub fn send(&self, capture_us: u64, key: bool, payload: &[u8]) -> Result<(), NetError> {
        let mut st = self.lock();
        if st.closed {
            return Err(NetError::Closed);
        }
        st.stats.frames += 1;
        let outs = st.tx.frame(capture_us, key, payload);
        st.apply(outs);
        Ok(())
    }

    pub fn bitrate(&self) -> u32 {
        self.lock().tx.bitrate()
    }

    pub fn wants_keyframe(&self) -> bool {
        self.lock().tx.wants_keyframe()
    }

    /// How long encoding one frame took, stamped on this sender's clock (drives the ladder's
    /// CPU cap).
    pub fn encode_time(&self, us: u64) {
        let mut st = self.lock();
        let now = now_us(st.epoch);
        st.tx.encode_time(now, us);
    }

    /// Direct access to the sans-IO sender (e.g. its counters).
    pub fn with_sender<R>(&self, f: impl FnOnce(&mut Sender) -> R) -> R {
        f(&mut self.lock().tx)
    }

    pub fn stats(&self) -> SendStats {
        self.lock().stats.clone()
    }

    /// The receiver went away (control stream ended).
    pub fn is_closed(&self) -> bool {
        self.lock().closed
    }
}

impl Drop for VideoSend {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

async fn drive_send(state: Arc<Mutex<SendState>>, mut ctrl: RecvStream) {
    let lock = || state.lock().unwrap_or_else(|e| e.into_inner());
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut buf = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        tokio::select! {
            _ = tick.tick() => lock().feedback(),
            r = ctrl.read(&mut chunk) => {
                let Ok(Some(n)) = r else { break };
                buf.extend_from_slice(&chunk[..n]);
                let mut st = lock();
                let mut used = 0;
                loop {
                    match Control::decode(&buf[used..]) {
                        Ok(Some((msg, len))) => {
                            used += len;
                            st.on_control(msg);
                        }
                        Ok(None) => break,
                        Err(_) => {
                            st.closed = true;
                            return;
                        }
                    }
                }
                buf.drain(..used);
            }
        }
    }
    lock().closed = true;
}

#[derive(Clone, Debug)]
pub struct RecvConfig {
    pub media: ReceiverConfig,
    /// Concurrent group streams the peer may open (QUIC stream limit; only new credit is
    /// limited, so also set it in the endpoint's transport config).
    pub max_streams: u32,
    /// Groups at or ahead of the one playing; more is a protocol error.
    pub max_groups: usize,
    /// Bytes buffered across those groups (frames of the playing group are kept until the
    /// next group starts); more is a protocol error.
    pub max_buffered: u64,
}

impl Default for RecvConfig {
    fn default() -> Self {
        Self { media: ReceiverConfig::default(), max_streams: 8, max_groups: 16, max_buffered: 32 << 20 }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RecvStats {
    pub frames: u64,
    pub bytes: u64,
    pub reports: u64,
    pub keyframe_requests: u64,
    /// Streams of older groups refused or stopped because a newer group is playing.
    pub stale_streams: u64,
    /// Streams refused for bad data (garbage header, oversized frame).
    pub rejected: u64,
    pub resets: u64,
}

enum Event {
    Open(u64, oneshot::Sender<()>),
    Data(u64, Bytes),
    End { group: u64, reset: bool },
    Refused { invalid: bool },
    Closed(ConnectionError),
}

struct RxGroup {
    stop: Option<oneshot::Sender<()>>,
    read: u64,
    acked: u64,
    dead: bool,
}

/// Receiving half of a call's video. Nothing is read from the network unless [`Self::recv`]
/// is being awaited, so a stalled decoder shows up at the sender as unacked data.
pub struct VideoRecv {
    rx: Receiver,
    cfg: RecvConfig,
    epoch: Instant,
    events: mpsc::Receiver<Event>,
    ctrl: mpsc::Sender<Vec<u8>>,
    groups: BTreeMap<u64, RxGroup>,
    floor: u64,
    last_ack_us: u64,
    key_wanted: bool,
    failed: Option<NetError>,
    tick: tokio::time::Interval,
    stats: RecvStats,
    tasks: Vec<JoinHandle<()>>,
    _ctrl_in: RecvStream,
}

impl VideoRecv {
    pub async fn new(conn: Connection, cfg: RecvConfig) -> Result<Self, NetError> {
        conn.set_max_concurrent_uni_streams(VarInt::from_u32(cfg.max_streams));
        let (mut out, ctrl_in) = conn.open_bi().await?;
        out.write_all(&[CONTROL_VERSION]).await.map_err(|_| NetError::Closed)?;
        let (ctrl, ctrl_rx) = mpsc::channel(64);
        let (ev_tx, events) = mpsc::channel(16);
        let tasks = vec![
            tokio::spawn(write_control(out, ctrl_rx)),
            tokio::spawn(accept_groups(conn, ev_tx)),
        ];
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Ok(Self {
            rx: Receiver::new(cfg.media.clone()),
            cfg,
            epoch: Instant::now(),
            events,
            ctrl,
            groups: BTreeMap::new(),
            floor: 0,
            last_ack_us: 0,
            key_wanted: false,
            failed: None,
            tick,
            stats: RecvStats::default(),
            tasks,
            _ctrl_in: ctrl_in,
        })
    }

    /// Next frame in decode order. Cancel-safe.
    pub async fn recv(&mut self) -> Result<Frame, NetError> {
        loop {
            if let Some(e) = &self.failed {
                return Err(e.clone());
            }
            let now = now_us(self.epoch);
            if let Some(f) = self.rx.poll(now) {
                self.advance(f.group);
                self.maintain(now);
                self.stats.frames += 1;
                return Ok(f);
            }
            self.maintain(now);
            tokio::select! {
                ev = self.events.recv() => match ev {
                    Some(ev) => self.on_event(ev),
                    None => self.failed = Some(NetError::Closed),
                },
                _ = self.tick.tick() => {}
            }
        }
    }

    /// Ask for a keyframe (e.g. the decoder failed).
    pub fn request_keyframe(&mut self) {
        self.key_wanted = true;
    }

    pub fn stats(&self) -> RecvStats {
        self.stats.clone()
    }

    fn send_ctrl(&mut self, msg: Control) {
        let mut b = Vec::with_capacity(17);
        msg.encode(&mut b);
        // Full means the sender isn't reading: acks/reports are periodic, nothing to keep.
        let _ = self.ctrl.try_send(b);
    }

    fn maintain(&mut self, now: u64) {
        if now.saturating_sub(self.last_ack_us) >= ACK_US {
            self.last_ack_us = now;
            let acks: Vec<_> = self
                .groups
                .iter_mut()
                .filter(|(_, g)| !g.dead && g.read > g.acked)
                .map(|(k, g)| {
                    g.acked = g.read;
                    Control::Ack { group: *k, offset: g.read }
                })
                .collect();
            for a in acks {
                self.send_ctrl(a);
            }
        }
        if let Some(r) = self.rx.report(now) {
            self.stats.reports += 1;
            self.send_ctrl(Control::Report(r));
        }
        if std::mem::take(&mut self.key_wanted) | self.rx.wants_keyframe(now) {
            self.stats.keyframe_requests += 1;
            self.send_ctrl(Control::Keyframe);
        }
    }

    /// Playing `group` now: stop everything older.
    fn advance(&mut self, group: u64) {
        if group <= self.floor {
            return;
        }
        self.floor = group;
        let keep = self.groups.split_off(&group);
        for (_, mut g) in std::mem::replace(&mut self.groups, keep) {
            if let Some(s) = g.stop.take() {
                let _ = s.send(());
                self.stats.stale_streams += 1;
            }
        }
    }

    fn reject(&mut self, group: u64) {
        if let Some(g) = self.groups.get_mut(&group) {
            g.dead = true;
            if let Some(s) = g.stop.take() {
                let _ = s.send(());
            }
        }
        self.rx.on_reset(group);
        self.stats.rejected += 1;
        self.key_wanted = true;
    }

    fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Open(group, stop) => {
                if group < self.floor {
                    let _ = stop.send(());
                    self.stats.stale_streams += 1;
                    return;
                }
                self.groups.insert(group, RxGroup { stop: Some(stop), read: 0, acked: 0, dead: false });
                if self.groups.len() > self.cfg.max_groups {
                    self.failed = Some(NetError::Protocol("too many open groups"));
                }
            }
            Event::Data(group, b) => {
                let Some(g) = self.groups.get_mut(&group) else { return };
                if g.dead {
                    return;
                }
                g.read += b.len() as u64;
                self.stats.bytes += b.len() as u64;
                if self.groups.values().map(|g| g.read).sum::<u64>() > self.cfg.max_buffered {
                    self.failed = Some(NetError::Protocol("too much buffered video"));
                    return;
                }
                if self.rx.on_data(now_us(self.epoch), group, &b).is_err() {
                    self.reject(group);
                }
            }
            Event::End { group, reset } => {
                if let Some(g) = self.groups.get_mut(&group) {
                    g.stop = None;
                    if g.dead {
                        return;
                    }
                }
                if reset {
                    self.stats.resets += 1;
                    self.rx.on_reset(group);
                } else {
                    self.rx.on_finish(group);
                }
            }
            Event::Refused { invalid } => {
                if invalid {
                    self.stats.rejected += 1;
                } else {
                    self.stats.stale_streams += 1;
                }
            }
            Event::Closed(e) => self.failed = Some(NetError::Connection(e)),
        }
    }
}

impl Drop for VideoRecv {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

async fn write_control(mut out: SendStream, mut msgs: mpsc::Receiver<Vec<u8>>) {
    while let Some(m) = msgs.recv().await {
        if out.write_all(&m).await.is_err() {
            return;
        }
    }
    let _ = out.finish();
}

async fn accept_groups(conn: Connection, events: mpsc::Sender<Event>) {
    let highest = Arc::new(Mutex::new(None::<u64>));
    loop {
        match conn.accept_uni().await {
            Ok(s) => {
                tokio::spawn(read_group(s, events.clone(), highest.clone()));
            }
            Err(e) => {
                let _ = events.send(Event::Closed(e)).await;
                return;
            }
        }
    }
}

async fn read_group(mut s: RecvStream, events: mpsc::Sender<Event>, highest: Arc<Mutex<Option<u64>>>) {
    let mut hdr = [0u8; 8];
    if s.read_exact(&mut hdr).await.is_err() {
        return;
    }
    let group = u64::from_be_bytes(hdr);
    let refused = {
        let mut h = highest.lock().unwrap_or_else(|e| e.into_inner());
        if group >= MAX_GROUP_ID {
            Some(true)
        } else if h.is_some_and(|h| group <= h) {
            // Older than a group we already have (or a duplicate): never needed.
            Some(false)
        } else {
            *h = Some(group);
            None
        }
    };
    if let Some(invalid) = refused {
        let _ = s.stop(VarInt::from_u32(if invalid { CODE_REJECTED } else { CODE_STALE }));
        let _ = events.send(Event::Refused { invalid }).await;
        return;
    }
    let (stop_tx, mut stop) = oneshot::channel();
    if events.send(Event::Open(group, stop_tx)).await.is_err()
        || events.send(Event::Data(group, Bytes::copy_from_slice(&hdr))).await.is_err()
    {
        return;
    }
    loop {
        let ev = tokio::select! {
            biased;
            _ = &mut stop => {
                let _ = s.stop(VarInt::from_u32(CODE_STALE));
                return;
            }
            r = s.read_chunk(READ_CHUNK) => match r {
                Ok(Some(b)) => Event::Data(group, b),
                Ok(None) => Event::End { group, reset: false },
                Err(ReadError::Reset(_)) => Event::End { group, reset: true },
                Err(_) => return,
            },
        };
        let end = matches!(ev, Event::End { .. });
        if events.send(ev).await.is_err() || end {
            return;
        }
    }
}

#[cfg(test)]
mod impair;
#[cfg(test)]
mod scenarios;
#[cfg(test)]
mod tests;
