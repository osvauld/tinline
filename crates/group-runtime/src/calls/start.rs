//! The starter: picking a host, the periodic tick and the session hello.

use super::*;

impl GroupRt {
    // ---- the starter --------------------------------------------------------------------------

    pub fn group_call_start(self: &Arc<Self>, gid: &str) -> Result<String, Error> {
        let core = self.group_core()?;
        self.group_member_check(&core, gid)?;
        if core.ep_id.lock().is_none() {
            return Err(Error::NotStarted);
        }
        self.group_calls_configure();
        if self.gcall.busy() {
            return Err(Error::Busy);
        }
        self.not_in_call()?;
        let dir = GroupCallDir::new(self);
        let now = now_ms();
        let members = core.groups.lock().get(gid).map_or(0, |l| l.state.members.len());
        let expected = members.clamp(2, MAX_PARTICIPANTS) as u8;
        let call_id = w::new_call_id();
        let blobs: Vec<SignedStatus> = {
            let st = self.calls.st.lock();
            if st.pending.is_some() {
                return Err(Error::Busy);
            }
            st.statuses.get(gid).map(|m| m.values().map(|(b, _)| b.clone()).collect()).unwrap_or_default()
        };
        let me = dir.my_host_ref(gid);
        let ranking = rank::rank(&dir, &blobs, &RankCtx { gid, call_id: &call_id, expected: expected as usize, starter: me.as_ref(), now_ms: now });
        self.log(format!("group {}: starting call {}, {} candidate(s)", short(gid), short(&call_id), ranking.len()));
        let mut picker = Picker::new(ranking);
        let step = picker.start(now);
        self.calls.st.lock().pending = Some(Pending { gid: gid.to_string(), call_id: call_id.clone(), expected, picker });
        self.group_call_step(&core, gid, &call_id, step)?;
        Ok(call_id)
    }

    /// "no call host available: <why, for the person starting>". The apps show the part after
    /// ": ". Why: nobody holds a call-host role; none is online (no fresh status); every one says
    /// busy; or only phones are left, and a phone hosts only its own user's calls (§2.4, Q11).
    fn no_host_reason(self: &Arc<Self>, core: &GroupCore, gid: &str) -> String {
        let now = now_ms();
        let hosts = core.groups.lock().get(gid).map_or(0, |l| l.state.call_hosts.len());
        let i_host = GroupCallDir::new(self).my_host_ref(gid).is_some();
        let (fresh, busy, phones) = self.calls.st.lock().statuses.get(gid).map_or((0, 0, 0), |m| {
            let f: Vec<_> = m.values().map(|(_, s)| s).filter(|s| s.is_fresh(now)).collect();
            let free: Vec<_> = f.iter().filter(|s| !s.busy).collect();
            (f.len(), f.len() - free.len(), free.iter().filter(|s| s.class == DeviceClass::Phone).count())
        });
        let why = if hosts == 0 {
            "Nobody in this group can host calls yet."
        } else if fresh == 0 {
            "Nobody who can host calls is online right now."
        } else if busy == fresh {
            "Everyone who can host is busy right now, maybe in another call."
        } else if !i_host && phones == fresh - busy {
            "Only phones can host here, and a phone hosts only its own calls. An admin can let you host calls, or keep Tinline open on a computer that can."
        } else {
            "Nobody who can host answered."
        };
        format!("no call host available: {why}")
    }

    /// Acts on the picker's next step: ask the candidate (ourselves: host at once), or give up.
    pub(super) fn group_call_step(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, call_id: &str, mut step: PickerStep) -> Result<(), Error> {
        loop {
            let pick = match step {
                PickerStep::Exhausted => {
                    let mut st = self.calls.st.lock();
                    if st.pending.as_ref().is_some_and(|p| p.call_id == call_id) {
                        st.pending = None;
                    }
                    drop(st);
                    let why = self.no_host_reason(core, gid);
                    self.log(format!("group {}: no call host for {}: {why}", short(gid), short(call_id)));
                    self.calls_emit(GroupCallEventKind::StartFailed, gid, call_id, vec![], why.clone());
                    return Err(Error::Rejected(why));
                }
                PickerStep::Ask(p) => p,
            };
            let expected = self.calls.st.lock().pending.as_ref().filter(|p| p.call_id == call_id).map(|p| p.expected);
            let Some(expected) = expected else { return Ok(()) };
            let me = GroupCallDir::new(self).my_host_ref(gid);
            if me.as_ref() == Some(&pick.host) {
                let cfg = HostConfig {
                    gid: gid.to_string(),
                    call_id: call_id.to_string(),
                    host_gen: 1,
                    host_ref: pick.host.clone(),
                    starter_did: core.did.clone(),
                    backups: pick.backups.clone(),
                    meta_frontier: self.calls_frontier(core, gid),
                    cond: self.gcall.cond(),
                    own_user_in: true,
                };
                match self.gcall.host(cfg) {
                    Ok(_) => {
                        let mut st = self.calls.st.lock();
                        if st.pending.as_ref().is_some_and(|p| p.call_id == call_id) {
                            st.pending = None;
                        }
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::debug!("group call: hosting our own call failed: {e}");
                        let next = {
                            let mut st = self.calls.st.lock();
                            st.pending.as_mut().filter(|p| p.call_id == call_id).and_then(|p| p.picker.on_refused(&pick.host, now_ms()))
                        };
                        match next {
                            Some(n) => step = n,
                            None => return Ok(()),
                        }
                        continue;
                    }
                }
            }
            let req = HostRequest {
                gid: gid.to_string(),
                call_id: call_id.to_string(),
                starter_did: core.did.clone(),
                host: pick.host.clone(),
                backups: pick.backups.clone(),
                expected,
                issued_ms: now_ms(),
            };
            let blob =
                Signed::sign(&req, &core.account).map_err(|e| Error::Protocol(e.to_string()))?;
            let sig = *blob.sig();
            let route = Route::Device(pick.host.holder_did.clone(), pick.host.device_slot);
            self.calls_originate(core, gid, &sig, GroupMsg::CallHostRequest { request: blob }, route);
            return Ok(());
        }
    }

    pub(super) fn group_call_refused(self: &Arc<Self>, core: &Arc<GroupCore>, f: &HostRefusal) {
        let next = {
            let mut st = self.calls.st.lock();
            st.pending.as_mut().filter(|p| p.call_id == f.call_id && p.gid == f.gid).and_then(|p| p.picker.on_refused(&f.host, now_ms()))
        };
        if let Some(step) = next {
            self.log(format!("group {}: {} declined call {}: {}", short(&f.gid), short(&f.host.holder_did), short(&f.call_id), node_runtime::clip_reason(&f.reason)));
            let _ = self.group_call_step(core, &f.gid, &f.call_id, step);
        }
    }

    // ---- periodic work (the group dial loop's tick, every ~2 s) --------------------------------

    pub fn group_calls_tick(self: &Arc<Self>, core: &Arc<GroupCore>) {
        let now = now_ms();
        // Our status in every group whose calls we host.
        let mine: Vec<(String, HostRef)> = {
            let dir = GroupCallDir::new(self);
            let gids: Vec<String> = core.groups.lock().keys().cloned().collect();
            gids.into_iter().filter_map(|g| dir.my_host_ref(&g).map(|r| (g, r))).collect()
        };
        if !mine.is_empty() {
            self.group_calls_configure();
        }
        let fed = self.gcall.cond_known();
        let busy = self.gcall.busy();
        for (gid, host_ref) in mine {
            // Nothing until the platform reported its conditions (GA-A1).
            let Some(status) = own_status(&gid, host_ref, fed, busy) else { continue };
            let due = self.calls.st.lock().published.get(&gid).is_none_or(|(s, _, at)| *s != status || now >= at + STATUS_REFRESH_MS);
            if !due {
                continue;
            }
            let Ok(blob) = Signed::sign(
                &CallHostStatus {
                    issued_ms: now,
                    ..status.clone()
                },
                &core.account,
            ) else {
                continue;
            };
            self
                .calls
                .st
                .lock()
                .published
                .insert(gid.clone(), (status.clone(), blob.clone(), now));
            self.calls_keep_status(
                &gid,
                blob.clone(),
                CallHostStatus {
                    issued_ms: now,
                    ..status
                },
            );
            let sig = *blob.sig();
            self.calls_originate(
                core,
                &gid,
                &sig,
                GroupMsg::CallHostStatus { status: blob },
                Route::All,
            );
        }
        // The starter's pick: the asked candidate did not answer in time.
        let step = {
            let mut st = self.calls.st.lock();
            st.pending.as_mut().and_then(|p| p.picker.tick(now).map(|s| (p.gid.clone(), p.call_id.clone(), s)))
        };
        if let Some((gid, call_id, step)) = step {
            let _ = self.group_call_step(core, &gid, &call_id, step);
        }
        // A listen-only hosted call nobody is in ends.
        let empty = self.gcall.status().and_then(|s| {
            let own_in = s.client.as_ref().is_some_and(|c| !matches!(c.state, ClientState::Ended(_)));
            s.hosting.map(|h| h.roster.is_empty() && !own_in)
        });
        let end = {
            let mut st = self.calls.st.lock();
            match empty {
                Some(true) => {
                    let since = *st.empty_since.get_or_insert(now);
                    now >= since + IDLE_END_MS
                }
                _ => {
                    st.empty_since = None;
                    false
                }
            }
        };
        if end {
            self.log("group call: nobody left in the call hosted here, ending it");
            self.gcall.end();
        }
        // Forget what is stale.
        let mut st = self.calls.st.lock();
        for m in st.statuses.values_mut() {
            m.retain(|_, (_, s)| s.is_fresh(now));
        }
        for c in st.calls.values_mut() {
            c.retain(|_, (_, s)| s.started_ms + CALL_KEEP_MS > now);
        }
        st.seen.retain(|_, t| *t > now);
        st.ends.retain(|_, (_, _, _, t)| *t > now);
    }

    /// A new session: a member we host gets the fresh statuses and live calls; another host
    /// gets them too when we host; our host gets our own status (and the call we host).
    pub fn group_calls_on_session(&self, core: &GroupCore, sess: &Sess) {
        let (we_host, peer_host, member) = {
            let groups = core.groups.lock();
            let Some(l) = groups.get(&sess.gid) else { return };
            let st = &l.state;
            (st.is_host(&core.did, &core.dev), st.is_host(&sess.did, &sess.dev), st.is_member(&sess.did) && !st.is_removed(&sess.did))
        };
        let all = (sess.hosting && !peer_host && member && we_host) || (peer_host && we_host);
        let now = now_ms();
        let (statuses, calls): (Vec<SignedStatus>, Vec<(SignedStart, CallStart)>) = {
            let st = self.calls.st.lock();
            let statuses = if all {
                st.statuses.get(&sess.gid).map(|m| m.values().filter(|(_, s)| s.is_fresh(now)).map(|(b, _)| b.clone()).collect()).unwrap_or_default()
            } else if peer_host {
                st.published.get(&sess.gid).filter(|(_, _, at)| now < at + STATUS_TTL_MS).map(|(_, b, _)| vec![b.clone()]).unwrap_or_default()
            } else {
                vec![]
            };
            let calls = if all || peer_host {
                st.calls.get(&sess.gid).map(|c| c.values().filter(|(_, s)| all || s.host_ref.holder_did == core.did).cloned().collect()).unwrap_or_default()
            } else {
                vec![]
            };
            (statuses, calls)
        };
        let ends: Vec<SignedEnd> = if all || peer_host {
            let st = self.calls.st.lock();
            st.ends.values().filter(|(g, _, _, until)| *g == sess.gid && *until > now).map(|(_, _, b, _)| b.clone()).collect()
        } else {
            vec![]
        };
        for end in ends {
            let _ = sess.tx.send(Out::Msg(GroupMsg::CallEnd { end }));
        }
        for status in statuses {
            let _ = sess.tx.send(Out::Msg(GroupMsg::CallHostStatus { status }));
        }
        for (start, s) in calls {
            let hosts = self.call_addrs(core, &sess.gid, &s);
            let _ = sess.tx.send(Out::Msg(GroupMsg::CallStart { start, hosts }));
        }
    }
}
