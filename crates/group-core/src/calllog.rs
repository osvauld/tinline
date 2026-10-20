//! The group call log as the group vault keeps it (`clog/{gid}/{call_id}`, local to this
//! device): the stored `GroupCallRecord`, the `Finished` session one call leaves behind and
//! `merge`, which folds a session into the record. The live state machine fed by group-call
//! events and the label the apps show are p2pcore's `group/calllog.rs`.

use serde::{Deserialize, Serialize};

use crate::Error;
use crate::store::{GroupStore, io, json};

/// A group call as the log lists it. Times are unix seconds like `CallRecord`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupCallRecord {
    pub call_id: String,
    pub group_id: String,
    /// DID of whoever started the call (empty when this device never learned it).
    pub started_by: String,
    /// When the call started (announce time), unix seconds.
    pub started_at: u64,
    /// Seconds this device was in the call (joining to leaving); for a missed call, announce to
    /// end.
    pub duration_secs: u32,
    /// The most people (distinct DIDs, this account included) seen in the call at once; 0 for a
    /// missed call, which this device never saw the roster of.
    pub people: u32,
    /// The call ended without this device ever joining it.
    pub missed: bool,
}

/// One finished session, to merge into the stored record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub call_id: String,
    pub gid: String,
    pub starter: String,
    pub started_ms: u64,
    pub in_call_ms: u64,
    pub peak: u32,
    pub missed: bool,
}

/// The stored record after `f`: a rejoin adds its time and keeps the larger peak.
pub fn merge(old: Option<GroupCallRecord>, f: &Finished) -> GroupCallRecord {
    let secs = (f.in_call_ms / 1000).min(u32::MAX as u64) as u32;
    match old {
        Some(mut r) => {
            r.duration_secs = r.duration_secs.saturating_add(secs);
            r.people = r.people.max(f.peak);
            r.missed = r.missed && f.missed;
            if r.started_by.is_empty() {
                r.started_by = f.starter.clone();
            }
            r
        }
        None => GroupCallRecord { call_id: f.call_id.clone(), group_id: f.gid.clone(), started_by: f.starter.clone(), started_at: f.started_ms / 1000, duration_secs: secs, people: f.peak, missed: f.missed },
    }
}

impl GroupStore {
    pub fn clog_get(&self, gid: &str, call_id: &str) -> Option<GroupCallRecord> {
        serde_json::from_slice(&self.db.get(&format!("clog/{gid}/{call_id}")).ok()??).ok()
    }

    pub fn clog_put(&self, r: &GroupCallRecord) -> Result<(), Error> {
        self.db.put(&format!("clog/{}/{}", r.group_id, r.call_id), &json(r)).map_err(io)
    }

    /// Newest first; `gid` filters to one group.
    pub fn clog_list(&self, gid: Option<&str>) -> Result<Vec<GroupCallRecord>, Error> {
        let p = gid.map_or("clog/".to_string(), |g| format!("clog/{g}/"));
        let mut v: Vec<GroupCallRecord> = self.db.scan(&p).map_err(io)?.into_iter().filter_map(|(_, b)| serde_json::from_slice(&b).ok()).collect();
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at).then_with(|| a.call_id.cmp(&b.call_id)));
        Ok(v)
    }

    /// Stores what `f` left and returns the record.
    pub fn clog_record(&self, f: &Finished) -> Result<GroupCallRecord, Error> {
        let r = merge(self.clog_get(&f.gid, &f.call_id), f);
        self.clog_put(&r)?;
        Ok(r)
    }
}

#[cfg(test)]
mod tests;
