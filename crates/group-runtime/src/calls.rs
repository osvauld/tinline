//! Group calls over group chat (GA-I1; docs/design/group-call-spec.md §2, §3, §5, §11).
//!
//! Calls run on the group endpoint (U2): members dial only the call host's group endpoint,
//! the call host learns callers' group endpoints, nobody else does (U7, U21). Everything about
//! a call that is not media travels through the chat hosts as `GroupMsg::Call*`:
//!
//! - **Status** (§2.2): every device that is an active call host (`CallHostPermit` +
//!   `CallHostAccept`, `state::call_hosts`) and has not paused hosting publishes a signed
//!   `CallHostStatus` every 30 s and as soon as it changes (conditions, busy). Every device keeps
//!   the admitted ones per group (`rank::admit`, wall clock).
//! - **Start** (§2.3): the starter ranks the fresh statuses, then asks candidates in order with
//!   a signed `CallHostRequest` addressed to that device (the `rank::Picker`: next one on a
//!   refusal or after 5 s). Asking itself needs no message. The asked device re-checks its
//!   consent and conditions (paused, busy, a phone hosts only calls its own user is in) and
//!   either hosts (`gcall_host` with the request's backups; listen-only on a desktop) or
//!   answers a signed `CallHostRefusal`.
//! - **Announce**: the host's `CallStart` (also a takeover's, gen + 1) goes to every member
//!   with the DID-signed group addresses of the host and its backups (§9 G1). Members see it
//!   in `group_calls` and an `Announced` event, feed it to the call board
//!   (`gcall_observe_start`: takeover, glare), and join on demand (`group_call_join`); the
//!   starter joins at once. A hosted call's `End{ended}` is relayed the same way.
//! - **Relay rules**: members send only to their chat hosts. A host verifies every frame
//!   (signature, group, membership and not removed, call-host permit, sender = signer for
//!   frames from a member) and passes broadcast frames to its current member sessions (never a
//!   removed or still-joining one, §9.3) and to other hosts, addressed frames towards their
//!   recipient. Every device drops a blob whose signature it has seen.
//!
//! A listen-only call host ends the call after `IDLE_END_MS` with nobody in it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use proto::Signed;
use proto::gcall::{self as w, CallStart, HostRefusal, HostRequest, MAX_BACKUPS, MAX_PARTICIPANTS};
use proto::gcall::{SignedEnd, SignedStart};
use proto::gcall_status::SignedStatus;
use proto::gcall_status::{
    self as gs, CallHostStatus, DeviceClass, NetworkKind, PowerState, STATUS_REFRESH_MS,
    STATUS_TTL_MS, ThermalLevel,
};

use super::calldir::{GroupCallDir, resolve};
use super::engine::{GroupCore, Out, Sess};
use super::record::SignedHostAddr;
use super::relay::{Route, relay_targets};
use super::wire::GroupMsg;
use crate::Error;
use gcall_net::{CallDirectory, GcallConfig, GcallEvent, GcallEvents};
use gcall_core::rank::{self, Picker, PickerStep, RankCtx};
use gcall_core::{CallAuthority, ClientState, Class, Cond, EndReason, HostConfig, HostNotice, HostRef, PhoneAction, Thermal, phone_policy};
use crate::GroupRt;
use node_runtime::now_ms;

/// A listen-only host ends a call nobody is in after this long.
pub const IDLE_END_MS: u64 = 20_000;
/// A `CallHostRequest` older than this (or this far in the future) is ignored.
pub const REQUEST_MAX_AGE_MS: u64 = 60_000;
/// Calls are offered to (re)connecting members this long after they started.
pub const CALL_KEEP_MS: u64 = 4 * 3600 * 1000;
/// A group call someone else starts rings (like an incoming call) for at most this long; the
/// announcement itself stays (a quiet "Join").
pub const GROUP_CALL_RING_MS: u64 = 30_000;
/// A host's status saying "not busy" this long after a call it hosted started means that call
/// is over (one hosted call per node), whatever End we missed.
pub const HOST_IDLE_GRACE_MS: u64 = 10_000;
/// Seen-blob memory bound (relay dedupe).
const SEEN_MAX: usize = 4096;
/// A signed blob larger than this is not relayed (statuses are ~0.6 KiB, CallStarts ~1.5 KiB).
const MAX_BLOB: usize = 2 * w::MAX_CONTROL;

mod recv;
mod start;
mod status;
mod types;
pub use types::*;
use status::*;

#[derive(Default)]
pub struct GroupCalls {
    /// Groups whose call hosting this device paused (`group_call_hosting_enable(false)`).
    pub paused: Mutex<HashSet<String>>,
    pub events: Mutex<Option<Arc<dyn GroupCallSink>>>,
    pub st: Mutex<CallsSt>,
    /// What the call log is tracking now (calllog.rs).
    pub log: Mutex<super::calllog::CallLog>,
}

#[derive(Default)]
pub struct CallsSt {
    /// gid → (holder DID, device slot) → latest admitted status.
    pub statuses: HashMap<String, BTreeMap<(String, u64), (SignedStatus, CallHostStatus)>>,
    /// gid → call id → highest generation's CallStart.
    pub calls: HashMap<String, BTreeMap<String, (SignedStart, CallStart)>>,
    pub ended: HashSet<String>,
    /// call id → (gid, host DID, the signed End, forget after): every End{ended} seen, also for a
    /// call we never heard of (its CallStart may still arrive from a member that missed the End,
    /// and is then dropped). Sent on to a member that connects later, so nobody keeps a call alive.
    pub ends: HashMap<String, (String, String, SignedEnd, u64)>,
    /// Relay dedupe: signature → forget after (unix ms).
    pub seen: HashMap<[u8; 64], u64>,
    pub pending: Option<Pending>,
    /// gid → (our last status with `issued_ms` 0, its signed blob, when).
    pub published: HashMap<String, (CallHostStatus, SignedStatus, u64)>,
    /// A call hosted here has had nobody in it since.
    pub empty_since: Option<u64>,
}

/// The starter's pick in progress.
pub struct Pending {
    gid: String,
    call_id: String,
    expected: u8,
    picker: Picker,
}

struct Sink(Weak<GroupRt>);

impl GcallEvents for Sink {
    fn on_gcall_event(&self, ev: GcallEvent) {
        if let Some(i) = self.0.upgrade() {
            i.group_call_event(ev);
        }
    }
}

fn short(gid: &str) -> &str {
    super::short(gid)
}

impl GroupRt {
    // ---- configuration --------------------------------------------------------------------

    /// The group-call service runs over group state unless an app or test configured it.
    pub fn group_calls_configure(self: &Arc<Self>) {
        let cfg = GcallConfig {
            dir: Arc::new(GroupCallDir::new(self)),
            events: Arc::new(Sink(Arc::downgrade(self))),
            host_ref: None,
            cond: self.gcall.cond(),
            note: None,
        };
        self.gcall.configure_if_unset(cfg);
    }

    fn calls_emit(&self, kind: GroupCallEventKind, gid: &str, call_id: &str, participants: Vec<String>, detail: String) {
        self.calls_record(kind, gid, call_id, &participants, &detail);
        let Some(ev) = self.calls.events.lock().clone() else { return };
        let e = GroupCallEvent { kind, group_id: gid.into(), call_id: call_id.into(), participants, detail };
        self.handle.spawn_blocking(move || ev.on_group_call(e));
    }

    /// Feeds the call log (calllog.rs) and stores what a finished session leaves.
    fn calls_record(&self, kind: GroupCallEventKind, gid: &str, call_id: &str, participants: &[String], detail: &str) {
        if gid.is_empty() {
            return;
        }
        // try_lock: never wait on the call table from inside an emit.
        let known = self.calls.st.try_lock().and_then(|st| st.calls.get(gid)?.get(call_id).map(|(_, s)| (s.started_ms, s.starter_did.clone())));
        let done = self.calls.log.lock().apply(kind, gid, call_id, participants, detail, now_ms(), known);
        if let (Some(f), Some(core)) = (done, self.core.lock().clone())
            && let Err(e) = core.store.clog_record(&f)
        {
            tracing::warn!("group call log: {e}");
        }
    }

    pub fn call_gid(&self, call_id: &str) -> Option<String> {
        let st = self.calls.st.lock();
        st.calls.iter().find(|(_, c)| c.contains_key(call_id)).map(|(g, _)| g.clone())
    }

    /// True the first time `sig` is seen (relay dedupe).
    fn calls_first_sight(&self, sig: &[u8; 64], until: u64) -> bool {
        let mut st = self.calls.st.lock();
        if st.seen.contains_key(sig) {
            return false;
        }
        if st.seen.len() >= SEEN_MAX {
            let now = now_ms();
            st.seen.retain(|_, t| *t > now);
            if st.seen.len() >= SEEN_MAX {
                st.seen.clear();
            }
        }
        st.seen.insert(*sig, until);
        true
    }

    // ---- sending ----------------------------------------------------------------------------

    /// Sends `msg` on: a member to its hosts (only what it originates, `from = None`); a host to
    /// other hosts and to the current members `route` names.
    fn calls_send(&self, core: &GroupCore, gid: &str, msg: &GroupMsg, route: &Route, from: Option<u64>) {
        for s in relay_targets(core, gid, from, route) {
            let _ = s.tx.send(Out::Msg(msg.clone()));
        }
    }

    fn calls_originate(
        &self,
        core: &GroupCore,
        gid: &str,
        sig: &[u8; 64],
        msg: GroupMsg,
        route: Route,
    ) {
        self.calls_first_sight(sig, now_ms() + CALL_KEEP_MS);
        self.calls_send(core, gid, &msg, &route, None);
    }

    /// The DID-signed group addresses of `s`'s host and backups (what callers dial).
    fn call_addrs(&self, core: &GroupCore, gid: &str, s: &CallStart) -> Vec<SignedHostAddr> {
        let devs: Vec<(String, [u8; 32])> = {
            let groups = core.groups.lock();
            let Some(l) = groups.get(gid) else { return vec![] };
            std::iter::once(&s.host_ref).chain(s.backups.iter()).filter_map(|r| resolve(&l.state, r).map(|h| (h.did.clone(), h.dev))).collect()
        };
        let mut out: Vec<SignedHostAddr> = core
            .store
            .hosts(gid)
            .unwrap_or_default()
            .into_iter()
            .filter(|a| {
                a.open()
                    .ok()
                    .is_some_and(|a| devs.contains(&(a.did.clone(), a.dev)))
            })
            .collect();
        if devs.contains(&(core.did.clone(), core.dev))
            && let Some(mine) = self.group_my_addr(core, gid)
        {
            out.push(mine);
        }
        out.truncate(1 + MAX_BACKUPS);
        out
    }
}

impl GroupRt {
    // ---- the call layer's events --------------------------------------------------------------

    fn group_call_event(self: &Arc<Self>, ev: GcallEvent) {
        let core = self.core.lock().clone();
        let gid_of = |call_id: &str| self.call_gid(call_id).unwrap_or_default();
        let (kind, gid, call_id, participants, detail) = match &ev {
            GcallEvent::HostingStarted {
                call_id,
                host_gen,
                start,
            } => {
                let gid = start.open().map(|s| s.gid).unwrap_or_default();
                self.calls.st.lock().empty_since = None;
                if let (Some(core), Ok(s)) = (core.as_ref(), start.open()) {
                    let hosts = self.call_addrs(core, &s.gid, &s);
                    self.group_call_started(&s.gid, start.clone(), s.clone());
                    let sig = *start.sig();
                    self.calls_originate(
                        core,
                        &s.gid,
                        &sig,
                        GroupMsg::CallStart {
                            start: start.clone(),
                            hosts,
                        },
                        Route::All,
                    );
                }
                (GroupCallEventKind::HostingStarted, gid, call_id.clone(), vec![], format!("generation {host_gen}"))
            }
            GcallEvent::HostingEnded { call_id, reason, end, .. } => {
                let gid = gid_of(call_id);
                if *reason == EndReason::Ended
                    && let (Some(core), Some(end)) = (core.as_ref(), end.as_ref())
                {
                    self.group_call_ended(&gid, call_id, end);
                    let sig = *end.sig();
                    self.calls_originate(
                        core,
                        &gid,
                        &sig,
                        GroupMsg::CallEnd { end: end.clone() },
                        Route::All,
                    );
                }
                (GroupCallEventKind::HostingEnded, gid, call_id.clone(), vec![], format!("{reason:?}"))
            }
            GcallEvent::Joined { call_id, host, .. } => (GroupCallEventKind::Joined, gid_of(call_id), call_id.clone(), vec![], host.holder_did.clone()),
            GcallEvent::RosterChanged { call_id, roster, .. } => (GroupCallEventKind::RosterChanged, gid_of(call_id), call_id.clone(), roster.values().cloned().collect(), String::new()),
            GcallEvent::Reconnecting { call_id } => (GroupCallEventKind::Reconnecting, gid_of(call_id), call_id.clone(), vec![], String::new()),
            GcallEvent::Superseded { call_id, by } => (GroupCallEventKind::Reconnecting, gid_of(call_id), call_id.clone(), vec![], format!("superseded by {}", by.holder_did)),
            GcallEvent::Ended { call_id, why } => (GroupCallEventKind::Left, gid_of(call_id), call_id.clone(), vec![], format!("{why:?}")),
            GcallEvent::Notice(n) => {
                let call_id = self.gcall.status().map(|s| s.call_id).unwrap_or_default();
                let kind = match n {
                    HostNotice::Limited => GroupCallEventKind::HostingLimited,
                    HostNotice::Unlimited => GroupCallEventKind::HostingUnlimited,
                    HostNotice::MeteredData => GroupCallEventKind::MeteredData,
                };
                (kind, gid_of(&call_id), call_id, vec![], String::new())
            }
        };
        self.calls_emit(kind, &gid, &call_id, participants, detail);
    }

    pub fn group_calls_list(&self, core: &GroupCore, gid: &str) -> Vec<GroupCall> {
        let status = self.gcall.status();
        let calls: Vec<CallStart> = self.calls.st.lock().calls.get(gid).map(|c| c.values().map(|(_, s)| s.clone()).collect()).unwrap_or_default();
        let groups = core.groups.lock();
        let state = groups.get(gid).map(|l| &l.state);
        calls
            .into_iter()
            .map(|s| {
                let here = status.as_ref().filter(|st| st.call_id == s.call_id);
                GroupCall {
                    group_id: gid.to_string(),
                    host_device_id: state.and_then(|st| resolve(st, &s.host_ref)).map(|h| hex::encode(h.dev)),
                    call_id: s.call_id.clone(),
                    host_did: s.host_ref.holder_did.clone(),
                    starter_did: s.starter_did.clone(),
                    started_at: s.started_ms,
                    host_gen: s.host_gen,
                    hosted_here: here.is_some_and(|h| h.hosting.is_some()),
                    joined: here.and_then(|h| h.client.as_ref()).is_some_and(|c| c.state == ClientState::Joined),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;

/// The node's side of the group-call service: the account, the group endpoint and the `live` lock.
impl gcall_net::GcallHost for GroupRt {
    fn identity(&self) -> Result<identity::Identity, gcall_net::Error> {
        Ok(self.me().map_err(gcall_err)?.id.clone())
    }

    fn group_endpoint(&self) -> Option<iroh::Endpoint> {
        GroupRt::group_endpoint(self)
    }

    /// Runs `f` under the `live` lock (lock order: live -> gcall session).
    fn with_no_call(&self, f: &mut dyn FnMut() -> Result<(), gcall_net::Error>) -> Result<(), gcall_net::Error> {
        self.ctx_with_no_call(f)
    }
}

/// A group-call service error as a node error (the variants map one to one).
pub fn from_gcall(e: gcall_net::Error) -> Error {
    use gcall_net::Error as G;
    match e {
        G::NoIdentity => Error::NoIdentity,
        G::InCall => Error::InCall,
        G::Locked => Error::Locked,
        G::NotStarted => Error::NotStarted,
        G::Busy => Error::Busy,
        G::Timeout => Error::Timeout,
        G::Rejected(s) => Error::Rejected(s),
        G::Protocol(s) => Error::Protocol(s),
        G::Net(s) => Error::Net(s),
    }
}

/// A node error for the group-call service: only the variants the host closure can produce are
/// distinguished; the rest are text.
pub fn gcall_err(e: Error) -> gcall_net::Error {
    use gcall_net::Error as G;
    match e {
        Error::NoIdentity => G::NoIdentity,
        Error::InCall => G::InCall,
        Error::Locked => G::Locked,
        Error::NotStarted => G::NotStarted,
        Error::Busy => G::Busy,
        Error::Timeout => G::Timeout,
        Error::Rejected(s) => G::Rejected(s),
        Error::Protocol(s) => G::Protocol(s),
        Error::Net(s) => G::Net(s),
        other => G::Protocol(other.to_string()),
    }
}

impl GroupRt {
    fn ctx_with_no_call(&self, f: &mut dyn FnMut() -> Result<(), gcall_net::Error>) -> Result<(), gcall_net::Error> {
        self.with_no_call(|| f().map_err(from_gcall)).map_err(gcall_err)
    }

    /// `Node::stop` / `Node::lock` / an account switch, before the endpoints close.
    pub async fn gcall_shutdown(&self) {
        self.gcall.shutdown().await
    }
}
