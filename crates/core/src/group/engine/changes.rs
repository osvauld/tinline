//! Post-commit events, mail relay, and membership enforcement.

use super::*;

impl Inner {
    /// After a change: events, removal enforcement, relay to every other session, host
    /// announcements.
    pub(crate) fn group_after(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        changed: Changed,
        from: Option<u64>,
    ) {
        let gid = changed.gid.clone();
        // Sign receipt intents the change committed (bounded; failures stay queued) and push
        // them; receipts that waited for these docs are asked for again.
        core.member_receipts_pump_and_push();
        self.search_group(&gid, &changed.posts);
        // Order matters: the records first (a removed member gets the flags that removed it,
        // a new host's acceptance reaches others before its address), then enforcement, then
        // address announcements.
        let days: Vec<String> = changed.doc.day().map(str::to_string).into_iter().collect();
        self.group_push(core, &gid, from, &days);
        self.group_enforce(core, &gid);
        core.receipts_rehello(&gid);
        // Became (or stopped being) a host: announce our address.
        let mine = self.group_my_addr(core, &gid);
        let flipped = {
            let mut announced = core.announced.lock();
            if mine.is_some() {
                announced.insert(gid.clone())
            } else {
                announced.remove(&gid)
            }
        };
        if flipped {
            // GF: `group_push` above skipped `from`, and that session may not have the record
            // that makes us a host yet (it is in flight from a concurrent `group_after`); send it
            // first, or the receiver drops our address as not a host's and never sees it again.
            self.group_push(core, &gid, None, &[]);
            let hosts = core.host_addrs(&gid);
            for s in core.sessions_of(&gid) {
                let _ = s.tx.send(Out::Msg(GroupMsg::Hosts {
                    hosts: hosts.clone(),
                }));
            }
        }
        core.kick.notify_waiters();
        // Our other devices learn the group list (stage 2; deduplicated per session).
        self.selfsync_dirty(crate::chat::own::GROUPS_MARK);
        self.selfsync_dirty(&crate::group::own::contents::doc_name(&gid, &changed.doc));
        self.group_publish_key(core, &gid);
        // G3: invitees that became members read Joined.
        self.group_invitations_changed(core, &gid);
        if let Some(ev) = self.group_ev() {
            let msgs: Vec<_> = if changed.history {
                Vec::new()
            } else {
                changed
                    .new_items
                    .iter()
                    .filter_map(|i| self.group_message(core, &gid, i))
                    .collect()
            };
            let handle = self.handle.clone();
            handle.spawn_blocking(move || {
                for m in msgs {
                    ev.on_group_message(m);
                }
                ev.on_group_changed(gid);
            });
        }
    }

    /// As a member, publish our encryption key in the group once per device (U3 mail needs it).
    /// Every device writes at least this one meta record: posts live in day docs, and the
    /// member's device list ("appoint a host") comes from the meta doc.
    pub(super) fn group_publish_key(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str) {
        let key = core.account.encryption_public_key();
        let need = core.groups.lock().get(gid).is_some_and(|l| {
            l.state.owner.is_some()
                && l.state.is_member(&core.did)
                && !l.state.is_removed(&core.did)
                && (l.state.enc_keys.get(&core.did) != Some(&key)
                    || !l
                        .state
                        .members
                        .get(&core.did)
                        .is_some_and(|m| m.devices.contains(&core.dev)))
        });
        if need {
            match core.write(gid, Body::EncKey { key }) {
                Ok((_, c)) => self.group_after(core, c, None),
                Err(e) => tracing::warn!("group key: {e}"),
            }
        }
    }

    /// Sends sealed mail of ours: as a host we take it in ourselves; otherwise it waits in the
    /// outbox and goes to every host we have a session with (again on each new session) until
    /// one acknowledges it.
    pub(crate) fn group_send_mail(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        sm: crate::group::mail::SignedMail,
    ) {
        let bytes = sm.to_bytes();
        let hosting = core
            .groups
            .lock()
            .get(gid)
            .is_some_and(|l| l.state.is_host(&core.did, &core.dev));
        if hosting {
            self.group_mail_intake(core, gid, &bytes, None);
            return;
        }
        let Some(m) = sm.open().ok() else { return };
        if let Err(e) = core
            .store
            .put_out(gid, &crate::group::mail::id_hex(&m.id), &bytes)
        {
            tracing::warn!("group mail: {e}");
        }
        for s in core.sessions_of(gid) {
            if !s.hosting {
                let _ = s.tx.send(Out::Msg(GroupMsg::Mail {
                    mail: bytes.clone(),
                }));
            }
        }
    }

    /// Mail from a session (`from`) or from ourselves (`None`). Members may send only their own
    /// mail; sender and recipient must be members, the sender not removed. We open what is ours;
    /// as a host we keep the rest for its recipient and pass it to the recipient's sessions and
    /// to other hosts.
    pub(super) fn group_mail_intake(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        bytes: &[u8],
        from: Option<&Arc<Sess>>,
    ) {
        use crate::group::mail::*;
        let Some(sm) = SignedMail::from_bytes(bytes).ok() else {
            return;
        };
        let Ok(m) = sm.open() else { return };
        if m.group != gid {
            return;
        }
        let (we_host, ok, from_host) = {
            let groups = core.groups.lock();
            let Some(l) = groups.get(gid) else { return };
            let s = &l.state;
            let from_host = from.is_some_and(|x| s.is_host(&x.did, &x.dev));
            (
                s.is_host(&core.did, &core.dev),
                s.is_member(&m.from) && !s.is_removed(&m.from) && s.is_member(&m.to),
                from_host,
            )
        };
        if let Some(x) = from
            && !from_host
            && x.did != m.from
        {
            return;
        }
        let id = id_hex(&m.id);
        if we_host
            && let Some(x) = from
            && !from_host
        {
            // Taken (or refused for good): the sender stops resending either way.
            let _ = x.tx.send(Out::Msg(GroupMsg::MailAck { id: m.id }));
        }
        if !ok {
            return;
        }
        if m.to == core.did {
            self.group_mail_for_me(core, gid, &sm);
        }
        if !we_host {
            return;
        }
        let now = now_ms();
        if core
            .store
            .mail(gid, Some(&m.to), now)
            .map(|v| v.len())
            .unwrap_or(0)
            >= MAX_PENDING_PER_RECIPIENT
        {
            return;
        }
        let fresh = core
            .store
            .put_mail(gid, &m.to, &id, m.at.min(now) + MAIL_TTL_MS, bytes)
            .unwrap_or(false);
        if !fresh {
            return;
        }
        let hosts: Vec<(String, [u8; 32])> = core
            .groups
            .lock()
            .get(gid)
            .map(|l| {
                l.state
                    .hosts
                    .iter()
                    .map(|h| (h.did.clone(), h.dev))
                    .collect()
            })
            .unwrap_or_default();
        for s in core.sessions_of(gid) {
            if from.is_some_and(|x| x.id == s.id) {
                continue;
            }
            let to_recipient = s.hosting && s.did == m.to;
            let to_host = hosts.contains(&(s.did.clone(), s.dev));
            if to_recipient || to_host {
                let _ = s.tx.send(Out::Msg(GroupMsg::Mail {
                    mail: bytes.to_vec(),
                }));
            }
        }
    }

    /// A new session: our outbox to a host; to a member we host, its waiting mail; to another
    /// host, all mail we keep.
    pub(super) fn group_mail_on_session(&self, core: &GroupCore, sess: &Sess) {
        let (we_host, peer_host) = core
            .groups
            .lock()
            .get(&sess.gid)
            .map(|l| {
                (
                    l.state.is_host(&core.did, &core.dev),
                    l.state.is_host(&sess.did, &sess.dev),
                )
            })
            .unwrap_or_default();
        let mut out: Vec<Vec<u8>> = Vec::new();
        if !sess.hosting {
            out.extend(core.store.outbox(&sess.gid).unwrap_or_default());
        }
        if we_host {
            let now = now_ms();
            if peer_host {
                out.extend(
                    core.store
                        .mail(&sess.gid, None, now)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(_, b)| b),
                );
            } else if sess.hosting {
                out.extend(
                    core.store
                        .mail(&sess.gid, Some(&sess.did), now)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(_, b)| b),
                );
            }
        }
        for mail in out {
            let _ = sess.tx.send(Out::Msg(GroupMsg::Mail { mail }));
        }
    }

    /// Host side: a session whose member is now removed gets the signed proof and is closed.
    /// Sessions with a device that no longer hosts, or (when we stopped hosting) with the
    /// members we served, are closed.
    pub(super) fn group_enforce(&self, core: &GroupCore, gid: &str) {
        let removed: Vec<(Arc<Sess>, Vec<Vec<u8>>, Vec<u8>, Option<Vec<u8>>)> = {
            let groups = core.groups.lock();
            let Some(live) = groups.get(gid) else { return };
            if live.state.owner.is_none() {
                return;
            }
            let we_host = live.state.is_host(&core.did, &core.dev);
            for s in core.sessions_of(gid) {
                let stale = if s.hosting {
                    !we_host && !live.state.is_removed(&s.did)
                } else {
                    !live.state.is_host(&s.did, &s.dev)
                };
                if stale {
                    let _ = s.tx.send(Out::Close);
                    core.sessions.lock().remove(&s.id);
                }
            }
            core.sessions_of(gid)
                .into_iter()
                .filter(|s| s.hosting && live.state.is_removed(&s.did))
                .map(|s| {
                    let proof = core.removal_proof(live, &s.did);
                    let upto = core.removal_upto(live, &s.did, &s.peer_vv.lock());
                    (s, proof, live.doc.vv().encode(), upto)
                })
                .collect()
        };
        let gone: Vec<String> = core
            .groups
            .lock()
            .get(gid)
            .map(|l| l.state.removed.keys().cloned().collect())
            .unwrap_or_default();
        // D12: the removed member's upload window starts when we first learn of the removal.
        if let Err(e) = core.store.sync_removals_seen(gid, &gone, now_ms()) {
            tracing::warn!("group {}: removal times: {e}", crate::group::short(gid));
        }
        for did in gone {
            let _ = core.store.drop_dir(gid, &did);
        }
        for (s, records, vv, update) in removed {
            let _ = s.tx.send(Out::Msg(GroupMsg::Removed {
                records,
                vv,
                update,
            }));
            let _ = s.tx.send(Out::Close);
            core.sessions.lock().remove(&s.id);
        }
    }

    /// Sends every session of `gid` (except `from`) what it lacks of the meta doc and `days`.
    pub(super) fn group_push(
        &self,
        core: &GroupCore,
        gid: &str,
        from: Option<u64>,
        days: &[String],
    ) {
        core.push(gid, from, days);
    }
}
