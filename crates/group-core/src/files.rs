//! Who holds the file of a file post (`docs/design/group-files.md` §6), derived from the
//! `FileHeld` records in the post's day doc. Pure: the same rows give the same list on every
//! device.
//!
//! - Per `(author DID, device)` the latest `FileHeld` naming the post and its signed hash (by
//!   `at`, then record id) says whether that device holds it; a DID holds it if any of its
//!   devices does. An announcement with another hash counts for nothing.
//! - Only visible records (written while a member, U11) of current members count: a member who
//!   left or was removed drops off at once.
//! - The post's author holds it unless they announced `held: false` (and no device of theirs
//!   says `true` later).

use std::collections::BTreeMap;

use crate::record::{Body, FileRef};
use crate::shard::PostRow;
use crate::state::{GroupState, Vis};

/// The latest holding statement per `(did, dev)` for `post`/`hash` among a day doc's `rows`: `(at, id, held)`.
fn latest(
    rows: &[PostRow],
    post: &str,
    hash: &[u8; 32],
) -> BTreeMap<(String, [u8; 32]), (i64, String, bool)> {
    let mut out: BTreeMap<(String, [u8; 32]), (i64, String, bool)> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.vis == Vis::Visible) {
        let Body::FileHeld {
            post: p,
            hash: h,
            held,
            ..
        } = &r.entry.rec.body
        else {
            continue;
        };
        if p != post || h != hash {
            continue;
        }
        let key = (r.entry.rec.author.clone(), r.entry.rec.dev);
        let cand = (r.entry.rec.at, r.entry.id.clone(), *held);
        match out.get(&key) {
            Some(cur) if (cur.0, &cur.1) >= (cand.0, &cand.1) => {}
            _ => {
                out.insert(key, cand);
            }
        }
    }
    out
}

/// The DIDs holding the file of post `post` (by `author`) now: the author first (unless they
/// let it go), then the others in the order they last said they hold it.
pub fn holders(
    rows: &[PostRow],
    st: &GroupState,
    post: &str,
    author: &str,
    file: &FileRef,
) -> Vec<String> {
    let current = |did: &str| st.is_member(did) && !st.is_removed(did);
    let mut by_did: BTreeMap<String, (bool, i64, bool)> = BTreeMap::new();
    for ((did, _), (at, _, held)) in latest(rows, post, &file.hash) {
        let e = by_did.entry(did).or_insert((false, i64::MIN, false));
        e.0 |= held;
        if held {
            e.1 = e.1.max(at);
        }
        e.2 = true;
    }
    let mut out = Vec::new();
    let author_holds = by_did.get(author).is_none_or(|(held, _, _)| *held);
    if author_holds && current(author) {
        out.push(author.to_string());
    }
    let mut others: Vec<(i64, String)> = by_did
        .into_iter()
        .filter(|(did, (held, _, _))| *held && did != author && current(did))
        .map(|(did, (_, at, _))| (at, did))
        .collect();
    others.sort();
    out.extend(others.into_iter().map(|(_, d)| d));
    out
}

/// What this device (`did`, `dev`) last said about holding the file, if anything.
pub fn own_statement(
    rows: &[PostRow],
    post: &str,
    hash: &[u8; 32],
    did: &str,
    dev: &[u8; 32],
) -> Option<bool> {
    latest(rows, post, hash)
        .get(&(did.to_string(), *dev))
        .map(|(_, _, held)| *held)
}

#[cfg(test)]
mod tests;
