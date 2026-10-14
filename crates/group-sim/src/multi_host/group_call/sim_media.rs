use super::*;

impl Sim {
    // Member side: dial the host from the group endpoint and send a Join.
    pub(super) fn join_call(
        &mut self,
        m: usize,
        call_id: &str,
        host: &HostRef,
        hgen: u32,
    ) -> Result<u8, CR> {
        let rec = self.c[m]
            .records
            .get(&dir_key(&host.holder_did, host.device_slot))
            .filter(|r| r.permit.digest() == host.permit_digest)
            .cloned()
            .ok_or(CR::Unknown)?;
        let ep = rec.note.claims.endpoint.clone();
        let j = self.w.dial(m, &ep).map_err(|_| CR::Unreachable)?;
        let bytes = self.join_req(m, call_id, hgen, self.now);
        let before = self.c[m].dialing.replace(Dialing {
            call_id: call_id.into(),
            host: host.clone(),
            host_ep: ep.clone(),
            host_gen: hgen,
        });
        if j != m {
            self.w.send(m, j, ep, "join", bytes.clone());
        }
        let my_ep = self.w.gep(m);
        let r = self.host_join(j, &my_ep, &bytes);
        if r.is_err() {
            self.c[m].dialing = before;
        }
        r
    }
    pub(super) fn join_shown(&mut self, m: usize) -> Result<u8, CR> {
        let call_id = self.c[m].shown.clone().ok_or(CR::Unknown)?;
        let st = self.c[m].starts[&call_id].clone();
        let (host, _) = st.start().unwrap();
        self.join_call(m, &call_id, &host.clone(), st.msg.host_gen)
    }
    pub(super) fn ctl_rate_ok(&self, h: usize, did: &str) -> bool {
        let now = self.now;
        let hc = self.c[h].hosting.as_ref().unwrap();
        hc.ctl_times
            .get(did)
            .map_or(0, |q| q.iter().filter(|t| **t + 1000 > now).count())
            < MAX_CONTROL_PER_S
    }
    pub(super) fn host_join(&mut self, h: usize, conn_ep: &str, bytes: &[u8]) -> Result<u8, CR> {
        if bytes.len() > MAX_CONTROL {
            return Err(CR::Oversize);
        }
        let req: JoinReq = serde_json::from_slice(bytes).map_err(|_| CR::Malformed)?;
        let (j, n) = (&req.join, &req.note);
        let m = &j.msg;
        let Kind::Join { issued_ms, .. } = m.kind else {
            return Err(CR::Malformed);
        };
        if m.v != 1 {
            return Err(CR::Malformed);
        }
        if m.gid != self.w.gid {
            return Err(CR::WrongGroup);
        }
        if !j.verify() {
            return Err(CR::BadSignature);
        }
        let seen_gen = self.c[h].hgen.get(&m.call_id).copied().unwrap_or(0);
        if m.host_gen < seen_gen {
            return Err(CR::StaleGen);
        }
        let takeover = match &self.c[h].hosting {
            Some(hc) if hc.call_id == m.call_id => {
                if m.host_gen != hc.host_gen {
                    return Err(CR::StaleGen);
                }
                false
            }
            Some(_) => return Err(CR::WrongCall),
            None => match self.c[h].may_take.get(&m.call_id) {
                Some(&g) if g == m.host_gen => true,
                _ => return Err(CR::WrongCall),
            },
        };
        if !self.is_member(h, &m.signer) {
            return Err(CR::NotMember);
        }
        if !n.verify() || n.claims.owner != m.signer {
            return Err(CR::BadSignature);
        }
        if n.claims.audience != Audience::Group(self.w.gid.clone()) {
            return Err(CR::WrongGroup);
        }
        if n.claims.endpoint != conn_ep {
            return Err(CR::EndpointMismatch);
        }
        let dk = dir_key(&n.claims.owner, n.claims.slot);
        if let Some(cur) = self.w.d[h]
            .host
            .as_ref()
            .and_then(|hs| hs.directory.get(&dk))
        {
            if cur.claims.seq > n.claims.seq {
                return Err(CR::Stale);
            }
        }
        let did = m.signer.clone();
        if let Some(hc) = self.c[h].hosting.as_ref() {
            if hc.seen.contains(&j.id()) {
                return Err(CR::Replay);
            }
            if hc
                .last_join
                .get(&did)
                .is_some_and(|t| t + JOIN_MIN_GAP_MS > self.now)
            {
                return Err(CR::RateLimited);
            }
            if !self.ctl_rate_ok(h, &did) {
                return Err(CR::RateLimited);
            }
            let hc = self.c[h].hosting.as_ref().unwrap();
            match hc.parts.get(&did) {
                Some(p) => {
                    let Kind::Join { issued_ms: old, .. } = p.join.msg.kind else {
                        unreachable!()
                    };
                    if old >= issued_ms {
                        return Err(CR::Stale); // ASSUMPTION(G5)
                    }
                }
                None => {
                    if hc.limited {
                        return Err(CR::Limited);
                    }
                    let cap = hc.max.min(self.c[h].cond.cap());
                    if hc.parts.len() >= cap {
                        return Err(CR::Full);
                    }
                }
            }
        }
        // ---- all checks passed; mutate ----
        if takeover {
            let prev = self.c[h].starts.get(&m.call_id).cloned();
            let backups: Vec<HostRef> = prev
                .as_ref()
                .and_then(|p| p.start().map(|(_, b)| b.clone()))
                .unwrap_or_default()
                .into_iter()
                .filter(|b| Some(b) != self.host_ref_of(h).as_ref()) // ASSUMPTION(G9)
                .collect();
            let starter = (0..self.w.d.len()).find(|&i| self.did(i) == did).unwrap();
            self.begin_hosting(h, &m.call_id, m.host_gen, starter, backups)?;
        }
        let now = self.now;
        let hc = self.c[h].hosting.as_mut().unwrap();
        hc.seen.insert(j.id());
        hc.last_join.insert(did.clone(), now);
        let q = hc.ctl_times.entry(did.clone()).or_default();
        q.push_back(now);
        while q.front().is_some_and(|t| *t + 1000 <= now) {
            q.pop_front();
        }
        let old = hc.parts.remove(&did);
        let used: BTreeSet<u8> = hc.parts.values().map(|p| p.slot).collect();
        let slot = (0..=u8::MAX).find(|s| !used.contains(s)).unwrap();
        hc.parts.insert(
            did,
            Part {
                slot,
                join: j.clone(),
                endpoint: conn_ep.into(),
                last_ms: now,
            },
        );
        if let Some(old) = old {
            // §3.2 item 5: the older device's connection closes ("answered elsewhere").
            if old.endpoint != conn_ep {
                if let Some(&od) = self.w.live.get(&old.endpoint) {
                    let e = old.endpoint.clone();
                    self.w
                        .send(h, od, e, "close", b"answered-elsewhere".to_vec());
                    if let Some(jd) = self.c[od].joined.take() {
                        self.c[od].last_call = Some(jd.call_id);
                    }
                    self.c[od].log.push("call answered elsewhere".into());
                }
            }
        }
        self.push_roster(h);
        Ok(slot)
    }
    pub(super) fn roster_ctl(&mut self, h: usize, skip: Option<&str>) -> Ctl {
        let hc = self.c[h].hosting.as_mut().unwrap();
        hc.epoch += 1;
        let mut entries: Vec<RosterEntry> = hc
            .parts
            .iter()
            .filter(|(d, _)| Some(d.as_str()) != skip)
            .map(|(d, p)| RosterEntry {
                slot: p.slot,
                did: d.clone(),
                join: p.join.clone(),
            })
            .collect();
        entries.sort_by_key(|e| e.slot);
        let (cid, hgen, epoch) = (hc.call_id.clone(), hc.host_gen, hc.epoch);
        self.msg(h, &cid, hgen, Kind::Roster { epoch, entries })
    }
    pub(super) fn push_roster(&mut self, h: usize) {
        let r = self.roster_ctl(h, None);
        self.deliver_roster(h, &r);
    }
    pub(super) fn deliver_roster(&mut self, h: usize, r: &Ctl) -> Vec<(usize, Result<(), CR>)> {
        let bytes = r.wire();
        let eps: Vec<String> = self.c[h]
            .hosting
            .as_ref()
            .unwrap()
            .parts
            .values()
            .map(|p| p.endpoint.clone())
            .collect();
        let mut out = vec![];
        for ep in eps {
            let Ok(dev) = self.w.dial(h, &ep) else {
                continue;
            };
            if dev != h {
                self.w.send(h, dev, ep, "roster", bytes.clone());
            }
            out.push((dev, self.accept_roster(dev, &bytes)));
        }
        out
    }
    pub(super) fn accept_roster(&mut self, i: usize, bytes: &[u8]) -> Result<(), CR> {
        if bytes.len() > MAX_CONTROL {
            return Err(CR::Oversize);
        }
        let r: Ctl = serde_json::from_slice(bytes).map_err(|_| CR::Malformed)?;
        let m = &r.msg;
        let Kind::Roster { epoch, entries } = &m.kind else {
            return Err(CR::Malformed);
        };
        if m.gid != self.w.gid {
            return Err(CR::WrongGroup);
        }
        if !r.verify() {
            return Err(CR::BadSignature);
        }
        let (call_id, host, hgen) = match (&self.c[i].joined, &self.c[i].dialing) {
            (Some(j), _) => (j.call_id.clone(), j.host.clone(), j.host_gen),
            (None, Some(d)) => (d.call_id.clone(), d.host.clone(), d.host_gen),
            _ => return Err(CR::Unknown),
        };
        if m.call_id != call_id {
            return Err(CR::WrongCall);
        }
        if m.host_gen < hgen || m.host_gen < self.c[i].hgen.get(&call_id).copied().unwrap_or(0) {
            return Err(CR::StaleGen);
        }
        if m.host_gen != hgen {
            return Err(CR::WrongCall);
        }
        if m.signer != host.holder_did {
            return Err(CR::NotHost);
        }
        if self.c[i].revoked.contains(&host.permit_digest) {
            return Err(CR::Revoked);
        }
        if self.c[i].joined.as_ref().is_some_and(|j| *epoch <= j.epoch) {
            return Err(CR::Stale);
        }
        let mut slots = BTreeSet::new();
        let mut dids = BTreeSet::new();
        for e in entries {
            let jm = &e.join.msg;
            if !e.join.verify()
                || jm.signer != e.did
                || jm.call_id != call_id
                || jm.host_gen != hgen
                || jm.gid != self.w.gid
                || !matches!(jm.kind, Kind::Join { .. })
            {
                return Err(CR::BadSignature);
            }
            if !self.is_member(i, &e.did) {
                return Err(CR::NotMember); // ASSUMPTION(G10)
            }
            if !slots.insert(e.slot) || !dids.insert(e.did.clone()) {
                return Err(CR::Malformed);
            }
        }
        // ---- mutate ----
        let me = self.did(i);
        let roster: BTreeMap<u8, String> =
            entries.iter().map(|e| (e.slot, e.did.clone())).collect();
        if !dids.contains(&me) {
            if let Some(j) = self.c[i].joined.take() {
                self.c[i].last_call = Some(j.call_id);
                self.c[i].log.push("dropped from roster".into());
            }
            return Ok(());
        }
        if self.c[i].joined.is_none() {
            let d = self.c[i].dialing.take().unwrap();
            self.c[i].joined = Some(Joined {
                call_id: d.call_id,
                host: d.host,
                host_ep: d.host_ep,
                host_gen: d.host_gen,
                my_ep: self.w.gep(i),
                epoch: 0,
                roster: BTreeMap::new(),
                last_hb_ms: self.now,
                heard: BTreeMap::new(),
                mixed: BTreeMap::new(),
                dropped: 0,
                active_gen: 0,
                active: BTreeSet::new(),
            });
        }
        // §3.4: the blocker's own app mutes DIDs it has blocked. H is not told.
        let blocked: Vec<String> = roster
            .values()
            .filter(|d| self.w.d[i].blocked.contains_key(*d))
            .cloned()
            .collect();
        for b in blocked {
            if self.c[i].muted.insert(b) {
                self.c[i].log.push("1 blocked participant".into());
            }
        }
        let j = self.c[i].joined.as_mut().unwrap();
        j.epoch = *epoch;
        j.roster = roster;
        Ok(())
    }

    // ---- Leave / End ----
    pub(super) fn leave(&mut self, m: usize) -> Result<(), CR> {
        let j = self.c[m].joined.clone().ok_or(CR::Unknown)?;
        let ctl = self.msg(
            m,
            &j.call_id,
            j.host_gen,
            Kind::Leave {
                issued_ms: self.now,
            },
        );
        let h = self.dev_of(&j.host);
        if h != m {
            self.w.send(m, h, j.host_ep.clone(), "leave", ctl.wire());
        }
        self.host_leave(h, &j.my_ep, &ctl.wire())?;
        self.c[m].joined = None;
        self.c[m].last_call = Some(j.call_id);
        Ok(())
    }
    pub(super) fn host_leave(&mut self, h: usize, conn_ep: &str, bytes: &[u8]) -> Result<(), CR> {
        let ctl: Ctl = serde_json::from_slice(bytes).map_err(|_| CR::Malformed)?;
        if !ctl.verify() || !matches!(ctl.msg.kind, Kind::Leave { .. }) {
            return Err(CR::BadSignature);
        }
        let hc = self.c[h].hosting.as_ref().ok_or(CR::Unknown)?;
        if ctl.msg.call_id != hc.call_id || ctl.msg.host_gen != hc.host_gen {
            return Err(CR::WrongCall);
        }
        if hc
            .parts
            .get(&ctl.msg.signer)
            .is_none_or(|p| p.endpoint != conn_ep)
        {
            return Err(CR::NotParticipant);
        }
        let own = ctl.msg.signer == self.did(h);
        self.c[h]
            .hosting
            .as_mut()
            .unwrap()
            .parts
            .remove(&ctl.msg.signer);
        if own && self.c[h].cond.class == Class::Phone {
            // §2.4 / Q11: a phone hosts only while its own user is in the call.
            let next = self.next_backup(h);
            self.host_end(h, EndReason::HostLeaving, next, None);
            return Ok(());
        }
        self.push_roster(h);
        Ok(())
    }
    pub(super) fn next_backup(&self, h: usize) -> Option<HostRef> {
        let hc = self.c[h].hosting.as_ref()?;
        let (_, b) = hc.start.start()?;
        b.iter()
            .find(|r| !self.c[h].revoked.contains(&r.permit_digest))
            .cloned()
    }
    // `only`: send End to that DID alone and keep hosting the others (§3.2 item 2).
    pub(super) fn host_end(
        &mut self,
        h: usize,
        reason: EndReason,
        next: Option<HostRef>,
        only: Option<&str>,
    ) {
        let hc = self.c[h].hosting.as_ref().unwrap();
        let (cid, hgen) = (hc.call_id.clone(), hc.host_gen);
        let targets: Vec<(String, String)> = hc
            .parts
            .iter()
            .filter(|(d, _)| only.is_none_or(|o| o == d.as_str()))
            .map(|(d, p)| (d.clone(), p.endpoint.clone()))
            .collect();
        let end = self.msg(
            h,
            &cid,
            hgen,
            Kind::End {
                reason,
                next_host: next.clone(),
            },
        );
        let bytes = end.wire();
        match only {
            Some(o) => {
                self.c[h].hosting.as_mut().unwrap().parts.remove(o);
            }
            None => {
                self.c[h].hosting = None;
                if next.as_ref() == self.host_ref_of(h).as_ref() {
                    self.c[h].may_take.insert(cid.clone(), hgen + 1);
                }
            }
        }
        for (_, ep) in targets {
            let Ok(dev) = self.w.dial(h, &ep) else {
                continue;
            };
            if dev != h {
                self.w.send(h, dev, ep, "end", bytes.clone());
            }
            let _ = self.accept_end(dev, &bytes);
        }
        if only.is_some() {
            self.push_roster(h);
        } else if reason == EndReason::Ended {
            for i in self.relay_targets() {
                let e = self.w.gep(i);
                self.w.send(H2, i, e, "call-end", bytes.clone());
                let _ = self.accept_end(i, &bytes);
            }
        }
        self.run_queue();
    }
    pub(super) fn accept_end(&mut self, i: usize, bytes: &[u8]) -> Result<(), CR> {
        let e: Ctl = serde_json::from_slice(bytes).map_err(|_| CR::Malformed)?;
        let m = &e.msg;
        let Kind::End { reason, next_host } = &m.kind else {
            return Err(CR::Malformed);
        };
        if m.gid != self.w.gid {
            return Err(CR::WrongGroup);
        }
        if !e.verify() {
            return Err(CR::BadSignature);
        }
        let cur = self.c[i].hgen.get(&m.call_id).copied().unwrap_or(0);
        if m.host_gen < cur {
            return Err(CR::StaleGen);
        }
        let joined = self.c[i].joined.clone().filter(|j| j.call_id == m.call_id);
        let host = match (&joined, self.c[i].starts.get(&m.call_id)) {
            (Some(j), _) => {
                if j.host_gen != m.host_gen {
                    return Err(CR::StaleGen);
                }
                j.host.clone()
            }
            (None, Some(s)) if s.msg.host_gen == m.host_gen => s.start().unwrap().0.clone(),
            _ => return Err(CR::Unknown),
        };
        if host.holder_did != m.signer {
            return Err(CR::NotHost);
        }
        if self.c[i].revoked.contains(&host.permit_digest) {
            return Err(CR::Revoked);
        }
        // ---- mutate ----
        let call_id = m.call_id.clone();
        if *reason == EndReason::Ended {
            self.c[i].starts.remove(&call_id);
            self.c[i].shown = self.c[i]
                .starts
                .keys()
                .min_by_key(|c| call_hash(c))
                .cloned();
        }
        let Some(j) = joined else { return Ok(()) };
        self.c[i].joined = None;
        self.c[i].last_call = Some(call_id.clone());
        self.c[i].log.push(format!("end:{reason:?}"));
        if let (Some(nh), true) = (
            next_host.clone(),
            matches!(
                reason,
                EndReason::HostLeaving | EndReason::HostRotating | EndReason::Superseded
            ),
        ) {
            // Redial at once; no heartbeat wait (§5 "planned host exit").
            let target = if *reason == EndReason::Superseded {
                self.c[i]
                    .starts
                    .iter()
                    .find(|(_, s)| s.start().is_some_and(|(r, _)| *r == nh))
                    .map(|(c, s)| (c.clone(), s.msg.host_gen))
            } else {
                Some((call_id, j.host_gen + 1))
            };
            if let Some((c, g)) = target {
                // Queued so that every participant sees this End before anyone redials.
                self.queue.push((i, c, nh, g, j));
            }
        }
        Ok(())
    }
    pub(super) fn run_queue(&mut self) {
        for (i, c, nh, g, j) in std::mem::take(&mut self.queue) {
            let r = match self.join_call(i, &c, &nh, g) {
                Ok(_) => Redial::Joined {
                    host: self.dev_of(&nh),
                    at_ms: self.now,
                },
                Err(_) => self.redial_after_loss(i, &j, self.now),
            };
            self.redials.push((i, r));
        }
    }

    // ---- time, heartbeats, loss and redial (§5) ----
    pub(super) fn advance(&mut self, ms: u64) {
        let end = self.now + ms;
        while self.now + HEARTBEAT_MS <= end {
            self.now += HEARTBEAT_MS;
            self.tick();
        }
        self.now = end;
    }
}
