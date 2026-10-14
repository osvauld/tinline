//! Who a host relays a frame to (removal and joining enforcement for call frames): the one
//! rule, so a drift between the call relay and the mail relay cannot open a gap.
//!
//! A member (not a host) originates frames only to the hosts it uses. A host passes a frame to
//! the other hosts, and to the sessions it hosts whose member is current (a member, not
//! removed) and that the [`Route`] names. A removed or joining peer never gets a relayed frame.

use std::sync::Arc;

use super::calldir::device_slot;
use super::engine::{GroupCore, Sess};
use super::state::GroupState;

/// Where a frame goes from a host.
pub(super) enum Route {
    All,
    Device(String, u64),
    Did(String),
}

/// A peer of a session, as the rule sees it.
pub(super) struct Peer<'a> {
    pub did: &'a str,
    pub dev: &'a [u8; 32],
    /// We accepted the peer's session as its host.
    pub hosting: bool,
}

/// Whether a frame goes to `peer`. `we_host`: this device is a chat host of the group.
pub(super) fn relay_ok(st: &GroupState, we_host: bool, peer: &Peer, route: &Route) -> bool {
    if !we_host {
        return !peer.hosting;
    }
    if st.is_host(peer.did, peer.dev) {
        return true;
    }
    if !peer.hosting {
        return false;
    }
    let member = st.is_member(peer.did) && !st.is_removed(peer.did);
    member
        && match route {
            Route::All => true,
            Route::Device(did, slot) => peer.did == did && device_slot(peer.dev) == *slot,
            Route::Did(did) => peer.did == did,
        }
}

/// The sessions of `gid` a frame goes to: from a member (`from = None` is "originated here")
/// only what it originates; from a host to other hosts and to the members `route` names.
/// Never the session it came from.
pub(super) fn relay_targets(core: &GroupCore, gid: &str, from: Option<u64>, route: &Route) -> Vec<Arc<Sess>> {
    let groups = core.groups.lock();
    let Some(live) = groups.get(gid) else { return vec![] };
    let st = &live.state;
    let we_host = st.is_host(&core.did, &core.dev);
    if !we_host && from.is_some() {
        return vec![];
    }
    core.sessions_of(gid)
        .into_iter()
        .filter(|s| Some(s.id) != from)
        .filter(|s| relay_ok(st, we_host, &Peer { did: &s.did, dev: &s.dev, hosting: s.hosting }, route))
        .collect()
}

#[cfg(test)]
mod tests;
