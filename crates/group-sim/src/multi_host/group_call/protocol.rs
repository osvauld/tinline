use super::*;

// ============================================================================================
// Tunables. RECOMMENDED DEFAULTS from group-call-spec.md §8, PENDING THE USER'S ANSWERS.
// Tests read these names and never hard-code the values.
// ============================================================================================
/// Q1: participant cap for a desktop or headless host (the launch cap).
pub(super) const CAP_DESKTOP: usize = 12;
/// Q1: phone host on unmetered Wi-Fi.
pub(super) const CAP_PHONE_WIFI: usize = 8;
/// Q1: phone host on metered/mobile data.
pub(super) const CAP_PHONE_METERED: usize = 4;
/// Q2: speakers forwarded by a desktop or headless host (announced in `CallStart`).
pub(super) const K_DESKTOP: usize = 3;
/// Q2: speakers forwarded by a phone host.
pub(super) const K_PHONE: usize = 2;
/// Q3: Opus bitrate for group calls. 1:1 calls keep 48 kbps (`node.rs:2388`).
pub(super) const GROUP_BITRATE_BPS: u64 = 32_000;
/// Q9: how long clients redial automatically after losing the host, before "Call ended".
pub(super) const REDIAL_WINDOW_MS: u64 = 10_000;
/// Q12: longest a U12 host rotation may be deferred while the host is hosting a call.
pub(super) const ROTATION_DEFER_CAP_MS: u64 = 3_600_000;
/// §5: heartbeat period from H, and the number of missed heartbeats that means "host lost".
pub(super) const HEARTBEAT_MS: u64 = 1_000;
pub(super) const HEARTBEAT_MISSES: u64 = 3;
/// §6: upstream datagrams per second per member.
pub(super) const MAX_UP_PPS: u64 = 60;
/// §2.2: lifetime of a `CallHostStatus`.
pub(super) const STATUS_TTL_MS: u64 = 60_000;

// Other proposed spec values [P] that are not §8 questions. The section is in brackets.
pub(super) const UP_BURST: u64 = 10; // [§6]
pub(super) const MAX_DATAGRAM: usize = 1200; // [§6]
pub(super) const MAX_CONTROL: usize = 4096; // [§6]
pub(super) const MAX_CONTROL_PER_S: usize = 10; // [§6]
pub(super) const JOIN_MIN_GAP_MS: u64 = 2_000; // [§6] at most 1 Join per 2 s per DID
pub(super) const ANSWER_TIMEOUT_MS: u64 = 5_000; // [§2.3] candidate answer; [§5] per-backup redial
pub(super) const MEMBER_DROP_MS: u64 = 5_000; // [§5]
pub(super) const MAX_BACKUPS: usize = 2; // [§2.3]
pub(super) const FRAME_MS: u64 = 20; // Opus frame (crates/audio)
pub(super) const SPEECH_DBOV: u32 = 50; // [§1.3] speech threshold, -dBov (lower = louder)
pub(super) const AVG_FRAMES: usize = 15; // [§1.3] 300 ms moving average
pub(super) const HYST_DB: u32 = 6; // [§1.3]
pub(super) const HYST_FRAMES: u32 = 10; // [§1.3] 200 ms
pub(super) const DOM_WINDOW: usize = 3_000; // [§6] rolling 60 s
pub(super) const DOM_SHARE_PCT: usize = 90; // [§6]
pub(super) const DOM_DEMOTE_FRAMES: u64 = 500; // [§6] 10 s
pub(super) const BATTERY_TIER2_PCT: u8 = 50; // [§2.3]
pub(super) const BATTERY_ADVERTISE_PCT: u8 = 30; // [§2.3, §2.4]
pub(super) const BATTERY_LIMIT_PCT: u8 = 15; // [§2.4]
pub(super) const BATTERY_END_PCT: u8 = 8; // [§2.4]
pub(super) const ROTATION_DELAY_MS: u64 = 2 * 86_400_000; // U12 detail (decided): 2 days
// [§4, A] Wire bytes per forwarded stream, apart from the Opus payload.
pub(super) const DRED_BYTES: u64 = 5;
pub(super) const AUDIO_HDR: u64 = 5;
pub(super) const GROUP_HDR: u64 = 2;
pub(super) const QUIC_BYTES: u64 = 30;
pub(super) const UDP_IP4: u64 = 28;

pub(super) const CALL_DOMAIN: &[u8] = b"tinline/group-sim/gcall/not-production/v1\0";
pub(super) const STATUS_DOMAIN: &[u8] = b"tinline/group-sim/gcall-status/not-production/v1\0";
pub(super) const UP: u8 = 0x01;
pub(super) const DOWN: u8 = 0x02;
// G6-D1 (decided): a gated sender's level-only datagram `[0x03][level]`. H uses it for selection
// and never forwards it.
pub(super) const LEVEL_ONLY: u8 = 0x03;
pub(super) const GATE_FRAMES: u32 = 15; // G6-D1: stop sending Opus after 300 ms below the threshold
pub(super) const LEVEL_ONLY_FRAMES: u32 = 25; // G6-D1: one level-only datagram every 500 ms while gated

// Places where the spec is silent or ambiguous and this model had to pick something. Each
// entry is also listed in the README and in the report.
#[allow(dead_code)]
pub(super) const ASSUMPTIONS: &[&str] = &[
    "G1: CallStart does not carry the host_call permit. Members get it in a call-host record (note + permit) shipped next to the CallStart (`Announce`).",
    "G2: A call host that is not a chat host has no endpoint directory. Join therefore carries the joiner's DID-signed group note. H checks it against the connecting endpoint, and against its chat directory when it has one.",
    "G3: The moving average uses only datagrams that arrive inside the 300 ms window. With no datagram in the window the sender counts as silent.",
    "G4: The dominance cap counts frames where the sender holds a slot while at least K other senders are eligible. The spec says 'K-1 others', but then there is no contention.",
    "G5: 'Newer Join' means a larger issued_ms (signed). A Join with an older or equal issued_ms from the same DID is refused as Stale.",
    "G6: The tie-break 'lower busy/load' is a no-op, because busy candidates are already filtered out and the status has no load field.",
    "G7: Tiers are missing for a phone on battery at 30-49 % unmetered, and for a desktop on battery or metered. Both are ranked after tier 3 (TIER_UNLISTED).",
    "G8: 'Starter itself' matches on DID. A starter with two candidate devices (desktop and phone) cannot be told apart by the other members.",
    "G9: After a takeover, the CallStart{gen+1} carries backups = the previous backups minus the new host.",
    "G10: Members also check that every Roster entry's DID is a member and not removed. This makes a rogue host that keeps a removed DID in the Roster detectable.",
];

// ---- signed call control (§3.1) ------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) struct HostRef {
    pub(super) holder_did: String,
    pub(super) device_slot: u64,
    pub(super) permit_digest: String, // no transport key (U4 revised)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum EndReason {
    Ended,
    HostLeaving,
    HostRotating,
    Removed,
    Superseded,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct RosterEntry {
    pub(super) slot: u8,
    pub(super) did: String,
    pub(super) join: Ctl,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Kind {
    Start {
        host_ref: HostRef,
        starter_did: String,
        started_ms: u64,
        k: u8,
        max_participants: u8,
        backups: Vec<HostRef>,
        meta_frontier: String,
    },
    Join {
        issued_ms: u64,
        nonce: u64,
    },
    Roster {
        epoch: u32,
        entries: Vec<RosterEntry>,
    },
    Leave {
        issued_ms: u64,
    },
    End {
        reason: EndReason,
        next_host: Option<HostRef>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct CallMsg {
    pub(super) v: u8,
    pub(super) gid: String,
    pub(super) call_id: String, // 16 random bytes, hex
    pub(super) signer: String,
    pub(super) host_gen: u32,
    pub(super) kind: Kind,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Ctl {
    pub(super) msg: CallMsg,
    pub(super) sig: Vec<u8>,
}
impl Ctl {
    pub(super) fn sign(id: &Identity, msg: CallMsg) -> Self {
        let sig = id.sign(&signed(CALL_DOMAIN, &msg)).to_vec();
        Self { msg, sig }
    }
    pub(super) fn verify(&self) -> bool {
        valid(&self.msg.signer, &signed(CALL_DOMAIN, &self.msg), &self.sig)
    }
    pub(super) fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
    pub(super) fn id(&self) -> String {
        digest(self)
    }
    pub(super) fn start(&self) -> Option<(&HostRef, &Vec<HostRef>)> {
        match &self.msg.kind {
            Kind::Start {
                host_ref, backups, ..
            } => Some((host_ref, backups)),
            _ => None,
        }
    }
}
// ASSUMPTION(G1): the CallStart travels with the host's call-host record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Announce {
    pub(super) start: Ctl,
    pub(super) record: HostRecord,
}
// ASSUMPTION(G2): the Join travels with the joiner's DID-signed group note.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct JoinReq {
    pub(super) join: Ctl,
    pub(super) note: EpNote,
}
// G6-D1: unsigned (H controls the media anyway, U14); sent whenever S changes; the gen only
// increases.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Active {
    pub(super) r#gen: u32,
    pub(super) slots: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Heartbeat {
    pub(super) epoch: u32,
}

// ---- host conditions, status, ranking (§2.2-§2.4) ------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Class {
    Desktop,
    Headless,
    Phone,
}
// Android thermal status order (PowerManager.THERMAL_STATUS_*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) enum Thermal {
    None,
    Light,
    Moderate,
    Severe,
    Critical,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Power {
    Mains,
    Battery(u8),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Cond {
    pub(super) class: Class,
    pub(super) plugged: bool,
    pub(super) battery: u8,
    pub(super) metered: bool,
    pub(super) metered_override: bool,
    pub(super) thermal: Thermal,
}
impl Cond {
    pub(super) fn desktop() -> Self {
        Self {
            class: Class::Desktop,
            plugged: true,
            battery: 100,
            metered: false,
            metered_override: false,
            thermal: Thermal::None,
        }
    }
    pub(super) fn phone(battery: u8, plugged: bool) -> Self {
        Self {
            class: Class::Phone,
            plugged,
            battery,
            ..Self::desktop()
        }
    }
    pub(super) fn cap(&self) -> usize {
        match (self.class, self.metered) {
            (Class::Phone, true) => CAP_PHONE_METERED,
            (Class::Phone, false) => CAP_PHONE_WIFI,
            _ => CAP_DESKTOP,
        }
    }
    pub(super) fn k(&self) -> usize {
        if self.class == Class::Phone {
            K_PHONE
        } else {
            K_DESKTOP
        }
    }
}
impl Default for Cond {
    fn default() -> Self {
        Self::desktop()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) enum PhoneAction {
    Advertise,
    StopAdvertising,
    Limited,
    End,
}
// §2.4 cutoffs table, as a pure function. "User ends hosting" is a separate trigger.
pub(super) fn phone_policy(c: &Cond) -> PhoneAction {
    if c.class != Class::Phone {
        return PhoneAction::Advertise;
    }
    let on_battery = !c.plugged;
    if c.thermal >= Thermal::Critical || (on_battery && c.battery < BATTERY_END_PCT) {
        PhoneAction::End
    } else if c.thermal >= Thermal::Severe || (on_battery && c.battery < BATTERY_LIMIT_PCT) {
        PhoneAction::Limited
    } else if c.thermal >= Thermal::Moderate
        || (on_battery && c.battery < BATTERY_ADVERTISE_PCT)
        || (c.metered && !c.metered_override)
    {
        PhoneAction::StopAdvertising
    } else {
        PhoneAction::Advertise
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct StatusBody {
    pub(super) v: u8,
    pub(super) gid: String,
    pub(super) host_ref: HostRef,
    pub(super) class: Class,
    pub(super) power: Power,
    pub(super) metered: bool,
    pub(super) thermal_ok: bool,
    pub(super) max_participants: u8,
    pub(super) busy: bool,
    pub(super) issued_ms: u64,
    pub(super) ttl_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Status {
    pub(super) body: StatusBody,
    pub(super) sig: Vec<u8>,
}
impl Status {
    pub(super) fn verify(&self) -> bool {
        valid(
            &self.body.host_ref.holder_did,
            &signed(STATUS_DOMAIN, &self.body),
            &self.sig,
        )
    }
}

pub(super) const TIER_UNLISTED: u8 = 4; // ASSUMPTION(G7)
pub(super) fn tier(s: &StatusBody, expected: usize) -> Option<u8> {
    if !s.thermal_ok || s.busy || (s.max_participants as usize) < expected {
        return None;
    }
    match (s.class, s.power, s.metered) {
        (Class::Phone, Power::Battery(p), _) if p < BATTERY_ADVERTISE_PCT => None,
        (Class::Phone, _, true) => (expected <= CAP_PHONE_METERED).then_some(3),
        (Class::Phone, Power::Mains, false) => Some(1),
        (Class::Phone, Power::Battery(p), false) if p >= BATTERY_TIER2_PCT => Some(2),
        (Class::Phone, Power::Battery(_), false) => Some(TIER_UNLISTED),
        (_, Power::Mains, false) => Some(0),
        _ => Some(TIER_UNLISTED),
    }
}
pub(super) fn tie_hash(call_id: &str, r: &HostRef) -> [u8; 32] {
    *blake3::hash(&[call_id.as_bytes(), &serde_json::to_vec(r).unwrap()].concat()).as_bytes()
}
pub(super) fn call_hash(call_id: &str) -> [u8; 32] {
    *blake3::hash(call_id.as_bytes()).as_bytes()
}
// §2.3 ranking. Input: statuses that have already been verified. Tie-breaks: (1) the starter's
// DID when it is in the best tier (ASSUMPTION G8); (2) no-op (ASSUMPTION G6);
// (3) lowest BLAKE3(call_id || host_ref).
pub(super) fn rank(
    st: &[StatusBody],
    call_id: &str,
    starter: &str,
    expected: usize,
    now: u64,
) -> Vec<HostRef> {
    let mut latest: BTreeMap<&HostRef, &StatusBody> = BTreeMap::new();
    for s in st {
        let fresh = s.issued_ms <= now && now < s.issued_ms + s.ttl_ms.min(STATUS_TTL_MS);
        if fresh
            && latest
                .get(&s.host_ref)
                .is_none_or(|c| c.issued_ms < s.issued_ms)
        {
            latest.insert(&s.host_ref, s);
        }
    }
    let tiered: Vec<(u8, &StatusBody)> = latest
        .values()
        .filter_map(|s| tier(s, expected).map(|t| (t, *s)))
        .collect();
    let Some(best) = tiered.iter().map(|(t, _)| *t).min() else {
        return vec![];
    };
    let mut v: Vec<_> = tiered
        .into_iter()
        .map(|(t, s)| {
            let starter_first = !(t == best && s.host_ref.holder_did == starter);
            (
                (t, starter_first, tie_hash(call_id, &s.host_ref)),
                s.host_ref.clone(),
            )
        })
        .collect();
    v.sort();
    v.into_iter().map(|(_, r)| r).collect()
}

// ---- forwarder: pure function over sender-reported levels (§1.3, §6) -----------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Fwd {
    pub(super) k: usize,
    pub(super) hysteresis: bool,
    pub(super) dominance_cap: bool,
    pub(super) frame: u64,
    pub(super) hist: BTreeMap<u8, VecDeque<Option<u8>>>,
    pub(super) set: BTreeSet<u8>,
    pub(super) challenge: BTreeMap<u8, u32>,
    pub(super) held: BTreeMap<u8, VecDeque<bool>>,
    pub(super) demoted_until: BTreeMap<u8, u64>,
    pub(super) lowered: BTreeSet<u8>, // host user's "lower priority" (§6)
    // G6-D2: a provisional slot K+1 for a new speaker (slot, since frame, level sum, count),
    // and senders whose provisional slot was dropped (no new one until they go quiet).
    pub(super) provisional: bool,
    pub(super) prov: Option<(u8, u64, u32, u32)>,
    pub(super) denied: BTreeSet<u8>,
}
impl Fwd {
    pub(super) fn new(k: usize) -> Self {
        Self {
            k,
            hysteresis: true,
            dominance_cap: true,
            frame: 0,
            hist: BTreeMap::new(),
            set: BTreeSet::new(),
            challenge: BTreeMap::new(),
            held: BTreeMap::new(),
            demoted_until: BTreeMap::new(),
            lowered: BTreeSet::new(),
            provisional: true,
            prov: None,
            denied: BTreeSet::new(),
        }
    }
    // G6-D2: what H forwards and announces in `Active`: S plus the provisional slot.
    pub(super) fn forwarded(&self) -> BTreeSet<u8> {
        let mut f = self.set.clone();
        f.extend(self.prov.map(|p| p.0));
        f
    }
    // ASSUMPTION(G3): the mean of the levels that arrived in the window. 127 means silent.
    pub(super) fn avg(&self, s: u8) -> u32 {
        let got: Vec<u32> = self.hist[&s].iter().flatten().map(|l| *l as u32).collect();
        if got.is_empty() {
            127
        } else {
            got.iter().sum::<u32>() / got.len() as u32
        }
    }
    pub(super) fn low(&self, s: u8) -> bool {
        self.lowered.contains(&s) || self.demoted_until.get(&s).is_some_and(|&t| t > self.frame)
    }
}
// One 20 ms frame. `levels` maps each slot that sent a datagram this frame to its level.
// Returns the next state; `prev` is not changed.
pub(super) fn select(prev: &Fwd, levels: &BTreeMap<u8, u8>) -> Fwd {
    let mut n = prev.clone();
    // G6-D2: a sender outside S at speech level gets the free provisional slot at once (H does
    // this on the datagram itself, before the frame ends).
    for (&s, &l) in levels {
        if n.provisional
            && (l as u32) < SPEECH_DBOV
            && !n.set.contains(&s)
            && n.prov.is_none()
            && !n.low(s)
            && !n.denied.contains(&s)
        {
            n.prov = Some((s, n.frame, 0, 0));
        }
    }
    n.frame += 1;
    let senders: BTreeSet<u8> = n.hist.keys().chain(levels.keys()).copied().collect();
    for s in &senders {
        let h = n.hist.entry(*s).or_default();
        h.push_back(levels.get(s).copied());
        while h.len() > AVG_FRAMES {
            h.pop_front();
        }
    }
    let avg: BTreeMap<u8, u32> = senders.iter().map(|s| (*s, n.avg(*s))).collect();
    let eligible: BTreeSet<u8> = senders
        .iter()
        .copied()
        .filter(|s| avg[s] < SPEECH_DBOV)
        .collect();
    n.set.retain(|s| eligible.contains(s));
    let key = |n: &Fwd, s: u8| (n.low(s), avg[&s], s);
    // Fill free slots in priority order. Demoted or lowered senders come last.
    let mut waiting: Vec<u8> = eligible.difference(&n.set).copied().collect();
    waiting.sort_by_key(|s| key(&n, *s));
    for s in waiting {
        if n.set.len() < n.k {
            n.set.insert(s);
        }
    }
    // Replacement of the weakest member of S by the best waiting sender.
    loop {
        let mut waiting: Vec<u8> = eligible.difference(&n.set).copied().collect();
        waiting.sort_by_key(|s| key(&n, *s));
        let Some(&c) = waiting.first() else {
            n.challenge.clear();
            break;
        };
        let q = *n.set.iter().max_by_key(|s| key(&n, **s)).unwrap();
        let (cl, ql) = (n.low(c), n.low(q));
        let swap = if cl {
            false // a demoted or lowered sender never displaces anyone
        } else if ql {
            true
        } else if !n.hysteresis {
            avg[&c] < avg[&q]
        } else if avg[&c] + HYST_DB <= avg[&q] {
            let run = n.challenge.get(&c).copied().unwrap_or(0) + 1;
            n.challenge = BTreeMap::from([(c, run)]);
            run >= HYST_FRAMES
        } else {
            n.challenge.clear();
            false
        };
        if !swap {
            break;
        }
        n.set.remove(&q);
        n.set.insert(c);
        n.challenge.clear();
        if n.hysteresis {
            break; // at most one hysteresis swap per frame
        }
    }
    // Dominance cap (§6). ASSUMPTION(G4): contended = at least K other eligible senders.
    if n.dominance_cap {
        for s in &senders {
            let contended = eligible.iter().filter(|x| *x != s).count() >= n.k;
            let holds = n.set.contains(s);
            let h = n.held.entry(*s).or_default();
            h.push_back(holds && contended);
            while h.len() > DOM_WINDOW {
                h.pop_front();
            }
            let count = h.iter().filter(|b| **b).count();
            if holds && contended && count * 100 > DOM_WINDOW * DOM_SHARE_PCT {
                n.demoted_until.insert(*s, n.frame + DOM_DEMOTE_FRAMES);
                n.held.get_mut(s).unwrap().clear();
            }
        }
    }
    // G6-D2: after HYST_FRAMES the provisional slot replaces the weakest member of S if its
    // spurt was louder by HYST_DB (or S has room), else it is dropped. The normal rule may
    // promote it earlier.
    if let Some((p, since, mut sum, mut cnt)) = n.prov.take() {
        if !n.set.contains(&p) {
            if let Some(l) = levels.get(&p) {
                sum += *l as u32;
                cnt += 1;
            }
            if n.frame - since < HYST_FRAMES as u64 {
                n.prov = Some((p, since, sum, cnt));
            } else {
                let spurt = if cnt == 0 { 127 } else { sum / cnt };
                let out = if n.low(p) {
                    None
                } else if n.set.len() < n.k {
                    (spurt < SPEECH_DBOV).then_some(None)
                } else {
                    let q = *n.set.iter().max_by_key(|s| key(&n, **s)).unwrap();
                    (n.low(q) || spurt + HYST_DB <= avg[&q]).then_some(Some(q))
                };
                match out {
                    Some(q) => {
                        if let Some(q) = q {
                            n.set.remove(&q);
                        }
                        n.set.insert(p);
                        n.challenge.clear();
                    }
                    None => {
                        n.denied.insert(p);
                    }
                }
            }
        }
    }
    n.denied
        .retain(|s| avg.get(s).is_some_and(|a| *a < SPEECH_DBOV));
    n
}
