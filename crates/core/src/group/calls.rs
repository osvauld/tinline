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

use super::calldir::{GroupCallDir, device_slot, resolve};
use super::engine::{GroupCore, Out, Sess};
use super::record::SignedHostAddr;
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
/// Seen-blob memory bound (relay dedupe).
const SEEN_MAX: usize = 4096;
/// A signed blob larger than this is not relayed (statuses are ~0.6 KiB, CallStarts ~1.5 KiB).
const MAX_BLOB: usize = 2 * w::MAX_CONTROL;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum GroupCallEventKind {
    /// A call began in the group (`group_calls` lists it; `group_call_join` joins).
    Announced,
    /// A call of the group ended.
    Ended,
    /// `group_call_start` found no call host that agreed (`detail`: why).
    StartFailed,
    /// This device is in the call (also after a move to a backup host).
    Joined,
    /// Who is in the call changed (`participants`: DIDs).
    RosterChanged,
    /// The call host was lost or is handing over; moving to the next one.
    Reconnecting,
    /// This device left the call (`detail`: why: left, ended, removed, host lost, ...).
    Left,
    /// This device hosts the call now.
    HostingStarted,
    /// This device stopped hosting it (`detail`: why).
    HostingEnded,
    /// Hosting limited by battery or heat (one speaker, no new callers), or back to normal.
    HostingLimited,
    HostingUnlimited,
    /// The hosting device moved to metered data.
    MeteredData,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupCallEvent {
    pub kind: GroupCallEventKind,
    pub group_id: String,
    pub call_id: String,
    /// `RosterChanged`: the participants' DIDs.
    pub participants: Vec<String>,
    pub detail: String,
}

/// Registered with `Node::set_group_call_events`. Called on core threads: do not block.
#[uniffi::export(with_foreign)]
pub trait GroupCallEvents: Send + Sync {
    fn on_group_call(&self, event: GroupCallEvent);
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupCall {
    pub group_id: String,
    pub call_id: String,
    pub host_did: String,
    /// Hex group device ID of the host, when it is a current call host.
    pub host_device_id: Option<String>,
    pub starter_did: String,
    pub started_at: u64,
    /// Increments when a backup takes the call over.
    pub host_gen: u32,
    pub hosted_here: bool,
    /// This device is in it.
    pub joined: bool,
}

#[derive(Default)]
pub(crate) struct GroupCalls {
    /// Groups whose call hosting this device paused (`group_call_hosting_enable(false)`).
    pub(crate) paused: Mutex<HashSet<String>>,
    pub(crate) events: Mutex<Option<Arc<dyn GroupCallEvents>>>,
    st: Mutex<CallsSt>,
}

#[derive(Default)]
struct CallsSt {
    /// gid → (holder DID, device slot) → latest admitted status.
    statuses: HashMap<String, BTreeMap<(String, u64), (SignedBlob, CallHostStatus)>>,
    /// gid → call id → highest generation's CallStart.
    calls: HashMap<String, BTreeMap<String, (SignedBlob, CallStart)>>,
    ended: HashSet<String>,
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

/// Where a frame goes from a host.
enum Route {
    All,
    Device(String, u64),
    Did(String),
}

struct Sink(Weak<Inner>);

impl GcallEvents for Sink {
    fn on_gcall_event(&self, ev: GcallEvent) {
        if let Some(i) = self.0.upgrade() {
            i.group_call_event(ev);
        }
    }
}

fn status_class(c: Class) -> DeviceClass {
    match c {
        Class::Desktop => DeviceClass::Desktop,
        Class::Headless => DeviceClass::Headless,
        Class::Phone => DeviceClass::Phone,
    }
}

fn status_thermal(t: Thermal) -> ThermalLevel {
    match t {
        Thermal::None => ThermalLevel::None,
        Thermal::Light => ThermalLevel::Light,
        Thermal::Moderate => ThermalLevel::Moderate,
        Thermal::Severe => ThermalLevel::Severe,
        Thermal::Critical => ThermalLevel::Critical,
    }
}

fn power_of(c: &Cond) -> PowerState {
    match (c.class, c.plugged) {
        (Class::Phone, true) => PowerState::Charging(c.battery.min(100)),
        (_, true) if c.battery >= 100 => PowerState::Mains,
        (_, true) => PowerState::Charging(c.battery.min(100)),
        (_, false) => PowerState::Battery(c.battery.min(100)),
    }
}

fn short(gid: &str) -> &str {
    &gid[..8.min(gid.len())]
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
        let Some(ev) = self.group.calls.events.lock().clone() else { return };
        let e = GroupCallEvent { kind, group_id: gid.into(), call_id: call_id.into(), participants, detail };
        self.handle.spawn_blocking(move || ev.on_group_call(e));
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
        let groups = core.groups.lock();
        let Some(live) = groups.get(gid) else { return };
        let st = &live.state;
        let we_host = st.is_host(&core.did, &core.dev);
        if !we_host && from.is_some() {
            return;
        }
        for s in core.sessions_of(gid) {
            if Some(s.id) == from {
                continue;
            }
            let peer_host = st.is_host(&s.did, &s.dev);
            let ok = if !we_host {
                !s.hosting
            } else if peer_host {
                true
            } else if s.hosting {
                let member = st.is_member(&s.did) && !st.is_removed(&s.did);
                member
                    && match route {
                        Route::All => true,
                        Route::Device(did, slot) => s.did == *did && device_slot(&s.dev) == *slot,
                        Route::Did(did) => s.did == *did,
                    }
            } else {
                false
            };
            if ok {
                let _ = s.tx.send(Out::Msg(msg.clone()));
            }
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

    // ---- receiving --------------------------------------------------------------------------

    /// A `GroupMsg::Call*` from a session (`from`): verified, handled here if it concerns us,
    /// and passed on when we host.
    pub(crate) fn group_call_msg(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, msg: GroupMsg, from: Option<&Arc<Sess>>) {
        let dir = GroupCallDir::new(self);
        let now = now_ms();
        // A member session we host may send only frames it signed itself.
        let member_sender: Option<String> =
            from.filter(|s| s.hosting && !core.groups.lock().get(gid).is_some_and(|l| l.state.is_host(&s.did, &s.dev))).map(|s| s.did.clone());
        let signer_ok = |signer: &str| member_sender.as_deref().is_none_or(|d| d == signer);
        let fid = from.map(|s| s.id);
        let big = |b: &SignedBlob| b.payload.len() + b.signature.len() > MAX_BLOB;
        match msg {
            GroupMsg::CallHostStatus { status } => {
                if big(&status) {
                    return;
                }
                let Ok(s) = rank::admit(&dir, gid, &status, now) else { return };
                if !signer_ok(&s.host_ref.holder_did) || !self.calls_first_sight(&status.signature, s.expires_ms()) {
                    return;
                }
                self.calls_keep_status(gid, status.clone(), s);
                self.calls_send(core, gid, &GroupMsg::CallHostStatus { status }, &Route::All, fid);
            }
            GroupMsg::CallStart { start, hosts } => {
                if big(&start) {
                    return;
                }
                let Ok(s) = w::open_start(&start) else { return };
                let ok = s.gid == gid && dir.host_permit_ok(gid, &s.host_ref) && dir.is_member(gid, &s.starter_did) && signer_ok(&s.host_ref.holder_did);
                if !ok || !self.calls_first_sight(&start.signature, now + CALL_KEEP_MS) {
                    return;
                }
                let hosts: Vec<SignedHostAddr> = hosts.into_iter().take(1 + MAX_BACKUPS).collect();
                core.merge_hosts_pub(gid, &hosts);
                self.group_call_started(gid, start.clone(), s);
                self.calls_send(core, gid, &GroupMsg::CallStart { start, hosts }, &Route::All, fid);
            }
            GroupMsg::CallEnd { end } => {
                if big(&end) {
                    return;
                }
                let Ok(e) = w::open_end(&end) else { return };
                let known = self.group.calls.st.lock().calls.get(gid).and_then(|c| c.get(&e.call_id)).is_some_and(|(_, s)| s.host_ref.holder_did == e.host_did);
                if e.gid != gid || e.reason != EndReason::Ended || !known || !signer_ok(&e.host_did) || !self.calls_first_sight(&end.signature, now + CALL_KEEP_MS) {
                    return;
                }
                self.group_call_ended(gid, &e.call_id, &end);
                self.calls_send(core, gid, &GroupMsg::CallEnd { end }, &Route::All, fid);
            }
            GroupMsg::CallHostRequest { request } => {
                if big(&request) {
                    return;
                }
                let Ok(r) = w::open_host_request(&request) else { return };
                let fresh = r.issued_ms + REQUEST_MAX_AGE_MS > now && r.issued_ms <= now + gs::MAX_CLOCK_SKEW_MS;
                let ok = r.gid == gid && fresh && dir.is_member(gid, &r.starter_did) && dir.host_permit_ok(gid, &r.host) && signer_ok(&r.starter_did);
                if !ok || !self.calls_first_sight(&request.signature, now + REQUEST_MAX_AGE_MS) {
                    return;
                }
                // For this device (also when paused: it then declines).
                let for_me = core.groups.lock().get(gid).and_then(|l| resolve(&l.state, &r.host)).is_some_and(|h| h.did == core.did && h.dev == core.dev);
                if for_me {
                    self.group_call_requested(core, gid, r);
                    return;
                }
                let route = Route::Device(r.host.holder_did.clone(), r.host.device_slot);
                self.calls_send(core, gid, &GroupMsg::CallHostRequest { request }, &route, fid);
            }
            GroupMsg::CallHostRefusal { refusal } => {
                if big(&refusal) {
                    return;
                }
                let Ok(f) = w::open_host_refusal(&refusal) else { return };
                let ok = f.gid == gid && dir.is_member(gid, &f.starter_did) && signer_ok(&f.host.holder_did);
                if !ok || !self.calls_first_sight(&refusal.signature, now + REQUEST_MAX_AGE_MS) {
                    return;
                }
                if f.starter_did == core.did {
                    self.group_call_refused(core, &f);
                }
                let route = Route::Did(f.starter_did.clone());
                self.calls_send(core, gid, &GroupMsg::CallHostRefusal { refusal }, &route, fid);
            }
            _ => {}
        }
    }

    fn calls_keep_status(&self, gid: &str, blob: SignedBlob, s: CallHostStatus) {
        let mut st = self.group.calls.st.lock();
        let m = st.statuses.entry(gid.to_string()).or_default();
        let key = (s.host_ref.holder_did.clone(), s.host_ref.device_slot);
        if m.get(&key).is_none_or(|(_, have)| have.issued_ms <= s.issued_ms) {
            m.insert(key, (blob, s));
        }
    }

    /// A CallStart (relayed, or one of ours): remembered, fed to the call board, joined by
    /// the starter whose pick it answers.
    fn group_call_started(self: &Arc<Self>, gid: &str, blob: SignedBlob, s: CallStart) {
        let new_call = {
            let mut st = self.group.calls.st.lock();
            if st.ended.contains(&s.call_id) {
                return;
            }
            let calls = st.calls.entry(gid.to_string()).or_default();
            if calls.get(&s.call_id).is_some_and(|(_, have)| have.host_gen >= s.host_gen) {
                return;
            }
            let new = !calls.contains_key(&s.call_id);
            calls.insert(s.call_id.clone(), (blob.clone(), s.clone()));
            new
        };
        self.group_calls_configure();
        if let Err(e) = self.gcall_observe_start(&blob) {
            tracing::debug!("group call {}: {e}", short(gid));
        }
        let join = {
            let mut st = self.group.calls.st.lock();
            let ours = st.pending.as_mut().is_some_and(|p| p.call_id == s.call_id && p.picker.on_started(&s.host_ref));
            if ours {
                st.pending = None;
            }
            ours
        };
        if new_call {
            self.log(format!("group {}: call {} by {} hosted by {}", short(gid), &s.call_id, s.starter_did, s.host_ref.holder_did));
            self.calls_emit(GroupCallEventKind::Announced, gid, &s.call_id, vec![], s.starter_did.clone());
        }
        let hosted_here = self.gcall_status().is_some_and(|st| st.call_id == s.call_id && st.hosting.is_some());
        if join
            && !hosted_here
            && let Err(e) = self.gcall_join(&blob, None)
        {
            self.calls_emit(GroupCallEventKind::StartFailed, gid, &s.call_id, vec![], e.to_string());
        }
    }

    fn group_call_ended(self: &Arc<Self>, gid: &str, call_id: &str, end: &SignedBlob) {
        let _ = self.gcall_observe_end(end);
        let had = {
            let mut st = self.group.calls.st.lock();
            st.ended.insert(call_id.to_string());
            st.calls.get_mut(gid).and_then(|c| c.remove(call_id)).is_some()
        };
        if had {
            self.calls_emit(GroupCallEventKind::Ended, gid, call_id, vec![], String::new());
        }
    }

    // ---- the asked device ---------------------------------------------------------------------

    fn group_call_requested(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, r: HostRequest) {
        self.group_calls_configure();
        let cond = self.gcall.cond();
        let paused = self.group.calls.paused.lock().contains(gid);
        let why: Option<String> = if paused {
            Some("call hosting paused".into())
        } else if self.gcall.busy() {
            Some("busy".into())
        } else if cond.class == Class::Phone {
            // §2.4 / Q11: a phone hosts only while its own user is in the call.
            Some("a phone hosts only calls its user starts".into())
        } else if phone_policy(&cond) != PhoneAction::Advertise {
            Some("conditions".into())
        } else {
            let dir = GroupCallDir::new(self);
            let backups: Vec<HostRef> = r.backups.iter().filter(|b| **b != r.host && dir.host_permit_ok(gid, b)).take(MAX_BACKUPS).cloned().collect();
            let cfg = HostConfig {
                gid: gid.to_string(),
                call_id: r.call_id.clone(),
                host_gen: 1,
                host_ref: r.host.clone(),
                starter_did: r.starter_did.clone(),
                backups,
                meta_frontier: self.calls_frontier(core, gid),
                cond,
                own_user_in: false,
            };
            self.gcall_host(cfg).err().map(|e| e.to_string())
        };
        let Some(reason) = why else {
            self.log(format!("group {}: hosting call {} for {}", short(gid), r.call_id, r.starter_did));
            return;
        };
        self.log(format!("group {}: declined to host call {}: {reason}", short(gid), r.call_id));
        let f = HostRefusal {
            v: GCALL_VERSION,
            gid: gid.to_string(),
            call_id: r.call_id.clone(),
            starter_did: r.starter_did.clone(),
            host: r.host.clone(),
            reason: reason.chars().take(100).collect(),
        };
        let blob = w::sign_host_refusal(&core.account, f.clone());
        if f.starter_did == core.did {
            self.group_call_refused(core, &f);
        }
        let sig = blob.signature.clone();
        self.calls_originate(core, gid, &sig, GroupMsg::CallHostRefusal { refusal: blob }, Route::Did(f.starter_did));
    }

    fn calls_frontier(&self, core: &GroupCore, gid: &str) -> String {
        let vv = core.groups.lock().get(gid).map(|l| l.doc.vv().encode()).unwrap_or_default();
        blake3::hash(&vv).to_hex()[..32].to_string()
    }

    // ---- the starter --------------------------------------------------------------------------

    pub(crate) fn group_call_start(self: &Arc<Self>, gid: &str) -> Result<String, Error> {
        let core = self.group_core()?;
        self.group_member_check(&core, gid)?;
        if core.ep_id.lock().is_none() {
            return Err(Error::NotStarted);
        }
        self.group_calls_configure();
        if self.gcall.busy() {
            return Err(Error::Busy);
        }
        self.not_in_call()?;
        let dir = GroupCallDir::new(self);
        let now = now_ms();
        let members = core.groups.lock().get(gid).map_or(0, |l| l.state.members.len());
        let expected = members.clamp(2, MAX_PARTICIPANTS) as u8;
        let call_id = w::new_call_id();
        let blobs: Vec<SignedBlob> = {
            let st = self.group.calls.st.lock();
            if st.pending.is_some() {
                return Err(Error::Busy);
            }
            st.statuses.get(gid).map(|m| m.values().map(|(b, _)| b.clone()).collect()).unwrap_or_default()
        };
        let me = dir.my_host_ref(gid);
        let ranking = rank::rank(&dir, &blobs, &RankCtx { gid, call_id: &call_id, expected: expected as usize, starter: me.as_ref(), now_ms: now });
        self.log(format!("group {}: starting call {call_id}, {} candidate(s)", short(gid), ranking.len()));
        let mut picker = Picker::new(ranking);
        let step = picker.start(now);
        self.group.calls.st.lock().pending = Some(Pending { gid: gid.to_string(), call_id: call_id.clone(), expected, picker });
        self.group_call_step(&core, gid, &call_id, step)?;
        Ok(call_id)
    }

    /// Acts on the picker's next step: ask the candidate (ourselves: host at once), or give up.
    fn group_call_step(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, call_id: &str, mut step: PickerStep) -> Result<(), Error> {
        loop {
            let pick = match step {
                PickerStep::Exhausted => {
                    let mut st = self.group.calls.st.lock();
                    if st.pending.as_ref().is_some_and(|p| p.call_id == call_id) {
                        st.pending = None;
                    }
                    drop(st);
                    self.calls_emit(GroupCallEventKind::StartFailed, gid, call_id, vec![], "no call host available".into());
                    return Err(Error::Rejected("no call host available".into()));
                }
                PickerStep::Ask(p) => p,
            };
            let expected = self.group.calls.st.lock().pending.as_ref().filter(|p| p.call_id == call_id).map(|p| p.expected);
            let Some(expected) = expected else { return Ok(()) };
            let me = GroupCallDir::new(self).my_host_ref(gid);
            if me.as_ref() == Some(&pick.host) {
                let cfg = HostConfig {
                    gid: gid.to_string(),
                    call_id: call_id.to_string(),
                    host_gen: 1,
                    host_ref: pick.host.clone(),
                    starter_did: core.did.clone(),
                    backups: pick.backups.clone(),
                    meta_frontier: self.calls_frontier(core, gid),
                    cond: self.gcall.cond(),
                    own_user_in: true,
                };
                match self.gcall_host(cfg) {
                    Ok(_) => {
                        let mut st = self.group.calls.st.lock();
                        if st.pending.as_ref().is_some_and(|p| p.call_id == call_id) {
                            st.pending = None;
                        }
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::debug!("group call: hosting our own call failed: {e}");
                        let next = {
                            let mut st = self.group.calls.st.lock();
                            st.pending.as_mut().filter(|p| p.call_id == call_id).and_then(|p| p.picker.on_refused(&pick.host, now_ms()))
                        };
                        match next {
                            Some(n) => step = n,
                            None => return Ok(()),
                        }
                        continue;
                    }
                }
            }
            let req = HostRequest {
                v: GCALL_VERSION,
                gid: gid.to_string(),
                call_id: call_id.to_string(),
                starter_did: core.did.clone(),
                host: pick.host.clone(),
                backups: pick.backups.clone(),
                expected,
                issued_ms: now_ms(),
            };
            let blob = w::sign_host_request(&core.account, req);
            let sig = blob.signature.clone();
            let route = Route::Device(pick.host.holder_did.clone(), pick.host.device_slot);
            self.calls_originate(core, gid, &sig, GroupMsg::CallHostRequest { request: blob }, route);
            return Ok(());
        }
    }

    fn group_call_refused(self: &Arc<Self>, core: &Arc<GroupCore>, f: &HostRefusal) {
        let next = {
            let mut st = self.group.calls.st.lock();
            st.pending.as_mut().filter(|p| p.call_id == f.call_id && p.gid == f.gid).and_then(|p| p.picker.on_refused(&f.host, now_ms()))
        };
        if let Some(step) = next {
            self.log(format!("group {}: {} declined call {}: {}", short(&f.gid), f.host.holder_did, f.call_id, f.reason));
            let _ = self.group_call_step(core, &f.gid, &f.call_id, step);
        }
    }

    // ---- periodic work (the group dial loop's tick, every ~2 s) --------------------------------

    pub(crate) fn group_calls_tick(self: &Arc<Self>, core: &Arc<GroupCore>) {
        let now = now_ms();
        // Our status in every group whose calls we host.
        let mine: Vec<(String, HostRef)> = {
            let dir = GroupCallDir::new(self);
            let gids: Vec<String> = core.groups.lock().keys().cloned().collect();
            gids.into_iter().filter_map(|g| dir.my_host_ref(&g).map(|r| (g, r))).collect()
        };
        if !mine.is_empty() {
            self.group_calls_configure();
        }
        let cond = self.gcall.cond();
        let busy = self.gcall.busy() || phone_policy(&cond) != PhoneAction::Advertise;
        for (gid, host_ref) in mine {
            let status = CallHostStatus {
                v: GCALL_VERSION,
                gid: gid.clone(),
                host_ref,
                class: status_class(cond.class),
                power: power_of(&cond),
                network: if cond.metered { NetworkKind::Metered } else { NetworkKind::Unmetered },
                thermal: status_thermal(cond.thermal),
                busy,
                load: 0,
                max_participants: cond.cap().clamp(2, MAX_PARTICIPANTS) as u8,
                issued_ms: 0,
                ttl_ms: STATUS_TTL_MS,
            };
            let due = self.group.calls.st.lock().published.get(&gid).is_none_or(|(s, _, at)| *s != status || now >= at + STATUS_REFRESH_MS);
            if !due {
                continue;
            }
            let blob = gs::sign_status(&core.account, CallHostStatus { issued_ms: now, ..status.clone() });
            self.group.calls.st.lock().published.insert(gid.clone(), (status.clone(), blob.clone(), now));
            self.calls_keep_status(&gid, blob.clone(), CallHostStatus { issued_ms: now, ..status });
            let sig = blob.signature.clone();
            self.calls_originate(core, &gid, &sig, GroupMsg::CallHostStatus { status: blob }, Route::All);
        }
        // The starter's pick: the asked candidate did not answer in time.
        let step = {
            let mut st = self.group.calls.st.lock();
            st.pending.as_mut().and_then(|p| p.picker.tick(now).map(|s| (p.gid.clone(), p.call_id.clone(), s)))
        };
        if let Some((gid, call_id, step)) = step {
            let _ = self.group_call_step(core, &gid, &call_id, step);
        }
        // A listen-only hosted call nobody is in ends.
        let empty = self.gcall_status().and_then(|s| {
            let own_in = s.client.as_ref().is_some_and(|c| !matches!(c.state, ClientState::Ended(_)));
            s.hosting.map(|h| h.roster.is_empty() && !own_in)
        });
        let end = {
            let mut st = self.group.calls.st.lock();
            match empty {
                Some(true) => {
                    let since = *st.empty_since.get_or_insert(now);
                    now >= since + IDLE_END_MS
                }
                _ => {
                    st.empty_since = None;
                    false
                }
            }
        };
        if end {
            self.log("group call: nobody left in the call hosted here, ending it");
            self.gcall_end();
        }
        // Forget what is stale.
        let mut st = self.group.calls.st.lock();
        for m in st.statuses.values_mut() {
            m.retain(|_, (_, s)| s.is_fresh(now));
        }
        for c in st.calls.values_mut() {
            c.retain(|_, (_, s)| s.started_ms + CALL_KEEP_MS > now);
        }
        st.seen.retain(|_, t| *t > now);
    }

    /// A new session: a member we host gets the fresh statuses and live calls; another host
    /// gets them too when we host; our host gets our own status (and the call we host).
    pub(crate) fn group_calls_on_session(&self, core: &GroupCore, sess: &Sess) {
        let (we_host, peer_host, member) = {
            let groups = core.groups.lock();
            let Some(l) = groups.get(&sess.gid) else { return };
            let st = &l.state;
            (st.is_host(&core.did, &core.dev), st.is_host(&sess.did, &sess.dev), st.is_member(&sess.did) && !st.is_removed(&sess.did))
        };
        let all = (sess.hosting && !peer_host && member && we_host) || (peer_host && we_host);
        let now = now_ms();
        let (statuses, calls): (Vec<SignedBlob>, Vec<(SignedBlob, CallStart)>) = {
            let st = self.group.calls.st.lock();
            let statuses = if all {
                st.statuses.get(&sess.gid).map(|m| m.values().filter(|(_, s)| s.is_fresh(now)).map(|(b, _)| b.clone()).collect()).unwrap_or_default()
            } else if peer_host {
                st.published.get(&sess.gid).filter(|(_, _, at)| now < at + STATUS_TTL_MS).map(|(_, b, _)| vec![b.clone()]).unwrap_or_default()
            } else {
                vec![]
            };
            let calls = if all || peer_host {
                st.calls.get(&sess.gid).map(|c| c.values().filter(|(_, s)| all || s.host_ref.holder_did == core.did).cloned().collect()).unwrap_or_default()
            } else {
                vec![]
            };
            (statuses, calls)
        };
        for status in statuses {
            let _ = sess.tx.send(Out::Msg(GroupMsg::CallHostStatus { status }));
        }
        for (start, s) in calls {
            let hosts = self.call_addrs(core, &sess.gid, &s);
            let _ = sess.tx.send(Out::Msg(GroupMsg::CallStart { start, hosts }));
        }
    }

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

impl Node {
    fn calls_ready(&self) -> Result<Arc<GroupCore>, Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            inner.group_ensure(&core).await?;
            inner.group_calls_configure();
            Ok(core)
        })?
    }
}

/// Group calls (GA-I1). Plain group membership is the authority (no per-contact call tokens,
/// §3.3); joining is always the user's choice.
#[uniffi::export]
impl Node {
    pub fn set_group_call_events(&self, events: Arc<dyn GroupCallEvents>) {
        *self.inner.group.calls.events.lock() = Some(events);
    }

    /// Starts a call in the group: picks the best call host from the fresh `CallHostStatus`es
    /// (§2.3) and asks it through the chat hosts; this device joins once it announces the call.
    /// Returns the call id at once; progress comes as events (`Joined`, or `StartFailed`).
    /// `Rejected("no call host available")` when no call host advertises.
    pub fn group_call_start(&self, group_id: String) -> Result<String, Error> {
        self.calls_ready()?;
        self.inner.group_call_start(&group_id)
    }

    /// Joins a call announced in the group (`group_calls`).
    pub fn group_call_join(&self, group_id: String, call_id: String) -> Result<(), Error> {
        let core = self.calls_ready()?;
        self.inner.group_member_check(&core, &group_id)?;
        let blob = self.inner.group.calls.st.lock().calls.get(&group_id).and_then(|c| c.get(&call_id)).map(|(b, _)| b.clone());
        let blob = blob.ok_or(Error::NotFound)?;
        self.inner.gcall_join(&blob, None)
    }

    /// Leaves the call this device is in. A call hosted here that nobody is in any more ends.
    pub fn group_call_leave(&self) {
        self.inner.group.calls.st.lock().pending = None;
        self.inner.gcall_leave();
        let empty = self.inner.gcall_status().and_then(|s| s.hosting).is_some_and(|h| h.roster.is_empty());
        if empty {
            self.inner.gcall_end();
        }
    }

    /// Mutes this device's microphone in the group call.
    pub fn group_call_set_muted(&self, muted: bool) {
        self.inner.gcall_set_mic_muted(muted)
    }

    /// The calls going on in the group, as this device knows them.
    pub fn group_calls(&self, group_id: String) -> Result<Vec<GroupCall>, Error> {
        let core = self.inner.group_core()?;
        Ok(self.inner.group_calls_list(&core, &group_id))
    }

    /// Test hook: the DIDs whose fresh `CallHostStatus` this device holds for the group.
    pub fn group_call_hosts_advertising_for_test(&self, group_id: String) -> Vec<String> {
        let now = now_ms();
        let st = self.inner.group.calls.st.lock();
        st.statuses.get(&group_id).map(|m| m.values().filter(|(_, s)| s.is_fresh(now)).map(|(_, s)| s.host_ref.holder_did.clone()).collect()).unwrap_or_default()
    }

    /// Pauses (or resumes) this device's call hosting for the group without touching its
    /// signed `CallHostAccept`: while paused it advertises nothing, declines requests and does
    /// not take calls over. Not persisted (a restart resumes).
    pub fn group_call_hosting_enable(&self, group_id: String, enable: bool) {
        let mut p = self.inner.group.calls.paused.lock();
        if enable {
            p.remove(&group_id);
        } else {
            p.insert(group_id);
        }
    }
}
