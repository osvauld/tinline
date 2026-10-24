//! Placing a call: the slot, fanning out to the contact's devices.

use super::*;

impl NodeCtx {
    pub fn new_call(
        info: CallInfo,
        state: CallState,
        epoch: u64,
    ) -> Result<(Arc<Call>, mpsc::UnboundedReceiver<Cmd>), Error> {
        // Everything slow or fallible comes before the slot is taken.
        let sender = audio::Sender::new(48_000)?;
        let receiver = audio::Receiver::new()?;
        let (tx, rx) = mpsc::unbounded_channel();
        let call = Arc::new(Call {
            info,
            conn: Mutex::new(None),
            state: Mutex::new(state),
            cmd: tx,
            started: Instant::now(),
            sender: Mutex::new(sender),
            receiver: Mutex::new(receiver),
            mic: Mutex::new(Vec::with_capacity(audio::FRAME * 2)),
            played: Mutex::new(VecDeque::with_capacity(audio::RATE as usize)),
            sent: Mutex::new(0),
            tone: Mutex::new(None),
            ended: Mutex::new(false),
            notify: ReentrantMutex::new(()),
            last_rx: Mutex::new(Instant::now()),
            started_at: now(),
            active_at: Mutex::new(None),
            direct: Mutex::new(false),
            waiting: AtomicBool::new(false),
            answer_on_promote: AtomicBool::new(false),
            epoch,
            ended_wake: tokio::sync::Notify::new(),
            video: Default::default(),
        });
        Ok((call, rx))
    }

    /// Takes the call slot. With `announce` the first state is delivered to the UI before any
    /// other thread can see (and end) the call, and after the previous call's events.
    pub fn begin_call(
        &self,
        info: CallInfo,
        state: CallState,
        announce: bool,
        epoch: u64,
    ) -> Result<(Arc<Call>, mpsc::UnboundedReceiver<Cmd>), Error> {
        let (call, rx) = Self::new_call(info, state.clone(), epoch)?;
        {
            let _first = call.notify.lock();
            let previous = {
                let mut live = self.live.lock();
                if live.call.is_some() {
                    return Err(Error::Busy);
                }
                // The user is in a group call (G6): incoming 1:1 calls get Busy, outgoing InCall.
                if self.hooks().is_some_and(|h| h.in_group_call()) {
                    return Err(Error::InCall);
                }
                live.call = Some(call.clone());
                live.last.take()
            };
            // The previous call's `Ended` is still being delivered on another thread, perhaps.
            let _previous = previous.as_ref().map(|p| p.notify.lock());
            if announce {
                self.log(format!("call {}: {state:?}", call.info.call_id));
                self.sink.on_call_state(call.info.call_id.clone(), state);
            }
        }
        Ok((call, rx))
    }

    /// Parks an incoming call beside the active one. Busy unless there is an answered call
    /// and nothing already waiting.
    pub fn begin_waiting(&self, info: CallInfo, epoch: u64) -> Result<(Arc<Call>, mpsc::UnboundedReceiver<Cmd>), Error> {
        let (call, rx) = Self::new_call(info, CallState::Ringing, epoch)?;
        call.waiting.store(true, Ordering::SeqCst);
        let mut live = self.live.lock();
        match &live.call {
            Some(c) if c.state() == CallState::Active && live.waiting.is_none() => {
                live.waiting = Some(call.clone());
                Ok((call, rx))
            }
            _ => Err(Error::Busy),
        }
    }

    pub fn start_call(self: Arc<Self>, did: String, video: bool) -> Result<CallInfo, Error> {
        let me = self.me()?;
        let ep = self.endpoint()?;
        let (contact, held_name) = {
            let s = self.shared.lock();
            if s.state.blocked.contains(&did) {
                return Err(Error::Protocol("you blocked them; unblock first".into()));
            }
            // They block us (U20): no dialling; the call ends at once with that reason.
            let held = crate::block::blocked_me(&s.state, &did).then(|| crate::block::known_name(&s.state, &did).unwrap_or_default());
            let contact = s.state.contacts.iter().find(|c| c.did == did).cloned();
            if contact.is_none() && held.is_none() {
                return Err(Error::NotFound);
            }
            (contact, held)
        };
        let info = CallInfo {
            call_id: random_id(),
            peer_did: did,
            peer_name: contact.as_ref().map(display_name).or(held_name.clone()).unwrap_or_default(),
            incoming: false,
            // Only a call that offers video can be one.
            video: video && self.video_enabled(),
        };
        let contact = match (contact, held_name) {
            (Some(c), None) => c,
            _ => {
                let (call, _cmds) = self.begin_call(info.clone(), CallState::Dialing, true, me.epoch)?;
                let this = self.clone();
                self.handle.spawn(async move { this.end_call(&call, crate::block::BLOCKED.into()) });
                return Ok(info);
            }
        };
        let (call, cmds) = self.begin_call(info.clone(), CallState::Dialing, true, me.epoch)?;
        if info.video {
            call.video.camera_on_from_start();
        }
        let this = self.clone();
        self.handle.spawn(async move {
            // A panic below must not leave the slot busy forever.
            let _slot = SlotGuard { inner: &this, call: call.clone() };
            this.clone().run_outgoing(ep, me, contact, call, cmds).await;
        });
        Ok(info)
    }

    /// One logical call to a person: one call id, dialled at all their known devices at once
    /// (design: `docs/design/device-linking.md` section 7). We are the coordinator:
    ///
    /// - the first `Accept` wins and its connection becomes the call; every other leg gets
    ///   `Cancel(AnsweredElsewhere)` (a leg that accepted too, simultaneously, is sent the same
    ///   and ends on its side);
    /// - a `Decline` from any device ends the call `declined`, the rest get
    ///   `Cancel(DeclinedElsewhere)`;
    /// - `Busy` / `Reject` / a dead connection only drop that leg; when none is left the call ends
    ///   `no_answer` if some device rang and gave up, else `busy` if every device we reached said
    ///   busy, else `unreachable`;
    /// - hanging up while dialling or ringing sends `Cancel(CallerHangup)` to every leg (a plain
    ///   `Hangup` when we hold no device list for them: that is how a peer from before device lists
    ///   understands it).
    ///
    /// Media is read from the winning connection only; messages of the other legs are not read
    /// once they are cancelled.
    pub async fn run_outgoing(
        self: Arc<Self>,
        ep: Endpoint,
        me: Arc<Me>,
        contact: StoredContact,
        call: Arc<Call>,
        mut cmds: mpsc::UnboundedReceiver<Cmd>,
    ) {
        let offer = self.video_offer(&call);
        let (mut legs, mut ev_rx) = self.spawn_legs(&ep, &me, &contact, &call, offer.clone());
        // What each leg is doing, to decide the end when none is left.
        #[derive(Clone, Copy, PartialEq)]
        enum LegPhase {
            Dialing,
            Live,
            Busy,
            Gone,
            GaveUp,
        }
        let mut state = vec![LegPhase::Dialing; legs.len()];
        // Telling the others is queued; each leg task sends it and closes.
        let cancel_others = |legs: &mut Vec<Option<mpsc::UnboundedSender<Msg>>>, keep: Option<usize>, msg: Msg| {
            for (i, l) in legs.iter_mut().enumerate() {
                if Some(i) != keep
                    && let Some(tx) = l.take()
                {
                    let _ = tx.send(msg.clone());
                }
            }
        };
        let hangup_msg = if contact.device_list.is_some() {
            Msg::Cancel { reason: proto::CancelReason::CallerHangup }
        } else {
            Msg::Hangup
        };
        // Legs that may redial a woken device (`connect_leg`) get the wake window on top.
        let wakeable = contact.devices.iter().any(|d| self.can_wake(&contact.did, &d.device, wake::Kind::Call));
        let wake_extra = if wakeable { Duration::from_millis(crate::push::CALL_WAKE_DIAL_MS) } else { Duration::ZERO };
        let dial_deadline = tokio::time::Instant::now() + DIAL_TOTAL + wake_extra;
        let mut ring_deadline: Option<tokio::time::Instant> = None;
        let reason: String = loop {
            if *call.ended.lock() {
                // Ended from outside (glare: a sibling's simultaneous call won; lock).
                cancel_others(&mut legs, None, hangup_msg.clone());
                return;
            }
            if state.iter().all(|s| matches!(s, LegPhase::Busy | LegPhase::Gone | LegPhase::GaveUp)) {
                break if state.contains(&LegPhase::GaveUp) {
                    "no_answer"
                } else if state.contains(&LegPhase::Busy) {
                    "busy"
                } else {
                    "unreachable"
                }
                .into();
            }
            tokio::select! {
                ev = ev_rx.recv() => {
                    let Some((idx, ev)) = ev else { break "unreachable".into() };
                    match ev {
                        LegEvent::Connected => {
                            state[idx] = LegPhase::Live;
                            ring_deadline.get_or_insert(tokio::time::Instant::now() + RING_TIMEOUT);
                        }
                        LegEvent::Ringing => {
                            if call.state() == CallState::Dialing {
                                self.set_state(&call, CallState::Ringing);
                            }
                        }
                        LegEvent::Accepted { renewed_grant, devices, video, conn, ctrl } => {
                            legs[idx] = None;
                            if *call.ended.lock() {
                                conn.close(0u32.into(), b"bye");
                                continue;
                            }
                            if let Some(g) = renewed_grant {
                                self.renew_grant(call.epoch, &call.info.peer_did, g);
                            }
                            if let Some(list) = &devices {
                                self.accept_device_list(call.epoch, &call.info.peer_did, list);
                            }
                            cancel_others(&mut legs, None, Msg::Cancel { reason: proto::CancelReason::AnsweredElsewhere });
                            // A device that answered at the same moment has an `Accepted` still in
                            // the queue: it gets the same cancel.
                            Self::cancel_late_answers(&self.handle, ev_rx, Msg::Cancel { reason: proto::CancelReason::AnsweredElsewhere });
                            *call.conn.lock() = Some(conn.clone());
                            let datagrams = self.start_media(&call, &conn);
                            self.set_state(&call, CallState::Active);
                            if let Some(h) = self.hooks() {
                                h.chat_call_activity(&call.info.peer_did);
                            }
                            // An answer may only take video we offered.
                            let theirs = video.filter(|_| offer.is_some());
                            self.video_decided(&call, &conn, offer, theirs);
                            self.clone().run_call(call, conn, *ctrl, cmds, None, Some(datagrams)).await;
                            return;
                        }
                        LegEvent::Declined(why) => {
                            self.log(format!("declined: {}", clip_reason(&why)));
                            cancel_others(&mut legs, Some(idx), Msg::Cancel { reason: proto::CancelReason::DeclinedElsewhere });
                            break "declined".into();
                        }
                        LegEvent::Busy => {
                            legs[idx] = None;
                            state[idx] = LegPhase::Busy;
                        }
                        LegEvent::GaveUp => {
                            legs[idx] = None;
                            state[idx] = LegPhase::GaveUp;
                        }
                        LegEvent::Failed(why) => {
                            self.log(format!("device {idx} of {}: {why}", contact.name));
                            legs[idx] = None;
                            state[idx] = LegPhase::Gone;
                        }
                        // They block us: the call ends at once with that reason.
                        LegEvent::Blocked(notice) if self.is_block_from(&call.info.peer_did, &notice) => {
                            let _ = self.take_notice(&notice);
                            cancel_others(&mut legs, Some(idx), hangup_msg.clone());
                            break crate::block::BLOCKED.into();
                        }
                        LegEvent::Blocked(_) => {
                            legs[idx] = None;
                            state[idx] = LegPhase::Gone;
                        }
                        // They let us message, not call: ends at once (P6), our call token goes.
                        LegEvent::CallsNotAllowed => {
                            self.drop_call_token(call.epoch, &call.info.peer_did);
                            cancel_others(&mut legs, Some(idx), hangup_msg.clone());
                            break crate::caps::CALLS_NOT_ALLOWED.into();
                        }
                    }
                }
                cmd = cmds.recv() => match cmd {
                    Some(Cmd::Answer) => {}
                    // Decline / hangup while dialling or ringing, or the handle went away.
                    _ => {
                        cancel_others(&mut legs, None, hangup_msg.clone());
                        break "cancelled".into();
                    }
                },
                _ = call.ended_wake.notified() => {}
                _ = tokio::time::sleep_until(dial_deadline), if ring_deadline.is_none() => {
                    cancel_others(&mut legs, None, hangup_msg.clone());
                    break "unreachable".into();
                }
                _ = async { tokio::time::sleep_until(ring_deadline.expect("guarded")).await }, if ring_deadline.is_some() => {
                    cancel_others(&mut legs, None, Msg::Hangup);
                    break "no_answer".into();
                }
            }
        };
        // Answers that crossed our decision (or a hangup) still need telling.
        let late = match reason.as_str() {
            "declined" => Some(Msg::Cancel { reason: proto::CancelReason::DeclinedElsewhere }),
            "cancelled" => Some(hangup_msg.clone()),
            _ => None,
        };
        if let Some(late) = late {
            Self::cancel_late_answers(&self.handle, ev_rx, late);
        }
        self.end_call(&call, reason);
    }

    /// Dials every device of the callee: one `Leg` task each, with its hello. Returns the
    /// senders to tell each leg what to do next and the receiver of what the legs report.
    #[allow(clippy::type_complexity)]
    fn spawn_legs(
        self: &Arc<Self>,
        ep: &Endpoint,
        me: &Me,
        contact: &StoredContact,
        call: &Call,
        offer: Option<proto::VideoOffer>,
    ) -> (Vec<Option<mpsc::UnboundedSender<Msg>>>, mpsc::UnboundedReceiver<(usize, LegEvent)>) {
        let list = self.my_device_list(me);
        let (ev_tx, ev_rx) = mpsc::unbounded_channel::<(usize, LegEvent)>();
        let mut legs: Vec<Option<mpsc::UnboundedSender<Msg>>> = Vec::new();
        for (idx, device) in contact.devices.iter().enumerate() {
            let (tx, rx) = mpsc::unbounded_channel();
            legs.push(Some(tx));
            let leg = Leg {
                ctx: self.clone(),
                did: contact.did.clone(),
                ep: ep.clone(),
                device: device.clone(),
                hello: {
                    // Our call token from them; without one, the message token (they answer "calls
                    // not allowed" instead of a generic refusal).
                    let mut hello = proto::call_hello(
                        me.attestation.clone(),
                        contact.call_grant_from_them.clone().unwrap_or_else(|| contact.grant_from_them.clone()),
                        call.info.call_id.clone(),
                    );
                    if let Msg::CallHello { relay, devices, video, camera, .. } = &mut hello {
                        *relay = relay_of(ep);
                        *devices = list.clone();
                        *video = offer.clone();
                        *camera = call.info.video && offer.is_some();
                    }
                    hello
                },
                idx,
                events: ev_tx.clone(),
                cmds: rx,
                name: contact.name.clone(),
            };
            self.handle.spawn(leg.run());
        }
        drop(ev_tx);
        (legs, ev_rx)
    }

    /// Keeps reading the legs' events after the call was decided: a device whose `Accept` was
    /// already on its way gets `msg` (a `Cancel`) and is closed, so it stops instead of waiting
    /// on a call nobody will carry. Ends when the last leg task is gone.
    pub fn cancel_late_answers(
        handle: &tokio::runtime::Handle,
        mut events: mpsc::UnboundedReceiver<(usize, LegEvent)>,
        msg: Msg,
    ) {
        handle.spawn(async move {
            while let Some((_, ev)) = events.recv().await {
                if let LegEvent::Accepted { conn, mut ctrl, .. } = ev {
                    let _ = ctrl.send(&msg).await;
                    ctrl.finish();
                    let _ = tokio::time::timeout(Duration::from_millis(300), conn.closed()).await;
                    conn.close(0u32.into(), b"bye");
                }
            }
        });
    }
}
