//! Test hooks on `Node` (not for apps; exported through uniffi like the rest).

use super::*;

/// Test hooks (not for apps).
#[uniffi::export]
impl Node {
    /// Group endpoint ids this device holds for `group_id` (hosts' addresses and, on a host, its
    /// members' directory).
    pub fn group_known_endpoints_for_test(&self, group_id: String) -> Vec<String> {
        self.inner.group_core().map(|c| c.known_endpoints(&group_id)).unwrap_or_default()
    }

    /// Host side: the member group endpoints in its directory.
    pub fn group_directory_for_test(&self, group_id: String) -> Vec<String> {
        self.inner.group_core().map(|c| c.store.dir(&group_id).unwrap_or_default().into_iter().map(|(_, d)| d.endpoint).collect()).unwrap_or_default()
    }

    /// Dial only hosts of `did` (`None`: all).
    pub fn group_dial_only_for_test(&self, did: Option<String>) {
        *self.inner.group.dial_only.lock() = did;
        if let Ok(c) = self.inner.group_core() {
            c.close_sessions_except(self.inner.group.dial_only.lock().as_deref());
        }
    }

    /// The next `n` invite fetches (one network attempt each) fail as if the connection was lost.
    pub fn group_fail_next_fetches_for_test(&self, n: u32) {
        self.inner.group.fail_fetches.store(n, std::sync::atomic::Ordering::SeqCst);
    }

    /// Pauses (or resumes) the background fetch of older days: the platform sets it while the
    /// network is metered or the battery low, the inputs group call hosting uses.
    pub fn group_set_backfill_paused(&self, paused: bool) {
        self.inner.group.backfill_paused.store(paused, std::sync::atomic::Ordering::SeqCst);
    }

    /// Pause between two history fetches, ms (0 = the default 200).
    pub fn group_backfill_pause_ms_for_test(&self, ms: u64) {
        self.inner.group.backfill_pause_ms.store(ms, std::sync::atomic::Ordering::SeqCst);
    }

    /// Writes our post into the day of `at_ms` (a time in the past, say).
    pub fn group_post_at_for_test(&self, group_id: String, at_ms: i64, text: String) -> Result<GroupMessage, Error> {
        let core = self.inner.group_core()?;
        self.inner.group_member_check(&core, &group_id)?;
        let (entry, changed) = core.write_at(&group_id, Body::Post { text, reply_to: None, image: None }, at_ms)?;
        let item = changed.new_items.iter().find(|i| i.id == entry.id).cloned();
        self.inner.group_after(&core, changed, None);
        self.inner.group_message(&core, &group_id, &item.ok_or(Error::NotFound)?).ok_or(Error::NotFound)
    }

    /// Compacts the old day docs now (otherwise every hour); returns how many it closed. Also
    /// counts as the hourly run, so none starts behind the test's back for the next hour, and
    /// waits out one already running.
    pub fn group_compact_now_for_test(&self) -> Result<u32, Error> {
        let core = self.inner.group_core()?;
        Ok(self.inner.group_compact_now(&core))
    }

    /// The days of posts this device holds of the group, oldest first.
    pub fn group_days_for_test(&self, group_id: String) -> Result<Vec<String>, Error> {
        Ok(self.inner.group_core()?.store.days(&group_id)?)
    }

    /// Forgets every day doc of the group on this device (its history), keeping the meta
    /// records: what a wiped host looks like. Returns how many days went.
    pub fn group_drop_days_for_test(&self, group_id: String) -> Result<u32, Error> {
        let core = self.inner.group_core()?;
        let n = core.store.days(&group_id)?.len() as u32;
        let mut groups = core.groups.lock();
        let live = groups.get_mut(&group_id).ok_or(Error::NotFound)?;
        core.store.drop_days(&group_id)?;
        live.shards = Default::default();
        live.meta = GroupMeta { last: None, unread: 0, newest_day: None, pending_days: Default::default(), owed_days: Default::default(), ..live.meta.clone() };
        core.store.put_meta(&group_id, &live.meta)?;
        Ok(n)
    }

    /// How many day docs of the group are in memory.
    pub fn group_cached_days_for_test(&self, group_id: String) -> Result<u32, Error> {
        let core = self.inner.group_core()?;
        Ok(core.groups.lock().get(&group_id).map_or(0, |l| l.shards.cached() as u32))
    }

    /// How many of the stored days of the group are closed (compacted).
    pub fn group_closed_days_for_test(&self, group_id: String) -> Result<u32, Error> {
        let core = self.inner.group_core()?;
        Ok(core.store.days(&group_id)?.iter().filter(|d| core.store.shard_meta(&group_id, d).ok().flatten().is_some_and(|m| m.closed)).count() as u32)
    }

    /// The next `n` dials to a host fail as if the connection was lost.
    pub fn group_fail_next_dials_for_test(&self, n: u32) {
        self.inner.group.fail_dials.store(n, std::sync::atomic::Ordering::SeqCst);
    }

    /// Behave like a modified app that ignores its own removal (keeps posting and dialing).
    pub fn group_ignore_removal_for_test(&self, ignore: bool) {
        self.inner.group.ignore_removal.store(ignore, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Test hooks that release builds do not have (Rust only: not exported through uniffi).
#[cfg(any(test, feature = "dev-tools"))]
impl Node {
    /// Uses `policy` (unchecked: seconds-long intervals) instead of the stored rotation policy,
    /// in memory only; `None` goes back to the stored one. Not in release builds.
    pub fn group_set_rotation_policy_for_test(&self, policy: Option<crate::group::rotation::RotationPolicy>) {
        *self.inner.group.rotation_policy_for_test.lock() = policy;
        if let Ok(c) = self.inner.group_core() {
            c.kick_pub();
        }
    }

    /// The window a removed member's pre-removal upload is accepted in, ms, instead of
    /// `REMOVED_UPLOAD_WINDOW_MS` (2 days); `None` goes back to it. Not in release builds.
    pub fn group_set_removed_upload_window_for_test(&self, ms: Option<u64>) {
        *self.inner.group.removed_upload_window_for_test.lock() = ms;
    }

    /// Presence `(every_ms, window_ms)` instead of 15 s / 60 s; `None` goes back to them. Not in
    /// release builds.
    pub fn group_set_presence_timing_for_test(&self, timing: Option<(u64, u64)>) {
        *self.inner.group.presence_timing_for_test.lock() = timing;
        if let Ok(c) = self.inner.group_core() {
            c.kick_pub();
        }
    }

    /// The group's meta frontier on this device now (`(peer, counter)`): what a record written
    /// now names as `seen`. Not in release builds.
    pub fn group_meta_frontier_for_test(&self, group_id: String) -> Result<Vec<(u64, i32)>, Error> {
        let core = self.inner.group_core()?;
        let groups = core.groups.lock();
        Ok(groups.get(&group_id).ok_or(Error::NotFound)?.doc.seen())
    }

    /// Posts claiming the signed frontier `seen` (say, one from before admins-only), skipping
    /// the local checks: what a modified app could do. Not in release builds.
    pub fn group_post_with_seen_for_test(&self, group_id: String, text: String, seen: Vec<(u64, i32)>) -> Result<(), Error> {
        let core = self.inner.group_core()?;
        let (_, changed) = core.write_seen_for_test(&group_id, Body::Post { text, reply_to: None, image: None }, seen)?;
        self.inner.group_after(&core, changed, None);
        Ok(())
    }

    /// A throwaway identity's `OpenJoin` claiming `seen`, added here and pushed to our hosts as
    /// if this member relayed it: what a modified app could do. Returns the throwaway DID. Not in
    /// release builds.
    pub fn group_forge_open_join_for_test(&self, group_id: String, seen: Vec<(u64, i32)>) -> Result<String, Error> {
        let core = self.inner.group_core()?;
        let (did, changed) = core.forge_open_join_for_test(&group_id, "throwaway".into(), seen)?;
        if let Some(c) = changed {
            self.inner.group_after(&core, c, None);
        }
        Ok(did)
    }

    /// Signs a presence frame as this device (host or not) listing `dids` and sends it on every
    /// session of the group: what a modified app could send. Not in release builds.
    pub fn group_send_presence_for_test(&self, group_id: String, dids: Vec<String>) -> Result<(), Error> {
        let core = self.inner.group_core()?;
        self.inner.group_presence_forge_for_test(&core, &group_id, dids)
    }
}
