//! The node-level service: at most one live session.

use std::sync::{OnceLock, Weak};

use super::*;

/// Placeholder type for an empty `Weak<dyn GcallHost>` (never upgrades).
struct NoHost;
impl GcallHost for NoHost {
    fn identity(&self) -> Result<Identity, Error> {
        Err(Error::NotStarted)
    }
    fn group_endpoint(&self) -> Option<Endpoint> {
        None
    }
    fn with_no_call(&self, _: &mut dyn FnMut() -> Result<(), Error>) -> Result<(), Error> {
        Err(Error::NotStarted)
    }
}

/// The group-call network service; the node owns one. At most one live session.
pub struct GcallNet {
    pub(super) handle: tokio::runtime::Handle,
    /// The node, installed once after it is built (`install_host`).
    pub(super) node: OnceLock<Weak<dyn GcallHost>>,
    pub(super) cfg: Mutex<Option<Arc<GcallConfig>>>,
    /// The latest `gcall_set_cond`, also before any configuration.
    pub(super) cond: Mutex<Option<Cond>>,
    pub(super) cur: Mutex<Option<Arc<Session>>>,
    pub(super) known: Known,
}

/// What this node knows about calls beyond its own session.
#[derive(Default)]
pub(super) struct Calls {
    /// CallStarts seen through the chat relay (or the API), highest generation per call_id;
    /// what a backup that is not in the call takes over from, and what a `Superseded` follows.
    pub(super) starts: BTreeMap<String, CallStart>,
    /// gid → board (glare and split settling for the call hosted here).
    pub(super) boards: BTreeMap<String, CallBoard>,
    /// A `Superseded` whose winner's CallStart is not known yet: (winner, gid, when).
    pub(super) follow: Option<(HostRef, String, Instant)>,
}

pub(super) type Known = Arc<Mutex<Calls>>;

impl GcallNet {
    /// A service whose tasks run on `handle`. Call [`install_host`](Self::install_host) before use.
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        GcallNet { handle, node: OnceLock::new(), cfg: Default::default(), cond: Default::default(), cur: Default::default(), known: Default::default() }
    }

    /// Installs the node (once; later calls are ignored). The service keeps only a `Weak`.
    pub fn install_host(&self, node: Weak<dyn GcallHost>) {
        let _ = self.node.set(node);
    }

    pub(super) fn node_weak(&self) -> Weak<dyn GcallHost> {
        self.node.get().cloned().unwrap_or_else(|| Weak::<NoHost>::new())
    }

    /// The node, while it is alive.
    pub(super) fn node(&self) -> Result<Arc<dyn GcallHost>, Error> {
        self.node.get().and_then(Weak::upgrade).ok_or(Error::NotStarted)
    }

    pub(super) fn live(&self) -> Option<Arc<Session>> {
        self.cur.lock().clone().filter(|s| !s.finished())
    }

    /// This device's user is in a group call (or one is being set up): 1:1 calls are refused.
    /// Hosting listen-only does not count.
    pub fn in_call(&self) -> bool {
        self.live().is_some_and(|s| {
            let st = s.st.lock();
            st.client.as_ref().map_or(st.host.is_none(), |c| !is_ended(c))
        })
    }

    pub fn push_mic(&self, pcm: &[i16]) {
        if let Some(s) = self.live() {
            s.push_mic(pcm);
        }
    }

    pub fn pull_speaker(&self) -> Vec<i16> {
        match self.live() {
            Some(s) => s.pull_speaker(),
            None => vec![0; audio::FRAME],
        }
    }

    pub(super) fn known_start(&self, call_id: &str) -> Option<CallStart> {
        self.known.lock().starts.get(call_id).cloned()
    }

    /// A session is live (in a call, or hosting one).
    pub fn busy(&self) -> bool {
        self.live().is_some()
    }

    /// This device's user is in a group call (joining, joined or reconnecting): what the apps'
    /// audio threads follow. Hosting listen-only does not count.
    pub fn own_in_call(&self) -> bool {
        self.live().is_some_and(|s| s.st.lock().client.as_ref().is_some_and(|c| !is_ended(c)))
    }

    /// The conditions the platform reported (`gcall_set_cond`, or an explicit `gcall_configure`),
    /// None until then. Call-host status publishing waits for them: a device that never said
    /// what it is must not advertise as a desktop on mains (GA-A1).
    pub fn cond_known(&self) -> Option<Cond> {
        *self.cond.lock()
    }

    /// The current local hosting conditions.
    pub fn cond(&self) -> Cond {
        let set = *self.cond.lock();
        set.or_else(|| self.cfg.lock().as_ref().map(|c| c.cond)).unwrap_or_else(Cond::desktop)
    }

    /// Configures unless something (a test, the app) already did. True if this configured.
    pub fn configure_if_unset(&self, mut cfg: GcallConfig) -> bool {
        let mut slot = self.cfg.lock();
        if slot.is_some() {
            return false;
        }
        if let Some(c) = *self.cond.lock() {
            cfg.cond = c;
        }
        *slot = Some(Arc::new(cfg));
        true
    }
}
