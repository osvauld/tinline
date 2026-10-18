//! Incoming call setup and the shared call loop.

use super::*;

impl Inner {
    pub(crate) async fn incoming_call(
        self: Arc<Self>,
        conn: Connection,
        mut ctrl: Ctrl,
        hello: Msg,
        remote: [u8; 32],
        me: &Me,
        permit: OwnedSemaphorePermit,
    ) -> Result<(), Error> {
        let Msg::CallHello { call_id, relay: hint, devices: list, video, .. } = &hello else { unreachable!() };
        let (call_id, hint, list, video) = (call_id.clone(), relay_hint(hint), list.clone(), video.clone());
        // It reaches the UI and logs: keep it to what we ourselves generate.
        if call_id.is_empty() || call_id.len() > 64 || !call_id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
            ctrl.finish();
            let _ = tokio::time::timeout(LINGER, conn.closed()).await;
            return Ok(());
        }
        let verified = {
            let s = self.shared.lock();
            let contacts = &s.state.contacts;
            // Any of our tokens says who they are; which one rings is checked after the block
            // check (P6).
            proto::accept_hello(
                &me.id,
                &hello,
                remote,
                now_ms(),
                |did| contacts.iter().any(|c| c.did == did),
            )
        };
        let caller = match verified {
            Ok(c) => c,
            Err(e) => {
                self.log(format!("rejected call: {e}"));
                ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
                ctrl.finish();
                let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                return Ok(());
            }
        };
        // A DID we block: refused at once with our notice, never rung (the caller's app ends the
        // call with "blocked").
        if let Some(notice) = self.refusal_for(&caller.did) {
            self.log("incoming call refused: blocked");
            ctrl.send(&Msg::Notice { notice }).await?;
            ctrl.finish();
            let _ = tokio::time::timeout(LINGER, conn.closed()).await;
            return Ok(());
        }
        // P6: a call token of ours, not taken back, and calls allowed for them. A contact that
        // may only message is told so (never rung); a token taken back by a block gets the
        // generic refusal.
        if let Err(denied) = self.token_check(&caller.did, &caller.grant, proto::Cap::Call) {
            let reason = match denied {
                crate::caps::Denied::CallsNotAllowed => crate::CALLS_NOT_ALLOWED,
                crate::caps::Denied::Refused => REFUSED,
            };
            self.log(format!("incoming call refused: {reason}"));
            ctrl.send(&Msg::Reject { reason: reason.into() }).await?;
            ctrl.finish();
            let _ = tokio::time::timeout(LINGER, conn.closed()).await;
            return Ok(());
        }
        let name = {
            let s = self.shared.lock();
            s.state.contacts.iter().find(|c| c.did == caller.did).map(display_name).unwrap_or_default()
        };
        if !self.availability().available {
            // Same answer as any other refusal: the caller must not learn we are "away".
            self.log("incoming call turned away: unavailable");
            self.log_refused(me.epoch, &call_id, &caller.did, &name, "unavailable", false);
            ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
            ctrl.finish();
            let _ = tokio::time::timeout(LINGER, conn.closed()).await;
            return Ok(());
        }
        let info = CallInfo { call_id, peer_did: caller.did.clone(), peer_name: name, incoming: true };
        self.yield_on_glare(&caller.did, me.device, remote);
        let (call, cmds) = match self.begin_call(info.clone(), CallState::Ringing, false, me.epoch) {
            Ok(c) => c,
            // In a call: ring quietly over it, if nothing is waiting there already.
            Err(_) => match self.begin_waiting(info.clone(), me.epoch) {
                Ok(c) => {
                    self.log("incoming call while in a call: waiting");
                    c
                }
                Err(_) => {
                    self.log_refused(me.epoch, &info.call_id, &info.peer_did, &info.peer_name, "busy", true);
                    ctrl.send(&Msg::Busy).await?;
                    ctrl.finish();
                    let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                    return Ok(());
                }
            },
        };
        // From here every exit must free the slot; the guard does it if nothing else did.
        let _slot = SlotGuard { inner: &self, call: call.clone() };
        *call.conn.lock() = Some(conn.clone());
        *call.video.peer_offer.lock() = video;
        // Their device list first (it decides whether this device is dialled), then this device.
        if let Some(list) = &list {
            self.accept_device_list(me.epoch, &caller.did, list);
        }
        self.note_device(me.epoch, &caller.did, remote, hint);
        ctrl.send(&Msg::Ringing).await?;
        self.log(format!("incoming call from {}", info.peer_name));
        {
            // Not if the call already ended meanwhile: the UI never heard of it.
            let _order = call.notify.lock();
            if !matches!(call.state(), CallState::Ended { .. }) {
                self.events.on_incoming_call(info.clone());
                self.events.on_call_state(info.call_id.clone(), CallState::Ringing);
            }
        }
        // Authenticated and holding the call slot: no longer one of the unproven connections.
        drop(permit);
        self.clone().run_call(call, conn, ctrl, cmds, Some(caller.did), None).await;
        Ok(())
    }

    /// Both sides dialled each other at once: each would answer the other's hello with Busy
    /// and both calls die. The call from the lower device key wins; the higher side drops its
    /// own outgoing call so the incoming one can take the slot.
    pub(crate) fn yield_on_glare(&self, peer: &str, mine: [u8; 32], theirs: [u8; 32]) {
        let ours = self.live.lock().call.clone();
        if let Some(ours) = ours
            && !ours.info.incoming
            && ours.info.peer_did == peer
            && matches!(ours.state(), CallState::Dialing | CallState::Ringing)
            && mine > theirs
        {
            self.end_call(&ours, "superseded".into());
        }
    }

    /// Drives one call from ringing to the end, on either side. `incoming_from` is the caller's
    /// DID when we are the callee, so the answer can carry a renewed grant.
    pub(crate) async fn run_call(
        self: Arc<Self>,
        call: Arc<Call>,
        conn: Connection,
        mut ctrl: Ctrl,
        mut cmds: mpsc::UnboundedReceiver<Cmd>,
        incoming_from: Option<String>,
        preactive: Option<oneshot::Sender<()>>,
    ) {
        let ring = if call.waiting.load(Ordering::SeqCst) { waiting_ring_timeout() } else { RING_TIMEOUT };
        let ring_deadline = tokio::time::Instant::now() + ring;
        let mut datagrams: Option<oneshot::Sender<()>> = preactive;
        // Our camera state goes to the peer once video runs (first the current one).
        let mut camera = call.video.camera_changes();
        camera.mark_changed();
        let mut watchdog = tokio::time::interval(Duration::from_secs(5));
        watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // How a call that dies under us ends, by who we are and how far it got.
        let lost = |active: bool| -> String {
            if active { "connection_lost" } else if incoming_from.is_some() { "cancelled" } else { "unreachable" }.into()
        };
        let reason: String = loop {
            if *call.ended.lock() {
                break "ended".into();
            }
            let active = call.state() == CallState::Active;
            tokio::select! {
                msg = ctrl.recv() => match msg {
                    Ok(Some(Msg::Ringing)) if !active => self.set_state(&call, CallState::Ringing),
                    // The caller told us another of our devices took the call (or declined it),
                    // or that it gave up. If we had answered meanwhile, we stop at once.
                    Ok(Some(Msg::Cancel { reason })) if incoming_from.is_some() => {
                        self.log(format!("call cancelled by the caller: {reason:?}"));
                        break match reason {
                            proto::CancelReason::AnsweredElsewhere => "answered_elsewhere",
                            proto::CancelReason::DeclinedElsewhere => "declined_elsewhere",
                            proto::CancelReason::CallerHangup if active => "hangup_remote",
                            proto::CancelReason::CallerHangup => "cancelled",
                        }
                        .into();
                    }
                    Ok(Some(Msg::Decline { reason })) => {
                        self.log(format!("declined: {}", clip_reason(&reason)));
                        break "declined".into();
                    }
                    Ok(Some(Msg::Busy)) => break "busy".into(),
                    // A refusal (not a contact there, blocked, or away) reads as "couldn't reach".
                    Ok(Some(Msg::Reject { reason })) => {
                        self.log(format!("rejected: {}", clip_reason(&reason)));
                        break "unreachable".into();
                    }
                    // They blocked us during the call.
                    Ok(Some(Msg::Notice { notice })) if self.is_block_from(&call.info.peer_did, &notice) => {
                        let _ = self.take_notice(&notice);
                        break crate::BLOCKED.into();
                    }
                    Ok(Some(Msg::Hangup)) => {
                        // A callee only hangs up unanswered when its ring timed out; a refusal is a Decline.
                        break if active { "hangup_remote" } else if incoming_from.is_some() { "cancelled" } else { "no_answer" }.into();
                    }
                    Ok(Some(Msg::Video { camera })) if active => self.remote_camera(&call, camera),
                    Ok(None) => break if active { "hangup_remote".into() } else { lost(false) },
                    Ok(Some(other)) => self.log(format!("ignoring {} mid-call", msg_name(&other))),
                    Err(e) => {
                        self.log(format!("connection lost: {e}"));
                        break lost(active);
                    }
                },
                cmd = cmds.recv() => match cmd {
                    Some(Cmd::Answer) if incoming_from.is_some() && !active => {
                        let me = match self.me() { Ok(m) => m, Err(_) => break "hangup_local".into() };
                        let renewed = incoming_from.as_deref().map(|did| self.fresh_token(&me, did, proto::Cap::Call));
                        let devices = self.my_device_list(&me);
                        // Video only if they offered it and we take it.
                        let theirs = call.video.peer_offer.lock().clone();
                        let video = theirs.as_ref().and_then(|_| self.video_offer(&call));
                        if let Err(e) = ctrl.send(&Msg::Accept { renewed_grant: renewed, devices, video: video.clone() }).await {
                            self.log(format!("connection lost: {e}"));
                            break lost(false);
                        }
                        datagrams = Some(self.start_media(&call, &conn));
                        self.set_state(&call, CallState::Active);
                        self.chat_call_activity(&call.info.peer_did);
                        self.video_decided(&call, &conn, video, theirs);
                    }
                    Some(Cmd::Answer) => {}
                    Some(Cmd::Block(notice)) => {
                        let _ = ctrl.send(&Msg::Notice { notice }).await;
                        break crate::BLOCKED.into();
                    }
                    // Not yet answered: the callee refusing is a decline, the caller giving up
                    // is a hangup (the callee shows it as missed).
                    Some(Cmd::Decline) | Some(Cmd::Hangup) if !active && incoming_from.is_some() => {
                        // Declined over another call: the caller is told we are busy.
                        let reply = if call.waiting.load(Ordering::SeqCst) {
                            Msg::Busy
                        } else {
                            Msg::Decline { reason: "declined".into() }
                        };
                        let _ = ctrl.send(&reply).await;
                        break "declined_local".into();
                    }
                    Some(Cmd::Decline) | Some(Cmd::Hangup) if !active => {
                        let _ = ctrl.send(&Msg::Hangup).await;
                        break "cancelled".into();
                    }
                    Some(Cmd::Decline) | Some(Cmd::Hangup) | None => {
                        let _ = ctrl.send(&Msg::Hangup).await;
                        break "hangup_local".into();
                    }
                },
                why = conn.closed() => {
                    self.log(format!("connection lost: {why}"));
                    break lost(active);
                }
                r = camera.changed(), if active && call.video.negotiated() => {
                    let on = *camera.borrow_and_update();
                    if r.is_ok() && let Err(e) = ctrl.send(&Msg::Video { camera: on }).await {
                        self.log(format!("connection lost: {e}"));
                        break lost(active);
                    }
                }
                _ = watchdog.tick(), if active => {
                    if call.last_rx.lock().elapsed() > NO_AUDIO_TIMEOUT {
                        let _ = ctrl.send(&Msg::Hangup).await;
                        break "connection_lost".into();
                    }
                }
                _ = tokio::time::sleep_until(ring_deadline), if !active => {
                    let _ = ctrl.send(&Msg::Hangup).await;
                    break "no_answer".into();
                }
            }
        };
        ctrl.finish();
        drop(datagrams);
        // Free the slot now, so a call placed right after this one isn't Busy; the connection
        // gets a moment in the background for the final frame to leave.
        self.note_direct(&call);
        call.conn.lock().take();
        self.end_call(&call, reason);
        tokio::spawn(async move {
            let _ = tokio::time::timeout(Duration::from_millis(300), conn.closed()).await;
            conn.close(0u32.into(), b"bye");
        });
    }
}
