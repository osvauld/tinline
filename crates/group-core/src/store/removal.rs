//! When this device first learned of each removal (`rm_seen/*`, local clock): the start of the
//! window in which a removed member may still upload its pre-removal posts (U11, D12;
//! `docs/rotation.md`). Never a record's `at`: an author cannot stretch or shrink it.

use storage::Op;

use super::{GroupStore, io};
use crate::Error;

impl GroupStore {
    /// When we first derived `did`'s removal from `gid` (unix ms), if we have.
    pub fn removal_seen(&self, gid: &str, did: &str) -> Result<Option<u64>, Error> {
        let Some(b) = self.db.get(&format!("rm_seen/{gid}/{did}")).map_err(io)? else {
            return Ok(None);
        };
        <[u8; 8]>::try_from(b.as_slice())
            .map(|b| Some(u64::from_be_bytes(b)))
            .map_err(|_| Error::Io("malformed rm_seen row".into()))
    }

    /// Records `now` for `did` (a removal we hold no row for).
    pub fn note_removal_seen(&self, gid: &str, did: &str, now: u64) -> Result<(), Error> {
        self.db
            .put(&format!("rm_seen/{gid}/{did}"), &now.to_be_bytes())
            .map_err(io)
    }

    /// Records `now` for every DID of `removed` not recorded yet, and forgets the rows of DIDs
    /// no longer removed (re-added later: a later removal opens a fresh window).
    pub fn sync_removals_seen(
        &self,
        gid: &str,
        removed: &[String],
        now: u64,
    ) -> Result<(), Error> {
        let p = format!("rm_seen/{gid}/");
        let held: Vec<String> = self
            .db
            .list_prefixed(&p)
            .map_err(io)?
            .into_iter()
            .filter_map(|k| k.strip_prefix(&p).map(str::to_string))
            .collect();
        let mut ops = Vec::new();
        for did in removed.iter().filter(|d| !held.contains(d)) {
            ops.push(
                self.db
                    .put_op(&format!("{p}{did}"), &now.to_be_bytes())
                    .map_err(io)?,
            );
        }
        for did in held.iter().filter(|d| !removed.contains(d)) {
            ops.push(Op::Delete(format!("{p}{did}")));
        }
        if ops.is_empty() {
            return Ok(());
        }
        self.db.apply(&ops).map_err(io)
    }
}

#[cfg(test)]
mod tests;
