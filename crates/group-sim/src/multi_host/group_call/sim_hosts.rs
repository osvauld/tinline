use super::*;

impl Sim {
    pub(super) fn new() -> Self {
        let w = World::new(Cfg::default());
        let n = w.d.len();
        let mut s = Sim {
            c: vec![CallDev::default(); n],
            w,
            permits: BTreeMap::new(),
            now: 1_000_000,
            hold_relay: false,
            held: vec![],
            redials: vec![],
            queue: vec![],
            ever_hosted: BTreeSet::new(),
        };
        s.c[HP].cond = Cond::phone(80, false);
        for h in CALL_HOSTS {
            let slot = s.w.d[h].slot;
            let p = s.call_permit(OLGA, h, slot);
            s.permits.insert(h, p);
            s.c[h].consent = true;
        }
        for h in CALL_HOSTS {
            s.publish_call_record(h);
        }
        s
    }
    pub(super) fn did(&self, i: usize) -> String {
        self.w.did(i)
    }
    pub(super) fn call_permit(&self, issuer: usize, h: usize, slot: u64) -> MGrant {
        let parent = self.w.grants.get(&self.did(issuer)).cloned();
        MGrant::issue(
            &self.w.d[issuer].id,
            &self.w.gid,
            &self.did(h),
            Some(slot),
            &[Cap::CallHost],
            false,
            50 + h as u64,
            parent,
        )
    }
    pub(super) fn record(&self, h: usize) -> HostRecord {
        HostRecord {
            note: self.w.d[h].my_note.clone(),
            permit: self.permits[&h].clone(),
        }
    }
    pub(super) fn host_ref(&self, h: usize) -> HostRef {
        HostRef {
            holder_did: self.did(h),
            device_slot: self.w.d[h].slot,
            permit_digest: self.permits.get(&h).map(|p| p.digest()).unwrap_or_default(),
        }
    }
    pub(super) fn dev_of(&self, r: &HostRef) -> usize {
        (0..self.w.d.len())
            .find(|&i| self.did(i) == r.holder_did && self.w.d[i].slot == r.device_slot)
            .unwrap()
    }
    pub(super) fn snap(&self, i: usize) -> Vec<u8> {
        let mut v = self.w.snap(i);
        v.extend(serde_json::to_vec(&self.c[i]).unwrap());
        v
    }
    pub(super) fn snap_all(&self) -> Vec<Vec<u8>> {
        (0..self.w.d.len()).map(|i| self.snap(i)).collect()
    }
    pub(super) fn is_member(&self, i: usize, did: &str) -> bool {
        let d = &self.w.d[i];
        d.board.roster.contains(did) && (self.w.rogue.contains(&i) || !d.view.removed.contains(did))
    }
    pub(super) fn msg(&self, i: usize, call_id: &str, host_gen: u32, kind: Kind) -> Ctl {
        let m = CallMsg {
            v: 1,
            gid: self.w.gid.clone(),
            call_id: call_id.into(),
            signer: self.did(i),
            host_gen,
            kind,
        };
        Ctl::sign(&self.w.d[i].id, m)
    }
    pub(super) fn new_call_id(&mut self) -> String {
        hex32(&self.w.rng.bytes32()[..16])
    }

    // ---- call-host records ----
    pub(super) fn publish_call_record(&mut self, h: usize) {
        let bytes = self.record(h).wire();
        for i in 0..self.w.d.len() {
            if i != h {
                let e = self.w.gep(i);
                self.w.send(h, i, e, "call-host-record", bytes.clone());
            }
            let _ = self.accept_call_record(i, &bytes);
        }
    }
    // After an endpoint rotation the host pushes its new call record itself. It dials each
    // member at the endpoint in its chat directory (multi-host §9 Q6).
    pub(super) fn push_call_record(&mut self, h: usize) -> Vec<usize> {
        let bytes = self.record(h).wire();
        let _ = self.accept_call_record(h, &bytes);
        let dir: Vec<EpNote> = self.w.d[h]
            .host
            .as_ref()
            .map(|hs| hs.directory.values().cloned().collect())
            .unwrap_or_default();
        let mut reached = vec![];
        for n in dir {
            if let Ok(j) = self.w.dial(h, &n.claims.endpoint) {
                if self.did(j) == n.claims.owner {
                    self.w.send(h, j, n.claims.endpoint.clone(), "call-host-record", bytes.clone());
                    if self.accept_call_record(j, &bytes).is_ok() {
                        reached.push(j);
                    }
                }
            }
        }
        reached
    }
    pub(super) fn check_call_record(&self, i: usize, r: &HostRecord) -> Result<(), CR> {
        if !r.note.verify() {
            return Err(CR::BadSignature);
        }
        if r.note.claims.audience != Audience::Group(self.w.gid.clone()) {
            return Err(CR::WrongGroup);
        }
        if !r.permit.allows(&self.w.g, &r.note.claims.owner, Cap::CallHost) {
            return Err(CR::PermitInvalid);
        }
        if r.permit.body.device != Some(r.note.claims.slot) {
            return Err(CR::DeviceMismatch);
        }
        if self.c[i].revoked.contains(&r.permit.digest()) {
            return Err(CR::Revoked);
        }
        if !self.is_member(i, &r.note.claims.owner) {
            return Err(CR::NotMember);
        }
        Ok(())
    }
    pub(super) fn accept_call_record(&mut self, i: usize, bytes: &[u8]) -> Result<(), CR> {
        let r: HostRecord = serde_json::from_slice(bytes).map_err(|_| CR::Malformed)?;
        self.check_call_record(i, &r)?;
        let key = dir_key(&r.note.claims.owner, r.note.claims.slot);
        if let Some(cur) = self.c[i].records.get(&key) {
            if cur.note.claims.seq >= r.note.claims.seq && cur.permit == r.permit {
                return Err(CR::Stale);
            }
        }
        self.c[i].records.insert(key, r);
        Ok(())
    }
    // Admin-signed `RemoveHost{permit digest}` aimed at a call-host permit (§5 last row).
    pub(super) fn revoke_op(&mut self, admin: usize, h: usize) -> Vec<u8> {
        let d = self.permits[&h].digest();
        self.w.op(admin, Op::RemoveHost { permit: d }).wire()
    }
    pub(super) fn accept_revoke(&mut self, i: usize, bytes: &[u8]) -> Result<(), CR> {
        let op: SignedOp = serde_json::from_slice(bytes).map_err(|_| CR::Malformed)?;
        if !op.verify() {
            return Err(CR::BadSignature);
        }
        if op.body.group != self.w.gid {
            return Err(CR::WrongGroup);
        }
        let issuer = &op.body.issuer;
        let admin = *issuer == self.w.g.body.creator
            || op
                .body
                .issuer_grant
                .as_ref()
                .is_some_and(|g| g.allows(&self.w.g, issuer, Cap::Admin));
        if !admin {
            return Err(CR::Refused("not admin"));
        }
        let Op::RemoveHost { permit } = &op.body.op else {
            return Err(CR::Malformed);
        };
        if !self.c[i].rev_ops.insert(op.id()) {
            return Err(CR::Duplicate);
        }
        self.c[i].revoked.insert(permit.clone());
        self.c[i].records.retain(|_, r| r.permit.digest() != *permit);
        Ok(())
    }
    // Delivers a revocation to devices and runs the §5 reaction on each one: an honest host
    // stops hosting, and its participants leave and redial the next backup that is not revoked.
    pub(super) fn revoke(&mut self, admin: usize, h: usize, to: &[usize]) {
        let bytes = self.revoke_op(admin, h);
        let digest = self.permits[&h].digest();
        for &i in to {
            let e = self.w.gep(i);
            self.w.send(admin, i, e, "op", bytes.clone());
            let _ = self.accept_revoke(i, &bytes);
        }
        for &i in to {
            if self.c[i].hosting.is_some() && i == h && !self.w.rogue.contains(&h) {
                self.c[h].hosting = None;
                self.c[h].log.push("hosting stopped: permit revoked".into());
            }
        }
        for &i in to {
            let Some(j) = self.c[i].joined.clone() else {
                continue;
            };
            if j.host.permit_digest == digest {
                self.c[i].joined = None;
                self.c[i].last_call = Some(j.call_id.clone());
                self.c[i].log.push("left: host permit revoked".into());
                let r = self.redial_after_loss(i, &j, self.now);
                self.redials.push((i, r));
            }
        }
    }

    // ---- status and selection ----
    pub(super) fn status(&self, h: usize, issued_ms: u64, ttl_ms: u64) -> Status {
        let c = &self.c[h].cond;
        let body = StatusBody {
            v: 1,
            gid: self.w.gid.clone(),
            host_ref: self.host_ref(h),
            class: c.class,
            power: if c.plugged {
                Power::Mains
            } else {
                Power::Battery(c.battery)
            },
            metered: c.metered,
            thermal_ok: c.thermal < Thermal::Moderate,
            max_participants: c.cap() as u8,
            busy: self.c[h].hosting.is_some() || phone_policy(c) != PhoneAction::Advertise,
            issued_ms,
            ttl_ms,
        };
        let sig = self.w.d[h].id.sign(&signed(STATUS_DOMAIN, &body)).to_vec();
        Status { body, sig }
    }
    pub(super) fn accept_status(&self, i: usize, s: &Status) -> bool {
        let r = &s.body.host_ref;
        s.verify()
            && s.body.gid == self.w.gid
            && self.c[i]
                .records
                .get(&dir_key(&r.holder_did, r.device_slot))
                .is_some_and(|rec| rec.permit.digest() == r.permit_digest)
            && !self.c[i].revoked.contains(&r.permit_digest)
            && self.is_member(i, &r.holder_did)
    }
    pub(super) fn rank_at(&self, i: usize, st: &[Status], call_id: &str, starter: &str, n: usize) -> Vec<HostRef> {
        let ok: Vec<StatusBody> = st
            .iter()
            .filter(|s| self.accept_status(i, s))
            .map(|s| s.body.clone())
            .collect();
        rank(&ok, call_id, starter, n, self.now)
    }

    // ---- CallStart ----
    pub(super) fn start_msg(&self, h: usize, call_id: &str, hgen: u32, starter: usize, backups: Vec<HostRef>) -> Ctl {
        let c = &self.c[h].cond;
        let frontier = digest(&self.w.d[h].view.ops.keys().collect::<Vec<_>>());
        self.msg(
            h,
            call_id,
            hgen,
            Kind::Start {
                host_ref: self.host_ref(h),
                starter_did: self.did(starter),
                started_ms: self.now,
                k: c.k() as u8,
                max_participants: c.cap() as u8,
                backups,
                meta_frontier: frontier,
            },
        )
    }
    // CallHostRequest answered by candidate `h`. It re-checks local consent and conditions
    // (§2.3), and that a phone hosts only while its own user is in the call (§2.4, Q11).
    pub(super) fn start_call(&mut self, h: usize, starter: usize, backups: Vec<HostRef>) -> Result<String, CR> {
        let call_id = self.new_call_id();
        self.begin_hosting(h, &call_id, 1, starter, backups)?;
        Ok(call_id)
    }
    pub(super) fn begin_hosting(
        &mut self,
        h: usize,
        call_id: &str,
        hgen: u32,
        starter: usize,
        backups: Vec<HostRef>,
    ) -> Result<(), CR> {
        let c = self.c[h].cond;
        if !self.c[h].consent {
            return Err(CR::Refused("call hosting disabled locally"));
        }
        if self.c[h].hosting.is_some() {
            return Err(CR::Busy);
        }
        if self.c[h].revoked.contains(&self.permits[&h].digest()) {
            return Err(CR::Revoked);
        }
        if c.class == Class::Phone {
            if !matches!(phone_policy(&c), PhoneAction::Advertise) {
                return Err(CR::Refused("phone conditions"));
            }
            let own_in = if hgen == 1 {
                self.did(starter) == self.did(h)
            } else {
                self.c[h].last_call.as_deref() == Some(call_id)
            };
            if !own_in {
                return Err(CR::Refused("phone never hosts listen-only"));
            }
        }
        if backups.len() > MAX_BACKUPS {
            return Err(CR::Malformed);
        }
        let start = self.start_msg(h, call_id, hgen, starter, backups);
        self.c[h].hosting = Some(HostCall {
            call_id: call_id.into(),
            host_gen: hgen,
            start: start.clone(),
            k: c.k(),
            max: c.cap(),
            epoch: 0,
            parts: BTreeMap::new(),
            seen: BTreeSet::new(),
            last_join: BTreeMap::new(),
            buckets: BTreeMap::new(),
            ctl_times: BTreeMap::new(),
            limited: false,
            fwd: Fwd::new(c.k()),
            pending: BTreeMap::new(),
            levels: BTreeMap::new(),
            active_gen: 0,
            active: BTreeSet::new(),
            level_only_rx: 0,
            dropped: BTreeMap::new(),
            warned_metered: false,
        });
        self.c[h].may_take.remove(call_id);
        self.ever_hosted.insert(h);
        let a = Announce {
            start,
            record: self.record(h),
        };
        self.announce(h, serde_json::to_vec(&a).unwrap());
        Ok(())
    }
    // The host's own copy is applied at once. Everyone else gets it through chat host H2,
    // unless the relay is held.
    pub(super) fn announce(&mut self, h: usize, bytes: Vec<u8>) {
        let _ = self.accept_start(h, &bytes);
        if self.hold_relay {
            self.held.push(bytes);
            return;
        }
        self.relay(bytes);
        self.settle();
    }
    // Chat host H2 relays a group Ephemeral only to devices it serves: current members and
    // hosts. A removed member gets nothing. Without that rule she would learn a rotated call
    // host's endpoint from the record inside the Announce (see GC-S11).
    pub(super) fn relay_targets(&self) -> Vec<usize> {
        (0..self.w.d.len())
            .filter(|&i| self.w.hosts.contains(&i) || self.is_member(H2, &self.did(i)))
            .collect()
    }
    pub(super) fn relay(&mut self, bytes: Vec<u8>) {
        for i in self.relay_targets() {
            let e = self.w.gep(i);
            self.w.send(H2, i, e, "call-start", bytes.clone());
            let _ = self.accept_start(i, &bytes);
        }
    }
    pub(super) fn flush_relay(&mut self) {
        self.hold_relay = false;
        for b in std::mem::take(&mut self.held) {
            self.relay(b);
        }
        self.settle();
    }
    // Members' checks on a CallStart (§3.2 items 1-3) plus generation, split and glare rules.
    pub(super) fn accept_start(&mut self, i: usize, bytes: &[u8]) -> Result<(), CR> {
        if bytes.len() > MAX_CONTROL {
            return Err(CR::Oversize);
        }
        let a: Announce = serde_json::from_slice(bytes).map_err(|_| CR::Malformed)?;
        let s = &a.start;
        let m = &s.msg;
        let Kind::Start {
            host_ref,
            starter_did,
            k,
            max_participants,
            backups,
            ..
        } = &m.kind
        else {
            return Err(CR::Malformed);
        };
        if m.v != 1 || *k == 0 || *max_participants as usize > CAP_DESKTOP || backups.len() > MAX_BACKUPS {
            return Err(CR::Malformed);
        }
        if m.gid != self.w.gid {
            return Err(CR::WrongGroup);
        }
        if !s.verify() {
            return Err(CR::BadSignature);
        }
        self.check_call_record(i, &a.record)?;
        let rec = &a.record;
        if rec.note.claims.owner != m.signer
            || host_ref.holder_did != m.signer
            || host_ref.device_slot != rec.note.claims.slot
            || host_ref.permit_digest != rec.permit.digest()
        {
            return Err(CR::DeviceMismatch);
        }
        if !self.is_member(i, starter_did) {
            return Err(CR::NotMember);
        }
        let cur = self.c[i].hgen.get(&m.call_id).copied().unwrap_or(0);
        if m.host_gen < cur {
            return Err(CR::StaleGen);
        }
        if m.host_gen == cur {
            if let Some(have) = self.c[i].starts.get(&m.call_id) {
                let (have_ref, _) = have.start().unwrap();
                if have_ref == host_ref {
                    return Err(CR::Duplicate);
                }
                // Split after redial (§5): the lower index in the previous gen's backups wins.
                let order = self.c[i]
                    .prev
                    .get(&m.call_id)
                    .and_then(|p| p.start().map(|(_, b)| b.clone()))
                    .unwrap_or_default();
                let idx = |r: &HostRef| order.iter().position(|b| b == r).unwrap_or(usize::MAX);
                if (idx(host_ref), tie_hash(&m.call_id, host_ref))
                    >= (idx(have_ref), tie_hash(&m.call_id, have_ref))
                {
                    return Err(CR::Superseded);
                }
            }
        }
        // ---- all checks passed; mutate ----
        let call_id = m.call_id.clone();
        let me = self.host_ref_of(i);
        if m.host_gen > cur {
            if let Some(old) = self.c[i].starts.remove(&call_id) {
                self.c[i].prev.insert(call_id.clone(), old);
            }
        }
        let key = dir_key(&rec.note.claims.owner, rec.note.claims.slot);
        if self.c[i].records.get(&key).is_none_or(|r| r.note.claims.seq < rec.note.claims.seq) {
            self.c[i].records.insert(key, rec.clone());
        }
        self.c[i].hgen.insert(call_id.clone(), m.host_gen);
        self.c[i].starts.insert(call_id.clone(), s.clone());
        match me {
            Some(me) if backups.contains(&me) => {
                self.c[i].may_take.insert(call_id.clone(), m.host_gen + 1);
            }
            _ => {
                self.c[i].may_take.remove(&call_id);
            }
        }
        self.c[i].shown = self.c[i].starts.keys().min_by_key(|c| call_hash(c)).cloned();
        Ok(())
    }
    pub(super) fn host_ref_of(&self, i: usize) -> Option<HostRef> {
        self.permits.contains_key(&i).then(|| self.host_ref(i))
    }
    // Glare (§3.1) and split (§5): a host whose call lost sends End{superseded, next_host}.
    pub(super) fn settle(&mut self) {
        for h in 0..self.w.d.len() {
            let Some(hc) = self.c[h].hosting.as_ref() else {
                continue;
            };
            let call_id = hc.call_id.clone();
            let winner_call = self.c[h].shown.clone();
            let winner = winner_call.as_ref().and_then(|w| self.c[h].starts.get(w)).cloned();
            let Some(winner) = winner else { continue };
            let (wref, _) = winner.start().unwrap();
            if *wref != self.host_ref(h) && (winner.msg.call_id != call_id || winner.msg.host_gen == hc.host_gen) {
                let wref = wref.clone();
                self.host_end(h, EndReason::Superseded, Some(wref), None);
            }
        }
    }

    // ---- Join (§3.2 at H) ----
    pub(super) fn join_req(&mut self, m: usize, call_id: &str, hgen: u32, issued_ms: u64) -> Vec<u8> {
        let nonce = self.w.rng.next_u64();
        let join = self.msg(m, call_id, hgen, Kind::Join { issued_ms, nonce });
        serde_json::to_vec(&JoinReq {
            join,
            note: self.w.d[m].my_note.clone(),
        })
        .unwrap()
    }
}
