//! Group endpoint rotation (crash-safe rebind) and authenticated moved-host announcements
//! (`docs/rotation.md`).

use super::lifecycle::bind_group_endpoint;
use super::*;
use crate::group::rotation::{RotationPolicy, draw_jitter, rotation_due};
use crate::group::store::EpState;

/// Clears `GroupCore::rotating` when the rotation ends, however it ends.
struct Rotating<'a>(&'a AtomicBool);

impl Drop for Rotating<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl Inner {
    // ---- endpoint rotation ---------------------------------------------------------------------

    /// The rotation policy in force: the stored one (a test may override it in memory).
    pub(crate) fn group_rotation_policy(&self, core: &GroupCore) -> Result<RotationPolicy, Error> {
        #[cfg(any(test, feature = "dev-tools"))]
        if let Some(p) = *self.group.rotation_policy_for_test.lock() {
            return Ok(p);
        }
        Ok(core.store.rotation_policy()?)
    }

    /// The current endpoint key and schedule (the first key is made here).
    pub(super) fn group_ep_state(&self, core: &GroupCore) -> Result<EpState, Error> {
        let policy = self.group_rotation_policy(core)?;
        Ok(core.store.ep_state_or_init(now_ms(), draw_jitter(policy.jitter_ms))?)
    }

    /// When this device's group endpoint is due to rotate (periodic only, D11: no removal,
    /// flag, revocation or block moves it). `None` while it hosts no chat and no calls, or
    /// before it has a key.
    pub(crate) fn group_rotation_due(&self, core: &GroupCore) -> Result<Option<u64>, Error> {
        let Some(ep) = core.store.ep_state()? else {
            return Ok(None);
        };
        let hosting = core
            .groups
            .lock()
            .values()
            .any(|l| hosts_or_calls(&l.state, &core.did, &core.dev));
        let policy = self.group_rotation_policy(core)?;
        Ok(rotation_due(ep.made, &policy, ep.jitter, now_ms(), hosting))
    }

    pub(super) async fn group_rotation_tick(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
    ) -> Result<(), Error> {
        let Some(due) = self.group_rotation_due(core)? else {
            return Ok(());
        };
        if due > now_ms() || core.ep_id.lock().is_none() {
            return Ok(());
        }
        // G6 §5: not while a group call runs on this endpoint (it would cut it), for at most
        // an hour past the deadline.
        if self.gcall.busy() && now_ms() < due + CALL_ROTATION_DEFER_MS {
            return Ok(());
        }
        self.group_rotate(core).await
    }

    /// A new group endpoint key, crash-safe: the next key is staged, bound while the old
    /// endpoint keeps serving, promoted in one transaction, and only then swapped in and the
    /// old endpoint closed (every session with it ends; whoever knew only it can no longer
    /// reach us). Any failure before the swap leaves the old endpoint serving; the next tick
    /// tries again. Then we tell the members we host where we are now.
    pub(crate) async fn group_rotate(self: &Arc<Self>, core: &Arc<GroupCore>) -> Result<(), Error> {
        if core.rotating.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let _running = Rotating(&core.rotating);
        if core.ep_id.lock().is_none() {
            return Err(Error::NotStarted);
        }
        let policy = self.group_rotation_policy(core)?;
        let next = core.store.ep_stage()?;
        let new = match bind_group_endpoint(&next).await {
            Ok(ep) => ep,
            Err(e) => {
                let _ = core.store.ep_discard_pending();
                return Err(e);
            }
        };
        drop(next);
        let now = now_ms();
        let announce_until = now.saturating_add(policy.announce_ms);
        let promoted = core
            .store
            .ep_promote(now, announce_until, draw_jitter(policy.jitter_ms));
        let old = {
            let mut slot = core.ep.lock().await;
            if let Err(e) = promoted.map_err(Error::from).and(if core.closed() {
                Err(Error::Locked)
            } else {
                Ok(())
            }) {
                drop(slot);
                let _ = core.store.ep_discard_pending();
                let _ = tokio::time::timeout(Duration::from_secs(3), new.close()).await;
                return Err(e);
            }
            let old = slot.replace(new.clone());
            *core.ep_id.lock() = Some(new.clone());
            // Sessions on the old endpoint end; nothing hands out its address any more.
            core.my_addr.lock().clear();
            let mut ss = core.sessions.lock();
            for s in ss.values() {
                let _ = s.tx.send(Out::Close);
            }
            ss.clear();
            old
        };
        self.group_serve(core, &new);
        let hosted: Vec<String> = core
            .groups
            .lock()
            .iter()
            .filter(|(_, l)| hosts_or_calls(&l.state, &core.did, &core.dev))
            .map(|(g, _)| g.clone())
            .collect();
        // Our new address is in every Hello from now on (the dial loop runs concurrently with
        // `group_rotate_now`); signed again below once the relay is known.
        for gid in &hosted {
            let _ = self.group_my_addr(core, gid);
        }
        if let Some(ep) = old {
            let _ = tokio::time::timeout(Duration::from_secs(3), ep.close()).await;
        }
        self.group_relay_ready().await;
        self.log(format!("group endpoint rotated to {}", new.id()));
        for gid in &hosted {
            let _ = self.group_my_addr(core, gid);
        }
        self.group_announce_moved(core, Duration::ZERO);
        core.backoff.lock().clear();
        core.kick.notify_waiters();
        Ok(())
    }

    /// Starts the `Moved` campaign of the current endpoint if its announce window (persisted,
    /// so it resumes after a restart) is still open and none runs for it yet.
    pub(super) fn group_announce_moved(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        first_wait: Duration,
    ) {
        let Some(ep) = core.ep_id.lock().clone() else {
            return;
        };
        let until = match core.store.ep_state() {
            Ok(Some(s)) => s.announce_until,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!("group endpoint state: {e}");
                return;
            }
        };
        if until <= now_ms() {
            return;
        }
        {
            let mut running = core.announcing.lock();
            if running.as_ref() == Some(&ep.id()) {
                return;
            }
            *running = Some(ep.id());
        }
        let (this, core) = (self.clone(), core.clone());
        self.handle.spawn(async move {
            tokio::time::sleep(first_wait).await;
            this.group_tell_moved(&core, &ep, until).await;
            let mut running = core.announcing.lock();
            if running.as_ref() == Some(&ep.id()) {
                *running = None;
            }
        });
    }

    /// Dials every current member device in the directories of the groups whose chat we host
    /// that has no session with us yet and hands it our new address (`Moved`), with backoff,
    /// until all are back, the endpoint changes again, or `until` (unix ms) has passed
    /// (PROVISIONAL: members offline longer must get a fresh token). A call host that does not
    /// host chat has no member directory: its new address reaches the members through the chat
    /// hosts (its next Hello, relayed as `Hosts`).
    async fn group_tell_moved(&self, core: &Arc<GroupCore>, ep: &Endpoint, until: u64) {
        let mut wait = 2u64;
        loop {
            let same = core
                .ep_id
                .lock()
                .as_ref()
                .is_some_and(|e| e.id() == ep.id());
            if core.closed() || !same || now_ms() > until {
                break;
            }
            let targets = self.group_moved_targets(core);
            if targets.is_empty() {
                break;
            }
            let auth = endpoint_auth(&core.account, core.dev, *ep.id().as_bytes());
            for (gid, d) in targets {
                let Ok(key) = d.endpoint.parse::<iroh::PublicKey>() else {
                    continue;
                };
                let Ok(addr) = addr_for(key.as_bytes(), d.relay.as_deref()) else {
                    continue;
                };
                let hosts = core.host_addrs(&gid);
                let (ep, auth) = (ep.clone(), auth.clone());
                tokio::spawn(async move {
                    let res: Result<(), Error> = async {
                        let conn = tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, GROUP_ALPN))
                            .await
                            .map_err(|_| Error::Timeout)?
                            .map_err(Error::net)?;
                        let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
                        let mut w = Writer(send);
                        let mut r = Reader::new(recv);
                        w.send(&GroupMsg::Moved {
                            auth,
                            group: gid,
                            hosts,
                        })
                        .await?;
                        let _ = tokio::time::timeout(HELLO_TIMEOUT, r.recv()).await;
                        conn.close(0u32.into(), b"moved");
                        Ok(())
                    }
                    .await;
                    if let Err(e) = res {
                        tracing::debug!("telling a member we moved: {e}");
                    }
                });
            }
            tokio::time::sleep(Duration::from_secs(wait)).await;
            wait = (wait * 2).min(30);
        }
    }

    /// `(group, member device)` pairs to tell we moved: current members of groups whose chat
    /// we host, in our directory, without a session with us.
    fn group_moved_targets(&self, core: &GroupCore) -> Vec<(String, DirEntry)> {
        let groups = core.groups.lock();
        let mut out = Vec::new();
        for (gid, l) in groups.iter() {
            if !l.state.is_host(&core.did, &core.dev) {
                continue;
            }
            for (k, d) in core.store.dir(gid).unwrap_or_default() {
                let Some((did, dev)) = k.rsplit_once('/') else {
                    continue;
                };
                let Some(dev) = hex::decode(dev)
                    .ok()
                    .and_then(|b| <[u8; 32]>::try_from(b).ok())
                else {
                    continue;
                };
                if l.state.is_member(did)
                    && !l.state.is_removed(did)
                    && !(did == core.did && dev == core.dev)
                    && !core.has_session(gid, did, &dev)
                {
                    out.push((gid.clone(), d));
                }
            }
        }
        out
    }

    /// Member side: a host says it moved. Kept only from an active host proving its endpoint.
    pub(super) fn group_moved_in(
        &self,
        core: &Arc<GroupCore>,
        remote: [u8; 32],
        auth: EndpointAuth,
        gid: &str,
        hosts: &[SignedHostAddr],
    ) {
        let Some(auth) = verify_endpoint(&auth, &remote) else {
            return;
        };
        let ok = core
            .groups
            .lock()
            .get(gid)
            .is_some_and(|l| l.state.is_host(&auth.did, &auth.dev));
        if !ok {
            return;
        }
        let mine = hosts.iter().any(|s| {
            s.open()
                .ok()
                .is_some_and(|a| a.did == auth.did && a.dev == auth.dev && a.endpoint == remote)
        });
        if !mine {
            return;
        }
        core.merge_hosts_from(gid, hosts, None);
        core.backoff
            .lock()
            .remove(&(gid.to_string(), host_key(&auth.did, &auth.dev)));
        self.log(format!(
            "group {}: host {} moved",
            crate::group::short(gid),
            auth.did
        ));
        core.kick.notify_waiters();
    }
}
