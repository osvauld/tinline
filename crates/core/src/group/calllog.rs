//! The group call log: one local record per group call this device took part in or saw
//! announced and missed (docs/design/group-call-spec.md; playground "Group call · 19 min · 4
//! people" / "Missed group call · 5 min").
//!
//! Fed by the same `GroupCallEvent`s the apps get. `CallLog` is the pure state machine (what
//! is going on now); `Finished` is what one session leaves behind and `merge` folds it into the
//! stored `GroupCallRecord`. Records live in the group vault (`clog/{gid}/{call_id}`), local to
//! this device: the synced 1:1 `CallRecord` doc is untouched, and what this device saw (its
//! own peak, its own join time) is not something another device of the account can know.
//!
//! One record per call: a host move (`Joined` again, a new `host_gen`) neither starts a second
//! session nor a second record; leaving and rejoining the same call adds to the same record.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use super::calls::{CALL_KEEP_MS, GroupCallEventKind};
use super::store::{GroupStore, io, json};
use crate::Error;

/// A group call as the log lists it. Times are unix seconds like `CallRecord`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
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

/// The line the apps show for a record (playground `callText` / `groupWhat`): "Group call · 19
/// min · 4 people", "Missed group call · 5 min" (calm, never an error).
#[uniffi::export]
pub fn group_call_label(r: GroupCallRecord) -> String {
    let s = r.duration_secs;
    let len = if s < 60 { format!("{s} s") } else { format!("{} min", (s + 30) / 60) };
    if r.missed {
        format!("Missed group call \u{b7} {len}")
    } else if r.people > 1 {
        format!("Group call \u{b7} {len} \u{b7} {} people", r.people)
    } else {
        format!("Group call \u{b7} {len}")
    }
}

/// One finished session, to merge into the stored record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finished {
    pub(crate) call_id: String,
    pub(crate) gid: String,
    pub(crate) starter: String,
    pub(crate) started_ms: u64,
    pub(crate) in_call_ms: u64,
    pub(crate) peak: u32,
    pub(crate) missed: bool,
}

#[derive(Debug, Default)]
struct Live {
    gid: String,
    starter: String,
    started_ms: u64,
    joined_ms: Option<u64>,
    peak: u32,
    /// A session of this call was already written.
    recorded: bool,
    seen_ms: u64,
}

#[derive(Debug, Default)]
pub(crate) struct CallLog {
    live: HashMap<String, Live>,
}

impl CallLog {
    /// Folds one event in. `known`: the call's start time (ms) and starter, when the announce
    /// is held. Returns the session that just finished, if any.
    pub(crate) fn apply(&mut self, kind: GroupCallEventKind, gid: &str, call_id: &str, participants: &[String], detail: &str, now_ms: u64, known: Option<(u64, String)>) -> Option<Finished> {
        self.live.retain(|_, l| now_ms.saturating_sub(l.seen_ms) < CALL_KEEP_MS);
        fn entry<'a>(live: &'a mut HashMap<String, Live>, gid: &str, call_id: &str, now_ms: u64, known: &Option<(u64, String)>) -> &'a mut Live {
            let e = live.entry(call_id.to_string()).or_insert_with(|| Live { gid: gid.into(), started_ms: now_ms, ..Default::default() });
            e.seen_ms = now_ms;
            if let Some((ms, who)) = known {
                e.started_ms = *ms;
                e.starter = who.clone();
            }
            e
        }
        match kind {
            GroupCallEventKind::Announced => {
                let e = entry(&mut self.live, gid, call_id, now_ms, &known);
                if known.is_none() && e.starter.is_empty() {
                    e.starter = detail.into();
                }
                None
            }
            GroupCallEventKind::Joined => {
                let e = entry(&mut self.live, gid, call_id, now_ms, &known);
                e.joined_ms.get_or_insert(now_ms);
                e.peak = e.peak.max(1);
                None
            }
            GroupCallEventKind::RosterChanged => {
                let e = entry(&mut self.live, gid, call_id, now_ms, &known);
                if e.joined_ms.is_some() {
                    let n = participants.iter().collect::<BTreeSet<_>>().len() as u32;
                    e.peak = e.peak.max(n).max(1);
                }
                None
            }
            GroupCallEventKind::Left => {
                let e = self.live.get_mut(call_id)?;
                let joined = e.joined_ms.take()?;
                e.recorded = true;
                e.seen_ms = now_ms;
                Some(Finished { call_id: call_id.into(), gid: e.gid.clone(), starter: e.starter.clone(), started_ms: e.started_ms, in_call_ms: now_ms.saturating_sub(joined), peak: e.peak, missed: false })
            }
            GroupCallEventKind::Ended => {
                let e = self.live.remove(call_id)?;
                if let Some(joined) = e.joined_ms {
                    return Some(Finished { call_id: call_id.into(), gid: e.gid, starter: e.starter, started_ms: e.started_ms, in_call_ms: now_ms.saturating_sub(joined), peak: e.peak, missed: false });
                }
                if e.recorded {
                    return None;
                }
                Some(Finished { call_id: call_id.into(), gid: e.gid, starter: e.starter, started_ms: e.started_ms, in_call_ms: now_ms.saturating_sub(e.started_ms), peak: 0, missed: true })
            }
            _ => None,
        }
    }
}

/// The stored record after `f`: a rejoin adds its time and keeps the larger peak.
pub(crate) fn merge(old: Option<GroupCallRecord>, f: &Finished) -> GroupCallRecord {
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
    pub(crate) fn clog_get(&self, gid: &str, call_id: &str) -> Option<GroupCallRecord> {
        serde_json::from_slice(&self.db.get(&format!("clog/{gid}/{call_id}")).ok()??).ok()
    }

    pub(crate) fn clog_put(&self, r: &GroupCallRecord) -> Result<(), Error> {
        self.db.put(&format!("clog/{}/{}", r.group_id, r.call_id), &json(r)).map_err(io)
    }

    /// Newest first; `gid` filters to one group.
    pub(crate) fn clog_list(&self, gid: Option<&str>) -> Result<Vec<GroupCallRecord>, Error> {
        let p = gid.map_or("clog/".to_string(), |g| format!("clog/{g}/"));
        let mut v: Vec<GroupCallRecord> = self.db.scan(&p).map_err(io)?.into_iter().filter_map(|(_, b)| serde_json::from_slice(&b).ok()).collect();
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at).then_with(|| a.call_id.cmp(&b.call_id)));
        Ok(v)
    }

    /// Stores what `f` left and returns the record.
    pub(crate) fn clog_record(&self, f: &Finished) -> Result<GroupCallRecord, Error> {
        let r = merge(self.clog_get(&f.gid, &f.call_id), f);
        self.clog_put(&r)?;
        Ok(r)
    }
}

#[cfg(test)]
mod tests;
