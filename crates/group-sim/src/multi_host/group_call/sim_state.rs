use super::*;

// Copies H sends: every datagram from a selected sender goes to every receiver except that
// sender (no self-echo).
pub(super) fn copies(
    set: &BTreeSet<u8>,
    sent: &BTreeSet<u8>,
    receivers: &BTreeSet<u8>,
) -> Vec<(u8, u8)> {
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
pub(super) fn mix(
    forwarded: &BTreeSet<u8>,
    roster: &BTreeMap<u8, String>,
    muted: &BTreeSet<String>,
) -> BTreeSet<u8> {
    forwarded
        .iter()
        .copied()
        .filter(|s| roster.get(s).is_some_and(|d| !muted.contains(d)))
        .collect()
}
pub(super) fn audio(seq: u32, level: u8) -> Vec<u8> {
    let mut v = seq.to_be_bytes().to_vec();
    v.push(level);
    v.extend([0xA5u8; 120]); // stand-in Opus payload (H never decodes it)
    v
}

// ---- per-device call state -----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) enum CR {
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
pub(super) struct Part {
    pub(super) slot: u8,
    pub(super) join: Ctl,
    pub(super) endpoint: String, // only the host holds participants' endpoints (U7)
    pub(super) last_ms: u64,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct HostCall {
    pub(super) call_id: String,
    pub(super) host_gen: u32,
    pub(super) start: Ctl,
    pub(super) k: usize,
    pub(super) max: usize,
    pub(super) epoch: u32,
    pub(super) parts: BTreeMap<String, Part>, // DID -> participant (one device per DID)
    pub(super) seen: BTreeSet<String>,        // Join ids (replay)
    pub(super) last_join: BTreeMap<String, u64>,
    pub(super) buckets: BTreeMap<String, (u64, u64)>, // DID -> (milli-tokens, last ms)
    pub(super) ctl_times: BTreeMap<String, VecDeque<u64>>,
    pub(super) limited: bool,
    pub(super) fwd: Fwd,
    pub(super) pending: BTreeMap<u8, Vec<Vec<u8>>>,
    pub(super) levels: BTreeMap<u8, u8>,
    pub(super) active_gen: u32,
    pub(super) active: BTreeSet<u8>,
    pub(super) level_only_rx: u64,
    pub(super) dropped: BTreeMap<String, u64>,
    pub(super) warned_metered: bool,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Dialing {
    pub(super) call_id: String,
    pub(super) host: HostRef,
    pub(super) host_ep: String,
    pub(super) host_gen: u32,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Joined {
    pub(super) call_id: String,
    pub(super) host: HostRef,
    pub(super) host_ep: String,
    pub(super) host_gen: u32,
    pub(super) my_ep: String,
    pub(super) epoch: u32,
    pub(super) roster: BTreeMap<u8, String>, // slot -> DID; DIDs and slots only (U7)
    pub(super) last_hb_ms: u64,
    pub(super) heard: BTreeMap<u8, u64>,
    pub(super) mixed: BTreeMap<String, u64>,
    pub(super) dropped: u64,
    pub(super) active_gen: u32,
    pub(super) active: BTreeSet<u8>,
}
#[derive(Clone, Debug, Default, Serialize)]
pub(super) struct CallDev {
    pub(super) cond: Cond,
    pub(super) consent: bool,
    pub(super) records: BTreeMap<String, HostRecord>, // accepted call-host records
    pub(super) revoked: BTreeSet<String>,             // revoked call-host permit digests
    pub(super) rev_ops: BTreeSet<String>,
    pub(super) hgen: BTreeMap<String, u32>, // call_id -> highest host_gen seen
    pub(super) starts: BTreeMap<String, Ctl>, // call_id -> accepted CallStart at that hgen
    pub(super) prev: BTreeMap<String, Ctl>, // call_id -> CallStart of the previous hgen
    pub(super) may_take: BTreeMap<String, u32>, // call_id -> hgen this device may start on a Join
    pub(super) shown: Option<String>,       // "Call in progress" (glare winner)
    pub(super) dialing: Option<Dialing>,
    pub(super) joined: Option<Joined>,
    pub(super) last_call: Option<String>,
    pub(super) muted: BTreeSet<String>,
    pub(super) log: Vec<String>,
    pub(super) hosting: Option<HostCall>,
    pub(super) rotation_due_ms: Option<u64>,
    pub(super) seq: u32,
    pub(super) quiet: u32,
    pub(super) gated: bool,
    pub(super) since_level: u32,
    pub(super) opus_sent: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Redial {
    Joined { host: usize, at_ms: u64 },
    HostLost { at_ms: u64 },
}

pub(super) const CALL_HOSTS: [usize; 4] = [H1, BOB, OLGA, HP];

pub(super) struct Sim {
    pub(super) w: World,
    pub(super) c: Vec<CallDev>,
    pub(super) permits: BTreeMap<usize, MGrant>,
    pub(super) now: u64,
    pub(super) hold_relay: bool,
    pub(super) held: Vec<Vec<u8>>,
    pub(super) redials: Vec<(usize, Redial)>,
    pub(super) queue: Vec<(usize, String, HostRef, u32, Joined)>,
    pub(super) ever_hosted: BTreeSet<usize>,
}
