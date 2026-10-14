//! Read access to a group's derived state from a [`GroupCore`], so callers do not repeat
//! `core.groups.lock().get(gid).is_some_and(..)`.

use super::engine::GroupCore;
use super::state::GroupState;

impl GroupCore {
    /// Runs `f` on the derived state of `gid` under the groups lock; `None` for an unknown group.
    pub(crate) fn with_state<R>(&self, gid: &str, f: impl FnOnce(&GroupState) -> R) -> Option<R> {
        self.groups.lock().get(gid).map(|l| f(&l.state))
    }

    /// This device is an accepted chat host of `gid` (false for an unknown group).
    pub(crate) fn am_host(&self, gid: &str) -> bool {
        self.with_state(gid, |st| st.is_host(&self.did, &self.dev)).unwrap_or(false)
    }
}
