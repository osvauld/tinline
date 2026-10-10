//! The call log that own devices sync (`tinline/self/1`, doc `calls`), apart from the account
//! doc: it only grows and is trimmed, while contacts must stay small and arrive first.
//!
//! ```text
//! calls/<call_id>        CallRecord JSON                  one logical record per call
//! ```
//!
//! Same rules as the account doc: the working copy (`History`) is flushed in before every merge,
//! so a missing entry means a local deletion (trimmed, or the contact was removed).

use std::collections::{BTreeMap, HashSet};
use std::ops::Deref;

use loro::LoroValue;

use super::doc::{Doc, entries, io, text};
use crate::Error;
use crate::store::CallRecord;

pub const CALLS_DOC: &str = "calls";
const MAX_HISTORY: usize = 500;

pub struct CallsDoc(Doc);

impl Deref for CallsDoc {
    type Target = Doc;
    fn deref(&self) -> &Doc {
        &self.0
    }
}

/// Which of two records of one call is the better account of it: one that was answered and
/// has a duration beats "answered elsewhere"; ties by content, so every device picks the same.
fn rank(r: &CallRecord) -> (bool, u32, bool, String) {
    let elsewhere = r.reason.ends_with("_elsewhere");
    (!elsewhere, r.duration_secs, !r.missed, serde_json::to_string(r).unwrap_or_default())
}

impl CallsDoc {
    pub fn new(device: &[u8; 32]) -> Self {
        Self(Doc::new(device, CALLS_DOC))
    }

    pub fn load(device: &[u8; 32], snapshot: Option<&[u8]>) -> Result<Self, Error> {
        Doc::load(device, CALLS_DOC, snapshot).map(Self)
    }

    fn records(&self) -> BTreeMap<String, CallRecord> {
        match self.doc.get_map("calls").get_deep_value() {
            LoroValue::Map(m) => m
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), serde_json::from_str(&text(v)?).ok()?)))
                .collect(),
            _ => BTreeMap::new(),
        }
    }

    /// Flushes the working copy into the doc; whether anything changed.
    pub fn write_from(&self, calls: &[CallRecord]) -> Result<bool, Error> {
        let before = self.vv();
        let cm = self.doc.get_map("calls");
        let have = self.records();
        let mine: HashSet<&str> = calls.iter().map(|c| c.call_id.as_str()).collect();
        for r in calls {
            if have.get(&r.call_id).is_none_or(|old| rank(r) > rank(old)) {
                cm.insert(&r.call_id, serde_json::to_string(r).map_err(io)?).map_err(io)?;
            }
        }
        for id in have.keys() {
            if !mine.contains(id.as_str()) {
                cm.delete(id).map_err(io)?;
            }
        }
        self.doc.commit();
        Ok(self.vv() != before)
    }

    /// Merges the doc into the working copy (newest first, trimmed), leaving out calls with
    /// `blocked` contacts (removed on some device); whether the working copy changed.
    pub fn read_into(&self, calls: &mut Vec<CallRecord>, blocked: &HashSet<String>) -> bool {
        let before = serde_json::to_string(&*calls).unwrap_or_default();
        let mut by_id: BTreeMap<String, CallRecord> = calls.drain(..).map(|r| (r.call_id.clone(), r)).collect();
        let root = self.doc.get_deep_value();
        for (id, v) in super::doc::field(&root, "calls").map(entries).unwrap_or_default() {
            let Some(r) = text(v).and_then(|t| serde_json::from_str::<CallRecord>(&t).ok()) else { continue };
            if r.call_id != id {
                continue;
            }
            match by_id.get(&id) {
                Some(mine) if rank(mine) >= rank(&r) => {}
                _ => {
                    by_id.insert(id, r);
                }
            }
        }
        let mut all: Vec<CallRecord> = by_id.into_values().filter(|r| !blocked.contains(&r.peer_did)).collect();
        all.sort_by(|a, b| b.started_at.cmp(&a.started_at).then_with(|| b.call_id.cmp(&a.call_id)));
        all.truncate(MAX_HISTORY);
        *calls = all;
        serde_json::to_string(&*calls).unwrap_or_default() != before
    }
}
