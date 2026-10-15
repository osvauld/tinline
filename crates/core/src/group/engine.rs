//! The group runtime: the group endpoint (U2, `tinline/group/2`), host and member sessions,
//! live relay, catch-up, day-doc sync and removal enforcement. Everything hangs off
//! `Inner::group` (`GroupRt`).
//!
//! Members only dial hosts; hosts accept members and dial other hosts like members do; nobody
//! else is ever dialled, so a member learns only hosts' group endpoints (U7). Every update is
//! verified on arrival (`GroupDoc::apply_remote`) by every device, hosts included (U10).
//!
//! A group is its `meta` doc (always in memory, with the derived `GroupState`) and one doc per
//! UTC day of posts (`shard.rs`: loaded on demand, a bounded cache). A local write or a remote
//! update touches only the doc it belongs to; sessions send the meta doc before any day doc.

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
use super::doc::{DocId, Entry, GroupDoc, Reject};
use super::record::*;
use super::shard::{Shards, note_posts, recent_days};
use super::state::{GroupState, Item, Vis, derive_meta, post_item, post_visible};
use super::store::{DirEntry, GroupMeta, GroupStore};
use super::wire::*;
use crate::Error;
use crate::chat::doc::{DAY_MS, day_of, writer_id};
use crate::node::{Inner, addr_for, now, now_ms, relay_of};
use crate::wire::DIAL_TIMEOUT;

mod history;
mod synth;
#[cfg(test)]
mod durability;
#[cfg(test)]
pub(crate) fn receipt_cores_for_test(account: identity::Identity, source_epoch: u64, target_epoch: u64) -> (Arc<GroupCore>, Arc<GroupCore>, String, Vec<u8>) {
    durability::receipt_cores_for_test(account, source_epoch, target_epoch)
}

#[cfg(test)]
pub(crate) fn own_content_core_for_test(account: identity::Identity, epoch: u64, populated: bool) -> (Arc<GroupCore>, String, String) {
    durability::own_content_core_for_test(account, epoch, populated)
}

const HELLO_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a host waits for a removed member's pre-removal upload (U11).
const REMOVED_UPLOAD_WAIT: Duration = Duration::from_secs(10);
const TICK: Duration = Duration::from_secs(2);
const MAX_HELLO_DAYS: usize = 64;
const COMPACT_EVERY: Duration = Duration::from_secs(3600);
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
    pub(crate) req_events: Mutex<Option<Arc<dyn super::requests::GroupRequestEvents>>>,
    /// Test knob: a modified app that skips its own chat-request checks.
    pub(crate) skip_request_checks: AtomicBool,
    /// Delay between a removal / host revocation and the hosts' endpoint rotation, ms (U12:
    /// 2 days; 0 = that default). Test knob.
    pub(crate) rotation_delay_ms: AtomicU64,
    /// Group calls over this runtime (GA-I1, `calls.rs`).
    pub(crate) calls: super::calls::GroupCalls,
    /// How long a host that rotated keeps telling offline members it moved, ms (0 = the rotation
    /// delay, as in production). Test knob: a shortened rotation delay would otherwise shrink
    /// this window to seconds, and a member that comes back a little later is never told.
    pub(crate) moved_window_ms: AtomicU64,
    /// G3: invitation events and the DIDs with an invitation delivery task running.
    pub(crate) inv_events: Mutex<Option<Arc<dyn super::invitations::GroupInvitationEvents>>>,
    pub(crate) inv_delivering: Mutex<HashSet<String>>,
    /// GL: set by `group_close` (stop, lock, account switch), cleared by `start`. While set, only
    /// a call the app made opens the group store (`group_core`); a background task left over
    /// from the stopped session never does. Read and written under `core`'s lock.
    pub(crate) parked: AtomicBool,
    /// Test knobs (GL): this many invite fetches / host dials fail as if the connection was lost.
    pub(crate) fail_fetches: std::sync::atomic::AtomicU32,
    pub(crate) fail_dials: std::sync::atomic::AtomicU32,
    /// History backfill waits while set (the platform's conditions: metered network, low
    /// battery; the same inputs call hosting uses).
    pub(crate) backfill_paused: AtomicBool,
    /// Pause between two day updates of the background sync, ms (0 = the default). Test knob.
    pub(crate) backfill_pause_ms: AtomicU64,
}

/// Takes one injected failure from a test knob, if any is left.
pub(crate) fn injected(n: &std::sync::atomic::AtomicU32) -> bool {
    n.try_update(Ordering::SeqCst, Ordering::SeqCst, |v| v.checked_sub(1)).is_ok()
}

/// U12: hosts rotate their group endpoint this long after a removal or a host revocation.
pub const ROTATION_DELAY_MS: u64 = 2 * 24 * 3600 * 1000;
/// G6 §5 / Q12: a due rotation waits for a running group call, at most this long.
pub const CALL_ROTATION_DEFER_MS: u64 = 3600 * 1000;

impl GroupRt {
    pub(crate) fn rotation_delay(&self) -> u64 {
        match self.rotation_delay_ms.load(Ordering::SeqCst) {
            0 => ROTATION_DELAY_MS,
            v => v,
        }
    }

    pub(crate) fn moved_window(&self) -> u64 {
        match self.moved_window_ms.load(Ordering::SeqCst) {
            0 => self.rotation_delay(),
            v => v,
        }
    }
}

/// One group in memory: the meta doc, what it means, our local state and the day docs held.
pub(crate) struct Live {
    pub doc: GroupDoc,
    /// Records of the meta doc.
    pub entries: Vec<Entry>,
    pub state: GroupState,
    pub meta: GroupMeta,
    pub shards: Shards,
}

impl Live {
    /// A group we hold nothing of yet, writing under `wid`.
    pub fn empty(gid: &str, wid: u64, meta: GroupMeta) -> Self {
        Self { doc: GroupDoc::new(gid, wid), entries: Vec::new(), state: Default::default(), meta, shards: Shards::default() }
    }

    fn rederive(&mut self, gid: &str) {
        self.state = derive_meta(gid, &self.entries, &self.doc.vv());
    }

    fn restore_meta(&mut self, gid: &str, doc: GroupDoc, meta: GroupMeta, entries: usize) {
        self.doc = doc;
        self.meta = meta;
        self.entries.truncate(entries);
        self.rederive(gid);
        // Cached posts may have been rejudged against unpersisted meta. Reload
        // their accepted originals against the restored state on the next use.
        self.shards = Shards::default();
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
    /// Groups whose sessions `group_after` told that we host (GF: kept apart from the `my_addr`
    /// cache, which other paths fill too, so filling it elsewhere cannot swallow the announcement).
    announced: Mutex<HashSet<String>>,
    next: AtomicU64,
    kick: Notify,
    closed: AtomicBool,
    loops: AtomicBool,
    /// Groups whose members we are still telling about our new endpoint (U12).
    moving: Mutex<HashSet<String>>,
    /// When day docs were last compacted (hourly, and once after open).
    compacted: Mutex<Option<Instant>>,
    /// G3: invite codes (waiters for host answers, the wrong-code limiter).
    pub(crate) codes: super::code::CodeRt,
}

pub(crate) struct Sess {
    pub(crate) id: u64,
    pub(crate) gid: String,
    pub(crate) did: String,
    pub(crate) dev: [u8; 32],
    /// We accepted it: we host this session's member.
    pub(crate) hosting: bool,
    /// Host side: the member was joining (not yet a member) at Hello.
    joining: bool,
    pub(crate) tx: mpsc::UnboundedSender<Out>,
    /// What the peer holds of the meta doc.
    peer_vv: Mutex<VersionVector>,
    /// What the peer holds of the day docs we know it holds some of.
    peer_days: Mutex<HashMap<String, VersionVector>>,
    /// The peer's inventory of day docs is complete (`GroupMsg::Days`, the empty page).
    inv_done: AtomicBool,
}

pub(crate) enum Out {
    Msg(GroupMsg),
    Close,
}

impl Sess {
    /// The peer holds at least `vv` of `doc`.
    fn peer_has(&self, doc: &DocId, vv: &VersionVector) {
        match doc {
            DocId::Meta => self.peer_vv.lock().merge(vv),
            DocId::Day(d) => self.peer_days.lock().entry(d.clone()).or_default().merge(vv),
        }
    }

    /// The session's policy for what the peer sends (`GroupCore::apply`'s `admit`): on the host
    /// side, a joiner may send only its own records until it is a member.
    fn admit(&self) -> impl FnOnce(&GroupState, &[Entry], &[Entry]) -> Result<(), String> + use<> {
        let (did, joining, hosting) = (self.did.clone(), self.joining, self.hosting);
        move |state, _, fresh| {
            if hosting && joining && !state.is_member(&did) && fresh.iter().any(|e| e.rec.author != did) {
                return Err("a joiner may send only its own records".into());
            }
            Ok(())
        }
    }
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

/// What the other side of a new session says it holds: the meta doc and the recent day docs.
pub(crate) struct Peer {
    vv: VersionVector,
    days: HashMap<String, VersionVector>,
}

impl Peer {
    fn new(vv: &[u8], days: &[(String, Vec<u8>)]) -> Result<Self, Error> {
        if days.len() > MAX_HELLO_DAYS {
            return Err(Error::Protocol("too many days in hello".into()));
        }
        let mut map = HashMap::new();
        for (d, v) in days {
            if !DocId::Day(d.clone()).is_valid() {
                return Err(Error::Protocol("bad day in hello".into()));
            }
            map.insert(d.clone(), decode_vv(v)?);
        }
        Ok(Self { vv: decode_vv(vv)?, days: map })
    }
}

/// What changed in a group, for events.
pub(crate) struct Changed {
    pub gid: String,
    /// The doc whose update this was (what sessions relay), if it came from a doc at all.
    pub doc: DocId,
    /// The doc's version vector after the change.
    pub doc_vv: Vec<u8>,
    /// New conversation lines that show: system lines and posts.
    pub new_items: Vec<Item>,
    /// Per day doc, for the search index (`search/api.rs`).
    pub posts: Vec<PostDiff>,
    /// Arrived for a day older than a session pushes live (history catching up): no new-message events.
    pub history: bool,
}

/// The posts of one day doc a change made show, and the doc's version vector before and after.
pub(crate) struct PostDiff {
    pub day: String,
    pub put: Vec<Item>,
    pub prev: Vec<u8>,
    pub vv: Vec<u8>,
}

impl GroupCore {
    /// The version vector of a doc of `live` (the group's entry in `groups`, already locked).
    pub(crate) fn doc_vv_of(&self, live: &Live, gid: &str, doc: &DocId) -> VersionVector {
        match doc {
            DocId::Meta => live.doc.vv(),
            DocId::Day(d) => match live.shards.peek(d) {
                Some(s) => s.doc.vv(),
                None => self.store.shard_meta(gid, d).ok().flatten().and_then(|m| decode_vv(&m.vv).ok()).unwrap_or_default(),
            },
        }
    }

    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// GL: someone is waiting for `gid` (a join): forget the dial backoff of its hosts and dial
    /// now, so a failed dial is tried again within seconds instead of at the slow backoff.
    pub(crate) fn hurry(&self, gid: &str) {
        self.backoff.lock().retain(|(g, _), _| g != gid);
        self.kick.notify_waiters();
    }

    /// Marks the core closed, ends its sessions and loops, and closes its store (for every
    /// holder, after the operations in flight). The endpoint is the caller's.
    pub(crate) fn shut(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.kick.notify_waiters();
        for s in self.sessions.lock().values() {
            let _ = s.tx.send(Out::Close);
        }
        self.store.db.close();
    }

    pub(crate) fn sessions_of(&self, gid: &str) -> Vec<Arc<Sess>> {
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

    /// Writes one record of ours into `gid`; returns the entry and what changed. A post goes
    /// into the day doc of its time, every other record into the meta doc.
    pub fn write(&self, gid: &str, body: Body) -> Result<(Entry, Changed), Error> {
        self.write_at(gid, body, now_ms() as i64)
    }

    /// `write` with the record's time given (tests write into past days).
    pub fn write_at(&self, gid: &str, body: Body, at: i64) -> Result<(Entry, Changed), Error> {
        let mut groups = self.groups.lock();
        let live = groups.get_mut(gid).ok_or(Error::NotFound)?;
        let rec = Record { v: 1, group: gid.to_string(), author: self.did.clone(), dev: self.dev, at, seen: live.doc.seen(), body };
        if matches!(rec.body, Body::Post { .. }) { self.write_post(live, gid, rec) } else { self.write_meta(live, gid, rec) }
    }

    fn write_meta(&self, live: &mut Live, gid: &str, rec: Record) -> Result<(Entry, Changed), Error> {
        let previous_doc = live.doc.checkpoint();
        let previous_meta = live.meta.clone();
        let previous_entries = live.entries.len();
        let (entry, update) = live.doc.write(&self.account, &rec).map_err(|e| Error::Protocol(e.to_string()))?;
        let before: HashSet<String> = live.state.items.iter().map(|i| i.id.clone()).collect();
        live.entries.push(entry.clone());
        live.rederive(gid);
        let doc = &live.doc;
        if let Err(e) = self.store.append_meta(gid, &mut live.meta, &update, || doc.snapshot()) {
            live.restore_meta(gid, previous_doc, previous_meta, previous_entries);
            return Err(e);
        }
        let new_items = live.state.items.iter().filter(|i| !before.contains(&i.id)).cloned().collect();
        Ok((entry, Changed { gid: gid.to_string(), doc: DocId::Meta, doc_vv: live.doc.vv().encode(), new_items, posts: Vec::new(), history: false }))
    }

    fn write_post(&self, live: &mut Live, gid: &str, rec: Record) -> Result<(Entry, Changed), Error> {
        let previous_meta = live.meta.clone();
        let day = day_of(rec.at);
        let Live { state, shards, meta, .. } = live;
        let shard = shards.get(&self.store, gid, self.wid, &day, state)?;
        let prev = shard.doc.vv().encode();
        let (entry, update) = shard.doc.write(&self.account, &rec).map_err(|e| Error::Protocol(e.to_string()))?;
        let vis = post_visible(state, &entry.rec.author, &entry.rec.seen);
        shard.insert(entry.clone(), vis);
        let shown: Vec<Item> = post_item(&entry, state).filter(|_| vis == Vis::Visible).into_iter().collect();
        note_posts(meta, &self.did, &shown);
        meta.owed_days.insert(day.clone());
        if vis == Vis::Pending {
            meta.pending_days.insert(day.clone());
        }
        if meta.newest_day.as_ref().is_none_or(|d| *d < day) {
            meta.newest_day = Some(day.clone());
        }
        let vv = shard.doc.vv().encode();
        let doc = &shard.doc;
        if let Err(e) = self.store.append_shard(gid, &day, &mut shard.meta, vv.clone(), &update, || doc.snapshot(), Some(meta)) {
            *meta = previous_meta;
            shards.remove(&day);
            return Err(e);
        }
        let posts = vec![PostDiff { day: day.clone(), put: shown.clone(), prev, vv: vv.clone() }];
        Ok((entry, Changed { gid: gid.to_string(), doc: DocId::Day(day), doc_vv: vv, new_items: shown, posts, history: false }))
    }

    /// Validates and applies a remote update of one doc; `admit` is the session's policy, asked
    /// with the group's current state and meta entries and the batch's new entries before
    /// anything is imported.
    pub(crate) fn apply(&self, gid: &str, doc: &DocId, update: &[u8], admit: impl FnOnce(&GroupState, &[Entry], &[Entry]) -> Result<(), String>) -> Result<Option<Changed>, Reject> {
        self.apply_ingress(gid, doc, update, false, admit)
    }

    pub(crate) fn apply_own_contents(&self, gid: &str, doc: &DocId, update: &[u8]) -> Result<Option<Changed>, Reject> {
        self.apply_ingress(gid, doc, update, true, |_, _, _| Ok(()))
    }

    fn apply_ingress(&self, gid: &str, doc: &DocId, update: &[u8], own_forward: bool, admit: impl FnOnce(&GroupState, &[Entry], &[Entry]) -> Result<(), String>) -> Result<Option<Changed>, Reject> {
        if !doc.is_valid() {
            return Err(Reject::WrongDoc);
        }
        let mut groups = self.groups.lock();
        let Some(live) = groups.get_mut(gid) else { return Err(Reject::Policy("unknown group".into())) };
        match doc {
            DocId::Meta => self.apply_meta(live, gid, update, own_forward, admit),
            DocId::Day(day) => self.apply_day(live, gid, day, update, own_forward, admit),
        }
    }

    fn apply_meta(&self, live: &mut Live, gid: &str, update: &[u8], own_forward: bool, admit: impl FnOnce(&GroupState, &[Entry], &[Entry]) -> Result<(), String>) -> Result<Option<Changed>, Reject> {
        let previous_doc = live.doc.checkpoint();
        let previous_meta = live.meta.clone();
        let previous_entries = live.entries.len();
        let fresh = {
            let Live { doc, entries, state, .. } = &mut *live;
            doc.apply_remote(update, |fresh| admit(state, entries, fresh))?
        };
        // A known record ID can still arrive in a newly accepted op range.
        // Persist that VV before acknowledging it, even without a new item.
        if fresh.is_empty() && live.doc.vv() == previous_doc.vv() {
            return Ok(None);
        }
        let before: HashSet<String> = live.state.items.iter().map(|i| i.id.clone()).collect();
        live.entries.extend(fresh);
        live.rederive(gid);
        // Posts that were waiting for meta records may show now.
        let (posts, mut new_items) = self.rejudge_pending(live, gid);
        if own_forward {
            // Borrowed writers don't appear under our core.wid. Persist the
            // validated frontier with the ops before issuing a cache receipt.
            live.meta.own_owed_meta = live.doc.vv().encode();
        }
        let doc = &live.doc;
        if let Err(e) = self.store.append_meta(gid, &mut live.meta, update, || doc.snapshot()) {
            live.restore_meta(gid, previous_doc, previous_meta, previous_entries);
            return Err(Reject::Persistence(e.to_string()));
        }
        new_items.extend(live.state.items.iter().filter(|i| !before.contains(&i.id)).cloned());
        Ok(Some(Changed { gid: gid.to_string(), doc: DocId::Meta, doc_vv: live.doc.vv().encode(), new_items, posts, history: false }))
    }

    /// Judges again the posts that named meta records we lacked, in every day that has some.
    fn rejudge_pending(&self, live: &mut Live, gid: &str) -> (Vec<PostDiff>, Vec<Item>) {
        let (mut diffs, mut shown) = (Vec::new(), Vec::new());
        let Live { state, shards, meta, .. } = live;
        for day in meta.pending_days.clone() {
            let shard = match shards.get(&self.store, gid, self.wid, &day, state) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("group {gid} day {day}: {e}");
                    continue;
                }
            };
            let now_shown = shard.rejudge(state);
            if !shard.has_pending() {
                meta.pending_days.remove(&day);
            }
            if !now_shown.is_empty() {
                let vv = shard.doc.vv().encode();
                diffs.push(PostDiff { day, put: now_shown.clone(), prev: vv.clone(), vv });
                shown.extend(now_shown);
            }
        }
        note_posts(meta, &self.did, &shown);
        (diffs, shown)
    }

    fn apply_day(&self, live: &mut Live, gid: &str, day: &str, update: &[u8], own_forward: bool, admit: impl FnOnce(&GroupState, &[Entry], &[Entry]) -> Result<(), String>) -> Result<Option<Changed>, Reject> {
        let previous_meta = live.meta.clone();
        let Live { entries, state, shards, meta, .. } = live;
        let shard = shards.get(&self.store, gid, self.wid, day, state).map_err(|e| Reject::Policy(e.to_string()))?;
        let prev_vv = shard.doc.vv();
        let prev = prev_vv.encode();
        let fresh = match shard.doc.apply_remote_inplace(update, |fresh| admit(state, entries, fresh)) {
            Ok(f) => f,
            Err(e) => {
                // A refused batch may have left ops in the doc: forget it, the store has the rest.
                if shard.doc.tainted() {
                    shards.remove(day);
                }
                return Err(e);
            }
        };
        let fresh: Vec<Entry> = fresh.into_iter().filter(|e| !shard.has(e)).collect();
        if shard.doc.vv() == prev_vv {
            return Ok(None);
        }
        if own_forward && fresh.iter().any(|e| e.rec.author == self.did) {
            // An own-device phone write keeps its original writer, not ours.
            // Persist the upstream obligation atomically with its day ops/VV.
            meta.owed_days.insert(day.to_string());
        }
        let mut shown = Vec::new();
        for e in fresh {
            let vis = post_visible(state, &e.rec.author, &e.rec.seen);
            if vis == Vis::Pending {
                meta.pending_days.insert(day.to_string());
            }
            if vis == Vis::Visible {
                shown.extend(post_item(&e, state));
            }
            shard.insert(e, vis);
        }
        note_posts(meta, &self.did, &shown);
        if meta.newest_day.as_deref().is_none_or(|d| d < day) {
            meta.newest_day = Some(day.to_string());
        }
        let vv = shard.doc.vv().encode();
        let doc = &shard.doc;
        if let Err(e) = self.store.append_shard(gid, day, &mut shard.meta, vv.clone(), update, || doc.snapshot(), Some(meta)) {
            *meta = previous_meta;
            // Never acknowledge or retain a day VV that exists only in memory.
            // The next access reloads the last accepted rows from the store.
            shards.remove(day);
            return Err(Reject::Persistence(e.to_string()));
        }
        let posts = vec![PostDiff { day: day.to_string(), put: shown.clone(), prev, vv: vv.clone() }];
        Ok(Some(Changed { gid: gid.to_string(), doc: DocId::Day(day.to_string()), doc_vv: vv, new_items: shown, posts, history: false }))
    }

    /// The version vector of one of `gid`'s docs as we hold it (empty if we hold none of it).
    pub(crate) fn doc_vv(&self, gid: &str, doc: &DocId) -> VersionVector {
        let groups = self.groups.lock();
        let Some(live) = groups.get(gid) else { return VersionVector::default() };
        self.doc_vv_of(live, gid, doc)
    }

    /// The recent days (see `HELLO_DAYS`) of `gid` we hold, with their version vectors.
    pub(crate) fn hello_days(&self, gid: &str) -> Vec<(String, Vec<u8>)> {
        recent_days(now_ms() as i64, HELLO_DAYS)
            .into_iter()
            .filter_map(|d| {
                let m = self.store.shard_meta(gid, &d).ok().flatten()?;
                Some((d, m.vv))
            })
            .collect()
    }

    /// Host addresses we pass on for `gid`: active chat and call hosts only (never a plain
    /// member's endpoint), ours included when we host either.
    fn host_addrs(&self, gid: &str) -> Vec<SignedHostAddr> {
        let groups = self.groups.lock();
        let Some(live) = groups.get(gid) else { return Vec::new() };
        let mut out: Vec<SignedHostAddr> = self
            .store
            .hosts(gid)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.open().is_some_and(|a| hosts_or_calls(&live.state, &a.did, &a.dev) && !(a.did == self.did && a.dev == self.dev)))
            .collect();
        if hosts_or_calls(&live.state, &self.did, &self.dev)
            && let Some(mine) = self.my_addr.lock().get(gid).cloned()
        {
            out.push(mine);
        }
        out
    }

    /// Sends every session of `gid` (except `from`) what it lacks of the meta doc and then of
    /// the day docs `days` (the meta doc always goes first, so a post never arrives before the
    /// records it names). Older days reach a peer through the inventory sync (`history.rs`), not through here.
    pub(crate) fn push(&self, gid: &str, from: Option<u64>, days: &[String]) {
        let mut groups = self.groups.lock();
        let Some(live) = groups.get_mut(gid) else { return };
        let ours = live.doc.vv();
        // A session of a member removed just now still gets this last push (it carries the
        // flags that removed it); `group_enforce` closes it right after. It gets no posts.
        let sessions: Vec<Arc<Sess>> = self.sessions_of(gid).into_iter().filter(|s| Some(s.id) != from).collect();
        for s in &sessions {
            let mut pv = s.peer_vv.lock();
            if let Some(update) = live.doc.export_since(&pv) {
                let _ = s.tx.send(Out::Msg(GroupMsg::Update { doc: DocId::Meta, update }));
                pv.merge(&ours);
            }
        }
        for day in days {
            let Live { state, shards, .. } = &mut *live;
            let Ok(shard) = shards.get(&self.store, gid, self.wid, day, state) else { continue };
            let theirs = shard.doc.vv();
            for s in &sessions {
                if s.hosting && state.is_removed(&s.did) {
                    continue;
                }
                let mut pd = s.peer_days.lock();
                let pv = pd.entry(day.clone()).or_default();
                if let Some(update) = shard.doc.export_since(pv) {
                    let _ = s.tx.send(Out::Msg(GroupMsg::Update { doc: DocId::Day(day.clone()), update }));
                    pv.merge(&theirs);
                }
            }
        }
    }

    /// The days of the last `HELLO_DAYS` we hold (what a new session catches the peer up on).
    pub(crate) fn held_recent_days(&self, gid: &str) -> Vec<String> {
        recent_days(now_ms() as i64, HELLO_DAYS).into_iter().filter(|d| self.store.shard_meta(gid, d).ok().flatten().is_some()).collect()
    }

    /// `merge_hosts`, and on a change pass the addresses on to every other session of `gid`
    /// (all but `from`) and wake the dialer. GF: every path that learns an address spreads it;
    /// before, only a `Hosts` frame did, so an address learned in a Hello/Welcome (e.g. a new
    /// host's, when its session to the first host was re-established right after it accepted)
    /// reached that host but never its members.
    fn merge_hosts_from(&self, gid: &str, addrs: &[SignedHostAddr], from: Option<u64>) -> bool {
        if !self.merge_hosts(gid, addrs) {
            return false;
        }
        // The records that make these hosts valid go first, on each session's ordered stream.
        self.push(gid, None, &[]);
        let all = self.host_addrs(gid);
        for s in self.sessions_of(gid) {
            if Some(s.id) != from {
                let _ = s.tx.send(Out::Msg(GroupMsg::Hosts { hosts: all.clone() }));
            }
        }
        self.kick.notify_waiters();
        true
    }

    /// Stores host addresses that are newer and belong to active chat or call hosts (or, before
    /// we hold any state, i.e. while joining, any validly signed one for this group). True if any
    /// changed.
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
            if live.state.owner.is_some() && !hosts_or_calls(&live.state, &a.did, &a.dev) {
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

    /// Addresses for a token (invite link answer, direct invitation, own-device sync): active
    /// CHAT hosts only, ours included when we host the chat. A call-only host refuses joiners
    /// and fetches; the joiner learns call hosts from a chat host's Welcome.
    pub(crate) fn chat_host_addrs(&self, gid: &str) -> Vec<SignedHostAddr> {
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

    pub(crate) fn merge_hosts_pub(&self, gid: &str, addrs: &[SignedHostAddr]) -> bool {
        self.merge_hosts_from(gid, addrs, None)
    }

    pub(crate) fn kick_pub(&self) {
        self.kick.notify_waiters();
    }

    /// Sends `msg` on a session with one of `gid`'s active hosts (we are the member side);
    /// returns which host, or `None` if we have no such session.
    pub(crate) fn send_to_a_host(&self, gid: &str, msg: GroupMsg) -> Option<(String, [u8; 32])> {
        let hosts: Vec<(String, [u8; 32])> = self.groups.lock().get(gid).map(|l| l.state.hosts.iter().map(|h| (h.did.clone(), h.dev)).collect()).unwrap_or_default();
        let s = self.sessions_of(gid).into_iter().find(|s| !s.hosting && hosts.contains(&(s.did.clone(), s.dev)))?;
        s.tx.send(Out::Msg(msg)).ok()?;
        Some((s.did.clone(), s.dev))
    }

    /// Sends `msg` on our session with host device `did`/`dev` of `gid`, if there is one.
    pub(crate) fn send_to_host(&self, gid: &str, did: &str, dev: &[u8; 32], msg: GroupMsg) -> bool {
        self.sessions_of(gid).into_iter().find(|s| !s.hosting && s.did == did && &s.dev == dev).is_some_and(|s| s.tx.send(Out::Msg(msg)).is_ok())
    }
}

/// Device `dev` of `did` hosts the group's chat or its calls: the devices whose group endpoint
/// members may know (U7, U21).
pub(crate) fn hosts_or_calls(st: &GroupState, did: &str, dev: &[u8; 32]) -> bool {
    st.is_host(did, dev) || st.is_call_host(did, dev)
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
    ///
    /// GL: a core of another session (an earlier unlock, another account) lets its store go
    /// before the new one opens, and while the runtime is parked (after stop / lock / a switch,
    /// until the next start) only the app's own calls open it, never a leftover task.
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
        if let Some(old) = slot.take() {
            old.shut();
            if let Ok(mut ep) = old.ep.try_lock()
                && let Some(ep) = ep.take()
            {
                self.handle.spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(3), ep.close()).await;
                });
            }
        }
        if self.group.parked.load(Ordering::SeqCst) && !crate::node::user_call() {
            return Err(Error::NotStarted);
        }
        let me = self.me()?;
        let (dir, profile_dev) = {
            let s = self.shared.lock();
            let dir = s.acct.as_ref().ok_or(Error::Locked)?.store.dir().to_path_buf();
            let dev = match s.disk.as_ref() {
                Some(crate::store::Disk::V2(p)) => p.device_id_bytes(),
                _ => None,
            };
            (dir, dev)
        };
        // G2 stage 1 (P10 for groups): a group store made from nothing writes under a fresh
        // device ID, so a lost `group.redb` never makes us reuse a (writer ID, counter) that some
        // host may still hold. A store from G1 (no `dev/id` row yet) keeps the profile's ID.
        let path = dir.join("group.redb");
        let existed = path.exists();
        let store = GroupStore::open(path, &me.profile.mnemonic)?;
        let dev = store.device_id(if existed { profile_dev } else { None })?;
        let did = me.id.did().to_string();
        let wid = writer_id(&did, &dev);
        let mut groups = HashMap::new();
        for (gid, meta) in store.groups()? {
            let (snap, ups) = store.meta_rows(&gid)?;
            let doc = match GroupDoc::load(&gid, wid, snap.as_deref(), &ups) {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("group {gid}: {e}");
                    continue;
                }
            };
            // Our own sealed rows: verified when they entered the store, not again. Day docs are
            // not read here at all.
            let entries = doc.entries_trusted().unwrap_or_default();
            let state = derive_meta(&gid, &entries, &doc.vv());
            groups.insert(gid, Live { doc, entries, state, meta, shards: Shards::default() });
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
            announced: Default::default(),
            next: AtomicU64::new(1),
            kick: Notify::new(),
            closed: AtomicBool::new(false),
            loops: AtomicBool::new(false),
            moving: Default::default(),
            compacted: Default::default(),
            codes: Default::default(),
        });
        *slot = Some(core.clone());
        self.search_reconcile_groups(&core);
        Ok(core)
    }

    /// Stops everything of the group runtime (lock, stop, account switch). The store is closed
    /// before this returns (GL): tasks still holding the core find it closed and cannot keep the
    /// file, and none opens it again until the next start (`parked`).
    pub(crate) async fn group_close(&self) {
        let core = {
            let mut slot = self.group.core.lock();
            self.group.parked.store(true, Ordering::SeqCst);
            slot.take()
        };
        if let Some(core) = core {
            core.shut();
            let ep = core.ep.lock().await.take();
            *core.ep_id.lock() = None;
            if let Some(ep) = ep {
                let _ = tokio::time::timeout(Duration::from_secs(3), ep.close()).await;
            }
        }
    }

    /// `group_close` without waiting (for `Node`'s drop on a core thread): the store closes; the
    /// group endpoint is closed on a task.
    pub(crate) fn group_close_now(&self) {
        let core = {
            let mut slot = self.group.core.lock();
            self.group.parked.store(true, Ordering::SeqCst);
            slot.take()
        };
        if let Some(core) = core {
            core.shut();
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
        let ep = self.group_bind(core).await?;
        if !core.loops.swap(true, Ordering::SeqCst) {
            let (this, c) = (self.clone(), core.clone());
            self.handle.spawn(async move { this.group_dial_loop(c).await });
        }
        Ok(ep)
    }

    /// Binds the group endpoint if it is not bound, with its accept loop (not the dial loop:
    /// the rotation, which runs inside the dial loop, rebinds through here).
    async fn group_bind(self: &Arc<Self>, core: &Arc<GroupCore>) -> Result<Endpoint, Error> {
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
            .alpns(vec![GROUP_ALPN.to_vec(), proto::gcall::GCALL_ALPN.to_vec()])
            .transport_config(transport);
        if std::env::var_os("P2P_RELAY_ONLY").is_some() {
            builder = builder.clear_ip_transports();
        }
        let ep = builder.bind().await.map_err(Error::net)?;
        *slot = Some(ep.clone());
        *core.ep_id.lock() = Some(ep.clone());
        drop(slot);
        self.log(format!("group endpoint {} bound", ep.id()));
        let (this, c, e) = (self.clone(), core.clone(), ep.clone());
        self.handle.spawn(async move { this.group_accept_loop(c, e).await });
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

    /// Our signed host address for `gid` (refreshed when the relay changes), if we host its chat
    /// or its calls.
    pub(crate) fn group_my_addr(&self, core: &GroupCore, gid: &str) -> Option<SignedHostAddr> {
        let hosting = core.groups.lock().get(gid).is_some_and(|l| hosts_or_calls(&l.state, &core.did, &core.dev));
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
        self.search_group(&gid, &changed.posts);
        // Order matters: the records first (a removed member gets the flags that removed it,
        // a new host's acceptance reaches others before its address), then enforcement, then
        // address announcements.
        let days: Vec<String> = changed.doc.day().map(str::to_string).into_iter().collect();
        self.group_push(core, &gid, from, &days);
        self.group_enforce(core, &gid);
        // Became (or stopped being) a host: announce our address.
        let mine = self.group_my_addr(core, &gid);
        let flipped = {
            let mut announced = core.announced.lock();
            if mine.is_some() { announced.insert(gid.clone()) } else { announced.remove(&gid) }
        };
        if flipped {
            // GF: `group_push` above skipped `from`, and that session may not have the record
            // that makes us a host yet (it is in flight from a concurrent `group_after`); send it
            // first, or the receiver drops our address as not a host's and never sees it again.
            self.group_push(core, &gid, None, &[]);
            let hosts = core.host_addrs(&gid);
            for s in core.sessions_of(&gid) {
                let _ = s.tx.send(Out::Msg(GroupMsg::Hosts { hosts: hosts.clone() }));
            }
        }
        core.kick.notify_waiters();
        // Our other devices learn the group list (stage 2; deduplicated per session).
        self.selfsync_dirty(crate::chat::own::GROUPS_MARK);
        self.selfsync_dirty(&super::own::contents::doc_name(&gid, &changed.doc));
        self.group_publish_key(core, &gid);
        // G3: invitees that became members read Joined.
        self.group_invitations_changed(core, &gid);
        if let Some(ev) = self.group_ev() {
            let msgs: Vec<_> = if changed.history { Vec::new() } else { changed.new_items.iter().filter_map(|i| self.group_message(core, &gid, i)).collect() };
            let handle = self.handle.clone();
            handle.spawn_blocking(move || {
                for m in msgs {
                    ev.on_group_message(m);
                }
                ev.on_group_changed(gid);
            });
        }
    }

    /// As a member, publish our encryption key in the group once per device (U3 mail needs it).
    /// Every device writes at least this one meta record: posts live in day docs, and the
    /// member's device list ("appoint a host") comes from the meta doc.
    fn group_publish_key(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str) {
        let key = core.account.encryption_public_key();
        let need = core.groups.lock().get(gid).is_some_and(|l| {
            l.state.owner.is_some()
                && l.state.is_member(&core.did)
                && !l.state.is_removed(&core.did)
                && (l.state.enc_keys.get(&core.did) != Some(&key) || !l.state.members.get(&core.did).is_some_and(|m| m.devices.contains(&core.dev)))
        });
        if need {
            match core.write(gid, Body::EncKey { key }) {
                Ok((_, c)) => self.group_after(core, c, None),
                Err(e) => tracing::warn!("group key: {e}"),
            }
        }
    }

    /// Sends sealed mail of ours: as a host we take it in ourselves; otherwise it waits in the
    /// outbox and goes to every host we have a session with (again on each new session) until
    /// one acknowledges it.
    pub(crate) fn group_send_mail(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, sm: super::mail::SignedMail) {
        let bytes = sm.to_bytes();
        let hosting = core.groups.lock().get(gid).is_some_and(|l| l.state.is_host(&core.did, &core.dev));
        if hosting {
            self.group_mail_intake(core, gid, &bytes, None);
            return;
        }
        let Some(m) = sm.open() else { return };
        if let Err(e) = core.store.put_out(gid, &super::mail::id_hex(&m.id), &bytes) {
            tracing::warn!("group mail: {e}");
        }
        for s in core.sessions_of(gid) {
            if !s.hosting {
                let _ = s.tx.send(Out::Msg(GroupMsg::Mail { mail: bytes.clone() }));
            }
        }
    }

    /// Mail from a session (`from`) or from ourselves (`None`). Members may send only their own
    /// mail; sender and recipient must be members, the sender not removed. We open what is ours;
    /// as a host we keep the rest for its recipient and pass it to the recipient's sessions and
    /// to other hosts.
    fn group_mail_intake(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, bytes: &[u8], from: Option<&Arc<Sess>>) {
        use super::mail::*;
        let Some(sm) = SignedMail::from_bytes(bytes) else { return };
        let Some(m) = sm.open() else { return };
        if m.group != gid {
            return;
        }
        let (we_host, ok, from_host) = {
            let groups = core.groups.lock();
            let Some(l) = groups.get(gid) else { return };
            let s = &l.state;
            let from_host = from.is_some_and(|x| s.is_host(&x.did, &x.dev));
            (s.is_host(&core.did, &core.dev), s.is_member(&m.from) && !s.is_removed(&m.from) && s.is_member(&m.to), from_host)
        };
        if let Some(x) = from
            && !from_host
            && x.did != m.from
        {
            return;
        }
        let id = id_hex(&m.id);
        if we_host
            && let Some(x) = from
            && !from_host
        {
            // Taken (or refused for good): the sender stops resending either way.
            let _ = x.tx.send(Out::Msg(GroupMsg::MailAck { id: m.id }));
        }
        if !ok {
            return;
        }
        if m.to == core.did {
            self.group_mail_for_me(core, gid, &sm);
        }
        if !we_host {
            return;
        }
        let now = now_ms();
        if core.store.mail(gid, Some(&m.to), now).map(|v| v.len()).unwrap_or(0) >= MAX_PENDING_PER_RECIPIENT {
            return;
        }
        let fresh = core.store.put_mail(gid, &m.to, &id, m.at.min(now) + MAIL_TTL_MS, bytes).unwrap_or(false);
        if !fresh {
            return;
        }
        let hosts: Vec<(String, [u8; 32])> = core.groups.lock().get(gid).map(|l| l.state.hosts.iter().map(|h| (h.did.clone(), h.dev)).collect()).unwrap_or_default();
        for s in core.sessions_of(gid) {
            if from.is_some_and(|x| x.id == s.id) {
                continue;
            }
            let to_recipient = s.hosting && s.did == m.to;
            let to_host = hosts.contains(&(s.did.clone(), s.dev));
            if to_recipient || to_host {
                let _ = s.tx.send(Out::Msg(GroupMsg::Mail { mail: bytes.to_vec() }));
            }
        }
    }

    /// A new session: our outbox to a host; to a member we host, its waiting mail; to another
    /// host, all mail we keep.
    fn group_mail_on_session(&self, core: &GroupCore, sess: &Sess) {
        let (we_host, peer_host) = core
            .groups
            .lock()
            .get(&sess.gid)
            .map(|l| (l.state.is_host(&core.did, &core.dev), l.state.is_host(&sess.did, &sess.dev)))
            .unwrap_or_default();
        let mut out: Vec<Vec<u8>> = Vec::new();
        if !sess.hosting {
            out.extend(core.store.outbox(&sess.gid).unwrap_or_default());
        }
        if we_host {
            let now = now_ms();
            if peer_host {
                out.extend(core.store.mail(&sess.gid, None, now).unwrap_or_default().into_iter().map(|(_, b)| b));
            } else if sess.hosting {
                out.extend(core.store.mail(&sess.gid, Some(&sess.did), now).unwrap_or_default().into_iter().map(|(_, b)| b));
            }
        }
        for mail in out {
            let _ = sess.tx.send(Out::Msg(GroupMsg::Mail { mail }));
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
        let gone: Vec<String> = core.groups.lock().get(gid).map(|l| l.state.removed.keys().cloned().collect()).unwrap_or_default();
        for did in gone {
            let _ = core.store.drop_dir(gid, &did);
        }
        for (s, records, vv, update) in removed {
            let _ = s.tx.send(Out::Msg(GroupMsg::Removed { records, vv, update }));
            let _ = s.tx.send(Out::Close);
            core.sessions.lock().remove(&s.id);
        }
    }

    /// Sends every session of `gid` (except `from`) what it lacks of the meta doc and `days`.
    fn group_push(&self, core: &GroupCore, gid: &str, from: Option<u64>, days: &[String]) {
        core.push(gid, from, days);
    }

    // ---- accepting (host side) ---------------------------------------------------------------

    async fn group_accept_loop(self: Arc<Self>, core: Arc<GroupCore>, ep: Endpoint) {
        while let Some(incoming) = ep.accept().await {
            if core.closed() {
                break;
            }
            let (this, core) = (self.clone(), core.clone());
            tokio::spawn(async move {
                let Ok(mut accepting) = incoming.accept() else { return };
                let Ok(Ok(alpn)) = tokio::time::timeout(HELLO_TIMEOUT, accepting.alpn()).await else { return };
                // Group calls (G6) run on the group endpoint (U2/U7): `crate::gcall::net`.
                if alpn.as_slice() == proto::gcall::GCALL_ALPN {
                    if let Err(e) = this.handle_gcall_incoming(accepting).await {
                        tracing::debug!("incoming gcall: {e}");
                    }
                    return;
                }
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
        let (auth, group, vv, days, invite, hosts, relay) = match hello {
            Some(GroupMsg::Hello { auth, group, vv, days, invite, hosts, relay }) => (auth, group, vv, days, invite, hosts, relay),
            Some(GroupMsg::Fetch { code }) => {
                // G3: someone with a short invite link asks for the invite behind it.
                self.group_fetch_session(&core, writer, reader, code).await;
                let _ = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
                return Ok(());
            }
            Some(GroupMsg::Moved { auth, group, hosts }) => {
                self.group_moved_in(&core, remote, auth, &group, &hosts);
                let _ = writer.send(&GroupMsg::Ack { doc: DocId::Meta, vv: Vec::new() }).await;
                let _ = writer.0.finish();
                let _ = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
                return Ok(());
            }
            _ => return Err(Error::Protocol("expected hello".into())),
        };
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
                let relay = relay.filter(|r| r.len() <= 300);
                let entry = DirEntry { endpoint: conn.remote_id().to_string(), seen_ms: now_ms(), relay };
                let _ = core.store.put_dir(&group, &auth.did, &auth.dev, &entry);
                // A host (or call host) that rotated shows its new address in its Hello: pass it
                // on to everyone else at once (records first, then the addresses).
                core.merge_hosts_from(&group, &hosts, None);
                let mine = EndpointAuth::new(&core.account, core.dev, *core.ep_id.lock().as_ref().map(|e| e.id()).ok_or(Error::NotStarted)?.as_bytes());
                let our_vv = core.groups.lock().get(&group).map(|l| l.doc.vv().encode()).unwrap_or_default();
                let hosts = core.host_addrs(&group);
                let our_days = core.hello_days(&group);
                writer.send(&GroupMsg::Welcome { auth: mine, vv: our_vv, days: our_days, hosts }).await?;
                let peer = Peer::new(&vv, &days)?;
                self.group_run(core, conn, writer, reader, group, auth, true, joining, peer).await
            }
        }
    }

    /// A removed member may upload only its own posts whose signed frontier predates its removal
    /// (U11 option A); anything else is refused and nothing new is sent to it. It sends one
    /// `Update` per day doc it owes and finishes; each is answered by an `Ack` of that doc.
    async fn group_removed_upload(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, did: &str, writer: &mut Writer, reader: &mut Reader) {
        while let Ok(Ok(Some(GroupMsg::Update { doc, update }))) = tokio::time::timeout(REMOVED_UPLOAD_WAIT, reader.recv()).await {
            let did_owned = did.to_string();
            let res = core.apply(gid, &doc, &update, move |state, entries, fresh| {
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
                    let vv = core.doc_vv(gid, &doc).encode();
                    let _ = writer.send(&GroupMsg::Ack { doc, vv }).await;
                    if let Some(c) = changed {
                        self.group_after(core, c, None);
                    }
                }
                Err(e) => {
                    self.log(format!("group {gid}: refused a removed member's upload: {e}"));
                    let _ = writer.send(&GroupMsg::Refused { reason: e.to_string() }).await;
                    return;
                }
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
            if core.compacted.lock().is_none_or(|t| t.elapsed() >= COMPACT_EVERY) {
                *core.compacted.lock() = Some(Instant::now());
                let (this, core) = (self.clone(), core.clone());
                self.handle.spawn_blocking(move || {
                    this.group_compact(&core);
                });
            }
            self.group_req_tick(&core);
            self.group_calls_tick(&core);
            self.group_codes_tick(&core);
            if let Err(e) = self.group_rotation_tick(&core).await {
                tracing::warn!("group endpoint rotation: {e}");
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
        let (mine, forwarded) = core.groups.lock().get(gid).map(|l| (l.doc.vv().get(&core.wid).copied().unwrap_or(0), !l.meta.owed_days.is_empty() || !l.meta.own_owed_meta.is_empty())).unwrap_or((0, false));
        if mine == 0 && !forwarded {
            return false;
        }
        forwarded || !core.store.acks(gid).unwrap_or_default().iter().any(|(_, vv)| decode_vv(vv).ok().and_then(|v| v.get(&core.wid).copied()).unwrap_or(0) >= mine)
    }

    async fn group_dial(self: Arc<Self>, core: Arc<GroupCore>, gid: String, host: HostAddr) -> Result<(), Error> {
        // Not `group_ensure` (the dial loop is started by it): the endpoint is up while it runs.
        let ep = core.ep_id.lock().clone().ok_or(Error::NotStarted)?;
        if injected(&self.group.fail_dials) {
            return Err(Error::Net("connection lost (injected)".into()));
        }
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
        let days = core.hello_days(&gid);
        writer.send(&GroupMsg::Hello { auth, group: gid.clone(), vv: vv.encode(), days, invite, hosts: core.host_addrs(&gid), relay: relay_of(&ep) }).await?;
        let reply = tokio::time::timeout(HELLO_TIMEOUT, reader.recv()).await.map_err(|_| Error::Timeout)??;
        match reply {
            Some(GroupMsg::Welcome { auth, vv, days, hosts }) => {
                if !auth.verify(conn.remote_id().as_bytes()) || auth.did != host.did || auth.dev != host.dev {
                    return Err(Error::Protocol("host proof does not verify".into()));
                }
                core.refusals.lock().remove(&gid);
                core.merge_hosts_from(&gid, &hosts, None);
                let peer = Peer::new(&vv, &days)?;
                self.group_run(core, conn, writer, reader, gid, auth, false, false, peer).await
            }
            Some(GroupMsg::Removed { records, update, .. }) => {
                if let Some(u) = update {
                    match core.apply(&gid, &DocId::Meta, &u, |_, _, _| Ok(())) {
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
                // Upload the posts of ours the host lacks (one update per day doc); it keeps
                // only the pre-removal ones.
                let owed: Vec<String> = core.groups.lock().get(&gid).map(|l| l.meta.owed_days.iter().cloned().collect()).unwrap_or_default();
                for day in owed {
                    let up = {
                        let mut groups = core.groups.lock();
                        let Some(live) = groups.get_mut(&gid) else { break };
                        let Live { state, shards, .. } = live;
                        shards.get(&core.store, &gid, core.wid, &day, state).ok().and_then(|s| s.doc.export_peer_since(core.wid, &VersionVector::default()))
                    };
                    let Some(update) = up else { continue };
                    let doc = DocId::Day(day.clone());
                    writer.send(&GroupMsg::Update { doc, update }).await?;
                    match tokio::time::timeout(Duration::from_secs(10), reader.recv()).await {
                        Ok(Ok(Some(GroupMsg::Ack { vv, .. }))) => self.group_acked(&core, &gid, &host.did, &host.dev, &DocId::Day(day), &vv),
                        _ => break,
                    }
                }
                let _ = writer.0.finish();
                conn.close(0u32.into(), b"removed");
                Ok(())
            }
            Some(GroupMsg::Refused { reason }) => {
                core.refusals.lock().insert(gid.clone(), reason.clone());
                conn.close(0u32.into(), b"refused");
                // Learned from an own device (stage 2) and refused before we ever held it (the
                // account left meanwhile, or a stale list): forget it again.
                let stale = core.groups.lock().get(&gid).is_some_and(|l| l.meta.from_own && l.state.owner.is_none() && reason == "not a member");
                if stale {
                    core.groups.lock().remove(&gid);
                    let _ = core.store.forget(&gid);
                    self.search_forget(&crate::search::api::group_src(&gid));
                }
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
        theirs: Peer,
    ) -> Result<(), Error> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let id = core.next.fetch_add(1, Ordering::SeqCst);
        let sess = Arc::new(Sess {
            id,
            gid: gid.clone(),
            did: peer.did.clone(),
            dev: peer.dev,
            hosting,
            joining,
            tx,
            peer_vv: Mutex::new(theirs.vv),
            peer_days: Mutex::new(theirs.days),
            inv_done: AtomicBool::new(false),
        });
        core.sessions.lock().insert(id, sess.clone());
        self.log(format!("group {} session with {} ({})", &gid[..8.min(gid.len())], &peer.did, if hosting { "we host" } else { "they host" }));
        // Catch the other side up: the meta doc, then the recent days (older ones are history).
        self.group_push(&core, &gid, None, &core.held_recent_days(&gid));
        // GF: the host addresses again, after the records. Those in Hello/Welcome are merged
        // before this session's records arrive, so an address of a host the other side does not
        // yet know as one (e.g. accepted while we were apart) was dropped there and never re-sent;
        // here it follows the records that make it valid on the same ordered stream.
        let hosts = core.host_addrs(&gid);
        if !hosts.is_empty() {
            let _ = sess.tx.send(Out::Msg(GroupMsg::Hosts { hosts }));
        }
        self.group_mail_on_session(&core, &sess);
        self.group_calls_on_session(&core, &sess);
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
        // Both sides exchange the day docs the other lacks, newest first, in the background.
        let backfill = tokio::spawn(self.clone().group_days_sync(core.clone(), sess.clone()));
        let res = self.group_read(&core, &sess, &mut reader).await;
        core.sessions.lock().remove(&id);
        backfill.abort();
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
                GroupMsg::Update { doc, update } => {
                    let res = core.apply(&sess.gid, &doc, &update, sess.admit());
                    match res {
                        Ok(changed) => {
                            // The peer holds at least what it just sent.
                            if let Ok(meta) = loro::LoroDoc::decode_import_blob_meta(&update, false) {
                                sess.peer_has(&doc, &meta.partial_end_vv);
                            }
                            let vv = changed.as_ref().map(|c| c.doc_vv.clone()).unwrap_or_else(|| core.doc_vv(&sess.gid, &doc).encode());
                            let _ = sess.tx.send(Out::Msg(GroupMsg::Ack { doc: doc.clone(), vv }));
                            if let Some(mut c) = changed {
                                if doc == DocId::Meta {
                                    self.group_maybe_join(core, &sess.gid);
                                }
                                // Older than the days a session pushes live: history catching up.
                                c.history = doc.day().is_some_and(|d| !recent_days(now_ms() as i64, HELLO_DAYS).iter().any(|r| r == d));
                                self.group_after(core, c, Some(sess.id));
                            }
                        }
                        Err(Reject::Persistence(reason)) => {
                            // Retryable storage failure, not a membership refusal.
                            // group_run closes this session; no Ack or fan-out.
                            return Err(Error::Io(reason));
                        }
                        Err(e) => {
                            self.log(format!("group {}: refused an update from {}: {e}", &sess.gid[..8.min(sess.gid.len())], sess.did));
                            let _ = sess.tx.send(Out::Msg(GroupMsg::Refused { reason: e.to_string() }));
                            let _ = sess.tx.send(Out::Close);
                            return Err(Error::Rejected(e.to_string()));
                        }
                    }
                }
                GroupMsg::Ack { doc, vv } => {
                    if !doc.is_valid() {
                        return Err(Error::Protocol("ack for a bad doc".into()));
                    }
                    sess.peer_has(&doc, &decode_vv(&vv)?);
                    if !sess.hosting {
                        self.group_acked(core, &sess.gid, &sess.did, &sess.dev, &doc, &vv);
                        if let Some(ev) = self.group_ev() {
                            let g = sess.gid.clone();
                            self.handle.spawn_blocking(move || ev.on_group_changed(g));
                        }
                    }
                }
                GroupMsg::Days { days, .. } => {
                    if days.len() > DAYS_PAGE {
                        return Err(Error::Protocol("too many days in a page".into()));
                    }
                    if days.is_empty() {
                        sess.inv_done.store(true, Ordering::SeqCst);
                    }
                    for (d, v) in days {
                        if !DocId::Day(d.clone()).is_valid() {
                            return Err(Error::Protocol("bad day in inventory".into()));
                        }
                        sess.peer_has(&DocId::Day(d), &decode_vv(&v)?);
                    }
                }
                GroupMsg::Hosts { hosts } => {
                    core.merge_hosts_from(&sess.gid, &hosts, Some(sess.id));
                }
                GroupMsg::Refused { reason } => {
                    return Err(Error::Rejected(reason));
                }
                GroupMsg::Mail { mail } => {
                    self.group_mail_intake(core, &sess.gid, &mail, Some(sess));
                }
                GroupMsg::MailAck { id } => {
                    if !sess.hosting {
                        let _ = core.store.drop_out(&sess.gid, &super::mail::id_hex(&id));
                    }
                }
                m @ (GroupMsg::CallHostStatus { .. }
                | GroupMsg::CallStart { .. }
                | GroupMsg::CallEnd { .. }
                | GroupMsg::CallHostRequest { .. }
                | GroupMsg::CallHostRefusal { .. }) => {
                    self.group_call_msg(core, &sess.gid, m, Some(sess));
                }
                m @ (GroupMsg::Code { .. } | GroupMsg::RevokeCode { .. }) if sess.hosting => {
                    // G3: a member has us keep (or forget) an invite code; members only.
                    let ack = self.group_code_in(core, &sess.gid, &sess.did, m);
                    let _ = sess.tx.send(Out::Msg(ack));
                }
                GroupMsg::CodeAck { code, ok, reason } if !sess.hosting => {
                    self.group_code_ack_in(core, code, ok, reason);
                }
                _ => return Err(Error::Protocol("unexpected group frame".into())),
            }
        }
    }

    // ---- endpoint rotation (U12) ---------------------------------------------------------------

    /// When this device's group endpoint is due to rotate: the earliest trigger (removal or
    /// host revocation) in a group we host that happened after the key was made, plus the
    /// delay. `None` if nothing is pending.
    pub(crate) fn group_rotation_due(&self, core: &GroupCore) -> Option<u64> {
        let made = core.store.endpoint_made();
        let delay = self.group.rotation_delay();
        core.groups
            .lock()
            .values()
            .filter(|l| hosts_or_calls(&l.state, &core.did, &core.dev))
            .flat_map(|l| {
                // A call host that does not host chat moves half a delay after the chat hosts:
                // it learns their new endpoints through their `Moved` at its old one, then
                // shows its own new address in its next Hello (GA-I1). Both at once would leave
                // each knowing only the other's closed endpoint.
                let extra = if l.state.is_host(&core.did, &core.dev) { 0 } else { delay / 2 };
                l.state.rotation_triggers.iter().filter(|t| **t > 0 && **t as u64 > made).map(move |t| *t as u64 + delay + extra).collect::<Vec<_>>()
            })
            .min()
    }

    async fn group_rotation_tick(self: &Arc<Self>, core: &Arc<GroupCore>) -> Result<(), Error> {
        let Some(due) = self.group_rotation_due(core) else { return Ok(()) };
        if due > now_ms() || core.ep_id.lock().is_none() {
            return Ok(());
        }
        // G6 §5: not while a group call runs on this endpoint (it would cut it), for at most
        // an hour past the deadline.
        if self.gcall.busy() && now_ms() < due + CALL_ROTATION_DEFER_MS {
            return Ok(());
        }
        self.group_rotate(core).await
    }

    /// New group endpoint key; the old endpoint closes (every session with it ends, and whoever
    /// knew only it, a removed member, can no longer reach us). Then we tell the members we
    /// host where we are now, and dial the other hosts from the new endpoint.
    pub(crate) async fn group_rotate(self: &Arc<Self>, core: &Arc<GroupCore>) -> Result<(), Error> {
        core.store.rotate_endpoint_secret(now_ms())?;
        let old = core.ep.lock().await.take();
        *core.ep_id.lock() = None;
        {
            let mut ss = core.sessions.lock();
            for s in ss.values() {
                let _ = s.tx.send(Out::Close);
            }
            ss.clear();
        }
        core.my_addr.lock().clear();
        if let Some(ep) = old {
            let _ = tokio::time::timeout(Duration::from_secs(3), ep.close()).await;
        }
        let ep = self.group_bind(core).await?;
        self.group_relay_ready().await;
        self.log(format!("group endpoint rotated to {}", ep.id()));
        let hosted: Vec<(String, bool)> =
            core.groups.lock().iter().filter(|(_, l)| hosts_or_calls(&l.state, &core.did, &core.dev)).map(|(g, l)| (g.clone(), l.state.is_host(&core.did, &core.dev))).collect();
        for (gid, chat_host) in hosted {
            let _ = self.group_my_addr(core, &gid);
            // A call host that does not host chat has no member directory: its new address
            // reaches the members through the chat hosts (its next Hello, relayed as `Hosts`).
            if chat_host && core.moving.lock().insert(gid.clone()) {
                let (this, core) = (self.clone(), core.clone());
                self.handle.spawn(async move { this.group_tell_moved(core, gid).await });
            }
        }
        core.backoff.lock().clear();
        core.kick.notify_waiters();
        Ok(())
    }

    /// Dials every current member device in our directory that has no session with us yet and
    /// hands it our new address (`Moved`), with backoff, until all are back, the endpoint
    /// changes again, or the delay period has passed (PROVISIONAL: members offline longer must
    /// get a fresh token).
    async fn group_tell_moved(self: Arc<Self>, core: Arc<GroupCore>, gid: String) {
        let Some(ep) = core.ep_id.lock().clone() else {
            core.moving.lock().remove(&gid);
            return;
        };
        let end = Instant::now() + Duration::from_millis(self.group.moved_window());
        let mut wait = 2u64;
        loop {
            let same = core.ep_id.lock().as_ref().is_some_and(|e| e.id() == ep.id());
            if core.closed() || !same || Instant::now() > end {
                break;
            }
            let targets = {
                let groups = core.groups.lock();
                let Some(l) = groups.get(&gid) else { break };
                if !l.state.is_host(&core.did, &core.dev) {
                    break;
                }
                let dir = core.store.dir(&gid).unwrap_or_default();
                let t: Vec<(String, DirEntry)> = dir
                    .into_iter()
                    .filter(|(k, _)| {
                        let Some((did, dev)) = k.rsplit_once('/') else { return false };
                        let Some(dev) = hex::decode(dev).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()) else { return false };
                        l.state.is_member(did) && !l.state.is_removed(did) && !(did == core.did && dev == core.dev) && !core.has_session(&gid, did, &dev)
                    })
                    .collect();
                t
            };
            if targets.is_empty() {
                break;
            }
            let hosts = core.host_addrs(&gid);
            let auth = EndpointAuth::new(&core.account, core.dev, *ep.id().as_bytes());
            for (_, d) in targets {
                let Ok(key) = d.endpoint.parse::<iroh::PublicKey>() else { continue };
                let Ok(addr) = addr_for(key.as_bytes(), d.relay.as_deref()) else { continue };
                let (ep, auth, gid, hosts) = (ep.clone(), auth.clone(), gid.clone(), hosts.clone());
                tokio::spawn(async move {
                    let res: Result<(), Error> = async {
                        let conn = tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, GROUP_ALPN)).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?;
                        let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
                        let mut w = Writer(send);
                        let mut r = Reader::new(recv);
                        w.send(&GroupMsg::Moved { auth, group: gid, hosts }).await?;
                        let _ = tokio::time::timeout(HELLO_TIMEOUT, r.recv()).await;
                        conn.close(0u32.into(), b"moved");
                        Ok(())
                    }
                    .await;
                    if let Err(e) = res {
                        tracing::debug!("telling a member we moved: {e}");
                    }
                });
            }
            tokio::time::sleep(Duration::from_secs(wait)).await;
            wait = (wait * 2).min(30);
        }
        core.moving.lock().remove(&gid);
    }

    /// Member side: a host says it moved. Kept only from an active host proving its endpoint.
    fn group_moved_in(&self, core: &Arc<GroupCore>, remote: [u8; 32], auth: EndpointAuth, gid: &str, hosts: &[SignedHostAddr]) {
        if !auth.verify(&remote) {
            return;
        }
        let ok = core.groups.lock().get(gid).is_some_and(|l| l.state.is_host(&auth.did, &auth.dev));
        if !ok {
            return;
        }
        let mine = hosts.iter().any(|s| s.open().is_some_and(|a| a.did == auth.did && a.dev == auth.dev && a.endpoint == remote));
        if !mine {
            return;
        }
        core.merge_hosts_from(gid, hosts, None);
        core.backoff.lock().remove(&(gid.to_string(), host_key(&auth.did, &auth.dev)));
        self.log(format!("group {}: host {} moved", &gid[..8.min(gid.len())], auth.did));
        core.kick.notify_waiters();
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
