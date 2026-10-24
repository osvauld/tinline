//! Optional authenticated own-device archive pulls. Session-local request IDs;
//! no readiness certificate, host receipt, direct-route suppression or blob sync.
use super::*;
use group_runtime::{
    doc::{DocId, batching::BatchLimits},
    record::InviteToken,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const CAPABILITY: &str = "cap/group-history/v1";
pub const PAGE_DAYS: usize = 64;
pub const PAGE_BYTES: usize = 128 * 1024;
pub const VV_BYTES: usize = 64 * 1024;
pub const BATCH_BYTES: usize = 256 * 1024;
pub const RETRY: Duration = Duration::from_secs(5);
pub const RESCAN: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct History {
    pub enabled: bool,
    pub scopes: BTreeSet<String>,
    pub queued: VecDeque<String>,
    pub queued_set: BTreeSet<String>,
    pub resumes: BTreeMap<String, Scope>,
    pub current: Option<Scope>,
    pub docs: VecDeque<(String, VersionVector)>,
    pub awaiting: Option<Awaiting>,
    pub continue_doc: Option<String>,
    pub serial: u64,
    pub last_scan: Option<Instant>,
    pub next_issue: Option<Instant>,
}
pub struct Scope {
    pub gid: String,
    pub meta_done: bool,
    pub cursor: Option<String>,
    pub eof: bool,
    pub page_ready: bool,
}
pub struct Awaiting {
    pub message: SelfMsg,
    pub sent: Instant,
}

pub fn supported(docs: &[(String, Vec<u8>)]) -> bool {
    group_contents::supported(docs)
        && docs.iter().any(|(name, bytes)| {
            name == CAPABILITY
                && VersionVector::decode(bytes)
                    .ok()
                    .is_some_and(|vv| vv == VersionVector::default())
        })
}
pub fn valid_group(group: &str) -> bool {
    group.len() == 64 && group_documents::parse_doc(&format!("group/{group}/meta")).is_some()
}
pub fn vv(bytes: &[u8]) -> Result<VersionVector, Error> {
    if bytes.len() > VV_BYTES {
        return Err(Error::Protocol("own history frontier bounds".into()));
    }
    let v = VersionVector::decode(bytes)
        .map_err(|_| Error::Protocol("bad own history frontier".into()))?;
    if v.len() > 4096 || v.iter().any(|(_, n)| *n < 0) {
        return Err(Error::Protocol("own history frontier bounds".into()));
    }
    Ok(v)
}
pub fn covers(local: &VersionVector, advertised: &VersionVector) -> bool {
    advertised
        .iter()
        .all(|(p, n)| local.get(p).copied().unwrap_or(0) >= *n)
}
pub fn request_id(message: &SelfMsg) -> u64 {
    match message {
        SelfMsg::GroupWant { request, .. } | SelfMsg::GroupHistoryGet { request, .. } => *request,
        _ => 0,
    }
}
impl History {
    pub fn offer(&mut self, inner: &Arc<SyncRt>, tokens: &[Vec<u8>]) {
        for bytes in tokens.iter().take(group_runtime::own::MAX_OWN_GROUPS) {
            let Ok(token) = cryptography::codec::from_bytes::<InviteToken>(bytes, group_runtime::record::MAX_TOKEN) else {
                continue;
            };
            if token.invite.is_none()
                && valid_group(&token.group)
                && inner.group.group_own_history_context(&token.group)
                && self.scopes.len() < group_runtime::own::MAX_OWN_GROUPS
                && self.scopes.insert(token.group.clone())
            {
                self.enqueue(token.group);
            }
        }
    }
    pub fn enqueue(&mut self, group: String) {
        if self.queued_set.insert(group.clone()) {
            self.queued.push_back(group);
        }
    }
    pub fn changed(&mut self, group: &str) {
        if self.scopes.contains(group) {
            self.enqueue(group.into());
        }
    }
    pub fn next_id(&mut self) -> Result<u64, Error> {
        self.serial = self
            .serial
            .checked_add(1)
            .ok_or_else(|| Error::Protocol("own history request id exhausted".into()))?;
        Ok(self.serial)
    }
    pub fn delay(&self) -> Duration {
        let now = Instant::now();
        let due = if let Some(waiting) = &self.awaiting {
            waiting.sent + RETRY
        } else if self.current.is_some() || !self.queued.is_empty() {
            self.next_issue.unwrap_or(now)
        } else {
            self.last_scan
                .map(|time| time + RESCAN)
                .unwrap_or(now + RESCAN)
        };
        due.saturating_duration_since(now)
    }
    pub async fn pump(
        &mut self,
        inner: &Arc<SyncRt>,
        me: &Arc<Me>,
        remote: &[u8; 32],
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        if !self.enabled {
            return Ok(());
        }
        inner.group.own_group_admit(me.epoch, remote)?;
        let now = Instant::now();
        if let Some(waiting) = self.awaiting.as_mut() {
            if now.duration_since(waiting.sent) >= RETRY {
                writer.send(&waiting.message).await?;
                waiting.sent = now;
            }
            return Ok(());
        }
        if self.next_issue.is_some_and(|time| now < time) {
            return Ok(());
        }
        if self.last_scan.is_none() {
            self.last_scan = Some(now);
        }
        if self.current.is_none()
            && self.queued.is_empty()
            && self
                .last_scan
                .is_none_or(|time| now.duration_since(time) >= RESCAN)
        {
            for group in self.scopes.clone() {
                self.enqueue(group);
            }
            self.last_scan = Some(now);
        }
        // At most one outbound request per service visit. Skip covered entries
        // only within the bounded page; never enqueue a whole retained archive.
        loop {
            if self.current.is_none() {
                let Some(gid) = self.queued.pop_front() else {
                    return Ok(());
                };
                self.queued_set.remove(&gid);
                self.current = Some(self.resumes.remove(&gid).unwrap_or(Scope {
                    gid,
                    meta_done: false,
                    cursor: None,
                    eof: false,
                    page_ready: false,
                }));
            }
            let Some(scope) = self.current.as_ref() else {
                return Err(Error::Protocol("own history scope missing".into()));
            };
            let message = if !scope.meta_done {
                let doc = format!("group/{}/meta", scope.gid);
                let bytes = inner.group.group_own_local_vv(&doc)?.encode();
                if vv(&bytes).is_err() {
                    // A legitimate large frontier needs a scalable protocol;
                    // defer this scope, not account/other-scope sync.
                    self.current = None;
                    continue;
                }
                SelfMsg::GroupWant {
                    request: self.next_id()?,
                    doc,
                    vv: bytes,
                }
            } else if let Some(doc) = self.continue_doc.clone() {
                let bytes = inner.group.group_own_local_vv(&doc)?.encode();
                if vv(&bytes).is_err() {
                    self.continue_doc = None;
                    self.docs.pop_front();
                    continue;
                }
                SelfMsg::GroupWant {
                    request: self.next_id()?,
                    doc,
                    vv: bytes,
                }
            } else if let Some((doc, goal)) = self.docs.pop_front() {
                let held = inner.group.group_own_local_vv(&doc)?;
                if covers(&held, &goal) || vv(&held.encode()).is_err() {
                    continue;
                }
                // Retain this advertised goal until the response finishes the doc.
                self.docs.push_front((doc.clone(), goal));
                SelfMsg::GroupWant {
                    request: self.next_id()?,
                    doc,
                    vv: held.encode(),
                }
            } else if scope.eof {
                self.current = None;
                continue;
            } else if scope.page_ready && !self.queued.is_empty() {
                // Rotate at page boundaries; only one page's goals stay resident.
                let Some(mut scope) = self.current.take() else {
                    return Err(Error::Protocol("own history scope missing".into()));
                };
                scope.page_ready = false;
                let gid = scope.gid.clone();
                self.resumes.insert(gid.clone(), scope);
                self.enqueue(gid);
                continue;
            } else {
                let group = scope.gid.clone();
                let after = scope.cursor.clone();
                SelfMsg::GroupHistoryGet {
                    request: self.next_id()?,
                    group,
                    after,
                }
            };
            inner.group.own_group_admit(me.epoch, remote)?;
            writer.send(&message).await?;
            self.awaiting = Some(Awaiting { message, sent: now });
            self.next_issue = Some(now + Duration::from_millis(25));
            return Ok(());
        }
    }
}

pub async fn handle(
    inner: &Arc<SyncRt>,
    me: &Arc<Me>,
    remote: &[u8; 32],
    peer: &mut PeerChat,
    message: SelfMsg,
    writer: &mut FrameWriter,
) -> Result<(), Error> {
    if !peer.history.enabled {
        return Err(Error::Protocol("own history was not negotiated".into()));
    }
    inner.group.own_group_admit(me.epoch, remote)?;
    match message {
        SelfMsg::GroupHistoryChanged { group } => {
            if !valid_group(&group) {
                return Err(Error::Protocol("bad own history group".into()));
            }
            peer.history.changed(&group);
        }
        SelfMsg::GroupHistoryGet {
            request,
            group,
            after,
        } => {
            if request == 0
                || !valid_group(&group)
                || after
                    .as_ref()
                    .is_some_and(|s| !DocId::Day(s.clone()).is_valid())
            {
                return Err(Error::Protocol("bad own history request".into()));
            }
            let (days, next, deferred) = match inner.group.group_own_history_page(
                me.epoch,
                remote,
                &group,
                after.as_deref(),
                PAGE_DAYS,
            ) {
                Ok(page) if page.days.iter().all(|(_, bytes)| vv(bytes).is_ok()) => {
                    (page.days, page.next, false)
                }
                Ok(_) => (vec![], None, true),
                Err(Error::Protocol(_)) => (vec![], None, true),
                Err(e) => return Err(e),
            };
            inner.group.own_group_admit(me.epoch, remote)?;
            writer
                .send(&SelfMsg::GroupHistoryPage {
                    request,
                    group,
                    after,
                    days,
                    next,
                    deferred,
                })
                .await?;
        }
        SelfMsg::GroupWant {
            request,
            doc,
            vv: bytes,
        } => {
            if request == 0 || group_documents::parse_doc(&doc).is_none() {
                return Err(Error::Protocol("bad own history want".into()));
            }
            let theirs = vv(&bytes)?;
            let limits = BatchLimits {
                bytes: BATCH_BYTES,
                operations: 64,
                writers: 4096,
            };
            let (update, frontier, more, deferred) = match inner
                .group.group_own_export_batch(me.epoch, remote, &doc, &theirs, limits)
            {
                Ok(Some(batch)) => (batch.update, batch.frontier.encode(), batch.more, false),
                Ok(None) => (vec![], VersionVector::default().encode(), false, false),
                Err(Error::Protocol(_)) => (vec![], VersionVector::default().encode(), false, true),
                Err(e) => return Err(e),
            };
            let sig = chat_runtime::chat::wire::sign_batch(&me.profile.device_secret, &doc, &update);
            inner.group.own_group_admit(me.epoch, remote)?;
            writer
                .send(&SelfMsg::GroupBatch {
                    request,
                    doc,
                    vv: frontier,
                    update,
                    sig,
                    more,
                    deferred,
                })
                .await?;
            // Never advance peer.docs based on a planned/sent batch.
        }
        SelfMsg::GroupHistoryPage {
            request,
            group,
            after,
            days,
            next,
            deferred,
        } => {
            let Some(waiting) = peer.history.awaiting.as_ref() else {
                return Ok(());
            };
            if request_id(&waiting.message) != request {
                return Ok(());
            }
            if !matches!(&waiting.message, SelfMsg::GroupHistoryGet { group: g, after: a, .. } if g == &group && a == &after)
            {
                return Err(Error::Protocol("own history page scope mismatch".into()));
            }
            if days.len() > PAGE_DAYS
                || days.iter().map(|(d, v)| d.len() + v.len()).sum::<usize>() > PAGE_BYTES
                || next != days.last().map(|(d, _)| d.clone())
                || (deferred && !days.is_empty())
            {
                return Err(Error::Protocol("own history page bounds".into()));
            }
            let mut previous = after;
            let mut docs = VecDeque::new();
            for (day, bytes) in days {
                if !DocId::Day(day.clone()).is_valid()
                    || previous.as_ref().is_some_and(|p| p >= &day)
                {
                    return Err(Error::Protocol("bad own history page cursor".into()));
                }
                previous = Some(day.clone());
                docs.push_back((format!("group/{group}/{day}"), vv(&bytes)?));
            }
            peer.history.awaiting = None;
            let scope = peer
                .history
                .current
                .as_mut()
                .ok_or_else(|| Error::Protocol("own history page without scope".into()))?;
            scope.eof = next.is_none();
            scope.page_ready = true;
            scope.cursor = next;
            peer.history.docs = docs;
        }
        SelfMsg::GroupBatch {
            request,
            doc,
            vv: bytes,
            update,
            sig,
            more,
            deferred,
        } => {
            let Some(waiting) = peer.history.awaiting.as_ref() else {
                return Ok(());
            };
            if request_id(&waiting.message) != request {
                return Ok(());
            }
            if !matches!(&waiting.message, SelfMsg::GroupWant { doc: name, .. } if name == &doc) {
                return Err(Error::Protocol("own history batch scope mismatch".into()));
            }
            if update.len() > BATCH_BYTES
                || (deferred && (!update.is_empty() || more))
                || (more && update.is_empty())
            {
                return Err(Error::Protocol("own history batch bounds".into()));
            }
            if !chat_sig_ok(remote, &doc, &update, &sig) {
                return Err(Error::Protocol("bad batch signature".into()));
            }
            let planned = vv(&bytes)?;
            if !update.is_empty() {
                // Imports/owed frontiers must persist before the cache receipt.
                let accepted = inner.group.group_own_content_apply(me.epoch, remote, &doc, &update)?;
                if !covers(&vv(&accepted)?, &planned) {
                    return Err(Error::Protocol(
                        "own history batch frontier mismatch".into(),
                    ));
                }
                let sig = chat_runtime::chat::wire::sign_batch(&me.profile.device_secret, &doc, &[]);
                inner.group.own_group_admit(me.epoch, remote)?;
                writer
                    .send(&SelfMsg::Sync {
                        doc: doc.clone(),
                        vv: accepted,
                        update: vec![],
                        sig,
                    })
                    .await?;
            }
            peer.history.awaiting = None;
            peer.history.continue_doc =
                (more && !deferred && !doc.ends_with("/meta")).then(|| doc.clone());
            if !(more && !deferred) {
                let scope = peer
                    .history
                    .current
                    .as_mut()
                    .ok_or_else(|| Error::Protocol("own history batch without scope".into()))?;
                if doc.ends_with("/meta") {
                    scope.meta_done = true;
                } else {
                    peer.history.docs.pop_front();
                }
                // A deferred or stale advertisement is retried on another pass;
                // advancing the cursor does not certify history or host delivery.
            }
        }
        _ => return Err(Error::Protocol("unexpected own history message".into())),
    }
    Ok(())
}
