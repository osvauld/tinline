//! What a group's meta records mean: owner, members, admins, hosts, flags, removals; and, per
//! post, whether it shows. `derive_meta` is a pure function of the meta record set and its oplog
//! positions and `post_visible` of that state and the post's own `seen`, so every device that
//! holds the same records reaches the same verdicts in every delivery order. Posts live in day
//! docs (`docs/group-shards.md`); their `seen` is the author's meta version vector.
//!
//! Rules (decisions in `rotation-open-decisions.md` §0; PROVISIONAL ones marked):
//! - Genesis: written by the owner; `group_id(owner, nonce)` must be the group.
//! - Admins: the owner, plus members the owner made admin (`Admin`, owner only; the latest by
//!   `(at, id)` per member wins). "Admin as of a frontier" counts only the owner's `Admin`
//!   records inside that frontier.
//! - Members: the owner, plus every DID with a `Join` whose invite verifies, names this group
//!   and was issued by someone who was owner/admin as of the join's frontier. `Leave` ends it
//!   (latest of join/leave by `(at, id)`).
//! - Flags (U9/U15): a `Flag` counts if its author was admin as of the flag's frontier; any 3
//!   counted flags on a member remove it (the same admin may flag 3 times). PROVISIONAL: the
//!   owner cannot be flagged out (§11 Q3); a removed admin's earlier flags still count (§11 Q9).
//!   Removal is final for this slice (readmission is a later slice).
//! - Posts (U11 option A, signed frontier): a post shows if its author was a member as of its
//!   frontier and fewer than 3 counted flags on the author are inside its frontier. A removed
//!   member's honest offline post (frontier from before the removal) therefore shows, in every
//!   delivery order; anything written after it saw its own removal does not. A post whose
//!   frontier names meta records we do not hold yet is pending (hidden, judged again when the
//!   meta doc advances).
//! - Hosts: device `dev` of `did` hosts when the latest `HostPermit` for it (by an owner/admin
//!   as of the permit's frontier) is active, the device's own latest `HostAccept` is active,
//!   and `did` is a member (not removed).
//! - Call hosts (U21, G6 §2.1): the same rule over the separate `CallHostPermit` /
//!   `CallHostAccept` records. Chat hosting never implies call hosting, nor the reverse.
//! - Public groups (`docs/design/public-group.md`): `Open` and `AdminsOnly` count when written
//!   by an admin as of their frontier. The setting as of a frontier is causal (`in_force`): of
//!   the counted records of that kind inside the frontier, a record whose own `seen` includes
//!   another supersedes it; of those nobody inside the frontier superseded (concurrent ones),
//!   the latest by `(at, id)` wins. So an author-chosen `at` only breaks ties between
//!   concurrent records, and any admin who saw a setting can change it. An `OpenJoin` is a join
//!   when the group was open as of its frontier. A post by a non-admin (as of its frontier) is
//!   hidden if admins-only was on as of it.

use std::collections::{BTreeMap, BTreeSet};


use super::doc::{Entry, seen_has};
use super::record::{Body, group_id};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberInfo {
    pub did: String,
    /// The name the member gave when joining (the owner's: in genesis).
    pub name: String,
    pub joined_at: i64,
    pub invited_by: Option<String>,
    /// Every device ID this DID has written with (for "make host: pick one of their devices").
    pub devices: BTreeSet<[u8; 32]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagInfo {
    pub by: String,
    pub at: i64,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    pub did: String,
    pub dev: [u8; 32],
    pub appointed_by: String,
    pub at: i64,
}

/// A device that may host group calls (or was asked to): `permit` is the id of the admin's
/// latest `CallHostPermit` for it, the `permit_digest` of its `gcall::HostRef`, so a re-issued
/// permit is a new reference and a revoked one stops verifying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallHostInfo {
    pub did: String,
    pub dev: [u8; 32],
    pub appointed_by: String,
    pub at: i64,
    pub permit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    Post,
    Joined,
    Left,
    Removed,
    AdminGranted,
    AdminRevoked,
    HostAdded,
    HostRemoved,
}

/// One line of the conversation: a post, or a system line derived from an op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub kind: ItemKind,
    pub author: String,
    /// Who the line is about (joined/left/removed/admin/host), or the post's author.
    pub subject: String,
    pub at: i64,
    pub text: String,
    pub reply_to: Option<String>,
    /// A post shown although its author is removed now: written before the removal (U11).
    pub before_removal: bool,
    pub peer: u64,
    pub counter: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupState {
    pub name: String,
    pub owner: Option<String>,
    pub members: BTreeMap<String, MemberInfo>,
    pub admins: BTreeSet<String>,
    pub removed: BTreeMap<String, Vec<FlagInfo>>,
    /// Counted flags per member not (yet) removed.
    pub flags: BTreeMap<String, Vec<FlagInfo>>,
    pub left: BTreeSet<String>,
    /// Each member's published encryption key (`Body::EncKey`, latest by `(at, id)`).
    pub enc_keys: BTreeMap<String, [u8; 32]>,
    pub hosts: Vec<HostInfo>,
    /// Devices that accepted hosting but hold no active permit.
    pub host_offers: Vec<(String, [u8; 32])>,
    /// Devices an admin appointed (active permit) that have not accepted (yet), with who
    /// appointed them: the "host this group?" prompt on that device.
    pub host_requests: Vec<HostInfo>,
    /// Devices that host group calls: an active admin `CallHostPermit`, their own active
    /// `CallHostAccept`, and a member DID (U21; separate from `hosts`).
    pub call_hosts: Vec<CallHostInfo>,
    /// Devices an admin allowed to host calls that have not accepted (yet).
    pub call_host_requests: Vec<CallHostInfo>,
    /// Anyone may join with `OpenJoin` (the latest admin `Open` record is active).
    pub open: bool,
    /// Only the owner and admins post (a channel).
    pub admins_only: bool,
    /// System lines (not posts: those are judged per post, `post_visible`), ordered by `(at, id)`.
    pub items: Vec<Item>,
    /// What `post_visible` needs: per-DID join/leave and flag positions.
    pub pos: Positions,
}

/// Per-DID positions in the meta doc, so a post is judged against its `seen` in
/// `O(records about its author)` without re-deriving anything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Positions {
    /// The meta doc's version vector at derivation: the ops we hold.
    have: BTreeMap<u64, i32>,
    owner: Option<String>,
    /// Valid joins (`true`) and leaves of each non-owner DID by `(peer, counter)`, in `(at, id)` order.
    events: BTreeMap<String, Vec<(u64, i32, bool)>>,
    /// Counted flags on each DID by `(peer, counter)`.
    flags: BTreeMap<String, Vec<(u64, i32)>>,
    /// Admin `AdminsOnly` records, in `(at, id)` order.
    admins_only: Vec<Setting>,
    /// The owner's `Admin` records per member `(peer, counter, admin)`, in `(at, id)` order.
    admin_events: BTreeMap<String, Vec<(u64, i32, bool)>>,
}

/// One counted `Open` / `AdminsOnly` record: its position, value and own frontier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Setting {
    peer: u64,
    counter: i32,
    on: bool,
    seen: Vec<(u64, i32)>,
}

impl Setting {
    fn of(e: &Entry, on: bool) -> Self {
        Self { peer: e.peer, counter: e.counter, on, seen: e.rec.seen.clone() }
    }
}

/// The value of a setting as of `seen` (`None`: every record held), from its counted records
/// in `(at, id)` order (the rule of the module doc): the records inside the frontier that no
/// other record inside it supersedes (whose `seen` includes them), and of those the latest by
/// `(at, id)`. Off when there is none. Pure in the record set, so every delivery order agrees.
fn in_force(recs: &[Setting], seen: Option<&[(u64, i32)]>) -> bool {
    let inside = |s: &Setting| seen.is_none_or(|f| seen_has(f, s.peer, s.counter));
    recs.iter()
        .enumerate()
        .rev()
        .filter(|(_, s)| inside(s))
        .find(|(i, s)| !recs.iter().enumerate().any(|(j, t)| j != *i && inside(t) && seen_has(&t.seen, s.peer, s.counter)))
        .is_some_and(|(_, s)| s.on)
}

/// Whether a post shows (`post_visible`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vis {
    Visible,
    /// Its author was not a member, or was removed, as of its frontier.
    Hidden,
    /// Its frontier names meta records we do not hold yet.
    Pending,
}

/// Judges one post by its author and signed frontier against the derived meta state (U11
/// option A, the rule of the module doc).
pub fn post_visible(st: &GroupState, author: &str, seen: &[(u64, i32)]) -> Vis {
    let p = &st.pos;
    let Some(owner) = &p.owner else { return Vis::Pending };
    if seen.iter().any(|(peer, c)| p.have.get(peer).copied().unwrap_or(0) < *c) {
        return Vis::Pending;
    }
    let member = author == owner
        || p.events.get(author).and_then(|evs| evs.iter().rev().find(|(peer, c, _)| seen_has(seen, *peer, *c))).is_some_and(|(_, _, join)| *join);
    let flagged = p.flags.get(author).is_some_and(|f| f.iter().filter(|(peer, c)| seen_has(seen, *peer, *c)).count() >= REMOVAL_FLAGS);
    let latest = |evs: &[(u64, i32, bool)]| evs.iter().rev().find(|(peer, c, _)| seen_has(seen, *peer, *c)).is_some_and(|(_, _, on)| *on);
    let muted = in_force(&p.admins_only, Some(seen)) && author != owner && !p.admin_events.get(author).is_some_and(|evs| latest(evs));
    if member && !flagged && !muted { Vis::Visible } else { Vis::Hidden }
}

/// The conversation line of a post; `before_removal` is its author's removal now.
pub fn post_item(e: &Entry, st: &GroupState) -> Option<Item> {
    let Body::Post { text, reply_to } = &e.rec.body else { return None };
    Some(Item {
        id: e.id.clone(),
        kind: ItemKind::Post,
        author: e.rec.author.clone(),
        subject: e.rec.author.clone(),
        at: e.rec.at,
        text: text.clone(),
        reply_to: reply_to.clone(),
        before_removal: st.removed.contains_key(&e.rec.author),
        peer: e.peer,
        counter: e.counter,
    })
}

pub const REMOVAL_FLAGS: usize = 3;

impl GroupState {
    pub fn is_member(&self, did: &str) -> bool {
        self.members.contains_key(did)
    }

    pub fn is_admin(&self, did: &str) -> bool {
        self.admins.contains(did)
    }

    pub fn is_host(&self, did: &str, dev: &[u8; 32]) -> bool {
        self.hosts.iter().any(|h| h.did == did && &h.dev == dev)
    }

    pub fn is_removed(&self, did: &str) -> bool {
        self.removed.contains_key(did)
    }

    pub fn call_host(&self, did: &str, dev: &[u8; 32]) -> Option<&CallHostInfo> {
        self.call_hosts.iter().find(|h| h.did == did && &h.dev == dev)
    }

    pub fn is_call_host(&self, did: &str, dev: &[u8; 32]) -> bool {
        self.call_host(did, dev).is_some()
    }
}

mod derive;
pub use derive::derive_meta;

#[cfg(test)]
mod tests;
