//! The group call log: one local record per group call this device took part in or saw
//! announced and missed (docs/design/group-call-spec.md; playground "Group call · 19 min · 4
//! people" / "Missed group call · 5 min").
//!
//! Fed by the same `GroupCallEvent`s the apps get. `CallLog` is the pure state machine (what
//! is going on now); `Finished` is what one session leaves behind and `merge` folds it into the
//! stored `GroupCallRecord`. Records live in the group vault (`clog/{gid}/{call_id}`), local to
//! this device: the synced 1:1 `CallRecord` doc is untouched, and what this device saw (its
//! own peak, its own join time) is not something another device of the account can know.
//! `Finished`, `merge` and the vault rows are `group_core::calllog`, re-exported here.
//!
//! One record per call: a host move (`Joined` again, a new `host_gen`) neither starts a second
//! session nor a second record; leaving and rejoining the same call adds to the same record.

use std::collections::{BTreeSet, HashMap};

pub use group_core::calllog::*;

use super::calls::{CALL_KEEP_MS, GroupCallEventKind};

// The record lives with its vault rows (`group_core::calllog`); this declares it to UniFFI.
/// A group call as the log lists it. Times are unix seconds like `CallRecord`.
#[uniffi::remote(Record)]
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

#[cfg(test)]
mod tests;
