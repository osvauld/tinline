//! G6: group calls relayed through one call host (U21). Simulation tests GC-S01..GC-S17 from
//! `docs/design/group-call-spec.md` §7.
//!
//! Status: tests first. Simulation only; no production API, wire format, transport, audio or UI.
//! This file is a child module of `multi_host.rs`, so it reuses that module's `World` without
//! widening visibility. The reused parts are the signed genesis and grants, DID-signed group
//! endpoint notes, host records and the chat-host directory (U7), admin-signed ops, U9 flag
//! removal, `rotate_host`/`rotate_member` (U12), U5-revised block notices and the rogue-host
//! fixture.
//!
//! Model (test-only):
//! - A call-host permit is the multi-host permit leaf with `Cap::CallHost` (§2.1), issued by the
//!   group creator and bound to one device slot. A call-host record is `HostRecord { note,
//!   permit }`. Members accept it like a chat-host record, but they check `CallHost`.
//! - Signed call control (§3.1): `CallStart`, `Join`, `Roster`, `Leave` and `End` are signed by
//!   the DID with real Ed25519. They use a simulation domain that stands in for
//!   `tinline/gcall/v1\0`. The `Heartbeat` is unsigned, because it travels on the authenticated
//!   stream.
//! - The host's forwarder is the pure function `select` over RFC 6464 levels that the senders
//!   report (§1.3). It has a moving average, hysteresis and the §6 dominance cap.
//! - Ranking (§2.3) and the phone degrade policy (§2.4) are pure functions as well.
//! - Time is fake milliseconds. Whether a dial succeeds is decided when the dial is made. The
//!   5 s timeout per backup (§5) is modelled as elapsed time on the redial log, not as a
//!   running timer.
//! - The `CallStart` is relayed through the chat hosts as a group `Ephemeral` message. Here it
//!   is a frame from chat host `H2` to every device. `hold_relay` delays that relay so that
//!   splits and glare can be modelled.
//!
//! Spec gaps this model has to fill are listed in `ASSUMPTIONS` below. Each one is marked
//! `ASSUMPTION(Gx)` at the code that uses it.
use super::*;
use std::collections::VecDeque;

// ============================================================================================
// Tunables. RECOMMENDED DEFAULTS from group-call-spec.md §8, PENDING THE USER'S ANSWERS.
// Tests read these names and never hard-code the values.
// ============================================================================================
/// Q1: participant cap for a desktop or headless host (the launch cap).
const CAP_DESKTOP: usize = 12;
/// Q1: phone host on unmetered Wi-Fi.
const CAP_PHONE_WIFI: usize = 8;
/// Q1: phone host on metered/mobile data.
const CAP_PHONE_METERED: usize = 4;
/// Q2: speakers forwarded by a desktop or headless host (announced in `CallStart`).
const K_DESKTOP: usize = 3;
/// Q2: speakers forwarded by a phone host.
const K_PHONE: usize = 2;
/// Q3: Opus bitrate for group calls. 1:1 calls keep 48 kbps (`node.rs:2388`).
const GROUP_BITRATE_BPS: u64 = 32_000;
/// Q9: how long clients redial automatically after losing the host, before "Call ended".
const REDIAL_WINDOW_MS: u64 = 10_000;
/// Q12: longest a U12 host rotation may be deferred while the host is hosting a call.
const ROTATION_DEFER_CAP_MS: u64 = 3_600_000;
/// §5: heartbeat period from H, and the number of missed heartbeats that means "host lost".
const HEARTBEAT_MS: u64 = 1_000;
const HEARTBEAT_MISSES: u64 = 3;
/// §6: upstream datagrams per second per member.
const MAX_UP_PPS: u64 = 60;
/// §2.2: lifetime of a `CallHostStatus`.
const STATUS_TTL_MS: u64 = 60_000;

// Other proposed spec values [P] that are not §8 questions. The section is in brackets.
const UP_BURST: u64 = 10; // [§6]
const MAX_DATAGRAM: usize = 1200; // [§6]
const MAX_CONTROL: usize = 4096; // [§6]
const MAX_CONTROL_PER_S: usize = 10; // [§6]
const JOIN_MIN_GAP_MS: u64 = 2_000; // [§6] at most 1 Join per 2 s per DID
const ANSWER_TIMEOUT_MS: u64 = 5_000; // [§2.3] candidate answer; [§5] per-backup redial
const MEMBER_DROP_MS: u64 = 5_000; // [§5]
const MAX_BACKUPS: usize = 2; // [§2.3]
const FRAME_MS: u64 = 20; // Opus frame (crates/audio)
const SPEECH_DBOV: u32 = 50; // [§1.3] speech threshold, -dBov (lower = louder)
const AVG_FRAMES: usize = 15; // [§1.3] 300 ms moving average
const HYST_DB: u32 = 6; // [§1.3]
const HYST_FRAMES: u32 = 10; // [§1.3] 200 ms
const DOM_WINDOW: usize = 3_000; // [§6] rolling 60 s
const DOM_SHARE_PCT: usize = 90; // [§6]
const DOM_DEMOTE_FRAMES: u64 = 500; // [§6] 10 s
const BATTERY_TIER2_PCT: u8 = 50; // [§2.3]
const BATTERY_ADVERTISE_PCT: u8 = 30; // [§2.3, §2.4]
const BATTERY_LIMIT_PCT: u8 = 15; // [§2.4]
const BATTERY_END_PCT: u8 = 8; // [§2.4]
const ROTATION_DELAY_MS: u64 = 2 * 86_400_000; // U12 detail (decided): 2 days
// [§4, A] Wire bytes per forwarded stream, apart from the Opus payload.
const DRED_BYTES: u64 = 5;
const AUDIO_HDR: u64 = 5;
const GROUP_HDR: u64 = 2;
const QUIC_BYTES: u64 = 30;
const UDP_IP4: u64 = 28;

const CALL_DOMAIN: &[u8] = b"tinline/group-sim/gcall/not-production/v1\0";
const STATUS_DOMAIN: &[u8] = b"tinline/group-sim/gcall-status/not-production/v1\0";
const UP: u8 = 0x01;
const DOWN: u8 = 0x02;
// G6-D1 (decided): a gated sender's level-only datagram `[0x03][level]`. H uses it for selection
// and never forwards it.
const LEVEL_ONLY: u8 = 0x03;
const GATE_FRAMES: u32 = 15; // G6-D1: stop sending Opus after 300 ms below the threshold
const LEVEL_ONLY_FRAMES: u32 = 25; // G6-D1: one level-only datagram every 500 ms while gated

// Places where the spec is silent or ambiguous and this model had to pick something. Each
// entry is also listed in the README and in the report.
#[allow(dead_code)]
const ASSUMPTIONS: &[&str] = &[
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
struct HostRef {
    holder_did: String,
    device_slot: u64,
    permit_digest: String, // no transport key (U4 revised)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum EndReason {
    Ended,
    HostLeaving,
    HostRotating,
    Removed,
    Superseded,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RosterEntry {
    slot: u8,
    did: String,
    join: Ctl,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Kind {
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
struct CallMsg {
    v: u8,
    gid: String,
    call_id: String, // 16 random bytes, hex
    signer: String,
    host_gen: u32,
    kind: Kind,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Ctl {
    msg: CallMsg,
    sig: Vec<u8>,
}
impl Ctl {
    fn sign(id: &Identity, msg: CallMsg) -> Self {
        let sig = id.sign(&signed(CALL_DOMAIN, &msg)).to_vec();
        Self { msg, sig }
    }
    fn verify(&self) -> bool {
        valid(&self.msg.signer, &signed(CALL_DOMAIN, &self.msg), &self.sig)
    }
    fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
    fn id(&self) -> String {
        digest(self)
    }
    fn start(&self) -> Option<(&HostRef, &Vec<HostRef>)> {
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
struct Announce {
    start: Ctl,
    record: HostRecord,
}
// ASSUMPTION(G2): the Join travels with the joiner's DID-signed group note.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct JoinReq {
    join: Ctl,
    note: EpNote,
}
// G6-D1: unsigned (H controls the media anyway, U14); sent whenever S changes; the gen only
// increases.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Active {
    r#gen: u32,
    slots: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Heartbeat {
    epoch: u32,
}

// ---- host conditions, status, ranking (§2.2-§2.4) ------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Class {
    Desktop,
    Headless,
    Phone,
}
// Android thermal status order (PowerManager.THERMAL_STATUS_*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum Thermal {
    None,
    Light,
    Moderate,
    Severe,
    Critical,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Power {
    Mains,
    Battery(u8),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct Cond {
    class: Class,
    plugged: bool,
    battery: u8,
    metered: bool,
    metered_override: bool,
    thermal: Thermal,
}
impl Cond {
    fn desktop() -> Self {
        Self {
            class: Class::Desktop,
            plugged: true,
            battery: 100,
            metered: false,
            metered_override: false,
            thermal: Thermal::None,
        }
    }
    fn phone(battery: u8, plugged: bool) -> Self {
        Self {
            class: Class::Phone,
            plugged,
            battery,
            ..Self::desktop()
        }
    }
    fn cap(&self) -> usize {
        match (self.class, self.metered) {
            (Class::Phone, true) => CAP_PHONE_METERED,
            (Class::Phone, false) => CAP_PHONE_WIFI,
            _ => CAP_DESKTOP,
        }
    }
    fn k(&self) -> usize {
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
enum PhoneAction {
    Advertise,
    StopAdvertising,
    Limited,
    End,
}
// §2.4 cutoffs table, as a pure function. "User ends hosting" is a separate trigger.
fn phone_policy(c: &Cond) -> PhoneAction {
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
struct StatusBody {
    v: u8,
    gid: String,
    host_ref: HostRef,
    class: Class,
    power: Power,
    metered: bool,
    thermal_ok: bool,
    max_participants: u8,
    busy: bool,
    issued_ms: u64,
    ttl_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Status {
    body: StatusBody,
    sig: Vec<u8>,
}
impl Status {
    fn verify(&self) -> bool {
        valid(
            &self.body.host_ref.holder_did,
            &signed(STATUS_DOMAIN, &self.body),
            &self.sig,
        )
    }
}

const TIER_UNLISTED: u8 = 4; // ASSUMPTION(G7)
fn tier(s: &StatusBody, expected: usize) -> Option<u8> {
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
fn tie_hash(call_id: &str, r: &HostRef) -> [u8; 32] {
    *blake3::hash(&[call_id.as_bytes(), &serde_json::to_vec(r).unwrap()].concat()).as_bytes()
}
fn call_hash(call_id: &str) -> [u8; 32] {
    *blake3::hash(call_id.as_bytes()).as_bytes()
}
// §2.3 ranking. Input: statuses that have already been verified. Tie-breaks: (1) the starter's
// DID when it is in the best tier (ASSUMPTION G8); (2) no-op (ASSUMPTION G6);
// (3) lowest BLAKE3(call_id || host_ref).
fn rank(st: &[StatusBody], call_id: &str, starter: &str, expected: usize, now: u64) -> Vec<HostRef> {
    let mut latest: BTreeMap<&HostRef, &StatusBody> = BTreeMap::new();
    for s in st {
        let fresh = s.issued_ms <= now && now < s.issued_ms + s.ttl_ms.min(STATUS_TTL_MS);
        if fresh && latest.get(&s.host_ref).is_none_or(|c| c.issued_ms < s.issued_ms) {
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
            ((t, starter_first, tie_hash(call_id, &s.host_ref)), s.host_ref.clone())
        })
        .collect();
    v.sort();
    v.into_iter().map(|(_, r)| r).collect()
}

// ---- forwarder: pure function over sender-reported levels (§1.3, §6) -----------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Fwd {
    k: usize,
    hysteresis: bool,
    dominance_cap: bool,
    frame: u64,
    hist: BTreeMap<u8, VecDeque<Option<u8>>>,
    set: BTreeSet<u8>,
    challenge: BTreeMap<u8, u32>,
    held: BTreeMap<u8, VecDeque<bool>>,
    demoted_until: BTreeMap<u8, u64>,
    lowered: BTreeSet<u8>, // host user's "lower priority" (§6)
}
impl Fwd {
    fn new(k: usize) -> Self {
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
        }
    }
    // ASSUMPTION(G3): the mean of the levels that arrived in the window. 127 means silent.
    fn avg(&self, s: u8) -> u32 {
        let got: Vec<u32> = self.hist[&s].iter().flatten().map(|l| *l as u32).collect();
        if got.is_empty() {
            127
        } else {
            got.iter().sum::<u32>() / got.len() as u32
        }
    }
    fn low(&self, s: u8) -> bool {
        self.lowered.contains(&s) || self.demoted_until.get(&s).is_some_and(|&t| t > self.frame)
    }
}
// One 20 ms frame. `levels` maps each slot that sent a datagram this frame to its level.
// Returns the next state; `prev` is not changed.
fn select(prev: &Fwd, levels: &BTreeMap<u8, u8>) -> Fwd {
    let mut n = prev.clone();
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
    n
}
// Copies H sends: every datagram from a selected sender goes to every receiver except that
// sender (no self-echo).
fn copies(set: &BTreeSet<u8>, sent: &BTreeSet<u8>, receivers: &BTreeSet<u8>) -> Vec<(u8, u8)> {
    let mut v = vec![];
    for s in set.intersection(sent) {
        for r in receivers {
            if r != s {
                v.push((*s, *r));
            }
        }
    }
    v
}
// §1.4 client mix: at most K forwarded slots, minus locally muted DIDs.
fn mix(forwarded: &BTreeSet<u8>, roster: &BTreeMap<u8, String>, muted: &BTreeSet<String>) -> BTreeSet<u8> {
    forwarded
        .iter()
        .copied()
        .filter(|s| roster.get(s).is_some_and(|d| !muted.contains(d)))
        .collect()
}
fn audio(seq: u32, level: u8) -> Vec<u8> {
    let mut v = seq.to_be_bytes().to_vec();
    v.push(level);
    v.extend([0xA5u8; 120]); // stand-in Opus payload (H never decodes it)
    v
}

// ---- per-device call state -----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
enum CR {
    Malformed,
    Oversize,
    BadSignature,
    WrongGroup,
    WrongCall,
    StaleGen,
    NotMember,
    PermitInvalid,
    DeviceMismatch,
    Revoked,
    NotHost,
    EndpointMismatch,
    Replay,
    RateLimited,
    Full,
    Limited,
    Stale,
    Unknown,
    Superseded,
    Duplicate,
    Busy,
    Unreachable,
    NotParticipant,
    Refused(&'static str),
}

#[derive(Clone, Debug, Serialize)]
struct Part {
    slot: u8,
    join: Ctl,
    endpoint: String, // only the host holds participants' endpoints (U7)
    last_ms: u64,
}
#[derive(Clone, Debug, Serialize)]
struct HostCall {
    call_id: String,
    host_gen: u32,
    start: Ctl,
    k: usize,
    max: usize,
    epoch: u32,
    parts: BTreeMap<String, Part>, // DID -> participant (one device per DID)
    seen: BTreeSet<String>,        // Join ids (replay)
    last_join: BTreeMap<String, u64>,
    buckets: BTreeMap<String, (u64, u64)>, // DID -> (milli-tokens, last ms)
    ctl_times: BTreeMap<String, VecDeque<u64>>,
    limited: bool,
    fwd: Fwd,
    pending: BTreeMap<u8, Vec<Vec<u8>>>,
    levels: BTreeMap<u8, u8>,
    active_gen: u32,
    active: BTreeSet<u8>,
    level_only_rx: u64,
    dropped: BTreeMap<String, u64>,
    warned_metered: bool,
}
#[derive(Clone, Debug, Serialize)]
struct Dialing {
    call_id: String,
    host: HostRef,
    host_ep: String,
    host_gen: u32,
}
#[derive(Clone, Debug, Serialize)]
struct Joined {
    call_id: String,
    host: HostRef,
    host_ep: String,
    host_gen: u32,
    my_ep: String,
    epoch: u32,
    roster: BTreeMap<u8, String>, // slot -> DID; DIDs and slots only (U7)
    last_hb_ms: u64,
    heard: BTreeMap<u8, u64>,
    mixed: BTreeMap<String, u64>,
    dropped: u64,
    active_gen: u32,
    active: BTreeSet<u8>,
}
#[derive(Clone, Debug, Default, Serialize)]
struct CallDev {
    cond: Cond,
    consent: bool,
    records: BTreeMap<String, HostRecord>, // accepted call-host records
    revoked: BTreeSet<String>,             // revoked call-host permit digests
    rev_ops: BTreeSet<String>,
    hgen: BTreeMap<String, u32>,       // call_id -> highest host_gen seen
    starts: BTreeMap<String, Ctl>,    // call_id -> accepted CallStart at that hgen
    prev: BTreeMap<String, Ctl>,      // call_id -> CallStart of the previous hgen
    may_take: BTreeMap<String, u32>,  // call_id -> hgen this device may start on a Join
    shown: Option<String>,            // "Call in progress" (glare winner)
    dialing: Option<Dialing>,
    joined: Option<Joined>,
    last_call: Option<String>,
    muted: BTreeSet<String>,
    log: Vec<String>,
    hosting: Option<HostCall>,
    rotation_due_ms: Option<u64>,
    seq: u32,
    quiet: u32,
    gated: bool,
    since_level: u32,
    opus_sent: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum Redial {
    Joined { host: usize, at_ms: u64 },
    HostLost { at_ms: u64 },
}

const CALL_HOSTS: [usize; 4] = [H1, BOB, OLGA, HP];

struct Sim {
    w: World,
    c: Vec<CallDev>,
    permits: BTreeMap<usize, MGrant>,
    now: u64,
    hold_relay: bool,
    held: Vec<Vec<u8>>,
    redials: Vec<(usize, Redial)>,
    queue: Vec<(usize, String, HostRef, u32, Joined)>,
    ever_hosted: BTreeSet<usize>,
}

impl Sim {
    fn new() -> Self {
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
    fn did(&self, i: usize) -> String {
        self.w.did(i)
    }
    fn call_permit(&self, issuer: usize, h: usize, slot: u64) -> MGrant {
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
    fn record(&self, h: usize) -> HostRecord {
        HostRecord {
            note: self.w.d[h].my_note.clone(),
            permit: self.permits[&h].clone(),
        }
    }
    fn host_ref(&self, h: usize) -> HostRef {
        HostRef {
            holder_did: self.did(h),
            device_slot: self.w.d[h].slot,
            permit_digest: self.permits.get(&h).map(|p| p.digest()).unwrap_or_default(),
        }
    }
    fn dev_of(&self, r: &HostRef) -> usize {
        (0..self.w.d.len())
            .find(|&i| self.did(i) == r.holder_did && self.w.d[i].slot == r.device_slot)
            .unwrap()
    }
    fn snap(&self, i: usize) -> Vec<u8> {
        let mut v = self.w.snap(i);
        v.extend(serde_json::to_vec(&self.c[i]).unwrap());
        v
    }
    fn snap_all(&self) -> Vec<Vec<u8>> {
        (0..self.w.d.len()).map(|i| self.snap(i)).collect()
    }
    fn is_member(&self, i: usize, did: &str) -> bool {
        let d = &self.w.d[i];
        d.board.roster.contains(did) && (self.w.rogue.contains(&i) || !d.view.removed.contains(did))
    }
    fn msg(&self, i: usize, call_id: &str, host_gen: u32, kind: Kind) -> Ctl {
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
    fn new_call_id(&mut self) -> String {
        hex32(&self.w.rng.bytes32()[..16])
    }

    // ---- call-host records ----
    fn publish_call_record(&mut self, h: usize) {
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
    fn push_call_record(&mut self, h: usize) -> Vec<usize> {
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
    fn check_call_record(&self, i: usize, r: &HostRecord) -> Result<(), CR> {
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
    fn accept_call_record(&mut self, i: usize, bytes: &[u8]) -> Result<(), CR> {
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
    fn revoke_op(&mut self, admin: usize, h: usize) -> Vec<u8> {
        let d = self.permits[&h].digest();
        self.w.op(admin, Op::RemoveHost { permit: d }).wire()
    }
    fn accept_revoke(&mut self, i: usize, bytes: &[u8]) -> Result<(), CR> {
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
    fn revoke(&mut self, admin: usize, h: usize, to: &[usize]) {
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
    fn status(&self, h: usize, issued_ms: u64, ttl_ms: u64) -> Status {
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
    fn accept_status(&self, i: usize, s: &Status) -> bool {
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
    fn rank_at(&self, i: usize, st: &[Status], call_id: &str, starter: &str, n: usize) -> Vec<HostRef> {
        let ok: Vec<StatusBody> = st
            .iter()
            .filter(|s| self.accept_status(i, s))
            .map(|s| s.body.clone())
            .collect();
        rank(&ok, call_id, starter, n, self.now)
    }

    // ---- CallStart ----
    fn start_msg(&self, h: usize, call_id: &str, hgen: u32, starter: usize, backups: Vec<HostRef>) -> Ctl {
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
    fn start_call(&mut self, h: usize, starter: usize, backups: Vec<HostRef>) -> Result<String, CR> {
        let call_id = self.new_call_id();
        self.begin_hosting(h, &call_id, 1, starter, backups)?;
        Ok(call_id)
    }
    fn begin_hosting(
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
    fn announce(&mut self, h: usize, bytes: Vec<u8>) {
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
    fn relay_targets(&self) -> Vec<usize> {
        (0..self.w.d.len())
            .filter(|&i| self.w.hosts.contains(&i) || self.is_member(H2, &self.did(i)))
            .collect()
    }
    fn relay(&mut self, bytes: Vec<u8>) {
        for i in self.relay_targets() {
            let e = self.w.gep(i);
            self.w.send(H2, i, e, "call-start", bytes.clone());
            let _ = self.accept_start(i, &bytes);
        }
    }
    fn flush_relay(&mut self) {
        self.hold_relay = false;
        for b in std::mem::take(&mut self.held) {
            self.relay(b);
        }
        self.settle();
    }
    // Members' checks on a CallStart (§3.2 items 1-3) plus generation, split and glare rules.
    fn accept_start(&mut self, i: usize, bytes: &[u8]) -> Result<(), CR> {
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
    fn host_ref_of(&self, i: usize) -> Option<HostRef> {
        self.permits.contains_key(&i).then(|| self.host_ref(i))
    }
    // Glare (§3.1) and split (§5): a host whose call lost sends End{superseded, next_host}.
    fn settle(&mut self) {
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
    fn join_req(&mut self, m: usize, call_id: &str, hgen: u32, issued_ms: u64) -> Vec<u8> {
        let nonce = self.w.rng.next_u64();
        let join = self.msg(m, call_id, hgen, Kind::Join { issued_ms, nonce });
        serde_json::to_vec(&JoinReq {
            join,
            note: self.w.d[m].my_note.clone(),
        })
        .unwrap()
    }
    // Member side: dial the host from the group endpoint and send a Join.
    fn join_call(&mut self, m: usize, call_id: &str, host: &HostRef, hgen: u32) -> Result<u8, CR> {
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
    fn join_shown(&mut self, m: usize) -> Result<u8, CR> {
        let call_id = self.c[m].shown.clone().ok_or(CR::Unknown)?;
        let st = self.c[m].starts[&call_id].clone();
        let (host, _) = st.start().unwrap();
        self.join_call(m, &call_id, &host.clone(), st.msg.host_gen)
    }
    fn ctl_rate_ok(&self, h: usize, did: &str) -> bool {
        let now = self.now;
        let hc = self.c[h].hosting.as_ref().unwrap();
        hc.ctl_times
            .get(did)
            .map_or(0, |q| q.iter().filter(|t| **t + 1000 > now).count())
            < MAX_CONTROL_PER_S
    }
    fn host_join(&mut self, h: usize, conn_ep: &str, bytes: &[u8]) -> Result<u8, CR> {
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
        if let Some(cur) = self.w.d[h].host.as_ref().and_then(|hs| hs.directory.get(&dk)) {
            if cur.claims.seq > n.claims.seq {
                return Err(CR::Stale);
            }
        }
        let did = m.signer.clone();
        if let Some(hc) = self.c[h].hosting.as_ref() {
            if hc.seen.contains(&j.id()) {
                return Err(CR::Replay);
            }
            if hc.last_join.get(&did).is_some_and(|t| t + JOIN_MIN_GAP_MS > self.now) {
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
                    self.w.send(h, od, e, "close", b"answered-elsewhere".to_vec());
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
    fn roster_ctl(&mut self, h: usize, skip: Option<&str>) -> Ctl {
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
    fn push_roster(&mut self, h: usize) {
        let r = self.roster_ctl(h, None);
        self.deliver_roster(h, &r);
    }
    fn deliver_roster(&mut self, h: usize, r: &Ctl) -> Vec<(usize, Result<(), CR>)> {
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
            let Ok(dev) = self.w.dial(h, &ep) else { continue };
            if dev != h {
                self.w.send(h, dev, ep, "roster", bytes.clone());
            }
            out.push((dev, self.accept_roster(dev, &bytes)));
        }
        out
    }
    fn accept_roster(&mut self, i: usize, bytes: &[u8]) -> Result<(), CR> {
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
        let roster: BTreeMap<u8, String> = entries.iter().map(|e| (e.slot, e.did.clone())).collect();
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
    fn leave(&mut self, m: usize) -> Result<(), CR> {
        let j = self.c[m].joined.clone().ok_or(CR::Unknown)?;
        let ctl = self.msg(m, &j.call_id, j.host_gen, Kind::Leave { issued_ms: self.now });
        let h = self.dev_of(&j.host);
        if h != m {
            self.w.send(m, h, j.host_ep.clone(), "leave", ctl.wire());
        }
        self.host_leave(h, &j.my_ep, &ctl.wire())?;
        self.c[m].joined = None;
        self.c[m].last_call = Some(j.call_id);
        Ok(())
    }
    fn host_leave(&mut self, h: usize, conn_ep: &str, bytes: &[u8]) -> Result<(), CR> {
        let ctl: Ctl = serde_json::from_slice(bytes).map_err(|_| CR::Malformed)?;
        if !ctl.verify() || !matches!(ctl.msg.kind, Kind::Leave { .. }) {
            return Err(CR::BadSignature);
        }
        let hc = self.c[h].hosting.as_ref().ok_or(CR::Unknown)?;
        if ctl.msg.call_id != hc.call_id || ctl.msg.host_gen != hc.host_gen {
            return Err(CR::WrongCall);
        }
        if hc.parts.get(&ctl.msg.signer).is_none_or(|p| p.endpoint != conn_ep) {
            return Err(CR::NotParticipant);
        }
        let own = ctl.msg.signer == self.did(h);
        self.c[h].hosting.as_mut().unwrap().parts.remove(&ctl.msg.signer);
        if own && self.c[h].cond.class == Class::Phone {
            // §2.4 / Q11: a phone hosts only while its own user is in the call.
            let next = self.next_backup(h);
            self.host_end(h, EndReason::HostLeaving, next, None);
            return Ok(());
        }
        self.push_roster(h);
        Ok(())
    }
    fn next_backup(&self, h: usize) -> Option<HostRef> {
        let hc = self.c[h].hosting.as_ref()?;
        let (_, b) = hc.start.start()?;
        b.iter().find(|r| !self.c[h].revoked.contains(&r.permit_digest)).cloned()
    }
    // `only`: send End to that DID alone and keep hosting the others (§3.2 item 2).
    fn host_end(&mut self, h: usize, reason: EndReason, next: Option<HostRef>, only: Option<&str>) {
        let hc = self.c[h].hosting.as_ref().unwrap();
        let (cid, hgen) = (hc.call_id.clone(), hc.host_gen);
        let targets: Vec<(String, String)> = hc
            .parts
            .iter()
            .filter(|(d, _)| only.is_none_or(|o| o == d.as_str()))
            .map(|(d, p)| (d.clone(), p.endpoint.clone()))
            .collect();
        let end = self.msg(h, &cid, hgen, Kind::End { reason, next_host: next.clone() });
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
            let Ok(dev) = self.w.dial(h, &ep) else { continue };
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
    fn accept_end(&mut self, i: usize, bytes: &[u8]) -> Result<(), CR> {
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
            self.c[i].shown = self.c[i].starts.keys().min_by_key(|c| call_hash(c)).cloned();
        }
        let Some(j) = joined else { return Ok(()) };
        self.c[i].joined = None;
        self.c[i].last_call = Some(call_id.clone());
        self.c[i].log.push(format!("end:{reason:?}"));
        if let (Some(nh), true) = (
            next_host.clone(),
            matches!(reason, EndReason::HostLeaving | EndReason::HostRotating | EndReason::Superseded),
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
    fn run_queue(&mut self) {
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
    fn advance(&mut self, ms: u64) {
        let end = self.now + ms;
        while self.now + HEARTBEAT_MS <= end {
            self.now += HEARTBEAT_MS;
            self.tick();
        }
        self.now = end;
    }
    fn tick(&mut self) {
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
    fn redial_after_loss(&mut self, m: usize, j: &Joined, lost_at: u64) -> Redial {
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
    fn host_ingress(&mut self, h: usize, conn_ep: &str, bytes: &[u8]) -> Result<(), CR> {
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
    fn upstream(&mut self, m: usize, bytes: Vec<u8>) -> Result<(), CR> {
        let j = self.c[m].joined.clone().ok_or(CR::Unknown)?;
        let h = self.w.dial(m, &j.host_ep).map_err(|_| CR::Unreachable)?;
        if h != m {
            self.w.send(m, h, j.host_ep.clone(), "dgram", bytes.clone());
        }
        self.host_ingress(h, &j.my_ep, &bytes)
    }
    fn speak(&mut self, m: usize, level: u8) -> Result<(), CR> {
        self.c[m].seq += 1;
        let mut d = vec![UP];
        d.extend(audio(self.c[m].seq, level));
        self.upstream(m, d)
    }
    // G6-D1 sender silence gate. `seq` keeps counting one per frame. After GATE_FRAMES quiet
    // frames the sender stops sending Opus and sends `[0x03][level]` every LEVEL_ONLY_FRAMES.
    fn speak_gated(&mut self, m: usize, level: u8) -> Result<(), CR> {
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
    fn accept_active(&mut self, i: usize, bytes: &[u8]) -> Result<(), CR> {
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
    fn host_frame(&mut self, h: usize) -> BTreeSet<u8> {
        let Some(hc) = self.c[h].hosting.as_mut() else {
            return BTreeSet::new();
        };
        let pending = std::mem::take(&mut hc.pending);
        let levels = std::mem::take(&mut hc.levels);
        hc.fwd = select(&hc.fwd, &levels);
        let set = hc.fwd.set.clone();
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
    fn client_rx(&mut self, i: usize, bytes: &[u8]) {
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
    fn frame(&mut self, h: usize, speakers: &[(usize, u8)]) -> BTreeSet<u8> {
        self.now += FRAME_MS;
        for &(m, l) in speakers {
            let _ = self.speak(m, l);
        }
        self.host_frame(h)
    }

    // ---- U9 removal (reused multi_host flags) ----
    fn remove_member(&mut self, target: usize, to: &[usize]) {
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
    fn host_sweep(&mut self, h: usize) {
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
    fn rotation_tick(&mut self, h: usize) -> &'static str {
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

fn base() -> (Sim, String) {
    let mut s = Sim::new();
    let b = vec![s.host_ref(BOB), s.host_ref(OLGA)];
    let cid = s.start_call(H1, OLGA, b).unwrap();
    (s, cid)
}
fn roster_of(s: &Sim, h: usize) -> BTreeMap<u8, String> {
    s.c[h]
        .hosting
        .as_ref()
        .unwrap()
        .parts
        .iter()
        .map(|(d, p)| (p.slot, d.clone()))
        .collect()
}
fn assert_unchanged(s: &Sim, before: &[Vec<u8>], case: &str) {
    for (i, b) in before.iter().enumerate() {
        assert!(*b == s.snap(i), "{case}: device {i} state changed");
    }
}

// ============================================================================================
// Tests
// ============================================================================================

// KIND: invariant
// GC-S01. H refuses each bad Join on a populated call: a non-member, a non-member host binary,
// a forged signature (tampered body, wrong signer), a wrong gid, a wrong call_id, a replay, an
// endpoint that does not match, an oversize frame, a removed member (U12: immediate) and a
// stale host_gen after a redial. Every device's state, H included, stays byte-identical.
// Positive controls are a valid Join on the same call and a valid Join at the new generation.
#[test]
fn gc_s01_bad_joins_are_refused_and_leave_every_state_byte_identical() {
    let (mut s, cid) = base();
    for m in [A1, BOB] {
        s.join_shown(m).unwrap();
    }
    s.advance(3_000);
    let gid = s.w.gid.clone();
    let hgen = 1;
    let mallory = ident(37);
    let m_key = SigningKey::from_bytes(&[9; 32]);
    let m_ep = ep("grp", &m_key);
    let m_note = EpNote::sign(&mallory, Audience::Group(gid.clone()), 77, &m_ep, 1);
    let carol_ep = s.w.gep(CAROL);
    let carol_note = s.w.d[CAROL].my_note.clone();
    let mk = |s: &Sim, id: &Identity, signer: String, call_id: &str, gid: &str, hgen: u32, nonce: u64| {
        let m = CallMsg {
            v: 1,
            gid: gid.into(),
            call_id: call_id.into(),
            signer,
            host_gen: hgen,
            kind: Kind::Join {
                issued_ms: s.now,
                nonce,
            },
        };
        Ctl::sign(id, m)
    };
    let req = |j: Ctl, n: &EpNote| serde_json::to_vec(&JoinReq { join: j, note: n.clone() }).unwrap();
    let carol = s.w.d[CAROL].id.clone();
    let carol_did = s.did(CAROL);
    let mut tampered = mk(&s, &carol, carol_did.clone(), &cid, &gid, hgen, 1);
    tampered.msg.kind = Kind::Join {
        issued_ms: s.now,
        nonce: 2,
    };
    let wrong_signer = mk(&s, &mallory, carol_did.clone(), &cid, &gid, hgen, 3);
    let hugo_note = s.w.d[H2].my_note.clone();
    let hugo = s.w.d[H2].id.clone();
    let accepted_a1 = s.c[H1].hosting.as_ref().unwrap().parts[&s.did(A1)].join.clone();
    let a1_note = s.w.d[A1].my_note.clone();
    let a1_ep = s.w.gep(A1);
    let bob_join = mk(&s, &s.w.d[BOB].id.clone(), s.did(BOB), &cid, &gid, hgen, 4);
    let bob_note = s.w.d[BOB].my_note.clone();
    let mut oversize = req(mk(&s, &carol, carol_did.clone(), &cid, &gid, hgen, 5), &carol_note);
    oversize.extend(vec![b' '; MAX_CONTROL]);
    let cases: Vec<(&str, String, Vec<u8>, CR)> = vec![
        ("non-member", m_ep.clone(), req(mk(&s, &mallory, mallory.did().into(), &cid, &gid, hgen, 6), &m_note), CR::NotMember),
        ("non-member host binary", s.w.gep(H2), req(mk(&s, &hugo, hugo.did().into(), &cid, &gid, hgen, 7), &hugo_note), CR::NotMember),
        ("tampered body", carol_ep.clone(), req(tampered, &carol_note), CR::BadSignature),
        ("signed by another key", carol_ep.clone(), req(wrong_signer, &carol_note), CR::BadSignature),
        ("wrong gid", carol_ep.clone(), req(mk(&s, &carol, carol_did.clone(), &cid, &"f".repeat(64), hgen, 8), &carol_note), CR::WrongGroup),
        ("wrong call_id", carol_ep.clone(), req(mk(&s, &carol, carol_did.clone(), &"0".repeat(32), &gid, hgen, 9), &carol_note), CR::WrongCall),
        ("future host_gen", carol_ep.clone(), req(mk(&s, &carol, carol_did.clone(), &cid, &gid, hgen + 1, 10), &carol_note), CR::StaleGen),
        ("replayed accepted Join", a1_ep.clone(), req(accepted_a1, &a1_note), CR::Replay),
        ("bob's Join from carol's connection", carol_ep.clone(), req(bob_join, &bob_note), CR::EndpointMismatch),
        ("oversize", carol_ep.clone(), oversize, CR::Oversize),
    ];
    let roster = roster_of(&s, H1);
    for (case, conn, bytes, want) in cases {
        let before = s.snap_all();
        assert_eq!(s.host_join(H1, &conn, &bytes), Err(want), "{case}");
        assert_unchanged(&s, &before, case);
        assert_eq!(roster_of(&s, H1), roster, "{case}");
    }
    // Positive control: carol's valid Join on the same call.
    s.join_shown(CAROL).unwrap();
    assert_eq!(roster_of(&s, H1).len(), 3);
    // Removed member (U12 detail: refusal is immediate). H ejects her and refuses a re-Join.
    s.remove_member(CAROL, &[OLGA, A1, A2, BOB, CAROL, HP, H1, H2]);
    assert!(s.c[CAROL].joined.is_none());
    s.advance(3_000);
    let before = s.snap_all();
    let again = s.join_req(CAROL, &cid, hgen, s.now);
    assert_eq!(s.host_join(H1, &carol_ep, &again), Err(CR::NotMember));
    assert_unchanged(&s, &before, "removed member");
    // Stale host_gen after a redial. H1 drops, members move to BOB at gen 2, and a gen-1 Join
    // reaches BOB.
    s.w.d[H1].online = false;
    s.advance(4_000);
    assert_eq!(s.c[BOB].hosting.as_ref().unwrap().host_gen, 2);
    let old = s.join_req(OLGA, &cid, 1, s.now);
    let before = s.snap_all();
    assert_eq!(s.host_join(BOB, &s.w.gep(OLGA), &old), Err(CR::StaleGen));
    assert_unchanged(&s, &before, "stale host_gen");
    // Positive control at gen 2.
    let ok = s.join_req(OLGA, &cid, 2, s.now);
    s.c[OLGA].dialing = Some(Dialing {
        call_id: cid.clone(),
        host: s.host_ref(BOB),
        host_ep: s.w.gep(BOB),
        host_gen: 2,
    });
    assert!(s.host_join(BOB, &s.w.gep(OLGA), &ok).is_ok());
}

// KIND: invariant (ignored: not expressible)
// GC-S01, §3.2 item 4, second half: "the device slot is not on the DID's signed device list as
// unlinked".
#[test]
#[ignore = "GC-S01: the multi_host fixture has no signed device list (U8), so an unlinked slot cannot be expressed; the Join also carries no device slot apart from the group note's"]
fn gc_s01b_join_from_a_device_slot_marked_unlinked_is_refused() {
    unimplemented!("needs a signed device-list fixture (crates/proto rotation_device_list_push)");
}

// KIND: invariant
// GC-S02 / R05. Members refuse each bad CallStart and show no call. The cases are a chat-host
// permit without `CallHost`, a revoked call permit, a non-member host binary with a valid
// CallHost permit, a sibling device's permit, a permit issued by a non-admin, a forged
// signature, a wrong gid and a host_ref that does not match. Every device stays byte-identical.
// The positive control is a valid CallStart, which is shown.
#[test]
fn gc_s02_callstart_without_a_valid_call_permit_is_refused_and_no_call_is_shown() {
    let mut s = Sim::new();
    let cid = s.new_call_id();
    let gid = s.w.gid.clone();
    let announce = |s: &Sim, h: usize, permit: MGrant, gid: &str, f: &dyn Fn(&mut CallMsg)| {
        let mut start = s.start_msg(h, &cid, 1, OLGA, vec![]);
        start.msg.gid = gid.into();
        let rf = HostRef {
            holder_did: s.did(h),
            device_slot: s.w.d[h].slot,
            permit_digest: permit.digest(),
        };
        if let Kind::Start { host_ref, .. } = &mut start.msg.kind {
            *host_ref = rf;
        }
        f(&mut start.msg);
        let start = Ctl::sign(&s.w.d[h].id, start.msg);
        let record = HostRecord {
            note: s.w.d[h].my_note.clone(),
            permit,
        };
        serde_json::to_vec(&Announce { start, record }).unwrap()
    };
    let none = |_: &mut CallMsg| {};
    let chat_permit = s.w.d[H1].host.as_ref().unwrap().record.permit.clone();
    let hugo_call = s.call_permit(OLGA, H2, s.w.d[H2].slot);
    let sibling = s.permits[&HP].clone();
    let carol_made = MGrant::issue(
        &s.w.d[CAROL].id,
        &gid,
        &s.did(H1),
        Some(s.w.d[H1].slot),
        &[Cap::CallHost],
        false,
        90,
        None,
    );
    let good = s.permits[&H1].clone();
    let mut forged = announce(&s, H1, good.clone(), &gid, &none);
    let at = forged.windows(4).position(|w| w == b"\"k\":").unwrap();
    forged[at + 4] = b'9';
    let mismatched = {
        let mut a: Announce = serde_json::from_slice(&announce(&s, H1, good.clone(), &gid, &none)).unwrap();
        if let Kind::Start { host_ref, .. } = &mut a.start.msg.kind {
            host_ref.device_slot ^= 1;
        }
        a.start = Ctl::sign(&s.w.d[H1].id, a.start.msg);
        serde_json::to_vec(&a).unwrap()
    };
    let cases: Vec<(&str, Vec<u8>, CR)> = vec![
        ("chat host without host_call (R05)", announce(&s, H1, chat_permit, &gid, &none), CR::PermitInvalid),
        ("non-member host binary with a CallHost permit", announce(&s, H2, hugo_call, &gid, &none), CR::NotMember),
        ("sibling device's permit", announce(&s, H1, sibling, &gid, &none), CR::DeviceMismatch),
        ("permit issued by a non-admin", announce(&s, H1, carol_made, &gid, &none), CR::PermitInvalid),
        ("forged signature", forged, CR::BadSignature),
        ("wrong gid", announce(&s, H1, good.clone(), &"e".repeat(64), &none), CR::WrongGroup),
        ("host_ref does not match the record", mismatched, CR::DeviceMismatch),
    ];
    let members = [OLGA, A1, A2, BOB, CAROL, HP];
    for (case, bytes, want) in cases {
        let before = s.snap_all();
        for &m in &members {
            assert_eq!(s.accept_start(m, &bytes), Err(want.clone()), "{case} at {m}");
            assert!(s.c[m].shown.is_none(), "{case}");
        }
        assert_unchanged(&s, &before, case);
    }
    // Revoked: an admin revokes H1's call permit, then H1's otherwise valid CallStart is refused.
    let mut r = Sim::new();
    let ok = {
        let st = r.start_msg(H1, &cid, 1, OLGA, vec![]);
        serde_json::to_vec(&Announce {
            start: st,
            record: r.record(H1),
        })
        .unwrap()
    };
    r.revoke(A1, H1, &[OLGA, A1, A2, BOB, CAROL, HP, H1]);
    let before = r.snap_all();
    for m in members {
        assert_eq!(r.accept_start(m, &ok), Err(CR::Revoked), "revoked at {m}");
    }
    assert_unchanged(&r, &before, "revoked");
    assert_eq!(r.start_call(H1, OLGA, vec![]), Err(CR::Revoked));
    // Positive control: H1's valid CallStart is shown everywhere.
    s.begin_hosting(H1, &cid, 1, OLGA, vec![]).unwrap();
    for m in members {
        assert_eq!(s.c[m].shown.as_deref(), Some(cid.as_str()));
    }
}

// KIND: invariant
// GC-S03. After a redial to gen 2, these are ignored everywhere with state byte-identical:
// the old host's End{ended} at gen 1, a replayed gen-1 CallStart, a freshly signed gen-1
// CallStart and a gen-1 Roster.
#[test]
fn gc_s03_stale_end_and_callstart_after_redial_are_ignored_everywhere() {
    let (mut s, cid) = base();
    for m in [A1, CAROL] {
        s.join_shown(m).unwrap();
    }
    let gen1 = serde_json::to_vec(&Announce {
        start: s.c[H1].hosting.as_ref().unwrap().start.clone(),
        record: s.record(H1),
    })
    .unwrap();
    s.w.d[H1].online = false;
    s.advance(4_000);
    assert_eq!(s.c[BOB].hosting.as_ref().unwrap().host_gen, 2);
    s.w.d[H1].online = true;
    let stale_end = s.msg(
        H1,
        &cid,
        1,
        Kind::End {
            reason: EndReason::Ended,
            next_host: None,
        },
    );
    let fresh_gen1 = serde_json::to_vec(&Announce {
        start: s.start_msg(H1, &cid, 1, OLGA, vec![]),
        record: s.record(H1),
    })
    .unwrap();
    let stale_roster = s.msg(
        H1,
        &cid,
        1,
        Kind::Roster {
            epoch: 99,
            entries: vec![],
        },
    );
    let all: Vec<usize> = (0..s.w.d.len()).collect();
    let before = s.snap_all();
    for &i in &all {
        if s.c[i].hgen.contains_key(&cid) {
            assert_eq!(s.accept_end(i, &stale_end.wire()), Err(CR::StaleGen), "end at {i}");
            assert_eq!(s.accept_start(i, &gen1), Err(CR::StaleGen), "replayed start at {i}");
            assert_eq!(s.accept_start(i, &fresh_gen1), Err(CR::StaleGen), "fresh gen-1 at {i}");
        }
        if s.c[i].joined.is_some() {
            assert_eq!(s.accept_roster(i, &stale_roster.wire()), Err(CR::StaleGen), "roster at {i}");
        }
    }
    assert_unchanged(&s, &before, "stale gen-1 control");
    for m in [A1, CAROL] {
        assert_eq!(s.c[m].joined.as_ref().unwrap().host_gen, 2);
    }
}

// KIND: invariant
// GC-S04. The starter and every member rank the same status set, given to each in a different
// order, and get the same ranking and backups. Tiers, exclusions, TTL (capped at
// STATUS_TTL_MS), the starter tie-break and the hash tie-break all hold. Forged statuses and
// statuses without a permit are dropped. The picker moves to the next candidate after a refusal,
// or after ANSWER_TIMEOUT_MS when a candidate does not answer.
#[test]
fn gc_s04_same_status_set_gives_identical_ranking_and_backups_everywhere() {
    let mut s = Sim::new();
    let cid = s.new_call_id();
    s.c[HP].cond = Cond::phone(90, true); // phone charging on Wi-Fi: tier 1
    let now = s.now;
    let mut st: Vec<Status> = CALL_HOSTS.iter().map(|&h| s.status(h, now - 1_000, STATUS_TTL_MS)).collect();
    // A forged status (busy flipped after signing), and one from a non-permit device.
    let mut forged = s.status(BOB, now, STATUS_TTL_MS);
    forged.body.max_participants = 1;
    st.push(forged);
    let mut carol = s.status(BOB, now, STATUS_TTL_MS);
    carol.body.host_ref.holder_did = s.did(CAROL);
    carol.sig = s.w.d[CAROL].id.sign(&signed(STATUS_DOMAIN, &carol.body)).to_vec();
    st.push(carol);
    let starter = s.did(CAROL);
    let reference = s.rank_at(CAROL, &st, &cid, &starter, 4);
    let mut tier0: Vec<HostRef> = [H1, BOB, OLGA].iter().map(|&h| s.host_ref(h)).collect();
    tier0.sort_by_key(|r| tie_hash(&cid, r));
    let mut expect = tier0.clone();
    expect.push(s.host_ref(HP));
    assert_eq!(reference, expect, "tier 0 ordered by hash, then the tier-1 phone");
    let mut rng = SimRng(4);
    for m in [OLGA, A1, A2, BOB, HP] {
        let mut shuffled = st.clone();
        for i in (1..shuffled.len()).rev() {
            shuffled.swap(i, (rng.next_u64() % (i as u64 + 1)) as usize);
        }
        assert_eq!(s.rank_at(m, &shuffled, &cid, &starter, 4), reference, "member {m}");
    }
    let backups: Vec<HostRef> = reference[1..=MAX_BACKUPS].to_vec();
    assert_eq!(backups.len(), 2);
    // The starter itself goes first when it is in the best tier.
    let bob_starts = s.rank_at(CAROL, &st, &cid, &s.did(BOB), 4);
    assert_eq!(bob_starts[0], s.host_ref(BOB));
    // HP as starter is tier 1, so it does not jump ahead of tier 0.
    let hp_starts = s.rank_at(CAROL, &st, &cid, &s.did(HP), 4);
    assert_ne!(hp_starts[0], s.host_ref(HP));
    // Exclusions and tiers (pure).
    let body = |s: &Sim, h: usize, f: &dyn Fn(&mut StatusBody)| {
        let mut b = s.status(h, now, STATUS_TTL_MS).body;
        f(&mut b);
        b
    };
    let rows: Vec<(&str, StatusBody, usize, Option<u8>)> = vec![
        ("desktop mains unmetered", body(&s, BOB, &|_| {}), 4, Some(0)),
        ("phone charging", body(&s, HP, &|b| b.power = Power::Mains), 4, Some(1)),
        ("phone 50 %", body(&s, HP, &|b| b.power = Power::Battery(50)), 4, Some(2)),
        ("phone 49 % (gap G7)", body(&s, HP, &|b| b.power = Power::Battery(49)), 4, Some(TIER_UNLISTED)),
        ("phone 29 %", body(&s, HP, &|b| b.power = Power::Battery(29)), 4, None),
        ("phone metered, N = cap", body(&s, HP, &|b| b.metered = true), CAP_PHONE_METERED, Some(3)),
        ("phone metered, N > cap", body(&s, HP, &|b| b.metered = true), CAP_PHONE_METERED + 1, None),
        ("thermal not ok", body(&s, BOB, &|b| b.thermal_ok = false), 4, None),
        ("busy", body(&s, BOB, &|b| b.busy = true), 4, None),
        ("too small", body(&s, BOB, &|b| b.max_participants = 3), 4, None),
        ("desktop on battery (gap G7)", body(&s, BOB, &|b| b.power = Power::Battery(90)), 4, Some(TIER_UNLISTED)),
    ];
    for (case, b, n, want) in rows {
        assert_eq!(tier(&b, n), want, "{case}");
    }
    // TTL: stale statuses are dropped. A claimed TTL longer than STATUS_TTL_MS is capped.
    let old = s.status(BOB, now - STATUS_TTL_MS, STATUS_TTL_MS);
    let long = s.status(OLGA, now - STATUS_TTL_MS, 10 * STATUS_TTL_MS);
    let fresh = s.status(H1, now - STATUS_TTL_MS + 1, STATUS_TTL_MS);
    assert_eq!(s.rank_at(CAROL, &[old, long, fresh], &cid, &starter, 4), vec![s.host_ref(H1)]);
    // Picker (§2.3): the first candidate refuses (consent off), the second is offline (no answer
    // within ANSWER_TIMEOUT_MS), and the third hosts.
    let first = s.dev_of(&reference[0]);
    s.c[first].consent = false;
    let second = s.dev_of(&reference[1]);
    s.w.d[second].online = false;
    let mut elapsed = 0;
    let mut host = None;
    for r in &reference {
        let h = s.dev_of(r);
        if !s.w.d[h].online {
            elapsed += ANSWER_TIMEOUT_MS;
            continue;
        }
        let b: Vec<HostRef> = reference.iter().filter(|x| *x != r).take(MAX_BACKUPS).cloned().collect();
        if s.start_call(h, CAROL, b).is_ok() {
            host = Some(h);
            break;
        }
    }
    assert_eq!(host, Some(s.dev_of(&reference[2])));
    assert_eq!(elapsed, ANSWER_TIMEOUT_MS);
}

// KIND: invariant
// GC-S05. Two members start a call at the same time on different hosts. The CallStart relay is
// held, so each call gets participants first. After the relay, the lower BLAKE3(call_id) wins
// everywhere, the losing host sends End{superseded, next_host}, and its participants (including
// the losing host's own user) redial the winner. Everyone converges on one call.
#[test]
fn gc_s05_concurrent_starts_converge_on_one_call_and_the_loser_ends_superseded() {
    let mut s = Sim::new();
    s.hold_relay = true;
    let c1 = s.start_call(H1, OLGA, vec![s.host_ref(OLGA)]).unwrap();
    let c2 = s.start_call(BOB, CAROL, vec![s.host_ref(OLGA)]).unwrap();
    // Starters get their host's CallStart directly (the CallHostRequest answer).
    let a1 = serde_json::to_vec(&Announce {
        start: s.c[H1].hosting.as_ref().unwrap().start.clone(),
        record: s.record(H1),
    })
    .unwrap();
    let a2 = serde_json::to_vec(&Announce {
        start: s.c[BOB].hosting.as_ref().unwrap().start.clone(),
        record: s.record(BOB),
    })
    .unwrap();
    for m in [OLGA, A1] {
        s.accept_start(m, &a1).unwrap();
        s.join_shown(m).unwrap();
    }
    for m in [CAROL] {
        s.accept_start(m, &a2).unwrap();
        s.join_shown(m).unwrap();
    }
    s.join_shown(BOB).unwrap(); // BOB's own user in BOB's call
    assert_eq!(roster_of(&s, H1).len(), 2);
    assert_eq!(roster_of(&s, BOB).len(), 2);
    s.flush_relay();
    let (winner, win_host, lose_host) = if call_hash(&c1) < call_hash(&c2) {
        (c1.clone(), H1, BOB)
    } else {
        (c2.clone(), BOB, H1)
    };
    assert!(s.c[lose_host].hosting.is_none(), "loser stopped hosting");
    for m in [OLGA, A1, A2, BOB, CAROL, HP, H1] {
        assert_eq!(s.c[m].shown.as_deref(), Some(winner.as_str()), "shown at {m}");
    }
    for m in [OLGA, A1, BOB, CAROL] {
        let j = s.c[m].joined.as_ref().unwrap_or_else(|| panic!("{m} not in a call"));
        assert_eq!(j.call_id, winner, "{m} converged");
        assert_eq!(s.dev_of(&j.host), win_host);
    }
    assert_eq!(roster_of(&s, win_host).len(), 4);
    let superseded = [OLGA, A1, BOB, CAROL]
        .iter()
        .filter(|&&m| s.c[m].log.iter().any(|l| l == "end:Superseded"))
        .count();
    assert_eq!(superseded, 2, "the losing call's two participants got End{{superseded}}");
}

// KIND: invariant
// GC-S06. Speaker selection for N = 3/5/8/12 and K = 2/3. When every member sends all the time,
// the forwarded counts equal the §4 formulas. H downlink is N-1. H uplink is (N-2)K+1, or
// (N-1)^2 when N <= K, and that is the worst case with H in S. A dedicated host has downlink N
// and uplink NK-K. Each member gets at most K streams and never its own. The §4 table follows
// from the per-stream bytes. With the silence gate and one speaker, the counts are 1 down and
// N-2 up, or 0 down and N-1 up when H speaks. Hysteresis stops flapping, and a sustained louder
// speaker replaces after the average delay plus HYST_FRAMES.
#[test]
fn gc_s06_forwarded_counts_match_section_4_and_hysteresis_prevents_flapping() {
    // (N, K, H up streams, H up kbps, H down kbps, out pps, in pps, MB/h, member down kbps)
    let table: [(usize, usize, usize, u64, u64, u64, u64, u64, u64); 8] = [
        (3, 2, 3, 228, 152, 150, 100, 171, 152),
        (3, 3, 4, 304, 152, 200, 100, 205, 152),
        (5, 2, 7, 532, 304, 350, 200, 376, 152),
        (5, 3, 10, 760, 304, 500, 200, 479, 228),
        (8, 2, 13, 988, 532, 650, 350, 684, 152),
        (8, 3, 19, 1444, 532, 950, 350, 889, 228),
        (12, 2, 21, 1596, 836, 1050, 550, 1094, 152),
        (12, 3, 31, 2356, 836, 1550, 550, 1436, 228),
    ];
    let overhead = DRED_BYTES + AUDIO_HDR + GROUP_HDR + QUIC_BYTES + UDP_IP4;
    let stream_bytes = |bps: u64| bps / 8 / 50 + overhead;
    let kbps = |bps: u64| stream_bytes(bps) * 50 * 8 / 1000;
    assert_eq!(stream_bytes(48_000), 190);
    assert_eq!(kbps(48_000), 76);
    for (n, k, up_s, up_k, down_k, out_pps, in_pps, mbh, mem_k) in table {
        let slots: BTreeSet<u8> = (0..n as u8).collect();
        // H = slot 0, loudest, so H is in S (the worst case).
        let mut f = Fwd::new(k);
        let levels: BTreeMap<u8, u8> = slots.iter().map(|&s| (s, 10 + 2 * s)).collect();
        for _ in 0..AVG_FRAMES * 2 {
            f = select(&f, &levels);
        }
        assert_eq!(f.set.len(), k.min(n));
        assert!(f.set.contains(&0));
        let cp = copies(&f.set, &slots, &slots);
        assert!(cp.iter().all(|(a, b)| a != b), "no self-echo");
        for r in &slots {
            assert!(cp.iter().filter(|(_, to)| to == r).count() <= k, "member downlink <= K");
        }
        let h_up = cp.iter().filter(|(_, to)| *to != 0).count();
        let h_down = slots.len() - 1;
        let formula = if n > k { (n - 2) * k + 1 } else { (n - 1) * (n - 1) };
        assert_eq!(h_up, formula, "N={n} K={k}");
        assert_eq!(h_up, up_s, "table row N={n} K={k}");
        let per = kbps(48_000);
        assert_eq!(h_up as u64 * per, up_k);
        assert_eq!(h_down as u64 * per, down_k);
        assert_eq!(h_up as u64 * 50, out_pps);
        assert_eq!(h_down as u64 * 50, in_pps);
        assert_eq!(((h_up + h_down) as u64 * per * 3600 + 4000) / 8000, mbh, "MB/h N={n} K={k}");
        assert_eq!(k.min(n - 1) as u64 * per, mem_k);
        // H not in S (quietest) costs one stream less, so the formula really is the worst case.
        let mut g = Fwd::new(k);
        let quiet_h: BTreeMap<u8, u8> = slots.iter().map(|&s| (s, if s == 0 { 45 } else { 10 + s })).collect();
        for _ in 0..AVG_FRAMES * 2 {
            g = select(&g, &quiet_h);
        }
        if n > k {
            assert!(!g.set.contains(&0));
            assert_eq!(copies(&g.set, &slots, &slots).iter().filter(|(_, t)| *t != 0).count(), (n - 2) * k);
        }
        // Dedicated host: all N are remote.
        let ded = copies(&f.set, &slots, &slots).len();
        assert_eq!(ded, if n > k { n * k - k } else { n * (n - 1) });
        // Silence gate (G6-D1, Q4 decided yes): one speaker. The others send only level-only
        // datagrams, which are never forwarded (see GC-S06b).
        let mut gte = Fwd::new(k);
        for (speaker, down, up) in [(1u8, 1, n - 2), (0u8, 0, n - 1)] {
            for _ in 0..AVG_FRAMES * 2 {
                gte = select(&gte, &BTreeMap::from([(speaker, 20)]));
            }
            let sent = BTreeSet::from([speaker]);
            let c = copies(&gte.set, &sent, &slots);
            assert_eq!(c.iter().filter(|(_, t)| *t != 0).count(), up, "gated up N={n}");
            assert_eq!(usize::from(speaker != 0), down);
        }
    }
    // Q3: the group bitrate. ASSUMPTION: the overhead is the same at 32 kbps. Then 150 bytes /
    // 60 kbps, about 21 % less. The spec says 145 bytes / 58 kbps / 24 %.
    assert_eq!(stream_bytes(GROUP_BITRATE_BPS), 150);
    assert_eq!(kbps(GROUP_BITRATE_BPS), 60);
    assert_eq!((kbps(48_000) - kbps(GROUP_BITRATE_BPS)) * 100 / kbps(48_000), 21);

    // Hysteresis: three speakers with close, jittery levels and K = 2.
    let run = |hyst: bool| {
        let mut f = Fwd::new(2);
        f.hysteresis = hyst;
        f.dominance_cap = false;
        let mut rng = SimRng(6);
        let mut changes = 0;
        let mut last = BTreeSet::new();
        for t in 0..500 {
            let lv: BTreeMap<u8, u8> = [(1u8, 30u8), (2, 31), (3, 32)]
                .iter()
                .map(|&(s, b)| (s, b - 6 + (rng.next_u64() % 13) as u8))
                .collect();
            f = select(&f, &lv);
            if t > AVG_FRAMES && f.set != last {
                changes += 1;
            }
            last = f.set.clone();
        }
        changes
    };
    assert_eq!(run(true), 0, "no flapping with hysteresis");
    assert!(run(false) >= 10, "flapping without hysteresis: {}", run(false));
    // Replacement: S = {1 at 30, 2 at 32}. Slot 3 is present at 60 (not speech) and then bursts
    // at level 5 for L frames. Its average first reaches 32 - 6 = 26 on burst frame 10, so the
    // swap needs 10 qualifying frames and happens on frame 19. A 200 ms (10-frame) burst never
    // swaps.
    for (burst, swap_at) in [(10usize, None), (30, Some(19usize))] {
        let mut f = Fwd::new(2);
        f.dominance_cap = false;
        let steady = BTreeMap::from([(1u8, 30u8), (2, 32), (3, 60)]);
        for _ in 0..AVG_FRAMES * 2 {
            f = select(&f, &steady);
        }
        assert_eq!(f.set, BTreeSet::from([1, 2]));
        let mut at = None;
        for t in 1..=burst + AVG_FRAMES {
            let lv = BTreeMap::from([(1u8, 30u8), (2, 32), (3, if t <= burst { 5 } else { 60 })]);
            f = select(&f, &lv);
            if at.is_none() && f.set.contains(&3) {
                at = Some(t);
            }
        }
        assert_eq!(at, swap_at, "burst {burst}");
    }
}

// KIND: invariant
// GC-S06b (G6-D1). This runs the world forwarder with the sender silence gate. H sends
// `Active { gen, slots }` to every participant each time S changes, with gen increasing by one
// each time. Every client's active set follows H's. A stale or repeated Active is ignored with
// the client's state byte-identical. A gated sender stops Opus after GATE_FRAMES and sends
// `[0x03][level]` every LEVEL_ONLY_FRAMES. H uses those for selection and never forwards them:
// every downstream datagram is `[0x02][slot][audio]`. `seq` keeps counting while gated, so the
// first resumed packet jumps by the length of the pause.
#[test]
fn gc_s06b_active_on_every_set_change_and_level_only_datagrams_never_forwarded() {
    let (mut s, _) = base();
    let ps = [OLGA, A1, CAROL];
    for m in ps {
        s.join_shown(m).unwrap();
    }
    let f0 = s.w.frames.len();
    let mut changes = 0u32;
    let mut last = BTreeSet::new();
    for t in 0..350u32 {
        s.now += FRAME_MS;
        let script = [
            (OLGA, !(100..300).contains(&t)),
            (A1, (100..200).contains(&t)),
            (CAROL, t >= 300),
        ];
        for (m, on) in script {
            s.speak_gated(m, if on { 20 } else { 70 }).unwrap();
        }
        let set = s.host_frame(H1);
        if set != last {
            changes += 1;
            last = set.clone();
        }
        assert_eq!(s.c[H1].hosting.as_ref().unwrap().active_gen, changes);
        for m in ps {
            let j = s.c[m].joined.as_ref().unwrap();
            assert_eq!((j.active_gen, &j.active), (changes, &set), "client {m} at frame {t}");
        }
    }
    assert!(changes >= 4, "S changed {changes} times");
    let new = &s.w.frames[f0..];
    let actives: Vec<&Frame> = new.iter().filter(|f| f.kind == "active").collect();
    assert_eq!(actives.len(), changes as usize * ps.len(), "one Active per change per participant");
    for m in ps {
        let gens: Vec<u32> = actives
            .iter()
            .filter(|f| f.to == m)
            .map(|f| serde_json::from_slice::<Active>(&f.bytes).unwrap().r#gen)
            .collect();
        assert_eq!(gens, (1..=changes).collect::<Vec<_>>());
    }
    // Level-only datagrams reached H but were never forwarded.
    assert!(s.c[H1].hosting.as_ref().unwrap().level_only_rx > 0);
    assert!(new.iter().any(|f| f.kind == "dgram" && f.bytes[0] == LEVEL_ONLY));
    for f in new.iter().filter(|f| f.from == H1 && f.kind == "dgram") {
        assert_eq!(f.bytes[0], DOWN);
        assert_eq!(f.bytes.len(), 2 + 5 + 120, "forwarded datagrams are full audio only");
    }
    // Silence gate at CAROL: GATE_FRAMES Opus packets, then none until frame 300, then a jump.
    let seqs: Vec<u32> = new
        .iter()
        .filter(|f| f.from == CAROL && f.kind == "dgram" && f.bytes[0] == UP)
        .map(|f| u32::from_be_bytes(f.bytes[1..5].try_into().unwrap()))
        .collect();
    let expect: Vec<u32> = (1..=GATE_FRAMES).chain(301..=350).collect();
    assert_eq!(seqs, expect, "gated sender stops Opus and keeps counting seq");
    let level_only = new
        .iter()
        .filter(|f| f.from == CAROL && f.kind == "dgram" && f.bytes[0] == LEVEL_ONLY)
        .count() as u32;
    assert_eq!(level_only, (300 - GATE_FRAMES) / LEVEL_ONLY_FRAMES);
    // A stale or repeated Active is ignored.
    let stale = serde_json::to_vec(&Active { r#gen: changes, slots: vec![0, 1, 2] }).unwrap();
    let before = s.snap(OLGA);
    assert_eq!(s.accept_active(OLGA, &stale), Err(CR::Stale));
    assert!(before == s.snap(OLGA));
}

// KIND: limitation
// GC-S07. A sender always reports level 0. With no dominance cap it holds a slot 100 % of the
// time. The cap (§6) bounds that below DOM_SHARE_PCT, but the sender still dominates. Muting it
// locally removes it from the mix, but its forwarded slot is still used up. The host's "lower
// priority" control keeps it out of the set completely while others speak.
#[test]
fn gc_s07_level_zero_spoofer_is_bounded_by_dominance_cap_local_mute_and_lower_priority() {
    let spoofer = 9u8;
    let honest = [(1u8, 20u8), (2, 26), (3, 34)];
    let frames = 5 * 60 * 50; // 5 minutes
    let run = |cap: bool, lowered: bool| {
        let mut f = Fwd::new(K_PHONE);
        f.dominance_cap = cap;
        if lowered {
            f.lowered.insert(spoofer);
        }
        let mut held = 0usize;
        let mut honest_slots = 0usize;
        for _ in 0..frames {
            let mut lv: BTreeMap<u8, u8> = honest.iter().copied().collect();
            lv.insert(spoofer, 0);
            f = select(&f, &lv);
            held += usize::from(f.set.contains(&spoofer));
            honest_slots += f.set.iter().filter(|s| **s != spoofer).count();
        }
        (held * 100 / frames, honest_slots)
    };
    let (no_cap, honest_no_cap) = run(false, false);
    let (capped, honest_capped) = run(true, false);
    let (lowered, _) = run(true, true);
    assert_eq!(no_cap, 100, "limitation: the spoofer holds a slot permanently");
    assert!(capped < DOM_SHARE_PCT, "dominance cap bounds the share: {capped} %");
    assert!(capped >= 75, "limitation: the spoofer still dominates: {capped} %");
    assert!(honest_capped > honest_no_cap, "the cap gives honest speakers more slot time");
    assert_eq!(lowered, 0, "lower priority keeps it out while K others speak");
    // Local mute: the client drops the spoofer from its mix, but the slot is still forwarded.
    let roster: BTreeMap<u8, String> = [(1, "a"), (2, "b"), (9, "spoofer")]
        .iter()
        .map(|(s, d)| (*s, d.to_string()))
        .collect();
    let forwarded = BTreeSet::from([1u8, spoofer]);
    let muted = BTreeSet::from(["spoofer".to_string()]);
    assert_eq!(mix(&forwarded, &roster, &muted), BTreeSet::from([1]));
    assert_eq!(forwarded.len(), K_PHONE, "limitation: the muted spoofer still uses a forwarded slot");
}

// KIND: limitation (ignored: not expressible)
#[test]
#[ignore = "GC-S07: the Opus VBR packet-size plausibility check (§6 [A]) needs a real Opus encoder (crates/audio, not a group-sim dependency); measure in GC-D09"]
fn gc_s07b_opus_packet_size_flags_claimed_loud_but_tiny_packets() {
    unimplemented!("needs real Opus frames");
}

// KIND: invariant
// GC-S08. H is lost after HEARTBEAT_MISSES missed heartbeats. (a) backups[0] is up: members
// redial it at once with host_gen+1, it signs CallStart{gen 2}, and the call continues.
// (b) backups[0] is down: members reach backups[1] after ANSWER_TIMEOUT_MS. (c) Both are down:
// "Call ended — host lost" within REDIAL_WINDOW_MS. (d) Split: some members reach b0 and others
// reach b1 at the same gen. Once the relay delivers both CallStarts, b0 wins (lower backup
// index), b1 ends superseded and its members move to b0.
#[test]
fn gc_s08_host_drop_redials_backup0_else_backup1_and_split_resolves_to_backup0() {
    let setup = || {
        let (mut s, cid) = base();
        for m in [A1, CAROL, HP] {
            s.join_shown(m).unwrap();
        }
        s.advance(2_000);
        (s, cid)
    };
    // (a)
    let (mut s, cid) = setup();
    let last_hb = s.c[A1].joined.as_ref().unwrap().last_hb_ms;
    s.w.d[H1].online = false;
    s.advance(10_000);
    let lost = last_hb + HEARTBEAT_MISSES * HEARTBEAT_MS;
    for m in [A1, CAROL, HP] {
        let r = s.redials.iter().find(|(d, _)| *d == m).map(|(_, r)| r);
        assert_eq!(r, Some(&Redial::Joined { host: BOB, at_ms: lost }), "member {m}");
        let j = s.c[m].joined.as_ref().unwrap();
        assert_eq!((j.call_id.as_str(), j.host_gen, s.dev_of(&j.host)), (cid.as_str(), 2, BOB));
    }
    assert_eq!(roster_of(&s, BOB).len(), 3);
    let (_, b) = s.c[BOB].hosting.as_ref().unwrap().start.start().unwrap();
    assert_eq!(b, &vec![s.host_ref(OLGA)], "new backups = old minus self (G9)");
    // (b)
    let (mut s, _) = setup();
    let last_hb = s.c[A1].joined.as_ref().unwrap().last_hb_ms;
    s.w.d[H1].online = false;
    s.w.d[BOB].online = false;
    s.advance(10_000);
    let lost = last_hb + HEARTBEAT_MISSES * HEARTBEAT_MS;
    for m in [A1, CAROL, HP] {
        let r = s.redials.iter().find(|(d, _)| *d == m).map(|(_, r)| r);
        assert_eq!(r, Some(&Redial::Joined { host: OLGA, at_ms: lost + ANSWER_TIMEOUT_MS }));
        assert!(ANSWER_TIMEOUT_MS <= REDIAL_WINDOW_MS);
    }
    // (c)
    let (mut s, _) = setup();
    let last_hb = s.c[A1].joined.as_ref().unwrap().last_hb_ms;
    for h in [H1, BOB, OLGA] {
        s.w.d[h].online = false;
    }
    s.advance(10_000);
    let lost = last_hb + HEARTBEAT_MISSES * HEARTBEAT_MS;
    for m in [A1, CAROL, HP] {
        let r = s.redials.iter().find(|(d, _)| *d == m).map(|(_, r)| r);
        assert_eq!(r, Some(&Redial::HostLost { at_ms: lost + REDIAL_WINDOW_MS }));
        assert!(s.c[m].joined.is_none());
        assert!(s.c[m].log.iter().any(|l| l == "Call ended — host lost"));
    }
    // (d) split
    let (mut s, cid) = setup();
    s.hold_relay = true;
    s.w.cut(HP, BOB);
    s.w.d[H1].online = false;
    s.advance(4_000);
    assert_eq!(s.c[BOB].hosting.as_ref().unwrap().host_gen, 2);
    assert_eq!(s.c[OLGA].hosting.as_ref().unwrap().host_gen, 2, "b1 started gen 2 too");
    assert_eq!(s.dev_of(&s.c[HP].joined.as_ref().unwrap().host), OLGA);
    s.w.heal(HP, BOB);
    s.flush_relay();
    assert!(s.c[OLGA].hosting.is_none(), "b1 ended superseded");
    for m in [A1, CAROL, HP] {
        let j = s.c[m].joined.as_ref().unwrap();
        assert_eq!((j.call_id.as_str(), j.host_gen, s.dev_of(&j.host)), (cid.as_str(), 2, BOB), "{m}");
    }
    assert_eq!(roster_of(&s, BOB).len(), 3);
    assert!(s.c[HP].log.iter().any(|l| l == "end:Superseded"));
}

// KIND: invariant + limitation
// GC-S09. A member is removed mid-call. An honest H ejects her at once: End{removed} goes to her
// only, everyone else gets a new Roster, and her datagrams and re-Join are refused. A rogue H
// (the `World::rogue` fixture) keeps her. (a) If the rogue lists her in the Roster, honest
// members refuse that Roster (ASSUMPTION G10), so this is detectable. (b) If the rogue leaves
// her out of the Roster but forwards her audio under another participant's slot, members cannot
// tell (limitation, §6).
#[test]
fn gc_s09_removed_member_is_ejected_by_honest_host_and_kept_by_rogue_host() {
    let all = [OLGA, A1, A2, BOB, CAROL, HP, H1, H2];
    let (mut s, cid) = base();
    for m in [OLGA, A1, BOB, CAROL] {
        s.join_shown(m).unwrap();
    }
    let epoch = s.c[OLGA].joined.as_ref().unwrap().epoch;
    s.remove_member(CAROL, &all);
    assert!(s.c[CAROL].joined.is_none());
    assert!(s.c[CAROL].log.iter().any(|l| l == "end:Removed"));
    let carol_did = s.did(CAROL);
    for m in [OLGA, A1, BOB] {
        let j = s.c[m].joined.as_ref().unwrap();
        assert_eq!(j.epoch, epoch + 1);
        assert!(!j.roster.values().any(|d| *d == carol_did));
    }
    // Her datagrams on the old connection are refused, and so is a re-Join.
    let carol_ep = s.w.gep(CAROL);
    let mut d = vec![UP];
    d.extend(audio(1, 10));
    assert_eq!(s.host_ingress(H1, &carol_ep, &d), Err(CR::NotParticipant));
    s.advance(3_000);
    let req = s.join_req(CAROL, &cid, 1, s.now);
    assert_eq!(s.host_join(H1, &carol_ep, &req), Err(CR::NotMember));

    // Rogue H.
    let (mut r, _) = base();
    r.w.rogue.insert(H1);
    for m in [OLGA, A1, BOB, CAROL] {
        r.join_shown(m).unwrap();
    }
    r.remove_member(CAROL, &all);
    assert!(r.c[CAROL].joined.is_some(), "limitation: rogue host keeps the removed member");
    assert!(r.w.d[OLGA].view.removed.contains(&r.did(CAROL)));
    // (a) The rogue's Roster still lists her, so honest members refuse it. Their state is unchanged.
    let roster = r.roster_ctl(H1, None);
    let before: Vec<Vec<u8>> = [OLGA, A1, BOB].iter().map(|&m| r.snap(m)).collect();
    for (k, m) in [OLGA, A1, BOB].into_iter().enumerate() {
        assert_eq!(r.accept_roster(m, &roster.wire()), Err(CR::NotMember));
        assert!(before[k] == r.snap(m));
    }
    // (b) The rogue leaves her out of the Roster and forwards her audio under BOB's slot.
    let clean = r.roster_ctl(H1, Some(&r.did(CAROL)));
    for m in [OLGA, A1, BOB] {
        r.accept_roster(m, &clean.wire()).unwrap();
    }
    let bob_slot = r.c[H1].hosting.as_ref().unwrap().parts[&r.did(BOB)].slot;
    let mut injected = vec![DOWN, bob_slot];
    injected.extend(audio(7, 3)); // carol's packet, relabelled
    r.client_rx(OLGA, &injected);
    let j = r.c[OLGA].joined.as_ref().unwrap();
    assert!(!j.roster.values().any(|d| *d == r.did(CAROL)), "roster looks clean");
    assert_eq!(j.mixed.get(&r.did(BOB)), Some(&1), "limitation: carol's audio is attributed to bob");
}

// KIND: invariant + limitation
// GC-S10. A personal block between two participants (BOB blocks CAROL; U5 revised, U19, U20).
// Group membership and every grant stay the same. H's state is byte-identical, and nothing
// about the block reaches H. BOB's app mutes CAROL locally ("1 blocked participant"). His audio
// still reaches her, and every other member hears both. Limitation: the §3.4 pre-join warning
// "X is in this call" cannot be shown, because non-participants never get the Roster.
#[test]
fn gc_s10_personal_block_mutes_locally_and_the_host_learns_nothing() {
    let (mut s, _) = base();
    for m in [OLGA, CAROL] {
        s.join_shown(m).unwrap();
    }
    // Pre-join: BOB has the CallStart but no Roster, so there is nothing to warn from.
    assert!(s.c[BOB].joined.is_none() && s.c[BOB].shown.is_some());
    let carol = s.did(CAROL);
    let h1_before = s.snap(H1);
    let frames_before = s.w.frames.len();
    let rosters: Vec<_> = (0..s.w.d.len()).map(|i| s.w.d[i].board.roster.clone()).collect();
    s.w.block(BOB, &carol);
    assert!(s.snap(H1) == h1_before, "H learns nothing");
    assert!(s.w.frames[frames_before..].iter().all(|f| f.to != H1 && f.from != H1));
    for (i, r) in rosters.iter().enumerate() {
        assert_eq!(&s.w.d[i].board.roster, r, "membership unchanged at {i}");
    }
    s.advance(3_000);
    s.join_shown(BOB).unwrap();
    assert!(s.c[BOB].muted.contains(&carol));
    assert!(s.c[BOB].log.iter().any(|l| l == "1 blocked participant"));
    for _ in 0..50 {
        s.frame(H1, &[(BOB, 20), (CAROL, 22)]);
    }
    let bob = s.did(BOB);
    let carol_slot = s.c[H1].hosting.as_ref().unwrap().parts[&carol].slot;
    let jb = s.c[BOB].joined.as_ref().unwrap();
    assert!(jb.heard[&carol_slot] > 0, "H still forwards carol (it does not know)");
    assert_eq!(jb.mixed.get(&carol), None, "blocker's mix excludes the blocked slot");
    assert!(s.c[CAROL].joined.as_ref().unwrap().mixed[&bob] > 0, "blocker's audio reaches the blocked person");
    let jo = s.c[OLGA].joined.as_ref().unwrap();
    assert!(jo.mixed[&bob] > 0 && jo.mixed[&carol] > 0);
    assert!(s.c[CAROL].joined.as_ref().unwrap().mixed.get(&s.did(CAROL)).is_none(), "no self-echo");
}

// KIND: invariant
// GC-S11. A U12 rotation falls due while H1 (a chat host and call host) is hosting.
// (a) The call ends before the cap: the rotation waits for the end of the call, so it is late by
// less than ROTATION_DEFER_CAP_MS. (b) The call is still going at due + cap: H1 rotates, pushes
// its new records, sends End{host_rotating, next_host: self}, and members redial its new
// endpoint at host_gen+1. The old endpoint is dead. The removed member was refused from the
// moment of removal, and she never learns the new endpoint.
#[test]
fn gc_s11_u12_rotation_is_deferred_up_to_the_cap_then_members_redial_the_new_endpoint() {
    for cap_reached in [false, true] {
        let (mut s, cid) = base();
        for m in [OLGA, A1, BOB, CAROL] {
            s.join_shown(m).unwrap();
        }
        let removed_at = s.now;
        s.remove_member(CAROL, &[OLGA, A1, A2, BOB, CAROL, HP, H1, H2]);
        s.c[H1].rotation_due_ms = Some(removed_at + ROTATION_DELAY_MS);
        assert!(s.c[CAROL].joined.is_none(), "refusal is immediate");
        let old_ep = s.w.gep(H1);
        let due = removed_at + ROTATION_DELAY_MS;
        s.now = due;
        for m in [OLGA, A1, BOB] {
            s.c[m].joined.as_mut().unwrap().last_hb_ms = due;
        }
        s.c[H1].hosting.as_mut().unwrap().parts.values_mut().for_each(|p| p.last_ms = due);
        assert_eq!(s.rotation_tick(H1), "deferred");
        if !cap_reached {
            s.advance(20 * 60_000);
            assert_eq!(s.rotation_tick(H1), "deferred");
            s.host_end(H1, EndReason::Ended, None, None);
            assert_eq!(s.rotation_tick(H1), "rotated");
            assert!(s.now - due < ROTATION_DEFER_CAP_MS);
            assert_eq!(s.w.dial(OLGA, &old_ep), Err(RouteErr::EndpointGone));
            continue;
        }
        s.advance(ROTATION_DEFER_CAP_MS);
        assert_eq!(s.rotation_tick(H1), "rotated with End{host_rotating}");
        assert_eq!(s.now - due, ROTATION_DEFER_CAP_MS);
        let new_ep = s.w.gep(H1);
        assert_ne!(old_ep, new_ep);
        assert_eq!(s.w.dial(OLGA, &old_ep), Err(RouteErr::EndpointGone));
        for m in [OLGA, A1, BOB] {
            assert!(s.c[m].log.iter().any(|l| l == "end:HostRotating"));
            let j = s.c[m].joined.as_ref().unwrap_or_else(|| panic!("{m} did not redial"));
            assert_eq!((j.call_id.as_str(), j.host_gen, j.host_ep.as_str()), (cid.as_str(), 2, new_ep.as_str()));
        }
        // The removed member has only the dead address.
        let carol_rec = &s.c[CAROL].records[&s.w.hk(H1)];
        assert_eq!(carol_rec.note.claims.endpoint, old_ep);
        assert!(!has(&s.w.received(CAROL), new_ep.as_bytes()));
    }
}

// KIND: invariant
// GC-S12. A member's periodic group-endpoint rotation during a call. Deferred: the endpoint stays
// the same while she is in the call and rotates after she leaves. Forced: she sends her note to
// the chat hosts, sends Leave, and re-Joins from the new endpoint after JOIN_MIN_GAP_MS. The call
// keeps its call_id and host_gen. A Join that still carries the old note is refused as Stale.
#[test]
fn gc_s12_member_rotation_is_deferred_or_a_clean_leave_and_rejoin() {
    let (mut s, cid) = base();
    for m in [OLGA, A1, CAROL] {
        s.join_shown(m).unwrap();
    }
    s.advance(3_000);
    // Deferred.
    s.c[A1].rotation_due_ms = Some(s.now);
    let a1_ep = s.w.gep(A1);
    let deferred = s.c[A1].joined.is_some();
    assert!(deferred);
    s.advance(5_000);
    assert_eq!(s.w.gep(A1), a1_ep, "no rotation while in the call");
    s.leave(A1).unwrap();
    s.w.rotate_member(A1);
    s.c[A1].rotation_due_ms = None;
    assert_ne!(s.w.gep(A1), a1_ep);
    // Forced, for CAROL.
    let old_note = s.w.d[CAROL].my_note.clone();
    let epoch = s.c[OLGA].joined.as_ref().unwrap().epoch;
    let reached = s.w.rotate_member(CAROL);
    assert!(reached.contains(&H1), "note reached the chat host");
    s.leave(CAROL).unwrap();
    s.advance(JOIN_MIN_GAP_MS);
    // A Join with the old note from the old endpoint is refused.
    let mut stale: JoinReq = serde_json::from_slice(&s.join_req(CAROL, &cid, 1, s.now)).unwrap();
    stale.note = old_note.clone();
    let before = s.snap_all();
    assert_eq!(
        s.host_join(H1, &old_note.claims.endpoint, &serde_json::to_vec(&stale).unwrap()),
        Err(CR::Stale)
    );
    assert_unchanged(&s, &before, "old note");
    s.join_shown(CAROL).unwrap();
    let j = s.c[CAROL].joined.as_ref().unwrap();
    assert_eq!((j.call_id.as_str(), j.host_gen), (cid.as_str(), 1));
    assert_eq!(j.my_ep, s.w.gep(CAROL));
    let jo = s.c[OLGA].joined.as_ref().unwrap();
    assert_eq!(jo.epoch, epoch + 2, "Leave + re-Join; call continues");
    assert!(jo.roster.values().any(|d| *d == s.did(CAROL)));
}

// KIND: invariant
// GC-S13. Flooding is bounded. Oversize datagrams are dropped. More than MAX_UP_PPS is cut to the
// rate plus UP_BURST. Join spam within JOIN_MIN_GAP_MS is refused with state byte-identical, and
// so is control above MAX_CONTROL. The other members are unaffected: every datagram of theirs is
// forwarded and their rosters do not change.
#[test]
fn gc_s13_floods_are_bounded_and_other_members_are_unaffected() {
    let (mut s, cid) = base();
    for m in [OLGA, A1, CAROL] {
        s.join_shown(m).unwrap();
    }
    let a1_ep = s.c[A1].joined.as_ref().unwrap().my_ep.clone();
    let mut big = vec![UP];
    big.extend(vec![0u8; MAX_DATAGRAM]);
    let olga = s.did(OLGA);
    let mut accepted = 0u64;
    let secs = 5;
    for _ in 0..secs * 50 {
        s.now += FRAME_MS;
        s.speak(OLGA, 20).unwrap();
        s.speak(A1, 25).unwrap();
        for _ in 0..4 {
            if s.speak(CAROL, 30).is_ok() {
                accepted += 1;
            }
        }
        assert_eq!(s.host_ingress(H1, &a1_ep, &big), Err(CR::Oversize));
        s.host_frame(H1);
    }
    assert!(accepted <= secs * MAX_UP_PPS + UP_BURST, "flood bounded: {accepted}");
    assert!(accepted >= secs * MAX_UP_PPS, "rate honoured: {accepted}");
    let hc = s.c[H1].hosting.as_ref().unwrap();
    assert_eq!(hc.dropped.get(&olga), None, "olga never dropped");
    let olga_slot = hc.parts[&olga].slot;
    assert_eq!(s.c[A1].joined.as_ref().unwrap().heard[&olga_slot], secs * 50, "every olga frame forwarded");
    // A level-only flood (G6-D1 `[0x03][level]`) goes through the same bucket and is never
    // forwarded. Malformed level-only lengths are dropped.
    let rx0 = s.c[H1].hosting.as_ref().unwrap().level_only_rx;
    let f0 = s.w.frames.len();
    for _ in 0..50 {
        s.now += FRAME_MS;
        for _ in 0..4 {
            let _ = s.upstream(CAROL, vec![LEVEL_ONLY, 10]);
        }
        s.host_frame(H1);
    }
    let rx = s.c[H1].hosting.as_ref().unwrap().level_only_rx - rx0;
    assert!(rx <= MAX_UP_PPS + UP_BURST, "level-only flood bounded: {rx}");
    assert!(!s.w.frames[f0..].iter().any(|f| f.from == H1 && f.kind == "dgram"));
    let carol_conn = s.c[CAROL].joined.as_ref().unwrap().my_ep.clone();
    for bad in [vec![LEVEL_ONLY], vec![LEVEL_ONLY, 1, 2]] {
        assert_eq!(s.host_ingress(H1, &carol_conn, &bad), Err(CR::Malformed));
    }
    // Join spam.
    let roster = roster_of(&s, H1);
    let carol_ep = s.w.gep(CAROL);
    let first = s.join_req(CAROL, &cid, 1, s.now + 1);
    assert!(s.host_join(H1, &carol_ep, &first).is_ok(), "a newer Join after the gap is fine");
    for _ in 0..5 {
        let r = s.join_req(CAROL, &cid, 1, s.now + 2);
        let before = s.snap_all();
        assert_eq!(s.host_join(H1, &carol_ep, &r), Err(CR::RateLimited));
        assert_unchanged(&s, &before, "join spam");
    }
    let mut huge = s.join_req(BOB, &cid, 1, s.now);
    huge.extend(vec![b' '; MAX_CONTROL]);
    let before = s.snap_all();
    assert_eq!(s.host_join(H1, &s.w.gep(BOB), &huge), Err(CR::Oversize));
    assert_unchanged(&s, &before, "oversize control");
    assert_eq!(roster_of(&s, H1), roster);
}

// KIND: finding
// GC-S13 / §6. The spec claims "a Roster with up to 12 entries fits" in MAX_CONTROL. This
// measures a 12-entry Roster in two encodings. One is this simulation's JSON. The other is the
// proto `sign_blob` shape, where each Join is a SignedBlob nested in a base64url payload,
// approximated by length only.
#[test]
fn gc_s13b_twelve_entry_roster_size_against_the_control_limit() {
    let s = Sim::new();
    let cid = "ab".repeat(16);
    let ids: Vec<Identity> = (40u8..52).map(ident).collect();
    let entries: Vec<RosterEntry> = ids
        .iter()
        .enumerate()
        .map(|(k, id)| {
            let m = CallMsg {
                v: 1,
                gid: s.w.gid.clone(),
                call_id: cid.clone(),
                signer: id.did().into(),
                host_gen: 1,
                kind: Kind::Join {
                    issued_ms: 1_760_000_000_000,
                    nonce: u64::MAX - k as u64,
                },
            };
            RosterEntry {
                slot: k as u8,
                did: id.did().into(),
                join: Ctl::sign(id, m),
            }
        })
        .collect();
    let roster = s.msg(H1, &cid, 1, Kind::Roster { epoch: 1, entries: entries.clone() });
    let sim_len = roster.wire().len();
    let b64 = |n: usize| n.div_ceil(3) * 4;
    let blob = |payload: usize| b64(payload) + b64(64) + r#"{"payload":"","signature":""}"#.len();
    let join_payload = serde_json::to_vec(&entries[0].join.msg).unwrap().len();
    let entry_len = 12 + entries[0].did.len() + blob(join_payload) + 20;
    let proto_len = blob(12 * entry_len + 200);
    println!("12-entry Roster: sim JSON {sim_len} B, proto sign_blob shape ~{proto_len} B, limit {MAX_CONTROL} B");
    assert!(sim_len > MAX_CONTROL, "finding: sim encoding does not fit");
    assert!(proto_len > MAX_CONTROL, "finding: sign_blob encoding does not fit");
}

// KIND: invariant
// GC-S14. A U7 audit over a whole call lifecycle: statuses, ranking, start, joins, media, a
// member leaving, a forced member rotation and re-Join, a removal and ejection, a U12 rotation
// with End{host_rotating}, a host drop and redial to the backup at gen 3, and the end of the call. Afterwards, no member
// device has ever received or holds another member's group endpoint (any generation) or device
// slot, except devices that hold a call-host or chat-host permit, which U7 allows. The Roster
// holds DIDs and slots only.
#[test]
fn gc_s14_u7_audit_no_member_learns_another_members_endpoint_over_the_lifecycle() {
    let (mut s, cid) = base();
    let mut eps: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    let record_eps = |s: &Sim, eps: &mut BTreeMap<usize, BTreeSet<String>>| {
        for m in MEMBERS {
            eps.entry(m).or_default().insert(s.w.gep(m));
        }
    };
    record_eps(&s, &mut eps);
    let st: Vec<Status> = CALL_HOSTS.iter().map(|&h| s.status(h, s.now, STATUS_TTL_MS)).collect();
    let _ = s.rank_at(CAROL, &st, &cid, &s.did(CAROL), 4);
    for m in [A1, CAROL, HP, A2] {
        let _ = s.join_shown(m);
        s.advance(JOIN_MIN_GAP_MS);
    }
    for _ in 0..100 {
        s.frame(H1, &[(A2, 20), (CAROL, 25), (HP, 30)]);
    }
    s.leave(HP).unwrap();
    s.w.rotate_member(A2);
    record_eps(&s, &mut eps);
    s.leave(A2).unwrap();
    s.advance(JOIN_MIN_GAP_MS);
    s.join_shown(A2).unwrap();
    s.join_shown(HP).unwrap();
    s.remove_member(CAROL, &[OLGA, A1, A2, BOB, CAROL, HP, H1, H2]);
    s.c[H1].rotation_due_ms = Some(s.now);
    s.advance(ROTATION_DEFER_CAP_MS);
    assert_eq!(s.rotation_tick(H1), "rotated with End{host_rotating}");
    record_eps(&s, &mut eps);
    s.w.d[H1].online = false;
    s.advance(5_000);
    s.w.d[H1].online = true;
    assert_eq!(s.c[BOB].hosting.as_ref().unwrap().host_gen, 3);
    for _ in 0..50 {
        s.frame(BOB, &[(A2, 20), (HP, 22)]);
    }
    s.host_end(BOB, EndReason::Ended, None, None);
    // Devices that hosted hold participants' endpoints by design (U7), so they are not audited.
    // Endpoints of devices with a call-host or chat-host permit may be known to everyone.
    assert_eq!(s.ever_hosted, BTreeSet::from([H1, BOB]));
    let permitted: BTreeSet<usize> = CALL_HOSTS.iter().copied().chain([H1, H2]).collect();
    let audited: Vec<usize> = MEMBERS.iter().copied().filter(|m| !s.ever_hosted.contains(m)).collect();
    assert_eq!(audited, vec![OLGA, A1, A2, CAROL, HP]);
    for m in audited {
        let hay = s.w.received(m);
        for (&o, set) in &eps {
            if o == m || permitted.contains(&o) || s.did(o) == s.did(m) {
                continue;
            }
            for e in set {
                assert!(!has(&hay, e.as_bytes()), "member {m} holds member {o}'s group endpoint");
            }
            let slot = s.w.d[o].slot.to_string();
            assert!(!has(&hay, format!("\"slot\":{slot}").as_bytes()), "member {m} holds member {o}'s device slot");
            assert!(!has(&hay, format!("\"device_slot\":{slot}").as_bytes()));
        }
    }
    // Every Roster frame that was sent holds DIDs and slots only.
    for f in s.w.frames.iter().filter(|f| f.kind == "roster") {
        let r: Ctl = serde_json::from_slice(&f.bytes).unwrap();
        let Kind::Roster { entries, .. } = r.msg.kind else { panic!() };
        for e in entries {
            let j = serde_json::to_string(&e).unwrap();
            assert!(!j.contains("endpoint") && !j.contains("grp-") && !j.contains("device"), "{j}");
        }
    }
}

// KIND: invariant
// GC-S15. An admin revokes H1's host_call permit mid-call. Members drop the call and redial
// backups[0] at host_gen+1. If backups[0] is also revoked, they skip it and go to backups[1].
// Afterwards, control from the revoked host (a rogue that keeps sending) is refused.
#[test]
fn gc_s15_revoked_host_call_mid_call_members_redial_the_next_valid_backup() {
    let all = [OLGA, A1, A2, BOB, CAROL, HP, H1, H2];
    for both in [false, true] {
        let (mut s, cid) = base();
        for m in [A1, CAROL, HP] {
            s.join_shown(m).unwrap();
        }
        if both {
            let b = s.revoke_op(A1, BOB);
            for i in all {
                let _ = s.accept_revoke(i, &b);
            }
        }
        s.w.rogue.insert(H1); // keeps hosting after revocation
        s.revoke(A1, H1, &all);
        let want = if both { OLGA } else { BOB };
        for m in [A1, CAROL, HP] {
            let j = s.c[m].joined.as_ref().unwrap();
            assert_eq!((j.call_id.as_str(), j.host_gen, s.dev_of(&j.host)), (cid.as_str(), 2, want), "both={both} {m}");
        }
        // The rogue H1 still sends a Roster and an End at gen 1. Both are refused.
        let r = s.msg(H1, &cid, 1, Kind::Roster { epoch: 50, entries: vec![] });
        let e = s.msg(H1, &cid, 1, Kind::End { reason: EndReason::Ended, next_host: None });
        let before = s.snap_all();
        for m in [A1, CAROL, HP] {
            assert!(s.accept_roster(m, &r.wire()).is_err());
            assert!(s.accept_end(m, &e.wire()).is_err());
        }
        assert_unchanged(&s, &before, "revoked host control");
        assert_eq!(s.accept_call_record(A1, &s.record(H1).wire()), Err(CR::Revoked));
    }
}

// KIND: invariant
// GC-S16. The phone degrade thresholds (§2.4) as a pure table, then while hosting. A phone
// refuses to host listen-only. Under Limited it drops K to 1 and refuses new Joins. Under End it
// sends End{host_leaving, next_host: backups[0]}, and members redial at once with no heartbeat
// wait. A switch from Wi-Fi to metered keeps the call but refuses Joins beyond CAP_PHONE_METERED.
// The phone's own user leaving also ends the call with host_leaving.
#[test]
fn gc_s16_phone_degrade_reduces_k_refuses_joins_and_hands_over_with_host_leaving() {
    use PhoneAction::*;
    let p = |battery: u8, plugged: bool, thermal: Thermal, metered: bool| Cond {
        thermal,
        metered,
        ..Cond::phone(battery, plugged)
    };
    let table = [
        (p(30, false, Thermal::None, false), Advertise),
        (p(29, false, Thermal::None, false), StopAdvertising),
        (p(15, false, Thermal::None, false), StopAdvertising),
        (p(14, false, Thermal::None, false), Limited),
        (p(8, false, Thermal::None, false), Limited),
        (p(7, false, Thermal::None, false), End),
        (p(5, true, Thermal::None, false), Advertise),
        (p(90, true, Thermal::Light, false), Advertise),
        (p(90, true, Thermal::Moderate, false), StopAdvertising),
        (p(90, true, Thermal::Severe, false), Limited),
        (p(90, true, Thermal::Critical, false), End),
        (p(90, true, Thermal::None, true), StopAdvertising),
        (Cond { metered_override: true, ..p(90, true, Thermal::None, true) }, Advertise),
    ];
    for (c, want) in table {
        assert_eq!(phone_policy(&c), want, "{c:?}");
    }
    let mut s = Sim::new();
    s.c[HP].cond = Cond::phone(80, false);
    assert_eq!(s.start_call(HP, OLGA, vec![]), Err(CR::Refused("phone never hosts listen-only")));
    let cid = s.start_call(HP, HP, vec![s.host_ref(H1), s.host_ref(BOB)]).unwrap();
    assert_eq!(s.c[HP].hosting.as_ref().unwrap().k, K_PHONE);
    for m in [HP, OLGA, A1, BOB] {
        s.join_shown(m).unwrap();
    }
    // Stop advertising at thermal MODERATE: the status says busy, and the call continues.
    s.c[HP].cond.thermal = Thermal::Moderate;
    assert!(s.status(HP, s.now, STATUS_TTL_MS).body.busy);
    s.c[HP].cond.thermal = Thermal::None;
    // Limited at 14 %.
    s.c[HP].cond.battery = 14;
    assert_eq!(phone_policy(&s.c[HP].cond), Limited);
    {
        let hc = s.c[HP].hosting.as_mut().unwrap();
        hc.limited = true;
        hc.k = 1;
        hc.fwd.k = 1;
    }
    s.c[HP].log.push("hosting limited".into());
    let before = s.snap_all();
    let r = s.join_req(CAROL, &cid, 1, s.now);
    assert_eq!(s.host_join(HP, &s.w.gep(CAROL), &r), Err(CR::Limited));
    assert_unchanged(&s, &before, "limited");
    for _ in 0..30 {
        let set = s.frame(HP, &[(HP, 20), (OLGA, 22), (A1, 24)]);
        assert!(set.len() <= 1);
    }
    // End at 7 %. Members redial H1 at once.
    s.c[HP].cond.battery = 7;
    assert_eq!(phone_policy(&s.c[HP].cond), End);
    let t = s.now;
    let next = s.next_backup(HP);
    assert_eq!(next, Some(s.host_ref(H1)));
    s.host_end(HP, EndReason::HostLeaving, next, None);
    for m in [OLGA, A1, BOB] {
        let r = s.redials.iter().find(|(d, _)| *d == m).map(|(_, r)| r);
        assert_eq!(r, Some(&Redial::Joined { host: H1, at_ms: t }), "no heartbeat wait for {m}");
        assert_eq!(s.c[m].joined.as_ref().unwrap().host_gen, 2);
    }
    // Wi-Fi to metered mid-call: everyone stays, and new Joins beyond the metered cap are refused.
    let mut s = Sim::new();
    s.c[HP].cond = Cond::phone(80, true);
    let cid = s.start_call(HP, HP, vec![s.host_ref(H1)]).unwrap();
    for m in [HP, OLGA, A1, BOB, CAROL] {
        s.join_shown(m).unwrap();
    }
    s.c[HP].cond.metered = true;
    s.c[HP].hosting.as_mut().unwrap().warned_metered = true; // "data use" warning shown
    assert_eq!(roster_of(&s, HP).len(), 5, "continues above the metered cap");
    s.leave(A1).unwrap();
    s.advance(JOIN_MIN_GAP_MS);
    let r = s.join_req(A1, &cid, 1, s.now);
    let before = s.snap_all();
    assert_eq!(s.host_join(HP, &s.w.gep(A1), &r), Err(CR::Full));
    assert_unchanged(&s, &before, "metered cap");
    assert_eq!(roster_of(&s, HP).len(), CAP_PHONE_METERED);
    // The phone's own user leaves, which ends the call with host_leaving.
    s.leave(HP).unwrap();
    assert!(s.c[HP].hosting.is_none());
    for m in [OLGA, BOB, CAROL] {
        assert!(s.c[m].log.iter().any(|l| l == "end:HostLeaving"));
        assert_eq!(s.dev_of(&s.c[m].joined.as_ref().unwrap().host), H1);
    }
}

// KIND: invariant
// GC-S17. One DID joins from two linked devices (alice's A1 and A2). The newer Join replaces the
// older one, the older device's connection closes ("answered elsewhere"), and there is one slot
// per DID. These are refused with state byte-identical: a replay of the old Join, an older-issued
// Join from A1 (ASSUMPTION G5), and A1's datagrams after the switch. A newer Join from A1 takes
// the slot back.
#[test]
fn gc_s17_same_did_on_two_linked_devices_newer_join_replaces_older() {
    let (mut s, cid) = base();
    s.join_shown(OLGA).unwrap();
    s.join_shown(A1).unwrap();
    let a1_join = s.c[H1].hosting.as_ref().unwrap().parts[&s.did(A1)].join.clone();
    s.advance(JOIN_MIN_GAP_MS);
    s.join_shown(A2).unwrap();
    let alice = s.did(A1);
    let r = roster_of(&s, H1);
    assert_eq!(r.values().filter(|d| **d == alice).count(), 1, "one slot per DID");
    assert_eq!(r.len(), 2);
    assert_eq!(s.c[H1].hosting.as_ref().unwrap().parts[&alice].endpoint, s.w.gep(A2));
    assert!(s.c[A1].joined.is_none());
    assert!(s.c[A1].log.iter().any(|l| l == "call answered elsewhere"));
    assert!(s.c[A2].joined.is_some());
    let mut d = vec![UP];
    d.extend(audio(1, 10));
    assert_eq!(s.host_ingress(H1, &s.w.gep(A1), &d), Err(CR::NotParticipant));
    s.advance(JOIN_MIN_GAP_MS);
    let replay = serde_json::to_vec(&JoinReq {
        join: a1_join,
        note: s.w.d[A1].my_note.clone(),
    })
    .unwrap();
    let older = s.join_req(A1, &cid, 1, s.now - 10 * JOIN_MIN_GAP_MS);
    for (case, bytes, want) in [("replay", replay, CR::Replay), ("older issued_ms", older, CR::Stale)] {
        let before = s.snap_all();
        assert_eq!(s.host_join(H1, &s.w.gep(A1), &bytes), Err(want), "{case}");
        assert_unchanged(&s, &before, case);
    }
    s.join_shown(A1).unwrap();
    assert_eq!(s.c[H1].hosting.as_ref().unwrap().parts[&alice].endpoint, s.w.gep(A1));
    assert!(s.c[A2].joined.is_none());
    assert_eq!(roster_of(&s, H1).len(), 2);
}
