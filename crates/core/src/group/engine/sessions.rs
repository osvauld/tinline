//! Authenticated group-session execution and incoming frames.

use super::*;

impl Inner {
    // ---- one session (both sides) --------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn group_run(
        self: Arc<Self>,
        core: Arc<GroupCore>,
        conn: Connection,
        mut writer: Writer,
        mut reader: Reader,
        gid: String,
        peer: proto::gcall::GroupNote,
        hosting: bool,
        joining: bool,
        open_join: bool,
        show: bool,
        theirs: Peer,
    ) -> Result<(), Error> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let id = core.next.fetch_add(1, Ordering::SeqCst);
        let sess = Arc::new(Sess {
            id,
            gid: gid.clone(),
            did: peer.did.clone(),
            dev: peer.dev,
            ep: *conn.remote_id().as_bytes(),
            hosting,
            joining,
            open_join,
            show,
            tx,
            peer_vv: Mutex::new(theirs.vv),
            peer_days: Mutex::new(theirs.days),
            inv_done: AtomicBool::new(false),
            receipt_vv: Default::default(),
            receipt_waiting: Default::default(),
        });
        core.sessions.lock().insert(id, sess.clone());
        self.log(format!(
            "group {} session with {} ({})",
            crate::group::short(&gid),
            &peer.did,
            if hosting { "we host" } else { "they host" }
        ));
        // Catch the other side up: the meta doc, then the recent days (older ones are history).
        self.group_push(&core, &gid, None, &core.held_recent_days(&gid));
        // GF: the host addresses again, after the records. Those in Hello/Welcome are merged
        // before this session's records arrive, so an address of a host the other side does not
        // yet know as one (e.g. accepted while we were apart) was dropped there and never re-sent;
        // here it follows the records that make it valid on the same ordered stream.
        let hosts = core.host_addrs(&gid);
        if !hosts.is_empty() {
            let _ = sess.tx.send(Out::Msg(GroupMsg::Hosts { hosts }));
        }
        // Member receipts: after the docs, so the posts they cover arrive first (none in an open
        // group, public group §4).
        if let Some(hello) = core.session_receipt_hello(&gid) {
            let _ = sess.tx.send(Out::Msg(hello));
        }
        self.group_mail_on_session(&core, &sess);
        self.group_calls_on_session(&core, &sess);
        self.group_presence_on_session(&core, &sess);
        self.group_images_on_session(&core, &gid);
        if !sess.hosting {
            self.group_fail_refused(&core, &gid, false);
        }
        if let Some(ev) = self.group_ev() {
            let g = gid.clone();
            self.handle.spawn_blocking(move || ev.on_group_changed(g));
        }
        let writer_task = tokio::spawn(async move {
            while let Some(out) = rx.recv().await {
                match out {
                    Out::Msg(m) => {
                        if writer.send(&m).await.is_err() {
                            break;
                        }
                    }
                    Out::Close => break,
                }
            }
            let _ = writer.0.finish();
        });
        // Both sides exchange the day docs the other lacks, newest first, in the background.
        let backfill = tokio::spawn(self.clone().group_days_sync(core.clone(), sess.clone()));
        let res = self.group_read(&core, &sess, &mut reader).await;
        core.sessions.lock().remove(&id);
        self.group_presence_session_end(&core, &sess);
        backfill.abort();
        writer_task.abort();
        conn.close(0u32.into(), b"bye");
        if let Some(ev) = self.group_ev() {
            self.handle.spawn_blocking(move || ev.on_group_changed(gid));
        }
        core.kick.notify_waiters();
        res
    }

    pub(super) async fn group_read(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        sess: &Arc<Sess>,
        reader: &mut Reader,
    ) -> Result<(), Error> {
        loop {
            if core.closed() {
                return Ok(());
            }
            let Some(msg) = reader.recv().await? else {
                return Ok(());
            };
            if !core.sessions.lock().contains_key(&sess.id) {
                return Ok(());
            }
            match msg {
                GroupMsg::Update { doc, update } => {
                    let res = core.apply(&sess.gid, &doc, &update, sess.admit());
                    match res {
                        Ok(changed) => {
                            // The peer holds at least what it just sent.
                            if let Ok(meta) = loro::LoroDoc::decode_import_blob_meta(&update, false)
                            {
                                sess.peer_has(&doc, &meta.partial_end_vv);
                            }
                            let vv = changed
                                .as_ref()
                                .map(|c| c.doc_vv.clone())
                                .unwrap_or_else(|| core.doc_vv(&sess.gid, &doc).encode());
                            let _ = sess.tx.send(Out::Msg(GroupMsg::Ack {
                                doc: doc.clone(),
                                vv,
                            }));
                            if let Some(mut c) = changed {
                                if doc == DocId::Meta {
                                    let ours = core.doc_vv(&sess.gid, &DocId::Meta);
                                    let caught_up = ours
                                        .partial_cmp(&sess.peer_vv.lock())
                                        .is_some_and(|o| o != std::cmp::Ordering::Less);
                                    self.group_maybe_join(core, &sess.gid, caught_up);
                                    if !sess.hosting {
                                        self.group_fail_refused(core, &sess.gid, false);
                                    }
                                }
                                // Older than the days a session pushes live: history catching up.
                                c.history = doc.day().is_some_and(|d| {
                                    !recent_days(now_ms() as i64, HELLO_DAYS)
                                        .iter()
                                        .any(|r| r == d)
                                });
                                self.group_after(core, c, Some(sess.id));
                            }
                        }
                        Err(Reject::Persistence(reason)) => {
                            // Retryable storage failure, not a membership refusal.
                            // group_run closes this session; no Ack or fan-out.
                            return Err(Error::Io(reason));
                        }
                        Err(Reject::Policy(r)) if r == admit::ADMINS_ONLY => {
                            // A non-admin's post in an admins-only group: refuse the batch, keep
                            // the session; the member marks such posts failed and stops sending.
                            self.log(format!(
                                "group {}: refused an update from {}: {r}",
                                crate::group::short(&sess.gid),
                                sess.did
                            ));
                            let _ = sess.tx.send(Out::Msg(GroupMsg::Refused { reason: r }));
                        }
                        Err(e) => {
                            self.log(format!(
                                "group {}: refused an update from {}: {e}",
                                crate::group::short(&sess.gid),
                                sess.did
                            ));
                            let _ = sess.tx.send(Out::Msg(GroupMsg::Refused {
                                reason: e.to_string(),
                            }));
                            let _ = sess.tx.send(Out::Close);
                            return Err(Error::Rejected(e.to_string()));
                        }
                    }
                }
                GroupMsg::Ack { doc, vv } => {
                    if !doc.is_valid() {
                        return Err(Error::Protocol("ack for a bad doc".into()));
                    }
                    sess.peer_has(&doc, &decode_vv(&vv)?);
                    if !sess.hosting {
                        self.group_acked(core, &sess.gid, &sess.did, &sess.dev, &doc, &vv);
                        if let Some(ev) = self.group_ev() {
                            let g = sess.gid.clone();
                            self.handle.spawn_blocking(move || ev.on_group_changed(g));
                        }
                    }
                }
                GroupMsg::Days { days, .. } => {
                    if days.len() > DAYS_PAGE {
                        return Err(Error::Protocol("too many days in a page".into()));
                    }
                    if days.is_empty() {
                        sess.inv_done.store(true, Ordering::SeqCst);
                    }
                    for (d, v) in days {
                        if !DocId::Day(d.clone()).is_valid() {
                            return Err(Error::Protocol("bad day in inventory".into()));
                        }
                        sess.peer_has(&DocId::Day(d), &decode_vv(&v)?);
                    }
                }
                GroupMsg::Hosts { hosts } => {
                    core.merge_hosts_from(&sess.gid, &hosts, Some(sess.id));
                }
                GroupMsg::Refused { reason } if !sess.hosting && reason == admit::ADMINS_ONLY => {
                    self.group_fail_refused(core, &sess.gid, true);
                }
                GroupMsg::Refused { reason } => {
                    return Err(Error::Rejected(reason));
                }
                GroupMsg::Mail { mail } => {
                    self.group_mail_intake(core, &sess.gid, &mail, Some(sess));
                }
                GroupMsg::MailAck { id } => {
                    if !sess.hosting {
                        let _ = core
                            .store
                            .drop_out(&sess.gid, &crate::group::mail::id_hex(&id));
                    }
                }
                m @ (GroupMsg::CallHostStatus { .. }
                | GroupMsg::CallStart { .. }
                | GroupMsg::CallEnd { .. }
                | GroupMsg::CallHostRequest { .. }
                | GroupMsg::CallHostRefusal { .. }) => {
                    self.group_call_msg(core, &sess.gid, m, Some(sess));
                }
                m @ (GroupMsg::Code { .. } | GroupMsg::RevokeCode { .. }) if sess.hosting => {
                    // G3: a member has us keep (or forget) an invite code; members only.
                    let ack = self.group_code_in(core, &sess.gid, &sess.did, m);
                    let _ = sess.tx.send(Out::Msg(ack));
                }
                GroupMsg::CodeAck { code, ok, reason } if !sess.hosting => {
                    self.group_code_ack_in(core, code, ok, reason);
                }
                GroupMsg::ReceiptHello { days } => {
                    core.on_receipt_hello(sess, days)?;
                }
                GroupMsg::Receipts { day, update } => {
                    if !core.on_receipts(sess, &day, &update)?.is_empty()
                        && let Some(ev) = self.group_ev()
                    {
                        let g = sess.gid.clone();
                        self.handle.spawn_blocking(move || ev.on_group_changed(g));
                    }
                }
                GroupMsg::ReceiptAck { day, vv } => {
                    core.on_receipt_ack(sess, &day, &vv)?;
                }
                GroupMsg::Presence { presence } => {
                    self.group_presence_in(core, sess, presence);
                }
                _ => return Err(Error::Protocol("unexpected group frame".into())),
            }
        }
    }
}
