//! `Node` entry points of the group-call service (`gcall_net`), plus the node's side of it: the
//! [`GcallHost`] implementation on `Inner`.

use std::sync::Arc;

use iroh::{Endpoint, EndpointAddr};
use identity::Identity;
use proto::gcall::{SignedEnd, SignedStart};

use super::*;
use crate::Error;
use crate::node::Inner;

impl GcallHost for Inner {
    fn identity(&self) -> Result<Identity, gcall_net::Error> {
        Ok(self.me().map_err(Error::from)?.id.clone())
    }

    fn group_endpoint(&self) -> Option<Endpoint> {
        Inner::group_endpoint(self)
    }

    /// Runs `f` under the `live` lock (lock order: live -> gcall session).
    fn with_no_call(&self, f: &mut dyn FnMut() -> Result<(), gcall_net::Error>) -> Result<(), gcall_net::Error> {
        self.with_no_call(|| f().map_err(|e| Error::from(e).into())).map_err(|e| gcall_net::Error::from(Error::from(e)))
    }
}

impl Inner {
    /// `Node::stop` / `Node::lock` / an account switch, before the endpoints close.
    pub(crate) async fn gcall_shutdown(&self) {
        self.gcall.shutdown().await
    }

    /// A `tinline/gcall/1` connection (called from the accept loop in `group/engine/accepting.rs`).
    pub(crate) async fn handle_gcall_incoming(self: Arc<Self>, accepting: iroh::endpoint::Accepting) -> Result<(), Error> {
        self.gcall.incoming(accepting).await.map_err(Error::from)
    }
}

/// Group calls on a [`crate::Node`]. **Unstable** plain Rust; the app API is the group layer's
/// UniFFI calls (`group::calls`). Calls run on the group endpoint (U2/U7): the calls that start
/// a session bring it up first.
impl crate::Node {
    /// Sets the directory, event sink, own host ref and conditions. Applies to sessions
    /// started afterwards. Group chat configures itself (`GroupCallDir`) unless this was called.
    pub fn gcall_configure(&self, cfg: GcallConfig) {
        self.inner.gcall.configure(cfg)
    }

    /// The group endpoint, bound (otherwise it is bound only once the account has a group).
    fn gcall_ensure_endpoint(&self) -> Result<(), Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            inner.group_ensure(&core).await.map(|_| ())
        })?
    }

    /// New local hosting conditions (§2.4: battery, charging, thermal, metered), from the
    /// platform. A call hosted here degrades as the policy says (K = 1 and no new Joins, or
    /// `End{host_leaving, next_host}`); later takeovers and the advertised `CallHostStatus`
    /// use them.
    pub fn gcall_set_cond(&self, cond: Cond) {
        self.inner.gcall.set_cond(cond)
    }

    /// A CallStart relayed through the chat hosts (`GcallNet::observe_start`).
    pub fn gcall_observe_start(&self, start: &SignedStart) -> Result<(), Error> {
        self.inner.gcall.observe_start(start).map_err(Error::from)
    }

    /// A relayed `End{ended}`: the call leaves the group's board.
    pub fn gcall_observe_end(&self, end: &SignedEnd) -> Result<(), Error> {
        self.inner.gcall.observe_end(end).map_err(Error::from)
    }

    /// Host a call (`GcallNet::host`).
    pub fn gcall_host(&self, cfg: HostConfig) -> Result<SignedStart, Error> {
        self.gcall_ensure_endpoint()?;
        self.inner.gcall.host(cfg).map_err(Error::from)
    }

    /// Join the call announced by `start` (`GcallNet::join`).
    pub fn gcall_join(
        &self,
        start: &SignedStart,
        host_addr: Option<EndpointAddr>,
    ) -> Result<(), Error> {
        self.gcall_ensure_endpoint()?;
        self.inner.gcall.join(start, host_addr).map_err(Error::from)
    }

    pub fn gcall_leave(&self) {
        self.inner.gcall.leave()
    }

    pub fn gcall_end(&self) {
        self.inner.gcall.end()
    }

    pub fn gcall_set_mic_muted(&self, muted: bool) {
        self.inner.gcall.set_mic_muted(muted)
    }

    pub fn gcall_mute_participant(&self, did: &str, muted: bool) {
        self.inner.gcall.mute_participant(did, muted)
    }

    pub fn gcall_push_mic(&self, pcm: &[i16]) {
        self.inner.gcall.push_mic(pcm)
    }

    pub fn gcall_pull_speaker(&self) -> Vec<i16> {
        self.inner.gcall.pull_speaker()
    }

    pub fn gcall_status(&self) -> Option<GcallStatus> {
        self.inner.gcall.status()
    }

    /// This node's group endpoint on loopback (direct, no relay), bound if needed: for tests
    /// and same-machine hosts.
    pub fn gcall_loopback_addr(&self) -> Result<EndpointAddr, Error> {
        self.gcall_ensure_endpoint()?;
        self.inner.gcall.loopback_addr().map_err(Error::from)
    }
}
