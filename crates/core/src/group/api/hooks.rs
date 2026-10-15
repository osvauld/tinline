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

    /// The U12 rotation delay in seconds (default 2 days).
    pub fn group_set_rotation_delay_for_test(&self, secs: u64) {
        self.inner.group.rotation_delay_ms.store(secs.max(1) * 1000, std::sync::atomic::Ordering::SeqCst);
    }

    /// How long a rotated host keeps telling offline members it moved, in seconds (default: the
    /// rotation delay).
    pub fn group_set_moved_window_for_test(&self, secs: u64) {
        self.inner.group.moved_window_ms.store(secs.max(1) * 1000, std::sync::atomic::Ordering::SeqCst);
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
        let (entry, changed) = core.write_at(&group_id, Body::Post { text, reply_to: None }, at_ms)?;
        let item = changed.new_items.iter().find(|i| i.id == entry.id).cloned();
        self.inner.group_after(&core, changed, None);
        self.inner.group_message(&core, &group_id, &item.ok_or(Error::NotFound)?).ok_or(Error::NotFound)
    }

    /// Compacts the old day docs now (otherwise every hour); returns how many it closed.
    pub fn group_compact_now_for_test(&self) -> Result<u32, Error> {
        let core = self.inner.group_core()?;
        Ok(self.inner.group_compact(&core))
    }

    /// The days of posts this device holds of the group, oldest first.
    pub fn group_days_for_test(&self, group_id: String) -> Result<Vec<String>, Error> {
        self.inner.group_core()?.store.days(&group_id)
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
