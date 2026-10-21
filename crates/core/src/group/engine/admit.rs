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
//!     Cost (PROVISIONAL, conservative): an honest offline post written concurrently with the
//!     switch is refused too, and the session that carries it is closed each time it tries.

use super::super::code::NOT_OPEN;
use super::*;

/// Refusal reason for a non-admin post while admins-only is on.
pub(crate) const ADMINS_ONLY: &str = "admins only";

/// The session's side of the policy: who the peer is and how it came in.
#[derive(Debug, Clone)]
pub(crate) struct SessPolicy {
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
    pub(crate) fn admit(&self, state: &GroupState, fresh: &[Entry]) -> Result<(), String> {
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
                Body::Post { .. }
                    if state.admins_only
                        && !state.is_admin(&e.rec.author)
                        && !state.is_admin(did)
                        && post_visible(state, &e.rec.author, &e.rec.seen) != Vis::Hidden =>
                {
                    return Err(ADMINS_ONLY.into());
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
