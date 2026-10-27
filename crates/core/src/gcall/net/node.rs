//! `Node` entry points of the group-call service (`gcall_net`). The node's side of it (the
//! [`GcallHost`] implementation) is the group runtime's (`group_runtime::calls`).

use iroh::EndpointAddr;
use proto::gcall::SignedStart;

use super::*;
use crate::Error;

/// Group calls on a [`crate::Node`]. **Unstable** plain Rust; the app API is the group layer's
/// UniFFI calls (`group::calls`). Calls run on the group endpoint (U2/U7): the calls that start
/// a session bring it up first.
impl crate::Node {
    /// Sets the directory, event sink, own host ref and conditions. Applies to sessions
    /// started afterwards. Group chat configures itself (`GroupCallDir`) unless this was called.
    pub fn gcall_configure(&self, cfg: GcallConfig) {
        self.open().group.gcall.configure(cfg)
    }

    /// The group endpoint, bound (otherwise it is bound only once the account has a group).
    fn gcall_ensure_endpoint(&self) -> Result<(), Error> {
        self.run(|inner| async move {
            let core = inner.group.group_core()?;
            inner.group.group_ensure(&core).await.map(|_| ())
        })
    }

    /// New local hosting conditions (§2.4: battery, charging, thermal, metered), from the
    /// platform. A call hosted here degrades as the policy says (K = 1 and no new Joins, or
    /// `End{host_leaving, next_host}`); later takeovers and the advertised `CallHostStatus`
    /// use them.
    pub fn gcall_set_cond(&self, cond: Cond) {
        self.open().group.gcall.set_cond(cond)
    }

    /// A CallStart relayed through the chat hosts (`GcallNet::observe_start`).
    pub fn gcall_observe_start(&self, start: &SignedStart) -> Result<(), Error> {
        self.open().group.gcall.observe_start(start).map_err(Error::from)
    }

    /// Host a call (`GcallNet::host`).
    pub fn gcall_host(&self, cfg: HostConfig) -> Result<SignedStart, Error> {
        self.gcall_ensure_endpoint()?;
        self.open().group.gcall.host(cfg).map_err(Error::from)
    }

    /// Join the call announced by `start` (`GcallNet::join`).
    pub fn gcall_join(
        &self,
        start: &SignedStart,
        host_addr: Option<EndpointAddr>,
    ) -> Result<(), Error> {
        self.gcall_ensure_endpoint()?;
        self.open().group.gcall.join(start, host_addr).map_err(Error::from)
    }

    pub fn gcall_leave(&self) {
        self.open().group.gcall.leave()
    }

    pub fn gcall_end(&self) {
        self.open().group.gcall.end()
    }

    pub fn gcall_status(&self) -> Option<GcallStatus> {
        self.open().group.gcall.status()
    }

    /// This node's group endpoint on loopback (direct, no relay), bound if needed: for tests
    /// and same-machine hosts.
    pub fn gcall_loopback_addr(&self) -> Result<EndpointAddr, Error> {
        self.gcall_ensure_endpoint()?;
        self.open().group.gcall.loopback_addr().map_err(Error::from)
    }
}
