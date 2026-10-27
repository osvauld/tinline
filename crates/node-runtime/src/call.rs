//! The call object and its state machine: legs, state changes, media.

use super::*;

pub enum Cmd {
    Answer,
    Decline,
    Hangup,
    /// We blocked the peer: tell them with the notice and end the call (`"blocked"`).
    Block(proto::SignedBlockNotice),
}

pub struct Call {
    pub info: CallInfo,
    pub conn: Mutex<Option<Connection>>,
    pub state: Mutex<CallState>,
    pub cmd: mpsc::UnboundedSender<Cmd>,
    pub started: Instant,
    pub sender: Mutex<audio::Sender>,
    pub receiver: Mutex<audio::Receiver>,
    pub mic: Mutex<Vec<i16>>,
    /// The last second played, for `rx_freq_hz`.
    pub played: Mutex<VecDeque<i16>>,
    pub sent: Mutex<u64>,
    pub tone: Mutex<Option<(f32, audio::Tone)>>,
    /// Set once by `end_call`; later calls are no-ops, so every path may call it.
    pub ended: Mutex<bool>,
    /// Held while a state change is made and delivered, so one call's events reach the UI in
    /// the order they happened. Reentrant: a callback may start the next call.
    pub notify: ReentrantMutex<()>,
    /// When media last arrived (or the call went active); feeds the no-audio watchdog.
    pub last_rx: Mutex<Instant>,
    /// Unix seconds when the call began, for the history.
    pub started_at: u64,
    /// When it went active, for the duration.
    pub active_at: Mutex<Option<Instant>>,
    /// Whether the selected path was direct when last looked at (the connection is gone by the
    /// time the call has ended).
    pub direct: Mutex<bool>,
    /// An incoming call that arrived during another call and sits in `Live::waiting`, not
    /// yet in the slot. Declining it tells the caller `Busy`.
    pub waiting: AtomicBool,
    /// "End & answer" was chosen: answer this call the moment it takes over the slot.
    pub answer_on_promote: AtomicBool,
    /// The session (see `Shared::epoch`) it belongs to: a call that outlives a switch of account
    /// must not write into the next one.
    pub epoch: u64,
    /// Woken when the call ends from outside its own task (glare, lock), so an outgoing call
    /// that is still dialling several devices stops at once.
    pub ended_wake: tokio::sync::Notify,
    pub video: call_video::CallVideo,
    /// An outgoing call still `Dialing` whose callee phone the relay took a wake for: the UI
    /// says "Waking their phone…" (`CallStats::waking`).
    pub waking: AtomicBool,
}

/// Ends the call when dropped unless it already ended: covers every early return (and panic)
/// between taking the slot and `run_call` finishing it.
pub struct SlotGuard<'a> {
    pub inner: &'a NodeCtx,
    pub call: Arc<Call>,
}

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        self.inner.end_call(&self.call, "connection_lost".into());
    }
}

impl Call {
    pub fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    pub fn state(&self) -> CallState {
        self.state.lock().clone()
    }
}

/// What one dialled device of the callee reports to `run_outgoing`.
#[allow(clippy::large_enum_variant)]
pub enum LegEvent {
    /// Connected and hello sent: the device is being rung (or about to be).
    Connected,
    Ringing,
    /// The device answered; the connection and control stream are handed over.
    Accepted {
        renewed_grant: Option<proto::SignedGrant>,
        devices: Option<proto::SignedDeviceList>,
        video: Option<proto::VideoOffer>,
        conn: Connection,
        ctrl: Box<Ctrl>,
    },
    Declined(String),
    Busy,
    /// It rang and the callee's ring timed out (it sent `Hangup`).
    GaveUp,
    /// The device answered with a notice (they block us, if it verifies).
    Blocked(proto::SignedBlockNotice),
    /// `Reject { CALLS_NOT_ALLOWED }`: they let us message them but not call (P6).
    CallsNotAllowed,
    /// Could not connect, refused (`Reject`: not a contact there, blocked, unavailable), or the
    /// connection died.
    Failed(String),
}

/// One connection of a fanned-out outgoing call: connects, sends the hello, relays what the
/// device answers, and on a message from the coordinator sends it (a `Cancel`) and closes.
pub struct Leg {
    /// For a call wake when the callee device cannot be reached (`push.rs`).
    pub ctx: Arc<NodeCtx>,
    /// The call this leg dials for (its `waking` flag).
    pub call: Arc<Call>,
    /// The callee's DID.
    pub did: String,
    pub ep: Endpoint,
    pub device: ContactDevice,
    pub hello: Msg,
    pub idx: usize,
    pub events: mpsc::UnboundedSender<(usize, LegEvent)>,
    pub cmds: mpsc::UnboundedReceiver<Msg>,
    pub name: String,
}

impl Leg {
    pub async fn run(mut self) {
        let idx = self.idx;
        let fail = |events: &mpsc::UnboundedSender<(usize, LegEvent)>, why: String| {
            let _ = events.send((idx, LegEvent::Failed(why)));
        };
        let addr = match addr_for(&self.device.device, self.device.relay.as_deref()) {
            Ok(a) => a,
            Err(e) => return fail(&self.events, e.to_string()),
        };
        tracing::debug!("dialing {}", crate::short(&addr.id.to_string()));
        let conn = match connect_leg(&self.ctx, &self.call, &self.ep, addr.clone(), &self.did, self.device.device, &mut self.cmds).await {
            Ok(c) => c,
            Err(NotConnected::Failed(why)) => return fail(&self.events, why),
            // Told to stop before we even connected: nothing was sent, nothing to say.
            Err(NotConnected::Told(None)) => return,
            // ...but the device was woken and rings from the wake alone: tell it.
            Err(NotConnected::Told(Some(msg))) => return self.deliver_late(addr, msg).await,
        };
        let (send, recv) = match conn.open_bi().await {
            Ok(s) => s,
            Err(e) => return fail(&self.events, e.to_string()),
        };
        let mut ctrl = Ctrl::new(send, recv);
        if let Err(e) = ctrl.send(&self.hello).await {
            conn.close(0u32.into(), b"bye");
            return fail(&self.events, e.to_string());
        }
        let _ = self.events.send((idx, LegEvent::Connected));
        loop {
            tokio::select! {
                msg = ctrl.recv() => {
                    let ev = match msg {
                        Ok(Some(Msg::Ringing)) => {
                            let _ = self.events.send((idx, LegEvent::Ringing));
                            continue;
                        }
                        Ok(Some(Msg::Accept { renewed_grant, devices, video })) => {
                            let keep = conn.clone();
                            let ev = LegEvent::Accepted { renewed_grant, devices, video, conn, ctrl: Box::new(ctrl) };
                            if self.events.send((idx, ev)).is_err() {
                                keep.close(0u32.into(), b"bye");
                            }
                            return;
                        }
                        Ok(Some(Msg::Decline { reason })) => LegEvent::Declined(reason),
                        Ok(Some(Msg::Busy)) => LegEvent::Busy,
                        Ok(Some(Msg::Hangup)) => LegEvent::GaveUp,
                        Ok(Some(Msg::Notice { notice })) => LegEvent::Blocked(notice),
                        Ok(Some(Msg::Reject { reason })) if reason == crate::caps::CALLS_NOT_ALLOWED => LegEvent::CallsNotAllowed,
                        Ok(Some(Msg::Reject { reason })) => LegEvent::Failed(format!("rejected: {}", clip_reason(&reason))),
                        Ok(Some(other)) => {
                            tracing::debug!("ignoring {} before the answer", msg_name(&other));
                            continue;
                        }
                        Ok(None) => LegEvent::Failed("closed".into()),
                        Err(e) => LegEvent::Failed(e.to_string()),
                    };
                    let _ = self.events.send((idx, ev));
                    conn.close(0u32.into(), b"bye");
                    return;
                }
                cmd = self.cmds.recv() => {
                    if let Some(msg) = cmd {
                        let _ = ctrl.send(&msg).await;
                    }
                    ctrl.finish();
                    // Let the frame leave; the callee closes first when it can.
                    let _ = tokio::time::timeout(Duration::from_millis(300), conn.closed()).await;
                    conn.close(0u32.into(), b"bye");
                    return;
                }
                _ = conn.closed() => {
                    return fail(&self.events, "connection lost".into());
                }
            }
        }
    }
}

/// Why [`connect_leg`] returned without a connection.
enum NotConnected {
    Failed(String),
    /// Told to stop; with the message to deliver anyway when the device was already woken.
    Told(Option<Msg>),
}

impl Leg {
    /// We stopped (hung up, or another device answered) after waking this device, which rings
    /// from the wake alone (`push/ring.rs`) until a call arrives or its window ends. Keeps dialling
    /// it for up to [`crate::push::CALL_LATE_CANCEL_MS`] and sends the hello and then `msg`: the
    /// callee takes the call over its wake ring and ends it at once (a missed call).
    async fn deliver_late(&self, addr: iroh::EndpointAddr, msg: Msg) {
        use crate::push::{CALL_ATTEMPT_MS, CALL_LATE_CANCEL_MS, CALL_REDIAL_MS};
        let until = tokio::time::Instant::now() + Duration::from_millis(CALL_LATE_CANCEL_MS);
        loop {
            let attempt = tokio::time::Instant::now();
            if attempt >= until || self.ctx.shared.lock().epoch != self.call.epoch {
                tracing::info!("call: the end did not reach a woken device of {}", crate::short(&self.did));
                return;
            }
            let limit = Duration::from_millis(CALL_ATTEMPT_MS).min(until - attempt);
            if let Ok(Ok(conn)) = tokio::time::timeout(limit, crate::version::connect(&self.ep, addr.clone(), &crate::version::CALL)).await {
                if let Ok((send, recv)) = conn.open_bi().await {
                    let mut ctrl = Ctrl::new(send, recv);
                    if ctrl.send(&self.hello).await.is_ok() && ctrl.send(&msg).await.is_ok() {
                        ctrl.finish();
                        let _ = tokio::time::timeout(Duration::from_millis(300), conn.closed()).await;
                        conn.close(0u32.into(), b"bye");
                        tracing::info!("call: the end reached a woken device of {}", crate::short(&self.did));
                        return;
                    }
                }
                conn.close(0u32.into(), b"bye");
            }
            tokio::time::sleep_until(attempt + Duration::from_millis(CALL_REDIAL_MS)).await;
        }
    }
}

/// Dials one callee device. With a call token for it (`docs/push.md` "Calls") a first dial still
/// running after [`crate::push::CALL_EARLY_WAKE_MS`] sends a `call` wake; if it fails (after
/// [`crate::push::CALL_FIRST_DIAL_MS`]) the wake is sent again (throttled) and the device is redialled
/// every [`crate::push::CALL_REDIAL_MS`] for [`crate::push::CALL_WAKE_DIAL_MS`], while the call
/// stays `Dialing`; the wake crate's 10 s throttle allows a repeat wake meanwhile. Without a token
/// it is one dial of [`DIAL_TIMEOUT`], as before. `Told`: told to stop (the message is kept when
/// a wake went out); `Failed(why)`: could not connect.
async fn connect_leg(
    ctx: &Arc<NodeCtx>,
    call: &Arc<Call>,
    ep: &Endpoint,
    addr: iroh::EndpointAddr,
    did: &str,
    device: [u8; 32],
    cmds: &mut mpsc::UnboundedReceiver<Msg>,
) -> Result<Connection, NotConnected> {
    use crate::push::{CALL_ATTEMPT_MS, CALL_EARLY_WAKE_MS, CALL_FIRST_DIAL_MS, CALL_REDIAL_MS, CALL_WAKE_DIAL_MS};
    let wakeable = ctx.can_wake(did, &device, wake::Kind::Call);
    let first = if wakeable { Duration::from_millis(CALL_FIRST_DIAL_MS) } else { DIAL_TIMEOUT };
    let dial = |limit: Duration, addr: iroh::EndpointAddr| {
        let ep = ep.clone();
        async move { tokio::time::timeout(limit, crate::version::connect(&ep, addr, &crate::version::CALL)).await }
    };
    let wake = |ctx: Arc<NodeCtx>, call: Arc<Call>, did: String| async move {
        if ctx.wake_device(&did, device, wake::Kind::Call).await {
            call.waking.store(true, Ordering::Relaxed);
        }
    };
    let first_dial = dial(first, addr.clone());
    tokio::pin!(first_dial);
    let early = tokio::time::sleep(Duration::from_millis(CALL_EARLY_WAKE_MS));
    tokio::pin!(early);
    let mut early_sent = !wakeable;
    let first_try = loop {
        tokio::select! {
            c = &mut first_dial => break c,
            // In its own task: the dial keeps going while the relay answers.
            _ = &mut early, if !early_sent => {
                early_sent = true;
                tokio::spawn(wake(ctx.clone(), call.clone(), did.to_string()));
            }
            m = cmds.recv() => return Err(NotConnected::Told(m.filter(|_| wakeable && early_sent))),
        }
    };
    let mut why = match first_try {
        Ok(Ok(c)) => return Ok(c),
        Ok(Err(e)) => e,
        Err(_) => Error::Timeout,
    };
    if !wakeable || !matches!(why, Error::Net(_) | Error::Timeout) {
        return Err(NotConnected::Failed(why.to_string()));
    }
    let until = tokio::time::Instant::now() + Duration::from_millis(CALL_WAKE_DIAL_MS);
    loop {
        // The wake may take a moment (the relay); a hang-up does not wait for it.
        tokio::select! {
            _ = wake(ctx.clone(), call.clone(), did.to_string()) => {}
            m = cmds.recv() => return Err(NotConnected::Told(m)),
        }
        let attempt = tokio::time::Instant::now();
        if attempt >= until {
            return Err(NotConnected::Failed(why.to_string()));
        }
        let limit = Duration::from_millis(CALL_ATTEMPT_MS).min(until - attempt);
        let tried = tokio::select! {
            c = dial(limit, addr.clone()) => c,
            m = cmds.recv() => return Err(NotConnected::Told(m)),
        };
        match tried {
            Ok(Ok(c)) => return Ok(c),
            Ok(Err(e)) => why = e,
            Err(_) => why = Error::Timeout,
        }
        tokio::select! {
            _ = tokio::time::sleep_until(attempt + Duration::from_millis(CALL_REDIAL_MS)) => {}
            m = cmds.recv() => return Err(NotConnected::Told(m)),
        }
    }
}

impl NodeCtx {
    /// Spawns the datagram reader; dropping the returned sender stops it.
    pub fn start_media(&self, call: &Arc<Call>, conn: &Connection) -> oneshot::Sender<()> {
        let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
        let call = call.clone();
        let conn = conn.clone();
        *call.last_rx.lock() = Instant::now();
        self.handle.spawn(async move {
            loop {
                tokio::select! {
                    d = conn.read_datagram() => match d {
                        Ok(d) => {
                            *call.last_rx.lock() = Instant::now();
                            call.receiver.lock().push(&d, call.now_ms());
                        }
                        Err(e) => {
                            // A hangup closes the connection: only an unexpected end is a warning.
                            let id = crate::short(&call.info.call_id);
                            if matches!(e, iroh::endpoint::ConnectionError::ApplicationClosed(_) | iroh::endpoint::ConnectionError::LocallyClosed) {
                                tracing::debug!("call {id}: media ended: {e}");
                            } else {
                                tracing::warn!("call {id}: media ended: {e}");
                            }
                            break;
                        }
                    },
                    _ = &mut stop_rx => break,
                }
            }
        });
        stop_tx
    }

    /// A callee's `Accept` may carry a fresh grant for us. It replaces the stored one only if it
    /// is validly signed by them for us and outlives the old one; anything else is ignored, so a
    /// peer can't shorten or break the grant we call them with.
    pub fn renew_grant(self: &Arc<Self>, epoch: u64, did: &str, grant: proto::SignedGrant) {
        let Ok(me) = self.me() else { return };
        let replaced = {
            let mut s = self.shared.lock();
            if s.epoch != epoch {
                return;
            }
            match s.state.contacts.iter_mut().find(|c| c.did == did) {
                Some(c) if crate::caps::outlives(c.call_grant_from_them.as_ref(), &grant, proto::Cap::Call, did, me.id.did(), now_ms()) => {
                    c.call_grant_from_them = Some(grant);
                    true
                }
                _ => false,
            }
        };
        if replaced {
            self.persist_in_background(epoch);
        }
    }

    /// Never overwrites `Ended`, and the change and its delivery are one step, so the UI sees a
    /// call's states in the order they happened.
    pub fn set_state(&self, call: &Call, state: CallState) {
        let _order = call.notify.lock();
        {
            let mut cur = call.state.lock();
            if matches!(*cur, CallState::Ended { .. }) {
                return;
            }
            *cur = state.clone();
        }
        if state == CallState::Active {
            call.active_at.lock().get_or_insert_with(Instant::now);
        }
        self.log(format!("call {}: {state:?}", call.info.call_id));
        self.sink.on_call_state(call.info.call_id.clone(), state);
    }

    /// Frees the slot, closes the connection and emits Ended — once per call, however many
    /// paths get here. The slot is free before `Ended` is delivered (a UI that reacts to it may
    /// place the next call at once); the next call's first event waits for this one's.
    pub fn end_call(&self, call: &Arc<Call>, reason: String) {
        if std::mem::replace(&mut *call.ended.lock(), true) {
            return;
        }
        let _order = call.notify.lock();
        self.note_direct(call);
        call.ended_wake.notify_one();
        call.video.stop();
        let state = CallState::Ended { reason: reason.clone() };
        *call.state.lock() = state.clone();
        // A waiting call takes over the slot when the active one ends.
        let mut promoted = None;
        {
            let mut live = self.live.lock();
            if live.call.as_ref().is_some_and(|c| Arc::ptr_eq(c, call)) {
                live.call = None;
                live.last = Some(call.clone());
                if let Some(w) = live.waiting.take() {
                    w.waiting.store(false, Ordering::SeqCst);
                    live.call = Some(w.clone());
                    promoted = Some(w);
                }
            } else if live.waiting.as_ref().is_some_and(|c| Arc::ptr_eq(c, call)) {
                live.waiting = None;
            }
            if live.call.is_none() && live.waiting.is_none() {
                self.floor.release(self.slot_id);
            }
        }
        if let Some(conn) = call.conn.lock().take() {
            conn.close(0u32.into(), b"bye");
        }
        self.record_ended(call, &reason);
        self.log(format!("call {}: {state:?}", call.info.call_id));
        self.sink.on_call_state(call.info.call_id.clone(), state);
        if let Some(w) = promoted {
            if w.answer_on_promote.load(Ordering::SeqCst) {
                let _ = w.cmd.send(Cmd::Answer);
            } else {
                // Now an ordinary ringing call: the UI hears of it as a fresh incoming call.
                let _order = w.notify.lock();
                if !matches!(w.state(), CallState::Ended { .. }) {
                    self.sink.on_incoming_call(w.info.clone());
                    self.sink.on_call_state(w.info.call_id.clone(), CallState::Ringing);
                }
            }
        }
    }

    /// Remembers whether the call is on a direct path while we still have its connection.
    pub fn note_direct(&self, call: &Call) {
        if let Some((direct, _)) = call.conn.lock().as_ref().and_then(selected_path) {
            *call.direct.lock() = direct;
        }
    }

    /// Writes the call into the history (before `Ended` is delivered, so the UI's re-read sees
    /// it). Not for a call we gave up on in favour of theirs, nor for a contact since removed.
    pub fn record_ended(&self, call: &Call, reason: &str) {
        if reason == "superseded" {
            return;
        }
        let name = {
            let s = self.shared.lock();
            if s.epoch != call.epoch {
                return;
            }
            // A contact, or one whose block notice we hold (their entry was deleted, U20).
            match crate::block::known_name(&s.state, &call.info.peer_did) {
                Some(n) => n,
                None => return,
            }
        };
        let answered = call.active_at.lock().is_some();
        let duration_secs = call
            .active_at
            .lock()
            .filter(|_| reason != "answered_elsewhere")
            .map_or(0, |t| t.elapsed().as_secs() as u32);
        self.log_call(call.epoch, CallRecord {
            call_id: call.info.call_id.clone(),
            peer_did: call.info.peer_did.clone(),
            peer_name: name,
            incoming: call.info.incoming,
            started_at: call.started_at,
            duration_secs,
            reason: reason.to_string(),
            direct: *call.direct.lock(),
            missed: call.info.incoming
                && !answered
                && !matches!(reason, "declined_local" | "answered_elsewhere" | "declined_elsewhere"),
            video: call.info.video || call.video.camera_seen(),
        });
    }

    pub fn command(&self, call_id: &str, cmd: Cmd) -> Result<(), Error> {
        let live = self.live.lock();
        let call = live
            .call
            .iter()
            .chain(live.waiting.iter())
            .find(|c| c.info.call_id == call_id)
            .ok_or(Error::NotFound)?;
        call.cmd.send(cmd).map_err(|_| Error::NotFound)
    }

    /// Whether `call_id` is this slot's call (or the one waiting over it).
    pub fn has_call(&self, call_id: &str) -> bool {
        let live = self.live.lock();
        live.call.iter().chain(live.waiting.iter()).any(|c| c.info.call_id == call_id)
    }

    pub fn active_call(&self) -> Option<(Arc<Call>, Option<f32>)> {
        let live = self.live.lock();
        let call = live.call.clone()?;
        (call.state() == CallState::Active).then_some((call, live.tone))
    }

    /// Feeds the microphone to the active 1:1 call. `false`: there is none, and the caller may
    /// feed a group call instead.
    pub fn push_call_mic(&self, pcm: &[i16]) -> bool {
        let Some((call, tone)) = self.active_call() else { return false };
        let Some(conn) = call.conn.lock().clone() else { return true };
        let mut mic = call.mic.lock();
        mic.extend_from_slice(pcm);
        let mut frame = [0i16; audio::FRAME];
        while mic.len() >= audio::FRAME {
            frame.copy_from_slice(&mic[..audio::FRAME]);
            mic.drain(..audio::FRAME);
            if let Some(hz) = tone {
                let mut t = call.tone.lock();
                if t.as_ref().is_none_or(|(f, _)| *f != hz) {
                    *t = Some((hz, audio::Tone::new(hz, 10_000.0)));
                }
                t.as_mut().unwrap().1.next_frame(&mut frame);
            }
            let datagram = match call.sender.lock().encode(&frame) {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("call {}: encode: {e}", crate::short(&call.info.call_id));
                    continue;
                }
            };
            if conn.send_datagram(Bytes::from(datagram)).is_ok() {
                *call.sent.lock() += 1;
            }
        }
        true
    }

    /// The next frame of the active 1:1 call; `None`: there is none, and the caller may pull a
    /// group call's instead.
    pub fn pull_call_speaker(&self) -> Option<Vec<i16>> {
        let mut out = [0i16; audio::FRAME];
        let (call, _) = self.active_call()?;
        {
            call.receiver.lock().pull(&mut out, call.now_ms());
            let mut played = call.played.lock();
            played.extend(out.iter().copied());
            let excess = played.len().saturating_sub(audio::RATE as usize);
            played.drain(..excess);
        }
        Some(out.to_vec())
    }

    pub fn call_stats(&self) -> Option<CallStats> {
        let call = self.live.lock().call.clone()?;
        let rx = call.receiver.lock().stats();
        let (direct, rtt_ms) = call.conn.lock().as_ref().and_then(selected_path).unwrap_or((false, 0));
        if call.state() == CallState::Active {
            *call.direct.lock() = direct;
        }
        let state = call.state();
        let played: Vec<i16> = call.played.lock().iter().copied().collect();
        Some(CallStats {
            call_id: call.info.call_id.clone(),
            reconnecting: state == CallState::Active && call.last_rx.lock().elapsed() > RECONNECT_AFTER,
            waking: state == CallState::Dialing && call.waking.load(Ordering::Relaxed),
            state,
            secs: call.started.elapsed().as_secs() as u32,
            direct,
            rtt_ms,
            sent: *call.sent.lock(),
            received: rx.received,
            lost: rx.lost,
            recovered: rx.recovered_fec + rx.recovered_dred,
            concealed: rx.concealed,
            buffered_ms: rx.buffered_ms,
            rx_freq_hz: audio::estimate_frequency(&played).unwrap_or(0.0),
            rx_rms: audio::rms(&played),
        })
    }
}

/// Whether the connection's selected path is direct (not via a relay), and its round trip.
pub fn selected_path(c: &Connection) -> Option<(bool, u32)> {
    c.paths().iter().find(|p| p.is_selected()).map(|p| (p.is_ip(), p.rtt().as_millis() as u32))
}
