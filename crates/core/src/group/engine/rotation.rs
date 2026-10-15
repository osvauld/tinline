//! Group endpoint rotation and authenticated moved-host announcements.

use super::*;

impl Inner {
    // ---- endpoint rotation (U12) ---------------------------------------------------------------

    /// When this device's group endpoint is due to rotate: the earliest trigger (removal or
    /// host revocation) in a group we host that happened after the key was made, plus the
    /// delay. `None` if nothing is pending.
    pub(crate) fn group_rotation_due(&self, core: &GroupCore) -> Option<u64> {
        let made = core.store.endpoint_made();
        let delay = self.group.rotation_delay();
        core.groups
            .lock()
            .values()
            .filter(|l| hosts_or_calls(&l.state, &core.did, &core.dev))
            .flat_map(|l| {
                // A call host that does not host chat moves half a delay after the chat hosts:
                // it learns their new endpoints through their `Moved` at its old one, then
                // shows its own new address in its next Hello (GA-I1). Both at once would leave
                // each knowing only the other's closed endpoint.
                let extra = if l.state.is_host(&core.did, &core.dev) {
                    0
                } else {
                    delay / 2
                };
                l.state
                    .rotation_triggers
                    .iter()
                    .filter(|t| **t > 0 && **t as u64 > made)
                    .map(move |t| *t as u64 + delay + extra)
                    .collect::<Vec<_>>()
            })
            .min()
    }

    pub(super) async fn group_rotation_tick(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
    ) -> Result<(), Error> {
        let Some(due) = self.group_rotation_due(core) else {
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

    /// New group endpoint key; the old endpoint closes (every session with it ends, and whoever
    /// knew only it, a removed member, can no longer reach us). Then we tell the members we
    /// host where we are now, and dial the other hosts from the new endpoint.
    pub(crate) async fn group_rotate(self: &Arc<Self>, core: &Arc<GroupCore>) -> Result<(), Error> {
        core.store.rotate_endpoint_secret(now_ms())?;
        let old = core.ep.lock().await.take();
        *core.ep_id.lock() = None;
        {
            let mut ss = core.sessions.lock();
            for s in ss.values() {
                let _ = s.tx.send(Out::Close);
            }
            ss.clear();
        }
        core.my_addr.lock().clear();
        if let Some(ep) = old {
            let _ = tokio::time::timeout(Duration::from_secs(3), ep.close()).await;
        }
        let ep = self.group_bind(core).await?;
        self.group_relay_ready().await;
        self.log(format!("group endpoint rotated to {}", ep.id()));
        let hosted: Vec<(String, bool)> = core
            .groups
            .lock()
            .iter()
            .filter(|(_, l)| hosts_or_calls(&l.state, &core.did, &core.dev))
            .map(|(g, l)| (g.clone(), l.state.is_host(&core.did, &core.dev)))
            .collect();
        for (gid, chat_host) in hosted {
            let _ = self.group_my_addr(core, &gid);
            // A call host that does not host chat has no member directory: its new address
            // reaches the members through the chat hosts (its next Hello, relayed as `Hosts`).
            if chat_host && core.moving.lock().insert(gid.clone()) {
                let (this, core) = (self.clone(), core.clone());
                self.handle
                    .spawn(async move { this.group_tell_moved(core, gid).await });
            }
        }
        core.backoff.lock().clear();
        core.kick.notify_waiters();
        Ok(())
    }

    /// Dials every current member device in our directory that has no session with us yet and
    /// hands it our new address (`Moved`), with backoff, until all are back, the endpoint
    /// changes again, or the delay period has passed (PROVISIONAL: members offline longer must
    /// get a fresh token).
    pub(super) async fn group_tell_moved(self: Arc<Self>, core: Arc<GroupCore>, gid: String) {
        let Some(ep) = core.ep_id.lock().clone() else {
            core.moving.lock().remove(&gid);
            return;
        };
        let end = Instant::now() + Duration::from_millis(self.group.moved_window());
        let mut wait = 2u64;
        loop {
            let same = core
                .ep_id
                .lock()
                .as_ref()
                .is_some_and(|e| e.id() == ep.id());
            if core.closed() || !same || Instant::now() > end {
                break;
            }
            let targets = {
                let groups = core.groups.lock();
                let Some(l) = groups.get(&gid) else { break };
                if !l.state.is_host(&core.did, &core.dev) {
                    break;
                }
                let dir = core.store.dir(&gid).unwrap_or_default();
                let t: Vec<(String, DirEntry)> = dir
                    .into_iter()
                    .filter(|(k, _)| {
                        let Some((did, dev)) = k.rsplit_once('/') else {
                            return false;
                        };
                        let Some(dev) = hex::decode(dev)
                            .ok()
                            .and_then(|b| <[u8; 32]>::try_from(b).ok())
                        else {
                            return false;
                        };
                        l.state.is_member(did)
                            && !l.state.is_removed(did)
                            && !(did == core.did && dev == core.dev)
                            && !core.has_session(&gid, did, &dev)
                    })
                    .collect();
                t
            };
            if targets.is_empty() {
                break;
            }
            let hosts = core.host_addrs(&gid);
            let auth = EndpointAuth::new(&core.account, core.dev, *ep.id().as_bytes());
            for (_, d) in targets {
                let Ok(key) = d.endpoint.parse::<iroh::PublicKey>() else {
                    continue;
                };
                let Ok(addr) = addr_for(key.as_bytes(), d.relay.as_deref()) else {
                    continue;
                };
                let (ep, auth, gid, hosts) = (ep.clone(), auth.clone(), gid.clone(), hosts.clone());
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
        core.moving.lock().remove(&gid);
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
        if !auth.verify(&remote) {
            return;
        }
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
            &gid[..8.min(gid.len())],
            auth.did
        ));
        core.kick.notify_waiters();
    }
}
