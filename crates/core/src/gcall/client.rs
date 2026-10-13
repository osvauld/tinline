//! One participant's side (§1.4, §3.2, §5): verifies the host's control, tracks the Roster
//! and `Active`, keeps one jitter buffer per forwarded slot and mixes them, runs the sender
//! silence gate, and redials the backups when the host is lost.

use std::collections::{BTreeMap, BTreeSet};

use identity::Identity;
use proto::SignedBlob;
use proto::gcall::{self as w, CallStart, EndReason, GroupCallMsg, HostRef};

use super::host::frame;
use super::*;

/// Level-only datagrams while gated: one per `LEVEL_ONLY_MS` of 20 ms frames.
const LEVEL_ONLY_FRAMES: u32 = (LEVEL_ONLY_MS / FRAME_MS) as u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    /// Join sent to `host()`, waiting for a Roster that lists this DID.
    Dialing,
    Joined,
    /// Host lost or handing over; Join sent to the next candidate ("Reconnecting…").
    Redialing,
    Ended(ClientEnd),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientEnd {
    /// `End{ended}`.
    Ended,
    /// `End{removed}`.
    Removed,
    /// This device left.
    Left,
    /// No host answered: "Call ended — host lost" with a Rejoin button (§5).
    HostLost,
    /// A Roster no longer lists this DID.
    Dropped,
    /// The same DID joined from another device (§3.2 item 5).
    AnsweredElsewhere,
    /// The call lost a glare or split; see [`ClientAction::Superseded`].
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientAction {
    /// Close any current call connection and open `tinline/gcall/1` to `host` from the group
    /// endpoint. Following `Control`/`Datagram` actions go there.
    Dial { host: HostRef },
    /// An encoded control frame for the host.
    Control(Vec<u8>),
    Datagram(Vec<u8>),
    Joined { slot: u8 },
    RosterChanged,
    /// Join the call hosted by `by` instead (`CallBoard::hosted_by`).
    Superseded { by: HostRef },
    Ended(ClientEnd),
}

pub struct ClientEngine {
    id: Identity,
    note: Option<SignedBlob>,
    start: CallStart,
    host: HostRef,
    expected_gen: u32,
    state: ClientState,
    local: bool,
    epoch: u32,
    roster: BTreeMap<u8, String>,
    my_slot: Option<u8>,
    active_gen: u32,
    active: BTreeSet<u8>,
    last_hb_ms: u64,
    /// Remaining redial candidates after `host`, and when the current attempt times out.
    candidates: Vec<HostRef>,
    deadline: Option<u64>,
    receivers: BTreeMap<u8, audio::Receiver>,
    muted: BTreeSet<String>,
    sender: audio::Sender,
    gated_frames: u32,
    heard: BTreeMap<u8, u64>,
    dropped: u64,
}

impl ClientEngine {
    /// Join the call in `start` (already accepted by the `CallBoard`). `note` is this
    /// device's DID-signed group endpoint note (G2). `now`: Unix ms (Join `issued_ms`).
    pub fn new(id: Identity, start: CallStart, note: Option<SignedBlob>, now: u64, auth: &dyn CallAuthority) -> Result<(Self, Vec<ClientAction>), Refusal> {
        if !auth.host_permit_ok(&start.gid, &start.host_ref) {
            return Err(Refusal::PermitInvalid);
        }
        let sender = audio::Sender::new(GROUP_BITRATE_BPS).map_err(|_| Refusal::Refused)?;
        let mut c = ClientEngine {
            id,
            note,
            host: start.host_ref.clone(),
            expected_gen: start.host_gen,
            start,
            state: ClientState::Dialing,
            local: false,
            epoch: 0,
            roster: BTreeMap::new(),
            my_slot: None,
            active_gen: 0,
            active: BTreeSet::new(),
            last_hb_ms: now,
            candidates: vec![],
            deadline: Some(now + ANSWER_TIMEOUT_MS),
            receivers: BTreeMap::new(),
            muted: BTreeSet::new(),
            sender,
            gated_frames: 0,
            heard: BTreeMap::new(),
            dropped: 0,
        };
        let out = vec![ClientAction::Dial { host: c.host.clone() }, c.join_frame(now)];
        Ok((c, out))
    }

    pub fn state(&self) -> ClientState {
        self.state
    }

    pub fn call_start(&self) -> &CallStart {
        &self.start
    }

    pub fn host(&self) -> &HostRef {
        &self.host
    }

    pub fn host_gen(&self) -> u32 {
        self.expected_gen
    }

    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// slot → DID; DIDs and slots only (U7).
    pub fn roster(&self) -> &BTreeMap<u8, String> {
        &self.roster
    }

    pub fn my_slot(&self) -> Option<u8> {
        self.my_slot
    }

    pub fn active(&self) -> (u32, &BTreeSet<u8>) {
        (self.active_gen, &self.active)
    }

    /// Downstream datagrams accepted per slot.
    pub fn heard(&self, slot: u8) -> u64 {
        self.heard.get(&slot).copied().unwrap_or(0)
    }

    /// Downstream datagrams dropped (malformed, or a slot not in the Roster).
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// The wiring routes this device's own host engine through `ConnId::LOCAL`; a local
    /// participant gets no heartbeats and never times its host out.
    pub fn set_local(&mut self, local: bool) {
        self.local = local;
    }

    /// Local mute (§3.4 personal block, §6 spoofer). The host is not told.
    pub fn set_muted(&mut self, did: &str, muted: bool) {
        if muted {
            self.muted.insert(did.into());
        } else {
            self.muted.remove(did);
        }
    }

    /// A control frame from the host connection.
    pub fn on_control(&mut self, msg: GroupCallMsg, now: u64, auth: &dyn CallAuthority) -> Result<Vec<ClientAction>, Refusal> {
        if matches!(self.state, ClientState::Ended(_)) {
            return Err(Refusal::Unknown);
        }
        match msg {
            GroupCallMsg::Heartbeat { .. } => {
                self.last_hb_ms = now;
                Ok(vec![])
            }
            GroupCallMsg::Start { start } => self.on_start(&start, auth),
            GroupCallMsg::Roster { roster } => self.on_roster(&roster, now, auth),
            GroupCallMsg::Active { r#gen, slots } => self.on_active(r#gen, slots),
            GroupCallMsg::End { end } => self.on_end(&end, now, auth),
            GroupCallMsg::Join { .. } | GroupCallMsg::Leave { .. } => Err(Refusal::Malformed),
        }
    }

    // The CallStart of the generation we dialed, sent by the host after our Join.
    fn on_start(&mut self, blob: &SignedBlob, auth: &dyn CallAuthority) -> Result<Vec<ClientAction>, Refusal> {
        let s = w::open_start(blob)?;
        if s.gid != self.start.gid {
            return Err(Refusal::WrongGroup);
        }
        if s.call_id != self.start.call_id {
            return Err(Refusal::WrongCall);
        }
        if s.host_gen < self.expected_gen {
            return Err(Refusal::StaleGen);
        }
        if s.host_gen != self.expected_gen || s.host_ref != self.host {
            return Err(Refusal::NotHost);
        }
        if !auth.host_permit_ok(&s.gid, &s.host_ref) {
            return Err(Refusal::PermitInvalid);
        }
        self.start = s;
        Ok(vec![])
    }

    fn check_host_msg(&self, gid: &str, call_id: &str, host_gen: u32, host_did: &str, auth: &dyn CallAuthority) -> Result<(), Refusal> {
        if gid != self.start.gid {
            return Err(Refusal::WrongGroup);
        }
        if call_id != self.start.call_id {
            return Err(Refusal::WrongCall);
        }
        if host_gen < self.expected_gen {
            return Err(Refusal::StaleGen);
        }
        if host_gen != self.expected_gen {
            return Err(Refusal::WrongCall);
        }
        if host_did != self.host.holder_did {
            return Err(Refusal::NotHost);
        }
        if !auth.host_permit_ok(gid, &self.host) {
            return Err(Refusal::PermitInvalid);
        }
        Ok(())
    }

    fn on_roster(&mut self, blob: &SignedBlob, _now: u64, auth: &dyn CallAuthority) -> Result<Vec<ClientAction>, Refusal> {
        let r = w::open_roster(blob)?;
        self.check_host_msg(&r.gid, &r.call_id, r.host_gen, &r.host_did, auth)?;
        if r.epoch <= self.epoch {
            return Err(Refusal::Stale);
        }
        // G10: a Roster listing a non-member (e.g. a removed DID kept by a rogue host) is refused.
        if r.entries.iter().any(|e| !auth.is_member(&r.gid, &e.did)) {
            return Err(Refusal::NotMember);
        }
        let roster: BTreeMap<u8, String> = r.entries.iter().map(|e| (e.slot, e.did.clone())).collect();
        let mine = roster.iter().find(|(_, d)| *d == self.id.did()).map(|(s, _)| *s);
        let mut out = vec![];
        match (mine, self.state) {
            (None, ClientState::Joined) => {
                self.state = ClientState::Ended(ClientEnd::Dropped);
                return Ok(vec![ClientAction::Ended(ClientEnd::Dropped)]);
            }
            (None, _) => return Ok(vec![]), // not yet in it
            (Some(slot), ClientState::Dialing | ClientState::Redialing) => {
                self.state = ClientState::Joined;
                self.deadline = None;
                self.candidates.clear();
                self.my_slot = Some(slot);
                out.push(ClientAction::Joined { slot });
            }
            (Some(slot), _) => self.my_slot = Some(slot),
        }
        // A slot whose DID changed (or that left) gets a fresh decoder.
        self.receivers.retain(|s, _| roster.get(s).is_some_and(|d| self.roster.get(s) == Some(d)));
        self.epoch = r.epoch;
        self.roster = roster;
        out.push(ClientAction::RosterChanged);
        Ok(out)
    }

    fn on_active(&mut self, r#gen: u32, slots: Vec<u8>) -> Result<Vec<ClientAction>, Refusal> {
        if r#gen <= self.active_gen {
            return Err(Refusal::Stale);
        }
        self.active_gen = r#gen;
        self.active = slots.into_iter().collect();
        for (s, r) in self.receivers.iter_mut() {
            r.set_active(self.active.contains(s));
        }
        Ok(vec![])
    }

    fn on_end(&mut self, blob: &SignedBlob, now: u64, auth: &dyn CallAuthority) -> Result<Vec<ClientAction>, Refusal> {
        let e = w::open_end(blob)?;
        if e.host_gen > self.expected_gen {
            return Err(Refusal::Unknown);
        }
        self.check_host_msg(&e.gid, &e.call_id, e.host_gen, &e.host_did, auth)?;
        let end = |s: &mut Self, why| {
            s.state = ClientState::Ended(why);
            Ok(vec![ClientAction::Ended(why)])
        };
        match (e.reason, e.next_host) {
            (EndReason::Removed, _) => end(self, ClientEnd::Removed),
            (EndReason::Superseded, Some(by)) => {
                self.state = ClientState::Ended(ClientEnd::Superseded);
                Ok(vec![ClientAction::Superseded { by }, ClientAction::Ended(ClientEnd::Superseded)])
            }
            // §5 planned exit: redial at once, no heartbeat wait.
            (EndReason::HostLeaving | EndReason::HostRotating, Some(next)) => {
                let mut c = vec![next.clone()];
                c.extend(self.start.backups.iter().filter(|b| **b != next && **b != self.host).cloned());
                Ok(self.redial(c, now, auth))
            }
            _ => end(self, ClientEnd::Ended),
        }
    }

    /// Call at least every second (every 20 ms is fine). Detects host loss (3 missed
    /// heartbeats or a revoked host permit) and redial timeouts.
    pub fn tick(&mut self, now: u64, auth: &dyn CallAuthority) -> Vec<ClientAction> {
        match self.state {
            ClientState::Joined => {
                let lost = !self.local && now >= self.last_hb_ms + HEARTBEAT_MISSES * HEARTBEAT_MS;
                if lost || !auth.host_permit_ok(&self.start.gid, &self.host) {
                    let c = self.start.backups.iter().filter(|b| **b != self.host).cloned().collect();
                    return self.redial(c, now, auth);
                }
                vec![]
            }
            ClientState::Dialing | ClientState::Redialing if self.deadline.is_some_and(|d| now >= d) => self.next_candidate(now, auth),
            _ => vec![],
        }
    }

    /// The IO layer could not reach the host being dialed: try the next candidate now.
    pub fn dial_failed(&mut self, now: u64, auth: &dyn CallAuthority) -> Vec<ClientAction> {
        match self.state {
            ClientState::Dialing | ClientState::Redialing => self.next_candidate(now, auth),
            _ => vec![],
        }
    }

    /// The host closed the connection. `answered_elsewhere`: its close reason said so.
    pub fn on_closed(&mut self, answered_elsewhere: bool, now: u64, auth: &dyn CallAuthority) -> Vec<ClientAction> {
        if answered_elsewhere {
            self.state = ClientState::Ended(ClientEnd::AnsweredElsewhere);
            return vec![ClientAction::Ended(ClientEnd::AnsweredElsewhere)];
        }
        match self.state {
            ClientState::Joined => {
                let c = self.start.backups.iter().filter(|b| **b != self.host).cloned().collect();
                self.redial(c, now, auth)
            }
            ClientState::Dialing | ClientState::Redialing => self.next_candidate(now, auth),
            ClientState::Ended(_) => vec![],
        }
    }

    /// Leave the call (sends a signed Leave).
    pub fn leave(&mut self, now: u64) -> Vec<ClientAction> {
        if matches!(self.state, ClientState::Ended(_)) {
            return vec![];
        }
        let l = w::sign_leave(&self.id, &self.start.gid, &self.start.call_id, self.expected_gen, now);
        self.state = ClientState::Ended(ClientEnd::Left);
        vec![ClientAction::Control(frame(&GroupCallMsg::Leave { leave: l })), ClientAction::Ended(ClientEnd::Left)]
    }

    // §5: Join{host_gen + 1} at each candidate in turn, ANSWER_TIMEOUT_MS each, skipping
    // revoked permits.
    fn redial(&mut self, candidates: Vec<HostRef>, now: u64, auth: &dyn CallAuthority) -> Vec<ClientAction> {
        self.expected_gen = self.start.host_gen + 1;
        self.candidates = candidates;
        self.epoch = 0;
        self.active_gen = 0;
        self.active.clear();
        self.receivers.clear();
        self.my_slot = None;
        self.local = false;
        self.next_candidate(now, auth)
    }

    fn next_candidate(&mut self, now: u64, auth: &dyn CallAuthority) -> Vec<ClientAction> {
        while !self.candidates.is_empty() {
            let h = self.candidates.remove(0);
            if !auth.host_permit_ok(&self.start.gid, &h) {
                continue;
            }
            self.host = h.clone();
            self.state = ClientState::Redialing;
            self.deadline = Some(now + ANSWER_TIMEOUT_MS);
            self.last_hb_ms = now;
            return vec![ClientAction::Dial { host: h }, self.join_frame(now)];
        }
        self.deadline = None;
        self.state = ClientState::Ended(ClientEnd::HostLost);
        vec![ClientAction::Ended(ClientEnd::HostLost)]
    }

    fn join_frame(&mut self, now: u64) -> ClientAction {
        let (_, join) = w::sign_join(&self.id, &self.start.gid, &self.start.call_id, self.expected_gen, now);
        ClientAction::Control(frame(&GroupCallMsg::Join { join, note: self.note.clone() }))
    }

    /// A downstream datagram `[0x02][slot][audio]`. Slots not in the Roster are dropped (§6).
    pub fn on_datagram(&mut self, bytes: &[u8], now: u64) {
        if self.state != ClientState::Joined {
            return;
        }
        let Ok((slot, a)) = w::parse_down(bytes) else {
            self.dropped += 1;
            return;
        };
        if !self.roster.contains_key(&slot) || Some(slot) == self.my_slot {
            self.dropped += 1;
            return;
        }
        *self.heard.entry(slot).or_default() += 1;
        if !self.receivers.contains_key(&slot) {
            let Ok(mut r) = audio::Receiver::new_group() else { return };
            r.set_active(self.active_gen == 0 || self.active.contains(&slot));
            self.receivers.insert(slot, r);
        }
        if let Some(r) = self.receivers.get_mut(&slot) {
            r.push(a, now);
        }
    }

    /// One 20 ms microphone frame through the silence gate (G6-D1): Opus while speaking,
    /// `[0x03][level]` every `LEVEL_ONLY_MS` while gated.
    pub fn on_mic(&mut self, pcm: &[i16; audio::FRAME]) -> Vec<ClientAction> {
        let Ok(f) = self.sender.encode_group(pcm) else { return vec![] };
        if self.state != ClientState::Joined {
            return vec![];
        }
        match f {
            audio::GroupFrame::Opus(d) => {
                self.gated_frames = 0;
                vec![ClientAction::Datagram(w::up_audio(&d))]
            }
            audio::GroupFrame::Gated { level } => {
                self.gated_frames += 1;
                if self.gated_frames.is_multiple_of(LEVEL_ONLY_FRAMES) {
                    vec![ClientAction::Datagram(w::up_level(level).to_vec())]
                } else {
                    vec![]
                }
            }
        }
    }

    /// Pull 20 ms from every slot's jitter buffer and mix the ones not muted (§1.4).
    pub fn mix(&mut self, out: &mut [i16; audio::FRAME], now: u64) {
        // One fixed scratch frame and integer sum instead of per-slot boxed PCM/vectors.
        // At most MAX_PARTICIPANTS sources: the sum is safely inside i32's range.
        let mut sums = [0i32; audio::FRAME];
        let mut frame = [0i16; audio::FRAME];
        for (slot, r) in self.receivers.iter_mut() {
            // Pull even muted slots so their decoder clocks advance; pull overwrites the
            // entire scratch frame, including silence/idle/error paths.
            r.pull(&mut frame, now);
            if self.roster.get(slot).is_some_and(|d| !self.muted.contains(d)) {
                for (sum, sample) in sums.iter_mut().zip(frame.iter()) {
                    *sum += *sample as i32;
                }
            }
        }
        audio::mix_sums(&sums, out);
    }
}
