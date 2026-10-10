//! The group runtime: the group endpoint (U2, `tinline/group/1`), host and member sessions,
//! live relay, catch-up and removal enforcement. Everything hangs off `Inner::group` (`GroupRt`).
//!
//! Members only dial hosts; hosts accept members and dial other hosts like members do; nobody
//! else is ever dialled, so a member learns only hosts' group endpoints (U7). Every update is
//! verified on arrival (`GroupDoc::apply_remote`) by every device, hosts included (U10).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use iroh::endpoint::{Connection, QuicTransportConfig, presets};
use iroh::{Endpoint, SecretKey};
use loro::VersionVector;
use parking_lot::Mutex;
use tokio::sync::{Notify, mpsc};

use super::api::GroupEvents;
use super::doc::{Entry, GroupDoc, Reject};
use super::record::*;
use super::state::{GroupState, derive};
use super::store::{DirEntry, GroupMeta, GroupStore};
use super::wire::*;
use crate::Error;
use crate::chat::doc::writer_id;
use crate::node::{Inner, addr_for, now, now_ms, relay_of};

const HELLO_TIMEOUT: Duration = Duration::from_secs(15);
const DIAL_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a host waits for a removed member's pre-removal upload (U11).
const REMOVED_UPLOAD_WAIT: Duration = Duration::from_secs(10);
const TICK: Duration = Duration::from_secs(2);
const BACKOFF: [u64; 6] = [1, 2, 5, 15, 30, 60];
pub const INVITE_TTL: u64 = 7 * 24 * 3600;

/// Test knobs and the open runtime.
#[derive(Default)]
pub(crate) struct GroupRt {
    pub(crate) core: Mutex<Option<Arc<GroupCore>>>,
    pub(crate) events: Mutex<Option<Arc<dyn GroupEvents>>>,
    /// Test knob: dial only hosts of this DID.
    pub(crate) dial_only: Mutex<Option<String>>,
    /// Test knob: act like a modified app that ignores its own removal.
    pub(crate) ignore_removal: AtomicBool,
}

pub(crate) struct Live {
    pub doc: GroupDoc,
    pub entries: Vec<Entry>,
    pub state: GroupState,
    pub meta: GroupMeta,
}

impl Live {
    fn rederive(&mut self, gid: &str) {
        self.state = derive(gid, &self.entries);
    }
}

pub(crate) struct GroupCore {
    epoch: u64,
    pub store: GroupStore,
    pub account: identity::Identity,
    pub did: String,
    pub dev: [u8; 32],
    pub wid: u64,
    pub groups: Mutex<HashMap<String, Live>>,
    pub ep: tokio::sync::Mutex<Option<Endpoint>>,
    pub ep_id: Mutex<Option<Endpoint>>,
    sessions: Mutex<HashMap<u64, Arc<Sess>>>,
    dialing: Mutex<HashSet<(String, String)>>,
    backoff: Mutex<HashMap<(String, String), (Instant, usize)>>,
    /// The last refusal a host gave us per group (for `group_join`).
    pub refusals: Mutex<HashMap<String, String>>,
    my_addr: Mutex<HashMap<String, SignedHostAddr>>,
    next: AtomicU64,
    kick: Notify,
    closed: AtomicBool,
    loops: AtomicBool,
}

pub(crate) struct Sess {
    id: u64,
    gid: String,
    did: String,
    dev: [u8; 32],
    /// We accepted it: we host this session's member.
    hosting: bool,
    /// Host side: the member was joining (not yet a member) at Hello.
    joining: bool,
    tx: mpsc::UnboundedSender<Out>,
    peer_vv: Mutex<VersionVector>,
}

enum Out {
    Msg(GroupMsg),
    Close,
}

pub(crate) fn host_key(did: &str, dev: &[u8; 32]) -> String {
    format!("{did}/{}", hex::encode(dev))
}

fn decode_vv(b: &[u8]) -> Result<VersionVector, Error> {
    if b.is_empty() {
        return Ok(VersionVector::default());
    }
    VersionVector::decode(b).map_err(|_| Error::Protocol("bad version vector".into()))
}

/// What changed in a group, for events.
pub(crate) struct Changed {
    pub gid: String,
    pub new_items: Vec<super::state::Item>,
}

impl GroupCore {
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn sessions_of(&self, gid: &str) -> Vec<Arc<Sess>> {
        self.sessions.lock().values().filter(|s| s.gid == gid).cloned().collect()
    }

    /// Test knob support: drops sessions with hosts other than `only`'s.
    pub fn close_sessions_except(&self, only: Option<&str>) {
        let Some(only) = only else { return };
        let mut ss = self.sessions.lock();
        ss.retain(|_, s| {
            let keep = s.hosting || s.did == only;
            if !keep {
                let _ = s.tx.send(Out::Close);
            }
            keep
        });
    }

    pub fn has_session(&self, gid: &str, did: &str, dev: &[u8; 32]) -> bool {
        self.sessions.lock().values().any(|s| s.gid == gid && s.did == did && &s.dev == dev)
    }

    /// The group endpoint keys this device holds for `gid` (hosts' addresses; on a host, also
    /// its members' directory; and every live session's remote) — for the U7 tests.
    pub fn known_endpoints(&self, gid: &str) -> Vec<String> {
        let mut out: HashSet<String> = HashSet::new();
        for a in self.store.hosts(gid).unwrap_or_default() {
            if let Some(a) = a.open()
                && !(a.did == self.did && a.dev == self.dev)
            {
                out.insert(iroh::PublicKey::from_bytes(&a.endpoint).map(|k| k.to_string()).unwrap_or_default());
            }
        }
        for (_, d) in self.store.dir(gid).unwrap_or_default() {
            out.insert(d.endpoint);
        }
        let mut v: Vec<String> = out.into_iter().collect();
        v.sort();
        v
    }

    /// Writes one record of ours into `gid`; returns the entry and what changed.
    pub fn write(&self, gid: &str, body: Body) -> Result<(Entry, Changed), Error> {
        let mut groups = self.groups.lock();
        let live = groups.get_mut(gid).ok_or(Error::NotFound)?;
        let rec = Record { v: 1, group: gid.to_string(), author: self.did.clone(), dev: self.dev, at: now_ms() as i64, seen: live.doc.seen(), body };
        let (entry, update) = live.doc.write(&self.account, &rec).map_err(|e| Error::Protocol(e.to_string()))?;
        let before: HashSet<String> = live.state.items.iter().map(|i| i.id.clone()).collect();
        live.entries.push(entry.clone());
        live.rederive(gid);
        let doc = &live.doc;
        self.store.append(gid, &mut live.meta, &update, || doc.snapshot())?;
        let new_items = live.state.items.iter().filter(|i| !before.contains(&i.id)).cloned().collect();
        Ok((entry, Changed { gid: gid.to_string(), new_items }))
    }

    /// Validates and applies a remote update; `admit` is the session's policy, asked with the
    /// group's current state and entries and the batch's new entries before anything is imported.
    fn apply(&self, gid: &str, update: &[u8], admit: impl FnOnce(&GroupState, &[Entry], &[Entry]) -> Result<(), String>) -> Result<Option<Changed>, Reject> {
        let mut groups = self.groups.lock();
        let Some(live) = groups.get_mut(gid) else { return Err(Reject::Policy("unknown group".into())) };
        let fresh = {
            let Live { doc, entries, state, .. } = &mut *live;
            doc.apply_remote(update, |fresh| admit(state, entries, fresh))?
        };
        if fresh.is_empty() {
            return Ok(None);
        }
        let before: HashSet<String> = live.state.items.iter().map(|i| i.id.clone()).collect();
        live.entries.extend(fresh);
        live.rederive(gid);
        let doc = &live.doc;
        if let Err(e) = self.store.append(gid, &mut live.meta, update, || doc.snapshot()) {
            tracing::warn!("group store: {e}");
        }
        let new_items = live.state.items.iter().filter(|i| !before.contains(&i.id)).cloned().collect();
        Ok(Some(Changed { gid: gid.to_string(), new_items }))
    }

    /// Host addresses we pass on for `gid`: active hosts only (never a member's endpoint), ours
    /// included when we host.
    fn host_addrs(&self, gid: &str) -> Vec<SignedHostAddr> {
        let groups = self.groups.lock();
        let Some(live) = groups.get(gid) else { return Vec::new() };
        let mut out: Vec<SignedHostAddr> = self
            .store
            .hosts(gid)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.open().is_some_and(|a| live.state.is_host(&a.did, &a.dev) && !(a.did == self.did && a.dev == self.dev)))
            .collect();
        if live.state.is_host(&self.did, &self.dev)
            && let Some(mine) = self.my_addr.lock().get(gid).cloned()
        {
            out.push(mine);
        }
        out
    }

    /// Stores host addresses that are newer and belong to active hosts (or, before we hold any
    /// state, i.e. while joining, any validly signed one for this group). True if any changed.
    fn merge_hosts(&self, gid: &str, addrs: &[SignedHostAddr]) -> bool {
        let groups = self.groups.lock();
        let Some(live) = groups.get(gid) else { return false };
        let stored: HashMap<String, HostAddr> =
            self.store.hosts(gid).unwrap_or_default().into_iter().filter_map(|s| s.open()).map(|a| (host_key(&a.did, &a.dev), a)).collect();
        let mut changed = false;
        for s in addrs.iter().take(64) {
            let Some(a) = s.open() else { continue };
            if a.group != gid || (a.did == self.did && a.dev == self.dev) {
                continue;
            }
            if live.state.owner.is_some() && !live.state.is_host(&a.did, &a.dev) {
                continue;
            }
            if stored.get(&host_key(&a.did, &a.dev)).is_some_and(|old| old.at >= a.at) {
                continue;
            }
            if self.store.put_host(gid, &a.did, &a.dev, s).is_ok() {
                changed = true;
            }
        }
        changed
    }

    /// The signed records that prove `did` was removed: genesis, the owner's admin grants of the
    /// flaggers, and the counted flags.
    fn removal_proof(&self, live: &Live, did: &str) -> Vec<Vec<u8>> {
        let Some(flags) = live.state.removed.get(did) else { return Vec::new() };
        let flag_ids: HashSet<&str> = flags.iter().map(|f| f.id.as_str()).collect();
        let flaggers: HashSet<&str> = flags.iter().map(|f| f.by.as_str()).collect();
        let owner = live.state.owner.clone().unwrap_or_default();
        let mut out = Vec::new();
        for e in &live.entries {
            let keep = match &e.rec.body {
                Body::Genesis { .. } => e.rec.author == owner,
                Body::Admin { member, .. } => e.rec.author == owner && flaggers.contains(member.as_str()),
                Body::Flag { .. } => flag_ids.contains(e.id.as_str()),
                _ => false,
            };
            if keep && let Some(b) = self.raw_record(live, &e.id) {
                out.push(b);
            }
        }
        out
    }

    /// What `did` (holding `theirs`) lacks of the causal past of the flags that removed it.
    fn removal_upto(&self, live: &Live, did: &str, theirs: &VersionVector) -> Option<Vec<u8>> {
        let flags = live.state.removed.get(did)?;
        let ids: Vec<(u64, i32)> = flags
            .iter()
            .filter_map(|f| live.entries.iter().find(|e| e.id == f.id))
            .map(|e| (e.peer, e.counter))
            .collect();
        live.doc.export_until(&ids, theirs)
    }

    fn raw_record(&self, live: &Live, id: &str) -> Option<Vec<u8>> {
        live.doc.raw_value(id)
    }
}

/// Checks a removal proof against our own DID: a genesis for `gid`, and at least 3 flags on us
/// by the owner or by DIDs the owner made admin (PROVISIONAL: the "admin as of the flag's
/// frontier" refinement is applied when the doc itself carries the flags).
pub(crate) fn proof_removes(gid: &str, me: &str, records: &[Vec<u8>]) -> Option<Vec<(String, i64)>> {
    let recs: Vec<Record> = records.iter().filter_map(|b| SignedRecord::open(b, gid).ok()).collect();
    let owner = recs.iter().find_map(|r| match &r.body {
        Body::Genesis { nonce, .. } if group_id(&r.author, nonce) == gid => Some(r.author.clone()),
        _ => None,
    })?;
    let admins: HashSet<String> = recs
        .iter()
        .filter_map(|r| match &r.body {
            Body::Admin { member, admin: true } if r.author == owner => Some(member.clone()),
            _ => None,
        })
        .collect();
    let flags: Vec<(String, i64)> = recs
        .iter()
        .filter_map(|r| match &r.body {
            Body::Flag { member } if member == me && (r.author == owner || admins.contains(&r.author)) => Some((r.author.clone(), r.at)),
            _ => None,
        })
        .collect();
    (flags.len() >= super::state::REMOVAL_FLAGS).then_some(flags)
}

impl Inner {
    // ---- lifecycle -------------------------------------------------------------------------

    /// The group runtime of the unlocked account, opened on first use.
    pub(crate) fn group_core(self: &Arc<Self>) -> Result<Arc<GroupCore>, Error> {
        let epoch = self.shared.lock().epoch;
        // Held while opening, so two callers never open the store twice.
        let mut slot = self.group.core.lock();
        if let Some(c) = slot.clone()
            && c.epoch == epoch
            && !c.closed()
        {
            return Ok(c);
        }
        let me = self.me()?;
        let (dir, dev) = {
            let s = self.shared.lock();
            let dir = s.acct.as_ref().ok_or(Error::Locked)?.store.dir().to_path_buf();
            let dev = match s.disk.as_ref() {
                Some(crate::store::Disk::V2(p)) => p.device_id_bytes(),
                _ => None,
            };
            (dir, dev)
        };
        let dev = match dev {
            Some(d) => d,
            None => {
                // The chat store makes the device ID on open; make sure it ran.
                self.chat_open();
                match self.shared.lock().disk.as_ref() {
                    Some(crate::store::Disk::V2(p)) => p.device_id_bytes().ok_or(Error::NoIdentity)?,
                    _ => return Err(Error::NoIdentity),
                }
            }
        };
        let store = GroupStore::open(dir.join("group.redb"), &me.profile.mnemonic)?;
        let did = me.id.did().to_string();
        let wid = writer_id(&did, &dev);
        let mut groups = HashMap::new();
        for (gid, meta) in store.groups()? {
            let (snap, ups) = store.doc_rows(&gid)?;
            let doc = match GroupDoc::load(&gid, wid, snap.as_deref(), &ups) {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("group {gid}: {e}");
                    continue;
                }
            };
            let entries = doc.entries().unwrap_or_default();
            let state = derive(&gid, &entries);
            groups.insert(gid, Live { doc, entries, state, meta });
        }
        let core = Arc::new(GroupCore {
            epoch,
            store,
            account: me.id.clone(),
            did,
            dev,
            wid,
            groups: Mutex::new(groups),
            ep: tokio::sync::Mutex::new(None),
            ep_id: Mutex::new(None),
            sessions: Default::default(),
            dialing: Default::default(),
            backoff: Default::default(),
            refusals: Default::default(),
            my_addr: Default::default(),
            next: AtomicU64::new(1),
            kick: Notify::new(),
            closed: AtomicBool::new(false),
            loops: AtomicBool::new(false),
        });
        if let Some(old) = slot.take() {
            old.closed.store(true, Ordering::SeqCst);
        }
        *slot = Some(core.clone());
        Ok(core)
    }

    /// Stops everything of the group runtime (lock, stop, account switch).
    pub(crate) async fn group_close(&self) {
        let core = self.group.core.lock().take();
        if let Some(core) = core {
            core.closed.store(true, Ordering::SeqCst);
            core.kick.notify_waiters();
            for s in core.sessions.lock().values() {
                let _ = s.tx.send(Out::Close);
            }
            let ep = core.ep.lock().await.take();
            *core.ep_id.lock() = None;
            if let Some(ep) = ep {
                let _ = tokio::time::timeout(Duration::from_secs(3), ep.close()).await;
            }
        }
    }

    /// Called once the private endpoint is up: brings the group endpoint up if we have groups.
    pub(crate) fn group_started(self: &Arc<Self>) {
        let this = self.clone();
        self.handle.spawn(async move {
            let Ok(core) = this.group_core() else { return };
            if core.groups.lock().is_empty() {
                return;
            }
            if let Err(e) = this.group_ensure(&core).await {
                tracing::warn!("group endpoint: {e}");
            }
        });
    }

    /// Binds the group endpoint (once) and starts the accept and dial loops.
    pub(crate) async fn group_ensure(self: &Arc<Self>, core: &Arc<GroupCore>) -> Result<Endpoint, Error> {
        if self.shared.lock().endpoint.is_none() {
            return Err(Error::NotStarted);
        }
        let mut slot = core.ep.lock().await;
        if let Some(ep) = slot.as_ref() {
            return Ok(ep.clone());
        }
        if core.closed() {
            return Err(Error::Locked);
        }
        let transport = QuicTransportConfig::builder()
            .keep_alive_interval(Duration::from_secs(5))
            .max_idle_timeout(Some(Duration::from_secs(20).try_into().expect("small timeout")))
            .build();
        let mut builder = Endpoint::builder(presets::N0)
            .secret_key(SecretKey::from_bytes(&core.store.endpoint_secret()?))
            .alpns(vec![GROUP_ALPN.to_vec()])
            .transport_config(transport);
        if std::env::var_os("P2P_RELAY_ONLY").is_some() {
            builder = builder.clear_ip_transports();
        }
        let ep = builder.bind().await.map_err(Error::net)?;
        *slot = Some(ep.clone());
        *core.ep_id.lock() = Some(ep.clone());
        drop(slot);
        self.log(format!("group endpoint {} bound", ep.id()));
        if !core.loops.swap(true, Ordering::SeqCst) {
            let (this, c, e) = (self.clone(), core.clone(), ep.clone());
            self.handle.spawn(async move { this.group_accept_loop(c, e).await });
            let (this, c) = (self.clone(), core.clone());
            self.handle.spawn(async move { this.group_dial_loop(c).await });
        }
        Ok(ep)
    }

    /// Waits (up to 5 s) for the group endpoint to know its home relay, so the addresses we
    /// hand out carry it.
    pub(crate) async fn group_relay_ready(&self) {
        let Some(ep) = self.group_endpoint() else { return };
        for _ in 0..50 {
            if relay_of(&ep).is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub(crate) fn group_endpoint(&self) -> Option<Endpoint> {
        self.group.core.lock().as_ref().and_then(|c| c.ep_id.lock().clone())
    }

    /// Our signed host address for `gid` (refreshed when the relay changes), if we host it.
    pub(crate) fn group_my_addr(&self, core: &GroupCore, gid: &str) -> Option<SignedHostAddr> {
        let hosting = core.groups.lock().get(gid).is_some_and(|l| l.state.is_host(&core.did, &core.dev));
        if !hosting {
            core.my_addr.lock().remove(gid);
            return None;
        }
        let ep = core.ep_id.lock().clone()?;
        let relay = relay_of(&ep);
        let mut cache = core.my_addr.lock();
        if let Some(s) = cache.get(gid)
            && s.open().is_some_and(|a| a.relay == relay)
        {
            return Some(s.clone());
        }
        let a = HostAddr { group: gid.to_string(), did: core.did.clone(), dev: core.dev, endpoint: *ep.id().as_bytes(), relay, at: now_ms() }.sign(&core.account);
        cache.insert(gid.to_string(), a.clone());
        Some(a)
    }

    fn group_ev(&self) -> Option<Arc<dyn GroupEvents>> {
        self.group.events.lock().clone()
    }

    /// After a change: events, removal enforcement, relay to every other session, host
    /// announcements.
    pub(crate) fn group_after(self: &Arc<Self>, core: &Arc<GroupCore>, changed: Changed, from: Option<u64>) {
        let gid = changed.gid.clone();
        // Order matters: the records first (a removed member gets the flags that removed it,
        // a new host's acceptance reaches others before its address), then enforcement, then
        // address announcements.
        self.group_push(core, &gid, from);
        self.group_enforce(core, &gid);
        // Became (or stopped being) a host: announce our address.
        let before = core.my_addr.lock().contains_key(&gid);
        let mine = self.group_my_addr(core, &gid);
        if mine.is_some() != before {
            let hosts = core.host_addrs(&gid);
            for s in core.sessions_of(&gid) {
                let _ = s.tx.send(Out::Msg(GroupMsg::Hosts { hosts: hosts.clone() }));
            }
        }
        core.kick.notify_waiters();
        if let Some(ev) = self.group_ev() {
            let msgs: Vec<_> = changed.new_items.iter().filter_map(|i| self.group_message(core, &gid, i)).collect();
            let handle = self.handle.clone();
            handle.spawn_blocking(move || {
                for m in msgs {
                    ev.on_group_message(m);
                }
                ev.on_group_changed(gid);
            });
        }
    }

    /// Host side: a session whose member is now removed gets the signed proof and is closed.
    /// Sessions with a device that no longer hosts, or (when we stopped hosting) with the
    /// members we served, are closed.
    fn group_enforce(&self, core: &GroupCore, gid: &str) {
        let removed: Vec<(Arc<Sess>, Vec<Vec<u8>>, Vec<u8>, Option<Vec<u8>>)> = {
            let groups = core.groups.lock();
            let Some(live) = groups.get(gid) else { return };
            if live.state.owner.is_none() {
                return;
            }
            let we_host = live.state.is_host(&core.did, &core.dev);
            for s in core.sessions_of(gid) {
                let stale = if s.hosting { !we_host && !live.state.is_removed(&s.did) } else { !live.state.is_host(&s.did, &s.dev) };
                if stale {
                    let _ = s.tx.send(Out::Close);
                    core.sessions.lock().remove(&s.id);
                }
            }
            core.sessions_of(gid)
                .into_iter()
                .filter(|s| s.hosting && live.state.is_removed(&s.did))
                .map(|s| {
                    let proof = core.removal_proof(live, &s.did);
                    let upto = core.removal_upto(live, &s.did, &s.peer_vv.lock());
                    (s, proof, live.doc.vv().encode(), upto)
                })
                .collect()
        };
        for (s, records, vv, update) in removed {
            let _ = s.tx.send(Out::Msg(GroupMsg::Removed { records, vv, update }));
            let _ = s.tx.send(Out::Close);
            core.sessions.lock().remove(&s.id);
        }
    }

    /// Sends every session of `gid` (except `from`) what it lacks.
    fn group_push(&self, core: &GroupCore, gid: &str, from: Option<u64>) {
        let groups = core.groups.lock();
        let Some(live) = groups.get(gid) else { return };
        let ours = live.doc.vv();
        for s in core.sessions_of(gid) {
            // A session of a member removed just now still gets this last push (it carries the
            // flags that removed it); `group_enforce` closes it right after.
            if Some(s.id) == from {
                continue;
            }
            let mut pv = s.peer_vv.lock();
            if let Some(update) = live.doc.export_since(&pv) {
                let _ = s.tx.send(Out::Msg(GroupMsg::Update { update }));
                pv.merge(&ours);
            }
        }
    }

    // ---- accepting (host side) ---------------------------------------------------------------

    async fn group_accept_loop(self: Arc<Self>, core: Arc<GroupCore>, ep: Endpoint) {
        while let Some(incoming) = ep.accept().await {
            if core.closed() {
                break;
            }
            let (this, core) = (self.clone(), core.clone());
            tokio::spawn(async move {
                let Ok(accepting) = incoming.accept() else { return };
                let Ok(conn) = tokio::time::timeout(HELLO_TIMEOUT, accepting).await else { return };
                let Ok(conn) = conn else { return };
                if let Err(e) = this.group_host_session(core, conn).await {
                    tracing::debug!("group incoming: {e}");
                }
            });
        }
    }

    async fn group_host_session(self: Arc<Self>, core: Arc<GroupCore>, conn: Connection) -> Result<(), Error> {
        let remote = *conn.remote_id().as_bytes();
        let (send, recv) = tokio::time::timeout(HELLO_TIMEOUT, conn.accept_bi()).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?;
        let mut writer = Writer(send);
        let mut reader = Reader::new(recv);
        let hello = tokio::time::timeout(HELLO_TIMEOUT, reader.recv()).await.map_err(|_| Error::Timeout)??;
        let Some(GroupMsg::Hello { auth, group, vv, invite, hosts }) = hello else { return Err(Error::Protocol("expected hello".into())) };
        if !auth.verify(&remote) {
            return Err(Error::Protocol("endpoint proof does not verify".into()));
        }
        enum Verdict {
            Refuse(&'static str),
            Removed(Vec<Vec<u8>>, Vec<u8>, Option<Vec<u8>>),
            Welcome { joining: bool },
        }
        let verdict = {
            let groups = core.groups.lock();
            match groups.get(&group) {
                None => Verdict::Refuse("unknown group"),
                Some(live) if !live.state.is_host(&core.did, &core.dev) => Verdict::Refuse("not a host of this group"),
                Some(live) if live.state.is_removed(&auth.did) => {
                    let upto = core.removal_upto(live, &auth.did, &decode_vv(&vv).unwrap_or_default());
                    Verdict::Removed(core.removal_proof(live, &auth.did), live.doc.vv().encode(), upto)
                }
                Some(live) if live.state.is_member(&auth.did) => Verdict::Welcome { joining: false },
                Some(live) => {
                    let ok = invite.as_ref().and_then(|i| i.open()).is_some_and(|i| i.group == group && i.expires >= now() && live.state.is_admin(&i.issuer));
                    if ok { Verdict::Welcome { joining: true } } else { Verdict::Refuse("not a member") }
                }
            }
        };
        match verdict {
            Verdict::Refuse(why) => {
                writer.send(&GroupMsg::Refused { reason: why.into() }).await?;
                let _ = writer.0.finish();
                let _ = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
                Ok(())
            }
            Verdict::Removed(records, vv, update) => {
                writer.send(&GroupMsg::Removed { records, vv, update }).await?;
                self.group_removed_upload(&core, &group, &auth.did, &mut writer, &mut reader).await;
                let _ = writer.0.finish();
                let _ = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
                Ok(())
            }
            Verdict::Welcome { joining } => {
                // U7: the host keeps the member device's group endpoint.
                let entry = DirEntry { endpoint: conn.remote_id().to_string(), seen_ms: now_ms() };
                let _ = core.store.put_dir(&group, &auth.did, &auth.dev, &entry);
                core.merge_hosts(&group, &hosts);
                let mine = EndpointAuth::new(&core.account, core.dev, *core.ep_id.lock().as_ref().map(|e| e.id()).ok_or(Error::NotStarted)?.as_bytes());
                let our_vv = core.groups.lock().get(&group).map(|l| l.doc.vv().encode()).unwrap_or_default();
                let hosts = core.host_addrs(&group);
                writer.send(&GroupMsg::Welcome { auth: mine, vv: our_vv, hosts }).await?;
                let peer_vv = decode_vv(&vv)?;
                self.group_run(core, conn, writer, reader, group, auth, true, joining, peer_vv).await
            }
        }
    }

    /// A removed member may upload only its own posts whose signed frontier predates its removal
    /// (U11 option A); anything else is refused and nothing new is sent to it.
    async fn group_removed_upload(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, did: &str, writer: &mut Writer, reader: &mut Reader) {
        let Ok(Ok(Some(GroupMsg::Update { update }))) = tokio::time::timeout(REMOVED_UPLOAD_WAIT, reader.recv()).await else { return };
        let did_owned = did.to_string();
        let res = core.apply(gid, &update, move |state, entries, fresh| {
            let flag_entries: Vec<&Entry> = state
                .removed
                .get(&did_owned)
                .map(|f| f.iter().filter_map(|fi| entries.iter().find(|e| e.id == fi.id)).collect())
                .unwrap_or_default();
            for e in fresh {
                let before = flag_entries.iter().filter(|f| f.seen_by(&e.rec.seen)).count() < super::state::REMOVAL_FLAGS;
                if e.rec.author != did_owned || !matches!(e.rec.body, Body::Post { .. }) || !before {
                    return Err("removed: only posts written before the removal are accepted".into());
                }
            }
            Ok(())
        });
        match res {
            Ok(changed) => {
                let vv = core.groups.lock().get(gid).map(|l| l.doc.vv().encode()).unwrap_or_default();
                let _ = writer.send(&GroupMsg::Ack { vv }).await;
                if let Some(c) = changed {
                    self.group_after(core, c, None);
                }
            }
            Err(e) => {
                self.log(format!("group {gid}: refused a removed member's upload: {e}"));
                let _ = writer.send(&GroupMsg::Refused { reason: e.to_string() }).await;
            }
        }
    }

    // ---- dialing (member side) ----------------------------------------------------------------

    async fn group_dial_loop(self: Arc<Self>, core: Arc<GroupCore>) {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(TICK) => {}
                _ = core.kick.notified() => {}
            }
            if core.closed() {
                break;
            }
            for (gid, addr) in self.group_targets(&core) {
                let key = (gid.clone(), host_key(&addr.did, &addr.dev));
                if !core.dialing.lock().insert(key.clone()) {
                    continue;
                }
                let (this, core) = (self.clone(), core.clone());
                tokio::spawn(async move {
                    let ok = this.clone().group_dial(core.clone(), gid.clone(), addr).await.is_ok();
                    core.dialing.lock().remove(&key);
                    let mut b = core.backoff.lock();
                    if ok {
                        b.remove(&key);
                    } else {
                        let n = b.get(&key).map(|(_, n)| n + 1).unwrap_or(0).min(BACKOFF.len() - 1);
                        b.insert(key, (Instant::now() + Duration::from_secs(BACKOFF[n]), n));
                    }
                });
            }
        }
    }

    /// Hosts to dial now: active hosts of each group we belong to (any validly signed address
    /// from the invite while we are still joining), minus ourselves, live sessions, dials in
    /// flight and those backing off.
    fn group_targets(&self, core: &GroupCore) -> Vec<(String, HostAddr)> {
        let only = self.group.dial_only.lock().clone();
        let ignore = self.group.ignore_removal.load(Ordering::SeqCst);
        let mut out = Vec::new();
        let groups: Vec<(String, bool, bool, Vec<(String, [u8; 32])>, bool)> = core
            .groups
            .lock()
            .iter()
            .map(|(g, l)| {
                let joining = l.state.owner.is_none();
                let gone = !joining && !l.state.is_member(&core.did) && !l.state.is_removed(&core.did);
                let removed = l.state.is_removed(&core.did) || !l.meta.removal_proof.is_empty();
                let hosts = l.state.hosts.iter().map(|h| (h.did.clone(), h.dev)).collect();
                (g.clone(), joining, gone, hosts, removed)
            })
            .collect();
        let now = Instant::now();
        for (gid, joining, gone, hosts, removed) in groups {
            // A member that left stops dialing once its leave reached a host.
            if gone && !self.group_owes(core, &gid) {
                continue;
            }
            // Removed: only to upload posts written before the removal (until acked).
            if removed && !ignore && !self.group_owes(core, &gid) {
                continue;
            }
            for s in core.store.hosts(&gid).unwrap_or_default() {
                let Some(a) = s.open() else { continue };
                if a.did == core.did && a.dev == core.dev {
                    continue;
                }
                if !joining && !hosts.contains(&(a.did.clone(), a.dev)) {
                    continue;
                }
                if only.as_ref().is_some_and(|o| *o != a.did) {
                    continue;
                }
                let key = (gid.clone(), host_key(&a.did, &a.dev));
                if core.has_session(&gid, &a.did, &a.dev) || core.dialing.lock().contains(&key) {
                    continue;
                }
                if core.backoff.lock().get(&key).is_some_and(|(t, _)| *t > now) {
                    continue;
                }
                out.push((gid.clone(), a));
            }
        }
        out
    }

    /// Whether some op of ours is not yet acknowledged by any host.
    fn group_owes(&self, core: &GroupCore, gid: &str) -> bool {
        let mine = core.groups.lock().get(gid).map(|l| l.doc.vv().get(&core.wid).copied().unwrap_or(0)).unwrap_or(0);
        if mine == 0 {
            return false;
        }
        !core.store.acks(gid).unwrap_or_default().iter().any(|(_, vv)| decode_vv(vv).ok().and_then(|v| v.get(&core.wid).copied()).unwrap_or(0) >= mine)
    }

    async fn group_dial(self: Arc<Self>, core: Arc<GroupCore>, gid: String, host: HostAddr) -> Result<(), Error> {
        // Not `group_ensure` (the dial loop is started by it): the endpoint is up while it runs.
        let ep = core.ep_id.lock().clone().ok_or(Error::NotStarted)?;
        let addr = addr_for(&host.endpoint, host.relay.as_deref())?;
        let conn = tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, GROUP_ALPN)).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?;
        let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
        let mut writer = Writer(send);
        let mut reader = Reader::new(recv);
        let (vv, invite) = {
            let groups = core.groups.lock();
            let live = groups.get(&gid).ok_or(Error::NotFound)?;
            let invite = live.meta.joining.as_ref().and_then(|b| bincode::deserialize::<SignedInvite>(b).ok());
            (live.doc.vv(), invite)
        };
        let auth = EndpointAuth::new(&core.account, core.dev, *ep.id().as_bytes());
        writer.send(&GroupMsg::Hello { auth, group: gid.clone(), vv: vv.encode(), invite, hosts: core.host_addrs(&gid) }).await?;
        let reply = tokio::time::timeout(HELLO_TIMEOUT, reader.recv()).await.map_err(|_| Error::Timeout)??;
        match reply {
            Some(GroupMsg::Welcome { auth, vv, hosts }) => {
                if !auth.verify(conn.remote_id().as_bytes()) || auth.did != host.did || auth.dev != host.dev {
                    return Err(Error::Protocol("host proof does not verify".into()));
                }
                core.refusals.lock().remove(&gid);
                if core.merge_hosts(&gid, &hosts) {
                    core.kick.notify_waiters();
                }
                let peer_vv = decode_vv(&vv)?;
                self.group_run(core, conn, writer, reader, gid, auth, false, false, peer_vv).await
            }
            Some(GroupMsg::Removed { records, vv, update }) => {
                if let Some(u) = update {
                    match core.apply(&gid, &u, |_, _, _| Ok(())) {
                        Ok(Some(c)) => self.group_after(&core, c, None),
                        Ok(None) => {}
                        Err(e) => tracing::debug!("group {gid}: removal records: {e}"),
                    }
                }
                let ignore = self.group.ignore_removal.load(Ordering::SeqCst);
                if !ignore && proof_removes(&gid, &core.did, &records).is_some() {
                    let mut groups = core.groups.lock();
                    if let Some(live) = groups.get_mut(&gid) {
                        live.meta.removal_proof = records;
                        let _ = core.store.put_meta(&gid, &live.meta);
                    }
                    drop(groups);
                    if let Some(ev) = self.group_ev() {
                        let g = gid.clone();
                        self.handle.spawn_blocking(move || ev.on_group_changed(g));
                    }
                }
                // Upload what we wrote that the host lacks; it keeps only pre-removal posts.
                let theirs = decode_vv(&vv)?;
                let up = core.groups.lock().get(&gid).and_then(|l| l.doc.export_peer_since(core.wid, &theirs));
                if let Some(update) = up {
                    writer.send(&GroupMsg::Update { update }).await?;
                    if let Ok(Ok(Some(GroupMsg::Ack { vv }))) = tokio::time::timeout(Duration::from_secs(10), reader.recv()).await {
                        let _ = core.store.put_ack(&gid, &host.did, &host.dev, &vv);
                    }
                }
                let _ = writer.0.finish();
                conn.close(0u32.into(), b"removed");
                Ok(())
            }
            Some(GroupMsg::Refused { reason }) => {
                core.refusals.lock().insert(gid.clone(), reason.clone());
                conn.close(0u32.into(), b"refused");
                Err(Error::Rejected(reason))
            }
            _ => Err(Error::Protocol("unexpected reply".into())),
        }
    }

    // ---- one session (both sides) --------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    async fn group_run(
        self: Arc<Self>,
        core: Arc<GroupCore>,
        conn: Connection,
        mut writer: Writer,
        mut reader: Reader,
        gid: String,
        peer: EndpointAuth,
        hosting: bool,
        joining: bool,
        peer_vv: VersionVector,
    ) -> Result<(), Error> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let id = core.next.fetch_add(1, Ordering::SeqCst);
        let sess = Arc::new(Sess { id, gid: gid.clone(), did: peer.did.clone(), dev: peer.dev, hosting, joining, tx, peer_vv: Mutex::new(peer_vv) });
        core.sessions.lock().insert(id, sess.clone());
        self.log(format!("group {} session with {} ({})", &gid[..8.min(gid.len())], &peer.did, if hosting { "we host" } else { "they host" }));
        // Catch the other side up.
        self.group_push(&core, &gid, None);
        if let Some(ev) = self.group_ev() {
            let g = gid.clone();
            self.handle.spawn_blocking(move || ev.on_group_changed(g));
        }
        let writer_task = tokio::spawn(async move {
            while let Some(out) = rx.recv().await {
                match out {
                    Out::Msg(m) => {
                        if writer.send(&m).await.is_err() {
                            break;
                        }
                    }
                    Out::Close => break,
                }
            }
            let _ = writer.0.finish();
        });
        let res = self.group_read(&core, &sess, &mut reader).await;
        core.sessions.lock().remove(&id);
        writer_task.abort();
        conn.close(0u32.into(), b"bye");
        if let Some(ev) = self.group_ev() {
            self.handle.spawn_blocking(move || ev.on_group_changed(gid));
        }
        core.kick.notify_waiters();
        res
    }

    async fn group_read(self: &Arc<Self>, core: &Arc<GroupCore>, sess: &Arc<Sess>, reader: &mut Reader) -> Result<(), Error> {
        loop {
            if core.closed() {
                return Ok(());
            }
            let Some(msg) = reader.recv().await? else { return Ok(()) };
            if !core.sessions.lock().contains_key(&sess.id) {
                return Ok(());
            }
            match msg {
                GroupMsg::Update { update } => {
                    let (did, joining, hosting) = (sess.did.clone(), sess.joining, sess.hosting);
                    let res = core.apply(&sess.gid, &update, move |state, _, fresh| {
                        // Host side, a joiner: only its own records until it is a member.
                        if hosting && joining && !state.is_member(&did) && fresh.iter().any(|e| e.rec.author != did) {
                            return Err("a joiner may send only its own records".into());
                        }
                        Ok(())
                    });
                    match res {
                        Ok(changed) => {
                            // The peer holds at least what it just sent.
                            if let Ok(meta) = loro::LoroDoc::decode_import_blob_meta(&update, false) {
                                sess.peer_vv.lock().merge(&meta.partial_end_vv);
                            }
                            let vv = core.groups.lock().get(&sess.gid).map(|l| l.doc.vv()).unwrap_or_default();
                            let _ = sess.tx.send(Out::Msg(GroupMsg::Ack { vv: vv.encode() }));
                            if let Some(c) = changed {
                                self.group_maybe_join(core, &sess.gid);
                                self.group_after(core, c, Some(sess.id));
                            }
                        }
                        Err(e) => {
                            self.log(format!("group {}: refused an update from {}: {e}", &sess.gid[..8.min(sess.gid.len())], sess.did));
                            let _ = sess.tx.send(Out::Msg(GroupMsg::Refused { reason: e.to_string() }));
                            let _ = sess.tx.send(Out::Close);
                            return Err(Error::Rejected(e.to_string()));
                        }
                    }
                }
                GroupMsg::Ack { vv } => {
                    let vv = decode_vv(&vv)?;
                    sess.peer_vv.lock().merge(&vv);
                    if !sess.hosting {
                        let _ = core.store.put_ack(&sess.gid, &sess.did, &sess.dev, &vv.encode());
                        if let Some(ev) = self.group_ev() {
                            let g = sess.gid.clone();
                            self.handle.spawn_blocking(move || ev.on_group_changed(g));
                        }
                    }
                }
                GroupMsg::Hosts { hosts } => {
                    if core.merge_hosts(&sess.gid, &hosts) {
                        let all = core.host_addrs(&sess.gid);
                        for s in core.sessions_of(&sess.gid) {
                            if s.id != sess.id {
                                let _ = s.tx.send(Out::Msg(GroupMsg::Hosts { hosts: all.clone() }));
                            }
                        }
                        core.kick.notify_waiters();
                    }
                }
                GroupMsg::Refused { reason } => {
                    return Err(Error::Rejected(reason));
                }
                _ => return Err(Error::Protocol("unexpected group frame".into())),
            }
        }
    }

    /// We joined with an invite and now hold the group: write our `Join` (once).
    fn group_maybe_join(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str) {
        let invite = {
            let mut groups = core.groups.lock();
            let Some(live) = groups.get_mut(gid) else { return };
            if live.state.owner.is_none() || live.state.is_member(&core.did) {
                if live.meta.joining.is_some() && live.state.is_member(&core.did) {
                    live.meta.joining = None;
                    let _ = core.store.put_meta(gid, &live.meta);
                }
                return;
            }
            let Some(b) = live.meta.joining.take() else { return };
            let _ = core.store.put_meta(gid, &live.meta);
            match bincode::deserialize::<SignedInvite>(&b) {
                Ok(i) => i,
                Err(_) => return,
            }
        };
        let name = self.me().map(|m| m.profile.name.clone()).unwrap_or_default();
        match core.write(gid, Body::Join { invite, name }) {
            Ok((_, c)) => self.group_after(core, c, None),
            Err(e) => tracing::warn!("group join: {e}"),
        }
    }
}
