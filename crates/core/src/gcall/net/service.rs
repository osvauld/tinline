//! The node-level service in `Inner`: at most one live session.

use super::*;

/// Lives in `Inner`. At most one live session.
#[derive(Default)]
pub(crate) struct Service {
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

impl Service {
    pub(super) fn live(&self) -> Option<Arc<Session>> {
        self.cur.lock().clone().filter(|s| !s.finished())
    }

    /// This device's user is in a group call (or one is being set up): 1:1 calls are refused.
    /// Hosting listen-only does not count.
    pub(crate) fn in_call(&self) -> bool {
        self.live().is_some_and(|s| {
            let st = s.st.lock();
            st.client.as_ref().map_or(st.host.is_none(), |c| !is_ended(c))
        })
    }

    pub(crate) fn push_mic(&self, pcm: &[i16]) {
        if let Some(s) = self.live() {
            s.push_mic(pcm);
        }
    }

    pub(crate) fn pull_speaker(&self) -> Vec<i16> {
        match self.live() {
            Some(s) => s.pull_speaker(),
            None => vec![0; audio::FRAME],
        }
    }

    pub(super) fn known_start(&self, call_id: &str) -> Option<CallStart> {
        self.known.lock().starts.get(call_id).cloned()
    }

    /// A session is live (in a call, or hosting one).
    pub(crate) fn busy(&self) -> bool {
        self.live().is_some()
    }

    /// This device's user is in a group call (joining, joined or reconnecting): what the apps'
    /// audio threads follow. Hosting listen-only does not count.
    pub(crate) fn own_in_call(&self) -> bool {
        self.live().is_some_and(|s| s.st.lock().client.as_ref().is_some_and(|c| !is_ended(c)))
    }

    /// The conditions the platform reported (`gcall_set_cond`, or an explicit `gcall_configure`),
    /// None until then. Call-host status publishing waits for them: a device that never said
    /// what it is must not advertise as a desktop on mains (GA-A1).
    pub(crate) fn cond_known(&self) -> Option<Cond> {
        *self.cond.lock()
    }

    /// The current local hosting conditions.
    pub(crate) fn cond(&self) -> Cond {
        let set = *self.cond.lock();
        set.or_else(|| self.cfg.lock().as_ref().map(|c| c.cond)).unwrap_or_else(Cond::desktop)
    }

    /// Configures unless something (a test, the app) already did. True if this configured.
    pub(crate) fn configure_if_unset(&self, mut cfg: GcallConfig) -> bool {
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
