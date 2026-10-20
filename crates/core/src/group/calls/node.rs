//! The `Node` API of group calls.

use super::*;

impl Node {
    fn calls_ready(&self) -> Result<Arc<GroupCore>, Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            inner.group_ensure(&core).await?;
            inner.group_calls_configure();
            Ok(core)
        })?
    }
}

/// Group calls (GA-I1). Plain group membership is the authority (no per-contact call tokens,
/// §3.3); joining is always the user's choice.
#[uniffi::export]
impl Node {
    pub fn set_group_call_events(&self, events: Arc<dyn GroupCallEvents>) {
        *self.inner.group.calls.events.lock() = Some(events);
    }

    /// Starts a call in the group: picks the best call host from the fresh `CallHostStatus`es
    /// (§2.3) and asks it through the chat hosts; this device joins once it announces the call.
    /// Returns the call id at once; progress comes as events (`Joined`, or `StartFailed`).
    /// `Rejected("no call host available")` when no call host advertises.
    pub fn group_call_start(&self, group_id: String) -> Result<String, Error> {
        self.calls_ready()?;
        self.inner.group_call_start(&group_id)
    }

    /// Joins a call announced in the group (`group_calls`).
    pub fn group_call_join(&self, group_id: String, call_id: String) -> Result<(), Error> {
        let core = self.calls_ready()?;
        self.inner.group_member_check(&core, &group_id)?;
        let blob = self.inner.group.calls.st.lock().calls.get(&group_id).and_then(|c| c.get(&call_id)).map(|(b, _)| b.clone());
        let blob = blob.ok_or(Error::NotFound)?;
        self.inner.gcall_join(&blob, None)
    }

    /// Leaves the call this device is in. A call hosted here that nobody is in any more ends.
    pub fn group_call_leave(&self) {
        self.inner.group.calls.st.lock().pending = None;
        self.inner.gcall_leave();
        let empty = self.inner.gcall_status().and_then(|s| s.hosting).is_some_and(|h| h.roster.is_empty());
        if empty {
            self.inner.gcall_end();
        }
    }

    /// The platform's local conditions (battery, charging, heat, metered data, device class;
    /// §2.4). Until the first call this device advertises no call hosting and declines to host.
    /// Push on start and whenever they change.
    pub fn group_call_set_conditions(&self, conditions: GroupCallConditions) {
        self.inner.gcall_set_cond(conditions.into())
    }

    /// The group call this device is in or hosts, if any: what the apps' audio threads and
    /// foreground service follow (re-read on every `GroupCallEvents` callback; the callbacks
    /// can arrive out of order).
    pub fn group_call_here(&self) -> Option<GroupCallHere> {
        let st = self.inner.gcall_status()?;
        let in_call = self.inner.gcall.own_in_call();
        let hosting = self.inner.gcall.busy() && st.hosting.is_some();
        if !in_call && !hosting {
            return None;
        }
        let group_id = self.inner.call_gid(&st.call_id).unwrap_or_default();
        Some(GroupCallHere { group_id, call_id: st.call_id, in_call, hosting })
    }

    /// Mutes this device's microphone in the group call.
    pub fn group_call_set_muted(&self, muted: bool) {
        self.inner.gcall_set_mic_muted(muted)
    }

    /// How long a started group call rings, in ms (apps ring up to this long from the moment
    /// they hear the `Announced` event, and stop earlier on join, "Not now" or `Ended`).
    pub fn group_call_ring_ms(&self) -> u64 {
        GROUP_CALL_RING_MS
    }

    /// Whether the announced call `call_id` of the group should ring this device: it is known
    /// and still running, somebody else started it, that DID is not blocked by me or blocking
    /// me, and this device is on no group call. The apps add what only they know: a 1:1 call in
    /// progress, and a muted group where the app has a per-group mute.
    pub fn group_call_rings(&self, group_id: String, call_id: String) -> bool {
        let Ok(core) = self.inner.group_core() else { return false };
        let found = self.inner.group.calls.st.lock().calls.get(&group_id).and_then(|c| c.get(&call_id)).map(|(_, s)| (s.starter_did.clone(), s.started_ms));
        let Some((starter, started)) = found else { return false };
        // Only a call started within the ring time rings: an old one re-announced to a device
        // that just (re)connected or restarted shows as "Call in progress", silently.
        if now_ms().saturating_sub(started) > GROUP_CALL_RING_MS {
            return false;
        }
        if starter == core.did || self.inner.gcall.busy() {
            return false;
        }
        let s = self.inner.shared.lock();
        !s.state.blocked.contains(&starter) && !crate::block::blocked_me(&s.state, &starter)
    }

    /// The calls going on in the group, as this device knows them.
    pub fn group_calls(&self, group_id: String) -> Result<Vec<GroupCall>, Error> {
        let core = self.inner.group_core()?;
        Ok(self.inner.group_calls_list(&core, &group_id))
    }

    /// Group calls this device was in or missed, newest first; `None` lists every group (the
    /// Calls tab), `Some` one group (its thread). Local to this device.
    pub fn group_call_history(&self, group_id: Option<String>) -> Result<Vec<crate::group::calllog::GroupCallRecord>, Error> {
        Ok(self.inner.group_core()?.store.clog_list(group_id.as_deref())?)
    }

    /// Test hook: the DIDs whose fresh `CallHostStatus` this device holds for the group.
    pub fn group_call_hosts_advertising_for_test(&self, group_id: String) -> Vec<String> {
        let now = now_ms();
        let st = self.inner.group.calls.st.lock();
        st.statuses.get(&group_id).map(|m| m.values().filter(|(_, s)| s.is_fresh(now)).map(|(_, s)| s.host_ref.holder_did.clone()).collect()).unwrap_or_default()
    }

    /// Pauses (or resumes) this device's call hosting for the group without touching its
    /// signed `CallHostAccept`: while paused it advertises nothing, declines requests and does
    /// not take calls over. Not persisted (a restart resumes).
    pub fn group_call_hosting_enable(&self, group_id: String, enable: bool) {
        let mut p = self.inner.group.calls.paused.lock();
        if enable {
            p.remove(&group_id);
        } else {
            p.insert(group_id);
        }
    }
}
