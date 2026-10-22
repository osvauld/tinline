//! Member-side host selection, backoff, and outgoing group sessions.

use super::*;

impl Inner {
    // ---- dialing (member side) ----------------------------------------------------------------

    pub(super) async fn group_dial_loop(self: Arc<Self>, core: Arc<GroupCore>) {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(TICK) => {}
                _ = core.kick.notified() => {}
            }
            if core.closed() {
                break;
            }
            if core
                .compacted
                .lock()
                .is_none_or(|t| t.elapsed() >= COMPACT_EVERY)
            {
                *core.compacted.lock() = Some(Instant::now());
                let (this, core) = (self.clone(), core.clone());
                self.handle.spawn_blocking(move || {
                    this.group_compact(&core);
                });
            }
            self.group_req_tick(&core);
            self.group_calls_tick(&core);
            self.group_codes_tick(&core);
            self.group_presence_tick(&core);
            self.group_images_pump(&core);
            if let Err(e) = self.group_rotation_tick(&core).await {
                tracing::warn!("group endpoint rotation: {e}");
            }
            for (gid, addr) in self.group_targets(&core) {
                let key = (gid.clone(), host_key(&addr.did, &addr.dev));
                if !core.dialing.lock().insert(key.clone()) {
                    continue;
                }
                let (this, core) = (self.clone(), core.clone());
                tokio::spawn(async move {
                    let ok = this
                        .clone()
                        .group_dial(core.clone(), gid.clone(), addr)
                        .await
                        .is_ok();
                    core.dialing.lock().remove(&key);
                    let mut b = core.backoff.lock();
                    if ok {
                        b.remove(&key);
                    } else {
                        let n = b
                            .get(&key)
                            .map(|(_, n)| n + 1)
                            .unwrap_or(0)
                            .min(BACKOFF.len() - 1);
                        b.insert(key, (Instant::now() + Duration::from_secs(BACKOFF[n]), n));
                    }
                });
            }
        }
    }

    /// Hosts to dial now: active hosts of each group we belong to (any validly signed address
    /// from the invite while we are still joining), minus ourselves, live sessions, dials in
    /// flight and those backing off.
    pub(super) fn group_targets(&self, core: &GroupCore) -> Vec<(String, HostAddr)> {
        let only = self.group.dial_only.lock().clone();
        let ignore = self.group.ignore_removal.load(Ordering::SeqCst);
        let mut out = Vec::new();
        let groups: Vec<(String, bool, bool, Vec<(String, [u8; 32])>, bool)> = core
            .groups
            .lock()
            .iter()
            .map(|(g, l)| {
                let joining = l.state.owner.is_none();
                let gone =
                    !joining && !l.state.is_member(&core.did) && !l.state.is_removed(&core.did);
                let removed = l.state.is_removed(&core.did) || !l.meta.removal_proof.is_empty();
                let hosts = l
                    .state
                    .hosts
                    .iter()
                    .map(|h| (h.did.clone(), h.dev))
                    .collect();
                (g.clone(), joining, gone, hosts, removed)
            })
            .collect();
        let now = Instant::now();
        for (gid, joining, gone, hosts, removed) in groups {
            // A member that left stops dialing once its leave reached a host.
            if gone && !self.group_owes(core, &gid) {
                continue;
            }
            // Removed: only to upload posts written before the removal (until acked).
            if removed && !ignore && !self.group_owes(core, &gid) {
                continue;
            }
            for s in core.store.hosts(&gid).unwrap_or_default() {
                let Some(a) = s.open().ok() else { continue };
                if a.did == core.did && a.dev == core.dev {
                    continue;
                }
                if !joining && !hosts.contains(&(a.did.clone(), a.dev)) {
                    continue;
                }
                if only.as_ref().is_some_and(|o| *o != a.did) {
                    continue;
                }
                let key = (gid.clone(), host_key(&a.did, &a.dev));
                if core.has_session(&gid, &a.did, &a.dev) || core.dialing.lock().contains(&key) {
                    continue;
                }
                if core.backoff.lock().get(&key).is_some_and(|(t, _)| *t > now) {
                    continue;
                }
                out.push((gid.clone(), a));
            }
        }
        out
    }

    /// Whether some op of ours is not yet acknowledged by any host.
    pub(super) fn group_owes(&self, core: &GroupCore, gid: &str) -> bool {
        let (mine, forwarded) = core
            .groups
            .lock()
            .get(gid)
            .map(|l| {
                (
                    l.doc.vv().get(&core.wid).copied().unwrap_or(0),
                    !l.meta.owed_days.is_empty() || !l.meta.own_owed_meta.is_empty(),
                )
            })
            .unwrap_or((0, false));
        if mine == 0 && !forwarded {
            return false;
        }
        forwarded
            || !core
                .store
                .acks(gid)
                .unwrap_or_default()
                .iter()
                .any(|(_, vv)| {
                    decode_vv(vv)
                        .ok()
                        .and_then(|v| v.get(&core.wid).copied())
                        .unwrap_or(0)
                        >= mine
                })
    }

    pub(super) async fn group_dial(
        self: Arc<Self>,
        core: Arc<GroupCore>,
        gid: String,
        host: HostAddr,
    ) -> Result<(), Error> {
        // Not `group_ensure` (the dial loop is started by it): the endpoint is up while it runs.
        let ep = core.ep_id.lock().clone().ok_or(Error::NotStarted)?;
        if injected(&self.group.fail_dials) {
            return Err(Error::Net("connection lost (injected)".into()));
        }
        let addr = addr_for(&host.endpoint, host.relay.as_deref())?;
        let conn = tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, GROUP_ALPN))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(Error::net)?;
        let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
        let mut writer = Writer(send);
        let mut reader = Reader::new(recv);
        let (vv, invite) = {
            let groups = core.groups.lock();
            let live = groups.get(&gid).ok_or(Error::NotFound)?;
            let invite = live
                .meta
                .joining
                .as_ref()
                .and_then(|b| cryptography::codec::from_bytes::<SignedInvite>(b, crate::store::MAX_ROW).ok());
            (live.doc.vv(), invite)
        };
        let auth = endpoint_auth(&core.account, core.dev, *ep.id().as_bytes());
        let days = core.hello_days(&gid);
        writer
            .send(&GroupMsg::Hello {
                auth,
                group: gid.clone(),
                vv: vv.encode(),
                days,
                invite,
                hosts: core.host_addrs(&gid),
                relay: relay_of(&ep),
                presence: core.presence.visible(),
            })
            .await?;
        let reply = tokio::time::timeout(HELLO_TIMEOUT, reader.recv())
            .await
            .map_err(|_| Error::Timeout)??;
        match reply {
            Some(GroupMsg::Welcome {
                auth,
                vv,
                days,
                hosts,
            }) => {
                let Some(auth) = verify_endpoint(&auth, conn.remote_id().as_bytes())
                    .filter(|a| a.did == host.did && a.dev == host.dev)
                else {
                    return Err(Error::Protocol("host proof does not verify".into()));
                };
                core.refusals.lock().remove(&gid);
                core.merge_hosts_from(&gid, &hosts, None);
                let peer = Peer::new(&vv, &days)?;
                self.group_run(
                    core, conn, writer, reader, gid, auth, false, false, false, false, peer,
                )
                .await
            }
            Some(GroupMsg::Removed {
                records, update, ..
            }) => {
                if let Some(u) = update {
                    match core.apply(&gid, &DocId::Meta, &u, |_, _, _| Ok(())) {
                        Ok(Some(c)) => self.group_after(&core, c, None),
                        Ok(None) => {}
                        Err(e) => tracing::debug!("group {gid}: removal records: {e}"),
                    }
                }
                let ignore = self.group.ignore_removal.load(Ordering::SeqCst);
                if !ignore && proof_removes(&gid, &core.did, &records).is_some() {
                    let mut groups = core.groups.lock();
                    if let Some(live) = groups.get_mut(&gid) {
                        live.meta.removal_proof = records;
                        let _ = core.store.put_meta(&gid, &live.meta);
                    }
                    drop(groups);
                    if let Some(ev) = self.group_ev() {
                        let g = gid.clone();
                        self.handle.spawn_blocking(move || ev.on_group_changed(g));
                    }
                }
                // Upload the posts of ours the host lacks (one update per day doc); it keeps
                // only the pre-removal ones.
                let owed: Vec<String> = core
                    .groups
                    .lock()
                    .get(&gid)
                    .map(|l| l.meta.owed_days.iter().cloned().collect())
                    .unwrap_or_default();
                for day in owed {
                    let up = {
                        let mut groups = core.groups.lock();
                        let Some(live) = groups.get_mut(&gid) else {
                            break;
                        };
                        let Live { state, shards, .. } = live;
                        shards
                            .get(&core.store, &gid, core.wid, &day, state)
                            .ok()
                            .and_then(|s| {
                                s.doc.export_peer_since(core.wid, &VersionVector::default())
                            })
                    };
                    let Some(update) = up else { continue };
                    let doc = DocId::Day(day.clone());
                    writer.send(&GroupMsg::Update { doc, update }).await?;
                    match tokio::time::timeout(Duration::from_secs(10), reader.recv()).await {
                        Ok(Ok(Some(GroupMsg::Ack { vv, .. }))) => self.group_acked(
                            &core,
                            &gid,
                            &host.did,
                            &host.dev,
                            &DocId::Day(day),
                            &vv,
                        ),
                        _ => break,
                    }
                }
                let _ = writer.0.finish();
                conn.close(0u32.into(), b"removed");
                Ok(())
            }
            Some(GroupMsg::Refused { reason }) => {
                core.refusals.lock().insert(gid.clone(), reason.clone());
                conn.close(0u32.into(), b"refused");
                // Learned from an own device (stage 2) and refused before we ever held it (the
                // account left meanwhile, or a stale list): forget it again.
                let stale = core.groups.lock().get(&gid).is_some_and(|l| {
                    l.meta.from_own && l.state.owner.is_none() && reason == "not a member"
                });
                if stale {
                    core.groups.lock().remove(&gid);
                    let _ = core.store.forget(&gid);
                    self.search.forget(&search_ix::group_src(&gid));
                }
                Err(Error::Rejected(reason))
            }
            _ => Err(Error::Protocol("unexpected reply".into())),
        }
    }

    /// We joined with an invite (or an open link) and now hold the group: write our `Join`
    /// (`OpenJoin`) once. An open join waits until we hold the admin's `Open{active: true}`, so
    /// our record names it in its frontier (otherwise it would not count). `caught_up`: we hold
    /// all of the meta doc the host had at `Welcome`, so a group that is still closed was closed
    /// before we could join: the open join ends there.
    pub(super) fn group_maybe_join(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, caught_up: bool) {
        enum How {
            /// The invite, and its stored bytes (to put back if the write fails).
            Invite(SignedInvite, Vec<u8>),
            Open,
        }
        let how = {
            let mut groups = core.groups.lock();
            let Some(live) = groups.get_mut(gid) else {
                return;
            };
            if live.state.owner.is_none() || live.state.is_member(&core.did) {
                if (live.meta.joining.is_some() || live.meta.open_join)
                    && live.state.is_member(&core.did)
                {
                    live.meta.joining = None;
                    live.meta.open_join = false;
                    let _ = core.store.put_meta(gid, &live.meta);
                }
                return;
            }
            // The flag is cleared before the write (so a concurrent call does not write twice)
            // and put back if the write fails (so it is tried again).
            if let Some(b) = live.meta.joining.take() {
                let _ = core.store.put_meta(gid, &live.meta);
                match cryptography::codec::from_bytes::<SignedInvite>(&b, crate::store::MAX_ROW) {
                    Ok(i) => How::Invite(i, b),
                    Err(_) => return,
                }
            } else if live.meta.open_join && live.state.open {
                live.meta.open_join = false;
                let _ = core.store.put_meta(gid, &live.meta);
                How::Open
            } else if live.meta.open_join && caught_up {
                // Closed before our `OpenJoin` was written: it would not count, and a host refuses
                // it. Stop here; `group_join` reports the refusal (and forgets the group).
                live.meta.open_join = false;
                let _ = core.store.put_meta(gid, &live.meta);
                drop(groups);
                core.refusals
                    .lock()
                    .insert(gid.to_string(), super::super::code::NOT_OPEN.into());
                return;
            } else {
                return;
            }
        };
        let name = self
            .me()
            .map(|m| m.profile.name.clone())
            .unwrap_or_default();
        let (body, restore) = match how {
            How::Invite(invite, b) => (Body::Join { invite, name }, Some(b)),
            How::Open => (Body::OpenJoin { name }, None),
        };
        match core.write(gid, body) {
            Ok((_, c)) => self.group_after(core, c, None),
            Err(e) => {
                tracing::warn!("group join: {e}");
                let mut groups = core.groups.lock();
                if let Some(live) = groups.get_mut(gid)
                    && !live.state.is_member(&core.did)
                {
                    match restore {
                        Some(b) => live.meta.joining = Some(b),
                        None => live.meta.open_join = true,
                    }
                    let _ = core.store.put_meta(gid, &live.meta);
                }
            }
        }
    }
}
