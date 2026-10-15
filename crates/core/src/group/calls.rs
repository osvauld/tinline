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
use proto::SignedBlob;
use proto::gcall::{self as w, CallStart, GCALL_VERSION, HostRefusal, HostRequest, MAX_BACKUPS, MAX_PARTICIPANTS};
use proto::gcall_status::{self as gs, CallHostStatus, DeviceClass, NetworkKind, PowerState, STATUS_REFRESH_MS, STATUS_TTL_MS, ThermalLevel};

use super::calldir::{GroupCallDir, resolve};
use super::engine::{GroupCore, Out, Sess};
use super::record::SignedHostAddr;
use super::relay::{Route, relay_targets};
use super::wire::GroupMsg;
use crate::Error;
use crate::gcall::net::{CallDirectory, GcallConfig, GcallEvent, GcallEvents};
use crate::gcall::rank::{self, Picker, PickerStep, RankCtx};
use crate::gcall::{CallAuthority, ClientState, Class, Cond, EndReason, HostConfig, HostNotice, HostRef, PhoneAction, Thermal, phone_policy};
use crate::node::{Inner, Node, now_ms};

/// A listen-only host ends a call nobody is in after this long.
pub const IDLE_END_MS: u64 = 20_000;
/// A `CallHostRequest` older than this (or this far in the future) is ignored.
pub const REQUEST_MAX_AGE_MS: u64 = 60_000;
/// Calls are offered to (re)connecting members this long after they started.
pub const CALL_KEEP_MS: u64 = 4 * 3600 * 1000;
/// A group call someone else starts rings (like an incoming call) for at most this long; the
/// announcement itself stays (a quiet "Join").
pub const GROUP_CALL_RING_MS: u64 = 30_000;
/// Seen-blob memory bound (relay dedupe).
const SEEN_MAX: usize = 4096;
/// A signed blob larger than this is not relayed (statuses are ~0.6 KiB, CallStarts ~1.5 KiB).
const MAX_BLOB: usize = 2 * w::MAX_CONTROL;

mod node;
mod recv;
mod start;
mod status;
mod types;
pub use types::*;
use status::*;

#[derive(Default)]
pub(crate) struct GroupCalls {
    /// Groups whose call hosting this device paused (`group_call_hosting_enable(false)`).
    pub(crate) paused: Mutex<HashSet<String>>,
    pub(crate) events: Mutex<Option<Arc<dyn GroupCallEvents>>>,
    st: Mutex<CallsSt>,
    /// What the call log is tracking now (calllog.rs).
    pub(crate) log: Mutex<super::calllog::CallLog>,
}

#[derive(Default)]
struct CallsSt {
    /// gid → (holder DID, device slot) → latest admitted status.
    statuses: HashMap<String, BTreeMap<(String, u64), (SignedBlob, CallHostStatus)>>,
    /// gid → call id → highest generation's CallStart.
    calls: HashMap<String, BTreeMap<String, (SignedBlob, CallStart)>>,
    ended: HashSet<String>,
    /// call id → (gid, host DID, the signed End, forget after): every End{ended} seen, also for a
    /// call we never heard of (its CallStart may still arrive from a member that missed the End,
    /// and is then dropped). Sent on to a member that connects later, so nobody keeps a call alive.
    ends: HashMap<String, (String, String, SignedBlob, u64)>,
    /// Relay dedupe: signature → forget after (unix ms).
    seen: HashMap<String, u64>,
    pending: Option<Pending>,
    /// gid → (our last status with `issued_ms` 0, its signed blob, when).
    published: HashMap<String, (CallHostStatus, SignedBlob, u64)>,
    /// A call hosted here has had nobody in it since.
    empty_since: Option<u64>,
}

/// The starter's pick in progress.
struct Pending {
    gid: String,
    call_id: String,
    expected: u8,
    picker: Picker,
}

struct Sink(Weak<Inner>);

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

impl Inner {
    // ---- configuration --------------------------------------------------------------------

    /// The group-call service runs over group state unless an app or test configured it.
    pub(crate) fn group_calls_configure(self: &Arc<Self>) {
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
        let Some(ev) = self.group.calls.events.lock().clone() else { return };
        let e = GroupCallEvent { kind, group_id: gid.into(), call_id: call_id.into(), participants, detail };
        self.handle.spawn_blocking(move || ev.on_group_call(e));
    }

    /// Feeds the call log (calllog.rs) and stores what a finished session leaves.
    fn calls_record(&self, kind: GroupCallEventKind, gid: &str, call_id: &str, participants: &[String], detail: &str) {
        if gid.is_empty() {
            return;
        }
        // try_lock: never wait on the call table from inside an emit.
        let known = self.group.calls.st.try_lock().and_then(|st| st.calls.get(gid)?.get(call_id).map(|(_, s)| (s.started_ms, s.starter_did.clone())));
        let done = self.group.calls.log.lock().apply(kind, gid, call_id, participants, detail, now_ms(), known);
        if let (Some(f), Some(core)) = (done, self.group.core.lock().clone())
            && let Err(e) = core.store.clog_record(&f)
        {
            tracing::warn!("group call log: {e}");
        }
    }

    fn call_gid(&self, call_id: &str) -> Option<String> {
        let st = self.group.calls.st.lock();
        st.calls.iter().find(|(_, c)| c.contains_key(call_id)).map(|(g, _)| g.clone())
    }

    /// True the first time `sig` is seen (relay dedupe).
    fn calls_first_sight(&self, sig: &str, until: u64) -> bool {
        let mut st = self.group.calls.st.lock();
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
        st.seen.insert(sig.to_string(), until);
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

    fn calls_originate(&self, core: &GroupCore, gid: &str, sig: &str, msg: GroupMsg, route: Route) {
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
            .filter(|a| a.open().is_some_and(|a| devs.contains(&(a.did.clone(), a.dev))))
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

impl Inner {
    // ---- the call layer's events --------------------------------------------------------------

    fn group_call_event(self: &Arc<Self>, ev: GcallEvent) {
        let core = self.group.core.lock().clone();
        let gid_of = |call_id: &str| self.call_gid(call_id).unwrap_or_default();
        let (kind, gid, call_id, participants, detail) = match &ev {
            GcallEvent::HostingStarted { call_id, host_gen, start } => {
                let gid = w::open_start(start).map(|s| s.gid).unwrap_or_default();
                self.group.calls.st.lock().empty_since = None;
                if let (Some(core), Ok(s)) = (core.as_ref(), w::open_start(start)) {
                    let hosts = self.call_addrs(core, &s.gid, &s);
                    self.group_call_started(&s.gid, start.clone(), s.clone());
                    let sig = start.signature.clone();
                    self.calls_originate(core, &s.gid, &sig, GroupMsg::CallStart { start: start.clone(), hosts }, Route::All);
                }
                (GroupCallEventKind::HostingStarted, gid, call_id.clone(), vec![], format!("generation {host_gen}"))
            }
            GcallEvent::HostingEnded { call_id, reason, end, .. } => {
                let gid = gid_of(call_id);
                if *reason == EndReason::Ended
                    && let (Some(core), Some(end)) = (core.as_ref(), end.as_ref())
                {
                    self.group_call_ended(&gid, call_id, end);
                    let sig = end.signature.clone();
                    self.calls_originate(core, &gid, &sig, GroupMsg::CallEnd { end: end.clone() }, Route::All);
                }
                (GroupCallEventKind::HostingEnded, gid, call_id.clone(), vec![], format!("{reason:?}"))
            }
            GcallEvent::Joined { call_id, host, .. } => (GroupCallEventKind::Joined, gid_of(call_id), call_id.clone(), vec![], host.holder_did.clone()),
            GcallEvent::RosterChanged { call_id, roster, .. } => (GroupCallEventKind::RosterChanged, gid_of(call_id), call_id.clone(), roster.values().cloned().collect(), String::new()),
            GcallEvent::Reconnecting { call_id } => (GroupCallEventKind::Reconnecting, gid_of(call_id), call_id.clone(), vec![], String::new()),
            GcallEvent::Superseded { call_id, by } => (GroupCallEventKind::Reconnecting, gid_of(call_id), call_id.clone(), vec![], format!("superseded by {}", by.holder_did)),
            GcallEvent::Ended { call_id, why } => (GroupCallEventKind::Left, gid_of(call_id), call_id.clone(), vec![], format!("{why:?}")),
            GcallEvent::Notice(n) => {
                let call_id = self.gcall_status().map(|s| s.call_id).unwrap_or_default();
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

    fn group_calls_list(&self, core: &GroupCore, gid: &str) -> Vec<GroupCall> {
        let status = self.gcall_status();
        let calls: Vec<CallStart> = self.group.calls.st.lock().calls.get(gid).map(|c| c.values().map(|(_, s)| s.clone()).collect()).unwrap_or_default();
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
