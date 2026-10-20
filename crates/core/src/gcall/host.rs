//! The call host's side (§1, §3, §5, §6). One engine per hosted call and generation.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use identity::Identity;
use proto::Signed;
use proto::gcall::{
    self as w, CallStart, End, EndReason, GroupCallMsg, HostRef, Join, Roster, RosterEntry,
    UpDatagram,
};
use proto::gcall::{SignedEnd, SignedJoin, SignedLeave, SignedNote, SignedStart};

use super::policy::{Class, Cond, PhoneAction, phone_policy};
use super::select::Forwarder;
use super::*;

/// What the starter's pick (or a takeover) hands to [`HostEngine::new`].
#[derive(Debug, Clone)]
pub struct HostConfig {
    pub gid: String,
    pub call_id: String,
    pub host_gen: u32,
    /// Must name this device; `holder_did` is the signing identity's DID.
    pub host_ref: HostRef,
    pub starter_did: String,
    pub backups: Vec<HostRef>,
    pub meta_frontier: String,
    pub cond: Cond,
    /// The host's own user is in the call. Phones never host listen-only (§2.4, Q11).
    pub own_user_in: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    /// A newer Join from the same DID on another device (§3.2 item 5).
    AnsweredElsewhere,
    /// The participant is no longer a member (an `End{removed}` was sent first).
    Removed,
    /// No control and no datagram for `MEMBER_DROP_MS`.
    Dropped,
    /// The call ended (an `End` was sent first).
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostNotice {
    /// "Hosting limited": K = 1, new Joins refused (§2.4).
    Limited,
    /// Conditions recovered; K and Joins back to normal.
    Unlimited,
    /// Wi-Fi to metered mid-call: warn the host's user about data use.
    MeteredData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostAction {
    /// An encoded control frame for the connection's control stream.
    Control { conn: ConnId, frame: Vec<u8> },
    Datagram { conn: ConnId, bytes: Vec<u8> },
    /// Close the connection after flushing its control stream.
    Close { conn: ConnId, reason: CloseReason },
    Notice(HostNotice),
    /// The engine is finished. `end` is the signed End that went to the participants (relay it
    /// through the chat hosts when `reason` is `Ended`); `None` when hosting stopped without
    /// one (own permit revoked).
    Ended {
        reason: EndReason,
        next_host: Option<HostRef>,
        end: Option<SignedEnd>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Part {
    slot: u8,
    conn: ConnId,
    join: Join,
    sig: [u8; 64],
    last_ms: u64,
    /// Upstream token bucket, milli-datagrams, and when it was last filled.
    bucket: u64,
    bucket_ms: u64,
}

/// Everything a refused Join or Leave must leave untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
struct State {
    epoch: u32,
    parts: BTreeMap<String, Part>,
    seen: BTreeSet<[u8; 64]>,
    last_join: BTreeMap<String, u64>,
    cond: Cond,
    limited: bool,
    fwd: Forwarder,
    levels: BTreeMap<u8, u8>,
    active_gen: u32,
    active: BTreeSet<u8>,
    next_frame_ms: u64,
    next_hb_ms: u64,
    ended: bool,
}

pub struct HostEngine {
    id: Identity,
    start: CallStart,
    start_blob: SignedStart,
    st: State,
    /// Authenticated remote endpoint per connection (transport bookkeeping).
    conns: BTreeMap<ConnId, [u8; 32]>,
    /// Per-connection control rate (counts refused frames too).
    ctl_times: BTreeMap<ConnId, VecDeque<u64>>,
    level_only_rx: u64,
    dropped: BTreeMap<String, u64>,
}

impl HostEngine {
    /// Re-checks local conditions (§2.3) and signs the CallStart. `now`: Unix ms.
    pub fn new(id: Identity, cfg: HostConfig, now: u64) -> Result<Self, Refusal> {
        if cfg.host_ref.holder_did != id.did() || cfg.backups.len() > w::MAX_BACKUPS || cfg.host_gen == 0 {
            return Err(Refusal::Malformed);
        }
        if cfg.cond.class == Class::Phone && (phone_policy(&cfg.cond) != PhoneAction::Advertise || !cfg.own_user_in) {
            return Err(Refusal::Refused);
        }
        let k = cfg.cond.k();
        let start = CallStart {
            gid: cfg.gid,
            call_id: cfg.call_id,
            host_gen: cfg.host_gen,
            host_ref: cfg.host_ref,
            starter_did: cfg.starter_did,
            started_ms: now,
            k: k as u8,
            max_participants: cfg.cond.cap() as u8,
            backups: cfg.backups,
            meta_frontier: cfg.meta_frontier,
        };
        let start_blob = Signed::sign(&start, &id).map_err(|_| Refusal::Malformed)?;
        Ok(HostEngine {
            id,
            start,
            start_blob,
            st: State {
                epoch: 0,
                parts: BTreeMap::new(),
                seen: BTreeSet::new(),
                last_join: BTreeMap::new(),
                cond: cfg.cond,
                limited: false,
                fwd: Forwarder::new(k),
                levels: BTreeMap::new(),
                active_gen: 0,
                active: BTreeSet::new(),
                next_frame_ms: now + FRAME_MS,
                next_hb_ms: now + HEARTBEAT_MS,
                ended: false,
            },
            conns: BTreeMap::new(),
            ctl_times: BTreeMap::new(),
            level_only_rx: 0,
            dropped: BTreeMap::new(),
        })
    }

    /// §5 redial: this device is `prev.backups[i]` and got the first valid
    /// `Join{prev.host_gen + 1}`. The new CallStart lists the previous backups minus this device (G9). The wiring
    /// feeds that Join next and drops the engine if it is refused.
    pub fn takeover(id: Identity, prev: &CallStart, me: HostRef, cond: Cond, own_user_in: bool, now: u64) -> Result<Self, Refusal> {
        if !prev.backups.contains(&me) {
            return Err(Refusal::Refused);
        }
        let cfg = HostConfig {
            gid: prev.gid.clone(),
            call_id: prev.call_id.clone(),
            host_gen: prev.host_gen + 1,
            backups: prev.backups.iter().filter(|b| **b != me).cloned().collect(),
            host_ref: me,
            starter_did: prev.starter_did.clone(),
            meta_frontier: prev.meta_frontier.clone(),
            cond,
            own_user_in,
        };
        Self::new(id, cfg, now)
    }

    pub fn call_start(&self) -> &CallStart {
        &self.start
    }

    /// The signed CallStart to announce through the chat hosts (G1: next to the call-host
    /// record).
    pub fn start_blob(&self) -> &SignedStart {
        &self.start_blob
    }

    pub fn is_ended(&self) -> bool {
        self.st.ended
    }

    /// slot → DID.
    pub fn roster(&self) -> BTreeMap<u8, String> {
        self.st.parts.iter().map(|(d, p)| (p.slot, d.clone())).collect()
    }

    pub fn slot_of(&self, did: &str) -> Option<u8> {
        self.st.parts.get(did).map(|p| p.slot)
    }

    pub fn conn_of(&self, did: &str) -> Option<ConnId> {
        self.st.parts.get(did).map(|p| p.conn)
    }

    pub fn epoch(&self) -> u32 {
        self.st.epoch
    }

    pub fn k(&self) -> usize {
        self.st.fwd.k()
    }

    /// Every host forwards every speaker (Q2), so S is never full in a real call; tests of the
    /// G6-D2 provisional slot shrink K by hand.
    #[cfg(test)]
    pub(crate) fn set_k_for_test(&mut self, k: usize) {
        self.st.fwd.set_k(k);
    }

    /// `(gen, forwarded slots)` as last announced in `Active`.
    pub fn active(&self) -> (u32, &BTreeSet<u8>) {
        (self.st.active_gen, &self.st.active)
    }

    pub fn forwarder(&self) -> &Forwarder {
        &self.st.fwd
    }

    pub fn level_only_rx(&self) -> u64 {
        self.level_only_rx
    }

    /// Datagrams dropped per DID (malformed, oversize, over rate).
    #[cfg(test)]
    pub fn dropped(&self, did: &str) -> u64 {
        self.dropped.get(did).copied().unwrap_or(0)
    }

    /// Participant cap now: the announced one, lowered by current conditions.
    pub fn cap(&self) -> usize {
        (self.start.max_participants as usize).min(self.st.cond.cap())
    }

    /// The first backup whose permit is still valid.
    pub fn next_backup(&self, auth: &dyn CallAuthority) -> Option<HostRef> {
        self.start.backups.iter().find(|b| auth.host_permit_ok(&self.start.gid, b)).cloned()
    }

    /// A transport connection was accepted on `tinline/gcall/1`; `endpoint` is the
    /// authenticated remote endpoint key.
    pub fn connected(&mut self, conn: ConnId, endpoint: [u8; 32]) {
        if conn != ConnId::LOCAL {
            self.conns.insert(conn, endpoint);
        }
    }

    /// The connection closed. A participant on it leaves the Roster.
    pub fn disconnected(&mut self, conn: ConnId) -> Vec<HostAction> {
        self.conns.remove(&conn);
        self.ctl_times.remove(&conn);
        let gone: Vec<String> = self.st.parts.iter().filter(|(_, p)| p.conn == conn).map(|(d, _)| d.clone()).collect();
        if gone.is_empty() || self.st.ended {
            return vec![];
        }
        for d in gone {
            self.remove_part(&d);
        }
        self.after_removal()
    }

    /// After someone left the Roster: the last one out ends the call for good (otherwise an empty
    /// call stays announced forever, and "End" on a member only leaves it); else Roster + Active.
    fn after_removal(&mut self) -> Vec<HostAction> {
        if self.st.parts.is_empty() {
            return self.end(EndReason::Ended, None);
        }
        let mut out = self.roster_actions();
        out.extend(self.active_actions());
        out
    }

    /// A control frame from `conn`. Members send only `Join` and `Leave`.
    pub fn on_control(&mut self, conn: ConnId, msg: GroupCallMsg, now: u64, auth: &dyn CallAuthority) -> Result<Vec<HostAction>, Refusal> {
        if self.st.ended {
            return Err(Refusal::Unknown);
        }
        let q = self.ctl_times.entry(conn).or_default();
        while q.front().is_some_and(|t| t + 1_000 <= now) {
            q.pop_front();
        }
        if q.len() >= MAX_CONTROL_PER_S {
            return Err(Refusal::RateLimited);
        }
        q.push_back(now);
        match msg {
            GroupCallMsg::Join { join, note } => self.join(conn, &join, note.as_ref(), now, auth),
            GroupCallMsg::Leave { leave } => self.leave(conn, &leave, auth),
            _ => Err(Refusal::Malformed),
        }
    }

    // §3.2 at H. Every check runs before any state changes.
    fn join(
        &mut self,
        conn: ConnId,
        blob: &SignedJoin,
        note: Option<&SignedNote>,
        now: u64,
        auth: &dyn CallAuthority,
    ) -> Result<Vec<HostAction>, Refusal> {
        let j = blob.open()?;
        let s = &self.start;
        if j.gid != s.gid {
            return Err(Refusal::WrongGroup);
        }
        if j.call_id != s.call_id {
            return Err(Refusal::WrongCall);
        }
        if j.host_gen != s.host_gen {
            return Err(Refusal::StaleGen);
        }
        if !auth.is_member(&s.gid, &j.did) {
            return Err(Refusal::NotMember);
        }
        if conn == ConnId::LOCAL {
            if j.did != self.id.did() {
                return Err(Refusal::EndpointMismatch);
            }
        } else {
            let ep = self.conns.get(&conn).ok_or(Refusal::Unknown)?;
            if !auth.endpoint_ok(&s.gid, &j.did, note, ep) {
                return Err(Refusal::EndpointMismatch);
            }
        }
        if self.st.seen.contains(blob.sig()) {
            return Err(Refusal::Replay);
        }
        if self.st.last_join.get(&j.did).is_some_and(|t| t + JOIN_MIN_GAP_MS > now) {
            return Err(Refusal::RateLimited);
        }
        match self.st.parts.get(&j.did) {
            // G5: newer = larger signed issued_ms.
            Some(p) if p.join.issued_ms >= j.issued_ms => return Err(Refusal::Stale),
            Some(_) => {}
            None if self.st.limited => return Err(Refusal::Limited),
            None if self.st.parts.len() >= self.cap() => return Err(Refusal::Full),
            None => {}
        }
        // ---- accepted ----
        let did = j.did.clone();
        self.st.seen.insert(*blob.sig());
        self.st.last_join.insert(did.clone(), now);
        let old = self.st.parts.remove(&did);
        let slot = match &old {
            Some(o) => o.slot, // one slot per DID; the device behind it changes
            None => {
                let used: BTreeSet<u8> = self.st.parts.values().map(|p| p.slot).collect();
                (0..=u8::MAX).find(|s| !used.contains(s)).expect("at most 12 participants")
            }
        };
        let part = Part {
            slot,
            conn,
            join: j,
            sig: *blob.sig(),
            last_ms: now,
            bucket: UP_BURST * 1_000,
            bucket_ms: now,
        };
        self.st.parts.insert(did, part);
        let mut out = vec![];
        if let Some(o) = old.filter(|o| o.conn != conn) {
            out.push(HostAction::Close { conn: o.conn, reason: CloseReason::AnsweredElsewhere });
        }
        out.push(HostAction::Control { conn, frame: frame(&GroupCallMsg::Start { start: self.start_blob.clone() }) });
        out.extend(self.roster_actions());
        if self.st.active_gen > 0 {
            let a = GroupCallMsg::Active { r#gen: self.st.active_gen, slots: self.st.active.iter().copied().collect() };
            out.push(HostAction::Control { conn, frame: frame(&a) });
        }
        Ok(out)
    }

    fn leave(
        &mut self,
        conn: ConnId,
        blob: &SignedLeave,
        auth: &dyn CallAuthority,
    ) -> Result<Vec<HostAction>, Refusal> {
        let l = blob.open()?;
        let s = &self.start;
        if l.gid != s.gid || l.call_id != s.call_id || l.host_gen != s.host_gen {
            return Err(Refusal::WrongCall);
        }
        let p = self.st.parts.get(&l.did).filter(|p| p.conn == conn).ok_or(Refusal::NotParticipant)?;
        if l.issued_ms < p.join.issued_ms {
            return Err(Refusal::Stale); // a Leave from before this Join
        }
        self.remove_part(&l.did);
        if l.did == self.id.did() && self.st.cond.class == Class::Phone {
            // §2.4 / Q11: a phone hosts only while its own user is in the call. With nobody else
            // in it there is no one to hand over to: end it for everyone (`End{ended}`), so every
            // member's list drops it, instead of a hand-over nobody redials (left listed for hours).
            if self.st.parts.is_empty() {
                return Ok(self.end(EndReason::Ended, None));
            }
            let next = self.next_backup(auth);
            return Ok(self.end(EndReason::HostLeaving, next));
        }
        Ok(self.after_removal())
    }

    /// An upstream datagram from `conn`. Audio from a forwarded slot goes out at once to every
    /// other participant; level-only datagrams only feed selection.
    pub fn on_datagram(&mut self, conn: ConnId, bytes: &[u8], now: u64) -> Result<Vec<HostAction>, Refusal> {
        if self.st.ended {
            return Err(Refusal::Unknown);
        }
        let did = self.st.parts.iter().find(|(_, p)| p.conn == conn).map(|(d, _)| d.clone()).ok_or(Refusal::NotParticipant)?;
        let up = match w::parse_up(bytes) {
            Ok(up) => up,
            Err(e) => {
                *self.dropped.entry(did).or_default() += 1;
                return Err(match e {
                    proto::Error::FrameTooLarge => Refusal::Oversize,
                    _ => Refusal::Malformed,
                });
            }
        };
        let p = self.st.parts.get_mut(&did).expect("found above");
        p.bucket = (p.bucket + now.saturating_sub(p.bucket_ms) * MAX_UP_PPS).min(UP_BURST * 1_000);
        p.bucket_ms = now;
        if p.bucket < 1_000 {
            *self.dropped.entry(did).or_default() += 1;
            return Err(Refusal::RateLimited);
        }
        p.bucket -= 1_000;
        p.last_ms = now;
        let slot = p.slot;
        // Age selection BEFORE recording this arrival. Otherwise a delayed tick puts fresh
        // levels/provisional speech in the first historical frame and then ages them out.
        let mut out = self.selection_actions(now, false);
        match up {
            UpDatagram::Level(l) => {
                self.st.levels.insert(slot, l);
                self.level_only_rx += 1;
                Ok(out)
            }
            UpDatagram::Audio(a) => {
                let level = w::audio_level(a);
                self.st.levels.insert(slot, level);
                if self.st.fwd.offer(slot, level) {
                    out.extend(self.active_actions()); // G6-D2: announce before forwarding
                }
                if self.st.fwd.is_forwarded(slot) {
                    let d = w::down_audio(slot, a);
                    for q in self.st.parts.values().filter(|q| q.conn != conn) {
                        out.push(HostAction::Datagram { conn: q.conn, bytes: d.clone() });
                    }
                }
                Ok(out)
            }
        }
    }

    /// Advance the selection clock using only observations received before this advance.
    /// Both ticks and accepted arrivals use this so scheduling order after a stall cannot
    /// make current audio belong to historical frames. Keep the existing bounded catch-up.
    // An arrival advances strictly older frames; the tick can include the current boundary.
    fn selection_actions(&mut self, now: u64, include_now: bool) -> Vec<HostAction> {
        let mut out = vec![];
        let mut frames = 0;
        while self.st.next_frame_ms < now || (include_now && self.st.next_frame_ms == now) {
            let levels = std::mem::take(&mut self.st.levels);
            self.st.fwd.frame(&levels);
            out.extend(self.active_actions());
            self.st.next_frame_ms += FRAME_MS;
            frames += 1;
            if frames >= 50 {
                self.st.next_frame_ms = now + FRAME_MS;
                break;
            }
        }
        out
    }

    /// Call at least every 20 ms. Runs speaker selection per frame, heartbeats, the member
    /// drop timer and the membership/permit sweep.
    pub fn tick(&mut self, now: u64, auth: &dyn CallAuthority) -> Vec<HostAction> {
        if self.st.ended {
            return vec![];
        }
        let gid = self.start.gid.clone();
        if !auth.host_permit_ok(&gid, &self.start.host_ref) {
            // §5: own permit revoked or own removal. Members detect it and redial; an End
            // signed under a revoked permit would be refused anyway.
            let mut out: Vec<HostAction> = self.remote_conns().into_iter().map(|conn| HostAction::Close { conn, reason: CloseReason::Ended }).collect();
            self.st.ended = true;
            out.push(HostAction::Ended { reason: EndReason::HostLeaving, next_host: None, end: None });
            return out;
        }
        let mut out = self.selection_actions(now, true);
        let mut changed = false;
        // §3.2 item 2: a removed member is dropped at once with End{removed} to them only.
        let removed: Vec<String> = self.st.parts.keys().filter(|d| !auth.is_member(&gid, d)).cloned().collect();
        for d in removed {
            let conn = self.st.parts[&d].conn;
            let e = self.sign_end(EndReason::Removed, None);
            out.push(HostAction::Control { conn, frame: frame(&GroupCallMsg::End { end: e }) });
            out.push(HostAction::Close { conn, reason: CloseReason::Removed });
            self.remove_part(&d);
            changed = true;
        }
        // §5 member drop.
        let gone: Vec<String> = self
            .st
            .parts
            .iter()
            .filter(|(_, p)| p.conn != ConnId::LOCAL && p.last_ms + MEMBER_DROP_MS <= now)
            .map(|(d, _)| d.clone())
            .collect();
        for d in gone {
            out.push(HostAction::Close { conn: self.st.parts[&d].conn, reason: CloseReason::Dropped });
            self.remove_part(&d);
            changed = true;
        }
        if changed {
            out.extend(self.after_removal());
            if self.st.ended {
                return out;
            }
        }
        if now >= self.st.next_hb_ms {
            self.st.next_hb_ms = now + HEARTBEAT_MS;
            let hb = frame(&GroupCallMsg::Heartbeat { epoch: self.st.epoch });
            for conn in self.remote_conns() {
                out.push(HostAction::Control { conn, frame: hb.clone() });
            }
        }
        out
    }

    /// New local conditions (§2.4 cutoffs).
    pub fn set_cond(&mut self, cond: Cond, auth: &dyn CallAuthority) -> Vec<HostAction> {
        if self.st.ended {
            return vec![];
        }
        let was_metered = self.st.cond.metered;
        self.st.cond = cond;
        let mut out = vec![];
        match phone_policy(&cond) {
            PhoneAction::End => {
                let next = self.next_backup(auth);
                return self.end(EndReason::HostLeaving, next);
            }
            PhoneAction::Limited if !self.st.limited => {
                self.st.limited = true;
                self.st.fwd.set_k(1);
                self.st.fwd.set_provisional(false);
                out.push(HostAction::Notice(HostNotice::Limited));
                out.extend(self.active_actions());
            }
            PhoneAction::Advertise | PhoneAction::StopAdvertising if self.st.limited => {
                self.st.limited = false;
                self.st.fwd.set_k(cond.k());
                self.st.fwd.set_provisional(true);
                out.push(HostAction::Notice(HostNotice::Unlimited));
                out.extend(self.active_actions());
            }
            // K follows the conditions if they ever differ by host state.
            _ if !self.st.limited && self.st.fwd.k() != cond.k() => {
                self.st.fwd.set_k(cond.k());
                out.extend(self.active_actions());
            }
            _ => {}
        }
        if cond.metered && !was_metered {
            out.push(HostAction::Notice(HostNotice::MeteredData));
        }
        out
    }

    /// End the call for everyone: `End` on every control stream, then close.
    pub fn end(&mut self, reason: EndReason, next_host: Option<HostRef>) -> Vec<HostAction> {
        if self.st.ended {
            return vec![];
        }
        let e = self.sign_end(reason, next_host.clone());
        let f = frame(&GroupCallMsg::End { end: e.clone() });
        let mut out = vec![];
        for p in self.st.parts.values() {
            out.push(HostAction::Control { conn: p.conn, frame: f.clone() });
        }
        for conn in self.remote_conns() {
            out.push(HostAction::Close { conn, reason: CloseReason::Ended });
        }
        self.st.ended = true;
        out.push(HostAction::Ended { reason, next_host, end: Some(e) });
        out
    }

    /// Glare or split (§3.1, §5): this call lost to the one hosted by `winner`.
    pub fn supersede(&mut self, winner: HostRef) -> Vec<HostAction> {
        self.end(EndReason::Superseded, Some(winner))
    }

    fn remote_conns(&self) -> BTreeSet<ConnId> {
        self.st.parts.values().map(|p| p.conn).filter(|c| *c != ConnId::LOCAL).collect()
    }

    fn remove_part(&mut self, did: &str) {
        if let Some(p) = self.st.parts.remove(did) {
            self.st.fwd.remove(p.slot);
            self.st.levels.remove(&p.slot);
        }
    }

    fn sign_end(&self, reason: EndReason, next_host: Option<HostRef>) -> SignedEnd {
        let s = &self.start;
        let end = End {
            gid: s.gid.clone(),
            call_id: s.call_id.clone(),
            host_gen: s.host_gen,
            host_did: self.id.did().into(),
            reason,
            next_host,
        };
        Signed::sign(&end, &self.id).expect("our own call's End signs")
    }

    fn roster_actions(&mut self) -> Vec<HostAction> {
        self.st.epoch += 1;
        let s = &self.start;
        let mut entries: Vec<RosterEntry> = self
            .st
            .parts
            .iter()
            .map(|(d, p)| RosterEntry {
                slot: p.slot,
                did: d.clone(),
                issued_ms: p.join.issued_ms,
                nonce: p.join.nonce,
                sig: p.sig.to_vec(),
            })
            .collect();
        entries.sort_by_key(|e| e.slot);
        let r = Roster {
            gid: s.gid.clone(),
            call_id: s.call_id.clone(),
            host_gen: s.host_gen,
            host_did: self.id.did().into(),
            epoch: self.st.epoch,
            entries,
        };
        let f = frame(&GroupCallMsg::Roster {
            roster: Signed::sign(&r, &self.id).expect("our own roster signs"),
        });
        self.st
            .parts
            .values()
            .map(|p| HostAction::Control {
                conn: p.conn,
                frame: f.clone(),
            })
            .collect()
    }

    // G6-D1/D2: `Active` to every participant whenever the forwarded set changes.
    fn active_actions(&mut self) -> Vec<HostAction> {
        let f = self.st.fwd.forwarded();
        if f == self.st.active {
            return vec![];
        }
        self.st.active_gen += 1;
        self.st.active = f;
        let a = frame(&GroupCallMsg::Active { r#gen: self.st.active_gen, slots: self.st.active.iter().copied().collect() });
        self.st.parts.values().map(|p| HostAction::Control { conn: p.conn, frame: a.clone() }).collect()
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> impl PartialEq + std::fmt::Debug + use<> {
        self.st.clone()
    }
}

/// Every frame the engines build is far below `MAX_CONTROL` (a 12-entry Roster is ~3.8 KB).
pub(super) fn frame(m: &GroupCallMsg) -> Vec<u8> {
    w::encode_control(m).expect("gcall frames fit MAX_CONTROL")
}
