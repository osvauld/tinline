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
mod tests {
    use super::*;
    use GroupCallEventKind as K;

    const G: &str = "gid1";
    const C: &str = "call1";
    fn dids(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("did:key:synthetic{i}")).collect()
    }
    fn ev(l: &mut CallLog, k: K, p: &[String], t: u64) -> Option<Finished> {
        l.apply(k, G, C, p, "did:key:synthetic0", t, None)
    }

    #[test]
    fn joined_then_left_records_duration_and_peak() {
        let mut l = CallLog::default();
        assert!(ev(&mut l, K::Announced, &[], 1_000).is_none());
        ev(&mut l, K::Joined, &[], 2_000);
        ev(&mut l, K::RosterChanged, &dids(2), 3_000);
        ev(&mut l, K::RosterChanged, &dids(4), 4_000);
        ev(&mut l, K::RosterChanged, &dids(3), 5_000);
        let f = ev(&mut l, K::Left, &[], 62_000).unwrap();
        let r = merge(None, &f);
        assert_eq!((r.duration_secs, r.people, r.missed, r.started_at), (60, 4, false, 1));
        assert_eq!(r.started_by, "did:key:synthetic0");
        // The host's End arriving after our Left makes no second record.
        assert!(ev(&mut l, K::Ended, &[], 70_000).is_none());
    }

    #[test]
    fn never_joined_then_ended_is_missed_from_announce() {
        let mut l = CallLog::default();
        ev(&mut l, K::Announced, &[], 10_000);
        let f = ev(&mut l, K::Ended, &[], 310_000).unwrap();
        let r = merge(None, &f);
        assert_eq!((r.duration_secs, r.people, r.missed), (300, 0, true));
        assert!(ev(&mut l, K::Ended, &[], 311_000).is_none());
    }

    #[test]
    fn ended_while_joined_records_once_and_unseen_calls_record_nothing() {
        let mut l = CallLog::default();
        ev(&mut l, K::Announced, &[], 0);
        ev(&mut l, K::Joined, &[], 1_000);
        let f = ev(&mut l, K::Ended, &[], 121_000).unwrap();
        assert_eq!((merge(None, &f).duration_secs, merge(None, &f).missed), (120, false));
        assert!(ev(&mut l, K::Left, &[], 121_500).is_none());
        let mut l = CallLog::default();
        assert!(ev(&mut l, K::Ended, &[], 5).is_none());
        assert!(ev(&mut l, K::Left, &[], 6).is_none());
    }

    #[test]
    fn host_move_is_one_session() {
        let mut l = CallLog::default();
        ev(&mut l, K::Announced, &[], 0);
        ev(&mut l, K::Joined, &[], 1_000);
        ev(&mut l, K::Reconnecting, &[], 20_000);
        ev(&mut l, K::Joined, &[], 22_000); // on the backup host (gen + 1)
        ev(&mut l, K::Announced, &[], 22_000); // takeover re-announce
        let f = ev(&mut l, K::Left, &[], 61_000).unwrap();
        assert_eq!(merge(None, &f).duration_secs, 60);
        assert!(ev(&mut l, K::Ended, &[], 62_000).is_none());
    }

    #[test]
    fn rejoin_adds_to_the_same_record_and_duplicate_dids_count_once() {
        let mut l = CallLog::default();
        ev(&mut l, K::Joined, &[], 0);
        let mut p = dids(2);
        p.push(p[0].clone());
        ev(&mut l, K::RosterChanged, &p, 1);
        let r = merge(None, &ev(&mut l, K::Left, &[], 30_000).unwrap());
        assert_eq!((r.duration_secs, r.people), (30, 2));
        ev(&mut l, K::Joined, &[], 40_000);
        ev(&mut l, K::RosterChanged, &dids(3), 41_000);
        let r = merge(Some(r), &ev(&mut l, K::Left, &[], 70_000).unwrap());
        assert_eq!((r.duration_secs, r.people, r.missed), (60, 3, false));
    }

    #[test]
    fn labels_follow_the_design() {
        let mk = |secs, people, missed| GroupCallRecord { call_id: "c".into(), group_id: "g".into(), started_by: String::new(), started_at: 0, duration_secs: secs, people, missed };
        assert_eq!(group_call_label(mk(19 * 60, 4, false)), "Group call \u{b7} 19 min \u{b7} 4 people");
        assert_eq!(group_call_label(mk(45, 1, false)), "Group call \u{b7} 45 s");
        assert_eq!(group_call_label(mk(300, 0, true)), "Missed group call \u{b7} 5 min");
    }

    #[test]
    fn records_round_trip_through_the_vault_newest_first() {
        let s = GroupStore::in_memory("alpha beta gamma");
        let mk = |id: &str, g: &str, at: u64, missed: bool| Finished { call_id: id.into(), gid: g.into(), starter: "did:key:synthetic0".into(), started_ms: at * 1000, in_call_ms: 90_000, peak: if missed { 0 } else { 3 }, missed };
        s.clog_record(&mk("a", "g1", 100, false)).unwrap();
        s.clog_record(&mk("b", "g1", 300, true)).unwrap();
        s.clog_record(&mk("c", "g2", 200, false)).unwrap();
        // Same call again: merged, not duplicated.
        let again = s.clog_record(&mk("a", "g1", 100, false)).unwrap();
        assert_eq!(again.duration_secs, 180);
        let all = s.clog_list(None).unwrap();
        assert_eq!(all.iter().map(|r| r.call_id.as_str()).collect::<Vec<_>>(), ["b", "c", "a"]);
        assert_eq!(s.clog_list(Some("g1")).unwrap().len(), 2);
        assert!(s.clog_list(Some("g1")).unwrap()[0].missed);
        assert!(s.clog_list(Some("nope")).unwrap().is_empty());
    }
}
