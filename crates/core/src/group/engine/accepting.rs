//! Host-side group admission and pre-removal uploads.

use super::*;

impl Inner {
    // ---- accepting (host side) ---------------------------------------------------------------

    pub(super) async fn group_accept_loop(self: Arc<Self>, core: Arc<GroupCore>, ep: Endpoint) {
        while let Some(incoming) = ep.accept().await {
            if core.closed() {
                break;
            }
            let (this, core) = (self.clone(), core.clone());
            tokio::spawn(async move {
                let Ok(mut accepting) = incoming.accept() else {
                    return;
                };
                let Ok(Ok(alpn)) = tokio::time::timeout(HELLO_TIMEOUT, accepting.alpn()).await
                else {
                    return;
                };
                // Group calls (G6) run on the group endpoint (U2/U7): `crate::gcall::net`.
                if alpn.as_slice() == proto::gcall::GCALL_ALPN {
                    if let Err(e) = this.handle_gcall_incoming(accepting).await {
                        tracing::debug!("incoming gcall: {e}");
                    }
                    return;
                }
                let Ok(conn) = tokio::time::timeout(HELLO_TIMEOUT, accepting).await else {
                    return;
                };
                let Ok(conn) = conn else { return };
                if let Err(e) = this.group_host_session(core, conn).await {
                    tracing::debug!("group incoming: {e}");
                }
            });
        }
    }

    pub(super) async fn group_host_session(
        self: Arc<Self>,
        core: Arc<GroupCore>,
        conn: Connection,
    ) -> Result<(), Error> {
        let remote = *conn.remote_id().as_bytes();
        let (send, recv) = tokio::time::timeout(HELLO_TIMEOUT, conn.accept_bi())
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(Error::net)?;
        let mut writer = Writer(send);
        let mut reader = Reader::new(recv);
        let hello = tokio::time::timeout(HELLO_TIMEOUT, reader.recv())
            .await
            .map_err(|_| Error::Timeout)??;
        let (auth, group, vv, days, invite, hosts, relay) = match hello {
            Some(GroupMsg::Hello {
                auth,
                group,
                vv,
                days,
                invite,
                hosts,
                relay,
            }) => (auth, group, vv, days, invite, hosts, relay),
            Some(GroupMsg::Fetch { code }) => {
                // G3: someone with a short invite link asks for the invite behind it.
                self.group_fetch_session(&core, writer, reader, code).await;
                let _ = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
                return Ok(());
            }
            Some(GroupMsg::Moved { auth, group, hosts }) => {
                self.group_moved_in(&core, remote, auth, &group, &hosts);
                let _ = writer
                    .send(&GroupMsg::Ack {
                        doc: DocId::Meta,
                        vv: Vec::new(),
                    })
                    .await;
                let _ = writer.0.finish();
                let _ = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
                return Ok(());
            }
            _ => return Err(Error::Protocol("expected hello".into())),
        };
        if !auth.verify(&remote) {
            return Err(Error::Protocol("endpoint proof does not verify".into()));
        }
        enum Verdict {
            Refuse(&'static str),
            Removed(Vec<Vec<u8>>, Vec<u8>, Option<Vec<u8>>),
            Welcome { joining: bool },
        }
        let verdict = {
            let groups = core.groups.lock();
            match groups.get(&group) {
                None => Verdict::Refuse("unknown group"),
                Some(live) if !live.state.is_host(&core.did, &core.dev) => {
                    Verdict::Refuse("not a host of this group")
                }
                Some(live) if live.state.is_removed(&auth.did) => {
                    let upto =
                        core.removal_upto(live, &auth.did, &decode_vv(&vv).unwrap_or_default());
                    Verdict::Removed(
                        core.removal_proof(live, &auth.did),
                        live.doc.vv().encode(),
                        upto,
                    )
                }
                Some(live) if live.state.is_member(&auth.did) => {
                    Verdict::Welcome { joining: false }
                }
                Some(live) => {
                    let ok = invite.as_ref().and_then(|i| i.open()).is_some_and(|i| {
                        i.group == group && i.expires >= now() && live.state.is_admin(&i.issuer)
                    });
                    if ok {
                        Verdict::Welcome { joining: true }
                    } else {
                        Verdict::Refuse("not a member")
                    }
                }
            }
        };
        match verdict {
            Verdict::Refuse(why) => {
                writer
                    .send(&GroupMsg::Refused { reason: why.into() })
                    .await?;
                let _ = writer.0.finish();
                let _ = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
                Ok(())
            }
            Verdict::Removed(records, vv, update) => {
                writer
                    .send(&GroupMsg::Removed {
                        records,
                        vv,
                        update,
                    })
                    .await?;
                self.group_removed_upload(&core, &group, &auth.did, &mut writer, &mut reader)
                    .await;
                let _ = writer.0.finish();
                let _ = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
                Ok(())
            }
            Verdict::Welcome { joining } => {
                // U7: the host keeps the member device's group endpoint.
                let relay = relay.filter(|r| r.len() <= 300);
                let entry = DirEntry {
                    endpoint: conn.remote_id().to_string(),
                    seen_ms: now_ms(),
                    relay,
                };
                let _ = core.store.put_dir(&group, &auth.did, &auth.dev, &entry);
                // A host (or call host) that rotated shows its new address in its Hello: pass it
                // on to everyone else at once (records first, then the addresses).
                core.merge_hosts_from(&group, &hosts, None);
                let mine = EndpointAuth::new(
                    &core.account,
                    core.dev,
                    *core
                        .ep_id
                        .lock()
                        .as_ref()
                        .map(|e| e.id())
                        .ok_or(Error::NotStarted)?
                        .as_bytes(),
                );
                let our_vv = core
                    .groups
                    .lock()
                    .get(&group)
                    .map(|l| l.doc.vv().encode())
                    .unwrap_or_default();
                let hosts = core.host_addrs(&group);
                let our_days = core.hello_days(&group);
                writer
                    .send(&GroupMsg::Welcome {
                        auth: mine,
                        vv: our_vv,
                        days: our_days,
                        hosts,
                    })
                    .await?;
                let peer = Peer::new(&vv, &days)?;
                self.group_run(core, conn, writer, reader, group, auth, true, joining, peer)
                    .await
            }
        }
    }

    /// A removed member may upload only its own posts whose signed frontier predates its removal
    /// (U11 option A); anything else is refused and nothing new is sent to it. It sends one
    /// `Update` per day doc it owes and finishes; each is answered by an `Ack` of that doc.
    pub(super) async fn group_removed_upload(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        did: &str,
        writer: &mut Writer,
        reader: &mut Reader,
    ) {
        while let Ok(Ok(Some(GroupMsg::Update { doc, update }))) =
            tokio::time::timeout(REMOVED_UPLOAD_WAIT, reader.recv()).await
        {
            let did_owned = did.to_string();
            let res = core.apply(gid, &doc, &update, move |state, entries, fresh| {
                let flag_entries: Vec<&Entry> = state
                    .removed
                    .get(&did_owned)
                    .map(|f| {
                        f.iter()
                            .filter_map(|fi| entries.iter().find(|e| e.id == fi.id))
                            .collect()
                    })
                    .unwrap_or_default();
                for e in fresh {
                    let before = flag_entries
                        .iter()
                        .filter(|f| f.seen_by(&e.rec.seen))
                        .count()
                        < crate::group::state::REMOVAL_FLAGS;
                    if e.rec.author != did_owned
                        || !matches!(e.rec.body, Body::Post { .. })
                        || !before
                    {
                        return Err(
                            "removed: only posts written before the removal are accepted".into(),
                        );
                    }
                }
                Ok(())
            });
            match res {
                Ok(changed) => {
                    let vv = core.doc_vv(gid, &doc).encode();
                    let _ = writer.send(&GroupMsg::Ack { doc, vv }).await;
                    if let Some(c) = changed {
                        self.group_after(core, c, None);
                    }
                }
                Err(e) => {
                    self.log(format!(
                        "group {gid}: refused a removed member's upload: {e}"
                    ));
                    let _ = writer
                        .send(&GroupMsg::Refused {
                            reason: e.to_string(),
                        })
                        .await;
                    return;
                }
            }
        }
    }
}
