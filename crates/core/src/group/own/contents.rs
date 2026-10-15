//! Capability-negotiated own-device group contents. Not group host authority.
//! Only the self-sync dispatcher may call the Inner methods after transport auth.
use std::sync::Arc;

use loro::VersionVector;

use crate::Error;
use crate::group::doc::batching::{BatchLimits, ExportBatch};
use crate::group::doc::{DocId, GroupDoc, Reject};
use crate::group::engine::{Changed, GroupCore};
use crate::node::Inner;

pub(crate) const CAPABILITY: &str = "cap/group-contents/v1";
// Existing own-device batches use this bound under the 16 MiB frame ceiling.
pub(crate) const MAX_BATCH: usize = 12 * 1024 * 1024;
pub(crate) const HISTORY_PAGE_DAYS: usize = 64;
const HISTORY_PAGE_BYTES: usize = 128 * 1024;

/// A bounded inventory slice, not a completed catch-up or destination receipt.
#[derive(Debug)]
pub(crate) struct OwnHistoryPage {
    pub days: Vec<(String, Vec<u8>)>,
    /// Exclusive cursor for the next request. Only an empty page ends a pass.
    pub next: Option<String>,
}

#[cfg(test)]
mod auth_tests;

pub(crate) fn doc_name(gid: &str, doc: &DocId) -> String {
    match doc {
        DocId::Meta => format!("group/{gid}/meta"),
        DocId::Day(day) => format!("group/{gid}/{day}"),
    }
}

pub(crate) fn parse_doc(name: &str) -> Option<(String, DocId)> {
    let (gid, suffix) = name.strip_prefix("group/")?.split_once('/')?;
    if gid.len() != 64
        || !gid
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let doc = if suffix == "meta" {
        DocId::Meta
    } else {
        DocId::Day(suffix.into())
    };
    doc.is_valid().then(|| (gid.into(), doc))
}

impl GroupCore {
    /// All retained days, oldest first, at most one bounded index page. Source
    /// changes behind the cursor require another pass; this is not a snapshot.
    pub(crate) fn own_history_page(
        &self,
        gid: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<OwnHistoryPage, Error> {
        if limit == 0 || limit > HISTORY_PAGE_DAYS {
            return Err(Error::Protocol("own history page bounds".into()));
        }
        if after.is_some_and(|day| !DocId::Day(day.into()).is_valid()) {
            return Err(Error::Protocol("bad own history cursor".into()));
        }
        if parse_doc(&doc_name(gid, &DocId::Meta)).is_none()
            || !self.groups.lock().contains_key(gid)
        {
            return Err(Error::Protocol("unknown own history group".into()));
        }
        let mut days = Vec::new();
        let mut bytes = 0;
        for day in self.store.days_after(gid, after, limit)? {
            if !DocId::Day(day.clone()).is_valid() {
                return Err(Error::Io("invalid stored group day index".into()));
            }
            let meta = self
                .store
                .shard_meta(gid, &day)?
                .ok_or_else(|| Error::Io("group day index lacks persisted frontier".into()))?;
            VersionVector::decode(&meta.vv)
                .map_err(|_| Error::Io("invalid stored group day frontier".into()))?;
            let size = day.len() + meta.vv.len();
            if bytes + size > HISTORY_PAGE_BYTES {
                if days.is_empty() {
                    return Err(Error::Protocol(
                        "own history frontier exceeds page budget".into(),
                    ));
                }
                break;
            }
            bytes += size;
            days.push((day, meta.vv));
        }
        let next = days.last().map(|(day, _)| day.clone());
        Ok(OwnHistoryPage { days, next })
    }

    /// Meta before recent days, bounded by the caller's shared self-sync budget.
    /// Archived history pagination is a later stage; direct host sync is unchanged.
    pub(crate) fn own_content_docs(&self, limit: usize) -> Vec<(String, Vec<u8>)> {
        if limit == 0 {
            return Vec::new();
        }
        let mut groups: Vec<_> = self
            .groups
            .lock()
            .iter()
            .filter(|(_, live)| {
                live.state.is_member(&self.did) && !live.state.is_removed(&self.did)
            })
            .map(|(gid, live)| (gid.clone(), live.doc.vv().encode()))
            .collect();
        groups.sort_by(|a, b| a.0.cmp(&b.0));
        groups.truncate(super::MAX_OWN_GROUPS);
        let mut out: Vec<_> = groups
            .iter()
            .take(limit)
            .map(|(gid, vv)| (doc_name(gid, &DocId::Meta), vv.clone()))
            .collect();
        // Interleave recent-day ranks across groups instead of filling the first
        // group's archive before offering another group's recent messages.
        let days: Vec<_> = groups
            .iter()
            .map(|(gid, _)| self.held_recent_days(gid))
            .collect();
        for rank in 0..crate::group::wire::HELLO_DAYS as usize {
            for ((gid, _), days) in groups.iter().zip(&days) {
                if out.len() >= limit {
                    return out;
                }
                if let Some(day) = days.get(rank) {
                    let doc = DocId::Day(day.clone());
                    out.push((doc_name(gid, &doc), self.doc_vv(gid, &doc).encode()));
                }
            }
        }
        out
    }

    /// Export only already-persisted original record bytes, never a re-authored diff.
    pub(crate) fn own_content_export(
        &self,
        gid: &str,
        doc: &DocId,
        theirs: &VersionVector,
    ) -> Result<Option<(Vec<u8>, Vec<u8>)>, Error> {
        let Some(accepted) = self.own_persisted_doc(gid, doc)? else {
            return Ok(None);
        };
        let Some(update) = accepted.export_since(theirs) else {
            return Ok(None);
        };
        if update.len() > MAX_BATCH {
            return Err(Error::Protocol(
                "own group export needs bounded history batches".into(),
            ));
        }
        Ok(Some((update, accepted.vv().encode())))
    }

    /// Planning-only bounded export. The returned frontier is NOT a peer Ack.
    /// The self-sync wire dispatcher still uses the legacy whole-diff exporter.
    pub(crate) fn own_content_export_batch(
        &self,
        gid: &str,
        doc: &DocId,
        theirs: &VersionVector,
        limits: BatchLimits,
    ) -> Result<Option<ExportBatch>, Error> {
        let Some(accepted) = self.own_persisted_doc(gid, doc)? else {
            return Ok(None);
        };
        accepted
            .export_batch(theirs, limits)
            .map_err(|e| Error::Protocol(e.to_string()))
    }

    fn own_persisted_doc(&self, gid: &str, doc: &DocId) -> Result<Option<GroupDoc>, Error> {
        if !doc.is_valid() {
            return Err(Error::Protocol("bad own group document".into()));
        }
        if !self.groups.lock().contains_key(gid) {
            return Ok(None);
        }
        let (snapshot, updates) = match doc {
            DocId::Meta => self.store.meta_rows(gid)?,
            DocId::Day(day) => self.store.shard_rows(gid, day)?,
        };
        GroupDoc::load_in(gid, doc.clone(), self.wid, snapshot.as_deref(), &updates)
            .map(Some)
            .map_err(|e| Error::Io(e.to_string()))
    }

    pub(crate) fn own_content_apply(
        &self,
        gid: &str,
        doc: &DocId,
        update: &[u8],
    ) -> Result<Option<Changed>, Error> {
        if update.len() > MAX_BATCH {
            return Err(Error::Protocol("own group batch bounds".into()));
        }
        if !self.groups.lock().contains_key(gid) {
            // Group bootstrap tokens/state must precede contents. An arbitrary
            // caller-selected document name cannot create membership or a group.
            return Err(Error::Protocol("unknown own group document".into()));
        }
        self.apply_own_contents(gid, doc, update)
            .map_err(|e| match e {
                Reject::Persistence(reason) => Error::Io(reason),
                other => Error::Protocol(other.to_string()),
            })
    }
}

impl Inner {
    pub(crate) fn own_group_admit(&self, epoch: u64, remote: &[u8; 32]) -> Result<(), Error> {
        if self.me()?.epoch != epoch {
            return Err(Error::Locked);
        }
        if !self.is_own_device(remote) {
            return Err(Error::Rejected("not a current own device".into()));
        }
        Ok(())
    }

    pub(crate) fn group_own_content_docs(self: &Arc<Self>, limit: usize) -> Vec<(String, Vec<u8>)> {
        self.group_core()
            .map(|core| core.own_content_docs(limit))
            .unwrap_or_default()
    }

    pub(crate) fn group_own_content_export(
        self: &Arc<Self>,
        epoch: u64,
        remote: &[u8; 32],
        name: &str,
        theirs: &VersionVector,
    ) -> Result<Option<(Vec<u8>, Vec<u8>)>, Error> {
        self.own_group_admit(epoch, remote)?;
        let (gid, doc) =
            parse_doc(name).ok_or_else(|| Error::Protocol("bad own group document".into()))?;
        self.group_core()?.own_content_export(&gid, &doc, theirs)
    }

    pub(crate) fn group_own_history_page(
        self: &Arc<Self>,
        epoch: u64,
        remote: &[u8; 32],
        gid: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<OwnHistoryPage, Error> {
        self.own_group_admit(epoch, remote)?;
        self.group_core()?.own_history_page(gid, after, limit)
    }

    pub(crate) fn group_own_export_batch(
        self: &Arc<Self>,
        epoch: u64,
        remote: &[u8; 32],
        name: &str,
        theirs: &VersionVector,
        limits: BatchLimits,
    ) -> Result<Option<ExportBatch>, Error> {
        self.own_group_admit(epoch, remote)?;
        let (gid, doc) =
            parse_doc(name).ok_or_else(|| Error::Protocol("bad own group document".into()))?;
        self.group_core()?
            .own_content_export_batch(&gid, &doc, theirs, limits)
    }

    pub(crate) fn group_own_history_context(self: &Arc<Self>, gid: &str) -> bool {
        self.group_core()
            .is_ok_and(|core| core.groups.lock().contains_key(gid))
    }

    pub(crate) fn group_own_local_vv(self: &Arc<Self>, name: &str) -> Result<VersionVector, Error> {
        let (gid, doc) =
            parse_doc(name).ok_or_else(|| Error::Protocol("bad own group document".into()))?;
        Ok(self.group_core()?.doc_vv(&gid, &doc))
    }

    pub(crate) fn group_own_content_apply(
        self: &Arc<Self>,
        epoch: u64,
        remote: &[u8; 32],
        name: &str,
        update: &[u8],
    ) -> Result<Vec<u8>, Error> {
        self.own_group_admit(epoch, remote)?;
        let (gid, doc) =
            parse_doc(name).ok_or_else(|| Error::Protocol("bad own group document".into()))?;
        let core = self.group_core()?;
        if let Some(changed) = core.own_content_apply(&gid, &doc, update)? {
            self.group_after(&core, changed, None);
        }
        // A local cache receipt only, never an external host/contact receipt.
        Ok(core.doc_vv(&gid, &doc).encode())
    }
}
