//! Negotiated immutable-proof pulls on self/3. Cursor means inspected inventory,
//! not an acknowledgement, readiness or guaranteed upstream freshness.
use super::*;
use crate::group::{
    attestation::{
        AttestationError,
        persistence::{PROOFS_PER_PAGE, digest, protocol_error, valid_digest},
    },
    record::InviteToken,
};
use std::collections::{BTreeSet, VecDeque};

pub(super) const CAPABILITY: &str = "cap/group-receipts/v1";
const RETRY: Duration = Duration::from_secs(5);
const RESCAN: Duration = Duration::from_secs(60);
const MAX_PROOF: usize = crate::group::attestation::MAX_SIGNED;
const REQUEST_SPACING: Duration = Duration::from_millis(25);
#[derive(Default)]
pub(super) struct Receipts {
    pub enabled: bool,
    scopes: BTreeSet<String>,
    queue: VecDeque<(String, Option<String>)>,
    refresh: BTreeSet<String>,
    waiting: Option<Waiting>,
    serial: u64,
    scan: Option<Instant>,
    issue: Option<Instant>,
}
struct Waiting {
    request: u64,
    group: String,
    after: Option<String>,
    sent: Instant,
}
pub(super) fn supported(docs: &[(String, Vec<u8>)]) -> bool {
    group_history::supported(docs)
        && docs.iter().any(|(name, bytes)| {
            name == CAPABILITY
                && VersionVector::decode(bytes)
                    .ok()
                    .is_some_and(|v| v == VersionVector::default())
        })
}
impl Receipts {
    pub fn offer(&mut self, inner: &Arc<Inner>, tokens: &[Vec<u8>]) {
        for bytes in tokens.iter().take(crate::group::own::MAX_OWN_GROUPS) {
            let Ok(token) = cryptography::codec::from_bytes::<InviteToken>(bytes, crate::group::record::MAX_TOKEN) else {
                continue;
            };
            if token.invite.is_none()
                && group_documents::parse_doc(&format!("group/{}/meta", token.group)).is_some()
                && inner.group.group_own_history_context(&token.group)
                && self.scopes.len() < crate::group::own::MAX_OWN_GROUPS
                && self.scopes.insert(token.group.clone())
            {
                self.queue.push_back((token.group, None));
            }
        }
    }
    pub fn refresh(&mut self, group: &str) {
        if !self.enabled || !self.scopes.contains(group) {
            return;
        }
        if self.waiting.as_ref().is_some_and(|w| w.group == group)
            || self.queue.iter().any(|(g, _)| g == group)
        {
            // Finish the existing pass before rescanning; hot scopes must not
            // repeatedly reset the archive cursor and starve later proof rows.
            self.refresh.insert(group.into());
        } else {
            self.queue.push_back((group.into(), None));
        }
    }
    pub fn delay(&self) -> Duration {
        let now = Instant::now();
        let due = if let Some(w) = &self.waiting {
            w.sent + RETRY
        } else if !self.queue.is_empty() {
            self.issue.unwrap_or(now)
        } else {
            self.scan.map(|s| s + RESCAN).unwrap_or(now + RESCAN)
        };
        due.saturating_duration_since(now)
    }
    pub async fn pump(
        &mut self,
        inner: &Arc<Inner>,
        me: &Arc<Me>,
        remote: &[u8; 32],
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        if !self.enabled {
            return Ok(());
        }
        inner.group.own_group_admit(me.epoch, remote)?;
        let now = Instant::now();
        if let Some(w) = self.waiting.as_mut() {
            if now.duration_since(w.sent) >= RETRY {
                writer
                    .send(&SelfMsg::GroupReceiptGet {
                        request: w.request,
                        group: w.group.clone(),
                        after: w.after.clone(),
                    })
                    .await?;
                w.sent = now;
            }
            return Ok(());
        }
        if self.issue.is_some_and(|time| now < time) {
            return Ok(());
        }
        if self.scan.is_none() {
            self.scan = Some(now);
        }
        if self.queue.is_empty() && self.scan.is_some_and(|s| now.duration_since(s) >= RESCAN) {
            self.queue
                .extend(self.scopes.iter().cloned().map(|g| (g, None)));
            self.scan = Some(now);
        }
        let Some((group, after)) = self.queue.pop_front() else {
            return Ok(());
        };
        self.serial = self
            .serial
            .checked_add(1)
            .ok_or_else(|| Error::Protocol("receipt request ID exhausted".into()))?;
        let request = self.serial;
        inner.group.own_group_admit(me.epoch, remote)?;
        writer
            .send(&SelfMsg::GroupReceiptGet {
                request,
                group: group.clone(),
                after: after.clone(),
            })
            .await?;
        self.waiting = Some(Waiting {
            request,
            group,
            after,
            sent: now,
        });
        self.issue = Some(now + REQUEST_SPACING);
        Ok(())
    }
}

pub(super) async fn handle(
    inner: &Arc<Inner>,
    me: &Arc<Me>,
    remote: &[u8; 32],
    peer: &mut PeerChat,
    message: SelfMsg,
    writer: &mut FrameWriter,
) -> Result<(), Error> {
    if !peer.receipts.enabled {
        return Err(Error::Protocol("group receipts were not negotiated".into()));
    }
    inner.group.own_group_admit(me.epoch, remote)?;
    match message {
        SelfMsg::GroupReceiptGet {
            request,
            group,
            after,
        } => {
            if request == 0
                || group_documents::parse_doc(&format!("group/{group}/meta")).is_none()
                || after.as_deref().is_some_and(|s| !valid_digest(s))
            {
                return Err(Error::Protocol("bad receipt request".into()));
            }
            let proofs = inner.group.own_receipt_page(me.epoch, remote, &group, after.as_deref())?;
            let next = proofs.last().map(|(_, bytes)| digest(bytes));
            inner.group.own_group_admit(me.epoch, remote)?;
            writer
                .send(&SelfMsg::GroupReceiptPage {
                    request,
                    group,
                    after,
                    proofs,
                    next,
                })
                .await?;
        }
        SelfMsg::GroupReceiptPage {
            request,
            group,
            after,
            proofs,
            next,
        } => {
            let Some(w) = peer.receipts.waiting.as_ref() else {
                return Ok(());
            };
            if w.request != request {
                return Ok(());
            }
            if w.group != group || w.after != after {
                return Err(Error::Protocol("receipt response scope mismatch".into()));
            }
            if proofs.len() > PROOFS_PER_PAGE
                || proofs.iter().any(|(_, b)| b.len() > MAX_PROOF)
                || next != proofs.last().map(|(_, b)| digest(b))
                || next
                    .as_ref()
                    .is_some_and(|id| after.as_ref().is_some_and(|old| id <= old))
            {
                return Err(Error::Protocol("receipt page bounds or cursor".into()));
            }
            for (name, bytes) in proofs {
                let (gid, _) = group_documents::parse_doc(&name)
                    .ok_or_else(|| Error::Protocol("bad receipt document".into()))?;
                if gid != group {
                    return Err(Error::Protocol("foreign receipt document".into()));
                }
                match inner.group.own_receipt_accept(me.epoch, remote, &name, &bytes) {
                    Ok(changed) => {
                        if changed {
                            inner.sync_bump();
                        }
                    }
                    // Inspection advances, NOT acceptance. Retry these immutable
                    // rows in later passes after metadata catch-up/regrant. No
                    // pending payload queue or fabricated receipt/Ack is kept.
                    Err(AttestationError::PendingMetadata | AttestationError::Authority) => {}
                    Err(e) => return Err(protocol_error(e).into()),
                }
            }
            inner.group.own_group_admit(me.epoch, remote)?;
            peer.receipts.waiting = None;
            if let Some(id) = next {
                peer.receipts.queue.push_back((group, Some(id)));
            } else if peer.receipts.refresh.remove(&group) {
                peer.receipts.queue.push_back((group, None));
            }
        }
        _ => return Err(Error::Protocol("unexpected receipt message".into())),
    }
    Ok(())
}

#[cfg(test)]
mod faults;
#[cfg(test)]
mod guards;
#[cfg(test)]
mod tests;
