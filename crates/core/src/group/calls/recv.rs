//! Receiving: call frames from sessions, and the device that is asked to host.

use super::*;

impl Inner {
    // ---- receiving --------------------------------------------------------------------------

    /// A `GroupMsg::Call*` from a session (`from`): verified, handled here if it concerns us,
    /// and passed on when we host.
    pub(crate) fn group_call_msg(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, msg: GroupMsg, from: Option<&Arc<Sess>>) {
        let dir = GroupCallDir::new(self);
        let now = now_ms();
        // A member session we host may send only frames it signed itself.
        let member_sender: Option<String> =
            from.filter(|s| s.hosting && !core.groups.lock().get(gid).is_some_and(|l| l.state.is_host(&s.did, &s.dev))).map(|s| s.did.clone());
        let signer_ok = |signer: &str| member_sender.as_deref().is_none_or(|d| d == signer);
        let fid = from.map(|s| s.id);
        let big = |b: &SignedBlob| b.payload.len() + b.signature.len() > MAX_BLOB;
        match msg {
            GroupMsg::CallHostStatus { status } => {
                if big(&status) {
                    return;
                }
                let Ok(s) = rank::admit(&dir, gid, &status, now) else { return };
                if !signer_ok(&s.host_ref.holder_did) || !self.calls_first_sight(&status.signature, s.expires_ms()) {
                    return;
                }
                self.calls_keep_status(gid, status.clone(), s);
                self.calls_send(core, gid, &GroupMsg::CallHostStatus { status }, &Route::All, fid);
            }
            GroupMsg::CallStart { start, hosts } => {
                if big(&start) {
                    return;
                }
                let Ok(s) = w::open_start(&start) else { return };
                let ok = s.gid == gid && dir.host_permit_ok(gid, &s.host_ref) && dir.is_member(gid, &s.starter_did) && signer_ok(&s.host_ref.holder_did);
                if !ok || !self.calls_first_sight(&start.signature, now + CALL_KEEP_MS) {
                    return;
                }
                let hosts: Vec<SignedHostAddr> = hosts.into_iter().take(1 + MAX_BACKUPS).collect();
                core.merge_hosts_pub(gid, &hosts);
                self.group_call_started(gid, start.clone(), s);
                self.calls_send(core, gid, &GroupMsg::CallStart { start, hosts }, &Route::All, fid);
            }
            GroupMsg::CallEnd { end } => {
                if big(&end) {
                    return;
                }
                let Ok(e) = w::open_end(&end) else { return };
                let known = self.group.calls.st.lock().calls.get(gid).and_then(|c| c.get(&e.call_id)).is_some_and(|(_, s)| s.host_ref.holder_did == e.host_did);
                let unknown = !known && !self.group.calls.st.lock().calls.get(gid).is_some_and(|c| c.contains_key(&e.call_id)) && dir.is_member(gid, &e.host_did);
                if e.gid != gid || e.reason != EndReason::Ended || !(known || unknown) || !signer_ok(&e.host_did) || !self.calls_first_sight(&end.signature, now + CALL_KEEP_MS) {
                    return;
                }
                self.group_call_ended(gid, &e.call_id, &end);
                self.calls_send(core, gid, &GroupMsg::CallEnd { end }, &Route::All, fid);
            }
            GroupMsg::CallHostRequest { request } => {
                if big(&request) {
                    return;
                }
                let Ok(r) = w::open_host_request(&request) else { return };
                let fresh = r.issued_ms + REQUEST_MAX_AGE_MS > now && r.issued_ms <= now + gs::MAX_CLOCK_SKEW_MS;
                let ok = r.gid == gid && fresh && dir.is_member(gid, &r.starter_did) && dir.host_permit_ok(gid, &r.host) && signer_ok(&r.starter_did);
                if !ok || !self.calls_first_sight(&request.signature, now + REQUEST_MAX_AGE_MS) {
                    return;
                }
                // For this device (also when paused: it then declines).
                let for_me = core.groups.lock().get(gid).and_then(|l| resolve(&l.state, &r.host)).is_some_and(|h| h.did == core.did && h.dev == core.dev);
                if for_me {
                    self.group_call_requested(core, gid, r);
                    return;
                }
                let route = Route::Device(r.host.holder_did.clone(), r.host.device_slot);
                self.calls_send(core, gid, &GroupMsg::CallHostRequest { request }, &route, fid);
            }
            GroupMsg::CallHostRefusal { refusal } => {
                if big(&refusal) {
                    return;
                }
                let Ok(f) = w::open_host_refusal(&refusal) else { return };
                let ok = f.gid == gid && dir.is_member(gid, &f.starter_did) && signer_ok(&f.host.holder_did);
                if !ok || !self.calls_first_sight(&refusal.signature, now + REQUEST_MAX_AGE_MS) {
                    return;
                }
                if f.starter_did == core.did {
                    self.group_call_refused(core, &f);
                }
                let route = Route::Did(f.starter_did.clone());
                self.calls_send(core, gid, &GroupMsg::CallHostRefusal { refusal }, &route, fid);
            }
            _ => {}
        }
    }

    pub(super) fn calls_keep_status(&self, gid: &str, blob: SignedBlob, s: CallHostStatus) {
        let mut st = self.group.calls.st.lock();
        let m = st.statuses.entry(gid.to_string()).or_default();
        let key = (s.host_ref.holder_did.clone(), s.host_ref.device_slot);
        if m.get(&key).is_none_or(|(_, have)| have.issued_ms <= s.issued_ms) {
            m.insert(key, (blob, s));
        }
    }

    /// A CallStart (relayed, or one of ours): remembered, fed to the call board, joined by
    /// the starter whose pick it answers.
    pub(super) fn group_call_started(self: &Arc<Self>, gid: &str, blob: SignedBlob, s: CallStart) {
        let new_call = {
            let mut st = self.group.calls.st.lock();
            if st.ended.contains(&s.call_id) || st.ends.get(&s.call_id).is_some_and(|(_, host, _, _)| *host == s.host_ref.holder_did) {
                return;
            }
            let calls = st.calls.entry(gid.to_string()).or_default();
            if calls.get(&s.call_id).is_some_and(|(_, have)| have.host_gen >= s.host_gen) {
                return;
            }
            let new = !calls.contains_key(&s.call_id);
            calls.insert(s.call_id.clone(), (blob.clone(), s.clone()));
            new
        };
        self.group_calls_configure();
        if let Err(e) = self.gcall_observe_start(&blob) {
            tracing::debug!("group call {}: {e}", short(gid));
        }
        let join = {
            let mut st = self.group.calls.st.lock();
            let ours = st.pending.as_mut().is_some_and(|p| p.call_id == s.call_id && p.picker.on_started(&s.host_ref));
            if ours {
                st.pending = None;
            }
            ours
        };
        if new_call {
            self.log(format!("group {}: call {} by {} hosted by {}", short(gid), &s.call_id, s.starter_did, s.host_ref.holder_did));
            self.calls_emit(GroupCallEventKind::Announced, gid, &s.call_id, vec![], s.starter_did.clone());
        }
        let hosted_here = self.gcall_status().is_some_and(|st| st.call_id == s.call_id && st.hosting.is_some());
        if join
            && !hosted_here
            && let Err(e) = self.gcall_join(&blob, None)
        {
            self.calls_emit(GroupCallEventKind::StartFailed, gid, &s.call_id, vec![], e.to_string());
        }
    }

    pub(super) fn group_call_ended(self: &Arc<Self>, gid: &str, call_id: &str, end: &SignedBlob) {
        let _ = self.gcall_observe_end(end);
        let had = {
            let mut st = self.group.calls.st.lock();
            if let Ok(e) = w::open_end(end) {
                if st.ends.len() >= SEEN_MAX {
                    st.ends.clear();
                }
                st.ends.insert(call_id.to_string(), (gid.to_string(), e.host_did, end.clone(), now_ms() + CALL_KEEP_MS));
            }
            st.ended.insert(call_id.to_string());
            st.calls.get_mut(gid).and_then(|c| c.remove(call_id)).is_some()
        };
        if had {
            self.calls_emit(GroupCallEventKind::Ended, gid, call_id, vec![], String::new());
        }
    }

    // ---- the asked device ---------------------------------------------------------------------

    pub(super) fn group_call_requested(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, r: HostRequest) {
        self.group_calls_configure();
        let cond = self.gcall.cond();
        let paused = self.group.calls.paused.lock().contains(gid);
        let why: Option<String> = if let Some(why) = decline_reason(paused, self.gcall.busy(), self.gcall.cond_known()) {
            Some(why.into())
        } else {
            let dir = GroupCallDir::new(self);
            let backups: Vec<HostRef> = r.backups.iter().filter(|b| **b != r.host && dir.host_permit_ok(gid, b)).take(MAX_BACKUPS).cloned().collect();
            let cfg = HostConfig {
                gid: gid.to_string(),
                call_id: r.call_id.clone(),
                host_gen: 1,
                host_ref: r.host.clone(),
                starter_did: r.starter_did.clone(),
                backups,
                meta_frontier: self.calls_frontier(core, gid),
                cond,
                own_user_in: false,
            };
            self.gcall_host(cfg).err().map(|e| e.to_string())
        };
        let Some(reason) = why else {
            self.log(format!("group {}: hosting call {} for {}", short(gid), r.call_id, r.starter_did));
            return;
        };
        self.log(format!("group {}: declined to host call {}: {reason}", short(gid), r.call_id));
        let f = HostRefusal {
            v: GCALL_VERSION,
            gid: gid.to_string(),
            call_id: r.call_id.clone(),
            starter_did: r.starter_did.clone(),
            host: r.host.clone(),
            reason: reason.chars().take(100).collect(),
        };
        let blob = w::sign_host_refusal(&core.account, f.clone());
        if f.starter_did == core.did {
            self.group_call_refused(core, &f);
        }
        let sig = blob.signature.clone();
        self.calls_originate(core, gid, &sig, GroupMsg::CallHostRefusal { refusal: blob }, Route::Did(f.starter_did));
    }

    pub(super) fn calls_frontier(&self, core: &GroupCore, gid: &str) -> String {
        let vv = core.groups.lock().get(gid).map(|l| l.doc.vv().encode()).unwrap_or_default();
        blake3::hash(&vv).to_hex()[..32].to_string()
    }
}
