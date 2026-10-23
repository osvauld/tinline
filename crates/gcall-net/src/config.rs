//! Close codes, the directory, events, configuration and status types of group calls.

use super::*;


/// QUIC application close codes on `tinline/gcall/1`. 0 is a plain goodbye (left, link
/// replaced). The rest mirror [`CloseReason`]; `REFUSED` is a Join that was not accepted.
pub const CLOSE_BYE: u32 = 0;
pub const CLOSE_ENDED: u32 = 0x10;
pub const CLOSE_ANSWERED_ELSEWHERE: u32 = 0x11;
pub const CLOSE_REMOVED: u32 = 0x12;
pub const CLOSE_DROPPED: u32 = 0x13;
pub const CLOSE_REFUSED: u32 = 0x14;

/// Connect + first control frame, per attempt. The engine's own `ANSWER_TIMEOUT_MS` covers a
/// host that connects but never answers.
pub(super) const DIAL_TIMEOUT: Duration = Duration::from_millis(ANSWER_TIMEOUT_MS);
/// How long a closing writer waits for the peer to have read its last frames.
pub(super) const FLUSH_TIMEOUT: Duration = Duration::from_secs(1);
/// After the control stream ends, how long to wait for the peer's close (and its code).
pub(super) const CLOSE_WAIT: Duration = Duration::from_secs(2);
/// Level byte of the idle keepalive: silence (127 = -127 dBov).
pub(super) const SILENT_LEVEL: u8 = 127;
/// How long a `Superseded` waits for the winner's CallStart to arrive through the relay.
pub const FOLLOW_WINDOW: Duration = Duration::from_secs(30);

pub fn close_code(r: CloseReason) -> u32 {
    match r {
        CloseReason::Ended => CLOSE_ENDED,
        CloseReason::AnsweredElsewhere => CLOSE_ANSWERED_ELSEWHERE,
        CloseReason::Removed => CLOSE_REMOVED,
        CloseReason::Dropped => CLOSE_DROPPED,
    }
}

/// What the wiring needs to know about groups: the [`CallAuthority`] checks plus where a call
/// host can be reached. Group chat implements it over its derived state
/// (`group::calldir::GroupCallDir`, GA-I1); [`StaticDirectory`] is the fixed one for tests.
pub trait CallDirectory: CallAuthority + Send + Sync {
    /// The transport address of `host`'s group endpoint, if known.
    fn host_addr(&self, host: &HostRef) -> Option<EndpointAddr>;
    /// This device's call-host ref in `gid`, when [`GcallConfig::host_ref`] is not set (a
    /// device may host calls of several groups, each under its own permit).
    fn my_host_ref(&self, _gid: &str) -> Option<HostRef> {
        None
    }
    /// This device's DID-signed group endpoint note for a Join, when [`GcallConfig::note`] is
    /// not set.
    fn my_note(&self) -> Option<SignedNote> {
        None
    }
}

/// A [`CallDirectory`] over a [`StaticAuthority`] and a fixed host address book; for tests and
/// until group chat provides the real one. Both halves may be changed while a call runs.
#[derive(Default)]
pub struct StaticDirectory {
    pub auth: Mutex<StaticAuthority>,
    pub hosts: Mutex<BTreeMap<HostRef, EndpointAddr>>,
}

impl CallAuthority for StaticDirectory {
    fn is_member(&self, gid: &str, did: &str) -> bool {
        self.auth.lock().is_member(gid, did)
    }
    fn host_permit_ok(&self, gid: &str, host: &HostRef) -> bool {
        self.auth.lock().host_permit_ok(gid, host)
    }
    fn endpoint_ok(
        &self,
        gid: &str,
        did: &str,
        note: Option<&SignedNote>,
        endpoint: &[u8; 32],
    ) -> bool {
        self.auth.lock().endpoint_ok(gid, did, note, endpoint)
    }
}

impl CallDirectory for StaticDirectory {
    fn host_addr(&self, host: &HostRef) -> Option<EndpointAddr> {
        self.hosts.lock().get(host).cloned()
    }
}

/// Events out of the group-call layer. Delivered on core threads, outside any lock.
#[derive(Debug, Clone, PartialEq)]
pub enum GcallEvent {
    /// This device is in the Roster of `host` at `host_gen` (also after a redial).
    Joined { call_id: String, host_gen: u32, slot: u8, host: HostRef },
    /// slot → DID.
    RosterChanged { call_id: String, host_gen: u32, roster: BTreeMap<u8, String> },
    /// Host lost or handing over; redialing the backups ("Reconnecting…").
    Reconnecting { call_id: String },
    /// The call lost a glare or split; the call hosted by `by` is joined automatically when
    /// its CallStart is known (`gcall_observe_start`).
    Superseded { call_id: String, by: HostRef },
    /// This device's participation ended.
    Ended { call_id: String, why: ClientEnd },
    /// This device now hosts `call_id` at `host_gen` (a start or a takeover). `start` is the
    /// signed CallStart to announce through the chat hosts (G1).
    HostingStarted {
        call_id: String,
        host_gen: u32,
        start: SignedStart,
    },
    /// This device stopped hosting. `end` is the signed End (relay it when `reason` is `Ended`).
    HostingEnded {
        call_id: String,
        reason: EndReason,
        next_host: Option<HostRef>,
        end: Option<SignedEnd>,
    },
    Notice(HostNotice),
}

pub trait GcallEvents: Send + Sync {
    fn on_gcall_event(&self, ev: GcallEvent);
}

/// Set once per node (`gcall_configure`) before hosting or joining.
#[derive(Clone)]
pub struct GcallConfig {
    pub dir: Arc<dyn CallDirectory>,
    pub events: Arc<dyn GcallEvents>,
    /// This device's call-host ref, if it holds a `host_call` permit and has hosting enabled.
    /// Needed to host, and to take over as a backup.
    pub host_ref: Option<HostRef>,
    /// Local hosting conditions (§2.4).
    pub cond: Cond,
    /// This device's DID-signed group endpoint note, sent with every Join (G2).
    pub note: Option<SignedNote>,
}

impl GcallConfig {
    /// This device's host ref for `gid`: the configured one, else the directory's.
    pub fn host_ref_for(&self, gid: &str) -> Option<HostRef> {
        self.host_ref.clone().or_else(|| self.dir.my_host_ref(gid))
    }

    pub(super) fn note_now(&self) -> Option<SignedNote> {
        self.note.clone().or_else(|| self.dir.my_note())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientStatus {
    pub state: ClientState,
    pub host: HostRef,
    pub host_gen: u32,
    pub my_slot: Option<u8>,
    pub roster: BTreeMap<u8, String>,
    /// Slots the host forwards (`Active`).
    pub active: BTreeSet<u8>,
    /// Downstream datagrams accepted per roster slot.
    pub heard: BTreeMap<u8, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostingStatus {
    pub host_gen: u32,
    pub k: usize,
    pub epoch: u32,
    pub roster: BTreeMap<u8, String>,
    /// The forwarded set S (plus the G6-D2 provisional slot).
    pub forwarded: BTreeSet<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcallStatus {
    pub call_id: String,
    pub client: Option<ClientStatus>,
    pub hosting: Option<HostingStatus>,
}
