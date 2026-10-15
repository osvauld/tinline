//! The group-call [`CallDirectory`] over group chat's derived state (GA-I1, G6 §3.2). Read
//! only: every answer comes from the current `GroupState` (records every device verifies, U10),
//! the group store's DID-signed host addresses (newest wins, so a U12 rotation is followed) and,
//! on a chat host, its member directory (U7).
//!
//! - `is_member`: a member, not removed (3 counted flags).
//! - `host_permit_ok`: the `HostRef` names an active call host (admin `CallHostPermit` +
//!   the device's own `CallHostAccept` + a member DID) under the permit it names: the
//!   `permit_digest` is the id of that `CallHostPermit` record, `device_slot` the device ID's
//!   first 8 bytes. A revoked or re-issued permit stops verifying at once.
//! - `endpoint_ok`: the connecting group endpoint is the joiner's, by the chat host's member
//!   directory or by the DID-signed group note the Join carried (§9 G2).
//! - `host_addr`: the call host's newest DID-signed group address.
//!
//! The directory holds the node weakly and looks up the group runtime on every call, so it
//! keeps working across lock/unlock and account switches (a closed runtime knows nothing).

use std::sync::{Arc, Weak};

use iroh::EndpointAddr;
use proto::gcall::SignedNote;

use super::engine::GroupCore;
use super::state::{CallHostInfo, GroupState};
use crate::gcall::net::CallDirectory;
use crate::gcall::{CallAuthority, HostRef};
use crate::node::{Inner, addr_for, now_ms};

/// `HostRef::device_slot` of a group device ID: its first 8 bytes, big-endian.
pub fn device_slot(dev: &[u8; 32]) -> u64 {
    u64::from_be_bytes(dev[..8].try_into().expect("8 bytes"))
}

pub fn host_ref_of(h: &CallHostInfo) -> HostRef {
    HostRef { holder_did: h.did.clone(), device_slot: device_slot(&h.dev), permit_digest: h.permit.clone() }
}

/// The active call host `r` stands for in `st`, if any.
pub fn resolve<'a>(st: &'a GroupState, r: &HostRef) -> Option<&'a CallHostInfo> {
    st.call_hosts.iter().find(|h| h.did == r.holder_did && h.permit == r.permit_digest && device_slot(&h.dev) == r.device_slot)
}

pub(crate) struct GroupCallDir {
    inner: Weak<Inner>,
}

impl GroupCallDir {
    pub(crate) fn new(inner: &Arc<Inner>) -> Self {
        GroupCallDir { inner: Arc::downgrade(inner) }
    }

    fn core(&self) -> Option<Arc<GroupCore>> {
        let inner = self.inner.upgrade()?;
        let c = inner.group.core.lock().clone()?;
        (!c.closed()).then_some(c)
    }

    fn state<R>(&self, gid: &str, f: impl FnOnce(&GroupCore, &GroupState) -> R) -> Option<R> {
        let core = self.core()?;
        let groups = core.groups.lock();
        let live = groups.get(gid)?;
        Some(f(&core, &live.state))
    }
}

impl CallAuthority for GroupCallDir {
    fn is_member(&self, gid: &str, did: &str) -> bool {
        self.state(gid, |_, st| st.is_member(did) && !st.is_removed(did)).unwrap_or(false)
    }

    fn host_permit_ok(&self, gid: &str, host: &HostRef) -> bool {
        self.state(gid, |_, st| resolve(st, host).is_some()).unwrap_or(false)
    }

    fn endpoint_ok(
        &self,
        gid: &str,
        did: &str,
        note: Option<&SignedNote>,
        endpoint: &[u8; 32],
    ) -> bool {
        let Some(core) = self.core() else {
            return false;
        };
        if !self.is_member(gid, did) {
            return false;
        }
        // A chat host's member directory (U7).
        let key = iroh::PublicKey::from_bytes(endpoint).map(|k| k.to_string()).unwrap_or_default();
        let prefix = format!("{did}/");
        if core.store.dir(gid).unwrap_or_default().iter().any(|(k, d)| k.starts_with(&prefix) && d.endpoint == key) {
            return true;
        }
        // The joiner's DID-signed group note (§9 G2).
        note.and_then(|n| n.open().ok())
            .is_some_and(|n| n.did == did && &n.endpoint == endpoint)
    }
}

impl CallDirectory for GroupCallDir {
    fn host_addr(&self, host: &HostRef) -> Option<EndpointAddr> {
        let core = self.core()?;
        let found: Option<(String, [u8; 32])> = {
            let groups = core.groups.lock();
            groups.iter().find_map(|(gid, l)| resolve(&l.state, host).map(|h| (gid.clone(), h.dev)))
        };
        let (gid, dev) = found?;
        let addr = core
            .store
            .hosts(&gid)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|s| s.open().ok())
            .filter(|a| a.group == gid && a.did == host.holder_did && a.dev == dev)
            .max_by_key(|a| a.at)?;
        addr_for(&addr.endpoint, addr.relay.as_deref()).ok()
    }

    fn my_host_ref(&self, gid: &str) -> Option<HostRef> {
        let inner = self.inner.upgrade()?;
        if inner.group.calls.paused.lock().contains(gid) {
            return None;
        }
        self.state(gid, |core, st| st.call_host(&core.did, &core.dev).map(host_ref_of)).flatten()
    }

    fn my_note(&self) -> Option<SignedNote> {
        let core = self.core()?;
        let ep = core.ep_id.lock().clone()?;
        Some(proto::gcall::sign_note(&core.account, &core.dev, ep.id().as_bytes(), now_ms()))
    }
}
