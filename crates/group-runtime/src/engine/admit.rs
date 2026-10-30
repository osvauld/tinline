//! What a host takes from a session (`Sess::admit`, asked by `GroupCore::apply` with the batch's
//! new entries before anything is imported). Every device verifies records on arrival and judges
//! them by the derived rules (`group_core::state`); this is the extra, per-session policy of a
//! host, for what the derived rules cannot see: claims a member makes about its own past.
//!
//! - A session we dialled (`hosting == false`): the peer is a host; nothing extra.
//! - A joiner (invite or open link) sends only its own records until it is a member; an open
//!   joiner is refused once the group is closed before it is one.
//! - Host-to-host sync (the peer device hosts now) is exempt from the rest, so one late record
//!   cannot make a host refuse a whole batch from another host.
//! - Public groups (`docs/design/public-group.md` §1, §2), from member sessions only:
//!   - An `OpenJoin` is taken only while the group is open now and only from its own author: a
//!     member cannot add identities to a closed (or once-open) group by uploading throwaway
//!     DIDs' `OpenJoin`s whose author-claimed `seen` names an old `Open{true}`.
//!   - While admins-only is on now, a post by a non-admin author is refused unless the derived
//!     rules hide it anyway, or the uploading member is itself an admin (admins may post, so
//!     carrying another's post adds nothing). `seen` is the author's claim: a stale frontier
//!     would otherwise make a post written after `AdminsOnly{true}` show on every device.
//!     An honest offline post written concurrently with the switch is refused too. The host
//!     answers `Refused{ADMINS_ONLY}` and keeps the session (the batch is not imported).
//!
//! The member's side of the same rule (`fail_refused_posts`, `upload_cut`): once it knows
//! admins-only is on and it is not an admin (from the meta doc, or a host's `Refused`), its own
//! unacknowledged posts a host would refuse are marked failed (`GroupMeta::failed_posts`,
//! reason `NOT_POSTED`) and never uploaded: its uploads to hosts leave out every post the rule
//! refuses and every failed one, with everything after them under the same writer.

use super::super::code::NOT_OPEN;
use super::super::shard::PostRow;
use super::super::state::refused_admins_only;
use super::super::store::FailedPost;
use super::*;

/// Refusal reason for a non-admin post while admins-only is on.
pub const ADMINS_ONLY: &str = "admins only";

/// What the author of a post marked failed under admins-only sees.
pub const NOT_POSTED: &str = "Not posted: only admins post here";

/// The session's side of the policy: who the peer is and how it came in.
#[derive(Debug, Clone)]
pub struct SessPolicy {
    pub did: String,
    pub dev: [u8; 32],
    /// We accepted the session: the peer is a member (or joiner) we host, or another host.
    pub hosting: bool,
    pub joining: bool,
    pub open_join: bool,
}

impl SessPolicy {
    /// Whether the batch `fresh` (records new to us) may be imported, judged against `state`
    /// (the group's derived state now).
    pub fn admit(&self, state: &GroupState, fresh: &[Entry]) -> Result<(), String> {
        if !self.hosting {
            return Ok(());
        }
        let did = self.did.as_str();
        let member = state.is_member(did);
        if self.joining && !member && fresh.iter().any(|e| e.rec.author != did) {
            return Err("a joiner may send only its own records".into());
        }
        if self.open_join && !state.open && !member {
            return Err(NOT_OPEN.into());
        }
        if state.is_host(did, &self.dev) {
            return Ok(());
        }
        for e in fresh {
            match &e.rec.body {
                Body::OpenJoin { .. } if !state.open => return Err(NOT_OPEN.into()),
                Body::OpenJoin { .. } if e.rec.author != did => {
                    return Err("an open join must come from its own author".into());
                }
                Body::Post { .. } | Body::Voice { .. } | Body::File { .. } if refused_admins_only(state, did, &e.rec.author, &e.rec.seen) => {
                    return Err(ADMINS_ONLY.into());
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// The ops of day doc `day` that we (`me`, a member: not a host) must not upload to a host
/// holding `theirs` (`GroupDoc::export_since_before`): our failed posts and every post there
/// the host would refuse (`refused_admins_only`).
pub fn upload_cut(state: &GroupState, meta: &GroupMeta, me: &str, day: &str, rows: &[PostRow], theirs: &VersionVector) -> Vec<(u64, i32)> {
    let mut cut: Vec<(u64, i32)> = meta.failed_posts.values().filter(|f| f.day == day).map(|f| (f.peer, f.counter)).collect();
    if state.admins_only && !state.is_admin(me) {
        for r in rows {
            let e = &r.entry;
            // Only posts are refused in a channel (a file holder announcement is not a post).
            if e.rec.body.is_post() && theirs.get(&e.peer).copied().unwrap_or(0) <= e.counter && refused_admins_only(state, me, &e.rec.author, &e.rec.seen) {
                cut.push((e.peer, e.counter));
            }
        }
    }
    cut
}

impl GroupCore {
    /// Marks failed (reason `NOT_POSTED`) our own posts of `gid` that no host acknowledged and
    /// a host would refuse: by our state now, or, `told` (a host answered `Refused{ADMINS_ONLY}`,
    /// maybe before we hold the switch), every one the rules would show. A day whose posts of
    /// ours left unacknowledged are all failed or refused is no longer owed to a host. True if
    /// anything changed (persisted).
    pub fn fail_refused_posts(&self, gid: &str, told: bool) -> bool {
        let mut groups = self.groups.lock();
        let Some(live) = groups.get_mut(gid) else { return false };
        let Live { state, shards, meta, .. } = live;
        if state.owner.is_none() || state.is_host(&self.did, &self.dev) || (!told && (!state.admins_only || state.is_admin(&self.did))) {
            return false;
        }
        let mut next = meta.clone();
        for day in &meta.owed_days {
            let acks: Vec<VersionVector> = self.store.acks_day(gid, day).or_log_default("reading the day acks").iter().filter_map(|(_, v)| decode_vv(v).ok()).collect();
            let Ok(shard) = shards.get(&self.store, gid, self.wid, day, state) else { continue };
            let mut owed = false;
            for r in &shard.rows {
                let e = &r.entry;
                if e.rec.author != self.did || acks.iter().any(|vv| vv.get(&e.peer).copied().unwrap_or(0) > e.counter) {
                    continue;
                }
                // Only posts can be refused as posts; a file holder announcement is just owed.
                let refused = e.rec.body.is_post() && if told { post_visible(state, &e.rec.author, &e.rec.seen) != Vis::Hidden } else { refused_admins_only(state, &self.did, &e.rec.author, &e.rec.seen) };
                if e.peer == self.wid && (refused || next.failed_posts.contains_key(&e.id)) {
                    next.failed_posts.entry(e.id.clone()).or_insert_with(|| FailedPost { day: day.clone(), peer: e.peer, counter: e.counter, reason: NOT_POSTED.into() });
                } else if !refused {
                    owed = true;
                }
            }
            if !owed {
                next.owed_days.remove(day);
            }
        }
        if next.failed_posts == meta.failed_posts && next.owed_days == meta.owed_days {
            return false;
        }
        if self.store.put_meta(gid, &next).is_err() {
            return false;
        }
        *meta = next;
        true
    }
}

impl GroupRt {
    /// `fail_refused_posts`, telling the app when a post of ours turned failed.
    pub(super) fn group_fail_refused(&self, core: &GroupCore, gid: &str, told: bool) {
        if core.fail_refused_posts(gid, told)
            && let Some(ev) = self.group_ev()
        {
            let g = gid.to_string();
            self.handle.spawn_blocking(move || ev.on_group_changed(g));
        }
    }
}

#[cfg(test)]
mod tests;
