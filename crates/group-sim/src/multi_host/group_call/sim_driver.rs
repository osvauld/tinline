use super::*;

impl Sim {
    pub(super) fn tick(&mut self) {
        let n = self.w.d.len();
        // Members' keep-level datagrams (§1.4) keep H's view of them fresh.
        for m in 0..n {
            let Some(j) = self.c[m].joined.clone() else { continue };
            let Ok(h) = self.w.dial(m, &j.host_ep) else { continue };
            if h == m {
                continue;
            }
            let now = self.now;
            if let Some(hc) = self.c[h].hosting.as_mut().filter(|hc| hc.call_id == j.call_id && hc.host_gen == j.host_gen)
            {
                if let Some(p) = hc.parts.values_mut().find(|p| p.endpoint == j.my_ep) {
                    p.last_ms = now;
                    self.w.send(m, h, j.host_ep.clone(), "keep-level", vec![LEVEL_ONLY, 127]);
                }
            }
        }
        // Heartbeats from every online host.
        for h in 0..n {
            let Some(hc) = self.c[h].hosting.as_ref() else { continue };
            if !self.w.d[h].online {
                continue;
            }
            let hb = serde_json::to_vec(&Heartbeat { epoch: hc.epoch }).unwrap();
            let (cid, hg) = (hc.call_id.clone(), hc.host_gen);
            let me = self.host_ref(h);
            let eps: Vec<String> = hc.parts.values().map(|p| p.endpoint.clone()).collect();
            for ep in eps {
                let Ok(dev) = self.w.dial(h, &ep) else { continue };
                if dev == h {
                    continue;
                }
                self.w.send(h, dev, ep, "heartbeat", hb.clone());
                let now = self.now;
                if let Some(j) = self.c[dev].joined.as_mut().filter(|j| j.call_id == cid && j.host_gen == hg && j.host == me)
                {
                    j.last_hb_ms = now;
                }
            }
        }
        // Member drop at H (§5): 5 s without control or datagrams.
        for h in 0..n {
            let now = self.now;
            let Some(hc) = self.c[h].hosting.as_mut() else { continue };
            let own = self.w.d[h].id.did().to_string();
            let gone: Vec<String> = hc
                .parts
                .iter()
                .filter(|(d, p)| **d != own && p.last_ms + MEMBER_DROP_MS <= now)
                .map(|(d, _)| d.clone())
                .collect();
            if !gone.is_empty() {
                for d in gone {
                    hc.parts.remove(&d);
                }
                self.push_roster(h);
            }
        }
        // Host loss at members: 3 missed heartbeats.
        for m in 0..n {
            let Some(j) = self.c[m].joined.clone() else { continue };
            if self.dev_of(&j.host) == m || !self.w.d[m].online {
                continue;
            }
            if self.now >= j.last_hb_ms + HEARTBEAT_MISSES * HEARTBEAT_MS {
                self.c[m].joined = None;
                self.c[m].last_call = Some(j.call_id.clone());
                self.c[m].log.push(format!("host lost at {}; Reconnecting…", self.now));
                let r = self.redial_after_loss(m, &j, self.now);
                self.redials.push((m, r));
            }
        }
    }
    // §5: try backups[0], then backups[1] after 5 s, skipping revoked permits; give up after both.
    pub(super) fn redial_after_loss(&mut self, m: usize, j: &Joined, lost_at: u64) -> Redial {
        let start = self.c[m]
            .prev
            .get(&j.call_id)
            .filter(|s| s.msg.host_gen == j.host_gen)
            .or_else(|| self.c[m].starts.get(&j.call_id).filter(|s| s.msg.host_gen == j.host_gen))
            .cloned();
        let backups: Vec<HostRef> = start
            .and_then(|s| s.start().map(|(_, b)| b.clone()))
            .unwrap_or_default()
            .into_iter()
            .filter(|b| !self.c[m].revoked.contains(&b.permit_digest))
            .collect();
        let mut t = lost_at;
        for b in backups.iter() {
            if self.join_call(m, &j.call_id, b, j.host_gen + 1).is_ok() {
                self.c[m].log.push(format!("redial ok at {t}"));
                return Redial::Joined {
                    host: self.dev_of(b),
                    at_ms: t,
                };
            }
            t += ANSWER_TIMEOUT_MS;
        }
        self.c[m].log.push("Call ended — host lost".into());
        Redial::HostLost { at_ms: t }
    }

    // ---- media ----
    pub(super) fn host_ingress(&mut self, h: usize, conn_ep: &str, bytes: &[u8]) -> Result<(), CR> {
        let now = self.now;
        let hc = self.c[h].hosting.as_mut().ok_or(CR::Unknown)?;
        let Some((did, p)) = hc.parts.iter_mut().find(|(_, p)| p.endpoint == conn_ep) else {
            return Err(CR::NotParticipant);
        };
        let did = did.clone();
        let drop = if bytes.len() > MAX_DATAGRAM {
            Some(CR::Oversize)
        } else if !((bytes.len() >= 6 && bytes[0] == UP) || (bytes.len() == 2 && bytes[0] == LEVEL_ONLY)) {
            Some(CR::Malformed)
        } else {
            let b = hc.buckets.entry(did.clone()).or_insert((UP_BURST * 1000, now));
            b.0 = (b.0 + (now - b.1) * MAX_UP_PPS).min(UP_BURST * 1000);
            b.1 = now;
            if b.0 < 1000 {
                Some(CR::RateLimited)
            } else {
                b.0 -= 1000;
                None
            }
        };
        if let Some(e) = drop {
            *hc.dropped.entry(did).or_default() += 1;
            return Err(e);
        }
        p.last_ms = now;
        let slot = p.slot;
        if bytes[0] == LEVEL_ONLY {
            hc.levels.insert(slot, bytes[1]);
            hc.level_only_rx += 1;
        } else {
            hc.levels.insert(slot, bytes[5]);
            hc.pending.entry(slot).or_default().push(bytes[1..].to_vec());
        }
        Ok(())
    }
    pub(super) fn upstream(&mut self, m: usize, bytes: Vec<u8>) -> Result<(), CR> {
        let j = self.c[m].joined.clone().ok_or(CR::Unknown)?;
        let h = self.w.dial(m, &j.host_ep).map_err(|_| CR::Unreachable)?;
        if h != m {
            self.w.send(m, h, j.host_ep.clone(), "dgram", bytes.clone());
        }
        self.host_ingress(h, &j.my_ep, &bytes)
    }
    pub(super) fn speak(&mut self, m: usize, level: u8) -> Result<(), CR> {
        self.c[m].seq += 1;
        let mut d = vec![UP];
        d.extend(audio(self.c[m].seq, level));
        self.upstream(m, d)
    }
    // G6-D1 sender silence gate. `seq` keeps counting one per frame. After GATE_FRAMES quiet
    // frames the sender stops sending Opus and sends `[0x03][level]` every LEVEL_ONLY_FRAMES.
    pub(super) fn speak_gated(&mut self, m: usize, level: u8) -> Result<(), CR> {
        let c = &mut self.c[m];
        c.seq += 1;
        if (level as u32) < SPEECH_DBOV {
            c.quiet = 0;
            c.gated = false;
            c.since_level = 0;
        } else {
            c.quiet += 1;
            if c.quiet > GATE_FRAMES {
                c.gated = true;
            }
        }
        if !c.gated {
            c.opus_sent += 1;
            let mut d = vec![UP];
            d.extend(audio(c.seq, level));
            return self.upstream(m, d);
        }
        c.since_level += 1;
        if c.since_level >= LEVEL_ONLY_FRAMES {
            c.since_level = 0;
            return self.upstream(m, vec![LEVEL_ONLY, level]);
        }
        Ok(())
    }
    pub(super) fn accept_active(&mut self, i: usize, bytes: &[u8]) -> Result<(), CR> {
        let a: Active = serde_json::from_slice(bytes).map_err(|_| CR::Malformed)?;
        let j = self.c[i].joined.as_mut().ok_or(CR::Unknown)?;
        if a.r#gen <= j.active_gen {
            return Err(CR::Stale);
        }
        j.active_gen = a.r#gen;
        j.active = a.slots.into_iter().collect();
        Ok(())
    }
    // End of a 20 ms frame at host h: run the forwarder and send the copies.
    pub(super) fn host_frame(&mut self, h: usize) -> BTreeSet<u8> {
        let Some(hc) = self.c[h].hosting.as_mut() else {
            return BTreeSet::new();
        };
        let pending = std::mem::take(&mut hc.pending);
        let levels = std::mem::take(&mut hc.levels);
        hc.fwd = select(&hc.fwd, &levels);
        let set = hc.fwd.forwarded(); // G6-D2: S plus the provisional slot
        let parts: Vec<(u8, String)> = hc.parts.values().map(|p| (p.slot, p.endpoint.clone())).collect();
        if set != hc.active {
            // G6-D1: tell every participant before forwarding under the new set.
            hc.active_gen += 1;
            hc.active = set.clone();
            let msg = serde_json::to_vec(&Active {
                r#gen: hc.active_gen,
                slots: set.iter().copied().collect(),
            })
            .unwrap();
            for (_, ep) in &parts {
                let Ok(dev) = self.w.dial(h, ep) else { continue };
                if dev != h {
                    self.w.send(h, dev, ep.clone(), "active", msg.clone());
                }
                let _ = self.accept_active(dev, &msg);
            }
        }
        for (slot, ds) in pending {
            if !set.contains(&slot) {
                continue;
            }
            for (to_slot, ep) in &parts {
                if *to_slot == slot {
                    continue; // no self-echo
                }
                let Ok(dev) = self.w.dial(h, ep) else { continue };
                for d in &ds {
                    let mut down = vec![DOWN, slot];
                    down.extend(d);
                    if dev != h {
                        self.w.send(h, dev, ep.clone(), "dgram", down.clone());
                    }
                    self.client_rx(dev, &down);
                }
            }
        }
        set
    }
    pub(super) fn client_rx(&mut self, i: usize, bytes: &[u8]) {
        let muted = self.c[i].muted.clone();
        let Some(j) = self.c[i].joined.as_mut() else { return };
        if bytes.len() < 2 || bytes[0] != DOWN {
            j.dropped += 1;
            return;
        }
        let slot = bytes[1];
        let Some(did) = j.roster.get(&slot).cloned() else {
            j.dropped += 1; // §6: datagrams from slots not in the Roster are dropped
            return;
        };
        *j.heard.entry(slot).or_default() += 1;
        if !muted.contains(&did) {
            *j.mixed.entry(did).or_default() += 1;
        }
    }
    // One frame in which each listed device speaks once at `level`.
    pub(super) fn frame(&mut self, h: usize, speakers: &[(usize, u8)]) -> BTreeSet<u8> {
        self.now += FRAME_MS;
        for &(m, l) in speakers {
            let _ = self.speak(m, l);
        }
        self.host_frame(h)
    }

    // ---- U9 removal (reused multi_host flags) ----
    pub(super) fn remove_member(&mut self, target: usize, to: &[usize]) {
        let t = self.did(target);
        let ops: Vec<SignedOp> = [OLGA, A1, BOB]
            .iter()
            .map(|&a| self.w.op(a, Op::Flag { target: t.clone() }))
            .collect();
        for &i in to {
            for o in &ops {
                let e = self.w.gep(i);
                self.w.send(H2, i, e, "op", o.wire());
                let _ = self.w.accept_op(i, &o.wire());
            }
        }
        for &i in to {
            self.host_sweep(i);
        }
    }
    // An honest H drops a participant who is no longer a member at once (§3.2 item 2).
    pub(super) fn host_sweep(&mut self, h: usize) {
        let Some(hc) = self.c[h].hosting.as_ref() else { return };
        let gone: Vec<String> = hc
            .parts
            .keys()
            .filter(|d| !self.is_member(h, d))
            .cloned()
            .collect();
        for d in gone {
            self.host_end(h, EndReason::Removed, None, Some(&d));
        }
    }

    // ---- U12 host rotation while hosting (§5, Q12) ----
    pub(super) fn rotation_tick(&mut self, h: usize) -> &'static str {
        let Some(due) = self.c[h].rotation_due_ms else {
            return "none";
        };
        if self.now < due {
            return "not due";
        }
        if self.c[h].hosting.is_some() {
            if self.now < due + ROTATION_DEFER_CAP_MS {
                return "deferred";
            }
            // Cap reached: rotate, push the new records, then end and redial (next_host = self).
            self.w.rotate_host(h);
            self.push_call_record(h);
            self.c[h].rotation_due_ms = None;
            let me = self.host_ref(h);
            self.host_end(h, EndReason::HostRotating, Some(me), None);
            return "rotated with End{host_rotating}";
        }
        self.w.rotate_host(h);
        self.push_call_record(h);
        self.c[h].rotation_due_ms = None;
        "rotated"
    }
}

pub(super) fn base() -> (Sim, String) {
    let mut s = Sim::new();
    let b = vec![s.host_ref(BOB), s.host_ref(OLGA)];
    let cid = s.start_call(H1, OLGA, b).unwrap();
    (s, cid)
}
pub(super) fn roster_of(s: &Sim, h: usize) -> BTreeMap<u8, String> {
    s.c[h]
        .hosting
        .as_ref()
        .unwrap()
        .parts
        .iter()
        .map(|(d, p)| (p.slot, d.clone()))
        .collect()
}
pub(super) fn assert_unchanged(s: &Sim, before: &[Vec<u8>], case: &str) {
    for (i, b) in before.iter().enumerate() {
        assert!(*b == s.snap(i), "{case}: device {i} state changed");
    }
}
