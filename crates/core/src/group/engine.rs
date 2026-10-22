//! The group runtime: the group endpoint (U2, `tinline/group/5`), host and member sessions,
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
//!
//! Shared state and proof helpers live here. `documents` owns durable mutations; `replication`
//! owns frontiers and fan-out. `lifecycle`, `accepting`, `dialing`, `sessions`, and `rotation`
//! manage the runtime and transport; `changes` publishes committed changes and handles mail.
//! `history` and `synth` retain their backfill/compaction and synthetic-load responsibilities.

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
use crate::node::{Inner, addr_for, now_ms, relay_of};
use crate::wire::DIAL_TIMEOUT;

mod accepting;
pub(crate) mod admit;
mod changes;
mod dialing;
mod documents;
#[cfg(test)]
mod durability;
mod history;
pub(crate) mod images;
mod lifecycle;
pub(crate) mod member_receipts;
pub(crate) mod presence;
mod replication;
mod rotation;
mod sessions;
#[cfg(any(test, feature = "dev-tools"))]
mod synth;
#[cfg(test)]
pub(crate) fn receipt_cores_for_test(
    account: identity::Identity,
    source_epoch: u64,
    target_epoch: u64,
) -> (Arc<GroupCore>, Arc<GroupCore>, String, Vec<u8>) {
    durability::receipt_cores_for_test(account, source_epoch, target_epoch)
}

#[cfg(test)]
pub(crate) fn own_content_core_for_test(
    account: identity::Identity,
    epoch: u64,
    populated: bool,
) -> (Arc<GroupCore>, String, String) {
    durability::own_content_core_for_test(account, epoch, populated)
}

const HELLO_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a host waits for a removed member's pre-removal upload (U11).
const REMOVED_UPLOAD_WAIT: Duration = Duration::from_secs(10);
/// D12: a removed member may upload its pre-removal posts for this long after this host first
/// learned of the removal (local clock, `rm_seen`).
pub const REMOVED_UPLOAD_WINDOW_MS: u64 = 2 * 24 * 3600 * 1000;
const TICK: Duration = Duration::from_secs(2);
const MAX_HELLO_DAYS: usize = 64;
const COMPACT_EVERY: Duration = Duration::from_secs(3600);
const BACKOFF: [u64; 6] = [1, 2, 5, 15, 30, 60];
pub const INVITE_TTL: u64 = 7 * 24 * 3600 * 1000;

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
    /// Test knob: a rotation policy used instead of the stored one, unchecked (seconds-long
    /// intervals), in memory only.
    #[cfg(any(test, feature = "dev-tools"))]
    pub(crate) rotation_policy_for_test: Mutex<Option<super::rotation::RotationPolicy>>,
    /// Test knob: the removed-upload window, ms, instead of `REMOVED_UPLOAD_WINDOW_MS`.
    #[cfg(any(test, feature = "dev-tools"))]
    pub(crate) removed_upload_window_for_test: Mutex<Option<u64>>,
    /// Test knob: presence `(every_ms, window_ms)` instead of 15 s / 60 s (public group §3).
    #[cfg(any(test, feature = "dev-tools"))]
    pub(crate) presence_timing_for_test: Mutex<Option<(u64, u64)>>,
    /// Group calls over this runtime (GA-I1, `calls.rs`).
    pub(crate) calls: super::calls::GroupCalls,
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

/// G6 §5 / Q12: a due rotation waits for a running group call, at most this long.
pub use node_runtime::CALL_ROTATION_DEFER_MS;
/// After a restart inside a rotation's announce window, members that were online have this
/// long to come back on their own before the `Moved` campaign resumes dialing them.
const RESUME_ANNOUNCE_WAIT: Duration = Duration::from_secs(10);

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
        Self {
            doc: GroupDoc::new(gid, wid),
            entries: Vec::new(),
            state: Default::default(),
            meta,
            shards: Shards::default(),
        }
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
    /// A rotation is running (the dial loop's tick or `group_rotate_now`): one at a time.
    rotating: AtomicBool,
    /// The endpoint whose `Moved` campaign is running (until the persisted `ep/announce`).
    announcing: Mutex<Option<iroh::PublicKey>>,
    /// When day docs were last compacted (hourly, and once after open).
    compacted: Mutex<Option<Instant>>,
    /// Held for a whole compaction run, so the hourly one and a forced one never overlap.
    compacting: Mutex<()>,
    /// G3: invite codes (waiters for host answers, the wrong-code limiter).
    pub(crate) codes: super::code::CodeRt,
    /// Member message receipts over this store (`member_receipts`), signed as `did`/`dev`.
    pub(crate) receipts: Arc<crate::receipts::store::messages::MessageReceipts>,
    /// Who is online in open groups (public group §3): in memory only.
    pub(crate) presence: presence::PresenceRt,
    /// Image blobs of posts (public group §5).
    pub(crate) images: images::ImagesRt,
}

pub(crate) struct Sess {
    pub(crate) id: u64,
    pub(crate) gid: String,
    pub(crate) did: String,
    pub(crate) dev: [u8; 32],
    /// The peer's group endpoint (the connection's remote): what the image gate knows it by.
    pub(crate) ep: [u8; 32],
    /// We accepted it: we host this session's member.
    pub(crate) hosting: bool,
    /// Host side: the member was joining (not yet a member) at Hello.
    joining: bool,
    /// Host side: that join is an open one (no invite, public group §2): refused once the group
    /// closes before the joiner is a member.
    open_join: bool,
    /// Host side: the member's `Hello` said "show me as online" (public group §3).
    show: bool,
    pub(crate) tx: mpsc::UnboundedSender<Out>,
    /// What the peer holds of the meta doc.
    peer_vv: Mutex<VersionVector>,
    /// What the peer holds of the day docs we know it holds some of.
    peer_days: Mutex<HashMap<String, VersionVector>>,
    /// The peer's inventory of day docs is complete (`GroupMsg::Days`, the empty page).
    inv_done: AtomicBool,
    /// What the peer holds of each day's member receipt ledger (its `ReceiptHello`/`ReceiptAck`,
    /// plus what we sent since).
    receipt_vv: Mutex<HashMap<String, VersionVector>>,
    /// Days whose receipts we refused as pending (posts/records we lacked): asked again
    /// (`ReceiptHello`) after our docs advance.
    receipt_waiting: Mutex<std::collections::BTreeSet<String>>,
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
            DocId::Day(d) => self
                .peer_days
                .lock()
                .entry(d.clone())
                .or_default()
                .merge(vv),
        }
    }

    /// The session's policy for what the peer sends (`GroupCore::apply`'s `admit`; the rules
    /// are `admit::SessPolicy`).
    fn admit(&self) -> impl FnOnce(&GroupState, &[Entry], &[Entry]) -> Result<(), String> + use<> {
        let policy = self.policy();
        move |state, _, fresh| policy.admit(state, fresh)
    }

    pub(crate) fn policy(&self) -> admit::SessPolicy {
        admit::SessPolicy {
            did: self.did.clone(),
            dev: self.dev,
            hosting: self.hosting,
            joining: self.joining,
            open_join: self.open_join,
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
        Ok(Self {
            vv: decode_vv(vv)?,
            days: map,
        })
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

/// Device `dev` of `did` hosts the group's chat or its calls: the devices whose group endpoint
/// members may know (U7, U21).
pub(crate) fn hosts_or_calls(st: &GroupState, did: &str, dev: &[u8; 32]) -> bool {
    st.is_host(did, dev) || st.is_call_host(did, dev)
}

/// Checks a removal proof against our own DID: a genesis for `gid`, and at least 3 flags on us
/// by the owner or by DIDs the owner made admin (PROVISIONAL: the "admin as of the flag's
/// frontier" refinement is applied when the doc itself carries the flags).
pub(crate) fn proof_removes(
    gid: &str,
    me: &str,
    records: &[Vec<u8>],
) -> Option<Vec<(String, i64)>> {
    let recs: Vec<Record> = records
        .iter()
        .filter_map(|b| open_record(b, gid).ok())
        .collect();
    let owner = recs.iter().find_map(|r| match &r.body {
        Body::Genesis { nonce, .. } if group_id(&r.author, nonce) == gid => Some(r.author.clone()),
        _ => None,
    })?;
    let admins: HashSet<String> = recs
        .iter()
        .filter_map(|r| match &r.body {
            Body::Admin {
                member,
                admin: true,
            } if r.author == owner => Some(member.clone()),
            _ => None,
        })
        .collect();
    let flags: Vec<(String, i64)> = recs
        .iter()
        .filter_map(|r| match &r.body {
            Body::Flag { member }
                if member == me && (r.author == owner || admins.contains(&r.author)) =>
            {
                Some((r.author.clone(), r.at))
            }
            _ => None,
        })
        .collect();
    (flags.len() >= super::state::REMOVAL_FLAGS).then_some(flags)
}
