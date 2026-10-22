//! Who is online in an open group (`docs/design/public-group.md` §3). In memory only: nothing
//! here is stored, put in a doc or replicated as a record.
//!
//! - A host knows which member devices hold a live session with it. Every `PRESENCE_EVERY_MS`,
//!   and soon after a session starts or ends (at most one frame per `PRESENCE_MIN_GAP_MS`), it
//!   signs `Presence { group, host, dev, online: [(did, last seen)], seq, at }` with its DID and
//!   sends it to its member sessions and the other hosts (`relay::relay_targets`, as call frames).
//! - A member whose `Hello` said `presence: false` (or this account, for itself, while its own
//!   setting is off) is never listed. A member whose visible session ended stays listed with
//!   that time until the window passes, so a reconnect does not flicker.
//! - Receivers accept a frame only for an open group, from a session whose peer device hosts it
//!   (never from a member), signed by an active host device, with an `at` within
//!   `PRESENCE_MAX_SKEW_MS` of the local clock, and newer (`seq`) than any frame taken from that
//!   host device: the highest `seq` is kept after the frame expires (until the group closes or
//!   the device stops hosting), so an old frame cannot be replayed. A host relays an accepted
//!   frame to its other sessions as it came (the signature is the host's), so a member
//!   connected to one host sees the members of every host; the `seq` check ends relay loops.
//! - Online = the union over the frames held (and, on a host, its own sessions): a DID some host
//!   saw within `PRESENCE_WINDOW_MS`. Ages are taken on the host's clock (`at - last seen`) and
//!   counted from when the frame arrived here, so clocks need not agree.
//! - Private groups: no frames are sent and incoming ones are dropped (unchanged behaviour).

use std::collections::BTreeSet;

use super::*;
use crate::api::OnlineMember;
use crate::record::{MAX_PRESENCE, MAX_PRESENCE_DID, Presence, SignedPresence};
use crate::relay::{Route, relay_targets};

/// A host sends its online list this often.
pub const PRESENCE_EVERY_MS: u64 = 15_000;
/// A member is online while some host saw it this recently.
pub const PRESENCE_WINDOW_MS: u64 = 60_000;
/// Debounce of the frames a session start or end triggers, per group.
const PRESENCE_MIN_GAP_MS: u64 = 1_000;
/// Largest presence frame looked at (500 DIDs fit well inside).
const MAX_PRESENCE_BLOB: usize = 70 * 1024;
/// A frame whose `at` is further than this from our clock is dropped (a replay, or a host
/// whose clock is far off).
pub const PRESENCE_MAX_SKEW_MS: u64 = 120_000;

/// Presence state of one account's group runtime.
pub struct PresenceRt {
    /// This account shows as online in open groups (the local setting, `GroupStore`).
    visible: AtomicBool,
    st: Mutex<St>,
}

impl Default for PresenceRt {
    fn default() -> Self {
        Self::new(true)
    }
}

impl PresenceRt {
    pub fn new(visible: bool) -> Self {
        Self {
            visible: AtomicBool::new(visible),
            st: Default::default(),
        }
    }

    pub fn visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct St {
    /// Host side, per group: members whose last visible session here ended, and when (ms).
    left: HashMap<String, HashMap<String, u64>>,
    /// Host side, per group: our own frames.
    ours: HashMap<String, Ours>,
    /// The newest frame of each other host device, per group.
    frames: HashMap<String, HashMap<(String, [u8; 32]), Held>>,
    /// The highest `seq` taken from each host device, per group: kept after its frame expires
    /// (so an old frame cannot be replayed), until the group is no longer open or the device no
    /// longer hosts it. One number per active host: bounded.
    high: HashMap<String, HashMap<(String, [u8; 32]), u64>>,
    /// Per group: the online DIDs `on_group_presence` last reported.
    notified: HashMap<String, BTreeSet<String>>,
    /// Our last `seq` (all groups): at least the clock, so it keeps growing across restarts.
    seq: u64,
}

impl St {
    /// Takes `p` (already verified: signed by an active host of this open group) if it is newer
    /// than anything taken from that host device and its `at` is within
    /// `PRESENCE_MAX_SKEW_MS` of `now`; then it is the held frame of that device.
    fn take(&mut self, p: Presence, signed: SignedPresence, now: u64) -> bool {
        if p.at.abs_diff(now) > PRESENCE_MAX_SKEW_MS {
            return false;
        }
        let key = (p.host.clone(), p.dev);
        let high = self.high.entry(p.group.clone()).or_default();
        if high.get(&key).is_some_and(|s| *s >= p.seq) {
            return false;
        }
        high.insert(key.clone(), p.seq);
        self.frames.entry(p.group.clone()).or_default().insert(
            key,
            Held {
                p,
                signed,
                recv: now,
            },
        );
        true
    }

    /// Forgets what belongs to groups that are no longer open (`open`: each open group's active
    /// host devices) and frames older than `window`; `notified` keeps a closed group only until
    /// its empty list was reported.
    fn prune(
        &mut self,
        open: &HashMap<String, HashSet<(String, [u8; 32])>>,
        now: u64,
        window: u64,
    ) {
        self.frames.retain(|g, _| open.contains_key(g));
        self.ours.retain(|g, _| open.contains_key(g));
        self.left.retain(|g, _| open.contains_key(g));
        self.high.retain(|g, m| {
            let Some(hosts) = open.get(g) else {
                return false;
            };
            m.retain(|k, _| hosts.contains(k));
            !m.is_empty()
        });
        self.notified
            .retain(|g, s| open.contains_key(g) || !s.is_empty());
        for m in self.left.values_mut() {
            m.retain(|_, t| now.saturating_sub(*t) <= window);
        }
        for m in self.frames.values_mut() {
            m.retain(|_, h| now.saturating_sub(h.recv) <= window);
        }
    }
}

#[derive(Default)]
struct Ours {
    /// When we last sent one (ms; 0 = never).
    sent: u64,
    /// A session started or ended since.
    dirty: bool,
}

struct Held {
    p: Presence,
    signed: SignedPresence,
    /// When it arrived here (ms).
    recv: u64,
}

impl GroupRt {
    /// `(every, window)` ms: 15 s and 60 s (a test may shorten them).
    fn group_presence_timing(&self) -> (u64, u64) {
        #[cfg(any(test, feature = "testkit"))]
        if let Some(t) = *self.presence_timing_for_test.lock() {
            return t;
        }
        (PRESENCE_EVERY_MS, PRESENCE_WINDOW_MS)
    }

    /// `(open, we host)` of a group we hold.
    fn group_presence_view(core: &GroupCore, gid: &str) -> Option<(bool, bool)> {
        core.groups
            .lock()
            .get(gid)
            .map(|l| (l.state.open, l.state.is_host(&core.did, &core.dev)))
    }

    /// Host side: the members this device sees online now, newest first, at most `MAX_PRESENCE`.
    fn group_presence_own(
        &self,
        core: &GroupCore,
        gid: &str,
        now: u64,
        window: u64,
    ) -> Vec<(String, u64)> {
        let sessions = core.sessions_of(gid);
        let mut hidden: HashSet<String> = sessions
            .iter()
            .filter(|s| s.hosting && !s.show)
            .map(|s| s.did.clone())
            .collect();
        let me_visible = core.presence.visible();
        if !me_visible {
            hidden.insert(core.did.clone());
        }
        let mut seen: HashMap<String, u64> = HashMap::new();
        if me_visible {
            seen.insert(core.did.clone(), now);
        }
        for s in sessions.iter().filter(|s| s.hosting && s.show) {
            seen.insert(s.did.clone(), now);
        }
        if let Some(left) = core.presence.st.lock().left.get(gid) {
            for (d, t) in left {
                if now.saturating_sub(*t) <= window {
                    seen.entry(d.clone()).or_insert(*t);
                }
            }
        }
        let mut out: Vec<(String, u64)> = {
            let groups = core.groups.lock();
            let Some(l) = groups.get(gid) else {
                return vec![];
            };
            seen.into_iter()
                .filter(|(d, _)| {
                    !hidden.contains(d)
                        && l.state.is_member(d)
                        && !l.state.is_removed(d)
                        && d.len() <= MAX_PRESENCE_DID
                })
                .collect()
        };
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out.truncate(MAX_PRESENCE);
        out
    }

    /// Signs `online` as our frame for `gid` and sends it where the relay rule says.
    fn group_presence_send(
        &self,
        core: &GroupCore,
        gid: &str,
        online: Vec<(String, u64)>,
        now: u64,
    ) {
        let seq = {
            let mut st = core.presence.st.lock();
            st.seq = (st.seq + 1).max(now);
            let o = st.ours.entry(gid.to_string()).or_default();
            o.sent = now;
            o.dirty = false;
            st.seq
        };
        let p = Presence {
            group: gid.to_string(),
            host: core.did.clone(),
            dev: core.dev,
            online,
            seq,
            at: now,
        };
        let signed = match proto::Signed::sign(&p, &core.account) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("group {} presence: {e}", crate::short(gid));
                return;
            }
        };
        for s in relay_targets(core, gid, None, &Route::All) {
            let _ = s.tx.send(Out::Msg(GroupMsg::Presence {
                presence: signed.clone(),
            }));
        }
    }

    /// Dial-loop tick: our frames when due, expiry, and `on_group_presence` when the list
    /// changed.
    pub(super) fn group_presence_tick(self: &Arc<Self>, core: &Arc<GroupCore>) {
        let (every, window) = self.group_presence_timing();
        let now = now_ms();
        let (groups, open): (
            Vec<(String, bool, bool)>,
            HashMap<String, HashSet<(String, [u8; 32])>>,
        ) = {
            let all = core.groups.lock();
            let groups = all
                .iter()
                .map(|(g, l)| {
                    (
                        g.clone(),
                        l.state.open,
                        l.state.is_host(&core.did, &core.dev),
                    )
                })
                .collect();
            let open = all
                .iter()
                .filter(|(_, l)| l.state.open)
                .map(|(g, l)| {
                    let hosts = l
                        .state
                        .hosts
                        .iter()
                        .map(|h| (h.did.clone(), h.dev))
                        .collect();
                    (g.clone(), hosts)
                })
                .collect();
            (groups, open)
        };
        core.presence.st.lock().prune(&open, now, window);
        for (gid, open, host) in groups {
            if open && host {
                let due = core.presence.st.lock().ours.get(&gid).is_none_or(|o| {
                    let gap = now.saturating_sub(o.sent);
                    gap >= every || (o.dirty && gap >= PRESENCE_MIN_GAP_MS.min(every))
                });
                if due {
                    let online = self.group_presence_own(core, &gid, now, window);
                    self.group_presence_send(core, &gid, online, now);
                }
            }
            self.group_presence_notify(core, &gid);
        }
    }

    /// A session started: as a host of an open group, list it soon (unless it hides) and hand it
    /// the other hosts' latest frames if it may have them.
    pub(super) fn group_presence_on_session(&self, core: &GroupCore, sess: &Sess) {
        let Some((true, true)) = Self::group_presence_view(core, &sess.gid) else {
            return;
        };
        let frames: Vec<SignedPresence> = {
            let mut st = core.presence.st.lock();
            if sess.hosting
                && !sess.show
                && let Some(m) = st.left.get_mut(&sess.gid)
            {
                m.remove(&sess.did);
            }
            st.ours.entry(sess.gid.clone()).or_default().dirty = true;
            st.frames
                .get(&sess.gid)
                .map(|m| m.values().map(|h| h.signed.clone()).collect())
                .unwrap_or_default()
        };
        if relay_targets(core, &sess.gid, None, &Route::All)
            .iter()
            .any(|s| s.id == sess.id)
        {
            for presence in frames {
                let _ = sess.tx.send(Out::Msg(GroupMsg::Presence { presence }));
            }
        }
        core.kick.notify_waiters();
    }

    /// A session ended (already out of the session map): a visible member of ours with no other
    /// session here stays listed with this time until the window passes.
    pub(super) fn group_presence_session_end(&self, core: &GroupCore, sess: &Sess) {
        let Some((true, true)) = Self::group_presence_view(core, &sess.gid) else {
            return;
        };
        let other = core
            .sessions_of(&sess.gid)
            .iter()
            .any(|s| s.hosting && s.did == sess.did);
        let mut st = core.presence.st.lock();
        if sess.hosting && sess.show && !other {
            st.left
                .entry(sess.gid.clone())
                .or_default()
                .insert(sess.did.clone(), now_ms());
        }
        st.ours.entry(sess.gid.clone()).or_default().dirty = true;
    }

    /// A `Presence` from a session: kept if it is the newest of an active host of this open
    /// group, then relayed (when we host) to our other sessions.
    pub(super) fn group_presence_in(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        sess: &Sess,
        signed: SignedPresence,
    ) {
        if signed.len() > MAX_PRESENCE_BLOB {
            return;
        }
        let Ok(p) = signed.open() else { return };
        if p.group != sess.gid || (p.host == core.did && p.dev == core.dev) {
            return;
        }
        // Only from a host: the session's peer device hosts the group (we dialled it, or it is
        // another host that dialled us). A member never hands us presence.
        let ok = core.groups.lock().get(&sess.gid).is_some_and(|l| {
            l.state.open
                && l.state.is_host(&p.host, &p.dev)
                && l.state.is_host(&sess.did, &sess.dev)
        });
        if !ok || !core.presence.st.lock().take(p, signed.clone(), now_ms()) {
            return;
        }
        for s in relay_targets(core, &sess.gid, Some(sess.id), &Route::All) {
            let _ = s.tx.send(Out::Msg(GroupMsg::Presence {
                presence: signed.clone(),
            }));
        }
        self.group_presence_notify(core, &sess.gid);
    }

    /// The DIDs online in `gid` now (sorted; empty unless the group is open): the union of our
    /// own list (if we host) and the held frames of hosts that are still active, current members
    /// only.
    pub fn group_presence_online(&self, core: &GroupCore, gid: &str) -> Vec<String> {
        let (_, window) = self.group_presence_timing();
        let now = now_ms();
        let Some((true, we_host)) = Self::group_presence_view(core, gid) else {
            return vec![];
        };
        let mut online: BTreeSet<String> = BTreeSet::new();
        if we_host {
            online.extend(
                self.group_presence_own(core, gid, now, window)
                    .into_iter()
                    .map(|(d, _)| d),
            );
        }
        let frames: Vec<(Presence, u64)> = core
            .presence
            .st
            .lock()
            .frames
            .get(gid)
            .map(|m| m.values().map(|h| (h.p.clone(), h.recv)).collect())
            .unwrap_or_default();
        let groups = core.groups.lock();
        let Some(l) = groups.get(gid) else {
            return vec![];
        };
        for (p, recv) in frames {
            if !l.state.is_host(&p.host, &p.dev) {
                continue;
            }
            for (d, last) in p.online {
                let seen = recv.saturating_sub(p.at.saturating_sub(last));
                if now.saturating_sub(seen) <= window {
                    online.insert(d);
                }
            }
        }
        online
            .into_iter()
            .filter(|d| l.state.is_member(d) && !l.state.is_removed(d))
            .collect()
    }

    /// `group_online` rows: DID, name as the group knows it, admin.
    pub fn group_online_members(
        &self,
        core: &GroupCore,
        gid: &str,
    ) -> Result<Vec<OnlineMember>, Error> {
        if !core.groups.lock().contains_key(gid) {
            return Err(Error::NotFound);
        }
        let dids = self.group_presence_online(core, gid);
        let groups = core.groups.lock();
        let l = groups.get(gid).ok_or(Error::NotFound)?;
        Ok(dids
            .into_iter()
            .map(|did| OnlineMember {
                name: l
                    .state
                    .members
                    .get(&did)
                    .map(|m| m.name.clone())
                    .unwrap_or_default(),
                is_admin: l.state.is_admin(&did),
                did,
            })
            .collect())
    }

    /// `on_group_presence` if the online list of `gid` changed since we last said.
    fn group_presence_notify(self: &Arc<Self>, core: &GroupCore, gid: &str) {
        let now: BTreeSet<String> = self.group_presence_online(core, gid).into_iter().collect();
        let changed = {
            let mut st = core.presence.st.lock();
            if st.notified.get(gid).is_some_and(|s| *s == now)
                || (now.is_empty() && !st.notified.contains_key(gid))
            {
                false
            } else {
                st.notified.insert(gid.to_string(), now);
                true
            }
        };
        if changed && let Some(ev) = self.group_ev() {
            let g = gid.to_string();
            self.handle.spawn_blocking(move || ev.on_group_presence(g));
        }
    }

    /// The local setting: hosts learn it from our next `Hello`, so our sessions to hosts of open
    /// groups end and are dialled again; where we host, our own listing changes at once.
    pub fn group_presence_set_visible(
        &self,
        core: &GroupCore,
        visible: bool,
    ) -> Result<(), Error> {
        core.store.set_presence_visible(visible)?;
        if core.presence.visible.swap(visible, Ordering::SeqCst) == visible {
            return Ok(());
        }
        let open: HashSet<String> = core
            .groups
            .lock()
            .iter()
            .filter(|(_, l)| l.state.open)
            .map(|(g, _)| g.clone())
            .collect();
        core.sessions.lock().retain(|_, s| {
            let keep = s.hosting || !open.contains(&s.gid);
            if !keep {
                let _ = s.tx.send(Out::Close);
            }
            keep
        });
        {
            let mut st = core.presence.st.lock();
            for g in &open {
                st.ours.entry(g.clone()).or_default().dirty = true;
            }
        }
        core.kick.notify_waiters();
        Ok(())
    }

    /// Test hook: signs a frame as this device listing `dids` (seen now), whether or not we host,
    /// and sends it on every session of `gid` (what a modified app could do).
    #[cfg(any(test, feature = "testkit"))]
    pub fn group_presence_forge_for_test(
        &self,
        core: &GroupCore,
        gid: &str,
        dids: Vec<String>,
    ) -> Result<(), Error> {
        let now = now_ms();
        let seq = {
            let mut st = core.presence.st.lock();
            st.seq = (st.seq + 1).max(now);
            st.seq
        };
        let p = Presence {
            group: gid.to_string(),
            host: core.did.clone(),
            dev: core.dev,
            online: dids.into_iter().map(|d| (d, now)).collect(),
            seq,
            at: now,
        };
        let presence =
            proto::Signed::sign(&p, &core.account).map_err(|e| Error::Protocol(e.to_string()))?;
        for s in core.sessions_of(gid) {
            let _ = s.tx.send(Out::Msg(GroupMsg::Presence {
                presence: presence.clone(),
            }));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
